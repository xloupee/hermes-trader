# copy-rs

Local-only Rust dry-run harness for copy-trade planning.

This tool does not sign, send, submit, or hold private keys. It reads saved Helius `SWAP` JSON fixtures and prints a deterministic copy/skip plan.

## Run

Plan from a local fixture:

```bash
cargo run --manifest-path tools/copy-rs/Cargo.toml -- \
  plan \
  --input tools/copy-rs/fixtures/helius-swap-sol-to-token.json \
  --target-wallet 39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg \
  --copy-sol 0.01
```

Keep polling Helius and print readable dry-run decisions until stopped:

```bash
export HELIUS_API_KEY=your_helius_key

cargo run --manifest-path tools/copy-rs/Cargo.toml -- \
  watch \
  --target-wallet CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o \
  --copy-sol 0.01 \
  --interval-seconds 5
```

`watch` prints one readable line per copyable buy, including the coin mint address and token symbol/name when Helius includes it. Skipped sells and token rotations are hidden in readable mode. It never signs or sends a transaction.

Build a PumpPortal Local Transaction API request from a local fixture and ask PumpPortal for an unsigned serialized transaction:

```bash
cargo run --manifest-path tools/copy-rs/Cargo.toml -- \
  build-local \
  --input tools/copy-rs/fixtures/helius-swap-sol-to-token.json \
  --target-wallet 39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg \
  --copy-sol 0.01 \
  --public-key your_public_wallet_address \
  --slippage 10 \
  --priority-fee 0.00005 \
  --pool auto
```

Build PumpPortal local transactions while watching:

```bash
cargo run --manifest-path tools/copy-rs/Cargo.toml -- \
  watch \
  --target-wallet CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o \
  --copy-sol 0.01 \
  --interval-seconds 5 \
  --pumpportal-build \
  --public-key your_public_wallet_address \
  --slippage 10 \
  --priority-fee 0.00005 \
  --pool auto
```

PumpPortal build mode only requests an unsigned local transaction. It does not load a private key, sign, or send.

Use `--json` when you want machine-readable JSON lines for every new swap, including skipped events:

```bash
cargo run --manifest-path tools/copy-rs/Cargo.toml -- \
  watch \
  --target-wallet CyaE1VxvBrahnPWkqm5VsdCvyS2QmNht2UFrKJHga54o \
  --copy-sol 0.01 \
  --interval-seconds 5 \
  --json
```

## Test

```bash
cargo test --manifest-path tools/copy-rs/Cargo.toml
```

## Current rules

- Copy only SOL -> token swaps, treated as buys.
- Use fixed `--copy-sol` amount.
- PumpPortal Local build mode uses `action=buy`, `denominatedInSol=true`, and `pool=auto` by default.
- Skip sells and token rotations with `only SOL to token buys are copied in dry-run v1`.
- Skip non-`SWAP` events.
- Skip events that do not involve `--target-wallet`.
- Skip when output mint cannot be inferred.
