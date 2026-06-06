use anyhow::{Context, Result};
use serde::Deserialize;
use solana_pubkey::Pubkey;
use std::{collections::HashMap, fs::File, path::Path, str::FromStr};

#[derive(Clone, Debug)]
pub(crate) struct TelegramSnapshotConfig {
    sequence: u64,
    targets: HashMap<String, TelegramTargetConfig>,
}

#[derive(Clone, Debug)]
pub(crate) struct TelegramTargetConfig {
    pub(crate) copy_amount_sol: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotFile {
    version: u64,
    sequence: u64,
    routing: SnapshotRouting,
    subscribers: Vec<SnapshotSubscriber>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotRouting {
    live_trading_enabled: bool,
    emergency_stopped: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotSubscriber {
    trading_wallet_public_key: String,
    copy_amount_sol: f64,
    wallets: Vec<SnapshotWallet>,
}

#[derive(Debug, Deserialize)]
struct SnapshotWallet {
    address: String,
}

impl TelegramSnapshotConfig {
    pub(crate) fn load(
        path: Option<&Path>,
        copy_wallet: Option<&str>,
    ) -> Result<Option<TelegramSnapshotConfig>> {
        let Some(path) = path else {
            return Ok(None);
        };

        let file = File::open(path)
            .with_context(|| format!("open Telegram Jito snapshot {}", path.display()))?;
        let snapshot: SnapshotFile = serde_json::from_reader(file)
            .with_context(|| format!("parse Telegram Jito snapshot {}", path.display()))?;

        if snapshot.version != 1 {
            anyhow::bail!(
                "unsupported Telegram Jito snapshot version {}",
                snapshot.version
            );
        }

        let mut targets = HashMap::new();
        if snapshot.routing.live_trading_enabled && !snapshot.routing.emergency_stopped {
            for subscriber in snapshot.subscribers {
                if let Some(copy_wallet) = copy_wallet {
                    if subscriber.trading_wallet_public_key != copy_wallet {
                        continue;
                    }
                }
                if !subscriber.copy_amount_sol.is_finite() || subscriber.copy_amount_sol <= 0.0 {
                    continue;
                }

                for wallet in subscriber.wallets {
                    if Pubkey::from_str(&wallet.address).is_err() {
                        continue;
                    }
                    targets.insert(
                        wallet.address,
                        TelegramTargetConfig {
                            copy_amount_sol: subscriber.copy_amount_sol,
                        },
                    );
                }
            }
        }

        Ok(Some(TelegramSnapshotConfig {
            sequence: snapshot.sequence,
            targets,
        }))
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.sequence
    }

    pub(crate) fn target_wallets(&self) -> Vec<String> {
        let mut wallets = self.targets.keys().cloned().collect::<Vec<_>>();
        wallets.sort();
        wallets
    }

    pub(crate) fn target_config(&self, target_wallet: &str) -> Option<&TelegramTargetConfig> {
        self.targets.get(target_wallet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    const COPY_WALLET: &str = "FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W";
    const OTHER_WALLET: &str = "11111111111111111111111111111111";
    const TARGET_A: &str = "A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS";
    const TARGET_B: &str = "3yYDCAHnjJk41HCKN9gL5Hajnjc9PijfTQjPijesyMT6";
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn loads_active_targets_for_the_configured_copy_wallet() {
        let path = write_snapshot(&format!(
            r#"{{
              "version": 1,
              "sequence": 42,
              "generatedAtMs": 1,
              "checksum": "ignored",
              "routing": {{
                "liveTradingEnabled": true,
                "emergencyStopped": false
              }},
              "subscribers": [
                {{
                  "chatId": "chat-1",
                  "tradingWalletPublicKey": "{COPY_WALLET}",
                  "copyAmountSol": 0.0007,
                  "wallets": [
                    {{ "address": "{TARGET_B}", "label": "B" }},
                    {{ "address": "{TARGET_A}", "label": "A" }}
                  ]
                }},
                {{
                  "chatId": "chat-2",
                  "tradingWalletPublicKey": "{OTHER_WALLET}",
                  "copyAmountSol": 0.0009,
                  "wallets": [{{ "address": "{OTHER_WALLET}", "label": "other" }}]
                }}
              ]
            }}"#
        ));

        let snapshot = TelegramSnapshotConfig::load(Some(path.as_path()), Some(COPY_WALLET))
            .expect("snapshot loads")
            .expect("snapshot config");

        assert_eq!(snapshot.sequence(), 42);
        assert_eq!(
            snapshot.target_wallets(),
            vec![TARGET_B.to_string(), TARGET_A.to_string()]
        );
        assert_eq!(
            snapshot.target_config(TARGET_A).unwrap().copy_amount_sol,
            0.0007
        );
        assert!(snapshot.target_config(OTHER_WALLET).is_none());
    }

    #[test]
    fn emergency_stop_exports_no_active_targets() {
        let path = write_snapshot(&format!(
            r#"{{
              "version": 1,
              "sequence": 43,
              "routing": {{
                "liveTradingEnabled": true,
                "emergencyStopped": true
              }},
              "subscribers": [
                {{
                  "tradingWalletPublicKey": "{COPY_WALLET}",
                  "copyAmountSol": 0.0007,
                  "wallets": [{{ "address": "{TARGET_A}" }}]
                }}
              ]
            }}"#
        ));

        let snapshot = TelegramSnapshotConfig::load(Some(path.as_path()), Some(COPY_WALLET))
            .expect("snapshot loads")
            .expect("snapshot config");

        assert!(snapshot.target_wallets().is_empty());
    }

    fn write_snapshot(body: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "jito-telegram-snapshot-test-{}-{}-{}.json",
            std::process::id(),
            crate::event::now_ms(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = File::create(&path).expect("create temp snapshot");
        file.write_all(body.as_bytes())
            .expect("write temp snapshot");
        path
    }
}
