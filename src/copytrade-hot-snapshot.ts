import { createHash } from "node:crypto";
import { mkdir, rename, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import type { TradeExecutionProvider } from "./trade-execution.js";
import type {
  PumpPortalTradePool,
  SubscriberRecord,
  SubscriberStore,
  TelegramChatId,
  TradingWallet,
  TrailingSellConfig,
  TrailingSellMode,
  TrailingSellPercentBasis,
  WatchedWallet
} from "./types.js";

export const COPY_TRADE_HOT_PATH_SNAPSHOT_VERSION = 1;

export interface CopyTradeHotPathSnapshotRouting {
  executionProvider: TradeExecutionProvider;
  pool: PumpPortalTradePool;
  defaultSlippage: number;
  defaultPriorityFee: number;
  defaultTrailingSell: CopyTradeHotPathSnapshotTrailingSellConfig | null;
  priorityFeeMicroLamports: number | null;
  jitoTipLamports: number | null;
  jitoTipAccount: string | null;
  maxBuySol: number;
  dailySolCap: number;
  maxSignalAgeMs: number;
  minWalletReserveSol: number;
  allowedSources: string[];
  maxSlippage: number;
  maxPriorityFee: number;
  maxCopyWalletsPerChat: number;
  liveTradingEnabled: boolean;
  emergencyStopped: boolean;
}

export interface CopyTradeHotPathSnapshotTrailingSellStep {
  delayMs: number;
  percent: number;
}

export interface CopyTradeHotPathSnapshotTrailingSellConfig {
  enabled: boolean;
  mode: TrailingSellMode;
  percentBasis: TrailingSellPercentBasis;
  steps: CopyTradeHotPathSnapshotTrailingSellStep[];
}

export interface CopyTradeHotPathSnapshotSubscriber {
  chatId: string;
  tradingWalletPublicKey: string;
  signerKeypairPath: string | null;
  copyAmountSol: number;
  retryFailedBuys: boolean;
  buySlippage: number | null;
  buyPriorityFee: number | null;
  sellSlippage: number | null;
  sellPriorityFee: number | null;
  effectiveSellSlippage: number;
  effectiveSellPriorityFee: number;
  buyPressureSellEnabled: boolean;
  buyPressureSellTimeoutMs: number | null;
  dailySpentSol: number;
  wallets: Array<{
    address: string;
    label: string | null;
    trailingSell: CopyTradeHotPathSnapshotTrailingSellConfig | null;
  }>;
}

export interface CopyTradeHotPathSnapshotBody {
  version: typeof COPY_TRADE_HOT_PATH_SNAPSHOT_VERSION;
  sequence: number;
  generatedAtMs: number;
  routing: CopyTradeHotPathSnapshotRouting;
  subscribers: CopyTradeHotPathSnapshotSubscriber[];
}

export interface CopyTradeHotPathSnapshot extends CopyTradeHotPathSnapshotBody {
  checksum: string;
}

export interface CopyTradeHotSnapshotSubscriberStoreOptions {
  onChange: (reason: string) => Promise<void>;
  onError?: (error: Error, reason: string) => void;
}

type DailySpentSolLookup = ReadonlyMap<string, number> | Record<string, number>;
type MutatingSubscriberMethod =
  | "add"
  | "remove"
  | "setMode"
  | "setNotificationsPaused"
  | "watchWallet"
  | "renameWallet"
  | "unwatchWallet"
  | "watchCopyTradeWallet"
  | "renameCopyTradeWallet"
  | "setCopyTradeWalletEnabled"
  | "unwatchCopyTradeWallet"
  | "unwatchAllCopyTradeWallets"
  | "setCopyTradeWalletTrailingSellConfig"
  | "setTradingWallet"
  | "renameTradingWallet"
  | "setActiveTradingWallet"
  | "removeTradingWallet"
  | "removeAllTradingWallets"
  | "setCopyWallet"
  | "removeCopyWallet"
  | "setCopyAmountSol"
  | "setCopyTradeBuySlippage"
  | "setCopyTradeBuyPriorityFee"
  | "setCopyTradeSellSlippage"
  | "setCopyTradeSellPriorityFee"
  | "setCopyTradeRetryFailedBuys"
  | "setCopyTradeBuyPressureSellEnabled"
  | "setCopyTradeBuyPressureSellTimeoutMs"
  | "setCashbackPayoutWallet"
  | "resetCopyTradeExecutionSettings"
  | "setCopyTargetWallet";

export function copyTradeHotPathSnapshotChecksum(snapshot: CopyTradeHotPathSnapshotBody): string {
  return `sha256:${createHash("sha256").update(stableJsonStringify(snapshot)).digest("hex")}`;
}

export function attachCopyTradeHotPathSnapshotChecksum(snapshot: CopyTradeHotPathSnapshotBody): CopyTradeHotPathSnapshot {
  return {
    ...snapshot,
    checksum: copyTradeHotPathSnapshotChecksum(snapshot)
  };
}

export function validateCopyTradeHotPathSnapshot(snapshot: CopyTradeHotPathSnapshot): boolean {
  if (snapshot.version !== COPY_TRADE_HOT_PATH_SNAPSHOT_VERSION) {
    return false;
  }

  const { checksum, ...body } = snapshot;
  return checksum === copyTradeHotPathSnapshotChecksum(body);
}

export function createCopyTradeHotPathSnapshot({
  subscribers,
  routing,
  sequence,
  dailySpentSolByChatId,
  signerKeypairDir,
  generatedAtMs = Date.now()
}: {
  subscribers: SubscriberRecord[];
  routing: CopyTradeHotPathSnapshotRouting;
  sequence: number;
  dailySpentSolByChatId?: DailySpentSolLookup;
  signerKeypairDir?: string | null;
  generatedAtMs?: number;
}): CopyTradeHotPathSnapshot {
  if (!Number.isSafeInteger(sequence) || sequence <= 0) {
    throw new Error("copy trade hot-path snapshot sequence must be a positive safe integer");
  }

  return attachCopyTradeHotPathSnapshotChecksum({
    version: COPY_TRADE_HOT_PATH_SNAPSHOT_VERSION,
    sequence,
    generatedAtMs,
    routing,
    subscribers: !routing.emergencyStopped
      ? subscribers
          .map((subscriber) => snapshotSubscriber({
            subscriber,
            routing,
            dailySpentSolByChatId,
            signerKeypairDir
          }))
          .filter((subscriber): subscriber is CopyTradeHotPathSnapshotSubscriber => Boolean(subscriber))
          .sort((a, b) => a.chatId.localeCompare(b.chatId))
      : []
  });
}

export async function writeCopyTradeHotPathSnapshotFile(path: string, snapshot: CopyTradeHotPathSnapshot): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  const tempPath = `${path}.${process.pid}.${snapshot.sequence}.tmp`;
  await writeFile(tempPath, `${JSON.stringify(snapshot)}\n`, { encoding: "utf8", mode: 0o600 });
  await rename(tempPath, path);
}

export function createCopyTradeHotSnapshotSubscriberStore(
  store: SubscriberStore,
  options: CopyTradeHotSnapshotSubscriberStoreOptions
): SubscriberStore {
  function wrap<K extends MutatingSubscriberMethod>(method: K): SubscriberStore[K] {
    const original = store[method] as (...args: never[]) => Promise<unknown>;

    return (async (...args: never[]) => {
      const result = await original(...args);
      const changed = typeof result === "number" ? result > 0 : result !== false;

      if (changed) {
        const reason = `subscriber.${method}`;
        try {
          await options.onChange(reason);
        } catch (error) {
          options.onError?.(error instanceof Error ? error : new Error(String(error)), reason);
        }
      }

      return result;
    }) as SubscriberStore[K];
  }

  return {
    ...store,
    add: wrap("add"),
    remove: wrap("remove"),
    setMode: wrap("setMode"),
    setNotificationsPaused: wrap("setNotificationsPaused"),
    watchWallet: wrap("watchWallet"),
    renameWallet: wrap("renameWallet"),
    unwatchWallet: wrap("unwatchWallet"),
    watchCopyTradeWallet: wrap("watchCopyTradeWallet"),
    renameCopyTradeWallet: wrap("renameCopyTradeWallet"),
    setCopyTradeWalletEnabled: wrap("setCopyTradeWalletEnabled"),
    unwatchCopyTradeWallet: wrap("unwatchCopyTradeWallet"),
    unwatchAllCopyTradeWallets: wrap("unwatchAllCopyTradeWallets"),
    setCopyTradeWalletTrailingSellConfig: wrap("setCopyTradeWalletTrailingSellConfig"),
    setTradingWallet: wrap("setTradingWallet"),
    renameTradingWallet: wrap("renameTradingWallet"),
    setActiveTradingWallet: wrap("setActiveTradingWallet"),
    removeTradingWallet: wrap("removeTradingWallet"),
    removeAllTradingWallets: wrap("removeAllTradingWallets"),
    setCopyWallet: wrap("setCopyWallet"),
    removeCopyWallet: wrap("removeCopyWallet"),
    setCopyAmountSol: wrap("setCopyAmountSol"),
    setCopyTradeBuySlippage: wrap("setCopyTradeBuySlippage"),
    setCopyTradeBuyPriorityFee: wrap("setCopyTradeBuyPriorityFee"),
    setCopyTradeSellSlippage: wrap("setCopyTradeSellSlippage"),
    setCopyTradeSellPriorityFee: wrap("setCopyTradeSellPriorityFee"),
    setCopyTradeRetryFailedBuys: wrap("setCopyTradeRetryFailedBuys"),
    setCopyTradeBuyPressureSellEnabled: wrap("setCopyTradeBuyPressureSellEnabled"),
    setCopyTradeBuyPressureSellTimeoutMs: wrap("setCopyTradeBuyPressureSellTimeoutMs"),
    setCashbackPayoutWallet: wrap("setCashbackPayoutWallet"),
    resetCopyTradeExecutionSettings: wrap("resetCopyTradeExecutionSettings"),
    setCopyTargetWallet: wrap("setCopyTargetWallet")
  };
}

function snapshotSubscriber({
  subscriber,
  routing,
  dailySpentSolByChatId,
  signerKeypairDir
}: {
  subscriber: SubscriberRecord;
  routing: CopyTradeHotPathSnapshotRouting;
  dailySpentSolByChatId?: DailySpentSolLookup;
  signerKeypairDir?: string | null;
}): CopyTradeHotPathSnapshotSubscriber | null {
  const tradingWallet = subscriber.tradingWallet;
  const copyAmountSol = finitePositiveNumber(subscriber.copyAmountSol);
  const dailySpentSol = dailySpentSolForChat({
    chatId: subscriber.chatId,
    dailySpentSolByChatId,
    dailySolCap: routing.dailySolCap
  });

  if (
    subscriber.notificationsPaused === true ||
    !isLocalSolanaTradingWalletReady(tradingWallet) ||
    copyAmountSol === null ||
    dailySpentSol === null
  ) {
    return null;
  }

  const defaultTrailingSell = snapshotTrailingSellConfig(routing.defaultTrailingSell);
  const wallets = (subscriber.copyTradeWallets || [])
    .filter(isSnapshotCopyTradeWalletEnabled)
    .map((wallet) => ({
      address: wallet.address,
      label: wallet.label || null,
      trailingSell: snapshotTrailingSellConfig(wallet.trailingSellConfig) ?? defaultTrailingSell
    }))
    .sort((a, b) => a.address.localeCompare(b.address));

  if (wallets.length === 0 || capExceeded(wallets.length, routing.maxCopyWalletsPerChat)) {
    return null;
  }

  const buySlippage = nullableFiniteNumber(subscriber.copyTradeBuySlippagePercent);
  const buyPriorityFee = nullableFiniteNumber(subscriber.copyTradeBuyPriorityFeeSol);
  const sellSlippage = nullableFiniteNumber(subscriber.copyTradeSellSlippagePercent);
  const sellPriorityFee = nullableFiniteNumber(subscriber.copyTradeSellPriorityFeeSol);
  const effectiveBuySlippage = buySlippage ?? routing.defaultSlippage;
  const effectiveBuyPriorityFee = buyPriorityFee ?? routing.defaultPriorityFee;
  const effectiveSellSlippage = sellSlippage ?? routing.defaultSlippage;
  const effectiveSellPriorityFee = sellPriorityFee ?? routing.defaultPriorityFee;

  if (
    capExceeded(copyAmountSol, routing.maxBuySol) ||
    capExceeded(dailySpentSol + copyAmountSol, routing.dailySolCap) ||
    capExceeded(effectiveBuySlippage, routing.maxSlippage) ||
    capExceeded(effectiveBuyPriorityFee, routing.maxPriorityFee) ||
    capExceeded(effectiveSellSlippage, routing.maxSlippage) ||
    capExceeded(effectiveSellPriorityFee, routing.maxPriorityFee)
  ) {
    return null;
  }

  return {
    chatId: subscriber.chatId,
    tradingWalletPublicKey: tradingWallet.publicKey,
    signerKeypairPath: signerKeypairPathForWallet(signerKeypairDir, tradingWallet.publicKey),
    copyAmountSol,
    retryFailedBuys: subscriber.copyTradeRetryFailedBuys === true,
    buySlippage,
    buyPriorityFee,
    sellSlippage,
    sellPriorityFee,
    effectiveSellSlippage,
    effectiveSellPriorityFee,
    buyPressureSellEnabled: subscriber.copyTradeBuyPressureSellEnabled === true,
    buyPressureSellTimeoutMs: nullableFiniteNumber(subscriber.copyTradeBuyPressureSellTimeoutMs),
    dailySpentSol,
    wallets
  };
}

function signerKeypairPathForWallet(dir: string | null | undefined, publicKey: string): string | null {
  const normalizedDir = dir?.trim().replace(/\/+$/, "");
  return normalizedDir ? `${normalizedDir}/${publicKey}.json` : null;
}

function isLocalSolanaTradingWalletReady(wallet: TradingWallet | null): wallet is TradingWallet {
  return Boolean(
    wallet?.publicKey &&
    wallet.provider === "local-solana" &&
    wallet.encryptedSecretKey
  );
}

function isSnapshotCopyTradeWalletEnabled(wallet: WatchedWallet): boolean {
  return wallet.copyTradeEnabled !== false;
}

function finitePositiveNumber(value: number | null | undefined): number | null {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : null;
}

function nullableFiniteNumber(value: number | null | undefined): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function snapshotTrailingSellConfig(
  config: TrailingSellConfig | CopyTradeHotPathSnapshotTrailingSellConfig | null | undefined
): CopyTradeHotPathSnapshotTrailingSellConfig | null {
  if (!config) {
    return null;
  }

  const mode = config.mode === "formula" ? "formula" : "custom_steps";
  const percentBasis = config.percentBasis === "original_position" ? "original_position" : "remaining_balance";
  const steps = Array.isArray(config.steps)
    ? config.steps
        .map((step) => {
          const delayMs = Math.floor(Number(step.delayMs));
          const percent = Number(step.percent);
          return Number.isFinite(delayMs) &&
            delayMs >= 0 &&
            Number.isFinite(percent) &&
            percent > 0 &&
            percent <= 100
            ? { delayMs, percent }
            : null;
        })
        .filter((step): step is CopyTradeHotPathSnapshotTrailingSellStep => Boolean(step))
    : [];

  if (steps.length === 0) {
    return null;
  }

  return {
    enabled: config.enabled !== false,
    mode,
    percentBasis,
    steps
  };
}

function capExceeded(value: number, cap: number): boolean {
  return Number.isFinite(cap) && cap > 0 && value > cap;
}

function dailySpentSolForChat({
  chatId,
  dailySpentSolByChatId,
  dailySolCap
}: {
  chatId: string;
  dailySpentSolByChatId?: DailySpentSolLookup;
  dailySolCap: number;
}): number | null {
  if (!Number.isFinite(dailySolCap) || dailySolCap <= 0) {
    return 0;
  }

  const value = dailySpentSolByChatId instanceof Map
    ? dailySpentSolByChatId.get(chatId)
    : dailySpentSolByChatId
      ? (dailySpentSolByChatId as Record<string, number>)[chatId]
      : undefined;

  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;
}

function stableJsonStringify(value: unknown): string {
  if (value === null || typeof value !== "object") {
    return JSON.stringify(value);
  }

  if (Array.isArray(value)) {
    return `[${value.map(stableJsonStringify).join(",")}]`;
  }

  const record = value as Record<string, unknown>;
  return `{${Object.keys(record)
    .sort()
    .filter((key) => record[key] !== undefined)
    .map((key) => `${JSON.stringify(key)}:${stableJsonStringify(record[key])}`)
    .join(",")}}`;
}
