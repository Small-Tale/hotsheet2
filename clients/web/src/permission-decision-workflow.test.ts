import { signal } from 'kerfjs';
import { describe, expect, it, vi } from 'vitest';

import { ApiHttpError } from './api';
import type { Project } from './interactions/types';
import { createPermissionDecisionWorkflow } from './permission-decision-workflow';
import { PermissionInbox, VisiblePermissionTimer, type WirePermissionRequest } from './permission-notifications';

const project = (id: string): Project => ({
  id,
  name: id,
  root: `/work/${id}`,
  apiPath: `/api/${id}`,
  stores: [],
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
});
const request = (id: number, action = 'same'): WirePermissionRequest => ({
  id,
  project: 'a',
  connection: 'c',
  tool: 'Bash',
  action,
});
function setup() {
  const projects = signal([project('a'), project('b')]),
    inbox = new PermissionInbox(),
    timer = new VisiblePermissionTimer(),
    persist = vi.fn(),
    updateTimer = vi.fn(),
    clear = vi.fn(),
    load = vi.fn().mockResolvedValue({ requests: [], connections: [] }),
    send = vi.fn().mockResolvedValue(undefined);
  const workflow = createPermissionDecisionWorkflow({
    projects,
    inbox,
    timer,
    persistHistory: persist,
    updateTimer,
    clearDescription: clear,
    load,
    send,
  });
  return { projects, inbox, timer, persist, updateTimer, clear, load, send, workflow };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

describe('permission decision workflow (HS2-GGJ57M)', () => {
  it('forwards the reviewed matcher only with a remembered decision', async () => {
    const { projects, inbox, send, workflow } = setup();
    inbox.reconcile(projects.value[0], [request(1)], []);
    const pattern = { version: 1 as const, kind: 'template' as const, expression: 'npm run {{token}}' };
    await workflow.resolvePermission(inbox.pending()[0], 'allow', 'chat', false, true, undefined, pattern);
    expect(send).toHaveBeenCalledWith(
      projects.value[0],
      expect.objectContaining({ id: 1 }),
      'allow',
      'chat',
      undefined,
      pattern,
      false,
    );
  });
  it('groups remembered answers into one send and marks rules stale', async () => {
    const { projects, inbox, send, workflow } = setup();
    inbox.reconcile(projects.value[0], [request(1), request(2)], []);
    workflow.freshPermissionRules.value = new Set(['a', 'b']);
    await workflow.resolvePermission(inbox.pending()[0], 'allow', 'always', false, true, 'safe command');
    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith(
      projects.value[0],
      expect.objectContaining({ id: 1 }),
      'allow',
      'always',
      'safe command',
      undefined,
      false,
    );
    expect(inbox.pending()).toEqual([]);
    expect(workflow.freshPermissionRules.value.size).toBe(0);
    expect(inbox.history().map((item) => item.id)).toEqual([2, 1]);
  });

  it('forwards the automatic Allow Once review continuation flag (HS2-WZK2QK)', async () => {
    const { projects, inbox, send, workflow } = setup();
    inbox.reconcile(projects.value[0], [request(1)], []);
    await workflow.resolvePermission(inbox.pending()[0], 'allow', 'once', true, false, undefined, undefined, true);
    expect(send).toHaveBeenCalledWith(
      projects.value[0],
      expect.objectContaining({ id: 1 }),
      'allow',
      'once',
      undefined,
      undefined,
      true,
    );
    expect(inbox.pending()).toEqual([]);
  });

  it('sends once per grouped one-time answer and restores only a failed request', async () => {
    const { projects, inbox, send, workflow } = setup();
    inbox.reconcile(projects.value[0], [request(1), request(2)], []);
    send.mockImplementation(async (_project, item) => {
      if (item.id === 2) throw new Error('offline');
    });
    await workflow.resolvePermission(inbox.pending()[0], 'deny', 'once');
    expect(send).toHaveBeenCalledTimes(2);
    expect(inbox.pending().map((item) => item.id)).toEqual([2]);
    expect(workflow.permissionResolutionErrors.value['a:2']).toContain('offline');
    send.mockResolvedValue(undefined);
    await workflow.resolvePermission(inbox.pending()[0], 'deny', 'once');
    expect(inbox.pending()).toEqual([]);
    expect(workflow.permissionResolutionErrors.value['a:2']).toBeUndefined();
  });

  it('cancels a failed automatic answer and treats a missing request as externally resolved', async () => {
    const { projects, inbox, timer, send, workflow } = setup();
    inbox.reconcile(projects.value[0], [request(1)], []);
    send.mockRejectedValueOnce(new Error('disconnected'));
    await workflow.resolvePermission(inbox.pending()[0], 'allow', 'once', true);
    expect(timer.isCancelled('a:1')).toBe(true);
    expect(inbox.pending().map((item) => item.key)).toEqual(['a:1']);
    send.mockRejectedValueOnce(new ApiHttpError('Gone', 404));
    await workflow.resolvePermission(inbox.pending()[0], 'allow', 'once');
    expect(inbox.pending()).toEqual([]);
    expect(workflow.permissionResolutionErrors.value['a:1']).toBeUndefined();
  });

  it('reconciles empty, refill, project removal, and server resolution in sequence', async () => {
    const { projects, inbox, load, workflow, clear } = setup();
    load.mockResolvedValueOnce({ requests: [request(1)], connections: [] });
    await workflow.refreshPermissions('a');
    expect(inbox.pending().map((item) => item.key)).toEqual(['a:1']);
    load.mockResolvedValueOnce({ requests: [], connections: [] });
    await workflow.refreshPermissions('a');
    expect(inbox.pending()).toEqual([]);
    expect(clear).toHaveBeenCalledWith('a:1');
    load.mockResolvedValueOnce({ requests: [request(2)], connections: [] });
    await workflow.refreshPermissions('a');
    expect(inbox.pending().map((item) => item.key)).toEqual(['a:2']);
    workflow.serverResolvedPermission('a:2', { decision: 'deny', scope: 'always' });
    expect(inbox.pending()).toEqual([]);
    workflow.freshPermissionRules.value = new Set(['a', 'b']);
    workflow.markPermissionRulesStale('a');
    expect([...workflow.freshPermissionRules.value]).toEqual(['b']);
    inbox.reconcile(projects.value[0], [request(3)], []);
    projects.value = [project('b')];
    await workflow.refreshPermissions('a');
    // The full refresh drops requests owned by a closed project.
    await workflow.refreshPermissions();
    expect(inbox.pending()).toEqual([]);
  });

  it('rejects an out-of-order snapshot after an optimistic decision and refreshes again', async () => {
    const { projects, inbox, load, workflow, send } = setup();
    inbox.reconcile(projects.value[0], [request(1)], []);
    const old = deferred<{ requests: WirePermissionRequest[]; connections: [] }>();
    load.mockReturnValueOnce(old.promise).mockResolvedValueOnce({ requests: [], connections: [] });
    const refresh = workflow.refreshPermissions('a');
    await workflow.resolvePermission(inbox.pending()[0], 'allow', 'once');
    expect(send).toHaveBeenCalledTimes(1);
    old.resolve({ requests: [request(1)], connections: [] });
    await refresh;
    await vi.waitFor(() => {
      expect(load).toHaveBeenCalledTimes(2);
    });
    expect(inbox.pending()).toEqual([]);
  });
});
