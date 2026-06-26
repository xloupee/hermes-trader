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
    pub(crate) source_rpc: String,
    pub(crate) commitment: String,
    pub(crate) context_slot: Option<u64>,
    pub(crate) selection_strategy: &'static str,
}

pub(crate) type BlockhashCache = Arc<RwLock<Option<CachedBlockhash>>>;

pub(crate) fn spawn_blockhash_cache(
    rpc_urls: Vec<String>,
    refresh_ms: u64,
    refresh_timeout_ms: u64,
    commitment: String,
    stats: bool,
) -> Option<BlockhashCache> {
    if rpc_urls.is_empty() {
        return None;
    }
    let refresh_ms = refresh_ms.max(100);
    let cache = Arc::new(RwLock::new(None));
    let task_cache = Arc::clone(&cache);

    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut interval = tokio::time::interval(Duration::from_millis(refresh_ms));
        let mut last_error_log_ms = 0u128;

        loop {
            interval.tick().await;
            match fetch_latest_blockhash_any(&client, &rpc_urls, &commitment, refresh_timeout_ms)
                .await
            {
                Ok(blockhash) => {
                    last_error_log_ms = 0;
                    if stats {
                        eprintln!(
                            "refreshed blockhash {}; lastValidBlockHeight={}; commitment={}; contextSlot={}; sourceRpc={}; fetchedAtMs={}",
                            blockhash.hash,
                            blockhash.last_valid_block_height,
                            blockhash.commitment,
                            blockhash
                                .context_slot
                                .map(|slot| slot.to_string())
                                .unwrap_or_else(|| "unknown".to_string()),
                            blockhash.source_rpc,
                            blockhash.fetched_at_ms
                        );
                    }
                    if let Ok(mut guard) = task_cache.write() {
                        *guard = Some(blockhash);
                    }
                }
                Err(error) => {
                    let current_ms = now_ms();
                    if stats || current_ms.saturating_sub(last_error_log_ms) >= 5_000 {
                        eprintln!("blockhash refresh failed: {error}");
                        last_error_log_ms = current_ms;
                    }
                }
            }
        }
    });

    Some(cache)
}

pub(crate) fn cached_blockhash(
    cache: Option<&BlockhashCache>,
    stale_after_ms: u128,
) -> Option<CachedBlockhash> {
    cache
        .and_then(|cache| cache.read().ok())
        .and_then(|guard| guard.clone())
        .filter(|blockhash| now_ms().saturating_sub(blockhash.fetched_at_ms) <= stale_after_ms)
}

async fn fetch_latest_blockhash_any(
    client: &reqwest::Client,
    rpc_urls: &[String],
    commitment: &str,
    timeout_ms: u64,
) -> Result<CachedBlockhash, String> {
    let mut errors = Vec::new();
    let mut best: Option<CachedBlockhash> = None;
    for rpc_url in rpc_urls {
        match fetch_latest_blockhash(client, rpc_url, commitment, timeout_ms).await {
            Ok(blockhash) => match (&best, blockhash.context_slot) {
                (None, _) => best = Some(blockhash),
                (Some(existing), Some(slot)) if existing.context_slot.unwrap_or(0) < slot => {
                    best = Some(blockhash);
                }
                _ => {}
            },
            Err(error) => errors.push(format!("{}: {error}", rpc_url_label(rpc_url))),
        }
    }
    if let Some(blockhash) = best {
        return Ok(blockhash);
    }
    Err(format!(
        "all getLatestBlockhash RPCs failed: {}",
        errors.join("; ")
    ))
}

async fn fetch_latest_blockhash(
    client: &reqwest::Client,
    rpc_url: &str,
    commitment: &str,
    timeout_ms: u64,
) -> Result<CachedBlockhash, String> {
    let request = async {
        client
            .post(rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getLatestBlockhash",
                "params": [
                    {
                        "commitment": commitment
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
            .map_err(|error| format!("decode getLatestBlockhash response: {error}"))
    };

    let response = if timeout_ms > 0 {
        tokio::time::timeout(Duration::from_millis(timeout_ms), request)
            .await
            .map_err(|_| format!("getLatestBlockhash timed out after {timeout_ms}ms"))??
    } else {
        request.await?
    };

    if let Some(error) = response.error {
        return Err(format!("getLatestBlockhash RPC error: {}", error.message));
    }

    let result = response
        .result
        .ok_or_else(|| "getLatestBlockhash result missing".to_string())?;

    cached_blockhash_from_rpc(
        result.value,
        now_ms(),
        rpc_url_label(rpc_url),
        commitment.to_string(),
        result.context.map(|context| context.slot),
    )
}

fn cached_blockhash_from_rpc(
    value: RpcBlockhashValue,
    fetched_at_ms: u128,
    source_rpc: String,
    commitment: String,
    context_slot: Option<u64>,
) -> Result<CachedBlockhash, String> {
    let hash = Hash::from_str(&value.blockhash)
        .map_err(|error| format!("parse getLatestBlockhash blockhash: {error}"))?;
    Ok(CachedBlockhash {
        hash,
        last_valid_block_height: value.last_valid_block_height,
        fetched_at_ms,
        source_rpc,
        commitment,
        context_slot,
        selection_strategy: "highest_context_slot",
    })
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    result: Option<RpcResult>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcResult {
    context: Option<RpcContext>,
    value: RpcBlockhashValue,
}

#[derive(Debug, Deserialize)]
struct RpcContext {
    slot: u64,
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

fn rpc_url_label(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
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
            source_rpc: "rpc.example.com".to_string(),
            commitment: "processed".to_string(),
            context_slot: Some(789),
            selection_strategy: "highest_context_slot",
        };

        assert_eq!(blockhash.hash, hash);
        assert_eq!(blockhash.last_valid_block_height, 123);
        assert_eq!(blockhash.fetched_at_ms, 456);
    }

    #[test]
    fn cached_blockhash_filters_stale_entries() {
        let cache = Arc::new(RwLock::new(Some(CachedBlockhash {
            hash: Hash::default(),
            last_valid_block_height: 123,
            fetched_at_ms: now_ms().saturating_sub(10_000),
            source_rpc: "rpc.example.com".to_string(),
            commitment: "processed".to_string(),
            context_slot: Some(789),
            selection_strategy: "highest_context_slot",
        })));

        assert!(cached_blockhash(Some(&cache), 1_000).is_none());
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
            "rpc.example.com".to_string(),
            "confirmed".to_string(),
            Some(789),
        )
        .expect("valid RPC blockhash should refresh cache entry");

        assert_eq!(blockhash.hash, hash);
        assert_eq!(blockhash.last_valid_block_height, 123);
        assert_eq!(blockhash.fetched_at_ms, 456);
        assert_eq!(blockhash.source_rpc, "rpc.example.com");
        assert_eq!(blockhash.commitment, "confirmed");
        assert_eq!(blockhash.context_slot, Some(789));
    }

    #[test]
    fn invalid_rpc_blockhash_fails_cache_refresh() {
        let error = cached_blockhash_from_rpc(
            RpcBlockhashValue {
                blockhash: "not-a-blockhash".to_string(),
                last_valid_block_height: 123,
            },
            456,
            "rpc.example.com".to_string(),
            "processed".to_string(),
            None,
        )
        .expect_err("invalid RPC blockhash should not enter cache");

        assert!(error.starts_with("parse getLatestBlockhash blockhash:"));
    }
}
