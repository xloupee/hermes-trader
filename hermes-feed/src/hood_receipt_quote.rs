//! Strict receipt-end paper quotes for the current Hood bonding curve.
//!
//! This module is read-only and performs no RPC itself. The collector hydrates
//! an exact fixed-block [`HoodMarketSnapshot`] independently of the receipt and
//! passes it here for transaction/event/state reconciliation and an independent
//! fixed-size entry plus immediate full-position exit simulation.

use alloy_primitives::{Address, B256, I256, U256};
use alloy_sol_types::{SolCall, SolEvent};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::launchpad_adapter::{ActionKind, LaunchpadId};
use crate::noxa_abi::ReceiptLog;
use crate::noxa_rpc::{
    HoodMarketSnapshot, HoodV3PoolSnapshot, NoxaReceipt, RobinhoodBlock, RobinhoodTransaction,
};
use crate::robinhood::CHAIN_ID;
use crate::tier2_curve::{
    CurveAdapterError, CurveFormula, CurveState, HOOD_BUY_FOR_SELECTOR, HOOD_BUY_SELECTOR,
    HOOD_CREATE_SELECTOR, HOOD_FACTORY, HOOD_SELL_SELECTOR, HoodCurveBuyQuote, HoodCurveSellQuote,
    quote_hood_curve_buy, quote_hood_curve_sell,
};
use crate::v3_pool::{V3PoolState, V3Quote};

const BPS: u16 = 10_000;
pub const HOOD_MIGRATED_PAPER_ENTRY_WEI: u64 = 1_000_000_000_000_000;
pub const HOOD_MIGRATED_PAPER_MAX_ENTRY_WEI: u64 = 10_000_000_000_000_000;
pub const HOOD_MIGRATED_PAPER_SLIPPAGE_BPS: u16 = 100;
const TRANSFER_TOPIC: B256 =
    alloy_primitives::b256!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");

mod events {
    use alloy_sol_types::sol;

    sol! {
        event TokenCreated(
            address indexed token,
            address indexed creator,
            string name,
            string symbol,
            string metadataURI,
            uint256 virtualEth,
            uint256 virtualTokens,
            uint256 curveSupply
        );

        event Trade(
            address indexed token,
            address indexed trader,
            bool isBuy,
            uint256 ethAmount,
            uint256 tokenAmount,
            uint256 fee,
            uint256 virtualEthAfter,
            uint256 virtualTokensAfter
        );

        event Graduated(address indexed token, uint256 raisedEth);
        event Migrated(
            address indexed token,
            address indexed pair,
            uint256 ethLiquidity,
            uint256 tokenLiquidity,
            uint256 lpBurned
        );

        event PoolCreated(
            address indexed token0,
            address indexed token1,
            uint24 indexed fee,
            int24 tickSpacing,
            address pool
        );
        event Initialize(uint160 sqrtPriceX96, int24 tick);
        event Mint(
            address sender,
            address indexed owner,
            int24 indexed tickLower,
            int24 indexed tickUpper,
            uint128 amount,
            uint256 amount0,
            uint256 amount1
        );
        event Swap(
            address indexed sender,
            address indexed recipient,
            int256 amount0,
            int256 amount1,
            uint160 sqrtPriceX96,
            uint128 liquidity,
            int24 tick
        );
        event IncreaseLiquidity(
            uint256 indexed tokenId,
            uint128 liquidity,
            uint256 amount0,
            uint256 amount1
        );
        event Locked(
            uint256 indexed tokenId,
            address indexed creator,
            address indexed protocol,
            uint16 creatorShareBps
        );
        event V3Migrated(
            address indexed token,
            address indexed pool,
            uint256 tokenId,
            uint256 tokenLiquidity,
            uint256 ethLiquidity
        );

        function createToken(
            string name,
            string symbol,
            string metadataURI,
            uint256 minTokensOut,
            bytes32 salt,
            uint16 tradeFeeBps,
            uint256 totalSupply
        ) external payable returns (address token);
    }
}

pub const HOOD_TOKEN_CREATED_TOPIC: B256 = events::TokenCreated::SIGNATURE_HASH;
pub const HOOD_TRADE_TOPIC: B256 = events::Trade::SIGNATURE_HASH;
pub const HOOD_GRADUATED_TOPIC: B256 = events::Graduated::SIGNATURE_HASH;
pub const HOOD_MIGRATED_TOPIC: B256 = events::Migrated::SIGNATURE_HASH;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HoodIdentityRole {
    Factory,
    Migrator,
    Locker,
    PositionManager,
    V3Factory,
    SwapRouter,
    Weth,
    FallbackFactory,
    OwnerSafeProxy,
    OwnerSafeSingleton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodRuntimeIdentity {
    pub role: HoodIdentityRole,
    pub address: Address,
    pub code_bytes: usize,
    pub runtime_hash: B256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodExpectedProfile {
    pub identities: Vec<HoodRuntimeIdentity>,
    pub semantic: HoodSemanticProfile,
    pub owner: Address,
    pub pending_owner: Address,
    pub v3_fee: u32,
    pub v3_tick_spacing: i32,
    pub full_range_tick_lower: i32,
    pub full_range_tick_upper: i32,
    pub migrator_creator_share_bps: u16,
    pub locker_token_fee_burn_bps: u16,
    pub v3_pool_init_code_hash: B256,
}

impl HoodExpectedProfile {
    pub fn production() -> Self {
        use HoodIdentityRole::*;
        let identity = |role, address, code_bytes, runtime_hash| HoodRuntimeIdentity {
            role,
            address,
            code_bytes,
            runtime_hash,
        };
        Self {
            identities: vec![
                identity(
                    Factory,
                    HOOD_FACTORY,
                    20_518,
                    alloy_primitives::b256!(
                        "4aa0ce56b5b67d27f2fab59dcb796fa552d10ceafdecb06e088cdd254c92c0fc"
                    ),
                ),
                identity(
                    Migrator,
                    alloy_primitives::address!("5790ef23be2e1543442c12f4550fae147ba8edbe"),
                    3_577,
                    alloy_primitives::b256!(
                        "88b7c4f6dfb99df8493cf7b7905a538212fc1c7eb176ffbbcaade5a6988c83d6"
                    ),
                ),
                identity(
                    Locker,
                    alloy_primitives::address!("ad69d8a00564f4a2365cc74594925f95281706aa"),
                    3_583,
                    alloy_primitives::b256!(
                        "ee4522db997a71e396e90ef14a123f3a4a857268040b17a618ff2f47e204eb4a"
                    ),
                ),
                identity(
                    PositionManager,
                    alloy_primitives::address!("73991a25c818bf1f1128deaab1492d45638de0d3"),
                    24_384,
                    alloy_primitives::b256!(
                        "0a493d1af3d0f25fed8efa205244ebee14114267a08647fc38c515c7cd6ead4f"
                    ),
                ),
                identity(
                    V3Factory,
                    alloy_primitives::address!("1f7d7550b1b028f7571e69a784071f0205fd2efa"),
                    24_535,
                    alloy_primitives::b256!(
                        "ec72b1abd1f2faee020cfea9c646bd8994f9fb389054f6e574f103a895091739"
                    ),
                ),
                identity(
                    SwapRouter,
                    alloy_primitives::address!("caf681a66d020601342297493863e78c959e5cb2"),
                    24_497,
                    alloy_primitives::b256!(
                        "6f36c378e272c6324c48f045182bcb54bd8ad654cf9ebd42e8893d52c4cb25dc"
                    ),
                ),
                identity(
                    Weth,
                    alloy_primitives::address!("0bd7d308f8e1639fab988df18a8011f41eacad73"),
                    2_202,
                    alloy_primitives::b256!(
                        "5706be52f64875fee65a2cec0d80e47a23d8793cbe85d214b48445e2d05f5353"
                    ),
                ),
                identity(
                    FallbackFactory,
                    alloy_primitives::address!("8bceaa40b9acdfaedf85adf4ff01f5ad6517937f"),
                    13_859,
                    alloy_primitives::b256!(
                        "bab145d02e7005f0d84c6c1639d39b799b0ea16df99ebbdaf5a14d9da820b4e0"
                    ),
                ),
                identity(
                    OwnerSafeProxy,
                    alloy_primitives::address!("b3f3b54e11217f4f73e7a766b7caa187390d700d"),
                    171,
                    alloy_primitives::b256!(
                        "d7d408ebcd99b2b70be43e20253d6d92a8ea8fab29bd3be7f55b10032331fb4c"
                    ),
                ),
                identity(
                    OwnerSafeSingleton,
                    alloy_primitives::address!("29fcb43b46531bca003ddc8fcb67ffe91900c762"),
                    24_421,
                    alloy_primitives::b256!(
                        "b1f926978a0f44a2c0ec8fe822418ae969bd8c3f18d61e5103100339894f81ff"
                    ),
                ),
            ],
            semantic: HoodSemanticProfile::production(),
            owner: alloy_primitives::address!("b3f3b54e11217f4f73e7a766b7caa187390d700d"),
            pending_owner: Address::ZERO,
            v3_fee: 10_000,
            v3_tick_spacing: 200,
            full_range_tick_lower: -887_200,
            full_range_tick_upper: 887_200,
            migrator_creator_share_bps: 8_000,
            locker_token_fee_burn_bps: 8_000,
            v3_pool_init_code_hash: alloy_primitives::b256!(
                "e34f199b19b2b4f47f68442619d555527d244f78a3297ea89325f843f87b8b54"
            ),
        }
    }

    pub fn validate(&self) -> Result<(), HoodQuoteError> {
        use std::collections::HashSet;
        if self != &Self::production() {
            return Err(HoodQuoteError::ProfileMismatch);
        }
        let mut roles = HashSet::new();
        let mut addresses = HashSet::new();
        if self.identities.len() != 10
            || self.identities.iter().any(|identity| {
                identity.address == Address::ZERO
                    || identity.runtime_hash == B256::ZERO
                    || identity.code_bytes == 0
                    || !roles.insert(identity.role)
                    || !addresses.insert(identity.address)
            })
        {
            return Err(HoodQuoteError::ProfileMismatch);
        }
        Ok(())
    }

    pub fn identity(&self, role: HoodIdentityRole) -> Option<HoodRuntimeIdentity> {
        self.identities
            .iter()
            .copied()
            .find(|identity| identity.role == role)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodQuotePolicy {
    pub amount_in: U256,
    pub max_amount_in: U256,
    pub slippage_bps: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodSemanticProfile {
    pub factory: Address,
    pub weth: Address,
    pub fallback_factory: Address,
    pub active_migrator: Address,
    pub virtual_eth_seed: U256,
    pub creation_fee: U256,
    pub default_trade_fee_bps: u16,
    pub migration_fee: U256,
    pub migration_fee_bps: u16,
    pub guard_blocks: u16,
    pub current_guard_max_wallet_bps: u16,
    pub historical_guard_max_wallet_bps: u16,
    pub guard_disabled_l2_block: u64,
    pub guard_transition_tx_index: u64,
    pub fail_closed_on_guard_transition_block: bool,
    pub creator_fee_share_bps: u16,
    pub vanity_enforced: bool,
    pub curve_allocation_bps: u16,
    pub min_trade_fee_bps: u16,
    pub max_trade_fee_bps: u16,
}

impl HoodSemanticProfile {
    pub fn production() -> Self {
        Self {
            factory: HOOD_FACTORY,
            weth: alloy_primitives::address!("0bd7d308f8e1639fab988df18a8011f41eacad73"),
            fallback_factory: alloy_primitives::address!(
                "8bceaa40b9acdfaedf85adf4ff01f5ad6517937f"
            ),
            active_migrator: alloy_primitives::address!("5790ef23be2e1543442c12f4550fae147ba8edbe"),
            virtual_eth_seed: U256::from(2_810_000_000_000_000_000_u128),
            creation_fee: U256::ZERO,
            default_trade_fee_bps: 100,
            migration_fee: U256::from(50_000_000_000_000_000_u128),
            migration_fee_bps: 300,
            guard_blocks: 100,
            current_guard_max_wallet_bps: 0,
            historical_guard_max_wallet_bps: 1_000,
            guard_disabled_l2_block: 5_780_966,
            guard_transition_tx_index: 3,
            fail_closed_on_guard_transition_block: true,
            creator_fee_share_bps: 8_000,
            vanity_enforced: true,
            curve_allocation_bps: 8_000,
            min_trade_fee_bps: 100,
            max_trade_fee_bps: 500,
        }
    }

    pub fn validate_snapshot(self, snapshot: &HoodMarketSnapshot) -> Result<(), HoodQuoteError> {
        let config = snapshot.config;
        let total_supply = snapshot
            .token_curve_supply
            .checked_add(snapshot.token_lp_supply)
            .ok_or(HoodQuoteError::ProfileMismatch)?;
        let expected_curve_supply = total_supply
            .checked_mul(U256::from(self.curve_allocation_bps))
            .and_then(|value| value.checked_div(U256::from(BPS)))
            .ok_or(HoodQuoteError::ProfileMismatch)?;
        if snapshot.factory != self.factory
            || snapshot.weth != self.weth
            || snapshot.uniswap_factory != self.fallback_factory
            || snapshot.migrator != self.active_migrator
            || config.virtual_eth_seed != self.virtual_eth_seed
            || config.creation_fee != self.creation_fee
            || config.default_trade_fee_bps != self.default_trade_fee_bps
            || config.migration_fee != self.migration_fee
            || config.migration_fee_bps != self.migration_fee_bps
            || config.guard_blocks != self.guard_blocks
            || config.creator_fee_share_bps != self.creator_fee_share_bps
            || config.vanity_enforced != self.vanity_enforced
            || (self.fail_closed_on_guard_transition_block
                && snapshot.l2_block_number == self.guard_disabled_l2_block)
            || config.guard_max_wallet_bps
                != if snapshot.l2_block_number < self.guard_disabled_l2_block {
                    self.historical_guard_max_wallet_bps
                } else {
                    self.current_guard_max_wallet_bps
                }
            || !(self.min_trade_fee_bps..=self.max_trade_fee_bps)
                .contains(&snapshot.curve.trade_fee_bps)
            || total_supply == U256::ZERO
            || snapshot.token_curve_supply != expected_curve_supply
        {
            return Err(HoodQuoteError::ProfileMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodStateVersion {
    pub chain_id: u64,
    pub block_hash: B256,
    pub l2_block_number: u64,
    pub l1_block_number: u64,
    pub block_timestamp: u64,
    pub transaction_index: u64,
    pub terminal_log_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodObservedTradeEvidence {
    pub action: ActionKind,
    pub trader: Address,
    pub eth_amount: U256,
    pub token_amount: U256,
    pub fee: U256,
    pub virtual_eth_after: U256,
    pub virtual_tokens_after: U256,
    pub trade_log_index: Option<u64>,
    pub token_created_log_index: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodPaperEntryQuote {
    pub amount_in_requested: U256,
    pub amount_in_consumed: U256,
    pub refund: U256,
    pub fee: U256,
    pub expected_output: U256,
    pub min_receive: U256,
    pub slippage_bps: u16,
    pub state_after: CurveState,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodPaperExitQuote {
    pub amount_in: U256,
    pub gross_output: U256,
    pub fee: U256,
    pub expected_output: U256,
    pub min_receive: U256,
    pub slippage_bps: u16,
    pub state_after: CurveState,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodReceiptPaperQuote {
    pub record_type: String,
    pub tx_hash: B256,
    pub launchpad: LaunchpadId,
    pub token: Address,
    pub leader: Address,
    pub l2_block_number: u64,
    pub state_version: HoodStateVersion,
    pub quote_source: String,
    pub sizing_source: String,
    pub observed: HoodObservedTradeEvidence,
    pub token_curve_supply: U256,
    pub token_lp_supply: U256,
    pub receipt_end_curve: CurveState,
    pub entry: HoodPaperEntryQuote,
    pub full_position_exit: HoodPaperExitQuote,
    pub simulated_round_trip_return_bps: U256,
    pub execution_eligible: bool,
    pub execution_blocker: String,
    pub broadcast: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodMigrationEvidence {
    pub record_type: String,
    pub tx_hash: B256,
    pub launchpad: LaunchpadId,
    pub token: Address,
    pub pool: Address,
    pub leader: Address,
    pub trader: Address,
    pub l2_block_number: u64,
    pub block_hash: B256,
    pub transaction_index: u64,
    pub token_id: U256,
    pub raised_eth: U256,
    pub declared_eth_liquidity: U256,
    pub declared_token_liquidity: U256,
    pub actual_eth_liquidity: U256,
    pub actual_token_liquidity: U256,
    pub position_liquidity: U256,
    pub declared_and_actual_liquidity_match: bool,
    pub pool_initialize_sqrt_price_x96: U256,
    pub pool_initialize_tick: i32,
    pub reconstructed_boundary_sqrt_price_x96: U256,
    pub reconstructed_boundary_tick: i32,
    pub reconstructed_boundary_liquidity: U256,
    pub receipt_end_sqrt_price_x96: U256,
    pub receipt_end_tick: i32,
    pub receipt_end_liquidity: U256,
    pub receipt_end_swap_log_index: u64,
    pub receipt_end_swap_input: U256,
    pub receipt_end_swap_output: U256,
    pub log_order: HoodMigrationLogOrderEvidence,
    pub swap_amounts_reconstructed: bool,
    pub terminal_zero_liquidity_boundary_observed: bool,
    pub expected_profile_validated: bool,
    pub receipt_topology_verified: bool,
    pub pool_state_reconciled: bool,
    pub v3_quote_available: bool,
    pub execution_eligible: bool,
    pub execution_blocker: String,
    pub broadcast: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodMigratedV3PaperLeg {
    pub amount_in: U256,
    pub expected_output: U256,
    pub min_receive: U256,
    pub slippage_bps: u16,
    pub state_after: V3Quote,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodMigratedV3PaperQuote {
    pub record_type: String,
    pub tx_hash: B256,
    pub launchpad: LaunchpadId,
    pub token: Address,
    pub pool: Address,
    pub leader: Address,
    pub l2_block_number: u64,
    pub block_hash: B256,
    pub transaction_index: u64,
    pub quote_source: String,
    pub sizing_source: String,
    pub receipt_end_sqrt_price_x96: U256,
    pub receipt_end_tick: i32,
    pub receipt_end_liquidity: U256,
    pub position_liquidity: U256,
    pub tick_lower: i32,
    pub tick_upper: i32,
    pub migration: HoodMigrationEvidence,
    pub entry: HoodMigratedV3PaperLeg,
    pub full_position_exit: HoodMigratedV3PaperLeg,
    pub simulated_round_trip_return_bps: U256,
    pub execution_eligible: bool,
    pub execution_blocker: String,
    pub broadcast: bool,
}

/// Derive a fixed-size, receipt-confirmed Hood migrated-market paper entry and
/// state-chained full exit. The terminal receipt may sit above the full-range
/// position with zero active liquidity: WETH is token0, so a buy traverses the
/// empty range down to the initialized upper tick before consuming input.
pub fn quote_hood_migrated_v3_receipt(
    evidence: &HoodMigrationEvidence,
    profile: &HoodExpectedProfile,
) -> Result<HoodMigratedV3PaperQuote, HoodQuoteError> {
    profile.validate()?;
    if !validate_hood_migration_boundary_evidence(evidence, profile)
        || evidence.actual_eth_liquidity == U256::ZERO
        || evidence.actual_token_liquidity == U256::ZERO
    {
        return Err(HoodQuoteError::MigrationMismatch);
    }
    let weth = profile
        .identity(HoodIdentityRole::Weth)
        .ok_or(HoodQuoteError::ProfileMismatch)?
        .address;
    if weth >= evidence.token {
        return Err(HoodQuoteError::MigrationMismatch);
    }
    let position_liquidity = u128::try_from(evidence.position_liquidity)
        .ok()
        .filter(|amount| *amount != 0)
        .ok_or(HoodQuoteError::MigrationMismatch)?;
    let mut receipt_end = V3PoolState::new(
        evidence.pool,
        weth,
        evidence.token,
        profile.v3_fee,
        profile.v3_tick_spacing,
        evidence.receipt_end_sqrt_price_x96,
        evidence.receipt_end_tick,
        u128::try_from(evidence.receipt_end_liquidity)
            .map_err(|_| HoodQuoteError::MigrationMismatch)?,
    )
    .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    receipt_end
        .add_position(
            profile.full_range_tick_lower,
            profile.full_range_tick_upper,
            position_liquidity,
        )
        .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    let amount_in = U256::from(HOOD_MIGRATED_PAPER_ENTRY_WEI);
    if amount_in > U256::from(HOOD_MIGRATED_PAPER_MAX_ENTRY_WEI) {
        return Err(HoodQuoteError::InvalidEnvelope);
    }
    let entry = receipt_end
        .quote_exact_input(weth, amount_in, None)
        .map_err(|_| HoodQuoteError::IncompleteQuote)?;
    if entry.amount_in_consumed != amount_in
        || entry.amount_out == U256::ZERO
        || entry.initialized_ticks_crossed == 0
        || entry.liquidity_after != position_liquidity
    {
        return Err(HoodQuoteError::IncompleteQuote);
    }
    let mut post_entry = V3PoolState::new(
        evidence.pool,
        weth,
        evidence.token,
        profile.v3_fee,
        profile.v3_tick_spacing,
        evidence.receipt_end_sqrt_price_x96,
        evidence.receipt_end_tick,
        0,
    )
    .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    post_entry
        .add_position(
            profile.full_range_tick_lower,
            profile.full_range_tick_upper,
            position_liquidity,
        )
        .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    post_entry
        .set_observation(
            entry.sqrt_price_x96_after,
            entry.tick_after,
            entry.liquidity_after,
        )
        .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    let exit = post_entry
        .quote_exact_input(evidence.token, entry.amount_out, None)
        .map_err(|_| HoodQuoteError::IncompleteQuote)?;
    if exit.amount_in_consumed != entry.amount_out || exit.amount_out == U256::ZERO {
        return Err(HoodQuoteError::IncompleteQuote);
    }
    let apply_slippage = |amount: U256| {
        amount
            .checked_mul(U256::from(BPS - HOOD_MIGRATED_PAPER_SLIPPAGE_BPS))
            .and_then(|value| value.checked_div(U256::from(BPS)))
            .ok_or(HoodQuoteError::IncompleteQuote)
    };
    let round_trip = exit
        .amount_out
        .checked_mul(U256::from(BPS))
        .and_then(|value| value.checked_div(amount_in))
        .ok_or(HoodQuoteError::IncompleteQuote)?;
    Ok(HoodMigratedV3PaperQuote {
        record_type: "launchpad_hood_migrated_v3_paper_quote".into(),
        tx_hash: evidence.tx_hash,
        launchpad: LaunchpadId::HoodFun,
        token: evidence.token,
        pool: evidence.pool,
        leader: evidence.leader,
        l2_block_number: evidence.l2_block_number,
        block_hash: evidence.block_hash,
        transaction_index: evidence.transaction_index,
        quote_source: "confirmed_receipt_end_hood_migrated_v3_state".into(),
        sizing_source: "fixed_0_001_weth_policy_capped_at_0_01_weth".into(),
        receipt_end_sqrt_price_x96: evidence.receipt_end_sqrt_price_x96,
        receipt_end_tick: evidence.receipt_end_tick,
        receipt_end_liquidity: evidence.receipt_end_liquidity,
        position_liquidity: U256::from(position_liquidity),
        tick_lower: profile.full_range_tick_lower,
        tick_upper: profile.full_range_tick_upper,
        migration: evidence.clone(),
        entry: HoodMigratedV3PaperLeg {
            amount_in,
            expected_output: entry.amount_out,
            min_receive: apply_slippage(entry.amount_out)?,
            slippage_bps: HOOD_MIGRATED_PAPER_SLIPPAGE_BPS,
            state_after: entry,
        },
        full_position_exit: HoodMigratedV3PaperLeg {
            amount_in: exit.amount_in_requested,
            expected_output: exit.amount_out,
            min_receive: apply_slippage(exit.amount_out)?,
            slippage_bps: HOOD_MIGRATED_PAPER_SLIPPAGE_BPS,
            state_after: exit,
        },
        simulated_round_trip_return_bps: round_trip,
        execution_eligible: false,
        execution_blocker: "paper_only_no_signer_or_broadcast_capability".into(),
        broadcast: false,
    })
}

pub fn validate_hood_migrated_v3_pool_snapshot(
    evidence: &HoodMigrationEvidence,
    snapshot: &HoodV3PoolSnapshot,
    profile: &HoodExpectedProfile,
) -> bool {
    if profile.validate().is_err() {
        return false;
    }
    let Some(weth) = profile
        .identity(HoodIdentityRole::Weth)
        .map(|identity| identity.address)
    else {
        return false;
    };
    snapshot.pool == evidence.pool
        && snapshot.factory_pool == evidence.pool
        && snapshot.token0 == weth.min(evidence.token)
        && snapshot.token1 == weth.max(evidence.token)
        && snapshot.fee == profile.v3_fee
        && snapshot.tick_spacing == profile.v3_tick_spacing
        && snapshot.sqrt_price_x96 == evidence.receipt_end_sqrt_price_x96
        && snapshot.tick == evidence.receipt_end_tick
        && U256::from(snapshot.liquidity) == evidence.receipt_end_liquidity
        && snapshot.code_bytes != 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodMigrationLogOrderEvidence {
    pub pool_created: u64,
    pub initialize: u64,
    pub trade: u64,
    pub graduated: u64,
    pub curve_transfer: u64,
    pub lp_transfer: u64,
    pub weth_mint: u64,
    pub funding0: u64,
    pub funding1: u64,
    pub mint: u64,
    pub nft_mint: u64,
    pub increase_liquidity: u64,
    pub nft_lock: u64,
    pub locked: u64,
    pub v3_migrated: u64,
    pub migrated: u64,
    pub swap: u64,
}

impl HoodMigrationLogOrderEvidence {
    pub fn ordered_indices(self) -> [u64; 17] {
        [
            self.pool_created,
            self.initialize,
            self.trade,
            self.graduated,
            self.curve_transfer,
            self.lp_transfer,
            self.weth_mint,
            self.funding0,
            self.funding1,
            self.mint,
            self.nft_mint,
            self.increase_liquidity,
            self.nft_lock,
            self.locked,
            self.v3_migrated,
            self.migrated,
            self.swap,
        ]
    }
}

pub fn validate_hood_migration_boundary_evidence(
    evidence: &HoodMigrationEvidence,
    profile: &HoodExpectedProfile,
) -> bool {
    use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

    let Ok(expected_boundary_sqrt_price_x96) =
        get_sqrt_ratio_at_tick(profile.full_range_tick_upper)
    else {
        return false;
    };
    let order = evidence.log_order.ordered_indices();
    evidence.block_hash != B256::ZERO
        && evidence.reconstructed_boundary_sqrt_price_x96 == expected_boundary_sqrt_price_x96
        && evidence.reconstructed_boundary_tick == profile.full_range_tick_upper
        && evidence.reconstructed_boundary_liquidity == U256::ZERO
        && evidence.receipt_end_liquidity == U256::ZERO
        && evidence.receipt_end_tick > evidence.reconstructed_boundary_tick
        && evidence.receipt_end_sqrt_price_x96 > evidence.reconstructed_boundary_sqrt_price_x96
        && evidence.receipt_end_swap_log_index == evidence.log_order.swap
        && order.windows(2).all(|pair| pair[0] < pair[1])
        && evidence.terminal_zero_liquidity_boundary_observed
        && !evidence.pool_state_reconciled
        && !evidence.v3_quote_available
        && !evidence.execution_eligible
        && !evidence.broadcast
}

#[derive(Debug, Error)]
pub enum HoodQuoteError {
    #[error("transaction, receipt, block, state snapshot, or policy envelope is invalid")]
    InvalidEnvelope,
    #[error("Hood expected semantic profile does not match fixed-block state")]
    ProfileMismatch,
    #[error("receipt logs are unordered, duplicated, or contain inconsistent Hood events")]
    EventIdentity,
    #[error("direct Hood calldata does not match the receipt event")]
    CalldataMismatch,
    #[error("receipt token transfer does not match the Hood trade event")]
    TransferMismatch,
    #[error("receipt event and fixed-block curve state do not form an exact transition")]
    StateMismatch,
    #[error("migrated Hood receipts require the separately pinned V3 quote path")]
    MigratedUnsupported,
    #[error("independent entry or exit quote is incomplete")]
    IncompleteQuote,
    #[error("Hood graduation or migration receipt topology is inconsistent")]
    MigrationMismatch,
    #[error(transparent)]
    Curve(#[from] CurveAdapterError),
}

#[derive(Debug, Clone, Copy)]
struct TradeEvidence {
    token: Address,
    trader: Address,
    is_buy: bool,
    eth_amount: U256,
    token_amount: U256,
    fee: U256,
    virtual_eth_after: U256,
    virtual_tokens_after: U256,
    log_index: u64,
}

#[derive(Debug, Clone, Copy)]
struct CreatedEvidence {
    token: Address,
    creator: Address,
    virtual_eth: U256,
    virtual_tokens: U256,
    curve_supply: U256,
    log_index: u64,
}

/// Reconcile one direct, non-migrated Hood receipt against an independently
/// hydrated fixed-block state snapshot and emit a real two-leg paper quote.
pub fn quote_hood_curve_receipt(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
    snapshot: &HoodMarketSnapshot,
    profile: HoodSemanticProfile,
    policy: HoodQuotePolicy,
) -> Result<HoodReceiptPaperQuote, HoodQuoteError> {
    validate_envelope(transaction, receipt, block, snapshot, policy)?;
    profile.validate_snapshot(snapshot)?;
    if snapshot.curve.graduated || snapshot.curve.migrated {
        return Err(HoodQuoteError::MigratedUnsupported);
    }
    let created = exact_created(&receipt.logs)?;
    let trade = exact_trade(&receipt.logs)?;
    let token = created
        .map(|event| event.token)
        .or_else(|| trade.map(|event| event.token))
        .ok_or(HoodQuoteError::EventIdentity)?;
    if token != snapshot.token
        || created.is_some_and(|event| trade.is_some_and(|trade| event.token != trade.token))
    {
        return Err(HoodQuoteError::EventIdentity);
    }

    let (observed, receipt_end_curve) = if let Some(created) = created {
        reconcile_launch(
            transaction,
            snapshot,
            created,
            trade,
            &receipt.logs,
            profile,
        )?
    } else {
        reconcile_direct_trade(
            transaction,
            snapshot,
            trade.ok_or(HoodQuoteError::EventIdentity)?,
            &receipt.logs,
        )?
    };
    validate_receipt_end_snapshot(receipt_end_curve, snapshot, profile)?;
    let entry = quote_hood_curve_buy(receipt_end_curve, policy.amount_in)?;
    if entry.graduates || entry.amount_in_consumed != policy.amount_in || entry.refund != U256::ZERO
    {
        return Err(HoodQuoteError::IncompleteQuote);
    }
    let exit = quote_hood_curve_sell(entry.state_after, entry.amount_out)?;
    let entry_min = apply_slippage(entry.amount_out, policy.slippage_bps)?;
    let exit_min = apply_slippage(exit.amount_out, policy.slippage_bps)?;
    let round_trip_bps = exit
        .amount_out
        .checked_mul(U256::from(BPS))
        .ok_or(HoodQuoteError::IncompleteQuote)?
        / policy.amount_in;
    let terminal_log_index = receipt
        .logs
        .last()
        .map(|log| log.log_index)
        .ok_or(HoodQuoteError::InvalidEnvelope)?;

    Ok(HoodReceiptPaperQuote {
        record_type: "launchpad_hood_curve_paper_quote".into(),
        tx_hash: transaction.hash,
        launchpad: LaunchpadId::HoodFun,
        token,
        leader: transaction.from,
        l2_block_number: receipt.l2_block_number,
        state_version: HoodStateVersion {
            chain_id: CHAIN_ID,
            block_hash: block.hash,
            l2_block_number: block.l2_block_number,
            l1_block_number: block.l1_block_number,
            block_timestamp: block.timestamp,
            transaction_index: receipt.transaction_index,
            terminal_log_index,
        },
        quote_source: "confirmed_receipt_and_fixed_block_hood_curve_state".into(),
        sizing_source: "independent_fixed_wei_policy_not_leader_amount".into(),
        observed,
        token_curve_supply: snapshot.token_curve_supply,
        token_lp_supply: snapshot.token_lp_supply,
        receipt_end_curve,
        entry: entry_record(entry, entry_min, policy.slippage_bps),
        full_position_exit: exit_record(exit, exit_min, policy.slippage_bps),
        simulated_round_trip_return_bps: round_trip_bps,
        execution_eligible: false,
        execution_blocker: "paper_only_no_signer_or_broadcast_capability".into(),
        broadcast: false,
    })
}

/// Verify the pinned Hood-to-V3 migration topology without treating the
/// migrated pool as quote-ready. A separate fixed-block V3 pool/position
/// snapshot is required before any migrated-market paper quote can exist.
pub fn verify_hood_graduation_receipt(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
    pre: &HoodMarketSnapshot,
    post: &HoodMarketSnapshot,
    profile: &HoodExpectedProfile,
) -> Result<HoodMigrationEvidence, HoodQuoteError> {
    profile.validate()?;
    if !receipt.status
        || transaction.hash == B256::ZERO
        || transaction.hash != receipt.transaction_hash
        || transaction.l2_block_number != Some(receipt.l2_block_number)
        || transaction.transaction_index != Some(receipt.transaction_index)
        || receipt.block_hash != block.hash
        || receipt.l2_block_number != block.l2_block_number
        || receipt.l1_block_number != Some(block.l1_block_number)
        || pre.factory != HOOD_FACTORY
        || post.factory != HOOD_FACTORY
        || pre.token == Address::ZERO
        || pre.token != post.token
        || pre.l2_block_number.checked_add(1) != Some(post.l2_block_number)
        || post.l2_block_number != receipt.l2_block_number
        || pre.curve.graduated
        || pre.curve.migrated
        || !post.curve.graduated
        || !post.curve.migrated
        || post.curve.real_eth != U256::ZERO
        || post.curve.real_tokens != U256::ZERO
        || pre.token_curve_supply != post.token_curve_supply
        || pre.token_lp_supply != post.token_lp_supply
    {
        return Err(HoodQuoteError::MigrationMismatch);
    }
    profile.semantic.validate_snapshot(pre)?;
    profile.semantic.validate_snapshot(post)?;

    use HoodIdentityRole::{Locker, Migrator, PositionManager, V3Factory, Weth};
    let migrator = profile
        .identity(Migrator)
        .ok_or(HoodQuoteError::MigrationMismatch)?
        .address;
    let locker = profile
        .identity(Locker)
        .ok_or(HoodQuoteError::MigrationMismatch)?
        .address;
    let position_manager = profile
        .identity(PositionManager)
        .ok_or(HoodQuoteError::MigrationMismatch)?
        .address;
    let v3_factory = profile
        .identity(V3Factory)
        .ok_or(HoodQuoteError::MigrationMismatch)?
        .address;
    let weth = profile
        .identity(Weth)
        .ok_or(HoodQuoteError::MigrationMismatch)?
        .address;

    let (token0, token1) = if weth < pre.token {
        (weth, pre.token)
    } else {
        (pre.token, weth)
    };
    let pool_created_log =
        exact_pool_created_log(&receipt.logs, v3_factory, token0, token1, profile.v3_fee)?;
    let pool_created = events::PoolCreated::decode_raw_log_validate(
        pool_created_log.topics.iter().copied(),
        &pool_created_log.data,
    )
    .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    let expected_pool = crate::noxa_predict::predict_v3_pool_address(
        v3_factory,
        token0,
        token1,
        profile.v3_fee,
        profile.v3_pool_init_code_hash,
    );
    if pool_created.token0 != token0
        || pool_created.token1 != token1
        || u32::try_from(pool_created.fee).ok() != Some(profile.v3_fee)
        || i32::try_from(pool_created.tickSpacing).ok() != Some(profile.v3_tick_spacing)
        || pool_created.pool != expected_pool
    {
        return Err(HoodQuoteError::MigrationMismatch);
    }
    let pool = pool_created.pool;
    let initialize_log =
        exact_migration_log(&receipt.logs, pool, events::Initialize::SIGNATURE_HASH)?;
    let initialize = events::Initialize::decode_raw_log_validate(
        initialize_log.topics.iter().copied(),
        &initialize_log.data,
    )
    .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    let initialize_tick =
        i32::try_from(initialize.tick).map_err(|_| HoodQuoteError::MigrationMismatch)?;

    let trade_log =
        exact_token_migration_log(&receipt.logs, HOOD_FACTORY, HOOD_TRADE_TOPIC, pre.token)?;
    let trade = exact_trade_for_token(&receipt.logs, pre.token)?;
    let graduated_log =
        exact_token_migration_log(&receipt.logs, HOOD_FACTORY, HOOD_GRADUATED_TOPIC, pre.token)?;
    let graduated = events::Graduated::decode_raw_log_validate(
        graduated_log.topics.iter().copied(),
        &graduated_log.data,
    )
    .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    let v3_migrated_log = exact_token_migration_log(
        &receipt.logs,
        migrator,
        events::V3Migrated::SIGNATURE_HASH,
        pre.token,
    )?;
    let v3_migrated = events::V3Migrated::decode_raw_log_validate(
        v3_migrated_log.topics.iter().copied(),
        &v3_migrated_log.data,
    )
    .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    let migrated_log =
        exact_token_migration_log(&receipt.logs, HOOD_FACTORY, HOOD_MIGRATED_TOPIC, pre.token)?;
    let migrated = events::Migrated::decode_raw_log_validate(
        migrated_log.topics.iter().copied(),
        &migrated_log.data,
    )
    .map_err(|_| HoodQuoteError::MigrationMismatch)?;

    let raised_eth = post
        .curve
        .virtual_eth
        .checked_sub(profile.semantic.virtual_eth_seed)
        .ok_or(HoodQuoteError::MigrationMismatch)?;
    let migration_protocol_fee = raised_eth
        .checked_mul(U256::from(profile.semantic.migration_fee_bps))
        .and_then(|value| value.checked_div(U256::from(BPS)))
        .ok_or(HoodQuoteError::MigrationMismatch)?;
    let expected_eth_liquidity = raised_eth
        .checked_sub(profile.semantic.migration_fee)
        .and_then(|value| value.checked_sub(migration_protocol_fee))
        .ok_or(HoodQuoteError::MigrationMismatch)?;
    let pre_curve = CurveState {
        formula: CurveFormula::HoodConstantProductFeeOnInputV1,
        virtual_quote_reserve: pre.curve.virtual_eth,
        virtual_token_reserve: pre.curve.virtual_tokens,
        remaining_curve_tokens: pre.curve.real_tokens,
        fee_bps: pre.curve.trade_fee_bps,
    };
    validate_receipt_end_snapshot(pre_curve, pre, profile.semantic)
        .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    let graduation_quote = quote_hood_curve_buy(pre_curve, trade.eth_amount)
        .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    if !trade.is_buy
        || trade.token != pre.token
        || !graduation_quote.graduates
        || graduation_quote.amount_in_consumed != trade.eth_amount
        || graduation_quote.amount_out != trade.token_amount
        || graduation_quote.fee != trade.fee
        || graduation_quote.state_after.virtual_quote_reserve != post.curve.virtual_eth
        || graduation_quote.state_after.virtual_token_reserve != post.curve.virtual_tokens
        || graduation_quote.state_after.remaining_curve_tokens != U256::ZERO
        || trade.virtual_eth_after != post.curve.virtual_eth
        || trade.virtual_tokens_after != post.curve.virtual_tokens
        || pre.curve.creator != post.curve.creator
        || pre.curve.created_at_block != post.curve.created_at_block
        || pre.curve.trade_fee_bps != post.curve.trade_fee_bps
        || graduated.token != pre.token
        || graduated.raisedEth != raised_eth
        || v3_migrated.token != pre.token
        || v3_migrated.pool != pool
        || migrated.token != pre.token
        || migrated.pair != pool
        || v3_migrated.ethLiquidity != expected_eth_liquidity
        || migrated.ethLiquidity != expected_eth_liquidity
        || v3_migrated.tokenLiquidity != post.token_lp_supply
        || migrated.tokenLiquidity != post.token_lp_supply
        || migrated.lpBurned != U256::ZERO
    {
        return Err(HoodQuoteError::MigrationMismatch);
    }

    let mint_log = exact_migration_log(&receipt.logs, pool, events::Mint::SIGNATURE_HASH)?;
    let mint =
        events::Mint::decode_raw_log_validate(mint_log.topics.iter().copied(), &mint_log.data)
            .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    let swap_log = exact_migration_log(&receipt.logs, pool, events::Swap::SIGNATURE_HASH)?;
    let swap =
        events::Swap::decode_raw_log_validate(swap_log.topics.iter().copied(), &swap_log.data)
            .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    let swap_tick = i32::try_from(swap.tick).map_err(|_| HoodQuoteError::MigrationMismatch)?;
    if receipt
        .logs
        .iter()
        .filter(|log| log.address == pool)
        .count()
        != 3
    {
        return Err(HoodQuoteError::MigrationMismatch);
    }
    let increase_log = exact_u256_topic_migration_log(
        &receipt.logs,
        position_manager,
        events::IncreaseLiquidity::SIGNATURE_HASH,
        1,
        v3_migrated.tokenId,
    )?;
    let increase = events::IncreaseLiquidity::decode_raw_log_validate(
        increase_log.topics.iter().copied(),
        &increase_log.data,
    )
    .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    let locked_log = exact_u256_topic_migration_log(
        &receipt.logs,
        locker,
        events::Locked::SIGNATURE_HASH,
        1,
        v3_migrated.tokenId,
    )?;
    let locked = events::Locked::decode_raw_log_validate(
        locked_log.topics.iter().copied(),
        &locked_log.data,
    )
    .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    if mint.sender != position_manager
        || mint.owner != position_manager
        || i32::try_from(mint.tickLower).ok() != Some(profile.full_range_tick_lower)
        || i32::try_from(mint.tickUpper).ok() != Some(profile.full_range_tick_upper)
        || mint.amount != increase.liquidity
        || mint.amount == 0
        || mint.amount0 == U256::ZERO
        || mint.amount1 == U256::ZERO
        || mint.amount0 != increase.amount0
        || mint.amount1 != increase.amount1
        || increase.tokenId != v3_migrated.tokenId
        || v3_migrated.tokenId == U256::ZERO
        || locked.tokenId != v3_migrated.tokenId
        || locked.creator != pre.curve.creator
        || locked.protocol != profile.owner
        || locked.creatorShareBps != profile.migrator_creator_share_bps
    {
        return Err(HoodQuoteError::MigrationMismatch);
    }

    let mut receipt_end_pool = V3PoolState::new(
        pool,
        token0,
        token1,
        profile.v3_fee,
        profile.v3_tick_spacing,
        U256::from(initialize.sqrtPriceX96),
        initialize_tick,
        0,
    )
    .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    receipt_end_pool
        .add_position(
            profile.full_range_tick_lower,
            profile.full_range_tick_upper,
            mint.amount,
        )
        .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    let (swap_input_token, swap_input, swap_output) =
        exact_v3_swap_amounts(token0, token1, swap.amount0, swap.amount1)?;
    let reconstructed_swap = receipt_end_pool
        .quote_exact_input(swap_input_token, swap_input, None)
        .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    if swap.sender != trade.trader
        || swap.recipient != trade.trader
        || reconstructed_swap.amount_in_consumed != swap_input
        || reconstructed_swap.amount_out != swap_output
        || reconstructed_swap.liquidity_after != 0
        || swap.liquidity != 0
        || reconstructed_swap.tick_after != profile.full_range_tick_upper
        || swap_tick <= profile.full_range_tick_upper
        || U256::from(swap.sqrtPriceX96) <= reconstructed_swap.sqrt_price_x96_after
    {
        return Err(HoodQuoteError::MigrationMismatch);
    }

    let curve_transfer_log = exact_transfer_log(
        &receipt.logs,
        pre.token,
        HOOD_FACTORY,
        trade.trader,
        trade.token_amount,
    )?;
    let lp_transfer_log = exact_transfer_log(
        &receipt.logs,
        pre.token,
        HOOD_FACTORY,
        migrator,
        post.token_lp_supply,
    )?;
    let weth_mint_log = exact_transfer_log(
        &receipt.logs,
        weth,
        Address::ZERO,
        migrator,
        expected_eth_liquidity,
    )?;
    let weth_funding_log = exact_transfer_log(
        &receipt.logs,
        weth,
        migrator,
        pool,
        if token0 == weth {
            mint.amount0
        } else {
            mint.amount1
        },
    )?;
    let token_funding_log = exact_transfer_log(
        &receipt.logs,
        pre.token,
        migrator,
        pool,
        if token0 == pre.token {
            mint.amount0
        } else {
            mint.amount1
        },
    )?;
    let nft_mint_log = exact_erc721_transfer_log(
        &receipt.logs,
        position_manager,
        Address::ZERO,
        migrator,
        v3_migrated.tokenId,
    )?;
    let nft_lock_log = exact_erc721_transfer_log(
        &receipt.logs,
        position_manager,
        migrator,
        locker,
        v3_migrated.tokenId,
    )?;
    let (funding0_log, funding1_log) = if token0 == weth {
        (weth_funding_log, token_funding_log)
    } else {
        (token_funding_log, weth_funding_log)
    };

    let order = [
        pool_created_log.log_index,
        initialize_log.log_index,
        trade_log.log_index,
        graduated_log.log_index,
        curve_transfer_log.log_index,
        lp_transfer_log.log_index,
        weth_mint_log.log_index,
        funding0_log.log_index,
        funding1_log.log_index,
        mint_log.log_index,
        nft_mint_log.log_index,
        increase_log.log_index,
        nft_lock_log.log_index,
        locked_log.log_index,
        v3_migrated_log.log_index,
        migrated_log.log_index,
        swap_log.log_index,
    ];
    if order.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(HoodQuoteError::MigrationMismatch);
    }

    let actual_eth_liquidity = if token0 == weth {
        mint.amount0
    } else {
        mint.amount1
    };
    let actual_token_liquidity = if token0 == pre.token {
        mint.amount0
    } else {
        mint.amount1
    };
    let declared_and_actual_liquidity_match = actual_eth_liquidity == expected_eth_liquidity
        && actual_token_liquidity == post.token_lp_supply;
    Ok(HoodMigrationEvidence {
        record_type: "launchpad_hood_migration_evidence".into(),
        tx_hash: transaction.hash,
        launchpad: LaunchpadId::HoodFun,
        token: pre.token,
        pool,
        leader: transaction.from,
        trader: trade.trader,
        l2_block_number: receipt.l2_block_number,
        block_hash: receipt.block_hash,
        transaction_index: receipt.transaction_index,
        token_id: v3_migrated.tokenId,
        raised_eth,
        declared_eth_liquidity: expected_eth_liquidity,
        declared_token_liquidity: post.token_lp_supply,
        actual_eth_liquidity,
        actual_token_liquidity,
        position_liquidity: U256::from(mint.amount),
        declared_and_actual_liquidity_match,
        pool_initialize_sqrt_price_x96: U256::from(initialize.sqrtPriceX96),
        pool_initialize_tick: initialize_tick,
        reconstructed_boundary_sqrt_price_x96: reconstructed_swap.sqrt_price_x96_after,
        reconstructed_boundary_tick: reconstructed_swap.tick_after,
        reconstructed_boundary_liquidity: U256::from(reconstructed_swap.liquidity_after),
        receipt_end_sqrt_price_x96: U256::from(swap.sqrtPriceX96),
        receipt_end_tick: swap_tick,
        receipt_end_liquidity: U256::from(swap.liquidity),
        receipt_end_swap_log_index: swap_log.log_index,
        receipt_end_swap_input: swap_input,
        receipt_end_swap_output: swap_output,
        log_order: HoodMigrationLogOrderEvidence {
            pool_created: pool_created_log.log_index,
            initialize: initialize_log.log_index,
            trade: trade_log.log_index,
            graduated: graduated_log.log_index,
            curve_transfer: curve_transfer_log.log_index,
            lp_transfer: lp_transfer_log.log_index,
            weth_mint: weth_mint_log.log_index,
            funding0: funding0_log.log_index,
            funding1: funding1_log.log_index,
            mint: mint_log.log_index,
            nft_mint: nft_mint_log.log_index,
            increase_liquidity: increase_log.log_index,
            nft_lock: nft_lock_log.log_index,
            locked: locked_log.log_index,
            v3_migrated: v3_migrated_log.log_index,
            migrated: migrated_log.log_index,
            swap: swap_log.log_index,
        },
        swap_amounts_reconstructed: true,
        terminal_zero_liquidity_boundary_observed: true,
        expected_profile_validated: true,
        receipt_topology_verified: true,
        pool_state_reconciled: false,
        v3_quote_available: false,
        execution_eligible: false,
        execution_blocker: if declared_and_actual_liquidity_match {
            "terminal_zero_liquidity_boundary_unreconciled_quote_blocked".into()
        } else {
            "declared_actual_liquidity_mismatch_and_terminal_boundary_unreconciled".into()
        },
        broadcast: false,
    })
}

fn exact_v3_swap_amounts(
    token0: Address,
    token1: Address,
    amount0: I256,
    amount1: I256,
) -> Result<(Address, U256, U256), HoodQuoteError> {
    if amount0.is_positive() && amount1.is_negative() {
        Ok((token0, amount0.into_raw(), amount1.unsigned_abs()))
    } else if amount1.is_positive() && amount0.is_negative() {
        Ok((token1, amount1.into_raw(), amount0.unsigned_abs()))
    } else {
        Err(HoodQuoteError::MigrationMismatch)
    }
}

fn exact_migration_log(
    logs: &[ReceiptLog],
    address: Address,
    topic: B256,
) -> Result<&ReceiptLog, HoodQuoteError> {
    let mut matching = logs
        .iter()
        .filter(|log| log.address == address && log.topics.first() == Some(&topic));
    let log = matching.next().ok_or(HoodQuoteError::MigrationMismatch)?;
    if matching.next().is_some() {
        return Err(HoodQuoteError::MigrationMismatch);
    }
    Ok(log)
}

fn exact_token_migration_log(
    logs: &[ReceiptLog],
    address: Address,
    topic: B256,
    token: Address,
) -> Result<&ReceiptLog, HoodQuoteError> {
    let mut matching = logs.iter().filter(|log| {
        log.address == address
            && log.topics.first() == Some(&topic)
            && topic_address(log, 1) == Some(token)
    });
    let log = matching.next().ok_or(HoodQuoteError::MigrationMismatch)?;
    if matching.next().is_some() {
        return Err(HoodQuoteError::MigrationMismatch);
    }
    Ok(log)
}

fn exact_u256_topic_migration_log(
    logs: &[ReceiptLog],
    address: Address,
    topic: B256,
    topic_index: usize,
    value: U256,
) -> Result<&ReceiptLog, HoodQuoteError> {
    let expected = B256::from(value.to_be_bytes::<32>());
    let mut matching = logs.iter().filter(|log| {
        log.address == address
            && log.topics.first() == Some(&topic)
            && log.topics.get(topic_index) == Some(&expected)
    });
    let log = matching.next().ok_or(HoodQuoteError::MigrationMismatch)?;
    if matching.next().is_some() {
        return Err(HoodQuoteError::MigrationMismatch);
    }
    Ok(log)
}

fn exact_pool_created_log(
    logs: &[ReceiptLog],
    factory: Address,
    token0: Address,
    token1: Address,
    fee: u32,
) -> Result<&ReceiptLog, HoodQuoteError> {
    let matching = logs
        .iter()
        .filter(|log| {
            if log.address != factory
                || log.topics.first() != Some(&events::PoolCreated::SIGNATURE_HASH)
            {
                return false;
            }
            events::PoolCreated::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                .is_ok_and(|event| {
                    event.token0 == token0
                        && event.token1 == token1
                        && u32::try_from(event.fee).ok() == Some(fee)
                })
        })
        .collect::<Vec<_>>();
    match matching.as_slice() {
        [log] => Ok(log),
        _ => Err(HoodQuoteError::MigrationMismatch),
    }
}

fn exact_trade_for_token(
    logs: &[ReceiptLog],
    token: Address,
) -> Result<TradeEvidence, HoodQuoteError> {
    let matching = logs
        .iter()
        .filter(|log| {
            log.address == HOOD_FACTORY
                && log.topics.first() == Some(&HOOD_TRADE_TOPIC)
                && topic_address(log, 1) == Some(token)
        })
        .collect::<Vec<_>>();
    let [log] = matching.as_slice() else {
        return Err(HoodQuoteError::MigrationMismatch);
    };
    let event = events::Trade::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
        .map_err(|_| HoodQuoteError::MigrationMismatch)?;
    Ok(TradeEvidence {
        token: event.token,
        trader: event.trader,
        is_buy: event.isBuy,
        eth_amount: event.ethAmount,
        token_amount: event.tokenAmount,
        fee: event.fee,
        virtual_eth_after: event.virtualEthAfter,
        virtual_tokens_after: event.virtualTokensAfter,
        log_index: log.log_index,
    })
}

fn validate_envelope(
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
    snapshot: &HoodMarketSnapshot,
    policy: HoodQuotePolicy,
) -> Result<(), HoodQuoteError> {
    if !receipt.status
        || transaction.hash == B256::ZERO
        || transaction.hash != receipt.transaction_hash
        || transaction.to != Some(HOOD_FACTORY)
        || transaction.l2_block_number != Some(receipt.l2_block_number)
        || transaction.transaction_index != Some(receipt.transaction_index)
        || receipt.block_hash != block.hash
        || receipt.l2_block_number != block.l2_block_number
        || receipt.l1_block_number != Some(block.l1_block_number)
        || snapshot.l2_block_number != receipt.l2_block_number
        || snapshot.token == Address::ZERO
        || policy.amount_in == U256::ZERO
        || policy.amount_in > policy.max_amount_in
        || policy.slippage_bps >= BPS
        || receipt.logs.is_empty()
        || receipt
            .logs
            .windows(2)
            .any(|pair| pair[0].log_index >= pair[1].log_index)
    {
        return Err(HoodQuoteError::InvalidEnvelope);
    }
    Ok(())
}

fn reconcile_launch(
    transaction: &RobinhoodTransaction,
    snapshot: &HoodMarketSnapshot,
    created: CreatedEvidence,
    trade: Option<TradeEvidence>,
    logs: &[ReceiptLog],
    profile: HoodSemanticProfile,
) -> Result<(HoodObservedTradeEvidence, CurveState), HoodQuoteError> {
    let call = events::createTokenCall::abi_decode(&transaction.input)
        .map_err(|_| HoodQuoteError::CalldataMismatch)?;
    if transaction.input.get(..4) != Some(HOOD_CREATE_SELECTOR.as_slice())
        || created.creator != transaction.from
        || created.virtual_eth != profile.virtual_eth_seed
        || created.curve_supply != snapshot.token_curve_supply
        || call.tradeFeeBps != snapshot.curve.trade_fee_bps
        || call
            .totalSupply
            .checked_mul(U256::from(profile.curve_allocation_bps))
            .and_then(|value| value.checked_div(U256::from(BPS)))
            != Some(created.curve_supply)
        || snapshot.curve.creator != created.creator
        || snapshot.token_lp_supply == U256::ZERO
        || created
            .curve_supply
            .checked_mul(U256::from(BPS))
            .and_then(|value| value.checked_div(U256::from(profile.curve_allocation_bps)))
            .and_then(|total| total.checked_sub(created.curve_supply))
            != Some(snapshot.token_lp_supply)
    {
        return Err(HoodQuoteError::StateMismatch);
    }
    let initial = CurveState {
        formula: CurveFormula::HoodConstantProductFeeOnInputV1,
        virtual_quote_reserve: created.virtual_eth,
        virtual_token_reserve: created.virtual_tokens,
        remaining_curve_tokens: created.curve_supply,
        fee_bps: snapshot.curve.trade_fee_bps,
    };
    let buy_amount = transaction
        .value
        .checked_sub(snapshot.config.creation_fee)
        .ok_or(HoodQuoteError::CalldataMismatch)?;
    if buy_amount != U256::ZERO && trade.is_some_and(|trade| trade.token_amount < call.minTokensOut)
    {
        return Err(HoodQuoteError::CalldataMismatch);
    }
    match (buy_amount == U256::ZERO, trade) {
        (true, None) => Ok((
            HoodObservedTradeEvidence {
                action: ActionKind::Launch,
                trader: transaction.from,
                eth_amount: U256::ZERO,
                token_amount: U256::ZERO,
                fee: U256::ZERO,
                virtual_eth_after: initial.virtual_quote_reserve,
                virtual_tokens_after: initial.virtual_token_reserve,
                trade_log_index: None,
                token_created_log_index: Some(created.log_index),
            },
            initial,
        )),
        (false, Some(trade))
            if trade.is_buy
                && trade.trader == created.creator
                && created.log_index < trade.log_index =>
        {
            let quote = quote_hood_curve_buy(initial, buy_amount)?;
            validate_buy_transition(quote.state_after, trade, quote, logs)?;
            Ok((
                HoodObservedTradeEvidence {
                    action: ActionKind::Launch,
                    trader: trade.trader,
                    eth_amount: trade.eth_amount,
                    token_amount: trade.token_amount,
                    fee: trade.fee,
                    virtual_eth_after: trade.virtual_eth_after,
                    virtual_tokens_after: trade.virtual_tokens_after,
                    trade_log_index: Some(trade.log_index),
                    token_created_log_index: Some(created.log_index),
                },
                quote.state_after,
            ))
        }
        _ => Err(HoodQuoteError::EventIdentity),
    }
}

fn reconcile_direct_trade(
    transaction: &RobinhoodTransaction,
    snapshot: &HoodMarketSnapshot,
    trade: TradeEvidence,
    logs: &[ReceiptLog],
) -> Result<(HoodObservedTradeEvidence, CurveState), HoodQuoteError> {
    if trade.token != snapshot.token {
        return Err(HoodQuoteError::CalldataMismatch);
    }
    if trade.is_buy {
        let (words, expected_trader, minimum_index) =
            if transaction.input.get(..4) == Some(HOOD_BUY_SELECTOR.as_slice()) {
                (
                    exact_static_words(&transaction.input, HOOD_BUY_SELECTOR, 2)?,
                    transaction.from,
                    1,
                )
            } else if transaction.input.get(..4) == Some(HOOD_BUY_FOR_SELECTOR.as_slice()) {
                let words = exact_static_words(&transaction.input, HOOD_BUY_FOR_SELECTOR, 3)?;
                let recipient = address_word(words[1]).ok_or(HoodQuoteError::CalldataMismatch)?;
                if recipient == Address::ZERO {
                    return Err(HoodQuoteError::CalldataMismatch);
                }
                (words, recipient, 2)
            } else {
                return Err(HoodQuoteError::CalldataMismatch);
            };
        if address_word(words[0]) != Some(trade.token)
            || trade.trader != expected_trader
            || transaction.value == U256::ZERO
            || trade.token_amount < words[minimum_index]
        {
            return Err(HoodQuoteError::CalldataMismatch);
        }
        let post = event_curve_state(snapshot, trade)?;
        let pre = CurveState {
            formula: CurveFormula::HoodConstantProductFeeOnInputV1,
            virtual_quote_reserve: trade
                .virtual_eth_after
                .checked_sub(
                    trade
                        .eth_amount
                        .checked_sub(trade.fee)
                        .ok_or(HoodQuoteError::StateMismatch)?,
                )
                .ok_or(HoodQuoteError::StateMismatch)?,
            virtual_token_reserve: trade
                .virtual_tokens_after
                .checked_add(trade.token_amount)
                .ok_or(HoodQuoteError::StateMismatch)?,
            remaining_curve_tokens: post
                .remaining_curve_tokens
                .checked_add(trade.token_amount)
                .ok_or(HoodQuoteError::StateMismatch)?,
            fee_bps: snapshot.curve.trade_fee_bps,
        };
        let quote = quote_hood_curve_buy(pre, transaction.value)?;
        validate_buy_transition(post, trade, quote, logs)?;
        Ok((
            HoodObservedTradeEvidence {
                action: ActionKind::Buy,
                trader: trade.trader,
                eth_amount: trade.eth_amount,
                token_amount: trade.token_amount,
                fee: trade.fee,
                virtual_eth_after: trade.virtual_eth_after,
                virtual_tokens_after: trade.virtual_tokens_after,
                trade_log_index: Some(trade.log_index),
                token_created_log_index: None,
            },
            post,
        ))
    } else {
        let words = exact_static_words(&transaction.input, HOOD_SELL_SELECTOR, 3)?;
        if trade.trader != transaction.from
            || address_word(words[0]) != Some(trade.token)
            || words[1] != trade.token_amount
            || trade.eth_amount < words[2]
            || transaction.value != U256::ZERO
        {
            return Err(HoodQuoteError::CalldataMismatch);
        }
        let gross = trade
            .eth_amount
            .checked_add(trade.fee)
            .ok_or(HoodQuoteError::StateMismatch)?;
        let post = event_curve_state(snapshot, trade)?;
        let pre = CurveState {
            formula: CurveFormula::HoodConstantProductFeeOnInputV1,
            virtual_quote_reserve: trade
                .virtual_eth_after
                .checked_add(gross)
                .ok_or(HoodQuoteError::StateMismatch)?,
            virtual_token_reserve: trade
                .virtual_tokens_after
                .checked_sub(trade.token_amount)
                .ok_or(HoodQuoteError::StateMismatch)?,
            remaining_curve_tokens: post
                .remaining_curve_tokens
                .checked_sub(trade.token_amount)
                .ok_or(HoodQuoteError::StateMismatch)?,
            fee_bps: snapshot.curve.trade_fee_bps,
        };
        let quote = quote_hood_curve_sell(pre, trade.token_amount)?;
        if quote.amount_out != trade.eth_amount
            || quote.fee != trade.fee
            || quote.state_after != post
            || !exact_transfer(
                logs,
                trade.token,
                trade.trader,
                HOOD_FACTORY,
                trade.token_amount,
            )
        {
            return Err(HoodQuoteError::StateMismatch);
        }
        Ok((
            HoodObservedTradeEvidence {
                action: ActionKind::Sell,
                trader: trade.trader,
                eth_amount: trade.eth_amount,
                token_amount: trade.token_amount,
                fee: trade.fee,
                virtual_eth_after: trade.virtual_eth_after,
                virtual_tokens_after: trade.virtual_tokens_after,
                trade_log_index: Some(trade.log_index),
                token_created_log_index: None,
            },
            post,
        ))
    }
}

fn validate_receipt_end_snapshot(
    receipt_end: CurveState,
    snapshot: &HoodMarketSnapshot,
    profile: HoodSemanticProfile,
) -> Result<(), HoodQuoteError> {
    let expected_real_eth = receipt_end
        .virtual_quote_reserve
        .checked_sub(profile.virtual_eth_seed)
        .ok_or(HoodQuoteError::StateMismatch)?;
    if snapshot.curve.virtual_eth != receipt_end.virtual_quote_reserve
        || snapshot.curve.virtual_tokens != receipt_end.virtual_token_reserve
        || snapshot.curve.real_eth != expected_real_eth
        || snapshot.curve.real_tokens != receipt_end.remaining_curve_tokens
        || snapshot.curve.trade_fee_bps != receipt_end.fee_bps
        || snapshot.curve.graduated
        || snapshot.curve.migrated
    {
        return Err(HoodQuoteError::StateMismatch);
    }
    Ok(())
}

fn validate_buy_transition(
    expected_post: CurveState,
    trade: TradeEvidence,
    quote: HoodCurveBuyQuote,
    logs: &[ReceiptLog],
) -> Result<(), HoodQuoteError> {
    if quote.amount_in_consumed != trade.eth_amount
        || quote.amount_out != trade.token_amount
        || quote.fee != trade.fee
        || quote.state_after != expected_post
        || trade.virtual_eth_after != expected_post.virtual_quote_reserve
        || trade.virtual_tokens_after != expected_post.virtual_token_reserve
        || !exact_transfer(
            logs,
            trade.token,
            HOOD_FACTORY,
            trade.trader,
            trade.token_amount,
        )
    {
        return Err(HoodQuoteError::StateMismatch);
    }
    Ok(())
}

fn exact_created(logs: &[ReceiptLog]) -> Result<Option<CreatedEvidence>, HoodQuoteError> {
    let matching = logs
        .iter()
        .filter(|log| {
            log.address == HOOD_FACTORY && log.topics.first() == Some(&HOOD_TOKEN_CREATED_TOPIC)
        })
        .collect::<Vec<_>>();
    if matching.len() > 1 {
        return Err(HoodQuoteError::EventIdentity);
    }
    matching
        .first()
        .map(|log| {
            let event = events::TokenCreated::decode_raw_log_validate(
                log.topics.iter().copied(),
                &log.data,
            )
            .map_err(|_| HoodQuoteError::EventIdentity)?;
            Ok(CreatedEvidence {
                token: event.token,
                creator: event.creator,
                virtual_eth: event.virtualEth,
                virtual_tokens: event.virtualTokens,
                curve_supply: event.curveSupply,
                log_index: log.log_index,
            })
        })
        .transpose()
}

fn exact_trade(logs: &[ReceiptLog]) -> Result<Option<TradeEvidence>, HoodQuoteError> {
    let matching = logs
        .iter()
        .filter(|log| log.address == HOOD_FACTORY && log.topics.first() == Some(&HOOD_TRADE_TOPIC))
        .collect::<Vec<_>>();
    if matching.len() > 1 {
        return Err(HoodQuoteError::EventIdentity);
    }
    matching
        .first()
        .map(|log| {
            let event =
                events::Trade::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                    .map_err(|_| HoodQuoteError::EventIdentity)?;
            Ok(TradeEvidence {
                token: event.token,
                trader: event.trader,
                is_buy: event.isBuy,
                eth_amount: event.ethAmount,
                token_amount: event.tokenAmount,
                fee: event.fee,
                virtual_eth_after: event.virtualEthAfter,
                virtual_tokens_after: event.virtualTokensAfter,
                log_index: log.log_index,
            })
        })
        .transpose()
}

fn event_curve_state(
    snapshot: &HoodMarketSnapshot,
    trade: TradeEvidence,
) -> Result<CurveState, HoodQuoteError> {
    let total_supply = snapshot
        .token_curve_supply
        .checked_add(snapshot.token_lp_supply)
        .ok_or(HoodQuoteError::StateMismatch)?;
    let default_supply = U256::from(1_000_000_000_u64)
        .checked_mul(U256::from(1_000_000_000_000_000_000_u64))
        .ok_or(HoodQuoteError::StateMismatch)?;
    let default_virtual_tokens = U256::from(1_145_000_000_u64)
        .checked_mul(U256::from(1_000_000_000_000_000_000_u64))
        .ok_or(HoodQuoteError::StateMismatch)?;
    let virtual_seed = default_virtual_tokens
        .checked_mul(total_supply)
        .and_then(|value| value.checked_div(default_supply))
        .ok_or(HoodQuoteError::StateMismatch)?;
    let virtual_real_offset = virtual_seed
        .checked_sub(snapshot.token_curve_supply)
        .ok_or(HoodQuoteError::StateMismatch)?;
    let remaining = trade
        .virtual_tokens_after
        .checked_sub(virtual_real_offset)
        .ok_or(HoodQuoteError::StateMismatch)?;
    Ok(CurveState {
        formula: CurveFormula::HoodConstantProductFeeOnInputV1,
        virtual_quote_reserve: trade.virtual_eth_after,
        virtual_token_reserve: trade.virtual_tokens_after,
        remaining_curve_tokens: remaining,
        fee_bps: snapshot.curve.trade_fee_bps,
    })
}

fn exact_static_words(
    input: &[u8],
    selector: [u8; 4],
    count: usize,
) -> Result<Vec<U256>, HoodQuoteError> {
    if input.len() != 4 + count * 32 || input.get(..4) != Some(selector.as_slice()) {
        return Err(HoodQuoteError::CalldataMismatch);
    }
    Ok(input[4..]
        .chunks_exact(32)
        .map(U256::from_be_slice)
        .collect())
}

fn address_word(word: U256) -> Option<Address> {
    if word >> 160 != U256::ZERO {
        return None;
    }
    let bytes = word.to_be_bytes::<32>();
    Some(Address::from_slice(&bytes[12..]))
}

fn exact_transfer(
    logs: &[ReceiptLog],
    token: Address,
    from: Address,
    to: Address,
    amount: U256,
) -> bool {
    exact_transfer_log(logs, token, from, to, amount).is_ok()
}

fn exact_transfer_log(
    logs: &[ReceiptLog],
    token: Address,
    from: Address,
    to: Address,
    amount: U256,
) -> Result<&ReceiptLog, HoodQuoteError> {
    let mut matching = logs.iter().filter(|log| {
        log.address == token
            && log.topics.first() == Some(&TRANSFER_TOPIC)
            && topic_address(log, 1) == Some(from)
            && topic_address(log, 2) == Some(to)
            && (log.data.len() == 32 && U256::from_be_slice(&log.data) == amount)
    });
    let log = matching.next().ok_or(HoodQuoteError::MigrationMismatch)?;
    if matching.next().is_some() {
        return Err(HoodQuoteError::MigrationMismatch);
    }
    Ok(log)
}

fn exact_erc721_transfer_log(
    logs: &[ReceiptLog],
    collection: Address,
    from: Address,
    to: Address,
    token_id: U256,
) -> Result<&ReceiptLog, HoodQuoteError> {
    let mut matching = logs.iter().filter(|log| {
        log.address == collection
            && log.topics.first() == Some(&TRANSFER_TOPIC)
            && log.topics.len() == 4
            && topic_address(log, 1) == Some(from)
            && topic_address(log, 2) == Some(to)
            && U256::from_be_slice(log.topics[3].as_slice()) == token_id
            && log.data.is_empty()
    });
    let log = matching.next().ok_or(HoodQuoteError::MigrationMismatch)?;
    if matching.next().is_some() {
        return Err(HoodQuoteError::MigrationMismatch);
    }
    Ok(log)
}

fn topic_address(log: &ReceiptLog, index: usize) -> Option<Address> {
    let topic = log.topics.get(index)?;
    if topic.as_slice()[..12].iter().any(|byte| *byte != 0) {
        return None;
    }
    Some(Address::from_slice(&topic.as_slice()[12..]))
}

fn apply_slippage(amount: U256, slippage_bps: u16) -> Result<U256, HoodQuoteError> {
    let minimum = amount
        .checked_mul(U256::from(BPS - slippage_bps))
        .ok_or(HoodQuoteError::IncompleteQuote)?
        / U256::from(BPS);
    if minimum == U256::ZERO || minimum > amount {
        return Err(HoodQuoteError::IncompleteQuote);
    }
    Ok(minimum)
}

fn entry_record(quote: HoodCurveBuyQuote, minimum: U256, slippage_bps: u16) -> HoodPaperEntryQuote {
    HoodPaperEntryQuote {
        amount_in_requested: quote.amount_in_requested,
        amount_in_consumed: quote.amount_in_consumed,
        refund: quote.refund,
        fee: quote.fee,
        expected_output: quote.amount_out,
        min_receive: minimum,
        slippage_bps,
        state_after: quote.state_after,
    }
}

fn exit_record(quote: HoodCurveSellQuote, minimum: U256, slippage_bps: u16) -> HoodPaperExitQuote {
    HoodPaperExitQuote {
        amount_in: quote.amount_in,
        gross_output: quote.gross_output,
        fee: quote.fee,
        expected_output: quote.amount_out,
        min_receive: minimum,
        slippage_bps,
        state_after: quote.state_after,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::noxa_rpc::{HoodConfigSnapshot, HoodCurveStateSnapshot};

    #[derive(Debug, Deserialize)]
    struct LiveFixture {
        transaction: RobinhoodTransaction,
        block: RobinhoodBlock,
        receipt: NoxaReceipt,
        state: FixtureState,
    }

    #[derive(Debug, Deserialize)]
    struct FixtureState {
        token: Address,
        pre_block: u64,
        pre_curve: FixtureCurve,
        post_block: u64,
        post_curve: FixtureCurve,
        config: HoodConfigSnapshot,
        token_curve_supply: U256,
        token_lp_supply: U256,
        factory: Address,
    }

    #[derive(Debug, Clone, Deserialize)]
    struct FixtureCurve {
        virtual_eth: U256,
        virtual_tokens: U256,
        real_eth: U256,
        real_tokens: U256,
        creator: Address,
        created_at_block: U256,
        graduated: bool,
        migrated: bool,
        trade_fee_bps: u16,
    }

    impl LiveFixture {
        fn snapshot(&self) -> HoodMarketSnapshot {
            self.snapshot_at(self.state.post_block, &self.state.post_curve)
        }

        fn pre_snapshot(&self) -> HoodMarketSnapshot {
            self.snapshot_at(self.state.pre_block, &self.state.pre_curve)
        }

        fn snapshot_at(&self, l2_block_number: u64, curve: &FixtureCurve) -> HoodMarketSnapshot {
            HoodMarketSnapshot {
                factory: self.state.factory,
                token: self.state.token,
                l2_block_number,
                curve: HoodCurveStateSnapshot {
                    virtual_eth: curve.virtual_eth,
                    virtual_tokens: curve.virtual_tokens,
                    real_eth: curve.real_eth,
                    real_tokens: curve.real_tokens,
                    creator: curve.creator,
                    created_at_block: u64::try_from(curve.created_at_block).unwrap(),
                    graduated: curve.graduated,
                    migrated: curve.migrated,
                    trade_fee_bps: curve.trade_fee_bps,
                },
                config: self.state.config,
                token_curve_supply: self.state.token_curve_supply,
                token_lp_supply: self.state.token_lp_supply,
                migrator: HoodSemanticProfile::production().active_migrator,
                uniswap_factory: HoodSemanticProfile::production().fallback_factory,
                weth: HoodSemanticProfile::production().weth,
            }
        }
    }

    fn fixture(contents: &str) -> LiveFixture {
        serde_json::from_str(contents).unwrap()
    }

    fn policy() -> HoodQuotePolicy {
        HoodQuotePolicy {
            amount_in: U256::from(1_000_000_000_000_000_u64),
            max_amount_in: U256::from(10_000_000_000_000_000_u64),
            slippage_bps: 100,
        }
    }

    fn quote(value: &LiveFixture) -> Result<HoodReceiptPaperQuote, HoodQuoteError> {
        quote_hood_curve_receipt(
            &value.transaction,
            &value.receipt,
            &value.block,
            &value.snapshot(),
            HoodSemanticProfile::production(),
            policy(),
        )
    }

    #[test]
    fn live_launch_atomic_buy_reconciles_and_emits_two_leg_quote() {
        let value = fixture(include_str!(
            "../tests/fixtures/hood-launch-atomic-buy-live-proof.json"
        ));
        let output = quote(&value).unwrap();
        assert_eq!(output.launchpad, LaunchpadId::HoodFun);
        assert_eq!(output.token, value.state.token);
        assert_eq!(output.observed.action, ActionKind::Launch);
        assert_eq!(
            output.entry.amount_in_requested,
            U256::from(1_000_000_000_000_000_u64)
        );
        assert_eq!(
            output.full_position_exit.amount_in,
            output.entry.expected_output
        );
        assert!(output.entry.expected_output > U256::ZERO);
        assert!(output.full_position_exit.expected_output > U256::ZERO);
        assert!(!output.execution_eligible);
        assert!(!output.broadcast);
    }

    #[test]
    fn live_buy_for_proof_binds_token_recipient_minimum_and_receipt_trade() {
        let value = fixture(include_str!(
            "../tests/fixtures/hood-buy-for-live-proof.json"
        ));
        let output = quote(&value).unwrap();
        assert_eq!(
            output.tx_hash,
            alloy_primitives::b256!(
                "f7298ac6be29ffe53d0bac67be4dd0c1ff3353cd7fecb1be5bd0bc5d5f94ffad"
            )
        );
        assert_eq!(output.token, value.state.token);
        assert_eq!(output.leader, value.transaction.from);
        assert_eq!(output.observed.action, ActionKind::Buy);
        assert_eq!(output.observed.trader, value.transaction.from);
        assert_eq!(
            output.observed.token_amount,
            U256::from_str_radix("23239417169561597708568813", 10).unwrap()
        );
        assert!(output.entry.expected_output > U256::ZERO);
        assert!(output.full_position_exit.expected_output > U256::ZERO);
        assert!(!output.execution_eligible);
        assert!(!output.broadcast);
    }

    #[test]
    fn buy_for_proof_rejects_recipient_selector_and_minimum_drift() {
        let value = fixture(include_str!(
            "../tests/fixtures/hood-buy-for-live-proof.json"
        ));
        let assert_rejected = |transaction: &RobinhoodTransaction| {
            assert!(matches!(
                quote_hood_curve_receipt(
                    transaction,
                    &value.receipt,
                    &value.block,
                    &value.snapshot(),
                    HoodSemanticProfile::production(),
                    policy(),
                ),
                Err(HoodQuoteError::CalldataMismatch)
            ));
        };

        let mut wrong_recipient = value.transaction.clone();
        let mut input = wrong_recipient.input.to_vec();
        input[48..68].copy_from_slice(Address::with_last_byte(0xee).as_slice());
        wrong_recipient.input = input.into();
        assert_rejected(&wrong_recipient);

        let mut wrong_selector = value.transaction.clone();
        let mut input = wrong_selector.input.to_vec();
        input[..4].copy_from_slice(&HOOD_BUY_SELECTOR);
        wrong_selector.input = input.into();
        assert_rejected(&wrong_selector);

        let mut impossible_minimum = value.transaction.clone();
        let mut input = impossible_minimum.input.to_vec();
        let minimum = (U256::from_str_radix("23239417169561597708568813", 10).unwrap()
            + U256::from(1_u8))
        .to_be_bytes::<32>();
        input[68..100].copy_from_slice(&minimum);
        impossible_minimum.input = input.into();
        assert_rejected(&impossible_minimum);
    }

    #[test]
    fn live_normal_buy_reconciles_and_sell_transition_is_exact_but_non_terminal() {
        let buy = fixture(include_str!(
            "../tests/fixtures/hood-normal-buy-live-proof.json"
        ));
        let sell = fixture(include_str!("../tests/fixtures/hood-sell-live-proof.json"));
        let buy_output = quote(&buy).unwrap();
        assert_eq!(buy_output.observed.action, ActionKind::Buy);
        assert_eq!(buy_output.observed.trader, buy.transaction.from);

        let trade = exact_trade(&sell.receipt.logs).unwrap().unwrap();
        let (sell_observed, _) = reconcile_direct_trade(
            &sell.transaction,
            &sell.snapshot(),
            trade,
            &sell.receipt.logs,
        )
        .unwrap();
        assert_eq!(sell_observed.action, ActionKind::Sell);
        assert_eq!(sell_observed.trader, sell.transaction.from);
        assert!(matches!(quote(&sell), Err(HoodQuoteError::StateMismatch)));
    }

    #[test]
    fn live_state_and_transfer_tampering_fail_closed() {
        let value = fixture(include_str!(
            "../tests/fixtures/hood-normal-buy-live-proof.json"
        ));
        let mut state_tampered = value.snapshot();
        state_tampered.token_curve_supply += U256::from(1_u8);
        assert!(
            quote_hood_curve_receipt(
                &value.transaction,
                &value.receipt,
                &value.block,
                &state_tampered,
                HoodSemanticProfile::production(),
                policy(),
            )
            .is_err()
        );

        let mut receipt_tampered = value.receipt.clone();
        let transfer = receipt_tampered
            .logs
            .iter_mut()
            .find(|log| log.address == value.state.token)
            .unwrap();
        let mut tampered_data = transfer.data.to_vec();
        tampered_data[31] ^= 1;
        transfer.data = tampered_data.into();
        assert!(matches!(
            quote_hood_curve_receipt(
                &value.transaction,
                &receipt_tampered,
                &value.block,
                &value.snapshot(),
                HoodSemanticProfile::production(),
                policy(),
            ),
            Err(HoodQuoteError::StateMismatch)
        ));
    }

    #[test]
    fn every_block_terminal_curve_field_is_bound_to_receipt_state() {
        let value = fixture(include_str!(
            "../tests/fixtures/hood-normal-buy-live-proof.json"
        ));
        let assert_rejected = |snapshot: HoodMarketSnapshot| {
            assert!(matches!(
                quote_hood_curve_receipt(
                    &value.transaction,
                    &value.receipt,
                    &value.block,
                    &snapshot,
                    HoodSemanticProfile::production(),
                    policy(),
                ),
                Err(HoodQuoteError::StateMismatch)
            ));
        };

        let mut snapshot = value.snapshot();
        snapshot.curve.virtual_eth += U256::from(1_u8);
        assert_rejected(snapshot);
        let mut snapshot = value.snapshot();
        snapshot.curve.virtual_tokens += U256::from(1_u8);
        assert_rejected(snapshot);
        let mut snapshot = value.snapshot();
        snapshot.curve.real_eth += U256::from(1_u8);
        assert_rejected(snapshot);
        let mut snapshot = value.snapshot();
        snapshot.curve.real_tokens += U256::from(1_u8);
        assert_rejected(snapshot);
    }

    #[test]
    fn block_terminal_divergence_from_a_later_same_token_trade_fails_closed() {
        let value = fixture(include_str!(
            "../tests/fixtures/hood-normal-buy-live-proof.json"
        ));
        let receipt_output = quote(&value).unwrap();
        let later = quote_hood_curve_buy(
            receipt_output.receipt_end_curve,
            U256::from(1_000_000_000_000_000_u64),
        )
        .unwrap();
        let mut end_of_block = value.snapshot();
        end_of_block.curve.virtual_eth = later.state_after.virtual_quote_reserve;
        end_of_block.curve.virtual_tokens = later.state_after.virtual_token_reserve;
        end_of_block.curve.real_eth = later.state_after.virtual_quote_reserve
            - HoodSemanticProfile::production().virtual_eth_seed;
        end_of_block.curve.real_tokens = later.state_after.remaining_curve_tokens;
        assert!(matches!(
            quote_hood_curve_receipt(
                &value.transaction,
                &value.receipt,
                &value.block,
                &end_of_block,
                HoodSemanticProfile::production(),
                policy(),
            ),
            Err(HoodQuoteError::StateMismatch)
        ));
    }

    #[test]
    fn calldata_minimum_outputs_are_enforced() {
        let launch = fixture(include_str!(
            "../tests/fixtures/hood-launch-atomic-buy-live-proof.json"
        ));
        let launch_output = quote(&launch).unwrap();
        let mut launch_tx = launch.transaction.clone();
        let mut launch_input = launch_tx.input.to_vec();
        let impossible_launch_min =
            (launch_output.observed.token_amount + U256::from(1_u8)).to_be_bytes::<32>();
        launch_input[100..132].copy_from_slice(&impossible_launch_min);
        launch_tx.input = launch_input.into();
        assert!(matches!(
            quote_hood_curve_receipt(
                &launch_tx,
                &launch.receipt,
                &launch.block,
                &launch.snapshot(),
                HoodSemanticProfile::production(),
                policy(),
            ),
            Err(HoodQuoteError::CalldataMismatch)
        ));

        let buy = fixture(include_str!(
            "../tests/fixtures/hood-normal-buy-live-proof.json"
        ));
        let buy_output = quote(&buy).unwrap();
        let mut buy_tx = buy.transaction.clone();
        let mut buy_input = buy_tx.input.to_vec();
        let impossible_buy_min =
            (buy_output.observed.token_amount + U256::from(1_u8)).to_be_bytes::<32>();
        buy_input[36..68].copy_from_slice(&impossible_buy_min);
        buy_tx.input = buy_input.into();
        assert!(matches!(
            quote_hood_curve_receipt(
                &buy_tx,
                &buy.receipt,
                &buy.block,
                &buy.snapshot(),
                HoodSemanticProfile::production(),
                policy(),
            ),
            Err(HoodQuoteError::CalldataMismatch)
        ));

        let sell = fixture(include_str!("../tests/fixtures/hood-sell-live-proof.json"));
        let sell_trade = exact_trade(&sell.receipt.logs).unwrap().unwrap();
        let mut sell_tx = sell.transaction.clone();
        let mut sell_input = sell_tx.input.to_vec();
        let impossible_sell_min = (sell_trade.eth_amount + U256::from(1_u8)).to_be_bytes::<32>();
        sell_input[68..100].copy_from_slice(&impossible_sell_min);
        sell_tx.input = sell_input.into();
        assert!(matches!(
            quote_hood_curve_receipt(
                &sell_tx,
                &sell.receipt,
                &sell.block,
                &sell.snapshot(),
                HoodSemanticProfile::production(),
                policy(),
            ),
            Err(HoodQuoteError::CalldataMismatch)
        ));
    }

    #[test]
    fn atomic_launch_requires_creation_before_trade() {
        let value = fixture(include_str!(
            "../tests/fixtures/hood-launch-atomic-buy-live-proof.json"
        ));
        let mut receipt = value.receipt.clone();
        let created_position = receipt
            .logs
            .iter()
            .position(|log| log.topics.first() == Some(&HOOD_TOKEN_CREATED_TOPIC))
            .unwrap();
        let trade_position = receipt
            .logs
            .iter()
            .position(|log| log.topics.first() == Some(&HOOD_TRADE_TOPIC))
            .unwrap();
        let created_index = receipt.logs[created_position].log_index;
        receipt.logs[created_position].log_index = receipt.logs[trade_position].log_index;
        receipt.logs[trade_position].log_index = created_index;
        receipt.logs.sort_by_key(|log| log.log_index);
        assert!(matches!(
            quote_hood_curve_receipt(
                &value.transaction,
                &receipt,
                &value.block,
                &value.snapshot(),
                HoodSemanticProfile::production(),
                policy(),
            ),
            Err(HoodQuoteError::EventIdentity)
        ));
    }

    #[test]
    fn guard_configuration_is_bound_to_the_reviewed_block_epoch() {
        let historical = fixture(include_str!(
            "../tests/fixtures/hood-launch-atomic-buy-live-proof.json"
        ));
        let profile = HoodSemanticProfile::production();
        assert!(historical.state.post_block < profile.guard_disabled_l2_block);
        assert_eq!(historical.state.config.guard_max_wallet_bps, 1_000);
        quote(&historical).unwrap();

        let current = fixture(include_str!(
            "../tests/fixtures/hood-normal-buy-live-proof.json"
        ));
        let mut transition = current.snapshot();
        transition.l2_block_number = profile.guard_disabled_l2_block;
        assert!(matches!(
            profile.validate_snapshot(&transition),
            Err(HoodQuoteError::ProfileMismatch)
        ));
        let mut first_post = current.snapshot();
        first_post.l2_block_number = profile.guard_disabled_l2_block + 1;
        profile.validate_snapshot(&first_post).unwrap();

        let mut drifted = current.snapshot();
        drifted.config.guard_max_wallet_bps = 1_000;
        assert!(matches!(
            quote_hood_curve_receipt(
                &current.transaction,
                &current.receipt,
                &current.block,
                &drifted,
                profile,
                policy(),
            ),
            Err(HoodQuoteError::ProfileMismatch)
        ));
    }

    #[test]
    fn graduation_fixture_is_not_misquoted_as_direct_curve_state() {
        let value = fixture(include_str!(
            "../tests/fixtures/hood-graduation-migration-live-proof.json"
        ));
        assert!(matches!(
            quote(&value),
            Err(HoodQuoteError::InvalidEnvelope)
        ));

        let evidence = verify_hood_graduation_receipt(
            &value.transaction,
            &value.receipt,
            &value.block,
            &value.pre_snapshot(),
            &value.snapshot(),
            &HoodExpectedProfile::production(),
        )
        .unwrap();
        assert!(evidence.expected_profile_validated);
        assert!(evidence.receipt_topology_verified);
        assert!(!evidence.pool_state_reconciled);
        assert_eq!(evidence.receipt_end_swap_log_index, 53);
        assert_ne!(evidence.receipt_end_swap_input, U256::ZERO);
        assert_ne!(evidence.receipt_end_swap_output, U256::ZERO);
        assert_eq!(evidence.reconstructed_boundary_tick, 887_200);
        assert_eq!(evidence.reconstructed_boundary_liquidity, U256::ZERO);
        assert_eq!(evidence.receipt_end_liquidity, U256::ZERO);
        assert!(validate_hood_migration_boundary_evidence(
            &evidence,
            &HoodExpectedProfile::production()
        ));
        assert!(evidence.swap_amounts_reconstructed);
        assert!(evidence.terminal_zero_liquidity_boundary_observed);
        assert!(!evidence.declared_and_actual_liquidity_match);
        assert!(!evidence.v3_quote_available);
        assert_eq!(evidence.position_liquidity, U256::from(7_914_437_u64));
        let quote =
            quote_hood_migrated_v3_receipt(&evidence, &HoodExpectedProfile::production()).unwrap();
        assert_eq!(
            quote.entry.amount_in,
            U256::from(HOOD_MIGRATED_PAPER_ENTRY_WEI)
        );
        assert_eq!(
            quote.entry.expected_output,
            U256::from_str_radix("145465512933016462542115172", 10).unwrap()
        );
        assert_eq!(
            quote.entry.min_receive,
            U256::from_str_radix("144010857803686297916694020", 10).unwrap()
        );
        assert_eq!(quote.entry.state_after.initialized_ticks_crossed, 1);
        assert_eq!(
            quote.full_position_exit.expected_output,
            U256::from(989_999_999_999_957_u64)
        );
        assert_eq!(
            quote.full_position_exit.min_receive,
            U256::from(980_099_999_999_957_u64)
        );
        assert_eq!(quote.simulated_round_trip_return_bps, U256::from(9_899_u64));
        let mut pool_snapshot = HoodV3PoolSnapshot {
            pool: evidence.pool,
            factory_pool: evidence.pool,
            token0: HoodExpectedProfile::production()
                .identity(HoodIdentityRole::Weth)
                .unwrap()
                .address,
            token1: evidence.token,
            fee: 10_000,
            tick_spacing: 200,
            sqrt_price_x96: evidence.receipt_end_sqrt_price_x96,
            tick: evidence.receipt_end_tick,
            liquidity: 0,
            code_bytes: 22_142,
        };
        assert!(validate_hood_migrated_v3_pool_snapshot(
            &evidence,
            &pool_snapshot,
            &HoodExpectedProfile::production(),
        ));
        pool_snapshot.factory_pool = Address::with_last_byte(1);
        assert!(!validate_hood_migrated_v3_pool_snapshot(
            &evidence,
            &pool_snapshot,
            &HoodExpectedProfile::production(),
        ));
        assert!(!evidence.execution_eligible);
        assert!(!evidence.broadcast);
    }

    #[test]
    fn downstream_migration_boundary_forgeries_fail_closed() {
        use uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick;

        let value = fixture(include_str!(
            "../tests/fixtures/hood-graduation-migration-live-proof.json"
        ));
        let profile = HoodExpectedProfile::production();
        let evidence = verify_hood_graduation_receipt(
            &value.transaction,
            &value.receipt,
            &value.block,
            &value.pre_snapshot(),
            &value.snapshot(),
            &profile,
        )
        .unwrap();

        let mut nonzero_terminal_liquidity = evidence.clone();
        nonzero_terminal_liquidity.receipt_end_liquidity = U256::from(1_u8);
        assert!(!validate_hood_migration_boundary_evidence(
            &nonzero_terminal_liquidity,
            &profile
        ));

        let mut coordinated_boundary = evidence.clone();
        coordinated_boundary.reconstructed_boundary_tick -= profile.v3_tick_spacing;
        coordinated_boundary.reconstructed_boundary_sqrt_price_x96 =
            get_sqrt_ratio_at_tick(coordinated_boundary.reconstructed_boundary_tick).unwrap();
        assert!(!validate_hood_migration_boundary_evidence(
            &coordinated_boundary,
            &profile
        ));

        let mut reordered = evidence.clone();
        std::mem::swap(
            &mut reordered.log_order.migrated,
            &mut reordered.log_order.v3_migrated,
        );
        assert!(!validate_hood_migration_boundary_evidence(
            &reordered, &profile
        ));

        let mut boundary_not_crossed = evidence;
        boundary_not_crossed.receipt_end_tick = boundary_not_crossed.reconstructed_boundary_tick;
        boundary_not_crossed.receipt_end_sqrt_price_x96 =
            boundary_not_crossed.reconstructed_boundary_sqrt_price_x96;
        assert!(!validate_hood_migration_boundary_evidence(
            &boundary_not_crossed,
            &profile
        ));
    }

    #[test]
    fn migrated_v3_quote_rejects_receipt_and_position_tampering() {
        let value = fixture(include_str!(
            "../tests/fixtures/hood-graduation-migration-live-proof.json"
        ));
        let profile = HoodExpectedProfile::production();
        let evidence = verify_hood_graduation_receipt(
            &value.transaction,
            &value.receipt,
            &value.block,
            &value.pre_snapshot(),
            &value.snapshot(),
            &profile,
        )
        .unwrap();

        let mut zero_position = evidence.clone();
        zero_position.position_liquidity = U256::ZERO;
        assert!(matches!(
            quote_hood_migrated_v3_receipt(&zero_position, &profile),
            Err(HoodQuoteError::MigrationMismatch)
        ));

        let mut wrong_terminal_tick = evidence.clone();
        wrong_terminal_tick.receipt_end_tick = wrong_terminal_tick.reconstructed_boundary_tick;
        assert!(matches!(
            quote_hood_migrated_v3_receipt(&wrong_terminal_tick, &profile),
            Err(HoodQuoteError::MigrationMismatch)
        ));

        let mut nonzero_terminal_liquidity = evidence.clone();
        nonzero_terminal_liquidity.receipt_end_liquidity = U256::from(1_u8);
        assert!(matches!(
            quote_hood_migrated_v3_receipt(&nonzero_terminal_liquidity, &profile),
            Err(HoodQuoteError::MigrationMismatch)
        ));

        let mut unavailable_liquidity = evidence;
        unavailable_liquidity.position_liquidity = U256::from(u128::MAX) + U256::from(1_u8);
        assert!(matches!(
            quote_hood_migrated_v3_receipt(&unavailable_liquidity, &profile),
            Err(HoodQuoteError::MigrationMismatch)
        ));
    }

    #[test]
    fn graduation_verification_is_token_scoped_inside_batched_receipts() {
        let value = fixture(include_str!(
            "../tests/fixtures/hood-graduation-migration-live-proof.json"
        ));
        let target_topics = [
            HOOD_TRADE_TOPIC,
            HOOD_GRADUATED_TOPIC,
            events::V3Migrated::SIGNATURE_HASH,
            HOOD_MIGRATED_TOPIC,
        ];
        let other_token = alloy_primitives::address!("96cd468583e361794b62d5aa4c79b2b4cac2600d");
        let other_topic = B256::left_padding_from(other_token.as_slice());
        let mut batched = value.receipt.clone();
        let mut extra = batched
            .logs
            .iter()
            .filter(|log| {
                log.topics
                    .first()
                    .is_some_and(|topic| target_topics.contains(topic))
            })
            .cloned()
            .collect::<Vec<_>>();
        let next_log_index = batched.logs.last().unwrap().log_index + 1;
        for (offset, log) in extra.iter_mut().enumerate() {
            log.log_index = next_log_index + u64::try_from(offset).unwrap();
            log.topics[1] = other_topic;
        }
        batched.logs.extend(extra);
        assert!(
            verify_hood_graduation_receipt(
                &value.transaction,
                &batched,
                &value.block,
                &value.pre_snapshot(),
                &value.snapshot(),
                &HoodExpectedProfile::production(),
            )
            .is_ok()
        );

        let duplicate = batched
            .logs
            .iter()
            .find(|log| {
                log.topics.first() == Some(&HOOD_MIGRATED_TOPIC)
                    && topic_address(log, 1) == Some(value.snapshot().token)
            })
            .unwrap()
            .clone();
        batched.logs.push(ReceiptLog {
            log_index: batched.logs.last().unwrap().log_index + 1,
            ..duplicate
        });
        assert!(matches!(
            verify_hood_graduation_receipt(
                &value.transaction,
                &batched,
                &value.block,
                &value.pre_snapshot(),
                &value.snapshot(),
                &HoodExpectedProfile::production(),
            ),
            Err(HoodQuoteError::MigrationMismatch)
        ));
    }

    #[test]
    fn graduation_topology_emitter_order_and_arithmetic_tampering_fail_closed() {
        let value = fixture(include_str!(
            "../tests/fixtures/hood-graduation-migration-live-proof.json"
        ));
        let verify = |receipt: &NoxaReceipt, post: &HoodMarketSnapshot| {
            verify_hood_graduation_receipt(
                &value.transaction,
                receipt,
                &value.block,
                &value.pre_snapshot(),
                post,
                &HoodExpectedProfile::production(),
            )
        };

        let mut emitter = value.receipt.clone();
        emitter
            .logs
            .iter_mut()
            .find(|log| log.topics.first() == Some(&events::V3Migrated::SIGNATURE_HASH))
            .unwrap()
            .address = Address::with_last_byte(0xee);
        assert!(matches!(
            verify(&emitter, &value.snapshot()),
            Err(HoodQuoteError::MigrationMismatch)
        ));

        let mut reordered = value.receipt.clone();
        let trade_position = reordered
            .logs
            .iter()
            .position(|log| log.topics.first() == Some(&HOOD_TRADE_TOPIC))
            .unwrap();
        let graduated_position = reordered
            .logs
            .iter()
            .position(|log| log.topics.first() == Some(&HOOD_GRADUATED_TOPIC))
            .unwrap();
        let trade_index = reordered.logs[trade_position].log_index;
        reordered.logs[trade_position].log_index = reordered.logs[graduated_position].log_index;
        reordered.logs[graduated_position].log_index = trade_index;
        reordered.logs.sort_by_key(|log| log.log_index);
        assert!(matches!(
            verify(&reordered, &value.snapshot()),
            Err(HoodQuoteError::MigrationMismatch)
        ));

        let mut post = value.snapshot();
        post.curve.virtual_eth += U256::from(1_u8);
        assert!(matches!(
            verify(&value.receipt, &post),
            Err(HoodQuoteError::MigrationMismatch)
        ));

        let mut trade_fee = value.receipt.clone();
        let trade = trade_fee
            .logs
            .iter_mut()
            .find(|log| log.topics.first() == Some(&HOOD_TRADE_TOPIC))
            .unwrap();
        let mut data = trade.data.to_vec();
        data[4 * 32 - 1] ^= 1;
        trade.data = data.into();
        assert!(matches!(
            verify(&trade_fee, &value.snapshot()),
            Err(HoodQuoteError::MigrationMismatch)
        ));

        let mut pre = value.pre_snapshot();
        pre.curve.virtual_eth += U256::from(1_u8);
        pre.curve.real_eth += U256::from(1_u8);
        assert!(matches!(
            verify_hood_graduation_receipt(
                &value.transaction,
                &value.receipt,
                &value.block,
                &pre,
                &value.snapshot(),
                &HoodExpectedProfile::production(),
            ),
            Err(HoodQuoteError::MigrationMismatch)
        ));

        let mut identity = value.snapshot();
        identity.curve.creator = Address::with_last_byte(0xdd);
        assert!(matches!(
            verify(&value.receipt, &identity),
            Err(HoodQuoteError::MigrationMismatch)
        ));

        let profile = HoodExpectedProfile::production();
        let position_manager = profile
            .identity(HoodIdentityRole::PositionManager)
            .unwrap()
            .address;
        let locker = profile.identity(HoodIdentityRole::Locker).unwrap().address;
        let mut transfer_order = value.receipt.clone();
        let nft_lock_position = transfer_order
            .logs
            .iter()
            .position(|log| {
                log.address == position_manager
                    && log.topics.first() == Some(&TRANSFER_TOPIC)
                    && topic_address(log, 2) == Some(locker)
            })
            .unwrap();
        let locked_position = transfer_order
            .logs
            .iter()
            .position(|log| log.topics.first() == Some(&events::Locked::SIGNATURE_HASH))
            .unwrap();
        let transfer_index = transfer_order.logs[nft_lock_position].log_index;
        transfer_order.logs[nft_lock_position].log_index =
            transfer_order.logs[locked_position].log_index;
        transfer_order.logs[locked_position].log_index = transfer_index;
        transfer_order.logs.sort_by_key(|log| log.log_index);
        assert!(matches!(
            verify(&transfer_order, &value.snapshot()),
            Err(HoodQuoteError::MigrationMismatch)
        ));
    }

    #[test]
    fn migration_pool_state_tampering_fails_closed() {
        let value = fixture(include_str!(
            "../tests/fixtures/hood-graduation-migration-live-proof.json"
        ));
        let mut receipt = value.receipt.clone();
        let initialize = receipt
            .logs
            .iter_mut()
            .find(|log| log.topics.first() == Some(&events::Initialize::SIGNATURE_HASH))
            .unwrap();
        let mut data = initialize.data.to_vec();
        data[31] ^= 1;
        initialize.data = data.into();
        assert!(matches!(
            verify_hood_graduation_receipt(
                &value.transaction,
                &receipt,
                &value.block,
                &value.pre_snapshot(),
                &value.snapshot(),
                &HoodExpectedProfile::production(),
            ),
            Err(HoodQuoteError::MigrationMismatch)
        ));

        let mut receipt = value.receipt.clone();
        let swap = receipt
            .logs
            .iter_mut()
            .find(|log| log.topics.first() == Some(&events::Swap::SIGNATURE_HASH))
            .unwrap();
        let mut data = swap.data.to_vec();
        data[2 * 32 - 1] ^= 1;
        swap.data = data.into();
        assert!(matches!(
            verify_hood_graduation_receipt(
                &value.transaction,
                &receipt,
                &value.block,
                &value.pre_snapshot(),
                &value.snapshot(),
                &HoodExpectedProfile::production(),
            ),
            Err(HoodQuoteError::MigrationMismatch)
        ));
    }

    #[test]
    fn fixed_topics_match_live_receipts() {
        assert_eq!(
            HOOD_TOKEN_CREATED_TOPIC,
            alloy_primitives::b256!(
                "91de26bc430b3a4f1d6cfb11d72f2e5ca75d7622d37b2a88a8998ec28e747a11"
            )
        );
        assert_eq!(
            HOOD_TRADE_TOPIC,
            alloy_primitives::b256!(
                "2c76e7a47fd53e2854856ac3f0a5f3ee40d15cfaa82266357ea9779c486ab9c3"
            )
        );
    }
}
