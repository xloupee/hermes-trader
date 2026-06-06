use crate::{
    address_lookup::AddressLookupTableCache,
    blockhash::{cached_blockhash, BlockhashCache},
    event::now_ms,
    parser::{
        associated_token_program_id, compute_budget_program_id, read_u64_le, system_program_id,
        Action, FlashxPumpLayout, Route, RouteContext,
    },
    planner::ExecutionPlanLine,
    signal::SignalTimings,
    tx_builder::{
        build_auto_sell_unsigned_flashx_pump_with_cache,
        build_full_copy_unsigned_flashx_pump_with_fees_and_cache_and_spend,
        copy_wallet_token_account_for_flashx_pump, CopyPdaCache, TxBuildError, TxFeeConfig,
    },
    LiveOptions,
};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use solana_hash::Hash;
use solana_keypair::{read_keypair_file, Keypair};
use solana_message::{v0, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction::Transaction;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::task::JoinSet;

const FIRST_LIVE_MAX_COPY_SOL_CAP: f64 = 0.001;
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
const SIGNATURE_FEE_LAMPORTS_ESTIMATE: u64 = 5_000;
const ASSOCIATED_TOKEN_ACCOUNT_RENT_LAMPORTS_ESTIMATE: u64 = 2_100_000;
const SEND_WARM_TIMEOUT_MS: u64 = 750;
const AUTO_SELL_BALANCE_ATTEMPTS: usize = 8;
const AUTO_SELL_BALANCE_RETRY_MS: u64 = 250;
const DIRECT_PUMP_SELL_CONTEXT_CACHE_CAPACITY: usize = 512;

pub(crate) struct CopyExecutor {
    options: CopyExecutionOptions,
    keypair: Option<Keypair>,
    client: reqwest::Client,
    send_endpoints: Arc<Vec<SendEndpoint>>,
    blockhash_cache: Option<BlockhashCache>,
    address_lookup_tables: AddressLookupTableCache,
    pda_cache: CopyPdaCache,
    direct_pump_sell_contexts: Mutex<DirectPumpSellContextCache>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct DirectPumpSellContextKey {
    target_wallet: String,
    mint: String,
}

#[derive(Debug)]
struct DirectPumpSellContextCache {
    capacity: usize,
    entries: HashMap<DirectPumpSellContextKey, RouteContext>,
    order: VecDeque<DirectPumpSellContextKey>,
}

#[derive(Clone, Debug)]
pub(crate) struct CopyExecutionOptions {
    pub(crate) enable_copy_send: bool,
    pub(crate) dry_run: bool,
    pub(crate) simulate_copy_tx: bool,
    pub(crate) fast_copy_send: bool,
    pub(crate) send_fanout: bool,
    pub(crate) send_rpc_urls: Vec<String>,
    pub(crate) jito_send_urls: Vec<String>,
    pub(crate) jito_auth_uuid: Option<String>,
    pub(crate) max_copy_sol: Option<f64>,
    pub(crate) max_total_copy_spend_sol: Option<f64>,
    pub(crate) copy_wallet: Option<String>,
    pub(crate) copy_keypair_path: Option<PathBuf>,
    pub(crate) solana_rpc_url: Option<String>,
    pub(crate) auto_sell_after_buy: bool,
    pub(crate) auto_sell_delay_ms: u64,
    pub(crate) simulate_auto_sell: bool,
    pub(crate) isolate_buy_latency_test: bool,
    pub(crate) send_max_retries: u64,
    pub(crate) send_http_timeout_ms: u64,
    pub(crate) priority_fee_micro_lamports: Option<u64>,
    pub(crate) jito_tip_lamports: Option<u64>,
    pub(crate) jito_tip_account: Option<String>,
    pub(crate) warm_send_endpoints: bool,
    pub(crate) send_endpoint_warm_interval_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CopyExecutionLine {
    schema: &'static str,
    observed_at_ms: u128,
    execution_at_ms: u128,
    provider: &'static str,
    source: &'static str,
    endpoint: String,
    observed_wallet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet: Option<String>,
    observed_signature: String,
    slot: u64,
    selected_route: Route,
    mint: String,
    observed_action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_sol_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    planned_copy_sol_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    planned_copy_spend_lamports: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_copy_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_total_copy_spend_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    estimated_total_copy_spend_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    estimated_total_copy_spend_lamports: Option<u64>,
    send_enabled: bool,
    dry_run: bool,
    simulation_requested: bool,
    fast_copy_send: bool,
    skip_preflight: bool,
    feed_received_at_ms: u128,
    decoded_at_ms: u128,
    matched_at_ms: u128,
    planned_at_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    built_at_ms: Option<u128>,
    feed_received_to_decoded_us: u128,
    decoded_to_matched_us: u128,
    matched_to_planned_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    planned_to_built_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    executor_queue_us: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    guards_us: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unsigned_build_us: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sign_us: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    serialize_us: Option<u128>,
    batch_transaction_count: u64,
    matched_transaction_index: u64,
    batch_scan_us: u128,
    tx_parse_us: u128,
    account_expand_us: u128,
    wallet_match_us: u128,
    route_parse_us: u128,
    send_max_retries: u64,
    send_http_timeout_ms: u64,
    signed: bool,
    simulated: bool,
    sent: bool,
    decision: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    signed_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    simulation_completed_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    send_submitted_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature_returned_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_to_signed_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_to_simulation_completed_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_to_send_submitted_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_to_signature_returned_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    send_lane_ms: Option<u128>,
    slot_delta: Option<i64>,
    tx_delta: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    route_layout: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tx_version: Option<&'static str>,
    instruction_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    blockhash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    simulation_error: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    simulation_units_consumed: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    simulation_logs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    send_signature: Option<String>,
    send_rpc_url_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    send_rpc_winner: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    send_rpc_attempts: Vec<SendRpcAttemptLine>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    send_rpc_errors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    auto_sell_enabled: bool,
    auto_sell_delay_ms: u64,
    auto_sell_simulation_requested: bool,
    #[serde(skip_serializing_if = "is_false")]
    buy_latency_test_isolated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority_fee_micro_lamports: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jito_tip_lamports: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jito_tip_account: Option<String>,
    auto_sell_attempted: bool,
    auto_sell_signed: bool,
    auto_sell_simulated: bool,
    auto_sell_sent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_sell_decision: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_sell_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_sell_token_amount_raw: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_sell_submitted_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_sell_signature_returned_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    buy_signature_to_auto_sell_submitted_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    buy_signature_to_auto_sell_signature_returned_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_sell_copy_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_sell_send_signature: Option<String>,
    auto_sell_send_rpc_url_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_sell_send_rpc_winner: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    auto_sell_send_rpc_attempts: Vec<SendRpcAttemptLine>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    auto_sell_send_rpc_errors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_sell_simulation_error: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_sell_simulation_units_consumed: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    auto_sell_simulation_logs: Vec<String>,
}

#[derive(Debug)]
struct SendTransactionResult {
    signature: String,
    rpc_url_count: usize,
    rpc_winner: String,
    rpc_attempts: Vec<SendRpcAttemptLine>,
    rpc_errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SendRpcAttemptLine {
    label: String,
    kind: &'static str,
    status: &'static str,
    duration_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug)]
struct SendAttemptOutcome {
    attempt: SendRpcAttemptLine,
    signature: Option<String>,
    error: Option<String>,
}

struct SignedCopyTransaction {
    transaction: VersionedTransaction,
    signature: String,
    tx_version: &'static str,
}

#[derive(Clone, Debug)]
struct SendEndpoint {
    label: String,
    url: String,
    kind: SendEndpointKind,
    auth_uuid: Option<String>,
}

#[derive(Clone, Copy, Debug)]
struct SendConfig {
    fast_copy_send: bool,
    max_retries: u64,
    http_timeout_ms: u64,
}

fn sign_copy_transaction(
    instructions: &[solana_instruction::Instruction],
    keypair: &Keypair,
    blockhash: Hash,
    address_lookup_tables: &AddressLookupTableCache,
) -> std::result::Result<SignedCopyTransaction, String> {
    let table_accounts = address_lookup_tables.table_accounts();
    if !table_accounts.is_empty() {
        let message =
            v0::Message::try_compile(&keypair.pubkey(), instructions, &table_accounts, blockhash)
                .map_err(|error| format!("compile v0 message: {error}"))?;
        let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), &[keypair])
            .map_err(|error| format!("sign v0 transaction: {error}"))?;
        let signature = transaction
            .signatures
            .first()
            .map(ToString::to_string)
            .ok_or_else(|| "missing v0 signature".to_string())?;
        return Ok(SignedCopyTransaction {
            transaction,
            signature,
            tx_version: "v0",
        });
    }

    let legacy = Transaction::new_signed_with_payer(
        instructions,
        Some(&keypair.pubkey()),
        &[keypair],
        blockhash,
    );
    let signature = legacy
        .signatures
        .first()
        .map(ToString::to_string)
        .ok_or_else(|| "missing legacy signature".to_string())?;
    Ok(SignedCopyTransaction {
        transaction: VersionedTransaction::from(legacy),
        signature,
        tx_version: "legacy",
    })
}

#[derive(Clone, Copy, Debug)]
enum SendEndpointKind {
    Rpc,
    Jito,
}

impl CopyExecutor {
    pub(crate) fn from_options(
        options: &LiveOptions,
        blockhash_cache: Option<BlockhashCache>,
        address_lookup_tables: AddressLookupTableCache,
    ) -> Result<Self> {
        let execution_options = CopyExecutionOptions {
            enable_copy_send: options.enable_copy_send,
            dry_run: options.dry_run,
            simulate_copy_tx: options.simulate_copy_tx && !options.fast_copy_send,
            fast_copy_send: options.fast_copy_send,
            send_fanout: options.send_fanout,
            send_rpc_urls: normalized_send_rpc_urls(
                &options.send_rpc_urls,
                options.solana_rpc_url.as_deref(),
            ),
            jito_send_urls: normalized_send_rpc_urls(&options.jito_send_urls, None),
            jito_auth_uuid: options
                .jito_auth_uuid
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            max_copy_sol: options.max_copy_sol,
            max_total_copy_spend_sol: options.max_total_copy_spend_sol,
            copy_wallet: options.copy_wallet.clone(),
            copy_keypair_path: options.copy_keypair_path.clone(),
            solana_rpc_url: options.solana_rpc_url.clone(),
            auto_sell_after_buy: options.auto_sell_after_buy,
            auto_sell_delay_ms: options.auto_sell_delay_ms,
            simulate_auto_sell: options.simulate_auto_sell,
            isolate_buy_latency_test: options.isolate_buy_latency_test,
            send_max_retries: options.send_max_retries,
            send_http_timeout_ms: options.send_http_timeout_ms,
            priority_fee_micro_lamports: positive_u64(options.priority_fee_micro_lamports),
            jito_tip_lamports: positive_u64(options.jito_tip_lamports),
            jito_tip_account: options
                .jito_tip_account
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            warm_send_endpoints: options.warm_send_endpoints,
            send_endpoint_warm_interval_ms: options.send_endpoint_warm_interval_ms,
        };

        let keypair = match execution_options.copy_keypair_path.as_ref() {
            Some(path) => Some(
                read_keypair_file(path)
                    .map_err(|error| anyhow::anyhow!("{error}"))
                    .with_context(|| format!("read copy keypair {}", path.display()))?,
            ),
            None => None,
        };

        let send_endpoints = Arc::new(execution_options.selected_send_endpoints());

        Ok(Self {
            options: execution_options,
            keypair,
            client: send_http_client(),
            send_endpoints,
            blockhash_cache,
            address_lookup_tables,
            pda_cache: CopyPdaCache::default(),
            direct_pump_sell_contexts: Mutex::new(DirectPumpSellContextCache::new(
                DIRECT_PUMP_SELL_CONTEXT_CACHE_CAPACITY,
            )),
        })
    }

    pub(crate) async fn warm_send_endpoints_once(&self) {
        if !self.options.enable_copy_send || !self.options.warm_send_endpoints {
            return;
        }

        let endpoints = Arc::clone(&self.send_endpoints);
        if endpoints.is_empty() {
            return;
        }

        let mut warm_set = JoinSet::new();
        for endpoint in endpoints.iter().cloned() {
            let client = self.client.clone();
            warm_set.spawn(async move { warm_send_endpoint(&client, &endpoint).await });
        }

        while let Some(result) = warm_set.join_next().await {
            match result {
                Ok(Ok(_attempt)) => {}
                Ok(Err(error)) => eprintln!("send endpoint warmup failed: {error}"),
                Err(error) => eprintln!("send endpoint warmup join failed: {error}"),
            }
        }
    }

    pub(crate) fn spawn_send_endpoint_warmer(self: Arc<Self>) {
        if !self.options.enable_copy_send
            || !self.options.warm_send_endpoints
            || self.options.send_endpoint_warm_interval_ms == 0
            || self.send_endpoints.is_empty()
        {
            return;
        }

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(
                self.options.send_endpoint_warm_interval_ms,
            ));
            interval.tick().await;
            loop {
                interval.tick().await;
                self.warm_send_endpoints_once().await;
            }
        });
    }

    #[cfg(test)]
    pub(crate) async fn handle(
        &self,
        execution_plan: &ExecutionPlanLine,
        observed_action: Action,
        observed_sol_amount: Option<f64>,
        timings: SignalTimings,
    ) -> CopyExecutionLine {
        self.handle_inner(
            execution_plan,
            observed_action,
            observed_sol_amount,
            timings,
            None,
        )
        .await
    }

    pub(crate) async fn handle_with_executor_enqueued_at(
        &self,
        execution_plan: &ExecutionPlanLine,
        observed_action: Action,
        observed_sol_amount: Option<f64>,
        timings: SignalTimings,
        executor_enqueued_at: Instant,
    ) -> CopyExecutionLine {
        self.handle_inner(
            execution_plan,
            observed_action,
            observed_sol_amount,
            timings,
            Some(executor_enqueued_at),
        )
        .await
    }

    async fn handle_inner(
        &self,
        execution_plan: &ExecutionPlanLine,
        observed_action: Action,
        observed_sol_amount: Option<f64>,
        timings: SignalTimings,
        executor_enqueued_at: Option<Instant>,
    ) -> CopyExecutionLine {
        let executor_started_at = Instant::now();
        let mut line = CopyExecutionLine::new(
            execution_plan,
            observed_action,
            observed_sol_amount,
            &self.options,
            timings,
        );
        if let Some(executor_enqueued_at) = executor_enqueued_at {
            line.executor_queue_us = Some(
                executor_started_at
                    .duration_since(executor_enqueued_at)
                    .as_micros(),
            );
        }
        let guards_started_at = Instant::now();
        macro_rules! skip_guard {
            ($reason:expr) => {{
                line.record_guards_us(guards_started_at.elapsed().as_micros());
                return line.skip($reason);
            }};
        }

        if !self.options.simulate_copy_tx && !self.options.enable_copy_send {
            skip_guard!("copy execution is disabled");
        }

        if !execution_plan.allowed || execution_plan.decision != "wouldBuy" {
            skip_guard!("execution plan is not allowed");
        }

        if observed_action != Action::Buy {
            skip_guard!("copy execution only allows buy signals");
        }

        if execution_plan.route != Route::FlashxPump {
            skip_guard!("unsupported copy execution route");
        }

        let Some(observed_sol_amount) = observed_sol_amount else {
            skip_guard!("observed SOL amount is not confidently bounded");
        };
        if !observed_sol_amount.is_finite() || observed_sol_amount <= 0.0 {
            skip_guard!("observed SOL amount is not confidently bounded");
        }
        let Some(planned_copy_sol_amount) = execution_plan.spend_sol_amount else {
            skip_guard!("missing planned copy SOL amount");
        };
        if !planned_copy_sol_amount.is_finite() || planned_copy_sol_amount <= 0.0 {
            skip_guard!("invalid planned copy SOL amount");
        }
        let Some(copy_spend_lamports) = sol_to_lamports(planned_copy_sol_amount) else {
            skip_guard!("invalid planned copy SOL amount");
        };

        let Some(max_copy_sol) = self.options.max_copy_sol else {
            skip_guard!("missing max copy SOL guard");
        };
        if !max_copy_sol.is_finite() || max_copy_sol <= 0.0 {
            skip_guard!("invalid max copy SOL guard");
        }
        if max_copy_sol > FIRST_LIVE_MAX_COPY_SOL_CAP {
            skip_guard!(format!(
                "max copy SOL guard exceeds first-live cap {FIRST_LIVE_MAX_COPY_SOL_CAP}"
            ));
        }
        if planned_copy_sol_amount > max_copy_sol {
            skip_guard!("planned copy spend exceeds max copy SOL guard");
        }

        let Some(copy_wallet) = self.options.copy_wallet.as_deref() else {
            skip_guard!("missing copy wallet");
        };
        let Some(keypair) = self.keypair.as_ref() else {
            skip_guard!("missing copy keypair path");
        };
        if keypair.pubkey().to_string() != copy_wallet {
            skip_guard!("copy keypair does not match copy wallet");
        }

        let Some(cached_blockhash) = cached_blockhash(self.blockhash_cache.as_ref()) else {
            skip_guard!("missing warm blockhash");
        };

        let prebuild_guards_us = guards_started_at.elapsed().as_micros();
        let unsigned_build_started_at = Instant::now();
        let build = match build_full_copy_unsigned_flashx_pump_with_fees_and_cache_and_spend(
            execution_plan.route_context.as_ref(),
            copy_wallet,
            &execution_plan.mint,
            &self.options.tx_fee_config(),
            Some(&self.pda_cache),
            Some(copy_spend_lamports),
        ) {
            Ok(build) => build,
            Err(error) => {
                line.record_unsigned_build_us(unsigned_build_started_at);
                line.record_guards_us(prebuild_guards_us);
                return line.skip(tx_build_error_reason(error));
            }
        };
        line.record_unsigned_build_us(unsigned_build_started_at);
        line.route_layout = Some(build.route_layout);
        line.instruction_count = build.instructions.len();
        line.mark_built();

        let postbuild_guards_started_at = Instant::now();
        let estimated_total_spend_lamports =
            match estimate_total_copy_spend_lamports(&build, execution_plan.route_context.as_ref())
            {
                Ok(lamports) => lamports,
                Err(reason) => {
                    line.record_guards_us(
                        prebuild_guards_us + postbuild_guards_started_at.elapsed().as_micros(),
                    );
                    return line.skip(reason);
                }
            };
        line.estimated_total_copy_spend_lamports = Some(estimated_total_spend_lamports);
        line.estimated_total_copy_spend_sol = Some(lamports_to_sol(estimated_total_spend_lamports));

        match total_copy_spend_guard_reason(&self.options, estimated_total_spend_lamports) {
            Ok(Some(reason)) => {
                line.record_guards_us(
                    prebuild_guards_us + postbuild_guards_started_at.elapsed().as_micros(),
                );
                return line.skip(reason);
            }
            Ok(None) => {}
            Err(reason) => {
                line.record_guards_us(
                    prebuild_guards_us + postbuild_guards_started_at.elapsed().as_micros(),
                );
                return line.skip(reason);
            }
        }

        let blockhash = match Hash::from_str(&cached_blockhash.blockhash) {
            Ok(blockhash) => blockhash,
            Err(error) => {
                line.record_guards_us(
                    prebuild_guards_us + postbuild_guards_started_at.elapsed().as_micros(),
                );
                return line.skip(format!("invalid cached blockhash: {error}"));
            }
        };
        line.record_guards_us(
            prebuild_guards_us + postbuild_guards_started_at.elapsed().as_micros(),
        );

        let sign_started_at = Instant::now();
        let signed_tx = match sign_copy_transaction(
            &build.instructions,
            keypair,
            blockhash,
            &self.address_lookup_tables,
        ) {
            Ok(signed_tx) => signed_tx,
            Err(error) => {
                line.record_sign_us(sign_started_at);
                return line.error(format!("sign transaction: {error}"));
            }
        };
        line.record_sign_us(sign_started_at);
        let serialize_started_at = Instant::now();
        let tx_bytes = match bincode::serialize(&signed_tx.transaction) {
            Ok(bytes) => bytes,
            Err(error) => {
                line.record_serialize_us(serialize_started_at);
                return line.error(format!("serialize signed transaction: {error}"));
            }
        };
        let encoded_tx = STANDARD.encode(tx_bytes);
        line.record_serialize_us(serialize_started_at);

        line.signed = true;
        line.mark_signed();
        line.copy_signature = Some(signed_tx.signature);
        line.tx_version = Some(signed_tx.tx_version);
        line.blockhash = Some(cached_blockhash.blockhash);

        let mut simulation_ok = true;
        if self.options.simulate_copy_tx {
            match self.simulate_transaction(&encoded_tx).await {
                Ok(simulation) => {
                    line.simulated = true;
                    line.mark_simulation_completed();
                    line.simulation_error = simulation.err;
                    line.simulation_units_consumed = simulation.units_consumed;
                    line.simulation_logs = simulation.logs.unwrap_or_default();
                    simulation_ok = line.simulation_error.is_none();
                }
                Err(error) => {
                    simulation_ok = false;
                    line.simulated = true;
                    line.mark_simulation_completed();
                    line.simulation_error = Some(serde_json::Value::String(error));
                }
            }
        }

        if self.options.enable_copy_send {
            if self.options.dry_run {
                return line.skip("dry run blocks copy send");
            }
            if self.options.simulate_copy_tx && !simulation_ok {
                return line.skip("simulation failed; send blocked");
            }
            line.mark_send_submitted();
            match self.send_transaction(&encoded_tx).await {
                Ok(result) => {
                    line.sent = true;
                    line.mark_signature_returned();
                    line.send_signature = Some(result.signature);
                    line.send_rpc_url_count = result.rpc_url_count;
                    line.send_rpc_winner = Some(result.rpc_winner);
                    line.send_rpc_attempts = result.rpc_attempts;
                    line.send_rpc_errors = result.rpc_errors;
                    line.decision = "sent";
                    line
                }
                Err(error) => line.error(error),
            }
        } else if self.options.simulate_copy_tx {
            if simulation_ok {
                line.decision = "simulated";
                line
            } else {
                line.error("simulation failed")
            }
        } else {
            line.skip("copy send is disabled")
        }
    }

    pub(crate) fn should_spawn_auto_sell_after_buy(&self, line: &CopyExecutionLine) -> bool {
        line.was_sent() && self.options.auto_sell_after_buy_enabled()
    }

    pub(crate) fn observe_direct_pump_sell_route_context(
        &self,
        target_wallet: &str,
        mint: &str,
        route_context: Option<&RouteContext>,
    ) {
        if !self.options.auto_sell_after_buy_enabled() {
            return;
        }
        let Some(route_context) = route_context else {
            return;
        };
        if !is_direct_pump_sell_route_context(route_context) {
            return;
        }
        let Ok(mut cache) = self.direct_pump_sell_contexts.lock() else {
            return;
        };
        cache.insert(target_wallet, mint, route_context.clone());
    }

    pub(crate) async fn handle_auto_sell_result(
        &self,
        mut line: CopyExecutionLine,
        execution_plan: &ExecutionPlanLine,
    ) -> CopyExecutionLine {
        line.execution_at_ms = now_ms();
        let Some(keypair) = self.keypair.as_ref() else {
            line.auto_sell_attempted = true;
            line.skip_auto_sell("missing copy keypair path");
            return line;
        };

        self.handle_auto_sell(&mut line, execution_plan, keypair)
            .await;
        line
    }

    async fn simulate_transaction(&self, encoded_tx: &str) -> Result<SimulationValue, String> {
        let rpc_url = self
            .options
            .solana_rpc_url
            .as_deref()
            .ok_or_else(|| "missing SOLANA_RPC_URL".to_string())?;
        let response = self
            .client
            .post(rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "simulateTransaction",
                "params": [
                    encoded_tx,
                    {
                        "encoding": "base64",
                        "sigVerify": true,
                        "replaceRecentBlockhash": false,
                        "commitment": "processed"
                    }
                ]
            }))
            .send()
            .await
            .map_err(|error| format!("send simulateTransaction request: {error}"))?
            .error_for_status()
            .map_err(|error| format!("simulateTransaction HTTP status: {error}"))?
            .json::<RpcResponse<SimulationResult>>()
            .await
            .map_err(|error| format!("decode simulateTransaction response: {error}"))?;

        if let Some(error) = response.error {
            return Err(format!("simulateTransaction RPC error: {}", error.message));
        }

        response
            .result
            .map(|result| result.value)
            .ok_or_else(|| "simulateTransaction result missing".to_string())
    }

    async fn send_transaction(&self, encoded_tx: &str) -> Result<SendTransactionResult, String> {
        let endpoints = self.send_endpoints.as_ref();
        if endpoints.is_empty() {
            return Err(
                "missing SOLANA_RPC_URL, JITO_SEND_RPC_URLS, or JITO_BLOCK_ENGINE_SEND_URLS"
                    .to_string(),
            );
        }

        if endpoints.len() == 1 {
            let endpoint = &endpoints[0];
            let outcome = send_transaction_attempt(
                &self.client,
                endpoint,
                encoded_tx,
                self.options.send_config(),
            )
            .await;
            let attempts = vec![outcome.attempt];
            let Some(signature) = outcome.signature else {
                return Err(outcome
                    .error
                    .unwrap_or_else(|| "sendTransaction failed".to_string()));
            };
            return Ok(SendTransactionResult {
                signature,
                rpc_url_count: 1,
                rpc_winner: endpoint.label.clone(),
                rpc_attempts: attempts,
                rpc_errors: Vec::new(),
            });
        }

        let encoded_tx = Arc::<str>::from(encoded_tx.to_string());
        let mut send_set = JoinSet::new();
        for endpoint in endpoints {
            let client = self.client.clone();
            let encoded_tx = encoded_tx.clone();
            let endpoint = endpoint.clone();
            let send_config = self.options.send_config();
            send_set.spawn(async move {
                send_transaction_attempt(&client, &endpoint, encoded_tx.as_ref(), send_config).await
            });
        }

        let mut errors = Vec::new();
        let mut attempts = Vec::new();
        while let Some(result) = send_set.join_next().await {
            match result {
                Ok(outcome) => {
                    let label = outcome.attempt.label.clone();
                    attempts.push(outcome.attempt);
                    if let Some(signature) = outcome.signature {
                        // Keep the remaining sends alive. Fast ACK is useful for metrics, but
                        // aborting slower lanes can prevent a better landing path from submitting.
                        send_set.detach_all();
                        return Ok(SendTransactionResult {
                            signature,
                            rpc_url_count: endpoints.len(),
                            rpc_winner: label,
                            rpc_attempts: attempts,
                            rpc_errors: errors,
                        });
                    }
                    if let Some(error) = outcome.error {
                        errors.push(error);
                    }
                }
                Err(error) => errors.push(format!("join error: {error}")),
            }
        }

        Err(format!(
            "all sendTransaction fanout attempts failed: {}",
            errors.join("; ")
        ))
    }

    async fn handle_auto_sell(
        &self,
        line: &mut CopyExecutionLine,
        execution_plan: &ExecutionPlanLine,
        keypair: &Keypair,
    ) {
        line.auto_sell_attempted = true;

        if self.options.dry_run {
            line.skip_auto_sell("dry run blocks auto-sell");
            return;
        }
        if execution_plan.route != Route::FlashxPump {
            line.skip_auto_sell("unsupported auto-sell route");
            return;
        }
        if self.options.auto_sell_delay_ms > 5_000 {
            line.skip_auto_sell("auto-sell delay guard exceeds 5000ms");
            return;
        }

        tokio::time::sleep(Duration::from_millis(self.options.auto_sell_delay_ms)).await;

        let Some(copy_wallet) = self.options.copy_wallet.as_deref() else {
            line.skip_auto_sell("missing copy wallet");
            return;
        };

        let auto_sell_route_context = match auto_sell_route_context_for_plan(self, execution_plan) {
            Ok(route_context) => route_context,
            Err(reason) => {
                line.skip_auto_sell(reason);
                return;
            }
        };

        let token_account = match copy_wallet_token_account_for_flashx_pump(
            Some(&auto_sell_route_context),
            copy_wallet,
            &execution_plan.mint,
            Some(&self.pda_cache),
        ) {
            Ok(token_account) => token_account,
            Err(error) => {
                line.skip_auto_sell(tx_build_error_reason(error));
                return;
            }
        };

        let token_balance_raw = match self
            .auto_sell_token_balance_raw(&token_account.to_string())
            .await
        {
            Ok(amount) if amount > 0 => amount,
            Ok(_) => {
                line.skip_auto_sell("copy wallet token balance is zero after retries");
                return;
            }
            Err(error) => {
                line.error_auto_sell(error);
                return;
            }
        };
        let token_amount_raw =
            auto_sell_token_amount_raw(Some(&auto_sell_route_context), token_balance_raw);
        line.auto_sell_token_amount_raw = Some(token_amount_raw);

        let Some(cached_blockhash) = cached_blockhash(self.blockhash_cache.as_ref()) else {
            line.skip_auto_sell("missing warm blockhash for auto-sell");
            return;
        };
        let blockhash = match Hash::from_str(&cached_blockhash.blockhash) {
            Ok(blockhash) => blockhash,
            Err(error) => {
                line.skip_auto_sell(format!("invalid cached auto-sell blockhash: {error}"));
                return;
            }
        };

        let build = match build_auto_sell_unsigned_flashx_pump_with_cache(
            Some(&auto_sell_route_context),
            copy_wallet,
            &execution_plan.mint,
            token_amount_raw,
            Some(&self.pda_cache),
        ) {
            Ok(build) => build,
            Err(error) => {
                line.skip_auto_sell(tx_build_error_reason(error));
                return;
            }
        };

        let tx = Transaction::new_signed_with_payer(
            &build.instructions,
            Some(&keypair.pubkey()),
            &[keypair],
            blockhash,
        );
        let tx_bytes = match bincode::serialize(&tx) {
            Ok(bytes) => bytes,
            Err(error) => {
                line.error_auto_sell(format!("serialize signed auto-sell transaction: {error}"));
                return;
            }
        };
        let encoded_tx = STANDARD.encode(tx_bytes);
        line.auto_sell_signed = true;
        line.auto_sell_copy_signature = tx.signatures.first().map(ToString::to_string);

        if self.options.simulate_auto_sell {
            match self.simulate_transaction(&encoded_tx).await {
                Ok(simulation) => {
                    line.auto_sell_simulated = true;
                    line.auto_sell_simulation_error = simulation.err;
                    line.auto_sell_simulation_units_consumed = simulation.units_consumed;
                    line.auto_sell_simulation_logs = simulation.logs.unwrap_or_default();
                    if line.auto_sell_simulation_error.is_some() {
                        line.skip_auto_sell("auto-sell simulation failed; send blocked");
                        return;
                    }
                }
                Err(error) => {
                    line.auto_sell_simulated = true;
                    line.auto_sell_simulation_error = Some(serde_json::Value::String(error));
                    line.skip_auto_sell("auto-sell simulation failed; send blocked");
                    return;
                }
            }
        }

        line.mark_auto_sell_submitted();
        match self.send_transaction(&encoded_tx).await {
            Ok(result) => {
                line.auto_sell_sent = true;
                line.mark_auto_sell_signature_returned();
                line.auto_sell_send_signature = Some(result.signature);
                line.auto_sell_send_rpc_url_count = result.rpc_url_count;
                line.auto_sell_send_rpc_winner = Some(result.rpc_winner);
                line.auto_sell_send_rpc_attempts = result.rpc_attempts;
                line.auto_sell_send_rpc_errors = result.rpc_errors;
                line.auto_sell_decision = Some("sent");
            }
            Err(error) => line.error_auto_sell(error),
        }
    }

    async fn auto_sell_token_balance_raw(&self, token_account: &str) -> Result<u64, String> {
        let mut last_error = None;
        for attempt in 0..AUTO_SELL_BALANCE_ATTEMPTS {
            match self.token_account_balance_raw(token_account).await {
                Ok(amount) if amount > 0 => return Ok(amount),
                Ok(_) => last_error = None,
                Err(error) => last_error = Some(error),
            }

            if attempt + 1 < AUTO_SELL_BALANCE_ATTEMPTS {
                tokio::time::sleep(Duration::from_millis(AUTO_SELL_BALANCE_RETRY_MS)).await;
            }
        }

        if let Some(error) = last_error {
            Err(error)
        } else {
            Ok(0)
        }
    }

    async fn token_account_balance_raw(&self, token_account: &str) -> Result<u64, String> {
        let rpc_url = self
            .options
            .solana_rpc_url
            .as_deref()
            .ok_or_else(|| "missing SOLANA_RPC_URL".to_string())?;
        let fetch_balance = async {
            self.client
                .post(rpc_url)
                .json(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "getTokenAccountBalance",
                    "params": [
                        token_account,
                        { "commitment": "processed" }
                    ]
                }))
                .send()
                .await
                .map_err(|error| format!("send getTokenAccountBalance request: {error}"))?
                .error_for_status()
                .map_err(|error| format!("getTokenAccountBalance HTTP status: {error}"))?
                .json::<RpcResponse<TokenAccountBalanceResult>>()
                .await
                .map_err(|error| format!("decode getTokenAccountBalance response: {error}"))
        };
        let response = if self.options.send_http_timeout_ms > 0 {
            tokio::time::timeout(
                Duration::from_millis(self.options.send_http_timeout_ms),
                fetch_balance,
            )
            .await
            .map_err(|_| {
                format!(
                    "getTokenAccountBalance timed out after {}ms",
                    self.options.send_http_timeout_ms
                )
            })??
        } else {
            fetch_balance.await?
        };

        if let Some(error) = response.error {
            return Err(format!(
                "getTokenAccountBalance RPC error: {}",
                error.message
            ));
        }

        let amount = response
            .result
            .ok_or_else(|| "getTokenAccountBalance result missing".to_string())?
            .value
            .amount;
        amount
            .parse::<u64>()
            .map_err(|error| format!("parse token account balance: {error}"))
    }
}

impl DirectPumpSellContextCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn insert(&mut self, target_wallet: &str, mint: &str, route_context: RouteContext) {
        let key = DirectPumpSellContextKey {
            target_wallet: target_wallet.to_string(),
            mint: mint.to_string(),
        };
        if !self.entries.contains_key(&key) {
            self.order.push_back(key.clone());
        }
        self.entries.insert(key, route_context);
        while self.entries.len() > self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }

    fn get(&self, target_wallet: &str, mint: &str) -> Option<RouteContext> {
        let key = DirectPumpSellContextKey {
            target_wallet: target_wallet.to_string(),
            mint: mint.to_string(),
        };
        self.entries.get(&key).cloned()
    }
}

fn auto_sell_route_context_for_plan(
    executor: &CopyExecutor,
    execution_plan: &ExecutionPlanLine,
) -> Result<RouteContext, &'static str> {
    let Some(route_context) = execution_plan.route_context.as_ref() else {
        return Err("missing auto-sell route context");
    };

    if !is_direct_pump_route_context(route_context) {
        return Ok(route_context.clone());
    }

    let Ok(cache) = executor.direct_pump_sell_contexts.lock() else {
        return Err("direct-pump sell-side route context cache unavailable");
    };
    cache
        .get(&execution_plan.target_wallet, &execution_plan.mint)
        .filter(is_direct_pump_sell_route_context)
        .ok_or("missing direct-pump sell-side route context")
}

fn is_direct_pump_route_context(route_context: &RouteContext) -> bool {
    matches!(
        route_context,
        RouteContext::FlashxPump(context) if context.layout == FlashxPumpLayout::DirectPump
    )
}

fn is_direct_pump_sell_route_context(route_context: &RouteContext) -> bool {
    matches!(
        route_context,
        RouteContext::FlashxPump(context)
            if context.layout == FlashxPumpLayout::DirectPump
                && context.data.get(17).copied() == Some(1)
    )
}

fn auto_sell_token_amount_raw(route_context: Option<&RouteContext>, token_balance_raw: u64) -> u64 {
    let Some(RouteContext::FlashxPump(context)) = route_context else {
        return token_balance_raw;
    };

    if context.layout == FlashxPumpLayout::DirectPump {
        return token_balance_raw;
    }

    match read_u64_le(&context.data, 9) {
        Some(min_tokens_out) if min_tokens_out > 0 => token_balance_raw.min(min_tokens_out),
        _ => token_balance_raw,
    }
}

fn estimate_total_copy_spend_lamports(
    build: &crate::tx_builder::FullCopyUnsignedTxBuild,
    route_context: Option<&RouteContext>,
) -> Result<u64, String> {
    let Some(RouteContext::FlashxPump(context)) = route_context else {
        return Err("missing route context for total spend guard".to_string());
    };
    let spendable_sol_in = read_u64_le(&context.data, 1)
        .ok_or_else(|| "missing flashx SOL amount for total spend guard".to_string())?;

    let mut total = spendable_sol_in
        .checked_add(SIGNATURE_FEE_LAMPORTS_ESTIMATE)
        .ok_or_else(|| "estimated total spend overflow".to_string())?;
    total = total
        .checked_add(estimate_priority_fee_lamports(&build.instructions)?)
        .ok_or_else(|| "estimated total spend overflow".to_string())?;
    total = total
        .checked_add(estimate_system_transfer_lamports(&build.instructions)?)
        .ok_or_else(|| "estimated total spend overflow".to_string())?;
    if has_idempotent_associated_token_account_setup(&build.instructions) {
        total = total
            .checked_add(ASSOCIATED_TOKEN_ACCOUNT_RENT_LAMPORTS_ESTIMATE)
            .ok_or_else(|| "estimated total spend overflow".to_string())?;
    }

    Ok(total)
}

fn estimate_priority_fee_lamports(
    instructions: &[solana_instruction::Instruction],
) -> Result<u64, String> {
    let mut compute_unit_limit = 200_000u64;
    let mut compute_unit_price_micro_lamports = 0u64;

    for instruction in instructions
        .iter()
        .filter(|instruction| instruction.program_id == *compute_budget_program_id())
    {
        match instruction.data.first().copied() {
            Some(2) => {
                if let Some(units) = read_u32_le(&instruction.data, 1) {
                    compute_unit_limit = u64::from(units);
                }
            }
            Some(3) => {
                if let Some(price) = read_u64_le(&instruction.data, 1) {
                    compute_unit_price_micro_lamports = price;
                }
            }
            _ => {}
        }
    }

    compute_unit_limit
        .checked_mul(compute_unit_price_micro_lamports)
        .and_then(|micro_lamports| micro_lamports.checked_add(999_999))
        .map(|micro_lamports| micro_lamports / 1_000_000)
        .ok_or_else(|| "estimated priority fee overflow".to_string())
}

fn estimate_system_transfer_lamports(
    instructions: &[solana_instruction::Instruction],
) -> Result<u64, String> {
    let mut total = 0u64;
    for instruction in instructions
        .iter()
        .filter(|instruction| instruction.program_id == *system_program_id())
    {
        if instruction.data.len() >= 12 && read_u32_le(&instruction.data, 0) == Some(2) {
            let lamports = read_u64_le(&instruction.data, 4)
                .ok_or_else(|| "invalid system transfer amount".to_string())?;
            total = total
                .checked_add(lamports)
                .ok_or_else(|| "estimated system transfer overflow".to_string())?;
        }
    }
    Ok(total)
}

fn has_idempotent_associated_token_account_setup(
    instructions: &[solana_instruction::Instruction],
) -> bool {
    instructions.iter().any(|instruction| {
        instruction.program_id == *associated_token_program_id()
            && instruction.data.as_slice() == [1]
    })
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
}

fn sol_to_lamports(value: f64) -> Option<u64> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let lamports = (value * LAMPORTS_PER_SOL).floor();
    if !lamports.is_finite() || lamports <= 0.0 || lamports > u64::MAX as f64 {
        return None;
    }
    Some(lamports as u64)
}

fn lamports_to_sol(lamports: u64) -> f64 {
    lamports as f64 / LAMPORTS_PER_SOL
}

fn total_copy_spend_guard_reason(
    options: &CopyExecutionOptions,
    estimated_total_spend_lamports: u64,
) -> Result<Option<String>, String> {
    match options.max_total_copy_spend_lamports()? {
        Some(max_total_copy_spend_lamports)
            if estimated_total_spend_lamports > max_total_copy_spend_lamports =>
        {
            Ok(Some(format!(
                "estimated total copy spend {} lamports exceeds max total copy spend {} lamports",
                estimated_total_spend_lamports, max_total_copy_spend_lamports
            )))
        }
        _ => Ok(None),
    }
}

impl CopyExecutionLine {
    pub(crate) fn was_sent(&self) -> bool {
        self.sent && self.decision == "sent"
    }

    fn new(
        execution_plan: &ExecutionPlanLine,
        observed_action: Action,
        observed_sol_amount: Option<f64>,
        options: &CopyExecutionOptions,
        timings: SignalTimings,
    ) -> Self {
        Self {
            schema: "copytrade.localExecution.v1",
            observed_at_ms: execution_plan.observed_at_ms,
            execution_at_ms: now_ms(),
            provider: "shredstream",
            source: "jito-proxy",
            endpoint: execution_plan.endpoint.clone(),
            observed_wallet: execution_plan.target_wallet.clone(),
            copy_wallet: options.copy_wallet.clone(),
            observed_signature: execution_plan.signature.clone(),
            slot: execution_plan.slot,
            selected_route: execution_plan.route,
            mint: execution_plan.mint.clone(),
            observed_action,
            observed_sol_amount,
            planned_copy_sol_amount: execution_plan.spend_sol_amount,
            planned_copy_spend_lamports: execution_plan.spend_sol_amount.and_then(sol_to_lamports),
            max_copy_sol: options.max_copy_sol,
            max_total_copy_spend_sol: options.max_total_copy_spend_sol,
            estimated_total_copy_spend_sol: None,
            estimated_total_copy_spend_lamports: None,
            send_enabled: options.enable_copy_send,
            dry_run: options.dry_run,
            simulation_requested: options.simulate_copy_tx,
            fast_copy_send: options.fast_copy_send,
            skip_preflight: options.fast_copy_send,
            feed_received_at_ms: timings.grpc_message_received_at_ms,
            decoded_at_ms: timings.entries_deserialized_at_ms,
            matched_at_ms: timings.wallet_match_finished_at_ms,
            planned_at_ms: execution_plan.planned_at_ms,
            built_at_ms: None,
            feed_received_to_decoded_us: timings.deserialize_us,
            decoded_to_matched_us: timings.wallet_match_finished_at_us,
            matched_to_planned_ms: execution_plan
                .planned_at_ms
                .saturating_sub(timings.wallet_match_finished_at_ms),
            planned_to_built_ms: None,
            executor_queue_us: None,
            guards_us: None,
            unsigned_build_us: None,
            sign_us: None,
            serialize_us: None,
            batch_transaction_count: timings.batch_transaction_count,
            matched_transaction_index: timings.matched_transaction_index,
            batch_scan_us: timings.batch_scan_us,
            tx_parse_us: timings.tx_parse_us,
            account_expand_us: timings.account_expand_us,
            wallet_match_us: timings.wallet_match_us,
            route_parse_us: timings.route_parse_us,
            send_max_retries: options.send_max_retries,
            send_http_timeout_ms: options.send_http_timeout_ms,
            signed: false,
            simulated: false,
            sent: false,
            decision: "skip",
            signed_at_ms: None,
            simulation_completed_at_ms: None,
            send_submitted_at_ms: None,
            signature_returned_at_ms: None,
            observed_to_signed_ms: None,
            observed_to_simulation_completed_ms: None,
            observed_to_send_submitted_ms: None,
            observed_to_signature_returned_ms: None,
            send_lane_ms: None,
            slot_delta: None,
            tx_delta: None,
            route_layout: None,
            tx_version: None,
            instruction_count: 0,
            copy_signature: None,
            blockhash: None,
            simulation_error: None,
            simulation_units_consumed: None,
            simulation_logs: Vec::new(),
            send_signature: None,
            send_rpc_url_count: options.selected_send_rpc_url_count(),
            send_rpc_winner: None,
            send_rpc_attempts: Vec::new(),
            send_rpc_errors: Vec::new(),
            reason: None,
            auto_sell_enabled: options.auto_sell_after_buy_enabled(),
            auto_sell_delay_ms: options.auto_sell_delay_ms,
            auto_sell_simulation_requested: options.simulate_auto_sell_enabled(),
            buy_latency_test_isolated: options.isolate_buy_latency_test,
            priority_fee_micro_lamports: options.priority_fee_micro_lamports,
            jito_tip_lamports: options.jito_tip_lamports,
            jito_tip_account: options.jito_tip_account.clone(),
            auto_sell_attempted: false,
            auto_sell_signed: false,
            auto_sell_simulated: false,
            auto_sell_sent: false,
            auto_sell_decision: None,
            auto_sell_reason: None,
            auto_sell_token_amount_raw: None,
            auto_sell_submitted_at_ms: None,
            auto_sell_signature_returned_at_ms: None,
            buy_signature_to_auto_sell_submitted_ms: None,
            buy_signature_to_auto_sell_signature_returned_ms: None,
            auto_sell_copy_signature: None,
            auto_sell_send_signature: None,
            auto_sell_send_rpc_url_count: options.selected_send_rpc_url_count(),
            auto_sell_send_rpc_winner: None,
            auto_sell_send_rpc_attempts: Vec::new(),
            auto_sell_send_rpc_errors: Vec::new(),
            auto_sell_simulation_error: None,
            auto_sell_simulation_units_consumed: None,
            auto_sell_simulation_logs: Vec::new(),
        }
    }

    fn skip(mut self, reason: impl Into<String>) -> Self {
        self.decision = "skip";
        self.reason = Some(reason.into());
        self
    }

    fn error(mut self, reason: impl Into<String>) -> Self {
        self.decision = "error";
        self.reason = Some(reason.into());
        self
    }

    fn mark_signed(&mut self) {
        let timestamp = now_ms();
        self.signed_at_ms = Some(timestamp);
        self.observed_to_signed_ms = Some(timestamp.saturating_sub(self.observed_at_ms));
    }

    fn mark_built(&mut self) {
        let timestamp = now_ms();
        self.built_at_ms = Some(timestamp);
        self.planned_to_built_ms = Some(timestamp.saturating_sub(self.planned_at_ms));
    }

    fn record_guards_us(&mut self, us: u128) {
        self.guards_us = Some(us);
    }

    fn record_unsigned_build_us(&mut self, started_at: Instant) {
        self.unsigned_build_us = Some(started_at.elapsed().as_micros());
    }

    fn record_sign_us(&mut self, started_at: Instant) {
        self.sign_us = Some(started_at.elapsed().as_micros());
    }

    fn record_serialize_us(&mut self, started_at: Instant) {
        self.serialize_us = Some(started_at.elapsed().as_micros());
    }

    fn mark_simulation_completed(&mut self) {
        let timestamp = now_ms();
        self.simulation_completed_at_ms = Some(timestamp);
        self.observed_to_simulation_completed_ms =
            Some(timestamp.saturating_sub(self.observed_at_ms));
    }

    fn mark_send_submitted(&mut self) {
        let timestamp = now_ms();
        self.send_submitted_at_ms = Some(timestamp);
        self.observed_to_send_submitted_ms = Some(timestamp.saturating_sub(self.observed_at_ms));
    }

    fn mark_signature_returned(&mut self) {
        let timestamp = now_ms();
        self.signature_returned_at_ms = Some(timestamp);
        self.observed_to_signature_returned_ms =
            Some(timestamp.saturating_sub(self.observed_at_ms));
        if let Some(send_submitted_at_ms) = self.send_submitted_at_ms {
            self.send_lane_ms = Some(timestamp.saturating_sub(send_submitted_at_ms));
        }
    }

    fn skip_auto_sell(&mut self, reason: impl Into<String>) {
        self.auto_sell_decision = Some("skip");
        self.auto_sell_reason = Some(reason.into());
    }

    fn error_auto_sell(&mut self, reason: impl Into<String>) {
        self.auto_sell_decision = Some("error");
        self.auto_sell_reason = Some(reason.into());
    }

    fn mark_auto_sell_submitted(&mut self) {
        let timestamp = now_ms();
        self.auto_sell_submitted_at_ms = Some(timestamp);
        if let Some(buy_signature_at_ms) = self.signature_returned_at_ms {
            self.buy_signature_to_auto_sell_submitted_ms =
                Some(timestamp.saturating_sub(buy_signature_at_ms));
        }
    }

    fn mark_auto_sell_signature_returned(&mut self) {
        let timestamp = now_ms();
        self.auto_sell_signature_returned_at_ms = Some(timestamp);
        if let Some(buy_signature_at_ms) = self.signature_returned_at_ms {
            self.buy_signature_to_auto_sell_signature_returned_ms =
                Some(timestamp.saturating_sub(buy_signature_at_ms));
        }
    }
}

impl CopyExecutionOptions {
    fn max_total_copy_spend_lamports(&self) -> Result<Option<u64>, String> {
        self.max_total_copy_spend_sol
            .map(|value| {
                sol_to_lamports(value)
                    .ok_or_else(|| "invalid max total copy spend SOL guard".to_string())
            })
            .transpose()
    }

    fn tx_fee_config(&self) -> TxFeeConfig {
        TxFeeConfig {
            compute_unit_price_micro_lamports: self.priority_fee_micro_lamports,
            jito_tip_lamports: self.jito_tip_lamports,
            jito_tip_account: self.jito_tip_account.clone(),
        }
    }

    fn send_config(&self) -> SendConfig {
        SendConfig {
            fast_copy_send: self.fast_copy_send,
            max_retries: self.send_max_retries,
            http_timeout_ms: self.send_http_timeout_ms,
        }
    }

    fn auto_sell_after_buy_enabled(&self) -> bool {
        self.auto_sell_after_buy && !self.isolate_buy_latency_test
    }

    fn simulate_auto_sell_enabled(&self) -> bool {
        self.simulate_auto_sell && !self.isolate_buy_latency_test
    }

    fn selected_send_rpc_urls(&self) -> Vec<String> {
        let mut urls =
            normalized_send_rpc_urls(&self.send_rpc_urls, self.solana_rpc_url.as_deref());
        if !self.send_fanout {
            urls.truncate(1);
        }
        urls
    }

    fn selected_send_rpc_url_count(&self) -> usize {
        self.selected_send_endpoints().len()
    }

    fn selected_send_endpoints(&self) -> Vec<SendEndpoint> {
        let mut endpoints = self
            .selected_send_rpc_urls()
            .into_iter()
            .enumerate()
            .map(|(index, url)| SendEndpoint {
                label: if index == 0 {
                    format!("rpc-primary:{}", rpc_url_label(&url))
                } else {
                    format!("rpc-fanout-{}:{}", index, rpc_url_label(&url))
                },
                url,
                kind: SendEndpointKind::Rpc,
                auth_uuid: None,
            })
            .collect::<Vec<_>>();

        if self.send_fanout {
            endpoints.extend(self.jito_send_urls.iter().enumerate().map(|(index, url)| {
                let url = jito_transaction_url(url);
                SendEndpoint {
                    label: format!("jito-{}:{}", index + 1, rpc_url_label(&url)),
                    url,
                    kind: SendEndpointKind::Jito,
                    auth_uuid: self.jito_auth_uuid.clone(),
                }
            }));
        }

        endpoints
    }
}

fn positive_u64(value: Option<u64>) -> Option<u64> {
    value.filter(|value| *value > 0)
}

fn normalized_send_rpc_urls(configured: &[String], fallback: Option<&str>) -> Vec<String> {
    let mut urls = Vec::new();
    for value in configured {
        for part in value.split(',') {
            let trimmed = part.trim();
            if !trimmed.is_empty() && !urls.iter().any(|url| url == trimmed) {
                urls.push(trimmed.to_string());
            }
        }
    }

    if urls.is_empty() {
        if let Some(fallback) = fallback {
            let trimmed = fallback.trim();
            if !trimmed.is_empty() {
                urls.push(trimmed.to_string());
            }
        }
    }

    urls
}

fn rpc_url_label(rpc_url: &str) -> String {
    let without_query = rpc_url
        .trim()
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    let after_scheme = without_query
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(without_query);
    let host = after_scheme.split('/').next().unwrap_or("").trim();
    if host.is_empty() {
        "(unknown-rpc)".to_string()
    } else {
        host.to_string()
    }
}

fn jito_transaction_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.ends_with("/api/v1/transactions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/api/v1/transactions")
    }
}

fn send_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_nodelay(true)
        .build()
        .unwrap_or_else(|error| {
            eprintln!("falling back to default reqwest client: {error}");
            reqwest::Client::new()
        })
}

fn send_endpoint_kind(endpoint: &SendEndpoint) -> &'static str {
    match endpoint.kind {
        SendEndpointKind::Rpc => "rpc",
        SendEndpointKind::Jito => "jito",
    }
}

fn send_endpoint_post(
    client: &reqwest::Client,
    endpoint: &SendEndpoint,
) -> reqwest::RequestBuilder {
    let mut request = client.post(&endpoint.url);
    if matches!(endpoint.kind, SendEndpointKind::Jito) {
        if let Some(auth_uuid) = endpoint.auth_uuid.as_deref() {
            request = request.header("x-jito-auth", auth_uuid);
        }
    }
    request
}

async fn warm_send_endpoint(
    client: &reqwest::Client,
    endpoint: &SendEndpoint,
) -> Result<SendRpcAttemptLine, String> {
    let started_at = Instant::now();
    let request = send_endpoint_post(client, endpoint).json(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getHealth"
    }));
    let response =
        tokio::time::timeout(Duration::from_millis(SEND_WARM_TIMEOUT_MS), request.send())
            .await
            .map_err(|_| {
                format!(
                    "{} warmup timed out after {}ms",
                    endpoint.label, SEND_WARM_TIMEOUT_MS
                )
            })?
            .map_err(|error| format!("{} warmup request failed: {error}", endpoint.label))?;

    let _ = response.bytes().await;
    Ok(SendRpcAttemptLine {
        label: endpoint.label.clone(),
        kind: send_endpoint_kind(endpoint),
        status: "warmed",
        duration_ms: started_at.elapsed().as_millis(),
        signature: None,
        error: None,
    })
}

async fn send_transaction_attempt(
    client: &reqwest::Client,
    endpoint: &SendEndpoint,
    encoded_tx: &str,
    config: SendConfig,
) -> SendAttemptOutcome {
    let started_at = Instant::now();
    let send = send_transaction_to(client, endpoint, encoded_tx, config);
    let result = if config.http_timeout_ms > 0 {
        match tokio::time::timeout(Duration::from_millis(config.http_timeout_ms), send).await {
            Ok(result) => result,
            Err(_) => Err(format!(
                "sendTransaction timed out after {}ms",
                config.http_timeout_ms
            )),
        }
    } else {
        send.await
    };

    match result {
        Ok(signature) => {
            let attempt = SendRpcAttemptLine {
                label: endpoint.label.clone(),
                kind: send_endpoint_kind(endpoint),
                status: "submitted",
                duration_ms: started_at.elapsed().as_millis(),
                signature: Some(signature.clone()),
                error: None,
            };
            eprintln!(
                "sendTransaction lane submitted: label={} kind={} durationMs={}",
                attempt.label, attempt.kind, attempt.duration_ms
            );
            SendAttemptOutcome {
                attempt,
                signature: Some(signature),
                error: None,
            }
        }
        Err(error) => {
            let sanitized = send_error_message(endpoint, &error);
            let attempt = SendRpcAttemptLine {
                label: endpoint.label.clone(),
                kind: send_endpoint_kind(endpoint),
                status: "failed",
                duration_ms: started_at.elapsed().as_millis(),
                signature: None,
                error: Some(sanitized.clone()),
            };
            eprintln!(
                "sendTransaction lane failed: label={} kind={} durationMs={} error={}",
                attempt.label, attempt.kind, attempt.duration_ms, sanitized
            );
            SendAttemptOutcome {
                attempt,
                signature: None,
                error: Some(sanitized),
            }
        }
    }
}

async fn send_transaction_to(
    client: &reqwest::Client,
    endpoint: &SendEndpoint,
    encoded_tx: &str,
    config: SendConfig,
) -> Result<String, String> {
    let response = send_endpoint_post(client, endpoint)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [
                encoded_tx,
                {
                    "encoding": "base64",
                    "skipPreflight": config.fast_copy_send,
                    "preflightCommitment": "processed",
                    "maxRetries": config.max_retries
                }
            ]
        }))
        .send()
        .await
        .map_err(|error| format!("send sendTransaction request: {error}"))?
        .error_for_status()
        .map_err(|error| format!("sendTransaction HTTP status: {error}"))?
        .json::<RpcResponse<String>>()
        .await
        .map_err(|error| format!("decode sendTransaction response: {error}"))?;

    if let Some(error) = response.error {
        return Err(format!("sendTransaction RPC error: {}", error.message));
    }

    response
        .result
        .ok_or_else(|| "sendTransaction result missing".to_string())
}

fn send_error_message(endpoint: &SendEndpoint, error: &str) -> String {
    let mut sanitized = error.replace(&endpoint.url, "<redacted-rpc-url>");
    if let Some((base, query)) = endpoint.url.split_once('?') {
        sanitized = sanitized
            .replace(query, "<redacted-query>")
            .replace(base, "<redacted-rpc-url>")
            .replace(base.trim_end_matches('/'), "<redacted-rpc-url>");
    } else {
        sanitized = sanitized.replace(endpoint.url.trim_end_matches('/'), "<redacted-rpc-url>");
    }
    format!("{}: {sanitized}", endpoint.label)
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn tx_build_error_reason(error: TxBuildError) -> &'static str {
    match error {
        TxBuildError::MissingRouteContext(reason)
        | TxBuildError::UnsupportedLayout(reason)
        | TxBuildError::InvalidInstruction(reason) => reason,
    }
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct SimulationResult {
    value: SimulationValue,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimulationValue {
    err: Option<serde_json::Value>,
    logs: Option<Vec<String>>,
    units_consumed: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TokenAccountBalanceResult {
    value: TokenAccountBalanceValue,
}

#[derive(Debug, Deserialize)]
struct TokenAccountBalanceValue {
    amount: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{
        DirectPumpAccounts, FlashxPumpLayout, FlashxPumpResolvedAccounts, MigratedAmmAccounts,
    };
    use crate::planner::{execution_plan_line, PlannerOptions};
    use solana_pubkey::Pubkey;

    const COPY_WALLET: &str = "FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W";

    fn disabled_options() -> CopyExecutionOptions {
        CopyExecutionOptions {
            enable_copy_send: false,
            dry_run: true,
            simulate_copy_tx: false,
            fast_copy_send: false,
            send_fanout: false,
            send_rpc_urls: Vec::new(),
            jito_send_urls: Vec::new(),
            jito_auth_uuid: None,
            max_copy_sol: None,
            max_total_copy_spend_sol: None,
            copy_wallet: None,
            copy_keypair_path: None,
            solana_rpc_url: None,
            auto_sell_after_buy: false,
            auto_sell_delay_ms: 1_000,
            simulate_auto_sell: false,
            isolate_buy_latency_test: false,
            send_max_retries: 3,
            send_http_timeout_ms: 0,
            priority_fee_micro_lamports: None,
            jito_tip_lamports: None,
            jito_tip_account: None,
            warm_send_endpoints: false,
            send_endpoint_warm_interval_ms: 0,
        }
    }

    fn allowed_plan() -> ExecutionPlanLine {
        execution_plan_line(
            &crate::event::ShadowSignalLine {
                schema: "copytrade.shadowSignal.v1",
                observed_at_ms: 1,
                provider: "shredstream",
                source: "jito-proxy",
                endpoint: "local".to_string(),
                target_wallet: "target".to_string(),
                action: Action::Buy,
                mint: "abcPumpIsNotLowerpump".to_string(),
                signature: "observed".to_string(),
                slot: 2,
                route: Route::FlashxPump,
                sol_amount: Some(0.0005),
                token_amount: None,
                copyable: true,
                decision: "wouldCopy",
                reason: None,
                account_key_count: 1,
                route_context: None,
            },
            3,
            PlannerOptions {
                copy_sol_amount: None,
            },
        )
    }

    fn sample_timings() -> SignalTimings {
        SignalTimings {
            grpc_message_received_at_ms: 1,
            entries_deserialized_at_ms: 2,
            wallet_match_finished_at_ms: 3,
            trade_parsed_at_ms: 4,
            deserialize_us: 1_000,
            wallet_match_finished_at_us: 2_000,
            parse_us: 3_000,
            local_detect_us: 4_000,
            batch_transaction_count: 5,
            matched_transaction_index: 1,
            batch_scan_us: 500,
            tx_parse_us: 1_500,
            account_expand_us: 100,
            wallet_match_us: 50,
            route_parse_us: 1_350,
        }
    }

    fn flashx_context(layout: FlashxPumpLayout, min_tokens_out: u64) -> RouteContext {
        let mut data = vec![0];
        data.extend_from_slice(&990_000u64.to_le_bytes());
        data.extend_from_slice(&min_tokens_out.to_le_bytes());
        data.push(match layout {
            FlashxPumpLayout::DirectPump | FlashxPumpLayout::MigratedAmm => 0,
        });

        let dummy: Pubkey = COPY_WALLET.parse().unwrap();
        let flashx_router_program = *crate::parser::flashx_router_program_id();
        let pump_program = *crate::parser::pump_fun_program_id();
        let pump_amm_program = *crate::parser::pump_amm_program_id();
        let resolved_accounts = match layout {
            FlashxPumpLayout::DirectPump => {
                FlashxPumpResolvedAccounts::DirectPump(DirectPumpAccounts {
                    payer: dummy,
                    target_wallet: dummy,
                    flashx_router_program,
                    pump_program,
                    global_config: dummy,
                    fee_recipient: dummy,
                    mint: dummy,
                    bonding_curve: dummy,
                    associated_bonding_curve: dummy,
                    user_token_account: dummy,
                    system_program: *system_program_id(),
                    token_program: dummy,
                    creator_vault: dummy,
                    event_authority: dummy,
                    global_volume_accumulator: Some(dummy),
                    user_volume_accumulator: dummy,
                    fee_config: dummy,
                    fee_program: dummy,
                    bonding_curve_v2: dummy,
                    buyback_fee_recipient: dummy,
                })
            }
            FlashxPumpLayout::MigratedAmm => {
                FlashxPumpResolvedAccounts::MigratedAmm(MigratedAmmAccounts {
                    payer: dummy,
                    target_wallet: dummy,
                    flashx_router_program,
                    pump_amm_program,
                    pool_state: dummy,
                    global_config: dummy,
                    mint: dummy,
                    quote_mint: dummy,
                    user_base_token_account: dummy,
                    user_quote_token_account: dummy,
                    pool_base_token_account: dummy,
                    pool_quote_token_account: dummy,
                    protocol_fee_recipient: dummy,
                    protocol_fee_recipient_token_account: dummy,
                    base_token_program: dummy,
                    quote_token_program: dummy,
                    system_program: *system_program_id(),
                    associated_token_program: *associated_token_program_id(),
                    event_authority: dummy,
                    coin_creator_vault_ata: dummy,
                    coin_creator_vault_authority: dummy,
                    global_volume_accumulator: dummy,
                    user_volume_accumulator: dummy,
                    fee_config: dummy,
                    fee_program: dummy,
                    pool_v2: Some(dummy),
                    buyback_fee_recipient: Some(dummy),
                    buyback_fee_recipient_token_account: Some(dummy),
                })
            }
        };

        RouteContext::FlashxPump(crate::parser::FlashxPumpRouteContext {
            layout,
            program_id: flashx_router_program,
            accounts: Vec::new(),
            data,
            resolved_accounts,
        })
    }

    fn flashx_direct_sell_context() -> RouteContext {
        let mut route_context = flashx_context(FlashxPumpLayout::DirectPump, 1);
        let RouteContext::FlashxPump(context) = &mut route_context;
        context.data[17] = 1;
        route_context
    }

    fn executor(options: CopyExecutionOptions) -> CopyExecutor {
        let send_endpoints = Arc::new(options.selected_send_endpoints());
        CopyExecutor {
            options,
            keypair: None,
            client: send_http_client(),
            send_endpoints,
            blockhash_cache: None,
            address_lookup_tables: AddressLookupTableCache::default(),
            pda_cache: CopyPdaCache::default(),
            direct_pump_sell_contexts: Mutex::new(DirectPumpSellContextCache::new(
                DIRECT_PUMP_SELL_CONTEXT_CACHE_CAPACITY,
            )),
        }
    }

    #[test]
    fn execution_line_defaults_to_safe_disabled_state() {
        let plan = allowed_plan();

        let line = CopyExecutionLine::new(
            &plan,
            Action::Buy,
            Some(0.0005),
            &disabled_options(),
            sample_timings(),
        );

        assert_eq!(line.schema, "copytrade.localExecution.v1");
        assert!(!line.send_enabled);
        assert!(line.dry_run);
        assert!(!line.auto_sell_simulation_requested);
        assert_eq!(line.send_max_retries, 3);
        assert_eq!(line.send_http_timeout_ms, 0);
        assert!(!line.signed);
        assert!(!line.sent);
        assert_eq!(line.signed_at_ms, None);
        assert_eq!(line.observed_to_signed_ms, None);
        assert_eq!(line.observed_to_signature_returned_ms, None);
        assert!(!line.was_sent());
    }

    #[test]
    fn execution_line_records_auto_sell_simulation_request() {
        let plan = allowed_plan();
        let mut options = disabled_options();
        options.simulate_auto_sell = true;

        let line =
            CopyExecutionLine::new(&plan, Action::Buy, Some(0.0005), &options, sample_timings());

        assert!(line.auto_sell_simulation_requested);
    }

    #[test]
    fn buy_latency_isolation_forces_auto_sell_off_for_execution_lines() {
        let plan = allowed_plan();
        let mut options = disabled_options();
        options.auto_sell_after_buy = true;
        options.simulate_auto_sell = true;
        options.isolate_buy_latency_test = true;

        let line =
            CopyExecutionLine::new(&plan, Action::Buy, Some(0.0005), &options, sample_timings());

        assert!(line.buy_latency_test_isolated);
        assert!(!line.auto_sell_enabled);
        assert!(!line.auto_sell_simulation_requested);
    }

    #[test]
    fn direct_pump_auto_sell_uses_cached_sell_side_context() {
        let mut options = disabled_options();
        options.auto_sell_after_buy = true;
        let executor = executor(options);
        let mut plan = allowed_plan();
        plan.route_context = Some(flashx_context(FlashxPumpLayout::DirectPump, 1));

        assert_eq!(
            auto_sell_route_context_for_plan(&executor, &plan).unwrap_err(),
            "missing direct-pump sell-side route context"
        );

        let sell_context = flashx_direct_sell_context();
        executor.observe_direct_pump_sell_route_context(
            &plan.target_wallet,
            &plan.mint,
            Some(&sell_context),
        );
        let route_context =
            auto_sell_route_context_for_plan(&executor, &plan).expect("sell context should cache");

        assert!(is_direct_pump_sell_route_context(&route_context));
    }

    #[test]
    fn max_copy_sol_first_live_cap_is_one_milli_sol() {
        assert_eq!(FIRST_LIVE_MAX_COPY_SOL_CAP, 0.001);
    }

    #[test]
    fn total_copy_spend_estimate_includes_input_setup_fees_and_tip() {
        let route_context = flashx_context(FlashxPumpLayout::MigratedAmm, 1);
        let compute_budget_program = crate::parser::COMPUTE_BUDGET_PROGRAM_ID.parse().unwrap();
        let associated_token_program = crate::parser::ASSOCIATED_TOKEN_PROGRAM_ID.parse().unwrap();
        let system_program = crate::parser::SYSTEM_PROGRAM_ID.parse().unwrap();
        let copy_wallet = COPY_WALLET.parse().unwrap();
        let tip_account = "96gYZGLnUQYgE8MWWpYJw8yRjnvB51rAhbG1SogE3uSG"
            .parse()
            .unwrap();
        let mut compute_limit_data = vec![2];
        compute_limit_data.extend_from_slice(&400_000u32.to_le_bytes());
        let mut compute_price_data = vec![3];
        compute_price_data.extend_from_slice(&250_000u64.to_le_bytes());
        let mut tip_data = Vec::new();
        tip_data.extend_from_slice(&2u32.to_le_bytes());
        tip_data.extend_from_slice(&10_000u64.to_le_bytes());
        let build = crate::tx_builder::FullCopyUnsignedTxBuild {
            route_layout: "migrated-amm",
            copy_wallet_token_account: COPY_WALLET.parse().unwrap(),
            estimated_required_signer: COPY_WALLET.parse().unwrap(),
            setup_instruction_count: 4,
            main_instruction_count: 1,
            instructions: vec![
                solana_instruction::Instruction {
                    program_id: compute_budget_program,
                    accounts: Vec::new(),
                    data: compute_limit_data,
                },
                solana_instruction::Instruction {
                    program_id: compute_budget_program,
                    accounts: Vec::new(),
                    data: compute_price_data,
                },
                solana_instruction::Instruction {
                    program_id: associated_token_program,
                    accounts: vec![solana_instruction::AccountMeta::new(copy_wallet, true)],
                    data: vec![1],
                },
                solana_instruction::Instruction {
                    program_id: system_program,
                    accounts: vec![
                        solana_instruction::AccountMeta::new(copy_wallet, true),
                        solana_instruction::AccountMeta::new(tip_account, false),
                    ],
                    data: tip_data,
                },
            ],
        };

        assert_eq!(
            estimate_total_copy_spend_lamports(&build, Some(&route_context)).unwrap(),
            990_000
                + SIGNATURE_FEE_LAMPORTS_ESTIMATE
                + 100_000
                + 10_000
                + ASSOCIATED_TOKEN_ACCOUNT_RENT_LAMPORTS_ESTIMATE
        );
    }

    #[test]
    fn max_total_copy_spend_sol_must_be_positive_and_finite() {
        let mut options = disabled_options();
        options.max_total_copy_spend_sol = Some(0.0035);
        assert_eq!(
            options.max_total_copy_spend_lamports().unwrap(),
            Some(3_500_000)
        );

        options.max_total_copy_spend_sol = Some(0.0);
        assert_eq!(
            options.max_total_copy_spend_lamports().unwrap_err(),
            "invalid max total copy spend SOL guard"
        );
    }

    #[test]
    fn total_copy_spend_guard_blocks_estimate_above_cap() {
        let mut options = disabled_options();
        options.max_total_copy_spend_sol = Some(0.003);

        assert_eq!(
            total_copy_spend_guard_reason(&options, 3_205_000).unwrap(),
            Some(
                "estimated total copy spend 3205000 lamports exceeds max total copy spend 3000000 lamports"
                    .to_string()
            )
        );

        options.max_total_copy_spend_sol = Some(0.0035);
        assert_eq!(
            total_copy_spend_guard_reason(&options, 3_205_000).unwrap(),
            None
        );
    }

    #[test]
    fn migrated_auto_sell_caps_stale_balance_to_copied_min_out() {
        let route_context = flashx_context(FlashxPumpLayout::MigratedAmm, 23_000_000_000);

        assert_eq!(
            auto_sell_token_amount_raw(Some(&route_context), 47_000_000_000),
            23_000_000_000
        );
    }

    #[test]
    fn migrated_auto_sell_keeps_smaller_balance_when_no_stale_tokens_exist() {
        let route_context = flashx_context(FlashxPumpLayout::MigratedAmm, 23_000_000_000);

        assert_eq!(
            auto_sell_token_amount_raw(Some(&route_context), 22_500_000_000),
            22_500_000_000
        );
    }

    #[test]
    fn direct_auto_sell_uses_copy_wallet_balance() {
        let route_context = flashx_context(FlashxPumpLayout::DirectPump, 23_000_000_000);

        assert_eq!(
            auto_sell_token_amount_raw(Some(&route_context), 47_000_000_000),
            47_000_000_000
        );
    }

    #[test]
    fn direct_auto_sell_keeps_smaller_balance_when_no_stale_tokens_exist() {
        let route_context = flashx_context(FlashxPumpLayout::DirectPump, 23_000_000_000);

        assert_eq!(
            auto_sell_token_amount_raw(Some(&route_context), 22_500_000_000),
            22_500_000_000
        );
    }

    #[test]
    fn send_rpc_urls_fallback_to_primary_rpc() {
        let options = CopyExecutionOptions {
            solana_rpc_url: Some("https://primary.example.com/?api-key=secret".to_string()),
            ..disabled_options()
        };

        assert_eq!(
            options.selected_send_rpc_urls(),
            vec!["https://primary.example.com/?api-key=secret".to_string()]
        );
        assert_eq!(options.selected_send_rpc_url_count(), 1);
    }

    #[test]
    fn send_rpc_fanout_uses_deduped_configured_urls() {
        let mut options = disabled_options();
        options.send_fanout = true;
        options.solana_rpc_url = Some("https://primary.example.com".to_string());
        options.send_rpc_urls = vec![
            " https://first.example.com ".to_string(),
            "https://second.example.com,https://first.example.com".to_string(),
        ];

        assert_eq!(
            options.selected_send_rpc_urls(),
            vec![
                "https://first.example.com".to_string(),
                "https://second.example.com".to_string(),
            ]
        );
        assert_eq!(options.selected_send_rpc_url_count(), 2);
    }

    #[test]
    fn send_rpc_without_fanout_uses_first_configured_url() {
        let mut options = disabled_options();
        options.send_rpc_urls = vec![
            "https://first.example.com".to_string(),
            "https://second.example.com".to_string(),
        ];

        assert_eq!(
            options.selected_send_rpc_urls(),
            vec!["https://first.example.com".to_string()]
        );
        assert_eq!(options.selected_send_rpc_url_count(), 1);
    }

    #[test]
    fn jito_block_engine_urls_join_fanout_when_enabled() {
        let mut options = disabled_options();
        options.send_fanout = true;
        options.solana_rpc_url = Some("https://primary.example.com".to_string());
        options.jito_send_urls = vec![
            "https://frankfurt.mainnet.block-engine.jito.wtf".to_string(),
            "https://london.mainnet.block-engine.jito.wtf/api/v1/transactions".to_string(),
        ];
        options.jito_auth_uuid = Some("uuid".to_string());

        let endpoints = options.selected_send_endpoints();

        assert_eq!(endpoints.len(), 3);
        assert_eq!(endpoints[0].label, "rpc-primary:primary.example.com");
        assert_eq!(
            endpoints[1].url,
            "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/transactions"
        );
        assert_eq!(
            endpoints[2].url,
            "https://london.mainnet.block-engine.jito.wtf/api/v1/transactions"
        );
        assert_eq!(
            endpoints[1].label,
            "jito-1:frankfurt.mainnet.block-engine.jito.wtf"
        );
        assert_eq!(endpoints[1].auth_uuid.as_deref(), Some("uuid"));
        assert_eq!(options.selected_send_rpc_url_count(), 3);
    }

    #[test]
    fn jito_block_engine_urls_are_not_used_without_fanout() {
        let mut options = disabled_options();
        options.send_fanout = false;
        options.solana_rpc_url = Some("https://primary.example.com".to_string());
        options.jito_send_urls =
            vec!["https://frankfurt.mainnet.block-engine.jito.wtf".to_string()];

        let endpoints = options.selected_send_endpoints();

        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].label, "rpc-primary:primary.example.com");
    }

    #[test]
    fn rpc_url_label_removes_secret_query_string() {
        assert_eq!(
            rpc_url_label("https://mainnet.helius-rpc.com/?api-key=secret"),
            "mainnet.helius-rpc.com"
        );
        assert_eq!(
            rpc_url_label("https://rpc.example.com/custom/path?token=secret"),
            "rpc.example.com"
        );
        assert_eq!(
            jito_transaction_url("https://frankfurt.mainnet.block-engine.jito.wtf"),
            "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/transactions"
        );
    }

    #[test]
    fn send_error_message_redacts_rpc_url_queries() {
        let endpoint = SendEndpoint {
            label: "rpc-primary:mainnet.helius-rpc.com".to_string(),
            url: "https://mainnet.helius-rpc.com/?api-key=secret".to_string(),
            kind: SendEndpointKind::Rpc,
            auth_uuid: None,
        };

        let message = send_error_message(
            &endpoint,
            "send sendTransaction request: error sending request for url (https://mainnet.helius-rpc.com/?api-key=secret)",
        );

        assert!(message.contains("rpc-primary:mainnet.helius-rpc.com"));
        assert!(!message.contains("secret"));
        assert!(!message.contains("api-key"));
    }

    #[tokio::test]
    async fn disabled_executor_skips_before_signing() {
        let line = executor(disabled_options())
            .handle(&allowed_plan(), Action::Buy, Some(0.0005), sample_timings())
            .await;

        assert_eq!(line.decision, "skip");
        assert_eq!(line.reason.as_deref(), Some("copy execution is disabled"));
        assert!(!line.signed);
        assert!(!line.sent);
    }

    #[tokio::test]
    async fn max_copy_sol_cap_blocks_unsafe_first_live_value() {
        let mut options = disabled_options();
        options.simulate_copy_tx = true;
        options.max_copy_sol = Some(0.01);
        options.copy_wallet = Some(COPY_WALLET.to_string());

        let line = executor(options)
            .handle(&allowed_plan(), Action::Buy, Some(0.0005), sample_timings())
            .await;

        assert_eq!(line.decision, "skip");
        assert_eq!(
            line.reason.as_deref(),
            Some("max copy SOL guard exceeds first-live cap 0.001")
        );
        assert!(!line.signed);
        assert!(!line.sent);
    }

    #[tokio::test]
    async fn planned_amount_above_guard_blocks_before_keypair() {
        let mut options = disabled_options();
        options.simulate_copy_tx = true;
        options.max_copy_sol = Some(0.0004);
        options.copy_wallet = Some(COPY_WALLET.to_string());

        let line = executor(options)
            .handle(&allowed_plan(), Action::Buy, Some(0.0005), sample_timings())
            .await;

        assert_eq!(line.decision, "skip");
        assert_eq!(
            line.reason.as_deref(),
            Some("planned copy spend exceeds max copy SOL guard")
        );
        assert!(!line.signed);
        assert!(!line.sent);
    }

    #[tokio::test]
    async fn dry_run_blocks_send_even_when_send_flag_is_enabled() {
        let mut options = disabled_options();
        options.enable_copy_send = true;
        options.dry_run = true;
        options.max_copy_sol = Some(0.0005);
        options.copy_wallet = Some(COPY_WALLET.to_string());

        let line = executor(options)
            .handle(&allowed_plan(), Action::Buy, Some(0.0005), sample_timings())
            .await;

        assert_eq!(line.decision, "skip");
        assert_eq!(line.reason.as_deref(), Some("missing copy keypair path"));
        assert!(!line.sent);
    }

    #[test]
    fn was_sent_requires_sent_decision_and_flag() {
        let plan = allowed_plan();
        let mut line = CopyExecutionLine::new(
            &plan,
            Action::Buy,
            Some(0.0005),
            &disabled_options(),
            sample_timings(),
        );

        line.sent = true;
        assert!(!line.was_sent());

        line.decision = "sent";
        assert!(line.was_sent());
    }
}
