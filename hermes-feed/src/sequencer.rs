use std::time::Duration;

use alloy_primitives::{B256, keccak256};
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::robinhood::DIRECT_SEQUENCER_URL;

const CONDITIONS_FAILED_CODE: i64 = -32_003;
const GENERIC_SERVER_ERROR_CODE: i64 = -32_000;
const RATE_LIMIT_CODE: i64 = -32_005;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct ConditionalOptions {
    pub block_number_min: u64,
    pub block_number_max: u64,
    pub timestamp_min: Option<u64>,
    pub timestamp_max: Option<u64>,
}

impl ConditionalOptions {
    pub fn first_eligible_window(
        launch_l1_block: u64,
        l1_window: u64,
        timestamp_max: Option<u64>,
    ) -> Option<Self> {
        let block_number_min = launch_l1_block.checked_add(1)?;
        let block_number_max = block_number_min.checked_add(l1_window)?;
        Some(Self {
            block_number_min,
            block_number_max,
            timestamp_min: None,
            timestamp_max,
        })
    }

    fn to_rpc_value(self) -> Value {
        let mut value = json!({
            "knownAccounts": {},
            "blockNumberMin": hex_u64(self.block_number_min),
            "blockNumberMax": hex_u64(self.block_number_max),
        });
        if let Some(timestamp_min) = self.timestamp_min {
            value["timestampMin"] = Value::String(hex_u64(timestamp_min));
        }
        if let Some(timestamp_max) = self.timestamp_max {
            value["timestampMax"] = Value::String(hex_u64(timestamp_max));
        }
        value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionalResponse {
    Accepted { tx_hash: B256 },
    BoundaryNotReached { message: String },
    AlreadyKnown { tx_hash: B256, message: String },
    RateLimited { message: String },
    Rejected { code: i64, message: String },
    InvalidResponse(String),
}

#[derive(Clone)]
pub struct SequencerClient {
    client: Client,
    url: String,
}

impl SequencerClient {
    pub fn new() -> Result<Self> {
        Self::with_url(DIRECT_SEQUENCER_URL)
    }

    pub fn with_url(url: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .tcp_nodelay(true)
            .http2_adaptive_window(true)
            .pool_idle_timeout(None)
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(3))
            .build()
            .context("build direct sequencer HTTP/2 client")?;
        Ok(Self {
            client,
            url: url.into(),
        })
    }

    /// Submit one already-signed transaction with Nitro L1-height conditions.
    ///
    /// The caller may retry the exact same raw bytes only after an explicit
    /// `BoundaryNotReached`. An ambiguous transport failure must be reconciled
    /// by transaction hash before any different transaction is signed.
    pub async fn submit_conditional(
        &self,
        raw_transaction: &[u8],
        options: ConditionalOptions,
    ) -> Result<ConditionalResponse> {
        let expected_tx_hash = signed_transaction_hash(raw_transaction);
        let body = build_conditional_request(raw_transaction, options);
        let response = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .with_context(|| {
                format!(
                    "conditional submission for {expected_tx_hash} has an ambiguous transport result"
                )
            })?;
        let status = response.status();
        let bytes = response.bytes().await.with_context(|| {
            format!("conditional submission for {expected_tx_hash} has an ambiguous response body")
        })?;
        if !status.is_success() {
            return Ok(ConditionalResponse::Rejected {
                code: i64::from(status.as_u16()),
                message: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }
        Ok(classify_conditional_response(&bytes, expected_tx_hash))
    }
}

pub fn build_conditional_request(raw_transaction: &[u8], options: ConditionalOptions) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_sendRawTransactionConditional",
        "params": [format!("0x{}", hex::encode(raw_transaction)), options.to_rpc_value()],
    }))
    .expect("conditional request contains only serializable values")
}

pub fn classify_conditional_response(bytes: &[u8], expected_tx_hash: B256) -> ConditionalResponse {
    let value: Value = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        Err(error) => return ConditionalResponse::InvalidResponse(error.to_string()),
    };
    if let Some(result) = value.get("result").and_then(Value::as_str) {
        return match result.parse::<B256>() {
            Ok(tx_hash) if tx_hash == expected_tx_hash => ConditionalResponse::Accepted { tx_hash },
            Ok(tx_hash) => ConditionalResponse::InvalidResponse(format!(
                "sequencer returned transaction hash {tx_hash}, expected {expected_tx_hash}"
            )),
            Err(error) => ConditionalResponse::InvalidResponse(error.to_string()),
        };
    }
    let Some(error) = value.get("error") else {
        return ConditionalResponse::InvalidResponse("missing result and error".into());
    };
    let code = error
        .get("code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown JSON-RPC error")
        .to_owned();
    if matches!(code, CONDITIONS_FAILED_CODE | GENERIC_SERVER_ERROR_CODE)
        && message.contains("BlockNumberMin condition not met")
    {
        ConditionalResponse::BoundaryNotReached { message }
    } else if code == RATE_LIMIT_CODE || message.to_ascii_lowercase().contains("rate limit") {
        ConditionalResponse::RateLimited { message }
    } else if message.to_ascii_lowercase().contains("already known") {
        ConditionalResponse::AlreadyKnown {
            tx_hash: expected_tx_hash,
            message,
        }
    } else {
        ConditionalResponse::Rejected { code, message }
    }
}

pub fn signed_transaction_hash(raw_transaction: &[u8]) -> B256 {
    keccak256(raw_transaction)
}

fn hex_u64(value: u64) -> String {
    format!("0x{value:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_uses_l1_boundary_fields_and_exact_raw_bytes() {
        let options =
            ConditionalOptions::first_eligible_window(25_508_851, 3, Some(1_800_000_000)).unwrap();
        let request: Value =
            serde_json::from_slice(&build_conditional_request(&[0x02, 0xf8, 0x01], options))
                .unwrap();
        assert_eq!(request["method"], "eth_sendRawTransactionConditional");
        assert_eq!(request["params"][0], "0x02f801");
        assert_eq!(request["params"][1]["blockNumberMin"], "0x1853bf4");
        assert_eq!(request["params"][1]["blockNumberMax"], "0x1853bf7");
        assert_eq!(request["params"][1]["timestampMax"], "0x6b49d200");
        assert_eq!(request["params"][1]["knownAccounts"], json!({}));
    }

    #[test]
    fn only_explicit_unmet_min_is_a_safe_boundary_retry() {
        let expected = B256::with_last_byte(1);
        let response = classify_conditional_response(
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32003,"message":"conditions check failed for old state:BlockNumberMin condition not met"}}"#,
            expected,
        );
        assert!(matches!(
            response,
            ConditionalResponse::BoundaryNotReached { .. }
        ));

        let expired = classify_conditional_response(
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32003,"message":"BlockNumberMax condition not met"}}"#,
            expected,
        );
        assert!(matches!(expired, ConditionalResponse::Rejected { .. }));

        let testnet_variant = classify_conditional_response(
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"BlockNumberMin condition not met"}}"#,
            expected,
        );
        assert!(matches!(
            testnet_variant,
            ConditionalResponse::BoundaryNotReached { .. }
        ));
    }

    #[test]
    fn accepts_transaction_hash() {
        let expected = B256::with_last_byte(7);
        let body = format!(r#"{{"jsonrpc":"2.0","id":1,"result":"{expected}"}}"#);
        assert_eq!(
            classify_conditional_response(body.as_bytes(), expected),
            ConditionalResponse::Accepted { tx_hash: expected }
        );
    }

    #[test]
    fn rejects_a_success_hash_that_does_not_match_the_submitted_bytes() {
        let expected = B256::with_last_byte(7);
        let returned = B256::with_last_byte(8);
        let body = format!(r#"{{"jsonrpc":"2.0","id":1,"result":"{returned}"}}"#);
        let response = classify_conditional_response(body.as_bytes(), expected);
        assert!(
            matches!(response, ConditionalResponse::InvalidResponse(message)
            if message.contains(&returned.to_string()) && message.contains(&expected.to_string()))
        );
    }

    #[test]
    fn already_known_reconciles_to_the_submitted_hash() {
        let expected = B256::with_last_byte(9);
        let response = classify_conditional_response(
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"already known"}}"#,
            expected,
        );
        assert!(matches!(
            response,
            ConditionalResponse::AlreadyKnown { tx_hash, .. } if tx_hash == expected
        ));
    }
}
