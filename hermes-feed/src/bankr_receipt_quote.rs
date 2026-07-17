//! Strict, receipt-end Bankr/Doppler V4 paper quotes.
//!
//! The public production admission path performs read-only historical code and
//! block RPC checks at the confirmed receipt block. It never signs or executes.
//! After verifying one startup-pinned EntryPoint v0.7/EIP-7702 account, it
//! validates the exact reviewed Bankr standard profile, reconstructs its V4
//! concentrated-liquidity state, and simulates an independent tiny WETH entry
//! plus immediate full-position exit.

use alloy_primitives::{Address, B256, U256, keccak256};
use alloy_sol_types::{SolCall, SolEvent, SolValue};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

use crate::launchpad_adapter::LaunchpadId;
use crate::noxa_abi::ReceiptLog;
use crate::noxa_rpc::{NoxaReceipt, NoxaRpcClient, RobinhoodBlock, RobinhoodTransaction};
use crate::robinhood::{CHAIN_ID, WETH, WETH_RUNTIME_KECCAK256};
use crate::smart_account::{
    AccountExecutionProfile, ContractPin, ENTRY_POINT_V07, EntryPointCall, SmartAccountPin,
    SmartAccountPins, decode_entry_point_v07, discover_entry_point_v07_erc7579,
};
use crate::uniswap_v4::{CodePin, DYNAMIC_FEE_FLAG, V4PoolKey};
use crate::v3_pool::{V3PoolError, V3PoolState, V3Quote};

const BPS_DENOMINATOR: u16 = 10_000;
const WAD: U256 = U256::from_limbs([1_000_000_000_000_000_000, 0, 0, 0]);

pub const BANKR_AIRLOCK: Address =
    alloy_primitives::address!("eb7c034704ef8dcd2d32324c1545f62fb4ad0862");
pub const BANKR_DOPPLER_INITIALIZER: Address =
    alloy_primitives::address!("4e3468951d49f2eea976ed0d6e75ffcb44a9a544");
pub const BANKR_REHYPE_HOOK: Address =
    alloy_primitives::address!("6f02324d20cc679d0e585290caa6b16bacbc0f77");
pub const BANKR_TOKEN_FACTORY: Address =
    alloy_primitives::address!("1b37d3a72082029c44b35b604ea473617580b69a");
pub const BANKR_TOKEN_IMPLEMENTATION: Address =
    alloy_primitives::address!("3be8b97fd0e713b5abe0649fa830223b6b4bc599");
pub const BANKR_GOVERNANCE_FACTORY: Address =
    alloy_primitives::address!("db036746d65dd52126b1915f1adf555e6c5237cf");
pub const BANKR_LIQUIDITY_MIGRATOR: Address =
    alloy_primitives::address!("ba2f330edb16cd8056f5988d8ce19bbc63475a0e");
pub const BANKR_PROTOCOL_BENEFICIARY: Address =
    alloy_primitives::address!("edeaa06e2eb42a5c19ce27c6cffb36fd4fe1eda8");
pub const BANKR_INTEGRATOR: Address =
    alloy_primitives::address!("f60633d02690e2a15a54ab919925f3d038df163e");
pub const BANKR_PROOF_ACCOUNT: Address =
    alloy_primitives::address!("ff89978cb8171132395741b785d4a1f7e3efa124");
pub const BANKR_KERNEL_IMPLEMENTATION: Address =
    alloy_primitives::address!("d6cedde84be40893d153be9d467cd6ad37875b28");
pub const V4_POOL_MANAGER: Address =
    alloy_primitives::address!("8366a39cc670b4001a1121b8f6a443a643e40951");

pub const BANKR_AIRLOCK_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("86b37100cbe9841771c452a592985b4e921254b127a380246073b84ec953f7f8");
pub const V4_POOL_MANAGER_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("bd3881180b547f5fe817545743cfb4343e96b1bc6640dcd70c106b0066e95626");
pub const BANKR_DOPPLER_INITIALIZER_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("c41a91106002f15bf70ae266824317f3f3ac638ac72ca5253bae395fa47ee631");
pub const BANKR_REHYPE_HOOK_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("5d33a1d867ba0d17cc7af077786b1356848c72f8e0bf960ef88aa15f7a6962d1");
pub const BANKR_TOKEN_FACTORY_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("27abd63146eb5743b7871e211da17163afbb495863a626c0d002312af6813459");
pub const BANKR_TOKEN_IMPLEMENTATION_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("67a382a66d2b14a7032698e11c9ae4432435d2c803429d5c660692289ad10e12");
pub const BANKR_GOVERNANCE_FACTORY_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("efce8ac4a6fe83ae3dd1c3cfebc0e370e1595a66608bed5610ffdd1f291b7f63");
pub const BANKR_LIQUIDITY_MIGRATOR_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("7bf5115543e8e0769ceabe4da9b8e23547c9e95c1cce15d24d96f164406129e3");
pub const ENTRY_POINT_V07_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("8db5ff695839d655407cc8490bb7a5d82337a86a6b39c3f0258aa6c3b582fc58");
pub const BANKR_ACCOUNT_DESIGNATOR_HASH: B256 =
    alloy_primitives::b256!("4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4");
pub const BANKR_KERNEL_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("6f6d6691dc11fda98d3102802f20e7e816ccc576c16c9279ee1a884a51d1935d");

mod abi {
    use alloy_sol_types::sol;

    sol! {
        struct AirlockCreateParams {
            uint256 initialSupply;
            uint256 numTokensToSell;
            address numeraire;
            address tokenFactory;
            bytes tokenFactoryData;
            address governanceFactory;
            bytes governanceFactoryData;
            address poolInitializer;
            bytes poolInitializerData;
            address liquidityMigrator;
            bytes liquidityMigratorData;
            address integrator;
            bytes32 salt;
        }

        function create(AirlockCreateParams createData) external;

        struct VestingSchedule {
            uint64 cliff;
            uint64 duration;
        }

        function tokenFactoryData(
            string name,
            string symbol,
            VestingSchedule[] schedules,
            address[] beneficiaries,
            uint256[] scheduleIds,
            uint256[] amounts,
            string tokenURI,
            uint256 maxBalanceLimit,
            uint48 balanceLimitEnd,
            address controller,
            address[] excludedFromBalanceLimit
        ) external;

        struct Curve {
            int24 tickLower;
            int24 tickUpper;
            uint16 numPositions;
            uint256 shares;
        }

        struct BeneficiaryData {
            address beneficiary;
            uint96 shares;
        }

        struct DopplerInitData {
            uint24 fee;
            int24 tickSpacing;
            int24 farTick;
            Curve[] curves;
            BeneficiaryData[] beneficiaries;
            address dopplerHook;
            bytes onInitializationDopplerHookCalldata;
            bytes graduationDopplerHookCalldata;
        }

        struct FeeDistributionInfo {
            uint256 assetFeesToAssetBuybackWad;
            uint256 assetFeesToNumeraireBuybackWad;
            uint256 assetFeesToBeneficiaryWad;
            uint256 assetFeesToLpWad;
            uint256 numeraireFeesToAssetBuybackWad;
            uint256 numeraireFeesToNumeraireBuybackWad;
            uint256 numeraireFeesToBeneficiaryWad;
            uint256 numeraireFeesToLpWad;
        }

        struct RehypeInitData {
            address numeraire;
            address buybackDst;
            uint24 startFee;
            uint24 endFee;
            uint32 durationSeconds;
            uint32 startingTime;
            uint8 feeRoutingMode;
            FeeDistributionInfo feeDistributionInfo;
        }

        event Initialize(
            bytes32 indexed id,
            address indexed currency0,
            address indexed currency1,
            uint24 fee,
            int24 tickSpacing,
            address hooks,
            uint160 sqrtPriceX96,
            int24 tick
        );

        event ModifyLiquidity(
            bytes32 indexed id,
            address indexed sender,
            int24 tickLower,
            int24 tickUpper,
            int256 liquidityDelta,
            bytes32 salt
        );

        event Swap(
            bytes32 indexed id,
            address indexed sender,
            int128 amount0,
            int128 amount1,
            uint160 sqrtPriceX96,
            uint128 liquidity,
            int24 tick,
            uint24 fee
        );

        event Lock(address indexed pool, BeneficiaryData[] beneficiaries);

        event FeeScheduleSet(
            bytes32 indexed poolId,
            uint32 startingTime,
            uint24 startFee,
            uint24 endFee,
            uint32 durationSeconds
        );

        event UserOperationEvent(
            bytes32 indexed userOpHash,
            address indexed sender,
            address indexed paymaster,
            uint256 nonce,
            bool success,
            uint256 actualGasCost,
            uint256 actualGasUsed
        );
    }
}

mod airlock_events {
    use alloy_sol_types::sol;

    sol! {
        event Create(
            address asset,
            address indexed numeraire,
            address initializer,
            address poolOrHook
        );
    }
}

mod initializer_events {
    use alloy_sol_types::sol;

    sol! {
        event Create(
            address indexed poolOrHook,
            address indexed asset,
            address indexed numeraire
        );
    }
}

pub const BANKR_CREATE_SELECTOR: [u8; 4] = abi::createCall::SELECTOR;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BankrCreateProfileVersion {
    CurveTicksV1,
    CurveTicksV2,
    CurveTicksV3,
    CurveTicksV4,
    CurveTicksV5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BankrDopplerExpectedProfile {
    pub airlock: CodePin,
    pub pool_manager: CodePin,
    pub initializer: CodePin,
    pub rehype_hook: CodePin,
    pub token_factory: CodePin,
    pub token_implementation: CodePin,
    pub governance_factory: CodePin,
    pub liquidity_migrator: CodePin,
    pub weth: CodePin,
    pub entry_point: ContractPin,
    pub smart_account: SmartAccountPin,
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

impl BankrDopplerExpectedProfile {
    pub const fn production() -> Self {
        Self {
            airlock: CodePin {
                address: BANKR_AIRLOCK,
                runtime_code_hash: BANKR_AIRLOCK_RUNTIME_HASH,
            },
            pool_manager: CodePin {
                address: V4_POOL_MANAGER,
                runtime_code_hash: V4_POOL_MANAGER_RUNTIME_HASH,
            },
            initializer: CodePin {
                address: BANKR_DOPPLER_INITIALIZER,
                runtime_code_hash: BANKR_DOPPLER_INITIALIZER_RUNTIME_HASH,
            },
            rehype_hook: CodePin {
                address: BANKR_REHYPE_HOOK,
                runtime_code_hash: BANKR_REHYPE_HOOK_RUNTIME_HASH,
            },
            token_factory: CodePin {
                address: BANKR_TOKEN_FACTORY,
                runtime_code_hash: BANKR_TOKEN_FACTORY_RUNTIME_HASH,
            },
            token_implementation: CodePin {
                address: BANKR_TOKEN_IMPLEMENTATION,
                runtime_code_hash: BANKR_TOKEN_IMPLEMENTATION_RUNTIME_HASH,
            },
            governance_factory: CodePin {
                address: BANKR_GOVERNANCE_FACTORY,
                runtime_code_hash: BANKR_GOVERNANCE_FACTORY_RUNTIME_HASH,
            },
            liquidity_migrator: CodePin {
                address: BANKR_LIQUIDITY_MIGRATOR,
                runtime_code_hash: BANKR_LIQUIDITY_MIGRATOR_RUNTIME_HASH,
            },
            weth: CodePin {
                address: WETH,
                runtime_code_hash: WETH_RUNTIME_KECCAK256,
            },
            entry_point: ContractPin {
                address: ENTRY_POINT_V07,
                runtime_code_hash: ENTRY_POINT_V07_RUNTIME_HASH,
            },
            smart_account: SmartAccountPin {
                account: ContractPin {
                    address: BANKR_PROOF_ACCOUNT,
                    runtime_code_hash: BANKR_ACCOUNT_DESIGNATOR_HASH,
                },
                execution_profile: AccountExecutionProfile::Erc7579SingleCall,
                factory: None,
                delegation_implementation: Some(ContractPin {
                    address: BANKR_KERNEL_IMPLEMENTATION,
                    runtime_code_hash: BANKR_KERNEL_RUNTIME_HASH,
                }),
            },
            standard_lp_fee_ppm: 7_000,
            max_lp_fee_ppm: 100_000,
            hook_fee_denominator_ppm: 800_000,
            hook_start_fee_ppm: 800_000,
            hook_end_fee_ppm: 5_000,
            hook_duration_seconds: 10,
            quote_delay_guard_seconds: 1,
            tick_spacing: 200,
            pool_allocation_bps: 8_500,
            primary_curve_share_bps: 9_900,
            secondary_curve_share_bps: 100,
            creator_beneficiary_bps: 9_500,
            protocol_beneficiary_bps: 500,
        }
    }

    pub fn validate(self) -> Result<(), BankrQuoteError> {
        if self != Self::production()
            || !self.airlock.is_complete()
            || !self.pool_manager.is_complete()
            || !self.initializer.is_complete()
            || !self.rehype_hook.is_complete()
            || !self.token_factory.is_complete()
            || !self.token_implementation.is_complete()
            || !self.governance_factory.is_complete()
            || !self.liquidity_migrator.is_complete()
            || !self.weth.is_complete()
        {
            return Err(BankrQuoteError::InvalidExpectedProfile);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct BankrDopplerQuotePolicy {
    pub amount_in: U256,
    pub max_amount_in: U256,
    pub slippage_bps: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BankrDopplerStateVersion {
    pub chain_id: u64,
    pub block_hash: B256,
    pub l2_block_number: u64,
    pub transaction_index: u64,
    pub terminal_log_index: u64,
    pub receipt_timestamp: u64,
    pub first_eligible_quote_timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BankrDopplerMarketEvidence {
    pub envelope: BankrEnvelopeKind,
    pub create_profile_version: BankrCreateProfileVersion,
    pub leader: Address,
    pub outer_bundler: Option<Address>,
    pub account_designator_hash: B256,
    pub delegation_implementation: Address,
    pub delegation_runtime_hash: B256,
    pub token: Address,
    pub pool_id: B256,
    pub quote_asset: Address,
    pub pool_manager: Address,
    pub initializer: Address,
    pub rehype_hook: Address,
    pub buyback_destination: Address,
    pub lp_fee_ppm: u32,
    pub hook_start_fee_ppm: u32,
    pub hook_end_fee_ppm: u32,
    pub hook_duration_seconds: u64,
    pub tick_spacing: i32,
    pub initialize_sqrt_price_x96: U256,
    pub initialize_tick: i32,
    pub initialize_log_index: u64,
    pub last_liquidity_log_index: u64,
    pub launch_log_index: u64,
    pub user_operation_log_index: Option<u64>,
    pub position_count: usize,
    pub positions: Vec<BankrDopplerPositionEvidence>,
}

/// Deterministic identities available from an exact reviewed Airlock create
/// call before any receipt exists. Uniswap V4 has no pool contract address, so
/// `pool_id` is the canonical `PoolKey` hash used by PoolManager.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct BankrDopplerPredictedIdentity {
    pub create_profile_version: BankrCreateProfileVersion,
    pub token: Address,
    pub pool_id: B256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct BankrDopplerPositionEvidence {
    pub pool_id: B256,
    pub sender: Address,
    pub tick_lower: i32,
    pub tick_upper: i32,
    pub liquidity: U256,
    pub salt: B256,
    pub log_index: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BankrEnvelopeKind {
    Erc7579,
    DirectAirlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerifiedBankrEnvelope {
    Erc7579 { smart_account: SmartAccountPin },
    DirectAirlock { smart_account: SmartAccountPin },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BankrDopplerPaperSwapQuote {
    pub amount_in: U256,
    pub core_expected_output: U256,
    pub hook_output_fee: U256,
    pub expected_output: U256,
    pub min_receive: U256,
    pub slippage_bps: u16,
    pub lp_fee_ppm: u32,
    pub hook_fee_ppm: u32,
    pub hook_fee_denominator_ppm: u32,
    pub core_state_after: V3Quote,
    pub internal_buyback_state_after: Option<V3Quote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BankrDopplerReceiptPaperQuote {
    pub record_type: String,
    pub tx_hash: B256,
    pub launchpad: LaunchpadId,
    pub l2_block_number: u64,
    pub state_version: BankrDopplerStateVersion,
    pub quote_source: String,
    pub sizing_source: String,
    pub market: BankrDopplerMarketEvidence,
    pub entry: BankrDopplerPaperSwapQuote,
    pub full_position_exit: BankrDopplerPaperSwapQuote,
    pub simulated_round_trip_return_bps: U256,
    pub execution_eligible: bool,
    pub execution_blocker: String,
    pub broadcast: bool,
}

#[derive(Debug, Error)]
pub enum BankrQuoteError {
    #[error("expected Bankr/Doppler profile is incomplete or invalid")]
    InvalidExpectedProfile,
    #[error("receipt, transaction, or block envelope does not match")]
    InvalidEnvelope,
    #[error("paper sizing or slippage policy is unsafe")]
    UnsafePolicy,
    #[error("Bankr ERC-4337 account call is not the pinned proof profile")]
    SmartAccountIdentity,
    #[error("receipt-block Bankr account identity proof failed: {0}")]
    ReceiptBlockIdentity(String),
    #[error("Airlock create calldata is malformed or not the reviewed standard profile")]
    CreateCalldata,
    #[error("receipt launch identity is missing, duplicated, or inconsistent")]
    LaunchIdentity,
    #[error("pool initialization identity is inconsistent")]
    InitializeIdentity,
    #[error("pool liquidity sequence is incomplete or inconsistent")]
    LiquiditySequence,
    #[error("hook fee schedule is incomplete or inconsistent")]
    FeeSchedule,
    #[error("ERC-4337 success evidence is incomplete or inconsistent")]
    UserOperationEvidence,
    #[error("launch receipt contains an embedded pool swap")]
    EmbeddedSwapUnsupported,
    #[error("arithmetic overflow")]
    ArithmeticOverflow,
    #[error(transparent)]
    Pool(#[from] V3PoolError),
}

#[derive(Debug, Clone, Copy)]
struct LaunchEvidence {
    token: Address,
    pool_id: B256,
    sqrt_price_x96: U256,
    initialize_tick: i32,
    initialize_log_index: u64,
    last_liquidity_log_index: u64,
    launch_log_index: u64,
    user_operation_log_index: Option<u64>,
    schedule_start: u64,
    buyback_destination: Address,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReceiptBlockRuntimeObservation {
    address: Address,
    runtime_code_hash: B256,
    code_bytes: usize,
}

trait BankrReceiptBlockRpc {
    async fn code_at_l2_block(
        &self,
        address: Address,
        l2_block_number: u64,
    ) -> Result<alloy_primitives::Bytes, String>;

    async fn runtime_observation_at_l2_block(
        &self,
        address: Address,
        l2_block_number: u64,
    ) -> Result<ReceiptBlockRuntimeObservation, String>;

    async fn block_by_number(&self, l2_block_number: u64) -> Result<RobinhoodBlock, String>;
}

impl BankrReceiptBlockRpc for NoxaRpcClient {
    async fn code_at_l2_block(
        &self,
        address: Address,
        l2_block_number: u64,
    ) -> Result<alloy_primitives::Bytes, String> {
        NoxaRpcClient::code_at_l2_block(self, address, l2_block_number)
            .await
            .map_err(|error| error.to_string())
    }

    async fn runtime_observation_at_l2_block(
        &self,
        address: Address,
        l2_block_number: u64,
    ) -> Result<ReceiptBlockRuntimeObservation, String> {
        let code = NoxaRpcClient::code_at_l2_block(self, address, l2_block_number)
            .await
            .map_err(|error| error.to_string())?;
        Ok(ReceiptBlockRuntimeObservation {
            address,
            runtime_code_hash: keccak256(&code),
            code_bytes: code.len(),
        })
    }

    async fn block_by_number(&self, l2_block_number: u64) -> Result<RobinhoodBlock, String> {
        NoxaRpcClient::block_by_number(self, l2_block_number)
            .await
            .map_err(|error| error.to_string())
    }
}

/// Verify the transaction's EIP-7702 account identity at the canonical receipt
/// block, then reconstruct a non-broadcast Bankr quote. This is the only public
/// quote admission path for rotating or direct Bankr accounts.
pub async fn quote_bankr_doppler_launch_receipt_at_receipt_block(
    rpc: &NoxaRpcClient,
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
    profile: BankrDopplerExpectedProfile,
    policy: BankrDopplerQuotePolicy,
) -> Result<BankrDopplerReceiptPaperQuote, BankrQuoteError> {
    quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
        rpc,
        transaction,
        receipt,
        block,
        profile,
        policy,
    )
    .await
}

async fn quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc<
    R: BankrReceiptBlockRpc + Sync,
>(
    rpc: &R,
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
    profile: BankrDopplerExpectedProfile,
    policy: BankrDopplerQuotePolicy,
) -> Result<BankrDopplerReceiptPaperQuote, BankrQuoteError> {
    profile.validate()?;
    let (leader, erc7579) = if transaction.to == Some(profile.entry_point.address) {
        let discovered = discover_entry_point_v07_erc7579(
            EntryPointCall {
                chain_id: CHAIN_ID,
                destination: profile.entry_point,
                outer_bundler: transaction.from,
                calldata: &transaction.input,
            },
            profile.entry_point,
            ContractPin {
                address: profile.airlock.address,
                runtime_code_hash: profile.airlock.runtime_code_hash,
            },
        )
        .map_err(|error| BankrQuoteError::ReceiptBlockIdentity(error.to_string()))?;
        if discovered.value != U256::ZERO {
            return Err(BankrQuoteError::ReceiptBlockIdentity(
                "discovered account call carries native value".into(),
            ));
        }
        (discovered.leader, true)
    } else if transaction.to == Some(profile.airlock.address) && transaction.value == U256::ZERO {
        (transaction.from, false)
    } else {
        return Err(BankrQuoteError::ReceiptBlockIdentity(
            "transaction is neither exact EntryPoint nor direct Airlock envelope".into(),
        ));
    };
    if leader == Address::ZERO {
        return Err(BankrQuoteError::ReceiptBlockIdentity(
            "envelope has zero leader".into(),
        ));
    }
    let delegation = profile
        .smart_account
        .delegation_implementation
        .ok_or_else(|| {
            BankrQuoteError::ReceiptBlockIdentity(
                "expected profile has no delegated implementation".into(),
            )
        })?;
    let account_code = rpc
        .code_at_l2_block(leader, receipt.l2_block_number)
        .await
        .map_err(BankrQuoteError::ReceiptBlockIdentity)?;
    let delegated_code = rpc
        .code_at_l2_block(delegation.address, receipt.l2_block_number)
        .await
        .map_err(BankrQuoteError::ReceiptBlockIdentity)?;
    let smart_account =
        verified_smart_account_from_receipt_code(leader, &account_code, &delegated_code, profile)?;
    verify_bankr_dependencies_at_receipt_block(rpc, receipt.l2_block_number, profile, erc7579)
        .await?;
    let stable_block = rpc
        .block_by_number(receipt.l2_block_number)
        .await
        .map_err(BankrQuoteError::ReceiptBlockIdentity)?;
    if stable_block != *block || stable_block.hash != receipt.block_hash {
        return Err(BankrQuoteError::ReceiptBlockIdentity(
            "receipt block changed during identity verification".into(),
        ));
    }
    let envelope = if erc7579 {
        VerifiedBankrEnvelope::Erc7579 { smart_account }
    } else {
        VerifiedBankrEnvelope::DirectAirlock { smart_account }
    };
    quote_bankr_doppler_launch_receipt_verified(
        transaction,
        receipt,
        block,
        profile,
        policy,
        envelope,
    )
}

fn verified_smart_account_from_receipt_code(
    leader: Address,
    account_code: &[u8],
    delegated_code: &[u8],
    profile: BankrDopplerExpectedProfile,
) -> Result<SmartAccountPin, BankrQuoteError> {
    if leader == Address::ZERO {
        return Err(BankrQuoteError::ReceiptBlockIdentity(
            "envelope has zero leader".into(),
        ));
    }
    let delegation = profile
        .smart_account
        .delegation_implementation
        .ok_or_else(|| {
            BankrQuoteError::ReceiptBlockIdentity(
                "expected profile has no delegated implementation".into(),
            )
        })?;
    let mut expected_designator = Vec::with_capacity(23);
    expected_designator.extend_from_slice(&[0xef, 0x01, 0x00]);
    expected_designator.extend_from_slice(delegation.address.as_slice());
    if account_code != expected_designator
        || keccak256(account_code) != profile.smart_account.account.runtime_code_hash
    {
        return Err(BankrQuoteError::ReceiptBlockIdentity(
            "leader designator disagrees with reviewed profile".into(),
        ));
    }
    if delegated_code.is_empty() || keccak256(delegated_code) != delegation.runtime_code_hash {
        return Err(BankrQuoteError::ReceiptBlockIdentity(
            "delegated Kernel runtime disagrees with reviewed profile".into(),
        ));
    }
    Ok(SmartAccountPin {
        account: ContractPin {
            address: leader,
            runtime_code_hash: profile.smart_account.account.runtime_code_hash,
        },
        factory: None,
        execution_profile: AccountExecutionProfile::Erc7579SingleCall,
        delegation_implementation: Some(delegation),
    })
}

async fn verify_bankr_dependencies_at_receipt_block<R: BankrReceiptBlockRpc + Sync>(
    rpc: &R,
    l2_block_number: u64,
    profile: BankrDopplerExpectedProfile,
    require_entry_point: bool,
) -> Result<(), BankrQuoteError> {
    let expected = bankr_receipt_block_dependency_pins(profile, require_entry_point);
    let mut observed = Vec::with_capacity(expected.len());
    for pin in &expected {
        let observation = rpc
            .runtime_observation_at_l2_block(pin.address, l2_block_number)
            .await
            .map_err(BankrQuoteError::ReceiptBlockIdentity)?;
        observed.push(observation);
    }
    validate_bankr_receipt_block_dependencies(&expected, &observed)
}

fn bankr_receipt_block_dependency_pins(
    profile: BankrDopplerExpectedProfile,
    require_entry_point: bool,
) -> Vec<ContractPin> {
    let contract_pin = |pin: CodePin| ContractPin {
        address: pin.address,
        runtime_code_hash: pin.runtime_code_hash,
    };
    let mut pins = vec![
        contract_pin(profile.airlock),
        contract_pin(profile.pool_manager),
        contract_pin(profile.initializer),
        contract_pin(profile.rehype_hook),
        contract_pin(profile.token_factory),
        contract_pin(profile.token_implementation),
        contract_pin(profile.governance_factory),
        contract_pin(profile.liquidity_migrator),
        contract_pin(profile.weth),
    ];
    if require_entry_point {
        pins.push(profile.entry_point);
    }
    pins
}

fn validate_bankr_receipt_block_dependencies(
    expected: &[ContractPin],
    observed: &[ReceiptBlockRuntimeObservation],
) -> Result<(), BankrQuoteError> {
    if expected.len() != observed.len() {
        return Err(BankrQuoteError::ReceiptBlockIdentity(
            "receipt-block dependency proof is incomplete".into(),
        ));
    }
    for (pin, observation) in expected.iter().zip(observed) {
        if observation.address != pin.address
            || observation.code_bytes == 0
            || observation.runtime_code_hash != pin.runtime_code_hash
        {
            return Err(BankrQuoteError::ReceiptBlockIdentity(format!(
                "receipt-block dependency {} disagrees with reviewed profile",
                pin.address
            )));
        }
    }
    Ok(())
}

/// Private proof-fixture path. Production callers must use the receipt-block
/// verifier above.
#[cfg(test)]
fn quote_bankr_doppler_launch_receipt(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
    profile: BankrDopplerExpectedProfile,
    policy: BankrDopplerQuotePolicy,
) -> Result<BankrDopplerReceiptPaperQuote, BankrQuoteError> {
    quote_bankr_doppler_launch_receipt_verified(
        transaction,
        receipt,
        block,
        profile,
        policy,
        VerifiedBankrEnvelope::Erc7579 {
            smart_account: profile.smart_account,
        },
    )
}

fn quote_bankr_doppler_launch_receipt_verified(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
    profile: BankrDopplerExpectedProfile,
    policy: BankrDopplerQuotePolicy,
    envelope: VerifiedBankrEnvelope,
) -> Result<BankrDopplerReceiptPaperQuote, BankrQuoteError> {
    profile.validate()?;
    let smart_account = match envelope {
        VerifiedBankrEnvelope::Erc7579 { smart_account }
        | VerifiedBankrEnvelope::DirectAirlock { smart_account } => smart_account,
    };
    validate_verified_smart_account(smart_account, profile)?;
    validate_envelope(transaction, receipt, block, policy, profile, envelope)?;

    let (envelope_kind, leader, outer_bundler, create_calldata, require_user_operation) =
        match envelope {
            VerifiedBankrEnvelope::Erc7579 { .. } => {
                let accounts = [smart_account];
                let targets = [ContractPin {
                    address: profile.airlock.address,
                    runtime_code_hash: profile.airlock.runtime_code_hash,
                }];
                let unwrapped = decode_entry_point_v07(
                    EntryPointCall {
                        chain_id: CHAIN_ID,
                        destination: profile.entry_point,
                        outer_bundler: transaction.from,
                        calldata: &transaction.input,
                    },
                    SmartAccountPins {
                        entry_point: profile.entry_point,
                        accounts: &accounts,
                        allowed_targets: &targets,
                    },
                )
                .map_err(|_| BankrQuoteError::SmartAccountIdentity)?;
                if unwrapped.leader != smart_account.account.address
                    || unwrapped.target != profile.airlock.address
                    || unwrapped.value != U256::ZERO
                    || unwrapped.execution_profile != AccountExecutionProfile::Erc7579SingleCall
                    || unwrapped.delegation_implementation
                        != smart_account
                            .delegation_implementation
                            .map(|pin| pin.address)
                {
                    return Err(BankrQuoteError::SmartAccountIdentity);
                }
                (
                    BankrEnvelopeKind::Erc7579,
                    unwrapped.leader,
                    Some(unwrapped.outer_bundler),
                    unwrapped.calldata,
                    true,
                )
            }
            VerifiedBankrEnvelope::DirectAirlock { .. } => (
                BankrEnvelopeKind::DirectAirlock,
                smart_account.account.address,
                None,
                transaction.input.clone(),
                false,
            ),
        };

    let create = abi::createCall::abi_decode(&create_calldata)
        .map_err(|_| BankrQuoteError::CreateCalldata)?;
    if create.abi_encode().as_slice() != create_calldata.as_ref() {
        return Err(BankrQuoteError::CreateCalldata);
    }
    let decoded = validate_create_calldata(&create, profile)?;
    if decoded.profile_version == BankrCreateProfileVersion::CurveTicksV5
        && envelope_kind != BankrEnvelopeKind::Erc7579
    {
        return Err(BankrQuoteError::SmartAccountIdentity);
    }
    let (evidence, positions) = validate_receipt(
        &receipt.logs,
        leader,
        &decoded,
        block.timestamp,
        profile,
        require_user_operation,
    )?;

    let first_eligible_quote_timestamp = evidence
        .schedule_start
        .checked_add(profile.quote_delay_guard_seconds)
        .ok_or(BankrQuoteError::ArithmeticOverflow)?;
    let hook_fee_ppm = bankr_hook_fee_ppm(
        evidence.schedule_start,
        profile.hook_start_fee_ppm,
        profile.hook_end_fee_ppm,
        profile.hook_duration_seconds,
        first_eligible_quote_timestamp,
    )?;
    if hook_fee_ppm >= profile.hook_fee_denominator_ppm {
        return Err(BankrQuoteError::FeeSchedule);
    }

    let key = V4PoolKey::canonical(
        WETH,
        evidence.token,
        DYNAMIC_FEE_FLAG,
        profile.tick_spacing,
        profile.initializer.address,
    )
    .map_err(|_| BankrQuoteError::InitializeIdentity)?;
    if key.pool_id() != evidence.pool_id {
        return Err(BankrQuoteError::InitializeIdentity);
    }
    let mut state = V3PoolState::new(
        profile.pool_manager.address,
        key.currency0,
        key.currency1,
        profile.standard_lp_fee_ppm,
        key.tick_spacing,
        evidence.sqrt_price_x96,
        evidence.initialize_tick,
        0,
    )?;
    for position in &positions {
        state.add_position(
            position.tick_lower,
            position.tick_upper,
            u128::try_from(position.liquidity).map_err(|_| BankrQuoteError::LiquiditySequence)?,
        )?;
    }
    let entry_core = state.quote_exact_input(WETH, policy.amount_in, None)?;
    validate_complete_quote(&entry_core, policy.amount_in)?;
    let entry_hook_fee = output_hook_fee(
        entry_core.amount_out,
        hook_fee_ppm,
        profile.hook_fee_denominator_ppm,
    )?;
    let entry_net = entry_core
        .amount_out
        .checked_sub(entry_hook_fee)
        .ok_or(BankrQuoteError::ArithmeticOverflow)?;
    if entry_net == U256::ZERO {
        return Err(BankrQuoteError::FeeSchedule);
    }

    state.set_observation(
        entry_core.sqrt_price_x96_after,
        entry_core.tick_after,
        entry_core.liquidity_after,
    )?;
    let owner_fee = entry_hook_fee
        .checked_mul(U256::from(profile.protocol_beneficiary_bps))
        .ok_or(BankrQuoteError::ArithmeticOverflow)?
        / U256::from(BPS_DENOMINATOR);
    let buyback_input = entry_hook_fee
        .checked_sub(owner_fee)
        .ok_or(BankrQuoteError::ArithmeticOverflow)?;
    let buyback = state.quote_exact_input(evidence.token, buyback_input, None)?;
    validate_complete_quote(&buyback, buyback_input)?;
    state.set_observation(
        buyback.sqrt_price_x96_after,
        buyback.tick_after,
        buyback.liquidity_after,
    )?;

    let exit_core = state.quote_exact_input(evidence.token, entry_net, None)?;
    validate_complete_quote(&exit_core, entry_net)?;
    let exit_hook_fee = output_hook_fee(
        exit_core.amount_out,
        hook_fee_ppm,
        profile.hook_fee_denominator_ppm,
    )?;
    let exit_net = exit_core
        .amount_out
        .checked_sub(exit_hook_fee)
        .ok_or(BankrQuoteError::ArithmeticOverflow)?;
    let entry_min = apply_slippage(entry_net, policy.slippage_bps)?;
    let exit_min = apply_slippage(exit_net, policy.slippage_bps)?;
    let round_trip_return_bps = exit_net
        .checked_mul(U256::from(BPS_DENOMINATOR))
        .ok_or(BankrQuoteError::ArithmeticOverflow)?
        / policy.amount_in;

    Ok(BankrDopplerReceiptPaperQuote {
        record_type: "launchpad_bankr_doppler_v4_paper_quote".into(),
        tx_hash: receipt.transaction_hash,
        launchpad: LaunchpadId::BankrDoppler,
        l2_block_number: receipt.l2_block_number,
        state_version: BankrDopplerStateVersion {
            chain_id: CHAIN_ID,
            block_hash: receipt.block_hash,
            l2_block_number: receipt.l2_block_number,
            transaction_index: receipt.transaction_index,
            terminal_log_index: evidence
                .user_operation_log_index
                .unwrap_or(evidence.launch_log_index),
            receipt_timestamp: block.timestamp,
            first_eligible_quote_timestamp,
        },
        quote_source: "confirmed_receipt_end_bankr_doppler_first_nonzero_state".into(),
        sizing_source: "independent_fixed_tiny_weth_policy".into(),
        market: BankrDopplerMarketEvidence {
            envelope: envelope_kind,
            create_profile_version: decoded.profile_version,
            leader,
            outer_bundler,
            account_designator_hash: smart_account.account.runtime_code_hash,
            delegation_implementation: smart_account
                .delegation_implementation
                .map(|pin| pin.address)
                .ok_or(BankrQuoteError::SmartAccountIdentity)?,
            delegation_runtime_hash: smart_account
                .delegation_implementation
                .map(|pin| pin.runtime_code_hash)
                .ok_or(BankrQuoteError::SmartAccountIdentity)?,
            token: evidence.token,
            pool_id: evidence.pool_id,
            quote_asset: WETH,
            pool_manager: profile.pool_manager.address,
            initializer: profile.initializer.address,
            rehype_hook: profile.rehype_hook.address,
            buyback_destination: evidence.buyback_destination,
            lp_fee_ppm: profile.standard_lp_fee_ppm,
            hook_start_fee_ppm: profile.hook_start_fee_ppm,
            hook_end_fee_ppm: profile.hook_end_fee_ppm,
            hook_duration_seconds: profile.hook_duration_seconds,
            tick_spacing: profile.tick_spacing,
            initialize_sqrt_price_x96: evidence.sqrt_price_x96,
            initialize_tick: evidence.initialize_tick,
            initialize_log_index: evidence.initialize_log_index,
            last_liquidity_log_index: evidence.last_liquidity_log_index,
            launch_log_index: evidence.launch_log_index,
            user_operation_log_index: evidence.user_operation_log_index,
            position_count: positions.len(),
            positions,
        },
        entry: BankrDopplerPaperSwapQuote {
            amount_in: policy.amount_in,
            core_expected_output: entry_core.amount_out,
            hook_output_fee: entry_hook_fee,
            expected_output: entry_net,
            min_receive: entry_min,
            slippage_bps: policy.slippage_bps,
            lp_fee_ppm: profile.standard_lp_fee_ppm,
            hook_fee_ppm,
            hook_fee_denominator_ppm: profile.hook_fee_denominator_ppm,
            core_state_after: entry_core,
            internal_buyback_state_after: Some(buyback),
        },
        full_position_exit: BankrDopplerPaperSwapQuote {
            amount_in: entry_net,
            core_expected_output: exit_core.amount_out,
            hook_output_fee: exit_hook_fee,
            expected_output: exit_net,
            min_receive: exit_min,
            slippage_bps: policy.slippage_bps,
            lp_fee_ppm: profile.standard_lp_fee_ppm,
            hook_fee_ppm,
            hook_fee_denominator_ppm: profile.hook_fee_denominator_ppm,
            core_state_after: exit_core,
            internal_buyback_state_after: None,
        },
        simulated_round_trip_return_bps: round_trip_return_bps,
        execution_eligible: false,
        execution_blocker:
            "paper_only_bankr_rehype_router_permit2_and_account_execution_not_enabled".into(),
        broadcast: false,
    })
}

struct DecodedCreate {
    init: abi::DopplerInitData,
    rehype: abi::RehypeInitData,
    profile_version: BankrCreateProfileVersion,
}

/// Applies the canonical ABI and exact reviewed Bankr create-profile checks
/// without granting account identity or execution rights.
pub fn validate_bankr_create_calldata_for_observation(calldata: &[u8]) -> bool {
    predict_bankr_create_identity(calldata, BankrDopplerExpectedProfile::production()).is_ok()
}

/// Predict the token clone and canonical V4 pool ID from exact reviewed
/// Airlock calldata. This is pure candidate-time work: no receipt, RPC, signer,
/// or execution capability is consulted.
pub fn predict_bankr_create_identity(
    calldata: &[u8],
    profile: BankrDopplerExpectedProfile,
) -> Result<BankrDopplerPredictedIdentity, BankrQuoteError> {
    profile.validate()?;
    let call =
        abi::createCall::abi_decode(calldata).map_err(|_| BankrQuoteError::CreateCalldata)?;
    if call.abi_encode().as_slice() != calldata {
        return Err(BankrQuoteError::CreateCalldata);
    }
    let decoded = validate_create_calldata(&call, profile)?;

    let token = predict_bankr_token(&call, profile);
    if token == Address::ZERO || token == profile.weth.address {
        return Err(BankrQuoteError::CreateCalldata);
    }
    let pool_id = V4PoolKey::canonical(
        profile.weth.address,
        token,
        DYNAMIC_FEE_FLAG,
        profile.tick_spacing,
        profile.initializer.address,
    )
    .map_err(|_| BankrQuoteError::InitializeIdentity)?
    .pool_id();
    Ok(BankrDopplerPredictedIdentity {
        create_profile_version: decoded.profile_version,
        token,
        pool_id,
    })
}

fn predict_bankr_token(call: &abi::createCall, profile: BankrDopplerExpectedProfile) -> Address {
    // DopplerERC20V1Factory uses Solady LibClone.cloneDeterministic with the
    // immutable implementation pinned above. This is the exact Solady
    // CREATE2 init code, independent of token initialization calldata.
    let mut clone_init_code = Vec::with_capacity(54);
    clone_init_code.extend_from_slice(&alloy_primitives::hex!(
        "602c3d8160093d39f33d3d3d3d363d3d37363d73"
    ));
    clone_init_code.extend_from_slice(profile.token_implementation.address.as_slice());
    clone_init_code.extend_from_slice(&alloy_primitives::hex!("5af43d3d93803e602a57fd5bf3"));
    let init_code_hash = keccak256(&clone_init_code);
    let mut create2_preimage = Vec::with_capacity(85);
    create2_preimage.push(0xff);
    create2_preimage.extend_from_slice(profile.token_factory.address.as_slice());
    create2_preimage.extend_from_slice(call.createData.salt.as_slice());
    create2_preimage.extend_from_slice(init_code_hash.as_slice());
    let create2_hash = keccak256(create2_preimage);
    Address::from_slice(&create2_hash.as_slice()[12..])
}

fn validate_create_calldata(
    call: &abi::createCall,
    profile: BankrDopplerExpectedProfile,
) -> Result<DecodedCreate, BankrQuoteError> {
    let create = &call.createData;
    let expected_initial_supply = U256::from(100_000_000_000_000_000_u64)
        .checked_mul(U256::from(1_000_000_000_000_u64))
        .ok_or(BankrQuoteError::ArithmeticOverflow)?;
    let expected_tokens_to_sell = U256::from(85_000_000_000_000_000_u64)
        .checked_mul(U256::from(1_000_000_000_000_u64))
        .ok_or(BankrQuoteError::ArithmeticOverflow)?;
    if create.initialSupply != expected_initial_supply
        || create.numTokensToSell != expected_tokens_to_sell
        || create.numeraire != profile.weth.address
        || create.tokenFactory != profile.token_factory.address
        || !matches!(create.tokenFactoryData.len(), 928 | 960)
        || create.governanceFactory != profile.governance_factory.address
        || create.governanceFactoryData.as_ref() != [0_u8; 32]
        || create.poolInitializer != profile.initializer.address
        || create.liquidityMigrator != profile.liquidity_migrator.address
        || !create.liquidityMigratorData.is_empty()
        || create.integrator != BANKR_INTEGRATOR
        || create.salt == B256::ZERO
    {
        return Err(BankrQuoteError::CreateCalldata);
    }
    let init = abi::DopplerInitData::abi_decode(&create.poolInitializerData)
        .map_err(|_| BankrQuoteError::CreateCalldata)?;
    if init.abi_encode().as_slice() != create.poolInitializerData.as_ref()
        || u32::try_from(init.fee).ok() != Some(profile.standard_lp_fee_ppm)
        || i32::try_from(init.tickSpacing).ok() != Some(profile.tick_spacing)
        || i32::try_from(init.farTick).ok() != Some(887_000)
        || init.dopplerHook != profile.rehype_hook.address
        || !init.graduationDopplerHookCalldata.is_empty()
        || init.curves.len() != 2
        || init.beneficiaries.len() != 2
    {
        return Err(BankrQuoteError::CreateCalldata);
    }
    let expected_primary_share = WAD
        .checked_mul(U256::from(profile.primary_curve_share_bps))
        .ok_or(BankrQuoteError::ArithmeticOverflow)?
        / U256::from(BPS_DENOMINATOR);
    let expected_secondary_share = WAD
        .checked_mul(U256::from(profile.secondary_curve_share_bps))
        .ok_or(BankrQuoteError::ArithmeticOverflow)?
        / U256::from(BPS_DENOMINATOR);
    let curve = |curve: &abi::Curve, lower, upper, shares| {
        i32::try_from(curve.tickLower).ok() == Some(lower)
            && i32::try_from(curve.tickUpper).ok() == Some(upper)
            && curve.numPositions == 1
            && curve.shares == shares
    };
    let profile_version = if curve(&init.curves[0], -229_800, -119_800, expected_primary_share)
        && curve(&init.curves[1], -119_800, 887_200, expected_secondary_share)
    {
        BankrCreateProfileVersion::CurveTicksV1
    } else if curve(&init.curves[0], -229_600, -119_400, expected_primary_share)
        && curve(&init.curves[1], -119_400, 887_200, expected_secondary_share)
    {
        BankrCreateProfileVersion::CurveTicksV2
    } else if curve(&init.curves[0], -229_400, -119_400, expected_primary_share)
        && curve(&init.curves[1], -119_400, 887_200, expected_secondary_share)
    {
        BankrCreateProfileVersion::CurveTicksV3
    } else if curve(&init.curves[0], -229_400, -119_200, expected_primary_share)
        && curve(&init.curves[1], -119_200, 887_200, expected_secondary_share)
    {
        BankrCreateProfileVersion::CurveTicksV4
    } else if curve(&init.curves[0], -229_200, -119_200, expected_primary_share)
        && curve(&init.curves[1], -119_200, 887_200, expected_secondary_share)
    {
        BankrCreateProfileVersion::CurveTicksV5
    } else {
        return Err(BankrQuoteError::CreateCalldata);
    };
    let creator_share = WAD
        .checked_mul(U256::from(profile.creator_beneficiary_bps))
        .ok_or(BankrQuoteError::ArithmeticOverflow)?
        / U256::from(BPS_DENOMINATOR);
    let protocol_share = WAD
        .checked_mul(U256::from(profile.protocol_beneficiary_bps))
        .ok_or(BankrQuoteError::ArithmeticOverflow)?
        / U256::from(BPS_DENOMINATOR);
    if init.beneficiaries[0].beneficiary >= init.beneficiaries[1].beneficiary {
        return Err(BankrQuoteError::CreateCalldata);
    }
    let mut creator_beneficiary = None;
    let mut protocol_beneficiary_seen = false;
    let mut total_beneficiary_share = U256::ZERO;
    for beneficiary in &init.beneficiaries {
        let share = U256::from(beneficiary.shares);
        total_beneficiary_share = total_beneficiary_share
            .checked_add(share)
            .ok_or(BankrQuoteError::ArithmeticOverflow)?;
        if beneficiary.beneficiary == BANKR_PROTOCOL_BENEFICIARY {
            if protocol_beneficiary_seen || share != protocol_share {
                return Err(BankrQuoteError::CreateCalldata);
            }
            protocol_beneficiary_seen = true;
        } else if beneficiary.beneficiary == Address::ZERO
            || creator_beneficiary.is_some()
            || share != creator_share
        {
            return Err(BankrQuoteError::CreateCalldata);
        } else {
            creator_beneficiary = Some(beneficiary.beneficiary);
        }
    }
    let Some(creator_beneficiary) = creator_beneficiary else {
        return Err(BankrQuoteError::CreateCalldata);
    };
    if !protocol_beneficiary_seen || total_beneficiary_share != WAD {
        return Err(BankrQuoteError::CreateCalldata);
    }
    let mut token_call = Vec::with_capacity(4 + create.tokenFactoryData.len());
    token_call.extend_from_slice(&abi::tokenFactoryDataCall::SELECTOR);
    token_call.extend_from_slice(&create.tokenFactoryData);
    let token = abi::tokenFactoryDataCall::abi_decode(&token_call)
        .map_err(|_| BankrQuoteError::CreateCalldata)?;
    let uri = token.tokenURI.as_bytes();
    let unsold_supply = create
        .initialSupply
        .checked_sub(create.numTokensToSell)
        .ok_or(BankrQuoteError::ArithmeticOverflow)?;
    if token.abi_encode().get(4..) != Some(create.tokenFactoryData.as_ref())
        || token.name.is_empty()
        || token.name.len() > 64
        || token.symbol.is_empty()
        || token.symbol.len() > 32
        || token.schedules.len() != 1
        || token.schedules[0].cliff != 2_592_000
        || token.schedules[0].duration != 63_072_000
        || token.beneficiaries.as_slice() != [creator_beneficiary]
        || token.scheduleIds.as_slice() != [U256::ZERO]
        || token.amounts.as_slice() != [unsold_supply]
        || uri.len() != 66
        || !uri.starts_with(b"ipfs://bafkrei")
        || !uri[14..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || (b'2'..=b'7').contains(byte))
        || token.maxBalanceLimit != U256::ZERO
        || token.balanceLimitEnd != 0
        || token.controller != Address::ZERO
        || !token.excludedFromBalanceLimit.is_empty()
    {
        return Err(BankrQuoteError::CreateCalldata);
    }
    let rehype = abi::RehypeInitData::abi_decode(&init.onInitializationDopplerHookCalldata)
        .map_err(|_| BankrQuoteError::CreateCalldata)?;
    let fees = &rehype.feeDistributionInfo;
    if rehype.abi_encode().as_slice() != init.onInitializationDopplerHookCalldata.as_ref()
        || rehype.numeraire != profile.weth.address
        || rehype.buybackDst != create.integrator
        || u32::try_from(rehype.startFee).ok() != Some(profile.hook_start_fee_ppm)
        || u32::try_from(rehype.endFee).ok() != Some(profile.hook_end_fee_ppm)
        || u64::from(rehype.durationSeconds) != profile.hook_duration_seconds
        || rehype.startingTime != 0
        || rehype.feeRoutingMode != 0
        || fees.assetFeesToAssetBuybackWad != U256::ZERO
        || fees.assetFeesToNumeraireBuybackWad != WAD
        || fees.assetFeesToBeneficiaryWad != U256::ZERO
        || fees.assetFeesToLpWad != U256::ZERO
        || fees.numeraireFeesToAssetBuybackWad != U256::ZERO
        || fees.numeraireFeesToNumeraireBuybackWad != WAD
        || fees.numeraireFeesToBeneficiaryWad != U256::ZERO
        || fees.numeraireFeesToLpWad != U256::ZERO
    {
        return Err(BankrQuoteError::CreateCalldata);
    }
    Ok(DecodedCreate {
        init,
        rehype,
        profile_version,
    })
}

fn validate_receipt(
    logs: &[ReceiptLog],
    leader: Address,
    decoded: &DecodedCreate,
    block_timestamp: u64,
    profile: BankrDopplerExpectedProfile,
    require_user_operation: bool,
) -> Result<(LaunchEvidence, Vec<BankrDopplerPositionEvidence>), BankrQuoteError> {
    let mut airlock_create = None;
    let mut initializer_create = None;
    let mut initialize = None;
    let mut positions = Vec::new();
    let mut lock = None;
    let mut schedule = None;
    let mut user_operation = None;
    let mut pool_swap_ids = Vec::new();

    for log in logs {
        let Some(topic) = log.topics.first().copied() else {
            continue;
        };
        if log.address == profile.airlock.address && topic == airlock_events::Create::SIGNATURE_HASH
        {
            if airlock_create.is_some() {
                return Err(BankrQuoteError::LaunchIdentity);
            }
            let event = airlock_events::Create::decode_raw_log_validate(
                log.topics.iter().copied(),
                &log.data,
            )
            .map_err(|_| BankrQuoteError::LaunchIdentity)?;
            airlock_create = Some((event, log.log_index));
        } else if log.address == profile.initializer.address
            && topic == initializer_events::Create::SIGNATURE_HASH
        {
            if initializer_create.is_some() {
                return Err(BankrQuoteError::LaunchIdentity);
            }
            let event = initializer_events::Create::decode_raw_log_validate(
                log.topics.iter().copied(),
                &log.data,
            )
            .map_err(|_| BankrQuoteError::LaunchIdentity)?;
            initializer_create = Some((event, log.log_index));
        } else if log.address == profile.initializer.address && topic == abi::Lock::SIGNATURE_HASH {
            if lock.is_some() {
                return Err(BankrQuoteError::LaunchIdentity);
            }
            let event = abi::Lock::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                .map_err(|_| BankrQuoteError::LaunchIdentity)?;
            lock = Some((event, log.log_index));
        } else if log.address == profile.rehype_hook.address
            && topic == abi::FeeScheduleSet::SIGNATURE_HASH
        {
            if schedule.is_some() {
                return Err(BankrQuoteError::FeeSchedule);
            }
            let event =
                abi::FeeScheduleSet::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                    .map_err(|_| BankrQuoteError::FeeSchedule)?;
            schedule = Some((event, log.log_index));
        } else if log.address == profile.entry_point.address
            && topic == abi::UserOperationEvent::SIGNATURE_HASH
        {
            if user_operation.is_some() {
                return Err(BankrQuoteError::UserOperationEvidence);
            }
            let event = abi::UserOperationEvent::decode_raw_log_validate(
                log.topics.iter().copied(),
                &log.data,
            )
            .map_err(|_| BankrQuoteError::UserOperationEvidence)?;
            user_operation = Some((event, log.log_index));
        } else if log.address == profile.pool_manager.address {
            if topic == abi::Initialize::SIGNATURE_HASH {
                if initialize.is_some() {
                    return Err(BankrQuoteError::InitializeIdentity);
                }
                let event =
                    abi::Initialize::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                        .map_err(|_| BankrQuoteError::InitializeIdentity)?;
                initialize = Some((event, log.log_index));
            } else if topic == abi::ModifyLiquidity::SIGNATURE_HASH {
                let event = abi::ModifyLiquidity::decode_raw_log_validate(
                    log.topics.iter().copied(),
                    &log.data,
                )
                .map_err(|_| BankrQuoteError::LiquiditySequence)?;
                let delta = i128::try_from(event.liquidityDelta)
                    .map_err(|_| BankrQuoteError::LiquiditySequence)?;
                positions.push(BankrDopplerPositionEvidence {
                    pool_id: event.id,
                    sender: event.sender,
                    tick_lower: i32::try_from(event.tickLower)
                        .map_err(|_| BankrQuoteError::LiquiditySequence)?,
                    tick_upper: i32::try_from(event.tickUpper)
                        .map_err(|_| BankrQuoteError::LiquiditySequence)?,
                    liquidity: U256::from(
                        u128::try_from(delta).map_err(|_| BankrQuoteError::LiquiditySequence)?,
                    ),
                    salt: event.salt,
                    log_index: log.log_index,
                });
            } else if topic == abi::Swap::SIGNATURE_HASH {
                let event =
                    abi::Swap::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                        .map_err(|_| BankrQuoteError::EmbeddedSwapUnsupported)?;
                pool_swap_ids.push(event.id);
            }
        }
    }
    let (airlock, launch_log_index) = airlock_create.ok_or(BankrQuoteError::LaunchIdentity)?;
    let (initializer_create, initializer_create_log_index) =
        initializer_create.ok_or(BankrQuoteError::LaunchIdentity)?;
    let token = airlock.asset;
    if token == Address::ZERO
        || token == WETH
        || airlock.numeraire != WETH
        || airlock.initializer != profile.initializer.address
        || airlock.poolOrHook != token
        || initializer_create.poolOrHook != profile.pool_manager.address
        || initializer_create.asset != token
        || initializer_create.numeraire != WETH
    {
        return Err(BankrQuoteError::LaunchIdentity);
    }
    let key = V4PoolKey::canonical(
        WETH,
        token,
        DYNAMIC_FEE_FLAG,
        profile.tick_spacing,
        profile.initializer.address,
    )
    .map_err(|_| BankrQuoteError::InitializeIdentity)?;
    if pool_swap_ids.iter().any(|id| *id == key.pool_id()) {
        return Err(BankrQuoteError::EmbeddedSwapUnsupported);
    }
    let (initialize, initialize_log_index) =
        initialize.ok_or(BankrQuoteError::InitializeIdentity)?;
    let fee = u32::try_from(initialize.fee).map_err(|_| BankrQuoteError::InitializeIdentity)?;
    let tick_spacing =
        i32::try_from(initialize.tickSpacing).map_err(|_| BankrQuoteError::InitializeIdentity)?;
    let initialize_tick =
        i32::try_from(initialize.tick).map_err(|_| BankrQuoteError::InitializeIdentity)?;
    let expected_initialize_tick = match (decoded.profile_version, token < WETH) {
        (BankrCreateProfileVersion::CurveTicksV1, true)
        | (BankrCreateProfileVersion::CurveTicksV2, true) => -229_600,
        (BankrCreateProfileVersion::CurveTicksV3, true) => -229_400,
        (BankrCreateProfileVersion::CurveTicksV4, true) => -229_400,
        (BankrCreateProfileVersion::CurveTicksV5, true) => -229_200,
        (BankrCreateProfileVersion::CurveTicksV1, false) => 229_800,
        (BankrCreateProfileVersion::CurveTicksV2, false) => 229_600,
        (BankrCreateProfileVersion::CurveTicksV3, false) => 229_400,
        (BankrCreateProfileVersion::CurveTicksV4, false) => 229_400,
        (BankrCreateProfileVersion::CurveTicksV5, false) => 229_200,
    };
    let expected_sqrt_price_x96 = get_sqrt_ratio_at_tick(expected_initialize_tick)
        .map_err(|_| BankrQuoteError::InitializeIdentity)?;
    if initialize.id != key.pool_id()
        || initialize.currency0 != key.currency0
        || initialize.currency1 != key.currency1
        || fee != DYNAMIC_FEE_FLAG
        || tick_spacing != profile.tick_spacing
        || initialize.hooks != profile.initializer.address
        || initialize_tick != expected_initialize_tick
        || U256::from(initialize.sqrtPriceX96) != expected_sqrt_price_x96
    {
        return Err(BankrQuoteError::InitializeIdentity);
    }
    if positions.len() != 2
        || positions
            .iter()
            .any(|p| p.log_index <= initialize_log_index)
    {
        return Err(BankrQuoteError::LiquiditySequence);
    }
    let expected_ranges = match (decoded.profile_version, token < WETH) {
        (BankrCreateProfileVersion::CurveTicksV1, true)
        | (BankrCreateProfileVersion::CurveTicksV2, true) => [
            (-229_600, -119_400, B256::ZERO),
            (-119_400, 887_200, B256::with_last_byte(1)),
        ],
        (BankrCreateProfileVersion::CurveTicksV3, true) => [
            (-229_400, -119_400, B256::ZERO),
            (-119_400, 887_200, B256::with_last_byte(1)),
        ],
        (BankrCreateProfileVersion::CurveTicksV4, true) => [
            (-229_400, -119_200, B256::ZERO),
            (-119_200, 887_200, B256::with_last_byte(1)),
        ],
        (BankrCreateProfileVersion::CurveTicksV5, true) => [
            (-229_200, -119_200, B256::ZERO),
            (-119_200, 887_200, B256::with_last_byte(1)),
        ],
        (BankrCreateProfileVersion::CurveTicksV1, false) => [
            (119_800, 229_800, B256::ZERO),
            (-887_200, 119_800, B256::with_last_byte(1)),
        ],
        (BankrCreateProfileVersion::CurveTicksV2, false) => [
            (119_400, 229_600, B256::ZERO),
            (-887_200, 119_400, B256::with_last_byte(1)),
        ],
        (BankrCreateProfileVersion::CurveTicksV3, false) => [
            (119_400, 229_400, B256::ZERO),
            (-887_200, 119_400, B256::with_last_byte(1)),
        ],
        (BankrCreateProfileVersion::CurveTicksV4, false) => [
            (119_200, 229_400, B256::ZERO),
            (-887_200, 119_200, B256::with_last_byte(1)),
        ],
        (BankrCreateProfileVersion::CurveTicksV5, false) => [
            (119_200, 229_200, B256::ZERO),
            (-887_200, 119_200, B256::with_last_byte(1)),
        ],
    };
    let expected_reverse_liquidity = match (decoded.profile_version, token < WETH) {
        (BankrCreateProfileVersion::CurveTicksV4, true) => Some([
            U256::from_str_radix("badf8a38e438d69a45c2", 16)
                .expect("reviewed reverse V4 primary liquidity is valid"),
            U256::from_str_radix("1d082240a370451eb5ea2", 16)
                .expect("reviewed reverse V4 secondary liquidity is valid"),
        ]),
        (BankrCreateProfileVersion::CurveTicksV5, true) => Some([
            U256::from_str_radix("bcc248d856f01dd554c5", 16)
                .expect("reviewed reverse V5 primary liquidity is valid"),
            U256::from_str_radix("1d082240a370451eb5ea2", 16)
                .expect("reviewed reverse V5 secondary liquidity is valid"),
        ]),
        _ => None,
    };
    for (index, (position, expected)) in positions.iter().zip(expected_ranges).enumerate() {
        if position.pool_id != key.pool_id()
            || position.sender != profile.initializer.address
            || position.tick_lower != expected.0
            || position.tick_upper != expected.1
            || position.salt != expected.2
            || position.liquidity == U256::ZERO
            || expected_reverse_liquidity
                .is_some_and(|liquidity| position.liquidity != liquidity[index])
        {
            return Err(BankrQuoteError::LiquiditySequence);
        }
    }
    let last_liquidity_log_index = positions
        .last()
        .ok_or(BankrQuoteError::LiquiditySequence)?
        .log_index;

    let (lock, lock_log_index) = lock.ok_or(BankrQuoteError::LaunchIdentity)?;
    if lock.pool != token
        || lock.beneficiaries.len() != decoded.init.beneficiaries.len()
        || lock
            .beneficiaries
            .iter()
            .zip(&decoded.init.beneficiaries)
            .any(|(left, right)| {
                left.beneficiary != right.beneficiary || left.shares != right.shares
            })
    {
        return Err(BankrQuoteError::LaunchIdentity);
    }
    let (schedule, schedule_log_index) = schedule.ok_or(BankrQuoteError::FeeSchedule)?;
    if schedule.poolId != key.pool_id()
        || u64::from(schedule.startingTime) != block_timestamp
        || u32::try_from(schedule.startFee).ok() != Some(profile.hook_start_fee_ppm)
        || u32::try_from(schedule.endFee).ok() != Some(profile.hook_end_fee_ppm)
        || u64::from(schedule.durationSeconds) != profile.hook_duration_seconds
    {
        return Err(BankrQuoteError::FeeSchedule);
    }
    let user_operation_log_index = match (require_user_operation, user_operation) {
        (true, Some((event, log_index)))
            if event.sender == leader
                && event.paymaster == Address::ZERO
                && event.success
                && launch_log_index < log_index =>
        {
            Some(log_index)
        }
        (false, None) => None,
        _ => return Err(BankrQuoteError::UserOperationEvidence),
    };
    if !(initialize_log_index < last_liquidity_log_index
        && last_liquidity_log_index < initializer_create_log_index
        && initializer_create_log_index < lock_log_index
        && lock_log_index < schedule_log_index
        && schedule_log_index < launch_log_index)
    {
        return Err(BankrQuoteError::LiquiditySequence);
    }

    Ok((
        LaunchEvidence {
            token,
            pool_id: key.pool_id(),
            sqrt_price_x96: U256::from(initialize.sqrtPriceX96),
            initialize_tick,
            initialize_log_index,
            last_liquidity_log_index,
            launch_log_index,
            user_operation_log_index,
            schedule_start: u64::from(schedule.startingTime),
            buyback_destination: decoded.rehype.buybackDst,
        },
        positions,
    ))
}

pub fn bankr_hook_fee_ppm(
    start_time: u64,
    start_fee_ppm: u32,
    end_fee_ppm: u32,
    duration_seconds: u64,
    timestamp: u64,
) -> Result<u32, BankrQuoteError> {
    if start_fee_ppm < end_fee_ppm || duration_seconds == 0 {
        return Err(BankrQuoteError::FeeSchedule);
    }
    if timestamp <= start_time {
        return Ok(start_fee_ppm);
    }
    let elapsed = timestamp - start_time;
    if elapsed >= duration_seconds {
        return Ok(end_fee_ppm);
    }
    let range = u64::from(start_fee_ppm - end_fee_ppm);
    let delta = range
        .checked_mul(elapsed)
        .ok_or(BankrQuoteError::ArithmeticOverflow)?
        / duration_seconds;
    start_fee_ppm
        .checked_sub(u32::try_from(delta).map_err(|_| BankrQuoteError::ArithmeticOverflow)?)
        .ok_or(BankrQuoteError::ArithmeticOverflow)
}

fn output_hook_fee(amount: U256, fee: u32, denominator: u32) -> Result<U256, BankrQuoteError> {
    if denominator == 0 || fee > denominator {
        return Err(BankrQuoteError::FeeSchedule);
    }
    amount
        .checked_mul(U256::from(fee))
        .ok_or(BankrQuoteError::ArithmeticOverflow)
        .map(|value| value / U256::from(denominator))
}

fn apply_slippage(amount: U256, slippage_bps: u16) -> Result<U256, BankrQuoteError> {
    let retained = BPS_DENOMINATOR
        .checked_sub(slippage_bps)
        .ok_or(BankrQuoteError::UnsafePolicy)?;
    let minimum = amount
        .checked_mul(U256::from(retained))
        .ok_or(BankrQuoteError::ArithmeticOverflow)?
        / U256::from(BPS_DENOMINATOR);
    if minimum == U256::ZERO {
        return Err(BankrQuoteError::UnsafePolicy);
    }
    Ok(minimum)
}

fn validate_complete_quote(quote: &V3Quote, requested: U256) -> Result<(), BankrQuoteError> {
    if quote.amount_in_requested != requested
        || quote.amount_in_consumed != requested
        || quote.amount_out == U256::ZERO
    {
        return Err(BankrQuoteError::LiquiditySequence);
    }
    Ok(())
}

fn validate_envelope(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
    policy: BankrDopplerQuotePolicy,
    profile: BankrDopplerExpectedProfile,
    envelope: VerifiedBankrEnvelope,
) -> Result<(), BankrQuoteError> {
    let smart_account = match envelope {
        VerifiedBankrEnvelope::Erc7579 { smart_account }
        | VerifiedBankrEnvelope::DirectAirlock { smart_account } => smart_account,
    };
    let destination_matches = match envelope {
        VerifiedBankrEnvelope::Erc7579 { .. } => transaction.to == Some(ENTRY_POINT_V07),
        VerifiedBankrEnvelope::DirectAirlock { .. } => {
            transaction.to == Some(profile.airlock.address)
                && transaction.from == smart_account.account.address
        }
    };
    if !receipt.status
        || receipt.transaction_hash == B256::ZERO
        || receipt.block_hash == B256::ZERO
        || transaction.hash != receipt.transaction_hash
        || !destination_matches
        || transaction.value != U256::ZERO
        || transaction.l2_block_number != Some(receipt.l2_block_number)
        || transaction.transaction_index != Some(receipt.transaction_index)
        || block.l2_block_number != receipt.l2_block_number
        || block.hash != receipt.block_hash
        || receipt
            .l1_block_number
            .is_some_and(|l1| l1 != block.l1_block_number)
    {
        return Err(BankrQuoteError::InvalidEnvelope);
    }
    if policy.amount_in == U256::ZERO
        || policy.max_amount_in == U256::ZERO
        || policy.amount_in > policy.max_amount_in
        || policy.slippage_bps >= BPS_DENOMINATOR
    {
        return Err(BankrQuoteError::UnsafePolicy);
    }
    if receipt
        .logs
        .windows(2)
        .any(|pair| pair[0].log_index >= pair[1].log_index)
    {
        return Err(BankrQuoteError::InvalidEnvelope);
    }
    Ok(())
}

fn validate_verified_smart_account(
    smart_account: SmartAccountPin,
    profile: BankrDopplerExpectedProfile,
) -> Result<(), BankrQuoteError> {
    let expected = profile.smart_account;
    if smart_account.account.address == Address::ZERO
        || smart_account.account.runtime_code_hash != expected.account.runtime_code_hash
        || smart_account.factory.is_some()
        || smart_account.execution_profile != AccountExecutionProfile::Erc7579SingleCall
        || smart_account.delegation_implementation != expected.delegation_implementation
    {
        return Err(BankrQuoteError::SmartAccountIdentity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::Mutex;

    use alloy_consensus::{Transaction, TxEnvelope, transaction::SignerRecoverable};
    use alloy_eips::eip2718::Decodable2718;
    use alloy_primitives::Bytes;

    use super::*;

    #[derive(Deserialize)]
    struct LiveFixture {
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
    }

    #[derive(Clone, Deserialize)]
    struct ReceiptBlockIdentityFixture {
        l2_block_number: u64,
        block_hash: B256,
        leader: Address,
        account_designator: alloy_primitives::Bytes,
        account_designator_bytes: usize,
        account_designator_hash: B256,
        delegation_implementation: Address,
        delegation_runtime_bytes: usize,
        delegation_runtime_hash: B256,
    }

    #[derive(Clone, Deserialize)]
    struct ProductionLiveFixture {
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
        receipt_block_identity: ReceiptBlockIdentityFixture,
    }

    #[derive(Deserialize)]
    struct FinalTupleV4Fixture {
        reconciliation_evidence_sha256: BTreeMap<String, String>,
        expected_account_designator: Bytes,
        expected_account_designator_hash: B256,
        delegation_implementation: Address,
        delegation_runtime_hash: B256,
        delegation_runtime: Bytes,
        launches: Vec<FinalTupleV4Launch>,
    }

    #[derive(Deserialize)]
    struct FinalTupleV4Launch {
        window: String,
        envelope: BankrEnvelopeKind,
        raw_transaction: Bytes,
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
        receipt_block_identity: ReceiptBlockIdentityFixture,
    }

    #[derive(Deserialize)]
    struct ReverseV4Fixture {
        launches: Vec<ReverseV4Launch>,
    }

    #[derive(Deserialize)]
    struct ReverseV4Launch {
        window: String,
        envelope: BankrEnvelopeKind,
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
        receipt_block_identity: ReceiptBlockIdentityFixture,
    }

    #[derive(Deserialize)]
    struct FreshCurveTicksV5Fixture {
        expected_account_designator: Bytes,
        expected_account_designator_hash: B256,
        delegation_implementation: Address,
        delegation_runtime_hash: B256,
        delegation_runtime: Bytes,
        launches: Vec<FreshCurveTicksV5Launch>,
    }

    #[derive(Deserialize)]
    struct FreshCurveTicksV5Launch {
        window: String,
        envelope: BankrEnvelopeKind,
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
        receipt_block_identity: ReceiptBlockIdentityFixture,
    }

    enum MockReceiptBlockCall {
        Code {
            address: Address,
            l2_block_number: u64,
            result: Result<alloy_primitives::Bytes, String>,
        },
        RuntimeObservation {
            address: Address,
            l2_block_number: u64,
            result: Result<ReceiptBlockRuntimeObservation, String>,
        },
        Block {
            l2_block_number: u64,
            result: Result<RobinhoodBlock, String>,
        },
    }

    struct MockReceiptBlockRpc {
        calls: Mutex<VecDeque<MockReceiptBlockCall>>,
    }

    impl MockReceiptBlockRpc {
        fn new(calls: Vec<MockReceiptBlockCall>) -> Self {
            Self {
                calls: Mutex::new(calls.into()),
            }
        }

        fn next(&self) -> MockReceiptBlockCall {
            self.calls
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected receipt-block RPC call")
        }

        fn assert_exhausted(&self) {
            assert!(
                self.calls.lock().unwrap().is_empty(),
                "receipt-block admission did not issue every expected RPC call"
            );
        }
    }

    impl BankrReceiptBlockRpc for MockReceiptBlockRpc {
        async fn code_at_l2_block(
            &self,
            address: Address,
            l2_block_number: u64,
        ) -> Result<alloy_primitives::Bytes, String> {
            match self.next() {
                MockReceiptBlockCall::Code {
                    address: expected_address,
                    l2_block_number: expected_block,
                    result,
                } => {
                    assert_eq!(address, expected_address);
                    assert_eq!(l2_block_number, expected_block);
                    result
                }
                _ => panic!("expected a receipt-block code request"),
            }
        }

        async fn runtime_observation_at_l2_block(
            &self,
            address: Address,
            l2_block_number: u64,
        ) -> Result<ReceiptBlockRuntimeObservation, String> {
            match self.next() {
                MockReceiptBlockCall::RuntimeObservation {
                    address: expected_address,
                    l2_block_number: expected_block,
                    result,
                } => {
                    assert_eq!(address, expected_address);
                    assert_eq!(l2_block_number, expected_block);
                    result
                }
                _ => panic!("expected a receipt-block runtime observation request"),
            }
        }

        async fn block_by_number(&self, l2_block_number: u64) -> Result<RobinhoodBlock, String> {
            match self.next() {
                MockReceiptBlockCall::Block {
                    l2_block_number: expected_block,
                    result,
                } => {
                    assert_eq!(l2_block_number, expected_block);
                    result
                }
                _ => panic!("expected a stable-block reread"),
            }
        }
    }

    fn live_fixture() -> LiveFixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-live-proof.json"
        ))
        .unwrap()
    }

    fn production_fixture(contents: &str) -> ProductionLiveFixture {
        serde_json::from_str(contents).unwrap()
    }

    fn final_tuple_v4_fixture() -> FinalTupleV4Fixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v4-finaltuple-window-abc-live-proofs.json"
        ))
        .unwrap()
    }

    fn reverse_v4_fixture() -> ReverseV4Fixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v4-reverse-live-proofs.json"
        ))
        .unwrap()
    }

    fn fresh_curve_ticks_v5_fixture() -> FreshCurveTicksV5Fixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v5-fresh-six-live-proofs.json"
        ))
        .unwrap()
    }

    fn reverse_curve_ticks_v5_fixture() -> FreshCurveTicksV5Fixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v5-reverse-three-live-proofs.json"
        ))
        .unwrap()
    }

    #[derive(Debug, Deserialize)]
    struct CurveTicksV3Proof {
        tx_hash: B256,
        token: Address,
        pool_id: B256,
        transaction_type: String,
        outer_bundler: Address,
        outer_input: Bytes,
        entry_point: Address,
        handle_ops_selector: String,
        user_operation_sender: Address,
        account_call_selector: String,
        mode: B256,
        target: Address,
        value: String,
        create_calldata: Bytes,
        account_designator: Bytes,
        account_designator_hash: B256,
        delegation_implementation: Address,
        delegation_runtime_hash: B256,
        modify_liquidity_logs: Vec<CurveTicksV3LiquidityLog>,
    }

    #[derive(Debug, Deserialize)]
    struct CurveTicksV3LiquidityLog {
        pool_id: B256,
        sender: Address,
        data: Bytes,
    }

    #[derive(Debug, Deserialize)]
    struct LongNameLiveProof {
        tx_hash: B256,
        chain_id: u64,
        block_number: String,
        block_hash: B256,
        transaction_index: String,
        transaction_type: String,
        receipt_status: String,
        outer_bundler: Address,
        entry_point: Address,
        handle_ops_selector: String,
        outer_input: Bytes,
        user_operation_sender: Address,
        account_call_selector: String,
        mode: B256,
        target: Address,
        value: String,
        create_calldata: Bytes,
        account_designator: Bytes,
        account_designator_hash: B256,
        delegation_implementation: Address,
        delegation_runtime_hash: B256,
        token: Address,
        pool_id: B256,
        airlock_create_emitter: Address,
        airlock_create_topic: B256,
        token_name: String,
        token_name_bytes: usize,
        token_factory_data_bytes: usize,
        create_profile_version: String,
    }

    #[derive(Debug, Deserialize)]
    struct Clean6BeneficiaryOrderProof {
        chain_id: u64,
        tx_hash: B256,
        block_number: String,
        block_hash: B256,
        transaction_index: String,
        transaction_type: String,
        receipt_status: String,
        outer_bundler: Address,
        entry_point: Address,
        outer_input: Bytes,
        user_operation_sender: Address,
        account_designator: Bytes,
        delegation_implementation: Address,
        token: Address,
        pool_id: B256,
        creator_beneficiary: Address,
        protocol_beneficiary: Address,
        beneficiary_order: Vec<String>,
        create_profile_version: String,
    }

    fn curve_ticks_v3_proofs() -> Vec<CurveTicksV3Proof> {
        serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v3-quiet1-proof.json"
        ))
        .unwrap()
    }

    fn long_name_live_proof() -> LongNameLiveProof {
        serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v3-long-name-live-proof.json"
        ))
        .unwrap()
    }

    fn clean6_beneficiary_order_proof() -> Clean6BeneficiaryOrderProof {
        serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-v3-clean6-beneficiary-order-proof.json"
        ))
        .unwrap()
    }

    fn exact_create_calldata(transaction: &RobinhoodTransaction) -> Bytes {
        let profile = BankrDopplerExpectedProfile::production();
        if transaction.to == Some(profile.airlock.address) {
            return transaction.input.clone();
        }
        discover_entry_point_v07_erc7579(
            EntryPointCall {
                chain_id: CHAIN_ID,
                destination: profile.entry_point,
                outer_bundler: transaction.from,
                calldata: &transaction.input,
            },
            profile.entry_point,
            ContractPin {
                address: profile.airlock.address,
                runtime_code_hash: profile.airlock.runtime_code_hash,
            },
        )
        .unwrap()
        .calldata
    }

    fn canonical_receipt_identity(receipt: &NoxaReceipt) -> (Address, B256) {
        let profile = BankrDopplerExpectedProfile::production();
        let token = receipt
            .logs
            .iter()
            .find(|log| {
                log.address == profile.airlock.address
                    && log.topics.first() == Some(&airlock_events::Create::SIGNATURE_HASH)
            })
            .and_then(|log| {
                airlock_events::Create::decode_raw_log_validate(
                    log.topics.iter().copied(),
                    &log.data,
                )
                .ok()
            })
            .unwrap()
            .asset;
        let pool_id = receipt
            .logs
            .iter()
            .find(|log| {
                log.address == profile.pool_manager.address
                    && log.topics.first() == Some(&abi::Initialize::SIGNATURE_HASH)
            })
            .and_then(|log| {
                abi::Initialize::decode_raw_log_validate(log.topics.iter().copied(), &log.data).ok()
            })
            .unwrap()
            .id;
        (token, pool_id)
    }

    fn kernel_runtime() -> Vec<u8> {
        let encoded = include_str!("../tests/fixtures/bankr-kernel-runtime.hex").trim();
        hex::decode(encoded.strip_prefix("0x").unwrap()).unwrap()
    }

    fn successful_admission_calls(
        fixture: &ProductionLiveFixture,
        require_entry_point: bool,
    ) -> Vec<MockReceiptBlockCall> {
        successful_admission_calls_for(
            &fixture.receipt,
            &fixture.block,
            &fixture.receipt_block_identity,
            require_entry_point,
        )
    }

    fn successful_admission_calls_for(
        receipt: &NoxaReceipt,
        block: &RobinhoodBlock,
        identity: &ReceiptBlockIdentityFixture,
        require_entry_point: bool,
    ) -> Vec<MockReceiptBlockCall> {
        let profile = BankrDopplerExpectedProfile::production();
        let l2_block_number = receipt.l2_block_number;
        let mut calls = vec![
            MockReceiptBlockCall::Code {
                address: identity.leader,
                l2_block_number,
                result: Ok(identity.account_designator.clone()),
            },
            MockReceiptBlockCall::Code {
                address: BANKR_KERNEL_IMPLEMENTATION,
                l2_block_number,
                result: Ok(kernel_runtime().into()),
            },
        ];
        calls.extend(
            bankr_receipt_block_dependency_pins(profile, require_entry_point)
                .into_iter()
                .map(|pin| MockReceiptBlockCall::RuntimeObservation {
                    address: pin.address,
                    l2_block_number,
                    result: Ok(ReceiptBlockRuntimeObservation {
                        address: pin.address,
                        runtime_code_hash: pin.runtime_code_hash,
                        code_bytes: 1,
                    }),
                }),
        );
        calls.push(MockReceiptBlockCall::Block {
            l2_block_number,
            result: Ok(block.clone()),
        });
        calls
    }

    fn policy() -> BankrDopplerQuotePolicy {
        BankrDopplerQuotePolicy {
            amount_in: U256::from(1_000_000_000_000_000_u64),
            max_amount_in: U256::from(10_000_000_000_000_000_u64),
            slippage_bps: 100,
        }
    }

    fn proof_create_calldata(fixture: &LiveFixture) -> alloy_primitives::Bytes {
        let profile = BankrDopplerExpectedProfile::production();
        let accounts = [profile.smart_account];
        let targets = [ContractPin {
            address: profile.airlock.address,
            runtime_code_hash: profile.airlock.runtime_code_hash,
        }];
        decode_entry_point_v07(
            EntryPointCall {
                chain_id: CHAIN_ID,
                destination: profile.entry_point,
                outer_bundler: fixture.transaction.from,
                calldata: &fixture.transaction.input,
            },
            SmartAccountPins {
                entry_point: profile.entry_point,
                accounts: &accounts,
                allowed_targets: &targets,
            },
        )
        .unwrap()
        .calldata
    }

    fn decode_token_factory_data(call: &abi::createCall) -> abi::tokenFactoryDataCall {
        let mut calldata = Vec::with_capacity(4 + call.createData.tokenFactoryData.len());
        calldata.extend_from_slice(&abi::tokenFactoryDataCall::SELECTOR);
        calldata.extend_from_slice(&call.createData.tokenFactoryData);
        abi::tokenFactoryDataCall::abi_decode(&calldata).unwrap()
    }

    fn replace_token_factory_data(call: &mut abi::createCall, token: &abi::tokenFactoryDataCall) {
        call.createData.tokenFactoryData = token.abi_encode()[4..].to_vec().into();
    }

    fn replace_initializer_data(call: &mut abi::createCall, init: &abi::DopplerInitData) {
        call.createData.poolInitializerData = init.abi_encode().into();
    }

    fn decode_initializer_data(call: &abi::createCall) -> abi::DopplerInitData {
        abi::DopplerInitData::abi_decode(&call.createData.poolInitializerData).unwrap()
    }

    fn decode_rehype_data(init: &abi::DopplerInitData) -> abi::RehypeInitData {
        abi::RehypeInitData::abi_decode(&init.onInitializationDopplerHookCalldata).unwrap()
    }

    fn replace_rehype_data(init: &mut abi::DopplerInitData, rehype: &abi::RehypeInitData) {
        init.onInitializationDopplerHookCalldata = rehype.abi_encode().into();
    }

    fn hex_u256(value: &str) -> U256 {
        U256::from_str_radix(value, 16).unwrap()
    }

    fn receipt_block_observations(pins: &[ContractPin]) -> Vec<ReceiptBlockRuntimeObservation> {
        pins.iter()
            .map(|pin| ReceiptBlockRuntimeObservation {
                address: pin.address,
                runtime_code_hash: pin.runtime_code_hash,
                code_bytes: 1,
            })
            .collect()
    }

    #[tokio::test]
    async fn public_receipt_block_admission_accepts_exact_direct_and_erc7579_proofs() {
        for (contents, require_entry_point, expected_envelope) in [
            (
                include_str!("../tests/fixtures/bankr-doppler-v2-direct-live-proof.json"),
                false,
                BankrEnvelopeKind::DirectAirlock,
            ),
            (
                include_str!("../tests/fixtures/bankr-doppler-v2-erc7579-live-proof.json"),
                true,
                BankrEnvelopeKind::Erc7579,
            ),
        ] {
            let fixture = production_fixture(contents);
            let rpc =
                MockReceiptBlockRpc::new(successful_admission_calls(&fixture, require_entry_point));
            let quote = quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
                &rpc,
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                BankrDopplerExpectedProfile::production(),
                policy(),
            )
            .await
            .unwrap();
            rpc.assert_exhausted();
            assert_eq!(quote.market.envelope, expected_envelope);
            assert_eq!(quote.market.leader, fixture.receipt_block_identity.leader);
            assert_eq!(quote.state_version.block_hash, fixture.block.hash);
            assert_eq!(
                quote.state_version.l2_block_number,
                fixture.receipt.l2_block_number
            );
        }
    }

    #[tokio::test]
    async fn public_receipt_block_admission_propagates_code_errors_and_rejects_empty_code() {
        let contents = include_str!("../tests/fixtures/bankr-doppler-v2-direct-live-proof.json");

        for (call_index, message) in [(0_usize, "leader code unavailable"), (1, "kernel timeout")] {
            let fixture = production_fixture(contents);
            let mut calls = successful_admission_calls(&fixture, false);
            let address = if call_index == 0 {
                fixture.receipt_block_identity.leader
            } else {
                BANKR_KERNEL_IMPLEMENTATION
            };
            calls[call_index] = MockReceiptBlockCall::Code {
                address,
                l2_block_number: fixture.receipt.l2_block_number,
                result: Err(message.into()),
            };
            let error = quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
                &MockReceiptBlockRpc::new(calls),
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                BankrDopplerExpectedProfile::production(),
                policy(),
            )
            .await
            .unwrap_err();
            assert!(matches!(
                error,
                BankrQuoteError::ReceiptBlockIdentity(ref detail) if detail == message
            ));
        }

        for call_index in [0_usize, 1] {
            let fixture = production_fixture(contents);
            let mut calls = successful_admission_calls(&fixture, false);
            let address = if call_index == 0 {
                fixture.receipt_block_identity.leader
            } else {
                BANKR_KERNEL_IMPLEMENTATION
            };
            calls[call_index] = MockReceiptBlockCall::Code {
                address,
                l2_block_number: fixture.receipt.l2_block_number,
                result: Ok(alloy_primitives::Bytes::new()),
            };
            assert!(matches!(
                quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
                    &MockReceiptBlockRpc::new(calls),
                    &fixture.transaction,
                    &fixture.receipt,
                    &fixture.block,
                    BankrDopplerExpectedProfile::production(),
                    policy(),
                )
                .await,
                Err(BankrQuoteError::ReceiptBlockIdentity(_))
            ));
        }

        let fixture = production_fixture(contents);
        let mut calls = successful_admission_calls(&fixture, false);
        let dependency = BankrDopplerExpectedProfile::production().airlock.address;
        calls[2] = MockReceiptBlockCall::RuntimeObservation {
            address: dependency,
            l2_block_number: fixture.receipt.l2_block_number,
            result: Err("dependency code unavailable".into()),
        };
        assert!(matches!(
            quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
                &MockReceiptBlockRpc::new(calls),
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                BankrDopplerExpectedProfile::production(),
                policy(),
            )
            .await,
            Err(BankrQuoteError::ReceiptBlockIdentity(ref detail))
                if detail == "dependency code unavailable"
        ));

        let fixture = production_fixture(contents);
        let mut calls = successful_admission_calls(&fixture, false);
        calls[2] = MockReceiptBlockCall::RuntimeObservation {
            address: dependency,
            l2_block_number: fixture.receipt.l2_block_number,
            result: Ok(ReceiptBlockRuntimeObservation {
                address: dependency,
                runtime_code_hash: BankrDopplerExpectedProfile::production()
                    .airlock
                    .runtime_code_hash,
                code_bytes: 0,
            }),
        };
        assert!(matches!(
            quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
                &MockReceiptBlockRpc::new(calls),
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                BankrDopplerExpectedProfile::production(),
                policy(),
            )
            .await,
            Err(BankrQuoteError::ReceiptBlockIdentity(_))
        ));
    }

    #[tokio::test]
    async fn public_receipt_block_admission_requires_stable_exact_block_reread() {
        let contents = include_str!("../tests/fixtures/bankr-doppler-v2-erc7579-live-proof.json");

        let fixture = production_fixture(contents);
        let mut calls = successful_admission_calls(&fixture, true);
        let last = calls.len() - 1;
        calls[last] = MockReceiptBlockCall::Block {
            l2_block_number: fixture.receipt.l2_block_number,
            result: Err("block reread unavailable".into()),
        };
        assert!(matches!(
            quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
                &MockReceiptBlockRpc::new(calls),
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                BankrDopplerExpectedProfile::production(),
                policy(),
            )
            .await,
            Err(BankrQuoteError::ReceiptBlockIdentity(ref detail))
                if detail == "block reread unavailable"
        ));

        let fixture = production_fixture(contents);
        let mut calls = successful_admission_calls(&fixture, true);
        let last = calls.len() - 1;
        let mut reorged = fixture.block.clone();
        reorged.hash = B256::with_last_byte(0xee);
        calls[last] = MockReceiptBlockCall::Block {
            l2_block_number: fixture.receipt.l2_block_number,
            result: Ok(reorged),
        };
        assert!(matches!(
            quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
                &MockReceiptBlockRpc::new(calls),
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                BankrDopplerExpectedProfile::production(),
                policy(),
            )
            .await,
            Err(BankrQuoteError::ReceiptBlockIdentity(ref detail))
                if detail == "receipt block changed during identity verification"
        ));
    }

    #[test]
    fn receipt_block_dependency_proof_covers_every_quote_critical_runtime() {
        let profile = BankrDopplerExpectedProfile::production();
        let direct = bankr_receipt_block_dependency_pins(profile, false);
        assert_eq!(direct.len(), 9);
        assert_eq!(direct[0].address, profile.airlock.address);
        assert_eq!(direct[1].address, profile.pool_manager.address);
        assert_eq!(direct[2].address, profile.initializer.address);
        assert_eq!(direct[3].address, profile.rehype_hook.address);
        assert_eq!(direct[4].address, profile.token_factory.address);
        assert_eq!(direct[5].address, profile.token_implementation.address);
        assert_eq!(direct[6].address, profile.governance_factory.address);
        assert_eq!(direct[7].address, profile.liquidity_migrator.address);
        assert_eq!(direct[8].address, profile.weth.address);
        validate_bankr_receipt_block_dependencies(&direct, &receipt_block_observations(&direct))
            .unwrap();

        let erc7579 = bankr_receipt_block_dependency_pins(profile, true);
        assert_eq!(erc7579.len(), 10);
        assert_eq!(erc7579.last().unwrap().address, profile.entry_point.address);
        validate_bankr_receipt_block_dependencies(&erc7579, &receipt_block_observations(&erc7579))
            .unwrap();
    }

    #[test]
    fn incomplete_reordered_empty_or_drifted_receipt_block_pins_fail_closed() {
        let expected =
            bankr_receipt_block_dependency_pins(BankrDopplerExpectedProfile::production(), true);
        let exact = receipt_block_observations(&expected);

        let mut missing = exact.clone();
        missing.pop();
        assert!(validate_bankr_receipt_block_dependencies(&expected, &missing).is_err());

        let mut reordered = exact.clone();
        reordered.swap(0, 1);
        assert!(validate_bankr_receipt_block_dependencies(&expected, &reordered).is_err());

        let mut empty = exact.clone();
        empty[0].code_bytes = 0;
        assert!(validate_bankr_receipt_block_dependencies(&expected, &empty).is_err());

        let mut wrong_address = exact.clone();
        wrong_address[0].address = Address::with_last_byte(0xee);
        assert!(validate_bankr_receipt_block_dependencies(&expected, &wrong_address).is_err());

        let mut extra = exact.clone();
        extra.push(exact[0]);
        assert!(validate_bankr_receipt_block_dependencies(&expected, &extra).is_err());

        for index in 0..exact.len() {
            let mut drifted = exact.clone();
            drifted[index].runtime_code_hash = B256::with_last_byte((index + 1) as u8);
            assert!(
                validate_bankr_receipt_block_dependencies(&expected, &drifted).is_err(),
                "dependency index {index} accepted a drifted runtime"
            );
        }
    }

    #[test]
    fn reviewed_selectors_and_topics_match_live_proof() {
        assert_eq!(BANKR_CREATE_SELECTOR, [0x88, 0x2d, 0xb7, 0x07]);
        assert_eq!(
            airlock_events::Create::SIGNATURE_HASH,
            alloy_primitives::b256!(
                "68ff1cfcdcf76864161555fc0de1878d8f83ec6949bf351df74d8a4a1a2679ab"
            )
        );
        assert_eq!(
            initializer_events::Create::SIGNATURE_HASH,
            alloy_primitives::b256!(
                "b224da6575b2c2ffd42454faedb236f7dbe5f92a0c96bb99c0273dbe98464c7e"
            )
        );
        assert_eq!(
            abi::FeeScheduleSet::SIGNATURE_HASH,
            alloy_primitives::b256!(
                "cea1bdc74004c2beebf7a8d2d531c3950ca35e8326a55bdc553df9d1b593d7b3"
            )
        );
        BankrDopplerExpectedProfile::production()
            .validate()
            .unwrap();
    }

    #[test]
    fn exact_current_curve_profile_is_distinct_from_reviewed_v1_proof() {
        let fixture = live_fixture();
        let calldata = proof_create_calldata(&fixture);
        let mut call = abi::createCall::abi_decode(&calldata).unwrap();
        let legacy =
            validate_create_calldata(&call, BankrDopplerExpectedProfile::production()).unwrap();
        assert_eq!(
            legacy.profile_version,
            BankrCreateProfileVersion::CurveTicksV1
        );

        let mut init =
            abi::DopplerInitData::abi_decode(&call.createData.poolInitializerData).unwrap();
        init.curves[0].tickLower = (-229_600_i32).try_into().unwrap();
        init.curves[0].tickUpper = (-119_400_i32).try_into().unwrap();
        init.curves[1].tickLower = (-119_400_i32).try_into().unwrap();
        call.createData.poolInitializerData = init.abi_encode().into();
        let current =
            validate_create_calldata(&call, BankrDopplerExpectedProfile::production()).unwrap();
        assert_eq!(
            current.profile_version,
            BankrCreateProfileVersion::CurveTicksV2
        );
        assert!(validate_bankr_create_calldata_for_observation(
            &call.abi_encode()
        ));

        init.curves[1].tickLower = (-119_200_i32).try_into().unwrap();
        call.createData.poolInitializerData = init.abi_encode().into();
        assert!(!validate_bankr_create_calldata_for_observation(
            &call.abi_encode()
        ));
    }

    #[test]
    fn quiet_window_curve_ticks_v3_transactions_and_receipts_are_exact_proofs() {
        let profile = BankrDopplerExpectedProfile::production();
        let proofs = curve_ticks_v3_proofs();
        assert_eq!(proofs.len(), 2);
        assert_eq!(
            proofs
                .iter()
                .map(|proof| proof.transaction_type.as_str())
                .collect::<Vec<_>>(),
            ["0x2", "0x4"]
        );
        let kernel = kernel_runtime();
        let expected_position_ranges = [
            [(-229_400, -119_400), (-119_400, 887_200)],
            [(119_400, 229_400), (-887_200, 119_400)],
        ];

        for (proof, expected_ranges) in proofs.iter().zip(expected_position_ranges) {
            assert_ne!(proof.tx_hash, B256::ZERO);
            assert_eq!(proof.entry_point, profile.entry_point.address);
            assert_eq!(proof.handle_ops_selector, "0x765e827f");
            assert_ne!(proof.user_operation_sender, Address::ZERO);
            assert_eq!(proof.account_call_selector, "0xe9ae5c53");
            assert_eq!(proof.mode, B256::ZERO);
            assert_eq!(proof.target, profile.airlock.address);
            assert_eq!(proof.value, "0x0");
            assert_eq!(
                keccak256(&proof.account_designator),
                proof.account_designator_hash
            );
            assert_eq!(
                proof.account_designator_hash,
                profile.smart_account.account.runtime_code_hash
            );
            assert_eq!(
                proof.delegation_implementation,
                profile
                    .smart_account
                    .delegation_implementation
                    .unwrap()
                    .address
            );
            let mut expected_designator = vec![0xef, 0x01, 0x00];
            expected_designator.extend_from_slice(proof.delegation_implementation.as_slice());
            assert_eq!(proof.account_designator.as_ref(), expected_designator);
            assert_eq!(keccak256(&kernel), proof.delegation_runtime_hash);
            assert_eq!(
                Some(proof.delegation_runtime_hash),
                profile
                    .smart_account
                    .delegation_implementation
                    .map(|pin| pin.runtime_code_hash)
            );

            let discovered = crate::smart_account::discover_entry_point_v07_erc7579(
                crate::smart_account::EntryPointCall {
                    chain_id: CHAIN_ID,
                    destination: profile.entry_point,
                    outer_bundler: proof.outer_bundler,
                    calldata: &proof.outer_input,
                },
                profile.entry_point,
                crate::smart_account::ContractPin {
                    address: profile.airlock.address,
                    runtime_code_hash: profile.airlock.runtime_code_hash,
                },
            )
            .unwrap();
            assert_eq!(discovered.leader, proof.user_operation_sender);
            assert_eq!(discovered.outer_bundler, proof.outer_bundler);
            assert_eq!(discovered.target, profile.airlock.address);
            assert_eq!(discovered.value, U256::ZERO);
            assert_eq!(discovered.calldata, proof.create_calldata);

            let call = abi::createCall::abi_decode(&proof.create_calldata).unwrap();
            assert_eq!(call.abi_encode().as_slice(), proof.create_calldata.as_ref());
            let decoded = validate_create_calldata(&call, profile).unwrap();
            assert_eq!(
                decoded.profile_version,
                BankrCreateProfileVersion::CurveTicksV3
            );
            assert!(validate_bankr_create_calldata_for_observation(
                &proof.create_calldata
            ));
            let prediction =
                predict_bankr_create_identity(&proof.create_calldata, profile).unwrap();
            assert_eq!(
                prediction.create_profile_version,
                BankrCreateProfileVersion::CurveTicksV3
            );
            assert_eq!(prediction.token, proof.token);
            assert_eq!(prediction.pool_id, proof.pool_id);

            let observed_ranges = proof
                .modify_liquidity_logs
                .iter()
                .map(|log| {
                    let event = abi::ModifyLiquidity::decode_raw_log_validate(
                        [
                            abi::ModifyLiquidity::SIGNATURE_HASH,
                            log.pool_id,
                            B256::left_padding_from(log.sender.as_slice()),
                        ],
                        &log.data,
                    )
                    .unwrap();
                    (
                        i32::try_from(event.tickLower).unwrap(),
                        i32::try_from(event.tickUpper).unwrap(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(observed_ranges, expected_ranges);
        }
    }

    #[test]
    fn clean6_protocol_first_beneficiaries_are_an_exact_erc7579_proof() {
        let proof = clean6_beneficiary_order_proof();
        let profile = BankrDopplerExpectedProfile::production();
        assert_eq!(proof.chain_id, CHAIN_ID);
        assert_eq!(
            proof.tx_hash,
            alloy_primitives::b256!(
                "c85b51ecb810158b02511586552295fc26e2720764a9b4a4a9a9cda774efdc20"
            )
        );
        assert_eq!(proof.block_number, "0xb47d15");
        assert_eq!(
            proof.block_hash,
            alloy_primitives::b256!(
                "acf9c1d3989f0dc99a2a2f3d232a520764ce6e1d0fe44466faade61d80587fc3"
            )
        );
        assert_eq!(proof.transaction_index, "0x5");
        assert_eq!(proof.transaction_type, "0x2");
        assert_eq!(proof.receipt_status, "0x1");
        assert_eq!(proof.entry_point, profile.entry_point.address);
        assert_eq!(
            proof.outer_input.get(..4),
            Some([0x76, 0x5e, 0x82, 0x7f].as_slice())
        );
        assert_eq!(proof.delegation_implementation, BANKR_KERNEL_IMPLEMENTATION);
        assert_eq!(keccak256(kernel_runtime()), BANKR_KERNEL_RUNTIME_HASH);
        let mut expected_designator = vec![0xef, 0x01, 0x00];
        expected_designator.extend_from_slice(proof.delegation_implementation.as_slice());
        assert_eq!(proof.account_designator.as_ref(), expected_designator);
        assert_eq!(
            keccak256(&proof.account_designator),
            profile.smart_account.account.runtime_code_hash
        );
        assert_eq!(
            profile
                .smart_account
                .delegation_implementation
                .expect("production ERC-7579 profile pins its delegation implementation")
                .address,
            proof.delegation_implementation
        );

        let discovered = discover_entry_point_v07_erc7579(
            EntryPointCall {
                chain_id: proof.chain_id,
                destination: profile.entry_point,
                outer_bundler: proof.outer_bundler,
                calldata: &proof.outer_input,
            },
            profile.entry_point,
            ContractPin {
                address: profile.airlock.address,
                runtime_code_hash: profile.airlock.runtime_code_hash,
            },
        )
        .unwrap();
        assert_eq!(discovered.leader, proof.user_operation_sender);
        assert_eq!(discovered.outer_bundler, proof.outer_bundler);
        assert_eq!(discovered.target, profile.airlock.address);
        assert_eq!(discovered.value, U256::ZERO);

        let call = abi::createCall::abi_decode(&discovered.calldata).unwrap();
        assert_eq!(call.abi_encode().as_slice(), discovered.calldata.as_ref());
        let init = decode_initializer_data(&call);
        let token = decode_token_factory_data(&call);
        let creator_share = WAD * U256::from(9_500_u16) / U256::from(10_000_u16);
        let protocol_share = WAD * U256::from(500_u16) / U256::from(10_000_u16);
        assert_eq!(
            proof
                .beneficiary_order
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["protocol", "creator"]
        );
        assert_eq!(proof.protocol_beneficiary, BANKR_PROTOCOL_BENEFICIARY);
        assert_eq!(init.beneficiaries.len(), 2);
        assert_eq!(
            init.beneficiaries[0].beneficiary,
            proof.protocol_beneficiary
        );
        assert_eq!(U256::from(init.beneficiaries[0].shares), protocol_share);
        assert_eq!(init.beneficiaries[1].beneficiary, proof.creator_beneficiary);
        assert_eq!(U256::from(init.beneficiaries[1].shares), creator_share);
        assert_eq!(token.beneficiaries.as_slice(), [proof.creator_beneficiary]);
        assert_eq!(init.curves.len(), 2);
        assert_eq!(i32::try_from(init.curves[0].tickLower).unwrap(), -229_400);
        assert_eq!(i32::try_from(init.curves[0].tickUpper).unwrap(), -119_400);
        assert_eq!(
            init.curves[0].shares,
            WAD * U256::from(99_u8) / U256::from(100_u8)
        );
        assert_eq!(i32::try_from(init.curves[1].tickLower).unwrap(), -119_400);
        assert_eq!(i32::try_from(init.curves[1].tickUpper).unwrap(), 887_200);
        assert_eq!(init.curves[1].shares, WAD / U256::from(100_u8));
        let decoded = validate_create_calldata(&call, profile).unwrap();
        assert_eq!(
            decoded.profile_version,
            BankrCreateProfileVersion::CurveTicksV3
        );
        assert_eq!(proof.create_profile_version, "curve_ticks_v3");
        assert!(validate_bankr_create_calldata_for_observation(
            &discovered.calldata
        ));
        let prediction = predict_bankr_create_identity(&discovered.calldata, profile).unwrap();
        assert_eq!(prediction.token, proof.token);
        assert_eq!(prediction.pool_id, proof.pool_id);

        let mut unsorted = call;
        let mut unsorted_init = init;
        unsorted_init.beneficiaries.swap(0, 1);
        replace_initializer_data(&mut unsorted, &unsorted_init);
        assert!(validate_create_calldata(&unsorted, profile).is_err());
        assert!(!validate_bankr_create_calldata_for_observation(
            &unsorted.abi_encode()
        ));

        let creator_first_proof = curve_ticks_v3_proofs().remove(0);
        let creator_first_call = abi::createCall::abi_decode(&creator_first_proof.create_calldata)
            .expect("reviewed creator-first proof is canonical create calldata");
        let creator_first_init = decode_initializer_data(&creator_first_call);
        let creator_first_token = decode_token_factory_data(&creator_first_call);
        assert_ne!(
            creator_first_init.beneficiaries[0].beneficiary,
            BANKR_PROTOCOL_BENEFICIARY
        );
        assert_eq!(
            creator_first_init.beneficiaries[1].beneficiary,
            BANKR_PROTOCOL_BENEFICIARY
        );
        assert!(
            creator_first_init.beneficiaries[0].beneficiary
                < creator_first_init.beneficiaries[1].beneficiary
        );
        assert_eq!(
            creator_first_token.beneficiaries.as_slice(),
            [creator_first_init.beneficiaries[0].beneficiary]
        );
        assert_eq!(
            validate_create_calldata(&creator_first_call, profile)
                .unwrap()
                .profile_version,
            BankrCreateProfileVersion::CurveTicksV3
        );
    }

    #[test]
    fn clean6_beneficiary_binding_rejects_weight_identity_and_shape_drift() {
        let proof = clean6_beneficiary_order_proof();
        let profile = BankrDopplerExpectedProfile::production();
        let discovered = discover_entry_point_v07_erc7579(
            EntryPointCall {
                chain_id: proof.chain_id,
                destination: profile.entry_point,
                outer_bundler: proof.outer_bundler,
                calldata: &proof.outer_input,
            },
            profile.entry_point,
            ContractPin {
                address: profile.airlock.address,
                runtime_code_hash: profile.airlock.runtime_code_hash,
            },
        )
        .unwrap();
        let call = abi::createCall::abi_decode(&discovered.calldata).unwrap();
        let rejects = |call: &abi::createCall| {
            assert!(validate_create_calldata(call, profile).is_err());
            assert!(!validate_bankr_create_calldata_for_observation(
                &call.abi_encode()
            ));
        };

        let mut swapped_weights = call.clone();
        let mut init = decode_initializer_data(&swapped_weights);
        let protocol_share = init.beneficiaries[0].shares;
        init.beneficiaries[0].shares = init.beneficiaries[1].shares;
        init.beneficiaries[1].shares = protocol_share;
        replace_initializer_data(&mut swapped_weights, &init);
        rejects(&swapped_weights);

        let mut total_share_drift = call.clone();
        let mut init = decode_initializer_data(&total_share_drift);
        init.beneficiaries[1].shares += alloy_primitives::Uint::from(1_u8);
        replace_initializer_data(&mut total_share_drift, &init);
        rejects(&total_share_drift);

        let mut wrong_protocol = call.clone();
        let mut init = decode_initializer_data(&wrong_protocol);
        init.beneficiaries[0].beneficiary = Address::with_last_byte(1);
        replace_initializer_data(&mut wrong_protocol, &init);
        rejects(&wrong_protocol);

        let mut wrong_creator = call.clone();
        let mut init = decode_initializer_data(&wrong_creator);
        init.beneficiaries[1].beneficiary = Address::with_last_byte(2);
        replace_initializer_data(&mut wrong_creator, &init);
        rejects(&wrong_creator);

        let mut wrong_vesting = call.clone();
        let mut token = decode_token_factory_data(&wrong_vesting);
        token.beneficiaries[0] = Address::with_last_byte(3);
        replace_token_factory_data(&mut wrong_vesting, &token);
        rejects(&wrong_vesting);

        let mut duplicate_protocol = call.clone();
        let mut init = decode_initializer_data(&duplicate_protocol);
        init.beneficiaries[1].beneficiary = BANKR_PROTOCOL_BENEFICIARY;
        replace_initializer_data(&mut duplicate_protocol, &init);
        rejects(&duplicate_protocol);

        let mut duplicate_creator = call.clone();
        let mut init = decode_initializer_data(&duplicate_creator);
        init.beneficiaries[0].beneficiary = proof.creator_beneficiary;
        replace_initializer_data(&mut duplicate_creator, &init);
        rejects(&duplicate_creator);

        let mut zero_creator = call.clone();
        let mut init = decode_initializer_data(&zero_creator);
        init.beneficiaries[1].beneficiary = Address::ZERO;
        let mut token = decode_token_factory_data(&zero_creator);
        token.beneficiaries[0] = Address::ZERO;
        replace_initializer_data(&mut zero_creator, &init);
        replace_token_factory_data(&mut zero_creator, &token);
        rejects(&zero_creator);

        let mut extra_initializer_beneficiary = call.clone();
        let mut init = decode_initializer_data(&extra_initializer_beneficiary);
        init.beneficiaries.push(init.beneficiaries[0].clone());
        replace_initializer_data(&mut extra_initializer_beneficiary, &init);
        rejects(&extra_initializer_beneficiary);

        let mut extra_vesting_beneficiary = call.clone();
        let mut token = decode_token_factory_data(&extra_vesting_beneficiary);
        token.beneficiaries.push(proof.creator_beneficiary);
        replace_token_factory_data(&mut extra_vesting_beneficiary, &token);
        rejects(&extra_vesting_beneficiary);

        let mut malformed_initializer = call;
        let mut malformed_data = malformed_initializer
            .createData
            .poolInitializerData
            .to_vec();
        malformed_data.push(0);
        malformed_initializer.createData.poolInitializerData = malformed_data.into();
        rejects(&malformed_initializer);
    }

    #[test]
    fn live_curve_ticks_v3_long_name_is_an_exact_bounded_proof() {
        let proof = long_name_live_proof();
        let profile = BankrDopplerExpectedProfile::production();
        assert_eq!(
            proof.tx_hash,
            alloy_primitives::b256!(
                "c38dc6277d87370878d2479bc7f0267879f08460b00e219d3782145d707289c6"
            )
        );
        assert_eq!(proof.chain_id, CHAIN_ID);
        assert_eq!(proof.block_number, "0xb31ce8");
        assert_eq!(
            proof.block_hash,
            alloy_primitives::b256!(
                "bd14972ff5c4d65b51b90059db89471e047e103e800355a4287454f51bd5324e"
            )
        );
        assert_eq!(proof.transaction_index, "0xa");
        assert_eq!(proof.transaction_type, "0x2");
        assert_eq!(proof.receipt_status, "0x1");
        assert_eq!(proof.entry_point, profile.entry_point.address);
        assert_eq!(proof.handle_ops_selector, "0x765e827f");
        assert_eq!(proof.account_call_selector, "0xe9ae5c53");
        assert_eq!(proof.mode, B256::ZERO);
        assert_eq!(proof.target, profile.airlock.address);
        assert_eq!(proof.value, "0x0");
        assert_eq!(proof.airlock_create_emitter, profile.airlock.address);
        assert_eq!(
            proof.airlock_create_topic,
            airlock_events::Create::SIGNATURE_HASH
        );
        assert_eq!(
            keccak256(&proof.account_designator),
            proof.account_designator_hash
        );
        assert_eq!(
            proof.account_designator_hash,
            profile.smart_account.account.runtime_code_hash
        );
        assert_eq!(proof.delegation_implementation, BANKR_KERNEL_IMPLEMENTATION);
        assert_eq!(proof.delegation_runtime_hash, BANKR_KERNEL_RUNTIME_HASH);
        let mut expected_designator = vec![0xef, 0x01, 0x00];
        expected_designator.extend_from_slice(proof.delegation_implementation.as_slice());
        assert_eq!(proof.account_designator.as_ref(), expected_designator);

        let discovered = discover_entry_point_v07_erc7579(
            EntryPointCall {
                chain_id: CHAIN_ID,
                destination: profile.entry_point,
                outer_bundler: proof.outer_bundler,
                calldata: &proof.outer_input,
            },
            profile.entry_point,
            ContractPin {
                address: profile.airlock.address,
                runtime_code_hash: profile.airlock.runtime_code_hash,
            },
        )
        .unwrap();
        assert_eq!(discovered.leader, proof.user_operation_sender);
        assert_eq!(discovered.target, proof.target);
        assert_eq!(discovered.value, U256::ZERO);
        assert_eq!(discovered.calldata, proof.create_calldata);

        let call = abi::createCall::abi_decode(&proof.create_calldata).unwrap();
        assert_eq!(call.abi_encode().as_slice(), proof.create_calldata.as_ref());
        assert_eq!(
            call.createData.tokenFactoryData.len(),
            proof.token_factory_data_bytes
        );
        let token = decode_token_factory_data(&call);
        assert_eq!(token.name, proof.token_name);
        assert_eq!(token.name.len(), proof.token_name_bytes);
        let decoded = validate_create_calldata(&call, profile).unwrap();
        assert_eq!(
            decoded.profile_version,
            BankrCreateProfileVersion::CurveTicksV3
        );
        assert_eq!(proof.create_profile_version, "curve_ticks_v3");
        assert!(validate_bankr_create_calldata_for_observation(
            &proof.create_calldata
        ));
        let prediction = predict_bankr_create_identity(&proof.create_calldata, profile).unwrap();
        assert_eq!(
            prediction.create_profile_version,
            BankrCreateProfileVersion::CurveTicksV3
        );
        assert_eq!(prediction.token, proof.token);
        assert_eq!(prediction.pool_id, proof.pool_id);
    }

    #[test]
    fn long_name_profile_rejects_65_bytes_and_noncanonical_inner_padding() {
        let proof = long_name_live_proof();
        let call = abi::createCall::abi_decode(&proof.create_calldata).unwrap();
        let token = decode_token_factory_data(&call);

        let mut maximum_token = token.clone();
        maximum_token.name = "x".repeat(64);
        let mut maximum = call.clone();
        replace_token_factory_data(&mut maximum, &maximum_token);
        assert_eq!(maximum.createData.tokenFactoryData.len(), 960);
        assert!(validate_bankr_create_calldata_for_observation(
            &maximum.abi_encode()
        ));

        let mut overlong_token = token.clone();
        overlong_token.name = "x".repeat(65);
        let mut overlong = call.clone();
        replace_token_factory_data(&mut overlong, &overlong_token);
        assert_eq!(overlong.createData.tokenFactoryData.len(), 992);
        assert!(!validate_bankr_create_calldata_for_observation(
            &overlong.abi_encode()
        ));

        let mut malformed = call;
        let mut token_factory_data = malformed.createData.tokenFactoryData.to_vec();
        let name = proof.token_name.as_bytes();
        let name_start = token_factory_data
            .windows(name.len())
            .position(|window| window == name)
            .expect("live name bytes are present once in token factory data");
        assert_eq!(token_factory_data[name_start + name.len()], 0);
        token_factory_data[name_start + name.len()] = 1;
        malformed.createData.tokenFactoryData = token_factory_data.into();
        assert_eq!(malformed.abi_encode().len(), proof.create_calldata.len());
        assert!(!validate_bankr_create_calldata_for_observation(
            &malformed.abi_encode()
        ));
    }

    #[test]
    fn prereceipt_prediction_reproduces_v1_v2_and_both_envelopes() {
        let profile = BankrDopplerExpectedProfile::production();
        let v1 = live_fixture();
        let v2_direct = production_fixture(include_str!(
            "../tests/fixtures/bankr-doppler-v2-direct-live-proof.json"
        ));
        let v2_erc7579 = production_fixture(include_str!(
            "../tests/fixtures/bankr-doppler-v2-erc7579-live-proof.json"
        ));
        for (transaction, receipt, expected_version) in [
            (
                &v1.transaction,
                &v1.receipt,
                BankrCreateProfileVersion::CurveTicksV1,
            ),
            (
                &v2_direct.transaction,
                &v2_direct.receipt,
                BankrCreateProfileVersion::CurveTicksV2,
            ),
            (
                &v2_erc7579.transaction,
                &v2_erc7579.receipt,
                BankrCreateProfileVersion::CurveTicksV2,
            ),
        ] {
            let prediction =
                predict_bankr_create_identity(&exact_create_calldata(transaction), profile)
                    .unwrap();
            let (receipt_token, receipt_pool_id) = canonical_receipt_identity(receipt);
            assert_eq!(prediction.create_profile_version, expected_version);
            assert_eq!(prediction.token, receipt_token);
            assert_eq!(prediction.pool_id, receipt_pool_id);
        }
    }

    #[test]
    fn prereceipt_prediction_rejects_unreviewed_inputs_and_separates_salts() {
        let profile = BankrDopplerExpectedProfile::production();
        let fixture = production_fixture(include_str!(
            "../tests/fixtures/bankr-doppler-v2-direct-live-proof.json"
        ));
        let calldata = exact_create_calldata(&fixture.transaction);
        let call = abi::createCall::abi_decode(&calldata).unwrap();
        let canonical = predict_bankr_create_identity(&calldata, profile).unwrap();

        let mut changed_salt = call.clone();
        changed_salt.createData.salt = keccak256(call.createData.salt);
        let changed = predict_bankr_create_identity(&changed_salt.abi_encode(), profile).unwrap();
        assert_ne!(changed.token, canonical.token);
        assert_ne!(changed.pool_id, canonical.pool_id);

        let mut wrong_factory = call.clone();
        wrong_factory.createData.tokenFactory = Address::with_last_byte(1);
        assert!(matches!(
            predict_bankr_create_identity(&wrong_factory.abi_encode(), profile),
            Err(BankrQuoteError::CreateCalldata)
        ));

        let mut wrong_curve = call.clone();
        let mut init =
            abi::DopplerInitData::abi_decode(&wrong_curve.createData.poolInitializerData).unwrap();
        init.curves[0].tickLower = (-229_200_i32).try_into().unwrap();
        wrong_curve.createData.poolInitializerData = init.abi_encode().into();
        assert!(matches!(
            predict_bankr_create_identity(&wrong_curve.abi_encode(), profile),
            Err(BankrQuoteError::CreateCalldata)
        ));

        let mut wrong_implementation = profile;
        wrong_implementation.token_implementation.address = Address::with_last_byte(2);
        assert!(matches!(
            predict_bankr_create_identity(&calldata, wrong_implementation),
            Err(BankrQuoteError::InvalidExpectedProfile)
        ));
    }

    #[test]
    fn predicted_identity_forged_receipt_fails_closed() {
        let profile = BankrDopplerExpectedProfile::production();
        let mut fixture = production_fixture(include_str!(
            "../tests/fixtures/bankr-doppler-v2-direct-live-proof.json"
        ));
        let prediction =
            predict_bankr_create_identity(&fixture.transaction.input, profile).unwrap();
        let launch = fixture
            .receipt
            .logs
            .iter_mut()
            .find(|log| {
                log.address == profile.airlock.address
                    && log.topics.first() == Some(&airlock_events::Create::SIGNATURE_HASH)
            })
            .unwrap();
        let mut forged_bytes = [0_u8; 20];
        forged_bytes.copy_from_slice(prediction.token.as_slice());
        forged_bytes[19] ^= 1;
        let forged = Address::from(forged_bytes);
        let mut forged_data = launch.data.to_vec();
        forged_data[12..32].copy_from_slice(forged.as_slice());
        forged_data[76..96].copy_from_slice(forged.as_slice());
        launch.data = forged_data.into();
        let direct_account = SmartAccountPin {
            account: ContractPin {
                address: fixture.transaction.from,
                runtime_code_hash: profile.smart_account.account.runtime_code_hash,
            },
            ..profile.smart_account
        };
        assert!(matches!(
            quote_bankr_doppler_launch_receipt_verified(
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                profile,
                BankrDopplerQuotePolicy {
                    amount_in: U256::from(1_000_000_000_000_000_u64),
                    max_amount_in: U256::from(1_000_000_000_000_000_u64),
                    slippage_bps: 100,
                },
                VerifiedBankrEnvelope::DirectAirlock {
                    smart_account: direct_account,
                },
            ),
            Err(BankrQuoteError::LaunchIdentity)
        ));
    }

    #[test]
    fn curve_ticks_v3_closest_neighbors_remain_rejected() {
        let proof = curve_ticks_v3_proofs().remove(0);
        let call = abi::createCall::abi_decode(&proof.create_calldata).unwrap();
        let canonical_init =
            abi::DopplerInitData::abi_decode(&call.createData.poolInitializerData).unwrap();

        for mutate in [
            |init: &mut abi::DopplerInitData| {
                init.curves[0].tickLower = (-229_200_i32).try_into().unwrap();
            },
            |init: &mut abi::DopplerInitData| {
                init.curves[0].tickUpper = (-119_200_i32).try_into().unwrap();
            },
            |init: &mut abi::DopplerInitData| {
                init.curves[1].tickLower = (-119_200_i32).try_into().unwrap();
            },
        ] {
            let mut mutated = call.clone();
            let mut init = canonical_init.clone();
            mutate(&mut init);
            mutated.createData.poolInitializerData = init.abi_encode().into();
            assert!(!validate_bankr_create_calldata_for_observation(
                &mutated.abi_encode()
            ));
        }
    }

    #[tokio::test]
    async fn final_tuple_curve_ticks_v4_all_sixteen_decode_reconcile_and_quote_exactly() {
        let fixture = final_tuple_v4_fixture();
        let profile = BankrDopplerExpectedProfile::production();
        assert_eq!(fixture.launches.len(), 16);
        assert_eq!(
            fixture.reconciliation_evidence_sha256,
            BTreeMap::from([
                (
                    "window-a".into(),
                    "c6422a5983b65ab7f0b29a3c212360a88e968a02e7ccfd63d06b7eefe7132685".into(),
                ),
                (
                    "window-b".into(),
                    "e15ed22eb4897097899275cc8eadad04ddb85c3f0c60f18ff24f56afd00f5ab1".into(),
                ),
                (
                    "window-c".into(),
                    "3b41870e4668f30a11c7cf672be057f4ae4117458ca12825dba6ea6abb8812de".into(),
                ),
            ])
        );
        assert_eq!(fixture.expected_account_designator.len(), 23);
        assert_eq!(
            keccak256(&fixture.expected_account_designator),
            fixture.expected_account_designator_hash
        );
        assert_eq!(
            fixture.expected_account_designator_hash,
            BANKR_ACCOUNT_DESIGNATOR_HASH
        );
        assert_eq!(
            fixture.delegation_implementation,
            BANKR_KERNEL_IMPLEMENTATION
        );
        assert_eq!(fixture.delegation_runtime.len(), 24_469);
        assert_eq!(
            keccak256(&fixture.delegation_runtime),
            fixture.delegation_runtime_hash
        );
        assert_eq!(fixture.delegation_runtime_hash, BANKR_KERNEL_RUNTIME_HASH);

        let mut window_counts = BTreeMap::new();
        let mut erc7579_count = 0;
        let mut direct_count = 0;
        let mut hashes = Vec::new();
        for proof in &fixture.launches {
            *window_counts.entry(proof.window.as_str()).or_insert(0_u64) += 1;
            hashes.push(proof.transaction.hash);
            let envelope = TxEnvelope::decode_2718_exact(&proof.raw_transaction).unwrap();
            assert_eq!(*envelope.tx_hash(), proof.transaction.hash);
            assert_eq!(envelope.recover_signer().unwrap(), proof.transaction.from);
            assert_eq!(envelope.to(), proof.transaction.to);
            assert_eq!(envelope.value(), proof.transaction.value);
            assert_eq!(envelope.input(), proof.transaction.input.as_ref());
            assert_eq!(envelope.chain_id(), Some(CHAIN_ID));
            assert_eq!(proof.transaction.hash, proof.receipt.transaction_hash);
            assert_eq!(
                proof.transaction.l2_block_number,
                Some(proof.block.l2_block_number)
            );
            assert_eq!(proof.receipt.l2_block_number, proof.block.l2_block_number);
            assert_eq!(proof.receipt.block_hash, proof.block.hash);
            assert!(proof.receipt.status);
            assert_eq!(
                proof.receipt_block_identity.account_designator,
                fixture.expected_account_designator
            );
            assert_eq!(
                proof.receipt_block_identity.account_designator_hash,
                fixture.expected_account_designator_hash
            );
            assert_eq!(
                proof.receipt_block_identity.delegation_runtime_hash,
                fixture.delegation_runtime_hash
            );

            match proof.envelope {
                BankrEnvelopeKind::Erc7579 => {
                    erc7579_count += 1;
                    assert_eq!(proof.transaction.to, Some(profile.entry_point.address));
                    assert_eq!(
                        proof.transaction.input.get(..4),
                        Some([0x76, 0x5e, 0x82, 0x7f].as_slice())
                    );
                }
                BankrEnvelopeKind::DirectAirlock => {
                    direct_count += 1;
                    assert_eq!(proof.transaction.to, Some(profile.airlock.address));
                    assert_eq!(
                        proof.transaction.input.get(..4),
                        Some(BANKR_CREATE_SELECTOR.as_slice())
                    );
                }
            }

            let create_calldata = exact_create_calldata(&proof.transaction);
            let create = abi::createCall::abi_decode(&create_calldata).unwrap();
            assert_eq!(create.abi_encode().as_slice(), create_calldata.as_ref());
            let decoded = validate_create_calldata(&create, profile).unwrap();
            assert_eq!(
                decoded.profile_version,
                BankrCreateProfileVersion::CurveTicksV4
            );
            assert!(validate_bankr_create_calldata_for_observation(
                &create_calldata
            ));
            let predicted = predict_bankr_create_identity(&create_calldata, profile).unwrap();
            assert_eq!(
                predicted.create_profile_version,
                BankrCreateProfileVersion::CurveTicksV4
            );

            let calls = successful_admission_calls_for(
                &proof.receipt,
                &proof.block,
                &proof.receipt_block_identity,
                proof.envelope == BankrEnvelopeKind::Erc7579,
            );
            let rpc = MockReceiptBlockRpc::new(calls);
            let quote = quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
                &rpc,
                &proof.transaction,
                &proof.receipt,
                &proof.block,
                profile,
                policy(),
            )
            .await
            .unwrap();
            rpc.assert_exhausted();
            assert_eq!(
                quote.market.create_profile_version,
                BankrCreateProfileVersion::CurveTicksV4
            );
            assert_eq!(quote.market.envelope, proof.envelope);
            assert_eq!(quote.market.token, predicted.token);
            assert_eq!(quote.market.pool_id, predicted.pool_id);
            assert_eq!(
                quote
                    .market
                    .positions
                    .iter()
                    .map(|position| (position.tick_lower, position.tick_upper, position.salt))
                    .collect::<Vec<_>>(),
                vec![
                    (119_200, 229_400, B256::ZERO),
                    (-887_200, 119_200, B256::with_last_byte(1)),
                ]
            );
            assert!(quote.entry.expected_output > U256::ZERO);
            assert!(quote.full_position_exit.expected_output > U256::ZERO);
            assert_eq!(
                quote.full_position_exit.amount_in,
                quote.entry.expected_output
            );
            assert!(!quote.execution_eligible);
            assert!(!quote.broadcast);
        }
        assert_eq!(
            window_counts,
            BTreeMap::from([("window-a", 9), ("window-b", 4), ("window-c", 3)])
        );
        assert_eq!(erc7579_count, 15);
        assert_eq!(direct_count, 1);
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(hashes.len(), 16);
    }

    #[tokio::test]
    async fn fresh_curve_ticks_v5_all_six_erc7579_proofs_reconcile_and_quote_exactly() {
        let fixture = fresh_curve_ticks_v5_fixture();
        let profile = BankrDopplerExpectedProfile::production();
        assert_eq!(fixture.launches.len(), 6);
        assert_eq!(fixture.expected_account_designator.len(), 23);
        assert_eq!(
            keccak256(&fixture.expected_account_designator),
            fixture.expected_account_designator_hash
        );
        assert_eq!(
            fixture.expected_account_designator_hash,
            BANKR_ACCOUNT_DESIGNATOR_HASH
        );
        assert_eq!(
            fixture.delegation_implementation,
            BANKR_KERNEL_IMPLEMENTATION
        );
        assert_eq!(
            keccak256(&fixture.delegation_runtime),
            fixture.delegation_runtime_hash
        );
        assert_eq!(fixture.delegation_runtime_hash, BANKR_KERNEL_RUNTIME_HASH);

        let mut hashes = Vec::new();
        let mut windows = BTreeMap::new();
        for proof in &fixture.launches {
            hashes.push(proof.transaction.hash);
            *windows.entry(proof.window.as_str()).or_insert(0_u64) += 1;
            assert_eq!(proof.envelope, BankrEnvelopeKind::Erc7579);
            assert_eq!(proof.transaction.to, Some(profile.entry_point.address));
            assert_eq!(
                proof.transaction.input.get(..4),
                Some([0x76, 0x5e, 0x82, 0x7f].as_slice())
            );
            assert_eq!(proof.transaction.hash, proof.receipt.transaction_hash);
            assert_eq!(proof.receipt.block_hash, proof.block.hash);
            assert!(proof.receipt.status);
            assert_eq!(
                proof.receipt_block_identity.l2_block_number,
                proof.block.l2_block_number
            );
            assert_eq!(proof.receipt_block_identity.block_hash, proof.block.hash);
            assert_eq!(
                proof.receipt_block_identity.account_designator,
                fixture.expected_account_designator
            );
            assert_eq!(proof.receipt_block_identity.account_designator_bytes, 23);
            assert_eq!(
                proof.receipt_block_identity.account_designator_hash,
                fixture.expected_account_designator_hash
            );
            assert_eq!(
                proof.receipt_block_identity.delegation_implementation,
                fixture.delegation_implementation
            );
            assert_eq!(
                proof.receipt_block_identity.delegation_runtime_bytes,
                fixture.delegation_runtime.len()
            );
            assert_eq!(
                proof.receipt_block_identity.delegation_runtime_hash,
                fixture.delegation_runtime_hash
            );

            let create_calldata = exact_create_calldata(&proof.transaction);
            let create = abi::createCall::abi_decode(&create_calldata).unwrap();
            let decoded = validate_create_calldata(&create, profile).unwrap();
            assert_eq!(
                decoded.profile_version,
                BankrCreateProfileVersion::CurveTicksV5
            );
            let predicted = predict_bankr_create_identity(&create_calldata, profile).unwrap();
            assert_eq!(
                predicted.create_profile_version,
                BankrCreateProfileVersion::CurveTicksV5
            );
            assert!(predicted.token > WETH);

            let calls = successful_admission_calls_for(
                &proof.receipt,
                &proof.block,
                &proof.receipt_block_identity,
                true,
            );
            let rpc = MockReceiptBlockRpc::new(calls);
            let quote = quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
                &rpc,
                &proof.transaction,
                &proof.receipt,
                &proof.block,
                profile,
                policy(),
            )
            .await
            .unwrap();
            rpc.assert_exhausted();
            assert_eq!(
                quote.market.create_profile_version,
                BankrCreateProfileVersion::CurveTicksV5
            );
            assert_eq!(quote.market.envelope, BankrEnvelopeKind::Erc7579);
            assert_eq!(quote.market.token, predicted.token);
            assert_eq!(quote.market.pool_id, predicted.pool_id);
            assert_eq!(quote.market.initialize_tick, 229_200);
            assert_eq!(
                quote.market.initialize_sqrt_price_x96,
                U256::from(7_510_096_409_285_047_843_309_134_522_194_364_u128)
            );
            assert_eq!(
                quote
                    .market
                    .positions
                    .iter()
                    .map(|position| (position.tick_lower, position.tick_upper, position.salt))
                    .collect::<Vec<_>>(),
                vec![
                    (119_200, 229_200, B256::ZERO),
                    (-887_200, 119_200, B256::with_last_byte(1)),
                ]
            );
            assert!(quote.entry.expected_output > U256::ZERO);
            assert!(quote.full_position_exit.expected_output > U256::ZERO);
            assert!(!quote.execution_eligible);
            assert!(!quote.broadcast);
        }
        assert_eq!(
            windows,
            BTreeMap::from([("window-a", 1), ("window-b", 1), ("window-c", 4)])
        );
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(hashes.len(), 6);
    }

    #[test]
    fn curve_ticks_v5_rejects_boundaries_order_shares_and_direct_envelope() {
        let fixture = fresh_curve_ticks_v5_fixture();
        let proof = &fixture.launches[0];
        let profile = BankrDopplerExpectedProfile::production();
        let calldata = exact_create_calldata(&proof.transaction);
        let call = abi::createCall::abi_decode(&calldata).unwrap();
        let canonical = decode_initializer_data(&call);

        for (curve_index, upper) in [(0_usize, false), (0, true), (1, false), (1, true)] {
            let mut candidate = call.clone();
            let mut init = canonical.clone();
            let tick = if upper {
                &mut init.curves[curve_index].tickUpper
            } else {
                &mut init.curves[curve_index].tickLower
            };
            *tick = (i32::try_from(*tick).unwrap() + 1).try_into().unwrap();
            replace_initializer_data(&mut candidate, &init);
            assert!(!validate_bankr_create_calldata_for_observation(
                &candidate.abi_encode()
            ));
        }
        for mutate in [
            (|init: &mut abi::DopplerInitData| init.curves.swap(0, 1))
                as fn(&mut abi::DopplerInitData),
            |init| init.curves[0].shares += U256::from(1_u8),
            |init| init.curves[1].shares -= U256::from(1_u8),
            |init| init.curves[0].numPositions += 1,
        ] {
            let mut candidate = call.clone();
            let mut init = canonical.clone();
            mutate(&mut init);
            replace_initializer_data(&mut candidate, &init);
            assert!(!validate_bankr_create_calldata_for_observation(
                &candidate.abi_encode()
            ));
        }

        let (below_weth, token) = (1_u8..=u8::MAX)
            .find_map(|salt| {
                let mut candidate = call.clone();
                candidate.createData.salt = B256::with_last_byte(salt);
                let token = predict_bankr_token(&candidate, profile);
                (token != Address::ZERO && token < WETH).then_some((candidate, token))
            })
            .unwrap();
        assert!(token < WETH);
        assert!(validate_bankr_create_calldata_for_observation(
            &below_weth.abi_encode()
        ));

        let smart_account = verified_smart_account_from_receipt_code(
            proof.receipt_block_identity.leader,
            &fixture.expected_account_designator,
            &fixture.delegation_runtime,
            profile,
        )
        .unwrap();
        let mut direct = proof.transaction.clone();
        direct.from = proof.receipt_block_identity.leader;
        direct.to = Some(profile.airlock.address);
        direct.input = calldata;
        direct.value = U256::ZERO;
        assert!(matches!(
            quote_bankr_doppler_launch_receipt_verified(
                &direct,
                &proof.receipt,
                &proof.block,
                profile,
                policy(),
                VerifiedBankrEnvelope::DirectAirlock { smart_account },
            ),
            Err(BankrQuoteError::SmartAccountIdentity)
        ));
    }

    #[tokio::test]
    async fn reverse_curve_ticks_v5_all_three_live_proofs_reconcile_and_quote_exactly() {
        let fixture = reverse_curve_ticks_v5_fixture();
        let profile = BankrDopplerExpectedProfile::production();
        assert_eq!(fixture.launches.len(), 3);
        assert_eq!(
            keccak256(&fixture.expected_account_designator),
            BANKR_ACCOUNT_DESIGNATOR_HASH
        );
        assert_eq!(
            fixture.delegation_implementation,
            BANKR_KERNEL_IMPLEMENTATION
        );
        assert_eq!(
            keccak256(&fixture.delegation_runtime),
            BANKR_KERNEL_RUNTIME_HASH
        );

        let mut hashes = Vec::new();
        for proof in &fixture.launches {
            assert_eq!(proof.window, "window-a");
            assert_eq!(proof.envelope, BankrEnvelopeKind::Erc7579);
            assert_eq!(proof.transaction.to, Some(profile.entry_point.address));
            assert_eq!(proof.transaction.hash, proof.receipt.transaction_hash);
            assert_eq!(proof.receipt.block_hash, proof.block.hash);
            assert!(proof.receipt.status);
            assert_eq!(proof.receipt_block_identity.block_hash, proof.block.hash);
            assert_eq!(
                proof.receipt_block_identity.account_designator_hash,
                BANKR_ACCOUNT_DESIGNATOR_HASH
            );
            assert_eq!(
                proof.receipt_block_identity.delegation_runtime_hash,
                BANKR_KERNEL_RUNTIME_HASH
            );

            let create_calldata = exact_create_calldata(&proof.transaction);
            let predicted = predict_bankr_create_identity(&create_calldata, profile).unwrap();
            assert_eq!(
                predicted.create_profile_version,
                BankrCreateProfileVersion::CurveTicksV5
            );
            assert!(predicted.token < WETH);
            assert_eq!(
                predicted.token,
                canonical_receipt_identity(&proof.receipt).0
            );
            assert_eq!(
                predicted.pool_id,
                canonical_receipt_identity(&proof.receipt).1
            );

            let rpc = MockReceiptBlockRpc::new(successful_admission_calls_for(
                &proof.receipt,
                &proof.block,
                &proof.receipt_block_identity,
                true,
            ));
            let quote = quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
                &rpc,
                &proof.transaction,
                &proof.receipt,
                &proof.block,
                profile,
                policy(),
            )
            .await
            .unwrap();
            rpc.assert_exhausted();
            assert_eq!(
                quote.market.create_profile_version,
                BankrCreateProfileVersion::CurveTicksV5
            );
            assert_eq!(quote.market.envelope, BankrEnvelopeKind::Erc7579);
            assert_eq!(quote.market.token, predicted.token);
            assert_eq!(quote.market.pool_id, predicted.pool_id);
            assert_eq!(quote.market.initialize_tick, -229_200);
            assert_eq!(
                quote.market.initialize_sqrt_price_x96,
                U256::from_str_radix("b0fdfc8b493ef2e496b6", 16).unwrap()
            );
            assert_eq!(
                quote
                    .market
                    .positions
                    .iter()
                    .map(|position| (
                        position.tick_lower,
                        position.tick_upper,
                        position.liquidity,
                        position.salt,
                    ))
                    .collect::<Vec<_>>(),
                vec![
                    (
                        -229_200,
                        -119_200,
                        U256::from_str_radix("bcc248d856f01dd554c5", 16).unwrap(),
                        B256::ZERO,
                    ),
                    (
                        -119_200,
                        887_200,
                        U256::from_str_radix("1d082240a370451eb5ea2", 16).unwrap(),
                        B256::with_last_byte(1),
                    ),
                ]
            );
            assert!(quote.entry.expected_output > U256::ZERO);
            assert!(quote.full_position_exit.expected_output > U256::ZERO);
            assert!(!quote.execution_eligible);
            assert!(!quote.broadcast);
            hashes.push(quote.tx_hash);
        }
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(hashes.len(), 3);
    }

    #[test]
    fn reverse_curve_ticks_v5_rejects_receipt_range_liquidity_salt_and_pin_drift() {
        let fixture = reverse_curve_ticks_v5_fixture();
        let proof = &fixture.launches[0];
        let profile = BankrDopplerExpectedProfile::production();
        let smart_account = verified_smart_account_from_receipt_code(
            proof.receipt_block_identity.leader,
            &fixture.expected_account_designator,
            &fixture.delegation_runtime,
            profile,
        )
        .unwrap();
        let quote = |receipt: &NoxaReceipt| {
            quote_bankr_doppler_launch_receipt_verified(
                &proof.transaction,
                receipt,
                &proof.block,
                profile,
                policy(),
                VerifiedBankrEnvelope::Erc7579 { smart_account },
            )
        };
        assert!(quote(&proof.receipt).is_ok());
        let liquidity_indices = proof
            .receipt
            .logs
            .iter()
            .enumerate()
            .filter_map(|(index, log)| {
                (log.topics.first() == Some(&abi::ModifyLiquidity::SIGNATURE_HASH)).then_some(index)
            })
            .collect::<Vec<_>>();
        assert_eq!(liquidity_indices.len(), 2);
        for (offset, word_end) in [
            (0_usize, 32_usize),
            (0, 64),
            (0, 96),
            (0, 128),
            (1, 32),
            (1, 64),
            (1, 96),
            (1, 128),
        ] {
            let mut receipt = proof.receipt.clone();
            let mut data = receipt.logs[liquidity_indices[offset]].data.to_vec();
            data[word_end - 1] ^= 1;
            receipt.logs[liquidity_indices[offset]].data = data.into();
            assert!(matches!(
                quote(&receipt),
                Err(BankrQuoteError::LiquiditySequence)
            ));
        }
        let mut designator = fixture.expected_account_designator.to_vec();
        designator[3] ^= 1;
        assert!(
            verified_smart_account_from_receipt_code(
                proof.receipt_block_identity.leader,
                &designator,
                &fixture.delegation_runtime,
                profile,
            )
            .is_err()
        );
    }

    #[test]
    fn distinct_direct_fee100000_and_shifted_ticks_profile_remains_unsupported() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-unreviewed-direct-fee100000-profile.json"
        ))
        .unwrap();
        assert_eq!(fixture["envelope"], "direct_airlock");
        assert_eq!(fixture["fee"], 100_000);
        assert_eq!(fixture["curves"][0][0], -228_600);
        assert_eq!(fixture["curves"][0][1], -118_600);
        let calldata = hex::decode(
            fixture["input"]
                .as_str()
                .unwrap()
                .strip_prefix("0x")
                .unwrap(),
        )
        .unwrap();
        assert!(!validate_bankr_create_calldata_for_observation(&calldata));
        let call = abi::createCall::abi_decode(&calldata).unwrap();
        assert!(matches!(
            validate_create_calldata(&call, BankrDopplerExpectedProfile::production()),
            Err(BankrQuoteError::CreateCalldata)
        ));
    }

    #[test]
    fn curve_ticks_v5_rejects_position_salt_order_initialize_and_delegation_drift() {
        let fixture = fresh_curve_ticks_v5_fixture();
        let proof = &fixture.launches[0];
        let profile = BankrDopplerExpectedProfile::production();
        let smart_account = verified_smart_account_from_receipt_code(
            proof.receipt_block_identity.leader,
            &fixture.expected_account_designator,
            &fixture.delegation_runtime,
            profile,
        )
        .unwrap();
        let quote = |receipt: &NoxaReceipt| {
            quote_bankr_doppler_launch_receipt_verified(
                &proof.transaction,
                receipt,
                &proof.block,
                profile,
                policy(),
                VerifiedBankrEnvelope::Erc7579 { smart_account },
            )
        };
        let initialize_index = proof
            .receipt
            .logs
            .iter()
            .position(|log| log.topics.first() == Some(&abi::Initialize::SIGNATURE_HASH))
            .unwrap();
        for word_end in [128_usize, 160] {
            let mut receipt = proof.receipt.clone();
            let mut data = receipt.logs[initialize_index].data.to_vec();
            data[word_end - 1] ^= 1;
            receipt.logs[initialize_index].data = data.into();
            assert!(matches!(
                quote(&receipt),
                Err(BankrQuoteError::InitializeIdentity)
            ));
        }
        let liquidity_indices = proof
            .receipt
            .logs
            .iter()
            .enumerate()
            .filter_map(|(index, log)| {
                (log.topics.first() == Some(&abi::ModifyLiquidity::SIGNATURE_HASH)).then_some(index)
            })
            .collect::<Vec<_>>();
        assert_eq!(liquidity_indices.len(), 2);
        for (offset, word_end) in [
            (0_usize, 32_usize),
            (0, 64),
            (0, 128),
            (1, 32),
            (1, 64),
            (1, 128),
        ] {
            let mut receipt = proof.receipt.clone();
            let mut data = receipt.logs[liquidity_indices[offset]].data.to_vec();
            data[word_end - 1] ^= 1;
            receipt.logs[liquidity_indices[offset]].data = data.into();
            assert!(matches!(
                quote(&receipt),
                Err(BankrQuoteError::LiquiditySequence)
            ));
        }
        let mut reordered = proof.receipt.clone();
        reordered
            .logs
            .swap(liquidity_indices[0], liquidity_indices[1]);
        assert!(quote(&reordered).is_err());

        let mut designator = fixture.expected_account_designator.to_vec();
        designator[3] ^= 1;
        assert!(
            verified_smart_account_from_receipt_code(
                proof.receipt_block_identity.leader,
                &designator,
                &fixture.delegation_runtime,
                profile
            )
            .is_err()
        );
        let mut runtime = fixture.delegation_runtime.to_vec();
        runtime[0] ^= 1;
        assert!(
            verified_smart_account_from_receipt_code(
                proof.receipt_block_identity.leader,
                &fixture.expected_account_designator,
                &runtime,
                profile
            )
            .is_err()
        );
    }

    #[test]
    fn curve_ticks_v4_rejects_one_tick_drift_on_every_boundary_and_shape_drift() {
        let fixture = final_tuple_v4_fixture();
        let proof = &fixture.launches[0];
        let calldata = exact_create_calldata(&proof.transaction);
        let call = abi::createCall::abi_decode(&calldata).unwrap();
        let canonical = decode_initializer_data(&call);

        for (curve_index, upper) in [(0_usize, false), (0, true), (1, false), (1, true)] {
            let mut candidate = call.clone();
            let mut init = canonical.clone();
            let tick = if upper {
                &mut init.curves[curve_index].tickUpper
            } else {
                &mut init.curves[curve_index].tickLower
            };
            *tick = (i32::try_from(*tick).unwrap() + 1).try_into().unwrap();
            replace_initializer_data(&mut candidate, &init);
            assert!(!validate_bankr_create_calldata_for_observation(
                &candidate.abi_encode()
            ));
        }

        let rejects_init = |mutate: fn(&mut abi::DopplerInitData)| {
            let mut candidate = call.clone();
            let mut init = canonical.clone();
            mutate(&mut init);
            replace_initializer_data(&mut candidate, &init);
            assert!(!validate_bankr_create_calldata_for_observation(
                &candidate.abi_encode()
            ));
        };
        rejects_init(|init| init.curves[0].shares += U256::from(1_u8));
        rejects_init(|init| init.curves.swap(0, 1));
        rejects_init(|init| init.curves[1].numPositions += 1);
        rejects_init(|init| init.beneficiaries[0].beneficiary = Address::with_last_byte(0xee));

        let mut wrong_factory = call;
        wrong_factory.createData.tokenFactory = Address::with_last_byte(0xee);
        assert!(!validate_bankr_create_calldata_for_observation(
            &wrong_factory.abi_encode()
        ));
    }

    #[test]
    fn curve_ticks_v4_reverse_is_admitted_only_for_the_exact_reviewed_calldata_profile() {
        let fixture = reverse_v4_fixture();
        let proof = &fixture.launches[0];
        let profile = BankrDopplerExpectedProfile::production();
        let calldata = exact_create_calldata(&proof.transaction);
        let call = abi::createCall::abi_decode(&calldata).unwrap();
        let decoded = validate_create_calldata(&call, profile).unwrap();
        assert_eq!(
            decoded.profile_version,
            BankrCreateProfileVersion::CurveTicksV4
        );
        let predicted = predict_bankr_create_identity(&calldata, profile).unwrap();
        assert!(predicted.token < profile.weth.address);
        assert!(validate_bankr_create_calldata_for_observation(&calldata));
        assert_eq!(
            predicted.token,
            canonical_receipt_identity(&proof.receipt).0
        );
        assert_eq!(
            predicted.pool_id,
            canonical_receipt_identity(&proof.receipt).1
        );

        let mut wrong_profile = call;
        let mut init = decode_initializer_data(&wrong_profile);
        init.curves[0].tickUpper = (-119_400).try_into().unwrap();
        replace_initializer_data(&mut wrong_profile, &init);
        assert!(!validate_bankr_create_calldata_for_observation(
            &wrong_profile.abi_encode()
        ));
    }

    #[tokio::test]
    async fn curve_ticks_v4_reverse_both_full_proofs_reconcile_and_quote_exactly() {
        let fixture = reverse_v4_fixture();
        let profile = BankrDopplerExpectedProfile::production();
        assert_eq!(fixture.launches.len(), 2);
        let mut hashes = Vec::new();
        for proof in &fixture.launches {
            assert!(matches!(
                proof.window.as_str(),
                "post-stonks-a" | "post-stonks-b"
            ));
            assert_eq!(proof.envelope, BankrEnvelopeKind::Erc7579);
            assert_eq!(proof.transaction.to, Some(profile.entry_point.address));
            assert_eq!(
                proof.transaction.input.get(..4),
                Some([0x76, 0x5e, 0x82, 0x7f].as_slice())
            );
            assert_eq!(proof.receipt.transaction_hash, proof.transaction.hash);
            assert_eq!(proof.receipt.block_hash, proof.block.hash);
            assert_eq!(proof.receipt_block_identity.block_hash, proof.block.hash);
            assert_eq!(
                proof.receipt_block_identity.l2_block_number,
                proof.block.l2_block_number
            );
            assert_eq!(proof.receipt_block_identity.account_designator_bytes, 23);
            assert_eq!(
                proof.receipt_block_identity.account_designator_hash,
                BANKR_ACCOUNT_DESIGNATOR_HASH
            );
            assert_eq!(
                proof.receipt_block_identity.delegation_implementation,
                BANKR_KERNEL_IMPLEMENTATION
            );
            assert_eq!(
                proof.receipt_block_identity.delegation_runtime_hash,
                BANKR_KERNEL_RUNTIME_HASH
            );

            let create_calldata = exact_create_calldata(&proof.transaction);
            let predicted = predict_bankr_create_identity(&create_calldata, profile).unwrap();
            assert_eq!(
                predicted.create_profile_version,
                BankrCreateProfileVersion::CurveTicksV4
            );
            assert!(predicted.token < WETH);

            let rpc = MockReceiptBlockRpc::new(successful_admission_calls_for(
                &proof.receipt,
                &proof.block,
                &proof.receipt_block_identity,
                true,
            ));
            let quote = quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
                &rpc,
                &proof.transaction,
                &proof.receipt,
                &proof.block,
                profile,
                policy(),
            )
            .await
            .unwrap();
            rpc.assert_exhausted();
            assert_eq!(
                quote.market.create_profile_version,
                BankrCreateProfileVersion::CurveTicksV4
            );
            assert_eq!(quote.market.envelope, BankrEnvelopeKind::Erc7579);
            assert_eq!(quote.market.token, predicted.token);
            assert_eq!(quote.market.pool_id, predicted.pool_id);
            assert_eq!(quote.market.initialize_tick, -229_400);
            assert_eq!(
                quote.market.initialize_sqrt_price_x96,
                U256::from_str_radix("af3b2ac279070c26b9f3", 16).unwrap()
            );
            assert_eq!(
                quote
                    .market
                    .positions
                    .iter()
                    .map(|position| (
                        position.tick_lower,
                        position.tick_upper,
                        position.liquidity,
                        position.salt,
                    ))
                    .collect::<Vec<_>>(),
                vec![
                    (
                        -229_400,
                        -119_200,
                        U256::from_str_radix("badf8a38e438d69a45c2", 16).unwrap(),
                        B256::ZERO
                    ),
                    (
                        -119_200,
                        887_200,
                        U256::from_str_radix("1d082240a370451eb5ea2", 16).unwrap(),
                        B256::with_last_byte(1)
                    ),
                ]
            );
            assert_eq!(quote.entry.amount_in, U256::from(1_000_000_000_000_000_u64));
            assert_eq!(
                quote.full_position_exit.amount_in,
                quote.entry.expected_output
            );
            assert_eq!(quote.entry.slippage_bps, 100);
            assert!(quote.entry.expected_output > U256::ZERO);
            assert!(quote.full_position_exit.expected_output > U256::ZERO);
            assert!(!quote.execution_eligible);
            assert!(!quote.broadcast);
            hashes.push(quote.tx_hash);
        }
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(hashes.len(), 2);
    }

    #[test]
    fn curve_ticks_v4_reverse_rejects_every_calldata_boundary_order_and_share_tamper() {
        let fixture = reverse_v4_fixture();
        let proof = &fixture.launches[0];
        let calldata = exact_create_calldata(&proof.transaction);
        let call = abi::createCall::abi_decode(&calldata).unwrap();
        let canonical = decode_initializer_data(&call);

        for (curve_index, upper) in [(0_usize, false), (0, true), (1, false), (1, true)] {
            let mut candidate = call.clone();
            let mut init = canonical.clone();
            let tick = if upper {
                &mut init.curves[curve_index].tickUpper
            } else {
                &mut init.curves[curve_index].tickLower
            };
            *tick = (i32::try_from(*tick).unwrap() + 1).try_into().unwrap();
            replace_initializer_data(&mut candidate, &init);
            assert!(!validate_bankr_create_calldata_for_observation(
                &candidate.abi_encode()
            ));
        }
        for mutate in [
            (|init: &mut abi::DopplerInitData| init.curves.swap(0, 1))
                as fn(&mut abi::DopplerInitData),
            |init| init.curves[0].shares += U256::from(1_u8),
            |init| init.curves[1].shares -= U256::from(1_u8),
            |init| init.curves[0].numPositions += 1,
        ] {
            let mut candidate = call.clone();
            let mut init = canonical.clone();
            mutate(&mut init);
            replace_initializer_data(&mut candidate, &init);
            assert!(!validate_bankr_create_calldata_for_observation(
                &candidate.abi_encode()
            ));
        }
    }

    #[test]
    fn curve_ticks_v4_reverse_rejects_receipt_orientation_order_liquidity_salt_and_pin_tamper() {
        let fixture = reverse_v4_fixture();
        let proof = &fixture.launches[0];
        let profile = BankrDopplerExpectedProfile::production();
        let smart_account = verified_smart_account_from_receipt_code(
            proof.receipt_block_identity.leader,
            &proof.receipt_block_identity.account_designator,
            &kernel_runtime(),
            profile,
        )
        .unwrap();
        let quote = |receipt: &NoxaReceipt| {
            quote_bankr_doppler_launch_receipt_verified(
                &proof.transaction,
                receipt,
                &proof.block,
                profile,
                policy(),
                VerifiedBankrEnvelope::Erc7579 { smart_account },
            )
        };
        assert!(quote(&proof.receipt).is_ok());

        let initialize_index = proof
            .receipt
            .logs
            .iter()
            .position(|log| log.topics.first() == Some(&abi::Initialize::SIGNATURE_HASH))
            .unwrap();
        let liquidity_indices = proof
            .receipt
            .logs
            .iter()
            .enumerate()
            .filter_map(|(index, log)| {
                (log.topics.first() == Some(&abi::ModifyLiquidity::SIGNATURE_HASH)).then_some(index)
            })
            .collect::<Vec<_>>();
        assert_eq!(liquidity_indices.len(), 2);

        let mut wrong_orientation = proof.receipt.clone();
        wrong_orientation.logs[initialize_index].topics.swap(2, 3);
        assert!(matches!(
            quote(&wrong_orientation),
            Err(BankrQuoteError::InitializeIdentity)
        ));

        let mut wrong_initialize_tick = proof.receipt.clone();
        let mut initialize_data = wrong_initialize_tick.logs[initialize_index].data.to_vec();
        *initialize_data.last_mut().unwrap() ^= 1;
        wrong_initialize_tick.logs[initialize_index].data = initialize_data.into();
        assert!(matches!(
            quote(&wrong_initialize_tick),
            Err(BankrQuoteError::InitializeIdentity)
        ));

        let mut wrong_order = proof.receipt.clone();
        wrong_order
            .logs
            .swap(liquidity_indices[0], liquidity_indices[1]);
        assert!(quote(&wrong_order).is_err());

        for (offset, byte) in [
            (0_usize, 31_usize),
            (0, 63),
            (0, 95),
            (0, 127),
            (1, 31),
            (1, 63),
            (1, 95),
            (1, 127),
        ] {
            let mut receipt = proof.receipt.clone();
            let mut data = receipt.logs[liquidity_indices[offset]].data.to_vec();
            data[byte] ^= 1;
            receipt.logs[liquidity_indices[offset]].data = data.into();
            assert!(
                quote(&receipt).is_err(),
                "receipt mutation unexpectedly passed at position {offset}, byte {byte}"
            );
        }

        let mut zero_liquidity = proof.receipt.clone();
        let mut data = zero_liquidity.logs[liquidity_indices[0]].data.to_vec();
        data[64..96].fill(0);
        zero_liquidity.logs[liquidity_indices[0]].data = data.into();
        assert!(quote(&zero_liquidity).is_err());

        let mut wrong_designator = proof.receipt_block_identity.account_designator.to_vec();
        wrong_designator[3] ^= 1;
        assert!(
            verified_smart_account_from_receipt_code(
                proof.receipt_block_identity.leader,
                &wrong_designator,
                &kernel_runtime(),
                profile,
            )
            .is_err()
        );
        let mut wrong_kernel = kernel_runtime();
        wrong_kernel[0] ^= 1;
        assert!(
            verified_smart_account_from_receipt_code(
                proof.receipt_block_identity.leader,
                &proof.receipt_block_identity.account_designator,
                &wrong_kernel,
                profile,
            )
            .is_err()
        );
        let mut wrong_dependency_pin = profile;
        wrong_dependency_pin.pool_manager.runtime_code_hash = B256::with_last_byte(0xee);
        assert!(wrong_dependency_pin.validate().is_err());
    }

    #[tokio::test]
    async fn curve_ticks_v4_rejects_envelope_designator_runtime_and_receipt_liquidity_drift() {
        let fixture = final_tuple_v4_fixture();
        let proof = &fixture.launches[0];
        let profile = BankrDopplerExpectedProfile::production();

        let mut wrong_envelope = proof.transaction.clone();
        wrong_envelope.to = Some(Address::with_last_byte(0xee));
        let rpc = MockReceiptBlockRpc::new(Vec::new());
        assert!(matches!(
            quote_bankr_doppler_launch_receipt_at_receipt_block_with_rpc(
                &rpc,
                &wrong_envelope,
                &proof.receipt,
                &proof.block,
                profile,
                policy(),
            )
            .await,
            Err(BankrQuoteError::ReceiptBlockIdentity(_))
        ));
        rpc.assert_exhausted();

        let mut wrong_designator = fixture.expected_account_designator.to_vec();
        wrong_designator[3] ^= 1;
        assert!(
            verified_smart_account_from_receipt_code(
                proof.receipt_block_identity.leader,
                &wrong_designator,
                &fixture.delegation_runtime,
                profile,
            )
            .is_err()
        );
        let mut wrong_runtime = fixture.delegation_runtime.to_vec();
        wrong_runtime[0] ^= 1;
        assert!(
            verified_smart_account_from_receipt_code(
                proof.receipt_block_identity.leader,
                &fixture.expected_account_designator,
                &wrong_runtime,
                profile,
            )
            .is_err()
        );

        let smart_account = verified_smart_account_from_receipt_code(
            proof.receipt_block_identity.leader,
            &fixture.expected_account_designator,
            &fixture.delegation_runtime,
            profile,
        )
        .unwrap();
        let liquidity_indices = proof
            .receipt
            .logs
            .iter()
            .enumerate()
            .filter_map(|(index, log)| {
                (log.topics.first() == Some(&abi::ModifyLiquidity::SIGNATURE_HASH)).then_some(index)
            })
            .collect::<Vec<_>>();
        assert_eq!(liquidity_indices.len(), 2);
        for (log_offset, word_end) in [(0_usize, 32_usize), (0, 64), (1, 32), (1, 64)] {
            let mut receipt = proof.receipt.clone();
            let data = &mut receipt.logs[liquidity_indices[log_offset]].data.to_vec();
            data[word_end - 1] ^= 1;
            receipt.logs[liquidity_indices[log_offset]].data = data.clone().into();
            assert!(matches!(
                quote_bankr_doppler_launch_receipt_verified(
                    &proof.transaction,
                    &receipt,
                    &proof.block,
                    profile,
                    policy(),
                    VerifiedBankrEnvelope::Erc7579 { smart_account },
                ),
                Err(BankrQuoteError::LiquiditySequence)
            ));
        }

        let mut zero_liquidity = proof.receipt.clone();
        let data = &mut zero_liquidity.logs[liquidity_indices[0]].data.to_vec();
        data[64..96].fill(0);
        zero_liquidity.logs[liquidity_indices[0]].data = data.clone().into();
        assert!(matches!(
            quote_bankr_doppler_launch_receipt_verified(
                &proof.transaction,
                &zero_liquidity,
                &proof.block,
                profile,
                policy(),
                VerifiedBankrEnvelope::Erc7579 { smart_account },
            ),
            Err(BankrQuoteError::LiquiditySequence)
        ));

        let mut wrong_order = proof.receipt.clone();
        wrong_order
            .logs
            .swap(liquidity_indices[0], liquidity_indices[1]);
        assert!(
            quote_bankr_doppler_launch_receipt_verified(
                &proof.transaction,
                &wrong_order,
                &proof.block,
                profile,
                policy(),
                VerifiedBankrEnvelope::Erc7579 { smart_account },
            )
            .is_err()
        );
    }

    #[test]
    fn reviewed_create_profile_rejects_every_fixed_field_drift() {
        let fixture = production_fixture(include_str!(
            "../tests/fixtures/bankr-doppler-v2-direct-live-proof.json"
        ));
        let call = abi::createCall::abi_decode(&fixture.transaction.input).unwrap();
        assert!(validate_bankr_create_calldata_for_observation(
            &call.abi_encode()
        ));

        let rejects = |call: &abi::createCall| {
            assert!(!validate_bankr_create_calldata_for_observation(
                &call.abi_encode()
            ));
        };
        let mut mutated = call.clone();
        mutated.createData.initialSupply += U256::from(1_u8);
        rejects(&mutated);
        let mut mutated = call.clone();
        mutated.createData.numTokensToSell += U256::from(1_u8);
        rejects(&mutated);
        let mut mutated = call.clone();
        mutated.createData.integrator = Address::with_last_byte(1);
        rejects(&mutated);
        let mut mutated = call.clone();
        mutated.createData.governanceFactoryData = alloy_primitives::Bytes::new();
        rejects(&mutated);
        let mut mutated = call.clone();
        mutated.createData.liquidityMigratorData = vec![1_u8].into();
        rejects(&mutated);

        let token = decode_token_factory_data(&call);
        let mut mutated_token = token.clone();
        mutated_token.name.clear();
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated_token = token.clone();
        mutated_token.symbol = "x".repeat(33);
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated_token = token.clone();
        mutated_token.schedules[0].cliff += 1;
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated_token = token.clone();
        mutated_token.schedules[0].duration += 1;
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated_token = token.clone();
        mutated_token.beneficiaries[0] = Address::with_last_byte(1);
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated = call.clone();
        let mut init =
            abi::DopplerInitData::abi_decode(&mutated.createData.poolInitializerData).unwrap();
        init.beneficiaries[0].beneficiary = BANKR_PROTOCOL_BENEFICIARY;
        replace_initializer_data(&mut mutated, &init);
        let mut mutated_token = token.clone();
        mutated_token.beneficiaries[0] = BANKR_PROTOCOL_BENEFICIARY;
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated_token = token.clone();
        mutated_token.scheduleIds[0] = U256::from(1_u8);
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated_token = token.clone();
        mutated_token.amounts[0] += U256::from(1_u8);
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated_token = token.clone();
        mutated_token.tokenURI.replace_range(14..15, "0");
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated_token = token.clone();
        mutated_token.tokenURI.pop();
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated_token = token.clone();
        mutated_token.maxBalanceLimit = U256::from(1_u8);
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated_token = token.clone();
        mutated_token.balanceLimitEnd = 1_u64.try_into().unwrap();
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated_token = token.clone();
        mutated_token.controller = Address::with_last_byte(1);
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut mutated_token = token;
        mutated_token
            .excludedFromBalanceLimit
            .push(Address::with_last_byte(1));
        let mut mutated = call.clone();
        replace_token_factory_data(&mut mutated, &mutated_token);
        rejects(&mutated);
        let mut trailing = call.clone();
        trailing.createData.tokenFactoryData = {
            let mut bytes = call.createData.tokenFactoryData.to_vec();
            bytes.push(0);
            bytes.into()
        };
        rejects(&trailing);
    }

    #[test]
    fn reviewed_create_profile_rejects_outer_and_initializer_field_drift() {
        let fixture = production_fixture(include_str!(
            "../tests/fixtures/bankr-doppler-v2-direct-live-proof.json"
        ));
        let call = abi::createCall::abi_decode(&fixture.transaction.input).unwrap();
        let rejects = |candidate: &abi::createCall| {
            assert!(!validate_bankr_create_calldata_for_observation(
                &candidate.abi_encode()
            ));
        };

        for mutate in [
            |candidate: &mut abi::createCall| {
                candidate.createData.numeraire = Address::with_last_byte(1)
            },
            |candidate: &mut abi::createCall| {
                candidate.createData.tokenFactory = Address::with_last_byte(2)
            },
            |candidate: &mut abi::createCall| {
                candidate.createData.governanceFactory = Address::with_last_byte(3)
            },
            |candidate: &mut abi::createCall| {
                candidate.createData.poolInitializer = Address::with_last_byte(4)
            },
            |candidate: &mut abi::createCall| {
                candidate.createData.liquidityMigrator = Address::with_last_byte(5)
            },
        ] {
            let mut candidate = call.clone();
            mutate(&mut candidate);
            rejects(&candidate);
        }
        let mut candidate = call.clone();
        let mut governance_data = candidate.createData.governanceFactoryData.to_vec();
        governance_data[31] = 1;
        candidate.createData.governanceFactoryData = governance_data.into();
        rejects(&candidate);
        let mut candidate = call.clone();
        candidate.createData.salt = B256::ZERO;
        rejects(&candidate);
        let mut candidate = call.clone();
        let mut initializer_data = candidate.createData.poolInitializerData.to_vec();
        initializer_data.push(0);
        candidate.createData.poolInitializerData = initializer_data.into();
        rejects(&candidate);

        let rejects_init = |mutate: fn(&mut abi::DopplerInitData)| {
            let mut candidate = call.clone();
            let mut init = decode_initializer_data(&candidate);
            mutate(&mut init);
            replace_initializer_data(&mut candidate, &init);
            rejects(&candidate);
        };
        rejects_init(|init| init.fee = Default::default());
        rejects_init(|init| init.tickSpacing = 400_i32.try_into().unwrap());
        rejects_init(|init| init.farTick = 886_800_i32.try_into().unwrap());
        rejects_init(|init| init.dopplerHook = Address::with_last_byte(6));
        rejects_init(|init| init.graduationDopplerHookCalldata = vec![1].into());
        rejects_init(|init| {
            init.curves.pop();
        });
        rejects_init(|init| {
            init.beneficiaries.pop();
        });
        rejects_init(|init| init.curves[0].tickUpper = (-119_200_i32).try_into().unwrap());
        rejects_init(|init| init.curves[0].numPositions += 1);
        rejects_init(|init| init.curves[0].shares += U256::from(1_u8));
        rejects_init(|init| init.beneficiaries[0].beneficiary = Address::ZERO);
        rejects_init(|init| init.beneficiaries[0].shares = Default::default());
        rejects_init(|init| init.beneficiaries[1].beneficiary = Address::with_last_byte(7));
        rejects_init(|init| init.beneficiaries[1].shares = Default::default());
    }

    #[test]
    fn reviewed_token_factory_rejects_uncovered_shape_drift() {
        let fixture = production_fixture(include_str!(
            "../tests/fixtures/bankr-doppler-v2-direct-live-proof.json"
        ));
        let call = abi::createCall::abi_decode(&fixture.transaction.input).unwrap();
        let token = decode_token_factory_data(&call);
        let rejects_token = |mutate: fn(&mut abi::tokenFactoryDataCall)| {
            let mut candidate = call.clone();
            let mut token = token.clone();
            mutate(&mut token);
            replace_token_factory_data(&mut candidate, &token);
            assert!(!validate_bankr_create_calldata_for_observation(
                &candidate.abi_encode()
            ));
        };
        rejects_token(|token| token.name = "x".repeat(65));
        rejects_token(|token| token.symbol.clear());
        rejects_token(|token| token.schedules.push(token.schedules[0].clone()));
        rejects_token(|token| token.beneficiaries.push(token.beneficiaries[0]));
        rejects_token(|token| token.scheduleIds.push(U256::ZERO));
        rejects_token(|token| token.amounts.push(U256::ZERO));
        rejects_token(|token| token.tokenURI.replace_range(..1, "x"));
        rejects_token(|token| token.tokenURI.replace_range(14..15, "A"));
    }

    #[test]
    fn reviewed_rehype_profile_rejects_every_route_and_fee_drift() {
        let fixture = production_fixture(include_str!(
            "../tests/fixtures/bankr-doppler-v2-direct-live-proof.json"
        ));
        let call = abi::createCall::abi_decode(&fixture.transaction.input).unwrap();
        let rejects_rehype = |mutate: fn(&mut abi::RehypeInitData)| {
            let mut candidate = call.clone();
            let mut init = decode_initializer_data(&candidate);
            let mut rehype = decode_rehype_data(&init);
            mutate(&mut rehype);
            replace_rehype_data(&mut init, &rehype);
            replace_initializer_data(&mut candidate, &init);
            assert!(!validate_bankr_create_calldata_for_observation(
                &candidate.abi_encode()
            ));
        };
        rejects_rehype(|rehype| rehype.numeraire = Address::with_last_byte(1));
        rejects_rehype(|rehype| rehype.buybackDst = Address::with_last_byte(2));
        rejects_rehype(|rehype| rehype.startFee = Default::default());
        rejects_rehype(|rehype| rehype.endFee = Default::default());
        rejects_rehype(|rehype| rehype.durationSeconds += 1);
        rejects_rehype(|rehype| rehype.startingTime = 1);
        rejects_rehype(|rehype| rehype.feeRoutingMode = 1);
        rejects_rehype(|rehype| {
            rehype.feeDistributionInfo.assetFeesToAssetBuybackWad = U256::from(1_u8)
        });
        rejects_rehype(|rehype| {
            rehype.feeDistributionInfo.assetFeesToNumeraireBuybackWad -= U256::from(1_u8)
        });
        rejects_rehype(|rehype| {
            rehype.feeDistributionInfo.assetFeesToBeneficiaryWad = U256::from(1_u8)
        });
        rejects_rehype(|rehype| rehype.feeDistributionInfo.assetFeesToLpWad = U256::from(1_u8));
        rejects_rehype(|rehype| {
            rehype.feeDistributionInfo.numeraireFeesToAssetBuybackWad = U256::from(1_u8)
        });
        rejects_rehype(|rehype| {
            rehype
                .feeDistributionInfo
                .numeraireFeesToNumeraireBuybackWad -= U256::from(1_u8)
        });
        rejects_rehype(|rehype| {
            rehype.feeDistributionInfo.numeraireFeesToBeneficiaryWad = U256::from(1_u8)
        });
        rejects_rehype(|rehype| rehype.feeDistributionInfo.numeraireFeesToLpWad = U256::from(1_u8));
    }

    #[test]
    fn real_v2_rotating_and_direct_receipts_require_exact_receipt_block_identity() {
        let profile = BankrDopplerExpectedProfile::production();
        let kernel = kernel_runtime();
        assert_eq!(kernel.len(), 24_469);
        assert_eq!(keccak256(&kernel), BANKR_KERNEL_RUNTIME_HASH);

        let mut erc = production_fixture(include_str!(
            "../tests/fixtures/bankr-doppler-v2-erc7579-live-proof.json"
        ));
        let direct = production_fixture(include_str!(
            "../tests/fixtures/bankr-doppler-v2-direct-live-proof.json"
        ));
        for fixture in [&erc, &direct] {
            let identity = &fixture.receipt_block_identity;
            assert_eq!(identity.l2_block_number, fixture.receipt.l2_block_number);
            assert_eq!(identity.l2_block_number, fixture.block.l2_block_number);
            assert_eq!(identity.block_hash, fixture.receipt.block_hash);
            assert_eq!(identity.block_hash, fixture.block.hash);
            assert_eq!(identity.account_designator_bytes, 23);
            assert_eq!(identity.account_designator.len(), 23);
            assert_eq!(
                keccak256(&identity.account_designator),
                identity.account_designator_hash
            );
            assert_eq!(
                identity.account_designator_hash,
                BANKR_ACCOUNT_DESIGNATOR_HASH
            );
            assert_eq!(
                identity.delegation_implementation,
                BANKR_KERNEL_IMPLEMENTATION
            );
            assert_eq!(identity.delegation_runtime_bytes, kernel.len());
            assert_eq!(identity.delegation_runtime_hash, keccak256(&kernel));
        }

        let erc_pin = verified_smart_account_from_receipt_code(
            erc.receipt_block_identity.leader,
            &erc.receipt_block_identity.account_designator,
            &kernel,
            profile,
        )
        .unwrap();
        let erc_quote = quote_bankr_doppler_launch_receipt_verified(
            &erc.transaction,
            &erc.receipt,
            &erc.block,
            profile,
            policy(),
            VerifiedBankrEnvelope::Erc7579 {
                smart_account: erc_pin,
            },
        )
        .unwrap();
        assert_eq!(
            erc_quote.market.create_profile_version,
            BankrCreateProfileVersion::CurveTicksV2
        );
        assert_eq!(erc_quote.market.envelope, BankrEnvelopeKind::Erc7579);
        assert_eq!(erc_quote.market.leader, erc.receipt_block_identity.leader);

        let direct_pin = verified_smart_account_from_receipt_code(
            direct.receipt_block_identity.leader,
            &direct.receipt_block_identity.account_designator,
            &kernel,
            profile,
        )
        .unwrap();
        let direct_quote = quote_bankr_doppler_launch_receipt_verified(
            &direct.transaction,
            &direct.receipt,
            &direct.block,
            profile,
            policy(),
            VerifiedBankrEnvelope::DirectAirlock {
                smart_account: direct_pin,
            },
        )
        .unwrap();
        assert_eq!(
            direct_quote.market.create_profile_version,
            BankrCreateProfileVersion::CurveTicksV2
        );
        assert_eq!(
            direct_quote.market.envelope,
            BankrEnvelopeKind::DirectAirlock
        );
        assert_eq!(
            direct_quote.market.leader,
            direct.receipt_block_identity.leader
        );

        let mut wrong_designator = erc.receipt_block_identity.account_designator.to_vec();
        wrong_designator[3] ^= 1;
        assert!(matches!(
            verified_smart_account_from_receipt_code(
                erc.receipt_block_identity.leader,
                &wrong_designator,
                &kernel,
                profile,
            ),
            Err(BankrQuoteError::ReceiptBlockIdentity(_))
        ));
        let mut wrong_kernel = kernel.clone();
        wrong_kernel[0] ^= 1;
        assert!(matches!(
            verified_smart_account_from_receipt_code(
                erc.receipt_block_identity.leader,
                &erc.receipt_block_identity.account_designator,
                &wrong_kernel,
                profile,
            ),
            Err(BankrQuoteError::ReceiptBlockIdentity(_))
        ));

        erc.receipt
            .logs
            .retain(|log| log.topics.first() != Some(&abi::UserOperationEvent::SIGNATURE_HASH));
        assert!(matches!(
            quote_bankr_doppler_launch_receipt_verified(
                &erc.transaction,
                &erc.receipt,
                &erc.block,
                profile,
                policy(),
                VerifiedBankrEnvelope::Erc7579 {
                    smart_account: erc_pin,
                },
            ),
            Err(BankrQuoteError::UserOperationEvidence)
        ));

        let mut direct_with_user_operation = direct;
        let user_operation = production_fixture(include_str!(
            "../tests/fixtures/bankr-doppler-v2-erc7579-live-proof.json"
        ))
        .receipt
        .logs
        .into_iter()
        .find(|log| log.topics.first() == Some(&abi::UserOperationEvent::SIGNATURE_HASH))
        .unwrap();
        direct_with_user_operation.receipt.logs.push(user_operation);
        assert!(matches!(
            quote_bankr_doppler_launch_receipt_verified(
                &direct_with_user_operation.transaction,
                &direct_with_user_operation.receipt,
                &direct_with_user_operation.block,
                profile,
                policy(),
                VerifiedBankrEnvelope::DirectAirlock {
                    smart_account: direct_pin,
                },
            ),
            Err(BankrQuoteError::UserOperationEvidence)
        ));
    }

    #[test]
    fn rotating_designator_and_direct_airlock_envelopes_quote_fail_closed() {
        let profile = BankrDopplerExpectedProfile::production();
        let mut rotating = live_fixture();
        let rotating_account =
            alloy_primitives::address!("6af697bf5bccadffb998e9785b880abf7861ebd1");
        let mut input = rotating.transaction.input.to_vec();
        let offsets = input
            .windows(Address::len_bytes())
            .enumerate()
            .filter_map(|(index, window)| {
                (window == BANKR_PROOF_ACCOUNT.as_slice()).then_some(index)
            })
            .collect::<Vec<_>>();
        assert_eq!(offsets.len(), 1);
        input[offsets[0]..offsets[0] + Address::len_bytes()]
            .copy_from_slice(rotating_account.as_slice());
        rotating.transaction.input = input.into();
        let user_operation = rotating
            .receipt
            .logs
            .iter_mut()
            .find(|log| log.topics.first() == Some(&abi::UserOperationEvent::SIGNATURE_HASH))
            .unwrap();
        user_operation.topics[2] = B256::left_padding_from(rotating_account.as_slice());
        let dynamic_pin = SmartAccountPin {
            account: ContractPin {
                address: rotating_account,
                runtime_code_hash: BANKR_ACCOUNT_DESIGNATOR_HASH,
            },
            ..profile.smart_account
        };
        let quote = quote_bankr_doppler_launch_receipt_verified(
            &rotating.transaction,
            &rotating.receipt,
            &rotating.block,
            profile,
            policy(),
            VerifiedBankrEnvelope::Erc7579 {
                smart_account: dynamic_pin,
            },
        )
        .unwrap();
        assert_eq!(quote.market.leader, rotating_account);
        assert_eq!(quote.market.envelope, BankrEnvelopeKind::Erc7579);

        let mut drifted_pin = dynamic_pin;
        drifted_pin.account.runtime_code_hash = B256::with_last_byte(1);
        assert!(matches!(
            quote_bankr_doppler_launch_receipt_verified(
                &rotating.transaction,
                &rotating.receipt,
                &rotating.block,
                profile,
                policy(),
                VerifiedBankrEnvelope::Erc7579 {
                    smart_account: drifted_pin,
                },
            ),
            Err(BankrQuoteError::SmartAccountIdentity)
        ));

        let mut direct = live_fixture();
        let direct_calldata = proof_create_calldata(&direct);
        direct.transaction.from = BANKR_PROOF_ACCOUNT;
        direct.transaction.to = Some(BANKR_AIRLOCK);
        direct.transaction.input = direct_calldata;
        direct
            .receipt
            .logs
            .retain(|log| log.topics.first() != Some(&abi::UserOperationEvent::SIGNATURE_HASH));
        let quote = quote_bankr_doppler_launch_receipt_verified(
            &direct.transaction,
            &direct.receipt,
            &direct.block,
            profile,
            policy(),
            VerifiedBankrEnvelope::DirectAirlock {
                smart_account: profile.smart_account,
            },
        )
        .unwrap();
        assert_eq!(quote.market.envelope, BankrEnvelopeKind::DirectAirlock);
        assert!(quote.market.outer_bundler.is_none());
        assert!(quote.market.user_operation_log_index.is_none());
        assert_eq!(
            quote.state_version.terminal_log_index,
            quote.market.launch_log_index
        );
    }

    #[test]
    fn fee_schedule_uses_reviewed_eight_hundred_thousand_denominator() {
        assert_eq!(
            bankr_hook_fee_ppm(1_000, 800_000, 5_000, 10, 1_000).unwrap(),
            800_000
        );
        assert_eq!(
            bankr_hook_fee_ppm(1_000, 800_000, 5_000, 10, 1_001).unwrap(),
            720_500
        );
        assert_eq!(
            bankr_hook_fee_ppm(1_000, 800_000, 5_000, 10, 1_010).unwrap(),
            5_000
        );
        let amount = U256::from(45_943_174_477_075_835_u64)
            .checked_mul(U256::from(1_000_000_000_u64))
            .unwrap()
            + U256::from(731_242_444_u64);
        assert_eq!(
            output_hook_fee(amount, 5_000, 800_000).unwrap(),
            amount * U256::from(5_000) / U256::from(800_000)
        );
    }

    #[test]
    fn exact_live_user_and_internal_buyback_swaps_match_poolmanager() {
        let token = alloy_primitives::address!("008587181a84cc22e25681ba7bb1b06455066ba3");
        let mut state = V3PoolState::new(
            V4_POOL_MANAGER,
            token,
            WETH,
            7_000,
            200,
            hex_u256("ad7cd543bfeac2d29af4"),
            -229_600,
            0,
        )
        .unwrap();
        state
            .add_position(-229_600, -119_400, 873_703_326_502_923_350_498_616)
            .unwrap();
        state
            .add_position(-119_400, 887_200, 2_171_753_334_568_235_763_678_399)
            .unwrap();

        let outer = state
            .quote_exact_input(WETH, U256::from(4_950_000_000_000_000_u64), None)
            .unwrap();
        assert_eq!(outer.amount_out, hex_u256("2600d87a9c39b20a06edcc"));
        assert_eq!(outer.sqrt_price_x96_after, hex_u256("ad94fefc2b3f8b21cdac"));
        assert_eq!(outer.tick_after, -229_590);
        assert_eq!(outer.liquidity_after, 873_703_326_502_923_350_498_616);

        let hook_fee = output_hook_fee(outer.amount_out, 5_000, 800_000).unwrap();
        assert_eq!(
            hook_fee,
            U256::from(287_144_840_481_723_973_u64) * U256::from(1_000_000_u64)
                + U256::from(320_265_u64)
        );
        let owner_fee = hook_fee * U256::from(500) / U256::from(10_000);
        assert_eq!(
            owner_fee,
            U256::from(14_357_242_024_086_198_u64) * U256::from(1_000_000_u64)
                + U256::from(666_013_u64)
        );
        let internal_input = hook_fee - owner_fee;
        assert_eq!(
            internal_input,
            U256::from(272_787_598_457_637_774_u64) * U256::from(1_000_000_u64)
                + U256::from(654_252_u64)
        );

        state
            .set_observation(
                outer.sqrt_price_x96_after,
                outer.tick_after,
                outer.liquidity_after,
            )
            .unwrap();
        let internal = state
            .quote_exact_input(token, internal_input, None)
            .unwrap();
        assert_eq!(internal.amount_out, U256::from(28_996_270_382_456_u64));
        assert_eq!(
            internal.sqrt_price_x96_after,
            hex_u256("ad94da7ea3f35a5db886")
        );
        assert_eq!(internal.tick_after, -229_590);
        assert_eq!(internal.liquidity_after, outer.liquidity_after);
    }

    #[test]
    fn live_bankr_proof_reconstructs_first_nonzero_entry_and_exit() {
        let fixture = live_fixture();
        let quote = quote_bankr_doppler_launch_receipt(
            &fixture.transaction,
            &fixture.receipt,
            &fixture.block,
            BankrDopplerExpectedProfile::production(),
            policy(),
        )
        .unwrap();
        assert_eq!(quote.tx_hash, fixture.transaction.hash);
        assert_eq!(quote.launchpad, LaunchpadId::BankrDoppler);
        assert_eq!(quote.market.leader, BANKR_PROOF_ACCOUNT);
        assert_eq!(quote.market.lp_fee_ppm, 7_000);
        assert_eq!(quote.market.position_count, 2);
        assert_eq!(quote.market.positions.len(), 2);
        assert_eq!(
            quote.market.positions[1].log_index,
            quote.market.last_liquidity_log_index
        );
        assert!(
            quote
                .market
                .positions
                .iter()
                .all(|position| position.pool_id == quote.market.pool_id
                    && position.sender == quote.market.initializer
                    && position.liquidity != U256::ZERO)
        );
        assert_eq!(quote.entry.hook_fee_ppm, 720_500);
        assert_eq!(quote.entry.hook_fee_denominator_ppm, 800_000);
        assert_eq!(
            quote.state_version.first_eligible_quote_timestamp,
            fixture.block.timestamp + 1
        );
        assert!(quote.entry.expected_output > U256::ZERO);
        assert!(quote.entry.expected_output < quote.entry.core_expected_output);
        assert!(quote.entry.internal_buyback_state_after.is_some());
        assert_eq!(
            quote.full_position_exit.amount_in,
            quote.entry.expected_output
        );
        assert!(quote.full_position_exit.expected_output > U256::ZERO);
        assert!(!quote.execution_eligible);
        assert!(!quote.broadcast);
    }

    #[test]
    fn wrong_account_event_module_and_schedule_fail_closed() {
        let mut fixture = live_fixture();
        let user_operation = fixture.receipt.logs.last_mut().unwrap();
        user_operation.topics[2] = B256::with_last_byte(1);
        assert!(matches!(
            quote_bankr_doppler_launch_receipt(
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                BankrDopplerExpectedProfile::production(),
                policy(),
            ),
            Err(BankrQuoteError::UserOperationEvidence)
        ));

        let mut fixture = live_fixture();
        let airlock = fixture
            .receipt
            .logs
            .iter_mut()
            .find(|log| log.address == BANKR_AIRLOCK)
            .unwrap();
        airlock.address = Address::with_last_byte(1);
        assert!(
            quote_bankr_doppler_launch_receipt(
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                BankrDopplerExpectedProfile::production(),
                policy(),
            )
            .is_err()
        );

        let mut fixture = live_fixture();
        let schedule = fixture
            .receipt
            .logs
            .iter_mut()
            .find(|log| log.address == BANKR_REHYPE_HOOK)
            .unwrap();
        let mut data = schedule.data.to_vec();
        data[61..64].copy_from_slice(&799_999_u32.to_be_bytes()[1..]);
        schedule.data = data.into();
        assert!(matches!(
            quote_bankr_doppler_launch_receipt(
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                BankrDopplerExpectedProfile::production(),
                policy(),
            ),
            Err(BankrQuoteError::FeeSchedule)
        ));
    }

    #[test]
    fn unpinned_inner_target_fails_closed_before_receipt_quoting() {
        let mut fixture = live_fixture();
        let mut input = fixture.transaction.input.to_vec();
        let offset = input
            .windows(Address::len_bytes())
            .position(|window| window == BANKR_AIRLOCK.as_slice())
            .unwrap();
        input[offset..offset + Address::len_bytes()].fill(0x11);
        fixture.transaction.input = input.into();
        assert!(matches!(
            quote_bankr_doppler_launch_receipt(
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                BankrDopplerExpectedProfile::production(),
                policy(),
            ),
            Err(BankrQuoteError::SmartAccountIdentity)
        ));
    }

    #[test]
    fn foreign_liquidity_pool_or_sender_fails_closed() {
        for topic_index in [1_usize, 2_usize] {
            let mut fixture = live_fixture();
            let liquidity = fixture
                .receipt
                .logs
                .iter_mut()
                .find(|log| log.topics.first() == Some(&abi::ModifyLiquidity::SIGNATURE_HASH))
                .unwrap();
            liquidity.topics[topic_index] = B256::with_last_byte(0xee);
            assert!(matches!(
                quote_bankr_doppler_launch_receipt(
                    &fixture.transaction,
                    &fixture.receipt,
                    &fixture.block,
                    BankrDopplerExpectedProfile::production(),
                    policy(),
                ),
                Err(BankrQuoteError::LiquiditySequence)
            ));
        }
    }

    #[test]
    fn matching_pool_swap_before_initialize_is_never_ignored() {
        let mut fixture = live_fixture();
        let quote = quote_bankr_doppler_launch_receipt(
            &fixture.transaction,
            &fixture.receipt,
            &fixture.block,
            BankrDopplerExpectedProfile::production(),
            policy(),
        )
        .unwrap();
        let differential: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-first-swap-differential.json"
        ))
        .unwrap();
        let mut swap: ReceiptLog = differential["first_swap_receipt"]["logs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|log| {
                log["topics"][0].as_str()
                    == Some("0x40e9cecb9f5f1f1c5b9c97dec2917b7ee92e57ba5563708daca94dd84ad7112f")
            })
            .cloned()
            .map(serde_json::from_value)
            .unwrap()
            .unwrap();
        swap.topics[1] = quote.market.pool_id;
        swap.log_index = 0;
        fixture.receipt.logs.insert(0, swap);
        assert!(matches!(
            quote_bankr_doppler_launch_receipt(
                &fixture.transaction,
                &fixture.receipt,
                &fixture.block,
                BankrDopplerExpectedProfile::production(),
                policy(),
            ),
            Err(BankrQuoteError::EmbeddedSwapUnsupported)
        ));
    }

    #[test]
    fn mutated_initial_tick_or_sqrt_price_fails_closed() {
        for data_index in [127_usize, 159_usize] {
            let mut fixture = live_fixture();
            let initialize = fixture
                .receipt
                .logs
                .iter_mut()
                .find(|log| log.topics.first() == Some(&abi::Initialize::SIGNATURE_HASH))
                .unwrap();
            let mut data = initialize.data.to_vec();
            data[data_index] ^= 1;
            initialize.data = data.into();
            assert!(matches!(
                quote_bankr_doppler_launch_receipt(
                    &fixture.transaction,
                    &fixture.receipt,
                    &fixture.block,
                    BankrDopplerExpectedProfile::production(),
                    policy(),
                ),
                Err(BankrQuoteError::InitializeIdentity)
            ));
        }
    }
}
