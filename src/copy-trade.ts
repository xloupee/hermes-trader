import { escapeHtml } from "./format.js";
import { isSolToTokenBuy } from "./helius-swaps.js";
import { isValidSolanaAddress } from "./wallet-monitor.js";
import type { SubscriberRecord, TelegramReplyMarkup, WalletTradeData } from "./types.js";

export interface CopyTradeConfig {
  copyDefaultSolAmount: number;
  pumpPortalLocalTradeUrl: string;
  pumpPortalLocalSlippage: number;
  pumpPortalLocalPriorityFee: number;
  pumpPortalLocalPool: string;
}

export interface PumpPortalLocalTradeRequest {
  publicKey: string;
  action: "buy";
  mint: string;
  amount: number;
  denominatedInSol: "true";
  slippage: number;
  priorityFee: number;
  pool: string;
}

export interface PumpPortalBuildResult {
  status: "built" | "skipped" | "failed";
  message: string;
  request?: PumpPortalLocalTradeRequest;
  responseStatus?: number;
  responseContentType?: string | null;
  responseBytes?: number;
}

export interface CopyCandidate {
  trade: WalletTradeData;
  copySolAmount: number;
  build: PumpPortalBuildResult;
}

export interface CopyWalletBuildResult {
  copyWallet: string | null;
  copySolAmount: number;
  build: PumpPortalBuildResult;
}

function formatNumber(value: number): string {
  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits: Math.abs(value) < 0.001 ? 9 : 6
  }).format(value);
}

function shortAddress(value: string): string {
  return value.length <= 16 ? value : `${value.slice(0, 6)}...${value.slice(-6)}`;
}

function tokenLabel(trade: WalletTradeData): string {
  return trade.output?.symbol || (trade.mint ? shortAddress(trade.mint) : "Unknown token");
}

export function copySolAmountForSubscriber(subscriber: SubscriberRecord, config: CopyTradeConfig): number {
  return subscriber.copySolAmount ?? config.copyDefaultSolAmount;
}

export function copyWalletsForSubscriber(subscriber: SubscriberRecord): string[] {
  const savedCopyWallets = subscriber.copyWallets || [];
  const copyWallets = savedCopyWallets.length > 0 ? savedCopyWallets : subscriber.copyWallet ? [subscriber.copyWallet] : [];

  return [...new Set(copyWallets.map((wallet) => wallet.trim()).filter(Boolean))];
}

export function isCopyCandidateTrade(trade: WalletTradeData): boolean {
  return isSolToTokenBuy(trade.input, trade.output);
}

function buildPumpPortalLocalTradeRequestForWallet({
  trade,
  copyWallet,
  copySolAmount,
  config
}: {
  trade: WalletTradeData;
  copyWallet: string | null;
  copySolAmount: number;
  config: CopyTradeConfig;
}): PumpPortalBuildResult {
  if (!copyWallet) {
    return {
      status: "skipped",
      message: "PumpPortal build skipped: set /copywallet"
    };
  }

  if (!isValidSolanaAddress(copyWallet)) {
    return {
      status: "skipped",
      message: "PumpPortal build skipped: saved copy wallet is not a valid Solana public address"
    };
  }

  if (!Number.isFinite(copySolAmount) || copySolAmount <= 0) {
    return {
      status: "skipped",
      message: "PumpPortal build skipped: copy amount must be greater than 0"
    };
  }

  const mint = trade.output?.mint || trade.mint;

  if (!mint || !isValidSolanaAddress(mint)) {
    return {
      status: "skipped",
      message: "PumpPortal build skipped: copy token mint is missing or invalid"
    };
  }

  return {
    status: "skipped",
    message: "PumpPortal local request ready",
    request: {
      publicKey: copyWallet,
      action: "buy",
      mint,
      amount: copySolAmount,
      denominatedInSol: "true",
      slippage: config.pumpPortalLocalSlippage,
      priorityFee: config.pumpPortalLocalPriorityFee,
      pool: config.pumpPortalLocalPool
    }
  };
}

export function buildPumpPortalLocalTradeRequest({
  trade,
  subscriber,
  config
}: {
  trade: WalletTradeData;
  subscriber: SubscriberRecord;
  config: CopyTradeConfig;
}): PumpPortalBuildResult {
  const [copyWallet = null] = copyWalletsForSubscriber(subscriber);

  return buildPumpPortalLocalTradeRequestForWallet({
    trade,
    copyWallet,
    copySolAmount: copySolAmountForSubscriber(subscriber, config),
    config
  });
}

export async function buildPumpPortalLocalTransaction({
  trade,
  subscriber,
  config
}: {
  trade: WalletTradeData;
  subscriber: SubscriberRecord;
  config: CopyTradeConfig;
}): Promise<PumpPortalBuildResult> {
  const planned = buildPumpPortalLocalTradeRequest({ trade, subscriber, config });

  if (!planned.request) {
    return planned;
  }

  try {
    const response = await fetch(config.pumpPortalLocalTradeUrl, {
      method: "POST",
      headers: {
        "content-type": "application/json"
      },
      body: JSON.stringify(planned.request)
    });
    const contentType = response.headers.get("content-type");
    const bytes = await response.arrayBuffer();

    if (!response.ok) {
      const errorText = new TextDecoder().decode(bytes).trim();
      return {
        ...planned,
        status: "failed",
        message: errorText
          ? `PumpPortal local tx build failed: HTTP ${response.status} ${errorText}`
          : `PumpPortal local tx build failed: HTTP ${response.status}`,
        responseStatus: response.status,
        responseContentType: contentType,
        responseBytes: bytes.byteLength
      };
    }

    return {
      ...planned,
      status: "built",
      message: `PumpPortal unsigned tx built (${bytes.byteLength} bytes)`,
      responseStatus: response.status,
      responseContentType: contentType,
      responseBytes: bytes.byteLength
    };
  } catch (error) {
    return {
      ...planned,
      status: "failed",
      message: `PumpPortal local tx build failed: ${error instanceof Error ? error.message : String(error)}`
    };
  }
}

async function buildPumpPortalLocalTransactionForWallet({
  trade,
  copyWallet,
  copySolAmount,
  config
}: {
  trade: WalletTradeData;
  copyWallet: string | null;
  copySolAmount: number;
  config: CopyTradeConfig;
}): Promise<CopyWalletBuildResult> {
  const planned = buildPumpPortalLocalTradeRequestForWallet({
    trade,
    copyWallet,
    copySolAmount,
    config
  });

  if (!planned.request) {
    return {
      copyWallet,
      copySolAmount,
      build: planned
    };
  }

  try {
    const response = await fetch(config.pumpPortalLocalTradeUrl, {
      method: "POST",
      headers: {
        "content-type": "application/json"
      },
      body: JSON.stringify(planned.request)
    });
    const contentType = response.headers.get("content-type");
    const bytes = await response.arrayBuffer();

    if (!response.ok) {
      const errorText = new TextDecoder().decode(bytes).trim();
      return {
        copyWallet,
        copySolAmount,
        build: {
          ...planned,
          status: "failed",
          message: errorText
            ? `PumpPortal local tx build failed: HTTP ${response.status} ${errorText}`
            : `PumpPortal local tx build failed: HTTP ${response.status}`,
          responseStatus: response.status,
          responseContentType: contentType,
          responseBytes: bytes.byteLength
        }
      };
    }

    return {
      copyWallet,
      copySolAmount,
      build: {
        ...planned,
        status: "built",
        message: `PumpPortal unsigned tx built (${bytes.byteLength} bytes)`,
        responseStatus: response.status,
        responseContentType: contentType,
        responseBytes: bytes.byteLength
      }
    };
  } catch (error) {
    return {
      copyWallet,
      copySolAmount,
      build: {
        ...planned,
        status: "failed",
        message: `PumpPortal local tx build failed: ${error instanceof Error ? error.message : String(error)}`
      }
    };
  }
}

export async function buildPumpPortalLocalTransactions({
  trade,
  subscriber,
  config
}: {
  trade: WalletTradeData;
  subscriber: SubscriberRecord;
  config: CopyTradeConfig;
}): Promise<CopyWalletBuildResult[]> {
  const copySolAmount = copySolAmountForSubscriber(subscriber, config);
  const copyWallets = copyWalletsForSubscriber(subscriber);

  if (copyWallets.length === 0) {
    return [
      await buildPumpPortalLocalTransactionForWallet({
        trade,
        copyWallet: null,
        copySolAmount,
        config
      })
    ];
  }

  return Promise.all(
    copyWallets.map((copyWallet) =>
      buildPumpPortalLocalTransactionForWallet({
        trade,
        copyWallet,
        copySolAmount,
        config
      })
    )
  );
}

export function formatCopyCandidateMessage(candidate: CopyCandidate | { trade: WalletTradeData; builds: CopyWalletBuildResult[] }): string {
  const { trade } = candidate;
  const builds = "builds" in candidate ? candidate.builds : [{ copyWallet: null, copySolAmount: candidate.copySolAmount, build: candidate.build }];
  const walletName = trade.label || shortAddress(trade.targetWallet);
  const lines = [
    "<b>Copy candidate</b>",
    "",
    `<b>Watched wallet:</b> ${escapeHtml(walletName)}`,
    `<code>${escapeHtml(trade.targetWallet)}</code>`,
    "",
    `<b>Token:</b> ${escapeHtml(tokenLabel(trade))}`
  ];

  if (trade.mint) {
    lines.push(`<b>Contract address:</b> <code>${escapeHtml(trade.mint)}</code>`);
  }

  if (trade.input?.amount !== null && trade.input?.amount !== undefined) {
    lines.push(`<b>Target spent:</b> ${escapeHtml(formatNumber(trade.input.amount))} SOL`);
  }

  if (trade.output?.amount !== null && trade.output?.amount !== undefined) {
    lines.push(`<b>Target received:</b> ${escapeHtml(formatNumber(trade.output.amount))} tokens`);
  }

  lines.push(`<b>Copy wallets:</b> ${builds.filter((entry) => entry.copyWallet).length}`);

  if (trade.source || trade.pool) {
    lines.push(`<b>Source:</b> ${escapeHtml(trade.source || trade.pool || "Unknown")}`);
  }

  lines.push("");

  for (const entry of builds) {
    const copyWallet = entry.copyWallet ? shortAddress(entry.copyWallet) : "not set";
    lines.push(`<b>Copy wallet:</b> ${entry.copyWallet ? `<code>${escapeHtml(copyWallet)}</code>` : copyWallet}`);
    lines.push(`<b>Your copy amount:</b> ${escapeHtml(formatNumber(entry.copySolAmount))} SOL`);
    lines.push(`<b>PumpPortal:</b> ${escapeHtml(entry.build.message)}`);
  }

  lines.push("");
  lines.push("<i>Build-only: not signed, not sent.</i>");

  return lines.join("\n");
}

export function buildCopyCandidateReplyMarkup(trade: WalletTradeData): TelegramReplyMarkup | undefined {
  const rows: TelegramReplyMarkup["inline_keyboard"] = [];

  if (trade.mint) {
    rows.push([
      {
        text: "Copy CA",
        copy_text: {
          text: trade.mint
        }
      }
    ]);
  }

  const links = [
    trade.solscanTxUrl ? { text: "Solscan tx", url: trade.solscanTxUrl } : null,
    trade.solscanTokenUrl ? { text: "Solscan token", url: trade.solscanTokenUrl } : null
  ].filter((button): button is { text: string; url: string } => Boolean(button));

  if (links.length > 0) {
    rows.push(links);
  }

  return rows.length > 0 ? { inline_keyboard: rows } : undefined;
}
