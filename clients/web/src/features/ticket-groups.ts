import type { Signal } from 'kerfjs';

import type { Api, Capabilities, TicketRow as WireTicketRow } from '../api';
import type { Project } from '../interactions/types';
import type { CheckoutGroupSummary } from '../server-response-parsers';
import { type BulkTicketAction, ticketGroupDisabledReason } from '../ticket-bulk-operations';
import {
  collapsedTicketGroupKeys,
  projectTicketGroups,
  ticketGroupCollapsed,
  ticketGroupDialog,
  ticketGroupDropTarget,
  toggleTicketGroupCollapsed,
} from '../ticket-group-state';
import {
  isTicketGroupId,
  normalizeTicketGroupName,
  ticketGroupDisplayName,
  ticketGroupKey,
  ticketGroupNameKey,
  ticketGroupNameProblem,
  type TicketGroupOption,
  ticketGroupSummaries,
  ticketGroupValue,
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

/** A known group: the server's summary, or a loaded ticket's group the server has not listed yet. */
type KnownTicketGroup = TicketGroupOption & Partial<Pick<CheckoutGroupSummary, 'legacy' | 'named'>>;

const byName = (left: TicketGroupOption, right: TicketGroupOption) =>
  left.name.localeCompare(right.name, undefined, { sensitivity: 'base' }) || left.id.localeCompare(right.id);
const failureMessage = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/**
 * Ticket group behavior shared by the menu, the dialog, the group header, and drag and drop
 * (HS2-TX17CM, HS2-PRXD39, HS2-8CDFKR). Membership changes are ordinary bulk ticket updates, so
 * they are optimistic, roll back on failure, and can be undone. Renaming, ungrouping, or merging
 * a whole group goes through the server, which also reaches members this view has not loaded.
 * Groups are identified by their stored value; display names come from the project's group
 * registry summaries (HS2-9KZSD4).
 */
export function createTicketGroupsController(dependencies: TicketGroupsControllerDependencies) {
  const { project, api, tickets, selectedView, capabilitiesFor, executeBulkTicketAction, refreshProject, showToast } =
    dependencies;
  let session = 0;
  /** The project whose summaries were last requested, and the missing ids last asked about. */
  let syncedProject: string | undefined, syncedMissing: string | undefined;
  /** The open dialog's session, read fresh after an await. */
  const openSession = () => ticketGroupDialog.value?.session;
  const view = () => selectedView.value,
    rowsFor = (slugs: readonly string[]) => tickets.value.filter((ticket) => slugs.includes(ticket.slug)),
    membersOf = (group: string) =>
      tickets.value.filter((ticket) => {
        const value = ticketGroupValue(ticket);
        return value !== undefined && ticketGroupKey(value) === ticketGroupKey(group);
      }),
    summaries = (): readonly CheckoutGroupSummary[] => {
      const current = project();
      return (current && projectTicketGroups.value[current.id]) ?? [];
    };

  /** Every known group: the server's summaries plus loaded tickets' groups it has not listed. */
  function groups(): KnownTicketGroup[] {
    const listed = summaries(),
      known: KnownTicketGroup[] = listed.map(({ id, name, duplicate, legacy, named }) => ({
        id,
        name,
        duplicate,
        legacy,
        named,
      })),
      keys = new Set(known.map((group) => ticketGroupKey(group.id)));
    for (const loaded of ticketGroupSummaries(tickets.value, listed))
      if (!keys.has(ticketGroupKey(loaded.id))) {
        keys.add(ticketGroupKey(loaded.id));
        known.push({ id: loaded.id, name: loaded.name, legacy: !isTicketGroupId(loaded.id) });
      }
    return known.sort(byName);
  }
  /** Every known group's display name, for duplicate checks and search completion. */
  function groupNames(): string[] {
    return groups().map((group) => group.name);
  }
  const groupFor = (value: string) => groups().find((group) => ticketGroupKey(group.id) === ticketGroupKey(value));
  /** The display name of a stored group value. */
  const displayName = (value: string) => ticketGroupDisplayName(value, summaries());

  /** Display names the dialog's typed name must not repeat; a renamed group's own name is allowed. */
  function dialogNames(renamed?: string): string[] {
    return groups()
      .filter((group) => renamed === undefined || ticketGroupKey(group.id) !== ticketGroupKey(renamed))
      .map((group) => group.name);
  }
  /** Remember one project's summaries and refresh an open dialog's duplicate list. */
  function storeSummaries(projectId: string, list: readonly CheckoutGroupSummary[]): void {
    projectTicketGroups.value = { ...projectTicketGroups.value, [projectId]: list };
    const dialog = ticketGroupDialog.value;
    if (dialog?.projectId === projectId) ticketGroupDialog.value = { ...dialog, groups: dialogNames(dialog.group) };
  }
  /** Ask the server for the project's groups; an older server or a failed read keeps the last list. */
  function refreshGroups(): Promise<void> {
    const current = project();
    if (!current) return Promise.resolve();
    syncedProject = current.id;
    return api()
      .checkoutGroups(current.id)
      .then((result) => {
        storeSummaries(current.id, result.groups);
      })
      .catch(() => undefined);
  }
  /**
   * Load the summaries when the project changes, and again when loaded tickets store ids the last
   * summaries do not name, at most once per distinct set of missing ids (HS2-9KZSD4). It runs
   * when tickets change, never on a timer.
   */
  function syncGroups(): void {
    const current = project();
    if (!current) return;
    if (syncedProject !== current.id) {
      syncedMissing = undefined;
      void refreshGroups();
      return;
    }
    const listed = new Set(summaries().map((group) => ticketGroupKey(group.id))),
      missing = [
        ...new Set(
          tickets.value
            .map(ticketGroupValue)
            .filter((value): value is string => value !== undefined && isTicketGroupId(value) && !listed.has(value)),
        ),
      ]
        .sort()
        .join(' ');
    if (!missing || missing === syncedMissing) return;
    syncedMissing = missing;
    void refreshGroups();
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
      collapsedGroups: current
        ? collapsedTicketGroupKeys(
            current.id,
            view(),
            groups().map((group) => group.id),
          )
        : [],
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

  /**
   * The value to store for a chosen group: a known group's stored value, the one group a typed
   * display name names, or the typed name itself, which the server resolves through the registry.
   */
  function storedValue(group: string): string {
    const known = groupFor(group);
    if (known) return known.id;
    const key = ticketGroupNameKey(group),
      named = groups().filter((option) => ticketGroupNameKey(option.name) === key);
    return named.length === 1 ? named[0].id : normalizeTicketGroupName(group);
  }
  function setGroup(slugs: string[], group: string | null, status?: string): Promise<boolean> {
    const reason = disabledReason(slugs);
    if (reason) {
      showToast(reason);
      return Promise.resolve(false);
    }
    return executeBulkTicketAction({ kind: 'group', group: group === null ? null : storedValue(group), status }, slugs);
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
      groups: dialogNames(),
      session: ++session,
      projectId: current.id,
      slugs: [...slugs],
    };
    void refreshGroups();
    focusNameField();
  }
  function openRenameGroup(group: string): void {
    const current = project();
    if (!current || ticketGroupDisabledReason(membersOf(group), capabilitiesFor)) return;
    ticketGroupDialog.value = {
      mode: 'rename',
      name: displayName(group),
      group,
      groups: dialogNames(group),
      session: ++session,
      projectId: current.id,
      slugs: [],
    };
    void refreshGroups();
    focusNameField();
  }
  function editDialogName(name: string): void {
    const dialog = ticketGroupDialog.value;
    if (dialog && dialog.name !== name) ticketGroupDialog.value = { ...dialog, name, error: undefined };
  }
  function closeDialog(): void {
    if (!ticketGroupDialog.value?.busy) ticketGroupDialog.value = undefined;
  }
  /** Name a new group in the registry, then move the dialog's tickets into it (HS2-9KZSD4). */
  async function createGroup(
    dialog: NonNullable<typeof ticketGroupDialog.value>,
    name: string,
    typed: string,
  ): Promise<void> {
    ticketGroupDialog.value = { ...dialog, name: typed, busy: true, error: undefined };
    let created: { id: string; name: string };
    try {
      created = await api().createCheckoutGroup(dialog.projectId, name);
    } catch (reason) {
      if (openSession() === dialog.session)
        ticketGroupDialog.value = { ...dialog, name: typed, busy: false, error: failureMessage(reason) };
      return;
    }
    if (openSession() === dialog.session) ticketGroupDialog.value = undefined;
    await assignCreatedGroup(dialog.projectId, created, dialog.slugs);
  }
  /** List a just-created group at once, so its members show its name, then move the tickets into it. */
  async function assignCreatedGroup(
    projectId: string,
    created: { id: string; name: string },
    slugs: string[],
  ): Promise<boolean> {
    const listed = projectTicketGroups.value[projectId] ?? [];
    if (!listed.some((group) => group.id === created.id))
      storeSummaries(projectId, [
        ...listed,
        { id: created.id, name: created.name, count: 0, named: true, legacy: false, duplicate: false },
      ]);
    const moved = await setGroup(slugs, created.id);
    await refreshGroups();
    return moved;
  }
  /**
   * Create a group by name in the project's registry and move the tickets into it, without the
   * dialog: Add to Group › Automatic uses it for a name the AI proposed (HS2-4C32A0). A failed
   * create rejects and changes no ticket.
   */
  async function createAndAssignGroup(name: string, slugs: string[]): Promise<{ id: string; name: string }> {
    const current = project();
    if (!current) throw new Error('No project is open.');
    const created = await api().createCheckoutGroup(current.id, name);
    await assignCreatedGroup(current.id, created, slugs);
    return created;
  }
  async function renameGroup(
    dialog: NonNullable<typeof ticketGroupDialog.value>,
    current: Project,
    name: string,
    typed: string,
  ): Promise<void> {
    const group = dialog.group!;
    if (groupFor(group)?.named !== false && name === displayName(group)) {
      ticketGroupDialog.value = undefined;
      return;
    }
    ticketGroupDialog.value = { ...dialog, name: typed, busy: true, error: undefined };
    try {
      const result = await api().renameCheckoutGroup(current.id, group, name);
      if (openSession() === dialog.session) ticketGroupDialog.value = undefined;
      // A legacy group's members move to a new id; keep its collapse choice under that id.
      const renamedTo = result.id ?? group;
      if (
        ticketGroupKey(renamedTo) !== ticketGroupKey(group) &&
        ticketGroupCollapsed(current.id, view(), group) &&
        !ticketGroupCollapsed(current.id, view(), renamedTo)
      ) {
        toggleTicketGroupCollapsed(current.id, view(), group);
        toggleTicketGroupCollapsed(current.id, view(), renamedTo);
      }
      if (result.updated > 0 || !isTicketGroupId(group)) await refreshProject({ showLoading: false });
      await refreshGroups();
    } catch (reason) {
      if (openSession() === dialog.session)
        ticketGroupDialog.value = { ...dialog, name: typed, busy: false, error: failureMessage(reason) };
    }
  }
  async function submitDialog(typed: string): Promise<void> {
    const dialog = ticketGroupDialog.value,
      current = project();
    if (!dialog || dialog.busy || current?.id !== dialog.projectId) return;
    const name = normalizeTicketGroupName(typed);
    if (ticketGroupNameProblem(name, dialog.groups)) {
      ticketGroupDialog.value = { ...dialog, name: typed };
      return;
    }
    if (dialog.mode === 'new') await createGroup(dialog, name, typed);
    else await renameGroup(dialog, current, name, typed);
  }
  async function ungroup(group: string): Promise<void> {
    const current = project();
    if (!current) return;
    const reason = ticketGroupDisabledReason(membersOf(group), capabilitiesFor);
    if (reason) {
      showToast(reason);
      return;
    }
    const name = displayName(group);
    try {
      const result = await api().ungroupCheckoutGroup(current.id, group);
      await refreshProject({ showLoading: false });
      await refreshGroups();
      showToast(`Ungrouped ${result.updated} ticket${result.updated === 1 ? '' : 's'} from “${name}”.`);
    } catch (failure) {
      showToast(`Ungroup failed: ${failureMessage(failure)}`);
    }
  }
  /** The other listed groups whose display name has this flagged duplicate's name key (HS2-9KZSD4). */
  function duplicatesOf(group: string): KnownTicketGroup[] {
    const own = groupFor(group);
    if (!own?.duplicate) return [];
    const key = ticketGroupNameKey(own.name);
    return groups().filter(
      (other) =>
        ticketGroupKey(other.id) !== ticketGroupKey(group) &&
        other.legacy !== true &&
        ticketGroupNameKey(other.name) === key,
    );
  }
  /** Whether the group header menu offers Merge Duplicate Groups for this group. */
  const hasDuplicates = (group: string) => duplicatesOf(group).length > 0;
  /** Move every member of each same-named group into this one (HS2-9KZSD4). */
  async function mergeDuplicates(group: string): Promise<void> {
    const current = project(),
      others = duplicatesOf(group);
    if (!current || others.length === 0) return;
    const reason = ticketGroupDisabledReason(
      [group, ...others.map((other) => other.id)].flatMap((value) => membersOf(value)),
      capabilitiesFor,
    );
    if (reason) {
      showToast(reason);
      return;
    }
    const name = displayName(group);
    let merged = 0;
    try {
      for (const other of others) {
        await api().mergeCheckoutGroups(current.id, other.id, group);
        merged += 1;
      }
      showToast(`Merged ${merged} group${merged === 1 ? '' : 's'} into “${name}”.`);
    } catch (failure) {
      showToast(`Merge failed: ${failureMessage(failure)}`);
    }
    await refreshProject({ showLoading: false });
    await refreshGroups();
  }
  return {
    groups,
    groupNames,
    displayName,
    refreshGroups,
    syncGroups,
    collectionProps,
    toggleGroup,
    isCollapsed,
    navigable,
    disabledReason,
    setGroup,
    createAndAssignGroup,
    openNewGroup,
    openRenameGroup,
    editDialogName,
    closeDialog,
    submitDialog,
    ungroup,
    hasDuplicates,
    mergeDuplicates,
  };
}
