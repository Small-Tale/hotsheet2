import { describe, expect, it, vi } from 'vitest';

import type { Capabilities, FullTicket } from '../api';
import type { Project } from '../interactions/types';
import { ticketReaderEditState, type TicketReaderFrame } from '../ticket-reader-stack';
import { createLinkedReaderSubjects, readerScopeOf, WORKSPACE_READER_SCOPE } from './linked-reader-subject';

const ticket = (native: string, extra: Partial<FullTicket> = {}) =>
  ({
    slug: `HS2-${native.toUpperCase()}`,
    title: `Synthetic ${native}`,
    details: '',
    notes: [],
    blocked_by: [],
    connection_id: 'git',
    native_id: native,
    qualified_id: `git:${native}`,
    ...extra,
  }) as unknown as FullTicket;
const frame = (id: string, projectId: string, native: string, open = true, update = true): TicketReaderFrame => ({
  id,
  open,
  projectId,
  projectName: projectId,
  apiPath: `/__hotsheet/project-api/${projectId}`,
  ticket: ticket(native),
  activeTab: 'info',
  capabilities: { update } as Capabilities,
  edit: ticketReaderEditState(ticket(native)),
  codeReview: { loading: false, message: '', review: { commits: [], ranges: [], truncated: false } },
});

function setup(stack: TicketReaderFrame[]) {
  let current = stack;
  const refreshCodeReview = vi.fn(async () => undefined),
    resolve = createLinkedReaderSubjects({
      linkedReaderStack: () => current,
      projects: () => [{ id: 'alpha', name: 'Alpha' } as Project, { id: 'beta', name: 'Beta' } as Project],
      refreshCodeReview,
      replaceFrame: (id, update) => {
        current = current.map((item) => (item.id === id ? update(item) : item));
      },
    });
  return { resolve, refreshCodeReview, stack: () => current };
}

describe('createLinkedReaderSubjects (HS2-NXSMC7)', () => {
  it('resolves a linked frame to its own project, ticket, and Code Review', async () => {
    const { resolve, refreshCodeReview } = setup([frame('reader-1', 'beta', 'one')]);
    const subject = resolve('reader-1');
    expect(subject).toMatchObject({
      frameId: 'reader-1',
      project: { id: 'beta' },
      ticket: { qualified_id: 'git:one' },
      canUpdate: true,
      codeReview: { commits: [] },
    });
    await subject?.refreshCodeReview();
    expect(refreshCodeReview).toHaveBeenCalledWith('reader-1');
  });

  it('leaves the Inspector, the workspace reader, and closed or unknown frames to the caller', () => {
    const { resolve } = setup([frame('reader-1', 'gamma', 'one')]);
    expect(resolve(undefined)).toBeUndefined();
    expect(resolve(WORKSPACE_READER_SCOPE)).toBeUndefined();
    expect(resolve('reader-9')).toBeUndefined();
    // A frame whose project is no longer open cannot route an edit.
    expect(resolve('reader-1')).toBeUndefined();
  });

  it('lets only the top open frame update, and only when its provider updates tickets', () => {
    const { resolve } = setup([
      frame('reader-1', 'alpha', 'one'),
      frame('reader-2', 'beta', 'two'),
      frame('reader-3', 'beta', 'three', false),
    ]);
    expect(resolve('reader-1')?.canUpdate).toBe(false);
    expect(resolve('reader-2')?.canUpdate).toBe(true);
    const readOnly = setup([frame('reader-1', 'alpha', 'one', true, false)]);
    expect(readOnly.resolve('reader-1')?.canUpdate).toBe(false);
  });

  it("adopts the server's ticket into its own frame and ignores another ticket", () => {
    const { resolve, stack } = setup([frame('reader-1', 'alpha', 'one'), frame('reader-2', 'beta', 'two')]);
    const subject = resolve('reader-1')!;
    subject.adopt(ticket('one', { blocked_by: ['two'] }));
    expect(stack()[0].ticket.blocked_by).toEqual(['two']);
    expect(stack()[1].ticket.blocked_by).toEqual([]);
    subject.adopt(ticket('other', { blocked_by: ['x'] }));
    expect(stack()[0].ticket.qualified_id).toBe('git:one');
  });
});

describe('readerScopeOf (HS2-NXSMC7)', () => {
  it('reads the reader frame id that contains an action, or none outside a reader', () => {
    const inside = {
      closest: (selector: string) =>
        selector === '[data-component="ticket-reader"]' ? { dataset: { readerFrameId: 'reader-2' } } : null,
    } as unknown as Element;
    expect(readerScopeOf(inside)).toBe('reader-2');
    expect(readerScopeOf({ closest: () => null } as unknown as Element)).toBeUndefined();
  });
});
