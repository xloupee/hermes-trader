//! Validation rules for the independently audited active N0xa deployment.
//!
//! These checks are intentionally not shared with the retired NOXA deployment:
//! an otherwise similar token must not pass through the active copy path.

use alloy_primitives::{Address, U256};
use anyhow::{Result, bail};

use crate::noxa_predict::{NoxaLaunchConfig, predict_v3_pool_address};
use crate::noxa_rpc::{ActiveNoxaLaunchRecord, ActiveNoxaTokenSnapshot, V3PoolSnapshot};
use crate::robinhood::{
    ACTIVE_NOXA_LAUNCH_FACTORY, NOXA_POOL_FEE, UNISWAP_V3_FACTORY,
    UNISWAP_V3_POOL_INIT_CODE_KECCAK256, WETH,
};

/// Verify the factory record, token immutable views, and canonical V3 pool for
/// a token before an aggregator swap becomes copy-eligible. The caller must
/// also have pinned the active factory's embedded token creation bytecode via
/// [`crate::NoxaPredictor::new_active`].
pub fn validate_active_noxa_copy_token(
    token: Address,
    record: &ActiveNoxaLaunchRecord,
    token_view: &ActiveNoxaTokenSnapshot,
    pool_view: &V3PoolSnapshot,
    pool_runtime: &[u8],
    config: &NoxaLaunchConfig,
) -> Result<()> {
    let max_wallet = config
        .supply
        .checked_mul(config.max_wallet_bps)
        .ok_or_else(|| anyhow::anyhow!("active N0xa max-wallet calculation overflow"))?
        / U256::from(10_000_u64);
    let expected_max_tx = if config.max_tx_bps >= U256::from(10_000_u64) {
        U256::ZERO
    } else {
        config
            .supply
            .checked_mul(config.max_tx_bps)
            .ok_or_else(|| anyhow::anyhow!("active N0xa max-tx calculation overflow"))?
            / U256::from(10_000_u64)
    };
    let (token0, token1) = if token < WETH {
        (token, WETH)
    } else {
        (WETH, token)
    };
    let expected_pool = predict_v3_pool_address(
        UNISWAP_V3_FACTORY,
        token0,
        token1,
        NOXA_POOL_FEE,
        UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    );
    if record.token != token
        || record.pair_token != WETH
        || record.pool != expected_pool
        || record.dex_id != U256::ZERO
        || record.launch_config_id != U256::ZERO
        || record.restrictions_end_block == U256::ZERO
        || token_view.token != token
        || token_view.factory != ACTIVE_NOXA_LAUNCH_FACTORY
        || token_view.restrictions_end_block != record.restrictions_end_block
        || token_view.max_wallet_amount != max_wallet
        || token_view.max_tx_amount != expected_max_tx
        || pool_view.pool != expected_pool
        || pool_view.fee != NOXA_POOL_FEE
        || pool_view.liquidity == 0
        // Solidity immutables (token addresses, fee, and tick spacing) are
        // embedded in every V3 pool's runtime bytecode. Consequently a full
        // runtime hash differs for each legitimate pool and cannot be pinned
        // globally. The pinned factory + CREATE2 address + queried immutable
        // pool fields above are the identity proof; code must merely exist.
        || pool_runtime.is_empty()
        || !((pool_view.token0 == WETH && pool_view.token1 == token)
            || (pool_view.token0 == token && pool_view.token1 == WETH))
    {
        bail!("token does not satisfy the pinned active N0xa copy-policy deployment")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use alloy_primitives::Address;

    use super::*;

    fn valid_fixture() -> (
        Address,
        NoxaLaunchConfig,
        ActiveNoxaLaunchRecord,
        ActiveNoxaTokenSnapshot,
        V3PoolSnapshot,
    ) {
        let token = Address::with_last_byte(1);
        let config = NoxaLaunchConfig {
            pair_token: WETH,
            dex_id: U256::ZERO,
            initial_tick: -204_200,
            supply: U256::from(1_000_000_000_u64) * U256::from(1_000_000_000_000_000_000_u64),
            max_wallet_bps: U256::from(200),
            max_tx_bps: U256::from(10_000),
            restriction_l1_blocks: 366,
            flags: [false, true, false],
        };
        let pool = predict_v3_pool_address(
            UNISWAP_V3_FACTORY,
            token,
            WETH,
            NOXA_POOL_FEE,
            UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
        );
        let record = ActiveNoxaLaunchRecord {
            token,
            deployer: Address::ZERO,
            fee_wallet: Address::ZERO,
            pair_token: WETH,
            pool,
            dex_id: U256::ZERO,
            launch_config_id: U256::ZERO,
            position_id: U256::ZERO,
            restrictions_end_block: U256::from(1),
            initial_buy_amount: U256::ZERO,
            created_at_block: U256::ZERO,
            is_token0: true,
        };
        let token_view = ActiveNoxaTokenSnapshot {
            token,
            l2_block_number: 1,
            factory: ACTIVE_NOXA_LAUNCH_FACTORY,
            max_wallet_amount: config.supply / U256::from(50),
            max_tx_amount: U256::ZERO,
            restrictions_end_block: U256::from(1),
        };
        let pool_view = V3PoolSnapshot {
            pool,
            token0: token,
            token1: WETH,
            fee: NOXA_POOL_FEE,
            liquidity: 1,
        };
        (token, config, record, token_view, pool_view)
    }

    #[test]
    fn accepts_canonical_pool_with_nonempty_instance_runtime() {
        let (token, config, record, token_view, pool_view) = valid_fixture();

        validate_active_noxa_copy_token(token, &record, &token_view, &pool_view, &[0x60], &config)
            .unwrap();
    }

    #[test]
    fn rejects_canonical_pool_without_runtime() {
        let (token, config, record, token_view, pool_view) = valid_fixture();

        assert!(
            validate_active_noxa_copy_token(token, &record, &token_view, &pool_view, &[], &config,)
                .is_err()
        );
    }

    #[test]
    fn rejects_legacy_factory_even_with_a_canonical_pool() {
        let (token, config, record, mut token_view, pool_view) = valid_fixture();
        token_view.factory = Address::ZERO;
        assert!(
            validate_active_noxa_copy_token(
                token,
                &record,
                &token_view,
                &pool_view,
                &[0x60],
                &config,
            )
            .is_err()
        );
    }
}
