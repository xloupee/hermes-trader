use alloy_primitives::{Address, B256, U256, aliases::U24, keccak256};
use alloy_sol_types::{SolValue, sol};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uniswap_v3_math::{
    full_math::mul_div,
    tick_math::{MAX_TICK, MIN_TICK, get_sqrt_ratio_at_tick},
};

use crate::noxa_abi::NoxaLaunchIntent;
use crate::robinhood::{
    ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256, ACTIVE_NOXA_LAUNCH_FACTORY,
    ACTIVE_NOXA_TOKEN_CREATION_CODE_KECCAK256, NOXA_DEX_ID_UNISWAP, NOXA_FACTORY_RUNTIME_KECCAK256,
    NOXA_POOL_FEE, NOXA_RESTRICTION_L1_BLOCKS, NOXA_TICK_SPACING,
    NOXA_TOKEN_CREATION_CODE_KECCAK256, UNISWAP_V3_FACTORY, UNISWAP_V3_FACTORY_RUNTIME_KECCAK256,
    UNISWAP_V3_POOL_INIT_CODE_KECCAK256, UNISWAP_V3_POSITION_MANAGER, UNISWAP_V3_SWAP_ROUTER_02,
    WETH,
};
use crate::v3_pool::{V3PoolError, V3PoolState, V3Quote};

pub const LAUNCH_CONFIG_SELECTOR: [u8; 4] = [0x1c, 0xad, 0x86, 0x2d];
pub const DEX_CONFIG_SELECTOR: [u8; 4] = [0x71, 0x0b, 0xb9, 0x4c];
pub const TOKEN_CREATION_CODE_OFFSET: usize = 0x3264;
pub const TOKEN_CREATION_CODE_LEN: usize = 0x26ab;
pub const V3_POOL_CREATION_CODE_OFFSET: usize = 0x0703;
pub const V3_POOL_CREATION_CODE_LEN: usize = 0x58c8;
pub const ACTIVE_TOKEN_CREATION_CODE_OFFSET: usize = 10_303;
pub const ACTIVE_TOKEN_CREATION_CODE_LEN: usize = 3_734;

const BPS_DENOMINATOR: u64 = 10_000;
const Q96: U256 = U256::from_limbs([0, 1 << 32, 0, 0]);

sol! {
    struct ConstructorSocials {
        string telegram;
        string twitter;
        string discord;
        string website;
        string farcaster;
    }

    struct TokenConstructorConfig {
        string name;
        string symbol;
        uint256 supply;
        address pairToken;
        address positionManager;
        address dexFactory;
        uint24 poolFee;
        uint256 maxWalletBps;
        uint256 maxTxBps;
        uint256 restrictionBlocks;
    }

    struct TokenConstructorMetadata {
        address devWallet;
        string logo;
        string description;
        ConstructorSocials socials;
    }

    struct ActiveTokenConstructor {
        string name;
        string symbol;
        uint256 supply;
        uint16 maxWalletBps;
        uint16 maxTxBps;
        uint256 restrictionsEndBlock;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NoxaLaunchConfig {
    pub pair_token: Address,
    pub dex_id: U256,
    pub initial_tick: i32,
    pub supply: U256,
    pub max_wallet_bps: U256,
    pub max_tx_bps: U256,
    pub restriction_l1_blocks: u64,
    /// Undocumented factory flags are retained and pinned rather than ignored.
    pub flags: [bool; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NoxaDexConfig {
    pub name: String,
    pub factory: Address,
    pub position_manager: Address,
    pub swap_router: Address,
    pub pool_fee: u32,
    pub tick_spacing: i32,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PredictedNoxaLaunch {
    pub token: Address,
    pub pool: Address,
    pub restrictions_end_l1_block: u64,
    pub initial_buy_amount: U256,
    pub max_wallet_limit: U256,
    pub max_tx_limit: U256,
    #[serde(skip_serializing)]
    pub post_launch_pool: V3PoolState,
}

#[derive(Debug, Clone)]
pub struct NoxaPredictor {
    launch_factory: Address,
    launch_fee: U256,
    launch_config: NoxaLaunchConfig,
    dex_config: NoxaDexConfig,
    token_creation_code: Vec<u8>,
    pool_init_code_hash: B256,
    token_constructor: TokenConstructorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenConstructorKind {
    Legacy,
    Active,
}

#[derive(Debug, Error)]
pub enum NoxaPredictionError {
    #[error("factory configuration ABI is malformed")]
    MalformedConfiguration,
    #[error("factory configuration is unsupported or disabled")]
    UnsupportedConfiguration,
    #[error("pinned factory runtime does not contain the expected creation code")]
    RuntimeLayout,
    #[error("launch intent does not match the cached factory configuration")]
    IntentConfiguration,
    #[error("launch value does not cover the cached launch fee")]
    LaunchValue,
    #[error("prediction arithmetic overflow")]
    Arithmetic,
    #[error("predicted initial swap did not consume its full input")]
    IncompleteInitialSwap,
    #[error(transparent)]
    Pool(#[from] V3PoolError),
    #[error("Uniswap V3 arithmetic failed: {0}")]
    Math(String),
}

impl NoxaPredictor {
    pub fn new(
        launch_factory: Address,
        launch_fee: U256,
        launch_config: NoxaLaunchConfig,
        dex_config: NoxaDexConfig,
        launch_factory_runtime: &[u8],
        dex_factory_runtime: &[u8],
    ) -> Result<Self, NoxaPredictionError> {
        validate_config(&launch_config, &dex_config)?;
        if keccak256(launch_factory_runtime) != NOXA_FACTORY_RUNTIME_KECCAK256
            || keccak256(dex_factory_runtime) != UNISWAP_V3_FACTORY_RUNTIME_KECCAK256
        {
            return Err(NoxaPredictionError::RuntimeLayout);
        }
        let token_creation_code = creation_code_slice(
            launch_factory_runtime,
            TOKEN_CREATION_CODE_OFFSET,
            TOKEN_CREATION_CODE_LEN,
        )?
        .to_vec();
        let pool_creation_code = creation_code_slice(
            dex_factory_runtime,
            V3_POOL_CREATION_CODE_OFFSET,
            V3_POOL_CREATION_CODE_LEN,
        )?;
        if keccak256(&token_creation_code) != NOXA_TOKEN_CREATION_CODE_KECCAK256
            || keccak256(pool_creation_code) != UNISWAP_V3_POOL_INIT_CODE_KECCAK256
        {
            return Err(NoxaPredictionError::RuntimeLayout);
        }
        Ok(Self {
            launch_factory,
            launch_fee,
            launch_config,
            dex_config,
            token_creation_code,
            pool_init_code_hash: keccak256(pool_creation_code),
            token_constructor: TokenConstructorKind::Legacy,
        })
    }

    /// Build a predictor for the independently audited active N0xa deployment.
    /// The deployment uses the same launch config and V3 infrastructure, but a
    /// different LaunchToken constructor and a sender-derived CREATE2 salt.
    pub fn new_active(
        launch_fee: U256,
        launch_config: NoxaLaunchConfig,
        dex_config: NoxaDexConfig,
        launch_factory_runtime: &[u8],
        dex_factory_runtime: &[u8],
    ) -> Result<Self, NoxaPredictionError> {
        validate_config(&launch_config, &dex_config)?;
        if keccak256(launch_factory_runtime) != ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256
            || keccak256(dex_factory_runtime) != UNISWAP_V3_FACTORY_RUNTIME_KECCAK256
        {
            return Err(NoxaPredictionError::RuntimeLayout);
        }
        let token_creation_code = creation_code_slice(
            launch_factory_runtime,
            ACTIVE_TOKEN_CREATION_CODE_OFFSET,
            ACTIVE_TOKEN_CREATION_CODE_LEN,
        )?
        .to_vec();
        let pool_creation_code = creation_code_slice(
            dex_factory_runtime,
            V3_POOL_CREATION_CODE_OFFSET,
            V3_POOL_CREATION_CODE_LEN,
        )?;
        if keccak256(&token_creation_code) != ACTIVE_NOXA_TOKEN_CREATION_CODE_KECCAK256
            || keccak256(pool_creation_code) != UNISWAP_V3_POOL_INIT_CODE_KECCAK256
        {
            return Err(NoxaPredictionError::RuntimeLayout);
        }
        Ok(Self {
            launch_factory: ACTIVE_NOXA_LAUNCH_FACTORY,
            launch_fee,
            launch_config,
            dex_config,
            token_creation_code,
            pool_init_code_hash: keccak256(pool_creation_code),
            token_constructor: TokenConstructorKind::Active,
        })
    }

    pub fn launch_config(&self) -> &NoxaLaunchConfig {
        &self.launch_config
    }

    pub fn launch_fee(&self) -> U256 {
        self.launch_fee
    }

    pub fn dex_config(&self) -> &NoxaDexConfig {
        &self.dex_config
    }

    pub fn token_creation_code_hash(&self) -> B256 {
        keccak256(&self.token_creation_code)
    }

    pub fn pool_init_code_hash(&self) -> B256 {
        self.pool_init_code_hash
    }

    pub fn predict(
        &self,
        intent: &NoxaLaunchIntent,
        launch_l1_block: u64,
    ) -> Result<PredictedNoxaLaunch, NoxaPredictionError> {
        if self.token_constructor != TokenConstructorKind::Legacy {
            return Err(NoxaPredictionError::IntentConfiguration);
        }
        self.predict_with_salt(intent, intent.salt, launch_l1_block)
    }

    /// The active N0xa factory salts CREATE2 with `keccak256(abi.encode(sender,
    /// suppliedSalt))`, so the recovered feed sender is part of the prediction.
    pub fn predict_active(
        &self,
        intent: &NoxaLaunchIntent,
        sender: Address,
        launch_l1_block: u64,
    ) -> Result<PredictedNoxaLaunch, NoxaPredictionError> {
        if self.token_constructor != TokenConstructorKind::Active {
            return Err(NoxaPredictionError::IntentConfiguration);
        }
        let mut encoded = [0_u8; 64];
        encoded[12..32].copy_from_slice(sender.as_slice());
        encoded[32..].copy_from_slice(intent.salt.as_slice());
        self.predict_with_salt(intent, keccak256(encoded), launch_l1_block)
    }

    fn predict_with_salt(
        &self,
        intent: &NoxaLaunchIntent,
        create2_salt: B256,
        launch_l1_block: u64,
    ) -> Result<PredictedNoxaLaunch, NoxaPredictionError> {
        if intent.dex_id != self.launch_config.dex_id {
            return Err(NoxaPredictionError::IntentConfiguration);
        }
        let restrictions_end_l1_block = launch_l1_block
            .checked_add(self.launch_config.restriction_l1_blocks)
            .ok_or(NoxaPredictionError::Arithmetic)?;
        let constructor = match self.token_constructor {
            TokenConstructorKind::Legacy => encode_token_constructor(
                intent,
                &self.launch_config,
                &self.dex_config,
                restrictions_end_l1_block,
            ),
            TokenConstructorKind::Active => encode_active_token_constructor(
                intent,
                &self.launch_config,
                restrictions_end_l1_block,
            ),
        };
        let mut init_code = Vec::with_capacity(self.token_creation_code.len() + constructor.len());
        init_code.extend_from_slice(&self.token_creation_code);
        init_code.extend_from_slice(&constructor);
        let token = create2_address(self.launch_factory, create2_salt, keccak256(&init_code));
        let (token0, token1) = sorted_tokens(self.launch_config.pair_token, token);
        let pool = predict_v3_pool_address(
            self.dex_config.factory,
            token0,
            token1,
            self.dex_config.pool_fee,
            self.pool_init_code_hash,
        );
        let initial_buy_amount = intent
            .transaction_value
            .checked_sub(self.launch_fee)
            .ok_or(NoxaPredictionError::LaunchValue)?;
        let max_wallet_limit =
            basis_points(self.launch_config.supply, self.launch_config.max_wallet_bps)?;
        let max_tx_limit = basis_points(self.launch_config.supply, self.launch_config.max_tx_bps)?;
        let post_launch_pool = build_post_launch_pool(
            pool,
            token,
            &self.launch_config,
            &self.dex_config,
            initial_buy_amount,
        )?;
        Ok(PredictedNoxaLaunch {
            token,
            pool,
            restrictions_end_l1_block,
            initial_buy_amount,
            max_wallet_limit,
            max_tx_limit,
            post_launch_pool,
        })
    }
}

impl PredictedNoxaLaunch {
    pub fn quote_entry(
        &self,
        pair_token: Address,
        amount_in: U256,
    ) -> Result<V3Quote, NoxaPredictionError> {
        Ok(self
            .post_launch_pool
            .quote_exact_input(pair_token, amount_in, None)?)
    }
}

pub fn config_call(selector: [u8; 4], id: U256) -> Vec<u8> {
    let mut call = Vec::with_capacity(36);
    call.extend_from_slice(&selector);
    call.extend_from_slice(&id.to_be_bytes::<32>());
    call
}

pub fn decode_launch_config(bytes: &[u8]) -> Result<NoxaLaunchConfig, NoxaPredictionError> {
    if bytes.len() != 10 * 32 {
        return Err(NoxaPredictionError::MalformedConfiguration);
    }
    let words = words(bytes)?;
    Ok(NoxaLaunchConfig {
        pair_token: address_word(words[0])?,
        dex_id: word(words[1]),
        initial_tick: signed_i24(words[2])?,
        supply: word(words[3]),
        max_wallet_bps: word(words[4]),
        max_tx_bps: word(words[5]),
        restriction_l1_blocks: u64::try_from(word(words[6]))
            .map_err(|_| NoxaPredictionError::MalformedConfiguration)?,
        flags: [
            bool_word(words[7])?,
            bool_word(words[8])?,
            bool_word(words[9])?,
        ],
    })
}

pub fn decode_dex_config(bytes: &[u8]) -> Result<NoxaDexConfig, NoxaPredictionError> {
    if bytes.len() < 9 * 32 || word_at(bytes, 0)? != U256::from(32) {
        return Err(NoxaPredictionError::MalformedConfiguration);
    }
    let tuple = &bytes[32..];
    let name_offset = usize::try_from(word_at(tuple, 0)?)
        .map_err(|_| NoxaPredictionError::MalformedConfiguration)?;
    let name = decode_string(tuple, name_offset)?;
    Ok(NoxaDexConfig {
        name,
        factory: address_at(tuple, 1)?,
        position_manager: address_at(tuple, 2)?,
        swap_router: address_at(tuple, 3)?,
        pool_fee: u32::try_from(word_at(tuple, 4)?)
            .map_err(|_| NoxaPredictionError::MalformedConfiguration)?,
        tick_spacing: i32::try_from(word_at(tuple, 5)?)
            .map_err(|_| NoxaPredictionError::MalformedConfiguration)?,
        enabled: bool_at(tuple, 6)?,
    })
}

/// The active N0xa factory returns its DEX tuple directly (without the legacy
/// display-name string at ABI offset zero).
pub fn decode_active_dex_config(bytes: &[u8]) -> Result<NoxaDexConfig, NoxaPredictionError> {
    if bytes.len() != 6 * 32 {
        return Err(NoxaPredictionError::MalformedConfiguration);
    }
    Ok(NoxaDexConfig {
        name: "uniswap".to_owned(),
        factory: address_at(bytes, 0)?,
        position_manager: address_at(bytes, 1)?,
        swap_router: address_at(bytes, 2)?,
        pool_fee: u32::try_from(word_at(bytes, 3)?)
            .map_err(|_| NoxaPredictionError::MalformedConfiguration)?,
        tick_spacing: i32::try_from(word_at(bytes, 4)?)
            .map_err(|_| NoxaPredictionError::MalformedConfiguration)?,
        enabled: bool_at(bytes, 5)?,
    })
}

pub fn create2_address(factory: Address, salt: B256, init_code_hash: B256) -> Address {
    let mut encoded = [0_u8; 85];
    encoded[0] = 0xff;
    encoded[1..21].copy_from_slice(factory.as_slice());
    encoded[21..53].copy_from_slice(salt.as_slice());
    encoded[53..].copy_from_slice(init_code_hash.as_slice());
    Address::from_slice(&keccak256(encoded).as_slice()[12..])
}

pub fn predict_v3_pool_address(
    factory: Address,
    token0: Address,
    token1: Address,
    fee: u32,
    init_code_hash: B256,
) -> Address {
    let mut key = [0_u8; 96];
    key[12..32].copy_from_slice(token0.as_slice());
    key[44..64].copy_from_slice(token1.as_slice());
    key[92..96].copy_from_slice(&fee.to_be_bytes());
    create2_address(factory, keccak256(key), init_code_hash)
}

fn validate_config(
    launch: &NoxaLaunchConfig,
    dex: &NoxaDexConfig,
) -> Result<(), NoxaPredictionError> {
    let pinned_supply = U256::from(1_000_000_000_000_000_000_u128) * U256::from(1_000_000_000_u64);
    if launch.pair_token != WETH
        || launch.dex_id != U256::from(NOXA_DEX_ID_UNISWAP)
        || launch.initial_tick != -204_200
        || launch.supply != pinned_supply
        || launch.max_wallet_bps != U256::from(200)
        || launch.max_tx_bps != U256::from(BPS_DENOMINATOR)
        || launch.restriction_l1_blocks != NOXA_RESTRICTION_L1_BLOCKS
        || launch.supply == U256::ZERO
        || launch.max_wallet_bps == U256::ZERO
        || launch.max_wallet_bps > U256::from(BPS_DENOMINATOR)
        || launch.max_tx_bps == U256::ZERO
        || launch.max_tx_bps > U256::from(BPS_DENOMINATOR)
        || launch.restriction_l1_blocks == 0
        || launch.initial_tick < MIN_TICK
        || launch.initial_tick > MAX_TICK
        || launch.flags != [false, true, false]
        || dex.name != "uniswap"
        || dex.factory != UNISWAP_V3_FACTORY
        || dex.position_manager != UNISWAP_V3_POSITION_MANAGER
        || dex.swap_router != UNISWAP_V3_SWAP_ROUTER_02
        || dex.pool_fee != NOXA_POOL_FEE
        || dex.tick_spacing != NOXA_TICK_SPACING
        || !dex.enabled
    {
        return Err(NoxaPredictionError::UnsupportedConfiguration);
    }
    Ok(())
}

fn encode_token_constructor(
    intent: &NoxaLaunchIntent,
    launch: &NoxaLaunchConfig,
    dex: &NoxaDexConfig,
    _restrictions_end_l1_block: u64,
) -> Vec<u8> {
    let config = TokenConstructorConfig {
        name: intent.name.clone(),
        symbol: intent.symbol.clone(),
        supply: launch.supply,
        pairToken: launch.pair_token,
        positionManager: dex.position_manager,
        dexFactory: dex.factory,
        poolFee: U24::from(dex.pool_fee),
        maxWalletBps: launch.max_wallet_bps,
        maxTxBps: launch.max_tx_bps,
        // The token constructor receives a duration. NUMBER is the parent-L1
        // height on Nitro, so the token derives the same end height emitted by
        // the launch factory without the factory passing an absolute value.
        restrictionBlocks: U256::from(launch.restriction_l1_blocks),
    };
    let metadata = TokenConstructorMetadata {
        devWallet: intent.dev_wallet,
        logo: intent.logo.clone(),
        description: intent.description.clone(),
        socials: ConstructorSocials {
            telegram: intent.socials.telegram.clone(),
            twitter: intent.socials.twitter.clone(),
            discord: intent.socials.discord.clone(),
            website: intent.socials.website.clone(),
            farcaster: intent.socials.farcaster.clone(),
        },
    };
    (config, metadata).abi_encode_params()
}

fn encode_active_token_constructor(
    intent: &NoxaLaunchIntent,
    launch: &NoxaLaunchConfig,
    restrictions_end_l1_block: u64,
) -> Vec<u8> {
    ActiveTokenConstructor {
        name: intent.name.clone(),
        symbol: intent.symbol.clone(),
        supply: launch.supply,
        maxWalletBps: u16::try_from(launch.max_wallet_bps).unwrap_or_default(),
        maxTxBps: u16::try_from(launch.max_tx_bps).unwrap_or_default(),
        restrictionsEndBlock: U256::from(restrictions_end_l1_block),
    }
    .abi_encode_params()
}

fn build_post_launch_pool(
    pool: Address,
    token: Address,
    launch: &NoxaLaunchConfig,
    dex: &NoxaDexConfig,
    initial_buy_amount: U256,
) -> Result<V3PoolState, NoxaPredictionError> {
    let (token0, token1) = sorted_tokens(launch.pair_token, token);
    let launched_is_token0 = token == token0;
    let initial_tick = if launched_is_token0 {
        launch.initial_tick
    } else {
        launch
            .initial_tick
            .checked_neg()
            .ok_or(NoxaPredictionError::Arithmetic)?
    };
    if initial_tick % dex.tick_spacing != 0 {
        return Err(NoxaPredictionError::UnsupportedConfiguration);
    }
    let min_usable = (MIN_TICK / dex.tick_spacing) * dex.tick_spacing;
    let max_usable = (MAX_TICK / dex.tick_spacing) * dex.tick_spacing;
    let (tick_lower, tick_upper) = if launched_is_token0 {
        (initial_tick, max_usable)
    } else {
        (min_usable, initial_tick)
    };
    let sqrt_lower = get_sqrt_ratio_at_tick(tick_lower)
        .map_err(|error| NoxaPredictionError::Math(error.to_string()))?;
    let sqrt_upper = get_sqrt_ratio_at_tick(tick_upper)
        .map_err(|error| NoxaPredictionError::Math(error.to_string()))?;
    let liquidity = if launched_is_token0 {
        let intermediate = mul_div(sqrt_lower, sqrt_upper, Q96)
            .map_err(|error| NoxaPredictionError::Math(error.to_string()))?;
        mul_div(launch.supply, intermediate, sqrt_upper - sqrt_lower)
            .map_err(|error| NoxaPredictionError::Math(error.to_string()))?
    } else {
        mul_div(launch.supply, Q96, sqrt_upper - sqrt_lower)
            .map_err(|error| NoxaPredictionError::Math(error.to_string()))?
    };
    let liquidity = u128::try_from(liquidity).map_err(|_| NoxaPredictionError::Arithmetic)?;
    let initial_sqrt = get_sqrt_ratio_at_tick(initial_tick)
        .map_err(|error| NoxaPredictionError::Math(error.to_string()))?;
    let mut state = V3PoolState::new(
        pool,
        token0,
        token1,
        dex.pool_fee,
        dex.tick_spacing,
        initial_sqrt,
        initial_tick,
        0,
    )?;
    state.add_position(tick_lower, tick_upper, liquidity)?;
    if initial_buy_amount != U256::ZERO {
        let quote = state.quote_exact_input(launch.pair_token, initial_buy_amount, None)?;
        if quote.amount_in_consumed != initial_buy_amount {
            return Err(NoxaPredictionError::IncompleteInitialSwap);
        }
        state.set_observation(
            quote.sqrt_price_x96_after,
            quote.tick_after,
            quote.liquidity_after,
        )?;
    }
    Ok(state)
}

fn basis_points(value: U256, bps: U256) -> Result<U256, NoxaPredictionError> {
    value
        .checked_mul(bps)
        .map(|product| product / U256::from(BPS_DENOMINATOR))
        .ok_or(NoxaPredictionError::Arithmetic)
}

fn creation_code_slice(
    runtime: &[u8],
    offset: usize,
    len: usize,
) -> Result<&[u8], NoxaPredictionError> {
    runtime
        .get(
            offset
                ..offset
                    .checked_add(len)
                    .ok_or(NoxaPredictionError::RuntimeLayout)?,
        )
        .ok_or(NoxaPredictionError::RuntimeLayout)
}

fn words(bytes: &[u8]) -> Result<Vec<&[u8]>, NoxaPredictionError> {
    if !bytes.len().is_multiple_of(32) {
        return Err(NoxaPredictionError::MalformedConfiguration);
    }
    Ok(bytes.chunks_exact(32).collect())
}

fn word(bytes: &[u8]) -> U256 {
    U256::from_be_slice(bytes)
}

fn word_at(bytes: &[u8], index: usize) -> Result<U256, NoxaPredictionError> {
    let start = index
        .checked_mul(32)
        .ok_or(NoxaPredictionError::MalformedConfiguration)?;
    let value = bytes
        .get(start..start + 32)
        .ok_or(NoxaPredictionError::MalformedConfiguration)?;
    Ok(word(value))
}

fn address_word(bytes: &[u8]) -> Result<Address, NoxaPredictionError> {
    if bytes.len() != 32 || bytes[..12].iter().any(|byte| *byte != 0) {
        return Err(NoxaPredictionError::MalformedConfiguration);
    }
    Ok(Address::from_slice(&bytes[12..]))
}

fn address_at(bytes: &[u8], index: usize) -> Result<Address, NoxaPredictionError> {
    let start = index
        .checked_mul(32)
        .ok_or(NoxaPredictionError::MalformedConfiguration)?;
    address_word(
        bytes
            .get(start..start + 32)
            .ok_or(NoxaPredictionError::MalformedConfiguration)?,
    )
}

fn bool_word(bytes: &[u8]) -> Result<bool, NoxaPredictionError> {
    match word(bytes) {
        value if value == U256::ZERO => Ok(false),
        value if value == U256::from(1) => Ok(true),
        _ => Err(NoxaPredictionError::MalformedConfiguration),
    }
}

fn bool_at(bytes: &[u8], index: usize) -> Result<bool, NoxaPredictionError> {
    let start = index
        .checked_mul(32)
        .ok_or(NoxaPredictionError::MalformedConfiguration)?;
    bool_word(
        bytes
            .get(start..start + 32)
            .ok_or(NoxaPredictionError::MalformedConfiguration)?,
    )
}

fn signed_i24(bytes: &[u8]) -> Result<i32, NoxaPredictionError> {
    if bytes.len() != 32 {
        return Err(NoxaPredictionError::MalformedConfiguration);
    }
    let sign = bytes[29] & 0x80 != 0;
    let padding = if sign { 0xff } else { 0x00 };
    if bytes[..29].iter().any(|byte| *byte != padding) {
        return Err(NoxaPredictionError::MalformedConfiguration);
    }
    let raw = (i32::from(bytes[29]) << 16) | (i32::from(bytes[30]) << 8) | i32::from(bytes[31]);
    Ok(if sign { raw | !0x00ff_ffff } else { raw })
}

fn decode_string(bytes: &[u8], offset: usize) -> Result<String, NoxaPredictionError> {
    let len = usize::try_from(word_at(
        bytes,
        offset
            .checked_div(32)
            .ok_or(NoxaPredictionError::MalformedConfiguration)?,
    )?)
    .map_err(|_| NoxaPredictionError::MalformedConfiguration)?;
    if !offset.is_multiple_of(32) {
        return Err(NoxaPredictionError::MalformedConfiguration);
    }
    let start = offset
        .checked_add(32)
        .ok_or(NoxaPredictionError::MalformedConfiguration)?;
    let raw = bytes
        .get(
            start
                ..start
                    .checked_add(len)
                    .ok_or(NoxaPredictionError::MalformedConfiguration)?,
        )
        .ok_or(NoxaPredictionError::MalformedConfiguration)?;
    String::from_utf8(raw.to_vec()).map_err(|_| NoxaPredictionError::MalformedConfiguration)
}

fn sorted_tokens(left: Address, right: Address) -> (Address, Address) {
    if left < right {
        (left, right)
    } else {
        (right, left)
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{address, b256};

    use super::*;

    #[test]
    fn decodes_live_robinhood_launch_config() {
        let bytes = hex::decode(concat!(
            "0000000000000000000000000bd7d308f8e1639fab988df18a8011f41eacad73",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffce258",
            "0000000000000000000000000000000000000000033b2e3c9fd0803ce8000000",
            "00000000000000000000000000000000000000000000000000000000000000c8",
            "0000000000000000000000000000000000000000000000000000000000002710",
            "000000000000000000000000000000000000000000000000000000000000016e",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000001",
            "0000000000000000000000000000000000000000000000000000000000000000"
        ))
        .unwrap();
        let config = decode_launch_config(&bytes).unwrap();
        assert_eq!(config.initial_tick, -204_200);
        assert_eq!(config.restriction_l1_blocks, 366);
        assert_eq!(config.flags, [false, true, false]);
    }

    #[test]
    fn reconstructs_live_post_launch_pool_without_receipt_events() {
        let launch = NoxaLaunchConfig {
            pair_token: WETH,
            dex_id: U256::ZERO,
            initial_tick: -204_200,
            supply: U256::from(1_000_000_000_000_000_000_u128) * U256::from(1_000_000_000_u64),
            max_wallet_bps: U256::from(200),
            max_tx_bps: U256::from(10_000),
            restriction_l1_blocks: 366,
            flags: [false, true, false],
        };
        let dex = NoxaDexConfig {
            name: "uniswap".into(),
            factory: UNISWAP_V3_FACTORY,
            position_manager: UNISWAP_V3_POSITION_MANAGER,
            swap_router: UNISWAP_V3_SWAP_ROUTER_02,
            pool_fee: NOXA_POOL_FEE,
            tick_spacing: NOXA_TICK_SPACING,
            enabled: true,
        };
        let state = build_post_launch_pool(
            address!("efd703d89b7febc0ae43fdd72edd257819366272"),
            address!("955b339944cbd4834156366d766c260c80956b44"),
            &launch,
            &dex,
            U256::from_str_radix("b1a2bc2ec50000", 16).unwrap(),
        )
        .unwrap();
        assert_eq!(state.tick, 203_482);
        assert_eq!(
            state.sqrt_price_x96,
            U256::from_str_radix("665aef7589c635534122e931e613", 16).unwrap()
        );
        assert_eq!(
            state.liquidity,
            u128::from_str_radix("7cbf9d9985f0629c56e", 16).unwrap()
        );
    }

    #[test]
    fn predicts_known_v3_pool_from_extracted_init_hash() {
        let factory = address!("1f7d7550b1b028f7571e69a784071f0205fd2efa");
        let token0 = address!("0bd7d308f8e1639fab988df18a8011f41eacad73");
        let token1 = address!("955b339944cbd4834156366d766c260c80956b44");
        let init_hash = UNISWAP_V3_POOL_INIT_CODE_KECCAK256;
        let predicted = predict_v3_pool_address(factory, token0, token1, 10_000, init_hash);
        assert_eq!(
            predicted,
            address!("efd703d89b7febc0ae43fdd72edd257819366272")
        );
    }

    #[test]
    fn create2_uses_canonical_preimage() {
        // EIP-1014 example 0: deployer=0, salt=0, init_code=0x00.
        let factory = Address::ZERO;
        let salt = B256::ZERO;
        let init_hash = b256!("bc36789e7a1e281436464229828f817d6612f7b477d66591ff96a9e064bcc98a");
        let address = create2_address(factory, salt, init_hash);
        assert_eq!(
            address,
            address!("4d1a2e2bb4f88f0250f26ffff098b0b30b26bf38")
        );
    }
}
