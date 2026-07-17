use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use alloy_primitives::{Address, U256};
use anyhow::{Context, Result, bail};
use clap::Parser;
use hermes_feed::{
    AutomatedPaperRuntime, ConditionalOptions, FeedBoundary, PaperRuntimeSnapshot, RiskLimits,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Run a deterministic, broadcast-free Hermes trading scenario"
)]
struct Cli {
    #[arg(long)]
    scenario: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Scenario {
    pending_nonce: u64,
    limits: ScenarioLimits,
    steps: Vec<ScenarioStep>,
}

#[derive(Debug, Deserialize)]
struct ScenarioLimits {
    max_trade_amount_in: String,
    max_open_exposure: String,
    max_gas_cost_wei: String,
    max_session_loss: String,
    max_slippage_bps: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
enum ScenarioStep {
    PrepareEntry {
        token: String,
        amount_in: String,
        expected_token_out: String,
        gas_limit: u64,
        max_fee_per_gas: String,
        slippage_bps: u16,
        launch_l1_block: u64,
        l1_window: u64,
        timestamp_max: Option<u64>,
    },
    PrepareExit {
        token: String,
        expected_proceeds: String,
        gas_limit: u64,
        max_fee_per_gas: String,
        slippage_bps: u16,
        boundary_base_l1_block: u64,
        l1_window: u64,
        timestamp_max: Option<u64>,
    },
    Boundary {
        l1_block_number: u64,
        l1_timestamp: u64,
        #[serde(default = "default_true")]
        sequence_contiguous: bool,
    },
    Fill {
        order_id: Option<u64>,
        actual_amount: String,
        gas_cost: String,
    },
    ExplicitRejection {
        order_id: Option<u64>,
    },
    Snapshot,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let bytes = fs::read(&cli.scenario)
        .with_context(|| format!("read scenario {}", cli.scenario.display()))?;
    let scenario: Scenario = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse scenario {}", cli.scenario.display()))?;
    let mut runtime = AutomatedPaperRuntime::new(
        scenario.pending_nonce,
        RiskLimits {
            max_trade_amount_in: parse_u256(&scenario.limits.max_trade_amount_in)?,
            max_open_exposure: parse_u256(&scenario.limits.max_open_exposure)?,
            max_gas_cost_wei: parse_u256(&scenario.limits.max_gas_cost_wei)?,
            max_session_loss: parse_u256(&scenario.limits.max_session_loss)?,
            max_slippage_bps: scenario.limits.max_slippage_bps,
        },
    );
    write_record(json!({
        "record_type": "paper_scenario_start",
        "scenario": cli.scenario,
        "runtime": runtime.snapshot(),
    }))?;

    for (index, step) in scenario.steps.into_iter().enumerate() {
        let step_number = index + 1;
        match step {
            ScenarioStep::PrepareEntry {
                token,
                amount_in,
                expected_token_out,
                gas_limit,
                max_fee_per_gas,
                slippage_bps,
                launch_l1_block,
                l1_window,
                timestamp_max,
            } => {
                let order = runtime.prepare_entry(
                    Address::from_str(&token).context("parse entry token")?,
                    parse_u256(&amount_in)?,
                    parse_u256(&expected_token_out)?,
                    gas_limit,
                    parse_u128(&max_fee_per_gas)?,
                    slippage_bps,
                    conditions(launch_l1_block, l1_window, timestamp_max)?,
                )?;
                write_record(json!({
                    "record_type": "paper_order_prepared",
                    "step": step_number,
                    "order": order,
                    "runtime": runtime.snapshot(),
                }))?;
            }
            ScenarioStep::PrepareExit {
                token,
                expected_proceeds,
                gas_limit,
                max_fee_per_gas,
                slippage_bps,
                boundary_base_l1_block,
                l1_window,
                timestamp_max,
            } => {
                let order = runtime.prepare_exit(
                    Address::from_str(&token).context("parse exit token")?,
                    parse_u256(&expected_proceeds)?,
                    gas_limit,
                    parse_u128(&max_fee_per_gas)?,
                    slippage_bps,
                    conditions(boundary_base_l1_block, l1_window, timestamp_max)?,
                )?;
                write_record(json!({
                    "record_type": "paper_order_prepared",
                    "step": step_number,
                    "order": order,
                    "runtime": runtime.snapshot(),
                }))?;
            }
            ScenarioStep::Boundary {
                l1_block_number,
                l1_timestamp,
                sequence_contiguous,
            } => {
                let event = runtime.observe_boundary(FeedBoundary {
                    l1_block_number,
                    l1_timestamp,
                    sequence_contiguous,
                })?;
                write_record(json!({
                    "record_type": "paper_boundary",
                    "step": step_number,
                    "event": event,
                    "runtime": runtime.snapshot(),
                }))?;
            }
            ScenarioStep::Fill {
                order_id,
                actual_amount,
                gas_cost,
            } => {
                let order_id = resolve_order_id(&runtime.snapshot(), order_id)?;
                let reconciliation = runtime.reconcile_fill(
                    order_id,
                    parse_u256(&actual_amount)?,
                    parse_u256(&gas_cost)?,
                )?;
                write_record(json!({
                    "record_type": "paper_reconciliation",
                    "step": step_number,
                    "reconciliation": reconciliation,
                    "runtime": runtime.snapshot(),
                }))?;
            }
            ScenarioStep::ExplicitRejection { order_id } => {
                let order_id = resolve_order_id(&runtime.snapshot(), order_id)?;
                let order = runtime.reconcile_explicit_rejection(order_id)?;
                write_record(json!({
                    "record_type": "paper_explicit_rejection",
                    "step": step_number,
                    "order": order,
                    "runtime": runtime.snapshot(),
                }))?;
            }
            ScenarioStep::Snapshot => write_record(json!({
                "record_type": "paper_snapshot",
                "step": step_number,
                "runtime": runtime.snapshot(),
            }))?,
        }
    }

    write_record(json!({
        "record_type": "paper_scenario_complete",
        "runtime": runtime.snapshot(),
    }))
}

fn conditions(
    boundary_base_l1_block: u64,
    l1_window: u64,
    timestamp_max: Option<u64>,
) -> Result<ConditionalOptions> {
    ConditionalOptions::first_eligible_window(boundary_base_l1_block, l1_window, timestamp_max)
        .ok_or_else(|| anyhow::anyhow!("conditional boundary overflow"))
}

fn resolve_order_id(snapshot: &PaperRuntimeSnapshot, requested: Option<u64>) -> Result<u64> {
    let active = snapshot
        .pending_order
        .ok_or_else(|| anyhow::anyhow!("no pending order"))?;
    if requested.is_some_and(|requested| requested != active.id) {
        bail!("requested order does not match pending order");
    }
    Ok(active.id)
}

fn parse_u256(value: &str) -> Result<U256> {
    if let Some(value) = value.strip_prefix("0x") {
        U256::from_str_radix(value, 16).context("parse hexadecimal U256")
    } else {
        U256::from_str(value).context("parse decimal U256")
    }
}

fn parse_u128(value: &str) -> Result<u128> {
    if let Some(value) = value.strip_prefix("0x") {
        u128::from_str_radix(value, 16).context("parse hexadecimal u128")
    } else {
        value.parse().context("parse decimal u128")
    }
}

fn write_record(value: serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}

fn default_true() -> bool {
    true
}
