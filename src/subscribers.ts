import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { asRecord, stringValue } from "./types.js";
import type {
  AlertModeValue,
  SubscriberRecord,
  SubscriberStore,
  TelegramChatId,
  TradingWallet,
  TrailingSellConfig,
  TrailingSellPercentBasis,
  WatchedWallet
} from "./types.js";

export const LEGACY_MODE: AlertModeValue = "migrations";

export function normalizeChatId(chatId: TelegramChatId | undefined | null): string | null {
  return chatId === undefined || chatId === null ? null : String(chatId);
}

export function normalizeMode(value: unknown): AlertModeValue | null {
  return value === "migrations" || value === "newtokens" || value === "both" ? value : null;
}

function normalizeTrailingSellPercentBasis(value: unknown): TrailingSellPercentBasis {
  return value === "original_position" ? "original_position" : "remaining_balance";
}

export function normalizeTrailingSellConfig(value: unknown, fallbackNow = new Date().toISOString()): TrailingSellConfig | null {
  const record = asRecord(value);
  const rawSteps = Array.isArray(record.steps) ? record.steps : [];
  const steps = rawSteps
    .map((step) => {
      const stepRecord = asRecord(step);
      const delayMs = Number(stepRecord.delayMs ?? stepRecord.delay_ms);
      const percent = Number(stepRecord.percent);

      if (!Number.isFinite(delayMs) || delayMs < 0 || !Number.isFinite(percent) || percent <= 0 || percent > 100) {
        return null;
      }

      return {
        delayMs: Math.floor(delayMs),
        percent
      };
    })
    .filter((step): step is { delayMs: number; percent: number } => Boolean(step))
    .slice(0, 20);

  if (steps.length === 0 && value !== null && value !== undefined) {
    return null;
  }

  if (value === null || value === undefined) {
    return null;
  }

  return {
    enabled: record.enabled !== false,
    mode: record.mode === "formula" ? "formula" : "custom_steps",
    percentBasis: normalizeTrailingSellPercentBasis(record.percentBasis ?? record.percent_basis),
    steps,
    updatedAt: typeof record.updatedAt === "string" ? record.updatedAt : typeof record.updated_at === "string" ? record.updated_at : fallbackNow
  };
}

export function makeSubscriber(chatId: string, mode: AlertModeValue | null, now = new Date().toISOString()): SubscriberRecord {
  return {
    chatId,
    mode,
    notificationsPaused: false,
    watchedWallets: [],
    copyTradeWallets: [],
    tradingWallet: null,
    tradingWallets: [],
    copyWalletAddress: null,
    copyWalletAddresses: [],
    copyAmountSol: null,
    copyTradeBuySlippagePercent: null,
    copyTradeBuyPriorityFeeSol: null,
    copyTradeSellSlippagePercent: null,
    copyTradeSellPriorityFeeSol: null,
    copyTradeRetryFailedBuys: false,
    copyTradeBuyPressureSellEnabled: false,
    copyTradeBuyPressureSellTimeoutMs: null,
    cashbackPayoutWalletAddress: null,
    copyTargetWalletAddress: null,
    verifiedAt: now,
    updatedAt: now
  };
}

export function finiteNumber(value: unknown): number | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

export function normalizeWatchedWallet(value: unknown, fallbackNow = new Date().toISOString()): WatchedWallet | null {
  const record = asRecord(value);
  const address = stringValue(record.address || record.wallet || record.publicKey)?.trim();

  if (!address) {
    return null;
  }

  const label = stringValue(record.label)?.trim() || null;

  return {
    address,
    label,
    addedAt: typeof record.addedAt === "string" ? record.addedAt : fallbackNow,
    updatedAt: typeof record.updatedAt === "string" ? record.updatedAt : fallbackNow,
    copyTradeEnabled: record.copyTradeEnabled === false || record.copy_trade_enabled === false ? false : true,
    trailingSellConfig: normalizeTrailingSellConfig(record.trailingSellConfig ?? record.trailing_sell_config, fallbackNow)
  };
}

export function normalizeCopyWallet(value: unknown): string | null {
  const record = asRecord(value);
  const address =
    typeof value === "string"
      ? value.trim()
      : stringValue(record.address || record.wallet || record.publicKey || record.copyWallet || record.copyWalletAddress)?.trim();

  return address || null;
}

export function dedupeCopyWallets(copyWallets: string[]): string[] {
  return [...new Set(copyWallets.map((wallet) => wallet.trim()).filter(Boolean))];
}

export function normalizeCopyWallets(copyWallets: unknown, legacyCopyWallet?: unknown): string[] {
  const normalized = Array.isArray(copyWallets)
    ? copyWallets.map((wallet) => normalizeCopyWallet(wallet)).filter((wallet): wallet is string => Boolean(wallet))
    : [];
  const legacy = normalizeCopyWallet(legacyCopyWallet);

  return dedupeCopyWallets(legacy ? [...normalized, legacy] : normalized);
}

export function mergeSubscriber(
  subscribers: Map<string, SubscriberRecord>,
  chatId: string,
  mode: AlertModeValue | null,
  verifiedAt?: unknown,
  updatedAt?: unknown
): void {
  const now = new Date().toISOString();
  const existing = subscribers.get(chatId);

  subscribers.set(chatId, {
    chatId,
    mode,
    notificationsPaused: existing?.notificationsPaused === true,
    watchedWallets: existing?.watchedWallets || [],
    copyTradeWallets: existing?.copyTradeWallets || [],
    tradingWallet: existing?.tradingWallet || null,
    tradingWallets: existing?.tradingWallets || (existing?.tradingWallet ? [existing.tradingWallet] : []),
    copyWalletAddress: stringValue(existing?.copyWalletAddress) || null,
    copyWalletAddresses: existing?.copyWalletAddresses || [],
    copyAmountSol: finiteNumber(existing?.copyAmountSol),
    copyTradeBuySlippagePercent: finiteNumber(existing?.copyTradeBuySlippagePercent),
    copyTradeBuyPriorityFeeSol: finiteNumber(existing?.copyTradeBuyPriorityFeeSol),
    copyTradeSellSlippagePercent: finiteNumber(existing?.copyTradeSellSlippagePercent),
    copyTradeSellPriorityFeeSol: finiteNumber(existing?.copyTradeSellPriorityFeeSol),
    copyTradeRetryFailedBuys: existing?.copyTradeRetryFailedBuys === true,
    copyTradeBuyPressureSellEnabled: existing?.copyTradeBuyPressureSellEnabled === true,
    copyTradeBuyPressureSellTimeoutMs: finiteNumber(existing?.copyTradeBuyPressureSellTimeoutMs),
    cashbackPayoutWalletAddress: stringValue(existing?.cashbackPayoutWalletAddress) || null,
    copyTargetWalletAddress: stringValue(existing?.copyTargetWalletAddress) || null,
    verifiedAt: typeof verifiedAt === "string" ? verifiedAt : existing?.verifiedAt || now,
    updatedAt: typeof updatedAt === "string" ? updatedAt : existing?.updatedAt || now
  });
}

export function normalizeTradingWallet(value: unknown, fallbackNow = new Date().toISOString()): TradingWallet | null {
  const record = asRecord(value);
  const publicKey = stringValue(record.publicKey || record.public_key || record.wallet || record.address)?.trim();
  const encryptedApiKey = stringValue(record.encryptedApiKey || record.encrypted_api_key)?.trim();
  const encryptedSecretKey = stringValue(record.encryptedSecretKey || record.encrypted_secret_key)?.trim();
  const provider = record.provider === "local-solana" || (!record.provider && encryptedSecretKey && !encryptedApiKey)
    ? "local-solana"
    : "pumpportal-lightning";
  const kind = record.kind === "local-solana" || record.kind === "pumpportal-lightning" ? record.kind : provider;
  const secretKeyFormat =
    record.secretKeyFormat === "base58" || record.secret_key_format === "base58"
      ? "base58"
      : record.secretKeyFormat === "base64" || record.secret_key_format === "base64"
        ? "base64"
        : undefined;

  if (!publicKey || (!encryptedApiKey && !encryptedSecretKey)) {
    return null;
  }

  return {
    publicKey,
    provider,
    kind,
    encryptedApiKey: encryptedApiKey || "",
    apiKeyLast4: stringValue(record.apiKeyLast4 || record.api_key_last4)?.trim() || stringValue(record.keyLast4 || record.key_last4)?.trim() || "****",
    encryptedSecretKey: encryptedSecretKey || undefined,
    secretKeyFormat,
    keyLast4: stringValue(record.keyLast4 || record.key_last4)?.trim() || undefined,
    label: stringValue(record.label || record.nickname)?.trim() || null,
    createdAt: typeof record.createdAt === "string" ? record.createdAt : fallbackNow,
    updatedAt: typeof record.updatedAt === "string" ? record.updatedAt : fallbackNow
  };
}

export function dedupeTradingWallets(tradingWallets: TradingWallet[]): TradingWallet[] {
  const byPublicKey = new Map<string, TradingWallet>();

  for (const wallet of tradingWallets) {
    byPublicKey.set(wallet.publicKey, wallet);
  }

  return [...byPublicKey.values()].sort((left, right) => left.createdAt.localeCompare(right.createdAt));
}

export function dedupeWatchedWallets(watchedWallets: WatchedWallet[]): WatchedWallet[] {
  const byAddress = new Map<string, WatchedWallet>();

  for (const wallet of watchedWallets) {
    byAddress.set(wallet.address, wallet);
  }

  return [...byAddress.values()].sort((left, right) => left.address.localeCompare(right.address));
}

function loadLegacyChatIdsInto(subscribers: Map<string, SubscriberRecord>, chatIds: unknown[]): void {
  for (const chatId of chatIds) {
    const normalized = normalizeChatId(chatId as TelegramChatId);

    if (normalized) {
      mergeSubscriber(subscribers, normalized, LEGACY_MODE);
    }
  }
}

function loadSubscriberRecordInto(subscribers: Map<string, SubscriberRecord>, value: unknown): void {
  const record = asRecord(value);
  const chatId = normalizeChatId(record.chatId as TelegramChatId);

  if (!chatId) {
    return;
  }

  mergeSubscriber(subscribers, chatId, normalizeMode(record.mode), record.verifiedAt, record.updatedAt);
  const existing = subscribers.get(chatId);
  const watchedWallets = Array.isArray(record.watchedWallets)
    ? record.watchedWallets.map((wallet) => normalizeWatchedWallet(wallet)).filter((wallet): wallet is WatchedWallet => Boolean(wallet))
    : [];
  const copyTradeWallets = Array.isArray(record.copyTradeWallets)
    ? record.copyTradeWallets.map((wallet) => normalizeWatchedWallet(wallet)).filter((wallet): wallet is WatchedWallet => Boolean(wallet))
    : [];
  const tradingWallets = Array.isArray(record.tradingWallets)
    ? record.tradingWallets.map((wallet) => normalizeTradingWallet(wallet)).filter((wallet): wallet is TradingWallet => Boolean(wallet))
    : [];
  const tradingWallet = normalizeTradingWallet(record.tradingWallet || record.pumpPortalTradingWallet);
  const nextTradingWallets = dedupeTradingWallets(tradingWallet ? [...tradingWallets, tradingWallet] : tradingWallets);
  const legacyCopyTarget = copyTradeWallets.length === 0
    ? stringValue(record.copyTargetWalletAddress || record.copyTargetWallet)?.trim() || null
    : null;
  const legacyCopyTargetWallet = legacyCopyTarget
    ? watchedWallets.find((wallet) => wallet.address === legacyCopyTarget) || {
        address: legacyCopyTarget,
        label: null,
        addedAt: typeof record.updatedAt === "string" ? record.updatedAt : new Date().toISOString(),
        updatedAt: typeof record.updatedAt === "string" ? record.updatedAt : new Date().toISOString()
      }
    : null;
  const nextCopyTradeWallets = legacyCopyTargetWallet ? dedupeWatchedWallets([...copyTradeWallets, legacyCopyTargetWallet]) : copyTradeWallets;
  const nextWatchedWallets = legacyCopyTarget
    ? watchedWallets.filter((wallet) => wallet.address !== legacyCopyTarget)
    : watchedWallets;
  const notificationsPaused = record.notificationsPaused === true || record.notifications_paused === true;

  if (existing) {
    subscribers.set(chatId, {
      ...existing,
      notificationsPaused,
      watchedWallets: nextWatchedWallets.length > 0 || watchedWallets.length > 0 ? dedupeWatchedWallets(nextWatchedWallets) : existing.watchedWallets,
      copyTradeWallets: nextCopyTradeWallets.length > 0 ? dedupeWatchedWallets(nextCopyTradeWallets) : existing.copyTradeWallets,
      tradingWallet: tradingWallet || existing.tradingWallet || nextTradingWallets[0] || null,
      tradingWallets: nextTradingWallets.length > 0 ? nextTradingWallets : existing.tradingWallets,
      copyWalletAddresses: [],
      copyWalletAddress: null,
      copyAmountSol: finiteNumber(record.copyAmountSol ?? record.copyAmount) ?? existing.copyAmountSol,
      copyTradeBuySlippagePercent: finiteNumber(record.copyTradeBuySlippagePercent ?? record.copy_trade_buy_slippage_percent) ?? existing.copyTradeBuySlippagePercent,
      copyTradeBuyPriorityFeeSol: finiteNumber(record.copyTradeBuyPriorityFeeSol ?? record.copy_trade_buy_priority_fee_sol) ?? existing.copyTradeBuyPriorityFeeSol,
      copyTradeSellSlippagePercent: finiteNumber(record.copyTradeSellSlippagePercent ?? record.copy_trade_sell_slippage_percent) ?? existing.copyTradeSellSlippagePercent,
      copyTradeSellPriorityFeeSol: finiteNumber(record.copyTradeSellPriorityFeeSol ?? record.copy_trade_sell_priority_fee_sol) ?? existing.copyTradeSellPriorityFeeSol,
      copyTradeRetryFailedBuys: record.copyTradeRetryFailedBuys === true || record.copy_trade_retry_failed_buys === true,
      copyTradeBuyPressureSellEnabled:
        record.copyTradeBuyPressureSellEnabled === true || record.copy_trade_buy_pressure_sell_enabled === true,
      copyTradeBuyPressureSellTimeoutMs:
        finiteNumber(record.copyTradeBuyPressureSellTimeoutMs ?? record.copy_trade_buy_pressure_sell_timeout_ms) ??
          existing.copyTradeBuyPressureSellTimeoutMs,
      cashbackPayoutWalletAddress:
        stringValue(record.cashbackPayoutWalletAddress ?? record.cashback_payout_wallet_address)?.trim() ||
          existing.cashbackPayoutWalletAddress,
      copyTargetWalletAddress: legacyCopyTarget || existing.copyTargetWalletAddress
    });
  }
}

export function loadSubscriberDataInto(subscribers: Map<string, SubscriberRecord>, data: unknown): void {
  const record = asRecord(data);

  if (Array.isArray(data)) {
    loadLegacyChatIdsInto(subscribers, data);
    return;
  }

  if (Array.isArray(record.chatIds)) {
    loadLegacyChatIdsInto(subscribers, record.chatIds);
  }

  if (Array.isArray(record.subscribers)) {
    for (const entry of record.subscribers) {
      loadSubscriberRecordInto(subscribers, entry);
    }
  }
}

export function seedSubscriberMap(initialChatIds: Array<TelegramChatId | undefined> = []): Map<string, SubscriberRecord> {
  const subscribers = new Map<string, SubscriberRecord>();

  for (const chatId of initialChatIds) {
    const normalized = normalizeChatId(chatId);

    if (normalized) {
      mergeSubscriber(subscribers, normalized, LEGACY_MODE);
    }
  }

  return subscribers;
}

export async function readSubscriberRecords({
  path,
  initialChatIds = []
}: {
  path?: string;
  initialChatIds?: Array<TelegramChatId | undefined>;
}): Promise<SubscriberRecord[]> {
  const subscribers = seedSubscriberMap(initialChatIds);

  if (!path) {
    return [...subscribers.values()];
  }

  try {
    const body = await readFile(path, "utf8");
    loadSubscriberDataInto(subscribers, JSON.parse(body) as unknown);
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") {
      return [...subscribers.values()];
    }

    const message = error instanceof Error ? error.message : String(error);
    console.warn(`Could not load Telegram subscribers: ${message}`);
  }

  return [...subscribers.values()];
}

export function createSubscriberStore({
  path,
  initialChatIds = []
}: {
  path?: string;
  initialChatIds?: Array<TelegramChatId | undefined>;
}): SubscriberStore {
  const subscribers = seedSubscriberMap(initialChatIds);
  let loaded = false;

  async function load(): Promise<void> {
    if (loaded) {
      return;
    }

    loaded = true;

    if (!path) {
      return;
    }

    try {
      const body = await readFile(path, "utf8");
      loadSubscriberDataInto(subscribers, JSON.parse(body) as unknown);
    } catch (error) {
      if (error instanceof Error && "code" in error && error.code === "ENOENT") {
        return;
      }

      const message = error instanceof Error ? error.message : String(error);
      console.warn(`Could not load Telegram subscribers: ${message}`);
    }
  }

  async function save(): Promise<void> {
    if (!path) {
      return;
    }

    await mkdir(dirname(path), { recursive: true });
    await writeFile(
      path,
      `${JSON.stringify(
        {
          subscribers: [...subscribers.values()].sort((left, right) => left.chatId.localeCompare(right.chatId)),
          updatedAt: new Date().toISOString()
        },
        null,
        2
      )}\n`
    );
  }

  return {
    async init() {
      await load();
    },
    has(chatId) {
      return subscribers.has(String(chatId));
    },
    async add(chatId) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized) {
        return;
      }

      const existing = subscribers.get(normalized);

      if (existing) {
        subscribers.set(normalized, {
          ...existing,
          updatedAt: new Date().toISOString()
        });
      } else {
        subscribers.set(normalized, makeSubscriber(normalized, null));
      }

      await save();
    },
    async remove(chatId) {
      await load();
      subscribers.delete(String(chatId));
      await save();
    },
    get(chatId) {
      return subscribers.get(String(chatId)) || null;
    },
    async setMode(chatId, mode) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized);
      subscribers.set(normalized, {
        ...(existing || makeSubscriber(normalized, mode)),
        mode,
        notificationsPaused: false,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async setNotificationsPaused(chatId, paused) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      subscribers.set(normalized, {
        ...existing,
        notificationsPaused: paused,
        mode: paused ? null : existing.mode,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async watchWallet(chatId, address, label) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const now = new Date().toISOString();
      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const watchedWallets = existing.watchedWallets.filter((wallet) => wallet.address !== address);
      const previous = existing.watchedWallets.find((wallet) => wallet.address === address);
      const nextLabel = label?.trim() || previous?.label || null;

      watchedWallets.push({
        address,
        label: nextLabel,
        addedAt: previous?.addedAt || now,
        updatedAt: now
      });

      subscribers.set(normalized, {
        ...existing,
        watchedWallets: dedupeWatchedWallets(watchedWallets),
        updatedAt: now
      });
      await save();
      return true;
    },
    async renameWallet(chatId, address, label) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const walletIndex = existing.watchedWallets.findIndex((wallet) => wallet.address === address);

      if (walletIndex === -1) {
        return false;
      }

      const now = new Date().toISOString();
      const watchedWallets = existing.watchedWallets.map((wallet, index) =>
        index === walletIndex
          ? {
              ...wallet,
              label: label?.trim() || null,
              updatedAt: now
            }
          : wallet
      );

      subscribers.set(normalized, {
        ...existing,
        watchedWallets: dedupeWatchedWallets(watchedWallets),
        updatedAt: now
      });
      await save();
      return true;
    },
    async unwatchWallet(chatId, address) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const watchedWallets = existing.watchedWallets.filter((wallet) => wallet.address !== address);

      subscribers.set(normalized, {
        ...existing,
        watchedWallets,
        copyTargetWalletAddress: existing.copyTargetWalletAddress === address ? null : existing.copyTargetWalletAddress,
        updatedAt: new Date().toISOString()
      });
      await save();
      return watchedWallets.length !== existing.watchedWallets.length;
    },
    async watchCopyTradeWallet(chatId, address, label) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const now = new Date().toISOString();
      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const copyTradeWallets = existing.copyTradeWallets.filter((wallet) => wallet.address !== address);
      const previous = existing.copyTradeWallets.find((wallet) => wallet.address === address);
      const nextLabel = label?.trim() || previous?.label || null;

      copyTradeWallets.push({
        address,
        label: nextLabel,
        addedAt: previous?.addedAt || now,
        updatedAt: now,
        copyTradeEnabled: previous?.copyTradeEnabled === false ? false : true,
        trailingSellConfig: previous?.trailingSellConfig || null
      });

      subscribers.set(normalized, {
        ...existing,
        copyTradeWallets: dedupeWatchedWallets(copyTradeWallets),
        copyTargetWalletAddress: address,
        updatedAt: now
      });
      await save();
      return true;
    },
    async renameCopyTradeWallet(chatId, address, label) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const walletIndex = existing.copyTradeWallets.findIndex((wallet) => wallet.address === address);

      if (walletIndex === -1) {
        return false;
      }

      const now = new Date().toISOString();
      const copyTradeWallets = existing.copyTradeWallets.map((wallet, index) =>
        index === walletIndex
          ? {
              ...wallet,
              label: label?.trim() || null,
              updatedAt: now
            }
          : wallet
      );

      subscribers.set(normalized, {
        ...existing,
        copyTradeWallets: dedupeWatchedWallets(copyTradeWallets),
        updatedAt: now
      });
      await save();
      return true;
    },
    async setCopyTradeWalletEnabled(chatId, address, enabled) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const walletIndex = existing.copyTradeWallets.findIndex((wallet) => wallet.address === address);

      if (walletIndex === -1) {
        return false;
      }

      const now = new Date().toISOString();
      const copyTradeWallets = existing.copyTradeWallets.map((wallet, index) =>
        index === walletIndex
          ? {
              ...wallet,
              copyTradeEnabled: enabled,
              updatedAt: now
            }
          : wallet
      );

      subscribers.set(normalized, {
        ...existing,
        copyTradeWallets: dedupeWatchedWallets(copyTradeWallets),
        updatedAt: now
      });
      await save();
      return true;
    },
    async unwatchCopyTradeWallet(chatId, address) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const copyTradeWallets = existing.copyTradeWallets.filter((wallet) => wallet.address !== address);

      subscribers.set(normalized, {
        ...existing,
        copyTradeWallets,
        copyTargetWalletAddress: existing.copyTargetWalletAddress === address ? null : existing.copyTargetWalletAddress,
        updatedAt: new Date().toISOString()
      });
      await save();
      return copyTradeWallets.length !== existing.copyTradeWallets.length;
    },
    async unwatchAllCopyTradeWallets(chatId) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return 0;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const removedCount = existing.copyTradeWallets.length;

      if (removedCount === 0) {
        return 0;
      }

      subscribers.set(normalized, {
        ...existing,
        copyTradeWallets: [],
        copyTargetWalletAddress: null,
        updatedAt: new Date().toISOString()
      });
      await save();
      return removedCount;
    },
    async setCopyTradeWalletTrailingSellConfig(chatId, address, trailingSellConfig) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const walletIndex = existing.copyTradeWallets.findIndex((wallet) => wallet.address === address);

      if (walletIndex === -1) {
        return false;
      }

      const now = new Date().toISOString();
      const nextConfig = trailingSellConfig ? normalizeTrailingSellConfig({ ...trailingSellConfig, updatedAt: now }, now) : null;
      const copyTradeWallets = existing.copyTradeWallets.map((wallet, index) =>
        index === walletIndex
          ? {
              ...wallet,
              trailingSellConfig: nextConfig,
              updatedAt: now
            }
          : wallet
      );

      subscribers.set(normalized, {
        ...existing,
        copyTradeWallets: dedupeWatchedWallets(copyTradeWallets),
        updatedAt: now
      });
      await save();
      return true;
    },
    async setTradingWallet(chatId, wallet) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const tradingWallets = dedupeTradingWallets([...(existing.tradingWallets || []), wallet]);
      subscribers.set(normalized, {
        ...existing,
        tradingWallet: wallet,
        tradingWallets,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async renameTradingWallet(chatId, label) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);

      if (!existing.tradingWallet) {
        return false;
      }

      const now = new Date().toISOString();
      subscribers.set(normalized, {
        ...existing,
        tradingWallet: {
          ...existing.tradingWallet,
          label,
          updatedAt: now
        },
        tradingWallets: dedupeTradingWallets([...(existing.tradingWallets || []), existing.tradingWallet].filter((wallet): wallet is TradingWallet => Boolean(wallet)).map((wallet) =>
          wallet.publicKey === existing.tradingWallet?.publicKey
            ? {
                ...wallet,
                label,
                updatedAt: now
              }
            : wallet
        )),
        updatedAt: now
      });
      await save();
      return true;
    },
    async setActiveTradingWallet(chatId, publicKey) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const wallet = (existing.tradingWallets || []).find((entry) => entry.publicKey === publicKey);

      if (!wallet) {
        return false;
      }

      subscribers.set(normalized, {
        ...existing,
        tradingWallet: wallet,
        tradingWallets: dedupeTradingWallets(existing.tradingWallets || [wallet]),
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async removeTradingWallet(chatId, publicKey) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const tradingWallets = dedupeTradingWallets(existing.tradingWallets?.length ? existing.tradingWallets : existing.tradingWallet ? [existing.tradingWallet] : []);
      const nextWallets = tradingWallets.filter((wallet) => wallet.publicKey !== publicKey);

      if (nextWallets.length === tradingWallets.length) {
        return false;
      }

      const activeWallet = existing.tradingWallet && existing.tradingWallet.publicKey !== publicKey
        ? nextWallets.find((wallet) => wallet.publicKey === existing.tradingWallet?.publicKey) || nextWallets[0] || null
        : nextWallets[0] || null;

      subscribers.set(normalized, {
        ...existing,
        tradingWallet: activeWallet,
        tradingWallets: nextWallets,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async removeAllTradingWallets(chatId) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return 0;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const tradingWallets = dedupeTradingWallets(existing.tradingWallets?.length ? existing.tradingWallets : existing.tradingWallet ? [existing.tradingWallet] : []);

      if (tradingWallets.length === 0) {
        return 0;
      }

      subscribers.set(normalized, {
        ...existing,
        tradingWallet: null,
        tradingWallets: [],
        updatedAt: new Date().toISOString()
      });
      await save();
      return tradingWallets.length;
    },
    getTradingWallet(chatId) {
      return subscribers.get(String(chatId))?.tradingWallet || null;
    },
    listTradingWallets(chatId) {
      const subscriber = subscribers.get(String(chatId));
      return subscriber?.tradingWallets?.length ? [...subscriber.tradingWallets] : subscriber?.tradingWallet ? [subscriber.tradingWallet] : [];
    },
    async setCopyWallet(chatId, address) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const copyWalletAddresses = dedupeCopyWallets([...(existing.copyWalletAddresses || []), existing.copyWalletAddress || "", address]);

      subscribers.set(normalized, {
        ...existing,
        copyWalletAddress: copyWalletAddresses[0] || null,
        copyWalletAddresses,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async removeCopyWallet(chatId, address) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const currentCopyWallets = existing.copyWalletAddresses.length > 0
        ? existing.copyWalletAddresses
        : existing.copyWalletAddress
          ? [existing.copyWalletAddress]
          : [];
      const copyWalletAddresses = currentCopyWallets.filter((wallet) => wallet !== address);

      subscribers.set(normalized, {
        ...existing,
        copyWalletAddress: copyWalletAddresses[0] || null,
        copyWalletAddresses,
        updatedAt: new Date().toISOString()
      });
      await save();
      return copyWalletAddresses.length !== currentCopyWallets.length;
    },
    async setCopyAmountSol(chatId, amountSol) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      subscribers.set(normalized, {
        ...existing,
        copyAmountSol: amountSol,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async setCopyTradeBuySlippage(chatId, percent) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      subscribers.set(normalized, {
        ...existing,
        copyTradeBuySlippagePercent: percent,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async setCopyTradeBuyPriorityFee(chatId, sol) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      subscribers.set(normalized, {
        ...existing,
        copyTradeBuyPriorityFeeSol: sol,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async setCopyTradeSellSlippage(chatId, percent) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      subscribers.set(normalized, {
        ...existing,
        copyTradeSellSlippagePercent: percent,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async setCopyTradeSellPriorityFee(chatId, sol) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      subscribers.set(normalized, {
        ...existing,
        copyTradeSellPriorityFeeSol: sol,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async setCopyTradeRetryFailedBuys(chatId, enabled) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      subscribers.set(normalized, {
        ...existing,
        copyTradeRetryFailedBuys: enabled,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async setCopyTradeBuyPressureSellEnabled(chatId, enabled) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      subscribers.set(normalized, {
        ...existing,
        copyTradeBuyPressureSellEnabled: enabled,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async setCopyTradeBuyPressureSellTimeoutMs(chatId, timeoutMs) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      subscribers.set(normalized, {
        ...existing,
        copyTradeBuyPressureSellTimeoutMs: timeoutMs,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async setCashbackPayoutWallet(chatId, address) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      subscribers.set(normalized, {
        ...existing,
        cashbackPayoutWalletAddress: address,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async resetCopyTradeExecutionSettings(chatId) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      subscribers.set(normalized, {
        ...existing,
        copyTradeBuySlippagePercent: null,
        copyTradeBuyPriorityFeeSol: null,
        copyTradeSellSlippagePercent: null,
        copyTradeSellPriorityFeeSol: null,
        copyTradeRetryFailedBuys: false,
        copyTradeBuyPressureSellEnabled: false,
        copyTradeBuyPressureSellTimeoutMs: null,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    async setCopyTargetWallet(chatId, address) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);

      if (address) {
        const previous = existing.copyTradeWallets.find((wallet) => wallet.address === address);
        const now = new Date().toISOString();
        subscribers.set(normalized, {
          ...existing,
          copyTradeWallets: previous
            ? existing.copyTradeWallets
            : dedupeWatchedWallets([
                ...existing.copyTradeWallets,
                {
                  address,
                  label: existing.watchedWallets.find((wallet) => wallet.address === address)?.label || null,
                  addedAt: now,
                  updatedAt: now,
                  copyTradeEnabled: true,
                  trailingSellConfig: null
                }
              ]),
          copyTargetWalletAddress: address,
          updatedAt: now
        });
      } else {
        subscribers.set(normalized, {
          ...existing,
          copyTargetWalletAddress: null,
          updatedAt: new Date().toISOString()
        });
      }
      await save();
      return true;
    },
    listWatchedWallets(chatId) {
      return subscribers.get(String(chatId))?.watchedWallets || [];
    },
    listCopyTradeWallets(chatId) {
      return subscribers.get(String(chatId))?.copyTradeWallets || [];
    },
    listCopyWallets(chatId) {
      const subscriber = subscribers.get(String(chatId));

      if (!subscriber) {
        return [];
      }

      return subscriber.copyWalletAddresses.length > 0
        ? subscriber.copyWalletAddresses
        : subscriber.copyWalletAddress
          ? [subscriber.copyWalletAddress]
          : [];
    },
    list() {
      return [...subscribers.values()];
    },
    count() {
      return subscribers.size;
    }
  };
}
