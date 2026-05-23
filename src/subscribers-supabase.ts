import { createClient } from "@supabase/supabase-js";
import {
  dedupeCopyWallets,
  dedupeWatchedWallets,
  makeSubscriber,
  normalizeChatId,
  normalizeMode,
  seedSubscriberMap
} from "./subscribers.js";
import type { AlertModeValue, SubscriberRecord, SubscriberStore, TelegramChatId, WatchedWallet } from "./types.js";

interface SupabaseErrorLike {
  message?: string;
}

export interface SubscriberRow {
  chat_id: string;
  mode: string | null;
  copy_wallet_address: string | null;
  copy_wallet_addresses?: string[] | null;
  copy_amount_sol: string | number | null;
  copy_target_wallet_address: string | null;
  verified_at: string;
  updated_at: string;
  telegram_watched_wallets?: WatchedWalletRow[] | null;
}

export interface WatchedWalletRow {
  chat_id?: string;
  address: string;
  label: string | null;
  added_at: string;
  updated_at: string;
}

export interface SupabaseSubscriberRepository {
  listSubscribers: () => Promise<SubscriberRecord[]>;
  upsertSubscriber: (subscriber: SubscriberRecord) => Promise<void>;
  deleteSubscriber: (chatId: string) => Promise<void>;
  upsertWatchedWallet: (chatId: string, wallet: WatchedWallet) => Promise<void>;
  deleteWatchedWallet: (chatId: string, address: string) => Promise<void>;
}

function formatSupabaseError(error: SupabaseErrorLike | null): Error | null {
  return error ? new Error(error.message || "Supabase subscriber store request failed") : null;
}

function numericValue(value: string | number | null): number | null {
  if (value === null) {
    return null;
  }

  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function subscriberRow(record: SubscriberRecord): Omit<SubscriberRow, "telegram_watched_wallets"> {
  return {
    chat_id: record.chatId,
    mode: record.mode,
    copy_wallet_address: record.copyWalletAddress,
    copy_wallet_addresses: record.copyWalletAddresses,
    copy_amount_sol: record.copyAmountSol,
    copy_target_wallet_address: record.copyTargetWalletAddress,
    verified_at: record.verifiedAt,
    updated_at: record.updatedAt
  };
}

function watchedWalletRow(chatId: string, wallet: WatchedWallet): WatchedWalletRow {
  return {
    chat_id: chatId,
    address: wallet.address,
    label: wallet.label,
    added_at: wallet.addedAt,
    updated_at: wallet.updatedAt
  };
}

export function subscriberFromRow(row: SubscriberRow): SubscriberRecord {
  const watchedWallets = Array.isArray(row.telegram_watched_wallets)
    ? row.telegram_watched_wallets.map((wallet) => ({
        address: wallet.address,
        label: wallet.label,
        addedAt: wallet.added_at,
        updatedAt: wallet.updated_at
      }))
    : [];

  return {
    chatId: row.chat_id,
    mode: normalizeMode(row.mode),
    watchedWallets: dedupeWatchedWallets(watchedWallets),
    copyWalletAddress: row.copy_wallet_address,
    copyWalletAddresses: row.copy_wallet_addresses?.length
      ? row.copy_wallet_addresses
      : row.copy_wallet_address
        ? [row.copy_wallet_address]
        : [],
    copyAmountSol: numericValue(row.copy_amount_sol),
    copyTargetWalletAddress: row.copy_target_wallet_address,
    verifiedAt: row.verified_at,
    updatedAt: row.updated_at
  };
}

export function createSupabaseSubscriberRepository({
  url,
  serviceRoleKey
}: {
  url: string;
  serviceRoleKey: string;
}): SupabaseSubscriberRepository {
  const client = createClient(url, serviceRoleKey, {
    auth: {
      persistSession: false,
      autoRefreshToken: false
    }
  });

  return {
    async listSubscribers() {
      const { data, error } = await client
        .from("telegram_subscribers")
        .select(
          "chat_id,mode,copy_wallet_address,copy_wallet_addresses,copy_amount_sol,copy_target_wallet_address,verified_at,updated_at,telegram_watched_wallets(address,label,added_at,updated_at)"
        )
        .order("chat_id", { ascending: true });
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }

      return ((data || []) as SubscriberRow[])
        .map(subscriberFromRow)
        .sort((left, right) => left.chatId.localeCompare(right.chatId));
    },
    async upsertSubscriber(subscriber) {
      const { error } = await client.from("telegram_subscribers").upsert(subscriberRow(subscriber), { onConflict: "chat_id" });
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    },
    async deleteSubscriber(chatId) {
      const { error } = await client.from("telegram_subscribers").delete().eq("chat_id", chatId);
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    },
    async upsertWatchedWallet(chatId, wallet) {
      const { error } = await client
        .from("telegram_watched_wallets")
        .upsert(watchedWalletRow(chatId, wallet), { onConflict: "chat_id,address" });
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    },
    async deleteWatchedWallet(chatId, address) {
      const { error } = await client.from("telegram_watched_wallets").delete().eq("chat_id", chatId).eq("address", address);
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    }
  };
}

export function createSupabaseSubscriberStore({
  repository,
  initialChatIds = []
}: {
  repository: SupabaseSubscriberRepository;
  initialChatIds?: Array<TelegramChatId | undefined>;
}): SubscriberStore {
  const subscribers = seedSubscriberMap(initialChatIds);
  let loaded = false;

  async function load(): Promise<void> {
    if (loaded) {
      return;
    }

    loaded = true;
    const storedSubscribers = await repository.listSubscribers();

    for (const subscriber of storedSubscribers) {
      subscribers.set(subscriber.chatId, {
        ...subscriber,
        watchedWallets: dedupeWatchedWallets(subscriber.watchedWallets)
      });
    }
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

      const now = new Date().toISOString();
      const next = subscribers.has(normalized)
        ? {
            ...(subscribers.get(normalized) || makeSubscriber(normalized, null, now)),
            updatedAt: now
          }
        : makeSubscriber(normalized, null, now);

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
    },
    async remove(chatId) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized) {
        return;
      }

      await repository.deleteSubscriber(normalized);
      subscribers.delete(normalized);
    },
    get(chatId) {
      return subscribers.get(String(chatId)) || null;
    },
    async setMode(chatId, mode: AlertModeValue | null) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const next = {
        ...(subscribers.get(normalized) || makeSubscriber(normalized, mode)),
        mode,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
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
      const wallet = {
        address,
        label: label?.trim() || previous?.label || null,
        addedAt: previous?.addedAt || now,
        updatedAt: now
      };
      const next = {
        ...existing,
        watchedWallets: dedupeWatchedWallets([...watchedWallets, wallet]),
        updatedAt: now
      };

      await repository.upsertSubscriber(next);
      await repository.upsertWatchedWallet(normalized, wallet);
      subscribers.set(normalized, next);
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
      const wallet = {
        ...existing.watchedWallets[walletIndex],
        label: label?.trim() || null,
        updatedAt: now
      };
      const watchedWallets = existing.watchedWallets.map((entry, index) => (index === walletIndex ? wallet : entry));
      const next = {
        ...existing,
        watchedWallets: dedupeWatchedWallets(watchedWallets),
        updatedAt: now
      };

      await repository.upsertSubscriber(next);
      await repository.upsertWatchedWallet(normalized, wallet);
      subscribers.set(normalized, next);
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
      const next = {
        ...existing,
        watchedWallets,
        copyTargetWalletAddress: existing.copyTargetWalletAddress === address ? null : existing.copyTargetWalletAddress,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      await repository.deleteWatchedWallet(normalized, address);
      subscribers.set(normalized, next);
      return watchedWallets.length !== existing.watchedWallets.length;
    },
    async setCopyWallet(chatId, address) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const copyWalletAddresses = dedupeCopyWallets([...(existing.copyWalletAddresses || []), existing.copyWalletAddress || "", address]);
      const next = {
        ...existing,
        copyWalletAddress: copyWalletAddresses[0] || null,
        copyWalletAddresses,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
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
      const next = {
        ...existing,
        copyWalletAddress: copyWalletAddresses[0] || null,
        copyWalletAddresses,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
      return copyWalletAddresses.length !== currentCopyWallets.length;
    },
    async setCopyAmountSol(chatId, amountSol) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const next = {
        ...(subscribers.get(normalized) || makeSubscriber(normalized, null)),
        copyAmountSol: amountSol,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
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

      const next = {
        ...existing,
        copyTargetWalletAddress: address,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
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

export function createSupabaseSubscriberStoreFromEnv({
  url,
  serviceRoleKey,
  initialChatIds = []
}: {
  url: string;
  serviceRoleKey: string;
  initialChatIds?: Array<TelegramChatId | undefined>;
}): SubscriberStore {
  return createSupabaseSubscriberStore({
    repository: createSupabaseSubscriberRepository({ url, serviceRoleKey }),
    initialChatIds
  });
}

export async function importSubscribersToSupabase({
  repository,
  subscribers: records
}: {
  repository: SupabaseSubscriberRepository;
  subscribers: SubscriberRecord[];
}): Promise<void> {
  for (const subscriber of records) {
    await repository.upsertSubscriber(subscriber);

    for (const wallet of subscriber.watchedWallets) {
      await repository.upsertWatchedWallet(subscriber.chatId, wallet);
    }
  }
}
