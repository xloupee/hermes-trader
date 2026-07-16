# Clanker current production evidence — 2026-07-16

This is a local, read-only Robinhood Chain review. It used the public mainnet
RPC only. No wallet, signer, transaction sender, broadcast path, or deployment
was involved.

## Current pins

At observed head `11640358`, all six Clanker runtime hashes still matched
`config/launchpad-expected-pins.production.json`:

| Role | Address | Current runtime hash |
|---|---|---|
| Factory | `0xd3f2...9a94` | `0xf895112a2deed34ba2765d0147aff3494104a28293cc2f19af9275934088da33` |
| V4 PoolManager | `0x8366...0951` | `0xbd3881180b547f5fe817545743cfb4343e96b1bc6640dcd70c106b0066e95626` |
| Static fee hook | `0x48b8...e8cc` | `0x0883056c4856f8fe464ff49f9c1c028455459dad8ceddcc6d5159259fe51e07f` |
| Locker | `0x290f...bc99` | `0x2175e20d41bc72ad6596b2fdd2c43c75e9d8ca10a706a1ca6c1a3d1526c336bc` |
| Descending-MEV module | `0xea1f...299e` | `0x0815a0af5e056adaf07a1941b92082caa886b207676bd42c89ea6bde3956bc13` |
| Extension | `0x6f27...34b5` | `0xf742a12de7ec06481d0e98942d1830d8bf33502d854e5d97062ef5fda6f5e004` |

## Recent canonical launches

A bounded `eth_getLogs` scan over L2 blocks `11630000..11640358` found two
successful exact `TokenCreated` events from the pinned factory:

| Transaction | L2 block | Profile | Static fees | Positions |
|---|---:|---|---:|---:|
| `0x9dc9e2...cc0bc` | `11632898` | extensionless | 10,000 / 10,000 ppm | 1 |
| `0x487b66...f0bff` | `11635061` | pinned extension | 16,000 / 16,000 ppm | 5 |

Both receipts use WETH, dynamic-fee flag `0x800000`, tick spacing `200`, the
pinned static hook, locker, and descending-MEV module. Both have zero
transaction value and no launch-receipt pool swap. Their MEV configuration is
`666777 -> 41673 ppm` over `15` seconds.

The second receipt reproduces the already reviewed extension-bearing profile.
The first is a distinct current production profile: `extensionsSupply = 0`, an
empty extensions array, one positive liquidity position, and a 10,000 ppm
static fee in each direction. Its pool key, Initialize event, position, fee
events, and final TokenCreated identity are mutually consistent. The checked-in
fixture `tests/fixtures/clanker-v4-extensionless-live-proof.json` contains the
exact quote-relevant logs and block/transaction identity.

## Paper-only boundary

The strict receipt quoter now admits exactly two receipt shapes:

1. no extension, zero extension supply, and one position; or
2. the pinned extension, nonzero extension supply, and five positions.

Cross-paired extension/supply values, any other extension count, changed pin,
missing liquidity, embedded launch swap, or fee outside the reviewed bounds
still fails closed. Both shapes remain `execution_eligible = false` and
`broadcast = false`; this evidence does not authorize a canary.

## Reproduction

```sh
cast block-number --rpc-url https://rpc.mainnet.chain.robinhood.com
cast codehash 0xd3f2cc1731b7fd17f28798835c2e02f0a1839a94 --rpc-url https://rpc.mainnet.chain.robinhood.com
cast tx 0x9dc9e20f714c442cb9ec6ff45450b3ce4e54c9735bb9eb90d129d3bebe4cc0bc --rpc-url https://rpc.mainnet.chain.robinhood.com --json
cast receipt 0x9dc9e20f714c442cb9ec6ff45450b3ce4e54c9735bb9eb90d129d3bebe4cc0bc --rpc-url https://rpc.mainnet.chain.robinhood.com --json
cargo test clanker_receipt_quote --lib
```
