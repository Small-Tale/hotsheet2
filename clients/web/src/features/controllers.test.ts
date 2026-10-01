import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ConversationState } from '../ai-conversation';
import { Api, type CommandDefinition, type FullTicket, type RepositoryFile, type ToolConnection } from '../api';
import type { ProjectTabBarMode } from '../components/project-tab-bar';
import type { MobileMagnifiedTerminal, TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { ConversationExportOpenResult } from '../conversation-export';
import type { Project } from '../interactions/types';
import { INACTIVE_MOBILE_TERMINAL_FOCUS } from '../mobile-terminal-focus';
import type { DrawerAIChat } from '../project-drive';
import { initialTerminalVisibilityState } from '../terminal-visibility';
import { DEFAULT_WORKSPACE_PREFERENCES } from '../workspace-preferences';
import { createAiConfigurationController } from './ai-configuration';
import { createCommandsController } from './commands';
import { createConversationArchiveController } from './conversation-archive';
import { createGalleryController } from './gallery';
import { createPermissionsController } from './permissions';
import { createRepositoryController } from './repository';
import { createTerminalPresentation } from './terminal-presentation';

const project = (id: string): Project => ({
  id,
  name: id,
  root: `/work/${id}`,
  apiPath: `/api/${id}`,
  stores: [],
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
});
const command = (id: string): CommandDefinition => ({ id, title: id, kind: 'shell', command: 'true' });
const json = (value: unknown) => Response.json(value);
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
const fetchMock = vi.fn<typeof fetch>();
beforeEach(() => {
  vi.useFakeTimers();
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
  const stored = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
  });
  vi.stubGlobal('document', { querySelector: () => null, querySelectorAll: () => [], dispatchEvent: vi.fn() });
  vi.stubGlobal(
    'requestAnimationFrame',
    vi.fn(() => 1),
  );
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('feature owners retain live state across transitions (HS2-DHYGXJ)', () => {
  it('keeps command draft, range selection, validation and delayed autosave isolated by active project', async () => {
    const projects = signal([project('a'), project('b')]),
      selectedProjectId = signal('a');
    const owner = createCommandsController({
      projects,
      selectedProjectId,
      storedWorkspacePreferences: DEFAULT_WORKSPACE_PREFERENCES,
    });
    owner.commandDefinitions.value = [command('one'), command('two'), command('three')];
    owner.setCommandSettingsDraft('a', JSON.stringify(owner.commandDefinitions.value));
    owner.selectCommandRow('a', 'one', {});
    owner.selectCommandRow('a', 'three', { range: true });
    expect(owner.commandSelection('a')).toEqual(['one', 'two', 'three']);
    owner.selectCommandRow('a', 'two', { toggle: true });
    expect(owner.commandSelection('a')).toEqual(['one', 'three']);
    owner.updateCommandSetting('a', 'one', 'kind', 'ai');
    expect(owner.commandSettingsDefinitions('a')[0]).not.toHaveProperty('command');
    await vi.advanceTimersByTimeAsync(600);
    expect(owner.commandSettingsMessage('a')).toBe('one needs an AI prompt.');
    expect(fetchMock).not.toHaveBeenCalled();
    owner.updateCommandSetting('a', 'one', 'prompt', 'Review this project');
    const pending = deferred<Response>();
    fetchMock.mockReturnValueOnce(pending.promise);
    await vi.advanceTimersByTimeAsync(600);
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/a/commands');
    selectedProjectId.value = 'b';
    owner.commandDefinitions.value = [command('b-command')];
    owner.setCommandSettingsDraft('b', JSON.stringify([command('b-command')]));
    pending.resolve(json(owner.commandSettingsDefinitions('a')));
    await vi.advanceTimersByTimeAsync(0);
    expect(owner.commandDefinitions.value.map((item) => item.id)).toEqual(['b-command']);
    expect(owner.commandSettingsMessage('a')).toBe('Saved.');
    owner.commandSettingsDraftsByProject.value = {};
    expect(owner.commandSettingsDefinitions()).toEqual([command('b-command')]);
    fetchMock.mockImplementation(async (_input, init) => {
      if (typeof init?.body !== 'string') throw new Error('Expected JSON body');
      return json(JSON.parse(init.body));
    });
    owner.updateCommandSetting('b', 'b-command', 'title', 'After replacement');
    await vi.advanceTimersByTimeAsync(600);
    expect(owner.commandDefinitions.value[0].title).toBe('After replacement');
    expect(owner.commandSettingsMessage('b')).toBe('Saved.');
  });

  it('keeps a pending command save per project and flushes every one on page hide (HS2-25HAK3)', async () => {
    const projects = signal([project('a'), project('b')]),
      selectedProjectId = signal('a');
    const owner = createCommandsController({
      projects,
      selectedProjectId,
      storedWorkspacePreferences: DEFAULT_WORKSPACE_PREFERENCES,
    });
    const saves: Array<{ url: unknown; keepalive: boolean | undefined; ids: string[] }> = [];
    fetchMock.mockImplementation(async (input, init) => {
      if (typeof init?.body !== 'string') throw new Error('Expected JSON body');
      const body = JSON.parse(init.body) as CommandDefinition[];
      saves.push({ url: input, keepalive: init.keepalive, ids: body.map((item) => item.id) });
      return json(body);
    });
    owner.setCommandSettingsDraft('a', JSON.stringify([command('a-one'), command('a-two')]));
    owner.setCommandSettingsDraft('b', JSON.stringify([command('b-one')]));

    // Editing a second project inside the debounce window no longer drops the first one's save.
    owner.deleteCommandSetting('a', 'a-two');
    owner.updateCommandSetting('b', 'b-one', 'title', 'B renamed');
    await vi.advanceTimersByTimeAsync(600);
    expect(saves.map((save) => [save.url, save.keepalive, save.ids])).toEqual([
      ['/api/a/commands', undefined, ['a-one']],
      ['/api/b/commands', undefined, ['b-one']],
    ]);

    // A page hide inside the window saves at once, with keepalive, and nothing fires afterwards.
    saves.length = 0;
    owner.deleteCommandSetting('a', 'a-one');
    owner.flushCommandAutosaves();
    expect(saves).toEqual([{ url: '/api/a/commands', keepalive: true, ids: [] }]);
    await vi.advanceTimersByTimeAsync(600);
    expect(saves).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(0);
    expect(owner.commandSettingsMessage('a')).toBe('Saved.');

    // Flushing with nothing pending is a no-op, and later edits debounce normally again.
    owner.flushCommandAutosaves();
    expect(saves).toHaveLength(1);
    owner.updateCommandSetting('b', 'b-one', 'title', 'B again');
    await vi.advanceTimersByTimeAsync(599);
    expect(saves).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(saves.at(-1)).toMatchObject({ url: '/api/b/commands', keepalive: undefined });
  });

  it('persists kept command groups through add, populate, empty, reload, and delete (HS2-EZ5KMC)', async () => {
    const projects = signal([project('a')]),
      selectedProjectId = signal('a');
    const owner = createCommandsController({
      projects,
      selectedProjectId,
      storedWorkspacePreferences: DEFAULT_WORKSPACE_PREFERENCES,
    });
    const groupSaves: unknown[] = [];
    fetchMock.mockImplementation(async (input, init) => {
      if (typeof init?.body !== 'string') throw new Error('Expected JSON body');
      const body: unknown = JSON.parse(init.body);
      if (input === '/api/a/command-groups') groupSaves.push(body);
      return json(body);
    });
    owner.commandDefinitions.value = [command('one')];
    owner.setCommandSettingsDraft('a', JSON.stringify([command('one')]));
    vi.stubGlobal('window', { prompt: () => ' Ideas ' });
    owner.addCommandGroup('a');
    await vi.advanceTimersByTimeAsync(0);
    expect(owner.commandSettingsExtraGroups('a')).toEqual(['Ideas']);
    expect(groupSaves).toEqual([['Ideas']]);
    // A duplicate name is rejected without another save.
    owner.addCommandGroup('a');
    expect(owner.commandSettingsMessage('a')).toBe('A group named "Ideas" already exists.');
    expect(groupSaves).toHaveLength(1);
    // Reload: the server's kept groups replace the in-memory list.
    owner.commandSettingsExtraGroupsByProject.value = {};
    owner.loadCommandGroups('a', ['Ideas']);
    expect(owner.commandSettingsExtraGroups('a')).toEqual(['Ideas']);
    // Populating the kept group neither drops nor re-saves it.
    owner.reorderCommandSettings('a', ['one'], { kind: 'group', group: 'Ideas' });
    expect(owner.commandSettingsDefinitions('a')[0].group).toBe('Ideas');
    expect(owner.commandSettingsExtraGroups('a')).toEqual(['Ideas']);
    expect(groupSaves).toHaveLength(1);
    // Removing the group's last command keeps the group.
    owner.deleteCommandSetting('a', 'one');
    expect(owner.commandSettingsDefinitions('a')).toEqual([]);
    expect(owner.commandSettingsExtraGroups('a')).toEqual(['Ideas']);
    // An implicitly created group that is emptied is kept and saved too.
    owner.setCommandSettingsDraft('a', JSON.stringify([{ ...command('two'), group: 'Release' }]));
    owner.deleteCommandSetting('a', 'two');
    await vi.advanceTimersByTimeAsync(0);
    expect(owner.commandSettingsExtraGroups('a')).toEqual(['Ideas', 'Release']);
    expect(groupSaves.at(-1)).toEqual(['Ideas', 'Release']);
    // A refresh racing an in-flight group save does not clobber the local edit.
    const pending = deferred<Response>();
    fetchMock.mockReturnValueOnce(pending.promise);
    owner.deleteCommandGroup('a', 'Release');
    owner.loadCommandGroups('a', ['Ideas', 'Release']);
    expect(owner.commandSettingsExtraGroups('a')).toEqual(['Ideas']);
    pending.resolve(json(['Ideas']));
    await vi.advanceTimersByTimeAsync(0);
    owner.loadCommandGroups('a', ['Ideas']);
    expect(owner.commandSettingsExtraGroups('a')).toEqual(['Ideas']);
    // Deleting the last empty group persists an empty list; a failed save reports its error.
    fetchMock.mockRejectedValueOnce(new Error('offline'));
    owner.deleteCommandGroup('a', 'Ideas');
    await vi.advanceTimersByTimeAsync(0);
    expect(owner.commandSettingsExtraGroups('a')).toEqual([]);
    expect(owner.commandSettingsMessage('a')).toBe('offline');
    // A group edit for a project that is no longer open is kept locally without a request.
    const requests = fetchMock.mock.calls.length;
    vi.stubGlobal('window', { prompt: () => 'Later' });
    owner.addCommandGroup('closed');
    await vi.advanceTimersByTimeAsync(0);
    expect(owner.commandSettingsExtraGroups('closed')).toEqual(['Later']);
    expect(fetchMock).toHaveBeenCalledTimes(requests);
  });

  it('rejects stale repository pages after project replacement and permits reset/empty/refill pagination', async () => {
    const active = signal<Project | undefined>(project('a'));
    const owner = createRepositoryController({
      project: () => active.value,
      selectedTicket: signal<FullTicket | null>(null),
      showToast: vi.fn(),
    });
    const pending = deferred<Response>();
    fetchMock.mockReturnValueOnce(pending.promise);
    const first = owner.loadRepositoryDetail('unstaged', true);
    active.value = project('b');
    const file: RepositoryFile = { path: 'b.ts', unstaged: 'modified', untracked: false, conflicted: false };
    fetchMock.mockResolvedValueOnce(json({ items: [file], next_cursor: 1 }));
    await owner.loadRepositoryDetail('unstaged', true);
    pending.resolve(json({ items: [{ ...file, path: 'stale-a.ts' }], next_cursor: null }));
    await first;
    expect(owner.repositoryDetail.value.files.map((item) => item.path)).toEqual(['b.ts']);
    fetchMock.mockResolvedValueOnce(json({ items: [], next_cursor: null }));
    await owner.loadRepositoryDetail('unstaged');
    expect(owner.repositoryDetail.value).toMatchObject({ loaded: true, loading: false, nextCursor: undefined });
    const requests = fetchMock.mock.calls.length;
    await owner.loadRepositoryDetail('unstaged');
    expect(fetchMock).toHaveBeenCalledTimes(requests);
    fetchMock.mockResolvedValueOnce(json({ items: [], next_cursor: null }));
    await owner.loadRepositoryDetail('staged', true);
    expect(owner.repositoryDetail.value.files).toEqual([]);
    fetchMock.mockResolvedValueOnce(json({ items: [file], next_cursor: null }));
    await owner.loadRepositoryDetail('unstaged', true);
    expect(owner.repositoryDetail.value.files).toEqual([file]);
    owner.repositoryFileSelectionAnchor = 'new-anchor';
    expect(owner.repositoryFileSelectionAnchor).toBe('new-anchor');
    active.value = undefined;
    expect(owner.repositoryStatusSurface()).toBeNull();
  });

  it('restores failed optimistic permission decisions and resolves the original project after selection changes', async () => {
    const projects = signal([project('a'), project('b')]),
      selectedProjectId = signal('a');
    const owner = createPermissionsController({ projects, selectedProjectId });
    owner.permissionInbox.reconcile(
      projects.value[0],
      [{ id: 1, connection: 'c', tool: 'Bash', action: 'npm test' }],
      [],
    );
    const item = owner.pendingPermissions()[0];
    const pending = deferred<Response>();
    fetchMock.mockReturnValueOnce(pending.promise);
    const first = owner.resolvePermission(item, 'allow', 'once');
    expect(owner.pendingPermissions()).toEqual([]);
    selectedProjectId.value = 'b';
    pending.resolve(Response.json({ error: 'Disconnected' }, { status: 503 }));
    await first;
    expect(owner.pendingPermissions()[0].key).toBe('a:1');
    expect(owner.projectPendingPermissions()).toEqual([]);
    expect(String(owner.permissionPopupSurface())).toContain('Disconnected');
    fetchMock.mockResolvedValueOnce(json({ connection: 'c', decision: 'deny', persisted: false }));
    await owner.resolvePermission(item, 'deny', 'once');
    expect(fetchMock.mock.calls.at(-1)?.[0]).toBe('/api/a/permissions/1');
    expect(owner.pendingPermissions()).toEqual([]);
    expect(owner.projectPermissionHistory('a')[0]).toMatchObject({ decision: 'deny' });
    owner.permissionInbox.reconcile(projects.value[0], [{ id: 2, connection: 'c', tool: 'Bash', action: 'again' }], []);
    expect(owner.permissionCount('a')).toBe(1);
    expect(owner.permissionCount('b')).toBe(0);
  });

  it('removes stale permission popups when one project disconnects while reconciling another', async () => {
    const projects = signal([project('a'), project('b')]),
      owner = createPermissionsController({ projects, selectedProjectId: signal('a') });
    owner.permissionInbox.reconcile(projects.value[0], [{ id: 1, connection: 'a', tool: 'Bash', action: 'old' }], []);
    expect(owner.permissionPopupSurface()).toBeDefined();
    fetchMock.mockImplementation(async (input) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
      if (url === '/api/a/permissions') return Response.json({ error: 'offline' }, { status: 503 });
      if (url === '/api/b/permissions')
        return json([{ id: 2, connection: 'b', tool: 'Edit', action: 'b.ts', project: '/work/b' }]);
      return json([]);
    });
    await owner.refreshPermissions();
    expect(owner.pendingPermissions().map((item) => item.key)).toEqual(['b:2']);
    expect(owner.permissionPopupSurface()).toBeDefined();
    expect(owner.projectPermissionHistory('a')).toEqual([]);
    fetchMock.mockImplementation(async (input) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
      if (url.endsWith('/permissions')) return json([]);
      return json([]);
    });
    await owner.refreshPermissions();
    expect(owner.permissionPopupSurface()).toBeUndefined();
    expect(owner.projectPermissionHistory('b')).toHaveLength(1);
  });

  it('repairs a missed permission event on the next scheduled reconciliation', async () => {
    vi.stubGlobal('window', { setInterval });
    const owner = createPermissionsController({ projects: signal([project('a')]), selectedProjectId: signal('a') });
    let pending = [{ id: 1, connection: 'c', tool: 'Bash', action: 'old' }];
    fetchMock.mockImplementation(async (input) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
      return json(url.endsWith('/permissions') ? pending : []);
    });
    owner.startPermissionUpdates();
    await vi.advanceTimersByTimeAsync(0);
    expect(owner.pendingPermissions()).toHaveLength(1);
    pending = [];
    await vi.advanceTimersByTimeAsync(10_000);
    expect(owner.pendingPermissions()).toEqual([]);
    expect(owner.projectPermissionHistory('a')).toHaveLength(1);
  });

  it('reconciles again when a resolution arrives during an older permission fetch', async () => {
    const owner = createPermissionsController({ projects: signal([project('a')]), selectedProjectId: signal('a') });
    const older = deferred<Response>();
    let requests = 0;
    fetchMock.mockImplementation(async (input) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
      if (url.endsWith('/connections')) return json([]);
      requests += 1;
      return requests === 1 ? older.promise : json([]);
    });
    const first = owner.refreshPermissions();
    await owner.refreshPermissions();
    older.resolve(json([{ id: 7, connection: 'c', tool: 'Bash', action: 'stale' }]));
    await first;
    await vi.waitFor(() => {
      expect(requests).toBe(2);
    });
    await vi.waitFor(() => {
      expect(owner.pendingPermissions()).toEqual([]);
    });
  });

  it('never reopens an externally resolved popup from an older in-flight permissions response', async () => {
    const owner = createPermissionsController({ projects: signal([project('a')]), selectedProjectId: signal('a') });
    owner.permissionInbox.reconcile(project('a'), [{ id: 7, connection: 'c', tool: 'Bash', action: 'stale' }], []);
    const older = deferred<Response>();
    let requests = 0;
    fetchMock.mockImplementation(async (input) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
      if (url.endsWith('/connections')) return json([]);
      requests += 1;
      return requests === 1 ? older.promise : json([]);
    });
    const first = owner.refreshPermissions();
    owner.serverResolvedPermission('a:7');
    expect(owner.permissionPopupSurface()).toBeUndefined();
    older.resolve(json([{ id: 7, connection: 'c', tool: 'Bash', action: 'stale' }]));
    await first;
    expect(owner.permissionPopupSurface()).toBeUndefined();
    await vi.waitFor(() => {
      expect(requests).toBe(2);
    });
    expect(owner.pendingPermissions()).toEqual([]);
    expect(owner.projectPermissionHistory('a')).toEqual([expect.objectContaining({ decision: 'external' })]);
  });

  it('does not restore a popup when the server says its approval request is gone', async () => {
    const owner = createPermissionsController({ projects: signal([project('a')]), selectedProjectId: signal('a') });
    owner.permissionInbox.reconcile(project('a'), [{ id: 7, connection: 'c', tool: 'Bash', action: 'stale' }], []);
    const response = deferred<Response>();
    fetchMock.mockImplementation(async (input) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
      return url.endsWith('/permissions/7') ? response.promise : json([]);
    });
    const item = owner.pendingPermissions()[0];
    const answer = owner.resolvePermission(item, 'allow', 'once');
    expect(owner.permissionPopupSurface()).toBeUndefined();
    response.resolve(Response.json({ error: 'Request missing' }, { status: 404 }));
    await answer;
    expect(owner.permissionPopupSurface()).toBeUndefined();
    expect(owner.pendingPermissions()).toEqual([]);
    expect(owner.projectPermissionHistory('a')).toEqual([expect.objectContaining({ decision: 'external' })]);
  });

  it('does not restore a permission popup after its project closes during a fetch', async () => {
    const projects = signal([project('a')]),
      owner = createPermissionsController({ projects, selectedProjectId: signal('a') }),
      response = deferred<Response>();
    owner.permissionInbox.reconcile(projects.value[0], [{ id: 1, connection: 'c', tool: 'Bash', action: 'old' }], []);
    fetchMock.mockImplementation(async (input) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
      return url.endsWith('/permissions') ? response.promise : json([]);
    });
    const refresh = owner.refreshPermissions();
    projects.value = [];
    response.resolve(json([{ id: 2, connection: 'c', tool: 'Edit', action: 'new' }]));
    await refresh;
    expect(owner.pendingPermissions()).toEqual([]);
    expect(owner.permissionPopupSurface()).toBeUndefined();
  });

  it('auto-allows immediately (0 s) without presenting the popup while still recording and sending it (HS2-EBGCGW)', async () => {
    const projects = signal([project('a'), project('b')]),
      owner = createPermissionsController({ projects, selectedProjectId: signal('a') }),
      posts: string[] = [];
    owner.permissionAutomationByProject.value = {
      a: { action: 'allow', delayMs: 0 },
      b: { action: 'deny', delayMs: 15_000 },
    };
    let pendingA = [{ id: 1, connection: 'c', tool: 'Bash', action: 'npm test' }];
    fetchMock.mockImplementation(async (input, init) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
      if (init?.method === 'POST') {
        posts.push(url);
        pendingA = [];
        return json({ connection: 'c', decision: 'allow', persisted: false });
      }
      if (url === '/api/a/permissions') return json(pendingA);
      if (url === '/api/b/permissions') return json([{ id: 2, connection: 'd', tool: 'Edit', action: 'b.ts' }]);
      return json([]);
    });
    await owner.refreshPermissions();
    // The immediate request never became the visible popup: only the other project's request remains.
    expect(owner.pendingPermissions().map((item) => item.key)).toEqual(['b:2']);
    expect(String(owner.permissionPopupSurface())).toContain('b.ts');
    await vi.waitFor(() => {
      expect(posts).toEqual(['/api/a/permissions/1']);
    });
    expect(owner.projectPermissionHistory('a')).toEqual([
      expect.objectContaining({ key: 'a:1', decision: 'allow', scope: 'once', automatic: true }),
    ]);
    // A later refresh neither reopens nor re-sends the decided request.
    await owner.refreshPermissions();
    expect(posts).toHaveLength(1);
    expect(owner.projectPermissionHistory('a')).toHaveLength(1);
  });

  it('leaves ignored requests alone and falls back to the popup without retrying a failed immediate allow', async () => {
    vi.stubGlobal('window', { setInterval });
    const projects = signal([project('a')]),
      owner = createPermissionsController({ projects, selectedProjectId: signal('a') });
    let posts = 0;
    fetchMock.mockImplementation(async (input, init) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
      if (init?.method === 'POST') {
        posts += 1;
        return Response.json({ error: 'Disconnected' }, { status: 503 });
      }
      return json(url.endsWith('/permissions') ? [{ id: 1, connection: 'c', tool: 'Bash', action: 'npm test' }] : []);
    });
    owner.permissionInbox.reconcile(
      projects.value[0],
      [{ id: 1, connection: 'c', tool: 'Bash', action: 'npm test' }],
      [],
    );
    owner.permissionInbox.ignore('a:1');
    // Off -> Auto-allow 0 s: an ignored request stays parked for a manual decision.
    owner.permissionAutomationByProject.value = { a: { action: 'allow', delayMs: 0 } };
    owner.updatePermissionTimer();
    expect(posts).toBe(0);
    expect(owner.pendingPermissions()).toHaveLength(1);
    // Presenting it again lets the immediate setting decide it; the send fails.
    owner.permissionInbox.present('a:1');
    owner.updatePermissionTimer();
    expect(owner.pendingPermissions()).toEqual([]);
    await vi.waitFor(() => {
      expect(owner.pendingPermissions()).toHaveLength(1);
    });
    expect(posts).toBe(1);
    expect(String(owner.permissionPopupSurface())).toContain('Disconnected');
    // Neither the per-second timer nor a refresh turns the failure into a request loop.
    owner.startPermissionUpdates();
    await vi.advanceTimersByTimeAsync(25_000);
    expect(posts).toBe(1);
    expect(owner.pendingPermissions()).toHaveLength(1);
    expect(owner.permissionCountdown).toBeUndefined();
  });

  it('resets gallery-owned playback, gestures, annotations and menu state before a second image edit', () => {
    const ticket: FullTicket = {
      id: 't',
      slug: 'HS2-T',
      title: 'Proof',
      native_id: 't',
      qualified_id: 'git:t',
      connection_id: 'git',
      up_next: false,
      feedback_needed: false,
      tags: [],
      blocked_by: [],
      claim_count: 0,
      details: '',
      notes: [],
      attachments: [
        {
          id: 'one',
          filename: 'one.png',
          created_at: '',
          annotations: [{ id: 'a', x: 0, y: 0, width: 1, height: 1, text: 'before' }],
        },
        { id: 'two', filename: 'two.png', created_at: '' },
      ],
    };
    const selectedTicket = signal<FullTicket | null>(ticket);
    const owner = createGalleryController({
      selectedTicket,
      project: () => project('a'),
      api: () => new Api('/api/a'),
      attachmentContext: () => ({ checkout: 'a', ticket: 'HS2-T', baseUrl: '/api/a' }),
      showToast: vi.fn(),
      error: signal(''),
    });
    const images = owner.galleryImages();
    owner.resetAttachmentGallery(images[0].url);
    expect(owner.attachmentGalleryAnnotations.value[0].text).toBe('before');
    owner.attachmentGalleryAnnotations.value[0].text = 'local';
    expect(ticket.attachments[0].annotations?.[0].text).toBe('before');
    owner.attachmentGalleryDuration.value = 1000;
    owner.updateGalleryPlaybackPresentation(2000);
    expect(owner.attachmentGalleryLivePlayhead).toBe(1000);
    owner.attachmentGalleryLiveVolume = 0.2;
    owner.attachmentGalleryScale.value = 2;
    owner.shiftGallery(1);
    expect(owner.attachmentGalleryUrl.value).toBe(images[1].url);
    expect(owner.attachmentGalleryLivePlayhead).toBe(0);
    expect(owner.attachmentGalleryLiveVolume).toBe(1);
    expect(owner.attachmentGalleryScale.value).toBeUndefined();
    expect(owner.attachmentGalleryAnnotations.value).toEqual([]);
    owner.resetAttachmentGallery();
    selectedTicket.value = null;
    expect(owner.galleryImages()).toEqual([]);
    selectedTicket.value = ticket;
    owner.resetAttachmentGallery(images[0].url);
    expect(owner.attachmentGalleryAnnotations.value[0].text).toBe('before');
  });
});

describe('gallery source for stacked readers (HS2-97E0QR)', () => {
  it('opens, shifts, saves, and closes against the owning reader ticket, then returns to the workspace selection', async () => {
    const attachment = (id: string, text?: string) => ({
      id,
      filename: `${id}.png`,
      created_at: '',
      ...(text ? { annotations: [{ id: `${id}-a`, x: 0, y: 0, width: 1, height: 1, text }] } : {}),
    });
    const ticketFor = (id: string, slug: string, attachments: FullTicket['attachments']): FullTicket => ({
      id,
      slug,
      title: slug,
      native_id: id,
      qualified_id: `git:${id}`,
      connection_id: 'git',
      status: 'started',
      up_next: false,
      feedback_needed: false,
      tags: [],
      blocked_by: [],
      claim_count: 0,
      details: '',
      notes: [],
      attachments,
    });
    const workspace = ticketFor('selected', 'HS2-SEL', [attachment('sel')]),
      linkedTicket = ticketFor('linked', 'HS2-LNK', [attachment('l1', 'linked note'), attachment('l2')]),
      selectedTicket = signal<FullTicket | null>(workspace),
      updates: FullTicket[] = [],
      saved: Array<[string, string, string]> = [],
      api = new Api('/api/a');
    vi.spyOn(api, 'updateCheckoutAttachmentAnnotations').mockImplementation(async (checkout, id, attachmentId) => {
      saved.push([checkout, id, attachmentId]);
      return { store: 'git', ticket: { ...linkedTicket, title: 'saved', store: 'git' } };
    });
    const owner = createGalleryController({
      selectedTicket,
      project: () => project('a'),
      api: () => api,
      attachmentContext: (ticket, owning) => ({
        checkout: owning?.id ?? 'a',
        ticket: ticket.slug,
        baseUrl: '/api/a',
      }),
      showToast: vi.fn(),
      error: signal(''),
    });
    const source = {
      ticket: linkedTicket,
      project: project('b'),
      update: (ticket: FullTicket) => updates.push(ticket),
    };
    const linkedImages = owner.galleryImages(source.ticket, source.project);
    expect(linkedImages.map((image) => image.name)).toEqual(['l1.png', 'l2.png']);
    expect(linkedImages[0].url).toContain('/checkouts/b/tickets/git%3Alinked/attachments/l1');
    // Opened for the linked reader: the gallery's own view and annotations follow that ticket.
    owner.resetAttachmentGallery(linkedImages[0].url, source);
    expect(owner.galleryImages().map((image) => image.name)).toEqual(['l1.png', 'l2.png']);
    expect(owner.attachmentGalleryAnnotations.value[0].text).toBe('linked note');
    // Shifting keeps the source.
    owner.shiftGallery(1);
    expect(owner.attachmentGalleryUrl.value).toBe(linkedImages[1].url);
    owner.shiftGallery(-1);
    // An annotation edit saves to the owning project and ticket and refreshes that reader, not the selection.
    owner.beginGalleryAnnotationSession();
    owner.attachmentGalleryAnnotations.value = [{ id: 'new', x: 0, y: 0, width: 1, height: 1, text: 'edited' }];
    owner.finishGalleryAnnotationSession();
    await vi.waitFor(() => {
      expect(saved).toEqual([['b', 'git:linked', 'l1']]);
    });
    await vi.waitFor(() => {
      expect(updates.map((ticket) => ticket.title)).toEqual(['saved']);
    });
    expect(selectedTicket.value).toBe(workspace);
    // A read-only source (a linked reader) never starts an annotation session.
    owner.resetAttachmentGallery(linkedImages[0].url, { ...source, readOnly: true });
    owner.beginGalleryAnnotationSession();
    owner.attachmentGalleryAnnotations.value = [{ id: 'blocked', x: 0, y: 0, width: 1, height: 1, text: 'no' }];
    owner.finishGalleryAnnotationSession();
    for (let flush = 0; flush < 5; flush += 1) await Promise.resolve();
    expect(saved).toHaveLength(1);
    // Closing clears the source; the next open without one belongs to the workspace selection again.
    owner.resetAttachmentGallery();
    expect(owner.galleryImages().map((image) => image.name)).toEqual(['sel.png']);
    owner.resetAttachmentGallery(owner.galleryImages()[0].url);
    expect(owner.galleryImages().map((image) => image.name)).toEqual(['sel.png']);
  });
});

function chatOwners() {
  const active = signal<Project | undefined>(project('a'));
  const selectedProjectId = signal('a');
  const conversationConnectionId = signal<string | undefined>('chat-a');
  const conversationStates = signal<Record<string, ConversationState>>({
    'chat-a': {
      messages: [
        { id: 'one', role: 'user', content: 'First' },
        { id: 'two', role: 'assistant', content: 'Second' },
      ],
    },
  });
  const driveConnectionsByProject = signal<Record<string, ToolConnection[]>>({
    a: [
      {
        id: 'chat-a',
        tool: 'codex',
        project: '/work/a',
        role: 'main',
        busy: false,
        actions: ['send_turn'],
        model: 'slow',
      },
    ],
  });
  const terminalDrawerChatsByProject = signal<Record<string, DrawerAIChat[]>>({
    a: [{ id: 'ai-chat:chat-a', connectionId: 'chat-a', name: 'Chat', tool: 'codex', model: 'slow' }],
  });
  const conversationSelections = signal<Record<string, { model?: string; effort?: string }>>({});
  const common = {
    conversationConnectionId,
    conversationStates,
    driveConnectionsByProject,
    terminalDrawerChatsByProject,
    project: () => active.value,
    showToast: vi.fn(),
    error: signal(''),
  };
  const ai = createAiConfigurationController({
    ...common,
    selectedProjectId,
    conversationSelections,
    conversationOpen: signal(false),
    createDrawerAIChat: vi.fn(async () => undefined),
    beginConversation: vi.fn(),
    updateConversation: vi.fn(),
    commandSettingsDefinitions: () => [],
    commandSettingsEditingId: signal<string | undefined>(undefined),
  });
  ai.aiTools.value = [
    {
      id: 'codex',
      display_name: 'Codex',
      default_model: 'slow',
      actions: ['change_model', 'change_effort'],
      models: [
        { id: 'slow', label: 'Slow', effort_levels: ['high'] },
        { id: 'fast', label: 'Fast', effort_levels: ['low', 'medium'] },
      ],
    },
  ];
  const archive = createConversationArchiveController({
    ...common,
    aiToolLabel: ai.aiToolLabel,
    conversationAiSelection: ai.conversationAiSelection,
    terminalVisibility: signal(initialTerminalVisibilityState()),
    persistTerminalVisibility: vi.fn(),
    replaceConversationStates: (next) => {
      conversationStates.value = next;
    },
    selectDrawerItem: vi.fn(),
    setTerminalDrawerVisible: vi.fn(),
  });
  return { ...common, active, selectedProjectId, conversationSelections, ai, archive };
}

describe('conversation feature transitions (HS2-DHYGXJ)', () => {
  it('projects replacement model/effort state into selections, drawer chats and live connections', () => {
    const state = chatOwners();
    expect(state.ai.conversationAiSelection('chat-a')).toMatchObject({ model: 'slow', effort: 'high' });
    state.ai.selectConversationModel('fast');
    expect(state.ai.conversationAiSelection('chat-a')).toMatchObject({ model: 'fast', effort: 'low' });
    expect(state.terminalDrawerChatsByProject.value.a[0]).toMatchObject({ model: 'fast', effort: 'low' });
    expect(state.driveConnectionsByProject.value.a[0]).toMatchObject({ model: 'fast', effort: 'low' });
    state.ai.selectConversationEffort('medium');
    expect(state.ai.conversationAiSelection('chat-a')).toMatchObject({ model: 'fast', effort: 'medium' });
    state.conversationSelections.value = {};
    state.terminalDrawerChatsByProject.value = {};
    state.driveConnectionsByProject.value = {};
    expect(state.ai.conversationAiSelection('chat-a')).toMatchObject({ model: 'slow', effort: 'high' });
    state.ai.selectConversationModel('fast');
    expect(state.ai.conversationAiSelection('chat-a')).toMatchObject({ model: 'fast', effort: 'low' });
    state.ai.selectDriveModel('fast');
    state.active.value = project('b');
    state.selectedProjectId.value = 'b';
    expect(state.ai.effectiveDriveSelection()).toMatchObject({ model: 'slow' });
    expect(state.ai.effectiveDriveSelection('a')).toMatchObject({ model: 'fast' });
  });

  it('ignores late AI configuration from a replaced project', async () => {
    const state = chatOwners(),
      late = deferred<Response>();
    fetchMock.mockImplementation(async (url) => {
      if (typeof url !== 'string') throw new Error('Expected string URL');
      if (url === '/api/a/ai-tools') return late.promise;
      if (url.includes('ai-tools')) return json([{ id: 'b-tool', display_name: 'B tool', models: [] }]);
      return json({ tool: url.includes('/api/b/') ? 'b-tool' : 'a-tool' });
    });
    const first = state.ai.refreshAiConfiguration();
    state.active.value = project('b');
    state.selectedProjectId.value = 'b';
    await state.ai.refreshAiConfiguration();
    expect(state.ai.aiDefaults.value.tool).toBe('b-tool');
    late.resolve(json([{ id: 'a-tool', display_name: 'A tool', models: [] }]));
    await first;
    expect(state.ai.aiDefaults.value.tool).toBe('b-tool');
    expect(state.ai.aiToolOptions()).toEqual([{ id: 'b-tool', label: 'B tool' }]);
    expect(state.ai.aiConfigurationProjectId).toBe('b');
  });

  it('requests AI tools and settings in parallel and applies them together (HS2-QV8B7R)', async () => {
    const state = chatOwners(),
      tools = deferred<Response>(),
      settings = deferred<Response>();
    fetchMock.mockImplementation(async (url) => {
      if (typeof url !== 'string') throw new Error('Expected string URL');
      if (url.startsWith('/api/a/ai-tools')) return tools.promise;
      if (url === '/api/a/ai-settings') return settings.promise;
      throw new Error(`Unexpected ${url}`);
    });
    const loading = state.ai.refreshAiConfiguration();
    await vi.advanceTimersByTimeAsync(0);
    // Both requests are in flight before either answers.
    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual(['/api/a/ai-tools', '/api/a/ai-settings']);
    expect(state.ai.aiSettingsLoading.value).toBe(true);
    // Settings answering first applies nothing until the tools arrive too.
    settings.resolve(json({ tool: 'a-tool' }));
    await vi.advanceTimersByTimeAsync(0);
    expect(state.ai.aiSettingsLoading.value).toBe(true);
    expect(state.ai.aiConfigurationProjectId).not.toBe('a');
    tools.resolve(json([{ id: 'a-tool', display_name: 'A tool', models: [] }]));
    await loading;
    expect(state.ai.aiSettingsLoading.value).toBe(false);
    expect(state.ai.aiDefaults.value.tool).toBe('a-tool');
    expect(state.ai.aiToolOptions()).toEqual([{ id: 'a-tool', label: 'A tool' }]);
    expect(state.ai.aiConfigurationProjectId).toBe('a');
    expect(state.ai.aiSettingsMessage.value).toBe('');

    // A failing settings read fails the whole refresh without applying the tools.
    fetchMock.mockReset();
    fetchMock.mockImplementation(async (url) => {
      if (typeof url !== 'string') throw new Error('Expected string URL');
      if (url.startsWith('/api/a/ai-tools')) return json([{ id: 'other', display_name: 'Other', models: [] }]);
      return new Response(JSON.stringify({ error: 'settings unavailable' }), { status: 500 });
    });
    await state.ai.refreshAiConfiguration();
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(state.ai.aiSettingsMessage.value).toContain('settings unavailable');
    expect(state.ai.aiSettingsLoading.value).toBe(false);
    expect(state.ai.aiConfigurationProjectId).toBe('');
    expect(state.ai.aiToolOptions()).toEqual([{ id: 'a-tool', label: 'A tool' }]);
  });

  it('keeps an explicit AI-tool refresh ordered before the settings read (HS2-QV8B7R)', async () => {
    const state = chatOwners(),
      tools = deferred<Response>();
    fetchMock.mockImplementation(async (url) => {
      if (typeof url !== 'string') throw new Error('Expected string URL');
      if (url === '/api/a/ai-tools?refresh=true') return tools.promise;
      if (url === '/api/a/ai-settings') return json({ tool: 'a-tool' });
      throw new Error(`Unexpected ${url}`);
    });
    const loading = state.ai.refreshAiConfiguration(undefined, true);
    await vi.advanceTimersByTimeAsync(0);
    // The settings must be validated against the rescanned tools, so they wait for the refresh.
    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual(['/api/a/ai-tools?refresh=true']);
    tools.resolve(json([{ id: 'a-tool', display_name: 'A tool', models: [] }]));
    await loading;
    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual(['/api/a/ai-tools?refresh=true', '/api/a/ai-settings']);
    expect(state.ai.aiDefaults.value.tool).toBe('a-tool');
    expect(state.ai.aiSettingsLoading.value).toBe(false);
  });

  it('restores cached per-project AI configuration on A→B→A without refetching (HS2-AZZ9TF)', async () => {
    const state = chatOwners();
    fetchMock.mockImplementation(async (url) => {
      if (typeof url !== 'string') throw new Error('Expected string URL');
      const owner = url.includes('/api/b/') ? 'b' : 'a';
      if (url.includes('ai-tools')) return json([{ id: `${owner}-tool`, display_name: `${owner} tool`, models: [] }]);
      return json({ tool: `${owner}-tool` });
    });
    const activate = (id: string) => {
      state.active.value = project(id);
      state.selectedProjectId.value = id;
    };
    activate('a');
    expect(state.ai.restoreAiConfiguration(project('a'))).toBe(false);
    await state.ai.refreshAiConfiguration();
    activate('b');
    expect(state.ai.restoreAiConfiguration(project('b'))).toBe(false);
    await state.ai.refreshAiConfiguration();
    const calls = fetchMock.mock.calls.length;

    activate('a');
    expect(state.ai.restoreAiConfiguration(project('a'))).toBe(true);
    expect(state.ai.aiDefaults.value.tool).toBe('a-tool');
    expect(state.ai.aiToolOptions()).toEqual([{ id: 'a-tool', label: 'a tool' }]);
    expect(state.ai.aiConfigurationProjectId).toBe('a');
    // Restoring the already-current project is a no-op that still reports current.
    expect(state.ai.restoreAiConfiguration(project('a'))).toBe(true);
    activate('b');
    expect(state.ai.restoreAiConfiguration(project('b'))).toBe(true);
    expect(state.ai.aiDefaults.value.tool).toBe('b-tool');
    expect(fetchMock.mock.calls.length).toBe(calls);

    // Eviction forgets the cache: the next activation must load again.
    state.ai.forgetAiConfiguration('a');
    activate('a');
    expect(state.ai.restoreAiConfiguration(project('a'))).toBe(false);
    await state.ai.refreshAiConfiguration();
    expect(state.ai.aiDefaults.value.tool).toBe('a-tool');
    expect(fetchMock.mock.calls.length).toBeGreaterThan(calls);
  });

  it('keeps a late answer warm for its own project but drops it after the project is forgotten', async () => {
    const state = chatOwners(),
      lateA = deferred<Response>(),
      lateC = deferred<Response>();
    fetchMock.mockImplementation(async (url) => {
      if (typeof url !== 'string') throw new Error('Expected string URL');
      if (url === '/api/a/ai-tools') return lateA.promise;
      if (url === '/api/c/ai-tools') return lateC.promise;
      const owner = url.match(/\/api\/(\w)\//)?.[1] ?? 'x';
      if (url.includes('ai-tools')) return json([{ id: `${owner}-tool`, display_name: `${owner} tool`, models: [] }]);
      return json({ tool: `${owner}-tool` });
    });
    const first = state.ai.refreshAiConfiguration(project('a'));
    const third = state.ai.refreshAiConfiguration(project('c'));
    state.active.value = project('b');
    state.selectedProjectId.value = 'b';
    await state.ai.refreshAiConfiguration();
    lateA.resolve(json([{ id: 'a-tool', display_name: 'a tool', models: [] }]));
    await first;
    state.ai.forgetAiConfiguration('c');
    lateC.resolve(json([{ id: 'c-tool', display_name: 'c tool', models: [] }]));
    await third;
    expect(state.ai.aiDefaults.value.tool).toBe('b-tool');
    state.active.value = project('a');
    state.selectedProjectId.value = 'a';
    expect(state.ai.restoreAiConfiguration(project('a'))).toBe(true);
    expect(state.ai.aiDefaults.value.tool).toBe('a-tool');
    expect(state.ai.restoreAiConfiguration(project('c'))).toBe(false);
  });

  it('selects ranges, cancels a pending export destination, then opens a fresh replacement conversation', async () => {
    const state = chatOwners(),
      { archive } = state;
    archive.pickConversationMessage('chat-a', 'two');
    archive.pickConversationMessage('chat-a', 'one');
    expect(
      archive.conversationSelectedMessages('chat-a', state.conversationStates.value['chat-a'].messages),
    ).toHaveLength(2);
    archive.openConversationExport();
    expect(archive.conversationExportDialog.value).toMatchObject({
      step: 1,
      source: { projectId: 'a', conversationId: 'chat-a' },
    });
    archive.updateConversationExportDraft((draft) => ({ ...draft, scope: { kind: 'all' } }));
    const late = deferred<Response>();
    fetchMock.mockReturnValueOnce(late.promise);
    const choosing = archive.finishConversationExport();
    archive.conversationExportDialog.value = undefined;
    late.resolve(json({ destination: { selectionToken: 'chosen', displayPath: '/exports/chat', kind: 'directory' } }));
    await choosing;
    expect(archive.conversationExportDialog.value).toBeUndefined();
    expect(fetchMock).toHaveBeenCalledTimes(1);
    archive.clearConversationSelection('chat-a');
    expect(archive.conversationSelectedMessages('chat-a', state.conversationStates.value['chat-a'].messages)).toEqual(
      [],
    );
    state.active.value = project('b');
    state.conversationConnectionId.value = 'chat-b';
    state.conversationStates.value = { 'chat-b': { messages: [{ id: 'new', role: 'user', content: 'Fresh' }] } };
    archive.openConversationExport();
    expect(archive.conversationExportDialog.value).toMatchObject({
      step: 2,
      source: { projectId: 'b', conversationId: 'chat-b' },
    });
    expect(archive.conversationExportDialog.peek()?.messages.map((item) => item.id)).toEqual(['new']);
    archive.pickConversationMessage('chat-b', 'new');
    expect(
      archive.conversationSelectedMessages('chat-b', state.conversationStates.value['chat-b'].messages),
    ).toHaveLength(1);
  });
});

it('projects terminal/chat replacement, project switches and empty/refill without retaining render snapshots', () => {
  const state = chatOwners();
  const projects = signal([project('a'), project('b')]);
  const shellMode = signal<ProjectTabBarMode>('terminals');
  const groups = signal<TerminalDashboardGroup[]>([{ projectId: 'a', projectName: 'A', sessions: [] }]);
  const dimensions = signal({ width: 1000, height: 500 });
  const permissions = createPermissionsController({ projects, selectedProjectId: state.selectedProjectId });
  const magnified = signal<string | undefined>(undefined),
    mobileMagnified = signal<MobileMagnifiedTerminal | undefined>(undefined);
  const presentation = createTerminalPresentation({
    projects,
    project: state.project,
    shellMode,
    statsProjectId: signal<string | undefined>('b'),
    canGiveFeedback: () => false,
    ai: state.ai,
    permissions,
    terminals: {
      terminalGroups: groups,
      drawerTabOrder: () => ['ai-chat:chat-a'],
      terminalDashboardSize: dimensions,
      terminalFitAcross: signal(2),
      terminalFitHigh: signal(1),
      magnifiedTerminalKey: magnified,
      terminalHiddenKeys: () => ['hidden'],
      terminalDashboardLoading: signal(false),
      terminalDashboardMessage: signal(''),
      terminalContextMenu: signal<{ key: string; x: number; y: number } | undefined>(undefined),
      terminalDrawerBounds: dimensions,
      terminalDrawerFitAcross: signal(2),
      terminalDrawerFitHigh: signal(1),
      terminalDrawerSelected: signal('ai-chat:chat-a'),
      terminalDrawerMaximized: signal(false),
      mobileTerminalFocus: signal(INACTIVE_MOBILE_TERMINAL_FOCUS),
      mobileMagnifiedTerminal: () => mobileMagnified.value,
    },
    conversations: {
      conversationStates: state.conversationStates,
      driveConnectionsByProject: state.driveConnectionsByProject,
      terminalDrawerChatsByProject: state.terminalDrawerChatsByProject,
      conversationDrafts: signal<Record<string, string>>({}),
      conversationOpen: signal(true),
      conversationConnectionId: state.conversationConnectionId,
      conversationSelectedMessages: state.archive.conversationSelectedMessages,
    },
  });
  expect(presentation.workspaceTerminalGroups()[0].chats?.[0]).toMatchObject({
    tool: 'Codex',
    summary: 'Second',
    busy: false,
  });
  expect(presentation.projectTerminalDrawerProps()?.chatTabs).toHaveLength(1);
  expect(presentation.aiConversationSurface()).not.toBeNull();
  state.conversationStates.value = { 'chat-a': { messages: [], progress: 'Replacement' } };
  state.driveConnectionsByProject.value = { a: [{ ...state.driveConnectionsByProject.value.a[0], busy: true }] };
  expect(presentation.workspaceTerminalGroups()[0].chats?.[0]).toMatchObject({ summary: 'Replacement', busy: true });
  state.active.value = project('b');
  expect(presentation.projectTerminalDrawerProps()).toMatchObject({ projectId: 'b', chatTabs: [] });
  expect(presentation.aiConversationSurface()).toBeNull();
  groups.value = [];
  expect(presentation.workspaceTerminalGroups()).toEqual([]);
  shellMode.value = 'stats';
  expect(presentation.globalWorkspaceSurfaceProps()).toEqual({ kind: 'stats', projectName: 'b' });
  state.active.value = undefined;
  expect(presentation.projectTerminalDrawerProps()).toBeUndefined();
  state.active.value = project('a');
  groups.value = [{ projectId: 'a', projectName: 'Refilled', sessions: [] }];
  dimensions.value = { width: 390, height: 300 };
  shellMode.value = 'terminals';
  expect(presentation.globalWorkspaceSurfaceProps()).toMatchObject({
    kind: 'terminals',
    dashboard: { width: 390, height: 300, hiddenKeys: ['hidden'], mobileMagnified: undefined },
  });
  // HS2-WMN626: phone chrome is projected only while a terminal is magnified on a phone viewport.
  const phone = { viewport: { left: 0, top: 0, width: 390, height: 500 }, keyboardVisible: true, columns: 60 };
  mobileMagnified.value = phone;
  expect(presentation.globalWorkspaceSurfaceProps()).toMatchObject({ dashboard: { mobileMagnified: undefined } });
  magnified.value = 'a:t1';
  expect(presentation.globalWorkspaceSurfaceProps()).toMatchObject({
    dashboard: { magnifiedKey: 'a:t1', mobileMagnified: phone },
  });
  expect(presentation.projectTerminalDrawerProps()).toMatchObject({ magnifiedKey: 'a:t1', mobileMagnified: phone });
  mobileMagnified.value = undefined;
  expect(presentation.projectTerminalDrawerProps()?.mobileMagnified).toBeUndefined();
  expect(presentation.workspaceTerminalGroups()[0].chats?.[0].summary).toBe('Replacement');
});

it('isolates repeated LAN archive opens through read-only, failed resume and successful resume (HS2-76ZR5P)', async () => {
  vi.stubGlobal('crypto', { getRandomValues: crypto.getRandomValues.bind(crypto) });
  const state = chatOwners();
  let resume = false,
    fail = true;
  const ids: string[] = [];
  fetchMock.mockImplementation(async (url, init) => {
    if (url === '/__hotsheet/conversation-exports/open')
      return json({
        conversation: {
          displayPath: '/exports/saved',
          manifest: {
            format: 'hotsheet-conversation-export',
            manifestVersion: 1,
            exportId: 'saved-export',
            revision: 1,
            exportedAt: '2026-09-23T00:00:00Z',
            selectedMessageIds: ['saved-message'],
            bundle: { includeAttachments: false, includeMedia: false, includeSummary: false },
            entries: [],
            assets: [],
            source: { tool: 'codex', projectId: 'a', conversationId: 'original' },
            reopen: {
              conversationId: 'original',
              tool: 'codex',
              firstMessageId: 'saved-message',
              lastMessageId: 'saved-message',
              resumesOriginalSession: resume,
              sessionId: 'session',
            },
          },
          messages: [{ id: 'saved-message', role: 'assistant', content: 'Saved result' }],
          activity: [],
        } satisfies ConversationExportOpenResult,
      });
    const body = JSON.parse(init!.body as string);
    ids.push(body.connection_id);
    if (fail) return Response.json({ error: 'Resume unavailable' }, { status: 503 });
    return json({
      id: body.connection_id,
      tool: 'codex',
      project: '/work/a',
      role: 'main',
      busy: false,
      actions: ['send_turn'],
    } satisfies ToolConnection);
  });
  await state.archive.openSavedConversation();
  resume = true;
  await state.archive.openSavedConversation();
  fail = false;
  await state.archive.openSavedConversation();
  const tabs = state.terminalDrawerChatsByProject.value.a.slice(1);
  expect(new Set(tabs.map((tab) => tab.connectionId)).size).toBe(3);
  expect(tabs.map((tab) => [tab.readOnly, tab.localOnly])).toEqual([
    [true, true],
    [true, true],
    [false, false],
  ]);
  expect(ids).toEqual(tabs.slice(1).map((tab) => tab.connectionId));
  for (const tab of tabs)
    expect(state.conversationStates.value[tab.connectionId].messages[0].content).toBe('Saved result');
  expect(state.showToast).toHaveBeenCalledWith('Opened read-only; resume failed: Resume unavailable');
  expect(state.error.value).toBe('');
});
