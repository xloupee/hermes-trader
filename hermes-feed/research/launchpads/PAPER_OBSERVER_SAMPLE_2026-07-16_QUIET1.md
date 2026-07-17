# Paper observer quiet-window evidence: 2026-07-16 quiet1

This report records a clean local, read-only five-minute production-feed
window collected after the observer, reconciliation, paper-plan, and readiness
instrumentation changes. No wallet, keystore, signer, broadcast path, Droplet,
or server deployment was used.

## Session boundary and integrity

- Expected authority: `config/launchpad-expected-pins.production.json`
- Fresh observed snapshot: schema 4, 36 pins, L2 block `11682096`, block hash
  `0x9d3e4be4418407082d52cf5a3134451ac59621a1fd11d464587d47aa98c86a93`
- Scored range: L2 blocks `11682302..=11685271`
- Persisted start anchor: block `11682301`, hash
  `0xe59df3c967edadfbb70366a60b9bc215a0ed95e76fb4446534f1d1555a89bffd`
- Persisted cutoff anchor: block `11685271`, hash
  `0xf0748ec34612921f2d68efb95a4eadc0b042cb9ddbdf150c01cbbc1d110ec607`
- Feed coverage closed with zero reconnects. Reconciliation ran at concurrency
  one after producer close and FIFO drain.
- Pipeline: direct feed -> private mode-0600 raw FIFO -> `tee` raw JSONL ->
  private observer FIFO -> `hermes-launchpad-paper --input -`.
- Input/output volume: 3,616 raw frames, 3,617 observer lines, 104
  reconciliation evidence rows, 16 finalized rows, six readiness-window rows,
  and six readiness decisions. Every persisted artifact is mode 0600.
- The fresh snapshot was accepted by observer and reconciler startup validation;
  all expected pin, relationship, semantic, and historical-proof checks were
  therefore satisfied before evidence was admitted.

An earlier ten-minute diagnostic directory,
`.runtime/paper-session-20260716-fresh-10m`, is deliberately excluded. The
wrapper source was edited after feed coverage closed while its shell was still
reading that file, so it exited before reconciliation. Its raw evidence is
preserved for diagnosis but contributes no latency, readiness, or promotion
evidence.

## Live score

Latency is receive-to-observer-output wall time. A dash means there was no
confirmed observation and must not be read as zero.

| Launchpad | Truth | Confirmed | False positives | Missed (detector / coverage) | Latency p50 / p95 / p99 | Quote result |
|---|---:|---:|---:|---:|---|---|
| Bow | 0 | 0 | 0 | 0 (0 / 0) | - | no activity |
| LaunchHood V3 | 3 | 3 | 0 | 0 (0 / 0) | 5.390 / 7.316 / 7.316 ms | 3 available, 3/3 independently matched |
| Clanker | 3 | 0 | 0 | 3 (3 / 0) | - | 3 available, 3/3 independently matched from truth |
| Bankr/Doppler | 2 | 0 | 0 | 2 (2 / 0) | - | 2 blocked |
| Pons | 28 | 28 | 0 | 0 (0 / 0) | 4.911 / 9.360 / 17.064 ms | 1 available and independently matched; 27 not applicable |
| Hood | 0 | 0 | 0 | 0 (0 / 0) | - | no activity |

Pons emitted 40 observer claims: 28 reconciled inside the scored range and 12
were correctly classified as out of scope, not false positives. Flap remained
discovery-only and outside the six-launchpad score and readiness decision.

All 34 eligible action predictions matched. Every eligible independent quote,
entry-direction check, and exit-direction check matched: LaunchHood 3/3,
Clanker 3/3, and Pons 1/1. There were no direction, action, token, pool, or
quote *mismatches*. However, token and pool identity predictions were absent
for every confirmed LaunchHood and Pons transaction. The conservative
readiness window counts those missing fields as six and 56 identity-evidence
failures respectively; they are promotion blockers, not falsely reported
wrong identities.

The clean window therefore identifies two detector defects that require
transaction-level analysis: all three canonical Clanker truths and both
canonical Bankr/Doppler truths were absent from observer claims. Coverage was
complete, so none can be attributed to feed loss.

## Independent paper plans

Three LaunchHood receipts each produced the same fixed tiny, receipt-end-state
simulation:

- Entry: `0.001 WETH`
- Expected output: `729729.244852920426979991` tokens
- Entry minimum at 1% slippage: `722431.952404391222710191` tokens
- Immediate full-position exit: `0.000980107152180750 WETH`
- Exit minimum at 1% slippage: `0.000970306080658942 WETH`
- Simulated round-trip return: `9801 bps`
- Status: `quoted_restriction_gated`
- Execution eligible: false; broadcast: false

One current-generation Pons receipt produced an independent receipt-end-state
simulation:

- Entry: `0.001 WETH`
- Expected output: `663271.426017223774908828` tokens
- Entry minimum at 1% slippage: `656638.711757051537159739` tokens
- Immediate full-position exit: `0.000980106818839881 WETH`
- Exit minimum at 1% slippage: `0.000970305750651482 WETH`
- Simulated round-trip return: `9801 bps`
- Status: `quoted_execution_gated`
- Execution eligible: false; broadcast: false

These plans use independent fixed sizing, slippage, and full-exit policies.
They do not reuse leader amounts and are evidence records, not executable or
live quotes.

## Readiness decision

The conservative gate requires at least 100 independently quote-validated
confirmations per launchpad, at least ten for every supported profile or
envelope, at least three complete non-overlapping windows, and zero false
positives, detector misses, identity/direction/prediction failures, or quote
failures.

| Launchpad | Complete windows | Quote-validated | Main blockers | Ready |
|---|---:|---:|---|---|
| Bow | 1 / 3 | 0 / 100 | no activity; both profiles 0 / 10 | No |
| LaunchHood V3 | 1 / 3 | 3 / 100 | identity evidence missing; profile 3 / 10 | No |
| Clanker | 1 / 3 | 3 / 100 | 3 detector misses; profiles 0 / 10 and 3 / 10 | No |
| Bankr/Doppler | 1 / 3 | 0 / 100 | 2 detector misses; all four strata 0 / 10 | No |
| Pons | 1 / 3 | 1 / 100 | identity evidence missing; profile 1 / 10 | No |
| Hood | 1 / 3 | 0 / 100 | no activity; profile 0 / 10 | No |

No launchpad is ready for a canary. The readiness output always keeps
`authorizes_canary` and `execution_eligible` false; any future canary also
requires separate user approval and remains outside local-development scope.

## Next evidence work

1. Decode the five missed Clanker and Bankr transactions against their pinned
   profiles, add exact positive proofs and closest-neighbor negatives, then
   repeat a fresh clean window.
2. Decide and encode authoritative token/pool identity derivation for
   LaunchHood and current Pons, retaining missing evidence as fail-closed until
   it can be independently reconciled.
3. Accumulate at least two more non-overlapping complete windows and continue
   until every launchpad and profile threshold is met. Sparse Bow and Hood
   activity cannot be promoted from absence of errors.
4. Keep every quote and plan paper-only. Do not enable wallet, signing,
   broadcast, canary, or Droplet paths during this phase.
