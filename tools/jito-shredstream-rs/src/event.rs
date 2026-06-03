use crate::parser::{Action, ParsedTrade, WalletMentionKind, SOL_MINT};
use anyhow::Result;
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NormalizedCopyTradeEvent {
    pub(crate) schema: &'static str,
    pub(crate) observed_at_ms: u128,
    pub(crate) provider: &'static str,
    pub(crate) source: &'static str,
    pub(crate) endpoint: String,
    pub(crate) target_wallet: String,
    pub(crate) action: Action,
    pub(crate) mint: String,
    pub(crate) signature: String,
    pub(crate) slot: u64,
    pub(crate) route: crate::parser::Route,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sol_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) token_amount: Option<f64>,
    pub(crate) input: NormalizedAsset,
    pub(crate) output: NormalizedAsset,
    pub(crate) copyable: bool,
    pub(crate) filters: Vec<String>,
    pub(crate) account_key_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RejectionLine {
    pub(crate) schema: &'static str,
    pub(crate) observed_at_ms: u128,
    pub(crate) provider: &'static str,
    pub(crate) source: &'static str,
    pub(crate) endpoint: String,
    pub(crate) signature: String,
    pub(crate) slot: u64,
    pub(crate) reason: String,
    pub(crate) filters: Vec<String>,
    pub(crate) account_key_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WalletMentionLine {
    pub(crate) schema: &'static str,
    pub(crate) observed_at_ms: u128,
    pub(crate) provider: &'static str,
    pub(crate) source: &'static str,
    pub(crate) endpoint: String,
    pub(crate) target_wallet: String,
    pub(crate) signature: String,
    pub(crate) slot: u64,
    pub(crate) reason: String,
    pub(crate) account_key_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShadowSignalLine {
    schema: &'static str,
    observed_at_ms: u128,
    provider: &'static str,
    source: &'static str,
    endpoint: String,
    target_wallet: String,
    action: Action,
    mint: String,
    signature: String,
    slot: u64,
    route: crate::parser::Route,
    #[serde(skip_serializing_if = "Option::is_none")]
    sol_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_amount: Option<f64>,
    copyable: bool,
    decision: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    account_key_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NormalizedAsset {
    pub(crate) mint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) amount: Option<f64>,
}

pub(crate) fn normalized_event(
    observed_at_ms: u128,
    endpoint: String,
    signature: String,
    slot: u64,
    account_key_count: usize,
    parsed: ParsedTrade,
) -> NormalizedCopyTradeEvent {
    let (input, output) = match parsed.action {
        Action::Buy => (
            NormalizedAsset {
                mint: SOL_MINT.to_string(),
                amount: parsed.sol_amount,
            },
            NormalizedAsset {
                mint: parsed.mint.clone(),
                amount: parsed.token_amount,
            },
        ),
        Action::Sell => (
            NormalizedAsset {
                mint: parsed.mint.clone(),
                amount: parsed.token_amount,
            },
            NormalizedAsset {
                mint: SOL_MINT.to_string(),
                amount: parsed.sol_amount,
            },
        ),
    };

    NormalizedCopyTradeEvent {
        schema: "copytrade.feed.event.v1",
        observed_at_ms,
        provider: "shredstream",
        source: "jito-proxy",
        endpoint,
        target_wallet: parsed.target_wallet,
        action: parsed.action,
        mint: parsed.mint,
        signature,
        slot,
        route: parsed.route,
        sol_amount: parsed.sol_amount,
        token_amount: parsed.token_amount,
        input,
        output,
        copyable: matches!(parsed.action, Action::Buy),
        filters: vec!["jito-entry".to_string()],
        account_key_count,
    }
}

pub(crate) fn shadow_signal_line(
    observed_at_ms: u128,
    endpoint: String,
    signature: String,
    slot: u64,
    account_key_count: usize,
    parsed: &ParsedTrade,
) -> ShadowSignalLine {
    let copyable = matches!(parsed.action, Action::Buy);
    ShadowSignalLine {
        schema: "copytrade.shadowSignal.v1",
        observed_at_ms,
        provider: "shredstream",
        source: "jito-proxy",
        endpoint,
        target_wallet: parsed.target_wallet.clone(),
        action: parsed.action,
        mint: parsed.mint.clone(),
        signature,
        slot,
        route: parsed.route,
        sol_amount: parsed.sol_amount,
        token_amount: parsed.token_amount,
        copyable,
        decision: if copyable { "wouldCopy" } else { "skip" },
        reason: if copyable {
            None
        } else {
            Some("shadow mode only copies buy actions")
        },
        account_key_count,
    }
}

pub(crate) fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{ParsedTrade, Route};

    #[test]
    fn shadow_signal_for_buy_records_would_copy_decision() {
        let line = shadow_signal_line(
            123,
            "http://127.0.0.1:9999".to_string(),
            "sig".to_string(),
            456,
            12,
            &ParsedTrade {
                target_wallet: "wallet".to_string(),
                action: Action::Buy,
                mint: "mintpump".to_string(),
                route: Route::FlashxPump,
                sol_amount: Some(0.00099),
                token_amount: None,
            },
        );
        let value = serde_json::to_value(line).expect("shadow signal serializes");

        assert_eq!(value["schema"], "copytrade.shadowSignal.v1");
        assert_eq!(value["decision"], "wouldCopy");
        assert_eq!(value["copyable"], true);
        assert_eq!(value["action"], "buy");
        assert_eq!(value["route"], "flashx-pump");
        assert_eq!(value["solAmount"], 0.00099);
        assert!(value.get("tokenAmount").is_none());
        assert!(value.get("reason").is_none());
    }

    #[test]
    fn shadow_signal_for_sell_records_skip_reason() {
        let line = shadow_signal_line(
            123,
            "http://127.0.0.1:9999".to_string(),
            "sig".to_string(),
            456,
            12,
            &ParsedTrade {
                target_wallet: "wallet".to_string(),
                action: Action::Sell,
                mint: "mintpump".to_string(),
                route: Route::FlashxPump,
                sol_amount: None,
                token_amount: Some(42.0),
            },
        );
        let value = serde_json::to_value(line).expect("shadow signal serializes");

        assert_eq!(value["schema"], "copytrade.shadowSignal.v1");
        assert_eq!(value["decision"], "skip");
        assert_eq!(value["copyable"], false);
        assert_eq!(value["action"], "sell");
        assert_eq!(value["tokenAmount"], 42.0);
        assert_eq!(value["reason"], "shadow mode only copies buy actions");
        assert!(value.get("solAmount").is_none());
    }
}

pub(crate) fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}

pub(crate) fn wallet_mention_schema(kind: WalletMentionKind) -> &'static str {
    match kind {
        WalletMentionKind::NonTrade => "copytrade.feed.walletMention.nonTrade.v1",
        WalletMentionKind::UnsupportedRoute => "copytrade.feed.walletMention.unsupportedRoute.v1",
        WalletMentionKind::Unknown => "copytrade.feed.walletMention.unknown.v1",
    }
}
