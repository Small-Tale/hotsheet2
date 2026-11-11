import { describe, expect, it } from 'vitest';

import type { Capabilities, CheckoutTicketCounts, TicketRow } from './api';
import {
  bulkTagChoices,
  BulkTicketMutationSequencer,
  bulkTicketPatch,
  canAtomicallyBulkUpdate,
  canBulkUpdate,
  projectBulkTicketCounts,
  ticketGroupDisabledReason,
  ticketGroupMenuModel,
} from './ticket-bulk-operations';

const ticket = (slug: string, connection_id = 'git', tags: string[] = []): TicketRow => ({
  connection_id,
  native_id: slug,
  qualified_id: `${connection_id}:${slug}`,
  id: slug,
  slug,
  title: slug,
  up_next: false,
  feedback_needed: false,
  tags,
  blocked_by: [],
  claim_count: 0,
});
const capabilities = (update: boolean, atomic_batch = update) => ({ update, atomic_batch }) as Capabilities;

describe('bulk ticket operations', () => {
  it('keeps unseen Completed totals available through verify then archive projections', () => {
    const counts: CheckoutTicketCounts = {
        total: 309,
        queued: 309,
        backlog: 0,
        archive: 0,
        open: 0,
        up_next: 0,
        active: 0,
        started: 0,
        verified: 0,
        completed_today: 0,
      },
      completed = Array.from({ length: 100 }, (_, index) => ({ ...ticket(`CP${index}`), status: 'completed' })),
      verified = completed.map((item) => ({ ...item, status: 'verified' })),
      archived = verified.map((item) => ({ ...item, status: 'archive' }));
    const afterVerify = projectBulkTicketCounts(
      counts,
      completed.map((before, index) => ({ before, after: verified[index] })),
    );
    expect(afterVerify).toMatchObject({ queued: 309, archive: 0, verified: 100 });
    expect(afterVerify.queued - afterVerify.open - (afterVerify.verified ?? 0)).toBe(209);
    const afterArchive = projectBulkTicketCounts(
      afterVerify,
      verified.map((before, index) => ({ before, after: archived[index] })),
    );
    expect(afterArchive).toMatchObject({ queued: 209, archive: 100, verified: 0 });
    expect(afterArchive.queued - afterArchive.open - (afterArchive.verified ?? 0)).toBe(209);
  });
  it('allows best-effort bulk updates while distinguishing atomic providers', () => {
    const selected = [ticket('ONE', 'git'), ticket('TWO', 'jira')];
    expect(canBulkUpdate(selected, (id) => capabilities(id === 'git'))).toBe(false);
    expect(canBulkUpdate(selected, () => capabilities(true))).toBe(true);
    expect(canBulkUpdate(selected, () => capabilities(true, false))).toBe(true);
    expect(canBulkUpdate([], () => capabilities(true))).toBe(false);
    expect(canAtomicallyBulkUpdate(selected, () => capabilities(true))).toBe(true);
    expect(canAtomicallyBulkUpdate(selected, () => capabilities(true, false))).toBe(false);
  });

  it('builds field and soft-delete patches', () => {
    expect(bulkTicketPatch(ticket('ONE'), { kind: 'field', field: 'priority', value: 'high' })).toEqual({
      priority: 'high',
    });
    expect(bulkTicketPatch(ticket('ONE'), { kind: 'reopen' })).toEqual({ status: 'not_started', up_next: true });
    expect(bulkTicketPatch(ticket('ONE'), { kind: 'delete' })).toEqual({ status: 'deleted' });
  });

  it('adds and removes a tag per ticket without duplicates or empty writes', () => {
    const tagged = ticket('ONE', 'git', ['client', 'bug']);
    expect(bulkTicketPatch(tagged, { kind: 'add-tag', tag: ' regression ' })).toEqual({
      tags: ['client', 'bug', 'regression'],
    });
    expect(bulkTicketPatch(tagged, { kind: 'add-tag', tag: 'client' })).toBeUndefined();
    expect(bulkTicketPatch(tagged, { kind: 'remove-tag', tag: 'client' })).toEqual({ tags: ['bug'] });
    expect(bulkTicketPatch(tagged, { kind: 'remove-tag', tag: 'missing' })).toBeUndefined();
    expect(bulkTicketPatch(tagged, { kind: 'add-tag', tag: ' ' })).toBeUndefined();
  });

  it('collects stable unique remove choices across a mixed selection', () => {
    expect(
      bulkTagChoices([ticket('ONE', 'git', ['zeta', 'client']), ticket('TWO', 'git', ['client', 'alpha'])]),
    ).toEqual(['alpha', 'client', 'zeta']);
  });

  it('starts a later same-project mutation after the prior commit exposes fresh tokens', async () => {
    const sequencer = new BulkTicketMutationSequencer();
    const events: string[] = [];
    let token = 'before-verified';
    let releaseVerified!: () => void;
    const verifiedResponse = new Promise<void>((resolve) => {
      releaseVerified = resolve;
    });
    const verified = sequencer.enqueue('demo', async () => {
      events.push(`verified:${token}`);
      await verifiedResponse;
      token = 'after-verified';
      events.push('verified:committed');
    });
    expect(events).toEqual(['verified:before-verified']);
    const archived = sequencer.enqueue('demo', async () => {
      events.push(`archive:${token}`);
    });

    await Promise.resolve();
    expect(events).toEqual(['verified:before-verified']);
    releaseVerified();
    await Promise.all([verified, archived]);
    expect(events).toEqual(['verified:before-verified', 'verified:committed', 'archive:after-verified']);
  });

  it('continues the queue after a genuine conflict rolls the prior action back', async () => {
    const sequencer = new BulkTicketMutationSequencer();
    const events: string[] = [];
    const conflict = sequencer.enqueue('demo', async () => {
      events.push('verified:conflict');
      throw new Error('modified concurrently');
    });
    const archive = sequencer.enqueue('demo', async () => {
      events.push('archive:attempted');
      return true;
    });

    await expect(conflict).rejects.toThrow('modified concurrently');
    await expect(archive).resolves.toBe(true);
    expect(events).toEqual(['verified:conflict', 'archive:attempted']);
  });

  it('does not unnecessarily serialize independent projects', async () => {
    const sequencer = new BulkTicketMutationSequencer();
    let releaseDemo!: () => void;
    const demoResponse = new Promise<void>((resolve) => {
      releaseDemo = resolve;
    });
    const demo = sequencer.enqueue('demo', () => demoResponse);
    await expect(sequencer.enqueue('other', async () => 'ready')).resolves.toBe('ready');
    releaseDemo();
    await demo;
  });

  it('builds group patches that skip unchanged tickets and can carry a column status (HS2-PRXD39)', () => {
    const ungrouped = { ...ticket('HS2-U'), status: 'not_started' },
      grouped = { ...ticket('HS2-G'), status: 'started', group: 'Board' };
    expect(bulkTicketPatch(ungrouped, { kind: 'group', group: 'Board' })).toEqual({ group: 'Board' });
    // The same group in another casing is no change; its stored spelling is shared.
    expect(bulkTicketPatch(grouped, { kind: 'group', group: ' board ' })).toBeUndefined();
    expect(bulkTicketPatch(grouped, { kind: 'group', group: 'Search' })).toEqual({ group: 'Search' });
    expect(bulkTicketPatch(grouped, { kind: 'group', group: null })).toEqual({ group: null });
    expect(bulkTicketPatch(ungrouped, { kind: 'group', group: null })).toBeUndefined();
    expect(bulkTicketPatch({ ...ungrouped, group: null }, { kind: 'group', group: null })).toBeUndefined();
    expect(bulkTicketPatch(ungrouped, { kind: 'group', group: 'Board', status: 'started' })).toEqual({
      group: 'Board',
      status: 'started',
    });
    expect(bulkTicketPatch(grouped, { kind: 'group', group: 'Board', status: 'completed' })).toEqual({
      status: 'completed',
    });
    expect(bulkTicketPatch(grouped, { kind: 'group', group: 'Board', status: 'started' })).toBeUndefined();
  });

  it('offers grouping only when every target provider supports groups (HS2-TX17CM)', () => {
    const git = { update: true, groups: true } as Capabilities,
      github = { update: true } as Capabilities,
      readOnly = { update: false, groups: true } as Capabilities,
      lookup = (id: string) => ({ git, github, readOnly })[id];
    const a = { ...ticket('HS2-A'), group: 'Board' },
      b = { ...ticket('HS2-B'), group: 'board' },
      c = ticket('HS2-C'),
      external = ticket('GH-1', 'github');
    expect(ticketGroupDisabledReason([a, c], lookup)).toBeUndefined();
    expect(ticketGroupDisabledReason([a, external], lookup)).toBe(
      'One or more selected ticket providers do not support groups.',
    );
    expect(ticketGroupDisabledReason([ticket('X', 'readOnly')], lookup)).toBe(
      'One or more selected ticket providers do not support updates.',
    );
    expect(ticketGroupMenuModel([], ['Board'], lookup)).toBeUndefined();
    expect(ticketGroupMenuModel([a, b], ['Board', 'Search'], lookup)).toEqual({
      names: ['Board', 'Search'],
      current: 'Board',
      removable: true,
      disabledReason: undefined,
    });
    expect(ticketGroupMenuModel([a, c], ['Board'], lookup)).toMatchObject({ current: undefined, removable: true });
    expect(ticketGroupMenuModel([c], [], lookup)).toMatchObject({ names: [], removable: false });
    expect(ticketGroupMenuModel([c, external], [], lookup)?.disabledReason).toContain('groups');
  });
});
