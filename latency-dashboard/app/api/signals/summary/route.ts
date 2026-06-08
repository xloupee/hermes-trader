import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { listSignals, parseSignalFilters, summarizeSignals } from "@/lib/signals";

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const filters = parseSignalFilters(new URL(request.url).searchParams);
    const signals = await listSignals({ ...filters, limit: Math.max(filters.limit, 500) });
    return Response.json({ summary: summarizeSignals(signals), filters });
  } catch (error) {
    return authErrorResponse(error);
  }
}
