use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LeaderSlotRecord {
    pub(crate) slot: u64,
    pub(crate) identity: String,
    pub(crate) ip_address: Option<String>,
    pub(crate) tpu_port: Option<u16>,
    pub(crate) tpu_quic_port: Option<u16>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ErpcLeaderSlotCache {
    refreshed_at_ms: Option<u64>,
    slots: HashMap<u64, LeaderSlotRecord>,
}

impl ErpcLeaderSlotCache {
    pub(crate) fn refresh_from_json(
        &mut self,
        json: &str,
        refreshed_at_ms: u64,
    ) -> Result<usize, serde_json::Error> {
        let response: LeaderSlotsResponse = serde_json::from_str(json)?;
        let slots = response.into_records();
        let count = slots.len();
        self.refreshed_at_ms = Some(refreshed_at_ms);
        self.slots = slots
            .into_iter()
            .map(|record| (record.slot, record))
            .collect();
        Ok(count)
    }

    pub(crate) fn get(&self, slot: u64, now_ms: u64, stale_ms: u64) -> Option<&LeaderSlotRecord> {
        let refreshed_at_ms = self.refreshed_at_ms?;
        if stale_ms == 0 || now_ms.saturating_sub(refreshed_at_ms) > stale_ms {
            return None;
        }
        self.slots.get(&slot)
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }
}

#[derive(Debug, Deserialize)]
struct LeaderSlotsResponse {
    #[serde(default)]
    result: Option<LeaderSlotsResult>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

impl LeaderSlotsResponse {
    fn into_records(self) -> Vec<LeaderSlotRecord> {
        if self.error.is_some() {
            return Vec::new();
        }
        self.result
            .map(LeaderSlotsResult::into_records)
            .unwrap_or_default()
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LeaderSlotsResult {
    Data { data: Vec<LeaderSlotRecordWire> },
    Records(Vec<LeaderSlotRecordWire>),
    SlotMap(HashMap<String, LeaderSlotRecordWire>),
    Wrapped { leaders: Vec<LeaderSlotRecordWire> },
}

impl LeaderSlotsResult {
    fn into_records(self) -> Vec<LeaderSlotRecord> {
        match self {
            Self::Data { data } => data
                .into_iter()
                .filter_map(|record| record.into_record(None))
                .collect(),
            Self::Records(records) => records
                .into_iter()
                .filter_map(|record| record.into_record(None))
                .collect(),
            Self::SlotMap(records) => records
                .into_iter()
                .filter_map(|(slot, record)| record.into_record(slot.parse::<u64>().ok()))
                .collect(),
            Self::Wrapped { leaders } => leaders
                .into_iter()
                .filter_map(|record| record.into_record(None))
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct LeaderSlotRecordWire {
    #[serde(default)]
    slot: Option<SlotWire>,
    #[serde(default, alias = "identityPubkey", alias = "leaderIdentity")]
    identity: Option<String>,
    #[serde(default, alias = "ipAddress", alias = "ip")]
    ip_address: Option<String>,
    #[serde(default, alias = "tpuPort")]
    tpu_port: Option<u16>,
    #[serde(default, alias = "tpuQuicPort")]
    tpu_quic_port: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SlotWire {
    Number(u64),
    String(String),
}

impl SlotWire {
    fn into_u64(self) -> Option<u64> {
        match self {
            Self::Number(slot) => Some(slot),
            Self::String(slot) => slot.parse::<u64>().ok(),
        }
    }
}

impl LeaderSlotRecordWire {
    fn into_record(self, slot_hint: Option<u64>) -> Option<LeaderSlotRecord> {
        Some(LeaderSlotRecord {
            slot: self.slot.and_then(SlotWire::into_u64).or(slot_hint)?,
            identity: self.identity?,
            ip_address: self.ip_address,
            tpu_port: self.tpu_port,
            tpu_quic_port: self.tpu_quic_port,
        })
    }
}

pub(crate) fn leader_slots_rpc_body(start_slot: u64) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getLeaderSlots",
        "params": [start_slot]
    })
}

pub(crate) fn append_erpc_api_key(url: &str, api_key: &str) -> String {
    if api_key.trim().is_empty() || url.contains("api-key=") {
        return url.to_string();
    }
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}api-key={api_key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../fixtures/erpc/get_leader_slots.json");

    #[test]
    fn leader_slot_cache_loads_fixture() {
        let mut cache = ErpcLeaderSlotCache::default();
        let count = cache
            .refresh_from_json(FIXTURE, 1_000)
            .expect("fixture parses");

        assert_eq!(count, 2);
        assert_eq!(cache.len(), 2);
        let leader = cache.get(348_100_001, 1_001, 5_000).expect("slot cached");
        assert_eq!(
            leader.identity,
            "Leader11111111111111111111111111111111111111"
        );
        assert_eq!(leader.ip_address.as_deref(), Some("203.0.113.10"));
        assert_eq!(leader.tpu_port, Some(8003));
        assert_eq!(leader.tpu_quic_port, Some(8004));
    }

    #[test]
    fn leader_slot_cache_fails_closed_when_stale() {
        let mut cache = ErpcLeaderSlotCache::default();
        cache
            .refresh_from_json(FIXTURE, 1_000)
            .expect("fixture parses");

        assert!(cache.get(348_100_001, 7_001, 5_000).is_none());
        assert!(cache.get(348_100_001, 1_001, 0).is_none());
    }

    #[test]
    fn leader_slot_cache_accepts_slot_keyed_map_shape() {
        let mut cache = ErpcLeaderSlotCache::default();
        let count = cache
            .refresh_from_json(
                r#"{"jsonrpc":"2.0","id":1,"result":{"348100003":{"identity":"Leader33333333333333333333333333333333333333","ipAddress":"203.0.113.12","tpuPort":9001,"tpuQuicPort":9007}}}"#,
                1_000,
            )
            .expect("map shape parses");

        assert_eq!(count, 1);
        let leader = cache.get(348_100_003, 1_001, 5_000).expect("slot cached");
        assert_eq!(leader.tpu_quic_port, Some(9007));
    }

    #[test]
    fn leader_slot_cache_fails_closed_on_rpc_error() {
        let mut cache = ErpcLeaderSlotCache::default();
        let count = cache
            .refresh_from_json(
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"nope"}}"#,
                1_000,
            )
            .expect("error response still parses");

        assert_eq!(count, 0);
        assert!(cache.get(348_100_001, 1_001, 5_000).is_none());
    }

    #[test]
    fn leader_slots_rpc_body_uses_documented_method() {
        let body = leader_slots_rpc_body(416_462_031);
        assert_eq!(body["method"], "getLeaderSlots");
        assert_eq!(body["params"][0], 416_462_031);
    }

    #[test]
    fn api_key_is_appended_without_overwriting_existing_key() {
        assert_eq!(
            append_erpc_api_key("https://edge.erpc.global", "secret"),
            "https://edge.erpc.global?api-key=secret"
        );
        assert_eq!(
            append_erpc_api_key("https://edge.erpc.global/path?x=1", "secret"),
            "https://edge.erpc.global/path?x=1&api-key=secret"
        );
        assert_eq!(
            append_erpc_api_key("https://edge.erpc.global?api-key=present", "secret"),
            "https://edge.erpc.global?api-key=present"
        );
    }
}
