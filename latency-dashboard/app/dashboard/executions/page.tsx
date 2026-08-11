import { Suspense } from "react";
import { ExecutionsDashboard } from "@/components/dashboard/executions-dashboard";

export default function DashboardExecutionsPage() {
  return <Suspense fallback={<p>Loading executions…</p>}><ExecutionsDashboard /></Suspense>;
}
