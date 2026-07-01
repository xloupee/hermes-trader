use crate::event::now_ms;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Default)]
pub(crate) struct CacheRpcBackoff {
    inner: Arc<Mutex<HashMap<String, CacheRpcProviderState>>>,
}

#[derive(Clone, Debug, Default)]
struct CacheRpcProviderState {
    failures: u32,
    backoff_until_ms: u128,
}

impl CacheRpcBackoff {
    pub(crate) fn is_available(&self, rpc_url: &str) -> bool {
        let now = now_ms();
        self.inner
            .lock()
            .ok()
            .and_then(|states| states.get(rpc_url).cloned())
            .map(|state| state.backoff_until_ms <= now)
            .unwrap_or(true)
    }

    pub(crate) fn record_success(&self, rpc_url: &str) {
        if let Ok(mut states) = self.inner.lock() {
            states.remove(rpc_url);
        }
    }

    pub(crate) fn record_failure(&self, method: &str, rpc_url: &str, error: &str) {
        let Some(error_class) = retryable_cache_rpc_error_class(error) else {
            return;
        };
        let now = now_ms();
        let (failures, backoff_ms) = if let Ok(mut states) = self.inner.lock() {
            let state = states.entry(rpc_url.to_string()).or_default();
            state.failures = state.failures.saturating_add(1);
            let backoff_ms = cache_rpc_backoff_ms(state.failures);
            state.backoff_until_ms = now.saturating_add(u128::from(backoff_ms));
            (state.failures, backoff_ms)
        } else {
            (1, cache_rpc_backoff_ms(1))
        };
        eprintln!(
            "cache RPC provider backoff: method={method} provider={} errorClass={error_class} failures={failures} backoffMs={backoff_ms} error={}",
            rpc_url_label(rpc_url),
            sanitize_cache_rpc_error(error)
        );
    }
}

fn cache_rpc_backoff_ms(failures: u32) -> u64 {
    match failures {
        0 | 1 => 10_000,
        2 => 30_000,
        _ => 60_000,
    }
}

fn retryable_cache_rpc_error_class(error: &str) -> Option<&'static str> {
    let lower = error.to_ascii_lowercase();
    if lower.contains("429") || lower.contains("too many requests") || lower.contains("rate limit")
    {
        Some("rate_limit")
    } else if lower.contains("timed out") || lower.contains("timeout") {
        Some("timeout")
    } else if lower.contains("http status: 5")
        || lower.contains("http status 5")
        || lower.contains("server error")
    {
        Some("server_error")
    } else {
        None
    }
}

fn sanitize_cache_rpc_error(error: &str) -> String {
    error
        .replace('\n', " ")
        .replace('\r', " ")
        .chars()
        .take(240)
        .collect()
}

pub(crate) fn rpc_url_label(url: &str) -> String {
    let without_query = url
        .trim()
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches('/');
    let after_scheme = without_query
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(without_query);
    let host = after_scheme.split('/').next().unwrap_or("").trim();
    if host.is_empty() {
        "(unknown-rpc)".to_string()
    } else {
        host.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_failures_put_provider_in_backoff_until_success() {
        let backoff = CacheRpcBackoff::default();
        let rpc_url = "https://edge.erpc.global?api-key=secret";

        assert!(backoff.is_available(rpc_url));

        backoff.record_failure(
            "getRecentPrioritizationFees",
            rpc_url,
            "getRecentPrioritizationFees HTTP status: 429 Too Many Requests",
        );

        assert!(!backoff.is_available(rpc_url));

        backoff.record_success(rpc_url);

        assert!(backoff.is_available(rpc_url));
    }

    #[test]
    fn non_retryable_failures_do_not_trip_provider_backoff() {
        let backoff = CacheRpcBackoff::default();
        let rpc_url = "https://solana-rpc.publicnode.com";

        backoff.record_failure(
            "getLatestBlockhash",
            rpc_url,
            "parse getLatestBlockhash blockhash: invalid hash",
        );

        assert!(backoff.is_available(rpc_url));
    }

    #[test]
    fn rpc_url_label_strips_secret_query_string() {
        assert_eq!(
            rpc_url_label("https://edge.erpc.global?api-key=secret"),
            "edge.erpc.global"
        );
        assert_eq!(
            rpc_url_label("https://mainnet.helius-rpc.com/?api-key=secret"),
            "mainnet.helius-rpc.com"
        );
    }
}
