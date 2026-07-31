import { redirect } from "next/navigation";
import { requireAdmin } from "@/lib/auth";
import { DashboardShell } from "@/components/dashboard/dashboard-shell";

export default async function DashboardLayout({ children }: { children: React.ReactNode }) {
  try {
    const { email } = await requireAdmin();
    return <DashboardShell adminEmail={email}>{children}</DashboardShell>;
  } catch (error) {
    const status = typeof error === "object" && error !== null && "status" in error
      ? Number((error as { status?: unknown }).status)
      : 401;
    redirect(status === 403 ? "/login?error=forbidden" : "/login");
  }
}

