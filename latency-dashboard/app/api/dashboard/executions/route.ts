import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { encodeExecutionCursor, pageExecutionRows, parseExecutionFilters, summarizeExecutions, toDashboardExecution } from "@/lib/dashboard-contract.mjs";
import { listDashboardExecutions } from "@/lib/local-executions";

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const filters = parseExecutionFilters(new URL(request.url).searchParams);
    const fetchedRows = await listDashboardExecutions(filters);
    const page = pageExecutionRows(fetchedRows, filters.limit);
    const executions = page.items.map(toDashboardExecution);
    const lastExecution = executions[executions.length - 1];
    const nextCursor = lastExecution ? encodeExecutionCursor(lastExecution) : null;

    return Response.json({
      executions,
      summary: summarizeExecutions(executions),
      pagination: {
        limit: filters.limit,
        hasMore: page.hasMore,
        nextCursor
      },
      filters
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}
