import { escapeHtml } from "./format.js";
import type {
  CopyTradeSettings,
  PumpPortalLocalTradeBuildResult,
  TelegramReplyMarkup,
  WalletTradeAction,
  WalletTradeAsset,
  WalletTradeData
} from "./types.js";

const BASE58_ADDRESS = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;

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
  return BASE58_ADDRESS.test(value.trim());
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

  if (!copySettings?.copyWalletAddress && !copySettings?.copyAmountSol) {
    return message;
  }

  const lines = [message, "", "<b>Copy trade</b>"];

  lines.push(
    `<b>Copy wallet:</b> ${copySettings.copyWalletAddress ? `<code>${escapeHtml(copySettings.copyWalletAddress)}</code>` : "Not set"}`
  );
  lines.push(`<b>Copy amount:</b> ${copySettings.copyAmountSol ? `${formatNumber(copySettings.copyAmountSol)} SOL` : "Not set"}`);

  if (!copySettings.copyWalletAddress || !copySettings.copyAmountSol) {
    lines.push("<b>Status:</b> Incomplete setup");
    return lines.join("\n");
  }

  if (isCopyableSolToTokenBuy(trade)) {
    lines.push(`<b>Status:</b> Ready to copy ${formatNumber(copySettings.copyAmountSol)} SOL into this token`);
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
  if (!copySettings?.copyWalletAddress || !copySettings.copyAmountSol || !isCopyableSolToTokenBuy(trade) || !trade.mint) {
    return null;
  }

  const walletName = trade.label || shortenAddress(trade.targetWallet);
  return [
    "<b>Would copy trade</b>",
    `<b>Target:</b> ${escapeHtml(walletName)}`,
    `<code>${escapeHtml(trade.targetWallet)}</code>`,
    "",
    `<b>Copy wallet:</b> <code>${escapeHtml(copySettings.copyWalletAddress)}</code>`,
    `<b>Copy amount:</b> ${formatNumber(copySettings.copyAmountSol)} SOL`,
    "",
    "<b>Contract address</b>",
    `<code>${escapeHtml(trade.mint)}</code>`,
    "",
    `<b>Status:</b> Would buy this token with ${formatNumber(copySettings.copyAmountSol)} SOL`,
    `<b>PumpPortal:</b> ${formatPumpPortalBuildStatus(pumpPortalBuild)}`
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
