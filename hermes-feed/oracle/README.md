# Nitro differential oracle

This program validates Hermes transaction decoding against the canonical Nitro
Go implementation shipped for Robinhood Chain.

1. Record real feed frames with `hermes-feed probe --record`.
2. Replay them with `--emit-tx-hashes` to produce Rust fingerprints.
3. Check out `OffchainLabs/nitro` tag `v3.11.2` (`3599acae1ad2...`) and
   initialize its pinned `go-ethereum` submodule (`f3a977ddf30b...`).
4. Stage `main.go` under the go-ethereum checkout's
   `cmd/hermes-nitro-oracle` directory.
5. Run it against the same frame recording.
6. Compare the ordered JSON transaction records byte-for-byte.

Run `verify.sh <rust-output.jsonl> <go-output.jsonl>` for the final count,
SHA-256 and ordered comparison.

The oracle mirrors Nitro's batch and signed-transaction branches and executes
them with Nitro's exact Offchain Labs go-ethereum fork. It also preserves
Nitro's explicit rejection of EIP-4844 blob and Arbitrum-internal transaction
types.

The first recorded-mainnet validation matched all 6,286 transaction hashes.
See [VALIDATION.md](VALIDATION.md) for the pinned versions and evidence.
