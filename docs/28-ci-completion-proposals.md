# CI completion proposals (HS2-1C1565)

`scripts/ci-completion-proposal.mjs` prepares a local, durable proposal for a single coordination handoff. It reads the current Hot Sheet ticket and coordination snapshot, Git, the root-gate receipt, and GitHub Actions. It does not claim or complete tickets, mutate the coordination pool, push, remove worktrees, or poll the network. The integrator remains the authority for those transitions.

## Run it

After integrating a code handoff onto main, passing `scripts/check.sh` on that exact clean tree, pushing, and recording the coordination `Ci` phase:

```sh
zsh -ic 'node scripts/ci-completion-proposal.mjs register --project <main-checkout> --store <ticket-store> --assignment <operation-id> --integrated <full-main-sha> --worktree <source-worktree>'
```

For a findings-only handoff in `Verifying`, omit `--integrated` and `--worktree`. If a previous `## Review return` finding has been resolved, pass its exact note id as `--resolved-review-note <id>`; repeat for each resolved finding. Registration stores the current ticket revision, worker and integrator generations, handoff evidence, and the reviewed source-to-main mapping in the Git common directory under `hotsheet-ci-proposals/<operation-id>.json`. It does not copy ticket prose into that file. Re-register after a changed ticket, session, or integrated commit has been reviewed.

When CI changes, run this command explicitly, including after a restart:

```sh
zsh -ic 'node scripts/ci-completion-proposal.mjs refresh --project <main-checkout> --assignment <operation-id>'
```

`refresh` returns exit 0 only when the proposal is ready; exit 2 means the JSON lists outstanding reasons; exit 1 means an invocation or local evidence error. It writes only when the proposal's meaningful evidence changes. There is no CI wake subscription in this tool. If the integrator lacks an external wake, rerun `refresh` after a GitHub notification or invoke it at the point of completion; do not install fixed-interval polling. `scripts/ci-gate.mjs status --covers <sha>` remains the direct human check for the latest green main run.

## Evidence and limits

For code, readiness requires the exact source handoff, identical commit or stable patch id mapping to the recorded main commit, a root-gate receipt for that commit's tree, ancestry against a fresh `git ls-remote origin refs/heads/main` result, a successful covering **main push** CI run, an unchanged ticket revision, no unresolved Review return note or active claim, and the exact clean source worktree at its recorded commit. The selected store's canonical connection id and the ticket's native id must match the assignment, so a same-slug ticket from another store cannot approve completion. The source worktree cannot be the main checkout. CI evidence retains run ids, URLs, pending/failure/cancelled/superseded/unavailable states, and each run's ancestry result. A green source-branch run is ignored. A returned `Blocked` operation stays blocked, including after a later success. Findings-only handoffs require the authoritative findings note and current ticket but do not invent a code gate, CI run, or source worktree.

The collector reads up to 100 recent main push CI runs on each explicit refresh and queries the actual remote main ref; a stale local `origin/main` is not sufficient. If GitHub or the Git remote is unavailable, the run is outside that window, local Git lacks the remote commit, or the authoritative provider ticket cannot be read, it reports `unavailable` and does not approve completion. Fetch `origin/main` and refresh when ancestry cannot be proved. An independent gate failure during integration belongs to the `suspended_integration` / prerequisite path from HS2-2D3099; this proposal tool cannot turn it into CI. It never upgrades a live coordination pool.

The JSON includes the ticket's current scope hash and a command to inspect that scope, provider-qualified follow-up ids, Review return note ids, confidence factors, and exact worktree/branch cleanup targets. An integrator inspects the current ticket and proposal, runs `refresh` immediately before the existing completion and `coordination mark --phase done` workflow, then removes only the listed clean worktree after resolution. Changed notes, dirty worktrees, new source commits, and session replacement invalidate the proposal. Since external state can change between separate commands, the integrator still checks the ticket, claim and commit at the action boundary.

## Measurement

The proposal reports handoff-to-CI and handoff-to-completion spans from durable assignment transitions and the ticket completion timestamp when those facts exist; missing or reversed times remain unknown. HS2-34PWSK found no observed main-gate spans among 55 historical handoffs, so there is no defensible production before/after throughput or handoff-to-completion delta yet. The synthetic real-process fixture for this tool performs one registration and two refreshes, including a stale-scope rejection, in about 1.2 seconds on an initial 2026-10-28 development-host run and 26 seconds during a loaded rerun; those samples measure command overhead and host load, not production throughput. A later production audit can compare proposal refresh time and completed-ticket spans once real post-rollout events exist.
