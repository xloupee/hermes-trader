import { cookies } from "next/headers";
import { createAdminClient } from "@/lib/supabase/admin";
import { FAST_LOGIN_TOKEN, isFastLoginCredentials, SESSION_COOKIE } from "@/lib/auth";

export async function POST(request: Request) {
  const body = await request.json().catch(() => ({})) as { email?: unknown; password?: unknown };
  const email = typeof body.email === "string" ? body.email.trim() : "";
  const password = typeof body.password === "string" ? body.password : "";

  if (!email || !password) {
    return Response.json({ error: "email and password are required" }, { status: 400 });
  }

  if (isFastLoginCredentials(email, password)) {
    const cookieStore = await cookies();
    cookieStore.set(SESSION_COOKIE, FAST_LOGIN_TOKEN, {
      httpOnly: true,
      secure: process.env.NODE_ENV === "production",
      sameSite: "lax",
      path: "/",
      maxAge: 60 * 60 * 8
    });

    return Response.json({ ok: true });
  }

  const supabase = createAdminClient();
  const { data, error } = await supabase.auth.signInWithPassword({ email, password });
  if (error || !data.session) {
    return Response.json({ error: error?.message || "login failed" }, { status: 401 });
  }

  const cookieStore = await cookies();
  cookieStore.set(SESSION_COOKIE, data.session.access_token, {
    httpOnly: true,
    secure: process.env.NODE_ENV === "production",
    sameSite: "lax",
    path: "/",
    maxAge: data.session.expires_in
  });

  return Response.json({ ok: true });
}
