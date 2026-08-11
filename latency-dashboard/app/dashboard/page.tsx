import { Suspense } from "react";
import { OverviewDashboard } from "@/components/dashboard/overview-dashboard";

export default function DashboardRoute() {
  return <Suspense fallback={<p>Loading dashboard…</p>}><OverviewDashboard /></Suspense>;
}
