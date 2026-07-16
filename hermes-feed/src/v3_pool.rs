use std::collections::{BTreeMap, HashMap};

use alloy_primitives::{Address, I256, U256};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;
use uniswap_v3_math::error::UniswapV3MathError;
use uniswap_v3_math::liquidity_math::add_delta;
use uniswap_v3_math::swap_math::compute_swap_step;
use uniswap_v3_math::tick_bitmap::{flip_tick, next_initialized_tick_within_one_word};
use uniswap_v3_math::tick_math::{
    MAX_SQRT_RATIO, MAX_TICK, MIN_SQRT_RATIO, MIN_TICK, get_sqrt_ratio_at_tick,
    get_tick_at_sqrt_ratio,
};

const MAX_SWAP_STEPS: usize = 8_192;
const FEE_DENOMINATOR: u32 = 1_000_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct TickLiquidity {
    gross: u128,
    net: i128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V3PoolState {
    pub pool: Address,
    pub token0: Address,
    pub token1: Address,
    pub fee: u32,
    pub tick_spacing: i32,
    pub sqrt_price_x96: U256,
    pub tick: i32,
    pub liquidity: u128,
    ticks: BTreeMap<i32, TickLiquidity>,
    tick_bitmap: HashMap<i16, U256>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct V3Quote {
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in_requested: U256,
    pub amount_in_consumed: U256,
    pub amount_out: U256,
    pub sqrt_price_x96_after: U256,
    pub tick_after: i32,
    #[serde(
        serialize_with = "serialize_u128_hex",
        deserialize_with = "deserialize_u128_hex"
    )]
    pub liquidity_after: u128,
    pub initialized_ticks_crossed: usize,
    pub steps: usize,
}

fn serialize_u128_hex<S>(value: &u128, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&format!("0x{value:x}"))
}

fn deserialize_u128_hex<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    let digits = value
        .strip_prefix("0x")
        .ok_or_else(|| serde::de::Error::custom("u128 hex value must start with 0x"))?;
    u128::from_str_radix(digits, 16).map_err(serde::de::Error::custom)
}

#[derive(Debug, Error)]
pub enum V3PoolError {
    #[error("pool tokens must be non-zero, distinct, and sorted")]
    InvalidTokens,
    #[error("fee must be below one million pips")]
    InvalidFee,
    #[error("tick spacing must be positive")]
    InvalidTickSpacing,
    #[error("tick {0} is outside the Uniswap V3 range")]
    InvalidTick(i32),
    #[error("sqrt price is outside the Uniswap V3 range")]
    InvalidSqrtPrice,
    #[error("position ticks are invalid or not aligned to spacing")]
    InvalidPosition,
    #[error("liquidity delta exceeds signed int128")]
    LiquidityDeltaTooLarge,
    #[error("unknown input token {0}")]
    UnknownInputToken(Address),
    #[error("exact input must be non-zero and fit signed int256")]
    InvalidAmountIn,
    #[error("sqrt price limit is invalid for the swap direction")]
    InvalidPriceLimit,
    #[error("swap exhausted active liquidity before consuming the requested input")]
    InsufficientLiquidity,
    #[error("swap step limit exceeded")]
    StepLimitExceeded,
    #[error("arithmetic overflow")]
    ArithmeticOverflow,
    #[error(transparent)]
    Math(#[from] UniswapV3MathError),
}

impl V3PoolState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: Address,
        token0: Address,
        token1: Address,
        fee: u32,
        tick_spacing: i32,
        sqrt_price_x96: U256,
        tick: i32,
        liquidity: u128,
    ) -> Result<Self, V3PoolError> {
        if token0 == Address::ZERO || token1 == Address::ZERO || token0 >= token1 {
            return Err(V3PoolError::InvalidTokens);
        }
        if fee >= FEE_DENOMINATOR {
            return Err(V3PoolError::InvalidFee);
        }
        if tick_spacing <= 0 {
            return Err(V3PoolError::InvalidTickSpacing);
        }
        if !(MIN_TICK..=MAX_TICK).contains(&tick) {
            return Err(V3PoolError::InvalidTick(tick));
        }
        if sqrt_price_x96 < MIN_SQRT_RATIO || sqrt_price_x96 >= MAX_SQRT_RATIO {
            return Err(V3PoolError::InvalidSqrtPrice);
        }
        Ok(Self {
            pool,
            token0,
            token1,
            fee,
            tick_spacing,
            sqrt_price_x96,
            tick,
            liquidity,
            ticks: BTreeMap::new(),
            tick_bitmap: HashMap::new(),
        })
    }

    pub fn add_position(
        &mut self,
        tick_lower: i32,
        tick_upper: i32,
        amount: u128,
    ) -> Result<(), V3PoolError> {
        if amount == 0
            || tick_lower >= tick_upper
            || tick_lower < MIN_TICK
            || tick_upper > MAX_TICK
            || tick_lower % self.tick_spacing != 0
            || tick_upper % self.tick_spacing != 0
        {
            return Err(V3PoolError::InvalidPosition);
        }
        let signed = i128::try_from(amount).map_err(|_| V3PoolError::LiquidityDeltaTooLarge)?;
        // Stage every mutation so a later tick or active-liquidity overflow
        // cannot leave the caller with a partially applied position.
        let mut staged = self.clone();
        staged.update_tick(tick_lower, amount, signed)?;
        staged.update_tick(tick_upper, amount, -signed)?;
        if tick_lower <= self.tick && self.tick < tick_upper {
            staged.liquidity = add_delta(staged.liquidity, signed)?;
        }
        *self = staged;
        Ok(())
    }

    pub fn set_observation(
        &mut self,
        sqrt_price_x96: U256,
        tick: i32,
        liquidity: u128,
    ) -> Result<(), V3PoolError> {
        if !(MIN_TICK..=MAX_TICK).contains(&tick) {
            return Err(V3PoolError::InvalidTick(tick));
        }
        if sqrt_price_x96 < MIN_SQRT_RATIO || sqrt_price_x96 >= MAX_SQRT_RATIO {
            return Err(V3PoolError::InvalidSqrtPrice);
        }
        self.sqrt_price_x96 = sqrt_price_x96;
        self.tick = tick;
        self.liquidity = liquidity;
        Ok(())
    }

    pub fn quote_exact_input(
        &self,
        token_in: Address,
        amount_in: U256,
        sqrt_price_limit_x96: Option<U256>,
    ) -> Result<V3Quote, V3PoolError> {
        if amount_in == U256::ZERO || amount_in >= (U256::from(1_u8) << 255) {
            return Err(V3PoolError::InvalidAmountIn);
        }
        let zero_for_one = if token_in == self.token0 {
            true
        } else if token_in == self.token1 {
            false
        } else {
            return Err(V3PoolError::UnknownInputToken(token_in));
        };
        let token_out = if zero_for_one {
            self.token1
        } else {
            self.token0
        };
        let price_limit = sqrt_price_limit_x96.unwrap_or_else(|| {
            if zero_for_one {
                MIN_SQRT_RATIO + U256::from(1_u8)
            } else {
                MAX_SQRT_RATIO - U256::from(1_u8)
            }
        });
        if (zero_for_one && (price_limit <= MIN_SQRT_RATIO || price_limit >= self.sqrt_price_x96))
            || (!zero_for_one
                && (price_limit >= MAX_SQRT_RATIO || price_limit <= self.sqrt_price_x96))
        {
            return Err(V3PoolError::InvalidPriceLimit);
        }

        let mut remaining = amount_in;
        let mut output = U256::ZERO;
        let mut sqrt_price = self.sqrt_price_x96;
        let mut tick = self.tick;
        let mut liquidity = self.liquidity;
        let mut initialized_ticks_crossed = 0_usize;
        let mut steps = 0_usize;

        while remaining != U256::ZERO && sqrt_price != price_limit {
            if steps >= MAX_SWAP_STEPS {
                return Err(V3PoolError::StepLimitExceeded);
            }
            steps += 1;
            let sqrt_price_start = sqrt_price;
            let (next_tick, initialized) = next_initialized_tick_within_one_word(
                &self.tick_bitmap,
                tick,
                self.tick_spacing,
                zero_for_one,
            )?;
            let next_tick = next_tick.clamp(MIN_TICK, MAX_TICK);
            let sqrt_price_next = get_sqrt_ratio_at_tick(next_tick)?;
            let target = if zero_for_one {
                sqrt_price_next.max(price_limit)
            } else {
                sqrt_price_next.min(price_limit)
            };
            if liquidity == 0 {
                // Uniswap V3 traverses an empty range without consuming input.
                // Advancing to the target lets the common boundary-crossing
                // path below activate liquidity at the next initialized tick.
                sqrt_price = target;
            } else {
                let (next_sqrt, step_in, step_out, step_fee) = compute_swap_step(
                    sqrt_price,
                    target,
                    liquidity,
                    I256::from_raw(remaining),
                    self.fee,
                )?;
                let spent = step_in
                    .checked_add(step_fee)
                    .ok_or(V3PoolError::ArithmeticOverflow)?;
                remaining = remaining
                    .checked_sub(spent)
                    .ok_or(V3PoolError::ArithmeticOverflow)?;
                output = output
                    .checked_add(step_out)
                    .ok_or(V3PoolError::ArithmeticOverflow)?;
                sqrt_price = next_sqrt;
            }

            if sqrt_price == sqrt_price_next {
                if initialized {
                    let mut liquidity_net = self
                        .ticks
                        .get(&next_tick)
                        .ok_or(V3PoolError::InsufficientLiquidity)?
                        .net;
                    if zero_for_one {
                        liquidity_net = liquidity_net
                            .checked_neg()
                            .ok_or(V3PoolError::ArithmeticOverflow)?;
                    }
                    liquidity = add_delta(liquidity, liquidity_net)?;
                    initialized_ticks_crossed += 1;
                }
                tick = if zero_for_one {
                    next_tick.saturating_sub(1)
                } else {
                    next_tick
                };
            } else if sqrt_price != sqrt_price_start {
                tick = get_tick_at_sqrt_ratio(sqrt_price)?;
            } else {
                return Err(V3PoolError::InsufficientLiquidity);
            }
        }

        Ok(V3Quote {
            token_in,
            token_out,
            amount_in_requested: amount_in,
            amount_in_consumed: amount_in - remaining,
            amount_out: output,
            sqrt_price_x96_after: sqrt_price,
            tick_after: tick,
            liquidity_after: liquidity,
            initialized_ticks_crossed,
            steps,
        })
    }

    fn update_tick(
        &mut self,
        tick: i32,
        gross_delta: u128,
        net_delta: i128,
    ) -> Result<(), V3PoolError> {
        let entry = self.ticks.entry(tick).or_default();
        let was_initialized = entry.gross != 0;
        entry.gross = entry
            .gross
            .checked_add(gross_delta)
            .ok_or(V3PoolError::ArithmeticOverflow)?;
        entry.net = entry
            .net
            .checked_add(net_delta)
            .ok_or(V3PoolError::ArithmeticOverflow)?;
        if !was_initialized {
            flip_tick(&mut self.tick_bitmap, tick, self.tick_spacing)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{U256, address};

    use super::*;

    fn live_launch_pool() -> V3PoolState {
        let mut pool = V3PoolState::new(
            address!("efd703d89b7febc0ae43fdd72edd257819366272"),
            address!("0bd7d308f8e1639fab988df18a8011f41eacad73"),
            address!("955b339944cbd4834156366d766c260c80956b44"),
            10_000,
            200,
            U256::from_str_radix("665aef7589c635534122e931e613", 16).unwrap(),
            203_482,
            u128::from_str_radix("7cbf9d9985f0629c56e", 16).unwrap(),
        )
        .unwrap();
        pool.add_position(
            -887_200,
            204_200,
            u128::from_str_radix("7cbf9d9985f0629c56e", 16).unwrap(),
        )
        .unwrap();
        // The position was minted out of range and then activated by the launcher's
        // initial buy. Restore the final Swap observation from receipt log 0x24.
        pool.set_observation(
            U256::from_str_radix("665aef7589c635534122e931e613", 16).unwrap(),
            203_482,
            u128::from_str_radix("7cbf9d9985f0629c56e", 16).unwrap(),
        )
        .unwrap();
        pool
    }

    #[test]
    fn matches_first_public_swap_after_live_noxa_launch() {
        // Launch tx c62997... ended at L2 block 0x68fd86 / L1 0x1853bf3.
        // The first later pool Swap was tx 455d4d... at the first eligible L1
        // context 0x1853bf5. Its event is an exact differential oracle.
        let pool = live_launch_pool();
        let quote = pool
            .quote_exact_input(pool.token0, U256::from(12_000_000_000_000_000_u64), None)
            .unwrap();
        assert_eq!(
            quote.amount_out,
            U256::from_str_radix("6b0c664736ce5a3db06f7", 16).unwrap()
        );
        assert_eq!(
            quote.sqrt_price_x96_after,
            U256::from_str_radix("657f421942f1ecfd3c158b1a091b", 16).unwrap()
        );
        assert_eq!(quote.tick_after, 203_314);
        assert_eq!(quote.amount_in_consumed, quote.amount_in_requested);
    }

    #[test]
    fn supports_reverse_direction_and_preserves_input_state() {
        let pool = live_launch_pool();
        let quote = pool
            .quote_exact_input(
                pool.token1,
                U256::from(1_000_000_000_000_000_000_u128),
                None,
            )
            .unwrap();
        assert_eq!(quote.token_out, pool.token0);
        assert!(quote.amount_out > U256::ZERO);
        assert_eq!(pool.tick, 203_482);
    }

    #[test]
    fn rejects_unknown_tokens_and_wrong_limits() {
        let pool = live_launch_pool();
        assert!(matches!(
            pool.quote_exact_input(Address::with_last_byte(99), U256::from(1), None),
            Err(V3PoolError::UnknownInputToken(_))
        ));
        assert!(matches!(
            pool.quote_exact_input(pool.token0, U256::from(1), Some(pool.sqrt_price_x96)),
            Err(V3PoolError::InvalidPriceLimit)
        ));
    }

    fn empty_pool_at(tick: i32) -> V3PoolState {
        V3PoolState::new(
            Address::with_last_byte(3),
            Address::with_last_byte(1),
            Address::with_last_byte(2),
            3_000,
            1,
            get_sqrt_ratio_at_tick(tick).unwrap(),
            tick,
            0,
        )
        .unwrap()
    }

    #[test]
    fn traverses_multiword_empty_gap_one_for_zero() {
        let mut pool = empty_pool_at(0);
        pool.add_position(600, 700, 1_000_000).unwrap();

        let quote = pool
            .quote_exact_input(pool.token1, U256::from(1_000), None)
            .unwrap();

        assert_eq!(quote.amount_in_consumed, quote.amount_in_requested);
        assert!(quote.amount_out > U256::ZERO);
        assert_eq!(quote.initialized_ticks_crossed, 1);
        assert!(quote.steps >= 4);
        assert!(quote.tick_after >= 600);
    }

    #[test]
    fn traverses_multiword_empty_gap_zero_for_one() {
        let mut pool = empty_pool_at(0);
        pool.add_position(-700, -600, 1_000_000).unwrap();

        let quote = pool
            .quote_exact_input(pool.token0, U256::from(1_000), None)
            .unwrap();

        assert_eq!(quote.amount_in_consumed, quote.amount_in_requested);
        assert!(quote.amount_out > U256::ZERO);
        assert_eq!(quote.initialized_ticks_crossed, 1);
        assert!(quote.steps >= 4);
        assert!(quote.tick_after < -600);
    }

    #[test]
    fn failed_upper_tick_update_rolls_back_entire_position() {
        let mut pool = empty_pool_at(100);
        pool.add_position(-20, 10, i128::MAX as u128).unwrap();
        let before = pool.clone();

        assert!(matches!(
            pool.add_position(-10, 10, 2),
            Err(V3PoolError::ArithmeticOverflow)
        ));
        assert_eq!(pool, before);
    }

    #[test]
    fn failed_active_liquidity_update_rolls_back_ticks() {
        let mut pool = empty_pool_at(0);
        pool.liquidity = u128::MAX;
        let before = pool.clone();

        assert!(pool.add_position(-1, 1, 1).is_err());
        assert_eq!(pool, before);
    }
}
