# Jito ShredStream Feed Probe

Rust-only probe for the local Jito ShredStream proxy. It connects to
`ShredstreamProxy.SubscribeEntries`, decodes bincode `Vec<Entry>` payloads,
scans transactions for tracked wallets, and prints normalized copytrade events.

This is observe-only tooling. It does not submit trades, write to Telegram, or
touch Supabase.

## Run

On the VPS where `jito-shredstream-proxy` is listening on `127.0.0.1:9999`:

```bash
cargo run --manifest-path tools/jito-shredstream-rs/Cargo.toml -- \
  live \
  --endpoint http://127.0.0.1:9999 \
  --target-wallet <TARGET_WALLET> \
  --stats
```

Multiple wallets can be comma-separated:

```bash
SHREDSTREAM_TARGET_WALLETS=<WALLET_A>,<WALLET_B> \
cargo run --manifest-path tools/jito-shredstream-rs/Cargo.toml -- live
```

Use `--include-rejections` only for short debugging runs. It prints one line per
non-matching transaction and gets noisy fast.

## Output

Matched trades are emitted as JSONL with schema `copytrade.feed.event.v1`.

```json
{
  "schema": "copytrade.feed.event.v1",
  "provider": "shredstream",
  "source": "jito-proxy",
  "targetWallet": "...",
  "action": "buy",
  "mint": "...",
  "signature": "...",
  "route": "pump",
  "input": { "mint": "So11111111111111111111111111111111111111112", "amount": 0.1 },
  "output": { "mint": "...", "amount": 12345.67 },
  "copyable": true
}
```
