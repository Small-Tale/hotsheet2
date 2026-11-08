import { describe, expect, it, vi } from 'vitest';

import { activeAdvisoryOperationId, advisoryRenewExpiry, createAdvisoryClaimResource } from './advisory-claim-resource';
import type { AdvisoryClaimStatus } from './api';

const status = (worker: string): AdvisoryClaimStatus => ({
  projection: {
    claims: [{ worker, operation_id: 'op-a', expires_unix_ms: 2_000_000_000_000 }],
    conflicted: false,
    invalid_markers: 0,
  },
  provisional: false,
  remote_verified: true,
  automatic_handoff_safe: true,
  pending_operation_ids: [],
  pending_operations: [],
});
const emptyStatus = (): AdvisoryClaimStatus => ({
  ...status('worker-a'),
  projection: { claims: [], conflicted: false, invalid_markers: 0 },
});

describe('HS2-2MZWZT advisory claim resource', () => {
  it('HS2-WHW46N refreshes only a matching checkout, connection, and native ticket', async () => {
    const read = vi.fn().mockResolvedValue(emptyStatus());
    const resource = createAdvisoryClaimResource(() => ({
      advisoryClaimStatus: read,
      appendAdvisoryClaimEvent: vi.fn(),
    }));
    resource.select({ checkout: 'code', connection: 'jira', nativeId: 'ENG-1' });
    await Promise.resolve();
    for (const target of [
      ['other', 'jira', 'ENG-1'],
      ['code', 'other', 'ENG-1'],
      ['code', 'jira', 'ENG-2'],
    ])
      await resource.refreshMatching(...(target as [string, string, string]));
    expect(read).toHaveBeenCalledTimes(1);
    await resource.refreshMatching('code', 'jira', 'ENG-1');
    await resource.refreshMatching('code'); // reconnect resync
    expect(read).toHaveBeenCalledTimes(3);
  });
  it('HS2-WHW46N rechecks a matching remote invalidation that arrives during a local write', async () => {
    let finish!: (value: AdvisoryClaimStatus) => void;
    const pending = new Promise<AdvisoryClaimStatus>((resolve) => {
      finish = resolve;
    });
    const read = vi.fn().mockResolvedValueOnce(emptyStatus()).mockResolvedValueOnce(status('other-worker'));
    const resource = createAdvisoryClaimResource(() => ({
      advisoryClaimStatus: read,
      appendAdvisoryClaimEvent: vi.fn().mockReturnValue(pending),
    }));
    resource.select({ checkout: 'code', connection: 'jira', nativeId: 'ENG-1' });
    await Promise.resolve();
    const writing = resource.submit({ worker: 'me', operation_id: 'op-me', kind: { type: 'release' } });
    await resource.refreshMatching('code', 'jira', 'ENG-1');
    expect(read).toHaveBeenCalledTimes(1);
    finish(status('me'));
    await writing;
    await Promise.resolve();
    expect(read).toHaveBeenCalledTimes(2);
    expect(resource.state.value.status?.projection.claims[0].worker).toBe('other-worker');
  });
  it('caps v2 renewal at the operation horizon and refuses an elapsed horizon', () => {
    const now = 2_000_000_000_000;
    const issued = now - 24 * 60 * 60_000 + 60_000;
    const operationId = `v2:${issued}:0123456789abcdef`;
    expect(advisoryRenewExpiry(operationId, now)).toBe(now + 60_000);
    expect(advisoryRenewExpiry(operationId, now + 60_001)).toBeUndefined();
    expect(advisoryRenewExpiry('legacy-id', now)).toBe(now + 30 * 60_000);
  });
  it('uses the live worker operation over a stale saved retry id after reload', () => {
    expect(activeAdvisoryOperationId(status('worker-a'), 'worker-a', 'stale-operation')).toBe('op-a');
  });
  it('keeps a write busy when a collection refresh arrives before its response', async () => {
    let finish!: (value: AdvisoryClaimStatus) => void;
    const pending = new Promise<AdvisoryClaimStatus>((resolve) => {
      finish = resolve;
    });
    const read = vi.fn().mockResolvedValue(emptyStatus());
    const send = vi.fn().mockReturnValue(pending);
    const resource = createAdvisoryClaimResource(() => ({ advisoryClaimStatus: read, appendAdvisoryClaimEvent: send }));
    resource.select({ checkout: 'code', connection: 'jira', nativeId: 'ENG-1' });
    await Promise.resolve();
    const event = {
      worker: 'worker-a',
      operation_id: 'op-a',
      kind: { type: 'claim' as const, expires_unix_ms: 2_000_000_000_000 },
    };
    const writing = resource.submit(event);
    await resource.refresh();
    expect(resource.state.value.busy).toBe(true);
    expect(read).toHaveBeenCalledTimes(1);
    expect(await resource.submit(event)).toBeUndefined();
    expect(send).toHaveBeenCalledTimes(1);
    finish(status('worker-a'));
    await writing;
    expect(resource.state.value.status?.projection.claims[0].worker).toBe('worker-a');
    expect(resource.state.value.busy).toBe(false);
  });
  it('discards a late response for a different selected ticket', async () => {
    let finish!: (value: AdvisoryClaimStatus) => void;
    const first = new Promise<AdvisoryClaimStatus>((resolve) => {
      finish = resolve;
    });
    const read = vi.fn().mockReturnValueOnce(first).mockResolvedValueOnce(status('worker-b'));
    const resource = createAdvisoryClaimResource(() => ({
      advisoryClaimStatus: read,
      appendAdvisoryClaimEvent: vi.fn(),
    }));
    resource.select({ checkout: 'code', connection: 'jira', nativeId: 'ENG-1' });
    resource.select({ checkout: 'code', connection: 'jira', nativeId: 'ENG-2' });
    await Promise.resolve();
    finish(status('worker-a'));
    await Promise.resolve();
    expect(resource.state.value.target?.nativeId).toBe('ENG-2');
    expect(resource.state.value.status?.projection.claims[0].worker).toBe('worker-b');
  });

  it('re-reads the same ticket when the project API path changes on reconnect', async () => {
    const read = vi.fn().mockResolvedValueOnce(status('worker-a')).mockResolvedValueOnce(status('worker-b'));
    const resource = createAdvisoryClaimResource(() => ({
      advisoryClaimStatus: read,
      appendAdvisoryClaimEvent: vi.fn(),
    }));
    resource.select({ checkout: 'code', connection: 'jira', nativeId: 'ENG-1', apiPath: '/old' });
    await Promise.resolve();
    resource.select({ checkout: 'code', connection: 'jira', nativeId: 'ENG-1', apiPath: '/new' });
    await Promise.resolve();
    expect(read).toHaveBeenCalledTimes(2);
    expect(resource.state.value.status?.projection.claims[0].worker).toBe('worker-b');
  });

  it('clears stale status on a failed reconnect read and recovers on the next event', async () => {
    const read = vi
      .fn()
      .mockResolvedValueOnce(status('worker-a'))
      .mockRejectedValueOnce(new Error('invalid remote marker'))
      .mockResolvedValueOnce(status('worker-b'));
    const resource = createAdvisoryClaimResource(() => ({
      advisoryClaimStatus: read,
      appendAdvisoryClaimEvent: vi.fn(),
    }));
    resource.select({ checkout: 'code', connection: 'jira', nativeId: 'ENG-1' });
    await Promise.resolve();
    expect(resource.state.value.status?.projection.claims[0].worker).toBe('worker-a');
    await resource.refresh();
    expect(resource.state.value.status).toBeUndefined();
    expect(resource.state.value.error).toContain('invalid remote marker');
    await resource.refresh();
    expect(resource.state.value.status?.projection.claims[0].worker).toBe('worker-b');
    expect(resource.state.value.error).toBeUndefined();
    expect(read).toHaveBeenCalledTimes(3);
  });

  it('keeps a failed claim write unresolved, then accepts an exact retry', async () => {
    const send = vi
      .fn()
      .mockRejectedValueOnce(new Error('provider unavailable'))
      .mockResolvedValueOnce(status('worker-a'));
    const resource = createAdvisoryClaimResource(() => ({
      advisoryClaimStatus: vi.fn().mockResolvedValue(emptyStatus()),
      appendAdvisoryClaimEvent: send,
    }));
    resource.select({ checkout: 'code', connection: 'jira', nativeId: 'ENG-1' });
    await Promise.resolve();
    const event = {
      worker: 'worker-a',
      operation_id: 'op-a',
      kind: { type: 'claim' as const, expires_unix_ms: 2_000_000_000_000 },
    };
    expect(await resource.submit(event)).toBeUndefined();
    expect(resource.state.value.status).toBeUndefined();
    expect(resource.state.value.error).toContain('provider unavailable');
    expect(await resource.submit(event)).toEqual(status('worker-a'));
    expect(send).toHaveBeenNthCalledWith(2, 'code', 'jira', 'ENG-1', event);
  });
});
