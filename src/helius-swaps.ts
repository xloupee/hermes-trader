import { readableWebsocketData } from "./format.js";
import { asRecord, isRecord, stringValue } from "./types.js";
import type { ExplorerConfig, LooseRecord, WalletTradeAction, WalletTradeAsset, WalletTradeData } from "./types.js";

const SOL_MINT = "So11111111111111111111111111111111111111112";
const LAMPORTS_PER_SOL = 1_000_000_000;
const MIN_COPYABLE_NATIVE_SOL_INPUT = 0.001;

type AssetSource = "native-transfer" | "native-balance-change" | "token-transfer" | "token-balance-change";

interface AssetCandidate extends WalletTradeAsset {
  source: AssetSource;
}

interface SwapAssetRoute {
  input: WalletTradeAsset | null;
  output: WalletTradeAsset | null;
  action: WalletTradeAction;
  reason: string | null;
}

function numberValue(value: unknown): number | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function tokenAmountValue(value: LooseRecord): number | null {
  const directAmount = numberValue(value.tokenAmount ?? value.amount);

  if (directAmount !== null) {
    return directAmount;
  }

  const rawTokenAmount = asRecord(value.rawTokenAmount);
  const rawAmount = numberValue(rawTokenAmount.tokenAmount);
  const decimals = numberValue(rawTokenAmount.decimals);

  if (rawAmount !== null && decimals !== null) {
    return rawAmount / 10 ** decimals;
  }

  return numberValue(value.rawTokenAmount);
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

function makeCandidate({
  mint,
  symbol,
  amount,
  source
}: {
  mint: string | null;
  symbol: string | null;
  amount: number | null;
  source: AssetSource;
}): AssetCandidate {
  return {
    mint,
    symbol,
    amount,
    source
  };
}

function stripSource(asset: AssetCandidate | null): WalletTradeAsset | null {
  if (!asset) {
    return null;
  }

  return makeAsset({
    mint: asset.mint,
    symbol: asset.symbol,
    amount: asset.amount
  });
}

function isSolMint(mint: string | null): boolean {
  return mint === SOL_MINT;
}

function accountMatches(record: LooseRecord, targetWallet: string): boolean {
  return (
    stringValue(record.account) === targetWallet ||
    stringValue(record.userAccount) === targetWallet ||
    stringValue(record.fromUserAccount) === targetWallet ||
    stringValue(record.toUserAccount) === targetWallet
  );
}

function structuredSwapEvent(event: LooseRecord): LooseRecord | null {
  const swap = asRecord(asRecord(event.events).swap);
  return Object.keys(swap).length > 0 ? swap : null;
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

function structuredNativeAsset(
  value: unknown,
  targetWallet: string,
  source: AssetSource
): AssetCandidate | null {
  const record = asRecord(value);

  if (!accountMatches(record, targetWallet)) {
    return null;
  }

  const amount = nativeSolAmount(record);

  if (amount === null || amount <= 0) {
    return null;
  }

  return makeCandidate({
    mint: SOL_MINT,
    symbol: "SOL",
    amount,
    source
  });
}

function structuredTokenAssets(value: unknown, targetWallet: string, source: AssetSource): AssetCandidate[] {
  return arrayRecords(value)
    .filter((asset) => accountMatches(asset, targetWallet))
    .map((asset) => {
      const mint = tokenMint(asset);

      if (!mint) {
        return null;
      }

      const amount = tokenAmountValue(asset);

      return makeCandidate({
        mint,
        symbol: tokenSymbol(asset, isSolMint(mint) ? "SOL" : null),
        amount: amount === null ? null : Math.abs(amount),
        source
      });
    })
    .filter((asset): asset is AssetCandidate => asset !== null);
}

function pickStructuredSwapAssets(event: LooseRecord, targetWallet: string): SwapAssetRoute | null {
  const swap = structuredSwapEvent(event);

  if (!swap) {
    return null;
  }

  const nativeInput = structuredNativeAsset(swap.nativeInput, targetWallet, "native-balance-change");
  const nativeOutput = structuredNativeAsset(swap.nativeOutput, targetWallet, "native-balance-change");
  const tokenInputs = mergeTokenAssets(structuredTokenAssets(swap.tokenInputs, targetWallet, "token-balance-change"));
  const tokenOutputs = mergeTokenAssets(structuredTokenAssets(swap.tokenOutputs, targetWallet, "token-balance-change"));
  const nonSolTokenInputs = tokenInputs.filter((asset) => !isSolMint(asset.mint));
  const nonSolTokenOutputs = tokenOutputs.filter((asset) => !isSolMint(asset.mint));
  const hasWrappedSolToken = [...tokenInputs, ...tokenOutputs].some((asset) => isSolMint(asset.mint));
  const fallbackInput = nonSolTokenInputs.length === 1 ? nonSolTokenInputs[0] : nativeInput;
  const fallbackOutput = nonSolTokenOutputs.length === 1 ? nonSolTokenOutputs[0] : nativeOutput;

  if (hasWrappedSolToken) {
    return {
      input: stripSource(fallbackInput),
      output: stripSource(fallbackOutput),
      action: "unknown",
      reason: "watched wallet used wrapped SOL token transfers"
    };
  }

  if (nativeInput && nonSolTokenOutputs.length > 1 && nonSolTokenInputs.length === 0) {
    return {
      input: stripSource(nativeInput),
      output: null,
      action: "unknown",
      reason: "watched wallet received multiple output token mints"
    };
  }

  if (nativeInput && nativeOutput && nonSolTokenOutputs.length > 0) {
    return {
      input: stripSource(nativeInput),
      output: nonSolTokenOutputs.length === 1 ? stripSource(nonSolTokenOutputs[0]) : null,
      action: "unknown",
      reason: "watched wallet has native SOL movement on both sides of the swap"
    };
  }

  if (nativeInput && nonSolTokenOutputs.length === 1 && nonSolTokenInputs.length === 0 && !nativeOutput) {
    if (nativeInput.amount !== null && nativeInput.amount < MIN_COPYABLE_NATIVE_SOL_INPUT) {
      return {
        input: stripSource(nativeInput),
        output: stripSource(nonSolTokenOutputs[0]),
        action: "unknown",
        reason: "watched wallet native SOL transfer is too small to prove it is the swap input"
      };
    }

    return {
      input: stripSource(nativeInput),
      output: stripSource(nonSolTokenOutputs[0]),
      action: "buy",
      reason: null
    };
  }

  if (nonSolTokenInputs.length === 1 && nativeOutput && nonSolTokenOutputs.length === 0 && !nativeInput) {
    return {
      input: stripSource(nonSolTokenInputs[0]),
      output: stripSource(nativeOutput),
      action: "sell",
      reason: null
    };
  }

  if (nonSolTokenInputs.length === 1 && nonSolTokenOutputs.length === 1 && !nativeInput && !nativeOutput) {
    return {
      input: stripSource(nonSolTokenInputs[0]),
      output: stripSource(nonSolTokenOutputs[0]),
      action: "swap",
      reason: "watched wallet swapped one token for another token"
    };
  }

  return {
    input: stripSource(fallbackInput),
    output: stripSource(fallbackOutput),
    action: "unknown",
    reason: "structured Helius swap route is ambiguous for the watched wallet"
  };
}

function pickSwapAssets(event: LooseRecord, targetWallet: string): SwapAssetRoute {
  const structuredRoute = pickStructuredSwapAssets(event, targetWallet);

  if (structuredRoute) {
    return structuredRoute;
  }

  const outgoingNativeTransfer = combineNativeAssets(nativeTransferAssets(event, targetWallet, "negative"));
  const incomingNativeTransfer = combineNativeAssets(nativeTransferAssets(event, targetWallet, "positive"));
  const outgoingNativeBalance = pickNativeBalanceAsset(event, targetWallet, "negative");
  const incomingNativeBalance = pickNativeBalanceAsset(event, targetWallet, "positive");
  const outgoingTokens = tokenAssets(event, targetWallet, "negative");
  const incomingTokens = tokenAssets(event, targetWallet, "positive");
  const outgoingNonSolTokens = outgoingTokens.filter((asset) => !isSolMint(asset.mint));
  const incomingNonSolTokens = incomingTokens.filter((asset) => !isSolMint(asset.mint));
  const hasWrappedSolTokenTransfer = [...outgoingTokens, ...incomingTokens].some((asset) => isSolMint(asset.mint));
  const hasUnknownRelevantTokenTransfer = relevantTokenRecords(event, targetWallet).some((transfer) => !tokenMint(transfer));
  const hasUnknownRelevantTokenBalanceChange = relevantTokenBalanceChanges(event, targetWallet).some((change) => !tokenMint(change));

  const fallbackInput =
    outgoingNonSolTokens.length === 1 ? outgoingNonSolTokens[0] : outgoingNativeTransfer || outgoingNativeBalance || null;
  const fallbackOutput =
    incomingNonSolTokens.length === 1 ? incomingNonSolTokens[0] : incomingNativeTransfer || incomingNativeBalance || null;

  if (hasUnknownRelevantTokenTransfer || hasUnknownRelevantTokenBalanceChange) {
    return {
      input: stripSource(fallbackInput),
      output: stripSource(fallbackOutput),
      action: "unknown",
      reason: "watched wallet has a token movement without a mint"
    };
  }

  if (hasWrappedSolTokenTransfer) {
    return {
      input: stripSource(fallbackInput),
      output: stripSource(fallbackOutput),
      action: "unknown",
      reason: "watched wallet used wrapped SOL token transfers"
    };
  }

  if (outgoingNativeTransfer && incomingNonSolTokens.length > 1 && outgoingNonSolTokens.length === 0) {
    return {
      input: stripSource(outgoingNativeTransfer),
      output: null,
      action: "unknown",
      reason: "watched wallet received multiple output token mints"
    };
  }

  if (outgoingNativeTransfer && (incomingNativeTransfer || incomingNativeBalance) && incomingNonSolTokens.length > 0) {
    return {
      input: stripSource(outgoingNativeTransfer),
      output: incomingNonSolTokens.length === 1 ? stripSource(incomingNonSolTokens[0]) : null,
      action: "unknown",
      reason: "watched wallet has native SOL movement on both sides of the swap"
    };
  }

  if (outgoingNativeTransfer && incomingNonSolTokens.length === 1 && outgoingNonSolTokens.length === 0) {
    if (outgoingNativeTransfer.amount !== null && outgoingNativeTransfer.amount < MIN_COPYABLE_NATIVE_SOL_INPUT) {
      return {
        input: stripSource(outgoingNativeTransfer),
        output: stripSource(incomingNonSolTokens[0]),
        action: "unknown",
        reason: "watched wallet native SOL transfer is too small to prove it is the swap input"
      };
    }

    return {
      input: stripSource(outgoingNativeTransfer),
      output: stripSource(incomingNonSolTokens[0]),
      action: "unknown",
      reason: "watched wallet SOL-to-token route lacks structured Helius swap proof"
    };
  }

  if (outgoingNonSolTokens.length === 1 && incomingNativeTransfer && incomingNonSolTokens.length === 0) {
    return {
      input: stripSource(outgoingNonSolTokens[0]),
      output: stripSource(incomingNativeTransfer),
      action: "sell",
      reason: null
    };
  }

  if (outgoingNonSolTokens.length === 1 && incomingNonSolTokens.length === 1) {
    return {
      input: stripSource(outgoingNonSolTokens[0]),
      output: stripSource(incomingNonSolTokens[0]),
      action: "swap",
      reason: "watched wallet swapped one token for another token"
    };
  }

  return {
    input: stripSource(fallbackInput),
    output: stripSource(fallbackOutput),
    action: "unknown",
    reason: "watched wallet swap route is ambiguous"
  };
}

function nativeTransferAssets(event: LooseRecord, targetWallet: string, direction: "negative" | "positive"): AssetCandidate[] {
  return arrayRecords(event.nativeTransfers)
    .filter((transfer) =>
      direction === "negative"
        ? stringValue(transfer.fromUserAccount) === targetWallet
        : stringValue(transfer.toUserAccount) === targetWallet
    )
    .map((transfer) => {
      const amount = nativeSolAmount(transfer);

      if (amount === null || amount <= 0) {
        return null;
      }

      return makeCandidate({
        mint: SOL_MINT,
        symbol: "SOL",
        amount,
        source: "native-transfer"
      });
    })
    .filter((asset): asset is AssetCandidate => asset !== null);
}

function combineNativeAssets(assets: AssetCandidate[]): AssetCandidate | null {
  if (assets.length === 0) {
    return null;
  }

  const amount = assets.every((asset) => asset.amount !== null)
    ? assets.reduce((sum, asset) => sum + (asset.amount || 0), 0)
    : null;

  return makeCandidate({
    mint: SOL_MINT,
    symbol: "SOL",
    amount,
    source: assets[0]?.source || "native-transfer"
  });
}

function pickNativeBalanceAsset(event: LooseRecord, targetWallet: string, direction: "negative" | "positive"): AssetCandidate | null {
  const account = arrayRecords(event.accountData).find((entry) => entry.account === targetWallet);
  const lamports = numberValue(account?.nativeBalanceChange);

  if (lamports === null) {
    return null;
  }

  if (direction === "negative" && lamports < 0) {
    return makeCandidate({
      mint: SOL_MINT,
      symbol: "SOL",
      amount: Math.abs(lamports) / LAMPORTS_PER_SOL,
      source: "native-balance-change"
    });
  }

  if (direction === "positive" && lamports > 0) {
    return makeCandidate({
      mint: SOL_MINT,
      symbol: "SOL",
      amount: lamports / LAMPORTS_PER_SOL,
      source: "native-balance-change"
    });
  }

  return null;
}

function relevantTokenRecords(event: LooseRecord, targetWallet: string): LooseRecord[] {
  return arrayRecords(event.tokenTransfers).filter(
    (transfer) =>
      stringValue(transfer.fromUserAccount) === targetWallet ||
      stringValue(transfer.toUserAccount) === targetWallet
  );
}

function relevantTokenBalanceChanges(event: LooseRecord, targetWallet: string): LooseRecord[] {
  return arrayRecords(event.accountData).flatMap((account) =>
    arrayRecords(account.tokenBalanceChanges).filter((change) => stringValue(change.userAccount) === targetWallet)
  );
}

function tokenTransferAssets(event: LooseRecord, targetWallet: string, direction: "negative" | "positive"): AssetCandidate[] {
  return arrayRecords(event.tokenTransfers)
    .filter((transfer) =>
      direction === "negative"
        ? stringValue(transfer.fromUserAccount) === targetWallet
        : stringValue(transfer.toUserAccount) === targetWallet
    )
    .map((transfer) => {
      const mint = tokenMint(transfer);

      if (!mint) {
        return null;
      }

      return makeCandidate({
        mint,
        symbol: tokenSymbol(transfer, isSolMint(mint) ? "SOL" : null),
        amount: tokenAmountValue(transfer),
        source: "token-transfer"
      });
    })
    .filter((asset): asset is AssetCandidate => asset !== null);
}

function tokenBalanceAssets(event: LooseRecord, targetWallet: string, direction: "negative" | "positive"): AssetCandidate[] {
  const assets: AssetCandidate[] = [];

  for (const account of arrayRecords(event.accountData)) {
    for (const change of arrayRecords(account.tokenBalanceChanges)) {
      if (stringValue(change.userAccount) !== targetWallet) {
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
        assets.push(makeCandidate({
          mint: tokenMint(change),
          symbol: tokenSymbol(change, null),
          amount: Math.abs(signedAmount),
          source: "token-balance-change"
        }));
      }

      if (direction === "positive" && signedAmount > 0) {
        assets.push(makeCandidate({
          mint: tokenMint(change),
          symbol: tokenSymbol(change, null),
          amount: signedAmount,
          source: "token-balance-change"
        }));
      }
    }
  }

  return assets;
}

function mergeTokenAssets(assets: AssetCandidate[]): AssetCandidate[] {
  const byMint = new Map<string, AssetCandidate>();

  for (const asset of assets) {
    if (!asset.mint) {
      continue;
    }

    const existing = byMint.get(asset.mint);

    if (!existing) {
      byMint.set(asset.mint, { ...asset });
      continue;
    }

    byMint.set(asset.mint, {
      ...existing,
      symbol: existing.symbol || asset.symbol,
      amount:
        existing.amount !== null && asset.amount !== null
          ? existing.amount + asset.amount
          : existing.amount ?? asset.amount
    });
  }

  return [...byMint.values()];
}

function tokenAssets(event: LooseRecord, targetWallet: string, direction: "negative" | "positive"): AssetCandidate[] {
  const transfers = mergeTokenAssets(tokenTransferAssets(event, targetWallet, direction));
  const transferMints = new Set(transfers.map((asset) => asset.mint).filter(Boolean));
  const balanceChanges = mergeTokenAssets(
    tokenBalanceAssets(event, targetWallet, direction).filter((asset) => !asset.mint || !transferMints.has(asset.mint))
  );

  return mergeTokenAssets([...transfers, ...balanceChanges]);
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
  const { input, output, action, reason } = pickSwapAssets(event, targetWallet);
  const primaryMint =
    action === "buy" && output?.mint && output.mint !== SOL_MINT
      ? output.mint
      : action === "sell" && input?.mint && input.mint !== SOL_MINT
        ? input.mint
        : action === "swap" && output?.mint && output.mint !== SOL_MINT
          ? output.mint
          : null;
  const signature = stringValue(event.signature);
  const source = stringValue(event.source);
  const timestamp = numberValue(event.timestamp);
  const feePayer = stringValue(event.feePayer);
  const nativeAmount = input?.symbol === "SOL" ? input.amount : output?.symbol === "SOL" ? output.amount : null;
  const tokenAmount = output?.symbol !== "SOL" ? (output?.amount ?? null) : input?.symbol !== "SOL" ? (input?.amount ?? null) : null;

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
      events: asRecord(event.events),
      heliusSwapParser: {
        action,
        copyable: action === "buy",
        reason
      }
    })
  };
}
