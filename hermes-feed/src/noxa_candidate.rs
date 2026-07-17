use alloy_primitives::{Address, U256};
use serde::Serialize;
use thiserror::Error;

use crate::noxa_abi::{NoxaLaunchEvent, V3ExactInputIntent};
use crate::noxa_policy::{
    NoxaPolicyDecision, NoxaPolicyInput, NoxaRejectReason, evaluate_noxa_policy,
};
use crate::noxa_predict::PredictedNoxaLaunch;
use crate::noxa_rpc::TokenRestrictionSnapshot;
use crate::noxa_trade::{TradePlanError, TradeTransactionPlan};
use crate::robinhood::{
    NOXA_DEX_ID_UNISWAP, NOXA_LAUNCH_CONFIG_ID_WETH, NOXA_LAUNCH_FACTORY, NOXA_POOL_FEE,
    UNISWAP_V3_FACTORY, WETH,
};
use crate::sequencer::ConditionalOptions;

#[derive(Debug, Clone, Copy)]
pub struct VerifiedNoxaTradeInput<'a> {
    pub launch: &'a NoxaLaunchEvent,
    pub restrictions: &'a TokenRestrictionSnapshot,
    pub launch_l1_block: u64,
    pub launch_l1_timestamp: u64,
    pub recipient: Address,
    pub recipient_balance_before: U256,
    pub origin_bought_before: U256,
    pub amount_in: U256,
    pub quoted_amount_out: U256,
    pub slippage_bps: u16,
    pub nonce: u64,
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    pub l1_window: u64,
    pub timestamp_window_seconds: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct PredictedNoxaTradeInput<'a> {
    pub launch: &'a PredictedNoxaLaunch,
    pub launch_l1_block: u64,
    pub launch_l1_timestamp: u64,
    pub recipient: Address,
    pub amount_in: U256,
    pub quoted_amount_out: U256,
    pub slippage_bps: u16,
    pub nonce: u64,
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    pub l1_window: u64,
    pub timestamp_window_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreparedNoxaTradeCandidate {
    pub plan: TradeTransactionPlan,
    pub conditions: ConditionalOptions,
    pub policy: NoxaPolicyDecision,
    pub quoted_amount_out: U256,
    pub amount_out_minimum: U256,
}

#[derive(Debug, Error)]
pub enum NoxaCandidateError {
    #[error("verified launch does not match the pinned NOXA/Uniswap deployment")]
    Configuration,
    #[error("recipient restriction snapshot is missing or inconsistent")]
    RecipientSnapshot,
    #[error("trade amount, quote, slippage, or boundary window is invalid")]
    InvalidInput,
    #[error("restriction block does not fit the L1 height domain")]
    IntegerRange,
    #[error("NOXA restriction policy rejected the candidate: {0:?}")]
    Policy(NoxaRejectReason),
    #[error(transparent)]
    TradePlan(#[from] TradePlanError),
}

/// Turn receipt-verified launch state and a cached local quote into the exact
/// transaction plan that can be signed before the next eligible L1 boundary.
/// This function performs no RPC, filesystem, logging, or network work.
pub fn prepare_verified_noxa_trade(
    input: VerifiedNoxaTradeInput<'_>,
) -> Result<PreparedNoxaTradeCandidate, NoxaCandidateError> {
    let launch = input.launch;
    let restrictions = input.restrictions;
    if launch.token == Address::ZERO
        || launch.token == WETH
        || launch.dex_factory != UNISWAP_V3_FACTORY
        || launch.pair_token != WETH
        || launch.dex_id != U256::from(NOXA_DEX_ID_UNISWAP)
        || launch.launch_config_id != U256::from(NOXA_LAUNCH_CONFIG_ID_WETH)
        || restrictions.token != launch.token
        || restrictions.launch_factory != NOXA_LAUNCH_FACTORY
        || restrictions.liquidity_pool != launch.pool
        || restrictions.pair_token != WETH
        || restrictions.pool_fee != NOXA_POOL_FEE
        || restrictions.restriction_end_block != launch.restrictions_end_l1_block
    {
        return Err(NoxaCandidateError::Configuration);
    }
    if restrictions.recipient != Some(input.recipient)
        || restrictions.recipient_balance != Some(input.recipient_balance_before)
    {
        return Err(NoxaCandidateError::RecipientSnapshot);
    }
    if input.recipient == Address::ZERO
        || input.amount_in == U256::ZERO
        || input.quoted_amount_out == U256::ZERO
        || input.slippage_bps > 10_000
        || input.l1_window == 0
    {
        return Err(NoxaCandidateError::InvalidInput);
    }
    let restrictions_end_l1_block = u64::try_from(launch.restrictions_end_l1_block)
        .map_err(|_| NoxaCandidateError::IntegerRange)?;
    let current_l1_block = input
        .launch_l1_block
        .checked_add(1)
        .ok_or(NoxaCandidateError::IntegerRange)?;
    let policy = evaluate_noxa_policy(NoxaPolicyInput {
        launch_l1_block: input.launch_l1_block,
        restrictions_end_l1_block,
        current_l1_block,
        recipient_balance_before: input.recipient_balance_before,
        expected_bought_output: input.quoted_amount_out,
        origin_bought_before: input.origin_bought_before,
        max_wallet_limit: restrictions.max_wallet_limit,
        max_tx_limit: restrictions.max_tx_limit,
    });
    if let NoxaPolicyDecision::Reject { reason } = policy {
        return Err(NoxaCandidateError::Policy(reason));
    }
    let amount_out_minimum = input
        .quoted_amount_out
        .checked_mul(U256::from(10_000_u64 - u64::from(input.slippage_bps)))
        .ok_or(NoxaCandidateError::InvalidInput)?
        / U256::from(10_000_u64);
    if amount_out_minimum == U256::ZERO {
        return Err(NoxaCandidateError::InvalidInput);
    }
    let plan = TradeTransactionPlan::exact_input(
        input.nonce,
        input.gas_limit,
        input.max_fee_per_gas,
        input.max_priority_fee_per_gas,
        &V3ExactInputIntent {
            token_in: WETH,
            token_out: launch.token,
            fee: NOXA_POOL_FEE,
            recipient: input.recipient,
            amount_in: input.amount_in,
            amount_out_minimum,
            sqrt_price_limit_x96: U256::ZERO,
        },
    )?;
    let timestamp_max = input
        .launch_l1_timestamp
        .checked_add(input.timestamp_window_seconds)
        .ok_or(NoxaCandidateError::IntegerRange)?;
    let conditions = ConditionalOptions::first_eligible_window(
        input.launch_l1_block,
        input.l1_window,
        Some(timestamp_max),
    )
    .ok_or(NoxaCandidateError::IntegerRange)?;
    Ok(PreparedNoxaTradeCandidate {
        plan,
        conditions,
        policy,
        quoted_amount_out: input.quoted_amount_out,
        amount_out_minimum,
    })
}

/// Prepare the first-public-block trade entirely from feed calldata and the
/// startup-pinned factory cache. No receipt or RPC state is needed here.
pub fn prepare_predicted_noxa_trade(
    input: PredictedNoxaTradeInput<'_>,
) -> Result<PreparedNoxaTradeCandidate, NoxaCandidateError> {
    let predicted = input.launch;
    if predicted.token == Address::ZERO
        || predicted.token == WETH
        || predicted.pool == Address::ZERO
        || input.recipient == Address::ZERO
        || input.amount_in == U256::ZERO
        || input.quoted_amount_out == U256::ZERO
        || input.slippage_bps > 10_000
        || input.l1_window == 0
    {
        return Err(NoxaCandidateError::InvalidInput);
    }
    let current_l1_block = input
        .launch_l1_block
        .checked_add(1)
        .ok_or(NoxaCandidateError::IntegerRange)?;
    let policy = evaluate_noxa_policy(NoxaPolicyInput {
        launch_l1_block: input.launch_l1_block,
        restrictions_end_l1_block: predicted.restrictions_end_l1_block,
        current_l1_block,
        // The CREATE2 address does not exist before this launch, so a
        // successful canonical creation starts both counters at zero.
        recipient_balance_before: U256::ZERO,
        expected_bought_output: input.quoted_amount_out,
        origin_bought_before: U256::ZERO,
        max_wallet_limit: predicted.max_wallet_limit,
        max_tx_limit: predicted.max_tx_limit,
    });
    if let NoxaPolicyDecision::Reject { reason } = policy {
        return Err(NoxaCandidateError::Policy(reason));
    }
    let amount_out_minimum = input
        .quoted_amount_out
        .checked_mul(U256::from(10_000_u64 - u64::from(input.slippage_bps)))
        .ok_or(NoxaCandidateError::InvalidInput)?
        / U256::from(10_000_u64);
    if amount_out_minimum == U256::ZERO {
        return Err(NoxaCandidateError::InvalidInput);
    }
    let plan = TradeTransactionPlan::exact_input(
        input.nonce,
        input.gas_limit,
        input.max_fee_per_gas,
        input.max_priority_fee_per_gas,
        &V3ExactInputIntent {
            token_in: WETH,
            token_out: predicted.token,
            fee: NOXA_POOL_FEE,
            recipient: input.recipient,
            amount_in: input.amount_in,
            amount_out_minimum,
            sqrt_price_limit_x96: U256::ZERO,
        },
    )?;
    let timestamp_max = input
        .launch_l1_timestamp
        .checked_add(input.timestamp_window_seconds)
        .ok_or(NoxaCandidateError::IntegerRange)?;
    let conditions = ConditionalOptions::first_eligible_window(
        input.launch_l1_block,
        input.l1_window,
        Some(timestamp_max),
    )
    .ok_or(NoxaCandidateError::IntegerRange)?;
    Ok(PreparedNoxaTradeCandidate {
        plan,
        conditions,
        policy,
        quoted_amount_out: input.quoted_amount_out,
        amount_out_minimum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::noxa_predict::PredictedNoxaLaunch;
    use crate::v3_pool::V3PoolState;

    fn launch() -> NoxaLaunchEvent {
        NoxaLaunchEvent {
            token: Address::with_last_byte(99),
            deployer: Address::with_last_byte(10),
            dex_factory: UNISWAP_V3_FACTORY,
            pair_token: WETH,
            pool: Address::with_last_byte(11),
            dex_id: U256::from(NOXA_DEX_ID_UNISWAP),
            launch_config_id: U256::from(NOXA_LAUNCH_CONFIG_ID_WETH),
            position_id: U256::from(1),
            restrictions_end_l1_block: U256::from(466),
            initial_buy_amount: U256::from(1_000),
        }
    }

    fn restrictions(launch: &NoxaLaunchEvent, recipient: Address) -> TokenRestrictionSnapshot {
        TokenRestrictionSnapshot {
            token: launch.token,
            l2_block_number: 10,
            launch_factory: NOXA_LAUNCH_FACTORY,
            liquidity_pool: launch.pool,
            pair_token: WETH,
            pool_fee: NOXA_POOL_FEE,
            max_wallet_limit: U256::from(10_000),
            max_tx_limit: U256::from(10_000),
            restriction_end_block: launch.restrictions_end_l1_block,
            recipient: Some(recipient),
            recipient_balance: Some(U256::from(10)),
        }
    }

    #[test]
    fn prepares_pinned_plan_for_first_public_l1_block() {
        let launch = launch();
        let recipient = Address::with_last_byte(12);
        let restrictions = restrictions(&launch, recipient);
        let prepared = prepare_verified_noxa_trade(VerifiedNoxaTradeInput {
            launch: &launch,
            restrictions: &restrictions,
            launch_l1_block: 100,
            launch_l1_timestamp: 1_800_000_000,
            recipient,
            recipient_balance_before: U256::from(10),
            origin_bought_before: U256::ZERO,
            amount_in: U256::from(500),
            quoted_amount_out: U256::from(1_000),
            slippage_bps: 500,
            nonce: 7,
            gas_limit: 300_000,
            max_fee_per_gas: 10,
            max_priority_fee_per_gas: 0,
            l1_window: 3,
            timestamp_window_seconds: 30,
        })
        .unwrap();
        assert_eq!(prepared.plan.nonce, 7);
        assert_eq!(prepared.plan.expected_token_out, launch.token);
        assert_eq!(prepared.amount_out_minimum, U256::from(950));
        assert_eq!(prepared.conditions.block_number_min, 101);
        assert_eq!(prepared.conditions.block_number_max, 104);
        assert_eq!(prepared.conditions.timestamp_max, Some(1_800_000_030));
        assert!(matches!(
            prepared.policy,
            NoxaPolicyDecision::Restricted { .. }
        ));
    }

    #[test]
    fn rejects_quote_that_would_exceed_restricted_wallet() {
        let launch = launch();
        let recipient = Address::with_last_byte(12);
        let mut restrictions = restrictions(&launch, recipient);
        restrictions.max_wallet_limit = U256::from(100);
        let error = prepare_verified_noxa_trade(VerifiedNoxaTradeInput {
            launch: &launch,
            restrictions: &restrictions,
            launch_l1_block: 100,
            launch_l1_timestamp: 1_800_000_000,
            recipient,
            recipient_balance_before: U256::from(10),
            origin_bought_before: U256::ZERO,
            amount_in: U256::from(500),
            quoted_amount_out: U256::from(1_000),
            slippage_bps: 500,
            nonce: 7,
            gas_limit: 300_000,
            max_fee_per_gas: 10,
            max_priority_fee_per_gas: 0,
            l1_window: 3,
            timestamp_window_seconds: 30,
        })
        .unwrap_err();
        assert!(matches!(
            error,
            NoxaCandidateError::Policy(NoxaRejectReason::MaxWalletExceeded { .. })
        ));
    }

    #[test]
    fn prepares_receipt_free_candidate_for_next_feed_boundary() {
        let recipient = Address::with_last_byte(12);
        let predicted = PredictedNoxaLaunch {
            token: Address::with_last_byte(99),
            pool: Address::with_last_byte(11),
            restrictions_end_l1_block: 466,
            initial_buy_amount: U256::from(1_000),
            max_wallet_limit: U256::from(10_000),
            max_tx_limit: U256::from(10_000),
            post_launch_pool: V3PoolState::new(
                Address::with_last_byte(11),
                Address::with_last_byte(1),
                Address::with_last_byte(99),
                10_000,
                200,
                U256::from(1_u128 << 96),
                0,
                1,
            )
            .unwrap(),
        };
        let candidate = prepare_predicted_noxa_trade(PredictedNoxaTradeInput {
            launch: &predicted,
            launch_l1_block: 100,
            launch_l1_timestamp: 1_800_000_000,
            recipient,
            amount_in: U256::from(500),
            quoted_amount_out: U256::from(1_000),
            slippage_bps: 500,
            nonce: 7,
            gas_limit: 300_000,
            max_fee_per_gas: 10,
            max_priority_fee_per_gas: 0,
            l1_window: 3,
            timestamp_window_seconds: 30,
        })
        .unwrap();
        assert_eq!(candidate.plan.nonce, 7);
        assert_eq!(candidate.plan.expected_token_out, predicted.token);
        assert_eq!(candidate.amount_out_minimum, U256::from(950));
        assert_eq!(candidate.conditions.block_number_min, 101);
    }
}
