import { escapeHtml, readableWebsocketData } from "./format.js";
import { stringValue } from "./types.js";
import type { ExplorerConfig, LooseRecord, TelegramReplyMarkup, WalletTradeAction, WalletTradeData } from "./types.js";

const BASE58_ADDRESS = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;

function pickFirstObjectValue(object: LooseRecord | null | undefined, keys: readonly string[]): unknown {
  for (const key of keys) {
    if (object?.[key] !== undefined && object[key] !== null && object[key] !== "") {
      return object[key];
    }
  }

  return null;
}

function pickFirstString(object: LooseRecord | null | undefined, keys: readonly string[]): string | null {
  return stringValue(pickFirstObjectValue(object, keys));
}

function toFiniteNumber(value: unknown): number | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

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
  if (action === "buy") {
    return "bought";
  }

  if (action === "sell") {
    return "sold";
  }

  return "traded";
}

function normalizeAction(value: unknown): WalletTradeAction {
  const normalized = String(value ?? "").toLowerCase();

  if (normalized === "buy" || normalized === "bought") {
    return "buy";
  }

  if (normalized === "sell" || normalized === "sold") {
    return "sell";
  }

  return "unknown";
}

function walletCandidateValues(event: LooseRecord): string[] {
  const candidates = [
    "traderPublicKey",
    "trader",
    "wallet",
    "account",
    "owner",
    "user",
    "buyer",
    "seller",
    "maker",
    "signer",
    "publicKey",
    "targetWallet"
  ];

  return candidates.map((key) => stringValue(event[key])?.trim()).filter((value): value is string => Boolean(value));
}

export function isValidSolanaAddress(value: string): boolean {
  return BASE58_ADDRESS.test(value.trim());
}

export function eventMentionsWatchedWallet(event: LooseRecord, walletAddress: string): boolean {
  return walletCandidateValues(event).includes(walletAddress);
}

export function normalizeWalletTradeData({
  event,
  targetWallet,
  label,
  config
}: {
  event: LooseRecord;
  targetWallet: string;
  label?: string | null;
  config: ExplorerConfig;
}): WalletTradeData {
  const mint = pickFirstString(event, ["mint", "ca", "token", "tokenAddress"]);
  const signature = pickFirstString(event, ["signature", "tx", "txHash", "transaction", "transactionHash"]);
  const action = normalizeAction(pickFirstObjectValue(event, ["txType", "type", "eventType", "action", "side"]));

  return {
    observedAt: new Date().toISOString(),
    targetWallet,
    label: label || null,
    action,
    mint,
    signature,
    solAmount: toFiniteNumber(pickFirstObjectValue(event, ["solAmount", "sol_amount", "amountSol", "quoteAmount", "quote_amount"])),
    tokenAmount: toFiniteNumber(
      pickFirstObjectValue(event, ["tokenAmount", "token_amount", "tokensAmount", "tokenAmountUi", "token_amount_ui"])
    ),
    pool: pickFirstString(event, ["pool", "poolAddress", "bondingCurve", "bondingCurveKey", "raydiumPool"]),
    marketCapSol: toFiniteNumber(pickFirstObjectValue(event, ["marketCapSol", "marketCap"])),
    pumpFunUrl: mint ? `${config.pumpFunBaseUrl}/${mint}` : null,
    solscanTokenUrl: mint ? `${config.solscanBaseUrl}/token/${mint}` : null,
    solscanTxUrl: signature ? `${config.solscanBaseUrl}/tx/${signature}` : null,
    raw: readableWebsocketData(event)
  };
}

export function getWalletTradeEventId(trade: WalletTradeData): string | null {
  if (trade.signature) {
    return ["wallet-trade", trade.signature, trade.targetWallet, trade.action, trade.mint].filter(Boolean).join(":");
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
    `<b>${escapeHtml(walletName)}</b> ${escapeHtml(formatAction(trade.action))} ${trade.mint ? "a Pump.fun token" : "a token"}`
  ];

  lines.push("");
  lines.push("<b>Watched wallet</b>");
  lines.push(`<code>${escapeHtml(trade.targetWallet)}</code>`);

  if (trade.mint) {
    lines.push("");
    lines.push("<b>Contract address</b>");
    lines.push(`<code>${escapeHtml(trade.mint)}</code>`);
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
