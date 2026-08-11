import { ExecutionDetailDashboard } from "@/components/dashboard/execution-detail-dashboard";

export default async function DashboardExecutionDetailPage(
  { params }: { params: Promise<{ id: string }> }
) {
  const resolved = await params;
  return <ExecutionDetailDashboard id={resolved.id} />;
}
