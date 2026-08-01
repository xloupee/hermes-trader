import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { createAdminClient } from "@/lib/supabase/admin";

async function tableCount(table: "copytrade_local_executions" | "copytrade_signal_observations"): Promise<number | null> {
  const supabase = createAdminClient();
  const { count, error } = await supabase
    .from(table)
    .select("id", { count: "exact", head: true });

  if (error) {
    return null;
  }
  return count ?? 0;
}

export async function GET() {
  try {
    await requireAdmin();
    const [localExecutions, signalObservations] = await Promise.all([
      tableCount("copytrade_local_executions"),
      tableCount("copytrade_signal_observations")
    ]);

    return Response.json({
      time: new Date().toISOString(),
      tables: {
        copytradeLocalExecutions: localExecutions,
        copytradeSignalObservations: signalObservations
      },
      environment: {
        supabaseUrl: Boolean(process.env.NEXT_PUBLIC_SUPABASE_URL),
        hasServiceRole: Boolean(process.env.SUPABASE_SERVICE_ROLE_KEY)
      }
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}
