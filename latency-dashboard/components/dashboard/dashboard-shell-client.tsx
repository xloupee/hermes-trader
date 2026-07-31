"use client";

import { LogOut } from "lucide-react";

export function DashboardshellClientSignOut() {
  async function signOut() {
    await fetch("/api/auth/logout", { method: "POST" });
    window.location.assign("/login");
  }

  return (
    <button
      className="admin-button"
      onClick={signOut}
      type="button"
      title="Sign out"
      aria-label="Sign out"
    >
      <LogOut size={15} />
      Sign out
    </button>
  );
}

