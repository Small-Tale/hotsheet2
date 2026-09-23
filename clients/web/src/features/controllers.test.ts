import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ConversationState } from '../ai-conversation';
import { Api, type CommandDefinition, type FullTicket, type RepositoryFile, type ToolConnection } from '../api';
import type { ProjectTabBarMode } from '../components/project-tab-bar';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { ConversationExportOpenResult } from '../conversation-export';
import type { Project } from '../interactions/types';
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
    terminalDrawerCreateMenuOpen: signal(false),
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
      magnifiedTerminalKey: signal<string | undefined>(undefined),
      terminalHiddenKeys: () => ['hidden'],
      terminalDashboardLoading: signal(false),
      terminalDashboardMessage: signal(''),
      terminalContextMenu: signal<{ key: string; x: number; y: number } | undefined>(undefined),
      terminalDrawerBounds: dimensions,
      terminalDrawerFitAcross: signal(2),
      terminalDrawerFitHigh: signal(1),
      terminalDrawerSelected: signal('ai-chat:chat-a'),
      terminalDrawerMaximized: signal(false),
      terminalDrawerCreateMenuOpen: signal(false),
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
    dashboard: { width: 390, height: 300, hiddenKeys: ['hidden'] },
  });
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
