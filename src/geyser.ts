import * as YellowstoneGrpc from "@triton-one/yellowstone-grpc";
import { CommitmentLevel } from "@triton-one/yellowstone-grpc";
import type { SubscribeRequest, SubscribeUpdate } from "@triton-one/yellowstone-grpc";
import bs58 from "bs58";
import { readableWebsocketData } from "./format.js";
import type { ExplorerConfig, LooseRecord, WalletTradeData } from "./types.js";

const SOL_MINT = "So11111111111111111111111111111111111111112";
const LAMPORTS_PER_SOL = 1_000_000_000;
const MIN_COPYABLE_NATIVE_SOL_INPUT = 0.001;
const PUMP_FUN_PROGRAM = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMPSWAP_PROGRAM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";

type StreamLike = NodeJS.ReadWriteStream & {
  write: (chunk: SubscribeRequest) => boolean;
  destroy: (error?: Error) => void;
};
type GeyserClient = {
  connect: () => Promise<void>;
  subscribe: () => Promise<StreamLike>;
};
type GeyserClientConstructor = new (
  endpoint: string,
  xToken: string | undefined,
  channelOptions: unknown | undefined
) => GeyserClient;

export interface GeyserWallet {
  address: string;
  label?: string | null;
}

export interface GeyserWalletTradeReject {
  ok: false;
  reason: string;
  signature: string | null;
  slot: string | null;
  targetWallet: string;
  raw: LooseRecord;
}

export interface GeyserWalletTradeAccepted {
  ok: true;
  trade: WalletTradeData;
}

export type GeyserWalletTradeParseResult = GeyserWalletTradeAccepted | GeyserWalletTradeReject;

export interface GeyserWalletTradeListenerOptions {
  enabled: boolean;
  endpoint?: string;
  xToken?: string;
  wallets: GeyserWallet[];
  config: ExplorerConfig;
  onTrade: (trade: WalletTradeData, timing: { receivedAtMs: number; normalizedAtMs: number }) => void | Promise<void>;
  onReject?: (reject: GeyserWalletTradeReject) => void | Promise<void>;
  onStatus?: (message: string) => void;
  onError?: (error: Error) => void;
  reconnectDelayMs?: number;
}

export interface GeyserWalletTradeListener {
  start: () => void;
  stop: () => void;
  setWallets: (wallets: GeyserWallet[]) => void;
}

interface GeyserRoute {
  source: "GEYSER_PUMP_BONDING_CURVE" | "GEYSER_PUMPSWAP";
  pool: "pump" | "pump-amm";
}

interface TokenDelta {
  mint: string;
  amount: number;
  decimals: number | null;
}

function sortedWallets(wallets: GeyserWallet[]): GeyserWallet[] {
  const byAddress = new Map<string, GeyserWallet>();

  for (const wallet of wallets) {
    const address = wallet.address?.trim();

    if (!address) {
      continue;
    }

    byAddress.set(address, {
      address,
      label: wallet.label || byAddress.get(address)?.label || null
    });
  }

  return [...byAddress.values()].sort((a, b) => a.address.localeCompare(b.address));
}

function geyserClientConstructor(): GeyserClientConstructor {
  const module = YellowstoneGrpc as unknown as { default?: GeyserClientConstructor };
  return module.default || (YellowstoneGrpc as unknown as GeyserClientConstructor);
}

export function buildGeyserWalletSubscribeRequest(wallets: GeyserWallet[] | string[]): SubscribeRequest {
  const addresses = [...new Set((typeof wallets[0] === "string"
    ? wallets as string[]
    : (wallets as GeyserWallet[]).map((wallet) => wallet.address)).filter(Boolean))].sort();

  return {
    accounts: {},
    slots: {},
    transactions: addresses.length > 0
      ? {
          walletTrades: {
            vote: false,
            failed: false,
            signature: undefined,
            accountInclude: addresses,
            accountExclude: [],
            accountRequired: []
          }
        }
      : {},
    transactionsStatus: {},
    blocks: {},
    blocksMeta: {},
    entry: {},
    commitment: CommitmentLevel.PROCESSED,
    accountsDataSlice: []
  };
}

function uint8ToBase58(value: Uint8Array | undefined): string | null {
  return value && value.length > 0 ? bs58.encode(value) : null;
}

function allAccountKeys(update: SubscribeUpdate): string[] {
  const info = update.transaction?.transaction;
  const message = info?.transaction?.message;
  const meta = info?.meta;
  const keys = [
    ...(message?.accountKeys || []),
    ...(meta?.loadedWritableAddresses || []),
    ...(meta?.loadedReadonlyAddresses || [])
  ];

  return keys.map(uint8ToBase58).filter((key): key is string => Boolean(key));
}

function signatureFromUpdate(update: SubscribeUpdate): string | null {
  const signature = update.transaction?.transaction?.signature;
  return uint8ToBase58(signature);
}

function numberValue(value: unknown): number | null {
  if (value === undefined || value === null || value === "") {
    return null;
  }

  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function rawTokenAmount(balance: { uiTokenAmount?: { amount: string; decimals: number } | undefined }): number | null {
  const rawAmount = numberValue(balance.uiTokenAmount?.amount);
  const decimals = numberValue(balance.uiTokenAmount?.decimals);

  if (rawAmount === null || decimals === null) {
    return null;
  }

  return rawAmount / 10 ** decimals;
}

function tokenDeltasForWallet(update: SubscribeUpdate, targetWallet: string): TokenDelta[] {
  const meta = update.transaction?.transaction?.meta;
  const deltas = new Map<string, TokenDelta>();

  function add(balance: {
    accountIndex: number;
    mint: string;
    owner: string;
    uiTokenAmount?: { amount: string; decimals: number } | undefined;
  }, sign: 1 | -1): void {
    if (balance.owner !== targetWallet || !balance.mint || balance.mint === SOL_MINT) {
      return;
    }

    const amount = rawTokenAmount(balance);

    if (amount === null) {
      return;
    }

    const key = `${balance.accountIndex}:${balance.mint}`;
    const previous = deltas.get(key) || {
      mint: balance.mint,
      amount: 0,
      decimals: balance.uiTokenAmount?.decimals ?? null
    };
    deltas.set(key, {
      ...previous,
      amount: previous.amount + amount * sign
    });
  }

  for (const balance of meta?.preTokenBalances || []) {
    add(balance, -1);
  }

  for (const balance of meta?.postTokenBalances || []) {
    add(balance, 1);
  }

  const byMint = new Map<string, TokenDelta>();

  for (const delta of deltas.values()) {
    if (Math.abs(delta.amount) < Number.EPSILON) {
      continue;
    }

    const previous = byMint.get(delta.mint) || {
      mint: delta.mint,
      amount: 0,
      decimals: delta.decimals
    };
    byMint.set(delta.mint, {
      ...previous,
      amount: previous.amount + delta.amount,
      decimals: previous.decimals ?? delta.decimals
    });
  }

  return [...byMint.values()].filter((delta) => Math.abs(delta.amount) >= Number.EPSILON);
}

function nativeSolDelta(update: SubscribeUpdate, accountIndex: number): number | null {
  const meta = update.transaction?.transaction?.meta;
  const preLamports = numberValue(meta?.preBalances[accountIndex]);
  const postLamports = numberValue(meta?.postBalances[accountIndex]);

  if (preLamports === null || postLamports === null) {
    return null;
  }

  let deltaLamports = postLamports - preLamports;

  if (accountIndex === 0) {
    deltaLamports += numberValue(meta?.fee) || 0;
  }

  return deltaLamports / LAMPORTS_PER_SOL;
}

function geyserRoute(accountKeys: string[]): GeyserRoute | null {
  if (accountKeys.includes(PUMPSWAP_PROGRAM)) {
    return {
      source: "GEYSER_PUMPSWAP",
      pool: "pump-amm"
    };
  }

  if (accountKeys.includes(PUMP_FUN_PROGRAM)) {
    return {
      source: "GEYSER_PUMP_BONDING_CURVE",
      pool: "pump"
    };
  }

  return null;
}

function rejectResult({
  update,
  targetWallet,
  reason,
  route,
  accountKeys
}: {
  update: SubscribeUpdate;
  targetWallet: string;
  reason: string;
  route?: GeyserRoute | null;
  accountKeys?: string[];
}): GeyserWalletTradeReject {
  const signature = signatureFromUpdate(update);
  const slot = update.transaction?.slot || null;
  const raw = readableWebsocketData({
    signature,
    slot,
    targetWallet,
    source: route?.source || null,
    accountKeys: accountKeys?.slice(0, 40) || [],
    geyserParser: {
      accepted: false,
      reason
    }
  });

  return {
    ok: false,
    reason,
    signature,
    slot,
    targetWallet,
    raw
  };
}

export function geyserUpdateMentionsWallet(update: SubscribeUpdate, walletAddress: string): boolean {
  return allAccountKeys(update).includes(walletAddress);
}

export function normalizeGeyserWalletTrade({
  update,
  targetWallet,
  label,
  config
}: {
  update: SubscribeUpdate;
  targetWallet: string;
  label?: string | null;
  config: ExplorerConfig;
}): GeyserWalletTradeParseResult {
  const transaction = update.transaction?.transaction;
  const meta = transaction?.meta;
  const accountKeys = allAccountKeys(update);
  const route = geyserRoute(accountKeys);
  const signature = signatureFromUpdate(update);
  const slot = update.transaction?.slot || null;

  if (!update.transaction || !transaction) {
    return rejectResult({ update, targetWallet, reason: "update does not contain a transaction" });
  }

  if (transaction.isVote) {
    return rejectResult({ update, targetWallet, reason: "vote transaction", accountKeys });
  }

  if (!meta) {
    return rejectResult({ update, targetWallet, reason: "transaction metadata is missing", accountKeys });
  }

  if (meta.err) {
    return rejectResult({ update, targetWallet, reason: "failed transaction", accountKeys });
  }

  if (!route) {
    return rejectResult({ update, targetWallet, reason: "transaction does not include Pump or PumpSwap program", accountKeys });
  }

  const walletIndex = accountKeys.indexOf(targetWallet);

  if (walletIndex === -1) {
    return rejectResult({ update, targetWallet, reason: "watched wallet not found in transaction account keys", route, accountKeys });
  }

  const solDelta = nativeSolDelta(update, walletIndex);

  if (solDelta === null) {
    return rejectResult({ update, targetWallet, reason: "watched wallet native balance delta is unavailable", route, accountKeys });
  }

  const tokenDeltas = tokenDeltasForWallet(update, targetWallet);
  const incomingTokens = tokenDeltas.filter((delta) => delta.amount > 0);
  const outgoingTokens = tokenDeltas.filter((delta) => delta.amount < 0);
  const outgoingSol = solDelta < 0 ? Math.abs(solDelta) : 0;
  const incomingSol = solDelta > 0 ? solDelta : 0;
  const feePayer = accountKeys[0] || null;
  const observedAt = new Date().toISOString();
  const timestamp = update.createdAt instanceof Date
    ? Math.floor(update.createdAt.getTime() / 1000)
    : Math.floor(Date.now() / 1000);

  let action: WalletTradeData["action"] | null = null;
  let mint: string | null = null;
  let solAmount: number | null = null;
  let tokenAmount: number | null = null;

  if (outgoingSol > 0 && incomingTokens.length === 1 && outgoingTokens.length === 0) {
    if (outgoingSol < MIN_COPYABLE_NATIVE_SOL_INPUT) {
      return rejectResult({
        update,
        targetWallet,
        route,
        accountKeys,
        reason: "watched wallet native SOL spend is too small to prove it is the swap input"
      });
    }

    action = "buy";
    mint = incomingTokens[0].mint;
    solAmount = outgoingSol;
    tokenAmount = incomingTokens[0].amount;
  } else if (incomingSol > 0 && outgoingTokens.length === 1 && incomingTokens.length === 0) {
    action = "sell";
    mint = outgoingTokens[0].mint;
    solAmount = incomingSol;
    tokenAmount = Math.abs(outgoingTokens[0].amount);
  } else {
    return rejectResult({
      update,
      targetWallet,
      route,
      accountKeys,
      reason: "watched wallet Pump transaction route is ambiguous"
    });
  }

  const input = action === "buy"
    ? { mint: SOL_MINT, symbol: "SOL", amount: solAmount }
    : { mint, symbol: null, amount: tokenAmount };
  const output = action === "buy"
    ? { mint, symbol: null, amount: tokenAmount }
    : { mint: SOL_MINT, symbol: "SOL", amount: solAmount };

  return {
    ok: true,
    trade: {
      observedAt,
      provider: "geyser",
      targetWallet,
      label: label || null,
      action,
      mint,
      signature,
      timestamp,
      feePayer,
      source: route.source,
      input,
      output,
      solAmount,
      tokenAmount,
      pool: route.pool,
      marketCapSol: null,
      pumpFunUrl: mint ? `${config.pumpFunBaseUrl}/${mint}` : null,
      solscanTokenUrl: mint ? `${config.solscanBaseUrl}/token/${mint}` : null,
      solscanTxUrl: signature ? `${config.solscanBaseUrl}/tx/${signature}` : null,
      raw: readableWebsocketData({
        signature,
        slot,
        targetWallet,
        feePayer,
        source: route.source,
        pool: route.pool,
        accountKeys: accountKeys.slice(0, 40),
        geyserParser: {
          accepted: true,
          action,
          copyable: action === "buy",
          route: route.source,
          solDelta,
          tokenDeltas: tokenDeltas.map((delta) => ({
            mint: delta.mint,
            amount: delta.amount
          }))
        }
      })
    }
  };
}

export function createGeyserWalletTradeListener(options: GeyserWalletTradeListenerOptions): GeyserWalletTradeListener {
  let wallets = sortedWallets(options.wallets);
  let started = false;
  let connecting = false;
  let client: GeyserClient | null = null;
  let stream: StreamLike | null = null;
  let reconnectTimer: NodeJS.Timeout | null = null;
  const reconnectDelayMs = options.reconnectDelayMs ?? 2_500;

  function status(message: string): void {
    options.onStatus?.(message);
  }

  function error(errorValue: unknown): void {
    const nextError = errorValue instanceof Error ? errorValue : new Error(String(errorValue));
    options.onError?.(nextError);
  }

  function clearReconnectTimer(): void {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  }

  function closeStream(): void {
    const activeStream = stream;
    stream = null;

    if (activeStream) {
      activeStream.removeAllListeners();
      activeStream.destroy();
    }
  }

  function scheduleReconnect(reason: string): void {
    if (!started || !options.enabled || reconnectTimer || wallets.length === 0) {
      return;
    }

    closeStream();
    client = null;
    status(`Geyser stream closed: ${reason}; reconnecting in ${reconnectDelayMs}ms`);
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      connect().catch(error);
    }, reconnectDelayMs);
  }

  async function handleUpdate(update: SubscribeUpdate): Promise<void> {
    if (!update.transaction) {
      return;
    }

    const receivedAtMs = Date.now();

    for (const wallet of wallets) {
      if (!geyserUpdateMentionsWallet(update, wallet.address)) {
        continue;
      }

      const result = normalizeGeyserWalletTrade({
        update,
        targetWallet: wallet.address,
        label: wallet.label,
        config: options.config
      });

      if (result.ok) {
        await options.onTrade(result.trade, {
          receivedAtMs,
          normalizedAtMs: Date.now()
        });
      } else {
        await options.onReject?.(result);
      }
    }
  }

  async function connect(): Promise<void> {
    if (!started || !options.enabled || connecting || stream) {
      return;
    }

    if (!options.endpoint) {
      status("Geyser listener disabled: GEYSER_GRPC_URL is not set");
      return;
    }

    if (wallets.length === 0) {
      status("Geyser listener idle: no watched wallets");
      return;
    }

    connecting = true;

    try {
      const Client = geyserClientConstructor();
      client = new Client(options.endpoint, options.xToken, undefined);
      await client.connect();
      const nextStream = await client.subscribe() as StreamLike;
      stream = nextStream;
      nextStream.on("data", (update: SubscribeUpdate) => {
        handleUpdate(update).catch(error);
      });
      nextStream.on("error", (streamError: Error) => {
        error(streamError);
        scheduleReconnect(streamError.message);
      });
      nextStream.on("close", () => scheduleReconnect("close"));
      nextStream.on("end", () => scheduleReconnect("end"));
      nextStream.write(buildGeyserWalletSubscribeRequest(wallets));
      status(`Connected to Geyser stream for ${wallets.length} watched wallet(s)`);
    } catch (connectError) {
      error(connectError);
      scheduleReconnect(connectError instanceof Error ? connectError.message : String(connectError));
    } finally {
      connecting = false;
    }
  }

  function writeSubscription(): void {
    if (stream) {
      stream.write(buildGeyserWalletSubscribeRequest(wallets));
      status(`Updated Geyser wallet subscription: ${wallets.length} watched wallet(s)`);
      return;
    }

    connect().catch(error);
  }

  return {
    start() {
      started = true;

      if (!options.enabled) {
        return;
      }

      connect().catch(error);
    },
    stop() {
      started = false;
      clearReconnectTimer();
      closeStream();
      client = null;
    },
    setWallets(nextWallets) {
      wallets = sortedWallets(nextWallets);

      if (!started || !options.enabled) {
        return;
      }

      if (wallets.length === 0) {
        closeStream();
        status("Geyser listener idle: no watched wallets");
        return;
      }

      writeSubscription();
    }
  };
}
