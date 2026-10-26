---
name: hotsheet-worker
description: Implement an assigned Hot Sheet ticket in an isolated Git worktree and hand its verified commit to the main-branch integrator.
---
<!-- hotsheet-skill-version: 34 -->

Work one assigned ticket at a time. The integrator owns selection, `main`, integration,
push, remote CI, and ticket completion. Follow the project's `AGENTS.md` and the ticket's
full history; these instructions define the worker handoff only.

1. **Prepare isolation.** Confirm the assigned ticket slug, the integrator, and the
   shared Hot Sheet store. Create a ticket-named branch and separate Git worktree from
   the current shipping branch, or use the worktree the integrator prepared. Never edit
   the integrator's checkout. Link the worktree to the same ticket store and set up its
   AI-tool integration if needed; do not create a second ticket store. Keep build and
   test state inside the worktree. If the assigned ticket overlaps another worker's
   files or an existing dirty worktree, tell the integrator and wait for a different
   assignment.
2. **Claim exactly that ticket** with `hotsheet-cli claim <slug> --worker <id> --eta
   <duration>` or the equivalent Hot Sheet MCP operation. Use `HOTSHEET_WORKER_ID` when
   set; otherwise choose one stable session-specific id. Read the whole ticket,
   including new notes, before each action. If someone else owns its live claim, do
   not edit it. Renew before the lease expires and before lengthy work. Record phase
   transitions and activity as the project instructions require.
3. **Implement and verify only its scope.** Record preliminary thoughts for non-trivial
   work. Update affected tests, coverage, and docs; create follow-up tickets for gaps.
   Run the required local gates and inspect the diff. Stage named paths and commit the
   verified change on the worker branch with the ticket slug in the subject. Do not
   merge or push the worker branch or `main` as part of this handoff.
4. **Hand off without completing.** Add a regular ticket note headed `## Integration
   handoff` with the branch name, full commit SHA, worktree location labeled as
   machine-local, gate results, unresolved risks, and any follow-up slugs. State that
   the ticket remains Started pending integration. Release the claim immediately and
   notify the integrator of the ticket slug. The integrator reclaims the ticket and
   owns integration, push, CI confirmation, completion, and worktree removal. Do not
   mark the ticket Completed or Verified from a worker worktree.

If work stops earlier, record the current state and blocker in the ticket, then
release the claim. A failed renewal also stops edits. Before ending, check
`hotsheet-cli ls --claimed` and release every claim you still hold. Do not poll the
queue on a timer for another assignment.
