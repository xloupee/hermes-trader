use std::collections::{HashMap, HashSet};

use alloy_primitives::{Address, B256};
use thiserror::Error;

use crate::launchpad_adapter::{AdapterKind, LaunchpadId, RouteKind, WrapperKind};
use crate::noxa_abi::{AGGREGATOR_SWAP_SELECTOR, EXACT_INPUT_SINGLE_SELECTOR};
use crate::robinhood::{
    ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256, ACTIVE_NOXA_LAUNCH_FACTORY, CHAIN_ID,
    ROBINHOOD_SWAP_AGGREGATOR, UNISWAP_V3_FACTORY, UNISWAP_V3_FACTORY_RUNTIME_KECCAK256,
    UNISWAP_V3_SWAP_ROUTER_02, UNISWAP_V3_SWAP_ROUTER_02_RUNTIME_KECCAK256, WETH,
};

pub const MAX_WRAPPER_DEPTH: usize = 2;
pub const MAX_INNER_CALLS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DispatchKey {
    pub destination: Address,
    pub selector: [u8; 4],
    pub wrapper: WrapperKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContractRole {
    LaunchFactory,
    V3Factory,
    Router,
    Aggregator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContractPin {
    pub role: ContractRole,
    pub address: Address,
    pub implementation: Option<Address>,
    pub runtime_code_hash: B256,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchpadSpec {
    pub id: LaunchpadId,
    pub chain_id: u64,
    pub family: AdapterKind,
    pub observation_keys: Vec<DispatchKey>,
    pub contract_pins: Vec<ContractPin>,
    pub allowed_routes: Vec<RouteKind>,
    pub quote_assets: Vec<Address>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupPinSnapshot {
    pub chain_id: u64,
    pub pins: Vec<ContractPin>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicAggregatorPin {
    pub implementation: Address,
    pub runtime_code_hash: B256,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedInnerCalls<'a> {
    calls: Vec<BoundedCall<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedCall<'a> {
    pub destination: Address,
    pub calldata: &'a [u8],
    pub wrapper: WrapperKind,
    pub depth: usize,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum RegistryError {
    #[error("registry only supports Robinhood chain ID 4663")]
    WrongChain,
    #[error("a required startup contract/code pin is missing or drifted")]
    PinMismatch,
    #[error("registry contains an ambiguous destination/selector/wrapper key")]
    AmbiguousDispatch,
    #[error("candidate has no registered destination/selector/wrapper key")]
    UnknownDispatch,
    #[error("wrapper depth or inner call count exceeds its fixed bound")]
    WrapperBounds,
    #[error("candidate calldata has no selector")]
    MalformedCalldata,
}

#[derive(Debug, Clone)]
pub struct StaticLaunchpadRegistry {
    specs: Vec<LaunchpadSpec>,
    dispatch: HashMap<DispatchKey, usize>,
}

impl<'a> BoundedInnerCalls<'a> {
    pub fn new(calls: Vec<BoundedCall<'a>>) -> Result<Self, RegistryError> {
        if calls.is_empty()
            || calls.len() > MAX_INNER_CALLS
            || calls.iter().any(|call| call.depth > MAX_WRAPPER_DEPTH)
        {
            return Err(RegistryError::WrapperBounds);
        }
        Ok(Self { calls })
    }

    pub fn calls(&self) -> &[BoundedCall<'a>] {
        &self.calls
    }
}

impl StaticLaunchpadRegistry {
    /// Build the chain-4663 registry from an already-resolved startup snapshot.
    /// This function performs no I/O; callers must finish RPC pinning before it.
    pub fn noxa(
        startup: StartupPinSnapshot,
        aggregator: DynamicAggregatorPin,
    ) -> Result<Self, RegistryError> {
        if aggregator.implementation == Address::ZERO || aggregator.runtime_code_hash == B256::ZERO
        {
            return Err(RegistryError::PinMismatch);
        }
        let spec = noxa_spec(aggregator);
        Self::build(startup, vec![spec])
    }

    fn build(
        startup: StartupPinSnapshot,
        specs: Vec<LaunchpadSpec>,
    ) -> Result<Self, RegistryError> {
        if startup.chain_id != CHAIN_ID
            || specs.is_empty()
            || specs.iter().any(|spec| spec.chain_id != CHAIN_ID)
        {
            return Err(RegistryError::WrongChain);
        }
        let actual: HashSet<ContractPin> = startup.pins.into_iter().collect();
        if specs
            .iter()
            .flat_map(|spec| spec.contract_pins.iter())
            .any(|pin| !actual.contains(pin))
        {
            return Err(RegistryError::PinMismatch);
        }
        let mut dispatch = HashMap::new();
        for (index, spec) in specs.iter().enumerate() {
            for key in &spec.observation_keys {
                if dispatch.insert(*key, index).is_some() {
                    return Err(RegistryError::AmbiguousDispatch);
                }
            }
        }
        Ok(Self { specs, dispatch })
    }

    pub fn dispatch(
        &self,
        chain_id: Option<u64>,
        call: BoundedCall<'_>,
    ) -> Result<&LaunchpadSpec, RegistryError> {
        if chain_id != Some(CHAIN_ID) {
            return Err(RegistryError::WrongChain);
        }
        if call.depth > MAX_WRAPPER_DEPTH {
            return Err(RegistryError::WrapperBounds);
        }
        let selector: [u8; 4] = call
            .calldata
            .get(..4)
            .ok_or(RegistryError::MalformedCalldata)?
            .try_into()
            .expect("four-byte slice");
        let index = self
            .dispatch
            .get(&DispatchKey {
                destination: call.destination,
                selector,
                wrapper: call.wrapper,
            })
            .ok_or(RegistryError::UnknownDispatch)?;
        Ok(&self.specs[*index])
    }

    pub fn specs(&self) -> &[LaunchpadSpec] {
        &self.specs
    }
}

fn noxa_spec(aggregator: DynamicAggregatorPin) -> LaunchpadSpec {
    LaunchpadSpec {
        id: LaunchpadId::Noxa,
        chain_id: CHAIN_ID,
        family: AdapterKind::V3LaunchAtBirth,
        observation_keys: vec![
            DispatchKey {
                destination: UNISWAP_V3_SWAP_ROUTER_02,
                selector: EXACT_INPUT_SINGLE_SELECTOR,
                wrapper: WrapperKind::Direct,
            },
            DispatchKey {
                destination: ROBINHOOD_SWAP_AGGREGATOR,
                selector: AGGREGATOR_SWAP_SELECTOR,
                wrapper: WrapperKind::Direct,
            },
        ],
        contract_pins: vec![
            ContractPin {
                role: ContractRole::LaunchFactory,
                address: ACTIVE_NOXA_LAUNCH_FACTORY,
                implementation: None,
                runtime_code_hash: ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256,
            },
            ContractPin {
                role: ContractRole::V3Factory,
                address: UNISWAP_V3_FACTORY,
                implementation: None,
                runtime_code_hash: UNISWAP_V3_FACTORY_RUNTIME_KECCAK256,
            },
            ContractPin {
                role: ContractRole::Router,
                address: UNISWAP_V3_SWAP_ROUTER_02,
                implementation: None,
                runtime_code_hash: UNISWAP_V3_SWAP_ROUTER_02_RUNTIME_KECCAK256,
            },
            ContractPin {
                role: ContractRole::Aggregator,
                address: ROBINHOOD_SWAP_AGGREGATOR,
                implementation: Some(aggregator.implementation),
                runtime_code_hash: aggregator.runtime_code_hash,
            },
        ],
        allowed_routes: vec![RouteKind::V3SingleHop],
        quote_assets: vec![WETH],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aggregator() -> DynamicAggregatorPin {
        DynamicAggregatorPin {
            implementation: Address::with_last_byte(0xa1),
            runtime_code_hash: B256::with_last_byte(0xa2),
        }
    }

    fn startup() -> StartupPinSnapshot {
        let spec = noxa_spec(aggregator());
        StartupPinSnapshot {
            chain_id: CHAIN_ID,
            pins: spec.contract_pins,
        }
    }

    #[test]
    fn accepts_only_chain_4663_and_exact_destination_selector() {
        let registry = StaticLaunchpadRegistry::noxa(startup(), aggregator()).unwrap();
        let calldata = EXACT_INPUT_SINGLE_SELECTOR;
        let call = BoundedCall {
            destination: UNISWAP_V3_SWAP_ROUTER_02,
            calldata: &calldata,
            wrapper: WrapperKind::Direct,
            depth: 0,
        };
        assert_eq!(
            registry.dispatch(Some(CHAIN_ID), call).unwrap().id,
            LaunchpadId::Noxa
        );
        assert_eq!(
            registry.dispatch(Some(8_453), call),
            Err(RegistryError::WrongChain)
        );
        assert_eq!(
            registry.dispatch(None, call),
            Err(RegistryError::WrongChain)
        );
        assert_eq!(
            registry.dispatch(
                Some(CHAIN_ID),
                BoundedCall {
                    destination: Address::with_last_byte(7),
                    ..call
                }
            ),
            Err(RegistryError::UnknownDispatch)
        );
    }

    #[test]
    fn startup_fails_closed_on_code_or_address_lookalikes() {
        let mut drifted = startup();
        drifted.pins[0].runtime_code_hash = B256::with_last_byte(9);
        assert!(matches!(
            StaticLaunchpadRegistry::noxa(drifted, aggregator()),
            Err(RegistryError::PinMismatch)
        ));
        let mut lookalike = startup();
        lookalike.pins[2].address = Address::with_last_byte(9);
        assert!(matches!(
            StaticLaunchpadRegistry::noxa(lookalike, aggregator()),
            Err(RegistryError::PinMismatch)
        ));
    }

    #[test]
    fn ambiguity_is_rejected_when_building_the_snapshot() {
        let spec = noxa_spec(aggregator());
        let startup = StartupPinSnapshot {
            chain_id: CHAIN_ID,
            pins: spec.contract_pins.clone(),
        };
        assert!(matches!(
            StaticLaunchpadRegistry::build(startup, vec![spec.clone(), spec]),
            Err(RegistryError::AmbiguousDispatch)
        ));
    }

    #[test]
    fn wrapper_representation_is_bounded_and_unknown_shapes_fail_closed() {
        let calldata = EXACT_INPUT_SINGLE_SELECTOR;
        let calls = vec![BoundedCall {
            destination: UNISWAP_V3_SWAP_ROUTER_02,
            calldata: &calldata,
            wrapper: WrapperKind::Multicall,
            depth: 1,
        }];
        assert!(BoundedInnerCalls::new(calls).is_ok());
        assert_eq!(
            BoundedInnerCalls::new(vec![BoundedCall {
                destination: UNISWAP_V3_SWAP_ROUTER_02,
                calldata: &calldata,
                wrapper: WrapperKind::Erc4337,
                depth: MAX_WRAPPER_DEPTH + 1,
            }]),
            Err(RegistryError::WrapperBounds)
        );
        let too_many = (0..=MAX_INNER_CALLS)
            .map(|_| BoundedCall {
                destination: UNISWAP_V3_SWAP_ROUTER_02,
                calldata: calldata.as_slice(),
                wrapper: WrapperKind::Multicall,
                depth: 1,
            })
            .collect();
        assert_eq!(
            BoundedInnerCalls::new(too_many),
            Err(RegistryError::WrapperBounds)
        );
    }

    #[test]
    fn candidate_dispatch_has_no_io_dependency() {
        fn assert_candidate_api_is_pure(
            dispatch: for<'a, 'b> fn(
                &'a StaticLaunchpadRegistry,
                Option<u64>,
                BoundedCall<'b>,
            ) -> Result<&'a LaunchpadSpec, RegistryError>,
        ) {
            let _ = dispatch;
        }
        assert_candidate_api_is_pure(StaticLaunchpadRegistry::dispatch);
    }
}
