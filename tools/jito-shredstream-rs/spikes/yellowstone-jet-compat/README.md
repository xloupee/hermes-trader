# Yellowstone Jet TPU compatibility spike

This isolated crate probes whether `yellowstone-jet-tpu-client` can be used as
a direct TPU lane for the live Rust copy worker.

The production worker is currently on Solana 2.2.1 split crates. Jet 0.3.1 uses
newer Solana crates, so this spike intentionally lives outside the production
crate and does not participate in the worker build.

The spike verifies the API shape we need:

- create a `VersionedTransaction`
- serialize it to wire bytes with `bincode`
- pass `(signature, bytes)` to the Jet sender surface

Run:

```sh
cargo check --manifest-path tools/jito-shredstream-rs/spikes/yellowstone-jet-compat/Cargo.toml
```

The `yellowstone-jet-sidecar` bin is compile-checked by the same command. It is
not meant to be launched without live RPC/gRPC credentials.

Compile-only sidecar contract:

```http
POST /send
Content-Type: application/json

{
  "signature": "<base58 tx signature>",
  "transactionBase64": "<bincode serialized VersionedTransaction>"
}
```

Success response:

```json
{
  "status": "dispatched",
  "label": "tpu-jet",
  "signature": "<base58 tx signature>",
  "bytes": 612,
  "errorClass": null,
  "error": null
}
```

The sidecar uses:

- `JITO_TPU_JET_SIDECAR_BIND`, default `127.0.0.1:8787`
- `JITO_TPU_JET_RPC_URL`
- `JITO_TPU_JET_GRPC_URL`, or `JITO_TPU_JET_WS_URL` as a compatibility alias
- `JITO_TPU_JET_GRPC_X_TOKEN`, optional
- `JITO_TPU_JET_TIMEOUT_MS`, mapped to Jet `tpu.send_timeout`
- `JITO_TPU_JET_FANOUT_SLOTS`, mapped to Jet leader prediction lookahead

The production worker should only call this as a send lane after the sender has
started and warmed. Do not use it for per-trade RPC/config discovery.

Do not put live RPC, gRPC, or keypair values in this directory.
