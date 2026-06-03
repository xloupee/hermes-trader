use crate::{
    event::ShadowSignalLine,
    parser::{Action, ResolvedRouteAccount, Route, RouteContext},
    tx_builder::{
        build_copy_unsigned_flashx_pump, build_full_copy_unsigned_flashx_pump,
        build_unsigned_flashx_pump, TxBuildError,
    },
};
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PlannerOptions {
    pub(crate) copy_sol_amount: Option<f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecutionPlanLine {
    pub(crate) schema: &'static str,
    pub(crate) observed_at_ms: u128,
    pub(crate) planned_at_ms: u128,
    pub(crate) provider: &'static str,
    pub(crate) source: &'static str,
    pub(crate) endpoint: String,
    pub(crate) target_wallet: String,
    pub(crate) signature: String,
    pub(crate) slot: u64,
    pub(crate) route: Route,
    pub(crate) mint: String,
    pub(crate) shadow_action: Action,
    pub(crate) shadow_decision: String,
    pub(crate) allowed: bool,
    pub(crate) decision: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) spend_sol_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<&'static str>,
    #[serde(skip)]
    pub(crate) route_context: Option<RouteContext>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TxBuildPlannerOptions {
    pub(crate) max_plan_age_ms: u128,
}

#[derive(Clone, Debug)]
pub(crate) struct CopyTxPlannerOptions {
    pub(crate) max_plan_age_ms: u128,
    pub(crate) copy_wallet: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct UnsignedTxPlannerOptions {
    pub(crate) max_plan_age_ms: u128,
    pub(crate) copy_wallet: Option<String>,
    pub(crate) simulate_copy_tx: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TxBuildPlanLine {
    schema: &'static str,
    observed_at_ms: u128,
    planned_at_ms: u128,
    tx_build_planned_at_ms: u128,
    provider: &'static str,
    source: &'static str,
    endpoint: String,
    target_wallet: String,
    signature: String,
    slot: u64,
    selected_route: Route,
    mint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    spend_sol_amount: Option<f64>,
    buildable: bool,
    decision: &'static str,
    required_accounts: Vec<&'static str>,
    instruction_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    route_layout: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resolved_accounts: Vec<ResolvedRouteAccount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    missing_account_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CopyTxPlanLine {
    schema: &'static str,
    observed_at_ms: u128,
    planned_at_ms: u128,
    copy_planned_at_ms: u128,
    provider: &'static str,
    source: &'static str,
    endpoint: String,
    observed_wallet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet: Option<String>,
    signature: String,
    slot: u64,
    selected_route: Route,
    mint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    route_layout: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spend_sol_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet_token_account: Option<String>,
    copy_buildable: bool,
    decision: &'static str,
    copied_instruction_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    missing_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnsignedTxPlanLine {
    schema: &'static str,
    observed_at_ms: u128,
    planned_at_ms: u128,
    unsigned_planned_at_ms: u128,
    provider: &'static str,
    source: &'static str,
    endpoint: String,
    observed_wallet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet: Option<String>,
    signature: String,
    slot: u64,
    selected_route: Route,
    mint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    route_layout: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spend_sol_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet_token_account: Option<String>,
    instruction_count: usize,
    setup_instruction_count: usize,
    main_instruction_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    estimated_required_signer: Option<String>,
    buildable: bool,
    decision: &'static str,
    simulation_requested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    missing_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

pub(crate) fn execution_plan_line(
    signal: &ShadowSignalLine,
    planned_at_ms: u128,
    options: PlannerOptions,
) -> ExecutionPlanLine {
    let plan = plan_decision(signal, options);

    ExecutionPlanLine {
        schema: "copytrade.executionPlan.v1",
        observed_at_ms: signal.observed_at_ms,
        planned_at_ms,
        provider: "shredstream",
        source: "jito-proxy",
        endpoint: signal.endpoint.clone(),
        target_wallet: signal.target_wallet.clone(),
        signature: signal.signature.clone(),
        slot: signal.slot,
        route: signal.route,
        mint: signal.mint.clone(),
        shadow_action: signal.action,
        shadow_decision: signal.decision.to_string(),
        allowed: plan.allowed,
        decision: if plan.allowed { "wouldBuy" } else { "skip" },
        spend_sol_amount: plan.spend_sol_amount,
        reason: plan.reason,
        route_context: signal.route_context.clone(),
    }
}

struct PlanDecision {
    allowed: bool,
    spend_sol_amount: Option<f64>,
    reason: Option<&'static str>,
}

fn plan_decision(signal: &ShadowSignalLine, options: PlannerOptions) -> PlanDecision {
    if signal.schema != "copytrade.shadowSignal.v1" {
        return skipped("unsupported signal schema");
    }

    if !signal.copyable || signal.decision != "wouldCopy" {
        return skipped("shadow signal is not copyable");
    }

    if signal.action != Action::Buy {
        return skipped("execution planner only allows buy signals");
    }

    if signal.route != Route::FlashxPump {
        return skipped("route is not allowed for execution planning");
    }

    let spend_sol_amount = options.copy_sol_amount.or(signal.sol_amount);
    if !spend_sol_amount.is_some_and(|amount| amount.is_finite() && amount > 0.0) {
        return skipped("missing positive SOL spend amount");
    }

    PlanDecision {
        allowed: true,
        spend_sol_amount,
        reason: None,
    }
}

fn skipped(reason: &'static str) -> PlanDecision {
    PlanDecision {
        allowed: false,
        spend_sol_amount: None,
        reason: Some(reason),
    }
}

pub(crate) fn tx_build_plan_line(
    execution_plan: &ExecutionPlanLine,
    tx_build_planned_at_ms: u128,
    options: TxBuildPlannerOptions,
) -> TxBuildPlanLine {
    let decision = tx_build_decision(execution_plan, tx_build_planned_at_ms, options);

    TxBuildPlanLine {
        schema: "copytrade.txBuildPlan.v1",
        observed_at_ms: execution_plan.observed_at_ms,
        planned_at_ms: execution_plan.planned_at_ms,
        tx_build_planned_at_ms,
        provider: "shredstream",
        source: "jito-proxy",
        endpoint: execution_plan.endpoint.clone(),
        target_wallet: execution_plan.target_wallet.clone(),
        signature: execution_plan.signature.clone(),
        slot: execution_plan.slot,
        selected_route: execution_plan.route,
        mint: execution_plan.mint.clone(),
        spend_sol_amount: execution_plan.spend_sol_amount,
        buildable: decision.buildable,
        decision: decision.decision,
        required_accounts: decision.required_accounts,
        instruction_count: decision.instruction_count,
        route_layout: decision.route_layout,
        resolved_accounts: decision.resolved_accounts,
        missing_account_reason: decision.missing_account_reason,
        reason: decision.reason,
    }
}

pub(crate) fn copy_tx_plan_line(
    execution_plan: &ExecutionPlanLine,
    copy_planned_at_ms: u128,
    options: CopyTxPlannerOptions,
) -> CopyTxPlanLine {
    let decision = copy_tx_decision(execution_plan, copy_planned_at_ms, &options);

    CopyTxPlanLine {
        schema: "copytrade.copyTxPlan.v1",
        observed_at_ms: execution_plan.observed_at_ms,
        planned_at_ms: execution_plan.planned_at_ms,
        copy_planned_at_ms,
        provider: "shredstream",
        source: "jito-proxy",
        endpoint: execution_plan.endpoint.clone(),
        observed_wallet: execution_plan.target_wallet.clone(),
        copy_wallet: options.copy_wallet,
        signature: execution_plan.signature.clone(),
        slot: execution_plan.slot,
        selected_route: execution_plan.route,
        mint: execution_plan.mint.clone(),
        route_layout: decision.route_layout,
        spend_sol_amount: execution_plan.spend_sol_amount,
        copy_wallet_token_account: decision.copy_wallet_token_account,
        copy_buildable: decision.copy_buildable,
        decision: decision.decision,
        copied_instruction_count: decision.copied_instruction_count,
        missing_reason: decision.missing_reason,
        reason: decision.reason,
    }
}

pub(crate) fn unsigned_tx_plan_line(
    execution_plan: &ExecutionPlanLine,
    unsigned_planned_at_ms: u128,
    options: UnsignedTxPlannerOptions,
) -> UnsignedTxPlanLine {
    let decision = unsigned_tx_decision(execution_plan, unsigned_planned_at_ms, &options);

    UnsignedTxPlanLine {
        schema: "copytrade.unsignedTxPlan.v1",
        observed_at_ms: execution_plan.observed_at_ms,
        planned_at_ms: execution_plan.planned_at_ms,
        unsigned_planned_at_ms,
        provider: "shredstream",
        source: "jito-proxy",
        endpoint: execution_plan.endpoint.clone(),
        observed_wallet: execution_plan.target_wallet.clone(),
        copy_wallet: options.copy_wallet,
        signature: execution_plan.signature.clone(),
        slot: execution_plan.slot,
        selected_route: execution_plan.route,
        mint: execution_plan.mint.clone(),
        route_layout: decision.route_layout,
        spend_sol_amount: execution_plan.spend_sol_amount,
        copy_wallet_token_account: decision.copy_wallet_token_account,
        instruction_count: decision.instruction_count,
        setup_instruction_count: decision.setup_instruction_count,
        main_instruction_count: decision.main_instruction_count,
        estimated_required_signer: decision.estimated_required_signer,
        buildable: decision.buildable,
        decision: decision.decision,
        simulation_requested: options.simulate_copy_tx,
        missing_reason: decision.missing_reason,
        reason: decision.reason,
    }
}

struct TxBuildDecision {
    buildable: bool,
    decision: &'static str,
    required_accounts: Vec<&'static str>,
    instruction_count: usize,
    route_layout: Option<&'static str>,
    resolved_accounts: Vec<ResolvedRouteAccount>,
    missing_account_reason: Option<&'static str>,
    reason: Option<&'static str>,
}

fn tx_build_decision(
    execution_plan: &ExecutionPlanLine,
    tx_build_planned_at_ms: u128,
    options: TxBuildPlannerOptions,
) -> TxBuildDecision {
    if execution_plan.schema != "copytrade.executionPlan.v1" {
        return tx_build_skipped("unsupported execution plan schema");
    }

    if tx_build_planned_at_ms.saturating_sub(execution_plan.planned_at_ms) > options.max_plan_age_ms
    {
        return tx_build_skipped("execution plan is stale");
    }

    if !execution_plan.allowed || execution_plan.decision != "wouldBuy" {
        return tx_build_skipped("execution plan is not allowed");
    }

    if execution_plan.route != Route::FlashxPump {
        return tx_build_skipped("unsupported tx build route");
    }

    if !execution_plan
        .spend_sol_amount
        .is_some_and(|amount| amount.is_finite() && amount > 0.0)
    {
        return tx_build_skipped("missing positive SOL spend amount");
    }

    match build_unsigned_flashx_pump(execution_plan.route_context.as_ref()) {
        Ok(build) => TxBuildDecision {
            buildable: true,
            decision: "buildable",
            required_accounts: flashx_pump_required_accounts(),
            instruction_count: build.instructions.len(),
            route_layout: Some(build.route_layout),
            resolved_accounts: build.resolved_accounts,
            missing_account_reason: None,
            reason: None,
        },
        Err(
            TxBuildError::MissingRouteContext(reason) | TxBuildError::InvalidInstruction(reason),
        ) => TxBuildDecision {
            buildable: false,
            decision: "missingAccounts",
            required_accounts: flashx_pump_required_accounts(),
            instruction_count: 0,
            route_layout: None,
            resolved_accounts: Vec::new(),
            missing_account_reason: Some(reason),
            reason: None,
        },
        Err(TxBuildError::UnsupportedLayout(reason)) => TxBuildDecision {
            buildable: false,
            decision: "skip",
            required_accounts: Vec::new(),
            instruction_count: 0,
            route_layout: None,
            resolved_accounts: Vec::new(),
            missing_account_reason: None,
            reason: Some(reason),
        },
    }
}

fn tx_build_skipped(reason: &'static str) -> TxBuildDecision {
    TxBuildDecision {
        buildable: false,
        decision: "skip",
        required_accounts: Vec::new(),
        instruction_count: 0,
        route_layout: None,
        resolved_accounts: Vec::new(),
        missing_account_reason: None,
        reason: Some(reason),
    }
}

struct CopyTxDecision {
    copy_buildable: bool,
    decision: &'static str,
    copied_instruction_count: usize,
    route_layout: Option<&'static str>,
    copy_wallet_token_account: Option<String>,
    missing_reason: Option<&'static str>,
    reason: Option<&'static str>,
}

fn copy_tx_decision(
    execution_plan: &ExecutionPlanLine,
    copy_planned_at_ms: u128,
    options: &CopyTxPlannerOptions,
) -> CopyTxDecision {
    if execution_plan.schema != "copytrade.executionPlan.v1" {
        return copy_tx_skipped("unsupported execution plan schema");
    }

    if copy_planned_at_ms.saturating_sub(execution_plan.planned_at_ms) > options.max_plan_age_ms {
        return copy_tx_skipped("execution plan is stale");
    }

    if !execution_plan.allowed || execution_plan.decision != "wouldBuy" {
        return copy_tx_skipped("execution plan is not allowed");
    }

    if execution_plan.route != Route::FlashxPump {
        return copy_tx_skipped("unsupported copy route");
    }

    if !execution_plan
        .spend_sol_amount
        .is_some_and(|amount| amount.is_finite() && amount > 0.0)
    {
        return copy_tx_skipped("missing positive SOL spend amount");
    }

    let Some(copy_wallet) = options.copy_wallet.as_deref() else {
        return copy_tx_missing("missing copy wallet");
    };

    match build_copy_unsigned_flashx_pump(
        execution_plan.route_context.as_ref(),
        copy_wallet,
        &execution_plan.mint,
    ) {
        Ok(build) => CopyTxDecision {
            copy_buildable: true,
            decision: "buildable",
            copied_instruction_count: build.instructions.len(),
            route_layout: Some(build.route_layout),
            copy_wallet_token_account: Some(build.copy_wallet_token_account),
            missing_reason: None,
            reason: None,
        },
        Err(
            TxBuildError::MissingRouteContext(reason) | TxBuildError::InvalidInstruction(reason),
        ) => CopyTxDecision {
            copy_buildable: false,
            decision: "missingAccounts",
            copied_instruction_count: 0,
            route_layout: None,
            copy_wallet_token_account: None,
            missing_reason: Some(reason),
            reason: None,
        },
        Err(TxBuildError::UnsupportedLayout(reason)) => copy_tx_skipped(reason),
    }
}

fn copy_tx_missing(reason: &'static str) -> CopyTxDecision {
    CopyTxDecision {
        copy_buildable: false,
        decision: "missing",
        copied_instruction_count: 0,
        route_layout: None,
        copy_wallet_token_account: None,
        missing_reason: Some(reason),
        reason: None,
    }
}

fn copy_tx_skipped(reason: &'static str) -> CopyTxDecision {
    CopyTxDecision {
        copy_buildable: false,
        decision: "skip",
        copied_instruction_count: 0,
        route_layout: None,
        copy_wallet_token_account: None,
        missing_reason: None,
        reason: Some(reason),
    }
}

struct UnsignedTxDecision {
    buildable: bool,
    decision: &'static str,
    instruction_count: usize,
    setup_instruction_count: usize,
    main_instruction_count: usize,
    route_layout: Option<&'static str>,
    copy_wallet_token_account: Option<String>,
    estimated_required_signer: Option<String>,
    missing_reason: Option<&'static str>,
    reason: Option<&'static str>,
}

fn unsigned_tx_decision(
    execution_plan: &ExecutionPlanLine,
    unsigned_planned_at_ms: u128,
    options: &UnsignedTxPlannerOptions,
) -> UnsignedTxDecision {
    if execution_plan.schema != "copytrade.executionPlan.v1" {
        return unsigned_tx_skipped("unsupported execution plan schema");
    }

    if unsigned_planned_at_ms.saturating_sub(execution_plan.planned_at_ms) > options.max_plan_age_ms
    {
        return unsigned_tx_skipped("execution plan is stale");
    }

    if !execution_plan.allowed || execution_plan.decision != "wouldBuy" {
        return unsigned_tx_skipped("execution plan is not allowed");
    }

    if execution_plan.route != Route::FlashxPump {
        return unsigned_tx_skipped("unsupported unsigned tx route");
    }

    if !execution_plan
        .spend_sol_amount
        .is_some_and(|amount| amount.is_finite() && amount > 0.0)
    {
        return unsigned_tx_skipped("missing positive SOL spend amount");
    }

    let Some(copy_wallet) = options.copy_wallet.as_deref() else {
        return unsigned_tx_missing("missing copy wallet");
    };

    match build_full_copy_unsigned_flashx_pump(
        execution_plan.route_context.as_ref(),
        copy_wallet,
        &execution_plan.mint,
    ) {
        Ok(build) => UnsignedTxDecision {
            buildable: true,
            decision: "buildable",
            instruction_count: build.instructions.len(),
            setup_instruction_count: build.setup_instruction_count,
            main_instruction_count: build.main_instruction_count,
            route_layout: Some(build.route_layout),
            copy_wallet_token_account: Some(build.copy_wallet_token_account),
            estimated_required_signer: Some(build.estimated_required_signer),
            missing_reason: None,
            reason: None,
        },
        Err(
            TxBuildError::MissingRouteContext(reason) | TxBuildError::InvalidInstruction(reason),
        ) => UnsignedTxDecision {
            buildable: false,
            decision: "missingAccounts",
            instruction_count: 0,
            setup_instruction_count: 0,
            main_instruction_count: 0,
            route_layout: None,
            copy_wallet_token_account: None,
            estimated_required_signer: None,
            missing_reason: Some(reason),
            reason: None,
        },
        Err(TxBuildError::UnsupportedLayout(reason)) => unsigned_tx_skipped(reason),
    }
}

fn unsigned_tx_missing(reason: &'static str) -> UnsignedTxDecision {
    UnsignedTxDecision {
        buildable: false,
        decision: "missing",
        instruction_count: 0,
        setup_instruction_count: 0,
        main_instruction_count: 0,
        route_layout: None,
        copy_wallet_token_account: None,
        estimated_required_signer: None,
        missing_reason: Some(reason),
        reason: None,
    }
}

fn unsigned_tx_skipped(reason: &'static str) -> UnsignedTxDecision {
    UnsignedTxDecision {
        buildable: false,
        decision: "skip",
        instruction_count: 0,
        setup_instruction_count: 0,
        main_instruction_count: 0,
        route_layout: None,
        copy_wallet_token_account: None,
        estimated_required_signer: None,
        missing_reason: None,
        reason: Some(reason),
    }
}

fn flashx_pump_required_accounts() -> Vec<&'static str> {
    vec![
        "payer",
        "targetWallet",
        "mint",
        "flashxRouterProgram",
        "pumpAmmProgram",
        "associatedTokenProgram",
        "baseTokenProgram",
        "quoteTokenProgram",
        "systemProgram",
        "userBaseTokenAccount",
        "userQuoteTokenAccount",
        "poolBaseTokenAccount",
        "poolQuoteTokenAccount",
        "poolState",
        "globalConfig",
        "protocolFeeRecipient",
        "eventAuthority",
        "globalVolumeAccumulator",
        "userVolumeAccumulator",
        "feeConfig",
        "feeProgram",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        event::shadow_signal_line,
        parser::{parse_trade, static_account_keys, versioned_tx_signature_string},
    };
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use solana_transaction::versioned::VersionedTransaction;

    const TARGET_WALLET: &str = "A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS";
    const MIGRATED_BUY_SIGNATURE: &str =
        "Jo9sxcrorVCGkmafhNDQKByQBDBTSqM99tS9R1mYs6DjvFZHxZFuFhAvdSemCxFqauPcqS1t17ir3iDScu7cQF5";
    const MIGRATED_BUY_MINT: &str = "wXfe7vz2t8an9Ca5dy72ChU54fRvtefDRmb4rzUpump";
    const LIVE_DIRECT_PUMP_BUY_SIGNATURE: &str =
        "2BMXhQfpCcgGqaqSzPCM3uRgjBhbJf5jNh5UGsGyErQ3MF1muES8PBLhXC5kUyYFspeL9eFRT9xoSzLjTNBrEiCo";
    const LIVE_DIRECT_PUMP_MINT: &str = "8VigmMkK7f9FvTBDd8S2UmweezCgeBX4y5Xp4jMfpump";
    const COPY_WALLET: &str = "FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W";

    fn signal(action: Action, mint: &str, copyable: bool) -> ShadowSignalLine {
        ShadowSignalLine {
            schema: "copytrade.shadowSignal.v1",
            observed_at_ms: 123,
            provider: "shredstream",
            source: "jito-proxy",
            endpoint: "http://127.0.0.1:9999".to_string(),
            target_wallet: "wallet".to_string(),
            action,
            mint: mint.to_string(),
            signature: "sig".to_string(),
            slot: 456,
            route: Route::FlashxPump,
            sol_amount: if action == Action::Buy {
                Some(0.00099)
            } else {
                None
            },
            token_amount: if action == Action::Sell {
                Some(42.0)
            } else {
                None
            },
            copyable,
            decision: if copyable { "wouldCopy" } else { "skip" },
            reason: if copyable {
                None
            } else {
                Some("shadow mode only copies buy actions")
            },
            account_key_count: 14,
            route_context: None,
        }
    }

    fn allowed_execution_plan() -> ExecutionPlanLine {
        execution_plan_line(
            &signal(Action::Buy, "abcPumpIsNotLowerpump", true),
            200,
            PlannerOptions {
                copy_sol_amount: Some(0.00099),
            },
        )
    }

    #[test]
    fn buy_shadow_signal_becomes_allowed_execution_plan() {
        let plan = execution_plan_line(
            &signal(Action::Buy, "abcPumpIsNotLowerpump", true),
            234,
            PlannerOptions {
                copy_sol_amount: Some(0.0015),
            },
        );
        let value = serde_json::to_value(plan).expect("plan serializes");

        assert_eq!(value["schema"], "copytrade.executionPlan.v1");
        assert_eq!(value["decision"], "wouldBuy");
        assert_eq!(value["allowed"], true);
        assert_eq!(value["shadowAction"], "buy");
        assert_eq!(value["shadowDecision"], "wouldCopy");
        assert_eq!(value["mint"], "abcPumpIsNotLowerpump");
        assert_eq!(value["spendSolAmount"], 0.0015);
        assert!(value.get("reason").is_none());
    }

    #[test]
    fn sell_shadow_signal_becomes_skipped_execution_plan() {
        let plan = execution_plan_line(
            &signal(Action::Sell, "abcPumpIsNotLowerpump", false),
            234,
            PlannerOptions {
                copy_sol_amount: None,
            },
        );
        let value = serde_json::to_value(plan).expect("plan serializes");

        assert_eq!(value["schema"], "copytrade.executionPlan.v1");
        assert_eq!(value["decision"], "skip");
        assert_eq!(value["allowed"], false);
        assert_eq!(value["shadowAction"], "sell");
        assert_eq!(value["reason"], "shadow signal is not copyable");
        assert!(value.get("spendSolAmount").is_none());
    }

    #[test]
    fn supported_flashx_mint_without_pump_suffix_is_allowed() {
        let plan = execution_plan_line(
            &signal(Action::Buy, "not-a-pump-mint", true),
            234,
            PlannerOptions {
                copy_sol_amount: None,
            },
        );
        let value = serde_json::to_value(plan).expect("plan serializes");

        assert_eq!(value["allowed"], true);
        assert!(value.get("reason").is_none());
    }

    #[test]
    fn valid_execution_plan_without_route_context_skips_unsupported_layout() {
        let execution_plan = allowed_execution_plan();
        let build_plan = tx_build_plan_line(
            &execution_plan,
            250,
            TxBuildPlannerOptions {
                max_plan_age_ms: 1_000,
            },
        );
        let value = serde_json::to_value(build_plan).expect("tx build plan serializes");

        assert_eq!(value["schema"], "copytrade.txBuildPlan.v1");
        assert_eq!(value["selectedRoute"], "flashx-pump");
        assert_eq!(value["mint"], "abcPumpIsNotLowerpump");
        assert_eq!(value["spendSolAmount"], 0.00099);
        assert_eq!(value["buildable"], false);
        assert_eq!(value["decision"], "skip");
        assert_eq!(value["instructionCount"], 0);
        assert_eq!(value["reason"], "unsupported flashx-pump layout");
        assert!(value["requiredAccounts"].as_array().unwrap().is_empty());
        assert!(value.get("missingAccountReason").is_none());
    }

    #[test]
    fn migrated_flashx_execution_plan_builds_unsigned_instruction() {
        let execution_plan = migrated_buy_execution_plan();
        let build_plan = tx_build_plan_line(
            &execution_plan,
            250,
            TxBuildPlannerOptions {
                max_plan_age_ms: 1_000,
            },
        );
        let value = serde_json::to_value(build_plan).expect("tx build plan serializes");

        assert_eq!(value["schema"], "copytrade.txBuildPlan.v1");
        assert_eq!(value["signature"], MIGRATED_BUY_SIGNATURE);
        assert_eq!(value["selectedRoute"], "flashx-pump");
        assert_eq!(value["mint"], MIGRATED_BUY_MINT);
        assert_eq!(value["buildable"], true);
        assert_eq!(value["decision"], "buildable");
        assert_eq!(value["instructionCount"], 1);
        assert_eq!(value["routeLayout"], "migrated-amm");
        assert!(value["requiredAccounts"]
            .as_array()
            .expect("required accounts array")
            .contains(&serde_json::Value::String("mint".to_string())));
        assert!(value["resolvedAccounts"]
            .as_array()
            .expect("resolved accounts array")
            .iter()
            .any(|account| account["role"] == "mint" && account["pubkey"] == MIGRATED_BUY_MINT));
        assert!(value.get("missingAccountReason").is_none());
        assert!(value.get("reason").is_none());
    }

    #[test]
    fn live_direct_pump_execution_plan_builds_unsigned_instruction() {
        let execution_plan = live_direct_pump_buy_execution_plan();
        let build_plan = tx_build_plan_line(
            &execution_plan,
            250,
            TxBuildPlannerOptions {
                max_plan_age_ms: 1_000,
            },
        );
        let value = serde_json::to_value(build_plan).expect("tx build plan serializes");

        assert_eq!(value["schema"], "copytrade.txBuildPlan.v1");
        assert_eq!(value["signature"], LIVE_DIRECT_PUMP_BUY_SIGNATURE);
        assert_eq!(value["selectedRoute"], "flashx-pump");
        assert_eq!(value["mint"], LIVE_DIRECT_PUMP_MINT);
        assert_eq!(value["buildable"], true);
        assert_eq!(value["decision"], "buildable");
        assert_eq!(value["instructionCount"], 1);
        assert_eq!(value["routeLayout"], "direct-pump");
        assert!(value["resolvedAccounts"]
            .as_array()
            .expect("resolved accounts array")
            .iter()
            .any(|account| account["role"] == "pumpProgram"
                && account["pubkey"] == "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"));
        assert!(value.get("missingAccountReason").is_none());
        assert!(value.get("reason").is_none());
    }

    #[test]
    fn live_direct_pump_execution_plan_builds_copy_tx_plan() {
        let execution_plan = live_direct_pump_buy_execution_plan();
        let copy_plan = copy_tx_plan_line(
            &execution_plan,
            250,
            CopyTxPlannerOptions {
                max_plan_age_ms: 1_000,
                copy_wallet: Some(COPY_WALLET.to_string()),
            },
        );
        let value = serde_json::to_value(copy_plan).expect("copy tx plan serializes");

        assert_eq!(value["schema"], "copytrade.copyTxPlan.v1");
        assert_eq!(value["signature"], LIVE_DIRECT_PUMP_BUY_SIGNATURE);
        assert_eq!(value["observedWallet"], TARGET_WALLET);
        assert_eq!(value["copyWallet"], COPY_WALLET);
        assert_eq!(value["mint"], LIVE_DIRECT_PUMP_MINT);
        assert_eq!(value["routeLayout"], "direct-pump");
        assert_eq!(value["spendSolAmount"], 0.00099);
        assert_eq!(
            value["copyWalletTokenAccount"],
            "G2Bp3rC5GQHw8gWguLdujeZdTRoRgQia3Y1FmD5Ch4Vs"
        );
        assert_eq!(value["copyBuildable"], true);
        assert_eq!(value["decision"], "buildable");
        assert_eq!(value["copiedInstructionCount"], 1);
        assert!(value.get("missingReason").is_none());
        assert!(value.get("reason").is_none());
    }

    #[test]
    fn copy_tx_plan_requires_copy_wallet() {
        let execution_plan = live_direct_pump_buy_execution_plan();
        let copy_plan = copy_tx_plan_line(
            &execution_plan,
            250,
            CopyTxPlannerOptions {
                max_plan_age_ms: 1_000,
                copy_wallet: None,
            },
        );
        let value = serde_json::to_value(copy_plan).expect("copy tx plan serializes");

        assert_eq!(value["copyBuildable"], false);
        assert_eq!(value["decision"], "missing");
        assert_eq!(value["missingReason"], "missing copy wallet");
        assert!(value.get("copyWallet").is_none());
        assert!(value.get("copyWalletTokenAccount").is_none());
    }

    #[test]
    fn copy_tx_plan_builds_migrated_amm_copy_layout() {
        let execution_plan = migrated_buy_execution_plan();
        let copy_plan = copy_tx_plan_line(
            &execution_plan,
            250,
            CopyTxPlannerOptions {
                max_plan_age_ms: 1_000,
                copy_wallet: Some(COPY_WALLET.to_string()),
            },
        );
        let value = serde_json::to_value(copy_plan).expect("copy tx plan serializes");

        assert_eq!(value["routeLayout"], "migrated-amm");
        assert_eq!(value["copyBuildable"], true);
        assert_eq!(value["decision"], "buildable");
        assert_eq!(value["copiedInstructionCount"], 2);
        assert_eq!(
            value["copyWalletTokenAccount"],
            "2a7dXCUvaiwSsFDDxKAbcNarw1E6WC7j6ZCDWPRXCtnB"
        );
        assert!(value.get("reason").is_none());
    }

    #[test]
    fn live_direct_pump_execution_plan_builds_unsigned_tx_plan() {
        let execution_plan = live_direct_pump_buy_execution_plan();
        let unsigned_plan = unsigned_tx_plan_line(
            &execution_plan,
            250,
            UnsignedTxPlannerOptions {
                max_plan_age_ms: 1_000,
                copy_wallet: Some(COPY_WALLET.to_string()),
                simulate_copy_tx: false,
            },
        );
        let value = serde_json::to_value(unsigned_plan).expect("unsigned tx plan serializes");

        assert_eq!(value["schema"], "copytrade.unsignedTxPlan.v1");
        assert_eq!(value["signature"], LIVE_DIRECT_PUMP_BUY_SIGNATURE);
        assert_eq!(value["observedWallet"], TARGET_WALLET);
        assert_eq!(value["copyWallet"], COPY_WALLET);
        assert_eq!(value["mint"], LIVE_DIRECT_PUMP_MINT);
        assert_eq!(value["routeLayout"], "direct-pump");
        assert_eq!(value["spendSolAmount"], 0.00099);
        assert_eq!(
            value["copyWalletTokenAccount"],
            "G2Bp3rC5GQHw8gWguLdujeZdTRoRgQia3Y1FmD5Ch4Vs"
        );
        assert_eq!(value["instructionCount"], 3);
        assert_eq!(value["setupInstructionCount"], 2);
        assert_eq!(value["mainInstructionCount"], 1);
        assert_eq!(value["estimatedRequiredSigner"], COPY_WALLET);
        assert_eq!(value["buildable"], true);
        assert_eq!(value["decision"], "buildable");
        assert_eq!(value["simulationRequested"], false);
        assert!(value.get("missingReason").is_none());
        assert!(value.get("reason").is_none());
    }

    #[test]
    fn unsigned_tx_plan_requires_copy_wallet() {
        let execution_plan = live_direct_pump_buy_execution_plan();
        let unsigned_plan = unsigned_tx_plan_line(
            &execution_plan,
            250,
            UnsignedTxPlannerOptions {
                max_plan_age_ms: 1_000,
                copy_wallet: None,
                simulate_copy_tx: false,
            },
        );
        let value = serde_json::to_value(unsigned_plan).expect("unsigned tx plan serializes");

        assert_eq!(value["buildable"], false);
        assert_eq!(value["decision"], "missing");
        assert_eq!(value["missingReason"], "missing copy wallet");
        assert_eq!(value["instructionCount"], 0);
        assert_eq!(value["setupInstructionCount"], 0);
        assert_eq!(value["mainInstructionCount"], 0);
        assert!(value.get("copyWallet").is_none());
        assert!(value.get("copyWalletTokenAccount").is_none());
        assert!(value.get("estimatedRequiredSigner").is_none());
    }

    #[test]
    fn unsigned_tx_plan_builds_migrated_amm_copy_layout() {
        let execution_plan = migrated_buy_execution_plan();
        let unsigned_plan = unsigned_tx_plan_line(
            &execution_plan,
            250,
            UnsignedTxPlannerOptions {
                max_plan_age_ms: 1_000,
                copy_wallet: Some(COPY_WALLET.to_string()),
                simulate_copy_tx: true,
            },
        );
        let value = serde_json::to_value(unsigned_plan).expect("unsigned tx plan serializes");

        assert_eq!(value["schema"], "copytrade.unsignedTxPlan.v1");
        assert_eq!(value["routeLayout"], "migrated-amm");
        assert_eq!(value["buildable"], true);
        assert_eq!(value["decision"], "buildable");
        assert_eq!(value["instructionCount"], 4);
        assert_eq!(value["setupInstructionCount"], 2);
        assert_eq!(value["mainInstructionCount"], 2);
        assert_eq!(
            value["copyWalletTokenAccount"],
            "2a7dXCUvaiwSsFDDxKAbcNarw1E6WC7j6ZCDWPRXCtnB"
        );
        assert_eq!(value["simulationRequested"], true);
        assert!(value.get("reason").is_none());
    }

    #[test]
    fn tx_build_plan_skips_non_allowed_execution_plan() {
        let execution_plan = execution_plan_line(
            &signal(Action::Sell, "abcPumpIsNotLowerpump", false),
            200,
            PlannerOptions {
                copy_sol_amount: None,
            },
        );
        let build_plan = tx_build_plan_line(
            &execution_plan,
            250,
            TxBuildPlannerOptions {
                max_plan_age_ms: 1_000,
            },
        );
        let value = serde_json::to_value(build_plan).expect("tx build plan serializes");

        assert_eq!(value["schema"], "copytrade.txBuildPlan.v1");
        assert_eq!(value["buildable"], false);
        assert_eq!(value["decision"], "skip");
        assert_eq!(value["reason"], "execution plan is not allowed");
        assert_eq!(value["instructionCount"], 0);
        assert!(value["requiredAccounts"].as_array().unwrap().is_empty());
        assert!(value.get("missingAccountReason").is_none());
    }

    #[test]
    fn tx_build_plan_skips_stale_execution_plan() {
        let execution_plan = allowed_execution_plan();
        let build_plan = tx_build_plan_line(
            &execution_plan,
            1_500,
            TxBuildPlannerOptions {
                max_plan_age_ms: 1_000,
            },
        );
        let value = serde_json::to_value(build_plan).expect("tx build plan serializes");

        assert_eq!(value["buildable"], false);
        assert_eq!(value["decision"], "skip");
        assert_eq!(value["reason"], "execution plan is stale");
    }

    #[test]
    fn tx_build_plan_skips_unsupported_route() {
        let mut execution_plan = allowed_execution_plan();
        execution_plan.route = Route::Pump;
        let build_plan = tx_build_plan_line(
            &execution_plan,
            250,
            TxBuildPlannerOptions {
                max_plan_age_ms: 1_000,
            },
        );
        let value = serde_json::to_value(build_plan).expect("tx build plan serializes");

        assert_eq!(value["buildable"], false);
        assert_eq!(value["decision"], "skip");
        assert_eq!(value["reason"], "unsupported tx build route");
        assert!(value["requiredAccounts"].as_array().unwrap().is_empty());
    }

    #[test]
    fn tx_build_plan_skips_missing_positive_sol_amount() {
        let mut execution_plan = allowed_execution_plan();
        execution_plan.spend_sol_amount = Some(0.0);
        let build_plan = tx_build_plan_line(
            &execution_plan,
            250,
            TxBuildPlannerOptions {
                max_plan_age_ms: 1_000,
            },
        );
        let value = serde_json::to_value(build_plan).expect("tx build plan serializes");

        assert_eq!(value["buildable"], false);
        assert_eq!(value["decision"], "skip");
        assert_eq!(value["reason"], "missing positive SOL spend amount");
        assert!(value["requiredAccounts"].as_array().unwrap().is_empty());
    }

    fn migrated_buy_execution_plan() -> ExecutionPlanLine {
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
        let signal = shadow_signal_line(
            123,
            "replay".to_string(),
            MIGRATED_BUY_SIGNATURE.to_string(),
            456,
            account_keys.len(),
            &parsed,
        );

        execution_plan_line(
            &signal,
            200,
            PlannerOptions {
                copy_sol_amount: Some(0.00099),
            },
        )
    }

    fn live_direct_pump_buy_execution_plan() -> ExecutionPlanLine {
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
        let signal = shadow_signal_line(
            123,
            "replay".to_string(),
            LIVE_DIRECT_PUMP_BUY_SIGNATURE.to_string(),
            456,
            account_keys.len(),
            &parsed,
        );

        execution_plan_line(
            &signal,
            200,
            PlannerOptions {
                copy_sol_amount: Some(0.00099),
            },
        )
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
