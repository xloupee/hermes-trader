import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { getTrade } from "@/lib/latency";

export async function GET(_request: Request, context: { params: Promise<{ id: string }> }) {
  try {
    await requireAdmin();
    const { id } = await context.params;
    const trade = await getTrade(id);
    if (!trade) {
      return Response.json({ error: "not_found" }, { status: 404 });
    }
    return Response.json({ trade });
  } catch (error) {
    return authErrorResponse(error);
  }
}
