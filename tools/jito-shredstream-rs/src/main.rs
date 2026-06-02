use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use jito_shredstream::{shredstream_proxy_client::ShredstreamProxyClient, SubscribeEntriesRequest};
use serde::Serialize;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;
use std::{
    collections::HashSet,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

mod shared {
    #![allow(dead_code)]
    tonic::include_proto!("shared");
}

mod jito_shredstream {
    #![allow(dead_code)]
    tonic::include_proto!("shredstream");
}

const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMP_AMM_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const PUMP_FUN_BUY_DISCRIMINATOR: [u8; 8] = [102, 6, 61, 18, 1, 218, 235, 234];
const PUMP_FUN_SELL_DISCRIMINATOR: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];
const PUMP_FUN_TOKEN_DECIMALS: f64 = 1_000_000.0;
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

#[derive(Debug, Parser)]
#[command(
    name = "jito-feed-probe",
    about = "Watch Jito ShredStream entries for tracked-wallet Pump buys"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Connect to the local Jito ShredStream proxy and scan deshredded entries.
    Live(LiveOptions),
}

#[derive(Debug, Parser)]
struct LiveOptions {
    #[arg(
        long,
        env = "JITO_SHREDSTREAM_PROXY_URL",
        default_value = "http://127.0.0.1:9999"
    )]
    endpoint: String,

    #[arg(
        long = "target-wallet",
        env = "SHREDSTREAM_TARGET_WALLETS",
        value_delimiter = ','
    )]
    target_wallets: Vec<String>,

    #[arg(long, default_value_t = 0)]
    limit: usize,

    #[arg(long, default_value_t = false)]
    include_rejections: bool,

    #[arg(long, default_value_t = false)]
    stats: bool,

    #[arg(long, default_value_t = false)]
    print_mentions: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Route {
    Pump,
    PumpAmm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Action {
    Buy,
    Sell,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NormalizedCopyTradeEvent {
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
    route: Route,
    sol_amount: f64,
    token_amount: f64,
    input: NormalizedAsset,
    output: NormalizedAsset,
    copyable: bool,
    filters: Vec<String>,
    account_key_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RejectionLine {
    schema: &'static str,
    observed_at_ms: u128,
    provider: &'static str,
    source: &'static str,
    endpoint: String,
    signature: String,
    slot: u64,
    reason: String,
    filters: Vec<String>,
    account_key_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WalletMentionLine {
    schema: &'static str,
    observed_at_ms: u128,
    provider: &'static str,
    source: &'static str,
    endpoint: String,
    target_wallet: String,
    signature: String,
    slot: u64,
    account_key_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NormalizedAsset {
    mint: String,
    amount: f64,
}

#[derive(Debug)]
struct ParsedTrade {
    target_wallet: String,
    action: Action,
    mint: String,
    route: Route,
    sol_amount: f64,
    token_amount: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Live(options) => run_live(options).await,
    }
}

async fn run_live(options: LiveOptions) -> Result<()> {
    let target_wallets = parse_target_wallets(&options.target_wallets)?;
    let mut client = ShredstreamProxyClient::connect(options.endpoint.clone())
        .await
        .with_context(|| format!("connect to {}", options.endpoint))?;
    let mut stream = client
        .subscribe_entries(SubscribeEntriesRequest {})
        .await
        .context("subscribe to Jito ShredStream entries")?
        .into_inner();

    eprintln!(
        "subscribed to Jito ShredStream proxy {}; wallets={}; limit={}",
        options.endpoint,
        target_wallets.len(),
        options.limit
    );

    let mut seen = HashSet::new();
    let mut emitted = 0usize;

    while let Some(slot_entry) = stream
        .message()
        .await
        .context("receive Jito ShredStream entry")?
    {
        let entries =
            match bincode::deserialize::<Vec<solana_entry::entry::Entry>>(&slot_entry.entries) {
                Ok(entries) => entries,
                Err(error) => {
                    if options.include_rejections {
                        print_json(&RejectionLine {
                            schema: "copytrade.feed.rejection.v1",
                            observed_at_ms: now_ms(),
                            provider: "shredstream",
                            source: "jito-proxy",
                            endpoint: options.endpoint.clone(),
                            signature: "unknown-entry".to_string(),
                            slot: slot_entry.slot,
                            reason: format!("entry deserialize failed: {error}"),
                            filters: vec!["jito-entry".to_string()],
                            account_key_count: 0,
                        })?;
                    }
                    continue;
                }
            };

        if options.stats {
            eprintln!(
                "slot {} entries={} transactions={}",
                slot_entry.slot,
                entries.len(),
                entries
                    .iter()
                    .map(|entry| entry.transactions.len())
                    .sum::<usize>()
            );
        }

        for entry in entries {
            for versioned_tx in entry.transactions {
                let signature = versioned_tx_signature_string(&versioned_tx);
                if signature.is_empty() || !seen.insert(signature.clone()) {
                    continue;
                }

                let account_keys = static_account_keys(&versioned_tx);
                match parse_trade(&versioned_tx, &account_keys, &target_wallets) {
                    Some(parsed) => {
                        print_json(&normalized_event(
                            now_ms(),
                            options.endpoint.clone(),
                            signature,
                            slot_entry.slot,
                            account_keys.len(),
                            parsed,
                        ))?;
                        emitted += 1;
                        if options.limit > 0 && emitted >= options.limit {
                            return Ok(());
                        }
                    }
                    None if options.include_rejections => {
                        print_json(&RejectionLine {
                            schema: "copytrade.feed.rejection.v1",
                            observed_at_ms: now_ms(),
                            provider: "shredstream",
                            source: "jito-proxy",
                            endpoint: options.endpoint.clone(),
                            signature,
                            slot: slot_entry.slot,
                            reason: "no supported target Pump instruction in static account keys"
                                .to_string(),
                            filters: vec!["jito-entry".to_string()],
                            account_key_count: account_keys.len(),
                        })?;
                    }
                    None if options.print_mentions => {
                        if let Some(target_wallet) =
                            mentioned_target_wallet(&account_keys, &target_wallets)
                        {
                            print_json(&WalletMentionLine {
                                schema: "copytrade.feed.walletMention.v1",
                                observed_at_ms: now_ms(),
                                provider: "shredstream",
                                source: "jito-proxy",
                                endpoint: options.endpoint.clone(),
                                target_wallet,
                                signature,
                                slot: slot_entry.slot,
                                account_key_count: account_keys.len(),
                            })?;
                        }
                    }
                    None => {}
                }
            }
        }
    }

    Ok(())
}

fn normalized_event(
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

fn parse_target_wallets(values: &[String]) -> Result<Vec<String>> {
    let mut wallets = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        Pubkey::from_str(trimmed).with_context(|| format!("invalid target wallet {trimmed}"))?;
        wallets.push(trimmed.to_string());
    }

    if wallets.is_empty() {
        anyhow::bail!("provide at least one --target-wallet");
    }

    wallets.sort();
    wallets.dedup();
    Ok(wallets)
}

fn static_account_keys(versioned_tx: &VersionedTransaction) -> Vec<String> {
    versioned_tx
        .message
        .static_account_keys()
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn versioned_tx_signature_string(versioned_tx: &VersionedTransaction) -> String {
    versioned_tx
        .signatures
        .first()
        .map(ToString::to_string)
        .unwrap_or_default()
}

fn parse_trade(
    versioned_tx: &VersionedTransaction,
    account_keys: &[String],
    target_wallets: &[String],
) -> Option<ParsedTrade> {
    let target_wallets = target_wallets.iter().collect::<HashSet<_>>();

    for instruction in versioned_tx.message.instructions() {
        let program_id = account_keys.get(instruction.program_id_index as usize)?;
        let route = match program_id.as_str() {
            PUMP_FUN_PROGRAM_ID => Route::Pump,
            PUMP_AMM_PROGRAM_ID => Route::PumpAmm,
            _ => continue,
        };
        let action = parse_action(&instruction.data)?;
        let accounts = instruction
            .accounts
            .iter()
            .map(|index| *index as usize)
            .collect::<Vec<_>>();
        let (user_index, mint_index, quote_index) = match route {
            Route::Pump => (6, 2, None),
            Route::PumpAmm => (1, 3, Some(4)),
        };
        let user = account_keys.get(*accounts.get(user_index)?)?;
        if !target_wallets.contains(user) {
            continue;
        }
        let mint = account_keys.get(*accounts.get(mint_index)?)?;
        if let Some(index) = quote_index {
            let quote_mint = account_keys.get(*accounts.get(index)?)?;
            if quote_mint != SOL_MINT {
                continue;
            }
        }
        let token_amount = read_u64_le(&instruction.data, 8)? as f64 / PUMP_FUN_TOKEN_DECIMALS;
        let sol_amount = read_u64_le(&instruction.data, 16)? as f64 / LAMPORTS_PER_SOL;
        return Some(ParsedTrade {
            target_wallet: user.to_string(),
            action,
            mint: mint.to_string(),
            route,
            sol_amount,
            token_amount,
        });
    }

    None
}

fn mentioned_target_wallet(account_keys: &[String], target_wallets: &[String]) -> Option<String> {
    let account_keys = account_keys.iter().collect::<HashSet<_>>();
    target_wallets
        .iter()
        .find(|wallet| account_keys.contains(wallet))
        .cloned()
}

fn parse_action(data: &[u8]) -> Option<Action> {
    if data.len() < 8 {
        return None;
    }
    let discriminator = &data[..8];
    if discriminator == PUMP_FUN_BUY_DISCRIMINATOR {
        Some(Action::Buy)
    } else if discriminator == PUMP_FUN_SELL_DISCRIMINATOR {
        Some(Action::Sell)
    } else {
        None
    }
}

fn read_u64_le(data: &[u8], offset: usize) -> Option<u64> {
    let bytes = data.get(offset..offset + 8)?;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string(value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_hash::Hash;
    use solana_message::{
        compiled_instruction::CompiledInstruction, legacy::Message, MessageHeader, VersionedMessage,
    };

    #[test]
    fn parses_pump_buy_into_copyable_normalized_event() {
        let target_wallet = "CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o";
        let mint = "9BB6NFEcjBCtnNLFko2FqVQBq8HHM13kCyYcdQbgpump";
        let transaction = fixture_transaction(Route::Pump, Action::Buy, target_wallet, mint);
        let account_keys = static_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[target_wallet.to_string()])
            .expect("trade should parse");

        let event = normalized_event(
            123,
            "replay".to_string(),
            "sig".to_string(),
            456,
            account_keys.len(),
            parsed,
        );
        let value = serde_json::to_value(event).expect("event serializes");

        assert_eq!(value["schema"], "copytrade.feed.event.v1");
        assert_eq!(value["provider"], "shredstream");
        assert_eq!(value["source"], "jito-proxy");
        assert_eq!(value["action"], "buy");
        assert_eq!(value["route"], "pump");
        assert_eq!(value["copyable"], true);
        assert_eq!(value["input"]["mint"], SOL_MINT);
        assert_eq!(value["output"]["mint"], mint);
    }

    #[test]
    fn parses_pump_amm_sell_as_not_copyable() {
        let target_wallet = "CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o";
        let mint = "9BB6NFEcjBCtnNLFko2FqVQBq8HHM13kCyYcdQbgpump";
        let transaction = fixture_transaction(Route::PumpAmm, Action::Sell, target_wallet, mint);
        let account_keys = static_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[target_wallet.to_string()])
            .expect("trade should parse");

        let event = normalized_event(
            123,
            "replay".to_string(),
            "sig".to_string(),
            456,
            account_keys.len(),
            parsed,
        );
        let value = serde_json::to_value(event).expect("event serializes");

        assert_eq!(value["action"], "sell");
        assert_eq!(value["copyable"], false);
        assert_eq!(value["input"]["mint"], mint);
        assert_eq!(value["output"]["mint"], SOL_MINT);
    }

    fn fixture_transaction(
        route: Route,
        action: Action,
        target_wallet: &str,
        mint: &str,
    ) -> VersionedTransaction {
        let account_keys = vec![
            pubkey("11111111111111111111111111111111"),
            pubkey("SysvarRent111111111111111111111111111111111"),
            pubkey(mint),
            pubkey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
            pubkey(SOL_MINT),
            pubkey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"),
            pubkey(target_wallet),
            pubkey(match route {
                Route::Pump => PUMP_FUN_PROGRAM_ID,
                Route::PumpAmm => PUMP_AMM_PROGRAM_ID,
            }),
        ];
        let program_id_index = (account_keys.len() - 1) as u8;
        let accounts = match route {
            Route::Pump => vec![0, 1, 2, 3, 4, 5, 6],
            Route::PumpAmm => vec![0, 6, 1, 2, 4],
        };

        VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::Legacy(Message {
                header: MessageHeader {
                    num_required_signatures: 0,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                account_keys,
                recent_blockhash: Hash::default(),
                instructions: vec![CompiledInstruction {
                    program_id_index,
                    accounts,
                    data: instruction_data(action),
                }],
            }),
        }
    }

    fn instruction_data(action: Action) -> Vec<u8> {
        let discriminator = match action {
            Action::Buy => PUMP_FUN_BUY_DISCRIMINATOR,
            Action::Sell => PUMP_FUN_SELL_DISCRIMINATOR,
        };
        let mut data = Vec::new();
        data.extend_from_slice(&discriminator);
        data.extend_from_slice(&1_000_000u64.to_le_bytes());
        data.extend_from_slice(&200_000_000u64.to_le_bytes());
        data
    }

    fn pubkey(value: &str) -> Pubkey {
        Pubkey::from_str(value).expect("fixture pubkey is valid")
    }
}
