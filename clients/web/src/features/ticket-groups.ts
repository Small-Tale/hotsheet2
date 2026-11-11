import type { Signal } from 'kerfjs';

import type { Api, Capabilities, TicketRow as WireTicketRow } from '../api';
import type { Project } from '../interactions/types';
import { type BulkTicketAction, ticketGroupDisabledReason } from '../ticket-bulk-operations';
import {
  collapsedTicketGroupKeys,
  reportedTicketGroups,
  ticketGroupCollapsed,
  ticketGroupDialog,
  ticketGroupDropTarget,
  toggleTicketGroupCollapsed,
} from '../ticket-group-state';
import {
  existingTicketGroup,
  normalizeTicketGroupName,
  ticketGroupKey,
  ticketGroupName,
  ticketGroupNameProblem,
  ticketGroupSummaries,
  withoutCollapsedGroupMembers,
} from '../ticket-groups';

/** Live application bindings the ticket groups controller reads (HS2-3XNTSD). */
export interface TicketGroupsControllerDependencies {
  project: () => Project | undefined;
  api: () => Api;
  tickets: Signal<WireTicketRow[]>;
  /** The collapse preference is kept per project view. */
  selectedView: { readonly value: string };
  capabilitiesFor: (connectionId: string) => Capabilities | undefined;
  executeBulkTicketAction: (action: BulkTicketAction, targetSlugs?: string[]) => Promise<boolean>;
  refreshProject: (options?: { showLoading?: boolean }) => Promise<unknown>;
  showToast: (message: string) => void;
}

export type TicketGroupsController = ReturnType<typeof createTicketGroupsController>;

/**
 * Ticket group behavior shared by the menu, the dialog, the group header, and drag and drop
 * (HS2-TX17CM, HS2-PRXD39, HS2-8CDFKR). Membership changes are ordinary bulk ticket updates, so
 * they are optimistic, roll back on failure, and can be undone. Renaming or ungrouping a whole
 * group goes through the server, which also reaches members this view has not loaded.
 */
export function createTicketGroupsController(dependencies: TicketGroupsControllerDependencies) {
  const { project, api, tickets, selectedView, capabilitiesFor, executeBulkTicketAction, refreshProject, showToast } =
    dependencies;
  let session = 0;
  /** The open dialog's session, read fresh after an await. */
  const openSession = () => ticketGroupDialog.value?.session;
  const view = () => selectedView.value,
    rowsFor = (slugs: readonly string[]) => tickets.value.filter((ticket) => slugs.includes(ticket.slug)),
    membersOf = (group: string) =>
      tickets.value.filter((ticket) => ticketGroupKey(ticketGroupName(ticket) ?? '') === ticketGroupKey(group));

  /** Every known group name: the loaded tickets' groups plus those the server last reported. */
  function groupNames(): string[] {
    const current = project(),
      names = ticketGroupSummaries(tickets.value).map((group) => group.name);
    for (const name of (current && reportedTicketGroups.value[current.id]) ?? [])
      if (!existingTicketGroup(name, names)) names.push(name);
    return names.sort((left, right) => left.localeCompare(right, undefined, { sensitivity: 'base' }));
  }
  /** Ask the server for the project's groups; an older server or a failed read keeps the last list. */
  function refreshGroupNames(): void {
    const current = project();
    if (!current) return;
    void api()
      .checkoutGroups(current.id)
      .then((result) => {
        reportedTicketGroups.value = {
          ...reportedTicketGroups.value,
          [current.id]: result.groups.map((group) => group.name),
        };
        const dialog = ticketGroupDialog.value;
        if (dialog?.projectId === current.id) ticketGroupDialog.value = { ...dialog, groups: groupNames() };
      })
      .catch(() => undefined);
  }
  const isCollapsed = (group: string) => {
    const current = project();
    return Boolean(current && ticketGroupCollapsed(current.id, view(), group));
  };
  /** The `collapsedGroups` and drop-target props for a collection in `scope` (`list` or a column id). */
  function collectionProps(scope: string): { collapsedGroups: string[]; dropGroup: string | undefined } {
    const current = project(),
      drop = ticketGroupDropTarget.value;
    return {
      collapsedGroups: current ? collapsedTicketGroupKeys(current.id, view(), groupNames()) : [],
      dropGroup: drop?.scope === scope ? drop.key : undefined,
    };
  }
  function toggleGroup(group: string): void {
    const current = project();
    if (current) toggleTicketGroupCollapsed(current.id, view(), group);
  }
  /** Rows a person can reach: members of a collapsed group are hidden (HS2-WZ3JGZ). */
  const navigable = (rows: readonly WireTicketRow[]) => withoutCollapsedGroupMembers(rows, isCollapsed);
  const disabledReason = (slugs: readonly string[]) => ticketGroupDisabledReason(rowsFor(slugs), capabilitiesFor);

  function setGroup(slugs: string[], group: string | null, status?: string): Promise<boolean> {
    const reason = disabledReason(slugs);
    if (reason) {
      showToast(reason);
      return Promise.resolve(false);
    }
    // Reuse an existing group's spelling so the optimistic rows join it at once.
    const name = group === null ? null : (existingTicketGroup(group, groupNames()) ?? normalizeTicketGroupName(group));
    return executeBulkTicketAction({ kind: 'group', group: name, status }, slugs);
  }
  function focusNameField(): void {
    queueMicrotask(() => {
      const field = document.querySelector<HTMLElement & { select?: () => void }>('[name="ticket-group-name"]');
      field?.focus();
      field?.select?.();
    });
  }
  function openNewGroup(slugs: string[]): void {
    const current = project();
    if (!current || slugs.length === 0 || disabledReason(slugs)) return;
    ticketGroupDialog.value = {
      mode: 'new',
      name: '',
      groups: groupNames(),
      session: ++session,
      projectId: current.id,
      slugs: [...slugs],
    };
    refreshGroupNames();
    focusNameField();
  }
  function openRenameGroup(group: string): void {
    const current = project();
    if (!current || ticketGroupDisabledReason(membersOf(group), capabilitiesFor)) return;
    ticketGroupDialog.value = {
      mode: 'rename',
      name: group,
      group,
      groups: groupNames(),
      session: ++session,
      projectId: current.id,
      slugs: [],
    };
    refreshGroupNames();
    focusNameField();
  }
  function editDialogName(name: string): void {
    const dialog = ticketGroupDialog.value;
    if (dialog && dialog.name !== name) ticketGroupDialog.value = { ...dialog, name, error: undefined };
  }
  function closeDialog(): void {
    if (!ticketGroupDialog.value?.busy) ticketGroupDialog.value = undefined;
  }
  async function submitDialog(typed: string): Promise<void> {
    const dialog = ticketGroupDialog.value,
      current = project();
    if (!dialog || dialog.busy || current?.id !== dialog.projectId) return;
    const name = normalizeTicketGroupName(typed);
    if (ticketGroupNameProblem(name, dialog.groups, dialog.mode === 'rename' ? dialog.group : undefined)) {
      ticketGroupDialog.value = { ...dialog, name: typed };
      return;
    }
    if (dialog.mode === 'new') {
      ticketGroupDialog.value = undefined;
      await setGroup(dialog.slugs, name);
      return;
    }
    if (name === dialog.group) {
      ticketGroupDialog.value = undefined;
      return;
    }
    ticketGroupDialog.value = { ...dialog, name: typed, busy: true, error: undefined };
    try {
      await api().renameCheckoutGroup(current.id, dialog.group!, name);
      if (openSession() === dialog.session) ticketGroupDialog.value = undefined;
      // Keep the group's collapse choice under its new name.
      if (ticketGroupCollapsed(current.id, view(), dialog.group!) && !ticketGroupCollapsed(current.id, view(), name)) {
        toggleTicketGroupCollapsed(current.id, view(), dialog.group!);
        toggleTicketGroupCollapsed(current.id, view(), name);
      }
      await refreshProject({ showLoading: false });
      refreshGroupNames();
    } catch (reason) {
      if (openSession() === dialog.session)
        ticketGroupDialog.value = {
          ...dialog,
          name: typed,
          busy: false,
          error: reason instanceof Error ? reason.message : String(reason),
        };
    }
  }
  async function ungroup(group: string): Promise<void> {
    const current = project();
    if (!current) return;
    const reason = ticketGroupDisabledReason(membersOf(group), capabilitiesFor);
    if (reason) {
      showToast(reason);
      return;
    }
    try {
      const result = await api().ungroupCheckoutGroup(current.id, group);
      await refreshProject({ showLoading: false });
      refreshGroupNames();
      showToast(`Ungrouped ${result.updated} ticket${result.updated === 1 ? '' : 's'} from “${group}”.`);
    } catch (failure) {
      showToast(`Ungroup failed: ${failure instanceof Error ? failure.message : String(failure)}`);
    }
  }
  return {
    groupNames,
    refreshGroupNames,
    collectionProps,
    toggleGroup,
    isCollapsed,
    navigable,
    disabledReason,
    setGroup,
    openNewGroup,
    openRenameGroup,
    editDialogName,
    closeDialog,
    submitDialog,
    ungroup,
  };
}
