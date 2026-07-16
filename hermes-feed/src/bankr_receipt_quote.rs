//! Strict, receipt-end Bankr/Doppler V4 paper quotes.
//!
//! This module performs no RPC, signing, or execution. It accepts an already
//! confirmed transaction, receipt, and block; unwraps one startup-pinned
//! EntryPoint v0.7/EIP-7702 account; validates the exact reviewed Bankr
//! standard profile; reconstructs its V4 concentrated-liquidity state; and
//! simulates an independent tiny WETH entry plus immediate full-position exit.

use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::{SolCall, SolEvent, SolValue};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

use crate::launchpad_adapter::LaunchpadId;
use crate::noxa_abi::ReceiptLog;
use crate::noxa_rpc::{NoxaReceipt, RobinhoodBlock, RobinhoodTransaction};
use crate::robinhood::{CHAIN_ID, WETH, WETH_RUNTIME_KECCAK256};
use crate::smart_account::{
    AccountExecutionProfile, ContractPin, ENTRY_POINT_V07, EntryPointCall, SmartAccountPin,
    SmartAccountPins, decode_entry_point_v07,
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
pub const BANKR_GOVERNANCE_FACTORY: Address =
    alloy_primitives::address!("db036746d65dd52126b1915f1adf555e6c5237cf");
pub const BANKR_LIQUIDITY_MIGRATOR: Address =
    alloy_primitives::address!("ba2f330edb16cd8056f5988d8ce19bbc63475a0e");
pub const BANKR_PROTOCOL_BENEFICIARY: Address =
    alloy_primitives::address!("edeaa06e2eb42a5c19ce27c6cffb36fd4fe1eda8");
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BankrDopplerExpectedProfile {
    pub airlock: CodePin,
    pub pool_manager: CodePin,
    pub initializer: CodePin,
    pub rehype_hook: CodePin,
    pub token_factory: CodePin,
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
    pub leader: Address,
    pub outer_bundler: Address,
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
    pub initialize_tick: i32,
    pub initialize_log_index: u64,
    pub last_liquidity_log_index: u64,
    pub launch_log_index: u64,
    pub user_operation_log_index: u64,
    pub position_count: usize,
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
struct Position {
    pool_id: B256,
    sender: Address,
    tick_lower: i32,
    tick_upper: i32,
    liquidity: u128,
    salt: B256,
    log_index: u64,
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
    user_operation_log_index: u64,
    schedule_start: u64,
    buyback_destination: Address,
}

/// Reconstruct the reviewed Bankr standard launch and produce a non-broadcast
/// entry/full-exit quote from its first nonzero deterministic timestamp.
pub fn quote_bankr_doppler_launch_receipt(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
    profile: BankrDopplerExpectedProfile,
    policy: BankrDopplerQuotePolicy,
) -> Result<BankrDopplerReceiptPaperQuote, BankrQuoteError> {
    profile.validate()?;
    validate_envelope(transaction, receipt, block, policy)?;

    let accounts = [profile.smart_account];
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
    if unwrapped.leader != profile.smart_account.account.address
        || unwrapped.target != profile.airlock.address
        || unwrapped.value != U256::ZERO
        || unwrapped.execution_profile != AccountExecutionProfile::Erc7579SingleCall
        || unwrapped.delegation_implementation
            != profile
                .smart_account
                .delegation_implementation
                .map(|pin| pin.address)
    {
        return Err(BankrQuoteError::SmartAccountIdentity);
    }

    let create = abi::createCall::abi_decode(&unwrapped.calldata)
        .map_err(|_| BankrQuoteError::CreateCalldata)?;
    if create.abi_encode().as_slice() != unwrapped.calldata.as_ref() {
        return Err(BankrQuoteError::CreateCalldata);
    }
    let decoded = validate_create_calldata(&create, profile)?;
    let (evidence, positions) = validate_receipt(
        &receipt.logs,
        unwrapped.leader,
        &decoded,
        block.timestamp,
        profile,
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
        state.add_position(position.tick_lower, position.tick_upper, position.liquidity)?;
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
            terminal_log_index: evidence.user_operation_log_index,
            receipt_timestamp: block.timestamp,
            first_eligible_quote_timestamp,
        },
        quote_source: "confirmed_receipt_end_bankr_doppler_first_nonzero_state".into(),
        sizing_source: "independent_fixed_tiny_weth_policy".into(),
        market: BankrDopplerMarketEvidence {
            leader: unwrapped.leader,
            outer_bundler: unwrapped.outer_bundler,
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
            initialize_tick: evidence.initialize_tick,
            initialize_log_index: evidence.initialize_log_index,
            last_liquidity_log_index: evidence.last_liquidity_log_index,
            launch_log_index: evidence.launch_log_index,
            user_operation_log_index: evidence.user_operation_log_index,
            position_count: positions.len(),
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
}

fn validate_create_calldata(
    call: &abi::createCall,
    profile: BankrDopplerExpectedProfile,
) -> Result<DecodedCreate, BankrQuoteError> {
    let create = &call.createData;
    if create.initialSupply == U256::ZERO
        || create.numTokensToSell == U256::ZERO
        || create
            .numTokensToSell
            .checked_mul(U256::from(BPS_DENOMINATOR))
            .ok_or(BankrQuoteError::ArithmeticOverflow)?
            != create
                .initialSupply
                .checked_mul(U256::from(profile.pool_allocation_bps))
                .ok_or(BankrQuoteError::ArithmeticOverflow)?
        || create.numeraire != profile.weth.address
        || create.tokenFactory != profile.token_factory.address
        || create.governanceFactory != profile.governance_factory.address
        || create.poolInitializer != profile.initializer.address
        || create.liquidityMigrator != profile.liquidity_migrator.address
        || create.integrator == Address::ZERO
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
    if !curve(&init.curves[0], -229_800, -119_800, expected_primary_share)
        || !curve(&init.curves[1], -119_800, 887_200, expected_secondary_share)
    {
        return Err(BankrQuoteError::CreateCalldata);
    }
    let creator_share = WAD
        .checked_mul(U256::from(profile.creator_beneficiary_bps))
        .ok_or(BankrQuoteError::ArithmeticOverflow)?
        / U256::from(BPS_DENOMINATOR);
    let protocol_share = WAD
        .checked_mul(U256::from(profile.protocol_beneficiary_bps))
        .ok_or(BankrQuoteError::ArithmeticOverflow)?
        / U256::from(BPS_DENOMINATOR);
    if init.beneficiaries[0].beneficiary == Address::ZERO
        || init.beneficiaries[0].beneficiary >= init.beneficiaries[1].beneficiary
        || U256::from(init.beneficiaries[0].shares) != creator_share
        || init.beneficiaries[1].beneficiary != BANKR_PROTOCOL_BENEFICIARY
        || U256::from(init.beneficiaries[1].shares) != protocol_share
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
    Ok(DecodedCreate { init, rehype })
}

fn validate_receipt(
    logs: &[ReceiptLog],
    leader: Address,
    decoded: &DecodedCreate,
    block_timestamp: u64,
    profile: BankrDopplerExpectedProfile,
) -> Result<(LaunchEvidence, Vec<Position>), BankrQuoteError> {
    let mut airlock_create = None;
    let mut initializer_create = None;
    let mut initialize = None;
    let mut positions = Vec::new();
    let mut lock = None;
    let mut schedule = None;
    let mut user_operation = None;
    let mut pool_swaps = 0usize;

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
                positions.push(Position {
                    pool_id: event.id,
                    sender: event.sender,
                    tick_lower: i32::try_from(event.tickLower)
                        .map_err(|_| BankrQuoteError::LiquiditySequence)?,
                    tick_upper: i32::try_from(event.tickUpper)
                        .map_err(|_| BankrQuoteError::LiquiditySequence)?,
                    liquidity: u128::try_from(delta)
                        .map_err(|_| BankrQuoteError::LiquiditySequence)?,
                    salt: event.salt,
                    log_index: log.log_index,
                });
            } else if topic == abi::Swap::SIGNATURE_HASH {
                let event =
                    abi::Swap::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                        .map_err(|_| BankrQuoteError::EmbeddedSwapUnsupported)?;
                if initialize
                    .as_ref()
                    .is_some_and(|(init, _)| init.id == event.id)
                {
                    pool_swaps += 1;
                }
            }
        }
    }

    if pool_swaps != 0 {
        return Err(BankrQuoteError::EmbeddedSwapUnsupported);
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
    let (initialize, initialize_log_index) =
        initialize.ok_or(BankrQuoteError::InitializeIdentity)?;
    let fee = u32::try_from(initialize.fee).map_err(|_| BankrQuoteError::InitializeIdentity)?;
    let tick_spacing =
        i32::try_from(initialize.tickSpacing).map_err(|_| BankrQuoteError::InitializeIdentity)?;
    let initialize_tick =
        i32::try_from(initialize.tick).map_err(|_| BankrQuoteError::InitializeIdentity)?;
    let expected_initialize_tick = if token < WETH { -229_600 } else { 229_800 };
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
    let expected_ranges = if token < WETH {
        [
            (-229_600, -119_400, B256::ZERO),
            (-119_400, 887_200, B256::with_last_byte(1)),
        ]
    } else {
        [
            (119_800, 229_800, B256::ZERO),
            (-887_200, 119_800, B256::with_last_byte(1)),
        ]
    };
    for (position, expected) in positions.iter().zip(expected_ranges) {
        if position.pool_id != key.pool_id()
            || position.sender != profile.initializer.address
            || position.tick_lower != expected.0
            || position.tick_upper != expected.1
            || position.salt != expected.2
            || position.liquidity == 0
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
    let (user_operation, user_operation_log_index) =
        user_operation.ok_or(BankrQuoteError::UserOperationEvidence)?;
    if user_operation.sender != leader
        || user_operation.paymaster != Address::ZERO
        || !user_operation.success
    {
        return Err(BankrQuoteError::UserOperationEvidence);
    }
    if !(initialize_log_index < last_liquidity_log_index
        && last_liquidity_log_index < initializer_create_log_index
        && initializer_create_log_index < lock_log_index
        && lock_log_index < schedule_log_index
        && schedule_log_index < launch_log_index
        && launch_log_index < user_operation_log_index)
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
) -> Result<(), BankrQuoteError> {
    if !receipt.status
        || receipt.transaction_hash == B256::ZERO
        || receipt.block_hash == B256::ZERO
        || transaction.hash != receipt.transaction_hash
        || transaction.to != Some(ENTRY_POINT_V07)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct LiveFixture {
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
    }

    fn live_fixture() -> LiveFixture {
        serde_json::from_str(include_str!(
            "../tests/fixtures/bankr-doppler-live-proof.json"
        ))
        .unwrap()
    }

    fn policy() -> BankrDopplerQuotePolicy {
        BankrDopplerQuotePolicy {
            amount_in: U256::from(1_000_000_000_000_000_u64),
            max_amount_in: U256::from(10_000_000_000_000_000_u64),
            slippage_bps: 100,
        }
    }

    fn hex_u256(value: &str) -> U256 {
        U256::from_str_radix(value, 16).unwrap()
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
