export type LooseRecord = Record<string, unknown>;

export type TelegramChatId = string | number;
export type AlertModeValue = "migrations" | "newtokens" | "both";
export type TrailingSellMode = "custom_steps" | "formula";
export type TrailingSellPercentBasis = "remaining_balance" | "original_position";

export interface TrailingSellStep {
  delayMs: number;
  percent: number;
}

export interface TrailingSellConfig {
  enabled: boolean;
  mode: TrailingSellMode;
  percentBasis: TrailingSellPercentBasis;
  steps: TrailingSellStep[];
  updatedAt: string;
}

export interface WatchedWallet {
  address: string;
  label: string | null;
  addedAt: string;
  updatedAt: string;
  trailingSellConfig?: TrailingSellConfig | null;
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
  pumpPortalCreateWalletUrl: string;
  pumpPortalLightningTradeUrl: string;
  pumpPortalWalletKeyEncryptionSecret?: string;
  migrationLogPath: string;
  walletTradeLogPath: string;
  copyTradeEmergencyStopPath: string;
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
  notifyOnShutdown: boolean;
  copyTradeEnabled: boolean;
  copyTradeDryRun: boolean;
  copyTradeExecutionProvider: "pumpportal-lightning" | "direct-pump" | "direct-pumpswap" | "direct-auto";
  copyTradeSlippage: number;
  copyTradePriorityFee: number;
  copyTradePool: PumpPortalTradePool;
  copyTradeMaxBuySol: number;
  copyTradeDailySolCap: number;
  copyTradeMinWalletReserveSol: number;
  copyTradeMaxSignalAgeMs: number;
  copyTradeMaxCopyWalletsPerChat: number;
  copyTradeAllowedSources: string[];
  copyTradeMaxSlippage: number;
  copyTradeMaxPriorityFee: number;
  copyTradeTrailingSellEnabled: boolean;
  copyTradeTrailingSellHoldMs: number;
  copyTradeTrailingSellFirstPercent: number;
  copyTradeTrailingSellTrailPercent: number;
  copyTradeTrailingSellIntervalMs: number;
  copyTradeTrailingSellMaxBuilds: number;
  copyTradeBuyPressureSellEnabled: boolean;
  copyTradeBuyPressureSellPercent: number;
  copyTradeBuyPressureSellTimeoutMs: number;
  copyTradeBuyPressureSellMinBuys: number;
  copyTradeBuyPressureSellMinTotalSol: number;
  copyTradeBuyPressureSellStatePath: string;
  directExecutionEnabled: boolean;
  directExecutionLiveEnabled: boolean;
  directExecutionBuildOnly: boolean;
  directExecutionSimulateOnly: boolean;
  directExecutionSkipPreflight: boolean;
  directExecutionConfirmationMode: "inline" | "background";
  directExecutionMaxRetries: number;
  directExecutionCanaryChatIds: string[];
  directExecutionCanaryWallets: string[];
  platformFeeEnabled: boolean;
  platformFeeBps: number;
  platformFeeTreasury?: string;
}

export interface LegacyBotConfig extends MigrationFormatConfig {
  telegramToken?: string;
  telegramChatId?: string;
  telegramVerifyCode?: string;
  telegramSubscribersPath?: string;
  pumpPortalApiKey?: string;
  pumpPortalWsUrl: string;
  pumpPortalCreateWalletUrl?: string;
  pumpPortalLightningTradeUrl?: string;
  pumpPortalWalletKeyEncryptionSecret?: string;
  copyTradeTrailingSellEnabled?: boolean;
  copyTradeTrailingSellHoldMs?: number;
  copyTradeTrailingSellFirstPercent?: number;
  copyTradeTrailingSellTrailPercent?: number;
  copyTradeTrailingSellIntervalMs?: number;
  copyTradeTrailingSellMaxBuilds?: number;
  copyTradeSlippage?: number;
  copyTradePriorityFee?: number;
  copyTradeMaxBuySol?: number;
  copyTradeDailySolCap?: number;
  copyTradeMinWalletReserveSol?: number;
  copyTradeMaxSignalAgeMs?: number;
  copyTradeMaxCopyWalletsPerChat?: number;
  copyTradeAllowedSources?: string[];
  copyTradeMaxSlippage?: number;
  copyTradeMaxPriorityFee?: number;
  copyTradeBuyPressureSellEnabled?: boolean;
  copyTradeBuyPressureSellTimeoutMs?: number;
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
  url?: string;
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
  unwatchAllCopyTradeWallets: (chatId: TelegramChatId) => Promise<number>;
  setCopyTradeWalletTrailingSellConfig: (
    chatId: TelegramChatId,
    address: string,
    config: TrailingSellConfig | null
  ) => Promise<boolean>;
  setTradingWallet: (chatId: TelegramChatId, wallet: TradingWallet) => Promise<boolean>;
  renameTradingWallet: (chatId: TelegramChatId, label: string | null) => Promise<boolean>;
  setActiveTradingWallet: (chatId: TelegramChatId, publicKey: string) => Promise<boolean>;
  removeTradingWallet: (chatId: TelegramChatId, publicKey: string) => Promise<boolean>;
  removeAllTradingWallets: (chatId: TelegramChatId) => Promise<number>;
  getTradingWallet: (chatId: TelegramChatId) => TradingWallet | null;
  listTradingWallets: (chatId: TelegramChatId) => TradingWallet[];
  setCopyWallet: (chatId: TelegramChatId, address: string) => Promise<boolean>;
  removeCopyWallet: (chatId: TelegramChatId, address: string) => Promise<boolean>;
  setCopyAmountSol: (chatId: TelegramChatId, amountSol: number) => Promise<boolean>;
  setCopyTradeBuySlippage: (chatId: TelegramChatId, percent: number) => Promise<boolean>;
  setCopyTradeBuyPriorityFee: (chatId: TelegramChatId, sol: number) => Promise<boolean>;
  setCopyTradeSellSlippage: (chatId: TelegramChatId, percent: number) => Promise<boolean>;
  setCopyTradeSellPriorityFee: (chatId: TelegramChatId, sol: number) => Promise<boolean>;
  setCopyTradeRetryFailedBuys: (chatId: TelegramChatId, enabled: boolean) => Promise<boolean>;
  setCopyTradeBuyPressureSellEnabled: (chatId: TelegramChatId, enabled: boolean) => Promise<boolean>;
  setCopyTradeBuyPressureSellTimeoutMs: (chatId: TelegramChatId, timeoutMs: number | null) => Promise<boolean>;
  resetCopyTradeExecutionSettings: (chatId: TelegramChatId) => Promise<boolean>;
  setCopyTargetWallet: (chatId: TelegramChatId, address: string | null) => Promise<boolean>;
  listWatchedWallets: (chatId: TelegramChatId) => WatchedWallet[];
  listCopyTradeWallets: (chatId: TelegramChatId) => WatchedWallet[];
  listCopyWallets: (chatId: TelegramChatId) => string[];
  list: () => SubscriberRecord[];
  count: () => number;
}

export interface SubscriberRecord {
  chatId: string;
  mode: AlertModeValue | null;
  watchedWallets: WatchedWallet[];
  copyTradeWallets: WatchedWallet[];
  tradingWallet: TradingWallet | null;
  tradingWallets: TradingWallet[];
  copyWalletAddress: string | null;
  copyWalletAddresses: string[];
  copyAmountSol: number | null;
  copyTradeBuySlippagePercent: number | null;
  copyTradeBuyPriorityFeeSol: number | null;
  copyTradeSellSlippagePercent: number | null;
  copyTradeSellPriorityFeeSol: number | null;
  copyTradeRetryFailedBuys: boolean;
  copyTradeBuyPressureSellEnabled: boolean;
  copyTradeBuyPressureSellTimeoutMs: number | null;
  copyTargetWalletAddress: string | null;
  verifiedAt: string;
  updatedAt: string;
}

export interface TradingWallet {
  publicKey: string;
  provider?: "pumpportal-lightning" | "local-solana";
  kind?: "pumpportal-lightning" | "local-solana";
  encryptedApiKey: string;
  apiKeyLast4: string;
  encryptedSecretKey?: string;
  secretKeyFormat?: "base58" | "base64";
  keyLast4?: string;
  label: string | null;
  createdAt: string;
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
  action: "buy" | "sell";
  mint: string;
  amount: number | `${number}%`;
  denominatedInSol: "true" | "false";
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

export interface PumpPortalLightningWallet {
  publicKey: string;
  privateKey: string;
  apiKey: string;
}

export interface PumpPortalLightningTradeRequest {
  action: "buy" | "sell";
  mint: string;
  amount: number | `${number}%`;
  denominatedInSol: "true" | "false";
  slippage: number;
  priorityFee: number;
  pool: PumpPortalTradePool;
  skipPreflight?: boolean;
}

export interface PumpPortalLightningTradeResult {
  ok: boolean;
  status: number | null;
  signature: string | null;
  errorText: string | null;
  raw: unknown;
}

export type CopyTradeExecutionAction = "buy" | "sell";
export type CopyTradeExecutionStatus = "submitted" | "failed" | "skipped" | "simulated" | "confirmed" | "expired";

export interface CopyTradeExecutionRecord {
  chatId: string;
  sourceWalletAddress: string;
  sourceWalletLabel: string | null;
  tradingWalletPublicKey: string;
  mint: string;
  action: CopyTradeExecutionAction;
  amount: number | string;
  denominatedInSol: "true" | "false";
  status: CopyTradeExecutionStatus;
  signature: string | null;
  errorText: string | null;
  httpStatus: number | null;
  observedTrade: WalletTradeData;
  request: unknown;
  response: unknown;
  trailingSellStepIndex?: number | null;
  trailingSellTotalSteps?: number | null;
  createdAt?: string;
}

export interface CopyTradeExecutionStatusUpdate {
  chatId: string;
  action: CopyTradeExecutionAction;
  signature: string;
  status: CopyTradeExecutionStatus;
  errorText?: string | null;
  response?: unknown;
  trailingSellStepIndex?: number | null;
  trailingSellTotalSteps?: number | null;
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

export interface TokenSocialLinks {
  twitterUrl: string | null;
  telegramUrl: string | null;
  websiteUrl: string | null;
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
  socialLinks: TokenSocialLinks;
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
