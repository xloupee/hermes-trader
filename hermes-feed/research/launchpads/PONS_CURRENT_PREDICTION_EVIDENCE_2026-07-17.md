# Pons current-generation receipt-free identity evidence

Review date: 2026-07-17 UTC. Implementation parent:
`38fc684a67d648f7118ed3bdcf038422d866496a`. Network: Robinhood Chain
mainnet, chain ID `4663`.

## Bounded conclusion

Current-generation Pons launches can predict the token and canonical Uniswap
V3 pool before a receipt without candidate-time RPC. This conclusion is
restricted to canonical `launchToken` calldata sent directly to the pinned
current factory, or to the already strict EIP-7702 self-batch decoder whose
inner call is that same canonical launch. Legacy Pons remains unresolved and
discovery-only.

This evidence grants no signing, broadcast, execution, canary, deployment,
wallet, server, or Droplet authority.

## Independent expected semantics

The production expected document retains its distinct
`expected_protocol_pins` / `reviewed_protocol_pins` role. The new prediction
fields were derived from the original fixed review boundary and historical
transaction traces, not from the later fresh startup snapshot.

At L2 `10980306`, block
`0x918363e5b20e86dbe7e952f261a60c9882975ec434abb5815a9dbecdc6354173`,
the independently queried current factory had:

- direct nonproxy runtime: 24,192 bytes, Keccak-256
  `0x921a0d1b2d854de5435804e8ee118658f05173a0eeebca5f41b41385b97cd1b5`;
- launch enabled: `true`;
- launch fee: `500000000000000` wei;
- locker: `0x31ca5E101941A93A7DD6d0497928700625CF54B5`;
- config 0: WETH, 4.2 ETH initial liquidity, tick `-204200`, supply
  `1000000000000000000000000000`, max-wallet 200 bps, max-tx 220 bps,
  366 restriction blocks, flags `[false,true,false]`;
- DEX config 0: `uniswap v3`, factory `0x1f7d...2efa`, position manager
  `0x7399...e0d3`, SwapRouter02 `0xcaf6...cb2`, fee 10,000, tick spacing
  200, enabled;
- pure predictor selector `0xea9d3fdc`.

The token creation prefix is the exact 9,453-byte CREATE2 init-code prefix at
factory-runtime offset 14,686. Its raw-byte Keccak-256 is
`0x86588bc75e5a00a2e28ba6f44fb4c15c899dcd9a0622b28d116d8ca5f8635804`.
The canonical Uniswap V3 pool init-code hash is
`0xe34f199b19b2b4f47f68442619d555527d244f78a3297ea89325f843f87b8b54`.

## Four independent transaction / trace / receipt proofs

| Transaction | L2 block/index | Init-code hash | Token | Pool |
| --- | --- | --- | --- | --- |
| `0x603805cc5b1ffc04f410ba9481764c2d2eb7e1f568f66bac9b15c6949f78578b` | `12031758/3` | `0x67081e21036d9e62b220b258fe3da54736781d8cbcfd112441ddfdb72bcbb131` | `0x2911813196a33af90d6a764d1a3418f40d5372bd` | `0x1b4d60cc176c5f77d8176ada2c61bb2d77613ef9` |
| `0xcd92a3cce53c0e1d24a80f829799bd912feda584175c70da8d0c908ffa564d47` | `12032202/2` | `0xa349c93116fcc1ef94ec224dd32c593e5d1dded453a8dce0ab66b727a4792d90` | `0xfe931bf067933b6b4930d0806a2f33c30dd937aa` | `0xcb4bf6d00aa91aabb6a6ec3ae174993faa6ac626` |
| `0x6893ff469f43ea30c9b5ced4b5c4ad45d8a9135bc8b2c737a6d88253cdc04e2c` | `12033041/3` | `0x649ddd02a68c3cde7eca06965b334b2018765303570ece181e8b981a9d7c9e1f` | `0x869ee80857560e5251d18bb5a302b5735898a646` | `0x5274bf5077a581a8fa92871abd7038b2092e195e` |
| `0xf40cc1543052e78c2c2fbb2dd6561dd14905ffc3bae0266e397d7c3ce6a36583` | `11997838/1` | `0x86646c4208b17897af416c0f51ada2250def7d1fe8a49442c1add374b78b54a2` | `0x0d433d684ebf899682a7a3301a3770656711e568` | `0xcfcf579c77111ce46bbccf22efaae5df158af5a9` |

For every proof:

1. canonical calldata decodes and re-encodes exactly;
2. the locally encoded constructor appended to the pinned prefix equals the
   raw CREATE2 trace input byte-for-byte;
3. standard CREATE2 using the calldata salt reproduces the receipt token;
4. the factory's `0xea9d3fdc` pure view at the immediately preceding block
   returns the same token;
5. canonical V3 CREATE2 using sorted token/WETH, fee 10,000, and the pinned
   pool init hash reproduces the receipt pool; and
6. production receipt reconciliation and warm-state paper quote reconstruction
   accept the same token and pool.

The token constructor receives name, symbol, logo, description, and the five
social strings positionally. It receives the recovered outer signer as token
deployer. The calldata developer wallet and the factory fee wallet are not
constructor inputs. Reordering the raw social positions, or changing salt,
signer, or identity-bearing metadata, changes the prediction. Developer/fee
wallet changes do not.

## Expected versus observed startup boundary

The snapshot collector now reads the prefix and five factory getters at one
pinned block and serializes them only into `pons_v3_semantics` in the observed
startup document. Startup requires exact equality with the separately reviewed
expected semantics and rejects missing, partial, or drifted pairs.

Historical validation succeeded with 39 pins at the fixed review boundary and
the separate two-pin Pons EIP-7702 proof boundary. It made 84 logical RPC
requests with zero retries, rate limits, server errors, or transport errors.

A later fresh comparison also succeeded at L2 `12127362`, block
`0x9c8b4677e93af3f853ca19ca61bd3afce7069cc4a4b1e28d25a88efb773ac19e`,
L1 `25552476`, timestamp `1784292123`: 41 pins, 83 logical requests, zero
errors. The fresh values were compared against the pre-existing expected
profile; none were copied into it.

## Artifact identity

The production expected-pin Keccak recorded below is preserved as historical
provenance but was computed over the ASCII hex representation, not the file
bytes. The immutable supplemental correction is
`PONS_CURRENT_PREDICTION_EXPECTED_PIN_DIGEST_CORRECTION_2026-07-17.json`.
It records the authoritative file-byte Keccak without changing any expected
pin content or adopting any observed value as expected authority.

| Artifact | SHA-256 | Keccak-256 |
| --- | --- | --- |
| Expanded production expected pins | `f1fa4f3080fc3d3c193a5de4424410bc747da784d465f118b6a67d2e74a93095` | `0xe877d69efb9f62650c1acce4b4d2392b35285146e8231f113a1feb80677613fb` |
| Four-proof fixture | `214bff4eef3d5c28d05f5e7c0c5faeca8936fb6320b274b2b548ba2b95094c1f` | `0xdc18398029ca6ec6e463e6aded72e7020c6f8addb34345e5d449f3921a352697` |
| Historical validation report | `54523fbe02fbfc3046a5923e4bac6de3228d9a14039df1f899a6a75e3c4fee41` | `0x1ead90d1d5e1a9716b60c50447440a0c58813b6dd3ce7feae5de9f04f876cdc4` |
| Fresh startup snapshot | `ea3b8797b451f3593be95aea3a9a3793bbd4450eee5fd92d6f57ea0a13d3f39a` | `0xa6846040ad5bb9c217c19c756f6e6d8c6710d2f4561480047dc0a05994902dc7` |
| Fresh validation report | `9589560d19d01881ff532057612ec3c8c4d41d6809c9d6d94455f1ef5eb336b5` | `0x7d4e3491cc1d9352f7a389fbce1bbdcd32f415cafac8cb90eedc93130ffb86d4` |

The older `PRODUCTION_PIN_INDEPENDENT_ATTESTATION_2026-07-17_74C1062`
remains an immutable historical attestation of the earlier byte identity. It
is not rewritten to pretend that the expanded expected document is unchanged.
This document is the supplemental review surface for the Pons semantic
extension; the implementation diff passed independent P0-P3 review.
