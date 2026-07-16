use anyhow::{Result, bail};
use clap::Parser;
use hermes_feed::NoxaRpcClient;
use hermes_feed::robinhood::{CHAIN_ID, PUBLIC_RPC_URL};
use serde_json::json;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Read one canonical Robinhood L2 head without mutation"
)]
struct Cli {
    #[arg(long, default_value = PUBLIC_RPC_URL)]
    rpc_url: String,
    #[arg(long)]
    number_only: bool,
    /// Emit `<number> <hash>` from the same RPC block response for shell scripts.
    #[arg(long, conflicts_with = "number_only")]
    shell_fields: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    let chain_id = rpc.chain_id().await?;
    if chain_id != CHAIN_ID {
        bail!("RPC chain ID {chain_id} does not match Robinhood {CHAIN_ID}");
    }
    let block = rpc.latest_block().await?;
    if args.number_only {
        println!("{}", block.l2_block_number);
    } else if args.shell_fields {
        println!("{} {}", block.l2_block_number, block.hash);
    } else {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "record_type": "launchpad_chain_head",
                "chain_id": CHAIN_ID,
                "l2_block_number": block.l2_block_number,
                "block_hash": block.hash,
                "block_timestamp": block.timestamp,
                "l1_block_number": block.l1_block_number,
            }))?
        );
    }
    Ok(())
}
