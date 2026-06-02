use serde::Serialize;
use solana_transaction::versioned::VersionedTransaction;
use std::collections::HashSet;

pub(crate) mod routes;

pub(crate) const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
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

#[derive(Debug)]
pub(crate) struct ParsedTrade {
    pub(crate) target_wallet: String,
    pub(crate) action: Action,
    pub(crate) mint: String,
    pub(crate) route: Route,
    pub(crate) sol_amount: Option<f64>,
    pub(crate) token_amount: Option<f64>,
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
        if parsed.is_some() {
            return parsed;
        }
    }

    None
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

#[cfg(test)]
mod tests {
    use super::*;
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

    fn pubkey(value: &str) -> Pubkey {
        Pubkey::from_str(value).expect("fixture pubkey is valid")
    }
}
