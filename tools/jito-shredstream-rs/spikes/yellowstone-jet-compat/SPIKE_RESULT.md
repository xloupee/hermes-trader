# Yellowstone Jet TPU Spike Result

Date: 2026-06-22

## Result

`yellowstone-jet-tpu-client = 0.3.1` compiles in an isolated spike and supports
the shape needed by the copy-trade worker: a known signature plus already
serialized transaction bytes.

Verified command:

```sh
cargo check --manifest-path tools/jito-shredstream-rs/spikes/yellowstone-jet-compat/Cargo.toml
```

The spike serializes a `VersionedTransaction` with `bincode` and keeps the
Yellowstone sender constructor and response types typechecked without opening a
network connection.

It also includes a compile-checked `yellowstone-jet-sidecar` binary that exposes
a local `/send` JSON contract and calls:

```rust
sender.send_txn(signature, wire_transaction).await
```

This proves the sidecar integration boundary can keep Jet's newer dependency
graph outside the production worker while still accepting the same signed bytes.

## Integration Decision

Do not pull Jet directly into the live worker as the first in-process lane. Jet
compiled by pulling a newer Yellowstone/Solana dependency graph than the
production worker's current Solana 2.2.1 stack.

Use Jet through a local sidecar lane:

- lane label/kind: `tpu-jet` / `tpu_jet`
- default off
- same signed wire bytes as Helius
- no per-trade RPC/filesystem/Supabase/Telegram/config lookup in the worker
- Helius/RPC/Jito fallback preserved in `mixed`
- dispatch-only telemetry; no ACK claim

Keep the direct TPU fallback available:

- `solana-tpu-client = 2.2.1`
- lane label/kind: `tpu-quic`
- default off
- same signed wire bytes as Helius
- no per-trade RPC/filesystem/Supabase/Telegram/config lookup
- Helius/RPC/Jito fallback preserved

Revisit in-process Jet only after a deliberate Solana dependency upgrade.

## Canary Gates

Treat TPU dispatch as local dispatch only, not ACK or landing proof. Promotion
requires fresh post-restart samples showing no landed-rate regression, better
same-slot rate, materially better p50/p90 `txDelta`, better `txDelta<=50`, no
hot-path timing regression, and no double-send/double-buy risk.
