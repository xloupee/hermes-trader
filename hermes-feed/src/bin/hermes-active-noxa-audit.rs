use alloy_primitives::{Address, B256, U256, keccak256};
use anyhow::{Context, Result, bail};
use clap::Parser;
use hermes_feed::{
    NoxaPredictor, NoxaRpcClient, decode_launch_call, decode_token_launched,
    validate_active_noxa_copy_token,
};
use serde_json::json;

const ACTIVE_NOXA_FACTORY: Address =
    alloy_primitives::address!("52453b4289a6c3a70bb8b4682bcd3d8731267e28");
const TOKEN_CREATION_CODE_OFFSET: usize = 10_303;
const TOKEN_CREATION_CODE_END: usize = 14_037;
const LATER_LAUNCH_TX: B256 =
    alloy_primitives::b256!("66077159478a6b35774e6301f0a2f60f750993cf5592995a5f4ec5351398d83b");
const ACTIVE_LAUNCH_FIXTURES: [B256; 3] = [
    LATER_LAUNCH_TX,
    alloy_primitives::b256!("b43e13794ca0360cff28c84423eaf2e3878b7187d5e68595d9d67d42b406f290"),
    alloy_primitives::b256!("d27e92bfe5ee3374c77fac387b0b8d3a6cf11a3a1dedf7169b7e1d828e191c4a"),
];

#[derive(Debug, Parser)]
#[command(about = "Read-only audit of the active N0xa launch deployment")]
struct Cli {
    #[arg(long, default_value = "https://rpc.mainnet.chain.robinhood.com")]
    rpc_url: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    let status = rpc.factory_status_for(ACTIVE_NOXA_FACTORY).await?;
    let runtime = rpc
        .code_at_l2_block(ACTIVE_NOXA_FACTORY, status.pinned_l2_block)
        .await?;
    let token_creation_code = runtime
        .get(TOKEN_CREATION_CODE_OFFSET..TOKEN_CREATION_CODE_END)
        .context("active N0xa embedded LaunchToken creation code layout changed")?;
    if token_creation_code.len() != TOKEN_CREATION_CODE_END - TOKEN_CREATION_CODE_OFFSET {
        bail!("active N0xa embedded LaunchToken creation code length changed");
    }
    let owner = rpc
        .factory_owner_at_for(ACTIVE_NOXA_FACTORY, status.pinned_l2_block)
        .await?;
    let config = rpc
        .launch_config_at_for(ACTIVE_NOXA_FACTORY, U256::ZERO, status.pinned_l2_block)
        .await?;
    let dex = rpc
        .active_dex_config_at_for(ACTIVE_NOXA_FACTORY, U256::ZERO, status.pinned_l2_block)
        .await?;
    let dex_runtime = rpc
        .code_at_l2_block(dex.factory, status.pinned_l2_block)
        .await?;
    let predictor = NoxaPredictor::new_active(
        status.launch_fee,
        config.clone(),
        dex.clone(),
        &runtime,
        &dex_runtime,
    )?;
    let transaction = rpc
        .transaction_by_hash(LATER_LAUNCH_TX)
        .await?
        .context("later fixture transaction missing")?;
    let l2_block = transaction
        .l2_block_number
        .context("later fixture transaction is not mined")?;
    let block = rpc.block_by_number(l2_block).await?;
    let intent = decode_launch_call(&transaction.input, transaction.value)
        .context("later fixture did not decode as launchToken")?;
    let predicted = predictor.predict_active(&intent, transaction.from, block.l1_block_number)?;
    let receipt = rpc
        .receipt(LATER_LAUNCH_TX)
        .await?
        .context("later receipt missing")?;
    let actual = receipt
        .logs
        .iter()
        .filter(|log| log.address == ACTIVE_NOXA_FACTORY)
        .find_map(decode_token_launched)
        .context("later TokenLaunched event missing")?;
    if predicted.token != actual.token || predicted.pool != actual.pool {
        bail!("later fixture CREATE2 prediction does not match the emitted launch event");
    }
    let (launch_record, token_snapshot, token_runtime, pool_runtime, pool_snapshot) = tokio::try_join!(
        rpc.active_noxa_launch_record(ACTIVE_NOXA_FACTORY, actual.token, receipt.l2_block_number),
        rpc.active_noxa_token_snapshot(actual.token, receipt.l2_block_number),
        rpc.code_at_l2_block(actual.token, receipt.l2_block_number),
        rpc.code_at_l2_block(actual.pool, receipt.l2_block_number),
        rpc.v3_pool_snapshot_at(actual.pool, receipt.l2_block_number),
    )?;
    if launch_record.token != actual.token
        || launch_record.pool != actual.pool
        || launch_record.pair_token != config.pair_token
        || launch_record.dex_id != U256::ZERO
        || launch_record.launch_config_id != U256::ZERO
        || launch_record.restrictions_end_block != token_snapshot.restrictions_end_block
        || token_snapshot.factory != ACTIVE_NOXA_FACTORY
        || token_runtime.is_empty()
        || pool_runtime.is_empty()
        || pool_snapshot.fee != 10_000
        || pool_snapshot.liquidity == 0
    {
        bail!("later fixture token or canonical V3 pool validation failed");
    }
    validate_active_noxa_copy_token(
        actual.token,
        &launch_record,
        &token_snapshot,
        &pool_snapshot,
        &pool_runtime,
        &config,
    )?;
    for fixture_tx in ACTIVE_LAUNCH_FIXTURES.into_iter().skip(1) {
        let transaction = rpc
            .transaction_by_hash(fixture_tx)
            .await?
            .context("active N0xa fixture transaction missing")?;
        let fixture_block = transaction
            .l2_block_number
            .context("active N0xa fixture is unmined")?;
        let block = rpc.block_by_number(fixture_block).await?;
        let intent = decode_launch_call(&transaction.input, transaction.value)
            .context("active N0xa fixture did not decode as launchToken")?;
        let predicted =
            predictor.predict_active(&intent, transaction.from, block.l1_block_number)?;
        let receipt = rpc
            .receipt(fixture_tx)
            .await?
            .context("active N0xa fixture receipt missing")?;
        let actual = receipt
            .logs
            .iter()
            .filter(|log| log.address == ACTIVE_NOXA_FACTORY)
            .find_map(decode_token_launched)
            .context("active N0xa fixture TokenLaunched event missing")?;
        if predicted.token != actual.token || predicted.pool != actual.pool {
            bail!("active N0xa fixture CREATE2 prediction does not match event");
        }
    }
    println!(
        "{}",
        serde_json::to_string(&json!({
            "record_type": "active_n0xa_deployment_audit",
            "factory": ACTIVE_NOXA_FACTORY,
            "pinned_l2_block": status.pinned_l2_block,
            "factory_runtime_bytes": runtime.len(),
            "factory_runtime_keccak256": keccak256(&runtime),
            "token_creation_code_offset": TOKEN_CREATION_CODE_OFFSET,
            "token_creation_code_bytes": token_creation_code.len(),
            "token_creation_code_keccak256": keccak256(token_creation_code),
            "owner": owner,
            "launch_config": config,
            "dex_config": dex,
            "later_fixture": {
                "tx_hash": LATER_LAUNCH_TX,
                "predicted_token": predicted.token,
                "predicted_pool": predicted.pool,
                "event_token": actual.token,
                "event_pool": actual.pool,
                "exact_match": true,
                "additional_fixture_count": ACTIVE_LAUNCH_FIXTURES.len() - 1,
                "token_factory": token_snapshot.factory,
                "factory_record": launch_record,
                "restriction_end_block": token_snapshot.restrictions_end_block,
                "token_runtime_keccak256": keccak256(&token_runtime),
                "pool_runtime_keccak256": keccak256(&pool_runtime),
                "pool_fee": pool_snapshot.fee,
                "pool_liquidity": pool_snapshot.liquidity.to_string(),
            },
        }))?
    );
    Ok(())
}
