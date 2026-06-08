import type { User } from "@supabase/supabase-js";
import { cookies } from "next/headers";
import { createAdminClient } from "@/lib/supabase/admin";

type AdminUser = Pick<User, "id" | "email">;

export interface AdminSession {
  user: AdminUser;
  email: string | null;
}

export const SESSION_COOKIE = "latency_session";
export const FAST_LOGIN_EMAIL = "123";
export const FAST_LOGIN_PASSWORD = "123";
export const FAST_LOGIN_TOKEN = "latency_fast_login";

function fastLoginEnabled(): boolean {
  return process.env.LATENCY_FAST_LOGIN === "1" || process.env.NODE_ENV !== "production";
}

export function isFastLoginCredentials(email: string, password: string): boolean {
  return fastLoginEnabled() && email === FAST_LOGIN_EMAIL && password === FAST_LOGIN_PASSWORD;
}

function isFastLoginToken(token: string): boolean {
  return fastLoginEnabled() && token === FAST_LOGIN_TOKEN;
}

export async function currentUser(): Promise<AdminUser | null> {
  const token = (await cookies()).get(SESSION_COOKIE)?.value;
  if (!token) {
    return null;
  }

  if (isFastLoginToken(token)) {
    return {
      id: "fast-login-admin",
      email: FAST_LOGIN_EMAIL
    };
  }

  const supabase = createAdminClient();
  const { data, error } = await supabase.auth.getUser(token);
  if (error || !data.user) {
    return null;
  }
  return data.user;
}

export async function requireAdmin(): Promise<AdminSession> {
  const user = await currentUser();
  if (!user) {
    throw Object.assign(new Error("unauthenticated"), { status: 401 });
  }

  if (user.id === "fast-login-admin") {
    return { user, email: user.email ?? null };
  }

  const admin = createAdminClient();
  const byUserId = await admin
    .from("latency_admin_users")
    .select("id,email,auth_user_id")
    .eq("auth_user_id", user.id)
    .maybeSingle();

  if (byUserId.data) {
    return { user, email: user.email ?? byUserId.data.email ?? null };
  }

  if (user.email) {
    const byEmail = await admin
      .from("latency_admin_users")
      .select("id,email,auth_user_id")
      .eq("email", user.email)
      .maybeSingle();

    if (byEmail.data) {
      return { user, email: user.email };
    }
  }

  throw Object.assign(new Error("forbidden"), { status: 403 });
}

export function authErrorResponse(error: unknown) {
  const status = typeof error === "object" && error !== null && "status" in error
    ? Number((error as { status?: unknown }).status)
    : 500;
  return Response.json(
    { error: status === 401 ? "unauthenticated" : status === 403 ? "forbidden" : "server_error" },
    { status: Number.isFinite(status) ? status : 500 }
  );
}
