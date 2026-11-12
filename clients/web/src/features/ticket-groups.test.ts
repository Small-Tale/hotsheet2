import { signal } from 'kerfjs';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Api, Capabilities, TicketRow } from '../api';
import type { Project } from '../interactions/types';
import {
  collapsedTicketGroups,
  projectTicketGroups,
  ticketGroupCollapsed,
  ticketGroupDialog,
  ticketGroupDropTarget,
} from '../ticket-group-state';
import { createTicketGroupsController } from './ticket-groups';

const UNLOADED = '9C5P1T4VZC',
  CREATED = '1A2B3C4D5E',
  RENAMED = '7K3M9P2QXA',
  SPRINT_A = '2F3G4H5J6K',
  SPRINT_B = '3M4N5P6Q7R',
  SPRINT_C = '4S5T6V7W8X';
const summary = (
  id: string,
  name: string,
  flags: { count?: number; legacy?: boolean; duplicate?: boolean; named?: boolean } = {},
) => ({
  id,
  name,
  count: flags.count ?? 1,
  named: flags.named ?? true,
  legacy: flags.legacy ?? false,
  duplicate: flags.duplicate ?? false,
});
const row = (slug: string, group?: string, connection_id = 'git'): TicketRow =>
  ({
    slug,
    id: slug,
    qualified_id: `${connection_id}:${slug}`,
    connection_id,
    group,
    tags: [],
  }) as unknown as TicketRow;

function setup(
  rows: TicketRow[] = [row('HS2-A', 'Board'), row('HS2-B', 'board'), row('HS2-C'), row('GH-1', undefined, 'github')],
) {
  const tickets = signal(rows),
    selectedView = signal('all'),
    executeBulkTicketAction = vi.fn(() => Promise.resolve(true)),
    refreshProject = vi.fn(() => Promise.resolve()),
    showToast = vi.fn(),
    api = {
      checkoutGroups: vi.fn(() =>
        Promise.resolve({ registry: true, groups: [summary(UNLOADED, 'Unloaded', { count: 4 })] }),
      ),
      createCheckoutGroup: vi.fn((_checkout: string, name: string) =>
        Promise.resolve({ id: CREATED, name, created: true }),
      ),
      renameCheckoutGroup: vi.fn(() => Promise.resolve({ id: RENAMED, name: 'Renamed', updated: 2 })),
      ungroupCheckoutGroup: vi.fn(() => Promise.resolve({ updated: 2 })),
      mergeCheckoutGroups: vi.fn(() => Promise.resolve({ id: SPRINT_A, name: 'Sprint', updated: 1 })),
    };
  let current: Project | undefined = { id: 'p', apiPath: '/api' } as Project;
  const controller = createTicketGroupsController({
    project: () => current,
    api: () => api as unknown as Api,
    tickets,
    selectedView,
    capabilitiesFor: (id) => ({ update: true, groups: id === 'git' }) as Capabilities,
    executeBulkTicketAction,
    refreshProject,
    showToast,
  });
  return {
    controller,
    tickets,
    selectedView,
    executeBulkTicketAction,
    refreshProject,
    showToast,
    api,
    setProject: (value: Project | undefined) => {
      current = value;
    },
  };
}
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('ticket groups controller (HS2-3XNTSD)', () => {
  beforeEach(() => {
    collapsedTicketGroups.value = new Set();
    projectTicketGroups.value = {};
    ticketGroupDialog.value = undefined;
    ticketGroupDropTarget.value = undefined;
    vi.stubGlobal('document', { querySelector: () => null });
  });

  it('lists loaded groups plus server-reported ones, without duplicates', async () => {
    const { controller, api } = setup();
    expect(controller.groupNames()).toEqual(['Board']);
    await controller.refreshGroups();
    expect(api.checkoutGroups).toHaveBeenCalledWith('p');
    expect(controller.groupNames()).toEqual(['Board', 'Unloaded']);
    projectTicketGroups.value = { p: [summary('BOARD', 'BOARD', { legacy: true }), summary(RENAMED, 'Zeta')] };
    expect(controller.groupNames()).toEqual(['BOARD', 'Zeta']);
    expect(controller.groups()).toMatchObject([{ id: 'BOARD', legacy: true }, { id: RENAMED }]);
  });

  it('keeps the last list when the server cannot report groups', async () => {
    const { controller, api } = setup();
    projectTicketGroups.value = { p: [summary(RENAMED, 'Kept')] };
    api.checkoutGroups.mockRejectedValueOnce(new Error('404'));
    await controller.refreshGroups();
    expect(controller.groupNames()).toEqual(['Board', 'Kept']);
  });

  it('sets a group through the bulk path, reusing an existing group’s stored value', async () => {
    const { controller, executeBulkTicketAction } = setup();
    await controller.setGroup(['HS2-C'], '  BOARD ');
    expect(executeBulkTicketAction).toHaveBeenLastCalledWith({ kind: 'group', group: 'Board', status: undefined }, [
      'HS2-C',
    ]);
    await controller.setGroup(['HS2-C'], ' New   name ', 'started');
    expect(executeBulkTicketAction).toHaveBeenLastCalledWith({ kind: 'group', group: 'New name', status: 'started' }, [
      'HS2-C',
    ]);
    await controller.setGroup(['HS2-A'], null);
    expect(executeBulkTicketAction).toHaveBeenLastCalledWith({ kind: 'group', group: null, status: undefined }, [
      'HS2-A',
    ]);
    // An id, or the one group a typed display name names, is stored as the id (HS2-9KZSD4).
    projectTicketGroups.value = { p: [summary(UNLOADED, 'Unloaded')] };
    await controller.setGroup(['HS2-C'], UNLOADED);
    expect(executeBulkTicketAction).toHaveBeenLastCalledWith({ kind: 'group', group: UNLOADED, status: undefined }, [
      'HS2-C',
    ]);
    await controller.setGroup(['HS2-C'], 'unloaded');
    expect(executeBulkTicketAction).toHaveBeenLastCalledWith({ kind: 'group', group: UNLOADED, status: undefined }, [
      'HS2-C',
    ]);
  });

  it('refuses a provider without group support and says why', async () => {
    const { controller, executeBulkTicketAction, showToast } = setup();
    expect(controller.disabledReason(['HS2-A'])).toBeUndefined();
    expect(controller.disabledReason(['HS2-A', 'GH-1'])).toContain('do not support groups');
    await expect(controller.setGroup(['GH-1'], 'Board')).resolves.toBe(false);
    expect(executeBulkTicketAction).not.toHaveBeenCalled();
    expect(showToast).toHaveBeenCalledWith(expect.stringContaining('do not support groups'));
    controller.openNewGroup(['GH-1']);
    expect(ticketGroupDialog.value).toBeUndefined();
  });

  it('opens New Group, validates live, creates the group, then assigns its id to every target', async () => {
    const { controller, executeBulkTicketAction, api } = setup();
    let shownName: string | undefined;
    executeBulkTicketAction.mockImplementation(() => {
      shownName = controller.displayName(CREATED);
      return Promise.resolve(true);
    });
    controller.openNewGroup([]);
    expect(ticketGroupDialog.value).toBeUndefined();
    controller.openNewGroup(['HS2-C', 'HS2-A']);
    expect(ticketGroupDialog.value).toMatchObject({
      mode: 'new',
      name: '',
      groups: ['Board'],
      slugs: ['HS2-C', 'HS2-A'],
    });
    await settle();
    // The server's list arrives while the dialog is open and extends the duplicate check.
    expect(ticketGroupDialog.value?.groups).toEqual(['Board', 'Unloaded']);
    controller.editDialogName('unloaded');
    expect(ticketGroupDialog.value?.name).toBe('unloaded');
    await controller.submitDialog('unloaded');
    expect(api.createCheckoutGroup).not.toHaveBeenCalled();
    expect(ticketGroupDialog.value).toBeDefined();
    await controller.submitDialog('   ');
    expect(ticketGroupDialog.value).toBeDefined();
    const pending = controller.submitDialog('  Search   syntax ');
    expect(ticketGroupDialog.value).toMatchObject({ busy: true });
    await pending;
    expect(ticketGroupDialog.value).toBeUndefined();
    expect(api.createCheckoutGroup).toHaveBeenCalledWith('p', 'Search syntax');
    expect(executeBulkTicketAction).toHaveBeenCalledWith({ kind: 'group', group: CREATED, status: undefined }, [
      'HS2-C',
      'HS2-A',
    ]);
    // The new group is named before its members move, and the summaries are read again afterward.
    expect(shownName).toBe('Search syntax');
    expect(api.checkoutGroups).toHaveBeenCalledTimes(2);
  });

  it('keeps New Group open with the server message when the group cannot be created', async () => {
    const { controller, executeBulkTicketAction, api } = setup();
    api.createCheckoutGroup.mockRejectedValueOnce(new Error('invalid_group_name'));
    controller.openNewGroup(['HS2-C']);
    await controller.submitDialog('Polish');
    expect(ticketGroupDialog.value).toMatchObject({ busy: false, error: 'invalid_group_name', name: 'Polish' });
    expect(executeBulkTicketAction).not.toHaveBeenCalled();
  });

  it('gives each open its own session and closes on cancel', () => {
    const { controller } = setup();
    controller.openNewGroup(['HS2-C']);
    const first = ticketGroupDialog.value!.session;
    controller.closeDialog();
    expect(ticketGroupDialog.value).toBeUndefined();
    controller.openRenameGroup('Board');
    expect(ticketGroupDialog.value).toMatchObject({ mode: 'rename', name: 'Board', group: 'Board', groups: [] });
    expect(ticketGroupDialog.value!.session).toBeGreaterThan(first);
    // Editing after close is ignored; closing twice is harmless.
    controller.closeDialog();
    controller.closeDialog();
    controller.editDialogName('late');
    expect(ticketGroupDialog.value).toBeUndefined();
  });

  it('renames a legacy group through the server, carries the collapse choice to its new id, and refreshes', async () => {
    const { controller, api, refreshProject } = setup();
    controller.toggleGroup('Board');
    controller.openRenameGroup('Board');
    await controller.submitDialog('Board');
    expect(api.renameCheckoutGroup).not.toHaveBeenCalled();
    expect(ticketGroupDialog.value).toBeUndefined();
    controller.openRenameGroup('Board');
    // A case-only change of the same group is a real rename.
    await controller.submitDialog('BOARD polish');
    expect(api.renameCheckoutGroup).toHaveBeenCalledWith('p', 'Board', 'BOARD polish');
    expect(refreshProject).toHaveBeenCalledWith({ showLoading: false });
    expect(ticketGroupDialog.value).toBeUndefined();
    expect(ticketGroupCollapsed('p', 'all', 'Board')).toBe(false);
    expect(ticketGroupCollapsed('p', 'all', RENAMED)).toBe(true);
  });

  it('renames an id group as a registry edit, keeping its collapse key (HS2-9KZSD4)', async () => {
    const { controller, api, refreshProject } = setup([row('HS2-A', SPRINT_A), row('HS2-B', SPRINT_A)]);
    projectTicketGroups.value = { p: [summary(SPRINT_A, 'Sprint', { count: 2 }), summary(RENAMED, 'Zeta')] };
    api.renameCheckoutGroup.mockResolvedValueOnce({ id: SPRINT_A, name: 'Sprint 2', updated: 0 });
    controller.toggleGroup(SPRINT_A);
    controller.openRenameGroup(SPRINT_A);
    expect(ticketGroupDialog.value).toMatchObject({ name: 'Sprint', group: SPRINT_A, groups: ['Zeta'] });
    // Another group's name is taken; the group's own name in another casing is not.
    await controller.submitDialog('zeta');
    expect(api.renameCheckoutGroup).not.toHaveBeenCalled();
    await controller.submitDialog('Sprint 2');
    expect(api.renameCheckoutGroup).toHaveBeenCalledWith('p', SPRINT_A, 'Sprint 2');
    expect(refreshProject).not.toHaveBeenCalled();
    expect(api.checkoutGroups).toHaveBeenCalled();
    expect(ticketGroupCollapsed('p', 'all', SPRINT_A)).toBe(true);
  });

  it('renames a placeholder group even under its placeholder name (HS2-9KZSD4)', async () => {
    const { controller, api } = setup([row('HS2-A', SPRINT_A)]);
    projectTicketGroups.value = { p: [summary(SPRINT_A, `Group ${SPRINT_A}`, { named: false })] };
    controller.openRenameGroup(SPRINT_A);
    expect(ticketGroupDialog.value?.name).toBe(`Group ${SPRINT_A}`);
    await controller.submitDialog(`Group ${SPRINT_A}`);
    expect(api.renameCheckoutGroup).toHaveBeenCalledWith('p', SPRINT_A, `Group ${SPRINT_A}`);
  });

  it('keeps the rename dialog open with the server message on failure, then recovers', async () => {
    const { controller, api, refreshProject } = setup();
    api.renameCheckoutGroup.mockRejectedValueOnce(new Error('A group named “Zeta” already exists.'));
    controller.openRenameGroup('Board');
    const pending = controller.submitDialog('Zeta');
    expect(ticketGroupDialog.value).toMatchObject({ busy: true, name: 'Zeta' });
    // A busy dialog ignores cancel and a second submit.
    controller.closeDialog();
    await controller.submitDialog('Other');
    expect(api.renameCheckoutGroup).toHaveBeenCalledTimes(1);
    await pending;
    expect(ticketGroupDialog.value).toMatchObject({ busy: false, error: 'A group named “Zeta” already exists.' });
    expect(refreshProject).not.toHaveBeenCalled();
    controller.editDialogName('Zeta 2');
    expect(ticketGroupDialog.value?.error).toBeUndefined();
    await controller.submitDialog('Zeta 2');
    expect(ticketGroupDialog.value).toBeUndefined();
    expect(refreshProject).toHaveBeenCalledTimes(1);
  });

  it('ungroups through the server by stored value and reports the result or the failure', async () => {
    const { controller, api, refreshProject, showToast, tickets } = setup();
    await controller.ungroup('Board');
    expect(api.ungroupCheckoutGroup).toHaveBeenCalledWith('p', 'Board');
    expect(refreshProject).toHaveBeenCalledTimes(1);
    expect(showToast).toHaveBeenLastCalledWith('Ungrouped 2 tickets from “Board”.');
    api.ungroupCheckoutGroup.mockRejectedValueOnce(new Error('offline'));
    await controller.ungroup('Board');
    expect(showToast).toHaveBeenLastCalledWith('Ungroup failed: offline');
    expect(refreshProject).toHaveBeenCalledTimes(1);
    projectTicketGroups.value = { p: [summary(SPRINT_A, 'Sprint')] };
    tickets.value = [...tickets.value, row('HS2-S', SPRINT_A)];
    await controller.ungroup(SPRINT_A);
    expect(api.ungroupCheckoutGroup).toHaveBeenLastCalledWith('p', SPRINT_A);
    expect(showToast).toHaveBeenLastCalledWith('Ungrouped 2 tickets from “Sprint”.');
  });

  it('merges every other same-named group into a flagged duplicate only (HS2-9KZSD4)', async () => {
    const { controller, api, refreshProject, showToast } = setup([
      row('HS2-A', SPRINT_A),
      row('HS2-B', SPRINT_B),
      row('HS2-C', SPRINT_C),
      row('HS2-D', RENAMED),
    ]);
    const listed = [
      summary(SPRINT_A, 'Sprint', { duplicate: true }),
      summary(SPRINT_B, 'sprint ', { duplicate: true }),
      summary(SPRINT_C, 'SPRINT', { duplicate: true }),
      summary(RENAMED, 'Zeta'),
    ];
    projectTicketGroups.value = { p: listed };
    expect(controller.hasDuplicates(RENAMED)).toBe(false);
    expect(controller.hasDuplicates(SPRINT_B)).toBe(true);
    await controller.mergeDuplicates(RENAMED);
    expect(api.mergeCheckoutGroups).not.toHaveBeenCalled();
    await controller.mergeDuplicates(SPRINT_B);
    expect(api.mergeCheckoutGroups.mock.calls).toEqual([
      ['p', SPRINT_A, SPRINT_B],
      ['p', SPRINT_C, SPRINT_B],
    ]);
    expect(showToast).toHaveBeenLastCalledWith('Merged 2 groups into “sprint ”.');
    expect(refreshProject).toHaveBeenCalledWith({ showLoading: false });
    projectTicketGroups.value = { p: listed };
    api.mergeCheckoutGroups.mockRejectedValueOnce(new Error('invalid_group_merge'));
    await controller.mergeDuplicates(SPRINT_A);
    expect(showToast).toHaveBeenLastCalledWith('Merge failed: invalid_group_merge');
  });

  it('renders two ids with one name as two groups with separate collapse state (HS2-9KZSD4)', () => {
    const { controller } = setup([row('HS2-A', SPRINT_A), row('HS2-B', SPRINT_B)]);
    projectTicketGroups.value = {
      p: [summary(SPRINT_A, 'Sprint', { duplicate: true }), summary(SPRINT_B, 'Sprint', { duplicate: true })],
    };
    expect(controller.groups().map((group) => group.id)).toEqual([SPRINT_A, SPRINT_B]);
    controller.toggleGroup(SPRINT_A);
    expect(controller.collectionProps('list').collapsedGroups).toEqual([SPRINT_A]);
    expect(controller.isCollapsed(SPRINT_B)).toBe(false);
  });

  it('loads summaries per project and again only for a new set of unknown ids (HS2-9KZSD4)', async () => {
    const { controller, api, tickets, setProject } = setup([row('HS2-A', SPRINT_A)]);
    api.checkoutGroups.mockResolvedValue({ registry: true, groups: [summary(SPRINT_A, 'Sprint')] });
    controller.syncGroups();
    await settle();
    expect(api.checkoutGroups).toHaveBeenCalledTimes(1);
    expect(controller.displayName(SPRINT_A)).toBe('Sprint');
    // Known ids, legacy names, and repeated syncs ask nothing more.
    tickets.value = [...tickets.value, row('HS2-L', 'Legacy')];
    controller.syncGroups();
    controller.syncGroups();
    expect(api.checkoutGroups).toHaveBeenCalledTimes(1);
    // An unknown id asks once; the same missing set does not ask again.
    tickets.value = [...tickets.value, row('HS2-B', SPRINT_B)];
    controller.syncGroups();
    await settle();
    controller.syncGroups();
    await settle();
    expect(api.checkoutGroups).toHaveBeenCalledTimes(2);
    expect(controller.displayName(SPRINT_B)).toBe(`Group ${SPRINT_B}`);
    // A different missing set asks again, and so does another project.
    tickets.value = [...tickets.value, row('HS2-C', SPRINT_C)];
    controller.syncGroups();
    expect(api.checkoutGroups).toHaveBeenCalledTimes(3);
    setProject({ id: 'q', apiPath: '/api' } as Project);
    controller.syncGroups();
    expect(api.checkoutGroups).toHaveBeenLastCalledWith('q');
    setProject(undefined);
    controller.syncGroups();
    expect(api.checkoutGroups).toHaveBeenCalledTimes(4);
  });

  it('projects collapse and drop state per collection scope and view', () => {
    const { controller, selectedView, tickets } = setup();
    controller.toggleGroup('board');
    ticketGroupDropTarget.value = { scope: 'started', key: 'board' };
    expect(controller.collectionProps('list')).toEqual({ collapsedGroups: ['board'], dropGroup: undefined });
    expect(controller.collectionProps('started')).toEqual({ collapsedGroups: ['board'], dropGroup: 'board' });
    expect(controller.isCollapsed('Board')).toBe(true);
    expect(controller.navigable(tickets.value).map((ticket) => ticket.slug)).toEqual(['HS2-C', 'GH-1']);
    // Another view keeps its own preference; returning restores it.
    selectedView.value = 'backlog';
    expect(controller.collectionProps('list').collapsedGroups).toEqual([]);
    expect(controller.navigable(tickets.value)).toHaveLength(4);
    selectedView.value = 'all';
    controller.toggleGroup('Board');
    expect(controller.collectionProps('list').collapsedGroups).toEqual([]);
  });

  it('does nothing without an open project', async () => {
    const { controller, setProject, api } = setup();
    setProject(undefined);
    controller.toggleGroup('Board');
    controller.openNewGroup(['HS2-C']);
    controller.openRenameGroup('Board');
    await controller.refreshGroups();
    await controller.ungroup('Board');
    await controller.mergeDuplicates('Board');
    expect(collapsedTicketGroups.value.size).toBe(0);
    expect(ticketGroupDialog.value).toBeUndefined();
    expect(api.checkoutGroups).not.toHaveBeenCalled();
    expect(api.ungroupCheckoutGroup).not.toHaveBeenCalled();
    expect(controller.collectionProps('list')).toEqual({ collapsedGroups: [], dropGroup: undefined });
  });
});
