# Confirmed reserve-cache validation

Validation date: 2026-07-12 UTC

## Registry bootstrap

The canonical factory grew during development, demonstrating why a static pair
list is unsafe. Plain JSON-RPC batching hit HTTP 429 after 219.7 seconds. The
optimized implementation uses the deployed Multicall3 contract at
`0xca11bde05977b3631167028862be2a173976ca11`.

- 50-pair Multicall canary: 618 ms
- complete 5,369-pair pinned bootstrap: 36.140 seconds
- checkpoint size: approximately 1.21 MB
- checkpoint restore plus five-second run: 5.686 seconds, so restore overhead
  was approximately 686 ms

Checkpoint restoration verifies the canonical block hash. Update ranges must
begin at the prior cursor plus one, so skipped poll cycles are backfilled and a
gap fails closed. Checkpoints are replaced atomically and rate-limited to avoid
rewriting the registry on every L2 block.

## Incremental pairs

After the initial bootstrap, confirmed logs contained updates for newly created
pairs. Incremental factory-tail loading added six pairs without a full rebuild.
The installed shadow subsequently added another pair, reaching 5,378 cached
pairs.

## Live shadow

The release shadow runs separately from the existing intent-only observer. In
the first qualified sample:

| Metric | Result |
|---|---:|
| Candidates | 20 |
| Reserve-aware follows | 15 |
| Below-minimum rejects | 5 |
| Cache lag | 2 blocks maximum |
| Decision time | 4.9–86.8 µs |
| Errors | 0 |
| Restarts | 0 |

The five below-minimum rejections show that scaling the leader's calldata
minimum is insufficient: applying leader price impact to confirmed pre-state
can make the follower quote unsafe even when the typed intent passes.

This remains paper-only. No key loading, signing, nonce allocation, simulation
RPC, transaction submission, or mainnet asset movement exists in this runtime.
