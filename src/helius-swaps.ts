import { readableWebsocketData } from "./format.js";
import { asRecord, isRecord, stringValue } from "./types.js";
import type { ExplorerConfig, LooseRecord, WalletTradeAsset, WalletTradeData } from "./types.js";

const SOL_MINT = "So11111111111111111111111111111111111111112";
const LAMPORTS_PER_SOL = 1_000_000_000;

function numberValue(value: unknown): number | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function tokenAmountValue(value: LooseRecord): number | null {
  return numberValue(value.tokenAmount ?? value.amount ?? value.rawTokenAmount);
}

function nativeSolAmount(value: LooseRecord): number | null {
  const amount = numberValue(value.amount);
  return amount === null ? null : amount / LAMPORTS_PER_SOL;
}

function tokenSymbol(value: LooseRecord, fallback: string | null): string | null {
  return stringValue(value.symbol || value.tokenSymbol || value.name || value.tokenName) || fallback;
}

function tokenMint(value: LooseRecord): string | null {
  return stringValue(value.mint || value.tokenMint);
}

function makeAsset({ mint, symbol, amount }: { mint: string | null; symbol: string | null; amount: number | null }): WalletTradeAsset {
  return {
    mint,
    symbol,
    amount
  };
}

function affectedWallets(event: LooseRecord): string[] {
  const wallets = new Set<string>();
  const feePayer = stringValue(event.feePayer);

  if (feePayer) {
    wallets.add(feePayer);
  }

  for (const transfer of arrayRecords(event.nativeTransfers)) {
    const fromUserAccount = stringValue(transfer.fromUserAccount);
    const toUserAccount = stringValue(transfer.toUserAccount);

    if (fromUserAccount) {
      wallets.add(fromUserAccount);
    }

    if (toUserAccount) {
      wallets.add(toUserAccount);
    }
  }

  for (const transfer of arrayRecords(event.tokenTransfers)) {
    const fromUserAccount = stringValue(transfer.fromUserAccount);
    const toUserAccount = stringValue(transfer.toUserAccount);

    if (fromUserAccount) {
      wallets.add(fromUserAccount);
    }

    if (toUserAccount) {
      wallets.add(toUserAccount);
    }
  }

  for (const account of arrayRecords(event.accountData)) {
    const address = stringValue(account.account);

    if (address) {
      wallets.add(address);
    }
  }

  return [...wallets];
}

function arrayRecords(value: unknown): LooseRecord[] {
  return Array.isArray(value) ? value.filter(isRecord) : [];
}

function pickSwapAssets(event: LooseRecord, targetWallet: string): { input: WalletTradeAsset | null; output: WalletTradeAsset | null } {
  const tokenTransfers = arrayRecords(event.tokenTransfers);
  const nativeTransfers = arrayRecords(event.nativeTransfers);
  const outgoingToken = tokenTransfers.find((transfer) => transfer.fromUserAccount === targetWallet);
  const incomingToken = tokenTransfers.find((transfer) => transfer.toUserAccount === targetWallet);
  const outgoingNative = nativeTransfers.find((transfer) => transfer.fromUserAccount === targetWallet);
  const incomingNative = nativeTransfers.find((transfer) => transfer.toUserAccount === targetWallet);

  const input = outgoingToken
    ? makeAsset({
        mint: tokenMint(outgoingToken),
        symbol: tokenSymbol(outgoingToken, null),
        amount: tokenAmountValue(outgoingToken)
      })
    : outgoingNative
      ? makeAsset({
          mint: SOL_MINT,
          symbol: "SOL",
          amount: nativeSolAmount(outgoingNative)
        })
      : null;
  const output = incomingToken
    ? makeAsset({
        mint: tokenMint(incomingToken),
        symbol: tokenSymbol(incomingToken, null),
        amount: tokenAmountValue(incomingToken)
      })
    : incomingNative
      ? makeAsset({
          mint: SOL_MINT,
          symbol: "SOL",
          amount: nativeSolAmount(incomingNative)
        })
      : null;

  return {
    input: input || pickTokenBalanceAsset(event, targetWallet, "negative") || pickNativeBalanceAsset(event, targetWallet, "negative"),
    output: output || pickTokenBalanceAsset(event, targetWallet, "positive") || pickNativeBalanceAsset(event, targetWallet, "positive")
  };
}

function pickNativeBalanceAsset(event: LooseRecord, targetWallet: string, direction: "negative" | "positive"): WalletTradeAsset | null {
  const account = arrayRecords(event.accountData).find((entry) => entry.account === targetWallet);
  const lamports = numberValue(account?.nativeBalanceChange);

  if (lamports === null) {
    return null;
  }

  if (direction === "negative" && lamports < 0) {
    return makeAsset({ mint: SOL_MINT, symbol: "SOL", amount: Math.abs(lamports) / LAMPORTS_PER_SOL });
  }

  if (direction === "positive" && lamports > 0) {
    return makeAsset({ mint: SOL_MINT, symbol: "SOL", amount: lamports / LAMPORTS_PER_SOL });
  }

  return null;
}

function pickTokenBalanceAsset(event: LooseRecord, targetWallet: string, direction: "negative" | "positive"): WalletTradeAsset | null {
  for (const account of arrayRecords(event.accountData)) {
    for (const change of arrayRecords(account.tokenBalanceChanges)) {
      if (change.userAccount !== targetWallet) {
        continue;
      }

      const rawTokenAmount = asRecord(change.rawTokenAmount);
      const rawAmount = Number(rawTokenAmount.tokenAmount);
      const decimals = Number(rawTokenAmount.decimals || 0);

      if (!Number.isFinite(rawAmount) || !Number.isFinite(decimals)) {
        continue;
      }

      const signedAmount = rawAmount / 10 ** decimals;

      if (direction === "negative" && signedAmount < 0) {
        return makeAsset({
          mint: tokenMint(change),
          symbol: tokenSymbol(change, null),
          amount: Math.abs(signedAmount)
        });
      }

      if (direction === "positive" && signedAmount > 0) {
        return makeAsset({
          mint: tokenMint(change),
          symbol: tokenSymbol(change, null),
          amount: signedAmount
        });
      }
    }
  }

  return null;
}

export function heliusEventMentionsWatchedWallet(event: LooseRecord, walletAddress: string): boolean {
  return affectedWallets(event).includes(walletAddress);
}

export function isHeliusSwapEvent(event: LooseRecord): boolean {
  return String(event.type || "").toUpperCase() === "SWAP";
}

export function isSolToTokenBuy(input: WalletTradeAsset | null, output: WalletTradeAsset | null): boolean {
  return input?.mint === SOL_MINT && Boolean(output?.mint && output.mint !== SOL_MINT);
}

export function normalizeHeliusSwapData({
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
  const { input, output } = pickSwapAssets(event, targetWallet);
  const primaryMint = output?.mint && output.mint !== SOL_MINT ? output.mint : input?.mint && input.mint !== SOL_MINT ? input.mint : output?.mint || input?.mint || null;
  const signature = stringValue(event.signature);
  const source = stringValue(event.source);
  const timestamp = numberValue(event.timestamp);
  const feePayer = stringValue(event.feePayer);
  const nativeAmount = input?.symbol === "SOL" ? input.amount : output?.symbol === "SOL" ? output.amount : null;
  const tokenAmount = output?.symbol !== "SOL" ? (output?.amount ?? null) : input?.symbol !== "SOL" ? (input?.amount ?? null) : null;
  const action = isSolToTokenBuy(input, output) ? "buy" : output?.mint === SOL_MINT && input?.mint !== SOL_MINT ? "sell" : "swap";

  return {
    observedAt: new Date().toISOString(),
    provider: "helius",
    targetWallet,
    label: label || null,
    action,
    mint: primaryMint,
    signature,
    timestamp,
    feePayer,
    source,
    input,
    output,
    solAmount: nativeAmount,
    tokenAmount,
    pool: source,
    marketCapSol: null,
    pumpFunUrl: null,
    solscanTokenUrl: primaryMint ? `${config.solscanBaseUrl}/token/${primaryMint}` : null,
    solscanTxUrl: signature ? `${config.solscanBaseUrl}/tx/${signature}` : null,
    raw: readableWebsocketData({
      ...event,
      events: asRecord(event.events)
    })
  };
}
