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

The docs show the proxy needs block engine/auth/region and UDP destination settings, for example:

```text
BLOCK_ENGINE_URL=https://mainnet.block-engine.jito.wtf
AUTH_KEYPAIR=my_keypair.json
DESIRED_REGIONS=ny
SRC_BIND_PORT=20000
DEST_IP_PORTS=<host>:8001
GRPC_SERVICE_PORT=9999
```

Firewall/NAT must allow UDP on `SRC_BIND_PORT`; the docs suggest checking packet arrival with `tcpdump` on that UDP port.
