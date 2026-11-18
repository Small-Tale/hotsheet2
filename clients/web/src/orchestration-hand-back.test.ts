import { describe, expect, it } from 'vitest';

import {
  answeredHandBacks,
  handBackDigest,
  handBackOperationId,
  type PendingHandBack,
  recoveredHandBacks,
} from './orchestration-hand-back';

describe('drawer hand-back continuation (HS2-BPTQ5X)', () => {
  it('names a request by a stable digest of its hosted session and a bounded random part', () => {
    expect(handBackDigest('c1', 'op-1')).toMatch(/^[0-9a-f]{8}$/u);
    expect(handBackDigest('c1', 'op-1')).toBe(handBackDigest('c1', 'op-1'));
    expect(handBackDigest('c1', 'op-1')).not.toBe(handBackDigest('c1', 'op-2'));
    const id = handBackOperationId('c1', 'op-1', '0f3c-9a1b-ffff');
    expect(id).toBe(`hand-back-${handBackDigest('c1', 'op-1')}-0f3c9a1bffff`);
    expect(id).toMatch(/^[A-Za-z0-9._-]{1,160}$/u);
    expect(handBackOperationId('c1', 'op-1', 'x'.repeat(100)).length).toBeLessThanOrEqual(160);
  });

  it('recovers only requests that name a session this drawer shows', () => {
    const sessions = [
      { checkout_id: 'c1', operation_id: 'op-1' },
      { checkout_id: 'c1', operation_id: 'op-2' },
    ];
    const asks = [
      { request_id: 'r1', operation_id: handBackOperationId('c1', 'op-2', 'a1'), owner_checkout_id: 'o' },
      { request_id: 'r2', operation_id: handBackOperationId('c9', 'op-9', 'a2'), owner_checkout_id: 'o' },
      { request_id: 'r3', operation_id: 'validate-x-y-0123456789ab', owner_checkout_id: 'o' },
    ];
    expect(recoveredHandBacks(asks, sessions)).toEqual([
      { checkout_id: 'c1', operation_id: 'op-2', handBackOperationId: asks[0].operation_id, requestId: 'r1' },
    ]);
    expect(recoveredHandBacks([], sessions)).toEqual([]);
    expect(recoveredHandBacks(asks, [])).toEqual([]);
  });

  it('answers a request seen pending once it is pending no longer, once', () => {
    const entry = (requestId?: string): PendingHandBack => ({
      checkout_id: 'c1',
      operation_id: 'op-1',
      handBackOperationId: 'hand-back-x',
      ...(requestId ? { requestId } : {}),
    });
    const seen = new Set<string>();
    const waiting = [entry('r1'), entry()];
    // Never seen pending: not answered (it may not have reached the inbox yet).
    expect(answeredHandBacks(waiting, new Set(), seen)).toEqual([]);
    expect(answeredHandBacks(waiting, new Set(['r1']), seen)).toEqual([]);
    expect(answeredHandBacks(waiting, new Set(['r1']), seen)).toEqual([]);
    expect(answeredHandBacks(waiting, new Set(), seen)).toEqual([entry('r1')]);
    // Answered once: a repeat of the same inbox does not send it twice.
    expect(answeredHandBacks(waiting, new Set(), seen)).toEqual([]);
  });
});
