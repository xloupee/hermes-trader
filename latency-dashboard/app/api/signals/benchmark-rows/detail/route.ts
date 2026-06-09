import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { getBenchmarkRowDetail } from "@/lib/benchmark-rows";

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const searchParams = new URL(request.url).searchParams;
    const rowId = searchParams.get("rowId") || "";
    const signalIdValue = Number(searchParams.get("signalId"));
    const signalId = Number.isInteger(signalIdValue) && signalIdValue > 0 ? signalIdValue : null;
    const row = await getBenchmarkRowDetail(rowId, signalId);
    if (!row) {
      return Response.json({ error: "Benchmark row not found" }, { status: 404 });
    }
    return Response.json({ row });
  } catch (error) {
    return authErrorResponse(error);
  }
}
