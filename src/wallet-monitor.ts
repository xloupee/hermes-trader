import { escapeHtml } from "./format.js";
import type {
  CopyTradeSettings,
  PumpPortalLightningTradeResult,
  PumpPortalLocalTradeBuildResult,
  PumpPortalLocalTradeRequest,
  TelegramReplyMarkup,
  WalletTradeAction,
  WalletTradeAsset,
  WalletTradeData
} from "./types.js";

const BASE58_ADDRESS = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;
const BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

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
    "<b>Wallet trade detected</b>",
    `<b>${escapeHtml(walletName)}</b> ${escapeHtml(formatAction(trade.action))} ${trade.mint ? "a token" : "tokens"}`
  ];

  lines.push("");
  lines.push("<b>Watched wallet</b>");
  lines.push(`<code>${escapeHtml(trade.targetWallet)}</code>`);

  if (trade.mint) {
    lines.push("");
    lines.push("<b>Contract address</b>");
    lines.push(`<code>${escapeHtml(trade.mint)}</code>`);
  }

  const swapParts = [
    trade.input ? formatAsset(trade.input) : null,
    trade.output ? formatAsset(trade.output) : null
  ];

  if (swapParts[0] || swapParts[1]) {
    lines.push("");
    lines.push(`<b>Swap:</b> ${escapeHtml(swapParts[0] || "Unknown")} -> ${escapeHtml(swapParts[1] || "Unknown")}`);
  }

  const amounts = [
    formatOptionalAmount(trade.solAmount, "SOL"),
    formatOptionalAmount(trade.tokenAmount, "tokens"),
    trade.marketCapSol === null ? null : `market cap ${formatNumber(trade.marketCapSol)} SOL`
  ].filter(Boolean);

  if (amounts.length > 0) {
    lines.push("");
    lines.push(`<b>Amounts:</b> ${escapeHtml(amounts.join(" | "))}`);
  }

  if (trade.pool) {
    lines.push(`<b>Pool:</b> ${escapeHtml(trade.pool)}`);
  }

  if (trade.source) {
    lines.push(`<b>Source:</b> ${escapeHtml(trade.source)}`);
  }

  const fallbackLinks = [
    link("Pump.fun", trade.pumpFunUrl),
    link("Solscan token", trade.solscanTokenUrl),
    link("Solscan tx", trade.solscanTxUrl)
  ].filter(Boolean);

  if (fallbackLinks.length > 0) {
    lines.push("");
    lines.push(`<b>Links:</b> ${fallbackLinks.join(" | ")}`);
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

  const lines = [message, "", "<b>Copy trade</b>"];

  lines.push(`<b>Copy wallets:</b> ${copyWalletAddresses.length || "Not set"}`);
  lines.push(...copyWalletAddresses.map((wallet) => `<code>${escapeHtml(wallet)}</code>`));
  lines.push(`<b>Copy amount:</b> ${copyAmountSol ? `${formatNumber(copyAmountSol)} SOL` : "Not set"}`);

  if (copyWalletAddresses.length === 0 || !copyAmountSol) {
    lines.push("<b>Status:</b> Incomplete setup");
    return lines.join("\n");
  }

  if (isCopyableSolToTokenBuy(trade)) {
    lines.push(`<b>Status:</b> Ready to copy ${formatNumber(copyAmountSol)} SOL into this token from ${copyWalletAddresses.length} wallet(s)`);
  } else {
    lines.push("<b>Status:</b> Not a copyable SOL-to-token buy");
  }

  return lines.join("\n");
}

export function isCopyableSolToTokenBuy(trade: WalletTradeData): boolean {
  return trade.input?.symbol === "SOL" && Boolean(trade.output?.mint) && trade.output?.symbol !== "SOL";
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
    "<b>Would copy trade</b>",
    `<b>Target:</b> ${escapeHtml(walletName)}`,
    `<code>${escapeHtml(trade.targetWallet)}</code>`,
    "",
    `<b>Copy wallet:</b> <code>${escapeHtml(copyWalletAddress)}</code>`,
    `<b>Copy amount:</b> ${formatNumber(copySettings.copyAmountSol)} SOL`,
    "",
    "<b>Contract address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`,
    "",
    `<b>Status:</b> Would buy this token with ${formatNumber(copySettings.copyAmountSol)} SOL`,
    `<b>PumpPortal:</b> ${formatPumpPortalBuildStatus(pumpPortalBuild)}`
  ].join("\n");
}

function formatPumpPortalLightningTradeStatus(result: PumpPortalLightningTradeResult): string {
  if (result.ok) {
    return result.signature ? `Submitted: <code>${escapeHtml(result.signature)}</code>` : "Submitted";
  }

  const status = result.status === null ? "request failed" : `HTTP ${result.status}`;
  const detail = result.errorText?.trim();
  return detail ? `Failed: ${escapeHtml(status)} - ${escapeHtml(detail)}` : `Failed: ${escapeHtml(status)}`;
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
  result: PumpPortalLightningTradeResult;
}): string | null {
  if (!trade.mint || !isCopyableSolToTokenBuy(trade)) {
    return null;
  }

  const walletName = trade.label || shortenAddress(trade.targetWallet);
  return [
    "<b>Auto copy buy submitted</b>",
    `<b>Target:</b> ${escapeHtml(walletName)}`,
    `<code>${escapeHtml(trade.targetWallet)}</code>`,
    "",
    `<b>Trading wallet:</b> <code>${escapeHtml(tradingWalletPublicKey)}</code>`,
    `<b>Copy amount:</b> ${formatNumber(copyAmountSol)} SOL`,
    "",
    "<b>Contract address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`,
    "",
    `<b>PumpPortal:</b> ${formatPumpPortalLightningTradeStatus(result)}`
  ].join("\n");
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
  copyWalletAddress,
  steps
}: {
  trade: WalletTradeData;
  copyWalletAddress: string;
  steps: Array<{ delayMs: number; request: PumpPortalLocalTradeRequest }>;
}): string | null {
  if (steps.length === 0 || !trade.mint) {
    return null;
  }

  const walletName = trade.label || shortenAddress(trade.targetWallet);
  const lines = [
    "<b>Trailing sell scheduled</b>",
    `<b>Target:</b> ${escapeHtml(walletName)}`,
    `<b>My wallet:</b> <code>${escapeHtml(copyWalletAddress)}</code>`,
    "",
    "<b>Contract address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`,
    "",
    "<b>Sell builds:</b>"
  ];

  lines.push(
    ...steps.map((step, index) => {
      const seconds = formatNumber(step.delayMs / 1000);
      return `${index + 1}. Sell ${escapeHtml(String(step.request.amount))} after ${seconds}s`;
    })
  );
  lines.push("");
  lines.push("<i>Build-only: not signed, not sent.</i>");

  return lines.join("\n");
}

export function formatCopyTradeTrailingSellBuildMessage({
  trade,
  copyWalletAddress,
  stepIndex,
  totalSteps,
  request,
  pumpPortalBuild
}: {
  trade: WalletTradeData;
  copyWalletAddress: string;
  stepIndex: number;
  totalSteps: number;
  request: PumpPortalLocalTradeRequest;
  pumpPortalBuild: PumpPortalLocalTradeBuildResult;
}): string {
  const walletName = trade.label || shortenAddress(trade.targetWallet);

  return [
    "<b>Trailing sell build</b>",
    `<b>Step:</b> ${stepIndex + 1}/${totalSteps}`,
    `<b>Target:</b> ${escapeHtml(walletName)}`,
    `<b>My wallet:</b> <code>${escapeHtml(copyWalletAddress)}</code>`,
    "",
    "<b>Contract address</b>",
    `<code>${escapeHtml(request.mint)}</code>`,
    "",
    `<b>Sell amount:</b> ${escapeHtml(String(request.amount))}`,
    `<b>PumpPortal:</b> ${formatPumpPortalBuildStatus(pumpPortalBuild)}`,
    "",
    "<i>Build-only: not signed, not sent.</i>"
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
          text: "Copy CA",
          copy_text: {
            text: trade.mint
          }
        }
      ]
    ]
  };
}
