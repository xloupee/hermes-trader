use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{BufWriter, Write};
use std::str::FromStr;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use alloy_consensus::Transaction;
use alloy_primitives::{Address, B256, U256};
use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use hermes_feed::feed::BroadcastMessage;
use hermes_feed::robinhood::{
    DIRECT_FEED_URL, NOXA_FACTORY_RUNTIME_KECCAK256, NOXA_LAUNCH_FACTORY, NOXA_POOL_FEE,
    PUBLIC_RPC_URL, TESTNET_CHAIN_ID, TESTNET_RPC_URL, TESTNET_SEQUENCER_URL,
    UNISWAP_V3_SWAP_ROUTER_02, WETH,
};
use hermes_feed::{
    ConditionalOptions, ConditionalRetryDecision, ConditionalRetryState, DedicatedNonceManager,
    FeedDecoder, Filter, NoxaLaunchEvent, NoxaLaunchHeader, NoxaPolicyInput, NoxaRpcClient,
    RiskLedger, RiskLimits, SequenceTracker, SequencerClient, TokenRestrictionSnapshot,
    TradePreflightInput, V3ExactInputIntent, decode_launch_call, decode_launch_header,
    decode_token_launched, encode_v3_exact_input_single, evaluate_noxa_policy,
    evaluate_testnet_preflight, hydrate_noxa_launch_receipt, validate_signed_testnet_canary,
};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_tungstenite::tungstenite::Message;

const FACTORY_DEPLOYMENT_L2_BLOCK: u64 = 61_688;
const DEFAULT_QUOTE_AMOUNT_IN: &str = "10000000000000000";
const OUTPUT_QUEUE_CAPACITY: usize = 4_096;

static JSON_OUTPUT: OnceLock<SyncSender<JsonOutput>> = OnceLock::new();

enum JsonOutput {
    Record(Value),
    Shutdown,
}

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Paper-only low-latency NOXA launch observer and V3 validator"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Read and pin the current NOXA factory state.
    Status(RpcArgs),
    /// Reconstruct one historical launch receipt and locally quote its pool.
    Inspect(InspectArgs),
    /// Strictly decode TokenLaunched logs over an L2 block range.
    Backfill(BackfillArgs),
    /// Watch the post-execution Nitro feed for NOXA launch transactions.
    Observe(ObserveArgs),
    /// Measure parent-Ethereum head arrival versus Robinhood post-execution feed adoption.
    CalibrateBoundary(CalibrateBoundaryArgs),
    /// Read testnet nonce, funding, wrapped balance, allowance, and router code.
    TestnetPreflight(TestnetPreflightArgs),
    /// Validate, and only with --broadcast submit, an externally signed testnet self-transfer.
    TestnetSubmitCanary(TestnetSubmitCanaryArgs),
}

#[derive(Debug, Args, Clone)]
struct RpcArgs {
    #[arg(long, default_value = PUBLIC_RPC_URL)]
    rpc_url: String,
}

#[derive(Debug, Args)]
struct InspectArgs {
    #[command(flatten)]
    rpc: RpcArgs,
    #[arg(long)]
    tx_hash: String,
    /// WETH exact input in base units for the local post-launch quote.
    #[arg(long, default_value = DEFAULT_QUOTE_AMOUNT_IN)]
    amount_in: String,
    /// Quote haircut used for the prepared SwapRouter02 minimum output.
    #[arg(long, default_value_t = 500)]
    slippage_bps: u16,
    /// Optional recipient used to emit prepared router calldata. No key is loaded.
    #[arg(long)]
    recipient: Option<String>,
    /// Repeat the local quote to measure hot-math cost after hydration.
    #[arg(long, default_value_t = 0)]
    benchmark_iterations: u64,
}

#[derive(Debug, Args)]
struct BackfillArgs {
    #[command(flatten)]
    rpc: RpcArgs,
    #[arg(long, default_value_t = FACTORY_DEPLOYMENT_L2_BLOCK)]
    from_l2_block: u64,
    /// Inclusive end L2 block. Omit to pin the current head.
    #[arg(long)]
    to_l2_block: Option<u64>,
    #[arg(long, default_value_t = 2_000)]
    blocks_per_request: u64,
}

#[derive(Debug, Args)]
struct ObserveArgs {
    #[arg(long, default_value = DIRECT_FEED_URL)]
    feed_url: String,
    #[arg(long, default_value_t = 10)]
    warmup_seconds: u64,
    #[arg(long, default_value_t = 10)]
    health_interval_seconds: u64,
    #[arg(long, default_value_t = 30)]
    factory_status_interval_seconds: u64,
    /// Stop after this many seconds; omit to keep watching.
    #[arg(long)]
    run_seconds: Option<u64>,
    /// WETH exact input used for verified launch shadow quotes.
    #[arg(long, default_value = DEFAULT_QUOTE_AMOUNT_IN)]
    amount_in: String,
    #[arg(long, default_value_t = 500)]
    slippage_bps: u16,
    /// Optional paper recipient used to prepare router calldata; no transaction is signed.
    #[arg(long)]
    recipient: Option<String>,
    #[command(flatten)]
    rpc: RpcArgs,
}

#[derive(Debug, Args)]
struct CalibrateBoundaryArgs {
    /// Ethereum parent-chain websocket supporting eth_subscribe(newHeads).
    #[arg(long)]
    l1_ws_url: String,
    #[arg(long, default_value = DIRECT_FEED_URL)]
    feed_url: String,
    #[arg(long, default_value_t = 10)]
    warmup_seconds: u64,
    #[arg(long, default_value_t = 100)]
    samples: usize,
    /// Stop cleanly after this many seconds even if the sample target is not met.
    #[arg(long)]
    run_seconds: Option<u64>,
}

#[derive(Debug, Args)]
struct TestnetPreflightArgs {
    #[arg(long, default_value = TESTNET_RPC_URL)]
    rpc_url: String,
    #[arg(long)]
    account: String,
    #[arg(long)]
    wrapped_native: String,
    #[arg(long)]
    router: String,
    #[arg(long)]
    amount_in: String,
    #[arg(long, default_value_t = 300_000)]
    gas_limit: u64,
    #[arg(long, default_value_t = 100_000_000)]
    max_fee_per_gas: u128,
}

#[derive(Debug, Args)]
struct TestnetSubmitCanaryArgs {
    #[arg(long, default_value = TESTNET_RPC_URL)]
    rpc_url: String,
    #[arg(long, default_value = TESTNET_SEQUENCER_URL)]
    sequencer_url: String,
    /// Binary EIP-2718 bytes or a 0x-prefixed hex text file. Never a private key.
    #[arg(long)]
    raw_tx_file: String,
    /// Maximum transferred wei; required and independently checked against signed bytes.
    #[arg(long)]
    max_value_wei: String,
    /// Maximum gas-limit times max-fee-per-gas in wei.
    #[arg(long)]
    max_gas_cost_wei: String,
    #[arg(long, default_value_t = 3)]
    l1_window: u64,
    #[arg(long, default_value_t = 2)]
    max_boundary_attempts: u16,
    /// Poll this long for hash/receipt reconciliation after submission.
    #[arg(long, default_value_t = 15)]
    reconcile_seconds: u64,
    /// Actually transmit the validated bytes. Without this flag the command is read-only.
    #[arg(long, default_value_t = false)]
    broadcast: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    // Parse before the writer thread takes the stdout lock so clap can print
    // --help/--version without deadlocking against an idle JSON writer.
    let command = Cli::parse().command;
    let writer = start_json_writer()?;
    let result = match command {
        Command::Status(args) => status(args).await,
        Command::Inspect(args) => inspect(args).await,
        Command::Backfill(args) => backfill(args).await,
        Command::Observe(args) => observe(args).await,
        Command::CalibrateBoundary(args) => calibrate_boundary(args).await,
        Command::TestnetPreflight(args) => testnet_preflight(args).await,
        Command::TestnetSubmitCanary(args) => testnet_submit_canary(args).await,
    };
    let writer_result = stop_json_writer(writer);
    result.and(writer_result)
}

async fn testnet_preflight(args: TestnetPreflightArgs) -> Result<()> {
    let account = Address::from_str(&args.account).context("invalid --account")?;
    let wrapped_native =
        Address::from_str(&args.wrapped_native).context("invalid --wrapped-native")?;
    let router = Address::from_str(&args.router).context("invalid --router")?;
    let amount_in = parse_u256(&args.amount_in)?;
    let client = NoxaRpcClient::with_url(args.rpc_url)?;
    let (chain_id, pending_nonce, native_balance, wrapped_balance, router_allowance, router_code) =
        tokio::try_join!(
            client.chain_id(),
            client.pending_nonce(account),
            client.native_balance(account),
            client.erc20_balance(wrapped_native, account),
            client.erc20_allowance(wrapped_native, account, router),
            client.code_at(router),
        )?;
    if chain_id != TESTNET_CHAIN_ID {
        bail!("RPC chain ID {chain_id} is not Robinhood testnet {TESTNET_CHAIN_ID}");
    }
    let input = TradePreflightInput {
        chain_id,
        account,
        wrapped_native,
        router,
        router_code_present: !router_code.is_empty(),
        native_balance,
        wrapped_balance,
        router_allowance,
        amount_in,
        gas_limit: args.gas_limit,
        max_fee_per_gas: args.max_fee_per_gas,
    };
    let decision = evaluate_testnet_preflight(input);
    write_json(json!({
        "record_type": "noxa_testnet_preflight",
        "network": "robinhood_testnet",
        "chain_id": chain_id,
        "pending_nonce": pending_nonce,
        "account": account,
        "wrapped_native": wrapped_native,
        "router": router,
        "router_code_bytes": router_code.len(),
        "native_balance": native_balance,
        "wrapped_balance": wrapped_balance,
        "router_allowance": router_allowance,
        "amount_in": amount_in,
        "gas_limit": args.gas_limit,
        "max_fee_per_gas": args.max_fee_per_gas,
        "ready": decision.is_ok(),
        "reject_reason": decision.as_ref().err().map(ToString::to_string),
    }))?;
    decision.map_err(anyhow::Error::from)
}

async fn testnet_submit_canary(args: TestnetSubmitCanaryArgs) -> Result<()> {
    if args.l1_window == 0 || args.max_boundary_attempts == 0 {
        bail!("--l1-window and --max-boundary-attempts must be non-zero");
    }
    let maximum_value = parse_u256(&args.max_value_wei)?;
    let maximum_gas_cost = parse_u256(&args.max_gas_cost_wei)?;
    let raw = read_raw_transaction(&args.raw_tx_file)?;
    let canary = validate_signed_testnet_canary(&raw, maximum_value)?;
    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    let (chain_id, pending_nonce, native_balance, latest_block) = tokio::try_join!(
        rpc.chain_id(),
        rpc.pending_nonce(canary.signer),
        rpc.native_balance(canary.signer),
        rpc.latest_block(),
    )?;
    if chain_id != TESTNET_CHAIN_ID {
        bail!("RPC chain ID {chain_id} is not Robinhood testnet {TESTNET_CHAIN_ID}");
    }
    if canary.nonce != pending_nonce {
        bail!(
            "signed nonce {} does not match pending nonce {pending_nonce}",
            canary.nonce
        );
    }
    let signed_gas_cost = U256::from(canary.gas_limit)
        .checked_mul(U256::from(canary.max_fee_per_gas))
        .context("signed gas cost overflow")?;
    if signed_gas_cost > maximum_gas_cost {
        bail!("signed maximum gas cost exceeds --max-gas-cost-wei");
    }
    let required_balance = canary
        .value
        .checked_add(signed_gas_cost)
        .context("required native balance overflow")?;
    if native_balance < required_balance {
        bail!("native balance cannot cover the signed value plus maximum gas cost");
    }

    let mut nonces = DedicatedNonceManager::from_pending_nonce(pending_nonce);
    let lease = nonces.reserve()?;
    nonces.mark_signed(lease.nonce, canary.hash)?;
    let mut risk = RiskLedger::new(RiskLimits {
        max_trade_amount_in: maximum_value,
        max_open_exposure: maximum_value,
        max_gas_cost_wei: maximum_gas_cost,
        max_session_loss: maximum_value,
        max_slippage_bps: 0,
    });
    let reservation = risk.reserve(canary.value, canary.gas_limit, canary.max_fee_per_gas, 0)?;
    let conditions = ConditionalOptions::first_eligible_window(
        latest_block.l1_block_number,
        args.l1_window,
        None,
    )
    .context("conditional L1 window overflow")?;

    write_json(json!({
        "record_type": "noxa_testnet_canary_validated",
        "broadcast": args.broadcast,
        "chain_id": chain_id,
        "hash": canary.hash,
        "signer": canary.signer,
        "nonce": canary.nonce,
        "value": canary.value,
        "gas_limit": canary.gas_limit,
        "max_fee_per_gas": canary.max_fee_per_gas,
        "max_priority_fee_per_gas": canary.max_priority_fee_per_gas,
        "maximum_value": maximum_value,
        "maximum_gas_cost": maximum_gas_cost,
        "native_balance": native_balance,
        "conditions": conditions,
    }))?;
    if !args.broadcast {
        risk.release_unsubmitted(reservation.id)?;
        nonces.release_never_submitted(canary.nonce)?;
        return Ok(());
    }

    // Mark ambiguous before the network call: a transport error can still mean
    // the sequencer received the bytes, so this nonce must never be reused.
    nonces.mark_submitted(canary.nonce, canary.hash)?;
    let sequencer = SequencerClient::with_url(args.sequencer_url)?;
    let mut retry = ConditionalRetryState {
        expected_tx_hash: canary.hash,
        conditions,
        attempts: 0,
        max_boundary_attempts: args.max_boundary_attempts,
    };
    let submission_started_unix_ns = unix_ns();
    let submission_started = Instant::now();
    let mut attempt_elapsed_ns = Vec::new();
    let decision = loop {
        let attempt_started = Instant::now();
        let response = match sequencer
            .submit_conditional(&canary.raw, retry.conditions)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                attempt_elapsed_ns.push(attempt_started.elapsed().as_nanos());
                break ConditionalRetryDecision::ReconcileByHash {
                    tx_hash: canary.hash,
                    reason: error.to_string(),
                };
            }
        };
        attempt_elapsed_ns.push(attempt_started.elapsed().as_nanos());
        let decision = retry.on_response(response);
        if matches!(decision, ConditionalRetryDecision::RetrySameBytes) {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        }
        break decision;
    };
    write_json(json!({
        "record_type": "noxa_testnet_canary_submission",
        "hash": canary.hash,
        "classified_attempts": retry.attempts,
        "network_attempts": attempt_elapsed_ns.len(),
        "submission_started_unix_ns": submission_started_unix_ns,
        "submission_elapsed_ns": submission_started.elapsed().as_nanos(),
        "attempt_elapsed_ns": attempt_elapsed_ns,
        "decision": &decision,
    }))?;

    let deadline = Instant::now() + Duration::from_secs(args.reconcile_seconds);
    loop {
        if let Some(receipt) = rpc.receipt(canary.hash).await? {
            nonces.finalize_included(canary.nonce, canary.hash)?;
            let realized_loss = if receipt.status {
                U256::ZERO
            } else {
                canary.value
            };
            let risk_status = risk.settle(reservation.id, realized_loss)?;
            write_json(json!({
                "record_type": "noxa_testnet_canary_reconciled",
                "hash": canary.hash,
                "included": true,
                "receipt_status": receipt.status,
                "l2_block_number": receipt.l2_block_number,
                "submit_to_receipt_ns": submission_started.elapsed().as_nanos(),
                "risk_status": risk_status,
            }))?;
            return Ok(());
        }
        if Instant::now() >= deadline {
            let known = rpc.transaction_by_hash(canary.hash).await?.is_some();
            write_json(json!({
                "record_type": "noxa_testnet_canary_reconciled",
                "hash": canary.hash,
                "included": false,
                "known_by_rpc": known,
                "reconciliation_elapsed_ns": submission_started.elapsed().as_nanos(),
                "nonce_state": nonces.active(),
                "risk_reservation": risk.active,
            }))?;
            bail!("canary remains pending or ambiguous; nonce is intentionally not reusable");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn read_raw_transaction(path: &str) -> Result<Vec<u8>> {
    let bytes = fs::read(path).with_context(|| format!("read signed transaction {path}"))?;
    if let Ok(text) = std::str::from_utf8(&bytes) {
        let trimmed = text.trim();
        if let Some(hex_text) = trimmed.strip_prefix("0x") {
            return hex::decode(hex_text).context("decode 0x-prefixed signed transaction");
        }
    }
    if bytes.is_empty() {
        bail!("signed transaction file is empty");
    }
    Ok(bytes)
}

async fn status(args: RpcArgs) -> Result<()> {
    let client = NoxaRpcClient::with_url(args.rpc_url)?;
    let status = client.factory_status().await?;
    write_json(json!({
        "record_type": "noxa_factory_status",
        "status": &status,
        "runtime_hash_matches_pin": status.runtime_keccak256 == NOXA_FACTORY_RUNTIME_KECCAK256,
        "can_launch_now": status.launch_enabled
            && status.runtime_keccak256 == NOXA_FACTORY_RUNTIME_KECCAK256,
    }))
}

async fn inspect(args: InspectArgs) -> Result<()> {
    if args.slippage_bps >= 10_000 {
        bail!("--slippage-bps must be below 10000");
    }
    let tx_hash = B256::from_str(&args.tx_hash).context("invalid --tx-hash")?;
    let amount_in = parse_u256(&args.amount_in)?;
    if amount_in == U256::ZERO {
        bail!("--amount-in must be non-zero");
    }
    let recipient = args
        .recipient
        .as_deref()
        .map(Address::from_str)
        .transpose()
        .context("invalid --recipient")?;
    let client = NoxaRpcClient::with_url(args.rpc.rpc_url)?;
    let transaction = client
        .transaction_by_hash(tx_hash)
        .await?
        .ok_or_else(|| anyhow::anyhow!("transaction not found"))?;
    if transaction.to != Some(NOXA_LAUNCH_FACTORY) {
        bail!("transaction destination is not the canonical NOXA factory");
    }
    let intent = decode_launch_call(&transaction.input, transaction.value)
        .ok_or_else(|| anyhow::anyhow!("transaction is not a strict NOXA launchToken call"))?;
    let receipt = client
        .receipt(tx_hash)
        .await?
        .ok_or_else(|| anyhow::anyhow!("receipt not found"))?;
    if !receipt.status {
        bail!("launch transaction reverted");
    }
    let block = client.block_by_number(receipt.l2_block_number).await?;
    let hydrated = hydrate_noxa_launch_receipt(
        &receipt.logs,
        block.l1_block_number,
        receipt.l2_block_number,
    )?;
    if hydrated.launch.dex_id != intent.dex_id
        || hydrated.launch.launch_config_id != intent.launch_config_id
    {
        bail!("launch calldata and receipt IDs do not match");
    }
    let launch_fee = client.launch_fee_at(receipt.l2_block_number).await?;
    let token_restrictions = client
        .token_restriction_snapshot(hydrated.launch.token, receipt.l2_block_number, recipient)
        .await?;
    validate_token_restrictions(&token_restrictions, &hydrated.launch)?;
    let provisional_initial_buy = transaction
        .value
        .checked_sub(launch_fee)
        .ok_or_else(|| anyhow::anyhow!("transaction value is below pinned launch fee"))?;
    let quote = hydrated.pool.quote_exact_input(WETH, amount_in, None)?;
    let quote_benchmark = if args.benchmark_iterations == 0 {
        None
    } else {
        let started = Instant::now();
        for _ in 0..args.benchmark_iterations {
            std::hint::black_box(hydrated.pool.quote_exact_input(WETH, amount_in, None)?);
        }
        let elapsed_ns = started.elapsed().as_nanos();
        Some(json!({
            "iterations": args.benchmark_iterations,
            "total_ns": elapsed_ns,
            "average_ns": elapsed_ns / u128::from(args.benchmark_iterations),
        }))
    };
    let restrictions_end = u64::try_from(hydrated.launch.restrictions_end_l1_block)
        .context("restriction end does not fit u64")?;
    let policy = token_restrictions
        .recipient_balance
        .map(|recipient_balance_before| {
            evaluate_noxa_policy(NoxaPolicyInput {
                launch_l1_block: block.l1_block_number,
                restrictions_end_l1_block: restrictions_end,
                current_l1_block: block.l1_block_number.saturating_add(1),
                recipient_balance_before,
                expected_bought_output: quote.amount_out,
                // This is a single direct router call, so no prior pool buy has
                // occurred for tx.origin within this transaction.
                origin_bought_before: U256::ZERO,
                max_wallet_limit: token_restrictions.max_wallet_limit,
                max_tx_limit: token_restrictions.max_tx_limit,
            })
        });
    let amount_out_minimum = quote
        .amount_out
        .checked_mul(U256::from(10_000_u64 - u64::from(args.slippage_bps)))
        .ok_or_else(|| anyhow::anyhow!("minimum output overflow"))?
        / U256::from(10_000_u64);
    let prepared_calldata = recipient
        .map(|recipient| {
            encode_v3_exact_input_single(&V3ExactInputIntent {
                token_in: WETH,
                token_out: hydrated.launch.token,
                fee: NOXA_POOL_FEE,
                recipient,
                amount_in,
                amount_out_minimum,
                sqrt_price_limit_x96: U256::ZERO,
            })
            .ok_or_else(|| anyhow::anyhow!("could not encode SwapRouter02 call"))
        })
        .transpose()?;
    write_json(json!({
        "record_type": "noxa_launch_inspection",
        "transaction": transaction,
        "launch_intent": intent,
        "receipt": {
            "l2_block_number": receipt.l2_block_number,
            "transaction_index": receipt.transaction_index,
            "log_count": receipt.logs.len(),
        },
        "block": block,
        "launch": hydrated.launch,
        "mint_events": hydrated.mint_events,
        "swap_events": hydrated.swap_events,
        "launch_fee": launch_fee,
        "provisional_initial_buy": provisional_initial_buy,
        "initial_buy_matches_event": provisional_initial_buy == hydrated.launch.initial_buy_amount,
        "quote": quote,
        "quote_benchmark": quote_benchmark,
        "token_restrictions": token_restrictions,
        "policy_at_first_eligible_l1": policy,
        "policy_evaluated": policy.is_some(),
        "prepared": {
            "router": UNISWAP_V3_SWAP_ROUTER_02,
            "prewrapped_weth_and_prior_approval_required": true,
            "transaction_value": U256::ZERO,
            "amount_out_minimum": amount_out_minimum,
            "calldata": prepared_calldata.map(|bytes| format!("0x{}", hex::encode(bytes))),
        },
    }))
}

async fn backfill(args: BackfillArgs) -> Result<()> {
    if args.blocks_per_request == 0 {
        bail!("--blocks-per-request must be non-zero");
    }
    let client = NoxaRpcClient::with_url(args.rpc.rpc_url)?;
    let status = client.factory_status().await?;
    let to = args.to_l2_block.unwrap_or(status.pinned_l2_block);
    if args.from_l2_block > to {
        bail!("--from-l2-block exceeds end block");
    }
    let mut cursor = args.from_l2_block;
    let mut decoded = 0_u64;
    let mut invalid = 0_u64;
    let mut first: Option<Value> = None;
    let mut last: Option<Value> = None;
    let started = Instant::now();
    while cursor <= to {
        let end = cursor
            .saturating_add(args.blocks_per_request.saturating_sub(1))
            .min(to);
        for observed in client.token_launched_logs(cursor, end).await? {
            if let Some(event) = decode_token_launched(&observed.log) {
                decoded = decoded.saturating_add(1);
                let summary = json!({
                    "l2_block_number": observed.l2_block_number,
                    "transaction_hash": observed.transaction_hash,
                    "token": event.token,
                    "pool": event.pool,
                    "restrictions_end_l1_block": event.restrictions_end_l1_block,
                });
                first.get_or_insert_with(|| summary.clone());
                last = Some(summary);
            } else {
                invalid = invalid.saturating_add(1);
            }
        }
        if end == u64::MAX {
            break;
        }
        cursor = end + 1;
    }
    write_json(json!({
        "record_type": "noxa_backfill_summary",
        "from_l2_block": args.from_l2_block,
        "to_l2_block": to,
        "strictly_decoded_events": decoded,
        "invalid_events": invalid,
        "first": first,
        "last": last,
        "elapsed_ms": started.elapsed().as_millis(),
        "factory_currently_enabled": status.launch_enabled,
    }))
}

async fn observe(args: ObserveArgs) -> Result<()> {
    if args.slippage_bps >= 10_000 {
        bail!("--slippage-bps must be below 10000");
    }
    if args.health_interval_seconds == 0 {
        bail!("--health-interval-seconds must be non-zero");
    }
    if args.factory_status_interval_seconds == 0 {
        bail!("--factory-status-interval-seconds must be non-zero");
    }
    let amount_in = parse_u256(&args.amount_in)?;
    if amount_in == U256::ZERO {
        bail!("--amount-in must be non-zero");
    }
    let recipient = args
        .recipient
        .as_deref()
        .map(Address::from_str)
        .transpose()
        .context("invalid --recipient")?;
    let slippage_bps = args.slippage_bps;
    let rpc = NoxaRpcClient::with_url(args.rpc.rpc_url.clone())?;
    let status = rpc.factory_status().await?;
    if status.runtime_keccak256 != NOXA_FACTORY_RUNTIME_KECCAK256 {
        bail!("NOXA factory runtime hash does not match the pinned implementation");
    }
    write_json(json!({
        "record_type": "noxa_observer_start",
        "feed_url": args.feed_url,
        "factory": NOXA_LAUNCH_FACTORY,
        "factory_status": status,
        "feed_semantics": "post_execution_soft_confirmation",
        "candidate_emission": "warmup_and_gap_fail_closed",
        "receipt_verification": "asynchronous_bounded",
        "health_interval_seconds": args.health_interval_seconds,
        "factory_status_interval_seconds": args.factory_status_interval_seconds,
    }))?;
    let stop_at = args
        .run_seconds
        .map(|seconds| Instant::now() + Duration::from_secs(seconds));
    let mut decoder = FeedDecoder::new(Filter::default());
    let mut sequences = SequenceTracker::default();
    let mut reconnects = 0_u64;
    let mut backoff = Duration::from_millis(250);
    let verifier_slots = Arc::new(Semaphore::new(32));
    let mut verifiers: JoinSet<Result<()>> = JoinSet::new();
    let mut status_tasks: JoinSet<Result<()>> = JoinSet::new();
    let health_interval = Duration::from_secs(args.health_interval_seconds);
    let factory_status_interval = Duration::from_secs(args.factory_status_interval_seconds);
    let mut last_health = Instant::now();
    let mut last_factory_status = Instant::now();
    let mut health_feed_messages = 0_u64;
    let mut health_signed_transactions = 0_u64;
    'observer: loop {
        if stop_at.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        let connect_timeout = stop_at
            .map(|deadline| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(5))
            })
            .unwrap_or(Duration::from_secs(5));
        if connect_timeout.is_zero() {
            break;
        }
        let connect = tokio::time::timeout(
            connect_timeout,
            tokio_tungstenite::connect_async(&args.feed_url),
        )
        .await;
        let (stream, _) = match connect {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                write_json(json!({
                    "record_type": "noxa_connection",
                    "state": "connect_error",
                    "error": error.to_string(),
                    "reconnects": reconnects,
                }))?;
                if sleep_with_deadline(backoff, stop_at).await {
                    break;
                }
                backoff = (backoff * 2).min(Duration::from_secs(5));
                reconnects = reconnects.saturating_add(1);
                continue;
            }
            Err(_) => {
                write_json(json!({
                    "record_type": "noxa_connection",
                    "state": "connect_timeout",
                    "reconnects": reconnects,
                }))?;
                if sleep_with_deadline(backoff, stop_at).await {
                    break;
                }
                backoff = (backoff * 2).min(Duration::from_secs(5));
                reconnects = reconnects.saturating_add(1);
                continue;
            }
        };
        backoff = Duration::from_millis(250);
        let connected_at = Instant::now();
        let (_, mut read) = stream.split();
        write_json(json!({
            "record_type": "noxa_connection",
            "state": "connected",
            "reconnects": reconnects,
            "received_unix_ns": unix_ns(),
        }))?;
        loop {
            let frame = if let Some(deadline) = stop_at {
                match tokio::time::timeout_at(deadline.into(), read.next()).await {
                    Ok(frame) => frame,
                    Err(_) => break 'observer,
                }
            } else {
                read.next().await
            };
            let Some(frame) = frame else { break };
            let received_unix_ns = unix_ns();
            let frame = match frame {
                Ok(frame) => frame,
                Err(error) => {
                    write_json(json!({
                        "record_type": "noxa_connection",
                        "state": "read_error",
                        "error": error.to_string(),
                        "reconnects": reconnects,
                    }))?;
                    break;
                }
            };
            let payload = match frame {
                Message::Text(text) => text.to_string(),
                Message::Binary(bytes) => {
                    String::from_utf8(bytes.to_vec()).context("binary feed frame was not UTF-8")?
                }
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
            };
            let feed: BroadcastMessage =
                serde_json::from_str(&payload).context("decode Nitro broadcast JSON")?;
            if feed.version != 1 {
                bail!("unsupported Nitro feed version {}", feed.version);
            }
            for message in &feed.messages {
                let sequence_number = message.sequence_number;
                let sequence = sequences.observe(sequence_number);
                let warmup = connected_at.elapsed() < Duration::from_secs(args.warmup_seconds);
                let emission_enabled = !warmup && sequence.is_contiguous();
                let l1_block_number = message.message.message.header.block_number;
                let l1_timestamp = message.message.message.header.timestamp;
                let mut launches = Vec::new();
                let report = decoder.decode_message_with(message, |context| {
                    let tx = context.transaction;
                    if tx.to() != Some(NOXA_LAUNCH_FACTORY) {
                        return;
                    }
                    let Some(header) = decode_launch_header(tx.input(), tx.value()) else {
                        return;
                    };
                    launches.push((*tx.tx_hash(), header));
                })?;
                health_feed_messages = health_feed_messages.saturating_add(1);
                health_signed_transactions =
                    health_signed_transactions.saturating_add(report.signed_transactions as u64);
                for (tx_hash, header) in launches {
                    if emission_enabled {
                        write_json(json!({
                            "record_type": "noxa_factory_call_observed",
                            "observation_semantics": "post_execution_feed",
                            "receipt_verified": false,
                            "received_unix_ns": received_unix_ns,
                            "sequence_number": sequence_number,
                            "l1_block_number": l1_block_number,
                            "l1_timestamp": l1_timestamp,
                            "tx_hash": tx_hash,
                            "header": header,
                            "dynamic_metadata_decoded": false,
                            "recovered_signer": false,
                        }))?;
                        match verifier_slots.clone().try_acquire_owned() {
                            Ok(permit) => {
                                let rpc = rpc.clone();
                                verifiers.spawn(async move {
                                    let _permit = permit;
                                    let record = verify_observed_factory_call(
                                        rpc,
                                        tx_hash,
                                        sequence_number,
                                        l1_block_number,
                                        l1_timestamp,
                                        received_unix_ns,
                                        header,
                                        amount_in,
                                        slippage_bps,
                                        recipient,
                                    )
                                    .await;
                                    let value = match record {
                                        Ok(value) => value,
                                        Err(error) => json!({
                                            "record_type": "noxa_receipt_verification_error",
                                            "tx_hash": tx_hash,
                                            "error": error.to_string(),
                                        }),
                                    };
                                    write_json(value)
                                });
                            }
                            Err(_) => write_json(json!({
                                "record_type": "noxa_receipt_verification_dropped",
                                "tx_hash": tx_hash,
                                "reason": "bounded verifier saturated",
                            }))?,
                        }
                    } else {
                        write_json(json!({
                            "record_type": "noxa_candidate_suppressed",
                            "tx_hash": tx_hash,
                            "warmup": warmup,
                            "sequence": sequence,
                        }))?;
                    }
                }
                if report.signed_transactions > 0 && !sequence.is_contiguous() {
                    // SequenceTracker remains unhealthy for the process lifetime,
                    // so later launch candidates stay fail-closed.
                }
                while let Some(joined) = verifiers.try_join_next() {
                    joined.context("NOXA receipt verifier task panicked")??;
                }
                while let Some(joined) = status_tasks.try_join_next() {
                    joined.context("NOXA factory-status task panicked")??;
                }
                if last_health.elapsed() >= health_interval {
                    write_json(json!({
                        "record_type": "noxa_feed_health",
                        "received_unix_ns": unix_ns(),
                        "sequence": sequences.current(),
                        "reconnects": reconnects,
                        "feed_messages_since_last_health": health_feed_messages,
                        "signed_transactions_since_last_health": health_signed_transactions,
                        "verifier_slots_available": verifier_slots.available_permits(),
                        "rpc": rpc.metrics(),
                    }))?;
                    last_health = Instant::now();
                    health_feed_messages = 0;
                    health_signed_transactions = 0;
                }
                if last_factory_status.elapsed() >= factory_status_interval
                    && status_tasks.is_empty()
                {
                    let rpc = rpc.clone();
                    status_tasks.spawn(async move {
                        let value = match rpc.factory_status().await {
                            Ok(status) => json!({
                                "record_type": "noxa_factory_status_watch",
                                "received_unix_ns": unix_ns(),
                                "runtime_hash_matches_pin": status.runtime_keccak256
                                    == NOXA_FACTORY_RUNTIME_KECCAK256,
                                "can_launch_now": status.launch_enabled
                                    && status.runtime_keccak256
                                        == NOXA_FACTORY_RUNTIME_KECCAK256,
                                "status": status,
                            }),
                            Err(error) => json!({
                                "record_type": "noxa_factory_status_watch_error",
                                "received_unix_ns": unix_ns(),
                                "error": error.to_string(),
                            }),
                        };
                        write_json(value)
                    });
                    last_factory_status = Instant::now();
                }
            }
        }
        reconnects = reconnects.saturating_add(1);
        write_json(json!({
            "record_type": "noxa_connection",
            "state": "disconnected",
            "reconnects": reconnects,
            "received_unix_ns": unix_ns(),
            "sequence": sequences.current(),
            "rpc": rpc.metrics(),
        }))?;
    }
    drain_tasks(&mut verifiers, "NOXA receipt verifiers").await?;
    drain_tasks(&mut status_tasks, "NOXA factory-status tasks").await
}

async fn calibrate_boundary(args: CalibrateBoundaryArgs) -> Result<()> {
    if args.samples == 0 {
        bail!("--samples must be non-zero");
    }
    let (l1_stream, _) = tokio_tungstenite::connect_async(&args.l1_ws_url)
        .await
        .context("connect parent Ethereum websocket")?;
    let (feed_stream, _) = tokio_tungstenite::connect_async(&args.feed_url)
        .await
        .context("connect Robinhood Nitro feed")?;
    let (mut l1_write, mut l1_read) = l1_stream.split();
    let (_, mut feed_read) = feed_stream.split();
    l1_write
        .send(Message::Text(
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "eth_subscribe",
                "params": ["newHeads"],
            })
            .to_string()
            .into(),
        ))
        .await
        .context("subscribe to parent Ethereum newHeads")?;
    let started = Instant::now();
    let stop_at = args
        .run_seconds
        .map(|seconds| Instant::now() + Duration::from_secs(seconds));
    let warmup = Duration::from_secs(args.warmup_seconds);
    let mut sequences = SequenceTracker::default();
    let mut head_arrivals: BTreeMap<u64, (Instant, u128)> = BTreeMap::new();
    let mut matched = HashSet::new();
    let mut samples = Vec::with_capacity(args.samples);
    write_json(json!({
        "record_type": "noxa_boundary_calibration_start",
        "l1_ws_url": args.l1_ws_url,
        "feed_url": args.feed_url,
        "warmup_seconds": args.warmup_seconds,
        "target_samples": args.samples,
        "metric": "parent_new_head_arrival_to_post_execution_nitro_feed",
    }))?;
    while samples.len() < args.samples {
        if stop_at.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        let deadline = stop_at.unwrap_or_else(|| Instant::now() + Duration::from_secs(86_400));
        tokio::select! {
            _ = tokio::time::sleep_until(deadline.into()) => {
                break;
            }
            message = l1_read.next() => {
                let Some(message) = message else {
                    bail!("parent Ethereum websocket closed");
                };
                let message = message.context("read parent Ethereum websocket")?;
                let text = match message {
                    Message::Text(text) => text.to_string(),
                    Message::Binary(bytes) => String::from_utf8(bytes.to_vec())
                        .context("parent websocket binary frame was not UTF-8")?,
                    Message::Close(_) => bail!("parent Ethereum websocket closed"),
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                };
                let value: Value = serde_json::from_str(&text)
                    .context("decode parent Ethereum websocket JSON")?;
                let Some(number) = value
                    .pointer("/params/result/number")
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                let number = parse_hex_u64(number)?;
                head_arrivals.insert(number, (Instant::now(), unix_ns()));
                let retain_from = number.saturating_sub(64);
                head_arrivals.retain(|block, _| *block >= retain_from);
            }
            message = feed_read.next() => {
                let Some(message) = message else {
                    bail!("Robinhood feed websocket closed");
                };
                let received = Instant::now();
                let received_unix_ns = unix_ns();
                let message = message.context("read Robinhood feed websocket")?;
                let text = match message {
                    Message::Text(text) => text.to_string(),
                    Message::Binary(bytes) => String::from_utf8(bytes.to_vec())
                        .context("feed binary frame was not UTF-8")?,
                    Message::Close(_) => bail!("Robinhood feed websocket closed"),
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                };
                let feed: BroadcastMessage = serde_json::from_str(&text)
                    .context("decode Robinhood feed JSON")?;
                if feed.version != 1 {
                    bail!("unsupported Nitro feed version {}", feed.version);
                }
                for message in &feed.messages {
                    let health = sequences.observe(message.sequence_number);
                    if started.elapsed() < warmup || !health.is_contiguous() {
                        continue;
                    }
                    let l1_block = message.message.message.header.block_number;
                    if matched.contains(&l1_block) {
                        continue;
                    }
                    let Some((head_received, head_unix_ns)) = head_arrivals.get(&l1_block) else {
                        continue;
                    };
                    let delta_ns = received
                        .checked_duration_since(*head_received)
                        .map(|duration| duration.as_nanos());
                    let Some(delta_ns) = delta_ns else {
                        continue;
                    };
                    matched.insert(l1_block);
                    samples.push(delta_ns);
                    write_json(json!({
                        "record_type": "noxa_boundary_sample",
                        "l1_block_number": l1_block,
                        "sequence_number": message.sequence_number,
                        "parent_head_received_unix_ns": head_unix_ns,
                        "feed_received_unix_ns": received_unix_ns,
                        "head_to_feed_ns": delta_ns,
                        "sample": samples.len(),
                    }))?;
                    if samples.len() >= args.samples {
                        break;
                    }
                }
            }
        }
    }
    samples.sort_unstable();
    write_json(json!({
        "record_type": "noxa_boundary_calibration_summary",
        "samples": samples.len(),
        "p50_ns": percentile(&samples, 50),
        "p95_ns": percentile(&samples, 95),
        "p99_ns": percentile(&samples, 99),
        "min_ns": samples.first(),
        "max_ns": samples.last(),
        "target_samples": args.samples,
        "run_seconds": args.run_seconds,
        "stopped_by_duration": samples.len() < args.samples && args.run_seconds.is_some(),
        "warning": "This measures the post-execution feed, not the safe earliest send time; predictive submission still needs testnet canaries and an explicit risk budget.",
    }))
}

async fn sleep_with_deadline(duration: Duration, deadline: Option<Instant>) -> bool {
    let sleep_for = deadline
        .map(|deadline| {
            deadline
                .saturating_duration_since(Instant::now())
                .min(duration)
        })
        .unwrap_or(duration);
    if sleep_for.is_zero() {
        return true;
    }
    tokio::time::sleep(sleep_for).await;
    deadline.is_some_and(|deadline| Instant::now() >= deadline)
}

async fn drain_tasks(tasks: &mut JoinSet<Result<()>>, label: &str) -> Result<()> {
    let drain = async {
        while let Some(joined) = tasks.join_next().await {
            joined.with_context(|| format!("{label} task panicked"))??;
        }
        Ok(())
    };
    match tokio::time::timeout(Duration::from_secs(10), drain).await {
        Ok(result) => result,
        Err(_) => {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            bail!("timed out draining {label}")
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn verify_observed_factory_call(
    rpc: NoxaRpcClient,
    tx_hash: B256,
    sequence_number: u64,
    feed_l1_block: u64,
    feed_l1_timestamp: u64,
    observed_unix_ns: u128,
    header: NoxaLaunchHeader,
    amount_in: U256,
    slippage_bps: u16,
    recipient: Option<Address>,
) -> Result<Value> {
    let started = Instant::now();
    let receipt_deadline = started + Duration::from_secs(2);
    let mut receipt = None;
    while Instant::now() < receipt_deadline {
        let remaining = receipt_deadline.saturating_duration_since(Instant::now());
        receipt = tokio::time::timeout(remaining, rpc.receipt(tx_hash))
            .await
            .context("receipt lookup exceeded the two-second visibility deadline")??;
        if receipt.is_some() {
            break;
        }
        let remaining = receipt_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        tokio::time::sleep(remaining.min(Duration::from_millis(25))).await;
    }
    let receipt = receipt.ok_or_else(|| anyhow::anyhow!("receipt not visible after 2 seconds"))?;
    let receipt_visibility_ns = started.elapsed().as_nanos();
    if !receipt.status {
        return Ok(json!({
            "record_type": "noxa_launch_reverted",
            "tx_hash": tx_hash,
            "sequence_number": sequence_number,
            "feed_l1_block": feed_l1_block,
            "receipt_l2_block": receipt.l2_block_number,
            "transaction_index": receipt.transaction_index,
            "receipt_visibility_ns": receipt_visibility_ns,
        }));
    }
    let transaction = rpc
        .transaction_by_hash(tx_hash)
        .await?
        .ok_or_else(|| anyhow::anyhow!("successful launch transaction is not visible"))?;
    if transaction.hash != tx_hash || transaction.to != Some(NOXA_LAUNCH_FACTORY) {
        bail!("verified transaction does not target the canonical NOXA factory");
    }
    let intent = decode_launch_call(&transaction.input, transaction.value).ok_or_else(|| {
        anyhow::anyhow!("successful transaction is not a strict launchToken call")
    })?;
    if intent.launch_config_id != header.launch_config_id
        || intent.dex_id != header.dex_id
        || intent.salt != header.salt
        || intent.transaction_value != header.transaction_value
    {
        bail!("strict launch calldata does not match feed hot-path header");
    }
    let block = rpc.block_by_number(receipt.l2_block_number).await?;
    if block.l1_block_number != feed_l1_block {
        bail!(
            "feed L1 block {} does not match receipt block L1 {}",
            feed_l1_block,
            block.l1_block_number
        );
    }
    let hydrated =
        hydrate_noxa_launch_receipt(&receipt.logs, feed_l1_block, receipt.l2_block_number)?;
    if hydrated.launch.dex_id != intent.dex_id
        || hydrated.launch.launch_config_id != intent.launch_config_id
    {
        bail!("launch calldata and receipt IDs do not match");
    }
    let token_restrictions = rpc
        .token_restriction_snapshot(hydrated.launch.token, receipt.l2_block_number, recipient)
        .await?;
    validate_token_restrictions(&token_restrictions, &hydrated.launch)?;
    let launch_fee = rpc.launch_fee_at(receipt.l2_block_number).await?;
    let provisional_initial_buy = intent
        .transaction_value
        .checked_sub(launch_fee)
        .ok_or_else(|| anyhow::anyhow!("transaction value below launch fee"))?;
    if provisional_initial_buy != hydrated.launch.initial_buy_amount {
        bail!("transaction value minus launch fee does not match launch event");
    }
    let quote = hydrated.pool.quote_exact_input(WETH, amount_in, None)?;
    let restrictions_end = u64::try_from(hydrated.launch.restrictions_end_l1_block)
        .context("restriction end does not fit u64")?;
    let policy = token_restrictions
        .recipient_balance
        .map(|recipient_balance_before| {
            evaluate_noxa_policy(NoxaPolicyInput {
                launch_l1_block: feed_l1_block,
                restrictions_end_l1_block: restrictions_end,
                current_l1_block: feed_l1_block.saturating_add(1),
                recipient_balance_before,
                expected_bought_output: quote.amount_out,
                origin_bought_before: U256::ZERO,
                max_wallet_limit: token_restrictions.max_wallet_limit,
                max_tx_limit: token_restrictions.max_tx_limit,
            })
        });
    let amount_out_minimum = quote
        .amount_out
        .checked_mul(U256::from(10_000_u64 - u64::from(slippage_bps)))
        .ok_or_else(|| anyhow::anyhow!("minimum output overflow"))?
        / U256::from(10_000_u64);
    let calldata = recipient
        .map(|recipient| {
            encode_v3_exact_input_single(&V3ExactInputIntent {
                token_in: WETH,
                token_out: hydrated.launch.token,
                fee: NOXA_POOL_FEE,
                recipient,
                amount_in,
                amount_out_minimum,
                sqrt_price_limit_x96: U256::ZERO,
            })
            .ok_or_else(|| anyhow::anyhow!("could not encode SwapRouter02 call"))
        })
        .transpose()?;
    let conditional = ConditionalOptions::first_eligible_window(
        feed_l1_block,
        3,
        feed_l1_timestamp.checked_add(30),
    )
    .ok_or_else(|| anyhow::anyhow!("conditional boundary overflow"))?;
    Ok(json!({
        "record_type": "noxa_launch_verified_shadow",
        "tx_hash": tx_hash,
        "sequence_number": sequence_number,
        "observed_unix_ns": observed_unix_ns,
        "receipt_visibility_ns": receipt_visibility_ns,
        "verification_total_ns": started.elapsed().as_nanos(),
        "block": block,
        "launch_intent": intent,
        "launch": hydrated.launch,
        "quote": quote,
        "token_restrictions": token_restrictions,
        "policy_at_first_eligible_l1": policy,
        "policy_evaluated": policy.is_some(),
        "prepared": {
            "router": UNISWAP_V3_SWAP_ROUTER_02,
            "amount_in": amount_in,
            "amount_out_minimum": amount_out_minimum,
            "prewrapped_weth_and_prior_approval_required": true,
            "calldata": calldata.map(|bytes| format!("0x{}", hex::encode(bytes))),
            "conditional_safe_fallback": conditional,
            "predictive_primary": {
                "source": "parent_ethereum_new_heads",
                "target_l1_block": feed_l1_block.saturating_add(1),
                "requires_empirical_delay_calibration": true,
                "early_send_can_revert": true,
            },
            "feed_trigger_is_post_execution_fallback": true,
        },
    }))
}

fn parse_u256(value: &str) -> Result<U256> {
    if let Some(hex) = value.strip_prefix("0x") {
        U256::from_str_radix(hex, 16).context("parse hexadecimal U256")
    } else {
        U256::from_str(value).context("parse decimal U256")
    }
}

fn validate_token_restrictions(
    snapshot: &TokenRestrictionSnapshot,
    launch: &NoxaLaunchEvent,
) -> Result<()> {
    if snapshot.token != launch.token
        || snapshot.launch_factory != NOXA_LAUNCH_FACTORY
        || snapshot.liquidity_pool != launch.pool
        || snapshot.pair_token != WETH
        || snapshot.pool_fee != NOXA_POOL_FEE
        || snapshot.restriction_end_block != launch.restrictions_end_l1_block
        || snapshot.max_wallet_limit == U256::ZERO
        || snapshot.max_tx_limit == U256::ZERO
    {
        bail!("launched-token view functions do not match the verified launch receipt");
    }
    if snapshot.recipient.is_some() != snapshot.recipient_balance.is_some() {
        bail!("recipient balance snapshot is incomplete");
    }
    Ok(())
}

fn parse_hex_u64(value: &str) -> Result<u64> {
    u64::from_str_radix(
        value
            .strip_prefix("0x")
            .ok_or_else(|| anyhow::anyhow!("hex quantity lacks 0x prefix"))?,
        16,
    )
    .context("parse hexadecimal u64")
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (percentile.saturating_mul(sorted.len()).saturating_add(99)) / 100;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn unix_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn start_json_writer() -> Result<JoinHandle<Result<()>>> {
    let (sender, receiver) = sync_channel(OUTPUT_QUEUE_CAPACITY);
    JSON_OUTPUT
        .set(sender)
        .map_err(|_| anyhow::anyhow!("JSON writer was already initialized"))?;
    Ok(std::thread::spawn(move || {
        let stdout = std::io::stdout();
        let mut output = BufWriter::new(stdout.lock());
        while let Ok(message) = receiver.recv() {
            match message {
                JsonOutput::Record(value) => {
                    serde_json::to_writer(&mut output, &value)?;
                    output.write_all(b"\n")?;
                    output.flush()?;
                }
                JsonOutput::Shutdown => {
                    output.flush()?;
                    return Ok(());
                }
            }
        }
        output.flush()?;
        Ok(())
    }))
}

fn stop_json_writer(writer: JoinHandle<Result<()>>) -> Result<()> {
    let sender = JSON_OUTPUT
        .get()
        .ok_or_else(|| anyhow::anyhow!("JSON writer was not initialized"))?;
    sender
        .send(JsonOutput::Shutdown)
        .map_err(|_| anyhow::anyhow!("JSON writer stopped before shutdown"))?;
    writer
        .join()
        .map_err(|_| anyhow::anyhow!("JSON writer thread panicked"))?
}

fn write_json(value: Value) -> Result<()> {
    let sender = JSON_OUTPUT
        .get()
        .ok_or_else(|| anyhow::anyhow!("JSON writer was not initialized"))?;
    match sender.try_send(JsonOutput::Record(value)) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(_)) => bail!("JSON output queue saturated; failing closed"),
        Err(TrySendError::Disconnected(_)) => bail!("JSON writer is unavailable"),
    }
}
