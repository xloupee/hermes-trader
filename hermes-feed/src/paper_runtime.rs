use std::collections::BTreeMap;

use alloy_primitives::{Address, B256, U256, keccak256};
use serde::Serialize;
use thiserror::Error;

use crate::boundary_gate::{BoundaryDecision, BoundaryGate, BoundaryGateError, FeedBoundary};
use crate::robinhood::WETH;
use crate::sequencer::ConditionalOptions;
use crate::testnet_orchestrator::{DedicatedNonceManager, NonceError, RiskLimits};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperOrderState {
    Prepared,
    Submitted,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "side")]
pub enum PaperOrderKind {
    Entry {
        token: Address,
        amount_in: U256,
        expected_token_out: U256,
    },
    Exit {
        token: Address,
        expected_proceeds: U256,
    },
}

impl PaperOrderKind {
    fn token(self) -> Address {
        match self {
            Self::Entry { token, .. } | Self::Exit { token, .. } => token,
        }
    }

    fn discriminator(self) -> u8 {
        match self {
            Self::Entry { .. } => 0,
            Self::Exit { .. } => 1,
        }
    }
}

#[derive(Debug, Clone)]
struct PendingPaperOrder {
    id: u64,
    nonce: u64,
    paper_hash: B256,
    kind: PaperOrderKind,
    max_gas_cost: U256,
    state: PaperOrderState,
    gate: BoundaryGate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PaperPosition {
    pub token: Address,
    pub token_amount: U256,
    pub cost_basis: U256,
    pub entry_nonce: u64,
    pub entry_order_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaperRuntimeSnapshot {
    pub next_nonce: u64,
    pub open_exposure: U256,
    pub realized_session_loss: U256,
    pub entry_halted: bool,
    pub pending_order: Option<PaperOrderSnapshot>,
    pub positions: Vec<PaperPosition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PaperOrderSnapshot {
    pub id: u64,
    pub nonce: u64,
    pub paper_hash: B256,
    pub kind: PaperOrderKind,
    pub max_gas_cost: U256,
    pub state: PaperOrderState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PaperBoundaryEvent {
    pub order_id: u64,
    pub nonce: u64,
    pub paper_hash: B256,
    pub decision: BoundaryDecision,
    pub state: PaperOrderState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PaperReconciliation {
    pub order_id: u64,
    pub nonce: u64,
    pub paper_hash: B256,
    pub kind: PaperOrderKind,
    pub actual_amount: U256,
    pub gas_cost: U256,
    pub realized_loss: U256,
    pub open_exposure: U256,
    pub session_loss: U256,
    pub entry_halted: bool,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PaperRuntimeError {
    #[error("another paper order is pending reconciliation")]
    OrderPending,
    #[error("no paper order is pending")]
    NoPendingOrder,
    #[error("paper order ID does not match the pending order")]
    OrderMismatch,
    #[error("paper order has not reached the submission boundary")]
    NotSubmitted,
    #[error("entry token or amounts are invalid")]
    InvalidEntry,
    #[error("exit proceeds are invalid")]
    InvalidExit,
    #[error("a position already exists for this token")]
    PositionExists,
    #[error("no position exists for this token")]
    PositionMissing,
    #[error("trade input exceeds its configured cap")]
    TradeAmount,
    #[error("open exposure would exceed its configured cap")]
    Exposure,
    #[error("slippage exceeds its configured cap")]
    Slippage,
    #[error("maximum gas cost overflows or exceeds its configured cap")]
    GasCost,
    #[error("reported fill gas exceeds the order's reserved maximum")]
    FillGasCost,
    #[error("session loss cap has halted new entries")]
    LossLimit,
    #[error("paper runtime accounting overflow")]
    Overflow,
    #[error(transparent)]
    Boundary(#[from] BoundaryGateError),
    #[error(transparent)]
    Nonce(#[from] NonceError),
}

/// Deterministic, broadcast-free trading state machine used by the live paper
/// runner and scenario tests. It exercises the same feed boundary and nonce
/// invariants as the signed runtime without ever producing transaction bytes.
#[derive(Debug, Clone)]
pub struct AutomatedPaperRuntime {
    limits: RiskLimits,
    nonces: DedicatedNonceManager,
    open_exposure: U256,
    realized_session_loss: U256,
    pending: Option<PendingPaperOrder>,
    positions: BTreeMap<Address, PaperPosition>,
    next_order_id: u64,
}

impl AutomatedPaperRuntime {
    pub fn new(pending_nonce: u64, limits: RiskLimits) -> Self {
        Self {
            limits,
            nonces: DedicatedNonceManager::from_pending_nonce(pending_nonce),
            open_exposure: U256::ZERO,
            realized_session_loss: U256::ZERO,
            pending: None,
            positions: BTreeMap::new(),
            next_order_id: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_entry(
        &mut self,
        token: Address,
        amount_in: U256,
        expected_token_out: U256,
        gas_limit: u64,
        max_fee_per_gas: u128,
        slippage_bps: u16,
        conditions: ConditionalOptions,
    ) -> Result<PaperOrderSnapshot, PaperRuntimeError> {
        self.require_idle()?;
        if token == Address::ZERO
            || token == WETH
            || amount_in == U256::ZERO
            || expected_token_out == U256::ZERO
        {
            return Err(PaperRuntimeError::InvalidEntry);
        }
        if self.positions.contains_key(&token) {
            return Err(PaperRuntimeError::PositionExists);
        }
        if self.entry_halted() {
            return Err(PaperRuntimeError::LossLimit);
        }
        if amount_in > self.limits.max_trade_amount_in {
            return Err(PaperRuntimeError::TradeAmount);
        }
        if slippage_bps > self.limits.max_slippage_bps {
            return Err(PaperRuntimeError::Slippage);
        }
        let exposure = self
            .open_exposure
            .checked_add(amount_in)
            .ok_or(PaperRuntimeError::Overflow)?;
        if exposure > self.limits.max_open_exposure {
            return Err(PaperRuntimeError::Exposure);
        }
        let max_gas_cost = self.validate_gas(gas_limit, max_fee_per_gas)?;
        let kind = PaperOrderKind::Entry {
            token,
            amount_in,
            expected_token_out,
        };
        self.prepare(kind, max_gas_cost, conditions)
    }

    pub fn prepare_exit(
        &mut self,
        token: Address,
        expected_proceeds: U256,
        gas_limit: u64,
        max_fee_per_gas: u128,
        slippage_bps: u16,
        conditions: ConditionalOptions,
    ) -> Result<PaperOrderSnapshot, PaperRuntimeError> {
        self.require_idle()?;
        if expected_proceeds == U256::ZERO {
            return Err(PaperRuntimeError::InvalidExit);
        }
        if !self.positions.contains_key(&token) {
            return Err(PaperRuntimeError::PositionMissing);
        }
        if slippage_bps > self.limits.max_slippage_bps {
            return Err(PaperRuntimeError::Slippage);
        }
        let max_gas_cost = self.validate_gas(gas_limit, max_fee_per_gas)?;
        self.prepare(
            PaperOrderKind::Exit {
                token,
                expected_proceeds,
            },
            max_gas_cost,
            conditions,
        )
    }

    pub fn observe_boundary(
        &mut self,
        boundary: FeedBoundary,
    ) -> Result<PaperBoundaryEvent, PaperRuntimeError> {
        let order = self
            .pending
            .as_mut()
            .ok_or(PaperRuntimeError::NoPendingOrder)?;
        let decision = order.gate.observe(boundary);
        let order_id = order.id;
        let nonce = order.nonce;
        let paper_hash = order.paper_hash;
        let state = match decision {
            BoundaryDecision::SubmitNow { .. } => {
                self.nonces.mark_submitted(nonce, paper_hash)?;
                order.state = PaperOrderState::Submitted;
                PaperOrderState::Submitted
            }
            BoundaryDecision::Expired { .. } | BoundaryDecision::FailedClosed
                if order.state == PaperOrderState::Prepared =>
            {
                self.nonces.release_never_submitted(nonce)?;
                order.state = PaperOrderState::Cancelled;
                PaperOrderState::Cancelled
            }
            _ => order.state,
        };
        if state == PaperOrderState::Cancelled {
            self.pending = None;
        }
        Ok(PaperBoundaryEvent {
            order_id,
            nonce,
            paper_hash,
            decision,
            state,
        })
    }

    pub fn reconcile_fill(
        &mut self,
        order_id: u64,
        actual_amount: U256,
        gas_cost: U256,
    ) -> Result<PaperReconciliation, PaperRuntimeError> {
        let order = self
            .pending
            .as_ref()
            .ok_or(PaperRuntimeError::NoPendingOrder)?;
        if order.id != order_id {
            return Err(PaperRuntimeError::OrderMismatch);
        }
        if order.state != PaperOrderState::Submitted {
            return Err(PaperRuntimeError::NotSubmitted);
        }
        if actual_amount == U256::ZERO {
            return Err(match order.kind {
                PaperOrderKind::Entry { .. } => PaperRuntimeError::InvalidEntry,
                PaperOrderKind::Exit { .. } => PaperRuntimeError::InvalidExit,
            });
        }
        if gas_cost > order.max_gas_cost {
            return Err(PaperRuntimeError::FillGasCost);
        }
        let order = order.clone();
        let trade_loss = match order.kind {
            PaperOrderKind::Entry {
                token, amount_in, ..
            } => {
                if self.positions.contains_key(&token) {
                    return Err(PaperRuntimeError::PositionExists);
                }
                self.open_exposure = self
                    .open_exposure
                    .checked_add(amount_in)
                    .ok_or(PaperRuntimeError::Overflow)?;
                self.positions.insert(
                    token,
                    PaperPosition {
                        token,
                        token_amount: actual_amount,
                        cost_basis: amount_in,
                        entry_nonce: order.nonce,
                        entry_order_id: order.id,
                    },
                );
                U256::ZERO
            }
            PaperOrderKind::Exit { token, .. } => {
                let position = self
                    .positions
                    .remove(&token)
                    .ok_or(PaperRuntimeError::PositionMissing)?;
                self.open_exposure = self
                    .open_exposure
                    .checked_sub(position.cost_basis)
                    .ok_or(PaperRuntimeError::Overflow)?;
                position
                    .cost_basis
                    .checked_sub(actual_amount)
                    .unwrap_or(U256::ZERO)
            }
        };
        let realized_loss = trade_loss
            .checked_add(gas_cost)
            .ok_or(PaperRuntimeError::Overflow)?;
        self.realized_session_loss = self
            .realized_session_loss
            .checked_add(realized_loss)
            .ok_or(PaperRuntimeError::Overflow)?;
        self.nonces
            .finalize_included(order.nonce, order.paper_hash)?;
        self.pending = None;
        Ok(PaperReconciliation {
            order_id: order.id,
            nonce: order.nonce,
            paper_hash: order.paper_hash,
            kind: order.kind,
            actual_amount,
            gas_cost,
            realized_loss,
            open_exposure: self.open_exposure,
            session_loss: self.realized_session_loss,
            entry_halted: self.entry_halted(),
        })
    }

    pub fn reconcile_explicit_rejection(
        &mut self,
        order_id: u64,
    ) -> Result<PaperOrderSnapshot, PaperRuntimeError> {
        let order = self
            .pending
            .as_ref()
            .ok_or(PaperRuntimeError::NoPendingOrder)?;
        if order.id != order_id {
            return Err(PaperRuntimeError::OrderMismatch);
        }
        let snapshot = Self::order_snapshot(order);
        self.nonces
            .release_explicitly_rejected(order.nonce, order.paper_hash)?;
        self.pending = None;
        Ok(PaperOrderSnapshot {
            state: PaperOrderState::Cancelled,
            ..snapshot
        })
    }

    pub fn snapshot(&self) -> PaperRuntimeSnapshot {
        PaperRuntimeSnapshot {
            next_nonce: self.nonces.next_nonce(),
            open_exposure: self.open_exposure,
            realized_session_loss: self.realized_session_loss,
            entry_halted: self.entry_halted(),
            pending_order: self.pending.as_ref().map(Self::order_snapshot),
            positions: self.positions.values().copied().collect(),
        }
    }

    fn prepare(
        &mut self,
        kind: PaperOrderKind,
        max_gas_cost: U256,
        conditions: ConditionalOptions,
    ) -> Result<PaperOrderSnapshot, PaperRuntimeError> {
        let gate = BoundaryGate::new(conditions)?;
        let lease = self.nonces.reserve()?;
        let id = self.next_order_id;
        self.next_order_id = self
            .next_order_id
            .checked_add(1)
            .ok_or(PaperRuntimeError::Overflow)?;
        let paper_hash = paper_order_hash(id, lease.nonce, kind);
        self.nonces.mark_signed(lease.nonce, paper_hash)?;
        let order = PendingPaperOrder {
            id,
            nonce: lease.nonce,
            paper_hash,
            kind,
            max_gas_cost,
            state: PaperOrderState::Prepared,
            gate,
        };
        let snapshot = Self::order_snapshot(&order);
        self.pending = Some(order);
        Ok(snapshot)
    }

    fn require_idle(&self) -> Result<(), PaperRuntimeError> {
        if self.pending.is_some() {
            Err(PaperRuntimeError::OrderPending)
        } else {
            Ok(())
        }
    }

    fn validate_gas(
        &self,
        gas_limit: u64,
        max_fee_per_gas: u128,
    ) -> Result<U256, PaperRuntimeError> {
        let max_gas_cost = U256::from(gas_limit)
            .checked_mul(U256::from(max_fee_per_gas))
            .ok_or(PaperRuntimeError::GasCost)?;
        if gas_limit == 0 || max_fee_per_gas == 0 || max_gas_cost > self.limits.max_gas_cost_wei {
            return Err(PaperRuntimeError::GasCost);
        }
        Ok(max_gas_cost)
    }

    fn entry_halted(&self) -> bool {
        self.realized_session_loss >= self.limits.max_session_loss
    }

    fn order_snapshot(order: &PendingPaperOrder) -> PaperOrderSnapshot {
        PaperOrderSnapshot {
            id: order.id,
            nonce: order.nonce,
            paper_hash: order.paper_hash,
            kind: order.kind,
            max_gas_cost: order.max_gas_cost,
            state: order.state,
        }
    }
}

fn paper_order_hash(id: u64, nonce: u64, kind: PaperOrderKind) -> B256 {
    let mut bytes = Vec::with_capacity(90);
    bytes.extend_from_slice(b"hermes-paper-order-v1");
    bytes.extend_from_slice(&id.to_be_bytes());
    bytes.extend_from_slice(&nonce.to_be_bytes());
    bytes.push(kind.discriminator());
    bytes.extend_from_slice(kind.token().as_slice());
    match kind {
        PaperOrderKind::Entry {
            amount_in,
            expected_token_out,
            ..
        } => {
            bytes.extend_from_slice(&amount_in.to_be_bytes::<32>());
            bytes.extend_from_slice(&expected_token_out.to_be_bytes::<32>());
        }
        PaperOrderKind::Exit {
            expected_proceeds, ..
        } => bytes.extend_from_slice(&expected_proceeds.to_be_bytes::<32>()),
    }
    keccak256(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> RiskLimits {
        RiskLimits {
            max_trade_amount_in: U256::from(1_000),
            max_open_exposure: U256::from(1_500),
            max_gas_cost_wei: U256::from(100),
            max_session_loss: U256::from(200),
            max_slippage_bps: 500,
        }
    }

    fn conditions(launch_l1_block: u64) -> ConditionalOptions {
        ConditionalOptions::first_eligible_window(launch_l1_block, 2, None).unwrap()
    }

    fn boundary(block: u64) -> FeedBoundary {
        FeedBoundary {
            l1_block_number: block,
            l1_timestamp: 1_800_000_000 + block,
            sequence_contiguous: true,
        }
    }

    fn enter(runtime: &mut AutomatedPaperRuntime, token: Address, cost: u64) -> u64 {
        let order = runtime
            .prepare_entry(
                token,
                U256::from(cost),
                U256::from(5_000),
                10,
                5,
                100,
                conditions(100),
            )
            .unwrap();
        let event = runtime.observe_boundary(boundary(101)).unwrap();
        assert_eq!(event.state, PaperOrderState::Submitted);
        runtime
            .reconcile_fill(order.id, U256::from(4_900), U256::from(10))
            .unwrap();
        order.id
    }

    #[test]
    fn entry_uses_boundary_nonce_and_reconciliation_exactly_once() {
        let token = Address::with_last_byte(1);
        let mut runtime = AutomatedPaperRuntime::new(7, limits());
        let order = runtime
            .prepare_entry(
                token,
                U256::from(500),
                U256::from(5_000),
                10,
                5,
                100,
                conditions(100),
            )
            .unwrap();
        assert_eq!(order.nonce, 7);
        assert!(matches!(
            runtime.observe_boundary(boundary(100)).unwrap().decision,
            BoundaryDecision::Waiting { .. }
        ));
        assert!(matches!(
            runtime.observe_boundary(boundary(101)).unwrap().decision,
            BoundaryDecision::SubmitNow { .. }
        ));
        assert!(matches!(
            runtime.observe_boundary(boundary(101)).unwrap().decision,
            BoundaryDecision::AlreadyTriggered { .. }
        ));
        runtime
            .reconcile_fill(order.id, U256::from(4_900), U256::from(10))
            .unwrap();
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.next_nonce, 8);
        assert_eq!(snapshot.open_exposure, U256::from(500));
        assert_eq!(snapshot.realized_session_loss, U256::from(10));
        assert_eq!(snapshot.positions.len(), 1);
        assert_eq!(snapshot.positions[0].token_amount, U256::from(4_900));
    }

    #[test]
    fn exposure_is_enforced_and_exit_tracks_realized_loss() {
        let first = Address::with_last_byte(1);
        let second = Address::with_last_byte(2);
        let mut runtime = AutomatedPaperRuntime::new(0, limits());
        enter(&mut runtime, first, 1_000);
        assert_eq!(
            runtime.prepare_entry(
                second,
                U256::from(600),
                U256::from(1_000),
                10,
                5,
                100,
                conditions(101),
            ),
            Err(PaperRuntimeError::Exposure)
        );
        let exit = runtime
            .prepare_exit(first, U256::from(900), 10, 5, 100, conditions(102))
            .unwrap();
        runtime.observe_boundary(boundary(103)).unwrap();
        let reconciliation = runtime
            .reconcile_fill(exit.id, U256::from(900), U256::from(10))
            .unwrap();
        assert_eq!(reconciliation.realized_loss, U256::from(110));
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.open_exposure, U256::ZERO);
        assert_eq!(snapshot.realized_session_loss, U256::from(120));
        assert!(snapshot.positions.is_empty());
    }

    #[test]
    fn explicit_rejection_reuses_nonce() {
        let mut runtime = AutomatedPaperRuntime::new(5, limits());
        let first = runtime
            .prepare_entry(
                Address::with_last_byte(1),
                U256::from(100),
                U256::from(1_000),
                10,
                5,
                100,
                conditions(100),
            )
            .unwrap();
        runtime.observe_boundary(boundary(101)).unwrap();
        runtime.reconcile_explicit_rejection(first.id).unwrap();
        let second = runtime
            .prepare_entry(
                Address::with_last_byte(2),
                U256::from(100),
                U256::from(1_000),
                10,
                5,
                100,
                conditions(101),
            )
            .unwrap();
        assert_eq!(second.nonce, 5);
    }

    #[test]
    fn feed_gap_cancels_before_submission_and_reuses_nonce() {
        let mut runtime = AutomatedPaperRuntime::new(5, limits());
        runtime
            .prepare_entry(
                Address::with_last_byte(1),
                U256::from(100),
                U256::from(1_000),
                10,
                5,
                100,
                conditions(100),
            )
            .unwrap();
        let mut unhealthy = boundary(101);
        unhealthy.sequence_contiguous = false;
        let event = runtime.observe_boundary(unhealthy).unwrap();
        assert_eq!(event.state, PaperOrderState::Cancelled);
        let next = runtime
            .prepare_entry(
                Address::with_last_byte(2),
                U256::from(100),
                U256::from(1_000),
                10,
                5,
                100,
                conditions(101),
            )
            .unwrap();
        assert_eq!(next.nonce, 5);
    }

    #[test]
    fn session_loss_halts_entries_but_not_risk_reducing_exit() {
        let token = Address::with_last_byte(1);
        let mut configured = limits();
        configured.max_session_loss = U256::from(10);
        let mut runtime = AutomatedPaperRuntime::new(0, configured);
        enter(&mut runtime, token, 100);
        assert!(runtime.snapshot().entry_halted);
        assert_eq!(
            runtime.prepare_entry(
                Address::with_last_byte(2),
                U256::from(1),
                U256::from(1),
                1,
                1,
                1,
                conditions(101),
            ),
            Err(PaperRuntimeError::LossLimit)
        );
        assert!(
            runtime
                .prepare_exit(token, U256::from(100), 1, 1, 1, conditions(101))
                .is_ok()
        );
    }
}
