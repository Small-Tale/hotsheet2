import { describe, expect, it } from 'vitest';

import type { Capabilities, FullTicket } from './api';
import { activeTicketReaderProject, popTicketReaderFrame, pushTicketReaderFrame, reconcileTicketReaderFrame, ticketReaderEditState, type TicketReaderFrame } from './ticket-reader-stack';

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
  projectId,
  projectName: projectId,
  apiPath: `/api/${projectId}`,
  ticket: ticket(id, slug),
  activeTab: 'info',
  capabilities: {update:true,notes:true,note_edit:true,note_delete:true} as Capabilities,
  edit: ticketReaderEditState(ticket(id,slug)),
});

describe('layered ticket reader stack', () => {
  it('preserves exact same-slug frames across projects and unwinds in LIFO order', () => {
    const first = frame('one', 'alpha', 'HS2-SAME01');
    const second = frame('two', 'beta', 'HS2-SAME01');
    const stacked = pushTicketReaderFrame(pushTicketReaderFrame([], first), second);
    expect(stacked.map(item => [item.projectId, item.ticket.qualified_id])).toEqual([
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
    const current=frame('one','alpha','HS2-SAME01');
    current.edit={...current.edit,detailsMode:'write',detailsDraft:'local details',blockedReasonEditing:true,blockedReasonDraft:'local reason',editingNoteId:'note-1',noteBase:'old note',noteDraft:'local note'};
    const remote={...current.ticket,details:'remote details',blocked_reason:'remote reason',notes:[{id:'note-1',kind:'regular' as const,created_at:'2026-09-11T00:00:00Z',edited_at:'2026-09-11T00:00:00Z',text:'remote note'}]};
    const reconciled=reconcileTicketReaderFrame(current,remote);
    expect(reconciled.ticket).toBe(remote);
    expect(reconciled.edit).toMatchObject({detailsDraft:'local details',detailsBase:'remote details',blockedReasonDraft:'local reason',blockedReasonBase:'remote reason',noteDraft:'local note',noteBase:'remote note'});
  });

  it('does not mutate the caller-owned stack while pushing or popping', () => {
    const original = [frame('one', 'alpha', 'HS2-ONE01')];
    const pushed = pushTicketReaderFrame(original, frame('two', 'alpha', 'HS2-TWO02'));
    const popped = popTicketReaderFrame(pushed);
    expect(original).toHaveLength(1);
    expect(pushed).toHaveLength(2);
    expect(popped.stack).not.toBe(pushed);
  });
});
