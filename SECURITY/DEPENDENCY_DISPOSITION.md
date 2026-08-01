# Dependency Security Disposition

Snapshot date: 2026-07-31

This disposition records advisories that cannot be removed without violating
the supported dependency contracts of the two independently installed Node.js
artifacts. It is not an acceptance of forced or breaking remediation.

## Vercel dashboard artifact

Scope: `latency-dashboard/package.json` and `latency-dashboard/package-lock.json`.
The Vercel project root is `latency-dashboard`, so root-package dependencies are
not installed into this artifact.

`npm audit --omit=dev` reports three high-severity package findings: direct
`next@15.5.22`, plus its transitive `postcss@8.4.31` and optional
`sharp@0.34.5` dependencies. The underlying advisories are:

- `postcss`: GHSA-qx2v-qp2m-jg93, GHSA-6g55-p6wh-862q, and
  GHSA-r28c-9q8g-f849. PostCSS is part of the build-time CSS processing surface;
  the file-read cases require processing attacker-controlled CSS or source-map
  comments, which this tracked-source build must not accept.
- `sharp`: GHSA-f88m-g3jw-g9cj. Sharp is Next's optional server-side image
  processing dependency and may be present in the deployed server artifact;
  untrusted image processing must not be introduced while this remains open.

Registry-compatible candidates checked on 2026-07-31 were `postcss@8.5.18`
and `sharp@0.35.0`. Both support Node 24, but neither is compatible with Next
15.5.22's declared dependency contract: Next pins PostCSS to `8.4.31` and
declares Sharp as `^0.34.3`. An npm override would therefore be an unsupported
resolution, not a proven compatible patch. No override is applied.

The audit-suggested `next@9.3.3` downgrade is invalid. It crosses multiple Next
major versions, predates the current App Router and React 19 contract, and
would create functional and security regression risk rather than a safe fix.

Follow-up release gate: upgrade only to an official supported Next release whose
manifest permits a PostCSS version newer than 8.5.17 and Sharp 0.35.0 or newer.
Then rerun `npm ci`, `npm test`, `npm run check`, `npm run build`,
`npm audit --omit=dev`, bundle/source-map secret scans, and exact-artifact
preview verification before promotion. Until then, builds must use only tracked
CSS and the dashboard must not add untrusted image-processing inputs.

## Root Solana service artifact

Scope: root `package.json` and `package-lock.json`. These findings affect the
Node.js Solana bot/service dependency graph, not the independently installed
Vercel dashboard artifact.

`npm audit --omit=dev` reports 13 inherited findings: 6 high and 7 moderate.
The actionable vulnerable leaves and paths are:

- `bigint-buffer@1.1.5` (GHSA-3gc7-fjrx-p6mg) through
  `@solana/buffer-layout-utils`, `@solana/spl-token`, and the Pump SDKs. This
  touches token amount/layout processing used by transaction construction and
  observation paths.
- `uuid@8.3.2` (GHSA-w5hq-g745-h8pq) through `jayson` and
  `@solana/web3.js`, propagated through Anchor, SPL Token, and the Pump SDKs.
  This belongs to the root RPC/client runtime graph.

The latest registry candidates checked were `@solana/spl-token@0.4.15`,
`@solana/web3.js@1.98.4`, `@solana/buffer-layout-utils@0.3.0`,
`jayson@4.3.0`, and `bigint-buffer@1.1.5`. They retain the vulnerable leaves:
SPL Token 0.4.15 still selects buffer-layout-utils with `bigint-buffer@^1.1.5`,
and current web3.js still selects Jayson with `uuid@^8.3.2`. No compatible
registry resolution removes these advisories.

The audit-suggested downgrades to `@solana/spl-token@0.1.8`,
`@solana/web3.js@0.0.3`, `@pump-fun/pump-sdk@1.1.0`, or
`@pump-fun/pump-swap-sdk@1.0.0` are breaking and outside the reviewed runtime
contract. `npm audit fix --force`, forced overrides, and speculative Solana
dependency changes are prohibited for this release.

Follow-up release gate: update only when upstream Solana and Pump SDK releases
publish a compatible graph that removes both vulnerable leaves. Review API and
transaction behavior changes, then rerun root `npm ci`, `npm run check`,
`npm test`, `npm audit --omit=dev`, supported shredstream Cargo check/test, and
the existing absence and secret scans before release consideration.
