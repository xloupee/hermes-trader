import type { ExplorerConfig, WalletTradeData, WatchedWallet } from "./types.js";

export const PUMP_BONDING_CURVE_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
export const PUMPSWAP_AMM_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
export const FLASHX_ROUTER_PROGRAM_ID = "FLASHX8DrLbgeR8FcfNV1F5krxYcYMUdBkrP1EPBtxB9";
export const WSOL_MINT = "So11111111111111111111111111111111111111112";

export type PumpDiscoveryEventType = "create" | "buy" | "sell" | "migrate" | "unknown-pump";
export type ShredstreamDecodeStatus = "decoded" | "invalid-data" | "missing-discriminator" | "unknown-discriminator";

export interface RawPumpDiscoveryEvent {
  source: "shredstream";
  slot: number;
  signature: string;
  receivedAtMs: number;
  sourceTiming?: ShredstreamSourceTiming;
  programId: string;
  routerProgramId?: string;
  eventType: PumpDiscoveryEventType;
  mint?: string;
  trader?: string;
  bondingCurve?: string;
  baseMint?: string;
  quoteMint?: string;
  pool?: "pump" | "pump-amm";
  amountSemantics?: string;
  solAmountLamports?: string;
  tokenAmountRaw?: string;
  maxSolCostLamports?: string;
  spendableSolLamports?: string;
  minSolOutputLamports?: string;
  maxQuoteAmountIn?: string;
  spendableQuoteAmountIn?: string;
  minQuoteAmountOut?: string;
  minTokenAmountOut?: string;
  rawInstructionDiscriminator?: string;
  instructionIndex?: number;
  decodeStatus?: ShredstreamDecodeStatus;
}

export interface ShredstreamInstructionInput {
  programIdIndex?: number;
  programId?: string;
  accounts?: Array<number | string>;
  dataBase64?: string;
}

export interface ShredstreamAddressTableLookupInput {
  accountKey: string;
  writableIndexes: number[];
  readonlyIndexes: number[];
}

export interface ShredstreamTransactionInput {
  slot: number;
  signature: string;
  receivedAtMs?: number;
  sourceTiming?: ShredstreamSourceTiming;
  accountKeys: string[];
  addressTableLookups?: ShredstreamAddressTableLookupInput[];
  preTokenBalances?: ShredstreamTokenBalanceInput[];
  postTokenBalances?: ShredstreamTokenBalanceInput[];
  instructions: ShredstreamInstructionInput[];
}

export interface ShredstreamTokenBalanceInput {
  accountIndex: number;
  mint: string;
  owner?: string | null;
  uiTokenAmount?: {
    amount?: string | number | null;
    decimals?: number | null;
  } | null;
}

export interface ShredstreamSourceTiming {
  sourceReadAtMs?: number;
  parsedAtMs?: number;
  altLookupStatus?: "not_needed" | "static_decoded" | "hydrated" | "timeout_or_error";
  altLookupCount?: number;
  altLookupStartedAtMs?: number;
  altLookupFinishedAtMs?: number;
  altLookupDurationMs?: number;
  altLookupTimeoutMs?: number;
}

export interface NormalizeShredstreamTransactionOptions {
  receivedAtMs?: number;
}

export interface ShredstreamWalletTradeOptions {
  event: RawPumpDiscoveryEvent;
  wallet: Pick<WatchedWallet, "address" | "label">;
  explorer: ExplorerConfig;
}

interface AccountIndexes {
  mint?: number;
  trader?: number;
  bondingCurve?: number;
  pool?: number;
  baseMint?: number;
  quoteMint?: number;
}

type AmountField =
  | "tokenAmountRaw"
  | "maxSolCostLamports"
  | "spendableSolLamports"
  | "minSolOutputLamports"
  | "maxQuoteAmountIn"
  | "spendableQuoteAmountIn"
  | "minQuoteAmountOut"
  | "minTokenAmountOut";

interface InstructionDecoder {
  eventType: Exclude<PumpDiscoveryEventType, "unknown-pump">;
  accounts: AccountIndexes;
  amountArgs?: Partial<Record<number, AmountField>>;
  amountSemantics?: string;
}

interface TokenBalanceDelta {
  mint: string;
  rawAmount: bigint;
  decimals: number | null;
}

type FlashxRouterTradeShape = "buy" | "sell" | "ambiguous";

// Verified against @pump-fun/pump-sdk@1.36.0 src/idl/pump.json.
const PUMP_INSTRUCTION_DECODERS: Record<string, InstructionDecoder> = {
  "181ec828051c0777": {
    eventType: "create",
    accounts: {
      mint: 0,
      trader: 7,
      bondingCurve: 2,
      baseMint: 0
    }
  },
  d6904cec5f8b31b4: {
    eventType: "create",
    accounts: {
      mint: 0,
      trader: 5,
      bondingCurve: 2,
      baseMint: 0
    }
  },
  "66063d1201daebea": {
    eventType: "buy",
    accounts: {
      mint: 2,
      trader: 6,
      bondingCurve: 3,
      baseMint: 2
    },
    amountArgs: { 0: "tokenAmountRaw", 1: "maxSolCostLamports" },
    amountSemantics: "token_amount_out_with_max_sol_cost"
  },
  "38fc74089edfcd5f": {
    eventType: "buy",
    accounts: {
      mint: 2,
      trader: 6,
      bondingCurve: 3,
      baseMint: 2
    },
    amountArgs: { 0: "spendableSolLamports", 1: "minTokenAmountOut" },
    amountSemantics: "spendable_sol_in_with_min_tokens_out"
  },
  c2ab1c46684d5b2f: {
    eventType: "buy",
    accounts: {
      mint: 1,
      trader: 13,
      bondingCurve: 10,
      baseMint: 1,
      quoteMint: 2
    },
    amountArgs: { 0: "spendableQuoteAmountIn", 1: "minTokenAmountOut" },
    amountSemantics: "spendable_quote_in_with_min_tokens_out"
  },
  b817ee6167c5d33d: {
    eventType: "buy",
    accounts: {
      mint: 1,
      trader: 13,
      bondingCurve: 10,
      baseMint: 1,
      quoteMint: 2
    },
    amountArgs: { 0: "tokenAmountRaw", 1: "maxQuoteAmountIn" },
    amountSemantics: "token_amount_out_with_max_quote_in"
  },
  "33e685a4017f83ad": {
    eventType: "sell",
    accounts: {
      mint: 2,
      trader: 6,
      bondingCurve: 3,
      baseMint: 2
    },
    amountArgs: { 0: "tokenAmountRaw", 1: "minSolOutputLamports" },
    amountSemantics: "token_amount_in_with_min_sol_output"
  },
  "5df6823ce7e940b2": {
    eventType: "sell",
    accounts: {
      mint: 1,
      trader: 13,
      bondingCurve: 10,
      baseMint: 1,
      quoteMint: 2
    },
    amountArgs: { 0: "tokenAmountRaw", 1: "minQuoteAmountOut" },
    amountSemantics: "token_amount_in_with_min_quote_out"
  },
  "9beae792ec9ea21e": {
    eventType: "migrate",
    accounts: {
      mint: 2,
      trader: 5,
      bondingCurve: 3,
      pool: 9,
      baseMint: 2
    }
  },
  bbcb121fceedfe29: {
    eventType: "migrate",
    accounts: {
      mint: 2,
      trader: 7,
      bondingCurve: 4,
      pool: 9,
      baseMint: 2,
      quoteMint: 3
    }
  }
};

// Verified against @pump-fun/pump-swap-sdk@1.16.0 src/idl/pump_amm.json.
const PUMPSWAP_INSTRUCTION_DECODERS: Record<string, InstructionDecoder> = {
  "66063d1201daebea": {
    eventType: "buy",
    accounts: {
      pool: 0,
      trader: 1,
      mint: 3,
      baseMint: 3,
      quoteMint: 4
    },
    amountArgs: { 0: "tokenAmountRaw", 1: "maxQuoteAmountIn" },
    amountSemantics: "base_amount_out_with_max_quote_in"
  },
  c62e1552b4d9e870: {
    eventType: "buy",
    accounts: {
      pool: 0,
      trader: 1,
      mint: 3,
      baseMint: 3,
      quoteMint: 4
    },
    amountArgs: { 0: "spendableQuoteAmountIn", 1: "minTokenAmountOut" },
    amountSemantics: "spendable_quote_in_with_min_base_out"
  },
  "33e685a4017f83ad": {
    eventType: "sell",
    accounts: {
      pool: 0,
      trader: 1,
      mint: 3,
      baseMint: 3,
      quoteMint: 4
    },
    amountArgs: { 0: "tokenAmountRaw", 1: "minQuoteAmountOut" },
    amountSemantics: "base_amount_in_with_min_quote_out"
  }
};

export function normalizeShredstreamTransaction(
  transaction: ShredstreamTransactionInput,
  options: NormalizeShredstreamTransactionOptions = {}
): RawPumpDiscoveryEvent[] {
  const receivedAtMs = options.receivedAtMs ?? transaction.receivedAtMs ?? Date.now();
  const events: RawPumpDiscoveryEvent[] = [];

  for (const [instructionIndex, instruction] of transaction.instructions.entries()) {
    const programId = resolveProgramId(transaction.accountKeys, instruction);

    if (isPumpProgram(programId)) {
      events.push(
        decodePumpInstruction({
          transaction,
          instruction,
          programId,
          instructionIndex,
          receivedAtMs
        })
      );
      continue;
    }

    const routerEvent = decodeKnownRouterInstruction({
      transaction,
      instruction,
      programId,
      instructionIndex,
      receivedAtMs
    });

    if (routerEvent) {
      events.push(routerEvent);
    }
  }

  return events;
}

export function isKnownPumpRouterProgram(programId: string | null): programId is string {
  return programId === FLASHX_ROUTER_PROGRAM_ID;
}

function decodeKnownRouterInstruction({
  transaction,
  instruction,
  programId,
  instructionIndex,
  receivedAtMs
}: {
  transaction: ShredstreamTransactionInput;
  instruction: ShredstreamInstructionInput;
  programId: string | null;
  instructionIndex: number;
  receivedAtMs: number;
}): RawPumpDiscoveryEvent | null {
  if (!isKnownPumpRouterProgram(programId)) {
    return null;
  }

  return decodeFlashxPumpInstruction({
    transaction,
    instruction,
    routerProgramId: programId,
    instructionIndex,
    receivedAtMs
  });
}

function decodeFlashxPumpInstruction({
  transaction,
  instruction,
  routerProgramId,
  instructionIndex,
  receivedAtMs
}: {
  transaction: ShredstreamTransactionInput;
  instruction: ShredstreamInstructionInput;
  routerProgramId: string;
  instructionIndex: number;
  receivedAtMs: number;
}): RawPumpDiscoveryEvent | null {
  const accounts = instruction.accounts || [];
  const resolvedAccounts = accounts.map((_, index) => resolveAccount(transaction.accountKeys, accounts, index));

  const data = decodeBase64(instruction.dataBase64);
  if (!data || data.length < 17 || data[0] !== 0) {
    return null;
  }

  const spendableSolLamports = readRouterU64(data, 1);
  const minTokenAmountOut = readRouterU64(data, 9);
  const trader = resolvedAccounts.find((account) => account === transaction.accountKeys[0]) || transaction.accountKeys[0] || undefined;
  const balanceDelta = trader ? singleTokenBalanceDelta(transaction, trader) : null;
  const tradeShape = balanceDelta ? (balanceDelta.rawAmount > 0n ? "buy" : "sell") : inferFlashxRouterTradeShape({
    spendableSolLamports,
    minTokenAmountOut
  });

  if (tradeShape !== "buy") {
    return null;
  }

  const pumpSwapMint = resolveFlashxPumpSwapBaseMint(resolvedAccounts);
  const mint =
    balanceDelta?.mint ||
    pumpSwapMint ||
    resolvedAccounts.find((account) => account && looksLikePumpMint(account)) ||
    resolvedAccounts[10] ||
    undefined;
  const bondingCurve = resolveFlashxBondingCurve(resolvedAccounts, mint);
  const isPumpSwapRoute = Boolean(pumpSwapMint || resolvedAccounts.includes(PUMPSWAP_AMM_PROGRAM_ID));

  if (!mint || !trader || !spendableSolLamports) {
    return null;
  }

  const event: RawPumpDiscoveryEvent = {
    source: "shredstream",
    slot: transaction.slot,
    signature: transaction.signature,
    receivedAtMs,
    ...(transaction.sourceTiming ? { sourceTiming: transaction.sourceTiming } : {}),
    programId: isPumpSwapRoute ? PUMPSWAP_AMM_PROGRAM_ID : PUMP_BONDING_CURVE_PROGRAM_ID,
    routerProgramId,
    eventType: "buy",
    mint,
    trader,
    bondingCurve: isPumpSwapRoute ? undefined : bondingCurve || undefined,
    baseMint: mint,
    quoteMint: WSOL_MINT,
    pool: isPumpSwapRoute ? "pump-amm" : "pump",
    amountSemantics: "flashx_spendable_sol_in_with_min_tokens_out",
    spendableSolLamports,
    solAmountLamports: spendableSolLamports,
    tokenAmountRaw: balanceDelta && balanceDelta.rawAmount > 0n ? balanceDelta.rawAmount.toString() : undefined,
    minTokenAmountOut: minTokenAmountOut || undefined,
    rawInstructionDiscriminator: data?.subarray(0, Math.min(8, data.length)).toString("hex"),
    instructionIndex,
    decodeStatus: "decoded"
  };

  return event;
}

export function isPumpProgram(programId: string | null): programId is string {
  return programId === PUMP_BONDING_CURVE_PROGRAM_ID || programId === PUMPSWAP_AMM_PROGRAM_ID;
}

/*
  Router instructions are top-level non-Pump programs that CPI into Pump. The raw
  shred does not include inner instructions, so we decode the stable account/data
  shape that carries the watched wallet, mint, bonding curve, and spend amount.
*/
function readRouterU64(data: Buffer | null, offset: number): string | null {
  if (!data || data.length < offset + 8) {
    return null;
  }

  return data.readBigUInt64LE(offset).toString();
}

function looksLikePumpMint(account: string): boolean {
  return account.endsWith("pump") || account.endsWith("bonk");
}

function resolveFlashxPumpSwapBaseMint(resolvedAccounts: Array<string | null>): string | undefined {
  const programIndex = resolvedAccounts.indexOf(PUMPSWAP_AMM_PROGRAM_ID);
  if (programIndex < 0) {
    return undefined;
  }

  const quoteMintIndex = resolvedAccounts.findIndex((account, index) => index > programIndex && account === WSOL_MINT);
  if (quoteMintIndex > programIndex + 1) {
    const baseMint = resolvedAccounts[quoteMintIndex - 1];
    if (baseMint && baseMint !== PUMPSWAP_AMM_PROGRAM_ID) {
      return baseMint;
    }
  }

  const shapedBaseMint = resolvedAccounts[programIndex + 7];
  return shapedBaseMint && shapedBaseMint !== WSOL_MINT ? shapedBaseMint : undefined;
}

function resolveFlashxBondingCurve(resolvedAccounts: Array<string | null>, mint: string | undefined): string | undefined {
  if (!mint) {
    return undefined;
  }

  const mintIndex = resolvedAccounts.indexOf(mint);

  if (mintIndex >= 0) {
    return resolvedAccounts[mintIndex + 1] || undefined;
  }

  return undefined;
}

function inferFlashxRouterTradeShape({
  spendableSolLamports,
  minTokenAmountOut
}: {
  spendableSolLamports: string | null;
  minTokenAmountOut: string | null;
}): FlashxRouterTradeShape {
  const firstAmount = bigintString(spendableSolLamports);
  const secondAmount = bigintString(minTokenAmountOut);

  if (firstAmount === null || secondAmount === null || secondAmount === 0n) {
    return "ambiguous";
  }

  if (firstAmount > secondAmount) {
    return "sell";
  }

  return "buy";
}

function singleTokenBalanceDelta(
  transaction: ShredstreamTransactionInput,
  owner: string
): TokenBalanceDelta | null {
  const deltas = tokenBalanceDeltasForOwner(transaction, owner);
  return deltas.length === 1 ? deltas[0] : null;
}

function tokenBalanceDeltasForOwner(transaction: ShredstreamTransactionInput, owner: string): TokenBalanceDelta[] {
  const byAccountAndMint = new Map<string, TokenBalanceDelta>();

  function apply(balance: ShredstreamTokenBalanceInput, sign: 1n | -1n): void {
    if (balance.owner !== owner || !balance.mint || balance.mint === WSOL_MINT) {
      return;
    }

    const rawAmount = rawTokenBalanceAmount(balance);

    if (rawAmount === null) {
      return;
    }

    const key = `${balance.accountIndex}:${balance.mint}`;
    const previous = byAccountAndMint.get(key) || {
      mint: balance.mint,
      rawAmount: 0n,
      decimals: balance.uiTokenAmount?.decimals ?? null
    };

    byAccountAndMint.set(key, {
      ...previous,
      rawAmount: previous.rawAmount + rawAmount * sign,
      decimals: previous.decimals ?? balance.uiTokenAmount?.decimals ?? null
    });
  }

  for (const balance of transaction.preTokenBalances || []) {
    apply(balance, -1n);
  }

  for (const balance of transaction.postTokenBalances || []) {
    apply(balance, 1n);
  }

  const byMint = new Map<string, TokenBalanceDelta>();

  for (const delta of byAccountAndMint.values()) {
    if (delta.rawAmount === 0n) {
      continue;
    }

    const previous = byMint.get(delta.mint) || {
      mint: delta.mint,
      rawAmount: 0n,
      decimals: delta.decimals
    };

    byMint.set(delta.mint, {
      ...previous,
      rawAmount: previous.rawAmount + delta.rawAmount,
      decimals: previous.decimals ?? delta.decimals
    });
  }

  return [...byMint.values()].filter((delta) => delta.rawAmount !== 0n);
}

function rawTokenBalanceAmount(balance: ShredstreamTokenBalanceInput): bigint | null {
  const value = balance.uiTokenAmount?.amount;

  if (typeof value === "number") {
    return Number.isSafeInteger(value) && value >= 0 ? BigInt(value) : null;
  }

  if (typeof value !== "string" || !/^\d+$/.test(value)) {
    return null;
  }

  return BigInt(value);
}

function bigintString(value: string | null): bigint | null {
  if (typeof value !== "string" || !/^\d+$/.test(value)) {
    return null;
  }

  return BigInt(value);
}

function decodePumpInstruction({
  transaction,
  instruction,
  programId,
  instructionIndex,
  receivedAtMs
}: {
  transaction: ShredstreamTransactionInput;
  instruction: ShredstreamInstructionInput;
  programId: string;
  instructionIndex: number;
  receivedAtMs: number;
}): RawPumpDiscoveryEvent {
  const event: RawPumpDiscoveryEvent = {
    source: "shredstream",
    slot: transaction.slot,
    signature: transaction.signature,
    receivedAtMs,
    ...(transaction.sourceTiming ? { sourceTiming: transaction.sourceTiming } : {}),
    programId,
    eventType: "unknown-pump",
    pool: programId === PUMPSWAP_AMM_PROGRAM_ID ? "pump-amm" : "pump",
    instructionIndex
  };

  const data = decodeBase64(instruction.dataBase64);

  if (!data) {
    event.decodeStatus = "invalid-data";
    return event;
  }

  if (data.length < 8) {
    event.decodeStatus = "missing-discriminator";
    return event;
  }

  const discriminator = data.subarray(0, 8).toString("hex");
  event.rawInstructionDiscriminator = discriminator;

  const decoder = instructionDecoder(programId, discriminator);

  if (!decoder) {
    event.decodeStatus = "unknown-discriminator";
    applyBestEffortAccounts(event, transaction.accountKeys, instruction.accounts || [], programId);
    return event;
  }

  event.decodeStatus = "decoded";
  event.eventType = decoder.eventType;
  applyDecodedAccounts(event, transaction.accountKeys, instruction.accounts || [], decoder.accounts);
  applyDecodedAmounts(event, data, decoder);

  return event;
}

export function rawPumpDiscoveryEventToWalletTrade({
  event,
  wallet,
  explorer
}: ShredstreamWalletTradeOptions): WalletTradeData | null {
  if (
    event.decodeStatus !== "decoded" ||
    (event.eventType !== "buy" && event.eventType !== "sell") ||
    !event.mint ||
    !event.trader ||
    event.trader !== wallet.address
  ) {
    return null;
  }

  const input = event.eventType === "buy"
    ? { mint: event.quoteMint || WSOL_MINT, symbol: event.quoteMint && event.quoteMint !== WSOL_MINT ? null : "SOL", amount: null }
    : { mint: event.mint, symbol: null, amount: tokenAmount(event) };
  const output = event.eventType === "buy"
    ? { mint: event.mint, symbol: null, amount: tokenAmount(event) }
    : { mint: event.quoteMint || WSOL_MINT, symbol: event.quoteMint && event.quoteMint !== WSOL_MINT ? null : "SOL", amount: null };

  return {
    observedAt: new Date(event.receivedAtMs).toISOString(),
    provider: "shredstream",
    targetWallet: wallet.address,
    label: wallet.label || null,
    action: event.eventType,
    mint: event.mint,
    signature: event.signature,
    timestamp: Math.floor(event.receivedAtMs / 1000),
    feePayer: event.trader,
    source: event.pool === "pump-amm" ? "SHREDSTREAM_PUMP_AMM" : "SHREDSTREAM_PUMP",
    input,
    output,
    solAmount: solAmount(event),
    tokenAmount: tokenAmount(event),
    pool: event.pool || null,
    marketCapSol: null,
    pumpFunUrl: `${explorer.pumpFunBaseUrl}/${event.mint}`,
    solscanTokenUrl: `${explorer.solscanBaseUrl}/token/${event.mint}`,
    solscanTxUrl: `${explorer.solscanBaseUrl}/tx/${event.signature}`,
    raw: {
      ...event,
      parser: "shredstream-pump-instruction"
    }
  };
}

function lamportsToSol(value: string | undefined): number | null {
  if (!value) {
    return null;
  }

  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed / 1_000_000_000 : null;
}

function tokenAmount(event: RawPumpDiscoveryEvent): number | null {
  if (!event.tokenAmountRaw) {
    return null;
  }

  const parsed = Number(event.tokenAmountRaw);
  return Number.isFinite(parsed) ? parsed / 1_000_000 : null;
}

function solAmount(event: RawPumpDiscoveryEvent): number | null {
  if (event.quoteMint && event.quoteMint !== WSOL_MINT) {
    return null;
  }

  return (
    lamportsToSol(event.spendableSolLamports) ??
    lamportsToSol(event.maxSolCostLamports) ??
    lamportsToSol(event.minSolOutputLamports) ??
    lamportsToSol(event.maxQuoteAmountIn) ??
    lamportsToSol(event.spendableQuoteAmountIn) ??
    lamportsToSol(event.minQuoteAmountOut)
  );
}

function instructionDecoder(programId: string, discriminator: string): InstructionDecoder | null {
  const decoders = programId === PUMPSWAP_AMM_PROGRAM_ID ? PUMPSWAP_INSTRUCTION_DECODERS : PUMP_INSTRUCTION_DECODERS;
  return decoders[discriminator] || null;
}

function resolveProgramId(accountKeys: string[], instruction: ShredstreamInstructionInput): string | null {
  if (instruction.programId) {
    return instruction.programId;
  }

  if (typeof instruction.programIdIndex !== "number") {
    return null;
  }

  return accountKeys[instruction.programIdIndex] || null;
}

function decodeBase64(value: unknown): Buffer | null {
  if (typeof value !== "string") {
    return null;
  }

  const input = value.trim();

  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(input) || input.length % 4 === 1) {
    return null;
  }

  const buffer = Buffer.from(input, "base64");
  const normalizedInput = input.replace(/=+$/, "");
  const normalizedOutput = buffer.toString("base64").replace(/=+$/, "");

  return normalizedOutput === normalizedInput ? buffer : null;
}

function applyDecodedAccounts(
  event: RawPumpDiscoveryEvent,
  accountKeys: string[],
  accounts: Array<number | string>,
  indexes: AccountIndexes
): void {
  const mint = resolveAccount(accountKeys, accounts, indexes.mint);
  const trader = resolveAccount(accountKeys, accounts, indexes.trader);
  const bondingCurve = resolveAccount(accountKeys, accounts, indexes.bondingCurve);
  const pool = resolveAccount(accountKeys, accounts, indexes.pool);
  const baseMint = resolveAccount(accountKeys, accounts, indexes.baseMint);
  const quoteMint = resolveAccount(accountKeys, accounts, indexes.quoteMint);

  if (mint) {
    event.mint = mint;
  }

  if (trader) {
    event.trader = trader;
  }

  if (bondingCurve) {
    event.bondingCurve = bondingCurve;
  }

  if (baseMint) {
    event.baseMint = baseMint;
  }

  if (quoteMint) {
    event.quoteMint = quoteMint;
  }

  if (pool) {
    event.pool = event.programId === PUMPSWAP_AMM_PROGRAM_ID ? "pump-amm" : "pump";
  }
}

function applyBestEffortAccounts(
  event: RawPumpDiscoveryEvent,
  accountKeys: string[],
  accounts: Array<number | string>,
  programId: string
): void {
  const indexes: AccountIndexes =
    programId === PUMPSWAP_AMM_PROGRAM_ID
      ? {
          pool: 0,
          trader: 1,
          mint: 3,
          baseMint: 3,
          quoteMint: 4
        }
      : {
          mint: 2,
          trader: 6,
          bondingCurve: 3,
          baseMint: 2
        };

  applyDecodedAccounts(event, accountKeys, accounts, indexes);
}

function resolveAccount(accountKeys: string[], accounts: Array<number | string>, instructionAccountIndex?: number): string | null {
  if (instructionAccountIndex === undefined) {
    return null;
  }

  const account = accounts[instructionAccountIndex];

  if (typeof account === "string") {
    return account;
  }

  if (typeof account !== "number") {
    return null;
  }

  return accountKeys[account] || null;
}

function applyDecodedAmounts(event: RawPumpDiscoveryEvent, data: Buffer, decoder: InstructionDecoder): void {
  if (decoder.amountSemantics) {
    event.amountSemantics = decoder.amountSemantics;
  }

  for (const [argIndexString, field] of Object.entries(decoder.amountArgs || {}) as Array<[string, AmountField]>) {
    const value = readU64Arg(data, Number(argIndexString));

    if (value === null) {
      continue;
    }

    event[field] = value;

    if (
      event.quoteMint === WSOL_MINT &&
      (field === "maxQuoteAmountIn" || field === "spendableQuoteAmountIn" || field === "minQuoteAmountOut")
    ) {
      event.solAmountLamports = value;
    }
  }
}

function readU64Arg(data: Buffer, argIndex?: number): string | null {
  if (argIndex === undefined) {
    return null;
  }

  const offset = 8 + argIndex * 8;

  if (data.length < offset + 8) {
    return null;
  }

  return data.readBigUInt64LE(offset).toString();
}
