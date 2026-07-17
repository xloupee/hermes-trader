# Bankr/Doppler curve-ticks V3 evidence

Evidence date: 2026-07-16 UTC. Network: Robinhood Chain mainnet, chain ID
4663. This is a paper-observer detection profile only; it does not enable
execution, signing, or broadcasting.

The quiet observer window contained two Bankr ground-truth launches in the raw
feed that the adapter rejected after successfully unwrapping the account call:

| Transaction | L2 block | Type | UserOperation sender | Token orientation |
| --- | ---: | --- | --- | --- |
| `0xeb5099bdb19890787a6479d63ffe22677c9009a85ee8f56f8a3400cb727964de` | 11,682,760 | `0x2` | `0x8eed177094d9f27aeb0e6656f3c14da4d4a9608f` | token below WETH |
| `0xadc88464c56530e2b1cec5d6467287a407a10184ad782fa03fee6cb3f77217e2` | 11,684,951 | `0x4` | `0xabfdb0c399fcff6e2f634e6358794b829fcbb459` | token above WETH |

Both canonical transactions have the same fail-closed envelope:

- EntryPoint v0.7 `0x0000000071727de22e5e9d8baf0edac6f37da032`,
  selector `0x765e827f`;
- one UserOperation and no `initCode` or paymaster data;
- account selector `0xe9ae5c53` (`execute(bytes32,bytes)`), all-zero mode;
- target Airlock `0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862`,
  value zero, inner selector `0x882db707`;
- account code `ef0100 || d6cedde84be40893d153be9d467cd6ad37875b28`,
  hash `4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4`;
  and
- delegated Kernel runtime hash
  `6f6d6691dc11fda98d3102802f20e7e816ccc576c16c9279ee1a884a51d1935d`.

All fixed Airlock, factory, initializer, Rehype, fee, supply, beneficiary, and
token-factory fields match the reviewed production profile. The sole new
calldata shape is the first curve lower tick:

```text
curve 0: [-229400, -119400], one position, 99 percent share
curve 1: [-119400,  887200], one position,  1 percent share
```

The successful receipts independently confirm both orientations. When the
token sorts below WETH, PoolManager `ModifyLiquidity` ranges are
`[-229400,-119400]` and `[-119400,887200]`. When it sorts above WETH, they are
`[119400,229400]` and `[-887200,119400]`. The account designator and delegated
runtime were read at each receipt block.

The checked-in fixture
`tests/fixtures/bankr-doppler-v3-quiet1-proof.json` retains the exact outer
calldata, extracted Airlock calldata, receipt block identity, account identity,
and raw `ModifyLiquidity` data for both transactions. Tests unwrap the real
EntryPoint calls, revalidate the complete create profile, decode the receipt
ranges, and reject one-tick-spacing mutations at each newly admitted boundary.

Reproduction:

```sh
cast tx <transaction> --rpc-url https://rpc.mainnet.chain.robinhood.com --json
cast receipt <transaction> --rpc-url https://rpc.mainnet.chain.robinhood.com --json
cast code <user-operation-sender> --block <receipt-block> \
  --rpc-url https://rpc.mainnet.chain.robinhood.com
cast codehash 0xd6cedde84be40893d153be9d467cd6ad37875b28 \
  --block <receipt-block> --rpc-url https://rpc.mainnet.chain.robinhood.com
```
