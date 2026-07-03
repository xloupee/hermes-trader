# Yellowstone Jet TPU Sidecar

This crate is the production-intended local sidecar for the `tpu-jet` send lane.
It keeps `yellowstone-jet-tpu-client` and its Solana 3.x dependency graph out of
the main `jito-shredstream-rs` worker.

The sidecar binds to `127.0.0.1:8787` by default and exposes:

- `GET /health`
- `POST /send` with `{ "signature": "...", "transactionBase64": "..." }`

A successful `/send` response means the transaction was dispatched through the
Jet sender. It is not an ACK or landing claim; live promotion still depends on
`slotDelta`, `txDelta`, same-slot rate, landed rate, and duplicate-risk checks.

Build from `tools/jito-shredstream-rs`:

```sh
cargo build --release \
  --manifest-path yellowstone-jet-sidecar/Cargo.toml \
  --target-dir target/yellowstone-jet-sidecar \
  --bin yellowstone-jet-sidecar
```

Required runtime env:

```sh
JITO_TPU_JET_RPC_URL=<state-rpc-url-or-SOLANA_RPC_URL>
JITO_TPU_JET_GRPC_URL=<yellowstone-geyser-grpc-url>
JITO_TPU_JET_GRPC_X_TOKEN=<x-token-if-required>
JITO_TPU_JET_SIDECAR_BIND=127.0.0.1:8787
JITO_TPU_JET_FANOUT_SLOTS=1
JITO_TPU_JET_TIMEOUT_MS=30
```

Provider aliases such as `JITO_ERPC_YELLOWSTONE_GRPC_URL` and
`JITO_SHREDER_FASTLANE_GRPC_URL` are resolved by `../run-tpu-jet-sidecar.sh`.

The earlier `spikes/yellowstone-jet-compat` crate remains as compatibility
evidence for the original API shape.
