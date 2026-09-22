import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { Project } from '../interactions/types';
import { mountTerminalViewport, type TerminalFocusRequest } from '../terminal-viewport';
import { createTerminalViewportsController } from './terminal-viewports';

vi.mock('../terminal-viewport', async (original) => ({
  ...(await original<typeof import('../terminal-viewport')>()),
  mountTerminalViewport: vi.fn(),
}));

const mount = vi.mocked(mountTerminalViewport);
const project = (id: string): Project => ({
  id,
  name: id,
  root: `/work/${id}`,
  apiPath: `/api/${id}`,
  stores: [],
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
});
function viewport(projectId: string, terminalId: string, progressive = false) {
  return {
    isConnected: true,
    dataset: {
      projectId,
      terminalId,
      displayMode: 'interactive',
      mountPolicy: progressive ? 'visible-progressive' : '',
    },
    closest: () => null,
  } as unknown as HTMLElement;
}
let elements: HTMLElement[], frames: FrameRequestCallback[];
beforeEach(() => {
  vi.useFakeTimers();
  mount.mockReset();
  elements = [];
  frames = [];
  vi.stubGlobal('document', { querySelectorAll: () => elements });
  vi.stubGlobal('location', { protocol: 'https:', host: 'example.test' });
  vi.stubGlobal('window', { setTimeout });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => frames.push(callback));
  vi.stubGlobal('IntersectionObserver', undefined);
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
async function paint() {
  frames.splice(0).forEach((frame) => {
    frame(0);
  });
  await vi.runOnlyPendingTimersAsync();
}

describe('terminal viewport feature ownership (HS2-DHYGXJ)', () => {
  it('uses live focus and projects, mounts once, disposes detached views and mounts a refill', async () => {
    const projects = signal([project('a')]);
    let pending: TerminalFocusRequest | undefined = { projectId: 'a', terminalId: 'different' };
    const openTicketReference = vi.fn(),
      dispose = vi.fn();
    mount.mockReturnValue(dispose);
    const owner = createTerminalViewportsController({
      projects,
      get pendingTerminalFocus() {
        return pending;
      },
      set pendingTerminalFocus(value) {
        pending = value;
      },
      openTicketReference,
    });
    const first = viewport('a', 'one');
    elements = [first];
    owner.syncTerminalViewportMounts();
    owner.syncTerminalViewportMounts();
    expect(mount).toHaveBeenCalledTimes(1);
    expect(mount.mock.calls[0][1]).toMatchObject({
      url: 'wss://example.test/api/a/terminals/one/attach',
      autoFocus: false,
    });
    expect(pending.terminalId).toBe('different');
    mount.mock.calls[0][1].onTicketReference?.('HS2-ABC123');
    expect(openTicketReference).toHaveBeenCalledWith(first, 'HS2-ABC123', undefined, 'a');
    mount.mock.calls[0][1].onTicketReference?.('invalid');
    expect(openTicketReference).toHaveBeenCalledTimes(1);
    elements = [];
    owner.syncTerminalViewportMounts();
    expect(dispose).not.toHaveBeenCalled();
    await paint();
    expect(dispose).toHaveBeenCalledTimes(1);
    projects.value = [project('b')];
    pending = { projectId: 'b', terminalId: 'two' };
    const second = viewport('b', 'two');
    elements = [second];
    owner.syncTerminalViewportMounts();
    expect(mount).toHaveBeenCalledTimes(2);
    expect(mount.mock.calls[1][1]).toMatchObject({
      url: 'wss://example.test/api/b/terminals/two/attach',
      autoFocus: true,
    });
    expect(pending).toBeUndefined();
    elements = [];
    owner.syncTerminalViewportMounts();
    await paint();
    expect(dispose).toHaveBeenCalledTimes(2);
  });

  it('cancels stale observed candidates before queued work and observes replacement previews', async () => {
    const callbacks: IntersectionObserverCallback[] = [],
      observe = vi.fn(),
      unobserve = vi.fn();
    class Observer {
      constructor(callback: IntersectionObserverCallback) {
        callbacks.push(callback);
      }
      observe = observe;
      unobserve = unobserve;
    }
    vi.stubGlobal('IntersectionObserver', Observer);
    mount.mockReturnValue(vi.fn());
    const owner = createTerminalViewportsController({
      projects: signal([project('a')]),
      pendingTerminalFocus: undefined,
      openTicketReference: vi.fn(),
    });
    const first = viewport('a', 'one', true),
      second = viewport('a', 'two', true);
    const intersect = (target: HTMLElement) => {
      callbacks[0](
        [{ target, isIntersecting: true } as unknown as IntersectionObserverEntry],
        {} as IntersectionObserver,
      );
    };
    elements = [first];
    owner.syncTerminalViewportMounts();
    expect(observe).toHaveBeenCalledWith(first);
    intersect(first);
    elements = [];
    owner.syncTerminalViewportMounts();
    await paint();
    expect(mount).not.toHaveBeenCalled();
    expect(unobserve).toHaveBeenCalledWith(first);
    elements = [second];
    owner.syncTerminalViewportMounts();
    intersect(first);
    intersect(second);
    intersect(second);
    await paint();
    expect(mount).toHaveBeenCalledTimes(1);
    expect(mount.mock.calls[0][0]).toBe(second);
    expect(unobserve).toHaveBeenCalledWith(second);
  });
});
