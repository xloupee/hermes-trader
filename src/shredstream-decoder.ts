export const PUMP_BONDING_CURVE_PROGRAM_ID = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
export const PUMPSWAP_AMM_PROGRAM_ID = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
export const WSOL_MINT = "So11111111111111111111111111111111111111112";

export type PumpDiscoveryEventType = "create" | "buy" | "sell" | "migrate" | "unknown-pump";
export type ShredstreamDecodeStatus = "decoded" | "invalid-data" | "missing-discriminator" | "unknown-discriminator";

export interface RawPumpDiscoveryEvent {
  source: "shredstream";
  slot: number;
  signature: string;
  receivedAtMs: number;
  programId: string;
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

export interface ShredstreamTransactionInput {
  slot: number;
  signature: string;
  receivedAtMs?: number;
  accountKeys: string[];
  instructions: ShredstreamInstructionInput[];
}

export interface NormalizeShredstreamTransactionOptions {
  receivedAtMs?: number;
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

    if (!isPumpProgram(programId)) {
      continue;
    }

    events.push(
      decodePumpInstruction({
        transaction,
        instruction,
        programId,
        instructionIndex,
        receivedAtMs
      })
    );
  }

  return events;
}

export function isPumpProgram(programId: string | null): programId is string {
  return programId === PUMP_BONDING_CURVE_PROGRAM_ID || programId === PUMPSWAP_AMM_PROGRAM_ID;
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
