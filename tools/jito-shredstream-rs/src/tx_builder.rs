use crate::parser::{
    associated_token_program_id, compute_budget_program_id, pump_amm_program_id, read_u64_le,
    system_program_id, DirectPumpAccounts, FlashxPumpLayout, MigratedAmmAccounts,
    ResolvedRouteAccountJson, RouteContext, PUMP_FUN_SELL_DISCRIMINATOR,
};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use std::{collections::HashMap, str::FromStr, sync::Mutex};

#[derive(Clone, Debug, Default)]
pub(crate) struct TxFeeConfig {
    pub(crate) compute_unit_price_micro_lamports: Option<u64>,
    pub(crate) jito_tip_lamports: Option<u64>,
    pub(crate) jito_tip_account: Option<String>,
    pub(crate) helius_sender_tip_lamports: Option<u64>,
    pub(crate) helius_sender_tip_account: Option<String>,
    pub(crate) nozomi_tip_lamports: Option<u64>,
    pub(crate) nozomi_tip_account: Option<String>,
    pub(crate) bloxroute_tip_lamports: Option<u64>,
    pub(crate) bloxroute_tip_account: Option<String>,
}

#[derive(Debug)]
pub(crate) struct UnsignedTxBuild {
    pub(crate) route_layout: &'static str,
    pub(crate) resolved_accounts: Vec<ResolvedRouteAccountJson>,
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
    pub(crate) copy_wallet_token_account: Pubkey,
    pub(crate) instructions: Vec<Instruction>,
}

#[derive(Debug)]
pub(crate) struct FullCopyUnsignedTxBuild {
    pub(crate) route_layout: &'static str,
    pub(crate) copy_wallet_token_account: Pubkey,
    pub(crate) estimated_required_signer: Pubkey,
    pub(crate) setup_instruction_count: usize,
    pub(crate) main_instruction_count: usize,
    pub(crate) instructions: Vec<Instruction>,
}

#[derive(Default, Debug)]
pub(crate) struct CopyPdaCache {
    associated_token_accounts: Mutex<HashMap<AssociatedTokenAddressKey, Pubkey>>,
    user_volume_accumulators: Mutex<HashMap<UserVolumeAccumulatorKey, Pubkey>>,
    flashx_wrapped_sol_accounts: Mutex<HashMap<FlashxWrappedSolAccountKey, (Pubkey, u8)>>,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct AssociatedTokenAddressKey {
    wallet: Pubkey,
    mint: Pubkey,
    token_program: Pubkey,
    associated_token_program: Pubkey,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct UserVolumeAccumulatorKey {
    wallet: Pubkey,
    pump_program: Pubkey,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct FlashxWrappedSolAccountKey {
    wallet: Pubkey,
    flashx_program: Pubkey,
}

const PUMP_FUN_BUY_EXACT_SOL_IN_DISCRIMINATOR: [u8; 8] = [56, 252, 116, 8, 158, 223, 205, 95];
const PUMP_FUN_COPY_MIN_TOKENS_OUT: u64 = 1;
const DIRECT_PUMP_COPY_MIN_SOL_OUT: u64 = 1;
const MISSING_DIRECT_PUMP_ROUTE_ACCOUNT: &str = "missing direct-pump route account";
const MISSING_MIGRATED_AMM_ROUTE_ACCOUNT: &str = "missing migrated-amm route account";

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

    if context.accounts.is_empty() {
        return Err(TxBuildError::MissingRouteContext(
            "missing flashx-pump migrated route accounts",
        ));
    }

    if context.data.is_empty() {
        return Err(TxBuildError::InvalidInstruction(
            "missing flashx-pump router instruction data",
        ));
    }

    let accounts = context
        .accounts
        .iter()
        .map(|account| AccountMeta {
            pubkey: account.pubkey,
            is_signer: account.is_signer,
            is_writable: account.is_writable,
        })
        .collect::<Vec<_>>();

    Ok(UnsignedTxBuild {
        route_layout: context.layout.as_str(),
        resolved_accounts: context.resolved_accounts_for_json(),
        instructions: vec![Instruction {
            program_id: context.program_id,
            accounts,
            data: context.data.to_vec(),
        }],
    })
}

fn parse_pubkey(value: &str) -> Result<Pubkey, TxBuildError> {
    Pubkey::from_str(value).map_err(|_| TxBuildError::InvalidInstruction("invalid route pubkey"))
}

fn fee_tip_transfers(fee_config: &TxFeeConfig) -> Result<Vec<(Pubkey, u64)>, TxBuildError> {
    let mut transfers = Vec::with_capacity(4);
    push_fee_tip_transfer(
        &mut transfers,
        fee_config.jito_tip_lamports,
        fee_config.jito_tip_account.as_deref(),
        "missing Jito tip account",
    )?;
    push_fee_tip_transfer(
        &mut transfers,
        fee_config.helius_sender_tip_lamports,
        fee_config.helius_sender_tip_account.as_deref(),
        "missing Helius Sender tip account",
    )?;
    push_fee_tip_transfer(
        &mut transfers,
        fee_config.nozomi_tip_lamports,
        fee_config.nozomi_tip_account.as_deref(),
        "missing Nozomi tip account",
    )?;
    push_fee_tip_transfer(
        &mut transfers,
        fee_config.bloxroute_tip_lamports,
        fee_config.bloxroute_tip_account.as_deref(),
        "missing bloXroute tip account",
    )?;
    Ok(transfers)
}

fn push_fee_tip_transfer(
    transfers: &mut Vec<(Pubkey, u64)>,
    lamports: Option<u64>,
    account: Option<&str>,
    missing_account_error: &'static str,
) -> Result<(), TxBuildError> {
    let Some(lamports) = lamports.filter(|value| *value > 0) else {
        return Ok(());
    };
    let Some(account) = account.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(TxBuildError::MissingRouteContext(missing_account_error));
    };
    let account = parse_pubkey(account)?;
    if let Some((_, existing_lamports)) = transfers
        .iter_mut()
        .find(|(existing_account, _)| *existing_account == account)
    {
        *existing_lamports = (*existing_lamports).max(lamports);
    } else {
        transfers.push((account, lamports));
    }
    Ok(())
}

pub(crate) fn build_copy_unsigned_flashx_pump(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
) -> Result<CopyUnsignedTxBuild, TxBuildError> {
    let mint = parse_pubkey(mint)?;
    build_copy_unsigned_flashx_pump_with_cache(route_context, copy_wallet, &mint, None)
}

fn build_copy_unsigned_flashx_pump_with_cache(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &Pubkey,
    pda_cache: Option<&CopyPdaCache>,
) -> Result<CopyUnsignedTxBuild, TxBuildError> {
    build_copy_unsigned_flashx_pump_with_cache_and_spend(
        route_context,
        copy_wallet,
        mint,
        pda_cache,
        None,
    )
}

fn build_copy_unsigned_flashx_pump_with_cache_and_spend(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &Pubkey,
    pda_cache: Option<&CopyPdaCache>,
    copy_spend_lamports: Option<u64>,
) -> Result<CopyUnsignedTxBuild, TxBuildError> {
    let Some(RouteContext::FlashxPump(context)) = route_context else {
        return Err(TxBuildError::UnsupportedLayout(
            "unsupported flashx-pump copy layout",
        ));
    };

    if context.accounts.is_empty() || context.data.is_empty() {
        return Err(TxBuildError::MissingRouteContext(
            "missing flashx-pump route accounts",
        ));
    }

    match context.layout {
        FlashxPumpLayout::DirectPump => build_copy_unsigned_flashx_direct_pump(
            context,
            copy_wallet,
            mint,
            pda_cache,
            copy_spend_lamports,
        ),
        FlashxPumpLayout::MigratedAmm => build_copy_unsigned_flashx_migrated_amm(
            context,
            copy_wallet,
            mint,
            pda_cache,
            copy_spend_lamports,
        ),
    }
}

pub(crate) fn copy_wallet_token_account_for_flashx_pump(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
    pda_cache: Option<&CopyPdaCache>,
) -> Result<Pubkey, TxBuildError> {
    let Some(RouteContext::FlashxPump(context)) = route_context else {
        return Err(TxBuildError::UnsupportedLayout(
            "unsupported flashx-pump copy layout",
        ));
    };

    let copy_wallet = parse_pubkey(copy_wallet)?;
    let mint = parse_pubkey(mint)?;
    match context.layout {
        FlashxPumpLayout::DirectPump => {
            let token_program = direct_pump_accounts(context)?.token_program;
            let associated_token_program = *associated_token_program_id();
            Ok(associated_token_address_cached(
                pda_cache,
                &copy_wallet,
                &mint,
                &token_program,
                &associated_token_program,
            ))
        }
        FlashxPumpLayout::MigratedAmm => {
            let accounts = migrated_amm_accounts(context)?;
            let base_token_program = accounts.base_token_program;
            let associated_token_program = accounts.associated_token_program;
            Ok(associated_token_address_cached(
                pda_cache,
                &copy_wallet,
                &mint,
                &base_token_program,
                &associated_token_program,
            ))
        }
    }
}

fn build_copy_unsigned_flashx_direct_pump(
    context: &crate::parser::FlashxPumpRouteContext,
    copy_wallet: &str,
    mint: &Pubkey,
    pda_cache: Option<&CopyPdaCache>,
    copy_spend_lamports: Option<u64>,
) -> Result<CopyUnsignedTxBuild, TxBuildError> {
    let direct_accounts = direct_pump_accounts(context)?;
    let Some(global_volume_accumulator) = direct_accounts.global_volume_accumulator else {
        return Err(TxBuildError::MissingRouteContext(
            MISSING_DIRECT_PUMP_ROUTE_ACCOUNT,
        ));
    };
    let token_program = direct_accounts.token_program;
    let copy_wallet = parse_pubkey(copy_wallet)?;
    let associated_token_program = *associated_token_program_id();
    let pump_program = direct_accounts.pump_program;
    let copy_wallet_token_account = associated_token_address_cached(
        pda_cache,
        &copy_wallet,
        mint,
        &token_program,
        &associated_token_program,
    );
    let copy_user_volume_accumulator =
        user_volume_accumulator_address_cached(pda_cache, &copy_wallet, &pump_program);

    let spendable_sol_in = copy_spend_lamports
        .or(direct_accounts.router_amount)
        .or_else(|| read_u64_le(&context.data, 1))
        .ok_or(TxBuildError::InvalidInstruction(
            "missing flashx SOL amount",
        ))?;

    let mut buy_data = Vec::with_capacity(25);
    buy_data.extend_from_slice(&PUMP_FUN_BUY_EXACT_SOL_IN_DISCRIMINATOR);
    buy_data.extend_from_slice(&spendable_sol_in.to_le_bytes());
    buy_data.extend_from_slice(&PUMP_FUN_COPY_MIN_TOKENS_OUT.to_le_bytes());
    buy_data.push(1);

    let accounts = vec![
        AccountMeta::new_readonly(direct_accounts.global_config, false),
        AccountMeta::new(direct_accounts.fee_recipient, false),
        AccountMeta::new_readonly(*mint, false),
        AccountMeta::new(direct_accounts.bonding_curve, false),
        AccountMeta::new(direct_accounts.associated_bonding_curve, false),
        AccountMeta::new(copy_wallet_token_account, false),
        AccountMeta::new(copy_wallet, true),
        AccountMeta::new_readonly(direct_accounts.system_program, false),
        AccountMeta::new_readonly(token_program, false),
        AccountMeta::new(direct_accounts.creator_vault, false),
        AccountMeta::new_readonly(direct_accounts.event_authority, false),
        AccountMeta::new_readonly(pump_program, false),
        AccountMeta::new_readonly(global_volume_accumulator, false),
        AccountMeta::new(copy_user_volume_accumulator, false),
        AccountMeta::new_readonly(direct_accounts.fee_config, false),
        AccountMeta::new_readonly(direct_accounts.fee_program, false),
        AccountMeta::new_readonly(direct_accounts.bonding_curve_v2, false),
        AccountMeta::new(direct_accounts.buyback_fee_recipient, false),
    ];

    Ok(CopyUnsignedTxBuild {
        route_layout: context.layout.as_str(),
        copy_wallet_token_account,
        instructions: vec![Instruction {
            program_id: pump_program,
            accounts,
            data: buy_data,
        }],
    })
}

fn build_copy_unsigned_flashx_migrated_amm(
    context: &crate::parser::FlashxPumpRouteContext,
    copy_wallet: &str,
    mint: &Pubkey,
    pda_cache: Option<&CopyPdaCache>,
    copy_spend_lamports: Option<u64>,
) -> Result<CopyUnsignedTxBuild, TxBuildError> {
    let migrated_accounts = migrated_amm_accounts(context)?;
    let copy_wallet = parse_pubkey(copy_wallet)?;
    let flashx_program = context.program_id;
    let pump_amm_program = migrated_accounts.pump_amm_program;
    let base_token_program = migrated_accounts.base_token_program;
    let associated_token_program = migrated_accounts.associated_token_program;
    let system_program = migrated_accounts.system_program;
    let quote_mint = migrated_accounts.quote_mint;
    let quote_token_program = migrated_accounts.quote_token_program;
    let target_quote_token_account = migrated_accounts.user_quote_token_account;
    let target_wallet = migrated_accounts.target_wallet;
    let target_base_token_account = migrated_accounts.user_base_token_account;
    let target_user_volume_accumulator = migrated_accounts.user_volume_accumulator;
    let target_user_volume_accumulator_quote_token_account =
        migrated_accounts.user_volume_accumulator_quote_token_account;
    let copy_base_token_account = associated_token_address_cached(
        pda_cache,
        &copy_wallet,
        mint,
        &base_token_program,
        &associated_token_program,
    );
    let (copy_quote_token_account, copy_quote_bump) =
        flashx_wrapped_sol_account_address_cached(pda_cache, &copy_wallet, &flashx_program);
    let copy_user_volume_accumulator =
        user_volume_accumulator_address_cached(pda_cache, &copy_wallet, &pump_amm_program);
    let copy_user_volume_accumulator_quote_token_account =
        target_user_volume_accumulator_quote_token_account.map(|_| {
            associated_token_address_cached(
                pda_cache,
                &copy_user_volume_accumulator,
                &quote_mint,
                &quote_token_program,
                &associated_token_program,
            )
        });

    let spendable_sol_in = copy_spend_lamports
        .or_else(|| read_u64_le(&context.data, 1))
        .ok_or(TxBuildError::InvalidInstruction(
            "missing flashx SOL amount",
        ))?;

    let mut setup_data = Vec::with_capacity(10);
    setup_data.push(1);
    setup_data.extend_from_slice(&spendable_sol_in.to_le_bytes());
    setup_data.push(copy_quote_bump);

    let setup_instruction = Instruction {
        program_id: flashx_program,
        accounts: vec![
            AccountMeta::new(copy_quote_token_account, false),
            AccountMeta::new(copy_wallet, true),
            AccountMeta::new_readonly(quote_mint, false),
            AccountMeta::new_readonly(quote_token_program, false),
            AccountMeta::new_readonly(system_program, false),
        ],
        data: setup_data,
    };

    let mut route_data = context.data.to_vec();
    rewrite_flashx_migrated_amounts(&mut route_data, spendable_sol_in)?;

    let route_accounts = context
        .accounts
        .iter()
        .map(|account| {
            let mut pubkey = account.pubkey;
            let mut is_signer = account.is_signer;
            let mut is_writable = account.is_writable;
            if pubkey == target_wallet {
                pubkey = copy_wallet;
                is_signer = true;
                is_writable = true;
            } else if pubkey == target_base_token_account {
                pubkey = copy_base_token_account;
            } else if pubkey == target_quote_token_account {
                pubkey = copy_quote_token_account;
            } else if pubkey == target_user_volume_accumulator {
                pubkey = copy_user_volume_accumulator;
            } else if Some(pubkey) == target_user_volume_accumulator_quote_token_account {
                if let Some(copy_account) = copy_user_volume_accumulator_quote_token_account {
                    pubkey = copy_account;
                }
            }

            AccountMeta {
                pubkey,
                is_signer,
                is_writable,
            }
        })
        .collect::<Vec<_>>();

    Ok(CopyUnsignedTxBuild {
        route_layout: context.layout.as_str(),
        copy_wallet_token_account: copy_base_token_account,
        instructions: vec![
            setup_instruction,
            Instruction {
                program_id: flashx_program,
                accounts: route_accounts,
                data: route_data,
            },
        ],
    })
}

fn rewrite_flashx_migrated_amounts(
    data: &mut [u8],
    quote_amount_in: u64,
) -> Result<(), TxBuildError> {
    if data.len() < 17 {
        return Err(TxBuildError::InvalidInstruction(
            "missing flashx migrated amounts",
        ));
    }
    let min_base_amount_out = scaled_flashx_migrated_min_base_amount_out(data, quote_amount_in)?;
    data[1..9].copy_from_slice(&quote_amount_in.to_le_bytes());
    data[9..17].copy_from_slice(&min_base_amount_out.to_le_bytes());
    Ok(())
}

fn scaled_flashx_migrated_min_base_amount_out(
    data: &[u8],
    quote_amount_in: u64,
) -> Result<u64, TxBuildError> {
    let observed_quote_amount_in = read_u64_le(data, 1).ok_or(TxBuildError::InvalidInstruction(
        "missing flashx migrated quote amount",
    ))?;
    let observed_min_base_amount_out = read_u64_le(data, 9).ok_or(
        TxBuildError::InvalidInstruction("missing flashx migrated min base amount"),
    )?;
    if quote_amount_in == 0 || observed_quote_amount_in == 0 {
        return Err(TxBuildError::InvalidInstruction(
            "missing flashx migrated quote amount",
        ));
    }
    let scaled = (observed_min_base_amount_out as u128).saturating_mul(quote_amount_in as u128)
        / observed_quote_amount_in as u128;
    if scaled == 0 && observed_min_base_amount_out > 0 {
        return Err(TxBuildError::InvalidInstruction(
            "scaled flashx migrated min base amount rounds to zero",
        ));
    }
    if scaled > u64::MAX as u128 {
        return Err(TxBuildError::InvalidInstruction(
            "scaled flashx migrated min base amount overflow",
        ));
    }
    Ok(scaled as u64)
}

pub(crate) fn build_full_copy_unsigned_flashx_pump(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    build_full_copy_unsigned_flashx_pump_with_fees(
        route_context,
        copy_wallet,
        mint,
        &TxFeeConfig::default(),
    )
}

pub(crate) fn build_full_copy_unsigned_flashx_pump_with_fees(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
    fee_config: &TxFeeConfig,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    build_full_copy_unsigned_flashx_pump_with_fees_and_cache(
        route_context,
        copy_wallet,
        mint,
        fee_config,
        None,
    )
}

pub(crate) fn build_full_copy_unsigned_flashx_pump_with_fees_and_cache(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
    fee_config: &TxFeeConfig,
    pda_cache: Option<&CopyPdaCache>,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    build_full_copy_unsigned_flashx_pump_with_fees_and_cache_and_spend(
        route_context,
        copy_wallet,
        mint,
        fee_config,
        pda_cache,
        None,
    )
}

pub(crate) fn build_full_copy_unsigned_flashx_pump_with_fees_and_cache_and_spend(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
    fee_config: &TxFeeConfig,
    pda_cache: Option<&CopyPdaCache>,
    copy_spend_lamports: Option<u64>,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    let mint = parse_pubkey(mint)?;
    build_full_copy_unsigned_flashx_pump_with_fees_and_cache_and_spend_for_mint(
        route_context,
        copy_wallet,
        &mint,
        fee_config,
        pda_cache,
        copy_spend_lamports,
    )
}

pub(crate) fn build_full_copy_unsigned_flashx_pump_with_fees_and_cache_and_spend_for_mint(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &Pubkey,
    fee_config: &TxFeeConfig,
    pda_cache: Option<&CopyPdaCache>,
    copy_spend_lamports: Option<u64>,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    let copy_build = build_copy_unsigned_flashx_pump_with_cache_and_spend(
        route_context,
        copy_wallet,
        mint,
        pda_cache,
        copy_spend_lamports,
    )?;
    let Some(RouteContext::FlashxPump(context)) = route_context else {
        return Err(TxBuildError::UnsupportedLayout(
            "unsupported flashx-pump copy layout",
        ));
    };

    let copy_wallet_pubkey = parse_pubkey(copy_wallet)?;
    let copy_wallet_token_account = copy_build.copy_wallet_token_account;
    let token_program = match context.layout {
        FlashxPumpLayout::DirectPump => direct_pump_accounts(context)?.token_program,
        FlashxPumpLayout::MigratedAmm => migrated_amm_accounts(context)?.base_token_program,
    };
    let associated_token_program = *associated_token_program_id();
    let system_program = *system_program_id();

    let mut instructions = Vec::with_capacity(copy_build.instructions.len() + 4);
    instructions.push(compute_unit_limit_instruction(400_000)?);
    if let Some(micro_lamports) = fee_config
        .compute_unit_price_micro_lamports
        .filter(|v| *v > 0)
    {
        instructions.push(compute_unit_price_instruction(micro_lamports)?);
    }
    instructions.push(create_associated_token_account_idempotent_instruction(
        &copy_wallet_pubkey,
        &copy_wallet_token_account,
        &mint,
        &token_program,
        &associated_token_program,
        &system_program,
    ));
    for (tip_account, tip_lamports) in fee_tip_transfers(fee_config)? {
        instructions.push(system_transfer_instruction(
            &copy_wallet_pubkey,
            &tip_account,
            tip_lamports,
        )?);
    }
    let main_instruction_count = copy_build.instructions.len();
    instructions.extend(copy_build.instructions);
    let setup_instruction_count = instructions.len().saturating_sub(main_instruction_count);

    Ok(FullCopyUnsignedTxBuild {
        route_layout: copy_build.route_layout,
        copy_wallet_token_account,
        estimated_required_signer: copy_wallet_pubkey,
        setup_instruction_count,
        main_instruction_count,
        instructions,
    })
}

pub(crate) fn build_auto_sell_unsigned_flashx_pump_with_cache(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
    token_amount_raw: u64,
    pda_cache: Option<&CopyPdaCache>,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    build_sell_unsigned_flashx_pump_with_cache(
        route_context,
        copy_wallet,
        mint,
        token_amount_raw,
        pda_cache,
        false,
    )
}

fn build_sell_unsigned_flashx_pump_with_cache(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
    token_amount_raw: u64,
    pda_cache: Option<&CopyPdaCache>,
    allow_direct_pump_buy_context: bool,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    if token_amount_raw == 0 {
        return Err(TxBuildError::InvalidInstruction(
            "missing positive auto-sell token amount",
        ));
    }

    let Some(RouteContext::FlashxPump(context)) = route_context else {
        return Err(TxBuildError::UnsupportedLayout(
            "unsupported auto-sell layout",
        ));
    };
    let copy_wallet_token_account =
        copy_wallet_token_account_for_flashx_pump(route_context, copy_wallet, mint, pda_cache)?;

    match context.layout {
        FlashxPumpLayout::DirectPump => build_auto_sell_unsigned_flashx_direct_pump(
            context,
            copy_wallet_token_account,
            copy_wallet,
            mint,
            token_amount_raw,
            pda_cache,
            allow_direct_pump_buy_context,
        ),
        FlashxPumpLayout::MigratedAmm => build_auto_sell_unsigned_flashx_migrated_amm(
            context,
            copy_wallet_token_account,
            copy_wallet,
            mint,
            token_amount_raw,
        ),
    }
}

pub(crate) fn build_auto_sell_unsigned_flashx_pump_with_fees_and_cache(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
    token_amount_raw: u64,
    fee_config: &TxFeeConfig,
    pda_cache: Option<&CopyPdaCache>,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    let mut build = build_auto_sell_unsigned_flashx_pump_with_cache(
        route_context,
        copy_wallet,
        mint,
        token_amount_raw,
        pda_cache,
    )?;
    let copy_wallet_pubkey = parse_pubkey(copy_wallet)?;
    let mut fee_instructions = Vec::new();
    if let Some(micro_lamports) = fee_config
        .compute_unit_price_micro_lamports
        .filter(|value| *value > 0)
    {
        fee_instructions.push(compute_unit_price_instruction(micro_lamports)?);
    }
    for (tip_account, tip_lamports) in fee_tip_transfers(fee_config)? {
        fee_instructions.push(system_transfer_instruction(
            &copy_wallet_pubkey,
            &tip_account,
            tip_lamports,
        )?);
    }

    if !fee_instructions.is_empty() {
        let insertion_index = build
            .instructions
            .iter()
            .position(|instruction| instruction.program_id != *compute_budget_program_id())
            .unwrap_or(build.instructions.len());
        let fee_instruction_count = fee_instructions.len();
        build
            .instructions
            .splice(insertion_index..insertion_index, fee_instructions);
        build.setup_instruction_count += fee_instruction_count;
    }

    Ok(build)
}

pub(crate) fn build_trailing_sell_unsigned_flashx_pump_with_fees_and_cache(
    route_context: Option<&RouteContext>,
    copy_wallet: &str,
    mint: &str,
    token_amount_raw: u64,
    fee_config: &TxFeeConfig,
    pda_cache: Option<&CopyPdaCache>,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    let mut build = build_sell_unsigned_flashx_pump_with_cache(
        route_context,
        copy_wallet,
        mint,
        token_amount_raw,
        pda_cache,
        false,
    )?;
    let copy_wallet_pubkey = parse_pubkey(copy_wallet)?;
    let mut fee_instructions = Vec::new();
    if let Some(micro_lamports) = fee_config
        .compute_unit_price_micro_lamports
        .filter(|value| *value > 0)
    {
        fee_instructions.push(compute_unit_price_instruction(micro_lamports)?);
    }
    for (tip_account, tip_lamports) in fee_tip_transfers(fee_config)? {
        fee_instructions.push(system_transfer_instruction(
            &copy_wallet_pubkey,
            &tip_account,
            tip_lamports,
        )?);
    }

    if !fee_instructions.is_empty() {
        let insertion_index = build
            .instructions
            .iter()
            .position(|instruction| instruction.program_id != *compute_budget_program_id())
            .unwrap_or(build.instructions.len());
        let fee_instruction_count = fee_instructions.len();
        build
            .instructions
            .splice(insertion_index..insertion_index, fee_instructions);
        build.setup_instruction_count += fee_instruction_count;
    }

    Ok(build)
}

fn build_auto_sell_unsigned_flashx_direct_pump(
    context: &crate::parser::FlashxPumpRouteContext,
    copy_wallet_token_account: Pubkey,
    copy_wallet: &str,
    _mint: &str,
    token_amount_raw: u64,
    _pda_cache: Option<&CopyPdaCache>,
    _allow_direct_pump_buy_context: bool,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    let copy_wallet_pubkey = parse_pubkey(copy_wallet)?;
    let mut sell_data = Vec::with_capacity(24);
    sell_data.extend_from_slice(&PUMP_FUN_SELL_DISCRIMINATOR);
    sell_data.extend_from_slice(&token_amount_raw.to_le_bytes());
    sell_data.extend_from_slice(&DIRECT_PUMP_COPY_MIN_SOL_OUT.to_le_bytes());

    let mut accounts = vec![
        AccountMeta::new_readonly(resolved_pubkey(context, "globalConfig")?, false),
        AccountMeta::new(resolved_pubkey(context, "feeRecipient")?, false),
        AccountMeta::new_readonly(resolved_pubkey(context, "mint")?, false),
        AccountMeta::new(resolved_pubkey(context, "bondingCurve")?, false),
        AccountMeta::new(resolved_pubkey(context, "associatedBondingCurve")?, false),
        AccountMeta::new(copy_wallet_token_account, false),
        AccountMeta::new(copy_wallet_pubkey, true),
        AccountMeta::new_readonly(resolved_pubkey(context, "systemProgram")?, false),
        AccountMeta::new(resolved_pubkey(context, "creatorVault")?, false),
        AccountMeta::new_readonly(resolved_pubkey(context, "tokenProgram")?, false),
        AccountMeta::new_readonly(resolved_pubkey(context, "eventAuthority")?, false),
        AccountMeta::new_readonly(resolved_pubkey(context, "pumpProgram")?, false),
        AccountMeta::new_readonly(resolved_pubkey(context, "feeConfig")?, false),
        AccountMeta::new_readonly(resolved_pubkey(context, "feeProgram")?, false),
        AccountMeta::new_readonly(resolved_pubkey(context, "bondingCurveV2")?, false),
        AccountMeta::new(resolved_pubkey(context, "buybackFeeRecipient")?, false),
    ];
    if let Ok(buyback_fee_recipient_token_account) =
        resolved_pubkey(context, "buybackFeeRecipientTokenAccount")
    {
        accounts.push(AccountMeta::new(buyback_fee_recipient_token_account, false));
    }

    let mut instructions = Vec::with_capacity(2);
    instructions.push(compute_unit_limit_instruction(400_000)?);
    instructions.push(Instruction {
        program_id: resolved_pubkey(context, "pumpProgram")?,
        accounts,
        data: sell_data,
    });

    Ok(FullCopyUnsignedTxBuild {
        route_layout: context.layout.as_str(),
        copy_wallet_token_account,
        estimated_required_signer: copy_wallet_pubkey,
        setup_instruction_count: 1,
        main_instruction_count: instructions.len().saturating_sub(1),
        instructions,
    })
}

fn build_auto_sell_unsigned_flashx_migrated_amm(
    context: &crate::parser::FlashxPumpRouteContext,
    copy_base_token_account: Pubkey,
    copy_wallet: &str,
    mint: &str,
    token_amount_raw: u64,
) -> Result<FullCopyUnsignedTxBuild, TxBuildError> {
    let copy_wallet_pubkey = parse_pubkey(copy_wallet)?;
    let mint = parse_pubkey(mint)?;
    let pump_amm_program =
        resolved_pubkey(context, "pumpAmmProgram").unwrap_or_else(|_| *pump_amm_program_id());
    let quote_mint = resolved_pubkey(context, "quoteMint")?;
    let quote_token_program = resolved_pubkey(context, "quoteTokenProgram")?;
    let associated_token_program = resolved_pubkey(context, "associatedTokenProgram")?;
    let system_program = resolved_pubkey(context, "systemProgram")?;
    let copy_quote_token_account = associated_token_address(
        &copy_wallet_pubkey,
        &quote_mint,
        &quote_token_program,
        &associated_token_program,
    );
    let mut sell_data = Vec::with_capacity(24);
    sell_data.extend_from_slice(&PUMP_FUN_SELL_DISCRIMINATOR);
    sell_data.extend_from_slice(&token_amount_raw.to_le_bytes());
    sell_data.extend_from_slice(&0u64.to_le_bytes());

    let sell_instruction = Instruction {
        program_id: pump_amm_program,
        accounts: vec![
            AccountMeta::new(resolved_pubkey(context, "poolState")?, false),
            AccountMeta::new(copy_wallet_pubkey, true),
            AccountMeta::new_readonly(resolved_pubkey(context, "globalConfig")?, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(quote_mint, false),
            AccountMeta::new(copy_base_token_account, false),
            AccountMeta::new(copy_quote_token_account, false),
            AccountMeta::new(resolved_pubkey(context, "poolBaseTokenAccount")?, false),
            AccountMeta::new(resolved_pubkey(context, "poolQuoteTokenAccount")?, false),
            AccountMeta::new_readonly(resolved_pubkey(context, "protocolFeeRecipient")?, false),
            AccountMeta::new(
                resolved_pubkey(context, "protocolFeeRecipientTokenAccount")?,
                false,
            ),
            AccountMeta::new_readonly(resolved_pubkey(context, "baseTokenProgram")?, false),
            AccountMeta::new_readonly(quote_token_program, false),
            AccountMeta::new_readonly(system_program, false),
            AccountMeta::new_readonly(associated_token_program, false),
            AccountMeta::new_readonly(resolved_pubkey(context, "eventAuthority")?, false),
            AccountMeta::new_readonly(pump_amm_program, false),
            AccountMeta::new(resolved_pubkey(context, "coinCreatorVaultAta")?, false),
            AccountMeta::new_readonly(
                resolved_pubkey(context, "coinCreatorVaultAuthority")?,
                false,
            ),
            AccountMeta::new_readonly(resolved_pubkey(context, "feeConfig")?, false),
            AccountMeta::new_readonly(resolved_pubkey(context, "feeProgram")?, false),
            AccountMeta::new_readonly(resolved_pubkey(context, "poolV2")?, false),
            AccountMeta::new(resolved_pubkey(context, "buybackFeeRecipient")?, false),
            AccountMeta::new(
                resolved_pubkey(context, "buybackFeeRecipientTokenAccount")?,
                false,
            ),
        ],
        data: sell_data,
    };

    let instructions = vec![
        compute_unit_limit_instruction(400_000)?,
        create_associated_token_account_idempotent_instruction(
            &copy_wallet_pubkey,
            &copy_quote_token_account,
            &quote_mint,
            &quote_token_program,
            &associated_token_program,
            &system_program,
        ),
        sell_instruction,
        close_token_account_instruction(
            &copy_quote_token_account,
            &copy_wallet_pubkey,
            &copy_wallet_pubkey,
            &quote_token_program,
        ),
    ];

    Ok(FullCopyUnsignedTxBuild {
        route_layout: context.layout.as_str(),
        copy_wallet_token_account: copy_base_token_account,
        estimated_required_signer: copy_wallet_pubkey,
        setup_instruction_count: 2,
        main_instruction_count: 2,
        instructions,
    })
}

fn resolved_pubkey(
    context: &crate::parser::FlashxPumpRouteContext,
    role: &'static str,
) -> Result<Pubkey, TxBuildError> {
    context
        .resolved_pubkey(role)
        .ok_or(TxBuildError::MissingRouteContext(
            MISSING_DIRECT_PUMP_ROUTE_ACCOUNT,
        ))
}

fn direct_pump_accounts(
    context: &crate::parser::FlashxPumpRouteContext,
) -> Result<&DirectPumpAccounts, TxBuildError> {
    context
        .direct_pump_accounts()
        .ok_or(TxBuildError::MissingRouteContext(
            MISSING_DIRECT_PUMP_ROUTE_ACCOUNT,
        ))
}

fn migrated_amm_accounts(
    context: &crate::parser::FlashxPumpRouteContext,
) -> Result<&MigratedAmmAccounts, TxBuildError> {
    context
        .migrated_amm_accounts()
        .ok_or(TxBuildError::MissingRouteContext(
            MISSING_MIGRATED_AMM_ROUTE_ACCOUNT,
        ))
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

fn associated_token_address_cached(
    cache: Option<&CopyPdaCache>,
    wallet: &Pubkey,
    mint: &Pubkey,
    token_program: &Pubkey,
    associated_token_program: &Pubkey,
) -> Pubkey {
    let Some(cache) = cache else {
        return associated_token_address(wallet, mint, token_program, associated_token_program);
    };
    cache.associated_token_address(wallet, mint, token_program, associated_token_program)
}

fn user_volume_accumulator_address(wallet: &Pubkey, pump_program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"user_volume_accumulator", wallet.as_ref()], pump_program).0
}

fn user_volume_accumulator_address_cached(
    cache: Option<&CopyPdaCache>,
    wallet: &Pubkey,
    pump_program: &Pubkey,
) -> Pubkey {
    let Some(cache) = cache else {
        return user_volume_accumulator_address(wallet, pump_program);
    };
    cache.user_volume_accumulator_address(wallet, pump_program)
}

fn flashx_wrapped_sol_account_address(wallet: &Pubkey, flashx_program: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"wrapped_sol_account", wallet.as_ref()], flashx_program)
}

fn flashx_wrapped_sol_account_address_cached(
    cache: Option<&CopyPdaCache>,
    wallet: &Pubkey,
    flashx_program: &Pubkey,
) -> (Pubkey, u8) {
    let Some(cache) = cache else {
        return flashx_wrapped_sol_account_address(wallet, flashx_program);
    };
    cache.flashx_wrapped_sol_account_address(wallet, flashx_program)
}

impl CopyPdaCache {
    fn associated_token_address(
        &self,
        wallet: &Pubkey,
        mint: &Pubkey,
        token_program: &Pubkey,
        associated_token_program: &Pubkey,
    ) -> Pubkey {
        let key = AssociatedTokenAddressKey {
            wallet: *wallet,
            mint: *mint,
            token_program: *token_program,
            associated_token_program: *associated_token_program,
        };
        let Ok(mut cache) = self.associated_token_accounts.lock() else {
            return associated_token_address(wallet, mint, token_program, associated_token_program);
        };
        if let Some(pubkey) = cache.get(&key) {
            return *pubkey;
        }
        let pubkey =
            associated_token_address(wallet, mint, token_program, associated_token_program);
        cache.insert(key, pubkey);
        pubkey
    }

    fn user_volume_accumulator_address(&self, wallet: &Pubkey, pump_program: &Pubkey) -> Pubkey {
        let key = UserVolumeAccumulatorKey {
            wallet: *wallet,
            pump_program: *pump_program,
        };
        let Ok(mut cache) = self.user_volume_accumulators.lock() else {
            return user_volume_accumulator_address(wallet, pump_program);
        };
        if let Some(pubkey) = cache.get(&key) {
            return *pubkey;
        }
        let pubkey = user_volume_accumulator_address(wallet, pump_program);
        cache.insert(key, pubkey);
        pubkey
    }

    fn flashx_wrapped_sol_account_address(
        &self,
        wallet: &Pubkey,
        flashx_program: &Pubkey,
    ) -> (Pubkey, u8) {
        let key = FlashxWrappedSolAccountKey {
            wallet: *wallet,
            flashx_program: *flashx_program,
        };
        let Ok(mut cache) = self.flashx_wrapped_sol_accounts.lock() else {
            return flashx_wrapped_sol_account_address(wallet, flashx_program);
        };
        if let Some(account) = cache.get(&key) {
            return *account;
        }
        let account = flashx_wrapped_sol_account_address(wallet, flashx_program);
        cache.insert(key, account);
        account
    }
}

fn compute_unit_limit_instruction(units: u32) -> Result<Instruction, TxBuildError> {
    let mut data = Vec::with_capacity(5);
    data.push(2);
    data.extend_from_slice(&units.to_le_bytes());

    Ok(Instruction {
        program_id: *compute_budget_program_id(),
        accounts: Vec::new(),
        data,
    })
}

fn compute_unit_price_instruction(micro_lamports: u64) -> Result<Instruction, TxBuildError> {
    let mut data = Vec::with_capacity(9);
    data.push(3);
    data.extend_from_slice(&micro_lamports.to_le_bytes());

    Ok(Instruction {
        program_id: *compute_budget_program_id(),
        accounts: Vec::new(),
        data,
    })
}

fn system_transfer_instruction(
    from: &Pubkey,
    to: &Pubkey,
    lamports: u64,
) -> Result<Instruction, TxBuildError> {
    let mut data = Vec::with_capacity(12);
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&lamports.to_le_bytes());

    Ok(Instruction {
        program_id: *system_program_id(),
        accounts: vec![AccountMeta::new(*from, true), AccountMeta::new(*to, false)],
        data,
    })
}

fn close_token_account_instruction(
    token_account: &Pubkey,
    destination: &Pubkey,
    owner: &Pubkey,
    token_program: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: *token_program,
        accounts: vec![
            AccountMeta::new(*token_account, false),
            AccountMeta::new(*destination, false),
            AccountMeta::new_readonly(*owner, true),
        ],
        data: vec![9],
    }
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
        parse_trade, static_account_keys, versioned_tx_signature_string,
        ASSOCIATED_TOKEN_PROGRAM_ID, COMPUTE_BUDGET_PROGRAM_ID, FLASHX_ROUTER_PROGRAM_ID,
        PUMP_AMM_PROGRAM_ID, PUMP_FUN_PROGRAM_ID, SYSTEM_PROGRAM_ID, TOKEN_PROGRAM_ID,
    };
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use solana_transaction::versioned::VersionedTransaction;

    const TARGET_WALLET: &str = "A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS";
    const MIGRATED_BUY_SIGNATURE: &str =
        "Jo9sxcrorVCGkmafhNDQKByQBDBTSqM99tS9R1mYs6DjvFZHxZFuFhAvdSemCxFqauPcqS1t17ir3iDScu7cQF5";
    const LIVE_MIGRATED_BUY_SIGNATURE: &str =
        "hYCB3CXxuEw4aofMSiNoenDwGJ9u3XQq1TVXBsSk33TztaUmypa1B2aPZbM7s7dpkW5qeCE7rEEPVMYWczDW33k";
    const FAILED_AUTO_SELL_MIGRATED_BUY_SIGNATURE: &str =
        "5Zi3KWTX4b6RUK5xNvghWDADwqJ4df5V3wpT4W7vMMdTvnQ4Mz6ikErAReiZsWYVgedNfNxjesf2dbTxCmRSgUTn";
    const LIVE_DIRECT_PUMP_BUY_SIGNATURE: &str =
        "2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo";
    const COPY_WALLET: &str = "FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W";
    const LIVE_COPY_WALLET: &str = "4Nth9MdFpNmirGREn5Bjv2UZaGTmJQG77pu5NgjLdmZs";
    const LIVE_MIGRATED_MINT: &str = "J6UVkdPVe4cbd6qGJHdoacMa7zvN3tiaordcyZRspump";
    const FAILED_AUTO_SELL_MIGRATED_MINT: &str = "6tLxxZJRHT3YPkpCqzMXnRpSfPzDSiyWipNL47yCpump";
    const LIVE_DIRECT_PUMP_MINT: &str = "8VigmMkK7f9FvTBDd8S2UmweezCgeBX4y5Xp4jMfpump";
    const BURV_MINT: &str = "BurvgViVffsLv7sfuy7eqUXAk4S6YsBMh51FoaxDpump";
    const AA_J8_CASHBACK_MIGRATED_BUY_SIGNATURE: &str =
        "3CKL2NRZcBo1Nmwcs8p5Byy2no9gPsSL8AZ2SKijVddwjDjAJwnGrXqcrKGxuCtBDhjtEHEv1Lef3itCdoDFnfSe";
    const AA_J8_CASHBACK_MIGRATED_MINT: &str = "AaJ8TeBife3m1VzLmeuSFFKLmnRkdS26fza9rKaSpump";
    const AA_J8_TARGET_USER_VOLUME_ACCUMULATOR: &str =
        "82JokvYzsarTaVkeD2ecUnT3SewbyhStn9aZ4RQwxUs2";
    const AA_J8_COPY_USER_VOLUME_ACCUMULATOR: &str = "GXoVMJUAEemnj9jpstLYrTuq8hrVvEuF5gyniCHuqA76";
    const AA_J8_TARGET_USER_VOLUME_QUOTE_ATA: &str = "787mk8YMhMeUomgmWcewwkJ8EXREibKjtJdLBoQmt51X";
    const AA_J8_COPY_USER_VOLUME_QUOTE_ATA: &str = "2Y2t4M4G6zuELxtRQRWaj6Z2ZX7yVtKVw1a8Qyf9zqsD";
    const AA_J8_POOL_V2: &str = "EUYFztgbRC9VgrwRU5QGFuLi8PHxqrHuchPFhSNrcokt";

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

        let build = build_unsigned_flashx_pump(parsed.route_context.as_deref())
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

        let build = build_unsigned_flashx_pump(parsed.route_context.as_deref())
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
        let cloned_context = parsed
            .route_context
            .clone()
            .expect("direct Pump route context");
        let RouteContext::FlashxPump(original_context) = parsed
            .route_context
            .as_deref()
            .expect("direct Pump route context");
        let RouteContext::FlashxPump(cloned_context) = &*cloned_context;
        assert!(std::sync::Arc::ptr_eq(
            &original_context.accounts,
            &cloned_context.accounts
        ));
        assert!(std::sync::Arc::ptr_eq(
            &original_context.data,
            &cloned_context.data
        ));
        assert_eq!(
            original_context
                .direct_pump_accounts()
                .expect("direct Pump accounts")
                .router_amount,
            Some(990_000)
        );

        let build = build_copy_unsigned_flashx_pump(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
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
            build.copy_wallet_token_account.to_string(),
            "G2Bp3rC5GQHw8gWguLdujeZdTRoRgQia3Y1FmD5Ch4Vs"
        );
        assert!(build.instructions[0]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == COPY_WALLET && account.is_signer));
        assert!(build.instructions[0]
            .accounts
            .iter()
            .any(|account| account.pubkey == build.copy_wallet_token_account));
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
        assert_eq!(parsed.mint, pubkey(LIVE_DIRECT_PUMP_MINT));
    }

    #[test]
    fn builds_unsigned_copy_instructions_for_live_migrated_amm_with_copy_wallet_accounts() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/buy-hYCB3CXxuEw4aofMSiNoenDwGJ9u3XQq1TVXBsSk33TztaUmypa1B2aPZbM7s7dpkW5qeCE7rEEPVMYWczDW33k.tx.base64"
        )));
        assert_eq!(
            versioned_tx_signature_string(&transaction),
            LIVE_MIGRATED_BUY_SIGNATURE
        );
        let account_keys = live_migrated_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live migrated FLASHX buy should parse");
        let RouteContext::FlashxPump(context) = parsed
            .route_context
            .as_deref()
            .expect("live migrated FLASHX route context should parse");
        let observed_min_base_amount_out =
            read_u64_le(&context.data, 9).expect("fixture has migrated AMM min base amount");

        let build = build_copy_unsigned_flashx_pump(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
        )
        .expect("migrated AMM copy route should build unsigned instructions");

        assert_eq!(build.route_layout, "migrated-amm");
        assert_eq!(parsed.mint, pubkey(LIVE_MIGRATED_MINT));
        assert_eq!(build.instructions.len(), 2);
        assert_eq!(
            build.copy_wallet_token_account.to_string(),
            "C68p1PQWjCCbgoeApEAPnnB21bF3ccnv5yBrnFH7L3xz"
        );
        assert_eq!(
            build.instructions[0].program_id.to_string(),
            FLASHX_ROUTER_PROGRAM_ID
        );
        assert_eq!(build.instructions[0].accounts.len(), 5);
        assert_eq!(
            build.instructions[0].accounts[0].pubkey.to_string(),
            "6tY2JS9eKJJ2pAD7FwXaCh5dwPp88htikrGoUXMnuHxE"
        );
        assert_eq!(
            build.instructions[0].accounts[1].pubkey.to_string(),
            COPY_WALLET
        );
        assert!(build.instructions[0].accounts[1].is_signer);
        assert_eq!(
            &build.instructions[0].data[0..9],
            &[1, 0x30, 0x1b, 0x0f, 0, 0, 0, 0, 0]
        );
        assert_eq!(build.instructions[0].data[9], 0xff);
        assert_eq!(read_u64_le(&build.instructions[1].data, 1), Some(990_000));
        assert_eq!(
            read_u64_le(&build.instructions[1].data, 9),
            Some(observed_min_base_amount_out)
        );
        assert_eq!(build.instructions[1].accounts.len(), 44);
        assert!(build.instructions[1]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == COPY_WALLET && account.is_signer));
        assert!(build.instructions[1].accounts.iter().any(|account| {
            account.pubkey.to_string() == "6tY2JS9eKJJ2pAD7FwXaCh5dwPp88htikrGoUXMnuHxE"
                && account.is_writable
        }));
        assert!(build.instructions[1].accounts.iter().any(|account| {
            account.pubkey == build.copy_wallet_token_account && account.is_writable
        }));
        assert!(build.instructions[1].accounts.iter().any(|account| {
            account.pubkey.to_string() == "D6EMAgGqecPhW7t9r7LvCnRCiS6uADBwc3Ki1tpc2Bud"
                && account.is_writable
        }));

        let override_build = build_copy_unsigned_flashx_pump_with_cache_and_spend(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint,
            None,
            Some(777_000),
        )
        .expect("migrated AMM copy route should rewrite planned quote amount");
        assert_eq!(
            read_u64_le(&override_build.instructions[0].data, 1),
            Some(777_000)
        );
        assert_eq!(
            read_u64_le(&override_build.instructions[1].data, 1),
            Some(777_000)
        );
        assert_eq!(
            read_u64_le(&override_build.instructions[1].data, 9),
            Some(
                scaled_flashx_migrated_min_base_amount_out(&context.data, 777_000)
                    .expect("scaled min base amount should fit")
            )
        );

        assert!(!build.instructions[1]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == TARGET_WALLET));
        assert!(!build.instructions[1].accounts.iter().any(|account| {
            account.pubkey.to_string() == "9tby86mfM4eLbh52yeoHHywLqQug8sB2ppwDVvnt6sgq"
        }));
        assert!(!build.instructions[1].accounts.iter().any(|account| {
            account.pubkey.to_string() == "83DqVhmHb3RmZa8ieYC7VtHB5upyC5GHAr6g4WYfMjg4"
        }));
        assert!(!build.instructions[1].accounts.iter().any(|account| {
            account.pubkey.to_string() == "82JokvYzsarTaVkeD2ecUnT3SewbyhStn9aZ4RQwxUs2"
        }));
    }

    #[test]
    fn builds_cashback_migrated_amm_copy_with_copy_volume_quote_account() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/migrated-buy-3CKL2NRZcBo1Nmwcs8p5Byy2no9gPsSL8AZ2SKijVddwjDjAJwnGrXqcrKGxuCtBDhjtEHEv1Lef3itCdoDFnfSe.tx.base64"
        )));
        assert_eq!(
            versioned_tx_signature_string(&transaction),
            AA_J8_CASHBACK_MIGRATED_BUY_SIGNATURE
        );
        let account_keys = aa_j8_cashback_migrated_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("AaJ8 cashback migrated FLASHX buy should parse");
        let RouteContext::FlashxPump(context) = parsed
            .route_context
            .as_deref()
            .expect("AaJ8 cashback migrated FLASHX route context should parse");

        assert_eq!(parsed.mint, pubkey(AA_J8_CASHBACK_MIGRATED_MINT));
        assert_eq!(context.layout, FlashxPumpLayout::MigratedAmm);
        assert_eq!(
            resolved_account_for_test(context, "userVolumeAccumulatorQuoteTokenAccount"),
            AA_J8_TARGET_USER_VOLUME_QUOTE_ATA
        );
        assert_eq!(resolved_account_for_test(context, "poolV2"), AA_J8_POOL_V2);

        let build = build_copy_unsigned_flashx_pump(
            parsed.route_context.as_deref(),
            LIVE_COPY_WALLET,
            &parsed.mint.to_string(),
        )
        .expect("cashback migrated AMM copy route should build unsigned instructions");

        assert_eq!(build.route_layout, "migrated-amm");
        assert_eq!(build.instructions.len(), 2);
        assert_eq!(build.instructions[1].accounts.len(), 45);
        assert_eq!(
            build.instructions[1].accounts[32].pubkey.to_string(),
            AA_J8_COPY_USER_VOLUME_QUOTE_ATA
        );
        assert_eq!(
            build.instructions[1].accounts[33].pubkey.to_string(),
            AA_J8_POOL_V2
        );
        assert!(build.instructions[1].accounts.iter().any(|account| {
            account.pubkey.to_string() == AA_J8_COPY_USER_VOLUME_ACCUMULATOR && account.is_writable
        }));
        assert!(!build.instructions[1]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == TARGET_WALLET));
        assert!(!build.instructions[1]
            .accounts
            .iter()
            .any(|account| { account.pubkey.to_string() == AA_J8_TARGET_USER_VOLUME_ACCUMULATOR }));
        assert!(!build.instructions[1]
            .accounts
            .iter()
            .any(|account| { account.pubkey.to_string() == AA_J8_TARGET_USER_VOLUME_QUOTE_ATA }));
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
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
        )
        .expect("full copy transaction shell should build");

        assert_eq!(build.route_layout, "direct-pump");
        assert_eq!(
            build.copy_wallet_token_account.to_string(),
            "G2Bp3rC5GQHw8gWguLdujeZdTRoRgQia3Y1FmD5Ch4Vs"
        );
        assert_eq!(build.estimated_required_signer.to_string(), COPY_WALLET);
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
    fn planned_copy_spend_overrides_observed_flashx_sol_amount() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        let account_keys = live_direct_pump_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live direct Pump FLASHX buy should parse");

        let build = build_full_copy_unsigned_flashx_pump_with_fees_and_cache_and_spend(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
            &TxFeeConfig::default(),
            None,
            Some(777_000),
        )
        .expect("full copy transaction shell should build with planned spend override");

        assert_eq!(read_u64_le(&build.instructions[2].data, 8), Some(777_000));
    }

    #[test]
    fn fee_config_adds_priority_fee_and_jito_tip_to_copy_transaction_shell() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        let account_keys = live_direct_pump_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live direct Pump FLASHX buy should parse");
        let fee_config = TxFeeConfig {
            compute_unit_price_micro_lamports: Some(250_000),
            jito_tip_lamports: Some(1_000),
            jito_tip_account: Some("96gYZGLnUQYgE8MWWpYJw8yRjnvB51rAhbG1SogE3uSG".to_string()),
            helius_sender_tip_lamports: None,
            helius_sender_tip_account: None,
            nozomi_tip_lamports: None,
            nozomi_tip_account: None,
            bloxroute_tip_lamports: None,
            bloxroute_tip_account: None,
        };

        let build = build_full_copy_unsigned_flashx_pump_with_fees(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
            &fee_config,
        )
        .expect("full copy transaction shell should build with fee config");

        assert_eq!(build.setup_instruction_count, 4);
        assert_eq!(build.main_instruction_count, 1);
        assert_eq!(build.instructions.len(), 5);
        assert_eq!(
            build.instructions[0].program_id.to_string(),
            COMPUTE_BUDGET_PROGRAM_ID
        );
        assert_eq!(
            build.instructions[1].program_id.to_string(),
            COMPUTE_BUDGET_PROGRAM_ID
        );
        assert_eq!(
            build.instructions[1].data,
            [vec![3], 250_000u64.to_le_bytes().to_vec()].concat()
        );
        assert_eq!(
            build.instructions[2].program_id.to_string(),
            ASSOCIATED_TOKEN_PROGRAM_ID
        );
        assert_eq!(
            build.instructions[3].program_id.to_string(),
            SYSTEM_PROGRAM_ID
        );
        assert_eq!(
            build.instructions[3].accounts[0].pubkey.to_string(),
            COPY_WALLET
        );
        assert!(build.instructions[3].accounts[0].is_signer);
        assert_eq!(
            build.instructions[3].accounts[1].pubkey.to_string(),
            "96gYZGLnUQYgE8MWWpYJw8yRjnvB51rAhbG1SogE3uSG"
        );
        assert_eq!(
            build.instructions[3].data,
            [2u32.to_le_bytes().to_vec(), 1_000u64.to_le_bytes().to_vec()].concat()
        );
        assert_eq!(
            build.instructions[4].program_id.to_string(),
            PUMP_FUN_PROGRAM_ID
        );
    }

    #[test]
    fn fee_config_adds_helius_sender_tip_to_copy_transaction_shell() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        let account_keys = live_direct_pump_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live direct Pump FLASHX buy should parse");
        let fee_config = TxFeeConfig {
            compute_unit_price_micro_lamports: Some(250_000),
            jito_tip_lamports: Some(1_000),
            jito_tip_account: Some("96gYZGLnUQYgE8MWWpYJw8yRjnvB51rAhbG1SogE3uSG".to_string()),
            helius_sender_tip_lamports: Some(200_000),
            helius_sender_tip_account: Some(
                "HWEoBxYs7ssKuudEjzjmpfJVX7Dvi7wescFsVx2L5yoY".to_string(),
            ),
            nozomi_tip_lamports: None,
            nozomi_tip_account: None,
            bloxroute_tip_lamports: None,
            bloxroute_tip_account: None,
        };

        let build = build_full_copy_unsigned_flashx_pump_with_fees(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
            &fee_config,
        )
        .expect("full copy transaction shell should build with Jito and Sender tips");

        assert_eq!(build.setup_instruction_count, 5);
        assert_eq!(build.main_instruction_count, 1);
        assert_eq!(build.instructions.len(), 6);
        assert_eq!(
            build.instructions[3].accounts[1].pubkey.to_string(),
            "96gYZGLnUQYgE8MWWpYJw8yRjnvB51rAhbG1SogE3uSG"
        );
        assert_eq!(
            build.instructions[3].data,
            [2u32.to_le_bytes().to_vec(), 1_000u64.to_le_bytes().to_vec()].concat()
        );
        assert_eq!(
            build.instructions[4].accounts[1].pubkey.to_string(),
            "HWEoBxYs7ssKuudEjzjmpfJVX7Dvi7wescFsVx2L5yoY"
        );
        assert_eq!(
            build.instructions[4].data,
            [
                2u32.to_le_bytes().to_vec(),
                200_000u64.to_le_bytes().to_vec()
            ]
            .concat()
        );
        assert_eq!(
            build.instructions[5].program_id.to_string(),
            PUMP_FUN_PROGRAM_ID
        );
    }

    #[test]
    fn fee_config_merges_same_jito_and_helius_sender_tip_account() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        let account_keys = live_direct_pump_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live direct Pump FLASHX buy should parse");
        let tip_account = "96gYZGLnUQYgE8MWWpYJw8yRjnvB51rAhbG1SogE3uSG".to_string();
        let fee_config = TxFeeConfig {
            compute_unit_price_micro_lamports: None,
            jito_tip_lamports: Some(1_000),
            jito_tip_account: Some(tip_account.clone()),
            helius_sender_tip_lamports: Some(200_000),
            helius_sender_tip_account: Some(tip_account),
            nozomi_tip_lamports: None,
            nozomi_tip_account: None,
            bloxroute_tip_lamports: None,
            bloxroute_tip_account: None,
        };

        let build = build_full_copy_unsigned_flashx_pump_with_fees(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
            &fee_config,
        )
        .expect("same tip account should build one merged transfer");

        assert_eq!(build.setup_instruction_count, 3);
        assert_eq!(build.instructions.len(), 4);
        assert_eq!(
            build.instructions[3].program_id.to_string(),
            PUMP_FUN_PROGRAM_ID
        );
        assert_eq!(
            build.instructions[2].data,
            [
                2u32.to_le_bytes().to_vec(),
                200_000u64.to_le_bytes().to_vec()
            ]
            .concat()
        );
    }

    #[test]
    fn fee_config_adds_provider_stack_tips_to_one_transaction_shell() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        let account_keys = live_direct_pump_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live direct Pump FLASHX buy should parse");
        let jito_account = "96gYZGLnUQYgE8MWWpYJw8yRjnvB51rAhbG1SogE3uSG".to_string();
        let helius_account = "HWEoBxYs7ssKuudEjzjmpfJVX7Dvi7wescFsVx2L5yoY".to_string();
        let nozomi_account = "CwyufX5F8vP7gB5Xv8iYfLsCfQeQf9MStjGgYQhE6S9g".to_string();
        let fee_config = TxFeeConfig {
            compute_unit_price_micro_lamports: Some(250_000),
            jito_tip_lamports: Some(1_000),
            jito_tip_account: Some(jito_account.clone()),
            helius_sender_tip_lamports: Some(200_000),
            helius_sender_tip_account: Some(helius_account.clone()),
            nozomi_tip_lamports: Some(1_000_000),
            nozomi_tip_account: Some(nozomi_account.clone()),
            bloxroute_tip_lamports: Some(1_250_000),
            bloxroute_tip_account: Some(nozomi_account.clone()),
        };

        let build = build_full_copy_unsigned_flashx_pump_with_fees(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
            &fee_config,
        )
        .expect("provider-stack transaction shell should build");

        assert_eq!(build.setup_instruction_count, 6);
        assert_eq!(build.main_instruction_count, 1);
        assert_eq!(build.instructions.len(), 7);
        assert_eq!(build.instructions[3].accounts[1].pubkey.to_string(), jito_account);
        assert_eq!(
            build.instructions[4].accounts[1].pubkey.to_string(),
            helius_account
        );
        assert_eq!(
            build.instructions[5].accounts[1].pubkey.to_string(),
            nozomi_account
        );
        assert_eq!(
            build.instructions[5].data,
            [
                2u32.to_le_bytes().to_vec(),
                1_250_000u64.to_le_bytes().to_vec()
            ]
            .concat()
        );
        assert_eq!(
            build.instructions[6].program_id.to_string(),
            PUMP_FUN_PROGRAM_ID
        );
    }

    #[test]
    fn positive_jito_tip_requires_tip_account() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        let account_keys = live_direct_pump_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live direct Pump FLASHX buy should parse");
        let fee_config = TxFeeConfig {
            compute_unit_price_micro_lamports: None,
            jito_tip_lamports: Some(1_000),
            jito_tip_account: None,
            helius_sender_tip_lamports: None,
            helius_sender_tip_account: None,
            nozomi_tip_lamports: None,
            nozomi_tip_account: None,
            bloxroute_tip_lamports: None,
            bloxroute_tip_account: None,
        };

        let error = build_full_copy_unsigned_flashx_pump_with_fees(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
            &fee_config,
        )
        .expect_err("tip account is required when tip lamports are configured");

        assert_eq!(
            error,
            TxBuildError::MissingRouteContext("missing Jito tip account")
        );
    }

    #[test]
    fn builds_full_unsigned_copy_transaction_shell_for_live_migrated_amm() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/buy-hYCB3CXxuEw4aofMSiNoenDwGJ9u3XQq1TVXBsSk33TztaUmypa1B2aPZbM7s7dpkW5qeCE7rEEPVMYWczDW33k.tx.base64"
        )));
        let account_keys = live_migrated_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live migrated FLASHX buy should parse");

        let build = build_full_copy_unsigned_flashx_pump(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
        )
        .expect("full migrated AMM copy transaction shell should build");

        assert_eq!(build.route_layout, "migrated-amm");
        assert_eq!(
            build.copy_wallet_token_account.to_string(),
            "C68p1PQWjCCbgoeApEAPnnB21bF3ccnv5yBrnFH7L3xz"
        );
        assert_eq!(build.estimated_required_signer.to_string(), COPY_WALLET);
        assert_eq!(build.setup_instruction_count, 2);
        assert_eq!(build.main_instruction_count, 2);
        assert_eq!(build.instructions.len(), 4);
        assert_eq!(
            build.instructions[0].program_id.to_string(),
            COMPUTE_BUDGET_PROGRAM_ID
        );
        assert_eq!(
            build.instructions[1].program_id.to_string(),
            ASSOCIATED_TOKEN_PROGRAM_ID
        );
        assert_eq!(
            build.instructions[2].program_id.to_string(),
            FLASHX_ROUTER_PROGRAM_ID
        );
        assert_eq!(
            build.instructions[3].program_id.to_string(),
            FLASHX_ROUTER_PROGRAM_ID
        );
    }

    #[test]
    fn builds_direct_pump_auto_sell_from_buy_side_context() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        let account_keys = live_direct_pump_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live direct Pump FLASHX buy should parse");

        let build = build_auto_sell_unsigned_flashx_pump_with_cache(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
            123_456,
            None,
        )
        .expect("direct Pump auto-sell should build from buy-side route context");

        assert_eq!(build.route_layout, "direct-pump");
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
        let RouteContext::FlashxPump(context) = parsed
            .route_context
            .as_deref()
            .expect("direct Pump route context");
        assert_eq!(
            build.instructions[1].accounts[12].pubkey,
            context.resolved_pubkey("feeConfig").expect("fee config")
        );
        assert_eq!(
            build.instructions[1].accounts[13].pubkey,
            context.resolved_pubkey("feeProgram").expect("fee program")
        );
        assert_eq!(
            build.instructions[1].accounts[14].pubkey,
            context
                .resolved_pubkey("bondingCurveV2")
                .expect("bonding curve v2")
        );
        assert_eq!(
            build.instructions[1].accounts[15].pubkey,
            context
                .resolved_pubkey("buybackFeeRecipient")
                .expect("buyback fee recipient")
        );
        assert!(build.instructions[1]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == COPY_WALLET && account.is_signer));
    }

    #[test]
    fn builds_trailing_sell_instruction_for_direct_pump_buy_side_context() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/live-buy-2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo.tx.base64"
        )));
        let account_keys = live_direct_pump_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live direct Pump FLASHX buy should parse");

        let build = build_trailing_sell_unsigned_flashx_pump_with_fees_and_cache(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
            123_456,
            &TxFeeConfig::default(),
            None,
        )
        .expect("direct Pump trailing sell should build from buy-side route context");

        assert_eq!(build.route_layout, "direct-pump");
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
        assert!(build.instructions[1]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == COPY_WALLET && account.is_signer));
    }

    #[test]
    fn builds_auto_sell_instruction_for_direct_pump_sell_side_context() {
        let route_context = burv_direct_pump_sell_route_context();
        let RouteContext::FlashxPump(context) = &route_context;

        let build = build_auto_sell_unsigned_flashx_pump_with_cache(
            Some(&route_context),
            COPY_WALLET,
            BURV_MINT,
            123_456,
            None,
        )
        .expect("auto-sell route should build from sell-side context");

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
        assert_eq!(
            &build.instructions[1].data[16..24],
            &DIRECT_PUMP_COPY_MIN_SOL_OUT.to_le_bytes()
        );
        assert!(build.instructions[1]
            .accounts
            .iter()
            .any(|account| account.pubkey.to_string() == COPY_WALLET && account.is_signer));
        assert!(build.instructions[1].accounts.iter().any(|account| {
            account.pubkey.to_string() == "5RGtdhAeLrhqdgEFsa4jR9xpt6Y4Bk9rTQzNTFAjMnho"
                && account.is_writable
        }));
        assert!(build.instructions[1].accounts.iter().any(|account| {
            account.pubkey.to_string() == resolved_account_for_test(context, "feeRecipient")
        }));
        assert!(build.instructions[1].accounts.iter().any(|account| {
            account.pubkey.to_string() == resolved_account_for_test(context, "bondingCurveV2")
        }));
    }

    #[test]
    fn builds_auto_sell_instruction_for_migrated_amm_copy_balance() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/buy-hYCB3CXxuEw4aofMSiNoenDwGJ9u3XQq1TVXBsSk33TztaUmypa1B2aPZbM7s7dpkW5qeCE7rEEPVMYWczDW33k.tx.base64"
        )));
        let account_keys = live_migrated_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("live migrated FLASHX buy should parse");

        let build = build_auto_sell_unsigned_flashx_pump_with_cache(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
            123_456,
            None,
        )
        .expect("migrated AMM auto-sell route should build");

        assert_eq!(build.route_layout, "migrated-amm");
        assert_eq!(
            build.copy_wallet_token_account.to_string(),
            "C68p1PQWjCCbgoeApEAPnnB21bF3ccnv5yBrnFH7L3xz"
        );
        assert_eq!(build.setup_instruction_count, 2);
        assert_eq!(build.main_instruction_count, 2);
        assert_eq!(build.instructions.len(), 4);
        assert_eq!(
            build.instructions[0].program_id.to_string(),
            COMPUTE_BUDGET_PROGRAM_ID
        );
        assert_eq!(
            build.instructions[1].program_id.to_string(),
            ASSOCIATED_TOKEN_PROGRAM_ID
        );
        assert_eq!(
            build.instructions[1].accounts[1].pubkey.to_string(),
            "8znPDiS1XkMMwHM7pJwCofbieMSLGqbZ1xPmeWB2wn9z"
        );
        assert_eq!(
            build.instructions[2].program_id.to_string(),
            PUMP_AMM_PROGRAM_ID
        );
        assert_eq!(build.instructions[2].accounts.len(), 24);
        assert_eq!(
            &build.instructions[2].data[0..8],
            &PUMP_FUN_SELL_DISCRIMINATOR
        );
        assert_eq!(
            &build.instructions[2].data[8..16],
            &123_456u64.to_le_bytes()
        );
        assert_eq!(&build.instructions[2].data[16..24], &0u64.to_le_bytes());
        assert_eq!(
            build.instructions[2].accounts[1].pubkey.to_string(),
            COPY_WALLET
        );
        assert!(build.instructions[2].accounts[1].is_signer);
        assert_eq!(
            build.instructions[2].accounts[5].pubkey,
            build.copy_wallet_token_account
        );
        assert_eq!(
            build.instructions[2].accounts[6].pubkey.to_string(),
            "8znPDiS1XkMMwHM7pJwCofbieMSLGqbZ1xPmeWB2wn9z"
        );
        assert_eq!(
            build.instructions[2].accounts[10].pubkey.to_string(),
            "BWXT6RUhit9FfJQM3pBmqeFLPYmuxgmyhMGC5sGr8RbA"
        );
        assert_eq!(
            build.instructions[2].accounts[17].pubkey.to_string(),
            "CQAM6AwSFEcJMiRgkmwcB5FbzBRvzqUzryLFrQZxrGf7"
        );
        assert_eq!(
            build.instructions[2].accounts[18].pubkey.to_string(),
            "6yeW4pQsvXTNzd6F3L8kbMFDf3WDV8i2jRETNspH2A72"
        );
        assert_eq!(
            build.instructions[2].accounts[19].pubkey.to_string(),
            "5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx"
        );
        assert_eq!(
            build.instructions[3].program_id.to_string(),
            TOKEN_PROGRAM_ID
        );
        assert_eq!(build.instructions[3].data, vec![9]);
        assert_eq!(
            build.instructions[3].accounts[0].pubkey.to_string(),
            "8znPDiS1XkMMwHM7pJwCofbieMSLGqbZ1xPmeWB2wn9z"
        );
        assert_eq!(
            build.instructions[3].accounts[1].pubkey.to_string(),
            COPY_WALLET
        );
        assert_eq!(
            build.instructions[3].accounts[2].pubkey.to_string(),
            COPY_WALLET
        );
        assert!(build.instructions[3].accounts[2].is_signer);
        assert!(!build
            .instructions
            .iter()
            .flat_map(|instruction| instruction.accounts.iter())
            .any(|account| account.pubkey.to_string() == TARGET_WALLET));
    }

    #[test]
    fn builds_auto_sell_instruction_for_failed_live_migrated_buyback_layout() {
        let transaction = replay_transaction(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/flashx/migrated-buy-5Zi3KWTX4b6RUK5xNvghWDADwqJ4df5V3wpT4W7vMMdTvnQ4Mz6ikErAReiZsWYVgedNfNxjesf2dbTxCmRSgUTn.tx.base64"
        )));
        assert_eq!(
            versioned_tx_signature_string(&transaction),
            FAILED_AUTO_SELL_MIGRATED_BUY_SIGNATURE
        );
        let account_keys = failed_auto_sell_migrated_buy_hydrated_account_keys(&transaction);
        let parsed = parse_trade(&transaction, &account_keys, &[TARGET_WALLET.to_string()])
            .expect("failed live migrated FLASHX buy should parse");
        assert_eq!(parsed.mint, pubkey(FAILED_AUTO_SELL_MIGRATED_MINT));

        let build = build_auto_sell_unsigned_flashx_pump_with_cache(
            parsed.route_context.as_deref(),
            COPY_WALLET,
            &parsed.mint.to_string(),
            32_212_701_563,
            None,
        )
        .expect("failed migrated AMM auto-sell route should build");

        assert_eq!(build.route_layout, "migrated-amm");
        assert_eq!(
            build.copy_wallet_token_account.to_string(),
            "D2yXA2HXMpxY98fHENhDkBzY9JDwJ4EcjcdCtypy8LiN"
        );
        assert_eq!(
            build.instructions[2].program_id.to_string(),
            PUMP_AMM_PROGRAM_ID
        );
        assert_eq!(build.instructions[2].accounts.len(), 24);
        assert_eq!(
            &build.instructions[2].data[8..16],
            &32_212_701_563u64.to_le_bytes()
        );
        assert_eq!(
            build.instructions[2].accounts[21].pubkey.to_string(),
            "8L81oN2mPTHQ9LJXxYcysggHXy5dfuSZP3yVe1tfzHeK"
        );
        assert!(!build.instructions[2].accounts[21].is_writable);
        assert_eq!(
            build.instructions[2].accounts[22].pubkey.to_string(),
            "GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL"
        );
        assert!(build.instructions[2].accounts[22].is_writable);
        assert_eq!(
            build.instructions[2].accounts[23].pubkey.to_string(),
            "AktftA98kSWAxn6kVSoqBXBELUArjKu2H9WmKB48ULFY"
        );
        assert!(build.instructions[2].accounts[23].is_writable);
    }

    fn replay_transaction(base64_fixture: &str) -> VersionedTransaction {
        let compact = base64_fixture.split_whitespace().collect::<String>();
        let bytes = STANDARD.decode(compact).expect("fixture is valid base64");
        bincode::deserialize(&bytes).expect("fixture decodes as a VersionedTransaction")
    }

    fn resolved_account_for_test(
        context: &crate::parser::FlashxPumpRouteContext,
        role: &'static str,
    ) -> String {
        context
            .resolved_pubkey(role)
            .map(|pubkey| pubkey.to_string())
            .unwrap_or_else(|| panic!("missing resolved account role {role}"))
    }

    fn migrated_buy_hydrated_account_keys(transaction: &VersionedTransaction) -> Vec<Pubkey> {
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
            .map(pubkey),
        );
        account_keys
    }

    fn live_direct_pump_buy_hydrated_account_keys(
        transaction: &VersionedTransaction,
    ) -> Vec<Pubkey> {
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
            .map(pubkey),
        );
        account_keys
    }

    fn live_migrated_buy_hydrated_account_keys(transaction: &VersionedTransaction) -> Vec<Pubkey> {
        let mut account_keys = static_account_keys(transaction);
        account_keys.extend(
            [
                "DKyUs1xXMDy8Z11zNsLnUg3dy9HZf6hYZidB6WodcaGy",
                "BWXT6RUhit9FfJQM3pBmqeFLPYmuxgmyhMGC5sGr8RbA",
                "AktftA98kSWAxn6kVSoqBXBELUArjKu2H9WmKB48ULFY",
                "5vPNE6VFyXmCmzmWotdxmRk57LEWiXxuAfZL3hKbi2LH",
                "86Vh4XGLW2b6nvWbRyDs4ScgMXbuvRCHT7WbUT3RFxKG",
                "jitodontfront81111111TradeWithAxiomDotTrade",
                "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
                "So11111111111111111111111111111111111111112",
                "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA",
                "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw",
                "G5UZAVbAf46s7cKWoyKu8kYTip9DGTpbLZ2qa9Aq69dP",
                "GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR",
                "C2aFPdENg4A2HQsmrd5rTw5TaYBX5Ku887cWjbFKtZpw",
                "5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx",
                "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ",
                "DKU4HtLZmD825BXdsGkE2bSEC6kunPeUyrgvB9DcawGV",
                "GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL",
            ]
            .into_iter()
            .map(pubkey),
        );
        account_keys
    }

    fn aa_j8_cashback_migrated_buy_hydrated_account_keys(
        transaction: &VersionedTransaction,
    ) -> Vec<Pubkey> {
        let mut account_keys = static_account_keys(transaction);
        account_keys.extend(
            [
                "76sxKrPtgoJHDJvxwFHqb3cAXWfRHFLe3VpKcLCAHSEf",
                "Bvtgim23rfocUzxVX9j9QFxTbBnH8JZxnaGLCEkXvjKS",
                "CA7v8gHfbquYXyDnDx6QxWW8hmL1H7X6Y2RYDrGLnuck",
                "3Tu1Y9aNveLFN4WTAwnAwXL6tbUp5MMe3RxyybG4jTAS",
                "DZfEurFKFtSbdWZsKSDTqpqsQgvXxmESpvRtXkAdgLwM",
                "jitodontfront81111111TradeWithAxiomDotTrade",
                "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
                "So11111111111111111111111111111111111111112",
                "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA",
                "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw",
                "9rPYyANsfQZw3DnDmKE3YCQF5E8oD89UXoHn9JFEhJUz",
                "GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR",
                "C2aFPdENg4A2HQsmrd5rTw5TaYBX5Ku887cWjbFKtZpw",
                "5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx",
                "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ",
                "EHAAiTxcdDwQ3U4bU6YcMsQGaekdzLS3B5SmYo46kJtL",
            ]
            .into_iter()
            .map(pubkey),
        );
        account_keys
    }

    fn failed_auto_sell_migrated_buy_hydrated_account_keys(
        transaction: &VersionedTransaction,
    ) -> Vec<Pubkey> {
        let mut account_keys = static_account_keys(transaction);
        account_keys.extend(
            [
                "7oi1L8U9MRu5zDz5syFahsiLUric47LzvJBQX6r827ws",
                "Bvtgim23rfocUzxVX9j9QFxTbBnH8JZxnaGLCEkXvjKS",
                "AktftA98kSWAxn6kVSoqBXBELUArjKu2H9WmKB48ULFY",
                "3PvqoztjnRxaAiFmLuEfqZkU4GSbjUareks8S2xCZaTa",
                "8vFGAKdwpn4hk7kc1cBgfWZzpyW3MEMDATDzVZhddeQb",
                "jitodontfront81111111TradeWithAxiomDotTrade",
                "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
                "So11111111111111111111111111111111111111112",
                "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA",
                "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw",
                "9rPYyANsfQZw3DnDmKE3YCQF5E8oD89UXoHn9JFEhJUz",
                "GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR",
                "C2aFPdENg4A2HQsmrd5rTw5TaYBX5Ku887cWjbFKtZpw",
                "5PHirr8joyTMp9JMm6nW7hNDVyEYdkzDqazxPD7RaTjx",
                "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ",
                "GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL",
            ]
            .into_iter()
            .map(pubkey),
        );
        account_keys
    }

    fn pubkey(value: &str) -> Pubkey {
        Pubkey::from_str(value).expect("fixture pubkey is valid")
    }

    fn burv_direct_pump_sell_route_context() -> RouteContext {
        let flashx_router_program = pubkey(FLASHX_ROUTER_PROGRAM_ID);
        let pump_program = pubkey(PUMP_FUN_PROGRAM_ID);
        let mut data = vec![0];
        data.extend_from_slice(&34_968_346_045u64.to_le_bytes());
        data.extend_from_slice(&579_882u64.to_le_bytes());
        data.extend_from_slice(&[1, 1, 0x1a, 0x32, 0]);

        RouteContext::FlashxPump(crate::parser::FlashxPumpRouteContext {
            layout: FlashxPumpLayout::DirectPump,
            program_id: flashx_router_program,
            accounts: vec![
                route_account("Bc8P1uc9nc7fgq5Z6yEvBM8ccXx9W4fEbqhwX9qnSQ84", true, false),
                route_account(TARGET_WALLET, true, true),
                route_account(BURV_MINT, false, false),
                route_account("CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM", true, false),
                route_account(FLASHX_ROUTER_PROGRAM_ID, false, false),
                route_account(PUMP_FUN_PROGRAM_ID, false, false),
                route_account("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf", false, false),
                route_account("4p4fH6hAMjSwQV5oWLD4TxieQiciZHBoGL7m7vjRovE9", true, false),
                route_account("He1PZZtQ3LRYe4bQKVH7XNtAcxUbs38mDkgZL58MUP8V", true, false),
                route_account(SYSTEM_PROGRAM_ID, false, false),
                route_account("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb", false, false),
                route_account("82L33tWkcKBcXFPyUiLJtRmnGomhCzyitELKNoXrJ2T", true, false),
                route_account("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1", false, false),
                route_account("8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt", false, false),
                route_account("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ", false, false),
                route_account("EYyZtDHLiBnLLw9u4z7uLrUoom3ZkrZugmhmqw48mNh6", false, false),
                route_account("5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD", true, false),
                route_account("5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD", true, false),
            ]
            .into(),
            data: data.into(),
            resolved_accounts: crate::parser::FlashxPumpResolvedAccounts::DirectPump(
                crate::parser::DirectPumpAccounts {
                    payer: pubkey(TARGET_WALLET),
                    target_wallet: pubkey(TARGET_WALLET),
                    flashx_router_program,
                    pump_program,
                    global_config: pubkey("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf"),
                    fee_recipient: pubkey("CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM"),
                    mint: pubkey(BURV_MINT),
                    bonding_curve: pubkey("4p4fH6hAMjSwQV5oWLD4TxieQiciZHBoGL7m7vjRovE9"),
                    associated_bonding_curve: pubkey(
                        "He1PZZtQ3LRYe4bQKVH7XNtAcxUbs38mDkgZL58MUP8V",
                    ),
                    user_token_account: pubkey("Bc8P1uc9nc7fgq5Z6yEvBM8ccXx9W4fEbqhwX9qnSQ84"),
                    system_program: pubkey(SYSTEM_PROGRAM_ID),
                    token_program: pubkey("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"),
                    creator_vault: pubkey("82L33tWkcKBcXFPyUiLJtRmnGomhCzyitELKNoXrJ2T"),
                    event_authority: pubkey("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1"),
                    global_volume_accumulator: None,
                    user_volume_accumulator: None,
                    fee_config: pubkey("8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt"),
                    fee_program: pubkey("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ"),
                    bonding_curve_v2: pubkey("EYyZtDHLiBnLLw9u4z7uLrUoom3ZkrZugmhmqw48mNh6"),
                    buyback_fee_recipient: pubkey("5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD"),
                    buyback_fee_recipient_token_account: Some(pubkey(
                        "5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD",
                    )),
                    router_amount: Some(34_968_346_045),
                },
            ),
        })
    }

    fn route_account(
        value: &str,
        is_writable: bool,
        is_signer: bool,
    ) -> crate::parser::RouteInstructionAccount {
        crate::parser::RouteInstructionAccount {
            pubkey: pubkey(value),
            is_writable,
            is_signer,
        }
    }
}
