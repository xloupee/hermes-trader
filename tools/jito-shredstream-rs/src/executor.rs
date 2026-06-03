use crate::{
    blockhash::{cached_blockhash, BlockhashCache},
    event::now_ms,
    parser::{Action, Route},
    planner::ExecutionPlanLine,
    tx_builder::{build_full_copy_unsigned_flashx_pump, TxBuildError},
    LiveOptions,
};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use solana_hash::Hash;
use solana_keypair::{read_keypair_file, Keypair};
use solana_signer::Signer;
use solana_transaction::Transaction;
use std::{path::PathBuf, str::FromStr};

const FIRST_LIVE_MAX_COPY_SOL_CAP: f64 = 0.001;

pub(crate) struct CopyExecutor {
    options: CopyExecutionOptions,
    keypair: Option<Keypair>,
    client: reqwest::Client,
    blockhash_cache: Option<BlockhashCache>,
}

#[derive(Clone, Debug)]
pub(crate) struct CopyExecutionOptions {
    pub(crate) enable_copy_send: bool,
    pub(crate) dry_run: bool,
    pub(crate) simulate_copy_tx: bool,
    pub(crate) max_copy_sol: Option<f64>,
    pub(crate) copy_wallet: Option<String>,
    pub(crate) copy_keypair_path: Option<PathBuf>,
    pub(crate) solana_rpc_url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CopyExecutionLine {
    schema: &'static str,
    observed_at_ms: u128,
    execution_at_ms: u128,
    provider: &'static str,
    source: &'static str,
    endpoint: String,
    observed_wallet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_wallet: Option<String>,
    observed_signature: String,
    slot: u64,
    selected_route: Route,
    mint: String,
    observed_action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_sol_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_copy_sol: Option<f64>,
    send_enabled: bool,
    dry_run: bool,
    simulation_requested: bool,
    signed: bool,
    simulated: bool,
    sent: bool,
    decision: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    signed_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    simulation_completed_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    send_submitted_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature_returned_at_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_to_signed_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_to_simulation_completed_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_to_send_submitted_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed_to_signature_returned_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    route_layout: Option<&'static str>,
    instruction_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    blockhash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    simulation_error: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    simulation_units_consumed: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    simulation_logs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    send_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

impl CopyExecutor {
    pub(crate) fn from_options(
        options: &LiveOptions,
        blockhash_cache: Option<BlockhashCache>,
    ) -> Result<Self> {
        let execution_options = CopyExecutionOptions {
            enable_copy_send: options.enable_copy_send,
            dry_run: options.dry_run,
            simulate_copy_tx: options.simulate_copy_tx,
            max_copy_sol: options.max_copy_sol,
            copy_wallet: options.copy_wallet.clone(),
            copy_keypair_path: options.copy_keypair_path.clone(),
            solana_rpc_url: options.solana_rpc_url.clone(),
        };

        let keypair = match execution_options.copy_keypair_path.as_ref() {
            Some(path) => Some(
                read_keypair_file(path)
                    .map_err(|error| anyhow::anyhow!("{error}"))
                    .with_context(|| format!("read copy keypair {}", path.display()))?,
            ),
            None => None,
        };

        Ok(Self {
            options: execution_options,
            keypair,
            client: reqwest::Client::new(),
            blockhash_cache,
        })
    }

    pub(crate) async fn handle(
        &self,
        execution_plan: &ExecutionPlanLine,
        observed_action: Action,
        observed_sol_amount: Option<f64>,
    ) -> CopyExecutionLine {
        let mut line = CopyExecutionLine::new(
            execution_plan,
            observed_action,
            observed_sol_amount,
            &self.options,
        );

        if !self.options.simulate_copy_tx && !self.options.enable_copy_send {
            return line.skip("copy execution is disabled");
        }

        if !execution_plan.allowed || execution_plan.decision != "wouldBuy" {
            return line.skip("execution plan is not allowed");
        }

        if observed_action != Action::Buy {
            return line.skip("copy execution only allows buy signals");
        }

        if execution_plan.route != Route::FlashxPump {
            return line.skip("unsupported copy execution route");
        }

        let Some(observed_sol_amount) = observed_sol_amount else {
            return line.skip("observed SOL amount is not confidently bounded");
        };
        if !observed_sol_amount.is_finite() || observed_sol_amount <= 0.0 {
            return line.skip("observed SOL amount is not confidently bounded");
        }

        let Some(max_copy_sol) = self.options.max_copy_sol else {
            return line.skip("missing max copy SOL guard");
        };
        if !max_copy_sol.is_finite() || max_copy_sol <= 0.0 {
            return line.skip("invalid max copy SOL guard");
        }
        if max_copy_sol > FIRST_LIVE_MAX_COPY_SOL_CAP {
            return line.skip(format!(
                "max copy SOL guard exceeds first-live cap {FIRST_LIVE_MAX_COPY_SOL_CAP}"
            ));
        }
        if observed_sol_amount > max_copy_sol {
            return line.skip("observed spend exceeds max copy SOL guard");
        }

        let Some(copy_wallet) = self.options.copy_wallet.as_deref() else {
            return line.skip("missing copy wallet");
        };
        let Some(keypair) = self.keypair.as_ref() else {
            return line.skip("missing copy keypair path");
        };
        if keypair.pubkey().to_string() != copy_wallet {
            return line.skip("copy keypair does not match copy wallet");
        }

        let Some(cached_blockhash) = cached_blockhash(self.blockhash_cache.as_ref()) else {
            return line.skip("missing warm blockhash");
        };

        let build = match build_full_copy_unsigned_flashx_pump(
            execution_plan.route_context.as_ref(),
            copy_wallet,
            &execution_plan.mint,
        ) {
            Ok(build) => build,
            Err(error) => return line.skip(tx_build_error_reason(error)),
        };
        if build.route_layout != "direct-pump" {
            return line.skip("unsupported copy execution layout");
        }

        let blockhash = match Hash::from_str(&cached_blockhash.blockhash) {
            Ok(blockhash) => blockhash,
            Err(error) => return line.skip(format!("invalid cached blockhash: {error}")),
        };

        let tx = Transaction::new_signed_with_payer(
            &build.instructions,
            Some(&keypair.pubkey()),
            &[keypair],
            blockhash,
        );
        let tx_bytes = match bincode::serialize(&tx) {
            Ok(bytes) => bytes,
            Err(error) => return line.error(format!("serialize signed transaction: {error}")),
        };
        let encoded_tx = STANDARD.encode(tx_bytes);

        line.signed = true;
        line.mark_signed();
        line.copy_signature = tx.signatures.first().map(ToString::to_string);
        line.blockhash = Some(cached_blockhash.blockhash);
        line.route_layout = Some(build.route_layout);
        line.instruction_count = build.instructions.len();

        let mut simulation_ok = true;
        if self.options.simulate_copy_tx {
            match self.simulate_transaction(&encoded_tx).await {
                Ok(simulation) => {
                    line.simulated = true;
                    line.mark_simulation_completed();
                    line.simulation_error = simulation.err;
                    line.simulation_units_consumed = simulation.units_consumed;
                    line.simulation_logs = simulation.logs.unwrap_or_default();
                    simulation_ok = line.simulation_error.is_none();
                }
                Err(error) => {
                    simulation_ok = false;
                    line.simulated = true;
                    line.mark_simulation_completed();
                    line.simulation_error = Some(serde_json::Value::String(error));
                }
            }
        }

        if self.options.enable_copy_send {
            if self.options.dry_run {
                return line.skip("dry run blocks copy send");
            }
            if self.options.simulate_copy_tx && !simulation_ok {
                return line.skip("simulation failed; send blocked");
            }
            line.mark_send_submitted();
            match self.send_transaction(&encoded_tx).await {
                Ok(signature) => {
                    line.sent = true;
                    line.mark_signature_returned();
                    line.send_signature = Some(signature);
                    line.decision = "sent";
                    line
                }
                Err(error) => line.error(error),
            }
        } else if self.options.simulate_copy_tx {
            if simulation_ok {
                line.decision = "simulated";
                line
            } else {
                line.error("simulation failed")
            }
        } else {
            line.skip("copy send is disabled")
        }
    }

    async fn simulate_transaction(&self, encoded_tx: &str) -> Result<SimulationValue, String> {
        let rpc_url = self
            .options
            .solana_rpc_url
            .as_deref()
            .ok_or_else(|| "missing SOLANA_RPC_URL".to_string())?;
        let response = self
            .client
            .post(rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "simulateTransaction",
                "params": [
                    encoded_tx,
                    {
                        "encoding": "base64",
                        "sigVerify": true,
                        "replaceRecentBlockhash": false,
                        "commitment": "processed"
                    }
                ]
            }))
            .send()
            .await
            .map_err(|error| format!("send simulateTransaction request: {error}"))?
            .error_for_status()
            .map_err(|error| format!("simulateTransaction HTTP status: {error}"))?
            .json::<RpcResponse<SimulationResult>>()
            .await
            .map_err(|error| format!("decode simulateTransaction response: {error}"))?;

        if let Some(error) = response.error {
            return Err(format!("simulateTransaction RPC error: {}", error.message));
        }

        response
            .result
            .map(|result| result.value)
            .ok_or_else(|| "simulateTransaction result missing".to_string())
    }

    async fn send_transaction(&self, encoded_tx: &str) -> Result<String, String> {
        let rpc_url = self
            .options
            .solana_rpc_url
            .as_deref()
            .ok_or_else(|| "missing SOLANA_RPC_URL".to_string())?;
        let response = self
            .client
            .post(rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "sendTransaction",
                "params": [
                    encoded_tx,
                    {
                        "encoding": "base64",
                        "skipPreflight": false,
                        "preflightCommitment": "processed",
                        "maxRetries": 0
                    }
                ]
            }))
            .send()
            .await
            .map_err(|error| format!("send sendTransaction request: {error}"))?
            .error_for_status()
            .map_err(|error| format!("sendTransaction HTTP status: {error}"))?
            .json::<RpcResponse<String>>()
            .await
            .map_err(|error| format!("decode sendTransaction response: {error}"))?;

        if let Some(error) = response.error {
            return Err(format!("sendTransaction RPC error: {}", error.message));
        }

        response
            .result
            .ok_or_else(|| "sendTransaction result missing".to_string())
    }
}

impl CopyExecutionLine {
    fn new(
        execution_plan: &ExecutionPlanLine,
        observed_action: Action,
        observed_sol_amount: Option<f64>,
        options: &CopyExecutionOptions,
    ) -> Self {
        Self {
            schema: "copytrade.localExecution.v1",
            observed_at_ms: execution_plan.observed_at_ms,
            execution_at_ms: now_ms(),
            provider: "shredstream",
            source: "jito-proxy",
            endpoint: execution_plan.endpoint.clone(),
            observed_wallet: execution_plan.target_wallet.clone(),
            copy_wallet: options.copy_wallet.clone(),
            observed_signature: execution_plan.signature.clone(),
            slot: execution_plan.slot,
            selected_route: execution_plan.route,
            mint: execution_plan.mint.clone(),
            observed_action,
            observed_sol_amount,
            max_copy_sol: options.max_copy_sol,
            send_enabled: options.enable_copy_send,
            dry_run: options.dry_run,
            simulation_requested: options.simulate_copy_tx,
            signed: false,
            simulated: false,
            sent: false,
            decision: "skip",
            signed_at_ms: None,
            simulation_completed_at_ms: None,
            send_submitted_at_ms: None,
            signature_returned_at_ms: None,
            observed_to_signed_ms: None,
            observed_to_simulation_completed_ms: None,
            observed_to_send_submitted_ms: None,
            observed_to_signature_returned_ms: None,
            route_layout: None,
            instruction_count: 0,
            copy_signature: None,
            blockhash: None,
            simulation_error: None,
            simulation_units_consumed: None,
            simulation_logs: Vec::new(),
            send_signature: None,
            reason: None,
        }
    }

    fn skip(mut self, reason: impl Into<String>) -> Self {
        self.decision = "skip";
        self.reason = Some(reason.into());
        self
    }

    fn error(mut self, reason: impl Into<String>) -> Self {
        self.decision = "error";
        self.reason = Some(reason.into());
        self
    }

    fn mark_signed(&mut self) {
        let timestamp = now_ms();
        self.signed_at_ms = Some(timestamp);
        self.observed_to_signed_ms = Some(timestamp.saturating_sub(self.observed_at_ms));
    }

    fn mark_simulation_completed(&mut self) {
        let timestamp = now_ms();
        self.simulation_completed_at_ms = Some(timestamp);
        self.observed_to_simulation_completed_ms =
            Some(timestamp.saturating_sub(self.observed_at_ms));
    }

    fn mark_send_submitted(&mut self) {
        let timestamp = now_ms();
        self.send_submitted_at_ms = Some(timestamp);
        self.observed_to_send_submitted_ms = Some(timestamp.saturating_sub(self.observed_at_ms));
    }

    fn mark_signature_returned(&mut self) {
        let timestamp = now_ms();
        self.signature_returned_at_ms = Some(timestamp);
        self.observed_to_signature_returned_ms =
            Some(timestamp.saturating_sub(self.observed_at_ms));
    }
}

fn tx_build_error_reason(error: TxBuildError) -> &'static str {
    match error {
        TxBuildError::MissingRouteContext(reason)
        | TxBuildError::UnsupportedLayout(reason)
        | TxBuildError::InvalidInstruction(reason) => reason,
    }
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct SimulationResult {
    value: SimulationValue,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimulationValue {
    err: Option<serde_json::Value>,
    logs: Option<Vec<String>>,
    units_consumed: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{execution_plan_line, PlannerOptions};

    const COPY_WALLET: &str = "FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W";

    fn disabled_options() -> CopyExecutionOptions {
        CopyExecutionOptions {
            enable_copy_send: false,
            dry_run: true,
            simulate_copy_tx: false,
            max_copy_sol: None,
            copy_wallet: None,
            copy_keypair_path: None,
            solana_rpc_url: None,
        }
    }

    fn allowed_plan() -> ExecutionPlanLine {
        execution_plan_line(
            &crate::event::ShadowSignalLine {
                schema: "copytrade.shadowSignal.v1",
                observed_at_ms: 1,
                provider: "shredstream",
                source: "jito-proxy",
                endpoint: "local".to_string(),
                target_wallet: "target".to_string(),
                action: Action::Buy,
                mint: "abcPumpIsNotLowerpump".to_string(),
                signature: "observed".to_string(),
                slot: 2,
                route: Route::FlashxPump,
                sol_amount: Some(0.0005),
                token_amount: None,
                copyable: true,
                decision: "wouldCopy",
                reason: None,
                account_key_count: 1,
                route_context: None,
            },
            3,
            PlannerOptions {
                copy_sol_amount: None,
            },
        )
    }

    fn executor(options: CopyExecutionOptions) -> CopyExecutor {
        CopyExecutor {
            options,
            keypair: None,
            client: reqwest::Client::new(),
            blockhash_cache: None,
        }
    }

    #[test]
    fn execution_line_defaults_to_safe_disabled_state() {
        let plan = allowed_plan();

        let line = CopyExecutionLine::new(&plan, Action::Buy, Some(0.0005), &disabled_options());

        assert_eq!(line.schema, "copytrade.localExecution.v1");
        assert!(!line.send_enabled);
        assert!(line.dry_run);
        assert!(!line.signed);
        assert!(!line.sent);
        assert_eq!(line.signed_at_ms, None);
        assert_eq!(line.observed_to_signed_ms, None);
        assert_eq!(line.observed_to_signature_returned_ms, None);
    }

    #[test]
    fn max_copy_sol_first_live_cap_is_one_milli_sol() {
        assert_eq!(FIRST_LIVE_MAX_COPY_SOL_CAP, 0.001);
    }

    #[tokio::test]
    async fn disabled_executor_skips_before_signing() {
        let line = executor(disabled_options())
            .handle(&allowed_plan(), Action::Buy, Some(0.0005))
            .await;

        assert_eq!(line.decision, "skip");
        assert_eq!(line.reason.as_deref(), Some("copy execution is disabled"));
        assert!(!line.signed);
        assert!(!line.sent);
    }

    #[tokio::test]
    async fn max_copy_sol_cap_blocks_unsafe_first_live_value() {
        let mut options = disabled_options();
        options.simulate_copy_tx = true;
        options.max_copy_sol = Some(0.01);
        options.copy_wallet = Some(COPY_WALLET.to_string());

        let line = executor(options)
            .handle(&allowed_plan(), Action::Buy, Some(0.0005))
            .await;

        assert_eq!(line.decision, "skip");
        assert_eq!(
            line.reason.as_deref(),
            Some("max copy SOL guard exceeds first-live cap 0.001")
        );
        assert!(!line.signed);
        assert!(!line.sent);
    }

    #[tokio::test]
    async fn observed_amount_above_guard_blocks_before_keypair() {
        let mut options = disabled_options();
        options.simulate_copy_tx = true;
        options.max_copy_sol = Some(0.0004);
        options.copy_wallet = Some(COPY_WALLET.to_string());

        let line = executor(options)
            .handle(&allowed_plan(), Action::Buy, Some(0.0005))
            .await;

        assert_eq!(line.decision, "skip");
        assert_eq!(
            line.reason.as_deref(),
            Some("observed spend exceeds max copy SOL guard")
        );
        assert!(!line.signed);
        assert!(!line.sent);
    }

    #[tokio::test]
    async fn dry_run_blocks_send_even_when_send_flag_is_enabled() {
        let mut options = disabled_options();
        options.enable_copy_send = true;
        options.dry_run = true;
        options.max_copy_sol = Some(0.0005);
        options.copy_wallet = Some(COPY_WALLET.to_string());

        let line = executor(options)
            .handle(&allowed_plan(), Action::Buy, Some(0.0005))
            .await;

        assert_eq!(line.decision, "skip");
        assert_eq!(line.reason.as_deref(), Some("missing copy keypair path"));
        assert!(!line.sent);
    }
}
