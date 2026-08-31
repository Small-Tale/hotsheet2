---
name: hotsheet
description: Plan and work through the complete Hot Sheet Up Next queue using priority, overlap, dependencies, and safe parallelism. Works headless, with or without a server.
---

<!-- hotsheet-skill-version: 31 -->

Work the project's complete Hot Sheet Up Next queue. An invocation normally drains every
actionable Up Next ticket; completing one ticket is not a stopping condition.

1. **Read and plan the whole queue.** List Up Next, read every queued ticket in full, and
   inspect relevant code before choosing execution order. Build a short working plan that
   identifies dependencies, implementation overlap or duplicates, independent work,
   safe parallelization opportunities, verification needs, and commit boundaries.
   Priority is an important guidance signal, not an absolute ordering rule. Prefer higher
   priority when other factors are equal, but reorder when dependencies, shared context,
   risk reduction, or avoiding duplicated work makes another order clearly better.
2. **Use available parallelism deliberately.** When sub-agents are available and the
   environment and user authorize delegation, assign concrete independent tickets or
   bounded investigations in parallel. Do not parallelize tickets that edit the same
   surfaces or depend on unresolved decisions. The primary agent owns integration,
   ticket status, verification, and publishing.
3. **Work each ticket end to end.** Mark it started, implement and verify its scope, then
   mark it completed with a result and verification note.
4. **File follow-ups** for discovered work that is not completed. Do not leave loose ends
   only in comments or TODOs.
5. **Publish at ticket boundaries.** Run required gates, review the diff, make one commit
   for that ticket, and push before beginning the next sequential ticket. Combine tickets
   only when their implementations overlap so strongly that separation would be unsafe
   or misleading, or when they are duplicates. Integrate parallel tickets separately.
6. **Re-read the queue after every completion.** Concurrent work and new findings can
   change the plan. Continue until no actionable Up Next ticket remains.

Stop early only for an explicit user ticket/time/budget limit, an empty queue, or a
genuine blocker requiring user input or unavailable external state. Exhaust safe
in-scope alternatives, record the blocker, leave status accurate, and continue other
independent Up Next work before stopping.

Notes:
- The CLI (`hotsheet-cli …`) and `hotsheet_*` MCP tools use the same engine and work
  without a server.
- Confirm HS2 generation before using connected MCP: `hotsheet-store.json` (directly
  or through `.hotsheet/store`) identifies HS2; `.hotsheet/db/PG_VERSION` identifies
  HS1. If uncertain, use `hotsheet-cli -C <HS2-store>`.
- If a ticket is unclear, do not guess. Record the needed decision, continue independent
  tickets, and return if the answer becomes available.
