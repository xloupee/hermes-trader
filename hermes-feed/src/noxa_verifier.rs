use std::time::{Duration, Instant};

use alloy_primitives::{Address, B256, U256};
use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::noxa_abi::{NoxaLaunchEvent, NoxaLaunchHeader, NoxaLaunchIntent, decode_launch_call};
use crate::noxa_launch::hydrate_noxa_launch_receipt;
use crate::noxa_rpc::{NoxaRpcClient, RobinhoodBlock, TokenRestrictionSnapshot};
use crate::robinhood::{NOXA_LAUNCH_FACTORY, NOXA_POOL_FEE, UNISWAP_V3_FACTORY, WETH};
use crate::v3_pool::V3Quote;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObservedNoxaFactoryCall {
    pub tx_hash: B256,
    pub sequence_number: u64,
    pub feed_l1_block: u64,
    pub feed_l1_timestamp: u64,
    pub observed_unix_ns: u128,
    pub header: NoxaLaunchHeader,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerifiedNoxaLaunch {
    pub observation: ObservedNoxaFactoryCall,
    pub receipt_l2_block: u64,
    pub transaction_index: u64,
    pub receipt_visibility_ns: u128,
    pub verification_total_ns: u128,
    pub block: RobinhoodBlock,
    pub intent: NoxaLaunchIntent,
    pub launch: NoxaLaunchEvent,
    pub quote: V3Quote,
    pub restrictions: TokenRestrictionSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum NoxaVerificationOutcome {
    Reverted {
        observation: Box<ObservedNoxaFactoryCall>,
        receipt_l2_block: u64,
        transaction_index: u64,
        receipt_visibility_ns: u128,
    },
    Verified(Box<VerifiedNoxaLaunch>),
}

/// Prove a feed-observed launch against its receipt and pinned contract views.
/// This runs asynchronously before the `launch + 1` boundary and returns typed
/// state for the no-RPC candidate builder.
pub async fn verify_noxa_factory_call(
    rpc: &NoxaRpcClient,
    observation: ObservedNoxaFactoryCall,
    amount_in: U256,
    recipient: Option<Address>,
    receipt_visibility_deadline: Duration,
) -> Result<NoxaVerificationOutcome> {
    if amount_in == U256::ZERO || receipt_visibility_deadline.is_zero() {
        bail!("verification amount and deadline must be non-zero");
    }
    let started = Instant::now();
    let deadline = started + receipt_visibility_deadline;
    let receipt = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("receipt not visible before verification deadline");
        }
        let receipt = tokio::time::timeout(remaining, rpc.receipt(observation.tx_hash))
            .await
            .context("receipt lookup exceeded visibility deadline")??;
        if let Some(receipt) = receipt {
            break receipt;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("receipt not visible before verification deadline");
        }
        tokio::time::sleep(remaining.min(Duration::from_millis(25))).await;
    };
    let receipt_visibility_ns = started.elapsed().as_nanos();
    if receipt.transaction_hash != observation.tx_hash {
        bail!("receipt transaction hash does not match observed hash");
    }
    if !receipt.status {
        return Ok(NoxaVerificationOutcome::Reverted {
            observation: Box::new(observation),
            receipt_l2_block: receipt.l2_block_number,
            transaction_index: receipt.transaction_index,
            receipt_visibility_ns,
        });
    }
    let transaction = rpc
        .transaction_by_hash(observation.tx_hash)
        .await?
        .ok_or_else(|| anyhow::anyhow!("successful launch transaction is not visible"))?;
    if transaction.hash != observation.tx_hash || transaction.to != Some(NOXA_LAUNCH_FACTORY) {
        bail!("verified transaction does not target the canonical NOXA factory");
    }
    let intent = decode_launch_call(&transaction.input, transaction.value).ok_or_else(|| {
        anyhow::anyhow!("successful transaction is not a strict launchToken call")
    })?;
    if intent.launch_config_id != observation.header.launch_config_id
        || intent.dex_id != observation.header.dex_id
        || intent.salt != observation.header.salt
        || intent.transaction_value != observation.header.transaction_value
    {
        bail!("strict launch calldata does not match feed hot-path header");
    }
    let block = rpc.block_by_number(receipt.l2_block_number).await?;
    if block.l1_block_number != observation.feed_l1_block {
        bail!("feed L1 block does not match receipt block L1 height");
    }
    let hydrated = hydrate_noxa_launch_receipt(
        &receipt.logs,
        observation.feed_l1_block,
        receipt.l2_block_number,
    )?;
    if hydrated.launch.dex_id != intent.dex_id
        || hydrated.launch.launch_config_id != intent.launch_config_id
    {
        bail!("launch calldata and receipt IDs do not match");
    }
    let restrictions = rpc
        .token_restriction_snapshot(hydrated.launch.token, receipt.l2_block_number, recipient)
        .await?;
    validate_verified_restrictions(&restrictions, &hydrated.launch)?;
    let launch_fee = rpc.launch_fee_at(receipt.l2_block_number).await?;
    let provisional_initial_buy = intent
        .transaction_value
        .checked_sub(launch_fee)
        .ok_or_else(|| anyhow::anyhow!("transaction value below launch fee"))?;
    if provisional_initial_buy != hydrated.launch.initial_buy_amount {
        bail!("transaction value minus launch fee does not match launch event");
    }
    let quote = hydrated.pool.quote_exact_input(WETH, amount_in, None)?;
    Ok(NoxaVerificationOutcome::Verified(Box::new(
        VerifiedNoxaLaunch {
            observation,
            receipt_l2_block: receipt.l2_block_number,
            transaction_index: receipt.transaction_index,
            receipt_visibility_ns,
            verification_total_ns: started.elapsed().as_nanos(),
            block,
            intent,
            launch: hydrated.launch,
            quote,
            restrictions,
        },
    )))
}

pub fn validate_verified_restrictions(
    snapshot: &TokenRestrictionSnapshot,
    launch: &NoxaLaunchEvent,
) -> Result<()> {
    if snapshot.token != launch.token
        || snapshot.launch_factory != NOXA_LAUNCH_FACTORY
        || snapshot.liquidity_pool != launch.pool
        || snapshot.pair_token != WETH
        || snapshot.pool_fee != NOXA_POOL_FEE
        || snapshot.restriction_end_block != launch.restrictions_end_l1_block
        || launch.dex_factory != UNISWAP_V3_FACTORY
        || snapshot.max_wallet_limit == U256::ZERO
        || snapshot.max_tx_limit == U256::ZERO
    {
        bail!("launched-token views do not match the verified launch receipt");
    }
    if snapshot.recipient.is_some() != snapshot.recipient_balance.is_some() {
        bail!("recipient balance snapshot is incomplete");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restriction_validation_rejects_wrong_pool() {
        let launch = NoxaLaunchEvent {
            token: Address::with_last_byte(1),
            deployer: Address::with_last_byte(2),
            dex_factory: UNISWAP_V3_FACTORY,
            pair_token: WETH,
            pool: Address::with_last_byte(3),
            dex_id: U256::ZERO,
            launch_config_id: U256::ZERO,
            position_id: U256::from(1),
            restrictions_end_l1_block: U256::from(100),
            initial_buy_amount: U256::from(1),
        };
        let snapshot = TokenRestrictionSnapshot {
            token: launch.token,
            l2_block_number: 1,
            launch_factory: NOXA_LAUNCH_FACTORY,
            liquidity_pool: Address::with_last_byte(4),
            pair_token: WETH,
            pool_fee: NOXA_POOL_FEE,
            max_wallet_limit: U256::from(1),
            max_tx_limit: U256::from(1),
            restriction_end_block: launch.restrictions_end_l1_block,
            recipient: None,
            recipient_balance: None,
        };
        assert!(validate_verified_restrictions(&snapshot, &launch).is_err());
    }
}
