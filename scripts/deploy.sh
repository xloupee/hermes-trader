#!/usr/bin/env bash
set -euo pipefail

VPS_HOST="${VPS_HOST:-root@157.90.240.233}"
APP_DIR="${APP_DIR:-/opt/pumpfun-migration-bot}"
SERVICE_NAME="${SERVICE_NAME:-pumpfun-migration-bot}"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$ROOT_DIR"

echo "Stopping any local bot runner to avoid Telegram polling conflicts..."
bash scripts/stop-local.sh

echo "Checking source..."
npm run check

echo "Deploying to $VPS_HOST:$APP_DIR..."
ssh "$VPS_HOST" "mkdir -p '$APP_DIR'"

COPYFILE_DISABLE=1 tar \
  --no-xattrs \
  --exclude='.git' \
  --exclude='.env' \
  --exclude='node_modules' \
  --exclude='logs' \
  --exclude='data' \
  --exclude='.DS_Store' \
  --exclude='jito-shredstream-keypair.json' \
  --exclude='tools/shredstream-rs/target' \
  -czf - . |
  ssh "$VPS_HOST" "
    set -euo pipefail
    TMP_DIR=\$(mktemp -d)
    tar -xzf - -C \"\$TMP_DIR\"
    TARGET_CACHE='$APP_DIR/.shredstream-rs-target-cache'
    if [ -d '$APP_DIR/tools/shredstream-rs/target' ]; then
      rm -rf \"\$TARGET_CACHE\"
      mv '$APP_DIR/tools/shredstream-rs/target' \"\$TARGET_CACHE\"
    fi
    find '$APP_DIR' -mindepth 1 -maxdepth 1 ! -name .env ! -name logs ! -name node_modules ! -name data ! -name .shredstream-rs-target-cache -exec rm -rf {} +
    cp -a \"\$TMP_DIR\"/. '$APP_DIR'/
    if [ -d \"\$TARGET_CACHE\" ]; then
      mkdir -p '$APP_DIR/tools/shredstream-rs'
      rm -rf '$APP_DIR/tools/shredstream-rs/target'
      mv \"\$TARGET_CACHE\" '$APP_DIR/tools/shredstream-rs/target'
    fi
    rm -rf \"\$TMP_DIR\"
    cd '$APP_DIR'
    npm ci
    npm run build
    if [ -f tools/shredstream-rs/Cargo.toml ]; then
      CARGO_BIN=\$(command -v cargo || true)
      if [ -z \"\$CARGO_BIN\" ] && [ -x /root/.cargo/bin/cargo ]; then
        CARGO_BIN=/root/.cargo/bin/cargo
      fi
      if [ -z \"\$CARGO_BIN\" ]; then
        echo 'cargo not found; skipping ShredStream Rust decoder rebuild'
      else
        \"\$CARGO_BIN\" build --release --manifest-path tools/shredstream-rs/Cargo.toml
      fi
    fi
    mkdir -p logs
    [ -f .env ] && chmod 600 .env
    cat > /etc/systemd/system/'$SERVICE_NAME'.service <<'EOF'
[Unit]
Description=Pump.fun Telegram Migration Bot
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=$APP_DIR
Environment=NODE_ENV=production
Environment=BOT_SHUTDOWN_REASON=deploy
ExecStart=/usr/bin/node dist/index.js
Restart=always
RestartSec=5
User=root

[Install]
WantedBy=multi-user.target
EOF
    systemctl daemon-reload
    systemctl enable '$SERVICE_NAME' >/dev/null
    systemctl restart '$SERVICE_NAME'
    sleep 2
    systemctl --no-pager --full status '$SERVICE_NAME'
    echo
    journalctl -u '$SERVICE_NAME' -n 20 --no-pager
  "
