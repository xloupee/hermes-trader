import Link from "next/link";
import { LoginForm } from "@/components/login-form";

export default async function LoginPage({ searchParams }: { searchParams: Promise<{ error?: string }> }) {
  const params = await searchParams;
  const message = params.error === "forbidden" ? "This account is not in latency_admin_users." : null;

  return (
    <main className="login-shell">
      <section className="login-panel">
        <p className="eyebrow">Hermes Trader</p>
        <h1>Operator access</h1>
        <p>Use the operator credentials or an approved Supabase account to open the private dashboard.</p>
        <LoginForm initialMessage={message} />
        <Link href="/">Return to Hermes Trader</Link>
      </section>
    </main>
  );
}
