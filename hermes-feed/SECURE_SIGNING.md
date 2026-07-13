# Secure Hermes signing

Hermes accepts only a standard encrypted Web3 Secret Storage keystore. The
private key and password must never be placed in source files, command-line
arguments, environment-variable values, logs, or chat.

## Runtime contract

- The keystore must be a regular file, not a symlink.
- Its mode must grant no group or other access (`0600` or stricter).
- Its immediate parent directory must not be a symlink or group/other writable
  (`0700` is recommended).
- Hermes holds the validated file open and rejects the load if the path's
  device or inode changes during decryption.
- Startup requires an explicit expected public address. A keystore decrypting
  to any other address is rejected.
- The password is read only from an inherited file descriptor numbered 3 or
  higher. Standard input and password command-line arguments are refused.
- Decrypted bytes and the password buffer are zeroized after loading. Debug and
  error output never includes either value.

The user provisions the encrypted keystore and protected password descriptor
directly on Falkenstein. Codex does not create, read, copy, or inspect either
secret.

## User-run provisioning

Perform the key import on a trusted local machine. Foundry's interactive mode
prompts in the terminal, so the private key and encryption password do not
become command arguments or shell-history entries:

```bash
cast wallet import hermes-trader --interactive
```

This creates an encrypted Web3 keystore in Foundry's keystore directory. Copy
only that encrypted file to Falkenstein; never copy a raw key file, seed phrase,
terminal transcript, or password file. Run these filesystem steps yourself,
replacing `SSH_USER` with your login:

```bash
# Falkenstein
install -d -m 700 /srv/codex-workspaces/hermes-secrets

# Trusted local machine
scp ~/.foundry/keystores/hermes-trader \
  SSH_USER@157.90.240.233:/srv/codex-workspaces/hermes-secrets/trader.json

# Falkenstein
chmod 600 /srv/codex-workspaces/hermes-secrets/trader.json
```

Confirm that the public address printed by Foundry is the dedicated trading
address you intend Hermes to use. The public address is safe to compare; do not
print or inspect the decrypted private key.

## Validation shape

After building the release binary, invoke it with descriptor 3 already attached
to the protected password source:

```bash
hermes-keystore-check \
  --keystore /srv/codex-workspaces/hermes-secrets/trader.json \
  --expected-address 0xYOUR_PUBLIC_TRADING_ADDRESS \
  --password-fd 3 \
  3<PROTECTED_PASSWORD_SOURCE
```

Replace `PROTECTED_PASSWORD_SOURCE` locally. Do not paste the resulting command
if it contains a secret path or value. Successful output contains only the
public address and validation status.

For a one-time interactive validation on Ubuntu, the password can be supplied
without a plaintext password file by attaching `systemd-ask-password` directly
to descriptor 3:

```bash
hermes-keystore-check \
  --keystore /srv/codex-workspaces/hermes-secrets/trader.json \
  --expected-address 0xYOUR_PUBLIC_TRADING_ADDRESS \
  --password-fd 3 \
  3< <(systemd-ask-password "Hermes keystore password")
```

Stop after validation. Do not start signed mode or add a persistent credential
source until the runtime batch is published and deployment is explicitly
approved. `--broadcast` additionally requires a separate live-canary approval;
the current runtime refuses it while the NOXA factory reports
`launchEnabled=false`.
