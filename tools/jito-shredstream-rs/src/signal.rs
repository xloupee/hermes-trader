use crate::{
    event::NormalizedCopyTradeEvent,
    parser::{Action, Route},
    LiveOptions,
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

const BLOCK_TIME_CACHE_CAPACITY: usize = 512;

pub(crate) struct SignalObservationWriter {
    supabase: SupabaseSignalClient,
    block_time: Option<BlockTimeClient>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SignalTimings {
    pub(crate) grpc_message_received_at_ms: u128,
    pub(crate) entries_deserialized_at_ms: u128,
    pub(crate) wallet_match_finished_at_ms: u128,
    pub(crate) trade_parsed_at_ms: u128,
    pub(crate) deserialize_us: u128,
    pub(crate) wallet_match_finished_at_us: u128,
    pub(crate) parse_us: u128,
    pub(crate) local_detect_us: u128,
    pub(crate) batch_transaction_count: u64,
    pub(crate) matched_transaction_index: u64,
    pub(crate) batch_scan_us: u128,
    pub(crate) tx_parse_us: u128,
    pub(crate) account_expand_us: u128,
    pub(crate) wallet_match_us: u128,
    pub(crate) route_parse_us: u128,
}

impl SignalObservationWriter {
    pub(crate) fn from_options(options: &LiveOptions) -> Result<Option<Self>> {
        if options.disable_signal_observations {
            return Ok(None);
        }

        let has_url = options
            .supabase_url
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        let has_key = options
            .supabase_service_role_key
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());

        if !has_url && !has_key {
            return Ok(None);
        }
        if !has_url || !has_key {
            return Err(anyhow!(
                "SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY must both be set to write signal observations"
            ));
        }

        Ok(Some(Self {
            supabase: SupabaseSignalClient::new(
                options.supabase_url.clone().unwrap(),
                options.supabase_service_role_key.clone().unwrap(),
                options.signal_table.clone(),
            )?,
            block_time: options
                .solana_rpc_url
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .map(|url| BlockTimeClient::new(url.clone())),
        }))
    }

    pub(crate) async fn write(
        &mut self,
        event: &NormalizedCopyTradeEvent,
        timings: SignalTimings,
    ) -> Result<()> {
        let block_time_ms = match &mut self.block_time {
            Some(client) => client.block_time_ms(event.slot).await?,
            None => None,
        };

        let row = SignalObservationRow::from_event(event, timings, block_time_ms)?;
        self.supabase.insert(&row).await
    }
}

struct SupabaseSignalClient {
    client: reqwest::Client,
    url: String,
    service_role_key: String,
}

impl SupabaseSignalClient {
    fn new(supabase_url: String, service_role_key: String, table: String) -> Result<Self> {
        let base = supabase_url.trim().trim_end_matches('/');
        if base.is_empty() {
            return Err(anyhow!("SUPABASE_URL is empty"));
        }
        let table = table.trim();
        if table.is_empty() {
            return Err(anyhow!("JITO_SIGNAL_TABLE is empty"));
        }

        Ok(Self {
            client: reqwest::Client::new(),
            url: format!(
                "{base}/rest/v1/{table}?on_conflict=provider,signature,target_wallet,action,mint"
            ),
            service_role_key,
        })
    }

    async fn insert(&self, row: &SignalObservationRow) -> Result<()> {
        let response = self
            .client
            .post(&self.url)
            .header("apikey", &self.service_role_key)
            .bearer_auth(&self.service_role_key)
            .header("Prefer", "resolution=ignore-duplicates,return=minimal")
            .json(row)
            .send()
            .await
            .context("send Supabase signal observation insert")?;

        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(anyhow!(
            "Supabase signal observation insert failed: {status} {body}"
        ))
    }
}

#[derive(Debug, Serialize)]
struct SignalObservationRow {
    provider: &'static str,
    source: &'static str,
    endpoint: String,
    target_wallet: String,
    signature: String,
    slot: i64,
    action: String,
    mint: String,
    route: String,
    observed_at_ms: i64,
    grpc_message_received_at_ms: i64,
    entries_deserialized_at_ms: i64,
    trade_parsed_at_ms: i64,
    block_time_ms: Option<i64>,
    observed_minus_block_time_ms: Option<i64>,
    grpc_received_minus_block_time_ms: Option<i64>,
    deserialize_ms: i64,
    parse_ms: i64,
    local_detect_ms: i64,
    deserialize_us: i64,
    parse_us: i64,
    local_detect_us: i64,
    batch_transaction_count: i64,
    matched_transaction_index: i64,
    batch_scan_us: i64,
    tx_parse_us: i64,
    account_expand_us: i64,
    wallet_match_us: i64,
    route_parse_us: i64,
    sol_amount: Option<f64>,
    token_amount: Option<f64>,
    copyable: bool,
    raw_event: serde_json::Value,
}

impl SignalObservationRow {
    fn from_event(
        event: &NormalizedCopyTradeEvent,
        timings: SignalTimings,
        block_time_ms: Option<i64>,
    ) -> Result<Self> {
        let observed_at_ms = i64::try_from(event.observed_at_ms)
            .context("observed_at_ms does not fit into bigint")?;
        let grpc_message_received_at_ms = i64::try_from(timings.grpc_message_received_at_ms)
            .context("grpc_message_received_at_ms does not fit into bigint")?;
        let entries_deserialized_at_ms = i64::try_from(timings.entries_deserialized_at_ms)
            .context("entries_deserialized_at_ms does not fit into bigint")?;
        let trade_parsed_at_ms = i64::try_from(timings.trade_parsed_at_ms)
            .context("trade_parsed_at_ms does not fit into bigint")?;
        let deserialize_us = i64::try_from(timings.deserialize_us)
            .context("deserialize_us does not fit into bigint")?;
        let parse_us =
            i64::try_from(timings.parse_us).context("parse_us does not fit into bigint")?;
        let local_detect_us = i64::try_from(timings.local_detect_us)
            .context("local_detect_us does not fit into bigint")?;
        let batch_transaction_count = i64::try_from(timings.batch_transaction_count)
            .context("batch_transaction_count does not fit into bigint")?;
        let matched_transaction_index = i64::try_from(timings.matched_transaction_index)
            .context("matched_transaction_index does not fit into bigint")?;
        let batch_scan_us = i64::try_from(timings.batch_scan_us)
            .context("batch_scan_us does not fit into bigint")?;
        let tx_parse_us =
            i64::try_from(timings.tx_parse_us).context("tx_parse_us does not fit into bigint")?;
        let account_expand_us = i64::try_from(timings.account_expand_us)
            .context("account_expand_us does not fit into bigint")?;
        let wallet_match_us = i64::try_from(timings.wallet_match_us)
            .context("wallet_match_us does not fit into bigint")?;
        let route_parse_us = i64::try_from(timings.route_parse_us)
            .context("route_parse_us does not fit into bigint")?;
        let slot = i64::try_from(event.slot).context("slot does not fit into bigint")?;

        Ok(Self {
            provider: "jito-shredstream",
            source: event.source,
            endpoint: event.endpoint.clone(),
            target_wallet: event.target_wallet.clone(),
            signature: event.signature.clone(),
            slot,
            action: action_string(event.action),
            mint: event.mint.clone(),
            route: route_string(event.route)?,
            observed_at_ms,
            grpc_message_received_at_ms,
            entries_deserialized_at_ms,
            trade_parsed_at_ms,
            block_time_ms,
            observed_minus_block_time_ms: block_time_ms
                .map(|block_time_ms| observed_at_ms - block_time_ms),
            grpc_received_minus_block_time_ms: block_time_ms
                .map(|block_time_ms| grpc_message_received_at_ms - block_time_ms),
            deserialize_ms: entries_deserialized_at_ms - grpc_message_received_at_ms,
            parse_ms: trade_parsed_at_ms - entries_deserialized_at_ms,
            local_detect_ms: trade_parsed_at_ms - grpc_message_received_at_ms,
            deserialize_us,
            parse_us,
            local_detect_us,
            batch_transaction_count,
            matched_transaction_index,
            batch_scan_us,
            tx_parse_us,
            account_expand_us,
            wallet_match_us,
            route_parse_us,
            sol_amount: event.sol_amount,
            token_amount: event.token_amount,
            copyable: event.copyable,
            raw_event: serde_json::to_value(event).context("serialize raw signal event")?,
        })
    }
}

struct BlockTimeClient {
    client: reqwest::Client,
    rpc_url: String,
    cache: SlotBlockTimeCache,
}

impl BlockTimeClient {
    fn new(rpc_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            rpc_url,
            cache: SlotBlockTimeCache::new(BLOCK_TIME_CACHE_CAPACITY),
        }
    }

    async fn block_time_ms(&mut self, slot: u64) -> Result<Option<i64>> {
        if let Some(block_time_ms) = self.cache.get(slot) {
            return Ok(block_time_ms);
        }

        let response = self
            .client
            .post(&self.rpc_url)
            .json(&RpcRequest {
                jsonrpc: "2.0",
                id: 1,
                method: "getBlockTime",
                params: [slot],
            })
            .send()
            .await
            .with_context(|| format!("request block time for slot {slot}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("getBlockTime failed: {status} {body}"));
        }

        let body: RpcResponse<Option<i64>> = response
            .json()
            .await
            .with_context(|| format!("decode block time response for slot {slot}"))?;
        if let Some(error) = body.error {
            return Err(anyhow!("getBlockTime RPC error: {}", error.message));
        }

        let block_time_ms = body.result.map(|seconds| seconds.saturating_mul(1_000));
        self.cache.insert(slot, block_time_ms);
        Ok(block_time_ms)
    }
}

#[derive(Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'a str,
    id: u8,
    method: &'a str,
    params: [u64; 1],
}

#[derive(Deserialize)]
struct RpcResponse<T> {
    result: T,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    message: String,
}

struct SlotBlockTimeCache {
    capacity: usize,
    values: HashMap<u64, Option<i64>>,
    order: VecDeque<u64>,
}

impl SlotBlockTimeCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            values: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
        }
    }

    fn get(&self, slot: u64) -> Option<Option<i64>> {
        self.values.get(&slot).copied()
    }

    fn insert(&mut self, slot: u64, block_time_ms: Option<i64>) {
        if self.capacity == 0 {
            return;
        }
        if self.values.contains_key(&slot) {
            self.values.insert(slot, block_time_ms);
            return;
        }

        while self.order.len() >= self.capacity {
            if let Some(expired) = self.order.pop_front() {
                self.values.remove(&expired);
            } else {
                break;
            }
        }

        self.values.insert(slot, block_time_ms);
        self.order.push_back(slot);
    }
}

fn action_string(action: Action) -> String {
    match action {
        Action::Buy => "buy",
        Action::Sell => "sell",
    }
    .to_string()
}

fn route_string(route: Route) -> Result<String> {
    let value = serde_json::to_value(route).context("serialize route")?;
    value
        .as_str()
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("route did not serialize to string"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{event::normalized_event, parser::ParsedTrade};

    #[test]
    fn signal_observation_computes_blocktime_lag() {
        let event = normalized_event(
            1_780_450_789_609,
            "http://127.0.0.1:9999".to_string(),
            "sig".to_string(),
            423_928_888,
            15,
            ParsedTrade {
                target_wallet: "wallet".to_string(),
                action: Action::Buy,
                mint: "mintpump".to_string(),
                route: Route::FlashxPump,
                sol_amount: Some(0.00099),
                token_amount: None,
                route_context: None,
            },
        );

        let timings = SignalTimings {
            grpc_message_received_at_ms: 1_780_450_789_600,
            entries_deserialized_at_ms: 1_780_450_789_604,
            wallet_match_finished_at_ms: 1_780_450_789_605,
            trade_parsed_at_ms: 1_780_450_789_609,
            deserialize_us: 4_200,
            wallet_match_finished_at_us: 5_000,
            parse_us: 5_300,
            local_detect_us: 9_500,
            batch_transaction_count: 42,
            matched_transaction_index: 17,
            batch_scan_us: 4_000,
            tx_parse_us: 1_300,
            account_expand_us: 100,
            wallet_match_us: 40,
            route_parse_us: 1_160,
        };
        let row = SignalObservationRow::from_event(&event, timings, Some(1_780_450_788_000))
            .expect("row builds");

        assert_eq!(row.provider, "jito-shredstream");
        assert_eq!(row.action, "buy");
        assert_eq!(row.route, "flashx-pump");
        assert_eq!(row.observed_minus_block_time_ms, Some(1_609));
        assert_eq!(row.grpc_received_minus_block_time_ms, Some(1_600));
        assert_eq!(row.deserialize_ms, 4);
        assert_eq!(row.parse_ms, 5);
        assert_eq!(row.local_detect_ms, 9);
        assert_eq!(row.deserialize_us, 4_200);
        assert_eq!(row.parse_us, 5_300);
        assert_eq!(row.local_detect_us, 9_500);
        assert_eq!(row.batch_transaction_count, 42);
        assert_eq!(row.matched_transaction_index, 17);
        assert_eq!(row.batch_scan_us, 4_000);
        assert_eq!(row.tx_parse_us, 1_300);
        assert_eq!(row.account_expand_us, 100);
        assert_eq!(row.wallet_match_us, 40);
        assert_eq!(row.route_parse_us, 1_160);
        assert_eq!(row.copyable, true);
        assert_eq!(row.raw_event["schema"], "copytrade.feed.event.v1");
    }

    #[test]
    fn slot_block_time_cache_evicts_oldest_slots() {
        let mut cache = SlotBlockTimeCache::new(2);

        cache.insert(1, Some(1000));
        cache.insert(2, None);
        assert_eq!(cache.get(1), Some(Some(1000)));
        assert_eq!(cache.get(2), Some(None));

        cache.insert(3, Some(3000));
        assert_eq!(cache.get(1), None);
        assert_eq!(cache.get(2), Some(None));
        assert_eq!(cache.get(3), Some(Some(3000)));
    }
}
