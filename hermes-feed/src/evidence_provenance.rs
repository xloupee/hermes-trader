//! Exact, read-only provenance for launchpad paper-evidence windows.
//!
//! Content and executable hashes bind evidence to the bytes that were actually
//! consumed. They are not signatures and never authorize execution.

use std::fs;
use std::path::Path;

use alloy_primitives::{B256, keccak256};
use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const EVIDENCE_PROVENANCE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum EvidenceAcquisition {
    Live,
    Replay,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObserverEvidenceProvenance {
    pub schema_version: u32,
    pub acquisition: EvidenceAcquisition,
    pub expected_pins_content_keccak256: B256,
    pub observed_snapshot_content_keccak256: B256,
    pub observed_snapshot_l2_block_number: u64,
    pub observed_snapshot_l2_block_hash: B256,
    pub observer_paper_binary_keccak256: B256,
}

impl ObserverEvidenceProvenance {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != EVIDENCE_PROVENANCE_SCHEMA_VERSION
            || self.expected_pins_content_keccak256 == B256::ZERO
            || self.observed_snapshot_content_keccak256 == B256::ZERO
            || self.observed_snapshot_l2_block_number == 0
            || self.observed_snapshot_l2_block_hash == B256::ZERO
            || self.observer_paper_binary_keccak256 == B256::ZERO
        {
            bail!("observer evidence provenance is incomplete or unsupported");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciliationEvidenceProvenance {
    pub record_type: String,
    pub observer: ObserverEvidenceProvenance,
    pub reconciler_binary_keccak256: B256,
    pub observer_output_content_keccak256: B256,
}

impl ReconciliationEvidenceProvenance {
    pub fn validate(&self) -> Result<()> {
        self.observer.validate()?;
        if self.record_type != "launchpad_reconciliation_provenance"
            || self.reconciler_binary_keccak256 == B256::ZERO
            || self.observer_output_content_keccak256 == B256::ZERO
        {
            bail!("reconciliation evidence provenance is incomplete or unsupported");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchpadReadinessProvenance {
    pub schema_version: u32,
    pub acquisition: EvidenceAcquisition,
    pub expected_pins_content_keccak256: B256,
    pub observed_snapshot_content_keccak256: B256,
    pub observed_snapshot_l2_block_number: u64,
    pub observed_snapshot_l2_block_hash: B256,
    pub observer_paper_binary_keccak256: B256,
    pub reconciler_binary_keccak256: B256,
    pub finalizer_paper_binary_keccak256: B256,
    pub observer_output_content_keccak256: B256,
    pub reconciliation_output_content_keccak256: B256,
}

impl LaunchpadReadinessProvenance {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != EVIDENCE_PROVENANCE_SCHEMA_VERSION
            || self.expected_pins_content_keccak256 == B256::ZERO
            || self.observed_snapshot_content_keccak256 == B256::ZERO
            || self.observed_snapshot_l2_block_number == 0
            || self.observed_snapshot_l2_block_hash == B256::ZERO
            || self.observer_paper_binary_keccak256 == B256::ZERO
            || self.reconciler_binary_keccak256 == B256::ZERO
            || self.finalizer_paper_binary_keccak256 == B256::ZERO
            || self.observer_output_content_keccak256 == B256::ZERO
            || self.reconciliation_output_content_keccak256 == B256::ZERO
        {
            bail!("readiness provenance is incomplete or unsupported");
        }
        if self.acquisition == EvidenceAcquisition::Live
            && self.observer_paper_binary_keccak256 != self.finalizer_paper_binary_keccak256
        {
            bail!("live evidence used different observer and finalizer paper binaries");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AggregatedReadinessProvenance {
    pub schema_version: u32,
    pub acquisition: EvidenceAcquisition,
    pub expected_pins_content_keccak256: B256,
    pub observer_paper_binary_keccak256: B256,
    pub reconciler_binary_keccak256: B256,
    pub finalizer_paper_binary_keccak256: B256,
    pub feed_binary_keccak256: Option<B256>,
    pub chain_head_binary_keccak256: Option<B256>,
    pub readiness_binary_keccak256: Option<B256>,
    pub observed_snapshot_content_keccak256: Vec<B256>,
    pub session_manifest_content_keccak256: Vec<B256>,
}

pub fn read_bytes_with_keccak(path: &Path, description: &str) -> Result<(Vec<u8>, B256)> {
    let bytes = fs::read(path).with_context(|| format!("read {description} {}", path.display()))?;
    if bytes.is_empty() {
        bail!("{description} {} is empty", path.display());
    }
    let digest = keccak256(&bytes);
    Ok((bytes, digest))
}

pub fn read_json_with_keccak<T: DeserializeOwned>(
    path: &Path,
    description: &str,
) -> Result<(T, B256)> {
    let (bytes, digest) = read_bytes_with_keccak(path, description)?;
    let decoded = serde_json::from_slice(&bytes)
        .with_context(|| format!("decode {description} {}", path.display()))?;
    Ok((decoded, digest))
}

pub fn current_executable_keccak256() -> Result<B256> {
    let executable = std::env::current_exe().context("resolve current executable")?;
    let (_, digest) = read_bytes_with_keccak(&executable, "current executable")?;
    Ok(digest)
}

pub fn verify_expected_self_keccak256(expected: B256) -> Result<B256> {
    if expected == B256::ZERO {
        bail!("expected self executable digest is zero");
    }
    let actual = current_executable_keccak256()?;
    if actual != expected {
        bail!("current executable digest does not match launcher preflight");
    }
    Ok(actual)
}

/// Handle the standalone launcher preflight before Clap requires normal-mode
/// arguments. Returns true after printing the exact current executable digest.
pub fn maybe_print_self_digest() -> Result<bool> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() == 1 && args[0] == "--print-self-digest" {
        println!("{}", current_executable_keccak256()?);
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_self_digest_rejects_zero_and_mismatch() {
        let actual = current_executable_keccak256().unwrap();
        assert_eq!(verify_expected_self_keccak256(actual).unwrap(), actual);
        assert!(verify_expected_self_keccak256(B256::ZERO).is_err());
        let mismatch = if actual == B256::with_last_byte(1) {
            B256::with_last_byte(2)
        } else {
            B256::with_last_byte(1)
        };
        assert!(verify_expected_self_keccak256(mismatch).is_err());
    }
}
