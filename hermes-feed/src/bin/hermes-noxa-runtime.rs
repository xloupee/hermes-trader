use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::fd::FromRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::mpsc::sync_channel;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use alloy_consensus::{Transaction, transaction::SignerRecoverable};
use alloy_primitives::{Address, B256, U256};
use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use futures_util::{StreamExt, TryStreamExt, stream, stream::SplitStream};
use hermes_feed::feed::BroadcastMessage;
use hermes_feed::robinhood::{
    ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256, ACTIVE_NOXA_LAUNCH_FACTORY, DIRECT_FEED_URL,
    NOXA_DEX_ID_UNISWAP, NOXA_FACTORY_RUNTIME_KECCAK256, NOXA_LAUNCH_CONFIG_ID_WETH,
    NOXA_LAUNCH_FACTORY, NOXA_POOL_FEE, PUBLIC_RPC_URL, ROBINHOOD_SWAP_AGGREGATOR, TESTNET_RPC_URL,
    TESTNET_SEQUENCER_URL, UNISWAP_V3_FACTORY, UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    UNISWAP_V3_SWAP_ROUTER_02, WETH,
};
use hermes_feed::{
    ApprovalTransactionPlan, AutomatedPaperRuntime, ConditionalOptions, CopyDecision, CopyPosition,
    FactoryStatus, FeedBoundary, FeedDecoder, Filter, HotPathExecutor, HotPathReport,
    KeystoreTradeSigner, NoxaPredictor, NoxaRpcClient, NoxaVerificationOutcome, ObservedCopySwap,
    ObservedNoxaFactoryCall, PaperOrderState, PredictedNoxaTradeInput, ReceiptLog,
    ReconciliationJob, RiskLimits, RpcMetricsSnapshot, SequenceTracker, SequencerClient,
    SignedPendingKind, SignedPosition, SignedTradingRuntime, TradeSigner, TradeTransactionPlan,
    V3ExactInputIntent, WatchedWalletCopyPolicy, decode_launch_call, decode_launch_header,
    decode_token_launched, decode_v3_exact_input_single, normalize_aggregator_copy_swap,
    predict_v3_pool_address, prepare_predicted_noxa_trade, validate_active_noxa_copy_token,
    verify_noxa_factory_call,
};
use serde::Serialize;
use serde_json::json;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{HeaderValue, Request},
    },
};

const BROADCAST_APPROVAL: &str = "MAINNET_CANARY_APPROVED";
const LIVE_CANARY_AMOUNT_IN_WEI: u64 = 100_000_000_000_000;
const LIVE_CANARY_MAX_WETH_BALANCE_WEI: u64 = 1_000_000_000_000_000;
const LIVE_CANARY_MAX_SLIPPAGE_BPS: u16 = 100;
const KILL_SWITCH_ARMED: &[u8] = b"ARMED\n";
const RECONCILIATION_QUEUE_CAPACITY: usize = 64;
const FEED_RECONNECT_INITIAL_BACKOFF: Duration = Duration::from_millis(250);
const FEED_RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(5);
const NITRO_FEED_CLIENT_VERSION: &str = "2";
const NITRO_FEED_CLIENT_VERSION_HEADER: &str = "Arbitrum-Feed-Client-Version";
const NITRO_REQUESTED_SEQUENCE_HEADER: &str = "Arbitrum-Requested-Sequence-Number";
const ACTIVE_NOXA_DISCOVERY_FROM_L2_BLOCK: u64 = 8_000_000;
const ACTIVE_NOXA_DISCOVERY_LOG_CHUNK: u64 = 25_000;
const ACTIVE_NOXA_DISCOVERY_BACKFILL_CONCURRENCY: usize = 2;

type FeedRead = SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>;

#[derive(Debug, Clone, Copy)]
struct FeedResume {
    reconnect: u64,
    requested_sequence: u64,
    replayed_messages: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedResumeDisposition {
    Replay,
    Exact,
    ForwardGap,
}

impl FeedResume {
    fn observe(&mut self, sequence: u64) -> FeedResumeDisposition {
        if sequence < self.requested_sequence {
            self.replayed_messages = self.replayed_messages.saturating_add(1);
            FeedResumeDisposition::Replay
        } else if sequence == self.requested_sequence {
            FeedResumeDisposition::Exact
        } else {
            FeedResumeDisposition::ForwardGap
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum RuntimeMode {
    Paper,
    Signed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum StrategyMode {
    Launch,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Deployment {
    /// Retired NOXA factory retained for compatibility with existing paper runs.
    LegacyNoxa,
    /// Independently pinned current N0xa factory. Never accepts lookalikes.
    ActiveNoxa,
}

#[derive(Debug, Clone, Parser)]
#[command(version, about = "Feed-driven NOXA paper and signed trading runtime")]
struct Cli {
    #[arg(long, value_enum, default_value_t = RuntimeMode::Paper)]
    mode: RuntimeMode,
    #[arg(long, value_enum, default_value_t = StrategyMode::Launch)]
    strategy: StrategyMode,
    #[arg(long, value_enum, default_value_t = Deployment::LegacyNoxa)]
    deployment: Deployment,
    #[arg(long, default_value = DIRECT_FEED_URL)]
    feed_url: String,
    #[arg(long, default_value = PUBLIC_RPC_URL)]
    rpc_url: String,
    #[arg(long, default_value = "https://sequencer.mainnet.chain.robinhood.com")]
    sequencer_url: String,
    #[arg(long)]
    recipient: String,
    #[arg(long, default_value = "100000000000000")]
    amount_in: String,
    /// Repeatable leader address. Required by --strategy copy.
    #[arg(long = "watch-wallet")]
    watched_wallets: Vec<String>,
    /// Local mode-0600 file containing one leader address per line.
    #[arg(long)]
    watch_wallet_file: Option<PathBuf>,
    /// Paper-only market discovery across all leaders. Never valid in signed mode.
    #[arg(long, default_value_t = false)]
    copy_discover_all_wallets: bool,
    /// Optional repeatable token bootstrap allowlist. New NOXA tokens are learned dynamically.
    #[arg(long = "copy-token")]
    copy_tokens: Vec<String>,
    #[arg(long, default_value = "1000000000000000000")]
    copy_max_leader_entry_amount: String,
    #[arg(long, default_value_t = 2)]
    copy_max_triggers: u64,
    /// Signed copy broadcasts additionally require explicit trust in the leader's calldata limit price.
    #[arg(long, default_value_t = false)]
    copy_trust_leader_limit_price: bool,
    #[arg(long, default_value = "100000000000000")]
    max_trade_amount_in: String,
    #[arg(long, default_value = "100000000000000")]
    max_open_exposure: String,
    /// Maximum WETH the live signer may hold. A signed broadcast fails closed above this balance.
    #[arg(long, default_value = "1000000000000000")]
    max_wallet_weth_balance: String,
    #[arg(long, default_value = "100000000000000")]
    max_gas_cost_wei: String,
    #[arg(long, default_value = "200000000000000")]
    max_session_loss: String,
    #[arg(long, default_value_t = 500)]
    max_slippage_bps: u16,
    #[arg(long, default_value_t = 500)]
    slippage_bps: u16,
    #[arg(long, default_value_t = 350_000)]
    gas_limit: u64,
    #[arg(long, default_value = "20000000")]
    max_fee_per_gas: String,
    #[arg(long, default_value = "0")]
    max_priority_fee_per_gas: String,
    #[arg(long, default_value_t = 3)]
    l1_window: u64,
    #[arg(long, default_value_t = 30)]
    timestamp_window_seconds: u64,
    #[arg(long, default_value_t = 5)]
    warmup_seconds: u64,
    #[arg(long, default_value_t = 300)]
    run_seconds: u64,
    #[arg(long, default_value_t = 25)]
    paper_reconciliation_delay_ms: u64,
    #[arg(long, default_value_t = 20)]
    reconciliation_seconds: u64,
    #[arg(long)]
    keystore: Option<PathBuf>,
    #[arg(long)]
    expected_address: Option<String>,
    #[arg(long, default_value_t = 3)]
    password_fd: i32,
    #[arg(long, default_value_t = false)]
    broadcast: bool,
    #[arg(long)]
    approval_token: Option<String>,
    /// Private mode-0600 file containing exactly `ARMED`; required for live broadcast.
    #[arg(long)]
    kill_switch_file: Option<PathBuf>,
    #[arg(long)]
    round_trip_exit_min_weth_out: Option<String>,
}

enum Engine {
    Paper {
        runtime: Box<AutomatedPaperRuntime>,
        pending_fill: Option<(u64, U256)>,
    },
    Signed {
        runtime: Box<SignedTradingRuntime<KeystoreTradeSigner>>,
        executor: Option<HotPathExecutor>,
    },
}

#[derive(Debug)]
struct LaunchProofMessage {
    source_tx_hash: B256,
    predicted_token: Address,
    predicted_pool: Address,
    result: Result<NoxaVerificationOutcome, String>,
}

#[derive(Debug)]
struct PaperFill {
    order_id: u64,
    actual_amount: U256,
}

#[derive(Debug)]
struct SubmitOutcome {
    tx_hash: B256,
    report: Result<HotPathReport, String>,
}

#[derive(Debug, Clone, Serialize)]
struct CopyTokenValidation {
    token: Address,
    pool: Address,
    validated_l2_block: u64,
    fee: u32,
    #[serde(serialize_with = "serialize_u128_decimal")]
    liquidity: u128,
    #[serde(serialize_with = "serialize_u256_decimal")]
    restriction_end_l1_block: U256,
    token_code_bytes: usize,
    pool_code_bytes: usize,
}

fn serialize_u256_decimal<S>(value: &U256, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&value.to_string())
}

fn serialize_u128_decimal<S>(value: &u128, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&value.to_string())
}

#[derive(Debug)]
struct CopyTokenProofMessage {
    token: Address,
    advertised_pool: Address,
    result: Result<CopyTokenValidation, String>,
}

#[derive(Debug, Default)]
struct CopyTokenRegistry {
    validated: HashMap<Address, Address>,
    observed_launches: HashMap<Address, Address>,
    pending: HashSet<Address>,
    validation_retries: HashMap<Address, ValidationRetry>,
}

#[derive(Debug)]
struct ValidationRetry {
    failures: usize,
    retry_at: Instant,
}

impl CopyTokenRegistry {
    fn observe_launch(&mut self, token: Address, pool: Address) {
        self.observed_launches.insert(token, pool);
    }

    fn launch_was_observed(&self, token: Address, pool: Address) -> bool {
        self.observed_launches
            .get(&token)
            .is_some_and(|observed_pool| *observed_pool == pool)
    }

    fn insert(&mut self, validation: CopyTokenValidation) {
        self.pending.remove(&validation.token);
        self.validation_retries.remove(&validation.token);
        self.validated.insert(validation.token, validation.pool);
    }

    fn insert_verified_launch(&mut self, token: Address, pool: Address) {
        self.pending.remove(&token);
        self.validation_retries.remove(&token);
        self.validated.insert(token, pool);
    }

    fn begin_validation(&mut self, token: Address) -> bool {
        if self.validated.contains_key(&token) || self.pending.contains(&token) {
            return false;
        }
        if self
            .validation_retries
            .get(&token)
            .is_some_and(|retry| Instant::now() < retry.retry_at)
        {
            return false;
        }
        self.pending.insert(token)
    }

    fn defer_failed_validation(&mut self, token: Address) -> Duration {
        const RETRY_BACKOFF_MS: [u64; 8] = [250, 500, 1_000, 2_000, 4_000, 8_000, 16_000, 30_000];
        self.pending.remove(&token);
        let failures = self
            .validation_retries
            .get(&token)
            .map_or(1, |retry| retry.failures.saturating_add(1));
        let delay =
            Duration::from_millis(RETRY_BACKOFF_MS[(failures - 1).min(RETRY_BACKOFF_MS.len() - 1)]);
        self.validation_retries.insert(
            token,
            ValidationRetry {
                failures,
                retry_at: Instant::now() + delay,
            },
        );
        delay
    }

    fn contains(&self, token: Address, pool: Address) -> bool {
        self.validated
            .get(&token)
            .is_some_and(|validated_pool| *validated_pool == pool)
    }
}

#[derive(Debug)]
struct ObservedCopyCandidate {
    swap: ObservedCopySwap,
    pool: Address,
    detected_at: Instant,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
enum ReconciliationOutcome {
    Included {
        tx_hash: B256,
        status: bool,
        l2_block_number: u64,
        gas_cost: U256,
        #[serde(skip)]
        logs: Vec<ReceiptLog>,
    },
    Unresolved {
        tx_hash: B256,
        reason: String,
    },
}

#[derive(Debug, Clone, Copy)]
enum ReconciledSignedAction {
    Entry(SignedPosition),
    Approval(SignedPosition),
    Exit,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args = Arc::new(Cli::parse());
    validate_args(&args)?;
    let recipient = Address::from_str(&args.recipient).context("parse --recipient")?;
    let amount_in = parse_u256(&args.amount_in)?;
    let copy_policy = match args.strategy {
        StrategyMode::Launch => None,
        StrategyMode::Copy => Some(WatchedWalletCopyPolicy::new(
            if args.copy_discover_all_wallets {
                HashSet::from([recipient])
            } else {
                load_watched_wallets(&args)?
            },
            parse_address_set(&args.copy_tokens, "--copy-token")?,
            amount_in,
            parse_u256(&args.copy_max_leader_entry_amount)?,
            args.copy_max_triggers,
        )?),
    };
    let max_fee_per_gas = parse_u128(&args.max_fee_per_gas)?;
    let max_priority_fee_per_gas = parse_u128(&args.max_priority_fee_per_gas)?;
    let limits = RiskLimits {
        max_trade_amount_in: parse_u256(&args.max_trade_amount_in)?,
        max_open_exposure: parse_u256(&args.max_open_exposure)?,
        max_gas_cost_wei: parse_u256(&args.max_gas_cost_wei)?,
        max_session_loss: parse_u256(&args.max_session_loss)?,
        max_slippage_bps: args.max_slippage_bps,
    };
    validate_caps(&args, amount_in, max_fee_per_gas, limits)?;
    ensure_live_broadcast_armed(&args)?;

    let rpc = NoxaRpcClient::with_url(args.rpc_url.clone())?;
    let launch_factory = match args.deployment {
        Deployment::LegacyNoxa => NOXA_LAUNCH_FACTORY,
        Deployment::ActiveNoxa => ACTIVE_NOXA_LAUNCH_FACTORY,
    };
    let expected_factory_runtime = match args.deployment {
        Deployment::LegacyNoxa => NOXA_FACTORY_RUNTIME_KECCAK256,
        Deployment::ActiveNoxa => ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256,
    };
    let status = rpc.factory_status_for(launch_factory).await?;
    if status.runtime_keccak256 != expected_factory_runtime {
        bail!("selected launch factory runtime hash does not match its pinned implementation");
    }
    if args.mode == RuntimeMode::Signed
        && args.broadcast
        && args.strategy == StrategyMode::Launch
        && !status.launch_enabled
    {
        bail!(
            "signed broadcast is disabled while the pinned NOXA factory reports launchEnabled=false"
        );
    }
    let launch_config = rpc
        .launch_config_at_for(
            launch_factory,
            U256::from(NOXA_LAUNCH_CONFIG_ID_WETH),
            status.pinned_l2_block,
        )
        .await?;
    let factory_owner = rpc
        .factory_owner_at_for(launch_factory, status.pinned_l2_block)
        .await?;
    if !rpc
        .code_at_l2_block(factory_owner, status.pinned_l2_block)
        .await?
        .is_empty()
    {
        bail!("NOXA factory owner is a contract; direct feed mutation detection is insufficient");
    }
    let dex_config = match args.deployment {
        Deployment::LegacyNoxa => {
            rpc.dex_config_at(U256::from(NOXA_DEX_ID_UNISWAP), status.pinned_l2_block)
                .await?
        }
        Deployment::ActiveNoxa => {
            rpc.active_dex_config_at_for(
                launch_factory,
                U256::from(NOXA_DEX_ID_UNISWAP),
                status.pinned_l2_block,
            )
            .await?
        }
    };
    if dex_config.factory != UNISWAP_V3_FACTORY
        || dex_config.swap_router != UNISWAP_V3_SWAP_ROUTER_02
        || dex_config.pool_fee != NOXA_POOL_FEE
        || !dex_config.enabled
    {
        bail!("selected Noxa deployment does not use the allowlisted factory and router");
    }
    let launch_runtime = rpc
        .code_at_l2_block(launch_factory, status.pinned_l2_block)
        .await?;
    let dex_runtime = rpc
        .code_at_l2_block(dex_config.factory, status.pinned_l2_block)
        .await?;
    let predictor = Arc::new(match args.deployment {
        Deployment::LegacyNoxa => NoxaPredictor::new(
            launch_factory,
            status.launch_fee,
            launch_config,
            dex_config,
            &launch_runtime,
            &dex_runtime,
        )?,
        Deployment::ActiveNoxa => NoxaPredictor::new_active(
            status.launch_fee,
            launch_config,
            dex_config,
            &launch_runtime,
            &dex_runtime,
        )?,
    });
    let copy_token_validation = match copy_policy.as_ref() {
        Some(policy) => {
            validate_copy_token_allowlist(
                &rpc,
                policy,
                args.deployment,
                predictor.launch_config(),
                status.pinned_l2_block,
            )
            .await?
        }
        None => Vec::new(),
    };
    let mut copy_token_registry = CopyTokenRegistry::default();
    for validation in copy_token_validation.iter().cloned() {
        copy_token_registry.insert(validation);
    }
    let historical_active_noxa_launches = if args.copy_discover_all_wallets {
        backfill_active_noxa_launches(&rpc, status.pinned_l2_block).await?
    } else {
        Vec::new()
    };
    for (token, pool) in historical_active_noxa_launches.iter().copied() {
        copy_token_registry.observe_launch(token, pool);
    }

    let (reconciliation_sender, reconciliation_receiver) =
        sync_channel::<ReconciliationJob>(RECONCILIATION_QUEUE_CAPACITY);
    let (job_sender, mut job_receiver) = mpsc::channel(RECONCILIATION_QUEUE_CAPACITY);
    let bridge = thread::spawn(move || {
        while let Ok(job) = reconciliation_receiver.recv() {
            if job_sender.blocking_send(job).is_err() {
                break;
            }
        }
    });

    let mut engine = match args.mode {
        RuntimeMode::Paper => Engine::Paper {
            runtime: Box::new(AutomatedPaperRuntime::new(0, limits)),
            pending_fill: None,
        },
        RuntimeMode::Signed => {
            let expected = Address::from_str(
                args.expected_address
                    .as_deref()
                    .expect("validated signed expected address"),
            )
            .context("parse --expected-address")?;
            if expected != recipient {
                bail!("--expected-address must equal --recipient");
            }
            let password_fd = args.password_fd;
            // SAFETY: validated descriptor ownership is transferred to this
            // process and closed immediately after keystore loading.
            let password = unsafe { File::from_raw_fd(password_fd) };
            let signer = KeystoreTradeSigner::load_from_reader(
                args.keystore
                    .as_deref()
                    .expect("validated signed keystore path"),
                password,
                expected,
            )?;
            signed_preflight(
                &rpc,
                signer.address(),
                amount_in,
                args.gas_limit,
                max_fee_per_gas,
                parse_u256(&args.max_wallet_weth_balance)?,
                if args.round_trip_exit_min_weth_out.is_some()
                    || args.strategy == StrategyMode::Copy
                {
                    3
                } else {
                    1
                },
            )
            .await?;
            let pending_nonce = rpc.pending_nonce(signer.address()).await?;
            let executor = if args.broadcast {
                Some(HotPathExecutor::new(
                    SequencerClient::with_url(args.sequencer_url.clone())?,
                    reconciliation_sender.clone(),
                ))
            } else {
                None
            };
            Engine::Signed {
                runtime: Box::new(SignedTradingRuntime::new(signer, pending_nonce, limits)),
                executor,
            }
        }
    };

    let rpc_startup_metrics = rpc.metrics();
    emit(json!({
        "record_type": "hermes_noxa_runtime_start",
        "mode": format!("{:?}", args.mode).to_ascii_lowercase(),
        "strategy": format!("{:?}", args.strategy).to_ascii_lowercase(),
        "broadcast": args.broadcast,
        "recipient": recipient,
        "amount_in": amount_in,
        "feed_url": args.feed_url,
        "factory_status": status,
        "factory_owner": factory_owner,
        "factory_owner_is_eoa": true,
        "runtime_hash_matches_pin": true,
        "live_safeguards": {
            "active_noxa_only": args.mode != RuntimeMode::Signed || !args.broadcast || args.deployment == Deployment::ActiveNoxa,
            "allowlisted_factory": launch_factory,
            "allowlisted_router": UNISWAP_V3_SWAP_ROUTER_02,
            "fixed_entry_amount_wei": LIVE_CANARY_AMOUNT_IN_WEI,
            "max_wallet_weth_balance": args.max_wallet_weth_balance,
            "max_slippage_bps": args.max_slippage_bps,
            "kill_switch_required": args.broadcast,
        },
        "launch_enabled": status.launch_enabled,
        "copy_policy": copy_policy.as_ref().map(|policy| json!({
            "watched_wallets": if args.copy_discover_all_wallets { 0 } else { policy.watched_wallets().len() },
            "discover_all_wallets": args.copy_discover_all_wallets,
            "bootstrap_tokens": policy.allowed_tokens().len(),
            "dynamic_token_validation": true,
            "dynamic_token_candidate_source": if args.deployment == Deployment::ActiveNoxa {
                "pinned_factory_log_backfill_and_live_observation"
            } else {
                "watched_swap"
            },
            "historical_factory_launches": historical_active_noxa_launches.len(),
            "max_triggers": args.copy_max_triggers,
            "max_leader_entry_amount": args.copy_max_leader_entry_amount,
            "follower_entry_amount": amount_in,
            "leader_limit_price_trusted_for_broadcast": args.copy_trust_leader_limit_price,
            "validated_tokens": copy_token_validation,
        })),
        "prediction_cache": {
            "launch_config": predictor.launch_config(),
            "dex_config": predictor.dex_config(),
            "token_creation_code_keccak256": predictor.token_creation_code_hash(),
            "pool_init_code_keccak256": predictor.pool_init_code_hash(),
        },
        "boundary_trigger": "contiguous_nitro_l1_header",
        "launch_receipt_on_hot_path": false,
        "private_key_logged": false,
        "rpc_startup": rpc_startup_metrics,
    }))?;

    let (stream, _) = tokio_tungstenite::connect_async(feed_request(&args.feed_url, 0)?)
        .await
        .context("connect Robinhood Nitro feed")?;
    let (_, mut feed_read) = stream.split();
    let deadline = Instant::now() + Duration::from_secs(args.run_seconds);
    let connected_at = Instant::now();
    let mut decoder = FeedDecoder::new(Filter::default());
    let mut sequences = SequenceTracker::default();
    let mut feed_reconnects = 0_u64;
    let mut feed_resume = None;
    let mut feed_replayed_messages = 0_u64;
    let mut feed_resume_forward_gaps = 0_u64;
    let mut last_boundary = None;
    let mut predictor_cache_revalidated = false;
    let mut copy_triggers = 0_u64;
    let verifier_slots = Arc::new(Semaphore::new(16));
    let (proof_sender, mut proof_receiver) = mpsc::channel::<LaunchProofMessage>(32);
    let (copy_token_proof_sender, mut copy_token_proof_receiver) =
        mpsc::channel::<CopyTokenProofMessage>(32);
    let (paper_fill_sender, mut paper_fill_receiver) = mpsc::channel::<PaperFill>(8);
    let (submit_sender, mut submit_receiver) = mpsc::channel::<SubmitOutcome>(8);
    let (reconcile_sender, mut reconcile_receiver) = mpsc::channel::<ReconciliationOutcome>(8);
    let mut tasks = JoinSet::new();
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    let mut shutdown_requested = false;

    let loop_result: Result<()> = async {
        loop {
            tokio::select! {
            biased;
            signal = &mut shutdown => {
                signal?;
                shutdown_requested = true;
                break;
            }
            Some(outcome) = submit_receiver.recv() => {
                handle_submit_outcome(&mut engine, outcome)?;
            }
            Some(outcome) = reconcile_receiver.recv() => {
                if let Some(action) = handle_reconciliation_outcome(&mut engine, outcome)? {
                    arm_reconciled_step(
                        &mut engine,
                        action,
                        last_boundary,
                        recipient,
                        max_fee_per_gas,
                        max_priority_fee_per_gas,
                        &args,
                    )?;
                }
            }
            Some(fill) = paper_fill_receiver.recv() => {
                handle_paper_fill(&mut engine, fill)?;
            }
            Some(proof) = proof_receiver.recv() => {
                if let Some((token, pool)) = handle_launch_proof(proof)? {
                    copy_token_registry.insert_verified_launch(token, pool);
                }
            }
            Some(proof) = copy_token_proof_receiver.recv() => {
                handle_copy_token_proof(&mut copy_token_registry, proof)?;
            }
            Some(job) = job_receiver.recv() => {
                let rpc = rpc.clone();
                let sender = reconcile_sender.clone();
                let timeout = Duration::from_secs(args.reconciliation_seconds);
                tasks.spawn(async move {
                    let outcome = reconcile_job(&rpc, job, timeout).await;
                    let _ = sender.send(outcome).await;
                    Ok::<(), anyhow::Error>(())
                });
            }
            frame = next_feed_frame(&mut feed_read, deadline) => {
                let disconnect = match frame {
                    Ok(Some(Message::Close(reason))) => Some(format!("feed closed: {reason:?}")),
                    Ok(Some(frame)) => {
                        process_feed_frame(
                            frame,
                            &mut decoder,
                            &mut sequences,
                            &mut feed_resume,
                            &mut feed_replayed_messages,
                            &mut feed_resume_forward_gaps,
                            &mut last_boundary,
                            connected_at,
                            &rpc,
                            &predictor,
                            args.deployment,
                            launch_factory,
                            copy_policy.as_ref(),
                            &mut copy_token_registry,
                            &mut copy_triggers,
                            status.launch_enabled,
                            factory_owner,
                            &mut predictor_cache_revalidated,
                            &args,
                            recipient,
                            amount_in,
                            max_fee_per_gas,
                            max_priority_fee_per_gas,
                            &verifier_slots,
                            &proof_sender,
                            &copy_token_proof_sender,
                            &mut engine,
                            &paper_fill_sender,
                            &submit_sender,
                            &mut tasks,
                        ).await?;
                        None
                    }
                    Ok(None) if Instant::now() >= deadline => break,
                    Ok(None) => Some("feed stream ended".to_owned()),
                    Err(error) => Some(error.to_string()),
                };
                if let Some(reason) = disconnect {
                    handle_boundary(
                        &mut engine,
                        FeedBoundary {
                            l1_block_number: u64::MAX,
                            l1_timestamp: u64::MAX,
                            sequence_contiguous: false,
                        },
                        &args,
                        &paper_fill_sender,
                        &submit_sender,
                        &mut tasks,
                    )?;
                    last_boundary = None;
                    feed_reconnects = feed_reconnects.saturating_add(1);
                    let requested_sequence = sequences
                        .current()
                        .last
                        .map_or(0, |last| last.saturating_add(1));
                    let Some(reconnected) = reconnect_feed(
                        &args.feed_url,
                        deadline,
                        feed_reconnects,
                        requested_sequence,
                        &reason,
                        args.broadcast,
                    ).await? else { break };
                    feed_read = reconnected;
                    feed_resume = Some(FeedResume {
                        reconnect: feed_reconnects,
                        requested_sequence,
                        replayed_messages: 0,
                    });
                }
            }
            }
            while let Some(joined) = tasks.try_join_next() {
                joined.context("runtime task panicked")??;
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        Ok(())
    }
    .await;

    // A feed stop cancels any transaction that has not crossed its boundary.
    // Already-released bytes remain leased and are drained below.
    let _ = handle_boundary(
        &mut engine,
        FeedBoundary {
            l1_block_number: u64::MAX,
            l1_timestamp: u64::MAX,
            sequence_contiguous: false,
        },
        &args,
        &paper_fill_sender,
        &submit_sender,
        &mut tasks,
    );
    drain_paper_pending(
        &mut engine,
        Duration::from_millis(args.paper_reconciliation_delay_ms)
            .saturating_add(Duration::from_secs(1)),
        &mut paper_fill_receiver,
        &mut tasks,
    )
    .await?;
    drain_signed_pending(
        &mut engine,
        &rpc,
        Duration::from_secs(args.reconciliation_seconds),
        &mut submit_receiver,
        &mut job_receiver,
        &mut reconcile_receiver,
        &reconcile_sender,
        &mut tasks,
    )
    .await?;

    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    drop(engine);
    drop(reconciliation_sender);
    let _ = bridge.join();
    loop_result?;
    let rpc_total = rpc.metrics();
    emit(json!({
        "record_type": "hermes_noxa_runtime_stop",
        "sequence": sequences.current(),
        "feed_reconnects": feed_reconnects,
        "feed_replayed_messages": feed_replayed_messages,
        "feed_resume_forward_gaps": feed_resume_forward_gaps,
        "copy_triggers": copy_triggers,
        "rpc": rpc_total,
        "rpc_after_startup": rpc_metrics_delta(rpc_total, rpc_startup_metrics),
        "reason": completion_reason(status.launch_enabled, shutdown_requested),
    }))
}

fn completion_reason(launch_enabled: bool, shutdown_requested: bool) -> &'static str {
    match (shutdown_requested, launch_enabled) {
        (true, true) => "shutdown_signal",
        (true, false) => "shutdown_signal_launch_disabled",
        (false, true) => "duration_complete",
        (false, false) => "duration_complete_launch_disabled",
    }
}

#[cfg(unix)]
async fn shutdown_signal() -> Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("install SIGTERM handler")?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.context("install SIGINT handler"),
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() -> Result<()> {
    tokio::signal::ctrl_c()
        .await
        .context("install shutdown handler")
}

async fn revalidate_predictor_cache(
    rpc: &NoxaRpcClient,
    predictor: &NoxaPredictor,
    deployment: Deployment,
    launch_factory: Address,
    expected_launch_enabled: bool,
    expected_factory_owner: Address,
) -> Result<FactoryStatus> {
    let expected_runtime = match deployment {
        Deployment::LegacyNoxa => NOXA_FACTORY_RUNTIME_KECCAK256,
        Deployment::ActiveNoxa => ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256,
    };
    let status = rpc.factory_status_for(launch_factory).await?;
    if status.launch_enabled != expected_launch_enabled
        || status.launch_fee != predictor.launch_fee()
        || status.runtime_keccak256 != expected_runtime
    {
        bail!("NOXA factory state changed during feed warmup; restart to rebuild the predictor");
    }
    let owner = rpc
        .factory_owner_at_for(launch_factory, status.pinned_l2_block)
        .await?;
    if owner != expected_factory_owner
        || !rpc
            .code_at_l2_block(owner, status.pinned_l2_block)
            .await?
            .is_empty()
    {
        bail!("NOXA factory ownership changed during feed warmup; restart required");
    }
    let launch_config = rpc
        .launch_config_at_for(
            launch_factory,
            U256::from(NOXA_LAUNCH_CONFIG_ID_WETH),
            status.pinned_l2_block,
        )
        .await?;
    let dex_config = match deployment {
        Deployment::LegacyNoxa => {
            rpc.dex_config_at(U256::from(NOXA_DEX_ID_UNISWAP), status.pinned_l2_block)
                .await?
        }
        Deployment::ActiveNoxa => {
            rpc.active_dex_config_at_for(
                launch_factory,
                U256::from(NOXA_DEX_ID_UNISWAP),
                status.pinned_l2_block,
            )
            .await?
        }
    };
    if &launch_config != predictor.launch_config() || &dex_config != predictor.dex_config() {
        bail!("NOXA factory configuration changed during feed warmup; restart required");
    }
    let launch_runtime = rpc
        .code_at_l2_block(launch_factory, status.pinned_l2_block)
        .await?;
    let dex_runtime = rpc
        .code_at_l2_block(dex_config.factory, status.pinned_l2_block)
        .await?;
    let refreshed = match deployment {
        Deployment::LegacyNoxa => NoxaPredictor::new(
            launch_factory,
            status.launch_fee,
            launch_config,
            dex_config,
            &launch_runtime,
            &dex_runtime,
        )?,
        Deployment::ActiveNoxa => NoxaPredictor::new_active(
            status.launch_fee,
            launch_config,
            dex_config,
            &launch_runtime,
            &dex_runtime,
        )?,
    };
    if refreshed.token_creation_code_hash() != predictor.token_creation_code_hash()
        || refreshed.pool_init_code_hash() != predictor.pool_init_code_hash()
    {
        bail!("NOXA embedded creation code changed during feed warmup; restart required");
    }
    Ok(status)
}

#[allow(clippy::too_many_arguments)]
async fn process_feed_frame(
    frame: Message,
    decoder: &mut FeedDecoder,
    sequences: &mut SequenceTracker,
    feed_resume: &mut Option<FeedResume>,
    feed_replayed_messages: &mut u64,
    feed_resume_forward_gaps: &mut u64,
    last_boundary: &mut Option<FeedBoundary>,
    connected_at: Instant,
    rpc: &NoxaRpcClient,
    predictor: &Arc<NoxaPredictor>,
    deployment: Deployment,
    launch_factory: Address,
    copy_policy: Option<&WatchedWalletCopyPolicy>,
    copy_token_registry: &mut CopyTokenRegistry,
    copy_triggers: &mut u64,
    launch_enabled: bool,
    factory_owner: Address,
    predictor_cache_revalidated: &mut bool,
    args: &Arc<Cli>,
    recipient: Address,
    amount_in: U256,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    verifier_slots: &Arc<Semaphore>,
    proof_sender: &mpsc::Sender<LaunchProofMessage>,
    copy_token_proof_sender: &mpsc::Sender<CopyTokenProofMessage>,
    engine: &mut Engine,
    paper_fill_sender: &mpsc::Sender<PaperFill>,
    submit_sender: &mpsc::Sender<SubmitOutcome>,
    tasks: &mut JoinSet<Result<()>>,
) -> Result<()> {
    let payload = match frame {
        Message::Text(text) => text.to_string(),
        Message::Binary(bytes) => {
            String::from_utf8(bytes.to_vec()).context("feed binary frame was not UTF-8")?
        }
        Message::Close(_) => bail!("Robinhood feed closed"),
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => return Ok(()),
    };
    let feed: BroadcastMessage = serde_json::from_str(&payload).context("decode Nitro feed")?;
    if feed.version != 1 {
        bail!("unsupported Nitro feed version {}", feed.version);
    }
    if connected_at.elapsed() >= Duration::from_secs(args.warmup_seconds)
        && !*predictor_cache_revalidated
    {
        let refreshed = revalidate_predictor_cache(
            rpc,
            predictor,
            deployment,
            launch_factory,
            launch_enabled,
            factory_owner,
        )
        .await?;
        *predictor_cache_revalidated = true;
        emit(json!({
            "record_type": "runtime_prediction_cache_revalidated",
            "pinned_l2_block": refreshed.pinned_l2_block,
            "pinned_l1_block": refreshed.pinned_l1_block,
            "launch_enabled": refreshed.launch_enabled,
            "launch_fee": refreshed.launch_fee,
            "runtime_keccak256": refreshed.runtime_keccak256,
        }))?;
    }
    for message in &feed.messages {
        let resume_disposition = feed_resume
            .as_mut()
            .map(|resume| resume.observe(message.sequence_number));
        if resume_disposition == Some(FeedResumeDisposition::Replay) {
            *feed_replayed_messages = feed_replayed_messages.saturating_add(1);
            continue;
        }
        if let Some(disposition) = resume_disposition {
            let Some(resume) = feed_resume.take() else {
                bail!("feed resume state disappeared while processing a sequence");
            };
            let outcome = if disposition == FeedResumeDisposition::Exact {
                "exact"
            } else {
                *feed_resume_forward_gaps = feed_resume_forward_gaps.saturating_add(1);
                "forward_gap"
            };
            emit(json!({
                "record_type": "runtime_feed_resume",
                "reconnect": resume.reconnect,
                "requested_sequence_number": resume.requested_sequence,
                "first_processed_sequence_number": message.sequence_number,
                "replayed_messages_skipped": resume.replayed_messages,
                "outcome": outcome,
                "broadcast": args.broadcast,
            }))?;
        }
        let sequence = sequences.observe(message.sequence_number);
        let boundary = FeedBoundary {
            l1_block_number: message.message.message.header.block_number,
            l1_timestamp: message.message.message.header.timestamp,
            sequence_contiguous: sequence.is_contiguous(),
        };
        *last_boundary = Some(boundary);
        handle_boundary(
            engine,
            boundary,
            args,
            paper_fill_sender,
            submit_sender,
            tasks,
        )?;

        let mut launches = Vec::new();
        let mut copy_swaps = Vec::new();
        let mut malformed_watched_copy_swaps = Vec::new();
        let mut copy_signer_error = None;
        let mut launch_signer_error = None;
        let mut non_launch_factory_transaction = None;
        decoder.decode_message_with(message, |context| {
            let tx = context.transaction;
            if tx.to() == Some(launch_factory) {
                if is_non_launch_factory_transaction(tx.input()) {
                    non_launch_factory_transaction = Some(*tx.tx_hash());
                } else if let (Some(intent), Some(header)) = (
                    decode_launch_call(tx.input(), tx.value()),
                    decode_launch_header(tx.input(), tx.value()),
                ) {
                    match tx.recover_signer() {
                        Ok(sender) => launches.push((*tx.tx_hash(), intent, header, sender)),
                        Err(error) => launch_signer_error = Some(error.to_string()),
                    }
                }
            }
            let direct_copy = tx.to() == Some(UNISWAP_V3_SWAP_ROUTER_02)
                && tx.input().get(..4)
                    == Some(hermes_feed::noxa_abi::EXACT_INPUT_SINGLE_SELECTOR.as_slice());
            let aggregator_copy = tx.to() == Some(ROBINHOOD_SWAP_AGGREGATOR)
                && tx.input().get(..4)
                    == Some(hermes_feed::noxa_abi::AGGREGATOR_SWAP_SELECTOR.as_slice());
            if args.strategy == StrategyMode::Copy && (direct_copy || aggregator_copy) {
                match tx.recover_signer() {
                    Ok(from)
                        if args.copy_discover_all_wallets
                            || copy_policy
                                .is_some_and(|policy| policy.watched_wallets().contains(&from)) =>
                    {
                        let normalized = if direct_copy {
                            decode_v3_exact_input_single(tx.input()).and_then(|intent| {
                                let token = copy_token(&intent)?;
                                Some((intent, expected_noxa_pool(token), tx.value()))
                            })
                        } else {
                            normalize_aggregator_copy_swap(tx.input(), tx.value(), from)
                                .ok()
                                .map(|normalized| (normalized.intent, normalized.pool, U256::ZERO))
                        };
                        match normalized {
                            Some((intent, pool, normalized_value)) => {
                                copy_swaps.push(ObservedCopyCandidate {
                                    swap: ObservedCopySwap {
                                        tx_hash: *tx.tx_hash(),
                                        chain_id: tx.chain_id(),
                                        from,
                                        // The follower always uses the pinned direct router. Aggregator
                                        // native-value semantics were checked during normalization.
                                        to: UNISWAP_V3_SWAP_ROUTER_02,
                                        value: normalized_value,
                                        intent,
                                    },
                                    pool,
                                    detected_at: Instant::now(),
                                })
                            }
                            None if !args.copy_discover_all_wallets => {
                                malformed_watched_copy_swaps.push((*tx.tx_hash(), from))
                            }
                            None => {}
                        }
                    }
                    Ok(_) => {}
                    Err(error) => copy_signer_error = Some(error.to_string()),
                }
            }
        })?;
        if let Some(error) = copy_signer_error {
            bail!("could not recover a direct V3 copy candidate signer: {error}");
        }
        if let Some(error) = launch_signer_error {
            bail!("could not recover selected factory launch sender: {error}");
        }
        for (tx_hash, leader) in malformed_watched_copy_swaps {
            emit(json!({
                "record_type": "runtime_copy_candidate_rejected",
                "source_tx_hash": tx_hash,
                "leader": leader,
                "reason": "watched swap calldata is not a supported canonical NOXA route",
            }))?;
        }
        if *predictor_cache_revalidated && let Some(tx_hash) = non_launch_factory_transaction {
            bail!(
                "feed observed non-launch factory transaction {tx_hash}; cached predictor invalidated"
            );
        }
        let emission_enabled = connected_at.elapsed() >= Duration::from_secs(args.warmup_seconds)
            && sequence.is_contiguous();
        // Paper discovery must learn launches replayed during the connection warmup. It still
        // cannot arm an order until `emission_enabled`, and the CLI validator confines
        // `copy_discover_all_wallets` to active-Noxa paper mode.
        let launch_observation_enabled = should_observe_launch(
            emission_enabled,
            args.copy_discover_all_wallets,
            sequence.is_contiguous(),
        );
        for (tx_hash, intent, header, sender) in launches {
            if !launch_observation_enabled {
                emit(json!({
                    "record_type": "runtime_candidate_suppressed",
                    "tx_hash": tx_hash,
                    "sequence": sequence,
                    "warmup": connected_at.elapsed() < Duration::from_secs(args.warmup_seconds),
                }))?;
                continue;
            }
            if !launch_enabled {
                emit(json!({
                    "record_type": "runtime_candidate_suppressed",
                    "tx_hash": tx_hash,
                    "reason": "factory_disabled_at_pinned_startup",
                }))?;
                continue;
            }
            let prediction_started = Instant::now();
            let prediction = match deployment {
                Deployment::LegacyNoxa => predictor.predict(&intent, boundary.l1_block_number),
                Deployment::ActiveNoxa => {
                    predictor.predict_active(&intent, sender, boundary.l1_block_number)
                }
            };
            let predicted = match prediction {
                Ok(predicted) => predicted,
                Err(error) => {
                    emit(json!({
                        "record_type": "runtime_candidate_rejected",
                        "tx_hash": tx_hash,
                        "reason": error.to_string(),
                        "stage": "receipt_free_prediction",
                    }))?;
                    continue;
                }
            };
            if args.strategy == StrategyMode::Launch {
                let quote =
                    match predicted.quote_entry(predictor.launch_config().pair_token, amount_in) {
                        Ok(quote) => quote,
                        Err(error) => {
                            emit(json!({
                                "record_type": "runtime_candidate_rejected",
                                "tx_hash": tx_hash,
                                "reason": error.to_string(),
                                "stage": "receipt_free_quote",
                            }))?;
                            continue;
                        }
                    };
                handle_predicted_candidate(
                    engine,
                    tx_hash,
                    &predicted,
                    quote.amount_out,
                    boundary,
                    recipient,
                    amount_in,
                    max_fee_per_gas,
                    max_priority_fee_per_gas,
                    args,
                    prediction_started,
                )?;
            }

            if deployment == Deployment::ActiveNoxa {
                copy_token_registry.observe_launch(predicted.token, predicted.pool);
                emit(json!({
                    "record_type": "runtime_active_noxa_launch_observed",
                    "tx_hash": tx_hash,
                    "sequence_number": message.sequence_number,
                    "l1_block_number": boundary.l1_block_number,
                    "predicted_token": predicted.token,
                    "predicted_pool": predicted.pool,
                    "prediction_and_optional_arm_ns": prediction_started.elapsed().as_nanos(),
                    "candidate_armed": args.strategy == StrategyMode::Launch,
                    "observed_during_warmup": !emission_enabled,
                    "receipt_verified": false,
                    "receipt_proof": "active_deployment_receipt_proof_pending",
                }))?;
                continue;
            }
            let Ok(permit) = verifier_slots.clone().try_acquire_owned() else {
                emit(json!({
                    "record_type": "runtime_launch_proof_suppressed",
                    "tx_hash": tx_hash,
                    "reason": "proof_capacity",
                }))?;
                continue;
            };
            let rpc = rpc.clone();
            let sender = proof_sender.clone();
            let predicted_token = predicted.token;
            let predicted_pool = predicted.pool;
            let observation = ObservedNoxaFactoryCall {
                tx_hash,
                sequence_number: message.sequence_number,
                feed_l1_block: boundary.l1_block_number,
                feed_l1_timestamp: boundary.l1_timestamp,
                observed_unix_ns: unix_ns(),
                header,
            };
            tasks.spawn(async move {
                let _permit = permit;
                let result = verify_noxa_factory_call(
                    &rpc,
                    observation,
                    amount_in,
                    Some(recipient),
                    Duration::from_secs(5),
                )
                .await
                .map_err(|error| error.to_string());
                let _ = sender
                    .send(LaunchProofMessage {
                        source_tx_hash: tx_hash,
                        predicted_token,
                        predicted_pool,
                        result,
                    })
                    .await;
                Ok(())
            });
            emit(json!({
                "record_type": "runtime_launch_observed",
                "tx_hash": tx_hash,
                "sequence_number": message.sequence_number,
                "l1_block_number": boundary.l1_block_number,
                "predicted_token": predicted.token,
                "predicted_pool": predicted.pool,
                "prediction_and_optional_arm_ns": prediction_started.elapsed().as_nanos(),
                "candidate_armed": args.strategy == StrategyMode::Launch,
                "receipt_verified": false,
            }))?;
        }
        for candidate in copy_swaps {
            let candidate_started = candidate.detected_at;
            let observed = candidate.swap;
            let Some(token) = copy_token(&observed.intent) else {
                continue;
            };
            if args.copy_discover_all_wallets
                && !copy_token_registry.contains(token, candidate.pool)
                && !copy_token_registry.launch_was_observed(token, candidate.pool)
            {
                continue;
            }
            if !emission_enabled {
                emit(json!({
                    "record_type": "runtime_copy_candidate_suppressed",
                    "source_tx_hash": observed.tx_hash,
                    "reason": "feed_warmup_or_sequence_gap",
                }))?;
                continue;
            }
            if !copy_token_registry.contains(token, candidate.pool) {
                let factory_proven_paper_discovery = args.mode == RuntimeMode::Paper
                    && args.deployment == Deployment::ActiveNoxa
                    && args.copy_discover_all_wallets
                    && copy_token_registry.launch_was_observed(token, candidate.pool);
                if deployment == Deployment::ActiveNoxa && !factory_proven_paper_discovery {
                    emit(json!({
                        "record_type": "runtime_copy_candidate_suppressed",
                        "source_tx_hash": observed.tx_hash,
                        "token": token,
                        "pool": candidate.pool,
                        "reason": "active_noxa_launch_not_observed",
                    }))?;
                    continue;
                }
                if copy_token_registry.begin_validation(token) {
                    let rpc = rpc.clone();
                    let sender = copy_token_proof_sender.clone();
                    let advertised_pool = candidate.pool;
                    let launch_config = predictor.launch_config().clone();
                    tasks.spawn(async move {
                        let result = validate_dynamic_copy_token(
                            &rpc,
                            token,
                            advertised_pool,
                            deployment,
                            launch_config,
                        )
                        .await
                        .map_err(|error| error.to_string());
                        let _ = sender
                            .send(CopyTokenProofMessage {
                                token,
                                advertised_pool,
                                result,
                            })
                            .await;
                        Ok(())
                    });
                }
                if factory_proven_paper_discovery {
                    emit(json!({
                        "record_type": "runtime_copy_factory_proven_candidate",
                        "source_tx_hash": observed.tx_hash,
                        "leader": observed.from,
                        "token": token,
                        "pool": candidate.pool,
                        "identity_proof": "pinned_active_noxa_factory_log",
                        "full_rpc_validation_pending": true,
                        "broadcast": false,
                    }))?;
                } else {
                    emit(json!({
                        "record_type": "runtime_copy_candidate_suppressed",
                        "source_tx_hash": observed.tx_hash,
                        "token": token,
                        "pool": candidate.pool,
                        "reason": "dynamic_noxa_validation_pending",
                    }))?;
                    continue;
                }
            }
            let policy = copy_policy
                .ok_or_else(|| anyhow::anyhow!("copy strategy has no validated policy"))?;
            handle_copy_candidate(
                engine,
                policy,
                observed,
                copy_triggers,
                boundary,
                recipient,
                max_fee_per_gas,
                max_priority_fee_per_gas,
                args,
                candidate_started,
            )?;
        }
    }
    Ok(())
}

fn rpc_metrics_delta(
    total: RpcMetricsSnapshot,
    baseline: RpcMetricsSnapshot,
) -> RpcMetricsSnapshot {
    RpcMetricsSnapshot {
        logical_requests: total
            .logical_requests
            .saturating_sub(baseline.logical_requests),
        http_attempts: total.http_attempts.saturating_sub(baseline.http_attempts),
        retries: total.retries.saturating_sub(baseline.retries),
        rate_limited: total.rate_limited.saturating_sub(baseline.rate_limited),
        server_errors: total.server_errors.saturating_sub(baseline.server_errors),
        transport_errors: total
            .transport_errors
            .saturating_sub(baseline.transport_errors),
    }
}

fn active_noxa_discovery_log_ranges(to_l2_block: u64) -> Vec<(u64, u64)> {
    if to_l2_block < ACTIVE_NOXA_DISCOVERY_FROM_L2_BLOCK {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut from = ACTIVE_NOXA_DISCOVERY_FROM_L2_BLOCK;
    loop {
        let to = from
            .saturating_add(ACTIVE_NOXA_DISCOVERY_LOG_CHUNK - 1)
            .min(to_l2_block);
        ranges.push((from, to));
        if to == to_l2_block {
            break;
        }
        from = to + 1;
    }
    ranges
}

async fn backfill_active_noxa_launches(
    rpc: &NoxaRpcClient,
    to_l2_block: u64,
) -> Result<Vec<(Address, Address)>> {
    let batches = stream::iter(active_noxa_discovery_log_ranges(to_l2_block))
        .map(|(from, to)| {
            let rpc = rpc.clone();
            async move {
                rpc.token_launched_logs_for(ACTIVE_NOXA_LAUNCH_FACTORY, from, to)
                    .await
                    .with_context(|| format!("backfill active Noxa launches {from}..={to}"))
            }
        })
        .buffered(ACTIVE_NOXA_DISCOVERY_BACKFILL_CONCURRENCY)
        .try_collect::<Vec<_>>()
        .await?;
    let mut launches = HashMap::<Address, Address>::new();
    for observed in batches.into_iter().flatten() {
        let launch = decode_token_launched(&observed.log)
            .context("active Noxa factory emitted malformed TokenLaunched log")?;
        if let Some(previous) = launches.insert(launch.token, launch.pool)
            && previous != launch.pool
        {
            bail!("active Noxa factory emitted conflicting pools for one token");
        }
    }
    let mut launches = launches.into_iter().collect::<Vec<_>>();
    launches.sort_unstable_by_key(|(token, _)| *token);
    Ok(launches)
}

fn should_observe_launch(
    emission_enabled: bool,
    copy_discover_all_wallets: bool,
    sequence_contiguous: bool,
) -> bool {
    emission_enabled || (copy_discover_all_wallets && sequence_contiguous)
}

fn is_non_launch_factory_transaction(input: &[u8]) -> bool {
    input.get(..4) != Some(hermes_feed::noxa_abi::LAUNCH_TOKEN_SELECTOR.as_slice())
}

#[allow(clippy::too_many_arguments)]
fn handle_predicted_candidate(
    engine: &mut Engine,
    source_tx_hash: B256,
    predicted: &hermes_feed::PredictedNoxaLaunch,
    quoted_amount_out: U256,
    boundary: FeedBoundary,
    recipient: Address,
    amount_in: U256,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    args: &Cli,
    candidate_started: Instant,
) -> Result<()> {
    let nonce = match engine {
        Engine::Paper { runtime, .. } => runtime.snapshot().next_nonce,
        Engine::Signed { runtime, .. } => runtime.snapshot().next_nonce,
    };
    let candidate = match prepare_predicted_noxa_trade(PredictedNoxaTradeInput {
        launch: predicted,
        launch_l1_block: boundary.l1_block_number,
        launch_l1_timestamp: boundary.l1_timestamp,
        recipient,
        amount_in,
        quoted_amount_out,
        slippage_bps: args.slippage_bps,
        nonce,
        gas_limit: args.gas_limit,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        l1_window: args.l1_window,
        timestamp_window_seconds: args.timestamp_window_seconds,
    }) {
        Ok(candidate) => candidate,
        Err(error) => {
            return emit(json!({
                "record_type": "runtime_candidate_rejected",
                "tx_hash": source_tx_hash,
                "reason": error.to_string(),
            }));
        }
    };
    match engine {
        Engine::Paper {
            runtime,
            pending_fill,
        } => match runtime.prepare_entry(
            predicted.token,
            amount_in,
            quoted_amount_out,
            args.gas_limit,
            max_fee_per_gas,
            args.slippage_bps,
            candidate.conditions,
        ) {
            Ok(order) => {
                *pending_fill = Some((order.id, quoted_amount_out));
                emit(json!({
                    "record_type": "runtime_paper_candidate_armed",
                    "source_tx_hash": source_tx_hash,
                    "order": order,
                    "candidate": candidate,
                    "prediction_and_sign_ns": candidate_started.elapsed().as_nanos(),
                    "receipt_verified": false,
                }))
            }
            Err(error) => emit(json!({
                "record_type": "runtime_candidate_rejected",
                "tx_hash": source_tx_hash,
                "reason": error.to_string(),
            })),
        },
        Engine::Signed { runtime, .. } => {
            ensure_live_broadcast_armed(args)?;
            match runtime.arm_trade(&candidate.plan, candidate.conditions, args.slippage_bps) {
                Ok(tx_hash) => emit(json!({
                    "record_type": "runtime_signed_candidate_armed",
                    "source_tx_hash": source_tx_hash,
                    "tx_hash": tx_hash,
                    "token": predicted.token,
                    "conditions": candidate.conditions,
                    "prediction_and_sign_ns": candidate_started.elapsed().as_nanos(),
                    "receipt_verified": false,
                    "raw_transaction_logged": false,
                })),
                Err(error) => emit(json!({
                    "record_type": "runtime_candidate_rejected",
                    "tx_hash": source_tx_hash,
                    "reason": error.to_string(),
                })),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_copy_candidate(
    engine: &mut Engine,
    policy: &WatchedWalletCopyPolicy,
    observed: ObservedCopySwap,
    copy_triggers: &mut u64,
    boundary: FeedBoundary,
    recipient: Address,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    args: &Cli,
    candidate_started: Instant,
) -> Result<()> {
    let observed_token = if observed.intent.token_in == WETH {
        observed.intent.token_out
    } else {
        observed.intent.token_in
    };
    let is_entry = observed.intent.token_in == WETH;
    let follower_position = match engine {
        Engine::Paper { runtime, .. } => {
            let snapshot = runtime.snapshot();
            snapshot
                .positions
                .iter()
                .find(|position| is_entry || position.token == observed_token)
                .map(|position| CopyPosition {
                    token: position.token,
                    token_amount: position.token_amount,
                })
        }
        Engine::Signed { runtime, .. } => {
            let snapshot = runtime.snapshot();
            snapshot
                .positions
                .iter()
                .find(|position| is_entry || position.token == observed_token)
                .map(|position| CopyPosition {
                    token: position.token,
                    token_amount: position.token_amount,
                })
        }
    };
    let decision = match if args.copy_discover_all_wallets {
        policy.evaluate_validated_discovered(&observed, follower_position, *copy_triggers)
    } else {
        policy.evaluate_validated(&observed, follower_position, *copy_triggers)
    } {
        Ok(decision) => decision,
        Err(reason) => {
            return emit(json!({
                "record_type": "runtime_copy_candidate_rejected",
                "source_tx_hash": observed.tx_hash,
                "leader": observed.from,
                "reason": reason,
            }));
        }
    };
    let conditions = ConditionalOptions::first_eligible_window(
        boundary.l1_block_number,
        args.l1_window,
        boundary
            .l1_timestamp
            .checked_add(args.timestamp_window_seconds),
    )
    .ok_or_else(|| anyhow::anyhow!("copy boundary window overflow"))?;
    let armed = match (engine, decision) {
        (
            Engine::Paper {
                runtime,
                pending_fill,
            },
            CopyDecision::Entry {
                leader,
                token,
                follower_amount_in,
                follower_minimum_out,
            },
        ) => match runtime.prepare_entry(
            token,
            follower_amount_in,
            follower_minimum_out,
            args.gas_limit,
            max_fee_per_gas,
            args.slippage_bps,
            conditions,
        ) {
            Ok(order) => {
                *pending_fill = Some((order.id, follower_minimum_out));
                emit(json!({
                    "record_type": "runtime_copy_paper_entry_armed",
                    "source_tx_hash": observed.tx_hash,
                    "leader": leader,
                    "token": token,
                    "follower_amount_in": follower_amount_in,
                    "follower_minimum_out": follower_minimum_out,
                    "paper_fill_basis": "leader_limit_price_floor",
                    "order": order,
                    "detection_to_order_ns": candidate_started.elapsed().as_nanos(),
                }))?;
                true
            }
            Err(error) => {
                emit_copy_runtime_rejection(observed, error.to_string())?;
                false
            }
        },
        (
            Engine::Paper {
                runtime,
                pending_fill,
            },
            CopyDecision::Exit {
                leader,
                token,
                follower_amount_in,
                follower_minimum_out,
            },
        ) => match runtime.prepare_exit(
            token,
            follower_minimum_out,
            args.gas_limit,
            max_fee_per_gas,
            args.slippage_bps,
            conditions,
        ) {
            Ok(order) => {
                *pending_fill = Some((order.id, follower_minimum_out));
                emit(json!({
                    "record_type": "runtime_copy_paper_exit_armed",
                    "source_tx_hash": observed.tx_hash,
                    "leader": leader,
                    "token": token,
                    "follower_amount_in": follower_amount_in,
                    "follower_minimum_out": follower_minimum_out,
                    "paper_fill_basis": "leader_limit_price_floor",
                    "order": order,
                    "detection_to_order_ns": candidate_started.elapsed().as_nanos(),
                }))?;
                true
            }
            Err(error) => {
                emit_copy_runtime_rejection(observed, error.to_string())?;
                false
            }
        },
        (
            Engine::Signed { runtime, .. },
            CopyDecision::Entry {
                leader,
                token,
                follower_amount_in,
                follower_minimum_out,
            },
        ) => {
            ensure_live_broadcast_armed(args)?;
            let plan = TradeTransactionPlan::exact_input(
                runtime.snapshot().next_nonce,
                args.gas_limit,
                max_fee_per_gas,
                max_priority_fee_per_gas,
                &V3ExactInputIntent {
                    token_in: WETH,
                    token_out: token,
                    fee: hermes_feed::robinhood::NOXA_POOL_FEE,
                    recipient,
                    amount_in: follower_amount_in,
                    amount_out_minimum: follower_minimum_out,
                    sqrt_price_limit_x96: U256::ZERO,
                },
            )?;
            match runtime.arm_trade(&plan, conditions, args.slippage_bps) {
                Ok(tx_hash) => {
                    emit(json!({
                        "record_type": "runtime_copy_signed_entry_armed",
                        "source_tx_hash": observed.tx_hash,
                        "leader": leader,
                        "tx_hash": tx_hash,
                        "token": token,
                        "follower_amount_in": follower_amount_in,
                        "follower_minimum_out": follower_minimum_out,
                        "detection_to_order_ns": candidate_started.elapsed().as_nanos(),
                        "raw_transaction_logged": false,
                    }))?;
                    true
                }
                Err(error) => {
                    emit_copy_runtime_rejection(observed, error.to_string())?;
                    false
                }
            }
        }
        (
            Engine::Signed { runtime, .. },
            CopyDecision::Exit {
                leader,
                token,
                follower_amount_in,
                follower_minimum_out,
            },
        ) => {
            ensure_live_broadcast_armed(args)?;
            let plan = TradeTransactionPlan::exact_input(
                runtime.snapshot().next_nonce,
                args.gas_limit,
                max_fee_per_gas,
                max_priority_fee_per_gas,
                &V3ExactInputIntent {
                    token_in: token,
                    token_out: WETH,
                    fee: hermes_feed::robinhood::NOXA_POOL_FEE,
                    recipient,
                    amount_in: follower_amount_in,
                    amount_out_minimum: follower_minimum_out,
                    sqrt_price_limit_x96: U256::ZERO,
                },
            )?;
            match runtime.arm_trade(&plan, conditions, args.slippage_bps) {
                Ok(tx_hash) => {
                    emit(json!({
                        "record_type": "runtime_copy_signed_exit_armed",
                        "source_tx_hash": observed.tx_hash,
                        "leader": leader,
                        "tx_hash": tx_hash,
                        "token": token,
                        "follower_amount_in": follower_amount_in,
                        "follower_minimum_out": follower_minimum_out,
                        "detection_to_order_ns": candidate_started.elapsed().as_nanos(),
                        "raw_transaction_logged": false,
                    }))?;
                    true
                }
                Err(error) => {
                    emit_copy_runtime_rejection(observed, error.to_string())?;
                    false
                }
            }
        }
    };
    if armed {
        *copy_triggers = copy_triggers
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("copy trigger counter overflow"))?;
    }
    Ok(())
}

fn emit_copy_runtime_rejection(observed: ObservedCopySwap, reason: String) -> Result<()> {
    emit(json!({
        "record_type": "runtime_copy_candidate_rejected",
        "source_tx_hash": observed.tx_hash,
        "leader": observed.from,
        "reason": reason,
    }))
}

fn handle_launch_proof(message: LaunchProofMessage) -> Result<Option<(Address, Address)>> {
    match message.result {
        Ok(NoxaVerificationOutcome::Verified(verified)) => {
            let exact = verified.launch.token == message.predicted_token
                && verified.launch.pool == message.predicted_pool;
            emit(json!({
                "record_type": "runtime_launch_proof",
                "source_tx_hash": message.source_tx_hash,
                "status": if exact { "exact_match" } else { "prediction_mismatch" },
                "predicted_token": message.predicted_token,
                "actual_token": verified.launch.token,
                "predicted_pool": message.predicted_pool,
                "actual_pool": verified.launch.pool,
                "receipt_visibility_ns": verified.receipt_visibility_ns,
                "verification_total_ns": verified.verification_total_ns,
            }))?;
            Ok(exact.then_some((verified.launch.token, verified.launch.pool)))
        }
        Ok(NoxaVerificationOutcome::Reverted {
            receipt_visibility_ns,
            ..
        }) => {
            emit(json!({
            "record_type": "runtime_launch_proof",
            "source_tx_hash": message.source_tx_hash,
            "status": "launch_reverted",
            "receipt_visibility_ns": receipt_visibility_ns,
            }))?;
            Ok(None)
        }
        Err(error) => {
            emit(json!({
                "record_type": "runtime_launch_proof",
                "source_tx_hash": message.source_tx_hash,
                "status": "proof_error",
                "error": error,
            }))?;
            Ok(None)
        }
    }
}

fn handle_copy_token_proof(
    registry: &mut CopyTokenRegistry,
    message: CopyTokenProofMessage,
) -> Result<()> {
    match message.result {
        Ok(validation) if validation.pool == message.advertised_pool => {
            emit(json!({
                "record_type": "runtime_copy_token_validated",
                "validation": validation,
                "source": "pinned_rpc_proof",
            }))?;
            registry.insert(validation);
        }
        Ok(validation) => {
            let retry_after = registry.defer_failed_validation(message.token);
            emit(json!({
                "record_type": "runtime_copy_token_rejected",
                "token": message.token,
                "advertised_pool": message.advertised_pool,
                "validated_pool": validation.pool,
                "reason": "pool_mismatch",
                "retry_after_ms": retry_after.as_millis(),
            }))?;
        }
        Err(error) => {
            let retry_after = registry.defer_failed_validation(message.token);
            emit(json!({
                "record_type": "runtime_copy_token_rejected",
                "token": message.token,
                "advertised_pool": message.advertised_pool,
                "reason": error,
                "retry_after_ms": retry_after.as_millis(),
            }))?;
        }
    }
    Ok(())
}

fn handle_boundary(
    engine: &mut Engine,
    boundary: FeedBoundary,
    args: &Cli,
    paper_fill_sender: &mpsc::Sender<PaperFill>,
    submit_sender: &mpsc::Sender<SubmitOutcome>,
    tasks: &mut JoinSet<Result<()>>,
) -> Result<()> {
    match engine {
        Engine::Paper {
            runtime,
            pending_fill,
        } => {
            if runtime.snapshot().pending_order.is_none() {
                return Ok(());
            }
            let event = runtime.observe_boundary(boundary)?;
            emit(json!({
                "record_type": "runtime_paper_boundary",
                "event": event,
            }))?;
            if event.state == PaperOrderState::Submitted {
                if let Some((order_id, actual_amount)) = pending_fill.take() {
                    let sender = paper_fill_sender.clone();
                    let delay = Duration::from_millis(args.paper_reconciliation_delay_ms);
                    tasks.spawn(async move {
                        tokio::time::sleep(delay).await;
                        let _ = sender
                            .send(PaperFill {
                                order_id,
                                actual_amount,
                            })
                            .await;
                        Ok(())
                    });
                }
            } else if event.state == PaperOrderState::Cancelled {
                *pending_fill = None;
            }
        }
        Engine::Signed { runtime, executor } => {
            if runtime.snapshot().pending_tx_hash.is_none() {
                return Ok(());
            }
            let release = runtime.observe_boundary(boundary)?;
            emit(json!({
                "record_type": "runtime_signed_boundary",
                "decision": release.decision,
                "tx_hash": release.tx_hash,
                "nonce": release.nonce,
            }))?;
            if let Some(transaction) = release.transaction {
                let tx_hash = transaction.hash;
                if let Some(executor) = executor.clone() {
                    if let Err(error) = ensure_live_broadcast_armed(args) {
                        runtime.complete_dry_run(tx_hash)?;
                        emit(json!({
                            "record_type": "runtime_live_kill_switch_blocked",
                            "tx_hash": tx_hash,
                            "reason": error.to_string(),
                            "broadcast": false,
                            "raw_transaction_logged": false,
                            "runtime": runtime.snapshot(),
                        }))?;
                        return Ok(());
                    }
                    let sender = submit_sender.clone();
                    tasks.spawn(async move {
                        let report = executor
                            .submit_transaction(transaction)
                            .await
                            .map_err(|error| error.to_string());
                        let _ = sender.send(SubmitOutcome { tx_hash, report }).await;
                        Ok(())
                    });
                } else {
                    runtime.complete_dry_run(tx_hash)?;
                    emit(json!({
                        "record_type": "runtime_signed_dry_run_complete",
                        "tx_hash": tx_hash,
                        "broadcast": false,
                        "raw_transaction_logged": false,
                        "runtime": runtime.snapshot(),
                    }))?;
                }
            }
        }
    }
    Ok(())
}

fn handle_paper_fill(engine: &mut Engine, fill: PaperFill) -> Result<()> {
    let Engine::Paper { runtime, .. } = engine else {
        return Ok(());
    };
    let reconciliation = runtime.reconcile_fill(fill.order_id, fill.actual_amount, U256::ZERO)?;
    emit(json!({
        "record_type": "runtime_paper_reconciliation",
        "reconciliation": reconciliation,
        "runtime": runtime.snapshot(),
    }))
}

fn handle_submit_outcome(engine: &mut Engine, outcome: SubmitOutcome) -> Result<()> {
    let Engine::Signed { runtime, .. } = engine else {
        return Ok(());
    };
    if runtime.snapshot().pending_tx_hash != Some(outcome.tx_hash) {
        return emit(json!({
            "record_type": "runtime_submission_report_after_reconciliation",
            "tx_hash": outcome.tx_hash,
        }));
    }
    match outcome.report {
        Ok(report) => {
            runtime.apply_submission_report(&report)?;
            emit(json!({
                "record_type": "runtime_submission_report",
                "report": {
                    "tx_hash": report.tx_hash,
                    "nonce": report.nonce,
                    "submit_elapsed_ns": report.submit_elapsed.as_nanos(),
                    "result": report.result,
                    "reconciliation_queued": report.reconciliation_queued,
                    "must_halt": report.must_halt(),
                },
                "runtime": runtime.snapshot(),
            }))
        }
        Err(error) => {
            runtime.halt_unresolved(outcome.tx_hash)?;
            emit(json!({
                "record_type": "runtime_submission_internal_error",
                "tx_hash": outcome.tx_hash,
                "error": error,
                "runtime": runtime.snapshot(),
            }))
        }
    }
}

fn handle_reconciliation_outcome(
    engine: &mut Engine,
    outcome: ReconciliationOutcome,
) -> Result<Option<ReconciledSignedAction>> {
    let Engine::Signed { runtime, .. } = engine else {
        return Ok(None);
    };
    let tx_hash = match &outcome {
        ReconciliationOutcome::Included { tx_hash, .. }
        | ReconciliationOutcome::Unresolved { tx_hash, .. } => *tx_hash,
    };
    if runtime.snapshot().pending_tx_hash != Some(tx_hash) {
        emit(json!({
            "record_type": "runtime_duplicate_reconciliation_ignored",
            "tx_hash": tx_hash,
        }))?;
        return Ok(None);
    }
    let pending_kind = runtime.pending_kind();
    let mut action = None;
    match &outcome {
        ReconciliationOutcome::Included {
            tx_hash,
            status,
            gas_cost,
            logs,
            ..
        } => {
            let actual_token_out = if *status {
                runtime
                    .pending_fill_target()
                    .map(|(token, recipient)| extract_erc20_received(logs, token, recipient))
                    .transpose()?
            } else {
                None
            };
            runtime.reconcile_included(*tx_hash, *status, actual_token_out, *gas_cost)?;
            if *status {
                action = match pending_kind {
                    Some(SignedPendingKind::Entry { token }) => runtime
                        .snapshot()
                        .positions
                        .into_iter()
                        .find(|position| position.token == token)
                        .map(ReconciledSignedAction::Entry),
                    Some(SignedPendingKind::Approval { token }) => runtime
                        .snapshot()
                        .positions
                        .into_iter()
                        .find(|position| position.token == token)
                        .map(ReconciledSignedAction::Approval),
                    Some(SignedPendingKind::Exit { .. }) => Some(ReconciledSignedAction::Exit),
                    None => None,
                };
            }
        }
        ReconciliationOutcome::Unresolved { tx_hash, .. } => {
            runtime.halt_unresolved(*tx_hash)?;
        }
    }
    emit(json!({
        "record_type": "runtime_reconciliation",
        "outcome": outcome,
        "runtime": runtime.snapshot(),
    }))?;
    Ok(action)
}

#[allow(clippy::too_many_arguments)]
fn arm_reconciled_step(
    engine: &mut Engine,
    action: ReconciledSignedAction,
    last_boundary: Option<FeedBoundary>,
    recipient: Address,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    args: &Cli,
) -> Result<()> {
    if args.strategy == StrategyMode::Copy {
        return arm_copy_reconciled_step(
            engine,
            action,
            last_boundary,
            recipient,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            args,
        );
    }
    arm_round_trip_step(
        engine,
        action,
        last_boundary,
        recipient,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        args,
    )
}

#[allow(clippy::too_many_arguments)]
fn arm_copy_reconciled_step(
    engine: &mut Engine,
    action: ReconciledSignedAction,
    last_boundary: Option<FeedBoundary>,
    recipient: Address,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    args: &Cli,
) -> Result<()> {
    let Engine::Signed { runtime, .. } = engine else {
        return Ok(());
    };
    ensure_live_broadcast_armed(args)?;
    match action {
        ReconciledSignedAction::Entry(position) => {
            let boundary = last_boundary
                .ok_or_else(|| anyhow::anyhow!("copy approval has no feed boundary"))?;
            let conditions = ConditionalOptions::first_eligible_window(
                boundary.l1_block_number,
                args.l1_window,
                boundary
                    .l1_timestamp
                    .checked_add(args.timestamp_window_seconds),
            )
            .ok_or_else(|| anyhow::anyhow!("copy approval boundary window overflow"))?;
            let plan = ApprovalTransactionPlan::new(
                runtime.snapshot().next_nonce,
                args.gas_limit,
                max_fee_per_gas,
                max_priority_fee_per_gas,
                position.token,
                position.token_amount,
                recipient,
            )?;
            let tx_hash = runtime.arm_approval(&plan, conditions)?;
            emit(json!({
                "record_type": "runtime_copy_exact_exit_approval_armed",
                "tx_hash": tx_hash,
                "token": position.token,
                "amount": position.token_amount,
                "conditions": conditions,
                "raw_transaction_logged": false,
            }))
        }
        ReconciledSignedAction::Approval(position) => emit(json!({
            "record_type": "runtime_copy_position_ready",
            "token": position.token,
            "token_amount": position.token_amount,
            "router_approved": position.router_approved,
            "waiting_for_watched_wallet_exit": true,
        })),
        ReconciledSignedAction::Exit => emit(json!({
            "record_type": "runtime_copy_exit_complete",
            "runtime": runtime.snapshot(),
        })),
    }
}

#[allow(clippy::too_many_arguments)]
fn arm_round_trip_step(
    engine: &mut Engine,
    action: ReconciledSignedAction,
    last_boundary: Option<FeedBoundary>,
    recipient: Address,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    args: &Cli,
) -> Result<()> {
    let Some(minimum_text) = args.round_trip_exit_min_weth_out.as_deref() else {
        return Ok(());
    };
    let Engine::Signed { runtime, .. } = engine else {
        return Ok(());
    };
    ensure_live_broadcast_armed(args)?;
    let boundary =
        last_boundary.ok_or_else(|| anyhow::anyhow!("round-trip has no feed boundary"))?;
    let conditions = ConditionalOptions::first_eligible_window(
        boundary.l1_block_number,
        args.l1_window,
        boundary
            .l1_timestamp
            .checked_add(args.timestamp_window_seconds),
    )
    .ok_or_else(|| anyhow::anyhow!("round-trip boundary window overflow"))?;
    match action {
        ReconciledSignedAction::Entry(position) => {
            let plan = ApprovalTransactionPlan::new(
                runtime.snapshot().next_nonce,
                args.gas_limit,
                max_fee_per_gas,
                max_priority_fee_per_gas,
                position.token,
                position.token_amount,
                recipient,
            )?;
            let tx_hash = runtime.arm_approval(&plan, conditions)?;
            emit(json!({
                "record_type": "runtime_round_trip_approval_armed",
                "tx_hash": tx_hash,
                "token": position.token,
                "amount": position.token_amount,
                "conditions": conditions,
                "raw_transaction_logged": false,
            }))
        }
        ReconciledSignedAction::Approval(position) => {
            let minimum_weth_out = parse_u256(minimum_text)?;
            let loss_floor = position
                .cost_basis
                .checked_mul(U256::from(10_000_u64 - u64::from(args.max_slippage_bps)))
                .and_then(|value| value.checked_div(U256::from(10_000_u64)))
                .ok_or_else(|| anyhow::anyhow!("round-trip loss floor overflow"))?;
            if minimum_weth_out < loss_floor {
                bail!("round-trip minimum WETH output violates the configured loss cap");
            }
            let plan = TradeTransactionPlan::exact_input(
                runtime.snapshot().next_nonce,
                args.gas_limit,
                max_fee_per_gas,
                max_priority_fee_per_gas,
                &V3ExactInputIntent {
                    token_in: position.token,
                    token_out: WETH,
                    fee: hermes_feed::robinhood::NOXA_POOL_FEE,
                    recipient,
                    amount_in: position.token_amount,
                    amount_out_minimum: minimum_weth_out,
                    sqrt_price_limit_x96: U256::ZERO,
                },
            )?;
            let tx_hash = runtime.arm_trade(&plan, conditions, args.slippage_bps)?;
            emit(json!({
                "record_type": "runtime_round_trip_exit_armed",
                "tx_hash": tx_hash,
                "token": position.token,
                "token_amount": position.token_amount,
                "minimum_weth_out": minimum_weth_out,
                "conditions": conditions,
                "raw_transaction_logged": false,
            }))
        }
        ReconciledSignedAction::Exit => emit(json!({
            "record_type": "runtime_round_trip_complete",
            "runtime": runtime.snapshot(),
        })),
    }
}

async fn reconcile_job(
    rpc: &NoxaRpcClient,
    job: ReconciliationJob,
    timeout: Duration,
) -> ReconciliationOutcome {
    let deadline = Instant::now() + timeout;
    loop {
        match rpc.receipt(job.tx_hash).await {
            Ok(Some(receipt)) if receipt.transaction_hash == job.tx_hash => {
                let (Some(gas_used), Some(effective_gas_price)) =
                    (receipt.gas_used, receipt.effective_gas_price)
                else {
                    return ReconciliationOutcome::Unresolved {
                        tx_hash: job.tx_hash,
                        reason: "receipt omitted gas accounting".into(),
                    };
                };
                let Some(gas_cost) = U256::from(gas_used).checked_mul(effective_gas_price) else {
                    return ReconciliationOutcome::Unresolved {
                        tx_hash: job.tx_hash,
                        reason: "receipt gas accounting overflow".into(),
                    };
                };
                return ReconciliationOutcome::Included {
                    tx_hash: job.tx_hash,
                    status: receipt.status,
                    l2_block_number: receipt.l2_block_number,
                    gas_cost,
                    logs: receipt.logs,
                };
            }
            Ok(Some(_)) => {
                return ReconciliationOutcome::Unresolved {
                    tx_hash: job.tx_hash,
                    reason: "receipt hash mismatch".into(),
                };
            }
            Ok(None) => {}
            Err(error) => {
                if Instant::now() >= deadline {
                    return ReconciliationOutcome::Unresolved {
                        tx_hash: job.tx_hash,
                        reason: error.to_string(),
                    };
                }
            }
        }
        if Instant::now() >= deadline {
            return ReconciliationOutcome::Unresolved {
                tx_hash: job.tx_hash,
                reason: "receipt not visible before reconciliation deadline".into(),
            };
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn extract_erc20_received(logs: &[ReceiptLog], token: Address, recipient: Address) -> Result<U256> {
    const TRANSFER_TOPIC: B256 =
        alloy_primitives::b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");
    let mut received = U256::ZERO;
    for log in logs {
        if log.address != token
            || log.topics.len() != 3
            || log.topics[0] != TRANSFER_TOPIC
            || log.topics[2].as_slice()[12..] != *recipient.as_slice()
            || log.data.len() != 32
        {
            continue;
        }
        received = received
            .checked_add(U256::from_be_slice(&log.data))
            .ok_or_else(|| anyhow::anyhow!("ERC-20 receipt fill overflow"))?;
    }
    if received == U256::ZERO {
        bail!("successful swap receipt has no non-zero token transfer to the signer");
    }
    Ok(received)
}

#[allow(clippy::too_many_arguments)]
async fn drain_signed_pending(
    engine: &mut Engine,
    rpc: &NoxaRpcClient,
    timeout: Duration,
    submit_receiver: &mut mpsc::Receiver<SubmitOutcome>,
    job_receiver: &mut mpsc::Receiver<ReconciliationJob>,
    reconcile_receiver: &mut mpsc::Receiver<ReconciliationOutcome>,
    reconcile_sender: &mpsc::Sender<ReconciliationOutcome>,
    tasks: &mut JoinSet<Result<()>>,
) -> Result<()> {
    let drain_deadline = Instant::now() + timeout;
    loop {
        let pending = match engine {
            Engine::Signed { runtime, .. } => runtime.snapshot().pending_tx_hash,
            Engine::Paper { .. } => None,
        };
        let Some(pending_hash) = pending else {
            return Ok(());
        };
        if Instant::now() >= drain_deadline {
            let Engine::Signed { runtime, .. } = engine else {
                return Ok(());
            };
            runtime.halt_unresolved(pending_hash)?;
            emit(json!({
                "record_type": "runtime_shutdown_reconciliation_unresolved",
                "tx_hash": pending_hash,
                "runtime": runtime.snapshot(),
            }))?;
            return Ok(());
        }
        tokio::select! {
            Some(outcome) = submit_receiver.recv() => {
                handle_submit_outcome(engine, outcome)?;
            }
            Some(outcome) = reconcile_receiver.recv() => {
                let _ = handle_reconciliation_outcome(engine, outcome)?;
            }
            Some(job) = job_receiver.recv() => {
                let rpc = rpc.clone();
                let sender = reconcile_sender.clone();
                let remaining = drain_deadline.saturating_duration_since(Instant::now());
                tasks.spawn(async move {
                    let outcome = reconcile_job(&rpc, job, remaining).await;
                    let _ = sender.send(outcome).await;
                    Ok(())
                });
            }
            joined = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(joined) = joined {
                    joined.context("runtime drain task panicked")??;
                }
            }
            _ = tokio::time::sleep_until(drain_deadline.into()) => {}
        }
    }
}

async fn drain_paper_pending(
    engine: &mut Engine,
    timeout: Duration,
    paper_fill_receiver: &mut mpsc::Receiver<PaperFill>,
    tasks: &mut JoinSet<Result<()>>,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let pending_order = match engine {
            Engine::Paper { runtime, .. } => runtime.snapshot().pending_order,
            Engine::Signed { .. } => None,
        };
        let Some(pending_order) = pending_order else {
            return Ok(());
        };
        if pending_order.state != PaperOrderState::Submitted {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let Engine::Paper { runtime, .. } = engine else {
                return Ok(());
            };
            let cancelled = runtime.reconcile_explicit_rejection(pending_order.id)?;
            emit(json!({
                "record_type": "runtime_paper_shutdown_cancelled",
                "order": cancelled,
                "runtime": runtime.snapshot(),
            }))?;
            return Ok(());
        }
        tokio::select! {
            Some(fill) = paper_fill_receiver.recv() => {
                handle_paper_fill(engine, fill)?;
            }
            joined = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(joined) = joined {
                    joined.context("paper drain task panicked")??;
                }
            }
            _ = tokio::time::sleep_until(deadline.into()) => {}
        }
    }
}

async fn next_feed_frame(read: &mut FeedRead, deadline: Instant) -> Result<Option<Message>> {
    match tokio::time::timeout_at(deadline.into(), read.next()).await {
        Ok(Some(frame)) => frame.map(Some).context("read Robinhood feed"),
        Ok(None) | Err(_) => Ok(None),
    }
}

async fn reconnect_feed(
    feed_url: &str,
    deadline: Instant,
    reconnect: u64,
    requested_sequence: u64,
    disconnect_reason: &str,
    broadcast: bool,
) -> Result<Option<FeedRead>> {
    emit(json!({
        "record_type": "runtime_feed_disconnected",
        "reconnect": reconnect,
        "requested_sequence_number": requested_sequence,
        "reason": disconnect_reason,
        "broadcast": broadcast,
    }))?;
    let mut backoff = FEED_RECONNECT_INITIAL_BACKOFF;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Ok(None);
        }
        tokio::time::sleep(backoff.min(deadline.saturating_duration_since(now))).await;
        let request = feed_request(feed_url, requested_sequence)?;
        match tokio::time::timeout_at(deadline.into(), tokio_tungstenite::connect_async(request))
            .await
        {
            Ok(Ok((stream, _))) => {
                emit(json!({
                    "record_type": "runtime_feed_reconnected",
                    "reconnect": reconnect,
                    "requested_sequence_number": requested_sequence,
                    "broadcast": broadcast,
                }))?;
                return Ok(Some(stream.split().1));
            }
            Ok(Err(error)) => {
                emit(json!({
                    "record_type": "runtime_feed_reconnect_error",
                    "reconnect": reconnect,
                    "error": error.to_string(),
                    "broadcast": broadcast,
                }))?;
                backoff = (backoff * 2).min(FEED_RECONNECT_MAX_BACKOFF);
            }
            Err(_) => return Ok(None),
        }
    }
}

fn feed_request(feed_url: &str, requested_sequence: u64) -> Result<Request<()>> {
    let mut request = feed_url
        .into_client_request()
        .context("build Robinhood Nitro feed request")?;
    request.headers_mut().insert(
        NITRO_FEED_CLIENT_VERSION_HEADER,
        HeaderValue::from_static(NITRO_FEED_CLIENT_VERSION),
    );
    request.headers_mut().insert(
        NITRO_REQUESTED_SEQUENCE_HEADER,
        HeaderValue::from_str(&requested_sequence.to_string())
            .context("encode requested Nitro feed sequence")?,
    );
    Ok(request)
}

async fn signed_preflight(
    rpc: &NoxaRpcClient,
    signer: Address,
    amount_in: U256,
    gas_limit: u64,
    max_fee_per_gas: u128,
    max_wallet_weth_balance: U256,
    required_transactions: u64,
) -> Result<()> {
    let (router_code, wrapped_balance, allowance, native_balance) = tokio::try_join!(
        rpc.code_at(UNISWAP_V3_SWAP_ROUTER_02),
        rpc.erc20_balance(WETH, signer),
        rpc.erc20_allowance(WETH, signer, UNISWAP_V3_SWAP_ROUTER_02),
        rpc.native_balance(signer),
    )?;
    if router_code.is_empty() {
        bail!("canonical SwapRouter02 has no bytecode");
    }
    let max_gas_cost = U256::from(gas_limit)
        .checked_mul(U256::from(max_fee_per_gas))
        .and_then(|value| value.checked_mul(U256::from(required_transactions)))
        .ok_or_else(|| anyhow::anyhow!("maximum gas cost overflow"))?;
    validate_signed_funding(
        wrapped_balance,
        allowance,
        native_balance,
        amount_in,
        max_wallet_weth_balance,
        max_gas_cost,
    )
}

fn validate_signed_funding(
    wrapped_balance: U256,
    allowance: U256,
    native_balance: U256,
    amount_in: U256,
    max_wallet_weth_balance: U256,
    max_gas_cost: U256,
) -> Result<()> {
    if wrapped_balance < amount_in {
        bail!("trading signer lacks pre-wrapped WETH for the capped input");
    }
    if wrapped_balance > max_wallet_weth_balance {
        bail!("trading signer WETH balance exceeds the live wallet cap");
    }
    if allowance != amount_in {
        bail!("trading signer must grant the router exactly the capped WETH input");
    }
    if native_balance < max_gas_cost {
        bail!("trading signer lacks native ETH for the complete capped transaction sequence");
    }
    Ok(())
}

async fn validate_copy_token_allowlist(
    rpc: &NoxaRpcClient,
    policy: &WatchedWalletCopyPolicy,
    deployment: Deployment,
    launch_config: &hermes_feed::NoxaLaunchConfig,
    pinned_l2_block: u64,
) -> Result<Vec<CopyTokenValidation>> {
    let mut validated = Vec::with_capacity(policy.allowed_tokens().len());
    for token in policy.allowed_tokens() {
        validated.push(
            match deployment {
                Deployment::LegacyNoxa => {
                    validate_copy_token_at(rpc, *token, expected_noxa_pool(*token), pinned_l2_block)
                        .await
                }
                Deployment::ActiveNoxa => {
                    validate_active_copy_token_at(
                        rpc,
                        *token,
                        expected_noxa_pool(*token),
                        launch_config,
                        pinned_l2_block,
                    )
                    .await
                }
            }
            .with_context(|| format!("validate bootstrap copy token {token}"))?,
        );
    }
    validated.sort_by_key(|entry| entry.token);
    Ok(validated)
}

async fn validate_dynamic_copy_token(
    rpc: &NoxaRpcClient,
    token: Address,
    advertised_pool: Address,
    deployment: Deployment,
    launch_config: hermes_feed::NoxaLaunchConfig,
) -> Result<CopyTokenValidation> {
    if advertised_pool != expected_noxa_pool(token) {
        bail!("aggregator advertised a non-canonical NOXA pool");
    }
    let block = rpc.latest_block().await?;
    match deployment {
        Deployment::LegacyNoxa => {
            validate_copy_token_at(rpc, token, advertised_pool, block.l2_block_number).await
        }
        Deployment::ActiveNoxa => {
            validate_active_copy_token_at(
                rpc,
                token,
                advertised_pool,
                &launch_config,
                block.l2_block_number,
            )
            .await
        }
    }
}

async fn validate_active_copy_token_at(
    rpc: &NoxaRpcClient,
    token: Address,
    expected_pool: Address,
    launch_config: &hermes_feed::NoxaLaunchConfig,
    pinned_l2_block: u64,
) -> Result<CopyTokenValidation> {
    let (record, token_view, token_code, pool_code, pool_snapshot) = tokio::try_join!(
        rpc.active_noxa_launch_record(ACTIVE_NOXA_LAUNCH_FACTORY, token, pinned_l2_block),
        rpc.active_noxa_token_snapshot(token, pinned_l2_block),
        rpc.code_at_l2_block(token, pinned_l2_block),
        rpc.code_at_l2_block(expected_pool, pinned_l2_block),
        rpc.v3_pool_snapshot_at(expected_pool, pinned_l2_block),
    )?;
    if token_code.is_empty() || pool_code.is_empty() {
        bail!("active N0xa token or canonical pool has no bytecode");
    }
    validate_active_noxa_copy_token(
        token,
        &record,
        &token_view,
        &pool_snapshot,
        &pool_code,
        launch_config,
    )?;
    Ok(CopyTokenValidation {
        token,
        pool: expected_pool,
        validated_l2_block: pinned_l2_block,
        fee: pool_snapshot.fee,
        liquidity: pool_snapshot.liquidity,
        restriction_end_l1_block: token_view.restrictions_end_block,
        token_code_bytes: token_code.len(),
        pool_code_bytes: pool_code.len(),
    })
}

async fn validate_copy_token_at(
    rpc: &NoxaRpcClient,
    token: Address,
    expected_pool: Address,
    pinned_l2_block: u64,
) -> Result<CopyTokenValidation> {
    let token_snapshot = rpc
        .token_restriction_snapshot(token, pinned_l2_block, None)
        .await?;
    if token_snapshot.launch_factory != NOXA_LAUNCH_FACTORY
        || token_snapshot.pair_token != WETH
        || token_snapshot.pool_fee != hermes_feed::robinhood::NOXA_POOL_FEE
        || token_snapshot.liquidity_pool != expected_pool
    {
        bail!("token does not report the pinned NOXA deployment");
    }
    let (token_code, pool_code, pool_snapshot) = tokio::try_join!(
        rpc.code_at_l2_block(token, pinned_l2_block),
        rpc.code_at_l2_block(expected_pool, pinned_l2_block),
        rpc.v3_pool_snapshot_at(expected_pool, pinned_l2_block),
    )?;
    let pair_matches = (pool_snapshot.token0 == WETH && pool_snapshot.token1 == token)
        || (pool_snapshot.token0 == token && pool_snapshot.token1 == WETH);
    if token_code.is_empty()
        || pool_code.is_empty()
        || !pair_matches
        || pool_snapshot.fee != hermes_feed::robinhood::NOXA_POOL_FEE
        || pool_snapshot.liquidity == 0
    {
        bail!("token has invalid bytecode, pool identity, fee, or liquidity");
    }
    Ok(CopyTokenValidation {
        token,
        pool: expected_pool,
        validated_l2_block: pinned_l2_block,
        fee: pool_snapshot.fee,
        liquidity: pool_snapshot.liquidity,
        restriction_end_l1_block: token_snapshot.restriction_end_block,
        token_code_bytes: token_code.len(),
        pool_code_bytes: pool_code.len(),
    })
}

fn copy_token(intent: &V3ExactInputIntent) -> Option<Address> {
    if intent.token_in == WETH && intent.token_out != Address::ZERO && intent.token_out != WETH {
        Some(intent.token_out)
    } else if intent.token_out == WETH
        && intent.token_in != Address::ZERO
        && intent.token_in != WETH
    {
        Some(intent.token_in)
    } else {
        None
    }
}

fn expected_noxa_pool(token: Address) -> Address {
    let (token0, token1) = if token < WETH {
        (token, WETH)
    } else {
        (WETH, token)
    };
    predict_v3_pool_address(
        UNISWAP_V3_FACTORY,
        token0,
        token1,
        hermes_feed::robinhood::NOXA_POOL_FEE,
        UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    )
}

fn validate_args(args: &Cli) -> Result<()> {
    if args.run_seconds == 0
        || args.l1_window == 0
        || args.timestamp_window_seconds == 0
        || args.reconciliation_seconds == 0
    {
        bail!("runtime durations and condition windows must be non-zero");
    }
    if args.slippage_bps > args.max_slippage_bps || args.max_slippage_bps >= 10_000 {
        bail!("slippage cap is invalid");
    }
    match args.strategy {
        StrategyMode::Launch => {
            if !args.watched_wallets.is_empty()
                || args.watch_wallet_file.is_some()
                || !args.copy_tokens.is_empty()
                || args.copy_trust_leader_limit_price
                || args.copy_discover_all_wallets
            {
                bail!("copy allowlists require --strategy copy");
            }
        }
        StrategyMode::Copy => {
            if args.copy_discover_all_wallets
                && (!args.watched_wallets.is_empty() || args.watch_wallet_file.is_some())
            {
                bail!("paper discovery cannot be combined with a wallet allowlist");
            }
            if !args.copy_discover_all_wallets
                && args.watched_wallets.is_empty()
                && args.watch_wallet_file.is_none()
            {
                bail!("copy strategy requires --watch-wallet or --watch-wallet-file");
            }
            if args.copy_discover_all_wallets && args.mode != RuntimeMode::Paper {
                bail!("all-wallet copy discovery is restricted to paper mode");
            }
            if args.copy_discover_all_wallets && args.deployment != Deployment::ActiveNoxa {
                bail!("all-wallet copy discovery requires the pinned active Noxa deployment");
            }
            if args.copy_max_triggers == 0 {
                bail!("copy trigger cap must be non-zero");
            }
            if args.round_trip_exit_min_weth_out.is_some() {
                bail!("copy strategy waits for watched-wallet exits and refuses timed round trips");
            }
            if args.mode == RuntimeMode::Signed
                && args.broadcast
                && !args.copy_trust_leader_limit_price
            {
                bail!(
                    "signed copy broadcast requires --copy-trust-leader-limit-price because the hot path does not perform an RPC quote"
                );
            }
        }
    }
    match args.mode {
        RuntimeMode::Paper => {
            if args.broadcast
                || args.keystore.is_some()
                || args.expected_address.is_some()
                || args.kill_switch_file.is_some()
                || args.round_trip_exit_min_weth_out.is_some()
            {
                bail!("paper mode refuses broadcast and keystore arguments");
            }
        }
        RuntimeMode::Signed => {
            if args.keystore.is_none() || args.expected_address.is_none() || args.password_fd < 3 {
                bail!("signed mode requires keystore, expected address, and password FD >= 3");
            }
            if args.broadcast && args.approval_token.as_deref() != Some(BROADCAST_APPROVAL) {
                bail!("broadcast requires the explicit mainnet canary approval token");
            }
            if args.broadcast {
                if args.deployment != Deployment::ActiveNoxa {
                    bail!(
                        "signed broadcast is restricted to the allowlisted active Noxa deployment"
                    );
                }
                if args.kill_switch_file.is_none() {
                    bail!("signed broadcast requires --kill-switch-file");
                }
                if parse_u256(&args.amount_in)? != U256::from(LIVE_CANARY_AMOUNT_IN_WEI)
                    || parse_u256(&args.max_trade_amount_in)?
                        != U256::from(LIVE_CANARY_AMOUNT_IN_WEI)
                    || parse_u256(&args.max_open_exposure)? != U256::from(LIVE_CANARY_AMOUNT_IN_WEI)
                {
                    bail!(
                        "signed broadcast requires the exact tiny live-canary entry and exposure cap"
                    );
                }
                let wallet_cap = parse_u256(&args.max_wallet_weth_balance)?;
                if wallet_cap < U256::from(LIVE_CANARY_AMOUNT_IN_WEI)
                    || wallet_cap > U256::from(LIVE_CANARY_MAX_WETH_BALANCE_WEI)
                {
                    bail!("signed broadcast wallet WETH cap is outside the live-canary range");
                }
                if args.slippage_bps > LIVE_CANARY_MAX_SLIPPAGE_BPS
                    || args.max_slippage_bps > LIVE_CANARY_MAX_SLIPPAGE_BPS
                {
                    bail!("signed broadcast slippage exceeds the live-canary cap");
                }
            }
            if let Some(minimum) = args.round_trip_exit_min_weth_out.as_deref()
                && (!args.broadcast || parse_u256(minimum)? == U256::ZERO)
            {
                bail!("round-trip exit requires broadcast and a non-zero minimum WETH output");
            }
            if args.rpc_url == TESTNET_RPC_URL || args.sequencer_url == TESTNET_SEQUENCER_URL {
                bail!("this executable has no canonical NOXA testnet deployment to trade");
            }
        }
    }
    Ok(())
}

fn ensure_live_broadcast_armed(args: &Cli) -> Result<()> {
    if args.mode != RuntimeMode::Signed || !args.broadcast {
        return Ok(());
    }
    validate_kill_switch(
        args.kill_switch_file
            .as_deref()
            .context("signed broadcast requires --kill-switch-file")?,
    )
}

fn validate_kill_switch(path: &Path) -> Result<()> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("open live kill switch {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("inspect live kill switch {}", path.display()))?;
    if !metadata.file_type().is_file()
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.len() != KILL_SWITCH_ARMED.len() as u64
    {
        bail!("live kill switch must be an owned mode-0600 regular file containing ARMED");
    }
    let mut state = Vec::with_capacity(KILL_SWITCH_ARMED.len());
    file.read_to_end(&mut state)
        .context("read live kill switch")?;
    if state != KILL_SWITCH_ARMED {
        bail!("live kill switch is disarmed");
    }
    Ok(())
}

fn validate_caps(
    args: &Cli,
    amount_in: U256,
    max_fee_per_gas: u128,
    limits: RiskLimits,
) -> Result<()> {
    if amount_in == U256::ZERO || amount_in > limits.max_trade_amount_in {
        bail!("amount input is zero or above its strict cap");
    }
    if amount_in > limits.max_open_exposure {
        bail!("amount input is above open-exposure cap");
    }
    if args.gas_limit == 0 || max_fee_per_gas == 0 {
        bail!("gas limit and maximum fee must be non-zero");
    }
    let max_gas_cost = U256::from(args.gas_limit)
        .checked_mul(U256::from(max_fee_per_gas))
        .ok_or_else(|| anyhow::anyhow!("maximum gas cost overflow"))?;
    if max_gas_cost > limits.max_gas_cost_wei {
        bail!("maximum gas cost exceeds its strict cap");
    }
    Ok(())
}

fn parse_u256(value: &str) -> Result<U256> {
    if let Some(value) = value.strip_prefix("0x") {
        U256::from_str_radix(value, 16).context("parse hexadecimal U256")
    } else {
        U256::from_str(value).context("parse decimal U256")
    }
}

fn parse_u128(value: &str) -> Result<u128> {
    if let Some(value) = value.strip_prefix("0x") {
        u128::from_str_radix(value, 16).context("parse hexadecimal u128")
    } else {
        value.parse().context("parse decimal u128")
    }
}

fn parse_address_set(values: &[String], flag: &str) -> Result<HashSet<Address>> {
    values
        .iter()
        .map(|value| {
            let address = Address::from_str(value)
                .with_context(|| format!("parse {flag} address {value}"))?;
            if address == Address::ZERO {
                bail!("{flag} cannot contain the zero address");
            }
            Ok(address)
        })
        .collect()
}

fn load_watched_wallets(args: &Cli) -> Result<HashSet<Address>> {
    let mut values = args.watched_wallets.clone();
    if let Some(path) = args.watch_wallet_file.as_deref() {
        let metadata = fs::symlink_metadata(path)
            .with_context(|| format!("inspect --watch-wallet-file {}", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("--watch-wallet-file must be a regular non-symlink file");
        }
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("--watch-wallet-file must not be accessible by group or other users");
        }
        let contents = fs::read_to_string(path)
            .with_context(|| format!("read --watch-wallet-file {}", path.display()))?;
        values.extend(
            contents
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_owned),
        );
    }
    if values.len() > 1_024 {
        bail!("copy watchlist exceeds 1024 entries");
    }
    let watched = parse_address_set(&values, "copy watchlist")?;
    if watched.is_empty() {
        bail!("copy watchlist contains no addresses");
    }
    Ok(watched)
}

fn emit(value: serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}

fn unix_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const RECIPIENT: &str = "0xd7A41D7E502F5D63B36Ec59c84F59A3eFA6B99a0";

    fn armed_kill_switch() -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .unwrap();
        file.write_all(KILL_SWITCH_ARMED).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn paper_mode_refuses_every_broadcast_request() {
        let args = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--mode",
            "paper",
            "--recipient",
            RECIPIENT,
            "--broadcast",
        ])
        .unwrap();
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn copy_strategy_requires_leaders_but_allows_dynamic_tokens() {
        let missing = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--strategy",
            "copy",
            "--deployment",
            "active-noxa",
            "--recipient",
            RECIPIENT,
        ])
        .unwrap();
        assert!(validate_args(&missing).is_err());

        let configured = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--strategy",
            "copy",
            "--recipient",
            RECIPIENT,
            "--watch-wallet",
            "0x1111111111111111111111111111111111111111",
            "--copy-token",
            "0x2222222222222222222222222222222222222222",
        ])
        .unwrap();
        assert!(validate_args(&configured).is_ok());
        assert_eq!(
            parse_address_set(&configured.watched_wallets, "--watch-wallet")
                .unwrap()
                .len(),
            1
        );

        let dynamic_only = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--strategy",
            "copy",
            "--recipient",
            RECIPIENT,
            "--watch-wallet",
            "0x1111111111111111111111111111111111111111",
        ])
        .unwrap();
        assert!(validate_args(&dynamic_only).is_ok());

        let launch_with_copy_flags = Cli {
            strategy: StrategyMode::Launch,
            ..configured
        };
        assert!(validate_args(&launch_with_copy_flags).is_err());
    }

    #[test]
    fn all_wallet_discovery_is_paper_only_and_excludes_explicit_watchlists() {
        let discovery = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--strategy",
            "copy",
            "--deployment",
            "active-noxa",
            "--recipient",
            RECIPIENT,
            "--copy-discover-all-wallets",
            "--copy-token",
            "0x2222222222222222222222222222222222222222",
        ])
        .unwrap();
        assert!(validate_args(&discovery).is_ok());

        let mixed = Cli {
            watched_wallets: vec!["0x1111111111111111111111111111111111111111".into()],
            ..discovery.clone()
        };
        assert!(validate_args(&mixed).is_err());

        let legacy = Cli {
            deployment: Deployment::LegacyNoxa,
            watched_wallets: Vec::new(),
            ..discovery.clone()
        };
        assert!(validate_args(&legacy).is_err());

        let signed = Cli {
            mode: RuntimeMode::Signed,
            watched_wallets: Vec::new(),
            keystore: Some(PathBuf::from(
                "/srv/codex-workspaces/hermes-secrets/trader.json",
            )),
            expected_address: Some(RECIPIENT.into()),
            ..discovery
        };
        assert!(validate_args(&signed).is_err());
    }

    #[test]
    fn paper_discovery_learns_contiguous_launches_during_warmup_without_general_emission() {
        assert!(should_observe_launch(false, true, true));
        assert!(!should_observe_launch(false, true, false));
        assert!(!should_observe_launch(false, false, true));
        assert!(should_observe_launch(true, false, true));
    }

    #[test]
    fn active_noxa_discovery_backfill_ranges_are_bounded_and_contiguous() {
        assert!(active_noxa_discovery_log_ranges(7_999_999).is_empty());
        assert_eq!(
            active_noxa_discovery_log_ranges(8_000_000),
            vec![(8_000_000, 8_000_000)]
        );
        assert_eq!(
            active_noxa_discovery_log_ranges(8_025_000),
            vec![(8_000_000, 8_024_999), (8_025_000, 8_025_000)]
        );
    }

    #[test]
    fn rpc_metrics_delta_separates_startup_from_live_watch_activity() {
        let baseline = RpcMetricsSnapshot {
            logical_requests: 100,
            http_attempts: 120,
            retries: 20,
            rate_limited: 20,
            server_errors: 0,
            transport_errors: 0,
        };
        let total = RpcMetricsSnapshot {
            logical_requests: 107,
            http_attempts: 127,
            retries: 20,
            rate_limited: 20,
            server_errors: 0,
            transport_errors: 0,
        };
        assert_eq!(
            rpc_metrics_delta(total, baseline),
            RpcMetricsSnapshot {
                logical_requests: 7,
                http_attempts: 7,
                retries: 0,
                rate_limited: 0,
                server_errors: 0,
                transport_errors: 0,
            }
        );
    }

    #[test]
    fn copy_strategy_refuses_timed_round_trip_exit_configuration() {
        let configured = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--strategy",
            "copy",
            "--recipient",
            RECIPIENT,
            "--watch-wallet",
            "0x1111111111111111111111111111111111111111",
            "--copy-token",
            "0x2222222222222222222222222222222222222222",
            "--round-trip-exit-min-weth-out",
            "1",
        ])
        .unwrap();
        assert!(validate_args(&configured).is_err());
    }

    #[test]
    fn dynamic_copy_registry_is_pool_bound_and_deduplicates_proofs() {
        let token = Address::with_last_byte(0x44);
        let pool = Address::with_last_byte(0x45);
        let other_pool = Address::with_last_byte(0x46);
        let mut registry = CopyTokenRegistry::default();
        assert!(!registry.launch_was_observed(token, pool));
        registry.observe_launch(token, pool);
        assert!(registry.launch_was_observed(token, pool));
        assert!(!registry.launch_was_observed(token, other_pool));
        assert!(registry.begin_validation(token));
        assert!(!registry.begin_validation(token));
        assert!(!registry.contains(token, pool));
        assert_eq!(
            registry.defer_failed_validation(token),
            Duration::from_millis(250)
        );
        assert!(!registry.begin_validation(token));
        registry
            .validation_retries
            .get_mut(&token)
            .unwrap()
            .retry_at = Instant::now();
        assert!(registry.begin_validation(token));
        assert_eq!(
            registry.defer_failed_validation(token),
            Duration::from_millis(500)
        );
        registry.insert_verified_launch(token, pool);
        assert!(registry.contains(token, pool));
        assert!(!registry.contains(token, other_pool));
        assert!(!registry.validation_retries.contains_key(&token));
        assert!(!registry.begin_validation(token));
    }

    #[test]
    fn local_watchlist_file_is_private_and_deduplicated() {
        use std::io::Write;

        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .unwrap();
        writeln!(file, "# local strategy watchlist").unwrap();
        writeln!(file, "0x1111111111111111111111111111111111111111").unwrap();
        writeln!(file, "0x2222222222222222222222222222222222222222").unwrap();
        let args = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--strategy",
            "copy",
            "--recipient",
            RECIPIENT,
            "--watch-wallet",
            "0x1111111111111111111111111111111111111111",
            "--watch-wallet-file",
            file.path().to_str().unwrap(),
        ])
        .unwrap();
        assert_eq!(load_watched_wallets(&args).unwrap().len(), 2);

        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o640))
            .unwrap();
        assert!(load_watched_wallets(&args).is_err());
    }

    #[test]
    fn signed_copy_broadcast_requires_explicit_leader_limit_price_trust() {
        let kill_switch = armed_kill_switch();
        let parsed = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--mode",
            "signed",
            "--strategy",
            "copy",
            "--recipient",
            RECIPIENT,
            "--expected-address",
            RECIPIENT,
            "--keystore",
            "/srv/codex-workspaces/hermes-secrets/trader.json",
            "--watch-wallet",
            "0x1111111111111111111111111111111111111111",
            "--copy-token",
            "0x2222222222222222222222222222222222222222",
            "--broadcast",
            "--approval-token",
            BROADCAST_APPROVAL,
        ])
        .unwrap();
        let untrusted = Cli {
            deployment: Deployment::ActiveNoxa,
            max_slippage_bps: LIVE_CANARY_MAX_SLIPPAGE_BPS,
            slippage_bps: LIVE_CANARY_MAX_SLIPPAGE_BPS,
            kill_switch_file: Some(kill_switch.path().to_path_buf()),
            ..parsed
        };
        assert!(validate_args(&untrusted).is_err());

        let trusted = Cli {
            copy_trust_leader_limit_price: true,
            ..untrusted
        };
        assert!(validate_args(&trusted).is_ok());
        assert!(ensure_live_broadcast_armed(&trusted).is_ok());
    }

    #[test]
    fn watched_wallet_copy_runs_entry_and_full_exit_through_paper_state() {
        let leader = Address::with_last_byte(11);
        let token = Address::with_last_byte(12);
        let recipient = Address::from_str(RECIPIENT).unwrap();
        let args = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--strategy",
            "copy",
            "--recipient",
            RECIPIENT,
            "--amount-in",
            "100",
            "--max-trade-amount-in",
            "100",
            "--max-open-exposure",
            "100",
            "--max-gas-cost-wei",
            "1000000",
            "--max-fee-per-gas",
            "1",
            "--gas-limit",
            "100",
            "--watch-wallet",
            &leader.to_string(),
            "--copy-token",
            &token.to_string(),
        ])
        .unwrap();
        let policy = WatchedWalletCopyPolicy::new(
            HashSet::from([leader]),
            HashSet::from([token]),
            U256::from(100),
            U256::from(1_000),
            2,
        )
        .unwrap();
        let limits = RiskLimits {
            max_trade_amount_in: U256::from(100),
            max_open_exposure: U256::from(100),
            max_gas_cost_wei: U256::from(1_000_000),
            max_session_loss: U256::from(100),
            max_slippage_bps: 500,
        };
        let mut engine = Engine::Paper {
            runtime: Box::new(AutomatedPaperRuntime::new(0, limits)),
            pending_fill: None,
        };
        let boundary = |block| FeedBoundary {
            l1_block_number: block,
            l1_timestamp: 1_800_000_000 + block,
            sequence_contiguous: true,
        };
        let observed = |tx_byte, token_in, token_out, amount_in, minimum_out| ObservedCopySwap {
            tx_hash: B256::with_last_byte(tx_byte),
            chain_id: Some(hermes_feed::robinhood::CHAIN_ID),
            from: leader,
            to: UNISWAP_V3_SWAP_ROUTER_02,
            value: U256::ZERO,
            intent: V3ExactInputIntent {
                token_in,
                token_out,
                fee: hermes_feed::robinhood::NOXA_POOL_FEE,
                recipient: leader,
                amount_in: U256::from(amount_in),
                amount_out_minimum: U256::from(minimum_out),
                sqrt_price_limit_x96: U256::ZERO,
            },
        };
        let mut triggers = 0;

        handle_copy_candidate(
            &mut engine,
            &policy,
            observed(1, WETH, token, 200, 500),
            &mut triggers,
            boundary(100),
            recipient,
            1,
            0,
            &args,
            Instant::now(),
        )
        .unwrap();
        let Engine::Paper { runtime, .. } = &mut engine else {
            unreachable!()
        };
        let entry = runtime.snapshot().pending_order.unwrap();
        runtime.observe_boundary(boundary(101)).unwrap();
        runtime
            .reconcile_fill(entry.id, U256::from(250), U256::ZERO)
            .unwrap();
        assert_eq!(
            runtime.snapshot().positions[0].token_amount,
            U256::from(250)
        );

        handle_copy_candidate(
            &mut engine,
            &policy,
            observed(2, token, WETH, 400, 160),
            &mut triggers,
            boundary(102),
            recipient,
            1,
            0,
            &args,
            Instant::now(),
        )
        .unwrap();
        let Engine::Paper { runtime, .. } = &mut engine else {
            unreachable!()
        };
        let exit = runtime.snapshot().pending_order.unwrap();
        runtime.observe_boundary(boundary(103)).unwrap();
        runtime
            .reconcile_fill(exit.id, U256::from(100), U256::ZERO)
            .unwrap();
        let final_state = runtime.snapshot();
        assert!(final_state.positions.is_empty());
        assert_eq!(final_state.open_exposure, U256::ZERO);
        assert_eq!(final_state.next_nonce, 2);
        assert_eq!(triggers, 2);
    }

    #[test]
    fn completion_reason_distinguishes_deadline_and_signal_shutdown() {
        assert_eq!(completion_reason(true, false), "duration_complete");
        assert_eq!(
            completion_reason(false, false),
            "duration_complete_launch_disabled"
        );
        assert_eq!(completion_reason(true, true), "shutdown_signal");
        assert_eq!(
            completion_reason(false, true),
            "shutdown_signal_launch_disabled"
        );
    }

    #[test]
    fn feed_request_uses_the_official_nitro_resume_headers() {
        let request = feed_request("wss://feed.mainnet.chain.robinhood.com", 8_859_765).unwrap();
        assert_eq!(
            request.headers()[NITRO_FEED_CLIENT_VERSION_HEADER],
            NITRO_FEED_CLIENT_VERSION
        );
        assert_eq!(
            request.headers()[NITRO_REQUESTED_SEQUENCE_HEADER],
            "8859765"
        );
    }

    #[test]
    fn feed_request_rejects_an_invalid_websocket_url() {
        assert!(feed_request("not a websocket URL", 1).is_err());
    }

    #[test]
    fn feed_resume_skips_replay_then_accepts_the_exact_requested_sequence() {
        let mut resume = FeedResume {
            reconnect: 1,
            requested_sequence: 100,
            replayed_messages: 0,
        };
        assert_eq!(resume.observe(98), FeedResumeDisposition::Replay);
        assert_eq!(resume.observe(99), FeedResumeDisposition::Replay);
        assert_eq!(resume.replayed_messages, 2);
        assert_eq!(resume.observe(100), FeedResumeDisposition::Exact);
    }

    #[test]
    fn feed_resume_reports_a_forward_gap() {
        let mut resume = FeedResume {
            reconnect: 1,
            requested_sequence: 100,
            replayed_messages: 0,
        };
        assert_eq!(resume.observe(101), FeedResumeDisposition::ForwardGap);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reconnect_handshake_sends_the_requested_sequence_to_the_server() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let (header_sender, header_receiver) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut header_sender = Some(header_sender);
            let websocket = tokio_tungstenite::accept_hdr_async(
                stream,
                move |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                      response| {
                    let requested = request.headers()[NITRO_REQUESTED_SEQUENCE_HEADER]
                        .to_str()
                        .unwrap()
                        .to_owned();
                    header_sender.take().unwrap().send(requested).unwrap();
                    Ok(response)
                },
            )
            .await
            .unwrap();
            drop(websocket);
        });

        let read = reconnect_feed(
            &url,
            Instant::now() + Duration::from_secs(2),
            1,
            42_424,
            "test disconnect",
            false,
        )
        .await
        .unwrap();
        assert!(read.is_some());
        assert_eq!(header_receiver.await.unwrap(), "42424");
        server.await.unwrap();
    }

    #[test]
    fn only_launch_selector_preserves_the_predictor_cache() {
        let mut launch = hermes_feed::noxa_abi::LAUNCH_TOKEN_SELECTOR.to_vec();
        launch.extend_from_slice(&[0_u8; 128]);
        assert!(!is_non_launch_factory_transaction(&launch));
        assert!(is_non_launch_factory_transaction(&[]));
        assert!(is_non_launch_factory_transaction(&[0x23, 0x6a, 0x4a, 0xfb]));
        assert!(is_non_launch_factory_transaction(&[0xf2, 0xfd, 0xe3, 0x8b]));
    }

    #[test]
    fn signed_broadcast_requires_exact_approval_and_mainnet_endpoints() {
        let missing = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--mode",
            "signed",
            "--recipient",
            RECIPIENT,
            "--expected-address",
            RECIPIENT,
            "--keystore",
            "/srv/codex-workspaces/hermes-secrets/trader.json",
            "--broadcast",
        ])
        .unwrap();
        assert!(validate_args(&missing).is_err());

        let kill_switch = armed_kill_switch();
        let parsed = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--mode",
            "signed",
            "--recipient",
            RECIPIENT,
            "--expected-address",
            RECIPIENT,
            "--keystore",
            "/srv/codex-workspaces/hermes-secrets/trader.json",
            "--broadcast",
            "--approval-token",
            BROADCAST_APPROVAL,
        ])
        .unwrap();
        let approved = Cli {
            deployment: Deployment::ActiveNoxa,
            max_slippage_bps: LIVE_CANARY_MAX_SLIPPAGE_BPS,
            slippage_bps: LIVE_CANARY_MAX_SLIPPAGE_BPS,
            kill_switch_file: Some(kill_switch.path().to_path_buf()),
            ..parsed
        };
        assert!(validate_args(&approved).is_ok());
        assert!(ensure_live_broadcast_armed(&approved).is_ok());

        let testnet = Cli {
            rpc_url: TESTNET_RPC_URL.into(),
            ..approved
        };
        assert!(validate_args(&testnet).is_err());
    }

    #[test]
    fn live_kill_switch_fails_closed_on_content_mode_and_symlink() {
        let mut switch = armed_kill_switch();
        assert!(validate_kill_switch(switch.path()).is_ok());

        switch.as_file_mut().set_len(0).unwrap();
        switch.write_all(b"DISARMED\n").unwrap();
        switch.flush().unwrap();
        assert!(validate_kill_switch(switch.path()).is_err());

        switch
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o640))
            .unwrap();
        assert!(validate_kill_switch(switch.path()).is_err());

        let link_dir = tempfile::tempdir().unwrap();
        let link = link_dir.path().join("switch");
        std::os::unix::fs::symlink(switch.path(), &link).unwrap();
        assert!(validate_kill_switch(&link).is_err());
    }

    #[test]
    fn live_broadcast_profile_rejects_legacy_oversized_and_loose_configuration() {
        let kill_switch = armed_kill_switch();
        let approved = Cli {
            mode: RuntimeMode::Signed,
            deployment: Deployment::ActiveNoxa,
            broadcast: true,
            approval_token: Some(BROADCAST_APPROVAL.into()),
            expected_address: Some(RECIPIENT.into()),
            keystore: Some("/srv/codex-workspaces/hermes-secrets/trader.json".into()),
            kill_switch_file: Some(kill_switch.path().to_path_buf()),
            max_slippage_bps: LIVE_CANARY_MAX_SLIPPAGE_BPS,
            slippage_bps: LIVE_CANARY_MAX_SLIPPAGE_BPS,
            ..Cli::try_parse_from(["hermes-noxa-runtime", "--recipient", RECIPIENT]).unwrap()
        };
        assert!(validate_args(&approved).is_ok());

        let legacy = Cli {
            deployment: Deployment::LegacyNoxa,
            ..approved.clone()
        };
        assert!(validate_args(&legacy).is_err());

        let oversized = Cli {
            amount_in: (LIVE_CANARY_AMOUNT_IN_WEI + 1).to_string(),
            ..approved.clone()
        };
        assert!(validate_args(&oversized).is_err());

        let loose_slippage = Cli {
            max_slippage_bps: LIVE_CANARY_MAX_SLIPPAGE_BPS + 1,
            ..approved.clone()
        };
        assert!(validate_args(&loose_slippage).is_err());

        let overfunded = Cli {
            max_wallet_weth_balance: (LIVE_CANARY_MAX_WETH_BALANCE_WEI + 1).to_string(),
            ..approved
        };
        assert!(validate_args(&overfunded).is_err());
    }

    #[test]
    fn signed_funding_enforces_exact_allowance_and_wallet_balance_cap() {
        let amount = U256::from(LIVE_CANARY_AMOUNT_IN_WEI);
        let wallet_cap = U256::from(LIVE_CANARY_MAX_WETH_BALANCE_WEI);
        let gas = U256::from(10_000_u64);
        assert!(validate_signed_funding(amount, amount, gas, amount, wallet_cap, gas).is_ok());
        assert!(
            validate_signed_funding(
                wallet_cap + U256::from(1),
                amount,
                gas,
                amount,
                wallet_cap,
                gas,
            )
            .is_err()
        );
        assert!(
            validate_signed_funding(amount, amount + U256::from(1), gas, amount, wallet_cap, gas)
                .is_err()
        );
    }

    #[test]
    fn configured_caps_reject_oversized_amount_and_gas() {
        let args = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--mode",
            "paper",
            "--recipient",
            RECIPIENT,
        ])
        .unwrap();
        let limits = RiskLimits {
            max_trade_amount_in: U256::from(100),
            max_open_exposure: U256::from(100),
            max_gas_cost_wei: U256::from(100),
            max_session_loss: U256::from(100),
            max_slippage_bps: 500,
        };
        assert!(validate_caps(&args, U256::from(101), 1, limits).is_err());
        assert!(validate_caps(&args, U256::from(1), 1, limits).is_err());
    }

    #[test]
    fn round_trip_requires_the_separately_approved_broadcast_mode() {
        let kill_switch = armed_kill_switch();
        let args = Cli::try_parse_from([
            "hermes-noxa-runtime",
            "--mode",
            "signed",
            "--recipient",
            RECIPIENT,
            "--expected-address",
            RECIPIENT,
            "--keystore",
            "/srv/codex-workspaces/hermes-secrets/trader.json",
            "--round-trip-exit-min-weth-out",
            "1",
        ])
        .unwrap();
        assert!(validate_args(&args).is_err());

        let approved = Cli {
            broadcast: true,
            approval_token: Some(BROADCAST_APPROVAL.into()),
            deployment: Deployment::ActiveNoxa,
            max_slippage_bps: LIVE_CANARY_MAX_SLIPPAGE_BPS,
            slippage_bps: LIVE_CANARY_MAX_SLIPPAGE_BPS,
            kill_switch_file: Some(kill_switch.path().to_path_buf()),
            ..args
        };
        assert!(validate_args(&approved).is_ok());
        assert!(ensure_live_broadcast_armed(&approved).is_ok());
    }

    #[test]
    fn receipt_fill_uses_only_output_token_transfers_to_signer() {
        let token = Address::with_last_byte(9);
        let recipient = Address::from_str(RECIPIENT).unwrap();
        let mut recipient_topic = [0_u8; 32];
        recipient_topic[12..].copy_from_slice(recipient.as_slice());
        let transfer_topic = alloy_primitives::b256!(
            "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
        );
        let transfer = |address, amount: u64| ReceiptLog {
            address,
            log_index: amount,
            topics: vec![transfer_topic, B256::ZERO, B256::from(recipient_topic)],
            data: U256::from(amount).to_be_bytes::<32>().to_vec().into(),
        };
        let logs = vec![
            transfer(Address::with_last_byte(8), 99),
            transfer(token, 40),
            transfer(token, 2),
        ];
        assert_eq!(
            extract_erc20_received(&logs, token, recipient).unwrap(),
            U256::from(42)
        );
        assert!(extract_erc20_received(&logs, Address::with_last_byte(7), recipient).is_err());
    }

    #[test]
    fn copy_token_validation_serializes_full_width_restriction_height_as_string() {
        let validation = CopyTokenValidation {
            token: Address::with_last_byte(1),
            pool: Address::with_last_byte(2),
            validated_l2_block: 42,
            fee: 10_000,
            liquidity: u128::MAX,
            restriction_end_l1_block: U256::MAX,
            token_code_bytes: 1,
            pool_code_bytes: 1,
        };

        let value = serde_json::to_value(validation).unwrap();
        assert_eq!(
            value["restriction_end_l1_block"],
            serde_json::Value::String(U256::MAX.to_string())
        );
        assert_eq!(
            value["liquidity"],
            serde_json::Value::String(u128::MAX.to_string())
        );
    }
}
