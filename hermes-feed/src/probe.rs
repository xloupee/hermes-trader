use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct SequenceObservation {
    pub first: Option<u64>,
    pub last: Option<u64>,
    pub gaps: u64,
    pub missing: u64,
    pub duplicates_or_reordered: u64,
}

impl SequenceObservation {
    pub fn is_contiguous(self) -> bool {
        self.gaps == 0 && self.missing == 0
    }
}

#[derive(Debug, Default)]
pub struct SequenceTracker {
    observation: SequenceObservation,
}

impl SequenceTracker {
    pub fn observe(&mut self, sequence: u64) -> SequenceObservation {
        if self.observation.first.is_none() {
            self.observation.first = Some(sequence);
        }
        if let Some(last) = self.observation.last {
            if sequence == last.saturating_add(1) {
                self.observation.last = Some(sequence);
            } else if sequence > last.saturating_add(1) {
                self.observation.gaps = self.observation.gaps.saturating_add(1);
                self.observation.missing = self
                    .observation
                    .missing
                    .saturating_add(sequence.saturating_sub(last).saturating_sub(1));
                self.observation.last = Some(sequence);
            } else {
                self.observation.duplicates_or_reordered =
                    self.observation.duplicates_or_reordered.saturating_add(1);
            }
        } else {
            self.observation.last = Some(sequence);
        }
        self.observation
    }

    pub fn current(&self) -> SequenceObservation {
        self.observation
    }
}

#[derive(Debug, Serialize)]
pub struct FrameReport {
    pub record_type: &'static str,
    pub source: String,
    pub received_mono_ns: u64,
    pub received_unix_ns: u128,
    pub warmup: bool,
    pub reconnects: u64,
    pub frame_bytes: usize,
    pub json_ns: u64,
    pub base64_ns: u64,
    pub l2_walk_ns: u64,
    pub envelope_decode_ns: u64,
    pub filter_ns: u64,
    pub feed_messages: usize,
    pub signed_transactions: usize,
    pub router_matches: usize,
    pub selector_matches: usize,
    pub recovered_signers: usize,
    pub candidates: usize,
    pub candidate_emission_enabled: bool,
    pub suppressed_candidates: usize,
    pub unsupported_l1_messages: usize,
    pub unsupported_l2_messages: usize,
    pub sequence_numbers: Vec<u64>,
    pub sequence: SequenceObservation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_gaps_and_reordered_messages() {
        let mut tracker = SequenceTracker::default();
        tracker.observe(10);
        tracker.observe(12);
        tracker.observe(12);
        let observation = tracker.observe(13);
        assert_eq!(observation.first, Some(10));
        assert_eq!(observation.last, Some(13));
        assert_eq!(observation.gaps, 1);
        assert_eq!(observation.missing, 1);
        assert_eq!(observation.duplicates_or_reordered, 1);
        assert!(!observation.is_contiguous());
    }

    #[test]
    fn contiguous_feed_is_candidate_eligible() {
        let mut tracker = SequenceTracker::default();
        tracker.observe(10);
        let observation = tracker.observe(11);
        assert!(observation.is_contiguous());
    }
}
