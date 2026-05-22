use anyhow::{bail, Context, Result};
use base64::Engine;
use clap::{Parser, Subcommand};
use copy_rs::{
    build_pumpportal_local_request, load_events, plan_copy, plan_first_copy, Decision,
    HeliusSwapEvent, PumpPortalLocalTradeRequest, PumpPortalPool, PUMPPORTAL_TRADE_LOCAL_URL,
};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};
use tokio::time::sleep;

#[derive(Debug, Parser)]
#[command(name = "copy-rs", about = "Local dry-run copy-trade planner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Plan {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        target_wallet: String,
        #[arg(long)]
        copy_sol: f64,
    },
    BuildLocal {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        target_wallet: String,
        #[arg(long)]
        copy_sol: f64,
        #[arg(long)]
        public_key: String,
        #[arg(long, default_value_t = 10.0)]
        slippage: f64,
        #[arg(long, default_value_t = 0.00005)]
        priority_fee: f64,
        #[arg(long, default_value = "auto")]
        pool: String,
        #[arg(long, default_value = PUMPPORTAL_TRADE_LOCAL_URL)]
        pumpportal_url: String,
    },
    Watch {
        #[arg(long)]
        target_wallet: String,
        #[arg(long)]
        copy_sol: f64,
        #[arg(long, default_value_t = 5)]
        interval_seconds: u64,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long, env = "HELIUS_API_KEY")]
        helius_api_key: String,
        #[arg(long, default_value_t = false)]
        json: bool,
        #[arg(long, default_value_t = false)]
        pumpportal_build: bool,
        #[arg(long)]
        public_key: Option<String>,
        #[arg(long, default_value_t = 10.0)]
        slippage: f64,
        #[arg(long, default_value_t = 0.00005)]
        priority_fee: f64,
        #[arg(long, default_value = "auto")]
        pool: String,
        #[arg(long, default_value = PUMPPORTAL_TRADE_LOCAL_URL)]
        pumpportal_url: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Plan {
            input,
            target_wallet,
            copy_sol,
        } => {
            let events = load_events(input)?;
            let plan = plan_first_copy(&events, &target_wallet, copy_sol);
            println!("{}", serde_json::to_string_pretty(&plan)?);
        }
        Command::BuildLocal {
            input,
            target_wallet,
            copy_sol,
            public_key,
            slippage,
            priority_fee,
            pool,
            pumpportal_url,
        } => {
            let pool = PumpPortalPool::from_str(&pool)?;
            let events = load_events(input)?;
            let plan = plan_first_copy(&events, &target_wallet, copy_sol);
            let output = build_pumpportal_output(
                &reqwest::Client::new(),
                &plan,
                &public_key,
                slippage,
                priority_fee,
                pool,
                &pumpportal_url,
            )
            .await?;

            eprintln!("{}", format_pumpportal_summary(output.pumpportal.as_ref()));
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Command::Watch {
            target_wallet,
            copy_sol,
            interval_seconds,
            limit,
            helius_api_key,
            json,
            pumpportal_build,
            public_key,
            slippage,
            priority_fee,
            pool,
            pumpportal_url,
        } => {
            let pumpportal = if pumpportal_build {
                let Some(public_key) = public_key else {
                    bail!("--public-key is required when --pumpportal-build is enabled");
                };

                Some(PumpPortalOptions {
                    public_key,
                    slippage,
                    priority_fee,
                    pool: PumpPortalPool::from_str(&pool)?,
                    url: pumpportal_url,
                })
            } else {
                None
            };

            watch_wallet(
                target_wallet,
                copy_sol,
                interval_seconds,
                limit,
                helius_api_key,
                json,
                pumpportal,
            )
            .await?;
        }
    }

    Ok(())
}

async fn watch_wallet(
    target_wallet: String,
    copy_sol: f64,
    interval_seconds: u64,
    limit: usize,
    helius_api_key: String,
    json: bool,
    pumpportal: Option<PumpPortalOptions>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let mut token_metadata = BTreeMap::new();
    let mut seen = BTreeSet::new();

    let pumpportal_status = if pumpportal.is_some() {
        "; PumpPortal local build enabled; no signing or sending"
    } else {
        ""
    };

    eprintln!(
        "watching {target_wallet}; dry-run only; copy amount {copy_sol} SOL; polling every {interval_seconds}s{pumpportal_status}"
    );

    loop {
        match fetch_swaps(&client, &helius_api_key, &target_wallet, limit).await {
            Ok(mut events) => {
                events.reverse();

                for event in events {
                    let signature = event
                        .signature
                        .clone()
                        .unwrap_or_else(|| format!("unknown-{}", seen.len()));

                    if !seen.insert(signature.clone()) {
                        continue;
                    }

                    let plan = plan_copy(&event, &target_wallet, copy_sol);
                    let mut line = WatchLine {
                        signature,
                        source: event.source.clone(),
                        decision: plan.decision.clone(),
                        skip_reason: plan.skip_reason.clone(),
                        copy_input_amount: plan.copy_input_amount,
                        copy_output_mint: plan.copy_output_mint.clone(),
                        pumpportal: None,
                        plan,
                    };

                    enrich_watch_line(&client, &helius_api_key, &mut token_metadata, &mut line)
                        .await;

                    if let Some(options) = pumpportal.as_ref() {
                        if line.decision == Decision::Copy {
                            line.pumpportal = Some(
                                build_pumpportal_for_watch(&client, &line.plan, options)
                                    .await
                                    .unwrap_or_else(|error| PumpPortalLocalBuildResult {
                                        decision: Decision::Skip,
                                        skip_reason: Some(format!("{error:#}")),
                                        request: None,
                                        response: None,
                                    }),
                            );
                        }
                    }

                    if json {
                        println!("{}", serde_json::to_string(&line)?);
                    } else if line.decision == Decision::Copy {
                        println!("{}", format_watch_line(&line));
                    }

                    if seen.len() > 2000 {
                        let to_keep = seen
                            .iter()
                            .rev()
                            .take(1000)
                            .cloned()
                            .collect::<BTreeSet<_>>();
                        seen = to_keep;
                    }
                }
            }
            Err(error) => {
                eprintln!("watch poll failed: {error:#}");
            }
        }

        sleep(Duration::from_secs(interval_seconds.max(1))).await;
    }
}

async fn build_pumpportal_for_watch(
    client: &reqwest::Client,
    plan: &copy_rs::CopyPlan,
    options: &PumpPortalOptions,
) -> Result<PumpPortalLocalBuildResult> {
    build_pumpportal_local(
        client,
        plan,
        &options.public_key,
        options.slippage,
        options.priority_fee,
        options.pool.clone(),
        &options.url,
    )
    .await
}

async fn build_pumpportal_output(
    client: &reqwest::Client,
    plan: &copy_rs::CopyPlan,
    public_key: &str,
    slippage: f64,
    priority_fee: f64,
    pool: PumpPortalPool,
    url: &str,
) -> Result<BuildLocalOutput> {
    let pumpportal =
        build_pumpportal_local(client, plan, public_key, slippage, priority_fee, pool, url).await?;

    Ok(BuildLocalOutput {
        plan: plan.clone(),
        pumpportal: Some(pumpportal),
    })
}

async fn build_pumpportal_local(
    client: &reqwest::Client,
    plan: &copy_rs::CopyPlan,
    public_key: &str,
    slippage: f64,
    priority_fee: f64,
    pool: PumpPortalPool,
    url: &str,
) -> Result<PumpPortalLocalBuildResult> {
    let build_plan = build_pumpportal_local_request(plan, public_key, slippage, priority_fee, pool);

    let Some(request) = build_plan.request else {
        return Ok(PumpPortalLocalBuildResult {
            decision: Decision::Skip,
            skip_reason: build_plan.skip_reason,
            request: None,
            response: None,
        });
    };

    let response = client
        .post(url)
        .json(&request)
        .send()
        .await
        .context("PumpPortal local transaction request failed")?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let body = response
        .bytes()
        .await
        .context("could not read PumpPortal local transaction response")?;

    let response = PumpPortalLocalBuildResponse {
        ok: status.is_success(),
        status: status.as_u16(),
        content_type,
        body_length: body.len(),
        encoded_transaction_base64: status
            .is_success()
            .then(|| base64::engine::general_purpose::STANDARD.encode(&body)),
        error_text: (!status.is_success()).then(|| String::from_utf8_lossy(&body).to_string()),
    };

    Ok(PumpPortalLocalBuildResult {
        decision: if status.is_success() {
            Decision::Copy
        } else {
            Decision::Skip
        },
        skip_reason: if status.is_success() {
            None
        } else {
            Some(format_pumpportal_error(
                status,
                response.error_text.as_deref(),
            ))
        },
        request: Some(request),
        response: Some(response),
    })
}

fn format_pumpportal_error(status: StatusCode, body: Option<&str>) -> String {
    let body = body.unwrap_or("").trim();
    let hint = if status == StatusCode::BAD_REQUEST {
        " Check that --public-key is your copy wallet public address and that the mint/pool is supported by PumpPortal."
    } else {
        ""
    };

    if body.is_empty() {
        format!("PumpPortal local transaction build failed with HTTP {status}.{hint}")
    } else {
        format!("PumpPortal local transaction build failed with HTTP {status}: {body}.{hint}")
    }
}

async fn fetch_swaps(
    client: &reqwest::Client,
    api_key: &str,
    wallet: &str,
    limit: usize,
) -> Result<Vec<HeliusSwapEvent>> {
    let url = format!(
        "https://api.helius.xyz/v0/addresses/{wallet}/transactions?type=SWAP&limit={limit}&api-key={api_key}",
        limit = limit.clamp(1, 100)
    );
    let response = client
        .get(url)
        .send()
        .await
        .context("Helius request failed")?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("could not read Helius response")?;

    if !status.is_success() {
        anyhow::bail!(
            "Helius HTTP {status}: {}",
            body.chars().take(200).collect::<String>()
        );
    }

    serde_json::from_str(&body).context("could not parse Helius transaction response")
}

async fn enrich_watch_line(
    client: &reqwest::Client,
    api_key: &str,
    cache: &mut BTreeMap<String, TokenMetadata>,
    line: &mut WatchLine,
) {
    if line.plan.output_symbol.is_some() || line.plan.output_name.is_some() {
        return;
    }

    let Some(mint) = line
        .copy_output_mint
        .as_deref()
        .or(line.plan.output_mint.as_deref())
    else {
        return;
    };

    let metadata = if let Some(metadata) = cache.get(mint) {
        metadata.clone()
    } else {
        let metadata = fetch_token_metadata(client, api_key, mint)
            .await
            .unwrap_or_default();
        cache.insert(mint.to_owned(), metadata.clone());
        metadata
    };

    line.plan.output_symbol = metadata.symbol;
    line.plan.output_name = metadata.name;
}

async fn fetch_token_metadata(
    client: &reqwest::Client,
    api_key: &str,
    mint: &str,
) -> Result<TokenMetadata> {
    let url = format!("https://mainnet.helius-rpc.com/?api-key={api_key}");
    let response = client
        .post(url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "copy-rs",
            "method": "getAsset",
            "params": {
                "id": mint,
            },
        }))
        .send()
        .await
        .context("Helius metadata request failed")?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("could not read Helius metadata response")?;

    if !status.is_success() {
        anyhow::bail!("Helius metadata HTTP {status}");
    }

    let value: Value =
        serde_json::from_str(&body).context("could not parse Helius metadata response")?;

    if value.get("error").is_some() {
        anyhow::bail!("Helius metadata response returned an error");
    }

    let symbol = value
        .pointer("/result/content/metadata/symbol")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .pointer("/result/token_info/symbol")
                .and_then(Value::as_str)
        })
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned);
    let name = value
        .pointer("/result/content/metadata/name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned);

    Ok(TokenMetadata { symbol, name })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WatchLine {
    signature: String,
    source: Option<String>,
    decision: Decision,
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_input_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_output_mint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pumpportal: Option<PumpPortalLocalBuildResult>,
    plan: copy_rs::CopyPlan,
}

#[derive(Debug, Clone)]
struct PumpPortalOptions {
    public_key: String,
    slippage: f64,
    priority_fee: f64,
    pool: PumpPortalPool,
    url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildLocalOutput {
    plan: copy_rs::CopyPlan,
    #[serde(skip_serializing_if = "Option::is_none")]
    pumpportal: Option<PumpPortalLocalBuildResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PumpPortalLocalBuildResult {
    decision: Decision,
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request: Option<PumpPortalLocalTradeRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response: Option<PumpPortalLocalBuildResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PumpPortalLocalBuildResponse {
    ok: bool,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
    body_length: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoded_transaction_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_text: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct TokenMetadata {
    symbol: Option<String>,
    name: Option<String>,
}

fn format_watch_line(line: &WatchLine) -> String {
    let source = line.source.as_deref().unwrap_or("UNKNOWN");
    let short_sig = short_signature(&line.signature);

    match &line.decision {
        Decision::Copy => {
            let token = token_label(&line.plan);
            let mint = line
                .plan
                .copy_output_mint
                .as_deref()
                .or(line.plan.output_mint.as_deref())
                .unwrap_or("unknown mint");
            let copy_amount = line
                .copy_input_amount
                .map(format_amount)
                .unwrap_or_else(|| "?".to_owned());
            let target_spent = line
                .plan
                .target_input_amount
                .map(format_amount)
                .unwrap_or_else(|| "?".to_owned());
            let target_received = line
                .plan
                .target_output_amount
                .map(format_amount)
                .unwrap_or_else(|| "?".to_owned());

            format!(
                "[COPY] {source} | {token} | mint {mint} | would spend {copy_amount} SOL | target spent {target_spent} SOL for {target_received} tokens | sig {short_sig}{}",
                pumpportal_suffix(line.pumpportal.as_ref())
            )
        }
        Decision::Skip => {
            let reason = line.skip_reason.as_deref().unwrap_or("skipped");
            let token = token_label(&line.plan);
            let mint = line.plan.output_mint.as_deref().unwrap_or("unknown mint");

            format!("[SKIP] {source} | {reason} | {token} | mint {mint} | sig {short_sig}")
        }
    }
}

fn pumpportal_suffix(result: Option<&PumpPortalLocalBuildResult>) -> String {
    let Some(result) = result else {
        return String::new();
    };

    format!(" | {}", format_pumpportal_summary(Some(result)))
}

fn format_pumpportal_summary(result: Option<&PumpPortalLocalBuildResult>) -> String {
    let Some(result) = result else {
        return "PumpPortal local tx not requested".to_owned();
    };

    match (
        &result.decision,
        result.response.as_ref(),
        result.skip_reason.as_deref(),
    ) {
        (Decision::Copy, Some(response), _) if response.ok => {
            let pool = result
                .request
                .as_ref()
                .map(|request| format!(" via {}", request.pool))
                .unwrap_or_default();
            format!(
                "PumpPortal local tx built{pool}: {} bytes",
                response.body_length
            )
        }
        (_, _, Some(reason)) => reason.to_owned(),
        _ => "PumpPortal local tx not built".to_owned(),
    }
}

fn token_label(plan: &copy_rs::CopyPlan) -> String {
    match (plan.output_symbol.as_deref(), plan.output_name.as_deref()) {
        (Some(symbol), Some(name)) if symbol != name => format!("{symbol} ({name})"),
        (Some(symbol), _) => symbol.to_owned(),
        (_, Some(name)) => name.to_owned(),
        _ => "unknown token".to_owned(),
    }
}

fn short_signature(signature: &str) -> String {
    if signature.len() <= 12 {
        return signature.to_owned();
    }

    format!(
        "{}...{}",
        &signature[..6],
        &signature[signature.len() - 6..]
    )
}

fn format_amount(amount: f64) -> String {
    if amount == 0.0 {
        return "0".to_owned();
    }

    let formatted = if amount.abs() >= 1.0 {
        format!("{amount:.6}")
    } else {
        format!("{amount:.9}")
    };

    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use copy_rs::CopyPlan;

    #[test]
    fn formats_copy_line_with_token_symbol_and_mint() {
        let line = WatchLine {
            signature: "123456789ABCDEFG".to_owned(),
            source: Some("PUMP_FUN".to_owned()),
            decision: Decision::Copy,
            skip_reason: None,
            copy_input_amount: Some(0.01),
            copy_output_mint: Some("MintAddress111".to_owned()),
            pumpportal: None,
            plan: CopyPlan {
                decision: Decision::Copy,
                target_wallet: "wallet".to_owned(),
                source_signature: Some("123456789ABCDEFG".to_owned()),
                input_mint: Some(copy_rs::SOL_MINT.to_owned()),
                output_mint: Some("MintAddress111".to_owned()),
                output_symbol: Some("COIN".to_owned()),
                output_name: Some("Coin Name".to_owned()),
                target_input_amount: Some(0.244493332),
                target_output_amount: Some(581975.94211),
                copy_input_mint: Some(copy_rs::SOL_MINT.to_owned()),
                copy_input_amount: Some(0.01),
                copy_output_mint: Some("MintAddress111".to_owned()),
                skip_reason: None,
            },
        };

        let formatted = format_watch_line(&line);

        assert!(formatted.contains("[COPY] PUMP_FUN"));
        assert!(formatted.contains("COIN (Coin Name)"));
        assert!(formatted.contains("mint MintAddress111"));
        assert!(formatted.contains("would spend 0.01 SOL"));
    }
}
