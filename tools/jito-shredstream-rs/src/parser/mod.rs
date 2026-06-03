use serde::Serialize;
use solana_message::{compiled_instruction::CompiledInstruction, VersionedMessage};
use solana_transaction::versioned::VersionedTransaction;
use std::collections::HashSet;

pub(crate) mod routes;

pub(crate) const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub(crate) const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
pub(crate) const COMPUTE_BUDGET_PROGRAM_ID: &str = "ComputeBudget111111111111111111111111111111";
pub(crate) const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub(crate) const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
pub(crate) const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub(crate) const PUMP_FUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub(crate) const PUMP_AMM_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
pub(crate) const FLASHX_ROUTER_PROGRAM_ID: &str = "FLASHX8DrLbgeR8FcfNV1F5krxYcYMUdBkrP1EPBtxB9";
pub(crate) const PUMP_FUN_BUY_DISCRIMINATOR: [u8; 8] = [102, 6, 61, 18, 1, 218, 235, 234];
pub(crate) const PUMP_FUN_SELL_DISCRIMINATOR: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];
pub(crate) const PUMP_FUN_TOKEN_DECIMALS: f64 = 1_000_000.0;
pub(crate) const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Route {
    Pump,
    PumpAmm,
    FlashxPump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Action {
    Buy,
    Sell,
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedTrade {
    pub(crate) target_wallet: String,
    pub(crate) action: Action,
    pub(crate) mint: String,
    pub(crate) route: Route,
    pub(crate) sol_amount: Option<f64>,
    pub(crate) token_amount: Option<f64>,
    pub(crate) route_context: Option<RouteContext>,
}

#[derive(Clone, Debug)]
pub(crate) enum RouteContext {
    FlashxPump(FlashxPumpRouteContext),
}

#[derive(Clone, Debug)]
pub(crate) struct FlashxPumpRouteContext {
    pub(crate) layout: FlashxPumpLayout,
    pub(crate) program_id: String,
    pub(crate) accounts: Vec<RouteInstructionAccount>,
    pub(crate) data: Vec<u8>,
    pub(crate) resolved_accounts: Vec<ResolvedRouteAccount>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlashxPumpLayout {
    MigratedAmm,
    DirectPump,
}

impl FlashxPumpLayout {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::MigratedAmm => "migrated-amm",
            Self::DirectPump => "direct-pump",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RouteInstructionAccount {
    pub(crate) pubkey: String,
    pub(crate) is_signer: bool,
    pub(crate) is_writable: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedRouteAccount {
    pub(crate) role: &'static str,
    pub(crate) pubkey: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WalletMentionKind {
    NonTrade,
    UnsupportedRoute,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WalletMentionClassification {
    pub(crate) kind: WalletMentionKind,
    pub(crate) reason: String,
}

pub(crate) fn parse_trade(
    versioned_tx: &VersionedTransaction,
    account_keys: &[String],
    target_wallets: &[String],
) -> Option<ParsedTrade> {
    let target_wallets = target_wallets.iter().collect::<HashSet<_>>();

    for instruction in versioned_tx.message.instructions() {
        let program_id = account_keys.get(instruction.program_id_index as usize)?;
        let parsed = match program_id.as_str() {
            PUMP_FUN_PROGRAM_ID => routes::pump::parse(instruction, account_keys, &target_wallets),
            PUMP_AMM_PROGRAM_ID => {
                routes::pump_amm::parse(instruction, account_keys, &target_wallets)
            }
            FLASHX_ROUTER_PROGRAM_ID => {
                routes::flashx::parse(instruction, account_keys, &target_wallets)
            }
            _ => None,
        };
        if let Some(mut parsed) = parsed {
            parsed.route_context =
                route_context(&versioned_tx.message, instruction, account_keys, &parsed);
            return Some(parsed);
        }
    }

    None
}

fn route_context(
    message: &VersionedMessage,
    instruction: &CompiledInstruction,
    account_keys: &[String],
    parsed: &ParsedTrade,
) -> Option<RouteContext> {
    match parsed.route {
        Route::FlashxPump => {
            routes::flashx::route_context(message, instruction, account_keys, parsed)
        }
        Route::Pump | Route::PumpAmm => None,
    }
}

pub(crate) fn mentioned_target_wallet(
    account_keys: &[String],
    target_wallets: &[String],
) -> Option<String> {
    let account_keys = account_keys.iter().collect::<HashSet<_>>();
    target_wallets
        .iter()
        .find(|wallet| account_keys.contains(wallet))
        .cloned()
}

pub(crate) fn classify_wallet_mention(
    versioned_tx: &VersionedTransaction,
    account_keys: &[String],
) -> WalletMentionClassification {
    let mut programs = Vec::new();
    for instruction in versioned_tx.message.instructions() {
        if let Some(program_id) = account_keys.get(instruction.program_id_index as usize) {
            programs.push(program_id.as_str());
        }
    }

    if programs.is_empty() {
        return WalletMentionClassification {
            kind: WalletMentionKind::Unknown,
            reason: "no outer instructions".to_string(),
        };
    }

    if programs
        .iter()
        .all(|program_id| is_non_trade_program(program_id))
    {
        return WalletMentionClassification {
            kind: WalletMentionKind::NonTrade,
            reason: "only system/compute/token housekeeping programs".to_string(),
        };
    }

    if programs
        .iter()
        .any(|program_id| is_supported_trade_program(program_id))
    {
        return WalletMentionClassification {
            kind: WalletMentionKind::UnsupportedRoute,
            reason: "supported trade program mentioned but account/data layout did not parse"
                .to_string(),
        };
    }

    WalletMentionClassification {
        kind: WalletMentionKind::Unknown,
        reason: "target wallet mentioned by unsupported non-housekeeping program".to_string(),
    }
}

#[cfg(test)]
pub(crate) fn static_account_keys(versioned_tx: &VersionedTransaction) -> Vec<String> {
    versioned_tx
        .message
        .static_account_keys()
        .iter()
        .map(ToString::to_string)
        .collect()
}

pub(crate) fn versioned_tx_signature_string(versioned_tx: &VersionedTransaction) -> String {
    versioned_tx
        .signatures
        .first()
        .map(ToString::to_string)
        .unwrap_or_default()
}

pub(crate) fn parse_action(data: &[u8]) -> Option<Action> {
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

pub(crate) fn read_u64_le(data: &[u8], offset: usize) -> Option<u64> {
    let bytes = data.get(offset..offset + 8)?;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

fn is_non_trade_program(program_id: &str) -> bool {
    matches!(
        program_id,
        SYSTEM_PROGRAM_ID
            | COMPUTE_BUDGET_PROGRAM_ID
            | TOKEN_PROGRAM_ID
            | TOKEN_2022_PROGRAM_ID
            | ASSOCIATED_TOKEN_PROGRAM_ID
    )
}

fn is_supported_trade_program(program_id: &str) -> bool {
    matches!(
        program_id,
        PUMP_FUN_PROGRAM_ID | PUMP_AMM_PROGRAM_ID | FLASHX_ROUTER_PROGRAM_ID
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use solana_hash::Hash;
    use solana_message::{
        compiled_instruction::CompiledInstruction, legacy::Message, MessageHeader, VersionedMessage,
    };
    use solana_pubkey::Pubkey;
    use std::str::FromStr;

    #[test]
    fn unsupported_program_can_still_fall_back_to_wallet_mention() {
        let target_wallet = "CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o";
        let transaction = VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::Legacy(Message {
                header: MessageHeader {
                    num_required_signatures: 0,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                account_keys: vec![
                    pubkey(target_wallet),
                    pubkey("11111111111111111111111111111111"),
                ],
                recent_blockhash: Hash::default(),
                instructions: vec![CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![0],
                    data: vec![1, 2, 3],
                }],
            }),
        };
        let account_keys = static_account_keys(&transaction);

        assert!(parse_trade(&transaction, &account_keys, &[target_wallet.to_string()]).is_none());
        assert_eq!(
            mentioned_target_wallet(&account_keys, &[target_wallet.to_string()]),
            Some(target_wallet.to_string())
        );
    }

    #[test]
    fn classifies_system_only_wallet_mention_as_non_trade() {
        let target_wallet = "CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o";
        let transaction = VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::Legacy(Message {
                header: MessageHeader {
                    num_required_signatures: 0,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                account_keys: vec![
                    pubkey(target_wallet),
                    pubkey("SysvarRent111111111111111111111111111111111"),
                    pubkey(SYSTEM_PROGRAM_ID),
                ],
                recent_blockhash: Hash::default(),
                instructions: vec![CompiledInstruction {
                    program_id_index: 2,
                    accounts: vec![1, 0],
                    data: vec![1, 2, 3],
                }],
            }),
        };
        let account_keys = static_account_keys(&transaction);

        assert_eq!(
            classify_wallet_mention(&transaction, &account_keys).kind,
            WalletMentionKind::NonTrade
        );
    }

    #[test]
    fn classifies_supported_trade_program_parse_miss_as_unsupported_route() {
        let target_wallet = "CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o";
        let transaction = VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::Legacy(Message {
                header: MessageHeader {
                    num_required_signatures: 0,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                account_keys: vec![pubkey(target_wallet), pubkey(PUMP_FUN_PROGRAM_ID)],
                recent_blockhash: Hash::default(),
                instructions: vec![CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![0],
                    data: vec![1, 2, 3],
                }],
            }),
        };
        let account_keys = static_account_keys(&transaction);

        assert_eq!(
            classify_wallet_mention(&transaction, &account_keys).kind,
            WalletMentionKind::UnsupportedRoute
        );
    }

    #[test]
    fn replays_live_mention_misses_as_non_trade_system_fanout() {
        let cases = [
            (
                "SvXrppD5RmfngsWtVH72fVT45B4eUnVCdszjkjeic3BM3oRQrucgJxkxX5TSVw8iQvCG2HjzvpLRxcBjN1CfQJB",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/fixtures/nontrade/system-fanout-SvXrppD5RmfngsWtVH72fVT45B4eUnVCdszjkjeic3BM3oRQrucgJxkxX5TSVw8iQvCG2HjzvpLRxcBjN1CfQJB.tx.base64"
                )),
            ),
            (
                "2tdBpJVSa33CD2tFgva41Dv2Vivwb9tfKNc2yPDBHd9oeWfdHXv2ykSKsJfPDdq3UCu9wHUqm818iyUFzDhKyNMd",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/fixtures/nontrade/system-fanout-2tdBpJVSa33CD2tFgva41Dv2Vivwb9tfKNc2yPDBHd9oeWfdHXv2ykSKsJfPDdq3UCu9wHUqm818iyUFzDhKyNMd.tx.base64"
                )),
            ),
        ];

        let target_wallet = "A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS";
        for (signature, fixture) in cases {
            let transaction = replay_transaction(fixture);
            assert_eq!(versioned_tx_signature_string(&transaction), signature);

            let account_keys = static_account_keys(&transaction);
            assert!(
                parse_trade(&transaction, &account_keys, &[target_wallet.to_string()]).is_none()
            );
            assert_eq!(
                mentioned_target_wallet(&account_keys, &[target_wallet.to_string()]),
                Some(target_wallet.to_string())
            );
            assert_eq!(
                classify_wallet_mention(&transaction, &account_keys).kind,
                WalletMentionKind::NonTrade
            );
        }
    }

    fn replay_transaction(base64_fixture: &str) -> VersionedTransaction {
        let compact = base64_fixture.split_whitespace().collect::<String>();
        let bytes = STANDARD.decode(compact).expect("fixture is valid base64");
        bincode::deserialize(&bytes).expect("fixture decodes as a VersionedTransaction")
    }

    fn pubkey(value: &str) -> Pubkey {
        Pubkey::from_str(value).expect("fixture pubkey is valid")
    }
}
