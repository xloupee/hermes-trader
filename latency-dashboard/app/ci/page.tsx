import type { Metadata } from "next";
import { CILedgerDashboard } from "@/components/ci-ledger-dashboard";

export const metadata: Metadata = {
  title: "Hermes CI",
  description: "VPS build ledger and detailed CI evidence"
};

export default function CIBuildsPage() {
  return <CILedgerDashboard />;
}
