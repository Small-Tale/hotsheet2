---
name: check-code-hygiene
description: Check Hot Sheet 2's CLI, server, core crates, web client, and migrator for standardization, readability, maintenance complexity, and defensive coding
allowed-tools: Read, Grep, Glob, Bash, Agent
---

> **Rules live in CLAUDE.md and the docs, not here.** Before flagging anything against a
> rule, open `CLAUDE.md` and `docs/12-code-organization-and-testing.md` and read the current
> text. Treat any figure below as a pointer to the right section. The mechanical recipes
> (greps, `wc`, ast-grep patterns) are the durable part of this skill.

Analyze Hot Sheet 2 for code hygiene problems and report them. Cover every surface and say
which one each finding belongs to:

- **CLI**: `crates/hotsheet-cli` (subcommands, argument parsing, output).
- **Server**: `crates/hotsheet-server` (routes, handlers, WebSocket/long-poll channel) and
  `crates/hotsheet-terminals`.
- **Core**: `hotsheet-model`, `hotsheet-ticketing`, `hotsheet-index`, `hotsheet-extsync`,
  `hotsheet-aitools`, `hotsheet-plugins`, `hotsheet-mcp`, `hotsheet-tls` (confirm with `ls crates`).
- **Clients**: `clients/web/src` (components, interactions, features, app runtime, UX demo).
- **Migrator**: `migrator/`.

Skip generated files, fixtures, and vendored assets.

## Analysis Areas

### 1. Standardization

- **Naming**:
  - Rust: `snake_case` items, `CamelCase` types, `SCREAMING_SNAKE_CASE` constants.
  - TypeScript: camelCase values, PascalCase types and components.
  - CSS: BEM blocks named for the owning component.

  Flag inconsistencies, including one concept with different names across the CLI, server
  JSON, and client (for example a field called one thing on the wire and another in the UI
  model).

- **One concern per module**: a Rust module or TS file holding unrelated responsibilities,
  or a component file exporting several unrelated components.
- **Error handling consistency**:
  - CLI: errors reach the user with an actionable message and a non-zero exit.
  - Server: handlers map failures to the shared API error type with stable status codes.
  - Client: API failures surface through the established toast/error components, not ad-hoc
    `console.error`.

  Flag operations of the same kind handled three different ways.

- **CLI ↔ server parity of naming**: a CLI flag and the matching HTTP field or route
  describing the same thing with different terms.
- **Imports**: Rust `use` grouping follows `rustfmt`; TS import order is enforced by lint.
  Flag only what the linters can't see, such as deep relative imports that bypass a module's
  public entry.

### 2. Human Readability

- **Function length**: flag functions over about 50 lines that hide more than one step.
  Large `match`/route tables and generated data are not findings.
- **Nesting depth**: more than three levels.
- **Magic numbers and strings**: timeouts, limits, ports, retry counts, and event names that
  should be named constants, especially values duplicated between the server and the client.
- **Unclear naming**: ambiguous abbreviations or single-letter names outside tight closures.
- **Comments**: they should explain _why_. Flag both a missing why on non-obvious logic and
  noise comments that restate the next line. Ticket-slug references in comments are the
  house style for decisions; flag ones pointing at tickets that are cancelled or deleted.
- **File length**: rank with `wc -l` across
  `crates/*/src/**/*.rs clients/web/src/**/*.{ts,tsx,css}`, then judge each large file by
  concern count. `crates/hotsheet-server/src/lib.rs` and `clients/web/src/app/runtime.tsx`
  are the usual suspects; a split proposal is a finding, length alone is not.

### 3. Maintenance Complexity

- **Coupling**:
  - Crate dependencies should follow the architecture in `docs/01-architecture.md`. Check
    `cargo tree --workspace --depth 1` for a CLI or server crate reaching into another
    crate's internals instead of its public API.
  - Client: modules under `components/` should not import `interactions/` or the app
    runtime.
- **Shared mutable state**:
  - Rust: `static`/`OnceLock`/`Mutex` globals.
  - TS: module-level `let` and signals outside a store or controller.

  Each needs a stated reason.

- **Async complexity**:
  - Rust: long `tokio` task chains without cancellation, or `spawn` without ownership of the
    handle.
  - TS: promise chains or async flows that can resolve after their owner is disposed.
- **Branching**: `if`/`else` or `match` chains over about six arms that dispatch on a type
  or provider and would be clearer as a table or trait.
- **Duplicate patterns**: the same workflow implemented separately in the CLI and the server,
  or in two client components. Headless parity means one shared application workflow
  invoked by both; flag forks.

### 4. Defensive Coding

- **Input validation at boundaries**:
  - Server handlers validate path, query, and body inputs (ids, slugs, sizes, enum strings)
    and reject unknown values explicitly.
  - CLI arguments are validated before touching the store.
  - Client API adapters parse responses instead of casting them.
- **Panics on boundaries**: `unwrap`/`expect`/indexing on user, file, network, or git input in
  non-test Rust code.
- **Error boundaries**: blanket `catch {}` (TS) or `let _ =` / `.ok()` (Rust) that swallow
  errors without a reason comment.
- **Null and option safety**: TS non-null assertions (`!`) in production code; Rust
  `Option` unwrapped where `?` or an explicit error belongs.
- **Type safety**: `any`, `as unknown as`, and unchecked `as` casts on parsed JSON in the
  client and migrator.
- **Injection and paths**:
  - Commands built from strings instead of typed program+args.
  - Path joins from request input without containment checks.
  - Markdown or HTML rendered without the sanitizing pipeline.
- **Secrets**: credentials, tokens, or keychain values in logs, settings files, ticket text,
  or error messages.

## Report Format

For each finding:

- **Surface**: cli | server | core | client | migrator
- **File**: path and line numbers
- **Category**: standardization | readability | maintenance | defensive
- **Severity**: high | medium | low
- **Description**: what the issue is
- **Suggestion**: how to fix it

End with a prioritized summary of the top 10 most impactful improvements across all
surfaces. File Hot Sheet tickets for every non-trivial finding (`hs-task` for cleanups,
`hs-bug` for real defects) and list their slugs.
