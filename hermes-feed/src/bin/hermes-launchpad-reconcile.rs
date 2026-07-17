use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Cursor};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alloy_primitives::{B256, U256, keccak256};
use anyhow::{Context, Result, bail};
use clap::Parser;
use futures_util::{StreamExt, stream};
use hermes_feed::bankr_receipt_quote::quote_bankr_doppler_launch_receipt_at_receipt_block;
use hermes_feed::evidence_provenance::{
    EvidenceAcquisition, ObserverEvidenceProvenance, ReconciliationEvidenceProvenance,
    current_executable_keccak256, maybe_print_self_digest, read_bytes_with_keccak,
    read_json_with_keccak, verify_expected_self_keccak256,
};
use hermes_feed::flap_abi::{decode_flap_token_bought, decode_flap_token_created};
use hermes_feed::launchpad_adapter::{ActionKind, LaunchpadId, WrapperKind};
use hermes_feed::launchpad_adapters::{
    CLANKER_FACTORY, CLANKER_TOKEN_CREATED_TOPIC, DOPPLER_CREATE_EMITTER, DOPPLER_CREATE_TOPIC,
    KLIK_FACTORY, KLIK_TOKEN_CREATED_TOPIC,
};
use hermes_feed::launchpad_ground_truth::{
    BOW_LAUNCHED_SIGNATURE, HOOD_TOKEN_CREATED_SIGNATURE, HOOD_TRADE_SIGNATURE,
    LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE, STONKS_V3_LAUNCHED_SIGNATURE,
    launchpad_for_ground_truth_log,
};
use hermes_feed::noxa_abi::{ReceiptLog, decode_token_launched};
use hermes_feed::paper_observer::{
    PaperExpectedPins, PaperLaunchpadObserver, PaperObservedStartupSnapshot,
};
use hermes_feed::pons::{
    PONS_CURRENT_FACTORY, PONS_LEGACY_DISCOVERY_BLOCKER, PONS_LEGACY_FACTORY,
    PONS_TOKEN_LAUNCHED_TOPIC,
};
use hermes_feed::pons_receipt_quote::pons_launch_event_identity;
use hermes_feed::robinhood::{
    ACTIVE_NOXA_LAUNCH_FACTORY, BOW_LAUNCH_FACTORY, CHAIN_ID, LAUNCHHOOD_V3_FACTORY,
    NOXA_LAUNCH_FACTORY, PUBLIC_RPC_URL,
};
use hermes_feed::stonks_v3_observer::{
    STONKS_V3_LAUNCHER, StonksV3ObservationEvidence,
    observe_stonks_v3_direct_launch_at_receipt_block,
};
use hermes_feed::tier2_curve::HOOD_FACTORY;
use hermes_feed::{
    BankrDopplerExpectedProfile, BankrDopplerQuotePolicy, BankrDopplerReceiptPaperQuote,
    ClankerQuotePolicy, ClankerReceiptPaperQuote, ClankerV4ExpectedProfile,
    Eip7702SelfBatchExpectedPins, Eip7702SelfBatchProvenance, HoodExpectedProfile,
    HoodMigrationEvidence, HoodQuotePolicy, HoodReceiptPaperQuote, NoxaRpcClient, PonsQuotePolicy,
    PonsReceiptPaperQuote, V3ReceiptPaperQuote, V3ReceiptQuotePolicy, quote_clanker_launch_receipt,
    quote_hood_curve_receipt, quote_pons_eip7702_provenance_receipt, quote_pons_launch_receipt,
    quote_v3_launch_receipt, verify_hood_graduation_receipt,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::{Instant, sleep};

async fn collect_bankr_quote(
    rpc: &NoxaRpcClient,
    transaction: &hermes_feed::RobinhoodTransaction,
    receipt: &hermes_feed::NoxaReceipt,
    block: &hermes_feed::RobinhoodBlock,
    profile: BankrDopplerExpectedProfile,
    policy: BankrDopplerQuotePolicy,
) -> Result<BankrDopplerReceiptPaperQuote, hermes_feed::BankrQuoteError> {
    quote_bankr_doppler_launch_receipt_at_receipt_block(
        rpc,
        transaction,
        receipt,
        block,
        profile,
        policy,
    )
    .await
}

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Read-only receipt/event reconciler for launchpad paper observations"
)]
struct Cli {
    #[arg(long)]
    expected_self_keccak256: Option<B256>,
    /// Provenance asserted by the observer input. Live and replay evidence can
    /// never be silently interchanged.
    #[arg(long, value_enum)]
    acquisition: EvidenceAcquisition,
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
    wrapper: WrapperKind,
    wrapper_provenance: Option<Eip7702SelfBatchProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReconciliationRequest {
    tx_hash: B256,
    launchpad: LaunchpadId,
    feed_sequence: u64,
    l1_block_number: u64,
    l1_timestamp: u64,
    evidence_source: EvidenceSource,
    initial_decision_dependency: bool,
    #[serde(default = "direct_wrapper")]
    wrapper: WrapperKind,
    #[serde(default)]
    wrapper_provenance: Option<Eip7702SelfBatchProvenance>,
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
    #[serde(default = "direct_wrapper")]
    wrapper: WrapperKind,
}

const fn direct_wrapper() -> WrapperKind {
    WrapperKind::Direct
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
    provenance: ObserverEvidenceProvenance,
    source_content_keccak256: B256,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PaperCapabilitiesRecord {
    record_type: String,
    provenance: ObserverEvidenceProvenance,
    capabilities: Value,
    broadcast: bool,
    signing: bool,
    candidate_time_rpc: bool,
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
    stonks_observation: Option<StonksV3ObservationEvidence>,
    pons_quote: Option<PonsReceiptPaperQuote>,
    hood_quote: Option<HoodReceiptPaperQuote>,
    hood_migration: Option<HoodMigrationEvidence>,
}

#[derive(Clone)]
struct ReconcileProfiles {
    clanker: Option<ClankerV4ExpectedProfile>,
    bankr: Option<BankrDopplerExpectedProfile>,
    pons: hermes_feed::PonsExpectedProfile,
    pons_eip7702: Option<Eip7702SelfBatchExpectedPins>,
    hood: HoodExpectedProfile,
}

struct PonsReconciliationOutcome {
    generation: Option<hermes_feed::PonsGeneration>,
    quote: Option<PonsReceiptPaperQuote>,
    blocker: Option<String>,
}

fn validate_reconciler_provenance_inputs(
    observer: &ObserverEvidenceProvenance,
    acquisition: EvidenceAcquisition,
    expected_pins_content_keccak256: B256,
    observed_snapshot_content_keccak256: B256,
) -> Result<()> {
    observer.validate()?;
    if observer.acquisition != acquisition
        || observer.expected_pins_content_keccak256 != expected_pins_content_keccak256
        || observer.observed_snapshot_content_keccak256 != observed_snapshot_content_keccak256
    {
        bail!("observer provenance disagrees with reconciler inputs");
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    if maybe_print_self_digest()? {
        return Ok(());
    }
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
    let (expected, expected_pins_content_keccak256): (PaperExpectedPins, B256) =
        read_json_with_keccak(&args.expected_pins, "expected pins")?;
    let (observed, observed_snapshot_content_keccak256): (PaperObservedStartupSnapshot, B256) =
        read_json_with_keccak(&args.observed_startup_snapshot, "observed startup snapshot")?;
    validate_reconciler_provenance_inputs(
        &observer_input.provenance,
        args.acquisition,
        expected_pins_content_keccak256,
        observed_snapshot_content_keccak256,
    )?;
    let reconciliation_provenance = ReconciliationEvidenceProvenance {
        record_type: "launchpad_reconciliation_provenance".into(),
        observer: observer_input.provenance.clone(),
        reconciler_binary_keccak256: match args.expected_self_keccak256 {
            Some(expected) => verify_expected_self_keccak256(expected)?,
            None if args.acquisition == EvidenceAcquisition::Live => {
                bail!("live reconciliation requires --expected-self-keccak256")
            }
            None => current_executable_keccak256()?,
        },
        observer_output_content_keccak256: observer_input.source_content_keccak256,
    };
    reconciliation_provenance.validate()?;
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
        pons_eip7702: expected.pons_eip7702_self_batch.clone(),
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
    println!("{}", serde_json::to_string(&reconciliation_provenance)?);
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
        if let Some(observation) = result.stonks_observation {
            println!("{}", serde_json::to_string(&observation)?);
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
    let (bytes, source_content_keccak256) = read_bytes_with_keccak(path, "observer JSONL")?;
    read_observer_input_from_reader(Cursor::new(bytes), source_content_keccak256)
        .with_context(|| format!("validate observer JSONL {}", path.display()))
}

fn read_observer_input_from_reader(
    input: impl BufRead,
    source_content_keccak256: B256,
) -> Result<ObserverInput> {
    let mut candidates = HashMap::new();
    let mut provenance = None;
    for (index, line) in input.lines().enumerate() {
        let line = line.with_context(|| format!("read observer line {}", index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line)
            .with_context(|| format!("decode observer line {}", index + 1))?;
        match value.get("record_type").and_then(Value::as_str) {
            Some("launchpad_paper_capabilities") => {
                if provenance.is_some() {
                    bail!("duplicate paper capabilities provenance record");
                }
                let capabilities: PaperCapabilitiesRecord = serde_json::from_value(value)
                    .with_context(|| {
                        format!("decode capabilities on observer line {}", index + 1)
                    })?;
                if capabilities.record_type != "launchpad_paper_capabilities"
                    || capabilities.broadcast
                    || capabilities.signing
                    || capabilities.candidate_time_rpc
                    || !capabilities.capabilities.is_array()
                {
                    bail!("paper capabilities record is unsafe or malformed");
                }
                capabilities.provenance.validate()?;
                provenance = Some(capabilities.provenance);
                continue;
            }
            Some("launchpad_paper_frame") => {}
            _ => continue,
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
                || request.wrapper != observation.wrapper
            {
                bail!("reconciliation request {key:?} feed provenance disagrees with observation");
            }
            match request.wrapper {
                WrapperKind::Eip7702SelfBatch => {
                    let valid = request.launchpad == LaunchpadId::Pons
                        && request
                            .wrapper_provenance
                            .as_ref()
                            .is_some_and(|provenance| {
                                Eip7702SelfBatchExpectedPins::production()
                                    .validate_provenance(provenance)
                                    .is_ok()
                            });
                    if !valid {
                        bail!("reconciliation request {key:?} has incomplete EIP-7702 provenance");
                    }
                }
                WrapperKind::Direct | WrapperKind::Erc4337 | WrapperKind::Multicall => {
                    if request.wrapper_provenance.is_some() {
                        bail!(
                            "reconciliation request {key:?} attaches EIP-7702 provenance to the wrong wrapper"
                        );
                    }
                }
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
                wrapper: request.wrapper,
                wrapper_provenance: request.wrapper_provenance,
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
    Ok(ObserverInput {
        candidates,
        provenance: provenance.context("observer input has no paper capabilities provenance")?,
        source_content_keccak256,
    })
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
        STONKS_V3_LAUNCHER,
        PONS_CURRENT_FACTORY,
        PONS_LEGACY_FACTORY,
        HOOD_FACTORY,
    ];
    let topics = [
        keccak256(BOW_LAUNCHED_SIGNATURE.as_bytes()),
        keccak256(LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE.as_bytes()),
        CLANKER_TOKEN_CREATED_TOPIC,
        DOPPLER_CREATE_TOPIC,
        keccak256(STONKS_V3_LAUNCHED_SIGNATURE.as_bytes()),
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
                wrapper: WrapperKind::Direct,
                wrapper_provenance: None,
            });
    }
    let stable_start = rpc.block_by_number(scan.start_head).await?;
    let stable_cutoff = rpc.block_by_number(scan.cutoff_head).await?;
    if stable_start.hash != start_anchor.hash || stable_cutoff.hash != cutoff_anchor.hash {
        bail!("ground-truth anchor reorged during event collection");
    }
    // Stonks emits both the shared Airlock `Create` and a launcher-owned
    // `Launched` attestation. The latter is the exact protocol taxonomy, so a
    // single transaction must not also become a Bankr/Doppler miss.
    suppress_shared_airlock_for_stonks(&mut input.candidates);
    let exact_event_logs = retained_ground_truth_event_logs(&input.candidates);
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

fn suppress_shared_airlock_for_stonks(
    candidates: &mut HashMap<(B256, LaunchpadId), ObservedCandidate>,
) {
    let stonks_transactions: HashSet<_> = candidates
        .values()
        .filter(|candidate| {
            candidate.ground_truth_event && candidate.launchpad == LaunchpadId::StonksV3
        })
        .map(|candidate| candidate.tx_hash)
        .collect();
    for tx_hash in stonks_transactions {
        candidates.remove(&(tx_hash, LaunchpadId::BankrDoppler));
    }
}

fn retained_ground_truth_event_logs(
    candidates: &HashMap<(B256, LaunchpadId), ObservedCandidate>,
) -> usize {
    candidates
        .values()
        .filter(|candidate| candidate.ground_truth_event)
        .map(|candidate| candidate.ground_truth_hits.len())
        .sum()
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
            let mut stonks_observation = None;
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
                match collect_v3_receipt_quote(
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
                    match collect_bankr_quote(
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
            if receipt.status && candidate.launchpad == LaunchpadId::StonksV3 {
                let transaction = rpc
                    .transaction_by_hash(candidate.tx_hash)
                    .await?
                    .with_context(|| format!("missing transaction {}", candidate.tx_hash))?;
                let block = rpc.block_by_number(receipt.l2_block_number).await?;
                match observe_stonks_v3_direct_launch_at_receipt_block(
                    rpc,
                    &transaction,
                    &receipt,
                    &block,
                )
                .await
                {
                    Ok(observation) => {
                        protocol_match = true;
                        truth_action = Some(ActionKind::Launch);
                        truth_token = Some(observation.asset);
                        truth_pool = Some(observation.pool);
                        quote_status = QuoteStatus::NotApplicable;
                        protocol_blocker = Some(observation.quote_blocker.clone());
                        stonks_observation = Some(observation);
                    }
                    Err(error) => {
                        quote_status = QuoteStatus::Blocked;
                        protocol_blocker = Some(format!("stonks_v3_observation:{error}"));
                    }
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
                let outcome = strict_pons_reconciliation(
                    &transaction,
                    &receipt,
                    candidate.wrapper,
                    candidate.wrapper_provenance.as_ref(),
                    profiles.pons,
                    profiles.pons_eip7702.as_ref(),
                    quote_policy,
                );
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
                stonks_observation,
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
                stonks_observation: None,
                pons_quote: None,
                hood_quote: None,
                hood_migration: None,
            });
        }
        sleep(poll_interval).await;
    }
}

fn collect_v3_receipt_quote(
    transaction: &hermes_feed::RobinhoodTransaction,
    receipt: &hermes_feed::NoxaReceipt,
    launchpad: LaunchpadId,
    policy: V3ReceiptQuotePolicy,
) -> std::result::Result<V3ReceiptPaperQuote, hermes_feed::V3ReceiptQuoteError> {
    quote_v3_launch_receipt(transaction, receipt, launchpad, policy)
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
    wrapper: WrapperKind,
    wrapper_provenance: Option<&Eip7702SelfBatchProvenance>,
    expected_profile: hermes_feed::PonsExpectedProfile,
    expected_eip7702: Option<&Eip7702SelfBatchExpectedPins>,
    policy: V3ReceiptQuotePolicy,
) -> PonsReconciliationOutcome {
    let generation = pons_receipt_generation(receipt);
    match generation {
        Some(hermes_feed::PonsGeneration::Current) => {
            let quote_policy = PonsQuotePolicy {
                amount_in: policy.amount_in,
                max_amount_in: policy.max_amount_in,
                slippage_bps: policy.slippage_bps,
            };
            let quote = match wrapper {
                WrapperKind::Direct if wrapper_provenance.is_none() => {
                    quote_pons_launch_receipt(transaction, receipt, expected_profile, quote_policy)
                }
                WrapperKind::Eip7702SelfBatch => match (wrapper_provenance, expected_eip7702) {
                    (Some(provenance), Some(expected)) => quote_pons_eip7702_provenance_receipt(
                        transaction,
                        receipt,
                        provenance,
                        expected,
                        expected_profile,
                        quote_policy,
                    ),
                    _ => Err(hermes_feed::PonsQuoteError::InvalidEnvelope),
                },
                _ => Err(hermes_feed::PonsQuoteError::InvalidEnvelope),
            };
            match quote {
                Ok(quote) => PonsReconciliationOutcome {
                    generation,
                    quote: Some(quote),
                    blocker: None,
                },
                Err(error) => PonsReconciliationOutcome {
                    generation,
                    quote: None,
                    blocker: Some(format!("pons_quote_error:{error}")),
                },
            }
        }
        Some(hermes_feed::PonsGeneration::Legacy) => PonsReconciliationOutcome {
            generation,
            quote: None,
            blocker: Some(PONS_LEGACY_DISCOVERY_BLOCKER.into()),
        },
        None => PonsReconciliationOutcome {
            generation,
            quote: None,
            blocker: Some("pons_receipt_factory_generation_missing_or_ambiguous".into()),
        },
    }
}

/// Derive the Pons generation from independently collected receipt evidence,
/// not from the outer transaction destination. Current launches may be hidden
/// in an unreviewed wrapper or delegated-account batch; those must remain quote
/// blocked, but they are still current-generation detector misses rather than
/// legacy discovery traffic.
fn pons_receipt_generation(
    receipt: &hermes_feed::NoxaReceipt,
) -> Option<hermes_feed::PonsGeneration> {
    let mut generation = None;
    for log in &receipt.logs {
        if pons_launch_event_identity(log).is_none() {
            continue;
        }
        let observed = match log.address {
            PONS_CURRENT_FACTORY => hermes_feed::PonsGeneration::Current,
            PONS_LEGACY_FACTORY => hermes_feed::PonsGeneration::Legacy,
            _ => continue,
        };
        if generation.is_some_and(|existing| existing != observed) {
            return None;
        }
        generation = Some(observed);
    }
    generation
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
        LaunchpadId::StonksV3 => exact_topic(
            log,
            STONKS_V3_LAUNCHER,
            keccak256(STONKS_V3_LAUNCHED_SIGNATURE.as_bytes()),
        ),
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
    use std::collections::HashSet;
    use std::io::Cursor;
    use std::str::FromStr;

    use alloy_primitives::{Address, Bytes};
    use hermes_feed::bankr_receipt_quote::BANKR_CREATE_SELECTOR;
    use hermes_feed::feed::BroadcastMessage;
    use hermes_feed::paper_observer::{
        ConfiguredBankrDopplerV4, ConfiguredCallPin, ConfiguredSmartAccount,
        ConfiguredSmartAccounts, ObservedRuntimePin, PaperFeedRuntime,
    };
    use hermes_feed::{NoxaReceipt, RobinhoodBlock, RobinhoodTransaction};
    use serde::Deserialize;
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[test]
    fn exact_stonks_truth_suppresses_shared_airlock_bankr_duplicate() {
        let tx_hash = alloy_primitives::b256!(
            "d53c3d8d8c76fd5f367d3d229a45e1aef65c0cdb712d94421f311f97fe6dd563"
        );
        let candidate = |launchpad| ObservedCandidate {
            tx_hash,
            launchpad,
            observer_claim: false,
            ground_truth_event: true,
            ground_truth_hits: vec![GroundTruthHit {
                l2_block_number: 12_033_710,
                block_hash: B256::ZERO,
                transaction_index: 2,
                log: ReceiptLog {
                    address: Address::ZERO,
                    log_index: u64::from(launchpad as u8),
                    topics: vec![B256::ZERO],
                    data: Bytes::new(),
                },
            }],
            wrapper: WrapperKind::Direct,
            wrapper_provenance: None,
        };
        let mut candidates = HashMap::from([
            (
                (tx_hash, LaunchpadId::BankrDoppler),
                candidate(LaunchpadId::BankrDoppler),
            ),
            (
                (tx_hash, LaunchpadId::StonksV3),
                candidate(LaunchpadId::StonksV3),
            ),
        ]);
        assert_eq!(retained_ground_truth_event_logs(&candidates), 2);
        suppress_shared_airlock_for_stonks(&mut candidates);
        assert_eq!(candidates.len(), 1);
        assert_eq!(retained_ground_truth_event_logs(&candidates), 1);
        assert!(candidates.contains_key(&(tx_hash, LaunchpadId::StonksV3)));
        assert!(!candidates.contains_key(&(tx_hash, LaunchpadId::BankrDoppler)));
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "explicit public read-only production reconciler proof; not part of hermetic CI"]
    async fn fresh_stonks_proof_crosses_concrete_noxa_rpc_reconciler_as_observe_only() {
        let tx_hash = alloy_primitives::b256!(
            "d53c3d8d8c76fd5f367d3d229a45e1aef65c0cdb712d94421f311f97fe6dd563"
        );
        let rpc = NoxaRpcClient::with_url(PUBLIC_RPC_URL).unwrap();
        let receipt = rpc.receipt(tx_hash).await.unwrap().unwrap();
        let launched = receipt
            .logs
            .iter()
            .find(|log| {
                exact_topic(
                    log,
                    STONKS_V3_LAUNCHER,
                    keccak256(STONKS_V3_LAUNCHED_SIGNATURE.as_bytes()),
                )
            })
            .unwrap()
            .clone();
        let candidate = ObservedCandidate {
            tx_hash,
            launchpad: LaunchpadId::StonksV3,
            observer_claim: false,
            ground_truth_event: true,
            ground_truth_hits: vec![GroundTruthHit {
                l2_block_number: receipt.l2_block_number,
                block_hash: receipt.block_hash,
                transaction_index: receipt.transaction_index,
                log: launched,
            }],
            wrapper: WrapperKind::Direct,
            wrapper_provenance: None,
        };
        let reconciled = reconcile_candidate(
            &rpc,
            candidate,
            Duration::from_secs(2),
            Duration::from_millis(1),
            bankr_quote_policy(),
            bankr_reconcile_profiles(),
        )
        .await
        .unwrap();
        assert_eq!(reconciled.evidence.launchpad, LaunchpadId::StonksV3);
        assert_eq!(reconciled.evidence.quote_status, QuoteStatus::NotApplicable);
        assert!(reconciled.bankr_quote.is_none());
        let observation = reconciled.stonks_observation.unwrap();
        assert_eq!(observation.profile, "stonks_v3_direct_launch");
        assert!(!observation.paper_evidence_ready);
        assert!(!observation.authorizes_canary);
        assert!(!observation.execution_eligible);
        assert!(!observation.broadcast);
    }

    #[derive(Deserialize)]
    struct BankrV4RawFrames {
        frames: Vec<BankrV4RawFrame>,
    }

    #[derive(Deserialize)]
    struct BankrV4RawFrame {
        window: String,
        line: u64,
        tx_hash: B256,
        envelope: String,
        source_path: String,
        payload_sha256: String,
        received_unix_ns: u64,
        payload: String,
    }

    #[derive(Deserialize)]
    struct BankrV4ProofSet {
        launches: Vec<BankrProof>,
    }

    #[derive(Deserialize)]
    struct BankrProof {
        envelope: String,
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
    }

    #[derive(Deserialize)]
    struct BankrV4RuntimeFixture {
        schema_version: u32,
        chain_id: u64,
        rpc_url: String,
        acquisition: String,
        verified_l2_blocks: Vec<BankrV4RuntimeBlock>,
        runtimes: Vec<BankrV4RuntimeCode>,
    }

    #[derive(Deserialize)]
    struct BankrV4RuntimeBlock {
        l2_block_number: u64,
        block_tag: String,
        transaction_hash: B256,
    }

    #[derive(Deserialize)]
    struct BankrV4RuntimeCode {
        role: String,
        address: Address,
        runtime_hash: B256,
        code: String,
    }

    struct RpcStep {
        method: &'static str,
        params: Value,
        result: Value,
    }

    fn observer_provenance() -> ObserverEvidenceProvenance {
        ObserverEvidenceProvenance {
            schema_version: hermes_feed::evidence_provenance::EVIDENCE_PROVENANCE_SCHEMA_VERSION,
            acquisition: EvidenceAcquisition::Live,
            expected_pins_content_keccak256: B256::with_last_byte(1),
            observed_snapshot_content_keccak256: B256::with_last_byte(2),
            observed_snapshot_l2_block_number: 900,
            observed_snapshot_l2_block_hash: B256::with_last_byte(8),
            observer_paper_binary_keccak256: B256::with_last_byte(3),
        }
    }

    fn capabilities_record() -> Value {
        json!({
            "record_type": "launchpad_paper_capabilities",
            "provenance": observer_provenance(),
            "capabilities": [],
            "broadcast": false,
            "signing": false,
            "candidate_time_rpc": false
        })
    }

    fn exact_bankr_v4_startup() -> (PaperExpectedPins, PaperObservedStartupSnapshot) {
        let mut expected: PaperExpectedPins = serde_json::from_str(include_str!(
            "../../tests/fixtures/launchpad-paper-expected-pins.synthetic.json"
        ))
        .unwrap();
        let mut observed: PaperObservedStartupSnapshot = serde_json::from_str(include_str!(
            "../../tests/fixtures/launchpad-paper-observed-startup.synthetic.json"
        ))
        .unwrap();
        let profile = BankrDopplerExpectedProfile::production();
        expected.bankr_doppler_v4 = Some(ConfiguredBankrDopplerV4 {
            airlock_runtime_hash: profile.airlock.runtime_code_hash,
            pool_manager_runtime_hash: profile.pool_manager.runtime_code_hash,
            initializer_runtime_hash: profile.initializer.runtime_code_hash,
            rehype_hook_runtime_hash: profile.rehype_hook.runtime_code_hash,
            token_factory_runtime_hash: profile.token_factory.runtime_code_hash,
            token_implementation_runtime_hash: profile.token_implementation.runtime_code_hash,
            governance_factory_runtime_hash: profile.governance_factory.runtime_code_hash,
            liquidity_migrator_runtime_hash: profile.liquidity_migrator.runtime_code_hash,
            standard_lp_fee_ppm: profile.standard_lp_fee_ppm,
            max_lp_fee_ppm: profile.max_lp_fee_ppm,
            hook_fee_denominator_ppm: profile.hook_fee_denominator_ppm,
            hook_start_fee_ppm: profile.hook_start_fee_ppm,
            hook_end_fee_ppm: profile.hook_end_fee_ppm,
            hook_duration_seconds: profile.hook_duration_seconds,
            quote_delay_guard_seconds: profile.quote_delay_guard_seconds,
            tick_spacing: profile.tick_spacing,
            pool_allocation_bps: profile.pool_allocation_bps,
            primary_curve_share_bps: profile.primary_curve_share_bps,
            secondary_curve_share_bps: profile.secondary_curve_share_bps,
            creator_beneficiary_bps: profile.creator_beneficiary_bps,
            protocol_beneficiary_bps: profile.protocol_beneficiary_bps,
        });
        expected.bankr_doppler_calls = vec![ConfiguredCallPin {
            destination: profile.airlock.address,
            runtime_hash: profile.airlock.runtime_code_hash,
            selector: BANKR_CREATE_SELECTOR,
        }];
        let delegation = profile.smart_account.delegation_implementation.unwrap();
        expected.erc4337 = Some(ConfiguredSmartAccounts {
            entry_point_runtime_hash: profile.entry_point.runtime_code_hash,
            accounts: vec![ConfiguredSmartAccount {
                account: profile.smart_account.account.address,
                runtime_hash: profile.smart_account.account.runtime_code_hash,
                execution_profile: profile.smart_account.execution_profile,
                factory: None,
                factory_runtime_hash: None,
                delegation_implementation: Some(delegation.address),
                delegation_runtime_hash: Some(delegation.runtime_code_hash),
            }],
        });
        let pin = |address, implementation, runtime_hash| ObservedRuntimePin {
            address,
            implementation,
            runtime_hash,
            code_bytes: None,
        };
        observed.pins.extend([
            pin(
                profile.airlock.address,
                None,
                profile.airlock.runtime_code_hash,
            ),
            pin(
                profile.pool_manager.address,
                None,
                profile.pool_manager.runtime_code_hash,
            ),
            pin(
                profile.initializer.address,
                None,
                profile.initializer.runtime_code_hash,
            ),
            pin(
                profile.rehype_hook.address,
                None,
                profile.rehype_hook.runtime_code_hash,
            ),
            pin(
                profile.token_factory.address,
                None,
                profile.token_factory.runtime_code_hash,
            ),
            pin(
                profile.token_implementation.address,
                None,
                profile.token_implementation.runtime_code_hash,
            ),
            pin(
                profile.governance_factory.address,
                None,
                profile.governance_factory.runtime_code_hash,
            ),
            pin(
                profile.liquidity_migrator.address,
                None,
                profile.liquidity_migrator.runtime_code_hash,
            ),
            pin(
                profile.entry_point.address,
                None,
                profile.entry_point.runtime_code_hash,
            ),
            pin(
                profile.smart_account.account.address,
                Some(delegation.address),
                profile.smart_account.account.runtime_code_hash,
            ),
            pin(delegation.address, None, delegation.runtime_code_hash),
        ]);
        (expected, observed)
    }

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
        let mut bytes = serde_json::to_vec(&capabilities_record())?;
        bytes.push(b'\n');
        bytes.extend(serde_json::to_vec(&value)?);
        bytes.push(b'\n');
        let digest = keccak256(&bytes);
        read_observer_input_from_reader(Cursor::new(bytes), digest)
    }

    fn rpc_block_value(block: &RobinhoodBlock) -> Value {
        json!({
            "number": format!("0x{:x}", block.l2_block_number),
            "l1BlockNumber": format!("0x{:x}", block.l1_block_number),
            "timestamp": format!("0x{:x}", block.timestamp),
            "hash": block.hash,
        })
    }

    fn rpc_transaction_value(transaction: &RobinhoodTransaction) -> Value {
        json!({
            "hash": transaction.hash,
            "from": transaction.from,
            "to": transaction.to,
            "input": format!("0x{}", hex::encode(transaction.input.as_ref())),
            "value": format!("{:#x}", transaction.value),
            "blockNumber": transaction
                .l2_block_number
                .map(|number| format!("0x{number:x}")),
            "transactionIndex": transaction
                .transaction_index
                .map(|index| format!("0x{index:x}")),
        })
    }

    fn rpc_receipt_value(receipt: &NoxaReceipt) -> Value {
        let logs = receipt
            .logs
            .iter()
            .map(|log| {
                json!({
                    "address": log.address,
                    "logIndex": format!("0x{:x}", log.log_index),
                    "topics": log.topics,
                    "data": format!("0x{}", hex::encode(log.data.as_ref())),
                })
            })
            .collect::<Vec<_>>();
        json!({
            "transactionHash": receipt.transaction_hash,
            "blockHash": receipt.block_hash,
            "status": if receipt.status { "0x1" } else { "0x0" },
            "blockNumber": format!("0x{:x}", receipt.l2_block_number),
            "l1BlockNumber": receipt
                .l1_block_number
                .map(|number| format!("0x{number:x}")),
            "transactionIndex": format!("0x{:x}", receipt.transaction_index),
            "gasUsed": receipt.gas_used.map(|gas| format!("0x{gas:x}")),
            "effectiveGasPrice": receipt
                .effective_gas_price
                .map(|price| format!("{price:#x}")),
            "logs": logs,
        })
    }

    async fn spawn_exact_rpc_server(
        steps: Vec<RpcStep>,
    ) -> (NoxaRpcClient, tokio::task::JoinHandle<usize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let rpc = NoxaRpcClient::with_url(url).unwrap();
        let server = tokio::spawn(async move {
            let expected_count = steps.len();
            for step in steps {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let header_end = loop {
                    let mut chunk = [0_u8; 8 * 1024];
                    let read = stream.read(&mut chunk).await.unwrap();
                    assert_ne!(read, 0, "RPC client closed before sending headers");
                    request.extend_from_slice(&chunk[..read]);
                    if let Some(offset) =
                        request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        break offset + 4;
                    }
                };
                let headers = std::str::from_utf8(&request[..header_end]).unwrap();
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .expect("RPC request omitted Content-Length");
                while request.len() < header_end + content_length {
                    let mut chunk = [0_u8; 8 * 1024];
                    let read = stream.read(&mut chunk).await.unwrap();
                    assert_ne!(read, 0, "RPC request body ended early");
                    request.extend_from_slice(&chunk[..read]);
                }
                assert_eq!(
                    request.len(),
                    header_end + content_length,
                    "RPC request contained trailing bytes"
                );
                let body: Value =
                    serde_json::from_slice(&request[header_end..header_end + content_length])
                        .unwrap();
                assert_eq!(body["jsonrpc"], "2.0");
                assert_eq!(body["id"], 1);
                assert_eq!(body["method"], step.method);
                assert_eq!(body["params"], step.params);
                let response = serde_json::to_vec(&json!({
                    "jsonrpc": "2.0",
                    "id": body["id"].clone(),
                    "result": step.result,
                }))
                .unwrap();
                let headers = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    response.len()
                );
                stream.write_all(headers.as_bytes()).await.unwrap();
                stream.write_all(&response).await.unwrap();
                stream.shutdown().await.unwrap();
            }
            expected_count
        });
        (rpc, server)
    }

    fn bankr_v4_runtime_fixture() -> BankrV4RuntimeFixture {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/bankr-doppler-v4-concrete-reconciler-runtime-code.json"
        ))
        .unwrap()
    }

    fn assert_bankr_runtime_fixture(fixture: &BankrV4RuntimeFixture, verified_blocks: usize) {
        assert_eq!(fixture.schema_version, 1);
        assert_eq!(fixture.chain_id, CHAIN_ID);
        assert_eq!(fixture.rpc_url, PUBLIC_RPC_URL);
        assert_eq!(
            fixture.acquisition,
            "read_only_eth_getCode_at_exact_receipt_blocks"
        );
        assert_eq!(fixture.verified_l2_blocks.len(), verified_blocks);

        let profile = BankrDopplerExpectedProfile::production();
        let delegation = profile.smart_account.delegation_implementation.unwrap();
        let expected = [
            (
                "airlock",
                profile.airlock.address,
                profile.airlock.runtime_code_hash,
            ),
            (
                "pool_manager",
                profile.pool_manager.address,
                profile.pool_manager.runtime_code_hash,
            ),
            (
                "initializer",
                profile.initializer.address,
                profile.initializer.runtime_code_hash,
            ),
            (
                "rehype_hook",
                profile.rehype_hook.address,
                profile.rehype_hook.runtime_code_hash,
            ),
            (
                "token_factory",
                profile.token_factory.address,
                profile.token_factory.runtime_code_hash,
            ),
            (
                "token_implementation",
                profile.token_implementation.address,
                profile.token_implementation.runtime_code_hash,
            ),
            (
                "governance_factory",
                profile.governance_factory.address,
                profile.governance_factory.runtime_code_hash,
            ),
            (
                "liquidity_migrator",
                profile.liquidity_migrator.address,
                profile.liquidity_migrator.runtime_code_hash,
            ),
            ("weth", profile.weth.address, profile.weth.runtime_code_hash),
            (
                "entry_point_v07",
                profile.entry_point.address,
                profile.entry_point.runtime_code_hash,
            ),
            (
                "kernel_delegation",
                delegation.address,
                delegation.runtime_code_hash,
            ),
        ];
        assert_eq!(fixture.runtimes.len(), expected.len());
        let mut addresses = HashSet::new();
        for (runtime, (role, address, runtime_hash)) in fixture.runtimes.iter().zip(expected) {
            assert_eq!(runtime.role, role);
            assert_eq!(runtime.address, address);
            assert_eq!(runtime.runtime_hash, runtime_hash);
            assert!(addresses.insert(runtime.address));
            let code = hex::decode(
                runtime
                    .code
                    .strip_prefix("0x")
                    .expect("runtime code is not hex-prefixed"),
            )
            .unwrap();
            assert!(!code.is_empty());
            assert_eq!(keccak256(&code), runtime.runtime_hash);
        }
    }

    fn runtime_code(fixture: &BankrV4RuntimeFixture, role: &str) -> String {
        fixture
            .runtimes
            .iter()
            .find(|runtime| runtime.role == role)
            .unwrap_or_else(|| panic!("missing runtime fixture role {role}"))
            .code
            .clone()
    }

    fn bankr_rpc_steps(
        proof: &BankrProof,
        leader: Address,
        fixture: &BankrV4RuntimeFixture,
        drift_role: Option<&str>,
    ) -> Vec<RpcStep> {
        let profile = BankrDopplerExpectedProfile::production();
        let delegation = profile.smart_account.delegation_implementation.unwrap();
        let tag = format!("0x{:x}", proof.receipt.l2_block_number);
        let designator = format!("0xef0100{}", hex::encode(delegation.address.as_slice()));
        assert_eq!(
            keccak256(hex::decode(designator.trim_start_matches("0x")).unwrap()),
            profile.smart_account.account.runtime_code_hash
        );
        let mut steps = vec![
            RpcStep {
                method: "eth_getTransactionReceipt",
                params: json!([proof.transaction.hash]),
                result: rpc_receipt_value(&proof.receipt),
            },
            RpcStep {
                method: "eth_getBlockByNumber",
                params: json!([tag, false]),
                result: rpc_block_value(&proof.block),
            },
            RpcStep {
                method: "eth_getTransactionByHash",
                params: json!([proof.transaction.hash]),
                result: rpc_transaction_value(&proof.transaction),
            },
            RpcStep {
                method: "eth_getBlockByNumber",
                params: json!([tag, false]),
                result: rpc_block_value(&proof.block),
            },
            RpcStep {
                method: "eth_getCode",
                params: json!([leader, tag]),
                result: json!(designator),
            },
            RpcStep {
                method: "eth_getCode",
                params: json!([delegation.address, tag]),
                result: json!(runtime_code(fixture, "kernel_delegation")),
            },
        ];
        for role in [
            "airlock",
            "pool_manager",
            "initializer",
            "rehype_hook",
            "token_factory",
            "token_implementation",
            "governance_factory",
            "liquidity_migrator",
            "weth",
        ] {
            let runtime = fixture
                .runtimes
                .iter()
                .find(|runtime| runtime.role == role)
                .unwrap();
            let mut code = runtime.code.clone();
            if drift_role == Some(role) {
                code.push_str("00");
            }
            steps.push(RpcStep {
                method: "eth_getCode",
                params: json!([runtime.address, tag]),
                result: json!(code),
            });
        }
        if proof.envelope == "erc7579" {
            let runtime = fixture
                .runtimes
                .iter()
                .find(|runtime| runtime.role == "entry_point_v07")
                .unwrap();
            let mut code = runtime.code.clone();
            if drift_role == Some("entry_point_v07") {
                code.push_str("00");
            }
            steps.push(RpcStep {
                method: "eth_getCode",
                params: json!([runtime.address, tag]),
                result: json!(code),
            });
        }
        if drift_role.is_none() {
            steps.push(RpcStep {
                method: "eth_getBlockByNumber",
                params: json!([tag, false]),
                result: rpc_block_value(&proof.block),
            });
        }
        steps
    }

    fn exact_bankr_candidate(frame: &BankrV4RawFrame) -> ObservedCandidate {
        let (expected, observed) = exact_bankr_v4_startup();
        let observer = PaperLaunchpadObserver::from_startup_snapshots(expected, observed).unwrap();
        let mut runtime = PaperFeedRuntime::new(observer);
        let broadcast: BroadcastMessage = serde_json::from_str(&frame.payload).unwrap();
        let report = runtime
            .decode_received_at(&broadcast, frame.received_unix_ns)
            .unwrap();
        assert!(report.rejections.is_empty());
        assert_eq!(
            report
                .reconciliation_requests
                .iter()
                .filter(|request| {
                    request.tx_hash == frame.tx_hash
                        && request.launchpad == LaunchpadId::BankrDoppler
                })
                .count(),
            1
        );
        let serialized_frame = json!({
            "record_type": "launchpad_paper_frame",
            "report": report,
        });
        parse_observer_frame(serialized_frame)
            .unwrap()
            .candidates
            .into_iter()
            .find_map(|(key, candidate)| {
                (key == (frame.tx_hash, LaunchpadId::BankrDoppler)).then_some(candidate)
            })
            .expect("strict collector parser omitted exact Bankr request")
    }

    fn bankr_reconcile_profiles() -> ReconcileProfiles {
        ReconcileProfiles {
            clanker: None,
            bankr: Some(BankrDopplerExpectedProfile::production()),
            pons: hermes_feed::PonsExpectedProfile::production(),
            pons_eip7702: None,
            hood: HoodExpectedProfile::production(),
        }
    }

    fn bankr_quote_policy() -> V3ReceiptQuotePolicy {
        V3ReceiptQuotePolicy {
            amount_in: U256::from(1_000_000_000_000_000_u64),
            max_amount_in: U256::from(10_000_000_000_000_000_u64),
            slippage_bps: 100,
        }
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
    fn exact_v4_raw_requests_strictly_parse_and_bind_collector_quotes_for_both_envelopes() {
        let frames: BankrV4RawFrames = serde_json::from_str(include_str!(
            "../../tests/fixtures/bankr-doppler-v4-finaltuple-raw-frames.json"
        ))
        .unwrap();
        let proofs: BankrV4ProofSet = serde_json::from_str(include_str!(
            "../../tests/fixtures/bankr-doppler-v4-finaltuple-window-abc-live-proofs.json"
        ))
        .unwrap();
        let capabilities = capabilities_record();
        assert_eq!(capabilities["candidate_time_rpc"], false);
        assert_eq!(capabilities["broadcast"], false);
        assert_eq!(capabilities["signing"], false);

        for frame in frames.frames {
            let (expected_window, expected_line, expected_received_unix_ns, expected_sha256) =
                match frame.envelope.as_str() {
                    "erc7579" => (
                        "window-a",
                        2762,
                        1_784_271_655_711_031_000,
                        "4672011994f731bc6ca47ac8538c00539eb02c64854f8facbff1e2fff7291e75",
                    ),
                    "direct_airlock" => (
                        "window-b",
                        1661,
                        1_784_271_886_078_187_000,
                        "2da502bfbc533b2188390ef7190c8f5316fb8084914f4cf821a83578d1c66a84",
                    ),
                    other => panic!("unexpected fixture envelope {other}"),
                };
            let mut payload_line = frame.payload.as_bytes().to_vec();
            payload_line.push(b'\n');
            assert_eq!(frame.payload_sha256, expected_sha256);
            assert_eq!(hex::encode(Sha256::digest(&payload_line)), expected_sha256);
            assert_eq!(frame.received_unix_ns, expected_received_unix_ns);
            assert!(
                frame
                    .source_path
                    .ends_with(&format!("/windows/{expected_window}/raw-feed.jsonl"))
            );
            assert_eq!(
                (frame.window.as_str(), frame.line),
                (expected_window, expected_line)
            );
            let (expected, observed) = exact_bankr_v4_startup();
            let observer =
                PaperLaunchpadObserver::from_startup_snapshots(expected, observed).unwrap();
            let mut runtime = PaperFeedRuntime::new(observer);
            let broadcast: BroadcastMessage = serde_json::from_str(&frame.payload).unwrap();
            let report = runtime
                .decode_received_at(&broadcast, frame.received_unix_ns)
                .unwrap();
            assert!(report.rejections.is_empty());
            assert_eq!(
                report
                    .reconciliation_requests
                    .iter()
                    .filter(|request| {
                        request.tx_hash == frame.tx_hash
                            && request.launchpad == LaunchpadId::BankrDoppler
                    })
                    .count(),
                1
            );
            let frame_value = json!({
                "record_type": "launchpad_paper_frame",
                "report": report,
            });
            let parsed = parse_observer_frame(frame_value.clone()).unwrap();
            let candidate = parsed
                .candidates
                .get(&(frame.tx_hash, LaunchpadId::BankrDoppler))
                .unwrap();
            assert!(candidate.observer_claim);
            assert_eq!(
                candidate.wrapper,
                if frame.envelope == "erc7579" {
                    WrapperKind::Erc4337
                } else {
                    WrapperKind::Direct
                }
            );

            let proof = proofs
                .launches
                .iter()
                .find(|proof| proof.transaction.hash == frame.tx_hash)
                .unwrap();
            assert_eq!(proof.envelope, frame.envelope);
            let quote: BankrDopplerReceiptPaperQuote =
                serde_json::from_str(if frame.envelope == "erc7579" {
                    include_str!(
                        "../../tests/fixtures/bankr-doppler-v4-finaltuple-paper-quote.json"
                    )
                } else {
                    include_str!(
                        "../../tests/fixtures/bankr-doppler-v4-finaltuple-direct-paper-quote.json"
                    )
                })
                .unwrap();
            assert_eq!(quote.tx_hash, frame.tx_hash);
            assert_eq!(quote.tx_hash, proof.receipt.transaction_hash);
            assert_eq!(quote.state_version.block_hash, proof.block.hash);
            assert_eq!(
                quote.state_version.l2_block_number,
                proof.block.l2_block_number
            );
            assert_eq!(
                quote.market.create_profile_version,
                hermes_feed::BankrCreateProfileVersion::CurveTicksV4
            );
            assert_eq!(
                quote.market.envelope,
                if frame.envelope == "erc7579" {
                    hermes_feed::BankrEnvelopeKind::Erc7579
                } else {
                    hermes_feed::BankrEnvelopeKind::DirectAirlock
                }
            );
            assert!(quote.entry.expected_output > U256::ZERO);
            assert!(quote.full_position_exit.expected_output > U256::ZERO);
            assert!(!quote.execution_eligible);
            assert!(!quote.broadcast);
            let mut malformed_request = frame_value;
            *malformed_request
                .pointer_mut("/report/reconciliation_requests/0/evidence_source")
                .unwrap() = json!("observer_inference");
            assert!(parse_observer_frame(malformed_request).is_err());
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bankr_v4_exact_raw_frames_cross_concrete_rpc_collector_dispatch_and_fail_closed_on_code_drift()
     {
        let frames: BankrV4RawFrames = serde_json::from_str(include_str!(
            "../../tests/fixtures/bankr-doppler-v4-finaltuple-raw-frames.json"
        ))
        .unwrap();
        let proofs: BankrV4ProofSet = serde_json::from_str(include_str!(
            "../../tests/fixtures/bankr-doppler-v4-finaltuple-window-abc-live-proofs.json"
        ))
        .unwrap();
        let runtime_fixture = bankr_v4_runtime_fixture();
        assert_bankr_runtime_fixture(&runtime_fixture, 2);
        assert_eq!(frames.frames.len(), 2);
        assert!(
            frames
                .frames
                .iter()
                .any(|frame| frame.envelope == "erc7579")
        );
        assert!(
            frames
                .frames
                .iter()
                .any(|frame| frame.envelope == "direct_airlock")
        );
        assert!(
            frames
                .frames
                .iter()
                .all(|frame| matches!(frame.envelope.as_str(), "erc7579" | "direct_airlock"))
        );
        let capabilities = capabilities_record();
        assert_eq!(capabilities["candidate_time_rpc"], false);
        assert_eq!(capabilities["broadcast"], false);
        assert_eq!(capabilities["signing"], false);

        for frame in &frames.frames {
            let proof = proofs
                .launches
                .iter()
                .find(|proof| proof.transaction.hash == frame.tx_hash)
                .unwrap();
            let runtime_block = runtime_fixture
                .verified_l2_blocks
                .iter()
                .find(|block| block.transaction_hash == frame.tx_hash)
                .unwrap();
            assert_eq!(runtime_block.l2_block_number, proof.receipt.l2_block_number);
            assert_eq!(
                runtime_block.block_tag,
                format!("0x{:x}", proof.receipt.l2_block_number)
            );
            let expected_quote: BankrDopplerReceiptPaperQuote =
                serde_json::from_str(if frame.envelope == "erc7579" {
                    include_str!(
                        "../../tests/fixtures/bankr-doppler-v4-finaltuple-paper-quote.json"
                    )
                } else {
                    include_str!(
                        "../../tests/fixtures/bankr-doppler-v4-finaltuple-direct-paper-quote.json"
                    )
                })
                .unwrap();
            let candidate = exact_bankr_candidate(frame);
            assert_eq!(
                candidate.wrapper,
                if frame.envelope == "erc7579" {
                    WrapperKind::Erc4337
                } else {
                    WrapperKind::Direct
                }
            );
            let steps =
                bankr_rpc_steps(proof, expected_quote.market.leader, &runtime_fixture, None);
            let expected_requests = steps.len();
            let (rpc, server) = spawn_exact_rpc_server(steps).await;
            let reconciled = reconcile_candidate(
                &rpc,
                candidate,
                Duration::from_secs(1),
                Duration::from_millis(1),
                bankr_quote_policy(),
                bankr_reconcile_profiles(),
            )
            .await
            .unwrap();
            let consumed = tokio::time::timeout(Duration::from_secs(2), server)
                .await
                .expect("concrete RPC transcript was not fully consumed")
                .unwrap();
            assert_eq!(consumed, expected_requests);
            assert_eq!(rpc.metrics().logical_requests as usize, expected_requests);

            assert!(reconciled.evidence.receipt_status);
            assert!(reconciled.evidence.protocol_event_match);
            assert!(reconciled.evidence.observer_claim);
            assert_eq!(reconciled.evidence.action, Some(ActionKind::Launch));
            assert_eq!(reconciled.evidence.token, Some(expected_quote.market.token));
            assert_eq!(reconciled.evidence.pool, None);
            assert_eq!(
                reconciled.evidence.pool_id,
                Some(expected_quote.market.pool_id)
            );
            assert_eq!(reconciled.evidence.quote_status, QuoteStatus::Available);
            assert_eq!(
                reconciled.evidence.l2_block_number,
                Some(proof.receipt.l2_block_number)
            );
            assert_eq!(
                reconciled.evidence.block_hash,
                Some(proof.receipt.block_hash)
            );
            assert!(reconciled.evidence.protocol_blocker.is_none());
            let actual_quote = reconciled.bankr_quote.as_ref().unwrap();
            assert_eq!(actual_quote, &expected_quote);
            assert!(actual_quote.entry.expected_output > U256::ZERO);
            assert!(actual_quote.full_position_exit.expected_output > U256::ZERO);
            assert!(!actual_quote.execution_eligible);
            assert!(!actual_quote.broadcast);
            assert!(reconciled.v3_quote.is_none());
            assert!(reconciled.clanker_quote.is_none());
            assert!(reconciled.pons_quote.is_none());
            assert!(reconciled.hood_quote.is_none());
            assert!(reconciled.hood_migration.is_none());
        }

        let frame = frames
            .frames
            .iter()
            .find(|frame| frame.envelope == "direct_airlock")
            .unwrap();
        let proof = proofs
            .launches
            .iter()
            .find(|proof| proof.transaction.hash == frame.tx_hash)
            .unwrap();
        let expected_quote: BankrDopplerReceiptPaperQuote = serde_json::from_str(include_str!(
            "../../tests/fixtures/bankr-doppler-v4-finaltuple-direct-paper-quote.json"
        ))
        .unwrap();
        let steps = bankr_rpc_steps(
            proof,
            expected_quote.market.leader,
            &runtime_fixture,
            Some("airlock"),
        );
        let expected_requests = steps.len();
        let (rpc, server) = spawn_exact_rpc_server(steps).await;
        let rejected = reconcile_candidate(
            &rpc,
            exact_bankr_candidate(frame),
            Duration::from_secs(1),
            Duration::from_millis(1),
            bankr_quote_policy(),
            bankr_reconcile_profiles(),
        )
        .await
        .unwrap();
        let consumed = tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("drift RPC transcript was not fully consumed")
            .unwrap();
        assert_eq!(consumed, expected_requests);
        assert_eq!(rpc.metrics().logical_requests as usize, expected_requests);
        assert_eq!(rejected.evidence.quote_status, QuoteStatus::Blocked);
        assert!(
            rejected
                .evidence
                .protocol_blocker
                .as_deref()
                .unwrap()
                .contains("receipt-block dependency")
        );
        assert!(rejected.bankr_quote.is_none());
        assert!(rejected.v3_quote.is_none());
        assert!(rejected.clanker_quote.is_none());
        assert!(rejected.pons_quote.is_none());
        assert!(rejected.hood_quote.is_none());
        assert!(rejected.hood_migration.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bankr_v5_raw_nitro_frame_crosses_strict_parser_and_concrete_rpc_reconciler() {
        let frames: BankrV4RawFrames = serde_json::from_str(include_str!(
            "../../tests/fixtures/bankr-doppler-v5-fresh-raw-frame.json"
        ))
        .unwrap();
        let proofs: BankrV4ProofSet = serde_json::from_str(include_str!(
            "../../tests/fixtures/bankr-doppler-v5-fresh-six-live-proofs.json"
        ))
        .unwrap();
        let runtime_fixture: BankrV4RuntimeFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/bankr-doppler-v5-concrete-reconciler-runtime-code.json"
        ))
        .unwrap();
        let expected_quote: BankrDopplerReceiptPaperQuote = serde_json::from_str(include_str!(
            "../../tests/fixtures/bankr-doppler-v5-fresh-paper-quote.json"
        ))
        .unwrap();
        assert_bankr_runtime_fixture(&runtime_fixture, 1);
        assert_eq!(frames.frames.len(), 1);
        assert_eq!(runtime_fixture.verified_l2_blocks.len(), 1);
        let frame = &frames.frames[0];
        let proof = proofs
            .launches
            .iter()
            .find(|proof| proof.transaction.hash == frame.tx_hash)
            .unwrap();
        assert_eq!(proof.envelope, "erc7579");
        assert_eq!(
            expected_quote.market.create_profile_version,
            hermes_feed::BankrCreateProfileVersion::CurveTicksV5
        );
        assert_eq!(
            expected_quote.market.envelope,
            hermes_feed::BankrEnvelopeKind::Erc7579
        );
        let runtime_block = &runtime_fixture.verified_l2_blocks[0];
        assert_eq!(runtime_block.transaction_hash, frame.tx_hash);
        assert_eq!(runtime_block.l2_block_number, proof.receipt.l2_block_number);
        assert_eq!(
            runtime_block.block_tag,
            format!("0x{:x}", proof.receipt.l2_block_number)
        );
        let candidate = exact_bankr_candidate(frame);
        assert_eq!(candidate.wrapper, WrapperKind::Erc4337);

        let steps = bankr_rpc_steps(proof, expected_quote.market.leader, &runtime_fixture, None);
        let expected_requests = steps.len();
        let (rpc, server) = spawn_exact_rpc_server(steps).await;
        let reconciled = reconcile_candidate(
            &rpc,
            candidate,
            Duration::from_secs(1),
            Duration::from_millis(1),
            bankr_quote_policy(),
            bankr_reconcile_profiles(),
        )
        .await
        .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), server)
                .await
                .unwrap()
                .unwrap(),
            expected_requests
        );
        assert_eq!(rpc.metrics().logical_requests as usize, expected_requests);
        assert!(reconciled.evidence.receipt_status);
        assert!(reconciled.evidence.protocol_event_match);
        assert!(reconciled.evidence.observer_claim);
        assert_eq!(reconciled.evidence.quote_status, QuoteStatus::Available);
        assert_eq!(reconciled.bankr_quote.as_ref(), Some(&expected_quote));
        assert!(!expected_quote.execution_eligible);
        assert!(!expected_quote.broadcast);

        let steps = bankr_rpc_steps(
            proof,
            expected_quote.market.leader,
            &runtime_fixture,
            Some("entry_point_v07"),
        );
        let expected_requests = steps.len();
        let (rpc, server) = spawn_exact_rpc_server(steps).await;
        let blocked = reconcile_candidate(
            &rpc,
            exact_bankr_candidate(frame),
            Duration::from_secs(1),
            Duration::from_millis(1),
            bankr_quote_policy(),
            bankr_reconcile_profiles(),
        )
        .await
        .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), server)
                .await
                .unwrap()
                .unwrap(),
            expected_requests
        );
        assert_eq!(blocked.evidence.quote_status, QuoteStatus::Blocked);
        assert!(
            blocked
                .evidence
                .protocol_blocker
                .as_deref()
                .unwrap()
                .contains("receipt-block dependency")
        );
        assert!(blocked.bankr_quote.is_none());
    }

    #[test]
    fn collector_deserializes_direct_and_exact_eip7702_requests_and_rejects_malformed_provenance() {
        let direct = parse_observer_frame(observer_frame()).unwrap();
        let direct_candidate = direct.candidates.values().next().unwrap();
        assert_eq!(direct_candidate.wrapper, WrapperKind::Direct);
        assert!(direct_candidate.wrapper_provenance.is_none());

        let profile = Eip7702SelfBatchExpectedPins::production();
        let provenance = profile.expected_provenance().unwrap();
        let mut wrapped = json!({
            "record_type": "launchpad_paper_frame",
            "report": {
                "observations": [{
                    "tx_hash": profile.proof_transaction,
                    "launchpad": "pons",
                    "feed_sequence": 42,
                    "l1_block_number": 25_549_554,
                    "l1_timestamp": 1_784_256_986,
                    "wrapper": "eip7702_self_batch"
                }],
                "reconciliation_requests": [{
                    "tx_hash": profile.proof_transaction,
                    "launchpad": "pons",
                    "feed_sequence": 42,
                    "l1_block_number": 25_549_554,
                    "l1_timestamp": 1_784_256_986,
                    "evidence_source": "independent_receipt_and_protocol_events",
                    "initial_decision_dependency": false,
                    "wrapper": "eip7702_self_batch",
                    "wrapper_provenance": provenance
                }]
            }
        });
        let parsed = parse_observer_frame(wrapped.clone()).unwrap();
        let candidate = parsed
            .candidates
            .get(&(profile.proof_transaction, LaunchpadId::Pons))
            .unwrap();
        assert_eq!(candidate.wrapper, WrapperKind::Eip7702SelfBatch);
        assert_eq!(candidate.wrapper_provenance, Some(provenance.clone()));

        *wrapped
            .pointer_mut("/report/reconciliation_requests/0/wrapper_provenance/authority")
            .unwrap() = json!(Address::with_last_byte(1));
        assert!(
            parse_observer_frame(wrapped)
                .unwrap_err()
                .to_string()
                .contains("incomplete EIP-7702 provenance")
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
    fn collector_requires_exactly_one_safe_capabilities_provenance_record() {
        let frame = observer_frame();
        let mut missing = serde_json::to_vec(&frame).unwrap();
        missing.push(b'\n');
        assert!(
            read_observer_input_from_reader(Cursor::new(&missing), keccak256(&missing))
                .unwrap_err()
                .to_string()
                .contains("no paper capabilities provenance")
        );

        let mut duplicate = serde_json::to_vec(&capabilities_record()).unwrap();
        duplicate.push(b'\n');
        duplicate.extend(serde_json::to_vec(&capabilities_record()).unwrap());
        duplicate.push(b'\n');
        duplicate.extend(serde_json::to_vec(&frame).unwrap());
        duplicate.push(b'\n');
        assert!(
            read_observer_input_from_reader(Cursor::new(&duplicate), keccak256(&duplicate))
                .unwrap_err()
                .to_string()
                .contains("duplicate paper capabilities")
        );

        let mut unsafe_record = capabilities_record();
        *unsafe_record.get_mut("broadcast").unwrap() = json!(true);
        let mut unsafe_bytes = serde_json::to_vec(&unsafe_record).unwrap();
        unsafe_bytes.push(b'\n');
        unsafe_bytes.extend(serde_json::to_vec(&frame).unwrap());
        unsafe_bytes.push(b'\n');
        assert!(
            read_observer_input_from_reader(Cursor::new(&unsafe_bytes), keccak256(&unsafe_bytes))
                .unwrap_err()
                .to_string()
                .contains("unsafe or malformed")
        );
    }

    #[test]
    fn reconciler_rejects_acquisition_and_pin_content_mismatches() {
        let provenance = observer_provenance();
        validate_reconciler_provenance_inputs(
            &provenance,
            EvidenceAcquisition::Live,
            B256::with_last_byte(1),
            B256::with_last_byte(2),
        )
        .unwrap();
        for (acquisition, expected, observed) in [
            (
                EvidenceAcquisition::Replay,
                B256::with_last_byte(1),
                B256::with_last_byte(2),
            ),
            (
                EvidenceAcquisition::Live,
                B256::with_last_byte(9),
                B256::with_last_byte(2),
            ),
            (
                EvidenceAcquisition::Live,
                B256::with_last_byte(1),
                B256::with_last_byte(9),
            ),
        ] {
            assert!(
                validate_reconciler_provenance_inputs(&provenance, acquisition, expected, observed)
                    .unwrap_err()
                    .to_string()
                    .contains("disagrees")
            );
        }
    }

    fn live_log(address: &str, log_index: u64, topics: &[&str], data: &str) -> ReceiptLog {
        ReceiptLog {
            address: Address::from_str(address).unwrap(),
            log_index,
            topics: topics
                .iter()
                .map(|topic| B256::from_str(topic).unwrap())
                .collect(),
            data: Bytes::from(hex::decode(data).unwrap()),
        }
    }

    fn bow_zero_buy_live_proof() -> (RobinhoodTransaction, NoxaReceipt) {
        let tx_hash =
            B256::from_str("1adcd30a5de19423f56b93d91df33d950179ed7ef4f9d4aae31fca13f72fc009")
                .unwrap();
        let logs = vec![
            live_log(
                "1f7d7550b1b028f7571e69a784071f0205fd2efa",
                1,
                &[
                    "783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118",
                    "00000000000000000000000000488257d5942b60119dc8c23dfe1c613c061b03",
                    "0000000000000000000000000bd7d308f8e1639fab988df18a8011f41eacad73",
                    "0000000000000000000000000000000000000000000000000000000000002710",
                ],
                "00000000000000000000000000000000000000000000000000000000000000c8000000000000000000000000d4759258987f7be17ae5afc7151da10bf54b2192",
            ),
            live_log(
                "d4759258987f7be17ae5afc7151da10bf54b2192",
                2,
                &["98636036cb66a9c19a37435efc1e90142190214e8abeb821bdba3f2990dd4c95"],
                "0000000000000000000000000000000000000000000289c75e384277ff7a6484fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffce64b",
            ),
            live_log(
                "d4759258987f7be17ae5afc7151da10bf54b2192",
                6,
                &[
                    "7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde",
                    "00000000000000000000000073991a25c818bf1f1128deaab1492d45638de0d3",
                    "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffce7d0",
                    "00000000000000000000000000000000000000000000000000000000000d89a0",
                ],
                "00000000000000000000000073991a25c818bf1f1128deaab1492d45638de0d300000000000000000000000000000000000000000000085cb16d31e60a6c05e20000000000000000000000000000000000000000033b2e3c9fd0803ce7ffc25c0000000000000000000000000000000000000000000000000000000000000000",
            ),
            live_log(
                "c70e510e14710ea535cab7b2414860af63feab79",
                12,
                &[
                    "ec774f0683e9ac48e8d835f412f9f877a8a5dee9af3170d78cf3ef33149d15e7",
                    "00000000000000000000000000488257d5942b60119dc8c23dfe1c613c061b03",
                    "000000000000000000000000660591c04dd40ac2d6604ecc2951e155fbd914b7",
                ],
                "000000000000000000000000d4759258987f7be17ae5afc7151da10bf54b2192000000000000000000000000000000000000000000000000000000000002a4c40000000000000000000000000000000000000000000000000000000000000462",
            ),
        ];
        (
            RobinhoodTransaction {
                hash: tx_hash,
                from: Address::from_str("660591c04dd40ac2d6604ecc2951e155fbd914b7").unwrap(),
                to: Some(BOW_LAUNCH_FACTORY),
                input: Bytes::new(),
                value: U256::ZERO,
                l2_block_number: Some(11_463_668),
                transaction_index: Some(1),
            },
            NoxaReceipt {
                transaction_hash: tx_hash,
                block_hash: B256::from_str(
                    "c15c854c65b16eae04478c619eaf930f3dfd897ce9e9e85b4cfb9448d82962cd",
                )
                .unwrap(),
                status: true,
                l2_block_number: 11_463_668,
                l1_block_number: Some(0x185d0bf),
                transaction_index: 1,
                gas_used: Some(0x6d680a),
                effective_gas_price: None,
                logs,
            },
        )
    }

    #[test]
    fn collector_wires_bow_receipt_to_replayable_two_leg_quote() {
        let (transaction, receipt) = bow_zero_buy_live_proof();
        let policy = V3ReceiptQuotePolicy {
            amount_in: U256::from(1_000_000_000_000_000_u64),
            max_amount_in: U256::from(10_000_000_000_000_000_u64),
            slippage_bps: 100,
        };
        let quote =
            collect_v3_receipt_quote(&transaction, &receipt, LaunchpadId::Bow, policy).unwrap();
        hermes_feed::validate_v3_quote_replay(&quote, policy).unwrap();
        assert_eq!(quote.tx_hash, transaction.hash);
        assert!(quote.entry.expected_output > U256::ZERO);
        assert!(quote.full_position_exit.expected_output > U256::ZERO);
        assert!(!quote.execution_eligible);
        assert!(!quote.broadcast);
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
            wrapper: WrapperKind::Direct,
            wrapper_provenance: None,
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
                wrapper: WrapperKind::Direct,
                wrapper_provenance: None,
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
    fn collector_dispatches_exact_clean4_eip7702_proof_to_wrapper_quote() {
        let fixture: PonsLiveFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/pons-eip7702-self-batch-clean4-proof.json"
        ))
        .unwrap();
        let profile = Eip7702SelfBatchExpectedPins::production();
        let provenance = profile.expected_provenance().unwrap();
        let outcome = strict_pons_reconciliation(
            &fixture.transaction,
            &fixture.receipt,
            WrapperKind::Eip7702SelfBatch,
            Some(&provenance),
            hermes_feed::PonsExpectedProfile::production(),
            Some(&profile),
            V3ReceiptQuotePolicy {
                amount_in: U256::from(1_000_000_000_000_000_u64),
                max_amount_in: U256::from(10_000_000_000_000_000_u64),
                slippage_bps: 100,
            },
        );
        assert_eq!(
            outcome.generation,
            Some(hermes_feed::PonsGeneration::Current)
        );
        assert!(outcome.blocker.is_none());
        let quote = outcome.quote.unwrap();
        assert_eq!(quote.wrapper_provenance, Some(provenance));
        assert!(quote.entry.expected_output > U256::ZERO);
        assert!(quote.full_position_exit.expected_output > U256::ZERO);
        assert!(!quote.execution_eligible);
        assert!(!quote.broadcast);
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
            WrapperKind::Direct,
            None,
            hermes_feed::PonsExpectedProfile::production(),
            None,
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

        let mut legacy = fixture.transaction.clone();
        legacy.to = Some(PONS_LEGACY_FACTORY);
        let mut legacy_receipt = fixture.receipt.clone();
        for log in &mut legacy_receipt.logs {
            if log.address == PONS_CURRENT_FACTORY {
                log.address = PONS_LEGACY_FACTORY;
            }
        }
        let legacy = strict_pons_reconciliation(
            &legacy,
            &legacy_receipt,
            WrapperKind::Direct,
            None,
            hermes_feed::PonsExpectedProfile::production(),
            None,
            policy,
        );
        assert_eq!(legacy.generation, Some(hermes_feed::PonsGeneration::Legacy));
        assert!(legacy.quote.is_none());
        assert!(legacy.blocker.unwrap().contains("discovery_only"));
    }

    #[test]
    fn wrapped_current_pons_launch_is_classified_from_receipt_but_quote_blocked() {
        let fixture: PonsLiveFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/pons-current-live-proof.json"
        ))
        .unwrap();
        let policy = V3ReceiptQuotePolicy {
            amount_in: U256::from(1_000_000_000_000_000_u64),
            max_amount_in: U256::from(10_000_000_000_000_000_u64),
            slippage_bps: 100,
        };
        let mut wrapped = fixture.transaction;
        wrapped.to = Some(wrapped.from);
        wrapped.value = U256::ZERO;
        wrapped.input = alloy_primitives::Bytes::from_static(&[0x3f, 0x70, 0x7e, 0x6b]);

        let outcome = strict_pons_reconciliation(
            &wrapped,
            &fixture.receipt,
            WrapperKind::Direct,
            None,
            hermes_feed::PonsExpectedProfile::production(),
            None,
            policy,
        );

        assert_eq!(
            outcome.generation,
            Some(hermes_feed::PonsGeneration::Current)
        );
        assert!(outcome.quote.is_none());
        assert!(
            outcome
                .blocker
                .unwrap()
                .contains("transaction, receipt, or paper policy envelope is invalid")
        );
    }

    #[test]
    fn mixed_pons_factory_events_leave_generation_ambiguous_and_fail_closed() {
        let fixture: PonsLiveFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/pons-current-live-proof.json"
        ))
        .unwrap();
        let policy = V3ReceiptQuotePolicy {
            amount_in: U256::from(1_000_000_000_000_000_u64),
            max_amount_in: U256::from(10_000_000_000_000_000_u64),
            slippage_bps: 100,
        };
        let mut mixed_receipt = fixture.receipt;
        let mut second_launch = mixed_receipt
            .logs
            .iter()
            .find(|log| pons_launch_event_identity(log).is_some())
            .unwrap()
            .clone();
        second_launch.address = PONS_LEGACY_FACTORY;
        second_launch.log_index = mixed_receipt.logs.last().unwrap().log_index + 1;
        mixed_receipt.logs.push(second_launch);

        let outcome = strict_pons_reconciliation(
            &fixture.transaction,
            &mixed_receipt,
            WrapperKind::Direct,
            None,
            hermes_feed::PonsExpectedProfile::production(),
            None,
            policy,
        );

        assert_eq!(outcome.generation, None);
        assert!(outcome.quote.is_none());
        assert_eq!(
            outcome.blocker.as_deref(),
            Some("pons_receipt_factory_generation_missing_or_ambiguous")
        );
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
