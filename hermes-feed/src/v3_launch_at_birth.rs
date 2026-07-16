//! Shared, pure adapter for launchpads that create canonical V3 liquidity in
//! the launch transaction. This module observes launch calls and constructs
//! paper plans only; it does not sign, submit, or perform network I/O.

use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::{SolCall, sol};
use serde::Serialize;
use thiserror::Error;

use crate::launchpad_adapter::{FollowerTradePlan, LaunchpadId, MarketIdentity, RouteKind};
use crate::noxa_abi::{V3ExactInputIntent, encode_v3_exact_input_single};
use crate::noxa_predict::predict_v3_pool_address;
use crate::robinhood::{
    BOW_LAUNCH_FACTORY, CHAIN_ID, LAUNCHHOOD_V3_FACTORY, UNISWAP_V3_FACTORY,
    UNISWAP_V3_FACTORY_RUNTIME_KECCAK256, UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    UNISWAP_V3_POSITION_MANAGER, UNISWAP_V3_POSITION_MANAGER_RUNTIME_KECCAK256,
    UNISWAP_V3_SWAP_ROUTER_02, UNISWAP_V3_SWAP_ROUTER_02_RUNTIME_KECCAK256, WETH,
    WETH_RUNTIME_KECCAK256,
};

const V3_FEE: u32 = 10_000;
const LAUNCHHOOD_CONFIG_ID: u64 = 0;
const MAX_CALLDATA_BYTES: usize = 32 * 1024;
const MAX_DYNAMIC_STRING_BYTES: usize = 4 * 1024;

sol! {
    struct BowLaunchParams {
        string name;
        string symbol;
        uint256 totalSupply;
        uint256 launchDelay;
        uint256 maxWallet;
        uint256 limitWindow;
        uint256 targetFdvWeth;
        bytes32 salt;
        string description;
        string website;
        string telegram;
        string twitter;
        string logoURI;
        string tokenURI;
        uint256 devBuyMinTokens;
    }
    function launch(BowLaunchParams p) external payable returns (address token, uint256 positionId);

    struct LaunchHoodTokenParams {
        string name;
        string symbol;
        string metadataURI;
        address rewardRecipient;
    }
    function launchToken(
        LaunchHoodTokenParams p,
        uint256 configId,
        uint256 dexId,
        bytes32 userSalt,
        uint256 minTokensOut
    ) external payable returns (address token, address pool, uint256 positionId);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractCodeSnapshot {
    pub address: Address,
    pub runtime_code_hash: B256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaunchMarket {
    pub launchpad: LaunchpadId,
    pub token: Address,
    pub pool: Address,
    pub quote_asset: Address,
    pub fee: u32,
    pub restriction_state: MarketRestrictionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketRestrictionState {
    Clear,
    Active,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaunchCallObservation {
    pub launchpad: LaunchpadId,
    pub factory: Address,
    pub deployer: Address,
    pub transaction_value: U256,
    pub embedded_initial_buy: bool,
    /// Recorded for audit only. It is never copied into a follower plan.
    pub leader_min_tokens_out: U256,
    pub predicted_market: Option<LaunchMarket>,
    /// False until all protocol-specific code and deterministic derivation
    /// gaps in the checked-in evidence have been closed.
    pub execution_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowerPlanInput {
    pub market: LaunchMarket,
    pub recipient: Address,
    pub spend_limit: U256,
    /// Output of the already-warm local V3 state model.
    pub locally_quoted_receive: U256,
    /// Fresh follower policy result, independent of the leader's slippage.
    pub min_receive: U256,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum V3LaunchError {
    #[error("adapter requires Robinhood mainnet chain ID 4663")]
    WrongChain,
    #[error("a startup-pinned contract address or runtime code hash mismatched")]
    CodeIdentity,
    #[error("destination and selector do not identify exactly one adapter")]
    UnknownDispatch,
    #[error("launch calldata is malformed or non-canonical")]
    MalformedCalldata,
    #[error("launch configuration is outside the pinned profile")]
    UnsupportedLaunchConfig,
    #[error("receipt launch event is malformed or belongs to another adapter")]
    MalformedReceipt,
    #[error("receipt pool does not equal the canonical V3 pool")]
    WrongPool,
    #[error("follower plan violates the pinned V3 or policy invariants")]
    UnsafeFollowerPlan,
    #[error("pre-receipt market identity is not proven by checked-in evidence")]
    PlanUnavailable,
}

#[derive(Debug, Clone)]
pub struct V3LaunchAtBirthAdapter;

impl V3LaunchAtBirthAdapter {
    /// Validate every code identity once during startup. The resulting registry
    /// contains no client handle and therefore cannot make candidate-time RPC.
    pub fn new(chain_id: u64, code: &[ContractCodeSnapshot]) -> Result<Self, V3LaunchError> {
        if chain_id != CHAIN_ID {
            return Err(V3LaunchError::WrongChain);
        }
        let expected = [
            (WETH, WETH_RUNTIME_KECCAK256),
            (UNISWAP_V3_FACTORY, UNISWAP_V3_FACTORY_RUNTIME_KECCAK256),
            (
                UNISWAP_V3_POSITION_MANAGER,
                UNISWAP_V3_POSITION_MANAGER_RUNTIME_KECCAK256,
            ),
            (
                UNISWAP_V3_SWAP_ROUTER_02,
                UNISWAP_V3_SWAP_ROUTER_02_RUNTIME_KECCAK256,
            ),
        ];
        if !expected.iter().all(|pin| {
            code.iter()
                .any(|got| (got.address, got.runtime_code_hash) == *pin)
        }) {
            return Err(V3LaunchError::CodeIdentity);
        }
        Ok(Self)
    }

    pub fn observe_launch_call(
        &self,
        chain_id: u64,
        destination: Address,
        deployer: Address,
        transaction_value: U256,
        input: &[u8],
    ) -> Result<LaunchCallObservation, V3LaunchError> {
        if chain_id != CHAIN_ID {
            return Err(V3LaunchError::WrongChain);
        }
        if input.len() > MAX_CALLDATA_BYTES {
            return Err(V3LaunchError::MalformedCalldata);
        }
        if destination == BOW_LAUNCH_FACTORY {
            let call =
                launchCall::abi_decode(input).map_err(|_| V3LaunchError::MalformedCalldata)?;
            if call.abi_encode().as_slice() != input
                || call.p.totalSupply == U256::ZERO
                || call.p.targetFdvWeth == U256::ZERO
                || [
                    &call.p.name,
                    &call.p.symbol,
                    &call.p.description,
                    &call.p.website,
                    &call.p.telegram,
                    &call.p.twitter,
                    &call.p.logoURI,
                    &call.p.tokenURI,
                ]
                .iter()
                .any(|value| value.len() > MAX_DYNAMIC_STRING_BYTES)
            {
                return Err(V3LaunchError::MalformedCalldata);
            }
            // Bow exposes tokenInitCodeHash(params, creator), but the checked-in
            // evidence does not pin enough local bytecode/constructor semantics
            // to reproduce it without RPC. Fail closed before receipt.
            return Ok(LaunchCallObservation {
                launchpad: LaunchpadId::Bow,
                factory: destination,
                deployer,
                transaction_value,
                embedded_initial_buy: transaction_value != U256::ZERO,
                leader_min_tokens_out: call.p.devBuyMinTokens,
                predicted_market: None,
                execution_ready: false,
            });
        }
        if destination == LAUNCHHOOD_V3_FACTORY {
            let call =
                launchTokenCall::abi_decode(input).map_err(|_| V3LaunchError::MalformedCalldata)?;
            if call.abi_encode().as_slice() != input {
                return Err(V3LaunchError::MalformedCalldata);
            }
            if call.configId != U256::from(LAUNCHHOOD_CONFIG_ID)
                || call.p.name.is_empty()
                || call.p.symbol.is_empty()
                || [&call.p.name, &call.p.symbol, &call.p.metadataURI]
                    .iter()
                    .any(|value| value.len() > MAX_DYNAMIC_STRING_BYTES)
                || deployer == Address::ZERO
                || (transaction_value != U256::ZERO && call.minTokensOut == U256::ZERO)
            {
                return Err(V3LaunchError::UnsupportedLaunchConfig);
            }
            return Ok(LaunchCallObservation {
                launchpad: LaunchpadId::LaunchHoodV3,
                factory: destination,
                deployer,
                transaction_value,
                embedded_initial_buy: transaction_value != U256::ZERO,
                leader_min_tokens_out: call.minTokensOut,
                // The artifacts prove CREATE2 is used, but do not pin the
                // normalized init-code formula/hash or canonical dexId.
                predicted_market: None,
                execution_ready: false,
            });
        }
        Err(V3LaunchError::UnknownDispatch)
    }

    pub fn pre_receipt_market(
        &self,
        observation: &LaunchCallObservation,
    ) -> Result<LaunchMarket, V3LaunchError> {
        observation
            .predicted_market
            .clone()
            .ok_or(V3LaunchError::PlanUnavailable)
    }

    /// Construct a paper plan only after asynchronous reconciliation has
    /// admitted a market into warm state. Receipt decoding remains gated until
    /// exact protocol event evidence is checked in.
    pub fn paper_plan(&self, input: FollowerPlanInput) -> Result<FollowerTradePlan, V3LaunchError> {
        if input.market.token == Address::ZERO
            || input.market.quote_asset != WETH
            || input.market.fee != V3_FEE
            || input.market.restriction_state != MarketRestrictionState::Clear
            || input.market.pool != canonical_pool(input.market.token)
            || input.recipient == Address::ZERO
            || input.spend_limit == U256::ZERO
            || input.min_receive == U256::ZERO
            || input.min_receive > input.locally_quoted_receive
        {
            return Err(V3LaunchError::UnsafeFollowerPlan);
        }
        let intent = V3ExactInputIntent {
            token_in: WETH,
            token_out: input.market.token,
            fee: V3_FEE,
            recipient: input.recipient,
            amount_in: input.spend_limit,
            amount_out_minimum: input.min_receive,
            sqrt_price_limit_x96: U256::ZERO,
        };
        let calldata =
            encode_v3_exact_input_single(&intent).ok_or(V3LaunchError::UnsafeFollowerPlan)?;
        Ok(FollowerTradePlan {
            launchpad: input.market.launchpad,
            route: RouteKind::V3SingleHop,
            destination: UNISWAP_V3_SWAP_ROUTER_02,
            value: U256::ZERO,
            calldata: calldata.into(),
            spend_limit: input.spend_limit,
            min_receive: input.min_receive,
            expected_market: MarketIdentity {
                token: input.market.token,
                quote_asset: input.market.quote_asset,
                pool: input.market.pool,
            },
        })
    }
}

fn canonical_pool(token: Address) -> Address {
    let (token0, token1) = if token < WETH {
        (token, WETH)
    } else {
        (WETH, token)
    };
    predict_v3_pool_address(
        UNISWAP_V3_FACTORY,
        token0,
        token1,
        V3_FEE,
        UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pins() -> Vec<ContractCodeSnapshot> {
        vec![
            ContractCodeSnapshot {
                address: WETH,
                runtime_code_hash: WETH_RUNTIME_KECCAK256,
            },
            ContractCodeSnapshot {
                address: UNISWAP_V3_FACTORY,
                runtime_code_hash: UNISWAP_V3_FACTORY_RUNTIME_KECCAK256,
            },
            ContractCodeSnapshot {
                address: UNISWAP_V3_POSITION_MANAGER,
                runtime_code_hash: UNISWAP_V3_POSITION_MANAGER_RUNTIME_KECCAK256,
            },
            ContractCodeSnapshot {
                address: UNISWAP_V3_SWAP_ROUTER_02,
                runtime_code_hash: UNISWAP_V3_SWAP_ROUTER_02_RUNTIME_KECCAK256,
            },
        ]
    }

    fn registry() -> V3LaunchAtBirthAdapter {
        V3LaunchAtBirthAdapter::new(CHAIN_ID, &pins()).unwrap()
    }

    fn launchhood_calldata(min_tokens_out: U256) -> Vec<u8> {
        launchTokenCall {
            p: LaunchHoodTokenParams {
                name: "fixture".into(),
                symbol: "FIX".into(),
                metadataURI: "ipfs://fixture".into(),
                rewardRecipient: Address::with_last_byte(9),
            },
            configId: U256::ZERO,
            dexId: U256::ZERO,
            userSalt: B256::with_last_byte(7),
            minTokensOut: min_tokens_out,
        }
        .abi_encode()
    }

    fn bow_calldata() -> Vec<u8> {
        launchCall {
            p: BowLaunchParams {
                name: "fixture".into(),
                symbol: "BOW".into(),
                totalSupply: U256::from(1_000_000_000_u64),
                launchDelay: U256::ZERO,
                maxWallet: U256::from(200),
                limitWindow: U256::from(366),
                targetFdvWeth: U256::from(10),
                salt: B256::with_last_byte(3),
                description: "fixture".into(),
                website: "".into(),
                telegram: "".into(),
                twitter: "".into(),
                logoURI: "".into(),
                tokenURI: "".into(),
                devBuyMinTokens: U256::from(777),
            },
        }
        .abi_encode()
    }

    #[test]
    fn positive_call_fixtures_separate_observation_from_prediction() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/v3_launch_at_birth.json"))
                .unwrap();
        assert_eq!(
            hex::encode(launchCall::SELECTOR),
            fixture["bow"]["selector"].as_str().unwrap()
        );
        assert_eq!(
            hex::encode(launchTokenCall::SELECTOR),
            fixture["launchhood_v3"]["selector"].as_str().unwrap()
        );
        let r = registry();
        let leader = Address::with_last_byte(4);
        let bow = r
            .observe_launch_call(
                CHAIN_ID,
                BOW_LAUNCH_FACTORY,
                leader,
                U256::from(10),
                &bow_calldata(),
            )
            .unwrap();
        assert_eq!(bow.launchpad, LaunchpadId::Bow);
        assert!(bow.predicted_market.is_none());
        assert_eq!(bow.leader_min_tokens_out, U256::from(777));

        let hood = r
            .observe_launch_call(
                CHAIN_ID,
                LAUNCHHOOD_V3_FACTORY,
                leader,
                U256::from(10),
                &launchhood_calldata(U256::from(888)),
            )
            .unwrap();
        assert_eq!(hood.launchpad, LaunchpadId::LaunchHoodV3);
        assert!(hood.predicted_market.is_none());
        assert!(!hood.execution_ready);
        assert_eq!(hood.leader_min_tokens_out, U256::from(888));
        assert_eq!(
            r.pre_receipt_market(&hood),
            Err(V3LaunchError::PlanUnavailable)
        );
    }

    #[test]
    fn cross_adapter_wrong_destination_and_malformed_calldata_fail_closed() {
        let r = registry();
        let leader = Address::with_last_byte(4);
        assert_eq!(
            r.observe_launch_call(
                CHAIN_ID,
                BOW_LAUNCH_FACTORY,
                leader,
                U256::ZERO,
                &launchhood_calldata(U256::ZERO)
            ),
            Err(V3LaunchError::MalformedCalldata)
        );
        assert_eq!(
            r.observe_launch_call(
                CHAIN_ID,
                LAUNCHHOOD_V3_FACTORY,
                leader,
                U256::ZERO,
                &bow_calldata()
            ),
            Err(V3LaunchError::MalformedCalldata)
        );
        let mut malformed = launchhood_calldata(U256::ZERO);
        malformed.pop();
        assert_eq!(
            r.observe_launch_call(
                CHAIN_ID,
                LAUNCHHOOD_V3_FACTORY,
                leader,
                U256::ZERO,
                &malformed
            ),
            Err(V3LaunchError::MalformedCalldata)
        );
        assert_eq!(
            r.observe_launch_call(
                CHAIN_ID,
                Address::with_last_byte(1),
                leader,
                U256::ZERO,
                &bow_calldata()
            ),
            Err(V3LaunchError::UnknownDispatch)
        );
    }

    #[test]
    fn chain_and_code_identity_mismatch_fail_closed() {
        assert_eq!(
            V3LaunchAtBirthAdapter::new(8453, &pins()).unwrap_err(),
            V3LaunchError::WrongChain
        );
        let mut bad = pins();
        bad[0].runtime_code_hash = B256::ZERO;
        assert_eq!(
            V3LaunchAtBirthAdapter::new(CHAIN_ID, &bad).unwrap_err(),
            V3LaunchError::CodeIdentity
        );
        assert_eq!(
            registry().observe_launch_call(
                8453,
                BOW_LAUNCH_FACTORY,
                Address::with_last_byte(1),
                U256::ZERO,
                &bow_calldata()
            ),
            Err(V3LaunchError::WrongChain)
        );
    }

    #[test]
    fn paper_plan_uses_follower_minimum_never_leader_minimum() {
        let r = registry();
        let observation = r
            .observe_launch_call(
                CHAIN_ID,
                LAUNCHHOOD_V3_FACTORY,
                Address::with_last_byte(4),
                U256::from(10),
                &launchhood_calldata(U256::from(888)),
            )
            .unwrap();
        let token = Address::with_last_byte(8);
        let market = LaunchMarket {
            launchpad: LaunchpadId::LaunchHoodV3,
            token,
            pool: canonical_pool(token),
            quote_asset: WETH,
            fee: V3_FEE,
            restriction_state: MarketRestrictionState::Clear,
        };
        let plan = r
            .paper_plan(FollowerPlanInput {
                market,
                recipient: Address::with_last_byte(5),
                spend_limit: U256::from(100),
                locally_quoted_receive: U256::from(600),
                min_receive: U256::from(555),
            })
            .unwrap();
        assert_eq!(plan.min_receive, U256::from(555));
        assert_ne!(plan.min_receive, observation.leader_min_tokens_out);
        let decoded = crate::noxa_abi::decode_v3_exact_input_single(&plan.calldata).unwrap();
        assert_eq!(decoded.amount_out_minimum, U256::from(555));
        assert_eq!(plan.value, U256::ZERO);
    }

    #[test]
    fn unsafe_paper_plan_fails_closed() {
        let r = registry();
        let token = Address::with_last_byte(8);
        let market = LaunchMarket {
            launchpad: LaunchpadId::Bow,
            token,
            pool: canonical_pool(token),
            quote_asset: WETH,
            fee: V3_FEE,
            restriction_state: MarketRestrictionState::Clear,
        };
        assert_eq!(
            r.paper_plan(FollowerPlanInput {
                market,
                recipient: Address::with_last_byte(5),
                spend_limit: U256::from(100),
                locally_quoted_receive: U256::from(500),
                min_receive: U256::from(501)
            }),
            Err(V3LaunchError::UnsafeFollowerPlan)
        );

        let restricted = LaunchMarket {
            launchpad: LaunchpadId::LaunchHoodV3,
            token,
            pool: canonical_pool(token),
            quote_asset: WETH,
            fee: V3_FEE,
            restriction_state: MarketRestrictionState::Unknown,
        };
        assert_eq!(
            r.paper_plan(FollowerPlanInput {
                market: restricted,
                recipient: Address::with_last_byte(5),
                spend_limit: U256::from(100),
                locally_quoted_receive: U256::from(500),
                min_receive: U256::from(400)
            }),
            Err(V3LaunchError::UnsafeFollowerPlan)
        );
    }

    #[test]
    fn noncanonical_oversized_and_unsupported_config_calls_fail_closed() {
        let r = registry();
        let leader = Address::with_last_byte(4);
        let mut trailing = bow_calldata();
        trailing.extend_from_slice(&[0_u8; 32]);
        assert_eq!(
            r.observe_launch_call(CHAIN_ID, BOW_LAUNCH_FACTORY, leader, U256::ZERO, &trailing),
            Err(V3LaunchError::MalformedCalldata)
        );
        let oversized = vec![0_u8; MAX_CALLDATA_BYTES + 1];
        assert_eq!(
            r.observe_launch_call(CHAIN_ID, BOW_LAUNCH_FACTORY, leader, U256::ZERO, &oversized),
            Err(V3LaunchError::MalformedCalldata)
        );
        let unsupported = launchTokenCall {
            p: LaunchHoodTokenParams {
                name: "fixture".into(),
                symbol: "FIX".into(),
                metadataURI: "".into(),
                rewardRecipient: leader,
            },
            configId: U256::from(1),
            dexId: U256::ZERO,
            userSalt: B256::ZERO,
            minTokensOut: U256::ZERO,
        }
        .abi_encode();
        assert_eq!(
            r.observe_launch_call(
                CHAIN_ID,
                LAUNCHHOOD_V3_FACTORY,
                leader,
                U256::ZERO,
                &unsupported
            ),
            Err(V3LaunchError::UnsupportedLaunchConfig)
        );
    }
}
