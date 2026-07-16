//! Unified, broadcast-free launchpad observation path for decoded Nitro transactions.
//!
//! All code identities are supplied at construction. Candidate handling is synchronous and
//! contains no RPC, filesystem, signing, submission, or control-plane capability.

use std::collections::HashSet;

use alloy_consensus::{Transaction, TxEnvelope, transaction::SignerRecoverable};
use alloy_primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::PonsAdapter;
use crate::decoder::{DecodeError, DecodeReport, FeedDecoder, Filter};
use crate::feed::BroadcastMessage;
use crate::flap_identity::{
    FLAP_PORTAL_IMPLEMENTATION, FLAP_PORTAL_IMPLEMENTATION_RUNTIME_KECCAK256, FLAP_PORTAL_PROXY,
    FLAP_PORTAL_PROXY_RUNTIME_KECCAK256, FLAP_VAULT_PORTAL_IMPLEMENTATION,
    FLAP_VAULT_PORTAL_IMPLEMENTATION_RUNTIME_KECCAK256, FLAP_VAULT_PORTAL_PROXY,
    FLAP_VAULT_PORTAL_PROXY_RUNTIME_KECCAK256,
};
use crate::launchpad_adapter::{
    AdapterKind, AttributionSource, LaunchpadId, RouteKind, WrapperKind,
};
use crate::launchpad_adapters::{
    DispatchEntry, ExecutionMode, ResearchStartupPins, RuntimeCodePin, V4AdapterSet,
    V4CandidateCall,
};
use crate::launchpad_registry::{
    BoundedCall, ContractPin, ContractRole, DispatchKey, LaunchpadSpec, ObservedContractPin,
    StartupPinSnapshot, StaticLaunchpadRegistry,
};
use crate::noxa_abi::{LAUNCH_TOKEN_SELECTOR, decode_launch_call};
use crate::pons::{
    PONS_CURRENT_FACTORY, PONS_LAUNCH_SELECTOR, PONS_LEGACY_FACTORY, RuntimeIdentity,
};
use crate::robinhood::{
    ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256, ACTIVE_NOXA_LAUNCH_FACTORY, BOW_LAUNCH_FACTORY,
    CHAIN_ID, LAUNCHHOOD_V3_FACTORY, NOXA_FACTORY_RUNTIME_KECCAK256, NOXA_LAUNCH_FACTORY,
    UNISWAP_V3_FACTORY, UNISWAP_V3_FACTORY_RUNTIME_KECCAK256, UNISWAP_V3_POSITION_MANAGER,
    UNISWAP_V3_POSITION_MANAGER_RUNTIME_KECCAK256, UNISWAP_V3_SWAP_ROUTER_02,
    UNISWAP_V3_SWAP_ROUTER_02_RUNTIME_KECCAK256, WETH, WETH_RUNTIME_KECCAK256,
};
use crate::smart_account::{
    AccountExecutionProfile, ContractPin as SmartContractPin, ENTRY_POINT_V07,
    ENTRY_POINT_V07_HANDLE_OPS_SELECTOR, EntryPointCall, OwnedValidatedSmartAccountPins,
    SmartAccountPin, decode_entry_point_v07_prevalidated,
};
use crate::tier2_curve::{
    CurveCandidateCall, HOOD_BUY_SELECTOR, HOOD_CREATE_SELECTOR, HOOD_FACTORY, HOOD_SELL_SELECTOR,
    LEAVEHOOD_BUY_SELECTOR, LEAVEHOOD_CORE_IMPLEMENTATION, LEAVEHOOD_CORE_PROXY,
    LEAVEHOOD_FACTORY_IMPLEMENTATION, LEAVEHOOD_FACTORY_PROXY, LEAVEHOOD_LAUNCH_SELECTORS,
    LEAVEHOOD_SELL_SELECTOR, LEAVEHOOD_SELL_WITH_SLIPPAGE_SELECTOR, RuntimePin as CurveRuntimePin,
    StartupPins as CurveStartupPins, Tier2CurveAdapter,
};
use crate::v3_launch_at_birth::{
    BOW_LAUNCH_SELECTOR, ContractCodeSnapshot, LAUNCHHOOD_V3_LAUNCH_SELECTOR,
    V3LaunchAtBirthAdapter,
};

pub const FLAP_STANDARD_LAUNCH_SELECTOR: [u8; 4] = [0x2e, 0x2f, 0xdb, 0xd9];
pub const FLAP_TAX_V3_LAUNCH_SELECTOR: [u8; 4] = [0x8c, 0xb5, 0x77, 0x2c];
const MAX_OPAQUE_CALLDATA: usize = 128 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationReachability {
    RawFeed,
    ReceiptOnly,
    StartupPinRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaunchpadCapability {
    pub launchpad: LaunchpadId,
    pub attribution_sources: Vec<AttributionSource>,
    pub observation: ObservationReachability,
    pub discovery_enabled: bool,
    pub paper_plan_supported: bool,
    pub live_execution_enabled: bool,
    pub wrappers: Vec<WrapperKind>,
    pub blockers: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaderOrigin {
    DirectSigner,
    Erc4337Sender,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaperLaunchpadObservation {
    pub tx_hash: B256,
    pub launchpad: LaunchpadId,
    pub leader: Address,
    pub outer_signer: Address,
    pub leader_origin: LeaderOrigin,
    pub wrapper: WrapperKind,
    pub kind: &'static str,
    pub planning_mode: ExecutionMode,
    pub live_execution_enabled: bool,
    pub detail: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct PaperFeedRejection {
    pub tx_hash: B256,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct PaperFeedReport {
    pub decode: PaperDecodeSummary,
    pub observations: Vec<PaperLaunchpadObservation>,
    pub rejections: Vec<PaperFeedRejection>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PaperDecodeSummary {
    pub messages: usize,
    pub signed_transactions: usize,
    pub envelope_decode_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedPinsProvenance {
    ReviewedProtocolPins,
    SyntheticOfflineFixture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedPinsProvenance {
    StartupObservation,
    SyntheticOfflineFixture,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaperExpectedPins {
    pub schema_version: u32,
    pub document_role: ExpectedPinsDocumentRole,
    pub provenance: ExpectedPinsProvenance,
    pub fixture_id: Option<String>,
    pub bow_factory_runtime_hash: B256,
    pub launchhood_v3_factory_runtime_hash: B256,
    pub hood_factory_runtime_hash: Option<B256>,
    pub leavehood_factory_proxy_runtime_hash: Option<B256>,
    pub leavehood_factory_implementation_runtime_hash: Option<B256>,
    pub leavehood_core_proxy_runtime_hash: Option<B256>,
    pub leavehood_core_implementation_runtime_hash: Option<B256>,
    pub klik_factory_runtime_hash: Option<B256>,
    pub trench_proxy_runtime_hash: Option<B256>,
    pub trench_implementation_runtime_hash: Option<B256>,
    #[serde(default)]
    pub bankr_doppler_calls: Vec<ConfiguredCallPin>,
    pub erc4337: Option<ConfiguredSmartAccounts>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedPinsDocumentRole {
    ExpectedProtocolPins,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PaperObservedStartupSnapshot {
    pub schema_version: u32,
    pub document_role: ObservedPinsDocumentRole,
    pub provenance: ObservedPinsProvenance,
    pub fixture_id: Option<String>,
    pub chain_id: u64,
    pub pins: Vec<ObservedRuntimePin>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedPinsDocumentRole {
    ObservedStartupSnapshot,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedRuntimePin {
    pub address: Address,
    pub implementation: Option<Address>,
    pub runtime_hash: B256,
    #[serde(default)]
    pub code_bytes: Option<usize>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ConfiguredCallPin {
    pub destination: Address,
    pub runtime_hash: B256,
    pub selector: [u8; 4],
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfiguredSmartAccounts {
    pub entry_point_runtime_hash: B256,
    #[serde(default)]
    pub accounts: Vec<ConfiguredSmartAccount>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct ConfiguredSmartAccount {
    pub account: Address,
    pub runtime_hash: B256,
    pub execution_profile: AccountExecutionProfile,
    pub factory: Option<Address>,
    pub factory_runtime_hash: Option<B256>,
    pub delegation_implementation: Option<Address>,
    pub delegation_runtime_hash: Option<B256>,
}

#[derive(Debug, Error)]
pub enum PaperObserverError {
    #[error("transaction is not a chain-4663 signed call")]
    WrongChain,
    #[error("transaction destination and selector are not registered")]
    UnknownDispatch,
    #[error("registered adapter rejected candidate: {0}")]
    Adapter(String),
    #[error("candidate signer recovery failed: {0}")]
    Signer(String),
    #[error("wrapper rejected candidate: {0}")]
    Wrapper(String),
    #[error("registry has an ambiguous production dispatch key")]
    AmbiguousDispatch,
    #[error("startup pin snapshot is invalid: {0}")]
    Startup(String),
}

/// One immutable production dispatch registry for every paper observation adapter.
pub struct PaperLaunchpadObserver {
    registry: StaticLaunchpadRegistry,
    v3: V3LaunchAtBirthAdapter,
    pons: PonsAdapter,
    curves: Tier2CurveAdapter,
    smart_accounts: Option<OwnedValidatedSmartAccountPins>,
}

/// End-to-end Nitro paper runtime. It intentionally exposes no executor, signer, or RPC client.
pub struct PaperFeedRuntime {
    decoder: FeedDecoder,
    observer: PaperLaunchpadObserver,
}

impl PaperFeedRuntime {
    pub fn new(observer: PaperLaunchpadObserver) -> Self {
        Self {
            decoder: FeedDecoder::new(Filter::default()),
            observer,
        }
    }

    pub fn capabilities(&self) -> Vec<LaunchpadCapability> {
        self.observer.capabilities()
    }

    pub fn decode(&mut self, feed: &BroadcastMessage) -> Result<PaperFeedReport, DecodeError> {
        let mut observations = Vec::new();
        let mut rejections = Vec::new();
        let report: DecodeReport = self.decoder.decode_with(feed, |context| {
            match self.observer.observe_transaction(context.transaction) {
                Ok(Some(observation)) => observations.push(observation),
                Ok(None) => {}
                Err(error) => rejections.push(PaperFeedRejection {
                    tx_hash: *context.transaction.tx_hash(),
                    reason: error.to_string(),
                }),
            }
        })?;
        Ok(PaperFeedReport {
            decode: PaperDecodeSummary {
                messages: report.messages,
                signed_transactions: report.signed_transactions,
                envelope_decode_ns: report.envelope_decode_ns,
            },
            observations,
            rejections,
        })
    }
}

impl PaperLaunchpadObserver {
    pub fn from_startup_snapshots(
        expected: PaperExpectedPins,
        observed: PaperObservedStartupSnapshot,
    ) -> Result<Self, PaperObserverError> {
        validate_document_pair(&expected, &observed)?;
        validate_observed_pins(&observed)?;
        let observed_code = |address| ContractCodeSnapshot {
            address,
            runtime_code_hash: find_observed_pin(&observed.pins, address, None)
                .map_or(B256::ZERO, |pin| pin.runtime_hash),
        };
        let v3 = V3LaunchAtBirthAdapter::new(
            CHAIN_ID,
            &[
                observed_code(WETH),
                observed_code(UNISWAP_V3_FACTORY),
                observed_code(UNISWAP_V3_POSITION_MANAGER),
                observed_code(UNISWAP_V3_SWAP_ROUTER_02),
            ],
        )
        .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        let pin = |address, runtime_code_hash| RuntimeCodePin {
            address,
            runtime_code_hash,
        };
        let v4 = V4AdapterSet::from_research(ResearchStartupPins {
            klik_factory: expected
                .klik_factory_runtime_hash
                .map(|hash| pin(crate::launchpad_adapters::KLIK_FACTORY, hash)),
            trench_proxy: expected
                .trench_proxy_runtime_hash
                .map(|hash| pin(crate::launchpad_adapters::TRENCH_PROXY, hash)),
            trench_implementation: expected
                .trench_implementation_runtime_hash
                .map(|hash| pin(crate::launchpad_adapters::TRENCH_IMPLEMENTATION, hash)),
            bankr_doppler_calls: expected
                .bankr_doppler_calls
                .iter()
                .map(|call| (pin(call.destination, call.runtime_hash), call.selector))
                .collect(),
        })
        .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        let curve_pin = |address,
                         implementation,
                         expected_hash: Option<B256>,
                         expected_implementation_hash: Option<B256>| {
            let observed_contract = find_observed_pin(&observed.pins, address, implementation);
            CurveRuntimePin {
                address,
                implementation,
                runtime_code_hash: expected_hash.unwrap_or(B256::ZERO),
                observed_runtime_code_hash: observed_contract
                    .map_or(B256::ZERO, |pin| pin.runtime_hash),
                implementation_runtime_code_hash: expected_implementation_hash,
                observed_implementation_runtime_code_hash: implementation.and_then(|address| {
                    find_observed_pin(&observed.pins, address, None).map(|pin| pin.runtime_hash)
                }),
            }
        };
        let curves = Tier2CurveAdapter::new(CurveStartupPins {
            chain_id: CHAIN_ID,
            hood_factory: curve_pin(HOOD_FACTORY, None, expected.hood_factory_runtime_hash, None),
            leavehood_factory: curve_pin(
                LEAVEHOOD_FACTORY_PROXY,
                Some(LEAVEHOOD_FACTORY_IMPLEMENTATION),
                expected.leavehood_factory_proxy_runtime_hash,
                expected.leavehood_factory_implementation_runtime_hash,
            ),
            leavehood_core: curve_pin(
                LEAVEHOOD_CORE_PROXY,
                Some(LEAVEHOOD_CORE_IMPLEMENTATION),
                expected.leavehood_core_proxy_runtime_hash,
                expected.leavehood_core_implementation_runtime_hash,
            ),
            v3_factory: curve_pin(
                UNISWAP_V3_FACTORY,
                None,
                Some(UNISWAP_V3_FACTORY_RUNTIME_KECCAK256),
                None,
            ),
            v3_router: curve_pin(
                UNISWAP_V3_SWAP_ROUTER_02,
                None,
                Some(UNISWAP_V3_SWAP_ROUTER_02_RUNTIME_KECCAK256),
                None,
            ),
        })
        .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        let pons_identities = PonsAdapter::required_startup_identities()
            .iter()
            .filter_map(|required| {
                find_observed_pin(&observed.pins, required.address, None).and_then(|pin| {
                    pin.code_bytes.map(|code_bytes| RuntimeIdentity {
                        address: pin.address,
                        code_bytes,
                        runtime_hash: pin.runtime_hash,
                    })
                })
            })
            .collect::<Vec<_>>();
        let pons = PonsAdapter::from_startup_identities(&pons_identities)
            .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        let wrappers = if expected.erc4337.is_some() {
            vec![WrapperKind::Direct, WrapperKind::Erc4337]
        } else {
            vec![WrapperKind::Direct]
        };
        let specs = paper_specs(&expected, &v4, &wrappers)?;
        let registry = StaticLaunchpadRegistry::from_specs(
            StartupPinSnapshot {
                chain_id: observed.chain_id,
                pins: observed
                    .pins
                    .iter()
                    .map(|pin| ObservedContractPin {
                        address: pin.address,
                        implementation: pin.implementation,
                        runtime_code_hash: pin.runtime_hash,
                    })
                    .collect(),
            },
            specs,
        )
        .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        let allowed_targets = registry_erc4337_targets(&registry)?;
        let smart_accounts = expected.erc4337.map(|smart| {
            let entry_point = SmartContractPin {
                address: ENTRY_POINT_V07,
                runtime_code_hash: smart.entry_point_runtime_hash,
            };
            let accounts = smart
                .accounts
                .into_iter()
                .map(configured_smart_account_pin)
                .collect::<Result<Vec<_>, _>>()?;
            validate_smart_account_pins(entry_point, &accounts, &observed.pins)?;
            OwnedValidatedSmartAccountPins::new(entry_point, accounts, allowed_targets)
                .map_err(|error| PaperObserverError::Startup(error.to_string()))
        });
        let smart_accounts = smart_accounts.transpose()?;
        Ok(Self {
            registry,
            v3,
            pons,
            curves,
            smart_accounts,
        })
    }

    /// Cheap prefilter. Calling this never recovers a signer or decodes ABI.
    pub fn might_observe(&self, destination: Address, input: &[u8]) -> bool {
        let Some(selector) = selector(input) else {
            return false;
        };
        self.registry
            .might_dispatch(destination, input, WrapperKind::Direct)
            || self.smart_accounts.as_ref().is_some_and(|pins| {
                destination == pins.entry_point().address
                    && selector == ENTRY_POINT_V07_HANDLE_OPS_SELECTOR
            })
    }

    pub fn observe_transaction(
        &self,
        transaction: &TxEnvelope,
    ) -> Result<Option<PaperLaunchpadObservation>, PaperObserverError> {
        if transaction.chain_id() != Some(CHAIN_ID) {
            return Err(PaperObserverError::WrongChain);
        }
        let Some(destination) = transaction.to() else {
            return Ok(None);
        };
        if !self.might_observe(destination, transaction.input()) {
            return Ok(None);
        }
        let outer_signer = transaction
            .recover_signer()
            .map_err(|error| PaperObserverError::Signer(error.to_string()))?;
        if let Some(pins) = &self.smart_accounts
            && destination == pins.entry_point().address
        {
            let unwrapped = decode_entry_point_v07_prevalidated(
                EntryPointCall {
                    chain_id: CHAIN_ID,
                    destination: pins.entry_point(),
                    outer_bundler: outer_signer,
                    calldata: transaction.input(),
                },
                pins.validated(),
            )
            .map_err(|error| PaperObserverError::Wrapper(error.to_string()))?;
            return self
                .observe_call(
                    *transaction.tx_hash(),
                    unwrapped.leader,
                    outer_signer,
                    LeaderOrigin::Erc4337Sender,
                    WrapperKind::Erc4337,
                    unwrapped.target,
                    unwrapped.value,
                    &unwrapped.calldata,
                )
                .map(Some);
        }
        self.observe_call(
            *transaction.tx_hash(),
            outer_signer,
            outer_signer,
            LeaderOrigin::DirectSigner,
            WrapperKind::Direct,
            destination,
            transaction.value(),
            transaction.input(),
        )
        .map(Some)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn observe_call(
        &self,
        tx_hash: B256,
        leader: Address,
        outer_signer: Address,
        leader_origin: LeaderOrigin,
        wrapper: WrapperKind,
        destination: Address,
        value: U256,
        input: &[u8],
    ) -> Result<PaperLaunchpadObservation, PaperObserverError> {
        let spec = self
            .registry
            .dispatch(
                Some(CHAIN_ID),
                BoundedCall {
                    destination,
                    calldata: input,
                    wrapper,
                    depth: usize::from(wrapper != WrapperKind::Direct),
                },
            )
            .map_err(|_| PaperObserverError::UnknownDispatch)?;
        let (launchpad, kind, planning_mode, detail) = match (spec.id, spec.family) {
            (LaunchpadId::Noxa, AdapterKind::V3LaunchAtBirth) => {
                let intent = decode_launch_call(input, value)
                    .ok_or_else(|| PaperObserverError::Adapter("malformed Noxa launch".into()))?;
                (
                    LaunchpadId::Noxa,
                    "launch",
                    ExecutionMode::PaperOnly,
                    serde_json::json!({
                        "launch_config_id": intent.launch_config_id,
                        "dex_id": intent.dex_id,
                        "salt": intent.salt,
                        "observed_value": value,
                    }),
                )
            }
            (LaunchpadId::Bow | LaunchpadId::LaunchHoodV3, AdapterKind::V3LaunchAtBirth) => {
                let observed = self
                    .v3
                    .observe_launch_call(CHAIN_ID, destination, leader, value, input)
                    .map_err(|error| PaperObserverError::Adapter(error.to_string()))?;
                (
                    observed.launchpad,
                    "launch",
                    ExecutionMode::ExecutionGated,
                    serde_json::to_value(observed).expect("serializable V3 observation"),
                )
            }
            (
                LaunchpadId::Clanker
                | LaunchpadId::BankrDoppler
                | LaunchpadId::KlikFinance
                | LaunchpadId::TrenchToday,
                AdapterKind::UniswapV4 | AdapterKind::DopplerV4 | AdapterKind::NativeCurve,
            ) => {
                let selector = selector(input).ok_or(PaperObserverError::UnknownDispatch)?;
                let destination_pin = spec
                    .contract_pins
                    .iter()
                    .find(|pin| pin.address == destination)
                    .ok_or(PaperObserverError::UnknownDispatch)?;
                let implementation = destination_pin
                    .implementation
                    .map(|address| {
                        spec.contract_pins
                            .iter()
                            .find(|pin| pin.address == address)
                            .map(|pin| RuntimeCodePin {
                                address,
                                runtime_code_hash: pin.runtime_code_hash,
                            })
                            .ok_or(PaperObserverError::UnknownDispatch)
                    })
                    .transpose()?;
                let entry = DispatchEntry {
                    launchpad: spec.id,
                    destination: RuntimeCodePin {
                        address: destination,
                        runtime_code_hash: destination_pin.runtime_code_hash,
                    },
                    selector,
                    implementation,
                    mode: match spec.id {
                        LaunchpadId::KlikFinance | LaunchpadId::TrenchToday => {
                            ExecutionMode::DiscoveryOnly
                        }
                        _ => ExecutionMode::ExecutionGated,
                    },
                };
                let observed = V4AdapterSet::observe_resolved(
                    entry,
                    &V4CandidateCall {
                        chain_id: CHAIN_ID,
                        leader,
                        destination,
                        destination_runtime_hash: destination_pin.runtime_code_hash,
                        implementation,
                        value,
                        input,
                    },
                )
                .map_err(|error| PaperObserverError::Adapter(error.to_string()))?;
                let launchpad = match &observed {
                    crate::launchpad_adapters::LaunchObservation::OpaqueLaunch {
                        launchpad,
                        ..
                    } => *launchpad,
                    crate::launchpad_adapters::LaunchObservation::KlikLaunch { .. } => {
                        LaunchpadId::KlikFinance
                    }
                };
                let mode = observed.planning_mode();
                (
                    launchpad,
                    "discovery",
                    mode,
                    serde_json::to_value(observed).expect("serializable V4 observation"),
                )
            }
            (LaunchpadId::Pons, AdapterKind::V3LaunchAtBirth) => {
                let runtime_hash = if destination == PONS_CURRENT_FACTORY {
                    crate::pons::PONS_CURRENT_FACTORY_RUNTIME
                } else if destination == PONS_LEGACY_FACTORY {
                    crate::pons::PONS_LEGACY_FACTORY_RUNTIME
                } else {
                    return Err(PaperObserverError::UnknownDispatch);
                };
                let observed = self
                    .pons
                    .observe_launch(crate::pons::PonsObservationInput {
                        tx_hash,
                        chain_id: CHAIN_ID,
                        destination,
                        destination_runtime_hash: runtime_hash,
                        calldata: input,
                        value,
                        sender: leader,
                        provenance: crate::pons::PonsAttributionProvenance::ExactFactoryTransaction,
                    })
                    .map_err(|error| PaperObserverError::Adapter(error.to_string()))?;
                (
                    LaunchpadId::Pons,
                    "launch",
                    ExecutionMode::ExecutionGated,
                    serde_json::to_value(observed).expect("serializable Pons observation"),
                )
            }
            (LaunchpadId::HoodFun | LaunchpadId::LeaveHood, AdapterKind::NativeCurve) => {
                let observed = self
                    .curves
                    .observe(
                        CurveCandidateCall {
                            chain_id: CHAIN_ID,
                            destination,
                            input,
                            value,
                        },
                        &[],
                    )
                    .map_err(|error| PaperObserverError::Adapter(error.to_string()))?;
                (
                    observed.protocol,
                    "observation",
                    if observed.paper_plan_supported {
                        ExecutionMode::PaperOnly
                    } else {
                        ExecutionMode::DiscoveryOnly
                    },
                    serde_json::to_value(observed).expect("serializable curve observation"),
                )
            }
            (LaunchpadId::Flap, AdapterKind::FlapPortal) => {
                if input.len() > MAX_OPAQUE_CALLDATA || input.len() < 4 + 32 {
                    return Err(PaperObserverError::Adapter(
                        "malformed Flap launch envelope".into(),
                    ));
                }
                (
                    LaunchpadId::Flap,
                    "launch_discovery",
                    ExecutionMode::DiscoveryOnly,
                    serde_json::json!({"destination": destination, "selector": selector(input)}),
                )
            }
            _ => return Err(PaperObserverError::UnknownDispatch),
        };
        Ok(PaperLaunchpadObservation {
            tx_hash,
            launchpad,
            leader,
            outer_signer,
            leader_origin,
            wrapper,
            kind,
            planning_mode,
            live_execution_enabled: false,
            detail,
        })
    }

    pub fn capabilities(&self) -> Vec<LaunchpadCapability> {
        let erc = self.smart_accounts.is_some();
        let enabled = |launchpad| {
            self.registry
                .specs()
                .iter()
                .any(|spec| spec.id == launchpad)
        };
        let wrappers = |erc| {
            if erc {
                vec![WrapperKind::Direct, WrapperKind::Erc4337]
            } else {
                vec![WrapperKind::Direct]
            }
        };
        vec![
            capability(
                LaunchpadId::Noxa,
                enabled(LaunchpadId::Noxa),
                true,
                wrappers(false),
                vec!["unified fanout is paper-only; signed Noxa path remains separate"],
            ),
            capability(
                LaunchpadId::Bow,
                enabled(LaunchpadId::Bow),
                true,
                wrappers(false),
                vec!["receipt-free token prediction incomplete"],
            ),
            capability(
                LaunchpadId::LaunchHoodV3,
                enabled(LaunchpadId::LaunchHoodV3),
                true,
                wrappers(false),
                vec!["receipt-free token prediction incomplete"],
            ),
            capability(
                LaunchpadId::Clanker,
                enabled(LaunchpadId::Clanker),
                true,
                wrappers(erc),
                vec!["hook, locker, extension, and router pins incomplete"],
            ),
            capability(
                LaunchpadId::BankrDoppler,
                enabled(LaunchpadId::BankrDoppler),
                true,
                wrappers(erc),
                vec![
                    "startup call pins and Permit2 semantics required; Virtuals is attribution only",
                ],
            ),
            capability(
                LaunchpadId::KlikFinance,
                enabled(LaunchpadId::KlikFinance),
                false,
                wrappers(erc),
                vec!["factory runtime and hook semantics required"],
            ),
            capability(
                LaunchpadId::TrenchToday,
                enabled(LaunchpadId::TrenchToday),
                false,
                wrappers(erc),
                vec!["curve direction, sell, and graduation semantics incomplete"],
            ),
            capability(
                LaunchpadId::Pons,
                enabled(LaunchpadId::Pons),
                true,
                wrappers(false),
                crate::pons::PONS_EXECUTION_GAPS.to_vec(),
            ),
            capability(
                LaunchpadId::Flap,
                enabled(LaunchpadId::Flap),
                false,
                wrappers(false),
                vec!["raw call is discovery-only; receipt/profile required for normalization"],
            ),
            capability(
                LaunchpadId::HoodFun,
                enabled(LaunchpadId::HoodFun),
                true,
                wrappers(false),
                vec!["warm market and opportunity-quality evidence required"],
            ),
            capability(
                LaunchpadId::LeaveHood,
                enabled(LaunchpadId::LeaveHood),
                false,
                wrappers(false),
                vec!["curve, fee, sell, and migration semantics incomplete"],
            ),
        ]
    }
}

fn paper_specs(
    expected: &PaperExpectedPins,
    v4: &V4AdapterSet,
    v4_wrappers: &[WrapperKind],
) -> Result<Vec<LaunchpadSpec>, PaperObserverError> {
    let direct = [WrapperKind::Direct];
    let shared_v3_pins = || {
        vec![
            contract_pin(
                ContractRole::ProtocolDependency,
                WETH,
                None,
                WETH_RUNTIME_KECCAK256,
            ),
            contract_pin(
                ContractRole::V3Factory,
                UNISWAP_V3_FACTORY,
                None,
                UNISWAP_V3_FACTORY_RUNTIME_KECCAK256,
            ),
            contract_pin(
                ContractRole::ProtocolDependency,
                UNISWAP_V3_POSITION_MANAGER,
                None,
                UNISWAP_V3_POSITION_MANAGER_RUNTIME_KECCAK256,
            ),
            contract_pin(
                ContractRole::Router,
                UNISWAP_V3_SWAP_ROUTER_02,
                None,
                UNISWAP_V3_SWAP_ROUTER_02_RUNTIME_KECCAK256,
            ),
        ]
    };
    let with_shared_v3 = |mut pins: Vec<ContractPin>| {
        pins.extend(shared_v3_pins());
        pins
    };
    let pons_pins = PonsAdapter::required_startup_identities()
        .iter()
        .map(|identity| {
            contract_pin(
                if matches!(identity.address, PONS_LEGACY_FACTORY | PONS_CURRENT_FACTORY) {
                    ContractRole::LaunchFactory
                } else {
                    ContractRole::ProtocolDependency
                },
                identity.address,
                None,
                identity.runtime_hash,
            )
        })
        .collect();
    let mut specs = vec![
        spec(
            LaunchpadId::Noxa,
            AdapterKind::V3LaunchAtBirth,
            keys(
                &[
                    (NOXA_LAUNCH_FACTORY, LAUNCH_TOKEN_SELECTOR),
                    (ACTIVE_NOXA_LAUNCH_FACTORY, LAUNCH_TOKEN_SELECTOR),
                ],
                &direct,
            ),
            vec![
                contract_pin(
                    ContractRole::LaunchFactory,
                    NOXA_LAUNCH_FACTORY,
                    None,
                    NOXA_FACTORY_RUNTIME_KECCAK256,
                ),
                contract_pin(
                    ContractRole::LaunchFactory,
                    ACTIVE_NOXA_LAUNCH_FACTORY,
                    None,
                    ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256,
                ),
            ],
            RouteKind::V3SingleHop,
        ),
        spec(
            LaunchpadId::Bow,
            AdapterKind::V3LaunchAtBirth,
            keys(&[(BOW_LAUNCH_FACTORY, BOW_LAUNCH_SELECTOR)], &direct),
            with_shared_v3(vec![contract_pin(
                ContractRole::LaunchFactory,
                BOW_LAUNCH_FACTORY,
                None,
                expected.bow_factory_runtime_hash,
            )]),
            RouteKind::V3SingleHop,
        ),
        spec(
            LaunchpadId::LaunchHoodV3,
            AdapterKind::V3LaunchAtBirth,
            keys(
                &[(LAUNCHHOOD_V3_FACTORY, LAUNCHHOOD_V3_LAUNCH_SELECTOR)],
                &direct,
            ),
            with_shared_v3(vec![contract_pin(
                ContractRole::LaunchFactory,
                LAUNCHHOOD_V3_FACTORY,
                None,
                expected.launchhood_v3_factory_runtime_hash,
            )]),
            RouteKind::V3SingleHop,
        ),
        spec(
            LaunchpadId::Pons,
            AdapterKind::V3LaunchAtBirth,
            keys(
                &[
                    (PONS_LEGACY_FACTORY, PONS_LAUNCH_SELECTOR),
                    (PONS_CURRENT_FACTORY, PONS_LAUNCH_SELECTOR),
                ],
                &direct,
            ),
            pons_pins,
            RouteKind::V3SingleHop,
        ),
        spec(
            LaunchpadId::Flap,
            AdapterKind::FlapPortal,
            keys(
                &[
                    (FLAP_PORTAL_PROXY, FLAP_STANDARD_LAUNCH_SELECTOR),
                    (FLAP_PORTAL_PROXY, FLAP_TAX_V3_LAUNCH_SELECTOR),
                    (FLAP_VAULT_PORTAL_PROXY, FLAP_STANDARD_LAUNCH_SELECTOR),
                    (FLAP_VAULT_PORTAL_PROXY, FLAP_TAX_V3_LAUNCH_SELECTOR),
                ],
                &direct,
            ),
            vec![
                contract_pin(
                    ContractRole::LaunchFactory,
                    FLAP_PORTAL_PROXY,
                    Some(FLAP_PORTAL_IMPLEMENTATION),
                    FLAP_PORTAL_PROXY_RUNTIME_KECCAK256,
                ),
                contract_pin(
                    ContractRole::Implementation,
                    FLAP_PORTAL_IMPLEMENTATION,
                    None,
                    FLAP_PORTAL_IMPLEMENTATION_RUNTIME_KECCAK256,
                ),
                contract_pin(
                    ContractRole::LaunchFactory,
                    FLAP_VAULT_PORTAL_PROXY,
                    Some(FLAP_VAULT_PORTAL_IMPLEMENTATION),
                    FLAP_VAULT_PORTAL_PROXY_RUNTIME_KECCAK256,
                ),
                contract_pin(
                    ContractRole::Implementation,
                    FLAP_VAULT_PORTAL_IMPLEMENTATION,
                    None,
                    FLAP_VAULT_PORTAL_IMPLEMENTATION_RUNTIME_KECCAK256,
                ),
            ],
            RouteKind::NativeBondingCurve,
        ),
    ];
    for entry in v4.entries() {
        let family = if entry.launchpad == LaunchpadId::BankrDoppler {
            AdapterKind::DopplerV4
        } else if entry.launchpad == LaunchpadId::TrenchToday {
            AdapterKind::NativeCurve
        } else {
            AdapterKind::UniswapV4
        };
        let route = if entry.launchpad == LaunchpadId::BankrDoppler {
            RouteKind::DopplerPermit2
        } else if entry.launchpad == LaunchpadId::TrenchToday {
            RouteKind::NativeBondingCurve
        } else {
            RouteKind::V4HookedPool
        };
        let mut pins = vec![contract_pin(
            ContractRole::LaunchFactory,
            entry.destination.address,
            entry.implementation.map(|pin| pin.address),
            entry.destination.runtime_code_hash,
        )];
        if let Some(implementation) = entry.implementation {
            pins.push(contract_pin(
                ContractRole::Implementation,
                implementation.address,
                None,
                implementation.runtime_code_hash,
            ));
        }
        let observation_keys = keys(&[(entry.destination.address, entry.selector)], v4_wrappers);
        if let Some(existing) = specs.iter_mut().find(|spec| spec.id == entry.launchpad) {
            if existing.family != family || existing.allowed_routes != [route] {
                return Err(PaperObserverError::Startup(
                    "V4 entries disagree on canonical launchpad family or route".into(),
                ));
            }
            existing.observation_keys.extend(observation_keys);
            for pin in pins {
                if !existing.contract_pins.contains(&pin) {
                    existing.contract_pins.push(pin);
                }
            }
        } else {
            specs.push(spec(entry.launchpad, family, observation_keys, pins, route));
        }
    }
    if let Some(hash) = expected.hood_factory_runtime_hash {
        specs.push(spec(
            LaunchpadId::HoodFun,
            AdapterKind::NativeCurve,
            keys(
                &[
                    (HOOD_FACTORY, HOOD_CREATE_SELECTOR),
                    (HOOD_FACTORY, HOOD_BUY_SELECTOR),
                    (HOOD_FACTORY, HOOD_SELL_SELECTOR),
                ],
                &direct,
            ),
            vec![contract_pin(
                ContractRole::LaunchFactory,
                HOOD_FACTORY,
                None,
                hash,
            )],
            RouteKind::NativeBondingCurve,
        ));
    }
    if let (Some(factory_hash), Some(factory_impl_hash), Some(core_hash), Some(core_impl_hash)) = (
        expected.leavehood_factory_proxy_runtime_hash,
        expected.leavehood_factory_implementation_runtime_hash,
        expected.leavehood_core_proxy_runtime_hash,
        expected.leavehood_core_implementation_runtime_hash,
    ) {
        specs.push(spec(
            LaunchpadId::LeaveHood,
            AdapterKind::NativeCurve,
            keys(
                &[
                    (LEAVEHOOD_FACTORY_PROXY, LEAVEHOOD_LAUNCH_SELECTORS[0]),
                    (LEAVEHOOD_FACTORY_PROXY, LEAVEHOOD_LAUNCH_SELECTORS[1]),
                    (LEAVEHOOD_CORE_PROXY, LEAVEHOOD_BUY_SELECTOR),
                    (LEAVEHOOD_CORE_PROXY, LEAVEHOOD_SELL_WITH_SLIPPAGE_SELECTOR),
                    (LEAVEHOOD_CORE_PROXY, LEAVEHOOD_SELL_SELECTOR),
                ],
                &direct,
            ),
            vec![
                contract_pin(
                    ContractRole::LaunchFactory,
                    LEAVEHOOD_FACTORY_PROXY,
                    Some(LEAVEHOOD_FACTORY_IMPLEMENTATION),
                    factory_hash,
                ),
                contract_pin(
                    ContractRole::Implementation,
                    LEAVEHOOD_FACTORY_IMPLEMENTATION,
                    None,
                    factory_impl_hash,
                ),
                contract_pin(
                    ContractRole::LaunchFactory,
                    LEAVEHOOD_CORE_PROXY,
                    Some(LEAVEHOOD_CORE_IMPLEMENTATION),
                    core_hash,
                ),
                contract_pin(
                    ContractRole::Implementation,
                    LEAVEHOOD_CORE_IMPLEMENTATION,
                    None,
                    core_impl_hash,
                ),
            ],
            RouteKind::NativeBondingCurve,
        ));
    }
    Ok(specs)
}

fn spec(
    id: LaunchpadId,
    family: AdapterKind,
    observation_keys: Vec<DispatchKey>,
    contract_pins: Vec<ContractPin>,
    route: RouteKind,
) -> LaunchpadSpec {
    LaunchpadSpec {
        id,
        chain_id: CHAIN_ID,
        family,
        observation_keys,
        contract_pins,
        allowed_routes: vec![route],
        quote_assets: vec![WETH],
    }
}

fn keys(forms: &[(Address, [u8; 4])], wrappers: &[WrapperKind]) -> Vec<DispatchKey> {
    forms
        .iter()
        .flat_map(|(destination, selector)| {
            wrappers.iter().map(|wrapper| DispatchKey {
                destination: *destination,
                selector: *selector,
                wrapper: *wrapper,
            })
        })
        .collect()
}

fn contract_pin(
    role: ContractRole,
    address: Address,
    implementation: Option<Address>,
    runtime_code_hash: B256,
) -> ContractPin {
    ContractPin {
        role,
        address,
        implementation,
        runtime_code_hash,
    }
}

fn validate_document_pair(
    expected: &PaperExpectedPins,
    observed: &PaperObservedStartupSnapshot,
) -> Result<(), PaperObserverError> {
    if expected.schema_version != 1 || observed.schema_version != 1 || observed.chain_id != CHAIN_ID
    {
        return Err(PaperObserverError::Startup(
            "unsupported pin schema or chain".into(),
        ));
    }
    let valid_provenance = match (expected.provenance, observed.provenance) {
        (
            ExpectedPinsProvenance::ReviewedProtocolPins,
            ObservedPinsProvenance::StartupObservation,
        ) => expected.fixture_id.is_none() && observed.fixture_id.is_none(),
        (
            ExpectedPinsProvenance::SyntheticOfflineFixture,
            ObservedPinsProvenance::SyntheticOfflineFixture,
        ) => expected.fixture_id.is_some() && expected.fixture_id == observed.fixture_id,
        _ => false,
    };
    if !valid_provenance {
        return Err(PaperObserverError::Startup(
            "expected and observed pin provenance is incompatible".into(),
        ));
    }
    Ok(())
}

fn validate_observed_pins(
    observed: &PaperObservedStartupSnapshot,
) -> Result<(), PaperObserverError> {
    let mut identities = HashSet::new();
    for pin in &observed.pins {
        if pin.address == Address::ZERO
            || pin.runtime_hash == B256::ZERO
            || !identities.insert(pin.address)
        {
            return Err(PaperObserverError::Startup(
                "observed startup pins contain zero or duplicate identities".into(),
            ));
        }
    }
    Ok(())
}

fn find_observed_pin(
    pins: &[ObservedRuntimePin],
    address: Address,
    implementation: Option<Address>,
) -> Option<&ObservedRuntimePin> {
    pins.iter()
        .find(|pin| pin.address == address && pin.implementation == implementation)
}

fn configured_smart_account_pin(
    configured: ConfiguredSmartAccount,
) -> Result<SmartAccountPin, PaperObserverError> {
    let optional_pin = |address: Option<Address>,
                        runtime_hash: Option<B256>,
                        role: &'static str| {
        match (address, runtime_hash) {
            (None, None) => Ok(None),
            (Some(address), Some(runtime_code_hash)) => Ok(Some(SmartContractPin {
                address,
                runtime_code_hash,
            })),
            _ => Err(PaperObserverError::Startup(format!(
                "smart-account {role} address/runtime hash pair is incomplete"
            ))),
        }
    };
    Ok(SmartAccountPin {
        account: SmartContractPin {
            address: configured.account,
            runtime_code_hash: configured.runtime_hash,
        },
        execution_profile: configured.execution_profile,
        factory: optional_pin(
            configured.factory,
            configured.factory_runtime_hash,
            "factory",
        )?,
        delegation_implementation: optional_pin(
            configured.delegation_implementation,
            configured.delegation_runtime_hash,
            "delegation implementation",
        )?,
    })
}

fn validate_smart_account_pins(
    entry_point: SmartContractPin,
    accounts: &[SmartAccountPin],
    observed: &[ObservedRuntimePin],
) -> Result<(), PaperObserverError> {
    let entry_point_matches = find_observed_pin(observed, entry_point.address, None)
        .is_some_and(|actual| actual.runtime_hash == entry_point.runtime_code_hash);
    let accounts_match = accounts.iter().all(|account| {
        let implementation = account
            .delegation_implementation
            .map(|implementation| implementation.address);
        let account_matches = find_observed_pin(observed, account.account.address, implementation)
            .is_some_and(|actual| actual.runtime_hash == account.account.runtime_code_hash);
        let factory_matches = account.factory.is_none_or(|expected| {
            find_observed_pin(observed, expected.address, None)
                .is_some_and(|actual| actual.runtime_hash == expected.runtime_code_hash)
        });
        let delegation_matches = account.delegation_implementation.is_none_or(|expected| {
            find_observed_pin(observed, expected.address, None)
                .is_some_and(|actual| actual.runtime_hash == expected.runtime_code_hash)
        });
        account_matches && factory_matches && delegation_matches
    });
    if !entry_point_matches || !accounts_match {
        return Err(PaperObserverError::Startup(
            "smart-account expected pin is missing or mismatched".into(),
        ));
    }
    Ok(())
}

fn registry_erc4337_targets(
    registry: &StaticLaunchpadRegistry,
) -> Result<Vec<SmartContractPin>, PaperObserverError> {
    let mut targets = Vec::<SmartContractPin>::new();
    for spec in registry.specs() {
        for key in spec
            .observation_keys
            .iter()
            .filter(|key| key.wrapper == WrapperKind::Erc4337)
        {
            let pin = spec
                .contract_pins
                .iter()
                .find(|pin| pin.address == key.destination)
                .ok_or_else(|| {
                    PaperObserverError::Startup(
                        "ERC-4337 dispatch target lacks a canonical runtime pin".into(),
                    )
                })?;
            let target = SmartContractPin {
                address: pin.address,
                runtime_code_hash: pin.runtime_code_hash,
            };
            if let Some(existing) = targets
                .iter()
                .find(|existing| existing.address == target.address)
            {
                if *existing != target {
                    return Err(PaperObserverError::Startup(
                        "ERC-4337 target has conflicting canonical runtime pins".into(),
                    ));
                }
            } else {
                targets.push(target);
            }
        }
    }
    Ok(targets)
}

fn capability(
    launchpad: LaunchpadId,
    discovery_enabled: bool,
    paper_plan_supported: bool,
    wrappers: Vec<WrapperKind>,
    blockers: Vec<&'static str>,
) -> LaunchpadCapability {
    LaunchpadCapability {
        launchpad,
        attribution_sources: if launchpad == LaunchpadId::BankrDoppler {
            vec![AttributionSource::Virtuals]
        } else {
            Vec::new()
        },
        observation: if discovery_enabled {
            ObservationReachability::RawFeed
        } else {
            ObservationReachability::StartupPinRequired
        },
        discovery_enabled,
        paper_plan_supported,
        live_execution_enabled: false,
        wrappers,
        blockers,
    }
}

fn selector(input: &[u8]) -> Option<[u8; 4]> {
    input.get(..4)?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use alloy_consensus::{SignableTransaction, TxEip1559};
    use alloy_eips::eip2718::Encodable2718;
    use alloy_primitives::{Bytes, Signature, TxKind};
    use base64::Engine;
    use k256::ecdsa::signature::hazmat::PrehashSigner;
    use k256::ecdsa::{RecoveryId, Signature as K256Signature, SigningKey};

    use super::*;

    fn startup() -> (PaperExpectedPins, PaperObservedStartupSnapshot) {
        let expected = PaperExpectedPins {
            schema_version: 1,
            document_role: ExpectedPinsDocumentRole::ExpectedProtocolPins,
            provenance: ExpectedPinsProvenance::SyntheticOfflineFixture,
            fixture_id: Some("launchpad-paper-offline-v2".into()),
            bow_factory_runtime_hash: B256::with_last_byte(10),
            launchhood_v3_factory_runtime_hash: B256::with_last_byte(11),
            hood_factory_runtime_hash: Some(B256::with_last_byte(1)),
            leavehood_factory_proxy_runtime_hash: Some(B256::with_last_byte(2)),
            leavehood_factory_implementation_runtime_hash: Some(B256::with_last_byte(3)),
            leavehood_core_proxy_runtime_hash: Some(B256::with_last_byte(4)),
            leavehood_core_implementation_runtime_hash: Some(B256::with_last_byte(5)),
            klik_factory_runtime_hash: Some(B256::with_last_byte(6)),
            trench_proxy_runtime_hash: Some(B256::with_last_byte(7)),
            trench_implementation_runtime_hash: Some(B256::with_last_byte(8)),
            bankr_doppler_calls: vec![ConfiguredCallPin {
                destination: Address::with_last_byte(0xb0),
                runtime_hash: B256::with_last_byte(9),
                selector: [0xba, 0x6b, 0x00, 0x01],
            }],
            erc4337: None,
        };
        let mut snapshot = PaperObservedStartupSnapshot {
            schema_version: 1,
            document_role: ObservedPinsDocumentRole::ObservedStartupSnapshot,
            provenance: ObservedPinsProvenance::SyntheticOfflineFixture,
            fixture_id: Some("launchpad-paper-offline-v2".into()),
            chain_id: CHAIN_ID,
            pins: vec![
                observed(NOXA_LAUNCH_FACTORY, None, NOXA_FACTORY_RUNTIME_KECCAK256),
                observed(
                    ACTIVE_NOXA_LAUNCH_FACTORY,
                    None,
                    ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256,
                ),
                observed(BOW_LAUNCH_FACTORY, None, B256::with_last_byte(10)),
                observed(LAUNCHHOOD_V3_FACTORY, None, B256::with_last_byte(11)),
                observed(
                    crate::launchpad_adapters::CLANKER_FACTORY,
                    None,
                    crate::launchpad_adapters::CLANKER_FACTORY_RUNTIME_HASH,
                ),
                observed(
                    crate::launchpad_adapters::KLIK_FACTORY,
                    None,
                    B256::with_last_byte(6),
                ),
                observed(
                    crate::launchpad_adapters::TRENCH_PROXY,
                    Some(crate::launchpad_adapters::TRENCH_IMPLEMENTATION),
                    B256::with_last_byte(7),
                ),
                observed(
                    crate::launchpad_adapters::TRENCH_IMPLEMENTATION,
                    None,
                    B256::with_last_byte(8),
                ),
                observed(Address::with_last_byte(0xb0), None, B256::with_last_byte(9)),
                observed(
                    PONS_LEGACY_FACTORY,
                    None,
                    crate::pons::PONS_LEGACY_FACTORY_RUNTIME,
                ),
                observed(
                    PONS_CURRENT_FACTORY,
                    None,
                    crate::pons::PONS_CURRENT_FACTORY_RUNTIME,
                ),
                observed(
                    FLAP_PORTAL_PROXY,
                    Some(FLAP_PORTAL_IMPLEMENTATION),
                    FLAP_PORTAL_PROXY_RUNTIME_KECCAK256,
                ),
                observed(
                    FLAP_PORTAL_IMPLEMENTATION,
                    None,
                    FLAP_PORTAL_IMPLEMENTATION_RUNTIME_KECCAK256,
                ),
                observed(
                    FLAP_VAULT_PORTAL_PROXY,
                    Some(FLAP_VAULT_PORTAL_IMPLEMENTATION),
                    FLAP_VAULT_PORTAL_PROXY_RUNTIME_KECCAK256,
                ),
                observed(
                    FLAP_VAULT_PORTAL_IMPLEMENTATION,
                    None,
                    FLAP_VAULT_PORTAL_IMPLEMENTATION_RUNTIME_KECCAK256,
                ),
                observed(HOOD_FACTORY, None, B256::with_last_byte(1)),
                observed(
                    LEAVEHOOD_FACTORY_PROXY,
                    Some(LEAVEHOOD_FACTORY_IMPLEMENTATION),
                    B256::with_last_byte(2),
                ),
                observed(
                    LEAVEHOOD_FACTORY_IMPLEMENTATION,
                    None,
                    B256::with_last_byte(3),
                ),
                observed(
                    LEAVEHOOD_CORE_PROXY,
                    Some(LEAVEHOOD_CORE_IMPLEMENTATION),
                    B256::with_last_byte(4),
                ),
                observed(LEAVEHOOD_CORE_IMPLEMENTATION, None, B256::with_last_byte(5)),
            ],
        };
        snapshot.pins.extend([
            observed(WETH, None, WETH_RUNTIME_KECCAK256),
            observed(
                UNISWAP_V3_FACTORY,
                None,
                UNISWAP_V3_FACTORY_RUNTIME_KECCAK256,
            ),
            observed(
                UNISWAP_V3_POSITION_MANAGER,
                None,
                UNISWAP_V3_POSITION_MANAGER_RUNTIME_KECCAK256,
            ),
            observed(
                UNISWAP_V3_SWAP_ROUTER_02,
                None,
                UNISWAP_V3_SWAP_ROUTER_02_RUNTIME_KECCAK256,
            ),
        ]);
        for identity in PonsAdapter::required_startup_identities() {
            if !snapshot
                .pins
                .iter()
                .any(|pin| pin.address == identity.address)
            {
                snapshot
                    .pins
                    .push(observed(identity.address, None, identity.runtime_hash));
            }
        }
        (expected, snapshot)
    }

    fn observed(
        address: Address,
        implementation: Option<Address>,
        runtime_hash: B256,
    ) -> ObservedRuntimePin {
        ObservedRuntimePin {
            address,
            implementation,
            runtime_hash,
            code_bytes: PonsAdapter::required_startup_identities()
                .iter()
                .find(|identity| identity.address == address)
                .map(|identity| identity.code_bytes),
        }
    }

    fn signed_transaction(destination: Address, input: Vec<u8>) -> TxEnvelope {
        let transaction = TxEip1559 {
            chain_id: CHAIN_ID,
            nonce: 0,
            gas_limit: 200_000,
            max_fee_per_gas: 1,
            max_priority_fee_per_gas: 0,
            to: TxKind::Call(destination),
            value: U256::ZERO,
            access_list: Default::default(),
            input: Bytes::from(input),
        };
        let key = SigningKey::from_slice(&[7_u8; 32]).unwrap();
        let (signature, recovery_id): (K256Signature, RecoveryId) = key
            .sign_prehash(transaction.signature_hash().as_slice())
            .unwrap();
        let signature: Signature = (signature, recovery_id).into();
        transaction.into_signed(signature).into()
    }

    fn feed(transaction: &TxEnvelope) -> BroadcastMessage {
        let mut l2 = vec![4];
        l2.extend_from_slice(&transaction.encoded_2718());
        serde_json::from_value(serde_json::json!({
            "version": 1,
            "messages": [{
                "sequenceNumber": 42,
                "message": {"message": {
                    "header": {"kind": 3, "blockNumber": 9, "timestamp": 10},
                    "l2Msg": base64::engine::general_purpose::STANDARD.encode(l2)
                }}
            }]
        }))
        .unwrap()
    }

    fn observer() -> PaperLaunchpadObserver {
        let (expected, observed) = startup();
        PaperLaunchpadObserver::from_startup_snapshots(expected, observed).unwrap()
    }

    #[test]
    fn raw_signed_nitro_fixture_reaches_registry_adapter_and_observation_output() {
        let observer = observer();
        let mut input = HOOD_CREATE_SELECTOR.to_vec();
        input.extend_from_slice(&[0_u8; 32]);
        let transaction = signed_transaction(HOOD_FACTORY, input);
        let expected_leader = transaction.recover_signer().unwrap();
        let mut runtime = PaperFeedRuntime::new(observer);
        let report = runtime.decode(&feed(&transaction)).unwrap();

        assert_eq!(report.decode.signed_transactions, 1);
        assert!(report.rejections.is_empty());
        assert_eq!(report.observations.len(), 1);
        assert_eq!(report.observations[0].launchpad, LaunchpadId::HoodFun);
        assert_eq!(report.observations[0].leader, expected_leader);
        assert_eq!(
            report.observations[0].leader_origin,
            LeaderOrigin::DirectSigner
        );
        assert!(!report.observations[0].live_execution_enabled);
    }

    #[test]
    fn cross_adapter_destination_does_not_fall_through() {
        let observer = observer();
        let mut input = HOOD_CREATE_SELECTOR.to_vec();
        input.extend_from_slice(&[0_u8; 32]);
        let transaction = signed_transaction(Address::with_last_byte(0xee), input);
        let mut runtime = PaperFeedRuntime::new(observer);
        let report = runtime.decode(&feed(&transaction)).unwrap();
        assert!(report.observations.is_empty());
        assert!(report.rejections.is_empty());
    }

    #[test]
    fn capability_output_is_complete_explicit_and_never_live() {
        let observer = observer();
        assert_eq!(observer.registry.specs().len(), 11);
        let capabilities = observer.capabilities();
        assert_eq!(capabilities.len(), 11);
        assert!(capabilities.iter().all(|capability| {
            capability.observation == ObservationReachability::RawFeed
                && capability.discovery_enabled
                && !capability.live_execution_enabled
                && !capability.blockers.is_empty()
        }));
        assert!(capabilities.iter().any(|capability| {
            capability.launchpad == LaunchpadId::BankrDoppler
                && capability.attribution_sources == vec![AttributionSource::Virtuals]
                && capability
                    .blockers
                    .iter()
                    .any(|blocker| blocker.contains("Virtuals"))
        }));
        serde_json::to_string(&capabilities).unwrap();
    }

    #[test]
    fn missing_or_mismatched_observed_curve_pin_fails_closed() {
        let (expected, mut observed) = startup();
        observed.pins.retain(|pin| pin.address != HOOD_FACTORY);
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected, observed).is_err());

        let (expected, mut observed) = startup();
        find_observed_mut(&mut observed.pins, HOOD_FACTORY).runtime_hash = B256::with_last_byte(99);
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected, observed).is_err());

        let (expected, mut observed) = startup();
        observed.pins.retain(|pin| pin.address != WETH);
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected, observed).is_err());

        let (expected, mut observed) = startup();
        find_observed_mut(&mut observed.pins, crate::pons::PONS_CURRENT_LOCKER).code_bytes = None;
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected, observed).is_err());
    }

    #[test]
    fn proxy_implementation_mismatch_and_duplicate_observation_fail_closed() {
        let (expected, mut observed) = startup();
        find_observed_mut(&mut observed.pins, LEAVEHOOD_FACTORY_PROXY).implementation =
            Some(Address::with_last_byte(0xee));
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected, observed).is_err());

        let (expected, mut observed) = startup();
        observed.pins.push(observed.pins[0]);
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected, observed).is_err());
    }

    #[test]
    fn expected_document_cannot_self_attest_as_observed_snapshot() {
        let value = serde_json::json!({
            "schema_version": 1,
            "document_role": "expected_protocol_pins",
            "provenance": "synthetic_offline_fixture",
            "fixture_id": "launchpad-paper-offline-v2",
            "chain_id": CHAIN_ID,
            "pins": []
        });
        assert!(serde_json::from_value::<PaperObservedStartupSnapshot>(value).is_err());

        let (expected, mut observed) = startup();
        observed.provenance = ObservedPinsProvenance::StartupObservation;
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected, observed).is_err());
    }

    #[test]
    fn verified_erc4337_capabilities_are_derived_from_canonical_registry_targets() {
        let (mut expected, mut snapshot) = startup();
        let entry_point_hash = B256::with_last_byte(0xe1);
        let account = Address::with_last_byte(0xa1);
        let account_hash = B256::with_last_byte(0xa1);
        expected.erc4337 = Some(ConfiguredSmartAccounts {
            entry_point_runtime_hash: entry_point_hash,
            accounts: vec![ConfiguredSmartAccount {
                account,
                runtime_hash: account_hash,
                execution_profile: AccountExecutionProfile::ExecuteAddressValueBytes,
                factory: None,
                factory_runtime_hash: None,
                delegation_implementation: None,
                delegation_runtime_hash: None,
            }],
        });
        snapshot
            .pins
            .push(observed(ENTRY_POINT_V07, None, entry_point_hash));
        snapshot.pins.push(observed(account, None, account_hash));

        let observer = PaperLaunchpadObserver::from_startup_snapshots(expected, snapshot).unwrap();
        let capabilities = observer.capabilities();
        assert_eq!(
            capabilities
                .iter()
                .find(|capability| capability.launchpad == LaunchpadId::Clanker)
                .unwrap()
                .wrappers,
            vec![WrapperKind::Direct, WrapperKind::Erc4337]
        );
        assert_eq!(
            capabilities
                .iter()
                .find(|capability| capability.launchpad == LaunchpadId::Noxa)
                .unwrap()
                .wrappers,
            vec![WrapperKind::Direct]
        );
    }

    #[test]
    fn incomplete_smart_account_factory_or_delegation_pairs_fail_closed() {
        let base = ConfiguredSmartAccount {
            account: Address::with_last_byte(0xa1),
            runtime_hash: B256::with_last_byte(0xa1),
            execution_profile: AccountExecutionProfile::ExecuteAddressValueBytes,
            factory: None,
            factory_runtime_hash: None,
            delegation_implementation: None,
            delegation_runtime_hash: None,
        };

        let mut missing_factory_hash = base;
        missing_factory_hash.factory = Some(Address::with_last_byte(0xf1));
        assert!(configured_smart_account_pin(missing_factory_hash).is_err());

        let mut missing_delegation_hash = base;
        missing_delegation_hash.execution_profile = AccountExecutionProfile::Erc7579SingleCall;
        missing_delegation_hash.delegation_implementation = Some(Address::with_last_byte(0xd1));
        assert!(configured_smart_account_pin(missing_delegation_hash).is_err());
    }

    #[test]
    fn multiple_bankr_calls_share_one_canonical_launchpad_authority() {
        let (mut expected, mut snapshot) = startup();
        let destination = Address::with_last_byte(0xb1);
        let runtime_hash = B256::with_last_byte(0xb1);
        let selector = [0xba, 0x6b, 0x00, 0x02];
        expected.bankr_doppler_calls.push(ConfiguredCallPin {
            destination,
            runtime_hash,
            selector,
        });
        snapshot
            .pins
            .push(observed(destination, None, runtime_hash));

        let observer = PaperLaunchpadObserver::from_startup_snapshots(expected, snapshot).unwrap();
        assert_eq!(observer.registry.specs().len(), 11);
        assert!(observer.might_observe(destination, &selector));
    }

    fn find_observed_mut(
        pins: &mut [ObservedRuntimePin],
        address: Address,
    ) -> &mut ObservedRuntimePin {
        pins.iter_mut().find(|pin| pin.address == address).unwrap()
    }
}
