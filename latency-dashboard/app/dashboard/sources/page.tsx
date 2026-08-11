import { Suspense } from "react";
import { SourcesDashboard } from "@/components/dashboard/sources-dashboard";

export default function DashboardSourcesPage() {
  return <Suspense fallback={<p>Loading sources…</p>}><SourcesDashboard /></Suspense>;
}
