import { redirect } from "next/navigation";
import { requireAdmin } from "@/lib/auth";
import { SignalFeed } from "@/components/signal-feed";

export default async function HomePage() {
  try {
    const { email } = await requireAdmin();
    return <SignalFeed adminEmail={email} />;
  } catch (error) {
    const status = typeof error === "object" && error !== null && "status" in error
      ? Number((error as { status?: unknown }).status)
      : 401;
    redirect(status === 403 ? "/login?error=forbidden" : "/login");
  }
}
