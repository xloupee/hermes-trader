use std::collections::BTreeMap;

use alloy_primitives::{Address, B256, U256};
use serde::Serialize;
use thiserror::Error;

use crate::boundary_gate::{BoundaryDecision, FeedBoundary};
use crate::hot_path::{
    ArmedHotTransaction, HotPathError, HotPathReport, HotTransaction, SubmissionResult,
};
use crate::noxa_trade::{
    ApprovalTransactionPlan, PreparedRawTransaction, TradePlanError, TradeTransactionPlan,
};
use crate::sequencer::ConditionalOptions;
use crate::signer::TradeSigner;
use crate::testnet_orchestrator::{
    DedicatedNonceManager, NonceError, RiskError, RiskLedger, RiskLimits, RiskStatus,
};

#[derive(Debug, Clone)]
struct PendingSignedTrade {
    nonce: u64,
    tx_hash: B256,
    risk_reservation_id: u64,
    kind: PendingSignedKind,
    armed: ArmedHotTransaction,
}

#[derive(Debug, Clone, Copy)]
enum PendingSignedKind {
    Entry { token: Address, amount_in: U256 },
    Exit { token: Address, cost_basis: U256 },
    Approval { token: Address },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SignedPendingKind {
    Entry { token: Address },
    Approval { token: Address },
    Exit { token: Address },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SignedPosition {
    pub token: Address,
    pub token_amount: U256,
    pub cost_basis: U256,
    pub entry_nonce: u64,
    pub entry_tx_hash: B256,
    pub router_approved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignedRuntimeSnapshot {
    pub signer: alloy_primitives::Address,
    pub next_nonce: u64,
    pub pending_tx_hash: Option<B256>,
    pub pending_nonce: Option<u64>,
    pub realized_session_loss: U256,
    pub open_exposure: U256,
    pub positions: Vec<SignedPosition>,
    pub entry_halted: bool,
    pub halted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignedBoundaryRelease {
    pub decision: BoundaryDecision,
    pub tx_hash: Option<B256>,
    pub nonce: Option<u64>,
    #[serde(skip)]
    pub transaction: Option<HotTransaction>,
}

#[derive(Debug, Error)]
pub enum SignedRuntimeError {
    #[error("signed runtime is halted pending operator review")]
    Halted,
    #[error("another signed transaction is pending reconciliation")]
    TradePending,
    #[error("no signed transaction is pending")]
    NoPendingTrade,
    #[error("trade plan nonce does not match the dedicated nonce lease")]
    NonceMismatch,
    #[error("submission report does not match the pending transaction")]
    ReportMismatch,
    #[error("a signed position already exists for this token")]
    PositionExists,
    #[error("no signed position exists for this token")]
    PositionMissing,
    #[error("signed exits must sell the complete recorded token position")]
    ExitAmountMismatch,
    #[error("signed exit requires a reconciled exact router approval")]
    ApprovalRequired,
    #[error("successful entry reconciliation requires a non-zero token fill")]
    FillMissing,
    #[error(transparent)]
    Nonce(#[from] NonceError),
    #[error(transparent)]
    Risk(#[from] RiskError),
    #[error(transparent)]
    TradePlan(#[from] TradePlanError),
    #[error(transparent)]
    HotPath(#[from] HotPathError),
}

/// Single-owner signed runtime core. It prepares one transaction ahead of the
/// L1 boundary, marks its nonce submitted before releasing bytes to the
/// network, and admits no second trade until the first hash is reconciled.
pub struct SignedTradingRuntime<S> {
    signer: S,
    nonces: DedicatedNonceManager,
    risk: RiskLedger,
    pending: Option<PendingSignedTrade>,
    positions: BTreeMap<Address, SignedPosition>,
    halted: bool,
}

impl<S: TradeSigner> SignedTradingRuntime<S> {
    pub fn new(signer: S, pending_nonce: u64, limits: RiskLimits) -> Self {
        Self {
            signer,
            nonces: DedicatedNonceManager::from_pending_nonce(pending_nonce),
            risk: RiskLedger::new(limits),
            pending: None,
            positions: BTreeMap::new(),
            halted: false,
        }
    }

    pub fn arm_trade(
        &mut self,
        plan: &TradeTransactionPlan,
        conditions: ConditionalOptions,
        slippage_bps: u16,
    ) -> Result<B256, SignedRuntimeError> {
        if self.halted {
            return Err(SignedRuntimeError::Halted);
        }
        if self.pending.is_some() {
            return Err(SignedRuntimeError::TradePending);
        }
        let trade = plan_trade(plan)?;
        let lease = self.nonces.reserve()?;
        if plan.nonce != lease.nonce {
            self.nonces.release_never_submitted(lease.nonce)?;
            return Err(SignedRuntimeError::NonceMismatch);
        }
        let (kind, reservation) = match trade {
            PlannedSignedTrade::Entry { token, amount_in } => {
                if self.positions.contains_key(&token) {
                    self.nonces.release_never_submitted(lease.nonce)?;
                    return Err(SignedRuntimeError::PositionExists);
                }
                let reservation = self.risk.reserve(
                    amount_in,
                    plan.gas_limit,
                    plan.max_fee_per_gas,
                    slippage_bps,
                );
                (PendingSignedKind::Entry { token, amount_in }, reservation)
            }
            PlannedSignedTrade::Exit {
                token,
                token_amount_in,
            } => {
                let Some(position) = self.positions.get(&token).copied() else {
                    self.nonces.release_never_submitted(lease.nonce)?;
                    return Err(SignedRuntimeError::PositionMissing);
                };
                if token_amount_in != position.token_amount {
                    self.nonces.release_never_submitted(lease.nonce)?;
                    return Err(SignedRuntimeError::ExitAmountMismatch);
                }
                if !position.router_approved {
                    self.nonces.release_never_submitted(lease.nonce)?;
                    return Err(SignedRuntimeError::ApprovalRequired);
                }
                let reservation = self.risk.reserve_exit(
                    position.cost_basis,
                    plan.gas_limit,
                    plan.max_fee_per_gas,
                    slippage_bps,
                );
                (
                    PendingSignedKind::Exit {
                        token,
                        cost_basis: position.cost_basis,
                    },
                    reservation,
                )
            }
        };
        let reservation = match reservation {
            Ok(reservation) => reservation,
            Err(error) => {
                self.nonces.release_never_submitted(lease.nonce)?;
                return Err(error.into());
            }
        };
        let prepared = match self.signer.sign_trade(plan) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.risk.release_unsubmitted(reservation.id)?;
                self.nonces.release_never_submitted(lease.nonce)?;
                return Err(error.into());
            }
        };
        self.finish_arm(lease.nonce, reservation, prepared, kind, conditions)
    }

    pub fn arm_approval(
        &mut self,
        plan: &ApprovalTransactionPlan,
        conditions: ConditionalOptions,
    ) -> Result<B256, SignedRuntimeError> {
        if self.halted {
            return Err(SignedRuntimeError::Halted);
        }
        if self.pending.is_some() {
            return Err(SignedRuntimeError::TradePending);
        }
        let Some(position) = self.positions.get(&plan.token) else {
            return Err(SignedRuntimeError::PositionMissing);
        };
        if plan.amount != position.token_amount || plan.expected_owner != self.signer.address() {
            return Err(SignedRuntimeError::ExitAmountMismatch);
        }
        let lease = self.nonces.reserve()?;
        if plan.nonce != lease.nonce {
            self.nonces.release_never_submitted(lease.nonce)?;
            return Err(SignedRuntimeError::NonceMismatch);
        }
        let reservation = match self
            .risk
            .reserve_maintenance(plan.gas_limit, plan.max_fee_per_gas)
        {
            Ok(reservation) => reservation,
            Err(error) => {
                self.nonces.release_never_submitted(lease.nonce)?;
                return Err(error.into());
            }
        };
        let prepared = match self.signer.sign_approval(plan) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.risk.release_unsubmitted(reservation.id)?;
                self.nonces.release_never_submitted(lease.nonce)?;
                return Err(error.into());
            }
        };
        self.finish_arm(
            lease.nonce,
            reservation,
            prepared,
            PendingSignedKind::Approval { token: plan.token },
            conditions,
        )
    }

    fn finish_arm(
        &mut self,
        nonce: u64,
        reservation: crate::testnet_orchestrator::RiskReservation,
        prepared: PreparedRawTransaction,
        kind: PendingSignedKind,
        conditions: ConditionalOptions,
    ) -> Result<B256, SignedRuntimeError> {
        if prepared.signer != self.signer.address() {
            self.risk.release_unsubmitted(reservation.id)?;
            self.nonces.release_never_submitted(nonce)?;
            return Err(TradePlanError::RecipientSignerMismatch.into());
        }
        self.nonces.mark_signed(nonce, prepared.hash)?;
        let armed = match ArmedHotTransaction::new(HotTransaction {
            raw: prepared.raw,
            hash: prepared.hash,
            nonce,
            conditions,
        }) {
            Ok(armed) => armed,
            Err(error) => {
                self.risk.release_unsubmitted(reservation.id)?;
                self.nonces.release_never_submitted(nonce)?;
                return Err(error.into());
            }
        };
        self.pending = Some(PendingSignedTrade {
            nonce,
            tx_hash: prepared.hash,
            risk_reservation_id: reservation.id,
            kind,
            armed,
        });
        Ok(prepared.hash)
    }

    pub fn observe_boundary(
        &mut self,
        boundary: FeedBoundary,
    ) -> Result<SignedBoundaryRelease, SignedRuntimeError> {
        let pending = self
            .pending
            .as_mut()
            .ok_or(SignedRuntimeError::NoPendingTrade)?;
        let (decision, transaction) = pending.armed.observe(boundary);
        let cancel_unsubmitted = transaction.is_none()
            && matches!(
                decision,
                BoundaryDecision::Expired { .. } | BoundaryDecision::FailedClosed
            );
        let pending_nonce = pending.nonce;
        let pending_hash = pending.tx_hash;
        let risk_reservation_id = pending.risk_reservation_id;
        if transaction.is_some() {
            // This transition happens before the caller can perform a network
            // await, preventing cancellation from making the nonce reusable.
            self.nonces.mark_submitted(pending.nonce, pending.tx_hash)?;
        } else if cancel_unsubmitted {
            self.nonces.release_never_submitted(pending_nonce)?;
            self.risk.release_unsubmitted(risk_reservation_id)?;
            self.pending = None;
        }
        Ok(SignedBoundaryRelease {
            decision,
            tx_hash: transaction.as_ref().map(|_| pending_hash),
            nonce: transaction.as_ref().map(|_| pending_nonce),
            transaction,
        })
    }

    pub fn apply_submission_report(
        &mut self,
        report: &HotPathReport,
    ) -> Result<(), SignedRuntimeError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(SignedRuntimeError::NoPendingTrade)?;
        if pending.nonce != report.nonce || pending.tx_hash != report.tx_hash {
            return Err(SignedRuntimeError::ReportMismatch);
        }
        if report.result.requires_reconciliation() {
            if report.must_halt() {
                self.halted = true;
            }
            return Ok(());
        }
        match report.result {
            SubmissionResult::BoundaryNotReached { .. } | SubmissionResult::Rejected { .. } => {
                let pending = self.pending.take().expect("pending checked above");
                self.nonces
                    .release_explicitly_rejected(pending.nonce, pending.tx_hash)?;
                self.risk.release_unsubmitted(pending.risk_reservation_id)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn reconcile_included(
        &mut self,
        tx_hash: B256,
        succeeded: bool,
        actual_token_out: Option<U256>,
        gas_cost: U256,
    ) -> Result<RiskStatus, SignedRuntimeError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(SignedRuntimeError::NoPendingTrade)?;
        if pending.tx_hash != tx_hash {
            return Err(SignedRuntimeError::ReportMismatch);
        }
        let requires_fill = !matches!(pending.kind, PendingSignedKind::Approval { .. });
        if succeeded && requires_fill && actual_token_out.is_none_or(|amount| amount.is_zero()) {
            return Err(SignedRuntimeError::FillMissing);
        }
        let reservation = self
            .risk
            .active
            .filter(|reservation| reservation.id == pending.risk_reservation_id)
            .ok_or(RiskError::ReservationMismatch)?;
        if gas_cost > reservation.max_gas_cost {
            return Err(RiskError::GasCost.into());
        }
        let pending = self.pending.take().expect("pending checked above");
        self.nonces
            .finalize_included(pending.nonce, pending.tx_hash)?;
        let status = if !succeeded {
            self.risk.settle(pending.risk_reservation_id, gas_cost)?
        } else {
            match pending.kind {
                PendingSignedKind::Entry { token, amount_in } => {
                    let actual_token_out =
                        actual_token_out.expect("successful entry fill checked above");
                    let status = self
                        .risk
                        .settle_entry(pending.risk_reservation_id, gas_cost)?;
                    let position = SignedPosition {
                        token,
                        token_amount: actual_token_out,
                        cost_basis: amount_in,
                        entry_nonce: pending.nonce,
                        entry_tx_hash: pending.tx_hash,
                        router_approved: false,
                    };
                    if self.positions.insert(position.token, position).is_some() {
                        self.halted = true;
                        return Err(SignedRuntimeError::PositionExists);
                    }
                    status
                }
                PendingSignedKind::Exit { token, cost_basis } => {
                    let actual_token_out =
                        actual_token_out.expect("successful exit fill checked above");
                    let trade_loss = cost_basis
                        .checked_sub(actual_token_out)
                        .unwrap_or(U256::ZERO);
                    let realized_loss = trade_loss
                        .checked_add(gas_cost)
                        .ok_or(RiskError::Overflow)?;
                    let status = self
                        .risk
                        .settle_exit(pending.risk_reservation_id, realized_loss)?;
                    if self.positions.remove(&token).is_none() {
                        self.halted = true;
                        return Err(SignedRuntimeError::PositionMissing);
                    }
                    status
                }
                PendingSignedKind::Approval { token } => {
                    let Some(position) = self.positions.get_mut(&token) else {
                        self.halted = true;
                        return Err(SignedRuntimeError::PositionMissing);
                    };
                    position.router_approved = true;
                    self.risk.settle(pending.risk_reservation_id, gas_cost)?
                }
            }
        };
        Ok(status)
    }

    pub fn pending_fill_target(&self) -> Option<(Address, Address)> {
        self.pending.as_ref().and_then(|pending| {
            let output_token = match pending.kind {
                PendingSignedKind::Entry { token, .. } => Some(token),
                PendingSignedKind::Exit { .. } => Some(crate::robinhood::WETH),
                PendingSignedKind::Approval { .. } => None,
            }?;
            Some((output_token, self.signer.address()))
        })
    }

    pub fn pending_kind(&self) -> Option<SignedPendingKind> {
        self.pending.as_ref().map(|pending| match pending.kind {
            PendingSignedKind::Entry { token, .. } => SignedPendingKind::Entry { token },
            PendingSignedKind::Approval { token } => SignedPendingKind::Approval { token },
            PendingSignedKind::Exit { token, .. } => SignedPendingKind::Exit { token },
        })
    }

    /// Finalize a transaction that was released by the boundary gate but was
    /// deliberately not sent to any network endpoint.
    pub fn complete_dry_run(&mut self, tx_hash: B256) -> Result<(), SignedRuntimeError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(SignedRuntimeError::NoPendingTrade)?;
        if pending.tx_hash != tx_hash {
            return Err(SignedRuntimeError::ReportMismatch);
        }
        let pending = self.pending.take().expect("pending checked above");
        self.nonces
            .release_explicitly_rejected(pending.nonce, pending.tx_hash)?;
        self.risk.release_unsubmitted(pending.risk_reservation_id)?;
        Ok(())
    }

    pub fn halt_unresolved(&mut self, tx_hash: B256) -> Result<(), SignedRuntimeError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(SignedRuntimeError::NoPendingTrade)?;
        if pending.tx_hash != tx_hash {
            return Err(SignedRuntimeError::ReportMismatch);
        }
        self.halted = true;
        Ok(())
    }

    pub fn snapshot(&self) -> SignedRuntimeSnapshot {
        SignedRuntimeSnapshot {
            signer: self.signer.address(),
            next_nonce: self.nonces.next_nonce(),
            pending_tx_hash: self.pending.as_ref().map(|pending| pending.tx_hash),
            pending_nonce: self.pending.as_ref().map(|pending| pending.nonce),
            realized_session_loss: self.risk.realized_session_loss,
            open_exposure: self.risk.open_exposure,
            positions: self.positions.values().copied().collect(),
            entry_halted: self.halted
                || self.risk.realized_session_loss >= self.risk.limits.max_session_loss,
            halted: self.halted,
        }
    }
}

enum PlannedSignedTrade {
    Entry {
        token: Address,
        amount_in: U256,
    },
    Exit {
        token: Address,
        token_amount_in: U256,
    },
}

fn plan_trade(plan: &TradeTransactionPlan) -> Result<PlannedSignedTrade, TradePlanError> {
    use crate::noxa_abi::{decode_v3_exact_input_single, decode_v3_exact_output_single};
    use crate::robinhood::WETH;

    if let Some(intent) = decode_v3_exact_input_single(&plan.calldata) {
        if intent.token_in == WETH {
            Ok(PlannedSignedTrade::Entry {
                token: intent.token_out,
                amount_in: intent.amount_in,
            })
        } else {
            Ok(PlannedSignedTrade::Exit {
                token: intent.token_in,
                token_amount_in: intent.amount_in,
            })
        }
    } else if let Some(intent) = decode_v3_exact_output_single(&plan.calldata) {
        if intent.token_in != WETH {
            return Err(TradePlanError::UnsafeSwapParameters);
        }
        Ok(PlannedSignedTrade::Entry {
            token: intent.token_out,
            amount_in: intent.amount_in_maximum,
        })
    } else {
        Err(TradePlanError::UnsupportedCalldata)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use alloy_primitives::Address;
    use k256::ecdsa::SigningKey;

    use super::*;
    use crate::noxa_abi::V3ExactInputIntent;
    use crate::noxa_trade::{ApprovalTransactionPlan, PreparedRawTransaction};
    use crate::robinhood::{NOXA_POOL_FEE, WETH};

    struct MemorySigner {
        key: SigningKey,
        address: Address,
    }

    impl MemorySigner {
        fn new() -> Self {
            let key = SigningKey::from_slice(&[7_u8; 32]).unwrap();
            let address = Address::from_private_key(&key);
            Self { key, address }
        }
    }

    impl TradeSigner for MemorySigner {
        fn address(&self) -> Address {
            self.address
        }

        fn sign_trade(
            &self,
            plan: &TradeTransactionPlan,
        ) -> Result<PreparedRawTransaction, TradePlanError> {
            plan.sign(&self.key)
        }

        fn sign_approval(
            &self,
            plan: &crate::noxa_trade::ApprovalTransactionPlan,
        ) -> Result<PreparedRawTransaction, TradePlanError> {
            plan.sign(&self.key)
        }
    }

    fn limits() -> RiskLimits {
        RiskLimits {
            max_trade_amount_in: U256::from(1_000),
            max_open_exposure: U256::from(1_000),
            max_gas_cost_wei: U256::from(100_000),
            max_session_loss: U256::from(100),
            max_slippage_bps: 500,
        }
    }

    fn plan(nonce: u64, recipient: Address) -> TradeTransactionPlan {
        TradeTransactionPlan::exact_input(
            nonce,
            1_000,
            10,
            0,
            &V3ExactInputIntent {
                token_in: WETH,
                token_out: Address::with_last_byte(99),
                fee: NOXA_POOL_FEE,
                recipient,
                amount_in: U256::from(500),
                amount_out_minimum: U256::from(1),
                sqrt_price_limit_x96: U256::ZERO,
            },
        )
        .unwrap()
    }

    fn exit_plan(
        nonce: u64,
        recipient: Address,
        token: Address,
        token_amount: U256,
    ) -> TradeTransactionPlan {
        TradeTransactionPlan::exact_input(
            nonce,
            1_000,
            10,
            0,
            &V3ExactInputIntent {
                token_in: token,
                token_out: WETH,
                fee: NOXA_POOL_FEE,
                recipient,
                amount_in: token_amount,
                amount_out_minimum: U256::from(1),
                sqrt_price_limit_x96: U256::ZERO,
            },
        )
        .unwrap()
    }

    fn boundary(block: u64) -> FeedBoundary {
        FeedBoundary {
            l1_block_number: block,
            l1_timestamp: 1_800_000_000 + block,
            sequence_contiguous: true,
        }
    }

    fn armed_runtime() -> (SignedTradingRuntime<MemorySigner>, B256) {
        let signer = MemorySigner::new();
        let plan = plan(7, signer.address());
        let mut runtime = SignedTradingRuntime::new(signer, 7, limits());
        let hash = runtime
            .arm_trade(
                &plan,
                ConditionalOptions::first_eligible_window(100, 2, None).unwrap(),
                100,
            )
            .unwrap();
        (runtime, hash)
    }

    #[test]
    fn signs_ahead_and_releases_bytes_once_at_the_feed_boundary() {
        let (mut runtime, hash) = armed_runtime();
        assert!(
            runtime
                .observe_boundary(boundary(100))
                .unwrap()
                .transaction
                .is_none()
        );
        let release = runtime.observe_boundary(boundary(101)).unwrap();
        assert_eq!(release.tx_hash, Some(hash));
        assert!(release.transaction.is_some());
        assert!(
            runtime
                .observe_boundary(boundary(101))
                .unwrap()
                .transaction
                .is_none()
        );
    }

    #[test]
    fn accepted_hash_stays_leased_until_reconciliation() {
        let (mut runtime, hash) = armed_runtime();
        runtime.observe_boundary(boundary(101)).unwrap();
        runtime
            .apply_submission_report(&HotPathReport {
                tx_hash: hash,
                nonce: 7,
                submit_elapsed: Duration::from_millis(1),
                result: SubmissionResult::Accepted,
                reconciliation_queued: true,
            })
            .unwrap();
        assert_eq!(runtime.snapshot().pending_tx_hash, Some(hash));
        assert!(matches!(
            runtime.reconcile_included(hash, true, None, U256::from(1)),
            Err(SignedRuntimeError::FillMissing)
        ));
        assert!(matches!(
            runtime.reconcile_included(hash, true, Some(U256::from(900)), U256::from(10_001),),
            Err(SignedRuntimeError::Risk(RiskError::GasCost))
        ));
        assert_eq!(runtime.snapshot().pending_tx_hash, Some(hash));
        assert_eq!(
            runtime
                .reconcile_included(hash, true, Some(U256::from(900)), U256::from(1))
                .unwrap(),
            RiskStatus::WithinLimits
        );
        assert_eq!(runtime.snapshot().next_nonce, 8);
        assert!(runtime.snapshot().pending_tx_hash.is_none());
        assert_eq!(runtime.snapshot().open_exposure, U256::from(500));
        assert_eq!(runtime.snapshot().positions.len(), 1);
        assert_eq!(
            runtime.snapshot().positions[0].token_amount,
            U256::from(900)
        );
    }

    #[test]
    fn dry_run_reuses_nonce_and_does_not_open_exposure() {
        let (mut runtime, hash) = armed_runtime();
        let release = runtime.observe_boundary(boundary(101)).unwrap();
        assert!(release.transaction.is_some());
        runtime.complete_dry_run(hash).unwrap();
        assert_eq!(runtime.snapshot().next_nonce, 7);
        assert_eq!(runtime.snapshot().open_exposure, U256::ZERO);
        assert!(runtime.snapshot().pending_tx_hash.is_none());
    }

    #[test]
    fn full_exit_releases_exposure_and_remains_allowed_through_loss_breach() {
        let (mut runtime, entry_hash) = armed_runtime();
        runtime.observe_boundary(boundary(101)).unwrap();
        runtime
            .reconcile_included(entry_hash, true, Some(U256::from(900)), U256::from(1))
            .unwrap();

        let signer = runtime.snapshot().signer;
        let token = Address::with_last_byte(99);
        assert!(matches!(
            runtime.arm_trade(
                &exit_plan(8, signer, token, U256::from(900)),
                ConditionalOptions::first_eligible_window(101, 2, None).unwrap(),
                100,
            ),
            Err(SignedRuntimeError::ApprovalRequired)
        ));
        let approval_hash = runtime
            .arm_approval(
                &ApprovalTransactionPlan::new(8, 1_000, 10, 0, token, U256::from(900), signer)
                    .unwrap(),
                ConditionalOptions::first_eligible_window(101, 2, None).unwrap(),
            )
            .unwrap();
        runtime.observe_boundary(boundary(102)).unwrap();
        assert_eq!(runtime.pending_fill_target(), None);
        runtime
            .reconcile_included(approval_hash, true, None, U256::from(1))
            .unwrap();
        assert!(runtime.snapshot().positions[0].router_approved);

        let exit_hash = runtime
            .arm_trade(
                &exit_plan(9, signer, token, U256::from(900)),
                ConditionalOptions::first_eligible_window(102, 2, None).unwrap(),
                100,
            )
            .unwrap();
        runtime.observe_boundary(boundary(103)).unwrap();
        assert_eq!(runtime.pending_fill_target(), Some((WETH, signer)));
        assert_eq!(
            runtime
                .reconcile_included(exit_hash, true, Some(U256::from(400)), U256::from(5),)
                .unwrap(),
            RiskStatus::LossLimitBreached
        );
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.open_exposure, U256::ZERO);
        assert!(snapshot.positions.is_empty());
        assert!(snapshot.entry_halted);
        assert!(!snapshot.halted);
        assert!(matches!(
            runtime.arm_trade(
                &plan(10, signer),
                ConditionalOptions::first_eligible_window(103, 2, None).unwrap(),
                100,
            ),
            Err(SignedRuntimeError::Risk(RiskError::LossLimit))
        ));
    }

    #[test]
    fn explicit_rejection_releases_nonce_and_risk_reservation() {
        let (mut runtime, hash) = armed_runtime();
        runtime.observe_boundary(boundary(101)).unwrap();
        runtime
            .apply_submission_report(&HotPathReport {
                tx_hash: hash,
                nonce: 7,
                submit_elapsed: Duration::from_millis(1),
                result: SubmissionResult::Rejected {
                    code: -32_000,
                    message: "definitive rejection".into(),
                },
                reconciliation_queued: false,
            })
            .unwrap();
        assert_eq!(runtime.snapshot().next_nonce, 7);
        assert!(runtime.snapshot().pending_tx_hash.is_none());
    }

    #[test]
    fn dropped_ambiguous_reconciliation_halts_runtime() {
        let (mut runtime, hash) = armed_runtime();
        runtime.observe_boundary(boundary(101)).unwrap();
        runtime
            .apply_submission_report(&HotPathReport {
                tx_hash: hash,
                nonce: 7,
                submit_elapsed: Duration::from_millis(1),
                result: SubmissionResult::TransportAmbiguous {
                    message: "timeout".into(),
                },
                reconciliation_queued: false,
            })
            .unwrap();
        assert!(runtime.snapshot().halted);
        assert!(matches!(
            runtime.arm_trade(
                &plan(8, runtime.snapshot().signer),
                ConditionalOptions::first_eligible_window(101, 2, None).unwrap(),
                100,
            ),
            Err(SignedRuntimeError::Halted)
        ));
    }

    #[test]
    fn feed_failure_before_submission_releases_nonce_and_risk() {
        let (mut runtime, _) = armed_runtime();
        let mut unhealthy = boundary(101);
        unhealthy.sequence_contiguous = false;
        let release = runtime.observe_boundary(unhealthy).unwrap();
        assert_eq!(release.decision, BoundaryDecision::FailedClosed);
        assert!(release.transaction.is_none());
        assert_eq!(runtime.snapshot().next_nonce, 7);
        assert!(runtime.snapshot().pending_tx_hash.is_none());
    }
}
