import { describe, expect, it } from 'vitest';

import type { Capabilities, TicketRow } from './api';
import { bulkTagChoices, BulkTicketMutationSequencer, bulkTicketPatch, canAtomicallyBulkUpdate, canBulkUpdate } from './ticket-bulk-operations';

const ticket = (slug: string, connection_id = 'git', tags: string[] = []): TicketRow => ({
  connection_id, native_id: slug, qualified_id: `${connection_id}:${slug}`, id: slug, slug, title: slug,
  up_next: false, feedback_needed: false, tags, blocked_by: [], claim_count: 0,
});
const capabilities = (update: boolean, atomic_batch = update) => ({ update, atomic_batch } as Capabilities);

describe('bulk ticket operations', () => {
  it('allows best-effort bulk updates while distinguishing atomic providers', () => {
    const selected = [ticket('ONE', 'git'), ticket('TWO', 'jira')];
    expect(canBulkUpdate(selected, id => capabilities(id === 'git'))).toBe(false);
    expect(canBulkUpdate(selected, () => capabilities(true))).toBe(true);
    expect(canBulkUpdate(selected, () => capabilities(true, false))).toBe(true);
    expect(canBulkUpdate([], () => capabilities(true))).toBe(false);
    expect(canAtomicallyBulkUpdate(selected, () => capabilities(true))).toBe(true);
    expect(canAtomicallyBulkUpdate(selected, () => capabilities(true, false))).toBe(false);
  });

  it('builds field and soft-delete patches', () => {
    expect(bulkTicketPatch(ticket('ONE'), { kind: 'field', field: 'priority', value: 'high' })).toEqual({ priority: 'high' });
    expect(bulkTicketPatch(ticket('ONE'), { kind: 'delete' })).toEqual({ status: 'deleted' });
  });

  it('adds and removes a tag per ticket without duplicates or empty writes', () => {
    const tagged = ticket('ONE', 'git', ['client', 'bug']);
    expect(bulkTicketPatch(tagged, { kind: 'add-tag', tag: ' regression ' })).toEqual({ tags: ['client', 'bug', 'regression'] });
    expect(bulkTicketPatch(tagged, { kind: 'add-tag', tag: 'client' })).toBeUndefined();
    expect(bulkTicketPatch(tagged, { kind: 'remove-tag', tag: 'client' })).toEqual({ tags: ['bug'] });
    expect(bulkTicketPatch(tagged, { kind: 'remove-tag', tag: 'missing' })).toBeUndefined();
    expect(bulkTicketPatch(tagged, { kind: 'add-tag', tag: ' ' })).toBeUndefined();
  });

  it('collects stable unique remove choices across a mixed selection', () => {
    expect(bulkTagChoices([ticket('ONE', 'git', ['zeta', 'client']), ticket('TWO', 'git', ['client', 'alpha'])])).toEqual(['alpha', 'client', 'zeta']);
  });

  it('starts a later same-project mutation after the prior commit exposes fresh tokens', async () => {
    const sequencer = new BulkTicketMutationSequencer();
    const events: string[] = [];
    let token = 'before-verified';
    let releaseVerified!: () => void;
    const verifiedResponse = new Promise<void>(resolve => { releaseVerified = resolve; });
    const verified = sequencer.enqueue('demo', async () => {
      events.push(`verified:${token}`);
      await verifiedResponse;
      token = 'after-verified';
      events.push('verified:committed');
    });
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
    const demoResponse = new Promise<void>(resolve => { releaseDemo = resolve; });
    const demo = sequencer.enqueue('demo', () => demoResponse);
    await expect(sequencer.enqueue('other', async () => 'ready')).resolves.toBe('ready');
    releaseDemo();
    await demo;
  });
});
