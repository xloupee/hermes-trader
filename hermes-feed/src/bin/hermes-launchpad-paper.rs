use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use alloy_primitives::{Address, B256, U256};
use anyhow::{Context, Result};
use clap::Parser;
use hermes_feed::feed::BroadcastMessage;
use hermes_feed::launchpad_adapter::{ActionKind, LaunchpadId};
use hermes_feed::launchpad_ground_truth::launchpad_for_ground_truth_log;
use hermes_feed::paper_observer::{
    PaperExpectedPins, PaperFeedRuntime, PaperLaunchpadObserver, PaperObservedStartupSnapshot,
    PaperPlanPolicy,
};
use hermes_feed::{
    BankrCreateProfileVersion, BankrDopplerExpectedProfile, BankrDopplerReceiptPaperQuote,
    BankrEnvelopeKind, ClankerLiquidityProfile, ClankerQuotePolicy, ClankerReceiptPaperQuote,
    ClankerV4ExpectedProfile, HoodExpectedProfile, HoodMigrationEvidence, HoodReceiptPaperQuote,
    PonsReceiptPaperQuote, V3ReceiptPaperQuote, bankr_hook_fee_ppm, quote_hood_curve_buy,
    quote_hood_curve_sell, validate_clanker_quote_replay,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

fn unix_now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u128::from(u64::MAX)) as u64
}

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
    record_type: String,
    tx_hash: B256,
    launchpad: LaunchpadId,
    receipt_status: bool,
    protocol_event_match: bool,
    #[serde(default)]
    observer_claim: bool,
    #[serde(default)]
    ground_truth_event: bool,
    ground_truth_hits: Vec<GroundTruthHit>,
    action: Option<ActionKind>,
    token: Option<alloy_primitives::Address>,
    pool: Option<alloy_primitives::Address>,
    quote_status: QuoteStatus,
    l2_block_number: Option<u64>,
    block_hash: Option<B256>,
    transaction_index: Option<u64>,
    reconciliation_started_unix_ns: u64,
    reconciliation_completed_unix_ns: u64,
    #[serde(default)]
    pons_generation: Option<hermes_feed::PonsGeneration>,
    #[serde(default)]
    protocol_blocker: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroundTruthHit {
    l2_block_number: u64,
    block_hash: B256,
    transaction_index: u64,
    log: hermes_feed::noxa_abi::ReceiptLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum QuoteStatus {
    Available,
    Blocked,
    NotApplicable,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct ReconciliationMetrics {
    record_type: &'static str,
    launchpad: LaunchpadId,
    coverage_from_l2_block: u64,
    coverage_to_l2_block: u64,
    raw_ground_truth_transactions: usize,
    quote_admitted_ground_truth_transactions: usize,
    observer_claims: usize,
    confirmed_observations: usize,
    false_positives: usize,
    reverted_attempts: usize,
    missed_transactions: usize,
    detector_misses: usize,
    feed_coverage_misses: usize,
    out_of_scope_observations: usize,
    unreconciled_observations: usize,
    quote_available: usize,
    quote_blocked: usize,
    quote_not_applicable: usize,
    action_prediction_eligible: usize,
    action_prediction_missing: usize,
    action_prediction_matches: usize,
    action_prediction_mismatches: usize,
    token_prediction_eligible: usize,
    token_prediction_missing: usize,
    token_prediction_matches: usize,
    token_prediction_mismatches: usize,
    pool_prediction_eligible: usize,
    pool_prediction_missing: usize,
    pool_prediction_matches: usize,
    pool_prediction_mismatches: usize,
    observation_latency_p50_ns: Option<u64>,
    observation_latency_p95_ns: Option<u64>,
    observation_latency_p99_ns: Option<u64>,
    reconciliation_rpc_duration_p50_ns: Option<u64>,
    reconciliation_rpc_duration_p95_ns: Option<u64>,
    reconciliation_rpc_duration_p99_ns: Option<u64>,
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
    ground_truth_window: Option<GroundTruthWindow>,
}

#[derive(Debug, Default)]
struct ObservedOutputCandidates {
    received_unix_ns: HashMap<(B256, LaunchpadId), u64>,
    observer_latency_ns: HashMap<(B256, LaunchpadId), u64>,
    feed_sequences: HashMap<(B256, LaunchpadId), u64>,
    feed_transactions: HashMap<B256, u64>,
    claims: HashMap<(B256, LaunchpadId), ObserverClaim>,
}

#[derive(Debug, Clone, Copy)]
struct ObserverClaim {
    action: Option<ActionKind>,
    predicted_token: Option<alloy_primitives::Address>,
    predicted_pool: Option<alloy_primitives::Address>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroundTruthWindow {
    record_type: String,
    start_head: u64,
    start_head_hash: B256,
    cutoff_head: u64,
    cutoff_head_hash: B256,
    from_l2_block: u64,
    to_l2_block: u64,
    confirmations: u64,
    scanned_blocks: u64,
    complete: bool,
    event_logs: usize,
    unique_protocol_keys: usize,
}

#[derive(Debug, Clone, Copy)]
struct GroundTruthQuoteBinding {
    l2_block_number: u64,
    block_hash: B256,
    transaction_index: u64,
    action: Option<ActionKind>,
    token: Option<Address>,
    pool: Option<Address>,
    quote_status: QuoteStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QuoteIdentityVersion {
    action: ActionKind,
    token: Address,
    pool: Option<Address>,
    l2_block_number: u64,
    block_hash: B256,
    transaction_index: u64,
}

#[derive(Debug)]
struct ConfirmedGroundTruth {
    by_key: HashMap<(B256, LaunchpadId), GroundTruthQuoteBinding>,
}

impl ConfirmedGroundTruth {
    fn from_records(records: &ReconciliationRecords) -> Result<Self> {
        let window = records
            .ground_truth_window
            .as_ref()
            .context("finalization requires a complete ground-truth coverage manifest")?;
        validate_ground_truth_window(window, &records.evidence)?;
        let mut by_key = HashMap::new();
        for record in &records.evidence {
            if !record.ground_truth_event {
                continue;
            }
            let block = record
                .l2_block_number
                .context("ground-truth evidence has no L2 block number")?;
            let block_hash = record
                .block_hash
                .filter(|hash| *hash != B256::ZERO)
                .context("ground-truth evidence has no canonical block hash")?;
            let transaction_index = record
                .transaction_index
                .context("ground-truth evidence has no transaction index")?;
            if !record.receipt_status || block < window.from_l2_block || block > window.to_l2_block
            {
                anyhow::bail!(
                    "ground-truth evidence is failed or outside coverage for {:?}",
                    (record.tx_hash, record.launchpad)
                );
            }
            if !record.protocol_event_match {
                continue;
            }
            let key = (record.tx_hash, record.launchpad);
            if record.quote_status == QuoteStatus::Available && record.protocol_blocker.is_some() {
                anyhow::bail!("available quote authority has a protocol blocker for {key:?}");
            }
            if record.launchpad == LaunchpadId::Pons
                && record.quote_status == QuoteStatus::Available
                && record.pons_generation != Some(hermes_feed::PonsGeneration::Current)
            {
                anyhow::bail!("Pons quote authority is not from the current generation");
            }
            if by_key
                .insert(
                    key,
                    GroundTruthQuoteBinding {
                        l2_block_number: block,
                        block_hash,
                        transaction_index,
                        action: record.action,
                        token: record.token,
                        pool: record.pool,
                        quote_status: record.quote_status,
                    },
                )
                .is_some()
            {
                anyhow::bail!("duplicate confirmed ground-truth evidence for {key:?}");
            }
        }
        Ok(Self { by_key })
    }

    fn available_quote_matches(
        &self,
        key: (B256, LaunchpadId),
        quote: QuoteIdentityVersion,
    ) -> Option<bool> {
        self.by_key.get(&key).map(|binding| {
            binding.quote_status == QuoteStatus::Available
                && binding.action == Some(quote.action)
                && binding.token == Some(quote.token)
                && binding.pool == quote.pool
                && binding.l2_block_number == quote.l2_block_number
                && binding.block_hash == quote.block_hash
                && binding.transaction_index == quote.transaction_index
        })
    }
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
        let ground_truth = ConfirmedGroundTruth::from_records(&records)?;
        for metrics in reconciliation_metrics(
            &observed_candidates,
            &records.evidence,
            records.ground_truth_window.as_ref(),
        )? {
            println!("{}", serde_json::to_string(&metrics)?);
        }
        for plan in finalized_v3_plans(
            &observed_candidates.feed_sequences,
            &ground_truth,
            records.v3_quotes,
            plan_policy,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_clanker_plans(
            &observed_candidates.feed_sequences,
            &ground_truth,
            records.clanker_quotes,
            plan_policy,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_bankr_plans(
            &observed_candidates.feed_sequences,
            &ground_truth,
            records.bankr_quotes,
            plan_policy,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_pons_plans(
            &observed_candidates.feed_sequences,
            &ground_truth,
            records.pons_quotes,
            plan_policy,
            pons_profile,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_hood_plans(
            &observed_candidates.feed_sequences,
            &ground_truth,
            records.hood_quotes,
            plan_policy,
            &hood_profile,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        return Ok(());
    }
    let mut runtime = PaperFeedRuntime::with_plan_policy(observer, plan_policy)?;
    let mut observed_candidates = ObservedOutputCandidates::default();
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
        let (feed, received_unix_ns): (BroadcastMessage, u64) =
            match value.get("payload").and_then(Value::as_str) {
                Some(payload) => (
                    serde_json::from_str(payload).with_context(|| {
                        format!("decode recorded payload at line {}", index + 1)
                    })?,
                    value
                        .get("received_unix_ns")
                        .and_then(Value::as_u64)
                        .with_context(|| {
                            format!(
                                "recorded payload has no receive timestamp at line {}",
                                index + 1
                            )
                        })?,
                ),
                None => (
                    serde_json::from_value(value)
                        .with_context(|| format!("decode Nitro frame at line {}", index + 1))?,
                    unix_now_ns(),
                ),
            };
        let report = runtime.decode_received_at(&feed, received_unix_ns)?;
        for transaction in &report.transactions {
            observed_candidates
                .feed_transactions
                .entry(transaction.tx_hash)
                .and_modify(|existing| *existing = (*existing).min(transaction.feed_sequence))
                .or_insert(transaction.feed_sequence);
        }
        for observation in &report.observations {
            let key = (observation.tx_hash, observation.launchpad);
            observed_candidates.received_unix_ns.insert(
                key,
                observation.observer_received_unix_ns.unwrap_or_default(),
            );
            observed_candidates
                .observer_latency_ns
                .insert(key, observation.observer_latency_ns.unwrap_or_default());
            observed_candidates
                .feed_sequences
                .insert(key, observation.feed_sequence.unwrap_or_default());
            observed_candidates.claims.insert(
                key,
                ObserverClaim {
                    action: observation.action,
                    predicted_token: observation.predicted_token,
                    predicted_pool: observation.predicted_pool,
                },
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
        let records = read_reconciliation_records(&path)?;
        validate_hood_migration_records(&records.evidence, &records.hood_migrations)?;
        let ground_truth = ConfirmedGroundTruth::from_records(&records)?;
        for metrics in reconciliation_metrics(
            &observed_candidates,
            &records.evidence,
            records.ground_truth_window.as_ref(),
        )? {
            println!("{}", serde_json::to_string(&metrics)?);
        }
        for plan in finalized_v3_plans(
            &observed_candidates.feed_sequences,
            &ground_truth,
            records.v3_quotes,
            plan_policy,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_clanker_plans(
            &observed_candidates.feed_sequences,
            &ground_truth,
            records.clanker_quotes,
            plan_policy,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_bankr_plans(
            &observed_candidates.feed_sequences,
            &ground_truth,
            records.bankr_quotes,
            plan_policy,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_pons_plans(
            &observed_candidates.feed_sequences,
            &ground_truth,
            records.pons_quotes,
            plan_policy,
            pons_profile,
        )? {
            println!("{}", serde_json::to_string(&plan)?);
        }
        for plan in finalized_hood_plans(
            &observed_candidates.feed_sequences,
            &ground_truth,
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
    let mut latencies: HashMap<(B256, LaunchpadId), u64> = HashMap::new();
    let mut sequences: HashMap<(B256, LaunchpadId), u64> = HashMap::new();
    let mut feed_transactions: HashMap<B256, u64> = HashMap::new();
    let mut claims = HashMap::new();
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
        let transactions = value
            .pointer("/report/transactions")
            .and_then(Value::as_array)
            .context("launchpad paper frame has no transaction inventory")?;
        for transaction in transactions {
            let tx_hash: B256 = serde_json::from_value(
                transaction
                    .get("tx_hash")
                    .cloned()
                    .context("feed transaction has no tx_hash")?,
            )?;
            let feed_sequence = transaction
                .get("feed_sequence")
                .and_then(Value::as_u64)
                .context("feed transaction has no feed sequence")?;
            feed_transactions
                .entry(tx_hash)
                .and_modify(|existing| *existing = (*existing).min(feed_sequence))
                .or_insert(feed_sequence);
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
            let observer_latency_ns = observation
                .get("observer_latency_ns")
                .and_then(Value::as_u64)
                .context("observation has no local latency")?;
            let key = (tx_hash, launchpad);
            let claim = ObserverClaim {
                action: observation
                    .get("action")
                    .filter(|value| !value.is_null())
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()?,
                predicted_token: observation
                    .get("predicted_token")
                    .filter(|value| !value.is_null())
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()?,
                predicted_pool: observation
                    .get("predicted_pool")
                    .filter(|value| !value.is_null())
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()?,
            };
            if claims.insert(key, claim).is_some() {
                anyhow::bail!("duplicate observer claim for {key:?}");
            }
            received
                .entry(key)
                .and_modify(|existing| {
                    *existing = (*existing).min(observer_received_unix_ns);
                })
                .or_insert(observer_received_unix_ns);
            sequences.entry(key).or_insert(feed_sequence);
            latencies
                .entry(key)
                .and_modify(|existing| *existing = (*existing).min(observer_latency_ns))
                .or_insert(observer_latency_ns);
        }
    }
    Ok(ObservedOutputCandidates {
        received_unix_ns: received,
        observer_latency_ns: latencies,
        feed_sequences: sequences,
        feed_transactions,
        claims,
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
                Some("launchpad_ground_truth_window") => {
                    if records.ground_truth_window.is_some() {
                        anyhow::bail!("duplicate ground-truth coverage manifest");
                    }
                    records.ground_truth_window =
                        Some(serde_json::from_value(value).with_context(|| {
                            format!(
                                "decode ground-truth window line {} from {}",
                                index + 1,
                                path.display()
                            )
                        })?);
                }
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
                Some("launchpad_reconciliation_evidence") => {
                    records
                        .evidence
                        .push(serde_json::from_value(value).with_context(|| {
                            format!("decode evidence line {} from {}", index + 1, path.display())
                        })?)
                }
                Some(other) => anyhow::bail!(
                    "unknown reconciliation record type {other} at line {}",
                    index + 1
                ),
                None => anyhow::bail!(
                    "reconciliation record has no explicit record_type at line {}",
                    index + 1
                ),
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
                && record.quote_status == QuoteStatus::Blocked
                && record.protocol_blocker.as_deref()
                    == Some("hood_migration_terminal_boundary_unreconciled_v3_quote_unavailable")
        });
        let expected_blocker = if migration.declared_and_actual_liquidity_match {
            "terminal_zero_liquidity_boundary_unreconciled_quote_blocked"
        } else {
            "declared_actual_liquidity_mismatch_and_terminal_boundary_unreconciled"
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
            || migration.pool_initialize_sqrt_price_x96 == U256::ZERO
            || migration.receipt_end_sqrt_price_x96 == U256::ZERO
            || migration.receipt_end_swap_input == U256::ZERO
            || migration.receipt_end_swap_output == U256::ZERO
            || !migration.swap_amounts_reconstructed
            || !migration.terminal_zero_liquidity_boundary_observed
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
                == Some("hood_migration_terminal_boundary_unreconciled_v3_quote_unavailable")
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
    ground_truth: &ConfirmedGroundTruth,
    quotes: Vec<V3ReceiptPaperQuote>,
    policy: PaperPlanPolicy,
) -> Result<Vec<FinalizedV3PaperPlan>> {
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
        let Some(confirmed) = ground_truth.available_quote_matches(
            key,
            QuoteIdentityVersion {
                action: ActionKind::Launch,
                token: quote.market.token,
                pool: Some(quote.market.pool),
                l2_block_number: quote.state_version.l2_block_number,
                block_hash: quote.state_version.block_hash,
                transaction_index: quote.state_version.transaction_index,
            },
        ) else {
            continue;
        };
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
    ground_truth: &ConfirmedGroundTruth,
    quotes: Vec<ClankerReceiptPaperQuote>,
    policy: PaperPlanPolicy,
) -> Result<Vec<FinalizedV3PaperPlan>> {
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
        let Some(confirmed) = ground_truth.available_quote_matches(
            key,
            QuoteIdentityVersion {
                action: ActionKind::Launch,
                token: quote.market.token,
                pool: None,
                l2_block_number: quote.state_version.l2_block_number,
                block_hash: quote.state_version.block_hash,
                transaction_index: quote.state_version.transaction_index,
            },
        ) else {
            continue;
        };
        let replay_policy = ClankerQuotePolicy {
            amount_in: quote.entry.amount_in,
            max_amount_in: policy.max_input_wei,
            slippage_bps: policy.slippage_bps,
        };
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
            || validate_clanker_quote_replay(
                &quote,
                ClankerV4ExpectedProfile::production(),
                replay_policy,
            )
            .is_err()
            || !clanker_quote_profile_is_consistent(&quote)
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

fn clanker_quote_profile_is_consistent(quote: &ClankerReceiptPaperQuote) -> bool {
    let profile = ClankerV4ExpectedProfile::production();
    if profile.validate().is_err() {
        return false;
    }
    let market = &quote.market;
    let shape_matches = match market.liquidity_profile {
        ClankerLiquidityProfile::ExtensionlessSinglePosition => {
            market.extension.is_none()
                && market.extensions_supply == U256::ZERO
                && market.position_count == 1
        }
        ClankerLiquidityProfile::PinnedExtensionFivePosition => {
            market.extension == Some(profile.extension.address)
                && market.extensions_supply != U256::ZERO
                && market.position_count == 5
        }
    };
    let Some(expected_first_eligible) = quote
        .state_version
        .receipt_timestamp
        .checked_add(profile.mev_delay_guard_seconds)
    else {
        return false;
    };
    shape_matches
        && quote.quote_source == "confirmed_receipt_end_clanker_v4_first_eligible_state"
        && quote.sizing_source == "independent_fixed_tiny_weth_policy"
        && quote.execution_blocker == "paper_only_clanker_hook_mev_and_router_execution_not_enabled"
        && market.token != Address::ZERO
        && market.token != hermes_feed::robinhood::WETH
        && market.pool_id != B256::ZERO
        && market.pool_manager == profile.pool_manager.address
        && market.quote_asset == hermes_feed::robinhood::WETH
        && market.hook == profile.hook.address
        && market.locker == profile.locker.address
        && market.mev_module == profile.mev_module.address
        && market.dynamic_fee_flag == hermes_feed::uniswap_v4::DYNAMIC_FEE_FLAG
        && market.tick_spacing == 200
        && market.initialize_log_index < market.last_liquidity_log_index
        && market.last_liquidity_log_index < market.launch_log_index
        && quote.state_version.terminal_log_index == market.launch_log_index
        && quote.state_version.first_eligible_quote_timestamp == expected_first_eligible
        && market.static_fee_config.clanker_fee_ppm <= profile.max_static_fee_ppm
        && market.static_fee_config.paired_fee_ppm <= profile.max_static_fee_ppm
        && market.mev_fee_config.starting_fee_ppm != 0
        && market.mev_fee_config.starting_fee_ppm <= profile.max_mev_fee_ppm
        && market.mev_fee_config.ending_fee_ppm <= market.mev_fee_config.starting_fee_ppm
        && market.mev_fee_config.seconds_to_decay != 0
        && market.mev_fee_config.seconds_to_decay <= profile.max_mev_seconds_to_decay
}

fn finalized_bankr_plans(
    observed_sequences: &HashMap<(B256, LaunchpadId), u64>,
    ground_truth: &ConfirmedGroundTruth,
    quotes: Vec<BankrDopplerReceiptPaperQuote>,
    policy: PaperPlanPolicy,
) -> Result<Vec<FinalizedV3PaperPlan>> {
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
        let Some(confirmed) = ground_truth.available_quote_matches(
            key,
            QuoteIdentityVersion {
                action: ActionKind::Launch,
                token: quote.market.token,
                pool: None,
                l2_block_number: quote.state_version.l2_block_number,
                block_hash: quote.state_version.block_hash,
                transaction_index: quote.state_version.transaction_index,
            },
        ) else {
            continue;
        };
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
    ground_truth: &ConfirmedGroundTruth,
    quotes: Vec<PonsReceiptPaperQuote>,
    policy: PaperPlanPolicy,
    expected_profile: hermes_feed::PonsExpectedProfile,
) -> Result<Vec<FinalizedV3PaperPlan>> {
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
        let Some(confirmed) = ground_truth.available_quote_matches(
            key,
            QuoteIdentityVersion {
                action: ActionKind::Launch,
                token: quote.market.token,
                pool: Some(quote.market.pool),
                l2_block_number: quote.state_version.l2_block_number,
                block_hash: quote.state_version.block_hash,
                transaction_index: quote.state_version.transaction_index,
            },
        ) else {
            continue;
        };
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
    ground_truth: &ConfirmedGroundTruth,
    quotes: Vec<HoodReceiptPaperQuote>,
    policy: PaperPlanPolicy,
    expected_profile: &HoodExpectedProfile,
) -> Result<Vec<FinalizedV3PaperPlan>> {
    expected_profile.validate()?;
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
        let Some(confirmed) = ground_truth.available_quote_matches(
            key,
            QuoteIdentityVersion {
                action: quote.observed.action,
                token: quote.token,
                pool: None,
                l2_block_number: quote.state_version.l2_block_number,
                block_hash: quote.state_version.block_hash,
                transaction_index: quote.state_version.transaction_index,
            },
        ) else {
            continue;
        };
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
    let expected_initialize_tick = match (
        quote.market.create_profile_version,
        quote.market.token < profile.weth.address,
    ) {
        (BankrCreateProfileVersion::CurveTicksV1, true)
        | (BankrCreateProfileVersion::CurveTicksV2, true) => -229_600,
        (BankrCreateProfileVersion::CurveTicksV1, false) => 229_800,
        (BankrCreateProfileVersion::CurveTicksV2, false) => 229_600,
    };
    let Some(expected_delegation) = profile.smart_account.delegation_implementation else {
        return false;
    };
    let valid_envelope = match quote.market.envelope {
        BankrEnvelopeKind::Erc7579 => {
            quote
                .market
                .outer_bundler
                .is_some_and(|bundler| bundler != alloy_primitives::Address::ZERO)
                && quote
                    .market
                    .user_operation_log_index
                    .is_some_and(|index| index > quote.market.launch_log_index)
                && quote.state_version.terminal_log_index
                    == quote.market.user_operation_log_index.unwrap_or_default()
        }
        BankrEnvelopeKind::DirectAirlock => {
            quote.market.outer_bundler.is_none()
                && quote.market.user_operation_log_index.is_none()
                && quote.state_version.terminal_log_index == quote.market.launch_log_index
        }
    };
    if quote.quote_source != "confirmed_receipt_end_bankr_doppler_first_nonzero_state"
        || quote.sizing_source != "independent_fixed_tiny_weth_policy"
        || quote.execution_blocker
            != "paper_only_bankr_rehype_router_permit2_and_account_execution_not_enabled"
        || !valid_envelope
        || quote.market.leader == alloy_primitives::Address::ZERO
        || quote.market.account_designator_hash != profile.smart_account.account.runtime_code_hash
        || quote.market.delegation_implementation != expected_delegation.address
        || quote.market.delegation_runtime_hash != expected_delegation.runtime_code_hash
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

fn validate_ground_truth_window(
    window: &GroundTruthWindow,
    evidence: &[ReconciliationEvidence],
) -> Result<()> {
    let expected_from = window
        .start_head
        .checked_add(1)
        .context("ground-truth start head overflow")?;
    let expected_scanned = if window.to_l2_block < window.from_l2_block {
        if window.cutoff_head != window.start_head {
            anyhow::bail!("ground-truth coverage range is inverted");
        }
        0
    } else {
        window.to_l2_block - window.from_l2_block + 1
    };
    if window.record_type != "launchpad_ground_truth_window"
        || !window.complete
        || window.start_head_hash == B256::ZERO
        || window.cutoff_head_hash == B256::ZERO
        || window.from_l2_block != expected_from
        || window.to_l2_block != window.cutoff_head
        || window.scanned_blocks != expected_scanned
        || window.confirmations == 0
        || window.event_logs < window.unique_protocol_keys
        || (expected_scanned == 0 && (window.event_logs != 0 || window.unique_protocol_keys != 0))
    {
        anyhow::bail!("invalid or incomplete ground-truth coverage manifest");
    }

    let mut keys = HashSet::new();
    let mut truth_keys = 0_usize;
    let mut truth_logs = 0_usize;
    for record in evidence {
        let key = (record.tx_hash, record.launchpad);
        if record.record_type != "launchpad_reconciliation_evidence" {
            anyhow::bail!("invalid reconciliation evidence type for {key:?}");
        }
        if !keys.insert(key) {
            anyhow::bail!("duplicate reconciliation evidence for {key:?}");
        }
        if record.ground_truth_event == record.ground_truth_hits.is_empty() {
            anyhow::bail!("ground-truth flag and exact log hits disagree for {key:?}");
        }
        if !record.ground_truth_event {
            continue;
        }
        truth_keys += 1;
        truth_logs += record.ground_truth_hits.len();
        let block = record
            .l2_block_number
            .context("ground-truth evidence has no L2 block number")?;
        if !record.receipt_status
            || record.block_hash.is_none_or(|hash| hash == B256::ZERO)
            || record.transaction_index.is_none()
            || block < window.from_l2_block
            || block > window.to_l2_block
        {
            anyhow::bail!("invalid canonical ground-truth evidence for {key:?}");
        }
        for hit in &record.ground_truth_hits {
            if hit.l2_block_number != block
                || Some(hit.block_hash) != record.block_hash
                || Some(hit.transaction_index) != record.transaction_index
                || hit.log.address == Address::ZERO
                || hit.log.topics.is_empty()
                || launchpad_for_ground_truth_log(&hit.log) != Some(record.launchpad)
            {
                anyhow::bail!("ground-truth log identity disagrees with receipt for {key:?}");
            }
        }
    }
    if truth_keys != window.unique_protocol_keys {
        anyhow::bail!(
            "ground-truth manifest declares {} keys but evidence contains {truth_keys}",
            window.unique_protocol_keys
        );
    }
    if truth_logs != window.event_logs {
        anyhow::bail!(
            "ground-truth manifest declares {} logs but evidence contains {truth_logs}",
            window.event_logs
        );
    }
    Ok(())
}

fn reconciliation_metrics(
    observed: &ObservedOutputCandidates,
    evidence: &[ReconciliationEvidence],
    window: Option<&GroundTruthWindow>,
) -> Result<Vec<ReconciliationMetrics>> {
    let window = window.context(
        "authoritative reconciliation metrics require a complete ground-truth coverage manifest",
    )?;
    validate_ground_truth_window(window, evidence)?;
    let indexed = evidence
        .iter()
        .map(|record| ((record.tx_hash, record.launchpad), record))
        .collect::<HashMap<_, _>>();
    for key in observed.claims.keys() {
        if !observed.feed_transactions.contains_key(&key.0) {
            anyhow::bail!("observer claim {key:?} is absent from feed transaction inventory");
        }
    }
    for record in evidence {
        let claimed = observed
            .claims
            .contains_key(&(record.tx_hash, record.launchpad));
        if record.observer_claim != claimed {
            anyhow::bail!(
                "reconciliation observer-membership flag disagrees with observer output for {:?}",
                (record.tx_hash, record.launchpad)
            );
        }
    }

    const LAUNCHPADS: [LaunchpadId; 6] = [
        LaunchpadId::Bow,
        LaunchpadId::LaunchHoodV3,
        LaunchpadId::Clanker,
        LaunchpadId::BankrDoppler,
        LaunchpadId::Pons,
        LaunchpadId::HoodFun,
    ];
    let mut rows = Vec::with_capacity(LAUNCHPADS.len());
    for launchpad in LAUNCHPADS {
        let truth = indexed
            .iter()
            .filter(|((_, id), record)| *id == launchpad && record.ground_truth_event)
            .map(|(key, record)| (*key, *record))
            .collect::<HashMap<_, _>>();
        let claims = observed
            .claims
            .iter()
            .filter(|((_, id), _)| *id == launchpad)
            .map(|(key, claim)| (*key, *claim))
            .collect::<HashMap<_, _>>();
        let confirmed_keys = truth
            .keys()
            .filter(|key| claims.contains_key(key))
            .copied()
            .collect::<Vec<_>>();
        let missed_keys = truth
            .keys()
            .filter(|key| !claims.contains_key(key))
            .copied()
            .collect::<Vec<_>>();

        let mut false_positives = 0_usize;
        let mut reverted_attempts = 0_usize;
        let mut out_of_scope = 0_usize;
        let mut unreconciled = 0_usize;
        for key in claims.keys().filter(|key| !truth.contains_key(key)) {
            match indexed.get(key) {
                None => unreconciled += 1,
                Some(record) => match record.l2_block_number {
                    None => unreconciled += 1,
                    Some(block) if block < window.from_l2_block || block > window.to_l2_block => {
                        out_of_scope += 1;
                    }
                    Some(_) if !record.receipt_status => reverted_attempts += 1,
                    Some(_) => false_positives += 1,
                },
            }
        }

        let mut action_eligible = 0_usize;
        let mut action_missing = 0_usize;
        let mut action_matches = 0_usize;
        let mut action_mismatches = 0_usize;
        let mut token_eligible = 0_usize;
        let mut token_missing = 0_usize;
        let mut token_matches = 0_usize;
        let mut token_mismatches = 0_usize;
        let mut pool_eligible = 0_usize;
        let mut pool_missing = 0_usize;
        let mut pool_matches = 0_usize;
        let mut pool_mismatches = 0_usize;
        for key in &confirmed_keys {
            let claim = claims[key];
            let record = truth[key];
            compare_prediction(
                claim.action,
                record.action,
                &mut action_eligible,
                &mut action_missing,
                &mut action_matches,
                &mut action_mismatches,
            );
            compare_prediction(
                claim.predicted_token,
                record.token,
                &mut token_eligible,
                &mut token_missing,
                &mut token_matches,
                &mut token_mismatches,
            );
            compare_prediction(
                claim.predicted_pool,
                record.pool,
                &mut pool_eligible,
                &mut pool_missing,
                &mut pool_matches,
                &mut pool_mismatches,
            );
        }

        let mut observer_latencies = confirmed_keys
            .iter()
            .filter_map(|key| observed.observer_latency_ns.get(key).copied())
            .collect::<Vec<_>>();
        let mut reconciliation_durations = truth
            .values()
            .filter_map(|record| {
                record
                    .reconciliation_completed_unix_ns
                    .checked_sub(record.reconciliation_started_unix_ns)
            })
            .collect::<Vec<_>>();
        observer_latencies.sort_unstable();
        reconciliation_durations.sort_unstable();
        rows.push(ReconciliationMetrics {
            record_type: "launchpad_paper_reconciliation_metrics",
            launchpad,
            coverage_from_l2_block: window.from_l2_block,
            coverage_to_l2_block: window.to_l2_block,
            raw_ground_truth_transactions: truth.len(),
            quote_admitted_ground_truth_transactions: truth
                .values()
                .filter(|record| {
                    record.protocol_event_match && record.quote_status == QuoteStatus::Available
                })
                .count(),
            observer_claims: claims.len(),
            confirmed_observations: confirmed_keys.len(),
            false_positives,
            reverted_attempts,
            missed_transactions: missed_keys.len(),
            detector_misses: missed_keys
                .iter()
                .filter(|key| observed.feed_transactions.contains_key(&key.0))
                .count(),
            feed_coverage_misses: missed_keys
                .iter()
                .filter(|key| !observed.feed_transactions.contains_key(&key.0))
                .count(),
            out_of_scope_observations: out_of_scope,
            unreconciled_observations: unreconciled,
            quote_available: truth
                .values()
                .filter(|record| record.quote_status == QuoteStatus::Available)
                .count(),
            quote_blocked: truth
                .values()
                .filter(|record| record.quote_status == QuoteStatus::Blocked)
                .count(),
            quote_not_applicable: truth
                .values()
                .filter(|record| record.quote_status == QuoteStatus::NotApplicable)
                .count(),
            action_prediction_eligible: action_eligible,
            action_prediction_missing: action_missing,
            action_prediction_matches: action_matches,
            action_prediction_mismatches: action_mismatches,
            token_prediction_eligible: token_eligible,
            token_prediction_missing: token_missing,
            token_prediction_matches: token_matches,
            token_prediction_mismatches: token_mismatches,
            pool_prediction_eligible: pool_eligible,
            pool_prediction_missing: pool_missing,
            pool_prediction_matches: pool_matches,
            pool_prediction_mismatches: pool_mismatches,
            observation_latency_p50_ns: percentile(&observer_latencies, 50),
            observation_latency_p95_ns: percentile(&observer_latencies, 95),
            observation_latency_p99_ns: percentile(&observer_latencies, 99),
            reconciliation_rpc_duration_p50_ns: percentile(&reconciliation_durations, 50),
            reconciliation_rpc_duration_p95_ns: percentile(&reconciliation_durations, 95),
            reconciliation_rpc_duration_p99_ns: percentile(&reconciliation_durations, 99),
        });
    }
    Ok(rows)
}

fn compare_prediction<T: Copy + Eq>(
    predicted: Option<T>,
    actual: Option<T>,
    eligible: &mut usize,
    missing: &mut usize,
    matches: &mut usize,
    mismatches: &mut usize,
) {
    let Some(actual) = actual else {
        return;
    };
    *eligible += 1;
    match predicted {
        None => *missing += 1,
        Some(predicted) if predicted == actual => *matches += 1,
        Some(_) => *mismatches += 1,
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
    use alloy_primitives::{Address, keccak256};
    use hermes_feed::launchpad_adapters::{
        CLANKER_FACTORY, CLANKER_TOKEN_CREATED_TOPIC, DOPPLER_CREATE_EMITTER, DOPPLER_CREATE_TOPIC,
    };
    use hermes_feed::launchpad_ground_truth::{
        BOW_LAUNCHED_SIGNATURE, HOOD_TOKEN_CREATED_SIGNATURE, LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE,
    };
    use hermes_feed::pons::{PONS_CURRENT_FACTORY, PONS_TOKEN_LAUNCHED_TOPIC};
    use hermes_feed::robinhood::{BOW_LAUNCH_FACTORY, LAUNCHHOOD_V3_FACTORY};
    use hermes_feed::tier2_curve::HOOD_FACTORY;
    use hermes_feed::{
        ClankerQuotePolicy, ClankerV4ExpectedProfile, NoxaReceipt, PonsQuotePolicy, RobinhoodBlock,
        RobinhoodTransaction, V3PaperSwapQuote, V3Quote, V3QuoteStateVersion,
        V3ReceiptMarketEvidence, quote_clanker_launch_receipt, quote_pons_launch_receipt,
    };

    use super::*;

    fn quote_authority(
        key: (B256, LaunchpadId),
        action: ActionKind,
        token: Address,
        pool: Option<Address>,
        l2_block_number: u64,
        block_hash: B256,
        transaction_index: u64,
    ) -> ConfirmedGroundTruth {
        ConfirmedGroundTruth {
            by_key: HashMap::from([(
                key,
                GroundTruthQuoteBinding {
                    l2_block_number,
                    block_hash,
                    transaction_index,
                    action: Some(action),
                    token: Some(token),
                    pool,
                    quote_status: QuoteStatus::Available,
                },
            )]),
        }
    }

    fn complete_window(unique_protocol_keys: usize) -> GroundTruthWindow {
        GroundTruthWindow {
            record_type: "launchpad_ground_truth_window".into(),
            start_head: 9,
            start_head_hash: B256::with_last_byte(9),
            cutoff_head: 20,
            cutoff_head_hash: B256::with_last_byte(20),
            from_l2_block: 10,
            to_l2_block: 20,
            confirmations: 2,
            scanned_blocks: 11,
            complete: true,
            event_logs: unique_protocol_keys,
            unique_protocol_keys,
        }
    }

    fn evidence_row(
        key: (B256, LaunchpadId),
        observer_claim: bool,
        ground_truth_event: bool,
        protocol_event_match: bool,
        quote_status: QuoteStatus,
    ) -> ReconciliationEvidence {
        ReconciliationEvidence {
            record_type: "launchpad_reconciliation_evidence".into(),
            tx_hash: key.0,
            launchpad: key.1,
            receipt_status: true,
            protocol_event_match,
            observer_claim,
            ground_truth_event,
            ground_truth_hits: if ground_truth_event {
                vec![GroundTruthHit {
                    l2_block_number: 10,
                    block_hash: B256::with_last_byte(10),
                    transaction_index: 1,
                    log: hermes_feed::noxa_abi::ReceiptLog {
                        address: match key.1 {
                            LaunchpadId::Bow => BOW_LAUNCH_FACTORY,
                            LaunchpadId::LaunchHoodV3 => LAUNCHHOOD_V3_FACTORY,
                            LaunchpadId::Clanker => CLANKER_FACTORY,
                            LaunchpadId::BankrDoppler => DOPPLER_CREATE_EMITTER,
                            LaunchpadId::Pons => PONS_CURRENT_FACTORY,
                            LaunchpadId::HoodFun => HOOD_FACTORY,
                            other => panic!("no ground-truth test log for {other:?}"),
                        },
                        log_index: 0,
                        topics: vec![match key.1 {
                            LaunchpadId::Bow => keccak256(BOW_LAUNCHED_SIGNATURE.as_bytes()),
                            LaunchpadId::LaunchHoodV3 => {
                                keccak256(LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE.as_bytes())
                            }
                            LaunchpadId::Clanker => CLANKER_TOKEN_CREATED_TOPIC,
                            LaunchpadId::BankrDoppler => DOPPLER_CREATE_TOPIC,
                            LaunchpadId::Pons => PONS_TOKEN_LAUNCHED_TOPIC,
                            LaunchpadId::HoodFun => {
                                keccak256(HOOD_TOKEN_CREATED_SIGNATURE.as_bytes())
                            }
                            other => panic!("no ground-truth test topic for {other:?}"),
                        }],
                        data: alloy_primitives::Bytes::new(),
                    },
                }]
            } else {
                Vec::new()
            },
            action: Some(ActionKind::Launch),
            token: Some(Address::with_last_byte(1)),
            pool: Some(Address::with_last_byte(2)),
            quote_status,
            l2_block_number: Some(10),
            block_hash: Some(B256::with_last_byte(10)),
            transaction_index: Some(1),
            reconciliation_started_unix_ns: 100,
            reconciliation_completed_unix_ns: 150,
            pons_generation: (key.1 == LaunchpadId::Pons)
                .then_some(hermes_feed::PonsGeneration::Current),
            protocol_blocker: None,
        }
    }

    #[test]
    fn reconciliation_metrics_separate_confirmed_false_positive_missed_and_unknown() {
        let confirmed_key = (B256::with_last_byte(1), LaunchpadId::Bow);
        let false_positive_key = (B256::with_last_byte(2), LaunchpadId::Clanker);
        let reverted_key = (B256::with_last_byte(6), LaunchpadId::Clanker);
        let unreconciled_key = (B256::with_last_byte(3), LaunchpadId::Pons);
        let missed_key = (B256::with_last_byte(4), LaunchpadId::HoodFun);
        let feed_missed_key = (B256::with_last_byte(5), LaunchpadId::BankrDoppler);
        let observed = ObservedOutputCandidates {
            observer_latency_ns: HashMap::from([
                (confirmed_key, 50),
                (false_positive_key, 60),
                (reverted_key, 65),
                (unreconciled_key, 70),
            ]),
            feed_transactions: HashMap::from([
                (confirmed_key.0, 1),
                (false_positive_key.0, 2),
                (reverted_key.0, 2),
                (unreconciled_key.0, 3),
                (missed_key.0, 4),
            ]),
            claims: HashMap::from([
                (
                    confirmed_key,
                    ObserverClaim {
                        action: Some(ActionKind::Launch),
                        predicted_token: Some(Address::with_last_byte(1)),
                        predicted_pool: Some(Address::with_last_byte(2)),
                    },
                ),
                (
                    false_positive_key,
                    ObserverClaim {
                        action: Some(ActionKind::Launch),
                        predicted_token: None,
                        predicted_pool: None,
                    },
                ),
                (
                    reverted_key,
                    ObserverClaim {
                        action: Some(ActionKind::Launch),
                        predicted_token: None,
                        predicted_pool: None,
                    },
                ),
                (
                    unreconciled_key,
                    ObserverClaim {
                        action: Some(ActionKind::Launch),
                        predicted_token: None,
                        predicted_pool: None,
                    },
                ),
            ]),
            ..ObservedOutputCandidates::default()
        };
        let mut reverted = evidence_row(reverted_key, true, false, false, QuoteStatus::Blocked);
        reverted.receipt_status = false;
        let evidence = [
            evidence_row(confirmed_key, true, true, true, QuoteStatus::Available),
            evidence_row(false_positive_key, true, false, false, QuoteStatus::Blocked),
            reverted,
            evidence_row(missed_key, false, true, true, QuoteStatus::Available),
            evidence_row(feed_missed_key, false, true, true, QuoteStatus::Available),
        ];

        let metrics =
            reconciliation_metrics(&observed, &evidence, Some(&complete_window(3))).unwrap();
        let bow = metrics
            .iter()
            .find(|row| row.launchpad == LaunchpadId::Bow)
            .unwrap();
        assert_eq!(bow.confirmed_observations, 1);
        assert_eq!(bow.observation_latency_p50_ns, Some(50));
        assert_eq!(bow.action_prediction_matches, 1);
        assert_eq!(bow.token_prediction_matches, 1);
        assert_eq!(bow.pool_prediction_matches, 1);
        let clanker = metrics
            .iter()
            .find(|row| row.launchpad == LaunchpadId::Clanker)
            .unwrap();
        assert_eq!(clanker.false_positives, 1);
        assert_eq!(clanker.reverted_attempts, 1);
        let pons = metrics
            .iter()
            .find(|row| row.launchpad == LaunchpadId::Pons)
            .unwrap();
        assert_eq!(pons.unreconciled_observations, 1);
        let hood = metrics
            .iter()
            .find(|row| row.launchpad == LaunchpadId::HoodFun)
            .unwrap();
        assert_eq!(hood.missed_transactions, 1);
        assert_eq!(hood.detector_misses, 1);
        assert_eq!(hood.feed_coverage_misses, 0);
        let bankr = metrics
            .iter()
            .find(|row| row.launchpad == LaunchpadId::BankrDoppler)
            .unwrap();
        assert_eq!(bankr.missed_transactions, 1);
        assert_eq!(bankr.detector_misses, 0);
        assert_eq!(bankr.feed_coverage_misses, 1);
    }

    #[test]
    fn quote_authority_requires_complete_unique_canonical_ground_truth() {
        let key = (B256::with_last_byte(1), LaunchpadId::Bow);
        let row = evidence_row(key, true, true, true, QuoteStatus::Available);
        let records = ReconciliationRecords {
            evidence: vec![row.clone()],
            ground_truth_window: Some(complete_window(1)),
            ..ReconciliationRecords::default()
        };
        let authority = ConfirmedGroundTruth::from_records(&records).unwrap();
        assert_eq!(
            authority.available_quote_matches(
                key,
                QuoteIdentityVersion {
                    action: ActionKind::Launch,
                    token: Address::with_last_byte(1),
                    pool: Some(Address::with_last_byte(2)),
                    l2_block_number: 10,
                    block_hash: B256::with_last_byte(10),
                    transaction_index: 1,
                },
            ),
            Some(true)
        );
        assert_eq!(
            authority.available_quote_matches(
                key,
                QuoteIdentityVersion {
                    action: ActionKind::Launch,
                    token: Address::with_last_byte(1),
                    pool: Some(Address::with_last_byte(2)),
                    l2_block_number: 10,
                    block_hash: B256::with_last_byte(0xee),
                    transaction_index: 1,
                },
            ),
            Some(false)
        );

        let missing_window = ReconciliationRecords {
            evidence: vec![row.clone()],
            ..ReconciliationRecords::default()
        };
        assert!(ConfirmedGroundTruth::from_records(&missing_window).is_err());

        let duplicates = ReconciliationRecords {
            evidence: vec![row.clone(), row.clone()],
            ground_truth_window: Some(complete_window(2)),
            ..ReconciliationRecords::default()
        };
        assert!(ConfirmedGroundTruth::from_records(&duplicates).is_err());

        let mut incomplete_window = complete_window(1);
        incomplete_window.complete = false;
        let incomplete = ReconciliationRecords {
            evidence: vec![row.clone()],
            ground_truth_window: Some(incomplete_window),
            ..ReconciliationRecords::default()
        };
        assert!(ConfirmedGroundTruth::from_records(&incomplete).is_err());

        let mut blocked_available = row.clone();
        blocked_available.protocol_blocker = Some("contradictory blocker".into());
        let blocked_available_records = ReconciliationRecords {
            evidence: vec![blocked_available],
            ground_truth_window: Some(complete_window(1)),
            ..ReconciliationRecords::default()
        };
        assert!(ConfirmedGroundTruth::from_records(&blocked_available_records).is_err());

        let mut cross_paired = row.clone();
        cross_paired.ground_truth_hits[0].log.address = CLANKER_FACTORY;
        cross_paired.ground_truth_hits[0].log.topics = vec![CLANKER_TOKEN_CREATED_TOPIC];
        let cross_paired_records = ReconciliationRecords {
            evidence: vec![cross_paired],
            ground_truth_window: Some(complete_window(1)),
            ..ReconciliationRecords::default()
        };
        assert!(ConfirmedGroundTruth::from_records(&cross_paired_records).is_err());

        let mut blocked = row;
        blocked.quote_status = QuoteStatus::Blocked;
        let blocked_records = ReconciliationRecords {
            evidence: vec![blocked],
            ground_truth_window: Some(complete_window(1)),
            ..ReconciliationRecords::default()
        };
        let blocked_authority = ConfirmedGroundTruth::from_records(&blocked_records).unwrap();
        assert_eq!(
            blocked_authority.available_quote_matches(
                key,
                QuoteIdentityVersion {
                    action: ActionKind::Launch,
                    token: Address::with_last_byte(1),
                    pool: Some(Address::with_last_byte(2)),
                    l2_block_number: 10,
                    block_hash: B256::with_last_byte(10),
                    transaction_index: 1,
                },
            ),
            Some(false)
        );
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

    fn clanker_extensionless_quote_fixture() -> ClankerReceiptPaperQuote {
        let fixture: ClankerLiveFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/clanker-v4-extensionless-live-proof.json"
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

    fn finalize_clanker_quote(
        quote: ClankerReceiptPaperQuote,
    ) -> Result<Vec<FinalizedV3PaperPlan>> {
        let key = (quote.tx_hash, quote.launchpad);
        let ground_truth = quote_authority(
            key,
            ActionKind::Launch,
            quote.market.token,
            None,
            quote.state_version.l2_block_number,
            quote.state_version.block_hash,
            quote.state_version.transaction_index,
        );
        finalized_clanker_plans(
            &HashMap::from([(key, 77)]),
            &ground_truth,
            vec![quote],
            PaperPlanPolicy {
                max_input_wei: U256::from(1_000_u64),
                slippage_bps: 100,
                ..PaperPlanPolicy::default()
            },
        )
    }

    fn bankr_quote_fixture() -> BankrDopplerReceiptPaperQuote {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/bankr-doppler-v2-paper-quote.json"
        ))
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
        let ground_truth = quote_authority(
            key,
            ActionKind::Launch,
            quote.market.token,
            None,
            quote.state_version.l2_block_number,
            quote.state_version.block_hash,
            quote.state_version.transaction_index,
        );
        finalized_bankr_plans(
            &HashMap::from([(key, 88)]),
            &ground_truth,
            vec![quote],
            PaperPlanPolicy {
                max_input_wei: U256::from(1_000_000_000_000_000_u64),
                slippage_bps: 100,
                ..PaperPlanPolicy::default()
            },
        )
    }

    fn finalize_pons_quote(quote: PonsReceiptPaperQuote) -> Result<Vec<FinalizedV3PaperPlan>> {
        let key = (quote.tx_hash, quote.launchpad);
        let ground_truth = quote_authority(
            key,
            ActionKind::Launch,
            quote.market.token,
            Some(quote.market.pool),
            quote.state_version.l2_block_number,
            quote.state_version.block_hash,
            quote.state_version.transaction_index,
        );
        finalized_pons_plans(
            &HashMap::from([(key, 99)]),
            &ground_truth,
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
        let ground_truth = quote_authority(
            key,
            ActionKind::Launch,
            quote.market.token,
            Some(quote.market.pool),
            quote.state_version.l2_block_number,
            quote.state_version.block_hash,
            quote.state_version.transaction_index,
        );
        let plans = finalized_v3_plans(
            &HashMap::from([(key, 42)]),
            &ground_truth,
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
        let ground_truth = quote_authority(
            key,
            ActionKind::Launch,
            quote.market.token,
            Some(quote.market.pool),
            quote.state_version.l2_block_number,
            quote.state_version.block_hash,
            quote.state_version.transaction_index,
        );
        quote.broadcast = true;
        assert!(
            finalized_v3_plans(
                &HashMap::from([(key, 42)]),
                &ground_truth,
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
        let plans = finalize_clanker_quote(quote).unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].status, "quoted_execution_gated");
        assert_eq!(plans[0].feed_sequence, 77);
        assert!(!plans[0].execution_eligible);
        assert!(!plans[0].broadcast);
    }

    #[test]
    fn extensionless_clanker_quote_finalizes_but_profile_tampering_fails_closed() {
        let quote = clanker_extensionless_quote_fixture();
        let plans = finalize_clanker_quote(quote.clone()).unwrap();
        assert_eq!(plans.len(), 1);
        assert!(!plans[0].execution_eligible);
        assert!(!plans[0].broadcast);

        let mut tampered = quote;
        tampered.market.extensions_supply = U256::from(1_u8);
        assert!(finalize_clanker_quote(tampered).is_err());
    }

    #[test]
    fn coherent_clanker_output_and_position_forgeries_cannot_finalize() {
        let quote = clanker_quote_fixture();

        let mut forged_outputs = quote.clone();
        forged_outputs.entry.expected_output += U256::from(1_u8);
        forged_outputs.entry.min_receive = forged_outputs.entry.expected_output;
        forged_outputs.full_position_exit.amount_in = forged_outputs.entry.expected_output;
        forged_outputs.full_position_exit.core_amount_in = forged_outputs.entry.expected_output;
        forged_outputs
            .full_position_exit
            .core_state_after
            .amount_in_requested = forged_outputs.entry.expected_output;
        forged_outputs
            .full_position_exit
            .core_state_after
            .amount_in_consumed = forged_outputs.entry.expected_output;
        forged_outputs.full_position_exit.expected_output += U256::from(1_u8);
        forged_outputs.full_position_exit.min_receive =
            forged_outputs.full_position_exit.expected_output;
        forged_outputs.simulated_round_trip_return_bps += U256::from(1_u8);
        assert!(clanker_quote_profile_is_consistent(&forged_outputs));
        assert!(finalize_clanker_quote(forged_outputs).is_err());

        let mut forged_positions = quote;
        forged_positions.market.positions[0].tick_lower -= 200;
        forged_positions.market.positions[0].liquidity += 1;
        assert!(clanker_quote_profile_is_consistent(&forged_positions));
        assert!(finalize_clanker_quote(forged_positions).is_err());
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
        let ground_truth = quote_authority(
            key,
            quote.observed.action,
            quote.token,
            None,
            quote.state_version.l2_block_number,
            quote.state_version.block_hash,
            quote.state_version.transaction_index,
        );
        finalized_hood_plans(
            &HashMap::from([(key, 71)]),
            &ground_truth,
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
        let ground_truth = quote_authority(
            key,
            quote.observed.action,
            quote.token,
            None,
            quote.state_version.l2_block_number,
            quote.state_version.block_hash,
            quote.state_version.transaction_index,
        );
        let mut profile = HoodExpectedProfile::production();
        profile.identities[0].runtime_hash = B256::with_last_byte(0xee);
        assert!(
            finalized_hood_plans(
                &HashMap::from([(key, 1)]),
                &ground_truth,
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
            "record_type": "launchpad_reconciliation_evidence",
            "tx_hash": tx_hash,
            "launchpad": "hood_fun",
            "receipt_status": true,
            "protocol_event_match": true,
            "observer_claim": true,
            "ground_truth_event": true,
            "ground_truth_hits": [{
                "l2_block_number": 10,
                "block_hash": B256::with_last_byte(10),
                "transaction_index": 1,
                "log": {
                    "address": hermes_feed::tier2_curve::HOOD_FACTORY,
                    "log_index": 0,
                    "topics": [B256::with_last_byte(1)],
                    "data": "0x"
                }
            }],
            "action": "buy",
            "token": Address::with_last_byte(1),
            "pool": Address::with_last_byte(2),
            "quote_status": "blocked",
            "l2_block_number": 10,
            "block_hash": B256::with_last_byte(10),
            "transaction_index": 1,
            "reconciliation_started_unix_ns": 1,
            "reconciliation_completed_unix_ns": 9,
            "protocol_blocker": "hood_migration_terminal_boundary_unreconciled_v3_quote_unavailable"
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
            "receipt_end_sqrt_price_x96": "8",
            "receipt_end_tick": -2,
            "receipt_end_liquidity": "0",
            "receipt_end_swap_log_index": 10,
            "receipt_end_swap_input": "11",
            "receipt_end_swap_output": "12",
            "swap_amounts_reconstructed": true,
            "terminal_zero_liquidity_boundary_observed": true,
            "expected_profile_validated": true,
            "receipt_topology_verified": true,
            "pool_state_reconciled": false,
            "v3_quote_available": false,
            "execution_eligible": false,
            "execution_blocker": "declared_actual_liquidity_mismatch_and_terminal_boundary_unreconciled",
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

        let mut out_of_scope_evidence = records.evidence.clone();
        out_of_scope_evidence[0].ground_truth_event = false;
        out_of_scope_evidence[0].ground_truth_hits.clear();
        validate_hood_migration_records(&out_of_scope_evidence, &records.hood_migrations).unwrap();
        let out_of_scope_records = ReconciliationRecords {
            evidence: out_of_scope_evidence,
            hood_migrations: records.hood_migrations.clone(),
            ground_truth_window: Some(complete_window(0)),
            ..ReconciliationRecords::default()
        };
        let authority = ConfirmedGroundTruth::from_records(&out_of_scope_records).unwrap();
        assert!(authority.by_key.is_empty());

        let mut forged = records.hood_migrations.clone();
        forged[0].declared_and_actual_liquidity_match = true;
        forged[0].execution_blocker =
            "terminal_zero_liquidity_boundary_unreconciled_quote_blocked".into();
        assert!(validate_hood_migration_records(&records.evidence, &forged).is_err());
    }
}
