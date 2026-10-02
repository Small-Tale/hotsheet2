import { describe, expect, it, vi } from 'vitest';

import { createDisposerScope } from './disposer-scope';

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
});
