import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { asRecord, stringValue } from "./types.js";
import type { AlertModeValue, SubscriberRecord, SubscriberStore, TelegramChatId, WatchedWallet } from "./types.js";

export const LEGACY_MODE: AlertModeValue = "migrations";

export function normalizeChatId(chatId: TelegramChatId | undefined | null): string | null {
  return chatId === undefined || chatId === null ? null : String(chatId);
}

export function normalizeMode(value: unknown): AlertModeValue | null {
  return value === "migrations" || value === "newtokens" || value === "both" ? value : null;
}

export function makeSubscriber(chatId: string, mode: AlertModeValue | null, now = new Date().toISOString()): SubscriberRecord {
  return {
    chatId,
    mode,
    watchedWallets: [],
    copyWalletAddress: null,
    copyAmountSol: null,
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
    updatedAt: typeof record.updatedAt === "string" ? record.updatedAt : fallbackNow
  };
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
    watchedWallets: existing?.watchedWallets || [],
    copyWalletAddress: stringValue(existing?.copyWalletAddress) || null,
    copyAmountSol: finiteNumber(existing?.copyAmountSol),
    copyTargetWalletAddress: stringValue(existing?.copyTargetWalletAddress) || null,
    verifiedAt: typeof verifiedAt === "string" ? verifiedAt : existing?.verifiedAt || now,
    updatedAt: typeof updatedAt === "string" ? updatedAt : existing?.updatedAt || now
  });
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

  if (existing) {
    subscribers.set(chatId, {
      ...existing,
      watchedWallets: watchedWallets.length > 0 ? dedupeWatchedWallets(watchedWallets) : existing.watchedWallets,
      copyWalletAddress: stringValue(record.copyWalletAddress || record.copyWallet || record.copyPublicKey)?.trim() || existing.copyWalletAddress,
      copyAmountSol: finiteNumber(record.copyAmountSol ?? record.copyAmount) ?? existing.copyAmountSol,
      copyTargetWalletAddress: stringValue(record.copyTargetWalletAddress || record.copyTargetWallet)?.trim() || existing.copyTargetWalletAddress
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
    async setCopyWallet(chatId, address) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      subscribers.set(normalized, {
        ...existing,
        copyWalletAddress: address,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
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
    async setCopyTargetWallet(chatId, address) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);

      if (address && !existing.watchedWallets.some((wallet) => wallet.address === address)) {
        return false;
      }

      subscribers.set(normalized, {
        ...existing,
        copyTargetWalletAddress: address,
        updatedAt: new Date().toISOString()
      });
      await save();
      return true;
    },
    listWatchedWallets(chatId) {
      return subscribers.get(String(chatId))?.watchedWallets || [];
    },
    list() {
      return [...subscribers.values()];
    },
    count() {
      return subscribers.size;
    }
  };
}
