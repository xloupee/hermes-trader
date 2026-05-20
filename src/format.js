export function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

export function pickFirstObjectValue(object, keys) {
  for (const key of keys) {
    if (object?.[key] !== undefined && object[key] !== null && object[key] !== "") {
      return object[key];
    }
  }

  return null;
}

export function buildExplorerLinks(event, config) {
  const mint = pickFirstObjectValue(event, ["mint", "ca", "token", "tokenAddress", "address"]);
  const signature = pickFirstObjectValue(event, ["signature", "tx", "txHash", "transaction", "transactionHash"]);

  return {
    mint,
    signature,
    pumpFunUrl: mint ? `${config.pumpFunBaseUrl}/${mint}` : null,
    solscanTokenUrl: mint ? `${config.solscanBaseUrl}/token/${mint}` : null,
    solscanTxUrl: signature ? `${config.solscanBaseUrl}/tx/${signature}` : null
  };
}

function formatNumber(value, maximumFractionDigits = 4) {
  const number = Number(value);

  if (!Number.isFinite(number)) {
    return value;
  }

  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits
  }).format(number);
}

function formatSol(value) {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  const number = toFiniteNumber(value);
  const precision = number !== null && Math.abs(number) < 0.001 ? 9 : 4;

  return `${formatNumber(value, precision)} SOL`;
}

function formatSolUsd(value, solUsdPrice) {
  const sol = formatSol(value);
  const solNumber = toFiniteNumber(value);
  const priceNumber = toFiniteNumber(solUsdPrice);

  if (solNumber === null || priceNumber === null) {
    return sol;
  }

  return `${sol} (${formatUsd(solNumber * priceNumber)})`;
}

function formatUsd(value) {
  const number = Number(value);

  if (!Number.isFinite(number)) {
    return value;
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

function toFiniteNumber(value) {
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function shortenAddress(value) {
  if (!value || value.length <= 16) {
    return value;
  }

  return `${value.slice(0, 6)}...${value.slice(-6)}`;
}

function link(label, url) {
  if (!url) {
    return null;
  }

  return `<a href="${escapeHtml(url)}">${escapeHtml(label)}</a>`;
}

function pickMetadataValue(metadata, keys) {
  return pickFirstObjectValue(metadata, keys);
}

export function extractMigrationData(event, config) {
  const links = buildExplorerLinks(event, config);
  const metadata = config.metadata || {};
  const tokenInfo = config.tokenInfo || {};
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

  return {
    observedAt: new Date().toISOString(),
    eventType: pickFirstObjectValue(event, ["txType", "type", "eventType"]),
    coinAddress: links.mint,
    name: pickFirstObjectValue(event, ["name", "tokenName"]) || pickMetadataValue(metadata, ["name"]) || tokenInfo.name,
    symbol: pickFirstObjectValue(event, ["symbol", "ticker"]) || pickMetadataValue(metadata, ["symbol", "ticker"]) || tokenInfo.symbol,
    description: pickMetadataValue(metadata, ["description"]) || tokenInfo.description,
    imageUrl: pickMetadataValue(metadata, ["image", "image_url", "imageUrl"]) || tokenInfo.image_uri,
    transactionAnalysis: config.transactionAnalysis || null,
    pool: pickFirstObjectValue(event, ["pool", "poolAddress", "bondingCurve", "raydiumPool", "poolCandidate"]) || tokenInfo.pool_address,
    destination: pickFirstObjectValue(event, ["destination", "dex", "exchange", "migrationTarget"]),
    marketCap: effectiveMarketCapSol,
    marketCapSol: effectiveMarketCapSol,
    marketCapUsd,
    solUsdPrice,
    initialBuy: pickFirstObjectValue(event, ["initialBuy"]),
    solAmount: pickFirstObjectValue(event, ["solAmount"]),
    traderPublicKey: pickFirstObjectValue(event, ["traderPublicKey", "creator", "creatorPublicKey", "user"]) || tokenInfo.creator,
    bondingCurveKey: pickFirstObjectValue(event, ["bondingCurveKey", "bondingCurve"]) || tokenInfo.bonding_curve,
    virtualSolInBondingCurve:
      pickFirstObjectValue(event, ["vSolInBondingCurve", "virtualSolInBondingCurve"]) || tokenInfo.virtual_quote_reserves,
    virtualTokensInBondingCurve:
      pickFirstObjectValue(event, ["vTokensInBondingCurve", "virtualTokensInBondingCurve"]) || tokenInfo.virtual_token_reserves,
    uri: pickFirstObjectValue(event, ["uri", "metadataUri", "metadata"]) || tokenInfo.metadata_uri,
    isMayhemMode: pickFirstObjectValue(event, ["is_mayhem_mode", "isMayhemMode"]),
    signature: links.signature,
    pumpFunUrl: links.pumpFunUrl,
    solscanTokenUrl: links.solscanTokenUrl,
    solscanTxUrl: links.solscanTxUrl,
    metadata,
    tokenInfo,
    raw: readableWebsocketData(event)
  };
}

function compactValue(value, depth = 0) {
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

export function readableWebsocketData(event) {
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
    "traderPublicKey",
    "txType",
    "initialBuy",
    "solAmount",
    "vTokensInBondingCurve",
    "vSolInBondingCurve",
    "uri",
    "is_mayhem_mode"
  ];
  const selected = {};

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

export function truncateForTelegram(value, maxLength = 2200) {
  if (value.length <= maxLength) {
    return value;
  }

  return `${value.slice(0, maxLength - 18)}\n... truncated`;
}

export function formatMigrationMessage(event, config) {
  const migration = extractMigrationData(event, config);
  const activeMethods = config.activeSubscriptionMethods || [];
  const isCreateEvent =
    migration.eventType === "create" ||
    (activeMethods.length === 1 && activeMethods.includes("subscribeNewToken")) ||
    config.alertMode === "newtokens";
  const heading = isCreateEvent ? "New Pump.fun token" : "Pumpfun migration detected";
  const tokenName = migration.name || "Unknown token";
  const tokenSymbol = migration.symbol ? ` (${migration.symbol})` : "";

  const lines = [`<b>${heading}</b>`, `<b>${escapeHtml(tokenName)}${escapeHtml(tokenSymbol)}</b>`];

  if (migration.description) {
    lines.push(escapeHtml(String(migration.description).slice(0, 180)));
  }

  if (migration.coinAddress) {
    lines.push("");
    lines.push("<b>Contract address</b>");
    lines.push(`<code>${escapeHtml(migration.coinAddress)}</code>`);
  }

  if (migration.marketCap !== null) {
    const marketCap = migration.marketCapUsd !== null ? formatUsd(migration.marketCapUsd) : formatSol(migration.marketCapSol);
    lines.push("");
    lines.push(`<b>Market cap:</b> ${escapeHtml(marketCap)}`);
  }

  if (migration.transactionAnalysis) {
    const flow = migration.transactionAnalysis;
    const flowLines = [];

    if (flow.networkFeeSol !== null && flow.networkFeeSol !== undefined) {
      flowLines.push(`<b>Network fee:</b> ${escapeHtml(formatSolUsd(flow.networkFeeSol, migration.solUsdPrice))}`);
    }

    for (const recipient of flow.recipients || []) {
      flowLines.push(
        `<b>${escapeHtml(recipient.label)}:</b> +${escapeHtml(formatSolUsd(recipient.deltaSol, migration.solUsdPrice))} <code>${escapeHtml(shortenAddress(recipient.address))}</code>`
      );
    }

    if (flowLines.length > 0) {
      lines.push("");
      lines.push("<b>Fees / SOL flow</b>");
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
    lines.push(`<b>Links:</b> ${fallbackLinks.join(" | ")}`);
  }

  return lines.join("\n");
}

export function buildMigrationReplyMarkup(event, config) {
  const migration = extractMigrationData(event, config);

  if (!migration.coinAddress) {
    return undefined;
  }

  return {
    inline_keyboard: [
      [
        {
          text: "Copy CA",
          copy_text: {
            text: migration.coinAddress
          }
        }
      ]
    ]
  };
}

export function getEventId(event) {
  return pickFirstObjectValue(event, [
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
