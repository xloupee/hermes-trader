# Hermes readiness monitoring

`hermes-readiness-monitor` is a read-only process. It polls the pinned mainnet
NOXA factory, checks bytecode at the known WETH/router/factory candidates on
Robinhood testnet, and watches the official Robinhood and NOXA contract pages
for a testnet deployment marker. It never loads a signer or submits a
transaction.

The retained user-service templates add a second defense: both make
`/srv/codex-workspaces/hermes-secrets` inaccessible to the process. The paper
shadow service also starts the runtime strictly in `--mode paper`, which refuses
keystore and broadcast arguments.

The retained paper-shadow template runs one 30-minute session. It does not
restart after normal completion or failure, so the observation window cannot
silently extend. The runtime reconnects transient feed disconnects inside the
same process with bounded backoff, preserves its original deadline and sequence
tracker, and fails closed at the disconnected boundary. Each reconnect uses
Nitro's official `Arbitrum-Requested-Sequence-Number` catch-up header with the
next expected sequence. Any older backlog replay is skipped, while a forward
jump remains a sequence gap and therefore fails closed. Its final record
separates reconnects, skipped replay messages, forward resume gaps, cumulative
sequence health, RPC metrics, and deadline versus SIGTERM/SIGINT completion.

## One-shot verification

```bash
cargo run --release --bin hermes-readiness-monitor -- --once
```

`ready_for_manual_testnet_address_validation=true` is only a wake-up signal. It
does not authorize a transaction. Official addresses, bytecode, interfaces, and
liquidity must still be verified before testnet wrapping or approval.

After those checks, `hermes-noxa testnet-validate-round-trip-step` provides a
strictly read-only gate for each externally signed wrap, exact approval, entry,
exact full-position approval, and exit. Validation does not authorize or submit
the bytes; broadcasting remains a separate explicit action.

The paired `testnet-submit-round-trip-step` command also defaults to dry-run and
requires a separate exact approval token plus `--broadcast`. Its existence does
not change the monitor's read-only behavior or authorize a transaction.

## Install-ready user units

The files under `ops/systemd/` are templates only. Merely building the project
does not install or start them. After explicit activation approval, install them
under the user's systemd configuration, reload the user manager, and start the
units. No system-level service or root access is required.

Useful read-only commands after activation:

```bash
systemctl --user status hermes-readiness-monitor.service
systemctl --user status hermes-paper-shadow.service
journalctl --user -u hermes-readiness-monitor.service -n 20 --no-pager
journalctl --user -u hermes-paper-shadow.service -n 20 --no-pager
```
