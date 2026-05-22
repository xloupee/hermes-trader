use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
};

const TARGET: &str = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg";

fn run_plan(fixture: &str) -> Value {
    let output = Command::cargo_bin("copy-rs")
        .unwrap()
        .args([
            "plan",
            "--input",
            fixture,
            "--target-wallet",
            TARGET,
            "--copy-sol",
            "0.01",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    serde_json::from_slice(&output).unwrap()
}

#[test]
fn cli_copies_sol_to_token_fixture() {
    let plan = run_plan("fixtures/helius-swap-sol-to-token.json");

    assert_eq!(plan["decision"], "copy");
    assert_eq!(plan["targetWallet"], TARGET);
    assert_eq!(
        plan["inputMint"],
        "So11111111111111111111111111111111111111112"
    );
    assert_eq!(
        plan["outputMint"],
        "DezXAZ8z7PnrnRJjz3n26VsZdrmRwNny4nBj9JkzjW7B"
    );
    assert_eq!(plan["copyInputAmount"], 0.01);
    assert_eq!(plan.get("skipReason"), None);
}

#[test]
fn cli_skips_token_to_sol_fixture() {
    let plan = run_plan("fixtures/helius-swap-token-to-sol.json");

    assert_eq!(plan["decision"], "skip");
    assert_eq!(
        plan["skipReason"],
        "only SOL to token buys are copied in dry-run v1"
    );
}

#[test]
fn cli_skips_token_to_token_fixture() {
    let plan = run_plan("fixtures/helius-swap-token-to-token.json");

    assert_eq!(plan["decision"], "skip");
    assert_eq!(
        plan["skipReason"],
        "only SOL to token buys are copied in dry-run v1"
    );
}

#[test]
fn cli_skips_unrelated_wallet_fixture() {
    let plan = run_plan("fixtures/helius-swap-unrelated-wallet.json");

    assert_eq!(plan["decision"], "skip");
    assert_eq!(
        plan["skipReason"],
        "target wallet is not involved in this swap"
    );
}

#[test]
fn cli_requires_plan_subcommand() {
    Command::cargo_bin("copy-rs")
        .unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn cli_watch_requires_helius_key() {
    Command::cargo_bin("copy-rs")
        .unwrap()
        .args(["watch", "--target-wallet", TARGET, "--copy-sol", "0.01"])
        .env_remove("HELIUS_API_KEY")
        .assert()
        .failure()
        .stderr(predicate::str::contains("helius-api-key"));
}

#[test]
fn cli_build_local_posts_pumpportal_request_for_copy_fixture() {
    let (url, body_rx) = spawn_pumpportal_mock(200, b"serialized-tx".to_vec());
    let output = Command::cargo_bin("copy-rs")
        .unwrap()
        .args([
            "build-local",
            "--input",
            "fixtures/helius-swap-sol-to-token.json",
            "--target-wallet",
            TARGET,
            "--copy-sol",
            "0.01",
            "--public-key",
            TARGET,
            "--slippage",
            "15",
            "--priority-fee",
            "0.00009",
            "--pool",
            "auto",
            "--pumpportal-url",
            &url,
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("PumpPortal local tx built"))
        .get_output()
        .stdout
        .clone();
    let response: Value = serde_json::from_slice(&output).unwrap();
    let body = body_rx.recv().unwrap();

    assert_eq!(body["publicKey"], TARGET);
    assert_eq!(body["action"], "buy");
    assert_eq!(body["mint"], "DezXAZ8z7PnrnRJjz3n26VsZdrmRwNny4nBj9JkzjW7B");
    assert_eq!(body["amount"], 0.01);
    assert_eq!(body["denominatedInSol"], "true");
    assert_eq!(body["slippage"], 15.0);
    assert_eq!(body["priorityFee"], 0.00009);
    assert_eq!(body["pool"], "auto");
    assert_eq!(response["pumpportal"]["decision"], "copy");
    assert_eq!(response["pumpportal"]["response"]["ok"], true);
    assert_eq!(response["pumpportal"]["response"]["bodyLength"], 13);
    assert_eq!(
        response["pumpportal"]["response"]["encodedTransactionBase64"],
        "c2VyaWFsaXplZC10eA=="
    );
}

#[test]
fn cli_build_local_skips_without_pumpportal_post_for_skip_fixture() {
    let output = Command::cargo_bin("copy-rs")
        .unwrap()
        .args([
            "build-local",
            "--input",
            "fixtures/helius-swap-token-to-sol.json",
            "--target-wallet",
            TARGET,
            "--copy-sol",
            "0.01",
            "--public-key",
            TARGET,
            "--pumpportal-url",
            "http://127.0.0.1:9/unused",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"decision\": \"skip\""))
        .get_output()
        .stdout
        .clone();
    let response: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(response["pumpportal"]["decision"], "skip");
    assert_eq!(
        response["pumpportal"]["skipReason"],
        "only SOL to token buys are copied in dry-run v1"
    );
    assert!(response["pumpportal"].get("request").is_none());
}

#[test]
fn cli_watch_pumpportal_build_requires_public_key() {
    Command::cargo_bin("copy-rs")
        .unwrap()
        .args([
            "watch",
            "--target-wallet",
            TARGET,
            "--copy-sol",
            "0.01",
            "--pumpportal-build",
        ])
        .env("HELIUS_API_KEY", "dummy")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--public-key is required"));
}

fn spawn_pumpportal_mock(status: u16, body: Vec<u8>) -> (String, mpsc::Receiver<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream);
        let request_text = String::from_utf8_lossy(&request);
        assert!(request_text.starts_with("POST /trade-local HTTP/1.1"));

        let body_start = find_header_end(&request).unwrap() + 4;
        let request_body: Value = serde_json::from_slice(&request[body_start..]).unwrap();
        tx.send(request_body).unwrap();

        let reason = if status == 200 { "OK" } else { "ERROR" };
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
    });

    (format!("http://{addr}/trade-local"), rx)
}

fn read_http_request(stream: &mut impl Read) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];

    loop {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0);
        request.extend_from_slice(&buffer[..read]);

        let Some(header_end) = find_header_end(&request) else {
            continue;
        };
        let content_length = parse_content_length(&request[..header_end]).unwrap_or(0);

        if request.len() >= header_end + 4 + content_length {
            return request;
        }
    }
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_content_length(headers: &[u8]) -> Option<usize> {
    String::from_utf8_lossy(headers)
        .lines()
        .find_map(|line| {
            line.strip_prefix("content-length:")
                .or_else(|| line.strip_prefix("Content-Length:"))
        })
        .and_then(|value| value.trim().parse().ok())
}
