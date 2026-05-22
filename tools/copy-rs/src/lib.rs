use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path};

pub const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const PUMPPORTAL_TRADE_LOCAL_URL: &str = "https://pumpportal.fun/api/trade-local";
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
const BUY_ONLY_SKIP_REASON: &str = "only SOL to token buys are copied in dry-run v1";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeliusSwapEvent {
    #[serde(rename = "type")]
    pub event_type: Option<String>,
    pub source: Option<String>,
    pub fee_payer: Option<String>,
    pub signature: Option<String>,
    #[serde(default)]
    pub native_transfers: Vec<NativeTransfer>,
    #[serde(default)]
    pub token_transfers: Vec<TokenTransfer>,
    #[serde(default)]
    pub account_data: Vec<AccountData>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTransfer {
    pub from_user_account: Option<String>,
    pub to_user_account: Option<String>,
    pub amount: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenTransfer {
    pub from_user_account: Option<String>,
    pub to_user_account: Option<String>,
    pub mint: Option<String>,
    pub token_amount: Option<f64>,
    pub amount: Option<f64>,
    pub symbol: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountData {
    pub account: Option<String>,
    pub native_balance_change: Option<f64>,
    #[serde(default)]
    pub token_balance_changes: Vec<TokenBalanceChange>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenBalanceChange {
    pub user_account: Option<String>,
    pub mint: Option<String>,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub raw_token_amount: Option<RawTokenAmount>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTokenAmount {
    pub token_amount: Option<String>,
    pub decimals: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyPlan {
    pub decision: Decision,
    pub target_wallet: String,
    pub source_signature: Option<String>,
    pub input_mint: Option<String>,
    pub output_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_name: Option<String>,
    pub target_input_amount: Option<f64>,
    pub target_output_amount: Option<f64>,
    pub copy_input_mint: Option<String>,
    pub copy_input_amount: Option<f64>,
    pub copy_output_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Copy,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PumpPortalLocalTradeRequest {
    pub public_key: String,
    pub action: String,
    pub mint: String,
    pub amount: f64,
    pub denominated_in_sol: String,
    pub slippage: f64,
    pub priority_fee: f64,
    pub pool: PumpPortalPool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PumpPortalPool {
    Auto,
    Pump,
    PumpAmm,
    Raydium,
    RaydiumCpmm,
    Launchlab,
    Bonk,
}

impl std::str::FromStr for PumpPortalPool {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "pump" => Ok(Self::Pump),
            "pump-amm" => Ok(Self::PumpAmm),
            "raydium" => Ok(Self::Raydium),
            "raydium-cpmm" => Ok(Self::RaydiumCpmm),
            "launchlab" => Ok(Self::Launchlab),
            "bonk" => Ok(Self::Bonk),
            _ => bail!(
                "unsupported PumpPortal pool `{value}`; use auto, pump, pump-amm, raydium, raydium-cpmm, launchlab, or bonk"
            ),
        }
    }
}

impl std::fmt::Display for PumpPortalPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Auto => "auto",
            Self::Pump => "pump",
            Self::PumpAmm => "pump-amm",
            Self::Raydium => "raydium",
            Self::RaydiumCpmm => "raydium-cpmm",
            Self::Launchlab => "launchlab",
            Self::Bonk => "bonk",
        };

        formatter.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PumpPortalBuildPlan {
    pub decision: Decision,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<PumpPortalLocalTradeRequest>,
}

#[derive(Debug, Clone, PartialEq)]
struct SwapSide {
    mint: String,
    amount: f64,
    symbol: Option<String>,
    name: Option<String>,
}

pub fn load_events(path: impl AsRef<Path>) -> Result<Vec<HeliusSwapEvent>> {
    let body = fs::read_to_string(path.as_ref())
        .with_context(|| format!("could not read {}", path.as_ref().display()))?;
    parse_events(&body)
}

pub fn parse_events(body: &str) -> Result<Vec<HeliusSwapEvent>> {
    let value: Value = serde_json::from_str(body).context("input is not valid JSON")?;
    let values = match value {
        Value::Array(values) => values,
        value @ Value::Object(_) => vec![value],
        _ => bail!("input must be a Helius event object or array"),
    };

    values
        .into_iter()
        .map(|value| serde_json::from_value(value).context("could not parse Helius swap event"))
        .collect()
}

pub fn plan_first_copy(events: &[HeliusSwapEvent], target_wallet: &str, copy_sol: f64) -> CopyPlan {
    if !copy_sol.is_finite() || copy_sol <= 0.0 {
        return skip(
            target_wallet,
            None,
            "copy SOL amount must be greater than 0",
        );
    }

    for event in events {
        let plan = plan_copy(event, target_wallet, copy_sol);

        if plan.decision == Decision::Copy {
            return plan;
        }
    }

    if let Some(event) = events.first() {
        plan_copy(event, target_wallet, copy_sol)
    } else {
        skip(target_wallet, None, "input file did not contain any events")
    }
}

pub fn plan_copy(event: &HeliusSwapEvent, target_wallet: &str, copy_sol: f64) -> CopyPlan {
    if event
        .event_type
        .as_deref()
        .map(str::to_uppercase)
        .as_deref()
        != Some("SWAP")
    {
        return skip(
            target_wallet,
            event.signature.clone(),
            "event type is not SWAP",
        );
    }

    if !affected_wallets(event).contains(target_wallet) {
        return skip(
            target_wallet,
            event.signature.clone(),
            "target wallet is not involved in this swap",
        );
    }

    let input = pick_input(event, target_wallet);
    let output = pick_output(event, target_wallet);

    let Some(input) = input else {
        return skip(
            target_wallet,
            event.signature.clone(),
            "input asset could not be inferred",
        );
    };

    let Some(output) = output else {
        return skip(
            target_wallet,
            event.signature.clone(),
            "output mint could not be inferred",
        );
    };

    if input.mint != SOL_MINT {
        return skip(target_wallet, event.signature.clone(), BUY_ONLY_SKIP_REASON);
    }

    if output.mint == SOL_MINT {
        return skip(target_wallet, event.signature.clone(), BUY_ONLY_SKIP_REASON);
    }

    CopyPlan {
        decision: Decision::Copy,
        target_wallet: target_wallet.to_owned(),
        source_signature: event.signature.clone(),
        input_mint: Some(input.mint),
        output_mint: Some(output.mint.clone()),
        output_symbol: output.symbol.clone(),
        output_name: output.name.clone(),
        target_input_amount: Some(input.amount),
        target_output_amount: Some(output.amount),
        copy_input_mint: Some(SOL_MINT.to_owned()),
        copy_input_amount: Some(copy_sol),
        copy_output_mint: Some(output.mint),
        skip_reason: None,
    }
}

pub fn build_pumpportal_local_request(
    plan: &CopyPlan,
    public_key: &str,
    slippage: f64,
    priority_fee: f64,
    pool: PumpPortalPool,
) -> PumpPortalBuildPlan {
    if plan.decision != Decision::Copy {
        return PumpPortalBuildPlan {
            decision: Decision::Skip,
            skip_reason: Some(
                plan.skip_reason
                    .clone()
                    .unwrap_or_else(|| "copy plan was skipped".to_owned()),
            ),
            request: None,
        };
    }

    if public_key.trim().is_empty() {
        return PumpPortalBuildPlan {
            decision: Decision::Skip,
            skip_reason: Some("public wallet address is required".to_owned()),
            request: None,
        };
    }

    if !is_solana_public_key(public_key.trim()) {
        return PumpPortalBuildPlan {
            decision: Decision::Skip,
            skip_reason: Some(
                "--public-key must be a valid Solana public wallet address, not a private key, label, or placeholder"
                    .to_owned(),
            ),
            request: None,
        };
    }

    let Some(mint) = plan
        .copy_output_mint
        .clone()
        .or_else(|| plan.output_mint.clone())
    else {
        return PumpPortalBuildPlan {
            decision: Decision::Skip,
            skip_reason: Some("copy output mint is missing".to_owned()),
            request: None,
        };
    };

    if !is_solana_public_key(&mint) {
        return PumpPortalBuildPlan {
            decision: Decision::Skip,
            skip_reason: Some("copy output mint is not a valid Solana mint address".to_owned()),
            request: None,
        };
    }

    let Some(amount) = plan.copy_input_amount else {
        return PumpPortalBuildPlan {
            decision: Decision::Skip,
            skip_reason: Some("copy input amount is missing".to_owned()),
            request: None,
        };
    };

    if !amount.is_finite() || amount <= 0.0 {
        return PumpPortalBuildPlan {
            decision: Decision::Skip,
            skip_reason: Some("copy input amount must be greater than 0".to_owned()),
            request: None,
        };
    }

    if !slippage.is_finite() || slippage <= 0.0 {
        return PumpPortalBuildPlan {
            decision: Decision::Skip,
            skip_reason: Some("slippage must be greater than 0".to_owned()),
            request: None,
        };
    }

    if !priority_fee.is_finite() || priority_fee < 0.0 {
        return PumpPortalBuildPlan {
            decision: Decision::Skip,
            skip_reason: Some("priority fee must be 0 or greater".to_owned()),
            request: None,
        };
    }

    PumpPortalBuildPlan {
        decision: Decision::Copy,
        skip_reason: None,
        request: Some(PumpPortalLocalTradeRequest {
            public_key: public_key.trim().to_owned(),
            action: "buy".to_owned(),
            mint,
            amount,
            denominated_in_sol: "true".to_owned(),
            slippage,
            priority_fee,
            pool,
        }),
    }
}

fn is_solana_public_key(value: &str) -> bool {
    bs58::decode(value)
        .into_vec()
        .map(|bytes| bytes.len() == 32)
        .unwrap_or(false)
}

fn skip(target_wallet: &str, signature: Option<String>, reason: impl Into<String>) -> CopyPlan {
    CopyPlan {
        decision: Decision::Skip,
        target_wallet: target_wallet.to_owned(),
        source_signature: signature,
        input_mint: None,
        output_mint: None,
        output_symbol: None,
        output_name: None,
        target_input_amount: None,
        target_output_amount: None,
        copy_input_mint: None,
        copy_input_amount: None,
        copy_output_mint: None,
        skip_reason: Some(reason.into()),
    }
}

fn affected_wallets(event: &HeliusSwapEvent) -> BTreeSet<&str> {
    let mut wallets = BTreeSet::new();

    if let Some(fee_payer) = event.fee_payer.as_deref() {
        wallets.insert(fee_payer);
    }

    for transfer in &event.native_transfers {
        if let Some(address) = transfer.from_user_account.as_deref() {
            wallets.insert(address);
        }

        if let Some(address) = transfer.to_user_account.as_deref() {
            wallets.insert(address);
        }
    }

    for transfer in &event.token_transfers {
        if let Some(address) = transfer.from_user_account.as_deref() {
            wallets.insert(address);
        }

        if let Some(address) = transfer.to_user_account.as_deref() {
            wallets.insert(address);
        }
    }

    for account in &event.account_data {
        if let Some(address) = account.account.as_deref() {
            wallets.insert(address);
        }
    }

    wallets
}

fn pick_input(event: &HeliusSwapEvent, target_wallet: &str) -> Option<SwapSide> {
    event
        .token_transfers
        .iter()
        .find(|transfer| transfer.from_user_account.as_deref() == Some(target_wallet))
        .and_then(token_transfer_side)
        .or_else(|| {
            event
                .native_transfers
                .iter()
                .find(|transfer| transfer.from_user_account.as_deref() == Some(target_wallet))
                .and_then(native_transfer_side)
        })
        .or_else(|| pick_token_balance_side(event, target_wallet, BalanceDirection::Negative))
        .or_else(|| pick_native_balance_side(event, target_wallet, BalanceDirection::Negative))
}

fn pick_output(event: &HeliusSwapEvent, target_wallet: &str) -> Option<SwapSide> {
    event
        .token_transfers
        .iter()
        .find(|transfer| transfer.to_user_account.as_deref() == Some(target_wallet))
        .and_then(token_transfer_side)
        .or_else(|| {
            event
                .native_transfers
                .iter()
                .find(|transfer| transfer.to_user_account.as_deref() == Some(target_wallet))
                .and_then(native_transfer_side)
        })
        .or_else(|| pick_token_balance_side(event, target_wallet, BalanceDirection::Positive))
        .or_else(|| pick_native_balance_side(event, target_wallet, BalanceDirection::Positive))
}

fn token_transfer_side(transfer: &TokenTransfer) -> Option<SwapSide> {
    Some(SwapSide {
        mint: transfer.mint.clone()?,
        amount: transfer.token_amount.or(transfer.amount)?,
        symbol: transfer.symbol.clone(),
        name: transfer.name.clone(),
    })
}

fn native_transfer_side(transfer: &NativeTransfer) -> Option<SwapSide> {
    Some(SwapSide {
        mint: SOL_MINT.to_owned(),
        amount: transfer.amount? / LAMPORTS_PER_SOL,
        symbol: Some("SOL".to_owned()),
        name: Some("Solana".to_owned()),
    })
}

#[derive(Debug, Clone, Copy)]
enum BalanceDirection {
    Negative,
    Positive,
}

fn pick_native_balance_side(
    event: &HeliusSwapEvent,
    target_wallet: &str,
    direction: BalanceDirection,
) -> Option<SwapSide> {
    event
        .account_data
        .iter()
        .find(|account| account.account.as_deref() == Some(target_wallet))
        .and_then(|account| {
            let lamports = account.native_balance_change?;

            match direction {
                BalanceDirection::Negative if lamports < 0.0 => Some(lamports.abs()),
                BalanceDirection::Positive if lamports > 0.0 => Some(lamports),
                _ => None,
            }
        })
        .map(|lamports| SwapSide {
            mint: SOL_MINT.to_owned(),
            amount: lamports / LAMPORTS_PER_SOL,
            symbol: Some("SOL".to_owned()),
            name: Some("Solana".to_owned()),
        })
}

fn pick_token_balance_side(
    event: &HeliusSwapEvent,
    target_wallet: &str,
    direction: BalanceDirection,
) -> Option<SwapSide> {
    event.account_data.iter().find_map(|account| {
        account.token_balance_changes.iter().find_map(|change| {
            if change.user_account.as_deref() != Some(target_wallet) {
                return None;
            }

            let raw = change.raw_token_amount.as_ref()?;
            let raw_amount = raw.token_amount.as_deref()?.parse::<f64>().ok()?;
            let decimals = raw.decimals.unwrap_or(0);
            let signed_amount = raw_amount / 10_f64.powi(decimals);

            match direction {
                BalanceDirection::Negative if signed_amount < 0.0 => Some(SwapSide {
                    mint: change.mint.clone()?,
                    amount: signed_amount.abs(),
                    symbol: change.symbol.clone(),
                    name: change.name.clone(),
                }),
                BalanceDirection::Positive if signed_amount > 0.0 => Some(SwapSide {
                    mint: change.mint.clone()?,
                    amount: signed_amount,
                    symbol: change.symbol.clone(),
                    name: change.name.clone(),
                }),
                _ => None,
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: &str = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg";
    const TOKEN: &str = "DezXAZ8z7PnrnRJjz3n26VsZdrmRwNny4nBj9JkzjW7B";

    fn sol_to_token_event() -> HeliusSwapEvent {
        parse_events(&format!(
            r#"{{
              "type": "SWAP",
              "feePayer": "{TARGET}",
              "signature": "sig1",
              "nativeTransfers": [{{"fromUserAccount": "{TARGET}", "toUserAccount": "Pool", "amount": 125000000}}],
              "tokenTransfers": [{{"fromUserAccount": "Pool", "toUserAccount": "{TARGET}", "mint": "{TOKEN}", "tokenAmount": 250000}}]
            }}"#
        ))
        .unwrap()
        .remove(0)
    }

    #[test]
    fn copies_sol_to_token() {
        let plan = plan_copy(&sol_to_token_event(), TARGET, 0.01);

        assert_eq!(plan.decision, Decision::Copy);
        assert_eq!(plan.input_mint.as_deref(), Some(SOL_MINT));
        assert_eq!(plan.output_mint.as_deref(), Some(TOKEN));
        assert_eq!(plan.copy_input_amount, Some(0.01));
    }

    #[test]
    fn skips_non_swap() {
        let mut event = sol_to_token_event();
        event.event_type = Some("TRANSFER".to_owned());
        let plan = plan_copy(&event, TARGET, 0.01);

        assert_eq!(plan.decision, Decision::Skip);
        assert_eq!(plan.skip_reason.as_deref(), Some("event type is not SWAP"));
    }

    #[test]
    fn builds_pumpportal_local_request_for_copy_plan() {
        let plan = plan_copy(&sol_to_token_event(), TARGET, 0.01);
        let build =
            build_pumpportal_local_request(&plan, TARGET, 10.0, 0.00005, PumpPortalPool::Auto);
        let request = build.request.unwrap();

        assert_eq!(build.decision, Decision::Copy);
        assert_eq!(request.public_key, TARGET);
        assert_eq!(request.action, "buy");
        assert_eq!(request.mint, TOKEN);
        assert_eq!(request.amount, 0.01);
        assert_eq!(request.denominated_in_sol, "true");
        assert_eq!(request.slippage, 10.0);
        assert_eq!(request.priority_fee, 0.00005);
        assert_eq!(request.pool, PumpPortalPool::Auto);
    }

    #[test]
    fn refuses_pumpportal_build_for_skip_plan() {
        let mut event = sol_to_token_event();
        event.event_type = Some("TRANSFER".to_owned());
        let plan = plan_copy(&event, TARGET, 0.01);
        let build =
            build_pumpportal_local_request(&plan, TARGET, 10.0, 0.00005, PumpPortalPool::Auto);

        assert_eq!(build.decision, Decision::Skip);
        assert!(build.request.is_none());
        assert_eq!(build.skip_reason.as_deref(), Some("event type is not SWAP"));
    }

    #[test]
    fn refuses_pumpportal_build_for_invalid_public_key() {
        let plan = plan_copy(&sol_to_token_event(), TARGET, 0.01);
        let build = build_pumpportal_local_request(
            &plan,
            "your_public_wallet_address",
            10.0,
            0.00005,
            PumpPortalPool::Auto,
        );

        assert_eq!(build.decision, Decision::Skip);
        assert!(build.request.is_none());
        assert_eq!(
            build.skip_reason.as_deref(),
            Some("--public-key must be a valid Solana public wallet address, not a private key, label, or placeholder")
        );
    }
}
