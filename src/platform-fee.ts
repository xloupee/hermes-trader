export interface PlatformFeeConfig {
  enabled?: boolean;
  bps?: number;
  treasury?: string | null;
  validateTreasury?: (treasury: string) => string | null;
}

export interface PlatformFeeSplit {
  enabled: boolean;
  bps: number;
  treasury: string | null;
  budgetLamports: bigint;
  feeLamports: bigint;
  tradeLamports: bigint;
  blockedReason: string | null;
}

export interface PlatformFeeTransferInstruction {
  kind: "system-transfer";
  fromPubkey: string;
  toPubkey: string;
  lamports: bigint;
}

const DEFAULT_PLATFORM_FEE_BPS = 100;
const BPS_DENOMINATOR = 10_000n;

function normalizedTreasury(treasury: string | null | undefined): string | null {
  const value = treasury?.trim();
  return value ? value : null;
}

function configBps(config: PlatformFeeConfig): number {
  return config.bps ?? DEFAULT_PLATFORM_FEE_BPS;
}

export function platformFeeConfigBlockedReason(config: PlatformFeeConfig): string | null {
  if (!config.enabled) {
    return null;
  }

  const bps = configBps(config);
  if (!Number.isInteger(bps) || bps < 0 || bps > 10_000) {
    return `PLATFORM_FEE_BPS must be an integer from 0 to 10000; got ${String(config.bps)}`;
  }

  const treasury = normalizedTreasury(config.treasury);
  if (!treasury) {
    return "PLATFORM_FEE_TREASURY is required when PLATFORM_FEE_ENABLED is true";
  }

  return config.validateTreasury?.(treasury) || null;
}

export function calculatePlatformFeeSplit({
  action,
  budgetLamports,
  config
}: {
  action: "buy" | "sell";
  budgetLamports: bigint | number;
  config: PlatformFeeConfig;
}): PlatformFeeSplit {
  const budget = BigInt(budgetLamports);
  if (budget < 0n) {
    throw new Error("budgetLamports must be non-negative");
  }

  const bps = configBps(config);
  const treasury = normalizedTreasury(config.treasury);
  const blockedReason = platformFeeConfigBlockedReason(config);

  if (!config.enabled || blockedReason || bps === 0) {
    return {
      enabled: Boolean(config.enabled && !blockedReason && bps > 0),
      bps,
      treasury,
      budgetLamports: budget,
      feeLamports: 0n,
      tradeLamports: budget,
      blockedReason
    };
  }

  const feeLamports = (budget * BigInt(bps)) / BPS_DENOMINATOR;
  return {
    enabled: true,
    bps,
    treasury,
    budgetLamports: budget,
    feeLamports,
    tradeLamports: budget - feeLamports,
    blockedReason: null
  };
}

export function buildPlatformFeeTransferInstruction({
  split,
  fromPubkey
}: {
  split: PlatformFeeSplit;
  fromPubkey: string;
}): PlatformFeeTransferInstruction | null {
  if (!split.enabled || split.feeLamports <= 0n || !split.treasury || split.blockedReason) {
    return null;
  }

  return {
    kind: "system-transfer",
    fromPubkey,
    toPubkey: split.treasury,
    lamports: split.feeLamports
  };
}

export function formatPlatformFeeDisclosure(split: PlatformFeeSplit): string {
  if (!split.enabled || split.feeLamports === 0n) {
    return `platformFee=0 | tradeLamports=${split.tradeLamports.toString()} | budgetLamports=${split.budgetLamports.toString()}`;
  }

  return [
    `platformFeeBps=${split.bps}`,
    `platformFeeLamports=${split.feeLamports.toString()}`,
    `tradeLamports=${split.tradeLamports.toString()}`,
    `budgetLamports=${split.budgetLamports.toString()}`,
    `treasury=${split.treasury || "none"}`
  ].join(" | ");
}
