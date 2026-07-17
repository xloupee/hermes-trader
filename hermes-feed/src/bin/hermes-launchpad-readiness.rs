use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use hermes_feed::launchpad_readiness::{LaunchpadReadinessWindow, evaluate_launchpad_readiness};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Evaluate conservative paper-observer sample readiness; never authorizes execution"
)]
struct Cli {
    /// JSONL readiness-window records, or `-` for stdin.
    #[arg(long, default_value = "-")]
    input: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let reader: Box<dyn BufRead> = if cli.input == Path::new("-") {
        Box::new(BufReader::new(io::stdin().lock()))
    } else {
        Box::new(BufReader::new(File::open(&cli.input).with_context(
            || format!("open readiness input {}", cli.input.display()),
        )?))
    };

    let mut windows = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line.with_context(|| format!("read readiness input line {line_number}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let window = serde_json::from_str::<LaunchpadReadinessWindow>(&line)
            .with_context(|| format!("decode readiness input line {line_number}"))?;
        windows.push(window);
    }
    for record in evaluate_launchpad_readiness(&windows).context("evaluate launchpad readiness")? {
        println!(
            "{}",
            serde_json::to_string(&record).context("encode launchpad readiness record")?
        );
    }
    Ok(())
}
