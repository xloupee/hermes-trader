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

  return `${formatNumber(value, 4)} SOL`;
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

export function extractMigrationData(event, config) {
  const links = buildExplorerLinks(event, config);

  return {
    observedAt: new Date().toISOString(),
    eventType: pickFirstObjectValue(event, ["txType", "type", "eventType"]),
    coinAddress: links.mint,
    name: pickFirstObjectValue(event, ["name", "tokenName"]),
    symbol: pickFirstObjectValue(event, ["symbol", "ticker"]),
    pool: pickFirstObjectValue(event, ["pool", "poolAddress", "bondingCurve", "raydiumPool", "poolCandidate"]),
    destination: pickFirstObjectValue(event, ["destination", "dex", "exchange", "migrationTarget"]),
    marketCap: pickFirstObjectValue(event, ["marketCapSol", "marketCap", "usdMarketCap"]),
    initialBuy: pickFirstObjectValue(event, ["initialBuy"]),
    solAmount: pickFirstObjectValue(event, ["solAmount"]),
    traderPublicKey: pickFirstObjectValue(event, ["traderPublicKey", "creator", "creatorPublicKey", "user"]),
    bondingCurveKey: pickFirstObjectValue(event, ["bondingCurveKey", "bondingCurve"]),
    virtualSolInBondingCurve: pickFirstObjectValue(event, ["vSolInBondingCurve", "virtualSolInBondingCurve"]),
    virtualTokensInBondingCurve: pickFirstObjectValue(event, ["vTokensInBondingCurve", "virtualTokensInBondingCurve"]),
    uri: pickFirstObjectValue(event, ["uri", "metadataUri", "metadata"]),
    isMayhemMode: pickFirstObjectValue(event, ["is_mayhem_mode", "isMayhemMode"]),
    signature: links.signature,
    pumpFunUrl: links.pumpFunUrl,
    solscanTokenUrl: links.solscanTokenUrl,
    solscanTxUrl: links.solscanTxUrl,
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
  const isCreateEvent = migration.eventType === "create" || config.pumpPortalSubscriptionMethod === "subscribeNewToken";
  const heading = isCreateEvent ? "New Pump.fun token" : "Pump.fun migration detected";
  const tokenName = migration.name || "Unknown token";
  const tokenSymbol = migration.symbol ? ` (${migration.symbol})` : "";

  const lines = [`<b>${heading}</b>`, `<b>${escapeHtml(tokenName)}${escapeHtml(tokenSymbol)}</b>`];

  if (migration.coinAddress) {
    lines.push("");
    lines.push("<b>Contract address</b>");
    lines.push(`<code>${escapeHtml(migration.coinAddress)}</code>`);
  }

  const stats = [];

  if (migration.marketCap !== null) {
    stats.push(`<b>Market cap:</b> ${escapeHtml(formatSol(migration.marketCap))}`);
  }

  if (migration.solAmount !== null) {
    stats.push(`<b>SOL spent:</b> ${escapeHtml(formatSol(migration.solAmount))}`);
  }

  if (migration.initialBuy !== null) {
    stats.push(`<b>Initial buy:</b> ${escapeHtml(formatNumber(migration.initialBuy, 2))} tokens`);
  }

  if (migration.virtualSolInBondingCurve !== null) {
    stats.push(`<b>Curve SOL:</b> ${escapeHtml(formatSol(migration.virtualSolInBondingCurve))}`);
  }

  if (stats.length > 0) {
    lines.push("");
    lines.push("<b>Stats</b>");
    lines.push(...stats);
  }

  const details = [
    migration.pool ? `<b>Pool:</b> ${escapeHtml(migration.pool)}` : null,
    migration.destination ? `<b>Destination:</b> ${escapeHtml(migration.destination)}` : null,
    migration.isMayhemMode !== null ? `<b>Mayhem mode:</b> ${migration.isMayhemMode ? "Yes" : "No"}` : null,
    migration.traderPublicKey
      ? `<b>Creator:</b> <code>${escapeHtml(shortenAddress(migration.traderPublicKey))}</code>`
      : null,
    migration.bondingCurveKey
      ? `<b>Bonding curve:</b> <code>${escapeHtml(shortenAddress(migration.bondingCurveKey))}</code>`
      : null,
    migration.virtualTokensInBondingCurve !== null
      ? `<b>Curve tokens:</b> ${escapeHtml(formatNumber(migration.virtualTokensInBondingCurve, 2))}`
      : null
  ].filter(Boolean);

  if (details.length > 0) {
    lines.push("");
    lines.push("<b>Details</b>");
    lines.push(...details);
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
