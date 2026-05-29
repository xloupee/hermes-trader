import { CommitmentLevel } from "@triton-one/yellowstone-grpc";
import type { SubscribeRequest, SubscribeUpdate } from "@triton-one/yellowstone-grpc";
import bs58 from "bs58";
import type { BotConfig, LooseRecord, WalletTradeData, WalletTradeAction } from "./types.js";
import { errorMessage } from "./types.js";

const SOL_MINT = "So11111111111111111111111111111111111111112";
const LAMPORTS_PER_SOL = 1_000_000_000;
const PUMP_FUN_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMP_FUN_FEE_ACCOUNT = "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM";
const PUMP_FUN_BUY_DISCRIMINATOR = Buffer.from([102, 6, 61, 18, 1, 218, 235, 234]);
const PUMP_FUN_SELL_DISCRIMINATOR = Buffer.from([51, 230, 133, 164, 1, 127, 131, 173]);
const PUMP_FUN_TOKEN_DECIMALS = 6;
const PUMP_FUN_MINT_ACCOUNT_INDEX = 2;
const PUMP_FUN_USER_ACCOUNT_INDEX = 6;

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

interface PumpFunInstructionMatch {
  instruction: YellowstoneInstruction;
  action: Extract<WalletTradeAction, "buy" | "sell">;
  tokenAmount: number;
  solLamports: number;
}

export function missingYellowstoneConfigWarning(): string {
  return "Yellowstone wallet monitor disabled; set YELLOWSTONE_ENABLED=true, YELLOWSTONE_ENDPOINT, and YELLOWSTONE_TOKEN to test QuickNode gRPC.";
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

function buildSubscribeRequest(wallets: string[], commitment: BotConfig["yellowstoneCommitment"]): SubscribeRequest {
  return {
    accounts: {},
    slots: {},
    transactions: {
      watchedWallets: {
        vote: false,
        failed: false,
        signature: undefined,
        accountInclude: wallets,
        accountExclude: [],
        accountRequired: [PUMP_FUN_FEE_ACCOUNT, PUMP_FUN_PROGRAM_ID]
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

function pumpFunInstructionMatch(instruction: YellowstoneInstruction): PumpFunInstructionMatch | null {
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

  return {
    instruction,
    action,
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

function normalizeYellowstoneTrade(
  update: SubscribeUpdate,
  targetWallet: string,
  explorer: YellowstoneWalletMonitorOptions["explorer"]
): WalletTradeData | null {
  const tx = update.transaction;
  const info = tx?.transaction;
  const transaction = info?.transaction;
  const message = transaction?.message;
  const meta = info?.meta;

  if (!tx || !info || !transaction || !message || !meta) {
    return null;
  }

  const accountKeys = [
    ...decodeKeys(message.accountKeys),
    ...decodeKeys(meta.loadedWritableAddresses),
    ...decodeKeys(meta.loadedReadonlyAddresses)
  ];
  const matches = allInstructions(message.instructions, meta.innerInstructions)
    .filter((instruction) => accountKeys[instruction.programIdIndex] === PUMP_FUN_PROGRAM_ID)
    .map(pumpFunInstructionMatch)
    .filter((match): match is PumpFunInstructionMatch => Boolean(match));
  const targetMatch = matches.find((match) => {
    const accounts = instructionAccountIndexes(match.instruction.accounts);
    const user = accountKeys[accounts[PUMP_FUN_USER_ACCOUNT_INDEX]];
    return user === targetWallet;
  });

  if (!targetMatch) {
    return null;
  }

  const accounts = instructionAccountIndexes(targetMatch.instruction.accounts);
  const mint = accountKeys[accounts[PUMP_FUN_MINT_ACCOUNT_INDEX]];
  const user = accountKeys[accounts[PUMP_FUN_USER_ACCOUNT_INDEX]];

  if (!mint || user !== targetWallet) {
    return null;
  }

  const signature = bs58.encode(info.signature);
  const solAmount = targetMatch.solLamports / LAMPORTS_PER_SOL;
  const tokenAmount = targetMatch.tokenAmount;
  const input = targetMatch.action === "buy"
    ? { mint: SOL_MINT, symbol: "SOL", amount: solAmount }
    : { mint, symbol: null, amount: tokenAmount };
  const output = targetMatch.action === "buy"
    ? { mint, symbol: null, amount: tokenAmount }
    : { mint: SOL_MINT, symbol: "SOL", amount: solAmount };
  const raw: LooseRecord = {
    provider: "yellowstone",
    parser: "pumpfun-instruction",
    slot: tx.slot,
    filters: update.filters,
    index: info.index,
    accountKeyCount: accountKeys.length,
    mentionedTargetWallet: targetWallet,
    instructionAccounts: accounts,
    pumpFunProgramId: PUMP_FUN_PROGRAM_ID,
    pumpFunFeeAccount: PUMP_FUN_FEE_ACCOUNT,
    logMessages: meta.logMessages?.slice(0, 20) || []
  };

  return {
    observedAt: new Date().toISOString(),
    provider: "yellowstone",
    targetWallet,
    label: null,
    action: targetMatch.action,
    mint,
    signature,
    timestamp: Math.floor(Date.now() / 1000),
    feePayer: accountKeys[0] || null,
    source: "YELLOWSTONE_PUMPFUN",
    input,
    output,
    solAmount,
    tokenAmount,
    pool: "pump",
    marketCapSol: null,
    pumpFunUrl: `${explorer.pumpFunBaseUrl}/${mint}`,
    solscanTokenUrl: `${explorer.solscanBaseUrl}/token/${mint}`,
    solscanTxUrl: `${explorer.solscanBaseUrl}/tx/${signature}`,
    raw
  };
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

    if (!running || !enabled || !endpoint || !token || activeWallets.length === 0) {
      return;
    }

    try {
      const { default: Client } = await import("@triton-one/yellowstone-grpc");
      const YellowstoneClient = Client as unknown as YellowstoneClientConstructor;
      const client = new YellowstoneClient(endpoint, token, {});
      const nextStream = await client.subscribe();

      if (!running || generation !== subscriptionGeneration) {
        nextStream.destroy();
        return;
      }

      stream = nextStream;
      stream.on("data", (update: SubscribeUpdate) => {
        const receivedAtMs = Date.now();
        for (const targetWallet of activeWallets) {
          const trade = normalizeYellowstoneTrade(update, targetWallet, explorer);
          if (!trade) {
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
      stream.write(buildSubscribeRequest(activeWallets, commitment));
      onStatus(`Yellowstone gRPC subscribed to ${activeWallets.length} wallet(s) at ${commitment} commitment${shadowOnly ? " in shadow mode" : ""}`);
    } catch (error) {
      onError(new Error(`Yellowstone gRPC connection failed: ${errorMessage(error)}`));
      scheduleReconnect();
    }
  }

  return {
    start() {
      running = true;
      if (!enabled || !endpoint || !token) {
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
