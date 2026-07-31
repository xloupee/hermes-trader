import assert from "node:assert/strict";
import { describe, test } from "node:test";
import fs from "node:fs/promises";
import path from "node:path";
import { dashboardRedirectPath } from "../auth-redirect.mjs";

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
  });
});
