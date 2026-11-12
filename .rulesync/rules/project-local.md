---
root: true
targets: ['*']
description: 'Hot Sheet 2 project-specific guidance'
---

# Hot Sheet 2 project guidance

Read the generated `AGENTS.md` before changing code. Project workflows are
installed in `.claude/skills/` and mirrored for Codex in `.agents/skills/`; both are
generated. Edit their sources instead: role skills in `plugins/shared/role-skills/`
(`scripts/sync-role-skills.mjs`) and the Hot Sheet skill in
`plugins/{claude,codex}/SKILL.md` (installed by Hot Sheet setup). Use the
equivalent available tool when a shared workflow names a tool-specific capability.
Keep durable project additions in this local Rulesync rule and provider
configuration in its provider directory.

## Identity and provider boundaries

A checkout id is a readable Hot Sheet path identity (`folder-shortpathhash`), not a
Git id, ticket-store id, or secret. Checkout-to-store links are many-to-many. Only
server instance data carries bearer credentials.

Ticketing is provider-neutral. Markdown/Git is the default and fullest-featured
provider, but no project must use it. GitHub Issues, Jira, and GitLab remain their
own authoritative systems; do not mirror them bidirectionally into Git. One code
project can connect several sources. Route every ticket by qualified
`(connection_id, native_id)` and expose unsupported provider operations explicitly.
Cross-provider copy or move is user-initiated and idempotent by operation id and
source provenance.

## Headless setup

Every project bootstrap or recovery path available in the graphical client also
needs an idempotent headless CLI path. This includes ticket source linking, shared
Hot Sheet skills, MCP integrations, and AI-tool setup. Both clients should call the
same underlying workflow. If remote creation cannot be automated safely, print
exact provider-neutral next steps.

## Client behavior

Hot Sheet's Tauri web UI uses Kerf for application state, routing, lists, API
resources, rendering, and event delegation. Its consumer catalog extension is
`clients/web/ai/component-catalog-extension.json`. The project-specific UI
decisions are in `docs/ui-design-philosophy.md` and `docs/design-guidelines.md`;
those documents point to Kerf for shared conventions.
Web Awesome Pro requires a separate license decision. The validated Web Awesome
integration and browser tests live in `spikes/kerf-webawesome/`, with rationale in
`docs/09-technology-decisions.md`. Its form controls emit host-level `input` and
`change`; lifecycle events such as `wa-show` and `wa-hide` retain their prefix.

List and column tickets share one responsive ticket-summary component contract,
including narrow horizontal sizes. App-owned component styles stay beside their
modules and are represented in `clients/web/kerf.components.json`; regenerate the
catalog after changes. Symbolic client icons use official Lucide assets through
the shared icon component. If several Lucide metaphors fit equally well, ask the
maintainer which one to standardize.

Application state updates use WebSockets or genuine long polling with cursor replay
and bounded failure backoff. Fixed-interval network polling is not allowed. Local
display timers must not issue requests. Cover idle request rate and reconnection.

Ticket text editors depart from Kerf's debounced server write: they never write
to the server while the user is typing (HS2-RE1PS6). The exact contract is in
`docs/ui-design-philosophy.md`. Cover rapid coalescing, composed-editor focus
moves, edits after save, and reload recovery.

## Push policy and verification

Push policy: `push-per-ticket`. The maintainer has authorized routine
fast-forward pushes of verified ticket commits to `origin/main`; do not ask again.
After pushing, check the covering `main` CI run with `node scripts/ci-gate.mjs
status --covers <full-sha>`. A remote run never replaces a failing ticket or
integration gate except for the explicit standalone override below.

Fast-push deviation (HS2-Q390N2): ticket and integration checks should run before
push and record an exact-tree receipt. `scripts/push.sh` reuses that receipt;
without one it runs the selected gate. Run `scripts/check.sh` at the appropriate
ticket or integration boundary to keep the push itself fast.
For an urgent standalone push when the maintainer explicitly accepts CI-based
verification, use
`scripts/push.sh --standalone --skip-tests`. This explicit override skips local
tests and the receipt, but still requires a clean checkout, the main-writer
reservation, a fast-forward update, remote SHA confirmation, and a covering
`main` CI run. Record the reason for the override in the ticket. The override
is not available to a ticket integrator, staged push, or promotion.

Batch deviation (HS2-9G0M3V): only while the maintainer allows the `batch`
integration strategy (`coordination strategy-allow --strategy batch --reason
<why>`) may a batch integrator depart from `push-per-ticket` and from that rule. It
integrates several non-overlapping reviewed handoffs, runs only the declared
`scripts/check.sh --smoke` gate, pushes once, and lets one covering `main` CI run
gate them all. A red run needs an attributed follow-up for each failure before the
batch tickets complete, and the circuit breaker falls back to serial while `main`
stays red. Serial remains the default.

CI-staged deviation (HS2-GT57BQ): only while the maintainer allows `ci-staged` may a
ci-staged integrator push each smoke-gated batch to its integration branch (default
`integration`) instead of `main`. CI on that exact staged SHA is the gate, so a batch
needs no other local gate. `main` moves only through `scripts/push.sh --promote`, a
fast-forward to a green staged SHA; a red batch leaves `main` untouched and is
re-staged. The integration branch is integrator-owned scratch, not shared history: a
rebuilt tip may replace only an old tip that `main` does not contain, by
`--force-with-lease`. `main` is never merged, rewritten or force-pushed.

Per-commit `CI` runs Linux x86_64 only (HS2-WSE3FJ). Linux arm64, Windows x86_64
and arm64, macOS Apple Silicon, and the Tauri desktop targets run in the
`Platforms` workflow (`.github/workflows/platforms.yml`, HS2-31RGRK) daily, by
`gh workflow run platforms.yml --ref main -f targets=<all|core|tauri|ids>`, and
for releases. A `Platforms` run never counts as a ticket's CI gate. Native
Windows or macOS evidence needs a passing target of that workflow.

Local verification uses `scripts/check.sh` (HS2-RS0FFT), which prints an affected
package and browser plan and records exact-input receipts. Inspect it with
`scripts/check.sh --plan`; use `--full` when the risk analysis of shared
infrastructure, gate changes, ambiguous dependencies, or a concrete failure
warrants broader checks. Client-only
changes do not routinely run unrelated Rust suites. Full browser and platform
regression remain in CI; run the full browser suite locally only for a specific
unresolved risk and record that reason. Review overlapping UI changes once as an
authorized batch. Never use an environment assertion to bypass receipt
verification. Preserve local edits and use a clean worktree when the current
checkout is dirty.

HS2-EJ8JTG is this project's deviation from the shared Testing Philosophy's
note-only tier declaration and full-affected T2 wording. Set exactly one
`tier:T0`, `tier:T1`, or `tier:T2` ticket tag when declaring risk; the tag is
authoritative, and the preliminary-thoughts note (or trivial claim note) explains
the choice. A reviewer raises risk by changing the tag. An untagged ticket uses
the default T1 profile. T0/T1 use the checked-in five-minute planning budget;
`deep-testing`, `testing:browser`, `testing:platform`, `testing:perf`, and
`test-budget:<duration>` select documented deeper checks. T2 has no time cap:
analyze the actual risks, record the chosen checks and reasons in the ticket,
then run that selection. Because this project configures a `development_strategy`,
`scripts/check.sh` refuses to run without `--test-plan <reviewed.json>` (HS2-RRNZYN).
A T2 ticket also runs `--full` or an explicit analyzed selection with `--t2-stage`
whenever other affected checks exist (HS2-BNAYKG); see
`docs/29-affected-local-verification.md`. Do not add profile flags otherwise;
inspect `--plan` for the selected and deferred checks. CI covers checks not run
locally. The completion note names deferred-to-CI checks and the successful
covering main CI run. If a defect escaped local selection, create a prioritized
follow-up bug naming the missed selection so the policy can be tuned.

## Effective development strategy (HS2-F9062F)

Read the versioned shared `development_strategy` setting headlessly with
`hotsheet-cli coordination --project . development-strategies`, then resolve it
with `hotsheet-cli coordination --project . development-strategy-resolve` at session
start and safe ticket or integration boundaries. The command's `--session` and
`--ticket` values name development strategies, not coordination generations.
An explicit user instruction takes precedence over a ticket strategy, then a
session strategy, then the project default, except that ticket and integration
gates, exact-tree verification, and green-main promotion remain mandatory unless
the maintainer explicitly selects the standalone fast-push deviation above.
Report and refuse a conflict with those requirements. Supply a user-selected
strategy as the highest-priority `--ticket` override and record its user
provenance separately. A ticket strategy takes precedence over a session
strategy when validated by the resolver. An unset project setting preserves the
existing integration default and risk-selected
T0/T1/T2 checks. A selected policy governs test-plan timing, local gate
selection, permitted integration branch, CI cadence, and red-CI recovery; custom
minimum tier, browser, platform, performance, and T0/T1 budget fields add to
mandatory affected checks. Re-resolve a policy change at the next safe boundary;
do not switch an active claim, batch, or integration generation silently. If the
CLI lacks the resolver, report the missing capability and use the project's
supported headless upgrade path before acting on a presumed strategy.

## Deployment-scripts deviation (HS2-51HMCC)

Hot Sheet 2 has no hosted deployment environment or infrastructure definitions.
The detached server runs on a user's machine; desktop packaging and local server
startup are not a Small Tale-hosted infrastructure apply. Therefore the shared
deployment-scripts rule's deploy entry point, credential and state preflight,
phased resource and secret apply, SHA-tagged service image, and deployed-URL smoke
test are presently inapplicable. Do not add a placeholder deploy script. Revisit
the rule after HS2-VXQNVM selects a hosted target; HS2-BYXA4V owns artifact
distribution. See `docs/01-architecture.md` §1.7.

## gitgist commit drafting (HS2-VA97HW)

`gitgist` is pinned as an exact root dev dependency; run `npm ci` at the repository
root once. `npm run commit:msg` drafts from the staged diff with the signed-in
`claude` CLI and retries with the signed-in `codex` CLI; neither needs an API key,
so never add one for drafting. `npm run commit:msg:offline` is the deterministic,
no-network path: it lists the staged files without invoking any provider, because
gitgist refuses commit-message drafting with `--no-ai`. Either draft is an input:
the author rewrites it to the slug-first subject, reviews `git diff --cached`, and
stages named files. Release notes stay with HS2-7MGYJQ.

Settings live only in `gitgist.config.json`. Its `exclude` list holds back the diff
bodies of these generated or imported files; their sources stay included, and
lockfiles are already covered by gitgist's built-in list:

| Pattern | Why |
| --- | --- |
| `AGENTS.md` | Rulesync output; its source is `.rulesync/rules/project-local.md` |
| `.agents/skills/*` | Codex mirrors of `.claude/skills` written by Hot Sheet setup |
| `plugins/*/instructions.md` | Pinned ai-playbook workflow snapshots (`scripts/sync-ai-playbook.mjs`) |
| `plugins/shared/ticket-driven-work.md` | Pinned ai-playbook rule snapshot |
| `plugins/shared/testing-philosophy.md` | Pinned ai-playbook rule snapshot |
| `plugins/shared/requirements-documentation.md` | Pinned ai-playbook rule snapshot |
| `plugins/shared/visual-qa.md` | Pinned ai-playbook rule snapshot |
| `clients/web/ai/component-catalog-extension.json` | Generated by `npm run catalog:generate` |
| `clients/web/ai/component-composition.generated.json` | Generated by `npm run catalog:generate` |
| `docs/design/exports/*` | Sketch exports, including multi-megabyte SVG text |

## Project references

The codebase map is `docs/CODEBASE-MAP.md`; requirements status is in
`docs/README.md`; feature coverage is in `docs/TEST-COVERAGE.md`. The manual plan
is `docs/manual-test-plan.md`.
Rust tests use `cargo nextest run`; web tests run from `clients/web`; migrator tests
run from `migrator/`. Prefer `node scripts/test-quiet.mjs` for quiet affected suites.
The terminal-sizing arbiter, merge and sync logic, claims, indexes, auth, migration,
and shared guidance setup require T2 transition coverage.

`AGENTS.md` is generated output. Change this rule or the pinned rule selection in
`guidance/rulesync/project-selection.json`, then regenerate it with
`hotsheet-cli guidance --project . apply` or the root-pinned `rulesync generate`.
Never edit it by hand. `node scripts/check-rulesync-generation.mjs --fetch` must report
byte equality; CI runs it (HS2-VA97HW).

Hot Sheet 2 is developed by **Small Tale Inc.** under `Small-Tale/hotsheet2`.
Package metadata, licenses, user-facing credits, and bundle identifiers use the
company identity. Individual developers remain the normal Git commit authors.
