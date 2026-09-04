import { describe, expect, it } from 'vitest';

import type { FullTicket } from './api';
import { isTicketConcurrencyConflict, reconcileActiveDraft, reconcileTicketPatch } from './ticket-field-reconciliation';

const ticket = (changes: Partial<FullTicket> = {}): FullTicket => ({
  connection_id: 'git-local', native_id: '01', qualified_id: 'git-local:01', id: '01', slug: 'HS2-TEST', title: 'Base title',
  details: 'Base details', category: 'task', priority: 'normal', status: 'started', up_next: true, feedback_needed: false,
  tags: ['one'], blocked_by: [], blocked_reason: '', claim_count: 0, notes: [{ id: 'N1', kind: 'regular', created_at: '2026-09-02T00:00:00Z', edited_at: '2026-09-02T00:00:00Z', text: 'Base note' }], attachments: [], concurrency_token: 'base',
  ...changes,
});

describe('field-aware ticket reconciliation', () => {
  it('retries local fields over a fresh token when only unrelated remote fields changed', () => {
    const result = reconcileTicketPatch(ticket(), ticket({ status: 'completed', concurrency_token: 'remote' }), { details: 'Mine' });
    expect(result).toEqual({ retry: { details: 'Mine' }, conflicts: [] });
  });

  it('does not resend a value that already converged remotely', () => {
    expect(reconcileTicketPatch(ticket(), ticket({ details: 'Mine' }), { details: 'Mine' })).toEqual({ retry: {}, conflicts: [] });
  });

  it('reports only divergent fields and retains independently retryable fields', () => {
    const result = reconcileTicketPatch(ticket(), ticket({ title: 'Theirs', status: 'completed' }), { title: 'Mine', details: 'Mine details' });
    expect(result.retry).toEqual({ details: 'Mine details' });
    expect(result.conflicts).toEqual([{ key: 'title', field: 'title', label: 'Title', base: 'Base title', mine: 'Mine', theirs: 'Theirs' }]);
  });

  it('reconciles note edits by note id rather than treating other notes as conflicts', () => {
    const extra = { id: 'N2', kind: 'regular' as const, created_at: '2026-09-02T01:00:00Z', edited_at: '2026-09-02T01:00:00Z', text: 'Remote extra' };
    expect(reconcileTicketPatch(ticket(), ticket({ notes: [...ticket().notes, extra] }), { note_id: 'N1', note: 'Mine' })).toEqual({ retry: { note: 'Mine', note_id: 'N1' }, conflicts: [] });
    expect(reconcileTicketPatch(ticket(), ticket({ notes: [{ ...ticket().notes[0], text: 'Theirs' }] }), { note_id: 'N1', note: 'Mine' }).conflicts[0]).toMatchObject({ key: 'note:N1', mine: 'Mine', theirs: 'Theirs' });
  });

  it('keeps activity summary metadata attached to a retried note write', () => {
    expect(reconcileTicketPatch(ticket(),ticket({status:'completed'}),{note:'Full detail',note_kind:'activity',note_summary:'Finished work'})).toEqual({retry:{note:'Full detail',note_kind:'activity',note_summary:'Finished work'},conflicts:[]});
  });

  it('covers active-draft interleavings without warning for remote-only or converged edits', () => {
    expect(reconcileActiveDraft('base', 'base', 'remote')).toEqual({ kind: 'adopt-remote', base: 'remote', draft: 'remote' });
    expect(reconcileActiveDraft('base', 'mine', 'base')).toEqual({ kind: 'unchanged', base: 'base', draft: 'mine' });
    expect(reconcileActiveDraft('base', 'same', 'same')).toEqual({ kind: 'converged', base: 'same', draft: 'same' });
    expect(reconcileActiveDraft('base', 'mine', 'theirs')).toEqual({ kind: 'conflict', base: 'theirs', draft: 'mine' });
    expect(reconcileActiveDraft('', '', '')).toEqual({ kind: 'unchanged', base: '', draft: '' });
  });

  it('recognizes only the typed concurrency failure', () => {
    expect(isTicketConcurrencyConflict(new Error('ticket changed since it was read'))).toBe(true);
    expect(isTicketConcurrencyConflict(new Error('offline'))).toBe(false);
    expect(isTicketConcurrencyConflict('ticket changed since it was read')).toBe(false);
  });
});
