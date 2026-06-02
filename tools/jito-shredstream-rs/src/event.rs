use crate::parser::{Action, ParsedTrade, WalletMentionKind, SOL_MINT};
use anyhow::Result;
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NormalizedCopyTradeEvent {
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
    input: NormalizedAsset,
    output: NormalizedAsset,
    copyable: bool,
    filters: Vec<String>,
    account_key_count: usize,
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
struct NormalizedAsset {
    mint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    amount: Option<f64>,
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

pub(crate) fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
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
