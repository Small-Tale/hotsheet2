import { describe, expect, it, vi } from 'vitest';

import { closeLikeUpstream, relayableCloseCode } from './ws-close-relay';

describe('relaying an upstream close code (HS2-QWT94V)', () => {
  it('passes only application codes', () => {
    expect(relayableCloseCode(4000)).toBe(4000);
    expect(relayableCloseCode(4999)).toBe(4999);
    for (const code of [1000, 1005, 1006, 3999, 5000, 4000.5, '4000', undefined, new Error('reset')])
      expect(relayableCloseCode(code)).toBeUndefined();
  });

  it('closes with the upstream code and reason, or plainly', () => {
    const target = { close: vi.fn() };
    closeLikeUpstream(target, 4000, Buffer.from('terminal ended'));
    expect(target.close).toHaveBeenLastCalledWith(4000, 'terminal ended');
    closeLikeUpstream(target, 4001, 'text reason');
    expect(target.close).toHaveBeenLastCalledWith(4001, 'text reason');
    closeLikeUpstream(target, 4002, undefined);
    expect(target.close).toHaveBeenLastCalledWith(4002, '');
    // A socket error arrives in the code position; a reserved code cannot be sent.
    closeLikeUpstream(target, new Error('reset'), undefined);
    expect(target.close).toHaveBeenLastCalledWith();
    closeLikeUpstream(target, 1006, Buffer.alloc(0));
    expect(target.close).toHaveBeenLastCalledWith();
  });
});
