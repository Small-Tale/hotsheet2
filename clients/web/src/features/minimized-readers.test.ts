import { signal } from 'kerfjs';
import { describe, expect, it, vi } from 'vitest';

import type { Capabilities, FullTicket, Note } from '../api';
import type { InspectorTab } from '../components/ticket-inspector';
import { createFeedbackResponseDrafts } from '../feedback-response-drafts';
import type { Project } from '../interactions/types';
import {
  MINIMIZED_DRAFT_LABELS,
  MINIMIZED_READERS_STORAGE_KEY,
  type MinimizedReader,
  parseMinimizedReaders,
} from '../minimized-readers';
import { ticketDraftKey } from '../ticket-draft-store';
import { type TicketReaderEditState, ticketReaderEditState, type TicketReaderFrame } from '../ticket-reader-stack';
import { createMinimizedReaders, WORKSPACE_READER_ID } from './minimized-readers';

const request: Note = {
  id: 'note-request',
  kind: 'feedback_needed',
  text: 'Which icon?',
  created_at: '2026-11-20T00:00:00Z',
  edited_at: '2026-11-20T00:00:00Z',
};

const ticket = (id: string, extra: Partial<FullTicket> = {}): FullTicket => ({
  id,
  native_id: id,
  connection_id: 'git-local',
  qualified_id: `git-local:${id}`,
  slug: `HS2-${id.toUpperCase()}`,
  title: `Synthetic ${id}`,
  category: 'feature',
  priority: 'default',
  status: 'started',
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-11-20T00:00:00Z',
  updated_at: '2026-11-20T00:00:00Z',
  details: 'Original details',
  concurrency_token: `token-${id}`,
  notes: [request],
  attachments: [],
  ...extra,
});

const capabilities = { update: true, notes: true, note_edit: true, note_delete: true } as Capabilities;
const project = (id: string): Project => ({ id, name: id, apiPath: `/api/${id}` }) as Project;

const linkedFrame = (id: string, projectId = 'alpha', edit?: Partial<TicketReaderEditState>): TicketReaderFrame => ({
  id: `frame-${id}`,
  open: true,
  projectId,
  projectName: projectId,
  apiPath: `/api/${projectId}`,
  ticket: ticket(id),
  activeTab: 'timeline',
  capabilities,
  edit: { ...ticketReaderEditState(ticket(id)), ...edit },
});

function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    values,
    get length() {
      return values.size;
    },
    clear: () => {
      values.clear();
    },
    key: (index: number) => [...values.keys()][index] ?? null,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
    removeItem: (key: string) => {
      values.delete(key);
    },
  } satisfies Storage & { values: Map<string, string> };
}

function harness(
  options: {
    stack?: TicketReaderFrame[];
    minimized?: MinimizedReader[];
    workspace?: Partial<TicketReaderEditState>;
    readerOpen?: boolean;
    confirm?: boolean;
    fetch?: (owner: Project, qualifiedId: string) => Promise<FullTicket>;
    flushOk?: boolean;
  } = {},
) {
  const storage = memoryStorage();
  const minimizedReaders = signal<MinimizedReader[]>(options.minimized ?? []),
    linkedReaderStack = signal<TicketReaderFrame[]>(options.stack ?? []),
    selectedProjectId = signal('alpha'),
    projects = signal<Project[]>([project('alpha'), project('beta')]),
    selectedTicket = signal<FullTicket | null>(ticket('w')),
    readerOpen = signal(options.readerOpen ?? false),
    readerTab = signal<InspectorTab>('attachments'),
    feedbackResponseDrafts = createFeedbackResponseDrafts();
  const cancels: string[] = [];
  const autosaves = new Map(
    (options.stack ?? []).map((frame) => [
      frame.id,
      {
        details: { cancel: () => cancels.push(`${frame.id}:details`) },
        note: { cancel: () => cancels.push(`${frame.id}:note`) },
        blocked: { cancel: () => cancels.push(`${frame.id}:blocked`) },
      },
    ]),
  );
  let workspaceEdit: TicketReaderEditState = { ...ticketReaderEditState(ticket('w')), ...options.workspace };
  const calls = {
    reset: 0,
    flushedInputs: 0,
    flushedWorkspace: 0,
    flushedLinked: [] as string[],
    presented: [] as string[],
    toasts: [] as string[],
    confirms: 0,
    menus: 0,
    minimizeMotion: 0,
    restoreMotion: 0,
    released: 0,
  };
  let nextId = 0;
  const fetchTicket = vi.fn(
    options.fetch ??
      ((_owner: Project, qualifiedId: string) =>
        Promise.resolve(ticket(qualifiedId.split(':')[1], { title: 'Fresh title' }))),
  );
  const controller = createMinimizedReaders({
    minimizedReaders,
    linkedReaderStack,
    linkedReaderAutosaves: autosaves,
    selectedProjectId,
    projects,
    project: () => projects.value.find((item) => item.id === selectedProjectId.value),
    selectedTicket,
    readerOpen,
    readerTab,
    workspaceReaderEdit: () => workspaceEdit,
    resetWorkspaceReader: () => {
      calls.reset += 1;
      workspaceEdit = ticketReaderEditState(ticket('w'));
    },
    flushDraftInputs: () => {
      calls.flushedInputs += 1;
    },
    flushWorkspaceReader: () => {
      calls.flushedWorkspace += 1;
      return Promise.resolve(options.flushOk ?? true);
    },
    flushLinkedReader: (id) => {
      calls.flushedLinked.push(id);
      return Promise.resolve(options.flushOk ?? true);
    },
    replaceLinkedReaderFrame: (id, update) => {
      linkedReaderStack.value = linkedReaderStack.value.map((frame) => (frame.id === id ? update(frame) : frame));
    },
    feedbackResponseDrafts,
    capabilitiesFor: (connectionId) => (connectionId === 'git-local' ? capabilities : undefined),
    fetchTicket,
    presentTicketReaderDialog: (id, _trigger, onOpen) => {
      calls.presented.push(id);
      onOpen();
    },
    readerDialog: () => null,
    confirm: () => {
      calls.confirms += 1;
      return Promise.resolve(options.confirm ?? true);
    },
    showToast: (message) => calls.toasts.push(message),
    storage: () => storage,
    newId: () => `new-${++nextId}`,
    now: () => 42,
    openMinimizedMenu: () => {
      calls.menus += 1;
    },
    animate: {
      minimize: () => {
        calls.minimizeMotion += 1;
        return Promise.resolve();
      },
      restore: () => {
        calls.restoreMotion += 1;
        return Promise.resolve(() => {
          calls.released += 1;
        });
      },
    },
    afterOpen: (run) => {
      run();
    },
  });
  const stored = () => parseMinimizedReaders(storage.values.get(MINIMIZED_READERS_STORAGE_KEY));
  return {
    controller,
    minimizedReaders,
    linkedReaderStack,
    selectedProjectId,
    readerOpen,
    feedbackResponseDrafts,
    storage,
    stored,
    calls,
    cancels,
    autosaves,
    fetchTicket,
  };
}

const flushPromises = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('minimizing a linked reader (HS2-Q8WF64)', () => {
  it('commits pending saves, keeps the frame and drafts, leaves the modal stack, and persists the record', async () => {
    const below = linkedFrame('a'),
      top = linkedFrame('b', 'alpha', { composingNote: true, newNoteDraft: 'Half a note' });
    const h = harness({ stack: [below, top] });
    expect(await h.controller.minimize(top.id)).toBe(true);
    expect(h.calls.flushedInputs).toBe(1);
    expect(h.calls.flushedLinked).toEqual([top.id]);
    expect(h.linkedReaderStack.value.map((frame) => frame.id)).toEqual([below.id]);
    expect(h.cancels).toEqual([`${top.id}:details`, `${top.id}:note`, `${top.id}:blocked`]);
    expect(h.autosaves.has(top.id)).toBe(false);
    const [minimized] = h.minimizedReaders.value;
    expect(minimized).toMatchObject({
      id: top.id,
      tabProjectId: 'alpha',
      slug: 'HS2-B',
      activeTab: 'timeline',
      minimizedAt: 42,
      drafts: { composingNote: true, newNoteDraft: 'Half a note' },
    });
    expect(minimized.frame).toEqual({ ...top, open: false });
    expect(h.stored().map((item) => item.id)).toEqual([top.id]);
    expect(h.controller.summaries()).toEqual([
      {
        id: top.id,
        slug: 'HS2-B',
        title: 'Synthetic b',
        category: 'feature',
        draft: MINIMIZED_DRAFT_LABELS.unsentNote,
      },
    ]);
    expect(h.calls.minimizeMotion).toBe(1);
  });

  it('keeps a field edit a failed save could not commit, in the frame and the ticket draft store', async () => {
    const top = linkedFrame('b', 'alpha', { detailsMode: 'write', detailsDraft: 'Offline edit' });
    const h = harness({ stack: [top], flushOk: false });
    expect(await h.controller.minimize(top.id)).toBe(true);
    expect(h.minimizedReaders.value[0].drafts.unsavedFields).toEqual(['details']);
    expect(h.controller.summaries()[0].draft).toBe(MINIMIZED_DRAFT_LABELS.unsavedEdit);
    expect(JSON.parse(h.storage.values.get(ticketDraftKey('alpha', 'git-local:b', 'details'))!)).toMatchObject({
      base: 'Original details',
      draft: 'Offline edit',
    });
  });

  it('ignores a repeated minimize while the first is still saving, and an unknown frame', async () => {
    const top = linkedFrame('b');
    const h = harness({ stack: [top] });
    const first = h.controller.minimize(top.id),
      second = h.controller.minimize(top.id);
    expect(await second).toBe(false);
    expect(await first).toBe(true);
    expect(h.minimizedReaders.value).toHaveLength(1);
    expect(await h.controller.minimize('missing')).toBe(false);
    expect(h.minimizedReaders.value).toHaveLength(1);
  });
});

describe('minimizing the workspace reader (HS2-Q8WF64)', () => {
  it('detaches it into a frame that owns its composer, replies, and response, then resets the workspace', async () => {
    const h = harness({
      readerOpen: true,
      workspace: {
        composingNote: true,
        newNoteDraft: 'Composer text',
        inlineFeedbackReplies: { [request.id]: [{ offset: 1, text: 'inline' }] },
        feedbackChoiceSelections: { [request.id]: ['C'] },
      },
    });
    const workspaceScope = { projectId: 'alpha', qualifiedId: 'git-local:w', surface: WORKSPACE_READER_ID };
    h.feedbackResponseDrafts.begin(workspaceScope, request, 'Reply in progress');
    expect(await h.controller.minimize(WORKSPACE_READER_ID)).toBe(true);
    expect(h.calls.flushedWorkspace).toBe(1);
    expect(h.calls.reset).toBe(1);
    expect(h.readerOpen.value).toBe(false);
    const [minimized] = h.minimizedReaders.value;
    expect(minimized.id).toBe('new-1');
    expect(minimized.frame).toMatchObject({
      id: 'new-1',
      open: false,
      projectId: 'alpha',
      activeTab: 'attachments',
      edit: {
        composingNote: true,
        newNoteDraft: 'Composer text',
        editingNoteId: request.id,
        responseRequestId: request.id,
        noteDraft: 'Reply in progress',
        feedbackChoiceSelections: { [request.id]: ['C'] },
      },
    });
    expect(minimized.drafts.response?.draft).toBe('Reply in progress');
    // The response moved from the workspace surface to the frame's own surface.
    expect(h.feedbackResponseDrafts.get(workspaceScope)).toBeUndefined();
    expect(h.feedbackResponseDrafts.get({ ...workspaceScope, surface: 'new-1' })?.draft).toBe('Reply in progress');
  });

  it('does nothing when the workspace reader is not open', async () => {
    const h = harness({ readerOpen: false });
    expect(await h.controller.minimize(WORKSPACE_READER_ID)).toBe(false);
    expect(h.minimizedReaders.value).toEqual([]);
    expect(h.calls.reset).toBe(0);
  });
});

describe('restoring minimized readers (HS2-Q8WF64)', () => {
  it('pushes a live frame back on top, opens it modal again, and refreshes its ticket keeping its drafts', async () => {
    const below = linkedFrame('a'),
      top = linkedFrame('b', 'alpha', { composingNote: true, newNoteDraft: 'Keep me' });
    const h = harness({ stack: [below, top] });
    await h.controller.minimize(top.id);
    expect(await h.controller.restore(top.id)).toBe(true);
    expect(h.minimizedReaders.value).toEqual([]);
    expect(h.stored()).toEqual([]);
    expect(h.linkedReaderStack.value.map((frame) => [frame.id, frame.open])).toEqual([
      [below.id, true],
      [top.id, true],
    ]);
    expect(h.calls.presented).toEqual([top.id]);
    expect(h.calls.restoreMotion).toBe(1);
    expect(h.calls.released).toBe(1);
    await flushPromises();
    const restored = h.linkedReaderStack.value.at(-1)!;
    expect(restored.ticket.title).toBe('Fresh title');
    expect(restored.edit.newNoteDraft).toBe('Keep me');
  });

  it('rebuilds a reloaded record from the ticket with its composer and response', async () => {
    const stored = parseMinimizedReaders(
      JSON.stringify([
        {
          id: 'frame-r',
          tabProjectId: 'alpha',
          projectId: 'beta',
          qualifiedId: 'git-local:r',
          slug: 'HS2-R',
          title: 'Stored title',
          category: 'bug',
          activeTab: 'timeline',
          minimizedAt: 1,
          drafts: {
            composingNote: true,
            newNoteDraft: 'Composer survives',
            response: { request, draft: 'Reply survives' },
          },
        },
      ]),
    );
    const h = harness({ minimized: stored });
    expect(h.controller.summaries()[0]).toMatchObject({ slug: 'HS2-R', draft: MINIMIZED_DRAFT_LABELS.unsentNote });
    expect(await h.controller.restore('frame-r')).toBe(true);
    expect(h.fetchTicket).toHaveBeenCalledWith(project('beta'), 'git-local:r');
    const frame = h.linkedReaderStack.value.at(-1)!;
    expect(frame).toMatchObject({
      id: 'frame-r',
      open: true,
      projectId: 'beta',
      activeTab: 'timeline',
      edit: { composingNote: true, newNoteDraft: 'Composer survives', noteDraft: 'Reply survives' },
    });
    expect(
      h.feedbackResponseDrafts.get({ projectId: 'beta', qualifiedId: 'git-local:r', surface: 'frame-r' })?.draft,
    ).toBe('Reply survives');
    // A rebuilt frame came from the server just now, so it is not fetched again.
    await flushPromises();
    expect(h.fetchTicket).toHaveBeenCalledTimes(1);
  });

  it('keeps a reloaded record minimized when its project is closed or the ticket cannot load', async () => {
    const record = (projectId: string): MinimizedReader => ({
      id: `frame-${projectId}`,
      tabProjectId: 'alpha',
      projectId,
      qualifiedId: 'git-local:z',
      slug: 'HS2-Z',
      title: 'Z',
      category: 'task',
      activeTab: 'info',
      minimizedAt: 1,
      drafts: {
        composingNote: false,
        newNoteDraft: '',
        inlineFeedbackReplies: {},
        feedbackChoiceSelections: {},
        unsavedFields: [],
      },
    });
    const h = harness({
      minimized: [record('gamma'), record('beta')],
      fetch: () => Promise.reject(new Error('offline')),
    });
    expect(await h.controller.restore('frame-gamma')).toBe(false);
    expect(await h.controller.restore('frame-beta')).toBe(false);
    expect(h.calls.toasts).toEqual([
      'Open the project that owns HS2-Z to restore it.',
      'Could not restore HS2-Z: offline',
    ]);
    expect(h.minimizedReaders.value.map((item) => item.id)).toEqual(['frame-gamma', 'frame-beta']);
    expect(h.linkedReaderStack.value).toEqual([]);
  });

  it('restores all of the current tab oldest first, leaving the newest on top and another tab alone', async () => {
    const a = linkedFrame('a'),
      b = linkedFrame('b'),
      c = linkedFrame('c');
    const h = harness({ stack: [a, b, c] });
    await h.controller.minimize(c.id);
    await h.controller.minimize(b.id);
    h.selectedProjectId.value = 'beta';
    await h.controller.minimize(a.id);
    expect(h.controller.summaries().map((item) => item.id)).toEqual([a.id]);
    h.selectedProjectId.value = 'alpha';
    expect(h.controller.summaries().map((item) => item.id)).toEqual([c.id, b.id]);
    expect(await h.controller.restoreAll()).toBe(true);
    expect(h.linkedReaderStack.value.map((frame) => [frame.id, frame.open])).toEqual([
      [c.id, true],
      [b.id, true],
    ]);
    expect(h.calls.presented).toEqual([b.id]);
    expect(h.minimizedReaders.value.map((item) => [item.id, item.tabProjectId])).toEqual([[a.id, 'beta']]);
    expect(await h.controller.restoreAll()).toBe(false);
  });
});

describe('closing all minimized readers (HS2-Q8WF64)', () => {
  it('closes at once when nothing would be lost', async () => {
    const h = harness({ stack: [linkedFrame('a'), linkedFrame('b')] });
    await h.controller.minimize('frame-b');
    await h.controller.minimize('frame-a');
    expect(await h.controller.closeAll()).toBe(true);
    expect(h.calls.confirms).toBe(0);
    expect(h.minimizedReaders.value).toEqual([]);
    expect(h.stored()).toEqual([]);
    expect(await h.controller.closeAll()).toBe(false);
  });

  it('asks first when a draft would be lost, keeps everything on Cancel, and discards on confirm', async () => {
    const drafted = linkedFrame('b', 'alpha', { detailsMode: 'write', detailsDraft: 'Unsaved' });
    const cancelled = harness({ stack: [linkedFrame('a'), drafted], confirm: false, flushOk: false });
    await cancelled.controller.minimize('frame-b');
    await cancelled.controller.minimize('frame-a');
    expect(await cancelled.controller.closeAll()).toBe(false);
    expect(cancelled.calls.confirms).toBe(1);
    expect(cancelled.minimizedReaders.value).toHaveLength(2);

    const confirmed = harness({ stack: [linkedFrame('a'), drafted], confirm: true, flushOk: false });
    await confirmed.controller.minimize('frame-b');
    const key = ticketDraftKey('alpha', 'git-local:b', 'details');
    expect(confirmed.storage.values.has(key)).toBe(true);
    expect(await confirmed.controller.closeAll()).toBe(true);
    expect(confirmed.calls.confirms).toBe(1);
    expect(confirmed.minimizedReaders.value).toEqual([]);
    expect(confirmed.storage.values.has(key)).toBe(false);
  });

  it('closes only the current tab and drops a closed tab’s minimized readers with their responses', async () => {
    const h = harness({ stack: [linkedFrame('a', 'alpha'), linkedFrame('b', 'beta')] });
    await h.controller.minimize('frame-a');
    h.feedbackResponseDrafts.begin(
      { projectId: 'beta', qualifiedId: 'git-local:b', surface: 'frame-b' },
      request,
      'Reply',
    );
    h.selectedProjectId.value = 'beta';
    await h.controller.minimize('frame-b');
    h.controller.disposeProjects(['beta']);
    expect(h.minimizedReaders.value.map((item) => item.id)).toEqual(['frame-a']);
    expect(h.feedbackResponseDrafts.get({ projectId: 'beta', qualifiedId: 'git-local:b', surface: 'frame-b' })).toBe(
      undefined,
    );
    expect(h.stored().map((item) => item.id)).toEqual(['frame-a']);
    h.controller.disposeProjects(['gamma']);
    expect(h.minimizedReaders.value).toHaveLength(1);
    expect(h.controller.retainedResponseScopes()).toEqual([
      { projectId: 'alpha', qualifiedId: 'git-local:a', surface: 'frame-a' },
    ]);
  });
});

describe('the minimize keyboard command (HS2-Q8WF64)', () => {
  it('minimizes the active reader, restores the only minimized ticket, and opens the list for several', async () => {
    const h = harness({ stack: [linkedFrame('a'), linkedFrame('b')] });
    h.controller.toggle(undefined);
    expect(h.calls.menus).toBe(0);
    h.controller.toggle('frame-b');
    await flushPromises();
    expect(h.minimizedReaders.value.map((item) => item.id)).toEqual(['frame-b']);
    h.controller.toggle(undefined);
    await flushPromises();
    expect(h.minimizedReaders.value).toEqual([]);
    expect(h.linkedReaderStack.value.at(-1)?.id).toBe('frame-b');
    h.controller.toggle('frame-b');
    await flushPromises();
    h.controller.toggle('frame-a');
    await flushPromises();
    h.controller.toggle(undefined);
    expect(h.calls.menus).toBe(1);
    expect(h.minimizedReaders.value).toHaveLength(2);
  });
});
