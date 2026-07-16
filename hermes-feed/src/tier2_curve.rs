//! Fail-closed observation and paper planning for the chain-4663 Tier-2
//! curve-to-V3 launchpads.
//!
//! This module deliberately has no RPC/client/filesystem dependencies. Runtime
//! code and proxy implementation checks happen before constructing the
//! registry; candidate handling only reads the resulting immutable snapshot.

use alloy_primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::launchpad_adapter::{
    ActionKind, FollowerTradePlan, LaunchpadId, MarketIdentity, ObservedAmounts,
    ObservedLeaderAction, ObservedRoute, RouteKind,
};
use crate::noxa_abi::{
    V3ExactInputIntent, decode_v3_exact_input_single, encode_v3_exact_input_single,
};
use crate::noxa_predict::predict_v3_pool_address;
use crate::robinhood::{
    CHAIN_ID, UNISWAP_V3_FACTORY, UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    UNISWAP_V3_POOL_RUNTIME_KECCAK256, UNISWAP_V3_SWAP_ROUTER_02, WETH,
};
use crate::v3_pool::V3PoolState;

pub const HOOD_FACTORY: Address =
    alloy_primitives::address!("5fcc1df0dc020cf454e742e9a8ae2554c37a452c");
pub const HOOD_LOCKER: Address =
    alloy_primitives::address!("ad69d8a00564f4a2365cc74594925f95281706aa");
pub const LEAVEHOOD_FACTORY_PROXY: Address =
    alloy_primitives::address!("2c81cd8acf4886f4abad332216b4444ae927fdb7");
pub const LEAVEHOOD_FACTORY_IMPLEMENTATION: Address =
    alloy_primitives::address!("7d6ccaadc2249a21a2b404eda2d9465e739c833b");
pub const LEAVEHOOD_CORE_PROXY: Address =
    alloy_primitives::address!("5090c9cd2228b0c4e6a83ee44ab77ce2e4cd89e3");
pub const LEAVEHOOD_CORE_IMPLEMENTATION: Address =
    alloy_primitives::address!("79446bca2a86b23cb6354178235222f491d18f56");

pub const HOOD_CREATE_SELECTOR: [u8; 4] = [0x42, 0xb6, 0x21, 0x37];
pub const HOOD_BUY_SELECTOR: [u8; 4] = [0xcc, 0xe7, 0xec, 0x13];
pub const HOOD_SELL_SELECTOR: [u8; 4] = [0x6a, 0x27, 0x24, 0x62];
pub const LEAVEHOOD_LAUNCH_SELECTORS: [[u8; 4]; 2] =
    [[0x0e, 0x1d, 0x30, 0x73], [0xfc, 0xd0, 0x50, 0x8f]];
pub const LEAVEHOOD_BUY_SELECTOR: [u8; 4] = [0x67, 0x84, 0xad, 0x1e];
pub const LEAVEHOOD_SELL_WITH_SLIPPAGE_SELECTOR: [u8; 4] = [0x0d, 0xda, 0x52, 0xf6];
pub const LEAVEHOOD_SELL_SELECTOR: [u8; 4] = [0x6c, 0x19, 0x7f, 0xf5];
pub const LEAVEHOOD_CLAIM_FEES_SELECTOR: [u8; 4] = [0xd6, 0xae, 0x6e, 0x44];
pub const HOOD_V3_FEE: u32 = 10_000;
pub const MAX_CURVE_CANDIDATE_CALLDATA_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FactoryGeneration {
    HoodCustomLaunchpad,
    LeaveHoodProxyV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketPhase {
    Curve,
    MigratedV3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum CurveFormula {
    HoodConstantProductFeeOnInputV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpportunityQuality {
    /// Creation attribution without any proven executable follow-on market.
    CreationOnly,
    /// Some organic activity exists, but route/liquidity breadth is insufficient.
    Observe,
    /// The warm snapshot has repeat activity, multiple traders and executable liquidity.
    PaperEligible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimePin {
    pub address: Address,
    pub implementation: Option<Address>,
    /// Expected hash from reviewed startup configuration.
    pub runtime_code_hash: B256,
    /// Hash observed by the one-time startup hydration pass.
    pub observed_runtime_code_hash: B256,
    pub implementation_runtime_code_hash: Option<B256>,
    pub observed_implementation_runtime_code_hash: Option<B256>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupPins {
    pub chain_id: u64,
    pub hood_factory: RuntimePin,
    pub leavehood_factory: RuntimePin,
    pub leavehood_core: RuntimePin,
    pub v3_factory: RuntimePin,
    pub v3_router: RuntimePin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnabledAdapters {
    hood: bool,
    leavehood: bool,
    v3: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurveCandidateCall<'a> {
    pub chain_id: u64,
    pub destination: Address,
    pub input: &'a [u8],
    pub value: U256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CurveObservation {
    pub protocol: LaunchpadId,
    pub generation: FactoryGeneration,
    pub action: ActionKind,
    pub phase: MarketPhase,
    pub token: Option<Address>,
    pub amount_in: Option<U256>,
    pub leader_min_receive: Option<U256>,
    pub launch_automation: bool,
    pub paper_plan_supported: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpportunityEvidence {
    pub launches: u64,
    pub follow_on_swaps: u64,
    pub distinct_follow_on_traders: u64,
    pub observed_quote_volume: U256,
    pub executable_liquidity_quote: U256,
    pub route_unambiguous: bool,
    pub runtime_pins_current: bool,
}

impl OpportunityEvidence {
    pub fn quality(&self) -> OpportunityQuality {
        if self.follow_on_swaps == 0
            || self.distinct_follow_on_traders == 0
            || self.observed_quote_volume == U256::ZERO
        {
            return OpportunityQuality::CreationOnly;
        }
        if self.follow_on_swaps >= 2
            && self.distinct_follow_on_traders >= 2
            && self.executable_liquidity_quote != U256::ZERO
            && self.route_unambiguous
            && self.runtime_pins_current
        {
            OpportunityQuality::PaperEligible
        } else {
            OpportunityQuality::Observe
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct CurveState {
    pub formula: CurveFormula,
    pub virtual_quote_reserve: U256,
    pub virtual_token_reserve: U256,
    pub remaining_curve_tokens: U256,
    pub fee_bps: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodCurveBuyQuote {
    pub amount_in_requested: U256,
    pub amount_in_consumed: U256,
    pub refund: U256,
    pub fee: U256,
    pub amount_for_curve: U256,
    pub amount_out: U256,
    pub graduates: bool,
    pub state_after: CurveState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodCurveSellQuote {
    pub amount_in: U256,
    pub gross_output: U256,
    pub fee: U256,
    pub amount_out: U256,
    pub state_after: CurveState,
}

#[derive(Debug, Clone)]
pub struct MarketSnapshot<'a> {
    pub protocol: LaunchpadId,
    pub generation: FactoryGeneration,
    pub token: Address,
    pub phase: MarketPhase,
    pub quality: OpportunityEvidence,
    pub curve: Option<CurveState>,
    pub v3: Option<&'a V3PoolState>,
    pub v3_pool_runtime_code_hash: Option<B256>,
    pub observed_v3_pool_runtime_code_hash: Option<B256>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CurvePaperPlan {
    pub plan: FollowerTradePlan,
    pub expected_receive: U256,
    /// This workstream is paper-only by construction.
    pub execution_enabled: bool,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum CurveAdapterError {
    #[error("candidate is not on Robinhood Chain 4663")]
    WrongChain,
    #[error("adapter is disabled because a startup identity pin is missing or drifted")]
    PinMismatch,
    #[error("destination and selector are not an allowlisted observation form")]
    UnknownDispatch,
    #[error("calldata is malformed or non-canonical")]
    Malformed,
    #[error("the route matches more than one warm market")]
    AmbiguousRoute,
    #[error("market identity is not present in warm state")]
    UnknownMarket,
    #[error("launch attribution is not a follow-on executable-liquidity signal")]
    LaunchAutomation,
    #[error("opportunity quality is below the paper threshold")]
    LowQuality,
    #[error("required curve, upgrade, sell, fee, or migration evidence is incomplete")]
    EvidenceIncomplete,
    #[error("market phase or route does not match the pinned snapshot")]
    RouteMismatch,
    #[error("invalid amount, slippage, fee, or reserve state")]
    InvalidState,
    #[error("quote arithmetic failed")]
    Arithmetic,
}

#[derive(Debug, Clone, Copy)]
pub struct Tier2CurveAdapter {
    enabled: EnabledAdapters,
}

impl Tier2CurveAdapter {
    /// Builds an immutable candidate-time registry from startup-observed pins.
    /// A non-zero hash proves that startup pinned exact runtime bytes; proxy
    /// generations additionally require the evidence-backed implementation.
    pub fn new(pins: StartupPins) -> Result<Self, CurveAdapterError> {
        if pins.chain_id != CHAIN_ID {
            return Err(CurveAdapterError::WrongChain);
        }
        let hood = valid_pin(pins.hood_factory, HOOD_FACTORY, None);
        let leavehood = valid_pin(
            pins.leavehood_factory,
            LEAVEHOOD_FACTORY_PROXY,
            Some(LEAVEHOOD_FACTORY_IMPLEMENTATION),
        ) && valid_pin(
            pins.leavehood_core,
            LEAVEHOOD_CORE_PROXY,
            Some(LEAVEHOOD_CORE_IMPLEMENTATION),
        );
        let v3 = valid_pin(pins.v3_factory, UNISWAP_V3_FACTORY, None)
            && valid_pin(pins.v3_router, UNISWAP_V3_SWAP_ROUTER_02, None);
        Ok(Self {
            enabled: EnabledAdapters {
                hood,
                leavehood,
                v3,
            },
        })
    }

    pub fn observe(
        &self,
        call: CurveCandidateCall<'_>,
        markets: &[MarketSnapshot<'_>],
    ) -> Result<CurveObservation, CurveAdapterError> {
        if call.chain_id != CHAIN_ID {
            return Err(CurveAdapterError::WrongChain);
        }
        if call.input.len() > MAX_CURVE_CANDIDATE_CALLDATA_BYTES {
            return Err(CurveAdapterError::Malformed);
        }
        let selector: [u8; 4] = call
            .input
            .get(..4)
            .ok_or(CurveAdapterError::Malformed)?
            .try_into()
            .map_err(|_| CurveAdapterError::Malformed)?;

        if call.destination == HOOD_FACTORY {
            if !self.enabled.hood {
                return Err(CurveAdapterError::PinMismatch);
            }
            let observed = observe_hood(selector, call.input, call.value)?;
            validate_observed_market_phase(&observed, markets)?;
            return Ok(observed);
        }
        if call.destination == LEAVEHOOD_FACTORY_PROXY {
            if !self.enabled.leavehood {
                return Err(CurveAdapterError::PinMismatch);
            }
            if LEAVEHOOD_LAUNCH_SELECTORS.contains(&selector) {
                require_abi_envelope(call.input)?;
                return Ok(CurveObservation {
                    protocol: LaunchpadId::LeaveHood,
                    generation: FactoryGeneration::LeaveHoodProxyV1,
                    action: ActionKind::Launch,
                    phase: MarketPhase::Curve,
                    token: None,
                    amount_in: None,
                    leader_min_receive: None,
                    launch_automation: true,
                    paper_plan_supported: false,
                });
            }
            return Err(CurveAdapterError::UnknownDispatch);
        }
        if call.destination == LEAVEHOOD_CORE_PROXY {
            if !self.enabled.leavehood {
                return Err(CurveAdapterError::PinMismatch);
            }
            if matches!(
                selector,
                LEAVEHOOD_BUY_SELECTOR
                    | LEAVEHOOD_SELL_WITH_SLIPPAGE_SELECTOR
                    | LEAVEHOOD_SELL_SELECTOR
            ) {
                require_abi_envelope(call.input)?;
                return Ok(CurveObservation {
                    protocol: LaunchpadId::LeaveHood,
                    generation: FactoryGeneration::LeaveHoodProxyV1,
                    action: if selector == LEAVEHOOD_BUY_SELECTOR {
                        ActionKind::Buy
                    } else {
                        ActionKind::Sell
                    },
                    phase: MarketPhase::Curve,
                    token: None,
                    amount_in: None,
                    leader_min_receive: None,
                    launch_automation: false,
                    paper_plan_supported: false,
                });
            }
            return Err(CurveAdapterError::UnknownDispatch);
        }
        if call.destination == UNISWAP_V3_SWAP_ROUTER_02 {
            if !self.enabled.v3 {
                return Err(CurveAdapterError::PinMismatch);
            }
            return observe_v3(call.input, markets);
        }
        Err(CurveAdapterError::UnknownDispatch)
    }

    pub fn normalize_observed_action(
        &self,
        tx_hash: B256,
        leader: Address,
        observation: &CurveObservation,
        market: &MarketSnapshot<'_>,
    ) -> Result<ObservedLeaderAction, CurveAdapterError> {
        if leader == Address::ZERO
            || observation.protocol != market.protocol
            || observation.generation != market.generation
            || observation.phase != market.phase
            || observation.token != Some(market.token)
        {
            return Err(CurveAdapterError::RouteMismatch);
        }
        let amount_in = observation
            .amount_in
            .filter(|amount| *amount != U256::ZERO)
            .ok_or(CurveAdapterError::InvalidState)?;
        let minimum_out = observation
            .leader_min_receive
            .ok_or(CurveAdapterError::InvalidState)?;
        let (asset_in, asset_out) = match observation.action {
            ActionKind::Buy => (WETH, market.token),
            ActionKind::Sell => (market.token, WETH),
            ActionKind::Launch => return Err(CurveAdapterError::LaunchAutomation),
        };
        let (pool, route) = match market.phase {
            MarketPhase::Curve => (HOOD_FACTORY, ObservedRoute::HoodCurve),
            MarketPhase::MigratedV3 => (
                market.v3.ok_or(CurveAdapterError::EvidenceIncomplete)?.pool,
                ObservedRoute::MigratedV3,
            ),
        };
        Ok(ObservedLeaderAction {
            tx_hash,
            launchpad: observation.protocol,
            leader,
            action: observation.action,
            market: MarketIdentity {
                token: market.token,
                quote_asset: WETH,
                pool,
            },
            asset_in,
            asset_out,
            observed_amounts: ObservedAmounts {
                amount_in,
                minimum_out,
            },
            observed_route: route,
        })
    }

    pub fn plan_follow_on(
        &self,
        observation: &CurveObservation,
        market: &MarketSnapshot<'_>,
        follower_amount_in: U256,
        follower_recipient: Address,
        slippage_bps: u16,
    ) -> Result<CurvePaperPlan, CurveAdapterError> {
        if observation.action == ActionKind::Launch || observation.launch_automation {
            return Err(CurveAdapterError::LaunchAutomation);
        }
        if market.quality.quality() != OpportunityQuality::PaperEligible {
            return Err(CurveAdapterError::LowQuality);
        }
        if observation.protocol != market.protocol
            || observation.generation != market.generation
            || observation.phase != market.phase
            || observation.token != Some(market.token)
        {
            return Err(CurveAdapterError::RouteMismatch);
        }
        if follower_amount_in == U256::ZERO
            || follower_recipient == Address::ZERO
            || slippage_bps >= 10_000
        {
            return Err(CurveAdapterError::InvalidState);
        }
        match market.phase {
            MarketPhase::Curve => {
                self.plan_curve(observation, market, follower_amount_in, slippage_bps)
            }
            MarketPhase::MigratedV3 => self.plan_v3(
                observation,
                market,
                follower_amount_in,
                follower_recipient,
                slippage_bps,
            ),
        }
    }

    fn plan_curve(
        &self,
        observation: &CurveObservation,
        market: &MarketSnapshot<'_>,
        amount_in: U256,
        slippage_bps: u16,
    ) -> Result<CurvePaperPlan, CurveAdapterError> {
        if market.protocol != LaunchpadId::HoodFun || !self.enabled.hood {
            // LeaveHood selectors are observable, but its curve formula, fees,
            // restrictions and verified implementation ABI are incomplete.
            return Err(CurveAdapterError::EvidenceIncomplete);
        }
        let state = market.curve.ok_or(CurveAdapterError::EvidenceIncomplete)?;
        if !(100..=500).contains(&state.fee_bps) {
            return Err(CurveAdapterError::InvalidState);
        }
        let expected = match observation.action {
            ActionKind::Buy => quote_curve_buy(state, amount_in)?,
            ActionKind::Sell => quote_curve_sell(state, amount_in)?,
            ActionKind::Launch => return Err(CurveAdapterError::LaunchAutomation),
        };
        let minimum = apply_slippage(expected, slippage_bps)?;
        let calldata = match observation.action {
            ActionKind::Buy => {
                encode_words(HOOD_BUY_SELECTOR, &[address_word(market.token), minimum])
            }
            ActionKind::Sell => encode_words(
                HOOD_SELL_SELECTOR,
                &[address_word(market.token), amount_in, minimum],
            ),
            ActionKind::Launch => unreachable!(),
        };
        Ok(CurvePaperPlan {
            plan: FollowerTradePlan {
                launchpad: LaunchpadId::HoodFun,
                route: RouteKind::NativeBondingCurve,
                destination: HOOD_FACTORY,
                value: if observation.action == ActionKind::Buy {
                    amount_in
                } else {
                    U256::ZERO
                },
                calldata: calldata.into(),
                spend_limit: amount_in,
                min_receive: minimum,
                expected_market: MarketIdentity {
                    token: market.token,
                    quote_asset: WETH,
                    pool: HOOD_FACTORY,
                },
            },
            expected_receive: expected,
            execution_enabled: false,
        })
    }

    fn plan_v3(
        &self,
        observation: &CurveObservation,
        market: &MarketSnapshot<'_>,
        amount_in: U256,
        recipient: Address,
        slippage_bps: u16,
    ) -> Result<CurvePaperPlan, CurveAdapterError> {
        if !self.enabled.v3 {
            return Err(CurveAdapterError::PinMismatch);
        }
        if market.protocol != LaunchpadId::HoodFun {
            // LeaveHood's V3 fee tier, migration event and token-to-pool
            // derivation are not established by the checkpoint evidence.
            return Err(CurveAdapterError::EvidenceIncomplete);
        }
        let pool = market.v3.ok_or(CurveAdapterError::EvidenceIncomplete)?;
        if pool.fee != HOOD_V3_FEE
            || pool.pool != expected_v3_pool(market.token)
            || !valid_v3_pool_runtime_pin(market)
        {
            return Err(CurveAdapterError::RouteMismatch);
        }
        let token_in = match observation.action {
            ActionKind::Buy => WETH,
            ActionKind::Sell => market.token,
            ActionKind::Launch => return Err(CurveAdapterError::LaunchAutomation),
        };
        let quote = pool
            .quote_exact_input(token_in, amount_in, None)
            .map_err(|_| CurveAdapterError::InvalidState)?;
        let minimum = apply_slippage(quote.amount_out, slippage_bps)?;
        let calldata = encode_v3_exact_input_single(&V3ExactInputIntent {
            token_in,
            token_out: quote.token_out,
            fee: HOOD_V3_FEE,
            recipient,
            amount_in,
            amount_out_minimum: minimum,
            sqrt_price_limit_x96: U256::ZERO,
        })
        .ok_or(CurveAdapterError::InvalidState)?;
        Ok(CurvePaperPlan {
            plan: FollowerTradePlan {
                launchpad: market.protocol,
                route: RouteKind::V3SingleHop,
                destination: UNISWAP_V3_SWAP_ROUTER_02,
                // Hermes supplies pre-wrapped WETH to SwapRouter02. Native
                // value is never attached to exactInputSingle.
                value: U256::ZERO,
                calldata: calldata.into(),
                spend_limit: amount_in,
                min_receive: minimum,
                expected_market: MarketIdentity {
                    token: market.token,
                    quote_asset: WETH,
                    pool: pool.pool,
                },
            },
            expected_receive: quote.amount_out,
            execution_enabled: false,
        })
    }
}

fn valid_pin(pin: RuntimePin, address: Address, implementation: Option<Address>) -> bool {
    pin.address == address
        && pin.implementation == implementation
        && pin.runtime_code_hash != B256::ZERO
        && pin.runtime_code_hash == pin.observed_runtime_code_hash
        && match implementation {
            Some(_) => pin
                .implementation_runtime_code_hash
                .is_some_and(|expected| {
                    expected != B256::ZERO
                        && pin.observed_implementation_runtime_code_hash == Some(expected)
                }),
            None => {
                pin.implementation_runtime_code_hash.is_none()
                    && pin.observed_implementation_runtime_code_hash.is_none()
            }
        }
}

fn observe_hood(
    selector: [u8; 4],
    input: &[u8],
    value: U256,
) -> Result<CurveObservation, CurveAdapterError> {
    if selector == HOOD_CREATE_SELECTOR {
        require_abi_envelope(input)?;
        return Ok(CurveObservation {
            protocol: LaunchpadId::HoodFun,
            generation: FactoryGeneration::HoodCustomLaunchpad,
            action: ActionKind::Launch,
            phase: MarketPhase::Curve,
            token: None,
            amount_in: None,
            leader_min_receive: None,
            launch_automation: true,
            paper_plan_supported: false,
        });
    }
    let (action, words) = match selector {
        HOOD_BUY_SELECTOR => (ActionKind::Buy, decode_static_words(input, 2)?),
        HOOD_SELL_SELECTOR => (ActionKind::Sell, decode_static_words(input, 3)?),
        _ => return Err(CurveAdapterError::UnknownDispatch),
    };
    let token = decode_address(words[0]).ok_or(CurveAdapterError::Malformed)?;
    if token == Address::ZERO
        || (action == ActionKind::Buy && value == U256::ZERO)
        || (action == ActionKind::Sell && value != U256::ZERO)
    {
        return Err(CurveAdapterError::Malformed);
    }
    let amount_in = if action == ActionKind::Buy {
        value
    } else {
        words[1]
    };
    let leader_min_receive = if action == ActionKind::Buy {
        words[1]
    } else {
        words[2]
    };
    if amount_in == U256::ZERO || leader_min_receive == U256::ZERO {
        return Err(CurveAdapterError::Malformed);
    }
    Ok(CurveObservation {
        protocol: LaunchpadId::HoodFun,
        generation: FactoryGeneration::HoodCustomLaunchpad,
        action,
        phase: MarketPhase::Curve,
        token: Some(token),
        amount_in: Some(amount_in),
        leader_min_receive: Some(leader_min_receive),
        launch_automation: false,
        paper_plan_supported: true,
    })
}

fn observe_v3(
    input: &[u8],
    markets: &[MarketSnapshot<'_>],
) -> Result<CurveObservation, CurveAdapterError> {
    let intent = decode_v3_exact_input_single(input).ok_or(CurveAdapterError::Malformed)?;
    if intent.fee != HOOD_V3_FEE || intent.amount_in == U256::ZERO {
        return Err(CurveAdapterError::RouteMismatch);
    }
    let (token, action) = if intent.token_in == WETH && intent.token_out != Address::ZERO {
        (intent.token_out, ActionKind::Buy)
    } else if intent.token_out == WETH && intent.token_in != Address::ZERO {
        (intent.token_in, ActionKind::Sell)
    } else {
        return Err(CurveAdapterError::RouteMismatch);
    };
    let mut matches = markets.iter().filter(|market| {
        market.token == token
            && market.phase == MarketPhase::MigratedV3
            && market
                .v3
                .is_some_and(|pool| pool.pool == expected_v3_pool(token))
            && valid_v3_pool_runtime_pin(market)
    });
    let market = matches.next().ok_or(CurveAdapterError::UnknownMarket)?;
    if matches.next().is_some() {
        return Err(CurveAdapterError::AmbiguousRoute);
    }
    Ok(CurveObservation {
        protocol: market.protocol,
        generation: market.generation,
        action,
        phase: MarketPhase::MigratedV3,
        token: Some(token),
        amount_in: Some(intent.amount_in),
        leader_min_receive: Some(intent.amount_out_minimum),
        launch_automation: false,
        paper_plan_supported: market.protocol == LaunchpadId::HoodFun,
    })
}

fn quote_curve_buy(state: CurveState, amount_quote: U256) -> Result<U256, CurveAdapterError> {
    Ok(quote_hood_curve_buy(state, amount_quote)?.amount_out)
}

fn quote_curve_sell(state: CurveState, amount_token: U256) -> Result<U256, CurveAdapterError> {
    Ok(quote_hood_curve_sell(state, amount_token)?.amount_out)
}

/// Source-exact Hood buy quote, including the graduation cap, consumed ETH and
/// refund. This mirrors `HoodCustomLaunchpad._buy` rather than its UI.
pub fn quote_hood_curve_buy(
    state: CurveState,
    amount_quote: U256,
) -> Result<HoodCurveBuyQuote, CurveAdapterError> {
    validate_curve(state)?;
    if amount_quote == U256::ZERO {
        return Err(CurveAdapterError::InvalidState);
    }
    let initial_fee = floor_fee(amount_quote, state.fee_bps)?;
    let initial_curve_amount = amount_quote
        .checked_sub(initial_fee)
        .ok_or(CurveAdapterError::Arithmetic)?;
    let invariant = state
        .virtual_quote_reserve
        .checked_mul(state.virtual_token_reserve)
        .ok_or(CurveAdapterError::Arithmetic)?;
    let initial_next_quote = state
        .virtual_quote_reserve
        .checked_add(initial_curve_amount)
        .ok_or(CurveAdapterError::Arithmetic)?;
    let initial_next_token = invariant / initial_next_quote;
    let mut amount_out = state
        .virtual_token_reserve
        .checked_sub(initial_next_token)
        .ok_or(CurveAdapterError::Arithmetic)?;
    let mut fee = initial_fee;
    let mut amount_for_curve = initial_curve_amount;
    let mut amount_consumed = amount_quote;
    let mut refund = U256::ZERO;
    let graduates = amount_out >= state.remaining_curve_tokens;
    if graduates {
        amount_out = state.remaining_curve_tokens;
        let terminal_virtual_tokens = state
            .virtual_token_reserve
            .checked_sub(amount_out)
            .filter(|value| *value != U256::ZERO)
            .ok_or(CurveAdapterError::InvalidState)?;
        let exact_curve_amount = div_ceil(invariant, terminal_virtual_tokens)?
            .checked_sub(state.virtual_quote_reserve)
            .ok_or(CurveAdapterError::Arithmetic)?
            .min(initial_curve_amount);
        let gross = div_ceil(
            exact_curve_amount
                .checked_mul(U256::from(10_000_u16))
                .ok_or(CurveAdapterError::Arithmetic)?,
            U256::from(10_000_u16 - state.fee_bps),
        )?
        .min(amount_quote);
        refund = amount_quote
            .checked_sub(gross)
            .ok_or(CurveAdapterError::Arithmetic)?;
        fee = gross
            .checked_sub(exact_curve_amount)
            .ok_or(CurveAdapterError::Arithmetic)?;
        amount_for_curve = exact_curve_amount;
        amount_consumed = gross;
    }
    if amount_out == U256::ZERO {
        return Err(CurveAdapterError::InvalidState);
    }
    Ok(HoodCurveBuyQuote {
        amount_in_requested: amount_quote,
        amount_in_consumed: amount_consumed,
        refund,
        fee,
        amount_for_curve,
        amount_out,
        graduates,
        state_after: CurveState {
            formula: state.formula,
            virtual_quote_reserve: state
                .virtual_quote_reserve
                .checked_add(amount_for_curve)
                .ok_or(CurveAdapterError::Arithmetic)?,
            virtual_token_reserve: state
                .virtual_token_reserve
                .checked_sub(amount_out)
                .ok_or(CurveAdapterError::Arithmetic)?,
            remaining_curve_tokens: state
                .remaining_curve_tokens
                .checked_sub(amount_out)
                .ok_or(CurveAdapterError::Arithmetic)?,
            fee_bps: state.fee_bps,
        },
    })
}

/// Source-exact Hood sell quote and deterministic post-trade curve state.
pub fn quote_hood_curve_sell(
    state: CurveState,
    amount_token: U256,
) -> Result<HoodCurveSellQuote, CurveAdapterError> {
    validate_curve(state)?;
    if amount_token == U256::ZERO {
        return Err(CurveAdapterError::InvalidState);
    }
    let invariant = state
        .virtual_quote_reserve
        .checked_mul(state.virtual_token_reserve)
        .ok_or(CurveAdapterError::Arithmetic)?;
    let next_token = state
        .virtual_token_reserve
        .checked_add(amount_token)
        .ok_or(CurveAdapterError::Arithmetic)?;
    let next_quote = div_ceil(invariant, next_token)?;
    let gross = state
        .virtual_quote_reserve
        .checked_sub(next_quote)
        .ok_or(CurveAdapterError::Arithmetic)?;
    let fee = floor_fee(gross, state.fee_bps)?;
    let output = gross
        .checked_sub(fee)
        .ok_or(CurveAdapterError::Arithmetic)?;
    if output == U256::ZERO {
        return Err(CurveAdapterError::InvalidState);
    }
    Ok(HoodCurveSellQuote {
        amount_in: amount_token,
        gross_output: gross,
        fee,
        amount_out: output,
        state_after: CurveState {
            formula: state.formula,
            virtual_quote_reserve: next_quote,
            virtual_token_reserve: next_token,
            remaining_curve_tokens: state
                .remaining_curve_tokens
                .checked_add(amount_token)
                .ok_or(CurveAdapterError::Arithmetic)?,
            fee_bps: state.fee_bps,
        },
    })
}

fn validate_curve(state: CurveState) -> Result<(), CurveAdapterError> {
    if state.formula != CurveFormula::HoodConstantProductFeeOnInputV1
        || state.virtual_quote_reserve == U256::ZERO
        || state.virtual_token_reserve == U256::ZERO
        || state.remaining_curve_tokens == U256::ZERO
        || !(100..=500).contains(&state.fee_bps)
    {
        return Err(CurveAdapterError::InvalidState);
    }
    Ok(())
}

fn validate_observed_market_phase(
    observation: &CurveObservation,
    markets: &[MarketSnapshot<'_>],
) -> Result<(), CurveAdapterError> {
    let Some(token) = observation.token else {
        return Ok(());
    };
    let mut matches = markets.iter().filter(|market| market.token == token);
    let Some(market) = matches.next() else {
        return Ok(());
    };
    if matches.next().is_some() {
        return Err(CurveAdapterError::AmbiguousRoute);
    }
    if market.protocol != observation.protocol
        || market.generation != observation.generation
        || market.phase != observation.phase
    {
        return Err(CurveAdapterError::RouteMismatch);
    }
    Ok(())
}

fn floor_fee(value: U256, fee_bps: u16) -> Result<U256, CurveAdapterError> {
    value
        .checked_mul(U256::from(fee_bps))
        .and_then(|value| value.checked_div(U256::from(10_000_u16)))
        .ok_or(CurveAdapterError::Arithmetic)
}

fn apply_slippage(value: U256, slippage_bps: u16) -> Result<U256, CurveAdapterError> {
    let minimum = value
        .checked_mul(U256::from(10_000_u16 - slippage_bps))
        .and_then(|value| value.checked_div(U256::from(10_000_u16)))
        .ok_or(CurveAdapterError::Arithmetic)?;
    if minimum == U256::ZERO {
        return Err(CurveAdapterError::InvalidState);
    }
    Ok(minimum)
}

fn div_ceil(numerator: U256, denominator: U256) -> Result<U256, CurveAdapterError> {
    if denominator == U256::ZERO {
        return Err(CurveAdapterError::Arithmetic);
    }
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    quotient
        .checked_add(U256::from(remainder != U256::ZERO))
        .ok_or(CurveAdapterError::Arithmetic)
}

fn expected_v3_pool(token: Address) -> Address {
    let (token0, token1) = if WETH < token {
        (WETH, token)
    } else {
        (token, WETH)
    };
    predict_v3_pool_address(
        UNISWAP_V3_FACTORY,
        token0,
        token1,
        HOOD_V3_FEE,
        UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    )
}

fn valid_v3_pool_runtime_pin(market: &MarketSnapshot<'_>) -> bool {
    market.v3_pool_runtime_code_hash == Some(UNISWAP_V3_POOL_RUNTIME_KECCAK256)
        && market.observed_v3_pool_runtime_code_hash == market.v3_pool_runtime_code_hash
}

fn require_abi_envelope(input: &[u8]) -> Result<(), CurveAdapterError> {
    if input.len() < 36 || !(input.len() - 4).is_multiple_of(32) {
        return Err(CurveAdapterError::Malformed);
    }
    Ok(())
}

fn decode_static_words(input: &[u8], count: usize) -> Result<Vec<U256>, CurveAdapterError> {
    if input.len() != 4 + count * 32 {
        return Err(CurveAdapterError::Malformed);
    }
    Ok(input[4..]
        .chunks_exact(32)
        .map(U256::from_be_slice)
        .collect())
}

fn decode_address(word: U256) -> Option<Address> {
    if word >> 160 != U256::ZERO {
        return None;
    }
    let bytes = word.to_be_bytes::<32>();
    Some(Address::from_slice(&bytes[12..]))
}

fn address_word(address: Address) -> U256 {
    U256::from_be_slice(address.as_slice())
}

fn encode_words(selector: [u8; 4], words: &[U256]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(4 + words.len() * 32);
    encoded.extend_from_slice(&selector);
    for word in words {
        encoded.extend_from_slice(&word.to_be_bytes::<32>());
    }
    encoded
}

#[cfg(test)]
mod tests {
    use alloy_primitives::b256;
    use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

    use super::*;

    fn hash(byte: u8) -> B256 {
        B256::repeat_byte(byte)
    }

    fn pin(address: Address, implementation: Option<Address>, byte: u8) -> RuntimePin {
        let runtime_code_hash = hash(byte);
        let implementation_runtime_code_hash = implementation.map(|_| hash(byte.wrapping_add(64)));
        RuntimePin {
            address,
            implementation,
            runtime_code_hash,
            observed_runtime_code_hash: runtime_code_hash,
            implementation_runtime_code_hash,
            observed_implementation_runtime_code_hash: implementation_runtime_code_hash,
        }
    }

    fn registry() -> Tier2CurveAdapter {
        Tier2CurveAdapter::new(StartupPins {
            chain_id: CHAIN_ID,
            hood_factory: pin(HOOD_FACTORY, None, 1),
            leavehood_factory: pin(
                LEAVEHOOD_FACTORY_PROXY,
                Some(LEAVEHOOD_FACTORY_IMPLEMENTATION),
                2,
            ),
            leavehood_core: pin(LEAVEHOOD_CORE_PROXY, Some(LEAVEHOOD_CORE_IMPLEMENTATION), 3),
            v3_factory: pin(UNISWAP_V3_FACTORY, None, 4),
            v3_router: pin(UNISWAP_V3_SWAP_ROUTER_02, None, 5),
        })
        .unwrap()
    }

    fn token() -> Address {
        Address::with_last_byte(0x77)
    }

    fn quality() -> OpportunityEvidence {
        OpportunityEvidence {
            launches: 10_000,
            follow_on_swaps: 3,
            distinct_follow_on_traders: 2,
            observed_quote_volume: U256::from(10),
            executable_liquidity_quote: U256::from(5),
            route_unambiguous: true,
            runtime_pins_current: true,
        }
    }

    fn hood_buy(minimum: U256) -> Vec<u8> {
        encode_words(HOOD_BUY_SELECTOR, &[address_word(token()), minimum])
    }

    fn hood_market() -> MarketSnapshot<'static> {
        MarketSnapshot {
            protocol: LaunchpadId::HoodFun,
            generation: FactoryGeneration::HoodCustomLaunchpad,
            token: token(),
            phase: MarketPhase::Curve,
            quality: quality(),
            curve: Some(CurveState {
                formula: CurveFormula::HoodConstantProductFeeOnInputV1,
                virtual_quote_reserve: U256::from(2_810_000_000_000_000_000_u128),
                virtual_token_reserve: U256::from(1_145_000_000_u64)
                    * U256::from(10_u64).pow(U256::from(18)),
                remaining_curve_tokens: U256::from(800_000_000_u64)
                    * U256::from(10_u64).pow(U256::from(18)),
                fee_bps: 300,
            }),
            v3: None,
            v3_pool_runtime_code_hash: None,
            observed_v3_pool_runtime_code_hash: None,
        }
    }

    #[test]
    fn observes_hood_follow_on_buy_and_separates_launch() {
        let adapter = registry();
        let buy = adapter
            .observe(
                CurveCandidateCall {
                    chain_id: CHAIN_ID,
                    destination: HOOD_FACTORY,
                    input: &hood_buy(U256::from(999)),
                    value: U256::from(100),
                },
                &[],
            )
            .unwrap();
        assert_eq!(buy.action, ActionKind::Buy);
        assert!(!buy.launch_automation);
        assert_eq!(buy.leader_min_receive, Some(U256::from(999)));
        let normalized = adapter
            .normalize_observed_action(
                B256::with_last_byte(0xa1),
                Address::with_last_byte(0xa2),
                &buy,
                &hood_market(),
            )
            .unwrap();
        assert_eq!(normalized.launchpad, LaunchpadId::HoodFun);
        assert_eq!(normalized.observed_route, ObservedRoute::HoodCurve);

        let mut launch = HOOD_CREATE_SELECTOR.to_vec();
        launch.extend_from_slice(&[0_u8; 32]);
        let launch = adapter
            .observe(
                CurveCandidateCall {
                    chain_id: CHAIN_ID,
                    destination: HOOD_FACTORY,
                    input: &launch,
                    value: U256::ZERO,
                },
                &[],
            )
            .unwrap();
        assert_eq!(launch.action, ActionKind::Launch);
        assert!(launch.launch_automation);
        assert!(!launch.paper_plan_supported);

        let sell_input = encode_words(
            HOOD_SELL_SELECTOR,
            &[address_word(token()), U256::from(50), U256::from(4)],
        );
        let sell = adapter
            .observe(
                CurveCandidateCall {
                    chain_id: CHAIN_ID,
                    destination: HOOD_FACTORY,
                    input: &sell_input,
                    value: U256::ZERO,
                },
                &[],
            )
            .unwrap();
        assert_eq!(sell.action, ActionKind::Sell);
        assert_eq!(sell.amount_in, Some(U256::from(50)));
    }

    #[test]
    fn constructs_fresh_hood_curve_minimum_instead_of_inheriting_leader_limit() {
        let adapter = registry();
        let observation = adapter
            .observe(
                CurveCandidateCall {
                    chain_id: CHAIN_ID,
                    destination: HOOD_FACTORY,
                    input: &hood_buy(U256::from(1)),
                    value: U256::from(100),
                },
                &[],
            )
            .unwrap();
        let plan = adapter
            .plan_follow_on(
                &observation,
                &hood_market(),
                U256::from(100_000_000_000_000_000_u128),
                Address::with_last_byte(4),
                500,
            )
            .unwrap();
        assert_eq!(plan.plan.route, RouteKind::NativeBondingCurve);
        assert_eq!(plan.plan.value, U256::from(100_000_000_000_000_000_u128));
        assert_ne!(plan.plan.min_receive, U256::from(1));
        assert!(!plan.execution_enabled);
        assert_eq!(
            U256::from_be_slice(&plan.plan.calldata[36..68]),
            plan.plan.min_receive
        );
    }

    #[test]
    fn hood_curve_quotes_match_deployed_floor_and_ceil_boundaries() {
        let state = CurveState {
            formula: CurveFormula::HoodConstantProductFeeOnInputV1,
            virtual_quote_reserve: U256::from(281_u16),
            virtual_token_reserve: U256::from(1_145_u16),
            remaining_curve_tokens: U256::from(800_u16),
            fee_bps: 100,
        };

        // buy: fee=floor(101/100)=1, net=100,
        // newVTok=floor(281*1145/(281+100))=844, output=301.
        assert_eq!(
            quote_curve_buy(state, U256::from(101_u8)).unwrap(),
            U256::from(301_u16)
        );

        // sell: newVEth=ceil(281*1145/(1145+100))=259,
        // gross=22, fee=floor(22/100)=0, output=22.
        assert_eq!(
            quote_curve_sell(state, U256::from(100_u8)).unwrap(),
            U256::from(22_u8)
        );
    }

    #[test]
    fn hood_graduation_quote_matches_live_cap_consumption_and_refund() {
        let state = CurveState {
            formula: CurveFormula::HoodConstantProductFeeOnInputV1,
            virtual_quote_reserve: U256::from(3_217_079_090_000_000_000_u128),
            virtual_token_reserve: U256::from_str_radix("1000115294025923372620596652", 10)
                .unwrap(),
            remaining_curve_tokens: U256::from_str_radix("655115294025923372620596652", 10)
                .unwrap(),
            fee_bps: 100,
        };
        let quote =
            quote_hood_curve_buy(state, U256::from(7_000_000_000_000_000_000_u128)).unwrap();
        assert!(quote.graduates);
        assert_eq!(
            quote.amount_out,
            U256::from_str_radix("655115294025923372620596652", 10).unwrap()
        );
        assert_eq!(
            quote.amount_in_consumed,
            U256::from(6_170_568_625_237_886_109_u128)
        );
        assert_eq!(quote.refund, U256::from(829_431_374_762_113_891_u128));
        assert_eq!(quote.fee, U256::from(61_705_686_252_378_862_u128));
        assert_eq!(
            quote.amount_for_curve,
            U256::from(6_108_862_938_985_507_247_u128)
        );
        assert_eq!(
            quote.state_after.virtual_quote_reserve,
            U256::from(9_325_942_028_985_507_247_u128)
        );
        assert_eq!(
            quote.state_after.virtual_token_reserve,
            U256::from_str_radix("345000000000000000000000000", 10).unwrap()
        );
        assert_eq!(quote.state_after.remaining_curve_tokens, U256::ZERO);
    }

    #[test]
    fn leavehood_curve_is_observable_but_planning_fails_closed() {
        let adapter = registry();
        let mut input = LEAVEHOOD_BUY_SELECTOR.to_vec();
        input.extend_from_slice(&[0_u8; 64]);
        let observation = adapter
            .observe(
                CurveCandidateCall {
                    chain_id: CHAIN_ID,
                    destination: LEAVEHOOD_CORE_PROXY,
                    input: &input,
                    value: U256::from(1),
                },
                &[],
            )
            .unwrap();
        assert_eq!(observation.action, ActionKind::Buy);
        assert!(!observation.paper_plan_supported);
        let market = MarketSnapshot {
            protocol: LaunchpadId::LeaveHood,
            generation: FactoryGeneration::LeaveHoodProxyV1,
            token: token(),
            phase: MarketPhase::Curve,
            quality: quality(),
            curve: None,
            v3: None,
            v3_pool_runtime_code_hash: None,
            observed_v3_pool_runtime_code_hash: None,
        };
        let mut identified = observation;
        identified.token = Some(token());
        assert_eq!(
            adapter.plan_follow_on(
                &identified,
                &market,
                U256::from(10),
                Address::with_last_byte(1),
                100
            ),
            Err(CurveAdapterError::EvidenceIncomplete)
        );

        for selector in LEAVEHOOD_LAUNCH_SELECTORS {
            let mut launch = selector.to_vec();
            launch.extend_from_slice(&[0_u8; 32]);
            let observed = adapter
                .observe(
                    CurveCandidateCall {
                        chain_id: CHAIN_ID,
                        destination: LEAVEHOOD_FACTORY_PROXY,
                        input: &launch,
                        value: U256::ZERO,
                    },
                    &[],
                )
                .unwrap();
            assert_eq!(observed.action, ActionKind::Launch);
            assert!(observed.launch_automation);
        }

        for selector in [
            LEAVEHOOD_SELL_WITH_SLIPPAGE_SELECTOR,
            LEAVEHOOD_SELL_SELECTOR,
        ] {
            let mut sell = selector.to_vec();
            sell.extend_from_slice(&[0_u8; 64]);
            let observed = adapter
                .observe(
                    CurveCandidateCall {
                        chain_id: CHAIN_ID,
                        destination: LEAVEHOOD_CORE_PROXY,
                        input: &sell,
                        value: U256::ZERO,
                    },
                    &[],
                )
                .unwrap();
            assert_eq!(observed.action, ActionKind::Sell);
        }

        let mut claim = LEAVEHOOD_CLAIM_FEES_SELECTOR.to_vec();
        claim.extend_from_slice(&[0_u8; 32]);
        assert_eq!(
            adapter.observe(
                CurveCandidateCall {
                    chain_id: CHAIN_ID,
                    destination: LEAVEHOOD_CORE_PROXY,
                    input: &claim,
                    value: U256::ZERO,
                },
                &[],
            ),
            Err(CurveAdapterError::UnknownDispatch)
        );
    }

    #[test]
    fn launch_spam_never_becomes_executable_liquidity() {
        let evidence = OpportunityEvidence {
            launches: 1_000_000,
            ..OpportunityEvidence::default()
        };
        assert_eq!(evidence.quality(), OpportunityQuality::CreationOnly);
        let mut market = hood_market();
        market.quality = evidence;
        let observation = CurveObservation {
            protocol: LaunchpadId::HoodFun,
            generation: FactoryGeneration::HoodCustomLaunchpad,
            action: ActionKind::Buy,
            phase: MarketPhase::Curve,
            token: Some(token()),
            amount_in: Some(U256::from(1)),
            leader_min_receive: Some(U256::from(1)),
            launch_automation: false,
            paper_plan_supported: true,
        };
        assert_eq!(
            registry().plan_follow_on(
                &observation,
                &market,
                U256::from(10),
                Address::with_last_byte(1),
                100
            ),
            Err(CurveAdapterError::LowQuality)
        );
    }

    #[test]
    fn rejects_lookalike_generation_malformed_and_wrong_chain_calls() {
        let adapter = registry();
        let input = hood_buy(U256::from(1));
        for (chain_id, destination, bytes, expected) in [
            (
                8453,
                HOOD_FACTORY,
                input.as_slice(),
                CurveAdapterError::WrongChain,
            ),
            (
                CHAIN_ID,
                Address::with_last_byte(0x99),
                input.as_slice(),
                CurveAdapterError::UnknownDispatch,
            ),
            (
                CHAIN_ID,
                HOOD_FACTORY,
                &input[..67],
                CurveAdapterError::Malformed,
            ),
        ] {
            assert_eq!(
                adapter.observe(
                    CurveCandidateCall {
                        chain_id,
                        destination,
                        input: bytes,
                        value: U256::from(1),
                    },
                    &[]
                ),
                Err(expected)
            );
        }

        let mut pins = StartupPins {
            chain_id: CHAIN_ID,
            hood_factory: pin(HOOD_FACTORY, None, 1),
            leavehood_factory: pin(LEAVEHOOD_FACTORY_PROXY, Some(Address::with_last_byte(9)), 2),
            leavehood_core: pin(LEAVEHOOD_CORE_PROXY, Some(LEAVEHOOD_CORE_IMPLEMENTATION), 3),
            v3_factory: pin(UNISWAP_V3_FACTORY, None, 4),
            v3_router: pin(UNISWAP_V3_SWAP_ROUTER_02, None, 5),
        };
        let disabled = Tier2CurveAdapter::new(pins).unwrap();
        let mut leave = LEAVEHOOD_LAUNCH_SELECTORS[0].to_vec();
        leave.extend_from_slice(&[0_u8; 32]);
        assert_eq!(
            disabled.observe(
                CurveCandidateCall {
                    chain_id: CHAIN_ID,
                    destination: LEAVEHOOD_FACTORY_PROXY,
                    input: &leave,
                    value: U256::ZERO,
                },
                &[]
            ),
            Err(CurveAdapterError::PinMismatch)
        );

        let mut drifted_hood = pin(HOOD_FACTORY, None, 1);
        drifted_hood.observed_runtime_code_hash = hash(99);
        pins.hood_factory = drifted_hood;
        pins.leavehood_factory = pin(
            LEAVEHOOD_FACTORY_PROXY,
            Some(LEAVEHOOD_FACTORY_IMPLEMENTATION),
            2,
        );
        let disabled = Tier2CurveAdapter::new(pins).unwrap();
        assert_eq!(
            disabled.observe(
                CurveCandidateCall {
                    chain_id: CHAIN_ID,
                    destination: HOOD_FACTORY,
                    input: &input,
                    value: U256::from(1),
                },
                &[],
            ),
            Err(CurveAdapterError::PinMismatch)
        );
        pins.chain_id = 1;
        assert!(matches!(
            Tier2CurveAdapter::new(pins),
            Err(CurveAdapterError::WrongChain)
        ));
    }

    #[test]
    fn rejects_oversized_candidate_calldata_before_opaque_observation() {
        let adapter = registry();
        for (destination, selector) in [
            (LEAVEHOOD_FACTORY_PROXY, LEAVEHOOD_LAUNCH_SELECTORS[0]),
            (LEAVEHOOD_CORE_PROXY, LEAVEHOOD_BUY_SELECTOR),
            (HOOD_FACTORY, HOOD_BUY_SELECTOR),
        ] {
            // The payload is deliberately ABI-envelope aligned so the opaque
            // LeaveHood checks would otherwise accept it based on shape alone.
            let mut oversized = selector.to_vec();
            oversized.extend_from_slice(&vec![0_u8; MAX_CURVE_CANDIDATE_CALLDATA_BYTES]);
            assert_eq!((oversized.len() - 4) % 32, 0);
            assert_eq!(
                adapter.observe(
                    CurveCandidateCall {
                        chain_id: CHAIN_ID,
                        destination,
                        input: &oversized,
                        value: U256::from(1),
                    },
                    &[],
                ),
                Err(CurveAdapterError::Malformed)
            );
        }
    }

    #[test]
    fn route_ambiguity_and_cross_adapter_negatives_fail_closed() {
        let adapter = registry();
        let wrong_selector =
            encode_words(HOOD_BUY_SELECTOR, &[address_word(token()), U256::from(1)]);
        assert_eq!(
            adapter.observe(
                CurveCandidateCall {
                    chain_id: CHAIN_ID,
                    destination: LEAVEHOOD_CORE_PROXY,
                    input: &wrong_selector,
                    value: U256::from(1),
                },
                &[]
            ),
            Err(CurveAdapterError::UnknownDispatch)
        );

        // A valid V3-shaped call is not attributed without exactly one warm,
        // migrated protocol market.
        let v3 = encode_v3_exact_input_single(&V3ExactInputIntent {
            token_in: WETH,
            token_out: token(),
            fee: HOOD_V3_FEE,
            recipient: Address::with_last_byte(3),
            amount_in: U256::from(10),
            amount_out_minimum: U256::from(1),
            sqrt_price_limit_x96: U256::ZERO,
        })
        .unwrap();
        assert_eq!(
            adapter.observe(
                CurveCandidateCall {
                    chain_id: CHAIN_ID,
                    destination: UNISWAP_V3_SWAP_ROUTER_02,
                    input: &v3,
                    value: U256::from(10),
                },
                &[]
            ),
            Err(CurveAdapterError::UnknownMarket)
        );

        let pool_address = expected_v3_pool(token());
        let (token0, token1) = if WETH < token() {
            (WETH, token())
        } else {
            (token(), WETH)
        };
        let pool = V3PoolState::new(
            pool_address,
            token0,
            token1,
            HOOD_V3_FEE,
            200,
            get_sqrt_ratio_at_tick(0).unwrap(),
            0,
            1,
        )
        .unwrap();
        let hood = MarketSnapshot {
            protocol: LaunchpadId::HoodFun,
            generation: FactoryGeneration::HoodCustomLaunchpad,
            token: token(),
            phase: MarketPhase::MigratedV3,
            quality: quality(),
            curve: None,
            v3: Some(&pool),
            v3_pool_runtime_code_hash: Some(UNISWAP_V3_POOL_RUNTIME_KECCAK256),
            observed_v3_pool_runtime_code_hash: Some(UNISWAP_V3_POOL_RUNTIME_KECCAK256),
        };
        let leavehood = MarketSnapshot {
            protocol: LaunchpadId::LeaveHood,
            generation: FactoryGeneration::LeaveHoodProxyV1,
            ..hood.clone()
        };
        assert_eq!(
            adapter.observe(
                CurveCandidateCall {
                    chain_id: CHAIN_ID,
                    destination: UNISWAP_V3_SWAP_ROUTER_02,
                    input: &v3,
                    value: U256::from(10),
                },
                &[hood, leavehood],
            ),
            Err(CurveAdapterError::AmbiguousRoute)
        );

        for foreign in [
            alloy_primitives::address!("62b33a039d289cbda50ebeb72fe4261449e61bcf"),
            alloy_primitives::address!("2e9fbf18f6492f6651b983c34629d292516de86e"),
        ] {
            assert_eq!(
                adapter.observe(
                    CurveCandidateCall {
                        chain_id: CHAIN_ID,
                        destination: foreign,
                        input: &wrong_selector,
                        value: U256::from(1),
                    },
                    &[],
                ),
                Err(CurveAdapterError::UnknownDispatch)
            );
        }
    }

    #[test]
    fn migrated_v3_is_a_distinct_follow_on_route_for_hood() {
        let pool_address = expected_v3_pool(token());
        let (token0, token1) = if WETH < token() {
            (WETH, token())
        } else {
            (token(), WETH)
        };
        let mut pool = V3PoolState::new(
            pool_address,
            token0,
            token1,
            HOOD_V3_FEE,
            200,
            get_sqrt_ratio_at_tick(0).unwrap(),
            0,
            1_000_000_000_000_000_000,
        )
        .unwrap();
        pool.add_position(-200, 200, 1_000_000_000_000_000_000)
            .unwrap();
        let market = MarketSnapshot {
            protocol: LaunchpadId::HoodFun,
            generation: FactoryGeneration::HoodCustomLaunchpad,
            token: token(),
            phase: MarketPhase::MigratedV3,
            quality: quality(),
            curve: None,
            v3: Some(&pool),
            v3_pool_runtime_code_hash: Some(UNISWAP_V3_POOL_RUNTIME_KECCAK256),
            observed_v3_pool_runtime_code_hash: Some(UNISWAP_V3_POOL_RUNTIME_KECCAK256),
        };
        let input = encode_v3_exact_input_single(&V3ExactInputIntent {
            token_in: WETH,
            token_out: token(),
            fee: HOOD_V3_FEE,
            recipient: Address::with_last_byte(8),
            amount_in: U256::from(10_000),
            amount_out_minimum: U256::from(1),
            sqrt_price_limit_x96: U256::ZERO,
        })
        .unwrap();
        let observed = registry()
            .observe(
                CurveCandidateCall {
                    chain_id: CHAIN_ID,
                    destination: UNISWAP_V3_SWAP_ROUTER_02,
                    input: &input,
                    value: U256::from(10_000),
                },
                std::slice::from_ref(&market),
            )
            .unwrap();
        assert_eq!(observed.phase, MarketPhase::MigratedV3);
        assert_eq!(observed.protocol, LaunchpadId::HoodFun);
        let plan = registry()
            .plan_follow_on(
                &observed,
                &market,
                U256::from(10_000),
                Address::with_last_byte(9),
                100,
            )
            .unwrap();
        assert_eq!(plan.plan.route, RouteKind::V3SingleHop);
        assert_eq!(plan.plan.value, U256::ZERO);
        assert_ne!(plan.plan.min_receive, U256::from(1));
        assert!(!plan.execution_enabled);

        let leavehood_market = MarketSnapshot {
            protocol: LaunchpadId::LeaveHood,
            generation: FactoryGeneration::LeaveHoodProxyV1,
            token: token(),
            phase: MarketPhase::MigratedV3,
            quality: quality(),
            curve: None,
            v3: Some(&pool),
            v3_pool_runtime_code_hash: Some(UNISWAP_V3_POOL_RUNTIME_KECCAK256),
            observed_v3_pool_runtime_code_hash: Some(UNISWAP_V3_POOL_RUNTIME_KECCAK256),
        };
        let leavehood_observation = CurveObservation {
            protocol: LaunchpadId::LeaveHood,
            generation: FactoryGeneration::LeaveHoodProxyV1,
            action: ActionKind::Buy,
            phase: MarketPhase::MigratedV3,
            token: Some(token()),
            amount_in: Some(U256::from(10_000)),
            leader_min_receive: Some(U256::from(1)),
            launch_automation: false,
            paper_plan_supported: false,
        };
        assert_eq!(
            registry().plan_follow_on(
                &leavehood_observation,
                &leavehood_market,
                U256::from(10_000),
                Address::with_last_byte(9),
                100,
            ),
            Err(CurveAdapterError::EvidenceIncomplete)
        );
    }

    #[test]
    fn candidate_path_types_contain_no_io_or_rpc_handles() {
        // Compile-time API guard: the hot-path inputs are borrowed bytes and
        // immutable warm snapshots. This assertion also catches accidental
        // growth that would indicate a client/config handle entered the path.
        assert_eq!(std::mem::size_of::<CurveCandidateCall<'_>>(), 80);
        assert!(std::mem::size_of::<Tier2CurveAdapter>() <= 3);
        let _ = b256!("0101010101010101010101010101010101010101010101010101010101010101");
    }
}
