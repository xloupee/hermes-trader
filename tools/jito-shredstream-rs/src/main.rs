use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod address_lookup;
mod blockhash;
mod event;
mod executor;
mod live;
mod parser;
mod planner;
mod proto;
mod signal;
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

    #[arg(long, default_value_t = false)]
    pub(crate) print_mentions: bool,

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

    #[arg(long, env = "JITO_BLOCKHASH_REFRESH_MS", default_value_t = 500)]
    pub(crate) blockhash_refresh_ms: u64,

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

    #[arg(
        long = "send-rpc-url",
        env = "JITO_SEND_RPC_URLS",
        value_delimiter = ','
    )]
    pub(crate) send_rpc_urls: Vec<String>,

    #[arg(
        long = "jito-send-url",
        env = "JITO_BLOCK_ENGINE_SEND_URLS",
        value_delimiter = ','
    )]
    pub(crate) jito_send_urls: Vec<String>,

    #[arg(long, env = "JITO_BLOCK_ENGINE_AUTH_UUID", hide_env_values = true)]
    pub(crate) jito_auth_uuid: Option<String>,

    #[arg(long, env = "JITO_ONE_SHOT_COPY_SEND", default_value_t = false)]
    pub(crate) one_shot_copy_send: bool,

    #[arg(long, env = "JITO_DRY_RUN", default_value_t = true)]
    pub(crate) dry_run: bool,

    #[arg(long, env = "JITO_MAX_COPY_SOL")]
    pub(crate) max_copy_sol: Option<f64>,

    #[arg(long, env = "JITO_MAX_TOTAL_COPY_SPEND_SOL")]
    pub(crate) max_total_copy_spend_sol: Option<f64>,

    #[arg(long, env = "JITO_COPY_KEYPAIR_PATH")]
    pub(crate) copy_keypair_path: Option<PathBuf>,

    #[arg(long, env = "JITO_COPY_EXECUTIONS_PATH")]
    pub(crate) copy_executions_path: Option<PathBuf>,

    #[arg(long, env = "JITO_AUTO_SELL_AFTER_BUY", default_value_t = false)]
    pub(crate) auto_sell_after_buy: bool,

    #[arg(long, env = "JITO_AUTO_SELL_DELAY_MS", default_value_t = 1_000)]
    pub(crate) auto_sell_delay_ms: u64,

    #[arg(long, env = "JITO_PRIORITY_FEE_MICRO_LAMPORTS")]
    pub(crate) priority_fee_micro_lamports: Option<u64>,

    #[arg(long, env = "JITO_TIP_LAMPORTS")]
    pub(crate) jito_tip_lamports: Option<u64>,

    #[arg(long, env = "JITO_TIP_ACCOUNT")]
    pub(crate) jito_tip_account: Option<String>,

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

    #[arg(long, env = "SOLANA_RPC_URL")]
    pub(crate) solana_rpc_url: Option<String>,

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

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Live(options) => live::run(options).await,
    }
}
