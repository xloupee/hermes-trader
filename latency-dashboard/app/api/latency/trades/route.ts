import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { listTrades, parseFilters } from "@/lib/latency";

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const filters = parseFilters(new URL(request.url).searchParams);
    const trades = await listTrades(filters);
    return Response.json({ trades, filters });
  } catch (error) {
    return authErrorResponse(error);
  }
}
