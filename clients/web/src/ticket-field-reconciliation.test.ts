import { describe, expect, it } from 'vitest';

import { ApiHttpError, type FullTicket } from './api';
import { beginFeedbackPromptEdit, endFeedbackPromptEdit } from './feedback-prompt-edits';
import {
  isTicketConcurrencyConflict,
  isWriteBehindUnavailable,
  mergedEditNotice,
  noteEditPatch,
  rebaseDraftValue,
  reconcileActiveDraft,
  reconcileReaderNoteDraft,
  reconcileTicketPatch,
  resolvedConflictPatch,
  TICKET_CHANGED_CODE,
  ticketFieldText,
  WRITE_BEHIND_UNAVAILABLE_CODE,
} from './ticket-field-reconciliation';

const ticket = (changes: Partial<FullTicket> = {}): FullTicket => ({
  connection_id: 'git-local',
  native_id: '01',
  qualified_id: 'git-local:01',
  id: '01',
  slug: 'HS2-TEST',
  title: 'Base title',
  details: 'Base details',
  category: 'task',
  priority: 'normal',
  status: 'started',
  up_next: true,
  feedback_needed: false,
  tags: ['one'],
  blocked_by: [],
  blocked_reason: '',
  claim_count: 0,
  notes: [
    {
      id: 'N1',
      kind: 'regular',
      created_at: '2026-09-02T00:00:00Z',
      edited_at: '2026-09-02T00:00:00Z',
      text: 'Base note',
    },
  ],
  attachments: [],
  concurrency_token: 'base',
  ...changes,
});

describe('field-aware ticket reconciliation', () => {
  it('retries local fields over a fresh token when only unrelated remote fields changed', () => {
    const result = reconcileTicketPatch(ticket(), ticket({ status: 'completed', concurrency_token: 'remote' }), {
      details: 'Mine',
    });
    expect(result).toEqual({ retry: { details: 'Mine' }, conflicts: [], merged: [] });
  });

  it('does not resend a value that already converged remotely', () => {
    expect(reconcileTicketPatch(ticket(), ticket({ details: 'Mine' }), { details: 'Mine' })).toEqual({
      retry: {},
      conflicts: [],
      merged: [],
    });
  });

  it('reports only divergent fields and retains independently retryable fields', () => {
    const result = reconcileTicketPatch(ticket(), ticket({ title: 'Theirs', status: 'completed' }), {
      title: 'Mine',
      details: 'Mine details',
    });
    expect(result.retry).toEqual({ details: 'Mine details' });
    expect(result.conflicts).toEqual([
      { key: 'title', field: 'title', label: 'Title', base: 'Base title', mine: 'Mine', theirs: 'Theirs' },
    ]);
  });

  it('reconciles note edits by note id rather than treating other notes as conflicts', () => {
    const extra = {
      id: 'N2',
      kind: 'regular' as const,
      created_at: '2026-09-02T01:00:00Z',
      edited_at: '2026-09-02T01:00:00Z',
      text: 'Remote extra',
    };
    expect(
      reconcileTicketPatch(ticket(), ticket({ notes: [...ticket().notes, extra] }), { note_id: 'N1', note: 'Mine' }),
    ).toEqual({ retry: { note: 'Mine', note_id: 'N1' }, conflicts: [], merged: [] });
    expect(
      reconcileTicketPatch(ticket(), ticket({ notes: [{ ...ticket().notes[0], text: 'Theirs' }] }), {
        note_id: 'N1',
        note: 'Mine',
      }).conflicts[0],
    ).toMatchObject({ key: 'note:N1', mine: 'Mine', theirs: 'Theirs' });
  });

  it('builds the patch that saves a conflict card choice to the card owner ticket (HS2-JT2AFG)', () => {
    const conflict = (field: string, key = field) => ({ key, field, label: field, base: '', mine: '', theirs: '' });
    expect(resolvedConflictPatch(conflict('details'), 'Merged', ticket())).toEqual({ details: 'Merged' });
    expect(resolvedConflictPatch(conflict('title'), 'Title', null)).toEqual({ title: 'Title' });
    expect(resolvedConflictPatch(conflict('blocked_reason'), '  ', ticket())).toEqual({ blocked_reason: null });
    expect(resolvedConflictPatch(conflict('blocked_reason'), ' Waiting ', ticket())).toEqual({
      blocked_reason: 'Waiting',
    });
    expect(resolvedConflictPatch(conflict('tags'), 'ui, , web ', ticket())).toEqual({ tags: ['ui', 'web'] });
    expect(resolvedConflictPatch(conflict('up_next'), 'True', ticket())).toEqual({ up_next: true });
    expect(resolvedConflictPatch(conflict('up_next'), 'false', ticket())).toEqual({ up_next: false });
    expect(resolvedConflictPatch(conflict('note', 'note:N1'), 'Mine', ticket())).toEqual({
      note_id: 'N1',
      note: 'Mine',
    });
    // An explicitly opened prompt keeps its flag only for the ticket that holds that prompt.
    const withPrompt = ticket({
      notes: [{ ...ticket().notes[0], id: 'P1', kind: 'feedback_needed', text: 'Which token?' }],
    });
    beginFeedbackPromptEdit('P1');
    try {
      expect(resolvedConflictPatch(conflict('note', 'note:P1'), 'Mine', withPrompt)).toEqual({
        note_id: 'P1',
        note: 'Mine',
        edit_feedback_prompt: true,
      });
      expect(resolvedConflictPatch(conflict('note', 'note:P1'), 'Mine', ticket())).toEqual({
        note_id: 'P1',
        note: 'Mine',
      });
    } finally {
      endFeedbackPromptEdit('P1');
    }
  });

  it('marks only an explicitly opened FEEDBACK NEEDED prompt edit and keeps the mark on retry (HS2-ZXGEFE)', () => {
    const withPrompt = ticket({
      notes: [{ ...ticket().notes[0], id: 'P1', kind: 'feedback_needed', text: 'Which token?' }, ...ticket().notes],
    });
    // A prompt that was not opened through Edit Note gets no flag, so the server keeps refusing it.
    expect(noteEditPatch(withPrompt, 'P1', 'Mine')).toEqual({ note_id: 'P1', note: 'Mine' });
    beginFeedbackPromptEdit('P1');
    try {
      expect(noteEditPatch(withPrompt, 'P1', 'Mine')).toEqual({
        note_id: 'P1',
        note: 'Mine',
        edit_feedback_prompt: true,
      });
      // Regular notes never carry the flag, even while a prompt edit is open.
      expect(noteEditPatch(withPrompt, 'N1', 'Mine')).toEqual({ note_id: 'N1', note: 'Mine' });
      const remote = ticket({
        notes: [...withPrompt.notes, { ...ticket().notes[0], id: 'N2', text: 'Remote extra' }],
      });
      expect(reconcileTicketPatch(withPrompt, remote, noteEditPatch(withPrompt, 'P1', 'Mine'))).toEqual({
        retry: { note: 'Mine', note_id: 'P1', edit_feedback_prompt: true },
        conflicts: [],
        merged: [],
      });
    } finally {
      endFeedbackPromptEdit('P1');
    }
    expect(noteEditPatch(withPrompt, 'P1', 'Mine')).toEqual({ note_id: 'P1', note: 'Mine' });
  });

  it('keeps activity summary metadata attached to a retried note write', () => {
    expect(
      reconcileTicketPatch(ticket(), ticket({ status: 'completed' }), {
        note: 'Full detail',
        note_kind: 'activity',
        note_summary: 'Finished work',
      }),
    ).toEqual({
      retry: { note: 'Full detail', note_kind: 'activity', note_summary: 'Finished work' },
      conflicts: [],
      merged: [],
    });
  });

  it('keeps note confidence attached to a retried note write and never treats it as a field (HS2-DWTJ43)', () => {
    expect(
      reconcileTicketPatch(ticket(), ticket({ status: 'completed' }), {
        note: 'Done',
        note_confidence: 0,
      }),
    ).toEqual({ retry: { note: 'Done', note_confidence: 0 }, conflicts: [], merged: [] });
    expect(reconcileTicketPatch(ticket(), ticket(), { note_confidence: 82 })).toEqual({
      retry: {},
      conflicts: [],
      merged: [],
    });
  });

  it('resends a note-metadata-only edit after unrelated token drift instead of dropping it (HS2-WZSFHK)', () => {
    const activity = { ...ticket().notes[0], kind: 'activity' as const, summary: 'Headline', confidence: 80 };
    const base = ticket({ notes: [activity] }),
      drifted = ticket({
        notes: [activity, { ...activity, id: 'N2', text: 'Unrelated AI note' }],
        status: 'completed',
      });
    expect(reconcileTicketPatch(base, drifted, { note_id: 'N1', note_summary: null })).toEqual({
      retry: { note_summary: null, note_id: 'N1' },
      conflicts: [],
      merged: [],
    });
    expect(reconcileTicketPatch(base, drifted, { note_id: 'N1', note_confidence: 65 })).toEqual({
      retry: { note_confidence: 65, note_id: 'N1' },
      conflicts: [],
      merged: [],
    });
    expect(reconcileTicketPatch(base, drifted, { note_id: 'N1', note_kind: 'feedback_needed' })).toEqual({
      retry: { note_kind: 'feedback_needed', note_id: 'N1' },
      conflicts: [],
      merged: [],
    });
    // Relinking a response stays scoped to the note instead of becoming a ticket-level field.
    expect(reconcileTicketPatch(base, drifted, { note_id: 'N1', response_to: 'N0' })).toEqual({
      retry: { response_to: 'N0', note_id: 'N1' },
      conflicts: [],
      merged: [],
    });
    expect(reconcileTicketPatch(base, drifted, { note_id: 'N1', response_to: null })).toEqual({
      retry: {},
      conflicts: [],
      merged: [],
    });
    // Already saved remotely (or cleared there too): nothing to resend.
    expect(reconcileTicketPatch(base, drifted, { note_id: 'N1', note_summary: 'Headline' })).toEqual({
      retry: {},
      conflicts: [],
      merged: [],
    });
    const cleared = ticket({ notes: [{ ...activity, summary: undefined }] });
    expect(reconcileTicketPatch(base, cleared, { note_id: 'N1', note_summary: null })).toEqual({
      retry: {},
      conflicts: [],
      merged: [],
    });
    // A confirmed dialog value wins over a concurrent change to the same note field.
    const theirs = ticket({ notes: [{ ...activity, summary: 'Their headline' }] });
    expect(reconcileTicketPatch(base, theirs, { note_id: 'N1', note_summary: 'Mine' })).toEqual({
      retry: { note_summary: 'Mine', note_id: 'N1' },
      conflicts: [],
      merged: [],
    });
  });

  it('covers active-draft interleavings without warning for remote-only or converged edits', () => {
    expect(reconcileActiveDraft('base', 'base', 'remote')).toEqual({
      kind: 'adopt-remote',
      base: 'remote',
      draft: 'remote',
    });
    expect(reconcileActiveDraft('base', 'mine', 'base')).toEqual({ kind: 'unchanged', base: 'base', draft: 'mine' });
    expect(reconcileActiveDraft('base', 'same', 'same')).toEqual({ kind: 'converged', base: 'same', draft: 'same' });
    expect(reconcileActiveDraft('base', 'mine', 'theirs')).toEqual({ kind: 'conflict', base: 'theirs', draft: 'mine' });
    expect(reconcileActiveDraft('', '', '')).toEqual({ kind: 'unchanged', base: '', draft: '' });
  });

  it('keeps a feedback reply through repeated refreshes while still detecting real reader note edits', () => {
    const request = { ...ticket().notes[0], kind: 'feedback_needed' as const, text: 'Please review this phase' },
      before = ticket({ notes: [request] }),
      refreshed = ticket({ notes: [{ ...request }] });
    expect(reconcileReaderNoteDraft(before, refreshed, 'N1', '', 'I checked it')).toBeUndefined();
    expect(reconcileReaderNoteDraft(refreshed, refreshed, 'N1', '', 'I checked it')).toBeUndefined();
    expect(
      reconcileReaderNoteDraft(
        before,
        ticket({ notes: [{ ...request, text: 'Please review the new phase' }] }),
        'N1',
        '',
        'I checked it',
      ),
    ).toBeUndefined();
    const edit = ticket({ notes: [{ ...request, kind: 'feedback_draft', text: 'Original draft' }] });
    expect(
      reconcileReaderNoteDraft(
        edit,
        ticket({ notes: [{ ...edit.notes[0], text: 'Other edit' }] }),
        'N1',
        'Original draft',
        'My edit',
      ),
    ).toEqual({ kind: 'conflict', base: 'Other edit', draft: 'My edit' });
  });

  it('merges disjoint concurrent edits of the same text field instead of conflicting (HS2-A4XCXE)', () => {
    const base = ticket({ details: 'Intro\nMiddle\nEnd' }),
      remote = ticket({ details: 'Intro\nMiddle\nEnd\nAI appended', concurrency_token: 'remote' });
    expect(reconcileTicketPatch(base, remote, { details: 'Intro edited\nMiddle\nEnd' })).toEqual({
      retry: { details: 'Intro edited\nMiddle\nEnd\nAI appended' },
      conflicts: [],
      merged: ['details'],
    });
    const note = { ...base.notes[0], text: 'Line one\nLine two' },
      noteRemote = ticket({ notes: [{ ...note, text: 'Line one\nLine two, theirs' }] });
    expect(
      reconcileTicketPatch(ticket({ notes: [note] }), noteRemote, { note_id: 'N1', note: 'Line one, mine\nLine two' }),
    ).toEqual({ retry: { note: 'Line one, mine\nLine two, theirs', note_id: 'N1' }, conflicts: [], merged: ['note'] });
  });

  it('names only fields whose retry combines both edits (HS2-JJ3R1G)', () => {
    const base = ticket({ details: 'a\nb', title: 'Fix the login bug', tags: ['one'] });
    // Token drift, a converged value, and an edit the remote already adopted are not merges to announce.
    expect(reconcileTicketPatch(base, ticket({ priority: 'high' }), { details: 'A\nb' }).merged).toEqual([]);
    expect(reconcileTicketPatch(base, ticket({ details: 'A\nb' }), { details: 'A\nb' }).merged).toEqual([]);
    expect(reconcileTicketPatch(base, ticket({ details: 'A\nb\nc' }), { details: 'A\nb' })).toEqual({
      retry: {},
      conflicts: [],
      merged: [],
    });
    // Words of a single-line title and set-like tags combine; an overlap stays a conflict, not a merge.
    const result = reconcileTicketPatch(
      base,
      ticket({ details: 'a\nb', title: 'Fix the login crash', tags: ['one', 'theirs'] }),
      { title: 'Fix the signup bug', tags: ['one', 'mine'] },
    );
    expect(result.retry).toEqual({ title: 'Fix the signup crash', tags: ['one', 'theirs', 'mine'] });
    expect(result.merged).toEqual(['title', 'tags']);
    const overlap = reconcileTicketPatch(base, ticket({ details: 'a\nB theirs' }), { details: 'a\nB mine' });
    expect(overlap.merged).toEqual([]);
    expect(overlap.conflicts).toHaveLength(1);
  });

  it('words the merged-edit toast for one field, several fields, and a restore (HS2-JJ3R1G)', () => {
    expect(mergedEditNotice(['details'])).toBe('Merged your edit with a newer change to Details.');
    expect(mergedEditNotice(['note', 'note'])).toBe('Merged your edit with a newer change to Note.');
    expect(mergedEditNotice(['title', 'tags'])).toBe('Merged your edit with newer changes.');
    expect(mergedEditNotice(['blocked_reason'], { restored: true })).toBe(
      'Restored an unsaved edit and merged it with a newer change to Blocked Reason.',
    );
  });

  it('still reports overlapping edits of the same lines as a conflict', () => {
    const result = reconcileTicketPatch(ticket({ details: 'Shared line' }), ticket({ details: 'Their line' }), {
      details: 'My line',
    });
    expect(result.retry).toEqual({});
    expect(result.conflicts).toMatchObject([{ key: 'details', mine: 'My line', theirs: 'Their line' }]);
  });

  it('merges concurrent tag additions and removals and keeps an empty blocked reason null', () => {
    expect(
      reconcileTicketPatch(ticket({ tags: ['one', 'two'] }), ticket({ tags: ['one', 'two', 'remote'] }), {
        tags: ['two', 'mine'],
      }),
    ).toEqual({ retry: { tags: ['two', 'remote', 'mine'] }, conflicts: [], merged: ['tags'] });
    expect(
      reconcileTicketPatch(
        ticket({ blocked_reason: 'Waiting\non review' }),
        ticket({ blocked_reason: 'Waiting\non review\nand CI' }),
        { blocked_reason: 'Waiting' },
      ).conflicts,
    ).toEqual([]);
  });

  it('merges an idle draft with a disjoint remote edit and asks only on overlap', () => {
    expect(reconcileActiveDraft('a\nb\nc', 'A\nb\nc', 'a\nb\nc\nd')).toEqual({
      kind: 'merged',
      base: 'a\nb\nc\nd',
      draft: 'A\nb\nc\nd',
      mine: 'A\nb\nc',
    });
    expect(reconcileActiveDraft('a\nb', 'a\nmine', 'a\ntheirs').kind).toBe('conflict');
  });

  it('rebases a draft typed on an older value onto the current ticket value', () => {
    expect(rebaseDraftValue('same', 'mine', 'same')).toEqual({ kind: 'unchanged' });
    expect(rebaseDraftValue('old', 'mine', 'mine')).toEqual({ kind: 'unchanged' });
    expect(rebaseDraftValue('a\nb', 'A\nb', 'a\nb\nc')).toEqual({ kind: 'merged', value: 'A\nb\nc' });
    expect(rebaseDraftValue('a', 'mine', 'theirs')).toEqual({ kind: 'conflict' });
    expect(ticketFieldText(ticket(), 'note', 'N1')).toBe('Base note');
    expect(ticketFieldText(ticket({ blocked_reason: null as unknown as string }), 'blocked_reason')).toBe('');
  });

  it('falls back from durable queued edits only on the typed write-behind refusal (HS2-DEVEYE)', () => {
    const text = 'durable queued edits are unavailable for this provider connection';
    expect(isWriteBehindUnavailable(new ApiHttpError(text, 409, WRITE_BEHIND_UNAVAILABLE_CODE))).toBe(true);
    // Reworded text with the code still falls back; the text alone or another status never does.
    expect(isWriteBehindUnavailable(new ApiHttpError('queue off', 409, WRITE_BEHIND_UNAVAILABLE_CODE))).toBe(true);
    expect(isWriteBehindUnavailable(new ApiHttpError(text, 409))).toBe(false);
    expect(isWriteBehindUnavailable(new ApiHttpError('source unavailable', 409))).toBe(false);
    expect(isWriteBehindUnavailable(new ApiHttpError(text, 503, WRITE_BEHIND_UNAVAILABLE_CODE))).toBe(false);
    expect(isWriteBehindUnavailable(new ApiHttpError(text, 409, TICKET_CHANGED_CODE))).toBe(false);
    expect(isWriteBehindUnavailable(new Error(text))).toBe(false);
    expect(isWriteBehindUnavailable(text)).toBe(false);
    // The two typed 409s stay disjoint: a write-behind refusal never enters the concurrency retry.
    expect(isTicketConcurrencyConflict(new ApiHttpError(text, 409, WRITE_BEHIND_UNAVAILABLE_CODE))).toBe(false);
  });

  it('recognizes only the typed concurrency failure', () => {
    // HS2-9S8CV1: the real Markdown checkout message names the ticket; providers say "issue".
    for (const message of [
      "ticket 'HS2-ABC123' changed since it was read",
      'ticket 01ABC changed since it was read',
      "provider conflict for 'github:7': issue changed since it was read",
      'ticket changed since it was read',
    ])
      expect(isTicketConcurrencyConflict(new ApiHttpError(message, 409, TICKET_CHANGED_CODE))).toBe(true);
    // The text alone, another status, or another 409 never enters the retry.
    expect(isTicketConcurrencyConflict(new ApiHttpError('ticket changed since it was read', 409))).toBe(false);
    expect(isTicketConcurrencyConflict(new ApiHttpError('changed', 412, TICKET_CHANGED_CODE))).toBe(false);
    expect(
      isTicketConcurrencyConflict(
        new ApiHttpError('durable queued edits are unavailable for this provider connection', 409),
      ),
    ).toBe(false);
    expect(isTicketConcurrencyConflict(new ApiHttpError('checkout links an unhosted git source', 409))).toBe(false);
    expect(isTicketConcurrencyConflict(new Error('ticket changed since it was read'))).toBe(false);
    expect(isTicketConcurrencyConflict(new Error('offline'))).toBe(false);
    expect(isTicketConcurrencyConflict('ticket changed since it was read')).toBe(false);
  });
});
