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

export function formatMigrationMessage(event, config) {
  const name = pickFirstObjectValue(event, ["name", "tokenName"]);
  const symbol = pickFirstObjectValue(event, ["symbol", "ticker"]);
  const marketCap = pickFirstObjectValue(event, ["marketCapSol", "marketCap", "usdMarketCap"]);
  const pool = pickFirstObjectValue(event, ["pool", "poolAddress", "bondingCurve", "raydiumPool"]);
  const links = buildExplorerLinks(event, config);

  const titleParts = [name, symbol ? `$${symbol}` : null].filter(Boolean);
  const lines = ["<b>Pump.fun migration detected</b>"];

  if (titleParts.length > 0) {
    lines.push(`<b>${escapeHtml(titleParts.join(" / "))}</b>`);
  }

  if (links.mint) {
    lines.push(`<b>Mint:</b> <code>${escapeHtml(links.mint)}</code>`);
  }

  if (pool) {
    lines.push(`<b>Pool:</b> <code>${escapeHtml(pool)}</code>`);
  }

  if (marketCap) {
    lines.push(`<b>Market cap:</b> ${escapeHtml(marketCap)}`);
  }

  if (links.signature) {
    lines.push(`<b>Tx:</b> <code>${escapeHtml(links.signature)}</code>`);
  }

  if (links.pumpFunUrl) {
    lines.push(`<a href="${escapeHtml(links.pumpFunUrl)}">Pump.fun</a>`);
  }

  if (links.solscanTokenUrl) {
    lines.push(`<a href="${escapeHtml(links.solscanTokenUrl)}">Solscan token</a>`);
  }

  if (links.solscanTxUrl) {
    lines.push(`<a href="${escapeHtml(links.solscanTxUrl)}">Solscan tx</a>`);
  }

  if (!links.mint && !links.signature) {
    lines.push("<b>Raw event:</b>");
    lines.push(`<code>${escapeHtml(JSON.stringify(event).slice(0, 2500))}</code>`);
  }

  return lines.join("\n");
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
