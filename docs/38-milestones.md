# 38. Milestones and major features

Status: **Partial.** The design carries the signed-off answers (HS2-8T71YW). Shipped: the
milestone registry itself, its Git publication, and its create, list, show, edit, close and
reopen surfaces (HS2-9PYNCN; §38.2, §38.6 rows marked shipped); the project `time_zone` setting, its
validation, CLI and HTTP surfaces, the Project Settings → General Time Zone select and the pure
deadline helpers of §38.3 (HS2-SZF152, `hotsheet_ticketing::project_time_zone`). Everything else is design,
built by the tickets in §38.11. The maintainer answered every design question on HS2-8T71YW (notes "Maintainer answers" and
"Maintainer answers, part 2"); this document records those answers and drops the
alternatives that were not chosen. Implementation is split in §38.11.

From investigation HS2-7147P8. Branch association stays HS2-6E0H3X; the orchestration
consumers are HS2-2CR7H3 (milestone prerequisite) and HS2-QY5YRN (due dates in scheduler
scoring).

## 38.1 Purpose

A milestone answers three questions about a release or a large feature without anyone
keeping a second status:

| Question                       | Answered by                                                            |
| ------------------------------ | ---------------------------------------------------------------------- |
| How much is done?              | Status counts and percent finished of the linked tickets               |
| Where is the risk?             | Blocked, stale, low-confidence and red-gate tickets; the due date      |
| Where is clarity missing?      | Open FEEDBACK NEEDED requests and open tickets with empty details      |
| What should I take next in it? | The ticket list filtered to the milestone, Up Next and unblocked first |

Everything shown is **derived** from the linked tickets when it is read. A milestone
stores only its own identity, title, description, due date and open or closed state.

Non-goals: estimates and weighted progress (story points, hours), burndown history,
Gantt scheduling, nested milestones (§38.2), syncing with a tracker's own milestones,
fixVersions or epics (§38.5), enforcing a branching strategy (HS2-6E0H3X observes
branches), and cross-project roll-ups (Workspace Grid aggregation is HS2-42B2FD).

## 38.2 Model

### Milestone registry

**Shipped (HS2-9PYNCN)**: `hotsheet_ticketing::milestones` (format, lock, titles, duplicate
flag, close and reopen, conflicted state, `.gitignore` line). Delete is built in the registry
for HS2-A820C1, which clears members first; no surface offers it yet.

Milestones belong to the **project**, not to a ticket source, exactly like ticket groups
([35](35-ticket-groups.md) §35.1, [02](02-ticket-storage.md) §2.7.1). The code checkout keeps
one committed file per milestone:

```text
<checkout>/.hotsheet2/milestones/<id>.json
```

```json
{
  "title": "Toolchain 2.0",
  "description": "Ship the new build toolchain to every package.",
  "due_on": "2026-12-04",
  "state": "closed",
  "closed_reason": "completed",
  "closed_at": "2026-12-03T17:20:00Z"
}
```

| Key             | Type                         | Rules                                                                                |
| --------------- | ---------------------------- | ------------------------------------------------------------------------------------ |
| (file name)     | milestone id                 | Ten uppercase Crockford base32 characters, random, the group id shape (`7K3M9P2QXA`) |
| `title`         | string                       | Required. Trimmed, inner whitespace collapsed, 1 to 120 characters, single line      |
| `description`   | Markdown string              | Optional; omitted when empty                                                         |
| `due_on`        | `YYYY-MM-DD`                 | Optional calendar date; its instant is §38.3                                         |
| `state`         | `open` or `closed`           | Required; a new milestone is `open`                                                  |
| `closed_reason` | `completed` or `not_planned` | Present only while `closed`; Close Milestone… defaults to `completed`                |
| `closed_at`     | RFC 3339 UTC                 | Present only while `closed`; set by the close, removed by a reopen                   |

- **Writing.** The file is written atomically, pretty-printed with two-space indentation,
  keys in the order above, one key per line, and a trailing newline, so a diff shows exactly
  the field that changed. Unknown keys are kept verbatim on rewrite (docs/19 §19.2).
- **Lock.** A machine-local `.hotsheet2/milestones.lock` serializes registry writes on one
  machine (create, edit, close, reopen, delete), as `groups.lock` does.
- **Titles** are unique within a project without regard to case or whitespace, like group
  names. Creating or renaming onto another milestone's title is refused
  (`milestone_title_taken`). Two clones that each create the same title leave two ids; both
  are listed and flagged `duplicate` until one is deleted or its members are moved (§38.4).
- **Ignore rule.** The managed `.gitignore` block ([04](04-core-server-cli.md) §4.7.0) gains
  `!/.hotsheet2/milestones/`, so the files are committed while the rest of `.hotsheet2/`
  stays ignored.
- **Nothing is deleted automatically.** A milestone with no members stays listed (it is the
  milestone's own record, unlike an empty group). Only **Delete Milestone…** removes a file
  (§38.6), and it first clears the link from every member.

### Publishing and sync

**Shipped (HS2-9PYNCN)**: `groups::git_sync` takes a `RegistryKind`, so both registries
share one committer, push check, waiting states and lock rules. Each has its own
machine-local status file (`.hotsheet2/milestones-sync.json` for milestones), reported as
`registry_sync` by the milestone list. A group commit and a milestone commit count as
registry-only together, so neither blocks the other's push. The server's sync pass commits and
pushes the milestone registry after the group pass fetched. The group registry's remote
overlay has no milestone counterpart: a pulled milestone file is read when the user's pull
brings it, and the checkout watcher announces it. A milestone publication state change has no
change event of its own yet (HS2-BXKWB4).

Registry writes are committed and pushed by Hot Sheet the same way as the group registry
(docs/35 §35.7), sharing its machinery:

- Every write (app, server, CLI or MCP) is committed in the checkout with
  `git commit --only` naming the changed `.hotsheet2/milestones/*.json` files, under the
  per-repository Git transaction lock. Messages: `Create milestone Toolchain 2.0`,
  `Close milestone 7K3M9P2QXA`, `Set due date of milestone 7K3M9P2QXA to 2026-12-04`. The
  server coalesces a burst into one commit (`Create milestone Alpha (+2 more)`); CLI and MCP
  commit as each write returns.
- The push, the waiting states (mid merge or rebase, detached `HEAD`, no upstream, behind,
  other unpublished commits, `HOTSHEET_NO_AUTOCOMMIT`), lock retries and hook refusals are
  the group registry's rules unchanged. A write never fails because of Git.
- One file per milestone means two clones that create different milestones, or edit
  different milestones, merge without a conflict.

### Concurrent edits of one milestone

Two clones that change the **same** milestone file before syncing can produce an ordinary
Git conflict in the code checkout, because the registry lives there and has no Hot Sheet
merge driver. Hot Sheet never picks a side:

- A file that holds conflict markers, or does not parse, reads as `state: conflicted`. The
  milestone stays listed under its id with a **Conflicted** badge, its members keep their
  links, and its roll-up is still computed. Every write to it is refused with
  `milestone_registry_conflict` naming the file.
- The person resolves the file with Git, as for any other conflict in their checkout. When
  the conflict comes from Hot Sheet's own pull of a registry commit, the sync status names
  the file, as for any sync failure.
- An orchestration prerequisite on a conflicted milestone is `stale`, never satisfied
  (§38.8).

A semantic merge driver for registry files is not part of v1: registry edits are rare and
small. If conflicts prove common, a driver is a follow-up, not a format change.

### Ids on tickets

A ticket names **at most one** milestone by its registry id:

- **Markdown/Git tickets** carry an optional `milestone` frontmatter key holding the id
  (`milestone: 7K3M9P2QXA`), omitted when unset.
- **GitHub, GitLab and Jira tickets** carry a provider-owned label `hs-milestone:<id>`
  (§38.5). This is how Hot Sheet already keeps state a tracker has no field for: GitHub's
  `status:` and `up-next` labels, Jira's `hotsheet-trash` label, and the `hs-group:<id>`
  label decided for groups on HS2-9KZSD4 ([16](16-external-sync-interface.md)). The adapter
  writes and reads the label itself and never surfaces it as a tag.

Rules for the link, whatever the source:

- **One namespace per project.** Every source a checkout links, Git or external, resolves
  ids through that checkout's registry, so one milestone can gather tickets from several
  sources.
- **Unknown ids.** An id the project's registry does not name (a store shared with another
  project, a registry not yet pulled, a deleted milestone) is kept, never dropped. It is
  listed as `Milestone <id>` (`named: false`, `state: unknown`) with its members until a
  person relinks or clears them. Ids are random, so a placeholder is resolved by pulling the
  registry or by relinking, never by creating a milestone with that id.
- **Stores without a project.** A store addressed without its checkout (the default-store
  and `/stores` routes, `hotsheet-cli -C <store>` outside a linked checkout) has no
  registry. It lists ids under placeholders, accepts ids only, and refuses create, edit,
  close and delete with `milestone_registry_required`.
- **No nesting** (maintainer answer). A milestone is not a ticket and has no `milestone`
  key; the registry format has no parent field. Nesting could be added later as an
  optional key without a format break.
- **Not a content edit.** Setting or clearing a ticket's milestone does not change
  `updated_at` (as for `group`, Up Next and priority).
- **Merge of clones.** On Git tickets `milestone` is a last-writer-wins scalar in the
  semantic merge, like `group`: the side with the newer `updated_at` wins and the ticket
  ends in exactly one milestone.
- **Copy and move.** A same-provider copy or move carries the id. A cross-provider copy or
  move carries it only to a destination with the `milestones` capability and otherwise
  drops it and names the drop in the operation result (docs/16 §16.4).
- **Registry edits touch no ticket.** Renaming, re-dating, closing or reopening a milestone
  is one registry write; only Delete Milestone… and Move Open Tickets to… rewrite members.

## 38.3 Due dates and the project time zone

### Due dates

- A milestone's date is its registry `due_on`.
- Every ticket may also carry its own optional `due_on` (`YYYY-MM-DD`): a Git frontmatter key,
  GitLab's issue `due_date`, Jira's `duedate`. GitHub issues have no due date field, so
  GitHub reports `due_dates: false` (§38.5). These are the tracker's own ticket fields, not
  milestone data.
- A ticket's **effective due** is the earlier of its own `due_on` and its milestone's
  `due_on` while that milestone is open. That is what the scheduler scores (§38.8), what the
  `due:` search token reads, and what `overdue` checks.
- Changing a ticket's `due_on` does not change `updated_at`.

### Project time zone setting

| Setting     | Scope                                                                | Value                                        | Default |
| ----------- | -------------------------------------------------------------------- | -------------------------------------------- | ------- |
| `time_zone` | Shared (`.hotsheet2/settings.json`, committed with the code project) | An IANA zone name (`Europe/Berlin`) or `UTC` | `UTC`   |

- **Shared only.** The value must be identical for every clone, so only the shared scope is
  read. `settings set time_zone … --scope local` and a global value are refused
  (`time_zone_shared_only`); a value already present in another scope is ignored.
- **Default.** Unset means `UTC`, so an unconfigured project behaves the same on every
  machine. The default is never taken from the device's zone.
- **Validation.** Writes accept only a zone the bundled time-zone database names (or `UTC`)
  and refuse anything else with `invalid_time_zone`. A hand-edited unknown name reads as
  `UTC`, and the project's settings diagnostics name the bad value.
- **Edited in** Project Settings → General as **Time Zone**: a select of every zone, the
  same control as the notifier quiet-hours zone ([32](32-notifier-plugins.md)), labeled with
  the zone name and its current offset, defaulting to "UTC". Headless:
  `hotsheet-cli settings set time_zone Europe/Berlin` (shared is the default scope) and the
  checkout-scoped settings route `GET`/`PUT /checkouts/{ref}/time-zone-settings` (HS2-SZF152). A
  hand-edited unknown name is named by `settings get time_zone` (stderr), the route's
  `invalid_value` and `diagnostic`, and the select's error text. `milestones --json` and MCP `hotsheet_milestones` `list`
  report the zone in effect.
- **Not retroactive.** Changing the zone changes no stored date; every deadline instant is
  recomputed when next read.

### Deadline instant

A date-only due date `D` means **the end of day `D` in the project time zone**: the
deadline instant is the start of day `D + 1` in that zone, minus one millisecond. It is
computed through the zone database, so a day with a daylight-saving change is 23 or 25 hours
long and still ends at local midnight.

- **Overdue** when `now` is after the deadline instant of the effective due.
- **"Due within N days"** compares deadline instants, not calendar dates.
- **Scheduling** uses the deadline instant as `WorkItem.due_at_ms` (§38.8).
- **Helpers** (HS2-SZF152, `hotsheet_ticketing::project_time_zone`): `deadline_instant_ms`,
  `is_overdue`, `is_due_within` (not overdue and at most N × 24 hours away), and
  `due_day_offset` (calendar days from today in the zone: 0 today, 1 tomorrow, negative overdue).
  A date at the end of the supported range saturates rather than failing.
- **Display** stays a date ("Due Fri, Dec 4"). Relative words ("today", "tomorrow",
  "3 days overdue") are computed in the project time zone, so every viewer of a project
  reads the same word whatever their device zone.

## 38.4 Progress, risk and clarity

One pure function computes the roll-up, `milestone_rollup(milestone, tickets, now,
time_zone, thresholds)` in `hotsheet-ticketing`, so CLI, MCP, HTTP and the orchestrator
agree.

**Members** are the tickets that name the milestone's id, across every source the reading
checkout links. Trash, `moved` tombstones (their destination counts instead) and tickets
closed as `not_planned`, `duplicate` or `obsolete` are **excluded** and counted separately
as `dropped`. A member in a source that cannot be read right now is counted as
`unavailable` and named, never silently dropped.

**Progress** counts members by bucket: `not_started` (not started and backlog), `started`
(with `final_testing` shown apart as `awaiting_ci`), `finished` (completed, verified,
archived as completed). `percent = finished / members`, an integer floor; 0 members reads
"No tickets" and no percent. Counts only: no weights.

**Signals.** Each is a list of `{ ticket, code, detail }`; thresholds are project settings.

| Kind    | Code              | Holds for a member when                                                                           |
| ------- | ----------------- | ------------------------------------------------------------------------------------------------- |
| Risk    | `blocked`         | Open, and an unfinished `blocked_by` ticket or a `blocked_reason`                                 |
| Risk    | `stale`           | Started, no live claim, and no update for `stale_hours` (default 48)                              |
| Risk    | `low_confidence`  | Finished, and `latest_confidence` below `confidence_floor` (default 70)                           |
| Risk    | `red_gate`        | Its latest orchestration run's gates rolled up `failed` (docs/26 §26.13); never parsed from notes |
| Risk    | `overdue`         | Open, and its effective due is past (§38.3)                                                       |
| Clarity | `feedback_needed` | It has an open FEEDBACK NEEDED request                                                            |
| Clarity | `no_details`      | Open, and its body is empty                                                                       |

A milestone is not a ticket, so it has no notes or FEEDBACK NEEDED of its own; questions
about a milestone go on one of its tickets.

**Health** is one word derived from the above, never set by hand:

| Health      | When                                                               |
| ----------- | ------------------------------------------------------------------ |
| `done`      | Closed as completed, or every member finished                      |
| `dropped`   | Closed as not planned                                              |
| `off_track` | Open, due date past, with open members                             |
| `at_risk`   | Any risk signal, or due within 7 days with less than 80 % finished |
| `unclear`   | No risk, but a clarity signal                                      |
| `on_track`  | Otherwise (including no due date)                                  |

Closing a milestone with open members is allowed. The response and the UI list the open
members, and the client offers **Move Open Tickets to…** (one atomic write per store, and
one adapter request per external ticket). Nothing moves on its own.

## 38.5 Providers

Milestones live in the Hot Sheet registry for **every** provider: Markdown/Git, GitHub,
GitLab and Jira (maintainer answer). Hot Sheet does not read, create or sync GitHub or
GitLab milestones, Jira fixVersions or Jira epics, and never maps a registry milestone onto
one. A tracker's own milestone, fixVersion and epic fields are left untouched.

Two `ProviderCapabilities` members, each `#[serde(default)]` false:

| Capability   | Means                                                    |
| ------------ | -------------------------------------------------------- |
| `milestones` | Tickets carry, change and are filtered by a milestone id |
| `due_dates`  | Tickets carry and change their own `due_on`              |

Creating, editing, closing and deleting milestones needs no capability: it is a registry
write in the checkout. A ticket write the source cannot hold fails before any request with
`provider connection '<id>' does not support '<capability>'`, as `groups` does; the client
keeps the control in place, disabled, with that reason.

| Provider     | Milestone link on a ticket               | `milestones`      | Ticket `due_on`  | `due_dates`  |
| ------------ | ---------------------------------------- | ----------------- | ---------------- | ------------ |
| Markdown/Git | `milestone` frontmatter key              | yes               | `due_on` key     | yes          |
| GitHub       | Provider-owned label `hs-milestone:<id>` | yes, when shipped | none             | no           |
| GitLab       | Provider-owned label `hs-milestone:<id>` | yes, when shipped | issue `due_date` | when shipped |
| Jira         | Provider-owned label `hs-milestone:<id>` | yes, when shipped | issue `duedate`  | when shipped |

- **Label rules.** At most one `hs-milestone:` label per ticket; a write replaces any existing
  one in the same request. An issue that carries several (edited in the tracker) reads the
  lowest id, reports `multiple_milestones` on the ticket, and the next Hot Sheet write leaves
  exactly one. The label is 23 characters, inside GitHub's 50-character limit, free of
  spaces for Jira, and uses a single colon so GitLab does not treat it as a scoped label. A
  missing repository or project label is created on first use.
- **Filtering.** `milestone:` queries reach the tracker as a label filter where the adapter
  supports label search, and are answered from the index otherwise.
- **Order of delivery.** Git ships first (HS2-A820C1). The external label mapping and ticket
  due dates are their own follow-up (HS2-6BKPP6), sharing the provider-owned label plumbing
  with groups' `hs-group:<id>` (HS2-PQ4A2C). Until it ships, GitHub, GitLab and Jira report
  `milestones: false` and `due_dates: false`, and their tickets read as having neither.

## 38.6 Surfaces

Every operation has CLI, MCP and HTTP parity (AGENTS.md headless rule). Inside a code
checkout the CLI resolves titles through that checkout's registry; elsewhere it takes ids.

**Shipped (HS2-9PYNCN)**: create, edit, close and reopen, and list and show with each
milestone's registry fields (no roll-up yet), on all three surfaces. A write returns
`{ milestone, changed }`; `changed: false` means nothing was written or committed. Every
refusal names its code (`milestone_title_taken`, `milestone_not_found`,
`milestone_ambiguous`, `invalid_milestone_title`, `invalid_due_on`,
`invalid_milestone_close_reason`, `milestone_registry_conflict`,
`milestone_registry_required`). The server answers the routes and announces
`milestones_changed` after each write that changed a file; the serverless MCP backend answers
the same routes from the registry. Roll-ups, delete, move-open, ticket links, filters and the
time zone in `list` are HS2-A820C1.

| Operation                     | CLI                                                                                        | MCP                                                         | HTTP                                                    |
| ----------------------------- | ------------------------------------------------------------------------------------------ | ----------------------------------------------------------- | ------------------------------------------------------- |
| Create a milestone            | `milestone create <title> [--due 2026-12-15] [--description-file <path>]`                  | `hotsheet_milestones` `action: create`                      | `POST /checkouts/{ref}/milestones`                      |
| Edit title, description, due  | `milestone edit <id or title> [--title …] [--due … / --clear-due] [--description-file …]`  | `hotsheet_milestones` `action: edit`                        | `PATCH /checkouts/{ref}/milestones/{id}`                |
| Close / reopen                | `milestone close <id or title> [--reason completed\|not_planned]` / `milestone reopen …`   | `hotsheet_milestones` `action: close` / `reopen`            | `POST .../milestones/{id}/close` / `.../reopen`         |
| Delete                        | `milestone delete <id or title>` (clears every member first)                               | `hotsheet_milestones` `action: delete`                      | `DELETE .../milestones/{id}`                            |
| List milestones with roll-ups | `milestones [--all] [--json]`                                                              | `hotsheet_milestones` `action: list`                        | `GET /checkouts/{ref}/milestones[?all=true]`            |
| One milestone's roll-up       | `milestone show <id or title> [--json]`                                                    | `hotsheet_milestones` `action: show`                        | `GET /checkouts/{ref}/milestones/{id}`                  |
| Move open members             | `milestone move-open <from> <to>`                                                          | `hotsheet_milestones` `action: move_open`                   | `POST .../milestones/{id}/move-open`                    |
| Link / unlink a ticket        | `edit <slug> --milestone <id or title>` / `--clear-milestone`                              | `hotsheet_update` `milestone` (id, title or `null`)         | `PATCH .../tickets/{id}` `milestone`                    |
| Create into a milestone       | `new --milestone <id or title>`                                                            | `hotsheet_create` `milestone`                               | `POST .../tickets` `milestone`                          |
| Set / clear a ticket due date | `edit <slug> --due <date>` / `--clear-due`                                                 | `hotsheet_update` `due_on`                                  | `PATCH .../tickets/{id}` `due_on`                       |
| Filter tickets                | `ls --milestone <id or title>`, `--has-milestone`, `--no-milestone`, `--due-before <date>` | `hotsheet_query` `milestone`, `has_milestone`, `due_before` | `GET .../tickets?milestone=&has_milestone=&due_before=` |
| Project time zone             | `settings set time_zone <zone>` / `settings get time_zone`                                 | reported by `hotsheet_milestones` `list`                    | `GET`/`PUT /checkouts/{ref}/time-zone-settings`         |

- Unlike a group name, `edit --milestone <title>` never creates a milestone: an unknown title
  is refused (`milestone_not_found`), because a milestone carries a date and a state a person
  sets deliberately. A duplicated title needs the id.
- `--all` adds closed milestones. All writes are idempotent: closing a closed milestone, or
  setting the milestone a ticket already has, changes nothing and commits nothing.
- Rows of every ticket list carry `milestone` and `due_on` when set.

**Search** (saved views and the search field): `milestone:"Toolchain 2.0"` or
`milestone:7K3M9P2QXA`, `has:milestone`, `due:<2026-12-01`, `due:<=7d`, `is:overdue`.

**Index.** Two derived columns, `milestone_id` and `effective_due_on`; the index rebuilds on
the schema bump (docs/03 §3.2). `Index::milestone_members` feeds the roll-up. The registry
itself is read from the checkout, not indexed.

**Live updates.** A member's ticket change event names its old and new milestone id. A
registry write emits one `milestones_changed` checkout event, and the checkout watcher emits
it too when a pull changes `.hotsheet2/milestones/`. A client holding a roll-up refetches it
once. Nothing polls.

## 38.7 Client

Placement as in the Design v1 captures on HS2-8T71YW (maintainer answer), in the existing
project shell.

- **Milestones view** (HS2-NSVKGE). The project sidebar gains a built-in **Milestones** view
  after the ticket views, with a Lucide `flag` icon. The workspace lists open milestones in
  due-date order (undated last), each a framed row: title, due date, a `ProgressBar` with
  "12 of 20 finished", a health `Badge`, and risk and clarity counts. A conflicted or
  unknown milestone shows its badge in the same row. Closed milestones collapse into a
  **Closed Milestones** disclosure at the end. **New Milestone…** opens a dialog with Title,
  Due Date and Description.
- **Milestone inspector** (HS2-NSVKGE). Selecting a milestone shows, in the right rail, a
  `ValueTable` (Due, Health, Progress, Started including awaiting CI, Not started, Dropped),
  then three `ListHeader` + `ListActionRow` sections: **Blocked**, **At Risk**, **Needs
  Answers**. Each row opens its ticket. Header actions: **Show Tickets** (the ticket list
  filtered `milestone:`), **Edit…**, **Close Milestone…** (or **Reopen Milestone** when
  closed), and **Delete Milestone…** in the overflow.
- **Ticket Info tab** (HS2-Y9V4NB). A **Milestone** section above Blocked By, built like it:
  a `ListHeader` with **Set Milestone…** (or Change Milestone…) and one `ListActionRow`
  naming the milestone, its due date and progress, with a trailing clear button; a **Due**
  row in the Info `ValueTable` that names where the effective due comes from ("From the
  milestone"). A source without the capability keeps both, disabled, with the reason.
- **Composer and bulk** (HS2-Y9V4NB). The new-ticket composer and the bulk-edit dialog accept
  a milestone; a list or column selection gets **Set Milestone…** in its context menu.
- **Project Settings → General** gains the **Time Zone** select (§38.3, HS2-SZF152).
- **Rows.** No new row element in v1: the shared summary-row contract stays as it is;
  `milestone:` search and the filtered list cover finding members.
- **Phone.** The Milestones view is a list; the inspector opens as the overlay rail.

## 38.8 Orchestration

### Milestone prerequisite (HS2-2CR7H3)

A new prerequisite kind beside `ticket` and `validation` (docs/26 §26.15). The rule is
**chosen per step** (maintainer answer):

| Kind        | Fields                                       | Rule               | Holds when                                                                                       |
| ----------- | -------------------------------------------- | ------------------ | ------------------------------------------------------------------------------------------------ |
| `milestone` | `target_id`, `milestone_id`, `rule`, `gates` | `members_finished` | The milestone has at least one member, and every member is confirmed finished with `gates` green |
| `milestone` | `target_id`, `milestone_id`, `rule`          | `closed`           | The milestone's registry state is `closed` with `closed_reason: completed`                       |

- `milestone_id` is the registry id in the target project's checkout. The registry is read
  through that target like any other target read, and the reading is stored as an
  observation.
- **`members_finished`** (the default). Defining the step reads the milestone's members
  through the gate as `tickets_read` and stores each as an observation; readiness refresh
  re-reads membership. A member confirmed not finished, or with a red gate, is `blocked`
  (`milestone_member_open`, `milestone_member_red_gate`, each naming the member); an
  unreadable member is `stale`; a member resting on an unconfirmed integrated commit is
  `provisional`; an empty milestone is `blocked` (`milestone_empty`). The milestone's own
  open or closed state does not matter under this rule, so closing a milestone may itself be
  the step the prerequisite gates. A member that another step works is an edge of the plan
  graph, so a cycle through a milestone is found. At most 256 members
  (`milestone_too_large`), keeping reads bounded. A member added after confirmation makes
  the prerequisite not hold again at the next refresh; readiness gates only the start, so a
  running step is untouched.
- **`closed`.** Holds as soon as the registry says closed as completed, whatever its members
  are. Open is `blocked` (`milestone_open`); closed as not planned is `blocked`
  (`milestone_not_planned`); a reopen after confirmation makes it not hold again at the next
  refresh.
- **Both rules.** A milestone the target's registry does not name, or a conflicted registry
  file, is `stale` (`milestone_unavailable`, `milestone_registry_conflict`), never satisfied.
- CLI `orchestration readiness define ... --requires-milestone <target>:<id or title>
[--milestone-rule members-finished|closed]`, MCP `readiness_define`
  `prerequisites[].kind: milestone` with `rule`, and the same route. The inspector's Plan and
  Prerequisites sections word each rule: "Milestone Toolchain 2.0: 6 of 20 tickets not
  finished: API-15, WEB-35, DOC-9 and 3 more. Due Fri, Dec 4", or "Waits for Milestone
  Toolchain 2.0 to close".

### Due dates in scheduling (HS2-QY5YRN)

The scheduler already scores `WorkItem.due_at_ms` (`due`: overdue 2000, within a day 1000,
within a week 300; docs/26 §26.17). The target ticket read (`TicketSeen`, `ReadyItem`) gains
`due_on` and `milestone`; readiness reads the milestone's registry `due_on` and the target
project's `time_zone` with it, and `due_at_ms` is the deadline instant (§38.3) of the
effective due. No new score term. Because the zone is a shared project setting, every
machine scoring the same target computes the same instant.

## 38.9 Format and compatibility

- `milestone` and `due_on` are optional ticket frontmatter keys, omitted when unset. Older
  readers keep unknown keys verbatim (docs/19 §19.2, as `orchestration_links`), so no version
  marker is needed and files written before this read with neither.
- The registry is a new directory; an older build ignores it. Its managed `.gitignore` line
  is added by the next `link`, `init` or local settings write, as for `groups/`.
- `time_zone` is a new shared settings key; an older build ignores it.
- The `hs-milestone:` label is new; an adapter older than the mapping shows it as an ordinary
  tag. Acceptable before the first public release (docs/19 §19.1).
- The index schema bump rebuilds the derived index; no ticket file is rewritten.
- No migration: there is no earlier milestone data. A project that used a `milestone` tag or
  group keeps it; converting is a person's act (relink with `edit --milestone` or bulk edit).

## 38.10 Test plan (implementation is T2)

| Layer                | Covers                                                                                                                                                                                                                                                                                                                           |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Registry unit        | Round trip and stable key order; unknown keys kept; title rules and duplicate titles; close and reopen add and remove `closed_reason` and `closed_at`; conflict markers and unparseable JSON read as `conflicted` and refuse writes; the lock serializes concurrent creates; idempotent repeats commit nothing                   |
| Registry Git         | A commit holds only registry files; coalesced server commits; push and every waiting state of docs/35 §35.7; two clones creating different milestones merge clean; two clones editing one file conflict and read as conflicted until resolved                                                                                    |
| Model unit           | Ticket `milestone` and `due_on` parse and serialize; omitted when unset; last-writer-wins merge; copy and move carry or drop the id by capability                                                                                                                                                                                |
| Time zone unit       | Default UTC; shared-only scope; invalid names refused on write and read as UTC; deadline instant on ordinary days and both daylight-saving transitions; overdue exactly at, before and after the instant; relative words across a zone change                                                                                    |
| Roll-up unit         | State matrix: each member status and close reason in and out; reopen; move (tombstone excluded, destination counted); trash and restore; empty then refill; cross-source member; unknown id; unreadable source; each signal at, below and above its threshold; health precedence including `dropped`                             |
| Index                | Members by id across stores; rebuild after the schema bump; effective due with and without a milestone date, and with a closed milestone                                                                                                                                                                                         |
| CLI / MCP / HTTP     | Parity per §38.6 on a real server with isolated homes; title and id resolution; unknown title refused; no-registry refusals; capability refusal before any request; delete clears members first                                                                                                                                  |
| Providers            | Fakes matching real GitHub, GitLab and Jira label shapes: one label written, a second label replaced, several labels read as the lowest with `multiple_milestones`; live tests in HS2-6BKPP6 against the safe test repositories                                                                                                  |
| Readiness            | Both rules: member added after confirm, member reopened, red gate, unreadable member, empty, closed, closed as not planned, reopened, unknown id, conflicted file, cycle through a member, provisional member, more than 256 members; order-independence                                                                         |
| Scheduler            | Effective due from ticket only, milestone only, both (earlier wins), closed milestone ignored; the same instant from two machines with different device zones                                                                                                                                                                    |
| Browser (Playwright) | Milestones view and inspector; New Milestone…, Edit…, Close and Reopen; Set Milestone… and clear, both ways (control to state, state to control); Time Zone select; roll-up updates on a change event without polling; disabled controls on a source without the capability; 1440, 1100 and 390 px in both color schemes; WebKit |
| Visual QA            | docs/manual-test-plan.md rows for the view, inspector, Info section and Time Zone setting                                                                                                                                                                                                                                        |

## 38.11 Implementation split

Filed after sign-off; each ticket is a follow-up of HS2-8T71YW and names the approved
`Design v1` captures it builds.

| #   | Ticket     | Scope                                                                                                                                                                                                                                                           | Blocked by             |
| --- | ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------- |
| 1   | HS2-9PYNCN | Registry storage and sync: `.hotsheet2/milestones/` format, lock, conflicted state, `.gitignore` line, commit and publish through the group registry machinery, `milestones_changed` (shipped, with the registry's create/list/show/edit/close/reopen surfaces) | —                      |
| 2   | HS2-SZF152 | Project time zone: shared `time_zone` key, validation, deadline-instant helper, Project Settings → General select                                                                                                                                               | —                      |
| 3   | HS2-A820C1 | Git model, index, roll-up and surfaces: ticket `milestone` and `due_on`, capabilities, index columns, `milestone_rollup`, CLI, MCP and HTTP per §38.6                                                                                                           | HS2-9PYNCN, HS2-SZF152 |
| 4   | HS2-NSVKGE | Milestones view and inspector, New/Edit/Close/Reopen/Delete dialogs, Move Open Tickets to…, /ux-demo states                                                                                                                                                     | HS2-A820C1             |
| 5   | HS2-Y9V4NB | Ticket Info tab Milestone section and Due row, composer, bulk edit, context-menu Set Milestone…                                                                                                                                                                 | HS2-A820C1             |
| 6   | HS2-2CR7H3 | Milestone prerequisite with both rules                                                                                                                                                                                                                          | HS2-A820C1, HS2-9PYNCN |
| 7   | HS2-QY5YRN | Due dates into scheduler work                                                                                                                                                                                                                                   | HS2-A820C1, HS2-SZF152 |
| 8   | HS2-6BKPP6 | `hs-milestone:<id>` labels and ticket due dates on GitHub, GitLab and Jira, with live tests; shares label plumbing with HS2-PQ4A2C                                                                                                                              | HS2-A820C1             |

HS2-PX50E8, the earlier single client ticket, is superseded by HS2-NSVKGE and HS2-Y9V4NB.
HS2-6E0H3X keeps branch association; a milestone may later name an integration branch.
