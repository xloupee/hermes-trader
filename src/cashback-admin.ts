import { config as loadDotenv } from "dotenv";
import {
  createSupabaseCashbackStore,
  formatCashbackSol,
  normalizeCashbackChatId,
  parseCashbackConfig,
  validateCashbackFeeShareBps
} from "./cashback.js";
import type { CashbackConfig, ResolvedCashbackConfig } from "./cashback.js";

function valueAfter(args: string[], flag: string): string | null {
  const index = args.indexOf(flag);
  return index === -1 ? null : args[index + 1] || null;
}

function serviceRoleKey(env: NodeJS.ProcessEnv): string {
  return env.SUPABASE_SERVICE_ROLE_KEY || env.SUPABASE_SERVICE_KEY || env.SUPABASE_SERVICE_ROLE || "";
}

function parseBoolean(value: string): boolean {
  const normalized = value.trim().toLowerCase();
  if (normalized === "true" || normalized === "on" || normalized === "yes" || normalized === "1") {
    return true;
  }
  if (normalized === "false" || normalized === "off" || normalized === "no" || normalized === "0") {
    return false;
  }

  throw new Error("enabled override must be true or false");
}

function parseLamports(value: string): bigint {
  if (!/^-?\d+$/.test(value.trim())) {
    throw new Error("adjustment lamports must be an integer");
  }

  return BigInt(value);
}

function requireArg(args: string[], index: number, label: string): string {
  const value = args[index];
  if (!value) {
    throw new Error(`${label} is required`);
  }

  return value;
}

function operator(args: string[]): string {
  const updatedBy = valueAfter(args, "--updated-by") || process.env.USER || "";
  if (!updatedBy.trim()) {
    throw new Error("--updated-by is required");
  }

  return updatedBy.trim();
}

function printConfig(config: ResolvedCashbackConfig): void {
  console.log(`chatId=${config.chatId}`);
  console.log(`enabled=${config.config.enabled} source=${config.enabledSource}`);
  console.log(`feeShareBps=${config.config.feeShareBps} source=${config.feeShareBpsSource}`);
  console.log(`minClaim=${formatCashbackSol(config.config.minClaimLamports)} SOL`);
  console.log(`maxPayoutPerDay=${formatCashbackSol(config.config.maxPayoutLamportsPerDay)} SOL`);
  console.log(`overrideEnabled=${config.override?.enabledOverride ?? "null"}`);
  console.log(`overrideFeeShareBps=${config.override?.feeShareBpsOverride ?? "null"}`);
  console.log(`overrideNote=${config.override?.note ?? ""}`);
  console.log(`overrideUpdatedBy=${config.override?.updatedBy ?? ""}`);
  console.log(`overrideUpdatedAt=${config.override?.updatedAt ?? ""}`);
}

function usage(exitCode = 1): never {
  console.error([
    "Usage:",
    "  npm run cashback-admin -- show <chat_id>",
    "  npm run cashback-admin -- set-enabled <chat_id> <true|false> --updated-by <operator> [--note <text>]",
    "  npm run cashback-admin -- clear-enabled <chat_id> --updated-by <operator> [--note <text>]",
    "  npm run cashback-admin -- set-fee-share <chat_id> <bps> --updated-by <operator> [--note <text>]",
    "  npm run cashback-admin -- clear-fee-share <chat_id> --updated-by <operator> [--note <text>]",
    "  npm run cashback-admin -- clear-all <chat_id> --updated-by <operator> [--note <text>]",
    "  npm run cashback-admin -- adjust <chat_id> <trading_wallet> <lamports> --reason <text> --updated-by <operator> [--key <execution_key>]",
    "",
    "Options:",
    "  --env <path>    Load an alternate env file before reading SUPABASE_URL and service role key."
  ].join("\n"));
  process.exit(exitCode);
}

async function main(): Promise<void> {
  const args = process.argv.slice(2);
  const envPath = valueAfter(args, "--env") || ".env";
  loadDotenv({ path: envPath });

  const command = args[0];
  if (!command || command === "--help" || command === "-h") {
    usage(command ? 0 : 1);
  }

  const url = process.env.SUPABASE_URL;
  const key = serviceRoleKey(process.env);
  if (!url || !key) {
    throw new Error("SUPABASE_URL and a Supabase service role key are required");
  }

  const store = createSupabaseCashbackStore({ url, serviceRoleKey: key });
  const baseConfig: CashbackConfig = parseCashbackConfig(process.env);

  if (command === "show") {
    const chatId = normalizeCashbackChatId(requireArg(args, 1, "chat_id"));
    printConfig(await store.getSubscriberConfig({ chatId, config: baseConfig }));
    return;
  }

  if (command === "set-enabled") {
    const chatId = normalizeCashbackChatId(requireArg(args, 1, "chat_id"));
    const enabledOverride = parseBoolean(requireArg(args, 2, "enabled override"));
    printConfig(await store.setSubscriberConfigOverride({
      chatId,
      enabledOverride,
      updatedBy: operator(args),
      note: valueAfter(args, "--note"),
      config: baseConfig
    }));
    return;
  }

  if (command === "clear-enabled") {
    const chatId = normalizeCashbackChatId(requireArg(args, 1, "chat_id"));
    printConfig(await store.clearSubscriberConfigOverride({
      chatId,
      field: "enabled",
      updatedBy: operator(args),
      note: valueAfter(args, "--note"),
      config: baseConfig
    }));
    return;
  }

  if (command === "set-fee-share") {
    const chatId = normalizeCashbackChatId(requireArg(args, 1, "chat_id"));
    const feeShareBpsOverride = validateCashbackFeeShareBps(Number(requireArg(args, 2, "fee-share bps")));
    printConfig(await store.setSubscriberConfigOverride({
      chatId,
      feeShareBpsOverride,
      updatedBy: operator(args),
      note: valueAfter(args, "--note"),
      config: baseConfig
    }));
    return;
  }

  if (command === "clear-fee-share" || command === "clear-all") {
    const chatId = normalizeCashbackChatId(requireArg(args, 1, "chat_id"));
    printConfig(await store.clearSubscriberConfigOverride({
      chatId,
      field: command === "clear-all" ? "all" : "feeShareBps",
      updatedBy: operator(args),
      note: valueAfter(args, "--note"),
      config: baseConfig
    }));
    return;
  }

  if (command === "adjust") {
    const chatId = normalizeCashbackChatId(requireArg(args, 1, "chat_id"));
    const tradingWalletPublicKey = requireArg(args, 2, "trading wallet");
    const cashbackLamports = parseLamports(requireArg(args, 3, "adjustment lamports"));
    const reason = valueAfter(args, "--reason");
    if (!reason) {
      throw new Error("--reason is required for manual cashback adjustments");
    }

    const entry = await store.createManualAdjustment({
      chatId,
      tradingWalletPublicKey,
      cashbackLamports,
      reason,
      adjustedBy: operator(args),
      executionKey: valueAfter(args, "--key")
    });
    console.log(`adjustmentId=${entry.id ?? ""}`);
    console.log(`chatId=${entry.chatId}`);
    console.log(`tradingWallet=${entry.tradingWalletPublicKey}`);
    console.log(`cashbackLamports=${entry.cashbackLamports.toString()}`);
    console.log(`reason=${entry.adjustmentReason ?? ""}`);
    console.log(`adjustedBy=${entry.adjustedBy ?? ""}`);
    return;
  }

  usage();
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
