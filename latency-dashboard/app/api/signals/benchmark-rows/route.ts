import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { listBenchmarkRows } from "@/lib/benchmark-rows";
import { parseSignalFilters } from "@/lib/signals";

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const filters = parseSignalFilters(new URL(request.url).searchParams);
    return Response.json(await listBenchmarkRows(filters));
  } catch (error) {
    return authErrorResponse(error);
  }
}
