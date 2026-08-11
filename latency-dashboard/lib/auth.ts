import { cookies } from "next/headers";
import { createAdminClient } from "@/lib/supabase/admin";
import { createClient } from "@/lib/supabase/server";
import { authFailure, resolveAdminAccess } from "@/lib/auth-contract.mjs";
import { OPERATOR_SESSION_COOKIE, verifyOperatorSessionToken } from "@/lib/operator-session.mjs";

interface AdminUser {
  id: string;
  email: string | null;
}

export interface AdminSession {
  user: AdminUser;
  email: string | null;
}

export async function requireAdmin(): Promise<AdminSession> {
  const cookieStore = await cookies();
  const operatorToken = cookieStore.get(OPERATOR_SESSION_COOKIE)?.value;
  if (await verifyOperatorSessionToken(operatorToken, process.env.HERMES_OPERATOR_SESSION_SECRET)) {
    return { user: { id: "local-operator", email: "123" }, email: "123" };
  }

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
