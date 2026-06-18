use crate::{
    address_lookup::AddressLookupTableCache,
    balance_cache::WalletBalanceCache,
    blockhash::spawn_blockhash_cache,
    event::{
        normalized_event_from_raw, now_ms, print_json, wallet_mention_schema,
        NormalizedCopyTradeEvent, RejectionLine, ShadowSignalLine, WalletMentionLine,
    },
    executor::{CopyExecutionOutput, CopyExecutor, TrailingSellPlan},
    parser::{
        classify_wallet_mention, parse_trade_for_mentioned_targets, signature_bytes_to_string,
        versioned_tx_signature_bytes, versioned_tx_signature_string, Action,
    },
    planner::{
        copy_tx_plan_line, tx_build_plan_line, unsigned_tx_plan_line, CopyRuntimeRequest,
        CopyTxPlanLine, CopyTxPlannerOptions, ExecutionPlanLine, PlannerOptions, TxBuildPlanLine,
        TxBuildPlannerOptions, UnsignedTxPlanLine, UnsignedTxPlannerOptions,
    },
    proto::jito_shredstream::{
        shredstream_proxy_client::ShredstreamProxyClient, SubscribeEntriesRequest,
    },
    signal::{SignalObservationWriter, SignalTimings},
    telegram_snapshot::TelegramSnapshotConfig,
    LiveOptions,
};
use anyhow::{Context, Result};
use arc_swap::ArcSwapOption;
use serde::Serialize;
use solana_pubkey::Pubkey;
use std::{
    collections::{HashSet, VecDeque},
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, Mutex};

struct TelegramRuntimeConfig {
    snapshot: TelegramSnapshotConfig,
    target_wallet_pubkey_set: HashSet<Pubkey>,
}

type SharedTelegramRuntime = Arc<ArcSwapOption<TelegramRuntimeConfig>>;

pub(crate) async fn run(options: LiveOptions) -> Result<()> {
    let telegram_runtime = load_telegram_runtime(
        options.telegram_snapshot_path.as_deref(),
        options.copy_wallet.as_deref(),
    )?;
    let fallback_target_wallets = parse_target_wallets(&options.target_wallets)?;
    let fallback_target_wallet_pubkey_set =
        parse_target_wallet_pubkey_set(&fallback_target_wallets)?;
    let target_wallet_count = telegram_runtime
        .as_ref()
        .map(|runtime| runtime.target_wallet_pubkey_set.len())
        .unwrap_or(fallback_target_wallet_pubkey_set.len());
    let telegram_runtime = Arc::new(ArcSwapOption::from(telegram_runtime.map(Arc::new)));
    let state_rpc_urls = options.normalized_state_rpc_urls();
    let address_lookup_tables =
        AddressLookupTableCache::load(&state_rpc_urls, &options.address_lookup_tables)
            .await
            .context("preload address lookup tables")?;
    let blockhash_cache = spawn_blockhash_cache(
        state_rpc_urls.clone(),
        options.blockhash_refresh_ms,
        options.stats,
    );
    let wallet_balance_cache =
        wallet_balance_cache_from_options(&options, telegram_runtime.load_full().as_deref());
    if let Some(cache) = &wallet_balance_cache {
        cache.replace_wallets(active_copy_wallets(
            telegram_runtime.load_full().as_deref(),
            options.copy_wallet.as_deref(),
        ));
        match cache.refresh_once().await {
            Ok(count) if options.stats => {
                eprintln!("preloaded copy wallet balances; wallets={count}");
            }
            Ok(_) => {}
            Err(error) => eprintln!("initial copy wallet balance refresh failed: {error}"),
        }
        cache.spawn_refresh_loop();
    }
    let copy_executor = Arc::new(CopyExecutor::from_options(
        &options,
        blockhash_cache.clone(),
        address_lookup_tables.clone(),
        wallet_balance_cache.clone(),
        telegram_runtime
            .load_full()
            .as_ref()
            .map(|runtime| runtime.snapshot.signer_keypair_paths())
            .unwrap_or_default(),
    )?);
    copy_executor.warm_send_endpoints_once().await;
    Arc::clone(&copy_executor).spawn_send_endpoint_warmer();
    spawn_telegram_snapshot_reloader(
        Arc::clone(&telegram_runtime),
        Arc::clone(&copy_executor),
        options.telegram_snapshot_path.clone(),
        options.copy_wallet.clone(),
        options.telegram_snapshot_reload_ms,
        wallet_balance_cache,
    );
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
        options.endpoint, target_wallet_count, options.limit
    );
    if let Some(runtime) = telegram_runtime.load_full().as_ref() {
        eprintln!(
            "loaded Telegram Jito snapshot sequence={}; activeCopyTargets={}",
            runtime.snapshot.sequence(),
            runtime.snapshot.active_profile_count()
        );
    }

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
    let mut copy_executions = CopyExecutionWriter::new(
        options.copy_executions_path.as_deref(),
        options.copy_executions_flush_each_write,
    )
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
    let (copy_execution_request_tx, copy_execution_request_rx) =
        mpsc::channel(nonzero_capacity(options.copy_execution_queue_capacity));
    spawn_copy_execution_workers(
        Arc::clone(&copy_executor),
        copy_execution_request_rx,
        copy_execution_tx.clone(),
        options.copy_execution_concurrency,
    );
    let signal_side_effect_tx = spawn_signal_side_effect_worker(
        SignalObservationWriter::from_options(&options)?,
        options.print_feed_events,
        options.signal_observation_queue_capacity,
    );
    let mut emitted = 0usize;

    loop {
        let slot_entry = tokio::select! {
            copy_execution = copy_execution_rx.recv() => {
                if let Some(copy_execution) = copy_execution {
                    if handle_copy_execution_result(
                        &mut copy_executions,
                        copy_execution,
                        &copy_executor,
                        &copy_execution_tx,
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

                let tx_parse_started_at = Instant::now();
                let wallet_match_started_at = tx_parse_started_at;
                let telegram_runtime_guard = telegram_runtime.load();
                let active_target_wallet_pubkey_set = telegram_runtime_guard
                    .as_ref()
                    .map(|runtime| &runtime.target_wallet_pubkey_set)
                    .unwrap_or(&fallback_target_wallet_pubkey_set);
                let static_account_keys = versioned_tx.message.static_account_keys();
                let static_account_key_count = static_account_keys.len();
                let target_wallet_mention = mentioned_static_target_wallet_in_set(
                    static_account_keys,
                    active_target_wallet_pubkey_set,
                );
                let wallet_match_finished_at = Instant::now();
                let wallet_match_finished_at_ms = now_ms();
                if target_wallet_mention.is_none() {
                    if options.include_rejections {
                        let signature = versioned_tx_signature_string(&versioned_tx);
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

                let signature_bytes = versioned_tx_signature_bytes(&versioned_tx);
                if !seen.insert(signature_bytes) {
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
                let account_keys = expanded_account_keys.as_slice();

                let route_parse_started_at = account_expand_finished_at;
                match parse_trade_for_mentioned_targets(
                    &versioned_tx,
                    &account_keys,
                    active_target_wallet_pubkey_set,
                ) {
                    Some(parsed) => {
                        let trade_parsed_at = Instant::now();
                        let trade_parsed_at_ms = now_ms();
                        let timings = SignalTimings {
                            grpc_message_received_at_ms,
                            entries_deserialized_at_ms,
                            wallet_match_finished_at_ms,
                            trade_parsed_at_ms,
                            deserialize_us: entries_deserialized_at
                                .duration_since(grpc_message_received_at)
                                .as_micros(),
                            wallet_match_finished_at_us: wallet_match_finished_at
                                .duration_since(entries_deserialized_at)
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
                        let signature = versioned_tx_signature_bytes(&versioned_tx);
                        let target_wallet = parsed.target_wallet;
                        let mint = parsed.mint;
                        let observed_action = parsed.action;
                        let observed_sol_amount = parsed.sol_amount;
                        let token_amount = parsed.token_amount;
                        let route = parsed.route;
                        if parsed.action == Action::Sell {
                            copy_executor.observe_direct_pump_sell_route_context(
                                &target_wallet,
                                &mint,
                                parsed.route_context.as_ref(),
                            );
                        }
                        let telegram_target_configs = telegram_runtime_guard
                            .as_ref()
                            .map(|runtime| {
                                runtime.snapshot.target_configs_for_pubkey(&target_wallet)
                            })
                            .unwrap_or(&[]);
                        let mut parsed_for_runtime = Some(parsed);
                        if telegram_target_configs.is_empty() {
                            let runtime_request = CopyRuntimeRequest::from_parsed_trade(
                                trade_parsed_at_ms,
                                now_ms(),
                                signature,
                                slot_entry.slot,
                                account_keys.len(),
                                parsed_for_runtime
                                    .take()
                                    .expect("parsed trade available for runtime request"),
                                PlannerOptions {
                                    copy_sol_amount: options.copy_plan_sol_amount,
                                },
                            );
                            if options.fast_copy_send {
                                enqueue_copy_execution(
                                    &copy_execution_request_tx,
                                    runtime_request,
                                    timings,
                                    None,
                                    None,
                                );
                            } else {
                                let shadow_signal =
                                    runtime_request.to_shadow_signal_line(options.endpoint.clone());
                                let execution_plan = runtime_request
                                    .to_execution_plan_line(options.endpoint.clone());
                                enqueue_copy_execution(
                                    &copy_execution_request_tx,
                                    runtime_request,
                                    timings,
                                    None,
                                    None,
                                );
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
                        } else {
                            let telegram_target_config_count = telegram_target_configs.len();
                            for (index, telegram_target_config) in
                                telegram_target_configs.iter().enumerate()
                            {
                                let parsed_for_request =
                                    if index + 1 == telegram_target_config_count {
                                        parsed_for_runtime.take().expect(
                                            "parsed trade available for final runtime request",
                                        )
                                    } else {
                                        parsed_for_runtime
                                            .as_ref()
                                            .expect("parsed trade available for runtime request")
                                            .clone()
                                    };
                                let runtime_request = CopyRuntimeRequest::from_parsed_trade(
                                    trade_parsed_at_ms,
                                    now_ms(),
                                    signature,
                                    slot_entry.slot,
                                    account_keys.len(),
                                    parsed_for_request,
                                    PlannerOptions {
                                        copy_sol_amount: Some(
                                            telegram_target_config.copy_amount_sol,
                                        ),
                                    },
                                );
                                if options.fast_copy_send {
                                    enqueue_copy_execution(
                                        &copy_execution_request_tx,
                                        runtime_request,
                                        timings,
                                        Some(telegram_target_config.copy_wallet),
                                        telegram_target_config.trailing_sell.clone(),
                                    );
                                } else {
                                    let shadow_signal = runtime_request
                                        .to_shadow_signal_line(options.endpoint.clone());
                                    let execution_plan = runtime_request
                                        .to_execution_plan_line(options.endpoint.clone());
                                    enqueue_copy_execution(
                                        &copy_execution_request_tx,
                                        runtime_request,
                                        timings,
                                        Some(telegram_target_config.copy_wallet),
                                        telegram_target_config.trailing_sell.clone(),
                                    );
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
                            }
                        }

                        let event = normalized_event_from_raw(
                            trade_parsed_at_ms,
                            options.endpoint.clone(),
                            signature_bytes_to_string(signature),
                            slot_entry.slot,
                            account_keys.len(),
                            target_wallet,
                            observed_action,
                            mint,
                            route,
                            observed_sol_amount,
                            token_amount,
                        );
                        enqueue_signal_side_effect(&signal_side_effect_tx, event, timings);
                        emitted += 1;
                        if options.limit > 0 && emitted >= options.limit {
                            return Ok(());
                        }
                        if drain_copy_execution_results(
                            &mut copy_execution_rx,
                            &mut copy_executions,
                            &copy_executor,
                            &copy_execution_tx,
                            options.one_shot_copy_send,
                        )? {
                            eprintln!("one-shot copy send completed; exiting");
                            return Ok(());
                        }
                    }
                    None if options.include_rejections => {
                        let signature = versioned_tx_signature_string(&versioned_tx);
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
                            let signature = versioned_tx_signature_string(&versioned_tx);
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
            &copy_executor,
            &copy_execution_tx,
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
    set: HashSet<[u8; 64]>,
    order: VecDeque<[u8; 64]>,
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
    flush_each_write: bool,
}

struct CopyExecutionRequest {
    runtime_request: CopyRuntimeRequest,
    timings: SignalTimings,
    executor_enqueued_at: Instant,
    copy_wallet: Option<Pubkey>,
    trailing_sell_plan: Option<TrailingSellPlan>,
}

fn spawn_copy_execution_workers(
    copy_executor: Arc<CopyExecutor>,
    copy_execution_request_rx: mpsc::Receiver<CopyExecutionRequest>,
    copy_execution_tx: mpsc::UnboundedSender<CopyExecutionOutput>,
    concurrency: usize,
) {
    let worker_count = nonzero_capacity(concurrency);
    let copy_execution_request_rx = Arc::new(Mutex::new(copy_execution_request_rx));
    for _ in 0..worker_count {
        let copy_executor = Arc::clone(&copy_executor);
        let copy_execution_request_rx = Arc::clone(&copy_execution_request_rx);
        let copy_execution_tx = copy_execution_tx.clone();
        tokio::spawn(async move {
            loop {
                let request = {
                    let mut copy_execution_request_rx = copy_execution_request_rx.lock().await;
                    copy_execution_request_rx.recv().await
                };
                let Some(request) = request else {
                    break;
                };
                handle_copy_execution_request(
                    Arc::clone(&copy_executor),
                    request,
                    copy_execution_tx.clone(),
                )
                .await;
            }
        });
    }
}

async fn handle_copy_execution_request(
    copy_executor: Arc<CopyExecutor>,
    request: CopyExecutionRequest,
    copy_execution_tx: mpsc::UnboundedSender<CopyExecutionOutput>,
) {
    let copy_execution = copy_executor
        .handle_with_executor_enqueued_at(
            &request.runtime_request,
            request.timings,
            request.executor_enqueued_at,
            request.copy_wallet,
            Some(copy_execution_tx.clone()),
        )
        .await;
    let rust_trailing_sell_line = copy_executor
        .should_spawn_trailing_sells_after_buy(&copy_execution, request.trailing_sell_plan.as_ref())
        .then(|| copy_execution.clone());
    let auto_sell_line = (rust_trailing_sell_line.is_none()
        && copy_executor.should_spawn_auto_sell_after_buy(&copy_execution))
    .then(|| copy_execution.clone());
    let target_sell_auto_sell_line = copy_executor
        .should_spawn_auto_sell_on_target_sell(&request.runtime_request)
        .then(|| copy_execution.clone());
    let execution_plan = (rust_trailing_sell_line.is_some()
        || auto_sell_line.is_some()
        || target_sell_auto_sell_line.is_some())
    .then(|| {
        request
            .runtime_request
            .to_execution_plan_line(copy_executor.endpoint().to_string())
    });
    if copy_execution_tx
        .send(CopyExecutionOutput::Copy(copy_execution))
        .is_err()
    {
        eprintln!("copy execution result dropped; receiver closed");
        return;
    }
    if let (Some(buy_line), Some(trailing_sell_plan)) =
        (rust_trailing_sell_line, request.trailing_sell_plan.clone())
    {
        let trailing_sell_executor = Arc::clone(&copy_executor);
        let trailing_sell_tx = copy_execution_tx.clone();
        let execution_plan = execution_plan
            .as_ref()
            .expect("execution plan exists for trailing sell")
            .clone();
        tokio::spawn(async move {
            trailing_sell_executor
                .handle_trailing_sell_results(
                    buy_line,
                    execution_plan,
                    trailing_sell_plan,
                    trailing_sell_tx,
                )
                .await;
        });
    }
    if let Some(auto_sell_line) = auto_sell_line {
        let auto_sell_executor = Arc::clone(&copy_executor);
        let auto_sell_tx = copy_execution_tx.clone();
        let execution_plan = execution_plan
            .as_ref()
            .expect("execution plan exists for auto-sell")
            .clone();
        tokio::spawn(async move {
            let auto_sell_result = auto_sell_executor
                .handle_auto_sell_result(auto_sell_line, &execution_plan)
                .await;
            if auto_sell_tx
                .send(CopyExecutionOutput::Copy(auto_sell_result))
                .is_err()
            {
                eprintln!("copy auto-sell result dropped; receiver closed");
            }
        });
    }
    if let Some(auto_sell_line) = target_sell_auto_sell_line {
        let auto_sell_executor = Arc::clone(&copy_executor);
        let auto_sell_tx = copy_execution_tx.clone();
        let execution_plan = execution_plan.expect("execution plan exists for target auto-sell");
        tokio::spawn(async move {
            let auto_sell_result = auto_sell_executor
                .handle_target_sell_auto_sell_result(auto_sell_line, &execution_plan)
                .await;
            if auto_sell_tx
                .send(CopyExecutionOutput::Copy(auto_sell_result))
                .is_err()
            {
                eprintln!("target-sell auto-sell result dropped; receiver closed");
            }
        });
    }
}

fn enqueue_copy_execution(
    copy_execution_request_tx: &mpsc::Sender<CopyExecutionRequest>,
    runtime_request: CopyRuntimeRequest,
    timings: SignalTimings,
    copy_wallet: Option<Pubkey>,
    trailing_sell_plan: Option<TrailingSellPlan>,
) -> bool {
    if copy_execution_request_tx
        .try_send(CopyExecutionRequest {
            runtime_request,
            timings,
            executor_enqueued_at: Instant::now(),
            copy_wallet,
            trailing_sell_plan,
        })
        .is_ok()
    {
        true
    } else {
        eprintln!("copy execution request dropped; worker closed or queue full");
        false
    }
}

fn spawn_telegram_snapshot_reloader(
    telegram_runtime: SharedTelegramRuntime,
    copy_executor: Arc<CopyExecutor>,
    telegram_snapshot_path: Option<std::path::PathBuf>,
    copy_wallet: Option<String>,
    reload_ms: u64,
    wallet_balance_cache: Option<WalletBalanceCache>,
) {
    let Some(telegram_snapshot_path) = telegram_snapshot_path else {
        return;
    };
    if reload_ms == 0 {
        return;
    }

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(reload_ms));
        interval.tick().await;
        loop {
            interval.tick().await;
            let runtime = match load_telegram_runtime(
                Some(telegram_snapshot_path.as_path()),
                copy_wallet.as_deref(),
            ) {
                Ok(Some(runtime)) => runtime,
                Ok(None) => continue,
                Err(error) => {
                    eprintln!("Telegram Jito snapshot reload failed: {error:#}");
                    continue;
                }
            };

            let current_fingerprint = telegram_runtime
                .load()
                .as_ref()
                .map(|runtime| runtime.snapshot.fingerprint());
            if current_fingerprint == Some(runtime.snapshot.fingerprint()) {
                continue;
            }

            let keypairs =
                CopyExecutor::load_snapshot_keypairs(runtime.snapshot.signer_keypair_paths());
            if let Some(cache) = &wallet_balance_cache {
                cache.replace_wallets(active_copy_wallets(Some(&runtime), copy_wallet.as_deref()));
                if let Err(error) = cache.refresh_once().await {
                    eprintln!("copy wallet balance refresh after snapshot reload failed: {error}");
                }
            }
            let sequence = runtime.snapshot.sequence();
            let target_wallet_count = runtime.target_wallet_pubkey_set.len();
            let active_profile_count = runtime.snapshot.active_profile_count();
            copy_executor.replace_snapshot_keypairs(keypairs);
            telegram_runtime.store(Some(Arc::new(runtime)));
            eprintln!(
                "reloaded Telegram Jito snapshot sequence={}; wallets={}; activeCopyTargets={}",
                sequence, target_wallet_count, active_profile_count
            );
        }
    });
}

fn load_telegram_runtime(
    path: Option<&Path>,
    copy_wallet: Option<&str>,
) -> Result<Option<TelegramRuntimeConfig>> {
    let Some(snapshot) = TelegramSnapshotConfig::load(path, copy_wallet)? else {
        return Ok(None);
    };
    let target_wallet_pubkey_set = parse_target_wallet_pubkey_set(&snapshot.target_wallets())?;
    Ok(Some(TelegramRuntimeConfig {
        snapshot,
        target_wallet_pubkey_set,
    }))
}

fn wallet_balance_cache_from_options(
    options: &LiveOptions,
    runtime: Option<&TelegramRuntimeConfig>,
) -> Option<WalletBalanceCache> {
    if !options.copy_wallet_balance_guard {
        return None;
    }
    let rpc_urls = options.normalized_state_rpc_urls();
    if rpc_urls.is_empty() {
        return None;
    }
    Some(WalletBalanceCache::new(
        rpc_urls,
        options.copy_wallet_balance_refresh_ms,
        options.copy_wallet_balance_stale_ms,
        options.send_http_timeout_ms,
        active_copy_wallets(runtime, options.copy_wallet.as_deref()),
    ))
}

fn active_copy_wallets(
    runtime: Option<&TelegramRuntimeConfig>,
    fallback_copy_wallet: Option<&str>,
) -> Vec<String> {
    let mut wallets = runtime
        .map(|runtime| runtime.snapshot.active_copy_wallets())
        .unwrap_or_default();
    if wallets.is_empty() {
        if let Some(copy_wallet) = fallback_copy_wallet
            .map(str::trim)
            .filter(|wallet| !wallet.is_empty())
        {
            wallets.push(copy_wallet.to_string());
        }
    }
    wallets.sort();
    wallets.dedup();
    wallets
}

struct SignalSideEffectRequest {
    event: NormalizedCopyTradeEvent,
    timings: SignalTimings,
}

fn spawn_signal_side_effect_worker(
    writer: Option<SignalObservationWriter>,
    print_feed_events: bool,
    queue_capacity: usize,
) -> Option<mpsc::Sender<SignalSideEffectRequest>> {
    if writer.is_none() && !print_feed_events {
        return None;
    }

    let (tx, mut rx) = mpsc::channel::<SignalSideEffectRequest>(nonzero_capacity(queue_capacity));
    tokio::spawn(async move {
        let mut writer = writer;
        while let Some(request) = rx.recv().await {
            if let Some(writer) = &mut writer {
                if let Err(error) = writer.write(&request.event, request.timings).await {
                    eprintln!("signal observation write failed: {error:#}");
                }
            }
            if print_feed_events {
                if let Err(error) = print_json(&request.event) {
                    eprintln!("feed event print failed: {error:#}");
                }
            }
        }
    });

    Some(tx)
}

fn enqueue_signal_side_effect(
    signal_side_effect_tx: &Option<mpsc::Sender<SignalSideEffectRequest>>,
    event: NormalizedCopyTradeEvent,
    timings: SignalTimings,
) {
    let Some(tx) = signal_side_effect_tx else {
        return;
    };

    if tx
        .try_send(SignalSideEffectRequest { event, timings })
        .is_err()
    {
        eprintln!("signal side-effect request dropped; worker closed or queue full");
    }
}

fn nonzero_capacity(value: usize) -> usize {
    value.max(1)
}

fn drain_copy_execution_results(
    copy_execution_rx: &mut mpsc::UnboundedReceiver<CopyExecutionOutput>,
    copy_executions: &mut CopyExecutionWriter,
    copy_executor: &Arc<CopyExecutor>,
    copy_execution_tx: &mpsc::UnboundedSender<CopyExecutionOutput>,
    one_shot_copy_send: bool,
) -> Result<bool> {
    while let Ok(copy_execution) = copy_execution_rx.try_recv() {
        if handle_copy_execution_result(
            copy_executions,
            copy_execution,
            copy_executor,
            copy_execution_tx,
            one_shot_copy_send,
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn handle_copy_execution_result(
    copy_executions: &mut CopyExecutionWriter,
    copy_execution: CopyExecutionOutput,
    copy_executor: &Arc<CopyExecutor>,
    copy_execution_tx: &mpsc::UnboundedSender<CopyExecutionOutput>,
    one_shot_copy_send: bool,
) -> Result<bool> {
    let one_shot_sent = one_shot_copy_send && copy_execution.was_sent();
    copy_executions.write(&copy_execution)?;
    enqueue_transaction_confirmation(copy_executor, copy_execution_tx, &copy_execution);
    Ok(one_shot_sent)
}

fn enqueue_transaction_confirmation(
    copy_executor: &Arc<CopyExecutor>,
    copy_execution_tx: &mpsc::UnboundedSender<CopyExecutionOutput>,
    copy_execution: &CopyExecutionOutput,
) {
    match copy_execution {
        CopyExecutionOutput::Copy(line) => {
            if line.was_sent() {
                let copy_executor = Arc::clone(copy_executor);
                let copy_execution_tx = copy_execution_tx.clone();
                let line = line.clone();
                tokio::spawn(async move {
                    let confirmation = copy_executor.confirm_copy_transaction(line).await;
                    if copy_execution_tx
                        .send(CopyExecutionOutput::TransactionConfirmation(confirmation))
                        .is_err()
                    {
                        eprintln!("copy confirmation result dropped; receiver closed");
                    }
                });
            }
            if line.auto_sell_was_sent() {
                let copy_executor = Arc::clone(copy_executor);
                let copy_execution_tx = copy_execution_tx.clone();
                let line = line.clone();
                tokio::spawn(async move {
                    let confirmation = copy_executor.confirm_auto_sell_transaction(line).await;
                    if copy_execution_tx
                        .send(CopyExecutionOutput::TransactionConfirmation(confirmation))
                        .is_err()
                    {
                        eprintln!("auto-sell confirmation result dropped; receiver closed");
                    }
                });
            }
        }
        CopyExecutionOutput::RustTrailingSell(line) if line.was_sent() => {
            let copy_executor = Arc::clone(copy_executor);
            let copy_execution_tx = copy_execution_tx.clone();
            let line = line.clone();
            tokio::spawn(async move {
                let confirmation = copy_executor
                    .confirm_rust_trailing_sell_transaction(line)
                    .await;
                if copy_execution_tx
                    .send(CopyExecutionOutput::TransactionConfirmation(confirmation))
                    .is_err()
                {
                    eprintln!("rust trailing sell confirmation result dropped; receiver closed");
                }
            });
        }
        CopyExecutionOutput::SendLaneAttribution(_) => {}
        _ => {}
    }
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
    fn new(path: Option<&Path>, flush_each_write: bool) -> Result<Self> {
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

        Ok(Self {
            file,
            flush_each_write,
        })
    }

    fn write(&mut self, line: &CopyExecutionOutput) -> Result<()> {
        line.write_json_line(self.file.as_mut(), self.flush_each_write)
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

    fn insert(&mut self, signature: [u8; 64]) -> bool {
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

        self.set.insert(signature);
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

fn parse_target_wallet_pubkey_set(values: &[String]) -> Result<HashSet<Pubkey>> {
    values
        .iter()
        .map(|wallet| Pubkey::from_str(wallet))
        .collect::<std::result::Result<HashSet<_>, _>>()
        .context("parse target wallet pubkeys")
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
    use super::{
        enqueue_copy_execution, mentioned_static_target_wallet_in_set, write_json_line,
        SeenSignatures,
    };
    use crate::{
        parser::{Action, Route},
        planner::CopyRuntimeRequest,
        signal::SignalTimings,
    };
    use serde::Serialize;
    use solana_pubkey::Pubkey;
    use std::{
        collections::HashSet,
        fs::{remove_file, File},
        io::BufWriter,
        path::PathBuf,
        str::FromStr,
    };
    use tokio::sync::mpsc;

    #[test]
    fn seen_signatures_evicts_oldest_when_capacity_is_reached() {
        let mut seen = SeenSignatures::new(2);

        assert!(seen.insert(signature_key(1)));
        assert!(seen.insert(signature_key(2)));
        assert!(!seen.insert(signature_key(1)));
        assert_eq!(seen.len(), 2);

        assert!(seen.insert(signature_key(3)));
        assert_eq!(seen.len(), 2);
        assert!(seen.insert(signature_key(1)));
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn seen_signatures_capacity_zero_disables_dedupe() {
        let mut seen = SeenSignatures::new(0);

        assert!(seen.insert(signature_key(1)));
        assert!(seen.insert(signature_key(1)));
        assert_eq!(seen.len(), 0);
    }

    fn signature_key(value: u8) -> [u8; 64] {
        [value; 64]
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
    fn copy_execution_enqueue_drops_when_bounded_lane_is_full() {
        let (tx, _rx) = mpsc::channel(1);

        assert!(enqueue_copy_execution(
            &tx,
            sample_runtime_request(1),
            sample_timings(),
            None,
            None,
        ));
        assert!(!enqueue_copy_execution(
            &tx,
            sample_runtime_request(2),
            sample_timings(),
            None,
            None,
        ));
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

    fn sample_runtime_request(slot: u64) -> CopyRuntimeRequest {
        CopyRuntimeRequest {
            observed_at_ms: slot as u128,
            planned_at_ms: slot as u128,
            target_wallet: Pubkey::from_str("CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o")
                .unwrap(),
            signature: [slot as u8; 64],
            slot,
            route: Route::Pump,
            mint: Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap(),
            observed_action: Action::Buy,
            observed_sol_amount: Some(0.001),
            token_amount: None,
            account_key_count: 1,
            planned_copy_sol_amount: Some(0.001),
            allowed: true,
            reason: None,
            route_context: None,
        }
    }

    fn sample_timings() -> SignalTimings {
        SignalTimings {
            grpc_message_received_at_ms: 0,
            entries_deserialized_at_ms: 0,
            wallet_match_finished_at_ms: 0,
            trade_parsed_at_ms: 0,
            deserialize_us: 0,
            wallet_match_finished_at_us: 0,
            parse_us: 0,
            local_detect_us: 0,
            batch_transaction_count: 1,
            matched_transaction_index: 0,
            batch_scan_us: 0,
            tx_parse_us: 0,
            account_expand_us: 0,
            wallet_match_us: 0,
            route_parse_us: 0,
        }
    }

    fn temp_path(file_name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("{}-{file_name}", std::process::id()));
        remove_file(&path).ok();
        path
    }
}
