use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use alloy_primitives::{B256, U256};
use anyhow::{Context, Result};
use clap::Parser;
use hermes_feed::V3ReceiptPaperQuote;
use hermes_feed::feed::BroadcastMessage;
use hermes_feed::launchpad_adapter::LaunchpadId;
use hermes_feed::paper_observer::{
    PaperExpectedPins, PaperFeedRuntime, PaperLaunchpadObserver, PaperObservedStartupSnapshot,
    PaperPlanPolicy,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Unified paper-only launchpad observer for Nitro feed frames"
)]
struct Cli {
    /// Reviewed protocol-owned expected pins. Never use an observed snapshot here.
    #[arg(long)]
    expected_pins: PathBuf,
    /// Independently collected startup runtime observations.
    #[arg(long)]
    observed_startup_snapshot: PathBuf,
    /// JSONL of direct Nitro BroadcastMessage objects or probe records containing `payload`.
    #[arg(long, default_value = "-")]
    input: PathBuf,

    /// Existing observer JSONL to join with independently collected evidence.
    /// This finalize-only mode does not decode feed frames again.
    #[arg(long)]
    observer_output_input: Option<PathBuf>,

    /// Independently collected receipt/event evidence JSONL, joined after feed EOF.
    #[arg(long)]
    reconciliation_input: Option<PathBuf>,

    #[arg(long, default_value_t = 1_000_000_000_000_000_u64)]
    paper_max_input_wei: u64,
    #[arg(long, default_value_t = 100)]
    paper_slippage_bps: u16,
    #[arg(long, default_value_t = 2_000)]
    paper_take_profit_bps: u16,
    #[arg(long, default_value_t = 1_000)]
    paper_stop_loss_bps: u16,
    #[arg(long, default_value_t = 300)]
    paper_max_hold_seconds: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReconciliationEvidence {
    tx_hash: B256,
    launchpad: LaunchpadId,
    receipt_status: bool,
    protocol_event_match: bool,
    observed_unix_ns: u64,
}

#[derive(Debug, Serialize)]
struct ReconciliationMetrics {
    record_type: &'static str,
    observed_candidates: usize,
    evidence_records: usize,
    confirmed: usize,
    false_positives: usize,
    missed_transactions: usize,
    unreconciled: usize,
    reconciliation_latency_p50_ns: Option<u64>,
    reconciliation_latency_p95_ns: Option<u64>,
    reconciliation_latency_p99_ns: Option<u64>,
}

#[derive(Debug, Default)]
struct ReconciliationRecords {
    evidence: Vec<ReconciliationEvidence>,
    v3_quotes: Vec<V3ReceiptPaperQuote>,
}

#[derive(Debug, Default)]
struct ObservedOutputCandidates {
    received_unix_ns: HashMap<(B256, LaunchpadId), u64>,
    feed_sequences: HashMap<(B256, LaunchpadId), u64>,
}

#[derive(Debug, Serialize)]
struct FinalizedV3PaperPlan {
    record_type: &'static str,
    tx_hash: B256,
    launchpad: LaunchpadId,
    feed_sequence: u64,
    status: &'static str,
    amount_in: U256,
    expected_output: U256,
    min_receive: U256,
    quote_source: String,
    quote_state_version: hermes_feed::V3QuoteStateVersion,
    exit_full_position: bool,
    exit_expected_output: U256,
    exit_min_receive: U256,
    leader_amounts_reused: bool,
    execution_eligible: bool,
    execution_blocker: String,
    broadcast: bool,
}

fn main() -> Result<()> {
    let args = Cli::parse();
    if args.expected_pins.canonicalize()? == args.observed_startup_snapshot.canonicalize()? {
        anyhow::bail!("expected pins and observed startup snapshot must be separate files");
    }
    let expected: PaperExpectedPins = serde_json::from_reader(BufReader::new(
        File::open(&args.expected_pins)
            .with_context(|| format!("open expected pins {}", args.expected_pins.display()))?,
    ))
    .with_context(|| format!("decode expected pins {}", args.expected_pins.display()))?;
    let observed: PaperObservedStartupSnapshot = serde_json::from_reader(BufReader::new(
        File::open(&args.observed_startup_snapshot).with_context(|| {
            format!(
                "open observed startup snapshot {}",
                args.observed_startup_snapshot.display()
            )
        })?,
    ))
    .with_context(|| {
        format!(
            "decode observed startup snapshot {}",
            args.observed_startup_snapshot.display()
        )
    })?;
    if args
        .reconciliation_input
        .as_ref()
        .is_some_and(|path| path == &args.input)
    {
        anyhow::bail!("feed input and reconciliation evidence must be independent files");
    }
    let observer = PaperLaunchpadObserver::from_startup_snapshots(expected, observed)?;
    let plan_policy = PaperPlanPolicy {
        max_input_wei: U256::from(args.paper_max_input_wei),
        slippage_bps: args.paper_slippage_bps,
        take_profit_bps: args.paper_take_profit_bps,
        stop_loss_bps: args.paper_stop_loss_bps,
        max_hold_seconds: args.paper_max_hold_seconds,
    };
    if let Some(observer_path) = args.observer_output_input.as_ref() {
        let reconciliation_path = args
            .reconciliation_input
            .as_ref()
            .context("--observer-output-input requires --reconciliation-input")?;
        if observer_path == reconciliation_path {
            anyhow::bail!("observer output and reconciliation evidence must be independent files");
        }
        let observed_candidates = read_observed_output_candidates(observer_path)?;
        let records = read_reconciliation_records(reconciliation_path)?;
        println!(
            "{}",
            serde_json::to_string(&reconciliation_metrics(
                &observed_candidates.received_unix_ns,
                &records.evidence
            ))?
        );
        for plan in finalized_v3_plans(
            &observed_candidates.feed_sequences,
            &records.evidence,
            records.v3_quotes,
            plan_policy,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        return Ok(());
    }
    let mut runtime = PaperFeedRuntime::with_plan_policy(observer, plan_policy)?;
    let mut observed_candidates = HashMap::new();
    let mut observed_sequences = HashMap::new();
    println!(
        "{}",
        serde_json::to_string(&json!({
            "record_type": "launchpad_paper_capabilities",
            "capabilities": runtime.capabilities(),
            "broadcast": false,
            "signing": false,
            "candidate_time_rpc": false,
        }))?
    );

    let input: Box<dyn BufRead> = if args.input == Path::new("-") {
        Box::new(BufReader::new(std::io::stdin()))
    } else {
        Box::new(BufReader::new(File::open(&args.input).with_context(
            || format!("open input {}", args.input.display()),
        )?))
    };
    for (index, line) in input.lines().enumerate() {
        let line = line.with_context(|| format!("read input line {}", index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line)
            .with_context(|| format!("parse input line {}", index + 1))?;
        let feed: BroadcastMessage = match value.get("payload").and_then(Value::as_str) {
            Some(payload) => serde_json::from_str(payload)
                .with_context(|| format!("decode recorded payload at line {}", index + 1))?,
            None => serde_json::from_value(value)
                .with_context(|| format!("decode Nitro frame at line {}", index + 1))?,
        };
        let report = runtime.decode(&feed)?;
        for observation in &report.observations {
            let key = (observation.tx_hash, observation.launchpad);
            observed_candidates.insert(
                key,
                observation.observer_received_unix_ns.unwrap_or_default(),
            );
            observed_sequences.insert(key, observation.feed_sequence.unwrap_or_default());
        }
        println!(
            "{}",
            serde_json::to_string(&json!({
                "record_type": "launchpad_paper_frame",
                "report": report,
            }))?
        );
    }
    if let Some(path) = args.reconciliation_input {
        let records = read_reconciliation_records(&path)?;
        println!(
            "{}",
            serde_json::to_string(&reconciliation_metrics(
                &observed_candidates,
                &records.evidence
            ))?
        );
        for plan in finalized_v3_plans(
            &observed_sequences,
            &records.evidence,
            records.v3_quotes,
            plan_policy,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
    }
    Ok(())
}

fn read_observed_output_candidates(path: &Path) -> Result<ObservedOutputCandidates> {
    let input = BufReader::new(
        File::open(path).with_context(|| format!("open observer output {}", path.display()))?,
    );
    let mut received: HashMap<(B256, LaunchpadId), u64> = HashMap::new();
    let mut sequences: HashMap<(B256, LaunchpadId), u64> = HashMap::new();
    for (index, line) in input.lines().enumerate() {
        let line = line.with_context(|| format!("read observer line {}", index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line)
            .with_context(|| format!("decode observer line {}", index + 1))?;
        if value.get("record_type").and_then(Value::as_str) != Some("launchpad_paper_frame") {
            continue;
        }
        let observations = value
            .pointer("/report/observations")
            .and_then(Value::as_array)
            .context("launchpad paper frame has no observations array")?;
        for observation in observations {
            let tx_hash: B256 = serde_json::from_value(
                observation
                    .get("tx_hash")
                    .cloned()
                    .context("observation has no tx_hash")?,
            )?;
            let launchpad: LaunchpadId = serde_json::from_value(
                observation
                    .get("launchpad")
                    .cloned()
                    .context("observation has no launchpad")?,
            )?;
            let observer_received_unix_ns = observation
                .get("observer_received_unix_ns")
                .and_then(Value::as_u64)
                .context("observation has no receive timestamp")?;
            let feed_sequence = observation
                .get("feed_sequence")
                .and_then(Value::as_u64)
                .context("observation has no feed sequence")?;
            let key = (tx_hash, launchpad);
            received
                .entry(key)
                .and_modify(|existing| {
                    *existing = (*existing).min(observer_received_unix_ns);
                })
                .or_insert(observer_received_unix_ns);
            sequences.entry(key).or_insert(feed_sequence);
        }
    }
    Ok(ObservedOutputCandidates {
        received_unix_ns: received,
        feed_sequences: sequences,
    })
}

fn read_reconciliation_records(path: &Path) -> Result<ReconciliationRecords> {
    let mut records = ReconciliationRecords::default();
    let input = BufReader::new(
        File::open(path)
            .with_context(|| format!("open reconciliation evidence {}", path.display()))?,
    );
    for (index, line) in input.lines().enumerate() {
        let line = line.with_context(|| {
            format!(
                "read reconciliation evidence line {} from {}",
                index + 1,
                path.display()
            )
        })?;
        if !line.trim().is_empty() {
            let value: Value = serde_json::from_str(&line).with_context(|| {
                format!(
                    "decode reconciliation line {} from {}",
                    index + 1,
                    path.display()
                )
            })?;
            match value.get("record_type").and_then(Value::as_str) {
                Some("launchpad_v3_paper_quote") => {
                    records
                        .v3_quotes
                        .push(serde_json::from_value(value).with_context(|| {
                            format!("decode V3 quote line {} from {}", index + 1, path.display())
                        })?)
                }
                Some(other) => anyhow::bail!(
                    "unknown reconciliation record type {other} at line {}",
                    index + 1
                ),
                None => records
                    .evidence
                    .push(serde_json::from_value(value).with_context(|| {
                        format!("decode evidence line {} from {}", index + 1, path.display())
                    })?),
            }
        }
    }
    Ok(records)
}

fn finalized_v3_plans(
    observed_sequences: &HashMap<(B256, LaunchpadId), u64>,
    evidence: &[ReconciliationEvidence],
    quotes: Vec<V3ReceiptPaperQuote>,
    policy: PaperPlanPolicy,
) -> Result<Vec<FinalizedV3PaperPlan>> {
    let evidence = evidence
        .iter()
        .map(|record| ((record.tx_hash, record.launchpad), record))
        .collect::<HashMap<_, _>>();
    let mut seen = HashMap::new();
    let mut plans = Vec::new();
    for quote in quotes {
        let key = (quote.tx_hash, quote.launchpad);
        if seen.insert(key, ()).is_some() {
            anyhow::bail!("duplicate V3 paper quote for {key:?}");
        }
        let Some(feed_sequence) = observed_sequences.get(&key).copied() else {
            continue;
        };
        let confirmed = evidence
            .get(&key)
            .is_some_and(|record| record.receipt_status && record.protocol_event_match);
        if !confirmed
            || quote.record_type != "launchpad_v3_paper_quote"
            || quote.entry.amount_in == U256::ZERO
            || quote.entry.amount_in > policy.max_input_wei
            || quote.entry.slippage_bps != policy.slippage_bps
            || quote.entry.expected_output == U256::ZERO
            || quote.entry.min_receive == U256::ZERO
            || quote.entry.min_receive > quote.entry.expected_output
            || quote.full_position_exit.amount_in != quote.entry.expected_output
            || quote.full_position_exit.expected_output == U256::ZERO
            || quote.full_position_exit.min_receive == U256::ZERO
            || quote.full_position_exit.min_receive > quote.full_position_exit.expected_output
            || quote.broadcast
            || quote.execution_eligible
            || quote.state_version.chain_id != hermes_feed::robinhood::CHAIN_ID
            || quote.state_version.l2_block_number != quote.l2_block_number
        {
            anyhow::bail!("unsafe or inconsistent V3 quote evidence for {key:?}");
        }
        plans.push(FinalizedV3PaperPlan {
            record_type: "launchpad_paper_finalized_plan",
            tx_hash: quote.tx_hash,
            launchpad: quote.launchpad,
            feed_sequence,
            status: "quoted_restriction_gated",
            amount_in: quote.entry.amount_in,
            expected_output: quote.entry.expected_output,
            min_receive: quote.entry.min_receive,
            quote_source: quote.quote_source,
            quote_state_version: quote.state_version,
            exit_full_position: true,
            exit_expected_output: quote.full_position_exit.expected_output,
            exit_min_receive: quote.full_position_exit.min_receive,
            leader_amounts_reused: false,
            execution_eligible: false,
            execution_blocker: quote.execution_blocker,
            broadcast: false,
        });
    }
    Ok(plans)
}

fn reconciliation_metrics(
    observed: &HashMap<(B256, LaunchpadId), u64>,
    evidence: &[ReconciliationEvidence],
) -> ReconciliationMetrics {
    let indexed = evidence
        .iter()
        .map(|record| ((record.tx_hash, record.launchpad), record))
        .collect::<HashMap<_, _>>();
    let confirmed_evidence =
        |record: &&ReconciliationEvidence| record.receipt_status && record.protocol_event_match;
    let confirmed = observed
        .keys()
        .filter(|key| indexed.get(key).is_some_and(confirmed_evidence))
        .count();
    let false_positives = observed
        .keys()
        .filter(|key| {
            indexed
                .get(key)
                .is_some_and(|record| !confirmed_evidence(record))
        })
        .count();
    let unreconciled = observed
        .keys()
        .filter(|key| !indexed.contains_key(key))
        .count();
    let missed_transactions = indexed
        .iter()
        .filter(|(key, record)| confirmed_evidence(record) && !observed.contains_key(key))
        .count();
    let mut latencies = indexed
        .iter()
        .filter_map(|(key, record)| {
            if confirmed_evidence(record) {
                observed
                    .get(key)
                    .and_then(|received| record.observed_unix_ns.checked_sub(*received))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    latencies.sort_unstable();
    ReconciliationMetrics {
        record_type: "launchpad_paper_reconciliation_metrics",
        observed_candidates: observed.len(),
        evidence_records: evidence.len(),
        confirmed,
        false_positives,
        missed_transactions,
        unreconciled,
        reconciliation_latency_p50_ns: percentile(&latencies, 50),
        reconciliation_latency_p95_ns: percentile(&latencies, 95),
        reconciliation_latency_p99_ns: percentile(&latencies, 99),
    }
}

fn percentile(values: &[u64], percentile: usize) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let index = (values.len() * percentile).div_ceil(100).saturating_sub(1);
    values.get(index).copied()
}

#[cfg(test)]
mod tests {
    use alloy_primitives::Address;
    use hermes_feed::{V3PaperSwapQuote, V3Quote, V3QuoteStateVersion, V3ReceiptMarketEvidence};

    use super::*;

    #[test]
    fn reconciliation_metrics_separate_confirmed_false_positive_missed_and_unknown() {
        let confirmed_key = (B256::with_last_byte(1), LaunchpadId::Bow);
        let false_positive_key = (B256::with_last_byte(2), LaunchpadId::Clanker);
        let unreconciled_key = (B256::with_last_byte(3), LaunchpadId::Pons);
        let missed_key = (B256::with_last_byte(4), LaunchpadId::HoodFun);
        let observed = HashMap::from([
            (confirmed_key, 100),
            (false_positive_key, 200),
            (unreconciled_key, 300),
        ]);
        let evidence = [
            ReconciliationEvidence {
                tx_hash: confirmed_key.0,
                launchpad: confirmed_key.1,
                receipt_status: true,
                protocol_event_match: true,
                observed_unix_ns: 150,
            },
            ReconciliationEvidence {
                tx_hash: false_positive_key.0,
                launchpad: false_positive_key.1,
                receipt_status: true,
                protocol_event_match: false,
                observed_unix_ns: 250,
            },
            ReconciliationEvidence {
                tx_hash: missed_key.0,
                launchpad: missed_key.1,
                receipt_status: true,
                protocol_event_match: true,
                observed_unix_ns: 400,
            },
        ];

        let metrics = reconciliation_metrics(&observed, &evidence);
        assert_eq!(metrics.confirmed, 1);
        assert_eq!(metrics.false_positives, 1);
        assert_eq!(metrics.missed_transactions, 1);
        assert_eq!(metrics.unreconciled, 1);
        assert_eq!(metrics.reconciliation_latency_p50_ns, Some(50));
        assert_eq!(metrics.reconciliation_latency_p95_ns, Some(50));
        assert_eq!(metrics.reconciliation_latency_p99_ns, Some(50));
    }

    fn swap_quote(
        token_in: Address,
        token_out: Address,
        amount_in: u64,
        amount_out: u64,
    ) -> V3PaperSwapQuote {
        V3PaperSwapQuote {
            amount_in: U256::from(amount_in),
            expected_output: U256::from(amount_out),
            min_receive: U256::from(amount_out * 99 / 100),
            slippage_bps: 100,
            state_after: V3Quote {
                token_in,
                token_out,
                amount_in_requested: U256::from(amount_in),
                amount_in_consumed: U256::from(amount_in),
                amount_out: U256::from(amount_out),
                sqrt_price_x96_after: U256::from(1_u128 << 96),
                tick_after: 0,
                liquidity_after: 1_000_000,
                initialized_ticks_crossed: 0,
                steps: 1,
            },
        }
    }

    fn quote_fixture() -> V3ReceiptPaperQuote {
        let weth = hermes_feed::robinhood::WETH;
        let token = Address::with_last_byte(0xee);
        let tx_hash = B256::with_last_byte(0xaa);
        V3ReceiptPaperQuote {
            record_type: "launchpad_v3_paper_quote".into(),
            tx_hash,
            launchpad: LaunchpadId::Bow,
            l2_block_number: 10,
            state_version: V3QuoteStateVersion {
                chain_id: hermes_feed::robinhood::CHAIN_ID,
                block_hash: B256::with_last_byte(0xbb),
                l2_block_number: 10,
                transaction_index: 2,
                terminal_log_index: 12,
            },
            quote_source: "confirmed_receipt_end_v3_state".into(),
            sizing_source: "independent_fixed_tiny_weth_policy".into(),
            market: V3ReceiptMarketEvidence {
                token,
                pool: Address::with_last_byte(0xdd),
                quote_asset: weth,
                fee: 10_000,
                tick_spacing: 200,
                launch_log_index: 12,
                pool_created_log_index: 1,
                initialize_log_index: 2,
                last_state_log_index: 6,
                mint_count: 1,
                swap_count: 0,
                restriction_end_block: None,
            },
            entry: swap_quote(weth, token, 1_000, 2_000),
            full_position_exit: swap_quote(token, weth, 2_000, 980),
            simulated_round_trip_return_bps: U256::from(9_800),
            execution_eligible: false,
            execution_blocker: "paper_only_token_restriction_and_runtime_checks_not_satisfied"
                .into(),
            broadcast: false,
        }
    }

    #[test]
    fn confirmed_quote_becomes_non_broadcast_finalized_plan() {
        let quote = quote_fixture();
        let key = (quote.tx_hash, quote.launchpad);
        let plans = finalized_v3_plans(
            &HashMap::from([(key, 42)]),
            &[ReconciliationEvidence {
                tx_hash: key.0,
                launchpad: key.1,
                receipt_status: true,
                protocol_event_match: true,
                observed_unix_ns: 100,
            }],
            vec![quote],
            PaperPlanPolicy {
                max_input_wei: U256::from(1_000),
                slippage_bps: 100,
                ..PaperPlanPolicy::default()
            },
        )
        .unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].expected_output, U256::from(2_000));
        assert_eq!(plans[0].min_receive, U256::from(1_980));
        assert_eq!(plans[0].exit_expected_output, U256::from(980));
        assert!(!plans[0].execution_eligible);
        assert!(!plans[0].broadcast);
        assert!(!plans[0].leader_amounts_reused);
    }

    #[test]
    fn unconfirmed_or_broadcast_quote_cannot_finalize() {
        let mut quote = quote_fixture();
        let key = (quote.tx_hash, quote.launchpad);
        quote.broadcast = true;
        assert!(
            finalized_v3_plans(
                &HashMap::from([(key, 42)]),
                &[ReconciliationEvidence {
                    tx_hash: key.0,
                    launchpad: key.1,
                    receipt_status: true,
                    protocol_event_match: true,
                    observed_unix_ns: 100,
                }],
                vec![quote],
                PaperPlanPolicy {
                    max_input_wei: U256::from(1_000),
                    slippage_bps: 100,
                    ..PaperPlanPolicy::default()
                },
            )
            .is_err()
        );
    }
}
