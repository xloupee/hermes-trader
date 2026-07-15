use alloy_primitives::{Address, U256};
use anyhow::{Result, bail};
use clap::Parser;
use hermes_feed::robinhood::{ACTIVE_NOXA_LAUNCH_FACTORY, CHAIN_ID, PUBLIC_RPC_URL};
use hermes_feed::{NoxaRpcClient, validate_active_noxa_copy_token};
use serde_json::json;

#[derive(Debug, Parser)]
#[command(about = "Read-only batch verifier for active-Noxa token and pool identity")]
struct Cli {
    #[arg(long, default_value = PUBLIC_RPC_URL)]
    rpc_url: String,
    #[arg(long, required = true)]
    token: Vec<Address>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    if rpc.chain_id().await? != CHAIN_ID {
        bail!("RPC is not Robinhood Chain mainnet");
    }
    let status = rpc.factory_status_for(ACTIVE_NOXA_LAUNCH_FACTORY).await?;
    let config = rpc
        .launch_config_at_for(
            ACTIVE_NOXA_LAUNCH_FACTORY,
            U256::ZERO,
            status.pinned_l2_block,
        )
        .await?;

    for token in args.token {
        let record = match rpc
            .active_noxa_launch_record(ACTIVE_NOXA_LAUNCH_FACTORY, token, status.pinned_l2_block)
            .await
        {
            Ok(record) => record,
            Err(error) => {
                println!(
                    "{}",
                    serde_json::to_string(&json!({
                        "record_type": "active_noxa_token_check",
                        "token": token,
                        "valid": false,
                        "reason": "factory_record_query_failed",
                        "error": error.to_string(),
                    }))?
                );
                continue;
            }
        };
        if record.token != token || record.pool == Address::ZERO {
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "record_type": "active_noxa_token_check",
                    "token": token,
                    "valid": false,
                    "reason": "not_registered_by_active_noxa_factory",
                    "factory_record_token": record.token,
                    "factory_record_pool": record.pool,
                }))?
            );
            continue;
        }

        let checked = async {
            let (token_view, token_code, pool_code, pool_view) = tokio::try_join!(
                rpc.active_noxa_token_snapshot(token, status.pinned_l2_block),
                rpc.code_at_l2_block(token, status.pinned_l2_block),
                rpc.code_at_l2_block(record.pool, status.pinned_l2_block),
                rpc.v3_pool_snapshot_at(record.pool, status.pinned_l2_block),
            )?;
            if token_code.is_empty() {
                bail!("registered token has no runtime bytecode");
            }
            validate_active_noxa_copy_token(
                token,
                &record,
                &token_view,
                &pool_view,
                &pool_code,
                &config,
            )?;
            Result::<_>::Ok((token_view, pool_view, token_code.len(), pool_code.len()))
        }
        .await;
        match checked {
            Ok((token_view, pool_view, token_code_bytes, pool_code_bytes)) => println!(
                "{}",
                serde_json::to_string(&json!({
                    "record_type": "active_noxa_token_check",
                    "token": token,
                    "pool": record.pool,
                    "valid": true,
                    "pinned_l2_block": status.pinned_l2_block,
                    "restriction_end_l1_block": token_view.restrictions_end_block.to_string(),
                    "pool_fee": pool_view.fee,
                    "pool_liquidity": pool_view.liquidity.to_string(),
                    "token_code_bytes": token_code_bytes,
                    "pool_code_bytes": pool_code_bytes,
                }))?
            ),
            Err(error) => println!(
                "{}",
                serde_json::to_string(&json!({
                    "record_type": "active_noxa_token_check",
                    "token": token,
                    "pool": record.pool,
                    "valid": false,
                    "reason": "registered_token_validation_failed",
                    "error": error.to_string(),
                }))?
            ),
        }
    }
    Ok(())
}
