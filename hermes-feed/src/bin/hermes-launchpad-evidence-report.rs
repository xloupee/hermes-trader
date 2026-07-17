use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use alloy_primitives::{B256, U256};
use anyhow::{Context, Result, bail};
use clap::Parser;
use hermes_feed::evidence_provenance::{
    maybe_print_self_digest, read_bytes_with_keccak, verify_expected_self_keccak256,
};
use hermes_feed::launchpad_adapter::LaunchpadId;
use hermes_feed::launchpad_readiness::{
    evaluate_completed_session_readiness, supported_profile_envelopes,
};
use hermes_feed::launchpad_session::{
    ValidatedPaperSession, ensure_compatible_independent_sessions, validate_completed_session,
};
use serde::Serialize;
use serde_json::{Value, json};

const LAUNCHPADS: [LaunchpadId; 6] = [
    LaunchpadId::Bow,
    LaunchpadId::LaunchHoodV3,
    LaunchpadId::Clanker,
    LaunchpadId::BankrDoppler,
    LaunchpadId::Pons,
    LaunchpadId::HoodFun,
];

#[derive(Debug, Parser)]
#[command(about = "Deterministic, paper-only aggregate over completed launchpad evidence sessions")]
struct Cli {
    /// Print a file's Keccak-256 digest and exit. Used to lock reviewed pins.
    #[arg(long, conflicts_with_all = ["session_dirs", "readiness_output", "expected_pins", "readiness_keccak256"])]
    print_file_keccak256: Option<PathBuf>,
    #[arg(long = "session-dir")]
    session_dirs: Vec<PathBuf>,
    #[arg(long = "partial-session-dir")]
    partial_session_dirs: Vec<PathBuf>,
    #[arg(long)]
    readiness_output: Option<PathBuf>,
    #[arg(long)]
    expected_pins: Option<PathBuf>,
    #[arg(long)]
    readiness_keccak256: Option<B256>,
    #[arg(long)]
    expected_self_keccak256: Option<B256>,
    #[arg(long)]
    campaign_lock: Option<PathBuf>,
    #[arg(long)]
    snapshot_keccak256: Option<B256>,
    #[arg(long)]
    local_runner_keccak256: Option<B256>,
}

#[derive(Debug, Default)]
struct Counts {
    confirmed: u64,
    quote_eligible: u64,
    false_positives: u64,
    detector_misses: u64,
    feed_coverage_misses: u64,
    identity_mismatches: u64,
    direction_mismatches: u64,
    prediction_mismatches: u64,
    quote_mismatches: u64,
}

#[derive(Debug, Serialize)]
struct PlanSizing {
    tx_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope_token: Option<String>,
    amount_in: U256,
    expected_output: U256,
    min_receive: U256,
    slippage_bps: U256,
}

#[derive(Debug, Serialize)]
struct ProfileCount {
    profile_envelope: String,
    observations: u64,
    missing: bool,
}

#[derive(Debug, Serialize)]
struct CoverageWindow {
    from_l2_block: u64,
    to_l2_block: u64,
    start_head_hash: B256,
    cutoff_head_hash: B256,
    manifest_content_keccak256: B256,
    snapshot_content_keccak256: B256,
}

fn main() -> Result<()> {
    if maybe_print_self_digest()? {
        return Ok(());
    }
    let cli = Cli::parse();
    if let Some(path) = cli.print_file_keccak256.as_ref() {
        let (_, digest) = read_bytes_with_keccak(path, "digest input")?;
        println!("{digest}");
        return Ok(());
    }
    if cli.session_dirs.is_empty() {
        bail!("aggregate mode requires at least one --session-dir");
    }
    let report_keccak256 = cli
        .expected_self_keccak256
        .context("aggregate mode requires --expected-self-keccak256")?;
    verify_expected_self_keccak256(report_keccak256)?;
    let campaign_lock = cli
        .campaign_lock
        .as_ref()
        .context("aggregate mode requires --campaign-lock")?;
    let snapshot_keccak256 = cli
        .snapshot_keccak256
        .context("aggregate mode requires --snapshot-keccak256")?;
    let local_runner_keccak256 = cli
        .local_runner_keccak256
        .context("aggregate mode requires --local-runner-keccak256")?;
    let readiness_output = cli
        .readiness_output
        .as_ref()
        .context("missing --readiness-output")?;
    let expected_pins = cli
        .expected_pins
        .as_ref()
        .context("missing --expected-pins")?;
    let readiness_keccak256 = cli
        .readiness_keccak256
        .context("missing --readiness-keccak256")?;
    if readiness_keccak256 == B256::ZERO {
        bail!("readiness executable digest must be nonzero");
    }
    for partial in &cli.partial_session_dirs {
        let metadata = std::fs::symlink_metadata(partial)
            .with_context(|| format!("inspect partial session {}", partial.display()))?;
        if !metadata.is_dir()
            || metadata.permissions().mode() & 0o077 != 0
            || partial.join("session-completion-manifest.json").exists()
            || std::fs::read_dir(partial)?.next().is_none()
        {
            bail!(
                "excluded partial session {} is absent, non-private, empty, or claims completion",
                partial.display()
            );
        }
    }

    let sessions = cli
        .session_dirs
        .iter()
        .map(|directory| {
            validate_completed_session(directory)
                .with_context(|| format!("validate completed session {}", directory.display()))
        })
        .collect::<Result<Vec<_>>>()?;
    ensure_compatible_independent_sessions(&sessions, readiness_keccak256)?;
    validate_non_overlapping(&sessions)?;

    let (_, expected_pins_digest) =
        read_bytes_with_keccak(expected_pins, "campaign expected pins")?;
    validate_expected_pins(&sessions, expected_pins_digest)?;
    let (campaign_lock_bytes, campaign_lock_digest) =
        read_bytes_with_keccak(campaign_lock, "campaign lock")?;
    validate_campaign_lock(
        &campaign_lock_bytes,
        expected_pins_digest,
        &sessions[0].executables,
        report_keccak256,
        snapshot_keccak256,
        local_runner_keccak256,
    )?;

    let expected_readiness = evaluate_completed_session_readiness(
        &sessions
            .iter()
            .flat_map(|s| s.windows.clone())
            .collect::<Vec<_>>(),
        &sessions
            .iter()
            .map(|s| s.manifest_content_keccak256)
            .collect::<Vec<_>>(),
        sessions[0].executables.feed_keccak256,
        sessions[0].executables.chain_head_keccak256,
        readiness_keccak256,
    )?;
    let (_, readiness_output_digest_before) =
        read_bytes_with_keccak(readiness_output, "authoritative readiness output")?;
    let authoritative = read_jsonl(readiness_output)?;
    let (_, readiness_output_digest_after) =
        read_bytes_with_keccak(readiness_output, "authoritative readiness output")?;
    if readiness_output_digest_before != readiness_output_digest_after {
        bail!("authoritative readiness output changed while being consumed");
    }
    let expected_values = expected_readiness
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    if authoritative != expected_values {
        bail!("authoritative readiness output disagrees with validated campaign sessions");
    }
    let readiness_by_launchpad = authoritative
        .into_iter()
        .map(|value| {
            let launchpad = parse_launchpad(&value, "launchpad")?;
            if value.get("input_trust").and_then(Value::as_str)
                != Some("completed_session_manifest")
                || value.get("authorizes_canary").and_then(Value::as_bool) != Some(false)
                || value.get("execution_eligible").and_then(Value::as_bool) != Some(false)
            {
                bail!("authoritative readiness output is not fail-closed trusted paper evidence");
            }
            Ok((launchpad, value))
        })
        .collect::<Result<HashMap<_, _>>>()?;

    let mut aggregate: HashMap<LaunchpadId, Counts> = HashMap::new();
    let mut profiles: HashMap<LaunchpadId, BTreeMap<String, u64>> = HashMap::new();
    let mut latencies: HashMap<LaunchpadId, Vec<u64>> = HashMap::new();
    let mut entries: HashMap<LaunchpadId, Vec<PlanSizing>> = HashMap::new();
    let mut exits: HashMap<LaunchpadId, Vec<PlanSizing>> = HashMap::new();
    let mut round_trips: HashMap<LaunchpadId, Vec<U256>> = HashMap::new();
    let mut windows: HashMap<LaunchpadId, Vec<CoverageWindow>> = HashMap::new();

    for (directory, session) in cli.session_dirs.iter().zip(&sessions) {
        collect_window_counts(session, &mut aggregate, &mut profiles, &mut windows)?;
        collect_metrics(directory, &mut aggregate)?;
        collect_confirmed_latencies(directory, &mut latencies)?;
        collect_plans(directory, &mut entries, &mut exits, &mut round_trips)?;
    }

    for launchpad in LAUNCHPADS {
        let counts = aggregate.remove(&launchpad).unwrap_or_default();
        let mut samples = latencies.remove(&launchpad).unwrap_or_default();
        samples.sort_unstable();
        if u64::try_from(samples.len()).ok() != Some(counts.confirmed) {
            bail!("confirmed observation and latency sample counts disagree");
        }
        let mut outcomes = round_trips.remove(&launchpad).unwrap_or_default();
        outcomes.sort_unstable();
        let mut entry_plans = entries.remove(&launchpad).unwrap_or_default();
        entry_plans.sort_by(|left, right| left.tx_hash.cmp(&right.tx_hash));
        let mut exit_plans = exits.remove(&launchpad).unwrap_or_default();
        exit_plans.sort_by(|left, right| left.tx_hash.cmp(&right.tx_hash));
        let profile_counts = profiles.remove(&launchpad).unwrap_or_default();
        let profile_rows = supported_profile_envelopes(launchpad)
            .context("campaign includes unsupported readiness launchpad")?
            .iter()
            .map(|name| {
                let observations = profile_counts.get(*name).copied().unwrap_or_default();
                ProfileCount {
                    profile_envelope: (*name).to_owned(),
                    observations,
                    missing: observations == 0,
                }
            })
            .collect::<Vec<_>>();
        let readiness = readiness_by_launchpad
            .get(&launchpad)
            .context("authoritative readiness omitted launchpad")?;
        let value = json!({
            "record_type": "launchpad_paper_campaign_report",
            "schema_version": 1,
            "launchpad": launchpad,
            "campaign": {
                "accepted_window_count": sessions.len(),
                "excluded_partial_window_count": cli.partial_session_dirs.len(),
                "coverage_windows": sorted_windows(windows.remove(&launchpad).unwrap_or_default()),
            },
            "provenance": {
                "acquisition": "live",
                "expected_pins_content_keccak256": expected_pins_digest,
                "readiness_output_content_keccak256": readiness_output_digest_before,
                "executables": sessions[0].executables,
                "orchestration": {
                    "report_keccak256": report_keccak256,
                    "pin_snapshot_keccak256": snapshot_keccak256,
                    "local_runner_keccak256": local_runner_keccak256,
                    "campaign_lock_content_keccak256": campaign_lock_digest,
                },
            },
            "observations": {"confirmed": counts.confirmed, "eligible": counts.quote_eligible},
            "latency_ns": {
                "sample_count": samples.len(),
                "p50": percentile(&samples, 50),
                "p95": percentile(&samples, 95),
                "p99": percentile(&samples, 99),
            },
            "errors": {
                "false_positives": counts.false_positives,
                "detector_misses": counts.detector_misses,
                "coverage_misses": counts.feed_coverage_misses,
                "identity_mismatches": counts.identity_mismatches,
                "direction_mismatches": counts.direction_mismatches,
                "prediction_mismatches": counts.prediction_mismatches,
                "quote_mismatches": counts.quote_mismatches,
            },
            "profiles": profile_rows,
            "entry": {"plan_count": entry_plans.len(), "plans": entry_plans},
            "exit": {"plan_count": exit_plans.len(), "plans": exit_plans},
            "simulated_round_trip_return_bps": {
                "sample_count": outcomes.len(),
                "min": outcomes.first(),
                "p50": percentile(&outcomes, 50),
                "p95": percentile(&outcomes, 95),
                "p99": percentile(&outcomes, 99),
                "max": outcomes.last(),
            },
            "readiness": {
                "paper_evidence_ready": readiness["paper_evidence_ready"],
                "authorizes_canary": readiness["authorizes_canary"],
                "execution_eligible": readiness["execution_eligible"],
                "input_trust": readiness["input_trust"],
                "failures": readiness["failures"],
            },
        });
        println!("{}", serde_json::to_string(&value)?);
    }
    Ok(())
}

fn validate_non_overlapping(sessions: &[ValidatedPaperSession]) -> Result<()> {
    let mut ranges = sessions
        .iter()
        .map(|session| {
            let window = session
                .windows
                .first()
                .context("session has no readiness windows")?;
            Ok((window.coverage_from_l2_block, window.coverage_to_l2_block))
        })
        .collect::<Result<Vec<_>>>()?;
    ranges.sort_unstable();
    for pair in ranges.windows(2) {
        if pair[1].0 <= pair[0].1 {
            bail!("completed campaign windows overlap");
        }
    }
    Ok(())
}

fn validate_expected_pins(sessions: &[ValidatedPaperSession], digest: B256) -> Result<()> {
    for session in sessions {
        for window in &session.windows {
            let provenance = window
                .provenance
                .as_ref()
                .context("readiness window lacks provenance")?;
            if provenance.expected_pins_content_keccak256 != digest {
                bail!("completed session expected-pin digest differs from campaign lock");
            }
        }
    }
    Ok(())
}

fn validate_campaign_lock(
    bytes: &[u8],
    expected_pins: B256,
    executables: &hermes_feed::launchpad_session::SessionExecutables,
    report: B256,
    snapshot: B256,
    local_runner: B256,
) -> Result<()> {
    let value: Value = serde_json::from_slice(bytes).context("decode campaign lock")?;
    if value.get("record_type").and_then(Value::as_str) != Some("launchpad_paper_campaign_lock")
        || value.get("schema_version").and_then(Value::as_u64) != Some(1)
        || value.get("expected_pins_content_keccak256")
            != Some(&serde_json::to_value(expected_pins)?)
        || value.get("executables") != Some(&serde_json::to_value(executables)?)
        || value.pointer("/orchestration/report_keccak256") != Some(&serde_json::to_value(report)?)
        || value.pointer("/orchestration/pin_snapshot_keccak256")
            != Some(&serde_json::to_value(snapshot)?)
        || value.pointer("/orchestration/local_runner_keccak256")
            != Some(&serde_json::to_value(local_runner)?)
    {
        bail!("campaign lock disagrees with the validated executable and pin tuple");
    }
    Ok(())
}

fn collect_window_counts(
    session: &ValidatedPaperSession,
    counts: &mut HashMap<LaunchpadId, Counts>,
    profiles: &mut HashMap<LaunchpadId, BTreeMap<String, u64>>,
    windows: &mut HashMap<LaunchpadId, Vec<CoverageWindow>>,
) -> Result<()> {
    for window in &session.windows {
        let row = counts.entry(window.launchpad).or_default();
        row.quote_eligible = checked_add(
            row.quote_eligible,
            window.quote_eligible_confirmed_observations,
        )?;
        row.false_positives = checked_add(row.false_positives, window.false_positives)?;
        row.detector_misses = checked_add(row.detector_misses, window.detector_misses)?;
        row.identity_mismatches = checked_add(row.identity_mismatches, window.identity_mismatches)?;
        row.direction_mismatches =
            checked_add(row.direction_mismatches, window.direction_mismatches)?;
        row.prediction_mismatches =
            checked_add(row.prediction_mismatches, window.prediction_mismatches)?;
        row.quote_mismatches = checked_add(row.quote_mismatches, window.quote_mismatches)?;
        for (name, value) in &window.profile_envelope_observations {
            let total = profiles
                .entry(window.launchpad)
                .or_default()
                .entry(name.clone())
                .or_default();
            *total = checked_add(*total, *value)?;
        }
        windows
            .entry(window.launchpad)
            .or_default()
            .push(CoverageWindow {
                from_l2_block: window.coverage_from_l2_block,
                to_l2_block: window.coverage_to_l2_block,
                start_head_hash: window.start_head_hash,
                cutoff_head_hash: window.cutoff_head_hash,
                manifest_content_keccak256: session.manifest_content_keccak256,
                snapshot_content_keccak256: session.observed_snapshot_content_keccak256,
            });
    }
    Ok(())
}

fn collect_metrics(directory: &Path, aggregate: &mut HashMap<LaunchpadId, Counts>) -> Result<()> {
    let rows = read_jsonl(&directory.join("launchpad-paper-finalized.jsonl"))?;
    let mut seen = HashSet::new();
    for value in rows.iter().filter(|v| {
        v.get("record_type").and_then(Value::as_str)
            == Some("launchpad_paper_reconciliation_metrics")
    }) {
        let launchpad = parse_launchpad(value, "launchpad")?;
        if !seen.insert(launchpad) {
            bail!("duplicate reconciliation metrics row");
        }
        let row = aggregate.entry(launchpad).or_default();
        row.confirmed = checked_add(row.confirmed, u64_field(value, "confirmed_observations")?)?;
        row.feed_coverage_misses = checked_add(
            row.feed_coverage_misses,
            u64_field(value, "feed_coverage_misses")?,
        )?;
        // Collapsed mismatch counters are already cross-checked through readiness windows.
    }
    if seen.len() != LAUNCHPADS.len() {
        bail!("finalized evidence must contain one metrics row per launchpad");
    }
    Ok(())
}

fn collect_confirmed_latencies(
    directory: &Path,
    output: &mut HashMap<LaunchpadId, Vec<u64>>,
) -> Result<()> {
    let reconciliation = read_jsonl(&directory.join("reconciliation-evidence.jsonl"))?;
    let confirmed = reconciliation
        .iter()
        .filter(|value| {
            value.get("record_type").and_then(Value::as_str)
                == Some("launchpad_reconciliation_evidence")
                && value.get("observer_claim").and_then(Value::as_bool) == Some(true)
                && value.get("ground_truth_event").and_then(Value::as_bool) == Some(true)
        })
        .map(|value| {
            Ok((
                string_field(value, "tx_hash")?.to_owned(),
                parse_launchpad(value, "launchpad")?,
            ))
        })
        .collect::<Result<HashSet<_>>>()?;
    let mut found = HashSet::new();
    for frame in read_jsonl(&directory.join("launchpad-paper.jsonl"))? {
        if frame.get("record_type").and_then(Value::as_str) != Some("launchpad_paper_frame") {
            continue;
        }
        let observations = frame
            .pointer("/report/observations")
            .and_then(Value::as_array)
            .context("paper frame lacks observations")?;
        for observation in observations {
            let key = (
                string_field(observation, "tx_hash")?.to_owned(),
                parse_launchpad(observation, "launchpad")?,
            );
            if confirmed.contains(&key) {
                if !found.insert(key.clone()) {
                    bail!("duplicate confirmed observer latency");
                }
                output
                    .entry(key.1)
                    .or_default()
                    .push(u64_field(observation, "observer_latency_ns")?);
            }
        }
    }
    if found != confirmed {
        bail!("confirmed reconciliation evidence lacks observer latency");
    }
    Ok(())
}

fn collect_plans(
    directory: &Path,
    entries: &mut HashMap<LaunchpadId, Vec<PlanSizing>>,
    exits: &mut HashMap<LaunchpadId, Vec<PlanSizing>>,
    round_trips: &mut HashMap<LaunchpadId, Vec<U256>>,
) -> Result<()> {
    for value in read_jsonl(&directory.join("launchpad-paper-finalized.jsonl"))? {
        if value.get("record_type").and_then(Value::as_str)
            != Some("launchpad_paper_finalized_plan")
        {
            continue;
        }
        if value.get("execution_eligible").and_then(Value::as_bool) != Some(false)
            || value.get("broadcast").and_then(Value::as_bool) != Some(false)
            || value.get("exit_full_position").and_then(Value::as_bool) != Some(true)
            || value
                .pointer("/exit_plan/full_position")
                .and_then(Value::as_bool)
                != Some(true)
            || value
                .pointer("/exit_plan/execution_eligible")
                .and_then(Value::as_bool)
                != Some(false)
            || value
                .pointer("/exit_plan/broadcast")
                .and_then(Value::as_bool)
                != Some(false)
        {
            bail!("finalized plan is not paper-only");
        }
        let launchpad = parse_launchpad(&value, "launchpad")?;
        let tx_hash = string_field(&value, "tx_hash")?.to_owned();
        let scope_token = value
            .get("scope_token")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let amount_in = u256_field(&value, "amount_in")?;
        let expected = u256_field(&value, "expected_output")?;
        let minimum = u256_field(&value, "min_receive")?;
        entries.entry(launchpad).or_default().push(plan_sizing(
            tx_hash.clone(),
            scope_token.clone(),
            amount_in,
            expected,
            minimum,
        )?);
        let exit_expected = u256_field(&value, "exit_expected_output")?;
        let exit_minimum = u256_field(&value, "exit_min_receive")?;
        exits.entry(launchpad).or_default().push(plan_sizing(
            tx_hash,
            scope_token,
            expected,
            exit_expected,
            exit_minimum,
        )?);
        round_trips
            .entry(launchpad)
            .or_default()
            .push(u256_field(&value, "simulated_round_trip_return_bps")?);
    }
    Ok(())
}

fn plan_sizing(
    tx_hash: String,
    scope_token: Option<String>,
    amount: U256,
    expected: U256,
    minimum: U256,
) -> Result<PlanSizing> {
    let haircut = expected
        .checked_sub(minimum)
        .context("minimum receive exceeds expected output")?;
    let slippage_bps = if expected == U256::ZERO {
        bail!("expected output must be nonzero for slippage reporting");
    } else {
        haircut
            .checked_mul(U256::from(10_000))
            .context("slippage numerator overflow")?
            / expected
    };
    Ok(PlanSizing {
        tx_hash,
        scope_token,
        amount_in: amount,
        expected_output: expected,
        min_receive: minimum,
        slippage_bps,
    })
}

fn read_jsonl(path: &Path) -> Result<Vec<Value>> {
    BufReader::new(File::open(path).with_context(|| format!("open {}", path.display()))?)
        .lines()
        .enumerate()
        .filter_map(|(index, line)| match line {
            Ok(line) if line.trim().is_empty() => None,
            Ok(line) => Some(
                serde_json::from_str(&line)
                    .with_context(|| format!("decode {} line {}", path.display(), index + 1)),
            ),
            Err(error) => Some(
                Err(error).with_context(|| format!("read {} line {}", path.display(), index + 1)),
            ),
        })
        .collect()
}

fn parse_launchpad(value: &Value, field: &str) -> Result<LaunchpadId> {
    serde_json::from_value(
        value
            .get(field)
            .cloned()
            .with_context(|| format!("missing {field}"))?,
    )
    .with_context(|| format!("decode {field}"))
}
fn string_field<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("missing string {field}"))
}
fn u64_field(value: &Value, field: &str) -> Result<u64> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .with_context(|| format!("missing integer {field}"))
}
fn u256_field(value: &Value, field: &str) -> Result<U256> {
    serde_json::from_value(
        value
            .get(field)
            .cloned()
            .with_context(|| format!("missing {field}"))?,
    )
    .with_context(|| format!("decode U256 {field}"))
}
fn checked_add(left: u64, right: u64) -> Result<u64> {
    left.checked_add(right).context("campaign counter overflow")
}
fn sorted_windows(mut windows: Vec<CoverageWindow>) -> Vec<CoverageWindow> {
    windows.sort_by_key(|window| {
        (
            window.from_l2_block,
            window.to_l2_block,
            window.start_head_hash,
            window.cutoff_head_hash,
            window.manifest_content_keccak256,
            window.snapshot_content_keccak256,
        )
    });
    windows
}
fn percentile<T: Copy>(sorted: &[T], percent: usize) -> Option<T> {
    if sorted.is_empty() {
        return None;
    }
    let rank = sorted
        .len()
        .saturating_mul(percent)
        .div_ceil(100)
        .saturating_sub(1);
    sorted.get(rank).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hermes_feed::launchpad_readiness::LaunchpadReadinessWindow;
    use hermes_feed::launchpad_session::{SessionBoundary, SessionExecutables};

    fn session(from: u64, to: u64) -> ValidatedPaperSession {
        ValidatedPaperSession {
            windows: vec![LaunchpadReadinessWindow {
                record_type: "launchpad_paper_readiness_window".into(),
                launchpad: LaunchpadId::Bow,
                coverage_from_l2_block: from,
                coverage_to_l2_block: to,
                start_head_hash: B256::with_last_byte(1),
                cutoff_head_hash: B256::with_last_byte(2),
                complete: true,
                quote_eligible_confirmed_observations: 0,
                profile_envelope_observations: BTreeMap::new(),
                false_positives: 0,
                detector_misses: 0,
                identity_mismatches: 0,
                direction_mismatches: 0,
                prediction_mismatches: 0,
                quote_mismatches: 0,
                provenance: None,
            }],
            manifest_content_keccak256: B256::with_last_byte(3),
            observed_snapshot_content_keccak256: B256::with_last_byte(4),
            snapshot_boundary: SessionBoundary {
                l2_block_number: from.saturating_sub(1),
                l2_block_hash: B256::with_last_byte(5),
            },
            executables: SessionExecutables {
                feed_keccak256: B256::with_last_byte(1),
                paper_keccak256: B256::with_last_byte(2),
                reconciler_keccak256: B256::with_last_byte(3),
                chain_head_keccak256: B256::with_last_byte(4),
                readiness_keccak256: B256::with_last_byte(5),
            },
        }
    }

    #[test]
    fn nearest_rank_percentiles_are_deterministic() {
        let values = [10, 20, 30, 40, 50];
        assert_eq!(percentile(&values, 50), Some(30));
        assert_eq!(percentile(&values, 95), Some(50));
        assert_eq!(percentile::<u64>(&[], 99), None);
    }

    #[test]
    fn plan_aggregation_is_checked_and_preserves_slippage() {
        let plan = plan_sizing(
            "0x01".into(),
            Some("0x0000000000000000000000000000000000000001".into()),
            U256::from(100),
            U256::from(90),
            U256::from(81),
        )
        .unwrap();
        assert_eq!(plan.slippage_bps, U256::from(1_000));
        assert_eq!(
            plan.scope_token.as_deref(),
            Some("0x0000000000000000000000000000000000000001")
        );
        assert!(
            plan_sizing(
                "0x02".into(),
                None,
                U256::ZERO,
                U256::from(1),
                U256::from(2)
            )
            .is_err()
        );
    }

    #[test]
    fn campaign_windows_reject_overlap_independent_of_input_order() {
        assert!(validate_non_overlapping(&[session(20, 30), session(10, 19)]).is_ok());
        assert!(validate_non_overlapping(&[session(20, 30), session(10, 20)]).is_err());
    }

    #[derive(serde::Deserialize)]
    struct WindowFixture {
        coverage_from_l2_block: u64,
        coverage_to_l2_block: u64,
    }

    #[test]
    fn fixture_overlap_is_rejected_by_real_reporter_validator() {
        let first: WindowFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/launchpad-evidence-report/success-window-a.json"
        ))
        .unwrap();
        let overlap: WindowFixture = serde_json::from_str(include_str!(
            "../../tests/fixtures/launchpad-evidence-report/overlap-window.json"
        ))
        .unwrap();
        assert!(
            validate_non_overlapping(&[
                session(first.coverage_from_l2_block, first.coverage_to_l2_block),
                session(overlap.coverage_from_l2_block, overlap.coverage_to_l2_block),
            ])
            .is_err()
        );
    }

    #[test]
    fn campaign_lock_validation_is_bound_to_the_hashed_byte_buffer() {
        let executables = session(10, 20).executables;
        let pins = B256::with_last_byte(6);
        let report = B256::with_last_byte(7);
        let snapshot = B256::with_last_byte(8);
        let runner = B256::with_last_byte(9);
        let bytes = serde_json::to_vec(&json!({
            "record_type": "launchpad_paper_campaign_lock",
            "schema_version": 1,
            "expected_pins_content_keccak256": pins,
            "executables": executables,
            "orchestration": {
                "report_keccak256": report,
                "pin_snapshot_keccak256": snapshot,
                "local_runner_keccak256": runner,
            }
        }))
        .unwrap();
        assert!(
            validate_campaign_lock(&bytes, pins, &executables, report, snapshot, runner).is_ok()
        );
        let mut changed = bytes;
        changed[0] = b'[';
        assert!(
            validate_campaign_lock(&changed, pins, &executables, report, snapshot, runner).is_err()
        );
    }

    #[test]
    fn coverage_rows_are_canonically_sorted() {
        let make = |from| CoverageWindow {
            from_l2_block: from,
            to_l2_block: from + 5,
            start_head_hash: B256::with_last_byte(1),
            cutoff_head_hash: B256::with_last_byte(2),
            manifest_content_keccak256: B256::with_last_byte(3),
            snapshot_content_keccak256: B256::with_last_byte(4),
        };
        let rows = sorted_windows(vec![make(20), make(10)]);
        assert_eq!(rows[0].from_l2_block, 10);
        assert_eq!(rows[1].from_l2_block, 20);
    }
}
