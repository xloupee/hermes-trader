use std::collections::{HashMap, HashSet};
use std::time::Duration;

use alloy_primitives::{Address, U256};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::v2_simulator::PairSnapshot;

const ROBINHOOD_CHAIN_ID: u64 = 4_663;
const GET_PAIR_SELECTOR: &str = "e6a43905";
const TOKEN0_SELECTOR: &str = "0x0dfe1681";
const TOKEN1_SELECTOR: &str = "0xd21220a7";
const GET_RESERVES_SELECTOR: &str = "0x0902f1ac";

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
        let response = self
            .http
            .post(&self.endpoint)
            .json(&requests)
            .send()
            .await
            .context("send JSON-RPC batch")?
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
    if bytes.len() != 32 {
        anyhow::bail!("address ABI result has {} bytes instead of 32", bytes.len());
    }
    Ok(Address::from_slice(&bytes[12..]))
}

fn decode_reserves(value: &str) -> Result<(U256, U256)> {
    let bytes = decode_hex(value)?;
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
}
