"use client";

import { useState } from "react";
import { ArrowRight } from "lucide-react";

export function LoginForm({ initialMessage = null }: { initialMessage?: string | null }) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [message, setMessage] = useState<string | null>(initialMessage);
  const [busy, setBusy] = useState(false);

  async function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setMessage(null);

    const response = await fetch("/api/auth/login", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ email, password })
    });
    const result = await response.json().catch(() => ({})) as { error?: string };

    if (!response.ok) {
      setMessage(result.error || "Could not sign in");
      setBusy(false);
      return;
    }

    window.location.assign("/dashboard");
  }

  return (
    <form className="login-form" noValidate onSubmit={submit}>
      <label>
        Email
        <input
          value={email}
          onChange={(event) => setEmail(event.target.value)}
          type="text"
          autoComplete="username"
          required
        />
      </label>
      <label>
        Password
        <input
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          type="password"
          autoComplete="current-password"
          required
        />
      </label>
      <button className="primary-button" disabled={busy} type="submit">
        <ArrowRight size={16} />
        {busy ? "Signing in" : "Sign in"}
      </button>
      {message ? <p className="form-message">{message}</p> : null}
    </form>
  );
}
