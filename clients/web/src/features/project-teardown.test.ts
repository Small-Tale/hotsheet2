import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { Project } from '../interactions/types';
import { createProjectTeardown, nextProjectAfterClose, pruneProjectRecords } from './project-teardown';

describe('pruneProjectRecords (HS2-DCTS07)', () => {
  it('drops only the closing ids from every record and keeps the rest intact', () => {
    const rows = signal<Record<string, number[]>>({ a: [1], b: [2], c: [3] }),
      flags = signal<Record<string, boolean>>({ b: true, c: false });
    pruneProjectRecords([rows, flags], new Set(['b']));
    expect(rows.value).toEqual({ a: [1], c: [3] });
    expect(flags.value).toEqual({ c: false });
  });

  it('replaces each record with a new object even when nothing closes, and handles empty records', () => {
    const before = { a: 1 },
      record = signal<Record<string, number>>(before),
      empty = signal<Record<string, number>>({});
    pruneProjectRecords([record, empty], new Set());
    expect(record.value).toEqual(before);
    expect(record.value).not.toBe(before);
    expect(empty.value).toEqual({});
  });
});

describe('nextProjectAfterClose (HS2-DCTS07)', () => {
  const tabs = ['a', 'b', 'c', 'd'].map((id) => ({ id }));
  const after = (...closing: string[]) => tabs.filter((tab) => !closing.includes(tab.id));

  it('prefers the nearest surviving tab to the right of the closed selection', () => {
    expect(nextProjectAfterClose(tabs, after('b'), 1)).toBe('c');
    expect(nextProjectAfterClose(tabs, after('b', 'c'), 1)).toBe('d');
  });

  it('falls back to the nearest tab on the left when nothing survives to the right', () => {
    expect(nextProjectAfterClose(tabs, after('d'), 3)).toBe('c');
    expect(nextProjectAfterClose(tabs, after('c', 'd'), 2)).toBe('b');
  });

  it('uses the first remaining tab when the selection was not found, and none when all close', () => {
    expect(nextProjectAfterClose(tabs, after('a'), -1)).toBe('b');
    expect(nextProjectAfterClose(tabs, [], 0)).toBe('');
  });
});

describe('createProjectTeardown (HS2-BSSWGV)', () => {
  const tab = (id: string) => ({ id, apiPath: `/api/${id}` }) as Project;
  const cancelable = () => ({ cancel: vi.fn() });

  function teardown(selected: string) {
    const projects = signal([tab('a'), tab('b'), tab('c')]),
      selectedProjectId = signal(selected),
      rows = signal<Record<string, unknown>>({ a: 1, b: 2, c: 3 }),
      providers = signal<Record<string, unknown>>({ a: 1, b: 2, c: 3 }),
      readerOpen = signal(true),
      readerDetailsAutosave = cancelable(),
      activation = { project: tab('c'), generation: 1, cached: true },
      calls = {
        activateOpenProject: vi.fn((id: string) => {
          selectedProjectId.value = id;
          return activation;
        }),
        refreshActivatedProject: vi.fn(async () => undefined),
        refreshProject: vi.fn(async () => undefined),
        refreshCommands: vi.fn(async () => undefined),
        refreshCustomViews: vi.fn(async () => undefined),
        refreshDriveConnections: vi.fn(async () => undefined),
        syncProjectChangeStreams: vi.fn(),
        resetTicketComposer: vi.fn(),
        forgetAiConfiguration: vi.fn(),
      };
    const owner = createProjectTeardown({
      ...calls,
      projects,
      selectedProjectId,
      statsProjectId: signal<string | undefined>('b'),
      projectRestoreFailures: signal([]),
      selectedProjectRestoreRoot: signal(''),
      terminalDrawerVisible: signal(false),
      commandDialogId: signal<string | undefined>('cmd'),
      commandSettingsEditingId: signal<string | undefined>(undefined),
      commandDefinitions: signal([]),
      commandRuns: signal([]),
      readerOpen,
      linkedReaderStack: signal([]),
      projectRecords: [rows],
      connectionRecords: [providers],
      histories: new Map([['b', {}]]),
      linkedReaderAutosaves: new Map(),
      readerReturnFocus: new Map(),
      readerClosing: new Set(),
      readerApprovedClose: new Set(),
      readerDetailsAutosave,
      readerNoteAutosave: cancelable(),
      readerBlockedReasonAutosave: cancelable(),
      projectTabRefresh: { cancel: vi.fn() },
      pendingCreatedTickets: { forgetProject: vi.fn() },
      projectsPendingActivation: new Set(['b']),
      warmProjects: { forget: vi.fn() },
      project: () => projects.value.find((item) => item.id === selectedProjectId.value),
      currentRememberedProjectRoots: () => ['/work/a', '/work/c'],
    });
    return {
      owner,
      projects,
      selectedProjectId,
      rows,
      providers,
      readerOpen,
      readerDetailsAutosave,
      calls,
      activation,
    };
  }

  beforeEach(() => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(new Response('{}', { status: 200 }))),
    );
    vi.stubGlobal('localStorage', { setItem: vi.fn() });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('closing the selected tab disposes its reader, prunes records, and activates the tab to its right', () => {
    const {
      owner,
      projects,
      selectedProjectId,
      rows,
      providers,
      readerOpen,
      readerDetailsAutosave,
      calls,
      activation,
    } = teardown('b');
    owner.closeProjectIds(['b']);
    expect(projects.value.map((item) => item.id)).toEqual(['a', 'c']);
    expect(rows.value).toEqual({ a: 1, c: 3 });
    expect(providers.value).toEqual({ a: 1, c: 3 });
    expect(readerOpen.value).toBe(false);
    expect(readerDetailsAutosave.cancel).toHaveBeenCalled();
    expect(calls.resetTicketComposer).toHaveBeenCalled();
    expect(calls.forgetAiConfiguration).toHaveBeenCalledWith('b');
    expect(selectedProjectId.value).toBe('c');
    expect(calls.refreshActivatedProject).toHaveBeenCalledWith(activation, false);
    expect(calls.refreshProject).not.toHaveBeenCalled();
    expect(calls.syncProjectChangeStreams).toHaveBeenCalled();
  });

  it('closing a background tab keeps the selection and refreshes the selected project', () => {
    const { owner, selectedProjectId, readerOpen, calls } = teardown('a');
    owner.closeProjectIds(['b']);
    expect(selectedProjectId.value).toBe('a');
    expect(readerOpen.value).toBe(true);
    expect(calls.activateOpenProject).not.toHaveBeenCalled();
    expect(calls.refreshProject).toHaveBeenCalled();
    expect(calls.refreshCommands).toHaveBeenCalled();
  });
});
