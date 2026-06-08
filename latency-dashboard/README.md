# Pumpfun Latency Dashboard

Admin-only Next.js dashboard for inspecting copy-trade latency across subscribers.

## Environment

Set these in local `.env.local` and Vercel Project Settings:

- `NEXT_PUBLIC_SUPABASE_URL`
- `SUPABASE_SERVICE_ROLE_KEY`
- `LATENCY_FAST_LOGIN` - set to `1` to enable the shortcut login in production. It is enabled automatically during local development.

The service-role key is used only by server route handlers. The browser never receives it.

## Run

```bash
npm install
npm run dev
```

## Local Copy Execution Reports

The `/signals` page can show Rust one-shot copy execution reports next to each
observed signal when `public.copytrade_local_executions` has rows. Sync the
local JSONL send log after a test run:

```bash
NEXT_PUBLIC_SUPABASE_URL=... \
SUPABASE_SERVICE_ROLE_KEY=... \
SOLANA_RPC_URL=... \
npm run sync:copy-executions -- --executions=/tmp/jito-copy-executions-local-send.jsonl
```

The sync computes confirmed slot delta, token fill, gross SOL spend, network
fee, and extra spend beyond the observed buy amount.

## Admin Access

Add an authenticated Supabase user to `public.latency_admin_users` by email and, once known, `auth_user_id`.

For local development, sign in with email `123` and password `123` to skip Supabase auth. The same shortcut only works in production if `LATENCY_FAST_LOGIN=1` is set.
