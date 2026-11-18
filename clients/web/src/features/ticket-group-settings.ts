import type { Api } from '../api';
import type { ConfirmationOptions } from '../confirmation';
import type { Project } from '../interactions/types';
import type { CheckoutGroupSummary } from '../server-response-parsers';
import {
  ticketGroupMergeDialog,
  ticketGroupOperation,
  ticketGroupSettingsByProject,
  type TicketGroupSettingsState,
} from '../ticket-group-state';

/** Live application bindings the Ticket groups settings page reads (HS2-GXKX1Q). */
export interface TicketGroupSettingsDependencies {
  project: () => Project | undefined;
  api: () => Api;
  confirm: (options: ConfirmationOptions) => Promise<boolean>;
  /** Re-read the project's tickets after members changed group. */
  refreshProject: () => Promise<unknown>;
  /** Re-read the ticket views' group summaries after a registry change. */
  refreshTicketGroups: () => Promise<void>;
  /** Open the shared New Group dialog with no tickets, or Rename Group for one group. */
  openNewGroup: () => void;
  openRenameGroup: (group: string, name: string) => void;
  /** Show the project's tickets in a group (View tickets). */
  viewGroupTickets: (group: CheckoutGroupSummary) => void;
  showToast: (message: string) => void;
}

export type TicketGroupSettingsController = ReturnType<typeof createTicketGroupSettingsController>;

const failureMessage = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));
const tickets = (count: number) => `${count} ticket${count === 1 ? '' : 's'}`;

/**
 * Project Settings › Ticket groups (HS2-GXKX1Q): the project's whole group list, and Rename, Merge
 * into…, and Delete group… over it. Merge and delete rewrite every member on the server, in one
 * write per source; while one runs, a non-blocking banner says so and its group's row is busy.
 * Each read keeps only the newest answer for a project, so a slow earlier read cannot replace it.
 */
export function createTicketGroupSettingsController(dependencies: TicketGroupSettingsDependencies) {
  const { project, api, confirm, refreshProject, refreshTicketGroups, showToast } = dependencies;
  const latestRead = new Map<string, number>();
  let session = 0;

  function store(projectId: string, next: TicketGroupSettingsState): void {
    ticketGroupSettingsByProject.value = { ...ticketGroupSettingsByProject.value, [projectId]: next };
  }
  function listed(projectId: string): readonly CheckoutGroupSummary[] {
    return ticketGroupSettingsByProject.value[projectId]?.groups ?? [];
  }
  const find = (projectId: string, id: string) => listed(projectId).find((group) => group.id === id);

  /** Read every group of the shown project; a failed read keeps the last list with its message. */
  function refresh(current = project()): Promise<void> {
    if (!current) return Promise.resolve();
    const read = (latestRead.get(current.id) ?? 0) + 1;
    latestRead.set(current.id, read);
    return api()
      .allCheckoutGroups(current.id)
      .then((result) => {
        if (latestRead.get(current.id) === read) store(current.id, { groups: result.groups });
      })
      .catch((reason: unknown) => {
        if (latestRead.get(current.id) !== read) return;
        store(current.id, { ...ticketGroupSettingsByProject.value[current.id], error: failureMessage(reason) });
      });
  }

  /** Run one member-rewriting operation with the banner up, then re-read everything it changed. */
  async function run(current: Project, groupId: string, title: string, operation: () => Promise<string>) {
    if (ticketGroupOperation.value) return false;
    ticketGroupOperation.value = { projectId: current.id, groupId, title };
    try {
      showToast(await operation());
      return true;
    } catch (reason) {
      showToast(`${title} failed: ${failureMessage(reason)}`);
      return false;
    } finally {
      ticketGroupOperation.value = undefined;
      await Promise.all([refresh(current), refreshTicketGroups(), refreshProject()]);
    }
  }

  function viewTickets(id: string): void {
    const current = project(),
      group = current && find(current.id, id);
    if (group) dependencies.viewGroupTickets(group);
  }

  function rename(id: string): void {
    const current = project(),
      group = current && find(current.id, id);
    if (group) dependencies.openRenameGroup(group.id, group.name);
  }

  function openMerge(id: string): void {
    const current = project(),
      group = current && find(current.id, id);
    if (!current || !group || ticketGroupOperation.value) return;
    const destinations = listed(current.id)
      .filter((other) => other.id !== group.id && !other.legacy)
      .map(({ id: value, name, duplicate }) => ({ id: value, name, duplicate }));
    if (destinations.length === 0) return;
    ticketGroupMergeDialog.value = {
      projectId: current.id,
      group: { id: group.id, name: group.name, count: group.count },
      destinations,
      into: destinations[0].id,
      session: ++session,
    };
  }
  function chooseMergeDestination(into: string): void {
    const dialog = ticketGroupMergeDialog.value;
    if (dialog && !dialog.busy && dialog.destinations.some((group) => group.id === into))
      ticketGroupMergeDialog.value = { ...dialog, into, error: undefined };
  }
  function cancelMerge(): void {
    if (!ticketGroupMergeDialog.value?.busy) ticketGroupMergeDialog.value = undefined;
  }
  async function submitMerge(): Promise<void> {
    const dialog = ticketGroupMergeDialog.value,
      current = project();
    if (!dialog || dialog.busy || current?.id !== dialog.projectId) return;
    const into = dialog.destinations.find((group) => group.id === dialog.into);
    if (!into) return;
    // The dialog closes at once; the banner carries the running merge, so the page stays usable.
    ticketGroupMergeDialog.value = undefined;
    await run(current, dialog.group.id, `Merging ${dialog.group.name} into ${into.name}`, async () => {
      const result = await api().mergeCheckoutGroups(current.id, dialog.group.id, into.id, true);
      return `Moved ${tickets(result.updated)} into “${into.name}” and removed “${dialog.group.name}”.`;
    });
  }

  async function remove(id: string): Promise<void> {
    const current = project(),
      group = current && find(current.id, id);
    if (!current || !group || ticketGroupOperation.value) return;
    const confirmed = await confirm({
      title: `Delete “${group.name}”?`,
      message:
        group.count > 0
          ? `${tickets(group.count)} will stay in their sources without a group.`
          : 'No tickets are in this group.',
      confirmLabel: 'Delete group',
      tone: 'danger',
    });
    if (!confirmed) return;
    await run(current, group.id, `Deleting ${group.name}`, async () => {
      const result = await api().deleteCheckoutGroup(current.id, group.id);
      return result.updated > 0
        ? `Deleted “${result.name}”; ${tickets(result.updated)} no longer have a group.`
        : `Deleted “${result.name}”.`;
    });
  }

  return {
    refresh,
    newGroup: () => {
      dependencies.openNewGroup();
    },
    viewTickets,
    rename,
    openMerge,
    chooseMergeDestination,
    cancelMerge,
    submitMerge,
    remove,
  };
}
