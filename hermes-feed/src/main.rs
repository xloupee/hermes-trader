use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use alloy_primitives::{Address, U256};
use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use futures_util::StreamExt;
use hermes_feed::feed::BroadcastMessage;
use hermes_feed::{
    Candidate, FeedDecoder, Filter, FrameReport, PairSnapshot, PaperPolicy, ReserveBook,
    SequenceObservation, SequenceTracker, V2SnapshotClient,
};
use serde::{Deserialize, Serialize};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Read and measure a live Nitro websocket feed.
    Probe(ProbeArgs),
    /// Replay newline-delimited frames recorded by `probe`.
    Replay(ReplayArgs),
    /// Compare matching sequence arrival times from two or more probe logs.
    Compare(CompareArgs),
    /// Evaluate captured V2 candidates without signing or submitting transactions.
    Paper(PaperArgs),
    /// Fetch a block-consistent V2 reserve snapshot for one token path.
    Snapshot(SnapshotArgs),
    /// Apply a block-consistent reserve snapshot to captured paper candidates.
    Simulate(SimulateArgs),
    /// Summarize one probe log, excluding warmup frames.
    Summarize(SummarizeArgs),
}

#[derive(Debug, Args)]
struct SimulateArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    snapshot: PathBuf,
    #[arg(long)]
    max_amount_in: String,
    #[arg(long, default_value_t = 3)]
    max_path_len: usize,
    #[arg(long, default_value_t = 2)]
    deadline_grace_seconds: u64,
}

#[derive(Debug, Args)]
struct SnapshotArgs {
    #[arg(long, default_value = "https://rpc.mainnet.chain.robinhood.com")]
    rpc_url: String,
    #[arg(long, default_value = "0x8bceaa40b9acdfaedf85adf4ff01f5ad6517937f")]
    factory: String,
    /// Token address in path order. Repeat for every token.
    #[arg(long = "token", required = true)]
    path: Vec<String>,
}

#[derive(Debug, Args)]
struct PaperArgs {
    /// JSONL containing candidate records emitted by the probe.
    #[arg(long)]
    input: PathBuf,
    /// Maximum simulated input amount, in decimal or 0x-prefixed base units.
    #[arg(long)]
    max_amount_in: String,
    #[arg(long, default_value_t = 3)]
    max_path_len: usize,
    #[arg(long, default_value_t = 2)]
    deadline_grace_seconds: u64,
}

#[derive(Debug, Args)]
struct ProbeArgs {
    /// Official local Nitro relay by default; set the Robinhood WSS URL for a direct trial.
    #[arg(long, default_value = "ws://127.0.0.1:9642")]
    url: String,
    #[arg(long, default_value = "relay-local")]
    source: String,
    /// Mark frames during this per-connection interval as warmup/catch-up.
    #[arg(long, default_value_t = 10)]
    warmup_seconds: u64,
    /// Append replayable frames without blocking the socket task.
    #[arg(long)]
    record: Option<PathBuf>,
    #[command(flatten)]
    filter: FilterArgs,
}

#[derive(Debug, Args)]
struct ReplayArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long, default_value = "replay")]
    source: String,
    #[command(flatten)]
    filter: FilterArgs,
}

#[derive(Debug, Args)]
struct CompareArgs {
    /// JSONL stdout captured from a probe. Repeat for every region/source.
    #[arg(long = "input", required = true)]
    inputs: Vec<PathBuf>,
    /// Refuse to choose a winner before this many exact sequences overlap.
    #[arg(long, default_value_t = 10_000)]
    min_matched_sequences: usize,
    /// Clock correction as SOURCE=REMOTE_MINUS_REFERENCE_NS. Repeatable.
    #[arg(long = "clock-offset")]
    clock_offsets: Vec<String>,
    /// Refuse a winner unless its p95 lead exceeds this clock-error bound.
    #[arg(long, default_value_t = 0)]
    max_clock_uncertainty_ns: u128,
}

#[derive(Debug, Args)]
struct SummarizeArgs {
    #[arg(long)]
    input: PathBuf,
}

#[derive(Debug, Args, Default)]
struct FilterArgs {
    /// Recover signers only for transactions sent to these routers. Repeatable.
    #[arg(long = "router")]
    routers: Vec<String>,
    /// Recover signers only for these four-byte calldata selectors. Repeatable.
    #[arg(long = "selector")]
    selectors: Vec<String>,
    /// Emit candidates only from these wallets. Repeatable.
    #[arg(long = "watch")]
    watched_wallets: Vec<String>,
    /// Emit every decoded transaction hash for offline differential testing.
    #[arg(long)]
    emit_tx_hashes: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct RecordedFrame {
    received_unix_ns: u128,
    payload: String,
}

#[derive(Debug, Deserialize)]
struct ComparableFrame {
    record_type: String,
    source: String,
    received_unix_ns: u128,
    #[serde(default)]
    warmup: bool,
    #[serde(default)]
    sequence_numbers: Vec<u64>,
    #[serde(default)]
    sequence: SequenceObservation,
}

#[derive(Debug, Deserialize)]
struct CandidateRecord {
    #[allow(dead_code)]
    record_type: String,
    source: String,
    candidate: Candidate,
}

#[derive(Debug, Deserialize)]
struct SnapshotRecord {
    record_type: String,
    pairs: Vec<PairSnapshot>,
}

#[derive(Debug, Deserialize)]
struct SummaryFrame {
    record_type: String,
    source: String,
    received_unix_ns: u128,
    #[serde(default)]
    warmup: bool,
    #[serde(default)]
    reconnects: u64,
    #[serde(default)]
    frame_bytes: u64,
    #[serde(default)]
    json_ns: u64,
    #[serde(default)]
    base64_ns: u64,
    #[serde(default)]
    l2_walk_ns: u64,
    #[serde(default)]
    envelope_decode_ns: u64,
    #[serde(default)]
    filter_ns: u64,
    #[serde(default)]
    feed_messages: u64,
    #[serde(default)]
    signed_transactions: u64,
    #[serde(default)]
    candidates: u64,
    #[serde(default)]
    unsupported_l1_messages: u64,
    #[serde(default)]
    unsupported_l2_messages: u64,
    #[serde(default)]
    sequence_numbers: Vec<u64>,
    #[serde(default)]
    sequence: SequenceObservation,
}

#[derive(Debug, Serialize)]
struct ProbeSummary {
    record_type: &'static str,
    source: String,
    first_live_unix_ns: u128,
    last_live_unix_ns: u128,
    duration_seconds: f64,
    warmup_frames_excluded: u64,
    live_frames: u64,
    live_sequences: u64,
    feed_messages: u64,
    signed_transactions: u64,
    candidates: u64,
    bytes: u64,
    sequence: SequenceObservation,
    reconnects: u64,
    connection_errors: u64,
    unsupported_l1_messages: u64,
    unsupported_l2_messages: u64,
    sequences_per_second: f64,
    average_local_ns_per_feed_message: u128,
    total_local_ns: NsQuantiles,
    json_ns: NsQuantiles,
    base64_ns: NsQuantiles,
    l2_walk_ns: NsQuantiles,
    envelope_decode_ns: NsQuantiles,
    filter_ns: NsQuantiles,
}

#[derive(Debug, Default, Serialize)]
struct NsQuantiles {
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

#[derive(Debug, Serialize)]
struct Comparison {
    record_type: &'static str,
    matched_sequences: usize,
    minimum_matched_sequences: usize,
    decision_ready: bool,
    max_clock_uncertainty_ns: u128,
    winner: Option<String>,
    sources: Vec<SourceComparison>,
}

#[derive(Debug, Clone, Serialize)]
struct SourceComparison {
    source: String,
    clock_offset_ns: i128,
    eligible: bool,
    samples: usize,
    wins: usize,
    gaps: u64,
    missing: u64,
    duplicates_or_reordered: u64,
    lag_ns_p50: u128,
    lag_ns_p95: u128,
    lag_ns_p99: u128,
    lag_ns_max: u128,
}

struct OutputSink {
    tx: Option<SyncSender<String>>,
    writer: Option<thread::JoinHandle<()>>,
}

impl OutputSink {
    fn stdout() -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel::<String>(8_192);
        let writer = thread::spawn(move || {
            let stdout = std::io::stdout();
            let mut output = BufWriter::new(stdout.lock());
            while let Ok(line) = rx.recv() {
                if writeln!(output, "{line}").is_err() {
                    break;
                }
            }
            let _ = output.flush();
        });
        Self {
            tx: Some(tx),
            writer: Some(writer),
        }
    }

    fn emit<T: Serialize>(&self, value: &T) -> Result<()> {
        let line = serde_json::to_string(value)?;
        match self
            .tx
            .as_ref()
            .expect("output sink is live")
            .try_send(line)
        {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                anyhow::bail!("stdout queue full; measurements incomplete")
            }
            Err(TrySendError::Disconnected(_)) => anyhow::bail!("stdout writer disconnected"),
        }
    }
}

impl Drop for OutputSink {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Probe(args) => probe(args).await,
        Command::Replay(args) => replay(args).await,
        Command::Compare(args) => compare(args),
        Command::Paper(args) => paper(args),
        Command::Snapshot(args) => snapshot(args).await,
        Command::Simulate(args) => simulate(args),
        Command::Summarize(args) => summarize(args),
    }
}

fn simulate(args: SimulateArgs) -> Result<()> {
    let max_amount_in = parse_u256(&args.max_amount_in)
        .with_context(|| format!("invalid --max-amount-in {}", args.max_amount_in))?;
    if max_amount_in == U256::ZERO {
        anyhow::bail!("--max-amount-in must be greater than zero");
    }
    let snapshot_file = std::fs::File::open(&args.snapshot)
        .with_context(|| format!("open reserve snapshot {}", args.snapshot.display()))?;
    let snapshot: SnapshotRecord = serde_json::from_reader(snapshot_file)
        .with_context(|| format!("decode reserve snapshot {}", args.snapshot.display()))?;
    if snapshot.record_type != "v2_reserve_snapshot" {
        anyhow::bail!("snapshot input is not a v2_reserve_snapshot record");
    }
    let minimum_snapshot_block = snapshot
        .pairs
        .iter()
        .map(|pair| pair.block_number)
        .min()
        .context("reserve snapshot has no pairs")?;
    let reserves = ReserveBook::from_snapshots(snapshot.pairs)?;
    let policy = PaperPolicy {
        max_amount_in,
        max_path_len: args.max_path_len,
        deadline_grace_seconds: args.deadline_grace_seconds,
    };
    let input = std::fs::File::open(&args.input)
        .with_context(|| format!("open simulation input {}", args.input.display()))?;
    for (index, line) in std::io::BufReader::new(input).lines().enumerate() {
        let line = line?;
        let value: serde_json::Value = serde_json::from_str(&line)
            .with_context(|| format!("parse JSON at simulation input line {}", index + 1))?;
        if value.get("record_type").and_then(|kind| kind.as_str()) != Some("candidate") {
            continue;
        }
        let record: CandidateRecord = serde_json::from_value(value)
            .with_context(|| format!("decode candidate at simulation input line {}", index + 1))?;
        let decision = policy.evaluate_with_reserves(
            record.candidate.v2_swap.as_ref(),
            record.candidate.l1_timestamp,
            &reserves,
            minimum_snapshot_block,
        );
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "record_type": "reserve_paper_decision",
                "source": record.source,
                "sequence_number": record.candidate.sequence_number,
                "observed_tx_hash": record.candidate.tx_hash,
                "decision": decision,
            }))?
        );
    }
    Ok(())
}

async fn snapshot(args: SnapshotArgs) -> Result<()> {
    let factory = Address::from_str(&args.factory)
        .with_context(|| format!("invalid --factory {}", args.factory))?;
    let path: Vec<_> = args
        .path
        .iter()
        .map(|token| Address::from_str(token).with_context(|| format!("invalid --token {token}")))
        .collect::<Result<_>>()?;
    let snapshots = V2SnapshotClient::new(args.rpc_url)?
        .fetch_path(factory, &path)
        .await?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "record_type": "v2_reserve_snapshot",
            "factory": factory,
            "path": path,
            "pairs": snapshots,
        }))?
    );
    Ok(())
}

fn paper(args: PaperArgs) -> Result<()> {
    let max_amount_in = parse_u256(&args.max_amount_in)
        .with_context(|| format!("invalid --max-amount-in {}", args.max_amount_in))?;
    if max_amount_in == U256::ZERO {
        anyhow::bail!("--max-amount-in must be greater than zero");
    }
    if args.max_path_len < 2 {
        anyhow::bail!("--max-path-len must be at least 2");
    }
    let policy = PaperPolicy {
        max_amount_in,
        max_path_len: args.max_path_len,
        deadline_grace_seconds: args.deadline_grace_seconds,
    };
    let stdin = std::io::stdin();
    let input: Box<dyn BufRead + '_> = if args.input == Path::new("-") {
        Box::new(stdin.lock())
    } else {
        let file = std::fs::File::open(&args.input)
            .with_context(|| format!("open paper input {}", args.input.display()))?;
        Box::new(std::io::BufReader::new(file))
    };
    for (index, line) in input.lines().enumerate() {
        let line = line?;
        let value: serde_json::Value = serde_json::from_str(&line)
            .with_context(|| format!("parse JSON at paper input line {}", index + 1))?;
        if value.get("record_type").and_then(|kind| kind.as_str()) != Some("candidate") {
            continue;
        }
        let record: CandidateRecord = serde_json::from_value(value)
            .with_context(|| format!("decode candidate at paper input line {}", index + 1))?;
        let decision = policy.evaluate(
            record.candidate.v2_swap.as_ref(),
            record.candidate.l1_timestamp,
        );
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "record_type": "paper_decision",
                "source": record.source,
                "sequence_number": record.candidate.sequence_number,
                "observed_tx_hash": record.candidate.tx_hash,
                "decision": decision,
            }))?
        );
    }
    Ok(())
}

fn parse_u256(value: &str) -> Result<U256> {
    if let Some(hex) = value.strip_prefix("0x") {
        Ok(U256::from_str_radix(hex, 16)?)
    } else {
        Ok(U256::from_str(value)?)
    }
}

async fn probe(args: ProbeArgs) -> Result<()> {
    let mut decoder = FeedDecoder::new(parse_filter(args.filter)?);
    let mut sequences = SequenceTracker::default();
    let output = OutputSink::stdout();
    let recorder = match args.record {
        Some(path) => Some(start_recorder(&path).await?),
        None => None,
    };
    let mut reconnects = 0_u64;
    let mut backoff = std::time::Duration::from_millis(250);
    loop {
        let stream = match tokio_tungstenite::connect_async(&args.url).await {
            Ok((stream, _)) => stream,
            Err(error) => {
                output.emit(&serde_json::json!({
                    "record_type": "connection",
                    "source": args.source,
                    "state": "connect_error",
                    "reconnects": reconnects,
                    "received_unix_ns": unix_ns(),
                    "error": error.to_string(),
                }))?;
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(std::time::Duration::from_secs(5));
                reconnects = reconnects.saturating_add(1);
                continue;
            }
        };
        backoff = std::time::Duration::from_millis(250);
        let connected_at = Instant::now();
        output.emit(&serde_json::json!({
            "record_type": "connection",
            "source": args.source,
            "state": "connected",
            "reconnects": reconnects,
            "received_unix_ns": unix_ns(),
        }))?;
        let (_, mut read) = stream.split();

        while let Some(frame) = read.next().await {
            // Timestamp immediately after the socket future becomes ready,
            // before text conversion, recording, JSON or base64 work.
            let received_mono_ns = monotonic_raw_ns();
            let received_unix_ns = unix_ns();
            let frame = match frame {
                Ok(frame) => frame,
                Err(error) => {
                    output.emit(&serde_json::json!({
                        "record_type": "connection",
                        "source": args.source,
                        "state": "read_error",
                        "reconnects": reconnects,
                        "received_unix_ns": received_unix_ns,
                        "error": error.to_string(),
                    }))?;
                    break;
                }
            };
            let payload = match frame {
                Message::Text(text) => text.to_string(),
                Message::Binary(bytes) => String::from_utf8(bytes.to_vec())
                    .context("binary websocket frame was not UTF-8 JSON")?,
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
            };

            if let Some(tx) = &recorder {
                let recorded = RecordedFrame {
                    received_unix_ns,
                    payload: payload.clone(),
                };
                if tx.try_send(recorded).is_err() {
                    anyhow::bail!("recorder queue full or disconnected; recording incomplete");
                }
            }

            process_frame(
                &args.source,
                received_mono_ns,
                received_unix_ns,
                connected_at.elapsed() < std::time::Duration::from_secs(args.warmup_seconds),
                reconnects,
                &payload,
                &mut decoder,
                &mut sequences,
                &output,
            )?;
        }
        output.emit(&serde_json::json!({
            "record_type": "connection",
            "source": args.source,
            "state": "disconnected",
            "reconnects": reconnects,
            "received_unix_ns": unix_ns(),
        }))?;
        reconnects = reconnects.saturating_add(1);
        tokio::time::sleep(backoff).await;
    }
}

async fn replay(args: ReplayArgs) -> Result<()> {
    let mut decoder = FeedDecoder::new(parse_filter(args.filter)?);
    let mut sequences = SequenceTracker::default();
    let output = OutputSink::stdout();
    let file = File::open(&args.input)
        .await
        .with_context(|| format!("open replay file {}", args.input.display()))?;
    let mut lines = BufReader::new(file).lines();
    while let Some(line) = lines.next_line().await? {
        let recorded: RecordedFrame =
            serde_json::from_str(&line).context("decode recorded frame")?;
        process_frame(
            &args.source,
            monotonic_raw_ns(),
            recorded.received_unix_ns,
            false,
            0,
            &recorded.payload,
            &mut decoder,
            &mut sequences,
            &output,
        )?;
    }
    Ok(())
}

fn process_frame(
    source: &str,
    received_mono_ns: u64,
    received_unix_ns: u128,
    warmup: bool,
    reconnects: u64,
    payload: &str,
    decoder: &mut FeedDecoder,
    sequences: &mut SequenceTracker,
    output: &OutputSink,
) -> Result<()> {
    let started = Instant::now();
    let feed: BroadcastMessage = serde_json::from_str(payload).context("decode feed JSON")?;
    let json_ns = elapsed_ns(started);
    let sequence_numbers: Vec<u64> = feed
        .messages
        .iter()
        .map(|message| message.sequence_number)
        .collect();
    for sequence_number in &sequence_numbers {
        sequences.observe(*sequence_number);
    }
    let decoded = decoder.decode(&feed)?;
    let sequence_health = sequences.current();
    let candidate_emission_enabled = sequence_health.is_contiguous();
    let suppressed_candidates = if candidate_emission_enabled {
        0
    } else {
        decoded.candidates.len()
    };
    let frame_report = FrameReport {
        record_type: "frame",
        source: source.to_owned(),
        received_mono_ns,
        received_unix_ns,
        warmup,
        reconnects,
        frame_bytes: payload.len(),
        json_ns,
        base64_ns: decoded.base64_ns,
        l2_walk_ns: decoded.l2_walk_ns,
        envelope_decode_ns: decoded.envelope_decode_ns,
        filter_ns: decoded.filter_ns,
        feed_messages: decoded.messages,
        signed_transactions: decoded.signed_transactions,
        router_matches: decoded.router_matches,
        selector_matches: decoded.selector_matches,
        recovered_signers: decoded.recovered_signers,
        candidates: decoded.candidates.len(),
        candidate_emission_enabled,
        suppressed_candidates,
        unsupported_l1_messages: decoded.unsupported_l1_messages,
        unsupported_l2_messages: decoded.unsupported_l2_messages,
        sequence_numbers,
        sequence: sequence_health,
    };
    output.emit(&frame_report)?;
    for fingerprint in decoded.transaction_fingerprints {
        output.emit(&serde_json::json!({
            "record_type": "transaction",
            "source": source,
            "transaction": fingerprint,
        }))?;
    }
    if candidate_emission_enabled {
        for candidate in decoded.candidates {
            output.emit(
                &serde_json::json!({"record_type": "candidate", "source": source, "candidate": candidate}),
            )?;
        }
    }
    Ok(())
}

fn compare(args: CompareArgs) -> Result<()> {
    if args.inputs.len() < 2 {
        anyhow::bail!("comparison needs at least two input files");
    }
    let clock_offsets = parse_clock_offsets(&args.clock_offsets)?;
    let mut arrivals: BTreeMap<u64, HashMap<String, u128>> = BTreeMap::new();
    let mut all_sources = HashSet::new();
    let mut health: HashMap<String, SequenceObservation> = HashMap::new();
    for path in args.inputs {
        let file = std::fs::File::open(&path)
            .with_context(|| format!("open comparison input {}", path.display()))?;
        for line in std::io::BufReader::new(file).lines() {
            let line = line?;
            let Ok(frame) = serde_json::from_str::<ComparableFrame>(&line) else {
                continue;
            };
            if frame.record_type != "frame" || frame.warmup {
                continue;
            }
            all_sources.insert(frame.source.clone());
            health
                .entry(frame.source.clone())
                .and_modify(|current| {
                    current.gaps = current.gaps.max(frame.sequence.gaps);
                    current.missing = current.missing.max(frame.sequence.missing);
                    current.duplicates_or_reordered = current
                        .duplicates_or_reordered
                        .max(frame.sequence.duplicates_or_reordered);
                })
                .or_insert(frame.sequence);
            for sequence in frame.sequence_numbers {
                arrivals
                    .entry(sequence)
                    .or_default()
                    .entry(frame.source.clone())
                    .or_insert(apply_clock_offset(
                        frame.received_unix_ns,
                        *clock_offsets.get(&frame.source).unwrap_or(&0),
                    ));
            }
        }
    }

    let source_count = all_sources.len();
    if source_count < 2 {
        anyhow::bail!("comparison needs at least two distinct source labels");
    }
    let mut lags: HashMap<String, Vec<u128>> = HashMap::new();
    let mut wins: HashMap<String, usize> = HashMap::new();
    let mut matched_sequences = 0;
    for observed in arrivals.values().filter(|seen| seen.len() == source_count) {
        matched_sequences += 1;
        let earliest = observed
            .values()
            .copied()
            .min()
            .expect("non-empty arrivals");
        for (source, arrival) in observed {
            let lag = arrival.saturating_sub(earliest);
            lags.entry(source.clone()).or_default().push(lag);
            if lag == 0 {
                *wins.entry(source.clone()).or_default() += 1;
            }
        }
    }
    if matched_sequences == 0 {
        anyhow::bail!("no sequence numbers were present in every source");
    }

    let mut sources: Vec<_> = all_sources.into_iter().collect();
    sources.sort();
    let sources: Vec<SourceComparison> = sources
        .into_iter()
        .map(|source| {
            let values = lags.entry(source.clone()).or_default();
            values.sort_unstable();
            let source_health = health.get(&source).copied().unwrap_or_default();
            SourceComparison {
                source: source.clone(),
                clock_offset_ns: *clock_offsets.get(&source).unwrap_or(&0),
                eligible: source_health.gaps == 0 && source_health.missing == 0,
                samples: values.len(),
                wins: wins.get(&source).copied().unwrap_or_default(),
                gaps: source_health.gaps,
                missing: source_health.missing,
                duplicates_or_reordered: source_health.duplicates_or_reordered,
                lag_ns_p50: percentile(values, 50),
                lag_ns_p95: percentile(values, 95),
                lag_ns_p99: percentile(values, 99),
                lag_ns_max: values.last().copied().unwrap_or_default(),
            }
        })
        .collect();
    let mut winner = pick_winner(&sources, matched_sequences, args.min_matched_sequences);
    if let Some(ref selected) = winner {
        let selected_p95 = sources
            .iter()
            .find(|s| &s.source == selected)
            .unwrap()
            .lag_ns_p95;
        let runner_up_p95 = sources
            .iter()
            .filter(|s| s.eligible && &s.source != selected)
            .map(|s| s.lag_ns_p95)
            .min()
            .unwrap_or(selected_p95);
        if runner_up_p95.saturating_sub(selected_p95) <= args.max_clock_uncertainty_ns {
            winner = None;
        }
    }
    let decision_ready = winner.is_some();
    println!(
        "{}",
        serde_json::to_string(&Comparison {
            record_type: "comparison",
            matched_sequences,
            minimum_matched_sequences: args.min_matched_sequences,
            decision_ready,
            max_clock_uncertainty_ns: args.max_clock_uncertainty_ns,
            winner,
            sources,
        })?
    );
    Ok(())
}

fn parse_clock_offsets(values: &[String]) -> Result<HashMap<String, i128>> {
    let mut offsets = HashMap::new();
    for value in values {
        let (source, raw) = value
            .split_once('=')
            .with_context(|| format!("clock offset must be SOURCE=NANOSECONDS: {value}"))?;
        if source.is_empty() {
            anyhow::bail!("clock-offset source cannot be empty");
        }
        let offset = raw
            .parse::<i128>()
            .with_context(|| format!("invalid clock offset nanoseconds: {raw}"))?;
        if offsets.insert(source.to_owned(), offset).is_some() {
            anyhow::bail!("duplicate clock offset for source {source}");
        }
    }
    Ok(offsets)
}

fn apply_clock_offset(timestamp: u128, remote_minus_reference: i128) -> u128 {
    if remote_minus_reference >= 0 {
        timestamp.saturating_sub(remote_minus_reference as u128)
    } else {
        timestamp.saturating_add(remote_minus_reference.unsigned_abs())
    }
}

fn summarize(args: SummarizeArgs) -> Result<()> {
    let file = std::fs::File::open(&args.input)
        .with_context(|| format!("open summary input {}", args.input.display()))?;
    let mut source = None;
    let mut first_live_unix_ns = None;
    let mut last_live_unix_ns = None;
    let mut warmup_frames = 0_u64;
    let mut live_frames = 0_u64;
    let mut live_sequences = 0_u64;
    let mut feed_messages = 0_u64;
    let mut signed_transactions = 0_u64;
    let mut candidates = 0_u64;
    let mut bytes = 0_u64;
    let mut sequence = SequenceObservation::default();
    let mut reconnects = 0_u64;
    let mut connection_errors = 0_u64;
    let mut unsupported_l1_messages = 0_u64;
    let mut unsupported_l2_messages = 0_u64;
    let mut total_local = Vec::new();
    let mut json = Vec::new();
    let mut base64 = Vec::new();
    let mut l2_walk = Vec::new();
    let mut envelope = Vec::new();
    let mut filter = Vec::new();

    for line in std::io::BufReader::new(file).lines() {
        let line = line?;
        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        match value.get("record_type").and_then(|kind| kind.as_str()) {
            Some("connection") => {
                if matches!(
                    value.get("state").and_then(|state| state.as_str()),
                    Some("connect_error" | "read_error" | "disconnected")
                ) {
                    connection_errors = connection_errors.saturating_add(1);
                }
            }
            Some("frame") => {
                let frame: SummaryFrame = serde_json::from_value(value)?;
                if frame.record_type != "frame" {
                    continue;
                }
                if let Some(current) = &source {
                    if current != &frame.source {
                        anyhow::bail!(
                            "summary input contains multiple sources: {current} and {}",
                            frame.source
                        );
                    }
                } else {
                    source = Some(frame.source.clone());
                }
                if frame.warmup {
                    warmup_frames = warmup_frames.saturating_add(1);
                    continue;
                }

                first_live_unix_ns.get_or_insert(frame.received_unix_ns);
                last_live_unix_ns = Some(frame.received_unix_ns);
                live_frames = live_frames.saturating_add(1);
                live_sequences = live_sequences.saturating_add(
                    u64::try_from(frame.sequence_numbers.len()).unwrap_or(u64::MAX),
                );
                feed_messages = feed_messages.saturating_add(frame.feed_messages);
                signed_transactions = signed_transactions.saturating_add(frame.signed_transactions);
                candidates = candidates.saturating_add(frame.candidates);
                bytes = bytes.saturating_add(frame.frame_bytes);
                sequence = frame.sequence;
                reconnects = reconnects.max(frame.reconnects);
                unsupported_l1_messages =
                    unsupported_l1_messages.saturating_add(frame.unsupported_l1_messages);
                unsupported_l2_messages =
                    unsupported_l2_messages.saturating_add(frame.unsupported_l2_messages);

                let local = frame
                    .json_ns
                    .saturating_add(frame.base64_ns)
                    .saturating_add(frame.l2_walk_ns);
                total_local.push(u128::from(local));
                json.push(u128::from(frame.json_ns));
                base64.push(u128::from(frame.base64_ns));
                l2_walk.push(u128::from(frame.l2_walk_ns));
                envelope.push(u128::from(frame.envelope_decode_ns));
                filter.push(u128::from(frame.filter_ns));
            }
            _ => {}
        }
    }

    let source = source.context("summary input contained no frame records")?;
    let first_live_unix_ns =
        first_live_unix_ns.context("summary input contained no live frames")?;
    let last_live_unix_ns = last_live_unix_ns.expect("first live frame sets last frame");
    let duration_ns = last_live_unix_ns.saturating_sub(first_live_unix_ns);
    let duration_seconds = duration_ns as f64 / 1_000_000_000_f64;
    let sequences_per_second = if duration_seconds > 0.0 {
        live_sequences as f64 / duration_seconds
    } else {
        0.0
    };
    let total_local_sum: u128 = total_local.iter().copied().sum();
    let average_local_ns_per_feed_message = if feed_messages > 0 {
        total_local_sum / u128::from(feed_messages)
    } else {
        0
    };

    let summary = ProbeSummary {
        record_type: "summary",
        source,
        first_live_unix_ns,
        last_live_unix_ns,
        duration_seconds,
        warmup_frames_excluded: warmup_frames,
        live_frames,
        live_sequences,
        feed_messages,
        signed_transactions,
        candidates,
        bytes,
        sequence,
        reconnects,
        connection_errors,
        unsupported_l1_messages,
        unsupported_l2_messages,
        sequences_per_second,
        average_local_ns_per_feed_message,
        total_local_ns: quantiles(&mut total_local),
        json_ns: quantiles(&mut json),
        base64_ns: quantiles(&mut base64),
        l2_walk_ns: quantiles(&mut l2_walk),
        envelope_decode_ns: quantiles(&mut envelope),
        filter_ns: quantiles(&mut filter),
    };
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

fn quantiles(values: &mut [u128]) -> NsQuantiles {
    if values.is_empty() {
        return NsQuantiles::default();
    }
    values.sort_unstable();
    NsQuantiles {
        p50: percentile(values, 50),
        p95: percentile(values, 95),
        p99: percentile(values, 99),
        max: values.last().copied().unwrap_or_default(),
    }
}

fn pick_winner(
    sources: &[SourceComparison],
    matched_sequences: usize,
    minimum_matched_sequences: usize,
) -> Option<String> {
    if matched_sequences < minimum_matched_sequences {
        return None;
    }
    sources
        .iter()
        .filter(|source| source.eligible)
        .min_by(|left, right| {
            left.lag_ns_p95
                .cmp(&right.lag_ns_p95)
                .then_with(|| left.lag_ns_p99.cmp(&right.lag_ns_p99))
                .then_with(|| left.lag_ns_p50.cmp(&right.lag_ns_p50))
                .then_with(|| left.lag_ns_max.cmp(&right.lag_ns_max))
                .then_with(|| right.wins.cmp(&left.wins))
                .then_with(|| left.source.cmp(&right.source))
        })
        .map(|source| source.source.clone())
}

fn percentile(values: &[u128], percentile: usize) -> u128 {
    if values.is_empty() {
        return 0;
    }
    let index = values
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1)
        .min(values.len() - 1);
    values[index]
}

#[cfg(test)]
mod tests {
    use super::{
        SourceComparison, U256, apply_clock_offset, parse_clock_offsets, parse_selectors,
        parse_u256, percentile, pick_winner,
    };

    #[test]
    fn percentile_uses_nearest_rank() {
        let values = [0, 0, 100];
        assert_eq!(percentile(&values, 50), 0);
        assert_eq!(percentile(&values, 95), 100);
        assert_eq!(percentile(&values, 99), 100);
    }

    #[test]
    fn winner_requires_sample_floor_and_healthy_feed() {
        let healthy = SourceComparison {
            source: "healthy".into(),
            clock_offset_ns: 0,
            eligible: true,
            samples: 10,
            wins: 8,
            gaps: 0,
            missing: 0,
            duplicates_or_reordered: 0,
            lag_ns_p50: 0,
            lag_ns_p95: 10,
            lag_ns_p99: 20,
            lag_ns_max: 30,
        };
        let mut faster_but_gapped = healthy.clone();
        faster_but_gapped.source = "gapped".into();
        faster_but_gapped.eligible = false;
        faster_but_gapped.gaps = 1;
        faster_but_gapped.missing = 2;
        faster_but_gapped.lag_ns_p95 = 0;

        assert_eq!(
            pick_winner(&[healthy.clone(), faster_but_gapped.clone()], 9, 10),
            None
        );
        assert_eq!(
            pick_winner(&[healthy, faster_but_gapped], 10, 10).as_deref(),
            Some("healthy")
        );
    }

    #[test]
    fn parses_exact_four_byte_selectors() {
        let parsed = parse_selectors(vec!["0x38ed1739".into()]).unwrap();
        assert!(parsed.contains(&[0x38, 0xed, 0x17, 0x39]));
        assert!(parse_selectors(vec!["0x1234".into()]).is_err());
        assert!(parse_selectors(vec!["0xzzzzzzzz".into()]).is_err());
    }

    #[test]
    fn clock_offsets_are_parsed_and_applied() {
        let offsets = parse_clock_offsets(&["fra=-200".into()]).unwrap();
        assert_eq!(offsets["fra"], -200);
        assert_eq!(apply_clock_offset(1_000, -200), 1_200);
        assert_eq!(apply_clock_offset(1_000, 200), 800);
    }

    #[test]
    fn parses_decimal_and_hex_u256() {
        assert_eq!(parse_u256("16").unwrap(), U256::from(16));
        assert_eq!(parse_u256("0x10").unwrap(), U256::from(16));
    }
}

async fn start_recorder(path: &Path) -> Result<mpsc::Sender<RecordedFrame>> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .with_context(|| format!("open record file {}", path.display()))?;
    let (tx, mut rx) = mpsc::channel::<RecordedFrame>(1_024);
    tokio::spawn(async move {
        let mut file = file;
        while let Some(frame) = rx.recv().await {
            match serde_json::to_vec(&frame) {
                Ok(mut encoded) => {
                    encoded.push(b'\n');
                    if let Err(error) = file.write_all(&encoded).await {
                        eprintln!("record frame: {error}");
                        break;
                    }
                }
                Err(error) => eprintln!("encode recorded frame: {error}"),
            }
        }
    });
    Ok(tx)
}

fn parse_filter(args: FilterArgs) -> Result<Filter> {
    Ok(Filter {
        routers: parse_addresses("router", args.routers)?,
        selectors: parse_selectors(args.selectors)?,
        watched_wallets: parse_addresses("watch", args.watched_wallets)?,
        emit_transaction_hashes: args.emit_tx_hashes,
    })
}

fn parse_selectors(values: Vec<String>) -> Result<HashSet<[u8; 4]>> {
    values
        .into_iter()
        .map(|value| {
            let hex = value.strip_prefix("0x").unwrap_or(&value);
            if hex.len() != 8 {
                anyhow::bail!("invalid --selector {value}: expected exactly four bytes");
            }
            let parsed = u32::from_str_radix(hex, 16)
                .with_context(|| format!("invalid --selector {value}: expected hexadecimal"))?;
            Ok(parsed.to_be_bytes())
        })
        .collect()
}

fn parse_addresses(kind: &str, values: Vec<String>) -> Result<HashSet<Address>> {
    values
        .into_iter()
        .map(|value| {
            Address::from_str(&value)
                .map_err(|error| anyhow::anyhow!("invalid --{kind} address {value}: {error}"))
        })
        .collect()
}

fn unix_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(unix)]
fn monotonic_raw_ns() -> u64 {
    let mut value = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let result = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC_RAW, &mut value) };
    if result != 0 {
        return 0;
    }
    (value.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(value.tv_nsec as u64)
}

#[cfg(not(unix))]
fn monotonic_raw_ns() -> u64 {
    0
}

fn elapsed_ns(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}
