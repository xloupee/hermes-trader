import { CommitmentLevel } from "@triton-one/yellowstone-grpc";
import type { SubscribeRequest, SubscribeUpdate } from "@triton-one/yellowstone-grpc";
import bs58 from "bs58";
import type { BotConfig, LooseRecord, WalletTradeData, WalletTradeAction } from "./types.js";
import { errorMessage } from "./types.js";

const SOL_MINT = "So11111111111111111111111111111111111111112";
const LAMPORTS_PER_SOL = 1_000_000_000;
const PUMP_FUN_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMP_AMM_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const PUMP_FUN_FEE_ACCOUNT = "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM";
const PUMP_FUN_BUY_DISCRIMINATOR = Buffer.from([102, 6, 61, 18, 1, 218, 235, 234]);
const PUMP_FUN_SELL_DISCRIMINATOR = Buffer.from([51, 230, 133, 164, 1, 127, 131, 173]);
const PUMP_FUN_TOKEN_DECIMALS = 6;
const PUMP_FUN_MINT_ACCOUNT_INDEX = 2;
const PUMP_FUN_USER_ACCOUNT_INDEX = 6;
const PUMP_AMM_USER_ACCOUNT_INDEX = 1;
const PUMP_AMM_BASE_MINT_ACCOUNT_INDEX = 3;
const PUMP_AMM_QUOTE_MINT_ACCOUNT_INDEX = 4;

export interface YellowstoneWalletMonitor {
  start: () => void;
  stop: () => void;
  setWallets: (wallets: string[]) => void;
}

export interface YellowstoneWalletMonitorOptions {
  enabled: boolean;
  endpoint?: string;
  token?: string;
  commitment: BotConfig["yellowstoneCommitment"];
  reconnectMs: number;
  shadowOnly: boolean;
  wallets: string[];
  explorer: {
    pumpFunBaseUrl: string;
    solscanBaseUrl: string;
  };
  onTrade: (trade: WalletTradeData, timing: { receivedAtMs: number; normalizedAtMs: number }) => void | Promise<void>;
  onStatus?: (message: string) => void;
  onError?: (error: Error) => void;
}

interface YellowstoneStream {
  destroy: () => void;
  on(event: "data", listener: (update: SubscribeUpdate) => void): YellowstoneStream;
  on(event: "error", listener: (error: Error) => void): YellowstoneStream;
  on(event: "close", listener: () => void): YellowstoneStream;
  write: (request: SubscribeRequest) => void;
}

interface YellowstoneClient {
  connect: () => Promise<void>;
  subscribe: () => Promise<YellowstoneStream>;
}

type YellowstoneClientConstructor = new (
  endpoint: string,
  token: string | undefined,
  channelOptions: Record<string, unknown> | undefined
) => YellowstoneClient;

interface YellowstoneInstruction {
  programIdIndex: number;
  accounts: Uint8Array;
  data: Uint8Array;
}

interface PumpTradeInstructionMatch {
  instruction: YellowstoneInstruction;
  route: YellowstoneRoute;
  action: Extract<WalletTradeAction, "buy" | "sell">;
  user: string;
  mint: string;
  quoteMint: string | null;
  tokenAmount: number;
  solLamports: number;
}

type YellowstoneRoute = "pump" | "pump-amm";

interface TokenDelta {
  mint: string;
  decimals: number;
  rawDelta: bigint;
  amount: number;
}

interface TokenBalance {
  mint?: string;
  owner?: string;
  uiTokenAmount?: {
    amount?: string;
    decimals?: number;
  };
}

interface YellowstoneTradeParseResult {
  trade: WalletTradeData | null;
  reason: string | null;
}

export function missingYellowstoneConfigWarning(): string {
  return "Geyser wallet monitor disabled; set GEYSER_ENABLED=true and GEYSER_GRPC_URL to test Yellowstone gRPC.";
}

function commitmentLevel(value: BotConfig["yellowstoneCommitment"]): CommitmentLevel {
  if (value === "confirmed") {
    return CommitmentLevel.CONFIRMED;
  }

  if (value === "finalized") {
    return CommitmentLevel.FINALIZED;
  }

  return CommitmentLevel.PROCESSED;
}

export function buildYellowstoneSubscribeRequest(wallets: string[], commitment: BotConfig["yellowstoneCommitment"]): SubscribeRequest {
  const accountInclude = [...new Set(wallets.filter(Boolean))].sort();

  return {
    accounts: {},
    slots: {},
    transactions: {
      pumpWallets: {
        vote: false,
        failed: false,
        signature: undefined,
        accountInclude,
        accountExclude: [],
        accountRequired: [PUMP_FUN_PROGRAM_ID]
      },
      pumpAmmWallets: {
        vote: false,
        failed: false,
        signature: undefined,
        accountInclude,
        accountExclude: [],
        accountRequired: [PUMP_AMM_PROGRAM_ID]
      }
    },
    transactionsStatus: {},
    blocks: {},
    blocksMeta: {},
    entry: {},
    commitment: commitmentLevel(commitment),
    accountsDataSlice: [],
    ping: undefined,
    fromSlot: undefined
  };
}

function decodeKeys(keys: Uint8Array[] | undefined): string[] {
  return (keys || []).map((key) => bs58.encode(key));
}

function instructionAccountIndexes(accounts: Uint8Array): number[] {
  return [...accounts].map((index) => Number(index));
}

function readU64LE(data: Uint8Array, offset: number): number | null {
  if (data.length < offset + 8) {
    return null;
  }

  const view = new DataView(data.buffer, data.byteOffset + offset, 8);
  const parsed = Number(view.getBigUint64(0, true));
  return Number.isFinite(parsed) ? parsed : null;
}

function discriminatorMatches(data: Uint8Array, discriminator: Buffer): boolean {
  return data.length >= discriminator.length && Buffer.from(data.slice(0, discriminator.length)).equals(discriminator);
}

function pumpTradeInstructionMatch(instruction: YellowstoneInstruction, accountKeys: string[]): PumpTradeInstructionMatch | null {
  const programId = accountKeys[instruction.programIdIndex];
  const route = programId === PUMP_FUN_PROGRAM_ID
    ? "pump"
    : programId === PUMP_AMM_PROGRAM_ID
      ? "pump-amm"
      : null;

  if (!route) {
    return null;
  }

  const action = discriminatorMatches(instruction.data, PUMP_FUN_BUY_DISCRIMINATOR)
    ? "buy"
    : discriminatorMatches(instruction.data, PUMP_FUN_SELL_DISCRIMINATOR)
      ? "sell"
      : null;

  if (!action) {
    return null;
  }

  const tokenAmountRaw = readU64LE(instruction.data, 8);
  const solLamports = readU64LE(instruction.data, 16);

  if (tokenAmountRaw === null || solLamports === null) {
    return null;
  }

  const accounts = instructionAccountIndexes(instruction.accounts);
  const user = route === "pump"
    ? accountKeys[accounts[PUMP_FUN_USER_ACCOUNT_INDEX]]
    : accountKeys[accounts[PUMP_AMM_USER_ACCOUNT_INDEX]];
  const mint = route === "pump"
    ? accountKeys[accounts[PUMP_FUN_MINT_ACCOUNT_INDEX]]
    : accountKeys[accounts[PUMP_AMM_BASE_MINT_ACCOUNT_INDEX]];
  const quoteMint = route === "pump-amm"
    ? accountKeys[accounts[PUMP_AMM_QUOTE_MINT_ACCOUNT_INDEX]] || null
    : null;

  if (!user || !mint) {
    return null;
  }

  return {
    instruction,
    route,
    action,
    user,
    mint,
    quoteMint,
    tokenAmount: tokenAmountRaw / 10 ** PUMP_FUN_TOKEN_DECIMALS,
    solLamports
  };
}

function allInstructions(
  messageInstructions: YellowstoneInstruction[] | undefined,
  innerInstructions: Array<{ instructions: YellowstoneInstruction[] }> | undefined
): YellowstoneInstruction[] {
  return [
    ...(messageInstructions || []),
    ...((innerInstructions || []).flatMap((inner) => inner.instructions || []))
  ];
}

function lamports(value: string | number | bigint | undefined): bigint {
  if (typeof value === "bigint") {
    return value;
  }

  if (typeof value === "number") {
    return BigInt(Math.trunc(value));
  }

  if (!value) {
    return 0n;
  }

  try {
    return BigInt(value);
  } catch {
    return 0n;
  }
}

function numberFromRawAmount(rawAmount: bigint, decimals: number): number {
  return Number(rawAmount) / 10 ** decimals;
}

function tokenBalanceRawAmount(balance: TokenBalance): bigint {
  return lamports(balance.uiTokenAmount?.amount);
}

function tokenBalanceDecimals(balance: TokenBalance): number {
  return balance.uiTokenAmount?.decimals ?? 0;
}

function aggregateTokenBalancesByMint(balances: TokenBalance[] | undefined, owner: string): Map<string, { raw: bigint; decimals: number }> {
  const byMint = new Map<string, { raw: bigint; decimals: number }>();

  for (const balance of balances || []) {
    if (balance.owner !== owner || !balance.mint || balance.mint === SOL_MINT) {
      continue;
    }

    const existing = byMint.get(balance.mint) || { raw: 0n, decimals: tokenBalanceDecimals(balance) };
    byMint.set(balance.mint, {
      raw: existing.raw + tokenBalanceRawAmount(balance),
      decimals: existing.decimals
    });
  }

  return byMint;
}

function tokenDeltas(preBalances: TokenBalance[] | undefined, postBalances: TokenBalance[] | undefined, owner: string): TokenDelta[] {
  const pre = aggregateTokenBalancesByMint(preBalances, owner);
  const post = aggregateTokenBalancesByMint(postBalances, owner);
  const mints = [...new Set([...pre.keys(), ...post.keys()])].sort();

  return mints
    .map((mint) => {
      const before = pre.get(mint);
      const after = post.get(mint);
      const decimals = after?.decimals ?? before?.decimals ?? 0;
      const rawDelta = (after?.raw ?? 0n) - (before?.raw ?? 0n);

      return {
        mint,
        decimals,
        rawDelta,
        amount: numberFromRawAmount(rawDelta < 0n ? -rawDelta : rawDelta, decimals)
      };
    })
    .filter((delta) => delta.rawDelta !== 0n);
}

function nativeDeltaLamports(accountKeys: string[], preBalances: string[] | undefined, postBalances: string[] | undefined, owner: string): bigint | null {
  let seen = false;
  let delta = 0n;

  for (const [index, account] of accountKeys.entries()) {
    if (account !== owner) {
      continue;
    }

    seen = true;
    delta += lamports(postBalances?.[index]) - lamports(preBalances?.[index]);
  }

  return seen ? delta : null;
}

function routeFromPrograms(accountKeys: string[], instructions: YellowstoneInstruction[]): YellowstoneRoute | null {
  const programIds = new Set(instructions.map((instruction) => accountKeys[instruction.programIdIndex]).filter(Boolean));

  if (programIds.has(PUMP_AMM_PROGRAM_ID)) {
    return "pump-amm";
  }

  if (programIds.has(PUMP_FUN_PROGRAM_ID)) {
    return "pump";
  }

  return null;
}

function sourceForRoute(route: YellowstoneRoute): string {
  return route === "pump-amm" ? "GEYSER_PUMPSWAP" : "GEYSER_PUMP_BONDING_CURVE";
}

function normalizeYellowstoneTradeResult(
  update: SubscribeUpdate,
  targetWallet: string,
  explorer: YellowstoneWalletMonitorOptions["explorer"]
): YellowstoneTradeParseResult {
  const tx = update.transaction;
  const info = tx?.transaction;
  const transaction = info?.transaction;
  const message = transaction?.message;
  const meta = info?.meta;

  if (!tx || !info || !transaction || !message || !meta) {
    return { trade: null, reason: "missing transaction payload" };
  }

  if (info.isVote) {
    return { trade: null, reason: "vote transaction" };
  }

  if (meta.err) {
    return { trade: null, reason: "failed transaction" };
  }

  const accountKeys = [
    ...decodeKeys(message.accountKeys),
    ...decodeKeys(meta.loadedWritableAddresses),
    ...decodeKeys(meta.loadedReadonlyAddresses)
  ];
  const instructions = allInstructions(message.instructions, meta.innerInstructions);
  const route = routeFromPrograms(accountKeys, instructions);

  if (!route) {
    return { trade: null, reason: "unsupported program" };
  }

  const targetTokenDeltas = tokenDeltas(meta.preTokenBalances, meta.postTokenBalances, targetWallet);
  const targetNativeDelta = nativeDeltaLamports(accountKeys, meta.preBalances, meta.postBalances, targetWallet);
  const matches = instructions
    .map((instruction) => pumpTradeInstructionMatch(instruction, accountKeys))
    .filter((match): match is PumpTradeInstructionMatch => Boolean(match));
  const targetMatches = matches.filter((match) => match.user === targetWallet);
  const unsupportedPumpSwapQuote = targetMatches.find((match) => match.route === "pump-amm" && match.quoteMint !== SOL_MINT);

  if (unsupportedPumpSwapQuote) {
    return { trade: null, reason: `unsupported PumpSwap quote mint ${unsupportedPumpSwapQuote.quoteMint || "unknown"}` };
  }

  const distinctTargetMatches = [...new Map(targetMatches.map((match) => [
    [match.route, match.action, match.mint].join(":"),
    match
  ])).values()];

  if (distinctTargetMatches.length === 0) {
    return { trade: null, reason: "target wallet not matched to pump trade instruction" };
  }

  if (distinctTargetMatches.length > 1) {
    return { trade: null, reason: "ambiguous target pump trade instructions" };
  }

  const targetMatch = distinctTargetMatches[0];

  const relevantTokenDeltas = targetTokenDeltas.length > 0
    ? targetTokenDeltas
    : [
        {
          mint: targetMatch.mint,
          decimals: PUMP_FUN_TOKEN_DECIMALS,
          rawDelta: targetMatch.action === "buy" ? 1n : -1n,
          amount: targetMatch.tokenAmount
        }
      ];

  if (relevantTokenDeltas.length !== 1) {
    return { trade: null, reason: relevantTokenDeltas.length === 0 ? "missing target token delta" : "ambiguous target token deltas" };
  }

  const tokenDelta = relevantTokenDeltas[0];
  const inferredAction =
    tokenDelta.rawDelta > 0n
      ? "buy"
      : tokenDelta.rawDelta < 0n
        ? "sell"
        : null;
  const action = targetMatch.action || inferredAction;

  if (!action) {
    return { trade: null, reason: "could not infer trade action" };
  }

  if (targetMatch.action !== inferredAction && targetTokenDeltas.length > 0) {
    return { trade: null, reason: "target token delta conflicts with pump instruction action" };
  }

  const signature = bs58.encode(info.signature);
  const slot = Number(tx.slot);
  const observedAt = new Date().toISOString();
  const timestamp = update.createdAt instanceof Date
    ? Math.floor(update.createdAt.getTime() / 1000)
    : Math.floor(Date.now() / 1000);
  const solLamports = targetNativeDelta !== null && targetNativeDelta !== 0n
    ? (targetNativeDelta < 0n ? -targetNativeDelta : targetNativeDelta)
    : BigInt(targetMatch.solLamports || 0);
  const solAmount = Number(solLamports) / LAMPORTS_PER_SOL;
  const tokenAmount = tokenDelta.amount || targetMatch.tokenAmount || null;
  const source = sourceForRoute(targetMatch.route);
  const input = action === "buy"
    ? { mint: SOL_MINT, symbol: "SOL", amount: solAmount }
    : { mint: tokenDelta.mint, symbol: null, amount: tokenAmount };
  const output = action === "buy"
    ? { mint: tokenDelta.mint, symbol: null, amount: tokenAmount }
    : { mint: SOL_MINT, symbol: "SOL", amount: solAmount };
  const raw: LooseRecord = {
    provider: "yellowstone",
    parser: targetTokenDeltas.length > 0 ? "balance-delta" : "pump-trade-instruction",
    source,
    route: targetMatch.route,
    slot,
    filters: update.filters,
    index: info.index,
    accountKeyCount: accountKeys.length,
    mentionedTargetWallet: targetWallet,
    targetNativeDeltaLamports: targetNativeDelta?.toString() ?? null,
    targetTokenDeltas: targetTokenDeltas.map((delta) => ({
      mint: delta.mint,
      rawDelta: delta.rawDelta.toString(),
      decimals: delta.decimals,
      amount: delta.amount
    })),
    pumpFunProgramId: PUMP_FUN_PROGRAM_ID,
    pumpAmmProgramId: PUMP_AMM_PROGRAM_ID,
    pumpFunFeeAccount: PUMP_FUN_FEE_ACCOUNT,
    targetPumpInstruction: {
      route: targetMatch.route,
      action: targetMatch.action,
      user: targetMatch.user,
      mint: targetMatch.mint,
      quoteMint: targetMatch.quoteMint
    },
    geyserParser: {
      action,
      copyable: action === "buy",
      reason: null
    },
    logMessages: meta.logMessages?.slice(0, 20) || []
  };

  return { trade: {
    observedAt,
    provider: "yellowstone",
    targetWallet,
    label: null,
    action,
    mint: tokenDelta.mint,
    signature,
    timestamp,
    feePayer: accountKeys[0] || null,
    source,
    input,
    output,
    solAmount,
    tokenAmount,
    pool: targetMatch.route,
    marketCapSol: null,
    pumpFunUrl: `${explorer.pumpFunBaseUrl}/${tokenDelta.mint}`,
    solscanTokenUrl: `${explorer.solscanBaseUrl}/token/${tokenDelta.mint}`,
    solscanTxUrl: `${explorer.solscanBaseUrl}/tx/${signature}`,
    raw
  }, reason: null };
}

export function normalizeYellowstoneTrade(
  update: SubscribeUpdate,
  targetWallet: string,
  explorer: YellowstoneWalletMonitorOptions["explorer"]
): WalletTradeData | null {
  return normalizeYellowstoneTradeResult(update, targetWallet, explorer).trade;
}

export function createYellowstoneWalletMonitor({
  enabled,
  endpoint,
  token,
  commitment,
  reconnectMs,
  shadowOnly,
  wallets,
  explorer,
  onTrade,
  onStatus = () => {},
  onError = () => {}
}: YellowstoneWalletMonitorOptions): YellowstoneWalletMonitor {
  let activeWallets = [...new Set(wallets.filter(Boolean))].sort();
  let stream: YellowstoneStream | null = null;
  let reconnectTimer: NodeJS.Timeout | null = null;
  let running = false;
  let subscriptionGeneration = 0;

  function clearReconnectTimer(): void {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  }

  function destroyStream(): void {
    if (stream) {
      stream.destroy();
      stream = null;
    }
  }

  function scheduleReconnect(): void {
    clearReconnectTimer();
    if (!running) {
      return;
    }

    reconnectTimer = setTimeout(() => {
      void connect();
    }, reconnectMs);
  }

  async function connect(): Promise<void> {
    const generation = ++subscriptionGeneration;
    clearReconnectTimer();
    destroyStream();

    if (!running || !enabled || !endpoint || activeWallets.length === 0) {
      return;
    }

    try {
      const { default: Client } = await import("@triton-one/yellowstone-grpc");
      const YellowstoneClient = Client as unknown as YellowstoneClientConstructor;
      const client = new YellowstoneClient(endpoint, token, {});
      await client.connect();
      const nextStream = await client.subscribe();

      if (!running || generation !== subscriptionGeneration) {
        nextStream.destroy();
        return;
      }

      stream = nextStream;
      stream.on("data", (update: SubscribeUpdate) => {
        const receivedAtMs = Date.now();
        for (const targetWallet of activeWallets) {
          const { trade, reason } = normalizeYellowstoneTradeResult(update, targetWallet, explorer);
          if (!trade) {
            if (reason && update.transaction?.transaction) {
              onStatus(`Yellowstone wallet trade rejected: ${JSON.stringify({
                reason,
                targetWallet,
                slot: update.transaction.slot,
                signature: update.transaction.transaction.signature
                  ? bs58.encode(update.transaction.transaction.signature)
                  : null,
                filters: update.filters
              })}`);
            }
            continue;
          }

          const normalizedAtMs = Date.now();
          onStatus(`Yellowstone wallet trade candidate: ${JSON.stringify({
            shadowOnly,
            targetWallet,
            action: trade.action,
            mint: trade.mint,
            signature: trade.signature,
            slot: trade.raw.slot
          })}`);
          Promise.resolve(onTrade(trade, { receivedAtMs, normalizedAtMs })).catch((error: unknown) =>
            onError(error instanceof Error ? error : new Error(errorMessage(error)))
          );
        }
      });
      stream.on("error", (error: Error) => {
        onError(error);
        scheduleReconnect();
      });
      stream.on("close", () => {
        onStatus("Yellowstone gRPC stream closed");
        scheduleReconnect();
      });
      stream.write(buildYellowstoneSubscribeRequest(activeWallets, commitment));
      onStatus(`Yellowstone gRPC subscribed to ${activeWallets.length} wallet(s) at ${commitment} commitment${shadowOnly ? " in shadow mode" : ""}`);
    } catch (error) {
      onError(new Error(`Yellowstone gRPC connection failed: ${errorMessage(error)}`));
      scheduleReconnect();
    }
  }

  return {
    start() {
      running = true;
      if (!enabled || !endpoint) {
        onStatus(missingYellowstoneConfigWarning());
        return;
      }

      void connect();
    },
    stop() {
      running = false;
      clearReconnectTimer();
      destroyStream();
    },
    setWallets(wallets: string[]) {
      const nextWallets = [...new Set(wallets.filter(Boolean))].sort();
      if (nextWallets.join(",") === activeWallets.join(",")) {
        return;
      }

      activeWallets = nextWallets;
      if (running) {
        void connect();
      }
    }
  };
}
