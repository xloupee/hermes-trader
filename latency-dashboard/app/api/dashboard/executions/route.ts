import { authErrorResponse } from "@/lib/auth";
import { attachTelegramSubscriberIds, encodeExecutionCursor, pageExecutionRows, parseExecutionFilters, summarizeExecutions, toDashboardExecution } from "@/lib/dashboard-contract.mjs";
import { getDashboardExecutionFreshness, listDashboardExecutions } from "@/lib/local-executions";
import { getGatewayConfirmationFreshness, listGatewayConfirmations } from "@/lib/gateway-confirmations";
import { enrichExecutionsWithLeaderDiagnostics } from "@/lib/leader-diagnostics";
import { listTelegramSubscribersByCopyWallet } from "@/lib/telegram-subscribers";

export async function GET(request: Request) {
  try {
    const filters = parseExecutionFilters(new URL(request.url).searchParams);
    const [fetchedRows, freshness, gatewayConfirmations, gatewayConfirmationFreshness] = await Promise.all([
      listDashboardExecutions(filters),
      getDashboardExecutionFreshness(),
      listGatewayConfirmations(filters),
      getGatewayConfirmationFreshness()
    ]);
    const page = pageExecutionRows(fetchedRows, filters.limit);
    const sanitizedRows = page.items.map(toDashboardExecution);
    const [rowsWithLeaders, subscriberByCopyWallet] = await Promise.all([
      enrichExecutionsWithLeaderDiagnostics(sanitizedRows),
      listTelegramSubscribersByCopyWallet(sanitizedRows)
    ]);
    const executions = attachTelegramSubscriberIds(rowsWithLeaders, subscriberByCopyWallet);
    const lastExecution = executions[executions.length - 1];
    const nextCursor = lastExecution ? encodeExecutionCursor(lastExecution) : null;

    return Response.json({
      executions,
      gatewayConfirmations,
      summary: summarizeExecutions(executions),
      freshness,
      gatewayConfirmationFreshness,
      pagination: {
        limit: filters.limit,
        hasMore: page.hasMore,
        nextCursor
      },
      filters
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}
