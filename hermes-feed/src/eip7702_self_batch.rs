//! Exact, paper-only admission for the independently reviewed Pons EIP-7702 self batch.
//!
//! This is deliberately a single-account profile, not a generic EIP-7702 executor. All
//! delegation evidence is preloaded; candidate admission performs no I/O.

use alloy_consensus::{TxEnvelope, transaction::SignerRecoverable};
use alloy_eips::eip7702::SignedAuthorization;
use alloy_primitives::{Address, B256, Bytes, U256, keccak256};
use alloy_sol_types::{SolCall, sol};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::noxa_rpc::RobinhoodTransaction;
use crate::pons::{
    PONS_CHAIN_ID, PONS_CURRENT_FACTORY, PONS_LAUNCH_SELECTOR, PonsAdapter,
    PonsAttributionProvenance, PonsLaunchObservation, PonsObservationInput,
};

pub const PONS_EIP7702_PROOF_TX: B256 =
    alloy_primitives::b256!("7a13c94f90ddaa7d35d639f046f30a44d1d9b5fe449550fd0b75e5e65a0fb4c6");
pub const PONS_EIP7702_PROOF_BLOCK: u64 = 11_777_530;
pub const PONS_EIP7702_PROOF_BLOCK_HASH: B256 =
    alloy_primitives::b256!("d8cda07d851127f7c500e598aaa63e8ec8a3d6b3bae39556d2e7c6ed92801fd6");
pub const PONS_EIP7702_PROOF_TX_INDEX: u64 = 7;
pub const PONS_EIP7702_ACCOUNT: Address =
    alloy_primitives::address!("fb3538b3fac2cc5ffc582446c55875a889abd146");
pub const PONS_EIP7702_IMPLEMENTATION: Address =
    alloy_primitives::address!("dc44136e7ca3509a73fc6c22b6a6bd302bf9a1e2");
pub const PONS_EIP7702_AUXILIARY_TARGET: Address =
    alloy_primitives::address!("83cab64494cff66ce1c331fa9224692bdece5abb");
pub const PONS_EIP7702_AUTHORIZATION_NONCE: u64 = 15;
pub const PONS_EIP7702_AUXILIARY_VALUE_WEI: u64 = 538_332_961_881_668;
pub const PONS_EIP7702_INNER_VALUE_WEI: u64 = 60_500_000_000_000_000;
pub const PONS_EIP7702_INITIAL_BUY_WEI: u64 = 60_000_000_000_000_000;
pub const PONS_EIP7702_OUTER_SELECTOR: [u8; 4] = [0x3f, 0x70, 0x7e, 0x6b];
pub const PONS_EIP7702_DESIGNATOR_HASH: B256 =
    alloy_primitives::b256!("9bdfa4cdd2727209e60a7bbb51630848fd4abafc4f612d59fde7018262fa23f3");
pub const PONS_EIP7702_IMPLEMENTATION_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("6d7379e6220b87ceeade4a4e069c6a5ca4636fc228a0c948a0c87177860f3baa");
pub const PONS_EIP7702_IMPLEMENTATION_DEPLOYMENT_TX: B256 =
    alloy_primitives::b256!("f9e2b8d0c51a2357469ca8d4b06f2c4abc6d6456f843550e1f0ce0152c25a49e");
const MAX_REVIEWED_BATCH_BYTES: usize = 16 * 1024;

sol! {
    struct ReviewedBatchCall {
        address target;
        uint256 value;
        bytes data;
    }

    function execute(ReviewedBatchCall[] calls) external payable;
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Eip7702SelfBatchExpectedPins {
    pub account: Address,
    pub implementation: Address,
    pub designator_hash: B256,
    pub implementation_runtime_hash: B256,
    pub authorization_nonce: u64,
    pub proof_transaction: B256,
    pub proof_l2_block_number: u64,
    pub proof_l2_block_hash: B256,
    pub proof_l1_block_number: u64,
    pub proof_block_timestamp: u64,
    pub proof_transaction_index: u64,
    pub implementation_deployment_transaction: B256,
}

impl Eip7702SelfBatchExpectedPins {
    pub const fn production() -> Self {
        Self {
            account: PONS_EIP7702_ACCOUNT,
            implementation: PONS_EIP7702_IMPLEMENTATION,
            designator_hash: PONS_EIP7702_DESIGNATOR_HASH,
            implementation_runtime_hash: PONS_EIP7702_IMPLEMENTATION_RUNTIME_HASH,
            authorization_nonce: PONS_EIP7702_AUTHORIZATION_NONCE,
            proof_transaction: PONS_EIP7702_PROOF_TX,
            proof_l2_block_number: PONS_EIP7702_PROOF_BLOCK,
            proof_l2_block_hash: PONS_EIP7702_PROOF_BLOCK_HASH,
            proof_l1_block_number: 25_549_554,
            proof_block_timestamp: 1_784_256_986,
            proof_transaction_index: PONS_EIP7702_PROOF_TX_INDEX,
            implementation_deployment_transaction: PONS_EIP7702_IMPLEMENTATION_DEPLOYMENT_TX,
        }
    }

    pub fn validate(&self) -> Result<(), Eip7702SelfBatchReject> {
        if self != &Self::production() {
            return Err(Eip7702SelfBatchReject::ExpectedPinsDrift);
        }
        Ok(())
    }

    pub fn expected_provenance(
        &self,
    ) -> Result<Eip7702SelfBatchProvenance, Eip7702SelfBatchReject> {
        self.validate()?;
        Ok(Eip7702SelfBatchProvenance {
            outer_signer: self.account,
            self_target: self.account,
            authority: self.account,
            authorization_chain_id: PONS_CHAIN_ID,
            authorization_nonce: self.authorization_nonce,
            implementation: self.implementation,
            designator_hash: self.designator_hash,
            implementation_runtime_hash: self.implementation_runtime_hash,
            auxiliary_target: PONS_EIP7702_AUXILIARY_TARGET,
            auxiliary_value: U256::from(PONS_EIP7702_AUXILIARY_VALUE_WEI),
            inner_factory: PONS_CURRENT_FACTORY,
            inner_selector: PONS_LAUNCH_SELECTOR,
            inner_value: U256::from(PONS_EIP7702_INNER_VALUE_WEI),
        })
    }

    pub fn validate_provenance(
        &self,
        provenance: &Eip7702SelfBatchProvenance,
    ) -> Result<(), Eip7702SelfBatchReject> {
        if provenance != &self.expected_provenance()? {
            return Err(Eip7702SelfBatchReject::DelegationPair);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Eip7702ObservedDelegation<'a> {
    pub account: Address,
    pub designator: &'a [u8],
    pub designator_hash: B256,
    pub implementation: Address,
    pub implementation_runtime_hash: B256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Eip7702SelfBatchProvenance {
    pub outer_signer: Address,
    pub self_target: Address,
    pub authority: Address,
    pub authorization_chain_id: u64,
    pub authorization_nonce: u64,
    pub implementation: Address,
    pub designator_hash: B256,
    pub implementation_runtime_hash: B256,
    pub auxiliary_target: Address,
    pub auxiliary_value: U256,
    pub inner_factory: Address,
    pub inner_selector: [u8; 4],
    pub inner_value: U256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Eip7702SelfBatchPons {
    pub tx_hash: B256,
    pub provenance: Eip7702SelfBatchProvenance,
    pub inner_calldata: Bytes,
    pub pons: PonsLaunchObservation,
    pub execution_eligible: bool,
    pub broadcast: bool,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum Eip7702SelfBatchReject {
    #[error("reviewed EIP-7702 expected pins drifted")]
    ExpectedPinsDrift,
    #[error("transaction is not EIP-7702 type 4")]
    WrongTransactionType,
    #[error("transaction is not on chain 4663")]
    WrongChain,
    #[error("outer transaction signature is invalid")]
    InvalidOuterSignature,
    #[error("signer, reviewed account, and self target are not identical")]
    AccountIdentity,
    #[error("outer transaction value is not the reviewed zero-value profile")]
    OuterValue,
    #[error("authorization list is not exactly the reviewed singleton")]
    AuthorizationCount,
    #[error("authorization chain is not exactly 4663")]
    AuthorizationChain,
    #[error("authorization nonce drifted")]
    AuthorizationNonce,
    #[error("authorization implementation drifted")]
    AuthorizationImplementation,
    #[error("authorization signature is invalid")]
    InvalidAuthorizationSignature,
    #[error("authorization authority differs from signer/self target")]
    AuthorizationAuthority,
    #[error("observed delegation pair is incomplete or mismatched")]
    DelegationPair,
    #[error("account designator prefix, length, implementation, or hash drifted")]
    Designator,
    #[error("delegated implementation runtime drifted")]
    ImplementationRuntime,
    #[error("outer execute batch is malformed or noncanonical")]
    OuterAbi,
    #[error("outer execute batch selector is not the reviewed selector")]
    OuterSelector,
    #[error("batch does not contain exactly the two reviewed ordered calls")]
    CallShape,
    #[error("auxiliary call differs from the reviewed target, value, or empty calldata")]
    AuxiliaryCall,
    #[error("inner call is not the reviewed current Pons launch")]
    InnerCall,
    #[error("inner Pons developer wallet differs from signer/authority")]
    DeveloperWallet,
}

pub fn pons_eip7702_designator() -> [u8; 23] {
    let mut designator = [0_u8; 23];
    designator[..3].copy_from_slice(&[0xef, 0x01, 0x00]);
    designator[3..].copy_from_slice(PONS_EIP7702_IMPLEMENTATION.as_slice());
    designator
}

struct ValidatedReviewedBatch {
    auxiliary_target: Address,
    auxiliary_value: U256,
    inner_factory: Address,
    inner_value: U256,
    inner_calldata: Bytes,
    pons: PonsLaunchObservation,
}

fn validate_reviewed_authorization(
    authorizations: &[SignedAuthorization],
    signer: Address,
    expected: &Eip7702SelfBatchExpectedPins,
) -> Result<Address, Eip7702SelfBatchReject> {
    let [authorization] = authorizations else {
        return Err(Eip7702SelfBatchReject::AuthorizationCount);
    };
    if authorization.chain_id() != &U256::from(PONS_CHAIN_ID) {
        return Err(Eip7702SelfBatchReject::AuthorizationChain);
    }
    if authorization.nonce() != expected.authorization_nonce {
        return Err(Eip7702SelfBatchReject::AuthorizationNonce);
    }
    if authorization.address() != &expected.implementation {
        return Err(Eip7702SelfBatchReject::AuthorizationImplementation);
    }
    let authority = authorization
        .recover_authority()
        .map_err(|_| Eip7702SelfBatchReject::InvalidAuthorizationSignature)?;
    if authority != signer {
        return Err(Eip7702SelfBatchReject::AuthorizationAuthority);
    }
    Ok(authority)
}

fn validate_reviewed_batch(
    input: &[u8],
    signer: Address,
    tx_hash: B256,
) -> Result<ValidatedReviewedBatch, Eip7702SelfBatchReject> {
    if input.len() < 4 || input[..4] != PONS_EIP7702_OUTER_SELECTOR {
        return Err(Eip7702SelfBatchReject::OuterSelector);
    }
    if input.len() > MAX_REVIEWED_BATCH_BYTES {
        return Err(Eip7702SelfBatchReject::OuterAbi);
    }
    let batch = executeCall::abi_decode(input).map_err(|_| Eip7702SelfBatchReject::OuterAbi)?;
    if batch.abi_encode().as_slice() != input {
        return Err(Eip7702SelfBatchReject::OuterAbi);
    }
    let [auxiliary, inner] = batch.calls.as_slice() else {
        return Err(Eip7702SelfBatchReject::CallShape);
    };
    if auxiliary.target != PONS_EIP7702_AUXILIARY_TARGET
        || auxiliary.value != U256::from(PONS_EIP7702_AUXILIARY_VALUE_WEI)
        || !auxiliary.data.is_empty()
    {
        return Err(Eip7702SelfBatchReject::AuxiliaryCall);
    }
    if inner.target != PONS_CURRENT_FACTORY
        || inner.value != U256::from(PONS_EIP7702_INNER_VALUE_WEI)
        || inner.data.get(..4) != Some(PONS_LAUNCH_SELECTOR.as_slice())
    {
        return Err(Eip7702SelfBatchReject::InnerCall);
    }
    let adapter = PonsAdapter::from_startup_identities(PonsAdapter::required_startup_identities())
        .map_err(|_| Eip7702SelfBatchReject::InnerCall)?;
    let pons = adapter
        .observe_launch(PonsObservationInput {
            tx_hash,
            chain_id: PONS_CHAIN_ID,
            destination: inner.target,
            destination_runtime_hash: crate::pons::PONS_CURRENT_FACTORY_RUNTIME,
            calldata: &inner.data,
            value: inner.value,
            sender: signer,
            provenance: PonsAttributionProvenance::ExactFactoryTransaction,
        })
        .map_err(|_| Eip7702SelfBatchReject::InnerCall)?;
    if pons.launch.developer_wallet != signer {
        return Err(Eip7702SelfBatchReject::DeveloperWallet);
    }
    Ok(ValidatedReviewedBatch {
        auxiliary_target: auxiliary.target,
        auxiliary_value: auxiliary.value,
        inner_factory: inner.target,
        inner_value: inner.value,
        inner_calldata: inner.data.clone(),
        pons,
    })
}

pub fn decode_pons_eip7702_self_batch(
    transaction: &TxEnvelope,
    observed: Eip7702ObservedDelegation<'_>,
    expected: &Eip7702SelfBatchExpectedPins,
) -> Result<Eip7702SelfBatchPons, Eip7702SelfBatchReject> {
    expected.validate()?;
    let signed = transaction
        .as_eip7702()
        .ok_or(Eip7702SelfBatchReject::WrongTransactionType)?;
    let tx = signed.tx();
    if tx.chain_id != PONS_CHAIN_ID {
        return Err(Eip7702SelfBatchReject::WrongChain);
    }
    let signer = transaction
        .recover_signer()
        .map_err(|_| Eip7702SelfBatchReject::InvalidOuterSignature)?;
    if signer != expected.account || tx.to != signer {
        return Err(Eip7702SelfBatchReject::AccountIdentity);
    }
    if tx.value != U256::ZERO {
        return Err(Eip7702SelfBatchReject::OuterValue);
    }
    let authority = validate_reviewed_authorization(&tx.authorization_list, signer, expected)?;
    let authorization = &tx.authorization_list[0];
    if observed.account != signer || observed.implementation != expected.implementation {
        return Err(Eip7702SelfBatchReject::DelegationPair);
    }
    let designator = pons_eip7702_designator();
    if observed.designator != designator
        || observed.designator_hash != expected.designator_hash
        || keccak256(observed.designator) != expected.designator_hash
    {
        return Err(Eip7702SelfBatchReject::Designator);
    }
    if observed.implementation_runtime_hash != expected.implementation_runtime_hash {
        return Err(Eip7702SelfBatchReject::ImplementationRuntime);
    }
    let batch = validate_reviewed_batch(&tx.input, signer, *transaction.tx_hash())?;
    Ok(Eip7702SelfBatchPons {
        tx_hash: *transaction.tx_hash(),
        provenance: Eip7702SelfBatchProvenance {
            outer_signer: signer,
            self_target: tx.to,
            authority,
            authorization_chain_id: PONS_CHAIN_ID,
            authorization_nonce: authorization.nonce(),
            implementation: *authorization.address(),
            designator_hash: expected.designator_hash,
            implementation_runtime_hash: expected.implementation_runtime_hash,
            auxiliary_target: batch.auxiliary_target,
            auxiliary_value: batch.auxiliary_value,
            inner_factory: batch.inner_factory,
            inner_selector: PONS_LAUNCH_SELECTOR,
            inner_value: batch.inner_value,
        },
        inner_calldata: batch.inner_calldata,
        pons: batch.pons,
        execution_eligible: false,
        broadcast: false,
    })
}

/// Revalidate the serialized raw-feed provenance against the exact immutable proof transaction
/// fetched by the reconciler. This path exists because the RPC transaction view intentionally
/// does not retain authorization signatures; admission of those signatures already occurred in
/// the raw-feed observer and this function remains restricted to the one pinned transaction hash.
pub fn validate_pons_eip7702_reconciliation(
    transaction: &RobinhoodTransaction,
    provenance: &Eip7702SelfBatchProvenance,
    expected: &Eip7702SelfBatchExpectedPins,
) -> Result<Eip7702SelfBatchPons, Eip7702SelfBatchReject> {
    expected.validate()?;
    if transaction.hash != expected.proof_transaction
        || transaction.hash != PONS_EIP7702_PROOF_TX
        || transaction.l2_block_number != Some(expected.proof_l2_block_number)
        || transaction.transaction_index != Some(expected.proof_transaction_index)
        || transaction.from != expected.account
        || transaction.to != Some(expected.account)
    {
        return Err(Eip7702SelfBatchReject::AccountIdentity);
    }
    if transaction.value != U256::ZERO {
        return Err(Eip7702SelfBatchReject::OuterValue);
    }
    expected.validate_provenance(provenance)?;
    let batch =
        validate_reviewed_batch(&transaction.input, provenance.authority, transaction.hash)?;
    if batch.auxiliary_target != provenance.auxiliary_target
        || batch.auxiliary_value != provenance.auxiliary_value
        || batch.inner_factory != provenance.inner_factory
        || batch.inner_value != provenance.inner_value
    {
        return Err(Eip7702SelfBatchReject::CallShape);
    }
    Ok(Eip7702SelfBatchPons {
        tx_hash: transaction.hash,
        provenance: provenance.clone(),
        inner_calldata: batch.inner_calldata,
        pons: batch.pons,
        execution_eligible: false,
        broadcast: false,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use alloy_consensus::{SignableTransaction, TxEip7702};
    use alloy_eips::{
        eip2930::AccessList,
        eip7702::{Authorization, SignedAuthorization},
    };
    use alloy_primitives::Signature;
    use serde::Deserialize;

    use super::*;
    use crate::noxa_rpc::NoxaReceipt;

    #[derive(Debug, Deserialize)]
    struct ProofFixture {
        transaction: ProofTransaction,
        proof_identity: ProofIdentity,
        receipt: NoxaReceipt,
        expected: ExpectedResult,
    }

    #[derive(Debug, Deserialize)]
    struct ProofTransaction {
        hash: B256,
        transaction_type: String,
        chain_id: u64,
        nonce: u64,
        from: Address,
        to: Address,
        value: U256,
        input: Bytes,
        gas: U256,
        max_fee_per_gas: U256,
        max_priority_fee_per_gas: U256,
        authorization_list: Vec<ProofAuthorization>,
        y_parity: U256,
        r: U256,
        s: U256,
        l2_block_number: u64,
        block_hash: B256,
        transaction_index: u64,
    }

    #[derive(Debug, Clone, Deserialize)]
    struct ProofAuthorization {
        chain_id: U256,
        implementation: Address,
        nonce: U256,
        y_parity: U256,
        r: U256,
        s: U256,
    }

    #[derive(Debug, Deserialize)]
    struct ProofIdentity {
        account: Address,
        authority: Address,
        self_target: Address,
        designator: Bytes,
        designator_hash: B256,
        implementation: Address,
        implementation_runtime_hash: B256,
    }

    #[derive(Debug, Deserialize)]
    struct ExpectedResult {
        auxiliary_target: Address,
        auxiliary_value: U256,
        factory: Address,
        inner_value: U256,
        initial_buy_amount: U256,
        token: Address,
        pool: Address,
        position_id: U256,
        fee: u32,
        tick_spacing: i32,
        execution_eligible: bool,
        broadcast: bool,
    }

    fn fixture() -> ProofFixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/pons-eip7702-self-batch-clean4-proof.json"
        ))
        .unwrap()
    }

    fn envelope(fixture: &ProofFixture) -> TxEnvelope {
        let raw = &fixture.transaction;
        let authorizations = raw
            .authorization_list
            .iter()
            .map(|auth| {
                SignedAuthorization::new_unchecked(
                    Authorization {
                        chain_id: auth.chain_id,
                        address: auth.implementation,
                        nonce: u64::try_from(auth.nonce).unwrap(),
                    },
                    u8::try_from(auth.y_parity).unwrap(),
                    auth.r,
                    auth.s,
                )
            })
            .collect();
        let tx = TxEip7702 {
            chain_id: raw.chain_id,
            nonce: raw.nonce,
            gas_limit: u64::try_from(raw.gas).unwrap(),
            max_fee_per_gas: u128::try_from(raw.max_fee_per_gas).unwrap(),
            max_priority_fee_per_gas: u128::try_from(raw.max_priority_fee_per_gas).unwrap(),
            to: raw.to,
            value: raw.value,
            access_list: AccessList::default(),
            authorization_list: authorizations,
            input: raw.input.clone(),
        };
        TxEnvelope::Eip7702(tx.into_signed(Signature::new(
            raw.r,
            raw.s,
            raw.y_parity == U256::from(1_u8),
        )))
    }

    fn rpc_transaction(fixture: &ProofFixture) -> RobinhoodTransaction {
        RobinhoodTransaction {
            hash: fixture.transaction.hash,
            from: fixture.transaction.from,
            to: Some(fixture.transaction.to),
            input: fixture.transaction.input.clone(),
            value: fixture.transaction.value,
            l2_block_number: Some(fixture.transaction.l2_block_number),
            transaction_index: Some(fixture.transaction.transaction_index),
        }
    }

    pub(crate) fn clean4_proof_envelope() -> TxEnvelope {
        let fixture = fixture();
        envelope(&fixture)
    }

    fn authorization(raw: &ProofAuthorization) -> SignedAuthorization {
        SignedAuthorization::new_unchecked(
            Authorization {
                chain_id: raw.chain_id,
                address: raw.implementation,
                nonce: u64::try_from(raw.nonce).unwrap(),
            },
            u8::try_from(raw.y_parity).unwrap(),
            raw.r,
            raw.s,
        )
    }

    fn observed(fixture: &ProofFixture) -> Eip7702ObservedDelegation<'_> {
        Eip7702ObservedDelegation {
            account: fixture.proof_identity.account,
            designator: &fixture.proof_identity.designator,
            designator_hash: fixture.proof_identity.designator_hash,
            implementation: fixture.proof_identity.implementation,
            implementation_runtime_hash: fixture.proof_identity.implementation_runtime_hash,
        }
    }

    fn assert_batch_reject(calls: Vec<ReviewedBatchCall>, expected: Eip7702SelfBatchReject) {
        let input = executeCall { calls }.abi_encode();
        assert_eq!(
            validate_reviewed_batch(&input, PONS_EIP7702_ACCOUNT, PONS_EIP7702_PROOF_TX)
                .map(|_| ()),
            Err(expected)
        );
    }

    #[test]
    fn clean4_full_type4_proof_decodes_to_exact_paper_only_pons_profile() {
        let fixture = fixture();
        let envelope = envelope(&fixture);
        assert_eq!(fixture.transaction.transaction_type, "0x4");
        assert_eq!(*envelope.tx_hash(), fixture.transaction.hash);
        assert_eq!(fixture.transaction.hash, PONS_EIP7702_PROOF_TX);
        assert_eq!(fixture.transaction.from, PONS_EIP7702_ACCOUNT);
        assert_eq!(fixture.proof_identity.authority, PONS_EIP7702_ACCOUNT);
        assert_eq!(fixture.proof_identity.self_target, PONS_EIP7702_ACCOUNT);
        assert_eq!(
            fixture.transaction.l2_block_number,
            PONS_EIP7702_PROOF_BLOCK
        );
        assert_eq!(
            fixture.transaction.block_hash,
            PONS_EIP7702_PROOF_BLOCK_HASH
        );
        assert_eq!(
            fixture.transaction.transaction_index,
            PONS_EIP7702_PROOF_TX_INDEX
        );
        assert!(fixture.receipt.status);
        assert_eq!(fixture.receipt.logs.len(), 18);

        let decoded = decode_pons_eip7702_self_batch(
            &envelope,
            observed(&fixture),
            &Eip7702SelfBatchExpectedPins::production(),
        )
        .unwrap();
        assert_eq!(decoded.provenance.outer_signer, PONS_EIP7702_ACCOUNT);
        assert_eq!(decoded.provenance.authority, PONS_EIP7702_ACCOUNT);
        assert_eq!(decoded.provenance.self_target, PONS_EIP7702_ACCOUNT);
        assert_eq!(
            decoded.provenance.auxiliary_target,
            fixture.expected.auxiliary_target
        );
        assert_eq!(
            decoded.provenance.auxiliary_value,
            fixture.expected.auxiliary_value
        );
        assert_eq!(decoded.provenance.inner_factory, fixture.expected.factory);
        assert_eq!(decoded.provenance.inner_value, fixture.expected.inner_value);
        assert_eq!(
            decoded.provenance.inner_value - U256::from(crate::pons::PONS_LAUNCH_FEE_WEI),
            fixture.expected.initial_buy_amount
        );
        assert_eq!(
            fixture.expected.token,
            alloy_primitives::address!("331a3c242517127cec8ba5d974b2cb07b9050363")
        );
        assert_eq!(
            fixture.expected.pool,
            alloy_primitives::address!("05a92349a7af8456474b3ef3ca7dda859677535a")
        );
        assert_eq!(fixture.expected.position_id, U256::from(184_384_u64));
        assert_eq!(fixture.expected.fee, 10_000);
        assert_eq!(fixture.expected.tick_spacing, 200);
        assert_eq!(
            decoded.execution_eligible,
            fixture.expected.execution_eligible
        );
        assert_eq!(decoded.broadcast, fixture.expected.broadcast);
        assert!(!decoded.execution_eligible);
        assert!(!decoded.broadcast);

        let quote = crate::pons_receipt_quote::quote_pons_eip7702_self_batch_receipt(
            &envelope,
            &fixture.receipt,
            observed(&fixture),
            &Eip7702SelfBatchExpectedPins::production(),
            crate::pons::PonsExpectedProfile::production(),
            crate::pons_receipt_quote::PonsQuotePolicy {
                amount_in: U256::from(1_000_000_000_000_000_u64),
                max_amount_in: U256::from(10_000_000_000_000_000_u64),
                slippage_bps: 100,
            },
        )
        .unwrap();
        assert_eq!(quote.market.token, fixture.expected.token);
        assert_eq!(quote.market.pool, fixture.expected.pool);
        assert_eq!(quote.market.position_id, fixture.expected.position_id);
        assert_eq!(
            quote.market.initial_buy_amount,
            fixture.expected.initial_buy_amount
        );
        assert!(quote.entry.expected_output > U256::ZERO);
        assert!(quote.full_position_exit.expected_output > U256::ZERO);
        assert_eq!(quote.wrapper_provenance, Some(decoded.provenance));
        assert!(!quote.execution_eligible);
        assert!(!quote.broadcast);
    }

    #[test]
    fn rejects_incomplete_or_drifted_delegation_proof_and_expected_pins() {
        let fixture = fixture();
        let envelope = envelope(&fixture);
        let mut drifted_runtime = observed(&fixture);
        drifted_runtime.implementation_runtime_hash = B256::with_last_byte(1);
        assert_eq!(
            decode_pons_eip7702_self_batch(
                &envelope,
                drifted_runtime,
                &Eip7702SelfBatchExpectedPins::production()
            ),
            Err(Eip7702SelfBatchReject::ImplementationRuntime)
        );

        let mut wrong_designator = fixture.proof_identity.designator.to_vec();
        wrong_designator[0] ^= 1;
        let mut drifted_designator = observed(&fixture);
        drifted_designator.designator = &wrong_designator;
        assert_eq!(
            decode_pons_eip7702_self_batch(
                &envelope,
                drifted_designator,
                &Eip7702SelfBatchExpectedPins::production()
            ),
            Err(Eip7702SelfBatchReject::Designator)
        );

        let mut pins = Eip7702SelfBatchExpectedPins::production();
        pins.authorization_nonce -= 1;
        assert_eq!(
            decode_pons_eip7702_self_batch(&envelope, observed(&fixture), &pins),
            Err(Eip7702SelfBatchReject::ExpectedPinsDrift)
        );
    }

    #[test]
    fn authorization_profile_rejects_count_chain_nonce_implementation_and_signature_drift() {
        let fixture = fixture();
        let expected = Eip7702SelfBatchExpectedPins::production();
        let exact = authorization(&fixture.transaction.authorization_list[0]);
        assert_eq!(
            validate_reviewed_authorization(
                std::slice::from_ref(&exact),
                PONS_EIP7702_ACCOUNT,
                &expected
            ),
            Ok(PONS_EIP7702_ACCOUNT)
        );
        assert_eq!(
            validate_reviewed_authorization(&[], PONS_EIP7702_ACCOUNT, &expected),
            Err(Eip7702SelfBatchReject::AuthorizationCount)
        );
        assert_eq!(
            validate_reviewed_authorization(
                &[exact.clone(), exact.clone()],
                PONS_EIP7702_ACCOUNT,
                &expected
            ),
            Err(Eip7702SelfBatchReject::AuthorizationCount)
        );

        let raw = &fixture.transaction.authorization_list[0];
        let wrong_chain = SignedAuthorization::new_unchecked(
            Authorization {
                chain_id: U256::ZERO,
                address: raw.implementation,
                nonce: PONS_EIP7702_AUTHORIZATION_NONCE,
            },
            u8::try_from(raw.y_parity).unwrap(),
            raw.r,
            raw.s,
        );
        assert_eq!(
            validate_reviewed_authorization(&[wrong_chain], PONS_EIP7702_ACCOUNT, &expected),
            Err(Eip7702SelfBatchReject::AuthorizationChain)
        );
        let wrong_nonce = SignedAuthorization::new_unchecked(
            Authorization {
                chain_id: U256::from(PONS_CHAIN_ID),
                address: raw.implementation,
                nonce: PONS_EIP7702_AUTHORIZATION_NONCE - 1,
            },
            u8::try_from(raw.y_parity).unwrap(),
            raw.r,
            raw.s,
        );
        assert_eq!(
            validate_reviewed_authorization(&[wrong_nonce], PONS_EIP7702_ACCOUNT, &expected),
            Err(Eip7702SelfBatchReject::AuthorizationNonce)
        );
        let wrong_implementation = SignedAuthorization::new_unchecked(
            Authorization {
                chain_id: U256::from(PONS_CHAIN_ID),
                address: Address::with_last_byte(9),
                nonce: PONS_EIP7702_AUTHORIZATION_NONCE,
            },
            u8::try_from(raw.y_parity).unwrap(),
            raw.r,
            raw.s,
        );
        assert_eq!(
            validate_reviewed_authorization(
                &[wrong_implementation],
                PONS_EIP7702_ACCOUNT,
                &expected
            ),
            Err(Eip7702SelfBatchReject::AuthorizationImplementation)
        );
        let invalid_signature = SignedAuthorization::new_unchecked(
            Authorization {
                chain_id: U256::from(PONS_CHAIN_ID),
                address: raw.implementation,
                nonce: PONS_EIP7702_AUTHORIZATION_NONCE,
            },
            2,
            raw.r,
            raw.s,
        );
        assert_eq!(
            validate_reviewed_authorization(&[invalid_signature], PONS_EIP7702_ACCOUNT, &expected),
            Err(Eip7702SelfBatchReject::InvalidAuthorizationSignature)
        );
    }

    #[test]
    fn rejects_wrong_transaction_type_chain_self_target_and_outer_value() {
        let fixture = fixture();
        let wrong_type: TxEnvelope = alloy_consensus::TxEip1559::default()
            .into_signed(Signature::new(U256::from(1_u8), U256::from(1_u8), false))
            .into();
        assert_eq!(
            decode_pons_eip7702_self_batch(
                &wrong_type,
                observed(&fixture),
                &Eip7702SelfBatchExpectedPins::production()
            ),
            Err(Eip7702SelfBatchReject::WrongTransactionType)
        );

        let mut wrong_chain = envelope(&fixture);
        let TxEnvelope::Eip7702(signed) = &mut wrong_chain else {
            unreachable!()
        };
        signed.tx_mut().chain_id = 8_453;
        assert_eq!(
            decode_pons_eip7702_self_batch(
                &wrong_chain,
                observed(&fixture),
                &Eip7702SelfBatchExpectedPins::production()
            ),
            Err(Eip7702SelfBatchReject::WrongChain)
        );

        let decoded = decode_pons_eip7702_self_batch(
            &envelope(&fixture),
            observed(&fixture),
            &Eip7702SelfBatchExpectedPins::production(),
        )
        .unwrap();
        let mut wrong_target = rpc_transaction(&fixture);
        wrong_target.to = Some(Address::with_last_byte(1));
        assert_eq!(
            validate_pons_eip7702_reconciliation(
                &wrong_target,
                &decoded.provenance,
                &Eip7702SelfBatchExpectedPins::production()
            ),
            Err(Eip7702SelfBatchReject::AccountIdentity)
        );
        let mut nonzero_value = rpc_transaction(&fixture);
        nonzero_value.value = U256::from(1_u8);
        assert_eq!(
            validate_pons_eip7702_reconciliation(
                &nonzero_value,
                &decoded.provenance,
                &Eip7702SelfBatchExpectedPins::production()
            ),
            Err(Eip7702SelfBatchReject::OuterValue)
        );
    }

    #[test]
    fn exact_batch_profile_rejects_selector_abi_count_order_and_call_drift() {
        let fixture = fixture();
        let decoded = executeCall::abi_decode(&fixture.transaction.input).unwrap();
        let calls = decoded.calls;

        let mut wrong_selector = fixture.transaction.input.to_vec();
        wrong_selector[0] ^= 1;
        assert_eq!(
            validate_reviewed_batch(&wrong_selector, PONS_EIP7702_ACCOUNT, PONS_EIP7702_PROOF_TX)
                .map(|_| ()),
            Err(Eip7702SelfBatchReject::OuterSelector)
        );
        let mut trailing = fixture.transaction.input.to_vec();
        trailing.push(0);
        assert_eq!(
            validate_reviewed_batch(&trailing, PONS_EIP7702_ACCOUNT, PONS_EIP7702_PROOF_TX)
                .map(|_| ()),
            Err(Eip7702SelfBatchReject::OuterAbi)
        );
        assert_batch_reject(vec![calls[0].clone()], Eip7702SelfBatchReject::CallShape);
        assert_batch_reject(
            vec![calls[1].clone(), calls[0].clone()],
            Eip7702SelfBatchReject::AuxiliaryCall,
        );
        assert_batch_reject(
            vec![calls[0].clone(), calls[0].clone()],
            Eip7702SelfBatchReject::InnerCall,
        );

        let mut wrong_auxiliary = calls.clone();
        wrong_auxiliary[0].target = Address::with_last_byte(1);
        assert_batch_reject(wrong_auxiliary, Eip7702SelfBatchReject::AuxiliaryCall);
        let mut auxiliary_calldata = calls.clone();
        auxiliary_calldata[0].data = Bytes::from_static(&[0]);
        assert_batch_reject(auxiliary_calldata, Eip7702SelfBatchReject::AuxiliaryCall);
        let mut auxiliary_value = calls.clone();
        auxiliary_value[0].value += U256::from(1_u8);
        assert_batch_reject(auxiliary_value, Eip7702SelfBatchReject::AuxiliaryCall);

        let mut wrong_factory = calls.clone();
        wrong_factory[1].target = crate::pons::PONS_LEGACY_FACTORY;
        assert_batch_reject(wrong_factory, Eip7702SelfBatchReject::InnerCall);
        let mut wrong_inner_value = calls.clone();
        wrong_inner_value[1].value -= U256::from(1_u8);
        assert_batch_reject(wrong_inner_value, Eip7702SelfBatchReject::InnerCall);
        let mut wrong_inner_selector = calls.clone();
        let mut inner_data = wrong_inner_selector[1].data.to_vec();
        inner_data[0] ^= 1;
        wrong_inner_selector[1].data = inner_data.into();
        assert_batch_reject(wrong_inner_selector, Eip7702SelfBatchReject::InnerCall);

        let mut wrong_config = calls.clone();
        let mut inner_data = wrong_config[1].data.to_vec();
        inner_data[67] = 1;
        wrong_config[1].data = inner_data.into();
        assert_batch_reject(wrong_config, Eip7702SelfBatchReject::InnerCall);
        let mut wrong_dex = calls.clone();
        let mut inner_data = wrong_dex[1].data.to_vec();
        inner_data[99] = 1;
        wrong_dex[1].data = inner_data.into();
        assert_batch_reject(wrong_dex, Eip7702SelfBatchReject::InnerCall);

        let mut wrong_wallet = calls;
        let padded_signer = [&[0_u8; 12][..], PONS_EIP7702_ACCOUNT.as_slice()].concat();
        let wallet_offset = wrong_wallet[1]
            .data
            .windows(32)
            .position(|window| window == padded_signer)
            .unwrap();
        let mut inner_data = wrong_wallet[1].data.to_vec();
        inner_data[wallet_offset + 31] ^= 1;
        wrong_wallet[1].data = inner_data.into();
        assert_batch_reject(wrong_wallet, Eip7702SelfBatchReject::DeveloperWallet);
    }

    #[test]
    fn receipt_reconciliation_rejects_boundary_and_topology_mismatches() {
        let fixture = fixture();
        let envelope = envelope(&fixture);
        let quote = |receipt: &NoxaReceipt| {
            crate::pons_receipt_quote::quote_pons_eip7702_self_batch_receipt(
                &envelope,
                receipt,
                observed(&fixture),
                &Eip7702SelfBatchExpectedPins::production(),
                crate::pons::PonsExpectedProfile::production(),
                crate::pons_receipt_quote::PonsQuotePolicy {
                    amount_in: U256::from(1_000_000_000_000_000_u64),
                    max_amount_in: U256::from(10_000_000_000_000_000_u64),
                    slippage_bps: 100,
                },
            )
        };

        let mut reverted = fixture.receipt.clone();
        reverted.status = false;
        assert!(quote(&reverted).is_err());
        let mut wrong_block = fixture.receipt.clone();
        wrong_block.l2_block_number -= 1;
        assert!(quote(&wrong_block).is_err());
        let mut wrong_hash = fixture.receipt.clone();
        wrong_hash.block_hash = B256::with_last_byte(1);
        assert!(quote(&wrong_hash).is_err());
        let mut wrong_index = fixture.receipt.clone();
        wrong_index.transaction_index += 1;
        assert!(quote(&wrong_index).is_err());

        let mut missing_launch = fixture.receipt.clone();
        missing_launch
            .logs
            .retain(|log| log.topics.first() != Some(&crate::pons::PONS_TOKEN_LAUNCHED_TOPIC));
        assert!(quote(&missing_launch).is_err());
        let mut wrong_pool = fixture.receipt.clone();
        wrong_pool
            .logs
            .iter_mut()
            .find(|log| log.address == crate::pons::PONS_V3_FACTORY)
            .unwrap()
            .address = Address::with_last_byte(2);
        assert!(quote(&wrong_pool).is_err());
        let mut wrong_locker = fixture.receipt.clone();
        wrong_locker
            .logs
            .iter_mut()
            .find(|log| log.address == crate::pons::PONS_CURRENT_LOCKER)
            .unwrap()
            .address = Address::with_last_byte(3);
        assert!(quote(&wrong_locker).is_err());
        let mut missing_swap = fixture.receipt.clone();
        missing_swap
            .logs
            .retain(|log| log.topics.first() != Some(&crate::pons::PONS_V3_SWAP_TOPIC));
        assert!(quote(&missing_swap).is_err());
    }
}
