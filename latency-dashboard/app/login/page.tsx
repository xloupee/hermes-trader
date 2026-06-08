import { LoginForm } from "@/components/login-form";

export default async function LoginPage({ searchParams }: { searchParams: Promise<{ error?: string }> }) {
  const params = await searchParams;
  const message = params.error === "forbidden" ? "This account is not in latency_admin_users." : null;

  return (
    <main className="login-shell">
      <section className="login-panel">
        <p className="eyebrow">Copy Latency</p>
        <h1>Admin sign in</h1>
        <LoginForm initialMessage={message} />
      </section>
    </main>
  );
}
