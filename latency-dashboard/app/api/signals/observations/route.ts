import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { listSignals, parseSignalFilters } from "@/lib/signals";

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const filters = parseSignalFilters(new URL(request.url).searchParams);
    const signals = await listSignals(filters);
    return Response.json({ signals, filters });
  } catch (error) {
    return authErrorResponse(error);
  }
}
