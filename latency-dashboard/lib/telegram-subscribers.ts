import { createAdminClient } from "@/lib/supabase/admin";

interface ExecutionWithCopyWallet {
  copyWallet: string | null;
}

export async function listTelegramSubscribersByCopyWallet(
  executions: ExecutionWithCopyWallet[]
): Promise<Map<string, string>> {
  const copyWallets = [
    ...new Set(
      executions
        .map((execution) => execution.copyWallet)
        .filter((wallet): wallet is string => Boolean(wallet))
    )
  ];

  if (copyWallets.length === 0) {
    return new Map();
  }

  const { data, error } = await createAdminClient()
    .from("telegram_trading_wallets")
    .select("chat_id,public_key")
    .in("public_key", copyWallets);

  if (error) {
    return new Map();
  }

  return new Map(
    ((data as Array<{ chat_id: string | null; public_key: string | null }> | null) || [])
      .filter((row): row is { chat_id: string; public_key: string } => Boolean(row.chat_id && row.public_key))
      .map((row) => [row.public_key, row.chat_id])
  );
}
