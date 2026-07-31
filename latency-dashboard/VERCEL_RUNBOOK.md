# Latency Dashboard Vercel Runbook

This runbook covers only the `latency-dashboard` frontend artifact. It does not
authorize a backend deployment, database or Supabase mutation, host access,
service restart, credential change, or infrastructure rollback.

## Project contract

- Vercel project root: `latency-dashboard`
- Node.js runtime and build version: `24.x`
- Install command: `npm ci`
- Test command: `npm test`
- Typecheck command: `npm run check`
- Build command: `npm run build`
- Required public configuration: `NEXT_PUBLIC_SUPABASE_URL` and
  `NEXT_PUBLIC_SUPABASE_ANON_KEY`
- Required server-only configuration: `SUPABASE_SERVICE_ROLE_KEY`

Configure the Vercel project to use Node 24 and the project root above. Reject a
preview if its source commit, project root, runtime, or environment scope does
not match this contract.

## Preview procedure

1. Select the exact reviewed Git commit. Record its full SHA before creating a
   preview.
2. Confirm CI passed `npm ci`, `npm test`, `npm run check`, `npm run build`, the
   tracked-tree absence gate, and bundle secret scans for that SHA.
3. Create a Vercel preview from that exact commit with `latency-dashboard` as
   the project root and Node 24. Do not deploy from an uncommitted checkout.
4. Record the preview deployment ID or immutable URL, source SHA, build log,
   creation time, and tester.
5. Verify `/`, `/login`, `/dashboard`, `/dashboard/executions`, an execution
   detail route, `/dashboard/sources`, `/dashboard/system`, and the `/signals`
   redirect. Authenticated checks require an approved Supabase account.
6. Confirm client assets and source maps contain no server-only credential or
   private-key material.

Preview creation and verification do not authorize production promotion.

## Exact-artifact promotion

Promote the already-verified immutable preview deployment. Do not trigger a new
build from the branch tip, local files, or a different commit. Before changing
the production alias, record:

- candidate deployment ID or immutable URL and full source SHA;
- current production deployment ID or immutable URL;
- CI and preview-verification evidence;
- approving operator and promotion time.

Retain the prior production deployment in Vercel. Do not delete it, overwrite
its evidence, or replace it with a rebuild during the rollback window.

## Frontend rollback

Rollback means moving the production alias to the retained prior Vercel
deployment artifact. Verify the prior deployment ID and source SHA before the
alias change, then record the new alias state and smoke-test the public and
authenticated routes.

A dashboard rollback must not revert or mutate database schema or rows,
Supabase Auth users or configuration, backend services, AX configuration,
credentials, secrets, wallets, or network policy. If the retained prior
artifact is unavailable or incompatible with the current API contract, stop and
escalate instead of rebuilding or changing backend state.
