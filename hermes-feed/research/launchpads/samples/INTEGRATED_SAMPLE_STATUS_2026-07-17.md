# Integrated Launchpad Sample Status — 2026-07-17

## Disposition

The seven launchpad evidence branches are integrated on `codex/samples-integration` with explicit fail-closed gaps preserved. This is a paper-evidence integration only. It is not execution, canary, promotion, deployment, or main-merge authorization.

The exact base is `agent/local-bankr-paper-observer` at `3829a7b2dccb2c651c85a920e19c2f705607ab6d`. Every source branch was a one-commit, add-only child of that exact base; every remote tip matched its reviewed local tip; and every imported path was confined to its declared `hermes-feed/research/launchpads/samples/<launchpad>/` namespace. No shared source, configuration, scripts, Cargo manifests, or lockfiles were changed.

## Import matrix

| Launchpad | Source commit | Imported evidence | Result |
|---|---|---|---|
| LaunchHood V3 | `70bbf6576b41b38670bb9fc46b0a2c7d6687cf73` | `launchhood_v3/**` | 10 embedded-initial-buy receipts; 10 entry/full-exit plans; zero targeted mismatches |
| Clanker | `f41fae9fd66ab412238a806d9deb8514fceb6d28` | `clanker/**` | 10 extensionless plus 10 pinned-five-position quotes; 3 strict envelope blocks |
| Bankr/Doppler | `dc5203ecb5d485a7e2bb18d9a69e61ead599b18d` | `bankr_doppler/**` | 11 samples across V1–V5; 6 embedded plans; V3/V4 active in recent inventory |
| Bow | `f43d8abedaf46a2d633bacdb0c7247648fc192f3` | `bow/**` | 10 zero-initial-buy plus 10 payable-initial-buy plans; 1 wrapped scored miss |
| Pons | `57e55bfd251e4483877eb4b60a5c339f0ee0f720` | `pons/**` | 10 current direct samples with exact prediction and quote matches |
| Hood | `3c53a4ce55d95070bf92ea5dd6367d86c361b9c9` | `hood/**` | 10 current-curve plans; 5 migrated-V3 plans; 5 strict topology blocks |
| Flap | `e3c982327af467feccdc200f5cb27c0a53e61843` | `flap/**` | 3,343 canonical TokenCreated claims; 0 quote claims; 0 in-range misses against the RPC log set |

The machine companion binds every source and integration commit, primary-artifact SHA-256, RPC endpoint, block ranges and available boundary hashes, profile disposition, receipt/event identity location, quote/entry/exit/slippage counts, and mismatch/miss counts.

## Aggregate evidence

- The tasks scanned 3,679,227 blocks in total. This is a sum of task ranges, not a unique union; several launchpad windows overlap.
- There are 81 quote or plan rows across LaunchHood, Clanker, Bankr, Bow, Pons, and Hood. Flap is discovery-only and deliberately has no quotes, entries, exits, slippage outcomes, readiness, or promotion claim.
- All imported evidence remains paper-only. Semantic checks found no enabled execution, signing, transaction construction, broadcast, canary, deployment, server access, or production mutation flag.
- All machine JSON and JSONL files parse. Imported checksums and source SHA bindings were rechecked after integration.

## Launchpad dispositions

### LaunchHood V3

The 100,000-block bounded scan found 21 canonical events and stopped after confirming the 10 most recent embedded-initial-buy samples. All ten have exact token/pool receipt identity, entry and full-exit plans at 100 bps slippage, and a simulated 9,801 bps immediate round trip. Targeted mismatch and miss counters are zero.

### Clanker

The 450,000-block scan found 1,573 exact factory/topic events. Ten of thirteen extensionless candidates were quote-eligible; the other three carried nonzero transaction value and correctly failed the zero-initial-buy envelope. A separate ten-row pinned-five-position sample passed. Historical replay did not measure live detector false-positive or miss rates.

### Bankr/Doppler

Three bounded historical/current windows total 629,227 blocks and 939 canonical Airlock events. The recent inventory contains 45 reviewed V3 and 31 reviewed V4 events. V1, V2, and V5 were historically observed but absent from the recent slice. Eleven targeted samples passed classification/identity checks and six have embedded paper plans. Twenty recent events remain unknown or unsupported. Direct-Airlock V5 remains explicitly unsupported.

The source artifact binds each window to an ordered event-inventory SHA-256 and each sample to its transaction/block identity, but does not record exact endpoint block hashes for the three windows. V1 and V3 also lack standalone exported plan JSON at this source commit. Both gaps remain explicit; this integration does not modify the shared quote path.

### Bow

The exact 750,000-block scan found 67 canonical events. Twenty direct profiles passed: ten zero-initial-buy and ten payable-initial-buy. The requested `0x6460c0afc4cbdac9e5e5b62db5eb982a92d4affc7051ccf89daa1e5df332f100` remains an unsupported scored miss because its launch is nested inside a 31-call Multicall3 envelope with 30 buyer swaps.

### Pons

The exact 750,000-block scan found 82 current-generation events and separately counted 7,094 legacy events. Ten current direct samples passed token prediction, pool prediction, entry, 100-bps slippage, and full-exit checks. Fifty-one rotating wrappers and two ambiguous transaction lookups remain fail closed; legacy observations do not count toward current readiness.

### Hood

Two disjoint ranges total exactly 750,000 blocks. Runtime pins matched 20/20 at both range endpoints. The current-curve sample contains four launches, three ordinary buys, and three sells, all with exact plans. Ten migration scopes were attempted: five produced exact migrated-V3 plans and five stayed blocked by strict receipt topology. Those five are explicit misses and must not be converted into plans without a separately reviewed verifier change.

### Flap

The exact 250,000-block finalized scan confirmed all 3,343 canonical Portal `TokenCreated` claims returned by the public RPC: 3,242 direct-Portal origins and 101 VaultPortal origins. It reported zero false positives, decode misses, in-range misses, and action mismatches. `TokenBought` and `TokenSold` were controls only and never substituted for launch ground truth. The zero-miss conclusion is relative to one public RPC log set; independent provider or Blockscout completeness replay remains unresolved.

## Safety conclusion

The integrated branch is suitable as a provenance-bound paper evidence package with the gaps above. It is not promotion-ready, and it grants no authority for wallet access, keys, signing, transaction construction, broadcast, canary, deployment, Droplet/server access, production mutation, or merging main.
