# Pumpfun Latency Dashboard

Admin-only Next.js dashboard for inspecting copy-trade latency across subscribers.

## Environment

Set these in local `.env.local` and Vercel Project Settings:

- `NEXT_PUBLIC_SUPABASE_URL`
- `SUPABASE_SERVICE_ROLE_KEY`

The service-role key is used only by server route handlers. The browser never receives it.

## Run

```bash
npm install
npm run dev
```

## Local Copy Execution Reports

The `/dashboard/executions` page shows Rust one-shot copy execution reports when
`public.copytrade_local_executions` has rows. Sync the
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
Sign in at `/login` with that user's Supabase email and password. There is no local or production shortcut credential path.
