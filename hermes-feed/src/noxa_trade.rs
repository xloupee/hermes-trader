use alloy_consensus::transaction::SignerRecoverable;
use alloy_consensus::{SignableTransaction, TxEip1559, TxEnvelope};
use alloy_eips::eip2718::{Decodable2718, Encodable2718};
use alloy_primitives::{Address, Bytes, Signature, TxKind, U256, keccak256};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::{RecoveryId, Signature as K256Signature, SigningKey};
use serde::Serialize;
use thiserror::Error;

use crate::noxa_abi::{
    EXACT_INPUT_SINGLE_SELECTOR, EXACT_OUTPUT_SINGLE_SELECTOR, V3ExactInputIntent,
    V3ExactOutputIntent, decode_v3_exact_input_single, decode_v3_exact_output_single,
    encode_v3_exact_input_single, encode_v3_exact_output_single,
};
use crate::robinhood::{CHAIN_ID, NOXA_POOL_FEE, UNISWAP_V3_SWAP_ROUTER_02, WETH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TradeTransactionPlan {
    pub chain_id: u64,
    pub nonce: u64,
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    pub to: Address,
    pub value: U256,
    pub expected_token_out: Address,
    pub expected_recipient: Address,
    pub calldata: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRawTransaction {
    pub raw: Vec<u8>,
    pub hash: alloy_primitives::B256,
    pub signer: Address,
}

#[derive(Debug, Error)]
pub enum TradePlanError {
    #[error("trade plan must target Robinhood chain ID 4663")]
    WrongChain,
    #[error("trade plan must call canonical SwapRouter02")]
    WrongRouter,
    #[error("trade plan must use pre-wrapped WETH and zero native value")]
    NonZeroValue,
    #[error("trade plan calldata is not a supported direct V3 single-hop method")]
    UnsupportedCalldata,
    #[error("trade plan calldata violates the pinned NOXA single-hop invariants")]
    UnsafeSwapParameters,
    #[error("gas limit and max fee must be non-zero")]
    InvalidGas,
    #[error("priority fee cannot exceed max fee")]
    InvalidFee,
    #[error("could not sign EIP-1559 prehash")]
    Signing,
    #[error("signed transaction failed round-trip validation")]
    RoundTrip,
    #[error("router recipient must equal the transaction signer")]
    RecipientSignerMismatch,
}

impl TradeTransactionPlan {
    pub fn exact_input(
        nonce: u64,
        gas_limit: u64,
        max_fee_per_gas: u128,
        max_priority_fee_per_gas: u128,
        intent: &V3ExactInputIntent,
    ) -> Result<Self, TradePlanError> {
        let calldata =
            encode_v3_exact_input_single(intent).ok_or(TradePlanError::UnsafeSwapParameters)?;
        Self::direct_v3(
            nonce,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            intent.token_out,
            intent.recipient,
            calldata,
        )
    }

    pub fn exact_output(
        nonce: u64,
        gas_limit: u64,
        max_fee_per_gas: u128,
        max_priority_fee_per_gas: u128,
        intent: &V3ExactOutputIntent,
    ) -> Result<Self, TradePlanError> {
        let calldata =
            encode_v3_exact_output_single(intent).ok_or(TradePlanError::UnsafeSwapParameters)?;
        Self::direct_v3(
            nonce,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            intent.token_out,
            intent.recipient,
            calldata,
        )
    }

    fn direct_v3(
        nonce: u64,
        gas_limit: u64,
        max_fee_per_gas: u128,
        max_priority_fee_per_gas: u128,
        expected_token_out: Address,
        expected_recipient: Address,
        calldata: Vec<u8>,
    ) -> Result<Self, TradePlanError> {
        let plan = Self {
            chain_id: CHAIN_ID,
            nonce,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            to: UNISWAP_V3_SWAP_ROUTER_02,
            value: U256::ZERO,
            expected_token_out,
            expected_recipient,
            calldata: calldata.into(),
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), TradePlanError> {
        if self.chain_id != CHAIN_ID {
            return Err(TradePlanError::WrongChain);
        }
        if self.to != UNISWAP_V3_SWAP_ROUTER_02 {
            return Err(TradePlanError::WrongRouter);
        }
        if self.value != U256::ZERO {
            return Err(TradePlanError::NonZeroValue);
        }
        let selector = self
            .calldata
            .get(..4)
            .ok_or(TradePlanError::UnsupportedCalldata)?;
        if self.expected_token_out == Address::ZERO
            || self.expected_token_out == WETH
            || self.expected_recipient == Address::ZERO
        {
            return Err(TradePlanError::UnsafeSwapParameters);
        }
        if selector == EXACT_INPUT_SINGLE_SELECTOR {
            let intent = decode_v3_exact_input_single(&self.calldata)
                .ok_or(TradePlanError::UnsupportedCalldata)?;
            if intent.token_in != WETH
                || intent.token_out != self.expected_token_out
                || intent.recipient != self.expected_recipient
                || intent.fee != NOXA_POOL_FEE
                || intent.amount_in == U256::ZERO
                || intent.amount_out_minimum == U256::ZERO
            {
                return Err(TradePlanError::UnsafeSwapParameters);
            }
        } else if selector == EXACT_OUTPUT_SINGLE_SELECTOR {
            let intent = decode_v3_exact_output_single(&self.calldata)
                .ok_or(TradePlanError::UnsupportedCalldata)?;
            if intent.token_in != WETH
                || intent.token_out != self.expected_token_out
                || intent.recipient != self.expected_recipient
                || intent.fee != NOXA_POOL_FEE
                || intent.amount_out == U256::ZERO
                || intent.amount_in_maximum == U256::ZERO
            {
                return Err(TradePlanError::UnsafeSwapParameters);
            }
        } else {
            return Err(TradePlanError::UnsupportedCalldata);
        }
        if self.gas_limit == 0 || self.max_fee_per_gas == 0 {
            return Err(TradePlanError::InvalidGas);
        }
        if self.max_priority_fee_per_gas > self.max_fee_per_gas {
            return Err(TradePlanError::InvalidFee);
        }
        Ok(())
    }

    pub fn unsigned_transaction(&self) -> Result<TxEip1559, TradePlanError> {
        self.validate()?;
        Ok(TxEip1559 {
            chain_id: self.chain_id,
            nonce: self.nonce,
            gas_limit: self.gas_limit,
            max_fee_per_gas: self.max_fee_per_gas,
            max_priority_fee_per_gas: self.max_priority_fee_per_gas,
            to: TxKind::Call(self.to),
            value: self.value,
            access_list: Default::default(),
            input: self.calldata.clone(),
        })
    }

    /// Sign a fully prepared plan in memory. Production key loading is
    /// deliberately outside this crate and the `hermes-noxa` CLI exposes no
    /// signing command.
    pub fn sign(&self, signing_key: &SigningKey) -> Result<PreparedRawTransaction, TradePlanError> {
        let transaction = self.unsigned_transaction()?;
        let (signature, recovery_id): (K256Signature, RecoveryId) = signing_key
            .sign_prehash(transaction.signature_hash().as_slice())
            .map_err(|_| TradePlanError::Signing)?;
        let signature: Signature = (signature, recovery_id).into();
        let envelope: TxEnvelope = transaction.into_signed(signature).into();
        let raw = envelope.encoded_2718();
        let decoded = TxEnvelope::decode_2718_exact(&raw).map_err(|_| TradePlanError::RoundTrip)?;
        let signer = decoded
            .recover_signer()
            .map_err(|_| TradePlanError::RoundTrip)?;
        if signer != self.expected_recipient {
            return Err(TradePlanError::RecipientSignerMismatch);
        }
        Ok(PreparedRawTransaction {
            hash: keccak256(&raw),
            raw,
            signer,
        })
    }
}

#[cfg(test)]
mod tests {
    use alloy_consensus::Transaction;
    use alloy_primitives::{Address, U256};

    use super::*;
    use crate::noxa_abi::{V3ExactInputIntent, V3ExactOutputIntent};
    use crate::robinhood::WETH;

    fn input_intent(recipient: Address) -> V3ExactInputIntent {
        V3ExactInputIntent {
            token_in: WETH,
            token_out: Address::with_last_byte(99),
            fee: 10_000,
            recipient,
            amount_in: U256::from(1_000),
            amount_out_minimum: U256::from(900),
            sqrt_price_limit_x96: U256::ZERO,
        }
    }

    #[test]
    fn signs_and_round_trips_prepared_eip1559_transaction() {
        let key = SigningKey::from_slice(&[7_u8; 32]).unwrap();
        let recipient = Address::from_private_key(&key);
        let plan = TradeTransactionPlan::exact_input(
            42,
            300_000,
            100_000_000,
            0,
            &input_intent(recipient),
        )
        .unwrap();
        let prepared = plan.sign(&key).unwrap();
        assert_eq!(prepared.hash, keccak256(&prepared.raw));
        assert_eq!(prepared.signer, Address::from_private_key(&key));

        let decoded = TxEnvelope::decode_2718_exact(&prepared.raw).unwrap();
        assert_eq!(decoded.chain_id(), Some(CHAIN_ID));
        assert_eq!(decoded.nonce(), 42);
        assert_eq!(decoded.to(), Some(UNISWAP_V3_SWAP_ROUTER_02));
        assert_eq!(decoded.value(), U256::ZERO);
    }

    #[test]
    fn rejects_native_value_and_unrecognized_calldata() {
        let mut plan = TradeTransactionPlan::exact_input(
            1,
            300_000,
            100_000_000,
            0,
            &input_intent(Address::with_last_byte(7)),
        )
        .unwrap();
        plan.value = U256::from(1);
        assert!(matches!(plan.validate(), Err(TradePlanError::NonZeroValue)));

        plan.value = U256::ZERO;
        plan.calldata = vec![1, 2, 3, 4].into();
        assert!(matches!(
            plan.validate(),
            Err(TradePlanError::UnsupportedCalldata)
        ));
    }

    #[test]
    fn rejects_malformed_and_unsafe_router_calls() {
        let recipient = Address::with_last_byte(7);
        let mut plan =
            TradeTransactionPlan::exact_input(1, 300_000, 100_000_000, 0, &input_intent(recipient))
                .unwrap();

        plan.calldata = EXACT_INPUT_SINGLE_SELECTOR.to_vec().into();
        assert!(matches!(
            plan.validate(),
            Err(TradePlanError::UnsupportedCalldata)
        ));

        plan.calldata = encode_v3_exact_input_single(&input_intent(recipient))
            .unwrap()
            .into();
        plan.calldata = [plan.calldata.as_ref(), &[0]].concat().into();
        assert!(matches!(
            plan.validate(),
            Err(TradePlanError::UnsupportedCalldata)
        ));

        let mut unsafe_intent = input_intent(recipient);
        unsafe_intent.token_in = Address::with_last_byte(1);
        let unsafe_calldata = encode_v3_exact_input_single(&unsafe_intent).unwrap();
        plan.calldata = unsafe_calldata.into();
        assert!(matches!(
            plan.validate(),
            Err(TradePlanError::UnsafeSwapParameters)
        ));
    }

    #[test]
    fn supports_exact_output_and_refuses_foreign_recipient_signing() {
        let recipient = Address::with_last_byte(7);
        let intent = V3ExactOutputIntent {
            token_in: WETH,
            token_out: Address::with_last_byte(99),
            fee: NOXA_POOL_FEE,
            recipient,
            amount_out: U256::from(900),
            amount_in_maximum: U256::from(1_000),
            sqrt_price_limit_x96: U256::ZERO,
        };
        let plan = TradeTransactionPlan::exact_output(1, 300_000, 100_000_000, 0, &intent).unwrap();
        assert!(matches!(
            plan.sign(&SigningKey::from_slice(&[7_u8; 32]).unwrap()),
            Err(TradePlanError::RecipientSignerMismatch)
        ));
    }
}
