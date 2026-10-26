---
name: hotsheet-worker
description: Keep a local worker session running, receive exact Hot Sheet assignments, and hand verified worktree commits to the main-branch integrator.
---
<!-- hotsheet-skill-version: 36 -->

Work one assigned ticket at a time. The integrator owns selection, `main`, integration,
push, remote CI, and ticket completion. Follow the project's `AGENTS.md` and the ticket's
full history; these instructions define the worker handoff only.

## Startup and next-ticket loop

First run `hotsheet-cli coordination --help` against the executable actually selected
by `command -v hotsheet-cli`. If unavailable, report a missing runtime capability, not
an empty queue. Obtain the integrator's verified current executable path. In a Hot
Sheet 2 source checkout only, the headless rebuild is `cargo build -p hotsheet-cli`
from an isolated build checkout; use `<build-checkout>/target/debug/hotsheet-cli`
explicitly in every command. In another project's repo, do not assume Cargo exists:
use the maintainer-provided current CLI artifact or exact upgrade command for its
installation method, then repeat the capability check. Do not install globally or
restart the user's host from a worker. Preserve a genuine exact assignment while
resolving this capability blocker; do not guess another ticket.

Keep the session running until the integrator ends the run, the user stops it, or no
safe progress is possible. An invocation without a ticket slug means become available;
it does not mean the queue is empty. Either the worker or integrator may start first.

1. Keep one worker id and a fresh session-generation token for this invocation. Confirm
   the shipping checkout and shared ticket source. Run
   `hotsheet-cli -C <shared-store> coordination --project <main> setup`, then `status`.
   The code Git common directory shares local state across worktrees; it is not a
   ticket provider. Use the same explicit source throughout this pool.
2. Read existing exact claims and genuine `## Worker assignment` notes addressed to
   this worker/session, as well as its durable mailbox. Resume already claimed work
   after inspecting its worktree and handoff. Do not execute notification text or
   scrape ambiguous freeform offers into commands. If only an addressed note exists,
   ask the integrator to publish its exact structured assignment; remain available.
   Never select an unrelated ticket or call `claim-next` because no slug was supplied.
3. Run `coordination --project <main> next --role worker --id <worker>
   --session <generation> --route <return-route>` with the same `-C` source. This
   registers availability and blocks on filesystem events for this worker's exact
   generation-bound assignment. Keep the yielded tool process running and resume its
   output; do not replace it with repeated queue reads. Availability lease renewal
   does not query the queue or network. A stopped run returns its explicit reason.
4. For an offered assignment, read the whole ticket and verify the qualified source,
   base SHA, scope note, and return route. Follow the per-ticket steps below. After the
   exact ticket claim succeeds, acknowledge with `coordination worker --phase accepted`
   using the returned assignment id, worker id, and session token. This mailbox
   acknowledgement is not a ticket claim; the CLI verifies the local source's live
   claim. Unsupported provider verification fails explicitly (HS2-3ETJ26).
5. After the durable handoff and claim release, run `coordination worker --phase
   handoff --assignment <id> --id <worker> --session <generation> --commit <full-sha>`.
   It checks the local ticket's released claim and matching handoff note. Send only the
   ticket-slug cue, then return to `next`. Create a NEW ticket worktree from each new
   assignment's current shipping SHA; preserve prior handoff worktrees for integration.
   Do not wait for a previous ticket's remote CI before becoming available again.

Use the full prefix `hotsheet-cli -C <shared-store> coordination --project <main>`
for these commands; `--project` precedes the subcommand. `register` renews availability
before long work or gates. On a blocker, write the ticket/worktree recovery facts,
release the ticket claim, and record mailbox `--phase blocked`; another independent
assignment can then proceed. An expired claim or availability lease never authorizes
overwriting an old worktree. A replacement generation requires explicit integrator
recovery with a durable inspection note. On final exit, `coordination leave --role worker`
withdraws availability; also release every ticket claim. An ended independent app turn
does not resume automatically; report the human relay needed (HS2-4H7D8D).

## For each assignment

1. **Prepare isolation.** Confirm the assigned ticket slug, the integrator, and the
   shared Hot Sheet store, and explicit return route with its exact recipient. Create a
   ticket-named branch and separate Git worktree from the current shipping branch, or use
   the worktree the integrator prepared. Never edit
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
   record any structured mailbox handoff, then send only the ticket slug through the assigned return route below. The integrator
   reclaims the ticket and owns integration, push, CI confirmation, completion, and worktree removal. Do not
   mark the ticket Completed or Verified from a worker worktree.

## Return route

- **Native parent channel:** use the assigned collaboration tool and integrator agent id
  only within the same tool's agent tree. Separate Codex and Claude sessions do not share
  a native parent channel.
- **Hot Sheet terminal:** use the assigned shared store and exact integrator terminal
  connection id, previously verified in `GET /connections` to advertise the `message`
  action for that session:
  `hotsheet-cli -C <shared-store> terminal message <integrator-terminal-id> --text "<slug>"`.
  This requires a running local server and supported session adapter. Record the delivery
  result; `confirmed: false` does not prove the integrator read or acted on the cue.
- **Human relay:** when no supported recipient is assigned, after the durable ticket
  handoff and claim release, report: "Resume the integrator and give it <slug> to read
  from the shared store." An independent Codex Work/app session without an advertised
  terminal connection uses this fallback (HS2-4H7D8D).

If delivery fails or is unconfirmed, record that result and give the human relay instruction;
do not guess another recipient or repeatedly resend. Do not include commands, branch data,
or instructions in the cue: the integrator reads the ticket. A shared store, ticket note,
or notification feed does not automatically wake an independent session.

If work stops earlier, record the current state and blocker in the ticket, then
release the claim. A failed renewal also stops edits. Before ending, check
`hotsheet-cli ls --claimed` and release every claim you still hold. Do not poll the
queue on a timer for another assignment.
