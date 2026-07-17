//! Completed local paper-session manifests and promotion-grade validation.
//!
//! This integrity layer detects accidental truncation, edits, stale snapshots,
//! and mixed artifacts. It is deliberately not authentication against a
//! malicious local writer who can replace both evidence and its manifest.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use alloy_primitives::B256;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::evidence_provenance::{
    EVIDENCE_PROVENANCE_SCHEMA_VERSION, EvidenceAcquisition, read_bytes_with_keccak,
};
use crate::launchpad_readiness::LaunchpadReadinessWindow;
use crate::paper_observer::PaperObservedStartupSnapshot;

pub const SESSION_MANIFEST_FILE: &str = "session-completion-manifest.json";
pub const MAX_SNAPSHOT_START_GAP_L2_BLOCKS: u64 = 500;
const ARTIFACT_FILES: [&str; 9] = [
    "raw-feed.jsonl",
    "launchpad-paper.jsonl",
    "probe-metrics.jsonl",
    "reconciliation-evidence.jsonl",
    "launchpad-paper-finalized.jsonl",
    "expected-pins.input.json",
    "observed-startup-snapshot.input.json",
    "start-anchor.txt",
    "cutoff-anchor.txt",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionExecutables {
    pub feed_keccak256: B256,
    pub paper_keccak256: B256,
    pub reconciler_keccak256: B256,
    pub chain_head_keccak256: B256,
    pub readiness_keccak256: B256,
}

impl SessionExecutables {
    fn validate(&self) -> Result<()> {
        if [
            self.feed_keccak256,
            self.paper_keccak256,
            self.reconciler_keccak256,
            self.chain_head_keccak256,
            self.readiness_keccak256,
        ]
        .contains(&B256::ZERO)
        {
            bail!("session executable provenance contains a zero digest");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionArtifact {
    pub content_keccak256: B256,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionBoundary {
    pub l2_block_number: u64,
    pub l2_block_hash: B256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCompletionManifest {
    pub record_type: String,
    pub schema_version: u32,
    pub completed: bool,
    pub acquisition: EvidenceAcquisition,
    pub snapshot_boundary: SessionBoundary,
    pub start_boundary: SessionBoundary,
    pub cutoff_boundary: SessionBoundary,
    pub maximum_snapshot_start_gap_l2_blocks: u64,
    pub executables: SessionExecutables,
    pub artifacts: BTreeMap<String, SessionArtifact>,
}

#[derive(Debug)]
pub struct ValidatedPaperSession {
    pub windows: Vec<LaunchpadReadinessWindow>,
    pub manifest_content_keccak256: B256,
    pub observed_snapshot_content_keccak256: B256,
}

pub fn ensure_distinct_observed_snapshots(digests: impl IntoIterator<Item = B256>) -> Result<()> {
    let mut seen = HashSet::new();
    for digest in digests {
        if digest == B256::ZERO || !seen.insert(digest) {
            bail!("independent live sessions reuse one observed startup snapshot digest");
        }
    }
    Ok(())
}

pub fn complete_session(directory: &Path, executables: SessionExecutables) -> Result<PathBuf> {
    executables.validate()?;
    let manifest_path = directory.join(SESSION_MANIFEST_FILE);
    let partial_path = directory.join(format!("{SESSION_MANIFEST_FILE}.partial"));
    if manifest_path.exists() || partial_path.exists() {
        bail!("refusing to overwrite session completion manifest");
    }
    let mut artifacts = BTreeMap::new();
    for name in ARTIFACT_FILES {
        let path = directory.join(name);
        validate_private_regular_file(&path)?;
        let (bytes, content_keccak256) = read_bytes_with_keccak(&path, name)?;
        artifacts.insert(
            name.to_owned(),
            SessionArtifact {
                content_keccak256,
                bytes: u64::try_from(bytes.len()).context("artifact length exceeds u64")?,
            },
        );
    }
    let (start_boundary, cutoff_boundary) = read_boundaries(directory)?;
    let snapshot_boundary = read_snapshot_boundary(directory)?;
    validate_snapshot_freshness(snapshot_boundary, start_boundary)?;
    validate_probe_metrics(&directory.join("probe-metrics.jsonl"))?;
    let windows = read_finalized_windows(&directory.join("launchpad-paper-finalized.jsonl"))?;
    validate_windows(
        &windows,
        snapshot_boundary,
        start_boundary,
        cutoff_boundary,
        &executables,
        &artifacts,
    )?;
    let manifest = SessionCompletionManifest {
        record_type: "launchpad_paper_session_completion".into(),
        schema_version: EVIDENCE_PROVENANCE_SCHEMA_VERSION,
        completed: true,
        acquisition: EvidenceAcquisition::Live,
        snapshot_boundary,
        start_boundary,
        cutoff_boundary,
        maximum_snapshot_start_gap_l2_blocks: MAX_SNAPSHOT_START_GAP_L2_BLOCKS,
        executables,
        artifacts,
    };
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&partial_path)
        .with_context(|| format!("create completion manifest {}", partial_path.display()))?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    serde_json::to_writer_pretty(&mut file, &manifest).context("encode completion manifest")?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.sync_all()?;
    fs::rename(&partial_path, &manifest_path).context("publish completion manifest")?;
    File::open(directory)?.sync_all()?;
    Ok(manifest_path)
}

pub fn validate_completed_session(directory: &Path) -> Result<ValidatedPaperSession> {
    let manifest_path = directory.join(SESSION_MANIFEST_FILE);
    validate_private_regular_file(&manifest_path)?;
    let (manifest_bytes, manifest_content_keccak256) =
        read_bytes_with_keccak(&manifest_path, "session completion manifest")?;
    let manifest: SessionCompletionManifest =
        serde_json::from_slice(&manifest_bytes).context("decode session completion manifest")?;
    if manifest.record_type != "launchpad_paper_session_completion"
        || manifest.schema_version != EVIDENCE_PROVENANCE_SCHEMA_VERSION
        || !manifest.completed
        || manifest.acquisition != EvidenceAcquisition::Live
        || manifest.maximum_snapshot_start_gap_l2_blocks != MAX_SNAPSHOT_START_GAP_L2_BLOCKS
    {
        bail!("session completion manifest is incomplete or unsupported");
    }
    manifest.executables.validate()?;
    if manifest.artifacts.len() != ARTIFACT_FILES.len() {
        bail!("session completion manifest has the wrong artifact set");
    }
    for name in ARTIFACT_FILES {
        if directory.join(format!("{name}.partial")).exists() {
            bail!("completed session retains a partial artifact");
        }
        let path = directory.join(name);
        validate_private_regular_file(&path)?;
        let (bytes, digest) = read_bytes_with_keccak(&path, name)?;
        let expected = manifest
            .artifacts
            .get(name)
            .with_context(|| format!("manifest omits {name}"))?;
        if digest != expected.content_keccak256
            || u64::try_from(bytes.len()).ok() != Some(expected.bytes)
        {
            bail!("session artifact {name} changed after completion");
        }
    }
    let (start_boundary, cutoff_boundary) = read_boundaries(directory)?;
    let snapshot_boundary = read_snapshot_boundary(directory)?;
    if start_boundary != manifest.start_boundary
        || cutoff_boundary != manifest.cutoff_boundary
        || snapshot_boundary != manifest.snapshot_boundary
    {
        bail!("session boundary artifacts disagree with completion manifest");
    }
    validate_snapshot_freshness(snapshot_boundary, start_boundary)?;
    validate_probe_metrics(&directory.join("probe-metrics.jsonl"))?;
    let windows = read_finalized_windows(&directory.join("launchpad-paper-finalized.jsonl"))?;
    validate_windows(
        &windows,
        snapshot_boundary,
        start_boundary,
        cutoff_boundary,
        &manifest.executables,
        &manifest.artifacts,
    )?;
    Ok(ValidatedPaperSession {
        windows,
        manifest_content_keccak256,
        observed_snapshot_content_keccak256: manifest
            .artifacts
            .get("observed-startup-snapshot.input.json")
            .expect("validated artifact set")
            .content_keccak256,
    })
}

fn validate_private_regular_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect session artifact {}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o077 != 0 {
        bail!(
            "session artifact {} is not a private regular file",
            path.display()
        );
    }
    Ok(())
}

fn read_anchor(path: &Path) -> Result<SessionBoundary> {
    let text = fs::read_to_string(path)?;
    let mut fields = text.split_whitespace();
    let l2_block_number = fields
        .next()
        .context("anchor has no block number")?
        .parse()?;
    let l2_block_hash = fields.next().context("anchor has no block hash")?.parse()?;
    if fields.next().is_some() || l2_block_number == 0 || l2_block_hash == B256::ZERO {
        bail!("anchor is malformed");
    }
    Ok(SessionBoundary {
        l2_block_number,
        l2_block_hash,
    })
}

fn read_boundaries(directory: &Path) -> Result<(SessionBoundary, SessionBoundary)> {
    let start = read_anchor(&directory.join("start-anchor.txt"))?;
    let cutoff = read_anchor(&directory.join("cutoff-anchor.txt"))?;
    if cutoff.l2_block_number <= start.l2_block_number {
        bail!("session cutoff does not follow start");
    }
    Ok((start, cutoff))
}

fn read_snapshot_boundary(directory: &Path) -> Result<SessionBoundary> {
    let snapshot: PaperObservedStartupSnapshot = serde_json::from_slice(&fs::read(
        directory.join("observed-startup-snapshot.input.json"),
    )?)?;
    let boundary = snapshot
        .observed_at
        .context("observed snapshot has no boundary")?;
    if boundary.l2_block_number == 0 || boundary.l2_block_hash == B256::ZERO {
        bail!("observed snapshot boundary is incomplete");
    }
    Ok(SessionBoundary {
        l2_block_number: boundary.l2_block_number,
        l2_block_hash: boundary.l2_block_hash,
    })
}

fn validate_snapshot_freshness(snapshot: SessionBoundary, start: SessionBoundary) -> Result<()> {
    if snapshot.l2_block_number > start.l2_block_number
        || start.l2_block_number - snapshot.l2_block_number > MAX_SNAPSHOT_START_GAP_L2_BLOCKS
    {
        bail!("observed startup snapshot is after start or too stale");
    }
    Ok(())
}

fn validate_probe_metrics(path: &Path) -> Result<()> {
    let input = BufReader::new(File::open(path)?);
    let mut states = Vec::new();
    for (index, line) in input.lines().enumerate() {
        let value: Value = serde_json::from_str(&line?)
            .with_context(|| format!("decode probe metrics line {}", index + 1))?;
        if let Some(state) = value.get("state").and_then(Value::as_str) {
            if matches!(state, "connect_error" | "read_error" | "disconnected") {
                bail!("probe metrics contain an error state");
            }
            states.push(state.to_owned());
        }
    }
    if states != ["connected", "coverage_closed"] {
        bail!("probe metrics do not contain exact connected then coverage_closed states");
    }
    Ok(())
}

fn read_finalized_windows(path: &Path) -> Result<Vec<LaunchpadReadinessWindow>> {
    let input = BufReader::new(File::open(path)?);
    let mut windows = Vec::new();
    for (index, line) in input.lines().enumerate() {
        let line = line?;
        let value: Value = serde_json::from_str(&line)
            .with_context(|| format!("decode finalized line {}", index + 1))?;
        if value.get("record_type").and_then(Value::as_str)
            == Some("launchpad_paper_readiness_window")
        {
            windows.push(serde_json::from_value(value)?);
        }
    }
    if windows.len() != 6 {
        bail!("finalized output must contain exactly six readiness rows");
    }
    Ok(windows)
}

fn validate_windows(
    windows: &[LaunchpadReadinessWindow],
    snapshot: SessionBoundary,
    start: SessionBoundary,
    cutoff: SessionBoundary,
    executables: &SessionExecutables,
    artifacts: &BTreeMap<String, SessionArtifact>,
) -> Result<()> {
    let expected_from = start
        .l2_block_number
        .checked_add(1)
        .context("start overflow")?;
    let expected_pins = artifacts["expected-pins.input.json"].content_keccak256;
    let observed_snapshot = artifacts["observed-startup-snapshot.input.json"].content_keccak256;
    let observer_output = artifacts["launchpad-paper.jsonl"].content_keccak256;
    let reconciliation_output = artifacts["reconciliation-evidence.jsonl"].content_keccak256;
    let mut launchpads = HashSet::new();
    for window in windows {
        let provenance = window
            .provenance
            .as_ref()
            .context("window has no provenance")?;
        provenance.validate()?;
        if !launchpads.insert(window.launchpad)
            || !window.complete
            || window.coverage_from_l2_block != expected_from
            || window.coverage_to_l2_block != cutoff.l2_block_number
            || window.start_head_hash != start.l2_block_hash
            || window.cutoff_head_hash != cutoff.l2_block_hash
            || provenance.acquisition != EvidenceAcquisition::Live
            || provenance.expected_pins_content_keccak256 != expected_pins
            || provenance.observed_snapshot_content_keccak256 != observed_snapshot
            || provenance.observed_snapshot_l2_block_number != snapshot.l2_block_number
            || provenance.observed_snapshot_l2_block_hash != snapshot.l2_block_hash
            || provenance.observer_output_content_keccak256 != observer_output
            || provenance.reconciliation_output_content_keccak256 != reconciliation_output
            || provenance.observer_paper_binary_keccak256 != executables.paper_keccak256
            || provenance.finalizer_paper_binary_keccak256 != executables.paper_keccak256
            || provenance.reconciler_binary_keccak256 != executables.reconciler_keccak256
        {
            bail!("readiness window disagrees with completed session evidence");
        }
    }
    if snapshot.l2_block_number > start.l2_block_number {
        bail!("snapshot boundary follows session start");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use alloy_primitives::keccak256;
    use tempfile::TempDir;

    use super::*;
    use crate::evidence_provenance::LaunchpadReadinessProvenance;
    use crate::launchpad_adapter::LaunchpadId;

    fn executables() -> SessionExecutables {
        SessionExecutables {
            feed_keccak256: B256::with_last_byte(1),
            paper_keccak256: B256::with_last_byte(2),
            reconciler_keccak256: B256::with_last_byte(3),
            chain_head_keccak256: B256::with_last_byte(4),
            readiness_keccak256: B256::with_last_byte(5),
        }
    }

    fn write_private(path: &Path, bytes: &[u8]) {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .unwrap();
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .unwrap();
        file.write_all(bytes).unwrap();
    }

    fn session(snapshot_block: u64) -> TempDir {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        write_private(&directory.path().join("raw-feed.jsonl"), b"{}\n");
        write_private(
            &directory.path().join("launchpad-paper.jsonl"),
            b"{\"record_type\":\"launchpad_paper_capabilities\"}\n",
        );
        write_private(
            &directory.path().join("probe-metrics.jsonl"),
            b"{\"state\":\"connected\"}\n{\"state\":\"coverage_closed\"}\n",
        );
        write_private(
            &directory.path().join("reconciliation-evidence.jsonl"),
            b"{\"record_type\":\"launchpad_reconciliation_provenance\"}\n",
        );
        write_private(&directory.path().join("expected-pins.input.json"), b"{}\n");
        let snapshot_hash = B256::with_last_byte(8);
        let snapshot = serde_json::json!({
            "schema_version": 4,
            "document_role": "observed_startup_snapshot",
            "provenance": "startup_observation",
            "fixture_id": null,
            "chain_id": 4663,
            "observed_at": {
                "l2_block_number": snapshot_block,
                "l2_block_hash": snapshot_hash,
                "l1_block_number": 25_000_000,
                "block_timestamp": 1_780_000_000
            },
            "pins": []
        });
        write_private(
            &directory
                .path()
                .join("observed-startup-snapshot.input.json"),
            format!("{snapshot}\n").as_bytes(),
        );
        let start_hash = B256::with_last_byte(10);
        let cutoff_hash = B256::with_last_byte(11);
        write_private(
            &directory.path().join("start-anchor.txt"),
            format!("1000 {start_hash}\n").as_bytes(),
        );
        write_private(
            &directory.path().join("cutoff-anchor.txt"),
            format!("1100 {cutoff_hash}\n").as_bytes(),
        );
        let expected_digest = keccak256(b"{}\n");
        let observed_bytes = fs::read(
            directory
                .path()
                .join("observed-startup-snapshot.input.json"),
        )
        .unwrap();
        let observer_digest =
            keccak256(fs::read(directory.path().join("launchpad-paper.jsonl")).unwrap());
        let reconciliation_digest =
            keccak256(fs::read(directory.path().join("reconciliation-evidence.jsonl")).unwrap());
        let provenance = LaunchpadReadinessProvenance {
            schema_version: EVIDENCE_PROVENANCE_SCHEMA_VERSION,
            acquisition: EvidenceAcquisition::Live,
            expected_pins_content_keccak256: expected_digest,
            observed_snapshot_content_keccak256: keccak256(observed_bytes),
            observed_snapshot_l2_block_number: snapshot_block,
            observed_snapshot_l2_block_hash: snapshot_hash,
            observer_paper_binary_keccak256: executables().paper_keccak256,
            reconciler_binary_keccak256: executables().reconciler_keccak256,
            finalizer_paper_binary_keccak256: executables().paper_keccak256,
            observer_output_content_keccak256: observer_digest,
            reconciliation_output_content_keccak256: reconciliation_digest,
        };
        let launchpads = [
            LaunchpadId::Bow,
            LaunchpadId::LaunchHoodV3,
            LaunchpadId::Clanker,
            LaunchpadId::BankrDoppler,
            LaunchpadId::Pons,
            LaunchpadId::HoodFun,
        ];
        let mut finalized = Vec::new();
        for launchpad in launchpads {
            let window = LaunchpadReadinessWindow {
                record_type: "launchpad_paper_readiness_window".into(),
                launchpad,
                coverage_from_l2_block: 1001,
                coverage_to_l2_block: 1100,
                start_head_hash: start_hash,
                cutoff_head_hash: cutoff_hash,
                complete: true,
                quote_eligible_confirmed_observations: 0,
                profile_envelope_observations: BTreeMap::new(),
                false_positives: 0,
                detector_misses: 0,
                identity_mismatches: 0,
                direction_mismatches: 0,
                prediction_mismatches: 0,
                quote_mismatches: 0,
                provenance: Some(provenance.clone()),
            };
            serde_json::to_writer(&mut finalized, &window).unwrap();
            finalized.push(b'\n');
        }
        write_private(
            &directory.path().join("launchpad-paper-finalized.jsonl"),
            &finalized,
        );
        directory
    }

    #[test]
    fn completed_manifest_binds_every_canonical_artifact_and_window() {
        let directory = session(900);
        complete_session(directory.path(), executables()).unwrap();
        let validated = validate_completed_session(directory.path()).unwrap();
        assert_eq!(validated.windows.len(), 6);
        assert_ne!(validated.manifest_content_keccak256, B256::ZERO);
    }

    #[test]
    fn edited_counts_or_coverage_after_completion_fail_the_finalized_artifact_hash() {
        for (field, value) in [
            ("quote_eligible_confirmed_observations", Value::from(100)),
            ("coverage_end_l2_block", Value::from(999)),
        ] {
            let directory = session(900);
            complete_session(directory.path(), executables()).unwrap();
            let finalized_path = directory.path().join("launchpad-paper-finalized.jsonl");
            let mut rows = fs::read_to_string(&finalized_path)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>();
            rows[0][field] = value;
            let bytes = rows
                .iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>();
            fs::write(&finalized_path, bytes).unwrap();
            assert!(
                validate_completed_session(directory.path())
                    .unwrap_err()
                    .to_string()
                    .contains("changed after completion")
            );
        }
    }

    #[test]
    fn missing_manifest_partial_finalizer_probe_error_and_stale_snapshot_fail_closed() {
        let no_manifest = session(900);
        assert!(validate_completed_session(no_manifest.path()).is_err());

        let partial = session(900);
        fs::rename(
            partial.path().join("launchpad-paper-finalized.jsonl"),
            partial
                .path()
                .join("launchpad-paper-finalized.jsonl.partial"),
        )
        .unwrap();
        assert!(complete_session(partial.path(), executables()).is_err());
        assert!(!partial.path().join(SESSION_MANIFEST_FILE).exists());

        let probe_error = session(900);
        fs::write(
            probe_error.path().join("probe-metrics.jsonl"),
            b"{\"state\":\"connected\"}\n{\"state\":\"read_error\"}\n{\"state\":\"coverage_closed\"}\n",
        )
        .unwrap();
        assert!(complete_session(probe_error.path(), executables()).is_err());

        let stale = session(1);
        assert!(
            complete_session(stale.path(), executables())
                .unwrap_err()
                .to_string()
                .contains("too stale")
        );
    }

    #[test]
    fn independent_sessions_cannot_reuse_one_snapshot_digest() {
        let digest = B256::with_last_byte(1);
        ensure_distinct_observed_snapshots([digest, B256::with_last_byte(2)]).unwrap();
        assert!(ensure_distinct_observed_snapshots([digest, digest]).is_err());
    }
}
