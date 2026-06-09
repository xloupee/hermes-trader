use crate::event::now_ms;
use arc_swap::ArcSwap;
use reqwest::Client;
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc, time::Duration};

#[derive(Clone, Debug)]
pub(crate) struct WalletBalanceCache {
    inner: Arc<WalletBalanceCacheInner>,
}

#[derive(Debug)]
struct WalletBalanceCacheInner {
    client: Client,
    rpc_url: String,
    refresh_ms: u64,
    stale_after_ms: u128,
    http_timeout_ms: u64,
    wallets: ArcSwap<Vec<String>>,
    balances: ArcSwap<HashMap<String, WalletBalanceEntry>>,
}

#[derive(Clone, Debug)]
pub(crate) struct WalletBalanceEntry {
    pub(crate) lamports: u64,
    pub(crate) fetched_at_ms: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WalletBalanceCheck {
    pub(crate) wallet: String,
    pub(crate) lamports: Option<u64>,
    pub(crate) fetched_at_ms: Option<u128>,
    pub(crate) age_ms: Option<u128>,
    pub(crate) required_lamports: u64,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct GetMultipleAccountsResult {
    value: Vec<Option<RpcAccount>>,
}

#[derive(Debug, Deserialize)]
struct RpcAccount {
    lamports: u64,
}

impl WalletBalanceCache {
    pub(crate) fn new(
        rpc_url: String,
        refresh_ms: u64,
        stale_after_ms: u128,
        http_timeout_ms: u64,
        initial_wallets: Vec<String>,
    ) -> Self {
        Self {
            inner: Arc::new(WalletBalanceCacheInner {
                client: Client::new(),
                rpc_url,
                refresh_ms,
                stale_after_ms,
                http_timeout_ms,
                wallets: ArcSwap::from_pointee(normalized_wallets(initial_wallets)),
                balances: ArcSwap::from_pointee(HashMap::new()),
            }),
        }
    }

    pub(crate) fn replace_wallets(&self, wallets: Vec<String>) {
        self.inner
            .wallets
            .store(Arc::new(normalized_wallets(wallets)));
    }

    pub(crate) async fn refresh_once(&self) -> Result<usize, String> {
        self.inner.refresh_once().await
    }

    pub(crate) fn spawn_refresh_loop(&self) {
        if self.inner.refresh_ms == 0 {
            return;
        }

        let cache = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(cache.inner.refresh_ms));
            interval.tick().await;
            loop {
                interval.tick().await;
                if let Err(error) = cache.refresh_once().await {
                    eprintln!("copy wallet balance refresh failed: {error}");
                }
            }
        });
    }

    pub(crate) fn check(&self, copy_wallet: &str, required_lamports: u64) -> WalletBalanceCheck {
        let wallet = copy_wallet.to_string();
        let Some(entry) = self.inner.balances.load().get(copy_wallet).cloned() else {
            return WalletBalanceCheck {
                wallet,
                lamports: None,
                fetched_at_ms: None,
                age_ms: None,
                required_lamports,
                reason: Some("copy wallet balance cache missing".to_string()),
            };
        };

        let age_ms = now_ms().saturating_sub(entry.fetched_at_ms);
        let reason = if age_ms > self.inner.stale_after_ms {
            Some(format!(
                "copy wallet balance cache stale: age {}ms exceeds {}ms",
                age_ms, self.inner.stale_after_ms
            ))
        } else if entry.lamports < required_lamports {
            Some(format!(
                "copy wallet balance {} lamports below required {} lamports",
                entry.lamports, required_lamports
            ))
        } else {
            None
        };

        WalletBalanceCheck {
            wallet,
            lamports: Some(entry.lamports),
            fetched_at_ms: Some(entry.fetched_at_ms),
            age_ms: Some(age_ms),
            required_lamports,
            reason,
        }
    }

    pub(crate) fn optimistic_decrement(&self, copy_wallet: &str, spent_lamports: u64) {
        if spent_lamports == 0 {
            return;
        }

        let mut balances = self.inner.balances.load().as_ref().clone();
        let Some(entry) = balances.get_mut(copy_wallet) else {
            return;
        };
        entry.lamports = entry.lamports.saturating_sub(spent_lamports);
        self.inner.balances.store(Arc::new(balances));
    }
}

impl WalletBalanceCacheInner {
    async fn refresh_once(&self) -> Result<usize, String> {
        let wallets = self.wallets.load_full();
        if wallets.is_empty() {
            self.balances.store(Arc::new(HashMap::new()));
            return Ok(0);
        }

        let request = async {
            self.client
                .post(&self.rpc_url)
                .json(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "getMultipleAccounts",
                    "params": [
                        wallets.as_ref(),
                        {
                            "commitment": "processed",
                            "encoding": "base64"
                        }
                    ]
                }))
                .send()
                .await
                .map_err(|error| format!("send getMultipleAccounts request: {error}"))?
                .error_for_status()
                .map_err(|error| format!("getMultipleAccounts HTTP status: {error}"))?
                .json::<RpcResponse<GetMultipleAccountsResult>>()
                .await
                .map_err(|error| format!("decode getMultipleAccounts response: {error}"))
        };

        let response = if self.http_timeout_ms > 0 {
            tokio::time::timeout(Duration::from_millis(self.http_timeout_ms), request)
                .await
                .map_err(|_| {
                    format!(
                        "getMultipleAccounts timed out after {}ms",
                        self.http_timeout_ms
                    )
                })??
        } else {
            request.await?
        };

        if let Some(error) = response.error {
            return Err(format!("getMultipleAccounts RPC error: {}", error.message));
        }

        let fetched_at_ms = now_ms();
        let values = response
            .result
            .ok_or_else(|| "getMultipleAccounts result missing".to_string())?
            .value;
        let mut balances = HashMap::with_capacity(wallets.len());
        for (wallet, account) in wallets.iter().zip(values.into_iter()) {
            balances.insert(
                wallet.clone(),
                WalletBalanceEntry {
                    lamports: account.map(|account| account.lamports).unwrap_or(0),
                    fetched_at_ms,
                },
            );
        }
        let count = balances.len();
        self.balances.store(Arc::new(balances));
        Ok(count)
    }
}

fn normalized_wallets(wallets: Vec<String>) -> Vec<String> {
    let mut wallets = wallets
        .into_iter()
        .map(|wallet| wallet.trim().to_string())
        .filter(|wallet| !wallet.is_empty())
        .collect::<Vec<_>>();
    wallets.sort();
    wallets.dedup();
    wallets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_with_wallet(wallet: &str, stale_after_ms: u128) -> WalletBalanceCache {
        WalletBalanceCache::new(
            "http://127.0.0.1:8899".to_string(),
            1_000,
            stale_after_ms,
            0,
            vec![wallet.to_string()],
        )
    }

    #[test]
    fn check_fails_closed_when_balance_missing() {
        let cache = cache_with_wallet("wallet", 5_000);

        let check = cache.check("wallet", 10);

        assert_eq!(check.lamports, None);
        assert_eq!(
            check.reason,
            Some("copy wallet balance cache missing".to_string())
        );
    }

    #[test]
    fn check_blocks_stale_balance() {
        let cache = cache_with_wallet("wallet", 0);
        cache.inner.balances.store(Arc::new(HashMap::from([(
            "wallet".to_string(),
            WalletBalanceEntry {
                lamports: 100,
                fetched_at_ms: now_ms().saturating_sub(10),
            },
        )])));

        let check = cache.check("wallet", 10);

        assert!(check.reason.unwrap().contains("balance cache stale"));
    }

    #[test]
    fn check_blocks_insufficient_balance() {
        let cache = cache_with_wallet("wallet", 5_000);
        cache.inner.balances.store(Arc::new(HashMap::from([(
            "wallet".to_string(),
            WalletBalanceEntry {
                lamports: 9,
                fetched_at_ms: now_ms(),
            },
        )])));

        let check = cache.check("wallet", 10);

        assert_eq!(check.lamports, Some(9));
        assert_eq!(
            check.reason,
            Some("copy wallet balance 9 lamports below required 10 lamports".to_string())
        );
    }

    #[test]
    fn optimistic_decrement_saturates_cached_balance() {
        let cache = cache_with_wallet("wallet", 5_000);
        cache.inner.balances.store(Arc::new(HashMap::from([(
            "wallet".to_string(),
            WalletBalanceEntry {
                lamports: 9,
                fetched_at_ms: now_ms(),
            },
        )])));

        cache.optimistic_decrement("wallet", 10);

        assert_eq!(cache.check("wallet", 0).lamports, Some(0));
    }
}
