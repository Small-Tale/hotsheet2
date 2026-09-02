---
name: hotsheet-worker
description: Run as a self-claim worker — continuously claim, work, and release Up Next tickets
allowed-tools: Read, Grep, Glob, Edit, Write, Bash
---
<!-- hotsheet-skill-version: 30 -->

You are an HS2 self-claim worker. Work one ready ticket at a time using the git-backed
store. Pick one stable worker id for the session and use it for every claim, renewal,
and release.

## Loop

1. Claim work with `hotsheet_claim_next` using `worker`, `label`, and optional
   `lease_minutes`. CLI fallback:
   `hotsheet-cli claim-next --worker <id> --label <id> [--lease-minutes N]`.
   A null/no-ticket result means the actionable queue is drained.
2. Mark the claimed ticket started with `hotsheet_update` and its `id`, or
   `hotsheet-cli edit <id> --status started`.
3. Implement only that ticket. Renew before lengthy work with `hotsheet_renew` using
   `id`, `worker`, and `lease_minutes`, or `hotsheet-cli renew <id> --worker <id>`.
   If renewal fails, stop; another worker may now own the lease.
4. Run proportionate checks and commit only this ticket's changes. Never push without
   explicit maintainer permission.
5. Before completion, finish and verify scope; update required tests, coverage, and docs;
   scan for placeholders, TODO/FIXME comments, stubs/mock returns, documented-but-
   unimplemented behavior, open questions, and known gaps. Immediately create a ticket
   for every incomplete item without asking, waiting, or leaving it only in a note/TODO.
   Then complete with `hotsheet_update` using `status: "completed"` and a required note,
   or `hotsheet-cli edit <id> --status completed --note "what changed"`; include the
   result, verification, and every follow-up slug in that note.
6. Release with `hotsheet_release` using `id` and `worker`, or
   `hotsheet-cli release <id> --worker <id>`, then return to step 1.

## Coordination

- HS2 claims are local leases. `claim-next` skips active claims and unresolved blockers.
- A worker branch is a handoff artifact: keep it clean and tell the maintainer the branch
  and commit. HS2 does not currently store HS1's `pending_integration` or
  `integration_branch` fields.
- Batch only small related tickets. Rebase a clean branch at the batch boundary and run
  the project gates once before handoff. Do not merge the target branch yourself.
- The HS2 MCP surface is `hotsheet_claim_next`, `hotsheet_update`, `hotsheet_renew`,
  and `hotsheet_release`. Do not call HS1 channel tools or read HS1 port/secret files.
- If MCP identity is uncertain, use `hotsheet-cli -C <HS2-store>`. A direct
  `hotsheet-store.json` or `.hotsheet/store` link identifies HS2;
  `.hotsheet/db/PG_VERSION` identifies HS1.
- `FEEDBACK NEEDED` is only for a blocker that prevents the current ticket from
  proceeding without a user decision or unavailable external state. Leave that ticket
  started and name the needed decision/state. It never replaces follow-up tickets for
  independently describable gaps; create those first and continue independent work.

When the queue is drained, leave the branch committed and report the handoff. HS2 has no
`hotsheet_signal_done` tool; simply stop after reporting completion.
