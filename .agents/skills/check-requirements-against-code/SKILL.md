---
name: check-requirements-against-code
description: Check Hot Sheet 2's requirements docs against the CLI, server, core crates, and clients, sync the AI summary docs, and report discrepancies
allowed-tools: Read, Grep, Glob, Bash, Agent, Edit, Write
---

Compare the requirements documents in `docs/` against the actual implementation across every
surface:

- the CLI (`crates/hotsheet-cli`);
- the server (`crates/hotsheet-server`, `crates/hotsheet-terminals`);
- the core crates;
- the web client (`clients/web`);
- the migrator (`migrator/`).

Also verify that the AI synthesis docs and `CLAUDE.md` agree with the requirements and the
code:

- the requirements summary table in `docs/README.md`;
- the codebase map in `docs/CODEBASE-MAP.md`;
- the feature coverage matrix in `docs/TEST-COVERAGE.md`.

Produce a report with recommendations and questions about every discrepancy, and fix the
synthesis docs in place.

## Steps

1. **Read all requirements documents.** Enumerate them from disk (`ls docs/[0-9]*.md`), not
   from a range written here, plus `docs/ux-components.md`, `docs/design-guidelines.md`,
   `docs/ui-design-philosophy.md`, `docs/manual-test-plan.md`, and anything under
   `docs/design/` that states behavior. Note every requirement, behavior, and constraint,
   with its status marker where one exists.

2. **Verify each requirement on every surface it touches.** A requirement often spans
   several surfaces. For example, a ticket field must round-trip through the file format, the
   CLI, the server API, and the client. Check each:
   - **CLI**: the subcommand and flags exist and behave as documented. Use
     `./target/debug/hotsheet-cli <subcommand> --help` against the docs in
     `docs/04-core-server-cli.md`.
   - **Server**: the routes exist with the documented method, path, and payload. Enumerate
     with `grep -n '\.route(' crates/hotsheet-server/src/lib.rs` and compare to the docs and
     `docs/CODEBASE-MAP.md`.
   - **Clients**: the UI surface exists, is wired in the real app (not only in `/ux-demo`),
     and behaves as `docs/06-clients.md` and `docs/ux-components.md` describe.
   - **Headless parity** (CLAUDE.md): every setup, bootstrap, recovery, source-linking,
     skill, or MCP workflow available in a client also has an idempotent CLI path. Flag
     client-only capabilities.
   - **Provider neutrality** (CLAUDE.md, `docs/16-external-sync-interface.md`): documented
     provider capabilities match what each provider implementation enforces.

3. **Check for undocumented features.** Look for CLI subcommands, server routes, settings
   keys, and client features with no requirements doc. Each should be documented or
   questioned.

4. **Check for stale documentation.** Look for requirements describing behavior that no
   longer exists or changed, and for status markers (Shipped / Partial / Design only /
   Deferred) that no longer match reality.

5. **Verify `CLAUDE.md`.** Its project test setup, commands, gates, and client rules must match
   the actual scripts (`clients/web/package.json`, `migrator/package.json`,
   `.cargo/config.toml`, `.github/workflows/*.yml`). Flag any command that no longer exists
   and any enforced gate it doesn't mention.

6. **Synchronize `docs/CODEBASE-MAP.md`.** Confirm, then **edit in place**:
   - the directory tree against `ls` of `crates/`, `clients/web/src/`, `migrator/`,
     `scripts/`, and `docs/`;
   - entry points, CLI subcommands, and server routes;
   - schema and file-format fields against `crates/hotsheet-model`;
   - settings keys against `crates/hotsheet-ticketing/src/settings.rs` and its users;
   - client modules;
   - the "where do I look for X" index, so every entry points at a file that exists.

7. **Synchronize the requirements summary in `docs/README.md`.** **Edit in place**:
   - Each numbered doc has a row with a correct status marker.
   - Any newly added doc is listed.
   - Any renamed or superseded doc is updated.
   - Shipped-versus-design status reflects the code you just checked.

8. **Synchronize `docs/TEST-COVERAGE.md`.** Run `node scripts/check-test-coverage.mjs`. Then:
   - Flag features whose documented status disagrees with the evidence (claims of
     double coverage whose test files no longer exist or no longer exercise the feature).
   - Flag shipped features missing a row.
   - Fix the rows you can verify.

9. **Final consistency pass.** Make `CLAUDE.md`, `docs/README.md`, `docs/CODEBASE-MAP.md`,
   and `docs/TEST-COVERAGE.md` agree with each other and with the source docs and code.
   Resolve every disagreement in favor of the code or the numbered source doc. The most
   common drift: a CLI subcommand, server route, or settings key added with only its doc
   updated, while the codebase map and summary lag behind. Look for that explicitly.

## Report Format

### Discrepancies Found

For each discrepancy:

- **Requirement**: doc, section, and the stated requirement
- **Surface**: cli | server | core | client | migrator (list each one affected)
- **Implementation**: what the code actually does (path and line numbers)
- **Type**: `missing` (doc says X, code doesn't do X) | `different` (doc says X, code does
  Y) | `undocumented` (code does X, no doc mentions it) | `stale` (doc says X, feature was
  removed or changed) | `parity` (a client capability with no CLI path, or the reverse)
- **Recommendation**: update the doc, or fix the code?

### Synthesis Doc Synchronization

- **`docs/CODEBASE-MAP.md`**: sections edited and why (or "no changes needed").
- **`docs/README.md` requirements summary**: rows edited and why.
- **`docs/TEST-COVERAGE.md`**: rows edited and why.
- **`CLAUDE.md`**: drift found. Report it; edit only factual command or gate names.

### Questions

List ambiguous requirements where the implementation made a judgment call, and ask whether
the current behavior is correct.

### Summary

- Requirements checked, and how many are fully implemented.
- Discrepancies by type and by surface.
- Files edited.
- Hot Sheet tickets filed for code fixes (list the slugs). File one for every discrepancy
  that needs a code change rather than a doc edit.
