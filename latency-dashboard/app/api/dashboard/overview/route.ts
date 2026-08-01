import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { dashboardOverviewSummary } from "@/lib/local-executions";
import { parseExecutionFilters } from "@/lib/dashboard-contract.mjs";

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const filters = parseExecutionFilters(new URL(request.url).searchParams);
    const summary = await dashboardOverviewSummary({ ...filters, cursor: null });

    return Response.json({
      summary,
      filters: {
        ...filters,
        cursor: null
      }
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}
