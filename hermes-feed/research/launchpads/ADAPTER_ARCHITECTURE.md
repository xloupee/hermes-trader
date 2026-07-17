# Static Multi-Launchpad Adapter Architecture

## Decision

Extend the existing Hermes runtime with **one static launchpad registry and a small set of protocol-family adapters**. Do not create one bot, process, or runtime per launchpad. Existing feed continuity, boundary gating, nonce/risk accounting, signing, sequencing/submission, audit, and position engines remain shared.

The current Noxa path becomes the first registry entry and a regression baseline. Its present assumptions in `hermes-noxa-runtime.rs` and `copy_policy.rs`—one launch factory, SwapRouter02 `exactInputSingle` or one aggregator, zero call value, WETH quote, and `NOXA_POOL_FEE`—must move behind the Noxa/V3 adapter boundary without changing behavior.

This document is architecture only. It authorizes no wallet/key access, live signing, broadcasts, service changes, deployments, or candidate-time RPC.

## Network and identity boundary

The registry is constructed for **Robinhood Chain mainnet, chain ID 4663**. Base is **8453**. A Base or Solana deployment, label, address, event, volume counter, or source tree cannot populate the Robinhood registry without independent chain-4663 runtime-code and transaction/log proof.

At startup, RPC may resolve proxies and pin configuration and runtime code hashes. Once the feed hot path begins:

- an unknown chain, destination, selector, implementation, factory, hook, or code hash fails closed;
- a lookalike clone does not inherit an adapter merely because its ABI or selector matches;
- receipts enrich and reconcile registry state asynchronously, but never gate the initial candidate decision;
- there is no RPC, API, database, filesystem, JSON parsing, logging, or control-plane lookup between candidate arrival and the decision/build/send path.

## Static registry

Use static data and enums rather than dynamic plugin loading or launchpad-specific runtimes. A representative shape is:

```rust
struct LaunchpadSpec {
    id: LaunchpadId,
    chain_id: u64,                 // exactly 4663
    family: AdapterKind,
    observation_keys: &'static [DispatchKey],
    contract_pins: &'static [ContractPin],
    allowed_routes: &'static [RouteKind],
    quote_assets: &'static [Address],
    prediction: PredictionKind,
    execution: ExecutionMode,
}

struct DispatchKey {
    destination: Address,
    selector: [u8; 4],
    wrapper: WrapperKind,
}

struct ContractPin {
    role: ContractRole,
    address: Address,
    implementation: Option<Address>,
    runtime_code_hash: B256,
}

enum AdapterKind {
    V3LaunchAtBirth,
    NativeCurve,
    UniswapV4,
    DopplerV4,
    FlapPortal,
}

enum RouteKind {
    V3SingleHop,
    NativeBondingCurve,
    V4HookedPool,
    DopplerPermit2,
}
```

`LaunchpadId` denotes an execution protocol, not a marketing/referrer label. Attribution is separate metadata:

```rust
struct Attribution {
    source: AttributionSource, // e.g. Virtuals
    protocol: LaunchpadId,     // e.g. BankrDoppler
}
```

Thus Virtuals-tagged Bankr launches dispatch, quote, execute, and reconcile through the Bankr/Doppler adapter. They do not create a Virtuals adapter. The same rule applies to any future front end or creator label that shares an already-registered factory and route.

## Hot-path flow

```text
feed bytes
  -> shared continuity and boundary gate
  -> cheap destination + selector lookup
  -> bounded wrapper unwrap (only allowlisted multicall / ERC-4337 forms)
  -> ambiguity and pinned-identity gate
  -> scheduled signer / smart-account identity recovery
  -> adapter observation decode
  -> shared copy and risk policy over normalized observation
  -> adapter-owned quote and follower plan
  -> shared nonce ledger, signer, sequencer, and submission
  -> async receipt reconciliation, positions, audit, and telemetry
```

Dispatch occurs before expensive signer recovery and full ABI decoding. The lookup returns zero or one adapter. Multiple matches, excessive nesting, unknown inner targets, malformed offsets, and selectors valid for more than one active pin all fail closed.

Wrapper support is deliberately bounded:

- direct calls are the default;
- only explicitly pinned multicall variants are unwrapped;
- ERC-4337 decoding accepts only pinned EntryPoint/account/factory shapes and a fixed maximum depth and call count;
- the leader identity is recovered from the validated smart-account operation semantics, not assumed to be the outer transaction sender, bundler, paymaster, or factory.

## Observation is not execution

Never mutate a decoded leader call into the follower transaction. Normalize the leader action first, then ask the chosen adapter to produce a new follower plan.

```rust
struct ObservedLeaderAction {
    launchpad: LaunchpadId,
    attribution: Option<AttributionSource>,
    leader: LeaderIdentity,
    action: ActionKind,             // launch, buy, sell
    market: MarketIdentity,
    asset_in: Asset,
    asset_out: Asset,
    observed_amounts: ObservedAmounts,
    observed_route: ObservedRoute,
}

struct FollowerTradePlan {
    launchpad: LaunchpadId,
    route: RouteKind,
    destination: Address,
    value: U256,
    calldata: Bytes,
    spend_limit: U256,
    min_receive: U256,
    expected_market: MarketIdentity,
    reconciliation: ReconciliationProof,
}
```

The leader may trade a curve through an aggregator or smart account while the follower uses a direct curve call; it may trade a Doppler/v4 market while a later policy selects another allowlisted exit route. Consequently, **the leader's minimum output, deadline, value, route bytes, permit, and hook data must never be reused across a different venue or route**. The adapter constructs a fresh quote and calldata from warm state and the follower's own risk/slippage policy.

## Adapter responsibilities

Each adapter family is responsible for:

1. matching and strictly decoding its allowlisted observation forms;
2. resolving direct, aggregator, multicall, and smart-account leader semantics that its pins explicitly support;
3. proving market identity against startup-pinned contracts, implementations, configuration, factories, pool managers, hooks, and code hashes;
4. predicting launch token or pool identity without a receipt when the protocol supports it;
5. purely constructing entry/exit quotes and calldata from warm immutable/configured state;
6. producing a receipt/log/state reconciliation proof for asynchronous processing.

The shared core retains feed and boundary handling, watch filters, scheduling, signer recovery primitives, copy-policy controls, risk and nonce ledgers, signing, submission, audit, positions, and stage timing. Adapters cannot perform network or control-plane I/O on a candidate.

## Protocol-family coverage

| Family | Initial registry entries | Shared implementation | Candidate-specific gates |
|---|---|---|---|
| V3 launch at birth | Noxa, bow.fun, LaunchHood V3; Pons observer | Factory/pool identity, CREATE2 where proven, V3 single-hop quote/calldata, V3 reconciliation | Exact factory/code/fee/quote pins; Pons stays paper-only until source and semantics are complete |
| Native curve / curve-to-pool | hood.fun, leavehood.com; trench discovery | Direct curve observation, local curve quote, graduation boundary model | No execution until direction, fees/taxes, sells, reserves, upgrade state, and graduation route are pinned |
| Flap portal | Flap observer | Portal CREATE2 and event observation; one simple allowlisted profile may get a local curve oracle | Proxy/module hashes, quote token, tax/extensions/vault, direct/aggregator routes, migration proof |
| Generic Uniswap v4 | Clanker, then klik.finance | PoolKey, PoolManager, hook identity, dynamic-fee bounds, v4 swap/reconciliation | Per-factory extension/hook allowlist; Klik execution waits for semantic validation |
| Doppler/v4/Permit2 | Bankr/Doppler | Generic v4 core plus Doppler market identity, Permit2 plan, bounded ERC-4337 observation | EntryPoint/account/factory identity and leader semantics; Virtuals remains attribution only |

Do not force native curves, Flap portal routes, Doppler, or hooked v4 pools through `exactInputSingle`. Route type is an adapter-owned fact.

## Startup and runtime state

Startup may perform a single fail-closed pinning phase before accepting candidates:

| Startup-pinned state | Candidate-time use |
|---|---|
| chain ID, genesis/network identity | equality check |
| proxy implementation slots and runtime code hashes | array/map lookup |
| factories, routers, EntryPoints, pool managers, hooks, Permit2, quote assets | dispatch and identity lookup |
| fees, curve parameters, token/pool salts, supported wrapper shapes | pure prediction and quote |
| watched leaders and smart-account mappings | precomputed identity lookup |
| risk, nonce, block/boundary, fee, routing, and signer state | shared in-memory engines |

Any startup read failure or drift disables the affected entry. Configuration refresh builds a new validated snapshot off-path and swaps it atomically; candidates never wait for refresh or fall back to RPC.

## Fail-closed test matrix

The architecture is not ready for paper integration until it has fixtures for:

- destination/selector collision, ambiguous registry entries, malformed ABI, and maximum wrapper depth/call count;
- clone and lookalike contracts with matching selectors but wrong addresses or runtime-code hashes;
- chain ID 4663 acceptance and explicit Base 8453, Solana, testnet, and replay rejection;
- direct leaders, aggregators, nested multicalls, and nested ERC-4337 operations where the leader differs from bundler/paymaster/outer sender;
- cross-adapter negatives, including a valid V3 call to the wrong factory, wrong fee/quote, wrong V4 hook, and Virtuals attribution routed only to Bankr/Doppler;
- deterministic prediction agreement and mismatch rejection before receipts;
- entry and exit quotes for native curve, V3, V4/hook, and Doppler/Permit2 paths, including the rule that leader min-out is never copied;
- an instrumented assertion that the candidate path performs no RPC, HTTP, database, filesystem, JSON, logging, Telegram, or process-spawn operation;
- proxy, implementation, module, hook, or config drift disabling only the affected registry entry;
- unchanged Noxa positive and negative fixtures, calldata, value, quote asset, fee, risk behavior, and submission sequencing.

Stage telemetry remains shared and async: `feed_received`, `decoded`, `matched`, `planned`, `built`, `signed`, `submitted`, and `signature_returned`. Emitting or persisting those measurements must not enter the pre-submit dependency graph.

## Delivery phases

1. **Registry extraction:** move current Noxa constants and policy assumptions behind `LaunchpadSpec`/`AdapterKind`; prove byte-for-byte-equivalent Noxa plans and unchanged negative behavior.
2. **Tier 1 V3 paper path:** add bow.fun and LaunchHood V3 observation, prediction, planning, and reconciliation using the shared V3 adapter.
3. **V4 foundation:** implement pinned PoolKey/hook/dynamic-fee semantics and paper fixtures; onboard Clanker first, then klik discovery.
4. **Doppler identity:** layer Bankr/Doppler and bounded ERC-4337/Permit2 semantics over the V4 foundation; store Virtuals only as attribution.
5. **Gated discovery:** collect trench, Flap, Pons, hood.fun, and leavehood observations and reconciliation evidence without enabling live execution.

Promotion between phases requires the gates in [INTEGRATION_RANKING.md](./INTEGRATION_RANKING.md) and the candidate evidence reports. No phase implies live broadcast approval.
