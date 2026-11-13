# 35. Ticket Groups

Ticket groups let a person gather related tickets under one named, collapsible
container in the list and column views. The requirement and the approved mocks are
on `HS2-3XNTSD`; the project group registry is `HS2-9KZSD4`. This document describes the
shipped behavior; the field format is
in [17-ticket-file-format.md](17-ticket-file-format.md), storage and merge rules in
[02-ticket-storage.md](02-ticket-storage.md), the index and search syntax in
[03-indexing-and-query.md](03-indexing-and-query.md), and the HTTP, CLI, and MCP
surfaces in [04-core-server-cli.md](04-core-server-cli.md).

## 35.1 Model (HS2-PDSHNN, HS2-9KZSD4)

- A ticket has an optional `group` **id**. Tickets are ungrouped by default, and a ticket
  belongs to at most one group. An id is ten uppercase Crockford base32 characters, for
  example `7K3M9P2QXA`.
- **Group registry.** Names belong to the project. The code checkout keeps one committed
  file per group, `.hotsheet2/groups/<id>.json` holding `{ "name": "…" }`, so two clones
  that create groups merge without a conflict. Hot Sheet commits and pushes these files
  itself (§35.7). One project can group tickets from all of
  its sources at once: every Markdown/Git source of a checkout shares the registry, and
  external sources join through HS2-PQ4A2C.
- A group is listed while at least one ticket that is not in Trash names it. A registry
  entry with no members is not a visible group, and neither entries nor ticket values are
  ever deleted automatically.
- Names are trimmed and inner whitespace runs collapse to one space. A name is at most
  120 characters on a single line. Names are unique within a project without regard to
  letter case or whitespace: setting a ticket's group by a name uses the existing group
  with that name and creates one only when none exists, and a rename to another listed
  group's name is refused.
- **Unknown ids.** A ticket can name an id that this project's registry does not name,
  for example a store shared with another project, or a registry not yet pulled. The group
  is listed under the placeholder `Group <id>` (`named: false`) and is never dropped;
  renaming it gives it a name in this project.
- **Duplicates.** Two clones can each create a group with the same name, leaving two ids.
  Both are listed and flagged `duplicate` until a merge moves one id's members onto the
  other. A duplicated name is ambiguous: operations that name it need the id.
- **Legacy names.** Before HS2-9KZSD4 the field held the display name. Such a value is
  still read, listed under its own name (`legacy: true`), and matched by name. The
  explicit migration rewrites it to a registry id (§35.2). Renaming a legacy group also
  gives it an id.
- **Stores without a project.** A store or connection addressed without its checkout
  (the default-store and `/stores` routes, `hotsheet-cli -C <store>` from outside a linked
  checkout) has no registry. It lists ids under placeholders, accepts ids only, and
  refuses create, rename, and migrate with `group_registry_required`.
- Ticketing stays provider-neutral. `ProviderCapabilities.groups` reports whether a
  source stores groups. Markdown/Git does. GitHub, GitLab, and Jira report `false` and
  reject a group write with an explicit unsupported error until their `hs-group:<id>`
  label mapping ships (HS2-PQ4A2C).
  [16-external-sync-interface.md](16-external-sync-interface.md) records the mappings that
  were investigated (HS2-TEN9X5).
- Setting or clearing a group does not change a ticket's `updated_at`, like Up Next and
  priority, so regrouping many tickets does not reorder a list sorted by last update.

## 35.2 Headless use

Every group operation the graphical client offers has a headless path that calls
the same workflow. Inside a code checkout linked to the store the CLI resolves names
through that checkout's registry; elsewhere it takes ids.

| Operation                     | CLI                                                                 | MCP                                                                |
| ----------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------ |
| Set or clear a ticket's group | `hotsheet-cli edit <slug> --group <id or name>` / `--clear-group`   | `hotsheet_update` with `group` (an id or name, or `null` to clear) |
| Create a ticket in a group    | `hotsheet-cli new --group <id or name>`                             | `hotsheet_create` with `group`                                     |
| List groups with counts       | `hotsheet-cli groups [--json]`                                      | `hotsheet_groups` with `action: "list"`                            |
| Filter tickets by group       | `hotsheet-cli ls --group <id or name>`, `--has-group`, `--no-group` | `hotsheet_query` with `group` or `has_group`                       |
| Create a group                | `hotsheet-cli group create <name>`                                  | `hotsheet_groups` with `action: "create"`                          |
| Rename a group                | `hotsheet-cli group rename <id or name> <to>`                       | `hotsheet_groups` with `action: "rename"`                          |
| Ungroup every member          | `hotsheet-cli group ungroup <id or name>`                           | `hotsheet_groups` with `action: "ungroup"`                         |
| Merge two groups              | `hotsheet-cli group merge <from> <into>`                            | `hotsheet_groups` with `action: "merge"`                           |
| Migrate legacy names to ids   | `hotsheet-cli group migrate`                                        | `hotsheet_groups` with `action: "migrate"`                         |

Rename is one registry edit and changes no ticket. Ungroup and merge update every member
in one atomic write per store. Renaming to the name of a different listed group is
refused; changing only the casing of a group's own name is allowed.

**Migration (HS2-9KZSD4).** `hotsheet-cli group migrate` (and `POST
/checkouts/{reference}/groups/migrate`) is the explicit format boundary of
[19-format-compatibility.md](19-format-compatibility.md) §19.1: reads never migrate on
their own. It creates one registry entry per distinct legacy name key, reusing an existing
entry with that key, and rewrites every ticket that still stores the name, Trash included,
in one write and one commit per store. A rerun finds nothing and changes nothing.

## 35.3 Rendering (HS2-WZ3JGZ, HS2-HV8S4Y)

One shared `TicketGroup` component renders a group in both views: a Kerf
`SunkenPanel` holding a `ListHeader` disclosure (size `m`, wrapping, with a count)
and the group's tickets. Groups are keyed by id (HS2-9KZSD4), so two groups with one name
render separately. The title is the registry name, which keeps the user's casing, a legacy
group's own name, or the `Group <id>` placeholder. The client reads names from `GET
/checkouts/{reference}/groups` when the project loads, after each group operation, and
once more when loaded tickets name an id it has no name for; it never polls.

- **Order.** A group appears where its first ticket would appear in the active
  sort, and its members follow that same sort. Ungrouped tickets keep their places.
- **List view.** The list splits into segments. Each run of ungrouped tickets is
  its own rounded card. Each group sits directly on the workspace, separated from
  its neighbors, and holds its own rounded card of member rows. No list surface is
  drawn behind a group.
- **Column view.** Each column shows only its own members of a group, where the
  group's first ticket in that column sorts, with a per-column count. One group can
  therefore appear in several columns at once.
- **Title alignment.** The group title starts on the left edge of the ticket type
  icons below it, in both views. The app sets only the public
  `--kui-layout-inline-margin` token on its own wrapper; a browser test measures the
  alignment.
- **Collapse.** Collapsing a group hides its tickets and keeps its header and count
  in place. The choice is a per-viewer preference for one project, view, and group,
  stored in the browser under `hotsheet.ticket-groups.collapsed` and keyed by group
  id; it is never sent to the server. In the column view a collapsed group is collapsed in every column.
- **Selection and keyboard.** Arrow keys, range selection, copy, and the reader's
  previous and next follow the rows as drawn, across group boundaries. Tickets
  hidden in a collapsed group are skipped, and Select All does not select them. The
  group toggle is a button with `aria-expanded`.
- **Pagination.** A group shows the members that are loaded. When a later page
  loads another member, it joins the group where the group already sits. A column's
  "N of M loaded" count is unchanged by grouping.
- **Filtered results** keep their grouping: a search shows matching tickets inside
  their group containers.

## 35.4 Changing membership (HS2-TX17CM, HS2-PRXD39)

- **Add to Group.** With one or more tickets selected, the ticket context menu and
  the toolbar's More Actions menu show **Add to Group** (Lucide `group`). Its
  submenu lists **New Group…** first, then the project's existing groups; a duplicated
  name shows its id in parentheses. Choosing
  a group moves every selected ticket into it, replacing any earlier membership.
- **New Group dialog.** A required Group Name field with Cancel and Create Group.
  A name that matches an existing group, compared case-insensitively, shows an
  inline error and disables Create Group. Submitting creates the group in the registry
  (`POST /checkouts/{reference}/groups`) and then assigns its id to every selected ticket
  in one bulk update.
- **Remove from Group** appears when the selection contains a grouped ticket.
- **Drag and drop.** Dragging tickets over a group outlines the whole group; there
  is no insertion line because order follows the sort. Dropping sets the group. In
  the column view, a group takes the drop only when every dragged ticket is already
  in that group's column (HS2-K2QA1J). A drag from another column is a status change:
  the whole column is the target even over one of its groups, and the drop changes
  only the status and keeps each ticket's group.
- **Leaving a group by dragging.** In the list view, dropping a grouped ticket on
  the list outside every group removes its group. In the column view, dropping it
  in its own column outside every group removes its group; dropping it in another
  column changes only the status and keeps the group.
- **Writes** are ordinary ticket updates: optimistic, rolled back on failure, and
  undoable with the standard Undo shortcut.
- **Sources without group support.** When the selection or the dragged tickets
  include a source whose provider does not report `groups`, the menu entries are
  disabled with the reason and the group does not accept the drop.

## 35.5 Managing a group (HS2-8CDFKR)

Right-clicking a group header, or pressing the keyboard's context-menu key on it,
opens a menu with **Rename Group…** and **Ungroup**. Rename uses the same dialog as
New Group and enforces unique names; it is a registry edit by id, and it also names a
placeholder group. A group flagged as a duplicate also offers **Merge Duplicate Groups**
(Lucide `merge`), which moves every other group with the same name into this one. These
actions go through the server so they also reach members that the current view has not
loaded. Migration has no graphical entry point; it is headless (§35.2).

A visible overflow button on the header is preferred, but Kerf's `ListHeader`
cannot yet pair its toggle with a separate trailing action. That gap is filed
upstream as `KF-X2MRCT`; the context menu is the interim entry point and is removed
in favor of the overflow button when a Kerf release ships it.

## 35.6 Search (HS2-44W3TP)

- `group:name` and `group:"name with spaces"` match tickets in that group,
  case-insensitively, through the project's registry names; `group:<id>` matches one id.
- `has:group` matches any grouped ticket, so `NOT has:group` finds ungrouped ones.
- Both combine with `AND`, `OR`, `NOT`, and parentheses like every other filter.
- The search field completes `group:` from the project's group names, and the
  syntax help lists the group filters.
- Saved views resolve `group:` through the checkout's registry on the server too, so
  notification rules match the same tickets.
- Saved views accept the same terms.

## 35.7 Publishing the registry (HS2-9EGWC4)

The registry lives in the code checkout, so Hot Sheet commits and publishes it the way the
ticket store publishes tickets, without disturbing anything else in that checkout.

- **Commit.** Every registry write (create, rename, a placeholder's name, migration),
  from the app, the server, the CLI, or MCP, is committed in the checkout. The commit
  holds only the changed `.hotsheet2/groups/*.json` files (`git commit --only`): other
  staged or unstaged changes stay exactly as they were. Messages are shaped like the
  ticket store's autocommits:
  - `Create group Sprint`
  - `Rename group 7K3M9P2QXA to Release train`
  - `Migrate 3 group names to the group registry`

  The server commits in the background and coalesces a burst of writes into one commit,
  such as `Create group Alpha (+2 more)`. The CLI and MCP commit as each write returns.

- **Push.** The commit is pushed to the current branch's upstream, with the repository's
  hooks, never forced and never with `--no-verify`, and only as a fast-forward that
  publishes registry commits alone. It stays local, and the sync status says why, when
  the branch has no upstream, is behind its upstream, or has other unpublished commits or
  a merge (those are the user's to publish). A refused push or a failing hook also leaves
  it local. `git pull --rebase` drops a local registry commit that the upstream already
  has; `git merge --ff-only` cannot fast-forward past it. Whether Hot Sheet should
  republish such a stranded commit itself is `HS2-TM7XSX`.
- **Commits land on the checked-out branch**, including a feature branch or a worktree's
  branch. They reach `main` when that branch does.
- **Waiting states.** A write never fails because of Git. When the checkout is mid
  merge, rebase, cherry-pick, or revert, or `HEAD` is detached, the write is left
  uncommitted and committed by the next write or sync pass. A checkout that is not a Git
  work tree, or that ignores `.hotsheet2/groups/`, is not committed at all; neither is any
  checkout while `HOTSHEET_NO_AUTOCOMMIT` is set.
- **Pull.** On the ticket store's sync cadence (every 30 s while the server runs), the
  server fetches the upstream branch of each checkout linked to a hosted store. Entries
  the upstream added or renamed, and that this checkout has not changed since the two
  histories split, are read from a machine-local overlay
  (`.hotsheet2/groups-remote.json`) so groups created in another clone appear without a
  manual pull. The branch and working tree are not touched; the files arrive with the
  user's next pull. A local edit always wins over the overlay. The same pass commits
  anything a busy state or a failing hook left behind and retries the push.
- **Status.** The last attempt is recorded in `.hotsheet2/groups-sync.json` as
  `synced`, `local_only`, `waiting`, `blocked`, or `failed`, with one line of detail; a
  registry Hot Sheet does not publish (no Git, ignored, disabled) has none. `GET /checkouts/{reference}/groups` returns it as `registry_sync`, and so does
  `hotsheet-cli groups --json`. `hotsheet-cli groups` prints a warning for `blocked` and
  `failed`, and the server logs them. Showing it in the graphical client is
  `HS2-VFYSQK`.

**This repository's hooks.** Measured on a loaded maintainer machine, the pre-commit
format check took 64-78 s, a pre-push of `main` needs a main-integration window and is
refused without one, and a pre-push of another branch spent 28 s before refusing without a
reviewed test plan. Both hooks therefore pass a registry-only change on a narrow path:
`pre-commit` runs only the whitespace check when every staged path is a registry entry,
and `pre-push` accepts a ref update that is a fast-forward, contains no merge, and touches
only registry entries. Anything else takes the full gate. A registry push to `main` does
not take the main-integration window; an integrator whose window it overtakes re-runs its
preflight.
