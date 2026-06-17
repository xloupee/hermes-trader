use crate::{
    address_lookup::AddressLookupTableCache,
    balance_cache::{WalletBalanceCache, WalletBalanceCheck},
    blockhash::{cached_blockhash, BlockhashCache},
    event::now_ms,
    parser::{
        associated_token_program_id, compute_budget_program_id, pump_fun_program_id, read_u64_le,
        signature_bytes_to_string, system_program_id, Action, FlashxPumpLayout, Route,
        RouteContext,
    },
    planner::{CopyRuntimeRequest, ExecutionPlanLine},
    signal::SignalTimings,
    tx_builder::{
        build_auto_sell_unsigned_flashx_pump_with_fees_and_cache,
        build_full_copy_unsigned_flashx_pump_with_fees_and_cache_and_spend_for_mint,
        build_trailing_sell_unsigned_flashx_pump_with_fees_and_cache,
        copy_wallet_token_account_for_flashx_pump, CopyPdaCache, TxBuildError, TxFeeConfig,
    },
    LiveOptions,
};
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize, Serializer};
use solana_hash::Hash;
use solana_keypair::{read_keypair_file, Keypair};
use solana_message::{v0, VersionedMessage};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction::Transaction;
use std::{
    collections::{HashMap, VecDeque},
    io::Write,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinSet};

pub(crate) const DEFAULT_MIGRATED_AMM_MIN_COPY_SOL: f64 = 0.00099;
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
const SIGNATURE_FEE_LAMPORTS_ESTIMATE: u64 = 5_000;
const ASSOCIATED_TOKEN_ACCOUNT_RENT_LAMPORTS_ESTIMATE: u64 = 2_100_000;
const SEND_WARM_TIMEOUT_MS: u64 = 750;
const HELIUS_SENDER_MIN_TIP_LAMPORTS: u64 = 200_000;
const HELIUS_SENDER_SWQOS_ONLY_MIN_TIP_LAMPORTS: u64 = 5_000;
const AUTO_SELL_BALANCE_ATTEMPTS: usize = 8;
const AUTO_SELL_BALANCE_RETRY_MS: u64 = 250;
const DIRECT_PUMP_CASHBACK_FLAG_OFFSET: usize = 82;
const DIRECT_PUMP_SELL_CONTEXT_CACHE_CAPACITY: usize = 512;
const TRAILING_SELL_MAX_STEPS: usize = 20;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TrailingSellPlan {
    pub(crate) mode: TrailingSellMode,
    pub(crate) percent_basis: TrailingSellPercentBasis,
    pub(crate) steps: Vec<TrailingSellStep>,
    pub(crate) sell_slippage_percent: Option<f64>,
    pub(crate) sell_priority_fee_sol: Option<f64>,
    pub(crate) priority_fee_micro_lamports: Option<u64>,
    pub(crate) jito_tip_lamports: Option<u64>,
    pub(crate) jito_tip_account: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrailingSellMode {
    CustomSteps,
    Formula,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrailingSellPercentBasis {
    RemainingBalance,
    OriginalPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrailingSellStep {
    pub(crate) delay_ms: u64,
    pub(crate) percent: f64,
}

pub(crate) struct CopyExecutor {
    options: CopyExecutionOptions,
    keypair: Option<Arc<Keypair>>,
    keypairs: ArcSwap<HashMap<Pubkey, Arc<Keypair>>>,
    client: reqwest::Client,
    send_endpoints: Arc<Vec<SendEndpoint>>,
    blockhash_cache: Option<BlockhashCache>,
    address_lookup_tables: AddressLookupTableCache,
    wallet_balance_cache: Option<WalletBalanceCache>,
    pda_cache: CopyPdaCache,
    direct_pump_sell_contexts: Mutex<DirectPumpSellContextCache>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct DirectPumpSellContextKey {
    target_wallet: Pubkey,
    mint: Pubkey,
}

#[derive(Debug)]
struct DirectPumpSellContextCache {
    capacity: usize,
    entries: HashMap<DirectPumpSellContextKey, RouteContext>,
    order: VecDeque<DirectPumpSellContextKey>,
}

#[derive(Clone, Debug)]
pub(crate) struct CopyExecutionOptions {
    pub(crate) endpoint: String,
    pub(crate) enable_copy_send: bool,
    pub(crate) dry_run: bool,
    pub(crate) simulate_copy_tx: bool,
    pub(crate) fast_copy_send: bool,
    pub(crate) send_fanout: bool,
    pub(crate) send_lane_mode: SendLaneMode,
    pub(crate) send_rpc_urls: Vec<String>,
    pub(crate) jito_send_urls: Vec<String>,
    pub(crate) jito_auth_uuid: Option<String>,
    pub(crate) helius_sender_enabled: bool,
    pub(crate) helius_sender_urls: Vec<String>,
    pub(crate) helius_sender_swqos_only: bool,
    pub(crate) helius_sender_tip_lamports: Option<u64>,
    pub(crate) helius_sender_tip_account: Option<String>,
    pub(crate) max_copy_sol: Option<f64>,
    pub(crate) max_total_copy_spend_sol: Option<f64>,
    pub(crate) migrated_amm_min_copy_sol: f64,
    pub(crate) migrated_amm_small_copy_mode: MigratedAmmSmallCopyMode,
    pub(crate) copy_wallet: Option<String>,
    pub(crate) copy_keypair_path: Option<PathBuf>,
    pub(crate) solana_rpc_url: Option<String>,
    pub(crate) auto_sell_after_buy: bool,
    pub(crate) auto_sell_delay_ms: u64,
    pub(crate) rust_trailing_sells_enabled: bool,
    pub(crate) rust_trailing_sell_confirmation_timeout_ms: u64,
    pub(crate) rust_trailing_sell_confirmation_poll_ms: u64,
    pub(crate) simulate_auto_sell: bool,
    pub(crate) isolate_buy_latency_test: bool,
    pub(crate) send_max_retries: u64,
    pub(crate) send_http_timeout_ms: u64,
    pub(crate) send_lane_logging: bool,
    pub(crate) priority_fee_micro_lamports: Option<u64>,
    pub(crate) jito_tip_lamports: Option<u64>,
    pub(crate) jito_tip_account: Option<String>,
    pub(crate) sell_priority_fee_micro_lamports: Option<u64>,
    pub(crate) sell_jito_tip_lamports: Option<u64>,
    pub(crate) sell_jito_tip_account: Option<String>,
    pub(crate) warm_send_endpoints: bool,
    pub(crate) send_endpoint_warm_interval_ms: u64,
    pub(crate) copy_wallet_balance_guard: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum MigratedAmmSmallCopyMode {
    Skip,
    Floor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SendLaneMode {
    Mixed,
    RpcOnly,
    JitoOnly,
    HeliusSenderOnly,
}

impl SendLaneMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Mixed => "mixed",
            Self::RpcOnly => "rpc_only",
            Self::JitoOnly => "jito_only",
            Self::HeliusSenderOnly => "helius_sender_only",
        }
    }

    fn uses_rpc_lanes(self) -> bool {
        matches!(self, Self::Mixed | Self::RpcOnly)
    }

    fn uses_jito_lanes(self) -> bool {
        matches!(self, Self::Mixed | Self::JitoOnly)
    }

    fn uses_helius_sender_lanes(self) -> bool {
        matches!(self, Self::Mixed | Self::HeliusSenderOnly)
    }

    fn uses_jito_tip(self) -> bool {
        matches!(self, Self::Mixed | Self::JitoOnly)
    }

    fn uses_helius_sender_tip(self) -> bool {
        matches!(self, Self::Mixed | Self::HeliusSenderOnly)
    }
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
    #[serde(serialize_with = "serialize_pubkey")]
    observed_wallet: Pubkey,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet: Option<String>,
    #[serde(serialize_with = "serialize_signature_bytes")]
    observed_signature: [u8; 64],
    slot: u64,
    selected_route: Route,
    #[serde(serialize_with = "serialize_pubkey")]
    mint: Pubkey,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet_token_account: Option<String>,
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
    copy_wallet_balance_guard: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet_balance_lamports: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet_balance_required_lamports: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet_balance_fetched_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet_balance_age_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet_balance_reason: Option<String>,
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
    send_lane_mode: &'static str,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    helius_sender_tip_lamports: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    helius_sender_tip_account: Option<String>,
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
    auto_sell_send_lane_mode: &'static str,
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

#[derive(Clone, Debug)]
pub(crate) enum CopyExecutionOutput {
    Copy(CopyExecutionLine),
    SendLaneAttribution(SendLaneAttributionLine),
    RustTrailingSell(RustTrailingSellLine),
    TransactionConfirmation(TransactionConfirmationLine),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SendLaneAttributionLine {
    schema: &'static str,
    observed_at_ms: u128,
    attribution_at_ms: u128,
    provider: &'static str,
    source: &'static str,
    endpoint: String,
    observed_wallet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet: Option<String>,
    mint: String,
    transaction_role: &'static str,
    submission_group_id: String,
    observed_signature: String,
    send_signature: String,
    send_lane_mode: &'static str,
    first_ack_lane: String,
    first_ack_at_ms: u128,
    all_attempts: Vec<SendLaneAttemptAttribution>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SendLaneAttemptAttribution {
    label: String,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<&'static str>,
    status: &'static str,
    duration_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    ack_at: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransactionConfirmationLine {
    schema: &'static str,
    observed_at_ms: u128,
    confirmation_at_ms: u128,
    provider: &'static str,
    source: &'static str,
    endpoint: String,
    observed_wallet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet: Option<String>,
    observed_signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_action: Option<Action>,
    slot: u64,
    selected_route: Route,
    #[serde(skip_serializing_if = "Option::is_none")]
    route_layout: Option<&'static str>,
    mint: String,
    transaction_role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    submitted_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature_returned_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    buy_send_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    step_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_steps: Option<usize>,
    checked: bool,
    status: &'static str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    confirmation_slot: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    slot_delta: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    landed_block_tx_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    landed_tx_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_tx_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_tx_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_tx_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    txs_after_observed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    same_slot_tx_delta: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tx_delta: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    block_position_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confirmation_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    err: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RustTrailingSellLine {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    buy_send_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet_token_account: Option<String>,
    slot: u64,
    selected_route: Route,
    mint: String,
    step_index: usize,
    total_steps: usize,
    delay_ms: u64,
    percent: f64,
    percent_basis: TrailingSellPercentBasis,
    mode: TrailingSellMode,
    schedule_anchor_at_ms: u128,
    due_at_ms: u128,
    step_started_at_ms: u128,
    drift_ms: i128,
    sell_slippage_percent: Option<f64>,
    sell_priority_fee_sol: Option<f64>,
    priority_fee_micro_lamports: Option<u64>,
    jito_tip_lamports: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jito_tip_account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    helius_sender_tip_lamports: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    helius_sender_tip_account: Option<String>,
    confirmation_checked: bool,
    confirmation_ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    confirmation_slot: Option<u64>,
    signed: bool,
    simulated: bool,
    sent: bool,
    decision: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    route_layout: Option<&'static str>,
    instruction_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_balance_raw: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_amount_raw: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sell_context_source: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sell_context_resolved_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sell_context_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cashback_detected: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cashback_detection_source: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cashback_sell_path_disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    blockhash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    submitted_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature_returned_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    buy_signature_to_submitted_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    buy_signature_to_signature_returned_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    send_signature: Option<String>,
    send_rpc_url_count: usize,
    send_lane_mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    send_rpc_winner: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    send_rpc_attempts: Vec<SendRpcAttemptLine>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    send_rpc_errors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    simulation_error: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    simulation_units_consumed: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    simulation_logs: Vec<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<&'static str>,
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
    finished_at_ms: u128,
    signature: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct SendLaneAttributionContext {
    observed_at_ms: u128,
    provider: &'static str,
    source: &'static str,
    endpoint: String,
    observed_wallet: Pubkey,
    copy_wallet: Option<String>,
    mint: Pubkey,
    transaction_role: &'static str,
    submission_group_id: String,
    observed_signature: [u8; 64],
    send_lane_mode: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CashbackSellGuard {
    source: &'static str,
}

#[derive(Debug)]
struct SignedCopyTransaction {
    transaction: VersionedTransaction,
    signature: String,
    tx_version: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TxVersionPreference {
    PreferLegacyWhenSafe,
    PreferV0WhenLookupsLoaded,
}

#[derive(Clone, Debug)]
struct SendEndpoint {
    label: String,
    url: String,
    kind: SendEndpointKind,
    auth_uuid: Option<String>,
    sender_mode: Option<&'static str>,
}

#[derive(Clone, Copy, Debug)]
struct SendConfig {
    fast_copy_send: bool,
    max_retries: u64,
    http_timeout_ms: u64,
    log_lanes: bool,
}

fn sign_copy_transaction(
    instructions: &[solana_instruction::Instruction],
    keypair: &Keypair,
    blockhash: Hash,
    address_lookup_tables: &AddressLookupTableCache,
    tx_version_preference: TxVersionPreference,
) -> std::result::Result<SignedCopyTransaction, String> {
    let table_accounts = address_lookup_tables.table_accounts();
    if tx_version_preference == TxVersionPreference::PreferLegacyWhenSafe {
        match sign_legacy_copy_transaction(instructions, keypair, blockhash) {
            Ok(transaction) => return Ok(transaction),
            Err(error) if table_accounts.is_empty() => return Err(error),
            Err(_) => {}
        }
    }

    if !table_accounts.is_empty() {
        return sign_v0_copy_transaction(instructions, keypair, blockhash, table_accounts);
    }

    sign_legacy_copy_transaction(instructions, keypair, blockhash)
}

fn sign_v0_copy_transaction(
    instructions: &[solana_instruction::Instruction],
    keypair: &Keypair,
    blockhash: Hash,
    table_accounts: &[solana_message::AddressLookupTableAccount],
) -> std::result::Result<SignedCopyTransaction, String> {
    let message =
        v0::Message::try_compile(&keypair.pubkey(), instructions, table_accounts, blockhash)
            .map_err(|error| format!("compile v0 message: {error}"))?;
    let transaction = VersionedTransaction::try_new(VersionedMessage::V0(message), &[keypair])
        .map_err(|error| format!("sign v0 transaction: {error}"))?;
    let signature = transaction
        .signatures
        .first()
        .map(ToString::to_string)
        .ok_or_else(|| "missing v0 signature".to_string())?;
    Ok(SignedCopyTransaction {
        transaction,
        signature,
        tx_version: "v0",
    })
}

fn sign_legacy_copy_transaction(
    instructions: &[solana_instruction::Instruction],
    keypair: &Keypair,
    blockhash: Hash,
) -> std::result::Result<SignedCopyTransaction, String> {
    if !legacy_message_account_count_fits(instructions, &keypair.pubkey()) {
        return Err("legacy message account count exceeds limit".to_string());
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

fn legacy_message_account_count_fits(
    instructions: &[solana_instruction::Instruction],
    payer: &Pubkey,
) -> bool {
    let mut account_keys = Vec::new();
    account_keys.push(*payer);
    for instruction in instructions {
        if !account_keys.contains(&instruction.program_id) {
            account_keys.push(instruction.program_id);
        }
        for account in &instruction.accounts {
            if !account_keys.contains(&account.pubkey) {
                account_keys.push(account.pubkey);
            }
        }
        if account_keys.len() > 256 {
            return false;
        }
    }
    true
}

fn tx_version_preference_for_route_layout(route_layout: &str) -> TxVersionPreference {
    if route_layout == "direct-pump" {
        TxVersionPreference::PreferLegacyWhenSafe
    } else {
        TxVersionPreference::PreferV0WhenLookupsLoaded
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SendEndpointKind {
    Rpc,
    Jito,
    HeliusSender,
}

impl CopyExecutor {
    pub(crate) fn from_options(
        options: &LiveOptions,
        blockhash_cache: Option<BlockhashCache>,
        address_lookup_tables: AddressLookupTableCache,
        wallet_balance_cache: Option<WalletBalanceCache>,
        snapshot_keypair_paths: Vec<(Pubkey, PathBuf)>,
    ) -> Result<Self> {
        let execution_options = CopyExecutionOptions {
            endpoint: options.endpoint.clone(),
            enable_copy_send: options.enable_copy_send,
            dry_run: options.dry_run,
            simulate_copy_tx: options.simulate_copy_tx && !options.fast_copy_send,
            fast_copy_send: options.fast_copy_send,
            send_fanout: options.send_fanout,
            send_lane_mode: options.send_lane_mode,
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
            helius_sender_enabled: options.helius_sender_enabled,
            helius_sender_urls: normalized_send_rpc_urls(&options.helius_sender_urls, None),
            helius_sender_swqos_only: options.helius_sender_swqos_only,
            helius_sender_tip_lamports: positive_u64(options.helius_sender_tip_lamports),
            helius_sender_tip_account: options
                .helius_sender_tip_account
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            max_copy_sol: options.max_copy_sol,
            max_total_copy_spend_sol: options.max_total_copy_spend_sol,
            migrated_amm_min_copy_sol: options.migrated_amm_min_copy_sol,
            migrated_amm_small_copy_mode: options.migrated_amm_small_copy_mode,
            copy_wallet: options.copy_wallet.clone(),
            copy_keypair_path: options.copy_keypair_path.clone(),
            solana_rpc_url: options.solana_rpc_url.clone(),
            auto_sell_after_buy: options.auto_sell_after_buy,
            auto_sell_delay_ms: options.auto_sell_delay_ms,
            rust_trailing_sells_enabled: options.rust_trailing_sells_enabled,
            rust_trailing_sell_confirmation_timeout_ms: options
                .rust_trailing_sell_confirmation_timeout_ms,
            rust_trailing_sell_confirmation_poll_ms: options
                .rust_trailing_sell_confirmation_poll_ms,
            simulate_auto_sell: options.simulate_auto_sell,
            isolate_buy_latency_test: options.isolate_buy_latency_test,
            send_max_retries: options.send_max_retries,
            send_http_timeout_ms: options.send_http_timeout_ms,
            send_lane_logging: options.stats,
            priority_fee_micro_lamports: positive_u64(options.priority_fee_micro_lamports),
            jito_tip_lamports: positive_u64(options.jito_tip_lamports),
            jito_tip_account: options
                .jito_tip_account
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            sell_priority_fee_micro_lamports: positive_u64(
                options.sell_priority_fee_micro_lamports,
            )
            .or_else(|| positive_u64(options.priority_fee_micro_lamports)),
            sell_jito_tip_lamports: positive_u64(options.sell_jito_tip_lamports)
                .or_else(|| positive_u64(options.jito_tip_lamports)),
            sell_jito_tip_account: options
                .sell_jito_tip_account
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .or_else(|| {
                    options
                        .jito_tip_account
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string)
                }),
            warm_send_endpoints: options.warm_send_endpoints,
            send_endpoint_warm_interval_ms: options.send_endpoint_warm_interval_ms,
            copy_wallet_balance_guard: options.copy_wallet_balance_guard,
        };
        execution_options
            .validate_helius_sender()
            .map_err(anyhow::Error::msg)?;
        execution_options
            .validate_send_lane_mode()
            .map_err(anyhow::Error::msg)?;

        let keypair = match execution_options.copy_keypair_path.as_ref() {
            Some(path) => Some(Arc::new(
                read_keypair_file(path)
                    .map_err(|error| anyhow::anyhow!("{error}"))
                    .with_context(|| format!("read copy keypair {}", path.display()))?,
            )),
            None => None,
        };
        let keypairs = Self::load_snapshot_keypairs(snapshot_keypair_paths);

        let send_endpoints = Arc::new(execution_options.selected_send_endpoints());

        Ok(Self {
            options: execution_options,
            keypair,
            keypairs: ArcSwap::from_pointee(keypairs),
            client: send_http_client(),
            send_endpoints,
            blockhash_cache,
            address_lookup_tables,
            wallet_balance_cache,
            pda_cache: CopyPdaCache::default(),
            direct_pump_sell_contexts: Mutex::new(DirectPumpSellContextCache::new(
                DIRECT_PUMP_SELL_CONTEXT_CACHE_CAPACITY,
            )),
        })
    }

    pub(crate) fn endpoint(&self) -> &str {
        &self.options.endpoint
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

    pub(crate) fn load_snapshot_keypairs(
        snapshot_keypair_paths: Vec<(Pubkey, PathBuf)>,
    ) -> HashMap<Pubkey, Arc<Keypair>> {
        let mut keypairs = HashMap::new();
        for (copy_wallet, path) in snapshot_keypair_paths {
            let keypair = match read_keypair_file(&path) {
                Ok(keypair) => keypair,
                Err(error) => {
                    eprintln!(
                        "snapshot copy keypair skipped for {} at {}: {}",
                        copy_wallet,
                        path.display(),
                        error
                    );
                    continue;
                }
            };
            if keypair.pubkey() != copy_wallet {
                eprintln!(
                    "snapshot copy keypair skipped for {} at {}: pubkey mismatch {}",
                    copy_wallet,
                    path.display(),
                    keypair.pubkey()
                );
                continue;
            }
            keypairs.insert(copy_wallet, Arc::new(keypair));
        }
        keypairs
    }

    pub(crate) fn replace_snapshot_keypairs(&self, keypairs: HashMap<Pubkey, Arc<Keypair>>) {
        self.keypairs.store(Arc::new(keypairs));
    }

    fn keypair_for_wallet(&self, copy_wallet: &str) -> Option<Arc<Keypair>> {
        let copy_wallet = Pubkey::from_str(copy_wallet).ok()?;
        self.keypair_for_pubkey(&copy_wallet)
    }

    fn keypair_for_pubkey(&self, copy_wallet: &Pubkey) -> Option<Arc<Keypair>> {
        self.keypairs.load().get(copy_wallet).cloned().or_else(|| {
            self.keypair
                .as_ref()
                .filter(|keypair| keypair.pubkey() == *copy_wallet)
                .cloned()
        })
    }

    #[cfg(test)]
    pub(crate) async fn handle(
        &self,
        execution_plan: &ExecutionPlanLine,
        observed_action: Action,
        observed_sol_amount: Option<f64>,
        timings: SignalTimings,
    ) -> CopyExecutionLine {
        let request = CopyRuntimeRequest::from_execution_plan(
            execution_plan,
            observed_action,
            observed_sol_amount,
        );
        self.handle_inner(&request, timings, None, None, None).await
    }

    pub(crate) async fn handle_with_executor_enqueued_at(
        &self,
        request: &CopyRuntimeRequest,
        timings: SignalTimings,
        executor_enqueued_at: Instant,
        copy_wallet: Option<Pubkey>,
        send_lane_attribution_tx: Option<mpsc::UnboundedSender<CopyExecutionOutput>>,
    ) -> CopyExecutionLine {
        self.handle_inner(
            request,
            timings,
            Some(executor_enqueued_at),
            copy_wallet,
            send_lane_attribution_tx,
        )
        .await
    }

    async fn handle_inner(
        &self,
        request: &CopyRuntimeRequest,
        timings: SignalTimings,
        executor_enqueued_at: Option<Instant>,
        copy_wallet_override: Option<Pubkey>,
        send_lane_attribution_tx: Option<mpsc::UnboundedSender<CopyExecutionOutput>>,
    ) -> CopyExecutionLine {
        let executor_started_at = Instant::now();
        let mut line = CopyExecutionLine::new(request, &self.options, timings);
        if let Some(copy_wallet) = copy_wallet_override.as_ref() {
            line.copy_wallet = Some(copy_wallet.to_string());
        }
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

        if !request.allowed || request.execution_decision() != "wouldBuy" {
            skip_guard!("execution plan is not allowed");
        }

        if request.observed_action != Action::Buy {
            skip_guard!("copy execution only allows buy signals");
        }

        if request.route != Route::FlashxPump {
            skip_guard!("unsupported copy execution route");
        }

        let Some(observed_sol_amount) = request.observed_sol_amount else {
            skip_guard!("observed SOL amount is not confidently bounded");
        };
        if !observed_sol_amount.is_finite() || observed_sol_amount <= 0.0 {
            skip_guard!("observed SOL amount is not confidently bounded");
        }
        let Some(planned_copy_sol_amount) = request.planned_copy_sol_amount else {
            skip_guard!("missing planned copy SOL amount");
        };
        if !planned_copy_sol_amount.is_finite() || planned_copy_sol_amount <= 0.0 {
            skip_guard!("invalid planned copy SOL amount");
        }
        let Some(copy_spend_lamports) = sol_to_lamports(planned_copy_sol_amount) else {
            skip_guard!("invalid planned copy SOL amount");
        };
        let effective_copy_spend_lamports = match copy_spend_after_migrated_amm_guard(
            &self.options,
            request.route_context.as_ref(),
            copy_spend_lamports,
        ) {
            Ok(CopySpendDecision::Use(lamports)) => lamports,
            Ok(CopySpendDecision::Skip(reason)) | Err(reason) => skip_guard!(reason),
        };
        let effective_planned_copy_sol_amount = lamports_to_sol(effective_copy_spend_lamports);
        if effective_copy_spend_lamports != copy_spend_lamports {
            line.planned_copy_sol_amount = Some(effective_planned_copy_sol_amount);
            line.planned_copy_spend_lamports = Some(effective_copy_spend_lamports);
        }

        match max_copy_sol_guard_reason(
            self.options.max_copy_sol,
            effective_planned_copy_sol_amount,
        ) {
            Ok(Some(reason)) => skip_guard!(reason),
            Ok(None) => {}
            Err(reason) => skip_guard!(reason),
        }

        let copy_wallet_string;
        let (copy_wallet, keypair) = if let Some(copy_wallet_pubkey) = copy_wallet_override.as_ref()
        {
            copy_wallet_string = copy_wallet_pubkey.to_string();
            (
                copy_wallet_string.as_str(),
                self.keypair_for_pubkey(copy_wallet_pubkey),
            )
        } else {
            let Some(copy_wallet) = self.options.copy_wallet.as_deref() else {
                skip_guard!("missing copy wallet");
            };
            (copy_wallet, self.keypair_for_wallet(copy_wallet))
        };
        let Some(keypair) = keypair else {
            if self.keypair.is_none() && self.keypairs.load().is_empty() {
                skip_guard!("missing copy keypair path");
            }
            skip_guard!("missing copy keypair for copy wallet");
        };

        let Some(cached_blockhash) = cached_blockhash(self.blockhash_cache.as_ref()) else {
            skip_guard!("missing warm blockhash");
        };

        let prebuild_guards_us = guards_started_at.elapsed().as_micros();
        let unsigned_build_started_at = Instant::now();
        let build =
            match build_full_copy_unsigned_flashx_pump_with_fees_and_cache_and_spend_for_mint(
                request.route_context.as_ref(),
                copy_wallet,
                &request.mint,
                &self.options.tx_fee_config(),
                Some(&self.pda_cache),
                Some(effective_copy_spend_lamports),
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
        line.copy_wallet_token_account = Some(build.copy_wallet_token_account.to_string());
        line.mark_built();

        let postbuild_guards_started_at = Instant::now();
        let estimated_total_spend_lamports =
            match estimate_total_copy_spend_lamports(&build, request.route_context.as_ref()) {
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

        if self.options.copy_wallet_balance_guard {
            let balance_check = match &self.wallet_balance_cache {
                Some(cache) => cache.check(copy_wallet, estimated_total_spend_lamports),
                None => WalletBalanceCheck {
                    wallet: copy_wallet.to_string(),
                    lamports: None,
                    fetched_at_ms: None,
                    age_ms: None,
                    required_lamports: estimated_total_spend_lamports,
                    reason: Some("copy wallet balance cache unavailable".to_string()),
                },
            };
            let balance_reason = balance_check.reason.clone();
            line.record_balance_check(balance_check);
            if let Some(reason) = balance_reason {
                line.record_guards_us(
                    prebuild_guards_us + postbuild_guards_started_at.elapsed().as_micros(),
                );
                return line.skip(reason);
            }
        }

        let blockhash = cached_blockhash.hash;
        line.record_guards_us(
            prebuild_guards_us + postbuild_guards_started_at.elapsed().as_micros(),
        );

        let sign_started_at = Instant::now();
        let signed_tx = match sign_copy_transaction(
            &build.instructions,
            &keypair,
            blockhash,
            &self.address_lookup_tables,
            tx_version_preference_for_route_layout(build.route_layout),
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
        line.copy_signature = Some(signed_tx.signature.clone());
        line.tx_version = Some(signed_tx.tx_version);
        line.blockhash = Some(blockhash.to_string());

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
            let attribution_context = SendLaneAttributionContext {
                observed_at_ms: line.observed_at_ms,
                provider: line.provider,
                source: line.source,
                endpoint: line.endpoint.clone(),
                observed_wallet: line.observed_wallet,
                copy_wallet: line.copy_wallet.clone(),
                mint: line.mint,
                transaction_role: "copy_buy",
                submission_group_id: signed_tx.signature.clone(),
                observed_signature: line.observed_signature,
                send_lane_mode: self.options.send_lane_mode.as_str(),
            };
            match self
                .send_transaction(
                    &encoded_tx,
                    Some(attribution_context),
                    send_lane_attribution_tx,
                )
                .await
            {
                Ok(result) => {
                    line.sent = true;
                    line.mark_signature_returned();
                    if let Some(cache) = &self.wallet_balance_cache {
                        cache.optimistic_decrement(copy_wallet, estimated_total_spend_lamports);
                    }
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

    pub(crate) fn should_spawn_trailing_sells_after_buy(
        &self,
        line: &CopyExecutionLine,
        trailing_sell_plan: Option<&TrailingSellPlan>,
    ) -> bool {
        line.was_sent()
            && self.options.rust_trailing_sells_enabled()
            && trailing_sell_plan
                .map(|plan| !effective_trailing_sell_steps(plan).is_empty())
                .unwrap_or(false)
    }

    pub(crate) fn should_spawn_auto_sell_on_target_sell(
        &self,
        request: &CopyRuntimeRequest,
    ) -> bool {
        request.observed_action == Action::Sell
            && self.options.auto_sell_after_buy_enabled()
            && request
                .route_context
                .as_ref()
                .is_some_and(is_direct_pump_sell_route_context)
    }

    pub(crate) fn observe_direct_pump_sell_route_context(
        &self,
        target_wallet: &Pubkey,
        mint: &Pubkey,
        route_context: Option<&RouteContext>,
    ) {
        if !self.options.auto_sell_after_buy_enabled()
            && !self.options.rust_trailing_sells_enabled()
        {
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
        let Some(copy_wallet) = line.copy_wallet.as_deref() else {
            line.auto_sell_attempted = true;
            line.skip_auto_sell("missing copy wallet");
            return line;
        };
        let Some(keypair) = self.keypair_for_wallet(copy_wallet) else {
            line.auto_sell_attempted = true;
            line.skip_auto_sell("missing copy keypair for copy wallet");
            return line;
        };

        self.handle_auto_sell(&mut line, execution_plan, &keypair, true)
            .await;
        line
    }

    pub(crate) async fn handle_target_sell_auto_sell_result(
        &self,
        mut line: CopyExecutionLine,
        execution_plan: &ExecutionPlanLine,
    ) -> CopyExecutionLine {
        line.execution_at_ms = now_ms();
        let Some(copy_wallet) = line.copy_wallet.as_deref() else {
            line.auto_sell_attempted = true;
            line.skip_auto_sell("missing copy wallet");
            return line;
        };
        let Some(keypair) = self.keypair_for_wallet(copy_wallet) else {
            line.auto_sell_attempted = true;
            line.skip_auto_sell("missing copy keypair for copy wallet");
            return line;
        };

        self.handle_auto_sell(&mut line, execution_plan, &keypair, false)
            .await;
        line
    }

    pub(crate) async fn handle_trailing_sell_results(
        self: Arc<Self>,
        buy_line: CopyExecutionLine,
        execution_plan: ExecutionPlanLine,
        trailing_sell_plan: TrailingSellPlan,
        copy_execution_tx: tokio::sync::mpsc::UnboundedSender<CopyExecutionOutput>,
    ) {
        let steps = effective_trailing_sell_steps(&trailing_sell_plan);
        if steps.is_empty() {
            return;
        }

        let anchor_at_ms = buy_line
            .signature_returned_at_ms
            .or(buy_line.send_submitted_at_ms)
            .unwrap_or_else(now_ms);
        let observed_signature = buy_line.observed_signature_string();
        let confirmation = self
            .wait_for_signature_confirmation(
                buy_line.send_signature.as_deref(),
                Some(&observed_signature),
                "copy buy transaction",
                self.options.rust_trailing_sell_confirmation_timeout_ms,
                self.options.rust_trailing_sell_confirmation_poll_ms,
            )
            .await;

        if !confirmation.ok {
            let mut line = RustTrailingSellLine::new(
                &buy_line,
                &trailing_sell_plan,
                0,
                steps.len(),
                steps[0],
                anchor_at_ms,
                &self.options,
            );
            line.confirmation_checked = confirmation.checked;
            line.confirmation_ok = false;
            line.confirmation_slot = confirmation.slot;
            line.skip(confirmation.reason.unwrap_or_else(|| {
                "copy buy was not confirmed before trailing sell scheduling timeout".to_string()
            }));
            if copy_execution_tx
                .send(CopyExecutionOutput::RustTrailingSell(line))
                .is_err()
            {
                eprintln!("rust trailing sell result dropped; receiver closed");
            }
            return;
        }

        let mut optimistic_sellable_balance_raw: Option<u64> = None;
        for (index, step) in steps.iter().copied().enumerate() {
            let due_at_ms = anchor_at_ms.saturating_add(u128::from(step.delay_ms));
            let now = now_ms();
            if due_at_ms > now {
                tokio::time::sleep(Duration::from_millis((due_at_ms - now) as u64)).await;
            }

            let mut line = RustTrailingSellLine::new(
                &buy_line,
                &trailing_sell_plan,
                index,
                steps.len(),
                step,
                anchor_at_ms,
                &self.options,
            );
            line.confirmation_checked = confirmation.checked;
            line.confirmation_ok = true;
            line.confirmation_slot = confirmation.slot;

            self.handle_trailing_sell_step(
                &mut line,
                &execution_plan,
                &trailing_sell_plan,
                optimistic_sellable_balance_raw,
            )
            .await;
            if line.sent {
                optimistic_sellable_balance_raw = update_optimistic_trailing_sell_balance_raw(
                    optimistic_sellable_balance_raw,
                    line.token_balance_raw,
                    line.token_amount_raw,
                );
            }

            if copy_execution_tx
                .send(CopyExecutionOutput::RustTrailingSell(line))
                .is_err()
            {
                eprintln!("rust trailing sell result dropped; receiver closed");
                return;
            }
        }
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

    async fn send_transaction(
        &self,
        encoded_tx: &str,
        attribution_context: Option<SendLaneAttributionContext>,
        attribution_tx: Option<mpsc::UnboundedSender<CopyExecutionOutput>>,
    ) -> Result<SendTransactionResult, String> {
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
            let first_ack_at_ms = outcome.finished_at_ms;
            if let (Some(context), Some(tx)) = (attribution_context, attribution_tx) {
                spawn_send_lane_attribution_collector(
                    JoinSet::new(),
                    context,
                    tx,
                    signature.clone(),
                    endpoint.label.clone(),
                    first_ack_at_ms,
                    attempts.clone(),
                    Vec::new(),
                );
            }
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

        let mut errors: Vec<String> = Vec::new();
        let mut attempts = Vec::new();
        while let Some(result) = send_set.join_next().await {
            match result {
                Ok(outcome) => {
                    let label = outcome.attempt.label.clone();
                    attempts.push(outcome.attempt);
                    if let Some(signature) = outcome.signature {
                        let first_ack_at_ms = outcome.finished_at_ms;
                        if let (Some(context), Some(tx)) = (attribution_context, attribution_tx) {
                            spawn_send_lane_attribution_collector(
                                send_set,
                                context,
                                tx,
                                signature.clone(),
                                label.clone(),
                                first_ack_at_ms,
                                attempts.clone(),
                                errors
                                    .iter()
                                    .filter(|error| error.starts_with("join error:"))
                                    .cloned()
                                    .collect(),
                            );
                        } else {
                            // Keep the remaining sends alive. Fast ACK is useful for metrics, but
                            // aborting slower lanes can prevent a better landing path from submitting.
                            send_set.detach_all();
                        }
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
        sleep_before_sell: bool,
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
        if sleep_before_sell {
            if self.options.auto_sell_delay_ms > 5_000 {
                line.skip_auto_sell("auto-sell delay guard exceeds 5000ms");
                return;
            }

            tokio::time::sleep(Duration::from_millis(self.options.auto_sell_delay_ms)).await;
        }

        let Some(copy_wallet) = line.copy_wallet.as_deref() else {
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
        let blockhash = cached_blockhash.hash;

        let build = match build_auto_sell_unsigned_flashx_pump_with_fees_and_cache(
            Some(&auto_sell_route_context),
            copy_wallet,
            &execution_plan.mint,
            token_amount_raw,
            &self.options.sell_tx_fee_config(),
            Some(&self.pda_cache),
        ) {
            Ok(build) => build,
            Err(error) => {
                line.skip_auto_sell(tx_build_error_reason(error));
                return;
            }
        };
        line.route_layout = Some(build.route_layout);
        line.instruction_count = build.instructions.len();
        line.mark_built();

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
        match self.send_transaction(&encoded_tx, None, None).await {
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

    async fn handle_trailing_sell_step(
        &self,
        line: &mut RustTrailingSellLine,
        execution_plan: &ExecutionPlanLine,
        trailing_sell_plan: &TrailingSellPlan,
        optimistic_sellable_balance_raw: Option<u64>,
    ) {
        if self.options.dry_run {
            line.skip("dry run blocks rust trailing sell");
            return;
        }
        if execution_plan.route != Route::FlashxPump {
            line.skip("unsupported rust trailing sell route");
            return;
        }
        let Some(copy_wallet) = line.copy_wallet.as_deref() else {
            line.skip("missing copy wallet");
            return;
        };
        let Some(keypair) = self.keypair_for_wallet(copy_wallet) else {
            line.skip("missing copy keypair for copy wallet");
            return;
        };

        let sell_context =
            match trailing_sell_route_context_for_plan(self, execution_plan, copy_wallet) {
                Ok(context) => context,
                Err(missing) => {
                    line.sell_context_source = Some(missing.source);
                    line.sell_context_reason = Some(missing.reason.to_string());
                    line.skip(missing.reason);
                    return;
                }
            };
        line.sell_context_source = Some(sell_context.source);
        line.sell_context_resolved_at_ms = Some(sell_context.resolved_at_ms);
        line.sell_context_reason = Some(sell_context.reason.to_string());

        match self
            .cashback_sell_guard_for_route_context(&sell_context.route_context)
            .await
        {
            Ok(Some(guard)) => {
                line.cashback_detected = Some(true);
                line.cashback_detection_source = Some(guard.source);
                line.cashback_sell_path_disabled = Some(true);
                line.skip("cashback-enabled sell path disabled");
                return;
            }
            Ok(None) => {
                line.cashback_detected = Some(false);
            }
            Err(reason) => {
                line.cashback_detection_source = Some("direct-pump-bonding-curve-rpc");
                line.cashback_sell_path_disabled = Some(true);
                line.skip(reason);
                return;
            }
        }

        let token_account = match copy_wallet_token_account_for_flashx_pump(
            Some(&sell_context.route_context),
            copy_wallet,
            &execution_plan.mint,
            Some(&self.pda_cache),
        ) {
            Ok(token_account) => token_account,
            Err(error) => {
                line.skip(tx_build_error_reason(error));
                return;
            }
        };
        line.copy_wallet_token_account = Some(token_account.to_string());

        let token_balance_raw = match self
            .auto_sell_token_balance_raw(&token_account.to_string())
            .await
        {
            Ok(amount) if amount > 0 => amount,
            Ok(_) => {
                line.skip("copy wallet token balance is zero after retries");
                return;
            }
            Err(error) => {
                line.error(error);
                return;
            }
        };
        line.token_balance_raw = Some(token_balance_raw);
        let mut sellable_balance_raw = token_balance_raw;
        if let Some(optimistic_sellable_balance_raw) = optimistic_sellable_balance_raw {
            sellable_balance_raw = sellable_balance_raw.min(optimistic_sellable_balance_raw);
            line.token_balance_raw = Some(sellable_balance_raw);
        }
        let token_amount_raw = trailing_sell_token_amount_raw(sellable_balance_raw, line.percent);
        if token_amount_raw == 0 {
            line.skip("rust trailing sell token amount rounds to zero");
            return;
        }
        line.token_amount_raw = Some(token_amount_raw);

        let Some(cached_blockhash) = cached_blockhash(self.blockhash_cache.as_ref()) else {
            line.skip("missing warm blockhash for rust trailing sell");
            return;
        };
        let blockhash = cached_blockhash.hash;
        line.blockhash = Some(blockhash.to_string());

        let build = match build_trailing_sell_unsigned_flashx_pump_with_fees_and_cache(
            Some(&sell_context.route_context),
            copy_wallet,
            &execution_plan.mint,
            token_amount_raw,
            &self.trailing_sell_tx_fee_config(trailing_sell_plan),
            Some(&self.pda_cache),
        ) {
            Ok(build) => build,
            Err(error) => {
                line.skip(tx_build_error_reason(error));
                return;
            }
        };
        line.route_layout = Some(build.route_layout);
        line.instruction_count = build.instructions.len();

        let tx = Transaction::new_signed_with_payer(
            &build.instructions,
            Some(&keypair.pubkey()),
            &[keypair],
            blockhash,
        );
        let tx_bytes = match bincode::serialize(&tx) {
            Ok(bytes) => bytes,
            Err(error) => {
                line.error(format!(
                    "serialize signed rust trailing sell transaction: {error}"
                ));
                return;
            }
        };
        let encoded_tx = STANDARD.encode(tx_bytes);
        line.signed = true;
        line.copy_signature = tx.signatures.first().map(ToString::to_string);

        if self.options.simulate_auto_sell {
            match self.simulate_transaction(&encoded_tx).await {
                Ok(simulation) => {
                    line.simulated = true;
                    line.simulation_error = simulation.err;
                    line.simulation_units_consumed = simulation.units_consumed;
                    line.simulation_logs = simulation.logs.unwrap_or_default();
                    if line.simulation_error.is_some() {
                        line.skip("rust trailing sell simulation failed; send blocked");
                        return;
                    }
                }
                Err(error) => {
                    line.simulated = true;
                    line.simulation_error = Some(serde_json::Value::String(error));
                    line.skip("rust trailing sell simulation failed; send blocked");
                    return;
                }
            }
        }

        line.mark_submitted();
        match self.send_transaction(&encoded_tx, None, None).await {
            Ok(result) => {
                line.sent = true;
                line.mark_signature_returned();
                line.send_signature = Some(result.signature);
                line.send_rpc_url_count = result.rpc_url_count;
                line.send_rpc_winner = Some(result.rpc_winner);
                line.send_rpc_attempts = result.rpc_attempts;
                line.send_rpc_errors = result.rpc_errors;
                line.decision = "sent";
            }
            Err(error) => line.error(error),
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

    async fn cashback_sell_guard_for_route_context(
        &self,
        route_context: &RouteContext,
    ) -> Result<Option<CashbackSellGuard>, String> {
        let RouteContext::FlashxPump(context) = route_context;
        match context.layout {
            FlashxPumpLayout::MigratedAmm => {
                if context
                    .resolved_pubkey("userVolumeAccumulatorQuoteTokenAccount")
                    .is_some()
                {
                    Ok(Some(CashbackSellGuard {
                        source: "migrated-amm-route-context",
                    }))
                } else {
                    Ok(None)
                }
            }
            FlashxPumpLayout::DirectPump => {
                let bonding_curve = context.resolved_pubkey("bondingCurve").ok_or_else(|| {
                    "missing direct-Pump bonding curve for cashback guard".to_string()
                })?;
                if self
                    .direct_pump_bonding_curve_cashback_enabled(&bonding_curve)
                    .await?
                {
                    Ok(Some(CashbackSellGuard {
                        source: "direct-pump-bonding-curve-rpc",
                    }))
                } else {
                    Ok(None)
                }
            }
        }
    }

    async fn direct_pump_bonding_curve_cashback_enabled(
        &self,
        bonding_curve: &Pubkey,
    ) -> Result<bool, String> {
        let rpc_url = self
            .options
            .solana_rpc_url
            .as_deref()
            .ok_or_else(|| "cashback detection failed: missing SOLANA_RPC_URL".to_string())?;
        let request = async {
            self.client
                .post(rpc_url)
                .json(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "getAccountInfo",
                    "params": [
                        bonding_curve.to_string(),
                        {
                            "encoding": "base64",
                            "commitment": "processed"
                        }
                    ]
                }))
                .send()
                .await
                .map_err(|error| {
                    format!("send direct-Pump cashback getAccountInfo request: {error}")
                })?
                .error_for_status()
                .map_err(|error| {
                    format!("direct-Pump cashback getAccountInfo HTTP status: {error}")
                })?
                .json::<RpcResponse<AccountInfoResult>>()
                .await
                .map_err(|error| {
                    format!("decode direct-Pump cashback getAccountInfo response: {error}")
                })
        };
        let response = if self.options.send_http_timeout_ms > 0 {
            tokio::time::timeout(
                Duration::from_millis(self.options.send_http_timeout_ms),
                request,
            )
            .await
            .map_err(|_| {
                format!(
                    "direct-Pump cashback getAccountInfo timed out after {}ms",
                    self.options.send_http_timeout_ms
                )
            })??
        } else {
            request.await?
        };

        if let Some(error) = response.error {
            return Err(format!(
                "direct-Pump cashback getAccountInfo RPC error: {}",
                error.message
            ));
        }
        let account = response
            .result
            .ok_or_else(|| "direct-Pump cashback getAccountInfo result missing".to_string())?
            .value
            .ok_or_else(|| "direct-Pump bonding curve account not found".to_string())?;
        let data = account
            .data
            .encoded_base64()
            .ok_or_else(|| "direct-Pump bonding curve account data is not base64".to_string())?;
        let bytes = STANDARD
            .decode(data)
            .map_err(|error| format!("decode direct-Pump bonding curve account data: {error}"))?;
        Ok(bytes
            .get(DIRECT_PUMP_CASHBACK_FLAG_OFFSET)
            .copied()
            .unwrap_or_default()
            != 0)
    }

    async fn wait_for_signature_confirmation(
        &self,
        signature: Option<&str>,
        observed_signature: Option<&str>,
        transaction_label: &'static str,
        timeout_ms: u64,
        poll_ms: u64,
    ) -> SignatureConfirmation {
        let Some(signature) = signature.filter(|value| !value.is_empty()) else {
            return SignatureConfirmation {
                checked: false,
                status: "missing_signature",
                ok: false,
                slot: None,
                block_position: None,
                block_position_error: None,
                confirmation_status: None,
                err: None,
                reason: Some(format!("missing {transaction_label} signature")),
            };
        };
        let Some(rpc_url) = self.options.solana_rpc_url.as_deref() else {
            return SignatureConfirmation {
                checked: false,
                status: "error",
                ok: false,
                slot: None,
                block_position: None,
                block_position_error: None,
                confirmation_status: None,
                err: None,
                reason: Some("missing SOLANA_RPC_URL".to_string()),
            };
        };

        let started_at = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);
        loop {
            let request = async {
                self.client
                    .post(rpc_url)
                    .json(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": signature,
                        "method": "getSignatureStatuses",
                        "params": [[signature], { "searchTransactionHistory": true }]
                    }))
                    .send()
                    .await
                    .map_err(|error| format!("send getSignatureStatuses request: {error}"))?
                    .error_for_status()
                    .map_err(|error| format!("getSignatureStatuses HTTP status: {error}"))?
                    .json::<RpcResponse<SignatureStatusesResult>>()
                    .await
                    .map_err(|error| format!("decode getSignatureStatuses response: {error}"))
            };
            let response = if self.options.send_http_timeout_ms > 0 {
                match tokio::time::timeout(
                    Duration::from_millis(self.options.send_http_timeout_ms),
                    request,
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err(format!(
                        "getSignatureStatuses timed out after {}ms",
                        self.options.send_http_timeout_ms
                    )),
                }
            } else {
                request.await
            };

            match response {
                Ok(response) => {
                    if let Some(error) = response.error {
                        return SignatureConfirmation {
                            checked: true,
                            status: "error",
                            ok: false,
                            slot: None,
                            block_position: None,
                            block_position_error: None,
                            confirmation_status: None,
                            err: None,
                            reason: Some(format!(
                                "getSignatureStatuses RPC error: {}",
                                error.message
                            )),
                        };
                    }
                    if let Some(status) = response
                        .result
                        .and_then(|result| result.value.into_iter().next().flatten())
                    {
                        if let Some(error) = status.err {
                            let (block_position, block_position_error) = self
                                .fetch_landed_block_position(
                                    rpc_url,
                                    status.slot,
                                    signature,
                                    observed_signature,
                                )
                                .await;
                            return SignatureConfirmation {
                                checked: true,
                                status: "failed",
                                ok: false,
                                slot: status.slot,
                                block_position,
                                block_position_error,
                                confirmation_status: status.confirmation_status,
                                err: Some(error.clone()),
                                reason: Some(format!(
                                    "{transaction_label} landed with error: {error}"
                                )),
                            };
                        }
                        if matches!(
                            status.confirmation_status.as_deref(),
                            Some("confirmed") | Some("finalized")
                        ) {
                            let (block_position, block_position_error) = self
                                .fetch_landed_block_position(
                                    rpc_url,
                                    status.slot,
                                    signature,
                                    observed_signature,
                                )
                                .await;
                            return SignatureConfirmation {
                                checked: true,
                                status: "landed",
                                ok: true,
                                slot: status.slot,
                                block_position,
                                block_position_error,
                                confirmation_status: status.confirmation_status,
                                err: None,
                                reason: None,
                            };
                        }
                    }
                }
                Err(error) => {
                    if started_at.elapsed() >= timeout {
                        return SignatureConfirmation {
                            checked: true,
                            status: "error",
                            ok: false,
                            slot: None,
                            block_position: None,
                            block_position_error: None,
                            confirmation_status: None,
                            err: None,
                            reason: Some(error),
                        };
                    }
                }
            }

            if started_at.elapsed() >= timeout {
                return SignatureConfirmation {
                    checked: true,
                    status: "submitted_not_landed",
                    ok: false,
                    slot: None,
                    block_position: None,
                    block_position_error: None,
                    confirmation_status: None,
                    err: None,
                    reason: Some(format!(
                        "{transaction_label} not found before confirmation timeout"
                    )),
                };
            }
            tokio::time::sleep(Duration::from_millis(poll_ms.max(1))).await;
        }
    }

    async fn fetch_landed_block_position(
        &self,
        rpc_url: &str,
        slot: Option<u64>,
        signature: &str,
        observed_signature: Option<&str>,
    ) -> (Option<BlockPosition>, Option<String>) {
        let Some(slot) = slot else {
            return (None, None);
        };

        let request = async {
            self.client
                .post(rpc_url)
                .json(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": format!("block-position-{slot}"),
                    "method": "getBlock",
                    "params": [
                        slot,
                        {
                            "encoding": "json",
                            "transactionDetails": "signatures",
                            "rewards": false,
                            "maxSupportedTransactionVersion": 0
                        }
                    ]
                }))
                .send()
                .await
                .map_err(|error| format!("send getBlock request: {error}"))?
                .error_for_status()
                .map_err(|error| format!("getBlock HTTP status: {error}"))?
                .json::<RpcResponse<BlockPositionResult>>()
                .await
                .map_err(|error| format!("decode getBlock response: {error}"))
        };

        let response = if self.options.send_http_timeout_ms > 0 {
            match tokio::time::timeout(
                Duration::from_millis(self.options.send_http_timeout_ms),
                request,
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(format!(
                    "getBlock timed out after {}ms",
                    self.options.send_http_timeout_ms
                )),
            }
        } else {
            request.await
        };

        let response = match response {
            Ok(response) => response,
            Err(error) => return (None, Some(error)),
        };
        if let Some(error) = response.error {
            return (None, Some(format!("getBlock RPC error: {}", error.message)));
        }
        let Some(result) = response.result else {
            return (None, Some("getBlock result missing".to_string()));
        };

        let tx_count = result.signatures.len();
        let landed_tx_index = result
            .signatures
            .iter()
            .position(|candidate| candidate == signature);
        let observed_tx_index = observed_signature
            .filter(|value| !value.is_empty())
            .and_then(|observed| {
                result
                    .signatures
                    .iter()
                    .position(|candidate| candidate == observed)
            });
        let txs_after_observed = match (observed_tx_index, landed_tx_index) {
            (Some(observed), Some(landed)) => Some(landed as i64 - observed as i64),
            _ => None,
        };

        (
            Some(BlockPosition {
                tx_count,
                landed_tx_index,
                observed_tx_index,
                txs_after_observed,
            }),
            None,
        )
    }

    pub(crate) async fn confirm_copy_transaction(
        &self,
        line: CopyExecutionLine,
    ) -> TransactionConfirmationLine {
        let observed_signature = line.observed_signature_string();
        let confirmation = self
            .wait_for_signature_confirmation(
                line.send_signature.as_deref(),
                Some(&observed_signature),
                "copy buy transaction",
                self.options.rust_trailing_sell_confirmation_timeout_ms,
                self.options.rust_trailing_sell_confirmation_poll_ms,
            )
            .await;
        TransactionConfirmationLine::from_copy_execution(&line, confirmation)
    }

    pub(crate) async fn confirm_rust_trailing_sell_transaction(
        &self,
        line: RustTrailingSellLine,
    ) -> TransactionConfirmationLine {
        let confirmation = self
            .wait_for_signature_confirmation(
                line.send_signature.as_deref(),
                Some(&line.observed_signature),
                "rust trailing sell transaction",
                self.options.rust_trailing_sell_confirmation_timeout_ms,
                self.options.rust_trailing_sell_confirmation_poll_ms,
            )
            .await;
        TransactionConfirmationLine::from_rust_trailing_sell(&line, confirmation)
    }

    pub(crate) async fn confirm_auto_sell_transaction(
        &self,
        line: CopyExecutionLine,
    ) -> TransactionConfirmationLine {
        let observed_signature = line.observed_signature_string();
        let confirmation = self
            .wait_for_signature_confirmation(
                line.auto_sell_send_signature.as_deref(),
                Some(&observed_signature),
                "auto-sell transaction",
                self.options.rust_trailing_sell_confirmation_timeout_ms,
                self.options.rust_trailing_sell_confirmation_poll_ms,
            )
            .await;
        TransactionConfirmationLine::from_auto_sell_execution(&line, confirmation)
    }

    fn trailing_sell_tx_fee_config(&self, plan: &TrailingSellPlan) -> TxFeeConfig {
        TxFeeConfig {
            compute_unit_price_micro_lamports: plan
                .priority_fee_micro_lamports
                .or(self.options.sell_priority_fee_micro_lamports),
            jito_tip_lamports: self
                .options
                .send_lane_mode
                .uses_jito_tip()
                .then_some(
                    plan.jito_tip_lamports
                        .or(self.options.sell_jito_tip_lamports),
                )
                .flatten(),
            jito_tip_account: self
                .options
                .send_lane_mode
                .uses_jito_tip()
                .then(|| {
                    plan.jito_tip_account
                        .clone()
                        .or_else(|| self.options.sell_jito_tip_account.clone())
                })
                .flatten(),
            helius_sender_tip_lamports: (self.options.helius_sender_enabled
                && self.options.send_lane_mode.uses_helius_sender_tip())
            .then_some(self.options.helius_sender_tip_lamports)
            .flatten(),
            helius_sender_tip_account: (self.options.helius_sender_enabled
                && self.options.send_lane_mode.uses_helius_sender_tip())
            .then(|| self.options.helius_sender_tip_account.clone())
            .flatten(),
        }
    }
}

fn spawn_send_lane_attribution_collector(
    mut send_set: JoinSet<SendAttemptOutcome>,
    context: SendLaneAttributionContext,
    attribution_tx: mpsc::UnboundedSender<CopyExecutionOutput>,
    send_signature: String,
    first_ack_lane: String,
    first_ack_at_ms: u128,
    completed_attempts: Vec<SendRpcAttemptLine>,
    completed_errors: Vec<String>,
) {
    tokio::spawn(async move {
        let mut all_attempts: Vec<SendLaneAttemptAttribution> = completed_attempts
            .into_iter()
            .map(|attempt| {
                let ack_at_ms = (attempt.status == "submitted"
                    && attempt.signature.as_deref() == Some(send_signature.as_str()))
                .then_some(first_ack_at_ms);
                send_lane_attempt_attribution(attempt, ack_at_ms)
            })
            .collect();
        all_attempts.extend(
            completed_errors
                .into_iter()
                .map(|error| SendLaneAttemptAttribution {
                    label: "fanout-join".to_string(),
                    kind: "internal",
                    mode: None,
                    status: "failed",
                    duration_ms: 0,
                    ack_at: None,
                    error: Some(error),
                }),
        );

        while let Some(result) = send_set.join_next().await {
            match result {
                Ok(outcome) => {
                    let ack_at_ms = outcome.signature.as_ref().map(|_| outcome.finished_at_ms);
                    all_attempts.push(send_lane_attempt_attribution(outcome.attempt, ack_at_ms));
                }
                Err(error) => all_attempts.push(SendLaneAttemptAttribution {
                    label: "fanout-join".to_string(),
                    kind: "internal",
                    mode: None,
                    status: "failed",
                    duration_ms: 0,
                    ack_at: None,
                    error: Some(format!("join error: {error}")),
                }),
            }
        }

        let line = SendLaneAttributionLine {
            schema: "copytrade.sendLaneAttribution.v1",
            observed_at_ms: context.observed_at_ms,
            attribution_at_ms: now_ms(),
            provider: context.provider,
            source: context.source,
            endpoint: context.endpoint,
            observed_wallet: context.observed_wallet.to_string(),
            copy_wallet: context.copy_wallet,
            mint: context.mint.to_string(),
            transaction_role: context.transaction_role,
            submission_group_id: context.submission_group_id,
            observed_signature: signature_bytes_to_string(context.observed_signature),
            send_signature,
            send_lane_mode: context.send_lane_mode,
            first_ack_lane,
            first_ack_at_ms,
            all_attempts,
        };

        if attribution_tx
            .send(CopyExecutionOutput::SendLaneAttribution(line))
            .is_err()
        {
            eprintln!("send lane attribution result dropped; receiver closed");
        }
    });
}

fn send_lane_attempt_attribution(
    attempt: SendRpcAttemptLine,
    ack_at: Option<u128>,
) -> SendLaneAttemptAttribution {
    SendLaneAttemptAttribution {
        label: attempt.label,
        kind: attempt.kind,
        mode: attempt.mode,
        status: attempt.status,
        duration_ms: attempt.duration_ms,
        ack_at,
        error: attempt.error,
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

    fn insert(&mut self, target_wallet: &Pubkey, mint: &Pubkey, route_context: RouteContext) {
        let key = DirectPumpSellContextKey {
            target_wallet: *target_wallet,
            mint: *mint,
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

    fn get(&self, target_wallet: &Pubkey, mint: &Pubkey) -> Option<RouteContext> {
        let key = DirectPumpSellContextKey {
            target_wallet: *target_wallet,
            mint: *mint,
        };
        self.entries.get(&key).cloned()
    }

    fn get_str(&self, target_wallet: &str, mint: &str) -> Option<RouteContext> {
        let target_wallet = Pubkey::from_str(target_wallet).ok()?;
        let mint = Pubkey::from_str(mint).ok()?;
        self.get(&target_wallet, &mint)
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

    if is_direct_pump_sell_route_context(route_context) {
        return Ok(route_context.clone());
    }

    let Ok(cache) = executor.direct_pump_sell_contexts.lock() else {
        return Err("direct-pump sell-side route context cache unavailable");
    };
    cache
        .get_str(&execution_plan.target_wallet, &execution_plan.mint)
        .filter(is_direct_pump_sell_route_context)
        .ok_or("missing direct-pump sell-side route context")
}

#[derive(Clone, Debug)]
struct TrailingSellRouteContext {
    route_context: RouteContext,
    source: &'static str,
    resolved_at_ms: u128,
    reason: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MissingTrailingSellRouteContext {
    source: &'static str,
    reason: &'static str,
}

fn trailing_sell_route_context_for_plan(
    executor: &CopyExecutor,
    execution_plan: &ExecutionPlanLine,
    copy_wallet: &str,
) -> Result<TrailingSellRouteContext, MissingTrailingSellRouteContext> {
    let route_context =
        execution_plan
            .route_context
            .clone()
            .ok_or(MissingTrailingSellRouteContext {
                source: "target_context_blocked",
                reason: "missing rust trailing sell route context",
            })?;
    if !is_direct_pump_route_context(&route_context) {
        return Ok(TrailingSellRouteContext {
            route_context,
            source: "derived",
            resolved_at_ms: now_ms(),
            reason: "non-direct route context is reusable for scheduled trailing sell",
        });
    }
    if !is_direct_pump_sell_route_context(&route_context) {
        return Ok(TrailingSellRouteContext {
            route_context,
            source: "derived",
            resolved_at_ms: now_ms(),
            reason: "direct Pump sell context derived from copy-buy route accounts",
        });
    }
    let cached_sell_context = executor
        .direct_pump_sell_contexts
        .lock()
        .ok()
        .and_then(|cache| cache.get_str(copy_wallet, &execution_plan.mint))
        .filter(is_direct_pump_sell_route_context);

    if let Some(route_context) = cached_sell_context {
        return Ok(TrailingSellRouteContext {
            route_context,
            source: "cached_copy_wallet",
            resolved_at_ms: now_ms(),
            reason: "copy-wallet direct-Pump sell route context cache hit",
        });
    }

    Err(MissingTrailingSellRouteContext {
        source: "target_context_blocked",
        reason: "missing copy-wallet sell route context",
    })
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

fn effective_trailing_sell_steps(plan: &TrailingSellPlan) -> Vec<TrailingSellStep> {
    let mut steps = match plan.percent_basis {
        TrailingSellPercentBasis::RemainingBalance => plan.steps.clone(),
        TrailingSellPercentBasis::OriginalPosition => {
            original_position_steps_to_remaining_balance_steps(&plan.steps)
        }
    };

    if plan.mode == TrailingSellMode::CustomSteps {
        let mut elapsed_ms = 0u64;
        for step in &mut steps {
            elapsed_ms = elapsed_ms.saturating_add(step.delay_ms);
            step.delay_ms = elapsed_ms;
        }
    }

    steps
        .into_iter()
        .filter(|step| step.percent.is_finite() && step.percent > 0.0 && step.percent <= 100.0)
        .take(TRAILING_SELL_MAX_STEPS)
        .collect()
}

fn original_position_steps_to_remaining_balance_steps(
    steps: &[TrailingSellStep],
) -> Vec<TrailingSellStep> {
    let mut remaining_original_percent = 100.0f64;
    let mut converted = Vec::with_capacity(steps.len());

    for step in steps {
        if remaining_original_percent <= 0.0 {
            break;
        }
        let original_percent_to_sell = step.percent.min(remaining_original_percent);
        let remaining_balance_percent =
            (original_percent_to_sell / remaining_original_percent * 100.0).min(100.0);
        remaining_original_percent -= original_percent_to_sell;
        if remaining_balance_percent > 0.0 {
            converted.push(TrailingSellStep {
                delay_ms: step.delay_ms,
                percent: (remaining_balance_percent * 1_000_000.0).round() / 1_000_000.0,
            });
        }
    }

    converted
}

fn trailing_sell_token_amount_raw(token_balance_raw: u64, percent: f64) -> u64 {
    if token_balance_raw == 0 || !percent.is_finite() || percent <= 0.0 {
        return 0;
    }
    if percent >= 100.0 {
        return token_balance_raw;
    }

    let basis_points = (percent * 100.0).floor();
    if !basis_points.is_finite() || basis_points <= 0.0 {
        return 0;
    }

    ((u128::from(token_balance_raw) * basis_points as u128) / 10_000u128) as u64
}

fn update_optimistic_trailing_sell_balance_raw(
    previous_sellable_balance_raw: Option<u64>,
    current_sellable_balance_raw: Option<u64>,
    sold_amount_raw: Option<u64>,
) -> Option<u64> {
    let sold_amount_raw = sold_amount_raw?;
    let sellable_balance_raw = previous_sellable_balance_raw.or(current_sellable_balance_raw)?;
    Some(sellable_balance_raw.saturating_sub(sold_amount_raw))
}

#[derive(Debug, PartialEq, Eq)]
enum CopySpendDecision {
    Use(u64),
    Skip(String),
}

fn copy_spend_after_migrated_amm_guard(
    options: &CopyExecutionOptions,
    route_context: Option<&RouteContext>,
    copy_spend_lamports: u64,
) -> Result<CopySpendDecision, String> {
    if !matches!(
        route_context,
        Some(RouteContext::FlashxPump(context)) if context.layout == FlashxPumpLayout::MigratedAmm
    ) {
        return Ok(CopySpendDecision::Use(copy_spend_lamports));
    }

    let min_copy_lamports = options.migrated_amm_min_copy_lamports()?;
    if copy_spend_lamports >= min_copy_lamports {
        return Ok(CopySpendDecision::Use(copy_spend_lamports));
    }

    match options.migrated_amm_small_copy_mode {
        MigratedAmmSmallCopyMode::Skip => Ok(CopySpendDecision::Skip(format!(
            "migrated AMM copy spend {} lamports below min {} lamports",
            copy_spend_lamports, min_copy_lamports
        ))),
        MigratedAmmSmallCopyMode::Floor => Ok(CopySpendDecision::Use(min_copy_lamports)),
    }
}

fn estimate_total_copy_spend_lamports(
    build: &crate::tx_builder::FullCopyUnsignedTxBuild,
    route_context: Option<&RouteContext>,
) -> Result<u64, String> {
    let Some(RouteContext::FlashxPump(context)) = route_context else {
        return Err("missing route context for total spend guard".to_string());
    };
    let spendable_sol_in = copy_input_lamports_from_build(build, context)?;

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

fn copy_input_lamports_from_build(
    build: &crate::tx_builder::FullCopyUnsignedTxBuild,
    context: &crate::parser::FlashxPumpRouteContext,
) -> Result<u64, String> {
    match context.layout {
        FlashxPumpLayout::MigratedAmm => build
            .instructions
            .iter()
            .find(|instruction| {
                instruction.program_id == context.program_id
                    && instruction.data.first().copied() == Some(1)
            })
            .and_then(|instruction| read_u64_le(&instruction.data, 1))
            .ok_or_else(|| "missing copied flashx SOL amount for total spend guard".to_string()),
        FlashxPumpLayout::DirectPump => build
            .instructions
            .iter()
            .find(|instruction| instruction.program_id == *pump_fun_program_id())
            .and_then(|instruction| read_u64_le(&instruction.data, 8))
            .ok_or_else(|| {
                "missing copied direct-pump SOL amount for total spend guard".to_string()
            }),
    }
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

fn max_copy_sol_guard_reason(
    max_copy_sol: Option<f64>,
    planned_copy_sol_amount: f64,
) -> Result<Option<String>, String> {
    let Some(max_copy_sol) = max_copy_sol else {
        return Ok(None);
    };
    if !max_copy_sol.is_finite() {
        return Err("invalid max copy SOL guard".to_string());
    }
    if max_copy_sol <= 0.0 {
        return Ok(None);
    }
    if planned_copy_sol_amount > max_copy_sol {
        return Ok(Some(
            "planned copy spend exceeds max copy SOL guard".to_string(),
        ));
    }
    Ok(None)
}

impl CopyExecutionLine {
    fn observed_wallet_string(&self) -> String {
        self.observed_wallet.to_string()
    }

    fn observed_signature_string(&self) -> String {
        signature_bytes_to_string(self.observed_signature)
    }

    fn mint_string(&self) -> String {
        self.mint.to_string()
    }

    pub(crate) fn was_sent(&self) -> bool {
        self.sent && self.decision == "sent"
    }

    pub(crate) fn auto_sell_was_sent(&self) -> bool {
        self.auto_sell_sent
            && self.auto_sell_decision == Some("sent")
            && self.auto_sell_send_signature.is_some()
    }

    fn new(
        request: &CopyRuntimeRequest,
        options: &CopyExecutionOptions,
        timings: SignalTimings,
    ) -> Self {
        let tx_fee_config = options.tx_fee_config();
        Self {
            schema: "copytrade.localExecution.v1",
            observed_at_ms: request.observed_at_ms,
            execution_at_ms: now_ms(),
            provider: "shredstream",
            source: "jito-proxy",
            endpoint: options.endpoint.clone(),
            observed_wallet: request.target_wallet,
            copy_wallet: options.copy_wallet.clone(),
            observed_signature: request.signature,
            slot: request.slot,
            selected_route: request.route,
            mint: request.mint,
            copy_wallet_token_account: None,
            observed_action: request.observed_action,
            observed_sol_amount: request.observed_sol_amount,
            planned_copy_sol_amount: request.planned_copy_sol_amount,
            planned_copy_spend_lamports: request.planned_copy_sol_amount.and_then(sol_to_lamports),
            max_copy_sol: options.max_copy_sol,
            max_total_copy_spend_sol: options.max_total_copy_spend_sol,
            estimated_total_copy_spend_sol: None,
            estimated_total_copy_spend_lamports: None,
            copy_wallet_balance_guard: options.copy_wallet_balance_guard,
            copy_wallet_balance_lamports: None,
            copy_wallet_balance_required_lamports: None,
            copy_wallet_balance_fetched_at_ms: None,
            copy_wallet_balance_age_ms: None,
            copy_wallet_balance_reason: None,
            send_enabled: options.enable_copy_send,
            dry_run: options.dry_run,
            simulation_requested: options.simulate_copy_tx,
            fast_copy_send: options.fast_copy_send,
            skip_preflight: options.fast_copy_send,
            feed_received_at_ms: timings.grpc_message_received_at_ms,
            decoded_at_ms: timings.entries_deserialized_at_ms,
            matched_at_ms: timings.wallet_match_finished_at_ms,
            planned_at_ms: request.planned_at_ms,
            built_at_ms: None,
            feed_received_to_decoded_us: timings.deserialize_us,
            decoded_to_matched_us: timings.wallet_match_finished_at_us,
            matched_to_planned_ms: request
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
            send_lane_mode: options.send_lane_mode.as_str(),
            send_rpc_winner: None,
            send_rpc_attempts: Vec::new(),
            send_rpc_errors: Vec::new(),
            reason: None,
            auto_sell_enabled: options.auto_sell_after_buy_enabled(),
            auto_sell_delay_ms: options.auto_sell_delay_ms,
            auto_sell_simulation_requested: options.simulate_auto_sell_enabled(),
            buy_latency_test_isolated: options.isolate_buy_latency_test,
            priority_fee_micro_lamports: tx_fee_config.compute_unit_price_micro_lamports,
            jito_tip_lamports: tx_fee_config.jito_tip_lamports,
            jito_tip_account: tx_fee_config.jito_tip_account,
            helius_sender_tip_lamports: tx_fee_config.helius_sender_tip_lamports,
            helius_sender_tip_account: tx_fee_config.helius_sender_tip_account,
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
            auto_sell_send_lane_mode: options.send_lane_mode.as_str(),
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

    fn record_balance_check(&mut self, check: WalletBalanceCheck) {
        self.copy_wallet_balance_lamports = check.lamports;
        self.copy_wallet_balance_required_lamports = Some(check.required_lamports);
        self.copy_wallet_balance_fetched_at_ms = check.fetched_at_ms;
        self.copy_wallet_balance_age_ms = check.age_ms;
        self.copy_wallet_balance_reason = check.reason;
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

impl CopyExecutionOutput {
    pub(crate) fn was_sent(&self) -> bool {
        matches!(self, CopyExecutionOutput::Copy(line) if line.was_sent())
    }

    pub(crate) fn write_json_line(
        &self,
        writer: Option<&mut std::io::BufWriter<std::fs::File>>,
        flush: bool,
    ) -> Result<()> {
        let Some(writer) = writer else {
            return Ok(());
        };

        match self {
            CopyExecutionOutput::Copy(line) => serde_json::to_writer(&mut *writer, line)?,
            CopyExecutionOutput::SendLaneAttribution(line) => {
                serde_json::to_writer(&mut *writer, line)?
            }
            CopyExecutionOutput::RustTrailingSell(line) => {
                serde_json::to_writer(&mut *writer, line)?
            }
            CopyExecutionOutput::TransactionConfirmation(line) => {
                serde_json::to_writer(&mut *writer, line)?
            }
        }
        writer.write_all(b"\n")?;
        if flush {
            writer.flush()?;
        }
        Ok(())
    }
}

impl From<CopyExecutionLine> for CopyExecutionOutput {
    fn from(line: CopyExecutionLine) -> Self {
        CopyExecutionOutput::Copy(line)
    }
}

fn slot_delta(observed_slot: u64, confirmation_slot: Option<u64>) -> Option<i64> {
    confirmation_slot.map(|slot| slot as i64 - observed_slot as i64)
}

fn same_slot_tx_delta(
    observed_slot: u64,
    confirmation_slot: Option<u64>,
    confirmation: &SignatureConfirmation,
) -> Option<i64> {
    if confirmation_slot != Some(observed_slot) {
        return None;
    }
    confirmation
        .block_position
        .as_ref()
        .and_then(|position| position.txs_after_observed)
}

impl TransactionConfirmationLine {
    fn from_copy_execution(line: &CopyExecutionLine, confirmation: SignatureConfirmation) -> Self {
        Self {
            schema: "copytrade.transactionConfirmation.v1",
            observed_at_ms: line.observed_at_ms,
            confirmation_at_ms: now_ms(),
            provider: "shredstream",
            source: "jito-proxy",
            endpoint: line.endpoint.clone(),
            observed_wallet: line.observed_wallet_string(),
            copy_wallet: line.copy_wallet.clone(),
            observed_signature: line.observed_signature_string(),
            observed_action: Some(line.observed_action),
            slot: line.slot,
            selected_route: line.selected_route,
            route_layout: line.route_layout,
            mint: line.mint_string(),
            transaction_role: "copy_buy",
            signature: line.send_signature.clone(),
            submitted_at_ms: line.send_submitted_at_ms,
            signature_returned_at_ms: line.signature_returned_at_ms,
            buy_send_signature: None,
            step_index: None,
            total_steps: None,
            checked: confirmation.checked,
            status: confirmation.status,
            ok: confirmation.ok,
            confirmation_slot: confirmation.slot,
            slot_delta: slot_delta(line.slot, confirmation.slot),
            landed_block_tx_count: confirmation
                .block_position
                .as_ref()
                .map(|position| position.tx_count),
            landed_tx_index: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.landed_tx_index),
            observed_tx_index: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.observed_tx_index),
            target_tx_index: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.observed_tx_index),
            copy_tx_index: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.landed_tx_index),
            txs_after_observed: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.txs_after_observed),
            same_slot_tx_delta: same_slot_tx_delta(line.slot, confirmation.slot, &confirmation),
            tx_delta: same_slot_tx_delta(line.slot, confirmation.slot, &confirmation),
            block_position_error: confirmation.block_position_error,
            confirmation_status: confirmation.confirmation_status,
            err: confirmation.err,
            reason: confirmation.reason,
        }
    }

    fn from_rust_trailing_sell(
        line: &RustTrailingSellLine,
        confirmation: SignatureConfirmation,
    ) -> Self {
        Self {
            schema: "copytrade.transactionConfirmation.v1",
            observed_at_ms: line.observed_at_ms,
            confirmation_at_ms: now_ms(),
            provider: "shredstream",
            source: "jito-proxy",
            endpoint: line.endpoint.clone(),
            observed_wallet: line.observed_wallet.clone(),
            copy_wallet: line.copy_wallet.clone(),
            observed_signature: line.observed_signature.clone(),
            observed_action: None,
            slot: line.slot,
            selected_route: line.selected_route,
            route_layout: line.route_layout,
            mint: line.mint.clone(),
            transaction_role: "rust_trailing_sell",
            signature: line.send_signature.clone(),
            submitted_at_ms: line.submitted_at_ms,
            signature_returned_at_ms: line.signature_returned_at_ms,
            buy_send_signature: line.buy_send_signature.clone(),
            step_index: Some(line.step_index),
            total_steps: Some(line.total_steps),
            checked: confirmation.checked,
            status: confirmation.status,
            ok: confirmation.ok,
            confirmation_slot: confirmation.slot,
            slot_delta: slot_delta(line.slot, confirmation.slot),
            landed_block_tx_count: confirmation
                .block_position
                .as_ref()
                .map(|position| position.tx_count),
            landed_tx_index: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.landed_tx_index),
            observed_tx_index: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.observed_tx_index),
            target_tx_index: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.observed_tx_index),
            copy_tx_index: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.landed_tx_index),
            txs_after_observed: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.txs_after_observed),
            same_slot_tx_delta: same_slot_tx_delta(line.slot, confirmation.slot, &confirmation),
            tx_delta: same_slot_tx_delta(line.slot, confirmation.slot, &confirmation),
            block_position_error: confirmation.block_position_error,
            confirmation_status: confirmation.confirmation_status,
            err: confirmation.err,
            reason: confirmation.reason,
        }
    }

    fn from_auto_sell_execution(
        line: &CopyExecutionLine,
        confirmation: SignatureConfirmation,
    ) -> Self {
        Self {
            schema: "copytrade.transactionConfirmation.v1",
            observed_at_ms: line.observed_at_ms,
            confirmation_at_ms: now_ms(),
            provider: "shredstream",
            source: "jito-proxy",
            endpoint: line.endpoint.clone(),
            observed_wallet: line.observed_wallet_string(),
            copy_wallet: line.copy_wallet.clone(),
            observed_signature: line.observed_signature_string(),
            observed_action: Some(line.observed_action),
            slot: line.slot,
            selected_route: line.selected_route,
            route_layout: line.route_layout,
            mint: line.mint_string(),
            transaction_role: if line.observed_action == Action::Sell {
                "target_auto_sell"
            } else {
                "auto_sell"
            },
            signature: line.auto_sell_send_signature.clone(),
            submitted_at_ms: line.auto_sell_submitted_at_ms,
            signature_returned_at_ms: line.auto_sell_signature_returned_at_ms,
            buy_send_signature: line.send_signature.clone(),
            step_index: None,
            total_steps: None,
            checked: confirmation.checked,
            status: confirmation.status,
            ok: confirmation.ok,
            confirmation_slot: confirmation.slot,
            slot_delta: slot_delta(line.slot, confirmation.slot),
            landed_block_tx_count: confirmation
                .block_position
                .as_ref()
                .map(|position| position.tx_count),
            landed_tx_index: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.landed_tx_index),
            observed_tx_index: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.observed_tx_index),
            target_tx_index: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.observed_tx_index),
            copy_tx_index: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.landed_tx_index),
            txs_after_observed: confirmation
                .block_position
                .as_ref()
                .and_then(|position| position.txs_after_observed),
            same_slot_tx_delta: same_slot_tx_delta(line.slot, confirmation.slot, &confirmation),
            tx_delta: same_slot_tx_delta(line.slot, confirmation.slot, &confirmation),
            block_position_error: confirmation.block_position_error,
            confirmation_status: confirmation.confirmation_status,
            err: confirmation.err,
            reason: confirmation.reason,
        }
    }
}

impl RustTrailingSellLine {
    pub(crate) fn was_sent(&self) -> bool {
        self.sent && self.decision == "sent"
    }

    fn new(
        buy_line: &CopyExecutionLine,
        plan: &TrailingSellPlan,
        step_index: usize,
        total_steps: usize,
        step: TrailingSellStep,
        anchor_at_ms: u128,
        options: &CopyExecutionOptions,
    ) -> Self {
        let step_started_at_ms = now_ms();
        let due_at_ms = anchor_at_ms.saturating_add(u128::from(step.delay_ms));
        let jito_tip_lamports = options
            .send_lane_mode
            .uses_jito_tip()
            .then_some(plan.jito_tip_lamports.or(options.sell_jito_tip_lamports))
            .flatten();
        let jito_tip_account = options
            .send_lane_mode
            .uses_jito_tip()
            .then(|| {
                plan.jito_tip_account
                    .clone()
                    .or_else(|| options.sell_jito_tip_account.clone())
            })
            .flatten();
        let helius_sender_tip_lamports = (options.helius_sender_enabled
            && options.send_lane_mode.uses_helius_sender_tip())
        .then_some(options.helius_sender_tip_lamports)
        .flatten();
        let helius_sender_tip_account = (options.helius_sender_enabled
            && options.send_lane_mode.uses_helius_sender_tip())
        .then(|| options.helius_sender_tip_account.clone())
        .flatten();
        Self {
            schema: "copytrade.rustTrailingSell.v1",
            observed_at_ms: buy_line.observed_at_ms,
            execution_at_ms: step_started_at_ms,
            provider: "shredstream",
            source: "jito-proxy",
            endpoint: buy_line.endpoint.clone(),
            observed_wallet: buy_line.observed_wallet_string(),
            copy_wallet: buy_line.copy_wallet.clone(),
            observed_signature: buy_line.observed_signature_string(),
            buy_send_signature: buy_line.send_signature.clone(),
            copy_wallet_token_account: buy_line.copy_wallet_token_account.clone(),
            slot: buy_line.slot,
            selected_route: buy_line.selected_route,
            mint: buy_line.mint_string(),
            step_index,
            total_steps,
            delay_ms: step.delay_ms,
            percent: step.percent,
            percent_basis: plan.percent_basis,
            mode: plan.mode,
            schedule_anchor_at_ms: anchor_at_ms,
            due_at_ms,
            step_started_at_ms,
            drift_ms: step_started_at_ms as i128 - due_at_ms as i128,
            sell_slippage_percent: plan.sell_slippage_percent,
            sell_priority_fee_sol: plan.sell_priority_fee_sol,
            priority_fee_micro_lamports: plan
                .priority_fee_micro_lamports
                .or(options.sell_priority_fee_micro_lamports),
            jito_tip_lamports,
            jito_tip_account,
            helius_sender_tip_lamports,
            helius_sender_tip_account,
            confirmation_checked: false,
            confirmation_ok: false,
            confirmation_slot: None,
            signed: false,
            simulated: false,
            sent: false,
            decision: "skip",
            reason: None,
            route_layout: None,
            instruction_count: 0,
            token_balance_raw: None,
            token_amount_raw: None,
            sell_context_source: None,
            sell_context_resolved_at_ms: None,
            sell_context_reason: None,
            cashback_detected: None,
            cashback_detection_source: None,
            cashback_sell_path_disabled: None,
            blockhash: None,
            submitted_at_ms: None,
            signature_returned_at_ms: None,
            buy_signature_to_submitted_ms: None,
            buy_signature_to_signature_returned_ms: None,
            copy_signature: None,
            send_signature: None,
            send_rpc_url_count: options.selected_send_rpc_url_count(),
            send_lane_mode: options.send_lane_mode.as_str(),
            send_rpc_winner: None,
            send_rpc_attempts: Vec::new(),
            send_rpc_errors: Vec::new(),
            simulation_error: None,
            simulation_units_consumed: None,
            simulation_logs: Vec::new(),
        }
    }

    fn skip(&mut self, reason: impl Into<String>) {
        self.decision = "skip";
        self.reason = Some(reason.into());
    }

    fn error(&mut self, reason: impl Into<String>) {
        self.decision = "error";
        self.reason = Some(reason.into());
    }

    fn mark_submitted(&mut self) {
        let timestamp = now_ms();
        self.submitted_at_ms = Some(timestamp);
        self.buy_signature_to_submitted_ms =
            Some(timestamp.saturating_sub(self.schedule_anchor_at_ms));
    }

    fn mark_signature_returned(&mut self) {
        let timestamp = now_ms();
        self.signature_returned_at_ms = Some(timestamp);
        self.buy_signature_to_signature_returned_ms =
            Some(timestamp.saturating_sub(self.schedule_anchor_at_ms));
    }
}

impl CopyExecutionOptions {
    fn migrated_amm_min_copy_lamports(&self) -> Result<u64, String> {
        sol_to_lamports(self.migrated_amm_min_copy_sol)
            .ok_or_else(|| "invalid migrated AMM min copy SOL guard".to_string())
    }

    fn max_total_copy_spend_lamports(&self) -> Result<Option<u64>, String> {
        self.max_total_copy_spend_sol
            .map(|value| {
                if !value.is_finite() {
                    return Err("invalid max total copy spend SOL guard".to_string());
                }
                if value <= 0.0 {
                    return Ok(None);
                }
                sol_to_lamports(value)
                    .map(Some)
                    .ok_or_else(|| "invalid max total copy spend SOL guard".to_string())
            })
            .transpose()
            .map(Option::flatten)
    }

    fn tx_fee_config(&self) -> TxFeeConfig {
        TxFeeConfig {
            compute_unit_price_micro_lamports: self.priority_fee_micro_lamports,
            jito_tip_lamports: self
                .send_lane_mode
                .uses_jito_tip()
                .then_some(self.jito_tip_lamports)
                .flatten(),
            jito_tip_account: self
                .send_lane_mode
                .uses_jito_tip()
                .then(|| self.jito_tip_account.clone())
                .flatten(),
            helius_sender_tip_lamports: (self.helius_sender_enabled
                && self.send_lane_mode.uses_helius_sender_tip())
            .then_some(self.helius_sender_tip_lamports)
            .flatten(),
            helius_sender_tip_account: (self.helius_sender_enabled
                && self.send_lane_mode.uses_helius_sender_tip())
            .then(|| self.helius_sender_tip_account.clone())
            .flatten(),
        }
    }

    fn sell_tx_fee_config(&self) -> TxFeeConfig {
        TxFeeConfig {
            compute_unit_price_micro_lamports: self.sell_priority_fee_micro_lamports,
            jito_tip_lamports: self
                .send_lane_mode
                .uses_jito_tip()
                .then_some(self.sell_jito_tip_lamports)
                .flatten(),
            jito_tip_account: self
                .send_lane_mode
                .uses_jito_tip()
                .then(|| self.sell_jito_tip_account.clone())
                .flatten(),
            helius_sender_tip_lamports: (self.helius_sender_enabled
                && self.send_lane_mode.uses_helius_sender_tip())
            .then_some(self.helius_sender_tip_lamports)
            .flatten(),
            helius_sender_tip_account: (self.helius_sender_enabled
                && self.send_lane_mode.uses_helius_sender_tip())
            .then(|| self.helius_sender_tip_account.clone())
            .flatten(),
        }
    }

    fn send_config(&self) -> SendConfig {
        SendConfig {
            fast_copy_send: self.fast_copy_send,
            max_retries: self.send_max_retries,
            http_timeout_ms: self.send_http_timeout_ms,
            log_lanes: self.send_lane_logging,
        }
    }

    fn validate_helius_sender(&self) -> std::result::Result<(), String> {
        if !self.helius_sender_enabled {
            return Ok(());
        }
        if !self.send_fanout {
            return Err("JITO_HELIUS_SENDER_ENABLED requires JITO_SEND_FANOUT=YES".to_string());
        }
        if !self.fast_copy_send {
            return Err("JITO_HELIUS_SENDER_ENABLED requires JITO_FAST_COPY_SEND=YES".to_string());
        }
        if self.helius_sender_urls.is_empty() {
            return Err("JITO_HELIUS_SENDER_ENABLED requires JITO_HELIUS_SENDER_URLS".to_string());
        }
        if self.priority_fee_micro_lamports.unwrap_or(0) == 0 {
            return Err(
                "JITO_HELIUS_SENDER_ENABLED requires JITO_PRIORITY_FEE_MICRO_LAMPORTS".to_string(),
            );
        }
        let Some(tip_lamports) = self.helius_sender_tip_lamports else {
            return Err(
                "JITO_HELIUS_SENDER_ENABLED requires JITO_HELIUS_SENDER_TIP_LAMPORTS".to_string(),
            );
        };
        let min_tip = helius_sender_min_tip_lamports(self.helius_sender_swqos_only);
        if tip_lamports < min_tip {
            return Err(format!(
                "JITO_HELIUS_SENDER_TIP_LAMPORTS must be >= {min_tip} lamports"
            ));
        }
        let Some(tip_account) = self.helius_sender_tip_account.as_deref() else {
            return Err(
                "JITO_HELIUS_SENDER_ENABLED requires JITO_HELIUS_SENDER_TIP_ACCOUNT".to_string(),
            );
        };
        Pubkey::from_str(tip_account)
            .map_err(|_| "JITO_HELIUS_SENDER_TIP_ACCOUNT must be a valid pubkey".to_string())?;
        Ok(())
    }

    fn validate_send_lane_mode(&self) -> std::result::Result<(), String> {
        if self.send_lane_mode.uses_jito_tip()
            && self.jito_tip_lamports.unwrap_or(0) > 0
            && self.jito_tip_account.is_none()
        {
            return Err(
                "JITO_TIP_ACCOUNT must be set when JITO_TIP_LAMPORTS is positive".to_string(),
            );
        }
        if self.send_lane_mode.uses_jito_tip()
            && self.sell_jito_tip_lamports.unwrap_or(0) > 0
            && self.sell_jito_tip_account.is_none()
        {
            return Err(
                "JITO_SELL_TIP_ACCOUNT must be set when JITO_SELL_TIP_LAMPORTS is positive"
                    .to_string(),
            );
        }

        match self.send_lane_mode {
            SendLaneMode::Mixed => Ok(()),
            SendLaneMode::RpcOnly => {
                if self.selected_send_rpc_urls().is_empty() {
                    return Err(
                        "JITO_SEND_LANE_MODE=rpc_only requires SOLANA_RPC_URL or JITO_SEND_RPC_URLS"
                            .to_string(),
                    );
                }
                Ok(())
            }
            SendLaneMode::JitoOnly => {
                if !self.send_fanout {
                    return Err(
                        "JITO_SEND_LANE_MODE=jito_only requires JITO_SEND_FANOUT=YES".to_string(),
                    );
                }
                if self.jito_send_urls.is_empty() {
                    return Err(
                        "JITO_SEND_LANE_MODE=jito_only requires JITO_BLOCK_ENGINE_SEND_URLS"
                            .to_string(),
                    );
                }
                Ok(())
            }
            SendLaneMode::HeliusSenderOnly => {
                if !self.helius_sender_enabled {
                    return Err(
                        "JITO_SEND_LANE_MODE=helius_sender_only requires JITO_HELIUS_SENDER_ENABLED=YES"
                            .to_string(),
                    );
                }
                Ok(())
            }
        }
    }

    fn auto_sell_after_buy_enabled(&self) -> bool {
        self.auto_sell_after_buy && !self.isolate_buy_latency_test
    }

    fn simulate_auto_sell_enabled(&self) -> bool {
        self.simulate_auto_sell && !self.isolate_buy_latency_test
    }

    fn rust_trailing_sells_enabled(&self) -> bool {
        self.rust_trailing_sells_enabled && !self.isolate_buy_latency_test
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
        let mut endpoints = if self.send_lane_mode.uses_rpc_lanes() {
            self.selected_send_rpc_urls()
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
                    sender_mode: None,
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        if self.send_fanout {
            if self.send_lane_mode.uses_jito_lanes() {
                endpoints.extend(self.jito_send_urls.iter().enumerate().map(|(index, url)| {
                    let url = jito_transaction_url(url);
                    SendEndpoint {
                        label: format!("jito-{}:{}", index + 1, rpc_url_label(&url)),
                        url,
                        kind: SendEndpointKind::Jito,
                        auth_uuid: self.jito_auth_uuid.clone(),
                        sender_mode: None,
                    }
                }));
            }
            if self.helius_sender_enabled && self.send_lane_mode.uses_helius_sender_lanes() {
                endpoints.extend(
                    self.helius_sender_urls
                        .iter()
                        .enumerate()
                        .map(|(index, url)| {
                            let url = helius_sender_url(url, self.helius_sender_swqos_only);
                            let mode = helius_sender_mode(self.helius_sender_swqos_only);
                            SendEndpoint {
                                label: format!(
                                    "helius-sender-{}-{}:{}",
                                    index + 1,
                                    mode,
                                    rpc_url_label(&url)
                                ),
                                url,
                                kind: SendEndpointKind::HeliusSender,
                                auth_uuid: None,
                                sender_mode: Some(mode),
                            }
                        }),
                );
            }
        }

        endpoints
    }
}

fn helius_sender_min_tip_lamports(swqos_only: bool) -> u64 {
    if swqos_only {
        HELIUS_SENDER_SWQOS_ONLY_MIN_TIP_LAMPORTS
    } else {
        HELIUS_SENDER_MIN_TIP_LAMPORTS
    }
}

fn helius_sender_mode(swqos_only: bool) -> &'static str {
    if swqos_only {
        "swqos"
    } else {
        "fast"
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

fn helius_sender_url(url: &str, swqos_only: bool) -> String {
    let trimmed = url.trim();
    let (base, query) = trimmed
        .split_once('?')
        .map(|(base, query)| (base, Some(query)))
        .unwrap_or((trimmed, None));
    let base = base.trim_end_matches('/');
    let mut with_fast = if base.ends_with("/fast") {
        base.to_string()
    } else {
        format!("{base}/fast")
    };
    if let Some(query) = query.filter(|value| !value.is_empty()) {
        with_fast.push('?');
        with_fast.push_str(query);
    }
    if swqos_only {
        with_fast.push(if with_fast.contains('?') { '&' } else { '?' });
        with_fast.push_str("swqos_only=true");
    }
    with_fast
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
        SendEndpointKind::HeliusSender => "helius_sender",
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
        mode: endpoint.sender_mode,
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
                mode: endpoint.sender_mode,
                status: "submitted",
                duration_ms: started_at.elapsed().as_millis(),
                signature: Some(signature.clone()),
                error: None,
            };
            if config.log_lanes {
                eprintln!(
                    "sendTransaction lane submitted: label={} kind={} durationMs={}",
                    attempt.label, attempt.kind, attempt.duration_ms
                );
            }
            SendAttemptOutcome {
                attempt,
                finished_at_ms: now_ms(),
                signature: Some(signature),
                error: None,
            }
        }
        Err(error) => {
            let sanitized = send_error_message(endpoint, &error);
            let attempt = SendRpcAttemptLine {
                label: endpoint.label.clone(),
                kind: send_endpoint_kind(endpoint),
                mode: endpoint.sender_mode,
                status: "failed",
                duration_ms: started_at.elapsed().as_millis(),
                signature: None,
                error: Some(sanitized.clone()),
            };
            if config.log_lanes {
                eprintln!(
                    "sendTransaction lane failed: label={} kind={} durationMs={} error={}",
                    attempt.label, attempt.kind, attempt.duration_ms, sanitized
                );
            }
            SendAttemptOutcome {
                attempt,
                finished_at_ms: now_ms(),
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

fn serialize_pubkey<S>(pubkey: &Pubkey, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&pubkey.to_string())
}

fn serialize_signature_bytes<S>(signature: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&signature_bytes_to_string(*signature))
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

#[derive(Debug, Deserialize)]
struct AccountInfoResult {
    value: Option<AccountInfoValue>,
}

#[derive(Debug, Deserialize)]
struct AccountInfoValue {
    data: AccountInfoData,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AccountInfoData {
    Encoded((String, String)),
    Legacy(Vec<String>),
}

impl AccountInfoData {
    fn encoded_base64(&self) -> Option<&str> {
        match self {
            Self::Encoded((data, encoding)) if encoding == "base64" => Some(data.as_str()),
            Self::Legacy(values) => values.first().map(String::as_str),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct SignatureConfirmation {
    checked: bool,
    status: &'static str,
    ok: bool,
    slot: Option<u64>,
    block_position: Option<BlockPosition>,
    block_position_error: Option<String>,
    confirmation_status: Option<String>,
    err: Option<serde_json::Value>,
    reason: Option<String>,
}

#[derive(Debug)]
struct BlockPosition {
    tx_count: usize,
    landed_tx_index: Option<usize>,
    observed_tx_index: Option<usize>,
    txs_after_observed: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SignatureStatusesResult {
    value: Vec<Option<SignatureStatusValue>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignatureStatusValue {
    confirmation_status: Option<String>,
    err: Option<serde_json::Value>,
    slot: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct BlockPositionResult {
    signatures: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{
        DirectPumpAccounts, FlashxPumpLayout, FlashxPumpResolvedAccounts, MigratedAmmAccounts,
        RouteInstructionAccount,
    };
    use crate::planner::{execution_plan_line, PlannerOptions};
    use crate::tx_builder::TxFeeConfig;
    use solana_message::AddressLookupTableAccount;
    use solana_pubkey::Pubkey;

    const COPY_WALLET: &str = "FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W";

    fn disabled_options() -> CopyExecutionOptions {
        CopyExecutionOptions {
            endpoint: "local".to_string(),
            enable_copy_send: false,
            dry_run: true,
            simulate_copy_tx: false,
            fast_copy_send: false,
            send_fanout: false,
            send_lane_mode: SendLaneMode::Mixed,
            send_rpc_urls: Vec::new(),
            jito_send_urls: Vec::new(),
            jito_auth_uuid: None,
            helius_sender_enabled: false,
            helius_sender_urls: Vec::new(),
            helius_sender_swqos_only: false,
            helius_sender_tip_lamports: None,
            helius_sender_tip_account: None,
            max_copy_sol: None,
            max_total_copy_spend_sol: None,
            migrated_amm_min_copy_sol: DEFAULT_MIGRATED_AMM_MIN_COPY_SOL,
            migrated_amm_small_copy_mode: MigratedAmmSmallCopyMode::Skip,
            copy_wallet: None,
            copy_keypair_path: None,
            solana_rpc_url: None,
            auto_sell_after_buy: false,
            auto_sell_delay_ms: 1_000,
            rust_trailing_sells_enabled: false,
            rust_trailing_sell_confirmation_timeout_ms: 30_000,
            rust_trailing_sell_confirmation_poll_ms: 100,
            simulate_auto_sell: false,
            isolate_buy_latency_test: false,
            send_max_retries: 3,
            send_http_timeout_ms: 0,
            send_lane_logging: false,
            priority_fee_micro_lamports: None,
            jito_tip_lamports: None,
            jito_tip_account: None,
            sell_priority_fee_micro_lamports: None,
            sell_jito_tip_lamports: None,
            sell_jito_tip_account: None,
            warm_send_endpoints: false,
            send_endpoint_warm_interval_ms: 0,
            copy_wallet_balance_guard: true,
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
                target_wallet: "CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o".to_string(),
                action: Action::Buy,
                mint: "So11111111111111111111111111111111111111112".to_string(),
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

    fn execution_line(
        plan: &ExecutionPlanLine,
        observed_action: Action,
        observed_sol_amount: Option<f64>,
        options: &CopyExecutionOptions,
    ) -> CopyExecutionLine {
        let request =
            CopyRuntimeRequest::from_execution_plan(plan, observed_action, observed_sol_amount);
        CopyExecutionLine::new(&request, options, sample_timings())
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
                    user_volume_accumulator: Some(dummy),
                    fee_config: dummy,
                    fee_program: dummy,
                    bonding_curve_v2: dummy,
                    buyback_fee_recipient: dummy,
                    buyback_fee_recipient_token_account: None,
                    router_amount: Some(990_000),
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
                    user_volume_accumulator_quote_token_account: None,
                    fee_config: dummy,
                    fee_program: dummy,
                    pool_v2: Some(dummy),
                    buyback_fee_recipient: Some(dummy),
                    buyback_fee_recipient_token_account: Some(dummy),
                })
            }
        };

        let accounts = match &resolved_accounts {
            FlashxPumpResolvedAccounts::DirectPump(accounts) => {
                direct_pump_route_accounts(accounts)
            }
            FlashxPumpResolvedAccounts::MigratedAmm(_) => Vec::new(),
        };

        RouteContext::FlashxPump(crate::parser::FlashxPumpRouteContext {
            layout,
            program_id: flashx_router_program,
            accounts: accounts.into(),
            data: data.into(),
            resolved_accounts,
        })
    }

    fn route_account(
        pubkey: Pubkey,
        is_writable: bool,
        is_signer: bool,
    ) -> RouteInstructionAccount {
        RouteInstructionAccount {
            pubkey,
            is_writable,
            is_signer,
        }
    }

    fn direct_pump_route_accounts(accounts: &DirectPumpAccounts) -> Vec<RouteInstructionAccount> {
        let readonly = |pubkey| route_account(pubkey, false, false);
        let writable = |pubkey| route_account(pubkey, true, false);
        let signer = |pubkey| route_account(pubkey, true, true);
        let mut route_accounts = vec![
            writable(accounts.user_token_account),
            signer(accounts.target_wallet),
            readonly(accounts.mint),
            writable(accounts.fee_recipient),
            readonly(accounts.flashx_router_program),
            readonly(accounts.pump_program),
            readonly(accounts.global_config),
            writable(accounts.bonding_curve),
            writable(accounts.associated_bonding_curve),
            readonly(accounts.system_program),
            readonly(accounts.token_program),
            writable(accounts.creator_vault),
            readonly(accounts.event_authority),
            readonly(accounts.fee_config),
            readonly(accounts.fee_program),
            readonly(accounts.bonding_curve_v2),
            writable(accounts.buyback_fee_recipient),
        ];
        if let Some(account) = accounts.buyback_fee_recipient_token_account {
            route_accounts.push(writable(account));
        }
        route_accounts
    }

    fn flashx_direct_sell_context() -> RouteContext {
        flashx_direct_sell_context_with_amount(1)
    }

    fn flashx_direct_sell_context_with_amount(amount: u64) -> RouteContext {
        let mut route_context = flashx_context(FlashxPumpLayout::DirectPump, 1);
        let RouteContext::FlashxPump(context) = &mut route_context;
        let data = std::sync::Arc::make_mut(&mut context.data);
        data[1..9].copy_from_slice(&amount.to_le_bytes());
        data[17] = 1;
        let FlashxPumpResolvedAccounts::DirectPump(accounts) = &mut context.resolved_accounts
        else {
            panic!("fixture should be direct Pump");
        };
        accounts.router_amount = Some(amount);
        route_context
    }

    fn flashx_cashback_migrated_context() -> RouteContext {
        let mut route_context = flashx_context(FlashxPumpLayout::MigratedAmm, 1);
        let RouteContext::FlashxPump(context) = &mut route_context;
        let FlashxPumpResolvedAccounts::MigratedAmm(accounts) = &mut context.resolved_accounts
        else {
            panic!("fixture should be migrated AMM");
        };
        accounts.user_volume_accumulator_quote_token_account =
            Some(COPY_WALLET.parse().expect("copy wallet pubkey"));
        route_context
    }

    fn executor(options: CopyExecutionOptions) -> CopyExecutor {
        let send_endpoints = Arc::new(options.selected_send_endpoints());
        CopyExecutor {
            options,
            keypair: None,
            keypairs: ArcSwap::from_pointee(HashMap::new()),
            client: send_http_client(),
            send_endpoints,
            blockhash_cache: None,
            address_lookup_tables: AddressLookupTableCache::default(),
            wallet_balance_cache: None,
            pda_cache: CopyPdaCache::default(),
            direct_pump_sell_contexts: Mutex::new(DirectPumpSellContextCache::new(
                DIRECT_PUMP_SELL_CONTEXT_CACHE_CAPACITY,
            )),
        }
    }

    fn loaded_lookup_cache() -> AddressLookupTableCache {
        AddressLookupTableCache::with_table_accounts_for_tests(vec![AddressLookupTableAccount {
            key: Pubkey::new_unique(),
            addresses: vec![Pubkey::new_unique(), Pubkey::new_unique()],
        }])
    }

    fn sample_transfer_instruction(payer: &Pubkey) -> solana_instruction::Instruction {
        solana_instruction::Instruction {
            program_id: *system_program_id(),
            accounts: vec![
                solana_instruction::AccountMeta::new(*payer, true),
                solana_instruction::AccountMeta::new(Pubkey::new_unique(), false),
            ],
            data: [2u32.to_le_bytes().to_vec(), 1u64.to_le_bytes().to_vec()].concat(),
        }
    }

    fn write_temp_keypair_file(label: &str, keypair: &Keypair) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "jito-copy-keypair-{label}-{}-{}.json",
            std::process::id(),
            now_ms()
        ));
        let file = std::fs::File::create(&path).expect("create temp keypair");
        serde_json::to_writer(file, &keypair.to_bytes().to_vec()).expect("write temp keypair");
        path
    }

    #[tokio::test]
    async fn cashback_sell_guard_detects_migrated_amm_cashback_from_route_context() {
        let executor = executor(disabled_options());
        let guard = executor
            .cashback_sell_guard_for_route_context(&flashx_cashback_migrated_context())
            .await
            .expect("route-context detection should not require RPC")
            .expect("cashback route should be guarded");

        assert_eq!(
            guard,
            CashbackSellGuard {
                source: "migrated-amm-route-context"
            }
        );
    }

    #[tokio::test]
    async fn cashback_sell_guard_allows_non_cashback_migrated_amm_route_context() {
        let executor = executor(disabled_options());
        let guard = executor
            .cashback_sell_guard_for_route_context(&flashx_context(
                FlashxPumpLayout::MigratedAmm,
                1,
            ))
            .await
            .expect("route-context detection should not require RPC");

        assert_eq!(guard, None);
    }

    #[tokio::test]
    async fn cashback_sell_guard_fails_closed_for_direct_pump_without_rpc() {
        let executor = executor(disabled_options());
        let error = executor
            .cashback_sell_guard_for_route_context(&flashx_context(FlashxPumpLayout::DirectPump, 1))
            .await
            .unwrap_err();

        assert_eq!(error, "cashback detection failed: missing SOLANA_RPC_URL");
    }

    #[test]
    fn execution_line_defaults_to_safe_disabled_state() {
        let plan = allowed_plan();

        let line = execution_line(&plan, Action::Buy, Some(0.0005), &disabled_options());

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
    fn cached_blockhash_reaches_executor_signer_as_typed_hash() {
        let cached_blockhash = crate::blockhash::CachedBlockhash {
            hash: Hash::default(),
            last_valid_block_height: 123,
            fetched_at_ms: 456,
        };
        let expected_jsonl_blockhash = cached_blockhash.hash.to_string();
        let keypair = Keypair::new();

        let signed_tx = sign_copy_transaction(
            &[],
            &keypair,
            cached_blockhash.hash,
            &AddressLookupTableCache::default(),
            TxVersionPreference::PreferV0WhenLookupsLoaded,
        )
        .expect("typed cached blockhash should sign transaction");

        assert_eq!(signed_tx.tx_version, "legacy");
        assert!(!signed_tx.signature.is_empty());
        assert_eq!(expected_jsonl_blockhash, Hash::default().to_string());
    }

    #[test]
    fn direct_pump_preference_signs_legacy_even_when_lookup_tables_are_loaded() {
        let keypair = Keypair::new();
        let lookup_cache = loaded_lookup_cache();

        let signed_tx = sign_copy_transaction(
            &[sample_transfer_instruction(&keypair.pubkey())],
            &keypair,
            Hash::default(),
            &lookup_cache,
            TxVersionPreference::PreferLegacyWhenSafe,
        )
        .expect("direct-pump sized transaction should sign as legacy");

        assert_eq!(signed_tx.tx_version, "legacy");
        assert!(matches!(
            signed_tx.transaction.message,
            VersionedMessage::Legacy(_)
        ));
        assert!(!signed_tx.signature.is_empty());
    }

    #[test]
    fn migrated_preference_keeps_v0_when_lookup_tables_are_loaded() {
        let keypair = Keypair::new();
        let lookup_cache = loaded_lookup_cache();

        let signed_tx = sign_copy_transaction(
            &[sample_transfer_instruction(&keypair.pubkey())],
            &keypair,
            Hash::default(),
            &lookup_cache,
            TxVersionPreference::PreferV0WhenLookupsLoaded,
        )
        .expect("lookup-backed migrated transaction should sign as v0");

        assert_eq!(signed_tx.tx_version, "v0");
        assert!(matches!(
            signed_tx.transaction.message,
            VersionedMessage::V0(_)
        ));
        assert!(!signed_tx.signature.is_empty());
    }

    #[test]
    fn direct_pump_full_copy_build_signs_legacy_even_when_lookup_tables_are_loaded() {
        let keypair = Keypair::new();
        let copy_wallet = keypair.pubkey().to_string();
        let route_context = flashx_context(FlashxPumpLayout::DirectPump, 1);
        let fee_config = TxFeeConfig {
            compute_unit_price_micro_lamports: Some(250_000),
            jito_tip_lamports: Some(10_000),
            jito_tip_account: Some(Pubkey::new_unique().to_string()),
            helius_sender_tip_lamports: None,
            helius_sender_tip_account: None,
        };
        let build = crate::tx_builder::build_full_copy_unsigned_flashx_pump_with_fees_and_cache(
            Some(&route_context),
            &copy_wallet,
            COPY_WALLET,
            &fee_config,
            None,
        )
        .expect("direct-Pump full copy build should succeed");
        assert_eq!(build.route_layout, "direct-pump");
        assert!(build.instructions.len() > 1);

        let signed_tx = sign_copy_transaction(
            &build.instructions,
            &keypair,
            Hash::default(),
            &loaded_lookup_cache(),
            tx_version_preference_for_route_layout(build.route_layout),
        )
        .expect("full direct-Pump copy build should sign");

        assert_eq!(signed_tx.tx_version, "legacy");
        assert!(matches!(
            signed_tx.transaction.message,
            VersionedMessage::Legacy(_)
        ));
    }

    #[test]
    fn direct_pump_legacy_preference_fails_safe_when_account_set_is_too_large() {
        let keypair = Keypair::new();
        let lookup_cache = loaded_lookup_cache();
        let mut accounts = Vec::new();
        for _ in 0..260 {
            accounts.push(solana_instruction::AccountMeta::new_readonly(
                Pubkey::new_unique(),
                false,
            ));
        }
        let instruction = solana_instruction::Instruction {
            program_id: Pubkey::new_unique(),
            accounts,
            data: Vec::new(),
        };

        let error = sign_copy_transaction(
            &[instruction],
            &keypair,
            Hash::default(),
            &lookup_cache,
            TxVersionPreference::PreferLegacyWhenSafe,
        )
        .expect_err("oversized account set should fail closed");

        assert!(error.contains("compile v0 message"));
    }

    #[test]
    fn route_layout_selects_legacy_preference_only_for_direct_pump() {
        assert_eq!(
            tx_version_preference_for_route_layout("direct-pump"),
            TxVersionPreference::PreferLegacyWhenSafe
        );
        assert_eq!(
            tx_version_preference_for_route_layout("migrated-amm"),
            TxVersionPreference::PreferV0WhenLookupsLoaded
        );
    }

    #[test]
    fn execution_line_records_auto_sell_simulation_request() {
        let plan = allowed_plan();
        let mut options = disabled_options();
        options.simulate_auto_sell = true;

        let line = execution_line(&plan, Action::Buy, Some(0.0005), &options);

        assert!(line.auto_sell_simulation_requested);
    }

    #[test]
    fn buy_latency_isolation_forces_auto_sell_off_for_execution_lines() {
        let plan = allowed_plan();
        let mut options = disabled_options();
        options.auto_sell_after_buy = true;
        options.simulate_auto_sell = true;
        options.isolate_buy_latency_test = true;

        let line = execution_line(&plan, Action::Buy, Some(0.0005), &options);

        assert!(line.buy_latency_test_isolated);
        assert!(!line.auto_sell_enabled);
        assert!(!line.auto_sell_simulation_requested);
    }

    #[test]
    fn snapshot_keypairs_are_loaded_by_pubkey() {
        let matching_keypair = Keypair::new();
        let mismatched_keypair = Keypair::new();
        let matching_path = write_temp_keypair_file("matching", &matching_keypair);
        let mismatched_path = write_temp_keypair_file("mismatched", &mismatched_keypair);
        let missing_wallet = Pubkey::new_unique();

        let keypairs = CopyExecutor::load_snapshot_keypairs(vec![
            (matching_keypair.pubkey(), matching_path.clone()),
            (missing_wallet, mismatched_path.clone()),
        ]);

        assert_eq!(keypairs.len(), 1);
        assert!(keypairs.contains_key(&matching_keypair.pubkey()));
        assert!(!keypairs.contains_key(&missing_wallet));

        let _ = std::fs::remove_file(matching_path);
        let _ = std::fs::remove_file(mismatched_path);
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
        let target_wallet = Pubkey::from_str(&plan.target_wallet).unwrap();
        let mint = Pubkey::from_str(&plan.mint).unwrap();
        executor.observe_direct_pump_sell_route_context(&target_wallet, &mint, Some(&sell_context));
        let route_context =
            auto_sell_route_context_for_plan(&executor, &plan).expect("sell context should cache");

        assert!(is_direct_pump_sell_route_context(&route_context));
    }

    #[test]
    fn direct_pump_trailing_sell_derives_from_buy_context_without_target_sell() {
        let mut options = disabled_options();
        options.rust_trailing_sells_enabled = true;
        let executor = executor(options);
        let mut plan = allowed_plan();
        plan.route_context = Some(flashx_context(FlashxPumpLayout::DirectPump, 1));

        let resolution = trailing_sell_route_context_for_plan(&executor, &plan, COPY_WALLET)
            .expect("rust trailing sell should derive from copy-buy route context");

        assert_eq!(resolution.source, "derived");
        assert_eq!(
            resolution.reason,
            "direct Pump sell context derived from copy-buy route accounts"
        );
        assert!(!is_direct_pump_sell_route_context(
            &resolution.route_context
        ));
    }

    #[test]
    fn direct_pump_trailing_sell_blocks_target_sell_context() {
        let mut options = disabled_options();
        options.rust_trailing_sells_enabled = true;
        let executor = executor(options);
        let mut plan = allowed_plan();
        plan.route_context = Some(flashx_direct_sell_context());

        assert_eq!(
            trailing_sell_route_context_for_plan(&executor, &plan, COPY_WALLET).unwrap_err(),
            MissingTrailingSellRouteContext {
                source: "target_context_blocked",
                reason: "missing copy-wallet sell route context"
            }
        );

        let sell_context = flashx_direct_sell_context();
        let target_wallet = Pubkey::from_str(&plan.target_wallet).unwrap();
        let mint = Pubkey::from_str(&plan.mint).unwrap();
        executor.observe_direct_pump_sell_route_context(&target_wallet, &mint, Some(&sell_context));
        assert_eq!(
            trailing_sell_route_context_for_plan(&executor, &plan, COPY_WALLET).unwrap_err(),
            MissingTrailingSellRouteContext {
                source: "target_context_blocked",
                reason: "missing copy-wallet sell route context"
            }
        );
    }

    #[test]
    fn direct_pump_trailing_sell_uses_copy_wallet_cached_sell_context() {
        let mut options = disabled_options();
        options.rust_trailing_sells_enabled = true;
        let executor = executor(options);
        let mut plan = allowed_plan();
        plan.route_context = Some(flashx_direct_sell_context());

        let sell_context = flashx_direct_sell_context();
        let copy_wallet = Pubkey::from_str(COPY_WALLET).unwrap();
        let mint = Pubkey::from_str(&plan.mint).unwrap();
        executor.observe_direct_pump_sell_route_context(&copy_wallet, &mint, Some(&sell_context));
        let resolution = trailing_sell_route_context_for_plan(&executor, &plan, COPY_WALLET)
            .expect("rust trailing sell should use copy-wallet sell-side context");

        assert!(is_direct_pump_sell_route_context(&resolution.route_context));
        assert_eq!(resolution.source, "cached_copy_wallet");
    }

    #[test]
    fn direct_pump_sell_context_cache_uses_pubkey_keys() {
        let mut cache = DirectPumpSellContextCache::new(2);
        let target_wallet = Pubkey::from_str(COPY_WALLET).unwrap();
        let mint = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();
        let other_mint = Pubkey::new_unique();
        let sell_context = flashx_direct_sell_context();

        cache.insert(&target_wallet, &mint, sell_context.clone());

        assert!(is_direct_pump_sell_route_context(
            &cache.get(&target_wallet, &mint).expect("typed cache hit")
        ));
        assert!(cache.get(&target_wallet, &other_mint).is_none());
        assert!(cache
            .get_str(COPY_WALLET, "So11111111111111111111111111111111111111112")
            .is_some());
        assert!(cache.get_str("not-a-pubkey", &mint.to_string()).is_none());
    }

    #[test]
    fn target_sell_auto_sell_uses_current_sell_side_context_without_cache() {
        let mut options = disabled_options();
        options.auto_sell_after_buy = true;
        let executor = executor(options);
        let mut plan = allowed_plan();
        plan.route_context = Some(flashx_direct_sell_context());
        let sell_request = CopyRuntimeRequest::from_execution_plan(&plan, Action::Sell, None);
        let buy_request = CopyRuntimeRequest::from_execution_plan(&plan, Action::Buy, Some(0.0005));

        assert!(executor.should_spawn_auto_sell_on_target_sell(&sell_request));
        assert!(!executor.should_spawn_auto_sell_on_target_sell(&buy_request));

        let route_context = auto_sell_route_context_for_plan(&executor, &plan)
            .expect("target sell context should be directly usable");
        assert!(is_direct_pump_sell_route_context(&route_context));
    }

    #[test]
    fn target_sell_auto_sell_is_not_armed_by_rust_trailing_sell_mode() {
        let mut options = disabled_options();
        options.rust_trailing_sells_enabled = true;
        let executor = executor(options);
        let mut plan = allowed_plan();
        plan.route_context = Some(flashx_direct_sell_context());
        let sell_request = CopyRuntimeRequest::from_execution_plan(&plan, Action::Sell, None);
        let buy_request = CopyRuntimeRequest::from_execution_plan(&plan, Action::Buy, Some(0.0005));

        assert!(!executor.should_spawn_auto_sell_on_target_sell(&sell_request));
        assert!(!executor.should_spawn_auto_sell_on_target_sell(&buy_request));
    }

    #[test]
    fn target_sell_auto_sell_keeps_route_context_through_shadow_skip() {
        let mut options = disabled_options();
        options.auto_sell_after_buy = true;
        let executor = executor(options);
        let dummy: Pubkey = COPY_WALLET.parse().unwrap();
        let parsed = crate::parser::ParsedTrade {
            target_wallet: dummy,
            action: Action::Sell,
            mint: dummy,
            route: Route::FlashxPump,
            sol_amount: None,
            token_amount: Some(42.0),
            route_context: Some(flashx_direct_sell_context()),
        };
        let signal = crate::event::shadow_signal_line(
            1,
            "local".to_string(),
            "observed".to_string(),
            2,
            1,
            &parsed,
        );

        assert!(!signal.copyable);
        assert!(signal.route_context.is_some());
        let plan = execution_plan_line(
            &signal,
            3,
            PlannerOptions {
                copy_sol_amount: None,
            },
        );

        assert!(!plan.allowed);
        assert!(plan.route_context.is_some());
        let request = CopyRuntimeRequest::from_execution_plan(&plan, Action::Sell, None);
        assert!(executor.should_spawn_auto_sell_on_target_sell(&request));
    }

    #[test]
    fn max_copy_sol_zero_or_missing_is_uncapped() {
        assert_eq!(max_copy_sol_guard_reason(None, 2.0).unwrap(), None);
        assert_eq!(max_copy_sol_guard_reason(Some(0.0), 2.0).unwrap(), None);
    }

    #[test]
    fn migrated_amm_small_copy_guard_skips_or_floors_below_min() {
        let route_context = flashx_context(FlashxPumpLayout::MigratedAmm, 1);
        let mut options = disabled_options();
        let min_lamports = options.migrated_amm_min_copy_lamports().unwrap();

        assert_eq!(
            copy_spend_after_migrated_amm_guard(&options, Some(&route_context), 100_000).unwrap(),
            CopySpendDecision::Skip(format!(
                "migrated AMM copy spend 100000 lamports below min {} lamports",
                min_lamports
            ))
        );
        assert_eq!(
            copy_spend_after_migrated_amm_guard(&options, Some(&route_context), min_lamports)
                .unwrap(),
            CopySpendDecision::Use(min_lamports)
        );

        options.migrated_amm_small_copy_mode = MigratedAmmSmallCopyMode::Floor;
        assert_eq!(
            copy_spend_after_migrated_amm_guard(&options, Some(&route_context), 100_000).unwrap(),
            CopySpendDecision::Use(min_lamports)
        );
    }

    #[test]
    fn migrated_amm_small_copy_guard_does_not_touch_direct_pump() {
        let route_context = flashx_context(FlashxPumpLayout::DirectPump, 1);
        let options = disabled_options();

        assert_eq!(
            copy_spend_after_migrated_amm_guard(&options, Some(&route_context), 100_000).unwrap(),
            CopySpendDecision::Use(100_000)
        );
    }

    #[test]
    fn total_copy_spend_estimate_uses_copied_input_setup_fees_and_tip() {
        let route_context = flashx_context(FlashxPumpLayout::MigratedAmm, 1);
        let compute_budget_program = crate::parser::COMPUTE_BUDGET_PROGRAM_ID.parse().unwrap();
        let flashx_program = crate::parser::FLASHX_ROUTER_PROGRAM_ID.parse().unwrap();
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
        let mut flashx_setup_data = vec![1];
        flashx_setup_data.extend_from_slice(&777_000u64.to_le_bytes());
        flashx_setup_data.push(42);
        let mut tip_data = Vec::new();
        tip_data.extend_from_slice(&2u32.to_le_bytes());
        tip_data.extend_from_slice(&10_000u64.to_le_bytes());
        let build = crate::tx_builder::FullCopyUnsignedTxBuild {
            route_layout: "migrated-amm",
            copy_wallet_token_account: COPY_WALLET.parse().unwrap(),
            estimated_required_signer: COPY_WALLET.parse().unwrap(),
            setup_instruction_count: 5,
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
                    program_id: flashx_program,
                    accounts: vec![solana_instruction::AccountMeta::new(copy_wallet, true)],
                    data: flashx_setup_data,
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
            777_000
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
        assert_eq!(options.max_total_copy_spend_lamports().unwrap(), None);
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
    fn direct_buy_context_auto_sell_uses_copy_wallet_balance() {
        let route_context = flashx_context(FlashxPumpLayout::DirectPump, 23_000_000_000);

        assert_eq!(
            auto_sell_token_amount_raw(Some(&route_context), 47_000_000_000),
            47_000_000_000
        );
    }

    #[test]
    fn direct_sell_context_auto_sell_uses_copy_wallet_balance() {
        let route_context = flashx_direct_sell_context_with_amount(5_125_379_616);

        assert_eq!(
            auto_sell_token_amount_raw(Some(&route_context), 9_535_782_270),
            9_535_782_270
        );
    }

    #[test]
    fn direct_sell_context_auto_sell_keeps_smaller_balance_when_no_stale_tokens_exist() {
        let route_context = flashx_direct_sell_context_with_amount(23_000_000_000);

        assert_eq!(
            auto_sell_token_amount_raw(Some(&route_context), 22_500_000_000),
            22_500_000_000
        );
    }

    #[test]
    fn trailing_sell_custom_original_position_steps_match_node_semantics() {
        let plan = TrailingSellPlan {
            mode: TrailingSellMode::CustomSteps,
            percent_basis: TrailingSellPercentBasis::OriginalPosition,
            steps: vec![
                TrailingSellStep {
                    delay_ms: 500,
                    percent: 50.0,
                },
                TrailingSellStep {
                    delay_ms: 500,
                    percent: 100.0,
                },
            ],
            sell_slippage_percent: None,
            sell_priority_fee_sol: None,
            priority_fee_micro_lamports: None,
            jito_tip_lamports: None,
            jito_tip_account: None,
        };

        assert_eq!(
            effective_trailing_sell_steps(&plan),
            vec![
                TrailingSellStep {
                    delay_ms: 500,
                    percent: 50.0,
                },
                TrailingSellStep {
                    delay_ms: 1_000,
                    percent: 100.0,
                }
            ]
        );
    }

    #[test]
    fn trailing_sell_amount_uses_raw_balance_basis_points() {
        assert_eq!(trailing_sell_token_amount_raw(1_000_000, 50.0), 500_000);
        assert_eq!(trailing_sell_token_amount_raw(3, 33.333), 0);
        assert_eq!(trailing_sell_token_amount_raw(3, 100.0), 3);
    }

    #[test]
    fn trailing_sell_optimistic_balance_tracks_submitted_steps() {
        let first_remaining =
            update_optimistic_trailing_sell_balance_raw(None, Some(34_214_804), Some(17_107_402));
        assert_eq!(first_remaining, Some(17_107_402));

        let second_remaining = update_optimistic_trailing_sell_balance_raw(
            first_remaining,
            Some(34_214_804),
            Some(17_107_402),
        );
        assert_eq!(second_remaining, Some(0));
    }

    #[test]
    fn rust_trailing_sell_line_records_buy_identity_and_token_account() {
        let plan = allowed_plan();
        let mut buy_line = execution_line(&plan, Action::Buy, Some(0.0005), &disabled_options());
        buy_line.send_signature = Some("copy-buy-signature".to_string());
        buy_line.copy_wallet_token_account = Some("copy-token-account".to_string());
        let trailing_sell_plan = TrailingSellPlan {
            mode: TrailingSellMode::CustomSteps,
            percent_basis: TrailingSellPercentBasis::RemainingBalance,
            steps: vec![TrailingSellStep {
                delay_ms: 500,
                percent: 50.0,
            }],
            sell_slippage_percent: Some(8.0),
            sell_priority_fee_sol: Some(0.00002),
            priority_fee_micro_lamports: Some(250_000),
            jito_tip_lamports: Some(10_000),
            jito_tip_account: Some(COPY_WALLET.to_string()),
        };

        let line = RustTrailingSellLine::new(
            &buy_line,
            &trailing_sell_plan,
            0,
            1,
            trailing_sell_plan.steps[0],
            1_000,
            &disabled_options(),
        );
        let json = serde_json::to_value(&line).expect("trailing sell line serializes");

        assert_eq!(
            json.get("schema").and_then(serde_json::Value::as_str),
            Some("copytrade.rustTrailingSell.v1")
        );
        assert_eq!(
            json.get("buySendSignature")
                .and_then(serde_json::Value::as_str),
            Some("copy-buy-signature")
        );
        assert_eq!(
            json.get("copyWalletTokenAccount")
                .and_then(serde_json::Value::as_str),
            Some("copy-token-account")
        );
        assert_eq!(
            json.get("mint").and_then(serde_json::Value::as_str),
            Some(plan.mint.as_str())
        );
    }

    #[test]
    fn transaction_confirmation_line_records_failed_copy_buy_status() {
        let plan = allowed_plan();
        let mut line = execution_line(&plan, Action::Buy, Some(0.0005), &disabled_options());
        line.copy_wallet = Some(COPY_WALLET.to_string());
        line.sent = true;
        line.decision = "sent";
        line.route_layout = Some("direct-pump");
        line.send_signature = Some("copy-buy-signature".to_string());
        line.send_submitted_at_ms = Some(10);
        line.signature_returned_at_ms = Some(11);

        let confirmation = SignatureConfirmation {
            checked: true,
            status: "failed",
            ok: false,
            slot: Some(2),
            block_position: Some(BlockPosition {
                tx_count: 10,
                landed_tx_index: Some(7),
                observed_tx_index: Some(3),
                txs_after_observed: Some(4),
            }),
            block_position_error: None,
            confirmation_status: Some("confirmed".to_string()),
            err: Some(serde_json::json!({
                "InstructionError": [1, { "Custom": 6024 }]
            })),
            reason: Some("copy buy transaction landed with error".to_string()),
        };
        let confirmation_line =
            TransactionConfirmationLine::from_copy_execution(&line, confirmation);
        let json = serde_json::to_value(&confirmation_line).expect("confirmation line serializes");

        assert_eq!(
            json.get("schema").and_then(serde_json::Value::as_str),
            Some("copytrade.transactionConfirmation.v1")
        );
        assert_eq!(
            json.get("transactionRole")
                .and_then(serde_json::Value::as_str),
            Some("copy_buy")
        );
        assert_eq!(
            json.get("observedAction")
                .and_then(serde_json::Value::as_str),
            Some("buy")
        );
        assert_eq!(
            json.get("signature").and_then(serde_json::Value::as_str),
            Some("copy-buy-signature")
        );
        assert_eq!(
            json.get("status").and_then(serde_json::Value::as_str),
            Some("failed")
        );
        assert_eq!(
            json.get("ok").and_then(serde_json::Value::as_bool),
            Some(false)
        );
        assert_eq!(
            json.get("confirmationSlot")
                .and_then(serde_json::Value::as_u64),
            Some(2)
        );
        assert_eq!(
            json.get("landedBlockTxCount")
                .and_then(serde_json::Value::as_u64),
            Some(10)
        );
        assert_eq!(
            json.get("observedTxIndex")
                .and_then(serde_json::Value::as_u64),
            Some(3)
        );
        assert_eq!(
            json.get("landedTxIndex")
                .and_then(serde_json::Value::as_u64),
            Some(7)
        );
        assert_eq!(
            json.get("targetTxIndex")
                .and_then(serde_json::Value::as_u64),
            Some(3)
        );
        assert_eq!(
            json.get("copyTxIndex").and_then(serde_json::Value::as_u64),
            Some(7)
        );
        assert_eq!(
            json.get("txsAfterObserved")
                .and_then(serde_json::Value::as_i64),
            Some(4)
        );
        assert_eq!(
            json.get("sameSlotTxDelta")
                .and_then(serde_json::Value::as_i64),
            Some(4)
        );
        assert_eq!(
            json.get("txDelta").and_then(serde_json::Value::as_i64),
            Some(4)
        );
        assert!(json.get("err").is_some());
    }

    #[test]
    fn transaction_confirmation_line_records_target_auto_sell_status() {
        let plan = allowed_plan();
        let mut line = execution_line(&plan, Action::Sell, None, &disabled_options());
        line.copy_wallet = Some(COPY_WALLET.to_string());
        line.route_layout = Some("direct-pump");
        line.auto_sell_sent = true;
        line.auto_sell_decision = Some("sent");
        line.auto_sell_send_signature = Some("auto-sell-signature".to_string());
        line.auto_sell_submitted_at_ms = Some(20);
        line.auto_sell_signature_returned_at_ms = Some(21);

        let confirmation = SignatureConfirmation {
            checked: true,
            status: "failed",
            ok: false,
            slot: Some(43),
            block_position: None,
            block_position_error: None,
            confirmation_status: Some("confirmed".to_string()),
            err: Some(serde_json::json!({
                "InstructionError": [3, { "Custom": 6024 }]
            })),
            reason: Some("auto-sell transaction landed with error".to_string()),
        };
        let confirmation_line =
            TransactionConfirmationLine::from_auto_sell_execution(&line, confirmation);
        let json = serde_json::to_value(&confirmation_line).expect("confirmation line serializes");

        assert_eq!(
            json.get("transactionRole")
                .and_then(serde_json::Value::as_str),
            Some("target_auto_sell")
        );
        assert_eq!(
            json.get("observedAction")
                .and_then(serde_json::Value::as_str),
            Some("sell")
        );
        assert_eq!(
            json.get("signature").and_then(serde_json::Value::as_str),
            Some("auto-sell-signature")
        );
        assert_eq!(
            json.get("status").and_then(serde_json::Value::as_str),
            Some("failed")
        );
        assert_eq!(
            json.get("confirmationSlot")
                .and_then(serde_json::Value::as_u64),
            Some(43)
        );
        assert!(json.get("err").is_some());
    }

    #[test]
    fn transaction_confirmation_line_records_trailing_sell_submitted_not_landed_status() {
        let plan = allowed_plan();
        let mut buy_line = execution_line(&plan, Action::Buy, Some(0.0005), &disabled_options());
        buy_line.send_signature = Some("copy-buy-signature".to_string());
        buy_line.copy_wallet_token_account = Some("copy-token-account".to_string());
        let trailing_sell_plan = TrailingSellPlan {
            mode: TrailingSellMode::CustomSteps,
            percent_basis: TrailingSellPercentBasis::RemainingBalance,
            steps: vec![TrailingSellStep {
                delay_ms: 500,
                percent: 50.0,
            }],
            sell_slippage_percent: Some(8.0),
            sell_priority_fee_sol: Some(0.00002),
            priority_fee_micro_lamports: Some(250_000),
            jito_tip_lamports: Some(10_000),
            jito_tip_account: Some(COPY_WALLET.to_string()),
        };
        let mut line = RustTrailingSellLine::new(
            &buy_line,
            &trailing_sell_plan,
            0,
            1,
            trailing_sell_plan.steps[0],
            1_000,
            &disabled_options(),
        );
        line.sent = true;
        line.decision = "sent";
        line.send_signature = Some("trailing-sell-signature".to_string());
        line.submitted_at_ms = Some(20);
        line.signature_returned_at_ms = Some(21);

        let confirmation = SignatureConfirmation {
            checked: true,
            status: "submitted_not_landed",
            ok: false,
            slot: None,
            block_position: None,
            block_position_error: None,
            confirmation_status: None,
            err: None,
            reason: Some(
                "rust trailing sell transaction not found before confirmation timeout".to_string(),
            ),
        };
        let confirmation_line =
            TransactionConfirmationLine::from_rust_trailing_sell(&line, confirmation);
        let json = serde_json::to_value(&confirmation_line).expect("confirmation line serializes");

        assert_eq!(
            json.get("schema").and_then(serde_json::Value::as_str),
            Some("copytrade.transactionConfirmation.v1")
        );
        assert_eq!(
            json.get("transactionRole")
                .and_then(serde_json::Value::as_str),
            Some("rust_trailing_sell")
        );
        assert_eq!(
            json.get("signature").and_then(serde_json::Value::as_str),
            Some("trailing-sell-signature")
        );
        assert_eq!(
            json.get("buySendSignature")
                .and_then(serde_json::Value::as_str),
            Some("copy-buy-signature")
        );
        assert_eq!(
            json.get("status").and_then(serde_json::Value::as_str),
            Some("submitted_not_landed")
        );
        assert_eq!(
            json.get("stepIndex").and_then(serde_json::Value::as_u64),
            Some(0)
        );
        assert_eq!(
            json.get("totalSteps").and_then(serde_json::Value::as_u64),
            Some(1)
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
    fn helius_sender_urls_join_fanout_when_enabled() {
        let mut options = disabled_options();
        options.send_fanout = true;
        options.fast_copy_send = true;
        options.solana_rpc_url = Some("https://mainnet.helius-rpc.com/?api-key=secret".to_string());
        options.priority_fee_micro_lamports = Some(500_000);
        options.helius_sender_enabled = true;
        options.helius_sender_urls = vec![
            "https://sender.helius-rpc.com".to_string(),
            "https://sender2.helius-rpc.com/fast".to_string(),
        ];
        options.helius_sender_tip_lamports = Some(HELIUS_SENDER_MIN_TIP_LAMPORTS);
        options.helius_sender_tip_account = Some(COPY_WALLET.to_string());

        options
            .validate_helius_sender()
            .expect("valid sender config");
        let endpoints = options.selected_send_endpoints();

        assert_eq!(endpoints.len(), 3);
        assert_eq!(endpoints[0].kind, SendEndpointKind::Rpc);
        assert_eq!(endpoints[1].kind, SendEndpointKind::HeliusSender);
        assert_eq!(
            endpoints[1].label,
            "helius-sender-1-fast:sender.helius-rpc.com"
        );
        assert_eq!(endpoints[1].url, "https://sender.helius-rpc.com/fast");
        assert_eq!(endpoints[1].sender_mode, Some("fast"));
        assert_eq!(endpoints[2].url, "https://sender2.helius-rpc.com/fast");
    }

    #[test]
    fn helius_sender_swqos_urls_are_labeled_and_query_tagged() {
        let mut options = disabled_options();
        options.send_fanout = true;
        options.fast_copy_send = true;
        options.priority_fee_micro_lamports = Some(500_000);
        options.helius_sender_enabled = true;
        options.helius_sender_swqos_only = true;
        options.helius_sender_urls =
            vec!["https://sender.helius-rpc.com/?api-key=secret".to_string()];
        options.helius_sender_tip_lamports = Some(HELIUS_SENDER_SWQOS_ONLY_MIN_TIP_LAMPORTS);
        options.helius_sender_tip_account = Some(COPY_WALLET.to_string());

        options
            .validate_helius_sender()
            .expect("valid sender swqos config");
        let endpoints = options.selected_send_endpoints();

        assert_eq!(endpoints.len(), 1);
        assert_eq!(
            endpoints[0].label,
            "helius-sender-1-swqos:sender.helius-rpc.com"
        );
        assert_eq!(
            endpoints[0].url,
            "https://sender.helius-rpc.com/fast?api-key=secret&swqos_only=true"
        );
        assert_eq!(endpoints[0].sender_mode, Some("swqos"));
    }

    #[test]
    fn send_lane_mode_filters_fee_tips_without_changing_priority_fee() {
        let mut options = configured_multi_lane_options();

        options.send_lane_mode = SendLaneMode::Mixed;
        let fee_config = options.tx_fee_config();
        assert_eq!(fee_config.compute_unit_price_micro_lamports, Some(500_000));
        assert_eq!(fee_config.jito_tip_lamports, Some(10_000));
        assert_eq!(
            fee_config.helius_sender_tip_lamports,
            Some(HELIUS_SENDER_MIN_TIP_LAMPORTS)
        );

        options.send_lane_mode = SendLaneMode::RpcOnly;
        let fee_config = options.tx_fee_config();
        assert_eq!(fee_config.compute_unit_price_micro_lamports, Some(500_000));
        assert_eq!(fee_config.jito_tip_lamports, None);
        assert_eq!(fee_config.jito_tip_account, None);
        assert_eq!(fee_config.helius_sender_tip_lamports, None);
        assert_eq!(fee_config.helius_sender_tip_account, None);

        options.send_lane_mode = SendLaneMode::JitoOnly;
        let fee_config = options.tx_fee_config();
        assert_eq!(fee_config.compute_unit_price_micro_lamports, Some(500_000));
        assert_eq!(fee_config.jito_tip_lamports, Some(10_000));
        assert_eq!(fee_config.jito_tip_account.as_deref(), Some(COPY_WALLET));
        assert_eq!(fee_config.helius_sender_tip_lamports, None);
        assert_eq!(fee_config.helius_sender_tip_account, None);

        options.send_lane_mode = SendLaneMode::HeliusSenderOnly;
        let fee_config = options.tx_fee_config();
        assert_eq!(fee_config.compute_unit_price_micro_lamports, Some(500_000));
        assert_eq!(fee_config.jito_tip_lamports, None);
        assert_eq!(fee_config.jito_tip_account, None);
        assert_eq!(
            fee_config.helius_sender_tip_lamports,
            Some(HELIUS_SENDER_MIN_TIP_LAMPORTS)
        );
        assert_eq!(
            fee_config.helius_sender_tip_account.as_deref(),
            Some(COPY_WALLET)
        );
    }

    #[test]
    fn send_lane_mode_filters_endpoint_families() {
        let mut options = configured_multi_lane_options();

        options.send_lane_mode = SendLaneMode::Mixed;
        let endpoints = options.selected_send_endpoints();
        assert_eq!(
            endpoint_kinds(&endpoints),
            vec!["rpc", "jito", "helius_sender"]
        );

        options.send_lane_mode = SendLaneMode::RpcOnly;
        let endpoints = options.selected_send_endpoints();
        assert_eq!(endpoint_kinds(&endpoints), vec!["rpc"]);
        assert_eq!(endpoints[0].label, "rpc-primary:primary.example.com");

        options.send_lane_mode = SendLaneMode::JitoOnly;
        let endpoints = options.selected_send_endpoints();
        assert_eq!(endpoint_kinds(&endpoints), vec!["jito"]);
        assert_eq!(
            endpoints[0].url,
            "https://frankfurt.mainnet.block-engine.jito.wtf/api/v1/transactions"
        );

        options.send_lane_mode = SendLaneMode::HeliusSenderOnly;
        let endpoints = options.selected_send_endpoints();
        assert_eq!(endpoint_kinds(&endpoints), vec!["helius_sender"]);
        assert_eq!(endpoints[0].url, "https://sender.helius-rpc.com/fast");
    }

    #[test]
    fn send_lane_mode_validation_fails_closed_for_missing_mode_config() {
        let mut options = disabled_options();
        options.send_lane_mode = SendLaneMode::RpcOnly;
        assert_eq!(
            options.validate_send_lane_mode().unwrap_err(),
            "JITO_SEND_LANE_MODE=rpc_only requires SOLANA_RPC_URL or JITO_SEND_RPC_URLS"
        );

        options.solana_rpc_url = Some("https://primary.example.com".to_string());
        options.send_lane_mode = SendLaneMode::JitoOnly;
        assert_eq!(
            options.validate_send_lane_mode().unwrap_err(),
            "JITO_SEND_LANE_MODE=jito_only requires JITO_SEND_FANOUT=YES"
        );

        options.send_fanout = true;
        assert_eq!(
            options.validate_send_lane_mode().unwrap_err(),
            "JITO_SEND_LANE_MODE=jito_only requires JITO_BLOCK_ENGINE_SEND_URLS"
        );

        options.send_lane_mode = SendLaneMode::HeliusSenderOnly;
        assert_eq!(
            options.validate_send_lane_mode().unwrap_err(),
            "JITO_SEND_LANE_MODE=helius_sender_only requires JITO_HELIUS_SENDER_ENABLED=YES"
        );

        options.send_lane_mode = SendLaneMode::RpcOnly;
        options.jito_tip_lamports = Some(10_000);
        options.jito_tip_account = None;
        assert!(options.validate_send_lane_mode().is_ok());
    }

    #[test]
    fn helius_sender_validation_fails_closed() {
        let mut options = disabled_options();
        options.helius_sender_enabled = true;
        assert_eq!(
            options.validate_helius_sender().unwrap_err(),
            "JITO_HELIUS_SENDER_ENABLED requires JITO_SEND_FANOUT=YES"
        );

        options.send_fanout = true;
        assert_eq!(
            options.validate_helius_sender().unwrap_err(),
            "JITO_HELIUS_SENDER_ENABLED requires JITO_FAST_COPY_SEND=YES"
        );

        options.fast_copy_send = true;
        assert_eq!(
            options.validate_helius_sender().unwrap_err(),
            "JITO_HELIUS_SENDER_ENABLED requires JITO_HELIUS_SENDER_URLS"
        );

        options.helius_sender_urls = vec!["https://sender.helius-rpc.com".to_string()];
        assert_eq!(
            options.validate_helius_sender().unwrap_err(),
            "JITO_HELIUS_SENDER_ENABLED requires JITO_PRIORITY_FEE_MICRO_LAMPORTS"
        );

        options.priority_fee_micro_lamports = Some(500_000);
        assert_eq!(
            options.validate_helius_sender().unwrap_err(),
            "JITO_HELIUS_SENDER_ENABLED requires JITO_HELIUS_SENDER_TIP_LAMPORTS"
        );

        options.helius_sender_tip_lamports = Some(1);
        assert_eq!(
            options.validate_helius_sender().unwrap_err(),
            format!(
                "JITO_HELIUS_SENDER_TIP_LAMPORTS must be >= {} lamports",
                HELIUS_SENDER_MIN_TIP_LAMPORTS
            )
        );

        options.helius_sender_tip_lamports = Some(HELIUS_SENDER_MIN_TIP_LAMPORTS);
        assert_eq!(
            options.validate_helius_sender().unwrap_err(),
            "JITO_HELIUS_SENDER_ENABLED requires JITO_HELIUS_SENDER_TIP_ACCOUNT"
        );

        options.helius_sender_tip_account = Some("not-a-pubkey".to_string());
        assert_eq!(
            options.validate_helius_sender().unwrap_err(),
            "JITO_HELIUS_SENDER_TIP_ACCOUNT must be a valid pubkey"
        );
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
            sender_mode: None,
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
    async fn max_copy_sol_zero_allows_large_planned_amount_before_keypair() {
        let mut options = disabled_options();
        options.simulate_copy_tx = true;
        options.max_copy_sol = Some(0.0);
        options.copy_wallet = Some(COPY_WALLET.to_string());
        let mut plan = allowed_plan();
        plan.spend_sol_amount = Some(1.5);

        let line = executor(options)
            .handle(&plan, Action::Buy, Some(1.5), sample_timings())
            .await;

        assert_eq!(line.decision, "skip");
        assert_eq!(line.reason.as_deref(), Some("missing copy keypair path"));
        assert!(!line.signed);
        assert!(!line.sent);
    }

    #[tokio::test]
    async fn migrated_amm_small_copy_skips_before_keypair() {
        let mut options = disabled_options();
        options.simulate_copy_tx = true;
        options.max_copy_sol = Some(0.001);
        options.copy_wallet = Some(COPY_WALLET.to_string());
        let mut plan = allowed_plan();
        plan.spend_sol_amount = Some(0.0001);
        plan.route_context = Some(flashx_context(FlashxPumpLayout::MigratedAmm, 1));
        let min_lamports = options.migrated_amm_min_copy_lamports().unwrap();

        let line = executor(options)
            .handle(&plan, Action::Buy, Some(0.00099), sample_timings())
            .await;
        let expected_reason = format!(
            "migrated AMM copy spend 100000 lamports below min {} lamports",
            min_lamports
        );

        assert_eq!(line.decision, "skip");
        assert_eq!(line.reason.as_deref(), Some(expected_reason.as_str()));
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
        let mut line = execution_line(&plan, Action::Buy, Some(0.0005), &disabled_options());

        line.sent = true;
        assert!(!line.was_sent());

        line.decision = "sent";
        assert!(line.was_sent());
    }

    #[test]
    fn send_lane_attribution_line_serializes_ack_attempts_without_causality_claim() {
        let line = sample_send_lane_attribution_line();

        let json = serde_json::to_value(&line).expect("attribution line serializes");

        assert_eq!(
            json.get("schema").and_then(serde_json::Value::as_str),
            Some("copytrade.sendLaneAttribution.v1")
        );
        assert_eq!(
            json.get("submissionGroupId")
                .and_then(serde_json::Value::as_str),
            Some("signed-tx-signature")
        );
        assert_eq!(
            json.get("sendSignature")
                .and_then(serde_json::Value::as_str),
            Some("signed-tx-signature")
        );
        assert_eq!(
            json.get("firstAckLane").and_then(serde_json::Value::as_str),
            Some("rpc-primary:example.com")
        );
        assert_eq!(
            json.get("sendLaneMode").and_then(serde_json::Value::as_str),
            Some("mixed")
        );
        assert!(json.get("landingLane").is_none());
        assert_eq!(
            json.pointer("/allAttempts/0/ackAt")
                .and_then(serde_json::Value::as_u64),
            Some(112)
        );
    }

    #[test]
    fn copy_execution_output_write_json_line_flushes_by_default() {
        let path = temp_path("jito-copy-execution-flush-default.jsonl");
        let file = std::fs::File::create(&path).expect("temp file creates");
        let mut writer = std::io::BufWriter::new(file);
        let line = CopyExecutionOutput::SendLaneAttribution(sample_send_lane_attribution_line());

        line.write_json_line(Some(&mut writer), true)
            .expect("line writes");

        let contents = std::fs::read_to_string(&path).expect("flushed temp file reads");
        std::fs::remove_file(&path).ok();
        assert!(contents.contains("\"schema\":\"copytrade.sendLaneAttribution.v1\""));
    }

    #[test]
    fn copy_execution_output_write_json_line_can_defer_flush() {
        let path = temp_path("jito-copy-execution-flush-deferred.jsonl");
        let file = std::fs::File::create(&path).expect("temp file creates");
        let mut writer = std::io::BufWriter::new(file);
        let line = CopyExecutionOutput::SendLaneAttribution(sample_send_lane_attribution_line());

        line.write_json_line(Some(&mut writer), false)
            .expect("line writes");

        let contents_before_drop =
            std::fs::read_to_string(&path).expect("unflushed temp file reads");
        assert_eq!(contents_before_drop, "");
        drop(writer);
        let contents_after_drop = std::fs::read_to_string(&path).expect("dropped writer flushes");
        std::fs::remove_file(&path).ok();
        assert!(contents_after_drop.contains("\"schema\":\"copytrade.sendLaneAttribution.v1\""));
    }

    fn sample_send_lane_attribution_line() -> SendLaneAttributionLine {
        let plan = allowed_plan();
        SendLaneAttributionLine {
            schema: "copytrade.sendLaneAttribution.v1",
            observed_at_ms: 100,
            attribution_at_ms: 125,
            provider: "shredstream",
            source: "jito-proxy",
            endpoint: "http://127.0.0.1:9999".to_string(),
            observed_wallet: plan.target_wallet,
            copy_wallet: Some(COPY_WALLET.to_string()),
            mint: plan.mint,
            transaction_role: "copy_buy",
            submission_group_id: "signed-tx-signature".to_string(),
            observed_signature: "observed-signature".to_string(),
            send_signature: "signed-tx-signature".to_string(),
            send_lane_mode: "mixed",
            first_ack_lane: "rpc-primary:example.com".to_string(),
            first_ack_at_ms: 112,
            all_attempts: vec![SendLaneAttemptAttribution {
                label: "rpc-primary:example.com".to_string(),
                kind: "rpc",
                mode: None,
                status: "submitted",
                duration_ms: 7,
                ack_at: Some(112),
                error: None,
            }],
        }
    }

    fn configured_multi_lane_options() -> CopyExecutionOptions {
        let mut options = disabled_options();
        options.send_fanout = true;
        options.fast_copy_send = true;
        options.solana_rpc_url = Some("https://primary.example.com".to_string());
        options.jito_send_urls =
            vec!["https://frankfurt.mainnet.block-engine.jito.wtf".to_string()];
        options.jito_tip_lamports = Some(10_000);
        options.jito_tip_account = Some(COPY_WALLET.to_string());
        options.helius_sender_enabled = true;
        options.helius_sender_urls = vec!["https://sender.helius-rpc.com".to_string()];
        options.helius_sender_tip_lamports = Some(HELIUS_SENDER_MIN_TIP_LAMPORTS);
        options.helius_sender_tip_account = Some(COPY_WALLET.to_string());
        options.priority_fee_micro_lamports = Some(500_000);
        options
    }

    fn endpoint_kinds(endpoints: &[SendEndpoint]) -> Vec<&'static str> {
        endpoints.iter().map(send_endpoint_kind).collect()
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "{}-{name}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time moves forward")
                .as_nanos()
        ))
    }
}
