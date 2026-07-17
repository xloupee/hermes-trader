# Independent production-pin attestation at `74c1062`

Review date: 2026-07-17 UTC. Repository parent:
`74c1062678591ea5e55ec9c2140e0e22a9d5551c`. Network evidence: Robinhood
Chain mainnet, chain ID `4663`.

Machine-readable companion:
`PRODUCTION_PIN_INDEPENDENT_ATTESTATION_2026-07-17_74C1062.json`.

## Decision

The exact production expected-pin configuration is independently attested as
internally complete, source-bound, fixed-boundary-backed, and consistent with
the retained fresh startup snapshot. All 42 serialized non-null runtime or
EIP-7702 designator commitments occur in the independently collected fresh
41-address snapshot. The difference is intentional: the configuration repeats
some commitments as coherence checks and shares several dependencies between
protocols.

No observed hash was promoted into expected configuration. The expected
document existed at its older reviewed boundary, has a distinct role and
provenance, remained byte-identical through fresh collection, and is still
byte-identical at this review parent. The snapshot and its report are separate
content-addressed artifacts.

No expected pin or implementation file was changed. This attestation is
paper-only and grants no canary, signing, broadcast, execution, deployment,
server, or Droplet authority.

## Exact artifact identity

| Artifact | SHA-256 | Keccak-256 |
| --- | --- | --- |
| `config/launchpad-expected-pins.production.json` | `36e632865b3fef00217fe5cb454f1fb7272025e81a8d25c3369eee0d4c1b8820` | `0x3a192547af7a76a47d5de1b8102da0dc0bb77b7047a53c855b7d264aefa1a34c` |
| Historical reviewed-boundary snapshot | `5770df0407c962411f68d5d3cc238c6a76273bb1f02bef61d7485bac7ff32d4c` | `0xf8406de82cb4817d28fb358edf8980a5258889cb60363594927f4e80b969152e` |
| Fresh startup snapshot | `33072122b2c68fa9f2c4573819a92e22672d2b4e893bbf0c4eec9ed06d0275b5` | `0xb55d32a0b08272961c43bd59879adde1fb1af6d705122b2ec4edb864e22b1683` |
| Fresh snapshot report | `3242a08073d33559ce80bb3416217cc8fe4d58c714a4eb2ed0ea2493fbc16693` | `0x78266070a92a8df135bc9b37d1aa3918c05b67e10ad329ec0d8e7a46dc81c245` |
| Prior checked-in pin review | `1a2171d830399dbf25bc3104ded7d0e23e65b05378c99dc27d5fe5afea8c470e` | `0x190f2b8df42ab2ccb25e2fde6b65a758f82340d28231bcc7769b3e5bd6643e91` |
| Checked-in fresh revalidation JSON | `83a6d32f8e62eb69fe5ff0634d058e26cf4c3b9a18caf383e5d4fcb2e59c2485` | `0x8ffd3b9bc92a7783a57fba99f941aabae715b759811ccf3ad6a35fc2c0773484` |

The expected document is schema `4`, role `expected_protocol_pins`, provenance
`reviewed_protocol_pins`. Its fixed review boundary is L2 `10980306`, block
`0x918363e5b20e86dbe7e952f261a60c9882975ec434abb5815a9dbecdc6354173`,
L1 `25542926`, timestamp `1784177178`.

The retained historical snapshot has the same boundary and contains 39 unique
addresses. The two later additions are the separately reviewed Pons EIP-7702
account/implementation pair.

The fresh snapshot is role `observed_startup_snapshot`, provenance
`startup_observation`, at L2 `11986583`, block
`0x98c0b73f140330b1384bb5295bb47a26d60c0050e8b5ddb5444e4bf6efc4ee45`,
L1 `25551300`, timestamp `1784277972`. It contains 41 unique addresses. Its
report says `expected_validation_passed=true` after 77 logical requests and 77
HTTP attempts, with zero retries, rate limits, server errors, or transport
errors.

## Review method

This review did not trust the prior prose conclusion alone. It:

1. independently hashed the exact expected, historical, fresh, and report
   files;
2. decoded the configuration and inventoried every runtime/designator
   commitment, semantic profile field, expected document role, smart-account
   pair, and intentionally null discovery field;
3. compared the older `reviewed_at` boundary with the retained historical
   snapshot and all 42 serialized runtime/designator commitments with the fresh
   snapshot;
4. inspected strict startup validation for document separation, complete
   production-profile equality, address/implementation matching, runtime hash,
   code length, mutable Hood configuration, and delegation-pair completeness;
5. inspected the real-proof and negative-test surfaces supporting Pons,
   Clanker, Bankr/Doppler, and Hood semantics; and
6. required the expected config, source tree, and scripts to have zero diff
   from parent `74c1062`.

## Per-profile result

| Profile | Attested surface | Result |
| --- | --- | --- |
| Document authority | One historical boundary; one expected role; seven intentionally null discovery runtime fields | Attested. Schema/provenance pairing is strict; LeaveHood factory/core, Klik, and Trench remain unauthoritative. |
| Bow | One factory runtime commitment | Attested as a fixed-block runtime commitment. Explorer source reproducibility is not claimed. |
| LaunchHood V3 | One factory runtime plus one three-field token implementation identity | Attested. Startup requires exact implementation address, byte length, and runtime hash separately from the factory. |
| Clanker V4 | Seven runtime commitments and five semantic policy fields | Attested. Config must equal the exact production factory/deployer/PoolManager/hook/locker/MEV/extension profile and exact fee/decay/guard/share policy. |
| Bankr/Doppler | Eight profile runtimes, thirteen semantic fields, one call pin, one EntryPoint, one account, one delegation pair | Attested. The strict proof is exact EntryPoint v0.7, `execute(bytes32,bytes)`, all-zero mode, designator, Kernel implementation/runtime, zero-value Airlock call, and selector `0x882db707`. |
| Pons | Seven V3 runtime identities plus a twelve-field EIP-7702 profile containing two runtime/designator commitments | Attested. Each V3 identity must appear exactly once. The self-batch profile must equal production and the fresh snapshot must contain both the 23-byte designator and implementation runtime. |
| Hood | Ten runtime identities, twenty semantic fields, nine other profile fields, and one scalar factory coherence field | Attested. Exact production equality, unique role/address identities, Safe pair, callable links, mutable config, curve, fee, migration, owner, tick, and guard semantics are all startup-bound. |

## Proof that observed values were not promoted

- Expected and fresh documents have different roles, provenances, boundaries,
  SHA-256 hashes, and Keccak-256 hashes.
- The expected boundary at L2 `10980306` predates the fresh boundary at L2
  `11986583`.
- The expected SHA-256 matches the value recorded before the fresh comparison
  and remains unchanged at parent `74c1062`.
- The snapshot writer creates a new observed document and refuses overwrite or
  hard-linked publication targets.
- Startup rejects expected-as-observed provenance, incomplete profiles, missing
  pins, changed hashes or byte lengths, and incomplete smart-account
  factory/delegation pairs.
- This commit adds only this JSON/Markdown attestation pair.

## Limits

- This is a source/document/artifact review and did not repeat public RPC
  requests. The fixed and fresh evidence is content-addressed but retained
  outside Git.
- The historical snapshot contains 39 pins. Pons EIP-7702 is substantiated
  separately by its exact proof provenance and negative tests, then by the
  later 41-pin fresh snapshot.
- Bow and the current Pons factory retain documented source-verification gaps.
  Runtime identity is attested; reproducible source builds are not.
- Schema 4 has no configurable Flap or Permit2 expected-pin fields. Hardcoded
  source constants are outside this configuration attestation.
- One fresh boundary does not prove continuous stability.
- Pin integrity does not prove feed quality, quote correctness, fillability,
  execution safety, or canary readiness.

## Conclusion

Every serialized production expected-pin entry is substantiated by the
combination of source-fixed production profiles, retained fixed-boundary proof,
strict startup comparison, and fresh snapshot evidence. No unsubstantiated
entry was found, and no observed hash was adopted as expected authority.

This conclusion remains paper-only:
`authorizes_canary=false`, `execution_eligible=false`, signing disabled,
broadcast disabled, deployment unauthorized, and server/Droplet work
unauthorized.
