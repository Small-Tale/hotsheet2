import { createScope } from 'kerfjs/scope';
import { describe, expect, it, vi } from 'vitest';

import { combineInteractionTeardowns } from './lifetime';

describe('kerfjs/scope createScope (the interaction lifetime contract)', () => {
  it('releases every tracked disposer once, newest first', () => {
    const scope = createScope();
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
    const scope = createScope();
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
    const scope = createScope();
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
    const scope = createScope();
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

  it('keeps tearing down past a throwing disposer and still aborts the signal', () => {
    const scope = createScope();
    const signal = scope.signal;
    const first = vi.fn();
    scope.add(first);
    scope.add(() => {
      throw new Error('broken teardown');
    });
    expect(() => {
      scope.dispose();
    }).not.toThrow();
    expect(first).toHaveBeenCalledTimes(1);
    expect(signal.aborted).toBe(true);
    expect(scope.size).toBe(0);
  });

  it('aborts the signal only after every disposer ran', () => {
    const scope = createScope();
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
    const scope = createScope();
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
    const scope = createScope();
    const dispose = vi.fn();
    scope.add(dispose);
    // Kerf implements dispose as a closure but types it as a method (KF-9AH3G8).
    // eslint-disable-next-line @typescript-eslint/unbound-method -- external Kerf typing boundary
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
