use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::rpc::{FactoryBootstrap, SyncUpdate};
use crate::v2_simulator::{PairSnapshot, QuoteError, ReserveCache};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheCheckpoint {
    pub chain_id: u64,
    pub factory: String,
    pub block_number: u64,
    pub block_hash: String,
    pub pairs: Vec<PairSnapshot>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CacheApplyReport {
    pub from_block: u64,
    pub to_block: u64,
    pub sync_logs: usize,
    pub applied_updates: usize,
    pub stale_updates: usize,
    pub unknown_pairs: usize,
}

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("cache update range starts at {actual}, expected {expected}")]
    BlockGap { expected: u64, actual: u64 },
    #[error("cache update range ends before it starts")]
    InvalidRange,
    #[error("checkpoint block hash changed at block {block_number}")]
    Reorg { block_number: u64 },
    #[error("factory tail is pinned to a different cache block")]
    TailBlockMismatch,
    #[error(transparent)]
    Quote(#[from] QuoteError),
}

#[derive(Debug, Clone)]
pub struct ConfirmedReserveCache {
    pub reserves: ReserveCache,
    pub block_number: u64,
    pub block_hash: String,
}

impl ConfirmedReserveCache {
    pub fn from_bootstrap(bootstrap: FactoryBootstrap) -> Result<Self, CacheError> {
        let mut reserves = ReserveCache::default();
        for pair in bootstrap.pairs {
            reserves.upsert_snapshot(pair)?;
        }
        Ok(Self {
            reserves,
            block_number: bootstrap.block_number,
            block_hash: bootstrap.block_hash,
        })
    }

    pub fn from_checkpoint(checkpoint: CacheCheckpoint) -> Result<Self, CacheError> {
        let mut reserves = ReserveCache::default();
        for pair in checkpoint.pairs {
            reserves.upsert_snapshot(pair)?;
        }
        Ok(Self {
            reserves,
            block_number: checkpoint.block_number,
            block_hash: checkpoint.block_hash,
        })
    }

    pub fn verify_checkpoint_hash(&self, canonical_hash: &str) -> Result<(), CacheError> {
        if self.block_hash != canonical_hash {
            return Err(CacheError::Reorg {
                block_number: self.block_number,
            });
        }
        Ok(())
    }

    pub fn apply_range(
        &mut self,
        from_block: u64,
        to_block: u64,
        to_block_hash: String,
        updates: &[SyncUpdate],
    ) -> Result<CacheApplyReport, CacheError> {
        if to_block < from_block {
            return Err(CacheError::InvalidRange);
        }
        let expected = self.block_number.saturating_add(1);
        if from_block != expected {
            return Err(CacheError::BlockGap {
                expected,
                actual: from_block,
            });
        }
        let mut report = CacheApplyReport {
            from_block,
            to_block,
            sync_logs: updates.len(),
            ..CacheApplyReport::default()
        };
        for update in updates {
            match self.reserves.apply_sync(
                update.pair,
                update.reserve0,
                update.reserve1,
                update.block_number,
            ) {
                Ok(true) => report.applied_updates += 1,
                Ok(false) => report.stale_updates += 1,
                Err(QuoteError::UnknownPair { .. }) => report.unknown_pairs += 1,
                Err(error) => return Err(error.into()),
            }
        }
        self.block_number = to_block;
        self.block_hash = to_block_hash;
        Ok(report)
    }

    pub fn checkpoint(&self, factory: String) -> CacheCheckpoint {
        CacheCheckpoint {
            chain_id: 4_663,
            factory,
            block_number: self.block_number,
            block_hash: self.block_hash.clone(),
            pairs: self.reserves.snapshots(),
        }
    }

    pub fn extend_registry(&mut self, tail: FactoryBootstrap) -> Result<usize, CacheError> {
        if tail.block_number != self.block_number || tail.block_hash != self.block_hash {
            return Err(CacheError::TailBlockMismatch);
        }
        let mut added = 0;
        for pair in tail.pairs {
            if self.reserves.upsert_snapshot(pair)? {
                added += 1;
            }
        }
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{Address, U256};

    use super::*;

    fn bootstrap() -> FactoryBootstrap {
        FactoryBootstrap {
            block_number: 10,
            block_hash: "0x10".into(),
            factory_pairs: 1,
            loaded_pairs: 1,
            pairs: vec![PairSnapshot {
                pair: Address::with_last_byte(9),
                token0: Address::with_last_byte(1),
                token1: Address::with_last_byte(2),
                reserve0: U256::from(100),
                reserve1: U256::from(200),
                block_number: 10,
            }],
        }
    }

    #[test]
    fn applies_contiguous_ranges_and_checkpoints() {
        let mut cache = ConfirmedReserveCache::from_bootstrap(bootstrap()).unwrap();
        let report = cache
            .apply_range(
                11,
                12,
                "0x12".into(),
                &[SyncUpdate {
                    pair: Address::with_last_byte(9),
                    reserve0: U256::from(110),
                    reserve1: U256::from(190),
                    block_number: 12,
                    log_index: 0,
                }],
            )
            .unwrap();
        assert_eq!(report.applied_updates, 1);
        let checkpoint = cache.checkpoint("0xfactory".into());
        assert_eq!(checkpoint.block_number, 12);
        assert_eq!(checkpoint.pairs[0].reserve0, U256::from(110));
    }

    #[test]
    fn rejects_skipped_ranges_and_reorged_checkpoint() {
        let mut cache = ConfirmedReserveCache::from_bootstrap(bootstrap()).unwrap();
        assert!(matches!(
            cache.apply_range(12, 12, "0x12".into(), &[]),
            Err(CacheError::BlockGap { .. })
        ));
        assert!(matches!(
            cache.verify_checkpoint_hash("0xdifferent"),
            Err(CacheError::Reorg { .. })
        ));
    }

    #[test]
    fn extends_registry_only_at_same_canonical_block() {
        let mut cache = ConfirmedReserveCache::from_bootstrap(bootstrap()).unwrap();
        let tail = FactoryBootstrap {
            block_number: 10,
            block_hash: "0x10".into(),
            factory_pairs: 2,
            loaded_pairs: 1,
            pairs: vec![PairSnapshot {
                pair: Address::with_last_byte(10),
                token0: Address::with_last_byte(3),
                token1: Address::with_last_byte(4),
                reserve0: U256::from(300),
                reserve1: U256::from(400),
                block_number: 10,
            }],
        };
        assert_eq!(cache.extend_registry(tail).unwrap(), 1);
        assert_eq!(cache.reserves.len(), 2);
    }
}
