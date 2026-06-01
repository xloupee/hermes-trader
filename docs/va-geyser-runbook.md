# VA RPC and Geyser Runbook

This runbook keeps three changes separate: using the VA Solana RPC, enabling direct execution, and promoting Geyser from observe-only to a copy-trade trigger.

## Endpoints

- RPC: `http://va.pixellabz.io/`
- WebSocket: `ws://va.pixellabz.io/`
- Geyser gRPC: `http://va.pixellabz.io:9000`
- Geyser stream limit: 20 streams

The production VPS egress IP must be whitelisted by the VA node operator. The current VPS IP used for the bot is `157.90.240.233`.

## RPC Access Check

Run these from the host that will run the bot:

```bash
curl -sS -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
  http://va.pixellabz.io/

curl -sS -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getSlot","params":[{"commitment":"processed"}]}' \
  http://va.pixellabz.io/
```

Expected: `getHealth` returns `ok`, and `getSlot` returns a current slot. If either returns `403`, the host IP is not whitelisted.

## RPC-Only Rollout

Set only:

```bash
SOLANA_RPC_URL=http://va.pixellabz.io/
```

Restart the existing service and confirm startup logs include the plain VA RPC endpoint. Changing RPC does not enable live trading by itself; live behavior is still gated by `COPY_TRADE_ENABLED`, `COPY_TRADE_DRY_RUN`, execution provider settings, direct execution mode flags, optional wallet gates, and emergency stop state.

Rollback:

```bash
SOLANA_RPC_URL=https://api.mainnet-beta.solana.com
```

or restore the previous provider URL and restart the service.

## Direct Execution

Use tiny caps first:

```bash
COPY_TRADE_EXECUTION_PROVIDER=direct-auto
COPY_TRADE_ENABLED=true
COPY_TRADE_DRY_RUN=false
COPY_TRADE_MAX_BUY_SOL=0.001
COPY_TRADE_DAILY_SOL_CAP=0.003
COPY_TRADE_MAX_SIGNAL_AGE_MS=60000
COPY_TRADE_MAX_SLIPPAGE=15
COPY_TRADE_MAX_PRIORITY_FEE=0.0002
DIRECT_EXECUTION_ENABLED=true
DIRECT_EXECUTION_LIVE_ENABLED=true
DIRECT_EXECUTION_BUILD_ONLY=false
DIRECT_EXECUTION_SIMULATE_ONLY=false
DIRECT_EXECUTION_CONFIRMATION_MODE=background
```

Keep one funded hot wallet and one copied wallet in scope. Verify the submitted buy signature on-chain, treasury/platform-fee deltas if fees are enabled, and any post-buy sell automation before increasing caps.

Rollback direct execution:

```bash
COPY_TRADE_EXECUTION_PROVIDER=pumpportal-lightning
DIRECT_EXECUTION_LIVE_ENABLED=false
```

For a hard stop, set `COPY_TRADE_ENABLED=false`, `COPY_TRADE_DRY_RUN=true`, or activate the copy-trade emergency stop from Telegram.

## Geyser Observe-Only

Start with:

```bash
GEYSER_ENABLED=true
GEYSER_GRPC_URL=http://va.pixellabz.io:9000
GEYSER_GRPC_TOKEN=
GEYSER_COMMITMENT=processed
GEYSER_SHADOW_ONLY=true
GEYSER_RECONNECT_MS=2000
COPY_TRADE_SIGNAL_PROVIDER=pumpportal
```

Expected logs:

- `Yellowstone gRPC subscribed to <n> wallet(s) at processed commitment in shadow mode`
- `Yellowstone wallet trade candidate: ...`
- `Yellowstone shadow wallet trade event: ...`
- `Yellowstone wallet trade rejected: ...` for unsupported, vote, failed, non-SOL-quote, or ambiguous transactions

Observe-only Geyser writes wallet-trade log entries but does not trigger Telegram alerts or copy buys.

The monitor uses shared transaction subscriptions for watched wallets. Do not create one Geyser stream per wallet; stay under the 20-stream limit by keeping a single shared bot stream.

## Feed Comparison

Let PumpPortal, Helius, and Geyser run for a defined canary window with active watched wallets. Keep PumpPortal as the trigger:

```bash
COPY_TRADE_SIGNAL_PROVIDER=pumpportal
GEYSER_SHADOW_ONLY=true
```

Then summarize:

```bash
npm run feed-latency -- --log logs/wallet-trades.jsonl
```

If you captured service logs to a file, include parser rejection counts:

```bash
npm run feed-latency -- --log logs/wallet-trades.jsonl --app-log logs/service.log
```

The report groups events by `signature + targetWallet + mint`, shows which provider saw each signature first, p50/p90 lag where timestamps are available, duplicate counts, missing-provider counts, parser/source counts, and example signatures for manual explorer review.

## Geyser Promotion

Only promote after observe-only parser output is clean and the comparison report shows Geyser is faster and reliable enough.

Canary promotion:

```bash
GEYSER_SHADOW_ONLY=false
COPY_TRADE_SIGNAL_PROVIDER=parallel
```

In `parallel`, PumpPortal and Geyser can both emit the same source transaction, but the shared signature/wallet duplicate guard plus durable copy-buy idempotency allow only one copy-buy attempt per copied mint/chat. Keep the same tiny direct-exec caps from the direct execution section.

Geyser-only mode:

```bash
COPY_TRADE_SIGNAL_PROVIDER=geyser
```

Do not use Geyser-only until parallel mode has clean evidence across a live canary window.

Rollback signal provider:

```bash
COPY_TRADE_SIGNAL_PROVIDER=pumpportal
GEYSER_SHADOW_ONLY=true
```

Then restart the service and confirm startup logs show `Copy trade signal provider: pumpportal`.
