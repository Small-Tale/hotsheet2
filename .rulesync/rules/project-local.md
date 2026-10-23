---
root: true
targets: ["*"]
description: "Hot Sheet 2 project-specific guidance"
---

# Hot Sheet 2 project guidance

Read the generated `AGENTS.md` before changing code. Project workflows live in
`.agents/skills/`; shared workflow sources live in `.claude/skills/`. Use the
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
resources, rendering, and event delegation. Read the installed Kerf guidance and
`clients/web/ai/component-catalog-extension.json` before selecting a component.
The project-specific UI decisions are in `docs/ui-design-philosophy.md` and
`docs/design-guidelines.md`; those documents point to Kerf for shared conventions.
Web Awesome Pro requires a separate license decision. The validated Web Awesome
integration and browser tests live in `spikes/kerf-webawesome/`, with rationale in
`docs/09-technology-decisions.md`. Its form controls emit host-level `input` and
`change`; lifecycle events such as `wa-show` and `wa-hide` retain their prefix.

List and column tickets share one responsive ticket-summary component contract,
including narrow horizontal sizes. App-owned component styles stay beside their
modules and are represented in `clients/web/kerf.components.json`; regenerate the
catalog after changes. Symbolic client icons use official Lucide assets through
the shared icon component, with an accessible name for icon-only controls. Every
actionable context-menu item has a meaningful icon. If several Lucide metaphors
fit equally well, ask the maintainer which one to standardize.

Application state updates use WebSockets or genuine long polling with cursor replay
and bounded failure backoff. Fixed-interval network polling is not allowed. Local
display timers must not issue requests. Cover idle request rate and reconnection.

Ticket text editors save to the server on blur, page hide, or explicit finish,
never while the user is typing. A 150 ms debounced local recovery copy is keyed
by project, ticket, and field and retains the value at edit start. Merge against
that value, keep the draft visible while saving, and restore an unsaved draft when
the editor reopens. Ordinary text fields have no routine Save/Cancel buttons;
discrete edits can save immediately, and object creation can submit explicitly.
Cover rapid coalescing, composed-editor focus moves, edits after save, and reload
recovery.

## Push policy and verification

Push policy: `push-per-ticket`. The maintainer has authorized routine
fast-forward pushes of verified ticket commits to `origin/main`; do not ask again.
Commit a finished ticket before starting the next one. After pushing, confirm the
remote contains the commit and use `node scripts/ci-gate.mjs status --covers
<full-sha>` to check the successful `main` CI run. A remote run never replaces a
failing local gate.

## Project references

The codebase map is `docs/CODEBASE-MAP.md`; requirements status is in
`docs/README.md`; feature coverage is in `docs/TEST-COVERAGE.md`. Update them when
their source behavior changes. The manual plan is `docs/manual-test-plan.md`.
Rust tests use `cargo nextest run`; web tests run from `clients/web`; migrator tests
run from `migrator/`. Prefer `node scripts/test-quiet.mjs` for quiet affected suites.
The terminal-sizing arbiter, merge and sync logic, claims, indexes, auth, migration,
and shared guidance setup require T2 transition coverage.

Hot Sheet 2 is developed by **Small Tale Inc.** under `Small-Tale/hotsheet2`.
Package metadata, licenses, user-facing credits, and bundle identifiers use the
company identity. Individual developers remain the normal Git commit authors.
