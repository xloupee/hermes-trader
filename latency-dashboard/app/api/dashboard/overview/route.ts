import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { summarizeExecutions, toDashboardExecution } from "@/lib/dashboard-contract.mjs";
import { listDashboardExecutions } from "@/lib/local-executions";
import { parseExecutionFilters } from "@/lib/dashboard-contract.mjs";

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const filters = parseExecutionFilters(new URL(request.url).searchParams);
    const executions = await listDashboardExecutions(filters);

    const sanitized = executions.map(toDashboardExecution);
    const summary = summarizeExecutions(sanitized);
    const latestAt = sanitized[0]?.observedAtMs ?? null;
    const sourceCount = new Set(sanitized.map((row) => row.source)).size;

    return Response.json({
      summary,
      latestObservedAtMs: latestAt,
      sourcesObserved: sourceCount,
      executions: sanitized,
      filters: {
        ...filters,
        cursor: null
      }
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}
