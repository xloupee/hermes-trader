use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alloy_primitives::{Address, keccak256};
use anyhow::{Context, Result, bail};
use clap::Parser;
use hermes_feed::NoxaRpcClient;
use hermes_feed::robinhood::{
    NOXA_FACTORY_RUNTIME_KECCAK256, NOXA_LAUNCH_FACTORY, PUBLIC_RPC_URL, TESTNET_CHAIN_ID,
    TESTNET_RPC_URL, UNISWAP_V3_SWAP_ROUTER_02, WETH,
};
use reqwest::Client;
use serde::Serialize;
use tokio::time::MissedTickBehavior;

const NOXA_FUN_DOCS_URL: &str = "https://docs.noxa.fi/contracts/noxa-fun/";
const NOXA_DEX_DOCS_URL: &str = "https://docs.noxa.fi/contracts/noxa-dex/";
const ROBINHOOD_CONTRACTS_DOCS_URL: &str = "https://docs.robinhood.com/chain/contracts/";

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Read-only NOXA enablement and Robinhood testnet deployment monitor"
)]
struct Cli {
    #[arg(long, default_value = PUBLIC_RPC_URL)]
    mainnet_rpc_url: String,
    #[arg(long, default_value = TESTNET_RPC_URL)]
    testnet_rpc_url: String,
    #[arg(long, default_value = NOXA_FUN_DOCS_URL)]
    noxa_fun_docs_url: String,
    #[arg(long, default_value = NOXA_DEX_DOCS_URL)]
    noxa_dex_docs_url: String,
    #[arg(long, default_value = ROBINHOOD_CONTRACTS_DOCS_URL)]
    robinhood_contracts_docs_url: String,
    #[arg(long, default_value_t = 300)]
    interval_seconds: u64,
    #[arg(long, default_value_t = false)]
    once: bool,
}

#[derive(Debug, Serialize)]
struct CodeProbe {
    address: Address,
    code_bytes: usize,
    code_present: bool,
}

#[derive(Debug, Serialize)]
struct DocsProbe {
    url: String,
    content_bytes: Option<usize>,
    content_keccak256: Option<String>,
    testnet_contract_marker: Option<bool>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReadinessSnapshot {
    record_type: &'static str,
    checked_unix_seconds: u64,
    mainnet_l1_block: u64,
    mainnet_l2_block: u64,
    launch_enabled: bool,
    factory_runtime_hash_matches_pin: bool,
    testnet_chain_id: u64,
    testnet_weth_candidate: CodeProbe,
    testnet_router_candidate: CodeProbe,
    testnet_noxa_factory_candidate: CodeProbe,
    noxa_fun_docs: DocsProbe,
    noxa_dex_docs: DocsProbe,
    robinhood_contracts_docs: DocsProbe,
    official_documentation_checks_complete: bool,
    official_testnet_contract_docs_detected: bool,
    candidate_testnet_bytecode_detected: bool,
    ready_for_manual_testnet_address_validation: bool,
    ready_for_mainnet_canary_review: bool,
    private_key_used: bool,
    broadcast: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args = Cli::parse();
    if args.interval_seconds == 0 {
        bail!("--interval-seconds must be non-zero");
    }

    let mainnet = NoxaRpcClient::with_url(args.mainnet_rpc_url.as_str())?;
    let testnet = NoxaRpcClient::with_url(args.testnet_rpc_url.as_str())?;
    let docs = Client::builder()
        .user_agent("hermes-readiness-monitor/0.1")
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .context("build official-documentation client")?;

    if args.once {
        emit_snapshot(&mainnet, &testnet, &docs, &args).await?;
        return Ok(());
    }

    let mut interval = tokio::time::interval(Duration::from_secs(args.interval_seconds));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Err(error) = emit_snapshot(&mainnet, &testnet, &docs, &args).await {
                    println!("{}", serde_json::to_string(&serde_json::json!({
                        "record_type": "hermes_readiness_error",
                        "error": error.to_string(),
                        "private_key_used": false,
                        "broadcast": false,
                    }))?);
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal.context("wait for shutdown signal")?;
                break;
            }
        }
    }
    Ok(())
}

async fn emit_snapshot(
    mainnet: &NoxaRpcClient,
    testnet: &NoxaRpcClient,
    docs: &Client,
    args: &Cli,
) -> Result<()> {
    let snapshot = collect_snapshot(mainnet, testnet, docs, args).await?;
    println!("{}", serde_json::to_string(&snapshot)?);
    Ok(())
}

async fn collect_snapshot(
    mainnet: &NoxaRpcClient,
    testnet: &NoxaRpcClient,
    docs: &Client,
    args: &Cli,
) -> Result<ReadinessSnapshot> {
    let status = mainnet.factory_status().await?;
    let (testnet_chain_id, weth_code, router_code, factory_code) = tokio::try_join!(
        testnet.chain_id(),
        testnet.code_at(WETH),
        testnet.code_at(UNISWAP_V3_SWAP_ROUTER_02),
        testnet.code_at(NOXA_LAUNCH_FACTORY),
    )?;
    let (noxa_fun_docs, noxa_dex_docs, robinhood_contracts_docs) = tokio::join!(
        fetch_docs(docs, &args.noxa_fun_docs_url),
        fetch_docs(docs, &args.noxa_dex_docs_url),
        fetch_docs(docs, &args.robinhood_contracts_docs_url),
    );
    if testnet_chain_id != TESTNET_CHAIN_ID {
        bail!("testnet RPC chain ID {testnet_chain_id} does not match {TESTNET_CHAIN_ID}");
    }

    let docs_complete = noxa_fun_docs.error.is_none()
        && noxa_dex_docs.error.is_none()
        && robinhood_contracts_docs.error.is_none();
    let docs_detected = noxa_fun_docs.testnet_contract_marker == Some(true)
        || noxa_dex_docs.testnet_contract_marker == Some(true)
        || robinhood_contracts_docs.testnet_contract_marker == Some(true);
    let bytecode_detected =
        !weth_code.is_empty() || !router_code.is_empty() || !factory_code.is_empty();
    let runtime_matches = status.runtime_keccak256 == NOXA_FACTORY_RUNTIME_KECCAK256;

    Ok(ReadinessSnapshot {
        record_type: "hermes_readiness_snapshot",
        checked_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before Unix epoch")?
            .as_secs(),
        mainnet_l1_block: status.pinned_l1_block,
        mainnet_l2_block: status.pinned_l2_block,
        launch_enabled: status.launch_enabled,
        factory_runtime_hash_matches_pin: runtime_matches,
        testnet_chain_id,
        testnet_weth_candidate: code_probe(WETH, &weth_code),
        testnet_router_candidate: code_probe(UNISWAP_V3_SWAP_ROUTER_02, &router_code),
        testnet_noxa_factory_candidate: code_probe(NOXA_LAUNCH_FACTORY, &factory_code),
        noxa_fun_docs,
        noxa_dex_docs,
        robinhood_contracts_docs,
        official_documentation_checks_complete: docs_complete,
        official_testnet_contract_docs_detected: docs_detected,
        candidate_testnet_bytecode_detected: bytecode_detected,
        ready_for_manual_testnet_address_validation: docs_detected || bytecode_detected,
        ready_for_mainnet_canary_review: status.launch_enabled && runtime_matches,
        private_key_used: false,
        broadcast: false,
    })
}

fn code_probe(address: Address, code: &[u8]) -> CodeProbe {
    CodeProbe {
        address,
        code_bytes: code.len(),
        code_present: !code.is_empty(),
    }
}

async fn fetch_docs(client: &Client, url: &str) -> DocsProbe {
    match fetch_docs_body(client, url).await {
        Ok(body) => DocsProbe {
            url: url.to_owned(),
            content_bytes: Some(body.len()),
            content_keccak256: Some(keccak256(&body).to_string()),
            testnet_contract_marker: Some(has_testnet_contract_marker(&body)),
            error: None,
        },
        Err(error) => DocsProbe {
            url: url.to_owned(),
            content_bytes: None,
            content_keccak256: None,
            testnet_contract_marker: None,
            error: Some(error.to_string()),
        },
    }
}

async fn fetch_docs_body(client: &Client, url: &str) -> Result<Vec<u8>> {
    Ok(client
        .get(url)
        .send()
        .await
        .with_context(|| format!("fetch official contract page {url}"))?
        .error_for_status()
        .with_context(|| format!("official contract page returned an error {url}"))?
        .bytes()
        .await
        .with_context(|| format!("read official contract page {url}"))?
        .to_vec())
}

fn has_testnet_contract_marker(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(body).to_ascii_lowercase();
    text.contains("46630")
        || text.contains("robinhood chain testnet")
        || text.contains("robinhood testnet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_chain_id_and_named_testnet_contract_markers() {
        assert!(has_testnet_contract_marker(
            b"Robinhood Testnet (Chain ID: 46630) SwapRouter02"
        ));
        assert!(has_testnet_contract_marker(
            b"Robinhood Chain Testnet contracts"
        ));
    }

    #[test]
    fn mainnet_contract_sections_do_not_trigger_testnet_readiness() {
        assert!(!has_testnet_contract_marker(
            b"Robinhood (Chain ID: 4663) WETH SwapRouter02"
        ));
    }
}
