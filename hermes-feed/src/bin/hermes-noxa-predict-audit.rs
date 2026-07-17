use alloy_primitives::{Address, B256, U256, keccak256};
use anyhow::{Context, Result, bail};
use clap::Parser;
use hermes_feed::{
    AutomatedPaperRuntime, FeedBoundary, NoxaPredictor, NoxaRpcClient, PaperOrderState,
    PredictedNoxaTradeInput, RiskLimits, decode_launch_call, hydrate_noxa_launch_receipt,
    prepare_predicted_noxa_trade,
};
use serde_json::json;
use std::time::Instant;

#[derive(Debug, Parser)]
#[command(about = "Audit receipt-free NOXA CREATE2 and pool-state prediction")]
struct Cli {
    #[arg(long, default_value = "https://rpc.mainnet.chain.robinhood.com")]
    rpc_url: String,
    #[arg(
        long,
        default_value = "0xc62997c2607d579233b552fad71faae7e392a4c13bc92b9d20c57425b9ffe418"
    )]
    tx_hash: B256,
    #[arg(long, default_value = "10000000000000000")]
    amount_in: U256,
    #[arg(long, default_value_t = 10_000)]
    benchmark_iterations: u64,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    let transaction = rpc
        .transaction_by_hash(args.tx_hash)
        .await?
        .context("historical launch transaction is missing")?;
    let l2_block = transaction
        .l2_block_number
        .context("historical launch transaction is unmined")?;
    let block = rpc.block_by_number(l2_block).await?;
    let intent = decode_launch_call(&transaction.input, transaction.value)
        .context("transaction is not canonical launchToken calldata")?;
    let launch_config = rpc
        .launch_config_at(intent.launch_config_id, l2_block)
        .await?;
    let dex_config = rpc.dex_config_at(intent.dex_id, l2_block).await?;
    let launch_fee = rpc.launch_fee_at(l2_block).await?;
    let launch_runtime = rpc
        .code_at_l2_block(hermes_feed::robinhood::NOXA_LAUNCH_FACTORY, l2_block)
        .await?;
    let dex_runtime = rpc.code_at_l2_block(dex_config.factory, l2_block).await?;
    let predictor = NoxaPredictor::new(
        hermes_feed::robinhood::NOXA_LAUNCH_FACTORY,
        launch_fee,
        launch_config.clone(),
        dex_config.clone(),
        &launch_runtime,
        &dex_runtime,
    )?;
    let predicted = predictor.predict(&intent, block.l1_block_number)?;
    let predicted_quote = predicted.quote_entry(launch_config.pair_token, args.amount_in)?;
    let receipt = rpc
        .receipt(args.tx_hash)
        .await?
        .context("historical launch receipt is missing")?;
    if !receipt.status {
        bail!("historical launch reverted");
    }
    let actual = hydrate_noxa_launch_receipt(
        &receipt.logs,
        block.l1_block_number,
        receipt.l2_block_number,
    )?;
    let actual_quote =
        actual
            .pool
            .quote_exact_input(launch_config.pair_token, args.amount_in, None)?;
    let exact = predicted.token == actual.launch.token
        && predicted.pool == actual.launch.pool
        && predicted.restrictions_end_l1_block
            == u64::try_from(actual.launch.restrictions_end_l1_block)?
        && predicted.initial_buy_amount == actual.launch.initial_buy_amount
        && predicted.post_launch_pool.sqrt_price_x96 == actual.pool.sqrt_price_x96
        && predicted.post_launch_pool.tick == actual.pool.tick
        && predicted.post_launch_pool.liquidity == actual.pool.liquidity
        && predicted_quote == actual_quote;
    let paper_recipient = Address::with_last_byte(0x42);
    let paper_candidate = prepare_predicted_noxa_trade(PredictedNoxaTradeInput {
        launch: &predicted,
        launch_l1_block: block.l1_block_number,
        launch_l1_timestamp: block.timestamp,
        recipient: paper_recipient,
        amount_in: args.amount_in,
        quoted_amount_out: predicted_quote.amount_out,
        slippage_bps: 500,
        nonce: 7,
        gas_limit: 350_000,
        max_fee_per_gas: 20_000_000,
        max_priority_fee_per_gas: 0,
        l1_window: 3,
        timestamp_window_seconds: 30,
    })?;
    let mut paper = AutomatedPaperRuntime::new(
        7,
        RiskLimits {
            max_trade_amount_in: args.amount_in,
            max_open_exposure: args.amount_in,
            max_gas_cost_wei: U256::from(7_000_000_000_000_u64),
            max_session_loss: U256::from(20_000_000_000_000_u64),
            max_slippage_bps: 500,
        },
    );
    let paper_order = paper.prepare_entry(
        predicted.token,
        args.amount_in,
        predicted_quote.amount_out,
        350_000,
        20_000_000,
        500,
        paper_candidate.conditions,
    )?;
    let waiting = paper.observe_boundary(FeedBoundary {
        l1_block_number: block.l1_block_number,
        l1_timestamp: block.timestamp,
        sequence_contiguous: true,
    })?;
    let submitted = paper.observe_boundary(FeedBoundary {
        l1_block_number: block.l1_block_number + 1,
        l1_timestamp: block.timestamp + 1,
        sequence_contiguous: true,
    })?;
    let paper_reconciliation =
        paper.reconcile_fill(paper_order.id, predicted_quote.amount_out, U256::ZERO)?;
    let paper_snapshot = paper.snapshot();
    let paper_exact = waiting.state == PaperOrderState::Prepared
        && submitted.state == PaperOrderState::Submitted
        && paper_snapshot.next_nonce == 8
        && paper_snapshot.open_exposure == args.amount_in
        && paper_snapshot.positions.len() == 1
        && paper_snapshot.positions[0].token == predicted.token
        && paper_snapshot.positions[0].token_amount == predicted_quote.amount_out;
    let benchmark_started = Instant::now();
    let mut benchmark_checksum = U256::ZERO;
    for _ in 0..args.benchmark_iterations {
        let iteration = predictor.predict(&intent, block.l1_block_number)?;
        let quote = iteration.quote_entry(launch_config.pair_token, args.amount_in)?;
        benchmark_checksum ^= quote.amount_out;
    }
    let benchmark_elapsed_ns = benchmark_started.elapsed().as_nanos();
    let benchmark_ns_per_iteration = benchmark_elapsed_ns
        .checked_div(u128::from(args.benchmark_iterations))
        .unwrap_or_default();
    println!(
        "{}",
        serde_json::to_string(&json!({
            "record_type": "noxa_receipt_free_prediction_audit",
            "exact_match": exact,
            "tx_hash": args.tx_hash,
            "l2_block": l2_block,
            "l1_block": block.l1_block_number,
            "launch_factory_runtime_keccak256": keccak256(&launch_runtime),
            "dex_factory_runtime_keccak256": keccak256(&dex_runtime),
            "token_creation_code_keccak256": predictor.token_creation_code_hash(),
            "pool_init_code_keccak256": predictor.pool_init_code_hash(),
            "launch_config": launch_config,
            "dex_config": dex_config,
            "predicted": predicted,
            "actual_token": actual.launch.token,
            "actual_pool": actual.launch.pool,
            "predicted_quote": predicted_quote,
            "actual_quote": actual_quote,
            "paper_flow": {
                "exact": paper_exact,
                "order": paper_order,
                "launch_boundary": waiting,
                "first_eligible_boundary": submitted,
                "reconciliation": paper_reconciliation,
                "final_runtime": paper_snapshot,
            },
            "benchmark": {
                "iterations": args.benchmark_iterations,
                "elapsed_ns": benchmark_elapsed_ns,
                "ns_per_prediction_and_quote": benchmark_ns_per_iteration,
                "checksum": benchmark_checksum,
            },
        }))?
    );
    if !exact || !paper_exact {
        bail!("receipt-free prediction or paper boundary flow did not exactly match");
    }
    Ok(())
}
