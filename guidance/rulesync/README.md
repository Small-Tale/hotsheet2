# Opt-in Rulesync guidance

Hot Sheet's built-in writer is the default. It ships pinned offline snapshots and
requires no Rulesync installation. Choose Rulesync when this project should select
and update reusable rules independently. The validated CLI version is **38.0.0**.

For Hot Sheet-managed selection from a pinned source, use Settings → Project guidance or
the headless path below. `register` resolves a full commit and caches the selected
source's rule bodies, so later apply and setup do not fetch the source again:

```sh
hotsheet-cli guidance --project . register --ref <tag-or-commit>
hotsheet-cli guidance --project . list
hotsheet-cli guidance --project . select --source ai-playbook --rule <rule-name>
hotsheet-cli guidance --project . owner rulesync
hotsheet-cli guidance --project . apply
```

Repeat `--rule` or add `--skill` to select more entries. This path materializes
`.rulesync/rules/.curated` and `.hotsheet2/rulesync.generated.jsonc`, then invokes
Rulesync. Review the generated `AGENTS.md` before committing because Rulesync owns
the whole file. The example configuration and migration steps below remain useful
when a project manages Rulesync inputs directly rather than through the picker.

The committed [example configuration](rulesync.jsonc) selects all available rules from
ai-playbook's `main` branch. Its committed [lockfile](rulesync.lock) pins the exact
reviewed v1.3.0 commit (`02b66e65…`) and all 23 artifact hashes. Frozen installs do
not advance that pin.
Copy both files to the consuming project's root.
To select only some rules, replace `rules: ["*"]` with, for example:

```json
"rules": ["hotsheet-ticket-driven-work", "hotsheet-testing-philosophy"]
```

Available names and section versions are listed in
[ai-playbook](https://github.com/Small-Tale/ai-playbook). The `agentsmd` target writes
the canonical `AGENTS.md`; `CLAUDE.md` imports it with `@AGENTS.md`. Leave the Claude
target out of this configuration so the reusable bodies have one output owner.
In a fresh project create `CLAUDE.md` containing that import. Preserve existing
Claude-local text beside its import in an existing project.

Hot Sheet 2 itself selects the five new v1.3.0 rules through its project guidance
picker: `git-commit-and-push`, `release-scripts`, `gitgist-release-notes`,
`deployment-scripts`, and `kerf-ui-interaction-defaults` (HS2-Y84WA5). The declared
`push-per-ticket` policy and standing authorization live in the local Rulesync
rule; the release, notes, and deploy rules set the target workflows for
`HS2-7MGYJQ` and `HS2-51HMCC`; `HS2-CD3D2T` will finish the push tooling. The
Kerf interaction rule now owns the shared UI defaults. The picker keeps these
rules opt-in for other projects because their release, deployment, Git, and UI
workflows may differ. Hot Sheet 2's selection is kept in its project guidance
state; the public example intentionally selects every available rule to exercise
the frozen source lock.

## Choose the writer first

From the consuming project, with its ticket store initialized:

```sh
hotsheet-cli -C <ticket-store> setup --guidance-owner rulesync --project .
```

This writes `.hotsheet2/guidance-owner.json` with schema version 1 and owner
`rulesync`. Commit it. It changes ownership only; existing instructions are left
intact. Every setup path, including server project-open refresh and disabled-tool
cleanup, then leaves instruction files untouched. Hot Sheet still maintains its
MCP config, hooks, and skills. `setup --diagnose` identifies instruction artifacts
as managed by Rulesync. An invalid owner marker fails safely instead of reverting
to the built-in writer. Missing markers and explicit `builtin` use the default.

## Fresh project

Copy the example config and lock, create the native Claude import if using Claude,
and add project guidance under `.rulesync/rules/project.md`:

```markdown
---
root: true
targets: ['*']
---

## Project specifics

The commands, architecture, and conventions for this project belong here.
```

Run the chosen tool's ordinary Hot Sheet setup to maintain its integrations, then:

```sh
npx --yes rulesync@38.0.0 install --frozen
npx --yes rulesync@38.0.0 generate --dry-run
npx --yes rulesync@38.0.0 generate
npx --yes rulesync@38.0.0 generate --check
```

Commit the configuration, lock, local rule sources, ownership marker, and generated
instruction files. Ignore `.rulesync/rules/.curated/`; it is a cache reconstructed
from the lock. Keep the local inputs outside `.curated/`.

## Migrate an existing Hot Sheet-managed project

Rulesync generates the whole `AGENTS.md` file. Hot Sheet saves unmarked local
guidance and nested project specifics into a Rulesync input when ownership changes:

1. Select Rulesync ownership before generation, so a background Hot Sheet refresh
   cannot compete. Back up both current instruction files, for example under
   `.hotsheet2/guidance-backup/`, and retain the backups until the result is reviewed.
2. Review `.rulesync/rules/project-local.md`, created by the ownership switch with
   the root frontmatter. It contains unmarked `AGENTS.md` prose and the contents of
   nested `hotsheet:specifics` blocks; the switch stops if that file already differs
   or a managed marker is incomplete. Project guidance lists local rule names and
   paths separately from pinned source rules. Keep Claude-only local guidance in `CLAUDE.md`
   beside its import. If a managed block was customized, retain those edits as local
   policy or use a reviewed same-name override as below. The reusable Hot Sheet
   blocks come from the selected ai-playbook source.
3. Copy the example configuration and lock. Change the selection if desired and run
   plain `install` once to update the lock's selection at its existing pinned ref.
   Run `generate --dry-run` and review the proposed result against the backups.
4. Generate, then verify each selected reusable section occurs once, every local
   policy and specific remains, and `CLAUDE.md` has one import. Run Hot Sheet
   `setup --refresh`; the generated instruction bytes must remain unchanged.

This remains an explicit migration review. The ownership command preserves prose
outside managed sections and nested project specifics automatically; it cannot decide
whether edits inside an old managed block are local customizations or obsolete prose.
The switch and a changed Hot Sheet Rulesync apply queue a deduplicated Up Next task
for an AI worker to consolidate `AGENTS.md` and `project-local.md` after review
(HS2-34S102). A completed task does not block a new task after a later rule change.
Direct `rulesync` updates are noticed when Hot Sheet next checks standalone
Rulesync status; the review task is then queued in the linked ticket source.

## Install, update, and customize

```sh
npx --yes rulesync@38.0.0 install
npx --yes rulesync@38.0.0 install --outdated
npx --yes rulesync@38.0.0 install --update
npx --yes rulesync@38.0.0 generate
```

Plain install reuses the locked revision; changing rule selection updates that
selection at the same locked ref. `--frozen` rejects an uncovered selection.
`--outdated` is read-only: it compares the lock with the declared `main` branch.
Hot Sheet exposes that standalone lock state without changing it through
`hotsheet-cli guidance --project . check-rulesync-updates` and the Project
guidance Settings tab. The check requires an installed Rulesync 38.0.0 binary;
missing or unsupported binaries and unresolved sources are `unknown`. Its
binary version is shown separately from each source's current, outdated, or
unlocked lock state. Hot Sheet-managed pins remain a separate update channel.
Use outdated followed by update to advance the committed lock, then review the
resolved commit, source changes, and generated diffs before committing them.
The validated Rulesync 38 Git transport rejects a raw SHA as an initial ref and
mishandles annotated release tags. The example therefore pins through its committed
lock and uses `--frozen` for reproduction. Do not replace its ref with a release tag
until the transport limitation has been revalidated.

For an override, copy a curated rule to the same filename directly under
`.rulesync/rules/` and edit that local source. Local same-name rules take precedence
and survive source updates. To return to upstream behavior, remove that local
override, install, and generate. Do not edit `.curated/`: locked installs restore it.
Project-specific additions should usually use a distinct local rule name, so they
continue receiving upstream improvements.

## Return to built-in management

Stop Rulesync generation in local scripts or CI first. Back up the current outputs
and local inputs. Rebuild `AGENTS.md` from the reviewed project-local guidance only,
including any local edits made since migration; omit Rulesync's reusable Hot Sheet
outputs so the built-in writer can install them once. Keep the native Claude import.
Then run:

```sh
hotsheet-cli -C <ticket-store> setup --guidance-owner builtin --project .
hotsheet-cli -C <ticket-store> setup claude --project .
hotsheet-cli -C <ticket-store> setup codex --project .
hotsheet-cli -C <ticket-store> setup --refresh --project .
```

Run only the tool setup commands the project uses. The ownership switch itself does
not delete content. Review the restored shared guidance and local policies, then
archive the Rulesync config and inputs or retain them without running generation.
Do not run both writers against the files while builtin ownership is active.

## Verification

`scripts/validate-rulesync.mjs` exercises the real pinned CLI against fresh and
existing temporary projects, selected/all rules, a frozen lock, actual Git source
updates and outdated checks, local overrides, Hot Sheet refresh, and the built-in
return. Build `hotsheet-cli`, then run from the HS2 checkout:

```sh
npx --yes --package=rulesync@38.0.0 -c 'node scripts/validate-rulesync.mjs'
```

This optional acceptance check uses public network access and temporary Git repos;
it does not make Rulesync a Hot Sheet runtime or build dependency. Shared planner
unit tests and CLI integration tests cover ownership, invalid markers, diagnostics,
repeated refresh, and disabled tools in the normal test suites.

Rulesync contracts: [sources and locks](https://rulesync.dyoshikawa.com/guide/declarative-sources.html),
[root rule composition](https://rulesync.dyoshikawa.com/reference/file-formats.html).
