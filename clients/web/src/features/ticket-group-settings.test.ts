import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Api } from '../api';
import type { Project } from '../interactions/types';
import type { CheckoutGroupSummary } from '../server-response-parsers';
import { ticketGroupMergeDialog, ticketGroupOperation, ticketGroupSettingsByProject } from '../ticket-group-state';
import { createTicketGroupSettingsController } from './ticket-group-settings';

const group = (id: string, name: string, count: number, extra: Partial<CheckoutGroupSummary> = {}) => ({
  id,
  name,
  count,
  named: true,
  legacy: false,
  duplicate: false,
  ...extra,
});
const launch = group('01M7P6A000', 'Launch readiness', 18),
  design = group('01M7P6B000', 'Design system', 7),
  future = group('01M7P6C000', 'Future work', 0),
  legacy = group('Old name', 'Old name', 1, { legacy: true });

function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function setup(list: CheckoutGroupSummary[] = [launch, design, future, legacy]) {
  const current = { id: 'project', name: 'Project' } as Project;
  const api = {
    allCheckoutGroups: vi.fn(() => Promise.resolve({ registry: true, groups: list })),
    mergeCheckoutGroups: vi.fn(() => Promise.resolve({ id: design.id, name: design.name, updated: 18 })),
    deleteCheckoutGroup: vi.fn(() => Promise.resolve({ name: launch.name, updated: 18 })),
  };
  const dependencies = {
    project: () => current,
    api: () => api as unknown as Api,
    confirm: vi.fn(() => Promise.resolve(true)),
    refreshProject: vi.fn(() => Promise.resolve()),
    refreshTicketGroups: vi.fn(() => Promise.resolve()),
    openNewGroup: vi.fn(),
    openRenameGroup: vi.fn(),
    viewGroupTickets: vi.fn(),
    showToast: vi.fn(),
  };
  return { current, api, dependencies, controller: createTicketGroupSettingsController(dependencies) };
}

beforeEach(() => {
  ticketGroupSettingsByProject.value = {};
  ticketGroupMergeDialog.value = undefined;
  ticketGroupOperation.value = undefined;
});

describe('Ticket groups settings controller (HS2-GXKX1Q)', () => {
  it('reads every group, keeps only the newest answer, and keeps the list on a failed read', async () => {
    const { api, controller } = setup();
    const slow = deferred<{ registry: boolean; groups: CheckoutGroupSummary[] }>();
    api.allCheckoutGroups.mockReturnValueOnce(slow.promise);
    const first = controller.refresh();
    await controller.refresh();
    expect(ticketGroupSettingsByProject.value.project?.groups).toHaveLength(4);
    slow.resolve({ registry: true, groups: [future] });
    await first;
    expect(ticketGroupSettingsByProject.value.project?.groups).toHaveLength(4);
    api.allCheckoutGroups.mockRejectedValueOnce(new Error('The server is unreachable.'));
    await controller.refresh();
    expect(ticketGroupSettingsByProject.value.project).toMatchObject({
      error: 'The server is unreachable.',
      groups: expect.any(Array) as unknown,
    });
    // A successful read clears the message.
    await controller.refresh();
    expect(ticketGroupSettingsByProject.value.project?.error).toBeUndefined();
  });

  it('opens New group, Rename with the listed name, and View tickets for a listed group only', async () => {
    const { dependencies, controller } = setup();
    await controller.refresh();
    controller.newGroup();
    expect(dependencies.openNewGroup).toHaveBeenCalledTimes(1);
    controller.rename(future.id);
    expect(dependencies.openRenameGroup).toHaveBeenCalledWith(future.id, 'Future work');
    controller.viewTickets(design.id);
    expect(dependencies.viewGroupTickets).toHaveBeenCalledWith(design);
    controller.rename('missing');
    controller.viewTickets('missing');
    expect(dependencies.openRenameGroup).toHaveBeenCalledTimes(1);
    expect(dependencies.viewGroupTickets).toHaveBeenCalledTimes(1);
  });

  it('merges into a chosen group, removing the source, with the banner up while it runs', async () => {
    const { api, dependencies, controller } = setup();
    await controller.refresh();
    controller.openMerge(launch.id);
    // Every other non-legacy group is a destination; the first is chosen.
    expect(ticketGroupMergeDialog.value).toMatchObject({
      group: { id: launch.id, name: 'Launch readiness', count: 18 },
      destinations: [
        { id: design.id, name: 'Design system' },
        { id: future.id, name: 'Future work' },
      ],
      into: design.id,
    });
    controller.chooseMergeDestination('not listed');
    expect(ticketGroupMergeDialog.value?.into).toBe(design.id);
    controller.chooseMergeDestination(future.id);
    expect(ticketGroupMergeDialog.value?.into).toBe(future.id);
    controller.chooseMergeDestination(design.id);
    const merging = deferred<{ id: string; name: string; updated: number }>();
    api.mergeCheckoutGroups.mockReturnValueOnce(merging.promise);
    const submitted = controller.submitMerge();
    // The dialog closes at once; the banner and the busy row carry the merge.
    expect(ticketGroupMergeDialog.value).toBeUndefined();
    expect(ticketGroupOperation.value).toEqual({
      projectId: 'project',
      groupId: launch.id,
      title: 'Merging Launch readiness into Design system',
    });
    expect(api.mergeCheckoutGroups).toHaveBeenCalledWith('project', launch.id, design.id, true);
    // Another operation cannot start meanwhile.
    controller.openMerge(design.id);
    expect(ticketGroupMergeDialog.value).toBeUndefined();
    merging.resolve({ id: design.id, name: design.name, updated: 18 });
    await submitted;
    expect(ticketGroupOperation.value).toBeUndefined();
    expect(dependencies.showToast).toHaveBeenCalledWith(
      'Moved 18 tickets into “Design system” and removed “Launch readiness”.',
    );
    expect(dependencies.refreshProject).toHaveBeenCalled();
    expect(dependencies.refreshTicketGroups).toHaveBeenCalled();
  });

  it('reports a failed merge and still re-reads; Cancel closes the dialog', async () => {
    const { api, dependencies, controller } = setup();
    await controller.refresh();
    controller.openMerge(launch.id);
    controller.cancelMerge();
    expect(ticketGroupMergeDialog.value).toBeUndefined();
    controller.openMerge(launch.id);
    api.mergeCheckoutGroups.mockRejectedValueOnce(new Error('A source refused the write.'));
    const reads = api.allCheckoutGroups.mock.calls.length;
    await controller.submitMerge();
    expect(dependencies.showToast).toHaveBeenCalledWith(
      'Merging Launch readiness into Design system failed: A source refused the write.',
    );
    expect(ticketGroupOperation.value).toBeUndefined();
    expect(api.allCheckoutGroups.mock.calls.length).toBe(reads + 1);
    // A group with no other group has nothing to merge into.
    const single = setup([launch]);
    await single.controller.refresh();
    single.controller.openMerge(launch.id);
    expect(ticketGroupMergeDialog.value).toBeUndefined();
  });

  it('deletes only after confirmation, naming what happens to the tickets', async () => {
    const { api, dependencies, controller } = setup();
    await controller.refresh();
    dependencies.confirm.mockResolvedValueOnce(false);
    await controller.remove(launch.id);
    expect(dependencies.confirm).toHaveBeenCalledWith({
      title: 'Delete “Launch readiness”?',
      message: '18 tickets will stay in their sources without a group.',
      confirmLabel: 'Delete group',
      tone: 'danger',
    });
    expect(api.deleteCheckoutGroup).not.toHaveBeenCalled();
    await controller.remove(launch.id);
    expect(api.deleteCheckoutGroup).toHaveBeenCalledWith('project', launch.id);
    expect(dependencies.showToast).toHaveBeenCalledWith(
      'Deleted “Launch readiness”; 18 tickets no longer have a group.',
    );
    // An empty group says so, and its toast does too.
    api.deleteCheckoutGroup.mockResolvedValueOnce({ name: 'Future work', updated: 0 });
    await controller.remove(future.id);
    expect(dependencies.confirm).toHaveBeenLastCalledWith(
      expect.objectContaining({ message: 'No tickets are in this group.' }),
    );
    expect(dependencies.showToast).toHaveBeenLastCalledWith('Deleted “Future work”.');
    api.deleteCheckoutGroup.mockRejectedValueOnce(new Error('Gone.'));
    await controller.remove(design.id);
    expect(dependencies.showToast).toHaveBeenLastCalledWith('Deleting Design system failed: Gone.');
  });
});
