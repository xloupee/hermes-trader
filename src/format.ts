import { asRecord, stringValue } from "./types.js";
import type {
  ExplorerConfig,
  LooseRecord,
  MigrationData,
  MigrationFormatConfig,
  TelegramInlineKeyboardButton,
  TelegramReplyMarkup,
  TokenSocialLinks
} from "./types.js";

interface ExplorerLinks {
  mint: string | null;
  signature: string | null;
  pumpFunUrl: string | null;
  solscanTokenUrl: string | null;
  solscanTxUrl: string | null;
}

export function escapeHtml(value: unknown): string {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

export function pickFirstObjectValue(object: LooseRecord | null | undefined, keys: readonly string[]): unknown {
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

export function buildExplorerLinks(event: LooseRecord, config: ExplorerConfig): ExplorerLinks {
  const mint = pickFirstString(event, ["mint", "ca", "token", "tokenAddress", "address"]);
  const signature = pickFirstString(event, ["signature", "tx", "txHash", "transaction", "transactionHash"]);

  return {
    mint,
    signature,
    pumpFunUrl: mint ? `${config.pumpFunBaseUrl}/${mint}` : null,
    solscanTokenUrl: mint ? `${config.solscanBaseUrl}/token/${mint}` : null,
    solscanTxUrl: signature ? `${config.solscanBaseUrl}/tx/${signature}` : null
  };
}

function formatNumber(value: unknown, maximumFractionDigits = 4): string {
  const number = Number(value);

  if (!Number.isFinite(number)) {
    return String(value);
  }

  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits
  }).format(number);
}

function formatSol(value: unknown): string | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  const number = toFiniteNumber(value);
  const precision = number !== null && Math.abs(number) < 0.001 ? 9 : 4;

  return `${formatNumber(value, precision)} SOL`;
}

function formatSolUsd(value: unknown, solUsdPrice: unknown): string | null {
  const sol = formatSol(value);
  const solNumber = toFiniteNumber(value);
  const priceNumber = toFiniteNumber(solUsdPrice);

  if (solNumber === null || priceNumber === null) {
    return sol;
  }

  return `${sol} (${formatUsd(solNumber * priceNumber)})`;
}

function formatUsd(value: unknown): string {
  const number = Number(value);

  if (!Number.isFinite(number)) {
    return String(value);
  }

  if (number >= 1000000) {
    return `$${formatNumber(number / 1000000, 2)}M`;
  }

  if (number >= 1000) {
    return `$${formatNumber(number / 1000, 2)}K`;
  }

  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: number >= 100 ? 0 : 2
  }).format(number);
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

function pickCreatorAddress(event: LooseRecord, tokenInfo: LooseRecord): string | null {
  return pickFirstString(event, ["creator", "creatorPublicKey", "creatorAddress"]) || stringValue(tokenInfo.creator);
}

function link(label: string, url: string | null): string | null {
  if (!url) {
    return null;
  }

  return `<a href="${escapeHtml(url)}">${escapeHtml(label)}</a>`;
}

function normalizeHttpUrl(value: unknown): string | null {
  const raw = stringValue(value)?.trim();

  if (!raw) {
    return null;
  }

  try {
    const url = new URL(raw);
    return url.protocol === "http:" || url.protocol === "https:" ? url.toString() : null;
  } catch {
    return null;
  }
}

function normalizeTelegramUrl(value: unknown): string | null {
  const raw = stringValue(value)?.trim();

  if (!raw) {
    return null;
  }

  if (raw.startsWith("@")) {
    return normalizeHttpUrl(`https://t.me/${raw.slice(1)}`);
  }

  if (/^(t\.me|telegram\.me)\//i.test(raw)) {
    return normalizeHttpUrl(`https://${raw}`);
  }

  return normalizeHttpUrl(raw);
}

function pickFirstNormalizedUrl(
  object: LooseRecord | null | undefined,
  keys: readonly string[],
  normalize: (value: unknown) => string | null
): string | null {
  for (const key of keys) {
    const normalized = normalize(object?.[key]);

    if (normalized) {
      return normalized;
    }
  }

  return null;
}

function extractTokenSocialLinks(metadata: LooseRecord, tokenInfo: LooseRecord): TokenSocialLinks {
  const sources = [metadata, tokenInfo];
  const pickSocialUrl = (keys: readonly string[], normalize: (value: unknown) => string | null = normalizeHttpUrl) => {
    for (const source of sources) {
      const normalized = pickFirstNormalizedUrl(source, keys, normalize);

      if (normalized) {
        return normalized;
      }
    }

    return null;
  };

  return {
    twitterUrl: pickSocialUrl(["twitter", "x", "twitterUrl", "xUrl"]),
    telegramUrl: pickSocialUrl(["telegram", "telegramUrl"], normalizeTelegramUrl),
    websiteUrl: pickSocialUrl(["website", "websiteUrl", "external_url", "externalUrl"])
  };
}

function pickMetadataString(metadata: LooseRecord, keys: readonly string[]): string | null {
  return pickFirstString(metadata, keys);
}

function pickBooleanValue(object: LooseRecord | null | undefined, keys: readonly string[]): boolean | null {
  const value = pickFirstObjectValue(object, keys);

  if (value === null) {
    return null;
  }

  if (typeof value === "boolean") {
    return value;
  }

  if (typeof value === "string") {
    const normalized = value.toLowerCase();

    if (normalized === "true" || normalized === "yes" || normalized === "1") {
      return true;
    }

    if (normalized === "false" || normalized === "no" || normalized === "0") {
      return false;
    }
  }

  return null;
}

function formatBooleanStatus(value: boolean | null): string {
  if (value === true) {
    return "Enabled";
  }

  if (value === false) {
    return "Disabled";
  }

  return "Unknown";
}

function formatCreatorFeeStatus(migration: MigrationData): string {
  if (!migration.creatorAddress) {
    return "Unknown";
  }

  return `Creator eligible for <code>${escapeHtml(migration.creatorAddress)}</code>`;
}

function hasObjectData(value: LooseRecord): boolean {
  return Boolean(value && typeof value === "object" && Object.keys(value).length > 0);
}

export function extractMigrationData(event: LooseRecord, config: MigrationFormatConfig): MigrationData {
  const links = buildExplorerLinks(event, config);
  const metadata = asRecord(config.metadata);
  const tokenInfo = asRecord(config.tokenInfo);
  const socialLinks = extractTokenSocialLinks(metadata, tokenInfo);
  const marketCapSol = pickFirstObjectValue(event, ["marketCapSol", "marketCap"]);
  const tokenInfoMarketCapSol = pickFirstObjectValue(tokenInfo, ["market_cap_quote", "market_cap"]);
  const effectiveMarketCapSol = marketCapSol ?? tokenInfoMarketCapSol;
  const explicitMarketCapUsd =
    pickFirstObjectValue(event, ["usdMarketCap", "marketCapUsd", "marketCapUSD"]) ??
    pickFirstObjectValue(tokenInfo, ["usd_market_cap", "market_cap_usd", "usdMarketCap", "marketCapUsd", "marketCapUSD"]);
  const solUsdPrice = toFiniteNumber(config.solUsdPrice);
  const marketCapSolNumber = toFiniteNumber(effectiveMarketCapSol);
  const marketCapUsd =
    explicitMarketCapUsd ?? (marketCapSolNumber !== null && solUsdPrice !== null ? marketCapSolNumber * solUsdPrice : null);
  const creatorAddress = pickCreatorAddress(event, tokenInfo);
  const agentBuybacksEnabled =
    pickBooleanValue(event, ["tokenized_agent", "tokenizedAgent", "agentBuybacksEnabled"]) ??
    pickBooleanValue(tokenInfo, ["tokenized_agent", "tokenizedAgent", "agentBuybacksEnabled"]) ??
    (hasObjectData(tokenInfo) ? false : null);

  return {
    observedAt: new Date().toISOString(),
    eventType: pickFirstString(event, ["txType", "type", "eventType"]),
    coinAddress: links.mint,
    name: pickFirstString(event, ["name", "tokenName"]) || pickMetadataString(metadata, ["name"]) || stringValue(tokenInfo.name),
    symbol: pickFirstString(event, ["symbol", "ticker"]) || pickMetadataString(metadata, ["symbol", "ticker"]) || stringValue(tokenInfo.symbol),
    description: pickMetadataString(metadata, ["description"]) || stringValue(tokenInfo.description),
    imageUrl: pickMetadataString(metadata, ["image", "image_url", "imageUrl"]) || stringValue(tokenInfo.image_uri),
    cashbackEnabled: pickBooleanValue(event, ["is_cashback_enabled", "isCashbackEnabled", "cashbackEnabled"]) ??
      pickBooleanValue(tokenInfo, ["is_cashback_enabled", "isCashbackEnabled", "cashbackEnabled"]),
    agentBuybacksEnabled,
    creatorFeeEligible: Boolean(creatorAddress),
    creatorAddress,
    transactionAnalysis: config.transactionAnalysis || null,
    pool: pickFirstString(event, ["pool", "poolAddress", "bondingCurve", "raydiumPool", "poolCandidate"]) || stringValue(tokenInfo.pool_address),
    destination: pickFirstString(event, ["destination", "dex", "exchange", "migrationTarget"]),
    marketCap: effectiveMarketCapSol,
    marketCapSol: effectiveMarketCapSol,
    marketCapUsd,
    solUsdPrice,
    initialBuy: pickFirstObjectValue(event, ["initialBuy"]),
    solAmount: pickFirstObjectValue(event, ["solAmount"]),
    traderPublicKey: pickFirstString(event, ["traderPublicKey", "creator", "creatorPublicKey", "user"]) || stringValue(tokenInfo.creator),
    bondingCurveKey: pickFirstString(event, ["bondingCurveKey", "bondingCurve"]) || stringValue(tokenInfo.bonding_curve),
    virtualSolInBondingCurve:
      pickFirstObjectValue(event, ["vSolInBondingCurve", "virtualSolInBondingCurve"]) || tokenInfo.virtual_quote_reserves,
    virtualTokensInBondingCurve:
      pickFirstObjectValue(event, ["vTokensInBondingCurve", "virtualTokensInBondingCurve"]) || tokenInfo.virtual_token_reserves,
    uri: pickFirstString(event, ["uri", "metadataUri", "metadata"]) || stringValue(tokenInfo.metadata_uri),
    isMayhemMode: pickFirstObjectValue(event, ["is_mayhem_mode", "isMayhemMode"]),
    signature: links.signature,
    pumpFunUrl: links.pumpFunUrl,
    solscanTokenUrl: links.solscanTokenUrl,
    solscanTxUrl: links.solscanTxUrl,
    socialLinks,
    metadata,
    tokenInfo,
    raw: readableWebsocketData(event)
  };
}

function compactValue(value: unknown, depth = 0): unknown {
  if (depth > 3) {
    return "[nested data]";
  }

  if (Array.isArray(value)) {
    return value.slice(0, 20).map((item) => compactValue(item, depth + 1));
  }

  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .slice(0, 40)
        .map(([key, nestedValue]) => [key, compactValue(nestedValue, depth + 1)])
    );
  }

  if (typeof value === "string" && value.length > 500) {
    return `${value.slice(0, 500)}...`;
  }

  return value;
}

export function readableWebsocketData(event: LooseRecord): LooseRecord {
  const priorityKeys = [
    "signature",
    "tx",
    "txHash",
    "transaction",
    "mint",
    "ca",
    "token",
    "tokenAddress",
    "name",
    "symbol",
    "ticker",
    "pool",
    "poolAddress",
    "bondingCurve",
    "bondingCurveKey",
    "raydiumPool",
    "marketCapSol",
    "marketCap",
    "usdMarketCap",
    "creator",
    "creatorPublicKey",
    "creatorAddress",
    "traderPublicKey",
    "txType",
    "initialBuy",
    "solAmount",
    "vTokensInBondingCurve",
    "vSolInBondingCurve",
    "uri",
    "is_cashback_enabled",
    "tokenized_agent",
    "is_mayhem_mode"
  ];
  const selected: LooseRecord = {};

  for (const key of priorityKeys) {
    if (event?.[key] !== undefined && event[key] !== null && event[key] !== "") {
      selected[key] = compactValue(event[key]);
    }
  }

  for (const [key, value] of Object.entries(event || {})) {
    if (selected[key] === undefined) {
      selected[key] = compactValue(value);
    }
  }

  return selected;
}

export function truncateForTelegram(value: string, maxLength = 2200): string {
  if (value.length <= maxLength) {
    return value;
  }

  return `${value.slice(0, maxLength - 18)}\n... truncated`;
}

export function formatMigrationMessage(event: LooseRecord, config: MigrationFormatConfig): string {
  const migration = extractMigrationData(event, config);
  const activeMethods = config.activeSubscriptionMethods || [];
  const isCreateEvent =
    migration.eventType === "create" ||
    (activeMethods.length === 1 && activeMethods.includes("subscribeNewToken")) ||
    config.alertMode === "newtokens";
  const heading = isCreateEvent ? "🌱 New Pump.fun Token" : "🚀 Pump.fun Migration";
  const tokenName = migration.name || "Unknown token";
  const tokenSymbol = migration.symbol ? ` (${migration.symbol})` : "";

  const lines = [`<b>${heading}</b>`, `<b>🪙 ${escapeHtml(tokenName)}${escapeHtml(tokenSymbol)}</b>`];

  if (migration.description) {
    lines.push(escapeHtml(String(migration.description).slice(0, 180)));
  }

  if (migration.coinAddress) {
    lines.push("");
    lines.push("<b>📋 Contract Address</b>");
    lines.push(`<code>${escapeHtml(migration.coinAddress)}</code>`);
  }

  if (migration.marketCap !== null) {
    const marketCap = migration.marketCapUsd !== null ? formatUsd(migration.marketCapUsd) : formatSol(migration.marketCapSol);
    lines.push("");
    lines.push(`<b>💵 Market Cap:</b> ${escapeHtml(marketCap)}`);
  }

  lines.push(`<b>💸 Cashback:</b> ${formatBooleanStatus(migration.cashbackEnabled)}`);
  lines.push(`<b>🤖 Agent Buybacks:</b> ${formatBooleanStatus(migration.agentBuybacksEnabled)}`);
  lines.push(`<b>👤 Creator Fees:</b> ${formatCreatorFeeStatus(migration)}`);

  if (migration.transactionAnalysis) {
    const flow = migration.transactionAnalysis;
    const flowLines = [];

    if (flow.networkFeeSol !== null && flow.networkFeeSol !== undefined) {
      flowLines.push(`<b>Network Fee:</b> ${escapeHtml(formatSolUsd(flow.networkFeeSol, migration.solUsdPrice))}`);
    }

    for (const recipient of flow.recipients || []) {
      flowLines.push(
        `<b>${escapeHtml(recipient.label)}:</b> +${escapeHtml(formatSolUsd(recipient.deltaSol, migration.solUsdPrice))} <code>${escapeHtml(shortenAddress(recipient.address))}</code>`
      );
    }

    if (flowLines.length > 0) {
      lines.push("");
      lines.push("<b>💧 Fees / SOL Flow</b>");
      lines.push(...flowLines);
    }
  }

  const fallbackLinks = [
    link("Pump.fun", migration.pumpFunUrl),
    link("Solscan token", migration.solscanTokenUrl),
    link("Solscan tx", migration.solscanTxUrl)
  ].filter(Boolean);

  if (fallbackLinks.length > 0) {
    lines.push("");
    lines.push(`<b>🔗 Links:</b> ${fallbackLinks.join(" | ")}`);
  }

  return lines.join("\n");
}

export function buildMigrationReplyMarkup(event: LooseRecord, config: MigrationFormatConfig): TelegramReplyMarkup | undefined {
  const migration = extractMigrationData(event, config);

  if (!migration.coinAddress) {
    return undefined;
  }

  const socialButtons: TelegramInlineKeyboardButton[] = [];

  if (migration.socialLinks.twitterUrl) {
    socialButtons.push({ text: "X", url: migration.socialLinks.twitterUrl });
  }

  if (migration.socialLinks.telegramUrl) {
    socialButtons.push({ text: "Telegram", url: migration.socialLinks.telegramUrl });
  }

  if (migration.socialLinks.websiteUrl) {
    socialButtons.push({ text: "Website", url: migration.socialLinks.websiteUrl });
  }

  const inlineKeyboard: TelegramInlineKeyboardButton[][] = [
    [
      {
        text: "📋 Copy CA",
        copy_text: {
          text: migration.coinAddress
        }
      }
    ]
  ];

  if (socialButtons.length > 0) {
    inlineKeyboard.push(socialButtons);
  }

  return {
    inline_keyboard: inlineKeyboard
  };
}

export function getEventId(event: LooseRecord): string | null {
  return pickFirstString(event, [
    "signature",
    "tx",
    "txHash",
    "transaction",
    "transactionHash",
    "mint",
    "ca",
    "token",
    "tokenAddress",
    "address"
  ]);
}
