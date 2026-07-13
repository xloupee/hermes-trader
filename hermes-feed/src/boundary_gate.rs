use serde::Serialize;
use thiserror::Error;

use crate::sequencer::ConditionalOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedBoundary {
    pub l1_block_number: u64,
    pub l1_timestamp: u64,
    pub sequence_contiguous: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum BoundaryDecision {
    Waiting {
        l1_block_number: u64,
        l1_timestamp: u64,
    },
    SubmitNow {
        l1_block_number: u64,
        l1_timestamp: u64,
    },
    AlreadyTriggered {
        l1_block_number: u64,
        l1_timestamp: u64,
    },
    Expired {
        l1_block_number: u64,
        l1_timestamp: u64,
    },
    FailedClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateState {
    Waiting,
    Triggered(FeedBoundary),
    Expired(FeedBoundary),
    FailedClosed,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryGateError {
    #[error("conditional block window is inverted")]
    InvertedBlockWindow,
    #[error("conditional timestamp window is inverted")]
    InvertedTimestampWindow,
}

/// Converts contiguous Nitro feed headers into one submission edge.
///
/// The gate performs no polling and owns no network client. Once it emits
/// `SubmitNow`, repeated messages cannot cause the transaction to be sent a
/// second time. Feed gaps and header regressions permanently fail the gate
/// closed.
#[derive(Debug, Clone)]
pub struct BoundaryGate {
    conditions: ConditionalOptions,
    state: GateState,
    last_boundary: Option<FeedBoundary>,
}

impl BoundaryGate {
    pub fn new(conditions: ConditionalOptions) -> Result<Self, BoundaryGateError> {
        if conditions.block_number_min > conditions.block_number_max {
            return Err(BoundaryGateError::InvertedBlockWindow);
        }
        if matches!(
            (conditions.timestamp_min, conditions.timestamp_max),
            (Some(min), Some(max)) if min > max
        ) {
            return Err(BoundaryGateError::InvertedTimestampWindow);
        }
        Ok(Self {
            conditions,
            state: GateState::Waiting,
            last_boundary: None,
        })
    }

    pub fn observe(&mut self, boundary: FeedBoundary) -> BoundaryDecision {
        match self.state {
            GateState::Triggered(triggered) => {
                return BoundaryDecision::AlreadyTriggered {
                    l1_block_number: triggered.l1_block_number,
                    l1_timestamp: triggered.l1_timestamp,
                };
            }
            GateState::Expired(expired) => {
                return BoundaryDecision::Expired {
                    l1_block_number: expired.l1_block_number,
                    l1_timestamp: expired.l1_timestamp,
                };
            }
            GateState::FailedClosed => return BoundaryDecision::FailedClosed,
            GateState::Waiting => {}
        }

        if !boundary.sequence_contiguous || self.regressed(boundary) {
            self.state = GateState::FailedClosed;
            return BoundaryDecision::FailedClosed;
        }
        self.last_boundary = Some(boundary);

        if boundary.l1_block_number > self.conditions.block_number_max
            || self
                .conditions
                .timestamp_max
                .is_some_and(|max| boundary.l1_timestamp > max)
        {
            self.state = GateState::Expired(boundary);
            return BoundaryDecision::Expired {
                l1_block_number: boundary.l1_block_number,
                l1_timestamp: boundary.l1_timestamp,
            };
        }

        if boundary.l1_block_number < self.conditions.block_number_min
            || self
                .conditions
                .timestamp_min
                .is_some_and(|min| boundary.l1_timestamp < min)
        {
            return BoundaryDecision::Waiting {
                l1_block_number: boundary.l1_block_number,
                l1_timestamp: boundary.l1_timestamp,
            };
        }

        self.state = GateState::Triggered(boundary);
        BoundaryDecision::SubmitNow {
            l1_block_number: boundary.l1_block_number,
            l1_timestamp: boundary.l1_timestamp,
        }
    }

    fn regressed(&self, boundary: FeedBoundary) -> bool {
        self.last_boundary.is_some_and(|last| {
            boundary.l1_block_number < last.l1_block_number
                || (boundary.l1_block_number == last.l1_block_number
                    && boundary.l1_timestamp < last.l1_timestamp)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> BoundaryGate {
        BoundaryGate::new(
            ConditionalOptions::first_eligible_window(100, 2, None).expect("valid window"),
        )
        .expect("valid gate")
    }

    fn boundary(l1_block_number: u64) -> FeedBoundary {
        FeedBoundary {
            l1_block_number,
            l1_timestamp: 1_800_000_000 + l1_block_number,
            sequence_contiguous: true,
        }
    }

    #[test]
    fn waits_at_launch_block_and_triggers_at_first_eligible_header() {
        let mut gate = gate();
        assert!(matches!(
            gate.observe(boundary(100)),
            BoundaryDecision::Waiting { .. }
        ));
        assert!(matches!(
            gate.observe(boundary(101)),
            BoundaryDecision::SubmitNow {
                l1_block_number: 101,
                ..
            }
        ));
    }

    #[test]
    fn duplicate_headers_cannot_trigger_a_second_submission() {
        let mut gate = gate();
        let eligible = boundary(101);
        assert!(matches!(
            gate.observe(eligible),
            BoundaryDecision::SubmitNow { .. }
        ));
        assert!(matches!(
            gate.observe(eligible),
            BoundaryDecision::AlreadyTriggered {
                l1_block_number: 101,
                ..
            }
        ));
    }

    #[test]
    fn expires_when_feed_skips_past_the_window() {
        let mut gate = gate();
        assert!(matches!(
            gate.observe(boundary(104)),
            BoundaryDecision::Expired {
                l1_block_number: 104,
                ..
            }
        ));
    }

    #[test]
    fn regression_fails_closed() {
        let mut gate = gate();
        assert!(matches!(
            gate.observe(boundary(100)),
            BoundaryDecision::Waiting { .. }
        ));
        assert_eq!(gate.observe(boundary(99)), BoundaryDecision::FailedClosed);
        assert_eq!(gate.observe(boundary(101)), BoundaryDecision::FailedClosed);
    }

    #[test]
    fn unhealthy_sequence_fails_closed() {
        let mut gate = gate();
        let mut eligible = boundary(101);
        eligible.sequence_contiguous = false;
        assert_eq!(gate.observe(eligible), BoundaryDecision::FailedClosed);
    }

    #[test]
    fn timestamp_window_is_enforced() {
        let conditions = ConditionalOptions {
            block_number_min: 101,
            block_number_max: 103,
            timestamp_min: Some(1_000),
            timestamp_max: Some(2_000),
        };
        let mut gate = BoundaryGate::new(conditions).unwrap();
        assert!(matches!(
            gate.observe(FeedBoundary {
                l1_block_number: 101,
                l1_timestamp: 999,
                sequence_contiguous: true,
            }),
            BoundaryDecision::Waiting { .. }
        ));
        assert!(matches!(
            gate.observe(FeedBoundary {
                l1_block_number: 102,
                l1_timestamp: 2_001,
                sequence_contiguous: true,
            }),
            BoundaryDecision::Expired { .. }
        ));
    }
}
