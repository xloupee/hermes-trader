# Dashboard component integration spec (manual)

## Scope
These scenarios are for the new `/dashboard` UI paths introduced in this branch.

## Paths
- `GET /dashboard/overview`
- `GET /dashboard/executions`
- `GET /dashboard/executions/[id]`
- `GET /dashboard/sources`
- `GET /dashboard/system`

## Acceptance checks
1. URL filters are preserved after refresh
   - start with `/dashboard/executions?since=24h&provider=...`
   - change any input and confirm route query string updates
   - hard refresh keeps the same result set

2. Preset behavior
   - `Landed Buys` persists `side=buy&outcome=landed` in the URL before requesting the first page
   - `Landed Sells` persists `side=sell&outcome=landed` in the URL before requesting the first page
   - `All` clears preset `side` and `outcome`; execution rows are never post-filtered in the client

3. Exact overview metrics
   - all four cards are sourced from full-range `/api/dashboard/overview` responses
   - landing rate denominator is `landed + failed_on_chain + ack_not_landed + send_failed`
   - the paginated `/api/dashboard/executions` page does not determine card values

4. Refresh controls
   - manual refresh triggers immediate network request
   - pause button toggles auto refresh
   - 15s auto interval is configured on `/overview`, `/executions`, `/sources`, `/system`, and `/executions/[id]`

5. Responsiveness
   - mobile width (`<=760px`) hides desktop tables and shows card layouts
   - mobile view still provides primary values for each row

6. Copy feedback
   - clicking copy icons updates adjacent feedback text to confirm success/failure
   - copied text is full value from clipboard for target and wallet fields

7. Detail diagnostics
   - `/dashboard/executions/[id]` shows grouped timing, signal, position, and fee sections
   - Raw JSON sections are collapsed by default using native `<details>`

8. Landing comparison labels
   - a landed sell with `landingComparison=no_target` reads `Landed · slot <copySlot> · no target comparison`
   - `same_slot` and `cross_slot` appear as secondary comparison context

9. Error and empty states
   - unreachable API returns an error message near toolbar/status
   - empty result returns “No ...” state with no broken table rows
