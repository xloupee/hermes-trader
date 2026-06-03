use crate::parser::{
    read_u64_le, Action, ParsedTrade, Route, LAMPORTS_PER_SOL, PUMP_FUN_TOKEN_DECIMALS,
};
use solana_message::compiled_instruction::CompiledInstruction;
use std::collections::HashSet;

pub(crate) fn parse(
    instruction: &CompiledInstruction,
    account_keys: &[String],
    target_wallets: &HashSet<&String>,
) -> Option<ParsedTrade> {
    let accounts = instruction
        .accounts
        .iter()
        .map(|index| *index as usize)
        .collect::<Vec<_>>();
    let first_account = account_keys.get(*accounts.first()?)?;
    let second_account = account_keys.get(*accounts.get(1)?)?;
    let amount_in = read_u64_le(&instruction.data, 1)?;
    let _minimum_out = read_u64_le(&instruction.data, 9)?;

    if let Some(parsed) = parse_long_v2_layout(&accounts, account_keys, target_wallets, amount_in) {
        return Some(parsed);
    }

    if let Some(parsed) = parse_migrated_amm_layout(
        &accounts,
        account_keys,
        target_wallets,
        amount_in,
        &instruction.data,
    ) {
        return Some(parsed);
    }

    let mint = account_keys.get(*accounts.get(10)?)?;

    if target_wallets.contains(first_account) {
        return Some(ParsedTrade {
            target_wallet: first_account.to_string(),
            action: Action::Buy,
            mint: mint.to_string(),
            route: Route::FlashxPump,
            sol_amount: Some(amount_in as f64 / LAMPORTS_PER_SOL),
            token_amount: None,
        });
    }

    if target_wallets.contains(second_account) {
        return Some(ParsedTrade {
            target_wallet: second_account.to_string(),
            action: Action::Sell,
            mint: mint.to_string(),
            route: Route::FlashxPump,
            sol_amount: None,
            token_amount: Some(amount_in as f64 / PUMP_FUN_TOKEN_DECIMALS),
        });
    }

    None
}

fn parse_migrated_amm_layout(
    accounts: &[usize],
    account_keys: &[String],
    target_wallets: &HashSet<&String>,
    amount_in: u64,
    data: &[u8],
) -> Option<ParsedTrade> {
    if accounts.len() < 40 {
        return None;
    }

    let target_wallet = account_key_at(accounts, account_keys, 1)?;
    if !target_wallets.contains(target_wallet) {
        return None;
    }

    let mint = account_key_at(accounts, account_keys, 12).filter(|mint| is_pump_mint(mint))?;
    match data.get(17).copied()? {
        0 => Some(ParsedTrade {
            target_wallet: target_wallet.to_string(),
            action: Action::Buy,
            mint: mint.to_string(),
            route: Route::FlashxPump,
            sol_amount: Some(amount_in as f64 / LAMPORTS_PER_SOL),
            token_amount: None,
        }),
        1 => Some(ParsedTrade {
            target_wallet: target_wallet.to_string(),
            action: Action::Sell,
            mint: mint.to_string(),
            route: Route::FlashxPump,
            sol_amount: None,
            token_amount: Some(amount_in as f64 / PUMP_FUN_TOKEN_DECIMALS),
        }),
        _ => None,
    }
}

fn parse_long_v2_layout(
    accounts: &[usize],
    account_keys: &[String],
    target_wallets: &HashSet<&String>,
    amount_in: u64,
) -> Option<ParsedTrade> {
    if accounts.len() < 50 {
        return None;
    }

    let target_wallet = account_key_at(accounts, account_keys, 1)?;
    if !target_wallets.contains(target_wallet) {
        return None;
    }

    if let Some(mint) = account_key_at(accounts, account_keys, 19).filter(|mint| is_pump_mint(mint))
    {
        return Some(ParsedTrade {
            target_wallet: target_wallet.to_string(),
            action: Action::Buy,
            mint: mint.to_string(),
            route: Route::FlashxPump,
            sol_amount: Some(amount_in as f64 / LAMPORTS_PER_SOL),
            token_amount: None,
        });
    }

    if let Some(mint) = account_key_at(accounts, account_keys, 9).filter(|mint| is_pump_mint(mint))
    {
        return Some(ParsedTrade {
            target_wallet: target_wallet.to_string(),
            action: Action::Sell,
            mint: mint.to_string(),
            route: Route::FlashxPump,
            sol_amount: None,
            token_amount: Some(amount_in as f64 / PUMP_FUN_TOKEN_DECIMALS),
        });
    }

    None
}

fn account_key_at<'a>(
    accounts: &[usize],
    account_keys: &'a [String],
    account_position: usize,
) -> Option<&'a String> {
    account_keys.get(*accounts.get(account_position)?)
}

fn is_pump_mint(account_key: &str) -> bool {
    account_key.ends_with("pump")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        event::normalized_event,
        parser::{
            static_account_keys, versioned_tx_signature_string, FLASHX_ROUTER_PROGRAM_ID, SOL_MINT,
        },
    };
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use solana_hash::Hash;
    use solana_message::{legacy::Message, MessageHeader, VersionedMessage};
    use solana_pubkey::Pubkey;
    use solana_transaction::versioned::VersionedTransaction;
    use std::str::FromStr;

    const TARGET_WALLET: &str = "A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS";
    const FLASHX_MINT: &str = "E3wDF3hJtojFit9RQ1aX3SDiJh5ygYuj2bBPyJfUpump";
    const FLASHX_V2_MINT: &str = "6QPqSGYksJgxJmMfPzsTc7jK32YEFqTiCRYGZLvHpump";
    const MIGRATED_MINT: &str = "wXfe7vz2t8an9Ca5dy72ChU54fRvtefDRmb4rzUpump";

    #[test]
    fn parses_flashx_pump_buy_from_router_outer_instruction() {
        let transaction = fixture_transaction(Action::Buy, TARGET_WALLET, FLASHX_MINT);
        let account_keys = static_account_keys(&transaction);
        let parsed =
            crate::parser::parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
                .expect("FLASHX buy should parse");

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
        assert_eq!(value["action"], "buy");
        assert_eq!(value["route"], "flashx-pump");
        assert_eq!(value["targetWallet"], TARGET_WALLET);
        assert_eq!(value["mint"], FLASHX_MINT);
        assert_eq!(value["copyable"], true);
        assert_eq!(value["input"]["mint"], SOL_MINT);
        assert_eq!(value["output"]["mint"], FLASHX_MINT);
        assert!((value["solAmount"].as_f64().unwrap() - 0.00099).abs() < f64::EPSILON);
        assert!(value.get("tokenAmount").is_none());
        assert!(value["output"].get("amount").is_none());
    }

    #[test]
    fn parses_flashx_pump_sell_from_router_outer_instruction() {
        let transaction = fixture_transaction(Action::Sell, TARGET_WALLET, FLASHX_MINT);
        let account_keys = static_account_keys(&transaction);
        let parsed =
            crate::parser::parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
                .expect("FLASHX sell should parse");

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
        assert_eq!(value["route"], "flashx-pump");
        assert_eq!(value["copyable"], false);
        assert_eq!(value["input"]["mint"], FLASHX_MINT);
        assert_eq!(value["output"]["mint"], SOL_MINT);
        assert!((value["tokenAmount"].as_f64().unwrap() - 104_905.207_774).abs() < f64::EPSILON);
        assert!(value.get("solAmount").is_none());
        assert!(value["output"].get("amount").is_none());
    }

    #[test]
    fn replays_real_flashx_router_transactions_from_raw_bytes() {
        let cases = [
            ReplayCase {
                signature: "2vA8ofU1vmTTnqLUwD8c8gj3ShPG4nTovgFvMaxMP5EMHtCKZ1bm4jVQmS9AjaSTseRApW5ZsbPRHk8rwnSkLzVf",
                fixture: include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/fixtures/flashx/buy-2vA8ofU1vmTTnqLUwD8c8gj3ShPG4nTovgFvMaxMP5EMHtCKZ1bm4jVQmS9AjaSTseRApW5ZsbPRHk8rwnSkLzVf.tx.base64"
                )),
                action: Action::Buy,
                mint: FLASHX_MINT,
                sol_amount: Some(0.00099),
                token_amount: None,
                copyable: true,
            },
            ReplayCase {
                signature: "iuJwKCiEDeaKZ4C3FMPZ8hSNh93fsdK1fXJCZYxRhjLxyFmGjA3v83ipgoqJECtMwZFVQtgKgW8S46JLy7xx8Wy",
                fixture: include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/fixtures/flashx/buy-iuJwKCiEDeaKZ4C3FMPZ8hSNh93fsdK1fXJCZYxRhjLxyFmGjA3v83ipgoqJECtMwZFVQtgKgW8S46JLy7xx8Wy.tx.base64"
                )),
                action: Action::Buy,
                mint: FLASHX_MINT,
                sol_amount: Some(0.00099),
                token_amount: None,
                copyable: true,
            },
            ReplayCase {
                signature: "4m7URrfKhTFWVFuQfyBDMaYDGbKMWLrEeLn3XmB4PPHmPSBeiu18uZyukBXSHd22oqLUXtuqvpHuCjFiu7VPzCF6",
                fixture: include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/fixtures/flashx/sell-4m7URrfKhTFWVFuQfyBDMaYDGbKMWLrEeLn3XmB4PPHmPSBeiu18uZyukBXSHd22oqLUXtuqvpHuCjFiu7VPzCF6.tx.base64"
                )),
                action: Action::Sell,
                mint: FLASHX_MINT,
                sol_amount: None,
                token_amount: Some(104_905.207_774),
                copyable: false,
            },
            ReplayCase {
                signature: "2w97Y3Ddyk1FsmftLRpFi5VngMmUZzVuRz1A9B6KAwcMkL8L2qnhvzJ9fP3WYYzdTmupjF9PCcBTCma9jYdVkBSx",
                fixture: include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/fixtures/flashx/buyv2-2w97Y3Ddyk1FsmftLRpFi5VngMmUZzVuRz1A9B6KAwcMkL8L2qnhvzJ9fP3WYYzdTmupjF9PCcBTCma9jYdVkBSx.tx.base64"
                )),
                action: Action::Buy,
                mint: FLASHX_V2_MINT,
                sol_amount: Some(0.00099),
                token_amount: None,
                copyable: true,
            },
            ReplayCase {
                signature: "2ww3fpS3SJmG6D1D8U9o8qpBGhhXEKLMxiPofTjJyqFttSrGEuXaiFqvWfqfbSn4JNaosAhLLVhLigTpMQUVaUav",
                fixture: include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/fixtures/flashx/sellv2-2ww3fpS3SJmG6D1D8U9o8qpBGhhXEKLMxiPofTjJyqFttSrGEuXaiFqvWfqfbSn4JNaosAhLLVhLigTpMQUVaUav.tx.base64"
                )),
                action: Action::Sell,
                mint: FLASHX_V2_MINT,
                sol_amount: None,
                token_amount: Some(17_160.142_596),
                copyable: false,
            },
            ReplayCase {
                signature: "Jo9sxcrorVCGkmafhNDQKByQBDBTSqM99tS9R1mYs6DjvFZHxZFuFhAvdSemCxFqauPcqS1t17ir3iDScu7cQF5",
                fixture: include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/fixtures/flashx/migrated-buy-Jo9sxcrorVCGkmafhNDQKByQBDBTSqM99tS9R1mYs6DjvFZHxZFuFhAvdSemCxFqauPcqS1t17ir3iDScu7cQF5.tx.base64"
                )),
                action: Action::Buy,
                mint: MIGRATED_MINT,
                sol_amount: Some(0.00099),
                token_amount: None,
                copyable: true,
            },
            ReplayCase {
                signature: "5DAmMyL269Qip9c2JoXfa2XCkNZ1S2nVSoa3KuZ6oowthFCXuUwvvWfBZ3omjyi6BRpM9DAiwsaQ732cqG9u3vhx",
                fixture: include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/fixtures/flashx/migrated-sell-5DAmMyL269Qip9c2JoXfa2XCkNZ1S2nVSoa3KuZ6oowthFCXuUwvvWfBZ3omjyi6BRpM9DAiwsaQ732cqG9u3vhx.tx.base64"
                )),
                action: Action::Sell,
                mint: MIGRATED_MINT,
                sol_amount: None,
                token_amount: Some(1_187.998_876),
                copyable: false,
            },
        ];

        for case in cases {
            let transaction = replay_transaction(case.fixture);
            assert_eq!(versioned_tx_signature_string(&transaction), case.signature);

            let account_keys = static_account_keys(&transaction);
            let parsed = crate::parser::parse_trade(
                &transaction,
                &account_keys,
                &[TARGET_WALLET.to_string()],
            )
            .expect("real FLASHX replay should parse");

            let event = normalized_event(
                123,
                "replay".to_string(),
                case.signature.to_string(),
                456,
                account_keys.len(),
                parsed,
            );
            let value = serde_json::to_value(event).expect("event serializes");

            assert_eq!(value["schema"], "copytrade.feed.event.v1");
            assert_eq!(value["signature"], case.signature);
            assert_eq!(value["targetWallet"], TARGET_WALLET);
            assert_eq!(value["action"], serde_json::to_value(case.action).unwrap());
            assert_eq!(value["route"], "flashx-pump");
            assert_eq!(value["mint"], case.mint);
            assert_eq!(value["copyable"], case.copyable);
            assert_optional_amount(&value, "solAmount", case.sol_amount);
            assert_optional_amount(&value, "tokenAmount", case.token_amount);
        }
    }

    struct ReplayCase {
        signature: &'static str,
        fixture: &'static str,
        action: Action,
        mint: &'static str,
        sol_amount: Option<f64>,
        token_amount: Option<f64>,
        copyable: bool,
    }

    fn replay_transaction(base64_fixture: &str) -> VersionedTransaction {
        let compact = base64_fixture.split_whitespace().collect::<String>();
        let bytes = STANDARD.decode(compact).expect("fixture is valid base64");
        bincode::deserialize(&bytes).expect("fixture decodes as a VersionedTransaction")
    }

    fn assert_optional_amount(value: &serde_json::Value, field: &str, expected: Option<f64>) {
        match expected {
            Some(expected) => {
                let actual = value[field].as_f64().expect("amount should be present");
                assert!((actual - expected).abs() < f64::EPSILON);
            }
            None => assert!(value.get(field).is_none()),
        }
    }

    fn fixture_transaction(
        action: Action,
        target_wallet: &str,
        mint: &str,
    ) -> VersionedTransaction {
        let (account_keys, accounts, data) = match action {
            Action::Buy => (
                vec![
                    pubkey(target_wallet),
                    pubkey(FLASHX_ROUTER_PROGRAM_ID),
                    pubkey("EFLkR6FmWkJ9yU5yLrm41F89pj7cHXBDdMK8YF1aAEHa"),
                    pubkey("BQThTXSc2MKmve6vBmPWTaRHQV5HrCdixwc2WqFZMfAh"),
                    pubkey("4zgbUbVwijbQFqfEVrhqqUcCCbiRJbXL8PGniCr3XK1E"),
                    pubkey("8xNoo7YGjZe54TZEg7rWkJQDnhFtLbPyKUmoM1uj6epB"),
                    pubkey("4y5eTjvLnCb29XDNQns9NaUqmxbiKjBFj9hayGvS5F35"),
                    pubkey("8aHZJSt6frgjRTTfg4foDXjZHMyZ2ZQQjpwcWzzCvAGp"),
                    pubkey("ComputeBudget111111111111111111111111111111"),
                    pubkey("11111111111111111111111111111111"),
                    pubkey("A7WqRwBimpwpyCkxkwPLNnQojCzMiZjnVwWzgooMq6mM"),
                    pubkey(mint),
                    pubkey("Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y"),
                    pubkey("5nKZFATtumvxVPH4ij9BcpeSaFhKXuqzuNaXzuDiMZQ"),
                ],
                vec![
                    0, 0, 14, 9, 1, 20, 10, 2, 21, 15, 11, 3, 4, 5, 0, 9, 22, 6, 23, 20, 12, 7, 24,
                    25, 13, 16, 1, 17, 1, 1, 1, 1,
                ],
                flashx_data(990_000, 23_312_268_344),
            ),
            Action::Sell => (
                vec![
                    pubkey(target_wallet),
                    pubkey(FLASHX_ROUTER_PROGRAM_ID),
                    pubkey("8xNoo7YGjZe54TZEg7rWkJQDnhFtLbPyKUmoM1uj6epB"),
                    pubkey("EFLkR6FmWkJ9yU5yLrm41F89pj7cHXBDdMK8YF1aAEHa"),
                    pubkey("BQThTXSc2MKmve6vBmPWTaRHQV5HrCdixwc2WqFZMfAh"),
                    pubkey("4zgbUbVwijbQFqfEVrhqqUcCCbiRJbXL8PGniCr3XK1E"),
                    pubkey("4y5eTjvLnCb29XDNQns9NaUqmxbiKjBFj9hayGvS5F35"),
                    pubkey("8aHZJSt6frgjRTTfg4foDXjZHMyZ2ZQQjpwcWzzCvAGp"),
                    pubkey("3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT"),
                    pubkey("ComputeBudget111111111111111111111111111111"),
                    pubkey("11111111111111111111111111111111"),
                    pubkey("A7WqRwBimpwpyCkxkwPLNnQojCzMiZjnVwWzgooMq6mM"),
                    pubkey(mint),
                    pubkey("5nKZFATtumvxVPH4ij9BcpeSaFhKXuqzuNaXzuDiMZQ"),
                ],
                vec![
                    2, 0, 14, 10, 1, 19, 11, 3, 20, 15, 12, 4, 5, 2, 0, 10, 6, 21, 22, 19, 23, 24,
                    7, 13, 16, 1, 17, 1, 1, 1, 1,
                ],
                flashx_data(104_905_207_774, 1_737_998),
            ),
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
                    program_id_index: 1,
                    accounts,
                    data,
                }],
            }),
        }
    }

    fn flashx_data(amount_in: u64, minimum_out: u64) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(0);
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.extend_from_slice(&minimum_out.to_le_bytes());
        data.extend_from_slice(&[0, 1, 0x21, 0x32, 0]);
        data
    }

    fn pubkey(value: &str) -> Pubkey {
        Pubkey::from_str(value).expect("fixture pubkey is valid")
    }
}
