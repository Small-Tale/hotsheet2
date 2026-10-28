# Scoped handoff readiness (HS2-NNSWDS)

The optional headless `hotsheet-cli coordination preflight` command checks a worker's
reviewed handoff before it becomes durable. It reads the selected local Markdown/Git
ticket source and the assignment in the code project's coordination registry. Other
ticket providers return an explicit unsupported result. `--publish` repeats the cheap
preflight, writes one regular handoff note and releases the exact live claim in a
single ticket transaction, then acknowledges mailbox Handoff. The integrator retains
review, integration, CI and completion authority. A real blocker uses the existing
blocked-report path.

The worker first records the actual `initial_testing` phase, commits and gates code
on a clean worker tree, then writes a machine-local JSON evidence file. The command
is read-only without `--publish`:

```sh
hotsheet-cli coordination preflight --id WORKER --session GENERATION \
  --assignment OPERATION --evidence-file /path/to/readiness.json
hotsheet-cli coordination preflight --id WORKER --session GENERATION \
  --assignment OPERATION --evidence-file /path/to/readiness.json --publish
```

Common fields are `ticket: {connection_id, native_id}`, `claim_attempt` (claim
event id), `ticket_updated_at`, `latest_note` (note id or null), `scope` (the exact
assignment array), `instructions_reviewed`, `docs_reviewed`, `coverage_reviewed`,
`ui_affecting`, and `unresolved_findings: [{id, follow_up}]`. Every listed follow-up must exist in
the local source. The reviewed ticket timestamp and last note must match the latest
ticket; a later note requires fresh review. The three review booleans are worker
attestations, not automatic proof.

For code, set `kind: "code"`, `worktree`, `branch`, `commit`, `tree`, and `risk_tier`
(`t0`, `t1`, or `t2`). Commit and tree are full 40-character SHA-1 values. Preflight
compares the reported branch/tree/commit to the clean worktree, checks the same Git
common directory and assignment-base ancestry, validates declared changed paths and
sections, and requires the root gate marker for that exact tree. The marker records
the local `scripts/check.sh` receipt; it does not turn a self-chosen tier into
verified risk classification. A changed commit or tree requires a new gate and
preflight.

For findings, set `kind: "findings"` and `note` beginning with `## Findings handoff`
followed by substantive content. No commit or code gate is required.

Rendered web source changes and work marked `ui_affecting: true` require
`ui: {real_parent, states, viewports,
inspected_rendered_page, after_attachment}`. The viewports must include `wide`,
`medium`, `narrow`, and `resize`; the worker must inspect the real application and
affected states. If the ticket has a reference or problem attachment, the matching
after image must be attached as correctness evidence and its id supplied. The command checks
the attachment id, purpose and explicit inspection statement; visual correctness itself
remains the worker's attestation and the integrator's review.

The report marks objective facts `verified` and worker judgments `self_attested`.
Failures are `missing`, `stale`, or `unsupported` with a repair. An exact evidence
file can be replayed after publication without a second note or release. A worker
becomes available for independent work as soon as this durable released handoff
exists; it does not wait for the integrator or CI. Older runtimes use the manual
note, release and mailbox Handoff steps in the shared worker skill.

## Measurement

The frozen HS2-34PWSK cohort found 11 matched review returns among 55 matched
handoffs (20%). It did not classify all omission causes. The HS2-K1QNGC live
rollout will record counts of preflight failures by classification, repairs before
publication, published handoffs and downstream review-return rounds, then compare
the matched return rate and latency to that bounded baseline. This implementation
has synthetic transition tests, not live post-rollout observations; no throughput
improvement is claimed yet. Record identifiers and metrics only, without ticket
content or customer data.
