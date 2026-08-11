"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { DASHBOARD_NAV } from "@/lib/dashboard-client";
import styles from "@/components/dashboard/dashboard-shell.module.css";

export function DashboardNav() {
  const pathname = usePathname();

  return (
    <nav className={styles.nav} aria-label="Dashboard navigation">
      {DASHBOARD_NAV.map((route) => {
        const active = pathname === route.href || pathname.startsWith(`${route.href}/`);
        return (
          <Link
            key={route.href}
            href={route.href}
            className={active ? styles.navActive : undefined}
            aria-current={active ? "page" : undefined}
          >
            {route.label}
          </Link>
        );
      })}
    </nav>
  );
}
