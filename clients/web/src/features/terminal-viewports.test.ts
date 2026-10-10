import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { Project } from '../interactions/types';
import {
  mountTerminalViewport,
  TERMINAL_VIEWPORT_PARK_EVENT,
  TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT,
  TERMINAL_VIEWPORT_RESUME_EVENT,
  type TerminalFocusRequest,
} from '../terminal-viewport';
import { createTerminalViewportsController, MAX_PARKED_TERMINAL_VIEWPORTS } from './terminal-viewports';

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

  it('disposes a grid preview while its terminal is magnified and remounts it after dismissal (HS2-ACMRF6)', async () => {
    mount.mockImplementation(() => vi.fn());
    const owner = createTerminalViewportsController({
        projects: signal([project('a')]),
        pendingTerminalFocus: undefined,
        openTicketReference: vi.fn(),
      }),
      preview = viewport('a', 'one', true),
      magnified = viewport('a', 'one'),
      restoredPreview = viewport('a', 'one', true);

    elements = [preview];
    owner.syncTerminalViewportMounts();
    await paint();
    expect(mount).toHaveBeenCalledWith(preview, expect.any(Object));

    elements = [magnified];
    owner.syncTerminalViewportMounts();
    expect(mount).toHaveBeenCalledWith(magnified, expect.any(Object));
    expect(mount.mock.results[0].value).toHaveBeenCalledTimes(1);
    await paint();

    elements = [restoredPreview];
    owner.syncTerminalViewportMounts();
    expect(mount.mock.results[1].value).toHaveBeenCalledTimes(1);
    await paint();
    expect(mount).toHaveBeenCalledWith(restoredPreview, expect.any(Object));
    expect(mount).toHaveBeenCalledTimes(3);
  });
});

describe('kept-alive terminal viewports across project switches (HS2-WGTQ6X)', () => {
  type Fake = HTMLElement & { events: string[]; removed: boolean };
  function keepAlive(projectId: string, terminalId: string): Fake {
    const target = new EventTarget() as unknown as Fake;
    const dispatch = target.dispatchEvent.bind(target);
    Object.assign(target, {
      isConnected: true,
      dataset: { projectId, terminalId, displayMode: 'interactive', mountPolicy: 'keep-alive' },
      closest: () => null,
      events: [] as string[],
      removed: false,
      remove() {
        target.removed = true;
      },
      dispatchEvent(event: Event) {
        target.events.push(event.type);
        return dispatch(event);
      },
    });
    return target;
  }
  function setup(projectIds = ['a', 'b']) {
    const projects = signal(projectIds.map(project)),
      parked: HTMLElement[] = [],
      restored: Array<[HTMLElement, HTMLElement]> = [],
      disposals = new Map<HTMLElement, ReturnType<typeof vi.fn>>();
    let pending: TerminalFocusRequest | undefined;
    mount.mockImplementation((element) => {
      const dispose = vi.fn();
      disposals.set(element, dispose);
      return dispose;
    });
    const owner = createTerminalViewportsController({
      projects,
      get pendingTerminalFocus() {
        return pending;
      },
      set pendingTerminalFocus(value) {
        pending = value;
      },
      openTicketReference: vi.fn(),
      parking: {
        park: (element) => parked.push(element),
        restore: (placeholder, element) => restored.push([placeholder, element]),
      },
    });
    const show = (...next: HTMLElement[]) => {
      elements = next;
      owner.syncTerminalViewportMounts();
    };
    return {
      projects,
      parked,
      restored,
      disposals,
      show,
      parkProjectViewports: owner.parkProjectViewports,
      focus: (request: TerminalFocusRequest | undefined) => (pending = request),
      pending: () => pending,
    };
  }

  it('parks a switched-away project terminal and restores the same live viewport on return', async () => {
    const { parked, restored, disposals, show } = setup();
    const a1 = keepAlive('a', 'one');
    show(a1);
    expect(mount).toHaveBeenCalledTimes(1);

    // Switch to project b: a's viewport is parked, not disposed, and told to release resources.
    const b1 = keepAlive('b', 'one');
    show(b1);
    await paint();
    expect(parked).toEqual([a1]);
    expect(a1.dataset.parked).toBe('true');
    expect(a1.events).toContain(TERMINAL_VIEWPORT_PARK_EVENT);
    expect(disposals.get(a1)).not.toHaveBeenCalled();

    // Back to a: the fresh placeholder is replaced by the warm viewport; nothing new mounts.
    const a1Again = keepAlive('a', 'one');
    show(a1Again);
    expect(restored).toEqual([[a1Again, a1]]);
    expect(a1.dataset.parked).toBeUndefined();
    expect(a1.events.at(-1)).toBe(TERMINAL_VIEWPORT_RESUME_EVENT);
    expect(mount).toHaveBeenCalledTimes(2);
    expect(parked).toEqual([a1, b1]);

    // The restored viewport is owned again: leaving it once more parks it again (repeat cycle).
    elements = [a1];
    show(a1, keepAlive('b', 'one'));
    expect(restored.at(-1)?.[1]).toBe(b1);
    show(b1);
    expect(parked.at(-1)).toBe(a1);
    await paint();
    for (const dispose of disposals.values()) expect(dispose).not.toHaveBeenCalled();
  });

  it('parks outgoing live viewports before a project change, once per transition', () => {
    const { parked, restored, disposals, show, parkProjectViewports } = setup();
    const a1 = keepAlive('a', 'one'),
      a2 = keepAlive('a', 'two'),
      b1 = keepAlive('b', 'one');
    show(a1, a2);
    parkProjectViewports('b');
    expect(parked).toEqual([]);
    parkProjectViewports('a');
    parkProjectViewports('a');
    expect(parked).toEqual([a1, a2]);
    expect(a1.events).toEqual([TERMINAL_VIEWPORT_PARK_EVENT]);
    expect(a2.events).toEqual([TERMINAL_VIEWPORT_PARK_EVENT]);
    show(b1);
    expect(parked).toEqual([a1, a2]);
    expect(disposals.get(a1)).not.toHaveBeenCalled();
    expect(disposals.get(a2)).not.toHaveBeenCalled();

    const replacement = keepAlive('a', 'one');
    show(replacement);
    expect(restored).toContainEqual([replacement, a1]);
    expect(a1.events.at(-1)).toBe(TERMINAL_VIEWPORT_RESUME_EVENT);
    parkProjectViewports('a');
    expect(parked).toEqual([a1, a2, b1, a1]);
    expect(disposals.get(a1)).not.toHaveBeenCalled();
  });

  it('asks a restored viewport for focus only when a pending request names it', () => {
    const { show, focus, pending } = setup();
    const a1 = keepAlive('a', 'one');
    show(a1);
    show(keepAlive('b', 'one'));
    focus({ projectId: 'a', terminalId: 'one' });
    const resumed: unknown[] = [];
    a1.addEventListener(TERMINAL_VIEWPORT_RESUME_EVENT, (event) => resumed.push((event as CustomEvent).detail));
    show(keepAlive('a', 'one'));
    expect(resumed).toEqual([{ focus: true }]);
    expect(pending()).toBeUndefined();

    show(keepAlive('b', 'one'));
    show(keepAlive('a', 'one'));
    expect(resumed).toEqual([{ focus: true }, { focus: false }]);
  });

  it('evicts parked viewports when their project closes or their socket closes', () => {
    const { projects, disposals, show } = setup(['a', 'b', 'c']);
    const a1 = keepAlive('a', 'one'),
      b1 = keepAlive('b', 'one');
    show(a1);
    show(b1);
    show(keepAlive('c', 'one'));
    expect(a1.dataset.parked).toBe('true');
    expect(b1.dataset.parked).toBe('true');

    // Closing project a disposes its warm viewport on the next sync.
    projects.value = [project('b'), project('c')];
    show(...elements);
    expect(disposals.get(a1)).toHaveBeenCalledTimes(1);
    expect(a1.removed).toBe(true);

    // A parked socket that closes is evicted instead of reconnecting in the background.
    b1.dispatchEvent(new CustomEvent(TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT));
    expect(disposals.get(b1)).toHaveBeenCalledTimes(1);
    const b1Again = keepAlive('b', 'one');
    show(b1Again);
    expect(mount).toHaveBeenLastCalledWith(b1Again, expect.any(Object));

    // A late close event from an evicted viewport is inert.
    b1.dispatchEvent(new CustomEvent(TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT));
    expect(disposals.get(b1)).toHaveBeenCalledTimes(1);
  });

  it('bounds how many viewports stay warm, evicting the oldest parked first', () => {
    const ids = Array.from({ length: MAX_PARKED_TERMINAL_VIEWPORTS + 2 }, (_, index) => `p${index}`);
    const { disposals, show } = setup([...ids, 'last']);
    const first = keepAlive('p0', 'one'),
      second = keepAlive('p1', 'one');
    show(first);
    show(second);
    for (const id of ids.slice(2)) show(keepAlive(id, 'one'));
    show(keepAlive('last', 'one'));
    expect(disposals.get(first)).toHaveBeenCalledTimes(1);
    expect(disposals.get(second)).toHaveBeenCalledTimes(1);
    const warm = [...disposals].filter(([, dispose]) => dispose.mock.calls.length === 0);
    expect(warm).toHaveLength(MAX_PARKED_TERMINAL_VIEWPORTS + 1); // parked plus the visible one
  });

  it('keeps disposing viewports that are not kept alive', async () => {
    const { parked, disposals, show } = setup();
    const preview = viewport('a', 'one');
    show(preview);
    show();
    await paint();
    expect(parked).toEqual([]);
    expect(disposals.get(preview)).toHaveBeenCalledTimes(1);
  });
});
