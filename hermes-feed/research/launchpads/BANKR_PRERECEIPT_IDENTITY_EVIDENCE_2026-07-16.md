# Bankr pre-receipt token and V4 pool identity

Evidence date: 2026-07-16 UTC. Network: Robinhood Chain mainnet, chain ID
4663. This proof only enables deterministic paper-observer identity. It does
not enable execution, signing, broadcasting, or candidate-time RPC.

## Reviewed deployment and source

The Whetstone `doppler` repository at commit
`568fc2fe42e6aaf5928fac5dd4365555f0dcad86` contains the exact chain-4663
deployment mapping:

- Airlock `0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862`;
- DopplerERC20V1Factory `0x1b37d3a72082029c44b35b604ea473617580b69a`;
- DopplerERC20V1 implementation
  `0x3be8b97fd0e713b5abe0649fa830223b6b4bc599`.

`Airlock.create` passes `createData.salt` unchanged to the token factory and
uses the Airlock as both token recipient and owner. `DopplerERC20V1Factory`
uses `LibClone.cloneDeterministic(IMPLEMENTATION, salt)`. The separately
reviewed Whetstone SDK at commit
`a31f15b9351295dc7b89d5fe0e327edb3fdaa12f` independently implements the same
Solady clone init-code and CREATE2 calculation.

Canonical RPC observations agree with that source and deployment map:

```text
factory IMPLEMENTATION() = 0x3be8b97fd0e713b5abe0649fa830223b6b4bc599
factory runtime hash       = 27abd63146eb5743b7871e211da17163afbb495863a626c0d002312af6813459
implementation runtime hash= 67a382a66d2b14a7032698e11c9ae4432435d2c803429d5c660692289ad10e12
```

The implementation address and runtime hash are expected production
authority and fresh startup-snapshot dependencies. The live getter observation
is corroboration only; it is not allowed to self-attest expected identity.

## Exact derivation

For every fully validated V1, V2, or V3 Airlock create profile:

```text
cloneInitCode =
  602c3d8160093d39f33d3d3d3d363d3d37363d73
  || implementation
  || 5af43d3d93803e602a57fd5bf3

token = last20(keccak256(
  ff || tokenFactory || createData.salt || keccak256(cloneInitCode)
))
```

Bankr launches a Uniswap V4 pool, so there is no pool contract address to
predict. The meaningful identity is the PoolManager pool ID:

```text
poolId = keccak256(abi.encode(PoolKey({
  currency0: min(WETH, token),
  currency1: max(WETH, token),
  fee: 0x800000,
  tickSpacing: 200,
  hooks: 0x4e3468951d49f2eea976ed0d6e75ffcb44a9a544
})))
```

Hermes therefore leaves generic address-valued `predicted_pool` empty and
emits a separate `predicted_pool_id`. Reconciliation compares that value with
the canonical receipt `Initialize`/strict quote pool ID, and includes missing
or mismatched pool IDs in readiness identity failures.

## Reproduction coverage

Unit proofs reproduce token and pool ID exactly from raw pre-receipt calldata
for the original V1 ERC-7579 proof, the V2 direct envelope, the V2 ERC-7579
envelope, and both quiet-window V3 ERC-7579 launches (both token/WETH
orientations). Changed salts produce different token and pool IDs. Wrong
factory, unreviewed curve boundary, or implementation-pin drift fails closed,
and forged receipt token identity remains rejected by strict reconciliation.
