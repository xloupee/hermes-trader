# Production-pin revalidation at `19fe23a`

Evidence date: 2026-07-17 UTC. Network: Robinhood Chain mainnet, chain ID
`4663`. Repository commit:
`19fe23af9b0a8283dfed6943aa8c4ed53ccb09d4`.

Machine-readable checked-in evidence:
`PRODUCTION_PIN_REVALIDATION_2026-07-17_19FE23A.json`.

## Decision

A fresh startup-boundary snapshot passed validation against the unchanged
production expected-pin document. The observation contains 41 unique pin
addresses and reports no RPC retries, rate limits, server errors, or transport
errors. The Bankr EIP-7702 account designator, delegated Kernel runtime,
EntryPoint path, Airlock runtime, and inner selector all match the strict
per-account execution profile.

This is pin evidence at one boundary only. It is not paper-observer campaign
evidence and does not authorize a canary or execution. No wallet, key, signing,
broadcast, execution, canary, deployment, Droplet, or server action occurred.

## Content-addressed sources

The fresh artifacts were produced locally by the paper-only pin-snapshot path
and retained under the ignored `.runtime` directory. Their exact identities
are:

| Artifact | Runtime path | SHA-256 |
| --- | --- | --- |
| Observed startup snapshot | `hermes-feed/.runtime/pin-revalidation-19fe23a-root/observed-startup.json` | `33072122b2c68fa9f2c4573819a92e22672d2b4e893bbf0c4eec9ed06d0275b5` |
| Snapshot report | `hermes-feed/.runtime/pin-revalidation-19fe23a-root/snapshot-report.json` | `3242a08073d33559ce80bb3416217cc8fe4d58c714a4eb2ed0ea2493fbc16693` |
| Production expected pins | `hermes-feed/config/launchpad-expected-pins.production.json` | `36e632865b3fef00217fe5cb454f1fb7272025e81a8d25c3369eee0d4c1b8820` |

The expected-pin document was read but not changed. The source snapshot and
report remain runtime artifacts; the checked-in JSON records their hashes and
the independently checked facts needed for this decision.

## Fresh boundary

| Field | Observed value |
| --- | --- |
| Profile | `startup_snapshot` |
| L2 block | `11986583` |
| L2 block hash | `0x98c0b73f140330b1384bb5295bb47a26d60c0050e8b5ddb5444e4bf6efc4ee45` |
| L1 block | `25551300` |
| Block timestamp | `1784277972` (`2026-07-17T08:46:12Z`) |
| Observed pins | `41` unique addresses |
| Expected validation | `passed` |

The report contains exactly one verified boundary, the fresh startup snapshot
above. It does not claim a historical range or continuous monitoring period.

## Strict Bankr EIP-7702 proof

The snapshot report revalidates proof transaction
`0xc6597fe88f8f3f16b4ba6613c25050d75dc6f3c2b2c5315f0b47828f98f0609c`
at L2 block `10976731`, transaction index `1`.

The transaction uses EntryPoint v0.7
`0x0000000071727de22e5e9d8baf0edac6f37da032`, leader/account
`0xff89978cb8171132395741b785d4a1f7e3efa124`, and selector `0xe9ae5c53`
for `execute(bytes32,bytes)`. The execution mode is all zero and the admitted
per-account profile is `erc7579_single_call`.

The leader's 23-byte EIP-7702 designator is
`0xef0100d6cedde84be40893d153be9d467cd6ad37875b28`. Its hash is
`0x4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4`.
It delegates to Kernel
`0xd6cedde84be40893d153be9d467cd6ad37875b28`, whose runtime hash is
`0x6f6d6691dc11fda98d3102802f20e7e816ccc576c16c9279ee1a884a51d1935d`.
Both values match the expected per-account profile; its factory and factory
runtime hash remain explicitly `null`, so no incomplete factory/delegation
pair is inferred.

The single unwrapped call has value zero and targets Airlock
`0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862`, runtime hash
`0x86b37100cbe9841771c452a592985b4e921254b127a380246073b84ec953f7f8`,
with inner selector `0x882db707`. The proof reports unwrap depth `1` and one
inner call. These exact values match the strict Bankr destination profile.

## RPC health

The snapshot made 77 logical requests and 77 HTTP attempts. Retries, rate
limits, server errors, and transport errors were all zero. These counters
describe this snapshot run only; they are not a provider-availability SLA.

## Independent checks

The evidence preparation re-hashed both runtime artifacts, checked the
expected-pin file hash, parsed all three JSON documents, and asserted:

- chain ID and all L1/L2 boundary fields agree between snapshot and report;
- the snapshot has exactly 41 pin records and 41 unique addresses;
- `expected_validation_passed=true` and exactly one verified startup boundary;
- the Bankr account designator and delegated Kernel each occur exactly once in
  the observed pin set with the expected implementation and runtime hashes;
- the observed Airlock runtime and selector match the production expected-pin
  profile; and
- all reported RPC error counters are zero.

## Limitations and next evidence

This single observation can detect drift at the recorded startup boundary; it
cannot establish that pins remained unchanged before or after that block. The
runtime source files are content-addressed but not checked into Git, so their
availability remains an operational retention concern.

This batch does not measure feed latency, false positives, detector misses,
receipt/event reconciliation, quote accuracy, slippage, sizing, or exit plans.
Those claims require the separate immutable multi-window paper campaign. Until
that evidence passes its thresholds, `paper_evidence_ready=false`,
`authorizes_canary=false`, and `execution_eligible=false` for Bankr and every
other launchpad.
