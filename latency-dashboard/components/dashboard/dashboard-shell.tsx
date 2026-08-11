import { DashboardNav } from "@/components/dashboard/dashboard-nav";
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
        <div className={styles.brandRow}>
          <div className={styles.brandMark} aria-hidden="true">H</div>
          <div className={styles.brand}>
            <p className={styles.eyebrow}>HERMES / OPS</p>
            <p className={styles.subtitle}>Solana execution intelligence</p>
          </div>
        </div>
        <DashboardNav />
        <div className={styles.userTools}>
          <span className={styles.liveState}><i aria-hidden="true" /> read-only public</span>
          <span className={styles.userPill}>{adminEmail || "observer"}</span>
        </div>
      </header>
      <main className={styles.mainContent}>{children}</main>
    </div>
  );
}
