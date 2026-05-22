# Copy Trading Wallet Research

Last researched: 2026-05-22

This note maps how to extend this Pump.fun Telegram notifier toward wallet copy-trade monitoring, then trading. It is technical research, not financial advice. Copy trading memecoin wallets is operationally risky: the target wallet can be wrong, delayed data can turn a good fill into a bad one, and hot-wallet/API-key mistakes can drain funds.

## Summary

The best path for this repo is:

1. Add wallet-trade monitoring first.
2. Normalize and alert on target-wallet buys/sells.
3. Add a dry-run copier that calculates the intended mirrored order without signing.
4. Only then add execution behind explicit env flags, per-wallet budgets, token blocklists, slippage caps, and kill switches.

PumpPortal is the most natural fit because the bot already uses its websocket. Current docs expose `subscribeAccountTrade` for trades made by specific wallets and warn to use one websocket connection for all subscriptions. Trading data now requires an API key and is metered; `subscribeNewToken` and `subscribeMigration` are still free.

## Current Project Fit

The repo already has useful building blocks:

- `src/pumpportal.ts` keeps one websocket connection and supports multiple subscription methods.
- `src/index.ts` already dedupes events, enriches token metadata, logs events, and sends Telegram alerts.
- `src/format.ts` already centralizes Telegram-safe formatting and explorer links.
- `src/subscribers.ts` stores verified Telegram subscribers and per-subscriber alert modes.

The current event classifier only accepts token creation and migration events. Wallet-trade events from `subscribeAccountTrade` would be received by the websocket but skipped as an unknown event. The extension should add a third event family instead of forcing wallet trades into the migration formatter.

## Data Ingestion Options

### Option A: PumpPortal `subscribeAccountTrade`

This is the recommended MVP.

PumpPortal supports a realtime websocket at:

```text
wss://pumpportal.fun/api/data?api-key=your-api-key-here
```

For wallet monitoring, send:

```json
{
  "method": "subscribeAccountTrade",
  "keys": ["WALLET_TO_WATCH"]
}
```

Benefits:

- Minimal change to the current architecture.
- Avoids implementing Pump.fun instruction decoding from raw Solana transactions.
- Same stream can also subscribe to token creation, migrations, token trades, and multiple wallet trades.

Constraints:

- Account/token trade streams require a PumpPortal API key.
- Effective May 1, 2026, trading data requires an API key.
- `subscribeAccountTrade` and `subscribeTokenTrade` are metered at 0.01 SOL per 10,000 trades streamed.
- PumpSwap data additionally requires the API-key-linked wallet to hold at least 0.02 SOL; otherwise the stream is restricted to bonding-curve trades.
- The event shape should be treated as unstable until we log live samples.

### Option B: Solana RPC `logsSubscribe` plus `getTransaction`

This is the fallback or verification path.

Solana websocket `logsSubscribe` can subscribe to transactions mentioning one address. The important limitation is that the `mentions` filter supports exactly one pubkey per subscription. After receiving a signature, call `getTransaction` with `jsonParsed` or `json` encoding to inspect balances, token balances, account keys, and instructions.

Benefits:

- First-party Solana API.
- Can validate PumpPortal events independently.
- Useful for non-Pump.fun/PumpSwap trades.

Constraints:

- One-wallet-per-subscription can get expensive and operationally noisy.
- You still need program-specific parsing to determine whether a transaction is a buy, sell, transfer, ATA creation, failed swap, or unrelated activity.
- Public RPC may rate limit or lag under load; production needs a paid RPC.

### Option C: Indexer or enhanced transaction provider

Services such as Helius, Bitquery, and similar providers can classify transactions better than raw RPC. This can be useful later for historical target-wallet scoring or broader DEX support. It is less attractive for the first version because this repo already has a PumpPortal dependency and the MVP can stay small.

## Execution Options

### PumpPortal Local Transaction API

Best first execution path if this project ever signs trades.

Endpoint:

```text
POST https://pumpportal.fun/api/trade-local
```

Request fields include:

- `publicKey`
- `action`: `buy` or `sell`
- `mint`
- `amount`
- `denominatedInSol`
- `slippage`
- `priorityFee`
- `pool`: `pump`, `raydium`, `pump-amm`, `launchlab`, `raydium-cpmm`, `bonk`, or `auto`

The API returns a serialized transaction. Your code signs locally and sends it through your RPC. That means the private key does not go to PumpPortal, but the key is still hot in your runtime. PumpPortal currently charges 0.5% on local trades, excluding Solana network fees and Pump.fun bonding curve fees.

Use this for Pump.fun/PumpSwap-specific copying because it has native pool support and avoids hand-building Pump.fun instructions.

### PumpPortal Lightning Transaction API

Endpoint:

```text
POST https://pumpportal.fun/api/trade?api-key=your-api-key-here
```

This is simpler and may be faster, but the API key controls a linked wallet. Anyone with that API key can trade with that wallet. PumpPortal charges 1% on Lightning trades, excluding network and bonding curve fees. For this project, Lightning is acceptable only for tiny isolated wallets, not a default.

### Jupiter Swap API

Jupiter is the better general Solana swap path once a token is trading with broader liquidity. Current Jupiter docs recommend the Swap v2 Meta-Aggregator for most integrations:

- `GET /swap/v2/order` gets a quote and assembled transaction.
- The wallet signs the transaction.
- `POST /swap/v2/execute` sends it through Jupiter managed landing.

This is a good post-migration fallback, especially for Raydium/Meteora/Jupiter-routable tokens. It is less ideal for very early Pump.fun bonding-curve buys.

## How Copy Trading Should Work

### Detect

Subscribe to one or more target wallets:

```json
{
  "method": "subscribeAccountTrade",
  "keys": ["TARGET_WALLET_1", "TARGET_WALLET_2"]
}
```

Normalize each event into:

```ts
interface CopyTradeSignal {
  source: "pumpportal" | "solana-rpc";
  observedAt: string;
  targetWallet: string;
  signature: string | null;
  action: "buy" | "sell" | "unknown";
  mint: string | null;
  pool: string | null;
  solAmount: number | null;
  tokenAmount: number | null;
  marketCapSol: number | null;
  raw: Record<string, unknown>;
}
```

For the first release, log raw events before executing anything. Then add field pickers after seeing real production payloads.

### Decide

Never blindly mirror the target wallet amount. A safe policy engine should calculate an order like:

- Buy fixed SOL amount per signal, e.g. `0.01 SOL`, regardless of target size.
- Optionally scale by target trade size, capped by `COPY_TRADE_MAX_SOL_PER_BUY`.
- Ignore signals above a maximum age, e.g. `COPY_TRADE_MAX_SIGNAL_AGE_MS=3000`.
- Ignore unknown pools or unsupported actions.
- Ignore tokens matching a denylist.
- Stop if daily spend exceeds `COPY_TRADE_DAILY_SOL_CAP`.
- Stop if wallet SOL balance falls below `COPY_TRADE_MIN_SOL_BALANCE`.
- Optional: do not copy sells unless the bot has a position from a copied buy.

### Simulate / Dry Run

Before live execution, add a dry-run mode that sends Telegram alerts like:

```text
Copy signal: TARGET bought TOKEN
Would buy: 0.01 SOL
Pool: pump
Signal age: 821ms
Skipped live trading: COPY_TRADE_DRY_RUN=true
```

This gives you event quality, latency, and false-positive data without risking funds.

### Execute

If live trading is enabled:

1. Confirm `COPY_TRADE_ENABLED=true`.
2. Confirm `COPY_TRADE_DRY_RUN=false`.
3. Confirm wallet private key exists only in env or a secure local keyfile.
4. Build a local transaction with PumpPortal `trade-local` for Pump.fun/PumpSwap pools.
5. Sign locally.
6. Send via a paid Solana RPC, optionally with priority fee.
7. Wait for confirmation.
8. Log request, planned trade, signature, result, and any error.
9. Alert Telegram with the executed signature.

## Suggested Repo Changes

### Phase 1: Wallet Watch Alerts

Add environment variables:

```env
COPY_TRADE_WALLETS=wallet1,wallet2
COPY_TRADE_ALERTS_ENABLED=true
```

Implementation:

- Extend `src/types.ts` with `WalletTradeData` / `CopyTradeSignal`.
- Extend `src/pumpportal.ts` so subscription methods can include keyed subscriptions:

```ts
{ method: "subscribeAccountTrade", keys: copyTradeWallets }
```

- Add `src/copy-trading.ts` to normalize wallet trade events.
- Add `formatCopyTradeMessage()` in `src/format.ts` or a new formatter module.
- Add a subscriber mode or command for copy alerts, likely `/copytrades`.

Do not add live execution in this phase.

### Phase 2: Dry-Run Planner

Add environment variables:

```env
COPY_TRADE_DRY_RUN=true
COPY_TRADE_BUY_SOL=0.01
COPY_TRADE_MAX_SOL_PER_BUY=0.02
COPY_TRADE_MAX_SIGNAL_AGE_MS=3000
COPY_TRADE_ALLOWED_POOLS=pump,pump-amm
COPY_TRADE_MIN_SOL_BALANCE=0.05
COPY_TRADE_DAILY_SOL_CAP=0.1
```

Implementation:

- Convert normalized signals into a `CopyTradePlan`.
- Store copied positions in `data/copy-trades.json` so sells can reference known bot holdings.
- Alert every planned skip reason, not just planned trades. Skip telemetry is how you learn whether the wallet is copyable.

### Phase 3: Local Execution

Add environment variables:

```env
COPY_TRADE_ENABLED=false
COPY_TRADE_PUBLIC_KEY=
COPY_TRADE_PRIVATE_KEY_BASE58=
COPY_TRADE_SLIPPAGE_PERCENT=10
COPY_TRADE_PRIORITY_FEE_SOL=0.00005
COPY_TRADE_POOL=pump
PUMPPORTAL_TRADE_LOCAL_URL=https://pumpportal.fun/api/trade-local
```

Implementation:

- Add `@solana/web3.js` and `bs58`.
- Add `src/trading/pumpportal-local.ts` to request serialized transactions.
- Keep signing isolated in one module.
- Refuse to run if live trading is enabled while dry-run is still true, if no risk limits are set, or if the wallet key is missing.
- Never log private keys, request headers, or raw signed transactions.

## Latency and Reliability Notes

Copy trading is a latency game. PumpPortal states that memecoin sniping is highly competitive and suggests Eastern US/New York hosting for lower latency. They also recommend avoiding `pool: "auto"` when speed matters because pool discovery can add delay.

Practical defaults:

- Start with alerts and dry-run for at least a few sessions.
- Use `pool: "pump"` while the token is on the bonding curve, `pump-amm` after migration, and only use `auto` when correctness matters more than latency.
- Use one websocket connection and resubscribe on reconnect.
- Deduplicate by signature plus target wallet plus action.
- Reject stale signals.
- Have an immediate kill switch in env and Telegram, e.g. `/copyoff`.

## Security Model

Minimum safety bar before live execution:

- Use a dedicated hot wallet with only the amount you are willing to lose.
- Keep all keys out of git; `.env` is already ignored, but also avoid shell history and logs.
- Prefer local signing over hosted/Lightning signing.
- Add explicit confirmation at startup when live trading is enabled.
- Send startup Telegram warnings showing budget caps and dry-run/live state.
- Log every live trade to a local JSONL audit log.
- Add tests for skip logic, amount caps, and dry-run behavior.

## Open Questions Before Coding Execution

- Which target wallet(s) should be watched?
- Should the first version only alert, or should it dry-run plans too?
- Fixed trade size or percentage of the target wallet's apparent trade size?
- Copy buys only, or buys and sells?
- What is the absolute max SOL per trade and per day?
- Should copied positions auto-sell when the target sells, or only alert?
- Which pools are allowed: `pump`, `pump-amm`, Raydium, Jupiter-routable tokens?

## Source Notes

- PumpPortal realtime websocket supports `subscribeNewToken`, `subscribeTokenTrade`, `subscribeAccountTrade`, and `subscribeMigration`, and recommends one websocket connection for multiple subscriptions: https://www.pumpportal.net/data-api/real-time/
- PumpPortal PumpSwap data requires an API key and a linked wallet funded with at least 0.02 SOL; account and token trade streams are available through keyed subscriptions: https://www.pumpportal.net/data-api/pump-swap/
- PumpPortal fees page says trading data requires an API key effective 2026-05-01, account/token trade data is metered at 0.01 SOL per 10,000 trades, local trades cost 0.5%, and Lightning trades cost 1%: https://pumpportal.fun/fees/
- PumpPortal Local Transaction API returns a serialized transaction for local signing and supports `buy`, `sell`, `mint`, `amount`, `denominatedInSol`, `slippage`, `priorityFee`, and `pool`: https://www.pumpportal.net/local-trading-api/trading-api/
- PumpPortal Lightning API trades through a linked API-key wallet and supports `skipPreflight` and `jitoOnly`: https://www.pumpportal.net/trading-api/
- PumpPortal FAQ notes the competitive latency environment, Eastern US/New York hosting suggestion, `pool: "auto"` latency tradeoff, and key-safety warnings: https://pumpportal.fun/FAQ/
- Solana `logsSubscribe` supports a `mentions` filter, but only one pubkey per subscription: https://solana.com/docs/rpc/websocket/logssubscribe
- Solana `getTransaction` returns confirmed transaction details, metadata, balances, token balances, account keys, and instructions: https://solana.com/docs/rpc/http/gettransaction
- Jupiter Swap v2 docs recommend the Meta-Aggregator path for most integrations: `/order` returns a quote and assembled transaction, then `/execute` handles managed landing after signing: https://developers.jup.ag/docs/swap
- Jupiter Order & Execute docs describe the three-step flow and warn to never commit private keys: https://developers.jup.ag/docs/swap/order-and-execute
