import { describe, expect, it } from 'vitest';

import type { FullTicket } from './api';
import {
  isTicketConcurrencyConflict,
  rebaseDraftValue,
  reconcileActiveDraft,
  reconcileReaderNoteDraft,
  reconcileTicketPatch,
  ticketFieldText,
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
    expect(result).toEqual({ retry: { details: 'Mine' }, conflicts: [] });
  });

  it('does not resend a value that already converged remotely', () => {
    expect(reconcileTicketPatch(ticket(), ticket({ details: 'Mine' }), { details: 'Mine' })).toEqual({
      retry: {},
      conflicts: [],
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
    ).toEqual({ retry: { note: 'Mine', note_id: 'N1' }, conflicts: [] });
    expect(
      reconcileTicketPatch(ticket(), ticket({ notes: [{ ...ticket().notes[0], text: 'Theirs' }] }), {
        note_id: 'N1',
        note: 'Mine',
      }).conflicts[0],
    ).toMatchObject({ key: 'note:N1', mine: 'Mine', theirs: 'Theirs' });
  });

  it('keeps activity summary metadata attached to a retried note write', () => {
    expect(
      reconcileTicketPatch(ticket(), ticket({ status: 'completed' }), {
        note: 'Full detail',
        note_kind: 'activity',
        note_summary: 'Finished work',
      }),
    ).toEqual({ retry: { note: 'Full detail', note_kind: 'activity', note_summary: 'Finished work' }, conflicts: [] });
  });

  it('keeps note confidence attached to a retried note write and never treats it as a field (HS2-DWTJ43)', () => {
    expect(
      reconcileTicketPatch(ticket(), ticket({ status: 'completed' }), {
        note: 'Done',
        note_confidence: 0,
      }),
    ).toEqual({ retry: { note: 'Done', note_confidence: 0 }, conflicts: [] });
    expect(reconcileTicketPatch(ticket(), ticket(), { note_confidence: 82 })).toEqual({ retry: {}, conflicts: [] });
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
    });
    const note = { ...base.notes[0], text: 'Line one\nLine two' },
      noteRemote = ticket({ notes: [{ ...note, text: 'Line one\nLine two, theirs' }] });
    expect(
      reconcileTicketPatch(ticket({ notes: [note] }), noteRemote, { note_id: 'N1', note: 'Line one, mine\nLine two' }),
    ).toEqual({ retry: { note: 'Line one, mine\nLine two, theirs', note_id: 'N1' }, conflicts: [] });
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
    ).toEqual({ retry: { tags: ['two', 'remote', 'mine'] }, conflicts: [] });
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

  it('recognizes only the typed concurrency failure', () => {
    expect(isTicketConcurrencyConflict(new Error('ticket changed since it was read'))).toBe(true);
    expect(isTicketConcurrencyConflict(new Error('offline'))).toBe(false);
    expect(isTicketConcurrencyConflict('ticket changed since it was read')).toBe(false);
  });
});
