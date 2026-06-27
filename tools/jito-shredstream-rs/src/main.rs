use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod address_lookup;
mod balance_cache;
mod blockhash;
mod event;
mod executor;
mod live;
mod parser;
mod planner;
mod priority_fee_cache;
mod proto;
mod signal;
mod telegram_snapshot;
mod tx_builder;

#[derive(Debug, Parser)]
#[command(
    name = "jito-feed-probe",
    about = "Watch Jito ShredStream entries for tracked-wallet Pump buys"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Connect to the local Jito ShredStream proxy and scan deshredded entries.
    Live(LiveOptions),
}

#[derive(Debug, Parser)]
pub(crate) struct LiveOptions {
    #[arg(
        long,
        env = "JITO_SHREDSTREAM_PROXY_URL",
        default_value = "http://127.0.0.1:9999"
    )]
    pub(crate) endpoint: String,

    #[arg(
        long = "target-wallet",
        env = "SHREDSTREAM_TARGET_WALLETS",
        value_delimiter = ','
    )]
    pub(crate) target_wallets: Vec<String>,

    #[arg(long, default_value_t = 0)]
    pub(crate) limit: usize,

    #[arg(long, default_value_t = false)]
    pub(crate) include_rejections: bool,

    #[arg(long, default_value_t = false)]
    pub(crate) stats: bool,

    #[arg(
        long,
        env = "JITO_PRINT_MENTIONS",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) print_mentions: bool,

    #[arg(
        long,
        env = "JITO_PRINT_FEED_EVENTS",
        default_value_t = true,
        value_parser = parse_boolish
    )]
    pub(crate) print_feed_events: bool,

    #[arg(
        long,
        env = "JITO_FEED_PROBE_DEDUPE_CAPACITY",
        default_value_t = 50_000
    )]
    pub(crate) dedupe_capacity: usize,

    #[arg(long, env = "JITO_SHADOW_SIGNALS_PATH")]
    pub(crate) shadow_signals_path: Option<PathBuf>,

    #[arg(long, env = "JITO_EXECUTION_PLANS_PATH")]
    pub(crate) execution_plans_path: Option<PathBuf>,

    #[arg(long, env = "JITO_COPY_PLAN_SOL_AMOUNT")]
    pub(crate) copy_plan_sol_amount: Option<f64>,

    #[arg(long, env = "JITO_TELEGRAM_SNAPSHOT_PATH")]
    pub(crate) telegram_snapshot_path: Option<PathBuf>,

    #[arg(
        long,
        env = "JITO_TELEGRAM_SNAPSHOT_RELOAD_MS",
        default_value_t = 1_000
    )]
    pub(crate) telegram_snapshot_reload_ms: u64,

    #[arg(long, env = "JITO_TX_BUILD_PLANS_PATH")]
    pub(crate) tx_build_plans_path: Option<PathBuf>,

    #[arg(long, env = "JITO_TX_BUILD_PLAN_MAX_AGE_MS", default_value_t = 2_000)]
    pub(crate) tx_build_plan_max_age_ms: u128,

    #[arg(long, env = "JITO_COPY_TX_PLANS_PATH")]
    pub(crate) copy_tx_plans_path: Option<PathBuf>,

    #[arg(long, env = "JITO_UNSIGNED_TX_PLANS_PATH")]
    pub(crate) unsigned_tx_plans_path: Option<PathBuf>,

    #[arg(long, env = "JITO_COPY_WALLET")]
    pub(crate) copy_wallet: Option<String>,

    #[arg(
        long,
        env = "JITO_COPY_WALLET_BALANCE_GUARD",
        default_value_t = true,
        value_parser = parse_boolish
    )]
    pub(crate) copy_wallet_balance_guard: bool,

    #[arg(
        long,
        env = "JITO_COPY_WALLET_BALANCE_REFRESH_MS",
        default_value_t = 1_000
    )]
    pub(crate) copy_wallet_balance_refresh_ms: u64,

    #[arg(
        long,
        env = "JITO_COPY_WALLET_BALANCE_STALE_MS",
        default_value_t = 120_000
    )]
    pub(crate) copy_wallet_balance_stale_ms: u128,

    #[arg(long, env = "JITO_BLOCKHASH_REFRESH_MS", default_value_t = 500)]
    pub(crate) blockhash_refresh_ms: u64,

    #[arg(
        long,
        env = "JITO_BLOCKHASH_REFRESH_TIMEOUT_MS",
        default_value_t = 1_200
    )]
    pub(crate) blockhash_refresh_timeout_ms: u64,

    #[arg(long, env = "JITO_BLOCKHASH_COMMITMENT", default_value = "processed")]
    pub(crate) blockhash_commitment: String,

    #[arg(long, env = "JITO_BLOCKHASH_STALE_MS", default_value_t = 30_000)]
    pub(crate) blockhash_stale_ms: u128,

    #[arg(
        long,
        env = "JITO_ACCOUNT_PRIORITY_FEE_ENABLED",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) account_priority_fee_enabled: bool,

    #[arg(
        long,
        env = "JITO_ACCOUNT_PRIORITY_FEE_REFRESH_MS",
        default_value_t = 1_000
    )]
    pub(crate) account_priority_fee_refresh_ms: u64,

    #[arg(
        long,
        env = "JITO_ACCOUNT_PRIORITY_FEE_STALE_MS",
        default_value_t = 5_000
    )]
    pub(crate) account_priority_fee_stale_ms: u128,

    #[arg(
        long,
        env = "JITO_ACCOUNT_PRIORITY_FEE_PERCENTILE",
        default_value_t = 75
    )]
    pub(crate) account_priority_fee_percentile: u8,

    #[arg(long, env = "JITO_SIMULATE_COPY_TX", default_value_t = false)]
    pub(crate) simulate_copy_tx: bool,

    #[arg(long, env = "JITO_ENABLE_COPY_SEND", default_value_t = false)]
    pub(crate) enable_copy_send: bool,

    #[arg(
        long,
        env = "JITO_FAST_COPY_SEND",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) fast_copy_send: bool,

    #[arg(
        long,
        env = "JITO_SEND_FANOUT",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) send_fanout: bool,

    #[arg(long, env = "JITO_SEND_LANE_MODE", default_value = "mixed", value_enum)]
    pub(crate) send_lane_mode: executor::SendLaneMode,

    #[arg(
        long = "send-rpc-url",
        env = "JITO_SEND_RPC_URLS",
        value_delimiter = ','
    )]
    pub(crate) send_rpc_urls: Vec<String>,

    #[arg(
        long = "sell-send-rpc-url",
        env = "JITO_SELL_SEND_RPC_URLS",
        value_delimiter = ','
    )]
    pub(crate) sell_send_rpc_urls: Vec<String>,

    #[arg(
        long = "jito-send-url",
        env = "JITO_BLOCK_ENGINE_SEND_URLS",
        value_delimiter = ','
    )]
    pub(crate) jito_send_urls: Vec<String>,

    #[arg(long, env = "JITO_BLOCK_ENGINE_AUTH_UUID", hide_env_values = true)]
    pub(crate) jito_auth_uuid: Option<String>,

    #[arg(
        long,
        env = "JITO_HELIUS_SENDER_ENABLED",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) helius_sender_enabled: bool,

    #[arg(
        long = "helius-sender-url",
        env = "JITO_HELIUS_SENDER_URLS",
        value_delimiter = ','
    )]
    pub(crate) helius_sender_urls: Vec<String>,

    #[arg(
        long,
        env = "JITO_HELIUS_SENDER_SWQOS_ONLY",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) helius_sender_swqos_only: bool,

    #[arg(long, env = "JITO_HELIUS_SENDER_TIP_LAMPORTS")]
    pub(crate) helius_sender_tip_lamports: Option<u64>,

    #[arg(long, env = "JITO_HELIUS_SENDER_TIP_ACCOUNT")]
    pub(crate) helius_sender_tip_account: Option<String>,

    #[arg(long, env = "JITO_HELIUS_SENDER_TIP_ACCOUNTS", value_delimiter = ',')]
    pub(crate) helius_sender_tip_accounts: Vec<String>,

    #[arg(
        long,
        env = "JITO_NOZOMI_ENABLED",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) nozomi_enabled: bool,

    #[arg(long = "nozomi-url", env = "JITO_NOZOMI_URLS", value_delimiter = ',')]
    pub(crate) nozomi_urls: Vec<String>,

    #[arg(long, env = "JITO_NOZOMI_TIP_LAMPORTS")]
    pub(crate) nozomi_tip_lamports: Option<u64>,

    #[arg(long, env = "JITO_NOZOMI_TIP_ACCOUNT")]
    pub(crate) nozomi_tip_account: Option<String>,

    #[arg(long, env = "JITO_NOZOMI_TIP_ACCOUNTS", value_delimiter = ',')]
    pub(crate) nozomi_tip_accounts: Vec<String>,

    #[arg(
        long,
        env = "JITO_ASTRALANE_ENABLED",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) astralane_enabled: bool,

    #[arg(
        long = "astralane-url",
        env = "JITO_ASTRALANE_URLS",
        value_delimiter = ','
    )]
    pub(crate) astralane_urls: Vec<String>,

    #[arg(long, env = "JITO_ASTRALANE_API_KEY", hide_env_values = true)]
    pub(crate) astralane_api_key: Option<String>,

    #[arg(long, env = "JITO_ASTRALANE_TIP_LAMPORTS")]
    pub(crate) astralane_tip_lamports: Option<u64>,

    #[arg(long, env = "JITO_ASTRALANE_TIP_ACCOUNT")]
    pub(crate) astralane_tip_account: Option<String>,

    #[arg(long, env = "JITO_ASTRALANE_TIP_ACCOUNTS", value_delimiter = ',')]
    pub(crate) astralane_tip_accounts: Vec<String>,

    #[arg(
        long,
        env = "JITO_ASTRALANE_MEV_PROTECT",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) astralane_mev_protect: bool,

    #[arg(
        long,
        env = "JITO_ASTRALANE_SWQOS_ONLY",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) astralane_swqos_only: bool,

    #[arg(
        long,
        env = "JITO_BEAM_ENABLED",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) beam_enabled: bool,

    #[arg(
        long,
        env = "JITO_BEAM_URL",
        default_value = "https://beam.rpcfast.com"
    )]
    pub(crate) beam_url: Option<String>,

    #[arg(long, env = "JITO_BEAM_TOKEN", hide_env_values = true)]
    pub(crate) beam_token: Option<String>,

    #[arg(long, env = "JITO_BEAM_PROVIDER")]
    pub(crate) beam_provider: Option<String>,

    #[arg(long, env = "JITO_BEAM_MODE", default_value = "fastest")]
    pub(crate) beam_mode: Option<String>,

    #[arg(long, env = "JITO_BEAM_TIP_LAMPORTS")]
    pub(crate) beam_tip_lamports: Option<u64>,

    #[arg(long, env = "JITO_BEAM_TIP_ACCOUNTS", value_delimiter = ',')]
    pub(crate) beam_tip_accounts: Vec<String>,

    #[arg(
        long,
        env = "JITO_TPU_JET_ENABLED",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) tpu_jet_enabled: bool,

    #[arg(long, env = "JITO_TPU_JET_RPC_URL")]
    pub(crate) tpu_jet_rpc_url: Option<String>,

    #[arg(long, env = "JITO_TPU_JET_WS_URL")]
    pub(crate) tpu_jet_ws_url: Option<String>,

    #[arg(
        long,
        env = "JITO_TPU_JET_SIDECAR_URL",
        default_value = "http://127.0.0.1:8787"
    )]
    pub(crate) tpu_jet_sidecar_url: Option<String>,

    #[arg(long, env = "JITO_TPU_JET_FANOUT_SLOTS", default_value_t = 12)]
    pub(crate) tpu_jet_fanout_slots: u64,

    #[arg(long, env = "JITO_TPU_JET_TIMEOUT_MS", default_value_t = 30)]
    pub(crate) tpu_jet_timeout_ms: u64,

    #[arg(
        long,
        env = "JITO_TPU_QUIC_ENABLED",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) tpu_quic_enabled: bool,

    #[arg(long, env = "JITO_TPU_QUIC_RPC_URL")]
    pub(crate) tpu_quic_rpc_url: Option<String>,

    #[arg(long, env = "JITO_TPU_QUIC_WS_URL")]
    pub(crate) tpu_quic_ws_url: Option<String>,

    #[arg(long, env = "JITO_TPU_QUIC_FANOUT_SLOTS", default_value_t = 12)]
    pub(crate) tpu_quic_fanout_slots: u64,

    #[arg(long, env = "JITO_TPU_QUIC_TIMEOUT_MS", default_value_t = 30)]
    pub(crate) tpu_quic_timeout_ms: u64,

    #[arg(long, env = "JITO_SELL_HELIUS_SENDER_TIP_LAMPORTS")]
    pub(crate) sell_helius_sender_tip_lamports: Option<u64>,

    #[arg(long, env = "JITO_SELL_HELIUS_SENDER_TIP_ACCOUNT")]
    pub(crate) sell_helius_sender_tip_account: Option<String>,

    #[arg(long, env = "JITO_ONE_SHOT_COPY_SEND", default_value_t = false)]
    pub(crate) one_shot_copy_send: bool,

    #[arg(long, env = "JITO_DRY_RUN", default_value_t = true)]
    pub(crate) dry_run: bool,

    #[arg(long, env = "JITO_MAX_COPY_SOL")]
    pub(crate) max_copy_sol: Option<f64>,

    #[arg(long, env = "JITO_MAX_TOTAL_COPY_SPEND_SOL")]
    pub(crate) max_total_copy_spend_sol: Option<f64>,

    #[arg(long, env = "JITO_MAX_PROVIDER_TIP_LAMPORTS")]
    pub(crate) max_provider_tip_lamports: Option<u64>,

    #[arg(long, env = "JITO_MAX_SIGNED_TX_BYTES")]
    pub(crate) max_signed_tx_bytes: Option<usize>,

    #[arg(long, env = "JITO_MAX_INSTRUCTION_COUNT")]
    pub(crate) max_instruction_count: Option<usize>,

    #[arg(long, env = "JITO_MAX_WRITABLE_ACCOUNT_COUNT")]
    pub(crate) max_writable_account_count: Option<usize>,

    #[arg(
        long,
        env = "JITO_MIGRATED_AMM_MIN_COPY_SOL",
        default_value_t = executor::DEFAULT_MIGRATED_AMM_MIN_COPY_SOL
    )]
    pub(crate) migrated_amm_min_copy_sol: f64,

    #[arg(
        long,
        env = "JITO_MIGRATED_AMM_SMALL_COPY_MODE",
        default_value = "skip",
        value_enum
    )]
    pub(crate) migrated_amm_small_copy_mode: executor::MigratedAmmSmallCopyMode,

    #[arg(long, env = "JITO_COPY_KEYPAIR_PATH")]
    pub(crate) copy_keypair_path: Option<PathBuf>,

    #[arg(long, env = "JITO_COPY_EXECUTIONS_PATH")]
    pub(crate) copy_executions_path: Option<PathBuf>,

    #[arg(
        long,
        env = "JITO_COPY_EXECUTIONS_FLUSH_EACH_WRITE",
        default_value_t = true,
        value_parser = parse_boolish
    )]
    pub(crate) copy_executions_flush_each_write: bool,

    #[arg(
        long,
        env = "JITO_COPY_EXECUTIONS_WRITE_QUEUE_CAPACITY",
        default_value_t = 1024
    )]
    pub(crate) copy_executions_write_queue_capacity: usize,

    #[arg(
        long,
        env = "JITO_COPY_EXECUTIONS_FLUSH_INTERVAL_MS",
        default_value_t = 250
    )]
    pub(crate) copy_executions_flush_interval_ms: u64,

    #[arg(long, env = "JITO_COPY_EXECUTION_CONCURRENCY", default_value_t = 4)]
    pub(crate) copy_execution_concurrency: usize,

    #[arg(
        long,
        env = "JITO_COPY_EXECUTION_QUEUE_CAPACITY",
        default_value_t = 1024
    )]
    pub(crate) copy_execution_queue_capacity: usize,

    #[arg(long, env = "JITO_AUTO_SELL_AFTER_BUY", default_value_t = false)]
    pub(crate) auto_sell_after_buy: bool,

    #[arg(long, env = "JITO_AUTO_SELL_DELAY_MS", default_value_t = 1_000)]
    pub(crate) auto_sell_delay_ms: u64,

    #[arg(
        long,
        env = "JITO_RUST_TRAILING_SELLS_ENABLED",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) rust_trailing_sells_enabled: bool,

    #[arg(
        long,
        env = "JITO_DIRECT_PUMP_CASHBACK_GUARD_FAIL_OPEN",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) direct_pump_cashback_guard_fail_open: bool,

    #[arg(
        long,
        env = "JITO_RUST_TRAILING_SELL_CONFIRMATION_TIMEOUT_MS",
        default_value_t = 30_000
    )]
    pub(crate) rust_trailing_sell_confirmation_timeout_ms: u64,

    #[arg(
        long,
        env = "JITO_RUST_TRAILING_SELL_CONFIRMATION_POLL_MS",
        default_value_t = 100
    )]
    pub(crate) rust_trailing_sell_confirmation_poll_ms: u64,

    #[arg(
        long,
        env = "JITO_SIMULATE_AUTO_SELL",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) simulate_auto_sell: bool,

    #[arg(
        long,
        env = "JITO_ISOLATE_BUY_LATENCY_TEST",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) isolate_buy_latency_test: bool,

    #[arg(long, env = "JITO_SEND_MAX_RETRIES", default_value_t = 3)]
    pub(crate) send_max_retries: u64,

    #[arg(long, env = "JITO_SEND_HTTP_TIMEOUT_MS", default_value_t = 0)]
    pub(crate) send_http_timeout_ms: u64,

    #[arg(long, env = "JITO_PRIORITY_FEE_MICRO_LAMPORTS")]
    pub(crate) priority_fee_micro_lamports: Option<u64>,

    #[arg(
        long,
        env = "JITO_DYNAMIC_PRIORITY_FEE_ENABLED",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) dynamic_priority_fee_enabled: bool,

    #[arg(long, env = "JITO_DYNAMIC_PRIORITY_FEE_BASELINE_MICRO_LAMPORTS")]
    pub(crate) dynamic_priority_fee_baseline_micro_lamports: Option<u64>,

    #[arg(long, env = "JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS")]
    pub(crate) dynamic_priority_fee_aggressive_micro_lamports: Option<u64>,

    #[arg(long, env = "JITO_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS")]
    pub(crate) dynamic_priority_fee_panic_micro_lamports: Option<u64>,

    #[arg(long, env = "JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS")]
    pub(crate) dynamic_priority_fee_max_micro_lamports: Option<u64>,

    #[arg(long, env = "JITO_TIP_LAMPORTS")]
    pub(crate) jito_tip_lamports: Option<u64>,

    #[arg(long, env = "JITO_TIP_ACCOUNT")]
    pub(crate) jito_tip_account: Option<String>,

    #[arg(long, env = "JITO_TIP_ACCOUNTS", value_delimiter = ',')]
    pub(crate) jito_tip_accounts: Vec<String>,

    #[arg(long, env = "JITO_SELL_PRIORITY_FEE_MICRO_LAMPORTS")]
    pub(crate) sell_priority_fee_micro_lamports: Option<u64>,

    #[arg(long, env = "JITO_SELL_TIP_LAMPORTS")]
    pub(crate) sell_jito_tip_lamports: Option<u64>,

    #[arg(long, env = "JITO_SELL_TIP_ACCOUNT")]
    pub(crate) sell_jito_tip_account: Option<String>,

    #[arg(
        long,
        env = "JITO_WARM_SEND_ENDPOINTS",
        default_value_t = true,
        value_parser = parse_boolish
    )]
    pub(crate) warm_send_endpoints: bool,

    #[arg(
        long,
        env = "JITO_SEND_ENDPOINT_WARM_INTERVAL_MS",
        default_value_t = 15_000
    )]
    pub(crate) send_endpoint_warm_interval_ms: u64,

    #[arg(long, env = "SUPABASE_URL")]
    pub(crate) supabase_url: Option<String>,

    #[arg(long, env = "SUPABASE_SERVICE_ROLE_KEY", hide_env_values = true)]
    pub(crate) supabase_service_role_key: Option<String>,

    #[arg(
        long,
        env = "JITO_DISABLE_SIGNAL_OBSERVATIONS",
        default_value_t = false,
        value_parser = parse_boolish
    )]
    pub(crate) disable_signal_observations: bool,

    #[arg(
        long,
        env = "JITO_SIGNAL_TABLE",
        default_value = "copytrade_signal_observations"
    )]
    pub(crate) signal_table: String,

    #[arg(
        long,
        env = "JITO_SIGNAL_OBSERVATION_QUEUE_CAPACITY",
        default_value_t = 4096
    )]
    pub(crate) signal_observation_queue_capacity: usize,

    #[arg(long, env = "SOLANA_RPC_URL")]
    pub(crate) solana_rpc_url: Option<String>,

    #[arg(
        long = "state-rpc-url",
        env = "JITO_STATE_RPC_URLS",
        value_delimiter = ','
    )]
    pub(crate) state_rpc_urls: Vec<String>,

    #[arg(
        long = "address-lookup-table",
        env = "JITO_ADDRESS_LOOKUP_TABLES",
        value_delimiter = ','
    )]
    pub(crate) address_lookup_tables: Vec<String>,
}

fn parse_boolish(value: &str) -> std::result::Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "y" | "on" => Ok(true),
        "0" | "false" | "no" | "n" | "off" => Ok(false),
        _ => Err("expected one of true/false/yes/no/1/0/on/off".to_string()),
    }
}

impl LiveOptions {
    pub(crate) fn normalized_state_rpc_urls(&self) -> Vec<String> {
        normalized_rpc_urls(&self.state_rpc_urls, self.solana_rpc_url.as_deref())
    }
}

pub(crate) fn normalized_rpc_urls(urls: &[String], fallback: Option<&str>) -> Vec<String> {
    let mut normalized = urls
        .iter()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        if let Some(fallback) = fallback.map(str::trim).filter(|url| !url.is_empty()) {
            normalized.push(fallback.to_string());
        }
    }
    normalized
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Live(options) => live::run(options).await,
    }
}
