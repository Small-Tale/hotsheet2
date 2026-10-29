# 05. AI-Tool Plugin Interface

> **Status: Proposed.** This carries over HS1's hard-won plugin design (docs/132)
> as a _starting point_, not an endpoint. HS1 spent an eight-phase epic clawing
> per-tool `if (tool === …)` branches back into one interface; HS2 begins there.
>
> **Ownership + extensibility decided (maintainer, 2026-08-20).** Two changes from
> HS1: (1) the plugin registry and all setup/instruction/skill/MCP/settings
> _management_ lives in the **core**, driven by **either the CLI (headless — no
> client, no server) or the server**, not the app layer (§5.1a, §5.12). (2) plugins
> are **external and loadable**, layered as manifest-only (data, no ABI) → behavioral
> by capability (**subprocess** for process-shaped behaviors, **WASM** for
> pure-compute), with built-ins shipped through the same loader and a trust gate
> (§5.12). Build tracked in HS2-91/HS2-92/HS2-93.

## 5.1 The principle

**AI-tool integration is entirely plugin-based. No tool is first-class — not even
Claude.** Every tool (Claude, Codex, Gemini, OpenCode, Antigravity, Goose, and
the editor tools Cursor/Copilot/Windsurf) is one implementation of a single
interface. The interface must fit the tool it was _not_ designed around, or it is
a hierarchy, not an interface (HS1's acceptance test — Claude was migrated last,
precisely because it's the deepest integration).

The hard rule, enforced by a lint/boundary check: **outside a plugin's own module,
no code branches on a tool id.** Generic modules ask the plugin, never
`if (tool === 'codex')`.

## 5.1a Ownership: the core, not the app

**Who prepares a project for an AI tool** — writes its instruction file, its skills,
its MCP config, installs its permission bridge — was the **app layer's** job in HS1.
That is wrong for HS2 (maintainer, 2026-08-20): it makes setup impossible headless,
and it re-splits logic the way §4.5 forbids. In HS2 the plugin **registry and all
setup/settings machinery live in the core** ([04-core-server-cli.md](04-core-server-cli.md)
§4.1 `plugins`), reachable from **both** thin binaries:

- **CLI, headless** — `hotsheet setup claude` / `hotsheet setup --detect` prepares a
  project with **no client and no server running**. This is the load-bearing case:
  a purely terminal, server-less workflow can still install instructions, skills, and
  MCP config for whatever tools are present.
- **Server** — the same core code runs when a client asks it to (`POST …/setup/<tool>`).

For a completely clean checkout, `hotsheet bootstrap` composes store initialization,
checkout linking, detected (or explicit) tool setup, and optional existing-remote
configuration into one idempotent headless workflow. Graphical project setup invokes
that workflow instead of maintaining a client-only implementation (HS2-J90FXF).

The **client never implements setup**; consistent with "clients never embed the
core" ([04](04-core-server-cli.md) §4.1), it _requests_ setup through the server API
and renders the plugin's declared `preferences`. What moved is _authorship of the
artifacts_, from the app down into the shared core.

Freshness uses the same ownership boundary (HS2-40HZMB): the headless
`hotsheet setup --refresh` command and the server's non-blocking project-open hook both
invoke the core merge-safe writers. They migrate only settings files that already exist
and refresh detected or previously managed enabled plugins, which repairs stale or
partial setup without opting a clean project into an absent tool. A Hot Sheet hook by itself
counts as managed, so an undetected tool's pre-upgrade hook is refreshed on project open.
Identical output is a
byte-level no-op, and managed instruction markers remain the boundary around content
Hot Sheet may replace.

**Read-only diagnosis (HS2-9N8KVR).** `hotsheet-cli setup --diagnose
[--tool <plugin-id>] [--project <directory>] [--json]` computes the same refresh
plan without applying it or migrating settings. It reports each plugin's enabled,
detected, and previously managed state; current, missing, outdated, preserved, or
disabled instruction, skill, MCP, and hook artifacts; the MCP command and expected
arguments; the checkout registry link; and the exact paths refresh would change.
Human output gives a cause and suggested fix per tool. JSON includes these findings
and a `would_change` array with write/remove actions. The local server and terminal
connection are reported when discoverable. Offline diagnosis cannot prove a tool's
MCP handshake or Codex hook trust; after changed hooks, use the tool's trust review
and restart its session. A malformed registry is reported without invoking its
repairing read path.

**Declared push policy (HS2-H0X9NC).** Shared project setting
`.hotsheet2/settings.json` uses `push_policy: "push-per-ticket"` or
`"push-per-batch"`. Setup writes the per-ticket default when the declaration is
missing; `setup --diagnose` stays read-only, reports the omission, and shows the
exact refresh that will add it. The worklist skill and shared workflow render
the same project policy and standing authorization. Per-ticket setup pushes each
verified commit before the next ticket. Per-batch setup keeps verified commits
until the queue drain or an earlier CI need, then pushes before the session ends.
Projects choosing per-batch record their measured gate or CI cost beside the
declaration as `push_policy_reason`. Changing the setting rewrites only Hot
Sheet's managed policy span, preserving surrounding project instructions.
Setup apply holds the shared Settings lock and re-reads the settings file before
adding a missing policy (HS2-0F2QC2). It preserves keys written after planning.
If the policy changed after planning, apply stops before writing and asks for a
new setup plan so the generated instructions match the policy.

Claude's `.claude/skills/hotsheet/SKILL.md` and Codex's
`.agents/skills/hotsheet/SKILL.md` are fully managed plugin artifacts kept synchronized
with their canonical shared workflows; refresh replaces a stale Hot Sheet adapter while
preserving unrelated user-authored skills and instruction content. The worklist skill and the
instruction block share one declared commit/push policy: every ticket gets at least one
commit, ideally unique to it, whose message names its slug; the push cadence follows
the project's shared `push_policy` setting. Instruction blocks and
skills carry numeric workflow-version markers. Before refreshing either artifact, setup
re-reads both installed targets and compares each marker with its bundled counterpart. If
either installed half is newer, setup preserves both as one workflow bundle while
continuing merge-safe MCP and hook maintenance. An equal-version installed half is also
preserved when its bytes differ outside the managed push-policy span: equal versions identify
one semantic generation, but do not prove that a project-specific adapter or formatting
variant is owned by the current writer. Policy changes update that marked span only.
Semantic bundled changes therefore require a
version bump. This prevents a stale or same-generation long-running server or CLI from
producing a mixed, downgraded, or de-customized workflow. Unversioned and older managed
artifacts still upgrade normally when uncustomized. Bundled skill versions are checked against
the current shared adapters, and Windows detection honors command wrappers from `PATHEXT`,
so freshness does not silently skip npm-installed tools or replace a newer workflow with an
older bundle.

**Shared instruction sections (HS2-329EED).** Several tools can read the same instruction
file — Codex, Antigravity, and OpenCode all read `AGENTS.md`. When the tools being set up
for one file carry byte-identical instruction bodies, setup writes **one shared section**
instead of one copy per tool:

**Versioned shared guidance (HS2-GG03S7).** The four built-in tool manifests opt into
four independently versioned sections from `plugins/shared/`: ticket work,
testing, requirements documentation, and visual QA. First-party setup uses `AGENTS.md`
as their shared source, even for Claude; `CLAUDE.md` imports it with `@AGENTS.md`.
Setup installs each section once per canonical target, even when several tools share it. Refresh
upgrades only an exact known predecessor in `plugins/shared/legacy-*.md`. It preserves nested
project `hotsheet:specifics` text, unrelated content, equal-version edits, unknown older
variants, and newer versions. Disabled tools retire unmodified default sections only
when no enabled tool still uses that target and the section has no project specifics.
Plugin authors opt in with `instructions.shared_guidance = true`; external plugins do not
receive these defaults unless they explicitly request them. Repository-specific policies
remain local to each project.

The maintained reusable section and workflow prose now comes from versioned
[`Small-Tale/ai-playbook`](https://github.com/Small-Tale/ai-playbook) sources.
Hot Sheet ships pinned snapshots at the existing embedded paths; setup and refresh
remain offline and require no Rulesync installation. The source SHA and body hashes
are recorded in `guidance/ai-playbook.lock.json`; CI compares them with that exact
upstream revision. See [snapshot maintenance](../guidance/README.md) for the
reproducible update and legacy migration process (HS2-46J829, HS2-R5V3AZ).

**Canonical instruction imports (HS2-HP1S4H).** A plugin may keep its native
`instructions.target` and declare `instructions.canonical.target` plus the native
`instructions.canonical.import` text. Claude declares `CLAUDE.md`, canonical
`AGENTS.md`, and `@AGENTS.md`. Setup writes the common workflow and sections into
the canonical file and a small managed `hotsheet:claude` import adapter into the
native file. The adapter carries workflow v64 so an older writer preserves its
per-tool block. An existing plain `@AGENTS.md` line is reused. Claude-local content
outside managed blocks stays beside that import.

Refresh moves recognized legacy managed sections from CLAUDE to AGENTS, merges
identical defaults, and carries nested project specifics when the destination has
none or the same specifics. A newer or equal-version customized legacy workflow,
unknown section, malformed markers, or conflicting specifics retain the legacy
layout for manual reconciliation. No customization is guessed away. Repeated
refresh is a no-op. Disabling Claude removes its owned import and membership while
other enabled tools keep the canonical guidance; re-enabling restores the import.
Read-only diagnosis includes both the native adapter and canonical source paths.

**Opt-in Rulesync ownership (HS2-QS38E1).**
`setup --guidance-owner rulesync --project <code-repo>` writes the versioned shared
`.hotsheet2/guidance-owner.json` marker without rewriting instructions. Explicit
setup, CLI/server refresh, and disabled-tool cleanup then leave native and canonical
instruction files untouched while maintaining MCP, hooks, and skills. Diagnostics
name Rulesync as their owner. An absent marker or owner `builtin` uses the built-in
offline writer; malformed or unsupported markers fail safely. Selection, lockfiles,
updates, local customization, migration review, and returning to built-in ownership
are documented in the [validated Rulesync guide](../guidance/rulesync/README.md).
Rulesync remains an optional external tool and is not invoked by normal setup.

**Project-selected rule and skill sources (HS2-2RD1M1, HS2-HBR6ZG).** Settings → Project guidance and
`hotsheet-cli guidance --project <code-repo>` share `.hotsheet2/guidance-sources.json`.
The Settings navigation names the page; its content begins with Instruction Writer,
then guidance recommendations, source updates, and the source and rule controls
(HS2-T0GHX5).
Register `Small-Tale/ai-playbook` (the default URL) or another Git repository at a
tag or commit; registration resolves a full commit, validates the versioned rule
manifest and SHA-256 body hashes, and caches the exact rule and skill files. Later
setup, refresh, and apply use that snapshot offline. Re-registering at another ref
retains selections and known prior bodies, so an exact older managed rule can be
upgraded while local edits remain protected. Rule IDs may be unprefixed or owned by
Hot Sheet, Kerf UI, or another project; owner and tag metadata power the picker.
The picker reports the installed and offered section versions.
Project guidance groups a pinned source's rules by declared owner, with selected
counts and the source's available tags visible while browsing a long catalog
(HS2-019VVA). The main toolbar searches rule names and descriptions
on Project guidance, with `owner:` and `tag:` chips; the Owner and Technology or Tag
selectors remain available for browsing (HS2-6ZA2RN).
The rule list scrolls within Project guidance while its page footer keeps Apply
Selected Guidance and the current rule-and-skill selection count visible. The
action shows a busy state while applying (HS2-VSXE33).
A rule preview reads the pinned snapshot and current installed section without
applying it. For the built-in writer, it uses the same section
renderer as Apply to show the resulting text; for Rulesync it compares the curated
rule input. A protected local edit or newer installed section is called out and
shown beside the offered pinned text. The preview is read-only and does not change
selection or the source pin.

`hotsheet-cli guidance --project <code-repo> check-updates` and authenticated
`GET /checkouts/{reference}/guidance/updates` query source refs without changing
the pinned commit, cached rule bytes, or installed guidance (HS2-RDBVTG).
The JSON results distinguish `current` (a branch still at its pin),
`revision_changed` (a branch moved, including a possible force push), `pinned`
(an immutable SHA or unchanged tag), `tag_moved` (review the tag before trusting
it), and `check_failed` (missing/ambiguous ref or transport failure). A changed
branch is a review candidate, not proof that its history advanced. Each remote
check is bounded to 20 seconds and transport errors are redacted. Checks also
list up to 12 other branch and tag refs with commit IDs, including refs outside
the selected source ref. These are leads for manual review, not claims of release
order, compatibility, or safe updates; larger lists are marked truncated. Ref
discovery is read-only and reports its own failure without hiding the selected
ref's result (HS2-B8M0QB). The command
is explicit; normal setup and apply continue to use the offline pinned snapshot.
Project guidance checks registered refs when the tab opens and caches the result
for 24 hours per checkout and running host/server build. Reopening on the same build
uses a fresh cache entry; opening after a Hot Sheet build change checks again
even within 24 hours. If the build identity is unavailable, the tab checks on
entry. A blue toolbar refresh action opens a dedicated source-updates dialog and
makes a fresh request; the dialog's **Check again** repeats it. A failed check
keeps any previous result visibly stale. A changed branch has a review action
that fetches and verifies the observed commit without changing the pin, then
lists rule and skill additions, removals, and changes with each item's
selection status. **Refresh
reviewed pin** rechecks the requested ref against that exact commit before
registration; if it moved again, repeat the review. **Apply Selected Guidance**
remains a separate explicit operation, and locally changed installed guidance
keeps its existing protection. A selected rule whose installed version differs
from the pin is labeled as pending installation even when the source itself is
current. When a candidate removes selected guidance, deselect and apply those
entries while the old pin is still available, then review the source again.
The headless equivalents are `guidance review --source <id>` and
`guidance register --id <id> --repository <url> --ref <ref>
--expected-commit <reviewed-sha>`, followed by `guidance apply`.

**Older rule revisions (HS2-63D6F9).** A source keeps its current pinned ref and
commit while each rule may choose an older, cached version by its body SHA-256.
Project Guidance shows the chosen version beside the available and installed
versions. Enter a full reviewed commit SHA in that rule's Older Version Commit
field to cache its earlier manifest entry, then choose it from Rule Version,
preview, and apply. The cache operation verifies that the ref still resolves to
the expected commit, checks the manifest and rule body hash, and requires the
offered rule version to be lower than the current pin. The chosen digest and
historic snapshot persist across a source refresh. Choosing the source pin
restores the current rule. An explicit older choice can replace a known newer
installation; local edits still require review. The headless equivalents are
`guidance cache-rule-version --source <id> --rule <name> --ref <sha>
--expected-commit <sha>`, `guidance choose-rule-version --source <id>
--rule <name> --sha256 <body-sha>`, and `guidance apply`. Omit `--sha256` to
restore the source pin. The source manifest may declare
`minimum_hotsheet_version` as a semantic version per rule. Selection, preview,
and apply reject a rule whose minimum exceeds the running Hot Sheet version;
older manifests without that field remain compatible. Separate source refs and
source connections are covered by HS2-1YXCCW.

**Several source refs in one review (HS2-1YXCCW).** Project Guidance accepts
several source rows, each with comma-separated branch names, tag names, or full
commits. A trailing `*` expands matching tags only; the review rejects empty,
duplicate, or overlarge expansions. It shows every exact full commit and the
verified rule and skill counts before registration. Registration resolves the
refs again and rejects a changed set, then writes all pins in one config update.
Each expanded ref receives its own stable source id under the row's group id;
existing one-ref source ids remain valid. Selections and cached older rule
digests stay with the matching ref across refreshes. Applying guidance remains
a separate action, and selecting the same rule from two refs still fails the
existing cross-source collision check. Headless clients write an array of
`{id, repository, refs}` to JSON and run `guidance review-batch
--sources-file <file>`; save its JSON output and pass it to `guidance
register-batch --review-file <file>`.

Bundled Hot Sheet shared guidance is a separate offline channel. On setup refresh,
an exact known predecessor upgrades automatically. If a locally changed older section
cannot be safely replaced, the setup report emits `bundled_guidance_blocked` with
its project-relative path, section, and installed/bundled versions; Project guidance
shows a review notice. `hotsheet-cli -C <ticket-store> setup --diagnose --project
<code-project> --json` exposes the same case in `guidance_notices` without writing.
Compare the named section with `plugins/shared/<section>.md`, reconcile local
changes deliberately, then run `hotsheet-cli -C <ticket-store> setup --refresh
--project <code-project>`. An unchanged section, a known legacy body that refresh
can upgrade, an equal-version local edit, or Rulesync-owned instructions do not
claim a newer blocked bundled update (HS2-29BHX8).

Standalone Rulesync projects with `rulesync.jsonc` and no
`.hotsheet2/guidance-sources.json` have a separate read-only lock check:
`hotsheet-cli guidance --project <code-repo> check-rulesync-updates`. Project
guidance shows the same current, outdated, unlocked, or unknown source-lock
state, with the installed Rulesync version reported separately. The checker
requires an installed Rulesync 38.0.0 executable and runs
`rulesync --json install --outdated`; it never invokes `npx`, writes the lock,
or generates instructions. Review `rulesync.lock` and the generated diff before
explicitly running `rulesync install --update` and `rulesync generate`. Hot
Sheet-managed pins use `guidance check-updates` instead (HS2-SFMZVJ).

`guidance list` lists source, version, owner, tags, selection, and installed version
as JSON; `guidance register --id <id> --repository <url-or-local-repo> --ref <tag-or-sha>`
pins a source; `guidance select --source <id> --rule <name> [--skill <name>]` replaces
that source's selection; `guidance owner builtin|rulesync` chooses the instruction
writer. Switching to Rulesync saves unmarked `AGENTS.md` prose and nested project
specifics in `.rulesync/rules/project-local.md` before changing ownership. A
conflicting local rule or incomplete managed marker stops the switch for review.
Project guidance lists local Rulesync rule names and paths separately from pinned
source rules.
When a project switches to Rulesync or an applied Rulesync selection changes
`AGENTS.md`, Hot Sheet queues one open `Consolidate Rulesync project guidance`
task in its linked writable ticket source (HS2-34S102). Repeated changes reuse the open
task; after it is completed, a later change creates a new one. The task asks an
AI worker to compare generated and local guidance and review redundant text.
Hot Sheet also detects changes made directly with Rulesync to `AGENTS.md` or
`rulesync.lock` on the next standalone update check and queues the same task.
Only a content digest is stored in the machine-local Hot Sheet home. Hot Sheet
does not remove guidance automatically.
If no writable source is linked yet, the digest stays pending and Project guidance
shows a review notice. Linking or selecting a source retries ticket creation
immediately (HS2-C9FC2K); the standalone check remains a headless retry path.
Source linking still succeeds if that follow-up cannot be written, and the notice
persists until a later successful retry.
`guidance apply` reconciles the selected files. With the built-in writer,
selected rule sections are merged into `AGENTS.md` and exact deselected bodies are
removed. With Rulesync ownership, `apply` materializes the selected pinned rules in
`.rulesync/rules/.curated`, writes `.hotsheet2/rulesync.generated.jsonc`, and invokes
Rulesync to generate `AGENTS.md`. Detected tools with a skill directory receive
selected source skills. The five bundled Hot Sheet rules remain the offline default
until an explicit source selection; installed tool workflow instructions remain
managed by tool setup.

**Project guidance recommendations (HS2-EECHKV design).** A project may ask Hot Sheet
to suggest which available rules and skills fit its codebase, both during first
setup and later as the project changes. This is a review layer over the existing
pin → select → apply workflow, not another instruction writer. The first version
uses the bundled guidance and the exact snapshots of sources already registered
for the project. A recommendation may suggest registering another source, but
must not fetch, trust, or install it without the existing source-review step.

The headless recommendation scan and the Settings review use the same project
service. The scan records bounded, explainable signals such as recognized manifest
names, declared languages and frameworks, configured AI tools, guidance owner,
selected and installed rule versions, and the pinned source commits. It never
executes project code or follows a link outside the checkout. It does not send
file contents, environment values, credentials, or untracked private files to an
AI service. An optional AI interview can ask a small number of specific questions
when those signals cannot settle a recommendation (for example, whether a
framework is actively used or only present as a transitive dependency). Show the
exact context that would leave the machine and obtain an explicit choice before
sending it. With that choice, AI may rank and explain available candidates and
ask those questions; it cannot introduce an unverified rule body. The local scan
remains usable without AI or network access.

Each recommendation identifies the source and pinned version, evidence, expected
benefit, possible conflict with current guidance, and whether it is already
selected or installed. The user can accept or dismiss individual suggestions,
including only part of a set. Accepting changes selection; applying still uses
the current explicit `guidance apply` operation and its local-edit protection.
Neither an AI answer nor a re-evaluation may replace a pin, change the guidance
owner, rewrite instructions, or undo a deliberate selection. A project with no
registered source retains its bundled default guidance and can review a separate
source-registration proposal; no empty-source state should imply that external
guidance was trusted or that a bundled rule is individually selectable.

Re-evaluation is read-only on Project guidance entry or an explicit headless/UI
check, with a bounded cache and no timer-based network polling. Compare a stable
fingerprint of relevant project signals, source pins and versions, and prior
decisions. Project Guidance exposes a toolbar AI Recommendations action, including
the narrow toolbar overflow menu. Its dialog presents loading, empty, result, and
offline/unknown states, recheck, and individual decisions; the recommendation
review does not occupy the settings form (HS2-WEKNRD). Show a “New guidance
recommendations are available” notice in that dialog only when
the candidate set or a candidate's evidence materially changes since the last
review. Record accepted and dismissed decisions by stable source/rule identity,
version, and evidence fingerprint so reopening the tab does not repeat a rejected
pitch; a changed version or changed evidence can be offered again with a clear
reason. The notice opens a diff of new, changed, and withdrawn suggestions and
allows partial acceptance or dismissal. Withdrawal never removes installed
guidance automatically. Keep the review record separate from the source lock and
avoid storing raw project-file content in it.

The headless path needs JSON output for the evidence, questions, decision state,
and review diff, plus explicit commands to record decisions and apply selected
guidance. The Settings path must expose the same states and reasons, including
offline/unknown results, without making the AI interview a setup requirement.
Cover first setup, no source, multiple pinned sources, conflicting rules, manual
selection overrides, accepted/rejected/partially accepted sets, evidence changes,
version changes, stale scans, and restart before shipping recommendations.

The headless local service (`HS2-JZ8W32`) is available through
`hotsheet-cli guidance --project <code-project> recommendations` and
`guidance recommend-decide --source <id> --kind <rule|skill> --name <name>
--fingerprint <reviewed-fingerprint> --decision <accept|dismiss>` under the same
`--project` scope. The authenticated checkout route is
`GET/POST /checkouts/{reference}/guidance/recommendations`. Both return JSON with
bounded local signals, candidates, and new/changed/withdrawn review changes.
Decisions use project Shared Settings; a stale fingerprint fails and an accepted
item is selected without running `guidance apply`. The initial scanner recognizes
common language manifests and selected `package.json` dependencies, and matches
them to rule tags or skill names in registered pinned sources. It reports when a
source needs registration, without doing a remote fetch. It also warns when the
first explicit selection would replace the bundled default at the next apply.
Project guidance Settings loads that local report on entry, exposes the evidence and
new/changed/withdrawn review diff, and lets people accept or dismiss individual
suggestions. Accept changes selection only; applying remains an explicit action.
The Settings view reports an unknown scan or decision error without changing a
selection. When local evidence has a concrete ambiguity, the optional interview
preview (`hotsheet-cli guidance --project <code-project>
recommend-interview-preview` or authenticated
`GET /checkouts/{reference}/guidance/recommendations/interview`) shows the exact,
bounded prompt. It contains allowlisted technology labels and pinned candidate
identities, never manifest values, rule bodies, source URLs, or descriptions. The
user explicitly chooses to share the preview, then copies it to an AI tool of
their choice and pastes back final advisory JSON. The `recommend-interview-open`
and `recommend-interview-submit --answer-file <json>` CLI commands provide the
same headless path; the authenticated route has `/open` and `/answer` POST
actions. The review state survives restart in project Shared Settings and resets
when evidence or a pin changes. Hot Sheet does not send the prompt itself. AI
advice is bounded to listed candidate IDs and cannot register, select, or apply
guidance. The local scan remains available without a network or AI tool.

The visual QA section (HS2-7WPJQ2) requires direct review of changed UI at wide and
narrow sizes and a matching attached after image when the ticket contains visual evidence.
The shared hotsheet skill carries the same review rule itself, so an `AGENTS.md`-only
project can apply it without a `CLAUDE.md` file.

The testing section (HS2-QG215V) also applies in fresh Claude and Codex projects:
browser fixtures must match the real API contract, new API compositions need a real-server
flow, stateful controls must be tested in both directions across resets, and actions must
work through each shipped parent composition. Project test runners and paths belong in
local `hotsheet:specifics` blocks, which refresh preserves.

The lint and coverage rule (HS2-DHYWSD) asks each code package for a working local lint
command and configuration, warning-free results, and documented suppressions. Teams keep
a feature-by-feature unit, end-to-end, and manual coverage record and a manual plan for
behavior automation cannot verify. The shared target is 100% line, branch, and documented
feature coverage, treated as a floor for transition and interaction tests. Exact lint
stacks, coverage paths, validators, and thresholds stay project-specific. Refresh knows
both earlier testing-section versions and retains their nested specifics while upgrading.

The bundled instruction block's Testing list (instructions v59, HS2-CP63FG) asks agents to
keep test output quiet: run failure-only reporters, or the project's quiet wrapper when it
has one, and read the summary and failure excerpts rather than the full log. Hot Sheet 2's
own wrapper is `scripts/test-quiet.mjs` ([12](12-code-organization-and-testing.md) §12.7.8).

Risk-tiered testing (HS2-GC3D2X) arrives as testing section v5 and instructions v60. The
testing section opens with three tiers. **T0 trivial** needs lint, typecheck, and directly
related tests. **T1 normal** runs the affected tests, adds one regression test per fix, and
gives new features double coverage. **T2 high-risk** gets the full treatment and the full
affected suites, and the section names generic T2 surfaces (merge/sync, claims, indexes,
formats/parsers, shared components, auth, migrations) for projects to extend. The
instruction block asks the `## Preliminary thoughts` note to declare the tier, or the claim
note for trivial work. It also asks agents to judge the completion confidence's
verification factor against that tier. Refresh upgrades the exact v4 testing section.

Instructions v61 (HS2-AEE51Y) add rerun discipline to the Testing list. After a failure,
agents rerun only the failing tests, then the affected set once. A rebase that does not
touch the ticket's area needs no rerun, and a passing suite is never rerun just to
double-check.

The ticket workflow section (HS2-PDD7XG) includes project-neutral note guidance: use
activity history for meaningful subtask starts and finishes, preserve conclusions in
regular Markdown notes, pair AI activity with a short `note_summary`, and send multiline
CLI Markdown through `--note-file`. Setup installs it for Claude and AGENTS-based tools;
refresh upgrades the exact prior v4 section while preserving nested project specifics.

The requirements section (HS2-0Y6M00) asks for a codebase map and status-marked
requirements summary at project-chosen relative paths. Fresh setup installs the rule but
does not invent project facts or files; the first relevant change creates any missing
synthesis document and links it from the documentation index. Refresh upgrades the exact
v2 section and retains local paths inside `hotsheet:specifics`.

```markdown
<!-- BEGIN hotsheet:agents-md -->
<!-- hotsheet-shared-section: antigravity, codex, opencode -->

…the shared instruction body…
<!-- END hotsheet:agents-md -->
```

- The section key is the target path lowercased with non-alphanumeric runs folded to `-`
  (`AGENTS.md` → `agents-md`). The second line lists the served tool ids, sorted; it is
  membership, not content.
- A shared section is used once two or more tools write the file, or when the file already
  has one. A single tool writing a clean canonical file keeps its per-tool
  `hotsheet:<tool>` section, so nothing churns for unshared files.
- Setup and refresh **migrate** existing per-tool copies of the writing tools into the
  shared section — placed where the existing shared section or the first per-tool copy was,
  otherwise appended — and remove those copies, including duplicates. Content outside the
  managed markers is preserved, and a second run is a byte-level no-op.
- Membership: explicit `setup <tool>` only adds that tool to an existing shared section. On
  `setup --refresh` and the server's project-open refresh, the served list is rebuilt from the
  refreshed tools. A tool listed in a shared section counts as previously managed, so
  disabling one sharer (removing it from `enabled_plugins`) drops it from the list while the
  section stays for the others. When no listed tool remains enabled, refresh removes the
  section. A disabled tool's _per-tool_ section leaves the same way (HS2-FKC8VN): when
  `enabled_plugins` is set and excludes a tool, refresh removes that tool's
  `hotsheet:<tool>` section while preserving everything outside the markers. With no
  `enabled_plugins` setting nothing counts as disabled, so refresh never removes anything.
  When removing a per-tool or shared section leaves the instruction file empty or
  whitespace-only (setup created the file for that section, as with `CLAUDE.md` in a clean
  project), the file itself is deleted and reported as removed, pruning parent directories
  the deletion leaves empty. Any remaining user content keeps the file (HS2-G9CD1W).
- **An explicit empty list disables every tool (HS2-8B3VJP).** `enabled_plugins` is read
  the same way by `hotsheet setup` and the server's project-open refresh
  (`hotsheet_plugins::enabled_plugins_from_setting`):

  | Setting value               | Meaning                                                          |
  | --------------------------- | ---------------------------------------------------------------- |
  | absent or `null`            | no restriction — refresh only adds or repairs                    |
  | `[]`                        | no tool enabled — refresh removes every tool's managed artifacts |
  | `["codex", …]`              | only the listed ids are enabled (non-string entries are ignored) |
  | any other value (malformed) | treated as absent, so a corrupt setting never deletes setup      |

  With `[]`, `setup --detect` fails with a "no detected AI tool is enabled" error instead
  of "none detected", and `bootstrap` reports that nothing is enabled and still completes
  project/store setup. No Hot Sheet client or server writes `[]` as a default; the key is
  omitted until a project opts into a list. Explicit `setup <tool>` ignores the list.

- **Disabled tools leave no managed artifacts (HS2-ZTGX6P).** Refresh also removes a
  disabled tool's other wholly Hot Sheet-owned artifacts, so it stops counting as
  previously managed and cannot be half re-enabled later:
  - its worklist skill file (pruning directories the removal leaves empty);
  - its `hotsheet` MCP server entry — only when that entry's command is Hot Sheet's MCP
    binary. Unrelated servers and settings in a shared config stay; a config left holding
    nothing Hot Sheet did not write is deleted, along with the `.git/info/exclude` line
    setup added for it;
  - its Hot Sheet permission hooks. The user's own hooks and settings stay; an event list
    the removal empties is dropped, and a config left empty is deleted with its exclude line.

  Ownership follows setup's rules. The instruction section and skill are one workflow
  bundle: if either installed half is newer than the bundled one, or an equal-version
  customization, both are kept (the tool then still counts as previously managed), while
  MCP and hook removal proceeds. Older and unversioned halves are removed. A path an
  enabled tool also declares is never removed. Re-enabling a detected tool makes the next
  refresh write the complete layout again; an undetected tool needs an explicit
  `hotsheet setup <tool>`, because nothing marks it as previously managed any more.

- **Refresh reports what it removed (HS2-CAM9J5).** The core refresh returns a
  `RefreshReport`: the tools it set up, plus one `RemovalReport` per disabled tool that had
  something to remove, listing project-relative paths it `removed` (deleted outright) and
  `edited` (kept, with Hot Sheet's section, MCP entry, hooks, or shared-section membership
  dropped). `hotsheet setup --refresh` prints them under
  `Removed disabled <tool> setup in <project>:` as `removed <path>` / `edited <path>` lines,
  and prints `Setup is current; …` only when nothing was set up or removed. A repeated
  refresh reports nothing further. The server's project-open refresh computes the same
  report but does not surface it yet (HS2-W3EZNE).
- Version preservation applies to the shared section's **body**. For every tool targeting
  the file, a newer shared body, or an equal-version body whose bytes differ, freezes that
  tool's instruction/skill bundle — even when it also has a stale per-tool copy — exactly
  like the per-tool rules above. Older and unversioned shared bodies upgrade in place.
- If the tools writing one file have **different** bodies (for example a third-party plugin
  with its own instructions, or a version-skewed plugin tree), each keeps a per-tool section.
  Those tools leave any existing shared section's served list, and a shared section left
  with no members is removed. Bundled first-party bodies are identical by construction
  (`every_plugin_shares_one_instruction_body`), so they always share.

In the source-backed web development bridge, the compiled CLI reports a digest of all
embedded setup assets and the bridge independently hashes the live `plugins/` tree. A
missing or mismatched digest refuses project setup with a rebuild command before any
managed file can be written; this covers instruction and manifest changes in addition to
the explicit skill-version check.
Machine-specific integrations never alter the shared ignore policy: Claude's permission
hook uses `.claude/settings.local.json`, Codex's uses `.codex/hooks.json`, and both are
added only to that checkout's `.git/info/exclude`. A newly created, wholly Hot Sheet-owned
MCP config is excluded the same way. A pre-existing config with unrelated user content
remains visible to git while the Hot Sheet entry is merged into it. That shared entry
uses `hotsheet-mcp --project .`: the shim resolves the code project and opens its
configured sources through the machine-local server, keeping the committed config
free of local binary and ticket-store paths (HS2-TSXSH6). The tool must find
`hotsheet-mcp` on its launch `PATH`; if its GUI environment does not, install the
shim on that `PATH` or keep its MCP entry in a local-only tool config. In a linked git
worktree the entry goes to the common directory's `info/exclude` (via the gitdir's
`commondir`), because git never reads a per-worktree exclude file (HS2-8XTDZP).

Launched-agent guidance handles sandbox denials explicitly (HS2-DVZXSD).
A sibling `<repo>.hs2` ticket store can be outside the command sandbox even
when its configured MCP shim can reach it. When `hotsheet-cli` reports
`Operation not permitted (os error 1)` opening the store, the agent uses the
equivalent `hotsheet_*` MCP operation without treating the store as corrupt.
If Chromium or WebKit fails to launch with EPERM or a mach-port denial, the
agent reruns that browser gate through approved unsandboxed execution. Without
that approval or a browser, the gate remains blocked and is never reported as
passing. Setup renders this recovery path into the shared instructions and the
Claude/Codex worklist skills; it does not widen filesystem access automatically.

All bundled instruction and ticket-workflow artifacts teach **portable durable
references** (HS2-ERKA8N). AI-authored documentation, ticket text, notes, and completion
summaries use repository-relative paths inside the current checkout. References to another
repository use its stable name and canonical URL when helpful, or a technical placeholder
such as `<repo-root>/path`; a developer's home directory, username, or absolute clone
location is not presented as shared project structure. An exact local path is retained only
when the path itself is indispensable evidence for a machine-local diagnostic.

AI-authored ticket notes are also optimized for **human scanning** (HS2-MBJX7C). The
bundled instructions, Hot Sheet skills, and derived `worklist.md` tell agents to lead with
the outcome rather than a chronological transcript; substantial notes use short Markdown
sections for results, verification, and follow-ups, with bullets for parallel facts and
tables only when they clarify a dense comparison or timeline. Simple updates stay brief,
empty sections are omitted, and raw logs or multi-part results are not left as one dense
paragraph. Multiline CLI notes use `--note-file` so the intended Markdown structure reaches
the ticket intact.

Thumbs feedback on AI-authored notes is saved as an ordinary ticket note. It does not
immediately change agent behavior; [the proposed feedback synthesis process](23-ai-feedback-synthesis.md)
describes how a maintainer can periodically review recurring themes and approve concise
repository guidance (HS2-355565).

Before implementing a ticket that is not trivially simple, the bundled instruction blocks
and Hot Sheet skills ask the AI to post its **preliminary thoughts** (HS2-C4X2MD): a short
`regular` note headed `## Preliminary thoughts` with its understanding of the problem or
likely root cause, the planned approach, the main risks or open questions, and how it will
verify the work. People can steer early, and a later reader sees the starting reasoning. A
quick, obvious change skips it, and it never replaces a `FEEDBACK NEEDED` blocker. Like
confidence, this is prompt-level guidance.

The bundled instruction blocks and Hot Sheet skills also require **integrating worktree and
background-agent work before completion** (HS2-YVV6A0). Work done in a git worktree, on a
side branch, or by a sub-agent or background worker is not done until it is merged, rebased,
or cherry-picked onto the branch the project ships from (or proposed through its pull-request
convention). The commit must be reachable there (`git branch --contains <sha>`), pushed where
the repository pushes, and gated. The completing note names the integrated commits, and the
worktree is removed afterwards. A delegated worker's report of completion is not completion:
the owning agent verifies the integration. An integration that fails or needs a decision
leaves the ticket `started`, with the branch, worktree path, and commit recorded. Agents that
share one checkout stage only their own files and check the staged diff for foreign changes.
This guidance shipped as instructions version 56 and skill version 57, so a setup refresh
updates existing projects.

Every AI completion also reports a **completion confidence** (HS2-DWTJ43). The bundled
instruction blocks and Hot Sheet skills require the note that moves a ticket to `completed`
to include a `## Confidence` section with an integer score (0-100) and one line per rubric
factor, each rated high/medium/low with a short phrase: clarity of the request; context and
supporting information available; comprehensiveness and realism of verification (unit, E2E,
real-browser visual QA; actually ran vs. assumed); scope deviation or unverified assumptions;
and known gaps deferred to follow-ups. The same integer is passed as structured metadata
(`--note-confidence` / MCP `note_confidence`) so clients never parse prose. Anchor bands:
90-100 fully verified end to end against the real system; 70-89 verified with minor
assumptions; 40-69 partially verified or an ambiguous ask; below 40 largely unverified, with
the gaps named. A bare number without the factor lines is non-compliant. Self-reported
confidence is weakly calibrated, so the factor lines — not the number — are the primary
value. External providers carry the score as a comment trailer (HS2-5YNASC).

**Actor role and the AI completion rule (HS2-RD4M29).** Every mutating surface accepts an
optional actor: CLI `--actor-role human|ai|system --actor-id <id>` (or the
`HOTSHEET_ACTOR_ROLE` / `HOTSHEET_ACTOR_ID` environment), MCP `actor_role` / `actor_id` on
every mutating tool (or the same environment in the shim), and an `actor:
{"role","id"}` object on server request bodies. A missing actor is _unspecified_, never
assumed AI, so older callers are unaffected.

Every AI session that Hot Sheet launches (`hotsheet-cli launch`, AI terminals, and client
drives) receives `HOTSHEET_ACTOR_ROLE=ai` and `HOTSHEET_ACTOR_ID=<worker id>`. Its CLI and
MCP mutations therefore act as `ai` automatically. This is the one deliberate behavior
change for existing callers: those sessions are AI by construction, so they now follow
the rule the bundled instructions already required.

External AI clients that Hot Sheet did not launch opt in through setup (HS2-28DM8B). Every
MCP config that `hotsheet-cli setup` generates declares `HOTSHEET_ACTOR_ROLE=ai` in its
`hotsheet-mcp` server environment: `env` in Claude-style `.mcp.json` and Codex TOML, and
`environment` in OpenCode. Their MCP mutations therefore act as `ai` too. MCP configs are
read only by AI clients, and an explicit `actor_role` argument still wins.

Rules are role-specific, and their feedback is written for the reader:

- **AI completion rule:** an `ai` actor that moves a ticket to `completed` (edit, batch,
  close as `completed`, or a provider route) must record a confidence score, either in
  the same request or earlier in the current completion cycle.
- **Refusal:** otherwise the write is refused before anything changes. The server and MCP
  answer 422 with the stable code `confidence_required`; the CLI exits non-zero with the
  same text. The message is machine-actionable: it gives the exact retry for each surface,
  the `## Confidence` section shape, and the rubric bands, so one retry succeeds.
- **Humans:** human and `system` actors are never required to score.

The actor is also recorded (HS2-32QDZ3):

- **What is stamped:** the author of every note appended through the git provider, and of
  the status-transition activity an edit or a completed close produces. It is stored as
  the `actor:` / `actor_id_hex:` note-marker tokens and exposed as `ApiNote.actor`.
- **What is not stamped:** claim-driven Started transitions stay unattributed; the claim
  history already names the worker. Editing a note keeps its original author.
- **External trackers** attribute comments to their own authenticated account, so Hot
  Sheet does not write a separate actor trailer there.

**The web client is a human actor (HS2-XF81CJ).** It adds `actor: {"role":"human"}` to
the bodies of the actor-aware ticket routes: update, close, and batch. A checkout batch
applies its single top-level `actor` to every update that does not name its own.

- **Effect:** people are recorded as the authors of their notes and status changes, and are
  never held to AI-only rules.
- **Paste:** pasting a copied ticket keeps each note's original author, and sends `null`
  for unknown authorship instead of claiming the paster wrote it.
- **Still open (`HS2-XF81CJ`):** which further AI-only rules to enforce, and how
  human-facing refusals should read, wait on the maintainer's decision.

The bundled instruction blocks tell an AI to identify itself as the `ai` actor. Further
role-specific rules and human-tailored feedback are follow-up work.

**Calibration (HS2-RD4M29).** `hotsheet-cli confidence-report [--json]` reads each
ticket's own history (`hotsheet-ticketing/src/calibration.rs`):

- **Events:** every completion is one event, scored with the newest score of its cycle.
  Its outcome is _reopened_ if a later reopen or Not Working report ends the cycle,
  _verified_ if it reached Verified first, and _pending_ otherwise.
- **Per-band totals:** for each rubric band (and unscored completions), the report gives
  counts, the reopen rate over resolved completions, and the mean score.
- **Reading it:** a well-calibrated rubric reopens less as the band rises, so these numbers
  are the evidence for tuning the anchors. The JSON lists every event for drill-down.
- **Other surfaces (HS2-Q1WCCY):** the same report is served by `GET /confidence-report`
  (default store), by `GET /checkouts/{reference}/confidence-report` (every git store the
  checkout links), and by the read-only MCP tool `hotsheet_confidence_report` (optional
  `checkout`), with or without a server. The web shows it in a project's statistics view;
  external trackers are excluded because their reopen history lives in native events.

Beyond the ticket-command reference, each bundled instruction block (`plugins/<tool>/
instructions.md`) carries a compact, **project-neutral** default policy so a brand-new
project that never customizes its `CLAUDE.md`/`AGENTS.md` still gets strong defaults
(HS2-3JMMAZ): ticket direct-terminal work by default (not only queue-driven work), create
every follow-up immediately, double (unit + E2E) test coverage with transition-matrix/
adversarial tests for stateful code, keeping requirements/docs in sync in the same change,
and per-ticket commit hygiene — while explicitly **leaving the push/PR decision to each
repository** rather than mandating it. The four first-party blocks share one body; the
`every_builtin_carries_the_full_default_guidance` test pins that content.

The blocks and the worklist skill also make **releasing a claim the rule on every exit path**
(HS2-M3F0MS): an AI releases its claim the moment it stops working a ticket for any reason
(completion, a blocker or `FEEDBACK NEEDED`, handoff, switching tickets, deferring, running
out of time or budget, or ending its turn), leaves the status alone with a note on where it
stopped, and checks `hotsheet-cli ls --claimed` before its final response so no stale claim
falsely signals live work. `every_builtin_tells_ais_to_release_claims_whenever_they_stop`
pins that guidance.

**Launcher-owned worker ids** are the mechanical safety net behind that guidance
(HS2-1VAW1C). A launcher that starts an AI session gives it a worker id,
`<tool>-<session>`, in `HOTSHEET_WORKER_ID`. The blocks and skills tell the AI to claim with
that id whenever it is set. When the session ends, the launcher releases every claim the id
still holds, leaving other workers' claims and every ticket's status alone. The release
scans the store resiliently: an unreadable ticket elsewhere, whether corrupt or caught
mid-write by another process, is skipped rather than aborting every other release
(HS2-JRPA23):

- **Connect terminals:** a server `connect` terminal uses its terminal id as the session. When
  the terminal exits or is deleted, the server releases the id's claims in every hosted
  store. This works both in-process and with the detached terminal broker. Opening the same id
  after its PTY exits creates a new AI connection and session-push registration; attaching to
  a still-live PTY keeps its existing registration.
- **Shell and command terminals:** every other server terminal gets `terminal-<terminal id>`,
  so a tool a user starts in it by hand claims under that id. The server releases the id's
  claims when the terminal exits or is deleted (HS2-RXWXQ8). It also releases them whenever
  a command the user started, such as an AI tool, exits back to the prompt while the shell
  stays open (HS2-WQQYT1). The signal is the PTY's foreground process group changing from the
  command back to the shell, checked every 500 ms on Unix, so it needs no shell integration. A
  claim in that terminal therefore lasts only as long as the foreground command that took it.
  On platforms without the signal, the terminal's exit still releases the claims.
- **After a server restart:** each terminal records the worker id from its launch
  environment and the detached broker reports it. On startup the server resumes the monitors
  of terminals that survived in the broker: an AI terminal gets its busy feed and exit
  release back, a shell terminal its exit release. A terminal hosted by a broker too old to
  report the id still relies on lease expiry.
- **AI chat drives:** a client-owned drive (`POST /drive/connections`) gives its tool
  `<tool>-<connection id>`. The session spans turns, so the claims are released when the
  drive is closed: at once for an idle drive, or when the interrupted turn finishes for a
  busy one. Drives live only in server memory, so the server records open drives in its
  session catalog (`live_drives`). On startup it releases the claims of any drive the
  previous run left open, opening each drive's store directly (HS2-VFXEF4).
- **External launches:** `hotsheet-cli launch <tool>` now spawns the tool and waits, instead
  of replacing itself. It ignores the terminal's interrupt, quit and hang-up signals, which
  belong to the tool, so it can release the session's claims even when the terminal window
  closes. It then exits with the tool's exit code, or `128 + signal`.
- **By hand:** `hotsheet-cli release --all --worker <id>` is the same release, headless.

Lease expiry remains the backstop for anything a launcher cannot observe. That includes a
command shorter than the 500 ms foreground check, a server that crashes before recording a new
drive, and a session that picked its own id.

**Which set of artifacts** to write is determined by **which plugins are active** —
so "core-owned setup" and "external loadable plugins" (§5.12) are the same
capability seen from two sides: the loader decides _what_ tools exist, the setup
capability decides _what each writes_, and either binary can drive it.

Capabilities divide by lifetime, and that division is what makes headless work:

| Bucket                      | Capabilities                                                                                  | Runs in                                                                     |
| --------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| **One-shot, host-agnostic** | `setup`, `instructions`, `skills`, `mcp`-config, `permissions`-install, settings read/write   | **CLI or server** — idempotent filesystem writes, no persistent host needed |
| **Persistent, server-only** | terminals/PTY, `drive`/trigger, busy tracking, connection registry, runtime permission bridge | **server** — needs the always-on host (§5.4–§5.7)                           |

The one-shot bucket is exactly the headless case. The persistent bucket is
unchanged — it always needed the server.

## 5.2 The general interface the ticket asks for

The ticket enumerates what the AI-tool interface must cover. Mapped to the design:

| Requirement                                                  | Where it lives                |
| ------------------------------------------------------------ | ----------------------------- |
| Initialize AI tools in terminals (MCP + similar connections) | §5.3 `setup` + §5.4 terminals |
| List AI-tool connections                                     | §5.6 connection registry      |
| Trigger commands to a target connection                      | §5.5 drive/trigger            |
| Permission checks & other user prompts                       | §5.7 permission bridge        |
| Track AI-tool busy-ness                                      | §5.6 busy tracking            |
| Carry over other useful concepts from HS1                    | §5.9                          |

## 5.3 A plugin's shape

A plugin is split into a **declarative half** (plain data — client-safe, no
filesystem) and a **behavioral half** (needs the host: fs, processes) — the split
HS1 discovered is unavoidable because the registry is reachable from client code.

**Declarative (data):**

- `id`, `displayName` (short, for running text: "Codex finished"), `productName`
  (full, for menus: "Codex CLI").
- `tier`: `cli-agent` (Hot Sheet drives it) vs `editor` (Hot Sheet only supplies
  context files).
- `maturity`: `stable | beta | unreleased` — a property of the _integration_, not
  a per-project setting; controls whether it ships to users at all.
- `detection`: `{ binaries, paths }` — evaluated by the host, not a closure, so the
  registry stays client-safe.
- `transport`: which drive transport this tool speaks (identity, not behavior —
  HS1 §132.11.7 learned this the hard way when a client "mirror" of transports
  drifted).
- `preferences`: declared per-tool settings (e.g. "interactive permissions
  on/off"), rendered by a shared settings renderer — no hand-written UI per tool.

**Behavioral (host-side, keyed by plugin id):**

- `instructions`: the managed instruction file (`CLAUDE.md` / `AGENTS.md` /
  `GEMINI.md` / a rules file) + frontmatter + adapter-family flag.
- `skills`: generate/refresh the worklist skill/rule artifact; report which
  artifact answers "is this tool prepared?" The shared Hot Sheet workflow asks agents
  doing user-visible UI work to attach representative real-browser captures across the
  changed states and relevant viewport sizes, then reference those attachments by name
  in the ticket's development and completion notes.
- `command`: the binary + how a launch line resolves (channel flags, model
  variants).
- `drive`: the transport implementation — run one turn; optional
  interrupt/reset/prestart/busy; optional long-lived backing service.
- `permissions`: install/remove the tool's permission bridge, merge-safe.
- `mcp`: write the `hotsheet-*` MCP server into the tool's config, in that tool's
  format (JSON / TOML / ACP session field).
- `metrics`: report **usage / cost metrics** from the tool's telemetry, mapped to a
  **unified metrics interface** all plugins conform to (HS2-46/HS2-69). Only
  usage/cost is kept — the HS1 debugging telemetry (span trees / tracing / waterfalls)
  is dropped. A tool without telemetry simply omits this capability.
- `activity`: emit **tool-agnostic progress / summary events** ("started ticket X",
  "edited file Y", "finished") that the **Announcer** (narration + TTS) and a timeline
  consume — a common cross-tool interface so narration isn't Claude-only (HS2-48).
  Design: HS2-70. A tool that exposes no activity stream omits it.

> **Cross-cutting theme:** `drive`, `metrics`, and `activity` are all _seams that
> generalize one concern across every tool_. Designing these capability interfaces
> well (and early, HS2-67/HS2-69/HS2-70) is what keeps adding a tool cheap — the
> §5.10 testability rule + conformance suite hold each to its contract.

**Absence is the signal.** A missing capability means "not supported" — there is
no `supportsDrive: false` boolean to drift from reality. Gemini has no drive;
Goose is only identity + a command; editor tools have no runtime. A missing method
can't be called by mistake.

**The host carries the machinery.** If two plugins would write the same code, that
code is a host helper, not per-plugin: the merge-safe hooks-file writer, managed
instruction sections, the adapter skill-tree writer, the MCP-config primitive, the
permission bridge, PTY/stdio framing, the commands-log emitter. A plugin _declares
what's specific_ and _calls host helpers for the rest_ — a common-shape tool
(AGENTS.md + a skills tree + a hooks file + a spawn drive) is nearly declarative.
That "nearly" is the whole external-plugin story (§5.12): for most tools a plugin
is _pure data_, which has no ABI problem and needs no code boundary.

**"The host" is whichever binary runs the capability** (§5.1a), not "the server."
The one-shot helpers above (instruction sections, skill-tree, MCP-config,
permission-bridge _install_) are plain filesystem writes and run inside the **CLI**
headless just as well as inside the server; only the persistent machinery (PTY,
drive runtime, busy, the _live_ permission bridge) requires the always-on server.

## 5.4 Terminals & initialization (init AI tools in terminals)

The **terminal/PTY manager** (in the core, hosted by the server) provides:

- One or more PTYs per project, spawned lazily, keyed by `(project, terminalId)`.
- A bare terminal-create request launches the host user's default shell (`SHELL` on
  Unix or `COMSPEC` on Windows, with `/bin/sh` and `cmd.exe` fallbacks), matching
  HS1's explicit new-terminal behavior.
- **No lost output from fast exits:** the server keeps its own PTY slave open until the
  child has exited and the master has nothing left to read, so a command that writes and
  exits while the drain is starved still lands in scrollback (macOS discards unread output
  shortly after the last slave close; HS2-BCE5XG).
- A **scrollback ring buffer** and multi-client attach (many viewers see one
  stream, tmux-style). Retained scrollback and the live broadcast share one atomic
  handoff: output concurrent with attach appears exactly once in either the snapshot or
  the new subscriber. A lagged subscriber atomically takes a replacement snapshot and a
  fresh live receiver rather than appending an overlapping replay (HS2-5W0V9M).
- **Input never waits behind output (HS2-J52ZJ7).** Every attach (the server's direct and
  broker WebSocket paths, and the broker's own stream to the server) splits into an output
  pump, a writer, and an input reader joined by a bounded 32-frame queue. Under heavy output
  to a slow viewer, the viewer's keystrokes still reach the PTY at once. While the queue is
  full, the pump coalesces live output into one pending frame instead of letting the
  fan-out lag. Only past 256 KiB of coalesced output (the default replay size) does it fall
  back to a scrollback replacement. The replay buffer itself is configurable and sized from the
  client history (`docs/06` §6.7.8, HS2-Y5DYC0). The server writes liveness pongs ahead of queued output.
- **PTY input never blocks an async worker (HS2-FERD7N).** Each terminal owns one ordered
  input queue drained by a dedicated writer thread, shared by every input path so order is
  kept. An attach (server direct, broker WebSocket, and the broker stream) only queues its
  input and keeps reading resize claims and probes even while a child is not reading stdin;
  it waits, parking no thread, only once 1 MiB of earlier input is still unwritten (a larger
  single chunk is accepted into an empty queue). `POST /terminals/{id}/input` and the broker
  `input` request await the write result asynchronously. Dropping a terminal discards its
  unwritten input.
- **A killed terminal never strands its input writer (HS2-RA494E).** The PTY master is
  non-blocking: the writer thread waits for input room in `poll(2)` with a short local
  timeout and, once the child has exited or the terminal is gone, fails the in-progress and
  remaining queued input instead of waiting. This matters on Linux, where a kernel write
  blocked on a full input buffer is not woken when the child dies. The output drain shares
  the master and waits in `poll(2)` too, ending on EOF/EIO as before.
- Survival across server restarts via a **detached PTY broker process** (carried
  from HS1 `src/terminals/broker/`) — terminals aren't killed when the server
  recycles or is explicitly stopped. Detached hosting is the server default; ordinary
  `hotsheet serve --stop` preserves the project's terminals, while
  `hotsheet serve --stop --kill-all-terminals` deliberately clears them. The broker
  answers a protocol-level Ping/Pong health probe and exits
  cleanly (removing its socket) after a five-minute grace with no terminals and no
  connected clients; any activity resets the grace. A server that outlives that idle
  exit reconnects through one shared restart lock, launches a fresh broker when the
  socket is gone, and retries the connection before serving the next terminal request.
  For a push-capable AI terminal, the broker also retains its session-push launch details
  and resolved session id while its PTY lives. Startup reattachment restores the server's
  message adapter; the password stays on the machine-local broker protocol and is absent
  from HTTP terminal metadata (HS2-6WA2D9).
  The server's per-terminal status pollers (busy feed, session-exit release, and the shell
  AI exit watch, every 500 ms) and its attach/launch existence checks use the broker's
  `info` op, which returns the terminal's state without its scrollback. A `read`
  re-encodes the whole 256 KiB scrollback as a JSON number array, about 850 KB per call,
  and polling it kept an idle broker and server at roughly 30-50% CPU each (HS2-7RRAA8).
  A broker that predates `info` answers it as a bad request; the server's handle then
  falls back to `list`, which also carries no scrollback, so a long-lived older broker gets
  the same relief without a restart. MCP presence sweeps check lease pids with a direct
  `kill(pid, 0)` rather than spawning `kill -0`.
- **Compact broker byte payloads (HS2-9R751Y).** Byte payloads (attach scrollback and
  output frames, attach input, `read` scrollback) decode from either a legacy JSON number
  array or a base64 string (`hotsheet_terminals::WireBytes`), so mixed versions always
  interoperate. Encoding is negotiated per connection:
  - The server's `attach` and `read` requests carry `"bytes": "base64"`. A current broker
    then answers in base64. A broker that predates the field ignores it and keeps sending
    arrays, and a request without the field (an older server) gets arrays.
  - Attach input goes to the broker as base64 only after that connection has streamed a
    base64 payload, so an older broker never receives a string it cannot parse.
  - The request/response `input` op stays an array, since keystrokes are small.
  - A 256 KiB replay frame shrinks from about 934 KB to about 350 KB on the wire. Its parse
    through the tagged frame enum drops from about 38 ms to about 4 ms (debug build,
    `wire_bytes_replay_cost`).
    New broker sockets use `/tmp/hotsheet2-brokers-<effective-uid>/<home-project-hash>.sock`:
    the short path stays within Unix socket address limits even when `HOTSHEET_HOME` is
    under macOS's long temporary-directory root. The private directory is owned by the
    effective user with mode 0700; sockets and persistent ownership-lock files use 0600.
    The key includes the canonical configured home and project, so separate homes do not
    share terminal brokers. Indexes, instance registrations, settings, and other state
    remain under the configured home. An owned legacy socket is adopted under a shared
    namespace-selection lock and pinned by its persistent ownership-lock inode, even
    after idle socket cleanup. Retained and fresh server handles therefore reuse the
    same legacy address across exits/restarts; failed health probes never select a
    second namespace. This preserves terminals across upgrades. Symlink/shared directories and
    non-socket stale paths are rejected with a diagnostic; exclusive no-follow locks
    serialize detached ownership, and shutdown removes only the socket inode it bound.
    Connect probes themselves have a 500 ms deadline, separate from the Ping read/write
    deadlines (HS2-YDD60V). A full macOS accept backlog can report connection refusal;
    the held broker lock keeps that case uncertain, so startup does not spawn a replacement.
    A transient `WouldBlock` while probing an old socket is retried briefly; cleanup
    requires a free owner lock plus connection refusal or disappearance and never removes
    a socket whose liveness is uncertain.
    Lock files remain in place to avoid unlink/recreate races; they hold no durable
    application data and the operating system releases their lock on process exit.
- Environment scrubbing (drop tool-marker vars like `TSX_*`/`npm_*`, plus the host's `HOT_SHEET_*` build pins, that leak into
  child shells — HS1 §22.13.1). Every launched process also drops the inherited
  repository-locating git variables (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, …;
  HS2-J79CZF): PTY shells through `scrub_env`, and AI tools, configured commands, the
  terminal broker, external apps, and `hotsheet-cli launch` through
  `hotsheet_ticketing::git::launch`. A server started from a git hook therefore
  never hands the hook's repository to an agent or shell working in a checkout
  ([`02` §2.12.1](02-ticket-storage.md)).
- **Server-arbitrated PTY sizing.** A PTY has exactly one size, but many viewers
  (across devices) attach at once and want different sizes. The **server is the
  sole arbiter** of the size: viewers send _size claims_, the server picks the size
  by a focus-follows policy with leases + hysteresis, and broadcasts the result.
  Full design: [06-clients.md](06-clients.md) §6.7. (This replaces HS1's ad-hoc
  "largest-or-last-writer" consensus, which never worked for remotes.)

**Initializing a tool** in a terminal = the plugin's `setup` composed from host
helpers: write the tool's MCP config (so `hotsheet_*` is available), write its
instruction file + skills, install its permission bridge if opted in, and resolve
its `[launch]` command. `[launch]` is explicitly separate from `[drive]`: the former
starts the interactive REPL, while the latter may start a headless protocol such as
Codex app-server or Claude stream-json. `POST /terminals` with `connect:<tool>` and no
`command` runs setup, safely resolves `[launch].program`, injects the server permission
route, and spawns it in the PTY. Launching then spawns that command in a PTY. `GET /terminals`
reports such a terminal as `kind: "ai"` with `tool: "<tool>"` (from its immutable `<tool>-<id>`
session worker id; an explicit `command` that also connects a tool reports no `tool`), which clients
use to name AI tabs after the provider (HS2-HZK0NK). This is exactly
the ticket's "initializing AI tools in terminals including setting up MCP and
similar connections."

A plugin may also declare **`[session_push]`** (`mechanism`, `port_args`, `password_env`) so the
host can message the interactive session its `[launch]` started (HS2-PEJ962). The launch then
appends the port wiring and exports a per-session random password; the connection advertises the
`message` action and `POST /connections/{id}/message` delivers a prompt (with an optional banner)
or a tool UI command through that mechanism's adapter. OpenCode declares `opencode-http`; a tool
with no section (Antigravity) reports the capability as unsupported. Details and the OpenCode
protocol are in [13](13-drive-transport-interface.md) §13.12.

Use `hotsheet-cli terminal message <terminal-id> --text "Your message"` against the local
server to message a capable session (HS2-NN6EXP). It uses the authenticated
`POST /connections/{id}/message` operation and reports unavailable or unreachable sessions.
The terminal drawer's Message AI Session button and browser-native prompt were removed after
UX review (HS2-JVSFES); CLI messaging, Drive-to-session routing, and the transport remain available.

## 5.5 Drive / trigger (send a command to a target connection)

> **Status: first slices built (HS2-106/107/108/110).** `crates/hotsheet-aitools` — the
> `Drive` trait; the **`AppServerDrive`** (Codex, **persistent** app-server daemon — a
> `turn/start` on a resumed thread, not a fresh process); the spawn-per-run `SpawnDrive`
> (agy, `codex exec` fallback) over an injected `ProcessSpawner`; the `ConnectionRegistry`
> (§5.6); and `host::trigger` which builds a `Drive` from a plugin's manifest `[drive]`
> and registers a `Connection` — all fake-tested (`docs/13` §13.7). **Drivability is the
> integration priority** (see the current-tool matrix, `docs/13` §13.0). **Still to
> build:** the live Codex daemon connection, the Claude channel drive + async `TurnEvent`
> stream (HS2-9), the ACP drive (OpenCode), the permission bridge (§5.7), and the real
> CLI/server trigger injecting `SystemSpawner`.
>
> **`hotsheet-cli trigger` launch safety is baked in (HS2-117).** A bare `trigger <tool>`
> is safe by default: it prepends a `hotsheet` → `hotsheet-cli` shim (+ the CLI's own dir)
> to the launched tool's PATH so a bare `hotsheet` can't hit an HS1 launcher; refuses to
> run in a project that still holds a live HS1 store unless the selected HS2 store has a
> schema-valid durable import receipt whose canonical `sourceProject` matches that checkout;
> fails closed for any absent, malformed, relative-path, or unrelated receipt; refuses
> when the tool isn't set up; and defaults `--mcp-config` to the tool's project config
> so it reaches **only** the Hot Sheet shim (Claude via `--strict-mcp-config`). `setup`
> now writes an
> **absolute** `hotsheet-mcp` path so the config works without the shim. Codex reads its
> MCP servers from `$CODEX_HOME`, so `trigger` **auto-builds a throwaway MCP-free
> `CODEX_HOME`** for it (HS2-YRDQNX) — a copy of `auth.json` plus a `config.toml` whose
> only server is the Hot Sheet shim — so a bare `trigger codex` can't reach the user's
> global MCP servers. For a linked git-backed ticket store, that isolated config grants
> workspace-write access to the canonical store path and its exact Git metadata directories;
> Codex otherwise protects `.git` even inside a writable root. The checkout remains the
> thread's working directory, and other filesystem paths stay sandboxed. The isolated
> Hot Sheet MCP entry approves its own store-scoped tools so a `never` approval-policy
> chat can query and create tickets; no other MCP server is loaded. An explicit
> `--env CODEX_HOME=…` overrides the isolated home. See
> `crates/hotsheet-aitools/src/launch_safety.rs` and `docs/04` §4.4.
>
> **`hotsheet-cli work` is the headless loop (HS2-118)** built on this: it drives one
> safe turn at a time — each turn takes the top Up Next ticket — until the queue drains,
> a `--max` cap, or a thrash stall (`--max-stall` turns with no queue change). It's the
> north-star bootstrap step (HS2 driving its own dev). Cross-turn session resume (one
> persistent instance rather than a fresh process per turn) is **HS2-3C1XK3**.

> **Direction confirmed (maintainer, 2026-08-19, HS2-41):** there is **no single
> transport all tools share** — but there is **one interface with optional
> capabilities that each tool conforms to as applicable** (absence = not supported,
> §5.3). Different tools implement different subsets. **Designing this interface is
> an early priority** — investigate it sooner rather than later, since it's the seam
> every tool and the whole test harness hang off (design ticket: HS2-67).

A **drive transport** is how the app steers a running tool. HS1 has four; HS2
models them as implementations of one `Drive` trait so a fifth is additive:

- **Persistent channel** (Claude): a long-lived MCP channel; a trigger injects a
  `<channel>` event into the running session ("run the worklist"). Permissions and
  busy flow back over the same channel.
- **Spawn-per-run** (Codex `exec`, Antigravity `agy --print`): each trigger spawns
  a one-shot process; done = process exit.
- **App-server / daemon** (Codex app-server): a long-lived backing service driven
  over JSON-RPC.
- **ACP** (OpenCode, Goose, Kiro): the Agent Client Protocol —
  `session/prompt` = trigger, `session/update` = busy, `stopReason` = done,
  `session/request_permission` = the permission overlay.

`Drive::run(target, content)` sends a command/prompt to a **target connection**
(the ticket's "triggering commands to a target connection"). `target` selects
_which_ connection when several exist (e.g. a git-worktree worker's channel vs the
main one). `run` may be sync (spawn) or async (POST to a running session) — both
allowed by the trait.

**Optional drive sub-capabilities** (a tool implements only what it supports —
absence = not supported): `interrupt`, `reset`, `prestart` (a daemon warm-up),
`isBusy`, and a long-lived **backing service** (Codex's app-server; Claude's channel
arguably). The host calls `drive.interrupt?()` etc. as no-ops for tools that don't
have them, so a new tool declares its subset and nothing branches on the tool id.
The v1 tools exercise two shapes: **Claude** (persistent channel) and **Codex**
(spawn / app-server) — enough to prove the interface isn't Claude-shaped. Getting
this trait boundary right early (HS2-67) is what lets a third tool be nearly
declarative.

## 5.6 Connection registry & busy tracking (list connections; track busy-ness)

> **Status: registry + busy wired (HS2-107/34X6BW/4M67VN).** `hotsheet_aitools::ConnectionRegistry`
> — register/unregister/get/list/count over `Connection { id, project, tool, role
(Main|Worker|DriveSpawned), transport, pid, started_at }`, plus **busy as a derived
> sliding-window view**: `note_activity(id, now)` is one heartbeat both hooks and
> byte-stream/spinner inference feed, `is_busy`/`busy_count` read the window, and
> `set_idle` drops it on a `Done`. The clock is injected (deterministic). Live
> `TurnHandle` signals feed it in the server, and hosted terminals supply OSC-133/spinner
> inference.

- **Connection registry.** Each live tool connection registers an entry
  (`project`, `tool`, `pid`/session, `startedAt`, role: main vs worker vs
  drive-spawned). The API exposes `list connections`, and the UI shows "N
  connections active." Carried from HS1's `channel-ports.d/` + `channelRegistry`,
  generalized so it's not Claude-specific (HS1's is per-Claude-channel-server).
- **Busy tracking, two complementary sources** (both carried from HS1):
  1. **Signals from the tool** — lifecycle (process exit) or hooks
     (UserPromptSubmit/PreToolUse/PostToolUse/Stop heartbeats), extending a
     sliding busy timer.
  2. **Byte-stream inference** — the PTY manager watches for the tool's animated
     spinner glyph; recent spinner output ⇒ busy even mid-single-tool-call, N
     seconds of silence ⇒ idle. This backstops a dropped Stop hook.
     The registry exposes `isBusy(connection)`; the UI shows "X working / X idle."

**Ticket-level active work.** Connection busy state answers whether a tool connection is
doing something, while a non-expired ticket claim lease answers which ticket a worker is
actively holding. The ticket list projects that lease immediately after status with a
yellow, one-third-speed adaptation of the MIT-licensed svg-spinners `gooey-balls-2`
indicator. This is deliberately independent of the durable `started`
status, and `claim_count` remains retry history rather than presence. The client schedules
the nearest lease expiry locally so the indicator clears on time without network polling.
Self-claim workers acquire through `claim-next`; general orchestration and delegated
workers acquire and atomically start their assigned slug/ULID through exact `claim`, renew before expiry or
lengthy work, and release on completion, handoff, error, or feedback. A same-worker exact
claim retry is idempotent and does not inflate `claim_count`; another live holder is a
conflict, while an expired lease can be acquired as a new attempt. Every successful claim
advances Not Started to Started in the same durable write; later status values are preserved.
Each successful claim, renewal, and explicit release is also appended to `claim_history`.
For telemetry, a worker interval starts at a `claim` event; renewals replace its current
deadline; a `release` ends it at that event's time, while an absent release ends it at the
last claim/renew `lease_expires_at`. A later claim starts a distinct interval, so expiry,
handoff, and repeated acquisitions remain exactly derivable without a background expiry job.

**Guarded long work (HS2-MFYYJ9).** `renew` succeeds only while the same worker's
claim is still live; an expired lease requires a new exact `claim`, which records a
new attempt rather than filling the expired interval. A worker with a live claim
may run `hotsheet-cli -C <store> claim-run <slug> --worker <id> -- <command>
[args...]` when its CLI supports the optional command. It renews the specific
claim attempt before starting the local command, again from the persisted lease
deadline while the command runs, and once more before reporting success. The
supervisor watches the lease deadline while a renewal waits for the ticket lock;
authority time is sampled inside that transaction after the lock is acquired. A
failed renewal, expired deadline, changed attempt or owner, or failed command
returns an error. The guard stops its owned process tree, including descendants
that inherited standard streams, before a blocking final check or on authority
loss. Unix uses a new process group; Windows starts the requested command only
after a gated helper joins an owned Job Object; the parent retains the start event
until the helper has opened it (HS2-MFYYJ9). It does not make a command's own side
effects transactional. Older CLIs require the worker to observe the live deadline
and renew synchronously before it passes during long gates, then check and renew
again before commit and handoff. Worker availability leases do not extend ticket
claims. Integrators compare claim-history deadlines with gate and commit evidence
and preserve any historical gap.

**Claim ETA (HS2-DQQ0AX).** A worker may attach its estimated completion time to the live
claim: `claim`, `claim-next`, and `renew` accept `--eta` (CLI), `eta` (MCP and the
`/claim-next`, `/tickets/{id}/claim`, and `/tickets/{id}/renew` bodies) as a duration from
now (`45m`, `2h`, `1h30m`, `1d`) or an RFC 3339 timestamp. One shared parser
(`ops::parse_claim_eta`) requires a future time at most 14 days away; anything else fails
explicitly (HTTP 400, CLI error, MCP tool error). The ticket carries the current
`claim_eta_at`, and each claim or renew event records `eta_at`, so estimate accuracy stays
derivable from `claim_history`. A new acquisition replaces any earlier estimate, a plain
renew or same-worker retry keeps it, `--eta` on renew re-estimates, and release, expiry
takeover, or a terminal status clears it with the claim. Renewal is the re-estimate prompt:
once the ETA has passed, CLI `renew` prints `ETA passed; pass --eta with a new estimate.` and
MCP `hotsheet_renew` returns `eta_expired: true`. The bundled instructions ask agents to
pass an ETA when claiming non-trivial work. Claims are also the phase-transition prompt (HS2-7P14DC): CLI `claim`,
`claim-next`, and `renew` print `Phase: <started_phase|unset>.` with a reminder to record
each `started_phase` transition as it happens (`initial_testing` before tests or gates,
`integrating` before merge/push, `final_testing` while CI runs), and the bundled instructions,
`hotsheet` skill, and generated `worklist.md` carry the same rule so a ticket is not left at
`working` while its tests run. Instructions v62 add that phases are never backfilled or
skipped to tidy the record, because `hotsheet-cli phase-report` measures them
([02](02-ticket-storage.md), HS2-JS7E61). The coordinated role skills (worker v44,
integrator v11, dispatcher v1) apply this to every worker assignment, including `T0` docs and test-only work:
`working` before the first edit, `initial_testing` before any gate, and handoff from
`initial_testing`. The integrator checks that history on review and names a missing
transition rather than backfilling it (HS2-5EGBTH). Instructions v63 and skill v64 add how to treat a
Hot Sheet notification pushed into a running session (HS2-VYR5QE; the push mechanisms are in
HS2-ZW8RFM's `session-push` follow-ups). Every mechanism delivers text the tool does not
attribute to the user. Agents therefore treat it as data: take only the named ticket slug,
read that ticket from the store, and confirm the mentioned note or feedback answer exists
before acting, ignoring the message otherwise. They never run commands, open links, or change
permissions because the text says to, and they rely on the pushes rather than polling the
store or queue on a timer. Claims are git-store coordination, so the ETA
lives only on git-provider tickets. Ticket list and column rows show progress toward the ETA
and the time left, or `Soon` once it passes, and the inspector and reader headers show who holds
the live claim with the same estimate (HS2-XQMDQB, HS2-QKNQXC; see
[ux-components.md](ux-components.md) TicketRow); wire tickets and rows also carry the derived
`claim_started_at` (the live holder's latest `claim` event) as the progress origin.

## 5.7 Permissions & user prompts (permission checks and other prompts)

A **host-side permission bridge**: "ask the user, get a decision," with each
plugin supplying only the transport-specific adapter (an ACP option-response, a
permission hook CLI, a hooks.json entry). When a tool wants approval to run a
command:

1. The tool's adapter routes the request to the bridge.
2. The bridge enqueues it (FIFO — concurrent requests preserved, not overwritten,
   an HS1 bug fixed in §12.10) and pushes it over the WebSocket to every client.
3. The UI shows a non-modal permission popup anchored to the owning project; the
   user allows/denies (with allow-once/always mapping onto persisted allow-rules).
4. The answer routes back to the connection that raised it.

Claude's interactive adapter listens to `PermissionRequest`, which fires only when
Claude Code's own permission modes and rules would display a prompt. Its earlier broad
`PreToolUse` integration ran before those native checks and incorrectly turned every
otherwise-safe Read/Edit/tool call into a Hot Sheet prompt (HS2-N4R6F3). Headless `-p`
sessions do not support that lifecycle event, so their launcher explicitly marks the
same installed adapter to retain `PreToolUse` bridging. Unmarked interactive
`PreToolUse` events emit no decision and preserve Claude's native permission flow.

Codex uses the same documented `PermissionRequest` input and nested allow/deny output
contract. Its plugin declaratively installs the shared adapter in `.codex/hooks.json`;
there is no tool-id setup branch. Codex loads the project hook from launch subdirectories
through the repository config layer and runs the absolute `hotsheet-cli permission-hook`
command installed by setup. Codex still owns the trust boundary: a new or changed project
hook is skipped until the user reviews its hash with `/hooks`. Hot Sheet does not pass
`--dangerously-bypass-hook-trust`. Before trust, or whenever route-back is absent or
unreachable, the adapter emits no decision and Codex presents its normal native prompt.
An unanswered request that reaches the generic bridge uses its existing eventual safe-deny
timeout and emits a removal event when it expires; the generated provider hook timeout is
slightly longer so that denial can be returned rather than the hook process being killed
first. That 86,430-second allowance applies only to `PermissionRequest`. The generated
Codex lifecycle handlers use 30 seconds for `SessionStart` and [Codex's supported
3-second maximum](https://learn.chatgpt.com/docs/hooks) for `SessionEnd`. Setup and
refresh migrate older lifecycle handlers that inherited the permission timeout, preserve
unrelated hooks, and issue the changed-hooks notice for a new `/hooks` trust review. Allow
and deny use Codex's native result; retries remain independent.

The client applies a user's decision optimistically: the popup and its clickable actions
disappear in the same render that begins the network request, preventing latency from
looking like a missed click or allowing duplicate answers. The presumed history entry is
kept when delivery succeeds. A communication failure removes that presumed history and
restores the request with an inline retryable error; authoritative resolution still wins
if another client answered while the request was in flight (HS2-66TBWX). A 404 means
the request is already gone, so the client keeps the popup closed and reconciles with
the server. An older in-flight permissions fetch cannot reopen a request after a
resolution event (HS2-7S0G60).

Both enqueue and resolution publish replayable event nudges. Resolution nudges matter when
another client or transport answers: every attached client refetches pending requests and
retains the disappeared request in notification history, including generic tools such as
`ToolSearch` whose action/details string is empty.

By default, `Always Allow` rules match the same project, tool, and exact action (for
example, the same Bash command); a different command asks again. A person can explicitly
replace selected action spans with `{{token}}` or `{{quoted}}` placeholders, or enter a
constrained regex, in the separate approval review. Pattern syntax is versioned (`1`),
anchored to the complete action, length bounded, and validated against the original
request before it is saved. Regex rules require a literal prefix and positive bounded
character classes; broad wildcards, branches, and optional segments are rejected. Exact
rules take precedence over patterns at the same chat/project reach. The action, pattern,
and match scope are shown before confirmation; a matching future request can then be
answered automatically. Remembered identities are keyed HMAC-SHA-256 values, while the
original action and optional pattern live only in memory and in the authenticated,
encrypted machine-local store. One machine OS credential-store master key is read at most once
per process for provider credentials and all hosted stores; a refused read fails closed for that process instead
of reopening the same OS prompt for each request. Only the server that wins the machine
writer lock initializes stores and reaches the credential store. Each rules file uses a
distinct derived key from its vault master, whose value is itself encrypted under
the machine master after migration (HS2-5A6JNS). Existing
v3 files migrate to v4 on first access after their own legacy key is read, preserving
approvals. A missing master or legacy key and invalid ciphertext disable durable
approvals. On macOS, `npm run sign:setup:macos` in `clients/web` installs an
owner-local code-signing identity for development. The local host signs only its
copied ad hoc server and CLI binaries with that identity; it leaves distribution
signatures and build outputs unchanged (HS2-FYXRRM). The first launch under the
new identity may need Keychain authorization, while later rebuilds retain the
same code identity. Without setup, ad hoc rebuilds may request it again.
The server binds HTTP before starting its primary and configured-store
permission-vault Keychain loads on a separate thread (HS2-Q7R2RM). While those
loads are pending, durable legacy answers cannot auto-allow or be edited, and an
explicit Always answer returns 503 without resolving the pending request. Once
the authenticated vault loads, its rules become available without replacing
in-session or checkout answers, and a rule-change event notifies open clients;
a load failure leaves the legacy vault locked. Health and unrelated routes
remain available during an OS credential prompt.
Legacy fingerprint-only rules
are discarded and require fresh approval; no migration guesses an action.
Rules are personal machine state, stored per linked ticket store under
`${HOTSHEET_HOME}/permissions/<store-id>.json`; they are never written to the code or
ticket repository. `GET /permissions` advertises `always_allow_supported` on each
pending request so clients only render that action when durable rule storage is active.
If a visible auto-Allow countdown expires during an Always review, the client sends
`POST /permissions/{id}` with Allow Once and `continue_review: true`. The server answers
the original request immediately and retains a bounded, short-lived snapshot for
`POST /permissions/{id}/remember` with the reviewed description and pattern. That second
request validates and persists the rule before installing it and resolving matching
pending asks. A failed persistence attempt leaves the snapshot available for retry.
If the asking hook disconnects, its pending request is removed and the clients are
notified immediately; an abandoned request does not remain as a false popup.
The same removal applies to a driven Codex app-server approval when its RPC connection
closes: the bridge wakes that waiter with a one-time denial, publishes a removal event,
and leaves unrelated approvals and remembered rules untouched (HS2-V5Y8WW).
An unanswered approval also emits the same removal event when its timeout expires;
a human answer that wins the race does not emit a second removal (HS2-J30B1J).
The server retains an eventual safe-deny guard of 24 hours. This is intentionally much
longer than client-side automation windows: an ignored or otherwise hidden popup does
not advance a client's visible-presentation countdown and must not disappear after the
old five-minute transport timeout.

**Rule reach, origin, and lifetime (HS2-J6009Z).** These are separate concepts.
The bridge matches a rule's project, tool, and exact action or reviewed pattern;
connection-scoped rules additionally match the raising connection. Both driven agents
and terminal permission hooks use that bridge. A project-wide rule can therefore answer
matching requests from either kind of AI connection within its project identity.

The shipped remembered-rule response retains `connection` only for `scope: "chat"`.
Project-wide `session` and `always` rules retain no originating connection, terminal id,
agent, or source kind. A live hook can supply `terminal_id` to associate a pending request
with its terminal, but that provenance is not copied into the remembered rule. The legacy row's
“All chats” text was derived from scope; it was not evidence that the request came
from a driven chat. A connection id alone is not a reliable source-kind label.

Shipped presentation (`HS2-AT46VG`): use “All AI sessions” for project-wide
reach and the generic “This AI session only” for connection-scoped reach when source kind
is unknown. Keep lifetime separate: “Until Hot Sheet restarts” describes the in-memory
project-wide `session` lifetime; “Until this AI session ends” describes connection-only lifetime,
and “Persistent” describes `always`. The Temporary filter includes both temporary lifetimes.
Do not infer origin from
command text or broaden matching while changing labels. The project/store identity shown
beside reach is investigated separately in `HS2-3Z16CB`.

Implementation references: `crates/hotsheet-aitools/src/permission.rs` (`Scope`,
`rule_for_request`, `effective_rule`, `RuleInfo`),
`crates/hotsheet-server/src/routes/permissions.rs` (`AskBody`, `ask_permission`), and
`clients/web/src/components/permission-rules-settings.tsx` (`ruleRows`).

**Project versus ticket-source ownership (HS2-3Z16CB).** The `project` field in a
permission rule is an exact matching namespace, not a guarantee of checkout ownership.
The terminal launcher resolves the deepest registered checkout containing its working
directory and exports that code-project root as `HOTSHEET_PROJECT`; CLI launch does the
same. `HOTSHEET_PERMISSION_PROJECT` carries the checkout's stable authorization identity.
Opening a project retains every configured source and never selects a default merely to
start a session (HS2-TSXSH6). Ticket-provider connection ids remain outside the
permission authorization key.

Durable rules use machine-local encrypted files keyed by checkout identity; they are
not stored in or synchronized with a ticket repository. An unregistered directory uses
an `unregistered:` identity and cannot receive a durable remembered answer.

**Decided target contract (HS2-G8ZSPQ).** The registered code project
owns a project-scoped remembered permission. Its authorization namespace must be a
single canonical checkout identity resolved from the checkout registry, independent of
the selected ticket, provider, source locator, or mutable default source. A project
with Git, GitHub, Jira, GitLab, or several sources uses that same identity in terminal
hooks, CLI launches, MCP, driven turns, matching, listing, and encrypted persistence.
Two registered code projects that share one ticket store retain separate permission
namespaces; all sources of one registered project share its namespace. Checkout identity
and source identity remain separate even when a project has only one source.

`HS2-G1SG1P` implements the checkout identity used by the current launch paths:
`checkout:<checkout-id>` selects its own machine-local encrypted vault. Terminal,
CLI, and client-driven launches carry `HOTSHEET_PERMISSION_PROJECT` to permission
hooks and the native driven bridge; `HOTSHEET_PROJECT` is the code-project root
for project launches (HS2-TSXSH6). A launch without a registered checkout uses an `unregistered:`
identity with no durable-rule location. Existing store-keyed grants remain in their
original namespace. Legacy hook installations that have not received the new launch
environment also retain their old namespace; `HS2-AFSF4X` provides explicit discard and fresh approval.
Project launch is source independent under `HS2-TSXSH6`.
CLI-launched Codex Work capture receives the linked Git stores separately in
`HOTSHEET_USAGE_STORES`; that claim binding does not change the project session identity.
Permission vault health is checked per checkout before applying a durable rule.
If one checkout vault is corrupt or disappears, its in-memory durable answers are
withdrawn and its pending requests cannot offer Always Allow. Other checkout
vaults and their grants remain available. Restoring the original authenticated
file reloads that checkout's answers; a missing known file is never recreated
silently, including after a server restart. The checkout registry records vault
registration before the file is created, so a missing file remains an unavailable
capability until its original authenticated file is restored (`HS2-KJA1V4`,
`HS2-D6Y877`).
Legacy vault damage also stays within its namespace: the server withdraws its
durable answers and disables Always Allow for legacy requests, while healthy
checkout vaults still register, match, and appear in `GET /permissions/rules`.
When no healthy checkout vault is available, the list retains its 503 unavailable
response. Restoring the legacy vault does not replace checkout grants
(`HS2-G1K4SA`).
Registering a checkout must not recreate a missing legacy vault. After legacy
corruption has locked durable answers, removing its file leaves the list
unavailable until the original authenticated file is restored, even if a
registered checkout has also lost its own vault (`HS2-6HEH2K`).

The server must register one machine-local encrypted durable-rule location per checkout
identity before offering Always Allow. It must report unavailable durable storage as an
explicit capability failure; it must not silently save a project grant under the primary
git store, a legacy empty namespace, or another registered project's file. A launch with
no unambiguous registered checkout cannot inherit an existing project-wide grant. It may
ask for one-time or connection-scoped consent while guiding the user through project
registration. Changing a checkout path or identity needs an explicit remap; matching a
path string by coincidence is not authorization.

Existing store/source-scoped approvals remain limited to their original namespace
until discarded; actions then require fresh approval in the project scope
(`HS2-AFSF4X`). A legacy store linked to two code projects never duplicates its
grants into either project. Settings → Permissions → Legacy Answers lists verified
linked Git sources. The person can inspect the count, then explicitly discard all
answers scoped to one source. The count does not expose action text. Discard removes
the source-scoped answers from the encrypted legacy vault for every linked project
while preserving unrelated namespaces and checkout vaults. It withdraws cached
grants before mutation, retries safely, and fails closed if identity, provenance,
or encryption material is unavailable. The headless equivalent is
`hotsheet-cli permission-discard --project <checkout-id> --source <connection-id>`
for JSON count, followed by `--confirm` to delete. Machine-local encrypted files
remain outside ticket stores and provider systems. The user-facing owner in the
remembered-answer list is the code project after this contract ships
(`HS2-7T5GPN`), not its default source.
The CLI inspects every machine-local server instance record, probes each authenticated
host for the selected Git source, and requires exactly one host to report it as hosted.
Before deletion it asks every host to withdraw cached source-scoped grants, then
routes the deletion through the hosting server. An unreadable or unreachable record,
an inconsistent response, or multiple hosts for the source stops deletion. With no
instance records, repeated offline deletion remains safe and idempotent. A server
anchored to another store can host an extra Git source; its anchor is not assumed to
be the selected source (`HS2-54EBK3`).
The Settings deletion route performs the same authenticated peer withdrawal before
touching the vault. A malformed, stale, or unreachable peer, or another host that
also reports hosting the source, stops deletion. Multiple instance records for the
same server count as one host (`HS2-7QJF8C`).

Acceptance cases for HS2-G1SG1P, HS2-AFSF4X, and HS2-TSXSH6:

| Situation                                                                  | Required result                                                                                             |
| -------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| One checkout, Git plus external providers; switch default source or ticket | Same project rule and durable capability across terminal, CLI, MCP, and driven paths.                       |
| Two checkouts share one git store                                          | A grant in one checkout never answers the other's request or appears as its rule.                           |
| External-only registered checkout                                          | Project rules persist and are listed without a git store; project launch loads its configured sources.      |
| Missing or ambiguous checkout registration                                 | No project grant matches or persists; the request remains answerable with a narrower scope.                 |
| Legacy store rule maps to one or multiple projects                         | Source-scoped answers are discarded for every linked project; future actions require fresh approval.        |
| Discard retries, restart, or failed vault access                           | No stale cached authorization or fallback to another file; unrelated namespaces and checkout grants remain. |
| Project move/remap                                                         | Old identity cannot authorize the new identity until the user confirms the remap.                           |

Regression coverage must exercise those transitions through the matcher, encrypted
persistence, real server routes, and the headless launch path; browser tests must cover
capability and consent presentation. `HS2-G1SG1P` owns identity propagation,
`HS2-AFSF4X` owns discard, and `HS2-TSXSH6` owns source-independent project launch.

Implementation references: `crates/hotsheet-server/src/routes/terminals/launch.rs`
(`terminal_project_for_cwd`), `crates/hotsheet-cli/src/dispatch/setup.rs`
(`permission_project`), `crates/hotsheet-server/src/hosting.rs` (`host_store`),
`crates/hotsheet-server/src/lib.rs` (`with_permission_rules`),
`crates/hotsheet-server/src/multistore.rs` (`permission_rules_path_for`), and
`clients/web/src/permission-notifications.ts` (`projectPermissionRules`). Existing
server regression coverage: `permission_rules_are_isolated_and_persisted_per_hosted_project`
in `crates/hotsheet-server/tests/http.rs`.

**Fewer repeat prompts (HS2-JNHAYB).** Besides Allow Once and Always Allow, the popup offers
**Allow This AI Session** (`scope: "chat"`): an in-memory rule for the asking connection only,
dropped when that drive connection closes and never written to disk. Rules apply by
precedence **chat > project > ask**: the newest chat rule for the raising connection wins,
else the newest project rule (`session` or `always`), else a human is asked. Remembering
an answer (chat, session, or always) also resolves every identical request already
waiting — the resolve response lists them in `also_resolved` and each gets its own
`permission_resolved` event. The client groups identical pending requests (same project,
connection, tool, and action) into one popup that names the count; a one-time answer is
sent for each. `GET /permissions/rules` lists every remembered rule with its scope and
`DELETE /permissions/rules` `{project, connection?, tool, action}` revokes one by its
opaque keyed action identifier (removing a durable rule from the encrypted store too);
`PATCH /permissions/rules` accepts the same exact slot plus either
`decision: "allow" | "deny"`, `scope: "session" | "always"`, or a versioned
`pattern` (`"exact"` to restore literal matching). A decision edit keeps
the lifetime; a duration edit keeps the answer and changes only a project-wide rule.
Chat rules cannot change duration. Durable changes are written before
the in-memory answer or lifetime changes. A failed keychain read, authentication check,
or write leaves an Always request pending without installing or cascading an approval. Before
remembering a chat or persistent answer, the popup reviews the tool, exact action, rule owner,
AI-session reach, match type, and lifetime, then requires a separate confirmation. Settings →
Permissions lists only the selected project's remembered answers with decrypted exact actions
and identifies checkout-owned rules by the code project's name and path in the recent preview,
full dialog, and match editor (HS2-7T5GPN). An older source-scoped answer is labeled as a
legacy ticket-source answer with its original source identity; it is never presented as a
checkout-wide approval before fresh consent. Confirmation cards use the same code-project
identity, or explicitly identify a legacy or unknown request owner. AI-session reach and
lifetime remain separate from ownership. Allowed rows convey the decision through their shield
and accessible name without a redundant visible Allowed label in the preview or full dialog
(HS2-0HKES8). The rows retain reviewed patterns, tool, owner, AI-session reach, lifetime, and
decision. Tool and lifetime use
compact badges in a naturally wrapping group in both preview and full dialog; the row
accessible name retains their full text (HS2-R3R7JE). The exact command is the primary
monospace title without a shaded code background, limited to two lines until Expand action
is selected. Collapse action restores the limit. Expansion is independent of selection,
transfers from preview into the full dialog, and resets when that dialog closes or the
project/settings context changes (HS2-Y8FYVY). Users can edit the
pattern, copy an action, or revoke its rule. Edit match opens a dedicated dialog from either
the preview or full list, with exact-action, placeholder, and constrained-regex modes
(HS2-400WZZ). Its draft persists across mode changes and failed validation; Cancel/reopen
restores the saved rule. Save is explicit because it changes reviewed automatic matching.
The server validates the complete pattern against the original action before changing the
rule. Save prevents repeated submissions or dismissal while pending, closes only on success,
and retains the draft/error on failure. A saved match then re-reads the list from a load that
started after the save; when that refresh fails, the status reports the saved change separately
from the failed refresh, the last good list stays visible, and Retry re-reads the list without
resubmitting the change (a 503 still shows the locked state) (HS2-9YVX6Q). Closing returns focus to its opener. Project/settings
changes discard the local draft; late responses cannot replace another editor. The opaque keyed `action` field remains the edit/revoke identity and is not
shown as a label. The exact action is returned only by the authenticated local rules API, not
events or logs. If the credential key or encrypted file is unavailable, the list shows a locked
state and durable in-memory rules are withdrawn before any automatic answer. Revocation writes
the durable file before removing the live answer. After the vault or credential recovers,
reloading the web page and opening Settings reauthenticates and restores durable answers without a server restart;
chat and session answers and pending requests remain in memory (HS2-8DAA0P). Selectable preview and dialog rows feed one toolbar ellipsis menu on each surface
for Allow, Disallow, Delete, and, for project-wide rules, switching between Until Hot Sheet restarts and Persistent; the duration
action names its target scope (HS2-BTPTWK, HS2-X4ES6Z).
The new duration appears immediately and survives
a reload according to its lifetime. Exact-action rows omit the redundant Exact match summary; reviewed placeholder and regex
labels remain visible, and the editor retains every match type (HS2-2TNY72).
The preview disclosure uses singular `answer` for one
remembered rule and plural `answers` for multiple rules (HS2-9QK0V7).
The full list shows 100 answers initially in a scrolling region,
loads 100 more on request, and filters by Permanent or Temporary and Allow or Disallow. Temporary
includes connection-only and until-restart rules, with their exact scope still shown in the row. Search appears above
two equally sized filter controls, which stack on phone widths. The filters and identity search
determine the loaded rows; changing a filter resets the loaded window but keeps selections.
The dialog omits the duplicate result count; per-rule lifetime badges distinguish connection-only
from until-restart answers within Temporary. In the preview and dialog, plain row clicks select one answer, Command/Ctrl-click toggles,
Shift-click extends a range, and Command/Ctrl-A selects all loaded rows while the list has focus.
The selected-answer toolbar ellipsis menu enables Allow, Deny, Remember Until Hot Sheet Restarts, Persistent, and Delete
when at least one answer is selected. Delete requires confirmation. A batch
reports successes and failures separately, keeps failed answers selected for retry, and
refreshes the list. Remembering an answer, editing or revoking one, and closing a
drive connection that held chat rules each publish a replayable `permission_rules_changed`
change event for the project, so another client's open Settings → Permissions list refetches
once instead of showing a revoked rule until its next resolution (HS2-KYK1S7).
Preview and dialog rows show chat, session, and persistent lifetimes alongside the exact
action and project/AI-session reach, including at phone width. The popup reviews the exact action
before a remembered approval; it does not ask for an optional Bash label. The rule-list API
returns both the decrypted exact action for authenticated display and an HMAC fingerprint
for edit/revoke identity. The encrypted file retains the exact action; old fingerprint-only
files are discarded on upgrade and
require fresh approval. No migration guesses an action from its fingerprint.
When loading an encrypted file with repeated exact project/tool/action slots, Hot Sheet
keeps the newest answer and rewrites the file (HS2-Q40BBV).

Remembered answers match the **exact project and tool**, and the exact action unless a
person explicitly reviews and saves a bounded matcher. For Claude and Codex
permission hooks, the action is the command, path, or serialized tool input; Codex
app-server approvals use a command, call ID, path, or reason.
For pending requests, `GET /permissions` also returns up to four deterministic suspected
secret spans as UTF-16 offsets and fixed labels (HS2-5ZRSJW). The classifier recognizes
secret-named assignments and flags, bearer/API-key headers, URL credentials and secret
query values, and `/secrets/` path segments. It skips ambiguous shell syntax, multiline
actions, short or example values, and values a bounded template cannot match. Suggestions
are advisory: the popup starts with exact matching, and each span must be explicitly
selected, reviewed, edited, or removed before the separate remembered-answer confirmation.
The API adds no copy of a suggested value, and classifier diagnostics carry only offsets
and labels. A suggestion never installs or changes a rule by itself.

Persistent rules are
stored inside a version-4 AES-256-GCM envelope with a fresh nonce. One master key is
generated in the host OS credential store, with distinct keys derived for each rules
file; neither is written to exported rule JSON, logs, or events. Version-3 envelopes
migrate on first access after their original OS-stored key is read.
Exact text is decrypted only in server memory. A missing, corrupt, or unavailable key
and a corrupt envelope fail closed. Atomic owner-only writes and a cross-process file
lock guard edits. Authenticated Settings lists the decrypted action and any reviewed
matcher; unauthenticated events and logs do not expose either.
The server's HTTP integration tests and debug server executable explicitly opt into
an in-memory permission-vault backend when the `hotsheet-aitools` test feature is
enabled. The feature alone keeps the OS credential store for other binaries and
subprocesses, including CLI migration tests built with `--all-features`; release
builds always use the OS store (HS2-R8S894).
The `*` matcher on Claude's hook registration selects which tool events reach Hot
Sheet; it does not broaden a remembered answer.

This is also the seam for **other prompts** the ticket mentions — the bridge is a
generic "the tool needs a human decision" channel, not just tool-permissions.

**Permission modes for driven chats (HS2-NHRDMW).** How often a drawer or sidebar chat
reaches the bridge depends on the mode its tool runs under. Each drivable plugin declares
its selectable modes as provider-native values in `[drive] permission_modes` (id, label,
description, optional `warning`) with a `default_permission_mode`, and lists
`permission_mode` in `session_options` when a chat may switch mid-conversation (exposed as
the `change_permission_mode` action). `GET /ai-tools` serves the declarations; clients never
hard-code mode names.

| Tool   | Modes (provider value → meaning)                                                                                                                                                                                                          | Default |
| ------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| Claude | `--permission-mode`: `acceptEdits` (edits run, other tools ask), `auto` (Claude's own classifier asks only for risky actions), `manual` (every tool call asks), `plan` (read-only), `bypassPermissions` (nothing asks; carries a warning) | `auto`  |
| Codex  | `approvalPolicy`: `never`, `on-request`, `on-failure`, `untrusted`; the sandbox stays `workspace-write` for every policy                                                                                                                  | `never` |

- **Project scope:** `ai.defaults` stores a per-provider `permission_mode` beside model and
  effort (the default provider's value mirrors at the top level), in the checkout's
  machine-local settings with the machine-wide fallback. Settings → AI tools shows a
  "Chat permissions" Select per provider; the headless equivalent is
  `hotsheet-cli ai-settings set|set-provider --permission-mode <mode>`.
- **Per chat:** `POST /drive/connections` takes an optional `permission_mode`. When it is
  omitted, the server picks the first of these that applies:
  1. the mode the resumed `session_id` last ran in;
  2. the project's saved mode for that tool;
  3. the machine-wide saved mode;
  4. the plugin default.

  A turn's `permission_mode` switches the chat from that turn on, and the switch is refused
  while a turn runs. The mode is per connection.

- **Across restarts (HS2-Z5QVXW):** the client session catalog records the chat's mode with
  its session. A new chat records it with its first recorded session; later idle switches
  persist immediately. A restarted server that recreates the drive from that session
  (`recoverProjectConnections` sends only `session_id`) resumes the chat in the mode the user
  picked, and `GET /drive/sessions` reports it. A remembered mode the tool no longer declares
  is skipped, so recovery never fails on it. `GET /connections` and every drive event report
  the active mode.
- **Validation:** a mode the tool does not declare is a `400` (a Claude value is not a Codex
  policy); a saved mode the tool no longer declares falls back to the plugin default. At
  launch Codex maps any non-Codex value to `never` rather than passing it through.
- **Opt-in:** a mode with a `warning` (Claude `bypassPermissions`) needs an explicit browser
  confirmation in Settings and in the chat menu, and shows a shield-alert icon in danger tone.
- **Visibility:** the chat's drawer tab carries a shield icon naming the active mode; the
  conversation model menu has a Permissions submenu beside Provider, Model, and Effort.

`auto` stays opt-in until it has been validated in a driven stream-json session (that the
bridge still receives the prompts `auto` raises); making it the Claude default is
`HS2-YAS0FR`.

For a driven headless Claude chat in `auto`, the Hot Sheet hook observes `PreToolUse` but
defers to Claude's own classifier. A resulting native `PermissionRequest` is sent to the
Hot Sheet bridge; remembered rules can answer it, and otherwise the user sees a pending
request. If the bridge cannot answer an auto-mode `PermissionRequest`, the hook returns an
explicit deny so the headless process cannot treat an unanswered ask as approval
(`HS2-QD1D9H`). Other modes retain their existing headless PreToolUse path. Live Claude Code
2.1.307 probes verified an ordinary Read generated no bridge request, while an explicit
native Bash ask generated one request and honored both allow and deny answers. This does
not yet prove which unconfigured actions Claude's auto classifier flags as risky.

**External interactive terminals.** `hotsheet-cli launch <tool>` runs the plugin's
interactive `[launch]` command in the caller's existing terminal after installing its
setup artifacts and injecting the machine-local project server's
`HOTSHEET_SERVER`/`HOTSHEET_SECRET` route-back. Launch takes a code-project path or
registered identity and opens all configured ticket sources; it starts the machine-local
server if necessary. No ticket source or default is required (HS2-TSXSH6). This path is capability-gated:
Claude and Codex both declare native `PermissionRequest` adapters, so
`hotsheet-cli launch claude` and `hotsheet-cli launch codex` are supported. A tool without
that declared hook remains rejected rather than receiving misleading, unused route-back
environment. Codex permissions also remain supported through Hot Sheet's app-server drive
(`trigger`/`work`).

**Hot Sheet's own shell terminals.** Every shell and command terminal that the server
hosts carries the same route-back as a Connect launch: `HOTSHEET_SECRET`,
`HOTSHEET_PROJECT` (the terminal checkout's code-project root), the server's
`HOTSHEET_HOME`, and, once the listener URL is known, `HOTSHEET_SERVER`
(HS2-HE4AVD, HS2-Q9BP1S). Passing the machine home explicitly matters because
terminal environment scrubbing removes ambient `HOTSHEET_*` values; hooks use
that home to find the replacement server after a restart. The installed permission
hook identifies its own agent,
so a Codex request for `Bash` appears as Codex rather than being inferred as Claude.
Running `claude` or `codex` by hand in an app shell therefore raises its permission
prompts in the app, as long as the project's setup has installed the tool's
`PermissionRequest` hook. Without the hook, the variables go unused and the tool's
native prompt stays in charge.

**Connected sessions (HS2-EV1XK3).** Both the Claude and Codex manifests also route
`SessionStart` and `SessionEnd` through the same adapter. In a Hot Sheet terminal
(`HOTSHEET_TERMINAL_ID`), `SessionStart` POSTs `/terminals/{id}/ai-connection` with the agent,
and `SessionEnd` DELETEs it. An authenticated interactive `PermissionRequest` also carries the
terminal id in `/permissions/ask`; the server marks it connected as soon as the request arrives,
including while the answer is pending, so a missed `SessionStart` cannot leave a live bridge
showing as disconnected (HS2-XYSXVT). A headless `PreToolUse` does not imply an interactive
session. The client also reloads terminal state when a permission request arrives, so the
visible prompt and connection icon reconcile together even if the connection event is missed.
Lifecycle hooks print nothing, because a `SessionStart` hook's output would otherwise
become model context.

The hook also sends its session id. The server ignores a `SessionEnd` for an older session
after a new `SessionStart` (for example, Codex `/clear`) has reported in the same terminal.
A session started from the terminal's shell also ends when the shell regains the foreground,
even if its `SessionEnd` hook never ran (HS2-Z1W068). The connection map is in memory, so a
server restart leaves running sessions unreported until their next hook (MCP presence:
HS2-HNXE38).
The id remains internal to the server. Terminal APIs expose the active connection and the last
trusted hook report's time and source (`session_start` or `permission_request`). The last report
remains visible after `SessionEnd`, but is forgotten when the terminal exits or is killed and
when the server restarts. An ended report does not imply the session remains connected.
`/mcp` in Codex tests the MCP server connection, not whether the project lifecycle and
permission hooks ran. Run `hotsheet-cli hook-diagnose` inside the affected terminal to test the
authenticated local permission bridge and read that terminal's active connection and last hook
report. `--json` gives the same facts to scripts. This read-only probe never reports a hook,
creates a connection, or tests MCP. Bridge reachability alone does not prove that project hooks
are installed, enabled, or trusted. Use Codex `/hooks` to inspect their trust and enabled state.
After server restart, a live session must report again before its connection can be shown. A
successful interactive permission prompt reaching Hot Sheet also proves the bridge is live.

**Codex session address (HS2-FKM97G, HS2-MN865E).** Codex's `SessionStart` report also carries
`codex_home` when `CODEX_HOME` is set; an unset home means the user's default. The server keeps
it and the thread id (`session_id`) internal on the terminal's connection. A permission ask from
the same session keeps the reported home. Codex halt detection rejoins the thread under that
home. Observed live on codex 0.162.1:

- `SessionStart` fires lazily, when the first prompt is submitted. A freshly opened Hot
  Sheet-owned TUI accepts its first message through a one-shot PTY bootstrap adapter;
  the reported thread then replaces it with `codex-queue` (HS2-8R7SN0).
- Its `session_id` is the thread id that `codex queue --thread` and `thread/resume` accept.
- `codex queue` stores messages per `CODEX_HOME`, so a TUI started with `--no-daemon` under the
  same home still received one. A different home cannot reach the thread
  (`no rollout found for thread id`).
- Pushing messages into a Codex session uses the shared session-push operation (HS2-PEJ962,
  [13](13-drive-transport-interface.md) §13.12) with a `codex queue` adapter (HS2-FKM97G).

**Claude session inbox (HS2-M15CHN).** Claude's `SessionStart` report also carries the
session's cross-session inbox: `messaging_socket` and `messaging_token` (Claude's
`CLAUDE_CODE_MESSAGING_SOCKET` and `CLAUDE_CODE_MESSAGING_TOKEN`), plus `tool_version` read from
`AI_AGENT` or `CLAUDE_CODE_EXECPATH`. The server keeps them in memory on the terminal's live
connection only. The token is a per-session credential, so terminal APIs and the last-hook-report
history never serve it or the socket path. A permission ask from the same session keeps the
inbox. The shared session-push operation then messages the session through the `claude-inbox`
adapter ([13](13-drive-transport-interface.md) §13.12). That adapter's wire format was pinned
live on Claude Code 2.1.302.

A tool runs a project hook only once it is installed and, for Codex, trusted. A reported
connection therefore shows that the session's hooks are live, and its permission prompts reach
the app. A missing report means they stay in the terminal. Codex trusts each hook entry by
content hash, so a setup refresh that changes `.codex/hooks.json` makes Codex skip the changed
entries until the user reviews them with `/hooks`. Setup compares the final file bytes with
what was present before writing. A changed or newly created Codex hooks file emits a
`codex_hooks_changed` notice containing its project-relative path (`HS2-4AR09Z`). An unchanged
file emits no notice; changes to instruction or MCP files alone do not trigger it. The Codex
plugin declares this review requirement with `hooks.change_notice`, rather than setup
branching on a tool identity. Setup never edits Codex's trust records.

The headless CLI prints the notice and tells the user to run `/hooks` in Codex for that
checkout. `setup --json` and `setup --refresh --json` return `{set_up, removed}`; each setup
report includes `notices` only when present. `POST /setup/{tool}` carries the same notices in
its shared setup reports. The local app's project-open refresh consumes the CLI JSON and shows
**Review updated Codex hooks**, separately from setup-failure warnings. Dismissing it clears
only that project's current notice. A subsequent unchanged refresh does not show it again;
a later changed refresh requires a fresh review notice. If a later setup step fails after
hooks were written, the failure also includes the review instruction. If project opening
fails after a successful refresh, the bridge retains the notice for that checkout until
a successful open delivers it.

**Halted sessions (HS2-HJ4D1H).** Every Hot Sheet terminal also carries
`HOTSHEET_TERMINAL_ID`. The Claude manifest's lifecycle hook additionally listens to
`StopFailure`, `Stop`, and `UserPromptSubmit` through the same `hotsheet-cli permission-hook`
adapter. `StopFailure` fires when a turn ends on an API error. Its `error` is one of
`overloaded` ("Selected model is at capacity"), `rate_limit`, `authentication_failed`,
`billing_error`, `model_not_found`, `server_error`, and so on, and it carries
`last_assistant_message` with the rendered diagnostic. The adapter POSTs both to `/terminals/{id}/halt`; older hook field
names remain accepted. `UserPromptSubmit` means the user prompted again, and a normal `Stop`
means the turn completed, so either sends `DELETE /terminals/{id}/halt`. `SessionStart` and
`SessionEnd` also clear an earlier halt when a session begins or ends. None of these events
prints a hook decision, and outside a Hot Sheet terminal (no `HOTSHEET_TERMINAL_ID`) or
without a reachable server they do nothing. Older Claude versions ignore the unknown event
names. Codex has no equivalent hook (`HS2-5ZQ7J8`): a failed Codex turn fires
`UserPromptSubmit` but no `Stop`, and no hook carries the failure.

The same Claude prompt and turn-end hooks send only `session_id`, `transcript_path`,
and the boundary kind to `/terminals/{id}/claude-turn` (HS2-W2GNTT). The server accepts
reports from the connected session and reads numeric usage from transcript bytes
appended since that prompt. It snapshots a unique live terminal claim at the prompt,
so a claim acquired later cannot retroactively own the turn. Transient transcript
or metrics errors leave the turn available for a repeated turn-end hook, which keeps
the original claim and records its usage once (HS2-F8578J). Transcript and prompt
text are never included in the hook request.
Claude's turn and lifecycle hooks are installed without a `matcher`: Claude 2.1.308
skips `UserPromptSubmit` and `Stop` when they carry `matcher: "*"`. A new session can
report its transcript path before creating the file; the server starts that cursor at
byte zero and waits briefly for the final append after `Stop`.

Codex halts are therefore detected best-effort by rejoining the TUI's thread on the shared
app-server daemon (HS2-MN865E):

- **Rejoin.** When a Codex session reports a UUID thread id, the server connects to
  `<codex_home or the default home>/app-server-control/app-server-control.sock` with
  `UdsWsTransport`. It sends `initialize` and `thread/resume {threadId}` only
  (`hotsheet_aitools::codex_thread_watch`). It never sends `turn/start`, never answers a server
  request, and never starts a daemon.
- **Mapping.** `turn/completed` with `turn.status: "failed"`, or `error` with
  `willRetry: false`, records the same terminal halt as `StopFailure`:
  - `error_type` is `turn.error.codexErrorInfo` (the variant name).
  - The message is `turn.error.message`, unwrapped from a provider JSON error body.
  - `agent` is `codex`.
- **Ignored and cleared.** `willRetry: true` is ignored. The next `turn/started` clears the halt.
- **Lifecycle.** One watch runs per terminal. It is replaced by a newer session and closed on
  `SessionEnd`, terminal kill, or shell exit.
- **Verified live** on codex 0.162.1 (daemon 0.163.0) with an invalid model; the cassette is
  `crates/hotsheet-aitools/tests/fixtures/codex-0.162.1-thread-rejoin-failure.json`.

Limits:

- The v2 protocol is experimental and its schema is version-specific.
- TUIs started with `--no-daemon` or `daemon_auto_start = false` are invisible.
- While a session is reported connected, the watch's subscription keeps the daemon from
  unloading the thread (it otherwise unloads 30 minutes after the last subscriber).
- A terminal whose Codex process dies without `SessionEnd` keeps its watch until the daemon
  closes it.

Driven sessions report completed
provider failure diagnostics (`HS2-AZVE3P`). Identical active halt error type/message reports
retain their original `at` timestamp and emit no new halt event (HS2-E6KAWY); clearing and
halting again starts a new episode. See [client prompt policy](06-clients.md) for pause,
acknowledgment, and deduplication behavior.
Because the detached broker retains terminals across a server restart, the hook resolves
the current registered server URL and secret for `HOTSHEET_PROJECT` on each permission
request. Launch-time route-back variables remain a fallback when no instance record is
available. A retained Codex or Claude session can therefore send a new approval request
to the replacement server without restarting the terminal (HS2-39S586).
The production local host keeps the CLI named by generated Codex hooks in its machine-home
`bin/hotsheet-cli`, separate from the host's temporary build snapshot. Open Codex sessions retain
their hook command across an HS2 host restart, so removing the old snapshot does not strand
their later permission requests. On shutdown the host leaves a small alias from its old snapshot
CLI path to the durable CLI. Setup detects a production snapshot from its environment or binary
path and prefers the machine-home CLI even if an explicit override still names the disposable
snapshot. The snapshot also carries a private `.stable-hook-cli` pointer so a detached setup
writer can recover the durable path from its binary directory (HS2-G8P0W9). On the first upgrade restart,
opening a project also restores an alias for its pre-upgrade hook path before setup replaces
the saved command (HS2-86VAAG).

**Interactive questions (HS2-KP9K85, HS2-SWY32A).** Claude's `PreToolUse` hook for
`AskUserQuestion` POSTs the complete question input and `tool_use_id` to
`/terminals/{id}/question`, then polls for an answer. The notice offers the original choices,
multiple selections, and free text for up to four questions. Submission includes the visible
episode time and tool ID; the server accepts it only for that live question, while the hook
poll also checks the Claude session ID. The hook returns Claude's documented `allow` response
with the original `questions` and the user's `answers`. “Answer in terminal” releases the hook
without an override, restoring Claude's native question prompt and clearing the shared notice.
Server loss or hook timeout also falls back to that prompt; failed polls make a best-effort
clear of the exact tool and session. `PostToolUse` or `PostToolUseFailure` clears the same tool use;
an older completion cannot clear a newer question. Session start/end and terminal removal
clear stale questions.

In the isolated live Claude check (HS2-5CX10W), a server-launched terminal delivered
two choices, a multi-select answer, and free text to Claude. Native fallback and
stale-answer rejection after session replacement also worked. A browser-created
Claude shell initially missed its hook connection because terminal launch omitted
the server's machine home. After supplying that home (HS2-Q9BP1S), a live
browser-created Claude 2.1.296 shell displayed the Hot Sheet notice at desktop and
phone widths. Answering Blue and Circle/Triangle through the browser returned both
answers to Claude, which repeated them in its terminal.

The plugin hook manifest can override `matcher` per event through `event_matchers`; events
without an override keep the shared matcher. Claude installs `AskUserQuestion` for the two
post-tool events, so unrelated tool completions do not launch the hook adapter. Its
`PermissionRequest` and `PreToolUse` hooks retain their wildcard matcher and permission
behavior. Refresh replaces older Hot Sheet hook entries while preserving other hooks.

**The claim/lease primitive** (`coord`) is what keeps distributed work sane, and it
underpins the git-storage concurrency story ([02-ticket-storage.md](02-ticket-storage.md)
§2.7). Two regimes:

- **Single shared server (this section's default):** a self-directed worker atomically
  claims the top Up Next ticket (`claim-next`), while an orchestrated worker claims its
  exact assigned slug/ULID (`POST /tickets/{id}/claim`); either holds a renewable lease,
  and the server's write chokepoint rejects a write to a ticket another actor holds.
  Lazy reclaim + poison quarantine carried over. `claim-next` selection runs over
  the **index** (fast), and the claim is persisted to the **ticket file**
  frontmatter (source of truth) — so a claim survives an index rebuild.
- **Multiple independent machines over a shared git remote (no single server):**
  coordination goes through **git itself** — a per-ticket claim marker
  (`refs/hotsheet/claims/<ulid>`) claimed by an **atomic-push compare-and-swap**,
  with lease expiry + a sweep for cleanup. Fully decentralized, no coordinator.
  **Validated** (custom refs work on GitHub; tags are the fallback) —
  [08-distributed-and-remote.md](08-distributed-and-remote.md) §8.5.

## 5.8 MCP & CLI access for tools

> **Status: MCP shim built (v1, HS2-7/43); project mode added (HS2-TSXSH6).**
> `crates/hotsheet-mcp` → the `hotsheet-mcp` binary: a stdio JSON-RPC 2.0 server
> exposing ticket CRUD plus git-backed Trash recovery through `hotsheet_restore`,
> `hotsheet_claim`, `hotsheet_claim_next`,
> `hotsheet_renew`, and `hotsheet_release`. It runs in
> **three modes over one `Backend` trait**, so the tool surface is identical either
> way — this is what lets a headless agent work **with or without a server**:
>
> - **`--project <code project>` → project startup**, opening the registered checkout
>   and all its configured Git and external sources through the machine-local server.
>   It starts that server when absent. Unqualified ticket reads, edits, and exact
>   claim, renew, and release operations use the checkout identity; callers can
>   specify a connection for ambiguous ticket reads or edits. Project-scoped
>   `hotsheet_claim_next` remains pending HS2-AV80C4.
> - **`--path <store>` → serverless**, straight to disk over `hotsheet_ticketing::ops`
>   (no server, no index — reads are a file scan, symmetric with the CLI; `docs/04`
>   §4.4). The headless default. A running server's watcher still picks up its writes.
> - **`--server <url> --secret <s>` → proxy** a running `hotsheet-server` over HTTP,
>   for index-backed reads + instant broadcast.

> Checkout-scoped MCP calls accept a checkout id, alias, or canonical path. The shim
> encodes a path as one HTTP route segment, and the serverless backend resolves the
> same reference after decoding it. An empty 404 from a checkout route reports a
> possible server/shim route mismatch with safe next steps; the shim does not query
> another ticket source to hide that error (`HS2-VHWWA6`).
>
> The full-ticket + list-row wire DTOs are defined once in `hotsheet_ticketing::wire`
> (the wire SSOT, §4.2) and shared by the server and both shim backends, so the JSON
> an agent sees never drifts between modes. The plugin-config writing half (the `mcp`
> capability that drops the entry into each tool's config) is HS2-98.
> `hotsheet_restore` calls the same `ops::restore` lifecycle as the CLI and web client;
> it accepts an optional checkout target, while explicit non-git provider connections
> return a capability error because their deletion lifecycle is provider-owned (HS2-GTNZ2Q).
> `hotsheet_assign` accepts a provider connection; the serverless backend can target its
> git connection directly. `hotsheet_get` with that connection reads the store and returns
> the server's provider-read body (full details, notes, attachments, and the qualified
> `connection_id:native_id` identity). Any other connection gets an explicit 409, because
> the serverless backend has no provider host (HS2-G78TZ9). `hotsheet_delete_note` takes a stable note id and obeys the
> provider's `note_delete` capability. `hotsheet_report_not_working` requires a connection,
> an explanation and/or `evidence` entries (`filename`, `content_base64`), and optionally
> `expected_token`. Both backends perform the provider's atomic report operation; the
> HTTP backend uses its JSON companion to the browser's multipart route. An unsupported
> provider rejects the report before any note or evidence is written (HS2-CW56C9).
> `hotsheet_rename_attachment`, `hotsheet_delete_attachment`, and
> `hotsheet_set_attachment_metadata` target a registered checkout's git-backed ticket and
> stable attachment ULID. Metadata replacement accepts a selected id array and one metadata
> object. The serverless and HTTP backends use the same checkout routes; external sources
> cannot use these git store operations. Repeating a rename or metadata replacement with unchanged
> values does not advance `updated_at` or create a git commit (HS2-31NJFQ).
>
> **MCP presence (HS2-HNXE38).** When the shim knows its project's server (`--server`, or
> the `HOTSHEET_SERVER`/`HOTSHEET_SECRET` route-back environment a Hot Sheet terminal gives
> a serverless shim, or else the server the machine-local instance registry
> `${HOTSHEET_HOME}/instances` records for a serverless shim's `--path` store, newest
> `started_at` first, HS2-E4J72D), the MCP `initialize` request starts a background lease: it
> `POST /mcp-presence` with a per-process `lease_id`, `pid`, `tool` (`HOTSHEET_AGENT`), `cwd`,
> and `terminal_id` (`HOTSHEET_TERMINAL_ID`), renews every 15 s, and `DELETE
/mcp-presence/{lease}` when stdin closes. Registration is an idempotent upsert, so a
> heartbeat after a server restart rebuilds the lease. The server keeps a lease for 45 s
> (three heartbeats) after its last renewal and drops it sooner when its pid is gone,
> using a server-local timer. `GET /mcp-presence` lists live leases with a project-level
> `connected`; `TerminalInfo.mcp_connected` marks terminals a lease names. Only scope
> transitions (project or terminal becoming connected/disconnected) emit an
> `mcp_presence` change event (`id` = terminal id, empty for the project; `message` =
> `connected`/`disconnected`), so heartbeats add no change-stream traffic. This is separate
> from the hook-reported `ai_connection`, which means permission prompts are routed.
> A serverless shim with no known server reports nothing.
>
> **Which shim the config launches (HS2-HNXE38 reopen).** Presence needs a current
> `hotsheet-mcp`. For a wholly Hot Sheet-owned local config, `setup` records an
> absolute command: an existing absolute
> `HOTSHEET_MCP_BIN` first, else the `hotsheet-mcp` beside the running CLI/server, else the
> bare name resolved through PATH. The development local host runs the server from a
> private snapshot without the shim, so it sets `HOTSHEET_MCP_BIN` to the stable
> `target/{release,debug}/hotsheet-mcp` matching the selected server profile; otherwise
> setup wrote a bare `hotsheet-mcp` that PATH could resolve to a stale install with no
> presence lease, and the badge correctly read "MCP not connected" while Claude reported
> the server healthy. `npm run server:rebuild` builds and installs the shim with the CLI.
> A config shared with user-owned entries instead records the portable bare command and
> `--project .`; the shim resolves the code project from the working directory.
>
> **Following a moved server (HS2-HNXE38 reopen).** A Hot Sheet terminal outlives a server
> restart through its broker, so its `HOTSHEET_SERVER` route-back can name a dead port, and a
> restarted server listens on a new port. After any failed heartbeat a serverless shim
> re-reads the instance registry for its `--path` store and, when it names a different
> endpoint, moves the lease there and registers at once (keeping `terminal_id`). Proxy-mode
> shims (`--server`) never relocate. A project-mode shim follows the bootstrap server's
> instance record, reopens the same checkout after a restart, and routes later tool calls
> to that server without changing the checkout identity (HS2-TSXSH6).
>
> **Client display (HS2-670ADP).** The web client folds MCP presence into the existing
> terminal AI connection badge on drawer tabs and dashboard tiles; there is no separate MCP
> icon and no project-level indicator. The connection is either fully connected or it is
> not: `plug` only when the hook reported in **and** `mcp_connected` is true, `unplug`
> otherwise. A hook-only session gets the same 15 s grace, then shows `unplug` with an
> accessible label that names what is missing ("MCP not connected", how to fix it). A
> current server always serializes `mcp_connected`; one that omits it predates MCP presence and
> keeps the hook-only verdict (version skew). The client
> re-reads terminals on `mcp_presence` events, never by polling.

AI tools reach tickets two ways, both over the one core:

- **MCP** — the `hotsheet_*` tool surface (create/update/get/query/claim/etc.).
  **Decided (maintainer, 2026-08-19): a small per-project MCP shim** spawned into
  each tool's config (as HS1 does with `channel.ts`), _not_ the server exposing MCP
  directly. This keeps the per-project namespacing (`hotsheet-channel-<slug>`) and
  the channel model tools already expect, and lets a tool reach the right project by
  its own config. The plugin's `mcp` capability writes whichever entry the tool's
  config format needs; the shim proxies to the core/server.
- **CLI** — `hotsheet` commands ([04-core-server-cli.md](04-core-server-cli.md) §4.4),
  for tools that shell out.

Both are thin over `hotsheet-core`; there is no duplicated handler tree (HS1's MCP
tools proxy the REST API — HS2's proxy the core directly).

## 5.9 Other HS1 concepts to carry over (evaluated)

Per the ticket's "evaluate other AI-tool interface concepts to carry over":

- **Worklist-as-file** (`worklist.md`) — keep. The file-based contract lets _any_
  tool participate without the API.
- **Auto-context** (HS1 docs/4 §4.18) — **keep; critical (HS2-25).** Per-category and
  per-tag guidance the user configures is **injected into the generated worklist**
  so the AI tool gets the right context automatically for each ticket. It rides the
  worklist-as-file contract (the guidance is composed into `worklist.md` during
  generation — [03](03-indexing-and-query.md) §3.6), so it works for every tool with
  no per-tool code. **Built (HS2-BZBVAS):** HS1-compatible read-time category
  defaults plus global/shared/local overrides are resolved once in
  `hotsheet_ticketing::auto_context`; an explicit empty entry suppresses a default,
  category matching is exact, and tag matching is case-insensitive with matched tags
  sorted by key. The computed structured `{source,key,text}` blocks are returned by
  REST and both MCP modes for get/query/claim and rendered beneath each generated
  worklist row. They are never persisted in a ticket or the index.
- **Skills/instructions generation** for editor tools (Cursor/Copilot/Windsurf) —
  keep; it's the whole Tier-B story and already tool-agnostic in HS1.
- **Assigned worker loop + worktrees** (the `/hotsheet-worker` skill) — keep the
  claim/lease + worktree isolation + "worker never merges, the main agent
  integrates" model. Drop the retired imperative worker-pool orchestration (HS1
  moved to prompt-driven, docs/90 partial retirement); keep it prompt-driven.

- **Telemetry/cost attribution & the Announcer** — defer (they ride Claude's OTLP
  stream; port later as their own tickets, tool-agnostic where possible).
- **Commands Log** transcript of triggers/permissions/shell runs — keep; plugins
  emit, the host owns the log shape.

**Assigned worker handoff (HS2-W3TKX9, HS2-7E5R4A).** In a parallel Codex/Claude session, the
runtime-designated assignment authority (dispatcher in three-role mode, integrator
in legacy mode) assigns an exact ticket after checking dependencies and file overlap. The
worker uses a separate ticket branch and worktree linked to the same ticket store,
claims that ticket, verifies and commits the branch, then writes an `Integration
handoff` ticket note naming its full commit SHA, branch, machine-local worktree
location, checks, and follow-ups. It releases the claim while the ticket remains
Started. The integrator owns merge or cherry-pick, push, CI confirmation, ticket
completion, and worktree removal. Claims coordinate ticket ownership; worktrees
isolate code changes. No worker independently completes a ticket from its branch.
`crates/hotsheet-plugins/src/tests.rs # hs2_w3tkx9_worker_skills_require_integration_handoff`
checks both skill variants and the completion boundary.

**Main-branch integrator (HS2-BNSQF9, HS2-7E5R4A).** The `/hotsheet-integrator` skill keeps one
integrator in `main`. It publishes the safe base, blockers and in-flight scopes for
the dispatcher; in legacy mode only, it also assigns exact tickets. On handoff it verifies the stored note, branch, SHA, and
released claim. It serializes integration, tests the resulting main commit, pushes
under the repository policy, and waits for a covering green CI run before removing
the clean worker worktree and completing the ticket. A failed handoff leaves the
ticket Started with a specific recovery note. Hot Sheet notifications trigger a
store read; there is no fixed-interval queue polling.
`crates/hotsheet-plugins/src/tests.rs # hs2_bnsqf9_integrator_skills_preserve_main_ownership`
pins the mirrored skills and integration boundary.

**Handoff return route (HS2-J81Y9W).** Every assignment records an exact recipient and
one supported route: the native parent channel within the same tool's agent tree,
or a live Hot Sheet terminal connection whose `GET /connections` entry advertises
`message` for the expected integrator session. Separate Codex and Claude sessions
do not share the native parent channel. With a running local server, the worker can
use `hotsheet-cli -C <shared-store> terminal message <integrator-terminal-id> --text "<slug>"`.
It first writes the durable handoff and releases its claim, then sends only the
ticket slug. The integrator reads the store before acting. The terminal route uses
the existing session-push adapters in [13 §13.12](13-drive-transport-interface.md#1312-messaging-a-live-interactive-session-session-push-hs2-pej962);
an unconfirmed receipt does not establish that the integrator acted.

Without a supported recipient, or after failed or unconfirmed delivery, the worker
reports the delivery state and asks for human relay: resume the integrator and give
it the ticket slug to read from the shared store. An independent Codex Work/app
session without an advertised terminal connection uses this fallback; HS2-4H7D8D
tracks investigating a supported wake route. Shared-store notes and the notification
feed alone do not wake independent sessions. Automatic note delivery remains
separate work in HS2-25TP3N; this workflow does not poll the queue on a timer.
`crates/hotsheet-plugins/src/tests.rs # hs2_j81y9w_role_skills_require_addressed_handoff_delivery`
pins the route choices, fallback, and note-before-release-before-cue ordering.

**Per-assignment route (HS2-HM9FRV).** `coordination assign` optionally accepts
`--return-route <native:...|terminal:...|human>` together with
`--route-evidence-note <id>`. The integrator first checks that the selected CLI
supports the flags and writes a regular note on the exact ticket, authored with its
integrator id. With dispatcher authority (HS2-TWZW7X) the dispatcher, as the active
assignment authority, authors that evidence for new operations instead; the default
handoff return route stays the integrator's registered route. The note starts with `## Assignment return route` and contains JSON
identifying the operation, worker, worker generation, and exact route. Assignment
checks those fields and the author against the requested reservation. The note is
durable address evidence; transport reachability still requires checking the exact
native parent or advertised terminal connection. When both flags are omitted, a new
assignment copies the integrator's registered route. A replay without an override
keeps its original route even if registration changed; a conflicting override is
refused without changing the coordination state. Overrides do not change the global
registration or the state schema. Older CLIs continue to use the registered default.

### 5.9.1 Continuous local integrator/worker sessions (HS2-76VMYG)

The repository-local role skills keep an active agent session running between exact
assignments. Either role can start first. A worker invoked without a slug announces
availability and waits; it does not claim an arbitrary queue item or infer that Up
Next is empty. The integrator discovers waiting workers and retained handoffs,
reviews dependency/file overlap, assigns one exact ticket per worker, and serializes
main integration. Workers return to availability after durable handoff and claim
release while earlier tickets may remain in CI. Tentative multi-ticket planning and
a graphical worker board are separate work; this runtime has one active assignment
per worker.

Both skills preflight `hotsheet-cli coordination --help` on the selected executable.
An older installed CLI is a capability blocker, not an empty queue. The integrator
supplies a verified updated executable. In an HS2 source checkout an isolated
`cargo build -p hotsheet-cli` yields `<build-checkout>/target/debug/hotsheet-cli`;
other code projects use their installation method's maintained artifact/upgrade
command. Workers neither assume those projects are Cargo workspaces nor install
globally or restart the running host.

`hotsheet-cli -C <shared-source> coordination --project <shipping-checkout> setup`
idempotently creates versioned local state under the code Git common directory's
`hotsheet-coordination/`. All code worktrees share that directory; it identifies the
code project, not a ticket provider. The pool persists and validates the canonical
selected-source identity. A different `-C` source cannot silently join the same pool.
Its schema uses qualified `(connection_id, native_id)` references, session generations,
operation ids, availability deadlines, return-route identifiers, and typed handoff
evidence (a code commit or findings note id);
it stores no session credentials or ticket content.
The first `Coordinator::open` with a schema-three executable immediately migrates
version-one or version-two state, preserving pending code CI and original findings,
and persists schema three. This also happens for `status` and `setup`. Schema three
is a semantic fence: older schema-two mutators do not recognize returned findings
and could incorrectly resolve an original record. Both older versions therefore
refuse the upgraded state. Stop old waiters and switch all members to the verified
schema-three artifact before any live pool open; checking `coordination --help`
alone does not prove this capability (HS2-A1F615/HS2-AEBJYN).

Use the same prefix, with `--project` before the subcommand:

| Command                                                                                                                                                                                                                                                        | Effect                                                                                                                                                                             |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `register --role <role> --id <id> --session <generation> --route <return-route> --lease-seconds <seconds>`                                                                                                                                                     | Announce or renew availability for this live invocation; route recipients must be verified as described above.                                                                     |
| `next --role worker --id <id> --session <generation> --route <return-route> --lease-seconds <seconds>`                                                                                                                                                         | Announce availability and block until this exact generation has an assignment or the run ends.                                                                                     |
| `next --role integrator --id <id> --session <generation> --route <return-route> --lease-seconds <seconds> --after <cursor>`                                                                                                                                    | Reconcile durable state, then block until a semantic local event changes it. Omit the cursor for an initial snapshot.                                                              |
| `status`                                                                                                                                                                                                                                                       | Read worker availability, qualified reservations, retained handoffs, integration/CI phases, and selected-source queue facts.                                                       |
| `assign --id <integrator> --session <generation> --worker <id> --worker-session <worker-generation> --operation <unique-id> --connection <source> --native-id <native-id> --slug <slug> --base <full-sha> [--return-route <route> --route-evidence-note <id>]` | Reserve one exact assignment after the integrator's scope review. A retried operation id returns the same reservation. An override requires matching addressed evidence.           |
| `worker --id <id> --session <generation> --assignment <id> --phase <phase> [--commit <full-sha> \| --findings-note <note-id>]`                                                                                                                                 | Acknowledge the real claim, durable code or findings handoff/release, or blocker. This is not itself a ticket claim.                                                               |
| `mark --id <integrator> --session <generation> --assignment <id> --phase <phase>`                                                                                                                                                                              | Record code integration/CI or findings verification, then resolution after ticket completion. Multiple code CI records may coexist.                                                |
| `recover --id <integrator> --session <generation> --assignment <id> --evidence-note <note-id> --inspected`                                                                                                                                                     | Fence a stale reservation only after inspecting and preserving its ticket/worktree/commit. The selected local source must have no live claim and must contain the inspection note. |
| `review-return --id <integrator> --session <generation> --assignment <id> --evidence-note <note-id> --inspected`                                                                                                                                               | Preserve a reviewed handoff and fence its integration, permitting a new stable correction assignment (HS2-65ETRJ).                                                                 |
| `finish --id <integrator> --session <generation> --reason <reason>`                                                                                                                                                                                            | End the run only when no work/handoff/CI remains in flight; waiting workers receive the outcome.                                                                                   |
| `leave --role <role> --id <id> --session <generation>`                                                                                                                                                                                                         | Withdraw availability on final exit without erasing assignments or handoffs. `setup --resume` explicitly reopens a stopped run.                                                    |

Each assignment in `coordination status` carries an ordered `transitions` history
(HS2-4273QB). A successful Offered, Accepted, Handoff, Verifying, Integrating, CI,
Done, Blocked, or Review return mutation records its RFC 3339 instant and the known
role/id. Retries of the same transition do not append another event. Older assignments
without this field remain readable and display an empty history; their timing remains
unknown unless independent ticket evidence supplies it.

`next` re-registers the session from its own `--route` and `--lease-seconds` flags; it
does not inherit what an earlier `register` recorded. Omitted flags keep their CLI
defaults (`human` relay and 120 seconds), so a bare `next` silently replaces a verified
terminal or native route and shortens availability. The worker and integrator workflows
therefore pass the same explicit route and lease (10 to 3600 seconds, the longest for
long gates) on `register` and on every `next`, reconnect, and renewal (HS2-CP6GWA). The
public CLI defaults and the state schema are unchanged.

The local default-source CLI checks eligibility before offering an assignment,
requires the assigned worker's live ticket claim before `accepted`, and requires
explicit claim release plus an `Integration handoff` note naming the full commit
before a code `handoff`. Assignment eligibility (including a worker's other claims),
acceptance and every acceptance replay, and stale recovery compare fresh full-precision
ticket timestamps after the relevant ticket reads (HS2-35R7M3). A claim expiring at
or before that instant is available; its fractional remainder is never extended by
the separate integer mailbox availability/wake clock. Expiry alone still does not
satisfy the explicit release required by handoff or blocked reporting.
For findings, the worker uses `--findings-note` with the id
of a nonempty `NoteKind::Regular` `## Findings handoff` note on that exact qualified
ticket and releases its claim first. The integrator must hold the exact live ticket
claim to mark or replay `verifying`; it compares the full timestamp, including
fractional seconds, and rechecks the note kind and evidence.
The optional `preflight` command (HS2-NNSWDS) accepts a machine-local JSON file bound
to the qualified ticket, exact assignment scope, claim-attempt id, ticket update time
and last note id. It verifies the live claim and `initial_testing` phase. Code evidence
also names the worktree, branch, full commit/tree and risk tier; the command checks
clean source, assignment-base ancestry, declared path/section scope and the
exact-tree root gate marker. Findings evidence supplies a proposed nonempty findings
note and requires no commit or code gate. Both kinds attest latest-instruction,
requirements and coverage review and list unresolved findings with existing follow-up
tickets. For rendered web code, the worker attests inspection of the real parent,
affected states and wide/medium/narrow/resize viewports; a ticket with reference
attachments also needs a matching after attachment. These subjective reviews remain
explicitly self-attested. The report classifies checked facts and attestations;
failed checks name a repair. `--publish` repeats preflight, refuses intervening notes
or claim loss under the ticket mutation lock, atomically writes its note and releases
the exact claim, then records mailbox Handoff. Exact-evidence replay is idempotent.
The worker can take independent work immediately after the durable released handoff.
Older runtimes retain the manual note, release and `worker --phase handoff` path. The
integrator still reviews the result and may return it.

The ticket must be Completed or Verified before `done`. Findings never enter
`integrating` or `ci` and require no empty commit,
push, or remote CI. Existing ticket claims remain authoritative. A mailbox phase does
not complete a ticket. Availability expiry never reallocates an in-progress ticket:
the integrator must inspect its artifacts and record a recovery note first. Old
session generations cannot acknowledge replacement assignments.

Reviewed code and findings handoffs can be explicitly returned for in-scope
corrections (HS2-65ETRJ/HS2-AEBJYN). Code returns require Handoff; findings permit
Handoff or Verifying. Before returning Verifying, explicitly release the integrator
claim, even after lease expiry. The exact live integrator adds a nonempty regular
`## Review return` note to the assigned ticket, naming the assignment id and original
evidence (full code commit or exact findings note id) as whitespace-separated tokens,
plus preserved artifacts and corrections. Original findings must still be a nonempty
regular `## Findings handoff` note on the exact qualified ticket.

`review-return` checks source and absence of a live claim under the ticket mutation
lock, retaining original typed evidence in Blocked state with the review note id.
An identical return replays without altering a later assignment or claim; conflicting
evidence is rejected. A NEW stable `assign` operation reserves corrections. The
original remains fenced from verification, integration and Done, even after the
corrected ticket completes. Integrating, CI and ordinary blocked records cannot be
returned. Existing fields are reused, but the schema-three startup fence still
applies before opening the pool; old mutators are not semantically compatible.

`next` subscribes to filesystem events before reading snapshots, so either startup
order and reconnect after a lost cue reconcile durable state. Queue edits, source
configuration changes, and known claim-expiry deadlines wake it; overflow/rescan
events force reconciliation. Availability lease deadlines renew only the local
registry, without queue scans or network requests. Semantic cursors ignore those
heartbeats. A duplicate live wait for the same worker/session is rejected by an OS
file lock, which is released even on process death. An active yielded CLI wait is
kept running; an ended Codex Work/app turn is not resurrected (HS2-4H7D8D).

Queue observation and claim/handoff verification initially support the selected local
Git source. `local_queue: null` means an unsupported provider query path, never empty;
enabled additional sources appear in `uncovered_sources` and prevent `drained`.
HS2-3ETJ26 owns provider-native query, claim-verification, and genuine event adapters.
Its native claim and authenticated event/setup prerequisites are HS2-BPKM76 and
HS2-SGQ183. Additive `source_support` snapshot entries expose registry-only implementation
support (HS2-FG7ZMF): canonical provider capabilities, native query/evidence-read support,
current queue/claim/event verification support, configuration state and explicit reasons.
These entries never resolve an adapter, credential or external ticket, and do not prove
remote availability or authorize a mailbox transition. Disabled, missing, invalid and
reconfigured source metadata stay distinct; an unregistered connection gets an explicit
unknown-source error. Additional Git stores remain outside the selected local observer.
Older snapshots omit `source_support`; reading them defaults this field to an empty report,
without changing the persisted pool schema or role versions. Foreign-source assignment
replay, handoff, recovery, review-return and code/findings completion remain rejected before
state mutation. GitHub/Jira/GitLab advertise `watch:false` until an actual authenticated
host subscription exists; Git retains its implemented watch capability.
This workflow adds no timer network polling. Remote CI completion needs an actual
supported status/event route or human cue; without one the integrator reports the
specific pending CI and unavailable wake route instead of reporting exhaustion.

#### Dispatcher assignment authority (HS2-TWZW7X)

Long review, gates and CI no longer delay refilling workers: an optional dispatcher role
can own assignment while the integrator keeps integration. Exactly one **assignment
authority** is in force per pool, recorded as `authority`:

- `integrator` (legacy and default): the integrator assigns, as above. Every pool at
  schema three or older has this authority. A dispatcher may not register or assign.
- `dispatcher`: one live dispatcher generation owns `assign`; the integrator's `assign`
  is refused. The integrator keeps `mark` (integration, verification, CI, done),
  `review-return`, `recover`, `finish`, push and CI completion. Recovery of an
  interrupted reservation stays with the integrator in both modes because it inspects
  the preserved artifacts; expiry never reassigns automatically.

Workers keep their exact atomic ticket claims, and every role uses the same durable,
source-qualified assignment records, operation-id replay and filesystem-event waits; there
is no second scheduler store. A duplicate dispatcher generation is refused while the live
one holds the lease; a replacement may register only after it expires or leaves, and the
old generation's assignment replays are then fenced. A released handoff frees the worker
for its next assignment while earlier operations stay Handoff, Integrating or CI; each
worker holds at most one Offered/Accepted assignment and keeps every historical record.

The integrator publishes an **integration report** with `publish`: the latest safe shipping
base (full SHA), qualified tickets it blocks from new work, and repository-relative path
scopes held by serial integration or review. The report records the publishing integrator
generation; after an integrator replacement the dispatcher is refused until the new
generation republishes. In dispatcher mode `assign` additionally requires, under the state
lock: the published base; a ticket outside the blocked list; at least one declared `--scope`
path that overlaps neither an in-flight scope nor any pending (not Done, not review-returned)
assignment's scope, comparing whole path components (or reviewed sections, below); and every `blocked_by` dependency
closed in the selected source. The existing eligibility, claim, worker-generation,
stopped-run and unsupported-source checks still apply. Every reservation check runs at one
serialized boundary: under the selected source's ticket transaction and then the
coordination state lock (the order review returns use), after the caller is verified as the
current assignment authority, from fresh ticket reads. A concurrent authority switch,
dependency reopen or claim therefore cannot slip between the checks and the reservation.
Explicit return-route evidence for a new operation must be authored by that active
assignment authority (the dispatcher here, the integrator in legacy mode) and still name the
exact operation, worker, worker generation and route. A reserved operation's route is
immutable across authority changes: replaying it with its full original arguments, including
the original evidence note, succeeds for whichever authority now holds the role, even after
an authority switch or a different dispatcher identity takes over, because the replay only
acknowledges the identical durable reservation. Its evidence must still address that exact
operation, worker, generation and route, and a different route is a different operation.
The reservation reads the clock inside that serialized boundary and again after ticket
validation, before the final authority and worker availability checks. Neither waiting
behind a held ticket mutation nor slow eligibility reads can authorize a reservation after
availability expires. A replay also rechecks its current authority after validation.

Either role may start first. A dispatcher `next` on a pool without dispatcher authority does
not register; it waits on genuine state events and reports why, then registers when the
integrator enables the authority. That registration is atomic with the authority check: if
the integrator returns authority in between, the same waiter stays unregistered and keeps
waiting, while duplicate or stale generations remain conflicts. An unregistered waiter has
no lease to renew, so only genuine events and real peer or claim deadlines wake it; it never
spins on an elapsed deadline. Snapshots add `live_dispatcher` and `dispatch_blockers`,
explicit reasons the dispatcher cannot reserve work now (for example
`assignment_authority_is_integrator`, `no_live_dispatcher`, `no_integrator_registered`,
`no_safe_base_published`, `integration_report_from_replaced_generation`,
`no_available_worker`, `selected_source_queue_unsupported`, `stopped`), never an
empty-queue inference. Older readers default both fields.

**Schema and rollout.** New fields are additive and omitted at their defaults, so a pool with
integrator authority keeps being written as schema three, field for field, and a schema-three
artifact keeps operating it; a schema-four executable never migrates a pool on open.
`coordination capabilities` reports supported schemas, roles and authorities without opening
the pool. Rollout: verify the candidate's capabilities; stop every waiter and switch all
participants to that one artifact (the pool stays schema three); then the exact live
integrator runs `authority --mode dispatcher --all-participants-upgraded`, which persists
schema four. Schema-three artifacts refuse a schema-four pool without rewriting it, so two
assignment authorities or mixed-version mutators cannot coexist. `authority --mode
integrator` returns assignment to the integrator only when no dispatcher session is live; it
stays schema four and never downgrades. Version six or newer is refused. Candidate builds
are exercised only on disposable, independently initialized projects and stores: a linked
worktree shares the live checkout's Git common directory and therefore its pool.

| Command                                                                                                                                                                      | Effect                                                                                                           |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `capabilities`                                                                                                                                                               | Print readable schemas, written schemas, roles and assignment authorities without opening the pool.              |
| `authority --id <integrator> --session <generation> --mode <integrator\|dispatcher> [--all-participants-upgraded]`                                                           | Switch assignment authority as the exact live integrator (see rollout).                                          |
| `publish --id <integrator> --session <generation> --base <full-sha> [--blocked <connection:native>]… [--in-flight <path>]… [--shared <file>]… [--all-participants-upgraded]` | Replace the integration report the dispatcher reads before each reservation (schema four; five with `--shared`). |
| `register`/`next`/`leave --role dispatcher …`                                                                                                                                | Announce, wait as, or withdraw the single dispatcher generation.                                                 |
| `assign … --scope <path>[#row:<id>\|#heading:<slug>]…`                                                                                                                       | As the authority holder; dispatcher reservations must declare non-overlapping scopes.                            |

#### Reviewed shared sections (HS2-YHYNJ6)

Every ticket updates related documentation, so a whole-path scope makes unrelated work
collide on shared documents. In the measured corpus all five released handoffs (HS2-TWZW7X,
HS2-MFYYJ9, HS2-DEVEYE, HS2-VBFSZJ, HS2-G3JVSR) changed `docs/TEST-COVERAGE.md`, so at most
one could be pending at a time. Every one of their documentation edits stayed inside keyed
sections, often several per document (HS2-TWZW7X touched three headings of this one).
Sharing the coverage document by section raises the corpus limit to two;
also sharing the codebase map, the documentation index and `docs/04`–`06` raises it to
three. The rest are genuine conflicts: shared code between HS2-MFYYJ9, HS2-DEVEYE and
HS2-VBFSZJ, and one codebase-map section edited by both HS2-MFYYJ9 and HS2-DEVEYE.

- **Reviewed documents.** `publish --shared <file>` lists documents the integrator has
  reviewed as section-addressable; each must be a file at the published base. A worker
  never chooses sharing. The first shared document persists schema five, and only with
  `--all-participants-upgraded`, so schema-four executables refuse the pool instead of
  dropping the review. Schema four and older pools must carry no shared or section data.
- **Section scopes.** A scope entry is a path, or `path#row:<id>` (a Markdown table row
  whose first cell is that lowercase id) or `path#heading:<slug>` (a heading with its
  non-row lines, slugged to lowercase hyphenated words). Fenced code is content. Any other
  `#` form is refused, which also closes a gap where a free-form fragment escaped the
  whole-file overlap.
- **Overlap.** A directory overlaps everything beneath it and a whole file overlaps every
  section of it. Two entries for the same file are disjoint only when both name different
  sections. Section scopes, whether reserved by the dispatcher or reported in flight by the
  integrator, require that document in the current report. Withdrawing a review keeps held
  sections pending but refuses new section reservations of that document.
- **Truthful handoffs.** A dispatcher-scoped code handoff is checked against the commit
  before any state is written. The assignment base must be an ancestor of the commit, and
  every path changed from base to commit (renames counted on both sides) needs a covering
  whole-path entry or, for a shared document, declared sections. Changes to undeclared
  sections, to text before the first heading, to a duplicated key, or adding or removing a
  sectioned document are refused. So is moving unchanged headings: every heading kept on
  both sides must keep its order, level and parent (the nearest preceding heading of a
  lower level), so a reorder, a level change, or a new heading inserted above existing
  children fails closed even when no block's text changed. New headings that reparent
  nothing, such as a new sibling or a section appended at the end, stay confined to their
  own keys. Whole trailing blank lines (empty, or spaces and tabs only) only separate
  sections; every byte of a section's last nonblank line, including trailing spaces, tabs
  and non-breaking spaces, is content. Findings handoffs carry no commit.
- **Reviewed scope changes.** There is no bypass for a scope that turns out too narrow. The
  worker records `--phase blocked` with a note naming the needed scope; the integrator
  reviews it and marks that operation Done, which releases its scope; the dispatcher then
  reserves a new operation under the same rules.
- **Integration.** Two sections inserted at the same line (HS2-DEVEYE and HS2-VBFSZJ in the
  corpus) still conflict textually when cherry-picked. The serial integrator keeps both
  blocks, and the merged document touches exactly the union of the declared sections.
  Table rows are keyed by their id alone: row order, and which table a row sits in, are
  deliberately not part of any section's content.
- **Fresh publication authority.** `publish` checks the exact live integrator generation
  with a clock read inside the state lock, after the shared documents are validated against
  Git, so an integrator whose lease lapses meanwhile cannot publish a review or persist
  schema five.

The real-process tests prove this contract with separate CLI processes in disposable
repositories; they are not the connected multi-session AI pilot, which HS2-K1QNGC owns, and
no live pool is upgraded here.

Tentative next plans (HS2-XCMAVT) and the worker read model (HS2-DBECD0) consume this
authority when built; the live multi-session rollout remains HS2-K1QNGC.

#### Three-role guidance and setup (HS2-7E5R4A)

`plugins/shared/role-skills/` is the authored source for dispatcher, integrator and
worker instructions. `scripts/sync-role-skills.mjs --write` generates byte-checked
Codex and Claude mirrors, including the plugin-bundled sources. Both first-party
manifests declare the three additional skills beside the worklist skill. The same
core setup path used by headless `setup <tool>`, `setup --refresh`, authenticated
server setup and graphical Project guidance installs or refreshes all three in
their tool skill directories; disabled-tool cleanup removes only managed copies.
A newer or equal-version customized role file is preserved and diagnosed.

The dispatcher reviews the whole queue and reserves only independently ready
qualified tickets at the integrator's published safe base. It rechecks open
dependencies independently of the visible `--unblocked` filter (HS2-BSYW6F),
claims, source capability, worker generation and path or reviewed section scopes.
It refills a worker after that worker's durable released handoff, while the
integrator serializes main review, local gates and push. Both consume the same
durable events. The worker preserves older source artifacts until integration and
returns to `next` without waiting for earlier CI. An unavailable terminal or native
route requires human relay; guidance does not claim an ended independent app
session automatically wakes. The worker's scoped self-review uses the optional
executable preflight on current runtimes (HS2-NNSWDS); bounded, integrator-addressed
advisory reviews use durable request/result/cancellation notes with a separate reviewer
identity and exact source/currentness checks ([contract](31-advisory-reviews.md), HS2-E20S6K), and
HS2-1C1565 CI completion proposals. The full required local gate remains until
HS2-FCCRCK changes that policy.

Rust transition tests and real separate CLI-process tests cover startup in both
orders, duplicate dispatch/waits, fenced generations, stale recovery, source scope,
claim expiry, lost cues, handoff during integrator absence, worktree integration, and
refill while CI remains pending. These synthetic process tests are not a real AI
session pilot. HS2-AJ7FJY separately records real Codex/Claude evidence and remaining
continuous-loop acceptance; no automatic app-session wake is claimed.

#### Suspended integration after an independent gate failure (HS2-2D3099)

An unpushed code handoff that has already been cherry-picked stays linked to its source
commit and exact local main SHA. If an independent required local gate fails, the live
integrator records a regular, operation-addressed `## Integration prerequisite` note on
the assigned ticket. `coordination suspend-integration` verifies the note, current
main SHA, unpushed upstream, and exact prerequisite ticket, then persists
`suspended_integration`. The state is distinct from `ci`: no push or covering
remote run is implied. It releases only the active main-mutation fence, so the
prerequisite may enter integration while the original source evidence remains
durable. It still prevents the pool from being declared drained.

The first suspension requires an explicit all-participants-upgraded assertion and
persists schema six. Older executables refuse the pool instead of dropping the new
evidence. A prerequisite may use the same serial fence. After its commit passes the
exact clean-tree root gate, the integrator records an operation-addressed regular
`## Prerequisite gate` note on the prerequisite ticket. `resume-integration` checks
the new main SHA descends from the suspended SHA, the tree is current, the root-gate
receipt names it, and no other integration or findings verification holds the fence.
It records the gate receipt on the original assignment and returns that assignment
to `integrating`. Another independent gate failure requires a new unpushed main
commit before another suspension. Earlier prerequisites, failure notes, and passed
gate receipts remain in ordered schema-six history. Every schema-six code handoff,
including a prerequisite, checks that the exact main SHA reached its configured
upstream before `ci`; same-phase `ci` replay remains idempotent. `done` follows
covering green CI and ticket completion.
Failed or missing evidence leaves the suspension intact. Provider sources without
local exact-ticket note verification report unsupported instead of inferring
authority from metadata.

HS2-34PWSK has no historical role-bounded main-mutation or local-gate durations, so
it cannot quantify a production before/after critical-section gain for this path.
The HS2-2D3099 real CLI-process fixture demonstrates the transition and serialization
with synthetic tickets, commits, evidence notes, and an exact tree receipt; it does
not represent a live multi-session pilot.

## 5.10 Testability — injected adapters + the fake agent

> **Load-bearing rule** (maintainer, 2026-08-19): adding a tool to HS1 was a manual
> testing slog. HS2 designs that out — the plugin interface is built to be tested
> against a **fake agent**, not a real LLM.

- **Every side-effecting interaction a plugin performs goes through an injected
  adapter** — `ProcessSpawner`, config-file writer, `PermissionTransport`,
  `McpConfigWriter`, `Clock`. **No plugin touches a real process, file, or global
  directly.** (HS1 half-learned this — docs/132 §132.7's "run() with an injected
  spawner reports the content it _would_ send"; here it's non-negotiable.) This is
  what makes drive / permissions / MCP-config / command all deterministically
  testable, and it's a hard rule the conformance suite enforces.
- **Tested against `hs-fake-agent`** — a scriptable test double that speaks the same
  protocols a real tool does (MCP calls, permission requests, PTY bytes/OSC/spinner,
  busy signals), so the host side is exercised end-to-end with no real tool.
- **A conformance suite parameterized over the whole registry is a hard CI gate** —
  a new tool inherits it by existing and can't merge until it passes conformance +
  the fake-agent E2E.
- **Real-tool drift** is caught by a thin, explicit layer: recorded protocol
  contracts (replayed in fast CI) + an opt-in, creds-gated live smoke per tool.

Full testing design: [12-code-organization-and-testing.md](12-code-organization-and-testing.md)
§12.7.7. Build: **HS2-64**.

## 5.11 Plugin loading & extensibility (external plugins)

> **Decided (maintainer, 2026-08-20); manifest-only loading built (HS2-92).** Plugins
> are **external and loadable** — a third party can add a new AI tool without
> recompiling the core — layered so the common case has no ABI at all. `hotsheet-plugins`
> now loads a plugin from a **bundled** first-party dir (`include_dir`) **or a real
> on-disk dir** through one code path (`Plugin::from_fs_dir` / `all_plugins(search_dirs)`);
> the machine search dir is `${HOTSHEET_HOME:-~/.hotsheet2}/plugins/<id>/` (kept **off**
> HS1's `~/.hotsheet`). `hotsheet-cli plugin list|install|remove` manages them, and
> `hotsheet-cli setup <third-party-id>` works with no recompile. A first-party id wins a
> collision (a third party can't shadow a built-in). Still to build: the behavioral
> subprocess/WASM boundary + the trust gate (**HS2-93**).

Rust has no stable ABI, so "loadable plugin" cannot mean "load a `.dylib`." The
**declarative/behavioral split (§5.3) is the escape hatch**: most of a plugin is
_data_, and data has no ABI problem. Plugins are therefore layered:

- **Manifest-only plugins — the bulk.** A directory with a manifest (id,
  `detection`, `preferences`, `tier`, `transport` id, launch command, the
  MCP-config _format_) plus template files (instruction file, skills/rules tree).
  **No code, no ABI, no code sandbox** (there is no code). A common-shape tool
  (§5.3) ships as _just this_. Loaded identically into the CLI and the server, so
  `hotsheet setup <third-party-tool>` works headless.
- **Behavioral plugins — manifest + code**, only for the custom bits (a persistent
  channel, an app-server drive, a bespoke permission bridge). The execution boundary
  is chosen **by capability**:
  - **Subprocess protocol (stdio JSON-RPC)** for the **process-shaped behaviors** —
    `drive`/trigger, terminals, MCP. These are _already_ subprocess-shaped in HS2
    (ACP, Codex app-server, the `hotsheet-mcp` shim), so an external drive plugin is
    just another executable speaking the capability protocol. Language-agnostic; OS
    crash-isolation.
  - **WASM (`wasmtime`/`extism`)** for **pure-compute transforms** that want a
    tighter sandbox — the host exposes only the §5.10 adapters (`ProcessSpawner`,
    config writer, `PermissionTransport`, `McpConfigWriter`, `Clock`) as
    capability-scoped imports; ambient fs/net is denied.

**Built-ins are first-party plugins, in this repo, through the same loader — from
day one** (maintainer, 2026-08-20). Claude and Codex are not special-cased and are
not compiled-in-then-extracted later: the initial set ships as first-party plugin
directories **in the HS2 repo**, bundled into the binaries as the built-in
search-path entry, and loaded by the exact same loader a third party's plugin uses.
This is the §5.10 anti-drift discipline applied to the loader itself — our own tools
ride the external interface, so it can't rot. **Third-party plugins are a
post-release capability:** once HS2 ships, developers add their own plugins
(machine `${HOTSHEET_HOME:-~/.hotsheet2}/plugins/` or project `.hotsheet2/plugins/`) with no fork and no
recompile. So there is exactly one loader and one plugin shape; "first-party" is a
provenance/trust label (§ trust gate), not a separate code path.

**The loader lives in core** (`plugins`, [04](04-core-server-cli.md) §4.1) and reads
a search path: **bundled built-ins → `${HOTSHEET_HOME:-~/.hotsheet2}/plugins/` (machine) → project
`.hotsheet2/plugins/`**. Both binaries load the same registry, which is what lets a
headless CLI set up a project for a plugin the user dropped in.

**Trust gate (mandatory, not optional).** A manifest is inert data, but what it
_writes_ is a supply-chain surface: a plugin's instruction template steers an agent,
and its launch command _executes_. So:

- **Install-time consent** shows exactly what a plugin will write and what it will
  launch, and its **provenance** (first-party / signed / unsigned third-party).
- **`hotsheet plugin verify`** runs the §5.10 conformance suite against a plugin
  (against `hs-fake-agent`) — the acceptance test a third-party plugin must pass,
  since we can't gate someone else's plugin in our CI.
- Subprocess/WASM behavior runs under the least-privilege boundary above; a
  manifest-only plugin can _write_ but never _executes host code_.

CLI surface: `hotsheet plugin list | info <id> | install <path|url> | verify <id> |
remove <id>`, and `hotsheet setup <tool|--detect>` (§5.1a).

> **Trust gate built (HS2-93, partial).** `hotsheet-cli plugin verify` checks a plugin
> structurally — the MCP `format` is known, and every declared **write target stays
> inside the project** (no `..`/absolute escape, enforced in the setup writer too, so a
> plugin can't be tricked into writing outside the project). `plugin info` / `install`
> **disclose what a plugin writes + launches** (plus provenance: first-party vs
> unsigned third-party) and `install` requires confirmation (`--yes` to skip). **Still
> to build:** the behavioral **subprocess/WASM sandbox** (no code-bearing plugins exist
> yet — all current plugins are manifest-only data) and the **`hs-fake-agent`
> conformance** half of `verify` (HS2-64).

### Provider-owned model discovery and defaults

Drivable plugin manifests declare model labels, effort levels, defaults, live-session
selection capabilities, and interactive-launch argument templates. A drive can also
expose the optional `RuntimeModelCatalogSource` capability. Generic hosts query that
capability without branching on a provider id: runtime ids, ordering, effort choices,
and an advertised default are authoritative, while manifest labels remain stable for
known ids. A valid manifest default (then the first live model) is the fallback when the
runtime does not advertise one.

Unsupported or unavailable providers use the complete manifest catalog. Successful and
failed runtime lookups are cached per plugin id and runtime version; a version change
invalidates the old result, while a transient version-probe failure retains the last good
catalog. `GET /ai-tools?refresh=true` explicitly retries same-version discovery, retaining
the last good catalog if that refresh fails. The web client uses that refresh path when
loading AI settings, and `hotsheet-cli ai-tools --json` uses the same capability/merge
core. Neither client maintains provider/model tables. Model catalogs are suggestions rather
than allowlists: settings, Drive overrides, command overrides, live conversations, and headless CLI settings may
name a nonblank model id that discovery did not return. Known models retain their model-specific
effort validation; an unlisted model may use an effort value already declared by that provider.
In the web client's Settings, Drive, and command-editor menus, `Other…` opens a focused exact-id dialog instead of
permanently occupying the main layout with a second model field. The selected custom id appears as
an ephemeral choice and is removed as soon as a catalog model is selected.
AI defaults are **per project** (HS2-SW5S13): `GET`/`PUT /checkouts/{reference}/ai-settings`
(the web client's Project Settings → AI tools panel, routed there by the project bridge) and
`hotsheet-cli ai-settings get|set` store a project's choice under `ai.defaults` in its
machine-local `<project>/.hotsheet2/settings.local.json`, because the installed providers and
models differ per machine. A project without its own choice inherits the machine-wide fallback,
still served by the unscoped `GET`/`PUT /ai-settings` and `hotsheet-cli ai-settings get|set
--global` from the global Hot Sheet 2 settings file, and then the discovered default tool. Every
saved value is validated against the installed providers; one that names an uninstalled tool is
ignored on read.

Each provider also keeps **its own default model and effort** (HS2-EK24KF). The saved value's
optional `providers` map (`{ "<tool>": { "model": …, "effort": … } }`) records one entry per
provider; the top-level `tool`/`model`/`effort` stay the default provider's choice and its entry
mirrors them. A provider without an entry falls back to its manifest default model and effort.
Saving validates every entry for an installed provider (an invalid one is rejected with
`provider '<tool>': …`), keeps the entries of providers that are not installed on this machine so
reinstalling restores them, and drops invalid stored entries on read. Switching the default
provider carries that provider's own entry to the top level and keeps the outgoing default's
choice as its entry, so switching back restores it. Project Settings → AI tools shows the
default-provider Select and then one Model/Effort pair per installed provider; the CLI's
`hotsheet-cli ai-settings set-provider --tool <id> [--model <id>] [--effort <level>] [--global]`
saves one provider's entry without changing the default.

The bundled Codex manifest therefore remains a useful offline fallback, while a reachable
Codex app-server supplies its current paginated `model/list` catalog at runtime.
OpenCode declares its installed-runtime catalog command (`opencode models`). Antigravity
uses the [documented `agy models` output](https://www.antigravity.google/docs/cli/headless/)
only when the first `agy` on `PATH` is a regular native executable at the
[default agent CLI install location](https://www.antigravity.google/docs/cli/install/):
`~/.local/bin/agy` on macOS/Linux or the user-local `agy/bin/agy.exe` on Windows.
The read-only path and file-format check precedes even `agy --version`, so an IDE
launcher, script, or symlink leaves the bundled catalog in use (HS2-V8VPW1). A
custom native CLI install can be trusted explicitly by setting
`HOTSHEET_ANTIGRAVITY_CLI_DIR` to the absolute directory containing `agy` (or
`agy.exe` on Windows). Discovery then probes that file directly, even if an IDE
launcher appears first on `PATH`. The trusted file must still be a regular,
executable native binary; an invalid path or script leaves the bundled catalog
in use. Changing or removing the trusted directory invalidates the live model
cache (HS2-6FVRZS). The exact model-id input remains available for other models.
Setting `HOTSHEET_AI_TOOL_PROBES=off` disables every runtime catalog probe: discovery
still detects installed tools on `PATH`, but every provider keeps its bundled catalog
and no tool is executed. Any other value, or no value, leaves probing on. The Rust test
environment forces it off so tests never run the developer's installed tools
(HS2-7W3D7V).
The provenance review (HS2-3VHPFR, 2026-10-08) found no documented offline check
strong enough to prove an installed `agy` is the agent CLI across supported platforms.
Google's current [Unix installer](https://antigravity.google/cli/install.sh) and
[Windows installer](https://antigravity.google/cli/install.ps1) verify an HTTPS release archive checksum
during installation but publish no signed manifest or durable local receipt, and the CLI
can self-update. A displayed macOS signing Team ID alone identifies a developer, not this
CLI; the inspected local binary did not pass strict signature verification. Discovery
therefore keeps the native default-install guard and offline manifest fallback
unless the user explicitly trusts a custom install directory, without trusting
a mutable release checksum or an unverified signing field.
OpenCode applies a selected model through ACP `session/set_config_option`; Antigravity passes
the selected model and effort through its declared `--model`/`--effort` spawn flags.
Bundled provider manifests also describe offline fallback catalogs. The Claude manifest
tracks the CLI aliases (including `fable`) and advertises its supported effort levels so
settings, Drive, conversations, and interactive terminal launches expose the same choices
even though the channel transport does not provide a live model-catalog endpoint.
Connection creation accepts optional model/effort selections. A live turn may override
them only when the descriptor advertises `change_model` and/or `change_effort`. Interactive
AI terminals use the same plugin declarations to expand model/effort launch arguments. Each
expanded value remains one literal process argument, so spaces, quotes, dollar signs, and shell
metacharacters in a manually entered model id are never reparsed as shell syntax.

## 5.12 Cross-references

- Storage concurrency the claim primitive protects: [02-ticket-storage.md](02-ticket-storage.md) §2.7
- The core that hosts the plugin registry + settings model: [04-core-server-cli.md](04-core-server-cli.md) §4.1, §4.9
- Clients that render permission prompts / busy state / plugin preferences: [06-clients.md](06-clients.md)
- AI-tool integration testing (fake agent, conformance gate, drift layer): [12-code-organization-and-testing.md](12-code-organization-and-testing.md) §12.7.7

### Driven failure diagnostics (HS2-AZVE3P)

Driven adapters preserve provider failure messages through terminal turn events and client
connection state. Codex retryable error notifications keep the turn running; success and
interruption clear the pending diagnostic. See [Drive transport](13-drive-transport-interface.md)
and [Clients](06-clients.md) for protocol and user feedback.
