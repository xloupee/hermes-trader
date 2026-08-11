import assert from "node:assert/strict";
import { describe, test } from "node:test";
import fs from "node:fs/promises";
import path from "node:path";
import { dashboardRedirectPath, protectedRequestKind } from "../auth-redirect.mjs";
import { resolveAdminAccess } from "../auth-contract.mjs";
import {
  createOperatorSessionToken,
  isOperatorShortcut,
  OPERATOR_SESSION_COOKIE,
  verifyOperatorSessionToken
} from "../operator-session.mjs";

const AUTH_LIB = path.join(process.cwd(), "lib/auth.ts");
const AUTH_CALLBACK = path.join(process.cwd(), "app/auth/callback/route.ts");
const AUTH_MIDDLEWARE = path.join(process.cwd(), "lib/supabase/middleware.ts");
const AUTH_LOGIN = path.join(process.cwd(), "app/api/auth/login/route.ts");

describe("auth hardening", () => {
  test("the requested 123 operator shortcut is exact", () => {
    assert.equal(isOperatorShortcut("123", "123"), true);
    assert.equal(isOperatorShortcut("123 ", "123"), false);
    assert.equal(isOperatorShortcut("123", "1234"), false);
    assert.equal(isOperatorShortcut("operator", "123"), false);
  });

  test("operator sessions are signed, expiring, and reject tampering", async () => {
    const secret = "test-only-operator-secret-with-enough-entropy";
    const now = Date.UTC(2026, 6, 31, 22, 0, 0);
    const token = await createOperatorSessionToken(secret, now);
    assert.equal(await verifyOperatorSessionToken(token, secret, now + 1_000), true);
    assert.equal(await verifyOperatorSessionToken(`${token.slice(0, -1)}x`, secret, now + 1_000), false);
    assert.equal(await verifyOperatorSessionToken(token, "wrong-secret", now + 1_000), false);
    assert.equal(await verifyOperatorSessionToken(token, secret, now + 8 * 24 * 60 * 60 * 1_000), false);
    assert.equal(OPERATOR_SESSION_COOKIE, "hermes_operator_session");
  });

  test("operator session remains server-only and HttpOnly", async () => {
    const [authSource, loginSource, middleware] = await Promise.all([
      fs.readFile(AUTH_LIB, "utf8"),
      fs.readFile(AUTH_LOGIN, "utf8"),
      fs.readFile(AUTH_MIDDLEWARE, "utf8")
    ]);
    assert.equal(authSource.includes("HERMES_OPERATOR_SESSION_SECRET"), true);
    assert.equal(loginSource.includes("httpOnly: true"), true);
    assert.equal(loginSource.includes('sameSite: "lax"'), true);
    assert.equal(loginSource.includes('secure: process.env.NODE_ENV === "production"'), true);
    assert.equal(middleware.includes("verifyOperatorSessionToken"), true);
    assert.equal(loginSource.includes('if (email === "123")'), true);
    assert.equal(authSource.includes("getSession("), false);
    assert.equal(authSource.includes("resolveAdminAccess"), true);
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

  test("middleware leaves the public dashboard surfaces open", () => {
    assert.equal(protectedRequestKind("/dashboard"), "none");
    assert.equal(protectedRequestKind("/dashboard/executions"), "none");
    assert.equal(protectedRequestKind("/api/dashboard/overview"), "none");
    assert.equal(protectedRequestKind("/api/me"), "dashboard_api");
    assert.equal(protectedRequestKind("/api/signals/summary"), "none");
  });
});
