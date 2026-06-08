import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { listLocalExecutions, summarizeLocalExecutions } from "@/lib/local-executions";
import { parseSignalFilters } from "@/lib/signals";

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const filters = parseSignalFilters(new URL(request.url).searchParams);
    const executions = await listLocalExecutions(filters);
    return Response.json({
      executions,
      summary: summarizeLocalExecutions(executions),
      filters
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}
