---
name: hotsheet
description: Plan and work through the complete Hot Sheet Up Next queue using priority, overlap, dependencies, and safe parallelism. Works headless, with or without a server.
allowed-tools: Read, Grep, Glob, Edit, Write, Bash
---

<!-- hotsheet-skill-version: 35 -->

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
3. **Work each ticket end to end under an exact claim lease.** Choose one stable,
   session-specific worker id. Immediately before active work, claim the assigned ticket
   with the atomic CLI form
   `hotsheet-cli claim <id> --worker <worker> [--label <label>] [--lease-minutes N] --start`,
   which acquires the claim and changes Not Started to Started in one durable write.
   Do not issue separate claim and status commands. Renew
   before the lease expires and before lengthy work with `hotsheet_renew` or
   `hotsheet-cli renew`; release with `hotsheet_release` or `hotsheet-cli release` when
   work stops for completion, handoff, error, or feedback. If another worker holds the
   live lease, do not work concurrently; replan around other tickets. Then implement and
   verify scope, run the completion checklist, and mark completed with a result and
   verification note. Delegated workers claim their own exact assigned ticket and use a
   distinct worker id; the primary agent remains responsible for integration.
4. **Create every follow-up immediately, without asking.** As soon as you identify an
   unfinished step, open question, known gap, out-of-scope task, or designed-but-unbuilt
   behavior, create its ticket. Do not ask permission, wait, promise to file it later, or
   leave it only in a comment/TODO/note. Reference every follow-up slug in the current
   ticket's completing note, then continue.
5. **Publish at ticket boundaries.** Run required gates, review the diff, make one commit
   for that ticket, include its ticket slug in the commit message, and push before
   beginning the next sequential ticket. Combine tickets only when their implementations
   overlap so strongly that separation would be unsafe or misleading, or when they are
   duplicates; a combined commit message must reference every ticket slug it addresses.
   Integrate parallel tickets separately.
6. **Re-read the queue after every completion.** Concurrent work and new findings can
   change the plan. Continue until no actionable Up Next ticket remains.

**Completion checklist:** finish and verify scope; update required tests, coverage, and
docs; scan for placeholders, TODO/FIXME comments, stubs/mock returns, documented-but-
unimplemented behavior, open questions, and known gaps; immediately create tickets for
every incomplete item; include result, verification, and all follow-up slugs in the
completing note.

Stop early only for an explicit user ticket/time/budget limit, an empty queue, or a
genuine blocker requiring user input or unavailable external state. For that current-
ticket blocker, leave the ticket started and add a `FEEDBACK NEEDED:` note naming the
specific decision or state required. FEEDBACK NEEDED is not deferred-work tracking:
create follow-ups first for independently describable gaps, exhaust safe alternatives,
and continue other independent Up Next work before stopping.

Notes:
- The CLI (`hotsheet-cli …`) and `hotsheet_*` MCP tools use the same engine and work
  without a server.
- Confirm HS2 generation before using connected MCP: `hotsheet-store.json` (directly
  or through `.hotsheet/store`) identifies HS2; `.hotsheet/db/PG_VERSION` identifies
  HS1. If uncertain, use `hotsheet-cli -C <HS2-store>`.
- If a ticket is unclear, do not guess. Record the needed decision, continue independent
  tickets, and return if the answer becomes available.
- Notes support Markdown. For multiline CLI notes, pass real line breaks with
  `hotsheet-cli edit <slug> --note-file <path>` or stdin via `--note-file -`; do not put
  JSON-escaped `\\n` sequences in `--note`, because the CLI stores that argument literally.
