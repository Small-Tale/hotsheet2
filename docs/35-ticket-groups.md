# 35. Ticket Groups

Ticket groups let a person gather related tickets under one named, collapsible
container in the list and column views. The requirement and the approved mocks are
on `HS2-3XNTSD`. This document describes the shipped behavior; the field format is
in [17-ticket-file-format.md](17-ticket-file-format.md), storage and merge rules in
[02-ticket-storage.md](02-ticket-storage.md), the index and search syntax in
[03-indexing-and-query.md](03-indexing-and-query.md), and the HTTP, CLI, and MCP
surfaces in [04-core-server-cli.md](04-core-server-cli.md).

## 35.1 Model (HS2-PDSHNN)

- A ticket has an optional `group` name. Tickets are ungrouped by default, and a
  ticket belongs to at most one group.
- A group exists while at least one ticket that is not in Trash names it. There is
  no group registry and no empty group: a group whose last ticket leaves it is gone.
- Names are trimmed and inner whitespace runs collapse to one space. A name is at
  most 120 characters on a single line.
- Names are unique within a project. Two names that differ only by letter case or
  whitespace are the same group, and setting a ticket to such a name stores the
  existing group's spelling. A project that connects several Markdown/Git sources
  shares one group namespace.
- Ticketing stays provider-neutral. `ProviderCapabilities.groups` reports whether a
  source stores groups. Markdown/Git does. GitHub, GitLab, and Jira report `false`
  and reject a group write with an explicit unsupported error.
  [16-external-sync-interface.md](16-external-sync-interface.md) records the native
  mappings that were investigated and why none is enabled (HS2-TEN9X5).
- Setting or clearing a group does not change a ticket's `updated_at`, like Up Next
  and priority, so regrouping many tickets does not reorder a list sorted by
  last update.

## 35.2 Headless use

Every group operation the graphical client offers has a headless path that calls
the same workflow.

| Operation                     | CLI                                                           | MCP                                                           |
| ----------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| Set or clear a ticket's group | `hotsheet-cli edit <slug> --group <name>` / `--clear-group`   | `hotsheet_update` with `group` (a string, or `null` to clear) |
| Create a ticket in a group    | `hotsheet-cli new --group <name>`                             | `hotsheet_create` with `group`                                |
| List groups with counts       | `hotsheet-cli groups`                                         | `hotsheet_groups` with `action: "list"`                       |
| Filter tickets by group       | `hotsheet-cli ls --group <name>`, `--has-group`, `--no-group` | `hotsheet_query` with `group` or `has_group`                  |
| Rename a group                | `hotsheet-cli group rename <from> <to>`                       | `hotsheet_groups` with `action: "rename"`                     |
| Ungroup every member          | `hotsheet-cli group ungroup <name>`                           | `hotsheet_groups` with `action: "ungroup"`                    |

Rename and ungroup update every member in one atomic write. Renaming to the name of
a different existing group is refused; changing only the casing of a group's own
name is allowed.

## 35.3 Rendering (HS2-WZ3JGZ, HS2-HV8S4Y)

One shared `TicketGroup` component renders a group in both views: a Kerf
`SunkenPanel` holding a `ListHeader` disclosure (size `m`, wrapping, with a count)
and the group's tickets. The name keeps the user's casing.

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
  stored in the browser under `hotsheet.ticket-groups.collapsed`; it is never sent
  to the server. In the column view a collapsed group is collapsed in every column.
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
  submenu lists **New Group…** first, then the project's existing groups. Choosing
  a group moves every selected ticket into it, replacing any earlier membership.
- **New Group dialog.** A required Group Name field with Cancel and Create Group.
  A name that matches an existing group, compared case-insensitively, shows an
  inline error and disables Create Group. Submitting assigns the group to every
  selected ticket in one bulk update.
- **Remove from Group** appears when the selection contains a grouped ticket.
- **Drag and drop.** Dragging tickets over a group outlines the whole group; there
  is no insertion line because order follows the sort. Dropping sets the group. In
  the column view, dropping on a group in another column also applies that column's
  status, as one update.
- **Leaving a group by dragging.** In the list view, dropping a grouped ticket on
  the list outside every group removes its group. In the column view, dropping it
  in its own column outside every group removes its group; dropping it in another
  column outside a group changes only the status and keeps the group.
- **Writes** are ordinary ticket updates: optimistic, rolled back on failure, and
  undoable with the standard Undo shortcut.
- **Sources without group support.** When the selection or the dragged tickets
  include a source whose provider does not report `groups`, the menu entries are
  disabled with the reason and the group does not accept the drop.

## 35.5 Managing a group (HS2-8CDFKR)

Right-clicking a group header, or pressing the keyboard's context-menu key on it,
opens a menu with **Rename Group…** and **Ungroup**. Rename uses the same dialog as
New Group and enforces unique names. Both actions go through the server so they
also reach members that the current view has not loaded.

A visible overflow button on the header is preferred, but Kerf's `ListHeader`
cannot yet pair its toggle with a separate trailing action. That gap is filed
upstream as `KF-X2MRCT`; the context menu is the interim entry point and is removed
in favor of the overflow button when a Kerf release ships it.

## 35.6 Search (HS2-44W3TP)

- `group:name` and `group:"name with spaces"` match tickets in that group,
  case-insensitively.
- `has:group` matches any grouped ticket, so `NOT has:group` finds ungrouped ones.
- Both combine with `AND`, `OR`, `NOT`, and parentheses like every other filter.
- The search field completes `group:` from the project's group names, and the
  syntax help lists the group filters.
- Saved views accept the same terms.
