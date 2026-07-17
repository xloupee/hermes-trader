use std::collections::HashSet;

use alloy_primitives::{Address, B256, U256};
use anyhow::{Context, Result, bail};
use clap::Parser;
use hermes_feed::robinhood::{
    ACTIVE_NOXA_LAUNCH_FACTORY, CHAIN_ID, NOXA_POOL_FEE, PUBLIC_RPC_URL, ROBINHOOD_SWAP_AGGREGATOR,
    UNISWAP_V3_FACTORY, UNISWAP_V3_POOL_INIT_CODE_KECCAK256, UNISWAP_V3_SWAP_ROUTER_02, WETH,
};
use hermes_feed::{
    NoxaRpcClient, ObservedCopySwap, WatchedWalletCopyPolicy, decode_aggregator_swap,
    decode_v3_exact_input_single, normalize_aggregator_copy_swap, predict_v3_pool_address,
    validate_active_noxa_copy_token,
};
use serde_json::json;

const FOLLOWER_AMOUNT_IN: u64 = 100_000_000_000_000;
const MAX_LEADER_ENTRY_AMOUNT: u64 = 1_000_000_000_000_000_000;

#[derive(Debug, Parser)]
#[command(about = "Read-only production-policy audit for historical Noxa copy candidates")]
struct Cli {
    #[arg(long, default_value = PUBLIC_RPC_URL)]
    rpc_url: String,
    #[arg(long, required = true)]
    tx_hash: Vec<B256>,
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

    for tx_hash in args.tx_hash {
        let transaction = rpc
            .transaction_by_hash(tx_hash)
            .await?
            .context("copy candidate transaction is missing")?;
        let receipt = rpc
            .receipt(tx_hash)
            .await?
            .context("copy candidate receipt is missing")?;
        let aggregator_diagnostics = if transaction.to == Some(ROBINHOOD_SWAP_AGGREGATOR) {
            decode_aggregator_swap(&transaction.input).map(|call| {
                json!({
                    "descriptor_count": call.descriptors.len(),
                    "fee_token": call.fee_token,
                    "amount_in": call.amount_in,
                    "minimum_return": call.minimum_return,
                    "user_fee_rate": call.user_fee_rate,
                    "descriptors": call.descriptors.iter().map(|leg| json!({
                        "dex_id": leg.dex_id,
                        "token_in": leg.token_in,
                        "token_out": leg.token_out,
                        "pool": leg.pool,
                        "fee": leg.fee,
                        "tick_spacing": leg.tick_spacing,
                        "router": leg.router,
                        "data": leg.data,
                        "callback": leg.callback,
                        "metadata": leg.metadata,
                    })).collect::<Vec<_>>(),
                })
            })
        } else {
            None
        };
        let normalized: Result<_, String> = if transaction.to == Some(UNISWAP_V3_SWAP_ROUTER_02) {
            decode_v3_exact_input_single(&transaction.input)
                .map(|intent| {
                    let token = copy_token(&intent).unwrap_or(Address::ZERO);
                    (intent, expected_pool(token), "direct_swap_router_02")
                })
                .ok_or_else(|| "malformed direct SwapRouter02 calldata".to_string())
        } else if transaction.to == Some(ROBINHOOD_SWAP_AGGREGATOR) {
            normalize_aggregator_copy_swap(&transaction.input, transaction.value, transaction.from)
                .map_err(|error| error.to_string())
                .map(|swap| (swap.intent, swap.pool, "robinhood_aggregator"))
        } else {
            Err("transaction does not target an allowlisted copy route".to_string())
        };
        let (intent, pool, route) = match normalized {
            Ok(normalized) => normalized,
            Err(reason) => {
                println!(
                    "{}",
                    serde_json::to_string(&json!({
                        "record_type": "active_noxa_copy_candidate_audit",
                        "tx_hash": tx_hash,
                        "leader": transaction.from,
                        "target": transaction.to,
                        "receipt_status": receipt.status,
                        "eligible": false,
                        "reason": reason,
                        "aggregator_diagnostics": aggregator_diagnostics,
                    }))?
                );
                continue;
            }
        };
        let Some(token) = copy_token(&intent) else {
            bail!("normalized copy candidate is not a WETH pair");
        };
        let record = rpc
            .active_noxa_launch_record(ACTIVE_NOXA_LAUNCH_FACTORY, token, status.pinned_l2_block)
            .await?;
        if record.token != token || record.pool != pool {
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "record_type": "active_noxa_copy_candidate_audit",
                    "tx_hash": tx_hash,
                    "leader": transaction.from,
                    "target": transaction.to,
                    "route": route,
                    "receipt_status": receipt.status,
                    "token": token,
                    "pool": pool,
                    "token_valid": false,
                    "eligible": false,
                    "reason": "token is not registered to the expected pool by the active Noxa factory",
                }))?
            );
            continue;
        }
        let (token_view, token_code, pool_code, pool_view) = tokio::try_join!(
            rpc.active_noxa_token_snapshot(token, status.pinned_l2_block),
            rpc.code_at_l2_block(token, status.pinned_l2_block),
            rpc.code_at_l2_block(pool, status.pinned_l2_block),
            rpc.v3_pool_snapshot_at(pool, status.pinned_l2_block),
        )?;
        let token_valid = !token_code.is_empty()
            && validate_active_noxa_copy_token(
                token,
                &record,
                &token_view,
                &pool_view,
                &pool_code,
                &config,
            )
            .is_ok();
        let observed = ObservedCopySwap {
            tx_hash,
            chain_id: Some(CHAIN_ID),
            from: transaction.from,
            to: UNISWAP_V3_SWAP_ROUTER_02,
            value: U256::ZERO,
            intent: intent.clone(),
        };
        let policy = WatchedWalletCopyPolicy::new(
            HashSet::from([transaction.from]),
            HashSet::from([token]),
            U256::from(FOLLOWER_AMOUNT_IN),
            U256::from(MAX_LEADER_ENTRY_AMOUNT),
            1,
        )?;
        let decision = if token_valid && receipt.status {
            policy.evaluate_validated(&observed, None, 0)
        } else {
            Err(hermes_feed::CopyRejectReason::TokenNotAllowed)
        };
        println!(
            "{}",
            serde_json::to_string(&json!({
                "record_type": "active_noxa_copy_candidate_audit",
                "tx_hash": tx_hash,
                "leader": transaction.from,
                "target": transaction.to,
                "route": route,
                "receipt_status": receipt.status,
                "token": token,
                "pool": pool,
                "token_valid": token_valid,
                "direction": if intent.token_in == WETH { "entry" } else { "exit" },
                "amount_in": intent.amount_in,
                "amount_out_minimum": intent.amount_out_minimum,
                "eligible": decision.is_ok(),
                "decision": decision.as_ref().ok(),
                "reason": decision.as_ref().err().map(ToString::to_string),
            }))?
        );
    }
    Ok(())
}

fn copy_token(intent: &hermes_feed::V3ExactInputIntent) -> Option<Address> {
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

fn expected_pool(token: Address) -> Address {
    let (token0, token1) = if token < WETH {
        (token, WETH)
    } else {
        (WETH, token)
    };
    predict_v3_pool_address(
        UNISWAP_V3_FACTORY,
        token0,
        token1,
        NOXA_POOL_FEE,
        UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    )
}
