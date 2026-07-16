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
| Hood | V3 migrator `0x5790...edbe` | `0x88b7c4f6dfb99df8493cf7b7905a538212fc1c7eb176ffbbcaade5a6988c83d6` | Verified source, exact factory link, and complete graduation proof `0x946fea...f645`. |
| Hood | permanent locker `0xad69...06aa` | `0xee4522db997a71e396e90ef14a123f3a4a857268040b17a618ff2f47e204eb4a` | Verified source and exact LP-NFT recipient in the graduation proof. |
| Hood | fallback V2 factory `0x8bce...937f` | `0xbab145d02e7005f0d84c6c1639d39b799b0ea16df99ebbdaf5a14d9da820b4e0` | Immutable factory link; retained because `migrator == 0` changes the migration generation. |
| Hood | owner Safe proxy / singleton | `0xd7d408...fb4c` / `0xb1f926...81ff` | Owner and slot-0 singleton are pinned as a required pair. |
| Pons current | factory `0x0c37...77a4` | `0x921a0d1b2d854de5435804e8ee118658f05173a0eeebca5f41b41385b97cd1b5` | Deployment/proof receipts plus fixed-block code commitment; current source remains unverified. |
| Pons legacy | factory `0xa5aa...1feb` | `0x0a62b8ed1d88d30c7b342ea8361dfaf0ac336706992cf0c8ba38b129f06391d4` | Explorer-verified legacy factory and fixed-block code commitment; observation only. |
| Pons current | locker `0x31ca...54b5` | `0x5bfb52957c2df2cc05b894cd707811c811ee0e38b4a26ea59bae08cd65b39bbd` | Exact LP-NFT recipient and `PositionLocked` emitter in complete proof receipts. |
| Pons dependency | V3 factory `0x1f7d...2efa` | `0xec72b1abd1f2faee020cfea9c646bd8994f9fb389054f6e574f103a895091739` | Exact `PoolCreated` emitter and CREATE2 dependency in proof receipts. |
| Pons dependency | position manager `0x7399...e0d3` | `0x0a493d1af3d0f25fed8efa205244ebee14114267a08647fc38c515c7cd6ead4f` | Exact mint/LP-NFT manager in proof receipts. |
| Pons dependency | SwapRouter02 `0xcaf6...5cb2` | `0x6f36c378e272c6324c48f045182bcb54bd8ad654cf9ebd42e8893d52c4cb25dc` | Exact launch-time initial-buy sender and shared reviewed router commitment. |
| Pons dependency | WETH `0x0bd7...ad73` | `0x5706be52f64875fee65a2cec0d80e47a23d8793cbe85d214b48445e2d05f5353` | Exact pair token in configuration and all current proof receipts. |
| Bankr/Doppler | Create emitter `0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862` | `0x86b37100cbe9841771c452a592985b4e921254b127a380246073b84ec953f7f8` | Proof receipt and fixed-block code commitment; only selector `0x882db707` is admitted. |
| Bankr/Doppler | V4 PoolManager `0x8366...0951` | `0xbd3881180b547f5fe817545743cfb4343e96b1bc6640dcd70c106b0066e95626` | Canonical pool event emitter, proof receipt, and fixed-block code commitment. |
| Bankr/Doppler | `DopplerHookInitializer` `0x4e34...a544` | `0xc41a91106002f15bf70ae266824317f3f3ac638ac72ca5253bae395fa47ee631` | Verified explorer source matching official Whetstone source and fixed-block code commitment. |
| Bankr/Doppler | `RehypeDopplerHookInitializer` `0x6f02...0f77` | `0x5d33a1d867ba0d17cc7af077786b1356848c72f8e0bf960ef88aa15f7a6962d1` | Verified explorer source matching official Whetstone source and fixed-block code commitment. |
| Bankr/Doppler | `DopplerERC20V1Factory` `0x1b37...b69a` | `0x27abd63146eb5743b7871e211da17163afbb495863a626c0d002312af6813459` | Exact module in proof calldata; verified explorer source and fixed-block code commitment. |
| Bankr/Doppler | `LaunchpadGovernanceFactory` `0xdb03...37cf` | `0xefce8ac4a6fe83ae3dd1c3cfebc0e370e1595a66608bed5610ffdd1f291b7f63` | Exact module in proof calldata; verified explorer source and fixed-block code commitment. |
| Bankr/Doppler | `NoOpMigrator` `0xba2f...5a0e` | `0x7bf5115543e8e0769ceabe4da9b8e23547c9e95c1cce15d24d96f164406129e3` | Exact module in proof calldata; verified explorer source and fixed-block code commitment. |
| Bankr wrapper | EntryPoint v0.7 | `0x8db5ff695839d655407cc8490bb7a5d82337a86a6b39c3f0258aa6c3b582fc58` | Canonical EntryPoint address plus fixed-block code commitment. |
| Bankr leader | EIP-7702 designator | `0x4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4` | Exact `ef0100 || d6cedd...75b28` designator from proof account. |
| Bankr leader | delegated Kernel runtime | `0x6f6d6691dc11fda98d3102802f20e7e816ccc576c16c9279ee1a884a51d1935d` | Fixed-block delegated implementation code commitment. |
| Clanker | factory `0xd3f2...9a94` | `0xf895112a2deed34ba2765d0147aff3494104a28293cc2f19af9275934088da33` | Existing protocol research plus fixed-block code commitment. |
| Clanker | V4 PoolManager `0x8366...0951` | `0xbd3881180b547f5fe817545743cfb4343e96b1bc6640dcd70c106b0066e95626` | Canonical pool event emitter and fixed-block code commitment. |
| Clanker | `ClankerHookStaticFeeV2` `0x48b8...e8cc` | `0x0883056c4856f8fe464ff49f9c1c028455459dad8ceddcc6d5159259fe51e07f` | Official `clanker-devco/v4-contracts` source at `b004c2e` plus fixed-block code commitment. |
| Clanker | locker `0x290f...bc99` | `0x2175e20d41bc72ad6596b2fdd2c43c75e9d8ca10a706a1ca6c1a3d1526c336bc` | Launch receipt identity and fixed-block code commitment. |
| Clanker | descending-MEV module `0xea1f...299e` | `0x0815a0af5e056adaf07a1941b92082caa886b207676bd42c89ea6bde3956bc13` | Official source semantics plus fixed-block code commitment. |
| Clanker | extension `0x6f27...34b5` | `0xf742a12de7ec06481d0e98942d1830d8bf33502d854e5d97062ef5fda6f5e004` | Exact launch receipt identity and fixed-block code commitment. |

Schema version 3 serializes the complete Clanker and Hood identity profiles,
reviewed fee/configuration semantics, and all seven Pons runtime identities separately from fresh
observations. The Pons fields are reviewed expected configuration; observed
startup code remains a different document and cannot populate them. The
reviewed-production provenance requires complete Clanker, Bankr/Doppler, and
Hood profiles; deleting one rejects startup rather than silently disabling its
strict receipt path. The Hood semantic profile also serializes the historical
guard value, disable block, transition transaction index, and fail-closed
transition-block policy instead of leaving that authority only in code. The
Clanker fee values emitted for an individual pool remain receipt-local state;
the expected document pins the official contract bounds instead of copying an
observed pool's values into expected configuration: static LP fees are capped
at 100,000 ppm, the MEV override at 800,000 ppm, and decay at 120 seconds, with
the pinned one-second guard and 20% protocol share.

The Bankr standard profile was independently decoded from proof transaction
`0xc6597f...f0609` and checked against the verified Robinhood explorer sources
and official `whetstoneresearch/doppler` source at commit `568fc2f`. The proof
pins a 7,000 ppm core LP fee and a separate Rehype output-token fee schedule:
800,000 to 5,000 over ten seconds, divided by 800,000. The reviewed standard
profile also fixes 200 tick spacing, 85% pool allocation, 99%/1% curves,
95%/5% beneficiaries, and a one-second paper-quote guard. These are exact
standard-profile invariants, not broad bounds for arbitrary Doppler launches;
variants remain rejected until independently reviewed.

Pons identities remain the exact commitments recorded in `PONS_FAMILY.md` and
are now serialized in the production expected document. The current factory's
source remains unverified, so this strengthens identity drift detection without
changing Pons from paper-only to execution-ready.

The Hood profile contains ten independently reviewed identities: factory,
migrator, locker, NFPM, V3 factory, SwapRouter02, WETH, latent V2 factory,
owner Safe proxy, and Safe singleton. It also pins the live factory config,
active migrator, owner/pending-owner, migrator/locker/NFPM/router links, 1% V3
fee, full-range ticks, fee-share semantics, and canonical V3 init-code hash.
The startup snapshot independently reads and compares every runtime, the Safe
slot-0 singleton, and all callable mutable/immutable links. A factory-only pin,
zero or changed migrator, partial Safe pair, or any link/config drift rejects
startup rather than silently falling back to a different migration route.
Receipt-side configuration applies the independently reviewed guard epoch:
`guardMaxWalletBps=1000` before L2 block 5,780,966 and `0` afterward. The
transition block fails closed because the setting changes at transaction index
3 while a block-tagged `eth_call` exposes only block-terminal state.

Graduation proof `0x946fea...f645` is admitted only as migration-identity
evidence. Its ordered factory/migrator/V3/NFPM/locker topology, CREATE2 pool,
fees, full-range ticks, NFT token ID, and declared liquidity agree across the
pinned contracts. Actual pool minting consumes approximately 6.270463008 ETH
and one token, however, rather than the declared 200 million-token LP
allocation. The collector records that mismatch and keeps V3 quoting and
execution unavailable pending an independent post-block pool/position
snapshot path.

## Explicit gaps

- Schema version 3 still has no configurable fields for Flap or Permit2
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
