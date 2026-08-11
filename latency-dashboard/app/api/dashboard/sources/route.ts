import { authErrorResponse } from "@/lib/auth";
import { parseSourceFilters, unsupportedSourceFilters } from "@/lib/dashboard-contract.mjs";
import { listDashboardSources } from "@/lib/dashboard-sources";

export async function GET(request: Request) {
  try {
    const searchParams = new URL(request.url).searchParams;
    const unsupported = unsupportedSourceFilters(searchParams);
    if (unsupported.length > 0) {
      return Response.json({ error: "unsupported_filters", filters: unsupported }, { status: 400 });
    }
    const filters = parseSourceFilters(searchParams);
    const sources = await listDashboardSources(filters);

    return Response.json({
      sources,
      filters
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}
