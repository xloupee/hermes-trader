use crate::parser::{
    Action, ComputeBudgetInfo, ParsedTrade, SharedRouteContext, WalletMentionKind, SOL_MINT,
};
use anyhow::Result;
use serde::Serialize;
use solana_pubkey::Pubkey;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_compute_unit_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_compute_unit_price_micro_lamports: Option<u64>,
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
    pub(crate) copyable: bool,
    pub(crate) decision: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<&'static str>,
    pub(crate) account_key_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_compute_unit_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_compute_unit_price_micro_lamports: Option<u64>,
    #[serde(skip)]
    pub(crate) route_context: Option<SharedRouteContext>,
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
    normalized_event_from_raw(
        observed_at_ms,
        endpoint,
        signature,
        slot,
        account_key_count,
        parsed.target_wallet,
        parsed.action,
        parsed.mint,
        parsed.route,
        parsed.sol_amount,
        parsed.token_amount,
        parsed.compute_budget,
        parsed.copyable,
    )
}

pub(crate) fn normalized_event_from_raw(
    observed_at_ms: u128,
    endpoint: String,
    signature: String,
    slot: u64,
    account_key_count: usize,
    target_wallet: Pubkey,
    action: Action,
    mint: Pubkey,
    route: crate::parser::Route,
    sol_amount: Option<f64>,
    token_amount: Option<f64>,
    compute_budget: ComputeBudgetInfo,
    copyable: bool,
) -> NormalizedCopyTradeEvent {
    let target_wallet = target_wallet.to_string();
    let mint = mint.to_string();
    let (input, output) = match action {
        Action::Buy => (
            NormalizedAsset {
                mint: SOL_MINT.to_string(),
                amount: sol_amount,
            },
            NormalizedAsset {
                mint: mint.clone(),
                amount: token_amount,
            },
        ),
        Action::Sell => (
            NormalizedAsset {
                mint: mint.clone(),
                amount: token_amount,
            },
            NormalizedAsset {
                mint: SOL_MINT.to_string(),
                amount: sol_amount,
            },
        ),
    };

    NormalizedCopyTradeEvent {
        schema: "copytrade.feed.event.v1",
        observed_at_ms,
        provider: "shredstream",
        source: "jito-proxy",
        endpoint,
        target_wallet,
        action,
        mint,
        signature,
        slot,
        route,
        sol_amount,
        token_amount,
        input,
        output,
        copyable,
        filters: vec!["jito-entry".to_string()],
        account_key_count,
        source_compute_unit_limit: compute_budget.compute_unit_limit,
        source_compute_unit_price_micro_lamports: compute_budget.compute_unit_price_micro_lamports,
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
    let copyable = parsed.copyable;
    ShadowSignalLine {
        schema: "copytrade.shadowSignal.v1",
        observed_at_ms,
        provider: "shredstream",
        source: "jito-proxy",
        endpoint,
        target_wallet: parsed.target_wallet.to_string(),
        action: parsed.action,
        mint: parsed.mint.to_string(),
        signature,
        slot,
        route: parsed.route,
        sol_amount: parsed.sol_amount,
        token_amount: parsed.token_amount,
        copyable,
        decision: if copyable { "wouldCopy" } else { "skip" },
        reason: if copyable {
            None
        } else if parsed.action == Action::Buy {
            Some("parsed buy layout is not supported for copy execution")
        } else {
            Some("shadow mode only copies buy actions")
        },
        account_key_count,
        source_compute_unit_limit: parsed.compute_budget.compute_unit_limit,
        source_compute_unit_price_micro_lamports: parsed
            .compute_budget
            .compute_unit_price_micro_lamports,
        route_context: parsed.route_context.clone(),
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
    use solana_pubkey::Pubkey;
    use std::str::FromStr;

    const TARGET_WALLET: &str = "CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o";
    const MINT: &str = "9BB6NFEcjBCtnNLFko2FqVQBq8HHM13kCyYcdQbgpump";

    #[test]
    fn shadow_signal_for_buy_records_would_copy_decision() {
        let line = shadow_signal_line(
            123,
            "http://127.0.0.1:9999".to_string(),
            "sig".to_string(),
            456,
            12,
            &ParsedTrade {
                target_wallet: pubkey(TARGET_WALLET),
                action: Action::Buy,
                mint: pubkey(MINT),
                route: Route::FlashxPump,
                sol_amount: Some(0.00099),
                token_amount: None,
                copyable: true,
                compute_budget: Default::default(),
                route_context: None,
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
                target_wallet: pubkey(TARGET_WALLET),
                action: Action::Sell,
                mint: pubkey(MINT),
                route: Route::FlashxPump,
                sol_amount: None,
                token_amount: Some(42.0),
                copyable: false,
                compute_budget: Default::default(),
                route_context: None,
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

    #[test]
    fn unsupported_buy_records_layout_specific_skip_reason() {
        let line = shadow_signal_line(
            123,
            "http://127.0.0.1:9999".to_string(),
            "sig".to_string(),
            456,
            12,
            &ParsedTrade {
                target_wallet: pubkey(TARGET_WALLET),
                action: Action::Buy,
                mint: pubkey(MINT),
                route: Route::FlashxPump,
                sol_amount: Some(0.001),
                token_amount: None,
                copyable: false,
                compute_budget: Default::default(),
                route_context: None,
            },
        );

        assert_eq!(
            line.reason,
            Some("parsed buy layout is not supported for copy execution")
        );
    }

    fn pubkey(value: &str) -> Pubkey {
        Pubkey::from_str(value).expect("fixture pubkey is valid")
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
