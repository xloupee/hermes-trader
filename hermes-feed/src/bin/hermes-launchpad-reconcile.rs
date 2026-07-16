use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alloy_primitives::{B256, keccak256};
use anyhow::{Context, Result, bail};
use clap::Parser;
use futures_util::{StreamExt, stream};
use hermes_feed::NoxaRpcClient;
use hermes_feed::flap_abi::{decode_flap_token_bought, decode_flap_token_created};
use hermes_feed::launchpad_adapter::LaunchpadId;
use hermes_feed::launchpad_adapters::{
    CLANKER_FACTORY, CLANKER_TOKEN_CREATED_TOPIC, DOPPLER_CREATE_EMITTER, DOPPLER_CREATE_TOPIC,
    KLIK_FACTORY, KLIK_TOKEN_CREATED_TOPIC,
};
use hermes_feed::noxa_abi::{ReceiptLog, decode_token_launched};
use hermes_feed::pons::{PONS_CURRENT_FACTORY, PONS_LEGACY_FACTORY, PONS_TOKEN_LAUNCHED_TOPIC};
use hermes_feed::robinhood::{
    ACTIVE_NOXA_LAUNCH_FACTORY, BOW_LAUNCH_FACTORY, CHAIN_ID, LAUNCHHOOD_V3_FACTORY,
    NOXA_LAUNCH_FACTORY, PUBLIC_RPC_URL,
};
use hermes_feed::tier2_curve::HOOD_FACTORY;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::{Instant, sleep};

const BOW_LAUNCHED_SIGNATURE: &str = "Launched(address,address,address,uint256,uint256)";
const LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE: &str = "TokenLaunched(address,address,address,address,uint256,uint256,uint256,uint256,uint256,uint256)";
const HOOD_TOKEN_CREATED_SIGNATURE: &str =
    "TokenCreated(address,address,string,string,string,uint256,uint256,uint256)";
const HOOD_TRADE_SIGNATURE: &str =
    "Trade(address,address,bool,uint256,uint256,uint256,uint256,uint256)";

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Read-only receipt/event reconciler for launchpad paper observations"
)]
struct Cli {
    /// JSONL emitted by hermes-launchpad-paper.
    #[arg(long)]
    input: PathBuf,
    #[arg(long, default_value = PUBLIC_RPC_URL)]
    rpc_url: String,
    #[arg(long, default_value_t = 30)]
    receipt_timeout_seconds: u64,
    #[arg(long, default_value_t = 250)]
    poll_interval_ms: u64,
    #[arg(long, default_value_t = 8)]
    concurrency: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
struct ObservedCandidate {
    tx_hash: B256,
    launchpad: LaunchpadId,
    observer_received_unix_ns: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct ReconciliationEvidence {
    tx_hash: B256,
    launchpad: LaunchpadId,
    receipt_status: bool,
    protocol_event_match: bool,
    observed_unix_ns: u64,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    if args.receipt_timeout_seconds == 0 || args.poll_interval_ms == 0 || args.concurrency == 0 {
        bail!("timeout, poll interval, and concurrency must be non-zero");
    }
    let candidates = read_candidates(&args.input)?;
    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    let chain_id = rpc.chain_id().await?;
    if chain_id != CHAIN_ID {
        bail!("RPC chain ID {chain_id} does not match Robinhood {CHAIN_ID}");
    }
    let timeout = Duration::from_secs(args.receipt_timeout_seconds);
    let poll_interval = Duration::from_millis(args.poll_interval_ms);
    let mut reconciled = stream::iter(candidates.into_values().map(|candidate| {
        let rpc = rpc.clone();
        async move { reconcile_candidate(&rpc, candidate, timeout, poll_interval).await }
    }))
    .buffer_unordered(args.concurrency);

    while let Some(evidence) = reconciled.next().await {
        println!("{}", serde_json::to_string(&evidence?)?);
    }
    Ok(())
}

fn read_candidates(path: &Path) -> Result<HashMap<(B256, LaunchpadId), ObservedCandidate>> {
    let input = BufReader::new(
        File::open(path).with_context(|| format!("open observer JSONL {}", path.display()))?,
    );
    let mut candidates = HashMap::new();
    for (index, line) in input.lines().enumerate() {
        let line = line.with_context(|| format!("read observer line {}", index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line)
            .with_context(|| format!("decode observer line {}", index + 1))?;
        if value.get("record_type").and_then(Value::as_str) != Some("launchpad_paper_frame") {
            continue;
        }
        let observations = value
            .pointer("/report/observations")
            .and_then(Value::as_array)
            .context("launchpad paper frame has no observations array")?;
        for observation in observations {
            let candidate = ObservedCandidate {
                tx_hash: serde_json::from_value(
                    observation
                        .get("tx_hash")
                        .cloned()
                        .context("observation has no tx_hash")?,
                )?,
                launchpad: serde_json::from_value(
                    observation
                        .get("launchpad")
                        .cloned()
                        .context("observation has no launchpad")?,
                )?,
                observer_received_unix_ns: observation
                    .get("observer_received_unix_ns")
                    .and_then(Value::as_u64)
                    .context("observation has no receive timestamp")?,
            };
            let key = (candidate.tx_hash, candidate.launchpad);
            if candidates.insert(key, candidate).is_some() {
                bail!("duplicate observer candidate {key:?}");
            }
        }
    }
    Ok(candidates)
}

async fn reconcile_candidate(
    rpc: &NoxaRpcClient,
    candidate: ObservedCandidate,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<ReconciliationEvidence> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(receipt) = rpc.receipt(candidate.tx_hash).await? {
            if receipt.transaction_hash != candidate.tx_hash {
                bail!(
                    "receipt transaction hash mismatch for {}",
                    candidate.tx_hash
                );
            }
            return Ok(ReconciliationEvidence {
                tx_hash: candidate.tx_hash,
                launchpad: candidate.launchpad,
                receipt_status: receipt.status,
                protocol_event_match: receipt.status
                    && protocol_event_match(candidate.launchpad, &receipt.logs),
                observed_unix_ns: unix_now_ns(),
            });
        }
        if Instant::now() >= deadline {
            return Ok(ReconciliationEvidence {
                tx_hash: candidate.tx_hash,
                launchpad: candidate.launchpad,
                receipt_status: false,
                protocol_event_match: false,
                observed_unix_ns: unix_now_ns(),
            });
        }
        sleep(poll_interval).await;
    }
}

fn protocol_event_match(launchpad: LaunchpadId, logs: &[ReceiptLog]) -> bool {
    logs.iter().any(|log| match launchpad {
        LaunchpadId::Noxa => {
            matches!(
                log.address,
                NOXA_LAUNCH_FACTORY | ACTIVE_NOXA_LAUNCH_FACTORY
            ) && decode_token_launched(log).is_some()
        }
        LaunchpadId::Bow => exact_topic(
            log,
            BOW_LAUNCH_FACTORY,
            keccak256(BOW_LAUNCHED_SIGNATURE.as_bytes()),
        ),
        LaunchpadId::LaunchHoodV3 => exact_topic(
            log,
            LAUNCHHOOD_V3_FACTORY,
            keccak256(LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE.as_bytes()),
        ),
        LaunchpadId::Clanker => exact_topic(log, CLANKER_FACTORY, CLANKER_TOKEN_CREATED_TOPIC),
        LaunchpadId::BankrDoppler => exact_topic(log, DOPPLER_CREATE_EMITTER, DOPPLER_CREATE_TOPIC),
        LaunchpadId::KlikFinance => exact_topic(log, KLIK_FACTORY, KLIK_TOKEN_CREATED_TOPIC),
        LaunchpadId::Pons => {
            matches!(log.address, PONS_CURRENT_FACTORY | PONS_LEGACY_FACTORY)
                && log.topics.first() == Some(&PONS_TOKEN_LAUNCHED_TOPIC)
        }
        LaunchpadId::Flap => {
            decode_flap_token_created(CHAIN_ID, log).is_some()
                || decode_flap_token_bought(CHAIN_ID, log).is_some()
        }
        LaunchpadId::HoodFun => {
            exact_topic(
                log,
                HOOD_FACTORY,
                keccak256(HOOD_TOKEN_CREATED_SIGNATURE.as_bytes()),
            ) || exact_topic(
                log,
                HOOD_FACTORY,
                keccak256(HOOD_TRADE_SIGNATURE.as_bytes()),
            )
        }
        LaunchpadId::TrenchToday | LaunchpadId::LeaveHood => false,
    })
}

fn exact_topic(log: &ReceiptLog, address: alloy_primitives::Address, topic: B256) -> bool {
    log.address == address && log.topics.first() == Some(&topic)
}

fn unix_now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{Address, Bytes};

    use super::*;

    fn log(address: alloy_primitives::Address, topic: B256) -> ReceiptLog {
        ReceiptLog {
            address,
            log_index: 0,
            topics: vec![topic],
            data: Bytes::new(),
        }
    }

    #[test]
    fn exact_protocol_topics_reconcile_and_cross_protocol_topics_do_not() {
        let clanker = log(CLANKER_FACTORY, CLANKER_TOKEN_CREATED_TOPIC);
        assert!(protocol_event_match(
            LaunchpadId::Clanker,
            std::slice::from_ref(&clanker)
        ));
        assert!(!protocol_event_match(LaunchpadId::BankrDoppler, &[clanker]));

        let bankr = log(DOPPLER_CREATE_EMITTER, DOPPLER_CREATE_TOPIC);
        assert!(protocol_event_match(
            LaunchpadId::BankrDoppler,
            std::slice::from_ref(&bankr)
        ));
        assert!(!protocol_event_match(LaunchpadId::Clanker, &[bankr]));

        let lookalike = log(Address::with_last_byte(0xee), CLANKER_TOKEN_CREATED_TOPIC);
        assert!(!protocol_event_match(LaunchpadId::Clanker, &[lookalike]));
    }

    #[test]
    fn verified_v3_and_hood_event_signatures_match_research_topics() {
        assert_eq!(
            keccak256(LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE.as_bytes()).as_slice()[..4],
            [0x23, 0x5e, 0x34, 0xa4]
        );
        assert_ne!(keccak256(BOW_LAUNCHED_SIGNATURE.as_bytes()), B256::ZERO);
        assert_ne!(
            keccak256(HOOD_TOKEN_CREATED_SIGNATURE.as_bytes()),
            keccak256(HOOD_TRADE_SIGNATURE.as_bytes())
        );
    }
}
