# Bankr CurveTicksV4 reverse-orientation evidence (2026-07-17)

## Scope and conclusion

This is a bounded paper-only extension of the existing `CurveTicksV4` Bankr
profile. It admits the opposite canonical currency orientation only when every
existing create-calldata, EntryPoint v0.7, ERC-7579, EIP-7702 delegation,
receipt-block runtime, event-order, fee, beneficiary, and paper-policy check
continues to pass.

It does not add a selector, account ABI, target, value, factory, curve version,
or execution path. `CurveTicksV5` with `token < WETH` remains rejected. V1-V3,
forward V4, and forward V5 retain their existing behavior.

## Independent proofs

| Transaction | L2 / index | Leader | Token | Pool ID |
| --- | ---: | --- | --- | --- |
| `0xccf350bfed931d136f9fbf5bc20fe49eb1404a5912aae2684549e32be68e4567` | `12107164 / 5` | `0x9b60922370fe0f6bd079e0302612bfb9a3045c3d` | `0x0106e926a9ccaedce5f87c859beaf89a56e96ba3` | `0x1d40b4a7fb7768a884ea86f285e00cb9fd5ca3d282680984875888c6e5b81720` |
| `0x88eaff5b44775c8ccda07bae4ca7548cf448093013d6c7326fac3c0c8d4b2c49` | `12109627 / 10` | `0x289a2b7758257a666c7561bb6b680f26ef4be208` | `0x022df6568187016fde9651cb8b5bc4aedcf80ba3` | `0xa3a17284bba4c29e85ace5fe502177d3c8db97a45b5069e2b1ba467418047832` |

Both full transaction/receipt proofs and both raw Nitro frames independently
agree on the following facts:

- one successful EntryPoint v0.7 operation to
  `0x0000000071727de22e5e9d8baf0edac6f37da032`, selector `0x765e827f`;
- account selector `0xe9ae5c53` (`execute(bytes32,bytes)`), all-zero mode,
  target `0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862`, value zero, inner
  selector `0x882db707`;
- empty factory bytes and a complete per-account EIP-7702 delegation pair;
- 23-byte designator `0xef0100d6cedde84be40893d153be9d467cd6ad37875b28`,
  hash `0x4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4`;
- delegated Kernel `0xd6cedde84be40893d153be9d467cd6ad37875b28`,
  runtime length 24,469 and hash
  `0x6f6d6691dc11fda98d3102802f20e7e816ccc576c16c9279ee1a884a51d1935d`;
- exact CurveTicksV4 calldata: `[-229400,-119200]` at 99% and
  `[-119200,887200]` at 1%, fee 7000, spacing 200, far tick 887000;
- both predicted tokens are numerically below WETH, so the canonical PoolKey is
  `currency0=token`, `currency1=WETH`;
- initialize tick `-229400`, square-root price
  `0xaf3b2ac279070c26b9f3`;
- ordered liquidity positions
  `[-229400,-119200]`, liquidity `0xbadf8a38e438d69a45c2`, salt zero,
  followed by `[-119200,887200]`, liquidity
  `0x1d082240a370451eb5ea2`, salt one.

The two exact liquidity values are equal across both independent proofs. The
new reverse profile therefore pins them. Existing orientations retain their
existing nonzero-liquidity rule.

## Quote direction and rounding

The existing concentrated-liquidity engine derives direction from canonical
currencies. For this profile, a WETH entry is `currency1 -> currency0`; the
full-position exit is `currency0 -> currency1`. Core LP fees, output-denominated
Rehype fees, owner/buyback routing, slippage minima, and integer division keep
the existing floor-rounding rules.

The fixed independent policy is:

- entry `0.001 WETH`;
- hard cap `0.01 WETH`;
- slippage `100 bps`;
- full output-token exit.

For the first proof the entry produces net
`0xbf87ba3afae63891153a` token units with minimum
`0xbd9d690211fd84cd1274`. The simulated full exit produces
`0x8db056fedba` wei with minimum `0x8c459dce3ab`. These are paper
calculations only; the round trip is deliberately poor under the launch fee
schedule and does not authorize execution.

## Runtime and provenance bindings

All eleven dependency runtimes were independently fetched with read-only
`eth_getCode` at exact tags `0xb8bd9c` and `0xb8c73b`; every hash matched the
existing production profile. No expected pin was copied from fresh state.

- full normalized transaction/receipt proofs SHA-256:
  `75f387c720a2533f56dcd3bc997411c48ed309a4277a91febff0219be32a40d5`;
- two exact raw Nitro frames SHA-256:
  `eb7ca9c7e8598779b9872376849c4054a5b27ba1bc3cd689aec88687743425ba`;
- first fixed paper quote SHA-256:
  `427a9f74652241b52317f66e50d757ca337a7762a444213b8c5337b1f46aaa8f`;
- second fixed paper quote SHA-256:
  `df30de0e2c684979fc6c6082b1184a002c9ec3eb5eb6551c6e39af75bcc09d9d`;
- exact-block concrete reconciler runtime fixture SHA-256:
  `9804b5a9d95afe09e00368fd6272c93c6e060166448ee3e4b277cee501d125dc`.

The source raw-frame payload-line hashes are
`168b5c615be31bd2ab3224cfa385c677a413e275d6496acbec20a34e8abb4965`
and `c2fdbc19c231eea8cd1d9d83d0006ffc0cdc62a01585d6be8125ede3aa0d087e`.

## Fail-closed tests

Tests reject drift in every calldata tick boundary, curve/range order, both
shares, position count, receipt currency orientation, initialize tick/sqrt,
event order, both range words, both exact liquidity values, zero liquidity,
both salts, PoolManager/runtime pins, account designator, delegated Kernel, and
paper-quote arithmetic. Both raw frames cross the observer and emit one strict
asynchronous reconciliation request. Both then cross a concrete loopback Noxa
JSON-RPC transcript and match their fixed quote byte-for-byte. Finalization
independently rederives direction, state, fees, rounding, ranges, liquidity,
and pins before emitting an execution-gated plan.

## Safety boundary and remaining unknowns

Candidate-time observation performs no RPC. Receipt reconciliation performs
read-only exact-block RPC. Every quote and finalized plan keeps
`execution_eligible=false` and `broadcast=false`. No key, keystore, signer,
wallet, transaction broadcast, canary, server access, deployment, or remote
host was used.

This evidence does not establish live execution safety, Permit2/router
construction, signing, broadcasting, or canary readiness. Those remain out of
scope and disabled.
