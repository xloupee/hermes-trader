use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

use alloy_primitives::B256;
use anyhow::{Context, Result, bail};
use clap::Parser;
use hermes_feed::evidence_provenance::{maybe_print_self_digest, verify_expected_self_keccak256};
use hermes_feed::launchpad_readiness::{
    LaunchpadReadinessWindow, evaluate_completed_session_readiness, evaluate_launchpad_readiness,
};
use hermes_feed::launchpad_session::{
    SessionExecutables, complete_session, ensure_compatible_independent_sessions,
    validate_completed_session,
};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Evaluate conservative paper-observer sample readiness; never authorizes execution"
)]
struct Cli {
    /// Free-standing JSONL readiness rows. This mode is diagnostic and always
    /// emits `input_trust: untrusted_input` with readiness false.
    #[arg(long)]
    input: Option<PathBuf>,
    /// Completed wrapper session directory. Repeat for independent sessions.
    #[arg(long = "session-dir")]
    session_dirs: Vec<PathBuf>,
    /// Validate canonical artifacts and atomically publish the final completion
    /// manifest. Used by the local runner only after every phase succeeds.
    #[arg(long)]
    complete_session: Option<PathBuf>,
    #[arg(long)]
    expected_self_keccak256: Option<B256>,
    #[arg(long)]
    feed_keccak256: Option<B256>,
    #[arg(long)]
    paper_keccak256: Option<B256>,
    #[arg(long)]
    reconciler_keccak256: Option<B256>,
    #[arg(long)]
    chain_head_keccak256: Option<B256>,
    /// Exact preflight-verified paper executable used to independently replay
    /// raw observer input and regenerate finalizer output before completion.
    #[arg(long)]
    paper_bin: Option<PathBuf>,
}

fn main() -> Result<()> {
    if maybe_print_self_digest()? {
        return Ok(());
    }
    let cli = Cli::parse();
    let trusted_mode = cli.complete_session.is_some() || !cli.session_dirs.is_empty();
    let verified_readiness_keccak256 = if trusted_mode {
        Some(verify_expected_self_keccak256(
            cli.expected_self_keccak256
                .context("trusted session mode requires --expected-self-keccak256")?,
        )?)
    } else {
        None
    };
    if let Some(directory) = cli.complete_session.as_ref() {
        if cli.input.is_some() || !cli.session_dirs.is_empty() {
            bail!("--complete-session cannot be combined with readiness inputs");
        }
        let readiness_keccak256 = verified_readiness_keccak256.expect("validated above");
        let paper_bin = cli.paper_bin.as_ref().context("missing --paper-bin")?;
        let path = complete_session(
            directory,
            SessionExecutables {
                feed_keccak256: cli.feed_keccak256.context("missing --feed-keccak256")?,
                paper_keccak256: cli.paper_keccak256.context("missing --paper-keccak256")?,
                reconciler_keccak256: cli
                    .reconciler_keccak256
                    .context("missing --reconciler-keccak256")?,
                chain_head_keccak256: cli
                    .chain_head_keccak256
                    .context("missing --chain-head-keccak256")?,
                readiness_keccak256,
            },
            paper_bin,
        )?;
        println!("{}", path.display());
        return Ok(());
    }
    if !cli.session_dirs.is_empty() {
        if cli.input.is_some() {
            bail!("--input cannot be combined with --session-dir");
        }
        let mut sessions = Vec::new();
        for directory in &cli.session_dirs {
            sessions.push(
                validate_completed_session(directory).with_context(|| {
                    format!("validate completed session {}", directory.display())
                })?,
            );
        }
        let readiness_keccak256 = verified_readiness_keccak256.expect("validated above");
        ensure_compatible_independent_sessions(&sessions, readiness_keccak256)?;
        let executables = sessions[0].executables.clone();
        let manifests = sessions
            .iter()
            .map(|session| session.manifest_content_keccak256)
            .collect::<Vec<_>>();
        let windows = sessions
            .into_iter()
            .flat_map(|session| session.windows)
            .collect::<Vec<_>>();
        emit(evaluate_completed_session_readiness(
            &windows,
            &manifests,
            executables.feed_keccak256,
            executables.chain_head_keccak256,
            readiness_keccak256,
        )?)?;
        return Ok(());
    }

    let input = cli.input.unwrap_or_else(|| PathBuf::from("-"));
    let reader: Box<dyn BufRead> = if input == Path::new("-") {
        Box::new(BufReader::new(io::stdin().lock()))
    } else {
        Box::new(BufReader::new(File::open(&input).with_context(|| {
            format!("open readiness input {}", input.display())
        })?))
    };
    let mut windows = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line.with_context(|| format!("read readiness input line {line_number}"))?;
        if line.trim().is_empty() {
            continue;
        }
        windows.push(
            serde_json::from_str::<LaunchpadReadinessWindow>(&line)
                .with_context(|| format!("decode readiness input line {line_number}"))?,
        );
    }
    emit(evaluate_launchpad_readiness(&windows)?)
}

fn emit(records: Vec<hermes_feed::launchpad_readiness::LaunchpadReadinessRecord>) -> Result<()> {
    for record in records {
        println!("{}", serde_json::to_string(&record)?);
    }
    Ok(())
}
