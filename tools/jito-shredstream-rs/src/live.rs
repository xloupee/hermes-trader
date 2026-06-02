use crate::{
    event::{normalized_event, now_ms, print_json, RejectionLine, WalletMentionLine},
    parser::{
        mentioned_target_wallet, parse_trade, static_account_keys, versioned_tx_signature_string,
    },
    proto::jito_shredstream::{
        shredstream_proxy_client::ShredstreamProxyClient, SubscribeEntriesRequest,
    },
    LiveOptions,
};
use anyhow::{Context, Result};
use solana_pubkey::Pubkey;
use std::{collections::HashSet, str::FromStr};

pub(crate) async fn run(options: LiveOptions) -> Result<()> {
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
