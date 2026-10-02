import { describe, expect, it, vi } from 'vitest';

import { createDisposerScope } from './disposer-scope';
import { combineInteractionTeardowns } from './interactions/lifetime';

describe('createDisposerScope', () => {
  it('releases every tracked disposer once, newest first', () => {
    const scope = createDisposerScope();
    const order: string[] = [];
    scope.add(() => order.push('a'));
    scope.add(() => order.push('b'));
    expect(scope.size).toBe(2);
    scope.dispose();
    expect(order).toEqual(['b', 'a']);
    expect(scope.size).toBe(0);
    scope.dispose();
    expect(order).toEqual(['b', 'a']);
  });

  it('returns the added disposer and can be refilled after teardown', () => {
    const scope = createDisposerScope();
    const first = vi.fn();
    expect(scope.add(first)).toBe(first);
    scope.dispose();
    const second = vi.fn();
    scope.add(second);
    scope.dispose();
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
  });

  it('keeps a disposer registered during teardown for the next generation', () => {
    const scope = createDisposerScope();
    const late = vi.fn();
    scope.add(() => scope.add(late));
    scope.dispose();
    expect(late).not.toHaveBeenCalled();
    expect(scope.size).toBe(1);
    scope.dispose();
    expect(late).toHaveBeenCalledTimes(1);
  });

  it('removes listeners on teardown and wires again after refill', () => {
    const target = new EventTarget();
    const handler = vi.fn();
    const listen = () => {
      target.addEventListener('ping', handler);
      return () => {
        target.removeEventListener('ping', handler);
      };
    };
    const scope = createDisposerScope();
    scope.add(listen());
    target.dispatchEvent(new Event('ping'));
    scope.dispose();
    target.dispatchEvent(new Event('ping'));
    expect(handler).toHaveBeenCalledTimes(1);
    scope.add(listen());
    target.dispatchEvent(new Event('ping'));
    expect(handler).toHaveBeenCalledTimes(2);
    scope.dispose();
  });

  it('aborts the signal only after every disposer ran', () => {
    const scope = createDisposerScope();
    const signal = scope.signal;
    const seen: boolean[] = [];
    scope.add(() => seen.push(signal.aborted));
    scope.add(() => seen.push(signal.aborted));
    expect(signal.aborted).toBe(false);
    scope.dispose();
    expect(seen).toEqual([false, false]);
    expect(signal.aborted).toBe(true);
  });

  it('walks native and delegated registrations across empty, refill, and repeated teardowns', () => {
    const target = new EventTarget();
    const native = vi.fn();
    const delegated = vi.fn();
    const scope = createDisposerScope();
    // Empty teardown: nothing to run, and the scope still works afterwards.
    scope.dispose();
    const first = scope.signal;
    target.addEventListener('ping', native, { signal: first });
    target.addEventListener('ping', delegated);
    scope.add(() => {
      target.removeEventListener('ping', delegated);
    });
    target.dispatchEvent(new Event('ping'));
    scope.dispose();
    target.dispatchEvent(new Event('ping'));
    expect([native.mock.calls.length, delegated.mock.calls.length]).toEqual([1, 1]);
    expect(first.aborted).toBe(true);
    // Refill: a later generation hands out a live signal and removes its own listeners.
    const second = scope.signal;
    expect(second).not.toBe(first);
    expect(second.aborted).toBe(false);
    target.addEventListener('ping', native, { signal: second });
    target.dispatchEvent(new Event('ping'));
    expect(native).toHaveBeenCalledTimes(2);
    scope.dispose();
    scope.dispose();
    target.dispatchEvent(new Event('ping'));
    expect(native).toHaveBeenCalledTimes(2);
    expect(second.aborted).toBe(true);
  });

  it('returns a bound teardown that stays idempotent when detached from the scope', () => {
    const scope = createDisposerScope();
    const dispose = vi.fn();
    scope.add(dispose);
    const teardown = scope.dispose;
    teardown();
    teardown();
    expect(dispose).toHaveBeenCalledTimes(1);
  });
});

describe('combineInteractionTeardowns', () => {
  it('runs group teardowns newest first, once', () => {
    const order: string[] = [];
    const combined = combineInteractionTeardowns([() => order.push('a'), () => order.push('b'), () => order.push('c')]);
    combined();
    combined();
    expect(order).toEqual(['c', 'b', 'a']);
  });
});
