//! Numeric, paper-evidence-only launchpad readiness evaluation.
//!
//! This module never enables signing, broadcasting, deployment, or canaries.
//! It aggregates independently completed reconciliation windows and emits a
//! deterministic statement about whether the paper evidence sample is large
//! and clean enough for a separate human-controlled promotion review.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use alloy_primitives::B256;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::evidence_provenance::{
    AggregatedReadinessProvenance, EvidenceAcquisition, LaunchpadReadinessProvenance,
};
use crate::launchpad_adapter::LaunchpadId;

pub const MIN_QUOTE_ELIGIBLE_CONFIRMED: u64 = 100;
pub const MIN_PROFILE_ENVELOPE_OBSERVATIONS: u64 = 10;
pub const MIN_INDEPENDENT_COMPLETE_WINDOWS: usize = 3;

pub const READINESS_LAUNCHPADS: [LaunchpadId; 7] = [
    LaunchpadId::Bow,
    LaunchpadId::LaunchHoodV3,
    LaunchpadId::Clanker,
    LaunchpadId::BankrDoppler,
    LaunchpadId::Pons,
    LaunchpadId::HoodFun,
    LaunchpadId::Flap,
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchpadReadinessWindow {
    pub record_type: String,
    pub launchpad: LaunchpadId,
    pub coverage_from_l2_block: u64,
    pub coverage_to_l2_block: u64,
    pub start_head_hash: B256,
    pub cutoff_head_hash: B256,
    pub complete: bool,
    pub quote_eligible_confirmed_observations: u64,
    pub profile_envelope_observations: BTreeMap<String, u64>,
    pub false_positives: u64,
    pub detector_misses: u64,
    pub identity_mismatches: u64,
    pub direction_mismatches: u64,
    pub prediction_mismatches: u64,
    pub quote_mismatches: u64,
    /// Old windows remain decodable for an explicit fail-closed error, but can
    /// never contribute to promotion readiness without exact provenance.
    #[serde(default)]
    pub provenance: Option<LaunchpadReadinessProvenance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LaunchpadReadinessPolicy {
    pub minimum_quote_eligible_confirmed_observations: u64,
    pub minimum_observations_per_supported_profile_envelope: u64,
    pub minimum_independent_complete_windows: usize,
    pub maximum_false_positives: u64,
    pub maximum_detector_misses: u64,
    pub maximum_identity_mismatches: u64,
    pub maximum_direction_mismatches: u64,
    pub maximum_prediction_mismatches: u64,
    pub maximum_quote_mismatches: u64,
}

impl LaunchpadReadinessPolicy {
    pub const fn conservative() -> Self {
        Self {
            minimum_quote_eligible_confirmed_observations: MIN_QUOTE_ELIGIBLE_CONFIRMED,
            minimum_observations_per_supported_profile_envelope: MIN_PROFILE_ENVELOPE_OBSERVATIONS,
            minimum_independent_complete_windows: MIN_INDEPENDENT_COMPLETE_WINDOWS,
            maximum_false_positives: 0,
            maximum_detector_misses: 0,
            maximum_identity_mismatches: 0,
            maximum_direction_mismatches: 0,
            maximum_prediction_mismatches: 0,
            maximum_quote_mismatches: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileEnvelopeReadiness {
    pub profile_envelope: &'static str,
    pub observations: u64,
    pub required: u64,
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaunchpadReadinessTotals {
    pub submitted_windows: usize,
    pub complete_windows: usize,
    pub independent_complete_windows: usize,
    pub quote_eligible_confirmed_observations: u64,
    pub false_positives: u64,
    pub detector_misses: u64,
    pub identity_mismatches: u64,
    pub direction_mismatches: u64,
    pub prediction_mismatches: u64,
    pub quote_mismatches: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaunchpadReadinessFailure {
    pub code: &'static str,
    pub actual: u64,
    pub required: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_envelope: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaunchpadReadinessRecord {
    pub record_type: &'static str,
    pub launchpad: LaunchpadId,
    pub paper_evidence_ready: bool,
    pub authorizes_canary: bool,
    pub execution_eligible: bool,
    pub input_trust: ReadinessInputTrust,
    pub policy: LaunchpadReadinessPolicy,
    pub totals: LaunchpadReadinessTotals,
    pub supported_profile_envelopes: Vec<ProfileEnvelopeReadiness>,
    pub failures: Vec<LaunchpadReadinessFailure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<AggregatedReadinessProvenance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessInputTrust {
    UntrustedInput,
    CompletedSessionManifest,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LaunchpadReadinessError {
    #[error("unknown readiness record type {0}")]
    UnknownRecordType(String),
    #[error("readiness input includes unsupported launchpad {0:?}")]
    UnsupportedLaunchpad(LaunchpadId),
    #[error("readiness window has invalid canonical coverage identity")]
    InvalidWindowIdentity,
    #[error("readiness window contains unknown profile or envelope {0}")]
    UnknownProfileEnvelope(String),
    #[error("profile or envelope observation count exceeds quote-eligible confirmations")]
    InvalidProfileEnvelopeCount,
    #[error("readiness counter aggregation overflowed")]
    CounterOverflow,
    #[error("readiness window is missing or has invalid provenance")]
    InvalidProvenance,
    #[error("replay evidence is not eligible for promotion readiness")]
    ReplayNotPromotionEligible,
    #[error("readiness input mixes expected-pin or build provenance")]
    MixedPromotionProvenance,
}

pub fn supported_profile_envelopes(launchpad: LaunchpadId) -> Option<&'static [&'static str]> {
    match launchpad {
        LaunchpadId::Bow => Some(&["zero_initial_buy", "payable_initial_buy"]),
        LaunchpadId::LaunchHoodV3 => Some(&["embedded_initial_buy"]),
        LaunchpadId::Clanker => Some(&[
            "extensionless_single_position",
            "pinned_extension_five_position",
        ]),
        LaunchpadId::BankrDoppler => Some(&[
            "curve_ticks_v1",
            "curve_ticks_v2",
            "curve_ticks_v3",
            "curve_ticks_v4",
            "curve_ticks_v5",
            "direct_airlock",
            "erc7579",
        ]),
        LaunchpadId::Pons => Some(&["current_generation"]),
        LaunchpadId::HoodFun => Some(&["current_curve", "migrated_v3_boundary"]),
        LaunchpadId::Flap => Some(&["discovery_only"]),
        _ => None,
    }
}

pub fn evaluate_launchpad_readiness(
    windows: &[LaunchpadReadinessWindow],
) -> Result<Vec<LaunchpadReadinessRecord>, LaunchpadReadinessError> {
    evaluate_with_trust(windows, ReadinessInputTrust::UntrustedInput, &[])
}

pub fn evaluate_completed_session_readiness(
    windows: &[LaunchpadReadinessWindow],
    session_manifest_content_keccak256: &[B256],
    feed_binary_keccak256: B256,
    chain_head_binary_keccak256: B256,
    readiness_binary_keccak256: B256,
) -> Result<Vec<LaunchpadReadinessRecord>, LaunchpadReadinessError> {
    if windows.is_empty()
        || session_manifest_content_keccak256.is_empty()
        || session_manifest_content_keccak256.contains(&B256::ZERO)
        || feed_binary_keccak256 == B256::ZERO
        || chain_head_binary_keccak256 == B256::ZERO
        || readiness_binary_keccak256 == B256::ZERO
    {
        return Err(LaunchpadReadinessError::InvalidProvenance);
    }
    let mut records = evaluate_with_trust(
        windows,
        ReadinessInputTrust::CompletedSessionManifest,
        session_manifest_content_keccak256,
    )?;
    for record in &mut records {
        if let Some(provenance) = record.provenance.as_mut() {
            provenance.feed_binary_keccak256 = Some(feed_binary_keccak256);
            provenance.chain_head_binary_keccak256 = Some(chain_head_binary_keccak256);
            provenance.readiness_binary_keccak256 = Some(readiness_binary_keccak256);
        }
    }
    Ok(records)
}

fn evaluate_with_trust(
    windows: &[LaunchpadReadinessWindow],
    input_trust: ReadinessInputTrust,
    session_manifest_content_keccak256: &[B256],
) -> Result<Vec<LaunchpadReadinessRecord>, LaunchpadReadinessError> {
    let mut aggregate_provenance = validate_promotion_provenance(windows)?;
    if let Some(provenance) = aggregate_provenance.as_mut() {
        provenance.session_manifest_content_keccak256 = session_manifest_content_keccak256.to_vec();
        provenance
            .session_manifest_content_keccak256
            .sort_unstable();
        provenance.session_manifest_content_keccak256.dedup();
    }
    let mut indexed: HashMap<LaunchpadId, Vec<&LaunchpadReadinessWindow>> = HashMap::new();
    for window in windows {
        if window.record_type != "launchpad_paper_readiness_window" {
            return Err(LaunchpadReadinessError::UnknownRecordType(
                window.record_type.clone(),
            ));
        }
        if supported_profile_envelopes(window.launchpad).is_none() {
            return Err(LaunchpadReadinessError::UnsupportedLaunchpad(
                window.launchpad,
            ));
        }
        if window.coverage_from_l2_block > window.coverage_to_l2_block
            || window.start_head_hash == B256::ZERO
            || window.cutoff_head_hash == B256::ZERO
        {
            return Err(LaunchpadReadinessError::InvalidWindowIdentity);
        }
        let supported = supported_profile_envelopes(window.launchpad).expect("checked above");
        for (profile, count) in &window.profile_envelope_observations {
            if !supported.contains(&profile.as_str()) {
                return Err(LaunchpadReadinessError::UnknownProfileEnvelope(
                    profile.clone(),
                ));
            }
            if *count > window.quote_eligible_confirmed_observations {
                return Err(LaunchpadReadinessError::InvalidProfileEnvelopeCount);
            }
        }
        indexed.entry(window.launchpad).or_default().push(window);
    }

    READINESS_LAUNCHPADS
        .into_iter()
        .map(|launchpad| {
            evaluate_one(
                launchpad,
                indexed.remove(&launchpad).unwrap_or_default(),
                aggregate_provenance.clone(),
                input_trust,
            )
        })
        .collect()
}

fn validate_promotion_provenance(
    windows: &[LaunchpadReadinessWindow],
) -> Result<Option<AggregatedReadinessProvenance>, LaunchpadReadinessError> {
    let Some(first) = windows.first() else {
        return Ok(None);
    };
    let first = first
        .provenance
        .as_ref()
        .ok_or(LaunchpadReadinessError::InvalidProvenance)?;
    first
        .validate()
        .map_err(|_| LaunchpadReadinessError::InvalidProvenance)?;
    if first.acquisition != EvidenceAcquisition::Live {
        return Err(LaunchpadReadinessError::ReplayNotPromotionEligible);
    }
    let mut observed_snapshots = BTreeSet::new();
    for window in windows {
        let provenance = window
            .provenance
            .as_ref()
            .ok_or(LaunchpadReadinessError::InvalidProvenance)?;
        provenance
            .validate()
            .map_err(|_| LaunchpadReadinessError::InvalidProvenance)?;
        if provenance.acquisition != EvidenceAcquisition::Live {
            return Err(LaunchpadReadinessError::ReplayNotPromotionEligible);
        }
        // Fresh startup snapshots and per-window output files intentionally
        // differ. The reviewed expected pins and exact executable tuple are
        // the cross-window compatibility key.
        if provenance.expected_pins_content_keccak256 != first.expected_pins_content_keccak256
            || provenance.observer_paper_binary_keccak256 != first.observer_paper_binary_keccak256
            || provenance.reconciler_binary_keccak256 != first.reconciler_binary_keccak256
            || provenance.finalizer_paper_binary_keccak256 != first.finalizer_paper_binary_keccak256
        {
            return Err(LaunchpadReadinessError::MixedPromotionProvenance);
        }
        observed_snapshots.insert(provenance.observed_snapshot_content_keccak256);
    }
    Ok(Some(AggregatedReadinessProvenance {
        schema_version: first.schema_version,
        acquisition: first.acquisition,
        expected_pins_content_keccak256: first.expected_pins_content_keccak256,
        observer_paper_binary_keccak256: first.observer_paper_binary_keccak256,
        reconciler_binary_keccak256: first.reconciler_binary_keccak256,
        finalizer_paper_binary_keccak256: first.finalizer_paper_binary_keccak256,
        feed_binary_keccak256: None,
        chain_head_binary_keccak256: None,
        readiness_binary_keccak256: None,
        observed_snapshot_content_keccak256: observed_snapshots.into_iter().collect(),
        session_manifest_content_keccak256: Vec::new(),
    }))
}

fn evaluate_one(
    launchpad: LaunchpadId,
    windows: Vec<&LaunchpadReadinessWindow>,
    provenance: Option<AggregatedReadinessProvenance>,
    input_trust: ReadinessInputTrust,
) -> Result<LaunchpadReadinessRecord, LaunchpadReadinessError> {
    let policy = LaunchpadReadinessPolicy::conservative();
    let complete_windows = windows.iter().filter(|window| window.complete).count();
    let independent_windows = independent_windows(&windows);
    let independent_complete_windows = independent_windows.len();
    let mut quote_eligible = 0_u64;
    let mut false_positives = 0_u64;
    let mut detector_misses = 0_u64;
    let mut identity_mismatches = 0_u64;
    let mut direction_mismatches = 0_u64;
    let mut prediction_mismatches = 0_u64;
    let mut quote_mismatches = 0_u64;
    let mut profile_counts = BTreeMap::<&'static str, u64>::new();
    for profile in supported_profile_envelopes(launchpad).expect("supported launchpad") {
        profile_counts.insert(profile, 0);
    }
    for window in &independent_windows {
        quote_eligible = checked_add(quote_eligible, window.quote_eligible_confirmed_observations)?;
        for (profile, count) in &window.profile_envelope_observations {
            if let Some(total) = profile_counts.get_mut(profile.as_str()) {
                *total = checked_add(*total, *count)?;
            }
        }
    }
    for window in &windows {
        false_positives = checked_add(false_positives, window.false_positives)?;
        detector_misses = checked_add(detector_misses, window.detector_misses)?;
        identity_mismatches = checked_add(identity_mismatches, window.identity_mismatches)?;
        direction_mismatches = checked_add(direction_mismatches, window.direction_mismatches)?;
        prediction_mismatches = checked_add(prediction_mismatches, window.prediction_mismatches)?;
        quote_mismatches = checked_add(quote_mismatches, window.quote_mismatches)?;
    }

    let profile_readiness = profile_counts
        .into_iter()
        .map(
            |(profile_envelope, observations)| ProfileEnvelopeReadiness {
                profile_envelope,
                observations,
                required: policy.minimum_observations_per_supported_profile_envelope,
                ready: observations >= policy.minimum_observations_per_supported_profile_envelope,
            },
        )
        .collect::<Vec<_>>();
    let totals = LaunchpadReadinessTotals {
        submitted_windows: windows.len(),
        complete_windows,
        independent_complete_windows,
        quote_eligible_confirmed_observations: quote_eligible,
        false_positives,
        detector_misses,
        identity_mismatches,
        direction_mismatches,
        prediction_mismatches,
        quote_mismatches,
    };
    let mut failures = Vec::new();
    push_minimum_failure(
        &mut failures,
        "insufficient_quote_eligible_confirmed_observations",
        quote_eligible,
        policy.minimum_quote_eligible_confirmed_observations,
        None,
    );
    push_minimum_failure(
        &mut failures,
        "insufficient_independent_complete_windows",
        independent_complete_windows as u64,
        policy.minimum_independent_complete_windows as u64,
        None,
    );
    for profile in &profile_readiness {
        push_minimum_failure(
            &mut failures,
            "insufficient_profile_envelope_observations",
            profile.observations,
            profile.required,
            Some(profile.profile_envelope),
        );
    }
    push_zero_failure(&mut failures, "false_positives_present", false_positives);
    push_zero_failure(&mut failures, "detector_misses_present", detector_misses);
    push_zero_failure(
        &mut failures,
        "identity_mismatches_present",
        identity_mismatches,
    );
    push_zero_failure(
        &mut failures,
        "direction_mismatches_present",
        direction_mismatches,
    );
    push_zero_failure(
        &mut failures,
        "prediction_mismatches_present",
        prediction_mismatches,
    );
    push_zero_failure(&mut failures, "quote_mismatches_present", quote_mismatches);
    if launchpad == LaunchpadId::Flap {
        push_zero_failure(&mut failures, "discovery_only_launchpad", 1);
    }
    if input_trust == ReadinessInputTrust::UntrustedInput {
        push_zero_failure(&mut failures, "untrusted_readiness_input", 1);
    }

    Ok(LaunchpadReadinessRecord {
        record_type: "launchpad_paper_readiness",
        launchpad,
        paper_evidence_ready: failures.is_empty(),
        authorizes_canary: false,
        execution_eligible: false,
        input_trust,
        policy,
        totals,
        supported_profile_envelopes: profile_readiness,
        failures,
        provenance,
    })
}

fn independent_windows<'a>(
    windows: &[&'a LaunchpadReadinessWindow],
) -> Vec<&'a LaunchpadReadinessWindow> {
    let mut complete = windows
        .iter()
        .filter(|window| window.complete)
        .copied()
        .collect::<Vec<_>>();
    complete.sort_by_key(|window| {
        (
            window.coverage_to_l2_block,
            window.coverage_from_l2_block,
            window.start_head_hash,
            window.cutoff_head_hash,
        )
    });
    let mut independent = Vec::new();
    let mut last_end = None;
    for window in complete {
        if last_end.is_none_or(|end| window.coverage_from_l2_block > end) {
            independent.push(window);
            last_end = Some(window.coverage_to_l2_block);
        }
    }
    independent
}

fn checked_add(left: u64, right: u64) -> Result<u64, LaunchpadReadinessError> {
    left.checked_add(right)
        .ok_or(LaunchpadReadinessError::CounterOverflow)
}

fn push_minimum_failure(
    failures: &mut Vec<LaunchpadReadinessFailure>,
    code: &'static str,
    actual: u64,
    required: u64,
    profile_envelope: Option<&'static str>,
) {
    if actual < required {
        failures.push(LaunchpadReadinessFailure {
            code,
            actual,
            required,
            profile_envelope,
        });
    }
}

fn push_zero_failure(
    failures: &mut Vec<LaunchpadReadinessFailure>,
    code: &'static str,
    actual: u64,
) {
    if actual != 0 {
        failures.push(LaunchpadReadinessFailure {
            code,
            actual,
            required: 0,
            profile_envelope: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence_provenance::EVIDENCE_PROVENANCE_SCHEMA_VERSION;

    fn provenance(index: u8) -> LaunchpadReadinessProvenance {
        LaunchpadReadinessProvenance {
            schema_version: EVIDENCE_PROVENANCE_SCHEMA_VERSION,
            acquisition: EvidenceAcquisition::Live,
            expected_pins_content_keccak256: B256::with_last_byte(1),
            observed_snapshot_content_keccak256: B256::with_last_byte(10 + index),
            observed_snapshot_l2_block_number: 900 + u64::from(index),
            observed_snapshot_l2_block_hash: B256::with_last_byte(40 + index),
            observer_paper_binary_keccak256: B256::with_last_byte(2),
            reconciler_binary_keccak256: B256::with_last_byte(3),
            finalizer_paper_binary_keccak256: B256::with_last_byte(2),
            observer_output_content_keccak256: B256::with_last_byte(20 + index),
            reconciliation_output_content_keccak256: B256::with_last_byte(30 + index),
        }
    }

    fn window(launchpad: LaunchpadId, index: u8, quote_eligible: u64) -> LaunchpadReadinessWindow {
        let from = 1_000 + u64::from(index) * 100;
        LaunchpadReadinessWindow {
            record_type: "launchpad_paper_readiness_window".into(),
            launchpad,
            coverage_from_l2_block: from,
            coverage_to_l2_block: from + 99,
            start_head_hash: B256::with_last_byte(index.saturating_mul(2).saturating_add(1)),
            cutoff_head_hash: B256::with_last_byte(index.saturating_mul(2).saturating_add(2)),
            complete: true,
            quote_eligible_confirmed_observations: quote_eligible,
            profile_envelope_observations: supported_profile_envelopes(launchpad)
                .unwrap()
                .iter()
                .map(|profile| ((*profile).into(), 4))
                .collect(),
            false_positives: 0,
            detector_misses: 0,
            identity_mismatches: 0,
            direction_mismatches: 0,
            prediction_mismatches: 0,
            quote_mismatches: 0,
            provenance: Some(provenance(index)),
        }
    }

    fn record(
        records: &[LaunchpadReadinessRecord],
        launchpad: LaunchpadId,
    ) -> &LaunchpadReadinessRecord {
        records
            .iter()
            .find(|record| record.launchpad == launchpad)
            .unwrap()
    }

    #[test]
    fn bankr_curve_versions_and_envelopes_are_orthogonal_readiness_strata() {
        assert_eq!(
            supported_profile_envelopes(LaunchpadId::BankrDoppler),
            Some(
                [
                    "curve_ticks_v1",
                    "curve_ticks_v2",
                    "curve_ticks_v3",
                    "curve_ticks_v4",
                    "curve_ticks_v5",
                    "direct_airlock",
                    "erc7579",
                ]
                .as_slice()
            )
        );

        let mut evidence = window(LaunchpadId::BankrDoppler, 0, 22);
        evidence.profile_envelope_observations = BTreeMap::from([
            ("curve_ticks_v4".into(), 16),
            ("curve_ticks_v5".into(), 6),
            ("direct_airlock".into(), 1),
            ("erc7579".into(), 21),
        ]);
        let records = evaluate_launchpad_readiness(std::slice::from_ref(&evidence)).unwrap();
        let bankr = record(&records, LaunchpadId::BankrDoppler);
        assert!(!bankr.authorizes_canary);
        assert!(!bankr.execution_eligible);
        assert_eq!(evidence.profile_envelope_observations["curve_ticks_v4"], 16);
        assert_eq!(evidence.profile_envelope_observations["curve_ticks_v5"], 6);
        assert_eq!(evidence.profile_envelope_observations["direct_airlock"], 1);
        assert_eq!(evidence.profile_envelope_observations["erc7579"], 21);
    }

    #[test]
    fn exact_conservative_thresholds_become_ready_but_never_authorize_canary() {
        let windows = [
            window(LaunchpadId::BankrDoppler, 0, 34),
            window(LaunchpadId::BankrDoppler, 1, 33),
            window(LaunchpadId::BankrDoppler, 2, 33),
        ];
        let records = evaluate_completed_session_readiness(
            &windows,
            &[B256::with_last_byte(90), B256::with_last_byte(91)],
            B256::with_last_byte(92),
            B256::with_last_byte(93),
            B256::with_last_byte(94),
        )
        .unwrap();
        let bankr = record(&records, LaunchpadId::BankrDoppler);
        assert!(bankr.paper_evidence_ready);
        assert!(!bankr.authorizes_canary);
        assert!(!bankr.execution_eligible);
        assert_eq!(
            bankr.input_trust,
            ReadinessInputTrust::CompletedSessionManifest
        );
        let provenance = bankr.provenance.as_ref().unwrap();
        assert_eq!(
            provenance.feed_binary_keccak256,
            Some(B256::with_last_byte(92))
        );
        assert_eq!(
            provenance.chain_head_binary_keccak256,
            Some(B256::with_last_byte(93))
        );
        assert_eq!(
            provenance.readiness_binary_keccak256,
            Some(B256::with_last_byte(94))
        );
        assert_eq!(bankr.totals.quote_eligible_confirmed_observations, 100);
        assert_eq!(bankr.totals.independent_complete_windows, 3);
        assert!(
            bankr
                .supported_profile_envelopes
                .iter()
                .all(|profile| profile.observations == 12 && profile.ready)
        );
    }

    #[test]
    fn free_standing_threshold_rows_are_explicitly_untrusted_and_never_ready() {
        let windows = [
            window(LaunchpadId::BankrDoppler, 0, 34),
            window(LaunchpadId::BankrDoppler, 1, 33),
            window(LaunchpadId::BankrDoppler, 2, 33),
        ];
        let records = evaluate_launchpad_readiness(&windows).unwrap();
        let bankr = record(&records, LaunchpadId::BankrDoppler);
        assert!(!bankr.paper_evidence_ready);
        assert_eq!(bankr.input_trust, ReadinessInputTrust::UntrustedInput);
        assert!(
            bankr
                .failures
                .iter()
                .any(|failure| failure.code == "untrusted_readiness_input")
        );
    }

    #[test]
    fn sparse_no_activity_and_missing_launchpads_are_explicitly_not_ready() {
        let records = evaluate_launchpad_readiness(&[]).unwrap();
        assert_eq!(records.len(), READINESS_LAUNCHPADS.len());
        assert!(records.iter().all(|record| !record.paper_evidence_ready));
        assert!(records.iter().all(|record| {
            record
                .failures
                .iter()
                .any(|failure| failure.code == "insufficient_quote_eligible_confirmed_observations")
        }));
    }

    #[test]
    fn observe_only_stonks_is_absent_from_promotion_readiness_set() {
        assert!(!READINESS_LAUNCHPADS.contains(&LaunchpadId::StonksV3));
    }

    #[test]
    fn flap_is_reported_but_remains_unconditionally_discovery_only() {
        assert!(READINESS_LAUNCHPADS.contains(&LaunchpadId::Flap));
        let mut windows = [
            window(LaunchpadId::Flap, 0, 100),
            window(LaunchpadId::Flap, 1, 100),
            window(LaunchpadId::Flap, 2, 100),
        ];
        for window in &mut windows {
            window
                .profile_envelope_observations
                .insert("discovery_only".into(), 100);
        }
        let records = evaluate_launchpad_readiness(&windows).unwrap();
        let flap = record(&records, LaunchpadId::Flap);
        assert!(!flap.paper_evidence_ready);
        assert!(!flap.authorizes_canary);
        assert!(!flap.execution_eligible);
        assert!(
            flap.failures
                .iter()
                .any(|failure| failure.code == "discovery_only_launchpad")
        );
    }

    #[test]
    fn sample_window_and_profile_thresholds_are_independent_fail_closed_gates() {
        let mut too_few_samples = [
            window(LaunchpadId::Clanker, 0, 33),
            window(LaunchpadId::Clanker, 1, 33),
            window(LaunchpadId::Clanker, 2, 33),
        ];
        for window in &mut too_few_samples {
            for count in window.profile_envelope_observations.values_mut() {
                *count = 10;
            }
        }
        let records = evaluate_launchpad_readiness(&too_few_samples).unwrap();
        assert!(!record(&records, LaunchpadId::Clanker).paper_evidence_ready);

        let mut too_few_profile = [
            window(LaunchpadId::Clanker, 0, 34),
            window(LaunchpadId::Clanker, 1, 33),
            window(LaunchpadId::Clanker, 2, 33),
        ];
        for window in &mut too_few_profile {
            window
                .profile_envelope_observations
                .insert("extensionless_single_position".into(), 3);
        }
        let records = evaluate_launchpad_readiness(&too_few_profile).unwrap();
        let clanker = record(&records, LaunchpadId::Clanker);
        assert!(!clanker.paper_evidence_ready);
        assert!(clanker.failures.iter().any(|failure| {
            failure.code == "insufficient_profile_envelope_observations"
                && failure.profile_envelope == Some("extensionless_single_position")
                && failure.actual == 9
        }));
    }

    #[test]
    fn overlapping_or_incomplete_windows_do_not_satisfy_independence() {
        let mut windows = [
            window(LaunchpadId::Pons, 0, 50),
            window(LaunchpadId::Pons, 1, 50),
            window(LaunchpadId::Pons, 2, 50),
        ];
        windows[1].coverage_from_l2_block = windows[0].coverage_from_l2_block;
        windows[1].coverage_to_l2_block = windows[0].coverage_to_l2_block;
        windows[2].complete = false;
        let records = evaluate_launchpad_readiness(&windows).unwrap();
        let pons = record(&records, LaunchpadId::Pons);
        assert_eq!(pons.totals.complete_windows, 2);
        assert_eq!(pons.totals.independent_complete_windows, 1);
        assert_eq!(pons.totals.quote_eligible_confirmed_observations, 50);
        assert!(!pons.paper_evidence_ready);
    }

    #[test]
    fn every_error_counter_independently_blocks_readiness() {
        for set_error in [
            |window: &mut LaunchpadReadinessWindow| window.false_positives = 1,
            |window: &mut LaunchpadReadinessWindow| window.detector_misses = 1,
            |window: &mut LaunchpadReadinessWindow| window.identity_mismatches = 1,
            |window: &mut LaunchpadReadinessWindow| window.direction_mismatches = 1,
            |window: &mut LaunchpadReadinessWindow| window.prediction_mismatches = 1,
            |window: &mut LaunchpadReadinessWindow| window.quote_mismatches = 1,
        ] {
            let mut windows = [
                window(LaunchpadId::HoodFun, 0, 34),
                window(LaunchpadId::HoodFun, 1, 33),
                window(LaunchpadId::HoodFun, 2, 33),
            ];
            set_error(&mut windows[0]);
            let records = evaluate_launchpad_readiness(&windows).unwrap();
            assert!(!record(&records, LaunchpadId::HoodFun).paper_evidence_ready);
        }
    }

    #[test]
    fn malformed_or_unsupported_windows_are_rejected() {
        let mut malformed = window(LaunchpadId::Bow, 0, 100);
        malformed.start_head_hash = B256::ZERO;
        assert_eq!(
            evaluate_launchpad_readiness(&[malformed]),
            Err(LaunchpadReadinessError::InvalidWindowIdentity)
        );

        let unsupported = window(LaunchpadId::Bow, 0, 100);
        let mut unsupported = LaunchpadReadinessWindow {
            launchpad: LaunchpadId::Flap,
            ..unsupported
        };
        unsupported.profile_envelope_observations.clear();
        assert_eq!(
            evaluate_launchpad_readiness(&[unsupported]),
            Err(LaunchpadReadinessError::UnsupportedLaunchpad(
                LaunchpadId::Flap
            ))
        );

        let mut unknown_profile = window(LaunchpadId::Bow, 0, 100);
        unknown_profile
            .profile_envelope_observations
            .insert("unreviewed_profile".into(), 1);
        assert_eq!(
            evaluate_launchpad_readiness(&[unknown_profile]),
            Err(LaunchpadReadinessError::UnknownProfileEnvelope(
                "unreviewed_profile".into()
            ))
        );
    }

    #[test]
    fn missing_invalid_or_replay_provenance_is_rejected() {
        let mut missing = window(LaunchpadId::Bow, 0, 100);
        missing.provenance = None;
        assert_eq!(
            evaluate_launchpad_readiness(&[missing]),
            Err(LaunchpadReadinessError::InvalidProvenance)
        );

        let mut invalid = window(LaunchpadId::Bow, 0, 100);
        invalid
            .provenance
            .as_mut()
            .unwrap()
            .observer_paper_binary_keccak256 = B256::ZERO;
        assert_eq!(
            evaluate_launchpad_readiness(&[invalid]),
            Err(LaunchpadReadinessError::InvalidProvenance)
        );

        let mut replay = window(LaunchpadId::Bow, 0, 100);
        replay.provenance.as_mut().unwrap().acquisition = EvidenceAcquisition::Replay;
        assert_eq!(
            evaluate_launchpad_readiness(&[replay]),
            Err(LaunchpadReadinessError::ReplayNotPromotionEligible)
        );
    }

    #[test]
    fn mixed_expected_pins_or_builds_are_rejected_but_fresh_snapshots_are_preserved() {
        let first = window(LaunchpadId::Bow, 0, 50);
        let mut second = window(LaunchpadId::Bow, 1, 50);
        let records = evaluate_launchpad_readiness(&[first.clone(), second.clone()]).unwrap();
        let aggregate = record(&records, LaunchpadId::Bow)
            .provenance
            .as_ref()
            .unwrap();
        assert_eq!(aggregate.observed_snapshot_content_keccak256.len(), 2);

        second
            .provenance
            .as_mut()
            .unwrap()
            .expected_pins_content_keccak256 = B256::with_last_byte(9);
        assert_eq!(
            evaluate_launchpad_readiness(&[first.clone(), second.clone()]),
            Err(LaunchpadReadinessError::MixedPromotionProvenance)
        );

        second.provenance = Some(provenance(1));
        second
            .provenance
            .as_mut()
            .unwrap()
            .reconciler_binary_keccak256 = B256::with_last_byte(9);
        assert_eq!(
            evaluate_launchpad_readiness(&[first, second]),
            Err(LaunchpadReadinessError::MixedPromotionProvenance)
        );
    }
}
