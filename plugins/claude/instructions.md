## Hot Sheet — ticket workflow

This project tracks work as **Hot Sheet** tickets (plain files under the store). Use
them to know what to do next and to record what you did. Everything below works
**headless** — no app, and no server required.

**Find work (priority order):**
- `hotsheet ls --up-next` — the prioritized Up Next queue.
- `hotsheet show <slug>` — read one ticket in full (e.g. `hotsheet show HS-7F3K9Q`).
- Or the MCP tools: `hotsheet_query` (with `up_next: true`) and `hotsheet_get`.

**Do the work, and record progress on the ticket as you go:**
- `hotsheet edit <slug> --status started` when you begin.
- `hotsheet edit <slug> --status completed --note "what you did"` when done.
- Or `hotsheet_update` / `hotsheet_close` (same effect through MCP).

**Create tickets for new work you discover** (bugs, follow-ups, gaps) rather than
leaving them in comments:
- `hotsheet new --title "…" --category bug` — or the `hotsheet_create` MCP tool.

Prefer the highest-priority Up Next ticket first. The CLI and the MCP tools go
through the exact same engine, so use whichever is handier.
