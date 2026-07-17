//! Unified, broadcast-free launchpad observation path for decoded Nitro transactions.
//!
//! All code identities are supplied at construction. Candidate handling is synchronous and
//! contains no RPC, filesystem, signing, submission, or control-plane capability.

use std::collections::HashSet;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use alloy_consensus::{Transaction, TxEnvelope, transaction::SignerRecoverable};
use alloy_primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::PonsAdapter;
use crate::bankr_receipt_quote::{
    BANKR_CREATE_SELECTOR, BankrDopplerExpectedProfile, predict_bankr_create_identity,
    validate_bankr_create_calldata_for_observation,
};
use crate::clanker_receipt_quote::{
    CLANKER_DESCENDING_MEV_MODULE, CLANKER_EXTENSION, CLANKER_STATIC_HOOK, ClankerV4ExpectedProfile,
};
use crate::decoder::{DecodeError, DecodeReport, FeedDecoder, Filter};
use crate::eip7702_self_batch::{
    Eip7702ObservedDelegation, Eip7702SelfBatchExpectedPins, PONS_EIP7702_OUTER_SELECTOR,
    decode_pons_eip7702_self_batch, pons_eip7702_designator,
};
use crate::feed::BroadcastMessage;
use crate::flap_identity::{
    FLAP_PORTAL_IMPLEMENTATION, FLAP_PORTAL_IMPLEMENTATION_RUNTIME_KECCAK256, FLAP_PORTAL_PROXY,
    FLAP_PORTAL_PROXY_RUNTIME_KECCAK256, FLAP_VAULT_PORTAL_IMPLEMENTATION,
    FLAP_VAULT_PORTAL_IMPLEMENTATION_RUNTIME_KECCAK256, FLAP_VAULT_PORTAL_PROXY,
    FLAP_VAULT_PORTAL_PROXY_RUNTIME_KECCAK256,
};
use crate::hood_receipt_quote::{HoodExpectedProfile, HoodIdentityRole};
use crate::launchpad_adapter::{
    ActionKind, AdapterKind, AttributionSource, LaunchpadId, RouteKind, WrapperKind,
};
use crate::launchpad_adapters::{
    CLANKER_DEPLOYER, CLANKER_FACTORY, CLANKER_LOCKER, DispatchEntry, ExecutionMode,
    ResearchStartupPins, RuntimeCodePin, V4_POOL_MANAGER, V4AdapterSet, V4CandidateCall,
    preload_clanker_prediction_profile,
};
use crate::launchpad_registry::{
    BoundedCall, ContractPin, ContractRole, DispatchKey, LaunchpadSpec, ObservedContractPin,
    StartupPinSnapshot, StaticLaunchpadRegistry,
};
use crate::noxa_abi::{LAUNCH_TOKEN_SELECTOR, decode_launch_call};
use crate::noxa_rpc::HoodProtocolSnapshot;
use crate::pons::{
    PONS_CURRENT_FACTORY, PONS_LAUNCH_SELECTOR, PONS_LEGACY_FACTORY, PonsExpectedProfile,
    RuntimeIdentity,
};
use crate::robinhood::{
    ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256, ACTIVE_NOXA_LAUNCH_FACTORY, BOW_LAUNCH_FACTORY,
    CHAIN_ID, LAUNCHHOOD_V3_FACTORY, LAUNCHHOOD_V3_FACTORY_RUNTIME_KECCAK256,
    LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION, LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_CODE_BYTES,
    LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_RUNTIME_KECCAK256, NOXA_FACTORY_RUNTIME_KECCAK256,
    NOXA_LAUNCH_FACTORY, UNISWAP_V3_FACTORY, UNISWAP_V3_FACTORY_RUNTIME_KECCAK256,
    UNISWAP_V3_POSITION_MANAGER, UNISWAP_V3_POSITION_MANAGER_RUNTIME_KECCAK256,
    UNISWAP_V3_SWAP_ROUTER_02, UNISWAP_V3_SWAP_ROUTER_02_RUNTIME_KECCAK256, WETH,
    WETH_RUNTIME_KECCAK256,
};
use crate::smart_account::{
    AccountExecutionProfile, ContractPin as SmartContractPin, ENTRY_POINT_V07,
    ENTRY_POINT_V07_HANDLE_OPS_SELECTOR, EntryPointCall, OwnedValidatedSmartAccountPins,
    SmartAccountDecodeError, SmartAccountPin, decode_entry_point_v07_prevalidated,
    discover_entry_point_v07_erc7579,
};
use crate::tier2_curve::{
    CurveCandidateCall, HOOD_BUY_FOR_SELECTOR, HOOD_BUY_SELECTOR, HOOD_CREATE_SELECTOR,
    HOOD_FACTORY, HOOD_SELL_SELECTOR, LEAVEHOOD_BUY_SELECTOR, LEAVEHOOD_CORE_IMPLEMENTATION,
    LEAVEHOOD_CORE_PROXY, LEAVEHOOD_FACTORY_IMPLEMENTATION, LEAVEHOOD_FACTORY_PROXY,
    LEAVEHOOD_LAUNCH_SELECTORS, LEAVEHOOD_SELL_SELECTOR, LEAVEHOOD_SELL_WITH_SLIPPAGE_SELECTOR,
    RuntimePin as CurveRuntimePin, StartupPins as CurveStartupPins, Tier2CurveAdapter,
    predict_hood_token_address,
};
use crate::uniswap_v4::CodePin as V4CodePin;
use crate::v3_launch_at_birth::{
    BOW_LAUNCH_SELECTOR, ContractCodeSnapshot, LAUNCHHOOD_V3_LAUNCH_SELECTOR,
    V3LaunchAtBirthAdapter,
};

pub const FLAP_STANDARD_LAUNCH_SELECTOR: [u8; 4] = [0x2e, 0x2f, 0xdb, 0xd9];
pub const FLAP_TAX_V3_LAUNCH_SELECTOR: [u8; 4] = [0x8c, 0xb5, 0x77, 0x2c];
pub const FLAP_VAULT_LAUNCH_SELECTOR: [u8; 4] = [0x1b, 0x80, 0x62, 0x20];
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
    Eip7702Authority,
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
    pub action: Option<ActionKind>,
    pub predicted_token: Option<Address>,
    pub predicted_pool: Option<Address>,
    /// Canonical Uniswap V4 PoolKey hash when the protocol has no pool address.
    pub predicted_pool_id: Option<B256>,
    pub planning_mode: ExecutionMode,
    pub live_execution_enabled: bool,
    pub feed_sequence: Option<u64>,
    pub l1_block_number: Option<u64>,
    pub l1_timestamp: Option<u64>,
    pub observer_received_unix_ns: Option<u64>,
    pub observer_latency_ns: Option<u64>,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaperFeedTransaction {
    pub tx_hash: B256,
    pub feed_sequence: u64,
    pub l1_block_number: u64,
    pub l1_timestamp: u64,
    pub frame_received_unix_ns: u64,
}

#[derive(Debug, Serialize)]
pub struct PaperFeedRejection {
    pub tx_hash: B256,
    pub feed_sequence: u64,
    pub l1_block_number: u64,
    pub l1_timestamp: u64,
    pub observer_received_unix_ns: u64,
    pub observer_latency_ns: u64,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct PaperFeedReport {
    pub frame_received_unix_ns: u64,
    pub frame_decode_elapsed_ns: u64,
    pub decode: PaperDecodeSummary,
    pub transactions: Vec<PaperFeedTransaction>,
    pub observations: Vec<PaperLaunchpadObservation>,
    pub trade_plans: Vec<PaperTradePlan>,
    pub reconciliation_requests: Vec<PaperReconciliationRequest>,
    pub rejections: Vec<PaperFeedRejection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperTradePlanStatus {
    AwaitingIndependentWarmQuote,
    UnavailableDiscoveryOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaperPlanPolicy {
    pub max_input_wei: U256,
    pub slippage_bps: u16,
    pub take_profit_bps: u16,
    pub stop_loss_bps: u16,
    pub max_hold_seconds: u64,
}

impl Default for PaperPlanPolicy {
    fn default() -> Self {
        Self {
            max_input_wei: U256::from(1_000_000_000_000_000_u64),
            slippage_bps: 100,
            take_profit_bps: 2_000,
            stop_loss_bps: 1_000,
            max_hold_seconds: 300,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaperTradePlan {
    pub tx_hash: B256,
    pub launchpad: LaunchpadId,
    pub feed_sequence: u64,
    pub status: PaperTradePlanStatus,
    pub max_input_wei: U256,
    pub slippage_bps: u16,
    pub expected_output: Option<U256>,
    pub min_receive: Option<U256>,
    pub quote_source: &'static str,
    pub leader_amounts_reused: bool,
    pub exit_full_position: bool,
    pub take_profit_bps: u16,
    pub stop_loss_bps: u16,
    pub max_hold_seconds: u64,
    pub broadcast: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaperReconciliationRequest {
    pub tx_hash: B256,
    pub launchpad: LaunchpadId,
    pub feed_sequence: u64,
    pub l1_block_number: u64,
    pub l1_timestamp: u64,
    pub evidence_source: &'static str,
    pub initial_decision_dependency: bool,
    pub wrapper: WrapperKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wrapper_provenance: Option<crate::eip7702_self_batch::Eip7702SelfBatchProvenance>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PinBlockBoundary {
    pub l2_block_number: u64,
    pub l2_block_hash: B256,
    pub l1_block_number: u64,
    pub block_timestamp: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaperExpectedPins {
    pub schema_version: u32,
    pub document_role: ExpectedPinsDocumentRole,
    pub provenance: ExpectedPinsProvenance,
    pub fixture_id: Option<String>,
    #[serde(default)]
    pub reviewed_at: Option<PinBlockBoundary>,
    pub pons_v3: ConfiguredPonsV3,
    #[serde(default)]
    pub pons_eip7702_self_batch: Option<Eip7702SelfBatchExpectedPins>,
    pub bow_factory_runtime_hash: B256,
    pub launchhood_v3_factory_runtime_hash: B256,
    pub launchhood_v3_token_implementation: ConfiguredRuntimeIdentity,
    pub hood_factory_runtime_hash: Option<B256>,
    #[serde(default)]
    pub hood_curve: Option<HoodExpectedProfile>,
    pub leavehood_factory_proxy_runtime_hash: Option<B256>,
    pub leavehood_factory_implementation_runtime_hash: Option<B256>,
    pub leavehood_core_proxy_runtime_hash: Option<B256>,
    pub leavehood_core_implementation_runtime_hash: Option<B256>,
    pub klik_factory_runtime_hash: Option<B256>,
    pub trench_proxy_runtime_hash: Option<B256>,
    pub trench_implementation_runtime_hash: Option<B256>,
    #[serde(default)]
    pub clanker_v4: Option<ConfiguredClankerV4>,
    #[serde(default)]
    pub bankr_doppler_v4: Option<ConfiguredBankrDopplerV4>,
    #[serde(default)]
    pub bankr_doppler_calls: Vec<ConfiguredCallPin>,
    pub erc4337: Option<ConfiguredSmartAccounts>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredPonsV3 {
    pub identities: Vec<ConfiguredPonsRuntimeIdentity>,
    pub prediction: crate::pons_predict::PonsPredictionSemantics,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredPonsRuntimeIdentity {
    pub address: Address,
    pub code_bytes: usize,
    pub runtime_hash: B256,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredRuntimeIdentity {
    pub address: Address,
    pub code_bytes: usize,
    pub runtime_hash: B256,
}

impl ConfiguredPonsV3 {
    pub fn expected_profile(&self) -> Result<PonsExpectedProfile, PaperObserverError> {
        let production = PonsExpectedProfile::production();
        let required = production.identities();
        if self.identities.len() != required.len() {
            return Err(PaperObserverError::Startup(
                "reviewed Pons profile must contain every exact identity once".into(),
            ));
        }
        for expected in required {
            let matches = self
                .identities
                .iter()
                .filter(|configured| configured.address == expected.address)
                .collect::<Vec<_>>();
            if matches.len() != 1
                || matches[0].code_bytes != expected.code_bytes
                || matches[0].runtime_hash != expected.runtime_hash
            {
                return Err(PaperObserverError::Startup(
                    "configured Pons identity disagrees with the independently reviewed profile"
                        .into(),
                ));
            }
        }
        self.prediction
            .validate_production()
            .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        if self.prediction != crate::pons_predict::PonsPredictionSemantics::production() {
            return Err(PaperObserverError::Startup(
                "configured Pons prediction semantics disagree with the independently reviewed profile"
                    .into(),
            ));
        }
        production
            .validate()
            .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        Ok(production)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredClankerV4 {
    pub factory_runtime_hash: B256,
    pub deployer_runtime_hash: B256,
    pub pool_manager_runtime_hash: B256,
    pub hook_runtime_hash: B256,
    pub locker_runtime_hash: B256,
    pub mev_module_runtime_hash: B256,
    pub extension_runtime_hash: B256,
    pub max_static_fee_ppm: u32,
    pub max_mev_fee_ppm: u32,
    pub max_mev_seconds_to_decay: u64,
    pub mev_delay_guard_seconds: u64,
    pub protocol_fee_share_percent: u8,
}

impl ConfiguredClankerV4 {
    pub fn expected_profile(self) -> Result<ClankerV4ExpectedProfile, PaperObserverError> {
        if self.deployer_runtime_hash != crate::launchpad_adapters::CLANKER_DEPLOYER_RUNTIME_HASH {
            return Err(PaperObserverError::Startup(
                "Clanker deployer library runtime pin drifted".into(),
            ));
        }
        let profile = ClankerV4ExpectedProfile {
            factory: V4CodePin {
                address: CLANKER_FACTORY,
                runtime_code_hash: self.factory_runtime_hash,
            },
            pool_manager: V4CodePin {
                address: V4_POOL_MANAGER,
                runtime_code_hash: self.pool_manager_runtime_hash,
            },
            hook: V4CodePin {
                address: CLANKER_STATIC_HOOK,
                runtime_code_hash: self.hook_runtime_hash,
            },
            locker: V4CodePin {
                address: CLANKER_LOCKER,
                runtime_code_hash: self.locker_runtime_hash,
            },
            mev_module: V4CodePin {
                address: CLANKER_DESCENDING_MEV_MODULE,
                runtime_code_hash: self.mev_module_runtime_hash,
            },
            extension: V4CodePin {
                address: CLANKER_EXTENSION,
                runtime_code_hash: self.extension_runtime_hash,
            },
            max_static_fee_ppm: self.max_static_fee_ppm,
            max_mev_fee_ppm: self.max_mev_fee_ppm,
            max_mev_seconds_to_decay: self.max_mev_seconds_to_decay,
            mev_delay_guard_seconds: self.mev_delay_guard_seconds,
            protocol_fee_share_percent: self.protocol_fee_share_percent,
        };
        profile
            .validate()
            .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        if profile != ClankerV4ExpectedProfile::production() {
            return Err(PaperObserverError::Startup(
                "configured Clanker profile disagrees with independently reviewed identity and semantics"
                    .into(),
            ));
        }
        Ok(profile)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredBankrDopplerV4 {
    pub airlock_runtime_hash: B256,
    pub pool_manager_runtime_hash: B256,
    pub initializer_runtime_hash: B256,
    pub rehype_hook_runtime_hash: B256,
    pub token_factory_runtime_hash: B256,
    pub token_implementation_runtime_hash: B256,
    pub governance_factory_runtime_hash: B256,
    pub liquidity_migrator_runtime_hash: B256,
    pub standard_lp_fee_ppm: u32,
    pub max_lp_fee_ppm: u32,
    pub hook_fee_denominator_ppm: u32,
    pub hook_start_fee_ppm: u32,
    pub hook_end_fee_ppm: u32,
    pub hook_duration_seconds: u64,
    pub quote_delay_guard_seconds: u64,
    pub tick_spacing: i32,
    pub pool_allocation_bps: u16,
    pub primary_curve_share_bps: u16,
    pub secondary_curve_share_bps: u16,
    pub creator_beneficiary_bps: u16,
    pub protocol_beneficiary_bps: u16,
}

impl ConfiguredBankrDopplerV4 {
    pub fn expected_profile(self) -> Result<BankrDopplerExpectedProfile, PaperObserverError> {
        let mut profile = BankrDopplerExpectedProfile::production();
        profile.airlock.runtime_code_hash = self.airlock_runtime_hash;
        profile.pool_manager.runtime_code_hash = self.pool_manager_runtime_hash;
        profile.initializer.runtime_code_hash = self.initializer_runtime_hash;
        profile.rehype_hook.runtime_code_hash = self.rehype_hook_runtime_hash;
        profile.token_factory.runtime_code_hash = self.token_factory_runtime_hash;
        profile.token_implementation.runtime_code_hash = self.token_implementation_runtime_hash;
        profile.governance_factory.runtime_code_hash = self.governance_factory_runtime_hash;
        profile.liquidity_migrator.runtime_code_hash = self.liquidity_migrator_runtime_hash;
        profile.standard_lp_fee_ppm = self.standard_lp_fee_ppm;
        profile.max_lp_fee_ppm = self.max_lp_fee_ppm;
        profile.hook_fee_denominator_ppm = self.hook_fee_denominator_ppm;
        profile.hook_start_fee_ppm = self.hook_start_fee_ppm;
        profile.hook_end_fee_ppm = self.hook_end_fee_ppm;
        profile.hook_duration_seconds = self.hook_duration_seconds;
        profile.quote_delay_guard_seconds = self.quote_delay_guard_seconds;
        profile.tick_spacing = self.tick_spacing;
        profile.pool_allocation_bps = self.pool_allocation_bps;
        profile.primary_curve_share_bps = self.primary_curve_share_bps;
        profile.secondary_curve_share_bps = self.secondary_curve_share_bps;
        profile.creator_beneficiary_bps = self.creator_beneficiary_bps;
        profile.protocol_beneficiary_bps = self.protocol_beneficiary_bps;
        profile
            .validate()
            .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        Ok(profile)
    }
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
    #[serde(default)]
    pub observed_at: Option<PinBlockBoundary>,
    pub pins: Vec<ObservedRuntimePin>,
    pub pons_v3_semantics: Option<crate::pons_predict::PonsPredictionSemantics>,
    #[serde(default)]
    pub hood_protocol: Option<HoodProtocolSnapshot>,
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
    pons_profile: PonsExpectedProfile,
    pons_predictor: crate::pons_predict::PonsCurrentPredictor,
    pons_eip7702_self_batch: Option<Eip7702SelfBatchExpectedPins>,
    curves: Tier2CurveAdapter,
    smart_accounts: Option<OwnedValidatedSmartAccountPins>,
    bankr_profile: Option<BankrDopplerExpectedProfile>,
}

/// End-to-end Nitro paper runtime. It intentionally exposes no executor, signer, or RPC client.
pub struct PaperFeedRuntime {
    decoder: FeedDecoder,
    observer: PaperLaunchpadObserver,
    plan_policy: PaperPlanPolicy,
}

impl PaperFeedRuntime {
    pub fn new(observer: PaperLaunchpadObserver) -> Self {
        Self {
            decoder: FeedDecoder::new(Filter::default()),
            observer,
            plan_policy: PaperPlanPolicy::default(),
        }
    }

    pub fn with_plan_policy(
        observer: PaperLaunchpadObserver,
        plan_policy: PaperPlanPolicy,
    ) -> Result<Self, PaperObserverError> {
        if plan_policy.max_input_wei == U256::ZERO
            || plan_policy.slippage_bps == 0
            || plan_policy.slippage_bps >= 10_000
            || plan_policy.take_profit_bps == 0
            || plan_policy.stop_loss_bps == 0
            || plan_policy.stop_loss_bps >= 10_000
            || plan_policy.max_hold_seconds == 0
        {
            return Err(PaperObserverError::Startup(
                "paper plan policy contains zero or out-of-range bounds".into(),
            ));
        }
        preload_clanker_prediction_profile()
            .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        Ok(Self {
            decoder: FeedDecoder::new(Filter::default()),
            observer,
            plan_policy,
        })
    }

    pub fn capabilities(&self) -> Vec<LaunchpadCapability> {
        self.observer.capabilities()
    }

    pub fn decode(&mut self, feed: &BroadcastMessage) -> Result<PaperFeedReport, DecodeError> {
        self.decode_received_at(feed, unix_now_ns())
    }

    /// Decode a frame while retaining the timestamp captured where the input
    /// was first received. For recorded probe wrappers this includes FIFO/tee
    /// backlog instead of resetting the latency clock at decode start.
    pub fn decode_received_at(
        &mut self,
        feed: &BroadcastMessage,
        frame_received_unix_ns: u64,
    ) -> Result<PaperFeedReport, DecodeError> {
        let frame_started = Instant::now();
        let mut observations = Vec::new();
        let mut transactions = Vec::new();
        let mut rejections = Vec::new();
        let report: DecodeReport = self.decoder.decode_with(feed, |context| {
            transactions.push(PaperFeedTransaction {
                tx_hash: *context.transaction.tx_hash(),
                feed_sequence: context.sequence_number,
                l1_block_number: context.l1_block_number,
                l1_timestamp: context.l1_timestamp,
                frame_received_unix_ns,
            });
            match self.observer.observe_transaction(context.transaction) {
                Ok(Some(mut observation)) => {
                    observation.feed_sequence = Some(context.sequence_number);
                    observation.l1_block_number = Some(context.l1_block_number);
                    observation.l1_timestamp = Some(context.l1_timestamp);
                    observation.observer_received_unix_ns = Some(frame_received_unix_ns);
                    observation.observer_latency_ns =
                        Some(unix_now_ns().saturating_sub(frame_received_unix_ns));
                    observations.push(observation);
                }
                Ok(None) => {}
                Err(error) => rejections.push(PaperFeedRejection {
                    tx_hash: *context.transaction.tx_hash(),
                    feed_sequence: context.sequence_number,
                    l1_block_number: context.l1_block_number,
                    l1_timestamp: context.l1_timestamp,
                    observer_received_unix_ns: frame_received_unix_ns,
                    observer_latency_ns: unix_now_ns().saturating_sub(frame_received_unix_ns),
                    reason: error.to_string(),
                }),
            }
        })?;
        let trade_plans = observations
            .iter()
            .map(|observation| paper_trade_plan(observation, self.plan_policy))
            .collect();
        let reconciliation_requests = observations
            .iter()
            .map(paper_reconciliation_request)
            .collect();
        Ok(PaperFeedReport {
            frame_received_unix_ns,
            frame_decode_elapsed_ns: elapsed_ns(frame_started),
            decode: PaperDecodeSummary {
                messages: report.messages,
                signed_transactions: report.signed_transactions,
                envelope_decode_ns: report.envelope_decode_ns,
            },
            transactions,
            observations,
            trade_plans,
            reconciliation_requests,
            rejections,
        })
    }
}

fn unix_now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u128::from(u64::MAX)) as u64
}

fn elapsed_ns(started: Instant) -> u64 {
    started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
}

fn paper_trade_plan(
    observation: &PaperLaunchpadObservation,
    policy: PaperPlanPolicy,
) -> PaperTradePlan {
    let (status, quote_source) = if observation.planning_mode == ExecutionMode::DiscoveryOnly {
        (
            PaperTradePlanStatus::UnavailableDiscoveryOnly,
            "adapter_semantics_incomplete",
        )
    } else {
        (
            PaperTradePlanStatus::AwaitingIndependentWarmQuote,
            "independent_warm_market_snapshot_required",
        )
    };
    PaperTradePlan {
        tx_hash: observation.tx_hash,
        launchpad: observation.launchpad,
        feed_sequence: observation
            .feed_sequence
            .expect("runtime attaches sequence before planning"),
        status,
        max_input_wei: policy.max_input_wei,
        slippage_bps: policy.slippage_bps,
        expected_output: None,
        min_receive: None,
        quote_source,
        leader_amounts_reused: false,
        exit_full_position: true,
        take_profit_bps: policy.take_profit_bps,
        stop_loss_bps: policy.stop_loss_bps,
        max_hold_seconds: policy.max_hold_seconds,
        broadcast: false,
    }
}

fn paper_reconciliation_request(
    observation: &PaperLaunchpadObservation,
) -> PaperReconciliationRequest {
    let wrapper_provenance = observation
        .detail
        .get("eip7702_self_batch")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    PaperReconciliationRequest {
        tx_hash: observation.tx_hash,
        launchpad: observation.launchpad,
        feed_sequence: observation
            .feed_sequence
            .expect("runtime attaches sequence before reconciliation"),
        l1_block_number: observation
            .l1_block_number
            .expect("runtime attaches L1 block before reconciliation"),
        l1_timestamp: observation
            .l1_timestamp
            .expect("runtime attaches L1 timestamp before reconciliation"),
        evidence_source: "independent_receipt_and_protocol_events",
        initial_decision_dependency: false,
        wrapper: observation.wrapper,
        wrapper_provenance,
    }
}

impl PaperLaunchpadObserver {
    pub fn from_startup_snapshots(
        expected: PaperExpectedPins,
        observed: PaperObservedStartupSnapshot,
    ) -> Result<Self, PaperObserverError> {
        validate_document_pair(&expected, &observed)?;
        validate_observed_pins(&observed)?;
        validate_launchhood_identity(&expected, &observed.pins)?;
        let pons_profile = expected.pons_v3.expected_profile()?;
        let observed_pons_semantics = observed.pons_v3_semantics.as_ref().ok_or_else(|| {
            PaperObserverError::Startup(
                "fresh startup snapshot is missing current Pons semantic getters".into(),
            )
        })?;
        let pons_predictor = crate::pons_predict::PonsCurrentPredictor::from_startup_profiles(
            &expected.pons_v3.prediction,
            observed_pons_semantics,
        )
        .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        let pons_eip7702_self_batch = expected
            .pons_eip7702_self_batch
            .as_ref()
            .map(|profile| {
                profile
                    .validate()
                    .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
                let account = find_observed_pin(
                    &observed.pins,
                    profile.account,
                    Some(profile.implementation),
                )
                .ok_or_else(|| {
                    PaperObserverError::Startup(
                        "fresh startup snapshot is missing the reviewed Pons EIP-7702 designator"
                            .into(),
                    )
                })?;
                let implementation =
                    find_observed_pin(&observed.pins, profile.implementation, None).ok_or_else(
                        || {
                            PaperObserverError::Startup(
                                "fresh startup snapshot is missing the reviewed Pons EIP-7702 implementation"
                                    .into(),
                            )
                        },
                    )?;
                if account.runtime_hash != profile.designator_hash
                    || account.code_bytes != Some(23)
                    || implementation.runtime_hash != profile.implementation_runtime_hash
                    || implementation.code_bytes.is_none_or(|length| length == 0)
                {
                    return Err(PaperObserverError::Startup(
                        "reviewed Pons EIP-7702 delegation pair is incomplete or drifted".into(),
                    ));
                }
                Ok(profile.clone())
            })
            .transpose()?;
        let hood_profile = match (expected.provenance, expected.hood_curve.as_ref()) {
            (ExpectedPinsProvenance::ReviewedProtocolPins, Some(profile)) => {
                profile
                    .validate()
                    .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
                let factory = profile.identity(HoodIdentityRole::Factory).ok_or_else(|| {
                    PaperObserverError::Startup("Hood factory identity missing".into())
                })?;
                if expected.hood_factory_runtime_hash != Some(factory.runtime_hash) {
                    return Err(PaperObserverError::Startup(
                        "Hood scalar factory pin and complete profile disagree".into(),
                    ));
                }
                Some(profile.clone())
            }
            (ExpectedPinsProvenance::ReviewedProtocolPins, None) => {
                return Err(PaperObserverError::Startup(
                    "reviewed production pins require the complete Hood profile".into(),
                ));
            }
            (_, Some(profile)) => {
                profile
                    .validate()
                    .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
                Some(profile.clone())
            }
            (_, None) => None,
        };
        if let Some(clanker) = expected.clanker_v4 {
            clanker.expected_profile()?;
        }
        let bankr_profile = expected
            .bankr_doppler_v4
            .map(ConfiguredBankrDopplerV4::expected_profile)
            .transpose()?;
        if let Some(profile) = bankr_profile {
            validate_bankr_profile_links(&expected, profile)?;
        }
        let observed_code = |address| ContractCodeSnapshot {
            address,
            runtime_code_hash: find_observed_pin(&observed.pins, address, None)
                .map_or(B256::ZERO, |pin| pin.runtime_hash),
        };
        if let Some(profile) = &hood_profile {
            let singleton = profile
                .identity(HoodIdentityRole::OwnerSafeSingleton)
                .ok_or_else(|| {
                    PaperObserverError::Startup("Hood Safe singleton pin missing".into())
                })?;
            for identity in &profile.identities {
                let implementation = (identity.role == HoodIdentityRole::OwnerSafeProxy)
                    .then_some(singleton.address);
                let observed_identity =
                    find_observed_pin(&observed.pins, identity.address, implementation)
                        .ok_or_else(|| {
                            PaperObserverError::Startup(format!(
                                "missing observed Hood {:?} identity",
                                identity.role
                            ))
                        })?;
                if observed_identity.runtime_hash != identity.runtime_hash
                    || observed_identity.code_bytes != Some(identity.code_bytes)
                {
                    return Err(PaperObserverError::Startup(format!(
                        "observed Hood {:?} identity drifted",
                        identity.role
                    )));
                }
            }
            validate_hood_protocol_snapshot(
                profile,
                observed.hood_protocol.as_ref().ok_or_else(|| {
                    PaperObserverError::Startup(
                        "fresh startup snapshot is missing Hood semantic state".into(),
                    )
                })?,
            )?;
        }
        let v3 = V3LaunchAtBirthAdapter::new(
            CHAIN_ID,
            &[
                observed_code(BOW_LAUNCH_FACTORY),
                observed_code(LAUNCHHOOD_V3_FACTORY),
                observed_code(LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION),
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
        let pons_identities = pons_profile
            .identities()
            .into_iter()
            .map(|required| {
                let observed = find_observed_pin(&observed.pins, required.address, None)
                    .ok_or_else(|| {
                        PaperObserverError::Startup(
                            "fresh startup snapshot is missing a reviewed Pons identity".into(),
                        )
                    })?;
                let code_bytes = observed.code_bytes.ok_or_else(|| {
                    PaperObserverError::Startup(
                        "fresh Pons startup observation is missing code length".into(),
                    )
                })?;
                if observed.runtime_hash != required.runtime_hash
                    || code_bytes != required.code_bytes
                {
                    return Err(PaperObserverError::Startup(
                        "fresh Pons startup observation disagrees with reviewed expected pins"
                            .into(),
                    ));
                }
                Ok(RuntimeIdentity {
                    address: observed.address,
                    code_bytes,
                    runtime_hash: observed.runtime_hash,
                })
            })
            .collect::<Result<Vec<_>, PaperObserverError>>()?;
        let pons = PonsAdapter::from_startup_identities(&pons_identities)
            .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        let wrappers = if expected.erc4337.is_some() {
            vec![WrapperKind::Direct, WrapperKind::Erc4337]
        } else {
            vec![WrapperKind::Direct]
        };
        let specs = paper_specs(&expected, &v4, &wrappers, &pons_profile)?;
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
        preload_clanker_prediction_profile()
            .map_err(|error| PaperObserverError::Startup(error.to_string()))?;
        Ok(Self {
            registry,
            v3,
            pons,
            pons_profile,
            pons_predictor,
            pons_eip7702_self_batch,
            curves,
            smart_accounts,
            bankr_profile,
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
            || self
                .pons_eip7702_self_batch
                .as_ref()
                .is_some_and(|profile| {
                    destination == profile.account && selector == PONS_EIP7702_OUTER_SELECTOR
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
        if let Some(profile) = &self.pons_eip7702_self_batch
            && destination == profile.account
            && selector(transaction.input()) == Some(PONS_EIP7702_OUTER_SELECTOR)
        {
            let designator = pons_eip7702_designator();
            let decoded = decode_pons_eip7702_self_batch(
                transaction,
                Eip7702ObservedDelegation {
                    account: profile.account,
                    designator: &designator,
                    designator_hash: profile.designator_hash,
                    implementation: profile.implementation,
                    implementation_runtime_hash: profile.implementation_runtime_hash,
                },
                profile,
            )
            .map_err(|error| PaperObserverError::Wrapper(error.to_string()))?;
            let mut observation = self.observe_call(
                decoded.tx_hash,
                decoded.provenance.authority,
                decoded.provenance.outer_signer,
                LeaderOrigin::Eip7702Authority,
                WrapperKind::Eip7702SelfBatch,
                decoded.provenance.inner_factory,
                decoded.provenance.inner_value,
                &decoded.inner_calldata,
            )?;
            let detail = observation.detail.as_object_mut().ok_or_else(|| {
                PaperObserverError::Adapter("Pons observation detail is not an object".into())
            })?;
            detail.insert(
                "eip7702_self_batch".into(),
                serde_json::to_value(decoded.provenance).expect("serializable EIP-7702 provenance"),
            );
            return Ok(Some(observation));
        }
        if let Some(pins) = &self.smart_accounts
            && destination == pins.entry_point().address
        {
            let observed = EntryPointCall {
                chain_id: CHAIN_ID,
                destination: pins.entry_point(),
                outer_bundler: outer_signer,
                calldata: transaction.input(),
            };
            let strict = decode_entry_point_v07_prevalidated(observed, pins.validated());
            let (leader, target, value, calldata, identity_pending) = match strict {
                Ok(unwrapped) => (
                    unwrapped.leader,
                    unwrapped.target,
                    unwrapped.value,
                    unwrapped.calldata,
                    false,
                ),
                Err(SmartAccountDecodeError::UnknownSmartAccount { .. }) => {
                    let profile = self.bankr_profile.ok_or_else(|| {
                        PaperObserverError::Wrapper(
                            "unpinned smart account is not eligible for Bankr discovery".into(),
                        )
                    })?;
                    let discovered = discover_entry_point_v07_erc7579(
                        observed,
                        pins.entry_point(),
                        SmartContractPin {
                            address: profile.airlock.address,
                            runtime_code_hash: profile.airlock.runtime_code_hash,
                        },
                    )
                    .map_err(|error| PaperObserverError::Wrapper(error.to_string()))?;
                    (
                        discovered.leader,
                        discovered.target,
                        discovered.value,
                        discovered.calldata,
                        true,
                    )
                }
                Err(error) => return Err(PaperObserverError::Wrapper(error.to_string())),
            };
            if self
                .bankr_profile
                .is_some_and(|profile| target == profile.airlock.address)
                && !validate_bankr_create_calldata_for_observation(&calldata)
            {
                return Err(PaperObserverError::Adapter(
                    "Bankr create calldata is not an exact reviewed profile".into(),
                ));
            }
            let mut observation = self.observe_call(
                *transaction.tx_hash(),
                leader,
                outer_signer,
                LeaderOrigin::Erc4337Sender,
                WrapperKind::Erc4337,
                target,
                value,
                &calldata,
            )?;
            if identity_pending {
                let detail = observation.detail.as_object_mut().ok_or_else(|| {
                    PaperObserverError::Adapter("Bankr discovery detail is not an object".into())
                })?;
                detail.insert(
                    "smart_account_identity".into(),
                    serde_json::Value::String("receipt_block_eip7702_verification_required".into()),
                );
            }
            return Ok(Some(observation));
        }
        if self
            .bankr_profile
            .is_some_and(|profile| destination == profile.airlock.address)
            && !validate_bankr_create_calldata_for_observation(transaction.input())
        {
            return Err(PaperObserverError::Adapter(
                "Bankr create calldata is not an exact reviewed profile".into(),
            ));
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
        let (
            launchpad,
            kind,
            planning_mode,
            action,
            predicted_token,
            predicted_pool,
            predicted_pool_id,
            detail,
        ) = match (spec.id, spec.family) {
            (LaunchpadId::Noxa, AdapterKind::V3LaunchAtBirth) => {
                let intent = decode_launch_call(input, value)
                    .ok_or_else(|| PaperObserverError::Adapter("malformed Noxa launch".into()))?;
                (
                    LaunchpadId::Noxa,
                    "launch",
                    ExecutionMode::PaperOnly,
                    Some(ActionKind::Launch),
                    None,
                    None,
                    None,
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
                let predicted_token = observed
                    .predicted_market
                    .as_ref()
                    .map(|market| market.token);
                let predicted_pool = observed.predicted_market.as_ref().map(|market| market.pool);
                (
                    observed.launchpad,
                    "launch",
                    ExecutionMode::ExecutionGated,
                    Some(ActionKind::Launch),
                    predicted_token,
                    predicted_pool,
                    None,
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
                let (predicted_token, predicted_pool_id) = if launchpad == LaunchpadId::BankrDoppler
                {
                    let prediction = predict_bankr_create_identity(
                        input,
                        self.bankr_profile.ok_or_else(|| {
                            PaperObserverError::Adapter(
                                "Bankr prediction profile is unavailable".into(),
                            )
                        })?,
                    )
                    .map_err(|error| PaperObserverError::Adapter(error.to_string()))?;
                    if prediction.create_profile_version
                        == crate::BankrCreateProfileVersion::CurveTicksV5
                        && wrapper != WrapperKind::Erc4337
                    {
                        return Err(PaperObserverError::Adapter(
                            "Bankr CurveTicksV5 is admitted only through the evidenced ERC7579 envelope"
                                .into(),
                        ));
                    }
                    (Some(prediction.token), Some(prediction.pool_id))
                } else {
                    match &observed {
                        crate::launchpad_adapters::LaunchObservation::OpaqueLaunch {
                            predicted_market: Some(market),
                            ..
                        } => (Some(market.token), Some(market.pool_id)),
                        _ => (None, None),
                    }
                };
                (
                    launchpad,
                    "discovery",
                    mode,
                    Some(ActionKind::Launch),
                    predicted_token,
                    None,
                    predicted_pool_id,
                    serde_json::to_value(observed).expect("serializable V4 observation"),
                )
            }
            (LaunchpadId::Pons, AdapterKind::V3LaunchAtBirth) => {
                let runtime_hash = self
                    .pons_profile
                    .identity(destination)
                    .ok_or(PaperObserverError::UnknownDispatch)?
                    .runtime_hash;
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
                let predicted = if observed.generation == crate::pons::PonsGeneration::Current {
                    Some(
                        self.pons_predictor
                            .predict(&observed, outer_signer)
                            .map_err(|error| PaperObserverError::Adapter(error.to_string()))?,
                    )
                } else {
                    None
                };
                (
                    LaunchpadId::Pons,
                    "launch",
                    if observed.generation == crate::pons::PonsGeneration::Current {
                        ExecutionMode::ExecutionGated
                    } else {
                        ExecutionMode::DiscoveryOnly
                    },
                    Some(ActionKind::Launch),
                    predicted.map(|market| market.token),
                    predicted.map(|market| market.pool),
                    None,
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
                let predicted_token = if observed.protocol == LaunchpadId::HoodFun
                    && observed.action == ActionKind::Launch
                {
                    Some(
                        predict_hood_token_address(leader, input)
                            .map_err(|error| PaperObserverError::Adapter(error.to_string()))?,
                    )
                } else {
                    observed.token
                };
                (
                    observed.protocol,
                    "observation",
                    if observed.paper_plan_supported {
                        ExecutionMode::PaperOnly
                    } else {
                        ExecutionMode::DiscoveryOnly
                    },
                    Some(observed.action),
                    predicted_token,
                    None,
                    None,
                    serde_json::to_value(observed).expect("serializable curve observation"),
                )
            }
            (LaunchpadId::Flap, AdapterKind::FlapPortal) => {
                if input.len() > MAX_OPAQUE_CALLDATA || input.len() < 4 + 32 {
                    return Err(PaperObserverError::Adapter(
                        "malformed Flap launch envelope".into(),
                    ));
                }
                if destination == FLAP_VAULT_PORTAL_PROXY
                    && !is_bounded_flap_vault_abi_envelope(input)
                {
                    return Err(PaperObserverError::Adapter(
                        "malformed Flap VaultPortal launch envelope".into(),
                    ));
                }
                (
                    LaunchpadId::Flap,
                    "launch_discovery",
                    ExecutionMode::DiscoveryOnly,
                    Some(ActionKind::Launch),
                    None,
                    None,
                    None,
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
            action,
            predicted_token,
            predicted_pool,
            predicted_pool_id,
            planning_mode,
            live_execution_enabled: false,
            feed_sequence: None,
            l1_block_number: None,
            l1_timestamp: None,
            observer_received_unix_ns: None,
            observer_latency_ns: None,
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
                if self.pons_eip7702_self_batch.is_some() {
                    vec![WrapperKind::Direct, WrapperKind::Eip7702SelfBatch]
                } else {
                    wrappers(false)
                },
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
    pons_profile: &PonsExpectedProfile,
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
    let mut pons_pins: Vec<_> = pons_profile
        .identities()
        .into_iter()
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
    if let Some(profile) = &expected.pons_eip7702_self_batch {
        pons_pins.extend([
            contract_pin(
                ContractRole::ProtocolDependency,
                profile.account,
                Some(profile.implementation),
                profile.designator_hash,
            ),
            contract_pin(
                ContractRole::Implementation,
                profile.implementation,
                None,
                profile.implementation_runtime_hash,
            ),
        ]);
    }
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
            with_shared_v3(vec![
                contract_pin(
                    ContractRole::LaunchFactory,
                    LAUNCHHOOD_V3_FACTORY,
                    None,
                    expected.launchhood_v3_factory_runtime_hash,
                ),
                contract_pin(
                    ContractRole::Implementation,
                    expected.launchhood_v3_token_implementation.address,
                    None,
                    expected.launchhood_v3_token_implementation.runtime_hash,
                ),
            ]),
            RouteKind::V3SingleHop,
        ),
        spec(
            LaunchpadId::Pons,
            AdapterKind::V3LaunchAtBirth,
            {
                let mut pons_keys = keys(
                    &[
                        (PONS_LEGACY_FACTORY, PONS_LAUNCH_SELECTOR),
                        (PONS_CURRENT_FACTORY, PONS_LAUNCH_SELECTOR),
                    ],
                    &direct,
                );
                if expected.pons_eip7702_self_batch.is_some() {
                    pons_keys.push(DispatchKey {
                        destination: PONS_CURRENT_FACTORY,
                        selector: PONS_LAUNCH_SELECTOR,
                        wrapper: WrapperKind::Eip7702SelfBatch,
                    });
                }
                pons_keys
            },
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
                    (FLAP_VAULT_PORTAL_PROXY, FLAP_VAULT_LAUNCH_SELECTOR),
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
        if entry.launchpad == LaunchpadId::Clanker
            && let Some(configured) = expected.clanker_v4
        {
            let profile = configured.expected_profile()?;
            pins.extend([
                contract_pin(
                    ContractRole::ProtocolDependency,
                    CLANKER_DEPLOYER,
                    None,
                    configured.deployer_runtime_hash,
                ),
                contract_pin(
                    ContractRole::ProtocolDependency,
                    profile.pool_manager.address,
                    None,
                    profile.pool_manager.runtime_code_hash,
                ),
                contract_pin(
                    ContractRole::ProtocolDependency,
                    profile.hook.address,
                    None,
                    profile.hook.runtime_code_hash,
                ),
                contract_pin(
                    ContractRole::ProtocolDependency,
                    profile.locker.address,
                    None,
                    profile.locker.runtime_code_hash,
                ),
                contract_pin(
                    ContractRole::ProtocolDependency,
                    profile.mev_module.address,
                    None,
                    profile.mev_module.runtime_code_hash,
                ),
                contract_pin(
                    ContractRole::ProtocolDependency,
                    profile.extension.address,
                    None,
                    profile.extension.runtime_code_hash,
                ),
            ]);
        }
        if entry.launchpad == LaunchpadId::BankrDoppler
            && let Some(configured) = expected.bankr_doppler_v4
        {
            let profile = configured.expected_profile()?;
            pins.extend([
                contract_pin(
                    ContractRole::ProtocolDependency,
                    profile.weth.address,
                    None,
                    profile.weth.runtime_code_hash,
                ),
                contract_pin(
                    ContractRole::ProtocolDependency,
                    profile.pool_manager.address,
                    None,
                    profile.pool_manager.runtime_code_hash,
                ),
                contract_pin(
                    ContractRole::ProtocolDependency,
                    profile.initializer.address,
                    None,
                    profile.initializer.runtime_code_hash,
                ),
                contract_pin(
                    ContractRole::ProtocolDependency,
                    profile.rehype_hook.address,
                    None,
                    profile.rehype_hook.runtime_code_hash,
                ),
                contract_pin(
                    ContractRole::ProtocolDependency,
                    profile.token_factory.address,
                    None,
                    profile.token_factory.runtime_code_hash,
                ),
                contract_pin(
                    ContractRole::Implementation,
                    profile.token_implementation.address,
                    None,
                    profile.token_implementation.runtime_code_hash,
                ),
                contract_pin(
                    ContractRole::ProtocolDependency,
                    profile.governance_factory.address,
                    None,
                    profile.governance_factory.runtime_code_hash,
                ),
                contract_pin(
                    ContractRole::ProtocolDependency,
                    profile.liquidity_migrator.address,
                    None,
                    profile.liquidity_migrator.runtime_code_hash,
                ),
            ]);
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
                    (HOOD_FACTORY, HOOD_BUY_FOR_SELECTOR),
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

/// Validate only the ABI envelope proven for the source-unverified VaultPortal
/// implementation: exact selector, one top-level dynamic argument at the
/// canonical offset, whole ABI words, and a bounded non-empty payload. The
/// tuple fields intentionally remain opaque and unavailable to prediction or
/// quoting until the implementation semantics are independently verified.
fn is_bounded_flap_vault_abi_envelope(input: &[u8]) -> bool {
    const WORD: usize = 32;

    if input.len() > MAX_OPAQUE_CALLDATA
        || input.len() < 4 + 2 * WORD
        || !(input.len() - 4).is_multiple_of(WORD)
        || input.get(..4) != Some(FLAP_VAULT_LAUNCH_SELECTOR.as_slice())
        || word_as_usize(&input[4..4 + WORD]) != Some(WORD)
    {
        return false;
    }
    input[4 + WORD..].iter().any(|byte| *byte != 0)
}

fn word_as_usize(word: &[u8]) -> Option<usize> {
    if word.len() != 32 || word[..24].iter().any(|byte| *byte != 0) {
        return None;
    }
    usize::try_from(u64::from_be_bytes(word[24..].try_into().ok()?)).ok()
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

fn validate_launchhood_identity(
    expected: &PaperExpectedPins,
    observed: &[ObservedRuntimePin],
) -> Result<(), PaperObserverError> {
    let implementation = expected.launchhood_v3_token_implementation;
    if expected.launchhood_v3_factory_runtime_hash != LAUNCHHOOD_V3_FACTORY_RUNTIME_KECCAK256
        || implementation.address != LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION
        || implementation.code_bytes != LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_CODE_BYTES
        || implementation.runtime_hash != LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_RUNTIME_KECCAK256
    {
        return Err(PaperObserverError::Startup(
            "LaunchHood factory immutable or token implementation authority drifted".into(),
        ));
    }
    let observed_implementation = find_observed_pin(observed, implementation.address, None)
        .ok_or_else(|| {
            PaperObserverError::Startup(
                "fresh startup snapshot is missing the LaunchHood token implementation".into(),
            )
        })?;
    if observed_implementation.runtime_hash != implementation.runtime_hash
        || observed_implementation.code_bytes != Some(implementation.code_bytes)
    {
        return Err(PaperObserverError::Startup(
            "fresh LaunchHood token implementation identity drifted".into(),
        ));
    }
    Ok(())
}

fn validate_document_pair(
    expected: &PaperExpectedPins,
    observed: &PaperObservedStartupSnapshot,
) -> Result<(), PaperObserverError> {
    if expected.schema_version != 4 || observed.schema_version != 4 || observed.chain_id != CHAIN_ID
    {
        return Err(PaperObserverError::Startup(
            "unsupported pin schema or chain".into(),
        ));
    }
    let valid_provenance = match (expected.provenance, observed.provenance) {
        (
            ExpectedPinsProvenance::ReviewedProtocolPins,
            ObservedPinsProvenance::StartupObservation,
        ) => {
            expected.fixture_id.is_none()
                && observed.fixture_id.is_none()
                && expected.reviewed_at.is_some_and(valid_pin_boundary)
                && observed.observed_at.is_some_and(valid_pin_boundary)
        }
        (
            ExpectedPinsProvenance::SyntheticOfflineFixture,
            ObservedPinsProvenance::SyntheticOfflineFixture,
        ) => {
            expected.fixture_id.is_some()
                && expected.fixture_id == observed.fixture_id
                && expected.reviewed_at.is_none()
                && observed.observed_at.is_none()
        }
        _ => false,
    };
    if !valid_provenance {
        return Err(PaperObserverError::Startup(
            "expected and observed pin provenance is incompatible".into(),
        ));
    }
    if let (Some(boundary), Some(hood)) = (observed.observed_at, &observed.hood_protocol)
        && hood.l2_block_number != boundary.l2_block_number
    {
        return Err(PaperObserverError::Startup(
            "Hood semantic snapshot and runtime pins use different block boundaries".into(),
        ));
    }
    if expected.provenance == ExpectedPinsProvenance::ReviewedProtocolPins
        && (expected.clanker_v4.is_none()
            || expected.bankr_doppler_v4.is_none()
            || expected.hood_curve.is_none())
    {
        return Err(PaperObserverError::Startup(
            "reviewed production pins require complete Clanker, Bankr/Doppler, and Hood profiles"
                .into(),
        ));
    }
    Ok(())
}

fn valid_pin_boundary(boundary: PinBlockBoundary) -> bool {
    boundary.l2_block_number != 0
        && boundary.l2_block_hash != B256::ZERO
        && boundary.l1_block_number != 0
        && boundary.block_timestamp != 0
}

fn validate_hood_protocol_snapshot(
    profile: &HoodExpectedProfile,
    observed: &HoodProtocolSnapshot,
) -> Result<(), PaperObserverError> {
    let address = |role| {
        profile
            .identity(role)
            .map(|identity| identity.address)
            .ok_or_else(|| PaperObserverError::Startup(format!("Hood {role:?} identity missing")))
    };
    let semantic = profile.semantic;
    let config = observed.config;
    let exact = observed.l2_block_number != 0
        && observed.factory == semantic.factory
        && observed.migrator == semantic.active_migrator
        && observed.fallback_factory == semantic.fallback_factory
        && observed.weth == semantic.weth
        && observed.owner == profile.owner
        && observed.pending_owner == profile.pending_owner
        && observed.owner_safe_singleton == address(HoodIdentityRole::OwnerSafeSingleton)?
        && config.virtual_eth_seed == semantic.virtual_eth_seed
        && config.creation_fee == semantic.creation_fee
        && config.default_trade_fee_bps == semantic.default_trade_fee_bps
        && config.migration_fee == semantic.migration_fee
        && config.migration_fee_bps == semantic.migration_fee_bps
        && config.guard_blocks == semantic.guard_blocks
        && config.guard_max_wallet_bps == semantic.current_guard_max_wallet_bps
        && config.creator_fee_share_bps == semantic.creator_fee_share_bps
        && config.vanity_enforced == semantic.vanity_enforced
        && observed.migrator_launchpad == semantic.factory
        && observed.migrator_position_manager == address(HoodIdentityRole::PositionManager)?
        && observed.migrator_locker == address(HoodIdentityRole::Locker)?
        && observed.migrator_weth == semantic.weth
        && observed.migrator_protocol == profile.owner
        && observed.migrator_creator_share_bps == profile.migrator_creator_share_bps
        && observed.migrator_v3_fee == profile.v3_fee
        && observed.locker_position_manager == address(HoodIdentityRole::PositionManager)?
        && observed.locker_weth == semantic.weth
        && observed.locker_burn_bps == profile.locker_token_fee_burn_bps
        && observed.position_manager_factory == address(HoodIdentityRole::V3Factory)?
        && observed.position_manager_weth == semantic.weth
        && observed.router_factory == address(HoodIdentityRole::V3Factory)?
        && observed.router_weth == semantic.weth;
    if !exact {
        return Err(PaperObserverError::Startup(
            "fresh Hood semantic snapshot disagrees with reviewed production profile".into(),
        ));
    }
    Ok(())
}

fn validate_bankr_profile_links(
    expected: &PaperExpectedPins,
    profile: BankrDopplerExpectedProfile,
) -> Result<(), PaperObserverError> {
    if expected.bankr_doppler_calls.len() != 1 {
        return Err(PaperObserverError::Startup(
            "reviewed Bankr profile requires exactly one Airlock call".into(),
        ));
    }
    let call = expected.bankr_doppler_calls[0];
    if call.destination != profile.airlock.address
        || call.runtime_hash != profile.airlock.runtime_code_hash
        || call.selector != BANKR_CREATE_SELECTOR
    {
        return Err(PaperObserverError::Startup(
            "Bankr dispatch disagrees with the independently reviewed profile".into(),
        ));
    }
    let smart = expected.erc4337.as_ref().ok_or_else(|| {
        PaperObserverError::Startup("reviewed Bankr profile requires ERC-4337 pins".into())
    })?;
    if smart.entry_point_runtime_hash != profile.entry_point.runtime_code_hash
        || smart.accounts.len() != 1
    {
        return Err(PaperObserverError::Startup(
            "Bankr EntryPoint or account set disagrees with the reviewed profile".into(),
        ));
    }
    let account = smart.accounts[0];
    let delegated = profile
        .smart_account
        .delegation_implementation
        .ok_or_else(|| PaperObserverError::Startup("Bankr delegation pin is missing".into()))?;
    if account.account != profile.smart_account.account.address
        || account.runtime_hash != profile.smart_account.account.runtime_code_hash
        || account.execution_profile != profile.smart_account.execution_profile
        || account.factory.is_some()
        || account.factory_runtime_hash.is_some()
        || account.delegation_implementation != Some(delegated.address)
        || account.delegation_runtime_hash != Some(delegated.runtime_code_hash)
    {
        return Err(PaperObserverError::Startup(
            "Bankr account execution/delegation pair disagrees with the reviewed profile".into(),
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
            || pin.implementation == Some(Address::ZERO)
            || (observed.provenance == ObservedPinsProvenance::StartupObservation
                && pin.code_bytes.is_none_or(|bytes| bytes == 0))
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
    use sha2::{Digest, Sha256};

    use super::*;

    #[derive(Deserialize)]
    struct BankrV4RawFrameFixture {
        frames: Vec<BankrV4RawFrame>,
    }

    #[derive(Deserialize)]
    struct BankrV4RawFrame {
        window: String,
        line: u64,
        tx_hash: B256,
        envelope: String,
        source_path: String,
        payload_sha256: String,
        received_unix_ns: u64,
        payload: String,
    }

    #[derive(Deserialize)]
    struct StonksV3RawRecord {
        received_unix_ns: u64,
        payload: String,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StrictPaperReconciliationRequest {
        tx_hash: B256,
        launchpad: LaunchpadId,
        feed_sequence: u64,
        l1_block_number: u64,
        l1_timestamp: u64,
        evidence_source: String,
        initial_decision_dependency: bool,
        wrapper: WrapperKind,
    }

    fn bankr_v4_raw_frames() -> BankrV4RawFrameFixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v4-finaltuple-raw-frames.json"
        ))
        .unwrap()
    }

    fn bankr_v4_reverse_raw_frames() -> BankrV4RawFrameFixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v4-reverse-raw-frames.json"
        ))
        .unwrap()
    }

    fn bankr_v5_raw_frames() -> BankrV4RawFrameFixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v5-fresh-raw-frame.json"
        ))
        .unwrap()
    }

    fn bankr_v5_reverse_raw_frames() -> Vec<StonksV3RawRecord> {
        #[derive(Deserialize)]
        struct Fixture {
            frames: Vec<StonksV3RawRecord>,
        }
        serde_json::from_str::<Fixture>(include_str!(
            "../tests/fixtures/bankr-doppler-v5-reverse-raw-frames.json"
        ))
        .unwrap()
        .frames
    }

    fn startup() -> (PaperExpectedPins, PaperObservedStartupSnapshot) {
        let expected = PaperExpectedPins {
            schema_version: 4,
            document_role: ExpectedPinsDocumentRole::ExpectedProtocolPins,
            provenance: ExpectedPinsProvenance::SyntheticOfflineFixture,
            fixture_id: Some("launchpad-paper-offline-v3".into()),
            reviewed_at: None,
            pons_v3: ConfiguredPonsV3 {
                identities: PonsExpectedProfile::production()
                    .identities()
                    .into_iter()
                    .map(|identity| ConfiguredPonsRuntimeIdentity {
                        address: identity.address,
                        code_bytes: identity.code_bytes,
                        runtime_hash: identity.runtime_hash,
                    })
                    .collect(),
                prediction: crate::pons_predict::PonsPredictionSemantics::production(),
            },
            pons_eip7702_self_batch: None,
            bow_factory_runtime_hash: crate::robinhood::BOW_LAUNCH_FACTORY_RUNTIME_KECCAK256,
            launchhood_v3_factory_runtime_hash: LAUNCHHOOD_V3_FACTORY_RUNTIME_KECCAK256,
            launchhood_v3_token_implementation: ConfiguredRuntimeIdentity {
                address: LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION,
                code_bytes: LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_CODE_BYTES,
                runtime_hash: LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_RUNTIME_KECCAK256,
            },
            hood_factory_runtime_hash: Some(B256::with_last_byte(1)),
            hood_curve: None,
            leavehood_factory_proxy_runtime_hash: Some(B256::with_last_byte(2)),
            leavehood_factory_implementation_runtime_hash: Some(B256::with_last_byte(3)),
            leavehood_core_proxy_runtime_hash: Some(B256::with_last_byte(4)),
            leavehood_core_implementation_runtime_hash: Some(B256::with_last_byte(5)),
            klik_factory_runtime_hash: Some(B256::with_last_byte(6)),
            trench_proxy_runtime_hash: Some(B256::with_last_byte(7)),
            trench_implementation_runtime_hash: Some(B256::with_last_byte(8)),
            clanker_v4: None,
            bankr_doppler_v4: None,
            bankr_doppler_calls: vec![ConfiguredCallPin {
                destination: Address::with_last_byte(0xb0),
                runtime_hash: B256::with_last_byte(9),
                selector: [0xba, 0x6b, 0x00, 0x01],
            }],
            erc4337: None,
        };
        let mut snapshot = PaperObservedStartupSnapshot {
            schema_version: 4,
            document_role: ObservedPinsDocumentRole::ObservedStartupSnapshot,
            provenance: ObservedPinsProvenance::SyntheticOfflineFixture,
            fixture_id: Some("launchpad-paper-offline-v3".into()),
            chain_id: CHAIN_ID,
            observed_at: None,
            pins: vec![
                observed(NOXA_LAUNCH_FACTORY, None, NOXA_FACTORY_RUNTIME_KECCAK256),
                observed(
                    ACTIVE_NOXA_LAUNCH_FACTORY,
                    None,
                    ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256,
                ),
                observed(
                    BOW_LAUNCH_FACTORY,
                    None,
                    crate::robinhood::BOW_LAUNCH_FACTORY_RUNTIME_KECCAK256,
                ),
                observed(
                    LAUNCHHOOD_V3_FACTORY,
                    None,
                    LAUNCHHOOD_V3_FACTORY_RUNTIME_KECCAK256,
                ),
                observed(
                    LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION,
                    None,
                    LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_RUNTIME_KECCAK256,
                ),
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
            pons_v3_semantics: Some(crate::pons_predict::PonsPredictionSemantics::production()),
            hood_protocol: None,
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

    #[test]
    fn pons_eip7702_profile_is_a_distinct_inner_dispatch_and_requires_both_pins() {
        let (mut expected, mut observed) = startup();
        let profile = Eip7702SelfBatchExpectedPins::production();
        expected.pons_eip7702_self_batch = Some(profile.clone());
        observed.pins.extend([
            ObservedRuntimePin {
                address: profile.account,
                implementation: Some(profile.implementation),
                runtime_hash: profile.designator_hash,
                code_bytes: Some(23),
            },
            ObservedRuntimePin {
                address: profile.implementation,
                implementation: None,
                runtime_hash: profile.implementation_runtime_hash,
                code_bytes: Some(1),
            },
        ]);
        let observer =
            PaperLaunchpadObserver::from_startup_snapshots(expected.clone(), observed.clone())
                .unwrap();
        let spec = observer
            .registry
            .dispatch(
                Some(CHAIN_ID),
                BoundedCall {
                    destination: PONS_CURRENT_FACTORY,
                    calldata: &[
                        PONS_LAUNCH_SELECTOR[0],
                        PONS_LAUNCH_SELECTOR[1],
                        PONS_LAUNCH_SELECTOR[2],
                        PONS_LAUNCH_SELECTOR[3],
                    ],
                    wrapper: WrapperKind::Eip7702SelfBatch,
                    depth: 1,
                },
            )
            .unwrap();
        assert_eq!(spec.id, LaunchpadId::Pons);
        assert!(
            observer
                .registry
                .dispatch(
                    Some(CHAIN_ID),
                    BoundedCall {
                        destination: profile.account,
                        calldata: &PONS_EIP7702_OUTER_SELECTOR,
                        wrapper: WrapperKind::Eip7702SelfBatch,
                        depth: 1,
                    },
                )
                .is_err()
        );
        let pons = observer
            .capabilities()
            .into_iter()
            .find(|capability| capability.launchpad == LaunchpadId::Pons)
            .unwrap();
        assert_eq!(
            pons.wrappers,
            vec![WrapperKind::Direct, WrapperKind::Eip7702SelfBatch]
        );
        assert!(!pons.live_execution_enabled);

        observed
            .pins
            .retain(|pin| pin.address != profile.implementation);
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected, observed).is_err());
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
            code_bytes: if address == LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION {
                Some(LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_CODE_BYTES)
            } else {
                PonsAdapter::required_startup_identities()
                    .iter()
                    .find(|identity| identity.address == address)
                    .map(|identity| identity.code_bytes)
            },
        }
    }

    fn signed_transaction(destination: Address, input: Vec<u8>) -> TxEnvelope {
        signed_transaction_with_value(destination, input, U256::ZERO)
    }

    fn signed_transaction_with_value(
        destination: Address,
        input: Vec<u8>,
        value: U256,
    ) -> TxEnvelope {
        let transaction = TxEip1559 {
            chain_id: CHAIN_ID,
            nonce: 0,
            gas_limit: 200_000,
            max_fee_per_gas: 1,
            max_priority_fee_per_gas: 0,
            to: TxKind::Call(destination),
            value,
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

    fn hood_launch_input() -> Vec<u8> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/hood-launch-atomic-buy-live-proof.json"
        ))
        .unwrap();
        hex::decode(
            fixture["transaction"]["input"]
                .as_str()
                .unwrap()
                .strip_prefix("0x")
                .unwrap(),
        )
        .unwrap()
    }

    fn observer() -> PaperLaunchpadObserver {
        let (expected, observed) = startup();
        PaperLaunchpadObserver::from_startup_snapshots(expected, observed).unwrap()
    }

    #[test]
    fn real_flap_direct_and_vault_launches_remain_strict_discovery_only() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/flap-anchored-live-proofs.json"
        ))
        .unwrap();
        let observer = observer();

        for proof in fixture["proofs"].as_array().unwrap() {
            let transaction = &proof["transaction"];
            let input = hex::decode(
                transaction["input"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("0x")
                    .unwrap(),
            )
            .unwrap();
            let tx_hash: B256 = transaction["hash"].as_str().unwrap().parse().unwrap();
            let signer: Address = transaction["from"].as_str().unwrap().parse().unwrap();
            let destination: Address = transaction["to"].as_str().unwrap().parse().unwrap();
            let value: U256 = transaction["value"].as_str().unwrap().parse().unwrap();
            let observation = observer
                .observe_call(
                    tx_hash,
                    signer,
                    signer,
                    LeaderOrigin::DirectSigner,
                    WrapperKind::Direct,
                    destination,
                    value,
                    &input,
                )
                .unwrap();

            assert_eq!(observation.tx_hash, tx_hash);
            assert_eq!(observation.leader, signer);
            assert_eq!(observation.outer_signer, signer);
            assert_eq!(observation.leader_origin, LeaderOrigin::DirectSigner);
            assert_eq!(observation.planning_mode, ExecutionMode::DiscoveryOnly);
            assert_eq!(observation.action, Some(ActionKind::Launch));
            assert_eq!(observation.predicted_token, None);
            assert_eq!(observation.predicted_pool, None);
            assert_eq!(observation.predicted_pool_id, None);
            assert!(!observation.live_execution_enabled);

            if proof["kind"] == "vault" {
                assert_eq!(destination, FLAP_VAULT_PORTAL_PROXY);
                assert_eq!(&input[..4], FLAP_VAULT_LAUNCH_SELECTOR);
                let mut trailing_junk = input.clone();
                trailing_junk.push(0);
                assert!(
                    observer
                        .observe_call(
                            tx_hash,
                            signer,
                            signer,
                            LeaderOrigin::DirectSigner,
                            WrapperKind::Direct,
                            destination,
                            value,
                            &trailing_junk,
                        )
                        .is_err()
                );
            }
        }

        assert!(observer.might_observe(FLAP_VAULT_PORTAL_PROXY, &FLAP_VAULT_LAUNCH_SELECTOR));
        assert!(!observer.might_observe(FLAP_VAULT_PORTAL_PROXY, &FLAP_STANDARD_LAUNCH_SELECTOR));
        assert!(!observer.might_observe(FLAP_VAULT_PORTAL_PROXY, &FLAP_TAX_V3_LAUNCH_SELECTOR));
        let capability = observer
            .capabilities()
            .into_iter()
            .find(|capability| capability.launchpad == LaunchpadId::Flap)
            .unwrap();
        assert!(capability.discovery_enabled);
        assert!(!capability.paper_plan_supported);
        assert!(!capability.live_execution_enabled);
    }

    fn bankr_v4_observer() -> PaperLaunchpadObserver {
        let (mut expected, mut observed_snapshot) = startup();
        let profile = BankrDopplerExpectedProfile::production();
        expected.bankr_doppler_v4 = Some(ConfiguredBankrDopplerV4 {
            airlock_runtime_hash: profile.airlock.runtime_code_hash,
            pool_manager_runtime_hash: profile.pool_manager.runtime_code_hash,
            initializer_runtime_hash: profile.initializer.runtime_code_hash,
            rehype_hook_runtime_hash: profile.rehype_hook.runtime_code_hash,
            token_factory_runtime_hash: profile.token_factory.runtime_code_hash,
            token_implementation_runtime_hash: profile.token_implementation.runtime_code_hash,
            governance_factory_runtime_hash: profile.governance_factory.runtime_code_hash,
            liquidity_migrator_runtime_hash: profile.liquidity_migrator.runtime_code_hash,
            standard_lp_fee_ppm: profile.standard_lp_fee_ppm,
            max_lp_fee_ppm: profile.max_lp_fee_ppm,
            hook_fee_denominator_ppm: profile.hook_fee_denominator_ppm,
            hook_start_fee_ppm: profile.hook_start_fee_ppm,
            hook_end_fee_ppm: profile.hook_end_fee_ppm,
            hook_duration_seconds: profile.hook_duration_seconds,
            quote_delay_guard_seconds: profile.quote_delay_guard_seconds,
            tick_spacing: profile.tick_spacing,
            pool_allocation_bps: profile.pool_allocation_bps,
            primary_curve_share_bps: profile.primary_curve_share_bps,
            secondary_curve_share_bps: profile.secondary_curve_share_bps,
            creator_beneficiary_bps: profile.creator_beneficiary_bps,
            protocol_beneficiary_bps: profile.protocol_beneficiary_bps,
        });
        expected.bankr_doppler_calls = vec![ConfiguredCallPin {
            destination: profile.airlock.address,
            runtime_hash: profile.airlock.runtime_code_hash,
            selector: BANKR_CREATE_SELECTOR,
        }];
        let delegation = profile.smart_account.delegation_implementation.unwrap();
        expected.erc4337 = Some(ConfiguredSmartAccounts {
            entry_point_runtime_hash: profile.entry_point.runtime_code_hash,
            accounts: vec![ConfiguredSmartAccount {
                account: profile.smart_account.account.address,
                runtime_hash: profile.smart_account.account.runtime_code_hash,
                execution_profile: profile.smart_account.execution_profile,
                factory: None,
                factory_runtime_hash: None,
                delegation_implementation: Some(delegation.address),
                delegation_runtime_hash: Some(delegation.runtime_code_hash),
            }],
        });
        observed_snapshot.pins.extend([
            observed(
                profile.airlock.address,
                None,
                profile.airlock.runtime_code_hash,
            ),
            observed(
                profile.pool_manager.address,
                None,
                profile.pool_manager.runtime_code_hash,
            ),
            observed(
                profile.initializer.address,
                None,
                profile.initializer.runtime_code_hash,
            ),
            observed(
                profile.rehype_hook.address,
                None,
                profile.rehype_hook.runtime_code_hash,
            ),
            observed(
                profile.token_factory.address,
                None,
                profile.token_factory.runtime_code_hash,
            ),
            observed(
                profile.token_implementation.address,
                None,
                profile.token_implementation.runtime_code_hash,
            ),
            observed(
                profile.governance_factory.address,
                None,
                profile.governance_factory.runtime_code_hash,
            ),
            observed(
                profile.liquidity_migrator.address,
                None,
                profile.liquidity_migrator.runtime_code_hash,
            ),
            observed(
                profile.entry_point.address,
                None,
                profile.entry_point.runtime_code_hash,
            ),
            observed(
                profile.smart_account.account.address,
                Some(delegation.address),
                profile.smart_account.account.runtime_code_hash,
            ),
            observed(delegation.address, None, delegation.runtime_code_hash),
        ]);
        PaperLaunchpadObserver::from_startup_snapshots(expected, observed_snapshot).unwrap()
    }

    #[test]
    fn raw_signed_nitro_fixture_reaches_registry_adapter_and_observation_output() {
        let observer = observer();
        let transaction = signed_transaction(HOOD_FACTORY, hood_launch_input());
        let expected_leader = transaction.recover_signer().unwrap();
        let mut runtime = PaperFeedRuntime::new(observer);
        let source_received_unix_ns = 1;
        let report = runtime
            .decode_received_at(&feed(&transaction), source_received_unix_ns)
            .unwrap();

        assert_eq!(report.decode.signed_transactions, 1);
        assert_eq!(report.transactions.len(), 1);
        assert_eq!(report.transactions[0].tx_hash, *transaction.tx_hash());
        assert_eq!(report.transactions[0].feed_sequence, 42);
        assert_eq!(report.transactions[0].l1_block_number, 9);
        assert_eq!(report.transactions[0].l1_timestamp, 10);
        assert_eq!(
            report.transactions[0].frame_received_unix_ns,
            source_received_unix_ns
        );
        assert!(report.rejections.is_empty());
        assert_eq!(report.observations.len(), 1);
        assert_eq!(report.observations[0].launchpad, LaunchpadId::HoodFun);
        assert!(report.observations[0].predicted_token.is_some());
        assert_eq!(report.observations[0].leader, expected_leader);
        assert_eq!(report.observations[0].feed_sequence, Some(42));
        assert_eq!(report.observations[0].l1_block_number, Some(9));
        assert_eq!(report.observations[0].l1_timestamp, Some(10));
        assert_eq!(
            report.observations[0].observer_received_unix_ns,
            Some(source_received_unix_ns)
        );
        assert!(
            report.observations[0]
                .observer_latency_ns
                .is_some_and(|value| value > 0)
        );
        assert_eq!(
            report.observations[0].leader_origin,
            LeaderOrigin::DirectSigner
        );
        assert!(!report.observations[0].live_execution_enabled);
        assert_eq!(report.trade_plans.len(), 1);
        assert_eq!(report.trade_plans[0].feed_sequence, 42);
        assert!(!report.trade_plans[0].leader_amounts_reused);
        assert!(report.trade_plans[0].expected_output.is_none());
        assert!(report.trade_plans[0].min_receive.is_none());
        assert!(!report.trade_plans[0].broadcast);
        assert_eq!(report.reconciliation_requests.len(), 1);
        assert_eq!(report.reconciliation_requests[0].feed_sequence, 42);
        assert!(!report.reconciliation_requests[0].initial_decision_dependency);
    }

    #[test]
    fn exact_raw_signed_bankr_v4_envelopes_emit_strict_async_requests() {
        let frames = bankr_v4_raw_frames();
        assert_eq!(frames.frames.len(), 2);
        for frame in frames.frames {
            let (expected_window, expected_line, expected_received_unix_ns, expected_sha256) =
                match frame.envelope.as_str() {
                    "erc7579" => (
                        "window-a",
                        2762,
                        1_784_271_655_711_031_000,
                        "4672011994f731bc6ca47ac8538c00539eb02c64854f8facbff1e2fff7291e75",
                    ),
                    "direct_airlock" => (
                        "window-b",
                        1661,
                        1_784_271_886_078_187_000,
                        "2da502bfbc533b2188390ef7190c8f5316fb8084914f4cf821a83578d1c66a84",
                    ),
                    other => panic!("unexpected fixture envelope {other}"),
                };
            let mut payload_line = frame.payload.as_bytes().to_vec();
            payload_line.push(b'\n');
            assert_eq!(frame.payload_sha256, expected_sha256);
            assert_eq!(hex::encode(Sha256::digest(&payload_line)), expected_sha256);
            assert_eq!(frame.received_unix_ns, expected_received_unix_ns);
            assert!(
                frame
                    .source_path
                    .ends_with(&format!("/windows/{expected_window}/raw-feed.jsonl"))
            );
            assert_eq!(
                (frame.window.as_str(), frame.line),
                (expected_window, expected_line)
            );
            let mut runtime = PaperFeedRuntime::new(bankr_v4_observer());
            let broadcast: BroadcastMessage = serde_json::from_str(&frame.payload).unwrap();
            let report = runtime
                .decode_received_at(&broadcast, frame.received_unix_ns)
                .unwrap();
            assert!(report.rejections.is_empty());
            assert_eq!(
                report
                    .observations
                    .iter()
                    .filter(|row| row.launchpad == LaunchpadId::BankrDoppler)
                    .count(),
                1
            );
            assert_eq!(
                report
                    .reconciliation_requests
                    .iter()
                    .filter(|row| row.launchpad == LaunchpadId::BankrDoppler)
                    .count(),
                1
            );
            assert!(
                report
                    .observations
                    .iter()
                    .all(|row| !row.live_execution_enabled)
            );
            assert!(report.trade_plans.iter().all(|plan| !plan.broadcast));
            let observation = report
                .observations
                .iter()
                .find(|row| row.tx_hash == frame.tx_hash)
                .unwrap();
            assert_eq!(observation.tx_hash, frame.tx_hash);
            assert_eq!(observation.launchpad, LaunchpadId::BankrDoppler);
            assert!(
                observation
                    .predicted_token
                    .is_some_and(|token| token > WETH)
            );
            assert!(observation.predicted_pool_id.is_some());
            assert!(!observation.live_execution_enabled);
            let request_row = report
                .reconciliation_requests
                .iter()
                .find(|row| row.tx_hash == frame.tx_hash)
                .unwrap();
            let request_value = serde_json::to_value(request_row).unwrap();
            let request: StrictPaperReconciliationRequest =
                serde_json::from_value(request_value).unwrap();
            assert_eq!(request.tx_hash, frame.tx_hash);
            assert_eq!(request.launchpad, LaunchpadId::BankrDoppler);
            assert_eq!(request.feed_sequence, observation.feed_sequence.unwrap());
            assert_eq!(
                request.l1_block_number,
                observation.l1_block_number.unwrap()
            );
            assert_eq!(request.l1_timestamp, observation.l1_timestamp.unwrap());
            assert_eq!(
                request.evidence_source,
                "independent_receipt_and_protocol_events"
            );
            assert!(!request.initial_decision_dependency);
            match frame.envelope.as_str() {
                "erc7579" => {
                    assert_eq!(observation.wrapper, WrapperKind::Erc4337);
                    assert_eq!(observation.leader_origin, LeaderOrigin::Erc4337Sender);
                    assert_eq!(request.wrapper, WrapperKind::Erc4337);
                    assert_eq!(
                        observation.detail["smart_account_identity"],
                        "receipt_block_eip7702_verification_required"
                    );
                }
                "direct_airlock" => {
                    assert_eq!(observation.wrapper, WrapperKind::Direct);
                    assert_eq!(observation.leader_origin, LeaderOrigin::DirectSigner);
                    assert_eq!(request.wrapper, WrapperKind::Direct);
                }
                other => panic!("unexpected fixture envelope {other}"),
            }
        }
    }

    #[test]
    fn exact_raw_nitro_bankr_v4_reverse_frames_emit_strict_paper_only_requests() {
        let frames = bankr_v4_reverse_raw_frames();
        assert_eq!(frames.frames.len(), 2);
        let expected = [
            (
                "post-stonks-a",
                3195,
                1_784_290_095_796_786_000,
                "168b5c615be31bd2ab3224cfa385c677a413e275d6496acbec20a34e8abb4965",
                alloy_primitives::address!("0106e926a9ccaedce5f87c859beaf89a56e96ba3"),
                alloy_primitives::b256!(
                    "1d40b4a7fb7768a884ea86f285e00cb9fd5ca3d282680984875888c6e5b81720"
                ),
            ),
            (
                "post-stonks-b",
                2794,
                1_784_290_343_752_758_000,
                "c2fdbc19c231eea8cd1d9d83d0006ffc0cdc62a01585d6be8125ede3aa0d087e",
                alloy_primitives::address!("022df6568187016fde9651cb8b5bc4aedcf80ba3"),
                alloy_primitives::b256!(
                    "a3a17284bba4c29e85ace5fe502177d3c8db97a45b5069e2b1ba467418047832"
                ),
            ),
        ];
        for (frame, (window, line, received, digest, token, pool_id)) in
            frames.frames.iter().zip(expected)
        {
            assert_eq!(frame.window, window);
            assert_eq!(frame.line, line);
            assert_eq!(frame.envelope, "erc7579");
            assert_eq!(frame.received_unix_ns, received);
            assert!(
                frame
                    .source_path
                    .ends_with(&format!("/{window}/raw-feed.jsonl"))
            );
            let mut payload_line = frame.payload.as_bytes().to_vec();
            payload_line.push(b'\n');
            assert_eq!(frame.payload_sha256, digest);
            assert_eq!(hex::encode(Sha256::digest(&payload_line)), digest);

            let mut runtime = PaperFeedRuntime::new(bankr_v4_observer());
            let broadcast: BroadcastMessage = serde_json::from_str(&frame.payload).unwrap();
            let report = runtime
                .decode_received_at(&broadcast, frame.received_unix_ns)
                .unwrap();
            assert!(report.rejections.is_empty());
            let observation = report
                .observations
                .iter()
                .find(|row| row.tx_hash == frame.tx_hash)
                .unwrap();
            assert_eq!(observation.launchpad, LaunchpadId::BankrDoppler);
            assert_eq!(observation.wrapper, WrapperKind::Erc4337);
            assert_eq!(observation.leader_origin, LeaderOrigin::Erc4337Sender);
            assert_eq!(observation.predicted_token, Some(token));
            assert!(token < WETH);
            assert_eq!(observation.predicted_pool_id, Some(pool_id));
            assert!(!observation.live_execution_enabled);
            assert_eq!(
                report
                    .reconciliation_requests
                    .iter()
                    .filter(|row| row.tx_hash == frame.tx_hash)
                    .count(),
                1
            );
            assert!(report.trade_plans.iter().all(|plan| !plan.broadcast));
        }
    }

    #[test]
    fn exact_raw_nitro_bankr_v5_erc7579_emits_one_strict_async_request() {
        let frames = bankr_v5_raw_frames();
        assert_eq!(frames.frames.len(), 1);
        let frame = &frames.frames[0];
        assert_eq!(frame.window, "window-a");
        assert_eq!(frame.line, 3100);
        assert_eq!(frame.envelope, "erc7579");
        assert_eq!(frame.received_unix_ns, 1_784_278_482_394_028_000);
        assert_eq!(
            frame.payload_sha256,
            "8c17197e9de53e4a65288729d0daf0c08a1489dde12ddb76350c8628dc6988b3"
        );
        let mut payload_line = frame.payload.as_bytes().to_vec();
        payload_line.push(b'\n');
        assert_eq!(
            hex::encode(Sha256::digest(payload_line)),
            frame.payload_sha256
        );

        let mut runtime = PaperFeedRuntime::new(bankr_v4_observer());
        let broadcast: BroadcastMessage = serde_json::from_str(&frame.payload).unwrap();
        let report = runtime
            .decode_received_at(&broadcast, frame.received_unix_ns)
            .unwrap();
        assert!(report.rejections.is_empty());
        let observation = report
            .observations
            .iter()
            .find(|row| row.tx_hash == frame.tx_hash)
            .unwrap();
        assert_eq!(observation.launchpad, LaunchpadId::BankrDoppler);
        assert_eq!(observation.wrapper, WrapperKind::Erc4337);
        assert_eq!(observation.leader_origin, LeaderOrigin::Erc4337Sender);
        assert!(
            observation
                .predicted_token
                .is_some_and(|token| token > WETH)
        );
        assert!(observation.predicted_pool_id.is_some());
        assert!(!observation.live_execution_enabled);
        assert_eq!(
            report
                .reconciliation_requests
                .iter()
                .filter(|row| row.tx_hash == frame.tx_hash)
                .count(),
            1
        );
        assert!(report.trade_plans.iter().all(|plan| !plan.broadcast));
    }

    #[test]
    fn exact_raw_nitro_bankr_v5_reverse_frames_emit_three_strict_async_requests() {
        let frames = bankr_v5_reverse_raw_frames();
        assert_eq!(frames.len(), 3);
        let expected = [
            (
                alloy_primitives::b256!(
                    "f05362bfc3dd65c67116b1630e8872e80380d2f6f7561455f4bbea9b2dcb391a"
                ),
                alloy_primitives::address!("08659aef179de34ba122c170af932ebe0d209ba3"),
                alloy_primitives::b256!(
                    "0ccdb9dc3ca3c2e9a5d3420ef8d6335544588e904f91372e86361acc8351cc42"
                ),
            ),
            (
                alloy_primitives::b256!(
                    "7c3641c37918052cf50e323ab99d99cd539ddd96c5c8f13511cc23db4ea8cd18"
                ),
                alloy_primitives::address!("0b20298a0807b5cb4e29a59a09f754dfcbebdba3"),
                alloy_primitives::b256!(
                    "18f2576d8f3002f0997d8cabe0a7964b442f0ea5c20c440ab84d13cdb81e2879"
                ),
            ),
            (
                alloy_primitives::b256!(
                    "81a8eb424f298df231b6d7e5acf8fafb7816742b80bd2b9b71caf8292b1c8bfc"
                ),
                alloy_primitives::address!("0b7c35adcc52ca404dc471811c9878f1ca858ba3"),
                alloy_primitives::b256!(
                    "2bb56a0a01ce1a2c462bfbf24a73c0ddfccb9304ab31ade33ad601e49ca91fd9"
                ),
            ),
        ];
        for (frame, (tx_hash, token, pool_id)) in frames.iter().zip(expected) {
            let mut runtime = PaperFeedRuntime::new(bankr_v4_observer());
            let broadcast: BroadcastMessage = serde_json::from_str(&frame.payload).unwrap();
            let report = runtime
                .decode_received_at(&broadcast, frame.received_unix_ns)
                .unwrap();
            assert!(report.rejections.is_empty());
            let observation = report
                .observations
                .iter()
                .find(|row| row.tx_hash == tx_hash)
                .unwrap();
            assert_eq!(observation.launchpad, LaunchpadId::BankrDoppler);
            assert_eq!(observation.wrapper, WrapperKind::Erc4337);
            assert_eq!(observation.leader_origin, LeaderOrigin::Erc4337Sender);
            assert_eq!(observation.predicted_token, Some(token));
            assert!(token < WETH);
            assert_eq!(observation.predicted_pool_id, Some(pool_id));
            assert!(!observation.live_execution_enabled);
            assert_eq!(
                report
                    .reconciliation_requests
                    .iter()
                    .filter(|row| row.tx_hash == tx_hash)
                    .count(),
                1
            );
            assert!(report.trade_plans.iter().all(|plan| !plan.broadcast));
        }
    }

    #[test]
    fn exact_raw_nitro_stonks_v3_is_decoded_but_not_globally_dispatched() {
        let record: StonksV3RawRecord = serde_json::from_str(include_str!(
            "../tests/fixtures/stonks-v3-direct-launch-fresh-raw-frame.json"
        ))
        .unwrap();
        assert_eq!(record.received_unix_ns, 1_784_282_709_119_456_000);
        let tx_hash = alloy_primitives::b256!(
            "d53c3d8d8c76fd5f367d3d229a45e1aef65c0cdb712d94421f311f97fe6dd563"
        );
        let observer = bankr_v4_observer();
        assert!(observer.capabilities().iter().all(|capability| {
            capability.launchpad != LaunchpadId::StonksV3 && !capability.live_execution_enabled
        }));
        let mut runtime = PaperFeedRuntime::new(observer);
        let broadcast: BroadcastMessage = serde_json::from_str(&record.payload).unwrap();
        let report = runtime
            .decode_received_at(&broadcast, record.received_unix_ns)
            .unwrap();
        assert!(report.transactions.iter().any(|row| row.tx_hash == tx_hash));
        assert!(report.observations.iter().all(|row| row.tx_hash != tx_hash));
        assert!(
            report
                .reconciliation_requests
                .iter()
                .all(|row| row.tx_hash != tx_hash)
        );
        assert!(report.trade_plans.iter().all(|plan| !plan.broadcast));
    }

    #[test]
    fn exact_curve_ticks_v5_direct_airlock_is_rejected_before_async_request() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v5-fresh-six-live-proofs.json"
        ))
        .unwrap();
        let transaction: crate::RobinhoodTransaction =
            serde_json::from_value(fixture["launches"][0]["transaction"].clone()).unwrap();
        let profile = BankrDopplerExpectedProfile::production();
        let discovered = discover_entry_point_v07_erc7579(
            EntryPointCall {
                chain_id: CHAIN_ID,
                destination: profile.entry_point,
                outer_bundler: transaction.from,
                calldata: &transaction.input,
            },
            profile.entry_point,
            SmartContractPin {
                address: profile.airlock.address,
                runtime_code_hash: profile.airlock.runtime_code_hash,
            },
        )
        .unwrap();
        let direct = signed_transaction(profile.airlock.address, discovered.calldata.to_vec());
        let mut runtime = PaperFeedRuntime::new(bankr_v4_observer());
        let report = runtime.decode(&feed(&direct)).unwrap();
        assert!(report.observations.is_empty());
        assert!(report.reconciliation_requests.is_empty());
        assert_eq!(report.rejections.len(), 1);
        assert!(
            report.rejections[0]
                .reason
                .contains("only through the evidenced ERC7579 envelope")
        );
    }

    #[test]
    fn malformed_curve_ticks_v4_is_rejected_before_reconciliation_request() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v4-finaltuple-window-abc-live-proofs.json"
        ))
        .unwrap();
        let input = fixture["launches"][9]["transaction"]["input"]
            .as_str()
            .unwrap();
        let mut calldata = hex::decode(input.strip_prefix("0x").unwrap()).unwrap();
        let boundary_word_end = 4 + 32 + 12 * 32 + 32 + 32 + 32 + 32;
        calldata[boundary_word_end - 1] ^= 1;
        let transaction = signed_transaction(
            BankrDopplerExpectedProfile::production().airlock.address,
            calldata,
        );
        let mut runtime = PaperFeedRuntime::new(bankr_v4_observer());
        let report = runtime.decode(&feed(&transaction)).unwrap();
        assert!(report.observations.is_empty());
        assert!(report.reconciliation_requests.is_empty());
        assert_eq!(report.rejections.len(), 1);
        assert!(
            report.rejections[0]
                .reason
                .contains("exact reviewed profile")
        );
    }

    #[test]
    fn clean4_type4_proof_reaches_frame_and_preserves_reconciliation_provenance() {
        let (mut expected, mut observed) = startup();
        let profile = Eip7702SelfBatchExpectedPins::production();
        expected.pons_eip7702_self_batch = Some(profile.clone());
        observed.pins.extend([
            ObservedRuntimePin {
                address: profile.account,
                implementation: Some(profile.implementation),
                runtime_hash: profile.designator_hash,
                code_bytes: Some(23),
            },
            ObservedRuntimePin {
                address: profile.implementation,
                implementation: None,
                runtime_hash: profile.implementation_runtime_hash,
                code_bytes: Some(1),
            },
        ]);
        let observer = PaperLaunchpadObserver::from_startup_snapshots(expected, observed).unwrap();
        let transaction = crate::eip7702_self_batch::tests::clean4_proof_envelope();
        let mut runtime = PaperFeedRuntime::new(observer);
        let report = runtime.decode_received_at(&feed(&transaction), 1).unwrap();

        assert!(report.rejections.is_empty());
        assert_eq!(report.observations.len(), 1);
        let observation = &report.observations[0];
        assert_eq!(observation.launchpad, LaunchpadId::Pons);
        assert_eq!(observation.leader_origin, LeaderOrigin::Eip7702Authority);
        assert_eq!(observation.wrapper, WrapperKind::Eip7702SelfBatch);
        assert_eq!(observation.leader, profile.account);
        assert_eq!(observation.outer_signer, profile.account);
        assert!(observation.predicted_token.is_some());
        assert!(observation.predicted_pool.is_some());
        assert!(!observation.live_execution_enabled);
        assert!(observation.detail.get("eip7702_self_batch").is_some());

        assert_eq!(report.trade_plans.len(), 1);
        assert!(!report.trade_plans[0].broadcast);
        assert_eq!(report.reconciliation_requests.len(), 1);
        let request = &report.reconciliation_requests[0];
        assert_eq!(request.wrapper, WrapperKind::Eip7702SelfBatch);
        let provenance = request.wrapper_provenance.as_ref().unwrap();
        assert_eq!(provenance.authority, profile.account);
        assert_eq!(provenance.self_target, profile.account);
        assert_eq!(provenance.implementation, profile.implementation);
        assert_eq!(provenance.designator_hash, profile.designator_hash);
    }

    #[test]
    fn raw_hood_buy_for_reaches_observer_with_exact_token_identity() {
        let observer = observer();
        let token = Address::with_last_byte(0xa1);
        let recipient = Address::with_last_byte(0xa2);
        let mut input = HOOD_BUY_FOR_SELECTOR.to_vec();
        for word in [
            U256::from_be_slice(token.as_slice()),
            U256::from_be_slice(recipient.as_slice()),
            U256::from(123_u64),
        ] {
            input.extend_from_slice(&word.to_be_bytes::<32>());
        }
        let transaction = signed_transaction_with_value(
            HOOD_FACTORY,
            input,
            U256::from(1_000_000_000_000_000_u64),
        );
        let mut runtime = PaperFeedRuntime::new(observer);
        let report = runtime.decode(&feed(&transaction)).unwrap();

        assert!(report.rejections.is_empty());
        assert_eq!(report.observations.len(), 1);
        assert_eq!(report.observations[0].launchpad, LaunchpadId::HoodFun);
        assert_eq!(report.observations[0].action, Some(ActionKind::Buy));
        assert_eq!(report.observations[0].predicted_token, Some(token));
        assert_eq!(
            report.observations[0].detail["recipient"],
            serde_json::json!(recipient)
        );
        assert_eq!(report.reconciliation_requests.len(), 1);
    }

    #[test]
    fn cross_adapter_destination_does_not_fall_through() {
        let observer = observer();
        let transaction = signed_transaction(Address::with_last_byte(0xee), hood_launch_input());
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
    fn configured_pons_profile_is_separate_from_and_must_match_fresh_observation() {
        let (mut expected, observed) = startup();
        expected.pons_v3.identities[0].runtime_hash = B256::with_last_byte(0xee);
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected, observed).is_err());

        let (mut expected, observed) = startup();
        expected.pons_v3.identities.pop();
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected, observed).is_err());

        let (expected, mut observed) = startup();
        find_observed_mut(&mut observed.pins, crate::pons::PONS_CURRENT_FACTORY).runtime_hash =
            B256::with_last_byte(0xee);
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected, observed).is_err());
    }

    #[test]
    fn reviewed_production_hood_profile_is_complete_and_tamper_evident() {
        let expected: PaperExpectedPins = serde_json::from_str(include_str!(
            "../config/launchpad-expected-pins.production.json"
        ))
        .unwrap();
        assert_eq!(
            expected.provenance,
            ExpectedPinsProvenance::ReviewedProtocolPins
        );
        let profile = expected.hood_curve.unwrap();
        profile.validate().unwrap();
        assert_eq!(profile.identities.len(), 10);
        assert_eq!(
            expected.hood_factory_runtime_hash,
            profile
                .identity(crate::hood_receipt_quote::HoodIdentityRole::Factory)
                .map(|identity| identity.runtime_hash)
        );

        let mut tampered = profile;
        tampered.identities[0].runtime_hash = B256::with_last_byte(0xee);
        assert!(tampered.validate().is_err());
        tampered = crate::hood_receipt_quote::HoodExpectedProfile::production();
        tampered.identities.pop();
        assert!(tampered.validate().is_err());

        let (_, mut observed) = startup();
        observed.provenance = ObservedPinsProvenance::StartupObservation;
        observed.fixture_id = None;
        let mut incomplete: PaperExpectedPins = serde_json::from_str(include_str!(
            "../config/launchpad-expected-pins.production.json"
        ))
        .unwrap();
        incomplete.clanker_v4 = None;
        assert!(validate_document_pair(&incomplete, &observed).is_err());
        incomplete = serde_json::from_str(include_str!(
            "../config/launchpad-expected-pins.production.json"
        ))
        .unwrap();
        incomplete.bankr_doppler_v4 = None;
        assert!(validate_document_pair(&incomplete, &observed).is_err());
        incomplete = serde_json::from_str(include_str!(
            "../config/launchpad-expected-pins.production.json"
        ))
        .unwrap();
        incomplete.hood_curve = None;
        assert!(validate_document_pair(&incomplete, &observed).is_err());
    }

    #[test]
    fn hood_semantic_startup_snapshot_rejects_mutable_link_or_config_drift() {
        let profile = crate::hood_receipt_quote::HoodExpectedProfile::production();
        let role = |role| profile.identity(role).unwrap().address;
        let semantic = profile.semantic;
        let mut snapshot = HoodProtocolSnapshot {
            factory: semantic.factory,
            l2_block_number: 1,
            config: crate::noxa_rpc::HoodConfigSnapshot {
                virtual_eth_seed: semantic.virtual_eth_seed,
                creation_fee: semantic.creation_fee,
                default_trade_fee_bps: semantic.default_trade_fee_bps,
                migration_fee: semantic.migration_fee,
                migration_fee_bps: semantic.migration_fee_bps,
                guard_blocks: semantic.guard_blocks,
                guard_max_wallet_bps: semantic.current_guard_max_wallet_bps,
                creator_fee_share_bps: semantic.creator_fee_share_bps,
                vanity_enforced: semantic.vanity_enforced,
            },
            migrator: semantic.active_migrator,
            fallback_factory: semantic.fallback_factory,
            weth: semantic.weth,
            owner: profile.owner,
            pending_owner: profile.pending_owner,
            owner_safe_singleton: role(HoodIdentityRole::OwnerSafeSingleton),
            migrator_launchpad: semantic.factory,
            migrator_position_manager: role(HoodIdentityRole::PositionManager),
            migrator_locker: role(HoodIdentityRole::Locker),
            migrator_weth: semantic.weth,
            migrator_protocol: profile.owner,
            migrator_creator_share_bps: profile.migrator_creator_share_bps,
            migrator_v3_fee: profile.v3_fee,
            locker_position_manager: role(HoodIdentityRole::PositionManager),
            locker_weth: semantic.weth,
            locker_burn_bps: profile.locker_token_fee_burn_bps,
            position_manager_factory: role(HoodIdentityRole::V3Factory),
            position_manager_weth: semantic.weth,
            router_factory: role(HoodIdentityRole::V3Factory),
            router_weth: semantic.weth,
        };
        validate_hood_protocol_snapshot(&profile, &snapshot).unwrap();
        snapshot.migrator = Address::with_last_byte(0xee);
        assert!(validate_hood_protocol_snapshot(&profile, &snapshot).is_err());
        snapshot.migrator = semantic.active_migrator;
        snapshot.config.guard_max_wallet_bps = 1;
        assert!(validate_hood_protocol_snapshot(&profile, &snapshot).is_err());
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
    fn launchhood_immutable_and_runtime_identity_are_both_required() {
        let (expected, observed) = startup();
        PaperLaunchpadObserver::from_startup_snapshots(expected.clone(), observed.clone()).unwrap();

        let mut wrong_factory = expected.clone();
        wrong_factory.launchhood_v3_factory_runtime_hash = B256::with_last_byte(0xf1);
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(wrong_factory, observed.clone())
                .is_err()
        );

        let mut wrong_address = expected.clone();
        wrong_address.launchhood_v3_token_implementation.address = Address::with_last_byte(0xf2);
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(wrong_address, observed.clone())
                .is_err()
        );

        let mut wrong_hash = expected.clone();
        wrong_hash.launchhood_v3_token_implementation.runtime_hash = B256::with_last_byte(0xf3);
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(wrong_hash, observed.clone()).is_err()
        );

        let mut wrong_length = expected.clone();
        wrong_length.launchhood_v3_token_implementation.code_bytes += 1;
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(wrong_length, observed.clone()).is_err()
        );

        let mut missing = observed.clone();
        missing
            .pins
            .retain(|pin| pin.address != LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION);
        assert!(PaperLaunchpadObserver::from_startup_snapshots(expected.clone(), missing).is_err());

        let mut observed_hash_drift = observed.clone();
        find_observed_mut(
            &mut observed_hash_drift.pins,
            LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION,
        )
        .runtime_hash = B256::with_last_byte(0xf4);
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(expected.clone(), observed_hash_drift,)
                .is_err()
        );

        let mut observed_length_drift = observed;
        find_observed_mut(
            &mut observed_length_drift.pins,
            LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION,
        )
        .code_bytes = Some(LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_CODE_BYTES + 1);
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(expected, observed_length_drift)
                .is_err()
        );
    }

    #[test]
    fn configured_clanker_dependencies_require_fresh_matching_startup_pins() {
        let (mut expected, mut observed_snapshot) = startup();
        let profile = ClankerV4ExpectedProfile::production();
        expected.clanker_v4 = Some(ConfiguredClankerV4 {
            factory_runtime_hash: profile.factory.runtime_code_hash,
            deployer_runtime_hash: crate::launchpad_adapters::CLANKER_DEPLOYER_RUNTIME_HASH,
            pool_manager_runtime_hash: profile.pool_manager.runtime_code_hash,
            hook_runtime_hash: profile.hook.runtime_code_hash,
            locker_runtime_hash: profile.locker.runtime_code_hash,
            mev_module_runtime_hash: profile.mev_module.runtime_code_hash,
            extension_runtime_hash: profile.extension.runtime_code_hash,
            max_static_fee_ppm: profile.max_static_fee_ppm,
            max_mev_fee_ppm: profile.max_mev_fee_ppm,
            max_mev_seconds_to_decay: profile.max_mev_seconds_to_decay,
            mev_delay_guard_seconds: profile.mev_delay_guard_seconds,
            protocol_fee_share_percent: profile.protocol_fee_share_percent,
        });
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(
                expected.clone(),
                observed_snapshot.clone()
            )
            .is_err()
        );
        observed_snapshot.pins.extend([
            observed(
                CLANKER_DEPLOYER,
                None,
                crate::launchpad_adapters::CLANKER_DEPLOYER_RUNTIME_HASH,
            ),
            observed(
                profile.pool_manager.address,
                None,
                profile.pool_manager.runtime_code_hash,
            ),
            observed(profile.hook.address, None, profile.hook.runtime_code_hash),
            observed(
                profile.locker.address,
                None,
                profile.locker.runtime_code_hash,
            ),
            observed(
                profile.mev_module.address,
                None,
                profile.mev_module.runtime_code_hash,
            ),
            observed(
                profile.extension.address,
                None,
                profile.extension.runtime_code_hash,
            ),
        ]);
        PaperLaunchpadObserver::from_startup_snapshots(expected.clone(), observed_snapshot.clone())
            .unwrap();
        let mut semantic_drift = expected.clone();
        semantic_drift.clanker_v4.as_mut().unwrap().max_mev_fee_ppm -= 1;
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(
                semantic_drift,
                observed_snapshot.clone()
            )
            .is_err()
        );
        find_observed_mut(&mut observed_snapshot.pins, profile.hook.address).runtime_hash =
            B256::with_last_byte(0xee);
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(expected, observed_snapshot).is_err()
        );
    }

    #[test]
    fn clanker_quiet1_observation_emits_token_and_v4_pool_id_prediction() {
        #[derive(Deserialize)]
        struct Proof {
            tx_hash: B256,
            signer: Address,
            expected_token: Address,
            expected_pool_id: B256,
            calldata: Bytes,
        }

        let (mut expected, mut observed_snapshot) = startup();
        let profile = ClankerV4ExpectedProfile::production();
        expected.clanker_v4 = Some(ConfiguredClankerV4 {
            factory_runtime_hash: profile.factory.runtime_code_hash,
            deployer_runtime_hash: crate::launchpad_adapters::CLANKER_DEPLOYER_RUNTIME_HASH,
            pool_manager_runtime_hash: profile.pool_manager.runtime_code_hash,
            hook_runtime_hash: profile.hook.runtime_code_hash,
            locker_runtime_hash: profile.locker.runtime_code_hash,
            mev_module_runtime_hash: profile.mev_module.runtime_code_hash,
            extension_runtime_hash: profile.extension.runtime_code_hash,
            max_static_fee_ppm: profile.max_static_fee_ppm,
            max_mev_fee_ppm: profile.max_mev_fee_ppm,
            max_mev_seconds_to_decay: profile.max_mev_seconds_to_decay,
            mev_delay_guard_seconds: profile.mev_delay_guard_seconds,
            protocol_fee_share_percent: profile.protocol_fee_share_percent,
        });
        observed_snapshot.pins.extend([
            observed(
                CLANKER_DEPLOYER,
                None,
                crate::launchpad_adapters::CLANKER_DEPLOYER_RUNTIME_HASH,
            ),
            observed(
                profile.pool_manager.address,
                None,
                profile.pool_manager.runtime_code_hash,
            ),
            observed(profile.hook.address, None, profile.hook.runtime_code_hash),
            observed(
                profile.locker.address,
                None,
                profile.locker.runtime_code_hash,
            ),
            observed(
                profile.mev_module.address,
                None,
                profile.mev_module.runtime_code_hash,
            ),
            observed(
                profile.extension.address,
                None,
                profile.extension.runtime_code_hash,
            ),
        ]);
        let observer =
            PaperLaunchpadObserver::from_startup_snapshots(expected, observed_snapshot).unwrap();
        let proofs: Vec<Proof> = serde_json::from_str(include_str!(
            "../tests/fixtures/clanker-charms-direct-deploys.json"
        ))
        .unwrap();
        let proof = &proofs[0];
        let observation = observer
            .observe_call(
                proof.tx_hash,
                proof.signer,
                proof.signer,
                LeaderOrigin::DirectSigner,
                WrapperKind::Direct,
                CLANKER_FACTORY,
                U256::ZERO,
                &proof.calldata,
            )
            .unwrap();
        assert_eq!(observation.predicted_token, Some(proof.expected_token));
        assert_eq!(observation.predicted_pool, None);
        assert_eq!(observation.predicted_pool_id, Some(proof.expected_pool_id));
        assert_eq!(observation.planning_mode, ExecutionMode::ExecutionGated);
        assert!(!observation.live_execution_enabled);
    }

    #[test]
    fn configured_bankr_profile_requires_exact_dependencies_and_delegation_pair() {
        let (mut expected, mut observed_snapshot) = startup();
        let profile = BankrDopplerExpectedProfile::production();
        expected.bankr_doppler_v4 = Some(ConfiguredBankrDopplerV4 {
            airlock_runtime_hash: profile.airlock.runtime_code_hash,
            pool_manager_runtime_hash: profile.pool_manager.runtime_code_hash,
            initializer_runtime_hash: profile.initializer.runtime_code_hash,
            rehype_hook_runtime_hash: profile.rehype_hook.runtime_code_hash,
            token_factory_runtime_hash: profile.token_factory.runtime_code_hash,
            token_implementation_runtime_hash: profile.token_implementation.runtime_code_hash,
            governance_factory_runtime_hash: profile.governance_factory.runtime_code_hash,
            liquidity_migrator_runtime_hash: profile.liquidity_migrator.runtime_code_hash,
            standard_lp_fee_ppm: profile.standard_lp_fee_ppm,
            max_lp_fee_ppm: profile.max_lp_fee_ppm,
            hook_fee_denominator_ppm: profile.hook_fee_denominator_ppm,
            hook_start_fee_ppm: profile.hook_start_fee_ppm,
            hook_end_fee_ppm: profile.hook_end_fee_ppm,
            hook_duration_seconds: profile.hook_duration_seconds,
            quote_delay_guard_seconds: profile.quote_delay_guard_seconds,
            tick_spacing: profile.tick_spacing,
            pool_allocation_bps: profile.pool_allocation_bps,
            primary_curve_share_bps: profile.primary_curve_share_bps,
            secondary_curve_share_bps: profile.secondary_curve_share_bps,
            creator_beneficiary_bps: profile.creator_beneficiary_bps,
            protocol_beneficiary_bps: profile.protocol_beneficiary_bps,
        });
        expected.bankr_doppler_calls = vec![ConfiguredCallPin {
            destination: profile.airlock.address,
            runtime_hash: profile.airlock.runtime_code_hash,
            selector: BANKR_CREATE_SELECTOR,
        }];
        let delegation = profile.smart_account.delegation_implementation.unwrap();
        expected.erc4337 = Some(ConfiguredSmartAccounts {
            entry_point_runtime_hash: profile.entry_point.runtime_code_hash,
            accounts: vec![ConfiguredSmartAccount {
                account: profile.smart_account.account.address,
                runtime_hash: profile.smart_account.account.runtime_code_hash,
                execution_profile: profile.smart_account.execution_profile,
                factory: None,
                factory_runtime_hash: None,
                delegation_implementation: Some(delegation.address),
                delegation_runtime_hash: Some(delegation.runtime_code_hash),
            }],
        });
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(
                expected.clone(),
                observed_snapshot.clone()
            )
            .is_err()
        );
        observed_snapshot.pins.extend([
            observed(
                profile.airlock.address,
                None,
                profile.airlock.runtime_code_hash,
            ),
            observed(
                profile.pool_manager.address,
                None,
                profile.pool_manager.runtime_code_hash,
            ),
            observed(
                profile.initializer.address,
                None,
                profile.initializer.runtime_code_hash,
            ),
            observed(
                profile.rehype_hook.address,
                None,
                profile.rehype_hook.runtime_code_hash,
            ),
            observed(
                profile.token_factory.address,
                None,
                profile.token_factory.runtime_code_hash,
            ),
            observed(
                profile.token_implementation.address,
                None,
                profile.token_implementation.runtime_code_hash,
            ),
            observed(
                profile.governance_factory.address,
                None,
                profile.governance_factory.runtime_code_hash,
            ),
            observed(
                profile.liquidity_migrator.address,
                None,
                profile.liquidity_migrator.runtime_code_hash,
            ),
            observed(
                profile.entry_point.address,
                None,
                profile.entry_point.runtime_code_hash,
            ),
            observed(
                profile.smart_account.account.address,
                Some(delegation.address),
                profile.smart_account.account.runtime_code_hash,
            ),
            observed(delegation.address, None, delegation.runtime_code_hash),
        ]);
        let observer = PaperLaunchpadObserver::from_startup_snapshots(
            expected.clone(),
            observed_snapshot.clone(),
        )
        .unwrap();
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v2-direct-live-proof.json"
        ))
        .unwrap();
        let calldata: Bytes =
            serde_json::from_value(fixture["transaction"]["input"].clone()).unwrap();
        let observation = observer
            .observe_call(
                B256::with_last_byte(1),
                Address::with_last_byte(1),
                Address::with_last_byte(1),
                LeaderOrigin::DirectSigner,
                WrapperKind::Direct,
                profile.airlock.address,
                U256::ZERO,
                &calldata,
            )
            .unwrap();
        assert_eq!(
            observation.predicted_token,
            Some(alloy_primitives::address!(
                "88368c6d8e52bfd2af862caf33b01acd57c53ba3"
            ))
        );
        assert_eq!(observation.predicted_pool, None);
        assert_eq!(
            observation.predicted_pool_id,
            Some(alloy_primitives::b256!(
                "3110c3afa7fd12379c53b5a49829e5a78144f2bff0440cd7da6917dda5f88f02"
            ))
        );

        let mut semantic_drift = expected.clone();
        semantic_drift
            .bankr_doppler_v4
            .as_mut()
            .unwrap()
            .hook_fee_denominator_ppm -= 1;
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(
                semantic_drift,
                observed_snapshot.clone()
            )
            .is_err()
        );

        let mut incomplete_delegation = expected.clone();
        incomplete_delegation.erc4337.as_mut().unwrap().accounts[0].delegation_runtime_hash = None;
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(
                incomplete_delegation,
                observed_snapshot.clone()
            )
            .is_err()
        );

        find_observed_mut(&mut observed_snapshot.pins, profile.rehype_hook.address).runtime_hash =
            B256::with_last_byte(0xee);
        assert!(
            PaperLaunchpadObserver::from_startup_snapshots(expected, observed_snapshot).is_err()
        );
    }

    #[test]
    fn expected_document_cannot_self_attest_as_observed_snapshot() {
        let value = serde_json::json!({
            "schema_version": 4,
            "document_role": "expected_protocol_pins",
            "provenance": "synthetic_offline_fixture",
            "fixture_id": "launchpad-paper-offline-v3",
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

        let mut missing_factory_address = base;
        missing_factory_address.factory_runtime_hash = Some(B256::with_last_byte(0xf1));
        assert!(configured_smart_account_pin(missing_factory_address).is_err());

        let mut missing_delegation_hash = base;
        missing_delegation_hash.execution_profile = AccountExecutionProfile::Erc7579SingleCall;
        missing_delegation_hash.delegation_implementation = Some(Address::with_last_byte(0xd1));
        assert!(configured_smart_account_pin(missing_delegation_hash).is_err());

        let mut missing_delegation_address = base;
        missing_delegation_address.execution_profile = AccountExecutionProfile::Erc7579SingleCall;
        missing_delegation_address.delegation_runtime_hash = Some(B256::with_last_byte(0xd1));
        assert!(configured_smart_account_pin(missing_delegation_address).is_err());
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
