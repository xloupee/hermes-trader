import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { encodeExecutionCursor, parseExecutionFilters, summarizeExecutions, toDashboardExecution } from "@/lib/dashboard-contract.mjs";
import { listDashboardExecutions } from "@/lib/local-executions";

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const filters = parseExecutionFilters(new URL(request.url).searchParams);
    const rows = await listDashboardExecutions(filters);
    const executions = rows.map(toDashboardExecution);
    const lastExecution = executions[executions.length - 1];
    const nextCursor = lastExecution ? encodeExecutionCursor(lastExecution) : null;

    return Response.json({
      executions,
      summary: summarizeExecutions(executions),
      pagination: {
        limit: filters.limit,
        hasMore: Boolean(nextCursor),
        nextCursor
      },
      filters
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}
