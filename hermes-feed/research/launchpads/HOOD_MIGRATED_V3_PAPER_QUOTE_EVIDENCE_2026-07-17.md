# Hood migrated V3 paper-quote evidence (2026-07-17)

Scope: Robinhood Chain `4663`, read-only historical RPC and local deterministic replay. No key,
signer, transaction construction, broadcast, or deployment was used.

## Exact graduation proof

- Transaction: `0x946fea5c130104cf0743512ebf879036725c2396761c04398037cf875917f645`
- L2 block: `11426764`
- Token: `0x86cd468583e361794b62d5aa4c79b2b4cac2600d`
- Pool: `0xb0f85a8494bad99dad3c7a1d4ccf0be8f108fd42`
- Canonical V3 factory: `0x1f7d7550b1b028f7571e69a784071f0205fd2efa`
- Pair: WETH token0 / Hood token1, fee `10000`, tick spacing `200`
- Position: NFT `171798`, ticks `[-887200, 887200]`, liquidity `7914437`
- Actual Mint amounts: `6270463008449067013` wei WETH and `1` token unit
- Declared migration amounts: `6270463768115942030` wei WETH and
  `200000000000000000000000000` token units
- Receipt-end state: tick `887271`, sqrt price
  `1461446703485210103287273052203988822378723970341`, active liquidity `0`

The declared and actual liquidity mismatch is retained as evidence. Quotes use the decoded Mint
liquidity and the receipt-end pool state; they never substitute declared migration amounts.

Zero active liquidity at receipt end is not by itself a buy blocker here. WETH is token0, so an
exact-input WETH buy moves price down, crosses the initialized upper tick, and activates the
full-range position before consuming the fixed input.

## Independent deterministic quote

The bounded paper policy is fixed to `0.001 WETH`, hard-capped at `0.01 WETH`, with `100 bps`
slippage. The full token output of the entry is then quoted as an exit against the post-entry state.

- Entry output: `145465512933016462542115172`
- Entry minimum: `144010857803686297916694020`
- Entry initialized ticks crossed: `1`
- Full-position exit output: `989999999999957` wei WETH
- Exit minimum: `980099999999957` wei WETH
- Immediate simulated round trip: `9899 bps`

A historical canonical QuoterV2 call at block `11426764` independently returned the same entry
output, post-entry sqrt price `633380101863564253225`, and one initialized tick crossing. Quoter
runtime is not a production dependency: local replay is authoritative and fail-closed.

## Pool identity and runtime policy

Pool identity is established by all of the following:

1. pinned canonical V3 factory identity and init-code hash;
2. CREATE2 prediction from sorted token pair and fee;
3. the token/pair/fee-scoped `PoolCreated` event;
4. exact pool getters and factory `getPool` when a fixed-block RPC snapshot is collected.

There is deliberately no universal V3 pool runtime hash pin. V3 pool deployed runtime includes
pool-specific immutables. Two observed 22,142-byte pools had distinct runtime hashes:
`0xcb384b...d09d` and `0x28f6f0...94c8`. Treating either as universal would reject valid pools or,
worse, promote an observed instance hash as a protocol invariant.

## Batch topology

The chain scan found 19 `V3Migrated` logs across 14 transactions. Transaction
`0xb44053ba23f3ad57ad40f7aff0eb035cdee00837fa8df9596561672770687504` contains five
independent migration scopes and transaction
`0x6f2eda661b50e42e14b9c9303d29675f82ab6276208c89c3151bc45f89922ced` contains two.
Receipt verification therefore selects factory/migrator events by indexed token, `PoolCreated` by
the exact sorted token pair and fee, pool events by the CREATE2-derived pool, and position-manager
and locker events by token ID. A second migration in the same receipt is permitted; a duplicate for
the same token/pool/token ID fails closed.

An explicit public-RPC run through the concrete reconciler emitted five and two distinct scoped
records respectively. Three of the five first-batch scopes and both second-batch scopes passed the
strict topology and produced quotes; the other two first-batch scopes remained blocked with
`hood_migration_strict_receipt` rather than being silently dropped or weakening the verifier.

## Promotion posture

Output records remain `execution_eligible=false` and `broadcast=false`. Finalized paper plans add
only take-profit `2000 bps`, stop-loss `1000 bps`, and max-hold `300 seconds`. Evidence collection
must measure latency, false positives, and misses before any one-wallet, one-trigger, tiny-amount,
auto-stop canary is considered.
