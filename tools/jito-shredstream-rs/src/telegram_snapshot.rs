use crate::executor::{
    TrailingSellMode, TrailingSellPercentBasis, TrailingSellPlan, TrailingSellStep,
};
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
    pub(crate) trailing_sell: Option<TrailingSellPlan>,
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
    #[serde(rename = "liveTradingEnabled")]
    _live_trading_enabled: bool,
    emergency_stopped: bool,
    #[serde(default)]
    default_slippage: Option<f64>,
    #[serde(default)]
    default_priority_fee: Option<f64>,
    #[serde(default)]
    default_trailing_sell: Option<SnapshotTrailingSellConfig>,
    #[serde(default)]
    priority_fee_micro_lamports: Option<u64>,
    #[serde(default)]
    jito_tip_lamports: Option<u64>,
    #[serde(default)]
    jito_tip_account: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotSubscriber {
    trading_wallet_public_key: String,
    copy_amount_sol: f64,
    #[serde(default)]
    sell_slippage: Option<f64>,
    #[serde(default)]
    sell_priority_fee: Option<f64>,
    #[serde(default)]
    effective_sell_slippage: Option<f64>,
    #[serde(default)]
    effective_sell_priority_fee: Option<f64>,
    wallets: Vec<SnapshotWallet>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotWallet {
    address: String,
    #[serde(default)]
    trailing_sell: Option<SnapshotTrailingSellConfig>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotTrailingSellConfig {
    enabled: bool,
    mode: Option<String>,
    percent_basis: Option<String>,
    steps: Vec<SnapshotTrailingSellStep>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotTrailingSellStep {
    delay_ms: f64,
    percent: f64,
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
        if !snapshot.routing.emergency_stopped {
            for subscriber in snapshot.subscribers {
                if let Some(copy_wallet) = copy_wallet {
                    if subscriber.trading_wallet_public_key != copy_wallet {
                        continue;
                    }
                }
                if !subscriber.copy_amount_sol.is_finite() || subscriber.copy_amount_sol <= 0.0 {
                    continue;
                }

                let sell_slippage_percent = subscriber
                    .effective_sell_slippage
                    .or(subscriber.sell_slippage)
                    .or(snapshot.routing.default_slippage)
                    .filter(|value| value.is_finite() && *value >= 0.0);
                let sell_priority_fee_sol = subscriber
                    .effective_sell_priority_fee
                    .or(subscriber.sell_priority_fee)
                    .or(snapshot.routing.default_priority_fee)
                    .filter(|value| value.is_finite() && *value >= 0.0);

                for wallet in subscriber.wallets {
                    if Pubkey::from_str(&wallet.address).is_err() {
                        continue;
                    }
                    let trailing_sell = wallet
                        .trailing_sell
                        .as_ref()
                        .or(snapshot.routing.default_trailing_sell.as_ref())
                        .and_then(|config| {
                            trailing_sell_plan_from_snapshot(
                                config,
                                sell_slippage_percent,
                                sell_priority_fee_sol,
                                snapshot.routing.priority_fee_micro_lamports,
                                snapshot.routing.jito_tip_lamports,
                                snapshot.routing.jito_tip_account.clone(),
                            )
                        });
                    targets.insert(
                        wallet.address,
                        TelegramTargetConfig {
                            copy_amount_sol: subscriber.copy_amount_sol,
                            trailing_sell,
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

fn trailing_sell_plan_from_snapshot(
    config: &SnapshotTrailingSellConfig,
    sell_slippage_percent: Option<f64>,
    sell_priority_fee_sol: Option<f64>,
    priority_fee_micro_lamports: Option<u64>,
    jito_tip_lamports: Option<u64>,
    jito_tip_account: Option<String>,
) -> Option<TrailingSellPlan> {
    if !config.enabled {
        return None;
    }

    let steps = config
        .steps
        .iter()
        .filter_map(|step| {
            if !step.delay_ms.is_finite()
                || step.delay_ms < 0.0
                || !step.percent.is_finite()
                || step.percent <= 0.0
                || step.percent > 100.0
            {
                return None;
            }

            Some(TrailingSellStep {
                delay_ms: step.delay_ms.floor() as u64,
                percent: step.percent,
            })
        })
        .take(20)
        .collect::<Vec<_>>();
    if steps.is_empty() {
        return None;
    }

    Some(TrailingSellPlan {
        mode: match config.mode.as_deref() {
            Some("formula") => TrailingSellMode::Formula,
            _ => TrailingSellMode::CustomSteps,
        },
        percent_basis: match config.percent_basis.as_deref() {
            Some("original_position") => TrailingSellPercentBasis::OriginalPosition,
            _ => TrailingSellPercentBasis::RemainingBalance,
        },
        steps,
        sell_slippage_percent,
        sell_priority_fee_sol,
        priority_fee_micro_lamports: priority_fee_micro_lamports.filter(|value| *value > 0),
        jito_tip_lamports: jito_tip_lamports.filter(|value| *value > 0),
        jito_tip_account: jito_tip_account
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    })
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
                "liveTradingEnabled": false,
                "emergencyStopped": false,
                "defaultSlippage": 10,
                "defaultPriorityFee": 0.00005,
                "priorityFeeMicroLamports": 250000,
                "jitoTipLamports": 10000,
                "jitoTipAccount": "96gYZGLnUQYgE8MWWpYJw8yRjnvB51rAhbG1SogE3uSG"
              }},
              "subscribers": [
                {{
                  "chatId": "chat-1",
                  "tradingWalletPublicKey": "{COPY_WALLET}",
                  "copyAmountSol": 0.0007,
                  "effectiveSellSlippage": 8,
                  "effectiveSellPriorityFee": 0.00002,
                  "wallets": [
                    {{
                      "address": "{TARGET_B}",
                      "label": "B",
                      "trailingSell": {{
                        "enabled": true,
                        "mode": "custom_steps",
                        "percentBasis": "original_position",
                        "steps": [
                          {{ "delayMs": 500, "percent": 50 }},
                          {{ "delayMs": 500, "percent": 100 }}
                        ]
                      }}
                    }},
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
        assert!(snapshot
            .target_config(TARGET_A)
            .unwrap()
            .trailing_sell
            .is_none());
        let trailing_sell = snapshot
            .target_config(TARGET_B)
            .unwrap()
            .trailing_sell
            .as_ref()
            .expect("target B trailing sell config");
        assert_eq!(trailing_sell.mode, TrailingSellMode::CustomSteps);
        assert_eq!(
            trailing_sell.percent_basis,
            TrailingSellPercentBasis::OriginalPosition
        );
        assert_eq!(
            trailing_sell.steps,
            vec![
                TrailingSellStep {
                    delay_ms: 500,
                    percent: 50.0,
                },
                TrailingSellStep {
                    delay_ms: 500,
                    percent: 100.0,
                }
            ]
        );
        assert_eq!(trailing_sell.sell_slippage_percent, Some(8.0));
        assert_eq!(trailing_sell.sell_priority_fee_sol, Some(0.00002));
        assert_eq!(trailing_sell.priority_fee_micro_lamports, Some(250000));
        assert_eq!(trailing_sell.jito_tip_lamports, Some(10000));
        assert_eq!(
            trailing_sell.jito_tip_account.as_deref(),
            Some("96gYZGLnUQYgE8MWWpYJw8yRjnvB51rAhbG1SogE3uSG")
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
