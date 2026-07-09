# ShredStream gRPC Notes

Issue #31 tracks connecting the local Pump/PumpSwap decoder to real Jito ShredStream data.

## Official Contract

Primary sources checked:

- Jito ShredStream docs: https://docs.jito.wtf/lowlatencytxnfeed/
- Jito proxy repo: https://github.com/jito-labs/shredstream-proxy
- Proxy deshred example: https://github.com/jito-labs/shredstream-proxy/blob/master/examples/deshred.rs
- Protobuf source: https://github.com/jito-labs/mev-protos/blob/c9614089ef48fb83f01767d87e8f73e6c2e59c0b/shredstream.proto

The proxy enables a local gRPC server with `GRPC_SERVICE_PORT=<PORT>` or `--grpc-service-port <PORT>`.

The gRPC service is:

```proto
service ShredstreamProxy {
  rpc SubscribeEntries(SubscribeEntriesRequest) returns (stream Entry);
}
```

The streamed message is:

```proto
message Entry {
  uint64 slot = 1;
  bytes entries = 2;
}
```

Important nuance: `entries` is not JSON and is not protobuf-encoded transactions. The proto comments and Rust example show it is serialized bytes for `Vec<solana_entry::entry::Entry>`, decoded in Rust with `bincode::deserialize`.

## Implemented Bridge

The first bridge is implemented as a Rust sidecar in `tools/shredstream-rs`.

The Node listener keeps JSONL as its internal boundary. In `SHREDSTREAM_SOURCE=grpc` mode, `src/shredstream-source.ts` starts:

```text
cargo run --manifest-path tools/shredstream-rs/Cargo.toml --quiet -- watch --grpc-url <url>
```

The sidecar subscribes to `ShredstreamProxy.SubscribeEntries`, decodes `Entry.entries` with `bincode::deserialize::<Vec<solana_entry::entry::Entry>>()`, and prints one normalized transaction JSON object per line:

```ts
interface ShredstreamTransactionInput {
  slot: number;
  signature: string;
  receivedAtMs?: number;
  accountKeys: string[];
  instructions: Array<{
    programIdIndex?: number;
    programId?: string;
    accounts?: Array<number | string>;
    dataBase64?: string;
  }>;
}
```

The sidecar pins Solana crates to `=2.2.1`, matching the current Jito proxy workspace dependencies.

## Remaining Gaps

- Address lookup table accounts are not hydrated in this first bridge. The emitted `accountKeys` are static transaction keys only.
- The listener is still discovery-only: no Telegram messages and no live trades.
- Live proxy validation is still required before relying on this as a token signal.

## Proxy Runtime Sketch

The proxy needs block engine/auth/region settings. This deployment consumes the
local gRPC service only, so it deliberately does **not** configure a raw UDP
destination:

```text
BLOCK_ENGINE_URL=https://mainnet.block-engine.jito.wtf
AUTH_KEYPAIR=my_keypair.json
DESIRED_REGIONS=ny
SRC_BIND_PORT=20000
GRPC_SERVICE_PORT=9999
```

Firewall/NAT must allow UDP on `SRC_BIND_PORT`; the docs suggest checking packet arrival with `tcpdump` on that UDP port.

Do not set `DEST_IP_PORTS=127.0.0.1:8001` unless a real raw-shred consumer is
bound there. Sending every shred to an unbound loopback port creates UDP
`NoPorts` and ICMP churn without helping the Rust worker. The managed wrapper
defaults `JITO_SHREDSTREAM_GRPC_ONLY=true`, unsets `DEST_IP_PORTS`, and rejects
`--dest-ip-ports` arguments. Set `JITO_SHREDSTREAM_GRPC_ONLY=false` only for an
intentional, verified raw-shred consumer.

## Droplet UDP receive-buffer preparation

The proxy has bursty UDP ingress. Its systemd template requires both
`net.core.rmem_default` and `net.core.rmem_max` to be at least 8 MiB before it
starts. Check the values without changing the host:

```bash
cd /opt/jito-feed-probe-watch
JITO_SHREDSTREAM_UDP_RCVBUF_BYTES=8388608 \
  ./prepare-shredstream-udp.sh check
```

Applying the settings is a separate, explicit root action. Inspect the current
Droplet values and newest live service definition first, then run:

```bash
sudo JITO_SHREDSTREAM_ALLOW_SYSCTL_APPLY=YES \
  JITO_SHREDSTREAM_UDP_RCVBUF_BYTES=8388608 \
  /opt/jito-feed-probe-watch/prepare-shredstream-udp.sh apply
```

This command changes only the runtime sysctls. Persist them through the
Droplet's normal sysctl configuration management after the canary proves the
chosen size; the script does not silently edit `/etc/sysctl.d`.

Install the reviewed proxy unit only after reconciling it with the newest live
unit and environment:

```bash
sudo install -m 0644 \
  /opt/jito-feed-probe-watch/systemd/jito-shredstream-proxy.service \
  /etc/systemd/system/jito-shredstream-proxy.service
sudo systemctl daemon-reload
sudo systemctl restart jito-shredstream-proxy.service
```

The unit uses `/etc/jito-shredstream-proxy.env` for the existing Jito auth,
region, source-port, and gRPC-port variables. The wrapper never prints those
values.

## Read-only post-restart health gate

After restart, sample kernel counters and recent proxy logs:

```bash
sudo JITO_SHREDSTREAM_HEALTH_INTERVAL_SECONDS=10 \
  /opt/jito-feed-probe-watch/check-shredstream-feed-health.sh
```

The gate fails if the sample observes any new `UdpRcvbufErrors`, UDP `NoPorts`,
or missed-FEC log lines since the proxy's current `ActiveEnterTimestamp`. The
totals are host-wide kernel counters, so investigate
another UDP service before attributing an unexpected delta to ShredStream. For
a deliberately shared host, the three thresholds are configurable through
`JITO_SHREDSTREAM_MAX_RCVBUF_ERROR_DELTA`,
`JITO_SHREDSTREAM_MAX_NO_PORTS_DELTA`, and
`JITO_SHREDSTREAM_MAX_FEC_MISSES`.
