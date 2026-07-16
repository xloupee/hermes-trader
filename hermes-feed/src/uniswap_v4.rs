//! Pure, warm-state Uniswap v4 market validation and follower planning.
//!
//! This module deliberately has no RPC or persistence handle. Startup code is
//! expected to validate every [`CodePin`] and build immutable market snapshots
//! before candidates are admitted. A quote is follower-owned warm state; no
//! leader route bytes, deadline, hook data, or minimum output enter this API.

use alloy_primitives::{Address, B256, U256, keccak256};
use alloy_sol_types::{SolValue, sol};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::robinhood::{CHAIN_ID, WETH};

pub const DYNAMIC_FEE_FLAG: u32 = 0x80_0000;
pub const MAX_LP_FEE_PPM: u32 = 1_000_000;
pub const MAX_TICK_SPACING: i32 = 32_767;

sol! {
    struct AbiPoolKey {
        address currency0;
        address currency1;
        uint24 fee;
        int24 tickSpacing;
        address hooks;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct CodePin {
    pub address: Address,
    pub runtime_code_hash: B256,
}

impl CodePin {
    pub fn is_complete(self) -> bool {
        self.address != Address::ZERO && self.runtime_code_hash != B256::ZERO
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct V4PoolKey {
    pub currency0: Address,
    pub currency1: Address,
    /// Static fee in parts-per-million, or [`DYNAMIC_FEE_FLAG`].
    pub fee: u32,
    pub tick_spacing: i32,
    pub hooks: Address,
}

impl V4PoolKey {
    pub fn canonical(
        currency_a: Address,
        currency_b: Address,
        fee: u32,
        tick_spacing: i32,
        hooks: Address,
    ) -> Result<Self, V4Error> {
        if currency_a == Address::ZERO || currency_b == Address::ZERO || currency_a == currency_b {
            return Err(V4Error::InvalidCurrencyPair);
        }
        let (currency0, currency1) = if currency_a < currency_b {
            (currency_a, currency_b)
        } else {
            (currency_b, currency_a)
        };
        let key = Self {
            currency0,
            currency1,
            fee,
            tick_spacing,
            hooks,
        };
        key.validate()?;
        Ok(key)
    }

    pub fn validate(self) -> Result<(), V4Error> {
        if self.currency0 == Address::ZERO
            || self.currency1 == Address::ZERO
            || self.currency0 >= self.currency1
        {
            return Err(V4Error::InvalidCurrencyPair);
        }
        if self.tick_spacing <= 0 || self.tick_spacing > MAX_TICK_SPACING {
            return Err(V4Error::InvalidTickSpacing);
        }
        if self.fee != DYNAMIC_FEE_FLAG && self.fee > MAX_LP_FEE_PPM {
            return Err(V4Error::InvalidFee);
        }
        Ok(())
    }

    pub fn pool_id(self) -> B256 {
        let encoded = AbiPoolKey {
            currency0: self.currency0,
            currency1: self.currency1,
            fee: alloy_primitives::aliases::U24::from(self.fee),
            tickSpacing: alloy_primitives::aliases::I24::try_from(self.tick_spacing)
                .expect("validated v4 tick spacing fits int24"),
            hooks: self.hooks,
        }
        .abi_encode();
        keccak256(encoded)
    }

    pub fn contains(self, asset: Address) -> bool {
        asset == self.currency0 || asset == self.currency1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum V4FeePolicy {
    Static { fee_ppm: u32 },
    Dynamic { min_fee_ppm: u32, max_fee_ppm: u32 },
}

impl V4FeePolicy {
    fn validate_for(self, key: V4PoolKey) -> Result<(), V4Error> {
        match self {
            Self::Static { fee_ppm } if key.fee == fee_ppm && fee_ppm <= MAX_LP_FEE_PPM => Ok(()),
            Self::Dynamic {
                min_fee_ppm,
                max_fee_ppm,
            } if key.fee == DYNAMIC_FEE_FLAG
                && min_fee_ppm <= max_fee_ppm
                && max_fee_ppm <= MAX_LP_FEE_PPM =>
            {
                Ok(())
            }
            _ => Err(V4Error::FeePolicyMismatch),
        }
    }

    fn admits(self, fee_ppm: u32) -> bool {
        match self {
            Self::Static { fee_ppm: pinned } => fee_ppm == pinned,
            Self::Dynamic {
                min_fee_ppm,
                max_fee_ppm,
            } => (min_fee_ppm..=max_fee_ppm).contains(&fee_ppm),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct HookPin {
    pub code: CodePin,
    /// Hash of the allowlisted immutable hook/profile configuration.
    pub configuration_hash: B256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct V4MarketSnapshot {
    pub chain_id: u64,
    pub pool_manager: CodePin,
    pub key: V4PoolKey,
    pub pool_id: B256,
    pub hook: HookPin,
    pub quote_asset: Address,
    pub fee_policy: V4FeePolicy,
    /// Monotonic version assigned when off-path state is atomically refreshed.
    pub state_version: u64,
}

impl V4MarketSnapshot {
    pub fn validate(&self, expected_pool_manager: Address) -> Result<(), V4Error> {
        if self.chain_id != CHAIN_ID {
            return Err(V4Error::WrongChain);
        }
        if !self.pool_manager.is_complete() || self.pool_manager.address != expected_pool_manager {
            return Err(V4Error::PoolManagerPinMismatch);
        }
        self.key.validate()?;
        if self.pool_id != self.key.pool_id() {
            return Err(V4Error::PoolIdMismatch);
        }
        if self.quote_asset != WETH || !self.key.contains(self.quote_asset) {
            return Err(V4Error::UnsupportedQuoteAsset);
        }
        if !self.hook.code.is_complete()
            || self.hook.code.address != self.key.hooks
            || self.hook.configuration_hash == B256::ZERO
        {
            return Err(V4Error::HookPinMismatch);
        }
        self.fee_policy.validate_for(self.key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct WarmV4Quote {
    pub pool_id: B256,
    pub state_version: u64,
    pub asset_in: Address,
    pub asset_out: Address,
    pub amount_in: U256,
    pub expected_amount_out: U256,
    pub applied_fee_ppm: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct FollowerV4Policy {
    pub recipient: Address,
    pub spend_limit: U256,
    pub max_slippage_bps: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct V4PaperPlan {
    pub pool_id: B256,
    pub pool_manager: Address,
    pub hook: Address,
    pub asset_in: Address,
    pub asset_out: Address,
    pub amount_in: U256,
    pub expected_amount_out: U256,
    pub min_receive: U256,
    pub spend_limit: U256,
    pub recipient: Address,
    pub state_version: u64,
}

pub fn build_follower_v4_plan(
    market: &V4MarketSnapshot,
    expected_pool_manager: Address,
    quote: WarmV4Quote,
    policy: FollowerV4Policy,
) -> Result<V4PaperPlan, V4Error> {
    market.validate(expected_pool_manager)?;
    if quote.pool_id != market.pool_id || quote.state_version != market.state_version {
        return Err(V4Error::StaleOrForeignQuote);
    }
    if quote.asset_in == quote.asset_out
        || !market.key.contains(quote.asset_in)
        || !market.key.contains(quote.asset_out)
    {
        return Err(V4Error::InvalidDirection);
    }
    if quote.amount_in == U256::ZERO
        || quote.expected_amount_out == U256::ZERO
        || quote.amount_in > policy.spend_limit
        || policy.recipient == Address::ZERO
    {
        return Err(V4Error::InvalidFollowerAmounts);
    }
    if policy.max_slippage_bps > 10_000 {
        return Err(V4Error::InvalidSlippage);
    }
    if !market.fee_policy.admits(quote.applied_fee_ppm) {
        return Err(V4Error::FeeOutsidePin);
    }
    let retained_bps = U256::from(10_000_u16 - policy.max_slippage_bps);
    let min_receive = quote
        .expected_amount_out
        .checked_mul(retained_bps)
        .ok_or(V4Error::ArithmeticOverflow)?
        / U256::from(10_000_u16);
    if min_receive == U256::ZERO {
        return Err(V4Error::InvalidFollowerAmounts);
    }
    Ok(V4PaperPlan {
        pool_id: market.pool_id,
        pool_manager: market.pool_manager.address,
        hook: market.key.hooks,
        asset_in: quote.asset_in,
        asset_out: quote.asset_out,
        amount_in: quote.amount_in,
        expected_amount_out: quote.expected_amount_out,
        min_receive,
        spend_limit: policy.spend_limit,
        recipient: policy.recipient,
        state_version: quote.state_version,
    })
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum V4Error {
    #[error("candidate is not Robinhood Chain mainnet")]
    WrongChain,
    #[error("v4 currency pair is non-canonical")]
    InvalidCurrencyPair,
    #[error("v4 tick spacing is invalid")]
    InvalidTickSpacing,
    #[error("v4 fee encoding is invalid")]
    InvalidFee,
    #[error("v4 fee policy does not match the pool key")]
    FeePolicyMismatch,
    #[error("pool manager address or runtime hash is not pinned")]
    PoolManagerPinMismatch,
    #[error("pool id does not match the canonical pool key")]
    PoolIdMismatch,
    #[error("only the pinned WETH quote profile is supported")]
    UnsupportedQuoteAsset,
    #[error("hook address, runtime hash, or configuration is not pinned")]
    HookPinMismatch,
    #[error("quote belongs to another pool or state version")]
    StaleOrForeignQuote,
    #[error("quote direction is not this market")]
    InvalidDirection,
    #[error("follower amount, limit, or recipient is invalid")]
    InvalidFollowerAmounts,
    #[error("follower slippage is invalid")]
    InvalidSlippage,
    #[error("warm quote fee is outside the pinned policy")]
    FeeOutsidePin,
    #[error("arithmetic overflow")]
    ArithmeticOverflow,
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{address, b256};

    use super::*;

    const POOL_MANAGER: Address = address!("8366a39cc670b4001a1121b8f6a443a643e40951");
    const TOKEN: Address = address!("6bbbb3be7424a911d5d131e272639512c1c12b07");
    const HOOK: Address = address!("0000000000000000000000000000000000000042");

    fn market() -> V4MarketSnapshot {
        let key = V4PoolKey::canonical(WETH, TOKEN, DYNAMIC_FEE_FLAG, 60, HOOK).unwrap();
        V4MarketSnapshot {
            chain_id: CHAIN_ID,
            pool_manager: CodePin {
                address: POOL_MANAGER,
                runtime_code_hash: b256!(
                    "1111111111111111111111111111111111111111111111111111111111111111"
                ),
            },
            key,
            pool_id: key.pool_id(),
            hook: HookPin {
                code: CodePin {
                    address: HOOK,
                    runtime_code_hash: b256!(
                        "2222222222222222222222222222222222222222222222222222222222222222"
                    ),
                },
                configuration_hash: b256!(
                    "3333333333333333333333333333333333333333333333333333333333333333"
                ),
            },
            quote_asset: WETH,
            fee_policy: V4FeePolicy::Dynamic {
                min_fee_ppm: 1_000,
                max_fee_ppm: 10_000,
            },
            state_version: 7,
        }
    }

    fn quote(market: &V4MarketSnapshot) -> WarmV4Quote {
        WarmV4Quote {
            pool_id: market.pool_id,
            state_version: market.state_version,
            asset_in: WETH,
            asset_out: TOKEN,
            amount_in: U256::from(100),
            expected_amount_out: U256::from(1_000),
            applied_fee_ppm: 5_000,
        }
    }

    #[test]
    fn pool_key_is_canonical_and_pool_id_is_order_independent() {
        let a = V4PoolKey::canonical(WETH, TOKEN, DYNAMIC_FEE_FLAG, 60, HOOK).unwrap();
        let b = V4PoolKey::canonical(TOKEN, WETH, DYNAMIC_FEE_FLAG, 60, HOOK).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.pool_id(), b.pool_id());
        let changed = V4PoolKey {
            hooks: Address::with_last_byte(0x43),
            ..a
        };
        assert_ne!(a.pool_id(), changed.pool_id());
    }

    #[test]
    fn builds_min_receive_from_follower_quote_not_leader_fields() {
        let market = market();
        let plan = build_follower_v4_plan(
            &market,
            POOL_MANAGER,
            quote(&market),
            FollowerV4Policy {
                recipient: Address::with_last_byte(9),
                spend_limit: U256::from(120),
                max_slippage_bps: 250,
            },
        )
        .unwrap();
        assert_eq!(plan.min_receive, U256::from(975));
        assert_eq!(plan.amount_in, U256::from(100));
        assert_eq!(plan.hook, HOOK);
    }

    #[test]
    fn rejects_wrong_chain_manager_hook_quote_and_dynamic_fee() {
        let base = market();
        let mut wrong = base.clone();
        wrong.chain_id = 8453;
        assert_eq!(wrong.validate(POOL_MANAGER), Err(V4Error::WrongChain));

        let mut wrong = base.clone();
        wrong.pool_manager.runtime_code_hash = B256::ZERO;
        assert_eq!(
            wrong.validate(POOL_MANAGER),
            Err(V4Error::PoolManagerPinMismatch)
        );

        let mut wrong = base.clone();
        wrong.hook.code.address = Address::with_last_byte(4);
        assert_eq!(wrong.validate(POOL_MANAGER), Err(V4Error::HookPinMismatch));

        let mut wrong = base.clone();
        wrong.quote_asset = Address::with_last_byte(5);
        assert_eq!(
            wrong.validate(POOL_MANAGER),
            Err(V4Error::UnsupportedQuoteAsset)
        );

        let mut out_of_bounds = quote(&base);
        out_of_bounds.applied_fee_ppm = 10_001;
        assert_eq!(
            build_follower_v4_plan(
                &base,
                POOL_MANAGER,
                out_of_bounds,
                FollowerV4Policy {
                    recipient: Address::with_last_byte(1),
                    spend_limit: U256::from(100),
                    max_slippage_bps: 100,
                },
            ),
            Err(V4Error::FeeOutsidePin)
        );
    }

    #[test]
    fn rejects_lookalike_pool_ids_and_stale_quotes() {
        let base = market();
        let mut lookalike = base.clone();
        lookalike.pool_id =
            b256!("9999999999999999999999999999999999999999999999999999999999999999");
        assert_eq!(
            lookalike.validate(POOL_MANAGER),
            Err(V4Error::PoolIdMismatch)
        );

        let mut stale = quote(&base);
        stale.state_version -= 1;
        assert_eq!(
            build_follower_v4_plan(
                &base,
                POOL_MANAGER,
                stale,
                FollowerV4Policy {
                    recipient: Address::with_last_byte(1),
                    spend_limit: U256::from(100),
                    max_slippage_bps: 100,
                },
            ),
            Err(V4Error::StaleOrForeignQuote)
        );
    }
}
