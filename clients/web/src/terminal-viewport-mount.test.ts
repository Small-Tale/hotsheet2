import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { mountStaticTerminalViewport, mountTerminalViewport } from './terminal-viewport';

const runtime = vi.hoisted(() => ({
  mountTerminalViewportRuntime: vi.fn(),
  mountStaticTerminalViewportRuntime: vi.fn(),
  browserRandomId: vi.fn(),
}));
vi.mock('./terminal-viewport-runtime', () => runtime);
vi.mock('./browser-id', () => ({ browserRandomId: runtime.browserRandomId }));

function viewport() {
  const children: unknown[] = [];
  const message = {
      className: '',
      textContent: '',
      setAttribute: vi.fn(),
      remove: vi.fn(() => {
        children.splice(children.indexOf(message), 1);
      }),
    },
    element = {
      dataset: {} as Record<string, string>,
      style: { width: '1280px', height: '768px', transform: 'scale(0.3)' },
      ownerDocument: { createElement: () => message },
      replaceChildren: vi.fn((child) => {
        children.splice(0, children.length, child);
      }),
      querySelector: vi.fn(() => (children.includes(message) ? message : null)),
      setAttribute: vi.fn(),
      removeAttribute: vi.fn(),
    };
  return {
    element: element as unknown as HTMLElement,
    message,
    children,
    replaced: element.replaceChildren,
    removed: element.removeAttribute,
    set: element.setAttribute,
  };
}

beforeEach(async () => {
  await import('./terminal-viewport-runtime');
  runtime.mountTerminalViewportRuntime.mockReset().mockImplementation(() => vi.fn());
  runtime.mountStaticTerminalViewportRuntime.mockReset().mockImplementation(() => vi.fn());
});
afterEach(() => vi.unstubAllGlobals());

describe('terminal lazy mount transitions (HS2-3ZBQDG)', () => {
  it('mounts distinct LAN viewers, preserves explicit identity and disposes exactly once', async () => {
    runtime.browserRandomId.mockReturnValueOnce('lan-viewer-one').mockReturnValueOnce('lan-viewer-two');
    const first = mountTerminalViewport(viewport().element, { url: 'ws://lan/one' });
    await vi.dynamicImportSettled();
    const second = mountTerminalViewport(viewport().element, { url: 'ws://lan/two' });
    await vi.dynamicImportSettled();
    const third = mountTerminalViewport(viewport().element, { url: 'ws://lan/three', viewerId: 'retained' });
    await vi.dynamicImportSettled();
    const ids = runtime.mountTerminalViewportRuntime.mock.calls.map(
      (call) => (call[1] as { viewerId: string }).viewerId,
    );
    expect(new Set(ids).size).toBe(3);
    expect(ids[2]).toBe('retained');
    first();
    first();
    second();
    third();
    for (const result of runtime.mountTerminalViewportRuntime.mock.results)
      expect(result.value).toHaveBeenCalledTimes(1);
  });

  it.each(['live', 'static'] as const)(
    'does not initialize a %s viewport disposed before the lazy module resolves',
    async (kind) => {
      const { element } = viewport(),
        dispose =
          kind === 'live'
            ? mountTerminalViewport(element, { url: 'ws://lan/one' })
            : mountStaticTerminalViewport(element, { output: 'hello' });
      dispose();
      await vi.dynamicImportSettled();
      expect(runtime.mountTerminalViewportRuntime).not.toHaveBeenCalled();
      expect(runtime.mountStaticTerminalViewportRuntime).not.toHaveBeenCalled();
    },
  );

  it.each(['live', 'static'] as const)(
    'makes a failed %s initialization visible and allows a later mount',
    async (kind) => {
      const { element, message, children, replaced, removed, set } = viewport(),
        mount = () =>
          kind === 'live'
            ? mountTerminalViewport(element, { url: 'ws://lan/one' })
            : mountStaticTerminalViewport(element, { output: 'hello' }),
        initialize =
          kind === 'live' ? runtime.mountTerminalViewportRuntime : runtime.mountStaticTerminalViewportRuntime;
      element.dataset.displayMode = 'scaled-preview';
      initialize.mockImplementationOnce(() => {
        throw new Error('module initialization failed');
      });
      const failed = mount();
      await vi.dynamicImportSettled();
      expect(element.dataset.connection).toBe('error');
      expect(message.setAttribute).toHaveBeenCalledWith('role', 'alert');
      expect(removed).toHaveBeenCalledWith('aria-hidden');
      expect(message.textContent).toContain('Terminal could not start');
      expect(replaced).toHaveBeenCalledWith(message);
      expect(element.style).toEqual({ width: '', height: '', transform: '' });
      failed();
      const terminal = {};
      initialize.mockImplementationOnce(() => {
        children.push(terminal);
        return vi.fn();
      });
      const retry = mount();
      await vi.dynamicImportSettled();
      expect(element.dataset.connection).toBe('loading');
      expect(initialize).toHaveBeenCalledTimes(2);
      expect(children).toEqual([terminal]);
      expect(message.remove).toHaveBeenCalledTimes(1);
      expect(set).toHaveBeenCalledWith('aria-hidden', 'true');
      retry();
    },
  );
});
