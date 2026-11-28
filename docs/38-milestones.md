# 38. Milestones and major features

Status: **Design — pending maintainer review** (HS2-8T71YW). Nothing here is built.
The open questions are the CHOICE list on HS2-8T71YW; the recommendation is written into
each section and marked _(CHOICE n)_ so a different answer changes only that paragraph.

From investigation HS2-7147P8. Implementation: model, index, CLI and MCP HS2-A820C1;
client HS2-PX50E8; branch association HS2-6E0H3X; orchestration consumers HS2-2CR7H3
(milestone prerequisite) and HS2-QY5YRN (due dates in scheduler scoring).

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
Gantt scheduling, enforcing a branching strategy (HS2-6E0H3X observes branches), and
cross-project roll-ups (Workspace Grid aggregation is HS2-42B2FD).

## 38.2 Model

### Milestone record (provider-neutral)

```text
Milestone {
  ref:          { connection_id, native_id }   # qualified, like every ticket reference
  title:        string (1..=120, single line)
  description:  markdown | null
  due_on:       YYYY-MM-DD | null
  state:        open | closed
  ticket:       TicketRef | null               # the ticket that backs it, when the provider has one
}
```

A milestone is addressed like a ticket, by `(connection_id, native_id)`, and is listed per
connection. A project shows the milestones of every ticket source linked to its checkout.

### Markdown/Git: a milestone is a ticket _(CHOICE 1)_

In a Git store a milestone **is a ticket** whose category is the reserved id `milestone`.
It therefore has notes, attachments, FEEDBACK NEEDED, assignment, history and sync for free,
and its slug is how people name it (`HS2-R1M0A2`). Mapping:

| Milestone field | Ticket field                                                                                                   |
| --------------- | -------------------------------------------------------------------------------------------------------------- |
| `title`         | `title`                                                                                                        |
| `description`   | body                                                                                                           |
| `due_on`        | `due_on` (§38.3)                                                                                               |
| `state`         | `open` while the status is not finished; `closed` when completed, verified, archived or closed with any reason |

`milestone` is a **built-in category**: every project has it whether or not its
`.hotsheet2/settings.json` lists it; settings may change its label and color, never its
id, and cannot delete it. A milestone ticket is excluded from `claim-next`,
`plan-dispatch` and automatic orchestration selection (an explicit `claim` still works),
and from Up Next counts. It cannot be moved to a provider without `milestones` (§38.5).

Rejected alternative: a registry of milestone files beside the group registry
(`.hotsheet2/milestones/<id>.json`). It merges well but has no notes, attachments or
FEEDBACK NEEDED, which is where milestone clarity questions belong.

### The link field

Any ticket that is not itself a milestone may name **one** milestone:

```yaml
milestone: { project_id: <project-id>, connection_id: <git-source-id>, native_id: 01KAB… }
```

- The shape is the `follow_ups` entry shape (docs/17), so it is qualified across project
  sources; `project_id` is omitted when the milestone is in the same store.
- One milestone per ticket keeps every roll-up unambiguous. Moving a ticket between
  milestones is one edit.
- A milestone ticket cannot carry a `milestone` _(CHOICE 3: no nesting in the first
  version)_. A larger release that needs sub-milestones uses two projects, or tags.
- Setting or clearing it does not change `updated_at` (as for `group`, Up Next and
  priority), so re-planning many tickets does not reorder a list sorted by last update.
- It is a scalar for merge: concurrent different values take the field-conflict path of
  docs/02 §2.7 (HS2-JJ3R1G), never a silent pick.
- A dangling link (milestone deleted, moved, or in an unlinked source) is kept, never
  dropped; the ticket reads `milestone_state: unavailable` and is listed under
  "Unavailable milestone" until a person clears or relinks it.
- A milestone's own deletion or move does not rewrite its tickets. Moving a milestone
  ticket between stores leaves the move tombstone, and links resolve through it (docs/02
  §2.13).

## 38.3 Due dates

Every ticket, milestone or not, may carry an optional `due_on` calendar date
(`YYYY-MM-DD`). A milestone's date is its ticket's `due_on`.

- A date, not a time: every provider stores a date (GitHub milestone `due_on`, GitLab
  `due_date`, Jira `duedate` and version `releaseDate`).
- Deadline instant _(CHOICE 6)_: the end of that day in UTC (`T23:59:59.999Z`). One
  instant on every machine keeps scheduler scores identical across machines; scoring buckets
  are a day wide, so the offset matters little.
- A ticket's **effective due** is the earlier of its own `due_on` and its open milestone's
  `due_on`. That is what the scheduler scores (§38.8) and what the `due:` search token reads.
- `due_on` changes do not change `updated_at`.

## 38.4 Progress, risk and clarity

One pure function computes the roll-up, `milestone_rollup(milestone, tickets, now,
thresholds)` in `hotsheet-ticketing`, so CLI, MCP, HTTP and the orchestrator agree.

**Members** are the tickets whose `milestone` names it, across every source the reading
checkout links. Trash, `moved` tombstones (their destination counts instead) and tickets
closed as `not_planned`, `duplicate` or `obsolete` are **excluded** and counted separately
as `dropped`.

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
| Risk    | `overdue`         | Open, and its effective due is past                                                               |
| Clarity | `feedback_needed` | It has an open FEEDBACK NEEDED request (also checked on the milestone ticket itself)              |
| Clarity | `no_details`      | Open, and its body is empty                                                                       |

**Health** is one word derived from the above, never set by hand:

| Health      | When                                                               |
| ----------- | ------------------------------------------------------------------ |
| `done`      | Closed, or every member finished                                   |
| `off_track` | Due date past with open members                                    |
| `at_risk`   | Any risk signal, or due within 7 days with less than 80 % finished |
| `unclear`   | No risk, but a clarity signal                                      |
| `on_track`  | Otherwise (including no due date)                                  |

Closing a milestone with open members is allowed. The response and the UI list the open
members; the client offers **Move Open Tickets to…** (one atomic write per store). Nothing
moves on its own.

## 38.5 Providers

New `ProviderCapabilities` members, each `#[serde(default)]` false:

| Capability       | Means                                                       |
| ---------------- | ----------------------------------------------------------- |
| `milestones`     | Lists milestones; tickets carry and change a milestone link |
| `milestone_edit` | Creates, renames, dates, closes and reopens milestones      |
| `due_dates`      | Tickets carry and change `due_on`                           |

A write the source cannot hold fails before any request with
`provider connection '<id>' does not support '<capability>'`, as `groups` does. The client
keeps the control in place, disabled, with that reason. A cross-provider copy drops the
link and date the destination cannot hold, and names what it dropped in the operation result.

| Provider     | Milestone is                                              | `milestones` | `milestone_edit` | `due_dates` | Notes                                                                                                                                                                                                 |
| ------------ | --------------------------------------------------------- | ------------ | ---------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Markdown/Git | A ticket of category `milestone` (§38.2)                  | yes          | yes              | yes         | Cross-store links allowed                                                                                                                                                                             |
| GitHub       | A repository milestone (`native_id` `milestone/<number>`) | yes          | yes              | no          | Issues have no due date. `state` and `due_on` map 1:1; no notes or attachments (`ticket: null`)                                                                                                       |
| GitLab       | A project milestone (`milestone/<iid>`)                   | yes          | yes              | yes         | Issue `due_date`. Group milestones are read-only (`milestone_edit` per milestone: refused)                                                                                                            |
| Jira         | A fixVersion (`version/<id>`) _(CHOICE 2)_                | yes          | yes (admin)      | yes         | `releaseDate` is the date, `released` is closed. An issue with several fixVersions reads the earliest, reports `multiple_milestones`, and a write replaces the set only with `replace_versions: true` |

External milestones are links within one connection: a GitHub issue can only name a
milestone of its own repository. Each mapping is its own follow-up with live provider
tests against the safe test repositories, after the Git implementation; until then each
external provider reports all three as false.

## 38.6 Surfaces

Every operation has CLI, MCP and HTTP parity (AGENTS.md headless rule).

| Operation                     | CLI                                                                                 | MCP                                                         | HTTP                                                    |
| ----------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------- | ------------------------------------------------------- |
| Create a milestone            | `new --category milestone --title 1.0 [--due 2026-12-15]`                           | `hotsheet_create` with `category: milestone`                | `POST .../tickets` (category `milestone`)               |
| Link / unlink a ticket        | `edit <slug> --milestone <slug or ref>` / `--clear-milestone`                       | `hotsheet_update` `milestone` (ref or `null`)               | `PATCH .../tickets/{id}` `milestone`                    |
| Create into a milestone       | `new --milestone <slug>`                                                            | `hotsheet_create` `milestone`                               | `POST .../tickets` `milestone`                          |
| Set / clear a due date        | `edit <slug> --due <date>` / `--clear-due`                                          | `hotsheet_update` `due_on`                                  | `PATCH` `due_on`                                        |
| List milestones with roll-ups | `milestones [--all] [--json]`                                                       | `hotsheet_milestones` `action: list`                        | `GET /checkouts/{ref}/milestones[?all=true]`            |
| One milestone's roll-up       | `milestone show <slug> [--json]`                                                    | `hotsheet_milestones` `action: show`                        | `GET /checkouts/{ref}/milestones/{connection}/{native}` |
| Move open members             | `milestone move-open <from> <to>`                                                   | `hotsheet_milestones` `action: move_open`                   | `POST .../milestones/{connection}/{native}/move-open`   |
| Filter tickets                | `ls --milestone <slug>`, `--has-milestone`, `--no-milestone`, `--due-before <date>` | `hotsheet_query` `milestone`, `has_milestone`, `due_before` | `GET .../tickets?milestone=&has_milestone=&due_before=` |

`--all` adds closed milestones. A slug resolves only within the checkout's sources;
elsewhere the exact `@project/connection:native-id` reference is required. Rows of every
list carry `milestone` and `due_on` when set.

**Search** (saved views and the search field): `milestone:"1.0"` or `milestone:HS2-R1M0A2`,
`has:milestone`, `due:<2026-12-01`, `due:<=7d`, `is:overdue`.

**Index.** Two derived columns, `milestone_key` (qualified reference text) and
`effective_due_on`; the index rebuilds on the schema bump (docs/03 §3.2).
`Index::milestone_members` feeds the roll-up.

**Live updates.** No new event: a member's ticket change event names its old and new
milestone, and a client holding that milestone's roll-up refetches it once. Nothing polls.

## 38.7 Client

Placement _(CHOICE 5)_, in the existing project shell; captures on HS2-8T71YW, batch
`Design v1`.

- **Milestones view.** The project sidebar gains a built-in **Milestones** view after the
  ticket views. The workspace lists open milestones in due-date order (undated last), each
  a framed row: title, due date, a `ProgressBar` with "12 of 20 finished", a health `Badge`,
  and risk and clarity counts. Closed milestones collapse into a disclosure at the end.
- **Milestone inspector.** Selecting a milestone shows, in the right rail, a `ValueTable`
  (Due, Progress, Health, Awaiting CI, Dropped), then three `ListHeader` + `ListActionRow`
  sections: **Blocked**, **At Risk**, **Needs Answers**. Each row opens its ticket.
  Header actions: **Show Tickets** (the ticket list filtered `milestone:`), **Edit…**,
  **Close Milestone…**.
- **Ticket Info tab.** A **Milestone** section above Blocked By, built like it: a
  `ListHeader` with **Set Milestone…** and one `ListActionRow` naming the milestone, its
  due date and progress, with a trailing clear button; a **Due** row in the Info
  `ValueTable`. A source without the capability keeps both, disabled, with the reason.
- **Composer and bulk.** The new-ticket composer and the bulk-edit dialog accept a
  milestone; a list or column selection gets **Set Milestone…** in its context menu.
- **Rows.** No new row element in the first version: the shared summary-row contract stays
  as it is; `milestone:` search and the filtered list cover finding members.
- **Phone.** The Milestones view is a list; the inspector opens as the overlay rail.

## 38.8 Orchestration

### Milestone prerequisite (HS2-2CR7H3) _(CHOICE 4)_

A new prerequisite kind beside `ticket` and `validation` (docs/26 §26.15):

| Kind        | Fields                                             | Holds when                                                                                                       |
| ----------- | -------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `milestone` | `target_id`, `connection_id`, `native_id`, `gates` | The milestone is readable and has at least one member, and every member is confirmed finished with `gates` green |

- Defining the step reads the milestone and its members through the gate as
  `tickets_read` and stores each member as an observation, so membership is a fact this
  machine confirmed, like any ticket read. Readiness refresh re-reads membership.
- Level: a member confirmed not finished, or with a red gate, is `blocked`
  (`milestone_member_open`, `milestone_member_red_gate`, each naming the member); an
  unreadable member or milestone is `stale`; a member resting on an unconfirmed
  integrated commit is `provisional`. An empty milestone is `blocked`
  (`milestone_empty`); a milestone closed as not planned is `blocked`
  (`milestone_not_planned`).
- A member that another step works is an edge of the plan graph, so a cycle through a
  milestone is found. At most 256 members (`milestone_too_large`), keeping reads bounded.
- A member added after a step was confirmed makes it not hold again at the next refresh;
  readiness gates only the start, so a running step is untouched.
- The milestone ticket's own status does not matter: closing a milestone is a person's
  release act and may be the very step this prerequisite gates.
- CLI `orchestration readiness define ... --requires-milestone <target>:<slug>`, MCP
  `readiness_define` `prerequisites[].kind: milestone`, the same route. The inspector's
  Plan and Prerequisites sections word it ("Milestone 1.0: 3 of 20 not finished: API-12,
  API-15, API-19").

### Due dates in scheduling (HS2-QY5YRN)

The scheduler already scores `WorkItem.due_at_ms` (`due`: overdue 2000, within a day 1000,
within a week 300; docs/26 §26.17). The target ticket read (`TicketSeen`, `ReadyItem`) gains
`due_on` and `milestone`; readiness reads the milestone's `due_on` with it, and
`due_at_ms` is the deadline instant (§38.3) of the effective due. No new score term.

## 38.9 Format and compatibility

- `milestone` and `due_on` are optional frontmatter keys, omitted when unset. Older readers
  keep unknown keys verbatim (docs/19 §19.2, as `orchestration_links`), so no version
  marker is needed and files written before this read with neither.
- The `milestone` category is an ordinary string to an older reader: it shows the ticket
  under an unknown category and lets it be claimed. Acceptable before the first public
  release (§19.1); recorded in the release notes of the version that ships it.
- External providers start at false for all three capabilities, so nothing changes for them
  until their mapping ships.
- The index schema bump rebuilds the derived index; no ticket file is rewritten.
- No migration: there is no earlier milestone data. A project that used a `milestone` tag or
  group keeps it; converting is a person's act (relink with `edit --milestone` or bulk edit).

## 38.10 Test plan (implementation is T2)

| Layer                | Covers                                                                                                                                                                                                                                                                         |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Model unit           | Parse and serialize round trip; omitted when unset; unknown-key preservation; link to self, to a milestone that carries a link, and from a milestone refused                                                                                                                   |
| Merge unit           | Concurrent different `milestone` / `due_on` values take the field-conflict path; same value merges clean                                                                                                                                                                       |
| Roll-up unit         | State matrix: each member status and close reason in and out; reopen; move (tombstone excluded, destination counted); trash and restore; empty then refill; cross-store member; dangling link; each signal at, below and above its threshold; health precedence                |
| Index                | Members by key across stores; rebuild after the schema bump; effective due with and without a milestone date                                                                                                                                                                   |
| CLI / MCP / HTTP     | Parity per §38.6 on a real server with isolated homes; slug and exact-reference resolution; capability refusal before any request                                                                                                                                              |
| Providers            | Fakes matching real GitHub, GitLab and Jira shapes (including several fixVersions); live tests in each mapping's follow-up                                                                                                                                                     |
| Readiness            | Milestone prerequisite: member added after confirm, member reopened, red gate, unreadable member, empty, not planned, cycle through a member, provisional member, more than 256 members; order-independence                                                                    |
| Scheduler            | Effective due from ticket only, milestone only, both (earlier wins), closed milestone ignored                                                                                                                                                                                  |
| Browser (Playwright) | Milestones view and inspector; Set Milestone… and clear, both ways (control to state, state to control); roll-up updates on a member's change event without polling; disabled controls on a source without the capability; 1440, 1100 and 390 px in both color schemes; WebKit |
| Visual QA            | docs/manual-test-plan.md rows for the view, inspector and Info section                                                                                                                                                                                                         |

## 38.11 Proposed implementation split (filed after sign-off)

1. HS2-A820C1 — Git model (`milestone`, `due_on`, built-in category), index, roll-up,
   CLI, MCP and HTTP.
2. HS2-PX50E8 — client: Milestones view, inspector, Info section, composer, bulk, search.
3. HS2-2CR7H3 — milestone prerequisite kind.
4. HS2-QY5YRN — due dates into scheduler work.
5. New, one each: GitHub, GitLab and Jira mappings; Move Open Tickets to….
6. HS2-6E0H3X keeps branch association; a milestone may later name an integration branch.
