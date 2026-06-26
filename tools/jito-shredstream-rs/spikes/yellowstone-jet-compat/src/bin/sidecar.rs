use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use solana_keypair::{Keypair, Signature};
use std::{env, net::SocketAddr, num::NonZeroUsize, str::FromStr, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use yellowstone_jet_tpu_client::yellowstone_grpc::sender::{
    create_yellowstone_tpu_sender, Endpoints, YellowstoneTpuSender, YellowstoneTpuSenderConfig,
};

type SharedSender = Arc<Mutex<YellowstoneTpuSender>>;

#[derive(Clone)]
struct AppState {
    sender: SharedSender,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendRequest {
    signature: String,
    transaction_base64: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SendResponse {
    status: &'static str,
    label: &'static str,
    signature: String,
    bytes: usize,
    error_class: Option<&'static str>,
    error: Option<String>,
}

#[tokio::main]
async fn main() {
    let bind_addr = env::var("JITO_TPU_JET_SIDECAR_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_string())
        .parse::<SocketAddr>()
        .expect("JITO_TPU_JET_SIDECAR_BIND must be host:port");
    let rpc = required_env("JITO_TPU_JET_RPC_URL").expect("set JITO_TPU_JET_RPC_URL");
    let grpc = required_env("JITO_TPU_JET_GRPC_URL")
        .or_else(|| env::var("JITO_TPU_JET_WS_URL").ok())
        .expect("set JITO_TPU_JET_GRPC_URL or JITO_TPU_JET_WS_URL");
    let grpc_x_token = env::var("JITO_TPU_JET_GRPC_X_TOKEN").ok();

    let created = create_yellowstone_tpu_sender(
        sender_config(),
        Keypair::new(),
        Endpoints {
            rpc,
            grpc,
            grpc_x_token,
        },
    )
    .await
    .expect("create Yellowstone Jet TPU sender");

    let state = AppState {
        sender: Arc::new(Mutex::new(created.sender)),
    };
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/send", post(send_transaction))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .expect("bind sidecar listener");
    axum::serve(listener, app).await.expect("serve sidecar");
}

async fn send_transaction(
    State(state): State<AppState>,
    Json(request): Json<SendRequest>,
) -> (StatusCode, Json<SendResponse>) {
    let signature = match Signature::from_str(request.signature.trim()) {
        Ok(signature) => signature,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SendResponse {
                    status: "error",
                    label: "tpu-jet",
                    signature: request.signature,
                    bytes: 0,
                    error_class: Some("invalid_signature"),
                    error: Some(error.to_string()),
                }),
            );
        }
    };
    let wire_transaction = match STANDARD.decode(request.transaction_base64.trim()) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        Ok(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SendResponse {
                    status: "error",
                    label: "tpu-jet",
                    signature: request.signature,
                    bytes: 0,
                    error_class: Some("empty_wire_transaction"),
                    error: Some("transaction_base64 decoded to an empty payload".to_string()),
                }),
            );
        }
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(SendResponse {
                    status: "error",
                    label: "tpu-jet",
                    signature: request.signature,
                    bytes: 0,
                    error_class: Some("invalid_base64"),
                    error: Some(error.to_string()),
                }),
            );
        }
    };
    let bytes = wire_transaction.len();
    let mut sender = state.sender.lock().await;
    match sender.send_txn(signature, wire_transaction).await {
        Ok(()) => (
            StatusCode::OK,
            Json(SendResponse {
                status: "dispatched",
                label: "tpu-jet",
                signature: request.signature,
                bytes,
                error_class: None,
                error: None,
            }),
        ),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(SendResponse {
                status: "error",
                label: "tpu-jet",
                signature: request.signature,
                bytes,
                error_class: Some("dispatch_error"),
                error: Some(error.to_string()),
            }),
        ),
    }
}

fn required_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn sender_config() -> YellowstoneTpuSenderConfig {
    let mut config = YellowstoneTpuSenderConfig::default();
    if let Some(timeout_ms) = positive_u64_env("JITO_TPU_JET_TIMEOUT_MS") {
        config.tpu.send_timeout = Duration::from_millis(timeout_ms);
    }
    if let Some(lookahead) = positive_usize_env("JITO_TPU_JET_FANOUT_SLOTS") {
        config.tpu.leader_prediction_lookahead = NonZeroUsize::new(lookahead);
    }
    config
}

fn positive_u64_env(name: &str) -> Option<u64> {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
}

fn positive_usize_env(name: &str) -> Option<usize> {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
}
