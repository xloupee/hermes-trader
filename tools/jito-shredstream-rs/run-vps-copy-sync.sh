#!/usr/bin/env bash
set -euo pipefail

APP_ENV_FILE="${JITO_APP_ENV_FILE:-/opt/pumpfun-migration-bot/.env}"
WORKER_ENV_FILE="${JITO_WORKER_ENV_FILE:-/etc/jito-copy-live.env}"
WORKER_DIR="${JITO_WORKER_DIR:-/opt/jito-feed-probe-watch}"

load_env_file() {
  local env_file="$1"
  local line key value

  while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    [[ -z "$line" || "${line:0:1}" == "#" || "$line" != *"="* ]] && continue

    key="${line%%=*}"
    value="${line#*=}"
    [[ "$key" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || continue

    export "$key=$value"
  done < "$env_file"
}

if [[ -f "$APP_ENV_FILE" ]]; then
  load_env_file "$APP_ENV_FILE"
fi

if [[ -f "$WORKER_ENV_FILE" ]]; then
  load_env_file "$WORKER_ENV_FILE"
fi

: "${SOLANA_RPC_URL:?SOLANA_RPC_URL must be set in $APP_ENV_FILE or $WORKER_ENV_FILE}"
: "${SUPABASE_URL:?SUPABASE_URL must be set in $APP_ENV_FILE or $WORKER_ENV_FILE}"
: "${SUPABASE_SERVICE_ROLE_KEY:?SUPABASE_SERVICE_ROLE_KEY must be set in $APP_ENV_FILE or $WORKER_ENV_FILE}"

export JITO_COPY_EXECUTIONS_PATH="${JITO_COPY_EXECUTIONS_PATH:-/var/log/jito-copy-executions-vps.jsonl}"
export JITO_SYNC_RECENT_LIMIT="${JITO_SYNC_RECENT_LIMIT:-100}"
export JITO_SYNC_REFRESH_SENT_ROWS="${JITO_SYNC_REFRESH_SENT_ROWS:-true}"
export JITO_SYNC_REFRESH_INTERVAL_MS="${JITO_SYNC_REFRESH_INTERVAL_MS:-5000}"
export JITO_SYNC_REFRESH_PENDING_LIMIT="${JITO_SYNC_REFRESH_PENDING_LIMIT:-25}"
export JITO_SYNC_BLOCK_POSITION_RETRY_ATTEMPTS="${JITO_SYNC_BLOCK_POSITION_RETRY_ATTEMPTS:-3}"
export JITO_SYNC_BLOCK_POSITION_RETRY_MS="${JITO_SYNC_BLOCK_POSITION_RETRY_MS:-500}"
export JITO_SUPABASE_CWD="${JITO_SUPABASE_CWD:-/opt/pumpfun-migration-bot}"

cd "$WORKER_DIR"
exec /usr/bin/node "$WORKER_DIR/sync-local-copy-executions-to-supabase.mjs" \
  --watch \
  --executions="$JITO_COPY_EXECUTIONS_PATH" \
  --interval-ms="${JITO_SYNC_INTERVAL_MS:-1000}" \
  --refresh-interval-ms="$JITO_SYNC_REFRESH_INTERVAL_MS" \
  --recent-limit="$JITO_SYNC_RECENT_LIMIT" \
  --refresh-pending-limit="$JITO_SYNC_REFRESH_PENDING_LIMIT" \
  --block-position-retry-attempts="$JITO_SYNC_BLOCK_POSITION_RETRY_ATTEMPTS" \
  --block-position-retry-ms="$JITO_SYNC_BLOCK_POSITION_RETRY_MS"
