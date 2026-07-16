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
use hermes_feed::{
    BankrDopplerExpectedProfile, BankrDopplerReceiptPaperQuote, ClankerReceiptPaperQuote,
    HoodExpectedProfile, HoodMigrationEvidence, HoodReceiptPaperQuote, PonsReceiptPaperQuote,
    V3ReceiptPaperQuote, bankr_hook_fee_ppm, quote_hood_curve_buy, quote_hood_curve_sell,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReconciliationEvidence {
    tx_hash: B256,
    launchpad: LaunchpadId,
    receipt_status: bool,
    protocol_event_match: bool,
    observed_unix_ns: u64,
    #[serde(default)]
    pons_generation: Option<hermes_feed::PonsGeneration>,
    #[serde(default)]
    protocol_blocker: Option<String>,
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
    clanker_quotes: Vec<ClankerReceiptPaperQuote>,
    bankr_quotes: Vec<BankrDopplerReceiptPaperQuote>,
    pons_quotes: Vec<PonsReceiptPaperQuote>,
    hood_quotes: Vec<HoodReceiptPaperQuote>,
    hood_migrations: Vec<HoodMigrationEvidence>,
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
    quote_state_version: Value,
    exit_full_position: bool,
    exit_expected_output: U256,
    exit_min_receive: U256,
    simulated_round_trip_return_bps: U256,
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
    let pons_profile = expected.pons_v3.expected_profile()?;
    let hood_profile = expected
        .hood_curve
        .as_ref()
        .context("complete reviewed Hood profile is required")?
        .clone();
    hood_profile.validate()?;
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
        validate_hood_migration_records(&records.evidence, &records.hood_migrations)?;
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
        for plan in finalized_clanker_plans(
            &observed_candidates.feed_sequences,
            &records.evidence,
            records.clanker_quotes,
            plan_policy,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_bankr_plans(
            &observed_candidates.feed_sequences,
            &records.evidence,
            records.bankr_quotes,
            plan_policy,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_pons_plans(
            &observed_candidates.feed_sequences,
            &records.evidence,
            records.pons_quotes,
            plan_policy,
            pons_profile,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_hood_plans(
            &observed_candidates.feed_sequences,
            &records.evidence,
            records.hood_quotes,
            plan_policy,
            &hood_profile,
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
        validate_hood_migration_records(&records.evidence, &records.hood_migrations)?;
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
        for plan in finalized_clanker_plans(
            &observed_sequences,
            &records.evidence,
            records.clanker_quotes,
            plan_policy,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_bankr_plans(
            &observed_sequences,
            &records.evidence,
            records.bankr_quotes,
            plan_policy,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_pons_plans(
            &observed_sequences,
            &records.evidence,
            records.pons_quotes,
            plan_policy,
            pons_profile,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_hood_plans(
            &observed_sequences,
            &records.evidence,
            records.hood_quotes,
            plan_policy,
            &hood_profile,
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
                Some("launchpad_clanker_v4_paper_quote") => {
                    records
                        .clanker_quotes
                        .push(serde_json::from_value(value).with_context(|| {
                            format!(
                                "decode Clanker V4 quote line {} from {}",
                                index + 1,
                                path.display()
                            )
                        })?)
                }
                Some("launchpad_bankr_doppler_v4_paper_quote") => {
                    records
                        .bankr_quotes
                        .push(serde_json::from_value(value).with_context(|| {
                            format!(
                                "decode Bankr/Doppler V4 quote line {} from {}",
                                index + 1,
                                path.display()
                            )
                        })?)
                }
                Some("launchpad_pons_v3_paper_quote") => {
                    records
                        .pons_quotes
                        .push(serde_json::from_value(value).with_context(|| {
                            format!(
                                "decode Pons V3 quote line {} from {}",
                                index + 1,
                                path.display()
                            )
                        })?)
                }
                Some("launchpad_hood_curve_paper_quote") => {
                    records
                        .hood_quotes
                        .push(serde_json::from_value(value).with_context(|| {
                            format!(
                                "decode Hood quote line {} from {}",
                                index + 1,
                                path.display()
                            )
                        })?)
                }
                Some("launchpad_hood_migration_evidence") => {
                    records
                        .hood_migrations
                        .push(serde_json::from_value(value).with_context(|| {
                            format!(
                                "decode Hood migration line {} from {}",
                                index + 1,
                                path.display()
                            )
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

fn validate_hood_migration_records(
    evidence: &[ReconciliationEvidence],
    migrations: &[HoodMigrationEvidence],
) -> Result<()> {
    let evidence = evidence
        .iter()
        .map(|record| ((record.tx_hash, record.launchpad), record))
        .collect::<HashMap<_, _>>();
    let mut seen = HashMap::new();
    for migration in migrations {
        let key = (migration.tx_hash, migration.launchpad);
        let confirmed = evidence.get(&key).is_some_and(|record| {
            record.receipt_status
                && record.protocol_event_match
                && record.protocol_blocker.as_deref()
                    == Some("hood_migration_topology_verified_v3_quote_unavailable")
        });
        let expected_blocker = if migration.declared_and_actual_liquidity_match {
            "migration_topology_only_missing_independent_v3_state_quote"
        } else {
            "declared_actual_liquidity_mismatch_and_missing_independent_v3_state_quote"
        };
        let liquidity_match = migration.actual_eth_liquidity == migration.declared_eth_liquidity
            && migration.actual_token_liquidity == migration.declared_token_liquidity;
        if seen.insert(key, ()).is_some()
            || !confirmed
            || migration.record_type != "launchpad_hood_migration_evidence"
            || migration.launchpad != LaunchpadId::HoodFun
            || migration.token == alloy_primitives::Address::ZERO
            || migration.pool == alloy_primitives::Address::ZERO
            || migration.leader == alloy_primitives::Address::ZERO
            || migration.trader == alloy_primitives::Address::ZERO
            || migration.token_id == U256::ZERO
            || migration.raised_eth == U256::ZERO
            || migration.declared_eth_liquidity == U256::ZERO
            || migration.declared_token_liquidity == U256::ZERO
            || migration.actual_eth_liquidity == U256::ZERO
            || migration.actual_token_liquidity == U256::ZERO
            || migration.declared_and_actual_liquidity_match != liquidity_match
            || !migration.expected_profile_validated
            || !migration.receipt_topology_verified
            || migration.pool_state_reconciled
            || migration.v3_quote_available
            || migration.execution_eligible
            || migration.execution_blocker != expected_blocker
            || migration.broadcast
        {
            anyhow::bail!("unsafe or inconsistent Hood migration evidence for {key:?}");
        }
    }
    for record in evidence.values().filter(|record| {
        record.launchpad == LaunchpadId::HoodFun
            && record.protocol_blocker.as_deref()
                == Some("hood_migration_topology_verified_v3_quote_unavailable")
    }) {
        let key = (record.tx_hash, record.launchpad);
        if migrations
            .iter()
            .filter(|migration| (migration.tx_hash, migration.launchpad) == key)
            .count()
            != 1
        {
            anyhow::bail!("Hood migration confirmation has no unique topology record for {key:?}");
        }
    }
    Ok(())
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
            quote_state_version: serde_json::to_value(quote.state_version)?,
            exit_full_position: true,
            exit_expected_output: quote.full_position_exit.expected_output,
            exit_min_receive: quote.full_position_exit.min_receive,
            simulated_round_trip_return_bps: quote.simulated_round_trip_return_bps,
            leader_amounts_reused: false,
            execution_eligible: false,
            execution_blocker: quote.execution_blocker,
            broadcast: false,
        });
    }
    Ok(plans)
}

fn finalized_clanker_plans(
    observed_sequences: &HashMap<(B256, LaunchpadId), u64>,
    evidence: &[ReconciliationEvidence],
    quotes: Vec<ClankerReceiptPaperQuote>,
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
            anyhow::bail!("duplicate Clanker V4 paper quote for {key:?}");
        }
        let Some(feed_sequence) = observed_sequences.get(&key).copied() else {
            continue;
        };
        let confirmed = evidence
            .get(&key)
            .is_some_and(|record| record.receipt_status && record.protocol_event_match);
        if !confirmed
            || quote.record_type != "launchpad_clanker_v4_paper_quote"
            || quote.launchpad != LaunchpadId::Clanker
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
            || quote.state_version.first_eligible_quote_timestamp
                <= quote.state_version.receipt_timestamp
        {
            anyhow::bail!("unsafe or inconsistent Clanker V4 quote evidence for {key:?}");
        }
        plans.push(FinalizedV3PaperPlan {
            record_type: "launchpad_paper_finalized_plan",
            tx_hash: quote.tx_hash,
            launchpad: quote.launchpad,
            feed_sequence,
            status: "quoted_execution_gated",
            amount_in: quote.entry.amount_in,
            expected_output: quote.entry.expected_output,
            min_receive: quote.entry.min_receive,
            quote_source: quote.quote_source,
            quote_state_version: serde_json::to_value(quote.state_version)?,
            exit_full_position: true,
            exit_expected_output: quote.full_position_exit.expected_output,
            exit_min_receive: quote.full_position_exit.min_receive,
            simulated_round_trip_return_bps: quote.simulated_round_trip_return_bps,
            leader_amounts_reused: false,
            execution_eligible: false,
            execution_blocker: quote.execution_blocker,
            broadcast: false,
        });
    }
    Ok(plans)
}

fn finalized_bankr_plans(
    observed_sequences: &HashMap<(B256, LaunchpadId), u64>,
    evidence: &[ReconciliationEvidence],
    quotes: Vec<BankrDopplerReceiptPaperQuote>,
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
            anyhow::bail!("duplicate Bankr/Doppler V4 paper quote for {key:?}");
        }
        let Some(feed_sequence) = observed_sequences.get(&key).copied() else {
            continue;
        };
        let confirmed = evidence
            .get(&key)
            .is_some_and(|record| record.receipt_status && record.protocol_event_match);
        if !confirmed
            || quote.record_type != "launchpad_bankr_doppler_v4_paper_quote"
            || quote.launchpad != LaunchpadId::BankrDoppler
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
            || quote.state_version.first_eligible_quote_timestamp
                <= quote.state_version.receipt_timestamp
            || !bankr_quote_arithmetic_is_consistent(&quote, policy)
        {
            anyhow::bail!("unsafe or inconsistent Bankr/Doppler V4 quote evidence for {key:?}");
        }
        plans.push(FinalizedV3PaperPlan {
            record_type: "launchpad_paper_finalized_plan",
            tx_hash: quote.tx_hash,
            launchpad: quote.launchpad,
            feed_sequence,
            status: "quoted_execution_gated",
            amount_in: quote.entry.amount_in,
            expected_output: quote.entry.expected_output,
            min_receive: quote.entry.min_receive,
            quote_source: quote.quote_source,
            quote_state_version: serde_json::to_value(quote.state_version)?,
            exit_full_position: true,
            exit_expected_output: quote.full_position_exit.expected_output,
            exit_min_receive: quote.full_position_exit.min_receive,
            simulated_round_trip_return_bps: quote.simulated_round_trip_return_bps,
            leader_amounts_reused: false,
            execution_eligible: false,
            execution_blocker: quote.execution_blocker,
            broadcast: false,
        });
    }
    Ok(plans)
}

fn finalized_pons_plans(
    observed_sequences: &HashMap<(B256, LaunchpadId), u64>,
    evidence: &[ReconciliationEvidence],
    quotes: Vec<PonsReceiptPaperQuote>,
    policy: PaperPlanPolicy,
    expected_profile: hermes_feed::PonsExpectedProfile,
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
            anyhow::bail!("duplicate Pons V3 paper quote for {key:?}");
        }
        let Some(feed_sequence) = observed_sequences.get(&key).copied() else {
            continue;
        };
        let confirmed = evidence.get(&key).is_some_and(|record| {
            record.receipt_status
                && record.protocol_event_match
                && record.pons_generation == Some(hermes_feed::PonsGeneration::Current)
                && record.protocol_blocker.is_none()
        });
        if !confirmed
            || quote.record_type != "launchpad_pons_v3_paper_quote"
            || quote.launchpad != LaunchpadId::Pons
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
            || !pons_quote_arithmetic_is_consistent(&quote, policy, expected_profile)
        {
            anyhow::bail!("unsafe or inconsistent Pons V3 quote evidence for {key:?}");
        }
        plans.push(FinalizedV3PaperPlan {
            record_type: "launchpad_paper_finalized_plan",
            tx_hash: quote.tx_hash,
            launchpad: quote.launchpad,
            feed_sequence,
            status: "quoted_execution_gated",
            amount_in: quote.entry.amount_in,
            expected_output: quote.entry.expected_output,
            min_receive: quote.entry.min_receive,
            quote_source: quote.quote_source,
            quote_state_version: serde_json::to_value(quote.state_version)?,
            exit_full_position: true,
            exit_expected_output: quote.full_position_exit.expected_output,
            exit_min_receive: quote.full_position_exit.min_receive,
            simulated_round_trip_return_bps: quote.simulated_round_trip_return_bps,
            leader_amounts_reused: false,
            execution_eligible: false,
            execution_blocker: quote.execution_blocker,
            broadcast: false,
        });
    }
    Ok(plans)
}

fn finalized_hood_plans(
    observed_sequences: &HashMap<(B256, LaunchpadId), u64>,
    evidence: &[ReconciliationEvidence],
    quotes: Vec<HoodReceiptPaperQuote>,
    policy: PaperPlanPolicy,
    expected_profile: &HoodExpectedProfile,
) -> Result<Vec<FinalizedV3PaperPlan>> {
    expected_profile.validate()?;
    let evidence = evidence
        .iter()
        .map(|record| ((record.tx_hash, record.launchpad), record))
        .collect::<HashMap<_, _>>();
    let mut seen = HashMap::new();
    let mut plans = Vec::new();
    for quote in quotes {
        let key = (quote.tx_hash, quote.launchpad);
        if seen.insert(key, ()).is_some() {
            anyhow::bail!("duplicate Hood curve paper quote for {key:?}");
        }
        let Some(feed_sequence) = observed_sequences.get(&key).copied() else {
            continue;
        };
        let confirmed = evidence.get(&key).is_some_and(|record| {
            record.receipt_status
                && record.protocol_event_match
                && record.protocol_blocker.is_none()
        });
        if !confirmed
            || quote.record_type != "launchpad_hood_curve_paper_quote"
            || quote.launchpad != LaunchpadId::HoodFun
            || quote.entry.amount_in_requested == U256::ZERO
            || quote.entry.amount_in_requested > policy.max_input_wei
            || quote.entry.amount_in_consumed != quote.entry.amount_in_requested
            || quote.entry.refund != U256::ZERO
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
            || !hood_quote_arithmetic_is_consistent(&quote, policy, expected_profile)
        {
            anyhow::bail!("unsafe or inconsistent Hood curve quote evidence for {key:?}");
        }
        plans.push(FinalizedV3PaperPlan {
            record_type: "launchpad_paper_finalized_plan",
            tx_hash: quote.tx_hash,
            launchpad: quote.launchpad,
            feed_sequence,
            status: "quoted_execution_gated",
            amount_in: quote.entry.amount_in_requested,
            expected_output: quote.entry.expected_output,
            min_receive: quote.entry.min_receive,
            quote_source: quote.quote_source,
            quote_state_version: serde_json::to_value(quote.state_version)?,
            exit_full_position: true,
            exit_expected_output: quote.full_position_exit.expected_output,
            exit_min_receive: quote.full_position_exit.min_receive,
            simulated_round_trip_return_bps: quote.simulated_round_trip_return_bps,
            leader_amounts_reused: false,
            execution_eligible: false,
            execution_blocker: quote.execution_blocker,
            broadcast: false,
        });
    }
    Ok(plans)
}

fn hood_quote_arithmetic_is_consistent(
    quote: &HoodReceiptPaperQuote,
    policy: PaperPlanPolicy,
    expected_profile: &HoodExpectedProfile,
) -> bool {
    if expected_profile.validate().is_err()
        || quote.quote_source != "confirmed_receipt_and_fixed_block_hood_curve_state"
        || quote.sizing_source != "independent_fixed_wei_policy_not_leader_amount"
        || quote.execution_blocker != "paper_only_no_signer_or_broadcast_capability"
        || quote.token == alloy_primitives::Address::ZERO
        || quote.leader == alloy_primitives::Address::ZERO
        || quote.entry.slippage_bps != policy.slippage_bps
        || quote.full_position_exit.slippage_bps != policy.slippage_bps
        || quote.token_curve_supply == U256::ZERO
        || quote.token_lp_supply == U256::ZERO
        || !(expected_profile.semantic.min_trade_fee_bps
            ..=expected_profile.semantic.max_trade_fee_bps)
            .contains(&quote.receipt_end_curve.fee_bps)
        || quote.receipt_end_curve.virtual_quote_reserve != quote.observed.virtual_eth_after
        || quote.receipt_end_curve.virtual_token_reserve != quote.observed.virtual_tokens_after
    {
        return false;
    }
    let Some(total_supply) = quote.token_curve_supply.checked_add(quote.token_lp_supply) else {
        return false;
    };
    let Some(expected_curve_supply) = total_supply
        .checked_mul(U256::from(expected_profile.semantic.curve_allocation_bps))
        .map(|value| value / U256::from(10_000_u16))
    else {
        return false;
    };
    let default_supply = U256::from(1_000_000_000_u64) * U256::from(1_000_000_000_000_000_000_u64);
    let default_virtual_tokens =
        U256::from(1_145_000_000_u64) * U256::from(1_000_000_000_000_000_000_u64);
    let Some(virtual_seed) = default_virtual_tokens
        .checked_mul(total_supply)
        .map(|value| value / default_supply)
    else {
        return false;
    };
    let Some(virtual_real_offset) = virtual_seed.checked_sub(quote.token_curve_supply) else {
        return false;
    };
    let Some(expected_remaining) = quote
        .observed
        .virtual_tokens_after
        .checked_sub(virtual_real_offset)
    else {
        return false;
    };
    if quote.token_curve_supply != expected_curve_supply
        || quote.receipt_end_curve.remaining_curve_tokens != expected_remaining
    {
        return false;
    }
    let Ok(entry) = quote_hood_curve_buy(quote.receipt_end_curve, quote.entry.amount_in_requested)
    else {
        return false;
    };
    if entry.graduates {
        return false;
    }
    let Ok(exit) = quote_hood_curve_sell(entry.state_after, entry.amount_out) else {
        return false;
    };
    let slippage = |amount: U256| {
        amount
            .checked_mul(U256::from(10_000_u16 - policy.slippage_bps))
            .map(|value| value / U256::from(10_000_u16))
    };
    let round_trip = exit
        .amount_out
        .checked_mul(U256::from(10_000_u16))
        .map(|value| value / quote.entry.amount_in_requested);
    quote.entry.amount_in_consumed == entry.amount_in_consumed
        && quote.entry.refund == entry.refund
        && quote.entry.fee == entry.fee
        && quote.entry.expected_output == entry.amount_out
        && quote.entry.min_receive == slippage(entry.amount_out).unwrap_or_default()
        && quote.entry.state_after == entry.state_after
        && quote.full_position_exit.amount_in == exit.amount_in
        && quote.full_position_exit.gross_output == exit.gross_output
        && quote.full_position_exit.fee == exit.fee
        && quote.full_position_exit.expected_output == exit.amount_out
        && quote.full_position_exit.min_receive == slippage(exit.amount_out).unwrap_or_default()
        && quote.full_position_exit.state_after == exit.state_after
        && Some(quote.simulated_round_trip_return_bps) == round_trip
}

fn pons_quote_arithmetic_is_consistent(
    quote: &PonsReceiptPaperQuote,
    policy: PaperPlanPolicy,
    expected_profile: hermes_feed::PonsExpectedProfile,
) -> bool {
    use hermes_feed::pons::{
        PONS_CURRENT_FACTORY, PONS_CURRENT_LOCKER, PONS_POOL_FEE, PONS_POSITION_MANAGER,
        PONS_SWAP_ROUTER_02, PONS_TICK_SPACING, PONS_V3_FACTORY, PONS_WETH, PonsGeneration,
    };
    use hermes_feed::robinhood::UNISWAP_V3_POOL_INIT_CODE_KECCAK256;
    use uniswap_v3_math::sqrt_price_math::{_get_amount_0_delta, _get_amount_1_delta};
    use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

    const MAX_WALLET_BPS: u16 = 200;
    const RESTRICTION_L1_BLOCKS: u64 = 366;
    let market = &quote.market;
    let entry = &quote.entry;
    let exit = &quote.full_position_exit;
    let token0 = market.token.min(PONS_WETH);
    let token1 = market.token.max(PONS_WETH);
    let expected_pool = hermes_feed::noxa_predict::predict_v3_pool_address(
        PONS_V3_FACTORY,
        token0,
        token1,
        PONS_POOL_FEE,
        UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    );
    let (expected_initialize_tick, expected_range) = if market.token < PONS_WETH {
        (-204_200, (-204_200, 887_200))
    } else {
        (204_200, (-887_200, 204_200))
    };
    let Ok(position_liquidity) = u128::try_from(market.position_liquidity) else {
        return false;
    };
    let Ok(receipt_end_liquidity) = u128::try_from(market.receipt_end_liquidity) else {
        return false;
    };
    if expected_profile.validate().is_err() {
        return false;
    }
    let expected_runtime_hash = |address| {
        expected_profile
            .identity(address)
            .map(|identity| identity.runtime_hash)
    };
    let Some(expected_first_eligible_l1) =
        quote.state_version.launch_l1_block_number.checked_add(1)
    else {
        return false;
    };
    let Some(expected_restriction_end_l1) = quote
        .state_version
        .launch_l1_block_number
        .checked_add(RESTRICTION_L1_BLOCKS)
    else {
        return false;
    };
    if quote.quote_source != "confirmed_receipt_end_pons_v3_state"
        || quote.sizing_source != "independent_fixed_tiny_weth_fresh_wallet_policy"
        || quote.execution_blocker
            != "paper_only_current_factory_source_prediction_restriction_and_route_gates_not_satisfied"
        || market.generation != PonsGeneration::Current
        || market.leader == alloy_primitives::Address::ZERO
        || market.token == alloy_primitives::Address::ZERO
        || market.token == PONS_WETH
        || market.pool != expected_pool
        || market.quote_asset != PONS_WETH
        || market.factory != PONS_CURRENT_FACTORY
        || Some(market.factory_runtime_hash) != expected_runtime_hash(PONS_CURRENT_FACTORY)
        || market.locker != PONS_CURRENT_LOCKER
        || Some(market.locker_runtime_hash) != expected_runtime_hash(PONS_CURRENT_LOCKER)
        || market.position_manager != PONS_POSITION_MANAGER
        || Some(market.position_manager_runtime_hash)
            != expected_runtime_hash(PONS_POSITION_MANAGER)
        || Some(market.v3_factory_runtime_hash) != expected_runtime_hash(PONS_V3_FACTORY)
        || Some(market.swap_router_runtime_hash) != expected_runtime_hash(PONS_SWAP_ROUTER_02)
        || Some(market.quote_asset_runtime_hash) != expected_runtime_hash(PONS_WETH)
        || market.position_id == U256::ZERO
        || market.fee != PONS_POOL_FEE
        || market.tick_spacing != PONS_TICK_SPACING
        || market.initialize_tick != expected_initialize_tick
        || (market.position_tick_lower, market.position_tick_upper) != expected_range
        || position_liquidity == 0
        || market.mint_count != 1
        || market.launch_swap_count > 1
        || !(market.initialize_log_index < market.mint_log_index
            && market.mint_log_index < market.locker_log_index
            && market.locker_log_index < market.launch_log_index)
        || quote.tx_hash == B256::ZERO
        || quote.state_version.block_hash == B256::ZERO
        || quote.state_version.first_eligible_l1_block_number != expected_first_eligible_l1
        || quote.state_version.restriction_end_l1_block_number != expected_restriction_end_l1
        || entry.state_after.token_in != PONS_WETH
        || entry.state_after.token_out != market.token
        || exit.state_after.token_in != market.token
        || exit.state_after.token_out != PONS_WETH
        || exit.slippage_bps != policy.slippage_bps
        || (token0 == PONS_WETH
            && (market.position_amount0 != U256::ZERO || market.position_amount1 == U256::ZERO))
        || (token1 == PONS_WETH
            && (market.position_amount1 != U256::ZERO || market.position_amount0 == U256::ZERO))
    {
        return false;
    }

    let Ok(expected_initialize_sqrt) = get_sqrt_ratio_at_tick(expected_initialize_tick) else {
        return false;
    };
    let Ok(lower_sqrt) = get_sqrt_ratio_at_tick(market.position_tick_lower) else {
        return false;
    };
    let Ok(upper_sqrt) = get_sqrt_ratio_at_tick(market.position_tick_upper) else {
        return false;
    };
    let expected_position_amounts = if token0 == PONS_WETH {
        let Ok(amount1) = _get_amount_1_delta(lower_sqrt, upper_sqrt, position_liquidity, true)
        else {
            return false;
        };
        (U256::ZERO, amount1)
    } else {
        let Ok(amount0) = _get_amount_0_delta(lower_sqrt, upper_sqrt, position_liquidity, true)
        else {
            return false;
        };
        (amount0, U256::ZERO)
    };
    if (market.position_amount0, market.position_amount1) != expected_position_amounts {
        return false;
    }

    let Ok(mut state) = hermes_feed::V3PoolState::new(
        market.pool,
        token0,
        token1,
        market.fee,
        market.tick_spacing,
        expected_initialize_sqrt,
        expected_initialize_tick,
        0,
    ) else {
        return false;
    };
    if state
        .add_position(
            market.position_tick_lower,
            market.position_tick_upper,
            position_liquidity,
        )
        .is_err()
    {
        return false;
    }
    if market.initial_buy_amount == U256::ZERO {
        if market.launch_swap_count != 0
            || market.initial_buy_swap_log_index.is_some()
            || market.initial_buy_state_after.is_some()
            || quote.state_version.terminal_log_index != market.launch_log_index
            || market.receipt_end_sqrt_price_x96 != state.sqrt_price_x96
            || market.receipt_end_tick != state.tick
            || receipt_end_liquidity != state.liquidity
        {
            return false;
        }
    } else {
        let Some(swap_log_index) = market.initial_buy_swap_log_index else {
            return false;
        };
        let Ok(initial_buy_state) =
            state.quote_exact_input(PONS_WETH, market.initial_buy_amount, None)
        else {
            return false;
        };
        if market.launch_swap_count != 1
            || swap_log_index <= market.launch_log_index
            || quote.state_version.terminal_log_index != swap_log_index
            || market.initial_buy_state_after.as_ref() != Some(&initial_buy_state)
            || market.receipt_end_sqrt_price_x96 != initial_buy_state.sqrt_price_x96_after
            || market.receipt_end_tick != initial_buy_state.tick_after
            || receipt_end_liquidity != initial_buy_state.liquidity_after
            || state
                .set_observation(
                    initial_buy_state.sqrt_price_x96_after,
                    initial_buy_state.tick_after,
                    initial_buy_state.liquidity_after,
                )
                .is_err()
        {
            return false;
        }
    }
    let Ok(expected_entry_state) = state.quote_exact_input(PONS_WETH, entry.amount_in, None) else {
        return false;
    };
    if expected_entry_state != entry.state_after {
        return false;
    }
    let mut post_entry = state;
    if post_entry
        .set_observation(
            expected_entry_state.sqrt_price_x96_after,
            expected_entry_state.tick_after,
            expected_entry_state.liquidity_after,
        )
        .is_err()
    {
        return false;
    }
    let Ok(expected_exit_state) =
        post_entry.quote_exact_input(market.token, expected_entry_state.amount_out, None)
    else {
        return false;
    };
    let retained_bps = U256::from(10_000_u16 - policy.slippage_bps);
    let expected_entry_min =
        expected_entry_state.amount_out * retained_bps / U256::from(10_000_u16);
    let expected_exit_min = expected_exit_state.amount_out * retained_bps / U256::from(10_000_u16);
    let expected_round_trip =
        expected_exit_state.amount_out * U256::from(10_000_u16) / entry.amount_in;
    let supply = U256::from(1_000_000_000_u64) * U256::from(1_000_000_000_000_000_000_u64);
    let max_fresh_wallet_output = supply * U256::from(MAX_WALLET_BPS) / U256::from(10_000_u16);

    entry.amount_in <= policy.max_input_wei
        && entry.amount_in == entry.state_after.amount_in_requested
        && entry.amount_in == entry.state_after.amount_in_consumed
        && entry.expected_output == expected_entry_state.amount_out
        && entry.min_receive == expected_entry_min
        && entry.expected_output <= max_fresh_wallet_output
        && exit.amount_in == expected_entry_state.amount_out
        && exit.state_after == expected_exit_state
        && exit.expected_output == expected_exit_state.amount_out
        && exit.min_receive == expected_exit_min
        && quote.simulated_round_trip_return_bps == expected_round_trip
}

fn bankr_quote_arithmetic_is_consistent(
    quote: &BankrDopplerReceiptPaperQuote,
    policy: PaperPlanPolicy,
) -> bool {
    let profile = BankrDopplerExpectedProfile::production();
    let entry = &quote.entry;
    let exit = &quote.full_position_exit;
    let Ok(pool_key) = hermes_feed::uniswap_v4::V4PoolKey::canonical(
        profile.weth.address,
        quote.market.token,
        hermes_feed::uniswap_v4::DYNAMIC_FEE_FLAG,
        profile.tick_spacing,
        profile.initializer.address,
    ) else {
        return false;
    };
    let Ok(expected_hook_fee_ppm) = bankr_hook_fee_ppm(
        quote.state_version.receipt_timestamp,
        profile.hook_start_fee_ppm,
        profile.hook_end_fee_ppm,
        profile.hook_duration_seconds,
        quote.state_version.first_eligible_quote_timestamp,
    ) else {
        return false;
    };
    let Some(expected_first_eligible_timestamp) = quote
        .state_version
        .receipt_timestamp
        .checked_add(profile.quote_delay_guard_seconds)
    else {
        return false;
    };
    let expected_initialize_tick = if quote.market.token < profile.weth.address {
        -229_600
    } else {
        229_800
    };
    if quote.quote_source != "confirmed_receipt_end_bankr_doppler_first_nonzero_state"
        || quote.sizing_source != "independent_fixed_tiny_weth_policy"
        || quote.execution_blocker
            != "paper_only_bankr_rehype_router_permit2_and_account_execution_not_enabled"
        || quote.market.leader != profile.smart_account.account.address
        || quote.market.outer_bundler == alloy_primitives::Address::ZERO
        || quote.market.token == alloy_primitives::Address::ZERO
        || quote.market.token == profile.weth.address
        || quote.market.pool_id != pool_key.pool_id()
        || quote.market.quote_asset != profile.weth.address
        || quote.market.pool_manager != profile.pool_manager.address
        || quote.market.initializer != profile.initializer.address
        || quote.market.rehype_hook != profile.rehype_hook.address
        || quote.market.buyback_destination == alloy_primitives::Address::ZERO
        || entry.amount_in == U256::ZERO
        || quote.market.lp_fee_ppm != profile.standard_lp_fee_ppm
        || quote.market.hook_start_fee_ppm != profile.hook_start_fee_ppm
        || quote.market.hook_end_fee_ppm != profile.hook_end_fee_ppm
        || quote.market.hook_duration_seconds != profile.hook_duration_seconds
        || quote.market.tick_spacing != profile.tick_spacing
        || quote.market.initialize_tick != expected_initialize_tick
        || quote.market.position_count != 2
        || quote.market.initialize_log_index >= quote.market.last_liquidity_log_index
        || quote.market.last_liquidity_log_index >= quote.market.launch_log_index
        || quote.market.launch_log_index >= quote.market.user_operation_log_index
        || quote.state_version.terminal_log_index != quote.market.user_operation_log_index
        || quote.state_version.first_eligible_quote_timestamp != expected_first_eligible_timestamp
        || entry.lp_fee_ppm != quote.market.lp_fee_ppm
        || exit.lp_fee_ppm != quote.market.lp_fee_ppm
        || entry.hook_fee_ppm != exit.hook_fee_ppm
        || entry.hook_fee_ppm != expected_hook_fee_ppm
        || entry.hook_fee_denominator_ppm != profile.hook_fee_denominator_ppm
        || exit.hook_fee_denominator_ppm != profile.hook_fee_denominator_ppm
        || entry.core_state_after.token_in != quote.market.quote_asset
        || entry.core_state_after.token_out != quote.market.token
        || exit.core_state_after.token_in != quote.market.token
        || exit.core_state_after.token_out != quote.market.quote_asset
        || exit.internal_buyback_state_after.is_some()
    {
        return false;
    }

    let swap_arithmetic = |amount_in: U256,
                           core_expected_output: U256,
                           hook_output_fee: U256,
                           expected_output: U256,
                           min_receive: U256,
                           slippage_bps: u16,
                           hook_fee_ppm: u32,
                           hook_fee_denominator_ppm: u32,
                           state: &hermes_feed::V3Quote| {
        if hook_fee_denominator_ppm == 0 || slippage_bps != policy.slippage_bps {
            return false;
        }
        let Some(calculated_hook_fee) = core_expected_output
            .checked_mul(U256::from(hook_fee_ppm))
            .map(|value| value / U256::from(hook_fee_denominator_ppm))
        else {
            return false;
        };
        let Some(calculated_output) = core_expected_output.checked_sub(calculated_hook_fee) else {
            return false;
        };
        let Some(retained_bps) = 10_000_u16.checked_sub(slippage_bps) else {
            return false;
        };
        let Some(calculated_minimum) = calculated_output
            .checked_mul(U256::from(retained_bps))
            .map(|value| value / U256::from(10_000_u16))
        else {
            return false;
        };
        state.amount_in_requested == amount_in
            && state.amount_in_consumed == amount_in
            && state.amount_out == core_expected_output
            && hook_output_fee == calculated_hook_fee
            && expected_output == calculated_output
            && min_receive == calculated_minimum
            && calculated_output != U256::ZERO
            && calculated_minimum != U256::ZERO
    };
    if !swap_arithmetic(
        entry.amount_in,
        entry.core_expected_output,
        entry.hook_output_fee,
        entry.expected_output,
        entry.min_receive,
        entry.slippage_bps,
        entry.hook_fee_ppm,
        entry.hook_fee_denominator_ppm,
        &entry.core_state_after,
    ) || !swap_arithmetic(
        exit.amount_in,
        exit.core_expected_output,
        exit.hook_output_fee,
        exit.expected_output,
        exit.min_receive,
        exit.slippage_bps,
        exit.hook_fee_ppm,
        exit.hook_fee_denominator_ppm,
        &exit.core_state_after,
    ) {
        return false;
    }

    let Some(internal) = entry.internal_buyback_state_after.as_ref() else {
        return false;
    };
    let Some(owner_fee) = entry
        .hook_output_fee
        .checked_mul(U256::from(profile.protocol_beneficiary_bps))
        .map(|value| value / U256::from(10_000_u16))
    else {
        return false;
    };
    let Some(internal_input) = entry.hook_output_fee.checked_sub(owner_fee) else {
        return false;
    };
    let Some(round_trip_bps) = exit
        .expected_output
        .checked_mul(U256::from(10_000_u16))
        .map(|value| value / entry.amount_in)
    else {
        return false;
    };
    internal.token_in == quote.market.token
        && internal.token_out == quote.market.quote_asset
        && internal.amount_in_requested == internal_input
        && internal.amount_in_consumed == internal_input
        && internal.amount_out != U256::ZERO
        && exit.amount_in == entry.expected_output
        && quote.simulated_round_trip_return_bps == round_trip_bps
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
    use hermes_feed::{
        BankrDopplerExpectedProfile, BankrDopplerQuotePolicy, ClankerQuotePolicy,
        ClankerV4ExpectedProfile, NoxaReceipt, PonsQuotePolicy, RobinhoodBlock,
        RobinhoodTransaction, V3PaperSwapQuote, V3Quote, V3QuoteStateVersion,
        V3ReceiptMarketEvidence, quote_bankr_doppler_launch_receipt, quote_clanker_launch_receipt,
        quote_pons_launch_receipt,
    };

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
                pons_generation: None,
                protocol_blocker: None,
            },
            ReconciliationEvidence {
                tx_hash: false_positive_key.0,
                launchpad: false_positive_key.1,
                receipt_status: true,
                protocol_event_match: false,
                observed_unix_ns: 250,
                pons_generation: None,
                protocol_blocker: None,
            },
            ReconciliationEvidence {
                tx_hash: missed_key.0,
                launchpad: missed_key.1,
                receipt_status: true,
                protocol_event_match: true,
                observed_unix_ns: 400,
                pons_generation: None,
                protocol_blocker: None,
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

    #[derive(Deserialize)]
    struct ClankerLiveFixture {
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
    }

    fn clanker_quote_fixture() -> ClankerReceiptPaperQuote {
        let fixture: ClankerLiveFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/clanker-v4-live-proof.json"
        ))
        .unwrap();
        quote_clanker_launch_receipt(
            &fixture.transaction,
            &fixture.receipt,
            &fixture.block,
            ClankerV4ExpectedProfile::production(),
            ClankerQuotePolicy {
                amount_in: U256::from(1_000_u64),
                max_amount_in: U256::from(1_000_u64),
                slippage_bps: 100,
            },
        )
        .unwrap()
    }

    fn bankr_quote_fixture() -> BankrDopplerReceiptPaperQuote {
        let fixture: ClankerLiveFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/bankr-doppler-live-proof.json"
        ))
        .unwrap();
        quote_bankr_doppler_launch_receipt(
            &fixture.transaction,
            &fixture.receipt,
            &fixture.block,
            BankrDopplerExpectedProfile::production(),
            BankrDopplerQuotePolicy {
                amount_in: U256::from(1_000_u64),
                max_amount_in: U256::from(1_000_u64),
                slippage_bps: 100,
            },
        )
        .unwrap()
    }

    fn pons_quote_fixture() -> PonsReceiptPaperQuote {
        pons_quote_from_fixture(include_str!(
            "../../tests/fixtures/pons-current-live-proof.json"
        ))
    }

    fn pons_initial_buy_quote_fixture() -> PonsReceiptPaperQuote {
        pons_quote_from_fixture(include_str!(
            "../../tests/fixtures/pons-current-initial-buy-live-proof.json"
        ))
    }

    fn pons_below_weth_quote_fixture() -> PonsReceiptPaperQuote {
        pons_quote_from_fixture(include_str!(
            "../../tests/fixtures/pons-current-token-below-weth-live-proof.json"
        ))
    }

    fn pons_quote_from_fixture(contents: &str) -> PonsReceiptPaperQuote {
        let fixture: ClankerLiveFixture = serde_json::from_str(contents).unwrap();
        quote_pons_launch_receipt(
            &fixture.transaction,
            &fixture.receipt,
            hermes_feed::PonsExpectedProfile::production(),
            PonsQuotePolicy {
                amount_in: U256::from(1_000_000_000_000_000_u64),
                max_amount_in: U256::from(1_000_000_000_000_000_u64),
                slippage_bps: 100,
            },
        )
        .unwrap()
    }

    fn finalize_bankr_quote(
        quote: BankrDopplerReceiptPaperQuote,
    ) -> Result<Vec<FinalizedV3PaperPlan>> {
        let key = (quote.tx_hash, quote.launchpad);
        finalized_bankr_plans(
            &HashMap::from([(key, 88)]),
            &[ReconciliationEvidence {
                tx_hash: key.0,
                launchpad: key.1,
                receipt_status: true,
                protocol_event_match: true,
                observed_unix_ns: 100,
                pons_generation: None,
                protocol_blocker: None,
            }],
            vec![quote],
            PaperPlanPolicy {
                max_input_wei: U256::from(1_000_u64),
                slippage_bps: 100,
                ..PaperPlanPolicy::default()
            },
        )
    }

    fn finalize_pons_quote(quote: PonsReceiptPaperQuote) -> Result<Vec<FinalizedV3PaperPlan>> {
        let key = (quote.tx_hash, quote.launchpad);
        finalized_pons_plans(
            &HashMap::from([(key, 99)]),
            &[ReconciliationEvidence {
                tx_hash: key.0,
                launchpad: key.1,
                receipt_status: true,
                protocol_event_match: true,
                observed_unix_ns: 100,
                pons_generation: Some(hermes_feed::PonsGeneration::Current),
                protocol_blocker: None,
            }],
            vec![quote],
            PaperPlanPolicy {
                max_input_wei: U256::from(1_000_000_000_000_000_u64),
                slippage_bps: 100,
                ..PaperPlanPolicy::default()
            },
            hermes_feed::PonsExpectedProfile::production(),
        )
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
                pons_generation: None,
                protocol_blocker: None,
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
                    pons_generation: None,
                    protocol_blocker: None,
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

    #[test]
    fn confirmed_clanker_quote_becomes_execution_gated_finalized_plan() {
        let quote = clanker_quote_fixture();
        let key = (quote.tx_hash, quote.launchpad);
        let plans = finalized_clanker_plans(
            &HashMap::from([(key, 77)]),
            &[ReconciliationEvidence {
                tx_hash: key.0,
                launchpad: key.1,
                receipt_status: true,
                protocol_event_match: true,
                observed_unix_ns: 100,
                pons_generation: None,
                protocol_blocker: None,
            }],
            vec![quote],
            PaperPlanPolicy {
                max_input_wei: U256::from(1_000_u64),
                slippage_bps: 100,
                ..PaperPlanPolicy::default()
            },
        )
        .unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].status, "quoted_execution_gated");
        assert_eq!(plans[0].feed_sequence, 77);
        assert!(!plans[0].execution_eligible);
        assert!(!plans[0].broadcast);
    }

    #[test]
    fn confirmed_bankr_quote_becomes_execution_gated_finalized_plan() {
        let quote = bankr_quote_fixture();
        let plans = finalize_bankr_quote(quote).unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].launchpad, LaunchpadId::BankrDoppler);
        assert_eq!(plans[0].status, "quoted_execution_gated");
        assert_eq!(plans[0].feed_sequence, 88);
        assert!(plans[0].expected_output > U256::ZERO);
        assert!(plans[0].min_receive > U256::ZERO);
        assert!(!plans[0].execution_eligible);
        assert!(!plans[0].broadcast);
    }

    #[test]
    fn confirmed_pons_quote_becomes_rederived_execution_gated_plan() {
        let plans = finalize_pons_quote(pons_quote_fixture()).unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].launchpad, LaunchpadId::Pons);
        assert_eq!(plans[0].status, "quoted_execution_gated");
        assert_eq!(plans[0].feed_sequence, 99);
        assert!(plans[0].expected_output > U256::ZERO);
        assert!(plans[0].exit_expected_output > U256::ZERO);
        assert!(!plans[0].execution_eligible);
        assert!(!plans[0].broadcast);

        let initial_buy = finalize_pons_quote(pons_initial_buy_quote_fixture()).unwrap();
        assert_eq!(initial_buy.len(), 1);
        assert!(initial_buy[0].expected_output > U256::ZERO);

        let below_weth = finalize_pons_quote(pons_below_weth_quote_fixture()).unwrap();
        assert_eq!(below_weth.len(), 1);
        assert!(below_weth[0].exit_expected_output > U256::ZERO);
    }

    #[test]
    fn tampered_pons_quote_or_pins_cannot_finalize() {
        let mut output = pons_quote_fixture();
        output.entry.expected_output += U256::from(1_u8);
        assert!(finalize_pons_quote(output).is_err());

        let mut pool = pons_quote_fixture();
        pool.market.pool = Address::with_last_byte(0xee);
        assert!(finalize_pons_quote(pool).is_err());

        let mut restriction = pons_quote_fixture();
        restriction.state_version.restriction_end_l1_block_number += 1;
        assert!(finalize_pons_quote(restriction).is_err());

        let mut liquidity = pons_quote_fixture();
        liquidity.market.position_liquidity += U256::from(1_u8);
        assert!(finalize_pons_quote(liquidity).is_err());

        let mut round_trip = pons_quote_fixture();
        round_trip.simulated_round_trip_return_bps += U256::from(1_u8);
        assert!(finalize_pons_quote(round_trip).is_err());

        let mut canonical_state = pons_quote_fixture();
        canonical_state.market.receipt_end_sqrt_price_x96 += U256::from(1_u8);
        assert!(finalize_pons_quote(canonical_state).is_err());

        let mut initial_buy = pons_initial_buy_quote_fixture();
        initial_buy.market.initial_buy_amount += U256::from(1_u8);
        assert!(finalize_pons_quote(initial_buy).is_err());

        let mut runtime_pin = pons_quote_fixture();
        runtime_pin.market.factory_runtime_hash = B256::with_last_byte(0xee);
        assert!(finalize_pons_quote(runtime_pin).is_err());
    }

    #[test]
    fn tampered_bankr_quote_arithmetic_cannot_finalize() {
        let mut output = bankr_quote_fixture();
        output.entry.expected_output += U256::from(1_u8);
        assert!(finalize_bankr_quote(output).is_err());

        let mut buyback = bankr_quote_fixture();
        buyback
            .entry
            .internal_buyback_state_after
            .as_mut()
            .unwrap()
            .amount_in_consumed += U256::from(1_u8);
        assert!(finalize_bankr_quote(buyback).is_err());

        let mut round_trip = bankr_quote_fixture();
        round_trip.simulated_round_trip_return_bps += U256::from(1_u8);
        assert!(finalize_bankr_quote(round_trip).is_err());

        let mut pinned_market = bankr_quote_fixture();
        pinned_market.market.pool_manager = Address::with_last_byte(0xee);
        assert!(finalize_bankr_quote(pinned_market).is_err());

        let mut hook_fee = bankr_quote_fixture();
        hook_fee.entry.hook_fee_ppm += 1;
        hook_fee.full_position_exit.hook_fee_ppm += 1;
        assert!(finalize_bankr_quote(hook_fee).is_err());

        let mut schedule = bankr_quote_fixture();
        schedule.market.hook_end_fee_ppm += 1;
        assert!(finalize_bankr_quote(schedule).is_err());
    }

    #[derive(serde::Deserialize)]
    struct HoodLiveFixture {
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
        state: HoodFixtureState,
    }

    #[derive(serde::Deserialize)]
    struct HoodFixtureState {
        token: Address,
        post_block: u64,
        post_curve: HoodFixtureCurve,
        config: hermes_feed::HoodConfigSnapshot,
        token_curve_supply: U256,
        token_lp_supply: U256,
        factory: Address,
    }

    #[derive(serde::Deserialize)]
    struct HoodFixtureCurve {
        virtual_eth: U256,
        virtual_tokens: U256,
        real_eth: U256,
        real_tokens: U256,
        creator: Address,
        created_at_block: U256,
        graduated: bool,
        migrated: bool,
        trade_fee_bps: u16,
    }

    fn hood_quote_fixture() -> HoodReceiptPaperQuote {
        let fixture: HoodLiveFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/hood-normal-buy-live-proof.json"
        ))
        .unwrap();
        let profile = HoodExpectedProfile::production();
        let snapshot = hermes_feed::HoodMarketSnapshot {
            factory: fixture.state.factory,
            token: fixture.state.token,
            l2_block_number: fixture.state.post_block,
            curve: hermes_feed::HoodCurveStateSnapshot {
                virtual_eth: fixture.state.post_curve.virtual_eth,
                virtual_tokens: fixture.state.post_curve.virtual_tokens,
                real_eth: fixture.state.post_curve.real_eth,
                real_tokens: fixture.state.post_curve.real_tokens,
                creator: fixture.state.post_curve.creator,
                created_at_block: u64::try_from(fixture.state.post_curve.created_at_block).unwrap(),
                graduated: fixture.state.post_curve.graduated,
                migrated: fixture.state.post_curve.migrated,
                trade_fee_bps: fixture.state.post_curve.trade_fee_bps,
            },
            config: fixture.state.config,
            token_curve_supply: fixture.state.token_curve_supply,
            token_lp_supply: fixture.state.token_lp_supply,
            migrator: profile.semantic.active_migrator,
            uniswap_factory: profile.semantic.fallback_factory,
            weth: profile.semantic.weth,
        };
        hermes_feed::quote_hood_curve_receipt(
            &fixture.transaction,
            &fixture.receipt,
            &fixture.block,
            &snapshot,
            profile.semantic,
            hermes_feed::HoodQuotePolicy {
                amount_in: U256::from(1_000_000_000_000_000_u64),
                max_amount_in: U256::from(10_000_000_000_000_000_u64),
                slippage_bps: 100,
            },
        )
        .unwrap()
    }

    fn finalize_hood_quote(quote: HoodReceiptPaperQuote) -> Result<Vec<FinalizedV3PaperPlan>> {
        let key = (quote.tx_hash, quote.launchpad);
        finalized_hood_plans(
            &HashMap::from([(key, 71)]),
            &[ReconciliationEvidence {
                tx_hash: key.0,
                launchpad: key.1,
                receipt_status: true,
                protocol_event_match: true,
                observed_unix_ns: 1,
                pons_generation: None,
                protocol_blocker: None,
            }],
            vec![quote],
            PaperPlanPolicy::default(),
            &HoodExpectedProfile::production(),
        )
    }

    #[test]
    fn strict_hood_quote_finalizes_with_real_entry_and_exit() {
        let plans = finalize_hood_quote(hood_quote_fixture()).unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].launchpad, LaunchpadId::HoodFun);
        assert!(plans[0].expected_output > U256::ZERO);
        assert!(plans[0].exit_expected_output > U256::ZERO);
        assert!(!plans[0].execution_eligible);
        assert!(!plans[0].broadcast);
    }

    #[test]
    fn tampered_hood_quote_or_profile_cannot_finalize() {
        let mut output = hood_quote_fixture();
        output.entry.expected_output += U256::from(1_u8);
        assert!(finalize_hood_quote(output).is_err());

        let mut round_trip = hood_quote_fixture();
        round_trip.simulated_round_trip_return_bps += U256::from(1_u8);
        assert!(finalize_hood_quote(round_trip).is_err());

        let mut observed_state = hood_quote_fixture();
        observed_state.observed.virtual_eth_after += U256::from(1_u8);
        assert!(finalize_hood_quote(observed_state).is_err());

        let mut supplies = hood_quote_fixture();
        supplies.token_curve_supply += U256::from(1_u8);
        assert!(finalize_hood_quote(supplies).is_err());

        let quote = hood_quote_fixture();
        let key = (quote.tx_hash, quote.launchpad);
        let mut profile = HoodExpectedProfile::production();
        profile.identities[0].runtime_hash = B256::with_last_byte(0xee);
        assert!(
            finalized_hood_plans(
                &HashMap::from([(key, 1)]),
                &[],
                vec![quote],
                PaperPlanPolicy::default(),
                &profile,
            )
            .is_err()
        );
    }

    #[test]
    fn hood_migration_evidence_parses_end_to_end_but_never_finalizes_as_a_quote() {
        let tx_hash = B256::with_last_byte(0x44);
        let evidence = serde_json::json!({
            "tx_hash": tx_hash,
            "launchpad": "hood_fun",
            "receipt_status": true,
            "protocol_event_match": true,
            "observed_unix_ns": 9,
            "protocol_blocker": "hood_migration_topology_verified_v3_quote_unavailable"
        });
        let migration = serde_json::json!({
            "record_type": "launchpad_hood_migration_evidence",
            "tx_hash": tx_hash,
            "launchpad": "hood_fun",
            "token": Address::with_last_byte(1),
            "pool": Address::with_last_byte(2),
            "leader": Address::with_last_byte(3),
            "trader": Address::with_last_byte(4),
            "l2_block_number": 10,
            "token_id": "1",
            "raised_eth": "2",
            "declared_eth_liquidity": "3",
            "declared_token_liquidity": "4",
            "actual_eth_liquidity": "5",
            "actual_token_liquidity": "6",
            "declared_and_actual_liquidity_match": false,
            "pool_initialize_sqrt_price_x96": "7",
            "pool_initialize_tick": -1,
            "expected_profile_validated": true,
            "receipt_topology_verified": true,
            "pool_state_reconciled": false,
            "v3_quote_available": false,
            "execution_eligible": false,
            "execution_blocker": "declared_actual_liquidity_mismatch_and_missing_independent_v3_state_quote",
            "broadcast": false
        });
        let path = std::env::temp_dir().join(format!(
            "hermes-hood-migration-{}-{}.jsonl",
            std::process::id(),
            tx_hash
        ));
        std::fs::write(&path, format!("{}\n{}\n", evidence, migration)).unwrap();
        let records = read_reconciliation_records(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(records.evidence.len(), 1);
        assert_eq!(records.hood_migrations.len(), 1);
        validate_hood_migration_records(&records.evidence, &records.hood_migrations).unwrap();
        assert!(records.hood_quotes.is_empty());
        assert!(validate_hood_migration_records(&records.evidence, &[]).is_err());

        let mut forged = records.hood_migrations.clone();
        forged[0].declared_and_actual_liquidity_match = true;
        forged[0].execution_blocker =
            "migration_topology_only_missing_independent_v3_state_quote".into();
        assert!(validate_hood_migration_records(&records.evidence, &forged).is_err());
    }
}
