//! Read-only, receipt-end paper quotes for launchpads that create canonical
//! Uniswap V3 liquidity in the launch transaction.
//!
//! This module performs no I/O. Callers fetch a confirmed receipt off the
//! candidate path, then pass it here for exact emitter, event, pool, and state
//! reconstruction before any quote is admitted.

use alloy_primitives::{Address, B256, I256, U256};
use alloy_sol_types::SolEvent;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::RobinhoodTransaction;
use crate::launchpad_adapter::LaunchpadId;
use crate::noxa_abi::{ReceiptLog, V3PoolEvent, decode_pool_created, decode_v3_pool_event};
use crate::noxa_predict::predict_v3_pool_address;
use crate::noxa_rpc::NoxaReceipt;
use crate::robinhood::{
    BOW_LAUNCH_FACTORY, CHAIN_ID, LAUNCHHOOD_V3_FACTORY, NOXA_RESTRICTION_L1_BLOCKS,
    UNISWAP_V3_FACTORY, UNISWAP_V3_POOL_INIT_CODE_KECCAK256, UNISWAP_V3_SWAP_ROUTER_02, WETH,
};
use crate::v3_pool::{V3PoolError, V3PoolState, V3Quote};

const V3_FEE: u32 = 10_000;
const V3_TICK_SPACING: i32 = 200;
const BPS_DENOMINATOR: u16 = 10_000;

mod bow_event {
    use alloy_sol_types::sol;

    sol! {
        event Launched(
            address indexed token,
            address indexed creator,
            address pool,
            uint256 positionId,
            uint256 launchDelay
        );
    }
}

mod launchhood_event {
    use alloy_sol_types::sol;

    sol! {
        event TokenLaunched(
            address indexed token,
            address indexed deployer,
            address indexed pool,
            address pairToken,
            uint256 configId,
            uint256 dexId,
            uint256 positionId,
            uint256 restrictionsEndBlock,
            uint256 initialBuyAmount,
            uint256 initialBuyTokens
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct V3ReceiptQuotePolicy {
    pub amount_in: U256,
    pub max_amount_in: U256,
    pub slippage_bps: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct V3ReceiptPositionEvidence {
    pub tick_lower: i32,
    pub tick_upper: i32,
    #[serde(
        serialize_with = "serialize_u128_hex",
        deserialize_with = "deserialize_u128_hex"
    )]
    pub liquidity: u128,
    pub log_index: u64,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct V3ReceiptMarketEvidence {
    pub token: Address,
    pub pool: Address,
    pub quote_asset: Address,
    pub fee: u32,
    pub tick_spacing: i32,
    pub launch_log_index: u64,
    pub pool_created_log_index: u64,
    pub initialize_log_index: u64,
    pub last_state_log_index: u64,
    pub mint_count: usize,
    pub swap_count: usize,
    pub restriction_end_block: Option<U256>,
    /// Exact receipt-created liquidity positions needed to replay a serialized
    /// quote without trusting its claimed outputs.
    #[serde(default)]
    pub positions: Vec<V3ReceiptPositionEvidence>,
    #[serde(default)]
    pub receipt_end_sqrt_price_x96: U256,
    #[serde(default)]
    pub receipt_end_tick: i32,
    #[serde(
        default,
        serialize_with = "serialize_u128_hex",
        deserialize_with = "deserialize_u128_hex"
    )]
    pub receipt_end_liquidity: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct V3QuoteStateVersion {
    pub chain_id: u64,
    pub block_hash: B256,
    pub l2_block_number: u64,
    pub transaction_index: u64,
    pub terminal_log_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct V3PaperSwapQuote {
    pub amount_in: U256,
    pub expected_output: U256,
    pub min_receive: U256,
    pub slippage_bps: u16,
    pub state_after: V3Quote,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct V3ReceiptPaperQuote {
    pub record_type: String,
    pub tx_hash: B256,
    pub launchpad: LaunchpadId,
    pub l2_block_number: u64,
    pub state_version: V3QuoteStateVersion,
    pub quote_source: String,
    pub sizing_source: String,
    pub market: V3ReceiptMarketEvidence,
    pub entry: V3PaperSwapQuote,
    pub full_position_exit: V3PaperSwapQuote,
    pub simulated_round_trip_return_bps: U256,
    pub execution_eligible: bool,
    pub execution_blocker: String,
    pub broadcast: bool,
}

#[derive(Debug, Error)]
pub enum V3ReceiptQuoteError {
    #[error("only Bow and LaunchHood V3 receipts are supported")]
    UnsupportedLaunchpad,
    #[error("receipt failed or has a zero transaction hash")]
    FailedReceipt,
    #[error("transaction and receipt envelope do not match the launch profile")]
    TransactionEnvelope,
    #[error("quote policy amount or slippage is unsafe")]
    UnsafePolicy,
    #[error("receipt logs are not in strict log-index order")]
    UnorderedLogs,
    #[error("receipt must contain exactly one exact launch event")]
    LaunchEventIdentity,
    #[error("launch event contains an invalid market identity")]
    LaunchMarketIdentity,
    #[error("receipt must contain exactly one canonical V3 PoolCreated event")]
    PoolCreatedIdentity,
    #[error("receipt pool address is not the canonical V3 CREATE2 address")]
    NonCanonicalPool,
    #[error("pool state event order is incomplete or unsupported")]
    InvalidStateSequence,
    #[error("receipt contains an unsupported V3 Burn event")]
    BurnUnsupported,
    #[error("receipt contains an unknown event emitted by the launch pool")]
    UnknownPoolEvent,
    #[error("embedded launch swap does not match the independently reconstructed V3 result")]
    EmbeddedSwapMismatch,
    #[error("launch restriction evidence is inconsistent with the receipt L1 block")]
    RestrictionEvidence,
    #[error("local quote did not consume all input or returned zero output")]
    IncompleteQuote,
    #[error("serialized V3 quote cannot be independently replayed")]
    QuoteReplayMismatch,
    #[error(transparent)]
    Pool(#[from] V3PoolError),
}

#[derive(Debug, Clone, Copy)]
struct LaunchIdentity {
    token: Address,
    deployer: Address,
    pool: Address,
    log_index: u64,
    restriction_end_block: Option<U256>,
    initial_buy_amount: U256,
    /// LaunchHood declares this in its launch event. Bow does not, so Bow's
    /// output must be derived from the pool delta and independent V3 replay.
    initial_buy_tokens: Option<U256>,
}

/// Reconstruct the exact pool state at the end of a confirmed launch receipt,
/// then quote a fixed-policy WETH entry and an immediate full-position exit.
/// Token restriction semantics remain a separate gate, so this evidence never
/// claims execution eligibility.
pub fn quote_v3_launch_receipt(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    launchpad: LaunchpadId,
    policy: V3ReceiptQuotePolicy,
) -> Result<V3ReceiptPaperQuote, V3ReceiptQuoteError> {
    validate_receipt_and_policy(transaction, receipt, launchpad, policy)?;
    let mut launch = exact_launch_identity(launchpad, &receipt.logs)?;
    let expected_factory = match launchpad {
        LaunchpadId::Bow => BOW_LAUNCH_FACTORY,
        LaunchpadId::LaunchHoodV3 => LAUNCHHOOD_V3_FACTORY,
        _ => return Err(V3ReceiptQuoteError::UnsupportedLaunchpad),
    };
    if transaction.to != Some(expected_factory) || transaction.from != launch.deployer {
        return Err(V3ReceiptQuoteError::TransactionEnvelope);
    }
    match launchpad {
        LaunchpadId::Bow => {
            launch.initial_buy_amount = transaction.value;
        }
        LaunchpadId::LaunchHoodV3 => {
            if transaction.value != launch.initial_buy_amount {
                return Err(V3ReceiptQuoteError::EmbeddedSwapMismatch);
            }
            let l1_block = receipt
                .l1_block_number
                .ok_or(V3ReceiptQuoteError::RestrictionEvidence)?;
            let expected_end = U256::from(
                l1_block
                    .checked_add(NOXA_RESTRICTION_L1_BLOCKS)
                    .ok_or(V3ReceiptQuoteError::RestrictionEvidence)?,
            );
            if launch.restriction_end_block != Some(expected_end) {
                return Err(V3ReceiptQuoteError::RestrictionEvidence);
            }
        }
        _ => {}
    }
    let (token0, token1) = sorted_pair(launch.token, WETH)?;

    let pool_created = receipt
        .logs
        .iter()
        .filter(|log| log.address == UNISWAP_V3_FACTORY)
        .filter_map(|log| decode_pool_created(log).map(|event| (log.log_index, event)))
        .collect::<Vec<_>>();
    if pool_created.len() != 1 {
        return Err(V3ReceiptQuoteError::PoolCreatedIdentity);
    }
    let (pool_created_log_index, created) = &pool_created[0];
    if created.token0 != token0
        || created.token1 != token1
        || created.fee != V3_FEE
        || created.tick_spacing != V3_TICK_SPACING
        || created.pool != launch.pool
        || *pool_created_log_index >= launch.log_index
    {
        return Err(V3ReceiptQuoteError::PoolCreatedIdentity);
    }
    let canonical_pool = predict_v3_pool_address(
        UNISWAP_V3_FACTORY,
        token0,
        token1,
        V3_FEE,
        UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    );
    if launch.pool != canonical_pool {
        return Err(V3ReceiptQuoteError::NonCanonicalPool);
    }

    let mut state = None;
    let mut initialize_log_index = None;
    let mut last_state_log_index = None;
    let mut mint_count = 0_usize;
    let mut swap_count = 0_usize;
    let mut positions = Vec::new();
    for log in receipt.logs.iter().filter(|log| log.address == launch.pool) {
        let event = decode_v3_pool_event(log).ok_or(V3ReceiptQuoteError::UnknownPoolEvent)?;
        if log.log_index <= *pool_created_log_index {
            return Err(V3ReceiptQuoteError::InvalidStateSequence);
        }
        match event {
            V3PoolEvent::Initialize {
                sqrt_price_x96,
                tick,
            } => {
                if state.is_some() || log.log_index >= launch.log_index {
                    return Err(V3ReceiptQuoteError::InvalidStateSequence);
                }
                state = Some(V3PoolState::new(
                    launch.pool,
                    token0,
                    token1,
                    V3_FEE,
                    V3_TICK_SPACING,
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
                if log.log_index >= launch.log_index {
                    return Err(V3ReceiptQuoteError::InvalidStateSequence);
                }
                let pool = state
                    .as_mut()
                    .ok_or(V3ReceiptQuoteError::InvalidStateSequence)?;
                pool.add_position(tick_lower, tick_upper, amount)?;
                positions.push(V3ReceiptPositionEvidence {
                    tick_lower,
                    tick_upper,
                    liquidity: amount,
                    log_index: log.log_index,
                });
                mint_count += 1;
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
                let valid_side_of_launch = match launchpad {
                    LaunchpadId::Bow => log.log_index > launch.log_index,
                    LaunchpadId::LaunchHoodV3 => log.log_index < launch.log_index,
                    _ => false,
                };
                if mint_count == 0 || !valid_side_of_launch || swap_count != 0 {
                    return Err(V3ReceiptQuoteError::InvalidStateSequence);
                }
                let pool = state
                    .as_mut()
                    .ok_or(V3ReceiptQuoteError::InvalidStateSequence)?;
                validate_embedded_swap(
                    pool,
                    launch,
                    sender,
                    recipient,
                    amount0,
                    amount1,
                    sqrt_price_x96,
                    liquidity,
                    tick,
                )?;
                pool.set_observation(sqrt_price_x96, tick, liquidity)?;
                swap_count += 1;
            }
            V3PoolEvent::Burn { .. } => return Err(V3ReceiptQuoteError::BurnUnsupported),
        }
        last_state_log_index = Some(log.log_index);
    }
    let state = state.ok_or(V3ReceiptQuoteError::InvalidStateSequence)?;
    if mint_count == 0 {
        return Err(V3ReceiptQuoteError::InvalidStateSequence);
    }
    if (launch.initial_buy_amount == U256::ZERO && swap_count != 0)
        || (launch.initial_buy_amount != U256::ZERO && swap_count != 1)
    {
        return Err(V3ReceiptQuoteError::EmbeddedSwapMismatch);
    }
    let entry_state = state.quote_exact_input(WETH, policy.amount_in, None)?;
    validate_complete_quote(&entry_state)?;
    let entry_min = apply_slippage(entry_state.amount_out, policy.slippage_bps)?;

    let mut post_entry = state.clone();
    post_entry.set_observation(
        entry_state.sqrt_price_x96_after,
        entry_state.tick_after,
        entry_state.liquidity_after,
    )?;
    let exit_state = post_entry.quote_exact_input(launch.token, entry_state.amount_out, None)?;
    validate_complete_quote(&exit_state)?;
    let exit_min = apply_slippage(exit_state.amount_out, policy.slippage_bps)?;
    let round_trip_return_bps = exit_state
        .amount_out
        .checked_mul(U256::from(BPS_DENOMINATOR))
        .ok_or(V3ReceiptQuoteError::UnsafePolicy)?
        / policy.amount_in;

    Ok(V3ReceiptPaperQuote {
        record_type: "launchpad_v3_paper_quote".into(),
        tx_hash: receipt.transaction_hash,
        launchpad,
        l2_block_number: receipt.l2_block_number,
        state_version: V3QuoteStateVersion {
            chain_id: CHAIN_ID,
            block_hash: receipt.block_hash,
            l2_block_number: receipt.l2_block_number,
            transaction_index: receipt.transaction_index,
            terminal_log_index: launch
                .log_index
                .max(last_state_log_index.ok_or(V3ReceiptQuoteError::InvalidStateSequence)?),
        },
        quote_source: "confirmed_receipt_end_v3_state".into(),
        sizing_source: "independent_fixed_tiny_weth_policy".into(),
        market: V3ReceiptMarketEvidence {
            token: launch.token,
            pool: launch.pool,
            quote_asset: WETH,
            fee: V3_FEE,
            tick_spacing: V3_TICK_SPACING,
            launch_log_index: launch.log_index,
            pool_created_log_index: *pool_created_log_index,
            initialize_log_index: initialize_log_index
                .ok_or(V3ReceiptQuoteError::InvalidStateSequence)?,
            last_state_log_index: last_state_log_index
                .ok_or(V3ReceiptQuoteError::InvalidStateSequence)?,
            mint_count,
            swap_count,
            restriction_end_block: launch.restriction_end_block,
            positions,
            receipt_end_sqrt_price_x96: state.sqrt_price_x96,
            receipt_end_tick: state.tick,
            receipt_end_liquidity: state.liquidity,
        },
        entry: V3PaperSwapQuote {
            amount_in: policy.amount_in,
            expected_output: entry_state.amount_out,
            min_receive: entry_min,
            slippage_bps: policy.slippage_bps,
            state_after: entry_state,
        },
        full_position_exit: V3PaperSwapQuote {
            amount_in: exit_state.amount_in_requested,
            expected_output: exit_state.amount_out,
            min_receive: exit_min,
            slippage_bps: policy.slippage_bps,
            state_after: exit_state,
        },
        simulated_round_trip_return_bps: round_trip_return_bps,
        execution_eligible: false,
        execution_blocker: "paper_only_token_restriction_and_runtime_checks_not_satisfied".into(),
        broadcast: false,
    })
}

/// Rebuild both legs from the serialized receipt-end pool state. This is
/// intentionally separate from receipt collection so finalization does not
/// trust claimed outputs merely because their transaction identity matches.
pub fn validate_v3_quote_replay(
    quote: &V3ReceiptPaperQuote,
    policy: V3ReceiptQuotePolicy,
) -> Result<(), V3ReceiptQuoteError> {
    let market = &quote.market;
    if !matches!(
        quote.launchpad,
        LaunchpadId::Bow | LaunchpadId::LaunchHoodV3
    ) || quote.record_type != "launchpad_v3_paper_quote"
        || quote.quote_source != "confirmed_receipt_end_v3_state"
        || quote.sizing_source != "independent_fixed_tiny_weth_policy"
        || quote.execution_eligible
        || quote.broadcast
        || market.token == Address::ZERO
        || market.token == WETH
        || market.pool == Address::ZERO
        || market.quote_asset != WETH
        || market.fee != V3_FEE
        || market.tick_spacing != V3_TICK_SPACING
        || market.mint_count == 0
        || market.mint_count != market.positions.len()
        || market.positions.is_empty()
        || market.receipt_end_sqrt_price_x96 == U256::ZERO
        || policy.amount_in == U256::ZERO
        || policy.max_amount_in == U256::ZERO
        || policy.amount_in > policy.max_amount_in
        || policy.slippage_bps >= BPS_DENOMINATOR
        || quote.entry.amount_in != policy.amount_in
        || quote.entry.slippage_bps != policy.slippage_bps
        || quote.full_position_exit.slippage_bps != policy.slippage_bps
    {
        return Err(V3ReceiptQuoteError::QuoteReplayMismatch);
    }
    match quote.launchpad {
        LaunchpadId::Bow if market.restriction_end_block.is_some() => {
            return Err(V3ReceiptQuoteError::QuoteReplayMismatch);
        }
        LaunchpadId::LaunchHoodV3
            if market
                .restriction_end_block
                .is_none_or(|block| block == U256::ZERO) =>
        {
            return Err(V3ReceiptQuoteError::QuoteReplayMismatch);
        }
        _ => {}
    }
    let (token0, token1) = sorted_pair(market.token, WETH)?;
    if predict_v3_pool_address(
        UNISWAP_V3_FACTORY,
        token0,
        token1,
        V3_FEE,
        UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    ) != market.pool
    {
        return Err(V3ReceiptQuoteError::QuoteReplayMismatch);
    }
    let mut state = V3PoolState::new(
        market.pool,
        token0,
        token1,
        market.fee,
        market.tick_spacing,
        market.receipt_end_sqrt_price_x96,
        market.receipt_end_tick,
        0,
    )?;
    let mut previous_log_index = None;
    for position in &market.positions {
        if position.log_index <= market.initialize_log_index
            || position.log_index >= market.launch_log_index
            || previous_log_index.is_some_and(|previous| position.log_index <= previous)
        {
            return Err(V3ReceiptQuoteError::QuoteReplayMismatch);
        }
        state.add_position(position.tick_lower, position.tick_upper, position.liquidity)?;
        previous_log_index = Some(position.log_index);
    }
    if state.liquidity != market.receipt_end_liquidity {
        return Err(V3ReceiptQuoteError::QuoteReplayMismatch);
    }
    let entry = state.quote_exact_input(WETH, policy.amount_in, None)?;
    validate_complete_quote(&entry)?;
    if quote.entry.expected_output != entry.amount_out
        || quote.entry.min_receive != apply_slippage(entry.amount_out, policy.slippage_bps)?
        || quote.entry.state_after != entry
    {
        return Err(V3ReceiptQuoteError::QuoteReplayMismatch);
    }
    state.set_observation(
        entry.sqrt_price_x96_after,
        entry.tick_after,
        entry.liquidity_after,
    )?;
    let exit = state.quote_exact_input(market.token, entry.amount_out, None)?;
    validate_complete_quote(&exit)?;
    let expected_round_trip = exit
        .amount_out
        .checked_mul(U256::from(BPS_DENOMINATOR))
        .ok_or(V3ReceiptQuoteError::QuoteReplayMismatch)?
        / policy.amount_in;
    if quote.full_position_exit.amount_in != entry.amount_out
        || quote.full_position_exit.expected_output != exit.amount_out
        || quote.full_position_exit.min_receive
            != apply_slippage(exit.amount_out, policy.slippage_bps)?
        || quote.full_position_exit.state_after != exit
        || quote.simulated_round_trip_return_bps != expected_round_trip
    {
        return Err(V3ReceiptQuoteError::QuoteReplayMismatch);
    }
    Ok(())
}

fn validate_receipt_and_policy(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    launchpad: LaunchpadId,
    policy: V3ReceiptQuotePolicy,
) -> Result<(), V3ReceiptQuoteError> {
    if !matches!(launchpad, LaunchpadId::Bow | LaunchpadId::LaunchHoodV3) {
        return Err(V3ReceiptQuoteError::UnsupportedLaunchpad);
    }
    if !receipt.status || receipt.transaction_hash == B256::ZERO || receipt.block_hash == B256::ZERO
    {
        return Err(V3ReceiptQuoteError::FailedReceipt);
    }
    if transaction.hash != receipt.transaction_hash
        || transaction.l2_block_number != Some(receipt.l2_block_number)
        || transaction.transaction_index != Some(receipt.transaction_index)
    {
        return Err(V3ReceiptQuoteError::TransactionEnvelope);
    }
    if policy.amount_in == U256::ZERO
        || policy.max_amount_in == U256::ZERO
        || policy.amount_in > policy.max_amount_in
        || policy.slippage_bps >= BPS_DENOMINATOR
    {
        return Err(V3ReceiptQuoteError::UnsafePolicy);
    }
    if receipt
        .logs
        .windows(2)
        .any(|pair| pair[0].log_index >= pair[1].log_index)
    {
        return Err(V3ReceiptQuoteError::UnorderedLogs);
    }
    Ok(())
}

fn exact_launch_identity(
    launchpad: LaunchpadId,
    logs: &[ReceiptLog],
) -> Result<LaunchIdentity, V3ReceiptQuoteError> {
    let identities = logs
        .iter()
        .filter_map(|log| decode_launch_identity(launchpad, log))
        .collect::<Vec<_>>();
    if identities.len() != 1 {
        return Err(V3ReceiptQuoteError::LaunchEventIdentity);
    }
    let identity = identities[0];
    if identity.token == Address::ZERO || identity.token == WETH || identity.pool == Address::ZERO {
        return Err(V3ReceiptQuoteError::LaunchMarketIdentity);
    }
    Ok(identity)
}

fn decode_launch_identity(launchpad: LaunchpadId, log: &ReceiptLog) -> Option<LaunchIdentity> {
    match launchpad {
        LaunchpadId::Bow if log.address == BOW_LAUNCH_FACTORY => {
            let event =
                bow_event::Launched::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                    .ok()?;
            Some(LaunchIdentity {
                token: event.token,
                deployer: event.creator,
                pool: event.pool,
                log_index: log.log_index,
                restriction_end_block: None,
                initial_buy_amount: U256::ZERO,
                initial_buy_tokens: None,
            })
        }
        LaunchpadId::LaunchHoodV3 if log.address == LAUNCHHOOD_V3_FACTORY => {
            let event = launchhood_event::TokenLaunched::decode_raw_log_validate(
                log.topics.iter().copied(),
                &log.data,
            )
            .ok()?;
            if event.pairToken != WETH || event.dexId != U256::ZERO || event.configId != U256::ZERO
            {
                return None;
            }
            Some(LaunchIdentity {
                token: event.token,
                deployer: event.deployer,
                pool: event.pool,
                log_index: log.log_index,
                restriction_end_block: Some(event.restrictionsEndBlock),
                initial_buy_amount: event.initialBuyAmount,
                initial_buy_tokens: Some(event.initialBuyTokens),
            })
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_embedded_swap(
    pool: &V3PoolState,
    launch: LaunchIdentity,
    sender: Address,
    recipient: Address,
    amount0: I256,
    amount1: I256,
    sqrt_price_x96: U256,
    liquidity: u128,
    tick: i32,
) -> Result<(), V3ReceiptQuoteError> {
    if launch.initial_buy_amount == U256::ZERO
        || sender != UNISWAP_V3_SWAP_ROUTER_02
        || recipient != launch.deployer
    {
        return Err(V3ReceiptQuoteError::EmbeddedSwapMismatch);
    }
    let (weth_delta, token_delta) = if pool.token0 == WETH {
        (amount0, amount1)
    } else {
        (amount1, amount0)
    };
    if weth_delta.is_negative()
        || weth_delta.into_raw() != launch.initial_buy_amount
        || !token_delta.is_negative()
        || token_delta.unsigned_abs() == U256::ZERO
    {
        return Err(V3ReceiptQuoteError::EmbeddedSwapMismatch);
    }
    let reconstructed = pool.quote_exact_input(WETH, launch.initial_buy_amount, None)?;
    let actual_token_output = token_delta.unsigned_abs();
    if reconstructed.amount_in_consumed != launch.initial_buy_amount
        || reconstructed.amount_out != actual_token_output
        || launch
            .initial_buy_tokens
            .is_some_and(|declared| declared != actual_token_output)
        || reconstructed.sqrt_price_x96_after != sqrt_price_x96
        || reconstructed.tick_after != tick
        || reconstructed.liquidity_after != liquidity
    {
        return Err(V3ReceiptQuoteError::EmbeddedSwapMismatch);
    }
    Ok(())
}

fn sorted_pair(token: Address, quote: Address) -> Result<(Address, Address), V3ReceiptQuoteError> {
    if token == Address::ZERO || quote == Address::ZERO || token == quote {
        return Err(V3ReceiptQuoteError::LaunchMarketIdentity);
    }
    Ok(if token < quote {
        (token, quote)
    } else {
        (quote, token)
    })
}

fn apply_slippage(amount: U256, slippage_bps: u16) -> Result<U256, V3ReceiptQuoteError> {
    let numerator = U256::from(BPS_DENOMINATOR - slippage_bps);
    let minimum = amount
        .checked_mul(numerator)
        .ok_or(V3ReceiptQuoteError::UnsafePolicy)?
        / U256::from(BPS_DENOMINATOR);
    if minimum == U256::ZERO || minimum > amount {
        return Err(V3ReceiptQuoteError::IncompleteQuote);
    }
    Ok(minimum)
}

fn validate_complete_quote(quote: &V3Quote) -> Result<(), V3ReceiptQuoteError> {
    if quote.amount_in_consumed != quote.amount_in_requested || quote.amount_out == U256::ZERO {
        return Err(V3ReceiptQuoteError::IncompleteQuote);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy_primitives::Bytes;

    use super::*;

    const BOW_TX: &str = "1adcd30a5de19423f56b93d91df33d950179ed7ef4f9d4aae31fca13f72fc009";
    const LAUNCHHOOD_TX: &str = "0ecc94840fc67b9f2c04e0e5b72f5a7e18d0dc6c816e86463e4e907b0700e978";

    fn policy() -> V3ReceiptQuotePolicy {
        V3ReceiptQuotePolicy {
            amount_in: U256::from(1_000_000_000_000_000_u64),
            max_amount_in: U256::from(10_000_000_000_000_000_u64),
            slippage_bps: 100,
        }
    }

    fn parse_b256(value: &str) -> B256 {
        B256::from_str(value).unwrap()
    }

    fn parse_address(value: &str) -> Address {
        Address::from_str(value).unwrap()
    }

    fn live_log(address: &str, log_index: u64, topics: &[&str], data: &str) -> ReceiptLog {
        ReceiptLog {
            address: parse_address(address),
            log_index,
            topics: topics.iter().map(|topic| parse_b256(topic)).collect(),
            data: Bytes::from(hex::decode(data).unwrap()),
        }
    }

    fn bow_fixture() -> (RobinhoodTransaction, NoxaReceipt) {
        let tx_hash = parse_b256(BOW_TX);
        let logs = vec![
            live_log(
                "1f7d7550b1b028f7571e69a784071f0205fd2efa",
                1,
                &[
                    "783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118",
                    "00000000000000000000000000488257d5942b60119dc8c23dfe1c613c061b03",
                    "0000000000000000000000000bd7d308f8e1639fab988df18a8011f41eacad73",
                    "0000000000000000000000000000000000000000000000000000000000002710",
                ],
                "00000000000000000000000000000000000000000000000000000000000000c8000000000000000000000000d4759258987f7be17ae5afc7151da10bf54b2192",
            ),
            live_log(
                "d4759258987f7be17ae5afc7151da10bf54b2192",
                2,
                &["98636036cb66a9c19a37435efc1e90142190214e8abeb821bdba3f2990dd4c95"],
                "0000000000000000000000000000000000000000000289c75e384277ff7a6484fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffce64b",
            ),
            live_log(
                "d4759258987f7be17ae5afc7151da10bf54b2192",
                6,
                &[
                    "7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde",
                    "00000000000000000000000073991a25c818bf1f1128deaab1492d45638de0d3",
                    "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffce7d0",
                    "00000000000000000000000000000000000000000000000000000000000d89a0",
                ],
                "00000000000000000000000073991a25c818bf1f1128deaab1492d45638de0d300000000000000000000000000000000000000000000085cb16d31e60a6c05e20000000000000000000000000000000000000000033b2e3c9fd0803ce7ffc25c0000000000000000000000000000000000000000000000000000000000000000",
            ),
            live_log(
                "c70e510e14710ea535cab7b2414860af63feab79",
                12,
                &[
                    "ec774f0683e9ac48e8d835f412f9f877a8a5dee9af3170d78cf3ef33149d15e7",
                    "00000000000000000000000000488257d5942b60119dc8c23dfe1c613c061b03",
                    "000000000000000000000000660591c04dd40ac2d6604ecc2951e155fbd914b7",
                ],
                "000000000000000000000000d4759258987f7be17ae5afc7151da10bf54b2192000000000000000000000000000000000000000000000000000000000002a4c40000000000000000000000000000000000000000000000000000000000000462",
            ),
        ];
        let transaction = RobinhoodTransaction {
            hash: tx_hash,
            from: parse_address("660591c04dd40ac2d6604ecc2951e155fbd914b7"),
            to: Some(BOW_LAUNCH_FACTORY),
            input: Bytes::new(),
            value: U256::ZERO,
            l2_block_number: Some(11_463_668),
            transaction_index: Some(1),
        };
        let receipt = NoxaReceipt {
            transaction_hash: tx_hash,
            block_hash: parse_b256(
                "c15c854c65b16eae04478c619eaf930f3dfd897ce9e9e85b4cfb9448d82962cd",
            ),
            status: true,
            l2_block_number: 11_463_668,
            l1_block_number: Some(0x185d0bf),
            transaction_index: 1,
            gas_used: Some(0x6d680a),
            effective_gas_price: None,
            logs,
        };
        (transaction, receipt)
    }

    fn bow_payable_fixture() -> (RobinhoodTransaction, NoxaReceipt) {
        let tx_hash =
            parse_b256("f842590f4b6abcda2e838397ceca82f07566d4256137a2ea88549cf331d1ab6b");
        let logs = vec![
            live_log(
                "1f7d7550b1b028f7571e69a784071f0205fd2efa",
                18,
                &[
                    "783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118",
                    "0000000000000000000000000bd7d308f8e1639fab988df18a8011f41eacad73",
                    "000000000000000000000000cede14a428b954333ba0e9a6df68d0e6fd786b03",
                    "0000000000000000000000000000000000000000000000000000000000002710",
                ],
                "00000000000000000000000000000000000000000000000000000000000000c8000000000000000000000000effe014849fb7056fd5aedd923e6dc0777d850ad",
            ),
            live_log(
                "effe014849fb7056fd5aedd923e6dc0777d850ad",
                19,
                &["98636036cb66a9c19a37435efc1e90142190214e8abeb821bdba3f2990dd4c95"],
                "00000000000000000000000000000000000064dbe3946352a8ef28c04d549e7800000000000000000000000000000000000000000000000000000000000319b4",
            ),
            live_log(
                "effe014849fb7056fd5aedd923e6dc0777d850ad",
                23,
                &[
                    "7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde",
                    "00000000000000000000000073991a25c818bf1f1128deaab1492d45638de0d3",
                    "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffff27660",
                    "0000000000000000000000000000000000000000000000000000000000031830",
                ],
                "00000000000000000000000073991a25c818bf1f1128deaab1492d45638de0d300000000000000000000000000000000000000000000085cb16d31e60a6c05e200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000033b2e3c9fd0803ce7ffc283",
            ),
            live_log(
                "c70e510e14710ea535cab7b2414860af63feab79",
                29,
                &[
                    "ec774f0683e9ac48e8d835f412f9f877a8a5dee9af3170d78cf3ef33149d15e7",
                    "000000000000000000000000cede14a428b954333ba0e9a6df68d0e6fd786b03",
                    "000000000000000000000000d27664a94b801e912ef2051646f29ce76a8a3fb9",
                ],
                "000000000000000000000000effe014849fb7056fd5aedd923e6dc0777d850ad000000000000000000000000000000000000000000000000000000000002bf630000000000000000000000000000000000000000000000000000000000000483",
            ),
            live_log(
                "effe014849fb7056fd5aedd923e6dc0777d850ad",
                35,
                &[
                    "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67",
                    "000000000000000000000000caf681a66d020601342297493863e78c959e5cb2",
                    "000000000000000000000000d27664a94b801e912ef2051646f29ce76a8a3fb9",
                ],
                "00000000000000000000000000000000000000000000000000470de4df820000fffffffffffffffffffffffffffffffffffffffffff5a0eba652442d64c90a8000000000000000000000000000000000000061ae1c60f0281b10a4b8a9eadd3900000000000000000000000000000000000000000000085cb16d31e60a6c05e20000000000000000000000000000000000000000000000000000000000031733",
            ),
        ];
        let transaction = RobinhoodTransaction {
            hash: tx_hash,
            from: parse_address("d27664a94b801e912ef2051646f29ce76a8a3fb9"),
            to: Some(BOW_LAUNCH_FACTORY),
            input: Bytes::new(),
            value: U256::from(20_000_000_000_000_000_u64),
            l2_block_number: Some(11_624_477),
            transaction_index: Some(5),
        };
        let receipt = NoxaReceipt {
            transaction_hash: tx_hash,
            block_hash: parse_b256(
                "2b8a8fb0552a2434068a1c60631b2ed60988a13a666e6231ec12f0ceadfc731e",
            ),
            status: true,
            l2_block_number: 11_624_477,
            l1_block_number: None,
            transaction_index: 5,
            gas_used: None,
            effective_gas_price: None,
            logs,
        };
        (transaction, receipt)
    }

    fn launchhood_fixture() -> (RobinhoodTransaction, NoxaReceipt) {
        let tx_hash = parse_b256(LAUNCHHOOD_TX);
        let logs = vec![
            live_log(
                "1f7d7550b1b028f7571e69a784071f0205fd2efa",
                192,
                &[
                    "783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118",
                    "0000000000000000000000000bd7d308f8e1639fab988df18a8011f41eacad73",
                    "0000000000000000000000001b0da262e376bea7fd0356460e9627bfe2ed61c9",
                    "0000000000000000000000000000000000000000000000000000000000002710",
                ],
                "00000000000000000000000000000000000000000000000000000000000000c800000000000000000000000007f80fd806d381cd8b95cc2b4e46fb470eb4b4f6",
            ),
            live_log(
                "07f80fd806d381cd8b95cc2b4e46fb470eb4b4f6",
                193,
                &["98636036cb66a9c19a37435efc1e90142190214e8abeb821bdba3f2990dd4c95"],
                "0000000000000000000000000000000000006a17b32fc5d4d48f7124aa2fdba00000000000000000000000000000000000000000000000000000000000031da8",
            ),
            live_log(
                "07f80fd806d381cd8b95cc2b4e46fb470eb4b4f6",
                196,
                &[
                    "7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde",
                    "00000000000000000000000073991a25c818bf1f1128deaab1492d45638de0d3",
                    "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffff27660",
                    "0000000000000000000000000000000000000000000000000000000000031da8",
                ],
                "00000000000000000000000073991a25c818bf1f1128deaab1492d45638de0d30000000000000000000000000000000000000000000007cbf9d9985f0629c56e00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000033b2e3c9fd0803ce7ffd018",
            ),
            live_log(
                "07f80fd806d381cd8b95cc2b4e46fb470eb4b4f6",
                203,
                &[
                    "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67",
                    "000000000000000000000000caf681a66d020601342297493863e78c959e5cb2",
                    "0000000000000000000000002cab3d5933ed494f48c221361da75ada676f366a",
                ],
                "000000000000000000000000000000000000000000000000000009184e72a000fffffffffffffffffffffffffffffffffffffffffffffe741f078372a1c04a2f0000000000000000000000000000000000006a17806976ba9d7b565f9faa3d8f0000000000000000000000000000000000000000000007cbf9d9985f0629c56e0000000000000000000000000000000000000000000000000000000000031da7",
            ),
            live_log(
                "62b33a039d289cbda50ebeb72fe4261449e61bcf",
                204,
                &[
                    "235e34a4e0e6a401dae6851f6fab4a919a1fdd0ae0073ac2fc4d1d4a87e548e5",
                    "0000000000000000000000001b0da262e376bea7fd0356460e9627bfe2ed61c9",
                    "0000000000000000000000002cab3d5933ed494f48c221361da75ada676f366a",
                    "00000000000000000000000007f80fd806d381cd8b95cc2b4e46fb470eb4b4f6",
                ],
                "0000000000000000000000000bd7d308f8e1639fab988df18a8011f41eacad7300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002a660000000000000000000000000000000000000000000000000000000000185d295000000000000000000000000000000000000000000000000000009184e72a00000000000000000000000000000000000000000000000018be0f87c8d5e3fb5d1",
            ),
        ];
        let transaction = RobinhoodTransaction {
            hash: tx_hash,
            from: parse_address("2cab3d5933ed494f48c221361da75ada676f366a"),
            to: Some(LAUNCHHOOD_V3_FACTORY),
            input: Bytes::new(),
            value: U256::from(10_000_000_000_000_u64),
            l2_block_number: Some(11_476_109),
            transaction_index: Some(13),
        };
        let receipt = NoxaReceipt {
            transaction_hash: tx_hash,
            block_hash: parse_b256(
                "7fef5d66e48aef0d3986f6c942cf7a89c8bd2b8519d0ae4e1998527cb7aaf4ae",
            ),
            status: true,
            l2_block_number: 11_476_109,
            l1_block_number: Some(0x185d127),
            transaction_index: 13,
            gas_used: Some(0x5a5f1f),
            effective_gas_price: None,
            logs,
        };
        (transaction, receipt)
    }

    #[test]
    fn bow_live_proof_reconstructs_entry_and_full_exit() {
        let (transaction, receipt) = bow_fixture();
        let quote =
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::Bow, policy()).unwrap();
        assert_eq!(
            quote.entry.expected_output,
            U256::from_str_radix("865ab468f1ddbed061c0", 16).unwrap()
        );
        assert_eq!(quote.market.swap_count, 0);
        assert_eq!(quote.state_version.block_hash, receipt.block_hash);
        assert!(!quote.execution_eligible);
        assert!(!quote.broadcast);
    }

    #[test]
    fn bow_payable_live_proof_reconstructs_embedded_buy_before_quoting() {
        let (transaction, receipt) = bow_payable_fixture();
        let quote =
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::Bow, policy()).unwrap();
        assert_eq!(quote.market.swap_count, 1);
        assert_eq!(quote.market.last_state_log_index, 35);
        assert_eq!(quote.state_version.terminal_log_index, 35);
        assert!(quote.entry.expected_output > U256::ZERO);
        assert!(quote.full_position_exit.expected_output < quote.entry.amount_in);
        assert!(!quote.execution_eligible);
        assert!(!quote.broadcast);
    }

    #[test]
    fn launchhood_live_proof_reproduces_embedded_swap_before_quoting() {
        let (transaction, receipt) = launchhood_fixture();
        let quote =
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::LaunchHoodV3, policy())
                .unwrap();
        assert_eq!(
            quote.entry.expected_output,
            U256::from_str_radix("9a86b39e027db1a86297", 16).unwrap()
        );
        assert_eq!(quote.market.swap_count, 1);
        assert_eq!(
            quote.market.restriction_end_block,
            Some(U256::from(0x185d295_u64))
        );
        assert!(quote.full_position_exit.expected_output < quote.entry.amount_in);
        validate_v3_quote_replay(&quote, policy()).unwrap();
    }

    #[test]
    fn serialized_live_quote_replay_rejects_output_state_and_position_tampering() {
        let (transaction, receipt) = bow_payable_fixture();
        let quote =
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::Bow, policy()).unwrap();
        validate_v3_quote_replay(&quote, policy()).unwrap();

        let mut output = quote.clone();
        output.entry.expected_output += U256::from(1_u8);
        output.entry.state_after.amount_out = output.entry.expected_output;
        assert!(matches!(
            validate_v3_quote_replay(&output, policy()),
            Err(V3ReceiptQuoteError::QuoteReplayMismatch)
        ));

        let mut position = quote;
        position.market.positions[0].liquidity += 1;
        assert!(validate_v3_quote_replay(&position, policy()).is_err());
    }

    #[test]
    fn rejects_wrong_transaction_envelope_and_duplicate_launch_event() {
        let (mut transaction, receipt) = bow_fixture();
        transaction.to = Some(LAUNCHHOOD_V3_FACTORY);
        assert!(matches!(
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::Bow, policy()),
            Err(V3ReceiptQuoteError::TransactionEnvelope)
        ));

        let (transaction, mut receipt) = bow_fixture();
        let mut duplicate = receipt.logs.last().unwrap().clone();
        duplicate.log_index += 1;
        receipt.logs.push(duplicate);
        assert!(matches!(
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::Bow, policy()),
            Err(V3ReceiptQuoteError::LaunchEventIdentity)
        ));
    }

    #[test]
    fn rejects_embedded_swap_or_restriction_drift() {
        let (transaction, mut receipt) = launchhood_fixture();
        let mut corrupted_swap = receipt.logs[3].data.to_vec();
        corrupted_swap[31] ^= 1;
        receipt.logs[3].data = corrupted_swap.into();
        assert!(matches!(
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::LaunchHoodV3, policy()),
            Err(V3ReceiptQuoteError::EmbeddedSwapMismatch)
        ));

        let (transaction, mut receipt) = launchhood_fixture();
        receipt.l1_block_number = Some(receipt.l1_block_number.unwrap() + 1);
        assert!(matches!(
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::LaunchHoodV3, policy()),
            Err(V3ReceiptQuoteError::RestrictionEvidence)
        ));

        let (mut transaction, receipt) = bow_payable_fixture();
        transaction.value += U256::from(1_u8);
        assert!(matches!(
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::Bow, policy()),
            Err(V3ReceiptQuoteError::EmbeddedSwapMismatch)
        ));

        let (transaction, mut receipt) = bow_payable_fixture();
        let mut corrupted_swap = receipt.logs[4].data.to_vec();
        corrupted_swap[31] ^= 1;
        receipt.logs[4].data = corrupted_swap.into();
        assert!(matches!(
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::Bow, policy()),
            Err(V3ReceiptQuoteError::EmbeddedSwapMismatch)
        ));

        let (transaction, mut receipt) = bow_payable_fixture();
        receipt.logs.pop();
        assert!(matches!(
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::Bow, policy()),
            Err(V3ReceiptQuoteError::EmbeddedSwapMismatch)
        ));
    }

    #[test]
    fn rejects_unordered_logs_and_unsafe_sizing() {
        let (transaction, mut receipt) = bow_fixture();
        receipt.logs.swap(0, 1);
        assert!(matches!(
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::Bow, policy()),
            Err(V3ReceiptQuoteError::UnorderedLogs)
        ));

        let (transaction, receipt) = bow_fixture();
        let mut unsafe_policy = policy();
        unsafe_policy.amount_in = unsafe_policy.max_amount_in + U256::from(1_u8);
        assert!(matches!(
            quote_v3_launch_receipt(&transaction, &receipt, LaunchpadId::Bow, unsafe_policy),
            Err(V3ReceiptQuoteError::UnsafePolicy)
        ));
    }
}
