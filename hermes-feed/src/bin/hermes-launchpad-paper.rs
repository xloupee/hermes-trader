use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use alloy_primitives::{B256, U256};
use anyhow::{Context, Result};
use clap::Parser;
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
    let mut runtime = PaperFeedRuntime::with_plan_policy(
        observer,
        PaperPlanPolicy {
            max_input_wei: U256::from(args.paper_max_input_wei),
            slippage_bps: args.paper_slippage_bps,
            take_profit_bps: args.paper_take_profit_bps,
            stop_loss_bps: args.paper_stop_loss_bps,
            max_hold_seconds: args.paper_max_hold_seconds,
        },
    )?;
    let mut observed_candidates = HashMap::new();
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
            observed_candidates.insert(
                (observation.tx_hash, observation.launchpad),
                observation.observer_received_unix_ns.unwrap_or_default(),
            );
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
        let evidence = read_reconciliation_evidence(&path)?;
        println!(
            "{}",
            serde_json::to_string(&reconciliation_metrics(&observed_candidates, &evidence))?
        );
    }
    Ok(())
}

fn read_reconciliation_evidence(path: &Path) -> Result<Vec<ReconciliationEvidence>> {
    let mut records = Vec::new();
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
            records.push(serde_json::from_str(&line).with_context(|| {
                format!(
                    "decode reconciliation evidence line {} from {}",
                    index + 1,
                    path.display()
                )
            })?);
        }
    }
    Ok(records)
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
    }
}
