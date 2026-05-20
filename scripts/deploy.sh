#!/usr/bin/env bash
set -euo pipefail

VPS_HOST="${VPS_HOST:-root@157.90.240.233}"
APP_DIR="${APP_DIR:-/opt/pumpfun-migration-bot}"
SERVICE_NAME="${SERVICE_NAME:-pumpfun-migration-bot}"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$ROOT_DIR"

echo "Checking source..."
npm run check

echo "Deploying to $VPS_HOST:$APP_DIR..."
ssh "$VPS_HOST" "mkdir -p '$APP_DIR'"

COPYFILE_DISABLE=1 tar \
  --no-xattrs \
  --exclude='.git' \
  --exclude='node_modules' \
  --exclude='logs' \
  --exclude='data' \
  --exclude='.DS_Store' \
  -czf - . |
  ssh "$VPS_HOST" "
    set -euo pipefail
    TMP_DIR=\$(mktemp -d)
    tar -xzf - -C \"\$TMP_DIR\"
    find '$APP_DIR' -mindepth 1 -maxdepth 1 ! -name logs ! -name node_modules ! -name data -exec rm -rf {} +
    cp -a \"\$TMP_DIR\"/. '$APP_DIR'/
    rm -rf \"\$TMP_DIR\"
    cd '$APP_DIR'
    npm ci
    mkdir -p logs
    [ -f .env ] && chmod 600 .env
    systemctl restart '$SERVICE_NAME'
    sleep 2
    systemctl --no-pager --full status '$SERVICE_NAME'
    echo
    journalctl -u '$SERVICE_NAME' -n 20 --no-pager
  "
