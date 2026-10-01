import { describe, expect, it } from 'vitest';

import {
  allowsImmediately,
  formatPermissionCountdown,
  formatPermissionDelay,
  parsePermissionAutomation,
  parsePermissionHistory,
  parsePermissionResolution,
  PERMISSION_DELAYS,
  permissionBelongsToProject,
  permissionDelaysFor,
  PermissionInbox,
  VisiblePermissionTimer,
} from './permission-notifications';

const project = { id: 'p', name: 'Project', root: '/p', apiPath: '/p' };
const request = { id: 1, connection: 'c', tool: 'Bash', action: 'npm test', always_allow_supported: true };
describe('permission notifications', () => {
  it('reconciles pending requests and records an external decision when they disappear', () => {
    const inbox = new PermissionInbox();
    inbox.reconcile(project, [request], [{ id: 'c', tool: 'Claude', project: '/p', role: 'main', busy: true }], 10);
    expect(inbox.visible()?.agent).toBe('Claude');
    inbox.ignore('p:1');
    expect(inbox.visible()).toBeUndefined();
    expect(inbox.pending()).toHaveLength(1);
    inbox.reconcile(project, [], [], 20);
    expect(inbox.history(20)[0]).toMatchObject({ decision: 'external', resolvedAt: 20 });
  });
  it('reports whether reconciliation changed renderable permission state', () => {
    const inbox = new PermissionInbox();
    expect(inbox.reconcile(project, [], [], 10)).toBe(false);
    expect(inbox.reconcile(project, [request], [], 20)).toBe(true);
    expect(inbox.reconcile(project, [request], [], 30)).toBe(false);
    expect(inbox.reconcile(project, [{ ...request, action: 'cargo test' }], [], 40)).toBe(true);
    expect(inbox.reconcile(project, [], [], 50)).toBe(true);
  });
  it('hides a disconnected project without inventing a decision, then accepts its live refill', () => {
    const inbox = new PermissionInbox();
    inbox.reconcile(project, [request], [], 10);
    expect(inbox.discardProject('p')).toBe(true);
    expect(inbox.discardProject('p')).toBe(false);
    expect(inbox.visible()).toBeUndefined();
    expect(inbox.history(20)).toEqual([]);
    inbox.reconcile(project, [request], [], 30);
    expect(inbox.visible()).toMatchObject({ key: 'p:1', ignored: false });
  });
  it('uses the asking agent from the hook without inventing a connection role', () => {
    const inbox = new PermissionInbox();
    const codex = { ...request, agent: 'codex', project: '/p' };
    inbox.reconcile(project, [codex], [{ id: 'c', tool: 'codex', project: '/p', role: 'main', busy: true }], 5);
    expect(inbox.visible()).toMatchObject({ agent: 'Codex', role: 'main worker' });
    inbox.reconcile(project, [codex], [], 10);
    expect(inbox.visible()).toMatchObject({ agent: 'Codex', role: '', projectName: 'Project' });
    inbox.reconcile(project, [{ ...codex, agent: 'claude' }], [], 20);
    expect(inbox.visible()).toMatchObject({ agent: 'Claude', role: '' });
    inbox.reconcile(project, [{ ...request, agent: undefined }], [], 30);
    expect(inbox.visible()).toMatchObject({ agent: 'Claude', role: '' });
  });
  it('records known automatic decisions separately from pending state', () => {
    const inbox = new PermissionInbox();
    inbox.reconcile(project, [request], [], 10);
    inbox.resolve('p:1', 'allow', 'always', true, 20);
    expect(inbox.pending()).toHaveLength(0);
    expect(inbox.history(20)[0]).toMatchObject({ decision: 'allow', scope: 'always', automatic: true });
  });
  it('applies an authoritative resolution once while a local answer may still be in flight', () => {
    const inbox = new PermissionInbox();
    inbox.reconcile(project, [request], [], 10);
    expect(inbox.resolve('p:1', 'allow', 'once', false, 20)).toBe(true);
    expect(inbox.resolve('p:1', 'allow', 'once', false, 30)).toBe(false);
    expect(inbox.pending()).toEqual([]);
    expect(inbox.history(30)).toEqual([
      expect.objectContaining({ key: 'p:1', decision: 'allow', scope: 'once', resolvedAt: 20 }),
    ]);
  });
  it('removes a disconnected approval without treating it as a human denial', () => {
    const inbox = new PermissionInbox();
    inbox.reconcile(project, [request], [], 10);
    expect(inbox.removeExternal('p:1', 20)).toBe(true);
    expect(inbox.removeExternal('p:1', 30)).toBe(false);
    expect(inbox.visible()).toBeUndefined();
    expect(inbox.history(30)).toEqual([expect.objectContaining({ decision: 'external', resolvedAt: 20 })]);
  });
  it('restores an optimistically resolved request without retaining its presumed history', () => {
    const inbox = new PermissionInbox();
    inbox.reconcile(project, [request], [], 10);
    const item = inbox.visible()!;
    inbox.resolve(item.key, 'allow', 'once', false, 20);
    inbox.restore(item);
    expect(inbox.visible()).toMatchObject({ key: 'p:1', ignored: false });
    expect(inbox.history(30)).toEqual([]);
  });
  it('retains an empty-action tool request when a resolution nudge reconciles it away', () => {
    const inbox = new PermissionInbox();
    const toolSearch = { ...request, id: 81, tool: 'ToolSearch', action: '' };
    inbox.reconcile(project, [toolSearch], [], 10);
    inbox.reconcile(project, [], [], 20);
    expect(inbox.pending()).toEqual([]);
    expect(inbox.history(20)[0]).toMatchObject({
      key: 'p:81',
      tool: 'ToolSearch',
      action: '',
      decision: 'external',
      resolvedAt: 20,
    });
  });
  it('counts only visible presentation time and pauses while hidden', () => {
    const timer = new VisiblePermissionTimer();
    expect(timer.tick('p:1', 15_000, 0)).toBe(15_000);
    expect(timer.tick('p:1', 15_000, 5_000)).toBe(10_000);
    timer.hide(5_000);
    expect(timer.tick('p:1', 15_000, 50_000)).toBe(10_000);
    expect(timer.tick('p:1', 15_000, 60_000)).toBe(0);
  });
  it('cancels one request without affecting valid fail-closed settings', () => {
    const timer = new VisiblePermissionTimer();
    timer.tick('p:1', 15_000, 0);
    timer.cancel('p:1', 1_000);
    expect(timer.tick('p:1', 15_000, 20_000)).toBeUndefined();
    expect(parsePermissionAutomation({ action: 'allow', delayMs: 15_000 })).toEqual({
      action: 'allow',
      delayMs: 15_000,
    });
    expect(parsePermissionAutomation({ action: 'allow', delayMs: 12 })).toEqual({ action: 'allow', delayMs: 60_000 });
    expect(formatPermissionCountdown(61_001)).toBe('1:02');
  });
  it('offers an immediate 0 s delay for Auto-allow only (HS2-EBGCGW)', () => {
    expect(PERMISSION_DELAYS[0]).toBe(0);
    expect(permissionDelaysFor('allow')).toContain(0);
    expect(permissionDelaysFor('off')).toContain(0);
    expect(permissionDelaysFor('deny')).not.toContain(0);
    expect(permissionDelaysFor('deny')).toEqual(PERMISSION_DELAYS.slice(1));
    expect(PERMISSION_DELAYS.map(formatPermissionDelay)).toEqual([
      '0 seconds',
      '15 seconds',
      '1 minute',
      '2 minutes',
      '5 minutes',
      '15 minutes',
      '60 minutes',
    ]);
    expect(formatPermissionDelay(1_000)).toBe('1 second');
    expect(parsePermissionAutomation({ action: 'allow', delayMs: 0 })).toEqual({ action: 'allow', delayMs: 0 });
    expect(parsePermissionAutomation({ action: 'off', delayMs: 0 })).toEqual({ action: 'off', delayMs: 0 });
    // Never deny without showing the request: a stored or switched-to deny + 0 s uses the shortest deny delay.
    expect(parsePermissionAutomation({ action: 'deny', delayMs: 0 })).toEqual({ action: 'deny', delayMs: 15_000 });
    expect(allowsImmediately({ action: 'allow', delayMs: 0 })).toBe(true);
    expect(allowsImmediately({ action: 'allow', delayMs: 15_000 })).toBe(false);
    expect(allowsImmediately({ action: 'off', delayMs: 0 })).toBe(false);
    expect(allowsImmediately({ action: 'deny', delayMs: 0 })).toBe(false);
    const timer = new VisiblePermissionTimer();
    expect(timer.isCancelled('p:1')).toBe(false);
    timer.cancel('p:1');
    expect(timer.isCancelled('p:1')).toBe(true);
    timer.remove('p:1');
    expect(timer.isCancelled('p:1')).toBe(false);
  });
  it('rejects malformed and older-than-seven-days persisted history', () => {
    const week = 7 * 24 * 60 * 60 * 1000,
      now = week + 10;
    expect(parsePermissionHistory({ key: 'not-an-array' }, now)).toEqual([]);
    expect(
      parsePermissionHistory(
        [null, { key: 'valid', resolvedAt: 10 }, { key: 'expired', resolvedAt: 9 }, { key: 3, resolvedAt: 10 }],
        now,
      ),
    ).toEqual([{ key: 'valid', resolvedAt: 10 }]);
  });
  it('parses replayed resolution outcomes and safely degrades session scope', () => {
    expect(parsePermissionResolution('allow:always')).toEqual({ decision: 'allow', scope: 'always' });
    expect(parsePermissionResolution('deny:once')).toEqual({ decision: 'deny', scope: 'once' });
    expect(parsePermissionResolution('allow:session')).toEqual({ decision: 'allow', scope: 'once' });
    expect(parsePermissionResolution()).toBeUndefined();
  });
  it('uses explicit project identity before connection fallback', () => {
    const projects = [
        { ...project, stores: ['/p.hs2'] },
        { id: 'q', name: 'Other', root: '/q', stores: ['/q.hs2'], apiPath: '/q' },
      ],
      connections = [{ id: 'c', tool: 'Claude', project: '/p', role: 'main' as const, busy: true }];
    expect(permissionBelongsToProject({ ...request, project: '/q.hs2' }, connections, projects, 'p')).toBe(false);
    expect(permissionBelongsToProject({ ...request, project: '/p.hs2' }, connections, projects, 'p')).toBe(true);
    expect(permissionBelongsToProject(request, connections, projects, 'p')).toBe(true);
  });
});
