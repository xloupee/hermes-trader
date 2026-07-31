import { createAdminClient } from "@/lib/supabase/admin";
import { createClient } from "@/lib/supabase/server";
import { authFailure, resolveAdminAccess } from "@/lib/auth-contract.mjs";

interface AdminUser {
  id: string;
  email: string | null;
}

export interface AdminSession {
  user: AdminUser;
  email: string | null;
}

export async function requireAdmin(): Promise<AdminSession> {
  const supabase = await createClient();
  const access = await resolveAdminAccess({
    getVerifiedUser: async () => {
      const { data, error } = await supabase.auth.getUser();
      return { user: data.user ? { id: data.user.id } : null, error };
    },
    findAdminUser: async (authUserId) => {
      const admin = createAdminClient();
      const { data, error } = await admin
        .from("latency_admin_users")
        .select("email,auth_user_id")
        .eq("auth_user_id", authUserId)
        .maybeSingle();
      return { adminUser: data, error };
    }
  });

  if (access.state !== "authorized") {
    throw authFailure(access.status);
  }

  const user: AdminUser = access.user;
  return { user, email: user.email };
}

export async function logoutSession(): Promise<void> {
  const supabase = await createClient();
  await supabase.auth.signOut();
}

export function authErrorResponse(error: unknown) {
  const candidateStatus =
    typeof error === "object" && error !== null && "status" in error
      ? Number((error as { status?: unknown }).status)
      : 500;
  const status = candidateStatus === 400 || candidateStatus === 401 || candidateStatus === 403 ? candidateStatus : 500;
  return Response.json(
    { error: status === 400 ? "invalid_request" : status === 401 ? "unauthenticated" : status === 403 ? "forbidden" : "server_error" },
    { status }
  );
}
