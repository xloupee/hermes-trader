# Deployment

Production deploys are handled by GitHub Actions when `main` is updated.

## Branch behavior

- `main`: runs CI, then deploys to the production VPS.
- `dev`: runs CI only. It does not deploy to the production VPS.
- Pull requests targeting `main` or `dev`: run CI only.

The deploy job is guarded with `github.ref == 'refs/heads/main'`, so a manual workflow run from another branch will not update production.

## Required GitHub secrets

Configure these repository secrets before relying on automatic production deploys:

- `VPS_SSH_PRIVATE_KEY`: private SSH key allowed to connect to the VPS user.
- `VPS_HOST`: SSH target, for example `root@157.90.240.233`. If omitted, `scripts/deploy.sh` defaults to `root@157.90.240.233`.

Optional repository secrets:

- `APP_DIR`: production app directory. Defaults to `/opt/pumpfun-migration-bot`.
- `SERVICE_NAME`: systemd service name. Defaults to `pumpfun-migration-bot`.

## What the deploy does

The workflow checks out the exact `main` commit, installs dependencies, runs the full test suite, configures SSH, then runs:

```bash
npm run deploy
```

The deploy script preserves the VPS `.env`, `logs`, `data`, and `node_modules` directories, rebuilds the app on the VPS, restarts the systemd service, and prints recent service logs.

## First enablement checklist

1. Add the required GitHub secrets.
2. Confirm branch protection requires the `CI / Check and test` job before merging to `main`.
3. Optionally require approval for the `production` environment in GitHub repository settings.
4. Merge a small non-functional change to `main` when ready to test the first automatic deploy.
