//! Strict, receipt-end paper quotes for current-generation Pons launches.
//!
//! This module performs no I/O, signing, or execution. It accepts a confirmed
//! transaction and receipt, validates the exact current Pons launch/locker/V3
//! topology, reconstructs the terminal pool state, and produces an independent
//! fixed-size WETH entry plus immediate full-position exit.

use alloy_primitives::{Address, B256, I256, U256};
use alloy_sol_types::SolEvent;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

use crate::launchpad_adapter::LaunchpadId;
use crate::noxa_abi::{ReceiptLog, V3PoolEvent, decode_pool_created, decode_v3_pool_event};
use crate::noxa_predict::predict_v3_pool_address;
use crate::noxa_rpc::{NoxaReceipt, RobinhoodTransaction};
use crate::pons::{
    PONS_CHAIN_ID, PONS_CURRENT_FACTORY, PONS_CURRENT_LOCKER, PONS_DEX_CONFIG_ID,
    PONS_LAUNCH_CONFIG_ID, PONS_LAUNCH_FEE_WEI, PONS_POOL_FEE, PONS_POSITION_LOCKED_TOPIC,
    PONS_POSITION_MANAGER, PONS_SWAP_ROUTER_02, PONS_TICK_SPACING, PONS_TOKEN_DEPLOYED_TOPIC,
    PONS_TOKEN_LAUNCHED_TOPIC, PONS_V3_FACTORY, PONS_WETH, PonsAdapter, PonsAttributionProvenance,
    PonsExpectedProfile, PonsGeneration, PonsObservationInput,
};
use crate::robinhood::UNISWAP_V3_POOL_INIT_CODE_KECCAK256;
use crate::v3_pool::{V3PoolError, V3PoolState, V3Quote};

const BPS_DENOMINATOR: u16 = 10_000;
const PONS_MAX_WALLET_BPS: u16 = 200;
const PONS_RESTRICTION_L1_BLOCKS: u64 = 366;
const ERC20_OR_721_TRANSFER_TOPIC: B256 =
    alloy_primitives::b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");

mod events {
    use alloy_sol_types::sol;

    sol! {
        event TokenDeployed(
            address indexed token,
            address indexed deployer,
            address indexed dexFactory,
            address pairToken,
            uint256 launchConfigId,
            uint256 dexConfigId
        );

        event TokenLaunched(
            address indexed token,
            address indexed deployer,
            address indexed dexFactory,
            address pairToken,
            address pool,
            uint256 launchConfigId,
            uint256 dexConfigId,
            uint256 positionId,
            uint256 restrictionsEndBlock,
            uint256 initialBuyAmount
        );

        event PositionLocked(
            address indexed token,
            address indexed creator,
            uint256 indexed dexConfigId,
            address pairToken,
            uint256 positionId,
            address positionManager
        );

        event IncreaseLiquidity(
            uint256 indexed tokenId,
            uint128 liquidity,
            uint256 amount0,
            uint256 amount1
        );

        event Mint(
            address sender,
            address indexed owner,
            int24 indexed tickLower,
            int24 indexed tickUpper,
            uint128 amount,
            uint256 amount0,
            uint256 amount1
        );
    }
}

/// Decode only the canonical identity fields carried by a Pons launch event.
/// This is intentionally independent of quote admission so legacy/discovery
/// launches remain measurable when no warm quote is available.
pub fn pons_launch_event_identity(log: &ReceiptLog) -> Option<(Address, Address)> {
    if !matches!(
        log.address,
        PONS_CURRENT_FACTORY | crate::pons::PONS_LEGACY_FACTORY
    ) || log.topics.first() != Some(&PONS_TOKEN_LAUNCHED_TOPIC)
    {
        return None;
    }
    let event =
        events::TokenLaunched::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
            .ok()?;
    (event.token != Address::ZERO && event.pool != Address::ZERO)
        .then_some((event.token, event.pool))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct PonsQuotePolicy {
    pub amount_in: U256,
    pub max_amount_in: U256,
    pub slippage_bps: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PonsStateVersion {
    pub chain_id: u64,
    pub block_hash: B256,
    pub l2_block_number: u64,
    pub transaction_index: u64,
    pub launch_l1_block_number: u64,
    pub first_eligible_l1_block_number: u64,
    pub restriction_end_l1_block_number: u64,
    pub terminal_log_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PonsMarketEvidence {
    pub generation: PonsGeneration,
    pub leader: Address,
    pub token: Address,
    pub pool: Address,
    pub quote_asset: Address,
    pub factory: Address,
    pub factory_runtime_hash: B256,
    pub locker: Address,
    pub locker_runtime_hash: B256,
    pub position_manager: Address,
    pub position_manager_runtime_hash: B256,
    pub v3_factory_runtime_hash: B256,
    pub swap_router_runtime_hash: B256,
    pub quote_asset_runtime_hash: B256,
    pub position_id: U256,
    pub initial_buy_amount: U256,
    pub initial_buy_state_after: Option<V3Quote>,
    pub fee: u32,
    pub tick_spacing: i32,
    pub position_tick_lower: i32,
    pub position_tick_upper: i32,
    pub position_liquidity: U256,
    pub position_amount0: U256,
    pub position_amount1: U256,
    pub receipt_end_sqrt_price_x96: U256,
    pub receipt_end_tick: i32,
    pub receipt_end_liquidity: U256,
    pub initialize_tick: i32,
    pub initialize_log_index: u64,
    pub mint_log_index: u64,
    pub locker_log_index: u64,
    pub launch_log_index: u64,
    pub initial_buy_swap_log_index: Option<u64>,
    pub mint_count: usize,
    pub launch_swap_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PonsPaperSwapQuote {
    pub amount_in: U256,
    pub expected_output: U256,
    pub min_receive: U256,
    pub slippage_bps: u16,
    pub state_after: V3Quote,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PonsReceiptPaperQuote {
    pub record_type: String,
    pub tx_hash: B256,
    pub launchpad: LaunchpadId,
    pub l2_block_number: u64,
    pub state_version: PonsStateVersion,
    pub quote_source: String,
    pub sizing_source: String,
    pub market: PonsMarketEvidence,
    pub entry: PonsPaperSwapQuote,
    pub full_position_exit: PonsPaperSwapQuote,
    pub simulated_round_trip_return_bps: U256,
    pub execution_eligible: bool,
    pub execution_blocker: String,
    pub broadcast: bool,
}

#[derive(Debug, Error)]
pub enum PonsQuoteError {
    #[error("only the independently pinned current Pons generation is quotable")]
    UnsupportedGeneration,
    #[error("transaction, receipt, or paper policy envelope is invalid")]
    InvalidEnvelope,
    #[error("Pons launch calldata is malformed or not the pinned configuration")]
    LaunchCalldata,
    #[error("Pons launch events are missing, duplicated, or inconsistent")]
    LaunchIdentity,
    #[error("canonical V3 pool creation identity is inconsistent")]
    PoolIdentity,
    #[error("Pons pool state events are incomplete, reordered, or unsupported")]
    StateSequence,
    #[error("Pons LP NFT was not minted and locked through the pinned identities")]
    PositionLockIdentity,
    #[error("Pons restriction boundary is inconsistent with the receipt L1 block")]
    RestrictionEvidence,
    #[error("launch-time initial buy does not match the reconstructed V3 swap")]
    InitialBuyMismatch,
    #[error("independent paper entry exceeds the fresh-wallet restriction cap")]
    RestrictionCap,
    #[error("quote arithmetic overflowed or did not consume the full input")]
    IncompleteQuote,
    #[error(transparent)]
    Pool(#[from] V3PoolError),
}

#[derive(Debug, Clone, Copy)]
struct LaunchIdentity {
    token: Address,
    deployer: Address,
    pool: Address,
    position_id: U256,
    restriction_end: u64,
    initial_buy_amount: U256,
    deployed_log_index: u64,
    launched_log_index: u64,
}

#[derive(Debug, Clone, Copy)]
struct PositionEvidence {
    liquidity: u128,
    amount0: U256,
    amount1: U256,
    log_index: u64,
}

/// Reconstruct a current-generation Pons launch and quote a fixed tiny entry
/// plus immediate full exit from the confirmed receipt-end V3 state.
pub fn quote_pons_launch_receipt(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    expected_profile: PonsExpectedProfile,
    policy: PonsQuotePolicy,
) -> Result<PonsReceiptPaperQuote, PonsQuoteError> {
    validate_envelope(transaction, receipt, policy)?;
    expected_profile
        .validate()
        .map_err(|_| PonsQuoteError::LaunchCalldata)?;
    let expected_identities = expected_profile.identities();
    let adapter = PonsAdapter::from_startup_identities(&expected_identities)
        .map_err(|_| PonsQuoteError::LaunchCalldata)?;
    let current_factory_identity = expected_profile
        .identity(PONS_CURRENT_FACTORY)
        .ok_or(PonsQuoteError::LaunchCalldata)?;
    let observation = adapter
        .observe_launch(PonsObservationInput {
            tx_hash: transaction.hash,
            chain_id: PONS_CHAIN_ID,
            destination: transaction.to.ok_or(PonsQuoteError::InvalidEnvelope)?,
            destination_runtime_hash: current_factory_identity.runtime_hash,
            calldata: &transaction.input,
            value: transaction.value,
            sender: transaction.from,
            provenance: PonsAttributionProvenance::ExactFactoryTransaction,
        })
        .map_err(|_| PonsQuoteError::LaunchCalldata)?;
    if observation.generation != PonsGeneration::Current {
        return Err(PonsQuoteError::UnsupportedGeneration);
    }

    let launch = exact_launch_identity(&receipt.logs, transaction.from)?;
    if transaction.value
        != U256::from(PONS_LAUNCH_FEE_WEI)
            .checked_add(launch.initial_buy_amount)
            .ok_or(PonsQuoteError::IncompleteQuote)?
    {
        return Err(PonsQuoteError::InitialBuyMismatch);
    }
    let launch_l1 = receipt
        .l1_block_number
        .ok_or(PonsQuoteError::RestrictionEvidence)?;
    let expected_restriction_end = launch_l1
        .checked_add(PONS_RESTRICTION_L1_BLOCKS)
        .ok_or(PonsQuoteError::RestrictionEvidence)?;
    if launch.restriction_end != expected_restriction_end {
        return Err(PonsQuoteError::RestrictionEvidence);
    }
    let first_eligible_l1 = launch_l1
        .checked_add(1)
        .ok_or(PonsQuoteError::RestrictionEvidence)?;

    let (token0, token1) = sorted_pair(launch.token, PONS_WETH)?;
    let created = receipt
        .logs
        .iter()
        .filter(|log| log.address == PONS_V3_FACTORY)
        .filter_map(|log| decode_pool_created(log).map(|event| (log.log_index, event)))
        .collect::<Vec<_>>();
    if created.len() != 1 {
        return Err(PonsQuoteError::PoolIdentity);
    }
    let (pool_created_log_index, created) = &created[0];
    let canonical_pool = predict_v3_pool_address(
        PONS_V3_FACTORY,
        token0,
        token1,
        PONS_POOL_FEE,
        UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    );
    if created.token0 != token0
        || created.token1 != token1
        || created.fee != PONS_POOL_FEE
        || created.tick_spacing != PONS_TICK_SPACING
        || created.pool != launch.pool
        || launch.pool != canonical_pool
        || *pool_created_log_index <= launch.deployed_log_index
    {
        return Err(PonsQuoteError::PoolIdentity);
    }

    let expected_tick = if launch.token < PONS_WETH {
        -204_200
    } else {
        204_200
    };
    let expected_sqrt =
        get_sqrt_ratio_at_tick(expected_tick).map_err(|_| PonsQuoteError::StateSequence)?;
    let expected_range = if launch.token < PONS_WETH {
        (-204_200, 887_200)
    } else {
        (-887_200, 204_200)
    };
    let mut state = None;
    let mut initialize_log_index = None;
    let mut position = None;
    let mut launch_swap_count = 0usize;
    let mut initial_buy_swap_log_index = None;
    let mut initial_buy_state_after = None;
    for log in receipt.logs.iter().filter(|log| log.address == launch.pool) {
        if log.log_index <= *pool_created_log_index {
            return Err(PonsQuoteError::StateSequence);
        }
        match decode_v3_pool_event(log).ok_or(PonsQuoteError::StateSequence)? {
            V3PoolEvent::Initialize {
                sqrt_price_x96,
                tick,
            } => {
                if state.is_some()
                    || tick != expected_tick
                    || sqrt_price_x96 != expected_sqrt
                    || log.log_index >= launch.launched_log_index
                {
                    return Err(PonsQuoteError::StateSequence);
                }
                state = Some(V3PoolState::new(
                    launch.pool,
                    token0,
                    token1,
                    PONS_POOL_FEE,
                    PONS_TICK_SPACING,
                    sqrt_price_x96,
                    tick,
                    0,
                )?);
                initialize_log_index = Some(log.log_index);
            }
            V3PoolEvent::Mint {
                tick_lower,
                tick_upper,
                amount,
            } => {
                if position.is_some()
                    || (tick_lower, tick_upper) != expected_range
                    || amount == 0
                    || log.log_index >= launch.launched_log_index
                {
                    return Err(PonsQuoteError::StateSequence);
                }
                let decoded =
                    events::Mint::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                        .map_err(|_| PonsQuoteError::StateSequence)?;
                if decoded.sender != PONS_POSITION_MANAGER
                    || decoded.owner != PONS_POSITION_MANAGER
                    || i32::try_from(decoded.tickLower).ok() != Some(tick_lower)
                    || i32::try_from(decoded.tickUpper).ok() != Some(tick_upper)
                    || decoded.amount != amount
                {
                    return Err(PonsQuoteError::StateSequence);
                }
                state
                    .as_mut()
                    .ok_or(PonsQuoteError::StateSequence)?
                    .add_position(tick_lower, tick_upper, amount)?;
                position = Some(PositionEvidence {
                    liquidity: amount,
                    amount0: decoded.amount0,
                    amount1: decoded.amount1,
                    log_index: log.log_index,
                });
            }
            V3PoolEvent::Swap {
                sender,
                recipient,
                amount0,
                amount1,
                sqrt_price_x96,
                liquidity,
                tick,
            } => {
                if position.is_none()
                    || launch_swap_count != 0
                    || log.log_index <= launch.launched_log_index
                {
                    return Err(PonsQuoteError::StateSequence);
                }
                let pool = state.as_mut().ok_or(PonsQuoteError::StateSequence)?;
                let reconstructed = validate_initial_buy(
                    pool,
                    launch,
                    observation.launch.developer_wallet,
                    sender,
                    recipient,
                    amount0,
                    amount1,
                    sqrt_price_x96,
                    liquidity,
                    tick,
                )?;
                pool.set_observation(sqrt_price_x96, tick, liquidity)?;
                launch_swap_count += 1;
                initial_buy_swap_log_index = Some(log.log_index);
                initial_buy_state_after = Some(reconstructed);
            }
            V3PoolEvent::Burn { .. } => return Err(PonsQuoteError::StateSequence),
        }
    }
    let state = state.ok_or(PonsQuoteError::StateSequence)?;
    let initialize_log_index = initialize_log_index.ok_or(PonsQuoteError::StateSequence)?;
    let position = position.ok_or(PonsQuoteError::StateSequence)?;
    let initial_buy_shape_is_valid = if launch.initial_buy_amount == U256::ZERO {
        launch_swap_count == 0 && initial_buy_state_after.is_none()
    } else {
        launch_swap_count == 1 && initial_buy_state_after.is_some()
    };
    if !initial_buy_shape_is_valid {
        return Err(PonsQuoteError::InitialBuyMismatch);
    }
    let terminal_log_index = initial_buy_swap_log_index.unwrap_or(launch.launched_log_index);

    let locker_log_index = validate_position_and_token_flow(
        &receipt.logs,
        launch,
        position,
        token0,
        *pool_created_log_index,
    )?;
    if !(initialize_log_index < position.log_index
        && position.log_index < locker_log_index
        && locker_log_index < launch.launched_log_index)
    {
        return Err(PonsQuoteError::StateSequence);
    }
    validate_initial_buy_transfers(
        &receipt.logs,
        launch,
        observation.launch.developer_wallet,
        initial_buy_state_after.as_ref(),
        initial_buy_swap_log_index,
    )?;

    let receipt_end_sqrt_price_x96 = state.sqrt_price_x96;
    let receipt_end_tick = state.tick;
    let receipt_end_liquidity = state.liquidity;

    let entry_state = state.quote_exact_input(PONS_WETH, policy.amount_in, None)?;
    validate_complete_quote(&entry_state, policy.amount_in)?;
    let supply = pons_total_supply()?;
    let max_fresh_wallet_output = supply
        .checked_mul(U256::from(PONS_MAX_WALLET_BPS))
        .ok_or(PonsQuoteError::IncompleteQuote)?
        / U256::from(BPS_DENOMINATOR);
    if entry_state.amount_out > max_fresh_wallet_output {
        return Err(PonsQuoteError::RestrictionCap);
    }
    let entry_min = apply_slippage(entry_state.amount_out, policy.slippage_bps)?;
    let mut post_entry = state.clone();
    post_entry.set_observation(
        entry_state.sqrt_price_x96_after,
        entry_state.tick_after,
        entry_state.liquidity_after,
    )?;
    let exit_state = post_entry.quote_exact_input(launch.token, entry_state.amount_out, None)?;
    validate_complete_quote(&exit_state, entry_state.amount_out)?;
    let exit_min = apply_slippage(exit_state.amount_out, policy.slippage_bps)?;
    let round_trip_return_bps = exit_state
        .amount_out
        .checked_mul(U256::from(BPS_DENOMINATOR))
        .ok_or(PonsQuoteError::IncompleteQuote)?
        / policy.amount_in;

    Ok(PonsReceiptPaperQuote {
        record_type: "launchpad_pons_v3_paper_quote".into(),
        tx_hash: receipt.transaction_hash,
        launchpad: LaunchpadId::Pons,
        l2_block_number: receipt.l2_block_number,
        state_version: PonsStateVersion {
            chain_id: PONS_CHAIN_ID,
            block_hash: receipt.block_hash,
            l2_block_number: receipt.l2_block_number,
            transaction_index: receipt.transaction_index,
            launch_l1_block_number: launch_l1,
            first_eligible_l1_block_number: first_eligible_l1,
            restriction_end_l1_block_number: expected_restriction_end,
            terminal_log_index,
        },
        quote_source: "confirmed_receipt_end_pons_v3_state".into(),
        sizing_source: "independent_fixed_tiny_weth_fresh_wallet_policy".into(),
        market: PonsMarketEvidence {
            generation: PonsGeneration::Current,
            leader: transaction.from,
            token: launch.token,
            pool: launch.pool,
            quote_asset: PONS_WETH,
            factory: PONS_CURRENT_FACTORY,
            factory_runtime_hash: current_factory_identity.runtime_hash,
            locker: PONS_CURRENT_LOCKER,
            locker_runtime_hash: expected_profile
                .identity(PONS_CURRENT_LOCKER)
                .ok_or(PonsQuoteError::LaunchCalldata)?
                .runtime_hash,
            position_manager: PONS_POSITION_MANAGER,
            position_manager_runtime_hash: expected_profile
                .identity(PONS_POSITION_MANAGER)
                .ok_or(PonsQuoteError::LaunchCalldata)?
                .runtime_hash,
            v3_factory_runtime_hash: expected_profile
                .identity(PONS_V3_FACTORY)
                .ok_or(PonsQuoteError::LaunchCalldata)?
                .runtime_hash,
            swap_router_runtime_hash: expected_profile
                .identity(PONS_SWAP_ROUTER_02)
                .ok_or(PonsQuoteError::LaunchCalldata)?
                .runtime_hash,
            quote_asset_runtime_hash: expected_profile
                .identity(PONS_WETH)
                .ok_or(PonsQuoteError::LaunchCalldata)?
                .runtime_hash,
            position_id: launch.position_id,
            initial_buy_amount: launch.initial_buy_amount,
            initial_buy_state_after,
            fee: PONS_POOL_FEE,
            tick_spacing: PONS_TICK_SPACING,
            position_tick_lower: expected_range.0,
            position_tick_upper: expected_range.1,
            position_liquidity: U256::from(position.liquidity),
            position_amount0: position.amount0,
            position_amount1: position.amount1,
            receipt_end_sqrt_price_x96,
            receipt_end_tick,
            receipt_end_liquidity: U256::from(receipt_end_liquidity),
            initialize_tick: expected_tick,
            initialize_log_index,
            mint_log_index: position.log_index,
            locker_log_index,
            launch_log_index: launch.launched_log_index,
            initial_buy_swap_log_index,
            mint_count: 1,
            launch_swap_count,
        },
        entry: PonsPaperSwapQuote {
            amount_in: policy.amount_in,
            expected_output: entry_state.amount_out,
            min_receive: entry_min,
            slippage_bps: policy.slippage_bps,
            state_after: entry_state,
        },
        full_position_exit: PonsPaperSwapQuote {
            amount_in: exit_state.amount_in_requested,
            expected_output: exit_state.amount_out,
            min_receive: exit_min,
            slippage_bps: policy.slippage_bps,
            state_after: exit_state,
        },
        simulated_round_trip_return_bps: round_trip_return_bps,
        execution_eligible: false,
        execution_blocker:
            "paper_only_current_factory_source_prediction_restriction_and_route_gates_not_satisfied"
                .into(),
        broadcast: false,
    })
}

fn validate_envelope(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    policy: PonsQuotePolicy,
) -> Result<(), PonsQuoteError> {
    if transaction.to != Some(PONS_CURRENT_FACTORY)
        || transaction.hash != receipt.transaction_hash
        || transaction.l2_block_number != Some(receipt.l2_block_number)
        || transaction.transaction_index != Some(receipt.transaction_index)
        || !receipt.status
        || receipt.transaction_hash == B256::ZERO
        || receipt.block_hash == B256::ZERO
        || receipt
            .logs
            .windows(2)
            .any(|pair| pair[0].log_index >= pair[1].log_index)
        || policy.amount_in == U256::ZERO
        || policy.max_amount_in == U256::ZERO
        || policy.amount_in > policy.max_amount_in
        || policy.slippage_bps >= BPS_DENOMINATOR
    {
        return Err(PonsQuoteError::InvalidEnvelope);
    }
    Ok(())
}

fn exact_launch_identity(
    logs: &[ReceiptLog],
    expected_deployer: Address,
) -> Result<LaunchIdentity, PonsQuoteError> {
    let deployed = logs
        .iter()
        .filter(|log| {
            log.address == PONS_CURRENT_FACTORY
                && log.topics.first() == Some(&PONS_TOKEN_DEPLOYED_TOPIC)
        })
        .collect::<Vec<_>>();
    let launched = logs
        .iter()
        .filter(|log| {
            log.address == PONS_CURRENT_FACTORY
                && log.topics.first() == Some(&PONS_TOKEN_LAUNCHED_TOPIC)
        })
        .collect::<Vec<_>>();
    if deployed.len() != 1 || launched.len() != 1 {
        return Err(PonsQuoteError::LaunchIdentity);
    }
    let deployed_event = events::TokenDeployed::decode_raw_log_validate(
        deployed[0].topics.iter().copied(),
        &deployed[0].data,
    )
    .map_err(|_| PonsQuoteError::LaunchIdentity)?;
    let launched_event = events::TokenLaunched::decode_raw_log_validate(
        launched[0].topics.iter().copied(),
        &launched[0].data,
    )
    .map_err(|_| PonsQuoteError::LaunchIdentity)?;
    if deployed_event.token != launched_event.token
        || deployed_event.deployer != expected_deployer
        || launched_event.deployer != expected_deployer
        || deployed_event.dexFactory != PONS_V3_FACTORY
        || launched_event.dexFactory != PONS_V3_FACTORY
        || deployed_event.pairToken != PONS_WETH
        || launched_event.pairToken != PONS_WETH
        || deployed_event.launchConfigId != U256::from(PONS_LAUNCH_CONFIG_ID)
        || launched_event.launchConfigId != U256::from(PONS_LAUNCH_CONFIG_ID)
        || deployed_event.dexConfigId != U256::from(PONS_DEX_CONFIG_ID)
        || launched_event.dexConfigId != U256::from(PONS_DEX_CONFIG_ID)
        || deployed_event.token == Address::ZERO
        || launched_event.pool == Address::ZERO
        || launched_event.positionId == U256::ZERO
        || deployed[0].log_index >= launched[0].log_index
    {
        return Err(PonsQuoteError::LaunchIdentity);
    }
    Ok(LaunchIdentity {
        token: launched_event.token,
        deployer: launched_event.deployer,
        pool: launched_event.pool,
        position_id: launched_event.positionId,
        restriction_end: u64::try_from(launched_event.restrictionsEndBlock)
            .map_err(|_| PonsQuoteError::RestrictionEvidence)?,
        initial_buy_amount: launched_event.initialBuyAmount,
        deployed_log_index: deployed[0].log_index,
        launched_log_index: launched[0].log_index,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_initial_buy(
    pool: &V3PoolState,
    launch: LaunchIdentity,
    developer_wallet: Address,
    sender: Address,
    recipient: Address,
    amount0: I256,
    amount1: I256,
    sqrt_price_x96: U256,
    liquidity: u128,
    tick: i32,
) -> Result<V3Quote, PonsQuoteError> {
    if launch.initial_buy_amount == U256::ZERO
        || sender != PONS_SWAP_ROUTER_02
        || recipient != developer_wallet
    {
        return Err(PonsQuoteError::InitialBuyMismatch);
    }
    let (weth_delta, token_delta) = if pool.token0 == PONS_WETH {
        (amount0, amount1)
    } else {
        (amount1, amount0)
    };
    if weth_delta.is_negative()
        || weth_delta.into_raw() != launch.initial_buy_amount
        || !token_delta.is_negative()
    {
        return Err(PonsQuoteError::InitialBuyMismatch);
    }
    let reconstructed = pool.quote_exact_input(PONS_WETH, launch.initial_buy_amount, None)?;
    if reconstructed.amount_in_consumed != launch.initial_buy_amount
        || reconstructed.amount_out != token_delta.unsigned_abs()
        || reconstructed.sqrt_price_x96_after != sqrt_price_x96
        || reconstructed.tick_after != tick
        || reconstructed.liquidity_after != liquidity
    {
        return Err(PonsQuoteError::InitialBuyMismatch);
    }
    Ok(reconstructed)
}

fn validate_initial_buy_transfers(
    logs: &[ReceiptLog],
    launch: LaunchIdentity,
    developer_wallet: Address,
    reconstructed: Option<&V3Quote>,
    swap_log_index: Option<u64>,
) -> Result<(), PonsQuoteError> {
    let token_transfers_after_launch = logs.iter().filter(|log| {
        log.address == launch.token
            && log.topics.first() == Some(&ERC20_OR_721_TRANSFER_TOPIC)
            && log.topics.len() == 3
            && log.log_index > launch.launched_log_index
    });
    let weth_transfers_after_launch = logs.iter().filter(|log| {
        log.address == PONS_WETH
            && log.topics.first() == Some(&ERC20_OR_721_TRANSFER_TOPIC)
            && log.topics.len() == 3
            && log.log_index > launch.launched_log_index
    });
    if launch.initial_buy_amount == U256::ZERO {
        if reconstructed.is_some()
            || swap_log_index.is_some()
            || token_transfers_after_launch.count() != 0
            || weth_transfers_after_launch.count() != 0
        {
            return Err(PonsQuoteError::InitialBuyMismatch);
        }
        return Ok(());
    }

    let reconstructed = reconstructed.ok_or(PonsQuoteError::InitialBuyMismatch)?;
    let swap_log_index = swap_log_index.ok_or(PonsQuoteError::InitialBuyMismatch)?;
    let before_swap = |log: &&ReceiptLog| log.log_index < swap_log_index;
    if token_transfers_after_launch
        .filter(before_swap)
        .filter(|log| {
            topic_address(log, 1) == Some(launch.pool)
                && topic_address(log, 2) == Some(developer_wallet)
                && data_u256(log) == Some(reconstructed.amount_out)
        })
        .count()
        != 1
        || weth_transfers_after_launch
            .clone()
            .filter(before_swap)
            .filter(|log| {
                topic_address(log, 1) == Some(Address::ZERO)
                    && topic_address(log, 2) == Some(PONS_SWAP_ROUTER_02)
                    && data_u256(log) == Some(launch.initial_buy_amount)
            })
            .count()
            != 1
        || weth_transfers_after_launch
            .filter(before_swap)
            .filter(|log| {
                topic_address(log, 1) == Some(PONS_SWAP_ROUTER_02)
                    && topic_address(log, 2) == Some(launch.pool)
                    && data_u256(log) == Some(launch.initial_buy_amount)
            })
            .count()
            != 1
    {
        return Err(PonsQuoteError::InitialBuyMismatch);
    }
    Ok(())
}

fn validate_position_and_token_flow(
    logs: &[ReceiptLog],
    launch: LaunchIdentity,
    position: PositionEvidence,
    token0: Address,
    pool_created_log_index: u64,
) -> Result<u64, PonsQuoteError> {
    let locked = logs
        .iter()
        .filter(|log| {
            log.address == PONS_CURRENT_LOCKER
                && log.topics.first() == Some(&PONS_POSITION_LOCKED_TOPIC)
        })
        .collect::<Vec<_>>();
    if locked.len() != 1 {
        return Err(PonsQuoteError::PositionLockIdentity);
    }
    let lock = events::PositionLocked::decode_raw_log_validate(
        locked[0].topics.iter().copied(),
        &locked[0].data,
    )
    .map_err(|_| PonsQuoteError::PositionLockIdentity)?;
    if lock.token != launch.token
        || lock.creator != launch.deployer
        || lock.dexConfigId != U256::from(PONS_DEX_CONFIG_ID)
        || lock.pairToken != PONS_WETH
        || lock.positionId != launch.position_id
        || lock.positionManager != PONS_POSITION_MANAGER
    {
        return Err(PonsQuoteError::PositionLockIdentity);
    }

    let increase = logs
        .iter()
        .filter(|log| log.address == PONS_POSITION_MANAGER)
        .filter_map(|log| {
            events::IncreaseLiquidity::decode_raw_log_validate(
                log.topics.iter().copied(),
                &log.data,
            )
            .ok()
            .map(|event| (log.log_index, event))
        })
        .filter(|(_, event)| event.tokenId == launch.position_id)
        .collect::<Vec<_>>();
    if increase.len() != 1
        || increase[0].1.liquidity != position.liquidity
        || increase[0].1.amount0 != position.amount0
        || increase[0].1.amount1 != position.amount1
        || increase[0].0 <= position.log_index
    {
        return Err(PonsQuoteError::PositionLockIdentity);
    }

    let transfers = logs
        .iter()
        .filter(|log| {
            log.address == PONS_POSITION_MANAGER
                && log.topics.first() == Some(&ERC20_OR_721_TRANSFER_TOPIC)
                && log.topics.len() == 4
                && U256::from_be_slice(log.topics[3].as_slice()) == launch.position_id
        })
        .collect::<Vec<_>>();
    if transfers.len() != 2
        || topic_address(transfers[0], 1) != Some(Address::ZERO)
        || topic_address(transfers[0], 2) != Some(PONS_CURRENT_FACTORY)
        || topic_address(transfers[1], 1) != Some(PONS_CURRENT_FACTORY)
        || topic_address(transfers[1], 2) != Some(PONS_CURRENT_LOCKER)
        || transfers[0].log_index <= position.log_index
        || transfers[1].log_index <= transfers[0].log_index
        || transfers[1].log_index >= locked[0].log_index
    {
        return Err(PonsQuoteError::PositionLockIdentity);
    }

    let supply = pons_total_supply()?;
    let token_transfers = logs
        .iter()
        .filter(|log| {
            log.address == launch.token
                && log.topics.first() == Some(&ERC20_OR_721_TRANSFER_TOPIC)
                && log.topics.len() == 3
        })
        .collect::<Vec<_>>();
    let mint_supply = token_transfers.iter().filter(|log| {
        topic_address(log, 1) == Some(Address::ZERO)
            && topic_address(log, 2) == Some(PONS_CURRENT_FACTORY)
            && data_u256(log) == Some(supply)
    });
    if mint_supply.count() != 1
        || token_transfers.iter().any(|log| {
            topic_address(log, 1) != Some(Address::ZERO)
                && topic_address(log, 2) == Some(Address::ZERO)
        })
    {
        return Err(PonsQuoteError::StateSequence);
    }
    let token_liquidity = if token0 == launch.token {
        position.amount0
    } else {
        position.amount1
    };
    let quote_liquidity = if token0 == PONS_WETH {
        position.amount0
    } else {
        position.amount1
    };
    if token_liquidity == U256::ZERO || quote_liquidity != U256::ZERO {
        return Err(PonsQuoteError::StateSequence);
    }
    if token_transfers
        .iter()
        .filter(|log| {
            topic_address(log, 1) == Some(PONS_CURRENT_FACTORY)
                && topic_address(log, 2) == Some(launch.pool)
                && data_u256(log) == Some(token_liquidity)
                && log.log_index > pool_created_log_index
                && log.log_index < position.log_index
        })
        .count()
        != 1
    {
        return Err(PonsQuoteError::StateSequence);
    }
    Ok(locked[0].log_index)
}

fn topic_address(log: &ReceiptLog, index: usize) -> Option<Address> {
    let topic = log.topics.get(index)?;
    if topic.as_slice()[..12].iter().any(|byte| *byte != 0) {
        return None;
    }
    Some(Address::from_slice(&topic.as_slice()[12..]))
}

fn data_u256(log: &ReceiptLog) -> Option<U256> {
    (log.data.len() == 32).then(|| U256::from_be_slice(&log.data))
}

fn sorted_pair(token: Address, quote: Address) -> Result<(Address, Address), PonsQuoteError> {
    if token == Address::ZERO || quote == Address::ZERO || token == quote {
        return Err(PonsQuoteError::PoolIdentity);
    }
    Ok(if token < quote {
        (token, quote)
    } else {
        (quote, token)
    })
}

fn pons_total_supply() -> Result<U256, PonsQuoteError> {
    U256::from(1_000_000_000_u64)
        .checked_mul(U256::from(1_000_000_000_000_000_000_u64))
        .ok_or(PonsQuoteError::IncompleteQuote)
}

fn apply_slippage(amount: U256, slippage_bps: u16) -> Result<U256, PonsQuoteError> {
    let minimum = amount
        .checked_mul(U256::from(BPS_DENOMINATOR - slippage_bps))
        .ok_or(PonsQuoteError::IncompleteQuote)?
        / U256::from(BPS_DENOMINATOR);
    if minimum == U256::ZERO || minimum > amount {
        return Err(PonsQuoteError::IncompleteQuote);
    }
    Ok(minimum)
}

fn validate_complete_quote(quote: &V3Quote, requested: U256) -> Result<(), PonsQuoteError> {
    if quote.amount_in_requested != requested
        || quote.amount_in_consumed != requested
        || quote.amount_out == U256::ZERO
    {
        return Err(PonsQuoteError::IncompleteQuote);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::noxa_rpc::RobinhoodBlock;
    use crate::pons::PONS_CURRENT_LAUNCH_FIXTURE_TX;

    #[derive(Deserialize)]
    struct LiveFixture {
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
    }

    #[derive(Deserialize)]
    struct SwapDifferentialFixture {
        pre_state: DifferentialPreState,
        observed_swap: DifferentialSwap,
    }

    #[derive(Deserialize)]
    struct DifferentialPreState {
        pool: Address,
        token0: Address,
        token1: Address,
        fee_ppm: u32,
        tick_spacing: i32,
        sqrt_price_x96: U256,
        tick: i32,
        position: DifferentialPosition,
    }

    #[derive(Deserialize)]
    struct DifferentialPosition {
        tick_lower: i32,
        tick_upper: i32,
        liquidity: U256,
    }

    #[derive(Deserialize)]
    struct DifferentialSwap {
        gross_amount_in: U256,
        price_moving_amount_in: U256,
        amount_out: U256,
        sqrt_price_x96: U256,
        liquidity: U256,
        tick: i32,
        initialized_ticks_crossed: usize,
        fee_ppm: u32,
    }

    fn live_fixture() -> LiveFixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/pons-current-live-proof.json"
        ))
        .unwrap()
    }

    fn initial_buy_fixture() -> LiveFixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/pons-current-initial-buy-live-proof.json"
        ))
        .unwrap()
    }

    fn token_below_weth_fixture() -> LiveFixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/pons-current-token-below-weth-live-proof.json"
        ))
        .unwrap()
    }

    fn policy() -> PonsQuotePolicy {
        PonsQuotePolicy {
            amount_in: U256::from(1_000_000_000_000_000_u64),
            max_amount_in: U256::from(10_000_000_000_000_000_u64),
            slippage_bps: 100,
        }
    }

    fn swap_differential_fixture() -> SwapDifferentialFixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/pons-current-first-swap-differential.json"
        ))
        .unwrap()
    }

    #[test]
    fn reviewed_event_signatures_and_fixture_identity_are_stable() {
        assert_eq!(
            events::TokenDeployed::SIGNATURE_HASH,
            PONS_TOKEN_DEPLOYED_TOPIC
        );
        assert_eq!(
            events::TokenLaunched::SIGNATURE_HASH,
            PONS_TOKEN_LAUNCHED_TOPIC
        );
        assert_eq!(
            events::PositionLocked::SIGNATURE_HASH,
            PONS_POSITION_LOCKED_TOPIC
        );
        assert_eq!(
            PONS_CURRENT_LAUNCH_FIXTURE_TX,
            alloy_primitives::b256!(
                "cce2b414f04ad3caab0ad38bc10cc1ac0741ed95ac740495535b71c8302fcc41"
            )
        );
    }

    #[test]
    fn reconstructs_real_current_generation_no_buy_launch_and_quotes_round_trip() {
        let fixture = live_fixture();
        assert_eq!(fixture.transaction.hash, PONS_CURRENT_LAUNCH_FIXTURE_TX);
        assert_eq!(fixture.block.hash, fixture.receipt.block_hash);
        assert_eq!(
            fixture.block.l1_block_number,
            fixture.receipt.l1_block_number.unwrap()
        );
        let quote = quote_pons_launch_receipt(
            &fixture.transaction,
            &fixture.receipt,
            PonsExpectedProfile::production(),
            policy(),
        )
        .unwrap();
        assert_eq!(
            quote.market.token,
            alloy_primitives::address!("432c99bbd9dc1d9040087598d7cf40502d7cc20b")
        );
        assert_eq!(
            quote.market.pool,
            alloy_primitives::address!("a1ad01da59552689835902b878ce6f5ea37f2b0b")
        );
        assert_eq!(quote.market.position_id, U256::from(109_352_u64));
        assert_eq!(quote.market.launch_swap_count, 0);
        assert_eq!(quote.market.initialize_tick, 204_200);
        assert_eq!(
            quote.state_version.restriction_end_l1_block_number,
            fixture.block.l1_block_number + PONS_RESTRICTION_L1_BLOCKS
        );
        assert!(quote.entry.expected_output > U256::ZERO);
        assert!(quote.full_position_exit.expected_output > U256::ZERO);
        assert!(quote.entry.min_receive < quote.entry.expected_output);
        assert!(!quote.execution_eligible);
        assert!(!quote.broadcast);
    }

    #[test]
    fn exact_first_public_swap_matches_independent_v3_math() {
        let fixture = swap_differential_fixture();
        let pre = fixture.pre_state;
        let observed = fixture.observed_swap;
        assert_eq!(pre.fee_ppm, observed.fee_ppm);
        let mut state = V3PoolState::new(
            pre.pool,
            pre.token0,
            pre.token1,
            pre.fee_ppm,
            pre.tick_spacing,
            pre.sqrt_price_x96,
            pre.tick,
            0,
        )
        .unwrap();
        state
            .add_position(
                pre.position.tick_lower,
                pre.position.tick_upper,
                u128::try_from(pre.position.liquidity).unwrap(),
            )
            .unwrap();
        let quote = state
            .quote_exact_input(PONS_WETH, observed.gross_amount_in, None)
            .unwrap();
        let price_moving = observed.gross_amount_in * U256::from(1_000_000_u32 - observed.fee_ppm)
            / U256::from(1_000_000_u32);
        assert_eq!(price_moving, observed.price_moving_amount_in);
        assert_eq!(quote.amount_out, observed.amount_out);
        assert_eq!(quote.sqrt_price_x96_after, observed.sqrt_price_x96);
        assert_eq!(quote.tick_after, observed.tick);
        assert_eq!(U256::from(quote.liquidity_after), observed.liquidity);
        assert_eq!(
            quote.initialized_ticks_crossed,
            observed.initialized_ticks_crossed
        );
    }

    #[test]
    fn reconstructs_real_current_initial_buy_and_both_token_orientations() {
        let initial_buy = initial_buy_fixture();
        let initial_quote = quote_pons_launch_receipt(
            &initial_buy.transaction,
            &initial_buy.receipt,
            PonsExpectedProfile::production(),
            policy(),
        )
        .unwrap();
        assert_eq!(initial_quote.market.launch_swap_count, 1);
        assert_eq!(
            initial_quote.market.initial_buy_amount,
            U256::from(1_000_000_000_000_000_u64)
        );
        assert!(initial_quote.market.token > PONS_WETH);
        assert_eq!(initial_quote.market.initialize_tick, 204_200);

        let below = token_below_weth_fixture();
        let below_quote = quote_pons_launch_receipt(
            &below.transaction,
            &below.receipt,
            PonsExpectedProfile::production(),
            policy(),
        )
        .unwrap();
        assert_eq!(below_quote.market.launch_swap_count, 1);
        assert!(below_quote.market.initial_buy_amount > U256::ZERO);
        assert!(below_quote.market.token < PONS_WETH);
        assert_eq!(below_quote.market.initialize_tick, -204_200);
        assert!(below_quote.entry.expected_output > U256::ZERO);
        assert!(below_quote.full_position_exit.expected_output > U256::ZERO);
    }

    #[test]
    fn rejects_incomplete_or_tampered_current_generation_receipts() {
        let fixture = live_fixture();

        let mut missing_token_flow = fixture.receipt.clone();
        missing_token_flow.logs.retain(|log| log.log_index != 26);
        assert!(matches!(
            quote_pons_launch_receipt(
                &fixture.transaction,
                &missing_token_flow,
                PonsExpectedProfile::production(),
                policy()
            ),
            Err(PonsQuoteError::StateSequence)
        ));

        let mut wrong_locker = fixture.receipt.clone();
        wrong_locker
            .logs
            .iter_mut()
            .find(|log| log.address == PONS_CURRENT_LOCKER)
            .unwrap()
            .address = Address::with_last_byte(0xee);
        assert!(matches!(
            quote_pons_launch_receipt(
                &fixture.transaction,
                &wrong_locker,
                PonsExpectedProfile::production(),
                policy()
            ),
            Err(PonsQuoteError::PositionLockIdentity)
        ));

        let mut changed_restriction = fixture.receipt.clone();
        let launched = changed_restriction
            .logs
            .iter_mut()
            .find(|log| log.topics.first() == Some(&PONS_TOKEN_LAUNCHED_TOPIC))
            .unwrap();
        let mut launched_data = launched.data.to_vec();
        launched_data[191] ^= 1;
        launched.data = launched_data.into();
        assert!(matches!(
            quote_pons_launch_receipt(
                &fixture.transaction,
                &changed_restriction,
                PonsExpectedProfile::production(),
                policy()
            ),
            Err(PonsQuoteError::RestrictionEvidence)
        ));

        let mut inconsistent_value = fixture.transaction.clone();
        inconsistent_value.value += U256::from(1_u8);
        assert!(matches!(
            quote_pons_launch_receipt(
                &inconsistent_value,
                &fixture.receipt,
                PonsExpectedProfile::production(),
                policy()
            ),
            Err(PonsQuoteError::InitialBuyMismatch)
        ));

        let mut unexpected_burn = fixture.receipt.clone();
        let mut burn = unexpected_burn
            .logs
            .iter()
            .find(|log| log.log_index == 21)
            .unwrap()
            .clone();
        burn.log_index = 35;
        burn.topics[1] = burn.topics[2];
        burn.topics[2] = B256::ZERO;
        unexpected_burn.logs.push(burn);
        assert!(matches!(
            quote_pons_launch_receipt(
                &fixture.transaction,
                &unexpected_burn,
                PonsExpectedProfile::production(),
                policy()
            ),
            Err(PonsQuoteError::StateSequence)
        ));
    }
}
