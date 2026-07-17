use alloy_consensus::transaction::SignerRecoverable;
use alloy_consensus::{SignableTransaction, Transaction, TxEip1559, TxEnvelope};
use alloy_eips::eip2718::{Decodable2718, Encodable2718};
use alloy_primitives::{Address, B256, Signature, TxKind, U256, keccak256};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::{RecoveryId, Signature as K256Signature, SigningKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::noxa_abi::{V3ExactInputIntent, decode_v3_exact_input_single};
use crate::noxa_trade::PreparedRawTransaction;
use crate::robinhood::TESTNET_CHAIN_ID;
use crate::sequencer::{ConditionalOptions, ConditionalResponse};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum NonceLeaseState {
    Reserved,
    Signed { tx_hash: B256 },
    Submitted { tx_hash: B256 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct NonceLease {
    pub nonce: u64,
    pub state: NonceLeaseState,
}

/// Single-owner nonce state. Only one transaction may be active, which avoids
/// replacement races until explicit multi-nonce reconciliation is implemented.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DedicatedNonceManager {
    next_nonce: u64,
    active: Option<NonceLease>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NonceError {
    #[error("a nonce lease is already active")]
    LeaseActive,
    #[error("nonce overflow")]
    Overflow,
    #[error("nonce lease does not match the active lease")]
    LeaseMismatch,
    #[error("signed transaction hash does not match the nonce lease")]
    HashMismatch,
    #[error("a submitted nonce cannot be released until its hash is reconciled")]
    SubmittedIsAmbiguous,
}

impl DedicatedNonceManager {
    pub fn from_pending_nonce(pending_nonce: u64) -> Self {
        Self {
            next_nonce: pending_nonce,
            active: None,
        }
    }

    pub fn next_nonce(&self) -> u64 {
        self.next_nonce
    }

    pub fn active(&self) -> Option<NonceLease> {
        self.active
    }

    pub fn reserve(&mut self) -> Result<NonceLease, NonceError> {
        if self.active.is_some() {
            return Err(NonceError::LeaseActive);
        }
        let nonce = self.next_nonce;
        self.next_nonce = self.next_nonce.checked_add(1).ok_or(NonceError::Overflow)?;
        let lease = NonceLease {
            nonce,
            state: NonceLeaseState::Reserved,
        };
        self.active = Some(lease);
        Ok(lease)
    }

    pub fn mark_signed(&mut self, nonce: u64, tx_hash: B256) -> Result<(), NonceError> {
        let active = self.active.as_mut().ok_or(NonceError::LeaseMismatch)?;
        if active.nonce != nonce || !matches!(active.state, NonceLeaseState::Reserved) {
            return Err(NonceError::LeaseMismatch);
        }
        active.state = NonceLeaseState::Signed { tx_hash };
        Ok(())
    }

    pub fn mark_submitted(&mut self, nonce: u64, tx_hash: B256) -> Result<(), NonceError> {
        let active = self.active.as_mut().ok_or(NonceError::LeaseMismatch)?;
        if active.nonce != nonce {
            return Err(NonceError::LeaseMismatch);
        }
        match active.state {
            NonceLeaseState::Signed { tx_hash: signed } if signed == tx_hash => {
                active.state = NonceLeaseState::Submitted { tx_hash };
                Ok(())
            }
            NonceLeaseState::Signed { .. } => Err(NonceError::HashMismatch),
            NonceLeaseState::Reserved | NonceLeaseState::Submitted { .. } => {
                Err(NonceError::LeaseMismatch)
            }
        }
    }

    pub fn finalize_included(&mut self, nonce: u64, tx_hash: B256) -> Result<(), NonceError> {
        let active = self.active.ok_or(NonceError::LeaseMismatch)?;
        if active.nonce != nonce {
            return Err(NonceError::LeaseMismatch);
        }
        let active_hash = match active.state {
            NonceLeaseState::Signed { tx_hash } | NonceLeaseState::Submitted { tx_hash } => tx_hash,
            NonceLeaseState::Reserved => return Err(NonceError::LeaseMismatch),
        };
        if active_hash != tx_hash {
            return Err(NonceError::HashMismatch);
        }
        self.active = None;
        Ok(())
    }

    pub fn release_never_submitted(&mut self, nonce: u64) -> Result<(), NonceError> {
        let active = self.active.ok_or(NonceError::LeaseMismatch)?;
        if active.nonce != nonce {
            return Err(NonceError::LeaseMismatch);
        }
        if matches!(active.state, NonceLeaseState::Submitted { .. }) {
            return Err(NonceError::SubmittedIsAmbiguous);
        }
        self.active = None;
        self.next_nonce = nonce;
        Ok(())
    }

    /// Release a signed nonce after a definitive local or sequencer rejection.
    /// The caller must never use this for timeouts, rate limits, malformed
    /// replies, or any other result where bytes may have been accepted.
    pub fn release_explicitly_rejected(
        &mut self,
        nonce: u64,
        tx_hash: B256,
    ) -> Result<(), NonceError> {
        let active = self.active.ok_or(NonceError::LeaseMismatch)?;
        if active.nonce != nonce {
            return Err(NonceError::LeaseMismatch);
        }
        let active_hash = match active.state {
            NonceLeaseState::Signed { tx_hash } | NonceLeaseState::Submitted { tx_hash } => tx_hash,
            NonceLeaseState::Reserved => return Err(NonceError::LeaseMismatch),
        };
        if active_hash != tx_hash {
            return Err(NonceError::HashMismatch);
        }
        self.active = None;
        self.next_nonce = nonce;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct RiskLimits {
    pub max_trade_amount_in: U256,
    pub max_open_exposure: U256,
    pub max_gas_cost_wei: U256,
    pub max_session_loss: U256,
    pub max_slippage_bps: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct RiskReservation {
    pub id: u64,
    pub amount_in: U256,
    pub max_gas_cost: U256,
    #[serde(default)]
    pub reduces_exposure: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RiskLedger {
    pub limits: RiskLimits,
    pub realized_session_loss: U256,
    #[serde(default)]
    pub open_exposure: U256,
    pub active: Option<RiskReservation>,
    next_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskStatus {
    WithinLimits,
    LossLimitBreached,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RiskError {
    #[error("a risk reservation is already active")]
    ReservationActive,
    #[error("trade input is zero or exceeds its limit")]
    TradeAmount,
    #[error("slippage exceeds its limit")]
    Slippage,
    #[error("maximum gas cost overflows or exceeds its limit")]
    GasCost,
    #[error("open exposure would exceed its limit")]
    Exposure,
    #[error("the session loss limit has already been reached")]
    LossLimit,
    #[error("risk reservation does not match")]
    ReservationMismatch,
    #[error("risk reservation has the wrong exposure direction")]
    ReservationDirection,
    #[error("risk accounting overflow")]
    Overflow,
}

impl RiskLedger {
    pub fn new(limits: RiskLimits) -> Self {
        Self {
            limits,
            realized_session_loss: U256::ZERO,
            open_exposure: U256::ZERO,
            active: None,
            next_id: 0,
        }
    }

    pub fn reserve(
        &mut self,
        amount_in: U256,
        gas_limit: u64,
        max_fee_per_gas: u128,
        slippage_bps: u16,
    ) -> Result<RiskReservation, RiskError> {
        if self.active.is_some() {
            return Err(RiskError::ReservationActive);
        }
        if amount_in == U256::ZERO || amount_in > self.limits.max_trade_amount_in {
            return Err(RiskError::TradeAmount);
        }
        if slippage_bps > self.limits.max_slippage_bps {
            return Err(RiskError::Slippage);
        }
        if self.realized_session_loss >= self.limits.max_session_loss {
            return Err(RiskError::LossLimit);
        }
        let projected_exposure = self
            .open_exposure
            .checked_add(amount_in)
            .ok_or(RiskError::Overflow)?;
        if projected_exposure > self.limits.max_open_exposure {
            return Err(RiskError::Exposure);
        }
        let max_gas_cost = U256::from(gas_limit)
            .checked_mul(U256::from(max_fee_per_gas))
            .ok_or(RiskError::GasCost)?;
        if max_gas_cost > self.limits.max_gas_cost_wei {
            return Err(RiskError::GasCost);
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(RiskError::Overflow)?;
        let reservation = RiskReservation {
            id,
            amount_in,
            max_gas_cost,
            reduces_exposure: false,
        };
        self.active = Some(reservation);
        Ok(reservation)
    }

    pub fn reserve_exit(
        &mut self,
        exposure_to_close: U256,
        gas_limit: u64,
        max_fee_per_gas: u128,
        slippage_bps: u16,
    ) -> Result<RiskReservation, RiskError> {
        if self.active.is_some() {
            return Err(RiskError::ReservationActive);
        }
        if exposure_to_close == U256::ZERO || exposure_to_close > self.open_exposure {
            return Err(RiskError::Exposure);
        }
        if slippage_bps > self.limits.max_slippage_bps {
            return Err(RiskError::Slippage);
        }
        let max_gas_cost = U256::from(gas_limit)
            .checked_mul(U256::from(max_fee_per_gas))
            .ok_or(RiskError::GasCost)?;
        if max_gas_cost > self.limits.max_gas_cost_wei {
            return Err(RiskError::GasCost);
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(RiskError::Overflow)?;
        let reservation = RiskReservation {
            id,
            amount_in: exposure_to_close,
            max_gas_cost,
            reduces_exposure: true,
        };
        self.active = Some(reservation);
        Ok(reservation)
    }

    pub fn reserve_maintenance(
        &mut self,
        gas_limit: u64,
        max_fee_per_gas: u128,
    ) -> Result<RiskReservation, RiskError> {
        if self.active.is_some() {
            return Err(RiskError::ReservationActive);
        }
        let max_gas_cost = U256::from(gas_limit)
            .checked_mul(U256::from(max_fee_per_gas))
            .ok_or(RiskError::GasCost)?;
        if max_gas_cost > self.limits.max_gas_cost_wei {
            return Err(RiskError::GasCost);
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(RiskError::Overflow)?;
        let reservation = RiskReservation {
            id,
            amount_in: U256::ZERO,
            max_gas_cost,
            reduces_exposure: false,
        };
        self.active = Some(reservation);
        Ok(reservation)
    }

    pub fn release_unsubmitted(&mut self, id: u64) -> Result<(), RiskError> {
        if self.active.is_none_or(|reservation| reservation.id != id) {
            return Err(RiskError::ReservationMismatch);
        }
        self.active = None;
        Ok(())
    }

    pub fn settle(&mut self, id: u64, realized_loss: U256) -> Result<RiskStatus, RiskError> {
        if self.active.is_none_or(|reservation| reservation.id != id) {
            return Err(RiskError::ReservationMismatch);
        }
        self.realized_session_loss = self
            .realized_session_loss
            .checked_add(realized_loss)
            .ok_or(RiskError::Overflow)?;
        self.active = None;
        Ok(self.status())
    }

    pub fn settle_entry(&mut self, id: u64, realized_loss: U256) -> Result<RiskStatus, RiskError> {
        let reservation = self
            .active
            .filter(|reservation| reservation.id == id)
            .ok_or(RiskError::ReservationMismatch)?;
        if reservation.reduces_exposure {
            return Err(RiskError::ReservationDirection);
        }
        let next_loss = self
            .realized_session_loss
            .checked_add(realized_loss)
            .ok_or(RiskError::Overflow)?;
        let next_exposure = self
            .open_exposure
            .checked_add(reservation.amount_in)
            .ok_or(RiskError::Overflow)?;
        if next_exposure > self.limits.max_open_exposure {
            return Err(RiskError::Exposure);
        }
        self.realized_session_loss = next_loss;
        self.open_exposure = next_exposure;
        self.active = None;
        Ok(self.status())
    }

    pub fn settle_exit(&mut self, id: u64, realized_loss: U256) -> Result<RiskStatus, RiskError> {
        let reservation = self
            .active
            .filter(|reservation| reservation.id == id)
            .ok_or(RiskError::ReservationMismatch)?;
        if !reservation.reduces_exposure {
            return Err(RiskError::ReservationDirection);
        }
        let next_loss = self
            .realized_session_loss
            .checked_add(realized_loss)
            .ok_or(RiskError::Overflow)?;
        let next_exposure = self
            .open_exposure
            .checked_sub(reservation.amount_in)
            .ok_or(RiskError::Exposure)?;
        self.realized_session_loss = next_loss;
        self.open_exposure = next_exposure;
        self.active = None;
        Ok(self.status())
    }

    fn status(&self) -> RiskStatus {
        if self.realized_session_loss >= self.limits.max_session_loss {
            RiskStatus::LossLimitBreached
        } else {
            RiskStatus::WithinLimits
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct TradePreflightInput {
    pub chain_id: u64,
    pub account: Address,
    pub wrapped_native: Address,
    pub router: Address,
    pub wrapped_code_present: bool,
    pub router_code_present: bool,
    pub native_balance: U256,
    pub wrapped_balance: U256,
    pub router_allowance: U256,
    pub amount_in: U256,
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub required_transactions: u64,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PreflightError {
    #[error("preflight must run on Robinhood Chain testnet 46630")]
    WrongChain,
    #[error("account, wrapped-native token, and router must be non-zero and distinct")]
    InvalidAddress,
    #[error("router has no deployed bytecode")]
    MissingRouterCode,
    #[error("wrapped-native token has no deployed bytecode")]
    MissingWrappedNativeCode,
    #[error("native balance cannot cover maximum gas cost")]
    InsufficientGasBalance,
    #[error("pre-wrapped balance is below the intended input")]
    InsufficientWrappedBalance,
    #[error("router allowance must equal the intended input exactly")]
    AllowanceNotExact,
    #[error("required transaction count must be non-zero")]
    InvalidTransactionCount,
    #[error("preflight arithmetic overflow")]
    Overflow,
}

pub fn evaluate_testnet_preflight(input: TradePreflightInput) -> Result<(), PreflightError> {
    if input.chain_id != TESTNET_CHAIN_ID {
        return Err(PreflightError::WrongChain);
    }
    if input.account == Address::ZERO
        || input.wrapped_native == Address::ZERO
        || input.router == Address::ZERO
        || input.wrapped_native == input.router
    {
        return Err(PreflightError::InvalidAddress);
    }
    if !input.wrapped_code_present {
        return Err(PreflightError::MissingWrappedNativeCode);
    }
    if !input.router_code_present {
        return Err(PreflightError::MissingRouterCode);
    }
    if input.required_transactions == 0 {
        return Err(PreflightError::InvalidTransactionCount);
    }
    let max_gas_cost = U256::from(input.gas_limit)
        .checked_mul(U256::from(input.max_fee_per_gas))
        .and_then(|value| value.checked_mul(U256::from(input.required_transactions)))
        .ok_or(PreflightError::Overflow)?;
    if input.native_balance < max_gas_cost {
        return Err(PreflightError::InsufficientGasBalance);
    }
    if input.wrapped_balance < input.amount_in {
        return Err(PreflightError::InsufficientWrappedBalance);
    }
    if input.router_allowance != input.amount_in {
        return Err(PreflightError::AllowanceNotExact);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TestnetRoundTripStepKind {
    Wrap,
    ApproveEntry,
    Entry,
    ApproveExit,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct TestnetRoundTripExpectation {
    pub kind: TestnetRoundTripStepKind,
    pub account: Address,
    pub wrapped_native: Address,
    pub router: Address,
    pub token: Address,
    pub expected_nonce: u64,
    pub exact_amount: U256,
    pub minimum_amount_out: U256,
    pub pool_fee: u32,
    pub maximum_gas_cost: U256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ValidatedTestnetRoundTripStep {
    #[serde(skip)]
    pub raw: Vec<u8>,
    pub kind: TestnetRoundTripStepKind,
    pub hash: B256,
    pub signer: Address,
    pub nonce: u64,
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    pub exact_amount: U256,
    pub minimum_amount_out: U256,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RoundTripStepError {
    #[error("round-trip addresses must be non-zero and mutually distinct")]
    InvalidAddress,
    #[error("round-trip amount, pool fee, and gas cap must be non-zero")]
    InvalidLimit,
    #[error("signed step is not an EIP-1559 transaction on Robinhood testnet 46630")]
    WrongEnvelope,
    #[error("signed step does not belong to the expected account")]
    WrongSigner,
    #[error("signed step nonce does not equal the pending nonce")]
    WrongNonce,
    #[error("signed step gas fields are invalid or exceed the explicit cap")]
    GasLimit,
    #[error("signed step target, value, or calldata does not exactly match the requested stage")]
    UnexpectedTransaction,
    #[error("signed step could not be decoded or recovered")]
    Decode,
}

pub fn validate_signed_testnet_round_trip_step(
    raw: &[u8],
    expected: TestnetRoundTripExpectation,
) -> Result<ValidatedTestnetRoundTripStep, RoundTripStepError> {
    validate_round_trip_expectation(expected)?;
    let envelope = TxEnvelope::decode_2718_exact(raw).map_err(|_| RoundTripStepError::Decode)?;
    if !matches!(envelope, TxEnvelope::Eip1559(_)) || envelope.chain_id() != Some(TESTNET_CHAIN_ID)
    {
        return Err(RoundTripStepError::WrongEnvelope);
    }
    let signer = envelope
        .recover_signer()
        .map_err(|_| RoundTripStepError::Decode)?;
    if signer != expected.account {
        return Err(RoundTripStepError::WrongSigner);
    }
    if envelope.nonce() != expected.expected_nonce {
        return Err(RoundTripStepError::WrongNonce);
    }
    let gas_cost = U256::from(envelope.gas_limit())
        .checked_mul(U256::from(envelope.max_fee_per_gas()))
        .ok_or(RoundTripStepError::GasLimit)?;
    if envelope.gas_limit() == 0
        || envelope.max_fee_per_gas() == 0
        || envelope.max_priority_fee_per_gas().unwrap_or_default() > envelope.max_fee_per_gas()
        || gas_cost > expected.maximum_gas_cost
    {
        return Err(RoundTripStepError::GasLimit);
    }

    let valid = match expected.kind {
        TestnetRoundTripStepKind::Wrap => {
            envelope.to() == Some(expected.wrapped_native)
                && envelope.value() == expected.exact_amount
                && envelope.input().as_ref() == [0xd0, 0xe3, 0x0d, 0xb0]
                && expected.minimum_amount_out == U256::ZERO
        }
        TestnetRoundTripStepKind::ApproveEntry => {
            envelope.to() == Some(expected.wrapped_native)
                && envelope.value() == U256::ZERO
                && decode_exact_approval(envelope.input())
                    == Some((expected.router, expected.exact_amount))
                && expected.minimum_amount_out == U256::ZERO
        }
        TestnetRoundTripStepKind::Entry => {
            envelope.to() == Some(expected.router)
                && envelope.value() == U256::ZERO
                && decode_v3_exact_input_single(envelope.input())
                    == Some(V3ExactInputIntent {
                        token_in: expected.wrapped_native,
                        token_out: expected.token,
                        fee: expected.pool_fee,
                        recipient: expected.account,
                        amount_in: expected.exact_amount,
                        amount_out_minimum: expected.minimum_amount_out,
                        sqrt_price_limit_x96: U256::ZERO,
                    })
        }
        TestnetRoundTripStepKind::ApproveExit => {
            envelope.to() == Some(expected.token)
                && envelope.value() == U256::ZERO
                && decode_exact_approval(envelope.input())
                    == Some((expected.router, expected.exact_amount))
                && expected.minimum_amount_out == U256::ZERO
        }
        TestnetRoundTripStepKind::Exit => {
            envelope.to() == Some(expected.router)
                && envelope.value() == U256::ZERO
                && decode_v3_exact_input_single(envelope.input())
                    == Some(V3ExactInputIntent {
                        token_in: expected.token,
                        token_out: expected.wrapped_native,
                        fee: expected.pool_fee,
                        recipient: expected.account,
                        amount_in: expected.exact_amount,
                        amount_out_minimum: expected.minimum_amount_out,
                        sqrt_price_limit_x96: U256::ZERO,
                    })
        }
    };
    if !valid {
        return Err(RoundTripStepError::UnexpectedTransaction);
    }
    Ok(ValidatedTestnetRoundTripStep {
        raw: raw.to_vec(),
        kind: expected.kind,
        hash: keccak256(raw),
        signer,
        nonce: envelope.nonce(),
        gas_limit: envelope.gas_limit(),
        max_fee_per_gas: envelope.max_fee_per_gas(),
        max_priority_fee_per_gas: envelope.max_priority_fee_per_gas().unwrap_or_default(),
        exact_amount: expected.exact_amount,
        minimum_amount_out: expected.minimum_amount_out,
    })
}

fn validate_round_trip_expectation(
    expected: TestnetRoundTripExpectation,
) -> Result<(), RoundTripStepError> {
    let addresses = [
        expected.account,
        expected.wrapped_native,
        expected.router,
        expected.token,
    ];
    if addresses.contains(&Address::ZERO)
        || addresses
            .iter()
            .enumerate()
            .any(|(index, address)| addresses[index + 1..].contains(address))
    {
        return Err(RoundTripStepError::InvalidAddress);
    }
    if expected.exact_amount == U256::ZERO
        || expected.pool_fee == 0
        || expected.maximum_gas_cost == U256::ZERO
        || matches!(
            expected.kind,
            TestnetRoundTripStepKind::Entry | TestnetRoundTripStepKind::Exit
        ) && expected.minimum_amount_out == U256::ZERO
    {
        return Err(RoundTripStepError::InvalidLimit);
    }
    Ok(())
}

fn decode_exact_approval(input: &[u8]) -> Option<(Address, U256)> {
    if input.len() != 68 || input[..4] != [0x09, 0x5e, 0xa7, 0xb3] || input[4..16] != [0_u8; 12] {
        return None;
    }
    Some((
        Address::from_slice(&input[16..36]),
        U256::from_be_slice(&input[36..68]),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct TestnetRoundTripAccountState {
    pub pending_nonce: u64,
    pub wrapped_balance: U256,
    pub token_balance: U256,
    pub wrapped_allowance: U256,
    pub token_allowance: U256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct TestnetRoundTripReconciliationInput {
    pub kind: TestnetRoundTripStepKind,
    pub exact_amount: U256,
    pub receipt_status: bool,
    pub before: TestnetRoundTripAccountState,
    pub after: TestnetRoundTripAccountState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct TestnetRoundTripReconciliation {
    pub kind: TestnetRoundTripStepKind,
    pub acquired_token: U256,
    pub received_wrapped: U256,
    pub open_token_exposure: U256,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RoundTripReconciliationError {
    #[error("testnet round-trip receipt was absent or reverted")]
    ReceiptFailed,
    #[error("pending nonce did not advance exactly once")]
    NonceMismatch,
    #[error("token balances do not prove the requested round-trip stage")]
    BalanceMismatch,
    #[error("router allowances do not prove an exact capped approval or consumption")]
    AllowanceMismatch,
    #[error("round-trip reconciliation arithmetic overflow")]
    Overflow,
}

pub fn reconcile_testnet_round_trip_step(
    input: TestnetRoundTripReconciliationInput,
) -> Result<TestnetRoundTripReconciliation, RoundTripReconciliationError> {
    if !input.receipt_status {
        return Err(RoundTripReconciliationError::ReceiptFailed);
    }
    if input.after.pending_nonce
        != input
            .before
            .pending_nonce
            .checked_add(1)
            .ok_or(RoundTripReconciliationError::Overflow)?
    {
        return Err(RoundTripReconciliationError::NonceMismatch);
    }
    let mut acquired_token = U256::ZERO;
    let mut received_wrapped = U256::ZERO;
    match input.kind {
        TestnetRoundTripStepKind::Wrap => {
            if input.after.wrapped_balance
                != input
                    .before
                    .wrapped_balance
                    .checked_add(input.exact_amount)
                    .ok_or(RoundTripReconciliationError::Overflow)?
                || input.after.token_balance != input.before.token_balance
            {
                return Err(RoundTripReconciliationError::BalanceMismatch);
            }
        }
        TestnetRoundTripStepKind::ApproveEntry => {
            if input.after.wrapped_balance != input.before.wrapped_balance
                || input.after.token_balance != input.before.token_balance
            {
                return Err(RoundTripReconciliationError::BalanceMismatch);
            }
            if input.after.wrapped_allowance != input.exact_amount {
                return Err(RoundTripReconciliationError::AllowanceMismatch);
            }
        }
        TestnetRoundTripStepKind::Entry => {
            if input.before.wrapped_allowance != input.exact_amount
                || input.after.wrapped_allowance != U256::ZERO
            {
                return Err(RoundTripReconciliationError::AllowanceMismatch);
            }
            if input.after.wrapped_balance
                != input
                    .before
                    .wrapped_balance
                    .checked_sub(input.exact_amount)
                    .ok_or(RoundTripReconciliationError::BalanceMismatch)?
                || input.after.token_balance <= input.before.token_balance
            {
                return Err(RoundTripReconciliationError::BalanceMismatch);
            }
            acquired_token = input
                .after
                .token_balance
                .checked_sub(input.before.token_balance)
                .ok_or(RoundTripReconciliationError::BalanceMismatch)?;
        }
        TestnetRoundTripStepKind::ApproveExit => {
            if input.before.token_balance != input.exact_amount
                || input.after.wrapped_balance != input.before.wrapped_balance
                || input.after.token_balance != input.before.token_balance
            {
                return Err(RoundTripReconciliationError::BalanceMismatch);
            }
            if input.after.token_allowance != input.exact_amount {
                return Err(RoundTripReconciliationError::AllowanceMismatch);
            }
        }
        TestnetRoundTripStepKind::Exit => {
            if input.before.token_balance != input.exact_amount
                || input.before.token_allowance != input.exact_amount
                || input.after.token_allowance != U256::ZERO
            {
                return Err(RoundTripReconciliationError::AllowanceMismatch);
            }
            if input.after.token_balance != U256::ZERO
                || input.after.wrapped_balance <= input.before.wrapped_balance
            {
                return Err(RoundTripReconciliationError::BalanceMismatch);
            }
            received_wrapped = input
                .after
                .wrapped_balance
                .checked_sub(input.before.wrapped_balance)
                .ok_or(RoundTripReconciliationError::BalanceMismatch)?;
        }
    }
    Ok(TestnetRoundTripReconciliation {
        kind: input.kind,
        acquired_token,
        received_wrapped,
        open_token_exposure: input.after.token_balance,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct TestnetCanaryPlan {
    pub nonce: u64,
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    pub account: Address,
    pub value: U256,
    pub maximum_value: U256,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CanaryError {
    #[error("canary account must be non-zero")]
    InvalidAccount,
    #[error("canary gas and fee fields are invalid")]
    InvalidGas,
    #[error("canary value exceeds its explicit maximum")]
    ValueLimit,
    #[error("signed canary is not pinned to Robinhood testnet")]
    WrongChain,
    #[error("signed canary must be a data-free self-transfer")]
    NotSelfTransfer,
    #[error("canary signing failed")]
    Signing,
    #[error("canary signer does not match the self-transfer account")]
    SignerMismatch,
    #[error("signed canary failed round-trip validation")]
    RoundTrip,
}

/// Metadata recovered from externally signed bytes after enforcing the
/// deliberately narrow testnet canary envelope.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ValidatedTestnetCanary {
    #[serde(skip)]
    pub raw: Vec<u8>,
    pub hash: B256,
    pub signer: Address,
    pub nonce: u64,
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    pub value: U256,
}

pub fn validate_signed_testnet_canary(
    raw: &[u8],
    maximum_value: U256,
) -> Result<ValidatedTestnetCanary, CanaryError> {
    let envelope = TxEnvelope::decode_2718_exact(raw).map_err(|_| CanaryError::RoundTrip)?;
    if envelope.chain_id() != Some(TESTNET_CHAIN_ID) {
        return Err(CanaryError::WrongChain);
    }
    let signer = envelope
        .recover_signer()
        .map_err(|_| CanaryError::RoundTrip)?;
    if envelope.to() != Some(signer) || !envelope.input().is_empty() {
        return Err(CanaryError::NotSelfTransfer);
    }
    if envelope.value() > maximum_value {
        return Err(CanaryError::ValueLimit);
    }
    Ok(ValidatedTestnetCanary {
        raw: raw.to_vec(),
        hash: keccak256(raw),
        signer,
        nonce: envelope.nonce(),
        gas_limit: envelope.gas_limit(),
        max_fee_per_gas: envelope.max_fee_per_gas(),
        max_priority_fee_per_gas: envelope.max_priority_fee_per_gas().unwrap_or_default(),
        value: envelope.value(),
    })
}

impl TestnetCanaryPlan {
    pub fn validate(&self) -> Result<(), CanaryError> {
        if self.account == Address::ZERO {
            return Err(CanaryError::InvalidAccount);
        }
        if self.gas_limit < 21_000
            || self.max_fee_per_gas == 0
            || self.max_priority_fee_per_gas > self.max_fee_per_gas
        {
            return Err(CanaryError::InvalidGas);
        }
        if self.value > self.maximum_value {
            return Err(CanaryError::ValueLimit);
        }
        Ok(())
    }

    /// Pre-sign a testnet-only self-transfer. No key loading or persistence is
    /// performed here; callers must inject a throwaway in-memory key.
    pub fn sign(&self, signing_key: &SigningKey) -> Result<PreparedRawTransaction, CanaryError> {
        self.validate()?;
        let transaction = TxEip1559 {
            chain_id: TESTNET_CHAIN_ID,
            nonce: self.nonce,
            gas_limit: self.gas_limit,
            max_fee_per_gas: self.max_fee_per_gas,
            max_priority_fee_per_gas: self.max_priority_fee_per_gas,
            to: TxKind::Call(self.account),
            value: self.value,
            access_list: Default::default(),
            input: Default::default(),
        };
        let (signature, recovery_id): (K256Signature, RecoveryId) = signing_key
            .sign_prehash(transaction.signature_hash().as_slice())
            .map_err(|_| CanaryError::Signing)?;
        let signature: Signature = (signature, recovery_id).into();
        let envelope: TxEnvelope = transaction.into_signed(signature).into();
        let raw = envelope.encoded_2718();
        let decoded = TxEnvelope::decode_2718_exact(&raw).map_err(|_| CanaryError::RoundTrip)?;
        let signer = decoded
            .recover_signer()
            .map_err(|_| CanaryError::RoundTrip)?;
        if signer != self.account {
            return Err(CanaryError::SignerMismatch);
        }
        Ok(PreparedRawTransaction {
            hash: keccak256(&raw),
            raw,
            signer,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct ConditionalRetryState {
    pub expected_tx_hash: B256,
    pub conditions: ConditionalOptions,
    pub attempts: u16,
    pub max_boundary_attempts: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum ConditionalRetryDecision {
    Complete { tx_hash: B256 },
    RetrySameBytes,
    ReconcileByHash { tx_hash: B256, reason: String },
    Failed { reason: String },
}

impl ConditionalRetryState {
    pub fn on_response(&mut self, response: ConditionalResponse) -> ConditionalRetryDecision {
        self.attempts = self.attempts.saturating_add(1);
        match response {
            ConditionalResponse::Accepted { tx_hash }
            | ConditionalResponse::AlreadyKnown { tx_hash, .. }
                if tx_hash == self.expected_tx_hash =>
            {
                ConditionalRetryDecision::Complete { tx_hash }
            }
            ConditionalResponse::BoundaryNotReached { .. }
                if self.attempts < self.max_boundary_attempts =>
            {
                ConditionalRetryDecision::RetrySameBytes
            }
            ConditionalResponse::BoundaryNotReached { message } => {
                ConditionalRetryDecision::Failed { reason: message }
            }
            ConditionalResponse::RateLimited { message }
            | ConditionalResponse::InvalidResponse(message) => {
                ConditionalRetryDecision::ReconcileByHash {
                    tx_hash: self.expected_tx_hash,
                    reason: message,
                }
            }
            ConditionalResponse::Rejected { code, message } => ConditionalRetryDecision::Failed {
                reason: format!("JSON-RPC {code}: {message}"),
            },
            ConditionalResponse::Accepted { tx_hash }
            | ConditionalResponse::AlreadyKnown { tx_hash, .. } => {
                ConditionalRetryDecision::ReconcileByHash {
                    tx_hash: self.expected_tx_hash,
                    reason: format!("sequencer returned mismatched hash {tx_hash}"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::Address;

    use super::*;

    fn sign_testnet_transaction(
        key: &SigningKey,
        chain_id: u64,
        nonce: u64,
        to: Address,
        value: U256,
        input: Vec<u8>,
    ) -> Vec<u8> {
        let transaction = TxEip1559 {
            chain_id,
            nonce,
            gas_limit: 300_000,
            max_fee_per_gas: 100_000_000,
            max_priority_fee_per_gas: 0,
            to: TxKind::Call(to),
            value,
            access_list: Default::default(),
            input: input.into(),
        };
        let (signature, recovery_id): (K256Signature, RecoveryId) = key
            .sign_prehash(transaction.signature_hash().as_slice())
            .unwrap();
        let signature: Signature = (signature, recovery_id).into();
        TxEnvelope::from(transaction.into_signed(signature)).encoded_2718()
    }

    fn approval_input(spender: Address, amount: U256) -> Vec<u8> {
        let mut input = vec![0x09, 0x5e, 0xa7, 0xb3];
        input.extend_from_slice(&[0_u8; 12]);
        input.extend_from_slice(spender.as_slice());
        input.extend_from_slice(&amount.to_be_bytes::<32>());
        input
    }

    #[test]
    fn validates_every_exact_testnet_round_trip_stage() {
        let key = SigningKey::from_slice(&[6_u8; 32]).unwrap();
        let account = Address::from_private_key(&key);
        let wrapped_native = Address::with_last_byte(2);
        let router = Address::with_last_byte(3);
        let token = Address::with_last_byte(4);
        let amount = U256::from(100);
        let cases = [
            (
                TestnetRoundTripStepKind::Wrap,
                wrapped_native,
                amount,
                vec![0xd0, 0xe3, 0x0d, 0xb0],
                U256::ZERO,
            ),
            (
                TestnetRoundTripStepKind::ApproveEntry,
                wrapped_native,
                U256::ZERO,
                approval_input(router, amount),
                U256::ZERO,
            ),
            (
                TestnetRoundTripStepKind::Entry,
                router,
                U256::ZERO,
                crate::noxa_abi::encode_v3_exact_input_single(&V3ExactInputIntent {
                    token_in: wrapped_native,
                    token_out: token,
                    fee: 10_000,
                    recipient: account,
                    amount_in: amount,
                    amount_out_minimum: U256::from(90),
                    sqrt_price_limit_x96: U256::ZERO,
                })
                .unwrap(),
                U256::from(90),
            ),
            (
                TestnetRoundTripStepKind::ApproveExit,
                token,
                U256::ZERO,
                approval_input(router, amount),
                U256::ZERO,
            ),
            (
                TestnetRoundTripStepKind::Exit,
                router,
                U256::ZERO,
                crate::noxa_abi::encode_v3_exact_input_single(&V3ExactInputIntent {
                    token_in: token,
                    token_out: wrapped_native,
                    fee: 10_000,
                    recipient: account,
                    amount_in: amount,
                    amount_out_minimum: U256::from(80),
                    sqrt_price_limit_x96: U256::ZERO,
                })
                .unwrap(),
                U256::from(80),
            ),
        ];
        for (offset, (kind, to, value, input, minimum_amount_out)) in cases.into_iter().enumerate()
        {
            let nonce = 12 + u64::try_from(offset).unwrap();
            let raw = sign_testnet_transaction(&key, TESTNET_CHAIN_ID, nonce, to, value, input);
            let validated = validate_signed_testnet_round_trip_step(
                &raw,
                TestnetRoundTripExpectation {
                    kind,
                    account,
                    wrapped_native,
                    router,
                    token,
                    expected_nonce: nonce,
                    exact_amount: amount,
                    minimum_amount_out,
                    pool_fee: 10_000,
                    maximum_gas_cost: U256::from(30_000_000_000_000_u64),
                },
            )
            .unwrap();
            assert_eq!(validated.kind, kind);
            assert_eq!(validated.nonce, nonce);
            assert_eq!(validated.hash, keccak256(&raw));
        }
    }

    #[test]
    fn rejects_overapproval_wrong_nonce_chain_and_gas_cap() {
        let key = SigningKey::from_slice(&[6_u8; 32]).unwrap();
        let expected = TestnetRoundTripExpectation {
            kind: TestnetRoundTripStepKind::ApproveEntry,
            account: Address::from_private_key(&key),
            wrapped_native: Address::with_last_byte(2),
            router: Address::with_last_byte(3),
            token: Address::with_last_byte(4),
            expected_nonce: 12,
            exact_amount: U256::from(100),
            minimum_amount_out: U256::ZERO,
            pool_fee: 10_000,
            maximum_gas_cost: U256::from(30_000_000_000_000_u64),
        };
        let raw = sign_testnet_transaction(
            &key,
            TESTNET_CHAIN_ID,
            12,
            expected.wrapped_native,
            U256::ZERO,
            approval_input(expected.router, U256::from(101)),
        );
        assert_eq!(
            validate_signed_testnet_round_trip_step(&raw, expected),
            Err(RoundTripStepError::UnexpectedTransaction)
        );

        let exact_raw = sign_testnet_transaction(
            &key,
            TESTNET_CHAIN_ID,
            12,
            expected.wrapped_native,
            U256::ZERO,
            approval_input(expected.router, expected.exact_amount),
        );
        let mut wrong_nonce = expected;
        wrong_nonce.expected_nonce = 13;
        assert_eq!(
            validate_signed_testnet_round_trip_step(&exact_raw, wrong_nonce),
            Err(RoundTripStepError::WrongNonce)
        );
        let wrong_chain = sign_testnet_transaction(
            &key,
            4663,
            12,
            expected.wrapped_native,
            U256::ZERO,
            approval_input(expected.router, expected.exact_amount),
        );
        assert_eq!(
            validate_signed_testnet_round_trip_step(&wrong_chain, expected),
            Err(RoundTripStepError::WrongEnvelope)
        );
        let mut low_gas_cap = expected;
        low_gas_cap.maximum_gas_cost = U256::from(1);
        assert_eq!(
            validate_signed_testnet_round_trip_step(&exact_raw, low_gas_cap),
            Err(RoundTripStepError::GasLimit)
        );
    }

    #[test]
    fn reconciles_wrap_entry_and_full_exit_to_zero_exposure() {
        let state =
            |pending_nonce, wrapped_balance, token_balance, wrapped_allowance, token_allowance| {
                TestnetRoundTripAccountState {
                    pending_nonce,
                    wrapped_balance: U256::from(wrapped_balance),
                    token_balance: U256::from(token_balance),
                    wrapped_allowance: U256::from(wrapped_allowance),
                    token_allowance: U256::from(token_allowance),
                }
            };
        let cases = [
            (
                TestnetRoundTripStepKind::Wrap,
                100,
                state(12, 0, 0, 0, 0),
                state(13, 100, 0, 0, 0),
            ),
            (
                TestnetRoundTripStepKind::ApproveEntry,
                100,
                state(13, 100, 0, 0, 0),
                state(14, 100, 0, 100, 0),
            ),
            (
                TestnetRoundTripStepKind::Entry,
                100,
                state(14, 100, 0, 100, 0),
                state(15, 0, 900, 0, 0),
            ),
            (
                TestnetRoundTripStepKind::ApproveExit,
                900,
                state(15, 0, 900, 0, 0),
                state(16, 0, 900, 0, 900),
            ),
            (
                TestnetRoundTripStepKind::Exit,
                900,
                state(16, 0, 900, 0, 900),
                state(17, 95, 0, 0, 0),
            ),
        ];
        let mut final_outcome = None;
        for (kind, exact_amount, before, after) in cases {
            final_outcome = Some(
                reconcile_testnet_round_trip_step(TestnetRoundTripReconciliationInput {
                    kind,
                    exact_amount: U256::from(exact_amount),
                    receipt_status: true,
                    before,
                    after,
                })
                .unwrap(),
            );
        }
        let final_outcome = final_outcome.unwrap();
        assert_eq!(final_outcome.received_wrapped, U256::from(95));
        assert_eq!(final_outcome.open_token_exposure, U256::ZERO);
    }

    #[test]
    fn reconciliation_rejects_revert_nonce_drift_and_partial_exit() {
        let before = TestnetRoundTripAccountState {
            pending_nonce: 20,
            wrapped_balance: U256::ZERO,
            token_balance: U256::from(500),
            wrapped_allowance: U256::ZERO,
            token_allowance: U256::from(500),
        };
        let mut input = TestnetRoundTripReconciliationInput {
            kind: TestnetRoundTripStepKind::Exit,
            exact_amount: U256::from(500),
            receipt_status: false,
            before,
            after: TestnetRoundTripAccountState {
                pending_nonce: 21,
                wrapped_balance: U256::from(50),
                token_balance: U256::ZERO,
                wrapped_allowance: U256::ZERO,
                token_allowance: U256::ZERO,
            },
        };
        assert_eq!(
            reconcile_testnet_round_trip_step(input),
            Err(RoundTripReconciliationError::ReceiptFailed)
        );
        input.receipt_status = true;
        input.after.pending_nonce = 22;
        assert_eq!(
            reconcile_testnet_round_trip_step(input),
            Err(RoundTripReconciliationError::NonceMismatch)
        );
        input.after.pending_nonce = 21;
        input.after.token_balance = U256::from(1);
        assert_eq!(
            reconcile_testnet_round_trip_step(input),
            Err(RoundTripReconciliationError::BalanceMismatch)
        );
    }

    #[test]
    fn nonce_manager_never_releases_an_ambiguous_submission() {
        let hash = B256::with_last_byte(7);
        let mut manager = DedicatedNonceManager::from_pending_nonce(12);
        let lease = manager.reserve().unwrap();
        manager.mark_signed(lease.nonce, hash).unwrap();
        manager.mark_submitted(lease.nonce, hash).unwrap();
        assert_eq!(
            manager.release_never_submitted(lease.nonce),
            Err(NonceError::SubmittedIsAmbiguous)
        );
        manager.finalize_included(lease.nonce, hash).unwrap();
        assert!(manager.active().is_none());
        assert_eq!(manager.next_nonce(), 13);
    }

    #[test]
    fn unbroadcast_nonce_can_be_reused() {
        let mut manager = DedicatedNonceManager::from_pending_nonce(4);
        let lease = manager.reserve().unwrap();
        manager.release_never_submitted(lease.nonce).unwrap();
        assert_eq!(manager.reserve().unwrap().nonce, 4);
    }

    #[test]
    fn explicitly_rejected_submission_can_reuse_its_nonce() {
        let hash = B256::with_last_byte(9);
        let mut manager = DedicatedNonceManager::from_pending_nonce(4);
        let lease = manager.reserve().unwrap();
        manager.mark_signed(lease.nonce, hash).unwrap();
        manager.mark_submitted(lease.nonce, hash).unwrap();
        manager
            .release_explicitly_rejected(lease.nonce, hash)
            .unwrap();
        assert_eq!(manager.reserve().unwrap().nonce, 4);
    }

    #[test]
    fn risk_limits_reserve_and_latch_session_loss() {
        let mut ledger = RiskLedger::new(RiskLimits {
            max_trade_amount_in: U256::from(1_000),
            max_open_exposure: U256::from(1_000),
            max_gas_cost_wei: U256::from(100_000),
            max_session_loss: U256::from(100),
            max_slippage_bps: 500,
        });
        let reservation = ledger.reserve(U256::from(500), 1_000, 10, 100).unwrap();
        assert_eq!(
            ledger.settle(reservation.id, U256::from(100)).unwrap(),
            RiskStatus::LossLimitBreached
        );
        assert!(matches!(
            ledger.reserve(U256::from(1), 1, 1, 1),
            Err(RiskError::LossLimit)
        ));
    }

    #[test]
    fn risk_limits_enforce_cumulative_open_exposure() {
        let mut ledger = RiskLedger::new(RiskLimits {
            max_trade_amount_in: U256::from(1_000),
            max_open_exposure: U256::from(750),
            max_gas_cost_wei: U256::from(100_000),
            max_session_loss: U256::from(100),
            max_slippage_bps: 500,
        });
        let first = ledger.reserve(U256::from(500), 1_000, 10, 100).unwrap();
        ledger.settle_entry(first.id, U256::ZERO).unwrap();
        assert_eq!(ledger.open_exposure, U256::from(500));
        assert_eq!(
            ledger.reserve(U256::from(251), 1_000, 10, 100),
            Err(RiskError::Exposure)
        );
        let second = ledger.reserve(U256::from(250), 1_000, 10, 100).unwrap();
        ledger.settle(second.id, U256::from(1)).unwrap();
        assert_eq!(ledger.open_exposure, U256::from(500));
        let exit = ledger
            .reserve_exit(U256::from(500), 1_000, 10, 100)
            .unwrap();
        ledger.settle_exit(exit.id, U256::from(1)).unwrap();
        assert_eq!(ledger.open_exposure, U256::ZERO);
    }

    #[test]
    fn preflight_requires_wrapped_balance_and_allowance() {
        let mut input = TradePreflightInput {
            chain_id: TESTNET_CHAIN_ID,
            account: Address::with_last_byte(1),
            wrapped_native: Address::with_last_byte(2),
            router: Address::with_last_byte(3),
            wrapped_code_present: true,
            router_code_present: true,
            native_balance: U256::from(1_000_000),
            wrapped_balance: U256::from(100),
            router_allowance: U256::from(100),
            amount_in: U256::from(100),
            gas_limit: 1_000,
            max_fee_per_gas: 10,
            required_transactions: 3,
        };
        assert_eq!(evaluate_testnet_preflight(input), Ok(()));

        input.wrapped_code_present = false;
        assert_eq!(
            evaluate_testnet_preflight(input),
            Err(PreflightError::MissingWrappedNativeCode)
        );
        input.wrapped_code_present = true;

        input.router_allowance = U256::from(99);
        assert_eq!(
            evaluate_testnet_preflight(input),
            Err(PreflightError::AllowanceNotExact)
        );

        input.router_allowance = U256::from(101);
        assert_eq!(
            evaluate_testnet_preflight(input),
            Err(PreflightError::AllowanceNotExact)
        );

        input.router_allowance = input.amount_in;
        input.native_balance = U256::from(29_999);
        assert_eq!(
            evaluate_testnet_preflight(input),
            Err(PreflightError::InsufficientGasBalance)
        );

        input.native_balance = U256::from(1_000_000);
        input.required_transactions = 0;
        assert_eq!(
            evaluate_testnet_preflight(input),
            Err(PreflightError::InvalidTransactionCount)
        );
    }

    #[test]
    fn canary_is_chain_pinned_and_signer_bound() {
        let key = SigningKey::from_slice(&[9_u8; 32]).unwrap();
        let account = Address::from_private_key(&key);
        let plan = TestnetCanaryPlan {
            nonce: 2,
            gas_limit: 21_000,
            max_fee_per_gas: 100,
            max_priority_fee_per_gas: 0,
            account,
            value: U256::from(1),
            maximum_value: U256::from(1),
        };
        let prepared = plan.sign(&key).unwrap();
        assert_eq!(prepared.signer, account);
        assert_eq!(prepared.hash, keccak256(&prepared.raw));
        let validated = validate_signed_testnet_canary(&prepared.raw, U256::from(1)).unwrap();
        assert_eq!(validated.signer, account);
        assert_eq!(validated.nonce, 2);
        assert_eq!(validated.value, U256::from(1));
    }

    #[test]
    fn conditional_retry_only_retries_explicit_early_boundary() {
        let hash = B256::with_last_byte(4);
        let mut state = ConditionalRetryState {
            expected_tx_hash: hash,
            conditions: ConditionalOptions::first_eligible_window(100, 3, None).unwrap(),
            attempts: 0,
            max_boundary_attempts: 2,
        };
        assert_eq!(
            state.on_response(ConditionalResponse::BoundaryNotReached {
                message: "early".into(),
            }),
            ConditionalRetryDecision::RetrySameBytes
        );
        assert!(matches!(
            state.on_response(ConditionalResponse::RateLimited {
                message: "slow down".into(),
            }),
            ConditionalRetryDecision::ReconcileByHash { tx_hash, .. } if tx_hash == hash
        ));
    }
}
