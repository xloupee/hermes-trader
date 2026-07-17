//! Receipt-free identity prediction for the current Pons generation.
//!
//! Construction is startup-only: a separately reviewed expected profile and
//! a fresh observed semantic snapshot must agree exactly. Candidate handling
//! then uses only warm in-memory data and performs no RPC calls.

use alloy_primitives::{Address, B256, U256, keccak256};
use alloy_sol_types::{SolValue, sol};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::noxa_predict::{create2_address, predict_v3_pool_address};
use crate::pons::{
    PONS_CURRENT_FACTORY, PONS_CURRENT_LOCKER, PONS_DEX_CONFIG_ID, PONS_LAUNCH_CONFIG_ID,
    PONS_LAUNCH_FEE_WEI, PONS_POOL_FEE, PONS_POSITION_MANAGER, PONS_SWAP_ROUTER_02,
    PONS_V3_FACTORY, PONS_WETH, PonsGeneration, PonsLaunchObservation,
};
use crate::robinhood::UNISWAP_V3_POOL_INIT_CODE_KECCAK256;

pub const PONS_TOKEN_CREATION_CODE_OFFSET: usize = 14_686;
pub const PONS_TOKEN_CREATION_CODE_BYTES: usize = 9_453;
pub const PONS_TOKEN_CREATION_CODE_KECCAK256: B256 =
    alloy_primitives::b256!("86588bc75e5a00a2e28ba6f44fb4c15c899dcd9a0622b28d116d8ca5f8635804");
pub const PONS_PREDICT_TOKEN_SELECTOR: [u8; 4] = [0xea, 0x9d, 0x3f, 0xdc];
pub const PONS_LAUNCH_CONFIG_SELECTOR: [u8; 4] = [0x1c, 0xad, 0x86, 0x2d];
pub const PONS_DEX_CONFIG_SELECTOR: [u8; 4] = [0x71, 0x0b, 0xb9, 0x4c];
pub const PONS_LAUNCH_ENABLED_SELECTOR: [u8; 4] = [0x23, 0x6a, 0x4a, 0xfb];
pub const PONS_LAUNCH_FEE_SELECTOR: [u8; 4] = [0xcf, 0x3c, 0xf5, 0x73];
pub const PONS_LOCKER_SELECTOR: [u8; 4] = [0xd7, 0xb9, 0x6d, 0x4e];
pub const PONS_INITIAL_LIQUIDITY_WEI: u64 = 4_200_000_000_000_000_000;
pub const PONS_INITIAL_TICK: i32 = -204_200;
pub const PONS_MAX_WALLET_BPS: u16 = 200;
pub const PONS_MAX_TX_BPS: u16 = 220;
pub const PONS_RESTRICTION_BLOCKS: u32 = 366;

fn pons_total_supply() -> U256 {
    U256::from(1_000_000_000_000_000_000_u128) * U256::from(1_000_000_000_u64)
}

sol! {
    struct TokenConstructorSocials {
        string twitter;
        string telegram;
        string discord;
        string website;
        string farcaster;
    }

}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PonsLaunchConfigSnapshot {
    pub pair_token: Address,
    pub initial_liquidity: U256,
    pub initial_tick: i32,
    pub supply: U256,
    pub max_wallet_bps: U256,
    pub max_tx_bps: U256,
    pub restriction_blocks: u32,
    pub flags: [bool; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PonsDexConfigSnapshot {
    pub name: String,
    pub factory: Address,
    pub position_manager: Address,
    pub swap_router: Address,
    pub pool_fee: u32,
    pub tick_spacing: i32,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PonsPredictionSemantics {
    pub factory: Address,
    pub launch_enabled: bool,
    pub launch_fee: U256,
    pub locker: Address,
    pub token_creation_code_offset: usize,
    pub token_creation_code_bytes: usize,
    pub token_creation_code_hash: B256,
    pub prediction_selector: [u8; 4],
    pub pool_init_code_hash: B256,
    pub launch_config_0: PonsLaunchConfigSnapshot,
    pub dex_config_0: PonsDexConfigSnapshot,
}

impl PonsPredictionSemantics {
    pub fn production() -> Self {
        Self {
            factory: PONS_CURRENT_FACTORY,
            launch_enabled: true,
            launch_fee: U256::from(PONS_LAUNCH_FEE_WEI),
            locker: PONS_CURRENT_LOCKER,
            token_creation_code_offset: PONS_TOKEN_CREATION_CODE_OFFSET,
            token_creation_code_bytes: PONS_TOKEN_CREATION_CODE_BYTES,
            token_creation_code_hash: PONS_TOKEN_CREATION_CODE_KECCAK256,
            prediction_selector: PONS_PREDICT_TOKEN_SELECTOR,
            pool_init_code_hash: UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
            launch_config_0: PonsLaunchConfigSnapshot {
                pair_token: PONS_WETH,
                initial_liquidity: U256::from(PONS_INITIAL_LIQUIDITY_WEI),
                initial_tick: PONS_INITIAL_TICK,
                supply: pons_total_supply(),
                max_wallet_bps: U256::from(PONS_MAX_WALLET_BPS),
                max_tx_bps: U256::from(PONS_MAX_TX_BPS),
                restriction_blocks: PONS_RESTRICTION_BLOCKS,
                flags: [false, true, false],
            },
            dex_config_0: PonsDexConfigSnapshot {
                name: "uniswap v3".into(),
                factory: PONS_V3_FACTORY,
                position_manager: PONS_POSITION_MANAGER,
                swap_router: PONS_SWAP_ROUTER_02,
                pool_fee: PONS_POOL_FEE,
                tick_spacing: 200,
                enabled: true,
            },
        }
    }

    pub fn validate_production(&self) -> Result<(), PonsPredictionError> {
        if self != &Self::production() {
            return Err(PonsPredictionError::SemanticDrift);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PredictedPonsMarket {
    pub token: Address,
    pub pool: Address,
}

#[derive(Debug, Clone)]
pub struct PonsCurrentPredictor {
    semantics: PonsPredictionSemantics,
    token_creation_code: Vec<u8>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PonsPredictionError {
    #[error("Pons expected or observed current-generation semantics drifted")]
    SemanticDrift,
    #[error("Pons token creation prefix is malformed or mismatched")]
    CreationCodeDrift,
    #[error("legacy Pons launches remain receipt-only discovery")]
    LegacyGeneration,
    #[error("Pons launch intent does not match current config zero")]
    UnsupportedConfiguration,
    #[error("Pons semantic getter returned malformed ABI")]
    MalformedGetter,
}

impl PonsCurrentPredictor {
    pub fn from_startup_profiles(
        expected: &PonsPredictionSemantics,
        observed: &PonsPredictionSemantics,
    ) -> Result<Self, PonsPredictionError> {
        expected.validate_production()?;
        if observed != expected {
            return Err(PonsPredictionError::SemanticDrift);
        }
        let token_creation_code =
            hex::decode(include_str!("pins/pons-current-token-creation-code.hex").trim())
                .map_err(|_| PonsPredictionError::CreationCodeDrift)?;
        if token_creation_code.len() != expected.token_creation_code_bytes
            || keccak256(&token_creation_code) != expected.token_creation_code_hash
        {
            return Err(PonsPredictionError::CreationCodeDrift);
        }
        Ok(Self {
            semantics: expected.clone(),
            token_creation_code,
        })
    }

    pub fn predict(
        &self,
        observed: &PonsLaunchObservation,
        outer_signer: Address,
    ) -> Result<PredictedPonsMarket, PonsPredictionError> {
        if observed.generation != PonsGeneration::Current {
            return Err(PonsPredictionError::LegacyGeneration);
        }
        if observed.launch.launch_config_id != U256::from(PONS_LAUNCH_CONFIG_ID)
            || observed.launch.dex_config_id != U256::from(PONS_DEX_CONFIG_ID)
            || outer_signer == Address::ZERO
        {
            return Err(PonsPredictionError::UnsupportedConfiguration);
        }

        let init_code = self.build_init_code(observed, outer_signer)?;
        let token = create2_address(
            self.semantics.factory,
            observed.launch.salt,
            keccak256(init_code),
        );
        let (token0, token1) = if token < self.semantics.launch_config_0.pair_token {
            (token, self.semantics.launch_config_0.pair_token)
        } else {
            (self.semantics.launch_config_0.pair_token, token)
        };
        let pool = predict_v3_pool_address(
            self.semantics.dex_config_0.factory,
            token0,
            token1,
            self.semantics.dex_config_0.pool_fee,
            self.semantics.pool_init_code_hash,
        );
        Ok(PredictedPonsMarket { token, pool })
    }

    fn build_init_code(
        &self,
        observed: &PonsLaunchObservation,
        outer_signer: Address,
    ) -> Result<Vec<u8>, PonsPredictionError> {
        // The factory forwards the five social strings positionally. Its call
        // tuple names differ from the token constructor names, so intentionally
        // do not remap fields by label here.
        let constructor = (
            observed.launch.name.clone(),
            observed.launch.symbol.clone(),
            observed.launch.logo.clone(),
            observed.launch.description.clone(),
            TokenConstructorSocials {
                twitter: observed.launch.socials.telegram.clone(),
                telegram: observed.launch.socials.twitter.clone(),
                discord: observed.launch.socials.discord.clone(),
                website: observed.launch.socials.website.clone(),
                farcaster: observed.launch.socials.farcaster.clone(),
            },
            outer_signer,
            self.semantics.dex_config_0.factory,
            self.semantics.dex_config_0.position_manager,
            self.semantics.launch_config_0.pair_token,
            alloy_primitives::aliases::U24::from(self.semantics.dex_config_0.pool_fee),
            self.semantics.launch_config_0.supply,
            PONS_MAX_WALLET_BPS,
            PONS_MAX_TX_BPS,
            PONS_RESTRICTION_BLOCKS,
        )
            .abi_encode_params();
        let mut init_code = Vec::with_capacity(self.token_creation_code.len() + constructor.len());
        init_code.extend_from_slice(&self.token_creation_code);
        init_code.extend_from_slice(&constructor);
        Ok(init_code)
    }
}

pub fn extract_creation_prefix(runtime: &[u8]) -> Result<&[u8], PonsPredictionError> {
    let end = PONS_TOKEN_CREATION_CODE_OFFSET
        .checked_add(PONS_TOKEN_CREATION_CODE_BYTES)
        .ok_or(PonsPredictionError::CreationCodeDrift)?;
    let code = runtime
        .get(PONS_TOKEN_CREATION_CODE_OFFSET..end)
        .ok_or(PonsPredictionError::CreationCodeDrift)?;
    if keccak256(code) != PONS_TOKEN_CREATION_CODE_KECCAK256 {
        return Err(PonsPredictionError::CreationCodeDrift);
    }
    Ok(code)
}

pub fn config_call(selector: [u8; 4], id: u64) -> [u8; 36] {
    let mut call = [0_u8; 36];
    call[..4].copy_from_slice(&selector);
    call[28..].copy_from_slice(&id.to_be_bytes());
    call
}

pub fn decode_launch_config(bytes: &[u8]) -> Result<PonsLaunchConfigSnapshot, PonsPredictionError> {
    if bytes.len() != 10 * 32 {
        return Err(PonsPredictionError::MalformedGetter);
    }
    let word = |index: usize| U256::from_be_slice(&bytes[index * 32..(index + 1) * 32]);
    let address = |index: usize| {
        let raw = &bytes[index * 32..(index + 1) * 32];
        if raw[..12].iter().any(|byte| *byte != 0) {
            return Err(PonsPredictionError::MalformedGetter);
        }
        Ok(Address::from_slice(&raw[12..]))
    };
    let bool_at = |index: usize| match word(index) {
        value if value == U256::ZERO => Ok(false),
        value if value == U256::from(1) => Ok(true),
        _ => Err(PonsPredictionError::MalformedGetter),
    };
    let tick_word = &bytes[2 * 32..3 * 32];
    let negative = tick_word[29] & 0x80 != 0;
    let expected_prefix = if negative { 0xff } else { 0x00 };
    if tick_word[..29].iter().any(|byte| *byte != expected_prefix) {
        return Err(PonsPredictionError::MalformedGetter);
    }
    let raw_tick =
        i32::from_be_bytes([expected_prefix, tick_word[29], tick_word[30], tick_word[31]]);
    if !(-8_388_608..=8_388_607).contains(&raw_tick) {
        return Err(PonsPredictionError::MalformedGetter);
    }
    Ok(PonsLaunchConfigSnapshot {
        pair_token: address(0)?,
        initial_liquidity: word(1),
        initial_tick: raw_tick,
        supply: word(3),
        max_wallet_bps: word(4),
        max_tx_bps: word(5),
        restriction_blocks: u32::try_from(word(6))
            .map_err(|_| PonsPredictionError::MalformedGetter)?,
        flags: [bool_at(7)?, bool_at(8)?, bool_at(9)?],
    })
}

pub fn decode_dex_config(bytes: &[u8]) -> Result<PonsDexConfigSnapshot, PonsPredictionError> {
    const ENCODED_BYTES: usize = 10 * 32;
    const TUPLE_START: usize = 32;
    const NAME_OFFSET: usize = 7 * 32;
    const NAME_LENGTH_START: usize = TUPLE_START + NAME_OFFSET;
    const NAME_DATA_START: usize = NAME_LENGTH_START + 32;

    if bytes.len() != ENCODED_BYTES
        || U256::from_be_slice(&bytes[TUPLE_START..TUPLE_START + 32]) != U256::from(NAME_OFFSET)
    {
        return Err(PonsPredictionError::MalformedGetter);
    }
    let decoded = crate::noxa_predict::decode_dex_config(bytes)
        .map_err(|_| PonsPredictionError::MalformedGetter)?;
    let name = decoded.name.as_bytes();
    if name.len() > 32
        || U256::from_be_slice(&bytes[NAME_LENGTH_START..NAME_DATA_START]) != U256::from(name.len())
        || bytes[NAME_DATA_START..NAME_DATA_START + name.len()] != name[..]
        || bytes[NAME_DATA_START + name.len()..]
            .iter()
            .any(|byte| *byte != 0)
    {
        return Err(PonsPredictionError::MalformedGetter);
    }
    Ok(PonsDexConfigSnapshot {
        name: decoded.name,
        factory: decoded.factory,
        position_manager: decoded.position_manager,
        swap_router: decoded.swap_router,
        pool_fee: decoded.pool_fee,
        tick_spacing: decoded.tick_spacing,
        enabled: decoded.enabled,
    })
}

pub fn decode_word(bytes: &[u8]) -> Result<U256, PonsPredictionError> {
    if bytes.len() != 32 {
        return Err(PonsPredictionError::MalformedGetter);
    }
    Ok(U256::from_be_slice(bytes))
}

pub fn decode_bool(bytes: &[u8]) -> Result<bool, PonsPredictionError> {
    match decode_word(bytes)? {
        value if value == U256::ZERO => Ok(false),
        value if value == U256::from(1) => Ok(true),
        _ => Err(PonsPredictionError::MalformedGetter),
    }
}

pub fn decode_address(bytes: &[u8]) -> Result<Address, PonsPredictionError> {
    if bytes.len() != 32 || bytes[..12].iter().any(|byte| *byte != 0) {
        return Err(PonsPredictionError::MalformedGetter);
    }
    Ok(Address::from_slice(&bytes[12..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::noxa_rpc::{NoxaReceipt, RobinhoodBlock, RobinhoodTransaction};
    use crate::pons::{
        PONS_CHAIN_ID, PONS_CURRENT_FACTORY_RUNTIME, PonsAdapter, PonsAttributionProvenance,
        PonsObservationInput,
    };
    use crate::pons_receipt_quote::{PonsQuotePolicy, quote_pons_launch_receipt};

    #[derive(Deserialize)]
    struct ProofFixture {
        proofs: Vec<Proof>,
    }

    #[derive(Deserialize)]
    struct Proof {
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
        trace: TraceProof,
        pre_block_prediction: PreBlockPrediction,
        expected_token: Address,
        expected_pool: Address,
    }

    #[derive(Deserialize)]
    struct TraceProof {
        token_create2_from: Address,
        token_create2_to: Address,
        token_init_code_bytes: usize,
        token_creation_prefix_bytes: usize,
        token_creation_prefix_hash: B256,
        constructor_args: alloy_primitives::Bytes,
        pool_create2_from: Address,
        pool_create2_to: Address,
    }

    #[derive(Deserialize)]
    struct PreBlockPrediction {
        selector: String,
        l2_block_number: u64,
        token: Address,
    }

    fn fixtures() -> ProofFixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/pons-current-prediction-proofs.json"
        ))
        .unwrap()
    }

    fn predictor() -> PonsCurrentPredictor {
        let profile = PonsPredictionSemantics::production();
        PonsCurrentPredictor::from_startup_profiles(&profile, &profile).unwrap()
    }

    fn observation(proof: &Proof) -> PonsLaunchObservation {
        PonsAdapter::from_startup_identities(PonsAdapter::required_startup_identities())
            .unwrap()
            .observe_launch(PonsObservationInput {
                tx_hash: proof.transaction.hash,
                chain_id: PONS_CHAIN_ID,
                destination: proof.transaction.to.unwrap(),
                destination_runtime_hash: PONS_CURRENT_FACTORY_RUNTIME,
                calldata: &proof.transaction.input,
                value: proof.transaction.value,
                sender: proof.transaction.from,
                provenance: PonsAttributionProvenance::ExactFactoryTransaction,
            })
            .unwrap()
    }

    #[test]
    fn four_raw_calldata_trace_view_and_receipt_proofs_match_exact_prediction() {
        let predictor = predictor();
        let proofs = fixtures().proofs;
        assert_eq!(proofs.len(), 4);
        for proof in &proofs {
            let observed = observation(proof);
            let predicted = predictor
                .predict(&observed, proof.transaction.from)
                .unwrap();
            let init_code = predictor
                .build_init_code(&observed, proof.transaction.from)
                .unwrap();
            assert_eq!(proof.trace.token_create2_from, PONS_CURRENT_FACTORY);
            assert_eq!(proof.trace.token_create2_to, proof.expected_token);
            assert_eq!(proof.trace.pool_create2_from, PONS_V3_FACTORY);
            assert_eq!(proof.trace.pool_create2_to, proof.expected_pool);
            assert_eq!(proof.trace.token_init_code_bytes, init_code.len());
            assert_eq!(
                proof.trace.token_creation_prefix_bytes,
                PONS_TOKEN_CREATION_CODE_BYTES
            );
            assert_eq!(
                proof.trace.token_creation_prefix_hash,
                PONS_TOKEN_CREATION_CODE_KECCAK256
            );
            assert_eq!(
                &init_code[PONS_TOKEN_CREATION_CODE_BYTES..],
                proof.trace.constructor_args.as_ref()
            );
            assert_eq!(predicted.token, proof.expected_token);
            assert_eq!(predicted.pool, proof.expected_pool);
            assert_eq!(proof.pre_block_prediction.selector, "0xea9d3fdc");
            assert_eq!(
                proof.pre_block_prediction.l2_block_number + 1,
                proof.block.l2_block_number
            );
            assert_eq!(proof.pre_block_prediction.token, predicted.token);

            let quote = quote_pons_launch_receipt(
                &proof.transaction,
                &proof.receipt,
                crate::pons::PonsExpectedProfile::production(),
                PonsQuotePolicy {
                    amount_in: U256::from(1_000_000_000_000_000_u64),
                    max_amount_in: U256::from(10_000_000_000_000_000_u64),
                    slippage_bps: 100,
                },
            )
            .unwrap();
            assert_eq!(quote.market.token, predicted.token);
            assert_eq!(quote.market.pool, predicted.pool);
            assert_eq!(proof.receipt.block_hash, proof.block.hash);
        }
    }

    #[test]
    fn prediction_is_sensitive_to_salt_signer_metadata_and_positional_socials() {
        let predictor = predictor();
        let proof = &fixtures().proofs[0];
        let observed = observation(proof);
        let baseline = predictor
            .predict(&observed, proof.transaction.from)
            .unwrap();

        let mut changed = observed.clone();
        changed.launch.salt = B256::with_last_byte(1);
        assert_ne!(
            predictor.predict(&changed, proof.transaction.from).unwrap(),
            baseline
        );
        assert_ne!(
            predictor
                .predict(&observed, Address::with_last_byte(1))
                .unwrap(),
            baseline
        );
        changed = observed.clone();
        changed.launch.logo.push('x');
        assert_ne!(
            predictor.predict(&changed, proof.transaction.from).unwrap(),
            baseline
        );
        changed = observed.clone();
        std::mem::swap(
            &mut changed.launch.socials.telegram,
            &mut changed.launch.socials.twitter,
        );
        assert_ne!(
            predictor.predict(&changed, proof.transaction.from).unwrap(),
            baseline
        );
    }

    #[test]
    fn calldata_developer_or_fee_wallet_is_not_a_token_constructor_input() {
        let predictor = predictor();
        let proof = &fixtures().proofs[0];
        let observed = observation(proof);
        let baseline = predictor
            .predict(&observed, proof.transaction.from)
            .unwrap();
        let mut changed = observed.clone();
        changed.launch.developer_wallet = Address::with_last_byte(0xee);
        assert_eq!(
            predictor.predict(&changed, proof.transaction.from).unwrap(),
            baseline
        );
    }

    #[test]
    fn expected_observed_config_runtime_prefix_and_pool_hash_drift_fail_closed() {
        let expected = PonsPredictionSemantics::production();
        let mut observed = expected.clone();
        observed.launch_config_0.max_tx_bps += U256::from(1);
        assert_eq!(
            PonsCurrentPredictor::from_startup_profiles(&expected, &observed).unwrap_err(),
            PonsPredictionError::SemanticDrift
        );
        observed = expected.clone();
        observed.factory = Address::with_last_byte(1);
        assert_eq!(
            PonsCurrentPredictor::from_startup_profiles(&expected, &observed).unwrap_err(),
            PonsPredictionError::SemanticDrift
        );
        observed = expected.clone();
        observed.dex_config_0.factory = Address::with_last_byte(1);
        assert_eq!(
            PonsCurrentPredictor::from_startup_profiles(&expected, &observed).unwrap_err(),
            PonsPredictionError::SemanticDrift
        );
        observed = expected.clone();
        observed.token_creation_code_hash = B256::with_last_byte(1);
        assert_eq!(
            PonsCurrentPredictor::from_startup_profiles(&expected, &observed).unwrap_err(),
            PonsPredictionError::SemanticDrift
        );
        observed = expected.clone();
        observed.pool_init_code_hash = B256::with_last_byte(1);
        assert_eq!(
            PonsCurrentPredictor::from_startup_profiles(&expected, &observed).unwrap_err(),
            PonsPredictionError::SemanticDrift
        );
        let mut runtime =
            vec![0_u8; PONS_TOKEN_CREATION_CODE_OFFSET + PONS_TOKEN_CREATION_CODE_BYTES];
        assert_eq!(
            extract_creation_prefix(&runtime).unwrap_err(),
            PonsPredictionError::CreationCodeDrift
        );
        runtime[PONS_TOKEN_CREATION_CODE_OFFSET] = 1;
        assert_eq!(
            extract_creation_prefix(&runtime).unwrap_err(),
            PonsPredictionError::CreationCodeDrift
        );
    }

    #[test]
    fn fixed_boundary_getter_abi_decodes_exactly_and_rejects_trailing_data() {
        let launch = hex::decode("0000000000000000000000000bd7d308f8e1639fab988df18a8011f41eacad730000000000000000000000000000000000000000000000003a4965bf58a40000fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffce2580000000000000000000000000000000000000000033b2e3c9fd0803ce800000000000000000000000000000000000000000000000000000000000000000000c800000000000000000000000000000000000000000000000000000000000000dc000000000000000000000000000000000000000000000000000000000000016e000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000000000").unwrap();
        let dex = hex::decode("000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000e00000000000000000000000001f7d7550b1b028f7571e69a784071f0205fd2efa00000000000000000000000073991a25c818bf1f1128deaab1492d45638de0d3000000000000000000000000caf681a66d020601342297493863e78c959e5cb2000000000000000000000000000000000000000000000000000000000000271000000000000000000000000000000000000000000000000000000000000000c80000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000a756e697377617020763300000000000000000000000000000000000000000000").unwrap();
        assert_eq!(
            decode_launch_config(&launch).unwrap(),
            PonsPredictionSemantics::production().launch_config_0
        );
        assert_eq!(
            decode_dex_config(&dex).unwrap(),
            PonsPredictionSemantics::production().dex_config_0
        );
        let mut trailing = launch;
        trailing.push(0);
        assert_eq!(
            decode_launch_config(&trailing).unwrap_err(),
            PonsPredictionError::MalformedGetter
        );
        let mut trailing = dex.clone();
        trailing.push(0);
        assert_eq!(
            decode_dex_config(&trailing).unwrap_err(),
            PonsPredictionError::MalformedGetter
        );
        let mut noncanonical_padding = dex;
        *noncanonical_padding.last_mut().unwrap() = 1;
        assert_eq!(
            decode_dex_config(&noncanonical_padding).unwrap_err(),
            PonsPredictionError::MalformedGetter
        );
    }

    #[test]
    fn legacy_and_cross_generation_observations_never_predict() {
        let predictor = predictor();
        let proof = &fixtures().proofs[0];
        let mut observed = observation(proof);
        observed.generation = PonsGeneration::Legacy;
        assert_eq!(
            predictor
                .predict(&observed, proof.transaction.from)
                .unwrap_err(),
            PonsPredictionError::LegacyGeneration
        );
    }

    #[test]
    fn four_raw_calls_reach_production_observer_with_both_identities_and_no_rpc() {
        use crate::launchpad_adapter::{LaunchpadId, WrapperKind};
        use crate::paper_observer::{
            LeaderOrigin, PaperExpectedPins, PaperLaunchpadObserver, PaperObservedStartupSnapshot,
        };
        let expected: PaperExpectedPins = serde_json::from_str(include_str!(
            "../tests/fixtures/launchpad-paper-expected-pins.synthetic.json"
        ))
        .unwrap();
        let observed: PaperObservedStartupSnapshot = serde_json::from_str(include_str!(
            "../tests/fixtures/launchpad-paper-observed-startup.synthetic.json"
        ))
        .unwrap();
        let observer = PaperLaunchpadObserver::from_startup_snapshots(expected, observed).unwrap();
        for proof in &fixtures().proofs {
            let report = observer
                .observe_call(
                    proof.transaction.hash,
                    proof.transaction.from,
                    proof.transaction.from,
                    LeaderOrigin::DirectSigner,
                    WrapperKind::Direct,
                    proof.transaction.to.unwrap(),
                    proof.transaction.value,
                    &proof.transaction.input,
                )
                .unwrap();
            assert_eq!(report.launchpad, LaunchpadId::Pons);
            assert_eq!(report.predicted_token, Some(proof.expected_token));
            assert_eq!(report.predicted_pool, Some(proof.expected_pool));
            assert!(!report.live_execution_enabled);
        }
    }

    #[test]
    fn incomplete_expected_or_observed_semantic_pair_fails_startup() {
        use crate::paper_observer::{
            PaperExpectedPins, PaperLaunchpadObserver, PaperObservedStartupSnapshot,
        };
        let expected_json =
            include_str!("../tests/fixtures/launchpad-paper-expected-pins.synthetic.json");
        let observed_json =
            include_str!("../tests/fixtures/launchpad-paper-observed-startup.synthetic.json");
        let expected: PaperExpectedPins = serde_json::from_str(expected_json).unwrap();
        let mut observed: PaperObservedStartupSnapshot =
            serde_json::from_str(observed_json).unwrap();
        observed.pons_v3_semantics = None;
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected, observed).is_err());

        let mut expected_value: serde_json::Value = serde_json::from_str(expected_json).unwrap();
        expected_value["pons_v3"]
            .as_object_mut()
            .unwrap()
            .remove("prediction");
        assert!(serde_json::from_value::<PaperExpectedPins>(expected_value).is_err());
    }
}
