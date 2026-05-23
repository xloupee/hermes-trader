import { createClient } from "@supabase/supabase-js";
import {
  dedupeCopyWallets,
  dedupeWatchedWallets,
  makeSubscriber,
  normalizeChatId,
  normalizeMode,
  normalizeTrailingSellConfig,
  seedSubscriberMap
} from "./subscribers.js";
import type {
  AlertModeValue,
  CopyTradeExecutionRecord,
  SubscriberRecord,
  SubscriberStore,
  TelegramChatId,
  TradingWallet,
  WatchedWallet
} from "./types.js";

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
  telegram_copytrade_wallets?: WatchedWalletRow[] | null;
  telegram_trading_wallets?: TradingWalletRow[] | null;
}

type BaseSubscriberRow = Omit<
  SubscriberRow,
  "telegram_watched_wallets" | "telegram_copytrade_wallets" | "telegram_trading_wallets"
>;

export interface WatchedWalletRow {
  chat_id?: string;
  address: string;
  label: string | null;
  added_at: string;
  updated_at: string;
  trailing_sell_config?: unknown;
}

export interface TradingWalletRow {
  chat_id?: string;
  public_key: string;
  encrypted_api_key: string;
  api_key_last4: string;
  created_at: string;
  updated_at: string;
}

export interface CopyTradeExecutionRow {
  chat_id: string;
  source_wallet_address: string;
  source_wallet_label: string | null;
  trading_wallet_public_key: string;
  mint: string;
  action: string;
  amount: string | number;
  denominated_in_sol: boolean;
  status: string;
  signature: string | null;
  error_text: string | null;
  http_status: number | null;
  observed_trade: unknown;
  request: unknown;
  response: unknown;
  trailing_sell_step_index: number | null;
  trailing_sell_total_steps: number | null;
  created_at?: string;
}

export interface SupabaseSubscriberRepository {
  listSubscribers: () => Promise<SubscriberRecord[]>;
  upsertSubscriber: (subscriber: SubscriberRecord) => Promise<void>;
  deleteSubscriber: (chatId: string) => Promise<void>;
  upsertWatchedWallet: (chatId: string, wallet: WatchedWallet) => Promise<void>;
  deleteWatchedWallet: (chatId: string, address: string) => Promise<void>;
  upsertCopyTradeWallet: (chatId: string, wallet: WatchedWallet) => Promise<void>;
  deleteCopyTradeWallet: (chatId: string, address: string) => Promise<void>;
  upsertTradingWallet: (chatId: string, wallet: TradingWallet) => Promise<void>;
}

export interface SupabaseCopyTradeRecorder {
  recordCopyTradeExecution: (record: CopyTradeExecutionRecord) => Promise<void>;
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
    copy_wallet_address: null,
    copy_wallet_addresses: [],
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

function copyTradeWalletRow(chatId: string, wallet: WatchedWallet): WatchedWalletRow {
  return {
    ...watchedWalletRow(chatId, wallet),
    trailing_sell_config: wallet.trailingSellConfig ?? null
  };
}

function groupRowsByChatId<T extends { chat_id?: string }>(rows: T[]): Map<string, T[]> {
  const byChatId = new Map<string, T[]>();

  for (const row of rows) {
    if (!row.chat_id) {
      continue;
    }

    const entries = byChatId.get(row.chat_id) || [];
    entries.push(row);
    byChatId.set(row.chat_id, entries);
  }

  return byChatId;
}

function tradingWalletRow(chatId: string, wallet: TradingWallet): TradingWalletRow {
  return {
    chat_id: chatId,
    public_key: wallet.publicKey,
    encrypted_api_key: wallet.encryptedApiKey,
    api_key_last4: wallet.apiKeyLast4,
    created_at: wallet.createdAt,
    updated_at: wallet.updatedAt
  };
}

function copyTradeExecutionRow(record: CopyTradeExecutionRecord): CopyTradeExecutionRow {
  const row: CopyTradeExecutionRow = {
    chat_id: record.chatId,
    source_wallet_address: record.sourceWalletAddress,
    source_wallet_label: record.sourceWalletLabel,
    trading_wallet_public_key: record.tradingWalletPublicKey,
    mint: record.mint,
    action: record.action,
    amount: String(record.amount),
    denominated_in_sol: record.denominatedInSol === "true",
    status: record.status,
    signature: record.signature,
    error_text: record.errorText,
    http_status: record.httpStatus,
    observed_trade: record.observedTrade,
    request: record.request,
    response: record.response,
    trailing_sell_step_index: record.trailingSellStepIndex ?? null,
    trailing_sell_total_steps: record.trailingSellTotalSteps ?? null
  };

  if (record.createdAt) {
    row.created_at = record.createdAt;
  }

  return row;
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
  const copyTradeWallets = Array.isArray(row.telegram_copytrade_wallets)
    ? row.telegram_copytrade_wallets.map((wallet) => ({
        address: wallet.address,
        label: wallet.label,
        addedAt: wallet.added_at,
        updatedAt: wallet.updated_at,
        trailingSellConfig: normalizeTrailingSellConfig(wallet.trailing_sell_config, wallet.updated_at)
      }))
    : [];
  const tradingWalletRow = Array.isArray(row.telegram_trading_wallets) ? row.telegram_trading_wallets[0] : null;
  const legacyCopyTargetWallet = row.copy_target_wallet_address && copyTradeWallets.length === 0
    ? watchedWallets.find((wallet) => wallet.address === row.copy_target_wallet_address) || {
        address: row.copy_target_wallet_address,
        label: null,
        addedAt: row.updated_at,
        updatedAt: row.updated_at
      }
    : null;
  const nextCopyTradeWallets = legacyCopyTargetWallet ? dedupeWatchedWallets([...copyTradeWallets, legacyCopyTargetWallet]) : copyTradeWallets;
  const nextWatchedWallets = legacyCopyTargetWallet
    ? watchedWallets.filter((wallet) => wallet.address !== row.copy_target_wallet_address)
    : watchedWallets;

  return {
    chatId: row.chat_id,
    mode: normalizeMode(row.mode),
    watchedWallets: dedupeWatchedWallets(nextWatchedWallets),
    copyTradeWallets: dedupeWatchedWallets(nextCopyTradeWallets),
    tradingWallet: tradingWalletRow
      ? {
          publicKey: tradingWalletRow.public_key,
          encryptedApiKey: tradingWalletRow.encrypted_api_key,
          apiKeyLast4: tradingWalletRow.api_key_last4,
          createdAt: tradingWalletRow.created_at,
          updatedAt: tradingWalletRow.updated_at
        }
      : null,
    copyWalletAddress: null,
    copyWalletAddresses: [],
    copyAmountSol: numericValue(row.copy_amount_sol),
    copyTargetWalletAddress: row.copy_target_wallet_address,
    verifiedAt: row.verified_at,
    updatedAt: row.updated_at
  };
}

export function createSupabaseCopyTradeRecorder({
  url,
  serviceRoleKey
}: {
  url: string;
  serviceRoleKey: string;
}): SupabaseCopyTradeRecorder {
  const client = createClient(url, serviceRoleKey, {
    auth: {
      persistSession: false,
      autoRefreshToken: false
    }
  });

  return {
    async recordCopyTradeExecution(record) {
      const { error } = await client.from("telegram_copytrade_executions").insert(copyTradeExecutionRow(record));
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    }
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
      const { data: subscriberRows, error } = await client
        .from("telegram_subscribers")
        .select("chat_id,mode,copy_wallet_address,copy_wallet_addresses,copy_amount_sol,copy_target_wallet_address,verified_at,updated_at")
        .order("chat_id", { ascending: true });
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }

      const [
        { data: watchedRows, error: watchedError },
        { data: copyTradeRows, error: copyTradeError },
        { data: tradingWalletRows, error: tradingWalletError }
      ] = await Promise.all([
        client.from("telegram_watched_wallets").select("chat_id,address,label,added_at,updated_at"),
        client.from("telegram_copytrade_wallets").select("chat_id,address,label,added_at,updated_at,trailing_sell_config"),
        client.from("telegram_trading_wallets").select("chat_id,public_key,encrypted_api_key,api_key_last4,created_at,updated_at")
      ]);
      const childError =
        formatSupabaseError(watchedError) ||
        formatSupabaseError(copyTradeError) ||
        formatSupabaseError(tradingWalletError);

      if (childError) {
        throw childError;
      }

      const watchedByChatId = groupRowsByChatId((watchedRows || []) as WatchedWalletRow[]);
      const copyTradeByChatId = groupRowsByChatId((copyTradeRows || []) as WatchedWalletRow[]);
      const tradingWalletsByChatId = groupRowsByChatId((tradingWalletRows || []) as TradingWalletRow[]);

      return ((subscriberRows || []) as BaseSubscriberRow[])
        .map((row) => ({
          ...row,
          telegram_watched_wallets: watchedByChatId.get(row.chat_id) || [],
          telegram_copytrade_wallets: copyTradeByChatId.get(row.chat_id) || [],
          telegram_trading_wallets: tradingWalletsByChatId.get(row.chat_id) || []
        }))
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
    },
    async upsertCopyTradeWallet(chatId, wallet) {
      const { error } = await client
        .from("telegram_copytrade_wallets")
        .upsert(copyTradeWalletRow(chatId, wallet), { onConflict: "chat_id,address" });
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    },
    async deleteCopyTradeWallet(chatId, address) {
      const { error } = await client.from("telegram_copytrade_wallets").delete().eq("chat_id", chatId).eq("address", address);
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    },
    async upsertTradingWallet(chatId, wallet) {
      const { error } = await client
        .from("telegram_trading_wallets")
        .upsert(tradingWalletRow(chatId, wallet), { onConflict: "chat_id" });
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
    const storedChatIds = new Set(storedSubscribers.map((subscriber) => subscriber.chatId));

    for (const subscriber of storedSubscribers) {
      const storedCopyTradeWallets = subscriber.copyTradeWallets || [];
      const legacyCopyTargetWallet = subscriber.copyTargetWalletAddress && storedCopyTradeWallets.length === 0
        ? subscriber.watchedWallets.find((wallet) => wallet.address === subscriber.copyTargetWalletAddress) || {
            address: subscriber.copyTargetWalletAddress,
            label: null,
            addedAt: subscriber.updatedAt,
            updatedAt: subscriber.updatedAt
          }
        : null;
      subscribers.set(subscriber.chatId, {
        ...subscriber,
        watchedWallets: dedupeWatchedWallets(
          legacyCopyTargetWallet
            ? subscriber.watchedWallets.filter((wallet) => wallet.address !== subscriber.copyTargetWalletAddress)
            : subscriber.watchedWallets
        ),
        copyTradeWallets: dedupeWatchedWallets(
          legacyCopyTargetWallet ? [...storedCopyTradeWallets, legacyCopyTargetWallet] : storedCopyTradeWallets
        ),
        tradingWallet: subscriber.tradingWallet || null
      });
    }

    for (const subscriber of subscribers.values()) {
      if (!storedChatIds.has(subscriber.chatId)) {
        await repository.upsertSubscriber(subscriber);
      }
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
        updatedAt: now,
        trailingSellConfig: previous?.trailingSellConfig || null
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
      const wallet = {
        address,
        label: label?.trim() || previous?.label || null,
        addedAt: previous?.addedAt || now,
        updatedAt: now
      };
      const next = {
        ...existing,
        copyTradeWallets: dedupeWatchedWallets([...copyTradeWallets, wallet]),
        copyTargetWalletAddress: address,
        updatedAt: now
      };

      await repository.upsertSubscriber(next);
      await repository.upsertCopyTradeWallet(normalized, wallet);
      subscribers.set(normalized, next);
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
      const wallet = {
        ...existing.copyTradeWallets[walletIndex],
        label: label?.trim() || null,
        updatedAt: now
      };
      const copyTradeWallets = existing.copyTradeWallets.map((entry, index) => (index === walletIndex ? wallet : entry));
      const next = {
        ...existing,
        copyTradeWallets: dedupeWatchedWallets(copyTradeWallets),
        updatedAt: now
      };

      await repository.upsertSubscriber(next);
      await repository.upsertCopyTradeWallet(normalized, wallet);
      subscribers.set(normalized, next);
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
      const next = {
        ...existing,
        copyTradeWallets,
        copyTargetWalletAddress: existing.copyTargetWalletAddress === address ? null : existing.copyTargetWalletAddress,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      await repository.deleteCopyTradeWallet(normalized, address);
      subscribers.set(normalized, next);
      return copyTradeWallets.length !== existing.copyTradeWallets.length;
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
      const wallet = {
        ...existing.copyTradeWallets[walletIndex],
        trailingSellConfig: nextConfig,
        updatedAt: now
      };
      const copyTradeWallets = existing.copyTradeWallets.map((entry, index) => (index === walletIndex ? wallet : entry));
      const next = {
        ...existing,
        copyTradeWallets: dedupeWatchedWallets(copyTradeWallets),
        updatedAt: now
      };

      await repository.upsertSubscriber(next);
      await repository.upsertCopyTradeWallet(normalized, wallet);
      subscribers.set(normalized, next);
      return true;
    },
    async setTradingWallet(chatId, wallet) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const next = {
        ...(subscribers.get(normalized) || makeSubscriber(normalized, null)),
        tradingWallet: wallet,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      await repository.upsertTradingWallet(normalized, wallet);
      subscribers.set(normalized, next);
      return true;
    },
    getTradingWallet(chatId) {
      return subscribers.get(String(chatId))?.tradingWallet || null;
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

      if (address) {
        const now = new Date().toISOString();
        const previous = existing.copyTradeWallets.find((wallet) => wallet.address === address);
        const wallet = previous || {
          address,
          label: existing.watchedWallets.find((watchedWallet) => watchedWallet.address === address)?.label || null,
          addedAt: now,
          updatedAt: now,
          trailingSellConfig: null
        };
        const next = {
          ...existing,
          copyTradeWallets: previous ? existing.copyTradeWallets : dedupeWatchedWallets([...existing.copyTradeWallets, wallet]),
          copyTargetWalletAddress: address,
          updatedAt: now
        };

        await repository.upsertSubscriber(next);
        if (!previous) {
          await repository.upsertCopyTradeWallet(normalized, wallet);
        }
        subscribers.set(normalized, next);
        return true;
      }

      const next = {
        ...existing,
        copyTargetWalletAddress: null,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
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

export function createSupabaseCopyTradeRecorderFromEnv({
  url,
  serviceRoleKey
}: {
  url: string;
  serviceRoleKey: string;
}): SupabaseCopyTradeRecorder {
  return createSupabaseCopyTradeRecorder({ url, serviceRoleKey });
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

    for (const wallet of subscriber.copyTradeWallets) {
      await repository.upsertCopyTradeWallet(subscriber.chatId, wallet);
    }

    if (subscriber.tradingWallet) {
      await repository.upsertTradingWallet(subscriber.chatId, subscriber.tradingWallet);
    }
  }
}
