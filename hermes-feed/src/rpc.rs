use std::collections::{HashMap, HashSet};
use std::time::Duration;

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::SolCall;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::v2_simulator::PairSnapshot;

const ROBINHOOD_CHAIN_ID: u64 = 4_663;
const GET_PAIR_SELECTOR: &str = "e6a43905";
const ALL_PAIRS_LENGTH_SELECTOR: &str = "0x574f2ba3";
const ALL_PAIRS_SELECTOR: &str = "1e3dd18b";
const TOKEN0_SELECTOR: &str = "0x0dfe1681";
const TOKEN1_SELECTOR: &str = "0xd21220a7";
const GET_RESERVES_SELECTOR: &str = "0x0902f1ac";
const SYNC_TOPIC: &str = "0x1c411e9a96e071241c2f21f7726b17ae89e3cab4c78be50e062b03a9fffbbad1";
const MULTICALL3: Address = alloy_primitives::address!("ca11bde05977b3631167028862be2a173976ca11");

mod multicall {
    use alloy_sol_types::sol;

    sol! {
        struct Call3 {
            address target;
            bool allowFailure;
            bytes callData;
        }

        struct Call3Result {
            bool success;
            bytes returnData;
        }

        function aggregate3(Call3[] calls) external payable returns (Call3Result[] returnData);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FactoryBootstrap {
    pub block_number: u64,
    pub block_hash: String,
    pub factory_pairs: usize,
    pub loaded_pairs: usize,
    pub pairs: Vec<PairSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncUpdate {
    pub pair: Address,
    pub reserve0: U256,
    pub reserve1: U256,
    pub block_number: u64,
    pub log_index: u64,
}

#[derive(Debug, Clone)]
pub struct V2SnapshotClient {
    endpoint: String,
    http: Client,
}

#[derive(Debug, Serialize)]
struct RpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    id: u64,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

impl V2SnapshotClient {
    pub fn new(endpoint: impl Into<String>) -> Result<Self> {
        let http = Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .build()
            .context("build HTTPS RPC client")?;
        Ok(Self {
            endpoint: endpoint.into(),
            http,
        })
    }

    pub async fn fetch_path(
        &self,
        factory: Address,
        path: &[Address],
    ) -> Result<Vec<PairSnapshot>> {
        if path.len() < 2 {
            anyhow::bail!("snapshot path needs at least two tokens");
        }
        let head = self
            .batch(vec![
                ("eth_chainId", json!([])),
                ("eth_blockNumber", json!([])),
            ])
            .await?;
        let chain_id = parse_quantity(value_string(&head[0], "eth_chainId")?)?;
        if chain_id != ROBINHOOD_CHAIN_ID {
            anyhow::bail!("RPC chain ID {chain_id} is not Robinhood Chain {ROBINHOOD_CHAIN_ID}");
        }
        let block_tag = value_string(&head[1], "eth_blockNumber")?.to_owned();
        let block_number = parse_quantity(&block_tag)?;

        let pair_calls = path
            .windows(2)
            .map(|hop| {
                (
                    "eth_call",
                    json!([{
                        "to": factory,
                        "data": encode_get_pair(hop[0], hop[1]),
                    }, block_tag.clone()]),
                )
            })
            .collect();
        let pair_results = self.batch(pair_calls).await?;
        let mut pairs = Vec::with_capacity(pair_results.len());
        let mut unique_pairs = HashSet::new();
        for (index, result) in pair_results.iter().enumerate() {
            let pair = decode_address(value_string(result, "factory getPair")?)?;
            if pair == Address::ZERO {
                anyhow::bail!("factory has no pair for path hop {index}");
            }
            if unique_pairs.insert(pair) {
                pairs.push(pair);
            }
        }

        let mut detail_calls = Vec::with_capacity(pairs.len() * 3);
        for pair in &pairs {
            for selector in [TOKEN0_SELECTOR, TOKEN1_SELECTOR, GET_RESERVES_SELECTOR] {
                detail_calls.push((
                    "eth_call",
                    json!([{"to": pair, "data": selector}, block_tag.clone()]),
                ));
            }
        }
        let details = self.batch(detail_calls).await?;
        pairs
            .into_iter()
            .enumerate()
            .map(|(index, pair)| {
                let offset = index * 3;
                let token0 = decode_address(value_string(&details[offset], "pair token0")?)?;
                let token1 = decode_address(value_string(&details[offset + 1], "pair token1")?)?;
                let (reserve0, reserve1) =
                    decode_reserves(value_string(&details[offset + 2], "pair getReserves")?)?;
                Ok(PairSnapshot {
                    pair,
                    token0,
                    token1,
                    reserve0,
                    reserve1,
                    block_number,
                })
            })
            .collect()
    }

    pub async fn bootstrap_factory(
        &self,
        factory: Address,
        pinned_block: Option<u64>,
        limit: Option<usize>,
        batch_size: usize,
    ) -> Result<FactoryBootstrap> {
        if batch_size == 0 {
            anyhow::bail!("RPC batch size must be greater than zero");
        }
        let head = self
            .batch(vec![
                ("eth_chainId", json!([])),
                ("eth_blockNumber", json!([])),
            ])
            .await?;
        let chain_id = parse_quantity(value_string(&head[0], "eth_chainId")?)?;
        if chain_id != ROBINHOOD_CHAIN_ID {
            anyhow::bail!("RPC chain ID {chain_id} is not Robinhood Chain {ROBINHOOD_CHAIN_ID}");
        }
        let head_block_tag = value_string(&head[1], "eth_blockNumber")?.to_owned();
        let head_block_number = parse_quantity(&head_block_tag)?;
        let block_number = pinned_block.unwrap_or(head_block_number);
        if block_number > head_block_number {
            anyhow::bail!("pinned block {block_number} is ahead of RPC head {head_block_number}");
        }
        let block_tag = format!("0x{block_number:x}");
        let metadata = self
            .batch(vec![
                (
                    "eth_call",
                    json!([{"to": factory, "data": ALL_PAIRS_LENGTH_SELECTOR}, block_tag]),
                ),
                ("eth_getBlockByNumber", json!([block_tag, false])),
            ])
            .await?;
        let factory_pairs = decode_u256(value_string(&metadata[0], "factory allPairsLength")?)?
            .try_into()
            .context("factory pair count does not fit usize")?;
        let block_hash = metadata[1]
            .get("hash")
            .and_then(Value::as_str)
            .context("pinned block omitted hash")?
            .to_owned();
        let count = limit.unwrap_or(factory_pairs).min(factory_pairs);
        let mut pair_addresses = Vec::with_capacity(count);
        for start in (0..count).step_by(batch_size) {
            let end = (start + batch_size).min(count);
            let calls = (start..end)
                .map(|index| Ok((factory, decode_hex(&encode_all_pairs(index))?)))
                .collect::<Result<Vec<_>>>()?;
            for result in self.multicall(&block_tag, calls).await? {
                let pair = decode_address_bytes(&result)?;
                if pair == Address::ZERO {
                    anyhow::bail!("factory allPairs returned zero address");
                }
                pair_addresses.push(pair);
            }
        }
        let pairs = self
            .fetch_pair_details_multicall(&pair_addresses, &block_tag, block_number, batch_size)
            .await?;
        Ok(FactoryBootstrap {
            block_number,
            block_hash,
            factory_pairs,
            loaded_pairs: pairs.len(),
            pairs,
        })
    }

    pub async fn sync_updates(&self, from_block: u64, to_block: u64) -> Result<Vec<SyncUpdate>> {
        if from_block > to_block {
            return Ok(Vec::new());
        }
        let result = self
            .batch(vec![(
                "eth_getLogs",
                json!([{
                    "fromBlock": format!("0x{from_block:x}"),
                    "toBlock": format!("0x{to_block:x}"),
                    "topics": [SYNC_TOPIC],
                }]),
            )])
            .await?;
        let logs = result[0]
            .as_array()
            .context("eth_getLogs returned a non-array result")?;
        let mut updates = Vec::with_capacity(logs.len());
        for log in logs {
            if log.get("removed").and_then(Value::as_bool) == Some(true) {
                anyhow::bail!("eth_getLogs returned a removed Sync log");
            }
            let pair = log
                .get("address")
                .and_then(Value::as_str)
                .context("Sync log omitted address")?
                .parse::<Address>()
                .context("Sync log has invalid pair address")?;
            let (reserve0, reserve1) = decode_sync_data(
                log.get("data")
                    .and_then(Value::as_str)
                    .context("Sync log omitted data")?,
            )?;
            updates.push(SyncUpdate {
                pair,
                reserve0,
                reserve1,
                block_number: parse_quantity(
                    log.get("blockNumber")
                        .and_then(Value::as_str)
                        .context("Sync log omitted blockNumber")?,
                )?,
                log_index: parse_quantity(
                    log.get("logIndex")
                        .and_then(Value::as_str)
                        .context("Sync log omitted logIndex")?,
                )?,
            });
        }
        updates.sort_by_key(|update| (update.block_number, update.log_index));
        Ok(updates)
    }

    pub async fn block_number(&self) -> Result<u64> {
        let result = self.batch(vec![("eth_blockNumber", json!([]))]).await?;
        parse_quantity(value_string(&result[0], "eth_blockNumber")?)
    }

    pub async fn factory_pair_count(&self, factory: Address) -> Result<usize> {
        let result = self
            .batch(vec![(
                "eth_call",
                json!([{"to": factory, "data": ALL_PAIRS_LENGTH_SELECTOR}, "latest"]),
            )])
            .await?;
        decode_u256(value_string(&result[0], "factory allPairsLength")?)?
            .try_into()
            .context("factory pair count does not fit usize")
    }

    pub async fn fetch_factory_tail(
        &self,
        factory: Address,
        start_index: usize,
        pinned_block: u64,
        batch_size: usize,
    ) -> Result<FactoryBootstrap> {
        if batch_size == 0 {
            anyhow::bail!("RPC batch size must be greater than zero");
        }
        let block_tag = format!("0x{pinned_block:x}");
        let metadata = self
            .batch(vec![
                ("eth_chainId", json!([])),
                (
                    "eth_call",
                    json!([{"to": factory, "data": ALL_PAIRS_LENGTH_SELECTOR}, block_tag]),
                ),
                ("eth_getBlockByNumber", json!([block_tag, false])),
            ])
            .await?;
        let chain_id = parse_quantity(value_string(&metadata[0], "eth_chainId")?)?;
        if chain_id != ROBINHOOD_CHAIN_ID {
            anyhow::bail!("RPC chain ID {chain_id} is not Robinhood Chain {ROBINHOOD_CHAIN_ID}");
        }
        let factory_pairs: usize =
            decode_u256(value_string(&metadata[1], "factory allPairsLength")?)?
                .try_into()
                .context("factory pair count does not fit usize")?;
        let block_hash = metadata[2]
            .get("hash")
            .and_then(Value::as_str)
            .context("pinned block omitted hash")?
            .to_owned();
        if start_index > factory_pairs {
            anyhow::bail!(
                "pair tail starts at {start_index}, beyond factory length {factory_pairs}"
            );
        }
        let mut pair_addresses = Vec::with_capacity(factory_pairs - start_index);
        for start in (start_index..factory_pairs).step_by(batch_size) {
            let end = (start + batch_size).min(factory_pairs);
            let calls = (start..end)
                .map(|index| Ok((factory, decode_hex(&encode_all_pairs(index))?)))
                .collect::<Result<Vec<_>>>()?;
            for result in self.multicall(&block_tag, calls).await? {
                let pair = decode_address_bytes(&result)?;
                if pair == Address::ZERO {
                    anyhow::bail!("factory allPairs returned zero address");
                }
                pair_addresses.push(pair);
            }
        }
        let pairs = self
            .fetch_pair_details_multicall(&pair_addresses, &block_tag, pinned_block, batch_size)
            .await?;
        Ok(FactoryBootstrap {
            block_number: pinned_block,
            block_hash,
            factory_pairs,
            loaded_pairs: pairs.len(),
            pairs,
        })
    }

    pub async fn block_hash(&self, block_number: u64) -> Result<String> {
        let block_tag = format!("0x{block_number:x}");
        let result = self
            .batch(vec![("eth_getBlockByNumber", json!([block_tag, false]))])
            .await?;
        result[0]
            .get("hash")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .with_context(|| format!("block {block_number} omitted hash"))
    }

    async fn fetch_pair_details_multicall(
        &self,
        pairs: &[Address],
        block_tag: &str,
        block_number: u64,
        batch_size: usize,
    ) -> Result<Vec<PairSnapshot>> {
        let pairs_per_batch = (batch_size / 3).max(1);
        let mut snapshots = Vec::with_capacity(pairs.len());
        for chunk in pairs.chunks(pairs_per_batch) {
            let mut calls = Vec::with_capacity(chunk.len() * 3);
            for pair in chunk {
                for selector in [TOKEN0_SELECTOR, TOKEN1_SELECTOR, GET_RESERVES_SELECTOR] {
                    calls.push((*pair, decode_hex(selector)?));
                }
            }
            let details = self.multicall(block_tag, calls).await?;
            for (index, pair) in chunk.iter().enumerate() {
                let offset = index * 3;
                let token0 = decode_address_bytes(&details[offset])?;
                let token1 = decode_address_bytes(&details[offset + 1])?;
                let (reserve0, reserve1) = decode_reserve_bytes(&details[offset + 2])?;
                snapshots.push(PairSnapshot {
                    pair: *pair,
                    token0,
                    token1,
                    reserve0,
                    reserve1,
                    block_number,
                });
            }
        }
        Ok(snapshots)
    }

    async fn multicall(
        &self,
        block_tag: &str,
        calls: Vec<(Address, Vec<u8>)>,
    ) -> Result<Vec<Vec<u8>>> {
        let call = multicall::aggregate3Call {
            calls: calls
                .into_iter()
                .map(|(target, call_data)| multicall::Call3 {
                    target,
                    allowFailure: false,
                    callData: Bytes::from(call_data),
                })
                .collect(),
        };
        let result = self
            .batch(vec![(
                "eth_call",
                json!([{
                    "to": MULTICALL3,
                    "data": format!("0x{}", hex::encode(call.abi_encode())),
                }, block_tag]),
            )])
            .await?;
        let encoded = decode_hex(value_string(&result[0], "Multicall3 aggregate3")?)?;
        let decoded = multicall::aggregate3Call::abi_decode_returns(&encoded)
            .context("decode Multicall3 aggregate3 result")?;
        decoded
            .into_iter()
            .map(|result| {
                if !result.success {
                    anyhow::bail!("Multicall3 subcall failed");
                }
                Ok(result.returnData.to_vec())
            })
            .collect()
    }

    async fn batch(&self, calls: Vec<(&str, Value)>) -> Result<Vec<Value>> {
        let requests: Vec<_> = calls
            .into_iter()
            .enumerate()
            .map(|(index, (method, params))| RpcRequest {
                jsonrpc: "2.0",
                id: index as u64 + 1,
                method: method.to_owned(),
                params,
            })
            .collect();
        let mut attempt = 0_u32;
        let response = loop {
            let response = self
                .http
                .post(&self.endpoint)
                .json(&requests)
                .send()
                .await
                .context("send JSON-RPC batch")?;
            if response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS
                && !response.status().is_server_error()
            {
                break response;
            }
            if attempt >= 5 {
                break response;
            }
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or_else(|| Duration::from_millis(250_u64 << attempt));
            tokio::time::sleep(retry_after.min(Duration::from_secs(4))).await;
            attempt += 1;
        };
        let response = response
            .error_for_status()
            .context("JSON-RPC HTTP status")?
            .json::<Vec<RpcResponse>>()
            .await
            .context("decode JSON-RPC batch")?;
        let mut by_id: HashMap<_, _> = response.into_iter().map(|item| (item.id, item)).collect();
        (1..=requests.len() as u64)
            .map(|id| {
                let item = by_id
                    .remove(&id)
                    .with_context(|| format!("JSON-RPC response omitted id {id}"))?;
                if let Some(error) = item.error {
                    anyhow::bail!("JSON-RPC error {}: {}", error.code, error.message);
                }
                item.result
                    .with_context(|| format!("JSON-RPC response {id} omitted result"))
            })
            .collect()
    }
}

fn encode_get_pair(token_a: Address, token_b: Address) -> String {
    format!(
        "0x{GET_PAIR_SELECTOR}{:0>64}{:0>64}",
        hex::encode(token_a),
        hex::encode(token_b)
    )
}

fn encode_all_pairs(index: usize) -> String {
    format!("0x{ALL_PAIRS_SELECTOR}{index:064x}")
}

fn value_string<'a>(value: &'a Value, context: &str) -> Result<&'a str> {
    value
        .as_str()
        .with_context(|| format!("{context} returned a non-string result"))
}

fn parse_quantity(value: &str) -> Result<u64> {
    u64::from_str_radix(value.strip_prefix("0x").unwrap_or(value), 16)
        .with_context(|| format!("invalid RPC quantity {value}"))
}

fn decode_address(value: &str) -> Result<Address> {
    let bytes = decode_hex(value)?;
    decode_address_bytes(&bytes)
}

fn decode_address_bytes(bytes: &[u8]) -> Result<Address> {
    if bytes.len() != 32 {
        anyhow::bail!("address ABI result has {} bytes instead of 32", bytes.len());
    }
    Ok(Address::from_slice(&bytes[12..]))
}

fn decode_reserves(value: &str) -> Result<(U256, U256)> {
    let bytes = decode_hex(value)?;
    decode_reserve_bytes(&bytes)
}

fn decode_reserve_bytes(bytes: &[u8]) -> Result<(U256, U256)> {
    if bytes.len() != 96 {
        anyhow::bail!(
            "getReserves ABI result has {} bytes instead of 96",
            bytes.len()
        );
    }
    Ok((
        U256::from_be_slice(&bytes[..32]),
        U256::from_be_slice(&bytes[32..64]),
    ))
}

fn decode_sync_data(value: &str) -> Result<(U256, U256)> {
    let bytes = decode_hex(value)?;
    if bytes.len() != 64 {
        anyhow::bail!("Sync data has {} bytes instead of 64", bytes.len());
    }
    Ok((
        U256::from_be_slice(&bytes[..32]),
        U256::from_be_slice(&bytes[32..]),
    ))
}

fn decode_u256(value: &str) -> Result<U256> {
    let bytes = decode_hex(value)?;
    if bytes.len() != 32 {
        anyhow::bail!("uint256 ABI result has {} bytes instead of 32", bytes.len());
    }
    Ok(U256::from_be_slice(&bytes))
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    hex::decode(value.strip_prefix("0x").unwrap_or(value)).context("decode RPC hex result")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_get_pair_call() {
        let encoded = encode_get_pair(Address::with_last_byte(1), Address::with_last_byte(2));
        assert_eq!(encoded.len(), 2 + 8 + 64 + 64);
        assert!(encoded.starts_with("0xe6a43905"));
        assert!(
            encoded.ends_with("0000000000000000000000000000000000000000000000000000000000000002")
        );
    }

    #[test]
    fn decodes_pair_reserves() {
        let encoded = format!("0x{:064x}{:064x}{:064x}", 123, 456, 789);
        assert_eq!(
            decode_reserves(&encoded).unwrap(),
            (U256::from(123), U256::from(456))
        );
    }

    #[test]
    fn encodes_factory_pair_index() {
        let encoded = encode_all_pairs(42);
        assert_eq!(encoded.len(), 2 + 8 + 64);
        assert!(
            encoded.ends_with("000000000000000000000000000000000000000000000000000000000000002a")
        );
    }

    #[test]
    fn decodes_sync_log_data() {
        let encoded = format!("0x{:064x}{:064x}", 123, 456);
        assert_eq!(
            decode_sync_data(&encoded).unwrap(),
            (U256::from(123), U256::from(456))
        );
    }
}
