---
root: true
targets: ["*"]
description: "Hot Sheet 2 project-specific guidance"
---

# Hot Sheet 2 project guidance

Read this repository's `AGENTS.md` before changing code. Project workflows live in
`.agents/skills/`; shared workflow sources live in `.claude/skills/`. Keep durable
project guidance in local Rulesync rules and provider configuration in its provider
directory.

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

Use Kerf for app state, routing, lists, rendering, event delegation, and reusable
components. Prefer Kerf UI components, then native HTML. Read the installed
`@kerfjs/ui` guidance and `clients/web/ai/component-catalog-extension.json` before
selecting or wrapping a visual component. App-owned component styles stay beside
their modules and are represented in `clients/web/kerf.components.json`; regenerate
the catalog after changes. Use Lucide through the shared icon component for symbolic
icons; never substitute Unicode glyphs or emoji.

Application state updates use WebSockets or genuine long polling with cursor replay
and bounded failure backoff. Fixed-interval network polling is not allowed. Local
display timers must not issue requests. Cover idle request rate and reconnection.

Ticket text editors save to the server on blur, page hide, or explicit finish. A
150 ms debounced local recovery copy keeps drafts between edits. Merge against the
value at edit start and keep the draft visible while saving. Discrete edits can
save immediately; object-creation workflows can have explicit submission.

An enabled child control must work through every shipped parent composition, not
only in its UX demo. Update the demo and component catalog for reusable variants.
Use a native `button` inside Kerf `ToolbarControlGroup` for ordinary actions.

## Verification and project references

The codebase map is `docs/CODEBASE-MAP.md`; requirements status is in
`docs/README.md`; feature coverage is in `docs/TEST-COVERAGE.md`. Update them when
their source behavior changes. The manual plan is `docs/manual-test-plan.md`.
Rust tests use `cargo nextest run`; web tests run from `clients/web`; migrator tests
run from `migrator/`. Prefer `node scripts/test-quiet.mjs` for quiet affected suites.
The terminal-sizing arbiter, merge and sync logic, claims, indexes, auth, migration,
and shared guidance setup require T2 transition coverage.

For a pushed ticket commit, `node scripts/ci-gate.mjs status --covers <full-sha>`
checks whether a successful `main` CI run covers it. A remote run never replaces a
failing local gate.

Hot Sheet 2 is developed by **Small Tale Inc.** under `Small-Tale/hotsheet2`.
Package metadata, licenses, user-facing credits, and bundle identifiers use the
company identity. Individual developers remain the normal Git commit authors.
