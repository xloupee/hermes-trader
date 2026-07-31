import assert from "node:assert/strict";
import { describe, test } from "node:test";
import fs from "node:fs/promises";
import path from "node:path";

const AUTH_LIB = path.join(process.cwd(), "lib/auth.ts");

describe("auth hardening", () => {
  test("legacy bypass and forged-token paths are removed", async () => {
    const source = await fs.readFile(AUTH_LIB, "utf8");
    assert.equal(source.includes("LATENCY_FAST_LOGIN"), false);
    assert.equal(source.includes("latency_session"), false);
    assert.equal(source.includes("forged"), false);
  });

  test("default password auth fallback markers are not present", async () => {
    const source = await fs.readFile(AUTH_LIB, "utf8");
    assert.equal(/123\/123/.test(source), false);
    assert.equal(source.includes("SESSION_COOKIE"), false);
  });
});
