import { DashboardShell } from "@/components/dashboard/dashboard-shell";

export default async function DashboardLayout({ children }: { children: React.ReactNode }) {
  return <DashboardShell adminEmail={null}>{children}</DashboardShell>;
}
