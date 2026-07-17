use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alloy_primitives::{B256, U256, keccak256};
use anyhow::{Context, Result, bail};
use clap::Parser;
use futures_util::{StreamExt, stream};
use hermes_feed::flap_abi::{decode_flap_token_bought, decode_flap_token_created};
use hermes_feed::launchpad_adapter::{ActionKind, LaunchpadId};
use hermes_feed::launchpad_adapters::{
    CLANKER_FACTORY, CLANKER_TOKEN_CREATED_TOPIC, DOPPLER_CREATE_EMITTER, DOPPLER_CREATE_TOPIC,
    KLIK_FACTORY, KLIK_TOKEN_CREATED_TOPIC,
};
use hermes_feed::launchpad_ground_truth::{
    BOW_LAUNCHED_SIGNATURE, HOOD_TOKEN_CREATED_SIGNATURE, HOOD_TRADE_SIGNATURE,
    LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE, launchpad_for_ground_truth_log,
};
use hermes_feed::noxa_abi::{ReceiptLog, decode_token_launched};
use hermes_feed::paper_observer::{
    PaperExpectedPins, PaperLaunchpadObserver, PaperObservedStartupSnapshot,
};
use hermes_feed::pons::{PONS_CURRENT_FACTORY, PONS_LEGACY_FACTORY, PONS_TOKEN_LAUNCHED_TOPIC};
use hermes_feed::pons_receipt_quote::pons_launch_event_identity;
use hermes_feed::robinhood::{
    ACTIVE_NOXA_LAUNCH_FACTORY, BOW_LAUNCH_FACTORY, CHAIN_ID, LAUNCHHOOD_V3_FACTORY,
    NOXA_LAUNCH_FACTORY, PUBLIC_RPC_URL,
};
use hermes_feed::tier2_curve::HOOD_FACTORY;
use hermes_feed::{
    BankrDopplerExpectedProfile, BankrDopplerQuotePolicy, BankrDopplerReceiptPaperQuote,
    ClankerQuotePolicy, ClankerReceiptPaperQuote, ClankerV4ExpectedProfile, HoodExpectedProfile,
    HoodMigrationEvidence, HoodQuotePolicy, HoodReceiptPaperQuote, NoxaRpcClient, PonsQuoteError,
    PonsQuotePolicy, PonsReceiptPaperQuote, V3ReceiptPaperQuote, V3ReceiptQuotePolicy,
    quote_bankr_doppler_launch_receipt_at_receipt_block, quote_clanker_launch_receipt,
    quote_hood_curve_receipt, quote_pons_launch_receipt, quote_v3_launch_receipt,
    verify_hood_graduation_receipt,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::{Instant, sleep};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Read-only receipt/event reconciler for launchpad paper observations"
)]
struct Cli {
    /// JSONL emitted by hermes-launchpad-paper.
    #[arg(long)]
    input: PathBuf,
    /// Independently reviewed protocol-owned expected pins.
    #[arg(long)]
    expected_pins: PathBuf,
    /// Fresh startup runtime snapshot, kept separate from expected pins.
    #[arg(long)]
    observed_startup_snapshot: PathBuf,
    #[arg(long, default_value = PUBLIC_RPC_URL)]
    rpc_url: String,
    #[arg(long, default_value_t = 30)]
    receipt_timeout_seconds: u64,
    #[arg(long, default_value_t = 250)]
    poll_interval_ms: u64,
    #[arg(long, default_value_t = 8)]
    concurrency: usize,
    /// Independent fixed paper entry size used only for V3 quote evidence.
    #[arg(long, default_value_t = 1_000_000_000_000_000_u64)]
    paper_amount_in_wei: u64,
    #[arg(long, default_value_t = 10_000_000_000_000_000_u64)]
    paper_max_amount_in_wei: u64,
    #[arg(long, default_value_t = 100)]
    paper_slippage_bps: u16,
    /// Canonical L2 head sampled after the observer and producer were ready.
    /// Ground truth scans the half-open session range (start, cutoff].
    #[arg(
        long,
        requires = "ground_truth_cutoff_head",
        requires = "ground_truth_start_hash"
    )]
    ground_truth_start_head: Option<u64>,
    /// Canonical hash captured in the same response as the start head number.
    #[arg(long, requires = "ground_truth_start_head")]
    ground_truth_start_hash: Option<B256>,
    /// Latest canonical L2 head sampled while the producer was still alive.
    #[arg(
        long,
        requires = "ground_truth_start_head",
        requires = "ground_truth_cutoff_hash"
    )]
    ground_truth_cutoff_head: Option<u64>,
    /// Canonical hash captured in the same response as the cutoff head number.
    #[arg(long, requires = "ground_truth_cutoff_head")]
    ground_truth_cutoff_hash: Option<B256>,
    #[arg(long, default_value_t = 2)]
    ground_truth_confirmations: u64,
    #[arg(long, default_value_t = 60)]
    ground_truth_confirmation_timeout_seconds: u64,
    /// Fail closed instead of issuing an unexpectedly broad eth_getLogs query.
    #[arg(long, default_value_t = 20_000)]
    max_ground_truth_blocks: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservedCandidate {
    tx_hash: B256,
    launchpad: LaunchpadId,
    observer_claim: bool,
    ground_truth_event: bool,
    ground_truth_hits: Vec<GroundTruthHit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReconciliationRequest {
    tx_hash: B256,
    launchpad: LaunchpadId,
    feed_sequence: u64,
    l1_block_number: u64,
    l1_timestamp: u64,
    evidence_source: EvidenceSource,
    initial_decision_dependency: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceSource {
    IndependentReceiptAndProtocolEvents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
struct ObservationIdentity {
    tx_hash: B256,
    launchpad: LaunchpadId,
    feed_sequence: u64,
    l1_block_number: u64,
    l1_timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct GroundTruthHit {
    l2_block_number: u64,
    block_hash: B256,
    transaction_index: u64,
    log: ReceiptLog,
}

#[derive(Debug)]
struct ObserverInput {
    candidates: HashMap<(B256, LaunchpadId), ObservedCandidate>,
}

#[derive(Debug, Clone, Serialize)]
struct GroundTruthWindow {
    record_type: &'static str,
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
struct GroundTruthScanConfig {
    start_head: u64,
    start_hash: B256,
    cutoff_head: u64,
    cutoff_hash: B256,
    confirmations: u64,
    confirmation_timeout: Duration,
    max_blocks: u64,
}

#[derive(Debug, Clone, Serialize)]
struct ReconciliationEvidence {
    record_type: &'static str,
    tx_hash: B256,
    launchpad: LaunchpadId,
    receipt_status: bool,
    protocol_event_match: bool,
    observer_claim: bool,
    ground_truth_event: bool,
    ground_truth_hits: Vec<GroundTruthHit>,
    action: Option<ActionKind>,
    token: Option<alloy_primitives::Address>,
    pool: Option<alloy_primitives::Address>,
    pool_id: Option<B256>,
    quote_status: QuoteStatus,
    l2_block_number: Option<u64>,
    block_hash: Option<B256>,
    transaction_index: Option<u64>,
    reconciliation_started_unix_ns: u64,
    reconciliation_completed_unix_ns: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pons_generation: Option<hermes_feed::PonsGeneration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol_blocker: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum QuoteStatus {
    Available,
    Blocked,
    NotApplicable,
}

#[derive(Debug)]
struct ReconciledCandidate {
    evidence: ReconciliationEvidence,
    v3_quote: Option<V3ReceiptPaperQuote>,
    clanker_quote: Option<ClankerReceiptPaperQuote>,
    bankr_quote: Option<BankrDopplerReceiptPaperQuote>,
    pons_quote: Option<PonsReceiptPaperQuote>,
    hood_quote: Option<HoodReceiptPaperQuote>,
    hood_migration: Option<HoodMigrationEvidence>,
}

#[derive(Clone)]
struct ReconcileProfiles {
    clanker: Option<ClankerV4ExpectedProfile>,
    bankr: Option<BankrDopplerExpectedProfile>,
    pons: hermes_feed::PonsExpectedProfile,
    hood: HoodExpectedProfile,
}

struct PonsReconciliationOutcome {
    generation: Option<hermes_feed::PonsGeneration>,
    quote: Option<PonsReceiptPaperQuote>,
    blocker: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    if args.expected_pins.canonicalize()? == args.observed_startup_snapshot.canonicalize()? {
        bail!("expected pins and observed startup snapshot must be separate files");
    }
    if args.receipt_timeout_seconds == 0
        || args.poll_interval_ms == 0
        || args.concurrency == 0
        || args.paper_amount_in_wei == 0
        || args.paper_amount_in_wei > args.paper_max_amount_in_wei
        || args.paper_slippage_bps >= 10_000
    {
        bail!("timeout, poll interval, and concurrency must be non-zero");
    }
    let observer_input = read_observer_input(&args.input)?;
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
    PaperLaunchpadObserver::from_startup_snapshots(expected.clone(), observed)?;
    let clanker_profile = expected
        .clanker_v4
        .map(|configured| configured.expected_profile())
        .transpose()?;
    let bankr_profile = expected
        .bankr_doppler_v4
        .map(|configured| configured.expected_profile())
        .transpose()?;
    let pons_profile = expected.pons_v3.expected_profile()?;
    let hood_profile = expected
        .hood_curve
        .as_ref()
        .context("complete reviewed Hood profile is required")?;
    hood_profile.validate()?;
    let profiles = ReconcileProfiles {
        clanker: clanker_profile,
        bankr: bankr_profile,
        pons: pons_profile,
        hood: hood_profile.clone(),
    };
    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    let chain_id = rpc.chain_id().await?;
    if chain_id != CHAIN_ID {
        bail!("RPC chain ID {chain_id} does not match Robinhood {CHAIN_ID}");
    }
    let timeout = Duration::from_secs(args.receipt_timeout_seconds);
    let poll_interval = Duration::from_millis(args.poll_interval_ms);
    let quote_policy = V3ReceiptQuotePolicy {
        amount_in: U256::from(args.paper_amount_in_wei),
        max_amount_in: U256::from(args.paper_max_amount_in_wei),
        slippage_bps: args.paper_slippage_bps,
    };
    let (candidates, ground_truth_window) = match (
        args.ground_truth_start_head,
        args.ground_truth_start_hash,
        args.ground_truth_cutoff_head,
        args.ground_truth_cutoff_hash,
    ) {
        (Some(start), Some(start_hash), Some(cutoff), Some(cutoff_hash)) => {
            augment_with_ground_truth(
                &rpc,
                observer_input,
                GroundTruthScanConfig {
                    start_head: start,
                    start_hash,
                    cutoff_head: cutoff,
                    cutoff_hash,
                    confirmations: args.ground_truth_confirmations,
                    confirmation_timeout: Duration::from_secs(
                        args.ground_truth_confirmation_timeout_seconds,
                    ),
                    max_blocks: args.max_ground_truth_blocks,
                },
            )
            .await?
        }
        (None, None, None, None) => (observer_input.candidates, None),
        _ => bail!("ground-truth start/cutoff numbers and hashes must be supplied together"),
    };
    if let Some(window) = ground_truth_window {
        println!("{}", serde_json::to_string(&window)?);
    }
    let mut reconciled = stream::iter(candidates.into_values().map(|candidate| {
        let rpc = rpc.clone();
        let profiles = profiles.clone();
        async move {
            reconcile_candidate(
                &rpc,
                candidate,
                timeout,
                poll_interval,
                quote_policy,
                profiles,
            )
            .await
        }
    }))
    .buffer_unordered(args.concurrency);

    while let Some(result) = reconciled.next().await {
        let result = result?;
        println!("{}", serde_json::to_string(&result.evidence)?);
        if let Some(quote) = result.v3_quote {
            println!("{}", serde_json::to_string(&quote)?);
        }
        if let Some(quote) = result.clanker_quote {
            println!("{}", serde_json::to_string(&quote)?);
        }
        if let Some(quote) = result.bankr_quote {
            println!("{}", serde_json::to_string(&quote)?);
        }
        if let Some(quote) = result.pons_quote {
            println!("{}", serde_json::to_string(&quote)?);
        }
        if let Some(quote) = result.hood_quote {
            println!("{}", serde_json::to_string(&quote)?);
        }
        if let Some(migration) = result.hood_migration {
            println!("{}", serde_json::to_string(&migration)?);
        }
    }
    Ok(())
}

fn read_observer_input(path: &Path) -> Result<ObserverInput> {
    let input = BufReader::new(
        File::open(path).with_context(|| format!("open observer JSONL {}", path.display()))?,
    );
    read_observer_input_from_reader(input)
        .with_context(|| format!("validate observer JSONL {}", path.display()))
}

fn read_observer_input_from_reader(input: impl BufRead) -> Result<ObserverInput> {
    let mut candidates = HashMap::new();
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
        let observations: Vec<ObservationIdentity> = serde_json::from_value(
            value
                .pointer("/report/observations")
                .cloned()
                .context("launchpad paper frame has no observations array")?,
        )
        .with_context(|| format!("decode observations on observer line {}", index + 1))?;
        let requests: Vec<ReconciliationRequest> = serde_json::from_value(
            value
                .pointer("/report/reconciliation_requests")
                .cloned()
                .context("launchpad paper frame has no reconciliation_requests array")?,
        )
        .with_context(|| {
            format!(
                "decode reconciliation requests on observer line {}",
                index + 1
            )
        })?;
        let mut observation_by_key = HashMap::new();
        for observation in observations {
            let key = (observation.tx_hash, observation.launchpad);
            if observation.feed_sequence == 0
                || observation.l1_block_number == 0
                || observation.l1_timestamp == 0
            {
                bail!("observer candidate {key:?} has incomplete feed provenance");
            }
            if observation_by_key.insert(key, observation).is_some() {
                bail!("duplicate observer observation {key:?}");
            }
        }
        let mut requested_keys = HashSet::new();
        for request in requests {
            let key = (request.tx_hash, request.launchpad);
            if request.evidence_source != EvidenceSource::IndependentReceiptAndProtocolEvents {
                bail!("reconciliation request {key:?} has an unsupported evidence source");
            }
            if request.initial_decision_dependency {
                bail!("reconciliation request {key:?} is an initial decision dependency");
            }
            let observation = observation_by_key
                .get(&key)
                .with_context(|| format!("reconciliation request {key:?} has no observation"))?;
            if request.feed_sequence != observation.feed_sequence
                || request.l1_block_number != observation.l1_block_number
                || request.l1_timestamp != observation.l1_timestamp
            {
                bail!("reconciliation request {key:?} feed provenance disagrees with observation");
            }
            if !requested_keys.insert(key) {
                bail!("duplicate reconciliation request {key:?}");
            }
            let candidate = ObservedCandidate {
                tx_hash: request.tx_hash,
                launchpad: request.launchpad,
                observer_claim: true,
                ground_truth_event: false,
                ground_truth_hits: Vec::new(),
            };
            let key = (candidate.tx_hash, candidate.launchpad);
            if candidates.insert(key, candidate).is_some() {
                bail!("duplicate observer candidate {key:?}");
            }
        }
        if requested_keys.len() != observation_by_key.len() {
            let missing = observation_by_key
                .keys()
                .find(|key| !requested_keys.contains(key))
                .expect("different set sizes imply a missing request");
            bail!("observer observation {missing:?} has no reconciliation request");
        }
    }
    Ok(ObserverInput { candidates })
}

async fn augment_with_ground_truth(
    rpc: &NoxaRpcClient,
    mut input: ObserverInput,
    scan: GroundTruthScanConfig,
) -> Result<(
    HashMap<(B256, LaunchpadId), ObservedCandidate>,
    Option<GroundTruthWindow>,
)> {
    let from_l2_block = scan
        .start_head
        .checked_add(1)
        .context("ground-truth start head overflow")?;
    if scan.cutoff_head < scan.start_head
        || scan.cutoff_head.saturating_sub(scan.start_head) > scan.max_blocks
        || scan.confirmation_timeout.is_zero()
    {
        bail!("ground-truth anchored L2 range is inverted or exceeds the configured bound");
    }
    let required_head = scan
        .cutoff_head
        .checked_add(scan.confirmations)
        .context("ground-truth confirmation height overflow")?;
    let confirmation_deadline = Instant::now() + scan.confirmation_timeout;
    loop {
        if rpc.latest_block_number().await? >= required_head {
            break;
        }
        if Instant::now() >= confirmation_deadline {
            bail!("ground-truth cutoff did not reach the required confirmations");
        }
        sleep(Duration::from_millis(250)).await;
    }
    let start_anchor = rpc.block_by_number(scan.start_head).await?;
    let cutoff_anchor = rpc.block_by_number(scan.cutoff_head).await?;
    if start_anchor.hash != scan.start_hash || cutoff_anchor.hash != scan.cutoff_hash {
        bail!("ground-truth head hash changed between sampling and collection");
    }

    let addresses = [
        BOW_LAUNCH_FACTORY,
        LAUNCHHOOD_V3_FACTORY,
        CLANKER_FACTORY,
        DOPPLER_CREATE_EMITTER,
        PONS_CURRENT_FACTORY,
        PONS_LEGACY_FACTORY,
        HOOD_FACTORY,
    ];
    let topics = [
        keccak256(BOW_LAUNCHED_SIGNATURE.as_bytes()),
        keccak256(LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE.as_bytes()),
        CLANKER_TOKEN_CREATED_TOPIC,
        DOPPLER_CREATE_TOPIC,
        PONS_TOKEN_LAUNCHED_TOPIC,
        keccak256(HOOD_TOKEN_CREATED_SIGNATURE.as_bytes()),
        keccak256(HOOD_TRADE_SIGNATURE.as_bytes()),
    ];
    let logs = if from_l2_block <= scan.cutoff_head {
        rpc.protocol_event_logs(&addresses, &topics, from_l2_block, scan.cutoff_head)
            .await?
    } else {
        Vec::new()
    };
    let mut exact_event_logs = 0_usize;
    let mut seen_logs = HashSet::new();
    for log in logs {
        let Some(launchpad) = launchpad_for_ground_truth_log(&log.log) else {
            continue;
        };
        let hit = GroundTruthHit {
            l2_block_number: log.l2_block_number,
            block_hash: log.block_hash,
            transaction_index: log.transaction_index,
            log: log.log,
        };
        if !seen_logs.insert((
            log.block_hash,
            log.transaction_hash,
            launchpad,
            hit.log.log_index,
            hit.log.address,
            hit.log.topics.first().copied(),
        )) {
            continue;
        }
        exact_event_logs += 1;
        let key = (log.transaction_hash, launchpad);
        input
            .candidates
            .entry(key)
            .and_modify(|candidate| {
                candidate.ground_truth_event = true;
                candidate.ground_truth_hits.push(hit.clone());
            })
            .or_insert(ObservedCandidate {
                tx_hash: log.transaction_hash,
                launchpad,
                observer_claim: false,
                ground_truth_event: true,
                ground_truth_hits: vec![hit],
            });
    }
    let stable_start = rpc.block_by_number(scan.start_head).await?;
    let stable_cutoff = rpc.block_by_number(scan.cutoff_head).await?;
    if stable_start.hash != start_anchor.hash || stable_cutoff.hash != cutoff_anchor.hash {
        bail!("ground-truth anchor reorged during event collection");
    }
    let unique_protocol_keys = input
        .candidates
        .values()
        .filter(|candidate| candidate.ground_truth_event)
        .count();
    Ok((
        input.candidates,
        Some(GroundTruthWindow {
            record_type: "launchpad_ground_truth_window",
            start_head: scan.start_head,
            start_head_hash: start_anchor.hash,
            cutoff_head: scan.cutoff_head,
            cutoff_head_hash: cutoff_anchor.hash,
            from_l2_block,
            to_l2_block: scan.cutoff_head,
            confirmations: scan.confirmations,
            scanned_blocks: scan.cutoff_head.saturating_sub(scan.start_head),
            complete: true,
            event_logs: exact_event_logs,
            unique_protocol_keys,
        }),
    ))
}

async fn reconcile_candidate(
    rpc: &NoxaRpcClient,
    candidate: ObservedCandidate,
    timeout: Duration,
    poll_interval: Duration,
    quote_policy: V3ReceiptQuotePolicy,
    profiles: ReconcileProfiles,
) -> Result<ReconciledCandidate> {
    let reconciliation_started_unix_ns = unix_now_ns();
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(receipt) = rpc.receipt(candidate.tx_hash).await? {
            if receipt.transaction_hash != candidate.tx_hash {
                bail!(
                    "receipt transaction hash mismatch for {}",
                    candidate.tx_hash
                );
            }
            validate_ground_truth_receipt_binding(&candidate, &receipt)?;
            let canonical_receipt_block = rpc.block_by_number(receipt.l2_block_number).await?;
            if canonical_receipt_block.hash != receipt.block_hash {
                bail!(
                    "receipt block is no longer canonical for {}",
                    candidate.tx_hash
                );
            }
            let mut protocol_match =
                receipt.status && protocol_event_match(candidate.launchpad, &receipt.logs);
            let mut v3_quote = None;
            let mut clanker_quote = None;
            let mut bankr_quote = None;
            let mut pons_quote = None;
            let mut hood_quote = None;
            let mut hood_migration = None;
            let mut pons_generation = None;
            let mut protocol_blocker = None;
            let mut truth_action = None;
            let mut truth_token = None;
            let mut truth_pool = None;
            let mut truth_pool_id = None;
            let mut quote_status = QuoteStatus::NotApplicable;
            if receipt.status
                && matches!(
                    candidate.launchpad,
                    LaunchpadId::Bow | LaunchpadId::LaunchHoodV3
                )
            {
                let transaction = rpc
                    .transaction_by_hash(candidate.tx_hash)
                    .await?
                    .with_context(|| format!("missing transaction {}", candidate.tx_hash))?;
                match quote_v3_launch_receipt(
                    &transaction,
                    &receipt,
                    candidate.launchpad,
                    quote_policy,
                ) {
                    Ok(quote) => {
                        protocol_match = true;
                        truth_action = Some(ActionKind::Launch);
                        truth_token = Some(quote.market.token);
                        truth_pool = Some(quote.market.pool);
                        quote_status = QuoteStatus::Available;
                        v3_quote = Some(quote);
                    }
                    Err(error) => {
                        quote_status = QuoteStatus::Blocked;
                        protocol_blocker = Some(format!("v3_strict_quote:{error}"));
                    }
                }
            }
            if receipt.status && candidate.launchpad == LaunchpadId::Clanker {
                let transaction = rpc
                    .transaction_by_hash(candidate.tx_hash)
                    .await?
                    .with_context(|| format!("missing transaction {}", candidate.tx_hash))?;
                let block = rpc.block_by_number(receipt.l2_block_number).await?;
                if let Some(profile) = profiles.clanker {
                    match quote_clanker_launch_receipt(
                        &transaction,
                        &receipt,
                        &block,
                        profile,
                        ClankerQuotePolicy {
                            amount_in: quote_policy.amount_in,
                            max_amount_in: quote_policy.max_amount_in,
                            slippage_bps: quote_policy.slippage_bps,
                        },
                    ) {
                        Ok(quote) => {
                            protocol_match = true;
                            truth_action = Some(ActionKind::Launch);
                            truth_token = Some(quote.market.token);
                            quote_status = QuoteStatus::Available;
                            clanker_quote = Some(quote);
                        }
                        Err(error) => {
                            quote_status = QuoteStatus::Blocked;
                            protocol_blocker = Some(format!("clanker_strict_quote:{error}"));
                        }
                    }
                } else {
                    quote_status = QuoteStatus::NotApplicable;
                    protocol_blocker = Some("clanker_expected_profile_not_configured".into());
                }
            }
            if receipt.status && candidate.launchpad == LaunchpadId::BankrDoppler {
                let transaction = rpc
                    .transaction_by_hash(candidate.tx_hash)
                    .await?
                    .with_context(|| format!("missing transaction {}", candidate.tx_hash))?;
                let block = rpc.block_by_number(receipt.l2_block_number).await?;
                if let Some(profile) = profiles.bankr {
                    match quote_bankr_doppler_launch_receipt_at_receipt_block(
                        rpc,
                        &transaction,
                        &receipt,
                        &block,
                        profile,
                        BankrDopplerQuotePolicy {
                            amount_in: quote_policy.amount_in,
                            max_amount_in: quote_policy.max_amount_in,
                            slippage_bps: quote_policy.slippage_bps,
                        },
                    )
                    .await
                    {
                        Ok(quote) => {
                            protocol_match = true;
                            truth_action = Some(ActionKind::Launch);
                            truth_token = Some(quote.market.token);
                            truth_pool_id = Some(quote.market.pool_id);
                            quote_status = QuoteStatus::Available;
                            bankr_quote = Some(quote);
                        }
                        Err(error) => {
                            quote_status = QuoteStatus::Blocked;
                            protocol_blocker = Some(format!("bankr_strict_quote:{error}"));
                        }
                    }
                } else {
                    quote_status = QuoteStatus::NotApplicable;
                    protocol_blocker = Some("bankr_expected_profile_not_configured".into());
                }
            }
            if receipt.status && candidate.launchpad == LaunchpadId::Pons {
                if let Some((token, pool)) =
                    receipt.logs.iter().find_map(pons_launch_event_identity)
                {
                    truth_action = Some(ActionKind::Launch);
                    truth_token = Some(token);
                    truth_pool = Some(pool);
                }
                let transaction = rpc
                    .transaction_by_hash(candidate.tx_hash)
                    .await?
                    .with_context(|| format!("missing transaction {}", candidate.tx_hash))?;
                let outcome =
                    strict_pons_reconciliation(&transaction, &receipt, profiles.pons, quote_policy);
                pons_generation = outcome.generation;
                pons_quote = outcome.quote;
                protocol_blocker = outcome.blocker;
                quote_status = match (pons_quote.is_some(), pons_generation) {
                    (true, _) => QuoteStatus::Available,
                    (false, Some(hermes_feed::PonsGeneration::Legacy)) => {
                        QuoteStatus::NotApplicable
                    }
                    (false, _) => QuoteStatus::Blocked,
                };
                if let Some(quote) = &pons_quote
                    && (truth_token != Some(quote.market.token)
                        || truth_pool != Some(quote.market.pool))
                {
                    bail!("Pons event identity disagrees with strict quote identity");
                }
            }
            if receipt.status && candidate.launchpad == LaunchpadId::HoodFun {
                let transaction = rpc
                    .transaction_by_hash(candidate.tx_hash)
                    .await?
                    .with_context(|| format!("missing transaction {}", candidate.tx_hash))?;
                let block = rpc.block_by_number(receipt.l2_block_number).await?;
                match hood_token_from_receipt(&receipt.logs) {
                    Some(token) => {
                        let snapshot = rpc
                            .hood_market_snapshot_at(HOOD_FACTORY, token, receipt.l2_block_number)
                            .await?;
                        let pre = if snapshot.curve.graduated || snapshot.curve.migrated {
                            Some(
                                rpc.hood_market_snapshot_at(
                                    HOOD_FACTORY,
                                    token,
                                    receipt
                                        .l2_block_number
                                        .checked_sub(1)
                                        .context("Hood graduation block has no predecessor")?,
                                )
                                .await?,
                            )
                        } else {
                            None
                        };
                        let stable_block = rpc.block_by_number(receipt.l2_block_number).await?;
                        if stable_block.hash != block.hash {
                            bail!("Hood block reorged during fixed-block snapshot");
                        } else if let Some(pre) = pre {
                            match verify_hood_graduation_receipt(
                                &transaction,
                                &receipt,
                                &block,
                                &pre,
                                &snapshot,
                                &profiles.hood,
                            ) {
                                Ok(evidence) => {
                                    protocol_match = true;
                                    truth_action = Some(ActionKind::Buy);
                                    truth_token = Some(evidence.token);
                                    truth_pool = Some(evidence.pool);
                                    quote_status = QuoteStatus::Blocked;
                                    protocol_blocker = Some(
                                        "hood_migration_terminal_boundary_unreconciled_v3_quote_unavailable".into(),
                                    );
                                    hood_migration = Some(evidence);
                                }
                                Err(error) => {
                                    quote_status = QuoteStatus::Blocked;
                                    protocol_blocker =
                                        Some(format!("hood_migration_verification:{error}"));
                                }
                            }
                        } else {
                            match quote_hood_curve_receipt(
                                &transaction,
                                &receipt,
                                &block,
                                &snapshot,
                                profiles.hood.semantic,
                                HoodQuotePolicy {
                                    amount_in: quote_policy.amount_in,
                                    max_amount_in: quote_policy.max_amount_in,
                                    slippage_bps: quote_policy.slippage_bps,
                                },
                            ) {
                                Ok(quote) => {
                                    protocol_match = true;
                                    truth_action = Some(quote.observed.action);
                                    truth_token = Some(quote.token);
                                    quote_status = QuoteStatus::Available;
                                    hood_quote = Some(quote);
                                }
                                Err(error) => {
                                    quote_status = QuoteStatus::Blocked;
                                    protocol_blocker = Some(format!("hood_strict_quote:{error}"));
                                }
                            }
                        }
                    }
                    None => {
                        quote_status = QuoteStatus::Blocked;
                        protocol_blocker = Some("hood_token_identity_missing".into());
                    }
                }
            }
            return Ok(ReconciledCandidate {
                evidence: ReconciliationEvidence {
                    record_type: "launchpad_reconciliation_evidence",
                    tx_hash: candidate.tx_hash,
                    launchpad: candidate.launchpad,
                    receipt_status: receipt.status,
                    protocol_event_match: protocol_match,
                    observer_claim: candidate.observer_claim,
                    ground_truth_event: candidate.ground_truth_event,
                    ground_truth_hits: candidate.ground_truth_hits.clone(),
                    action: truth_action,
                    token: truth_token,
                    pool: truth_pool,
                    pool_id: truth_pool_id,
                    quote_status,
                    l2_block_number: Some(receipt.l2_block_number),
                    block_hash: Some(receipt.block_hash),
                    transaction_index: Some(receipt.transaction_index),
                    reconciliation_started_unix_ns,
                    reconciliation_completed_unix_ns: unix_now_ns(),
                    pons_generation,
                    protocol_blocker,
                },
                v3_quote,
                clanker_quote,
                bankr_quote,
                pons_quote,
                hood_quote,
                hood_migration,
            });
        }
        if Instant::now() >= deadline {
            return Ok(ReconciledCandidate {
                evidence: ReconciliationEvidence {
                    record_type: "launchpad_reconciliation_evidence",
                    tx_hash: candidate.tx_hash,
                    launchpad: candidate.launchpad,
                    receipt_status: false,
                    protocol_event_match: false,
                    observer_claim: candidate.observer_claim,
                    ground_truth_event: candidate.ground_truth_event,
                    ground_truth_hits: candidate.ground_truth_hits.clone(),
                    action: None,
                    token: None,
                    pool: None,
                    pool_id: None,
                    quote_status: QuoteStatus::Blocked,
                    l2_block_number: None,
                    block_hash: None,
                    transaction_index: None,
                    reconciliation_started_unix_ns,
                    reconciliation_completed_unix_ns: unix_now_ns(),
                    pons_generation: None,
                    protocol_blocker: Some("receipt_timeout".into()),
                },
                v3_quote: None,
                clanker_quote: None,
                bankr_quote: None,
                pons_quote: None,
                hood_quote: None,
                hood_migration: None,
            });
        }
        sleep(poll_interval).await;
    }
}

fn validate_ground_truth_receipt_binding(
    candidate: &ObservedCandidate,
    receipt: &hermes_feed::NoxaReceipt,
) -> Result<()> {
    if candidate.ground_truth_event == candidate.ground_truth_hits.is_empty() {
        bail!("ground-truth flag and exact log hits disagree");
    }
    for hit in &candidate.ground_truth_hits {
        if hit.l2_block_number != receipt.l2_block_number
            || hit.block_hash != receipt.block_hash
            || hit.transaction_index != receipt.transaction_index
            || !receipt
                .logs
                .iter()
                .any(|receipt_log| receipt_log == &hit.log)
        {
            bail!(
                "ground-truth eth_getLogs hit does not match canonical receipt for {}",
                candidate.tx_hash
            );
        }
    }
    Ok(())
}

fn strict_pons_reconciliation(
    transaction: &hermes_feed::RobinhoodTransaction,
    receipt: &hermes_feed::NoxaReceipt,
    expected_profile: hermes_feed::PonsExpectedProfile,
    policy: V3ReceiptQuotePolicy,
) -> PonsReconciliationOutcome {
    let generation = match transaction.to {
        Some(PONS_CURRENT_FACTORY) => Some(hermes_feed::PonsGeneration::Current),
        Some(PONS_LEGACY_FACTORY) => Some(hermes_feed::PonsGeneration::Legacy),
        _ => None,
    };
    match quote_current_pons(transaction, receipt, expected_profile, policy) {
        Ok(Some(quote)) => PonsReconciliationOutcome {
            generation,
            quote: Some(quote),
            blocker: None,
        },
        Ok(None) => PonsReconciliationOutcome {
            generation,
            quote: None,
            blocker: Some(
                "legacy_pons_generation_is_discovery_only_without_strict_receipt_profile".into(),
            ),
        },
        Err(error) => PonsReconciliationOutcome {
            generation,
            quote: None,
            blocker: Some(format!("pons_quote_error:{error}")),
        },
    }
}

fn quote_current_pons(
    transaction: &hermes_feed::RobinhoodTransaction,
    receipt: &hermes_feed::NoxaReceipt,
    expected_profile: hermes_feed::PonsExpectedProfile,
    policy: V3ReceiptQuotePolicy,
) -> std::result::Result<Option<PonsReceiptPaperQuote>, PonsQuoteError> {
    if transaction.to != Some(PONS_CURRENT_FACTORY) {
        return Ok(None);
    }
    quote_pons_launch_receipt(
        transaction,
        receipt,
        expected_profile,
        PonsQuotePolicy {
            amount_in: policy.amount_in,
            max_amount_in: policy.max_amount_in,
            slippage_bps: policy.slippage_bps,
        },
    )
    .map(Some)
}

fn hood_token_from_receipt(logs: &[ReceiptLog]) -> Option<alloy_primitives::Address> {
    let created = keccak256(HOOD_TOKEN_CREATED_SIGNATURE.as_bytes());
    let trade = keccak256(HOOD_TRADE_SIGNATURE.as_bytes());
    let tokens = logs
        .iter()
        .filter(|log| {
            log.address == HOOD_FACTORY
                && matches!(log.topics.first(), Some(topic) if *topic == created || *topic == trade)
        })
        .map(|log| topic_address(log, 1))
        .collect::<Option<Vec<_>>>()?;
    let first = *tokens.first()?;
    tokens.iter().all(|token| *token == first).then_some(first)
}

fn topic_address(log: &ReceiptLog, index: usize) -> Option<alloy_primitives::Address> {
    let topic = log.topics.get(index)?;
    if topic.as_slice()[..12].iter().any(|byte| *byte != 0) {
        return None;
    }
    Some(alloy_primitives::Address::from_slice(
        &topic.as_slice()[12..],
    ))
}

fn protocol_event_match(launchpad: LaunchpadId, logs: &[ReceiptLog]) -> bool {
    logs.iter().any(|log| match launchpad {
        LaunchpadId::Noxa => {
            matches!(
                log.address,
                NOXA_LAUNCH_FACTORY | ACTIVE_NOXA_LAUNCH_FACTORY
            ) && decode_token_launched(log).is_some()
        }
        LaunchpadId::Bow => exact_topic(
            log,
            BOW_LAUNCH_FACTORY,
            keccak256(BOW_LAUNCHED_SIGNATURE.as_bytes()),
        ),
        LaunchpadId::LaunchHoodV3 => exact_topic(
            log,
            LAUNCHHOOD_V3_FACTORY,
            keccak256(LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE.as_bytes()),
        ),
        LaunchpadId::Clanker => exact_topic(log, CLANKER_FACTORY, CLANKER_TOKEN_CREATED_TOPIC),
        LaunchpadId::BankrDoppler => exact_topic(log, DOPPLER_CREATE_EMITTER, DOPPLER_CREATE_TOPIC),
        LaunchpadId::KlikFinance => exact_topic(log, KLIK_FACTORY, KLIK_TOKEN_CREATED_TOPIC),
        LaunchpadId::Pons => {
            matches!(log.address, PONS_CURRENT_FACTORY | PONS_LEGACY_FACTORY)
                && log.topics.first() == Some(&PONS_TOKEN_LAUNCHED_TOPIC)
        }
        LaunchpadId::Flap => {
            decode_flap_token_created(CHAIN_ID, log).is_some()
                || decode_flap_token_bought(CHAIN_ID, log).is_some()
        }
        LaunchpadId::HoodFun => {
            exact_topic(
                log,
                HOOD_FACTORY,
                keccak256(HOOD_TOKEN_CREATED_SIGNATURE.as_bytes()),
            ) || exact_topic(
                log,
                HOOD_FACTORY,
                keccak256(HOOD_TRADE_SIGNATURE.as_bytes()),
            )
        }
        LaunchpadId::TrenchToday | LaunchpadId::LeaveHood => false,
    })
}

fn exact_topic(log: &ReceiptLog, address: alloy_primitives::Address, topic: B256) -> bool {
    log.address == address && log.topics.first() == Some(&topic)
}

fn unix_now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use alloy_primitives::{Address, Bytes};
    use hermes_feed::{NoxaReceipt, RobinhoodTransaction};
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    fn log(address: alloy_primitives::Address, topic: B256) -> ReceiptLog {
        ReceiptLog {
            address,
            log_index: 0,
            topics: vec![topic],
            data: Bytes::new(),
        }
    }

    fn observer_frame() -> Value {
        json!({
            "record_type": "launchpad_paper_frame",
            "report": {
                "observations": [{
                    "tx_hash": B256::with_last_byte(1),
                    "launchpad": "clanker",
                    "feed_sequence": 42,
                    "l1_block_number": 25_500_000,
                    "l1_timestamp": 1_784_000_000,
                    "action": "launch"
                }],
                "reconciliation_requests": [{
                    "tx_hash": B256::with_last_byte(1),
                    "launchpad": "clanker",
                    "feed_sequence": 42,
                    "l1_block_number": 25_500_000,
                    "l1_timestamp": 1_784_000_000,
                    "evidence_source": "independent_receipt_and_protocol_events",
                    "initial_decision_dependency": false
                }]
            }
        })
    }

    fn parse_observer_frame(value: Value) -> Result<ObserverInput> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        read_observer_input_from_reader(Cursor::new(bytes))
    }

    #[test]
    fn collector_is_driven_by_exact_async_reconciliation_requests() {
        let input = parse_observer_frame(observer_frame()).unwrap();
        let key = (B256::with_last_byte(1), LaunchpadId::Clanker);
        let candidate = input.candidates.get(&key).unwrap();
        assert!(candidate.observer_claim);
        assert!(!candidate.ground_truth_event);

        let mut missing = observer_frame();
        *missing
            .pointer_mut("/report/reconciliation_requests")
            .unwrap() = json!([]);
        assert!(
            parse_observer_frame(missing)
                .unwrap_err()
                .to_string()
                .contains("has no reconciliation request")
        );

        let mut mismatched = observer_frame();
        *mismatched
            .pointer_mut("/report/reconciliation_requests/0/feed_sequence")
            .unwrap() = json!(43);
        assert!(
            parse_observer_frame(mismatched)
                .unwrap_err()
                .to_string()
                .contains("feed provenance disagrees")
        );
    }

    #[test]
    fn collector_rejects_forged_or_decision_dependent_requests() {
        let mut forged_source = observer_frame();
        *forged_source
            .pointer_mut("/report/reconciliation_requests/0/evidence_source")
            .unwrap() = json!("observer_inference");
        assert!(
            parse_observer_frame(forged_source)
                .unwrap_err()
                .to_string()
                .contains("decode reconciliation requests")
        );

        let mut decision_dependency = observer_frame();
        *decision_dependency
            .pointer_mut("/report/reconciliation_requests/0/initial_decision_dependency")
            .unwrap() = json!(true);
        assert!(
            parse_observer_frame(decision_dependency)
                .unwrap_err()
                .to_string()
                .contains("initial decision dependency")
        );

        let mut duplicate = observer_frame();
        let request = duplicate
            .pointer("/report/reconciliation_requests/0")
            .unwrap()
            .clone();
        duplicate
            .pointer_mut("/report/reconciliation_requests")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(request);
        assert!(
            parse_observer_frame(duplicate)
                .unwrap_err()
                .to_string()
                .contains("duplicate reconciliation request")
        );
    }

    #[test]
    fn exact_protocol_topics_reconcile_and_cross_protocol_topics_do_not() {
        let clanker = log(CLANKER_FACTORY, CLANKER_TOKEN_CREATED_TOPIC);
        assert!(protocol_event_match(
            LaunchpadId::Clanker,
            std::slice::from_ref(&clanker)
        ));
        assert!(!protocol_event_match(
            LaunchpadId::BankrDoppler,
            std::slice::from_ref(&clanker)
        ));

        let bankr = log(DOPPLER_CREATE_EMITTER, DOPPLER_CREATE_TOPIC);
        assert!(protocol_event_match(
            LaunchpadId::BankrDoppler,
            std::slice::from_ref(&bankr)
        ));
        assert!(!protocol_event_match(
            LaunchpadId::Clanker,
            std::slice::from_ref(&bankr)
        ));

        let lookalike = log(Address::with_last_byte(0xee), CLANKER_TOKEN_CREATED_TOPIC);
        assert!(!protocol_event_match(
            LaunchpadId::Clanker,
            std::slice::from_ref(&lookalike)
        ));

        assert_eq!(
            launchpad_for_ground_truth_log(&clanker),
            Some(LaunchpadId::Clanker)
        );
        assert_eq!(
            launchpad_for_ground_truth_log(&bankr),
            Some(LaunchpadId::BankrDoppler)
        );
        assert_eq!(launchpad_for_ground_truth_log(&lookalike), None);
        assert_eq!(
            launchpad_for_ground_truth_log(&log(PONS_LEGACY_FACTORY, PONS_TOKEN_LAUNCHED_TOPIC)),
            Some(LaunchpadId::Pons)
        );
        assert_eq!(
            launchpad_for_ground_truth_log(&log(PONS_CURRENT_FACTORY, CLANKER_TOKEN_CREATED_TOPIC)),
            None
        );
    }

    #[test]
    fn ground_truth_hit_must_match_canonical_receipt_coordinates_and_log() {
        let event = log(CLANKER_FACTORY, CLANKER_TOKEN_CREATED_TOPIC);
        let tx_hash = B256::with_last_byte(1);
        let block_hash = B256::with_last_byte(2);
        let candidate = ObservedCandidate {
            tx_hash,
            launchpad: LaunchpadId::Clanker,
            observer_claim: true,
            ground_truth_event: true,
            ground_truth_hits: vec![GroundTruthHit {
                l2_block_number: 10,
                block_hash,
                transaction_index: 3,
                log: event.clone(),
            }],
        };
        let receipt = NoxaReceipt {
            transaction_hash: tx_hash,
            block_hash,
            status: true,
            l2_block_number: 10,
            l1_block_number: None,
            transaction_index: 3,
            gas_used: None,
            effective_gas_price: None,
            logs: vec![event],
        };
        validate_ground_truth_receipt_binding(&candidate, &receipt).unwrap();

        let mut reorged = receipt.clone();
        reorged.block_hash = B256::with_last_byte(0xee);
        assert!(validate_ground_truth_receipt_binding(&candidate, &reorged).is_err());

        let mut missing_log = receipt;
        missing_log.logs.clear();
        assert!(validate_ground_truth_receipt_binding(&candidate, &missing_log).is_err());
    }

    #[derive(Deserialize)]
    struct ReceiptProofFixture {
        receipt: NoxaReceipt,
    }

    #[test]
    fn live_clanker_and_bankr_receipts_bind_independent_primary_event_hits() {
        let proofs = [
            (
                LaunchpadId::Clanker,
                include_str!("../../tests/fixtures/clanker-v4-live-proof.json"),
            ),
            (
                LaunchpadId::BankrDoppler,
                include_str!("../../tests/fixtures/bankr-doppler-live-proof.json"),
            ),
        ];
        for (launchpad, json) in proofs {
            let fixture: ReceiptProofFixture = serde_json::from_str(json).unwrap();
            let event = fixture
                .receipt
                .logs
                .iter()
                .find(|log| launchpad_for_ground_truth_log(log) == Some(launchpad))
                .expect("reviewed receipt contains exact primary protocol event")
                .clone();
            let candidate = ObservedCandidate {
                tx_hash: fixture.receipt.transaction_hash,
                launchpad,
                observer_claim: true,
                ground_truth_event: true,
                ground_truth_hits: vec![GroundTruthHit {
                    l2_block_number: fixture.receipt.l2_block_number,
                    block_hash: fixture.receipt.block_hash,
                    transaction_index: fixture.receipt.transaction_index,
                    log: event,
                }],
            };
            validate_ground_truth_receipt_binding(&candidate, &fixture.receipt).unwrap();
        }
    }

    #[test]
    fn verified_v3_and_hood_event_signatures_match_research_topics() {
        assert_eq!(
            keccak256(LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE.as_bytes()).as_slice()[..4],
            [0x23, 0x5e, 0x34, 0xa4]
        );
        assert_ne!(keccak256(BOW_LAUNCHED_SIGNATURE.as_bytes()), B256::ZERO);
        assert_ne!(
            keccak256(HOOD_TOKEN_CREATED_SIGNATURE.as_bytes()),
            keccak256(HOOD_TRADE_SIGNATURE.as_bytes())
        );
    }

    #[derive(Deserialize)]
    struct PonsLiveFixture {
        transaction: RobinhoodTransaction,
        receipt: NoxaReceipt,
    }

    #[test]
    fn collector_emits_quote_only_for_strict_current_pons_proof() {
        let fixture: PonsLiveFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/pons-current-live-proof.json"
        ))
        .unwrap();
        let policy = V3ReceiptQuotePolicy {
            amount_in: U256::from(1_000_000_000_000_000_u64),
            max_amount_in: U256::from(10_000_000_000_000_000_u64),
            slippage_bps: 100,
        };
        let outcome = strict_pons_reconciliation(
            &fixture.transaction,
            &fixture.receipt,
            hermes_feed::PonsExpectedProfile::production(),
            policy,
        );
        let event_identity = fixture
            .receipt
            .logs
            .iter()
            .find_map(pons_launch_event_identity)
            .expect("Pons launch event identity");
        assert_eq!(
            outcome.generation,
            Some(hermes_feed::PonsGeneration::Current)
        );
        assert!(outcome.blocker.is_none());
        let quote = outcome.quote.unwrap();
        assert_eq!(event_identity, (quote.market.token, quote.market.pool));
        assert_eq!(quote.launchpad, LaunchpadId::Pons);
        assert!(quote.entry.expected_output > U256::ZERO);
        assert!(!quote.execution_eligible);
        assert!(!quote.broadcast);

        let mut legacy = fixture.transaction;
        legacy.to = Some(PONS_LEGACY_FACTORY);
        let legacy = strict_pons_reconciliation(
            &legacy,
            &fixture.receipt,
            hermes_feed::PonsExpectedProfile::production(),
            policy,
        );
        assert_eq!(legacy.generation, Some(hermes_feed::PonsGeneration::Legacy));
        assert!(legacy.quote.is_none());
        assert!(legacy.blocker.unwrap().contains("discovery_only"));
    }

    #[test]
    fn pons_event_reconciliation_requires_a_pinned_factory() {
        let current = log(PONS_CURRENT_FACTORY, PONS_TOKEN_LAUNCHED_TOPIC);
        assert!(protocol_event_match(LaunchpadId::Pons, &[current]));
        let lookalike = log(Address::with_last_byte(0xee), PONS_TOKEN_LAUNCHED_TOPIC);
        assert!(!protocol_event_match(LaunchpadId::Pons, &[lookalike]));
    }

    #[derive(Deserialize)]
    struct HoodLiveFixture {
        receipt: NoxaReceipt,
    }

    #[test]
    fn hood_collector_extracts_one_exact_factory_token_and_rejects_lookalikes() {
        let fixture: HoodLiveFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/hood-normal-buy-live-proof.json"
        ))
        .unwrap();
        assert_eq!(
            hood_token_from_receipt(&fixture.receipt.logs),
            Some(alloy_primitives::address!(
                "21c2ed1755d26cc99607c7b76469ee480087600d"
            ))
        );
        let mut lookalike = fixture.receipt.logs;
        lookalike[0].address = Address::with_last_byte(0xee);
        assert_eq!(hood_token_from_receipt(&lookalike), None);
    }
}
