# DoubleZero Edge shadow migration

This package stages a **default-off, proxy-only** DoubleZero Edge feed beside the
current Jito feed. It does not configure GRE, BGP, PIM, routes, multicast, or a
DigitalOcean firewall. It does not start a second copytrade worker and it does
not authorize trades from the shadow feed.

The shadow proxy preserves the worker-facing contract:
`ShredstreamProxy.SubscribeEntries` carrying bincode `Vec<Entry>` payloads. It
uses `127.0.0.1:10099`; production remains on `127.0.0.1:9999`.

## Hard invariants

1. Never run a second executor against the shadow port. There must be exactly
   one copytrade worker and one global signature arbiter/deduper before a second
   source can influence execution.
2. `DOUBLEZERO_SHADOW_ALLOW_EXECUTION=false` at every shadow stage.
3. Do not enable cutover, change the production endpoint, or stop the primary
   feed during shadow validation.
4. Do not use this package to mutate network state. DoubleZero or the hosting
   provider must supply and validate the GRE/BGP/PIM configuration separately.
5. Before any Droplet deployment, retrieve and diff the newest live tree. Do
   not overwrite the live overlay with a stale checkout.

## Why the DigitalOcean gates are manual

An `UP` interface is insufficient evidence. The host must demonstrate all of:

- Bidirectional GRE packets through the DigitalOcean data plane.
- An established and stable DoubleZero BGP session.
- The assigned multicast group arriving on `doublezero1`.
- No provider firewall, anti-spoofing, or multicast restriction blocking the
  return path.

The preflight therefore requires four explicit attestations. Do not set them
from assumptions or from staged firewall rules:

```bash
DOUBLEZERO_DO_GRE_VALIDATED=true
DOUBLEZERO_DO_BGP_VALIDATED=true
DOUBLEZERO_DO_MULTICAST_VALIDATED=true
DOUBLEZERO_GRPC_ONLY_PROXY_VALIDATED=true
```

`DOUBLEZERO_GRPC_ONLY_PROXY_VALIDATED` means the exact installed proxy binary
has been tested with multicast input and gRPC output without a raw UDP
destination. Proxy versions differ; inspect and record its version and hash.

## Read-only preflight

Obtain the assigned multicast IP from DoubleZero, then run without `sudo`:

```bash
export DOUBLEZERO_MULTICAST_GROUP=233.x.x.x
export DOUBLEZERO_DO_GRE_VALIDATED=true
export DOUBLEZERO_DO_BGP_VALIDATED=true
export DOUBLEZERO_DO_MULTICAST_VALIDATED=true
export DOUBLEZERO_GRPC_ONLY_PROXY_VALIDATED=true
export DOUBLEZERO_SHADOW_ALLOW_EXECUTION=false
./doublezero-edge-shadow-preflight.sh
```

The check only reads the interface, IPv4 address, route selection, and listening
TCP ports. It fails if the multicast route does not resolve through
`doublezero1`, if port 10099 conflicts with production, or if any gate is absent.

## Installing the inert service template

Do this only after the live-tree snapshot/diff and explicit deployment approval.
The checked-in unit has no `[Install]` section and defaults
`DOUBLEZERO_SHADOW_ENABLED=false`, so copying it cannot enable or start it.

Create `/etc/jito-shredstream-doublezero-shadow.env` with mode `0600`:

```bash
DOUBLEZERO_SHADOW_ENABLED=true
DOUBLEZERO_SHADOW_ALLOW_EXECUTION=false
DOUBLEZERO_SHADOW_CONSUMER_MODE=observer
DOUBLEZERO_DEVICE=doublezero1
DOUBLEZERO_MULTICAST_GROUP=233.x.x.x
DOUBLEZERO_MULTICAST_PORT=20001
DOUBLEZERO_SHADOW_GRPC_PORT=10099
JITO_PRIMARY_GRPC_PORT=9999
DOUBLEZERO_DO_GRE_VALIDATED=true
DOUBLEZERO_DO_BGP_VALIDATED=true
DOUBLEZERO_DO_MULTICAST_VALIDATED=true
DOUBLEZERO_GRPC_ONLY_PROXY_VALIDATED=true
```

Starting the unit launches only `jito-shredstream-proxy forward-only`. It does
not launch `jito-copy-live`, alter its environment, or attach an executor to the
shadow gRPC endpoint.

## Shadow evidence and promotion gate

Use a non-trading observer or a single in-process global arbiter to compare the
same signatures from ports 9999 and 10099. Record source, receive time, decode
time, slot, signature, duplicate status, packet/FEC loss, and reconnects.

Promotion is forbidden until all of the following are proven:

- One worker/global arbiter owns execution and dedupes across both sources.
- No transaction can be submitted once per source.
- A representative multi-day sample shows stable GRE/BGP/multicast delivery.
- Feed coverage, first-seen race, FEC loss, and p50/p90/p99 are measured.
- A rollback changes only the arbiter's authorized source; it never launches a
  second worker.
- The production endpoint and primary proxy remain recoverable.

Only after a separately reviewed change may `global-arbiter` replace `observer`.
That mode additionally requires `DOUBLEZERO_GLOBAL_ARBITER_VALIDATED=true`.
This package contains no cutover command by design.
