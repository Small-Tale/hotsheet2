## Hot Sheet — ticket workflow

This project tracks work as **Hot Sheet** tickets (plain files under the store). Use
them to know what to do next and to record what you did. Everything below works
**headless** — no app, and no server required.

**Find and plan the complete queue:**
- `hotsheet-cli ls --up-next` — the prioritized Up Next queue.
- `hotsheet-cli show <slug>` — read one ticket in full (e.g. `hotsheet-cli show HS-7F3K9Q`).
- Or the MCP tools: `hotsheet_query` (with `up_next: true`) and `hotsheet_get`.

**Do the work, and record progress on the ticket as you go:**
- `hotsheet-cli edit <slug> --status started` when you begin.
- `hotsheet-cli edit <slug> --status completed --note "what you did"` when done.
- Or `hotsheet_update` (it takes a `note`) / `hotsheet_close` (same effect through MCP).

**Create every follow-up immediately, without asking.** As soon as you identify an
unfinished step, open question, known gap, out-of-scope task, or designed-but-unbuilt
behavior, create its ticket rather than leaving it in a comment, TODO, or note:
- `hotsheet-cli new --title "…" --category bug` — or the `hotsheet_create` MCP tool.

Before completing a ticket: finish and verify its scope; update required tests, coverage,
and docs; scan for incomplete work; create every needed follow-up; and include the result,
verification, and all follow-up slugs in the completing note. `FEEDBACK NEEDED` is only
for a blocker on the current ticket that requires a user decision or unavailable external
state. Leave that ticket started and name the blocker; it does not replace follow-ups for
independently describable work.

Normally continue until every actionable Up Next ticket is complete. Read the whole queue
before choosing an order; consider dependencies, overlap, shared context, risk, and safe
parallelization. Treat priority as important guidance rather than a hard ordering rule.
The CLI and MCP tools use the same engine, so use whichever is handier.
