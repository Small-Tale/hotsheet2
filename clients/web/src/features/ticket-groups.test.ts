import { signal } from 'kerfjs';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Api, Capabilities, TicketRow } from '../api';
import type { Project } from '../interactions/types';
import {
  collapsedTicketGroups,
  reportedTicketGroups,
  ticketGroupCollapsed,
  ticketGroupDialog,
  ticketGroupDropTarget,
} from '../ticket-group-state';
import { createTicketGroupsController } from './ticket-groups';

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
      checkoutGroups: vi.fn(() => Promise.resolve({ groups: [{ name: 'Unloaded', count: 4 }] })),
      renameCheckoutGroup: vi.fn(() => Promise.resolve({ name: 'Renamed', updated: 2 })),
      ungroupCheckoutGroup: vi.fn(() => Promise.resolve({ updated: 2 })),
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
    reportedTicketGroups.value = {};
    ticketGroupDialog.value = undefined;
    ticketGroupDropTarget.value = undefined;
    vi.stubGlobal('document', { querySelector: () => null });
  });

  it('lists loaded groups plus server-reported ones, without duplicates', async () => {
    const { controller, api } = setup();
    expect(controller.groupNames()).toEqual(['Board']);
    controller.refreshGroupNames();
    await settle();
    expect(api.checkoutGroups).toHaveBeenCalledWith('p');
    expect(controller.groupNames()).toEqual(['Board', 'Unloaded']);
    reportedTicketGroups.value = { p: ['BOARD', 'Zeta'] };
    expect(controller.groupNames()).toEqual(['Board', 'Zeta']);
  });

  it('keeps the last list when the server cannot report groups', async () => {
    const { controller, api } = setup();
    reportedTicketGroups.value = { p: ['Kept'] };
    api.checkoutGroups.mockRejectedValueOnce(new Error('404'));
    controller.refreshGroupNames();
    await settle();
    expect(controller.groupNames()).toEqual(['Board', 'Kept']);
  });

  it('sets a group through the bulk path, reusing an existing spelling', async () => {
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

  it('opens New Group, validates live, and assigns every target on submit', async () => {
    const { controller, executeBulkTicketAction } = setup();
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
    expect(executeBulkTicketAction).not.toHaveBeenCalled();
    expect(ticketGroupDialog.value).toBeDefined();
    await controller.submitDialog('   ');
    expect(ticketGroupDialog.value).toBeDefined();
    await controller.submitDialog('  Search   syntax ');
    expect(ticketGroupDialog.value).toBeUndefined();
    expect(executeBulkTicketAction).toHaveBeenCalledWith({ kind: 'group', group: 'Search syntax', status: undefined }, [
      'HS2-C',
      'HS2-A',
    ]);
  });

  it('gives each open its own session and closes on cancel', () => {
    const { controller } = setup();
    controller.openNewGroup(['HS2-C']);
    const first = ticketGroupDialog.value!.session;
    controller.closeDialog();
    expect(ticketGroupDialog.value).toBeUndefined();
    controller.openRenameGroup('Board');
    expect(ticketGroupDialog.value).toMatchObject({ mode: 'rename', name: 'Board', group: 'Board' });
    expect(ticketGroupDialog.value!.session).toBeGreaterThan(first);
    // Editing after close is ignored; closing twice is harmless.
    controller.closeDialog();
    controller.closeDialog();
    controller.editDialogName('late');
    expect(ticketGroupDialog.value).toBeUndefined();
  });

  it('renames through the server, carries the collapse choice, and refreshes', async () => {
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
    expect(ticketGroupCollapsed('p', 'all', 'board polish')).toBe(true);
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

  it('ungroups through the server and reports the result or the failure', async () => {
    const { controller, api, refreshProject, showToast } = setup();
    await controller.ungroup('Board');
    expect(api.ungroupCheckoutGroup).toHaveBeenCalledWith('p', 'Board');
    expect(refreshProject).toHaveBeenCalledTimes(1);
    expect(showToast).toHaveBeenLastCalledWith('Ungrouped 2 tickets from “Board”.');
    api.ungroupCheckoutGroup.mockRejectedValueOnce(new Error('offline'));
    await controller.ungroup('Board');
    expect(showToast).toHaveBeenLastCalledWith('Ungroup failed: offline');
    expect(refreshProject).toHaveBeenCalledTimes(1);
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
    controller.refreshGroupNames();
    await controller.ungroup('Board');
    expect(collapsedTicketGroups.value.size).toBe(0);
    expect(ticketGroupDialog.value).toBeUndefined();
    expect(api.checkoutGroups).not.toHaveBeenCalled();
    expect(api.ungroupCheckoutGroup).not.toHaveBeenCalled();
    expect(controller.collectionProps('list')).toEqual({ collapsedGroups: [], dropGroup: undefined });
  });
});
