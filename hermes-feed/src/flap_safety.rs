use alloy_primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::flap_identity::{FLAP_PORTAL_PROXY, FlapPortalVariant};
use crate::robinhood::CHAIN_ID;

pub const FLAP_INDEX_VAULT_FACTORY: Address =
    alloy_primitives::address!("e6ca297d1d963b6f00d5b216986123caeb883af6");
pub const FLAP_INDEX_VAULT_FACTORY_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("37ad8d77398199bf43ec8b2cf20065264d96c3b540dd198c64a8a70d1537fe7f");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlapRouteVariant {
    DirectPortalCurve,
    PortalDexRouter,
    DirectMigratedDex,
    Aggregator,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlapTaxVariant {
    None,
    Symmetric,
    Asymmetric,
    TaxV3,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlapVaultVariant {
    None,
    IndexVault,
    External,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlapMigrationVariant {
    V2,
    V3,
    V4,
    ConcentratedLiquidity,
    External,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlapQuoteVariant {
    NativeEth18,
    Erc20 { token: Address, decimals: u8 },
    NativeToQuote,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlapPaperAssessmentInput {
    pub chain_id: u64,
    pub source: FlapPortalVariant,
    pub destination: Address,
    pub implementation_pinned: bool,
    pub implementation_source_verified: bool,
    pub route: FlapRouteVariant,
    pub quote: FlapQuoteVariant,
    pub tax: FlapTaxVariant,
    pub vault: FlapVaultVariant,
    pub extension_enabled: bool,
    pub migration: FlapMigrationVariant,
    pub migration_route_pinned: bool,
    pub token: Address,
    pub follower_amount_in: U256,
    pub follower_local_quote_out: U256,
    pub follower_slippage_bps: u16,
    pub leader_min_out: U256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct FlapPaperAssessment {
    pub destination: Address,
    pub token: Address,
    pub amount_in: U256,
    pub local_quote_out: U256,
    pub minimum_receive: U256,
    pub route: FlapRouteVariant,
    pub execution: FlapExecutionGate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "reason")]
pub enum FlapExecutionGate {
    Disabled(FlapExecutionBlocker),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlapExecutionBlocker {
    MigrationSemanticsIncomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FlapSafetyError {
    #[error("candidate is not on Robinhood mainnet chain 4663")]
    ChainMismatch,
    #[error("candidate is not a direct call to the pinned Portal proxy")]
    WrongDestination,
    #[error("Portal implementation identity or verified semantics are incomplete")]
    ImplementationIncomplete,
    #[error("route is unsupported or ambiguous")]
    RouteIncomplete,
    #[error("quote normalization is incomplete or unsupported")]
    QuoteIncomplete,
    #[error("taxed Flap profiles remain observe-only")]
    TaxUnsupported,
    #[error("vault-backed Flap profiles remain observe-only")]
    VaultUnsupported,
    #[error("Flap extensions remain observe-only")]
    ExtensionUnsupported,
    #[error("paper amount, local quote, or slippage is invalid")]
    InvalidAmounts,
}

/// Builds only a broadcast-free paper plan from warm, already-normalized
/// state. It performs no I/O and never copies the leader's minimum output.
/// The current verified evidence leaves every migration route incomplete, so
/// the returned execution gate remains disabled until a later pinned startup
/// snapshot proves the selected migrator and DEX semantics.
pub fn prepare_flap_paper_assessment(
    input: FlapPaperAssessmentInput,
) -> Result<FlapPaperAssessment, FlapSafetyError> {
    if input.chain_id != CHAIN_ID {
        return Err(FlapSafetyError::ChainMismatch);
    }
    if input.source != FlapPortalVariant::Portal || input.destination != FLAP_PORTAL_PROXY {
        return Err(FlapSafetyError::WrongDestination);
    }
    if !input.implementation_pinned || !input.implementation_source_verified {
        return Err(FlapSafetyError::ImplementationIncomplete);
    }
    if input.route != FlapRouteVariant::DirectPortalCurve {
        return Err(FlapSafetyError::RouteIncomplete);
    }
    if input.quote != FlapQuoteVariant::NativeEth18 {
        return Err(FlapSafetyError::QuoteIncomplete);
    }
    if input.tax != FlapTaxVariant::None {
        return Err(FlapSafetyError::TaxUnsupported);
    }
    if input.vault != FlapVaultVariant::None {
        return Err(FlapSafetyError::VaultUnsupported);
    }
    if input.extension_enabled {
        return Err(FlapSafetyError::ExtensionUnsupported);
    }
    if input.token == Address::ZERO
        || input.follower_amount_in == U256::ZERO
        || input.follower_local_quote_out == U256::ZERO
        || input.follower_slippage_bps > 10_000
    {
        return Err(FlapSafetyError::InvalidAmounts);
    }
    let minimum_receive = input
        .follower_local_quote_out
        .checked_mul(U256::from(
            10_000_u64 - u64::from(input.follower_slippage_bps),
        ))
        .ok_or(FlapSafetyError::InvalidAmounts)?
        / U256::from(10_000_u64);
    if minimum_receive == U256::ZERO {
        return Err(FlapSafetyError::InvalidAmounts);
    }

    // The delivered evidence does not pin the complete DEX route for even the
    // UI-selected V2 migrator. A boolean supplied by candidate data cannot
    // promote that research gap into executable authority.
    let execution = FlapExecutionGate::Disabled(FlapExecutionBlocker::MigrationSemanticsIncomplete);
    Ok(FlapPaperAssessment {
        destination: FLAP_PORTAL_PROXY,
        token: input.token,
        amount_in: input.follower_amount_in,
        local_quote_out: input.follower_local_quote_out,
        minimum_receive,
        route: input.route,
        execution,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> FlapPaperAssessmentInput {
        FlapPaperAssessmentInput {
            chain_id: CHAIN_ID,
            source: FlapPortalVariant::Portal,
            destination: FLAP_PORTAL_PROXY,
            implementation_pinned: true,
            implementation_source_verified: true,
            route: FlapRouteVariant::DirectPortalCurve,
            quote: FlapQuoteVariant::NativeEth18,
            tax: FlapTaxVariant::None,
            vault: FlapVaultVariant::None,
            extension_enabled: false,
            migration: FlapMigrationVariant::V2,
            migration_route_pinned: false,
            token: Address::with_last_byte(0x44),
            follower_amount_in: U256::from(100),
            follower_local_quote_out: U256::from(1_000),
            follower_slippage_bps: 500,
            leader_min_out: U256::from(7),
        }
    }

    #[test]
    fn paper_plan_uses_fresh_follower_quote_and_disables_incomplete_migration() {
        let plan = prepare_flap_paper_assessment(input()).unwrap();
        assert_eq!(plan.minimum_receive, U256::from(950));
        assert_ne!(plan.minimum_receive, input().leader_min_out);
        assert_eq!(
            plan.execution,
            FlapExecutionGate::Disabled(FlapExecutionBlocker::MigrationSemanticsIncomplete)
        );
    }

    #[test]
    fn rejects_arbitrary_vault_and_unverified_vault_portal() {
        let mut arbitrary_vault = input();
        arbitrary_vault.vault = FlapVaultVariant::External;
        assert_eq!(
            prepare_flap_paper_assessment(arbitrary_vault),
            Err(FlapSafetyError::VaultUnsupported)
        );

        let mut vault_portal = input();
        vault_portal.source = FlapPortalVariant::VaultPortal;
        vault_portal.destination = vault_portal.source.proxy();
        vault_portal.implementation_source_verified = false;
        assert_eq!(
            prepare_flap_paper_assessment(vault_portal),
            Err(FlapSafetyError::WrongDestination)
        );
    }

    #[test]
    fn rejects_tax_quote_route_and_chain_gaps_explicitly() {
        let mut tax = input();
        tax.tax = FlapTaxVariant::TaxV3;
        assert_eq!(
            prepare_flap_paper_assessment(tax),
            Err(FlapSafetyError::TaxUnsupported)
        );

        let mut quote = input();
        quote.quote = FlapQuoteVariant::Erc20 {
            token: Address::with_last_byte(9),
            decimals: 6,
        };
        assert_eq!(
            prepare_flap_paper_assessment(quote),
            Err(FlapSafetyError::QuoteIncomplete)
        );

        let mut route = input();
        route.route = FlapRouteVariant::Ambiguous;
        assert_eq!(
            prepare_flap_paper_assessment(route),
            Err(FlapSafetyError::RouteIncomplete)
        );

        let mut chain = input();
        chain.chain_id = 8_453;
        assert_eq!(
            prepare_flap_paper_assessment(chain),
            Err(FlapSafetyError::ChainMismatch)
        );
    }

    #[test]
    fn migration_variants_are_explicit_and_fail_closed() {
        for migration in [
            FlapMigrationVariant::V3,
            FlapMigrationVariant::V4,
            FlapMigrationVariant::ConcentratedLiquidity,
            FlapMigrationVariant::External,
            FlapMigrationVariant::Unknown,
        ] {
            let mut candidate = input();
            candidate.migration = migration;
            candidate.migration_route_pinned = true;
            assert_eq!(
                prepare_flap_paper_assessment(candidate).unwrap().execution,
                FlapExecutionGate::Disabled(FlapExecutionBlocker::MigrationSemanticsIncomplete)
            );
        }
    }
}
