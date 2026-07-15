use std::collections::{BTreeMap, BTreeSet, HashSet};

use alloy_primitives::{Address, B256, U256};
use anyhow::{Context, Result, bail};
use clap::Parser;
use futures_util::{StreamExt, stream};
use hermes_feed::robinhood::{
    ACTIVE_NOXA_LAUNCH_FACTORY, CHAIN_ID, PUBLIC_RPC_URL, ROBINHOOD_SWAP_AGGREGATOR,
    UNISWAP_V3_SWAP_ROUTER_02, WETH,
};
use hermes_feed::{
    CopyDecision, NoxaRpcClient, ObservedCopySwap, V3ExactInputIntent, WatchedWalletCopyPolicy,
    decode_token_launched, decode_v3_exact_input_single, normalize_aggregator_copy_swap,
};
use serde::Serialize;
use serde_json::json;

const DEFAULT_LAUNCH_FROM_BLOCK: u64 = 8_000_000;
const DEFAULT_RECENT_BLOCKS: u64 = 100_000;
const LOG_BLOCK_CHUNK: u64 = 25_000;
const POOL_CHUNK: usize = 50;
const DEFAULT_TX_CONCURRENCY: usize = 2;
const MAX_LEADER_ENTRY_AMOUNT: u64 = 1_000_000_000_000_000_000;

#[derive(Debug, Parser)]
#[command(about = "Read-only wallet ranking over recent canonical active-Noxa pool swaps")]
struct Cli {
    #[arg(long, default_value = PUBLIC_RPC_URL)]
    rpc_url: String,
    #[arg(long, default_value_t = DEFAULT_LAUNCH_FROM_BLOCK)]
    launch_from_block: u64,
    #[arg(long, default_value_t = DEFAULT_RECENT_BLOCKS)]
    recent_blocks: u64,
    #[arg(long, conflicts_with = "recent_blocks")]
    scan_from_block: Option<u64>,
    #[arg(long, default_value_t = DEFAULT_TX_CONCURRENCY)]
    tx_concurrency: usize,
    #[arg(long, default_value_t = 25)]
    top_wallets: usize,
    #[arg(long, default_value_t = 50)]
    latest_entries: usize,
}

#[derive(Debug, Clone, Copy)]
struct PoolIdentity {
    token: Address,
    pool: Address,
}

#[derive(Debug, Default, Serialize)]
struct WalletStats {
    pool_swap_transactions: u64,
    canonical_copy_route_transactions: u64,
    safe_entries: u64,
    exits: u64,
    other_or_unsupported: u64,
    tokens: BTreeSet<Address>,
    latest_l2_block: u64,
}

#[derive(Debug, Serialize)]
struct EligibleEntry {
    leader: Address,
    tx_hash: B256,
    l2_block: u64,
    token: Address,
    pool: Address,
    route: &'static str,
    amount_in: U256,
    amount_out_minimum: U256,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    if args.recent_blocks == 0 {
        bail!("--recent-blocks must be nonzero");
    }
    if !(1..=16).contains(&args.tx_concurrency) {
        bail!("--tx-concurrency must be between 1 and 16");
    }
    if args.top_wallets == 0 || args.latest_entries == 0 {
        bail!("ranking output limits must be nonzero");
    }
    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    if rpc.chain_id().await? != CHAIN_ID {
        bail!("RPC is not Robinhood Chain mainnet");
    }
    let latest = rpc.latest_block().await?;
    if args.launch_from_block > latest.l2_block_number {
        bail!("launch backfill begins after the latest block");
    }

    let mut pools = BTreeMap::<Address, PoolIdentity>::new();
    let mut launch_count = 0_u64;
    let mut first_launch_l2_block = None;
    let mut latest_launch = None;
    let mut cursor = args.launch_from_block;
    while cursor <= latest.l2_block_number {
        let end = cursor
            .saturating_add(LOG_BLOCK_CHUNK - 1)
            .min(latest.l2_block_number);
        for observed in rpc
            .token_launched_logs_for(ACTIVE_NOXA_LAUNCH_FACTORY, cursor, end)
            .await?
        {
            let launch = decode_token_launched(&observed.log)
                .context("active factory emitted a malformed TokenLaunched log")?;
            launch_count += 1;
            first_launch_l2_block = Some(
                first_launch_l2_block.map_or(observed.l2_block_number, |block: u64| {
                    block.min(observed.l2_block_number)
                }),
            );
            if latest_launch
                .as_ref()
                .is_none_or(|(block, _, _)| observed.l2_block_number >= *block)
            {
                latest_launch = Some((observed.l2_block_number, launch.token, launch.pool));
            }
            pools.insert(
                launch.pool,
                PoolIdentity {
                    token: launch.token,
                    pool: launch.pool,
                },
            );
        }
        if end == latest.l2_block_number {
            break;
        }
        cursor = end + 1;
    }

    let scan_from = args.scan_from_block.unwrap_or_else(|| {
        latest
            .l2_block_number
            .saturating_sub(args.recent_blocks.saturating_sub(1))
    });
    if scan_from > latest.l2_block_number {
        bail!("swap scan begins after the latest block");
    }
    let pool_addresses = pools.keys().copied().collect::<Vec<_>>();
    let mut swaps_by_tx = BTreeMap::<B256, Vec<(u64, PoolIdentity)>>::new();
    for pool_chunk in pool_addresses.chunks(POOL_CHUNK) {
        let mut from = scan_from;
        while from <= latest.l2_block_number {
            let to = from
                .saturating_add(LOG_BLOCK_CHUNK - 1)
                .min(latest.l2_block_number);
            for observed in rpc.v3_swap_logs(pool_chunk, from, to).await? {
                let identity = *pools
                    .get(&observed.log.address)
                    .context("swap log came from an unrequested pool")?;
                swaps_by_tx
                    .entry(observed.transaction_hash)
                    .or_default()
                    .push((observed.l2_block_number, identity));
            }
            if to == latest.l2_block_number {
                break;
            }
            from = to + 1;
        }
    }

    let tx_hashes = swaps_by_tx.keys().copied().collect::<Vec<_>>();
    let swapped_pools = swaps_by_tx
        .values()
        .flatten()
        .map(|(_, identity)| identity.pool)
        .collect::<BTreeSet<_>>();
    let transactions = stream::iter(tx_hashes.into_iter().map(|hash| {
        let rpc = rpc.clone();
        async move {
            let transaction = rpc
                .transaction_by_hash(hash)
                .await?
                .with_context(|| format!("pool swap transaction {hash} is missing"))?;
            Ok::<_, anyhow::Error>((hash, transaction))
        }
    }))
    .buffer_unordered(args.tx_concurrency)
    .collect::<Vec<_>>()
    .await;

    let mut wallets = BTreeMap::<Address, WalletStats>::new();
    let mut eligible_entries = Vec::new();
    for transaction_result in transactions {
        let (hash, transaction) = transaction_result?;
        let observations = swaps_by_tx
            .get(&hash)
            .context("transaction lost its pool observation")?;
        let normalized = normalized_route(&transaction);
        let mut seen_wallet_token = BTreeSet::new();
        for (block, identity) in observations {
            if !seen_wallet_token.insert(identity.token) {
                continue;
            }
            let stats = wallets.entry(transaction.from).or_default();
            stats.pool_swap_transactions += 1;
            stats.tokens.insert(identity.token);
            stats.latest_l2_block = stats.latest_l2_block.max(*block);

            let Some((intent, pool, route, normalized_value)) = normalized.as_ref() else {
                stats.other_or_unsupported += 1;
                continue;
            };
            if *pool != identity.pool || copy_token(intent) != Some(identity.token) {
                stats.other_or_unsupported += 1;
                continue;
            }
            stats.canonical_copy_route_transactions += 1;
            if intent.token_in == WETH {
                let observed = ObservedCopySwap {
                    tx_hash: hash,
                    chain_id: Some(CHAIN_ID),
                    from: transaction.from,
                    to: UNISWAP_V3_SWAP_ROUTER_02,
                    value: *normalized_value,
                    intent: intent.clone(),
                };
                let policy = WatchedWalletCopyPolicy::new(
                    HashSet::from([transaction.from]),
                    HashSet::from([identity.token]),
                    U256::from(100_000_000_000_000_u64),
                    U256::from(MAX_LEADER_ENTRY_AMOUNT),
                    1,
                )?;
                if matches!(
                    policy.evaluate_validated(&observed, None, 0),
                    Ok(CopyDecision::Entry { .. })
                ) {
                    stats.safe_entries += 1;
                    eligible_entries.push(EligibleEntry {
                        leader: transaction.from,
                        tx_hash: hash,
                        l2_block: *block,
                        token: identity.token,
                        pool: identity.pool,
                        route,
                        amount_in: intent.amount_in,
                        amount_out_minimum: intent.amount_out_minimum,
                    });
                } else {
                    stats.other_or_unsupported += 1;
                }
            } else {
                stats.exits += 1;
            }
        }
    }

    let mut wallet_ranking = wallets.into_iter().collect::<Vec<_>>();
    wallet_ranking.sort_by(|(address_a, a), (address_b, b)| {
        b.safe_entries
            .cmp(&a.safe_entries)
            .then_with(|| {
                b.canonical_copy_route_transactions
                    .cmp(&a.canonical_copy_route_transactions)
            })
            .then_with(|| b.latest_l2_block.cmp(&a.latest_l2_block))
            .then_with(|| address_a.cmp(address_b))
    });
    let wallet_count = wallet_ranking.len();
    wallet_ranking.truncate(args.top_wallets);
    eligible_entries.sort_by(|a, b| {
        b.l2_block
            .cmp(&a.l2_block)
            .then_with(|| b.tx_hash.cmp(&a.tx_hash))
    });
    let eligible_entry_count = eligible_entries.len();
    eligible_entries.truncate(args.latest_entries);

    println!(
        "{}",
        serde_json::to_string(&json!({
            "record_type": "active_noxa_recent_market_scan",
            "chain_id": CHAIN_ID,
            "factory": ACTIVE_NOXA_LAUNCH_FACTORY,
            "launch_from_l2_block": args.launch_from_block,
            "scan_from_l2_block": scan_from,
            "scan_to_l2_block": latest.l2_block_number,
            "active_factory_launch_events": launch_count,
            "unique_active_factory_pools": pools.len(),
            "first_launch_l2_block": first_launch_l2_block,
            "latest_launch": latest_launch.map(|(l2_block, token, pool)| json!({
                "l2_block": l2_block,
                "token": token,
                "pool": pool,
            })),
            "pool_swap_transactions": swaps_by_tx.len(),
            "pools_with_swaps": swapped_pools.len(),
            "wallets": wallet_count,
            "eligible_entry_count": eligible_entry_count,
            "latest_eligible_entries": eligible_entries,
            "wallet_ranking": wallet_ranking.into_iter().map(|(address, stats)| json!({
                "address": address,
                "stats": stats,
            })).collect::<Vec<_>>(),
            "rpc": rpc.metrics(),
        }))?
    );
    Ok(())
}

fn normalized_route(
    transaction: &hermes_feed::RobinhoodTransaction,
) -> Option<(V3ExactInputIntent, Address, &'static str, U256)> {
    if transaction.to == Some(UNISWAP_V3_SWAP_ROUTER_02) {
        let intent = decode_v3_exact_input_single(&transaction.input)?;
        let token = copy_token(&intent)?;
        let (token0, token1) = if token < WETH {
            (token, WETH)
        } else {
            (WETH, token)
        };
        let pool = hermes_feed::predict_v3_pool_address(
            hermes_feed::robinhood::UNISWAP_V3_FACTORY,
            token0,
            token1,
            hermes_feed::robinhood::NOXA_POOL_FEE,
            hermes_feed::robinhood::UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
        );
        Some((intent, pool, "direct_swap_router_02", transaction.value))
    } else if transaction.to == Some(ROBINHOOD_SWAP_AGGREGATOR) {
        let swap =
            normalize_aggregator_copy_swap(&transaction.input, transaction.value, transaction.from)
                .ok()?;
        Some((swap.intent, swap.pool, "robinhood_aggregator", U256::ZERO))
    } else {
        None
    }
}

fn copy_token(intent: &V3ExactInputIntent) -> Option<Address> {
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
