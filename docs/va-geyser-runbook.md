# VA RPC and Geyser Runbook

This runbook covers the VA Solana node used by the bot for RPC access and the Yellowstone Geyser wallet feed. Treat RPC, direct execution, and Geyser signal promotion as separate changes. Changing `SOLANA_RPC_URL` must not automatically enable live trading.

## Endpoints

- RPC: `http://va.pixellabz.io/`
- WebSocket: `ws://va.pixellabz.io/`
- Geyser gRPC: `http://va.pixellabz.io:9000`
- Geyser stream limit: 20 streams

The server IP must be allowlisted by the VA node operator. To check the public IP to provide:

```bash
curl -sS https://api.ipify.org && echo
```

## RPC Smoke Tests

Run these from the machine that will run the bot:

```bash
curl -sS -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
  http://va.pixellabz.io/

curl -sS -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getSlot","params":[{"commitment":"processed"}]}' \
  http://va.pixellabz.io/
```

Expected result: `getHealth` returns `ok`, and `getSlot` returns a recent slot number.

## RPC-Only Rollout

Use this when only moving Solana RPC reads/sends to the VA node:

```env
SOLANA_RPC_URL=http://va.pixellabz.io/
```

Leave copy-trade and direct-execution gates unchanged:

```env
COPY_TRADE_ENABLED=false
COPY_TRADE_DRY_RUN=true
DIRECT_EXECUTION_ENABLED=false
DIRECT_EXECUTION_LIVE_ENABLED=false
GEYSER_ENABLED=false
COPY_TRADE_SIGNAL_PROVIDER=pumpportal
```

Restart the bot, then confirm startup logs still show live trading disabled unless you intentionally enabled it. RPC replacement alone should not change whether orders can be submitted.

Rollback:

```env
SOLANA_RPC_URL=https://api.mainnet-beta.solana.com
```

## Direct-Execution Canary

Direct execution controls whether the bot locally builds/signs/sends copy-trade transactions. It is independent from the Geyser feed.

Safe canary defaults:

```env
COPY_TRADE_ENABLED=true
COPY_TRADE_DRY_RUN=false
COPY_TRADE_EXECUTION_PROVIDER=direct-auto
DIRECT_EXECUTION_ENABLED=true
DIRECT_EXECUTION_LIVE_ENABLED=true
DIRECT_EXECUTION_CONFIRMATION_MODE=background
DIRECT_EXECUTION_CANARY_CHAT_IDS=1355697770
DIRECT_EXECUTION_CANARY_WALLETS=
COPY_TRADE_MAX_BUY_SOL=0.001
COPY_TRADE_DAILY_SOL_CAP=0.003
COPY_TRADE_MAX_SIGNAL_AGE_MS=60000
COPY_TRADE_MAX_SLIPPAGE=15
COPY_TRADE_MAX_PRIORITY_FEE=0.0002
COPY_TRADE_MAX_COPY_WALLETS_PER_CHAT=1
```

Expected startup log:

```text
Copy trade execution state: LIVE | executionProvider=direct-auto | COPY_TRADE_ENABLED=true | COPY_TRADE_DRY_RUN=false | ...
Direct execution controls | provider=direct-auto | enabled=true | live=true | ...
```

Rollback direct execution separately from RPC/Geyser:

```env
COPY_TRADE_ENABLED=false
COPY_TRADE_DRY_RUN=true
DIRECT_EXECUTION_LIVE_ENABLED=false
```

## Geyser Observe-Only

Use observe-only mode first to prove the feed parses watched-wallet transactions without triggering orders or alerts:

```env
GEYSER_ENABLED=true
GEYSER_GRPC_URL=http://va.pixellabz.io:9000
GEYSER_X_TOKEN=
COPY_TRADE_SIGNAL_PROVIDER=pumpportal
```

Expected startup log:

```text
Copy trade signal provider | mode=pumpportal | pumpPortal=trigger | geyser=diagnostic
Connected to Geyser stream for 1 watched wallet(s)
```

If there are no watched/copy-trade wallets, the expected log is:

```text
Geyser listener idle: no watched wallets
```

## Geyser Client Smoke Test

The repo dependency can connect without reflection. Use the bot logs as the primary smoke test, or run a short local script after `npm install`:

```bash
node --input-type=module <<'NODE'
import Client from "@triton-one/yellowstone-grpc";
const client = new Client("http://va.pixellabz.io:9000", undefined, {});
const stream = await client.subscribe();
stream.on("data", () => {
  console.log("geyser update received");
  stream.end();
  process.exit(0);
});
stream.on("error", (error) => {
  console.error(error.message);
  process.exit(1);
});
stream.write({
  slots: { client: {} },
  accounts: {},
  transactions: {},
  transactionsStatus: {},
  blocks: {},
  blocksMeta: {},
  entry: {},
  accountsDataSlice: [],
  commitment: 0
});
setTimeout(() => {
  console.log("connected; no update received before timeout");
  stream.end();
  process.exit(0);
}, 5000);
NODE
```

## Feed Comparison

Parsed wallet events are written to `WALLET_TRADE_LOG_PATH`, defaulting to `logs/wallet-trades.jsonl`.

Run:

```bash
npm run wallet-feed-report -- --path=logs/wallet-trades.jsonl --limit=50
```

Useful narrower window:

```bash
npm run wallet-feed-report -- --path=logs/wallet-trades.jsonl --since=2026-05-30T00:00:00Z --limit=50
```

Read the output:

- `matchedGroups` shows events seen by more than one provider.
- `Matched winners` shows which provider was first.
- `pumpportal->geyser` or `geyser->pumpportal` shows lag after the winning provider.
- `parseErrors=0` is expected.

## Parallel Signal Race

After observe-only parsing is clean, enable parallel race mode:

```env
GEYSER_ENABLED=true
GEYSER_GRPC_URL=http://va.pixellabz.io:9000
COPY_TRADE_SIGNAL_PROVIDER=parallel
```

Expected startup log:

```text
Copy trade signal provider | mode=parallel | pumpPortal=trigger | geyser=trigger
Connected to Geyser stream for 1 watched wallet(s)
Connected to PumpPortal websocket
```

Expected race log on a watched-wallet buy:

```text
Copy trade signal race: {"event":"copy_trade_signal_race","mode":"parallel","provider":"pumpportal","outcome":"won",...}
Copy trade signal race: {"event":"copy_trade_signal_race","mode":"parallel","provider":"geyser","outcome":"duplicate",...}
```

Only copyable SOL-to-token buys should claim the race. Sells and stale/source-blocked signals should log `outcome":"skipped"` and must not submit copy buys.

Rollback Geyser signal promotion without disabling RPC or direct execution:

```env
COPY_TRADE_SIGNAL_PROVIDER=pumpportal
```

Rollback the Geyser stream entirely:

```env
GEYSER_ENABLED=false
COPY_TRADE_SIGNAL_PROVIDER=pumpportal
```

## Stream Limit Guidance

The VA Geyser endpoint allows 20 streams. This bot should use one shared Geyser stream and place all watched/copy-trade wallets into that subscription. Do not start multiple bot instances against the same endpoint unless the operator has confirmed there is spare stream capacity.

Safe operating rules:

- Keep one production bot process.
- Do not run background experiments that create their own long-lived Geyser streams.
- Prefer `setWallets`/subscription refreshes inside the running bot over starting extra clients.
- Stop local test scripts after smoke testing.

## Deployment Checklist

1. Confirm the server IP is allowlisted.
2. Run RPC smoke tests.
3. Deploy the code.
4. Start with `COPY_TRADE_SIGNAL_PROVIDER=pumpportal`.
5. Enable `GEYSER_ENABLED=true` and verify observe-only logs.
6. Run `npm run wallet-feed-report` after watched-wallet activity.
7. Enable `COPY_TRADE_SIGNAL_PROVIDER=parallel` only after parser quality is clean.
8. Keep direct execution gates, canary chat/wallet lists, and trade caps separate from RPC/Geyser changes.
9. Roll back the smallest layer needed: signal provider, Geyser stream, direct execution, or RPC.

