const DEFAULT_DASHBOARD_DESTINATION = "/dashboard";

export function dashboardRedirectPath(value) {
  if (typeof value !== "string" || !value.startsWith("/") || value.startsWith("//") || value.includes("\\")) {
    return DEFAULT_DASHBOARD_DESTINATION;
  }

  try {
    const destination = new URL(value, "https://dashboard.invalid");
    const isDashboardPath = destination.pathname === "/dashboard" || destination.pathname.startsWith("/dashboard/");

    if (destination.origin !== "https://dashboard.invalid" || !isDashboardPath) {
      return DEFAULT_DASHBOARD_DESTINATION;
    }

    return `${destination.pathname}${destination.search}${destination.hash}`;
  } catch {
    return DEFAULT_DASHBOARD_DESTINATION;
  }
}
