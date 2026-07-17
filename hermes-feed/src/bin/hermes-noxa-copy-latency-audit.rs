use std::collections::HashSet;
use std::time::Instant;

use alloy_primitives::{B256, U256};
use anyhow::{Context, Result, bail};
use clap::Parser;
use hermes_feed::robinhood::{
    ACTIVE_NOXA_LAUNCH_FACTORY, CHAIN_ID, NOXA_POOL_FEE, PUBLIC_RPC_URL, ROBINHOOD_SWAP_AGGREGATOR,
    UNISWAP_V3_FACTORY, UNISWAP_V3_POOL_INIT_CODE_KECCAK256, UNISWAP_V3_SWAP_ROUTER_02, WETH,
};
use hermes_feed::{
    AutomatedPaperRuntime, ConditionalOptions, CopyDecision, NoxaRpcClient, ObservedCopySwap,
    PaperOrderKind, RiskLimits, WatchedWalletCopyPolicy, decode_v3_exact_input_single,
    normalize_aggregator_copy_swap, predict_v3_pool_address, validate_active_noxa_copy_token,
};
use serde_json::json;

const DEFAULT_REPLAY_TX: B256 =
    alloy_primitives::b256!("77f70b0e67cae3d3e7b0155389c6886817ff737f288c710eabad8bef51182298");
const FOLLOWER_AMOUNT_IN: u64 = 100_000_000_000_000;
const MAX_LEADER_ENTRY_AMOUNT: u64 = 1_000_000_000_000_000_000;

#[derive(Debug, Parser)]
#[command(about = "Read-only replay benchmark for a verified active-Noxa copy transaction")]
struct Cli {
    #[arg(long, default_value = PUBLIC_RPC_URL)]
    rpc_url: String,
    #[arg(long, default_value_t = DEFAULT_REPLAY_TX)]
    tx_hash: B256,
    #[arg(long, default_value_t = 10_000)]
    iterations: usize,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    if args.iterations == 0 || args.iterations > 1_000_000 {
        bail!("--iterations must be between 1 and 1000000");
    }

    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    if rpc.chain_id().await? != CHAIN_ID {
        bail!("RPC is not Robinhood Chain mainnet");
    }
    let transaction = rpc
        .transaction_by_hash(args.tx_hash)
        .await?
        .context("historical copy transaction is missing")?;
    let (intent, pool, normalized_value, source_route) =
        if transaction.to == Some(UNISWAP_V3_SWAP_ROUTER_02) {
            let intent = decode_v3_exact_input_single(&transaction.input)
                .context("historical direct transaction is not exactInputSingle")?;
            let token = intent.token_out;
            let (token0, token1) = if token < WETH {
                (token, WETH)
            } else {
                (WETH, token)
            };
            (
                intent,
                predict_v3_pool_address(
                    UNISWAP_V3_FACTORY,
                    token0,
                    token1,
                    NOXA_POOL_FEE,
                    UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
                ),
                transaction.value,
                "direct_swap_router_02",
            )
        } else if transaction.to == Some(ROBINHOOD_SWAP_AGGREGATOR) {
            let normalized = normalize_aggregator_copy_swap(
                &transaction.input,
                transaction.value,
                transaction.from,
            )?;
            (
                normalized.intent,
                normalized.pool,
                U256::ZERO,
                "robinhood_aggregator",
            )
        } else {
            bail!("historical transaction does not target an allowlisted copy route");
        };
    if intent.token_in != WETH {
        bail!("historical replay transaction is not a Noxa entry");
    }
    let historical_amount_out_minimum = intent.amount_out_minimum;
    if historical_amount_out_minimum == U256::ZERO {
        bail!("historical replay transaction has no slippage protection");
    }
    let benchmark_amount_out_minimum = intent.amount_out_minimum;
    let receipt = rpc
        .receipt(args.tx_hash)
        .await?
        .context("historical copy receipt is missing")?;
    if !receipt.status {
        bail!("historical copy transaction reverted");
    }
    let block = rpc.block_by_number(receipt.l2_block_number).await?;
    let token = intent.token_out;
    let (config, record, token_view, token_code, pool_code, pool_view) = tokio::try_join!(
        rpc.launch_config_at_for(
            ACTIVE_NOXA_LAUNCH_FACTORY,
            U256::ZERO,
            receipt.l2_block_number
        ),
        rpc.active_noxa_launch_record(ACTIVE_NOXA_LAUNCH_FACTORY, token, receipt.l2_block_number),
        rpc.active_noxa_token_snapshot(token, receipt.l2_block_number),
        rpc.code_at_l2_block(token, receipt.l2_block_number),
        rpc.code_at_l2_block(pool, receipt.l2_block_number),
        rpc.v3_pool_snapshot_at(pool, receipt.l2_block_number),
    )?;
    if token_code.is_empty() {
        bail!("historical Noxa token has no bytecode at its replay block");
    }
    validate_active_noxa_copy_token(token, &record, &token_view, &pool_view, &pool_code, &config)?;

    let observed = ObservedCopySwap {
        tx_hash: args.tx_hash,
        chain_id: Some(CHAIN_ID),
        from: transaction.from,
        to: UNISWAP_V3_SWAP_ROUTER_02,
        value: normalized_value,
        intent,
    };
    let policy = WatchedWalletCopyPolicy::new(
        HashSet::from([transaction.from]),
        HashSet::from([token]),
        U256::from(FOLLOWER_AMOUNT_IN),
        U256::from(MAX_LEADER_ENTRY_AMOUNT),
        1,
    )?;
    let conditions = ConditionalOptions::first_eligible_window(
        block.l1_block_number,
        3,
        block.timestamp.checked_add(30),
    )
    .context("historical replay boundary window overflow")?;
    let limits = RiskLimits {
        max_trade_amount_in: U256::from(FOLLOWER_AMOUNT_IN),
        max_open_exposure: U256::from(FOLLOWER_AMOUNT_IN),
        max_gas_cost_wei: U256::from(FOLLOWER_AMOUNT_IN),
        max_session_loss: U256::from(FOLLOWER_AMOUNT_IN * 2),
        max_slippage_bps: 100,
    };
    let mut samples = Vec::with_capacity(args.iterations);
    for _ in 0..args.iterations {
        let mut runtime = AutomatedPaperRuntime::new(0, limits);
        let started = Instant::now();
        let decision = policy.evaluate_validated(&observed, None, 0)?;
        let CopyDecision::Entry {
            token,
            follower_amount_in,
            follower_minimum_out,
            ..
        } = decision
        else {
            bail!("historical replay unexpectedly produced a copy exit");
        };
        let order = runtime.prepare_entry(
            token,
            follower_amount_in,
            follower_minimum_out,
            350_000,
            20_000_000,
            100,
            conditions,
        )?;
        samples.push(started.elapsed().as_nanos());
        if !matches!(order.kind, PaperOrderKind::Entry { token: order_token, .. } if order_token == token)
        {
            bail!("paper replay order token mismatch");
        }
    }
    samples.sort_unstable();
    let percentile = |numerator: usize, denominator: usize| -> u128 {
        let index = (samples.len() - 1)
            .saturating_mul(numerator)
            .checked_div(denominator)
            .unwrap_or_default();
        samples[index]
    };
    println!(
        "{}",
        serde_json::to_string(&json!({
            "record_type": "active_noxa_copy_detection_to_order_replay",
            "measurement_scope": "normalized_verified_candidate_to_paper_order",
            "live_measurement": false,
            "historical_transaction": args.tx_hash,
            "leader": transaction.from,
            "token": token,
            "pool": pool,
            "l2_block_number": receipt.l2_block_number,
            "l1_block_number": block.l1_block_number,
            "factory": ACTIVE_NOXA_LAUNCH_FACTORY,
            "router": UNISWAP_V3_SWAP_ROUTER_02,
            "source_route": source_route,
            "factory_and_pool_revalidated": true,
            "historical_amount_out_minimum": historical_amount_out_minimum,
            "benchmark_limit_adjusted": false,
            "benchmark_amount_out_minimum": benchmark_amount_out_minimum,
            "production_policy_would_accept_historical_trade": true,
            "iterations": args.iterations,
            "latency_ns": {
                "min": samples[0],
                "median": percentile(50, 100),
                "p95": percentile(95, 100),
                "p99": percentile(99, 100),
                "max": samples[samples.len() - 1],
            },
        }))?
    );
    Ok(())
}
