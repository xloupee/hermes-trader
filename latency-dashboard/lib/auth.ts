import { createAdminClient } from "@/lib/supabase/admin";
import { createClient } from "@/lib/supabase/server";

interface AdminUser {
  id: string;
  email: string | null;
}

export interface AdminSession {
  user: AdminUser;
  email: string | null;
}

export async function currentUser(): Promise<AdminUser | null> {
  const supabase = await createClient();
  const { data, error } = await supabase.auth.getUser();
  if (error || !data.user) {
    return null;
  }

  const admin = createAdminClient();
  const { data: adminUser, error: adminError } = await admin
    .from("latency_admin_users")
    .select("id,email,auth_user_id")
    .eq("auth_user_id", data.user.id)
    .maybeSingle();

  if (adminError || !adminUser) {
    return null;
  }

  return {
    id: data.user.id,
    email: adminUser.email ?? data.user.email ?? null
  };
}

export async function requireAdmin(): Promise<AdminSession> {
  const user = await currentUser();
  if (!user) {
    throw Object.assign(new Error("unauthenticated"), { status: 401 });
  }

  return { user, email: user.email };
}

export async function logoutSession(): Promise<void> {
  const supabase = await createClient();
  await supabase.auth.signOut();
}

export function authErrorResponse(error: unknown) {
  const status =
    typeof error === "object" && error !== null && "status" in error
      ? Number((error as { status?: unknown }).status)
      : 500;
  return Response.json(
    { error: status === 401 ? "unauthenticated" : status === 403 ? "forbidden" : "server_error" },
    { status: Number.isFinite(status) ? status : 500 }
  );
}
