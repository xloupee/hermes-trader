//! Fail-closed Pons family observation and paper-planning support.
//!
//! This module deliberately has no RPC client and exposes no signing or
//! broadcast type. Startup code may validate the supplied runtime identities,
//! after which candidate admission and paper planning are pure in-memory
//! operations. Markets are never predicted: a pool must arrive through an
//! asynchronously verified, factory-proven receipt snapshot.

use alloy_primitives::{Address, B256, Bytes, U256};
use alloy_sol_types::{SolCall, sol};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::noxa_abi::{V3ExactInputIntent, encode_v3_exact_input_single};
use crate::v3_pool::{V3PoolError, V3PoolState, V3Quote};

pub const PONS_CHAIN_ID: u64 = 4_663;
pub const PONS_LAUNCH_FEE_WEI: u64 = 500_000_000_000_000;
pub const PONS_POOL_FEE: u32 = 10_000;
pub const PONS_TICK_SPACING: i32 = 200;
pub const PONS_LAUNCH_CONFIG_ID: u64 = 0;
pub const PONS_DEX_CONFIG_ID: u64 = 0;
pub const PONS_LAUNCH_SELECTOR: [u8; 4] = [0x68, 0x63, 0x99, 0xcb];

pub const PONS_LEGACY_FACTORY: Address =
    alloy_primitives::address!("a5aab3f0c6eeadf30ef1d3eb997108e976351feb");
pub const PONS_CURRENT_FACTORY: Address =
    alloy_primitives::address!("0c37a24f5d23a486fa692d1500881d698b1f77a4");
pub const PONS_LEGACY_LOCKER: Address =
    alloy_primitives::address!("736d76699c26d0d966744cae304c000d471f7f35");
pub const PONS_CURRENT_LOCKER: Address =
    alloy_primitives::address!("31ca5e101941a93a7dd6d0497928700625cf54b5");
pub const PONS_WETH: Address =
    alloy_primitives::address!("0bd7d308f8e1639fab988df18a8011f41eacad73");
pub const PONS_V3_FACTORY: Address =
    alloy_primitives::address!("1f7d7550b1b028f7571e69a784071f0205fd2efa");
pub const PONS_POSITION_MANAGER: Address =
    alloy_primitives::address!("73991a25c818bf1f1128deaab1492d45638de0d3");
pub const PONS_SWAP_ROUTER_02: Address =
    alloy_primitives::address!("caf681a66d020601342297493863e78c959e5cb2");

pub const PONS_TOKEN_LAUNCHED_TOPIC: B256 =
    alloy_primitives::b256!("db51ea9ad51ab453a65a4cb7e60c3cb378c9501bb002609f8f97778fb6c4235a");
pub const PONS_TOKEN_DEPLOYED_TOPIC: B256 =
    alloy_primitives::b256!("1461370115e1c2be79cb529f8cfcbd11316e789d9c6099fc83417b0b4c48c62a");
pub const PONS_POOL_CREATED_TOPIC: B256 =
    alloy_primitives::b256!("783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118");
pub const PONS_POSITION_LOCKED_TOPIC: B256 =
    alloy_primitives::b256!("f3fabcec8f79e4c84abcb646b5b7eb0af5fa1fcc77977e928d8b87562cc96904");
pub const PONS_V3_SWAP_TOPIC: B256 =
    alloy_primitives::b256!("c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67");
pub const PONS_CURRENT_FACTORY_RUNTIME: B256 =
    alloy_primitives::b256!("921a0d1b2d854de5435804e8ee118658f05173a0eeebca5f41b41385b97cd1b5");
pub const PONS_LEGACY_FACTORY_RUNTIME: B256 =
    alloy_primitives::b256!("0a62b8ed1d88d30c7b342ea8361dfaf0ac336706992cf0c8ba38b129f06391d4");
pub const PONS_CURRENT_LOCKER_RUNTIME: B256 =
    alloy_primitives::b256!("5bfb52957c2df2cc05b894cd707811c811ee0e38b4a26ea59bae08cd65b39bbd");
pub const PONS_V3_FACTORY_RUNTIME: B256 =
    alloy_primitives::b256!("ec72b1abd1f2faee020cfea9c646bd8994f9fb389054f6e574f103a895091739");
pub const PONS_POSITION_MANAGER_RUNTIME: B256 =
    alloy_primitives::b256!("0a493d1af3d0f25fed8efa205244ebee14114267a08647fc38c515c7cd6ead4f");
pub const PONS_SWAP_ROUTER_RUNTIME: B256 =
    alloy_primitives::b256!("6f36c378e272c6324c48f045182bcb54bd8ad654cf9ebd42e8893d52c4cb25dc");
pub const PONS_WETH_RUNTIME: B256 =
    alloy_primitives::b256!("5706be52f64875fee65a2cec0d80e47a23d8793cbe85d214b48445e2d05f5353");

/// Research fixture identifier for the canonical current-generation launch
/// that paid exactly the launch fee and made no developer buy.
pub const PONS_CURRENT_LAUNCH_FIXTURE_TX: B256 =
    alloy_primitives::b256!("cce2b414f04ad3caab0ad38bc10cc1ac0741ed95ac740495535b71c8302fcc41");

sol! {
    struct PonsSocials {
        string telegram;
        string twitter;
        string discord;
        string website;
        string farcaster;
    }

    struct PonsLaunchParams {
        string name;
        string symbol;
        string logo;
        string description;
        PonsSocials socials;
        address devWallet;
    }

    function launchToken(
        PonsLaunchParams params,
        uint256 launchConfigId,
        uint256 dexConfigId,
        bytes32 salt
    ) external payable returns (address token, uint256 positionId);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PonsGeneration {
    Legacy,
    Current,
}

impl PonsGeneration {
    pub const fn factory(self) -> Address {
        match self {
            Self::Legacy => PONS_LEGACY_FACTORY,
            Self::Current => PONS_CURRENT_FACTORY,
        }
    }

    pub const fn factory_runtime(self) -> B256 {
        match self {
            Self::Legacy => PONS_LEGACY_FACTORY_RUNTIME,
            Self::Current => PONS_CURRENT_FACTORY_RUNTIME,
        }
    }

    pub const fn locker(self) -> Address {
        match self {
            Self::Legacy => PONS_LEGACY_LOCKER,
            Self::Current => PONS_CURRENT_LOCKER,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeIdentity {
    pub address: Address,
    pub code_bytes: usize,
    pub runtime_hash: B256,
}

const REQUIRED_CURRENT_IDENTITIES: [RuntimeIdentity; 7] = [
    RuntimeIdentity {
        address: PONS_CURRENT_FACTORY,
        code_bytes: 24_192,
        runtime_hash: PONS_CURRENT_FACTORY_RUNTIME,
    },
    RuntimeIdentity {
        address: PONS_LEGACY_FACTORY,
        code_bytes: 24_353,
        runtime_hash: PONS_LEGACY_FACTORY_RUNTIME,
    },
    RuntimeIdentity {
        address: PONS_CURRENT_LOCKER,
        code_bytes: 4_861,
        runtime_hash: PONS_CURRENT_LOCKER_RUNTIME,
    },
    RuntimeIdentity {
        address: PONS_V3_FACTORY,
        code_bytes: 24_535,
        runtime_hash: PONS_V3_FACTORY_RUNTIME,
    },
    RuntimeIdentity {
        address: PONS_POSITION_MANAGER,
        code_bytes: 24_384,
        runtime_hash: PONS_POSITION_MANAGER_RUNTIME,
    },
    RuntimeIdentity {
        address: PONS_SWAP_ROUTER_02,
        code_bytes: 24_497,
        runtime_hash: PONS_SWAP_ROUTER_RUNTIME,
    },
    RuntimeIdentity {
        address: PONS_WETH,
        code_bytes: 2_202,
        runtime_hash: PONS_WETH_RUNTIME,
    },
];

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PonsPinError {
    #[error("a required Pons runtime identity is missing or has drifted")]
    MissingOrDriftedIdentity,
}

#[derive(Debug, Clone, Copy)]
pub struct PonsAdapter {
    _validated: (),
}

impl PonsAdapter {
    /// Validate identities fetched before candidate processing begins. This
    /// function performs no I/O itself and retains no client capable of I/O.
    pub fn from_startup_identities(identities: &[RuntimeIdentity]) -> Result<Self, PonsPinError> {
        let complete = REQUIRED_CURRENT_IDENTITIES
            .iter()
            .all(|required| identities.iter().any(|actual| actual == required));
        if !complete {
            return Err(PonsPinError::MissingOrDriftedIdentity);
        }
        Ok(Self { _validated: () })
    }

    pub const fn required_startup_identities() -> &'static [RuntimeIdentity] {
        &REQUIRED_CURRENT_IDENTITIES
    }

    pub fn observe_launch(
        &self,
        input: PonsObservationInput<'_>,
    ) -> Result<ObservedLeaderAction, PonsObservationReject> {
        if input.chain_id != PONS_CHAIN_ID {
            return Err(PonsObservationReject::WrongChain);
        }
        if input.provenance != PonsAttributionProvenance::ExactFactoryTransaction {
            return Err(PonsObservationReject::MissingFactoryProvenance);
        }
        let generation = match input.destination {
            PONS_LEGACY_FACTORY => PonsGeneration::Legacy,
            PONS_CURRENT_FACTORY => PonsGeneration::Current,
            _ => return Err(PonsObservationReject::UnknownFactory),
        };
        if input.destination_runtime_hash != generation.factory_runtime() {
            return Err(PonsObservationReject::RuntimeDrift);
        }
        if input.calldata.get(..4) != Some(&PONS_LAUNCH_SELECTOR) {
            return Err(PonsObservationReject::WrongSelector);
        }
        if input.value < U256::from(PONS_LAUNCH_FEE_WEI) {
            return Err(PonsObservationReject::LaunchFeeUnderpayment);
        }
        let call = launchTokenCall::abi_decode(input.calldata)
            .map_err(|_| PonsObservationReject::MalformedCalldata)?;
        if call.abi_encode().as_slice() != input.calldata {
            return Err(PonsObservationReject::MalformedCalldata);
        }
        if call.launchConfigId != U256::from(PONS_LAUNCH_CONFIG_ID)
            || call.dexConfigId != U256::from(PONS_DEX_CONFIG_ID)
        {
            return Err(PonsObservationReject::UnsupportedConfiguration);
        }
        Ok(ObservedLeaderAction {
            generation,
            leader: input.sender,
            action: PonsAction::Launch,
            market: PonsMarketIdentity::UnresolvedUntilReceipt {
                factory: generation.factory(),
                salt: call.salt,
            },
            launch: PonsLaunchIntent {
                name: call.params.name,
                symbol: call.params.symbol,
                logo: call.params.logo,
                description: call.params.description,
                socials: PonsLaunchSocials {
                    telegram: call.params.socials.telegram,
                    twitter: call.params.socials.twitter,
                    discord: call.params.socials.discord,
                    website: call.params.socials.website,
                    farcaster: call.params.socials.farcaster,
                },
                developer_wallet: call.params.devWallet,
                launch_config_id: call.launchConfigId,
                dex_config_id: call.dexConfigId,
                salt: call.salt,
                observed_value: input.value,
            },
        })
    }

    /// Create a fresh follower paper plan from a receipt-proved market and a
    /// warm local V3 state. Leader route bytes, value, and minimum output are
    /// not accepted by this API and therefore cannot be copied.
    pub fn plan_paper_entry(
        &self,
        observed: &ObservedLeaderAction,
        market: &VerifiedPonsMarket,
        request: PonsPaperRequest,
    ) -> Result<FollowerTradePlan, PonsPaperPlanError> {
        validate_market(observed, market)?;
        if request.recipient == Address::ZERO
            || request.spend == U256::ZERO
            || request.max_slippage_bps > 10_000
        {
            return Err(PonsPaperPlanError::InvalidFollowerPolicy);
        }
        let quote = market
            .pool_state
            .quote_exact_input(PONS_WETH, request.spend, None)?;
        let retained_bps = U256::from(10_000_u64 - u64::from(request.max_slippage_bps));
        let minimum_receive = quote
            .amount_out
            .checked_mul(retained_bps)
            .ok_or(PonsPaperPlanError::ArithmeticOverflow)?
            / U256::from(10_000_u64);
        if minimum_receive == U256::ZERO {
            return Err(PonsPaperPlanError::InvalidFollowerPolicy);
        }
        let calldata = encode_v3_exact_input_single(&V3ExactInputIntent {
            token_in: PONS_WETH,
            token_out: quote.token_out,
            fee: PONS_POOL_FEE,
            recipient: request.recipient,
            amount_in: request.spend,
            amount_out_minimum: minimum_receive,
            sqrt_price_limit_x96: U256::ZERO,
        })
        .ok_or(PonsPaperPlanError::CalldataConstruction)?;
        Ok(FollowerTradePlan {
            generation: observed.generation,
            mode: PonsPlanMode::PaperOnly,
            route: PonsPaperRoute::PrewrappedWethV3SingleHop,
            destination: PONS_SWAP_ROUTER_02,
            value: U256::ZERO,
            calldata: Bytes::from(calldata),
            spend_limit: request.spend,
            minimum_receive,
            expected_market: VerifiedMarketIdentity {
                token: market.token,
                pool: market.pool_state.pool,
            },
            quote,
        })
    }

    pub const fn prediction_kind(&self) -> PonsPredictionKind {
        PonsPredictionKind::DisabledIncompleteEvidence
    }

    pub const fn execution_gate(&self) -> Result<(), PonsExecutionBlocked> {
        Err(PonsExecutionBlocked {
            missing_evidence: PONS_EXECUTION_GAPS,
        })
    }
}

pub const PONS_EXECUTION_GAPS: &[&str] = &[
    "verified current factory source and normalized token creation code",
    "proved current CREATE2 init-code construction",
    "complete buy sell value approval and frontend route semantics",
    "restriction flags creator fees and locker claims",
    "onchain graduation predicate and post-graduation behavior",
    "quiet-window paper sample with zero identity prediction and quote mismatches",
    "separate explicit live canary authorization",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PonsAttributionProvenance {
    ExactFactoryTransaction,
    PageOrIndexerLabel,
}

#[derive(Debug, Clone, Copy)]
pub struct PonsObservationInput<'a> {
    pub chain_id: u64,
    pub destination: Address,
    pub destination_runtime_hash: B256,
    pub calldata: &'a [u8],
    pub value: U256,
    pub sender: Address,
    pub provenance: PonsAttributionProvenance,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PonsObservationReject {
    #[error("Pons observations require Robinhood mainnet chain 4663")]
    WrongChain,
    #[error("page or index attribution is not factory provenance")]
    MissingFactoryProvenance,
    #[error("destination is not a pinned Pons factory generation")]
    UnknownFactory,
    #[error("the generation-specific factory runtime has drifted")]
    RuntimeDrift,
    #[error("destination and selector do not form a Pons launch dispatch key")]
    WrongSelector,
    #[error("launch value is below the pinned launch fee")]
    LaunchFeeUnderpayment,
    #[error("launch calldata is malformed or noncanonical")]
    MalformedCalldata,
    #[error("launch or DEX configuration is not the pinned profile zero")]
    UnsupportedConfiguration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PonsLaunchSocials {
    pub telegram: String,
    pub twitter: String,
    pub discord: String,
    pub website: String,
    pub farcaster: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PonsLaunchIntent {
    pub name: String,
    pub symbol: String,
    pub logo: String,
    pub description: String,
    pub socials: PonsLaunchSocials,
    pub developer_wallet: Address,
    pub launch_config_id: U256,
    pub dex_config_id: U256,
    pub salt: B256,
    pub observed_value: U256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PonsAction {
    Launch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PonsMarketIdentity {
    UnresolvedUntilReceipt { factory: Address, salt: B256 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedLeaderAction {
    pub generation: PonsGeneration,
    pub leader: Address,
    pub action: PonsAction,
    pub market: PonsMarketIdentity,
    pub launch: PonsLaunchIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PonsReceiptProvenance {
    ExactFactoryReceipt {
        emitter: Address,
        topic0: B256,
        lp_locker: Address,
        no_unexpected_burn_or_migration: bool,
    },
    PageOrIndexerLabel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPonsMarket {
    pub chain_id: u64,
    pub generation: PonsGeneration,
    pub factory_runtime_hash: B256,
    pub token: Address,
    pub quote_asset: Address,
    pub pool_state: V3PoolState,
    pub pool_created_emitter: Address,
    pub pool_fee: u32,
    pub tick_spacing: i32,
    pub provenance: PonsReceiptProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PonsPaperRequest {
    pub recipient: Address,
    pub spend: U256,
    pub max_slippage_bps: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PonsPlanMode {
    PaperOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PonsPaperRoute {
    PrewrappedWethV3SingleHop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VerifiedMarketIdentity {
    pub token: Address,
    pub pool: Address,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FollowerTradePlan {
    pub generation: PonsGeneration,
    pub mode: PonsPlanMode,
    pub route: PonsPaperRoute,
    pub destination: Address,
    pub value: U256,
    pub calldata: Bytes,
    pub spend_limit: U256,
    pub minimum_receive: U256,
    pub expected_market: VerifiedMarketIdentity,
    pub quote: V3Quote,
}

#[derive(Debug, Error)]
pub enum PonsPaperPlanError {
    #[error("paper market lacks exact generation factory receipt provenance")]
    MissingFactoryProvenance,
    #[error("paper market does not match the observation generation")]
    GenerationMismatch,
    #[error("legacy paper planning is disabled because the locker runtime is not pinned")]
    LegacyRuntimeEvidenceIncomplete,
    #[error("paper market does not match the pinned Pons V3 configuration")]
    MarketConfigurationMismatch,
    #[error("follower paper policy is invalid")]
    InvalidFollowerPolicy,
    #[error("follower amount arithmetic overflowed")]
    ArithmeticOverflow,
    #[error("fresh follower calldata could not be constructed")]
    CalldataConstruction,
    #[error(transparent)]
    Quote(#[from] V3PoolError),
}

fn validate_market(
    observed: &ObservedLeaderAction,
    market: &VerifiedPonsMarket,
) -> Result<(), PonsPaperPlanError> {
    if observed.generation != market.generation {
        return Err(PonsPaperPlanError::GenerationMismatch);
    }
    if market.generation == PonsGeneration::Legacy {
        return Err(PonsPaperPlanError::LegacyRuntimeEvidenceIncomplete);
    }
    let PonsReceiptProvenance::ExactFactoryReceipt {
        emitter,
        topic0,
        lp_locker,
        no_unexpected_burn_or_migration,
    } = market.provenance
    else {
        return Err(PonsPaperPlanError::MissingFactoryProvenance);
    };
    if market.chain_id != PONS_CHAIN_ID
        || emitter != market.generation.factory()
        || topic0 != PONS_TOKEN_LAUNCHED_TOPIC
        || lp_locker != market.generation.locker()
        || !no_unexpected_burn_or_migration
        || market.factory_runtime_hash != market.generation.factory_runtime()
        || market.token == Address::ZERO
        || market.quote_asset != PONS_WETH
        || market.pool_created_emitter != PONS_V3_FACTORY
        || market.pool_fee != PONS_POOL_FEE
        || market.tick_spacing != PONS_TICK_SPACING
        || market.pool_state.pool == Address::ZERO
        || market.pool_state.fee != PONS_POOL_FEE
        || market.pool_state.tick_spacing != PONS_TICK_SPACING
        || !((market.pool_state.token0 == PONS_WETH && market.pool_state.token1 == market.token)
            || (market.pool_state.token1 == PONS_WETH && market.pool_state.token0 == market.token))
    {
        return Err(PonsPaperPlanError::MarketConfigurationMismatch);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PonsPredictionKind {
    DisabledIncompleteEvidence,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("Pons live execution is blocked by incomplete evidence")]
pub struct PonsExecutionBlocked {
    pub missing_evidence: &'static [&'static str],
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{FixedBytes, address};
    use alloy_sol_types::SolCall;
    use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

    use super::*;

    fn adapter() -> PonsAdapter {
        PonsAdapter::from_startup_identities(PonsAdapter::required_startup_identities()).unwrap()
    }

    fn fixture_calldata() -> Vec<u8> {
        launchTokenCall {
            params: PonsLaunchParams {
                name: "Ponshood".into(),
                symbol: "PONS".into(),
                logo: "ipfs://fixture".into(),
                description: "chain-4663 fixture".into(),
                socials: PonsSocials {
                    telegram: String::new(),
                    twitter: "https://x.com/pons".into(),
                    discord: String::new(),
                    website: "https://pons.family".into(),
                    farcaster: String::new(),
                },
                devWallet: address!("da4bcee76b29efec9697fcf663601c2042043968"),
            },
            launchConfigId: U256::ZERO,
            dexConfigId: U256::ZERO,
            salt: FixedBytes::with_last_byte(0x42),
        }
        .abi_encode()
    }

    fn observation_input(generation: PonsGeneration, calldata: &[u8]) -> PonsObservationInput<'_> {
        PonsObservationInput {
            chain_id: PONS_CHAIN_ID,
            destination: generation.factory(),
            destination_runtime_hash: generation.factory_runtime(),
            calldata,
            value: U256::from(PONS_LAUNCH_FEE_WEI),
            sender: Address::with_last_byte(7),
            provenance: PonsAttributionProvenance::ExactFactoryTransaction,
        }
    }

    #[test]
    fn evidence_snapshot_requires_every_exact_runtime_identity() {
        let required = PonsAdapter::required_startup_identities();
        assert_eq!(required.len(), 7);
        assert!(PonsAdapter::from_startup_identities(required).is_ok());
        assert_eq!(
            PonsAdapter::from_startup_identities(&required[..6]).unwrap_err(),
            PonsPinError::MissingOrDriftedIdentity
        );
        let mut drifted = required.to_vec();
        drifted[0].runtime_hash = B256::with_last_byte(1);
        assert!(PonsAdapter::from_startup_identities(&drifted).is_err());
    }

    #[test]
    fn positive_fixtures_distinguish_legacy_and_current_generation() {
        let calldata = fixture_calldata();
        assert_eq!(launchTokenCall::SELECTOR, PONS_LAUNCH_SELECTOR);
        let legacy = adapter()
            .observe_launch(observation_input(PonsGeneration::Legacy, &calldata))
            .unwrap();
        let current = adapter()
            .observe_launch(observation_input(PonsGeneration::Current, &calldata))
            .unwrap();
        assert_eq!(legacy.generation, PonsGeneration::Legacy);
        assert_eq!(current.generation, PonsGeneration::Current);
        assert_eq!(current.launch.name, "Ponshood");
        assert_ne!(legacy.market, current.market);
        assert_ne!(PONS_CURRENT_LAUNCH_FIXTURE_TX, B256::ZERO);
    }

    #[test]
    fn rejects_cross_generation_runtime_and_same_selector_lookalikes() {
        let calldata = fixture_calldata();
        let mut wrong_generation = observation_input(PonsGeneration::Current, &calldata);
        wrong_generation.destination_runtime_hash = PONS_LEGACY_FACTORY_RUNTIME;
        assert_eq!(
            adapter().observe_launch(wrong_generation).unwrap_err(),
            PonsObservationReject::RuntimeDrift
        );
        let mut lookalike = observation_input(PonsGeneration::Current, &calldata);
        lookalike.destination = Address::with_last_byte(0x99);
        assert_eq!(
            adapter().observe_launch(lookalike).unwrap_err(),
            PonsObservationReject::UnknownFactory
        );
    }

    #[test]
    fn rejects_cross_chain_page_attribution_underpayment_and_malformed_abi() {
        let calldata = fixture_calldata();
        let mut input = observation_input(PonsGeneration::Current, &calldata);
        input.chain_id = 8_453;
        assert_eq!(
            adapter().observe_launch(input).unwrap_err(),
            PonsObservationReject::WrongChain
        );

        let mut input = observation_input(PonsGeneration::Current, &calldata);
        input.provenance = PonsAttributionProvenance::PageOrIndexerLabel;
        assert_eq!(
            adapter().observe_launch(input).unwrap_err(),
            PonsObservationReject::MissingFactoryProvenance
        );

        let mut input = observation_input(PonsGeneration::Current, &calldata);
        input.value = U256::from(PONS_LAUNCH_FEE_WEI - 1);
        assert_eq!(
            adapter().observe_launch(input).unwrap_err(),
            PonsObservationReject::LaunchFeeUnderpayment
        );

        let mut trailing = calldata.clone();
        trailing.push(0);
        assert_eq!(
            adapter()
                .observe_launch(observation_input(PonsGeneration::Current, &trailing))
                .unwrap_err(),
            PonsObservationReject::MalformedCalldata
        );
        assert_eq!(
            adapter()
                .observe_launch(observation_input(PonsGeneration::Current, &calldata[..100]))
                .unwrap_err(),
            PonsObservationReject::MalformedCalldata
        );
    }

    fn current_observation() -> ObservedLeaderAction {
        let calldata = fixture_calldata();
        adapter()
            .observe_launch(observation_input(PonsGeneration::Current, &calldata))
            .unwrap()
    }

    fn current_market() -> VerifiedPonsMarket {
        let token = address!("432c99bbd9dc1d9040087598d7cf40502d7cc20b");
        let (token0, token1) = if PONS_WETH < token {
            (PONS_WETH, token)
        } else {
            (token, PONS_WETH)
        };
        let mut pool_state = V3PoolState::new(
            address!("f28f09dfe76860a9962a6915f356be2ce29c760d"),
            token0,
            token1,
            PONS_POOL_FEE,
            PONS_TICK_SPACING,
            get_sqrt_ratio_at_tick(0).unwrap(),
            0,
            0,
        )
        .unwrap();
        pool_state
            .add_position(-200, 200, 1_000_000_000_000_000_000)
            .unwrap();
        VerifiedPonsMarket {
            chain_id: PONS_CHAIN_ID,
            generation: PonsGeneration::Current,
            factory_runtime_hash: PONS_CURRENT_FACTORY_RUNTIME,
            token,
            quote_asset: PONS_WETH,
            pool_state,
            pool_created_emitter: PONS_V3_FACTORY,
            pool_fee: PONS_POOL_FEE,
            tick_spacing: PONS_TICK_SPACING,
            provenance: PonsReceiptProvenance::ExactFactoryReceipt {
                emitter: PONS_CURRENT_FACTORY,
                topic0: PONS_TOKEN_LAUNCHED_TOPIC,
                lp_locker: PONS_CURRENT_LOCKER,
                no_unexpected_burn_or_migration: true,
            },
        }
    }

    #[test]
    fn paper_plan_is_fresh_local_and_separate_from_observation() {
        let observed = current_observation();
        let plan = adapter()
            .plan_paper_entry(
                &observed,
                &current_market(),
                PonsPaperRequest {
                    recipient: Address::with_last_byte(8),
                    spend: U256::from(1_000_000_u64),
                    max_slippage_bps: 100,
                },
            )
            .unwrap();
        assert_eq!(plan.mode, PonsPlanMode::PaperOnly);
        assert_eq!(plan.destination, PONS_SWAP_ROUTER_02);
        assert_eq!(plan.value, U256::ZERO);
        assert_eq!(&plan.calldata[..4], &[0x04, 0xe4, 0x5a, 0xaf]);
        assert_eq!(
            plan.minimum_receive,
            plan.quote.amount_out * U256::from(9_900) / U256::from(10_000)
        );
    }

    #[test]
    fn paper_planning_rejects_page_provenance_generation_and_legacy_gaps() {
        let observed = current_observation();
        let mut market = current_market();
        market.provenance = PonsReceiptProvenance::PageOrIndexerLabel;
        assert!(matches!(
            adapter().plan_paper_entry(
                &observed,
                &market,
                PonsPaperRequest {
                    recipient: Address::with_last_byte(8),
                    spend: U256::from(1),
                    max_slippage_bps: 1
                }
            ),
            Err(PonsPaperPlanError::MissingFactoryProvenance)
        ));

        let calldata = fixture_calldata();
        let legacy = adapter()
            .observe_launch(observation_input(PonsGeneration::Legacy, &calldata))
            .unwrap();
        market.generation = PonsGeneration::Legacy;
        market.provenance = PonsReceiptProvenance::ExactFactoryReceipt {
            emitter: PONS_LEGACY_FACTORY,
            topic0: PONS_TOKEN_LAUNCHED_TOPIC,
            lp_locker: PONS_LEGACY_LOCKER,
            no_unexpected_burn_or_migration: true,
        };
        assert!(matches!(
            adapter().plan_paper_entry(
                &legacy,
                &market,
                PonsPaperRequest {
                    recipient: Address::with_last_byte(8),
                    spend: U256::from(1),
                    max_slippage_bps: 1
                }
            ),
            Err(PonsPaperPlanError::LegacyRuntimeEvidenceIncomplete)
        ));
    }

    #[test]
    fn prediction_and_execution_gates_are_explicitly_closed() {
        let adapter = adapter();
        assert_eq!(
            adapter.prediction_kind(),
            PonsPredictionKind::DisabledIncompleteEvidence
        );
        let blocked = adapter.execution_gate().unwrap_err();
        assert_eq!(blocked.missing_evidence, PONS_EXECUTION_GAPS);
        assert!(
            blocked
                .missing_evidence
                .iter()
                .any(|gap| gap.contains("CREATE2"))
        );
    }

    #[test]
    fn candidate_surface_is_synchronous_and_has_no_io_capability() {
        let _: fn(
            &PonsAdapter,
            PonsObservationInput<'_>,
        ) -> Result<ObservedLeaderAction, PonsObservationReject> = PonsAdapter::observe_launch;
        assert_eq!(std::mem::size_of::<PonsAdapter>(), 0);
    }
}
