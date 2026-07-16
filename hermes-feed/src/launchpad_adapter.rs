use alloy_primitives::{Address, B256, Bytes, U256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::copy_observation::normalize_aggregator_copy_swap;
use crate::noxa_abi::{
    AGGREGATOR_SWAP_SELECTOR, EXACT_INPUT_SINGLE_SELECTOR, V3ExactInputIntent,
    decode_v3_exact_input_single, encode_v3_exact_input_single,
};
use crate::noxa_predict::predict_v3_pool_address;
use crate::noxa_trade::{TradePlanError, TradeTransactionPlan};
use crate::robinhood::{
    CHAIN_ID, NOXA_POOL_FEE, ROBINHOOD_SWAP_AGGREGATOR, UNISWAP_V3_FACTORY,
    UNISWAP_V3_POOL_INIT_CODE_KECCAK256, UNISWAP_V3_SWAP_ROUTER_02, WETH,
};
use crate::v3_pool::{V3PoolError, V3PoolState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchpadId {
    Noxa,
    Bow,
    LaunchHoodV3,
    Clanker,
    BankrDoppler,
    KlikFinance,
    TrenchToday,
    Pons,
    Flap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributionSource {
    Virtuals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    V3LaunchAtBirth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    V3SingleHop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WrapperKind {
    Direct,
    Multicall,
    Erc4337,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Launch,
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MarketIdentity {
    pub token: Address,
    pub quote_asset: Address,
    pub pool: Address,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObservedAmounts {
    pub amount_in: U256,
    pub minimum_out: U256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedRoute {
    PonsFactory,
    FlapPortal,
    DirectV3,
    RobinhoodAggregator,
}

/// A normalized leader fact. It is deliberately not executable calldata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObservedLeaderAction {
    pub tx_hash: B256,
    pub launchpad: LaunchpadId,
    pub leader: Address,
    pub action: ActionKind,
    pub market: MarketIdentity,
    pub asset_in: Address,
    pub asset_out: Address,
    pub observed_amounts: ObservedAmounts,
    pub observed_route: ObservedRoute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateCall<'a> {
    pub tx_hash: B256,
    pub chain_id: Option<u64>,
    pub leader: Address,
    pub destination: Address,
    pub value: U256,
    pub calldata: &'a [u8],
    pub wrapper: WrapperKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FollowerTradePlan {
    pub launchpad: LaunchpadId,
    pub route: RouteKind,
    pub destination: Address,
    pub value: U256,
    pub calldata: Bytes,
    pub spend_limit: U256,
    pub min_receive: U256,
    pub expected_market: MarketIdentity,
}

#[derive(Debug, Clone, Copy)]
pub struct FollowerPlanRequest {
    pub action: ObservedLeaderAction,
    pub recipient: Address,
    pub amount_in: U256,
    pub min_receive: U256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterQuote {
    pub amount_in: U256,
    pub expected_out: U256,
    pub min_receive: U256,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum AdapterError {
    #[error("candidate is not on Robinhood chain ID 4663")]
    WrongChain,
    #[error("candidate destination is not pinned for Noxa")]
    WrongDestination,
    #[error("candidate selector is not supported")]
    WrongSelector,
    #[error("candidate wrapper is not supported by this adapter")]
    WrongWrapper,
    #[error("candidate calldata is malformed")]
    Malformed,
    #[error("candidate recipient is not the leader")]
    RedirectedRecipient,
    #[error("candidate value is incompatible with its route")]
    WrongValue,
    #[error("candidate is not a pinned-fee single-hop WETH market")]
    UnsupportedMarket,
    #[error("candidate uses a non-zero price limit")]
    PriceLimit,
    #[error("candidate amounts must be non-zero")]
    ZeroAmount,
    #[error("candidate pool identity does not match the pinned V3 factory")]
    WrongMarketIdentity,
    #[error("follower plan inputs are invalid")]
    InvalidPlan,
    #[error("slippage must be at most 10,000 basis points")]
    InvalidSlippage,
    #[error("local quote failed")]
    Quote,
}

pub trait LaunchpadAdapter {
    fn kind(&self) -> AdapterKind;
    fn observe(&self, call: CandidateCall<'_>) -> Result<ObservedLeaderAction, AdapterError>;
    fn quote_exact_input(
        &self,
        action: &ObservedLeaderAction,
        pool: &V3PoolState,
        amount_in: U256,
        slippage_bps: u16,
    ) -> Result<AdapterQuote, AdapterError>;
    fn plan(&self, request: FollowerPlanRequest) -> Result<FollowerTradePlan, AdapterError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoxaV3Adapter;

impl NoxaV3Adapter {
    fn validate_action(action: &ObservedLeaderAction) -> Result<(), AdapterError> {
        if action.launchpad != LaunchpadId::Noxa
            || action.market.token == Address::ZERO
            || action.market.token == WETH
            || action.market.quote_asset != WETH
            || action.market.pool != expected_noxa_pool(action.market.token)
        {
            return Err(AdapterError::WrongMarketIdentity);
        }
        let expected_assets = match action.action {
            ActionKind::Buy => (WETH, action.market.token),
            ActionKind::Sell => (action.market.token, WETH),
            ActionKind::Launch => return Err(AdapterError::UnsupportedMarket),
        };
        if (action.asset_in, action.asset_out) != expected_assets {
            return Err(AdapterError::UnsupportedMarket);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn observe_direct_intent(
        &self,
        tx_hash: B256,
        chain_id: Option<u64>,
        leader: Address,
        destination: Address,
        value: U256,
        intent: V3ExactInputIntent,
    ) -> Result<ObservedLeaderAction, AdapterError> {
        if chain_id != Some(CHAIN_ID) {
            return Err(AdapterError::WrongChain);
        }
        if destination != UNISWAP_V3_SWAP_ROUTER_02 {
            return Err(AdapterError::WrongDestination);
        }
        let call = CandidateCall {
            tx_hash,
            chain_id,
            leader,
            destination,
            value,
            calldata: &[],
            wrapper: WrapperKind::Direct,
        };
        Self::normalize_intent(
            call,
            &intent,
            expected_noxa_pool(if intent.token_in == WETH {
                intent.token_out
            } else {
                intent.token_in
            }),
            ObservedRoute::DirectV3,
            value,
        )
    }

    fn normalize_intent(
        call: CandidateCall<'_>,
        intent: &V3ExactInputIntent,
        pool: Address,
        route: ObservedRoute,
        normalized_value: U256,
    ) -> Result<ObservedLeaderAction, AdapterError> {
        if intent.recipient != call.leader {
            return Err(AdapterError::RedirectedRecipient);
        }
        if normalized_value != U256::ZERO {
            return Err(AdapterError::WrongValue);
        }
        if intent.fee != NOXA_POOL_FEE || !is_weth_pair(intent.token_in, intent.token_out) {
            return Err(AdapterError::UnsupportedMarket);
        }
        if intent.sqrt_price_limit_x96 != U256::ZERO {
            return Err(AdapterError::PriceLimit);
        }
        if intent.amount_in == U256::ZERO || intent.amount_out_minimum == U256::ZERO {
            return Err(AdapterError::ZeroAmount);
        }
        let token = if intent.token_in == WETH {
            intent.token_out
        } else {
            intent.token_in
        };
        let expected_pool = expected_noxa_pool(token);
        if pool != expected_pool {
            return Err(AdapterError::WrongMarketIdentity);
        }
        Ok(ObservedLeaderAction {
            tx_hash: call.tx_hash,
            launchpad: LaunchpadId::Noxa,
            leader: call.leader,
            action: if intent.token_in == WETH {
                ActionKind::Buy
            } else {
                ActionKind::Sell
            },
            market: MarketIdentity {
                token,
                quote_asset: WETH,
                pool,
            },
            asset_in: intent.token_in,
            asset_out: intent.token_out,
            observed_amounts: ObservedAmounts {
                amount_in: intent.amount_in,
                minimum_out: intent.amount_out_minimum,
            },
            observed_route: route,
        })
    }
}

impl LaunchpadAdapter for NoxaV3Adapter {
    fn kind(&self) -> AdapterKind {
        AdapterKind::V3LaunchAtBirth
    }

    fn observe(&self, call: CandidateCall<'_>) -> Result<ObservedLeaderAction, AdapterError> {
        if call.chain_id != Some(CHAIN_ID) {
            return Err(AdapterError::WrongChain);
        }
        if call.wrapper != WrapperKind::Direct {
            return Err(AdapterError::WrongWrapper);
        }
        let selector = call.calldata.get(..4).ok_or(AdapterError::Malformed)?;
        if call.destination == UNISWAP_V3_SWAP_ROUTER_02 {
            if selector != EXACT_INPUT_SINGLE_SELECTOR {
                return Err(AdapterError::WrongSelector);
            }
            let intent =
                decode_v3_exact_input_single(call.calldata).ok_or(AdapterError::Malformed)?;
            Self::normalize_intent(
                call,
                &intent,
                expected_noxa_pool(if intent.token_in == WETH {
                    intent.token_out
                } else {
                    intent.token_in
                }),
                ObservedRoute::DirectV3,
                call.value,
            )
        } else if call.destination == ROBINHOOD_SWAP_AGGREGATOR {
            if selector != AGGREGATOR_SWAP_SELECTOR {
                return Err(AdapterError::WrongSelector);
            }
            let normalized = normalize_aggregator_copy_swap(call.calldata, call.value, call.leader)
                .map_err(|_| AdapterError::Malformed)?;
            Self::normalize_intent(
                call,
                &normalized.intent,
                normalized.pool,
                ObservedRoute::RobinhoodAggregator,
                U256::ZERO,
            )
        } else {
            Err(AdapterError::WrongDestination)
        }
    }

    fn quote_exact_input(
        &self,
        action: &ObservedLeaderAction,
        pool: &V3PoolState,
        amount_in: U256,
        slippage_bps: u16,
    ) -> Result<AdapterQuote, AdapterError> {
        Self::validate_action(action)?;
        if pool.pool != action.market.pool
            || pool.fee != NOXA_POOL_FEE
            || !is_weth_pair(pool.token0, pool.token1)
        {
            return Err(AdapterError::WrongMarketIdentity);
        }
        if slippage_bps > 10_000 {
            return Err(AdapterError::InvalidSlippage);
        }
        let quote = pool
            .quote_exact_input(action.asset_in, amount_in, None)
            .map_err(|_: V3PoolError| AdapterError::Quote)?;
        let min_receive = quote
            .amount_out
            .checked_mul(U256::from(10_000_u16 - slippage_bps))
            .and_then(|value| value.checked_div(U256::from(10_000_u16)))
            .ok_or(AdapterError::Quote)?;
        if min_receive == U256::ZERO {
            return Err(AdapterError::Quote);
        }
        Ok(AdapterQuote {
            amount_in,
            expected_out: quote.amount_out,
            min_receive,
        })
    }

    fn plan(&self, request: FollowerPlanRequest) -> Result<FollowerTradePlan, AdapterError> {
        let action = request.action;
        Self::validate_action(&action)?;
        if request.recipient == Address::ZERO
            || request.amount_in == U256::ZERO
            || request.min_receive == U256::ZERO
        {
            return Err(AdapterError::InvalidPlan);
        }
        let (token_in, token_out) = match action.action {
            ActionKind::Buy => (WETH, action.market.token),
            ActionKind::Sell => (action.market.token, WETH),
            ActionKind::Launch => return Err(AdapterError::InvalidPlan),
        };
        let intent = V3ExactInputIntent {
            token_in,
            token_out,
            fee: NOXA_POOL_FEE,
            recipient: request.recipient,
            amount_in: request.amount_in,
            amount_out_minimum: request.min_receive,
            sqrt_price_limit_x96: U256::ZERO,
        };
        let calldata = encode_v3_exact_input_single(&intent).ok_or(AdapterError::InvalidPlan)?;
        Ok(FollowerTradePlan {
            launchpad: LaunchpadId::Noxa,
            route: RouteKind::V3SingleHop,
            destination: UNISWAP_V3_SWAP_ROUTER_02,
            value: U256::ZERO,
            calldata: calldata.into(),
            spend_limit: request.amount_in,
            min_receive: request.min_receive,
            expected_market: action.market,
        })
    }
}

impl FollowerTradePlan {
    /// Compatibility bridge into the existing nonce/risk/signer runtime.
    pub fn into_noxa_transaction_plan(
        self,
        nonce: u64,
        gas_limit: u64,
        max_fee_per_gas: u128,
        max_priority_fee_per_gas: u128,
        recipient: Address,
    ) -> Result<TradeTransactionPlan, TradePlanError> {
        if self.launchpad != LaunchpadId::Noxa
            || self.route != RouteKind::V3SingleHop
            || self.destination != UNISWAP_V3_SWAP_ROUTER_02
            || self.value != U256::ZERO
        {
            return Err(TradePlanError::UnsafeSwapParameters);
        }
        let intent = decode_v3_exact_input_single(&self.calldata)
            .ok_or(TradePlanError::UnsupportedCalldata)?;
        if intent.recipient != recipient
            || intent.amount_in != self.spend_limit
            || intent.amount_out_minimum != self.min_receive
            || expected_noxa_pool(self.expected_market.token) != self.expected_market.pool
        {
            return Err(TradePlanError::UnsafeSwapParameters);
        }
        TradeTransactionPlan::exact_input(
            nonce,
            gas_limit,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            &intent,
        )
    }
}

pub fn expected_noxa_pool(token: Address) -> Address {
    let (token0, token1) = if token < WETH {
        (token, WETH)
    } else {
        (WETH, token)
    };
    predict_v3_pool_address(
        UNISWAP_V3_FACTORY,
        token0,
        token1,
        NOXA_POOL_FEE,
        UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    )
}

fn is_weth_pair(token_in: Address, token_out: Address) -> bool {
    token_in != Address::ZERO
        && token_out != Address::ZERO
        && token_in != token_out
        && ((token_in == WETH) ^ (token_out == WETH))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leader() -> Address {
        Address::with_last_byte(1)
    }

    fn token() -> Address {
        Address::with_last_byte(0x42)
    }

    fn intent() -> V3ExactInputIntent {
        V3ExactInputIntent {
            token_in: WETH,
            token_out: token(),
            fee: NOXA_POOL_FEE,
            recipient: leader(),
            amount_in: U256::from(200),
            amount_out_minimum: U256::from(500),
            sqrt_price_limit_x96: U256::ZERO,
        }
    }

    fn call(calldata: &[u8]) -> CandidateCall<'_> {
        CandidateCall {
            tx_hash: B256::with_last_byte(9),
            chain_id: Some(CHAIN_ID),
            leader: leader(),
            destination: UNISWAP_V3_SWAP_ROUTER_02,
            value: U256::ZERO,
            calldata,
            wrapper: WrapperKind::Direct,
        }
    }

    #[test]
    fn observation_and_follower_plan_are_distinct_and_noxa_compatible() {
        let calldata = encode_v3_exact_input_single(&intent()).unwrap();
        let action = NoxaV3Adapter.observe(call(&calldata)).unwrap();
        let follower = NoxaV3Adapter
            .plan(FollowerPlanRequest {
                action,
                recipient: leader(),
                amount_in: U256::from(100),
                min_receive: U256::from(250),
            })
            .unwrap();
        let legacy = TradeTransactionPlan::exact_input(
            7,
            300_000,
            100,
            0,
            &V3ExactInputIntent {
                amount_in: U256::from(100),
                amount_out_minimum: U256::from(250),
                ..intent()
            },
        )
        .unwrap();
        let bridged = follower
            .into_noxa_transaction_plan(7, 300_000, 100, 0, leader())
            .unwrap();
        assert_eq!(bridged, legacy);
        assert_ne!(bridged.calldata, Bytes::from(calldata));
    }

    #[test]
    fn rejects_other_chains_lookalike_destinations_and_wrappers() {
        let calldata = encode_v3_exact_input_single(&intent()).unwrap();
        let mut candidate = call(&calldata);
        candidate.chain_id = Some(8_453);
        assert_eq!(
            NoxaV3Adapter.observe(candidate),
            Err(AdapterError::WrongChain)
        );
        candidate = call(&calldata);
        candidate.destination = Address::with_last_byte(0xff);
        assert_eq!(
            NoxaV3Adapter.observe(candidate),
            Err(AdapterError::WrongDestination)
        );
        candidate = call(&calldata);
        candidate.wrapper = WrapperKind::Multicall;
        assert_eq!(
            NoxaV3Adapter.observe(candidate),
            Err(AdapterError::WrongWrapper)
        );
    }

    #[test]
    fn rejects_noxa_market_lookalikes() {
        let mut bad = intent();
        bad.fee = 500;
        let calldata = encode_v3_exact_input_single(&bad).unwrap();
        assert_eq!(
            NoxaV3Adapter.observe(call(&calldata)),
            Err(AdapterError::UnsupportedMarket)
        );

        let calldata = encode_v3_exact_input_single(&intent()).unwrap();
        let mut action = NoxaV3Adapter.observe(call(&calldata)).unwrap();
        action.market.pool = Address::with_last_byte(0xfe);
        assert_eq!(
            NoxaV3Adapter.plan(FollowerPlanRequest {
                action,
                recipient: leader(),
                amount_in: U256::from(100),
                min_receive: U256::from(1),
            }),
            Err(AdapterError::WrongMarketIdentity)
        );
    }
}
