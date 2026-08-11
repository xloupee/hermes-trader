# Pumpfun Latency Dashboard

Hermes Next.js dashboard for CI build visibility and read-only execution
intelligence.

## Environment

Set these in local `.env.local` and Vercel Project Settings:

- `NEXT_PUBLIC_SUPABASE_URL`
- `NEXT_PUBLIC_SUPABASE_ANON_KEY`
- `SUPABASE_SERVICE_ROLE_KEY`
- `LATENCY_FAST_LOGIN` - set to `1` to enable the shortcut login in production. It is enabled automatically during local development.
- `GITHUB_TOKEN` - server-only token with read access to the CI source repository.
- `CI_REPOSITORY` - explicit repository queried by `/ci`, currently `xloupee/pumpfun-migration-bot`.
- `CI_ALLOWED_REPOSITORIES` - comma-separated allowlist; keep it aligned with `CI_REPOSITORY`.

The service-role key is used only by server route handlers. The browser never
receives it. The GitHub token is also server-only. If GitHub is unavailable,
`/ci` fails closed and does not render historical fixture data.

The `/ci` page and its read-only CI feed routes, plus the temporary public
`/dashboard` execution surface and read-only dashboard data routes, are
public for the Vercel dashboard. Other admin routes remain protected. Re-enable
the relevant auth before sharing the deployment beyond the intended audience.

See [VERCEL_RUNBOOK.md](./VERCEL_RUNBOOK.md) for preview, exact-artifact
promotion, and frontend-only rollback policy.

## Run

```bash
npm install
npm run dev
```

## Local Copy Execution Reports

The `/dashboard/executions` page shows Rust one-shot copy execution reports when
`public.copytrade_local_executions` has rows. Sync the local JSONL send log
after a test run:

```bash
NEXT_PUBLIC_SUPABASE_URL=... \
SUPABASE_SERVICE_ROLE_KEY=... \
SOLANA_RPC_URL=... \
npm run sync:copy-executions -- --executions=/tmp/jito-copy-executions-local-send.jsonl
```

The sync computes confirmed slot delta, token fill, gross SOL spend, network
fee, and extra spend beyond the observed buy amount.

## Admin Access

Add an authenticated Supabase user to `public.latency_admin_users` by email
and, once known, `auth_user_id`. Sign in at `/login` with that user's
Supabase email and password, or use the private operator shortcut. The shortcut
issues a signed, HTTP-only session cookie using
`HERMES_OPERATOR_SESSION_SECRET`.
