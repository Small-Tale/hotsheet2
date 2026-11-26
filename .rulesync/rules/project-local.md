---
root: true
targets: ['*']
description: 'Hot Sheet 2 project-specific guidance'
---

# Hot Sheet 2 project guidance

Read this generated root before changing code. The shared Hot Sheet workflow
section below is binding; for queue planning and command recipes read
`.agents/skills/hotsheet/SKILL.md` and the relevant role skill in
`.agents/skills/` (Claude mirrors in `.claude/skills/`). Their sources are
`plugins/{codex,claude}/SKILL.md` and `plugins/shared/role-skills/`, not the
installed copies. Use an equivalent available tool when a skill names a
provider-specific capability. `guidance/README.md` identifies the pinned shared
rule source; HS2-JQAWX6 records the rule selection and instruction budget.

## Product and architecture

A checkout id is a readable Hot Sheet path identity (`folder-shortpathhash`),
not a Git id, ticket-store id, or secret. Checkout-to-store links are many-to-many;
only server instance data carries bearer credentials. Ticketing is
provider-neutral: Markdown/Git, GitHub Issues, Jira, and GitLab retain their
own authoritative records. Route by qualified `(connection_id, native_id)`;
expose unsupported operations; cross-provider copy/move requires user initiation,
an operation id, and source provenance.

Every graphical bootstrap or recovery path needs an idempotent headless CLI path
using the same underlying workflow, including source links, shared skills, MCP,
and AI-tool setup. If remote creation cannot be automated safely, print exact
provider-neutral next steps.

The Tauri client uses Kerf for state, routing, lists, API resources, rendering,
and event delegation. The Kerf and UI rules below apply with `clients/web` as
the installing package. The consumer catalog is
`clients/web/ai/component-catalog-extension.json`; project decisions and
deviations are in `docs/ui-design-philosophy.md` and
`docs/design-guidelines.md`. Kerf upgrades bump all four pinned packages
together, regenerate the catalog, and run the full root check.

Web Awesome Pro needs a separate license decision; see
`docs/09-technology-decisions.md` and `spikes/kerf-webawesome/`.
Its controls emit host-level `input` and `change`; lifecycle events keep
`wa-` prefixes. List and column tickets share one responsive summary contract.
App component styles live beside their modules and in
`clients/web/kerf.components.json`; regenerate the catalog after changes.
Use official Lucide assets through the shared icon component; ask the maintainer
when metaphors fit equally well. State updates use WebSockets or genuine long
polling with cursor replay and bounded backoff, never fixed-interval network
polling; test idle request rate and reconnection. Ticket editors never write
while typing (HS2-RE1PS6); follow `docs/ui-design-philosophy.md` and cover
rapid coalescing, focus moves, edits after save, and reload recovery.

Visible changes follow the Visual QA and UI Evaluation rules below;
`docs/manual-test-plan.md` owns manual UI checks. Use production components in
`/ux-demo`, and extend each changed component's demo for new states. Exercise
child actions through the shipped parent in E2E tests. Keep application logic
in pure modules with sibling tests; no import-time listeners, timers, or
polling. See `docs/12-code-organization-and-testing.md`.

## Ticket verification and push policy

Push policy: `push-per-ticket`. The maintainer has authorized routine
fast-forward pushes of verified ticket commits to `origin/main`; do not ask
again. Preserve local edits in a clean worktree; the Git Commit and Push rule
below owns commit shape and history safety. Use `scripts/check.sh --plan` to inspect affected checks and
`scripts/check.sh` at the ticket/integration boundary. `scripts/push.sh`
requires an exact-tree receipt outside the explicit deviations below. After a
push, confirm the remote SHA and covering `main` CI with
`node scripts/ci-gate.mjs status --covers <full-sha>`; keep the ticket in
`final_testing` until its gate is green. A Platforms run is never a ticket's
CI gate. The workflow skill and `docs/29-affected-local-verification.md`
own command details and risk selection.

Tests use isolated homes, data directories, servers, and browser fixtures;
build artifacts once per suite, wait for events or state rather than fixed
delays, and keep output quiet. A flaky test needs a ticket and a retries-off
load reproduction; quarantine markers name that ticket. Rust and TypeScript
lint and format checks are part of the root gate, with no warnings or silent
suppressions. See `docs/test-quarantine.md`,
`docs/12-code-organization-and-testing.md`, and the package lint configs.

Set exactly one `tier:T0`, `tier:T1`, or `tier:T2` ticket tag when declaring
risk; the tag is authoritative and the preliminary note explains it. Untagged
tickets use T1. T0/T1 use the checked-in five-minute planning budget unless
`deep-testing`, `testing:browser`, `testing:platform`, `testing:perf`, or
`test-budget:<duration>` selects more. T2 has no time cap: analyze risks and
record the selected checks. Outside CI First, `scripts/check.sh` requires
`--test-plan <reviewed.json>`; when other affected checks exist, T2 also
requires `--full` or an analyzed `--t2-stage` selection. CI First's
budgeted run needs neither flag. Fix checks that ran and failed; name deferred
checks and the green covering main CI run in the completion note. See
`docs/29-affected-local-verification.md`; create a prioritized bug if a
missed local selection lets a defect escape.

At session start and safe ticket/integration boundaries, read
`hotsheet-cli coordination --project . development-strategies` and
`development-strategy-resolve`. User-selected strategy overrides ticket,
session, then project strategy; pass it as the highest-priority `--ticket`
override and record its provenance. An unset project retains the default
integration mode and risk-selected checks. Re-resolve changes only at a safe
boundary. If the resolver is missing, use the supported headless upgrade path
before acting. The policy controls the gate and branch; never silently waive
ticket/integration verification or green-main promotion. See
`docs/34-development-strategies.md` (HS2-F9062F).

CI First (HS2-ZGS7JP) applies only when the resolver reports `ci-first` and
`push_without_receipt: true`. Run `scripts/check.sh` once per ticket within
its budget, including T2; fix failures and leave unstarted checks to CI.
`scripts/push.sh` may then push without a full-gate receipt but retains a clean
checkout, main-writer reservation, fast-forward, and remote confirmation.
Watch the covering `main` CI run. Attribute red CI to tickets since the last
green run, fix forward, and report it in the ticket and session before completion.
Other strategies still require the receipt.

The urgent standalone fast-push override (HS2-Q390N2) requires the maintainer's
explicit CI-based verification acceptance: `scripts/push.sh --standalone
--skip-tests`. It still requires a clean checkout, main-writer reservation,
fast-forward, remote SHA confirmation, and covering `main` CI. Record why in
the ticket; the override is unavailable to integrators, staged pushes, or
promotion. Batch (HS2-9G0M3V) and CI-staged (HS2-GT57BQ) deviations require
the maintainer's enabled strategy. Batch integrates non-overlapping reviewed
handoffs behind `scripts/check.sh --smoke` and one covering main CI run.
CI-staged gates each staged SHA on its integration branch; `main` advances
only by `scripts/push.sh --promote` to a green SHA. Never merge, rewrite, or
force-push main. See `docs/34-development-strategies.md` for recovery.

A `final_testing` prerequisite normally blocks dependents. Provisional work
requires the committed speculation policy, a reachable CI gate SHA with an
undecided run, and an assignment reservation; `speculate:never` opts out.
Its own gates remain mandatory. A red base gate requires repair, revalidation,
and reopening invalidated work; do not complete unconfirmed provisional work.
Automatic `claim-next` and `plan-dispatch` remain confirmed-only.
Use the CI watch and `speculation-sync --wait-ms`, never polling. See
`docs/05-ai-tool-plugins.md` §5.9.1 (HS2-2SJPCH).

## Local deviations and references

Hot Sheet 2 has no hosted deployment target. The shared deploy-script rule is
inapplicable; do not add a placeholder. Revisit after HS2-VXQNVM chooses a
hosted target; HS2-BYXA4V owns distribution. See `docs/01-architecture.md`
§1.7 (HS2-51HMCC).

## gitgist commit drafting (HS2-VA97HW)

`gitgist` is a pinned root dev dependency. `npm run commit:msg` drafts from
the staged diff using signed-in Claude, then Codex; no API key is needed.
`npm run commit:msg:offline` lists staged files without provider use.
Rewrite drafts into slug-first subjects after reviewing `git diff --cached`.
Settings live only in `gitgist.config.json`. Its exclusions omit diff bodies
from generated/imported files; their sources remain included:

| Pattern                                               | Why                                                                    |
| ----------------------------------------------------- | ---------------------------------------------------------------------- |
| `AGENTS.md`                                           | Rulesync output; its source is `.rulesync/rules/project-local.md`      |
| `.agents/skills/*`                                    | Codex mirrors of `.claude/skills` written by Hot Sheet setup           |
| `plugins/*/instructions.md`                           | Pinned ai-playbook workflow snapshots (`scripts/sync-ai-playbook.mjs`) |
| `plugins/shared/ticket-driven-work.md`                | Pinned ai-playbook rule snapshot                                       |
| `plugins/shared/testing-philosophy.md`                | Pinned ai-playbook rule snapshot                                       |
| `plugins/shared/requirements-documentation.md`        | Pinned ai-playbook rule snapshot                                       |
| `plugins/shared/visual-qa.md`                         | Pinned ai-playbook rule snapshot                                       |
| `clients/web/ai/component-catalog-extension.json`     | Generated by `npm run catalog:generate`                                |
| `clients/web/ai/component-composition.generated.json` | Generated by `npm run catalog:generate`                                |
| `docs/design/exports/*`                               | Sketch exports, including multi-megabyte SVG text                      |

`docs/CODEBASE-MAP.md` maps the codebase; `docs/README.md` tracks
requirements; `docs/TEST-COVERAGE.md` and `docs/manual-test-plan.md` track
coverage. Rust uses `cargo nextest run`, web tests run in `clients/web`,
migrator tests in `migrator/`; prefer `node scripts/test-quiet.mjs`.
Terminal sizing, merge/sync, claims, indexes, auth, migration, and shared
guidance require T2 transition coverage.

`AGENTS.md` is generated. Edit this rule or
`guidance/rulesync/project-selection.json`, then run
`hotsheet-cli guidance --project . apply` or pinned Rulesync; never edit the
output by hand. `node scripts/check-rulesync-generation.mjs --fetch` checks
byte equality (HS2-VA97HW). Hot Sheet 2 is developed by Small Tale Inc. under
`Small-Tale/hotsheet2`; metadata, licenses, credits, and bundle ids use the
company identity, while commit authors remain individual developers.
