use crate::event::now_ms;
use arc_swap::ArcSwap;
use reqwest::Client;
use serde::Deserialize;
use solana_pubkey::Pubkey;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Debug)]
pub(crate) struct PriorityFeeCache {
    inner: Arc<PriorityFeeCacheInner>,
}

#[derive(Debug)]
struct PriorityFeeCacheInner {
    client: Client,
    rpc_urls: Vec<String>,
    refresh_ms: u64,
    stale_after_ms: u128,
    http_timeout_ms: u64,
    percentile: u8,
    tracked: Mutex<HashMap<String, Vec<String>>>,
    entries: ArcSwap<HashMap<String, PriorityFeeEntry>>,
}

#[derive(Clone, Debug)]
struct PriorityFeeEntry {
    priority_fee_micro_lamports: u64,
    fetched_at_ms: u128,
    sample_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PriorityFeeLookup {
    pub(crate) account_count: usize,
    pub(crate) priority_fee_micro_lamports: Option<u64>,
    pub(crate) fetched_at_ms: Option<u128>,
    pub(crate) age_ms: Option<u128>,
    pub(crate) sample_count: Option<usize>,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecentPrioritizationFee {
    prioritization_fee: u64,
}

impl PriorityFeeCache {
    pub(crate) fn new(
        rpc_urls: Vec<String>,
        refresh_ms: u64,
        stale_after_ms: u128,
        http_timeout_ms: u64,
        percentile: u8,
    ) -> Self {
        Self {
            inner: Arc::new(PriorityFeeCacheInner {
                client: Client::new(),
                rpc_urls,
                refresh_ms,
                stale_after_ms,
                http_timeout_ms,
                percentile: percentile.min(100),
                tracked: Mutex::new(HashMap::new()),
                entries: ArcSwap::from_pointee(HashMap::new()),
            }),
        }
    }

    pub(crate) fn observe_writable_accounts(&self, accounts: &[Pubkey]) -> PriorityFeeLookup {
        let accounts = normalized_account_strings(accounts);
        if accounts.is_empty() {
            return PriorityFeeLookup::default();
        }
        let key = account_key(&accounts);
        if let Ok(mut tracked) = self.inner.tracked.lock() {
            for account in &accounts {
                tracked
                    .entry(account.clone())
                    .or_insert_with(|| vec![account.clone()]);
            }
        }

        let entries = self.inner.entries.load();
        let Some(entry) = entries
            .get(&key)
            .cloned()
            .or_else(|| self.inner.max_fresh_single_account_entry(&entries, &accounts))
        else {
            return PriorityFeeLookup {
                account_count: accounts.len(),
                ..PriorityFeeLookup::default()
            };
        };
        let age_ms = now_ms().saturating_sub(entry.fetched_at_ms);
        if age_ms > self.inner.stale_after_ms {
            return PriorityFeeLookup {
                account_count: accounts.len(),
                fetched_at_ms: Some(entry.fetched_at_ms),
                age_ms: Some(age_ms),
                sample_count: Some(entry.sample_count),
                ..PriorityFeeLookup::default()
            };
        }

        PriorityFeeLookup {
            account_count: accounts.len(),
            priority_fee_micro_lamports: Some(entry.priority_fee_micro_lamports),
            fetched_at_ms: Some(entry.fetched_at_ms),
            age_ms: Some(age_ms),
            sample_count: Some(entry.sample_count),
        }
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
                    eprintln!("account-aware priority fee refresh failed: {error}");
                }
            }
        });
    }

    pub(crate) async fn refresh_once(&self) -> Result<usize, String> {
        self.inner.refresh_once().await
    }
}

impl PriorityFeeCacheInner {
    fn max_fresh_single_account_entry(
        &self,
        entries: &HashMap<String, PriorityFeeEntry>,
        accounts: &[String],
    ) -> Option<PriorityFeeEntry> {
        let now = now_ms();
        accounts
            .iter()
            .filter_map(|account| entries.get(account))
            .filter(|entry| now.saturating_sub(entry.fetched_at_ms) <= self.stale_after_ms)
            .max_by_key(|entry| entry.priority_fee_micro_lamports)
            .cloned()
    }

    async fn refresh_once(&self) -> Result<usize, String> {
        let tracked = self
            .tracked
            .lock()
            .map_err(|_| "priority fee tracked account mutex poisoned".to_string())?
            .clone();
        if tracked.is_empty() {
            return Ok(0);
        }

        let mut updated = self.entries.load().as_ref().clone();
        let mut refresh_count = 0usize;
        let mut last_error = None;
        for (key, accounts) in tracked {
            match self.refresh_account_set(&accounts).await {
                Ok(entry) => {
                    updated.insert(key, entry);
                    refresh_count += 1;
                }
                Err(error) => last_error = Some(error),
            }
        }
        self.entries.store(Arc::new(updated));

        if refresh_count == 0 {
            if let Some(error) = last_error {
                return Err(error);
            }
        }
        Ok(refresh_count)
    }

    async fn refresh_account_set(&self, accounts: &[String]) -> Result<PriorityFeeEntry, String> {
        let mut errors = Vec::new();
        for rpc_url in &self.rpc_urls {
            match self
                .fetch_recent_prioritization_fees(rpc_url, accounts)
                .await
            {
                Ok(samples) => {
                    let priority_fee_micro_lamports =
                        percentile_priority_fee(&samples, self.percentile);
                    return Ok(PriorityFeeEntry {
                        priority_fee_micro_lamports,
                        fetched_at_ms: now_ms(),
                        sample_count: samples.len(),
                    });
                }
                Err(error) => errors.push(format!("{}: {error}", rpc_url_label(rpc_url))),
            }
        }
        Err(format!(
            "all getRecentPrioritizationFees RPCs failed: {}",
            errors.join("; ")
        ))
    }

    async fn fetch_recent_prioritization_fees(
        &self,
        rpc_url: &str,
        accounts: &[String],
    ) -> Result<Vec<RecentPrioritizationFee>, String> {
        let request = async {
            self.client
                .post(rpc_url)
                .json(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "getRecentPrioritizationFees",
                    "params": [accounts]
                }))
                .send()
                .await
                .map_err(|error| format!("send getRecentPrioritizationFees request: {error}"))?
                .error_for_status()
                .map_err(|error| format!("getRecentPrioritizationFees HTTP status: {error}"))?
                .json::<RpcResponse<Vec<RecentPrioritizationFee>>>()
                .await
                .map_err(|error| format!("decode getRecentPrioritizationFees response: {error}"))
        };

        let response = if self.http_timeout_ms > 0 {
            tokio::time::timeout(Duration::from_millis(self.http_timeout_ms), request)
                .await
                .map_err(|_| {
                    format!(
                        "getRecentPrioritizationFees timed out after {}ms",
                        self.http_timeout_ms
                    )
                })??
        } else {
            request.await?
        };

        if let Some(error) = response.error {
            return Err(format!(
                "getRecentPrioritizationFees RPC error: {}",
                error.message
            ));
        }
        response
            .result
            .ok_or_else(|| "getRecentPrioritizationFees result missing".to_string())
    }
}

fn normalized_account_strings(accounts: &[Pubkey]) -> Vec<String> {
    let mut accounts = accounts.iter().map(ToString::to_string).collect::<Vec<_>>();
    accounts.sort();
    accounts.dedup();
    accounts
}

fn account_key(accounts: &[String]) -> String {
    accounts.join(",")
}

fn percentile_priority_fee(samples: &[RecentPrioritizationFee], percentile: u8) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut fees = samples
        .iter()
        .map(|sample| sample.prioritization_fee)
        .collect::<Vec<_>>();
    fees.sort_unstable();
    let percentile = usize::from(percentile.min(100));
    let index = if percentile == 0 {
        0
    } else {
        ((fees.len() * percentile).saturating_add(99) / 100).saturating_sub(1)
    };
    fees[index.min(fees.len().saturating_sub(1))]
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
    fn percentile_priority_fee_uses_configured_percentile() {
        let samples = [0, 10, 30, 20]
            .into_iter()
            .map(|prioritization_fee| RecentPrioritizationFee { prioritization_fee })
            .collect::<Vec<_>>();

        assert_eq!(percentile_priority_fee(&samples, 0), 0);
        assert_eq!(percentile_priority_fee(&samples, 50), 10);
        assert_eq!(percentile_priority_fee(&samples, 75), 20);
        assert_eq!(percentile_priority_fee(&samples, 100), 30);
    }

    #[test]
    fn observe_writable_accounts_registers_sorted_unique_key_without_network() {
        let first = Pubkey::new_unique();
        let second = Pubkey::new_unique();
        let cache =
            PriorityFeeCache::new(vec!["http://127.0.0.1:8899".to_string()], 0, 5_000, 1, 75);

        let lookup = cache.observe_writable_accounts(&[second, first, second]);

        assert_eq!(lookup.account_count, 2);
        assert!(lookup.priority_fee_micro_lamports.is_none());
        assert_eq!(cache.inner.tracked.lock().unwrap().len(), 2);
    }

    #[test]
    fn observe_writable_accounts_uses_max_fresh_single_account_entry() {
        let first = Pubkey::new_unique();
        let second = Pubkey::new_unique();
        let cache =
            PriorityFeeCache::new(vec!["http://127.0.0.1:8899".to_string()], 0, 5_000, 1, 75);
        let mut entries = HashMap::new();
        entries.insert(
            first.to_string(),
            PriorityFeeEntry {
                priority_fee_micro_lamports: 100,
                fetched_at_ms: now_ms(),
                sample_count: 2,
            },
        );
        entries.insert(
            second.to_string(),
            PriorityFeeEntry {
                priority_fee_micro_lamports: 250,
                fetched_at_ms: now_ms(),
                sample_count: 3,
            },
        );
        cache.inner.entries.store(Arc::new(entries));

        let lookup = cache.observe_writable_accounts(&[first, second]);

        assert_eq!(lookup.account_count, 2);
        assert_eq!(lookup.priority_fee_micro_lamports, Some(250));
        assert_eq!(lookup.sample_count, Some(3));
    }
}
