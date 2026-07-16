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
| Clanker | factory `0xd3f2...9a94` | `0xf895112a2deed34ba2765d0147aff3494104a28293cc2f19af9275934088da33` | Existing protocol research plus fixed-block code commitment. |
| Clanker | V4 PoolManager `0x8366...0951` | `0xbd3881180b547f5fe817545743cfb4343e96b1bc6640dcd70c106b0066e95626` | Canonical pool event emitter and fixed-block code commitment. |
| Clanker | `ClankerHookStaticFeeV2` `0x48b8...e8cc` | `0x0883056c4856f8fe464ff49f9c1c028455459dad8ceddcc6d5159259fe51e07f` | Official `clanker-devco/v4-contracts` source at `b004c2e` plus fixed-block code commitment. |
| Clanker | locker `0x290f...bc99` | `0x2175e20d41bc72ad6596b2fdd2c43c75e9d8ca10a706a1ca6c1a3d1526c336bc` | Launch receipt identity and fixed-block code commitment. |
| Clanker | descending-MEV module `0xea1f...299e` | `0x0815a0af5e056adaf07a1941b92082caa886b207676bd42c89ea6bde3956bc13` | Official source semantics plus fixed-block code commitment. |
| Clanker | extension `0x6f27...34b5` | `0xf742a12de7ec06481d0e98942d1830d8bf33502d854e5d97062ef5fda6f5e004` | Exact launch receipt identity and fixed-block code commitment. |

Schema version 2 serializes the complete Clanker identity profile and reviewed
fee bounds separately from fresh observations. Historical block `10980306` and
fresh block `11507862` both matched all 26 requested startup identities. The
Clanker fee values emitted for an individual pool remain receipt-local state;
the expected document pins the official contract bounds instead of copying an
observed pool's values into expected configuration: static LP fees are capped
at 100,000 ppm, the MEV override at 800,000 ppm, and decay at 120 seconds, with
the pinned one-second guard and 20% protocol share.

Pons and Flap identities remain the exact constants recorded in
`PONS_FAMILY.md` and `FLAP_SH_EVIDENCE.json`. They are validated by the registry
and fresh observed snapshot, but are not yet serialized in the expected
document.

## Explicit gaps

- Schema version 2 still has no configurable fields for Pons, Flap, or Permit2
  identities. They must not be described as centrally reviewed configuration
  until the schema is extended again.
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
