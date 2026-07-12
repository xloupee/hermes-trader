use std::collections::HashMap;

use alloy_primitives::{Address, U256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const FEE_NUMERATOR: u64 = 997;
const FEE_DENOMINATOR: u64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PairSnapshot {
    pub pair: Address,
    pub token0: Address,
    pub token1: Address,
    pub reserve0: U256,
    pub reserve1: U256,
    pub block_number: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HopQuote {
    pub pair: Address,
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: U256,
    pub amount_out: U256,
    pub reserve_in_before: U256,
    pub reserve_out_before: U256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrderedCopyQuote {
    pub snapshot_block: u64,
    pub leader_amount_out: U256,
    pub follower_amount_in: U256,
    pub follower_amount_out: U256,
    pub leader_hops: Vec<HopQuote>,
    pub follower_hops: Vec<HopQuote>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum QuoteError {
    #[error("swap path needs at least two non-zero token addresses")]
    InvalidPath,
    #[error("input amount must be greater than zero")]
    ZeroInput,
    #[error("pair snapshot has identical or zero token addresses")]
    InvalidPair,
    #[error("duplicate pair snapshot for token pair")]
    DuplicatePair,
    #[error("pair snapshot is missing for hop {token_in}->{token_out}")]
    MissingPair {
        token_in: Address,
        token_out: Address,
    },
    #[error("pair reserves are zero for {pair}")]
    ZeroReserve { pair: Address },
    #[error("snapshot block {snapshot_block} is older than minimum {minimum_block}")]
    StaleSnapshot {
        snapshot_block: u64,
        minimum_block: u64,
    },
    #[error("checked V2 quote arithmetic overflowed")]
    ArithmeticOverflow,
    #[error("V2 quote rounded to zero")]
    ZeroOutput,
    #[error("reserve update referenced unknown pair {pair}")]
    UnknownPair { pair: Address },
}

#[derive(Debug, Clone, Default)]
pub struct ReserveBook {
    pairs: HashMap<(Address, Address), PairSnapshot>,
}

#[derive(Debug, Clone, Default)]
pub struct ReserveCache {
    book: ReserveBook,
    keys_by_pair: HashMap<Address, (Address, Address)>,
}

impl ReserveCache {
    pub fn len(&self) -> usize {
        self.book.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.book.pairs.is_empty()
    }

    pub fn upsert_snapshot(&mut self, snapshot: PairSnapshot) -> Result<bool, QuoteError> {
        if snapshot.token0 == Address::ZERO
            || snapshot.token1 == Address::ZERO
            || snapshot.token0 == snapshot.token1
        {
            return Err(QuoteError::InvalidPair);
        }
        let key = pair_key(snapshot.token0, snapshot.token1);
        if let Some(existing_key) = self.keys_by_pair.get(&snapshot.pair)
            && *existing_key != key
        {
            return Err(QuoteError::DuplicatePair);
        }
        if let Some(existing) = self.book.pairs.get(&key)
            && existing.block_number > snapshot.block_number
        {
            return Ok(false);
        }
        self.keys_by_pair.insert(snapshot.pair, key);
        self.book.pairs.insert(key, snapshot);
        Ok(true)
    }

    pub fn apply_sync(
        &mut self,
        pair: Address,
        reserve0: U256,
        reserve1: U256,
        block_number: u64,
    ) -> Result<bool, QuoteError> {
        let key = *self
            .keys_by_pair
            .get(&pair)
            .ok_or(QuoteError::UnknownPair { pair })?;
        let snapshot = self
            .book
            .pairs
            .get_mut(&key)
            .ok_or(QuoteError::UnknownPair { pair })?;
        if block_number < snapshot.block_number {
            return Ok(false);
        }
        snapshot.reserve0 = reserve0;
        snapshot.reserve1 = reserve1;
        snapshot.block_number = block_number;
        Ok(true)
    }

    pub fn path_book(
        &self,
        path: &[Address],
        minimum_snapshot_block: u64,
    ) -> Result<ReserveBook, QuoteError> {
        validate_path(path)?;
        let mut snapshots = Vec::with_capacity(path.len() - 1);
        let mut seen = std::collections::HashSet::new();
        for hop in path.windows(2) {
            let key = pair_key(hop[0], hop[1]);
            let snapshot = self.book.pairs.get(&key).ok_or(QuoteError::MissingPair {
                token_in: hop[0],
                token_out: hop[1],
            })?;
            if snapshot.block_number < minimum_snapshot_block {
                return Err(QuoteError::StaleSnapshot {
                    snapshot_block: snapshot.block_number,
                    minimum_block: minimum_snapshot_block,
                });
            }
            if seen.insert(key) {
                snapshots.push(snapshot.clone());
            }
        }
        ReserveBook::from_snapshots(snapshots)
    }

    pub fn snapshots(&self) -> Vec<PairSnapshot> {
        let mut snapshots: Vec<_> = self.book.pairs.values().cloned().collect();
        snapshots.sort_by_key(|snapshot| snapshot.pair);
        snapshots
    }
}

impl ReserveBook {
    pub fn from_snapshots(
        snapshots: impl IntoIterator<Item = PairSnapshot>,
    ) -> Result<Self, QuoteError> {
        let mut book = Self::default();
        for snapshot in snapshots {
            book.insert(snapshot)?;
        }
        Ok(book)
    }

    pub fn insert(&mut self, snapshot: PairSnapshot) -> Result<(), QuoteError> {
        if snapshot.token0 == Address::ZERO
            || snapshot.token1 == Address::ZERO
            || snapshot.token0 == snapshot.token1
        {
            return Err(QuoteError::InvalidPair);
        }
        let key = pair_key(snapshot.token0, snapshot.token1);
        if self.pairs.insert(key, snapshot).is_some() {
            return Err(QuoteError::DuplicatePair);
        }
        Ok(())
    }

    pub fn simulate_leader_then_follower(
        &self,
        path: &[Address],
        leader_amount_in: U256,
        follower_amount_in: U256,
        minimum_snapshot_block: u64,
    ) -> Result<OrderedCopyQuote, QuoteError> {
        validate_path(path)?;
        if leader_amount_in == U256::ZERO || follower_amount_in == U256::ZERO {
            return Err(QuoteError::ZeroInput);
        }
        let mut working = self.clone();
        let leader_hops = working.apply_path(path, leader_amount_in, minimum_snapshot_block)?;
        let follower_hops = working.apply_path(path, follower_amount_in, minimum_snapshot_block)?;
        let snapshot_block = leader_hops
            .iter()
            .chain(&follower_hops)
            .map(|hop| working.pairs[&pair_key(hop.token_in, hop.token_out)].block_number)
            .min()
            .unwrap_or_default();
        Ok(OrderedCopyQuote {
            snapshot_block,
            leader_amount_out: leader_hops
                .last()
                .expect("validated path has a hop")
                .amount_out,
            follower_amount_in,
            follower_amount_out: follower_hops
                .last()
                .expect("validated path has a hop")
                .amount_out,
            leader_hops,
            follower_hops,
        })
    }

    fn apply_path(
        &mut self,
        path: &[Address],
        initial_amount_in: U256,
        minimum_snapshot_block: u64,
    ) -> Result<Vec<HopQuote>, QuoteError> {
        let mut amount_in = initial_amount_in;
        let mut quotes = Vec::with_capacity(path.len() - 1);
        for hop in path.windows(2) {
            let token_in = hop[0];
            let token_out = hop[1];
            let key = pair_key(token_in, token_out);
            let snapshot = self.pairs.get_mut(&key).ok_or(QuoteError::MissingPair {
                token_in,
                token_out,
            })?;
            if snapshot.block_number < minimum_snapshot_block {
                return Err(QuoteError::StaleSnapshot {
                    snapshot_block: snapshot.block_number,
                    minimum_block: minimum_snapshot_block,
                });
            }
            let input_is_token0 = token_in == snapshot.token0;
            let (reserve_in, reserve_out) = if input_is_token0 {
                (snapshot.reserve0, snapshot.reserve1)
            } else {
                (snapshot.reserve1, snapshot.reserve0)
            };
            if reserve_in == U256::ZERO || reserve_out == U256::ZERO {
                return Err(QuoteError::ZeroReserve {
                    pair: snapshot.pair,
                });
            }
            let amount_out = get_amount_out(amount_in, reserve_in, reserve_out)?;
            let new_reserve_in = reserve_in
                .checked_add(amount_in)
                .ok_or(QuoteError::ArithmeticOverflow)?;
            let new_reserve_out = reserve_out
                .checked_sub(amount_out)
                .ok_or(QuoteError::ArithmeticOverflow)?;
            if input_is_token0 {
                snapshot.reserve0 = new_reserve_in;
                snapshot.reserve1 = new_reserve_out;
            } else {
                snapshot.reserve1 = new_reserve_in;
                snapshot.reserve0 = new_reserve_out;
            }
            quotes.push(HopQuote {
                pair: snapshot.pair,
                token_in,
                token_out,
                amount_in,
                amount_out,
                reserve_in_before: reserve_in,
                reserve_out_before: reserve_out,
            });
            amount_in = amount_out;
        }
        Ok(quotes)
    }
}

pub fn get_amount_out(
    amount_in: U256,
    reserve_in: U256,
    reserve_out: U256,
) -> Result<U256, QuoteError> {
    if amount_in == U256::ZERO {
        return Err(QuoteError::ZeroInput);
    }
    if reserve_in == U256::ZERO || reserve_out == U256::ZERO {
        return Err(QuoteError::ZeroReserve {
            pair: Address::ZERO,
        });
    }
    let amount_in_with_fee = amount_in
        .checked_mul(U256::from(FEE_NUMERATOR))
        .ok_or(QuoteError::ArithmeticOverflow)?;
    let numerator = amount_in_with_fee
        .checked_mul(reserve_out)
        .ok_or(QuoteError::ArithmeticOverflow)?;
    let denominator = reserve_in
        .checked_mul(U256::from(FEE_DENOMINATOR))
        .and_then(|value| value.checked_add(amount_in_with_fee))
        .ok_or(QuoteError::ArithmeticOverflow)?;
    let amount_out = numerator / denominator;
    if amount_out == U256::ZERO {
        return Err(QuoteError::ZeroOutput);
    }
    Ok(amount_out)
}

fn validate_path(path: &[Address]) -> Result<(), QuoteError> {
    if path.len() < 2
        || path.iter().any(|token| *token == Address::ZERO)
        || path.windows(2).any(|hop| hop[0] == hop[1])
    {
        return Err(QuoteError::InvalidPath);
    }
    Ok(())
}

fn pair_key(left: Address, right: Address) -> (Address, Address) {
    if left < right {
        (left, right)
    } else {
        (right, left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(last: u8) -> Address {
        Address::with_last_byte(last)
    }

    fn snapshot(token0: Address, token1: Address, reserve0: u64, reserve1: u64) -> PairSnapshot {
        PairSnapshot {
            pair: address(99),
            token0,
            token1,
            reserve0: U256::from(reserve0),
            reserve1: U256::from(reserve1),
            block_number: 50,
        }
    }

    #[test]
    fn matches_v2_constant_product_quote() {
        assert_eq!(
            get_amount_out(U256::from(100), U256::from(1_000), U256::from(1_000)).unwrap(),
            U256::from(90)
        );
    }

    #[test]
    fn follower_is_quoted_after_leader_price_impact() {
        let token0 = address(1);
        let token1 = address(2);
        let book = ReserveBook::from_snapshots([snapshot(token0, token1, 1_000, 1_000)]).unwrap();
        let quote = book
            .simulate_leader_then_follower(&[token0, token1], U256::from(100), U256::from(25), 50)
            .unwrap();
        assert_eq!(quote.leader_amount_out, U256::from(90));
        assert_eq!(quote.follower_amount_out, U256::from(20));
        assert_eq!(quote.follower_hops[0].reserve_in_before, U256::from(1_100));
        assert_eq!(quote.follower_hops[0].reserve_out_before, U256::from(910));
    }

    #[test]
    fn supports_reverse_direction() {
        let token0 = address(1);
        let token1 = address(2);
        let book = ReserveBook::from_snapshots([snapshot(token0, token1, 2_000, 1_000)]).unwrap();
        let quote = book
            .simulate_leader_then_follower(&[token1, token0], U256::from(100), U256::from(25), 50)
            .unwrap();
        assert_eq!(quote.leader_hops[0].reserve_in_before, U256::from(1_000));
        assert_eq!(quote.leader_hops[0].reserve_out_before, U256::from(2_000));
    }

    #[test]
    fn rejects_missing_and_stale_snapshots() {
        let token0 = address(1);
        let token1 = address(2);
        let missing = ReserveBook::default()
            .simulate_leader_then_follower(&[token0, token1], U256::from(100), U256::from(25), 0)
            .unwrap_err();
        assert!(matches!(missing, QuoteError::MissingPair { .. }));

        let book = ReserveBook::from_snapshots([snapshot(token0, token1, 1_000, 1_000)]).unwrap();
        let stale = book
            .simulate_leader_then_follower(&[token0, token1], U256::from(100), U256::from(25), 51)
            .unwrap_err();
        assert!(matches!(stale, QuoteError::StaleSnapshot { .. }));
    }

    #[test]
    fn cache_applies_monotonic_sync_updates() {
        let token0 = address(1);
        let token1 = address(2);
        let pair = address(99);
        let mut cache = ReserveCache::default();
        assert!(
            cache
                .upsert_snapshot(snapshot(token0, token1, 1_000, 1_000))
                .unwrap()
        );
        assert!(
            cache
                .apply_sync(pair, U256::from(1_100), U256::from(900), 51)
                .unwrap()
        );
        assert!(
            !cache
                .apply_sync(pair, U256::from(500), U256::from(500), 50)
                .unwrap()
        );
        let book = cache.path_book(&[token0, token1], 51).unwrap();
        let quote = book
            .simulate_leader_then_follower(&[token0, token1], U256::from(100), U256::from(25), 51)
            .unwrap();
        assert_eq!(quote.leader_hops[0].reserve_in_before, U256::from(1_100));
        assert_eq!(quote.leader_hops[0].reserve_out_before, U256::from(900));
    }
}
