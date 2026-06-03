use crate::{
    event::ShadowSignalLine,
    parser::{Action, Route},
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
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TxBuildPlannerOptions {
    pub(crate) max_plan_age_ms: u128,
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
    missing_account_reason: Option<&'static str>,
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

    if !signal.mint.ends_with("pump") {
        return skipped("mint is not a pump mint");
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
        missing_account_reason: decision.missing_account_reason,
        reason: decision.reason,
    }
}

struct TxBuildDecision {
    buildable: bool,
    decision: &'static str,
    required_accounts: Vec<&'static str>,
    instruction_count: usize,
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

    if !execution_plan.mint.ends_with("pump") {
        return tx_build_skipped("mint is not a pump mint");
    }

    if !execution_plan
        .spend_sol_amount
        .is_some_and(|amount| amount.is_finite() && amount > 0.0)
    {
        return tx_build_skipped("missing positive SOL spend amount");
    }

    TxBuildDecision {
        buildable: false,
        decision: "missingAccounts",
        required_accounts: flashx_pump_required_accounts(),
        instruction_count: 0,
        missing_account_reason: Some(
            "flashx-pump migrated route account resolver is not implemented",
        ),
        reason: None,
    }
}

fn tx_build_skipped(reason: &'static str) -> TxBuildDecision {
    TxBuildDecision {
        buildable: false,
        decision: "skip",
        required_accounts: Vec::new(),
        instruction_count: 0,
        missing_account_reason: None,
        reason: Some(reason),
    }
}

fn flashx_pump_required_accounts() -> Vec<&'static str> {
    vec![
        "payer",
        "targetWallet",
        "mint",
        "flashxRouterProgram",
        "pumpProgram",
        "pumpAmmProgram",
        "associatedTokenProgram",
        "tokenProgram",
        "systemProgram",
        "userTokenAccount",
        "poolBaseTokenAccount",
        "poolQuoteTokenAccount",
        "poolState",
        "poolAuthority",
        "globalConfig",
        "feeRecipient",
        "eventAuthority",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

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
        }
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
    fn non_pump_mint_is_skipped() {
        let plan = execution_plan_line(
            &signal(Action::Buy, "not-a-pump-mint", true),
            234,
            PlannerOptions {
                copy_sol_amount: None,
            },
        );
        let value = serde_json::to_value(plan).expect("plan serializes");

        assert_eq!(value["allowed"], false);
        assert_eq!(value["reason"], "mint is not a pump mint");
    }

    #[test]
    fn valid_execution_plan_reports_missing_flashx_route_accounts() {
        let execution_plan = execution_plan_line(
            &signal(Action::Buy, "abcPumpIsNotLowerpump", true),
            200,
            PlannerOptions {
                copy_sol_amount: Some(0.00099),
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
        assert_eq!(value["selectedRoute"], "flashx-pump");
        assert_eq!(value["mint"], "abcPumpIsNotLowerpump");
        assert_eq!(value["spendSolAmount"], 0.00099);
        assert_eq!(value["buildable"], false);
        assert_eq!(value["decision"], "missingAccounts");
        assert_eq!(value["instructionCount"], 0);
        assert_eq!(
            value["missingAccountReason"],
            "flashx-pump migrated route account resolver is not implemented"
        );
        assert!(value["requiredAccounts"]
            .as_array()
            .expect("required accounts array")
            .contains(&serde_json::Value::String("mint".to_string())));
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
        let execution_plan = execution_plan_line(
            &signal(Action::Buy, "abcPumpIsNotLowerpump", true),
            200,
            PlannerOptions {
                copy_sol_amount: Some(0.00099),
            },
        );
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
}
