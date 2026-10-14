import { describe, expect, it } from 'vitest';

import type { Capabilities, FullTicket } from './api';
import {
  activeTicketReaderProject,
  adoptCommittedReaderDrafts,
  disposeTicketReaderFrames,
  linkedReaderCanUpdate,
  popTicketReaderFrame,
  pushTicketReaderFrame,
  reconcileTicketReaderFrame,
  ticketEditTarget,
  ticketReaderEditState,
  type TicketReaderFrame,
} from './ticket-reader-stack';

const ticket = (id: string, slug: string): FullTicket => ({
  id,
  native_id: id,
  connection_id: `git-${id}`,
  qualified_id: `git-${id}:${id}`,
  slug,
  title: slug,
  category: 'bug',
  priority: 'default',
  status: 'started',
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-09-11T00:00:00Z',
  updated_at: '2026-09-11T00:00:00Z',
  details: '',
  concurrency_token: `token-${id}`,
  notes: [],
  attachments: [],
});

const frame = (id: string, projectId: string, slug: string): TicketReaderFrame => ({
  id,
  open: true,
  projectId,
  projectName: projectId,
  apiPath: `/api/${projectId}`,
  ticket: ticket(id, slug),
  activeTab: 'info',
  capabilities: { update: true, notes: true, note_edit: true, note_delete: true } as Capabilities,
  edit: ticketReaderEditState(ticket(id, slug)),
});

describe('layered ticket reader stack', () => {
  it('preserves exact same-slug frames across projects and unwinds in LIFO order', () => {
    const first = frame('one', 'alpha', 'HS2-SAME01');
    const second = frame('two', 'beta', 'HS2-SAME01');
    const stacked = pushTicketReaderFrame(pushTicketReaderFrame([], first), second);
    expect(stacked.map((item) => [item.projectId, item.ticket.qualified_id])).toEqual([
      ['alpha', 'git-one:one'],
      ['beta', 'git-two:two'],
    ]);
    expect(activeTicketReaderProject(stacked, 'workspace')).toBe('beta');
    const popped = popTicketReaderFrame(stacked);
    expect(popped.closed).toBe(second);
    expect(popped.stack).toEqual([first]);
    expect(activeTicketReaderProject(popped.stack, 'workspace')).toBe('alpha');
    expect(activeTicketReaderProject([], 'workspace')).toBe('workspace');
  });

  it('reconciles remote values without replacing independent dirty drafts', () => {
    const current = frame('one', 'alpha', 'HS2-SAME01');
    current.edit = {
      ...current.edit,
      detailsMode: 'write',
      detailsDraft: 'local details',
      blockedReasonEditing: true,
      blockedReasonDraft: 'local reason',
      editingNoteId: 'note-1',
      noteBase: 'old note',
      noteDraft: 'local note',
    };
    const remote = {
      ...current.ticket,
      details: 'remote details',
      blocked_reason: 'remote reason',
      notes: [
        {
          id: 'note-1',
          kind: 'regular' as const,
          created_at: '2026-09-11T00:00:00Z',
          edited_at: '2026-09-11T00:00:00Z',
          text: 'remote note',
        },
      ],
    };
    const reconciled = reconcileTicketReaderFrame(current, remote);
    expect(reconciled.ticket).toBe(remote);
    expect(reconciled.edit).toMatchObject({
      detailsDraft: 'local details',
      detailsBase: 'remote details',
      blockedReasonDraft: 'local reason',
      blockedReasonBase: 'remote reason',
      noteDraft: 'local note',
      noteBase: 'remote note',
    });
  });

  it('adopts committed merged text only into drafts still holding what was sent (HS2-A4XCXE)', () => {
    const current = frame('one', 'alpha', 'HS2-SAME01');
    current.edit = {
      ...current.edit,
      detailsMode: 'write',
      detailsDraft: 'mine',
      blockedReasonEditing: true,
      blockedReasonDraft: 'typed further',
      editingNoteId: 'note-1',
      noteDraft: 'my note',
    };
    const committed = {
      ...current.ticket,
      details: 'mine\nmerged remote line',
      blocked_reason: 'merged reason',
      notes: [
        {
          id: 'note-1',
          kind: 'regular' as const,
          created_at: '2026-09-11T00:00:00Z',
          edited_at: '2026-09-11T00:00:00Z',
          text: 'my note\nmerged',
        },
      ],
    };
    const adopted = adoptCommittedReaderDrafts(
      current,
      { details: 'mine', blocked_reason: 'sent reason', note_id: 'note-1', note: 'my note' },
      committed,
    );
    expect(adopted.edit).toMatchObject({
      detailsDraft: 'mine\nmerged remote line',
      detailsBase: 'mine\nmerged remote line',
      // The user kept typing after this save was sent, so that draft is left for its own save to rebase.
      blockedReasonDraft: 'typed further',
      noteDraft: 'my note\nmerged',
      noteBase: 'my note\nmerged',
    });
    expect(current.edit.detailsDraft).toBe('mine');
  });

  it('does not mutate the caller-owned stack while pushing or popping', () => {
    const original = [frame('one', 'alpha', 'HS2-ONE01')];
    const pushed = pushTicketReaderFrame(original, frame('two', 'alpha', 'HS2-TWO02'));
    const popped = popTicketReaderFrame(pushed);
    expect(original).toHaveLength(1);
    expect(pushed).toHaveLength(2);
    expect(popped.stack).not.toBe(pushed);
  });

  it('disposes every frame owned by a closing project without disturbing the others', () => {
    const alpha = frame('one', 'alpha', 'HS2-ONE01'),
      beta = frame('two', 'beta', 'HS2-TWO02'),
      again = frame('three', 'alpha', 'HS2-THREE3');
    const result = disposeTicketReaderFrames([alpha, beta, again], new Set(['alpha']));
    expect(result.retained).toEqual([beta]);
    expect(result.disposed).toEqual([alpha, again]);
  });

  it('routes field edits to the top linked reader with its own provider capability (HS2-KRVWKF)', () => {
    const selected = ticket('workspace', 'KF-ROOT01'),
      first = frame('one', 'alpha', 'HS2-LINK01'),
      second = frame('two', 'beta', 'HS2-LINK02');
    expect(ticketEditTarget([], selected, () => true)).toEqual({ ticket: selected, canUpdate: true });
    expect(ticketEditTarget([], selected, () => false).canUpdate).toBe(false);
    expect(ticketEditTarget([], null, () => true)).toEqual({ ticket: null, canUpdate: false });
    // A stacked reader wins even when the workspace selection is read-only, and the deepest frame owns it.
    const stacked = pushTicketReaderFrame(pushTicketReaderFrame([], first), second);
    expect(ticketEditTarget(stacked, selected, () => false)).toEqual({
      ticket: second.ticket,
      canUpdate: true,
      frame: second,
    });
    const readOnlyProvider = { ...second, capabilities: { ...second.capabilities, update: false } };
    expect(ticketEditTarget([first, readOnlyProvider], selected, () => true).canUpdate).toBe(false);
    // Unwinding hands editing back to the remaining frame, then to the workspace selection.
    const popped = popTicketReaderFrame(stacked).stack;
    expect(ticketEditTarget(popped, selected, () => true).ticket).toBe(first.ticket);
    expect(ticketEditTarget(popTicketReaderFrame(popped).stack, selected, () => true).ticket).toBe(selected);
  });

  it('offers linked reader field editing only on the active layer of a writable provider (HS2-KRVWKF)', () => {
    const linked = frame('one', 'alpha', 'HS2-LINK01');
    expect(linkedReaderCanUpdate(linked, true)).toBe(true);
    expect(linkedReaderCanUpdate(linked, false)).toBe(false);
    expect(linkedReaderCanUpdate({ ...linked, capabilities: { ...linked.capabilities, update: false } }, true)).toBe(
      false,
    );
  });
});
