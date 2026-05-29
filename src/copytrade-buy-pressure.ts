import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { asRecord, stringValue } from "./types.js";
import type { LooseRecord, SubscriberRecord, WalletTradeAction, WalletTradeData, WatchedWallet } from "./types.js";
import type { TradeExecutionProvider } from "./trade-execution.js";

export type CopyTradeBuyPressureSellTriggerKind = "buy_pressure" | "timeout";

export interface CopyTradeBuyPressureSellConfig {
  enabled: boolean;
  sellPercent: number;
  timeoutMs: number;
  minBuys: number;
  minTotalSol: number;
}

export interface CopyTradeBuyPressureSellTrigger {
  kind: CopyTradeBuyPressureSellTriggerKind;
  reason: string;
  buyCount: number;
  buySol: number;
  signature: string | null;
}

export interface CopyTradeBuyPressureSellWatcher {
  id: string;
  chatId: string;
  sourceWalletAddress: string;
  sourceWalletLabel: string | null;
  tradingWalletPublicKey: string;
  mint: string;
  observedSignature: string | null;
  copyBuySignature: string;
  executionProvider: TradeExecutionProvider;
  trade: WalletTradeData;
  copyTradeWallet: WatchedWallet;
  preBuyTokenBalance: number | null;
  postBuyTokenBalance: number | null;
  createdAtMs: number;
  expiresAtMs: number;
  confirmedAtMs: number;
  sellPercent: number;
  minBuys: number;
  minTotalSol: number;
  buyCount: number;
  buySol: number;
  buyKeys: string[];
  triggeredAtMs?: number | null;
  triggerKind?: CopyTradeBuyPressureSellTriggerKind | null;
  triggerReason?: string | null;
}

export interface CopyTradeBuyPressureSellStore {
  load: () => Promise<CopyTradeBuyPressureSellWatcher[]>;
  save: (watchers: CopyTradeBuyPressureSellWatcher[]) => Promise<void>;
}

function finiteNumber(value: unknown): number | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function positiveNumber(value: unknown, fallback: number): number {
  const number = finiteNumber(value);
  return number !== null && number > 0 ? number : fallback;
}

function nonNegativeNumber(value: unknown, fallback: number): number {
  const number = finiteNumber(value);
  return number !== null && number >= 0 ? number : fallback;
}

function positiveInteger(value: unknown, fallback: number): number {
  return Math.max(1, Math.floor(positiveNumber(value, fallback)));
}

function boundedPercent(value: unknown, fallback: number): number {
  return Math.min(100, positiveNumber(value, fallback));
}

function normalizeAction(value: unknown): WalletTradeAction {
  return value === "buy" || value === "sell" || value === "swap" || value === "unknown" ? value : "unknown";
}

function normalizeWalletTradeAsset(value: unknown): WalletTradeData["input"] {
  const record = asRecord(value);

  if (Object.keys(record).length === 0) {
    return null;
  }

  return {
    mint: stringValue(record.mint)?.trim() || null,
    symbol: stringValue(record.symbol)?.trim() || null,
    amount: finiteNumber(record.amount)
  };
}

function normalizeWalletTradeData(value: unknown): WalletTradeData | null {
  const record = asRecord(value);
  const targetWallet = stringValue(record.targetWallet)?.trim();
  const mint = stringValue(record.mint)?.trim() || null;

  if (!targetWallet || !mint) {
    return null;
  }

  return {
    observedAt: stringValue(record.observedAt) || new Date().toISOString(),
    provider: record.provider === "helius" ? "helius" : "pumpportal",
    targetWallet,
    label: stringValue(record.label)?.trim() || null,
    action: normalizeAction(record.action),
    mint,
    signature: stringValue(record.signature)?.trim() || null,
    timestamp: finiteNumber(record.timestamp),
    feePayer: stringValue(record.feePayer)?.trim() || null,
    source: stringValue(record.source)?.trim() || null,
    input: normalizeWalletTradeAsset(record.input),
    output: normalizeWalletTradeAsset(record.output),
    solAmount: finiteNumber(record.solAmount),
    tokenAmount: finiteNumber(record.tokenAmount),
    pool: stringValue(record.pool)?.trim() || null,
    marketCapSol: finiteNumber(record.marketCapSol),
    pumpFunUrl: stringValue(record.pumpFunUrl)?.trim() || null,
    solscanTokenUrl: stringValue(record.solscanTokenUrl)?.trim() || null,
    solscanTxUrl: stringValue(record.solscanTxUrl)?.trim() || null,
    raw: asRecord(record.raw)
  };
}

function normalizeWatchedWallet(value: unknown): WatchedWallet | null {
  const record = asRecord(value);
  const address = stringValue(record.address)?.trim();

  if (!address) {
    return null;
  }

  return {
    address,
    label: stringValue(record.label)?.trim() || null,
    addedAt: stringValue(record.addedAt) || new Date().toISOString(),
    updatedAt: stringValue(record.updatedAt) || new Date().toISOString()
  };
}

function normalizeExecutionProvider(value: unknown): TradeExecutionProvider {
  return value === "direct-pump" || value === "direct-pumpswap" || value === "direct-auto" ? value : "pumpportal-lightning";
}

export function copyTradeBuyPressureSellWatcherId({
  chatId,
  tradingWalletPublicKey,
  mint,
  copyBuySignature
}: {
  chatId: string;
  tradingWalletPublicKey: string;
  mint: string;
  copyBuySignature: string;
}): string {
  return [chatId, tradingWalletPublicKey, mint, copyBuySignature].join(":");
}

export function createCopyTradeBuyPressureSellWatcher({
  config,
  subscriber,
  trade,
  copyTradeWallet,
  buySignature,
  executionProvider,
  preBuyTokenBalance = null,
  postBuyTokenBalance = null,
  nowMs = Date.now()
}: {
  config: CopyTradeBuyPressureSellConfig;
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  copyTradeWallet: WatchedWallet;
  buySignature: string | null;
  executionProvider: TradeExecutionProvider;
  preBuyTokenBalance?: number | null;
  postBuyTokenBalance?: number | null;
  nowMs?: number;
}): CopyTradeBuyPressureSellWatcher | null {
  const mint = trade.mint?.trim();
  const tradingWalletPublicKey = subscriber.tradingWallet?.publicKey?.trim();
  const copyBuySignature = buySignature?.trim();

  if (!config.enabled || !mint || !tradingWalletPublicKey || !copyBuySignature) {
    return null;
  }

  const sellPercent = boundedPercent(config.sellPercent, 100);
  const timeoutMs = positiveInteger(config.timeoutMs, 120_000);
  const minBuys = positiveInteger(config.minBuys, 1);
  const minTotalSol = nonNegativeNumber(config.minTotalSol, 0);

  return {
    id: copyTradeBuyPressureSellWatcherId({
      chatId: subscriber.chatId,
      tradingWalletPublicKey,
      mint,
      copyBuySignature
    }),
    chatId: subscriber.chatId,
    sourceWalletAddress: copyTradeWallet.address,
    sourceWalletLabel: copyTradeWallet.label,
    tradingWalletPublicKey,
    mint,
    observedSignature: trade.signature,
    copyBuySignature,
    executionProvider,
    trade,
    copyTradeWallet,
    preBuyTokenBalance,
    postBuyTokenBalance,
    createdAtMs: nowMs,
    expiresAtMs: nowMs + timeoutMs,
    confirmedAtMs: nowMs,
    sellPercent,
    minBuys,
    minTotalSol,
    buyCount: 0,
    buySol: 0,
    buyKeys: []
  };
}

export function copyTradeBuyPressureTradeKey(trade: WalletTradeData): string | null {
  if (trade.signature) {
    return `signature:${trade.signature}`;
  }

  if (!trade.mint || !trade.targetWallet) {
    return null;
  }

  return [
    "synthetic",
    trade.provider,
    trade.targetWallet,
    trade.mint,
    trade.timestamp ?? trade.observedAt,
    trade.solAmount ?? "",
    trade.tokenAmount ?? ""
  ].join(":");
}

function tradeTimestampMs(trade: WalletTradeData): number | null {
  if (trade.timestamp === null || trade.timestamp === undefined || !Number.isFinite(trade.timestamp)) {
    return null;
  }

  return trade.timestamp > 10_000_000_000 ? trade.timestamp : trade.timestamp * 1000;
}

export function isOwnCopyTradeBuyPressureTrade({
  watcher,
  trade
}: {
  watcher: CopyTradeBuyPressureSellWatcher;
  trade: WalletTradeData;
}): boolean {
  const signature = trade.signature?.trim();

  return Boolean(
    (signature && (signature === watcher.copyBuySignature || signature === watcher.observedSignature)) ||
      trade.targetWallet === watcher.tradingWalletPublicKey ||
      trade.feePayer === watcher.tradingWalletPublicKey
  );
}

function buyPressureReason(watcher: CopyTradeBuyPressureSellWatcher): string {
  const totalSol = Number(watcher.buySol.toFixed(9));
  const totalLabel = watcher.minTotalSol > 0 ? ` / ${totalSol} SOL` : "";
  return `buy-pressure trigger after ${watcher.buyCount} buy${watcher.buyCount === 1 ? "" : "s"}${totalLabel}`;
}

function timeoutReason(watcher: CopyTradeBuyPressureSellWatcher): string {
  const seconds = Math.max(0, Math.round((watcher.expiresAtMs - watcher.createdAtMs) / 1000));
  return `timeout fallback after ${seconds}s without qualifying buy pressure`;
}

export function applyCopyTradeBuyPressureTrade({
  watcher,
  trade
}: {
  watcher: CopyTradeBuyPressureSellWatcher;
  trade: WalletTradeData;
}): {
  watcher: CopyTradeBuyPressureSellWatcher;
  changed: boolean;
  trigger: CopyTradeBuyPressureSellTrigger | null;
} {
  if (watcher.triggeredAtMs || trade.action !== "buy" || trade.mint !== watcher.mint || isOwnCopyTradeBuyPressureTrade({ watcher, trade })) {
    return { watcher, changed: false, trigger: null };
  }

  const observedAtMs = tradeTimestampMs(trade);
  if (observedAtMs !== null && observedAtMs < watcher.confirmedAtMs) {
    return { watcher, changed: false, trigger: null };
  }

  const key = copyTradeBuyPressureTradeKey(trade);
  if (!key || watcher.buyKeys.includes(key)) {
    return { watcher, changed: false, trigger: null };
  }

  const solAmount = Math.max(0, trade.solAmount ?? 0);
  const nextWatcher: CopyTradeBuyPressureSellWatcher = {
    ...watcher,
    buyCount: watcher.buyCount + 1,
    buySol: Number((watcher.buySol + solAmount).toFixed(9)),
    buyKeys: [...watcher.buyKeys, key].slice(-100)
  };
  const countMet = nextWatcher.buyCount >= nextWatcher.minBuys;
  const solMet = nextWatcher.minTotalSol <= 0 || nextWatcher.buySol >= nextWatcher.minTotalSol;

  if (!countMet || !solMet) {
    return { watcher: nextWatcher, changed: true, trigger: null };
  }

  return {
    watcher: nextWatcher,
    changed: true,
    trigger: {
      kind: "buy_pressure",
      reason: buyPressureReason(nextWatcher),
      buyCount: nextWatcher.buyCount,
      buySol: nextWatcher.buySol,
      signature: trade.signature
    }
  };
}

export function copyTradeBuyPressureTimeoutTrigger({
  watcher,
  nowMs = Date.now()
}: {
  watcher: CopyTradeBuyPressureSellWatcher;
  nowMs?: number;
}): CopyTradeBuyPressureSellTrigger | null {
  if (watcher.triggeredAtMs || nowMs < watcher.expiresAtMs) {
    return null;
  }

  return {
    kind: "timeout",
    reason: timeoutReason(watcher),
    buyCount: watcher.buyCount,
    buySol: watcher.buySol,
    signature: null
  };
}

export function claimCopyTradeBuyPressureSellTrigger({
  watcher,
  trigger,
  nowMs = Date.now()
}: {
  watcher: CopyTradeBuyPressureSellWatcher;
  trigger: CopyTradeBuyPressureSellTrigger;
  nowMs?: number;
}): CopyTradeBuyPressureSellWatcher {
  return {
    ...watcher,
    triggeredAtMs: nowMs,
    triggerKind: trigger.kind,
    triggerReason: trigger.reason
  };
}

function normalizeWatcher(value: unknown): CopyTradeBuyPressureSellWatcher | null {
  const record = asRecord(value);
  const trade = normalizeWalletTradeData(record.trade);
  const copyTradeWallet = normalizeWatchedWallet(record.copyTradeWallet);
  const chatId = stringValue(record.chatId)?.trim();
  const tradingWalletPublicKey = stringValue(record.tradingWalletPublicKey)?.trim();
  const mint = stringValue(record.mint)?.trim();
  const copyBuySignature = stringValue(record.copyBuySignature)?.trim();

  if (!trade || !copyTradeWallet || !chatId || !tradingWalletPublicKey || !mint || !copyBuySignature) {
    return null;
  }

  const createdAtMs = positiveInteger(record.createdAtMs, Date.now());
  const expiresAtMs = Math.max(createdAtMs, positiveInteger(record.expiresAtMs, createdAtMs + 120_000));

  return {
    id: stringValue(record.id)?.trim() || copyTradeBuyPressureSellWatcherId({ chatId, tradingWalletPublicKey, mint, copyBuySignature }),
    chatId,
    sourceWalletAddress: stringValue(record.sourceWalletAddress)?.trim() || copyTradeWallet.address,
    sourceWalletLabel: stringValue(record.sourceWalletLabel)?.trim() || null,
    tradingWalletPublicKey,
    mint,
    observedSignature: stringValue(record.observedSignature)?.trim() || null,
    copyBuySignature,
    executionProvider: normalizeExecutionProvider(record.executionProvider),
    trade,
    copyTradeWallet,
    preBuyTokenBalance: finiteNumber(record.preBuyTokenBalance),
    postBuyTokenBalance: finiteNumber(record.postBuyTokenBalance),
    createdAtMs,
    expiresAtMs,
    confirmedAtMs: finiteNumber(record.confirmedAtMs) ?? createdAtMs,
    sellPercent: boundedPercent(record.sellPercent, 100),
    minBuys: positiveInteger(record.minBuys, 1),
    minTotalSol: nonNegativeNumber(record.minTotalSol, 0),
    buyCount: Math.max(0, Math.floor(nonNegativeNumber(record.buyCount, 0))),
    buySol: nonNegativeNumber(record.buySol, 0),
    buyKeys: Array.isArray(record.buyKeys)
      ? record.buyKeys.map((key) => stringValue(key)?.trim()).filter((key): key is string => Boolean(key)).slice(-100)
      : [],
    triggeredAtMs: finiteNumber(record.triggeredAtMs),
    triggerKind: record.triggerKind === "buy_pressure" || record.triggerKind === "timeout" ? record.triggerKind : null,
    triggerReason: stringValue(record.triggerReason)?.trim() || null
  };
}

function serializeWatcher(watcher: CopyTradeBuyPressureSellWatcher): LooseRecord {
  return {
    id: watcher.id,
    chatId: watcher.chatId,
    sourceWalletAddress: watcher.sourceWalletAddress,
    sourceWalletLabel: watcher.sourceWalletLabel,
    tradingWalletPublicKey: watcher.tradingWalletPublicKey,
    mint: watcher.mint,
    observedSignature: watcher.observedSignature,
    copyBuySignature: watcher.copyBuySignature,
    executionProvider: watcher.executionProvider,
    trade: watcher.trade,
    copyTradeWallet: watcher.copyTradeWallet,
    preBuyTokenBalance: watcher.preBuyTokenBalance,
    postBuyTokenBalance: watcher.postBuyTokenBalance,
    createdAtMs: watcher.createdAtMs,
    expiresAtMs: watcher.expiresAtMs,
    confirmedAtMs: watcher.confirmedAtMs,
    sellPercent: watcher.sellPercent,
    minBuys: watcher.minBuys,
    minTotalSol: watcher.minTotalSol,
    buyCount: watcher.buyCount,
    buySol: watcher.buySol,
    buyKeys: watcher.buyKeys,
    triggeredAtMs: watcher.triggeredAtMs ?? null,
    triggerKind: watcher.triggerKind ?? null,
    triggerReason: watcher.triggerReason ?? null
  };
}

export function createJsonCopyTradeBuyPressureSellStore({ path }: { path: string }): CopyTradeBuyPressureSellStore {
  let queue = Promise.resolve();

  function withLock<T>(fn: () => Promise<T>): Promise<T> {
    const run = queue.then(fn, fn);
    queue = run.then(
      () => undefined,
      () => undefined
    );
    return run;
  }

  return {
    async load() {
      return withLock(async () => {
        let parsed: unknown;

        try {
          const text = await readFile(path, "utf8");
          parsed = text.trim() ? JSON.parse(text) as unknown : [];
        } catch (error) {
          if (error instanceof Error && "code" in error && error.code === "ENOENT") {
            return [];
          }

          throw error;
        }

        const records = Array.isArray(parsed) ? parsed : asRecord(parsed).watchers;
        return (Array.isArray(records) ? records : [])
          .map((record) => normalizeWatcher(record))
          .filter((watcher): watcher is CopyTradeBuyPressureSellWatcher => Boolean(watcher));
      });
    },
    async save(watchers) {
      return withLock(async () => {
        await mkdir(dirname(path), { recursive: true });
        const tempPath = `${path}.${process.pid}.${Date.now()}.tmp`;
        await writeFile(
          tempPath,
          `${JSON.stringify({ watchers: watchers.map(serializeWatcher) }, null, 2)}\n`,
          "utf8"
        );
        await rename(tempPath, path);
      });
    }
  };
}
