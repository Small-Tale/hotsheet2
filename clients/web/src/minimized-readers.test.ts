import { describe, expect, it } from 'vitest';

import type { Capabilities, FullTicket, Note } from './api';
import {
  addMinimizedReader,
  disposeMinimizedReaders,
  loadMinimizedReaders,
  MINIMIZED_DRAFT_LABELS,
  MINIMIZED_READERS_STORAGE_KEY,
  minimizedDraftLabel,
  type MinimizedReader,
  minimizedReaderDrafts,
  minimizedReadersForTab,
  minimizedReadersHaveDrafts,
  parseMinimizedReaders,
  restoredReaderEditState,
  saveMinimizedReaders,
  serializeMinimizedReaders,
  takeMinimizedReader,
  takeMinimizedReadersForTab,
} from './minimized-readers';
import { ticketReaderEditState, type TicketReaderFrame } from './ticket-reader-stack';

const request: Note = {
  id: 'note-request',
  kind: 'feedback_needed',
  text: 'Which icon?',
  created_at: '2026-11-20T00:00:00Z',
  edited_at: '2026-11-20T00:00:00Z',
};

const ticket = (id: string, notes: Note[] = []): FullTicket => ({
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
  notes,
  attachments: [],
});

const frame = (id: string, projectId = 'alpha'): TicketReaderFrame => ({
  id: `frame-${id}`,
  open: true,
  projectId,
  projectName: projectId,
  apiPath: `/api/${projectId}`,
  ticket: ticket(id),
  activeTab: 'info',
  capabilities: { update: true, notes: true, note_edit: true, note_delete: true } as Capabilities,
  edit: ticketReaderEditState(ticket(id)),
});

const entry = (
  id: string,
  tabProjectId = 'alpha',
  drafts = minimizedReaderDrafts(frame(id).edit),
): MinimizedReader => ({
  id: `frame-${id}`,
  tabProjectId,
  projectId: tabProjectId,
  qualifiedId: `git-local:${id}`,
  slug: `HS2-${id.toUpperCase()}`,
  title: `Synthetic ${id}`,
  category: 'feature',
  activeTab: 'info',
  minimizedAt: 1,
  drafts,
  frame: { ...frame(id, tabProjectId), open: false },
});

function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
    removeItem: (key: string) => {
      values.delete(key);
    },
  };
}

describe('minimized reader drafts and indicators (HS2-Q8WF64)', () => {
  it('reports no draft for a resting reader, so Close All closes without asking', () => {
    const drafts = minimizedReaderDrafts(frame('a').edit);
    expect(drafts).toEqual({
      composingNote: false,
      newNoteDraft: '',
      inlineFeedbackReplies: {},
      feedbackChoiceSelections: {},
      unsavedFields: [],
    });
    expect(minimizedDraftLabel(drafts)).toBeUndefined();
    expect(minimizedReadersHaveDrafts([entry('a')])).toBe(false);
  });

  it('labels each kind of draft, preferring an unsaved edit, then an unsent note, then a reply', () => {
    const edit = frame('a').edit;
    const composer = minimizedReaderDrafts({ ...edit, composingNote: true, newNoteDraft: 'Half a note' });
    expect(minimizedDraftLabel(composer)).toBe(MINIMIZED_DRAFT_LABELS.unsentNote);
    // An open but empty composer loses nothing.
    expect(minimizedDraftLabel(minimizedReaderDrafts({ ...edit, composingNote: true, newNoteDraft: '  ' }))).toBe(
      undefined,
    );
    const reply = minimizedReaderDrafts(edit, { request, draft: 'Use layers' });
    expect(minimizedDraftLabel(reply)).toBe(MINIMIZED_DRAFT_LABELS.reply);
    const inline = minimizedReaderDrafts({
      ...edit,
      inlineFeedbackReplies: { [request.id]: [{ offset: 3, text: 'x' }] },
    });
    expect(minimizedDraftLabel(inline)).toBe(MINIMIZED_DRAFT_LABELS.reply);
    const choices = minimizedReaderDrafts({ ...edit, feedbackChoiceSelections: { [request.id]: ['C'] } });
    expect(minimizedDraftLabel(choices)).toBe(MINIMIZED_DRAFT_LABELS.reply);
    const unsaved = minimizedReaderDrafts(
      { ...edit, detailsMode: 'write', detailsDraft: 'Changed', composingNote: true, newNoteDraft: 'Note' },
      { request, draft: 'Reply' },
    );
    expect(unsaved.unsavedFields).toEqual(['details']);
    expect(minimizedDraftLabel(unsaved)).toBe(MINIMIZED_DRAFT_LABELS.unsavedEdit);
  });

  it('records unsaved note and blocked-reason edits but not a note response, which the registry owns', () => {
    const edit = frame('a').edit;
    expect(
      minimizedReaderDrafts({
        ...edit,
        editingNoteId: 'n1',
        noteDraft: 'edited',
        noteBase: 'original',
        blockedReasonEditing: true,
        blockedReasonDraft: 'Waiting on CI',
      }).unsavedFields,
    ).toEqual(['note', 'blocked_reason']);
    expect(
      minimizedReaderDrafts({ ...edit, editingNoteId: request.id, noteDraft: 'reply' }, { request, draft: 'reply' })
        .unsavedFields,
    ).toEqual([]);
    // A details editor whose text matches what was saved is open, but holds no draft.
    expect(minimizedReaderDrafts({ ...edit, detailsMode: 'write' }).unsavedFields).toEqual([]);
  });

  it('drops empty reply and choice records so a minimized list stays small', () => {
    const drafts = minimizedReaderDrafts({
      ...frame('a').edit,
      inlineFeedbackReplies: { gone: [], kept: [{ offset: 0, text: 'Yes' }] },
      feedbackChoiceSelections: { gone: [], kept: ['A'] },
    });
    expect(drafts.inlineFeedbackReplies).toEqual({ kept: [{ offset: 0, text: 'Yes' }] });
    expect(drafts.feedbackChoiceSelections).toEqual({ kept: ['A'] });
  });
});

describe('minimized reader list transitions (HS2-Q8WF64)', () => {
  it('minimizes several tickets per tab and lists only the current tab, oldest first', () => {
    let list = addMinimizedReader([], entry('a', 'alpha'));
    list = addMinimizedReader(list, entry('b', 'beta'));
    list = addMinimizedReader(list, entry('c', 'alpha'));
    expect(minimizedReadersForTab(list, 'alpha').map((item) => item.id)).toEqual(['frame-a', 'frame-c']);
    expect(minimizedReadersForTab(list, 'beta').map((item) => item.id)).toEqual(['frame-b']);
    expect(minimizedReadersForTab(list, 'gamma')).toEqual([]);
  });

  it('re-minimizing the same frame replaces its record and moves it to the end', () => {
    let list = addMinimizedReader([], entry('a'));
    list = addMinimizedReader(list, entry('b'));
    list = addMinimizedReader(list, { ...entry('a'), title: 'Renamed' });
    expect(list.map((item) => [item.id, item.title])).toEqual([
      ['frame-b', 'Synthetic b'],
      ['frame-a', 'Renamed'],
    ]);
  });

  it('restores one, then the rest, and an unknown or repeated restore changes nothing', () => {
    const list = [entry('a'), entry('b'), entry('c')];
    const first = takeMinimizedReader(list, 'frame-b');
    expect(first.entry?.id).toBe('frame-b');
    expect(first.list.map((item) => item.id)).toEqual(['frame-a', 'frame-c']);
    const again = takeMinimizedReader(first.list, 'frame-b');
    expect(again.entry).toBeUndefined();
    expect(again.list).toEqual(first.list);
    const all = takeMinimizedReadersForTab(first.list, 'alpha');
    expect(all.entries.map((item) => item.id)).toEqual(['frame-a', 'frame-c']);
    expect(all.list).toEqual([]);
    // Empty, then refilled.
    expect(addMinimizedReader(all.list, entry('d')).map((item) => item.id)).toEqual(['frame-d']);
  });

  it('restores all of one tab while another tab keeps its minimized tickets', () => {
    const list = [entry('a', 'alpha'), entry('b', 'beta'), entry('c', 'alpha')];
    const taken = takeMinimizedReadersForTab(list, 'alpha');
    expect(taken.entries.map((item) => item.id)).toEqual(['frame-a', 'frame-c']);
    expect(taken.list.map((item) => item.id)).toEqual(['frame-b']);
  });

  it('closing a project tab disposes the tab’s minimized tickets and readers of its tickets', () => {
    const crossProject = { ...entry('x', 'alpha'), projectId: 'beta' };
    const result = disposeMinimizedReaders([entry('a', 'alpha'), entry('b', 'beta'), crossProject], new Set(['beta']));
    expect(result.retained.map((item) => item.id)).toEqual(['frame-a']);
    expect(result.disposed.map((item) => item.id)).toEqual(['frame-b', 'frame-x']);
  });

  it('asks before Close All only when some minimized ticket has a draft', () => {
    const drafted = entry(
      'b',
      'alpha',
      minimizedReaderDrafts({ ...frame('b').edit, composingNote: true, newNoteDraft: 'x' }),
    );
    expect(minimizedReadersHaveDrafts([entry('a'), entry('c')])).toBe(false);
    expect(minimizedReadersHaveDrafts([entry('a'), drafted])).toBe(true);
    expect(minimizedReadersHaveDrafts([])).toBe(false);
  });
});

describe('minimized readers across a reload (HS2-Q8WF64)', () => {
  it('stores records without the live frame and reads them back', () => {
    const drafts = minimizedReaderDrafts(
      {
        ...frame('a').edit,
        composingNote: true,
        newNoteDraft: 'Unsent',
        inlineFeedbackReplies: { [request.id]: [{ offset: 2, text: 'inline' }] },
        feedbackChoiceSelections: { [request.id]: ['C'] },
      },
      { request, draft: 'Reply text' },
    );
    const stored = serializeMinimizedReaders([entry('a', 'alpha', drafts)]);
    expect(stored).not.toContain('"frame"');
    expect(stored).not.toContain('concurrency_token');
    const [read] = parseMinimizedReaders(stored);
    expect(read).toEqual({ ...entry('a', 'alpha', drafts), frame: undefined });
    expect(minimizedDraftLabel(read.drafts)).toBe(MINIMIZED_DRAFT_LABELS.unsentNote);
  });

  it('round-trips through storage and clears the key when the list empties', () => {
    const storage = memoryStorage();
    saveMinimizedReaders(storage, [entry('a'), entry('b', 'beta')]);
    expect(loadMinimizedReaders(storage).map((item) => [item.id, item.tabProjectId])).toEqual([
      ['frame-a', 'alpha'],
      ['frame-b', 'beta'],
    ]);
    saveMinimizedReaders(storage, []);
    expect(storage.values.has(MINIMIZED_READERS_STORAGE_KEY)).toBe(false);
    expect(loadMinimizedReaders(storage)).toEqual([]);
    expect(loadMinimizedReaders(undefined)).toEqual([]);
  });

  it('survives unavailable storage on both read and write', () => {
    const throwing = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('quota');
      },
      removeItem: () => {
        throw new Error('blocked');
      },
    };
    expect(loadMinimizedReaders(throwing)).toEqual([]);
    expect(() => {
      saveMinimizedReaders(throwing, [entry('a')]);
    }).not.toThrow();
  });

  it('drops malformed and duplicate records and normalizes unknown fields', () => {
    const raw = JSON.stringify([
      null,
      { id: 'no-tab' },
      {
        id: 'frame-a',
        tabProjectId: 'alpha',
        projectId: 'alpha',
        qualifiedId: 'git-local:a',
        slug: 'HS2-A',
        activeTab: 'nonsense',
        drafts: {
          composingNote: 'yes',
          newNoteDraft: 'ignored without a composer',
          inlineFeedbackReplies: {
            n: [
              { offset: 'x', text: 1 },
              { offset: 1, text: 'ok' },
            ],
            bad: 'x',
          },
          feedbackChoiceSelections: { n: ['A', 2] },
          response: { request: { id: 'r', kind: 'regular', text: 't' }, draft: 'd' },
          unsavedFields: ['details', 'title'],
        },
      },
      { id: 'frame-a', tabProjectId: 'alpha', projectId: 'alpha', qualifiedId: 'q', slug: 'DUP' },
    ]);
    const list = parseMinimizedReaders(raw);
    expect(list).toHaveLength(1);
    expect(list[0]).toMatchObject({
      id: 'frame-a',
      title: '',
      category: 'issue',
      activeTab: 'info',
      minimizedAt: 0,
      drafts: {
        composingNote: false,
        newNoteDraft: '',
        inlineFeedbackReplies: { n: [{ offset: 1, text: 'ok' }], bad: [] },
        feedbackChoiceSelections: { n: ['A'] },
        unsavedFields: ['details'],
      },
    });
    expect(list[0].drafts.response).toBeUndefined();
    expect(parseMinimizedReaders('not json')).toEqual([]);
    expect(parseMinimizedReaders('{}')).toEqual([]);
    expect(parseMinimizedReaders(null)).toEqual([]);
  });

  it('rebuilds a reloaded reader’s composer, replies, and response over the current ticket', () => {
    const current = ticket('a', [request]);
    const edit = restoredReaderEditState(current, {
      composingNote: true,
      newNoteDraft: 'Unsent',
      inlineFeedbackReplies: { [request.id]: [{ offset: 1, text: 'r' }], deleted: [{ offset: 0, text: 'gone' }] },
      feedbackChoiceSelections: { [request.id]: ['C'], 'ticket-details': ['A'], deleted: ['B'] },
      response: { request, draft: 'Reply text' },
      unsavedFields: [],
    });
    expect(edit).toMatchObject({
      composingNote: true,
      newNoteDraft: 'Unsent',
      inlineFeedbackReplies: { [request.id]: [{ offset: 1, text: 'r' }] },
      feedbackChoiceSelections: { [request.id]: ['C'], 'ticket-details': ['A'] },
      editingNoteId: request.id,
      responseRequestId: request.id,
      noteDraft: 'Reply text',
      detailsMode: 'preview',
      detailsDraft: 'Original details',
    });
    const resting = restoredReaderEditState(current, minimizedReaderDrafts(frame('a').edit));
    expect(resting).toEqual(ticketReaderEditState(current));
  });
});
