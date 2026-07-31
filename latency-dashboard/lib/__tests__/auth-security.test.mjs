import assert from "node:assert/strict";
import { describe, test } from "node:test";
import fs from "node:fs/promises";
import path from "node:path";
import { dashboardRedirectPath, protectedRequestKind } from "../auth-redirect.mjs";
import { resolveAdminAccess } from "../auth-contract.mjs";

const AUTH_LIB = path.join(process.cwd(), "lib/auth.ts");
const AUTH_CALLBACK = path.join(process.cwd(), "app/auth/callback/route.ts");
const AUTH_MIDDLEWARE = path.join(process.cwd(), "lib/supabase/middleware.ts");

describe("auth hardening", () => {
  test("legacy bypass and forged-token paths are removed", async () => {
    const source = await fs.readFile(AUTH_LIB, "utf8");
    assert.equal(source.includes(["LATENCY", "FAST", "LOGIN"].join("_")), false);
    assert.equal(source.includes(["latency", "session"].join("_")), false);
    assert.equal(source.includes("forged"), false);
  });

  test("default password auth fallback markers are not present", async () => {
    const source = await fs.readFile(AUTH_LIB, "utf8");
    assert.equal(/123\/123/.test(source), false);
    assert.equal(source.includes("SESSION_COOKIE"), false);
    assert.equal(source.includes("getSession("), false);
    assert.equal(source.includes("data.user.email"), false);
    assert.equal(source.includes("resolveAdminAccess"), true);
    assert.equal(source.includes("authFailure(access.status)"), true);
  });

  test("anonymous requests resolve to 401", async () => {
    const result = await resolveAdminAccess({
      getVerifiedUser: async () => ({ user: null, error: null }),
      findAdminUser: async () => assert.fail("anonymous requests must not query the allowlist")
    });
    assert.deepEqual(result, { state: "unauthenticated", status: 401, user: null });
  });

  test("forged or invalid users resolve to 401", async () => {
    const result = await resolveAdminAccess({
      getVerifiedUser: async () => ({ user: { id: "forged-user" }, error: new Error("invalid JWT") }),
      findAdminUser: async () => assert.fail("invalid users must not query the allowlist")
    });
    assert.deepEqual(result, { state: "unauthenticated", status: 401, user: null });
  });

  test("verified but unlisted users resolve to 403", async () => {
    const result = await resolveAdminAccess({
      getVerifiedUser: async () => ({ user: { id: "verified-user" }, error: null }),
      findAdminUser: async (authUserId) => {
        assert.equal(authUserId, "verified-user");
        return { adminUser: null, error: null };
      }
    });
    assert.deepEqual(result, { state: "forbidden", status: 403, user: null });
  });

  test("listed users succeed with allowlist identity only", async () => {
    const result = await resolveAdminAccess({
      getVerifiedUser: async () => ({ user: { id: "listed-user", email: "untrusted@example.com" }, error: null }),
      findAdminUser: async () => ({
        adminUser: { auth_user_id: "listed-user", email: "operator@example.com" },
        error: null
      })
    });
    assert.deepEqual(result, {
      state: "authorized",
      status: 200,
      user: { id: "listed-user", email: "operator@example.com" }
    });
  });

  test("auth callback only permits relative dashboard destinations", () => {
    assert.equal(dashboardRedirectPath(null), "/dashboard");
    assert.equal(dashboardRedirectPath("/dashboard"), "/dashboard");
    assert.equal(dashboardRedirectPath("/dashboard/executions?source=erpc#latest"), "/dashboard/executions?source=erpc#latest");
    assert.equal(dashboardRedirectPath("https://attacker.invalid/dashboard"), "/dashboard");
    assert.equal(dashboardRedirectPath("//attacker.invalid/dashboard"), "/dashboard");
    assert.equal(dashboardRedirectPath("/dashboardish"), "/dashboard");
    assert.equal(dashboardRedirectPath("/"), "/dashboard");
  });

  test("callback exchange and middleware preserve refreshed SSR cookies", async () => {
    const [callback, middleware] = await Promise.all([
      fs.readFile(AUTH_CALLBACK, "utf8"),
      fs.readFile(AUTH_MIDDLEWARE, "utf8")
    ]);

    assert.equal(callback.includes("exchangeCodeForSession(code)"), true);
    assert.equal(callback.includes("dashboardRedirectPath"), true);
    assert.equal(middleware.includes("await supabase.auth.getUser()"), true);
    assert.match(middleware, /request\.cookies\.set\(name, value\)/);
    assert.match(middleware, /response\.cookies\.set\(name, value, options\)/);
    assert.match(middleware, /Cache-Control.*private, no-store/);
    assert.match(middleware, /NextResponse\.json\(\{ error: "unauthenticated" \}, \{ status: 401 \}\)/);
    assert.match(middleware, /NextResponse\.redirect/);
  });

  test("middleware protection covers dashboard pages and APIs", () => {
    assert.equal(protectedRequestKind("/dashboard"), "dashboard_page");
    assert.equal(protectedRequestKind("/dashboard/executions"), "dashboard_page");
    assert.equal(protectedRequestKind("/api/dashboard/overview"), "dashboard_api");
    assert.equal(protectedRequestKind("/api/me"), "dashboard_api");
    assert.equal(protectedRequestKind("/api/signals/summary"), "none");
  });
});
