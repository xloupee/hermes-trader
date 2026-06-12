use crate::event::now_ms;
use serde::Deserialize;
use solana_hash::Hash;
use std::{
    str::FromStr,
    sync::{Arc, RwLock},
    time::Duration,
};

#[derive(Clone, Debug)]
pub(crate) struct CachedBlockhash {
    pub(crate) hash: Hash,
    pub(crate) last_valid_block_height: u64,
    pub(crate) fetched_at_ms: u128,
}

pub(crate) type BlockhashCache = Arc<RwLock<Option<CachedBlockhash>>>;

pub(crate) fn spawn_blockhash_cache(
    rpc_url: Option<String>,
    refresh_ms: u64,
    stats: bool,
) -> Option<BlockhashCache> {
    let rpc_url = rpc_url?;
    let refresh_ms = refresh_ms.max(100);
    let cache = Arc::new(RwLock::new(None));
    let task_cache = Arc::clone(&cache);

    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut interval = tokio::time::interval(Duration::from_millis(refresh_ms));

        loop {
            interval.tick().await;
            match fetch_latest_blockhash(&client, &rpc_url).await {
                Ok(blockhash) => {
                    if stats {
                        eprintln!(
                            "refreshed blockhash {}; lastValidBlockHeight={}; fetchedAtMs={}",
                            blockhash.hash,
                            blockhash.last_valid_block_height,
                            blockhash.fetched_at_ms
                        );
                    }
                    if let Ok(mut guard) = task_cache.write() {
                        *guard = Some(blockhash);
                    }
                }
                Err(error) if stats => {
                    eprintln!("blockhash refresh failed: {error}");
                }
                Err(_) => {}
            }
        }
    });

    Some(cache)
}

pub(crate) fn cached_blockhash(cache: Option<&BlockhashCache>) -> Option<CachedBlockhash> {
    cache
        .and_then(|cache| cache.read().ok())
        .and_then(|guard| guard.clone())
}

async fn fetch_latest_blockhash(
    client: &reqwest::Client,
    rpc_url: &str,
) -> Result<CachedBlockhash, String> {
    let response = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLatestBlockhash",
            "params": [
                {
                    "commitment": "processed"
                }
            ]
        }))
        .send()
        .await
        .map_err(|error| format!("send getLatestBlockhash request: {error}"))?
        .error_for_status()
        .map_err(|error| format!("getLatestBlockhash HTTP status: {error}"))?
        .json::<RpcResponse>()
        .await
        .map_err(|error| format!("decode getLatestBlockhash response: {error}"))?;

    if let Some(error) = response.error {
        return Err(format!("getLatestBlockhash RPC error: {}", error.message));
    }

    let value = response
        .result
        .map(|result| result.value)
        .ok_or_else(|| "getLatestBlockhash result missing".to_string())?;

    cached_blockhash_from_rpc(value, now_ms())
}

fn cached_blockhash_from_rpc(
    value: RpcBlockhashValue,
    fetched_at_ms: u128,
) -> Result<CachedBlockhash, String> {
    let hash = Hash::from_str(&value.blockhash)
        .map_err(|error| format!("parse getLatestBlockhash blockhash: {error}"))?;
    Ok(CachedBlockhash {
        hash,
        last_valid_block_height: value.last_valid_block_height,
        fetched_at_ms,
    })
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    result: Option<RpcResult>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcResult {
    value: RpcBlockhashValue,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcBlockhashValue {
    blockhash: String,
    last_valid_block_height: u64,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_blockhash_records_warm_state_fields() {
        let hash = Hash::default();
        let blockhash = CachedBlockhash {
            hash,
            last_valid_block_height: 123,
            fetched_at_ms: 456,
        };

        assert_eq!(blockhash.hash, hash);
        assert_eq!(blockhash.last_valid_block_height, 123);
        assert_eq!(blockhash.fetched_at_ms, 456);
    }

    #[test]
    fn rpc_blockhash_is_parsed_when_cache_refresh_builds_entry() {
        let hash = Hash::default();
        let blockhash = cached_blockhash_from_rpc(
            RpcBlockhashValue {
                blockhash: hash.to_string(),
                last_valid_block_height: 123,
            },
            456,
        )
        .expect("valid RPC blockhash should refresh cache entry");

        assert_eq!(blockhash.hash, hash);
        assert_eq!(blockhash.last_valid_block_height, 123);
        assert_eq!(blockhash.fetched_at_ms, 456);
    }

    #[test]
    fn invalid_rpc_blockhash_fails_cache_refresh() {
        let error = cached_blockhash_from_rpc(
            RpcBlockhashValue {
                blockhash: "not-a-blockhash".to_string(),
                last_valid_block_height: 123,
            },
            456,
        )
        .expect_err("invalid RPC blockhash should not enter cache");

        assert!(error.starts_with("parse getLatestBlockhash blockhash:"));
    }
}
