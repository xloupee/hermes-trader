use anyhow::Result;
use clap::{Parser, Subcommand};

mod event;
mod live;
mod parser;
mod proto;

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
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Live(options) => live::run(options).await,
    }
}
