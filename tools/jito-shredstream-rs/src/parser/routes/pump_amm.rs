use crate::parser::{
    parse_action, read_u64_le, ParsedTrade, Route, LAMPORTS_PER_SOL, PUMP_FUN_TOKEN_DECIMALS,
    SOL_MINT,
};
use solana_message::compiled_instruction::CompiledInstruction;
use solana_pubkey::Pubkey;
use std::collections::HashSet;

pub(crate) fn parse(
    instruction: &CompiledInstruction,
    account_keys: &[Pubkey],
    target_wallets: &HashSet<Pubkey>,
) -> Option<ParsedTrade> {
    let action = parse_action(&instruction.data)?;
    let user = account_key_at(&instruction.accounts, account_keys, 1)?;
    if !target_wallets.contains(user) {
        return None;
    }
    let mint = account_key_at(&instruction.accounts, account_keys, 3)?;
    let quote_mint = account_key_at(&instruction.accounts, account_keys, 4)?;
    if quote_mint.to_string() != SOL_MINT {
        return None;
    }
    let token_amount = read_u64_le(&instruction.data, 8)? as f64 / PUMP_FUN_TOKEN_DECIMALS;
    let sol_amount = read_u64_le(&instruction.data, 16)? as f64 / LAMPORTS_PER_SOL;

    Some(ParsedTrade {
        target_wallet: user.to_string(),
        action,
        mint: mint.to_string(),
        route: Route::PumpAmm,
        sol_amount: Some(sol_amount),
        token_amount: Some(token_amount),
        route_context: None,
    })
}

fn account_key_at<'a>(
    accounts: &[u8],
    account_keys: &'a [Pubkey],
    account_position: usize,
) -> Option<&'a Pubkey> {
    account_keys.get(*accounts.get(account_position)? as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        event::normalized_event,
        parser::{static_account_keys, Action, PUMP_AMM_PROGRAM_ID, PUMP_FUN_SELL_DISCRIMINATOR},
    };
    use solana_hash::Hash;
    use solana_message::{legacy::Message, MessageHeader, VersionedMessage};
    use solana_pubkey::Pubkey;
    use solana_transaction::versioned::VersionedTransaction;
    use std::str::FromStr;

    #[test]
    fn parses_pump_amm_sell_as_not_copyable() {
        let target_wallet = "CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o";
        let mint = "9BB6NFEcjBCtnNLFko2FqVQBq8HHM13kCyYcdQbgpump";
        let transaction = fixture_transaction(Action::Sell, target_wallet, mint);
        let account_keys = static_account_keys(&transaction);
        let parsed =
            crate::parser::parse_trade(&transaction, &account_keys, &[target_wallet.to_string()])
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
            pubkey(PUMP_AMM_PROGRAM_ID),
        ];

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
                    program_id_index: 7,
                    accounts: vec![0, 6, 1, 2, 4],
                    data: instruction_data(action),
                }],
            }),
        }
    }

    fn instruction_data(action: Action) -> Vec<u8> {
        let discriminator = match action {
            Action::Buy => crate::parser::PUMP_FUN_BUY_DISCRIMINATOR,
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
