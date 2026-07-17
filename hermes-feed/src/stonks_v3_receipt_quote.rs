//! Independent, receipt-confirmed paper quotes for the exact StonksLauncherV3
//! WETH launch profile.
//!
//! The candidate path does not call this module. A caller must first produce a
//! [`StonksV3ObservationEvidence`] with the receipt-block observer, which pins
//! the direct wrapper, launcher dependencies, canonical pool, and exact eleven
//! minted positions. This module then reconstructs the receipt-end V3 state
//! without using the launcher's trade size (the reviewed direct launch has no
//! embedded buy), quotes a fixed tiny WETH entry, and quotes an immediate full
//! position exit. It never constructs a transaction or authorizes execution.

use alloy_primitives::{Address, B256, U256, keccak256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::launchpad_adapter::{LaunchpadId, WrapperKind};
use crate::noxa_predict::predict_v3_pool_address;
use crate::robinhood::{
    CHAIN_ID, UNISWAP_V3_FACTORY, UNISWAP_V3_FACTORY_RUNTIME_KECCAK256,
    UNISWAP_V3_POOL_INIT_CODE_KECCAK256, WETH, WETH_RUNTIME_KECCAK256,
};
use crate::stonks_v3_observer::{
    STONKS_V3_AIRLOCK, STONKS_V3_AIRLOCK_RUNTIME_HASH, STONKS_V3_BUNDLER,
    STONKS_V3_BUNDLER_RUNTIME_HASH, STONKS_V3_DN404_FACTORY, STONKS_V3_DN404_FACTORY_RUNTIME_HASH,
    STONKS_V3_DN404_RUNTIME_HASH, STONKS_V3_GOVERNANCE_FACTORY,
    STONKS_V3_GOVERNANCE_FACTORY_RUNTIME_HASH, STONKS_V3_INITIALIZER,
    STONKS_V3_INITIALIZER_RUNTIME_HASH, STONKS_V3_LAUNCHER, STONKS_V3_LAUNCHER_RUNTIME_HASH,
    STONKS_V3_MIGRATOR, STONKS_V3_MIGRATOR_RUNTIME_HASH, STONKS_V3_MIRROR_RUNTIME_HASH,
    STONKS_V3_USDG, STONKS_V3_USDG_IMPLEMENTATION, STONKS_V3_USDG_IMPLEMENTATION_RUNTIME_HASH,
    STONKS_V3_USDG_OWNER, STONKS_V3_USDG_OWNER_RUNTIME_HASH, STONKS_V3_USDG_RUNTIME_HASH,
    StonksV3ObservationEvidence, StonksV3PositionEvidence,
};
use crate::v3_pool::{V3PoolError, V3PoolState, V3Quote};

const FEE_PPM: u32 = 10_000;
const TICK_SPACING: i32 = 200;
const INITIAL_TICK: i32 = -197_400;
const FINAL_DENSE_TICK: i32 = -144_400;
const INITIAL_SQRT_PRICE_X96: U256 = alloy_primitives::uint!(0x363db22b79374d1d73fc0_U256);
const BPS_DENOMINATOR: u16 = 10_000;
const FIXED_AMOUNT_IN_WEI: u64 = 1_000_000_000_000_000;
const FIXED_MAX_AMOUNT_IN_WEI: u64 = 10_000_000_000_000_000;
const FIXED_SLIPPAGE_BPS: u16 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct StonksV3QuotePolicy {
    pub amount_in: U256,
    pub max_amount_in: U256,
    pub slippage_bps: u16,
}

pub fn stonks_v3_quote_policy() -> StonksV3QuotePolicy {
    StonksV3QuotePolicy {
        amount_in: U256::from(FIXED_AMOUNT_IN_WEI),
        max_amount_in: U256::from(FIXED_MAX_AMOUNT_IN_WEI),
        slippage_bps: FIXED_SLIPPAGE_BPS,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct StonksV3RuntimePin {
    pub address: Address,
    pub runtime_hash: B256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StonksV3DependencyPins {
    pub wrapper: WrapperKind,
    pub runtime_pins: Vec<StonksV3RuntimePin>,
    pub pool_init_code_hash: B256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StonksV3StateVersion {
    pub chain_id: u64,
    pub block_hash: B256,
    pub l2_block_number: u64,
    pub transaction_index: u64,
    pub terminal_log_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StonksV3MarketEvidence {
    pub leader: Address,
    pub creator: Address,
    pub launcher: Address,
    pub token: Address,
    pub token_runtime_hash: B256,
    pub mirror: Address,
    pub mirror_runtime_hash: B256,
    pub pool: Address,
    pub quote_asset: Address,
    pub fee_ppm: u32,
    pub tick_spacing: i32,
    pub initialize_sqrt_price_x96: U256,
    pub initialize_tick: i32,
    pub receipt_end_sqrt_price_x96: U256,
    pub receipt_end_tick: i32,
    #[serde(
        serialize_with = "serialize_u128_hex",
        deserialize_with = "deserialize_u128_hex"
    )]
    pub receipt_end_liquidity: u128,
    pub position_count: usize,
    pub positions: Vec<StonksV3PositionEvidence>,
    pub dependency_pins: StonksV3DependencyPins,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StonksV3PaperSwapQuote {
    pub amount_in: U256,
    pub expected_output: U256,
    pub min_receive: U256,
    pub pool_fee_ppm: u32,
    pub slippage_bps: u16,
    pub state_after: V3Quote,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StonksV3ReceiptPaperQuote {
    pub record_type: String,
    pub tx_hash: B256,
    pub launchpad: LaunchpadId,
    pub profile: String,
    pub l2_block_number: u64,
    pub state_version: StonksV3StateVersion,
    pub quote_source: String,
    pub sizing_source: String,
    pub max_amount_in: U256,
    pub candidate_time_prediction_available: bool,
    /// Keccak of the exact receipt-block observer record before reconciliation
    /// updates its quote status. This binds the quote to the independently
    /// emitted leader, mirror, pool, block and receipt-log provenance.
    pub observation_proof_keccak256: B256,
    pub market: StonksV3MarketEvidence,
    pub entry: StonksV3PaperSwapQuote,
    pub full_position_exit: StonksV3PaperSwapQuote,
    pub simulated_round_trip_return_bps: U256,
    pub paper_evidence_ready: bool,
    pub authorizes_canary: bool,
    pub execution_eligible: bool,
    pub execution_blocker: String,
    pub broadcast: bool,
}

#[derive(Debug, Error)]
pub enum StonksV3QuoteError {
    #[error("quote policy amount or slippage is unsafe")]
    UnsafePolicy,
    #[error("observation is not the exact receipt-confirmed Stonks V3 WETH profile")]
    ObservationProfile,
    #[error("serialized dependency pins differ from the reviewed Stonks profile")]
    DependencyPins,
    #[error("receipt-end position order, liquidity, or currency orientation is incomplete")]
    PositionProfile,
    #[error("local quote did not consume all input or returned zero output")]
    IncompleteQuote,
    #[error("serialized Stonks quote cannot be independently replayed")]
    QuoteReplayMismatch,
    #[error("quote arithmetic overflow")]
    ArithmeticOverflow,
    #[error(transparent)]
    Pool(#[from] V3PoolError),
}

fn serialize_u128_hex<S>(value: &u128, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&format!("0x{value:x}"))
}

fn deserialize_u128_hex<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    let digits = value
        .strip_prefix("0x")
        .ok_or_else(|| serde::de::Error::custom("u128 hex value must start with 0x"))?;
    u128::from_str_radix(digits, 16).map_err(serde::de::Error::custom)
}

pub fn stonks_v3_dependency_pins() -> StonksV3DependencyPins {
    StonksV3DependencyPins {
        wrapper: WrapperKind::Direct,
        runtime_pins: vec![
            pin(STONKS_V3_LAUNCHER, STONKS_V3_LAUNCHER_RUNTIME_HASH),
            pin(STONKS_V3_AIRLOCK, STONKS_V3_AIRLOCK_RUNTIME_HASH),
            pin(STONKS_V3_BUNDLER, STONKS_V3_BUNDLER_RUNTIME_HASH),
            pin(
                STONKS_V3_DN404_FACTORY,
                STONKS_V3_DN404_FACTORY_RUNTIME_HASH,
            ),
            pin(STONKS_V3_INITIALIZER, STONKS_V3_INITIALIZER_RUNTIME_HASH),
            pin(
                STONKS_V3_GOVERNANCE_FACTORY,
                STONKS_V3_GOVERNANCE_FACTORY_RUNTIME_HASH,
            ),
            pin(STONKS_V3_MIGRATOR, STONKS_V3_MIGRATOR_RUNTIME_HASH),
            pin(STONKS_V3_USDG, STONKS_V3_USDG_RUNTIME_HASH),
            pin(
                STONKS_V3_USDG_IMPLEMENTATION,
                STONKS_V3_USDG_IMPLEMENTATION_RUNTIME_HASH,
            ),
            pin(STONKS_V3_USDG_OWNER, STONKS_V3_USDG_OWNER_RUNTIME_HASH),
            pin(WETH, WETH_RUNTIME_KECCAK256),
            pin(UNISWAP_V3_FACTORY, UNISWAP_V3_FACTORY_RUNTIME_KECCAK256),
        ],
        pool_init_code_hash: UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    }
}

const fn pin(address: Address, runtime_hash: B256) -> StonksV3RuntimePin {
    StonksV3RuntimePin {
        address,
        runtime_hash,
    }
}

pub fn quote_stonks_v3_observation(
    observation: &StonksV3ObservationEvidence,
) -> Result<StonksV3ReceiptPaperQuote, StonksV3QuoteError> {
    quote_stonks_v3_observation_with_policy(observation, stonks_v3_quote_policy())
}

fn quote_stonks_v3_observation_with_policy(
    observation: &StonksV3ObservationEvidence,
    policy: StonksV3QuotePolicy,
) -> Result<StonksV3ReceiptPaperQuote, StonksV3QuoteError> {
    validate_policy(policy)?;
    validate_observation(observation)?;
    let state = state_from_positions(
        observation.pool,
        observation.asset,
        &observation.positions,
        observation.initialize_sqrt_price_x96,
        observation.initialize_tick,
    )?;
    let receipt_end_liquidity = state.liquidity;
    let entry_state = state.quote_exact_input(WETH, policy.amount_in, None)?;
    validate_complete_quote(&entry_state, policy.amount_in)?;
    let entry_min = apply_slippage(entry_state.amount_out, policy.slippage_bps)?;
    let mut post_entry = state.clone();
    post_entry.set_observation(
        entry_state.sqrt_price_x96_after,
        entry_state.tick_after,
        entry_state.liquidity_after,
    )?;
    let exit_state =
        post_entry.quote_exact_input(observation.asset, entry_state.amount_out, None)?;
    validate_complete_quote(&exit_state, entry_state.amount_out)?;
    let exit_min = apply_slippage(exit_state.amount_out, policy.slippage_bps)?;
    let round_trip = exit_state
        .amount_out
        .checked_mul(U256::from(BPS_DENOMINATOR))
        .ok_or(StonksV3QuoteError::ArithmeticOverflow)?
        / policy.amount_in;
    let terminal_log_index = observation
        .positions
        .last()
        .ok_or(StonksV3QuoteError::PositionProfile)?
        .log_index;

    Ok(StonksV3ReceiptPaperQuote {
        record_type: "launchpad_stonks_v3_paper_quote".into(),
        tx_hash: observation.tx_hash,
        launchpad: LaunchpadId::StonksV3,
        profile: "stonks_v3_direct_weth_receipt_confirmed".into(),
        l2_block_number: observation.l2_block_number,
        state_version: StonksV3StateVersion {
            chain_id: CHAIN_ID,
            block_hash: observation.block_hash,
            l2_block_number: observation.l2_block_number,
            transaction_index: observation.transaction_index,
            terminal_log_index,
        },
        quote_source: "confirmed_receipt_end_stonks_v3_exact_eleven_position_state".into(),
        sizing_source: "independent_fixed_tiny_weth_policy_not_launcher_size".into(),
        max_amount_in: policy.max_amount_in,
        candidate_time_prediction_available: false,
        observation_proof_keccak256: stonks_v3_observation_proof_keccak256(observation)?,
        market: StonksV3MarketEvidence {
            leader: observation.leader,
            creator: observation.creator,
            launcher: observation.launcher,
            token: observation.asset,
            token_runtime_hash: STONKS_V3_DN404_RUNTIME_HASH,
            mirror: observation.mirror,
            mirror_runtime_hash: STONKS_V3_MIRROR_RUNTIME_HASH,
            pool: observation.pool,
            quote_asset: WETH,
            fee_ppm: FEE_PPM,
            tick_spacing: TICK_SPACING,
            initialize_sqrt_price_x96: observation.initialize_sqrt_price_x96,
            initialize_tick: observation.initialize_tick,
            receipt_end_sqrt_price_x96: state.sqrt_price_x96,
            receipt_end_tick: state.tick,
            receipt_end_liquidity,
            position_count: observation.positions.len(),
            positions: observation.positions.clone(),
            dependency_pins: stonks_v3_dependency_pins(),
        },
        entry: swap_quote(
            policy.amount_in,
            entry_min,
            policy.slippage_bps,
            entry_state,
        ),
        full_position_exit: swap_quote(
            exit_state.amount_in_requested,
            exit_min,
            policy.slippage_bps,
            exit_state,
        ),
        simulated_round_trip_return_bps: round_trip,
        paper_evidence_ready: false,
        authorizes_canary: false,
        execution_eligible: false,
        execution_blocker:
            "receipt_confirmed_paper_only_candidate_time_prediction_and_promotion_gates_unavailable"
                .into(),
        broadcast: false,
    })
}

pub fn validate_stonks_v3_quote_replay(
    quote: &StonksV3ReceiptPaperQuote,
) -> Result<(), StonksV3QuoteError> {
    let policy = stonks_v3_quote_policy();
    validate_policy(policy)?;
    if quote.record_type != "launchpad_stonks_v3_paper_quote"
        || quote.launchpad != LaunchpadId::StonksV3
        || quote.profile != "stonks_v3_direct_weth_receipt_confirmed"
        || quote.tx_hash == B256::ZERO
        || quote.l2_block_number != quote.state_version.l2_block_number
        || quote.state_version.chain_id != CHAIN_ID
        || quote.state_version.block_hash == B256::ZERO
        || quote.quote_source != "confirmed_receipt_end_stonks_v3_exact_eleven_position_state"
        || quote.sizing_source != "independent_fixed_tiny_weth_policy_not_launcher_size"
        || quote.max_amount_in != policy.max_amount_in
        || quote.candidate_time_prediction_available
        || quote.observation_proof_keccak256 == B256::ZERO
        || quote.paper_evidence_ready
        || quote.authorizes_canary
        || quote.execution_eligible
        || quote.execution_blocker
            != "receipt_confirmed_paper_only_candidate_time_prediction_and_promotion_gates_unavailable"
        || quote.broadcast
        || quote.market.dependency_pins != stonks_v3_dependency_pins()
        || quote.market.leader == Address::ZERO
        || quote.market.creator != quote.market.leader
        || quote.market.launcher != STONKS_V3_LAUNCHER
        || quote.market.token_runtime_hash != STONKS_V3_DN404_RUNTIME_HASH
        || quote.market.mirror == Address::ZERO
        || quote.market.mirror_runtime_hash != STONKS_V3_MIRROR_RUNTIME_HASH
        || quote.market.quote_asset != WETH
        || predict_v3_pool_address(
            UNISWAP_V3_FACTORY,
            quote.market.token,
            WETH,
            FEE_PPM,
            UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
        ) != quote.market.pool
        || quote.market.fee_ppm != FEE_PPM
        || quote.market.tick_spacing != TICK_SPACING
        || quote.market.initialize_sqrt_price_x96 != INITIAL_SQRT_PRICE_X96
        || quote.market.initialize_tick != INITIAL_TICK
        || quote.market.receipt_end_sqrt_price_x96 != INITIAL_SQRT_PRICE_X96
        || quote.market.receipt_end_tick != INITIAL_TICK
        || quote.market.position_count != 11
        || quote.market.positions.len() != 11
        || quote.state_version.terminal_log_index
            != quote
                .market
                .positions
                .last()
                .ok_or(StonksV3QuoteError::QuoteReplayMismatch)?
                .log_index
        || quote.entry.amount_in != policy.amount_in
        || quote.entry.slippage_bps != policy.slippage_bps
        || quote.full_position_exit.slippage_bps != policy.slippage_bps
        || quote.entry.pool_fee_ppm != FEE_PPM
        || quote.full_position_exit.pool_fee_ppm != FEE_PPM
    {
        return Err(StonksV3QuoteError::QuoteReplayMismatch);
    }
    validate_position_profile(&quote.market.positions)?;
    let state = state_from_positions(
        quote.market.pool,
        quote.market.token,
        &quote.market.positions,
        quote.market.receipt_end_sqrt_price_x96,
        quote.market.receipt_end_tick,
    )?;
    if state.liquidity != quote.market.receipt_end_liquidity {
        return Err(StonksV3QuoteError::QuoteReplayMismatch);
    }
    let entry = state.quote_exact_input(WETH, policy.amount_in, None)?;
    validate_complete_quote(&entry, policy.amount_in)?;
    if entry != quote.entry.state_after
        || entry.amount_out != quote.entry.expected_output
        || quote.entry.min_receive != apply_slippage(entry.amount_out, policy.slippage_bps)?
    {
        return Err(StonksV3QuoteError::QuoteReplayMismatch);
    }
    let mut post_entry = state;
    post_entry.set_observation(
        entry.sqrt_price_x96_after,
        entry.tick_after,
        entry.liquidity_after,
    )?;
    let exit = post_entry.quote_exact_input(quote.market.token, entry.amount_out, None)?;
    validate_complete_quote(&exit, entry.amount_out)?;
    let round_trip = exit
        .amount_out
        .checked_mul(U256::from(BPS_DENOMINATOR))
        .ok_or(StonksV3QuoteError::ArithmeticOverflow)?
        / policy.amount_in;
    if exit != quote.full_position_exit.state_after
        || quote.full_position_exit.amount_in != entry.amount_out
        || quote.full_position_exit.expected_output != exit.amount_out
        || quote.full_position_exit.min_receive
            != apply_slippage(exit.amount_out, policy.slippage_bps)?
        || quote.simulated_round_trip_return_bps != round_trip
    {
        return Err(StonksV3QuoteError::QuoteReplayMismatch);
    }
    Ok(())
}

fn validate_policy(policy: StonksV3QuotePolicy) -> Result<(), StonksV3QuoteError> {
    if policy.amount_in == U256::ZERO
        || policy.max_amount_in == U256::ZERO
        || policy.amount_in > policy.max_amount_in
        || policy.slippage_bps == 0
        || policy.slippage_bps >= BPS_DENOMINATOR
    {
        return Err(StonksV3QuoteError::UnsafePolicy);
    }
    Ok(())
}

fn validate_observation(
    observation: &StonksV3ObservationEvidence,
) -> Result<(), StonksV3QuoteError> {
    if observation.record_type != "launchpad_stonks_v3_observation"
        || observation.profile != "stonks_v3_direct_launch"
        || observation.tx_hash == B256::ZERO
        || observation.chain_id != CHAIN_ID
        || observation.block_hash == B256::ZERO
        || observation.leader == Address::ZERO
        || observation.creator != observation.leader
        || observation.launcher != STONKS_V3_LAUNCHER
        || observation.asset == Address::ZERO
        || observation.asset >= WETH
        || observation.pool == Address::ZERO
        || observation.currency != 1
        || observation.numeraire != WETH
        || observation.initializer != STONKS_V3_INITIALIZER
        || observation.initialize_tick != INITIAL_TICK
        || observation.initialize_sqrt_price_x96 != INITIAL_SQRT_PRICE_X96
        || observation.position_count != 11
        || observation.positions.len() != 11
        || observation.paper_evidence_ready
        || observation.authorizes_canary
        || observation.execution_eligible
        || observation.broadcast
    {
        return Err(StonksV3QuoteError::ObservationProfile);
    }
    validate_position_profile(&observation.positions)
}

/// Hash the immutable receipt observer proof. Reconciliation is allowed to
/// replace only the legacy quote status/blocker after this module succeeds, so
/// normalize those mutable fields before hashing on either side of the JSONL
/// boundary.
pub fn stonks_v3_observation_proof_keccak256(
    observation: &StonksV3ObservationEvidence,
) -> Result<B256, StonksV3QuoteError> {
    validate_observation_shape_for_proof(observation)?;
    let mut canonical = observation.clone();
    canonical.quote_status = "unsupported".into();
    canonical.quote_blocker = "observe_only_stonks_v3_no_independent_quote_engine".into();
    canonical.paper_evidence_ready = false;
    canonical.authorizes_canary = false;
    canonical.execution_eligible = false;
    canonical.broadcast = false;
    serde_json::to_vec(&canonical)
        .map(keccak256)
        .map_err(|_| StonksV3QuoteError::ObservationProfile)
}

fn validate_observation_shape_for_proof(
    observation: &StonksV3ObservationEvidence,
) -> Result<(), StonksV3QuoteError> {
    let mut canonical = observation.clone();
    canonical.quote_status = "unsupported".into();
    canonical.quote_blocker = "observe_only_stonks_v3_no_independent_quote_engine".into();
    canonical.paper_evidence_ready = false;
    canonical.authorizes_canary = false;
    canonical.execution_eligible = false;
    canonical.broadcast = false;
    validate_observation(&canonical)
}

fn validate_position_profile(
    positions: &[StonksV3PositionEvidence],
) -> Result<(), StonksV3QuoteError> {
    let expected_lowers = [
        -197_400, -192_200, -186_800, -181_600, -176_200, -171_000, -165_600, -160_400, -155_000,
        -149_800, -144_400,
    ];
    let expected_liquidity = [
        "d337e874824d30601b",
        "118424301cff58bb532",
        "17af97258b0fee064fa",
        "2003ed986823d4bda3e",
        "2c7b0740803463e02d7",
        "3e6fd04fea88778a0bd",
        "5c0c6f336d2fca0c3d8",
        "8dae6c9e8dc828fb75e",
        "f86dc0a0b2f82f1b7f5",
        "2302c2d226824a4a3729",
        "2124c86ddee4d4e72e1a",
    ];
    let expected_amount0 = [
        "39e7139a8c08fa05ffa098",
        "39e7139a8c08fa05ffc48a",
        "39e7139a8c08fa05ffb76a",
        "39e7139a8c08fa05ffd0e1",
        "39e7139a8c08fa05fffe3f",
        "39e7139a8c08fa05fffbb6",
        "39e7139a8c08fa05fff225",
        "39e7139a8c08fa05fff1d3",
        "39e7139a8c08fa05fffc68",
        "39e7139a8c08fa05fffc72",
        "b0da228552db01b892506f",
    ];
    if positions.len() != 11 {
        return Err(StonksV3QuoteError::PositionProfile);
    }
    for (index, position) in positions.iter().enumerate() {
        let expected_upper = if index == 10 {
            887_200
        } else {
            FINAL_DENSE_TICK
        };
        let expected = U256::from_str_radix(expected_liquidity[index], 16)
            .map_err(|_| StonksV3QuoteError::PositionProfile)?;
        let expected_amount0 = U256::from_str_radix(expected_amount0[index], 16)
            .map_err(|_| StonksV3QuoteError::PositionProfile)?;
        if position.tick_lower != expected_lowers[index]
            || position.tick_upper != expected_upper
            || position.liquidity != expected
            || position.amount0 != expected_amount0
            || position.amount1 != U256::ZERO
            || (index > 0 && positions[index - 1].log_index >= position.log_index)
        {
            return Err(StonksV3QuoteError::PositionProfile);
        }
    }
    Ok(())
}

fn state_from_positions(
    pool: Address,
    token: Address,
    positions: &[StonksV3PositionEvidence],
    sqrt_price_x96: U256,
    tick: i32,
) -> Result<V3PoolState, StonksV3QuoteError> {
    validate_position_profile(positions)?;
    if token == Address::ZERO || token >= WETH || pool == Address::ZERO {
        return Err(StonksV3QuoteError::PositionProfile);
    }
    let mut state = V3PoolState::new(
        pool,
        token,
        WETH,
        FEE_PPM,
        TICK_SPACING,
        sqrt_price_x96,
        tick,
        0,
    )?;
    for position in positions {
        state.add_position(
            position.tick_lower,
            position.tick_upper,
            u128::try_from(position.liquidity).map_err(|_| StonksV3QuoteError::PositionProfile)?,
        )?;
    }
    if state.liquidity == 0 {
        return Err(StonksV3QuoteError::PositionProfile);
    }
    Ok(state)
}

fn validate_complete_quote(quote: &V3Quote, requested: U256) -> Result<(), StonksV3QuoteError> {
    if quote.amount_in_requested != requested
        || quote.amount_in_consumed != requested
        || quote.amount_out == U256::ZERO
    {
        return Err(StonksV3QuoteError::IncompleteQuote);
    }
    Ok(())
}

fn apply_slippage(amount: U256, slippage_bps: u16) -> Result<U256, StonksV3QuoteError> {
    let minimum = amount
        .checked_mul(U256::from(BPS_DENOMINATOR - slippage_bps))
        .ok_or(StonksV3QuoteError::ArithmeticOverflow)?
        / U256::from(BPS_DENOMINATOR);
    if minimum == U256::ZERO || minimum > amount {
        return Err(StonksV3QuoteError::IncompleteQuote);
    }
    Ok(minimum)
}

fn swap_quote(
    amount_in: U256,
    min_receive: U256,
    slippage_bps: u16,
    state_after: V3Quote,
) -> StonksV3PaperSwapQuote {
    StonksV3PaperSwapQuote {
        amount_in,
        expected_output: state_after.amount_out,
        min_receive,
        pool_fee_ppm: FEE_PPM,
        slippage_bps,
        state_after,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn observation() -> StonksV3ObservationEvidence {
        crate::stonks_v3_observer::tests::fixture_observation().await
    }

    fn policy() -> StonksV3QuotePolicy {
        stonks_v3_quote_policy()
    }

    #[tokio::test]
    async fn exact_observation_quotes_entry_and_full_exit() {
        let quote = quote_stonks_v3_observation(&observation().await).unwrap();
        assert_eq!(quote.entry.amount_in, policy().amount_in);
        assert_eq!(quote.entry.state_after.token_in, WETH);
        assert_eq!(quote.entry.state_after.token_out, quote.market.token);
        assert_eq!(
            quote.full_position_exit.amount_in,
            quote.entry.expected_output
        );
        assert_eq!(
            quote.full_position_exit.state_after.token_in,
            quote.market.token
        );
        assert_eq!(quote.full_position_exit.state_after.token_out, WETH);
        assert!(quote.entry.state_after.steps > 0);
        assert!(!quote.candidate_time_prediction_available);
        assert!(!quote.paper_evidence_ready);
        assert!(!quote.authorizes_canary);
        assert!(!quote.execution_eligible);
        assert!(!quote.broadcast);
        validate_stonks_v3_quote_replay(&quote).unwrap();
    }

    #[tokio::test]
    async fn fixed_policy_rejects_coherent_alternate_amount_slippage_and_cap() {
        let observed = observation().await;
        let amount = quote_stonks_v3_observation_with_policy(
            &observed,
            StonksV3QuotePolicy {
                amount_in: U256::from(5_000_000_000_000_000_u64),
                max_amount_in: U256::from(10_000_000_000_000_000_u64),
                slippage_bps: 100,
            },
        )
        .unwrap();
        assert_eq!(
            amount.entry.amount_in,
            U256::from(5_000_000_000_000_000_u64)
        );
        assert!(validate_stonks_v3_quote_replay(&amount).is_err());
        let slippage = quote_stonks_v3_observation_with_policy(
            &observed,
            StonksV3QuotePolicy {
                amount_in: U256::from(1_000_000_000_000_000_u64),
                max_amount_in: U256::from(10_000_000_000_000_000_u64),
                slippage_bps: 500,
            },
        )
        .unwrap();
        assert_eq!(slippage.entry.slippage_bps, 500);
        assert!(validate_stonks_v3_quote_replay(&slippage).is_err());
        let mut cap = quote_stonks_v3_observation(&observed).unwrap();
        cap.max_amount_in += U256::from(1_u8);
        assert!(validate_stonks_v3_quote_replay(&cap).is_err());
    }

    #[tokio::test]
    async fn rejects_policy_rounding_position_orientation_and_dependency_tamper() {
        let observed = observation().await;
        assert!(matches!(
            quote_stonks_v3_observation_with_policy(
                &observed,
                StonksV3QuotePolicy {
                    slippage_bps: 10_000,
                    ..policy()
                }
            ),
            Err(StonksV3QuoteError::UnsafePolicy)
        ));
        let mut quote = quote_stonks_v3_observation(&observed).unwrap();
        quote.entry.min_receive += U256::from(1_u8);
        assert!(validate_stonks_v3_quote_replay(&quote).is_err());
        let mut quote = quote_stonks_v3_observation(&observed).unwrap();
        quote.market.positions[4].liquidity += U256::from(1_u8);
        assert!(validate_stonks_v3_quote_replay(&quote).is_err());
        let mut quote = quote_stonks_v3_observation(&observed).unwrap();
        quote.market.token = WETH;
        assert!(validate_stonks_v3_quote_replay(&quote).is_err());
        let mut quote = quote_stonks_v3_observation(&observed).unwrap();
        quote.market.dependency_pins.runtime_pins[0].runtime_hash = B256::ZERO;
        assert!(validate_stonks_v3_quote_replay(&quote).is_err());
        let mut quote = quote_stonks_v3_observation(&observed).unwrap();
        quote.execution_blocker.clear();
        assert!(validate_stonks_v3_quote_replay(&quote).is_err());
        let mut quote = quote_stonks_v3_observation(&observed).unwrap();
        quote.market.leader = Address::ZERO;
        assert!(validate_stonks_v3_quote_replay(&quote).is_err());
        let mut quote = quote_stonks_v3_observation(&observed).unwrap();
        quote.market.creator = Address::ZERO;
        assert!(validate_stonks_v3_quote_replay(&quote).is_err());
        let mut quote = quote_stonks_v3_observation(&observed).unwrap();
        quote.market.mirror = Address::ZERO;
        assert!(validate_stonks_v3_quote_replay(&quote).is_err());
        let mut quote = quote_stonks_v3_observation(&observed).unwrap();
        quote.observation_proof_keccak256 = B256::ZERO;
        assert!(validate_stonks_v3_quote_replay(&quote).is_err());
    }

    #[tokio::test]
    async fn rejects_unordered_positions_fee_and_tick_crossing_tamper() {
        let observed = observation().await;
        let mut quote = quote_stonks_v3_observation(&observed).unwrap();
        quote.market.positions.swap(2, 3);
        assert!(validate_stonks_v3_quote_replay(&quote).is_err());
        let mut quote = quote_stonks_v3_observation(&observed).unwrap();
        quote.entry.pool_fee_ppm -= 1;
        assert!(validate_stonks_v3_quote_replay(&quote).is_err());
        let mut quote = quote_stonks_v3_observation(&observed).unwrap();
        quote.entry.state_after.initialized_ticks_crossed += 1;
        assert!(validate_stonks_v3_quote_replay(&quote).is_err());
    }

    #[tokio::test]
    async fn receipt_end_math_matches_first_real_post_launch_swap_exactly() {
        let evidence: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/stonks-v3-first-swap-differential.json"
        ))
        .unwrap();
        let observed = observation().await;
        assert_eq!(
            evidence["launch_tx_hash"].as_str().unwrap(),
            format!("{:#x}", observed.tx_hash)
        );
        assert_eq!(
            evidence["pool"].as_str().unwrap(),
            format!("{:#x}", observed.pool)
        );
        let first_swap = &evidence["first_swap"];
        let transaction_hash: B256 = first_swap["transaction_hash"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let block_hash: B256 = first_swap["block_hash"].as_str().unwrap().parse().unwrap();
        assert_eq!(
            transaction_hash,
            alloy_primitives::b256!(
                "2865fdf3440838b22832d5d07701b7e66a391c28f82ae1e20bd272971982206e"
            )
        );
        assert_eq!(
            block_hash,
            alloy_primitives::b256!(
                "5687ad644816a9213b4a182d54457d71c5cdfa5c3010f8275d0f7c21d4cf9814"
            )
        );
        assert_eq!(first_swap["l2_block_number"].as_u64(), Some(12_038_063));
        assert_eq!(first_swap["transaction_index"].as_u64(), Some(3));
        assert_eq!(first_swap["log_index"].as_u64(), Some(30));
        let log = crate::noxa_abi::ReceiptLog {
            address: evidence["pool"].as_str().unwrap().parse().unwrap(),
            log_index: first_swap["log_index"].as_u64().unwrap(),
            topics: first_swap["topics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|topic| topic.as_str().unwrap().parse().unwrap())
                .collect(),
            data: alloy_primitives::Bytes::from(
                hex::decode(
                    first_swap["raw_data"]
                        .as_str()
                        .unwrap()
                        .trim_start_matches("0x"),
                )
                .unwrap(),
            ),
        };
        assert_eq!(log.address, observed.pool);
        let crate::noxa_abi::V3PoolEvent::Swap {
            sender,
            recipient,
            amount0,
            amount1,
            sqrt_price_x96,
            liquidity,
            tick,
        } = crate::noxa_abi::decode_v3_pool_event(&log).unwrap()
        else {
            panic!("first Stonks pool event is not an exact V3 Swap");
        };
        assert_eq!(
            sender,
            first_swap["sender"]
                .as_str()
                .unwrap()
                .parse::<Address>()
                .unwrap()
        );
        assert_eq!(
            recipient,
            first_swap["recipient"]
                .as_str()
                .unwrap()
                .parse::<Address>()
                .unwrap()
        );
        assert_eq!(amount0.to_string(), first_swap["amount0"].as_str().unwrap());
        assert_eq!(amount1.to_string(), first_swap["amount1"].as_str().unwrap());
        let amount_in = amount1.into_raw();
        let quote = quote_stonks_v3_observation_with_policy(
            &observed,
            StonksV3QuotePolicy {
                amount_in,
                max_amount_in: U256::from(10_000_000_000_000_000_u64),
                slippage_bps: 100,
            },
        )
        .unwrap();
        let expected_output = amount0.checked_neg().unwrap().into_raw();
        assert_eq!(quote.entry.expected_output, expected_output);
        assert_eq!(quote.entry.state_after.sqrt_price_x96_after, sqrt_price_x96);
        assert_eq!(quote.entry.state_after.liquidity_after, liquidity);
        assert_eq!(quote.entry.state_after.tick_after, tick);
        assert_eq!(
            format!("{sqrt_price_x96:#x}"),
            first_swap["sqrt_price_x96_after"].as_str().unwrap()
        );
        assert_eq!(
            format!("0x{liquidity:x}"),
            first_swap["liquidity_after"].as_str().unwrap()
        );
        assert_eq!(i64::from(tick), first_swap["tick_after"].as_i64().unwrap());
    }
}
