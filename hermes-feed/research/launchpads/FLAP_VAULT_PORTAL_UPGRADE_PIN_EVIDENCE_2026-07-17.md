# Flap VaultPortal upgrade pin evidence

Evidence date: 2026-07-17 UTC. Network: Robinhood Chain mainnet, chain ID
`4663`. Candidate base: `d460da144b118be6e4ad41d5196682e9ffc83563`.
The machine-readable companion is
[`FLAP_VAULT_PORTAL_UPGRADE_PIN_EVIDENCE_2026-07-17.json`](./FLAP_VAULT_PORTAL_UPGRADE_PIN_EVIDENCE_2026-07-17.json).

## Disposition

The observed VaultPortal implementation changed from
`0x2813CD0b6089f76F3407792f79276E5d4f80935A` to
`0xe5789d9d5616dd8ec66de95bb31a29ac1c847769`. The current implementation
runtime is 20,897 bytes with Keccak-256
`0x8b4bcf2d4a81f646f500da41a331b01bed39065046a5058a333fb942c81c0464`.
This is sufficient identity evidence for a separately reviewed startup-pin
update. It is not a Vault route admission. Vault selector detection, candidate
normalization, quotes, readiness, execution, and canary authorization remain
disabled.

The strict old pin correctly caused a P1 availability/evidence failure before
feed acquisition. There was no P0 execution exposure because the runtime stayed
fail-closed and paper-only.

## RPC proof

The successful upgrade transaction
`0xe79d505019b265df2b9d8ddabf53720b1730ef852e09c88ae48b52da15091fbd`
landed at block `12,297,565`, hash
`0xe7831248a9f82c2b28f52b495fcb7496a1176fbf3ea3da3f0668ca061efb0da4`,
at `2026-07-17T17:26:28Z`. Log index 18 is the canonical
`Upgraded(address)` topic and names the new implementation.

`eth_getProof` at block `12,297,564` proves the EIP-1967 implementation slot
held the old implementation and the admin slot held
`0x21f7f9b33dfd0dbc3a94c0efa79f1546a1391ff5`. A proof at the upgrade block
shows the new implementation and the same admin. The proxy runtime itself did
not change: 2,840 bytes, Keccak-256
`0xe7109718479fd7c6d05b829ffc6a1469e4c949ae282497c15d179b2af4e5e3a9`.

The new implementation was deployed successfully by
`0x8187f13ed6c7c9554afe4dd4c4d4960174846063` in transaction
`0x8ce1948f451d5215a35c555b82aab70921d4bd08e67ccb8ee03a2dc68b2af669`,
block `11,969,555`, hash
`0xc53a8bd61c870f927423d1014fa0305bbcb8acfd92788e65bf29b0f87164c66d`,
at `2026-07-17T08:17:40Z`. Direct calls through the proxy and against the
implementation both return `version() = "1.12.1"` and
`PORTAL() = 0x26605f322f7fF986f381bB9A6e3f5DAb0bEaEb09`. The superseded
implementation returns version `1.10.0` and remains deployed with its original
18,792-byte runtime and hash
`0x4f096b230a8db270585d54fdd549982efda99462daad9c4b3e771a62e7071f56`.

The ProxyAdmin owner is the 2-of-3 Safe
`0xa4A727E0918cf9B39639Fc4cB7D742d39C5352a4`. Calldata proves the upgrade
path from the outer caller through MultiSend, the Safe, and ProxyAdmin to the
VaultPortal proxy. The JSON companion records each address and owner.

The new slot and runtime remained stable at independently sampled finalized and
head anchors. Primary anchors were finalized block `12,316,515`, hash
`0x2228c550ca6a90a1bb136851e0cac169337697c062ad89c6e3bd15b7cc1afae0`,
and head block `12,326,468`, hash
`0xb305ac03a6682cda11ba7907a9cce57eabe2a7f19071ce9e8d2ff9389822840c`.

## Explorer verification, kept separate

Blockscout's public v2 smart-contract response for the new implementation
reported `VaultPortal`, fully verified source, unchanged bytecode, Solidity
`v0.8.26+commit.8a97fa7a`, optimization enabled, and an ABI containing
`version`, `newTokenV6WithVault`, and `newTokenV7WithVault`. This is public
explorer evidence, not an RPC state proof.

The raw 442,773-byte response fetched from
`https://robinhoodchain.blockscout.com/api/v2/smart-contracts/0xe5789d9d5616dd8ec66de95bb31a29ac1c847769`
had SHA-256
`155b182c938231ae67187e282a64d4f6f742a2fa49ca76d21c73023bbcc7d16c`
and Keccak-256
`0x512fcde839c85d0f64b51ecc1c68fc21fbec89e2acc2ff75bea163daf9778208`.

## Independent event-topology check

A secondary reviewer bounded the first Blockscout page to all 15 successful
Vault selector `0x1b806220` transactions across blocks `12,298,899` through
`12,318,345` and fetched each receipt independently. All 15 receipts contained
exactly one `TokenCreated` and one `TokenBought`, both emitted by canonical
Portal `0x26605f322f7fF986f381bB9A6e3f5DAb0bEaEb09`; zero `TokenCreated` logs
were emitted by VaultPortal. This supports the existing Portal-emitter ground
truth only. It does not prove every Vault semantic. The secondary review did
not provide a raw transcript hash, so none is claimed here.

## Failed campaign boundary

The three attempted startup snapshots published zero snapshots and acquired
zero feed, observer, reconciliation, manifest, readiness, or report evidence.
The snapshot tool exited before publishing its report and each partial report
is zero bytes. Therefore the exact attempt block/hash/time is unknown. Block
`12,321,100` at `2026-07-17T18:05:45Z` is near the files' modification time
and already had the new implementation, but it is explicitly an approximation,
not the failed snapshot boundary.

No observed value was promoted directly to expected state during research.
The candidate constants are admitted only through this separate code and test
review. Flap remains unconditionally discovery-only for readiness.
