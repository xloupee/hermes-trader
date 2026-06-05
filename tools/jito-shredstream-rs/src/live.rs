use crate::{
    address_lookup::AddressLookupTableCache,
    blockhash::spawn_blockhash_cache,
    event::{
        normalized_event, now_ms, print_json, shadow_signal_line, wallet_mention_schema,
        RejectionLine, ShadowSignalLine, WalletMentionLine,
    },
    executor::{CopyExecutionLine, CopyExecutor},
    parser::{
        classify_wallet_mention, parse_trade_for_mentioned_targets, versioned_tx_signature_string,
    },
    planner::{
        copy_tx_plan_line, execution_plan_line, tx_build_plan_line, unsigned_tx_plan_line,
        CopyTxPlanLine, CopyTxPlannerOptions, ExecutionPlanLine, PlannerOptions, TxBuildPlanLine,
        TxBuildPlannerOptions, UnsignedTxPlanLine, UnsignedTxPlannerOptions,
    },
    proto::jito_shredstream::{
        shredstream_proxy_client::ShredstreamProxyClient, SubscribeEntriesRequest,
    },
    signal::{SignalObservationWriter, SignalTimings},
    LiveOptions,
};
use anyhow::{Context, Result};
use serde::Serialize;
use solana_pubkey::Pubkey;
use std::{
    collections::{HashSet, VecDeque},
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    str::FromStr,
    sync::Arc,
    time::Instant,
};
use tokio::sync::mpsc;

pub(crate) async fn run(options: LiveOptions) -> Result<()> {
    let target_wallets = parse_target_wallets(&options.target_wallets)?;
    let target_wallet_set = target_wallets.iter().cloned().collect::<HashSet<_>>();
    let target_wallet_pubkey_set = target_wallets
        .iter()
        .map(|wallet| Pubkey::from_str(wallet))
        .collect::<std::result::Result<HashSet<_>, _>>()
        .context("parse target wallet pubkeys")?;
    let address_lookup_tables = AddressLookupTableCache::load(
        options.solana_rpc_url.as_deref(),
        &options.address_lookup_tables,
    )
    .await
    .context("preload address lookup tables")?;
    let blockhash_cache = spawn_blockhash_cache(
        options.solana_rpc_url.clone(),
        options.blockhash_refresh_ms,
        options.stats,
    );
    let copy_executor = Arc::new(CopyExecutor::from_options(
        &options,
        blockhash_cache.clone(),
        address_lookup_tables.clone(),
    )?);
    copy_executor.warm_send_endpoints_once().await;
    Arc::clone(&copy_executor).spawn_send_endpoint_warmer();
    let mut client = ShredstreamProxyClient::connect(options.endpoint.clone())
        .await
        .with_context(|| format!("connect to {}", options.endpoint))?;
    let mut stream = client
        .subscribe_entries(SubscribeEntriesRequest {})
        .await
        .context("subscribe to Jito ShredStream entries")?
        .into_inner();

    eprintln!(
        "subscribed to Jito ShredStream proxy {}; wallets={}; limit={}",
        options.endpoint,
        target_wallets.len(),
        options.limit
    );

    let mut seen = SeenSignatures::new(options.dedupe_capacity);
    let mut shadow_signals = ShadowSignalWriter::new(options.shadow_signals_path.as_deref())
        .with_context(|| {
            format!(
                "open shadow signals path {}",
                options
                    .shadow_signals_path
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(disabled)".to_string())
            )
        })?;
    let mut execution_plans = ExecutionPlanWriter::new(options.execution_plans_path.as_deref())
        .with_context(|| {
            format!(
                "open execution plans path {}",
                options
                    .execution_plans_path
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(disabled)".to_string())
            )
        })?;
    let mut tx_build_plans = TxBuildPlanWriter::new(options.tx_build_plans_path.as_deref())
        .with_context(|| {
            format!(
                "open tx build plans path {}",
                options
                    .tx_build_plans_path
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(disabled)".to_string())
            )
        })?;
    let mut copy_tx_plans = CopyTxPlanWriter::new(options.copy_tx_plans_path.as_deref())
        .with_context(|| {
            format!(
                "open copy tx plans path {}",
                options
                    .copy_tx_plans_path
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(disabled)".to_string())
            )
        })?;
    let mut unsigned_tx_plans = UnsignedTxPlanWriter::new(
        options.unsigned_tx_plans_path.as_deref(),
    )
    .with_context(|| {
        format!(
            "open unsigned tx plans path {}",
            options
                .unsigned_tx_plans_path
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "(disabled)".to_string())
        )
    })?;
    let mut copy_executions = CopyExecutionWriter::new(options.copy_executions_path.as_deref())
        .with_context(|| {
            format!(
                "open copy executions path {}",
                options
                    .copy_executions_path
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(disabled)".to_string())
            )
        })?;
    let (copy_execution_tx, mut copy_execution_rx) = mpsc::unbounded_channel();
    let mut signal_observations = SignalObservationWriter::from_options(&options)?;
    let mut emitted = 0usize;

    loop {
        let slot_entry = tokio::select! {
            copy_execution = copy_execution_rx.recv() => {
                if let Some(copy_execution) = copy_execution {
                    if handle_copy_execution_result(
                        &mut copy_executions,
                        copy_execution,
                        options.one_shot_copy_send,
                    )? {
                        eprintln!("one-shot copy send completed; exiting");
                        return Ok(());
                    }
                }
                continue;
            }
            slot_entry = stream.message() => {
                match slot_entry.context("receive Jito ShredStream entry")? {
                    Some(slot_entry) => slot_entry,
                    None => break,
                }
            }
        };

        let grpc_message_received_at = Instant::now();
        let grpc_message_received_at_ms = now_ms();
        let entries =
            match bincode::deserialize::<Vec<solana_entry::entry::Entry>>(&slot_entry.entries) {
                Ok(entries) => entries,
                Err(error) => {
                    if options.include_rejections {
                        print_json(&RejectionLine {
                            schema: "copytrade.feed.rejection.v1",
                            observed_at_ms: now_ms(),
                            provider: "shredstream",
                            source: "jito-proxy",
                            endpoint: options.endpoint.clone(),
                            signature: "unknown-entry".to_string(),
                            slot: slot_entry.slot,
                            reason: format!("entry deserialize failed: {error}"),
                            filters: vec!["jito-entry".to_string()],
                            account_key_count: 0,
                        })?;
                    }
                    continue;
                }
            };
        let entries_deserialized_at = Instant::now();
        let entries_deserialized_at_ms = now_ms();
        let batch_transaction_count = entries
            .iter()
            .map(|entry| entry.transactions.len())
            .sum::<usize>();

        if options.stats {
            eprintln!(
                "slot {} entries={} transactions={}",
                slot_entry.slot,
                entries.len(),
                batch_transaction_count
            );
        }

        let mut batch_transaction_index = 0usize;
        for entry in entries {
            for versioned_tx in entry.transactions {
                let current_transaction_index = batch_transaction_index;
                batch_transaction_index += 1;

                let signature = versioned_tx_signature_string(&versioned_tx);
                if signature.is_empty() || !seen.insert(signature.clone()) {
                    continue;
                }

                let tx_parse_started_at = Instant::now();
                let wallet_match_started_at = tx_parse_started_at;
                let static_account_keys = versioned_tx.message.static_account_keys();
                let static_account_key_count = static_account_keys.len();
                let target_wallet_mention = mentioned_static_target_wallet_in_set(
                    static_account_keys,
                    &target_wallet_pubkey_set,
                );
                let wallet_match_finished_at = Instant::now();
                if target_wallet_mention.is_none() {
                    if options.include_rejections {
                        print_json(&RejectionLine {
                            schema: "copytrade.feed.rejection.v1",
                            observed_at_ms: now_ms(),
                            provider: "shredstream",
                            source: "jito-proxy",
                            endpoint: options.endpoint.clone(),
                            signature,
                            slot: slot_entry.slot,
                            reason: "no target wallet in static account keys".to_string(),
                            filters: vec!["jito-entry".to_string()],
                            account_key_count: static_account_key_count,
                        })?;
                    }
                    continue;
                }

                let account_expand_started_at = wallet_match_finished_at;
                let expanded_account_keys =
                    address_lookup_tables.expanded_account_keys(&versioned_tx);
                let account_expand_finished_at = Instant::now();
                if let Some(missing_lookup_table) = &expanded_account_keys.missing_lookup_table {
                    if options.stats {
                        eprintln!("missing address lookup table {missing_lookup_table}");
                    }
                }
                let account_keys = expanded_account_keys.keys;

                let route_parse_started_at = account_expand_finished_at;
                match parse_trade_for_mentioned_targets(
                    &versioned_tx,
                    &account_keys,
                    &target_wallet_set,
                ) {
                    Some(parsed) => {
                        let trade_parsed_at = Instant::now();
                        let trade_parsed_at_ms = now_ms();
                        let timings = SignalTimings {
                            grpc_message_received_at_ms,
                            entries_deserialized_at_ms,
                            trade_parsed_at_ms,
                            deserialize_us: entries_deserialized_at
                                .duration_since(grpc_message_received_at)
                                .as_micros(),
                            parse_us: trade_parsed_at
                                .duration_since(entries_deserialized_at)
                                .as_micros(),
                            local_detect_us: trade_parsed_at
                                .duration_since(grpc_message_received_at)
                                .as_micros(),
                            batch_transaction_count: batch_transaction_count as u64,
                            matched_transaction_index: current_transaction_index as u64,
                            batch_scan_us: tx_parse_started_at
                                .duration_since(entries_deserialized_at)
                                .as_micros(),
                            tx_parse_us: trade_parsed_at
                                .duration_since(tx_parse_started_at)
                                .as_micros(),
                            account_expand_us: account_expand_finished_at
                                .duration_since(account_expand_started_at)
                                .as_micros(),
                            wallet_match_us: wallet_match_finished_at
                                .duration_since(wallet_match_started_at)
                                .as_micros(),
                            route_parse_us: trade_parsed_at
                                .duration_since(route_parse_started_at)
                                .as_micros(),
                        };
                        let shadow_signal = shadow_signal_line(
                            trade_parsed_at_ms,
                            options.endpoint.clone(),
                            signature.clone(),
                            slot_entry.slot,
                            account_keys.len(),
                            &parsed,
                        );
                        let execution_plan = execution_plan_line(
                            &shadow_signal,
                            now_ms(),
                            PlannerOptions {
                                copy_sol_amount: options.copy_plan_sol_amount,
                            },
                        );
                        spawn_copy_execution(
                            Arc::clone(&copy_executor),
                            copy_execution_tx.clone(),
                            execution_plan.clone(),
                            parsed.action,
                            parsed.sol_amount,
                        );
                        if !options.fast_copy_send {
                            write_plan_outputs(
                                &mut shadow_signals,
                                &mut execution_plans,
                                &mut tx_build_plans,
                                &mut copy_tx_plans,
                                &mut unsigned_tx_plans,
                                &shadow_signal,
                                &execution_plan,
                                &options,
                            )?;
                        }

                        let event = normalized_event(
                            trade_parsed_at_ms,
                            options.endpoint.clone(),
                            signature,
                            slot_entry.slot,
                            account_keys.len(),
                            parsed,
                        );
                        if let Some(writer) = &mut signal_observations {
                            if let Err(error) = writer.write(&event, timings).await {
                                eprintln!("signal observation write failed: {error:#}");
                            }
                        }

                        if options.print_feed_events {
                            print_json(&event)?;
                        }
                        emitted += 1;
                        if options.limit > 0 && emitted >= options.limit {
                            return Ok(());
                        }
                        if drain_copy_execution_results(
                            &mut copy_execution_rx,
                            &mut copy_executions,
                            options.one_shot_copy_send,
                        )? {
                            eprintln!("one-shot copy send completed; exiting");
                            return Ok(());
                        }
                    }
                    None if options.include_rejections => {
                        print_json(&RejectionLine {
                            schema: "copytrade.feed.rejection.v1",
                            observed_at_ms: now_ms(),
                            provider: "shredstream",
                            source: "jito-proxy",
                            endpoint: options.endpoint.clone(),
                            signature,
                            slot: slot_entry.slot,
                            reason: "no supported target Pump instruction in static account keys"
                                .to_string(),
                            filters: vec!["jito-entry".to_string()],
                            account_key_count: account_keys.len(),
                        })?;
                    }
                    None if options.print_mentions => {
                        if let Some(target_wallet) = target_wallet_mention {
                            let classification =
                                classify_wallet_mention(&versioned_tx, &account_keys);
                            print_json(&WalletMentionLine {
                                schema: wallet_mention_schema(classification.kind),
                                observed_at_ms: now_ms(),
                                provider: "shredstream",
                                source: "jito-proxy",
                                endpoint: options.endpoint.clone(),
                                target_wallet,
                                signature,
                                slot: slot_entry.slot,
                                reason: classification.reason,
                                account_key_count: account_keys.len(),
                            })?;
                        }
                    }
                    None => {}
                }
            }
        }
        if drain_copy_execution_results(
            &mut copy_execution_rx,
            &mut copy_executions,
            options.one_shot_copy_send,
        )? {
            eprintln!("one-shot copy send completed; exiting");
            return Ok(());
        }
    }

    Ok(())
}

struct SeenSignatures {
    capacity: usize,
    set: HashSet<String>,
    order: VecDeque<String>,
}

struct ShadowSignalWriter {
    file: Option<BufWriter<File>>,
}

struct ExecutionPlanWriter {
    file: Option<BufWriter<File>>,
}

struct TxBuildPlanWriter {
    file: Option<BufWriter<File>>,
}

struct CopyTxPlanWriter {
    file: Option<BufWriter<File>>,
}

struct UnsignedTxPlanWriter {
    file: Option<BufWriter<File>>,
}

struct CopyExecutionWriter {
    file: Option<BufWriter<File>>,
}

fn spawn_copy_execution(
    copy_executor: Arc<CopyExecutor>,
    copy_execution_tx: mpsc::UnboundedSender<CopyExecutionLine>,
    execution_plan: ExecutionPlanLine,
    observed_action: crate::parser::Action,
    observed_sol_amount: Option<f64>,
) {
    tokio::spawn(async move {
        let copy_execution = copy_executor
            .handle(&execution_plan, observed_action, observed_sol_amount)
            .await;
        if copy_execution_tx.send(copy_execution).is_err() {
            eprintln!("copy execution result dropped; receiver closed");
        }
    });
}

fn drain_copy_execution_results(
    copy_execution_rx: &mut mpsc::UnboundedReceiver<CopyExecutionLine>,
    copy_executions: &mut CopyExecutionWriter,
    one_shot_copy_send: bool,
) -> Result<bool> {
    while let Ok(copy_execution) = copy_execution_rx.try_recv() {
        if handle_copy_execution_result(copy_executions, copy_execution, one_shot_copy_send)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn handle_copy_execution_result(
    copy_executions: &mut CopyExecutionWriter,
    copy_execution: CopyExecutionLine,
    one_shot_copy_send: bool,
) -> Result<bool> {
    let one_shot_sent = one_shot_copy_send && copy_execution.was_sent();
    copy_executions.write(&copy_execution)?;
    Ok(one_shot_sent)
}

impl ShadowSignalWriter {
    fn new(path: Option<&Path>) -> Result<Self> {
        let file = match path {
            Some(path) => Some(BufWriter::new(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .with_context(|| format!("open {}", path.display()))?,
            )),
            None => None,
        };

        Ok(Self { file })
    }

    fn write(&mut self, signal: &ShadowSignalLine) -> Result<()> {
        write_json_line(self.file.as_mut(), signal)
    }
}

impl ExecutionPlanWriter {
    fn new(path: Option<&Path>) -> Result<Self> {
        let file = match path {
            Some(path) => Some(BufWriter::new(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .with_context(|| format!("open {}", path.display()))?,
            )),
            None => None,
        };

        Ok(Self { file })
    }

    fn write(&mut self, plan: &ExecutionPlanLine) -> Result<()> {
        write_json_line(self.file.as_mut(), plan)
    }
}

impl TxBuildPlanWriter {
    fn new(path: Option<&Path>) -> Result<Self> {
        let file = match path {
            Some(path) => Some(BufWriter::new(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .with_context(|| format!("open {}", path.display()))?,
            )),
            None => None,
        };

        Ok(Self { file })
    }

    fn write(&mut self, plan: &TxBuildPlanLine) -> Result<()> {
        write_json_line(self.file.as_mut(), plan)
    }
}

impl CopyTxPlanWriter {
    fn new(path: Option<&Path>) -> Result<Self> {
        let file = match path {
            Some(path) => Some(BufWriter::new(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .with_context(|| format!("open {}", path.display()))?,
            )),
            None => None,
        };

        Ok(Self { file })
    }

    fn write(&mut self, plan: &CopyTxPlanLine) -> Result<()> {
        write_json_line(self.file.as_mut(), plan)
    }
}

impl UnsignedTxPlanWriter {
    fn new(path: Option<&Path>) -> Result<Self> {
        let file = match path {
            Some(path) => Some(BufWriter::new(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .with_context(|| format!("open {}", path.display()))?,
            )),
            None => None,
        };

        Ok(Self { file })
    }

    fn write(&mut self, plan: &UnsignedTxPlanLine) -> Result<()> {
        write_json_line(self.file.as_mut(), plan)
    }
}

fn write_plan_outputs(
    shadow_signals: &mut ShadowSignalWriter,
    execution_plans: &mut ExecutionPlanWriter,
    tx_build_plans: &mut TxBuildPlanWriter,
    copy_tx_plans: &mut CopyTxPlanWriter,
    unsigned_tx_plans: &mut UnsignedTxPlanWriter,
    shadow_signal: &ShadowSignalLine,
    execution_plan: &ExecutionPlanLine,
    options: &LiveOptions,
) -> Result<()> {
    shadow_signals.write(shadow_signal)?;
    execution_plans.write(execution_plan)?;
    tx_build_plans.write(&tx_build_plan_line(
        execution_plan,
        now_ms(),
        TxBuildPlannerOptions {
            max_plan_age_ms: options.tx_build_plan_max_age_ms,
        },
    ))?;
    copy_tx_plans.write(&copy_tx_plan_line(
        execution_plan,
        now_ms(),
        CopyTxPlannerOptions {
            max_plan_age_ms: options.tx_build_plan_max_age_ms,
            copy_wallet: options.copy_wallet.clone(),
        },
    ))?;
    unsigned_tx_plans.write(&unsigned_tx_plan_line(
        execution_plan,
        now_ms(),
        UnsignedTxPlannerOptions {
            max_plan_age_ms: options.tx_build_plan_max_age_ms,
            copy_wallet: options.copy_wallet.clone(),
            simulate_copy_tx: options.simulate_copy_tx && !options.fast_copy_send,
        },
    ))?;
    Ok(())
}

impl CopyExecutionWriter {
    fn new(path: Option<&Path>) -> Result<Self> {
        let file = match path {
            Some(path) => Some(BufWriter::new(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .with_context(|| format!("open {}", path.display()))?,
            )),
            None => None,
        };

        Ok(Self { file })
    }

    fn write(&mut self, line: &CopyExecutionLine) -> Result<()> {
        write_json_line(self.file.as_mut(), line)
    }
}

fn write_json_line<T: Serialize>(writer: Option<&mut BufWriter<File>>, value: &T) -> Result<()> {
    let Some(writer) = writer else {
        return Ok(());
    };

    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

impl SeenSignatures {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            set: HashSet::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
        }
    }

    fn insert(&mut self, signature: String) -> bool {
        if self.capacity == 0 {
            return true;
        }

        if self.set.contains(&signature) {
            return false;
        }

        while self.order.len() >= self.capacity {
            if let Some(expired) = self.order.pop_front() {
                self.set.remove(&expired);
            } else {
                break;
            }
        }

        self.set.insert(signature.clone());
        self.order.push_back(signature);
        true
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.set.len()
    }
}

fn parse_target_wallets(values: &[String]) -> Result<Vec<String>> {
    let mut wallets = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        Pubkey::from_str(trimmed).with_context(|| format!("invalid target wallet {trimmed}"))?;
        wallets.push(trimmed.to_string());
    }

    if wallets.is_empty() {
        anyhow::bail!("provide at least one --target-wallet");
    }

    wallets.sort();
    wallets.dedup();
    Ok(wallets)
}

fn mentioned_static_target_wallet_in_set(
    account_keys: &[Pubkey],
    target_wallets: &HashSet<Pubkey>,
) -> Option<String> {
    account_keys
        .iter()
        .find(|account_key| target_wallets.contains(account_key))
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::{mentioned_static_target_wallet_in_set, write_json_line, SeenSignatures};
    use serde::Serialize;
    use solana_pubkey::Pubkey;
    use std::{
        collections::HashSet,
        fs::{remove_file, File},
        io::BufWriter,
        path::PathBuf,
        str::FromStr,
    };

    #[test]
    fn seen_signatures_evicts_oldest_when_capacity_is_reached() {
        let mut seen = SeenSignatures::new(2);

        assert!(seen.insert("a".to_string()));
        assert!(seen.insert("b".to_string()));
        assert!(!seen.insert("a".to_string()));
        assert_eq!(seen.len(), 2);

        assert!(seen.insert("c".to_string()));
        assert_eq!(seen.len(), 2);
        assert!(seen.insert("a".to_string()));
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn seen_signatures_capacity_zero_disables_dedupe() {
        let mut seen = SeenSignatures::new(0);

        assert!(seen.insert("a".to_string()));
        assert!(seen.insert("a".to_string()));
        assert_eq!(seen.len(), 0);
    }

    #[test]
    fn write_json_line_appends_one_serialized_record() {
        #[derive(Serialize)]
        struct Record {
            schema: &'static str,
            decision: &'static str,
        }

        let path = temp_path("jito-shadow-signal-writer.jsonl");
        let file = File::create(&path).expect("temp file creates");
        let mut writer = BufWriter::new(file);

        write_json_line(
            Some(&mut writer),
            &Record {
                schema: "copytrade.shadowSignal.v1",
                decision: "wouldCopy",
            },
        )
        .expect("line writes");

        let contents = std::fs::read_to_string(&path).expect("temp file reads");
        remove_file(&path).ok();

        assert_eq!(
            contents,
            "{\"schema\":\"copytrade.shadowSignal.v1\",\"decision\":\"wouldCopy\"}\n"
        );
    }

    #[test]
    fn static_target_wallet_match_uses_pubkeys_without_stringifying_all_accounts() {
        let target = Pubkey::from_str("CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o")
            .expect("valid target wallet");
        let other = Pubkey::from_str("11111111111111111111111111111111").expect("valid pubkey");
        let mut target_wallets = HashSet::new();
        target_wallets.insert(target);

        assert_eq!(
            mentioned_static_target_wallet_in_set(&[other, target], &target_wallets),
            Some(target.to_string())
        );
        assert_eq!(
            mentioned_static_target_wallet_in_set(&[other], &target_wallets),
            None
        );
    }

    fn temp_path(file_name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("{}-{file_name}", std::process::id()));
        remove_file(&path).ok();
        path
    }
}
