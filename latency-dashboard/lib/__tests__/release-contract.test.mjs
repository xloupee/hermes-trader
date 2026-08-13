import assert from "node:assert/strict";
import { describe, test } from "node:test";
import fs from "node:fs/promises";
import path from "node:path";

const dashboardRoot = process.cwd();
const repositoryRoot = path.resolve(dashboardRoot, "..");

async function read(relativePath) {
  return fs.readFile(path.join(repositoryRoot, relativePath), "utf8");
}

describe("release contract", () => {
  test("dashboard and CI enforce Node 24 and run dashboard tests directly", async () => {
    const packageJson = JSON.parse(await read("latency-dashboard/package.json"));
    const workflow = await read(".github/workflows/deploy-production.yml");

    assert.equal(packageJson.engines.node, "24.x");
    assert.match(workflow, /node-version: "24"/);
    assert.match(workflow, /working-directory: latency-dashboard[\s\S]*?run: npm test/);
    assert.doesNotMatch(workflow, /npm test --if-present/);
  });

  test("deployment and credential documentation preserve release boundaries", async () => {
    const [readme, runbook, disposition] = await Promise.all([
      read("latency-dashboard/README.md"),
      read("latency-dashboard/VERCEL_RUNBOOK.md"),
      read("SECURITY/DEPENDENCY_DISPOSITION.md")
    ]);

    assert.match(readme, /NEXT_PUBLIC_SUPABASE_ANON_KEY/);
    assert.match(readme, /SUPABASE_SERVICE_ROLE_KEY.*server-only/s);
    assert.match(runbook, /Vercel project root: `latency-dashboard`/);
    assert.match(runbook, /Node\.js runtime and build version: `24\.x`/);
    assert.match(runbook, /Exact-artifact promotion/);
    assert.match(runbook, /Retain the prior production deployment/);
    assert.match(runbook, /must not revert or mutate database schema or rows/);
    assert.match(disposition, /## Vercel dashboard artifact/);
    assert.match(disposition, /postcss@8\.5\.18/);
    assert.match(disposition, /sharp@0\.35\.0/);
    assert.match(disposition, /## Root Solana service artifact/);
    assert.match(disposition, /Follow-up release gate/g);
  });

  test("CI renders actual checks and workflow jobs instead of synthetic planned builds", async () => {
    const dashboard = await read("latency-dashboard/components/ci-ledger-dashboard.tsx");

    assert.match(dashboard, /function checkBuilds\(pr: CiPullRequest\)/);
    assert.match(dashboard, /function workflowBuilds\(pr: CiPullRequest, run: CiRun \| null\)/);
    assert.match(dashboard, /<span className=\{styles\.eyebrow\}>Builds<\/span>/);
    assert.doesNotMatch(dashboard, /plannedPhases|Expected build path|expected stages|Awaiting marker/);
  });
});
