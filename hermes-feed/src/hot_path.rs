use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant};

use alloy_primitives::B256;
use serde::Serialize;
use thiserror::Error;

use crate::boundary_gate::{BoundaryDecision, BoundaryGate, BoundaryGateError, FeedBoundary};
use crate::sequencer::{
    ConditionalOptions, ConditionalResponse, SequencerClient, signed_transaction_hash,
};

/// Fully signed bytes produced from in-memory strategy state. Constructing this
/// value is deliberately separate from RPC hydration and receipt processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotTransaction {
    pub raw: Vec<u8>,
    pub hash: B256,
    pub nonce: u64,
    pub conditions: ConditionalOptions,
}

impl HotTransaction {
    pub fn validate(&self) -> Result<(), HotPathError> {
        if self.raw.is_empty() {
            return Err(HotPathError::EmptyTransaction);
        }
        if signed_transaction_hash(&self.raw) != self.hash {
            return Err(HotPathError::HashMismatch);
        }
        Ok(())
    }
}

/// One validated transaction waiting for its first eligible feed boundary.
/// Trigger state changes before the network await, so cancellation or a
/// duplicate feed message cannot submit the same bytes twice.
#[derive(Debug, Clone)]
pub struct ArmedHotTransaction {
    transaction: Option<HotTransaction>,
    gate: BoundaryGate,
}

impl ArmedHotTransaction {
    pub fn new(transaction: HotTransaction) -> Result<Self, HotPathError> {
        transaction.validate()?;
        let gate = BoundaryGate::new(transaction.conditions)?;
        Ok(Self {
            transaction: Some(transaction),
            gate,
        })
    }

    pub fn observe(
        &mut self,
        boundary: FeedBoundary,
    ) -> (BoundaryDecision, Option<HotTransaction>) {
        let decision = self.gate.observe(boundary);
        let transaction = matches!(decision, BoundaryDecision::SubmitNow { .. })
            .then(|| self.transaction.take())
            .flatten();
        (decision, transaction)
    }
}

/// The complete synchronous decision interface used by the feed loop. An
/// implementation may read cached in-memory state and sign locally, but cannot
/// receive an RPC client or perform asynchronous work through this API.
pub trait HotPathStrategy<C> {
    type Error: std::error::Error + Send + Sync + 'static;

    fn prepare(&mut self, candidate: C) -> Result<Option<HotTransaction>, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum SubmissionResult {
    Accepted,
    AlreadyKnown,
    BoundaryNotReached { message: String },
    RateLimited { message: String },
    Rejected { code: i64, message: String },
    InvalidResponse { message: String },
    TransportAmbiguous { message: String },
}

impl SubmissionResult {
    /// Once bytes may have reached the sequencer, their nonce must remain
    /// leased until the background reconciler proves the final state.
    pub fn requires_reconciliation(&self) -> bool {
        matches!(
            self,
            Self::Accepted
                | Self::AlreadyKnown
                | Self::RateLimited { .. }
                | Self::InvalidResponse { .. }
                | Self::TransportAmbiguous { .. }
        ) || matches!(self, Self::Rejected { code, .. } if *code >= 100)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReconciliationJob {
    pub tx_hash: B256,
    pub nonce: u64,
    pub submit_elapsed_ns: u128,
    pub result: SubmissionResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotPathReport {
    pub tx_hash: B256,
    pub nonce: u64,
    pub submit_elapsed: Duration,
    pub result: SubmissionResult,
    pub reconciliation_queued: bool,
}

#[derive(Debug, Error)]
pub enum HotPathError {
    #[error("signed transaction bytes are empty")]
    EmptyTransaction,
    #[error("signed transaction hash does not match its bytes")]
    HashMismatch,
    #[error("hot-path strategy failed: {0}")]
    Strategy(String),
    #[error("invalid conditional boundary: {0}")]
    Boundary(#[from] BoundaryGateError),
}

/// Receipt-free submission core. It owns no RPC client, database, or logger.
/// Background work is handed off with `try_send`, so a saturated consumer can
/// never delay the feed task or direct sequencer POST.
#[derive(Clone)]
pub struct HotPathExecutor {
    sequencer: SequencerClient,
    reconciliation: SyncSender<ReconciliationJob>,
}

impl HotPathExecutor {
    pub fn new(sequencer: SequencerClient, reconciliation: SyncSender<ReconciliationJob>) -> Self {
        Self {
            sequencer,
            reconciliation,
        }
    }

    pub async fn handle<C, S>(
        &self,
        strategy: &mut S,
        candidate: C,
    ) -> Result<Option<HotPathReport>, HotPathError>
    where
        S: HotPathStrategy<C>,
    {
        let Some(transaction) = strategy
            .prepare(candidate)
            .map_err(|error| HotPathError::Strategy(error.to_string()))?
        else {
            return Ok(None);
        };
        transaction.validate()?;
        Ok(Some(self.submit(transaction).await))
    }

    pub async fn handle_boundary(
        &self,
        armed: &mut ArmedHotTransaction,
        boundary: FeedBoundary,
    ) -> (BoundaryDecision, Option<HotPathReport>) {
        let (decision, transaction) = armed.observe(boundary);
        let report = match transaction {
            Some(transaction) => Some(self.submit(transaction).await),
            None => None,
        };
        (decision, report)
    }

    pub async fn submit_transaction(
        &self,
        transaction: HotTransaction,
    ) -> Result<HotPathReport, HotPathError> {
        transaction.validate()?;
        Ok(self.submit(transaction).await)
    }

    async fn submit(&self, transaction: HotTransaction) -> HotPathReport {
        // Validation is expected before this method. Keeping the actual timed
        // region to one sequencer call makes the metric submission-specific.
        let started = Instant::now();
        let response = self
            .sequencer
            .submit_conditional(&transaction.raw, transaction.conditions)
            .await;
        let submit_elapsed = started.elapsed();
        let result = match response {
            Ok(ConditionalResponse::Accepted { .. }) => SubmissionResult::Accepted,
            Ok(ConditionalResponse::AlreadyKnown { .. }) => SubmissionResult::AlreadyKnown,
            Ok(ConditionalResponse::BoundaryNotReached { message }) => {
                SubmissionResult::BoundaryNotReached { message }
            }
            Ok(ConditionalResponse::RateLimited { message }) => {
                SubmissionResult::RateLimited { message }
            }
            Ok(ConditionalResponse::Rejected { code, message }) => {
                SubmissionResult::Rejected { code, message }
            }
            Ok(ConditionalResponse::InvalidResponse(message)) => {
                SubmissionResult::InvalidResponse { message }
            }
            Err(error) => SubmissionResult::TransportAmbiguous {
                message: error.to_string(),
            },
        };
        let job = ReconciliationJob {
            tx_hash: transaction.hash,
            nonce: transaction.nonce,
            submit_elapsed_ns: submit_elapsed.as_nanos(),
            result: result.clone(),
        };
        let reconciliation_queued = match self.reconciliation.try_send(job) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
        };
        HotPathReport {
            tx_hash: transaction.hash,
            nonce: transaction.nonce,
            submit_elapsed,
            result,
            reconciliation_queued,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;

    struct Strategy {
        transaction: Option<HotTransaction>,
    }

    impl HotPathStrategy<()> for Strategy {
        type Error = Infallible;

        fn prepare(&mut self, (): ()) -> Result<Option<HotTransaction>, Self::Error> {
            Ok(self.transaction.take())
        }
    }

    fn transaction() -> HotTransaction {
        let raw = vec![0x02, 0xf8, 0x01];
        HotTransaction {
            hash: signed_transaction_hash(&raw),
            raw,
            nonce: 7,
            conditions: ConditionalOptions::first_eligible_window(100, 3, None).unwrap(),
        }
    }

    #[test]
    fn validates_exact_signed_bytes_before_submission() {
        let mut transaction = transaction();
        transaction.raw.push(0xff);
        assert!(matches!(
            transaction.validate(),
            Err(HotPathError::HashMismatch)
        ));
    }

    #[test]
    fn strategy_can_reject_without_creating_submission_work() {
        let mut strategy = Strategy { transaction: None };
        assert!(strategy.prepare(()).unwrap().is_none());
    }

    #[test]
    fn only_explicit_early_boundary_is_safe_without_reconciliation() {
        assert!(
            !SubmissionResult::BoundaryNotReached {
                message: "not reached".into()
            }
            .requires_reconciliation()
        );
        assert!(
            SubmissionResult::TransportAmbiguous {
                message: "timeout".into()
            }
            .requires_reconciliation()
        );
        assert!(SubmissionResult::Accepted.requires_reconciliation());
        assert!(
            SubmissionResult::Rejected {
                code: 500,
                message: "upstream failure".into(),
            }
            .requires_reconciliation()
        );
        assert!(
            !SubmissionResult::Rejected {
                code: -32_000,
                message: "JSON-RPC rejection".into(),
            }
            .requires_reconciliation()
        );
    }

    #[test]
    fn armed_transaction_is_released_exactly_once_at_feed_boundary() {
        let transaction = transaction();
        let mut armed = ArmedHotTransaction::new(transaction.clone()).unwrap();
        let waiting = FeedBoundary {
            l1_block_number: 100,
            l1_timestamp: 1_800_000_000,
            sequence_contiguous: true,
        };
        let (decision, prepared) = armed.observe(waiting);
        assert!(matches!(decision, BoundaryDecision::Waiting { .. }));
        assert!(prepared.is_none());

        let eligible = FeedBoundary {
            l1_block_number: 101,
            l1_timestamp: 1_800_000_001,
            sequence_contiguous: true,
        };
        let (decision, prepared) = armed.observe(eligible);
        assert!(matches!(decision, BoundaryDecision::SubmitNow { .. }));
        assert_eq!(prepared, Some(transaction));

        let (decision, prepared) = armed.observe(eligible);
        assert!(matches!(
            decision,
            BoundaryDecision::AlreadyTriggered { .. }
        ));
        assert!(prepared.is_none());
    }
}
