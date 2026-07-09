use crate::{
    cache_rpc::{rpc_url_label, CacheRpcBackoff},
    event::now_ms,
};
use arc_swap::ArcSwap;
use reqwest::Client;
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Debug)]
pub(crate) struct WalletBalanceCache {
    inner: Arc<WalletBalanceCacheInner>,
}

#[derive(Debug)]
struct WalletBalanceCacheInner {
    client: Client,
    rpc_urls: Vec<String>,
    refresh_ms: u64,
    stale_after_ms: u128,
    http_timeout_ms: u64,
    wallets: ArcSwap<Vec<String>>,
    balances: Mutex<HashMap<String, WalletBalanceEntry>>,
    reservations: Mutex<HashMap<String, WalletBalanceReservationEntry>>,
    cache_rpc_backoff: CacheRpcBackoff,
}

#[derive(Clone, Debug)]
pub(crate) struct WalletBalanceEntry {
    pub(crate) lamports: u64,
    pub(crate) fetched_at_ms: u128,
    pub(crate) source_rpc: String,
}

#[derive(Clone, Copy, Debug)]
struct WalletBalanceReservationEntry {
    lamports: u64,
    expires_at_ms: u128,
}

// Longer than the normal recent-blockhash validity window, so a slow or ambiguous submit cannot
// become spendable again while the original transaction could still land.
const RESERVATION_RECONCILE_MS: u128 = 120_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WalletBalanceCheck {
    pub(crate) wallet: String,
    pub(crate) lamports: Option<u64>,
    pub(crate) fetched_at_ms: Option<u128>,
    pub(crate) age_ms: Option<u128>,
    pub(crate) source_rpc: Option<String>,
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
        rpc_urls: Vec<String>,
        refresh_ms: u64,
        stale_after_ms: u128,
        http_timeout_ms: u64,
        initial_wallets: Vec<String>,
        cache_rpc_backoff: CacheRpcBackoff,
    ) -> Self {
        Self {
            inner: Arc::new(WalletBalanceCacheInner {
                client: Client::new(),
                rpc_urls,
                refresh_ms,
                stale_after_ms,
                http_timeout_ms,
                wallets: ArcSwap::from_pointee(normalized_wallets(initial_wallets)),
                balances: Mutex::new(HashMap::new()),
                reservations: Mutex::new(HashMap::new()),
                cache_rpc_backoff,
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
        let Ok(balances) = self.inner.balances.lock() else {
            return unavailable_balance_check(wallet, required_lamports, "lock poisoned");
        };
        let Some(entry) = balances.get(copy_wallet).cloned() else {
            return WalletBalanceCheck {
                wallet,
                lamports: None,
                fetched_at_ms: None,
                age_ms: None,
                source_rpc: None,
                required_lamports,
                reason: Some("copy wallet balance cache missing".to_string()),
            };
        };

        let now = now_ms();
        let age_ms = now.saturating_sub(entry.fetched_at_ms);
        let reserved_lamports = match self.active_reserved_lamports(copy_wallet, now) {
            Ok(value) => value,
            Err(detail) => return unavailable_balance_check(wallet, required_lamports, detail),
        };
        let available_lamports = entry.lamports.saturating_sub(reserved_lamports);
        let reason = if age_ms > self.inner.stale_after_ms {
            Some(format!(
                "copy wallet balance cache stale: age {}ms exceeds {}ms",
                age_ms, self.inner.stale_after_ms
            ))
        } else if available_lamports < required_lamports {
            Some(format!(
                "copy wallet balance {} lamports below required {} lamports",
                available_lamports, required_lamports
            ))
        } else {
            None
        };

        WalletBalanceCheck {
            wallet,
            lamports: Some(available_lamports),
            fetched_at_ms: Some(entry.fetched_at_ms),
            age_ms: Some(age_ms),
            source_rpc: Some(entry.source_rpc),
            required_lamports,
            reason,
        }
    }

    /// Checks and reserves against the last fetched on-chain balance. Reservations live in a
    /// separate ledger so a concurrent RPC refresh cannot restore spendable balance. Committed
    /// reservations remain conservative until the next reconciliation window expires.
    pub(crate) fn check_and_reserve(
        &self,
        copy_wallet: &str,
        required_lamports: u64,
    ) -> (WalletBalanceCheck, Option<WalletBalanceReservation>) {
        let wallet = copy_wallet.to_string();
        let Ok(balances) = self.inner.balances.lock() else {
            return (
                unavailable_balance_check(wallet, required_lamports, "lock poisoned"),
                None,
            );
        };
        let Some(entry) = balances.get(copy_wallet) else {
            return (
                unavailable_balance_check(wallet, required_lamports, "missing"),
                None,
            );
        };

        let now = now_ms();
        let age_ms = now.saturating_sub(entry.fetched_at_ms);
        let Ok(mut reservations) = self.inner.reservations.lock() else {
            return (
                unavailable_balance_check(wallet, required_lamports, "reservation lock poisoned"),
                None,
            );
        };
        prune_expired_reservations(&mut reservations, now);
        let reserved_lamports = reservations
            .get(copy_wallet)
            .map(|reservation| reservation.lamports)
            .unwrap_or_default();
        let available_lamports = entry.lamports.saturating_sub(reserved_lamports);
        let reason = if age_ms > self.inner.stale_after_ms {
            Some(format!(
                "copy wallet balance cache stale: age {}ms exceeds {}ms",
                age_ms, self.inner.stale_after_ms
            ))
        } else if available_lamports < required_lamports {
            Some(format!(
                "copy wallet balance {} lamports below required {} lamports",
                available_lamports, required_lamports
            ))
        } else {
            None
        };
        let check = WalletBalanceCheck {
            wallet: wallet.clone(),
            lamports: Some(available_lamports),
            fetched_at_ms: Some(entry.fetched_at_ms),
            age_ms: Some(age_ms),
            source_rpc: Some(entry.source_rpc.clone()),
            required_lamports,
            reason,
        };
        if check.reason.is_some() {
            return (check, None);
        }

        let reservation =
            reservations
                .entry(copy_wallet.to_string())
                .or_insert(WalletBalanceReservationEntry {
                    lamports: 0,
                    expires_at_ms: now.saturating_add(RESERVATION_RECONCILE_MS),
                });
        reservation.lamports = reservation.lamports.saturating_add(required_lamports);
        reservation.expires_at_ms = reservation
            .expires_at_ms
            .max(now.saturating_add(RESERVATION_RECONCILE_MS));
        (
            check,
            Some(WalletBalanceReservation {
                cache: self.clone(),
                wallet,
                lamports: required_lamports,
                committed: false,
            }),
        )
    }

    fn release_reservation(&self, copy_wallet: &str, lamports: u64) {
        if lamports == 0 {
            return;
        }
        if let Ok(mut reservations) = self.inner.reservations.lock() {
            if let Some(entry) = reservations.get_mut(copy_wallet) {
                entry.lamports = entry.lamports.saturating_sub(lamports);
                if entry.lamports == 0 {
                    reservations.remove(copy_wallet);
                }
            }
        }
    }

    fn active_reserved_lamports(&self, copy_wallet: &str, now: u128) -> Result<u64, &'static str> {
        let mut reservations = self
            .inner
            .reservations
            .lock()
            .map_err(|_| "reservation lock poisoned")?;
        prune_expired_reservations(&mut reservations, now);
        Ok(reservations
            .get(copy_wallet)
            .map(|reservation| reservation.lamports)
            .unwrap_or_default())
    }
}

fn prune_expired_reservations(
    reservations: &mut HashMap<String, WalletBalanceReservationEntry>,
    now: u128,
) {
    reservations.retain(|_, reservation| reservation.expires_at_ms > now);
}

#[derive(Debug)]
pub(crate) struct WalletBalanceReservation {
    cache: WalletBalanceCache,
    wallet: String,
    lamports: u64,
    committed: bool,
}

impl WalletBalanceReservation {
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for WalletBalanceReservation {
    fn drop(&mut self) {
        if !self.committed {
            self.cache.release_reservation(&self.wallet, self.lamports);
        }
    }
}

fn unavailable_balance_check(
    wallet: String,
    required_lamports: u64,
    detail: &str,
) -> WalletBalanceCheck {
    WalletBalanceCheck {
        wallet,
        lamports: None,
        fetched_at_ms: None,
        age_ms: None,
        source_rpc: None,
        required_lamports,
        reason: Some(format!("copy wallet balance cache {detail}")),
    }
}

impl WalletBalanceCacheInner {
    async fn refresh_once(&self) -> Result<usize, String> {
        let wallets = self.wallets.load_full();
        if wallets.is_empty() {
            *self
                .balances
                .lock()
                .map_err(|_| "copy wallet balance cache lock poisoned".to_string())? =
                HashMap::new();
            return Ok(0);
        }

        let mut errors = Vec::new();
        for rpc_url in &self.rpc_urls {
            if !self.cache_rpc_backoff.is_available(rpc_url) {
                errors.push(format!("{}: provider in backoff", rpc_url_label(rpc_url)));
                continue;
            }
            match self.refresh_once_from_rpc(rpc_url, wallets.as_ref()).await {
                Ok(balances) => {
                    self.cache_rpc_backoff.record_success(rpc_url);
                    let count = balances.len();
                    *self
                        .balances
                        .lock()
                        .map_err(|_| "copy wallet balance cache lock poisoned".to_string())? =
                        balances;
                    return Ok(count);
                }
                Err(error) => {
                    self.cache_rpc_backoff
                        .record_failure("getMultipleAccounts", rpc_url, &error);
                    errors.push(format!("{}: {error}", rpc_url_label(rpc_url)));
                }
            }
        }

        Err(format!(
            "all getMultipleAccounts RPCs failed: {}",
            errors.join("; ")
        ))
    }

    async fn refresh_once_from_rpc(
        &self,
        rpc_url: &str,
        wallets: &[String],
    ) -> Result<HashMap<String, WalletBalanceEntry>, String> {
        let request = async {
            self.client
                .post(rpc_url)
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
        let source_rpc = rpc_url_label(rpc_url);
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
                    source_rpc: source_rpc.clone(),
                },
            );
        }
        Ok(balances)
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
            vec!["http://127.0.0.1:8899".to_string()],
            1_000,
            stale_after_ms,
            0,
            vec![wallet.to_string()],
            CacheRpcBackoff::default(),
        )
    }

    fn seed_balance(cache: &WalletBalanceCache, wallet: &str, lamports: u64) {
        cache.inner.balances.lock().expect("balance lock").insert(
            wallet.to_string(),
            WalletBalanceEntry {
                lamports,
                fetched_at_ms: now_ms(),
                source_rpc: "rpc.example.com".to_string(),
            },
        );
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
        seed_balance(&cache, "wallet", 100);
        cache
            .inner
            .balances
            .lock()
            .expect("balance lock")
            .get_mut("wallet")
            .expect("wallet")
            .fetched_at_ms = now_ms().saturating_sub(10);

        let check = cache.check("wallet", 10);

        assert!(check.reason.unwrap().contains("balance cache stale"));
    }

    #[test]
    fn check_blocks_insufficient_balance() {
        let cache = cache_with_wallet("wallet", 5_000);
        seed_balance(&cache, "wallet", 9);

        let check = cache.check("wallet", 10);

        assert_eq!(check.lamports, Some(9));
        assert_eq!(check.source_rpc.as_deref(), Some("rpc.example.com"));
        assert_eq!(
            check.reason,
            Some("copy wallet balance 9 lamports below required 10 lamports".to_string())
        );
    }

    #[test]
    fn reservation_decrements_and_commit_keeps_cached_balance() {
        let cache = cache_with_wallet("wallet", 5_000);
        seed_balance(&cache, "wallet", 10);

        let (check, reservation) = cache.check_and_reserve("wallet", 10);
        assert!(check.reason.is_none());
        reservation.expect("reserved").commit();

        assert_eq!(cache.check("wallet", 0).lamports, Some(0));
    }

    #[test]
    fn dropped_reservation_refunds_cached_balance() {
        let cache = cache_with_wallet("wallet", 5_000);
        seed_balance(&cache, "wallet", 10);

        let (_, reservation) = cache.check_and_reserve("wallet", 7);
        drop(reservation);

        assert_eq!(cache.check("wallet", 0).lamports, Some(10));
    }

    #[test]
    fn refresh_cannot_erase_committed_reservation() {
        let cache = cache_with_wallet("wallet", 5_000);
        seed_balance(&cache, "wallet", 10);
        let (_, reservation) = cache.check_and_reserve("wallet", 7);
        reservation.expect("reserved").commit();

        seed_balance(&cache, "wallet", 10);

        assert_eq!(cache.check("wallet", 0).lamports, Some(3));
    }

    #[test]
    fn concurrent_reservations_cannot_overspend_same_wallet() {
        let cache = cache_with_wallet("wallet", 5_000);
        seed_balance(&cache, "wallet", 10);
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let cache = cache.clone();
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                let (check, reservation) = cache.check_and_reserve("wallet", 7);
                (check.reason.is_none(), reservation)
            }));
        }
        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("reservation thread"))
            .collect::<Vec<_>>();

        assert_eq!(results.iter().filter(|(ok, _)| *ok).count(), 1);
        let committed = results
            .into_iter()
            .find_map(|(ok, reservation)| ok.then_some(reservation).flatten())
            .expect("one reservation succeeds");
        committed.commit();
        assert_eq!(cache.check("wallet", 0).lamports, Some(3));
    }
}
