import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { listTrades, parseFilters, summarizeTrades } from "@/lib/latency";

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const filters = parseFilters(new URL(request.url).searchParams);
    const trades = await listTrades({ ...filters, limit: Math.max(filters.limit, 500) });
    return Response.json({ summary: summarizeTrades(trades), filters });
  } catch (error) {
    return authErrorResponse(error);
  }
}
