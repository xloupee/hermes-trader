import { authErrorResponse } from "@/lib/auth";
import { attachTelegramSubscriberIds, toDashboardExecution } from "@/lib/dashboard-contract.mjs";
import { getLocalExecution } from "@/lib/local-executions";
import { listTelegramSubscribersByCopyWallet } from "@/lib/telegram-subscribers";

export async function GET(_request: Request, { params }: { params: Promise<{ id: string }> }) {
  try {
    const { id: idParam } = await params;
    const id = Number(idParam);
    if (!Number.isFinite(id)) {
      return Response.json({ error: "invalid id" }, { status: 400 });
    }

    const execution = await getLocalExecution(Math.trunc(id));
    if (!execution) {
      return Response.json({ error: "execution not found" }, { status: 404 });
    }

    const subscriberByCopyWallet = await listTelegramSubscribersByCopyWallet([execution]);
    const [sanitized] = attachTelegramSubscriberIds([toDashboardExecution(execution)], subscriberByCopyWallet);
    return Response.json({ execution: sanitized });
  } catch (error) {
    return authErrorResponse(error);
  }
}
