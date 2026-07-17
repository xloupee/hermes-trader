# Differential validation evidence

Validation date: 2026-07-12

Reference software:

- Robinhood Chain ID: `4663`
- Nitro tag: `v3.11.2`
- Nitro commit: `3599acae1ad2fab4059fc46453c9cd3294126641`
- Offchain go-ethereum commit: `f3a977ddf30b138da2fe673ac5cbff2bc6dd4c88`
- Go toolchain: `go1.26.5 linux/amd64`
- Rust transaction decoder: Alloy `2.1.1`

Input:

- 46 recorded, real Robinhood mainnet feed frames
- Frames included initial catch-up and live traffic
- 6,286 signed transactions

Result:

- Rust fingerprints: 6,286
- Go/Nitro fingerprints: 6,286
- Ordered comparison: exact byte-for-byte match
- Both fingerprint streams SHA-256:
  `e282bf4d46ea1e1534d2d39a84cf20dd573e301b1b19a50d645efc1559f3edd2`

The comparison covers transaction extraction through recursive Nitro batches,
EIP-2718 decoding, and transaction hashing. The Rust live decoder remains
deliberately stricter on malformed batch tails: it fails closed, whereas Nitro
treats a bytestring read error as the end of a batch.

The raw capture is excluded from git because it contains several megabytes of
ephemeral public-chain traffic. Reproduce the check using `README.md` and
`verify.sh` in this directory.
