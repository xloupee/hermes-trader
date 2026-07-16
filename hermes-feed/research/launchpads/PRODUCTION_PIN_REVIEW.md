# Production launchpad pin review

Review date: 2026-07-16 UTC. Network: Robinhood Chain mainnet, chain ID 4663.

The reviewed expected configuration is
`config/launchpad-expected-pins.production.json`. It is independent of the
fresh startup snapshot: hashes were committed from protocol research, verified
explorer source where available, and a historical fixed-block code review at
L2 block `10980306` (block hash prefix `0x918363e5`). A separate latest-block
snapshot must match before the observer starts.

## Reviewed configurable pins

| Protocol | Address or role | Reviewed runtime hash | Evidence boundary |
|---|---|---|---|
| Bow | factory `0xc70e510e14710ea535cab7b2414860af63feab79` | `0x8d56cbcdf72dbf04ed8170d55878cc894997ccc54c2ab0aec782274eb7fe7a14` | Official/research address plus fixed-block code commitment; explorer source is not verified. |
| LaunchHood V3 | factory `0x62b33a039d289cbda50ebeb72fe4261449e61bcf` | `0x9b785dd157fe757dd427822df3e2bc3a1b6134f1d338b21c36c3de279bb67766` | Verified explorer ABI and fixed-block code commitment. |
| Hood | factory `0x5fcc1df0dc020cf454e742e9a8ae2554c37a452c` | `0x4aa0ce56b5b67d27f2fab59dcb796fa552d10ceafdecb06e088cdd254c92c0fc` | Verified `HoodCustomLaunchpad` explorer source and fixed-block code commitment. |
| Bankr/Doppler | Create emitter `0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862` | `0x86b37100cbe9841771c452a592985b4e921254b127a380246073b84ec953f7f8` | Proof receipt and fixed-block code commitment; only selector `0x882db707` is admitted. |
| Bankr wrapper | EntryPoint v0.7 | `0x8db5ff695839d655407cc8490bb7a5d82337a86a6b39c3f0258aa6c3b582fc58` | Canonical EntryPoint address plus fixed-block code commitment. |
| Bankr leader | EIP-7702 designator | `0x4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4` | Exact `ef0100 || d6cedd...75b28` designator from proof account. |
| Bankr leader | delegated Kernel runtime | `0x6f6d6691dc11fda98d3102802f20e7e816ccc576c16c9279ee1a884a51d1935d` | Fixed-block delegated implementation code commitment. |

Pons and Flap identities remain the exact constants recorded in
`PONS_FAMILY.md` and `FLAP_SH_EVIDENCE.json`. Clanker remains pinned to the
factory hash recorded in `ECOSYSTEM_TIER2.md`. These identities are validated
by the registry and fresh observed snapshot, but schema version 1 does not yet
serialize them in the expected document.

## Explicit gaps

- Schema version 1 has no configurable fields for Clanker, Pons, Flap,
  PoolManager, locker, hook, or Permit2 identities. They must not be described
  as centrally reviewed configuration until the schema is extended.
- Bow's explorer source is unverified; its reviewed hash is a fixed-block code
  commitment, not a source-build reproducibility claim.
- Pons current factory and Flap VaultPortal implementation retain their
  documented source-verification gaps and remain paper/discovery only.
- LeaveHood, Klik, and Trench fields are intentionally null. A fresh observation
  is not authority to populate an expected pin.

## Reproduction

Historical review comparison:

```sh
cargo run --bin hermes-launchpad-pin-snapshot -- \
  --l2-block 10980306 \
  --expected-pins config/launchpad-expected-pins.production.json
```

Fresh startup comparison writes a separate observed document:

```sh
cargo run --bin hermes-launchpad-pin-snapshot -- \
  --expected-pins config/launchpad-expected-pins.production.json \
  --snapshot-output .runtime/launchpad-observed-startup.json
```
