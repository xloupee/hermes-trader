use serde::Serialize;
use solana_message::{compiled_instruction::CompiledInstruction, VersionedMessage};
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;
use std::{collections::HashSet, str::FromStr, sync::OnceLock};

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
    pub(crate) program_id: Pubkey,
    pub(crate) accounts: Vec<RouteInstructionAccount>,
    pub(crate) data: Vec<u8>,
    pub(crate) resolved_accounts: FlashxPumpResolvedAccounts,
}

#[derive(Clone, Debug)]
pub(crate) enum FlashxPumpResolvedAccounts {
    MigratedAmm(MigratedAmmAccounts),
    DirectPump(DirectPumpAccounts),
}

#[derive(Clone, Debug)]
pub(crate) struct MigratedAmmAccounts {
    pub(crate) payer: Pubkey,
    pub(crate) target_wallet: Pubkey,
    pub(crate) flashx_router_program: Pubkey,
    pub(crate) pump_amm_program: Pubkey,
    pub(crate) pool_state: Pubkey,
    pub(crate) global_config: Pubkey,
    pub(crate) mint: Pubkey,
    pub(crate) quote_mint: Pubkey,
    pub(crate) user_base_token_account: Pubkey,
    pub(crate) user_quote_token_account: Pubkey,
    pub(crate) pool_base_token_account: Pubkey,
    pub(crate) pool_quote_token_account: Pubkey,
    pub(crate) protocol_fee_recipient: Pubkey,
    pub(crate) protocol_fee_recipient_token_account: Pubkey,
    pub(crate) base_token_program: Pubkey,
    pub(crate) quote_token_program: Pubkey,
    pub(crate) system_program: Pubkey,
    pub(crate) associated_token_program: Pubkey,
    pub(crate) event_authority: Pubkey,
    pub(crate) coin_creator_vault_ata: Pubkey,
    pub(crate) coin_creator_vault_authority: Pubkey,
    pub(crate) global_volume_accumulator: Pubkey,
    pub(crate) user_volume_accumulator: Pubkey,
    pub(crate) fee_config: Pubkey,
    pub(crate) fee_program: Pubkey,
    pub(crate) pool_v2: Option<Pubkey>,
    pub(crate) buyback_fee_recipient: Option<Pubkey>,
    pub(crate) buyback_fee_recipient_token_account: Option<Pubkey>,
}

#[derive(Clone, Debug)]
pub(crate) struct DirectPumpAccounts {
    pub(crate) payer: Pubkey,
    pub(crate) target_wallet: Pubkey,
    pub(crate) flashx_router_program: Pubkey,
    pub(crate) pump_program: Pubkey,
    pub(crate) global_config: Pubkey,
    pub(crate) fee_recipient: Pubkey,
    pub(crate) mint: Pubkey,
    pub(crate) bonding_curve: Pubkey,
    pub(crate) associated_bonding_curve: Pubkey,
    pub(crate) user_token_account: Pubkey,
    pub(crate) system_program: Pubkey,
    pub(crate) token_program: Pubkey,
    pub(crate) creator_vault: Pubkey,
    pub(crate) event_authority: Pubkey,
    pub(crate) global_volume_accumulator: Option<Pubkey>,
    pub(crate) user_volume_accumulator: Option<Pubkey>,
    pub(crate) fee_config: Pubkey,
    pub(crate) fee_program: Pubkey,
    pub(crate) bonding_curve_v2: Pubkey,
    pub(crate) buyback_fee_recipient: Pubkey,
    pub(crate) buyback_fee_recipient_token_account: Option<Pubkey>,
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
    pub(crate) pubkey: Pubkey,
    pub(crate) is_signer: bool,
    pub(crate) is_writable: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedRouteAccount {
    pub(crate) role: &'static str,
    pub(crate) pubkey: Pubkey,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedRouteAccountJson {
    pub(crate) role: &'static str,
    pub(crate) pubkey: String,
}

impl FlashxPumpRouteContext {
    pub(crate) fn resolved_pubkey(&self, role: &'static str) -> Option<Pubkey> {
        self.resolved_accounts.resolved_pubkey(role)
    }

    pub(crate) fn resolved_accounts_for_json(&self) -> Vec<ResolvedRouteAccountJson> {
        self.resolved_accounts
            .resolved_accounts()
            .into_iter()
            .map(|account| ResolvedRouteAccountJson {
                role: account.role,
                pubkey: account.pubkey.to_string(),
            })
            .collect()
    }
}

impl FlashxPumpResolvedAccounts {
    fn resolved_pubkey(&self, role: &'static str) -> Option<Pubkey> {
        match self {
            Self::DirectPump(accounts) => accounts.resolved_pubkey(role),
            Self::MigratedAmm(accounts) => accounts.resolved_pubkey(role),
        }
    }

    fn resolved_accounts(&self) -> Vec<ResolvedRouteAccount> {
        match self {
            Self::DirectPump(accounts) => accounts.resolved_accounts(),
            Self::MigratedAmm(accounts) => accounts.resolved_accounts(),
        }
    }
}

impl DirectPumpAccounts {
    fn resolved_pubkey(&self, role: &'static str) -> Option<Pubkey> {
        Some(match role {
            "payer" => self.payer,
            "targetWallet" => self.target_wallet,
            "flashxRouterProgram" => self.flashx_router_program,
            "pumpProgram" => self.pump_program,
            "globalConfig" => self.global_config,
            "feeRecipient" => self.fee_recipient,
            "mint" => self.mint,
            "bondingCurve" => self.bonding_curve,
            "associatedBondingCurve" => self.associated_bonding_curve,
            "userTokenAccount" => self.user_token_account,
            "systemProgram" => self.system_program,
            "tokenProgram" => self.token_program,
            "creatorVault" => self.creator_vault,
            "eventAuthority" => self.event_authority,
            "globalVolumeAccumulator" => self.global_volume_accumulator?,
            "userVolumeAccumulator" => self.user_volume_accumulator?,
            "feeConfig" => self.fee_config,
            "feeProgram" => self.fee_program,
            "bondingCurveV2" => self.bonding_curve_v2,
            "buybackFeeRecipient" => self.buyback_fee_recipient,
            "buybackFeeRecipientTokenAccount" => self.buyback_fee_recipient_token_account?,
            _ => return None,
        })
    }

    fn resolved_accounts(&self) -> Vec<ResolvedRouteAccount> {
        let mut accounts = vec![
            resolved("payer", self.payer),
            resolved("targetWallet", self.target_wallet),
            resolved("flashxRouterProgram", self.flashx_router_program),
            resolved("pumpProgram", self.pump_program),
            resolved("globalConfig", self.global_config),
            resolved("feeRecipient", self.fee_recipient),
            resolved("mint", self.mint),
            resolved("bondingCurve", self.bonding_curve),
            resolved("associatedBondingCurve", self.associated_bonding_curve),
            resolved("userTokenAccount", self.user_token_account),
            resolved("systemProgram", self.system_program),
            resolved("tokenProgram", self.token_program),
            resolved("creatorVault", self.creator_vault),
            resolved("eventAuthority", self.event_authority),
            resolved("feeConfig", self.fee_config),
            resolved("feeProgram", self.fee_program),
            resolved("bondingCurveV2", self.bonding_curve_v2),
            resolved("buybackFeeRecipient", self.buyback_fee_recipient),
        ];
        if let Some(pubkey) = self.global_volume_accumulator {
            accounts.insert(14, resolved("globalVolumeAccumulator", pubkey));
        }
        if let Some(pubkey) = self.user_volume_accumulator {
            let index = if self.global_volume_accumulator.is_some() {
                15
            } else {
                14
            };
            accounts.insert(index, resolved("userVolumeAccumulator", pubkey));
        }
        if let Some(pubkey) = self.buyback_fee_recipient_token_account {
            accounts.push(resolved("buybackFeeRecipientTokenAccount", pubkey));
        }
        accounts
    }
}

impl MigratedAmmAccounts {
    fn resolved_pubkey(&self, role: &'static str) -> Option<Pubkey> {
        Some(match role {
            "payer" => self.payer,
            "targetWallet" => self.target_wallet,
            "flashxRouterProgram" => self.flashx_router_program,
            "pumpAmmProgram" => self.pump_amm_program,
            "poolState" => self.pool_state,
            "globalConfig" => self.global_config,
            "mint" => self.mint,
            "quoteMint" => self.quote_mint,
            "userBaseTokenAccount" => self.user_base_token_account,
            "userQuoteTokenAccount" => self.user_quote_token_account,
            "poolBaseTokenAccount" => self.pool_base_token_account,
            "poolQuoteTokenAccount" => self.pool_quote_token_account,
            "protocolFeeRecipient" => self.protocol_fee_recipient,
            "protocolFeeRecipientTokenAccount" => self.protocol_fee_recipient_token_account,
            "baseTokenProgram" => self.base_token_program,
            "quoteTokenProgram" => self.quote_token_program,
            "systemProgram" => self.system_program,
            "associatedTokenProgram" => self.associated_token_program,
            "eventAuthority" => self.event_authority,
            "coinCreatorVaultAta" => self.coin_creator_vault_ata,
            "coinCreatorVaultAuthority" => self.coin_creator_vault_authority,
            "globalVolumeAccumulator" => self.global_volume_accumulator,
            "userVolumeAccumulator" => self.user_volume_accumulator,
            "feeConfig" => self.fee_config,
            "feeProgram" => self.fee_program,
            "poolV2" => self.pool_v2?,
            "buybackFeeRecipient" => self.buyback_fee_recipient?,
            "buybackFeeRecipientTokenAccount" => self.buyback_fee_recipient_token_account?,
            _ => return None,
        })
    }

    fn resolved_accounts(&self) -> Vec<ResolvedRouteAccount> {
        let mut accounts = vec![
            resolved("payer", self.payer),
            resolved("targetWallet", self.target_wallet),
            resolved("flashxRouterProgram", self.flashx_router_program),
            resolved("pumpAmmProgram", self.pump_amm_program),
            resolved("poolState", self.pool_state),
            resolved("globalConfig", self.global_config),
            resolved("mint", self.mint),
            resolved("quoteMint", self.quote_mint),
            resolved("userBaseTokenAccount", self.user_base_token_account),
            resolved("userQuoteTokenAccount", self.user_quote_token_account),
            resolved("poolBaseTokenAccount", self.pool_base_token_account),
            resolved("poolQuoteTokenAccount", self.pool_quote_token_account),
            resolved("protocolFeeRecipient", self.protocol_fee_recipient),
            resolved(
                "protocolFeeRecipientTokenAccount",
                self.protocol_fee_recipient_token_account,
            ),
            resolved("baseTokenProgram", self.base_token_program),
            resolved("quoteTokenProgram", self.quote_token_program),
            resolved("systemProgram", self.system_program),
            resolved("associatedTokenProgram", self.associated_token_program),
            resolved("eventAuthority", self.event_authority),
            resolved("coinCreatorVaultAta", self.coin_creator_vault_ata),
            resolved(
                "coinCreatorVaultAuthority",
                self.coin_creator_vault_authority,
            ),
            resolved("globalVolumeAccumulator", self.global_volume_accumulator),
            resolved("userVolumeAccumulator", self.user_volume_accumulator),
            resolved("feeConfig", self.fee_config),
            resolved("feeProgram", self.fee_program),
        ];
        if let Some(pubkey) = self.pool_v2 {
            accounts.push(resolved("poolV2", pubkey));
        }
        if let Some(pubkey) = self.buyback_fee_recipient {
            accounts.push(resolved("buybackFeeRecipient", pubkey));
        }
        if let Some(pubkey) = self.buyback_fee_recipient_token_account {
            accounts.push(resolved("buybackFeeRecipientTokenAccount", pubkey));
        }
        accounts
    }
}

fn resolved(role: &'static str, pubkey: Pubkey) -> ResolvedRouteAccount {
    ResolvedRouteAccount { role, pubkey }
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

#[cfg(test)]
pub(crate) fn parse_trade(
    versioned_tx: &VersionedTransaction,
    account_keys: &[Pubkey],
    target_wallets: &[String],
) -> Option<ParsedTrade> {
    let target_wallets = target_wallets
        .iter()
        .filter_map(|wallet| Pubkey::from_str(wallet).ok())
        .collect::<HashSet<_>>();
    parse_trade_with_target_set(versioned_tx, account_keys, &target_wallets)
}

#[cfg(test)]
pub(crate) fn parse_trade_with_target_set(
    versioned_tx: &VersionedTransaction,
    account_keys: &[Pubkey],
    target_wallets: &HashSet<Pubkey>,
) -> Option<ParsedTrade> {
    if mentioned_target_wallet_in_set(account_keys, target_wallets).is_none() {
        return None;
    }

    parse_trade_for_mentioned_targets(versioned_tx, account_keys, target_wallets)
}

pub(crate) fn parse_trade_for_mentioned_targets(
    versioned_tx: &VersionedTransaction,
    account_keys: &[Pubkey],
    target_wallets: &HashSet<Pubkey>,
) -> Option<ParsedTrade> {
    for instruction in versioned_tx.message.instructions() {
        let program_id = account_keys.get(instruction.program_id_index as usize)?;
        let parsed = if program_id == pump_fun_program_id() {
            routes::pump::parse(instruction, account_keys, target_wallets)
        } else if program_id == pump_amm_program_id() {
            routes::pump_amm::parse(instruction, account_keys, target_wallets)
        } else if program_id == flashx_router_program_id() {
            routes::flashx::parse(instruction, account_keys, target_wallets)
        } else {
            None
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
    account_keys: &[Pubkey],
    parsed: &ParsedTrade,
) -> Option<RouteContext> {
    match parsed.route {
        Route::FlashxPump => {
            routes::flashx::route_context(message, instruction, account_keys, parsed)
        }
        Route::Pump | Route::PumpAmm => None,
    }
}

#[cfg(test)]
pub(crate) fn mentioned_target_wallet(
    account_keys: &[Pubkey],
    target_wallets: &[String],
) -> Option<String> {
    let target_wallets = target_wallets
        .iter()
        .filter_map(|wallet| Pubkey::from_str(wallet).ok())
        .collect::<HashSet<_>>();
    mentioned_target_wallet_in_set(account_keys, &target_wallets)
}

#[cfg(test)]
pub(crate) fn mentioned_target_wallet_in_set(
    account_keys: &[Pubkey],
    target_wallets: &HashSet<Pubkey>,
) -> Option<String> {
    account_keys
        .iter()
        .find(|account_key| target_wallets.contains(account_key))
        .map(ToString::to_string)
}

pub(crate) fn classify_wallet_mention(
    versioned_tx: &VersionedTransaction,
    account_keys: &[Pubkey],
) -> WalletMentionClassification {
    let mut programs = Vec::new();
    for instruction in versioned_tx.message.instructions() {
        if let Some(program_id) = account_keys.get(instruction.program_id_index as usize) {
            programs.push(program_id);
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
pub(crate) fn static_account_keys(versioned_tx: &VersionedTransaction) -> Vec<Pubkey> {
    versioned_tx
        .message
        .static_account_keys()
        .iter()
        .copied()
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

pub(crate) fn system_program_id() -> &'static Pubkey {
    static ID: OnceLock<Pubkey> = OnceLock::new();
    ID.get_or_init(|| Pubkey::from_str(SYSTEM_PROGRAM_ID).expect("system program id is valid"))
}

pub(crate) fn compute_budget_program_id() -> &'static Pubkey {
    static ID: OnceLock<Pubkey> = OnceLock::new();
    ID.get_or_init(|| {
        Pubkey::from_str(COMPUTE_BUDGET_PROGRAM_ID).expect("compute budget program id is valid")
    })
}

pub(crate) fn token_program_id() -> &'static Pubkey {
    static ID: OnceLock<Pubkey> = OnceLock::new();
    ID.get_or_init(|| Pubkey::from_str(TOKEN_PROGRAM_ID).expect("token program id is valid"))
}

pub(crate) fn token_2022_program_id() -> &'static Pubkey {
    static ID: OnceLock<Pubkey> = OnceLock::new();
    ID.get_or_init(|| {
        Pubkey::from_str(TOKEN_2022_PROGRAM_ID).expect("token-2022 program id is valid")
    })
}

pub(crate) fn associated_token_program_id() -> &'static Pubkey {
    static ID: OnceLock<Pubkey> = OnceLock::new();
    ID.get_or_init(|| {
        Pubkey::from_str(ASSOCIATED_TOKEN_PROGRAM_ID).expect("associated token program id is valid")
    })
}

pub(crate) fn pump_fun_program_id() -> &'static Pubkey {
    static ID: OnceLock<Pubkey> = OnceLock::new();
    ID.get_or_init(|| Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("pump program id is valid"))
}

pub(crate) fn pump_amm_program_id() -> &'static Pubkey {
    static ID: OnceLock<Pubkey> = OnceLock::new();
    ID.get_or_init(|| Pubkey::from_str(PUMP_AMM_PROGRAM_ID).expect("pump amm program id is valid"))
}

pub(crate) fn flashx_router_program_id() -> &'static Pubkey {
    static ID: OnceLock<Pubkey> = OnceLock::new();
    ID.get_or_init(|| {
        Pubkey::from_str(FLASHX_ROUTER_PROGRAM_ID).expect("flashx program id is valid")
    })
}

fn is_non_trade_program(program_id: &Pubkey) -> bool {
    program_id == system_program_id()
        || program_id == compute_budget_program_id()
        || program_id == token_program_id()
        || program_id == token_2022_program_id()
        || program_id == associated_token_program_id()
}

fn is_supported_trade_program(program_id: &Pubkey) -> bool {
    program_id == pump_fun_program_id()
        || program_id == pump_amm_program_id()
        || program_id == flashx_router_program_id()
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
