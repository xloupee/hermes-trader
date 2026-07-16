//! Static chain-4663 launchpad dispatch and narrow adapter observations.
//!
//! The research snapshot does not contain enough PoolManager, hook, Permit2,
//! Doppler, Klik, or Trench runtime pins to enable execution. This module keeps
//! those absences explicit: callers may install startup-validated code pins,
//! but candidate processing performs only immutable comparisons and pure ABI
//! decoding.

use alloy_primitives::{Address, B256, U256, keccak256};
use alloy_sol_types::{SolCall, sol};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::robinhood::CHAIN_ID;
use crate::uniswap_v4::{
    FollowerV4Policy, V4Error, V4MarketSnapshot, V4PaperPlan, WarmV4Quote, build_follower_v4_plan,
};

pub const CLANKER_FACTORY: Address =
    alloy_primitives::address!("d3f2cc1731b7fd17f28798835c2e02f0a1839a94");
pub const CLANKER_FACTORY_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("f895112a2deed34ba2765d0147aff3494104a28293cc2f19af9275934088da33");
pub const CLANKER_DEPLOY_SELECTOR: [u8; 4] = [0xdf, 0x40, 0x22, 0x4a];
pub const CLANKER_TOKEN_CREATED_TOPIC: B256 =
    alloy_primitives::b256!("9299d1d1a88d8e1abdc591ae7a167a6bc63a8f17d695804e9091ee33aa89fb67");
pub const CLANKER_LOCKER: Address =
    alloy_primitives::address!("290f735f63824bb5836cde24a35f5103a5b5bc99");

pub const V4_POOL_MANAGER: Address =
    alloy_primitives::address!("8366a39cc670b4001a1121b8f6a443a643e40951");

pub const DOPPLER_CREATE_EMITTER: Address =
    alloy_primitives::address!("eb7c034704ef8dcd2d32324c1545f62fb4ad0862");
pub const DOPPLER_CREATE_TOPIC: B256 =
    alloy_primitives::b256!("68ff1cfcdcf76864161555fc0de1878d8f83ec6949bf351df74d8a4a1a2679ab");

pub const KLIK_FACTORY: Address =
    alloy_primitives::address!("16cf6788b762ee8969744586ed16fc5705140dd7");
pub const KLIK_DEPLOY_SELECTOR: [u8; 4] = [0x41, 0x01, 0x65, 0x9e];
pub const KLIK_TOKEN_CREATED_TOPIC: B256 =
    alloy_primitives::b256!("60122e78030aba0a2e4a67adb3e52b411343cc51778f919095d3fe394090c1b2");

pub const TRENCH_PROXY: Address =
    alloy_primitives::address!("77dc6f6361b7b99456fc3761ce5b7dda80d83f9d");
pub const TRENCH_IMPLEMENTATION: Address =
    alloy_primitives::address!("6d0ff368db6cf9c94a182ad2375e640ec71acee9");
pub const TRENCH_LAUNCH_SELECTOR: [u8; 4] = [0xf3, 0x9d, 0xc3, 0xed];
pub const TRENCH_UNPROVEN_SELECTOR_A: [u8; 4] = [0x2c, 0xe7, 0xa0, 0xfa];
pub const TRENCH_UNPROVEN_SELECTOR_B: [u8; 4] = [0xae, 0x87, 0xc3, 0x97];

const MAX_DISCOVERY_CALLDATA: usize = 128 * 1024;

sol! {
    function deployCoin(
        string name,
        string symbol,
        string metadata,
        bytes32 salt,
        uint256 initialBuy
    ) external payable;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchpadId {
    Clanker,
    BankrDoppler,
    KlikFinance,
    TrenchToday,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributionSource {
    Virtuals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    /// Observation plus a pure follower paper plan is structurally available,
    /// but signed execution remains outside this module.
    PaperOnly,
    /// More startup pins or protocol semantics are required even for planning.
    ExecutionGated,
    /// Launch discovery only; no trade direction or quote semantics are known.
    DiscoveryOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct RuntimeCodePin {
    pub address: Address,
    pub runtime_code_hash: B256,
}

impl RuntimeCodePin {
    fn complete(self) -> bool {
        self.address != Address::ZERO && self.runtime_code_hash != B256::ZERO
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchEntry {
    pub launchpad: LaunchpadId,
    pub destination: RuntimeCodePin,
    pub selector: [u8; 4],
    pub implementation: Option<RuntimeCodePin>,
    pub mode: ExecutionMode,
}

#[derive(Debug, Clone, Default)]
pub struct ResearchStartupPins {
    /// Klik's address is evidenced, but its runtime hash was not. Omit this
    /// until startup has independently validated the exact chain-4663 hash.
    pub klik_factory: Option<RuntimeCodePin>,
    /// Trench is an EIP-1967 proxy. Both proxy and point-in-time implementation
    /// must be independently code-hash pinned before discovery is admitted.
    pub trench_proxy: Option<RuntimeCodePin>,
    pub trench_implementation: Option<RuntimeCodePin>,
    /// Direct Bankr/Doppler call shapes were not recovered in the artifacts.
    /// Each admitted form therefore requires an explicit destination, selector,
    /// and code hash supplied by off-path startup validation.
    pub bankr_doppler_calls: Vec<(RuntimeCodePin, [u8; 4])>,
}

#[derive(Debug, Clone)]
pub struct StaticLaunchpadRegistry {
    entries: Vec<DispatchEntry>,
}

impl StaticLaunchpadRegistry {
    pub fn from_research(pins: ResearchStartupPins) -> Result<Self, AdapterError> {
        let mut entries = vec![DispatchEntry {
            launchpad: LaunchpadId::Clanker,
            destination: RuntimeCodePin {
                address: CLANKER_FACTORY,
                runtime_code_hash: CLANKER_FACTORY_RUNTIME_HASH,
            },
            selector: CLANKER_DEPLOY_SELECTOR,
            implementation: None,
            // Clanker deploy discovery is pinned, but token-specific hook,
            // PoolManager, locker, extension, router, and Permit2 pins are not.
            mode: ExecutionMode::ExecutionGated,
        }];

        if let Some(pin) = pins.klik_factory {
            if pin.address != KLIK_FACTORY || !pin.complete() {
                return Err(AdapterError::InvalidStartupPin);
            }
            entries.push(DispatchEntry {
                launchpad: LaunchpadId::KlikFinance,
                destination: pin,
                selector: KLIK_DEPLOY_SELECTOR,
                implementation: None,
                mode: ExecutionMode::DiscoveryOnly,
            });
        }

        match (pins.trench_proxy, pins.trench_implementation) {
            (None, None) => {}
            (Some(proxy), Some(implementation))
                if proxy.address == TRENCH_PROXY
                    && implementation.address == TRENCH_IMPLEMENTATION
                    && proxy.complete()
                    && implementation.complete() =>
            {
                entries.push(DispatchEntry {
                    launchpad: LaunchpadId::TrenchToday,
                    destination: proxy,
                    selector: TRENCH_LAUNCH_SELECTOR,
                    implementation: Some(implementation),
                    mode: ExecutionMode::DiscoveryOnly,
                });
            }
            _ => return Err(AdapterError::InvalidStartupPin),
        }

        for (destination, selector) in pins.bankr_doppler_calls {
            if !destination.complete() {
                return Err(AdapterError::InvalidStartupPin);
            }
            entries.push(DispatchEntry {
                launchpad: LaunchpadId::BankrDoppler,
                destination,
                selector,
                implementation: None,
                mode: ExecutionMode::ExecutionGated,
            });
        }
        if entries.iter().any(|entry| !entry.destination.complete()) {
            return Err(AdapterError::InvalidStartupPin);
        }
        Ok(Self { entries })
    }

    #[cfg(test)]
    fn from_entries(entries: Vec<DispatchEntry>) -> Result<Self, AdapterError> {
        if entries.iter().any(|entry| !entry.destination.complete()) {
            return Err(AdapterError::InvalidStartupPin);
        }
        Ok(Self { entries })
    }

    pub fn dispatch(&self, candidate: &CandidateCall<'_>) -> Result<DispatchEntry, AdapterError> {
        if candidate.chain_id != CHAIN_ID {
            return Err(AdapterError::WrongChain);
        }
        let selector: [u8; 4] = candidate
            .input
            .get(..4)
            .ok_or(AdapterError::MalformedCalldata)?
            .try_into()
            .expect("four-byte slice");
        let matches = self
            .entries
            .iter()
            .filter(|entry| {
                entry.destination.address == candidate.destination
                    && entry.destination.runtime_code_hash == candidate.destination_runtime_hash
                    && entry.selector == selector
                    && match (entry.implementation, candidate.implementation) {
                        (None, None) => true,
                        (Some(expected), Some(observed)) => expected == observed,
                        _ => false,
                    }
            })
            .copied()
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => Err(AdapterError::NoMatch),
            [entry] => Ok(*entry),
            _ => Err(AdapterError::Ambiguous),
        }
    }

    pub fn observe(
        &self,
        candidate: &CandidateCall<'_>,
    ) -> Result<LaunchObservation, AdapterError> {
        let entry = self.dispatch(candidate)?;
        match entry.launchpad {
            LaunchpadId::Clanker => {
                validate_opaque_abi(candidate.input, CLANKER_DEPLOY_SELECTOR)?;
                Ok(LaunchObservation::OpaqueLaunch {
                    launchpad: LaunchpadId::Clanker,
                    leader: candidate.leader,
                    calldata_hash: keccak256(candidate.input),
                    value: candidate.value,
                    mode: entry.mode,
                })
            }
            LaunchpadId::BankrDoppler => {
                validate_opaque_abi(candidate.input, entry.selector)?;
                Ok(LaunchObservation::OpaqueLaunch {
                    launchpad: LaunchpadId::BankrDoppler,
                    leader: candidate.leader,
                    calldata_hash: keccak256(candidate.input),
                    value: candidate.value,
                    mode: entry.mode,
                })
            }
            LaunchpadId::KlikFinance => {
                let call = deployCoinCall::abi_decode(candidate.input)
                    .map_err(|_| AdapterError::MalformedCalldata)?;
                if call.abi_encode().as_slice() != candidate.input {
                    return Err(AdapterError::MalformedCalldata);
                }
                Ok(LaunchObservation::KlikLaunch {
                    leader: candidate.leader,
                    name: call.name,
                    symbol: call.symbol,
                    metadata: call.metadata,
                    salt: call.salt,
                    observed_initial_buy: call.initialBuy,
                    value: candidate.value,
                })
            }
            LaunchpadId::TrenchToday => {
                validate_opaque_abi(candidate.input, TRENCH_LAUNCH_SELECTOR)?;
                Ok(LaunchObservation::OpaqueLaunch {
                    launchpad: LaunchpadId::TrenchToday,
                    leader: candidate.leader,
                    calldata_hash: keccak256(candidate.input),
                    value: candidate.value,
                    mode: ExecutionMode::DiscoveryOnly,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CandidateCall<'a> {
    pub chain_id: u64,
    pub leader: Address,
    pub destination: Address,
    pub destination_runtime_hash: B256,
    pub implementation: Option<RuntimeCodePin>,
    pub value: U256,
    pub input: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LaunchObservation {
    OpaqueLaunch {
        launchpad: LaunchpadId,
        leader: Address,
        calldata_hash: B256,
        value: U256,
        mode: ExecutionMode,
    },
    KlikLaunch {
        leader: Address,
        name: String,
        symbol: String,
        metadata: String,
        salt: B256,
        observed_initial_buy: U256,
        value: U256,
    },
}

impl LaunchObservation {
    pub fn planning_mode(&self) -> ExecutionMode {
        match self {
            Self::OpaqueLaunch { mode, .. } => *mode,
            Self::KlikLaunch { .. } => ExecutionMode::DiscoveryOnly,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ObservedV4Action {
    pub launchpad: LaunchpadId,
    pub attribution: Option<AttributionSource>,
    pub leader: Address,
    pub pool_id: B256,
    pub asset_in: Address,
    pub asset_out: Address,
    pub observed_amount_in: U256,
    pub observed_min_out: U256,
    /// Audit-only fingerprint. It is intentionally absent from follower plans.
    pub observed_route_hash: B256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedAdapterMarket {
    launchpad: LaunchpadId,
    pool_id: B256,
}

#[derive(Debug, Clone, Copy)]
pub struct V4ActionObservationInput<'a> {
    pub launchpad: LaunchpadId,
    pub attribution: Option<AttributionSource>,
    pub leader: Address,
    pub asset_in: Address,
    pub asset_out: Address,
    pub observed_amount_in: U256,
    pub observed_min_out: U256,
    pub observed_route: &'a [u8],
}

pub fn normalize_v4_action(
    market: &V4MarketSnapshot,
    input: V4ActionObservationInput<'_>,
) -> Result<ObservedV4Action, AdapterError> {
    if input.attribution.is_some() && input.launchpad != LaunchpadId::BankrDoppler {
        return Err(AdapterError::InvalidAttribution);
    }
    if input.leader == Address::ZERO
        || input.asset_in == input.asset_out
        || !market.key.contains(input.asset_in)
        || !market.key.contains(input.asset_out)
        || input.observed_amount_in == U256::ZERO
    {
        return Err(AdapterError::InvalidObservation);
    }
    Ok(ObservedV4Action {
        launchpad: input.launchpad,
        attribution: input.attribution,
        leader: input.leader,
        pool_id: market.pool_id,
        asset_in: input.asset_in,
        asset_out: input.asset_out,
        observed_amount_in: input.observed_amount_in,
        observed_min_out: input.observed_min_out,
        observed_route_hash: keccak256(input.observed_route),
    })
}

pub fn build_adapter_paper_plan(
    validated: ValidatedAdapterMarket,
    action: &ObservedV4Action,
    market: &V4MarketSnapshot,
    quote: WarmV4Quote,
    policy: FollowerV4Policy,
) -> Result<V4PaperPlan, AdapterError> {
    match action.launchpad {
        LaunchpadId::KlikFinance | LaunchpadId::TrenchToday => {
            return Err(AdapterError::ExecutionGated);
        }
        LaunchpadId::Clanker | LaunchpadId::BankrDoppler => {}
    }
    if validated.launchpad != action.launchpad
        || validated.pool_id != market.pool_id
        || action.pool_id != market.pool_id
        || action.asset_in != quote.asset_in
        || action.asset_out != quote.asset_out
    {
        return Err(AdapterError::InvalidObservation);
    }
    // No leader amount, min-out, route hash, value, hook bytes, deadline, or
    // permit material is passed to the shared follower planner.
    build_follower_v4_plan(market, V4_POOL_MANAGER, quote, policy).map_err(AdapterError::V4)
}

pub fn validate_clanker_market(
    factory: RuntimeCodePin,
    locker: RuntimeCodePin,
    market: &V4MarketSnapshot,
) -> Result<ValidatedAdapterMarket, AdapterError> {
    if factory.address != CLANKER_FACTORY
        || factory.runtime_code_hash != CLANKER_FACTORY_RUNTIME_HASH
        || locker.address != CLANKER_LOCKER
        || !locker.complete()
    {
        return Err(AdapterError::InvalidStartupPin);
    }
    market.validate(V4_POOL_MANAGER).map_err(AdapterError::V4)?;
    Ok(ValidatedAdapterMarket {
        launchpad: LaunchpadId::Clanker,
        pool_id: market.pool_id,
    })
}

pub fn validate_doppler_market(
    emitter: RuntimeCodePin,
    market: &V4MarketSnapshot,
) -> Result<ValidatedAdapterMarket, AdapterError> {
    if emitter.address != DOPPLER_CREATE_EMITTER || !emitter.complete() {
        return Err(AdapterError::InvalidStartupPin);
    }
    market.validate(V4_POOL_MANAGER).map_err(AdapterError::V4)?;
    Ok(ValidatedAdapterMarket {
        launchpad: LaunchpadId::BankrDoppler,
        pool_id: market.pool_id,
    })
}

fn validate_opaque_abi(input: &[u8], selector: [u8; 4]) -> Result<(), AdapterError> {
    if input.len() < 4
        || input.len() > MAX_DISCOVERY_CALLDATA
        || input[..4] != selector
        || !(input.len() - 4).is_multiple_of(32)
    {
        return Err(AdapterError::MalformedCalldata);
    }
    Ok(())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AdapterError {
    #[error("startup runtime or implementation pin is invalid")]
    InvalidStartupPin,
    #[error("candidate is not Robinhood Chain mainnet")]
    WrongChain,
    #[error("candidate does not match an exact pinned adapter")]
    NoMatch,
    #[error("candidate matches more than one adapter")]
    Ambiguous,
    #[error("candidate calldata is malformed or non-canonical")]
    MalformedCalldata,
    #[error("attribution is not valid for this protocol")]
    InvalidAttribution,
    #[error("normalized observation does not match the pinned market")]
    InvalidObservation,
    #[error("protocol execution is gated by missing evidence")]
    ExecutionGated,
    #[error(transparent)]
    V4(#[from] V4Error),
}

#[cfg(test)]
mod tests {
    use alloy_primitives::address;

    use super::*;
    use crate::robinhood::WETH;
    use crate::uniswap_v4::{CodePin, DYNAMIC_FEE_FLAG, HookPin, V4FeePolicy, V4PoolKey};

    const TOKEN: Address = address!("6bbbb3be7424a911d5d131e272639512c1c12b07");
    const HOOK: Address = address!("0000000000000000000000000000000000000042");

    fn hash(byte: u8) -> B256 {
        B256::repeat_byte(byte)
    }

    fn pin(address: Address, byte: u8) -> RuntimeCodePin {
        RuntimeCodePin {
            address,
            runtime_code_hash: hash(byte),
        }
    }

    fn research_registry() -> StaticLaunchpadRegistry {
        StaticLaunchpadRegistry::from_research(ResearchStartupPins {
            klik_factory: Some(pin(KLIK_FACTORY, 0x22)),
            trench_proxy: Some(pin(TRENCH_PROXY, 0x33)),
            trench_implementation: Some(pin(TRENCH_IMPLEMENTATION, 0x44)),
            bankr_doppler_calls: vec![(pin(Address::with_last_byte(0x55), 0x55), [1, 2, 3, 4])],
        })
        .unwrap()
    }

    fn candidate<'a>(
        destination: Address,
        runtime_hash: B256,
        implementation: Option<RuntimeCodePin>,
        input: &'a [u8],
    ) -> CandidateCall<'a> {
        CandidateCall {
            chain_id: CHAIN_ID,
            leader: Address::with_last_byte(9),
            destination,
            destination_runtime_hash: runtime_hash,
            implementation,
            value: U256::from(30_000_000_000_000_000_u64),
            input,
        }
    }

    fn market() -> V4MarketSnapshot {
        let key = V4PoolKey::canonical(WETH, TOKEN, DYNAMIC_FEE_FLAG, 60, HOOK).unwrap();
        V4MarketSnapshot {
            chain_id: CHAIN_ID,
            pool_manager: CodePin {
                address: V4_POOL_MANAGER,
                runtime_code_hash: hash(0x66),
            },
            key,
            pool_id: key.pool_id(),
            hook: HookPin {
                code: CodePin {
                    address: HOOK,
                    runtime_code_hash: hash(0x77),
                },
                configuration_hash: hash(0x88),
            },
            quote_asset: WETH,
            fee_policy: V4FeePolicy::Dynamic {
                min_fee_ppm: 1_000,
                max_fee_ppm: 10_000,
            },
            state_version: 12,
        }
    }

    #[test]
    fn observes_exact_clanker_and_rejects_lookalikes() {
        let registry = research_registry();
        let input = [CLANKER_DEPLOY_SELECTOR.as_slice(), &[0_u8; 32]].concat();
        let exact = candidate(CLANKER_FACTORY, CLANKER_FACTORY_RUNTIME_HASH, None, &input);
        assert!(matches!(
            registry.observe(&exact),
            Ok(LaunchObservation::OpaqueLaunch {
                launchpad: LaunchpadId::Clanker,
                mode: ExecutionMode::ExecutionGated,
                ..
            })
        ));

        let wrong_address = candidate(
            Address::with_last_byte(0x94),
            CLANKER_FACTORY_RUNTIME_HASH,
            None,
            &input,
        );
        assert_eq!(registry.observe(&wrong_address), Err(AdapterError::NoMatch));
        let wrong_hash = candidate(CLANKER_FACTORY, hash(1), None, &input);
        assert_eq!(registry.observe(&wrong_hash), Err(AdapterError::NoMatch));
    }

    #[test]
    fn klik_strictly_decodes_but_is_discovery_only() {
        let registry = research_registry();
        let input = deployCoinCall {
            name: "Klik".to_owned(),
            symbol: "KLIK".to_owned(),
            metadata: "ipfs://fixture".to_owned(),
            salt: hash(7),
            initialBuy: U256::from(42),
        }
        .abi_encode();
        let call = candidate(KLIK_FACTORY, hash(0x22), None, &input);
        let observation = registry.observe(&call).unwrap();
        assert_eq!(observation.planning_mode(), ExecutionMode::DiscoveryOnly);
        let mut malformed = input;
        malformed.push(0);
        let call = candidate(KLIK_FACTORY, hash(0x22), None, &malformed);
        assert_eq!(
            registry.observe(&call),
            Err(AdapterError::MalformedCalldata)
        );
    }

    #[test]
    fn trench_is_opaque_discovery_and_unproven_trade_selectors_never_dispatch() {
        let registry = research_registry();
        let input = [TRENCH_LAUNCH_SELECTOR.as_slice(), &[0_u8; 32]].concat();
        let call = candidate(
            TRENCH_PROXY,
            hash(0x33),
            Some(pin(TRENCH_IMPLEMENTATION, 0x44)),
            &input,
        );
        assert!(matches!(
            registry.observe(&call),
            Ok(LaunchObservation::OpaqueLaunch {
                launchpad: LaunchpadId::TrenchToday,
                mode: ExecutionMode::DiscoveryOnly,
                ..
            })
        ));

        for selector in [TRENCH_UNPROVEN_SELECTOR_A, TRENCH_UNPROVEN_SELECTOR_B] {
            let input = [selector.as_slice(), &[0_u8; 32]].concat();
            let call = candidate(
                TRENCH_PROXY,
                hash(0x33),
                Some(pin(TRENCH_IMPLEMENTATION, 0x44)),
                &input,
            );
            assert_eq!(registry.observe(&call), Err(AdapterError::NoMatch));
        }
    }

    #[test]
    fn rejects_wrong_chain_proxy_implementation_and_registry_ambiguity() {
        let registry = research_registry();
        let input = [TRENCH_LAUNCH_SELECTOR.as_slice(), &[0_u8; 32]].concat();
        let mut call = candidate(
            TRENCH_PROXY,
            hash(0x33),
            Some(pin(TRENCH_IMPLEMENTATION, 0x44)),
            &input,
        );
        call.chain_id = 8453;
        assert_eq!(registry.observe(&call), Err(AdapterError::WrongChain));
        call.chain_id = CHAIN_ID;
        call.implementation = Some(pin(TRENCH_IMPLEMENTATION, 0x45));
        assert_eq!(registry.observe(&call), Err(AdapterError::NoMatch));

        let entry = DispatchEntry {
            launchpad: LaunchpadId::Clanker,
            destination: pin(Address::with_last_byte(1), 1),
            selector: [1, 2, 3, 4],
            implementation: None,
            mode: ExecutionMode::ExecutionGated,
        };
        let ambiguous = StaticLaunchpadRegistry::from_entries(vec![entry, entry]).unwrap();
        let input = [entry.selector.as_slice(), &[0_u8; 32]].concat();
        let call = candidate(entry.destination.address, hash(1), None, &input);
        assert_eq!(ambiguous.dispatch(&call), Err(AdapterError::Ambiguous));
    }

    #[test]
    fn virtuals_is_only_bankr_attribution() {
        let market = market();
        assert_eq!(
            normalize_v4_action(
                &market,
                V4ActionObservationInput {
                    launchpad: LaunchpadId::Clanker,
                    attribution: Some(AttributionSource::Virtuals),
                    leader: Address::with_last_byte(1),
                    asset_in: WETH,
                    asset_out: TOKEN,
                    observed_amount_in: U256::from(10),
                    observed_min_out: U256::from(1),
                    observed_route: &[1, 2, 3],
                },
            ),
            Err(AdapterError::InvalidAttribution)
        );
        let bankr = normalize_v4_action(
            &market,
            V4ActionObservationInput {
                launchpad: LaunchpadId::BankrDoppler,
                attribution: Some(AttributionSource::Virtuals),
                leader: Address::with_last_byte(1),
                asset_in: WETH,
                asset_out: TOKEN,
                observed_amount_in: U256::from(10),
                observed_min_out: U256::from(1),
                observed_route: &[1, 2, 3],
            },
        )
        .unwrap();
        assert_eq!(bankr.launchpad, LaunchpadId::BankrDoppler);
        assert_eq!(bankr.attribution, Some(AttributionSource::Virtuals));
    }

    #[test]
    fn follower_plan_never_inherits_leader_min_out_or_route() {
        let market = market();
        let make_action = |min_out, route: &[u8]| {
            normalize_v4_action(
                &market,
                V4ActionObservationInput {
                    launchpad: LaunchpadId::Clanker,
                    attribution: None,
                    leader: Address::with_last_byte(1),
                    asset_in: WETH,
                    asset_out: TOKEN,
                    observed_amount_in: U256::from(999_999),
                    observed_min_out: min_out,
                    observed_route: route,
                },
            )
            .unwrap()
        };
        let quote = WarmV4Quote {
            pool_id: market.pool_id,
            state_version: market.state_version,
            asset_in: WETH,
            asset_out: TOKEN,
            amount_in: U256::from(100),
            expected_amount_out: U256::from(1_000),
            applied_fee_ppm: 5_000,
        };
        let policy = FollowerV4Policy {
            recipient: Address::with_last_byte(9),
            spend_limit: U256::from(100),
            max_slippage_bps: 100,
        };
        let validated = validate_clanker_market(
            RuntimeCodePin {
                address: CLANKER_FACTORY,
                runtime_code_hash: CLANKER_FACTORY_RUNTIME_HASH,
            },
            pin(CLANKER_LOCKER, 0x99),
            &market,
        )
        .unwrap();
        let first = build_adapter_paper_plan(
            validated,
            &make_action(U256::from(1), &[0xaa]),
            &market,
            quote,
            policy,
        )
        .unwrap();
        let second = build_adapter_paper_plan(
            validated,
            &make_action(U256::MAX, &[0xbb, 0xcc]),
            &market,
            quote,
            policy,
        )
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.min_receive, U256::from(990));
    }

    #[test]
    fn cross_adapter_destinations_do_not_inherit_selectors() {
        let registry = research_registry();
        let clanker_input = [CLANKER_DEPLOY_SELECTOR.as_slice(), &[0_u8; 32]].concat();
        let at_klik = candidate(KLIK_FACTORY, hash(0x22), None, &clanker_input);
        assert_eq!(registry.observe(&at_klik), Err(AdapterError::NoMatch));

        let klik_input = deployCoinCall {
            name: "x".into(),
            symbol: "x".into(),
            metadata: "x".into(),
            salt: hash(1),
            initialBuy: U256::ZERO,
        }
        .abi_encode();
        let at_clanker = candidate(
            CLANKER_FACTORY,
            CLANKER_FACTORY_RUNTIME_HASH,
            None,
            &klik_input,
        );
        assert_eq!(registry.observe(&at_clanker), Err(AdapterError::NoMatch));
    }
}
