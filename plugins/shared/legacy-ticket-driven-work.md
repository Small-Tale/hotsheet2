<!-- hotsheet:begin section=ticket-driven-work v=3 -->

## Ticket-Driven Work

When the user gives you work directly (not via the Hot Sheet channel or events), create Hot Sheet tickets before starting implementation — especially for substantial or multi-step work.

- **Do create tickets** for: features, bug fixes, refactoring, multi-step tasks, anything changing code. **Don't** for: simple questions, git commits, quick lookups, trivial one-liners. **When in doubt, create them.**
- Create via the Hot Sheet API (prefer the `hotsheet_*` MCP tools), mark Up Next, then work through them: set status `started` → implement → set `completed` with notes.
- **Create every follow-up immediately — without asking.** As soon as you identify unfinished steps, open design questions, known gaps, out-of-scope work, or designed-but-unbuilt behavior, create a follow-up ticket. Do not ask permission, wait for confirmation, promise to file it later, or leave it only in a comment/TODO/note. Reference every follow-up slug in the current ticket's completing note, then continue.
- **FEEDBACK NEEDED is for a blocker on the current ticket, not deferred work.** Use it only when the current ticket cannot proceed without a user decision or unavailable external state. Leave that ticket `started`, add a `FEEDBACK NEEDED:` note with the specific decision or state required, and wait after continuing any independent work. This does not replace a follow-up: create tickets first for every independently describable gap or later step.
- **Completion checklist** — before marking a ticket `completed`: (1) finish and verify its scope; (2) update required tests, coverage, and docs; (3) scan for placeholders, TODO/FIXME comments, stubs/mock returns, documented-but-unimplemented behavior, open questions, and known gaps; (4) immediately create a follow-up for every incomplete item; (5) include the result, verification, and all follow-up slugs in the completing note.
- **UI evidence continuity** — when a UI ticket already contains an image demonstrating the problem or requested design, reproduce that component/state/viewport after the change, attach the resulting screenshot to the ticket, and reference it in the completion note. Do not silently leave the evidence only in a local temporary path; if capture or attachment is genuinely impossible, say exactly why in the completion note.
- **Commit traceability** — every commit for ticket-driven work must include every ticket slug addressed by that commit in its commit message. This applies to both single-ticket and intentionally combined commits.

<!-- hotsheet:end section=ticket-driven-work -->
