use std::fs::File;
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::str::FromStr;

use alloy_primitives::Address;
use anyhow::{Context, Result, bail};
use clap::Parser;
use hermes_feed::{KeystoreTradeSigner, TradeSigner};
use serde_json::json;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Validate an encrypted Hermes trading keystore without exposing key material"
)]
struct Cli {
    #[arg(long)]
    keystore: PathBuf,
    #[arg(long)]
    expected_address: String,
    /// Inherited descriptor containing only the keystore password. Descriptors
    /// 0-2 are refused to prevent accidental terminal input or log coupling.
    #[arg(long, default_value_t = 3)]
    password_fd: i32,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.password_fd < 3 {
        bail!("--password-fd must be an inherited descriptor numbered 3 or higher");
    }
    let expected_address =
        Address::from_str(&cli.expected_address).context("parse expected trading address")?;
    // SAFETY: ownership of the explicitly supplied inherited descriptor is
    // transferred to this short-lived process and it is closed after loading.
    let password = unsafe { File::from_raw_fd(cli.password_fd) };
    let signer = KeystoreTradeSigner::load_from_reader(&cli.keystore, password, expected_address)?;
    println!(
        "{}",
        serde_json::to_string(&json!({
            "record_type": "hermes_keystore_check",
            "valid": true,
            "address": signer.address(),
            "password_source": "inherited_file_descriptor",
            "private_key_logged": false,
        }))?
    );
    Ok(())
}
