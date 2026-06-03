use crate::parser::{
    read_u64_le, FlashxPumpLayout, ResolvedRouteAccount, RouteContext, ASSOCIATED_TOKEN_PROGRAM_ID,
    COMPUTE_BUDGET_PROGRAM_ID, PUMP_FUN_PROGRAM_ID, PUMP_FUN_SELL_DISCRIMINATOR, SYSTEM_PROGRAM_ID,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use std::str::FromStr;

#[derive(Debug)]
pub(crate) struct UnsignedTxBuild {
    pub(crate) route_layout: &'static str,
    pub(crate) resolved_accounts: Vec<ResolvedRouteAccount>,
    pub(crate) instructions: Vec<Instruction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TxBuildError {
    MissingRouteContext(&'static str),
    UnsupportedLayout(&'static str),
    InvalidInstruction(&'static str),
}

#[derive(Debug)]
pub(crate) struct CopyUnsignedTxBuild {
    pub(crate) route_layout: &'static str,
    pub(crate) copy_wallet_token_account: String,
    pub(crate) instructions: Vec<Instruction>,
}

#[derive(Debug)]
pub(crate) struct FullCopyUnsignedTxBuild {
    pub(crate) route_layout: &'static str,
    pub(crate) copy_wallet_token_account: String,
    pub(crate) estimated_required_signer: String,
    pub(crate) setup_instruction_count: usize,
    pub(crate) main_instruction_count: usize,
    pub(crate) instructions: Vec<Instruction>,
}

const PUMP_FUN_BUY_EXACT_SOL_IN_DISCRIMINATOR: [u8; 8] = [56, 252, 116, 8, 158, 223, 205, 95];
const PUMP_FUN_COPY_MIN_TOKENS_OUT: u64 = 1;

pub(crate) fn build_unsigned_flashx_pump(
    route_context: Option<&RouteContext>,
) -> Result<UnsignedTxBuild, TxBuildError> {
    let Some(RouteContext::FlashxPump(context)) = route_context else {
        return Err(TxBuildError::UnsupportedLayout(
            "unsupported flashx-pump layout",
        ));
    };

    if !matches!(
        context.layout,
        FlashxPumpLayout::MigratedAmm | FlashxPumpLayout::DirectPump
    ) {
        return Err(TxBuildError::UnsupportedLayout(
            "unsupported flashx-pump layout",
        ));
    }

    if context.accounts.is_empty() || context.resolved_accounts.is_empty() {
        return Err(TxBuildError::MissingRouteContext(
            "missing flashx-pump migrated route accounts",
        ));
    }

    if context.data.is_empty() {
        return Err(TxBuildError::InvalidInstruction(
            "missing flashx-pump router instruction data",
        ));
    }

    let program_id = parse_pubkey(&context.program_id)?;
    let accounts = context
        .accounts
        .iter()
        .map(|account| {
            Ok(AccountMeta {
                pubkey: parse_pubkey(&account.pubkey)?,
                is_signer: account.is_signer,
                is_writable: account.is_writable,
            })
        })
        .collect::<Result<Vec<_>, TxBuildError>>()?;

    Ok(UnsignedTxBuild {
        route_layout: context.layout.as_str(),
        resolved_accounts: context.resolved_accounts.clone(),
        instructions: vec![Instruction {
            program_id,
            accounts,
            data: context.data.clone(),
        }],
    })
}

fn parse_pubkey(value: &str) -> Result<Pubkey, TxBuildError> {
    Pubkey::from_str(value).map_err(|_| TxBuildError::InvalidInstruction("invalid route pubkey"))
}

pub(crate) fn build_copy_unsigned_flashx_pump(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
) -> Result<CopyUnsignedTxBuild, TxBuildError> {
    let Some(RouteContext::FlashxPump(context)) = route_context else {
        return Err(TxBuildError::UnsupportedLayout(
            "unsupported flashx-pump copy layout",
        ));
    };

    if context.layout != FlashxPumpLayout::DirectPump {
        return Err(TxBuildError::UnsupportedLayout(
            "unsupported flashx-pump copy layout",
        ));
    }

    if context.accounts.is_empty() || context.data.is_empty() {
        return Err(TxBuildError::MissingRouteContext(
            "missing flashx-pump direct route accounts",
        ));
    }

    let token_program = resolved_pubkey(&context.resolved_accounts, "tokenProgram")?;
    let copy_wallet = parse_pubkey(copy_wallet)?;
    let mint = parse_pubkey(mint)?;
    let associated_token_program = parse_pubkey(ASSOCIATED_TOKEN_PROGRAM_ID)?;
    let pump_program = parse_pubkey(PUMP_FUN_PROGRAM_ID)?;
    let copy_wallet_token_account = associated_token_address(
        &copy_wallet,
        &mint,
        &token_program,
        &associated_token_program,
    );
    let copy_user_volume_accumulator = user_volume_accumulator_address(&copy_wallet, &pump_program);

    let spendable_sol_in = read_u64_le(&context.data, 1).ok_or(
        TxBuildError::InvalidInstruction("missing flashx SOL amount"),
    )?;

    let mut buy_data = Vec::with_capacity(25);
    buy_data.extend_from_slice(&PUMP_FUN_BUY_EXACT_SOL_IN_DISCRIMINATOR);
    buy_data.extend_from_slice(&spendable_sol_in.to_le_bytes());
    buy_data.extend_from_slice(&PUMP_FUN_COPY_MIN_TOKENS_OUT.to_le_bytes());
    buy_data.push(1);

    let accounts = vec![
        AccountMeta::new_readonly(
            resolved_pubkey(&context.resolved_accounts, "globalConfig")?,
            false,
        ),
        AccountMeta::new(
            resolved_pubkey(&context.resolved_accounts, "feeRecipient")?,
            false,
        ),
        AccountMeta::new_readonly(mint, false),
        AccountMeta::new(
            resolved_pubkey(&context.resolved_accounts, "bondingCurve")?,
            false,
        ),
        AccountMeta::new(
            resolved_pubkey(&context.resolved_accounts, "associatedBondingCurve")?,
            false,
        ),
        AccountMeta::new(copy_wallet_token_account, false),
        AccountMeta::new(copy_wallet, true),
        AccountMeta::new_readonly(
            resolved_pubkey(&context.resolved_accounts, "systemProgram")?,
            false,
        ),
        AccountMeta::new_readonly(
            resolved_pubkey(&context.resolved_accounts, "tokenProgram")?,
            false,
        ),
        AccountMeta::new(
            resolved_pubkey(&context.resolved_accounts, "creatorVault")?,
            false,
        ),
        AccountMeta::new_readonly(
            resolved_pubkey(&context.resolved_accounts, "eventAuthority")?,
            false,
        ),
        AccountMeta::new_readonly(
            resolved_pubkey(&context.resolved_accounts, "pumpProgram")?,
            false,
        ),
        AccountMeta::new_readonly(
            resolved_pubkey(&context.resolved_accounts, "globalVolumeAccumulator")?,
            false,
        ),
        AccountMeta::new(copy_user_volume_accumulator, false),
        AccountMeta::new_readonly(
            resolved_pubkey(&context.resolved_accounts, "feeConfig")?,
            false,
        ),
        AccountMeta::new_readonly(
            resolved_pubkey(&context.resolved_accounts, "feeProgram")?,
            false,
        ),
        AccountMeta::new_readonly(
            resolved_pubkey(&context.resolved_accounts, "bondingCurveV2")?,
            false,
        ),
        AccountMeta::new(
            resolved_pubkey(&context.resolved_accounts, "buybackFeeRecipient")?,
            false,
        ),
    ];

    Ok(CopyUnsignedTxBuild {
        route_layout: context.layout.as_str(),
        copy_wallet_token_account: copy_wallet_token_account.to_string(),
        instructions: vec![Instruction {
            program_id: pump_program,
            accounts,
            data: buy_data,
        }],
    })
}

pub(crate) fn build_full_copy_unsigned_flashx_pump(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    let copy_build = build_copy_unsigned_flashx_pump(route_context, copy_wallet, mint)?;
    let Some(RouteContext::FlashxPump(context)) = route_context else {
        return Err(TxBuildError::UnsupportedLayout(
            "unsupported flashx-pump copy layout",
        ));
    };

    let copy_wallet_pubkey = parse_pubkey(copy_wallet)?;
    let copy_wallet_token_account = parse_pubkey(&copy_build.copy_wallet_token_account)?;
    let mint = parse_pubkey(mint)?;
    let token_program = resolved_pubkey(&context.resolved_accounts, "tokenProgram")?;
    let associated_token_program = parse_pubkey(ASSOCIATED_TOKEN_PROGRAM_ID)?;
    let system_program = parse_pubkey(SYSTEM_PROGRAM_ID)?;

    let mut instructions = Vec::with_capacity(copy_build.instructions.len() + 2);
    instructions.push(compute_unit_limit_instruction(400_000)?);
    instructions.push(create_associated_token_account_idempotent_instruction(
        &copy_wallet_pubkey,
        &copy_wallet_token_account,
        &mint,
        &token_program,
        &associated_token_program,
        &system_program,
    ));
    instructions.extend(copy_build.instructions);

    Ok(FullCopyUnsignedTxBuild {
        route_layout: copy_build.route_layout,
        copy_wallet_token_account: copy_wallet_token_account.to_string(),
        estimated_required_signer: copy_wallet_pubkey.to_string(),
        setup_instruction_count: 2,
        main_instruction_count: instructions.len().saturating_sub(2),
        instructions,
    })
}

pub(crate) fn build_auto_sell_unsigned_flashx_pump(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
    token_amount_raw: u64,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    if token_amount_raw == 0 {
        return Err(TxBuildError::InvalidInstruction(
            "missing positive auto-sell token amount",
        ));
    }

    let copy_build = build_copy_unsigned_flashx_pump(route_context, copy_wallet, mint)?;
    let Some(RouteContext::FlashxPump(context)) = route_context else {
        return Err(TxBuildError::UnsupportedLayout(
            "unsupported auto-sell layout",
        ));
    };
    if context.layout != FlashxPumpLayout::DirectPump {
        return Err(TxBuildError::UnsupportedLayout(
            "unsupported auto-sell layout",
        ));
    }

    let pump_program = parse_pubkey(PUMP_FUN_PROGRAM_ID)?;
    let copy_wallet_token_account = parse_pubkey(&copy_build.copy_wallet_token_account)?;
    let copy_wallet_pubkey = parse_pubkey(copy_wallet)?;

    let mut sell_data = Vec::with_capacity(24);
    sell_data.extend_from_slice(&PUMP_FUN_SELL_DISCRIMINATOR);
    sell_data.extend_from_slice(&token_amount_raw.to_le_bytes());
    sell_data.extend_from_slice(&0u64.to_le_bytes());

    let sell_instruction = Instruction {
        program_id: pump_program,
        accounts: vec![
            AccountMeta::new_readonly(
                resolved_pubkey(&context.resolved_accounts, "globalConfig")?,
                false,
            ),
            AccountMeta::new(
                resolved_pubkey(&context.resolved_accounts, "feeRecipient")?,
                false,
            ),
            AccountMeta::new_readonly(parse_pubkey(mint)?, false),
            AccountMeta::new(
                resolved_pubkey(&context.resolved_accounts, "bondingCurve")?,
                false,
            ),
            AccountMeta::new(
                resolved_pubkey(&context.resolved_accounts, "associatedBondingCurve")?,
                false,
            ),
            AccountMeta::new(copy_wallet_token_account, false),
            AccountMeta::new(copy_wallet_pubkey, true),
            AccountMeta::new_readonly(
                resolved_pubkey(&context.resolved_accounts, "systemProgram")?,
                false,
            ),
            AccountMeta::new(
                resolved_pubkey(&context.resolved_accounts, "creatorVault")?,
                false,
            ),
            AccountMeta::new_readonly(
                resolved_pubkey(&context.resolved_accounts, "tokenProgram")?,
                false,
            ),
            AccountMeta::new_readonly(
                resolved_pubkey(&context.resolved_accounts, "eventAuthority")?,
                false,
            ),
            AccountMeta::new_readonly(
                resolved_pubkey(&context.resolved_accounts, "pumpProgram")?,
                false,
            ),
            AccountMeta::new_readonly(
                resolved_pubkey(&context.resolved_accounts, "feeConfig")?,
                false,
            ),
            AccountMeta::new_readonly(
                resolved_pubkey(&context.resolved_accounts, "feeProgram")?,
                false,
            ),
            AccountMeta::new_readonly(
                resolved_pubkey(&context.resolved_accounts, "bondingCurveV2")?,
                false,
            ),
            AccountMeta::new(
                resolved_pubkey(&context.resolved_accounts, "buybackFeeRecipient")?,
                false,
            ),
        ],
        data: sell_data,
    };

    let mut instructions = Vec::with_capacity(2);
    instructions.push(compute_unit_limit_instruction(400_000)?);
    instructions.push(sell_instruction);

    Ok(FullCopyUnsignedTxBuild {
        route_layout: copy_build.route_layout,
        copy_wallet_token_account: copy_build.copy_wallet_token_account,
        estimated_required_signer: copy_wallet_pubkey.to_string(),
        setup_instruction_count: 1,
        main_instruction_count: instructions.len().saturating_sub(1),
        instructions,
    })
}

fn resolved_pubkey(
    resolved_accounts: &[ResolvedRouteAccount],
    role: &'static str,
) -> Result<Pubkey, TxBuildError> {
    let account = resolved_accounts
        .iter()
        .find(|account| account.role == role)
        .ok_or(TxBuildError::MissingRouteContext(
            "missing direct-pump route account",
        ))?;
    parse_pubkey(&account.pubkey)
}

fn associated_token_address(
    wallet: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
    associated_token_program: &Pubkey,
) -> Pubkey {
    Pubkey::find_program_address(
        &[wallet.as_ref(), token_program.as_ref(), mint.as_ref()],
        associated_token_program,
    )
    .0
}

fn user_volume_accumulator_address(wallet: &Pubkey, pump_program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"user_volume_accumulator", wallet.as_ref()], pump_program).0
}

fn compute_unit_limit_instruction(units: u32) -> Result<Instruction, TxBuildError> {
    let program_id = parse_pubkey(COMPUTE_BUDGET_PROGRAM_ID)?;
    let mut data = Vec::with_capacity(5);
    data.push(2);
    data.extend_from_slice(&units.to_le_bytes());

    Ok(Instruction {
        program_id,
        accounts: Vec::new(),
        data,
    })
}

fn create_associated_token_account_idempotent_instruction(
    payer: &Pubkey,
    associated_token_account: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
    associated_token_program: &Pubkey,
    system_program: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: *associated_token_program,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(*associated_token_account, false),
            AccountMeta::new_readonly(*payer, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(*system_program, false),
            AccountMeta::new_readonly(*token_program, false),
        ],
        data: vec![1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{
        parse_trade, static_account_keys, versioned_tx_signature_string, FLASHX_ROUTER_PROGRAM_ID,
    };
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use solana_transaction::versioned::VersionedTransaction;

    const TARGET_WALLET: &str = "A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS";
    const MIGRATED_BUY_SIGNATURE: &str =
        "Jo9sxcrorVCGkmafhNDQKByQBDBTSqM99tS9R1mYs6DjvFZHxZFuFhAvdSemCxFqauPcqS1t17ir3iDScu7cQF5";
    const LIVE_DIRECT_PUMP_BUY_SIGNATURE: &str =
        "2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo";
    const COPY_WALLET: &str = "FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W";
    const LIVE_DIRECT_PUMP_MINT: &str = "8VigmMkK7f9FvTBDd8S2UmweezCgeBX4y5Xp4jMfpump";

    #[test]
    fn builds_unsigned_flashx_migrated_instruction_from_replay_context() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/migrated-buy-Jo9sxcrorVCGkmafhNDQKByQBDBTSqM99tS9R1mYs6DjvFZHxZFuFhAvdSemCxFqauPcqS1t17ir3iDScu7cQF5.tx.base64"
        )));
        assert_eq!(
            versioned_tx_signature_string(&transaction),
            MIGRATED_BUY_SIGNATURE
        );
        let account_keys = migrated_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("migrated FLASHX buy should parse");

        let build = build_unsigned_flashx_pump(parsed.route_context.as_ref())
            .expect("migrated route should build unsigned instruction");

        assert_eq!(build.route_layout, "migrated-amm");
        assert_eq!(build.instructions.len(), 1);
        assert_eq!(
            build.instructions[0].program_id.to_string(),
            FLASHX_ROUTER_PROGRAM_ID
        );
        assert!(!build.instructions[0].accounts.is_empty());
        assert!(!build.instructions[0].data.is_empty());
        assert!(build.instructions[0]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == TARGET_WALLET && account.is_signer));
    }

    #[test]
    fn builds_unsigned_flashx_direct_pump_instruction_from_live_replay_context() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        assert_eq!(
            versioned_tx_signature_string(&transaction),
            LIVE_DIRECT_PUMP_BUY_SIGNATURE
        );
        let account_keys = live_direct_pump_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live direct Pump FLASHX buy should parse");

        let build = build_unsigned_flashx_pump(parsed.route_context.as_ref())
            .expect("direct Pump route should build unsigned instruction");

        assert_eq!(build.route_layout, "direct-pump");
        assert_eq!(build.instructions.len(), 1);
        assert_eq!(
            build.instructions[0].program_id.to_string(),
            FLASHX_ROUTER_PROGRAM_ID
        );
        assert_eq!(build.instructions[0].accounts.len(), 32);
        assert!(!build.instructions[0].data.is_empty());
        assert!(build
            .resolved_accounts
            .iter()
            .any(|account| account.role == "pumpProgram"
                && account.pubkey == "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"));
    }

    #[test]
    fn builds_unsigned_copy_instruction_for_direct_pump_with_copy_wallet_accounts() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        let account_keys = live_direct_pump_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live direct Pump FLASHX buy should parse");

        let build = build_copy_unsigned_flashx_pump(
            parsed.route_context.as_ref(),
            COPY_WALLET,
            &parsed.mint,
        )
        .expect("direct Pump copy route should build unsigned instruction");

        assert_eq!(build.route_layout, "direct-pump");
        assert_eq!(build.instructions.len(), 1);
        assert_eq!(
            build.instructions[0].program_id.to_string(),
            PUMP_FUN_PROGRAM_ID
        );
        assert_eq!(build.instructions[0].accounts.len(), 18);
        assert_eq!(
            &build.instructions[0].data[0..8],
            &PUMP_FUN_BUY_EXACT_SOL_IN_DISCRIMINATOR
        );
        assert_eq!(
            &build.instructions[0].data[8..16],
            &990_000u64.to_le_bytes()
        );
        assert_eq!(
            &build.instructions[0].data[16..24],
            &PUMP_FUN_COPY_MIN_TOKENS_OUT.to_le_bytes()
        );
        assert_eq!(build.instructions[0].data[24], 1);
        assert_eq!(
            build.copy_wallet_token_account,
            "G2Bp3rC5GQHw8gWguLdujeZdTRoRgQia3Y1FmD5Ch4Vs"
        );
        assert!(build.instructions[0]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == COPY_WALLET && account.is_signer));
        assert!(build.instructions[0]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == build.copy_wallet_token_account));
        assert!(build.instructions[0]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string()
                == "A6z9cMVt6RovLTYpLbkawnTDEGtFpLuEgE3t7BYHJCm2"));
        assert!(!build.instructions[0]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string()
                == "8aHZJSt6frgjRTTfg4foDXjZHMyZ2ZQQjpwcWzzCvAGp"));
        assert!(!build.instructions[0]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == TARGET_WALLET));
        assert_eq!(parsed.mint, LIVE_DIRECT_PUMP_MINT);
    }

    #[test]
    fn builds_full_unsigned_copy_transaction_shell_for_direct_pump() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        let account_keys = live_direct_pump_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live direct Pump FLASHX buy should parse");

        let build = build_full_copy_unsigned_flashx_pump(
            parsed.route_context.as_ref(),
            COPY_WALLET,
            &parsed.mint,
        )
        .expect("full copy transaction shell should build");

        assert_eq!(build.route_layout, "direct-pump");
        assert_eq!(
            build.copy_wallet_token_account,
            "G2Bp3rC5GQHw8gWguLdujeZdTRoRgQia3Y1FmD5Ch4Vs"
        );
        assert_eq!(build.estimated_required_signer, COPY_WALLET);
        assert_eq!(build.setup_instruction_count, 2);
        assert_eq!(build.main_instruction_count, 1);
        assert_eq!(build.instructions.len(), 3);
        assert_eq!(
            build.instructions[0].program_id.to_string(),
            COMPUTE_BUDGET_PROGRAM_ID
        );
        assert_eq!(build.instructions[0].data, vec![2, 0x80, 0x1a, 0x06, 0x00]);
        assert_eq!(
            build.instructions[1].program_id.to_string(),
            ASSOCIATED_TOKEN_PROGRAM_ID
        );
        assert_eq!(build.instructions[1].data, vec![1]);
        assert_eq!(
            build.instructions[2].program_id.to_string(),
            PUMP_FUN_PROGRAM_ID
        );
        assert_eq!(
            &build.instructions[2].data[0..8],
            &PUMP_FUN_BUY_EXACT_SOL_IN_DISCRIMINATOR
        );
    }

    #[test]
    fn builds_auto_sell_instruction_for_direct_pump_copy_balance() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        let account_keys = live_direct_pump_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live direct Pump FLASHX buy should parse");

        let build = build_auto_sell_unsigned_flashx_pump(
            parsed.route_context.as_ref(),
            COPY_WALLET,
            &parsed.mint,
            123_456,
        )
        .expect("auto-sell route should build");

        assert_eq!(build.route_layout, "direct-pump");
        assert_eq!(build.instructions.len(), 2);
        assert_eq!(
            build.instructions[1].program_id.to_string(),
            PUMP_FUN_PROGRAM_ID
        );
        assert_eq!(
            &build.instructions[1].data[0..8],
            &PUMP_FUN_SELL_DISCRIMINATOR
        );
        assert_eq!(
            &build.instructions[1].data[8..16],
            &123_456u64.to_le_bytes()
        );
        assert_eq!(&build.instructions[1].data[16..24], &0u64.to_le_bytes());
        assert!(build.instructions[1]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == COPY_WALLET && account.is_signer));
        assert!(build.instructions[1].accounts.iter().any(|account| {
            account.pubkey.to_string() == "DhWQaUj4YBCyRvGuUfwcAjGNTvgB5murcVLT2VdTr1UZ"
                && account.is_writable
        }));
    }

    fn replay_transaction(base64_fixture: &str) -> VersionedTransaction {
        let compact = base64_fixture.split_whitespace().collect::<String>();
        let bytes = STANDARD.decode(compact).expect("fixture is valid base64");
        bincode::deserialize(&bytes).expect("fixture decodes as a VersionedTransaction")
    }

    fn migrated_buy_hydrated_account_keys(transaction: &VersionedTransaction) -> Vec<String> {
        let mut account_keys = static_account_keys(transaction);
        account_keys.extend(
            [
                "86Vh4XGLW2b6nvWbRyDs4ScgMXbuvRCHT7WbUT3RFxKG",
                "7GFUN3bWzJMKMRZ34JLsvcqdssDbXnp589SiE33KVwcC",
                "AktftA98kSWAxn6kVSoqBXBELUArjKu2H9WmKB48ULFY",
                "ECDrSz47nXihe5kyK4oWEePPsPi9qz6u5d6Fa2sDj3uM",
                "11111111111111111111111111111111",
                "11111111111111111111111111111111",
                "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
                "So11111111111111111111111111111111111111112",
                "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA",
                "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw",
                "7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ",
                "GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR",
                "C2aFPdENg4A2HQsmrd5rTw5TaYBX5Ku887cWjbFKtZpw",
                "5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx",
                "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ",
                "GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL",
            ]
            .into_iter()
            .map(ToString::to_string),
        );
        account_keys
    }

    fn live_direct_pump_buy_hydrated_account_keys(
        transaction: &VersionedTransaction,
    ) -> Vec<String> {
        let mut account_keys = static_account_keys(transaction);
        account_keys.extend(
            [
                "DKyUs1xXMDy8Z11zNsLnUg3dy9HZf6hYZidB6WodcaGy",
                "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM",
                "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD",
                "ECDrSz47nXihe5kyK4oWEePPsPi9qz6u5d6Fa2sDj3uM",
                "11111111111111111111111111111111",
                "11111111111111111111111111111111",
                "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
                "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
                "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf",
                "Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y",
                "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1",
                "8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt",
                "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ",
            ]
            .into_iter()
            .map(ToString::to_string),
        );
        account_keys
    }
}
