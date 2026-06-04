use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_message::AddressLookupTableAccount;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;
use std::{collections::HashMap, str::FromStr};

const FLASHX_LOOKUP_TABLE: &str = "4vX5U9XsiY11infmC13d6VFPjvUqtuRw744r4o94dyow";

#[derive(Clone, Debug, Default)]
pub(crate) struct AddressLookupTableCache {
    tables: HashMap<String, Vec<String>>,
    table_accounts: Vec<AddressLookupTableAccount>,
}

impl AddressLookupTableCache {
    pub(crate) async fn load(rpc_url: Option<&str>, table_keys: &[String]) -> Result<Self> {
        if table_keys.is_empty() {
            return Ok(Self::default());
        }

        let rpc_url =
            rpc_url.context("SOLANA_RPC_URL is required to preload address lookup tables")?;
        let client = reqwest::Client::new();
        let mut tables = HashMap::with_capacity(table_keys.len());

        for table_key in table_keys {
            let addresses = match fetch_lookup_table_addresses(&client, rpc_url, table_key).await {
                Ok(addresses) => addresses,
                Err(error) if table_key == FLASHX_LOOKUP_TABLE => {
                    eprintln!(
                        "using cached FLASHX lookup table after RPC preload failed: {error:#}"
                    );
                    flashx_lookup_table_addresses()
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("load address lookup table {table_key}"));
                }
            };
            tables.insert(table_key.to_string(), addresses);
        }

        let table_accounts = build_table_accounts(&tables)
            .map_err(|error| anyhow!("precompute lookup tables: {error}"))?;

        Ok(Self {
            tables,
            table_accounts,
        })
    }

    pub(crate) fn expanded_account_keys(
        &self,
        versioned_tx: &VersionedTransaction,
    ) -> ExpandedAccountKeys {
        let mut keys = versioned_tx
            .message
            .static_account_keys()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        let Some(address_table_lookups) = versioned_tx.message.address_table_lookups() else {
            return ExpandedAccountKeys {
                keys,
                missing_lookup_table: None,
            };
        };

        let mut writable = Vec::new();
        let mut readonly = Vec::new();

        for lookup in address_table_lookups {
            let table_key = lookup.account_key.to_string();
            let Some(addresses) = self.tables.get(&table_key) else {
                return ExpandedAccountKeys {
                    keys,
                    missing_lookup_table: Some(table_key),
                };
            };

            for index in &lookup.writable_indexes {
                let Some(address) = addresses.get(*index as usize) else {
                    return ExpandedAccountKeys {
                        keys,
                        missing_lookup_table: Some(table_key),
                    };
                };
                writable.push(address.clone());
            }

            for index in &lookup.readonly_indexes {
                let Some(address) = addresses.get(*index as usize) else {
                    return ExpandedAccountKeys {
                        keys,
                        missing_lookup_table: Some(table_key),
                    };
                };
                readonly.push(address.clone());
            }
        }

        keys.extend(writable);
        keys.extend(readonly);
        ExpandedAccountKeys {
            keys,
            missing_lookup_table: None,
        }
    }

    pub(crate) fn table_accounts(&self) -> &[AddressLookupTableAccount] {
        &self.table_accounts
    }
}

#[derive(Debug)]
pub(crate) struct ExpandedAccountKeys {
    pub(crate) keys: Vec<String>,
    pub(crate) missing_lookup_table: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    result: Option<RpcResult>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct RpcResult {
    value: Option<RpcAccount>,
}

#[derive(Debug, Deserialize)]
struct RpcAccount {
    data: RpcAccountData,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RpcAccountData {
    Encoded((String, String)),
    Legacy(Vec<String>),
}

async fn fetch_lookup_table_addresses(
    client: &reqwest::Client,
    rpc_url: &str,
    table_key: &str,
) -> Result<Vec<String>> {
    let response = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getAccountInfo",
            "params": [
                table_key,
                {
                    "encoding": "base64",
                    "commitment": "confirmed"
                }
            ]
        }))
        .send()
        .await
        .context("send getAccountInfo request")?
        .error_for_status()
        .context("getAccountInfo HTTP status")?
        .json::<RpcResponse>()
        .await
        .context("decode getAccountInfo response")?;

    if let Some(error) = response.error {
        return Err(anyhow!("getAccountInfo RPC error: {}", error.message));
    }

    let data = response
        .result
        .and_then(|result| result.value)
        .map(|account| account.data)
        .ok_or_else(|| anyhow!("lookup table account not found"))?;

    let encoded = match data {
        RpcAccountData::Encoded((encoded, _encoding)) => encoded,
        RpcAccountData::Legacy(values) => values
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("lookup table account data is empty"))?,
    };
    let bytes = base64_decode(&encoded).context("decode lookup table account data")?;
    let table = AddressLookupTable::deserialize(&bytes)
        .map_err(|error| anyhow!("deserialize lookup table: {error:?}"))?;

    Ok(table.addresses.iter().map(ToString::to_string).collect())
}

fn base64_decode(value: &str) -> Result<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .context("base64 decode")
}

fn build_table_accounts(
    tables: &HashMap<String, Vec<String>>,
) -> Result<Vec<AddressLookupTableAccount>, String> {
    let mut table_keys = tables.keys().cloned().collect::<Vec<_>>();
    table_keys.sort();

    table_keys
        .into_iter()
        .map(|key| {
            let addresses = tables
                .get(&key)
                .ok_or_else(|| format!("missing lookup table {key}"))?
                .iter()
                .map(|address| {
                    Pubkey::from_str(address)
                        .map_err(|error| format!("invalid lookup table address: {error}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(AddressLookupTableAccount {
                key: Pubkey::from_str(&key)
                    .map_err(|error| format!("invalid lookup table key: {error}"))?,
                addresses,
            })
        })
        .collect()
}

fn flashx_lookup_table_addresses() -> Vec<String> {
    let mut addresses = vec!["11111111111111111111111111111111".to_string(); 187];
    for (index, address) in [
        (124, "86Vh4XGLW2b6nvWbRyDs4ScgMXbuvRCHT7WbUT3RFxKG"),
        (117, "DKyUs1xXMDy8Z11zNsLnUg3dy9HZf6hYZidB6WodcaGy"),
        (66, "7GFUN3bWzJMKMRZ34JLsvcqdssDbXnp589SiE33KVwcC"),
        (58, "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM"),
        (52, "AktftA98kSWAxn6kVSoqBXBELUArjKu2H9WmKB48ULFY"),
        (42, "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD"),
        (164, "ECDrSz47nXihe5kyK4oWEePPsPi9qz6u5d6Fa2sDj3uM"),
        (5, "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"),
        (11, "So11111111111111111111111111111111111111112"),
        (4, "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
        (32, "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"),
        (33, "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf"),
        (34, "Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y"),
        (35, "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"),
        (36, "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw"),
        (65, "7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ"),
        (40, "GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR"),
        (38, "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1"),
        (37, "C2aFPdENg4A2HQsmrd5rTw5TaYBX5Ku887cWjbFKtZpw"),
        (39, "5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx"),
        (41, "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ"),
        (44, "GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL"),
    ] {
        addresses[index] = address.to_string();
    }
    addresses
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    const MIGRATED_BUY_LOOKUP_TABLE: &str = "4vX5U9XsiY11infmC13d6VFPjvUqtuRw744r4o94dyow";

    #[test]
    fn expands_v0_transaction_with_cached_lookup_table() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/migrated-buy-Jo9sxcrorVCGkmafhNDQKByQBDBTSqM99tS9R1mYs6DjvFZHxZFuFhAvdSemCxFqauPcqS1t17ir3iDScu7cQF5.tx.base64"
        )));
        let tables = HashMap::from([(
            MIGRATED_BUY_LOOKUP_TABLE.to_string(),
            migrated_buy_lookup_table_addresses(),
        )]);
        let cache = AddressLookupTableCache {
            table_accounts: build_table_accounts(&tables).unwrap(),
            tables,
        };

        let expanded = cache.expanded_account_keys(&transaction);

        assert_eq!(expanded.missing_lookup_table.as_deref(), None);
        assert_eq!(expanded.keys.len(), 35);
        assert_eq!(
            expanded.keys.get(28).map(String::as_str),
            Some("ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw")
        );
    }

    #[test]
    fn reports_missing_lookup_table_without_network_fetching() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/migrated-buy-Jo9sxcrorVCGkmafhNDQKByQBDBTSqM99tS9R1mYs6DjvFZHxZFuFhAvdSemCxFqauPcqS1t17ir3iDScu7cQF5.tx.base64"
        )));
        let expanded = AddressLookupTableCache::default().expanded_account_keys(&transaction);

        assert_eq!(
            expanded.missing_lookup_table.as_deref(),
            Some(MIGRATED_BUY_LOOKUP_TABLE)
        );
    }

    #[test]
    fn expands_live_direct_pump_transaction_with_cached_lookup_table() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        let tables = HashMap::from([(
            MIGRATED_BUY_LOOKUP_TABLE.to_string(),
            migrated_buy_lookup_table_addresses(),
        )]);
        let cache = AddressLookupTableCache {
            table_accounts: build_table_accounts(&tables).unwrap(),
            tables,
        };

        let expanded = cache.expanded_account_keys(&transaction);

        assert_eq!(expanded.missing_lookup_table.as_deref(), None);
        assert_eq!(expanded.keys.len(), 26);
        assert_eq!(
            expanded.keys.get(21).map(String::as_str),
            Some("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
        );
    }

    fn replay_transaction(base64_fixture: &str) -> VersionedTransaction {
        let compact = base64_fixture.split_whitespace().collect::<String>();
        let bytes = STANDARD.decode(compact).expect("fixture is valid base64");
        bincode::deserialize(&bytes).expect("fixture decodes as a VersionedTransaction")
    }

    fn migrated_buy_lookup_table_addresses() -> Vec<String> {
        let mut addresses = vec!["11111111111111111111111111111111".to_string(); 187];
        for (index, address) in [
            (124, "86Vh4XGLW2b6nvWbRyDs4ScgMXbuvRCHT7WbUT3RFxKG"),
            (117, "DKyUs1xXMDy8Z11zNsLnUg3dy9HZf6hYZidB6WodcaGy"),
            (66, "7GFUN3bWzJMKMRZ34JLsvcqdssDbXnp589SiE33KVwcC"),
            (58, "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM"),
            (52, "AktftA98kSWAxn6kVSoqBXBELUArjKu2H9WmKB48ULFY"),
            (42, "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD"),
            (164, "ECDrSz47nXihe5kyK4oWEePPsPi9qz6u5d6Fa2sDj3uM"),
            (5, "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"),
            (11, "So11111111111111111111111111111111111111112"),
            (4, "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
            (32, "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"),
            (33, "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf"),
            (34, "Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y"),
            (35, "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"),
            (36, "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw"),
            (65, "7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ"),
            (40, "GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR"),
            (38, "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1"),
            (37, "C2aFPdENg4A2HQsmrd5rTw5TaYBX5Ku887cWjbFKtZpw"),
            (39, "5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx"),
            (41, "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ"),
            (44, "GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL"),
        ] {
            addresses[index] = address.to_string();
        }
        addresses
    }
}
