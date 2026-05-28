import { escapeHtml } from "./format.js";
import type {
  CopyTradeSettings,
  PumpPortalLightningTradeResult,
  PumpPortalLocalTradeBuildResult,
  TelegramReplyMarkup,
  WalletTradeAction,
  WalletTradeAsset,
  WalletTradeData
} from "./types.js";
import type { TradeExecutionResult } from "./trade-execution.js";

const BASE58_ADDRESS = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;
const BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const SOL_MINT = "So11111111111111111111111111111111111111112";

function shortenAddress(value: string): string {
  if (!value || value.length <= 16) {
    return value;
  }

  return `${value.slice(0, 6)}...${value.slice(-6)}`;
}

function formatNumber(value: number): string {
  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits: Math.abs(value) < 0.001 ? 9 : 4
  }).format(value);
}

function formatOptionalAmount(value: number | null, suffix: string): string | null {
  return value === null ? null : `${formatNumber(value)} ${suffix}`;
}

function formatAction(action: WalletTradeAction): string {
  if (action === "swap") {
    return "swapped";
  }

  if (action === "buy") {
    return "bought";
  }

  if (action === "sell") {
    return "sold";
  }

  return "traded";
}

function formatActionStatus(action: WalletTradeAction): string {
  if (action === "buy") {
    return "🟢 Buy detected";
  }

  if (action === "sell") {
    return "🔴 Sell detected";
  }

  if (action === "swap") {
    return "🔁 Swap detected";
  }

  return "⚪ Trade detected";
}

export function isValidSolanaAddress(value: string): boolean {
  const trimmed = value.trim();

  if (!BASE58_ADDRESS.test(trimmed)) {
    return false;
  }

  return base58DecodedLength(trimmed) === 32;
}

function base58DecodedLength(value: string): number | null {
  const bytes = [0];

  for (const char of value) {
    const carryStart = BASE58_ALPHABET.indexOf(char);

    if (carryStart === -1) {
      return null;
    }

    let carry = carryStart;

    for (let index = 0; index < bytes.length; index += 1) {
      carry += bytes[index] * 58;
      bytes[index] = carry & 0xff;
      carry >>= 8;
    }

    while (carry > 0) {
      bytes.push(carry & 0xff);
      carry >>= 8;
    }
  }

  for (const char of value) {
    if (char !== "1") {
      break;
    }

    bytes.push(0);
  }

  return bytes.length;
}

export function getWalletTradeEventId(trade: WalletTradeData): string | null {
  if (trade.signature) {
    return ["wallet-trade", trade.provider, trade.signature, trade.targetWallet].filter(Boolean).join(":");
  }

  if (trade.mint) {
    return ["wallet-trade", trade.targetWallet, trade.action, trade.mint, trade.solAmount, trade.tokenAmount].filter(Boolean).join(":");
  }

  return null;
}

function link(label: string, url: string | null): string | null {
  if (!url) {
    return null;
  }

  return `<a href="${escapeHtml(url)}">${escapeHtml(label)}</a>`;
}

export function formatWalletTradeMessage(trade: WalletTradeData): string {
  const walletName = trade.label || shortenAddress(trade.targetWallet);
  const lines = [
    "<b>👀 Wallet Trade</b>",
    formatActionStatus(trade.action),
    "",
    `<b>🎯 Wallet:</b> ${escapeHtml(walletName)}`,
    `<b>📌 Action:</b> ${escapeHtml(formatAction(trade.action))} ${trade.mint ? "a token" : "tokens"}`
  ];

  lines.push("<b>🔎 Watched Wallet</b>");
  lines.push(`<code>${escapeHtml(trade.targetWallet)}</code>`);

  if (trade.mint) {
    lines.push("");
    lines.push("<b>🪙 Contract Address</b>");
    lines.push(`<code>${escapeHtml(trade.mint)}</code>`);
  }

  const swapParts = [
    trade.input ? formatAsset(trade.input) : null,
    trade.output ? formatAsset(trade.output) : null
  ];

  if (swapParts[0] || swapParts[1]) {
    lines.push("");
    lines.push(`<b>🔁 Swap:</b> ${escapeHtml(swapParts[0] || "Unknown")} -> ${escapeHtml(swapParts[1] || "Unknown")}`);
  }

  const amounts = [
    formatOptionalAmount(trade.solAmount, "SOL"),
    formatOptionalAmount(trade.tokenAmount, "tokens"),
    trade.marketCapSol === null ? null : `market cap ${formatNumber(trade.marketCapSol)} SOL`
  ].filter(Boolean);

  if (amounts.length > 0) {
    lines.push("");
    lines.push(`<b>💰 Amounts:</b> ${escapeHtml(amounts.join(" | "))}`);
  }

  if (trade.pool) {
    lines.push(`<b>🏊 Pool:</b> ${escapeHtml(trade.pool)}`);
  }

  if (trade.source) {
    lines.push(`<b>📡 Source:</b> ${escapeHtml(trade.source)}`);
  }

  const fallbackLinks = [
    link("Pump.fun", trade.pumpFunUrl),
    link("Solscan token", trade.solscanTokenUrl),
    link("Solscan tx", trade.solscanTxUrl)
  ].filter(Boolean);

  if (fallbackLinks.length > 0) {
    lines.push("");
    lines.push(`<b>🔗 Links:</b> ${fallbackLinks.join(" | ")}`);
  }

  return lines.join("\n");
}

export function formatWalletTradeMessageWithCopySettings(trade: WalletTradeData, copySettings?: CopyTradeSettings | null): string {
  const message = formatWalletTradeMessage(trade);
  const copyWalletAddresses = copySettings?.copyWalletAddresses?.length
    ? copySettings.copyWalletAddresses
    : copySettings?.copyWalletAddress
      ? [copySettings.copyWalletAddress]
      : [];
  const copyAmountSol = copySettings?.copyAmountSol || null;

  if (copyWalletAddresses.length === 0 && !copyAmountSol) {
    return message;
  }

  const lines = [message, "", "<b>⚡ Copy Trade Setup</b>"];

  lines.push(`<b>👛 Copy Wallets:</b> ${copyWalletAddresses.length || "Not set"}`);
  lines.push(...copyWalletAddresses.map((wallet) => `<code>${escapeHtml(wallet)}</code>`));
  lines.push(`<b>💰 Copy Amount:</b> ${copyAmountSol ? `${formatNumber(copyAmountSol)} SOL` : "Not set"}`);

  if (copyWalletAddresses.length === 0 || !copyAmountSol) {
    lines.push("🔴 Setup is <b>incomplete</b>");
    return lines.join("\n");
  }

  if (isCopyableSolToTokenBuy(trade)) {
    lines.push(`🟢 Ready to copy <b>${formatNumber(copyAmountSol)} SOL</b> into this token from ${copyWalletAddresses.length} wallet(s)`);
  } else {
    lines.push("⚪ Not a copyable SOL-to-token buy");
  }

  return lines.join("\n");
}

export function isCopyableSolToTokenBuy(trade: WalletTradeData): boolean {
  return trade.action === "buy" && isSolAsset(trade.input) && Boolean(trade.output?.mint) && !isSolAsset(trade.output);
}

function isSolAsset(asset?: WalletTradeAsset | null): boolean {
  return asset?.symbol === "SOL" || asset?.mint === SOL_MINT;
}

export function formatCopyTradeSimulationMessage(
  trade: WalletTradeData,
  copySettings?: CopyTradeSettings | null,
  pumpPortalBuild?: PumpPortalLocalTradeBuildResult | null
): string | null {
  const copyWalletAddress = copySettings?.copyWalletAddress || copySettings?.copyWalletAddresses?.[0] || null;

  if (!copyWalletAddress || !copySettings?.copyAmountSol || !isCopyableSolToTokenBuy(trade) || !trade.mint) {
    return null;
  }

  const walletName = trade.label || shortenAddress(trade.targetWallet);
  return [
    "<b>⚡ Copy Trade Simulation</b>",
    "🟡 Would buy this token",
    "",
    `<b>🎯 Target:</b> ${escapeHtml(walletName)}`,
    `<code>${escapeHtml(trade.targetWallet)}</code>`,
    "",
    `<b>👛 Copy Wallet:</b> <code>${escapeHtml(copyWalletAddress)}</code>`,
    `<b>💰 Copy Amount:</b> ${formatNumber(copySettings.copyAmountSol)} SOL`,
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`,
    "",
    `<b>Build:</b> ${formatPumpPortalBuildStatus(pumpPortalBuild)}`
  ].join("\n");
}

type FormattableTradeResult = PumpPortalLightningTradeResult | TradeExecutionResult;

function isProviderNeutralTradeResult(result: FormattableTradeResult): result is TradeExecutionResult {
  return typeof result.status === "string";
}

function formatTradeResultStatus(result: FormattableTradeResult): string {
  if (isProviderNeutralTradeResult(result)) {
    if (result.ok) {
      return result.signature ? `<b>Tx:</b> <code>${escapeHtml(result.signature)}</code>` : `<b>Status:</b> ${escapeHtml(result.status)}`;
    }

    const detail = result.errorText?.trim();
    return detail
      ? `<b>Trade failed:</b> ${escapeHtml(result.status)} - ${escapeHtml(detail)}`
      : `<b>Trade failed:</b> ${escapeHtml(result.status)}`;
  }

  if (result.ok) {
    return result.signature ? `<b>Tx:</b> <code>${escapeHtml(result.signature)}</code>` : "<b>Tx:</b> Submitted";
  }

  const status = result.status === null ? "request failed" : `HTTP ${result.status}`;
  const detail = result.errorText?.trim();
  return detail ? `<b>Trade failed:</b> ${escapeHtml(status)} - ${escapeHtml(detail)}` : `<b>Trade failed:</b> ${escapeHtml(status)}`;
}

export function formatAutoCopyBuyMessage({
  trade,
  tradingWalletPublicKey,
  copyAmountSol,
  result
}: {
  trade: WalletTradeData;
  tradingWalletPublicKey: string;
  copyAmountSol: number;
  result: FormattableTradeResult;
}): string | null {
  if (!trade.mint || !isCopyableSolToTokenBuy(trade)) {
    return null;
  }

  const walletName = trade.label || shortenAddress(trade.targetWallet);
  const lines = [
    "<b>⚡ Auto Copy Buy</b>",
    result.ok ? "🟢 Buy submitted" : "🔴 Buy failed",
    "",
    `<b>🎯 Target:</b> ${escapeHtml(walletName)}`,
    `<b>👛 Trading Wallet:</b> <code>${escapeHtml(tradingWalletPublicKey)}</code>`,
    `<b>💰 Copy Amount:</b> ${formatNumber(copyAmountSol)} SOL`,
    ...formatPlatformFeeLines(result),
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`,
    "",
    formatTradeResultStatus(result)
  ];

  if (!trade.label) {
    lines.splice(5, 0, `<code>${escapeHtml(trade.targetWallet)}</code>`);
  }

  return lines.join("\n");
}

function formatPlatformFeeLines(result: FormattableTradeResult): string[] {
  if (!isProviderNeutralTradeResult(result) || !result.platformFee?.enabled || result.platformFee.feeLamports === 0n) {
    return [];
  }

  return [
    `<b>🏦 Platform Fee:</b> ${formatNumber(Number(result.platformFee.feeLamports) / 1_000_000_000)} SOL`,
    `<b>📈 Trade Amount:</b> ${formatNumber(Number(result.platformFee.tradeLamports) / 1_000_000_000)} SOL`
  ];
}

function formatPumpPortalBuildStatus(result?: PumpPortalLocalTradeBuildResult | null): string {
  if (!result) {
    return "Local transaction build not requested";
  }

  if (result.ok) {
    return `Local transaction built${result.bodyLength === null ? "" : ` (${formatNumber(result.bodyLength)} bytes)`}`;
  }

  const status = result.status === null ? "request failed" : `HTTP ${result.status}`;
  const detail = result.errorText?.trim();
  return detail ? `Local transaction build failed: ${escapeHtml(status)} - ${escapeHtml(detail)}` : `Local transaction build failed: ${escapeHtml(status)}`;
}

export function formatCopyTradeTrailingSellScheduledMessage({
  trade,
  steps
}: {
  trade: WalletTradeData;
  steps: Array<{ delayMs: number; request: { amount: number | `${number}%` } }>;
}): string | null {
  if (steps.length === 0 || !trade.mint) {
    return null;
  }

  const walletName = trade.label || shortenAddress(trade.targetWallet);
  const lines = [
    "<b>📉 Trailing Sells</b>",
    "🟢 Sell schedule created",
    "",
    `<b>🎯 Target:</b> ${escapeHtml(walletName)}`,
    `<b>🪜 Steps:</b> ${steps.length}`,
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`,
    "",
    "<b>🕒 Sell Schedule</b>"
  ];

  lines.push(
    ...steps.map((step, index) => {
      const seconds = formatNumber(step.delayMs / 1000);
      return `${index + 1}. Sell ${escapeHtml(String(step.request.amount))} after ${seconds}s`;
    })
  );

  return lines.join("\n");
}

export function formatCopyTradeTrailingSellSkippedMessage({
  trade,
  reason
}: {
  trade: WalletTradeData;
  reason: string;
}): string | null {
  if (!trade.mint) {
    return null;
  }

  const walletName = trade.label || shortenAddress(trade.targetWallet);

  return [
    "<b>📉 Trailing Sells</b>",
    "🟡 Sell schedule skipped",
    "",
    `<b>🎯 Target:</b> ${escapeHtml(walletName)}`,
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`,
    "",
    `<b>Reason:</b> <code>${escapeHtml(reason)}</code>`
  ].join("\n");
}

export function formatCopyTradeTrailingSellResultMessage({
  trade,
  stepIndex,
  totalSteps,
  request,
  result,
  duplicateSignature = false
}: {
  trade: WalletTradeData;
  stepIndex: number;
  totalSteps: number;
  request: { mint: string; amount: number | `${number}%` };
  result: FormattableTradeResult;
  duplicateSignature?: boolean;
}): string {
  const walletName = trade.label || shortenAddress(trade.targetWallet);
  const statusLine = duplicateSignature ? "🟡 Duplicate tx returned" : result.ok ? "🟢 Sell submitted" : "🔴 Sell failed";

  return [
    "<b>📉 Trailing Sell</b>",
    statusLine,
    "",
    `<b>🎯 Target:</b> ${escapeHtml(walletName)}`,
    `<b>🪜 Step:</b> ${stepIndex + 1}/${totalSteps}`,
    `<b>💰 Sell Amount:</b> ${escapeHtml(String(request.amount))}`,
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${escapeHtml(request.mint)}</code>`,
    "",
    duplicateSignature
      ? `${formatTradeResultStatus(result)}\n\nThis step returned a transaction signature that was already used earlier in this trailing sell schedule.`
      : formatTradeResultStatus(result)
  ].join("\n");
}

function formatAsset({ amount, symbol, mint }: WalletTradeAsset): string | null {
  const label = symbol || (mint ? shortenAddress(mint) : null);

  if (amount === null && !label) {
    return null;
  }

  if (amount === null) {
    return label;
  }

  return `${formatNumber(amount)}${label ? ` ${label}` : ""}`;
}

export function buildWalletTradeReplyMarkup(trade: WalletTradeData): TelegramReplyMarkup | undefined {
  if (!trade.mint) {
    return undefined;
  }

  return {
    inline_keyboard: [
      [
        {
          text: "📋 Copy CA",
          copy_text: {
            text: trade.mint
          }
        }
      ]
    ]
  };
}
