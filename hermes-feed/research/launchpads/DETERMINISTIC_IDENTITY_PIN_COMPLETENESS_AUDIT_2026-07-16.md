# Deterministic identity expected-pin completeness audit — 2026-07-16

Scope: independently classify the new Bow, LaunchHood V3, Clanker, and
Bankr/Doppler identity dependencies as either (a) cryptographically committed
by an already-pinned runtime or (b) requiring a separate expected runtime pin
and fresh startup observation. This audit does not enable execution, signing,
broadcasting, or deployment.

## Snapshot baseline

The production expected document at commit `125f2c3` validates both at its
reviewed boundary and at a fresh confirmed head:

| snapshot | L2 block | block hash | emitted pins | result |
|---|---:|---|---:|---|
| reviewed boundary | 10,980,306 | `0x918363e5b20e86dbe7e952f261a60c9882975ec434abb5815a9dbecdc6354173` | 36 | pass |
| latest minus two | 11,705,894 | `0xbc44462a4861c15cf9064e5f375c247b78fffd9385489618ed26c60865825f90` | 36 | pass |

The pass means the current schema agrees with chain state. It does **not**
mean the schema contains dependencies introduced by pending deterministic
predictors. `pin_requests` can only observe addresses represented by the
current expected-profile types.

The LaunchHood follow-up closes one such gap by adding the independently
reviewed token implementation address, 6,821-byte length, and runtime hash to
expected authority, snapshot requests, startup validation, and the paper
registry. The factory runtime remains the independent authority for the
immutable implementation address; the new pin separately attests the deployed
bytes. A successful complete snapshot therefore contains one additional pin.

Independent `eth_getCode` reads at the reviewed boundary and latest returned
the same length/hash for every new address below.

## Commitment classification

| protocol dependency | independently reviewed authority | classification | expected/snapshot requirement |
|---|---|---|---|
| Bow token creation code, 7,841 bytes, `0x31f06442f2a00efc42dba795fef6459a1e88f3ba8447ceaea56b084a8d8414fa` | exact slice of the pinned Bow factory runtime; two pure-selector and receipt reproductions | **Bound by factory runtime** `0x8d56...7a14`. The checked-in copy still needs its compile-time length/hash assertion so local artifact drift fails closed. | No duplicate JSON field or runtime-address request. Retain the factory expected pin plus the compiled artifact commitment and audit tying the slice to that runtime. |
| LaunchHood `TOKEN_IMPL` address `0x5fdf73abc7a232d91b03638c2f9a52c16ab0e3be` | verified factory source declares `address public immutable TOKEN_IMPL`; getter returns the same address at reviewed/latest blocks | **Address bound by factory runtime** `0x9b785d...7766`, because Solidity substitutes the immutable into deployed runtime. | The address may remain a reviewed code constant for identity-only CREATE2, but startup must retain the exact factory pin and a relationship test/audit. |
| LaunchHood token implementation runtime, 6,821 bytes, `0xc4717d14bba5f205e8d92a9bf736e038467a353ce7053fcefa5c17da1dec6a47` | Blockscout-verified `LaunchHoodV3Token`; reviewed/latest code hashes match | **Not bound by factory runtime.** The factory commits the implementation address, not the bytes currently deployed at that address. | Add an explicit implementation address/runtime expected pin and fresh snapshot request before relying on token runtime semantics, restrictions, or execution. Identity-only paper prediction may stay enabled with `execution_ready = false` and restriction state unknown without claiming that semantic authority. |
| Clanker linked `ClankerDeployer` address `0xfb2bae281d9f9d11ae3aed87bb717b058c9797e6` | verified factory bytecode/source link and verified library source | **Address bound by factory runtime** `0xf89511...8da33`. | Do not treat the link address alone as sufficient because the factory `DELEGATECALL`s separately deployed code. |
| Clanker deployer library runtime, 17,375 bytes, `0x90b7bf626c59dbc11e746825236f79693e2f3da80b2f551f59ab7b5030e5a3c4` | Blockscout-verified `ClankerDeployer`; reviewed/latest hashes match | **Not bound by factory runtime.** | Add the address/runtime to the reviewed Clanker profile, registry dependency pins, and `hermes-launchpad-pin-snapshot` requests. A pending patch that adds only the JSON/profile pin but omits the snapshot request will make a fresh snapshot incomplete and startup fail. |
| Clanker token creation code, 16,310 bytes, `0xc3cf9289693d52fa53c127db0773c5eca16d8b29ab5c8b9aa9d3a72f39ad4815` | verified `ClankerDeployer` uses `new ClankerToken`; exact verified-token creation template and receipt reproductions | **Bound by the separately pinned deployer-library runtime**, not by the factory runtime alone. The checked-in artifact is a second local copy. | Require compile-time/local length and hash authentication plus the explicit deployer runtime pin. A second JSON code-hash field adds no independent chain observation if it only repeats the compiled constant. |
| Clanker per-launch hook address | canonical calldata and resulting V4 `PoolKey`/pool ID | **Cryptographically bound into the predicted pool ID**, but its runtime and factory enablement state are not bound by factory code. | Identity-only paper output may report the pool ID for canonical calldata. Quote/execution eligibility must require the hook address/runtime to match an explicit reviewed profile and independently snapshot mutable enablement/locker/MEV/extension relationships. The existing static-hook runtime pin covers only that static profile. |
| Bankr `DopplerERC20V1Factory` address/runtime `0x1b37...b69a` / `0x27abd6...459` | verified Airlock calldata/profile and verified factory source | **Already explicit.** | Keep the existing token-factory expected runtime and fresh snapshot request. |
| Bankr `IMPLEMENTATION` address `0x3be8b97fd0e713b5abe0649fa830223b6b4bc599` | verified token-factory source declares an immutable; getter returns the same value at reviewed/latest blocks | **Address bound by token-factory runtime.** | Bind the predictor's minimal-proxy init code to this exact reviewed address; reject profile/address drift. |
| Bankr token implementation runtime, 13,927 bytes, `0x67a382a66d2b14a7032698e11c9ae4432435d2c803429d5c660692289ad10e12` | Blockscout-verified `DopplerERC20V1`; reviewed/latest hashes match | **Not bound by token-factory runtime.** | Add the implementation address/runtime to the reviewed Bankr profile, registry dependency pins, and snapshot requests. The pending Bankr predictor does all three and should remain atomic during integration. |
| Bankr Solady clone init-code prefix/suffix | verified factory source calls `LibClone.cloneDeterministic`; standard fixed minimal-proxy construction | **Code template plus the factory-bound implementation address determine the init hash.** | Keep the exact prefix/suffix in reviewed code with proof tests. No independent contract address exists for the template, so a snapshot field would not add chain authority. |

## Required integration order

1. Integrate the Clanker and Bankr predictor batches without dropping their
   profile fields or negative tests.
2. Add the missing Clanker deployer-library request to
   `hermes-launchpad-pin-snapshot`; require exactly one observed pin and add a
   completeness test alongside the Bankr dependency test.
3. Preserve the Bankr token-implementation request and completeness test from
   its pending batch.
4. Preserve the explicit LaunchHood implementation address/length/runtime pin
   and its separate factory-immutable relationship check. This closes startup
   identity completeness but does not make execution ready.
5. Do not admit an arbitrary Clanker calldata hook into a quote-ready or
   executable profile merely because its pool ID is deterministic. Runtime and
   mutable factory-enablement evidence are separate requirements.
6. Re-run reviewed-boundary and latest-confirmed snapshot validation. The
   expected pin count must increase for each newly explicit runtime address;
   observed data must never be copied into expected authority without the
   verified-source/bytecode review above.

## Primary authority

- [LaunchHood factory verified source](https://robinhoodchain.blockscout.com/api?module=contract&action=getsourcecode&address=0x62b33a039d289cbda50ebeb72fe4261449e61bcf)
- [LaunchHood token implementation verified source](https://robinhoodchain.blockscout.com/api?module=contract&action=getsourcecode&address=0x5fdf73abc7a232d91b03638c2f9a52c16ab0e3be)
- [Clanker factory verified source](https://robinhoodchain.blockscout.com/api?module=contract&action=getsourcecode&address=0xd3f2cc1731b7fd17f28798835c2e02f0a1839a94)
- [Clanker deployer library verified source](https://robinhoodchain.blockscout.com/api?module=contract&action=getsourcecode&address=0xfb2bae281d9f9d11ae3aed87bb717b058c9797e6)
- [Bankr token factory verified source](https://robinhoodchain.blockscout.com/api?module=contract&action=getsourcecode&address=0x1b37d3a72082029c44b35b604ea473617580b69a)
- [Bankr token implementation verified source](https://robinhoodchain.blockscout.com/api?module=contract&action=getsourcecode&address=0x3be8b97fd0e713b5abe0649fa830223b6b4bc599)
