# NOXA canary gates

This runbook separates the funded testnet exercise from any mainnet action.
The `hermes-noxa` testnet canary command never reads or stores a private key: it
accepts only an externally signed EIP-2718 transaction file.

## Testnet canary

Use a new throwaway account, fund it from Robinhood's official testnet faucet,
and sign a small data-free self-transfer on chain `46630`. The signed nonce must
equal the account's pending nonce. Keep the signed transaction file inside the
active workspace and never place a seed phrase or private key there.

First run the read-only validation path. Omission of `--broadcast` is an
intentional safety gate:

```bash
hermes-noxa testnet-submit-canary \
  --source fsn1-codex \
  --raw-tx-file ./canary-46630.raw \
  --max-value-wei 100000000000000 \
  --max-gas-cost-wei 100000000000000
```

The command independently recovers the signer and rejects the transaction
unless all of these hold:

- chain ID is exactly `46630`;
- destination equals the recovered signer and calldata is empty;
- value and maximum gas cost fit their explicit CLI caps;
- signed nonce equals the testnet RPC's pending nonce;
- native balance covers value plus worst-case gas;
- the conditional window is derived from the latest Robinhood block's parent
  Ethereum height.

Review the emitted `noxa_testnet_canary_validated` record before repeating the
same command with `--broadcast`. Only an explicit early-boundary response may
retry the identical bytes. Rate limits, malformed replies, and ambiguous
transport results switch to hash/receipt reconciliation. An unresolved
submission retains its nonce and risk reservation; do not sign a replacement.

Capture stdout to one JSONL file per host, using stable `--source` labels such
as `fsn1-codex` and `us-east-2-ohio`. Compare completed runs with
`ops/summarize-testnet-canaries.sh`. Use identical value/gas caps and canary
counts in both regions; compare included receipts and report unresolved hashes
separately.

For a swap canary, first establish the official testnet wrapped-native and
router deployments. Run `testnet-preflight` to prove wrapped balance, allowance,
router bytecode, gas balance, and chain ID. Do not substitute guessed mainnet or
OP-stack addresses when testnet bytecode is absent.

## Mainnet preparation only

There is deliberately no general mainnet submission command. A mainnet canary
requires a separate, transaction-specific approval after all of the following
evidence is attached:

1. The complete two-hour Falkenstein report has zero unexplained sequence gaps,
   disconnects, decoder failures, or RPC-throttle blind spots.
2. The Ohio/us-east-2 benchmark is complete and the selected route is justified
   by matched samples and clock uncertainty.
3. At the pinned block, the NOXA factory runtime hash matches the audited pin
   and `launchEnabled()` is true. A paused factory is a hard stop.
4. The observed launch receipt is hydrated and its pool, pair token, fee,
   restriction end, max-wallet, and max-transaction limits are verified.
5. Quote, calldata, recipient, deadline, allowance, balance, nonce, chain ID,
   fees, slippage, maximum exposure, maximum gas loss, and session-loss latch
   are printed for human review.
6. The exact transaction hash, raw-byte hash, maximum total loss, and one-time
   approval are recorded. Approval of this runbook or a testnet canary is not
   approval to broadcast on mainnet.
7. A kill condition is defined for feed discontinuity, runtime-hash change,
   factory pause, stale state, receipt mismatch, nonce ambiguity, or any cap
   violation.

After approval, broadcast only the reviewed bytes. Never automatically replace
an ambiguous nonce, widen caps, change calldata, or retry a different signed
transaction.

### Enforced live-canary profile

`hermes-noxa-runtime` fails closed before a signed mainnet broadcast unless all
of these invariants hold:

- deployment is `active-noxa`, whose factory is pinned to
  `0x52453b4289a6c3a70bb8b4682bcd3d8731267e28`;
- the launch DEX reports the allowlisted Uniswap V3 factory and SwapRouter02
  `0xcaf681a66d020601342297493863e78c959e5cb2`;
- entry amount, per-trade cap, and total open-exposure cap are all exactly
  `100000000000000` wei (`0.0001 WETH`);
- the configured and observed signer WETH balance is no more than
  `1000000000000000` wei (`0.001 WETH`);
- configured slippage and its cap are no more than `100` basis points;
- the router allowance equals the single capped entry exactly;
- the separately supplied kill-switch file is an owned, non-symlinked,
  mode-0600 regular file containing exactly `ARMED` followed by a newline.

The runtime checks the kill switch at startup, before every signed step, and
again after the feed boundary releases a transaction but before sequencer
submission. Changing or removing the file prevents new submissions. The live
invocation must therefore include these explicit arguments in addition to the
signer and one-time approval arguments:

```text
--deployment active-noxa
--amount-in 100000000000000
--max-trade-amount-in 100000000000000
--max-open-exposure 100000000000000
--max-wallet-weth-balance 1000000000000000
--slippage-bps 100
--max-slippage-bps 100
--kill-switch-file /srv/codex-workspaces/.../live-canary.armed
```

Disarming the switch is an operator stop, not permission to replace or retry a
transaction whose submission outcome is ambiguous.

### One-trigger bounded copy canary

The bounded copy canary is narrower than the general preparation workflow. It
may be run only after a paper watch has produced all of the following in one
uninterrupted session:

- a factory-proven and independently receipt-audited leader entry;
- a guarded paper order with measured detection-to-order latency;
- entry reconciliation followed by an automatic full-position exit;
- zero final exposure, zero remaining positions, and no pending order;
- a completed health window with no missing feed sequence and exact reconnects;
- no post-startup RPC retry, rate-limit, server, or transport error.

The 2026-07-15 Falkenstein evidence satisfying those gates is recorded in
`oracle/active-noxa-bounded-exit-paper-20260715.json`. That evidence is not by
itself authorization to spend mainnet funds.

The operator must separately review and approve this complete envelope before
launching it:

- recipient and expected signer:
  `0x73bdbc2f46ed5b637431413a5f88f277cf1a52b5`;
- watched leader: `0xa5cd583d88ab54572bc1ea177388d44e77fde5b1`;
- active-Noxa token: `0xfab5f908facac184db22c69caee7b6258e215223`;
- exactly one trigger and exactly `100000000000000` wei WETH entry exposure;
- at most 100 basis points of configured slippage;
- automatic full-position exit with at least `99000000000000` wei WETH out;
- at most `1000000000000` wei realized session loss and
  `70000000000000` wei gas cost per signed step;
- exactly `350000` gas, `200000000` wei maximum fee per gas, and zero priority
  fee; startup additionally requires that cap to remain at least twice the
  latest Robinhood base fee;
- at most `1000000000000000` wei WETH in the signer wallet;
- the pinned active-Noxa factory and router enforced by the runtime;
- an armed private kill-switch file checked before every signed step and again
  before sequencer submission.

Before starting, the operator must independently confirm that the signer has
enough native gas, between `100000000000000` and `1000000000000000` wei WETH,
and an exact `100000000000000` wei WETH allowance to the pinned router. The
runtime preflight fails closed if these conditions, the chain, bytecode, nonce,
or wallet cap do not match.

After recording the separate approval, the credential-bearing invocation is
operator-executed. `KEYSTORE` and `KILL_SWITCH` are private paths supplied by
the operator; neither value is committed:

```bash
/srv/codex-workspaces/pumpfunnoti/hermes-feed/target/release/hermes-noxa-runtime \
  --mode signed \
  --deployment active-noxa \
  --strategy copy \
  --recipient 0x73bDBC2f46Ed5b637431413a5F88f277cf1A52B5 \
  --expected-address 0x73bDBC2f46Ed5b637431413a5F88f277cf1A52B5 \
  --watch-wallet 0xa5cd583d88ab54572bc1ea177388d44e77fde5b1 \
  --copy-token 0xfab5f908facac184db22c69caee7b6258e215223 \
  --copy-max-triggers 1 \
  --copy-trust-leader-limit-price \
  --amount-in 100000000000000 \
  --max-trade-amount-in 100000000000000 \
  --max-open-exposure 100000000000000 \
  --max-wallet-weth-balance 1000000000000000 \
  --max-gas-cost-wei 70000000000000 \
  --max-session-loss 1000000000000 \
  --slippage-bps 100 \
  --max-slippage-bps 100 \
  --gas-limit 350000 \
  --max-fee-per-gas 200000000 \
  --max-priority-fee-per-gas 0 \
  --l1-window 3 \
  --timestamp-window-seconds 30 \
  --copy-bounded-exit-min-weth-out 99000000000000 \
  --run-seconds 3600 \
  --warmup-seconds 5 \
  --reconciliation-seconds 20 \
  --keystore "$KEYSTORE" \
  --password-fd 3 \
  --broadcast \
  --approval-token MAINNET_CANARY_APPROVED \
  --kill-switch-file "$KILL_SWITCH" \
  3< <(systemd-ask-password "Hermes mainnet canary keystore password")
```

Do not start a replacement if any submission becomes ambiguous. Disarm the
kill switch and reconcile the leased nonce and transaction hash first.
