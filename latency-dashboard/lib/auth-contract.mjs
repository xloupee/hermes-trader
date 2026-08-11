function statusResult(state, status) {
  return { state, status, user: null };
}

export async function resolveAdminAccess({ getVerifiedUser, findAdminUser }) {
  const { user, error: authError } = await getVerifiedUser();
  if (authError || !user || typeof user.id !== "string" || !user.id) {
    return statusResult("unauthenticated", 401);
  }

  const { adminUser, error: adminError } = await findAdminUser(user.id);
  if (adminError) {
    throw adminError;
  }

  if (!adminUser || adminUser.auth_user_id !== user.id) {
    return statusResult("forbidden", 403);
  }

  return {
    state: "authorized",
    status: 200,
    user: {
      id: user.id,
      email: typeof adminUser.email === "string" ? adminUser.email : null
    }
  };
}

export function authFailure(status) {
  if (status === 401) {
    return Object.assign(new Error("unauthenticated"), { status: 401 });
  }
  if (status === 403) {
    return Object.assign(new Error("forbidden"), { status: 403 });
  }
  return Object.assign(new Error("server_error"), { status: 500 });
}
