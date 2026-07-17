# Stonks V3 direct-launch observe-only evidence

## Verdict

`stonks_v3_direct_launch` is admitted only as a receipt-reconciled, observe-only
Doppler-family profile. It has no paper quote, sizing, exit, adapter, readiness,
canary, signer, execution, or broadcast path. The production feed selector
registry remains unchanged: exact Stonks transactions are discovered from the
launcher-owned receipt event and independently reconciled after confirmation.

The implementation parent is
`90c4824bec5dc987259ce4f81858940e247fa81c`.

## Fresh proof

- Transaction: `0xd53c3d8d8c76fd5f367d3d229a45e1aef65c0cdb712d94421f311f97fe6dd563`
- L2 block/index: `12033710 / 2`
- L2 block hash: `0x0957a6b65e883afd4d75be2bf30f0f1c31fea1b0be62bda17e46547d9a724334`
- L1 block: `25551693`
- Exact launcher/selector/value: `0x2a71...6b42 / 0x376d6552 / 0`
- Exact creator and EOA sender: `0x426e...4d9a`
- Asset/mirror/pool: `0x01c1...ac85 / 0x076e...2d79 / 0xdb22...c8288`
- Currency/band: WETH, `[-197400,-144400]`
- Initialize: tick `-197400`, sqrt price
  `0x363db22b79374d1d73fc0`
- Curve: ten dense positions ending at `-144400`, plus the exact tail
  `[-144400,887200]`; all eleven liquidity and amount words are pinned.

The exact raw Nitro record came from campaign window C line 2097. The fixture
decodes the proof transaction but produces no existing Bankr observation,
quote request, or trade plan. The Stonks evidence is created only by the
receipt ground-truth reconciler.

Fixture SHA-256 values:

- raw Nitro record: `8bfa5246a53ed9d52b02b939dfec1dd372e3fa4396f115d6daa1b535fee520e3`
- selected exact transaction/receipt/block RPC proof:
  `2b65401b38e7f61b32025a8e714f30a4a0aea22fe5a4d478a3a23872ef3d779c`
- receipt-block runtime code:
  `216c547466fb0da100d129a8b7987f5f9f757066b2f124da1a336f1d771eed38`

## Receipt-block pins

The verifier checks the exact launcher, Airlock, Bundler, DN404 factory, V3
initializer, governance factory, migrator, WETH, USDG proxy, USDG
implementation, USDG owner, and Uniswap V3 factory at the receipt block. It
also pins every launcher dependency getter, both mutable WETH and USDG bands,
launcher owner and zero pending owner, Airlock owner, USDG owner, and USDG's
EIP-1967 implementation slot.

The receipt must contain exactly one matching DN404 factory event, canonical
V3 `PoolCreated`, pool `Initialize`, eleven ordered pool `Mint` events,
initializer `Lock`, Airlock `Create`, and launcher `Launched`, in that global
order. DN404 asset/mirror linkage is bidirectional. Pool identity is bound by
the pinned factory runtime and pool init-code hash, the canonical CREATE2
derivation, the factory's `PoolCreated` event and `getPool` result, and the
pool's factory/token/fee/tick-spacing getters. Pool runtime is intentionally
not compared with a fresh global hash: deployed V3 pool runtime includes
instance-specific immutables.

## Historical set

The six prior successful direct-launch transactions independently establish
the same launcher selector and creator-equals-sender shape:

| Transaction | Currency | Result |
| --- | --- | --- |
| `0x47d4825051b72ba5a54ef4c5d5517ee08b8567a77745c93ced914d9676d3a841` | WETH | exact observe-only profile passes |
| `0x93ac8ad387afbd2d7a69425262c95db4d574d5325184722a5bb141f0b3767ab1` | USDG | rejected; USDG observation is unsupported |
| `0xe457180cee58cb038345782353f5837532ea3fc5f62d79258e6b4e69f96649f3` | WETH | exact observe-only profile passes |
| `0x1bf45b237525dfb1c8bdf41ecfceb3b57bcf445755af0f3e62a55c916fd10dd0` | USDG | rejected; USDG observation is unsupported |
| `0x25a8ad84b92a2abebb58d4b5d52f4395c35e65bae676dd6e0493f2a40d5b7968` | USDG | rejected; USDG observation is unsupported |
| `0x35eada5401f9d39f121229a67725aff937254960da1c45859f4584845dd39738` | WETH | exact observe-only profile passes |

The three WETH histories and three USDG negative controls were revalidated
against public historical receipt-block RPC. This evidence does not authorize
USDG, `launchAndBuy`, a smart-account sender, a proxy/delegated launcher,
creator mismatch, band drift, dependency drift, event drift, or position drift.

## Safety boundary

The emitted record always states `quote_status=unsupported`,
`paper_evidence_ready=false`, `authorizes_canary=false`,
`execution_eligible=false`, and `broadcast=false`. Stonks is absent from paper
capabilities and the promotion readiness set. A Stonks receipt's shared
Airlock event is suppressed from Bankr/Doppler ground-truth metrics when the
exact launcher-owned `Launched` event is present, preventing double counting.

The next admissible step is an independent quote-engine research batch. This
observation batch provides no evidence for a quote, trade plan, or canary.
