import Link from "next/link";
import { DashboardshellClientSignOut } from "@/components/dashboard/dashboard-shell-client";
import { DASHBOARD_NAV } from "@/lib/dashboard-client";
import styles from "@/components/dashboard/dashboard-shell.module.css";

export function DashboardShell({
  adminEmail,
  children
}: {
  adminEmail: string | null;
  children: React.ReactNode;
}) {
  return (
    <div className={styles.dashboardShell}>
      <header className={styles.topBar}>
        <div className={styles.brand}>
          <p className={styles.eyebrow}>Operator Desk</p>
          <h1 className={styles.title}>Hermes Dashboard</h1>
          <p className={styles.subtitle}>Latency, execution, and source observability</p>
        </div>
        <div className={styles.userTools}>
          <span className={styles.userPill}>admin: {adminEmail || "unknown"}</span>
          <DashboardshellClientSignOut />
        </div>
      </header>
      <nav className={styles.nav} aria-label="Dashboard navigation">
        {DASHBOARD_NAV.map((route) => (
          <Link key={route.href} href={route.href}>
            {route.label}
          </Link>
        ))}
      </nav>
      <main className={styles.mainContent}>{children}</main>
    </div>
  );
}
