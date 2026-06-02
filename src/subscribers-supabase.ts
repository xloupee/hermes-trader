import { createClient } from "@supabase/supabase-js";
import {
  dedupeCopyWallets,
  dedupeTradingWallets,
  dedupeWatchedWallets,
  makeSubscriber,
  normalizeChatId,
  normalizeMode,
  normalizeTradingWallet,
  normalizeTrailingSellConfig,
  seedSubscriberMap
} from "./subscribers.js";
import {
  asRecord,
  type AlertModeValue,
  type CopyTradeExecutionRecord,
  type CopyTradeExecutionStatusUpdate,
  type SubscriberRecord,
  type SubscriberStore,
  type TelegramChatId,
  type TradingWallet,
  type WatchedWallet
} from "./types.js";

interface SupabaseErrorLike {
  message?: string;
}

export interface SubscriberRow {
  chat_id: string;
  mode: string | null;
  notifications_paused?: boolean | null;
  copy_wallet_address: string | null;
  copy_wallet_addresses?: string[] | null;
  copy_amount_sol: string | number | null;
  copy_trade_buy_slippage_percent?: string | number | null;
  copy_trade_buy_priority_fee_sol?: string | number | null;
  copy_trade_sell_slippage_percent?: string | number | null;
  copy_trade_sell_priority_fee_sol?: string | number | null;
  copy_trade_retry_failed_buys?: boolean | null;
  copy_trade_buy_pressure_sell_enabled?: boolean | null;
  copy_trade_buy_pressure_sell_timeout_ms?: string | number | null;
  cashback_payout_wallet_address?: string | null;
  copy_target_wallet_address: string | null;
  active_trading_wallet_public_key?: string | null;
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
  encrypted_api_key: string | null;
  api_key_last4: string;
  provider?: string | null;
  kind?: string | null;
  encrypted_secret_key?: string | null;
  secret_key_format?: string | null;
  key_last4?: string | null;
  label?: string | null;
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
  observed_signature?: string | null;
  target_observed_to_submit_ms?: number | null;
  target_blocktime_to_submit_ms?: number | null;
  build_ms?: number | null;
  send_ms?: number | null;
  winner_provider?: string | null;
  send_rpc_winner?: string | null;
  send_rpc_count?: number | null;
  target_slot?: number | null;
  copy_slot?: number | null;
  slot_delta?: number | null;
  latency_status?: string | null;
  latency_summary?: unknown;
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
  deleteAllCopyTradeWallets: (chatId: string) => Promise<void>;
  upsertTradingWallet: (chatId: string, wallet: TradingWallet) => Promise<void>;
  deleteTradingWallet: (chatId: string, publicKey: string) => Promise<void>;
  deleteAllTradingWallets: (chatId: string) => Promise<void>;
}

export interface SupabaseCopyTradeRecorder {
  recordCopyTradeExecution: (record: CopyTradeExecutionRecord) => Promise<void>;
  updateCopyTradeExecutionStatus?: (update: CopyTradeExecutionStatusUpdate) => Promise<void>;
}

function formatSupabaseError(error: SupabaseErrorLike | null): Error | null {
  return error ? new Error(error.message || "Supabase subscriber store request failed") : null;
}

function isMissingSupabaseColumn(error: SupabaseErrorLike | null): boolean {
  const message = error?.message?.toLowerCase() || "";
  return message.includes("column") && (message.includes("does not exist") || message.includes("schema cache"));
}

function numericValue(value: string | number | null): number | null {
  if (value === null) {
    return null;
  }

  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function subscriberRow(
  record: SubscriberRecord,
  {
    includeRetryFailedBuys = true,
    includeBuyPressureSell = true,
    includeCashbackPayoutWallet = true,
    includeNotificationsPaused = true
  }: {
    includeRetryFailedBuys?: boolean;
    includeBuyPressureSell?: boolean;
    includeCashbackPayoutWallet?: boolean;
    includeNotificationsPaused?: boolean;
  } = {}
): Omit<SubscriberRow, "telegram_watched_wallets"> {
  const row: Omit<SubscriberRow, "telegram_watched_wallets"> = {
    chat_id: record.chatId,
    mode: record.mode,
    notifications_paused: record.notificationsPaused,
    copy_wallet_address: null,
    copy_wallet_addresses: [],
    copy_amount_sol: record.copyAmountSol,
    copy_trade_buy_slippage_percent: record.copyTradeBuySlippagePercent,
    copy_trade_buy_priority_fee_sol: record.copyTradeBuyPriorityFeeSol,
    copy_trade_sell_slippage_percent: record.copyTradeSellSlippagePercent,
    copy_trade_sell_priority_fee_sol: record.copyTradeSellPriorityFeeSol,
    copy_trade_retry_failed_buys: record.copyTradeRetryFailedBuys,
    copy_trade_buy_pressure_sell_enabled: record.copyTradeBuyPressureSellEnabled,
    copy_trade_buy_pressure_sell_timeout_ms: record.copyTradeBuyPressureSellTimeoutMs,
    cashback_payout_wallet_address: record.cashbackPayoutWalletAddress,
    copy_target_wallet_address: record.copyTargetWalletAddress,
    active_trading_wallet_public_key: record.tradingWallet?.publicKey || null,
    verified_at: record.verifiedAt,
    updated_at: record.updatedAt
  };

  if (!includeRetryFailedBuys) {
    delete row.copy_trade_retry_failed_buys;
  }

  if (!includeBuyPressureSell) {
    delete row.copy_trade_buy_pressure_sell_enabled;
    delete row.copy_trade_buy_pressure_sell_timeout_ms;
  }

  if (!includeCashbackPayoutWallet) {
    delete row.cashback_payout_wallet_address;
  }

  if (!includeNotificationsPaused) {
    delete row.notifications_paused;
  }

  return row;
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
    trailing_sell_config: copyTradeWalletTrailingSellConfigValue(wallet)
  };
}

function copyTradeWalletTrailingSellConfigValue(wallet: WatchedWallet): unknown {
  if (wallet.trailingSellConfig) {
    return {
      ...wallet.trailingSellConfig,
      copyTradeEnabled: wallet.copyTradeEnabled !== false
    };
  }

  return wallet.copyTradeEnabled === false ? { copyTradeEnabled: false } : null;
}

function copyTradeEnabledFromTrailingSellConfig(value: unknown): boolean {
  const record = asRecord(value);
  return record.copyTradeEnabled === false || record.copy_trade_enabled === false ? false : true;
}

const TRADING_WALLET_STATE_PREFIX = "trading_wallets_v1:";

function encodeTradingWalletState(record: SubscriberRecord): string | null {
  const wallets = dedupeTradingWallets([
    ...(record.tradingWallets || []),
    ...(record.tradingWallet ? [record.tradingWallet] : [])
  ]);

  if (wallets.length === 0) {
    return null;
  }

  return `${TRADING_WALLET_STATE_PREFIX}${JSON.stringify({
    activePublicKey: record.tradingWallet?.publicKey || wallets[0]?.publicKey || null,
    wallets
  })}`;
}

function decodeTradingWalletState(value: string | null): { activePublicKey: string | null; wallets: TradingWallet[] } | null {
  if (!value?.startsWith(TRADING_WALLET_STATE_PREFIX)) {
    return null;
  }

  try {
    const parsed = JSON.parse(value.slice(TRADING_WALLET_STATE_PREFIX.length)) as unknown;
    const record = typeof parsed === "object" && parsed !== null ? parsed as Record<string, unknown> : {};
    const activePublicKey = typeof record.activePublicKey === "string" ? record.activePublicKey : null;
    const wallets = Array.isArray(record.wallets)
      ? record.wallets.map((wallet) => {
          const walletRecord = typeof wallet === "object" && wallet !== null ? wallet as Record<string, unknown> : {};
          return normalizeTradingWallet(walletRecord);
        }).filter((wallet): wallet is TradingWallet => Boolean(wallet))
      : [];

    return {
      activePublicKey,
      wallets: dedupeTradingWallets(wallets)
    };
  } catch {
    return null;
  }
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
    provider: wallet.provider || "pumpportal-lightning",
    kind: wallet.kind || wallet.provider || "pumpportal-lightning",
    encrypted_secret_key: wallet.encryptedSecretKey || null,
    secret_key_format: wallet.secretKeyFormat || null,
    key_last4: wallet.keyLast4 || wallet.apiKeyLast4,
    label: wallet.label,
    created_at: wallet.createdAt,
    updated_at: wallet.updatedAt
  };
}

function redactSensitivePayload(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(redactSensitivePayload);
  }

  if (typeof value === "bigint") {
    return value.toString();
  }

  if (!value || typeof value !== "object") {
    return value;
  }

  const redacted: Record<string, unknown> = {};

  for (const [key, entry] of Object.entries(value)) {
    const normalizedKey = key.toLowerCase().replace(/[^a-z0-9]/g, "");
    const sensitive =
      normalizedKey.includes("apikey") ||
      normalizedKey.includes("privatekey") ||
      normalizedKey.includes("secretkey") ||
      normalizedKey.includes("encryptedapikey") ||
      normalizedKey.includes("encryptedsecretkey");
    redacted[key] = sensitive ? "[redacted]" : redactSensitivePayload(entry);
  }

  return redacted;
}

function responseWithLatencySummary(response: unknown, latencySummary: unknown): unknown {
  if (!latencySummary || !response || typeof response !== "object" || Array.isArray(response)) {
    return response;
  }

  return {
    ...(response as Record<string, unknown>),
    latencySummary
  };
}

export function copyTradeExecutionRow(record: CopyTradeExecutionRecord): CopyTradeExecutionRow {
  const latency = record.latencySummary || null;
  const redactedLatency = latency ? redactSensitivePayload(latency) : null;
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
    observed_trade: redactSensitivePayload(record.observedTrade),
    request: redactSensitivePayload(record.request),
    response: responseWithLatencySummary(redactSensitivePayload(record.response), redactedLatency),
    observed_signature: latency?.observedSignature ?? record.observedTrade.signature ?? null,
    target_observed_to_submit_ms: latency?.targetObservedToSubmitMs ?? null,
    target_blocktime_to_submit_ms: latency?.targetBlockTimeToSubmitMs ?? null,
    build_ms: latency?.buildMs ?? null,
    send_ms: latency?.sendMs ?? null,
    winner_provider: latency?.winnerProvider ?? null,
    send_rpc_winner: latency?.sendRpcWinner ?? null,
    send_rpc_count: latency?.sendRpcCount ?? null,
    target_slot: latency?.targetSlot ?? null,
    copy_slot: latency?.copySlot ?? null,
    slot_delta: latency?.slotDelta ?? null,
    latency_status: latency?.status ?? null,
    latency_summary: redactedLatency,
    trailing_sell_step_index: record.trailingSellStepIndex ?? null,
    trailing_sell_total_steps: record.trailingSellTotalSteps ?? null
  };

  if (record.createdAt) {
    row.created_at = record.createdAt;
  }

  return row;
}

function legacyCopyTradeExecutionRow(row: CopyTradeExecutionRow): Omit<CopyTradeExecutionRow,
  "observed_signature" |
  "target_observed_to_submit_ms" |
  "target_blocktime_to_submit_ms" |
  "build_ms" |
  "send_ms" |
  "winner_provider" |
  "send_rpc_winner" |
  "send_rpc_count" |
  "target_slot" |
  "copy_slot" |
  "slot_delta" |
  "latency_status" |
  "latency_summary"
> {
  const {
    observed_signature: _observedSignature,
    target_observed_to_submit_ms: _targetObserved,
    target_blocktime_to_submit_ms: _targetBlocktime,
    build_ms: _buildMs,
    send_ms: _sendMs,
    winner_provider: _winnerProvider,
    send_rpc_winner: _sendRpcWinner,
    send_rpc_count: _sendRpcCount,
    target_slot: _targetSlot,
    copy_slot: _copySlot,
    slot_delta: _slotDelta,
    latency_status: _latencyStatus,
    latency_summary: _latencySummary,
    ...legacyRow
  } = row;

  return legacyRow;
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
        copyTradeEnabled: copyTradeEnabledFromTrailingSellConfig(wallet.trailing_sell_config),
        trailingSellConfig: normalizeTrailingSellConfig(wallet.trailing_sell_config, wallet.updated_at)
      }))
    : [];
  const tradingWalletState = decodeTradingWalletState(row.copy_target_wallet_address);
  const tradingWalletRows = Array.isArray(row.telegram_trading_wallets) ? row.telegram_trading_wallets : [];
  const rowTradingWallets = dedupeTradingWallets(
    tradingWalletRows
      .map((wallet) =>
        normalizeTradingWallet({
          publicKey: wallet.public_key,
          provider: wallet.provider,
          kind: wallet.kind,
          encryptedApiKey: wallet.encrypted_api_key,
          apiKeyLast4: wallet.api_key_last4,
          encryptedSecretKey: wallet.encrypted_secret_key,
          secretKeyFormat: wallet.secret_key_format,
          keyLast4: wallet.key_last4,
          label: wallet.label ?? null,
          createdAt: wallet.created_at,
          updatedAt: wallet.updated_at
        })
      )
      .filter((wallet): wallet is TradingWallet => Boolean(wallet))
  );
  const tradingWallets = tradingWalletState?.wallets.length
    ? dedupeTradingWallets([...tradingWalletState.wallets, ...rowTradingWallets])
    : rowTradingWallets;
  const activePublicKey = row.active_trading_wallet_public_key || tradingWalletState?.activePublicKey || null;
  const tradingWallet = activePublicKey
    ? tradingWallets.find((wallet) => wallet.publicKey === activePublicKey) || tradingWallets[0] || null
    : tradingWallets[0] || null;
  const legacyCopyTarget = row.copy_target_wallet_address?.startsWith(TRADING_WALLET_STATE_PREFIX) ? null : row.copy_target_wallet_address;
  const legacyCopyTargetWallet = legacyCopyTarget && copyTradeWallets.length === 0
    ? watchedWallets.find((wallet) => wallet.address === row.copy_target_wallet_address) || {
        address: legacyCopyTarget,
        label: null,
        addedAt: row.updated_at,
        updatedAt: row.updated_at
      }
    : null;
  const nextCopyTradeWallets = legacyCopyTargetWallet ? dedupeWatchedWallets([...copyTradeWallets, legacyCopyTargetWallet]) : copyTradeWallets;
  const nextWatchedWallets = legacyCopyTargetWallet
    ? watchedWallets.filter((wallet) => wallet.address !== legacyCopyTarget)
    : watchedWallets;

  return {
    chatId: row.chat_id,
    mode: normalizeMode(row.mode),
    notificationsPaused: row.notifications_paused === true,
    watchedWallets: dedupeWatchedWallets(nextWatchedWallets),
    copyTradeWallets: dedupeWatchedWallets(nextCopyTradeWallets),
    tradingWallet,
    tradingWallets,
    copyWalletAddress: null,
    copyWalletAddresses: [],
    copyAmountSol: numericValue(row.copy_amount_sol),
    copyTradeBuySlippagePercent: numericValue(row.copy_trade_buy_slippage_percent ?? null),
    copyTradeBuyPriorityFeeSol: numericValue(row.copy_trade_buy_priority_fee_sol ?? null),
    copyTradeSellSlippagePercent: numericValue(row.copy_trade_sell_slippage_percent ?? null),
    copyTradeSellPriorityFeeSol: numericValue(row.copy_trade_sell_priority_fee_sol ?? null),
    copyTradeRetryFailedBuys: row.copy_trade_retry_failed_buys === true,
    copyTradeBuyPressureSellEnabled: row.copy_trade_buy_pressure_sell_enabled === true,
    copyTradeBuyPressureSellTimeoutMs: numericValue(row.copy_trade_buy_pressure_sell_timeout_ms ?? null),
    cashbackPayoutWalletAddress: row.cashback_payout_wallet_address || null,
    copyTargetWalletAddress: legacyCopyTarget,
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
      const row = copyTradeExecutionRow(record);
      const { error } = await client.from("telegram_copytrade_executions").insert(row);
      if (isMissingSupabaseColumn(error) && record.latencySummary) {
        const retry = await client.from("telegram_copytrade_executions").insert(legacyCopyTradeExecutionRow(row));
        const retryError = formatSupabaseError(retry.error);

        if (retryError) {
          throw retryError;
        }
        return;
      }
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    },
    async updateCopyTradeExecutionStatus(update) {
      const values: Record<string, unknown> = {
        status: update.status,
        error_text: update.errorText ?? null
      };
      const latency = update.latencySummary || null;
      const redactedLatency = latency ? redactSensitivePayload(latency) : null;

      if (latency) {
        values.observed_signature = latency.observedSignature;
        values.target_observed_to_submit_ms = latency.targetObservedToSubmitMs;
        values.target_blocktime_to_submit_ms = latency.targetBlockTimeToSubmitMs;
        values.build_ms = latency.buildMs;
        values.send_ms = latency.sendMs;
        values.winner_provider = latency.winnerProvider;
        values.send_rpc_winner = latency.sendRpcWinner;
        values.send_rpc_count = latency.sendRpcCount;
        values.target_slot = latency.targetSlot;
        values.copy_slot = latency.copySlot;
        values.slot_delta = latency.slotDelta;
        values.latency_status = latency.status;
        values.latency_summary = redactedLatency;
      }

      if ("response" in update) {
        values.response = responseWithLatencySummary(redactSensitivePayload(update.response ?? null), redactedLatency);
      }

      let query = client
        .from("telegram_copytrade_executions")
        .update(values)
        .eq("chat_id", update.chatId)
        .eq("action", update.action)
        .eq("signature", update.signature);

      if (typeof update.trailingSellStepIndex === "number") {
        query = query.eq("trailing_sell_step_index", update.trailingSellStepIndex);
      } else {
        query = query.is("trailing_sell_step_index", null);
      }

      if (typeof update.trailingSellTotalSteps === "number") {
        query = query.eq("trailing_sell_total_steps", update.trailingSellTotalSteps);
      } else {
        query = query.is("trailing_sell_total_steps", null);
      }

      const { error } = await query;
      if (isMissingSupabaseColumn(error) && latency) {
        const legacyValues = { ...values };
        delete legacyValues.observed_signature;
        delete legacyValues.target_observed_to_submit_ms;
        delete legacyValues.target_blocktime_to_submit_ms;
        delete legacyValues.build_ms;
        delete legacyValues.send_ms;
        delete legacyValues.winner_provider;
        delete legacyValues.send_rpc_winner;
        delete legacyValues.send_rpc_count;
        delete legacyValues.target_slot;
        delete legacyValues.copy_slot;
        delete legacyValues.slot_delta;
        delete legacyValues.latency_status;
        delete legacyValues.latency_summary;

        let retry = client
          .from("telegram_copytrade_executions")
          .update(legacyValues)
          .eq("chat_id", update.chatId)
          .eq("action", update.action)
          .eq("signature", update.signature);

        if (typeof update.trailingSellStepIndex === "number") {
          retry = retry.eq("trailing_sell_step_index", update.trailingSellStepIndex);
        } else {
          retry = retry.is("trailing_sell_step_index", null);
        }

        if (typeof update.trailingSellTotalSteps === "number") {
          retry = retry.eq("trailing_sell_total_steps", update.trailingSellTotalSteps);
        } else {
          retry = retry.is("trailing_sell_total_steps", null);
        }

        const retryResult = await retry;
        const retryError = formatSupabaseError(retryResult.error);

        if (retryError) {
          throw retryError;
        }
        return;
      }
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
      const subscriberSelect =
        "chat_id,mode,notifications_paused,copy_wallet_address,copy_wallet_addresses,copy_amount_sol,copy_trade_buy_slippage_percent,copy_trade_buy_priority_fee_sol,copy_trade_sell_slippage_percent,copy_trade_sell_priority_fee_sol,copy_trade_retry_failed_buys,copy_trade_buy_pressure_sell_enabled,copy_trade_buy_pressure_sell_timeout_ms,cashback_payout_wallet_address,copy_target_wallet_address,active_trading_wallet_public_key,verified_at,updated_at";
      const legacySubscriberSelect =
        "chat_id,mode,copy_wallet_address,copy_wallet_addresses,copy_amount_sol,copy_trade_buy_slippage_percent,copy_trade_buy_priority_fee_sol,copy_trade_sell_slippage_percent,copy_trade_sell_priority_fee_sol,copy_target_wallet_address,active_trading_wallet_public_key,verified_at,updated_at";
      const subscriberResult = await client
        .from("telegram_subscribers")
        .select(subscriberSelect)
        .order("chat_id", { ascending: true });
      let subscriberRows = subscriberResult.data as unknown;
      let subscriberError = subscriberResult.error;

      if (isMissingSupabaseColumn(subscriberError)) {
        const legacySubscriberResult = await client
          .from("telegram_subscribers")
          .select(legacySubscriberSelect)
          .order("chat_id", { ascending: true });
        subscriberRows = legacySubscriberResult.data as unknown;
        subscriberError = legacySubscriberResult.error;
      }

      const formattedError = formatSupabaseError(subscriberError);

      if (formattedError) {
        throw formattedError;
      }

      const [
        { data: watchedRows, error: watchedError },
        copyTradeResult,
        tradingWalletResult
      ] = await Promise.all([
        client.from("telegram_watched_wallets").select("chat_id,address,label,added_at,updated_at"),
        client.from("telegram_copytrade_wallets").select("chat_id,address,label,added_at,updated_at,trailing_sell_config"),
        client.from("telegram_trading_wallets").select("chat_id,public_key,encrypted_api_key,api_key_last4,provider,kind,encrypted_secret_key,secret_key_format,key_last4,label,created_at,updated_at")
      ]);
      let copyTradeRows = copyTradeResult.data as unknown;
      let copyTradeError = copyTradeResult.error;
      let tradingWalletRows = tradingWalletResult.data as unknown;
      let tradingWalletError = tradingWalletResult.error;

      if (isMissingSupabaseColumn(tradingWalletError)) {
        const legacyTradingWalletResult = await client
          .from("telegram_trading_wallets")
          .select("chat_id,public_key,encrypted_api_key,api_key_last4,label,created_at,updated_at");
        tradingWalletRows = legacyTradingWalletResult.data as unknown;
        tradingWalletError = legacyTradingWalletResult.error;
      }

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
      let { error } = await client.from("telegram_subscribers").upsert(subscriberRow(subscriber), { onConflict: "chat_id" });

      if (isMissingSupabaseColumn(error)) {
        if (
          subscriber.copyTradeRetryFailedBuys ||
          subscriber.copyTradeBuyPressureSellEnabled ||
          subscriber.copyTradeBuyPressureSellTimeoutMs !== null ||
          subscriber.cashbackPayoutWalletAddress !== null ||
          subscriber.notificationsPaused
        ) {
          const formattedError = formatSupabaseError(error);
          throw formattedError || new Error("Supabase subscriber store request failed");
        }

        const fallbackResult = await client
          .from("telegram_subscribers")
          .upsert(subscriberRow(subscriber, {
            includeRetryFailedBuys: false,
            includeBuyPressureSell: false,
            includeCashbackPayoutWallet: false,
            includeNotificationsPaused: false
          }), { onConflict: "chat_id" });
        error = fallbackResult.error;
      }

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
    async deleteAllCopyTradeWallets(chatId) {
      const { error } = await client.from("telegram_copytrade_wallets").delete().eq("chat_id", chatId);
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    },
    async upsertTradingWallet(chatId, wallet) {
      const { error } = await client
        .from("telegram_trading_wallets")
        .upsert(tradingWalletRow(chatId, wallet), { onConflict: "chat_id,public_key" });
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    },
    async deleteTradingWallet(chatId, publicKey) {
      const { error } = await client.from("telegram_trading_wallets").delete().eq("chat_id", chatId).eq("public_key", publicKey);
      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    },
    async deleteAllTradingWallets(chatId) {
      const { error } = await client.from("telegram_trading_wallets").delete().eq("chat_id", chatId);
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
            notificationsPaused: false,
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
        notificationsPaused: false,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
      return true;
    },
    async setNotificationsPaused(chatId, paused) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const existing = subscribers.get(normalized) || makeSubscriber(normalized, null);
      const next = {
        ...existing,
        notificationsPaused: paused,
        mode: paused ? null : existing.mode,
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
        updatedAt: now,
        copyTradeEnabled: previous?.copyTradeEnabled === false ? false : true,
        trailingSellConfig: previous?.trailingSellConfig || null
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
      const wallet = {
        ...existing.copyTradeWallets[walletIndex],
        copyTradeEnabled: enabled,
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

      const next = {
        ...existing,
        copyTradeWallets: [],
        copyTargetWalletAddress: null,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      await repository.deleteAllCopyTradeWallets(normalized);
      subscribers.set(normalized, next);
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
        tradingWallets: dedupeTradingWallets([...(subscribers.get(normalized)?.tradingWallets || []), wallet]),
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      await repository.upsertTradingWallet(normalized, wallet);
      subscribers.set(normalized, next);
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
      const tradingWallet = {
        ...existing.tradingWallet,
        label,
        updatedAt: now
      };
      const tradingWallets = dedupeTradingWallets([...(existing.tradingWallets || []), existing.tradingWallet].filter((wallet): wallet is TradingWallet => Boolean(wallet)).map((wallet) =>
        wallet.publicKey === existing.tradingWallet?.publicKey
          ? {
              ...wallet,
              label,
              updatedAt: now
            }
          : wallet
      ));
      const next = {
        ...existing,
        tradingWallet,
        tradingWallets,
        updatedAt: now
      };

      await repository.upsertSubscriber(next);
      await repository.upsertTradingWallet(normalized, tradingWallet);
      subscribers.set(normalized, next);
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

      const next = {
        ...existing,
        tradingWallet: wallet,
        tradingWallets: dedupeTradingWallets(existing.tradingWallets || [wallet]),
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      await repository.upsertTradingWallet(normalized, wallet);
      subscribers.set(normalized, next);
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
      const next = {
        ...existing,
        tradingWallet: activeWallet,
        tradingWallets: nextWallets,
        updatedAt: new Date().toISOString()
      };

      await repository.deleteTradingWallet(normalized, publicKey);
      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
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

      const next = {
        ...existing,
        tradingWallet: null,
        tradingWallets: [],
        updatedAt: new Date().toISOString()
      };

      await repository.deleteAllTradingWallets(normalized);
      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
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
    async setCopyTradeBuySlippage(chatId, percent) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const next = {
        ...(subscribers.get(normalized) || makeSubscriber(normalized, null)),
        copyTradeBuySlippagePercent: percent,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
      return true;
    },
    async setCopyTradeBuyPriorityFee(chatId, sol) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const next = {
        ...(subscribers.get(normalized) || makeSubscriber(normalized, null)),
        copyTradeBuyPriorityFeeSol: sol,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
      return true;
    },
    async setCopyTradeSellSlippage(chatId, percent) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const next = {
        ...(subscribers.get(normalized) || makeSubscriber(normalized, null)),
        copyTradeSellSlippagePercent: percent,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
      return true;
    },
    async setCopyTradeSellPriorityFee(chatId, sol) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const next = {
        ...(subscribers.get(normalized) || makeSubscriber(normalized, null)),
        copyTradeSellPriorityFeeSol: sol,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
      return true;
    },
    async setCopyTradeRetryFailedBuys(chatId, enabled) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const next = {
        ...(subscribers.get(normalized) || makeSubscriber(normalized, null)),
        copyTradeRetryFailedBuys: enabled,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
      return true;
    },
    async setCopyTradeBuyPressureSellEnabled(chatId, enabled) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const next = {
        ...(subscribers.get(normalized) || makeSubscriber(normalized, null)),
        copyTradeBuyPressureSellEnabled: enabled,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
      return true;
    },
    async setCopyTradeBuyPressureSellTimeoutMs(chatId, timeoutMs) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const next = {
        ...(subscribers.get(normalized) || makeSubscriber(normalized, null)),
        copyTradeBuyPressureSellTimeoutMs: timeoutMs,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
      return true;
    },
    async setCashbackPayoutWallet(chatId, address) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const next = {
        ...(subscribers.get(normalized) || makeSubscriber(normalized, null)),
        cashbackPayoutWalletAddress: address,
        updatedAt: new Date().toISOString()
      };

      await repository.upsertSubscriber(next);
      subscribers.set(normalized, next);
      return true;
    },
    async resetCopyTradeExecutionSettings(chatId) {
      await load();
      const normalized = normalizeChatId(chatId);

      if (!normalized || !subscribers.has(normalized)) {
        return false;
      }

      const next = {
        ...(subscribers.get(normalized) || makeSubscriber(normalized, null)),
        copyTradeBuySlippagePercent: null,
        copyTradeBuyPriorityFeeSol: null,
        copyTradeSellSlippagePercent: null,
        copyTradeSellPriorityFeeSol: null,
        copyTradeRetryFailedBuys: false,
        copyTradeBuyPressureSellEnabled: false,
        copyTradeBuyPressureSellTimeoutMs: null,
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
          copyTradeEnabled: true,
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

    const tradingWallets = (subscriber.tradingWallets || []).length > 0
      ? subscriber.tradingWallets
      : subscriber.tradingWallet
        ? [subscriber.tradingWallet]
        : [];

    for (const wallet of tradingWallets) {
      await repository.upsertTradingWallet(subscriber.chatId, wallet);
    }
  }
}
