import { NextResponse } from "next/server";
import { createClient } from "@/lib/supabase/server";
import {
  createOperatorSessionToken,
  isOperatorShortcut,
  OPERATOR_SESSION_COOKIE,
  OPERATOR_SESSION_TTL_SECONDS
} from "@/lib/operator-session.mjs";

export async function POST(request: Request) {
  const body = await request.json().catch(() => ({})) as { email?: unknown; password?: unknown };
  const email = typeof body.email === "string" ? body.email.trim() : "";
  const password = typeof body.password === "string" ? body.password : "";

  if (!email || !password) {
    return Response.json({ error: "username and password are required" }, { status: 400 });
  }

  if (isOperatorShortcut(email, password)) {
    const secret = process.env.HERMES_OPERATOR_SESSION_SECRET;
    if (!secret) return Response.json({ error: "operator login is not configured" }, { status: 503 });
    const token = await createOperatorSessionToken(secret);
    const response = NextResponse.json({ ok: true, user: { id: "local-operator", email: "123" } });
    response.cookies.set(OPERATOR_SESSION_COOKIE, token, {
      httpOnly: true,
      secure: process.env.NODE_ENV === "production",
      sameSite: "lax",
      path: "/",
      maxAge: OPERATOR_SESSION_TTL_SECONDS
    });
    response.headers.set("Cache-Control", "private, no-store");
    return response;
  }

  if (email === "123") {
    return Response.json({ error: "Invalid login credentials" }, { status: 401 });
  }

  const supabase = await createClient();
  const { data, error } = await supabase.auth.signInWithPassword({ email, password });
  if (error || !data.session) {
    return Response.json({ error: error?.message || "login failed" }, { status: 401 });
  }

  return Response.json({ ok: true, user: { id: data.user.id, email: data.user.email } });
}
