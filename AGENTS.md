# Additional Conventions Beyond the Built-in Functions

As this project's AI coding tool, you must follow the additional conventions below, in addition to the built-in functions.

# Hot Sheet 2 project guidance

Read this generated root before changing code. For ticket claims, phases, notes,
handoffs, completion, and queue planning, read `.agents/skills/hotsheet/SKILL.md`
and the relevant role skill in `.agents/skills/`; Claude mirrors live in
`.claude/skills/`. Their sources are `plugins/{codex,claude}/SKILL.md` and
`plugins/shared/role-skills/`, not the installed copies. Use an equivalent
available tool when a skill names a provider-specific capability. Area-specific
documents are linked below; `guidance/README.md` identifies the pinned shared
rule source.

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
and event delegation. Before client work, read the installed
`clients/web/node_modules/kerfjs/ai/skill.md`,
`clients/web/node_modules/@kerfjs/ui/llms.txt`, and
the linked component, interaction, layout, accessibility, and writing guidance
relevant to the change. Use public components and `--kui-*` tokens, not private
classes, parts, tokens, or external sizing. The consumer catalog is
`clients/web/ai/component-catalog-extension.json`; project decisions and
deviations are in `docs/ui-design-philosophy.md` and
`docs/design-guidelines.md`. See `guidance/README.md` for source ownership.
Do not introduce a Kerf workaround for a missing public API: file a KF ticket
and keep the app ticket blocked. Kerf upgrades bump all four pinned packages
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

For visible changes, apply the real-app browser review in
`plugins/shared/visual-qa.md` at wide, medium, narrow, and wide again. Inspect
rendered states, both color schemes, and WebKit for sizing/focus risk. Attach
an after capture for a ticket's problem/design image. If visual capture or
attachment is blocked, leave the ticket open and record the exact blocker.
The installed Kerf docs and `docs/manual-test-plan.md` own further UI detail.

Use production components in `/ux-demo`, and extend each changed component's
demo for new states. Exercise child actions through the shipped parent in E2E
tests. Before composing a screen, name each region's owning public component
and read the installed Kerf interaction conventions. Keep application logic in
pure modules with sibling tests; no import-time listeners, timers, or polling.
See `docs/12-code-organization-and-testing.md` and
`docs/ui-design-philosophy.md`.

## Ticket verification and push policy

Push policy: `push-per-ticket`. The maintainer has authorized routine
fast-forward pushes of verified ticket commits to `origin/main`; do not ask
again. Preserve local edits in a clean worktree. Stage named paths, review the
staged diff, commit once per ticket with a slug-first subject, and never rewrite
shared history. Use `scripts/check.sh --plan` to inspect affected checks and
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

<!-- hotsheet:begin section=conventions-as-code v=2 -->

## Conventions as Code

Instruction-file prose does not hold on its own. When a reviewer corrects a convention, encode it in the same change as a source-level test or a lint rule, and name the test or rule with the ticket slug so the reason stays discoverable.

- **Policy tests.** Keep one structural test file per package that greps or parses source to pin conventions: no emoji or Unicode glyph icons; no cross-component CSS selectors; one exported component per file; every export has a demo; every signal has a reset; no handwritten boundary types outside the contract module; no machine-local paths; no invisible Unicode. The same file pins the instruction files: each managed section marker (`hotsheet:begin section=…`, `hotsheet-instructions-version`) occurs once and at one version, `CLAUDE.md` contains only the `@AGENTS.md` import, and, when the project uses Rulesync, the committed instruction file equals a dry-run regeneration. Add a case for each new correction; never delete one without the ticket that retires the convention.
- **Suppressions.** A lint disable carries a reason and a ticket slug on the same line, and a policy test caps disables per file. Unused disables fail lint. Formatter-ignore comments are banned.
- **Doctor and profile exceptions.** UI library doctor exceptions are exact `{id, rule, target, rationale}` entries, each naming the upstream ticket; a test pins their count and asserts each rationale's target still exists. The count only decreases. No broad suppression or raised budget substitutes for a fix; a missing library capability is an upstream ticket, not an exception.
- **Generated artifacts** (catalogs, composition manifests, API contracts, lockfiles) are checked in and byte-checked in CI; regenerate them in the same change that changes their source.

<!-- hotsheet:end section=conventions-as-code -->

<!-- hotsheet:begin section=customer-data-hygiene v=1 -->

## Customer Data Hygiene

Never copy customer documents, messages, names, or figures into tickets, notes, commit messages, logs, test fixtures, demo data, or telemetry. Record ids, counts, sizes, timings, and shapes instead. Reproduce a customer-reported defect with synthetic data that matches the shape, and state in the ticket that no customer content was copied. Observability events carry identifiers and metrics only. When a capture would show customer content, use demo data or redact before attaching.

<!-- hotsheet:end section=customer-data-hygiene -->

<!-- hotsheet:begin section=sandbox-recovery v=1 -->

## Launched-session sandbox recovery

When a Hot Sheet-launched session's `hotsheet-cli` command fails while
opening the ticket store with `Operation not permitted (os error 1)`, use
the configured `hotsheet_*` MCP tools for that store operation. A sibling
`<repo>.hs2` store may be outside the command sandbox. Keep the same claim,
actor, note, and completion workflow through MCP; do not infer store corruption
from this error alone.

If a Chromium or WebKit browser gate fails to launch with EPERM or a mach-port
denial, rerun that gate through approved unsandboxed execution. If approval or
a browser is unavailable, record the gate as blocked; do not count it as
passed (HS2-DVZXSD).

<!-- hotsheet:end section=sandbox-recovery -->

<!-- hotsheet:begin section=requirements-documentation v=4 -->

## Requirements Documentation

Keep human-readable requirements as the source of truth for product behavior. Update them in the same change that adds, removes, or changes behavior. Create and cross-link documents for major new functional areas.

Maintain two concise AI-readable synthesis documents at project-specific, repository-relative locations:

- A codebase map of directory structure, entry points, schema, settings, build and test paths, and where to find important behavior. Update it when structure or contracts change.
- A requirements summary covering each requirements document and marking what is shipped, partial, design-only, or deferred. Update it when requirements or implementation status change.

If either document is absent, create a minimal, truthful version in the same change that first needs it; link it from the project's documentation index and record its relative path in local `hotsheet:specifics` guidance. Setup and refresh install these instructions but do not invent a map or product status. Preserve any existing document locations and prefer targeted updates over rewrites. Source requirements documents and code win if a summary conflicts with them.

Any project with a rendered interface keeps a UI conventions document. It points at the design system's own guidance for philosophy and interaction conventions and records only the project's deviations and project-specific decisions. Cite ticket slugs for decisions in prose and code comments; use one name per concept and keep a glossary once terms accumulate; render diagrams rather than drawing them in text; record performance baselines and audits as dated documents.

<!-- hotsheet:end section=requirements-documentation -->

<!-- hotsheet:begin section=testing-philosophy v=8 -->

## Testing Philosophy

- Declare risk tier in preliminary thoughts (claim note for trivial work); reviewers may raise it. **T0 trivial**: copy/docs/constants/one style token or config value; lint, typecheck, directly related tests, visuals when visible. **T1 normal**: affected tests; a fix adds a regression at the natural layer, a feature gets unit and E2E coverage. **T2 high-risk**: merge/sync, claims/leases, indexes, formats/parsers, shared components, auth/credentials, migrations and project-named risks; all requirements below plus full affected suites.
- Cover each feature with isolated unit tests (external dependencies mocked) and real-flow E2E with minimal mocking. Fakes match real shapes, optional fields and status codes. Newly composed APIs need real-server exercises as well as mocked-client tests.
- Aim for all documented features and 100% line/branch coverage; execution is not proof of behavior/sequences. Enumerate state/transition matrices; test repeated, interleaved, out-of-order, reset and empty-then-refill sequences.
- Test controls both ways: user control → app state/output and programmatic state → live controls; reset, then edit again. Assert custom-element live properties, focus/events and all derived icons/badges/counts. Exercise child actions through shipped parents; isolated demos cannot prove wiring. Explicitly disable unsupported controls.
- Maintain feature-by-feature unit/E2E/manual evidence as behavior changes and a manual plan for what cannot reliably be automated.
- Every new code package has working local lint/config. Fix lint/type errors before finishing; keep warning-free and document justified suppressions. Reviewer convention corrections get a source policy test or lint rule named with the ticket slug in the same change.
- Keep output quiet: use failure-only reporters or the project's quiet wrapper; read summaries/failure excerpts, not full logs. A background agent may report pass/fail and failures only.
- **Rerun only what failed.** Target failing tests until green, then run the affected set once. After rebase rerun only overlapping affected areas; unrelated commits need no rerun. Never repeat a passing suite just to double-check.
- Verify the rebuilt instance using the user's actual steps/command; never test against the user's running host or a stale build. `test-operations` owns isolation, build-once and flake policy when installed.

<!-- hotsheet:end section=testing-philosophy -->

<!-- hotsheet:begin section=ticket-driven-work v=9 -->

## Ticket-Driven Work

The Hot Sheet workflow owns lifecycle, claims, phases/ETAs, follow-ups, feedback and completion (HS2-B39GQ9). This rule adds:

- Only a live claim signals activity; `claim_count` counts past attempts.
- Before feedback, check open tickets and code. Status reports, release waits and sandbox blockers are regular notes. Close tickets you created yourself; show visual tradeoffs with captures and `CHOICE` options.
- Re-read the whole ticket and new notes before every action; quote the note acted on in your result.
- Preserve scope and neighboring behavior/appearance; ticket collateral changes separately. Open named attachments, reference art, prior implementations and other projects before designing.
- Measure before performance/architecture restructuring; record the justification and close as not worth it when the number is small.
- Record `activity` at meaningful subtask start/finish, including repeated/reversed transitions. Keep conclusions/decisions/recommendations in regular Markdown notes, optionally linked from activity.
- AI activity writes include a plain one-line outcome `note_summary`, preferably under 80 characters; details belong in the body.
- Multiline CLI notes use `--note-file <path>` or `--note-file -` with real newlines; `--allow-literal-backslash-n` is only for intentional literal escapes.

<!-- hotsheet:end section=ticket-driven-work -->

<!-- hotsheet:begin section=visual-qa v=2 -->

## Visual QA

For any change that can affect a rendered interface, inspect the real application (not a demo route) on the rebuilt instance in a browser at representative wide, medium, and narrow sizes, including the wide-to-narrow-to-wide transition, before completing the ticket. Exercise the affected states and transitions, then review readability, clipping and overflow, alignment, spacing, typography, contrast, icons, responsive behavior, and consistency with adjacent components. Fix defects, rerun affected behavioral checks, and inspect the corrected result. DOM, accessibility, style, and geometry assertions support this review but do not replace looking at the rendered page. A capture that shows a visible defect is never correctness evidence; fix, rerun, and recapture before attaching.

If the ticket has a problem or design image, capture the corresponding after state at the same component and viewport as closely as practical. Attach the image to the ticket and name it in the completion note. Screenshots supplement behavioral tests. If browser capture or attachment is unavailable after practical attempts, describe the exact blocker and leave the visual ticket open for review rather than claiming visual validation.

<!-- hotsheet:end section=visual-qa -->
