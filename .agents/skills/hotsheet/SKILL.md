---
name: hotsheet
description: Read the Hot Sheet worklist and work through the current priority items
---
<!-- hotsheet-skill-version: 28 -->

Read `.hotsheet/worklist.md` and work through the tickets in priority order.

For each ticket:
1. Read the ticket details carefully
2. Implement the work described
3. When complete, mark it done via the Hot Sheet UI

Work through them in order of priority, where reasonable.

If the worklist says "Auto-Prioritize", follow those instructions to choose and mark tickets as Up Next before working on them.

Before using a connected MCP, confirm the generation. `hotsheet-store.json` (directly or
through `.hotsheet/store`) identifies an HS2 git store; `.hotsheet/db/PG_VERSION` identifies
an HS1 PGLite project. If both generations are relevant, the connected MCP may still belong
to HS1; use `target/debug/hotsheet-cli -C <HS2-store>` until the endpoint identifies as HS2.
A 401/403 alone does not prove the secret is stale. If the endpoint identifies as HS2, then
re-read its machine-local configuration; otherwise report the wrong-generation/project
mismatch explicitly.

**MCP tools (`hotsheet_*`) are preferred over curl when the channel is connected** — see the worklist for per-operation guidance. The 14-tool surface covers ticket lifecycle (`hotsheet_update_ticket`, `hotsheet_create_ticket`, `hotsheet_get_ticket`, `hotsheet_delete_ticket`, `hotsheet_restore_ticket`, `hotsheet_toggle_up_next`, `hotsheet_duplicate_tickets`), bulk operations (`hotsheet_batch`), notes (`hotsheet_edit_note`, `hotsheet_delete_note`), attachments (`hotsheet_add_attachment`), channel signaling (`hotsheet_signal_done`), feedback sugar (`hotsheet_request_feedback`), and query (`hotsheet_query_tickets`). Curl stays supported as the universal fallback for non-Claude AI agents and human terminal callers.

## Git: keep the target current + integrate ready branches

You run on the **target branch** (usually `main`) in the main worktree, so you are the **integrator**: a self-claim worker (`/hotsheet-worker`, working in its own git worktree) commits its work on its own branch and marks its ticket `pending_integration` with the `integration_branch` it landed on — you merge those into the target (a worker never writes the target itself). When no such workers are in play this section is simply unused.

- **Stay current** — `git fetch` then `git pull --rebase` (when the repo has a remote) before integrating; commit or stash your own in-progress changes first so a merge doesn't tangle with them.
- **Integrate ready branches** — for each ticket marked `pending_integration` (its `integration_branch` names the branch to merge), in ticket-priority order, `git merge` that branch into the target from committed state. Auto-resolve trivial/mechanical conflicts; if a conflict is non-trivial or ambiguous, **stop and ask the maintainer** rather than force it.
- **Run the gates after each merge** — type-check, lint, and the relevant tests — before moving on; if they fail in a way you can't quickly and safely fix, stop and ask.
- **Clear the marker** — for each ticket you integrated, `hotsheet_update_ticket` with `{ "id": <id>, "pending_integration": false }`.
- **NEVER `git push`** without the maintainer's explicit permission — local integration only.
