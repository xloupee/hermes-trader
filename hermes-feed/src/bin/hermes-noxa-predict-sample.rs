use std::{collections::BTreeMap, path::PathBuf};

use alloy_primitives::{U256, keccak256};
use anyhow::{Context, Result, bail};
use clap::Parser;
use hermes_feed::robinhood::{NOXA_LAUNCH_FACTORY, WETH};
use hermes_feed::{
    NoxaLaunchEvent, NoxaPredictor, NoxaRpcClient, ObservedLaunchLog, decode_launch_call,
    decode_token_launched, hydrate_noxa_launch_receipt,
};
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(about = "Differential-audit receipt-free prediction across historical NOXA launches")]
struct Cli {
    #[arg(long, default_value = "https://rpc.mainnet.chain.robinhood.com")]
    rpc_url: String,
    #[arg(long, default_value_t = 61_688)]
    from_l2_block: u64,
    #[arg(long, default_value_t = 6_880_646)]
    to_l2_block: u64,
    #[arg(long, default_value_t = 50_000)]
    blocks_per_request: u64,
    #[arg(long, default_value_t = 3)]
    samples_per_category: usize,
    #[arg(long, default_value = "10000000000000000")]
    amount_in: U256,
    /// Retain the complete reproducible manifest instead of printing it to stdout.
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct Candidate {
    observed: ObservedLaunchLog,
    event: NoxaLaunchEvent,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    if args.from_l2_block > args.to_l2_block
        || args.blocks_per_request == 0
        || args.samples_per_category == 0
        || args.amount_in == U256::ZERO
    {
        bail!("sample range, request span, sample count, and amount must be non-zero and valid");
    }
    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    let current = rpc.factory_status().await?;
    let launch_runtime = rpc
        .code_at_l2_block(NOXA_LAUNCH_FACTORY, current.pinned_l2_block)
        .await?;
    let current_dex = rpc
        .dex_config_at(U256::ZERO, current.pinned_l2_block)
        .await?;
    let dex_runtime = rpc
        .code_at_l2_block(current_dex.factory, current.pinned_l2_block)
        .await?;

    let mut categories: BTreeMap<&'static str, Vec<Candidate>> = BTreeMap::from([
        ("token0_zero_buy", Vec::new()),
        ("token0_nonzero_buy", Vec::new()),
        ("token1_zero_buy", Vec::new()),
        ("token1_nonzero_buy", Vec::new()),
    ]);
    let mut cursor = args.from_l2_block;
    let mut decoded_events = 0_u64;
    while cursor <= args.to_l2_block {
        let end = cursor
            .saturating_add(args.blocks_per_request.saturating_sub(1))
            .min(args.to_l2_block);
        for observed in rpc.token_launched_logs(cursor, end).await? {
            let event = decode_token_launched(&observed.log)
                .context("factory/topic-filtered log failed strict TokenLaunched decoding")?;
            decoded_events = decoded_events.saturating_add(1);
            let category = category(&event);
            categories
                .get_mut(category)
                .expect("all categories are initialized")
                .push(Candidate { observed, event });
        }
        if end == u64::MAX {
            break;
        }
        cursor = end + 1;
    }

    let category_counts: BTreeMap<_, _> = categories
        .iter()
        .map(|(name, values)| (*name, values.len()))
        .collect();
    let mut selected = Vec::new();
    for (name, values) in &categories {
        for index in evenly_spaced_indices(values.len(), args.samples_per_category) {
            selected.push((*name, values[index].clone()));
        }
    }
    selected.sort_by_key(|(_, candidate)| {
        (
            candidate.observed.l2_block_number,
            candidate.observed.transaction_index,
        )
    });

    let mut samples = Vec::with_capacity(selected.len());
    let mut exact_matches = 0_usize;
    let mut unavailable_historical_state = 0_usize;
    let mut verification_failures = 0_usize;
    let mut exact_matches_by_category = BTreeMap::<&'static str, usize>::new();
    for (category, candidate) in selected {
        let transaction_hash = candidate.observed.transaction_hash;
        let l2_block_number = candidate.observed.l2_block_number;
        let result = audit_candidate(
            &rpc,
            category,
            candidate,
            &launch_runtime,
            &dex_runtime,
            args.amount_in,
        )
        .await;
        match result {
            Ok(value) => {
                if value.get("exact_match").and_then(Value::as_bool) == Some(true) {
                    exact_matches += 1;
                    *exact_matches_by_category.entry(category).or_default() += 1;
                } else {
                    verification_failures += 1;
                }
                samples.push(value);
            }
            Err(error) => {
                let error = error.to_string();
                let unavailable = is_unavailable_historical_state(&error);
                if unavailable {
                    unavailable_historical_state += 1;
                } else {
                    verification_failures += 1;
                }
                samples.push(json!({
                    "category": category,
                    "exact_match": false,
                    "verification_status": if unavailable {
                        "unavailable_historical_state"
                    } else {
                        "error"
                    },
                    "transaction_hash": transaction_hash,
                    "l2_block_number": l2_block_number,
                    "error": error,
                }));
            }
        }
    }
    let required_categories_present = category_counts.values().all(|count| *count > 0);
    let all_exact = !samples.is_empty() && exact_matches == samples.len();
    let every_category_verified = categories.keys().all(|category| {
        exact_matches_by_category
            .get(category)
            .copied()
            .unwrap_or_default()
            > 0
    });
    let all_verifiable_samples_exact = verification_failures == 0 && exact_matches > 0;
    let record = json!({
        "record_type": "noxa_receipt_free_prediction_sample",
        "range": {
            "from_l2_block": args.from_l2_block,
            "to_l2_block": args.to_l2_block,
            "blocks_per_request": args.blocks_per_request,
        },
        "decoded_events": decoded_events,
        "category_counts": category_counts,
        "samples_per_category_requested": args.samples_per_category,
        "samples_run": samples.len(),
        "exact_matches": exact_matches,
        "exact_matches_by_category": exact_matches_by_category,
        "unavailable_historical_state": unavailable_historical_state,
        "verification_failures": verification_failures,
        "all_required_categories_present": required_categories_present,
        "every_category_verified": every_category_verified,
        "all_samples_exact": all_exact,
        "all_verifiable_samples_exact": all_verifiable_samples_exact,
        "launch_factory_runtime_keccak256": keccak256(&launch_runtime),
        "dex_factory_runtime_keccak256": keccak256(&dex_runtime),
        "rpc": rpc.metrics(),
        "samples": samples,
        "broadcast": false,
        "private_key_used": false,
    });
    if let Some(output) = &args.output {
        std::fs::write(output, serde_json::to_vec_pretty(&record)?)
            .with_context(|| format!("failed to write {}", output.display()))?;
        println!(
            "wrote {} exact samples to {}",
            exact_matches,
            output.display()
        );
    } else {
        println!("{}", serde_json::to_string(&record)?);
    }
    if !required_categories_present || !every_category_verified || !all_verifiable_samples_exact {
        bail!("historical prediction sample contained a verification failure");
    }
    Ok(())
}

async fn audit_candidate(
    rpc: &NoxaRpcClient,
    category: &str,
    candidate: Candidate,
    launch_runtime: &[u8],
    dex_runtime: &[u8],
    amount_in: U256,
) -> Result<Value> {
    let transaction = rpc
        .transaction_by_hash(candidate.observed.transaction_hash)
        .await?
        .context("sample transaction is missing")?;
    if transaction.l2_block_number != Some(candidate.observed.l2_block_number) {
        bail!("sample transaction block does not match its event");
    }
    let block = rpc
        .block_by_number(candidate.observed.l2_block_number)
        .await?;
    let intent = decode_launch_call(&transaction.input, transaction.value)
        .context("sample transaction is not canonical launchToken calldata")?;
    let (launch_config, dex_config, launch_fee, receipt) = tokio::try_join!(
        rpc.launch_config_at(intent.launch_config_id, candidate.observed.l2_block_number),
        rpc.dex_config_at(intent.dex_id, candidate.observed.l2_block_number),
        rpc.launch_fee_at(candidate.observed.l2_block_number),
        async {
            rpc.receipt(candidate.observed.transaction_hash)
                .await?
                .context("sample receipt is missing")
        },
    )?;
    if !receipt.status {
        bail!("TokenLaunched sample receipt reports failure");
    }
    let predictor = NoxaPredictor::new(
        NOXA_LAUNCH_FACTORY,
        launch_fee,
        launch_config.clone(),
        dex_config,
        launch_runtime,
        dex_runtime,
    )?;
    let predicted = predictor.predict(&intent, block.l1_block_number)?;
    let actual = hydrate_noxa_launch_receipt(
        &receipt.logs,
        block.l1_block_number,
        receipt.l2_block_number,
    )?;
    let predicted_quote = predicted.quote_entry(launch_config.pair_token, amount_in)?;
    let actual_quote = actual
        .pool
        .quote_exact_input(launch_config.pair_token, amount_in, None)?;
    let exact = predicted.token == candidate.event.token
        && predicted.token == actual.launch.token
        && predicted.pool == candidate.event.pool
        && predicted.pool == actual.launch.pool
        && predicted.restrictions_end_l1_block
            == u64::try_from(actual.launch.restrictions_end_l1_block)?
        && predicted.initial_buy_amount == actual.launch.initial_buy_amount
        && predicted.post_launch_pool.sqrt_price_x96 == actual.pool.sqrt_price_x96
        && predicted.post_launch_pool.tick == actual.pool.tick
        && predicted.post_launch_pool.liquidity == actual.pool.liquidity
        && predicted_quote == actual_quote;
    Ok(json!({
        "category": category,
        "exact_match": exact,
        "transaction_hash": candidate.observed.transaction_hash,
        "l2_block_number": candidate.observed.l2_block_number,
        "l1_block_number": block.l1_block_number,
        "token": actual.launch.token,
        "pool": actual.launch.pool,
        "launched_token_is_token0": actual.launch.token < WETH,
        "initial_buy_amount": actual.launch.initial_buy_amount,
        "post_launch_tick": actual.pool.tick,
        "post_launch_liquidity": format!("0x{:x}", actual.pool.liquidity),
        "quote_amount_in": amount_in,
        "quote_amount_out": actual_quote.amount_out,
    }))
}

fn category(event: &NoxaLaunchEvent) -> &'static str {
    match (
        event.token < event.pair_token,
        event.initial_buy_amount == U256::ZERO,
    ) {
        (true, true) => "token0_zero_buy",
        (true, false) => "token0_nonzero_buy",
        (false, true) => "token1_zero_buy",
        (false, false) => "token1_nonzero_buy",
    }
}

fn is_unavailable_historical_state(error: &str) -> bool {
    error.contains("missing trie node")
        && error.contains("state")
        && error.contains("not available")
}

fn evenly_spaced_indices(len: usize, requested: usize) -> Vec<usize> {
    let count = len.min(requested);
    if count == 0 {
        return Vec::new();
    }
    (0..count)
        .map(|index| ((2 * index + 1) * len) / (2 * count))
        .collect()
}

#[cfg(test)]
mod tests {
    use alloy_primitives::Address;

    use super::*;

    #[test]
    fn sample_indices_cover_the_bucket_without_duplicates() {
        assert_eq!(evenly_spaced_indices(10, 3), vec![1, 5, 8]);
        assert_eq!(evenly_spaced_indices(2, 3), vec![0, 1]);
        assert!(evenly_spaced_indices(0, 3).is_empty());
    }

    #[test]
    fn categorizes_both_orientations_and_buy_modes() {
        let event = |token, pair_token, initial_buy_amount| NoxaLaunchEvent {
            token,
            deployer: Address::with_last_byte(3),
            dex_factory: Address::with_last_byte(4),
            pair_token,
            pool: Address::with_last_byte(5),
            dex_id: U256::ZERO,
            launch_config_id: U256::ZERO,
            position_id: U256::ZERO,
            restrictions_end_l1_block: U256::ZERO,
            initial_buy_amount,
        };
        assert_eq!(
            category(&event(
                Address::with_last_byte(1),
                Address::with_last_byte(2),
                U256::ZERO
            )),
            "token0_zero_buy"
        );
        assert_eq!(
            category(&event(
                Address::with_last_byte(2),
                Address::with_last_byte(1),
                U256::from(1)
            )),
            "token1_nonzero_buy"
        );
    }

    #[test]
    fn distinguishes_unavailable_archive_state_from_verification_errors() {
        assert!(is_unavailable_historical_state(
            "eth_call JSON-RPC error: missing trie node abc state 0xabc is not available, not found"
        ));
        assert!(!is_unavailable_historical_state(
            "predicted pool does not match receipt"
        ));
    }
}
