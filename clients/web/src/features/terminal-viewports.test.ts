import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { Project } from '../interactions/types';
import {
  mountTerminalViewport,
  TERMINAL_VIEWPORT_PARK_EVENT,
  TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT,
  TERMINAL_VIEWPORT_REPARENT_EVENT,
  TERMINAL_VIEWPORT_RESUME_EVENT,
  type TerminalFocusRequest,
} from '../terminal-viewport';
import { createTerminalViewportsController, terminalViewportConfiguration } from './terminal-viewports';

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
    classList: { contains: () => false },
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
  function setup(projectIds = ['a', 'b'], warmLimit = { value: 12 }) {
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
      warmTerminalLimit: () => warmLimit.value,
    });
    const show = (...next: HTMLElement[]) => {
      elements = next;
      owner.syncTerminalViewportMounts();
    };
    return {
      owner,
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
    const limit = 12,
      ids = Array.from({ length: limit + 2 }, (_, index) => `p${index}`);
    const { disposals, show } = setup([...ids, 'last'], { value: limit });
    const first = keepAlive('p0', 'one'),
      second = keepAlive('p1', 'one');
    show(first);
    show(second);
    for (const id of ids.slice(2)) show(keepAlive(id, 'one'));
    show(keepAlive('last', 'one'));
    expect(disposals.get(first)).toHaveBeenCalledTimes(1);
    expect(disposals.get(second)).toHaveBeenCalledTimes(1);
    const warm = [...disposals].filter(([, dispose]) => dispose.mock.calls.length === 0);
    expect(warm).toHaveLength(limit + 1); // parked plus the visible one
  });

  it('follows the configured warm-terminal limit, applying a lowered limit at once (HS2-Y5DYC0)', () => {
    const warmLimit = { value: 3 },
      { disposals, show, owner } = setup(['p0', 'p1', 'p2', 'p3', 'p4'], warmLimit);
    const views = ['p0', 'p1', 'p2', 'p3', 'p4'].map((id) => keepAlive(id, 'one'));
    for (const view of views) show(view);
    show();
    // Five parked against a limit of three: the two oldest were evicted.
    expect(views.map((view) => disposals.get(view)?.mock.calls.length)).toEqual([1, 1, 0, 0, 0]);
    warmLimit.value = 1;
    owner.enforceWarmTerminalLimit();
    expect(views.map((view) => disposals.get(view)?.mock.calls.length)).toEqual([1, 1, 1, 1, 0]);
    // A limit of zero keeps nothing warm: the next parked viewport is disposed at once.
    warmLimit.value = 0;
    owner.enforceWarmTerminalLimit();
    expect(disposals.get(views[4])).toHaveBeenCalledTimes(1);
    const again = keepAlive('p0', 'one');
    show(again);
    show();
    expect(disposals.get(again)).toHaveBeenCalledTimes(1);
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

describe('live grid viewports survive a re-rendered container (HS2-AMNNBR, HS2-TBQQ00)', () => {
  type Grid = HTMLElement & { events: string[]; replacedBy?: HTMLElement; replacing?: HTMLElement };
  /** A scaled grid preview as the dashboard and drawer grid render it, in a (possibly magnified) card. */
  function gridViewport(terminalId: string, mode: 'preview' | 'magnified' = 'preview', projectId = 'a'): Grid {
    const target = new EventTarget() as unknown as Grid;
    const dispatch = target.dispatchEvent.bind(target);
    const preview = mode === 'preview';
    Object.assign(target, {
      isConnected: true,
      events: [] as string[],
      dataset: {
        projectId,
        terminalId,
        displayMode: preview ? 'scaled-preview' : 'interactive',
        mountPolicy: preview ? 'visible-progressive' : 'immediate',
        gridPolicy: preview ? 'dashboard-80x24' : undefined,
        mobileGridPolicy: preview ? undefined : '80xm',
      },
      classList: { contains: (name: string) => preview && name === 'terminal-viewport--scaled-preview' },
      closest: (selector: string) =>
        !preview && selector === '[data-fixed-aspect-terminal-card="magnified"]' ? target : null,
      replaceWith(live: Grid) {
        target.replacedBy = live;
        live.replacing = target;
      },
      dispatchEvent(event: Event) {
        target.events.push(event.type);
        return dispatch(event);
      },
    });
    return target;
  }
  function setup() {
    const disposals = new Map<HTMLElement, ReturnType<typeof vi.fn>>();
    mount.mockImplementation((element) => {
      const dispose = vi.fn();
      disposals.set(element, dispose);
      return dispose;
    });
    const owner = createTerminalViewportsController({
      projects: signal([project('a'), project('b')]),
      pendingTerminalFocus: undefined,
      openTicketReference: vi.fn(),
    });
    const show = async (...next: HTMLElement[]) => {
      elements = next;
      owner.syncTerminalViewportMounts();
      // Progressive work mounts two per turn; drain it.
      for (let turn = 0; turn < 4; turn += 1) await paint();
    };
    const live = () => [...disposals].filter(([, dispose]) => dispose.mock.calls.length === 0).map(([e]) => e);
    return { disposals, show, live };
  }

  it('keys adoption by display mode, policies, presentation class and magnified card', () => {
    expect(terminalViewportConfiguration(gridViewport('one'))).toBe(
      'scaled-preview|visible-progressive|dashboard-80x24|||scaled|',
    );
    expect(terminalViewportConfiguration(gridViewport('one', 'magnified'))).toBe(
      'interactive|immediate||80xm|||magnified',
    );
  });

  it('walks mount -> workspace grid -> full screen -> exit -> project grid without recreating a terminal', async () => {
    const { disposals, show, live } = setup();
    // Six mounted previews in one project's grid.
    const ids = ['1', '2', '3', '4', '5', '6'];
    let current = ids.map((id) => gridViewport(id));
    await show(...current);
    expect(mount).toHaveBeenCalledTimes(6);
    const originals = [...current];
    // Each later step re-renders every tile's container, leaving fresh placeholders.
    for (const step of ['workspace grid', 'full screen', 'exit full screen', 'project grid']) {
      const placeholders = ids.map((id) => gridViewport(id));
      await show(...placeholders);
      expect(mount, step).toHaveBeenCalledTimes(6);
      for (const [index, placeholder] of placeholders.entries()) {
        expect(placeholder.replacedBy, step).toBe(originals[index]);
        expect(originals[index].events.at(-1), step).toBe(TERMINAL_VIEWPORT_REPARENT_EVENT);
      }
      current = placeholders;
    }
    expect(live()).toEqual(originals);
    // The adopted viewport now owns the slot: a re-sync with it in the page keeps it, and removing it
    // finally disposes the same emulator once.
    await show(...originals);
    expect(mount).toHaveBeenCalledTimes(6);
    await show();
    for (const original of originals) expect(disposals.get(original)).toHaveBeenCalledTimes(1);
    expect(current).toHaveLength(6);
  });

  it('adopts only a placeholder with the same identity and configuration', async () => {
    const { disposals, show } = setup();
    const preview = gridViewport('one');
    await show(preview);
    // A different terminal, another project's same id, and the magnified view do not adopt it.
    const other = gridViewport('two'),
      otherProject = gridViewport('one', 'preview', 'b'),
      magnified = gridViewport('one', 'magnified');
    await show(other, otherProject, magnified);
    expect(other.replacedBy ?? otherProject.replacedBy ?? magnified.replacedBy).toBeUndefined();
    expect(disposals.get(preview)).toHaveBeenCalledTimes(1);
    expect(mount).toHaveBeenCalledWith(magnified, expect.any(Object));
    // Dismissing the magnified view mounts a fresh preview (HS2-ACMRF6), which a later re-render adopts.
    const restored = gridViewport('one');
    await show(restored);
    expect(disposals.get(magnified)).toHaveBeenCalledTimes(1);
    const rerendered = gridViewport('one');
    await show(rerendered);
    expect(rerendered.replacedBy).toBe(restored);
    expect(disposals.get(restored)).not.toHaveBeenCalled();
  });

  it('adopts each live viewport at most once and mounts duplicates or late refills fresh', async () => {
    const { disposals, show } = setup();
    const preview = gridViewport('one');
    await show(preview);
    // Two placeholders for one terminal: one adopts, the other mounts its own viewer.
    const first = gridViewport('one'),
      second = gridViewport('one');
    await show(first, second);
    expect([first.replacedBy, second.replacedBy].filter(Boolean)).toEqual([preview]);
    expect(mount).toHaveBeenCalledTimes(2);
    // Empty then refill across two syncs: the emptied viewport is disposed, the refill mounts fresh.
    await show();
    const refill = gridViewport('one');
    await show(refill);
    expect(refill.replacedBy).toBeUndefined();
    expect(disposals.get(preview)).toHaveBeenCalledTimes(1);
    expect(mount).toHaveBeenCalledTimes(3);
  });
});

describe('one live instance moves between the drawer and grid tiles (HS2-EYD24K)', () => {
  /** A minimal DOM node: enough tree, dataset, style, and event behavior for the controller's moves. */
  class FakeNode extends EventTarget {
    parentElement: FakeNode | null = null;
    children: FakeNode[] = [];
    style: Record<string, string> = {};
    classList = { contains: () => false };
    events: string[] = [];
    constructor(
      readonly dataset: Record<string, string>,
      readonly root = false,
    ) {
      super();
    }
    get isConnected(): boolean {
      return this.root || (this.parentElement?.isConnected ?? false);
    }
    private detach() {
      const parent = this.parentElement;
      if (parent) parent.children.splice(parent.children.indexOf(this), 1);
      this.parentElement = null;
    }
    append(child: FakeNode) {
      child.detach();
      child.parentElement = this;
      this.children.push(child);
    }
    replaceWith(other: FakeNode) {
      const parent = this.parentElement!;
      other.detach();
      parent.children[parent.children.indexOf(this)] = other;
      other.parentElement = parent;
      this.parentElement = null;
    }
    remove() {
      this.detach();
    }
    removeAttribute(name: string) {
      if (name === 'style') this.style = {};
    }
    closest(selector: string): FakeNode | null {
      const match = /^\[data-([\w-]+)="([^"]*)"\]$/.exec(selector);
      if (!match) return null;
      const key = match[1].replace(/-(\w)/g, (_, letter: string) => letter.toUpperCase());
      if (this.dataset[key] === match[2]) return this;
      return this.parentElement?.closest(selector) ?? null;
    }
    override dispatchEvent(event: Event): boolean {
      const detail = (event as CustomEvent<{ tile?: boolean } | null>).detail;
      this.events.push(detail && 'tile' in detail ? `${event.type}:${String(detail.tile)}` : event.type);
      return super.dispatchEvent(event);
    }
    *walk(): Generator<FakeNode> {
      for (const child of this.children) {
        yield child;
        yield* child.walk();
      }
    }
  }
  const asElement = (node: FakeNode) => node as unknown as HTMLElement;

  function setup(warmLimit = { value: 12 }) {
    const root = new FakeNode({}, true),
      holder = new FakeNode({}),
      disposals = new Map<HTMLElement, ReturnType<typeof vi.fn>>();
    mount.mockImplementation((element) => {
      const dispose = vi.fn();
      disposals.set(element, dispose);
      return dispose;
    });
    vi.stubGlobal('document', {
      querySelectorAll: () => [...root.walk()].filter((node) => node.dataset.component === 'terminal-viewport'),
    });
    const owner = createTerminalViewportsController({
      projects: signal([project('a')]),
      pendingTerminalFocus: undefined,
      openTicketReference: vi.fn(),
      parking: {
        park: (element) => {
          holder.append(element as unknown as FakeNode);
        },
        restore: (placeholder, element) => {
          (placeholder as unknown as FakeNode).replaceWith(element as unknown as FakeNode);
        },
      },
      warmTerminalLimit: () => warmLimit.value,
      createTileViewport: (host) =>
        asElement(
          new FakeNode({
            component: 'terminal-viewport',
            projectId: host.dataset.projectId ?? '',
            terminalId: host.dataset.terminalId ?? '',
            displayMode: 'interactive',
            mountPolicy: 'keep-alive',
          }),
        ),
    });
    /** The drawer's dedicated, kept-alive viewport for `id`, inside a fresh terminal session. */
    const drawer = (id: string) => {
      const session = new FakeNode({ component: 'terminal-session' }),
        dedicated = new FakeNode({
          component: 'terminal-viewport',
          projectId: 'a',
          terminalId: id,
          displayMode: 'interactive',
          mountPolicy: 'keep-alive',
        });
      session.append(dedicated);
      root.append(session);
      return { session, dedicated };
    };
    /** A grid tile's progressive preview placeholder for `id`, optionally in a magnified card. */
    const tile = (id: string, magnified = false) => {
      const card = new FakeNode({
          component: 'terminal-tile',
          ...(magnified ? { fixedAspectTerminalCard: 'magnified' } : {}),
        }),
        frame = new FakeNode({}),
        preview = new FakeNode({
          component: 'terminal-viewport',
          projectId: 'a',
          terminalId: id,
          displayMode: 'scaled-preview',
          mountPolicy: 'visible-progressive',
        });
      frame.append(preview);
      card.append(frame);
      root.append(card);
      return { card, preview };
    };
    const sync = () => {
      owner.syncTerminalViewportMounts();
    };
    const clear = () => {
      for (const child of [...root.children]) child.remove();
    };
    return { root, holder, disposals, owner, drawer, tile, sync, clear };
  }

  it('mounts a never-opened visible tile as a full instance and moves it to and from the drawer', async () => {
    const view = setup({ value: 1 }),
      grid = view.tile('x');
    view.sync();
    await paint();
    const instance = grid.preview.children[0];
    expect(instance.dataset).toMatchObject({ displayMode: 'interactive', mountPolicy: 'keep-alive' });
    expect(grid.preview.dataset.hostingInstance).toBe('true');
    expect(mount).toHaveBeenCalledTimes(1);
    expect(mount.mock.calls[0][1]).toMatchObject({ initialTilePresentation: true, autoFocus: false });

    const back = view.drawer('x');
    view.sync();
    expect(instance.parentElement).toBe(back.session);
    expect(instance.events.slice(-2)).toEqual([
      'hs-terminal-viewport-presentation:false',
      'hs-terminal-viewport-resume',
    ]);
    await paint();
    expect(mount.mock.calls.at(-1)?.[0]).toBe(asElement(grid.preview));
    expect(view.disposals.get(asElement(instance))).not.toHaveBeenCalled();

    back.session.remove();
    view.sync();
    expect(instance.parentElement).toBe(grid.preview);
    expect(view.disposals.get(asElement(grid.preview))).toHaveBeenCalledTimes(1);
    expect(view.disposals.get(asElement(instance))).not.toHaveBeenCalled();
  });

  it('falls back beyond the cap, then promotes an overflow preview when a slot opens', async () => {
    const warm = { value: 2 },
      view = setup(warm),
      x = view.tile('x'),
      y = view.tile('y'),
      z = view.tile('z');
    view.sync();
    await paint();
    await paint();
    expect(x.preview.children).toHaveLength(1);
    expect(y.preview.children).toHaveLength(1);
    expect(z.preview.children).toHaveLength(0);
    const firstInstance = x.preview.children[0];
    expect(mount.mock.calls.map(([element]) => element)).toEqual([
      asElement(x.preview.children[0]),
      asElement(y.preview.children[0]),
      asElement(z.preview),
    ]);

    // Lowering the cap evicts the oldest live tile, leaving its preview in place.
    warm.value = 1;
    view.owner.enforceWarmTerminalLimit();
    expect(view.disposals.get(asElement(firstInstance))).toHaveBeenCalledTimes(1);
    expect(x.preview.children).toHaveLength(0);
    await paint();
    expect(x.preview.dataset.hostingInstance).toBeUndefined();

    // Raising it again promotes the first overflow preview without a drawer visit.
    warm.value = 2;
    view.owner.enforceWarmTerminalLimit();
    expect(z.preview.children).toHaveLength(1);
    expect(z.preview.dataset.hostingInstance).toBe('true');
    expect(x.preview.children).toHaveLength(0);
    expect(view.disposals.get(asElement(z.preview))).toHaveBeenCalledTimes(1);
  });

  it('walks drawer → grid → drawer → grid on one emulator and socket, never remounting', async () => {
    const view = setup(),
      first = view.drawer('x');
    view.sync();
    expect(mount).toHaveBeenCalledTimes(1);
    const instance = first.dedicated;

    for (let round = 0; round < 3; round += 1) {
      // The drawer switches to its grid tab: the session leaves and a tile for the same terminal appears.
      view.clear();
      const grid = view.tile('x');
      view.sync();
      expect(instance.parentElement, `round ${String(round)}: hosted in the tile`).toBe(grid.preview);
      expect(grid.preview.dataset.hostingInstance).toBe('true');
      expect(grid.preview.style).toMatchObject({ width: '100%', height: '100%', display: 'block' });
      expect(instance.events.at(-1)).toBe('hs-terminal-viewport-presentation:true');
      await paint();
      expect(mount, 'the tile never mounts its own preview').toHaveBeenCalledTimes(1);

      // Back to the terminal tab: the drawer takes the same instance back.
      view.clear();
      const back = view.drawer('x');
      view.sync();
      expect(instance.parentElement).toBe(back.session);
      expect(instance.events.slice(-2)).toEqual([
        'hs-terminal-viewport-presentation:false',
        'hs-terminal-viewport-resume',
      ]);
      await paint();
      expect(mount).toHaveBeenCalledTimes(1);
    }
    expect(view.disposals.get(asElement(instance))).not.toHaveBeenCalled();
  });

  it('gives the instance to the drawer while a still-visible tile falls back to its lightweight preview', async () => {
    const view = setup(),
      first = view.drawer('x');
    view.sync();
    const instance = first.dedicated;
    view.clear();
    const grid = view.tile('x');
    view.sync();
    expect(instance.parentElement).toBe(grid.preview);

    // A Workspace Grid tile stays on screen while the drawer shows the terminal again.
    const back = view.drawer('x');
    view.sync();
    expect(instance.parentElement).toBe(back.session);
    expect(grid.preview.dataset.hostingInstance).toBeUndefined();
    expect(grid.preview.style).toEqual({});
    await paint();
    expect(mount).toHaveBeenCalledTimes(2);
    expect(mount.mock.calls[1][0]).toBe(asElement(grid.preview));

    // The drawer closes again: the tile's lightweight preview gives way to the instance.
    back.session.remove();
    view.sync();
    expect(view.disposals.get(asElement(grid.preview))).toHaveBeenCalledTimes(1);
    expect(instance.parentElement).toBe(grid.preview);
  });

  it('prefers the drawer when the drawer and a tile both ask for a parked instance in one pass', () => {
    const view = setup(),
      first = view.drawer('x');
    view.sync();
    const instance = first.dedicated;
    first.session.remove();
    view.sync();
    expect(instance.parentElement).toBe(view.holder);
    const grid = view.tile('x'),
      back = view.drawer('x');
    view.sync();
    expect(instance.parentElement).toBe(back.session);
    expect(grid.preview.dataset.hostingInstance).toBeUndefined();
  });

  it('parks a hosted instance when its tile leaves, and never hosts in a magnified card', async () => {
    const view = setup(),
      first = view.drawer('x');
    view.sync();
    const instance = first.dedicated;
    view.clear();
    const grid = view.tile('x');
    view.sync();
    grid.card.remove();
    view.sync();
    expect(instance.parentElement).toBe(view.holder);
    expect(instance.dataset.parked).toBe('true');
    const magnified = view.tile('x', true);
    view.sync();
    expect(instance.parentElement).toBe(view.holder);
    await paint();
    expect(mount.mock.calls.at(-1)?.[0]).toBe(asElement(magnified.preview));
  });

  it('counts tile-hosted instances in the warm limit, returning the oldest tile to its preview', async () => {
    const warm = { value: 2 },
      view = setup(warm),
      x = view.drawer('x'),
      y = view.drawer('y');
    view.sync();
    view.clear();
    const tileX = view.tile('x'),
      tileY = view.tile('y');
    view.sync();
    expect(x.dedicated.parentElement).toBe(tileX.preview);
    expect(y.dedicated.parentElement).toBe(tileY.preview);
    warm.value = 1;
    view.owner.enforceWarmTerminalLimit();
    // The oldest hosted instance is disposed and its tile mounts the lightweight preview.
    expect(view.disposals.get(asElement(x.dedicated))).toHaveBeenCalledTimes(1);
    expect(x.dedicated.parentElement).toBeNull();
    expect(y.dedicated.parentElement).toBe(tileY.preview);
    await paint();
    expect(mount.mock.calls.at(-1)?.[0]).toBe(asElement(tileX.preview));
    // A zero limit keeps nothing: the other tile falls back too.
    warm.value = 0;
    view.owner.enforceWarmTerminalLimit();
    expect(view.disposals.get(asElement(y.dedicated))).toHaveBeenCalledTimes(1);
  });
});
