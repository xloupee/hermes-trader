use crate::hot_path::HotPathReport;

impl HotPathReport {
    /// A transaction that may have reached the sequencer cannot be allowed to
    /// disappear when the background queue is saturated. The feed runtime must
    /// stop admitting new trades until this hash is durably reconciled.
    pub fn must_halt(&self) -> bool {
        self.result.requires_reconciliation() && !self.reconciliation_queued
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use alloy_primitives::B256;

    use crate::hot_path::{HotPathReport, SubmissionResult};

    #[test]
    fn missing_reconciliation_handoff_is_fail_closed() {
        let ambiguous = HotPathReport {
            tx_hash: B256::with_last_byte(1),
            nonce: 7,
            submit_elapsed: Duration::from_millis(2),
            result: SubmissionResult::TransportAmbiguous {
                message: "timeout".into(),
            },
            reconciliation_queued: false,
        };
        assert!(ambiguous.must_halt());

        let mut queued = ambiguous.clone();
        queued.reconciliation_queued = true;
        assert!(!queued.must_halt());

        let rejected = HotPathReport {
            result: SubmissionResult::Rejected {
                code: -32_000,
                message: "invalid transaction".into(),
            },
            ..ambiguous
        };
        assert!(!rejected.must_halt());

        let http_error = HotPathReport {
            result: SubmissionResult::Rejected {
                code: 500,
                message: "upstream error".into(),
            },
            ..ambiguous
        };
        assert!(http_error.must_halt());
    }
}
