use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use hermes_feed::feed::BroadcastMessage;
use hermes_feed::paper_observer::{
    PaperExpectedPins, PaperFeedRuntime, PaperLaunchpadObserver, PaperObservedStartupSnapshot,
};
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Unified paper-only launchpad observer for Nitro feed frames"
)]
struct Cli {
    /// Reviewed protocol-owned expected pins. Never use an observed snapshot here.
    #[arg(long)]
    expected_pins: PathBuf,
    /// Independently collected startup runtime observations.
    #[arg(long)]
    observed_startup_snapshot: PathBuf,
    /// JSONL of direct Nitro BroadcastMessage objects or probe records containing `payload`.
    #[arg(long, default_value = "-")]
    input: PathBuf,
}

fn main() -> Result<()> {
    let args = Cli::parse();
    if args.expected_pins.canonicalize()? == args.observed_startup_snapshot.canonicalize()? {
        anyhow::bail!("expected pins and observed startup snapshot must be separate files");
    }
    let expected: PaperExpectedPins = serde_json::from_reader(BufReader::new(
        File::open(&args.expected_pins)
            .with_context(|| format!("open expected pins {}", args.expected_pins.display()))?,
    ))
    .with_context(|| format!("decode expected pins {}", args.expected_pins.display()))?;
    let observed: PaperObservedStartupSnapshot = serde_json::from_reader(BufReader::new(
        File::open(&args.observed_startup_snapshot).with_context(|| {
            format!(
                "open observed startup snapshot {}",
                args.observed_startup_snapshot.display()
            )
        })?,
    ))
    .with_context(|| {
        format!(
            "decode observed startup snapshot {}",
            args.observed_startup_snapshot.display()
        )
    })?;
    let observer = PaperLaunchpadObserver::from_startup_snapshots(expected, observed)?;
    let mut runtime = PaperFeedRuntime::new(observer);
    println!(
        "{}",
        serde_json::to_string(&json!({
            "record_type": "launchpad_paper_capabilities",
            "capabilities": runtime.capabilities(),
            "broadcast": false,
            "signing": false,
            "candidate_time_rpc": false,
        }))?
    );

    let input: Box<dyn BufRead> = if args.input == Path::new("-") {
        Box::new(BufReader::new(std::io::stdin()))
    } else {
        Box::new(BufReader::new(File::open(&args.input).with_context(
            || format!("open input {}", args.input.display()),
        )?))
    };
    for (index, line) in input.lines().enumerate() {
        let line = line.with_context(|| format!("read input line {}", index + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line)
            .with_context(|| format!("parse input line {}", index + 1))?;
        let feed: BroadcastMessage = match value.get("payload").and_then(Value::as_str) {
            Some(payload) => serde_json::from_str(payload)
                .with_context(|| format!("decode recorded payload at line {}", index + 1))?,
            None => serde_json::from_value(value)
                .with_context(|| format!("decode Nitro frame at line {}", index + 1))?,
        };
        let report = runtime.decode(&feed)?;
        println!(
            "{}",
            serde_json::to_string(&json!({
                "record_type": "launchpad_paper_frame",
                "report": report,
            }))?
        );
    }
    Ok(())
}
