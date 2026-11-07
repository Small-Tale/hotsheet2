import { describe, expect, it } from 'vitest';

import type { ToolConnection } from './api';
import { mergeInterruptReply } from './drive-interrupt-reply';

const connection = (overrides: Partial<ToolConnection> = {}): ToolConnection =>
  ({ id: 'codex-1', tool: 'codex', busy: true, turn_id: 'turn-1', ...overrides }) as ToolConnection;

// HS2-80HD7K: an interrupt's reply is the accepted (still busy) snapshot of the turn it stopped.
describe('mergeInterruptReply', () => {
  it('applies the reply while the stored connection is still busy on that turn', () => {
    const other = connection({ id: 'claude-1', tool: 'claude' });
    const reply = connection({ in_flight_tool: undefined });
    expect(mergeInterruptReply([connection(), other], reply)).toEqual([other, reply]);
  });

  it('keeps an idle connection: the turn end was applied before the reply arrived', () => {
    const idle = connection({ busy: false, turn_id: undefined });
    expect(mergeInterruptReply([idle], connection())).toEqual([idle]);
  });

  it('keeps a connection that has moved on to another turn', () => {
    const next = connection({ turn_id: 'turn-2' });
    expect(mergeInterruptReply([next], connection())).toEqual([next]);
  });

  it('adds the reply when the connection is not stored yet, and stays usable after a refill', () => {
    const reply = connection();
    expect(mergeInterruptReply([], reply)).toEqual([reply]);
    // Ended, then a new turn starts and is stopped: the second reply applies again.
    const second = connection({ turn_id: 'turn-2' });
    expect(mergeInterruptReply([connection({ busy: false, turn_id: undefined })], reply)).toHaveLength(1);
    expect(mergeInterruptReply([second], second)).toEqual([second]);
  });

  it('returns a new array and never mutates its input', () => {
    const stored = [connection({ busy: false, turn_id: undefined })];
    const merged = mergeInterruptReply(stored, connection());
    expect(merged).not.toBe(stored);
    expect(stored).toHaveLength(1);
  });
});
