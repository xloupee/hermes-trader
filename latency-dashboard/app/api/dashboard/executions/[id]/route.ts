import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { toDashboardExecution } from "@/lib/dashboard-contract.mjs";
import { getLocalExecution } from "@/lib/local-executions";

export async function GET(_request: Request, { params }: { params: Promise<{ id: string }> }) {
  try {
    await requireAdmin();
    const { id: idParam } = await params;
    const id = Number(idParam);
    if (!Number.isFinite(id)) {
      return Response.json({ error: "invalid id" }, { status: 400 });
    }

    const execution = await getLocalExecution(Math.trunc(id));
    if (!execution) {
      return Response.json({ error: "execution not found" }, { status: 404 });
    }

    const sanitized = toDashboardExecution(execution);
    return Response.json({ execution: sanitized });
  } catch (error) {
    return authErrorResponse(error);
  }
}
