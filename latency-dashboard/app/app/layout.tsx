import type { Metadata } from "next";
import { CustomerAppShell } from "@/components/customer-app/customer-app-shell";

export const metadata: Metadata = {
  title: "Hermes Mission Control",
  description: "Interactive customer configuration prototype using local demo data"
};

export default function CustomerLayout({ children }: { children: React.ReactNode }) {
  return <CustomerAppShell>{children}</CustomerAppShell>;
}
