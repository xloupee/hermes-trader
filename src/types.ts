export type LooseRecord = Record<string, unknown>;

export type TelegramChatId = string | number;
export type AlertModeValue = "migrations" | "newtokens" | "both";

export interface WatchedWallet {
  address: string;
  label: string | null;
  addedAt: string;
  updatedAt: string;
}

export interface ExplorerConfig {
  pumpFunBaseUrl: string;
  solscanBaseUrl: string;
}

export interface MigrationFormatConfig extends ExplorerConfig {
  alertMode?: string;
  activeSubscriptionMethods?: string[];
  solUsdPrice?: number | null;
  tokenInfo?: LooseRecord | null;
  metadata?: LooseRecord | null;
  transactionAnalysis?: TransactionAnalysis | null;
}

export interface BotConfig extends MigrationFormatConfig {
  telegramToken?: string;
  telegramChatId?: string;
  telegramVerifyCode?: string;
  telegramSubscribersPath?: string;
  supabaseUrl?: string;
  supabaseServiceRoleKey?: string;
  pumpPortalApiKey?: string;
  pumpPortalWsUrl: string;
  pumpPortalTradeLocalUrl: string;
  migrationLogPath: string;
  walletTradeLogPath: string;
  heliusApiKey?: string;
  heliusApiBaseUrl: string;
  heliusWebhookAuthHeader?: string;
  heliusWebhookId?: string;
  heliusWebhookPublicUrl?: string;
  heliusWebhookStatePath: string;
  webhookPort: number;
  pumpFunCoinApiBaseUrl: string;
  solUsdPriceUrl: string;
  solanaRpcUrl: string;
  transactionFlowEnabled: boolean;
  transactionAccountLabels?: string;
  alertModeLabel?: string;
  shutdownReason?: string;
  copyTradeSlippage: number;
  copyTradePriorityFee: number;
  copyTradePool: PumpPortalTradePool;
}

export interface LegacyBotConfig extends MigrationFormatConfig {
  telegramToken?: string;
  telegramChatId?: string;
  telegramVerifyCode?: string;
  telegramSubscribersPath?: string;
  pumpPortalApiKey?: string;
  pumpPortalWsUrl: string;
  getModeLabel?: () => string;
  pumpPortalSubscriptionMethod?: string;
}

export interface TelegramChat {
  id?: TelegramChatId;
  type?: string;
  title?: string;
  username?: string;
  first_name?: string;
  last_name?: string;
}

export interface TelegramMessage {
  text?: string;
  chat?: TelegramChat;
}

export interface TelegramUpdate {
  update_id: number;
  message?: TelegramMessage;
  channel_post?: TelegramMessage;
  callback_query?: TelegramCallbackQuery;
}

export interface TelegramCallbackQuery {
  id: string;
  data?: string;
  message?: TelegramMessage;
}

export interface TelegramBotInfo {
  username?: string;
}

export interface TelegramReplyMarkup {
  inline_keyboard: TelegramInlineKeyboardButton[][];
}

export interface TelegramInlineKeyboardButton {
  text: string;
  callback_data?: string;
  copy_text?: {
    text: string;
  };
}

export interface SubscriberStore {
  init: () => Promise<void>;
  has: (chatId: TelegramChatId) => boolean;
  add: (chatId: TelegramChatId) => Promise<void>;
  remove: (chatId: TelegramChatId) => Promise<void>;
  get: (chatId: TelegramChatId) => SubscriberRecord | null;
  setMode: (chatId: TelegramChatId, mode: AlertModeValue | null) => Promise<boolean>;
  watchWallet: (chatId: TelegramChatId, address: string, label?: string | null) => Promise<boolean>;
  renameWallet: (chatId: TelegramChatId, address: string, label: string | null) => Promise<boolean>;
  unwatchWallet: (chatId: TelegramChatId, address: string) => Promise<boolean>;
  watchCopyTradeWallet: (chatId: TelegramChatId, address: string, label?: string | null) => Promise<boolean>;
  renameCopyTradeWallet: (chatId: TelegramChatId, address: string, label: string | null) => Promise<boolean>;
  unwatchCopyTradeWallet: (chatId: TelegramChatId, address: string) => Promise<boolean>;
  addMyWallet: (chatId: TelegramChatId, address: string, label?: string | null) => Promise<boolean>;
  renameMyWallet: (chatId: TelegramChatId, address: string, label: string | null) => Promise<boolean>;
  removeMyWallet: (chatId: TelegramChatId, address: string) => Promise<boolean>;
  setCopyWallet: (chatId: TelegramChatId, address: string) => Promise<boolean>;
  removeCopyWallet: (chatId: TelegramChatId, address: string) => Promise<boolean>;
  setCopyAmountSol: (chatId: TelegramChatId, amountSol: number) => Promise<boolean>;
  setCopyTargetWallet: (chatId: TelegramChatId, address: string | null) => Promise<boolean>;
  listWatchedWallets: (chatId: TelegramChatId) => WatchedWallet[];
  listCopyTradeWallets: (chatId: TelegramChatId) => WatchedWallet[];
  listMyWallets: (chatId: TelegramChatId) => WatchedWallet[];
  listCopyWallets: (chatId: TelegramChatId) => string[];
  list: () => SubscriberRecord[];
  count: () => number;
}

export interface SubscriberRecord {
  chatId: string;
  mode: AlertModeValue | null;
  watchedWallets: WatchedWallet[];
  copyTradeWallets: WatchedWallet[];
  myWallets: WatchedWallet[];
  copyWalletAddress: string | null;
  copyWalletAddresses: string[];
  copyAmountSol: number | null;
  copyTargetWalletAddress: string | null;
  verifiedAt: string;
  updatedAt: string;
}

export type WalletTradeAction = "buy" | "sell" | "swap" | "unknown";

export interface WalletTradeAsset {
  mint: string | null;
  symbol: string | null;
  amount: number | null;
}

export interface WalletTradeData {
  observedAt: string;
  provider: "pumpportal" | "helius";
  targetWallet: string;
  label: string | null;
  action: WalletTradeAction;
  mint: string | null;
  signature: string | null;
  timestamp: number | null;
  feePayer: string | null;
  source: string | null;
  input: WalletTradeAsset | null;
  output: WalletTradeAsset | null;
  solAmount: number | null;
  tokenAmount: number | null;
  pool: string | null;
  marketCapSol: number | null;
  pumpFunUrl: string | null;
  solscanTokenUrl: string | null;
  solscanTxUrl: string | null;
  raw: LooseRecord;
}

export interface CopyTradeSettings {
  copyWalletAddress: string | null;
  copyWalletAddresses?: string[];
  copyAmountSol: number | null;
  copyTargetWalletAddress?: string | null;
}

export type PumpPortalTradePool = "auto" | "pump" | "pump-amm" | "raydium" | "raydium-cpmm" | "launchlab" | "bonk";

export interface PumpPortalLocalTradeRequest {
  publicKey: string;
  action: "buy";
  mint: string;
  amount: number;
  denominatedInSol: "true";
  slippage: number;
  priorityFee: number;
  pool: PumpPortalTradePool;
}

export interface PumpPortalLocalTradeBuildResult {
  ok: boolean;
  status: number | null;
  bodyLength: number | null;
  errorText: string | null;
}

export interface TransactionAccountChange {
  address: string;
  deltaSol: number;
  label: string;
}

export interface TransactionAnalysis {
  feePayer: string | null;
  networkFeeSol: number;
  recipients: TransactionAccountChange[];
  senders: TransactionAccountChange[];
}

export interface MigrationData {
  observedAt: string;
  eventType: string | null;
  coinAddress: string | null;
  name: string | null;
  symbol: string | null;
  description: string | null;
  imageUrl: string | null;
  cashbackEnabled: boolean | null;
  agentBuybacksEnabled: boolean | null;
  creatorFeeEligible: boolean;
  creatorAddress: string | null;
  transactionAnalysis: TransactionAnalysis | null;
  pool: string | null;
  destination: string | null;
  marketCap: unknown;
  marketCapSol: unknown;
  marketCapUsd: unknown;
  solUsdPrice: number | null;
  initialBuy: unknown;
  solAmount: unknown;
  traderPublicKey: string | null;
  bondingCurveKey: string | null;
  virtualSolInBondingCurve: unknown;
  virtualTokensInBondingCurve: unknown;
  uri: string | null;
  isMayhemMode: unknown;
  signature: string | null;
  pumpFunUrl: string | null;
  solscanTokenUrl: string | null;
  solscanTxUrl: string | null;
  metadata: LooseRecord;
  tokenInfo: LooseRecord;
  raw: LooseRecord;
}

export function isRecord(value: unknown): value is LooseRecord {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}

export function asRecord(value: unknown): LooseRecord {
  return isRecord(value) ? value : {};
}

export function asOptionalRecord(value: unknown): LooseRecord | null {
  return isRecord(value) ? value : null;
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function stringValue(value: unknown): string | null {
  if (value === undefined || value === null || value === "") {
    return null;
  }

  return String(value);
}
