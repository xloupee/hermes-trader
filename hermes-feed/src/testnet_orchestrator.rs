use alloy_consensus::transaction::SignerRecoverable;
use alloy_consensus::{SignableTransaction, Transaction, TxEip1559, TxEnvelope};
use alloy_eips::eip2718::{Decodable2718, Encodable2718};
use alloy_primitives::{Address, B256, Signature, TxKind, U256, keccak256};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::{RecoveryId, Signature as K256Signature, SigningKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RiskLedger {
    pub limits: RiskLimits,
    pub realized_session_loss: U256,
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
    #[error("risk accounting overflow")]
    Overflow,
}

impl RiskLedger {
    pub fn new(limits: RiskLimits) -> Self {
        Self {
            limits,
            realized_session_loss: U256::ZERO,
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
        if amount_in > self.limits.max_open_exposure {
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
        Ok(
            if self.realized_session_loss >= self.limits.max_session_loss {
                RiskStatus::LossLimitBreached
            } else {
                RiskStatus::WithinLimits
            },
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct TradePreflightInput {
    pub chain_id: u64,
    pub account: Address,
    pub wrapped_native: Address,
    pub router: Address,
    pub router_code_present: bool,
    pub native_balance: U256,
    pub wrapped_balance: U256,
    pub router_allowance: U256,
    pub amount_in: U256,
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PreflightError {
    #[error("preflight must run on Robinhood Chain testnet 46630")]
    WrongChain,
    #[error("account, wrapped-native token, and router must be non-zero and distinct")]
    InvalidAddress,
    #[error("router has no deployed bytecode")]
    MissingRouterCode,
    #[error("native balance cannot cover maximum gas cost")]
    InsufficientGasBalance,
    #[error("pre-wrapped balance is below the intended input")]
    InsufficientWrappedBalance,
    #[error("router allowance is below the intended input")]
    InsufficientAllowance,
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
    if !input.router_code_present {
        return Err(PreflightError::MissingRouterCode);
    }
    let max_gas_cost = U256::from(input.gas_limit)
        .checked_mul(U256::from(input.max_fee_per_gas))
        .ok_or(PreflightError::Overflow)?;
    if input.native_balance < max_gas_cost {
        return Err(PreflightError::InsufficientGasBalance);
    }
    if input.wrapped_balance < input.amount_in {
        return Err(PreflightError::InsufficientWrappedBalance);
    }
    if input.router_allowance < input.amount_in {
        return Err(PreflightError::InsufficientAllowance);
    }
    Ok(())
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
    fn preflight_requires_wrapped_balance_and_allowance() {
        let mut input = TradePreflightInput {
            chain_id: TESTNET_CHAIN_ID,
            account: Address::with_last_byte(1),
            wrapped_native: Address::with_last_byte(2),
            router: Address::with_last_byte(3),
            router_code_present: true,
            native_balance: U256::from(1_000_000),
            wrapped_balance: U256::from(100),
            router_allowance: U256::from(100),
            amount_in: U256::from(100),
            gas_limit: 1_000,
            max_fee_per_gas: 10,
        };
        assert_eq!(evaluate_testnet_preflight(input), Ok(()));
        input.router_allowance = U256::from(99);
        assert_eq!(
            evaluate_testnet_preflight(input),
            Err(PreflightError::InsufficientAllowance)
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
