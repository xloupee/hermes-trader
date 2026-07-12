# Feed region decision

Measurement date: 2026-07-12 UTC

## Result

Falkenstein, Germany (`fsn1-codex`, Hetzner AS24940) beat the existing
DigitalOcean FRA1 host (`fra1-shared`) on the direct Robinhood Chain Nitro
sequencer feed.

| Metric | FRA1 | Falkenstein |
|---|---:|---:|
| Exact matched sequences | 10,393 | 10,393 |
| Arrival wins | 2,319 | 8,074 |
| Relative lag p50 | 5.348 ms | 0 ms |
| Relative lag p95 | 20.468 ms | 6.664 ms |
| Relative lag p99 | 30.812 ms | 16.154 ms |
| Gaps / missing / reconnects | 0 / 0 / 0 | 0 / 0 / 0 |

After clock correction, the p95 advantage was 13.804 ms.

## Clock qualification

Both hosts reported synchronized systemd clocks. Two independent 100/200
sample persistent-SSH midpoint trials measured FRA1 minus Falkenstein offsets
of -0.684 ms and -1.993 ms. Their best-RTT uncertainty bounds were ±4.964 ms
and ±5.140 ms. Correcting either negative FRA1 offset makes FRA1 arrivals later,
not earlier. The corrected 13.804 ms p95 lead exceeds both bounds. The
comparator withholds a winner when the p95 lead does not exceed the supplied
uncertainty; this run returned `decision_ready: true` and
`winner: fsn1-codex`.

## Decision

Use Falkenstein as the first persistent deployment candidate. This measurement
proves the feed-arrival choice between these two hosts; it does not prove final
transaction inclusion latency. Before live trading, repeat the submission leg
on testnet from a user-controlled Falkenstein host.

The DigitalOcean host remained isolated under `/srv/hermes-probe`; the existing
PumpFun process and configuration were not inspected or changed.
