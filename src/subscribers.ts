import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { asRecord, stringValue } from "./types.js";
import type { AlertModeValue, SubscriberRecord, SubscriberStore, TelegramChatId, WatchedWallet } from "./types.js";

const LEGACY_MODE: AlertModeValue = "migrations";

function normalizeChatId(chatId: TelegramChatId | undefined | null): string | null {
  return chatId === undefined || chatId === null ? null : String(chatId);
}

function normalizeMode(value: unknown): AlertModeValue | null {
  return value === "migrations" || value === "newtokens" || value === "both" ? value : null;
}

function makeSubscriber(chatId: string, mode: AlertModeValue | null, now = new Date().toISOString()): SubscriberRecord {
  return {
    chatId,
    mode,
    watchedWallets: [],
    copyWallet: null,
    copyWallets: [],
    copySolAmount: null,
    copySettingsUpdatedAt: null,
    verifiedAt: now,
    updatedAt: now
  };
}

function normalizeWatchedWallet(value: unknown, fallbackNow = new Date().toISOString()): WatchedWallet | null {
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

function normalizeCopyWallet(value: unknown): string | null {
  const record = asRecord(value);
  const address =
    typeof value === "string"
      ? value.trim()
      : stringValue(record.address || record.wallet || record.publicKey || record.copyWallet)?.trim();

  return address || null;
}

function dedupeCopyWallets(copyWallets: string[]): string[] {
  return [...new Set(copyWallets.map((wallet) => wallet.trim()).filter(Boolean))];
}

function normalizeCopyWallets(copyWallets: unknown, legacyCopyWallet?: unknown): string[] {
  const normalized = Array.isArray(copyWallets)
    ? copyWallets.map((wallet) => normalizeCopyWallet(wallet)).filter((wallet): wallet is string => Boolean(wallet))
    : [];
  const legacy = normalizeCopyWallet(legacyCopyWallet);

  return dedupeCopyWallets(legacy ? [...normalized, legacy] : normalized);
}

function mergeSubscriber(
  subscribers: Map<string, SubscriberRecord>,
  chatId: string,
  mode: AlertModeValue | null,
  verifiedAt?: unknown,
  updatedAt?: unknown,
  copyWallet?: unknown,
  copyWallets?: unknown,
  copySolAmount?: unknown,
  copySettingsUpdatedAt?: unknown
): void {
  const now = new Date().toISOString();
  const existing = subscribers.get(chatId);
  const normalizedCopySolAmount = typeof copySolAmount === "number" && Number.isFinite(copySolAmount) && copySolAmount > 0 ? copySolAmount : null;
  const normalizedCopyWallets = dedupeCopyWallets([
    ...(existing?.copyWallets || []),
    ...normalizeCopyWallets(copyWallets, copyWallet)
  ]);

  subscribers.set(chatId, {
    chatId,
    mode,
    watchedWallets: existing?.watchedWallets || [],
    copyWallet: normalizedCopyWallets[0] || existing?.copyWallet || null,
    copyWallets: normalizedCopyWallets,
    copySolAmount: normalizedCopySolAmount ?? existing?.copySolAmount ?? null,
    copySettingsUpdatedAt: typeof copySettingsUpdatedAt === "string" ? copySettingsUpdatedAt : existing?.copySettingsUpdatedAt || null,
    verifiedAt: typeof verifiedAt === "string" ? verifiedAt : existing?.verifiedAt || now,
    updatedAt: typeof updatedAt === "string" ? updatedAt : existing?.updatedAt || now
  });
}

export function createSubscriberStore({
  path,
  initialChatIds = []
}: {
  path?: string;
  initialChatIds?: Array<TelegramChatId | undefined>;
}): SubscriberStore {
  const subscribers = new Map<string, SubscriberRecord>();
  let loaded = false;

  for (const chatId of initialChatIds) {
    const normalized = normalizeChatId(chatId);

    if (normalized) {
      mergeSubscriber(subscribers, normalized, LEGACY_MODE);
    }
  }

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
      const data = JSON.parse(body) as unknown;
      const record = asRecord(data);

      if (Array.isArray(data)) {
        loadLegacyChatIds(data);
        return;
      }

      if (Array.isArray(record.chatIds)) {
        loadLegacyChatIds(record.chatIds);
      }

      if (Array.isArray(record.subscribers)) {
        for (const entry of record.subscribers) {
          loadSubscriberRecord(entry);
        }
      }
    } catch (error) {
      if (error instanceof Error && "code" in error && error.code === "ENOENT") {
        return;
      }

      const message = error instanceof Error ? error.message : String(error);
      console.warn(`Could not load Telegram subscribers: ${message}`);
    }
  }

  function loadLegacyChatIds(chatIds: unknown[]): void {
    for (const chatId of chatIds) {
      const normalized = normalizeChatId(chatId as TelegramChatId);

      if (normalized) {
        mergeSubscriber(subscribers, normalized, LEGACY_MODE);
      }
    }
  }

  function loadSubscriberRecord(value: unknown): void {
    const record = asRecord(value);
    const chatId = normalizeChatId(record.chatId as TelegramChatId);

    if (!chatId) {
      return;
    }

    mergeSubscriber(
      subscribers,
      chatId,
      normalizeMode(record.mode),
      record.verifiedAt,
      record.updatedAt,
      record.copyWallet,
      record.copyWallets,
      record.copySolAmount,
      record.copySettingsUpdatedAt
    );
    const existing = subscribers.get(chatId);
    const watchedWallets = Array.isArray(record.watchedWallets)
      ? record.watchedWallets.map((wallet) => normalizeWatchedWallet(wallet)).filter((wallet): wallet is WatchedWallet => Boolean(wallet))
      : [];

    if (existing && watchedWallets.length > 0) {
      subscribers.set(chatId, {
        ...existing,
        watchedWallets: dedupeWatchedWallets(watchedWallets)
      });
    }
  }

  function dedupeWatchedWallets(watchedWallets: WatchedWallet[]): WatchedWallet[] {
    const byAddress = new Map<string, WatchedWallet>();

    for (const wallet of watchedWallets) {
      byAddress.set(wallet.address, wallet);
    }

    return [...byAddress.values()].sort((left, right) => left.address.localeCompare(right.address));
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
      await save();
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

      watchedWallets.push({
        address,
        label: label?.trim() || null,
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

      const now = new Date().toISOString();
      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const copyWallets = dedupeCopyWallets([...(existing.copyWallets || []), address]);

      subscribers.set(normalized, {
        ...existing,
        copyWallet: copyWallets[0] || null,
        copyWallets,
        copySettingsUpdatedAt: now,
        updatedAt: now
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

      const now = new Date().toISOString();
      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const copyWallets = (existing.copyWallets || []).filter((wallet) => wallet !== address);

      subscribers.set(normalized, {
        ...existing,
        copyWallet: copyWallets[0] || null,
        copyWallets,
        copySettingsUpdatedAt: now,
        updatedAt: now
      });
      await save();
      return copyWallets.length !== (existing.copyWallets || []).length;
    },
    async setCopySolAmount(chatId, amount) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const now = new Date().toISOString();
      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);

      subscribers.set(normalized, {
        ...existing,
        copySolAmount: amount,
        copySettingsUpdatedAt: now,
        updatedAt: now
      });
      await save();
      return true;
    },
    listWatchedWallets(chatId) {
      return subscribers.get(String(chatId))?.watchedWallets || [];
    },
    listCopyWallets(chatId) {
      const subscriber = subscribers.get(String(chatId));

      if (!subscriber) {
        return [];
      }

      return subscriber.copyWallets.length > 0 ? subscriber.copyWallets : subscriber.copyWallet ? [subscriber.copyWallet] : [];
    },
    list() {
      return [...subscribers.values()];
    },
    count() {
      return subscribers.size;
    }
  };
}
