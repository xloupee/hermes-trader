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
   - `Landed Buys`, `Landed Sells`, `Non-landed Attempts` only toggle presentation while keeping backend query keys unchanged
   - `All` resets preset to no client-side outcome filter

3. Refresh controls
   - manual refresh triggers immediate network request
   - pause button toggles auto refresh
   - 15s auto interval is configured on `/overview`, `/executions`, `/sources`, `/system`, and `/executions/[id]`

4. Responsiveness
   - mobile width (`<=760px`) hides desktop tables and shows card layouts
   - mobile view still provides primary values for each row

5. Copy feedback
   - clicking copy icons updates adjacent feedback text to confirm success/failure
   - copied text is full value from clipboard for target and wallet fields

6. Detail diagnostics
   - `/dashboard/executions/[id]` shows grouped timing, signal, position, and fee sections
   - Raw JSON sections are collapsed by default using native `<details>`

7. Error and empty states
   - unreachable API returns an error message near toolbar/status
   - empty result returns “No ...” state with no broken table rows
