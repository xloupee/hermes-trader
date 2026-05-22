#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCREEN_NAME="${SCREEN_NAME:-pumpfun-notifier}"

echo "Stopping local $SCREEN_NAME screen session if present..."
screen -S "$SCREEN_NAME" -X quit >/dev/null 2>&1 || true

echo "Stopping local bot processes from $ROOT_DIR if present..."
while IFS= read -r pid; do
  [ -n "$pid" ] || continue
  cwd="$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p' | head -n 1 || true)"

  if [ "$cwd" = "$ROOT_DIR" ]; then
    kill "$pid" >/dev/null 2>&1 || true
  fi
done < <(pgrep -f 'node dist/index\.js|npm start' || true)

echo "Local bot runner stopped."
