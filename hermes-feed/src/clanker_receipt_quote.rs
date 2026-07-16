//! Strict, receipt-end Clanker V4 paper quotes.
//!
//! This module performs no RPC, signing, or execution. It accepts an already
//! confirmed transaction, receipt, and block, reconstructs the exact V4 pool
//! liquidity created by that receipt, and simulates the first timestamp at
//! which Clanker's pinned descending-fee profile permits a swap.

use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::SolEvent;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::launchpad_adapter::LaunchpadId;
use crate::launchpad_adapters::{
    CLANKER_DEPLOY_SELECTOR, CLANKER_FACTORY, CLANKER_FACTORY_RUNTIME_HASH, CLANKER_LOCKER,
    V4_POOL_MANAGER,
};
use crate::noxa_abi::ReceiptLog;
use crate::noxa_rpc::{NoxaReceipt, RobinhoodBlock, RobinhoodTransaction};
use crate::robinhood::{CHAIN_ID, WETH};
use crate::uniswap_v4::{CodePin, DYNAMIC_FEE_FLAG, V4PoolKey};
use crate::v3_pool::{V3PoolError, V3PoolState, V3Quote};

const BPS_DENOMINATOR: u16 = 10_000;
const FEE_DENOMINATOR: u32 = 1_000_000;
const FEE_SCALE: u64 = 1_000_000_000_000_000_000;

pub const CLANKER_STATIC_HOOK: Address =
    alloy_primitives::address!("48b8f6ad3a1b4aa477314c9a23035b8f84dde8cc");
pub const CLANKER_DESCENDING_MEV_MODULE: Address =
    alloy_primitives::address!("ea1fe197df140e5d88fc6b49f2d21ea05092299e");
pub const CLANKER_EXTENSION: Address =
    alloy_primitives::address!("6f27372ff493a3855e6746b9a4fe6ed2cc3034b5");

pub const V4_POOL_MANAGER_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("bd3881180b547f5fe817545743cfb4343e96b1bc6640dcd70c106b0066e95626");
pub const CLANKER_STATIC_HOOK_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("0883056c4856f8fe464ff49f9c1c028455459dad8ceddcc6d5159259fe51e07f");
pub const CLANKER_LOCKER_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("2175e20d41bc72ad6596b2fdd2c43c75e9d8ca10a706a1ca6c1a3d1526c336bc");
pub const CLANKER_DESCENDING_MEV_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("0815a0af5e056adaf07a1941b92082caa886b207676bd42c89ea6bde3956bc13");
pub const CLANKER_EXTENSION_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("f742a12de7ec06481d0e98942d1830d8bf33502d854e5d97062ef5fda6f5e004");

mod events {
    use alloy_sol_types::sol;

    sol! {
        event TokenCreated(
            address msgSender,
            address indexed tokenAddress,
            address indexed tokenAdmin,
            string tokenImage,
            string tokenName,
            string tokenSymbol,
            string tokenMetadata,
            string tokenContext,
            int24 startingTick,
            address poolHook,
            bytes32 poolId,
            address pairedToken,
            address locker,
            address mevModule,
            uint256 extensionsSupply,
            address[] extensions
        );

        event Initialize(
            bytes32 indexed id,
            address indexed currency0,
            address indexed currency1,
            uint24 fee,
            int24 tickSpacing,
            address hooks,
            uint160 sqrtPriceX96,
            int24 tick
        );

        event ModifyLiquidity(
            bytes32 indexed id,
            address indexed sender,
            int24 tickLower,
            int24 tickUpper,
            int256 liquidityDelta,
            bytes32 salt
        );

        event Swap(
            bytes32 indexed id,
            address indexed sender,
            int128 amount0,
            int128 amount1,
            uint160 sqrtPriceX96,
            uint128 liquidity,
            int24 tick,
            uint24 fee
        );

        event PoolInitialized(bytes32 poolId, uint24 clankerFee, uint24 pairedFee);
        event FeeConfigSet(
            bytes32 poolId,
            uint24 startingFee,
            uint24 endingFee,
            uint256 secondsToDecay
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct ClankerV4ExpectedProfile {
    pub factory: CodePin,
    pub pool_manager: CodePin,
    pub hook: CodePin,
    pub locker: CodePin,
    pub mev_module: CodePin,
    pub extension: CodePin,
    pub max_static_fee_ppm: u32,
    pub max_mev_fee_ppm: u32,
    pub max_mev_seconds_to_decay: u64,
    pub mev_delay_guard_seconds: u64,
    pub protocol_fee_share_percent: u8,
}

impl ClankerV4ExpectedProfile {
    /// Independently reviewed from the official Clanker V4 contracts and
    /// historical chain-4663 runtime commitments. Fresh startup code hashes
    /// must still match this profile before the observer is admitted.
    pub const fn production() -> Self {
        Self {
            factory: CodePin {
                address: CLANKER_FACTORY,
                runtime_code_hash: CLANKER_FACTORY_RUNTIME_HASH,
            },
            pool_manager: CodePin {
                address: V4_POOL_MANAGER,
                runtime_code_hash: V4_POOL_MANAGER_RUNTIME_HASH,
            },
            hook: CodePin {
                address: CLANKER_STATIC_HOOK,
                runtime_code_hash: CLANKER_STATIC_HOOK_RUNTIME_HASH,
            },
            locker: CodePin {
                address: CLANKER_LOCKER,
                runtime_code_hash: CLANKER_LOCKER_RUNTIME_HASH,
            },
            mev_module: CodePin {
                address: CLANKER_DESCENDING_MEV_MODULE,
                runtime_code_hash: CLANKER_DESCENDING_MEV_RUNTIME_HASH,
            },
            extension: CodePin {
                address: CLANKER_EXTENSION,
                runtime_code_hash: CLANKER_EXTENSION_RUNTIME_HASH,
            },
            max_static_fee_ppm: 100_000,
            max_mev_fee_ppm: 800_000,
            max_mev_seconds_to_decay: 120,
            mev_delay_guard_seconds: 1,
            protocol_fee_share_percent: 20,
        }
    }

    pub fn validate(self) -> Result<(), ClankerQuoteError> {
        if !self.factory.is_complete()
            || self.factory.address != CLANKER_FACTORY
            || !self.pool_manager.is_complete()
            || self.pool_manager.address != V4_POOL_MANAGER
            || !self.hook.is_complete()
            || self.hook.address != CLANKER_STATIC_HOOK
            || !self.locker.is_complete()
            || self.locker.address != CLANKER_LOCKER
            || !self.mev_module.is_complete()
            || self.mev_module.address != CLANKER_DESCENDING_MEV_MODULE
            || !self.extension.is_complete()
            || self.extension.address != CLANKER_EXTENSION
            || self.max_static_fee_ppm != 100_000
            || self.max_mev_fee_ppm != 800_000
            || self.max_mev_seconds_to_decay != 120
            || self.mev_delay_guard_seconds != 1
            || self.protocol_fee_share_percent != 20
        {
            return Err(ClankerQuoteError::InvalidExpectedProfile);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct ClankerQuotePolicy {
    pub amount_in: U256,
    pub max_amount_in: U256,
    pub slippage_bps: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct ClankerMevFeeConfig {
    pub starting_fee_ppm: u32,
    pub ending_fee_ppm: u32,
    pub seconds_to_decay: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct ClankerStaticFeeConfig {
    pub clanker_fee_ppm: u32,
    pub paired_fee_ppm: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ClankerStateVersion {
    pub chain_id: u64,
    pub block_hash: B256,
    pub l2_block_number: u64,
    pub transaction_index: u64,
    pub terminal_log_index: u64,
    pub receipt_timestamp: u64,
    pub first_eligible_quote_timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ClankerMarketEvidence {
    pub token: Address,
    pub token_admin: Address,
    pub pool_id: B256,
    pub pool_manager: Address,
    pub quote_asset: Address,
    pub hook: Address,
    pub locker: Address,
    pub mev_module: Address,
    pub extension: Address,
    pub dynamic_fee_flag: u32,
    pub tick_spacing: i32,
    pub starting_tick: i32,
    pub initialize_tick: i32,
    pub initialize_log_index: u64,
    pub last_liquidity_log_index: u64,
    pub launch_log_index: u64,
    pub position_count: usize,
    pub static_fee_config: ClankerStaticFeeConfig,
    pub mev_fee_config: ClankerMevFeeConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ClankerPaperSwapQuote {
    pub amount_in: U256,
    pub hook_protocol_fee: U256,
    pub core_amount_in: U256,
    pub expected_output: U256,
    pub min_receive: U256,
    pub slippage_bps: u16,
    pub lp_fee_ppm: u32,
    pub protocol_fee_ppm: u32,
    pub core_state_after: V3Quote,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ClankerReceiptPaperQuote {
    pub record_type: String,
    pub tx_hash: B256,
    pub launchpad: LaunchpadId,
    pub l2_block_number: u64,
    pub state_version: ClankerStateVersion,
    pub quote_source: String,
    pub sizing_source: String,
    pub market: ClankerMarketEvidence,
    pub entry: ClankerPaperSwapQuote,
    pub full_position_exit: ClankerPaperSwapQuote,
    pub simulated_round_trip_return_bps: U256,
    pub execution_eligible: bool,
    pub execution_blocker: String,
    pub broadcast: bool,
}

#[derive(Debug, Error)]
pub enum ClankerQuoteError {
    #[error("expected Clanker profile is incomplete or invalid")]
    InvalidExpectedProfile,
    #[error("receipt, transaction, or block envelope does not match")]
    InvalidEnvelope,
    #[error("paper sizing or slippage policy is unsafe")]
    UnsafePolicy,
    #[error("receipt logs are not strictly ordered")]
    UnorderedLogs,
    #[error("receipt must contain exactly one exact Clanker TokenCreated event")]
    TokenCreatedIdentity,
    #[error("TokenCreated market identity does not match the expected profile")]
    MarketIdentity,
    #[error("V4 PoolKey or Initialize event is missing or inconsistent")]
    InitializeIdentity,
    #[error("Clanker hook fee initialization is missing or inconsistent")]
    HookConfiguration,
    #[error("Clanker descending-MEV fee configuration is missing or inconsistent")]
    MevConfiguration,
    #[error("launch liquidity events are incomplete, negative, or inconsistent")]
    LiquiditySequence,
    #[error("launch receipt contains a pool swap; this quote profile requires zero initial buy")]
    EmbeddedSwapUnsupported,
    #[error("paper quote did not consume all input or returned zero output")]
    IncompleteQuote,
    #[error("paper quote arithmetic overflowed")]
    ArithmeticOverflow,
    #[error(transparent)]
    Pool(#[from] V3PoolError),
}

#[derive(Debug, Clone, Copy)]
struct LaunchIdentity {
    sender: Address,
    token: Address,
    token_admin: Address,
    starting_tick: i32,
    hook: Address,
    pool_id: B256,
    paired_token: Address,
    locker: Address,
    mev_module: Address,
    extension: Address,
    extensions_supply: U256,
    log_index: u64,
}

#[derive(Debug, Clone, Copy)]
struct Position {
    tick_lower: i32,
    tick_upper: i32,
    liquidity: u128,
    log_index: u64,
}

/// Reconstruct a zero-initial-buy Clanker launch and quote the first eligible
/// WETH entry plus an immediate full-position exit. The output is evidence
/// only and can never enable execution.
pub fn quote_clanker_launch_receipt(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
    profile: ClankerV4ExpectedProfile,
    policy: ClankerQuotePolicy,
) -> Result<ClankerReceiptPaperQuote, ClankerQuoteError> {
    profile.validate()?;
    validate_envelope(transaction, receipt, block, policy)?;
    let launch = exact_launch_identity(&receipt.logs)?;
    if launch.sender != transaction.from
        || launch.token == Address::ZERO
        || launch.token == WETH
        || launch.token_admin == Address::ZERO
        || launch.paired_token != WETH
        || launch.hook != profile.hook.address
        || launch.locker != profile.locker.address
        || launch.mev_module != profile.mev_module.address
        || launch.extension != profile.extension.address
        || launch.extensions_supply == U256::ZERO
    {
        return Err(ClankerQuoteError::MarketIdentity);
    }

    let key = V4PoolKey::canonical(
        WETH,
        launch.token,
        DYNAMIC_FEE_FLAG,
        200,
        profile.hook.address,
    )
    .map_err(|_| ClankerQuoteError::InitializeIdentity)?;
    if key.pool_id() != launch.pool_id {
        return Err(ClankerQuoteError::InitializeIdentity);
    }

    let mut initialize = None;
    let mut positions = Vec::new();
    let mut pool_swaps = 0_usize;
    for log in &receipt.logs {
        if log.address != profile.pool_manager.address {
            continue;
        }
        let Some(topic) = log.topics.first().copied() else {
            return Err(ClankerQuoteError::LiquiditySequence);
        };
        if topic == events::Initialize::SIGNATURE_HASH {
            let event =
                events::Initialize::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                    .map_err(|_| ClankerQuoteError::InitializeIdentity)?;
            let tick_spacing = i32::try_from(event.tickSpacing)
                .map_err(|_| ClankerQuoteError::InitializeIdentity)?;
            let tick =
                i32::try_from(event.tick).map_err(|_| ClankerQuoteError::InitializeIdentity)?;
            let fee =
                u32::try_from(event.fee).map_err(|_| ClankerQuoteError::InitializeIdentity)?;
            if initialize.is_some()
                || event.id != launch.pool_id
                || event.currency0 != key.currency0
                || event.currency1 != key.currency1
                || fee != DYNAMIC_FEE_FLAG
                || tick_spacing != key.tick_spacing
                || event.hooks != key.hooks
            {
                return Err(ClankerQuoteError::InitializeIdentity);
            }
            initialize = Some((U256::from(event.sqrtPriceX96), tick, log.log_index));
        } else if topic == events::ModifyLiquidity::SIGNATURE_HASH {
            let event = events::ModifyLiquidity::decode_raw_log_validate(
                log.topics.iter().copied(),
                &log.data,
            )
            .map_err(|_| ClankerQuoteError::LiquiditySequence)?;
            if event.id != launch.pool_id || initialize.is_none() {
                return Err(ClankerQuoteError::LiquiditySequence);
            }
            let delta = i128::try_from(event.liquidityDelta)
                .map_err(|_| ClankerQuoteError::LiquiditySequence)?;
            let liquidity =
                u128::try_from(delta).map_err(|_| ClankerQuoteError::LiquiditySequence)?;
            positions.push(Position {
                tick_lower: i32::try_from(event.tickLower)
                    .map_err(|_| ClankerQuoteError::LiquiditySequence)?,
                tick_upper: i32::try_from(event.tickUpper)
                    .map_err(|_| ClankerQuoteError::LiquiditySequence)?,
                liquidity,
                log_index: log.log_index,
            });
        } else if topic == events::Swap::SIGNATURE_HASH {
            let event =
                events::Swap::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                    .map_err(|_| ClankerQuoteError::EmbeddedSwapUnsupported)?;
            if event.id == launch.pool_id {
                pool_swaps += 1;
            }
        } else {
            return Err(ClankerQuoteError::LiquiditySequence);
        }
    }
    if pool_swaps != 0 {
        return Err(ClankerQuoteError::EmbeddedSwapUnsupported);
    }
    let (sqrt_price_x96, initialize_tick, initialize_log_index) =
        initialize.ok_or(ClankerQuoteError::InitializeIdentity)?;
    let expected_initialize_tick = if WETH < launch.token {
        launch
            .starting_tick
            .checked_neg()
            .ok_or(ClankerQuoteError::InitializeIdentity)?
    } else {
        launch.starting_tick
    };
    if initialize_tick != expected_initialize_tick
        || positions.len() != 5
        || positions
            .iter()
            .any(|position| position.log_index <= initialize_log_index)
    {
        return Err(ClankerQuoteError::LiquiditySequence);
    }
    let last_liquidity_log_index = positions
        .last()
        .ok_or(ClankerQuoteError::LiquiditySequence)?
        .log_index;
    if launch.log_index <= last_liquidity_log_index {
        return Err(ClankerQuoteError::LiquiditySequence);
    }

    let static_fee_config =
        validate_hook_configuration(&receipt.logs, launch, initialize_log_index, profile)?;
    let mev_fee_config =
        validate_mev_configuration(&receipt.logs, launch, initialize_log_index, profile)?;

    let first_eligible_timestamp = block
        .timestamp
        .checked_add(profile.mev_delay_guard_seconds)
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?;
    let mev_fee_ppm = descending_mev_fee_ppm(
        profile,
        mev_fee_config,
        block.timestamp,
        first_eligible_timestamp,
    )?;
    let entry_lp_fee_ppm = mev_fee_ppm.max(static_fee_config.paired_fee_ppm);
    let entry_protocol_fee_ppm = entry_lp_fee_ppm
        .checked_mul(u32::from(profile.protocol_fee_share_percent))
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?
        / 100;

    let mut state = V3PoolState::new(
        profile.pool_manager.address,
        key.currency0,
        key.currency1,
        entry_lp_fee_ppm,
        key.tick_spacing,
        sqrt_price_x96,
        initialize_tick,
        0,
    )?;
    for position in &positions {
        state.add_position(position.tick_lower, position.tick_upper, position.liquidity)?;
    }
    if state.liquidity != 0 {
        return Err(ClankerQuoteError::LiquiditySequence);
    }

    let entry_protocol_fee = exact_input_protocol_fee(policy.amount_in, entry_protocol_fee_ppm)?;
    let entry_core_amount = policy
        .amount_in
        .checked_sub(entry_protocol_fee)
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?;
    let entry_core = state.quote_exact_input(WETH, entry_core_amount, None)?;
    validate_complete_core_quote(&entry_core, entry_core_amount)?;
    let entry_min = apply_slippage(entry_core.amount_out, policy.slippage_bps)?;

    let exit_lp_fee_ppm = mev_fee_ppm.max(static_fee_config.clanker_fee_ppm);
    let exit_protocol_fee_ppm = exit_lp_fee_ppm
        .checked_mul(u32::from(profile.protocol_fee_share_percent))
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?
        / 100;
    let mut post_entry = V3PoolState::new(
        profile.pool_manager.address,
        key.currency0,
        key.currency1,
        exit_lp_fee_ppm,
        key.tick_spacing,
        entry_core.sqrt_price_x96_after,
        entry_core.tick_after,
        0,
    )?;
    for position in &positions {
        post_entry.add_position(position.tick_lower, position.tick_upper, position.liquidity)?;
    }
    if post_entry.liquidity != entry_core.liquidity_after {
        return Err(ClankerQuoteError::LiquiditySequence);
    }
    post_entry.set_observation(
        entry_core.sqrt_price_x96_after,
        entry_core.tick_after,
        entry_core.liquidity_after,
    )?;
    let exit_core = post_entry.quote_exact_input(launch.token, entry_core.amount_out, None)?;
    validate_complete_core_quote(&exit_core, entry_core.amount_out)?;
    let exit_protocol_fee = exit_core
        .amount_out
        .checked_mul(U256::from(exit_protocol_fee_ppm))
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?
        / U256::from(FEE_DENOMINATOR);
    let exit_net = exit_core
        .amount_out
        .checked_sub(exit_protocol_fee)
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?;
    let exit_min = apply_slippage(exit_net, policy.slippage_bps)?;
    let round_trip_return_bps = exit_net
        .checked_mul(U256::from(BPS_DENOMINATOR))
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?
        / policy.amount_in;

    Ok(ClankerReceiptPaperQuote {
        record_type: "launchpad_clanker_v4_paper_quote".into(),
        tx_hash: receipt.transaction_hash,
        launchpad: LaunchpadId::Clanker,
        l2_block_number: receipt.l2_block_number,
        state_version: ClankerStateVersion {
            chain_id: CHAIN_ID,
            block_hash: receipt.block_hash,
            l2_block_number: receipt.l2_block_number,
            transaction_index: receipt.transaction_index,
            terminal_log_index: launch.log_index,
            receipt_timestamp: block.timestamp,
            first_eligible_quote_timestamp: first_eligible_timestamp,
        },
        quote_source: "confirmed_receipt_end_clanker_v4_first_eligible_state".into(),
        sizing_source: "independent_fixed_tiny_weth_policy".into(),
        market: ClankerMarketEvidence {
            token: launch.token,
            token_admin: launch.token_admin,
            pool_id: launch.pool_id,
            pool_manager: profile.pool_manager.address,
            quote_asset: WETH,
            hook: launch.hook,
            locker: launch.locker,
            mev_module: launch.mev_module,
            extension: launch.extension,
            dynamic_fee_flag: DYNAMIC_FEE_FLAG,
            tick_spacing: key.tick_spacing,
            starting_tick: launch.starting_tick,
            initialize_tick,
            initialize_log_index,
            last_liquidity_log_index,
            launch_log_index: launch.log_index,
            position_count: positions.len(),
            static_fee_config,
            mev_fee_config,
        },
        entry: ClankerPaperSwapQuote {
            amount_in: policy.amount_in,
            hook_protocol_fee: entry_protocol_fee,
            core_amount_in: entry_core_amount,
            expected_output: entry_core.amount_out,
            min_receive: entry_min,
            slippage_bps: policy.slippage_bps,
            lp_fee_ppm: entry_lp_fee_ppm,
            protocol_fee_ppm: entry_protocol_fee_ppm,
            core_state_after: entry_core,
        },
        full_position_exit: ClankerPaperSwapQuote {
            amount_in: exit_core.amount_in_requested,
            hook_protocol_fee: exit_protocol_fee,
            core_amount_in: exit_core.amount_in_requested,
            expected_output: exit_net,
            min_receive: exit_min,
            slippage_bps: policy.slippage_bps,
            lp_fee_ppm: exit_lp_fee_ppm,
            protocol_fee_ppm: exit_protocol_fee_ppm,
            core_state_after: exit_core,
        },
        simulated_round_trip_return_bps: round_trip_return_bps,
        execution_eligible: false,
        execution_blocker: "paper_only_clanker_hook_mev_and_router_execution_not_enabled".into(),
        broadcast: false,
    })
}

fn validate_envelope(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
    policy: ClankerQuotePolicy,
) -> Result<(), ClankerQuoteError> {
    if !receipt.status
        || receipt.transaction_hash == B256::ZERO
        || receipt.block_hash == B256::ZERO
        || transaction.hash != receipt.transaction_hash
        || transaction.to != Some(CLANKER_FACTORY)
        || transaction.input.get(..4) != Some(CLANKER_DEPLOY_SELECTOR.as_slice())
        || transaction.value != U256::ZERO
        || transaction.l2_block_number != Some(receipt.l2_block_number)
        || transaction.transaction_index != Some(receipt.transaction_index)
        || block.l2_block_number != receipt.l2_block_number
        || block.hash != receipt.block_hash
        || receipt
            .l1_block_number
            .is_some_and(|l1| l1 != block.l1_block_number)
    {
        return Err(ClankerQuoteError::InvalidEnvelope);
    }
    if policy.amount_in == U256::ZERO
        || policy.max_amount_in == U256::ZERO
        || policy.amount_in > policy.max_amount_in
        || policy.slippage_bps >= BPS_DENOMINATOR
    {
        return Err(ClankerQuoteError::UnsafePolicy);
    }
    if receipt
        .logs
        .windows(2)
        .any(|pair| pair[0].log_index >= pair[1].log_index)
    {
        return Err(ClankerQuoteError::UnorderedLogs);
    }
    Ok(())
}

fn exact_launch_identity(logs: &[ReceiptLog]) -> Result<LaunchIdentity, ClankerQuoteError> {
    let matching = logs
        .iter()
        .filter(|log| {
            log.address == CLANKER_FACTORY
                && log.topics.first() == Some(&events::TokenCreated::SIGNATURE_HASH)
        })
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(ClankerQuoteError::TokenCreatedIdentity);
    }
    let log = matching[0];
    let event =
        events::TokenCreated::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
            .map_err(|_| ClankerQuoteError::TokenCreatedIdentity)?;
    if event.extensions.len() != 1 {
        return Err(ClankerQuoteError::MarketIdentity);
    }
    Ok(LaunchIdentity {
        sender: event.msgSender,
        token: event.tokenAddress,
        token_admin: event.tokenAdmin,
        starting_tick: i32::try_from(event.startingTick)
            .map_err(|_| ClankerQuoteError::MarketIdentity)?,
        hook: event.poolHook,
        pool_id: event.poolId,
        paired_token: event.pairedToken,
        locker: event.locker,
        mev_module: event.mevModule,
        extension: event.extensions[0],
        extensions_supply: event.extensionsSupply,
        log_index: log.log_index,
    })
}

fn validate_hook_configuration(
    logs: &[ReceiptLog],
    launch: LaunchIdentity,
    initialize_log_index: u64,
    profile: ClankerV4ExpectedProfile,
) -> Result<ClankerStaticFeeConfig, ClankerQuoteError> {
    let matching = logs
        .iter()
        .filter(|log| {
            log.address == profile.hook.address
                && log.topics.first() == Some(&events::PoolInitialized::SIGNATURE_HASH)
        })
        .collect::<Vec<_>>();
    if matching.len() != 1
        || matching[0].log_index <= initialize_log_index
        || matching[0].log_index >= launch.log_index
    {
        return Err(ClankerQuoteError::HookConfiguration);
    }
    let event = events::PoolInitialized::decode_raw_log_validate(
        matching[0].topics.iter().copied(),
        &matching[0].data,
    )
    .map_err(|_| ClankerQuoteError::HookConfiguration)?;
    if event.poolId != launch.pool_id {
        return Err(ClankerQuoteError::HookConfiguration);
    }
    let config = ClankerStaticFeeConfig {
        clanker_fee_ppm: u32::try_from(event.clankerFee)
            .map_err(|_| ClankerQuoteError::HookConfiguration)?,
        paired_fee_ppm: u32::try_from(event.pairedFee)
            .map_err(|_| ClankerQuoteError::HookConfiguration)?,
    };
    if config.clanker_fee_ppm > profile.max_static_fee_ppm
        || config.paired_fee_ppm > profile.max_static_fee_ppm
    {
        return Err(ClankerQuoteError::HookConfiguration);
    }
    Ok(config)
}

fn validate_mev_configuration(
    logs: &[ReceiptLog],
    launch: LaunchIdentity,
    initialize_log_index: u64,
    profile: ClankerV4ExpectedProfile,
) -> Result<ClankerMevFeeConfig, ClankerQuoteError> {
    let matching = logs
        .iter()
        .filter(|log| {
            log.address == profile.mev_module.address
                && log.topics.first() == Some(&events::FeeConfigSet::SIGNATURE_HASH)
        })
        .collect::<Vec<_>>();
    if matching.len() != 1
        || matching[0].log_index <= initialize_log_index
        || matching[0].log_index >= launch.log_index
    {
        return Err(ClankerQuoteError::MevConfiguration);
    }
    let event = events::FeeConfigSet::decode_raw_log_validate(
        matching[0].topics.iter().copied(),
        &matching[0].data,
    )
    .map_err(|_| ClankerQuoteError::MevConfiguration)?;
    if event.poolId != launch.pool_id {
        return Err(ClankerQuoteError::MevConfiguration);
    }
    let config = ClankerMevFeeConfig {
        starting_fee_ppm: u32::try_from(event.startingFee)
            .map_err(|_| ClankerQuoteError::MevConfiguration)?,
        ending_fee_ppm: u32::try_from(event.endingFee)
            .map_err(|_| ClankerQuoteError::MevConfiguration)?,
        seconds_to_decay: u64::try_from(event.secondsToDecay)
            .map_err(|_| ClankerQuoteError::MevConfiguration)?,
    };
    if config.starting_fee_ppm == 0
        || config.starting_fee_ppm > profile.max_mev_fee_ppm
        || config.ending_fee_ppm > config.starting_fee_ppm
        || config.seconds_to_decay == 0
        || config.seconds_to_decay > profile.max_mev_seconds_to_decay
    {
        return Err(ClankerQuoteError::MevConfiguration);
    }
    Ok(config)
}

pub fn descending_mev_fee_ppm(
    profile: ClankerV4ExpectedProfile,
    config: ClankerMevFeeConfig,
    pool_start_time: u64,
    timestamp: u64,
) -> Result<u32, ClankerQuoteError> {
    profile.validate()?;
    let first_eligible = pool_start_time
        .checked_add(profile.mev_delay_guard_seconds)
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?;
    if timestamp < first_eligible {
        return Err(ClankerQuoteError::MevConfiguration);
    }
    let module_disable_time = first_eligible
        .checked_add(config.seconds_to_decay)
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?;
    let hook_disable_time = pool_start_time
        .checked_add(profile.max_mev_seconds_to_decay)
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?;
    let disable_time = module_disable_time.min(hook_disable_time);
    if timestamp >= disable_time {
        return Ok(0);
    }
    let elapsed = timestamp - first_eligible;
    let remaining = config
        .seconds_to_decay
        .checked_sub(elapsed)
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?;
    let fee_range = u64::from(
        config
            .starting_fee_ppm
            .checked_sub(config.ending_fee_ppm)
            .ok_or(ClankerQuoteError::ArithmeticOverflow)?,
    );
    let normalized = u128::from(remaining)
        .checked_mul(u128::from(FEE_SCALE))
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?
        / u128::from(config.seconds_to_decay);
    let squared = normalized
        .checked_mul(normalized)
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?
        / u128::from(FEE_SCALE);
    let decay = u128::from(fee_range)
        .checked_mul(squared)
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?
        / u128::from(FEE_SCALE);
    let fee = u128::from(config.ending_fee_ppm)
        .checked_add(decay)
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?;
    u32::try_from(fee).map_err(|_| ClankerQuoteError::ArithmeticOverflow)
}

fn exact_input_protocol_fee(
    amount_in: U256,
    protocol_fee_ppm: u32,
) -> Result<U256, ClankerQuoteError> {
    let scaled = U256::from(protocol_fee_ppm)
        .checked_mul(U256::from(FEE_SCALE))
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?
        / U256::from(FEE_DENOMINATOR + protocol_fee_ppm);
    amount_in
        .checked_mul(scaled)
        .ok_or(ClankerQuoteError::ArithmeticOverflow)
        .map(|value| value / U256::from(FEE_SCALE))
}

fn apply_slippage(amount: U256, slippage_bps: u16) -> Result<U256, ClankerQuoteError> {
    let minimum = amount
        .checked_mul(U256::from(BPS_DENOMINATOR - slippage_bps))
        .ok_or(ClankerQuoteError::ArithmeticOverflow)?
        / U256::from(BPS_DENOMINATOR);
    if minimum == U256::ZERO || minimum > amount {
        return Err(ClankerQuoteError::IncompleteQuote);
    }
    Ok(minimum)
}

fn validate_complete_core_quote(
    quote: &V3Quote,
    expected_input: U256,
) -> Result<(), ClankerQuoteError> {
    if quote.amount_in_requested != expected_input
        || quote.amount_in_consumed != expected_input
        || quote.amount_out == U256::ZERO
    {
        return Err(ClankerQuoteError::IncompleteQuote);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct LiveFixture {
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
    }

    #[derive(Deserialize)]
    struct SwapDifferentialFixture {
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
        observed_swap: ObservedSwap,
    }

    #[derive(Deserialize)]
    struct ObservedSwap {
        transaction_hash: B256,
        gross_amount_in: U256,
        core_amount_in: U256,
        hook_protocol_fee: U256,
        amount_out: U256,
        sqrt_price_x96: U256,
        liquidity: String,
        tick: i32,
        fee_ppm: u32,
    }

    fn live_fixture() -> LiveFixture {
        serde_json::from_str(include_str!("../tests/fixtures/clanker-v4-live-proof.json")).unwrap()
    }

    fn swap_differential_fixture() -> SwapDifferentialFixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/clanker-v4-first-swap-differential.json"
        ))
        .unwrap()
    }

    fn policy() -> ClankerQuotePolicy {
        ClankerQuotePolicy {
            amount_in: U256::from(1_000_000_000_000_000_u64),
            max_amount_in: U256::from(10_000_000_000_000_000_u64),
            slippage_bps: 100,
        }
    }

    fn set_static_fees(receipt: &mut NoxaReceipt, clanker_fee: u32, paired_fee: u32) {
        let log = receipt
            .logs
            .iter_mut()
            .find(|log| {
                log.address == CLANKER_STATIC_HOOK
                    && log.topics.first() == Some(&events::PoolInitialized::SIGNATURE_HASH)
            })
            .unwrap();
        let mut data = log.data.to_vec();
        data[61..64].copy_from_slice(&clanker_fee.to_be_bytes()[1..]);
        data[93..96].copy_from_slice(&paired_fee.to_be_bytes()[1..]);
        log.data = data.into();
    }

    fn set_mev_fees(receipt: &mut NoxaReceipt, starting_fee: u32, ending_fee: u32) {
        let log = receipt
            .logs
            .iter_mut()
            .find(|log| {
                log.address == CLANKER_DESCENDING_MEV_MODULE
                    && log.topics.first() == Some(&events::FeeConfigSet::SIGNATURE_HASH)
            })
            .unwrap();
        let mut data = log.data.to_vec();
        data[61..64].copy_from_slice(&starting_fee.to_be_bytes()[1..]);
        data[93..96].copy_from_slice(&ending_fee.to_be_bytes()[1..]);
        log.data = data.into();
    }

    #[test]
    fn descending_fee_starts_at_pin_and_disables_to_static_fee() {
        let profile = ClankerV4ExpectedProfile::production();
        let config = ClankerMevFeeConfig {
            starting_fee_ppm: 666_777,
            ending_fee_ppm: 41_673,
            seconds_to_decay: 15,
        };
        let start = 1_784_079_024;
        assert_eq!(
            descending_mev_fee_ppm(profile, config, start, start + 1).unwrap(),
            666_777
        );
        assert!(
            descending_mev_fee_ppm(profile, config, start, start + 8).unwrap()
                < config.starting_fee_ppm
        );
        assert_eq!(
            descending_mev_fee_ppm(profile, config, start, start + 16).unwrap(),
            0
        );
        assert!(matches!(
            descending_mev_fee_ppm(profile, config, start, start),
            Err(ClankerQuoteError::MevConfiguration)
        ));
    }

    #[test]
    fn maximum_duration_obeys_outer_hook_disable_boundary() {
        let profile = ClankerV4ExpectedProfile::production();
        let config = ClankerMevFeeConfig {
            starting_fee_ppm: 800_000,
            ending_fee_ppm: 50_000,
            seconds_to_decay: profile.max_mev_seconds_to_decay,
        };
        let start = 1_784_079_024;
        assert!(
            descending_mev_fee_ppm(profile, config, start, start + 119).unwrap()
                >= config.ending_fee_ppm
        );
        assert_eq!(
            descending_mev_fee_ppm(profile, config, start, start + 120).unwrap(),
            0
        );
    }

    #[test]
    fn production_profile_is_complete_and_not_observed_at_runtime() {
        let profile = ClankerV4ExpectedProfile::production();
        profile.validate().unwrap();
        assert_ne!(profile.factory.runtime_code_hash, B256::ZERO);
        assert_ne!(profile.pool_manager.runtime_code_hash, B256::ZERO);
        assert_ne!(profile.hook.runtime_code_hash, B256::ZERO);
        assert_ne!(profile.locker.runtime_code_hash, B256::ZERO);
        assert_ne!(profile.mev_module.runtime_code_hash, B256::ZERO);
        assert_ne!(profile.extension.runtime_code_hash, B256::ZERO);
    }

    #[test]
    fn accepts_receipt_local_static_fees_within_reviewed_hook_bound() {
        let mut fixture = live_fixture();
        set_static_fees(&mut fixture.receipt, 20_000, 30_000);
        let quote = quote_clanker_launch_receipt(
            &fixture.transaction,
            &fixture.receipt,
            &fixture.block,
            ClankerV4ExpectedProfile::production(),
            policy(),
        )
        .unwrap();
        assert_eq!(quote.market.static_fee_config.clanker_fee_ppm, 20_000);
        assert_eq!(quote.market.static_fee_config.paired_fee_ppm, 30_000);

        set_mev_fees(&mut fixture.receipt, 10_000, 5_000);
        let quote = quote_clanker_launch_receipt(
            &fixture.transaction,
            &fixture.receipt,
            &fixture.block,
            ClankerV4ExpectedProfile::production(),
            policy(),
        )
        .unwrap();
        assert_eq!(quote.entry.lp_fee_ppm, 30_000);
        assert_eq!(quote.full_position_exit.lp_fee_ppm, 20_000);

        set_static_fees(&mut fixture.receipt, 100_001, 30_000);
        assert!(matches!(
            quote_clanker_launch_receipt(
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                ClankerV4ExpectedProfile::production(),
                policy(),
            ),
            Err(ClankerQuoteError::HookConfiguration)
        ));
    }

    #[test]
    fn live_clanker_launch_reconstructs_first_eligible_entry_and_exit() {
        let fixture = live_fixture();
        let quote = quote_clanker_launch_receipt(
            &fixture.transaction,
            &fixture.receipt,
            &fixture.block,
            ClankerV4ExpectedProfile::production(),
            policy(),
        )
        .unwrap();
        assert_eq!(quote.tx_hash, fixture.receipt.transaction_hash);
        assert_eq!(quote.market.position_count, 5);
        assert_eq!(quote.entry.lp_fee_ppm, 666_777);
        assert_eq!(quote.entry.protocol_fee_ppm, 133_355);
        assert_eq!(quote.market.static_fee_config.clanker_fee_ppm, 16_000);
        assert_eq!(quote.market.static_fee_config.paired_fee_ppm, 16_000);
        assert_eq!(
            quote.state_version.first_eligible_quote_timestamp,
            1_784_079_025
        );
        assert_eq!(
            quote.entry.hook_protocol_fee,
            U256::from(117_663_927_013_160_u64)
        );
        assert_eq!(
            quote.entry.expected_output,
            U256::from_str_radix("2769d1ed26d975c58dfc0", 16).unwrap()
        );
        assert_eq!(
            quote.full_position_exit.expected_output,
            U256::from(84_919_129_125_191_u64)
        );
        assert_eq!(quote.simulated_round_trip_return_bps, U256::from(849));
        assert!(!quote.execution_eligible);
        assert!(!quote.broadcast);
    }

    #[test]
    fn static_fee_model_exactly_matches_real_first_post_launch_swap() {
        let fixture = swap_differential_fixture();
        let profile = ClankerV4ExpectedProfile::production();
        let launch_quote = quote_clanker_launch_receipt(
            &fixture.transaction,
            &fixture.receipt,
            &fixture.block,
            profile,
            policy(),
        )
        .unwrap();
        assert_eq!(launch_quote.market.mev_fee_config.ending_fee_ppm, 41_673);

        let initialize = fixture
            .receipt
            .logs
            .iter()
            .find_map(|log| {
                events::Initialize::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                    .ok()
            })
            .unwrap();
        let mut state = V3PoolState::new(
            profile.pool_manager.address,
            initialize.currency0,
            initialize.currency1,
            fixture.observed_swap.fee_ppm,
            i32::try_from(initialize.tickSpacing).unwrap(),
            U256::from(initialize.sqrtPriceX96),
            i32::try_from(initialize.tick).unwrap(),
            0,
        )
        .unwrap();
        for event in fixture.receipt.logs.iter().filter_map(|log| {
            events::ModifyLiquidity::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                .ok()
        }) {
            state
                .add_position(
                    i32::try_from(event.tickLower).unwrap(),
                    i32::try_from(event.tickUpper).unwrap(),
                    u128::try_from(i128::try_from(event.liquidityDelta).unwrap()).unwrap(),
                )
                .unwrap();
        }
        let protocol_fee = exact_input_protocol_fee(
            fixture.observed_swap.gross_amount_in,
            fixture.observed_swap.fee_ppm * u32::from(profile.protocol_fee_share_percent) / 100,
        )
        .unwrap();
        assert_eq!(protocol_fee, fixture.observed_swap.hook_protocol_fee);
        assert_eq!(
            fixture.observed_swap.gross_amount_in - protocol_fee,
            fixture.observed_swap.core_amount_in
        );
        let quote = state
            .quote_exact_input(WETH, fixture.observed_swap.core_amount_in, None)
            .unwrap();
        assert_eq!(quote.amount_out, fixture.observed_swap.amount_out);
        assert_eq!(
            quote.sqrt_price_x96_after,
            fixture.observed_swap.sqrt_price_x96
        );
        assert_eq!(
            quote.liquidity_after,
            u128::from_str_radix(
                fixture.observed_swap.liquidity.strip_prefix("0x").unwrap(),
                16
            )
            .unwrap()
        );
        assert_eq!(quote.tick_after, fixture.observed_swap.tick);
        assert_eq!(
            fixture.observed_swap.transaction_hash,
            alloy_primitives::b256!(
                "85cc9fb0c4a458456a61653d3381b1b28498d81af945a0d855d5672546e7f803"
            )
        );
    }

    #[test]
    fn rejects_missing_hook_or_mev_configuration_and_incomplete_liquidity() {
        let profile = ClankerV4ExpectedProfile::production();
        let mut fixture = live_fixture();
        fixture
            .receipt
            .logs
            .retain(|log| log.address != profile.hook.address);
        assert!(matches!(
            quote_clanker_launch_receipt(
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                profile,
                policy()
            ),
            Err(ClankerQuoteError::HookConfiguration)
        ));

        let mut fixture = live_fixture();
        fixture
            .receipt
            .logs
            .retain(|log| log.address != profile.mev_module.address);
        assert!(matches!(
            quote_clanker_launch_receipt(
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                profile,
                policy()
            ),
            Err(ClankerQuoteError::MevConfiguration)
        ));

        let mut fixture = live_fixture();
        fixture.receipt.logs.remove(3);
        assert!(matches!(
            quote_clanker_launch_receipt(
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                profile,
                policy()
            ),
            Err(ClankerQuoteError::LiquiditySequence)
        ));

        let mut fixture = live_fixture();
        let mut duplicate = fixture.receipt.logs.last().unwrap().clone();
        duplicate.log_index += 1;
        fixture.receipt.logs.push(duplicate);
        assert!(matches!(
            quote_clanker_launch_receipt(
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                profile,
                policy()
            ),
            Err(ClankerQuoteError::TokenCreatedIdentity)
        ));
    }
}
