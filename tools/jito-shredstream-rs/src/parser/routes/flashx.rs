use crate::parser::{
    associated_token_program_id, flashx_router_program_id, pump_amm_program_id,
    pump_fun_program_id, read_u64_le, sol_mint, token_2022_program_id, token_program_id, Action,
    DirectPumpAccounts, FlashxPumpLayout, FlashxPumpResolvedAccounts, FlashxPumpRouteContext,
    MigratedAmmAccounts, ParsedTrade, Route, RouteContext, RouteInstructionAccount,
    LAMPORTS_PER_SOL, PUMP_FUN_TOKEN_DECIMALS,
};
use solana_message::compiled_instruction::CompiledInstruction;
use solana_message::VersionedMessage;
use solana_pubkey::Pubkey;
use std::collections::HashSet;

pub(crate) fn parse(
    instruction: &CompiledInstruction,
    account_keys: &[Pubkey],
    target_wallets: &HashSet<Pubkey>,
) -> Option<ParsedTrade> {
    let accounts = instruction.accounts.as_slice();
    let first_account = account_key_at(accounts, account_keys, 0)?;
    let second_account = account_key_at(accounts, account_keys, 1)?;
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

    if is_migrated_amm_candidate(&accounts, account_keys, target_wallets) {
        return None;
    }

    let mint = account_key_at(accounts, account_keys, 10)?;

    if target_wallets.contains(first_account) {
        return Some(ParsedTrade {
            target_wallet: *first_account,
            action: Action::Buy,
            mint: *mint,
            route: Route::FlashxPump,
            sol_amount: Some(amount_in as f64 / LAMPORTS_PER_SOL),
            token_amount: None,
            route_context: None,
        });
    }

    if target_wallets.contains(second_account) {
        return Some(ParsedTrade {
            target_wallet: *second_account,
            action: Action::Sell,
            mint: *mint,
            route: Route::FlashxPump,
            sol_amount: None,
            token_amount: Some(amount_in as f64 / PUMP_FUN_TOKEN_DECIMALS),
            route_context: None,
        });
    }

    None
}

fn parse_migrated_amm_layout(
    accounts: &[u8],
    account_keys: &[Pubkey],
    target_wallets: &HashSet<Pubkey>,
    amount_in: u64,
    data: &[u8],
) -> Option<ParsedTrade> {
    let target_wallet = account_key_at(accounts, account_keys, 1)?;
    if !target_wallets.contains(target_wallet) {
        return None;
    }

    let mint = account_key_at(accounts, account_keys, 12)?;
    if !is_migrated_amm_layout(accounts, account_keys) && !is_pump_mint(mint) {
        return None;
    }

    match data.get(17).copied()? {
        0 => Some(ParsedTrade {
            target_wallet: *target_wallet,
            action: Action::Buy,
            mint: *mint,
            route: Route::FlashxPump,
            sol_amount: Some(amount_in as f64 / LAMPORTS_PER_SOL),
            token_amount: None,
            route_context: None,
        }),
        1 => Some(ParsedTrade {
            target_wallet: *target_wallet,
            action: Action::Sell,
            mint: *mint,
            route: Route::FlashxPump,
            sol_amount: None,
            token_amount: Some(amount_in as f64 / PUMP_FUN_TOKEN_DECIMALS),
            route_context: None,
        }),
        _ => None,
    }
}

fn parse_long_v2_layout(
    accounts: &[u8],
    account_keys: &[Pubkey],
    target_wallets: &HashSet<Pubkey>,
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
            target_wallet: *target_wallet,
            action: Action::Buy,
            mint: *mint,
            route: Route::FlashxPump,
            sol_amount: Some(amount_in as f64 / LAMPORTS_PER_SOL),
            token_amount: None,
            route_context: None,
        });
    }

    if let Some(mint) = account_key_at(accounts, account_keys, 9).filter(|mint| is_pump_mint(mint))
    {
        return Some(ParsedTrade {
            target_wallet: *target_wallet,
            action: Action::Sell,
            mint: *mint,
            route: Route::FlashxPump,
            sol_amount: None,
            token_amount: Some(amount_in as f64 / PUMP_FUN_TOKEN_DECIMALS),
            route_context: None,
        });
    }

    None
}

pub(crate) fn route_context(
    message: &VersionedMessage,
    instruction: &CompiledInstruction,
    account_keys: &[Pubkey],
    parsed: &ParsedTrade,
) -> Option<RouteContext> {
    let accounts = instruction.accounts.as_slice();

    match parsed.action {
        Action::Buy => {
            if is_migrated_amm_buy_layout(&accounts, account_keys, parsed, &instruction.data) {
                return Some(RouteContext::FlashxPump(FlashxPumpRouteContext {
                    layout: FlashxPumpLayout::MigratedAmm,
                    program_id: *flashx_router_program_id(),
                    accounts: route_instruction_accounts(message, instruction, account_keys)?
                        .into(),
                    data: instruction.data.as_slice().into(),
                    resolved_accounts: migrated_amm_resolved_accounts(&accounts, account_keys)?,
                }));
            }

            if is_direct_pump_buy_layout(&accounts, account_keys, parsed, &instruction.data) {
                return Some(RouteContext::FlashxPump(FlashxPumpRouteContext {
                    layout: FlashxPumpLayout::DirectPump,
                    program_id: *flashx_router_program_id(),
                    accounts: route_instruction_accounts(message, instruction, account_keys)?
                        .into(),
                    data: instruction.data.as_slice().into(),
                    resolved_accounts: direct_pump_buy_resolved_accounts(
                        &accounts,
                        account_keys,
                        &instruction.data,
                    )?,
                }));
            }
        }
        Action::Sell => {
            if is_direct_pump_sell_layout(&accounts, account_keys, parsed, &instruction.data) {
                return Some(RouteContext::FlashxPump(FlashxPumpRouteContext {
                    layout: FlashxPumpLayout::DirectPump,
                    program_id: *flashx_router_program_id(),
                    accounts: route_instruction_accounts(message, instruction, account_keys)?
                        .into(),
                    data: instruction.data.as_slice().into(),
                    resolved_accounts: direct_pump_sell_resolved_accounts(
                        &accounts,
                        account_keys,
                        &instruction.data,
                    )?,
                }));
            }
        }
    }

    None
}

fn is_migrated_amm_buy_layout(
    accounts: &[u8],
    account_keys: &[Pubkey],
    parsed: &ParsedTrade,
    data: &[u8],
) -> bool {
    is_migrated_amm_layout(accounts, account_keys)
        && data.get(17).copied() == Some(0)
        && account_key_at(accounts, account_keys, 1)
            .is_some_and(|account| account == &parsed.target_wallet)
        && account_key_at(accounts, account_keys, 12).is_some_and(|account| account == &parsed.mint)
}

fn is_migrated_amm_layout(accounts: &[u8], account_keys: &[Pubkey]) -> bool {
    accounts.len() >= 35
        && has_full_account_keys(accounts, account_keys)
        && account_key_at(accounts, account_keys, 4)
            .is_some_and(|account| account == flashx_router_program_id())
        && account_key_at(accounts, account_keys, 5)
            .is_some_and(|account| account == pump_amm_program_id())
        && account_key_at(accounts, account_keys, 13).is_some_and(|account| account == sol_mint())
        && account_key_at(accounts, account_keys, 20).is_some_and(|account| {
            account == token_program_id() || account == token_2022_program_id()
        })
        && account_key_at(accounts, account_keys, 21)
            .is_some_and(|account| account == token_program_id())
        && account_key_at(accounts, account_keys, 23)
            .is_some_and(|account| account == associated_token_program_id())
}

fn is_migrated_amm_candidate(
    accounts: &[u8],
    account_keys: &[Pubkey],
    target_wallets: &HashSet<Pubkey>,
) -> bool {
    accounts.len() >= 35
        && account_key_at(accounts, account_keys, 1)
            .is_some_and(|account| target_wallets.contains(account))
        && account_key_at(accounts, account_keys, 12).is_some()
}

fn has_full_account_keys(accounts: &[u8], account_keys: &[Pubkey]) -> bool {
    accounts
        .iter()
        .all(|index| account_keys.get(*index as usize).is_some())
}

fn is_direct_pump_buy_layout(
    accounts: &[u8],
    account_keys: &[Pubkey],
    parsed: &ParsedTrade,
    data: &[u8],
) -> bool {
    accounts.len() >= 32
        && data.get(17).copied() == Some(0)
        && account_key_at(accounts, account_keys, 0)
            .is_some_and(|account| account == &parsed.target_wallet)
        && account_key_at(accounts, account_keys, 10).is_some_and(|account| account == &parsed.mint)
}

fn is_direct_pump_sell_layout(
    accounts: &[u8],
    account_keys: &[Pubkey],
    parsed: &ParsedTrade,
    data: &[u8],
) -> bool {
    accounts.len() >= 25
        && data.get(17).copied() == Some(1)
        && account_key_at(accounts, account_keys, 1)
            .is_some_and(|account| account == &parsed.target_wallet)
        && account_key_at(accounts, account_keys, 10).is_some_and(|account| account == &parsed.mint)
        && account_key_at(accounts, account_keys, 4)
            .is_some_and(|account| account == flashx_router_program_id())
        && account_key_at(accounts, account_keys, 5)
            .is_some_and(|account| account == pump_fun_program_id())
}

fn route_instruction_accounts(
    message: &VersionedMessage,
    instruction: &CompiledInstruction,
    account_keys: &[Pubkey],
) -> Option<Vec<RouteInstructionAccount>> {
    instruction
        .accounts
        .iter()
        .map(|index| {
            let account_index = *index as usize;
            Some(RouteInstructionAccount {
                pubkey: *account_keys.get(account_index)?,
                is_signer: message.is_signer(account_index),
                is_writable: message.is_maybe_writable(account_index, None),
            })
        })
        .collect()
}

fn migrated_amm_resolved_accounts(
    accounts: &[u8],
    account_keys: &[Pubkey],
) -> Option<FlashxPumpResolvedAccounts> {
    let suffix = migrated_amm_suffix_accounts(accounts, account_keys)?;
    Some(FlashxPumpResolvedAccounts::MigratedAmm(
        MigratedAmmAccounts {
            payer: *account_key_at(accounts, account_keys, 1)?,
            target_wallet: *account_key_at(accounts, account_keys, 1)?,
            flashx_router_program: *account_key_at(accounts, account_keys, 4)?,
            pump_amm_program: *account_key_at(accounts, account_keys, 5)?,
            pool_state: *account_key_at(accounts, account_keys, 9)?,
            global_config: *account_key_at(accounts, account_keys, 11)?,
            mint: *account_key_at(accounts, account_keys, 12)?,
            quote_mint: *account_key_at(accounts, account_keys, 13)?,
            user_base_token_account: *account_key_at(accounts, account_keys, 14)?,
            user_quote_token_account: *account_key_at(accounts, account_keys, 15)?,
            pool_base_token_account: *account_key_at(accounts, account_keys, 16)?,
            pool_quote_token_account: *account_key_at(accounts, account_keys, 17)?,
            protocol_fee_recipient: *account_key_at(accounts, account_keys, 18)?,
            protocol_fee_recipient_token_account: *account_key_at(accounts, account_keys, 19)?,
            base_token_program: *account_key_at(accounts, account_keys, 20)?,
            quote_token_program: *account_key_at(accounts, account_keys, 21)?,
            system_program: *account_key_at(accounts, account_keys, 22)?,
            associated_token_program: *account_key_at(accounts, account_keys, 23)?,
            event_authority: *account_key_at(accounts, account_keys, 24)?,
            coin_creator_vault_ata: *account_key_at(accounts, account_keys, 26)?,
            coin_creator_vault_authority: *account_key_at(accounts, account_keys, 27)?,
            global_volume_accumulator: *account_key_at(accounts, account_keys, 28)?,
            user_volume_accumulator: *account_key_at(accounts, account_keys, 29)?,
            user_volume_accumulator_quote_token_account: suffix
                .user_volume_accumulator_quote_token_account,
            fee_config: *account_key_at(accounts, account_keys, 30)?,
            fee_program: *account_key_at(accounts, account_keys, 31)?,
            pool_v2: suffix.pool_v2,
            buyback_fee_recipient: suffix.buyback_fee_recipient,
            buyback_fee_recipient_token_account: suffix.buyback_fee_recipient_token_account,
        },
    ))
}

struct MigratedAmmSuffixAccounts {
    user_volume_accumulator_quote_token_account: Option<Pubkey>,
    pool_v2: Option<Pubkey>,
    buyback_fee_recipient: Option<Pubkey>,
    buyback_fee_recipient_token_account: Option<Pubkey>,
}

fn migrated_amm_suffix_accounts(
    accounts: &[u8],
    account_keys: &[Pubkey],
) -> Option<MigratedAmmSuffixAccounts> {
    let mint = *account_key_at(accounts, account_keys, 12)?;
    let expected_pool_v2 =
        Pubkey::find_program_address(&[b"pool-v2", mint.as_ref()], pump_amm_program_id()).0;

    if account_key_at(accounts, account_keys, 32).copied() == Some(expected_pool_v2) {
        return Some(MigratedAmmSuffixAccounts {
            user_volume_accumulator_quote_token_account: None,
            pool_v2: Some(expected_pool_v2),
            buyback_fee_recipient: account_key_at(accounts, account_keys, 33).copied(),
            buyback_fee_recipient_token_account: account_key_at(accounts, account_keys, 34)
                .copied(),
        });
    }

    if account_key_at(accounts, account_keys, 33).copied() == Some(expected_pool_v2) {
        return Some(MigratedAmmSuffixAccounts {
            user_volume_accumulator_quote_token_account: account_key_at(accounts, account_keys, 32)
                .copied(),
            pool_v2: Some(expected_pool_v2),
            buyback_fee_recipient: account_key_at(accounts, account_keys, 34).copied(),
            buyback_fee_recipient_token_account: account_key_at(accounts, account_keys, 35)
                .copied(),
        });
    }

    Some(MigratedAmmSuffixAccounts {
        user_volume_accumulator_quote_token_account: None,
        pool_v2: account_key_at(accounts, account_keys, 32).copied(),
        buyback_fee_recipient: account_key_at(accounts, account_keys, 33).copied(),
        buyback_fee_recipient_token_account: account_key_at(accounts, account_keys, 34).copied(),
    })
}

fn direct_pump_buy_resolved_accounts(
    accounts: &[u8],
    account_keys: &[Pubkey],
    data: &[u8],
) -> Option<FlashxPumpResolvedAccounts> {
    Some(FlashxPumpResolvedAccounts::DirectPump(DirectPumpAccounts {
        payer: *account_key_at(accounts, account_keys, 0)?,
        target_wallet: *account_key_at(accounts, account_keys, 0)?,
        flashx_router_program: *account_key_at(accounts, account_keys, 4)?,
        pump_program: *account_key_at(accounts, account_keys, 5)?,
        global_config: *account_key_at(accounts, account_keys, 8)?,
        fee_recipient: *account_key_at(accounts, account_keys, 9)?,
        mint: *account_key_at(accounts, account_keys, 10)?,
        bonding_curve: *account_key_at(accounts, account_keys, 11)?,
        associated_bonding_curve: *account_key_at(accounts, account_keys, 12)?,
        user_token_account: *account_key_at(accounts, account_keys, 13)?,
        system_program: *account_key_at(accounts, account_keys, 15)?,
        token_program: *account_key_at(accounts, account_keys, 16)?,
        creator_vault: *account_key_at(accounts, account_keys, 17)?,
        event_authority: *account_key_at(accounts, account_keys, 18)?,
        global_volume_accumulator: Some(*account_key_at(accounts, account_keys, 20)?),
        user_volume_accumulator: Some(*account_key_at(accounts, account_keys, 21)?),
        fee_config: *account_key_at(accounts, account_keys, 22)?,
        fee_program: *account_key_at(accounts, account_keys, 23)?,
        bonding_curve_v2: *account_key_at(accounts, account_keys, 24)?,
        buyback_fee_recipient: *account_key_at(accounts, account_keys, 25)?,
        buyback_fee_recipient_token_account: account_key_at(accounts, account_keys, 27)
            .filter(|account| *account != flashx_router_program_id())
            .copied(),
        router_amount: read_u64_le(data, 1),
    }))
}

fn direct_pump_sell_resolved_accounts(
    accounts: &[u8],
    account_keys: &[Pubkey],
    data: &[u8],
) -> Option<FlashxPumpResolvedAccounts> {
    Some(FlashxPumpResolvedAccounts::DirectPump(DirectPumpAccounts {
        payer: *account_key_at(accounts, account_keys, 1)?,
        target_wallet: *account_key_at(accounts, account_keys, 1)?,
        flashx_router_program: *account_key_at(accounts, account_keys, 4)?,
        pump_program: *account_key_at(accounts, account_keys, 5)?,
        global_config: *account_key_at(accounts, account_keys, 8)?,
        fee_recipient: *account_key_at(accounts, account_keys, 9)?,
        mint: *account_key_at(accounts, account_keys, 10)?,
        bonding_curve: *account_key_at(accounts, account_keys, 11)?,
        associated_bonding_curve: *account_key_at(accounts, account_keys, 12)?,
        user_token_account: *account_key_at(accounts, account_keys, 13)?,
        system_program: *account_key_at(accounts, account_keys, 15)?,
        token_program: *account_key_at(accounts, account_keys, 17)?,
        creator_vault: *account_key_at(accounts, account_keys, 16)?,
        event_authority: *account_key_at(accounts, account_keys, 18)?,
        global_volume_accumulator: None,
        user_volume_accumulator: None,
        fee_config: *account_key_at(accounts, account_keys, 20)?,
        fee_program: *account_key_at(accounts, account_keys, 21)?,
        bonding_curve_v2: *account_key_at(accounts, account_keys, 22)?,
        buyback_fee_recipient: *account_key_at(accounts, account_keys, 23)?,
        buyback_fee_recipient_token_account: account_key_at(accounts, account_keys, 24)
            .filter(|account| *account != flashx_router_program_id())
            .copied(),
        router_amount: read_u64_le(data, 1),
    }))
}

fn account_key_at<'a>(
    accounts: &[u8],
    account_keys: &'a [Pubkey],
    account_position: usize,
) -> Option<&'a Pubkey> {
    account_keys.get(*accounts.get(account_position)? as usize)
}

fn is_pump_mint(account_key: &Pubkey) -> bool {
    account_key.to_string().ends_with("pump")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        event::normalized_event,
        parser::{
            static_account_keys, versioned_tx_signature_string, FLASHX_ROUTER_PROGRAM_ID,
            PUMP_FUN_PROGRAM_ID, SOL_MINT, SYSTEM_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
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
    const LIVE_MIGRATED_MINT: &str = "J6UVkdPVe4cbd6qGJHdoacMa7zvN3tiaordcyZRspump";
    const LIVE_DIRECT_PUMP_MINT: &str = "8VigmMkK7f9FvTBDd8S2UmweezCgeBX4y5Xp4jMfpump";
    const LIVE_DIRECT_PUMP_SELL_MINT: &str = "5crWJiLmj6ZgtLqfbWiMkryo9vxs96cJhVcq5VFkpump";
    const NON_SUFFIX_MIGRATED_MINT: &str = "9uV9GPNMS6WpjYBr95tbuLKWHkgRHZhN9weYxERuwobo";
    const COPY_WALLET: &str = "FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W";

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
    fn parses_live_direct_pump_sell_route_context_from_target_sell() {
        let transaction = live_direct_pump_sell_transaction();
        let account_keys = static_account_keys(&transaction);
        let parsed =
            crate::parser::parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
                .expect("live direct Pump sell should parse");

        assert_eq!(parsed.action, Action::Sell);
        assert_eq!(parsed.mint, pubkey(LIVE_DIRECT_PUMP_SELL_MINT));
        let RouteContext::FlashxPump(context) = parsed
            .route_context
            .as_ref()
            .expect("live direct Pump sell should resolve route context");
        assert_eq!(context.layout, FlashxPumpLayout::DirectPump);
        assert_eq!(
            context
                .direct_pump_accounts()
                .expect("direct Pump accounts")
                .router_amount,
            Some(34_970_684_247)
        );
        assert_eq!(
            context
                .resolved_pubkey("mint")
                .map(|pubkey| pubkey.to_string())
                .as_deref(),
            Some(LIVE_DIRECT_PUMP_SELL_MINT)
        );
        assert!(context.resolved_pubkey("userVolumeAccumulator").is_none());
        assert_eq!(
            context
                .resolved_pubkey("buybackFeeRecipientTokenAccount")
                .map(|pubkey| pubkey.to_string())
                .as_deref(),
            Some("5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD")
        );
    }

    #[test]
    fn parses_flashx_migrated_amm_buy_when_mint_does_not_end_pump() {
        let transaction = non_suffix_migrated_amm_transaction();
        let account_keys = static_account_keys(&transaction);
        let parsed =
            crate::parser::parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
                .expect("non-suffix migrated FLASHX buy should parse");

        assert_eq!(parsed.action, Action::Buy);
        assert_eq!(parsed.mint, pubkey(NON_SUFFIX_MIGRATED_MINT));
        let RouteContext::FlashxPump(context) = parsed
            .route_context
            .as_ref()
            .expect("route context should resolve");
        assert_eq!(context.layout, FlashxPumpLayout::MigratedAmm);
        assert_eq!(
            context
                .resolved_pubkey("baseTokenProgram")
                .map(|pubkey| pubkey.to_string())
                .as_deref(),
            Some(TOKEN_2022_PROGRAM_ID)
        );
        let copy_build = crate::tx_builder::build_copy_unsigned_flashx_pump(
            parsed.route_context.as_ref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
        )
        .expect("non-suffix migrated route should build copy instructions");
        assert_eq!(copy_build.route_layout, "migrated-amm");
        assert_eq!(copy_build.instructions.len(), 2);
        assert!(copy_build
            .instructions
            .iter()
            .flat_map(|instruction| instruction.accounts.iter())
            .any(|account| account.pubkey.to_string() == COPY_WALLET && account.is_signer));
        assert!(!copy_build
            .instructions
            .iter()
            .flat_map(|instruction| instruction.accounts.iter())
            .any(|account| account.pubkey.to_string() == TARGET_WALLET));

        let event = normalized_event(
            123,
            "replay".to_string(),
            "2vDt4SJrw9co6EXEzV4dhh2TFN1B9rhsrhQ8Tchs3xZH52jbfMCqtXqCkC2Eq42Vcyn8HqCeVKun9zcBnqQXGUAU"
                .to_string(),
            456,
            account_keys.len(),
            parsed,
        );
        let value = serde_json::to_value(event).expect("event serializes");

        assert_eq!(value["action"], "buy");
        assert_eq!(value["route"], "flashx-pump");
        assert_eq!(value["targetWallet"], TARGET_WALLET);
        assert_eq!(value["mint"], NON_SUFFIX_MIGRATED_MINT);
        assert_eq!(value["copyable"], true);
        assert_eq!(value["input"]["mint"], SOL_MINT);
        assert_eq!(value["output"]["mint"], NON_SUFFIX_MIGRATED_MINT);
        assert!((value["solAmount"].as_f64().unwrap() - 0.00099).abs() < f64::EPSILON);
        assert!(value.get("tokenAmount").is_none());
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
                signature: "hYCB3CXxuEw4aofMSiNoenDwGJ9u3XQq1TVXBsSk33TztaUmypa1B2aPZbM7s7dpkW5qeCE7rEEPVMYWczDW33k",
                fixture: include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/fixtures/flashx/buy-hYCB3CXxuEw4aofMSiNoenDwGJ9u3XQq1TVXBsSk33TztaUmypa1B2aPZbM7s7dpkW5qeCE7rEEPVMYWczDW33k.tx.base64"
                )),
                action: Action::Buy,
                mint: LIVE_MIGRATED_MINT,
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
            ReplayCase {
                signature: "2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo",
                fixture: include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
                )),
                action: Action::Buy,
                mint: LIVE_DIRECT_PUMP_MINT,
                sol_amount: Some(0.00099),
                token_amount: None,
                copyable: true,
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
            assert_eq!(
                value["action"],
                serde_json::to_value(case.action).unwrap(),
                "case {}",
                case.signature
            );
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

    fn live_direct_pump_sell_transaction() -> VersionedTransaction {
        let account_keys = vec![
            pubkey(TARGET_WALLET),
            pubkey(FLASHX_ROUTER_PROGRAM_ID),
            pubkey("HwvHGuNdBwZkqMnNW4rrCh6VnMMy2K29u3uPrgbRkopw"),
            pubkey("BhUimdz2Mr41p1RF3pY6wtCncxhEUgo7oaBXkFkZXkJK"),
            pubkey("6yGKUgoJYTGSsbJ1MbgKaRYq1gADVafecUGLtqWbMSk"),
            pubkey("EWQzRSwFmB9QxDZpHuPAjgKXprtLCUmvHAKJf6TV8iJz"),
            pubkey("CFzb8zvhad9MpLcDP3ZAQWej6fiNmFHBvbYZb3sNBacE"),
            pubkey("8aHZJSt6frgjRTTfg4foDXjZHMyZ2ZQQjpwcWzzCvAGp"),
            pubkey(SYSTEM_PROGRAM_ID),
            pubkey(SYSTEM_PROGRAM_ID),
            pubkey(SYSTEM_PROGRAM_ID),
            pubkey("8GG1EWaPbS7yHoWELsGKWm1QtrAjxD4dGQWFRaKkoAwk"),
            pubkey(LIVE_DIRECT_PUMP_SELL_MINT),
            pubkey("DkqLRb1K85dbU93arHhYARr5Jm67gNyTK91BKviXueCD"),
            pubkey("4FobGn5ZWYquoJkxMzh2VUAWvV36xMgxQ3M7uG1pGGhd"),
            pubkey("FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz"),
            pubkey("5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD"),
            pubkey("3PvqoztjnRxaAiFmLuEfqZkU4GSbjUareks8S2xCZaTa"),
            pubkey(SYSTEM_PROGRAM_ID),
            pubkey(PUMP_FUN_PROGRAM_ID),
            pubkey("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf"),
            pubkey(TOKEN_2022_PROGRAM_ID),
            pubkey("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1"),
            pubkey("8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt"),
            pubkey("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ"),
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
                    program_id_index: 1,
                    accounts: vec![
                        2, 0, 14, 10, 1, 19, 11, 3, 20, 15, 12, 4, 5, 2, 0, 10, 6, 21, 22, 19, 23,
                        24, 7, 13, 16, 1, 17, 1, 1, 1, 1,
                    ],
                    data: vec![
                        0, 87, 75, 106, 36, 8, 0, 0, 0, 8, 215, 8, 0, 0, 0, 0, 0, 1, 1, 26, 50, 0,
                    ],
                }],
            }),
        }
    }

    fn non_suffix_migrated_amm_transaction() -> VersionedTransaction {
        let account_keys = vec![
            pubkey(TARGET_WALLET),
            pubkey("Hao96PD9Ue3RtDMvvHXjxHPUmKgZSxTmgFikoUsxoWJ7"),
            pubkey(FLASHX_ROUTER_PROGRAM_ID),
            pubkey("83DqVhmHb3RmZa8ieYC7VtHB5upyC5GHAr6g4WYfMjg4"),
            pubkey("AxTZuziyf73B7xZM5FNC2Qwc6yvuxtBd6eUTXbBQvuwH"),
            pubkey("7BXW8ACG9sAnuj8ajikwqX41Z4JbbNKzRCc5MfAoCEHF"),
            pubkey("RSdHsQTcT4oesKPcWBuxvLiDD6PXiih5ANWpbZKQpDL"),
            pubkey("B59DykJ64vJG4a2ZyoWKwdkYbTJoxN68YtxoCacsTDUM"),
            pubkey("6GFtiF2g8BnFTH3Y6vAbUBfYkB6zn7dyRyeP1CYbfR79"),
            pubkey("82JokvYzsarTaVkeD2ecUnT3SewbyhStn9aZ4RQwxUs2"),
            pubkey("ComputeBudget111111111111111111111111111111"),
            pubkey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"),
            pubkey(NON_SUFFIX_MIGRATED_MINT),
            pubkey("11111111111111111111111111111111"),
            pubkey("H7z1jHozHuwG7qZB28VDsJTwfhKnhZi5YHbo6GC8r3D9"),
            pubkey("6tcNMJQ6V71AdFsgETBLGFF8ULJSddoMXydSjQypaNZg"),
            pubkey("szQHePmm6j39zyon5ZYqgJ3Z2HQ8P2weB14eokCH2eL"),
            pubkey("4E6JpXRfirkLtDx7h4DduE6ALtiLxVQKEsTLQbX7RdgF"),
            pubkey("8vFGAKdwpn4hk7kc1cBgfWZzpyW3MEMDATDzVZhddeQb"),
            pubkey("7GFUN3bWzJMKMRZ34JLsvcqdssDbXnp589SiE33KVwcC"),
            pubkey("qkYdTGRPHbWTWuBMz45bCiU6a23axRqf6sBHm9295WY"),
            pubkey("5vPNE6VFyXmCmzmWotdxmRk57LEWiXxuAfZL3hKbi2LH"),
            pubkey("5L2QKqDn5ukJSWGyqR4RPvFvwnBabKWqAqMzH4heaQNB"),
            pubkey("jitodontfront81111111TradeWithAxiomDotTrade"),
            pubkey(TOKEN_2022_PROGRAM_ID),
            pubkey(SOL_MINT),
            pubkey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
            pubkey("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA"),
            pubkey("ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw"),
            pubkey("7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ"),
            pubkey("GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR"),
            pubkey("C2aFPdENg4A2HQsmrd5rTw5TaYBX5Ku887cWjbFKtZpw"),
            pubkey("5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx"),
            pubkey("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ"),
            pubkey("A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW"),
        ];

        VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::Legacy(Message {
                header: MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                account_keys,
                recent_blockhash: Hash::default(),
                instructions: vec![CompiledInstruction {
                    program_id_index: 2,
                    accounts: vec![
                        3, 0, 18, 13, 2, 27, 14, 15, 4, 5, 0, 28, 12, 25, 1, 3, 6, 7, 29, 19, 24,
                        26, 13, 11, 30, 27, 8, 16, 31, 9, 32, 33, 17, 34, 20, 2, 26, 3, 2, 21, 2,
                        2, 2, 2,
                    ],
                    data: vec![
                        0, 48, 27, 15, 0, 0, 0, 0, 0, 227, 153, 135, 64, 0, 0, 0, 0, 0, 2, 31, 0,
                        50, 0,
                    ],
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
