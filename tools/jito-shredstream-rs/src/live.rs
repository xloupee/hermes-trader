use crate::{
    event::{
        normalized_event, now_ms, print_json, wallet_mention_schema, RejectionLine,
        WalletMentionLine,
    },
    parser::{
        classify_wallet_mention, mentioned_target_wallet, parse_trade, static_account_keys,
        versioned_tx_signature_string,
    },
    proto::jito_shredstream::{
        shredstream_proxy_client::ShredstreamProxyClient, SubscribeEntriesRequest,
    },
    LiveOptions,
};
use anyhow::{Context, Result};
use solana_pubkey::Pubkey;
use std::{
    collections::{HashSet, VecDeque},
    str::FromStr,
};

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

    let mut seen = SeenSignatures::new(options.dedupe_capacity);
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
                            let classification =
                                classify_wallet_mention(&versioned_tx, &account_keys);
                            print_json(&WalletMentionLine {
                                schema: wallet_mention_schema(classification.kind),
                                observed_at_ms: now_ms(),
                                provider: "shredstream",
                                source: "jito-proxy",
                                endpoint: options.endpoint.clone(),
                                target_wallet,
                                signature,
                                slot: slot_entry.slot,
                                reason: classification.reason,
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

struct SeenSignatures {
    capacity: usize,
    set: HashSet<String>,
    order: VecDeque<String>,
}

impl SeenSignatures {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            set: HashSet::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
        }
    }

    fn insert(&mut self, signature: String) -> bool {
        if self.capacity == 0 {
            return true;
        }

        if self.set.contains(&signature) {
            return false;
        }

        while self.order.len() >= self.capacity {
            if let Some(expired) = self.order.pop_front() {
                self.set.remove(&expired);
            } else {
                break;
            }
        }

        self.set.insert(signature.clone());
        self.order.push_back(signature);
        true
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.set.len()
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

#[cfg(test)]
mod tests {
    use super::SeenSignatures;

    #[test]
    fn seen_signatures_evicts_oldest_when_capacity_is_reached() {
        let mut seen = SeenSignatures::new(2);

        assert!(seen.insert("a".to_string()));
        assert!(seen.insert("b".to_string()));
        assert!(!seen.insert("a".to_string()));
        assert_eq!(seen.len(), 2);

        assert!(seen.insert("c".to_string()));
        assert_eq!(seen.len(), 2);
        assert!(seen.insert("a".to_string()));
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn seen_signatures_capacity_zero_disables_dedupe() {
        let mut seen = SeenSignatures::new(0);

        assert!(seen.insert("a".to_string()));
        assert!(seen.insert("a".to_string()));
        assert_eq!(seen.len(), 0);
    }
}
