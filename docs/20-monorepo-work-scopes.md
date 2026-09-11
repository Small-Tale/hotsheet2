# Monorepo work scopes

Status: **Proposed** (HS2-D1XNTE investigation; implementation is split into
follow-up tickets)

Hot Sheet currently uses **project** in the client for a tab backed by one
machine-local **checkout**. That is a useful runtime boundary, but it is too
heavy to also mean every package, service, or team inside a monorepo. This note
separates the concepts and recommends an incremental model that preserves the
zero-configuration experience for small repositories.

## 20.1 The problem in a real repository

[Kerf](https://github.com/brianwestphal/kerf) is one Git checkout containing the framework root, `ui`,
`eslint-plugin`, `create-kerf-component`, `site`, examples, benchmarks, and
shared tooling. One Hot Sheet project is workable today, but larger teams may
want package-specific worklists and working directories without turning every
package into an independent repository or ticket backlog.

The current architecture couples a checkout root to more than ticket display:

- the checkout id is derived from its canonical absolute path and registered in
  the machine-local checkout registry;
- one checkout connects to one or more authoritative ticket sources, with one
  default source for creation;
- project open starts repository watching, refreshes the checkout-local
  `.hotsheet/worklist.md`, and schedules project setup/freshness work;
- repository status, configured commands, terminals, and AI Drive sessions use
  the checkout root as their filesystem context;
- the web client persists state and running resources by project/checkout id.

Registering `kerf/ui` and `kerf/eslint-plugin` as ordinary projects would
therefore create overlapping runtime ownership. Parent-walking `.hotsheet/store`
resolution could also make both subroots silently reuse the same ticket store,
despite appearing as separate projects.

## 20.2 Concepts and boundaries

Use four orthogonal concepts instead of stretching one concept across all jobs:

| Concept | Owns | Does not own |
|---|---|---|
| **Checkout** | canonical filesystem root, repository runtime, watchers, setup, server resources, project tab | package organization or ticket authority |
| **Source/store** | authoritative ticket system, identity, permissions, sync, retention, creation target | code working directory or UI grouping |
| **Tag/saved view** | portable ticket metadata and ad hoc query/display | filesystem context, integrity, permissions, or exclusive ownership |
| **Work scope** | optional named slice of a checkout: relative code roots, ticket selector, and defaults | ticket identity, authorization, repository lifecycle, or a new server instance |

The recommended product term is **work scope**. “Project” is already overloaded,
“package” excludes services and cross-cutting domains, and “component” collides
with UI terminology. The UI may shorten the label to **Scope** where context is
clear.

Every checkout has an implicit, immutable **All** scope. Repositories that never
configure another scope behave exactly as they do today.

## 20.3 Alternatives considered

| Approach | Strengths | Weaknesses | Appropriate use |
|---|---|---|---|
| Nested checkouts/subroots | already possible; distinct cwd and client tab | duplicates overlapping repository/watch/setup/resource ownership; path-derived identity; unclear parent store inheritance | a truly independent nested repository or lifecycle boundary |
| Store per package | hard source/ACL/sync separation; clear creation route | fragments backlog and history; increases provider administration; does not inherently define code cwd | packages with genuinely different ticket authority or access policy |
| Tags and saved views | available now; portable, cheap, overlapping | free-form and drift-prone; provider capabilities vary; no path/cwd; not a security boundary | small-team organization and early conventions |
| Singular `project` field on every ticket | easy exclusive reporting when universally supported | requires provider/file-format support and bulk mutation; cannot express cross-cutting work; rename/move semantics are costly | not recommended as the foundation |
| First-class work scopes | separates organization from authority; supports cwd plus overlapping queries; zero-ticket migration | new config/API/UI concept; selectors can overlap or drift; needs strict path validation | recommended general monorepo model |

Tags are still a good implementation primitive for a scope selector, and stores
remain the correct authority boundary. A scope does not replace either one.

## 20.4 Proposed model

A work scope is checkout-local and has a stable id independent of its display
name and directory name:

```jsonc
{
  "id": "ui",
  "name": "UI",
  "roots": ["ui"],
  "ticketQuery": "tag:ui",
  "defaultSource": "team-github",
  "ticketDefaults": { "tags": ["ui"] },
  "archived": false
}
```

Recommended invariants:

1. `id` is checkout-unique, stable, and limited to a portable slug. Renaming or
   moving a directory does not change it.
2. `roots` contains checkout-relative paths. Resolve and canonicalize on the
   server; reject absolute paths, `..` escapes, and symlinks escaping the
   checkout. Version one may limit authoring to one root while keeping an array
   on the wire for future cross-cutting domains.
3. `ticketQuery` uses the existing provider-neutral query language. The server,
   not each client, expands and executes it.
4. `defaultSource` is optional and must be connected to the checkout. It affects
   ticket creation, not visibility or permission.
5. `ticketDefaults` is an allowlisted set of fields. Version one should support
   tags only; explicit create input wins over defaults.
6. A ticket may match zero, one, or several scopes. Scope membership is derived,
   not part of ticket identity. “All” always remains available.
7. Scope filters are never an authorization boundary. Source/provider policy is
   evaluated before scope filtering.

Example for Kerf:

| Scope | Root | Selector/default |
|---|---|---|
| All | `.` | no selector |
| Core | `src` | `tag:core`, default tag `core` |
| UI | `ui` | `tag:ui`, default tag `ui` |
| ESLint plugin | `eslint-plugin` | `tag:eslint-plugin` |
| Component tooling | `create-kerf-component` | `tag:component-tooling` |

This does not require separate stores. Kerf could add a separate source for a
private security backlog later and choose it as a scope default only where that
authority boundary is real.

## 20.5 Scale profile

### Small repository or team

- Do nothing: use the implicit All scope.
- Use tags and saved views until a distinct working directory or creation
  default provides enough value to justify a named scope.
- Keep one source unless permissions or ticket authority genuinely differ.

### Growing monorepo

- Add explicit scopes for stable packages/services/domains.
- A selected scope filters the worklist and supplies the default cwd for new
  terminals, configured commands, and AI Drive connections.
- Ticket creation previews applied tags and source, and allows an explicit
  override.
- Cross-package work can match several scopes or remain visible only in All.

### Large and enterprise monorepo

- Keep repository runtime and security enforcement at checkout/source
  boundaries; do not make thousands of overlapping nested projects.
- Page/facet scope results server-side and lazy-load scope counts.
- Permit explicit hierarchy/policy inheritance only after flat scopes are
  proven. A future `parentId` must not change ticket identity.
- Workspace manifest, CODEOWNERS, or build-graph discovery may propose scopes,
  but creation is previewed and confirmed. Generated suggestions must never
  silently rewrite tickets or permissions.
- If teams require exclusive cost/ownership reporting, add an orthogonal,
  provider-mapped ownership facet later; do not overload work scopes with a
  singular ticket project field.

## 20.6 Persistence and sharing

The current checkout registry is explicitly machine-local. Shared/local project settings
are checkout-root-owned (`<project-root>/.hotsheet/settings.json` and
`settings.local.json`) and therefore already have one stable owner when a checkout uses
zero, one, or several ticket sources. Scope definitions need an additional schema and
sharing contract; they should build on that project-owned boundary rather than a ticket
source.

Use two layers:

1. **Initial delivery:** store scope definitions in the versioned checkout
   registry schema and expose them through the same core/server/CLI contracts.
   This makes the feature useful without inventing a cross-repository ownership
   protocol. Scope order, selected scope, pins, and hidden state remain
   device-local.
2. **Opt-in team sharing:** add a checkout-root manifest with explicit schema and
   precedence rules. Import it into the local registry/cache. The manifest owns
   portable ids, names, relative roots, selectors, and ticket defaults; local
   settings own absolute paths and UI preferences. This needs a dedicated design
   and compatibility ticket because Kerf currently ignores most of `.hotsheet/`;
   the project-owned settings location is available, but it is not itself a portable
   work-scope manifest or merge protocol.

Do not put a scope definition independently into every connected store. That
would create divergent copies with no clear winner. Do not make a shared
manifest a prerequisite for the first useful increment.

## 20.7 API and headless behavior

Proposed resource surface:

- `GET/POST /checkouts/{checkout}/scopes`
- `PATCH/DELETE /checkouts/{checkout}/scopes/{scope}`
- checkout-qualified ticket query accepts `scope=<id>`; the server resolves its
  query and applies provider capability checks;
- ticket creation accepts `scope=<id>`; the server resolves default source and
  allowed field defaults, with explicit request fields taking precedence;
- terminals, commands, and client-owned Drive connection creation accept an
  optional `scopeId`; the server resolves the cwd beneath the checkout root;
- checkout events include scope-definition changes so clients can invalidate
  counts and selection safely.

CLI/MCP parity is required. Suggested CLI forms are `checkout scope list|add|edit|remove`
and `ls/new/work --scope <id>`. An unavailable selector capability must produce a
clear error rather than silently widening the result.

Repository status remains checkout-root status. A later path-prefix view can
summarize changes under a scope, but Git operations and repository identity do
not move to the subdirectory.

The checkout-local worklist has one writer and should remain at
`.hotsheet/worklist.md` initially. Scope-specific files inside package directories
would create multiple writers and additional AI-instruction discovery semantics;
that requires a separate decision.

## 20.8 Client behavior

- Keep one project tab per checkout. Add a scope switcher/group beneath the
  project, with All first and current selection remembered per checkout.
- “Add scope” chooses a relative root, name, optional ticket query/default tags,
  and optional connected default source. Advanced fields are progressive
  disclosure, not required for a directory-only scope.
- Ticket lists and counts come from scope-qualified server queries.
- Ticket creation explains inferred defaults before save and lets the user
  override them.
- Terminals and Drive sessions opened from a scope use and display that scope's
  cwd. Existing checkout-root resources remain associated with All.
- If a user opens a directory nested under an already-open checkout, offer
  “Create scope” first and “Open as separate checkout” as the advanced isolation
  choice. Never automatically consolidate or delete an existing registration.

## 20.9 Migration and rollout

1. **Phase 0 — conventions now:** document tag + saved-view recipes for package
   slices. No data migration.
2. **Phase 1 — local scopes:** add the validated domain model, registry/API/CLI
   CRUD, scope-aware ticket query/create, and the client switcher. Existing
   checkouts synthesize All, so registry migration is additive.
3. **Phase 2 — working context:** route new terminal, command, and Drive resources
   through a validated scope cwd. Keep existing resources at their original cwd.
4. **Phase 3 — sharing/discovery:** add an opt-in checkout manifest and preview
   adapters for common workspace manifests. Explicitly reconcile local edits and
   shared changes.
5. **Phase 4 — scale:** benchmark very large scope/source sets, add lazy counts
   and indexing as evidence requires, then decide whether hierarchy and an
   exclusive ownership facet are warranted.

Existing nested checkouts remain valid. A future consolidation assistant may
create equivalent scopes, show source/default differences, and only then offer
to remove redundant machine-local registrations. It must not move tickets,
rewrite tags, change sources, or delete files implicitly.

## 20.10 Risks and required safeguards

- **Terminology:** UI and docs must consistently distinguish checkout, source,
  store, project tab, and work scope.
- **False security:** every UI must state or imply correctly that a scope is a
  filter/context, not an access-control boundary.
- **Selector drift/overlap:** expose the query and match count; empty and
  multi-scope results are valid. Do not claim exclusive ownership.
- **Provider variance:** validate query/default capabilities and never fall back
  to All on error.
- **Path safety:** canonicalize on every server-side use, handle case semantics,
  and reject symlink escapes and deleted roots.
- **Source/default mismatch:** reject a disconnected source and explain when a
  scope selector would hide the newly created ticket.
- **Duplicate AI work:** scope-qualified autonomous queues must still use the
  existing ticket claim identity globally, so overlapping scopes cannot dispatch
  the same ticket twice.
- **Scale:** avoid one filesystem watcher, repository scan, or worklist writer per
  scope. Counts and results need bounded/paged APIs.
- **Configuration ownership:** shared-manifest precedence and merge behavior must
  be explicit before team-shared authoring ships.
- **Moves and renames:** stable ids survive display/root changes; missing roots
  degrade to a visible configuration error, not a new checkout id.

## 20.11 Decision guide

Use a **tag or saved view** when only the ticket list needs grouping. Use a
**work scope** when grouping also needs a code root, creation defaults, or a
working context. Add a **source/store** when ticket authority, access, sync, or
retention differs. Open a **separate checkout** when repository/runtime lifecycle
really is independent. These choices can coexist; none should be inferred from
directory layout alone.

Before Phase 1, maintainers should confirm the term “work scope,” local-first
persistence, and tags-only create defaults. The proposed wire shape deliberately
leaves room for multiple roots and future hierarchy without requiring them in
the first implementation.

## 20.12 Implementation follow-ups

- **HS2-H5B5CF:** work-scope domain model, checkout-registry persistence, and
  headless CRUD foundation.
- **HS2-8FG3JK:** scope-aware ticket query and creation semantics.
- **HS2-Z8A4T0:** client scope switcher, setup, persistence, and nested-root
  affordance.
- **HS2-RFK7GT:** validated scope cwd for terminals, commands, and Drive.
- **HS2-YDG6Q1:** opt-in shared manifest and workspace discovery preview.
- **HS2-XFYWX6:** safe nested-checkout consolidation assistant.
- **HS2-3XGVKP:** scope-aware worklist and autonomous worker contract.
- **HS2-8SAG6P:** enterprise-scale benchmark and hardening.
