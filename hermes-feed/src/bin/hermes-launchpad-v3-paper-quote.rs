use std::str::FromStr;

use alloy_primitives::{B256, U256};
use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use hermes_feed::launchpad_adapter::LaunchpadId;
use hermes_feed::robinhood::{CHAIN_ID, PUBLIC_RPC_URL};
use hermes_feed::{NoxaRpcClient, V3ReceiptQuotePolicy, quote_v3_launch_receipt};

const DEFAULT_AMOUNT_IN_WEI: &str = "1000000000000000";
const DEFAULT_MAX_AMOUNT_IN_WEI: &str = "10000000000000000";

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LaunchpadArg {
    Bow,
    LaunchHoodV3,
}

impl From<LaunchpadArg> for LaunchpadId {
    fn from(value: LaunchpadArg) -> Self {
        match value {
            LaunchpadArg::Bow => Self::Bow,
            LaunchpadArg::LaunchHoodV3 => Self::LaunchHoodV3,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Read-only receipt-end Bow/LaunchHood V3 paper entry and exit quote"
)]
struct Cli {
    #[arg(long)]
    tx_hash: B256,
    #[arg(long, value_enum)]
    launchpad: LaunchpadArg,
    #[arg(long, default_value = DEFAULT_AMOUNT_IN_WEI)]
    amount_in_wei: String,
    #[arg(long, default_value = DEFAULT_MAX_AMOUNT_IN_WEI)]
    max_amount_in_wei: String,
    #[arg(long, default_value_t = 100)]
    slippage_bps: u16,
    #[arg(long, default_value = PUBLIC_RPC_URL)]
    rpc_url: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    let chain_id = rpc.chain_id().await?;
    if chain_id != CHAIN_ID {
        bail!("RPC chain ID {chain_id} does not match Robinhood {CHAIN_ID}");
    }
    let receipt = rpc
        .receipt(args.tx_hash)
        .await?
        .with_context(|| format!("missing receipt {}", args.tx_hash))?;
    let transaction = rpc
        .transaction_by_hash(args.tx_hash)
        .await?
        .with_context(|| format!("missing transaction {}", args.tx_hash))?;
    let policy = V3ReceiptQuotePolicy {
        amount_in: parse_u256(&args.amount_in_wei).context("parse --amount-in-wei")?,
        max_amount_in: parse_u256(&args.max_amount_in_wei).context("parse --max-amount-in-wei")?,
        slippage_bps: args.slippage_bps,
    };
    let quote = quote_v3_launch_receipt(&transaction, &receipt, args.launchpad.into(), policy)?;
    println!("{}", serde_json::to_string(&quote)?);
    Ok(())
}

fn parse_u256(value: &str) -> Result<U256> {
    U256::from_str(value).with_context(|| format!("invalid U256 {value}"))
}
