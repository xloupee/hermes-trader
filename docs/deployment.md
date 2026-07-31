# Deployment

GitHub Actions is CI-only. It does not connect to a server, transfer files, restart services, or deploy an application.

## CI behavior

- Pushes to `main`: run the root, frontend, Rust, and tracked-tree absence checks.
- Pull requests targeting `main` or `dev`: run the same CI checks.
- Manual workflow runs execute checks only when run from `main`.

No push, pull request, or manual workflow run performs a backend deployment.

## Backend deployment policy

Backend deployment is manual and fail-closed. An operator must explicitly choose the target environment and host, review the exact commit, confirm the required checks passed, and invoke an approved deployment procedure outside GitHub Actions.

Deployment tooling must stop when target, credentials, application directory, or service name are missing. It must not infer or fall back to a historical host.

## Manual deployment checklist

1. Identify the exact commit and target environment.
2. Confirm the CI safety checks passed for that commit.
3. Supply every target and service parameter explicitly.
4. Obtain the required operational approval.
5. Run the approved manual procedure and retain its deployment evidence.
