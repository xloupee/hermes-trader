# Paper observer evidence sample: 2026-07-16

This report records a local, read-only five-minute session. No wallet,
keystore, signer, broadcast path, or server was used.

## Session boundary

- Expected authority: `config/launchpad-expected-pins.production.json`
- Fresh observed snapshot: schema 4, 36 pins, L2 block `11619224`, block hash
  `0xd59487deeddb984cf23ee26af67b87463f1d14e1d5cce911e968a8c4498a2139`
- Scored range: L2 blocks `11621789..=11624770`
- Start hash:
  `0xb486e9de75e22f5ad4bb6f7826e0c3938abb47d5820f5ca09884171195dd7581`
- Cutoff hash:
  `0x4f44d7e616e144b3c61f49a83e29a282ca293e88ead5f0fc38f392788afa42ae`
- Complete canonical scan: 2,982 blocks, 48 primary event logs, 48 unique
  protocol keys, two confirmations
- Pipeline: private raw FIFO -> `tee` raw JSONL -> private observer FIFO ->
  paper observer; reconciliation began only after producer close and FIFO drain
- Input/output volume: 3,769 raw frames, 3,770 observer lines, 116
  reconciliation lines

## Original live score

Latency is source receive to observer output. Null latency means there was no
confirmed observation and must not be represented as zero.

| Launchpad | Truth | Confirmed | False positive | Missed (detector / coverage) | Latency p50 / p95 / p99 | Quote result |
|---|---:|---:|---:|---:|---|---|
| Bow | 2 | 2 | 0 | 0 (0 / 0) | 0.265 / 0.357 / 0.357 ms | 0 available, 2 blocked |
| LaunchHood V3 | 2 | 0 | 0 | 2 (2 / 0) | null / null / null | 2 available |
| Clanker | 0 | 0 | 0 | 0 (0 / 0) | null / null / null | no activity |
| Bankr/Doppler | 5 | 1 | 0 | 4 (4 / 0) | 0.323 / 0.323 / 0.323 ms | 0 available, 5 blocked |
| Pons | 39 | 39 | 0 | 0 (0 / 0) | 0.394 / 1.424 / 1.475 ms | 39 legacy, not applicable |
| Hood | 0 | 0 | 0 | 0 (0 / 0) | null / null / null | no activity |

There were 14 out-of-range Pons observations from websocket backlog. They were
classified as out of scope, not false positives. There were also 51 Flap
observer-only discovery records; Flap remains outside the scored production
set until its semantics are complete.

## Prediction and quote findings

- No eligible action prediction mismatched. Legacy Pons had 39/39 action
  matches, but its token and pool predictions were explicitly missing rather
  than guessed.
- LaunchHood's two detector misses were direct canonical calls with nonzero
  value and leader `minTokensOut = 0`. The observer incorrectly treated that
  leader-controlled slippage field as an identity requirement. The corrected
  adapter accepts zero only for observation, explicitly requires `dexId = 0`,
  keeps token/pool prediction unavailable, and remains execution gated.
- Replaying the captured feed through the corrected observer produced two of
  two confirmed LaunchHood observations, zero false positives, zero misses,
  two action matches, and two finalized non-broadcast paper plans. Replay
  latency is intentionally excluded because replay wall time is not live
  observation latency.
- Both Bow launches were correctly observed but the strict quote rejected a
  payable embedded initial buy. The receipts coherently bind transaction
  value, WETH pool delta, token pool delta, and recipient. This is a quote
  implementation gap, not evidence of an unsafe receipt. Bow remains blocked
  until the embedded-buy reconstruction is implemented and negatively tested.
- Four Bankr misses used the exact reviewed ERC-7579 selector, zero mode/value,
  Airlock target, and inner selector, but rotated EIP-7702 accounts with the
  same reviewed designator and Kernel. The fifth was a direct Airlock call.
  Account identity must be verified at the canonical receipt block before
  admitting these envelopes; no global selector or unpinned-account fallback
  is acceptable.

## LaunchHood independent paper outcomes

Both confirmed LaunchHood receipts independently reconstructed the same fixed
tiny policy outcome after the embedded launch swap:

- Entry size: `0.001 WETH`
- Expected entry output: `729729.244852920426979991` tokens
- Entry minimum at 1% slippage: `722431.952404391222710191` tokens
- Immediate full-position exit: `0.000980107152180750 WETH`
- Exit minimum at 1% slippage: `0.000970306080658942 WETH`
- Simulated round-trip return: `9801 bps`
- Execution eligible: false
- Broadcast: false

These are paper simulations from receipt-end state, not executable or live
quotes. The token restriction and runtime checks remain unsatisfied.

## Promotion decision

No launchpad is ready for a canary from this sample. LaunchHood needs a fresh
post-fix live window with valid latency and a larger confirmed sample. Bow and
Bankr require the quote/envelope gaps above to be closed. Pons activity was
legacy discovery-only. Clanker and Hood had no events in this window. Any
canary remains separately approval-gated and out of scope for local work.
