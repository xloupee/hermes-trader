#!/usr/bin/env bash
set -euo pipefail

VPS_HOST="${JITO_COPY_TEST_VPS_HOST:-root@157.90.240.233}"

ssh -o BatchMode=yes -o ConnectTimeout=10 "$VPS_HOST" 'bash -s' <<'REMOTE'
set -euo pipefail

echo "--- services"
systemctl is-active jito-copy-live.service || true
systemctl is-active jito-copy-sync.service || true
systemctl show -p ActiveEnterTimestamp --value jito-copy-live.service || true

echo "--- live config"
python3 - <<'PY'
from pathlib import Path
import subprocess

main_pid = subprocess.check_output(
    ["systemctl", "show", "-p", "MainPID", "--value", "jito-copy-live.service"],
    text=True,
).strip()
env = {}
if main_pid and main_pid != "0":
    for item in Path(f"/proc/{main_pid}/environ").read_bytes().split(b"\0"):
        if b"=" in item:
            key, value = item.split(b"=", 1)
            env[key.decode()] = value.decode(errors="replace")

for key in [
    "JITO_MAX_COPY_SOL",
    "JITO_MAX_TOTAL_COPY_SPEND_SOL",
    "JITO_FAST_COPY_SEND",
    "JITO_ENABLE_COPY_SEND",
    "JITO_ONE_SHOT_COPY_SEND",
    "JITO_SEND_FANOUT",
    "JITO_AUTO_SELL_AFTER_BUY",
    "JITO_PRIORITY_FEE_MICRO_LAMPORTS",
    "JITO_TIP_LAMPORTS",
]:
    print(f"{key}: {env.get(key, '')}")

print(f"JITO_TIP_ACCOUNT: {'configured' if env.get('JITO_TIP_ACCOUNT', '') else ''}")

for key in ["JITO_SEND_RPC_URLS", "JITO_BLOCK_ENGINE_SEND_URLS"]:
    value = env.get(key, "")
    count = len([part for part in value.split(",") if part.strip()]) if value else 0
    print(f"{key}: {count} configured")
PY

echo "--- executions"
if [ -f /var/log/jito-copy-executions-vps.jsonl ]; then
  wc -l /var/log/jito-copy-executions-vps.jsonl
  tail -n 5 /var/log/jito-copy-executions-vps.jsonl
else
  echo "missing /var/log/jito-copy-executions-vps.jsonl"
fi

echo "--- latest tracked feed events"
python3 - <<'PY'
from collections import deque
from datetime import datetime, timezone
import json
from pathlib import Path

def event_rows(path):
    rows = deque(maxlen=5)
    if not path.exists():
        return rows
    with path.open(errors="replace") as handle:
        for line in handle:
            if not line.startswith("{"):
                continue
            try:
                row = json.loads(line)
            except Exception:
                continue
            if row.get("schema") == "copytrade.feed.event.v1":
                rows.append(row)
    return rows

for label, path in [
    ("observe", Path("/var/log/jito-feed-probe-watch.jsonl")),
    ("copy-live", Path("/var/log/jito-copy-live.log")),
]:
    print(f"{label}:")
    rows = event_rows(path)
    if not rows:
        print("  no feed events")
        continue
    for row in rows:
        ms = row.get("observedAtMs")
        timestamp = (
            datetime.fromtimestamp(ms / 1000, timezone.utc).isoformat()
            if isinstance(ms, (int, float))
            else "n/a"
        )
        print(
            " ",
            timestamp,
            row.get("action"),
            row.get("mint"),
            row.get("signature"),
            "slot",
            row.get("slot"),
        )
PY

echo "--- sync"
tail -n 10 /var/log/jito-copy-sync.err.log 2>/dev/null || true
REMOTE
