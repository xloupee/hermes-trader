interface VerifiedUser {
  id: string;
}

interface AllowlistedAdminUser {
  auth_user_id: string;
  email: string | null;
}

interface AdminUser {
  id: string;
  email: string | null;
}

type AdminAccessResult =
  | { state: "unauthenticated"; status: 401; user: null }
  | { state: "forbidden"; status: 403; user: null }
  | { state: "authorized"; status: 200; user: AdminUser };

export function resolveAdminAccess(dependencies: {
  getVerifiedUser: () => Promise<{ user: VerifiedUser | null; error: unknown }>;
  findAdminUser: (authUserId: string) => Promise<{ adminUser: AllowlistedAdminUser | null; error: unknown }>;
}): Promise<AdminAccessResult>;

export function authFailure(status: number): Error & { status: number };
