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
import type {
  CopyTradeBuyPressureSellTrigger,
  CopyTradeBuyPressureSellWatcher
} from "./copytrade-buy-pressure.js";

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

function nestedLine(index: number, total: number, text: string): string {
  return `${index === total - 1 ? "└" : "├"} ${text}`;
}

function nestedLines(lines: string[]): string[] {
  return lines.map((line, index) => nestedLine(index, lines.length, line));
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
    "<b>🎯 Trade</b>",
    `├ Wallet: ${escapeHtml(walletName)}`,
    `└ Action: ${escapeHtml(formatAction(trade.action))} ${trade.mint ? "a token" : "tokens"}`
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
    lines.push("<b>🔁 Swap</b>");
    lines.push(`└ ${escapeHtml(swapParts[0] || "Unknown")} -> ${escapeHtml(swapParts[1] || "Unknown")}`);
  }

  const amounts = [
    formatOptionalAmount(trade.solAmount, "SOL"),
    formatOptionalAmount(trade.tokenAmount, "tokens"),
    trade.marketCapSol === null ? null : `market cap ${formatNumber(trade.marketCapSol)} SOL`
  ].filter((item): item is string => Boolean(item));

  if (amounts.length > 0) {
    lines.push("");
    lines.push("<b>💰 Amounts</b>");
    lines.push(...nestedLines(amounts.map((amount) => escapeHtml(amount))));
  }

  const routeLines = [
    trade.pool ? `Pool: ${escapeHtml(trade.pool)}` : null,
    trade.source ? `Source: ${escapeHtml(trade.source)}` : null
  ].filter((line): line is string => line !== null);

  if (routeLines.length > 0) {
    lines.push("");
    lines.push("<b>📡 Route</b>");
    lines.push(...nestedLines(routeLines));
  }

  const fallbackLinks = [
    link("Pump.fun", trade.pumpFunUrl),
    link("Solscan token", trade.solscanTokenUrl),
    link("Solscan tx", trade.solscanTxUrl)
  ].filter((item): item is string => Boolean(item));

  if (fallbackLinks.length > 0) {
    lines.push("");
    lines.push("<b>🔗 Links</b>");
    lines.push(...nestedLines(fallbackLinks));
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

  lines.push(`├ Copy Wallets: ${copyWalletAddresses.length || "Not set"}`);
  lines.push(...copyWalletAddresses.map((wallet) => `├ <code>${escapeHtml(wallet)}</code>`));
  lines.push(`└ Copy Amount: ${copyAmountSol ? `${formatNumber(copyAmountSol)} SOL` : "Not set"}`);

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
    "<b>🎯 Target</b>",
    `├ ${escapeHtml(walletName)}`,
    `└ <code>${escapeHtml(trade.targetWallet)}</code>`,
    "",
    "<b>⚙️ Copy Setup</b>",
    `├ Copy Wallet: <code>${escapeHtml(copyWalletAddress)}</code>`,
    `└ Copy Amount: ${formatNumber(copySettings.copyAmountSol)} SOL`,
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`,
    "",
    "<b>🧱 Build</b>",
    `└ ${formatPumpPortalBuildStatus(pumpPortalBuild)}`
  ].join("\n");
}

type FormattableTradeResult = PumpPortalLightningTradeResult | TradeExecutionResult;

function isProviderNeutralTradeResult(result: FormattableTradeResult): result is TradeExecutionResult {
  return typeof result.status === "string";
}

function formatTradeResultStatus(result: FormattableTradeResult): string {
  if (isProviderNeutralTradeResult(result)) {
    if (result.ok) {
      return result.signature ? `<b>Trade</b>\n└ Tx: <code>${escapeHtml(result.signature)}</code>` : `<b>Trade</b>\n└ Status: ${escapeHtml(result.status)}`;
    }

    const detail = result.errorText?.trim();
    return detail
      ? `<b>Trade</b>\n└ Failed: ${escapeHtml(result.status)} - ${escapeHtml(detail)}`
      : `<b>Trade</b>\n└ Failed: ${escapeHtml(result.status)}`;
  }

  if (result.ok) {
    return result.signature ? `<b>Trade</b>\n└ Tx: <code>${escapeHtml(result.signature)}</code>` : "<b>Trade</b>\n└ Tx: Submitted";
  }

  const status = result.status === null ? "request failed" : `HTTP ${result.status}`;
  const detail = result.errorText?.trim();
  return detail ? `<b>Trade</b>\n└ Failed: ${escapeHtml(status)} - ${escapeHtml(detail)}` : `<b>Trade</b>\n└ Failed: ${escapeHtml(status)}`;
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
    "<b>🎯 Copy Buy</b>",
    `├ Target: ${escapeHtml(walletName)}`,
    `├ Trading Wallet: <code>${escapeHtml(tradingWalletPublicKey)}</code>`,
    `└ Copy Amount: ${formatNumber(copyAmountSol)} SOL`,
    ...formatPlatformFeeBlock(result),
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`,
    "",
    formatTradeResultStatus(result)
  ];

  return lines.join("\n");
}

function formatPlatformFeeLines(result: FormattableTradeResult): string[] {
  if (!isProviderNeutralTradeResult(result) || !result.platformFee?.enabled || result.platformFee.feeLamports === 0n) {
    return [];
  }

  return [
    `Platform Fee: ${formatNumber(Number(result.platformFee.feeLamports) / 1_000_000_000)} SOL`,
    `Trade Amount: ${formatNumber(Number(result.platformFee.tradeLamports) / 1_000_000_000)} SOL`
  ];
}

function formatPlatformFeeBlock(result: FormattableTradeResult): string[] {
  const lines = formatPlatformFeeLines(result);
  return lines.length > 0 ? ["", "<b>🏦 Fees</b>", ...nestedLines(lines)] : [];
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
    "<b>🎯 Sell Plan</b>",
    `├ Target: ${escapeHtml(walletName)}`,
    `└ Steps: ${steps.length}`,
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`,
    "",
    "<b>🕒 Sell Schedule</b>"
  ];

  lines.push(
    ...steps.map((step, index) => {
      const previousDelayMs = index > 0 ? steps[index - 1].delayMs : 0;
      const waitMs = Math.max(0, step.delayMs - previousDelayMs);
      const seconds = formatNumber(waitMs / 1000);
      const timing = index === 0 ? `after ${seconds}s` : `${seconds}s later`;
      return nestedLine(index, steps.length, `Sell ${escapeHtml(String(step.request.amount))} ${timing}`);
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
    "<b>🎯 Sell Plan</b>",
    `└ Target: ${escapeHtml(walletName)}`,
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`,
    "",
    "<b>Reason</b>",
    `└ <code>${escapeHtml(reason)}</code>`
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
    "<b>🎯 Sell</b>",
    `├ Target: ${escapeHtml(walletName)}`,
    `├ Step: ${stepIndex + 1}/${totalSteps}`,
    `└ Sell Amount: ${escapeHtml(String(request.amount))}`,
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${escapeHtml(request.mint)}</code>`,
    "",
    duplicateSignature
      ? `${formatTradeResultStatus(result)}\n\nThis step returned a transaction signature that was already used earlier in this trailing sell schedule.`
      : formatTradeResultStatus(result)
  ].join("\n");
}

export function formatCopyTradeBuyPressureSellScheduledMessage({
  trade,
  watcher
}: {
  trade: WalletTradeData;
  watcher: CopyTradeBuyPressureSellWatcher;
}): string | null {
  if (!trade.mint) {
    return null;
  }

  const walletName = trade.label || shortenAddress(trade.targetWallet);
  const timeoutSeconds = Math.max(0, Math.round((watcher.expiresAtMs - watcher.createdAtMs) / 1000));
  const buyThreshold = watcher.minTotalSol > 0
    ? `${watcher.minBuys} buy${watcher.minBuys === 1 ? "" : "s"} and ${formatNumber(watcher.minTotalSol)} SOL`
    : `${watcher.minBuys} buy${watcher.minBuys === 1 ? "" : "s"}`;

  return [
    "<b>📈 Buy-Pressure Sell</b>",
    "🟢 Exit watcher armed",
    "",
    "<b>🎯 Exit Rules</b>",
    `├ Target: ${escapeHtml(walletName)}`,
    `├ Sell Amount: ${formatNumber(watcher.sellPercent)}%`,
    `├ Buy Trigger: ${escapeHtml(buyThreshold)}`,
    `└ Timeout: ${formatNumber(timeoutSeconds)}s`,
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`
  ].join("\n");
}

export function formatCopyTradeBuyPressureSellResultMessage({
  trade,
  trigger,
  request,
  result
}: {
  trade: WalletTradeData;
  trigger: CopyTradeBuyPressureSellTrigger;
  request: { mint: string; amount: number | `${number}%` };
  result: FormattableTradeResult;
}): string {
  const walletName = trade.label || shortenAddress(trade.targetWallet);
  const skipped = !result.ok && result.errorText?.startsWith("Buy-pressure sell skipped:");
  const statusLine = result.ok ? "🟢 Sell submitted" : skipped ? "🟡 Sell skipped" : "🔴 Sell failed";
  const reasonLabel = trigger.kind === "buy_pressure" ? "Buy pressure" : "Timeout fallback";
  const buyStats = trigger.buyCount > 0
    ? `${trigger.buyCount} buy${trigger.buyCount === 1 ? "" : "s"} / ${formatNumber(trigger.buySol)} SOL`
    : "No qualifying buys";

  return [
    "<b>📈 Buy-Pressure Sell</b>",
    statusLine,
    "",
    "<b>🎯 Exit</b>",
    `├ Target: ${escapeHtml(walletName)}`,
    `├ Why: ${escapeHtml(reasonLabel)} - ${escapeHtml(trigger.reason)}`,
    `├ Matched Buys: ${escapeHtml(buyStats)}`,
    `└ Sell Amount: ${escapeHtml(String(request.amount))}`,
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${escapeHtml(request.mint)}</code>`,
    "",
    formatTradeResultStatus(result)
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
