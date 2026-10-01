import { describe, expect, it } from 'vitest';

import { createTrackedSizeObserver } from './tracked-size-observer';

type FakeElement = HTMLElement & { isConnected: boolean; name: string };

function element(name: string): FakeElement {
  return { name, isConnected: true } as unknown as FakeElement;
}

/** A ResizeObserver double that records bindings and lets the test deliver entries. */
function harness(options: { enabled?: () => boolean; paused?: () => boolean } = {}) {
  const observers: Array<{ callback: ResizeObserverCallback; observed: HTMLElement[]; disconnected: boolean }> = [];
  let slot: FakeElement | undefined;
  const sizes: Array<{ width: number; height: number; target: string }> = [];
  const tracked = createTrackedSizeObserver({
    find: () => slot,
    onSize: (size, target) => sizes.push({ ...size, target: (target as FakeElement).name }),
    ...options,
    createObserver: (callback) => {
      const record = { callback, observed: [] as HTMLElement[], disconnected: false };
      observers.push(record);
      return {
        observe: (target: Element) => {
          record.observed.push(target as HTMLElement);
        },
        disconnect: () => {
          record.disconnected = true;
        },
      };
    },
  });
  const live = () => observers.filter((record) => !record.disconnected);
  const report = (target: HTMLElement, width: number, height: number, record = live().at(-1)) => {
    record?.callback(
      [{ target, contentRect: { width, height } } as unknown as ResizeObserverEntry],
      {} as ResizeObserver,
    );
  };
  return {
    tracked,
    observers,
    live,
    sizes,
    report,
    render: (next: FakeElement | undefined) => {
      slot = next;
    },
  };
}

describe('tracked size observer (HS2-0PF13V)', () => {
  it('binds the rendered element and reports its floored size', () => {
    const h = harness(),
      drawer = element('drawer-1');
    h.render(drawer);
    h.tracked.sync();
    expect(h.tracked.target()).toBe(drawer);
    expect(h.live()).toHaveLength(1);
    expect(h.live()[0].observed).toEqual([drawer]);
    h.report(drawer, 816.7, 262.4);
    expect(h.sizes).toEqual([{ width: 816, height: 262, target: 'drawer-1' }]);
  });

  it('keeps one binding across repeated syncs of the same element', () => {
    const h = harness(),
      drawer = element('drawer-1');
    h.render(drawer);
    for (let index = 0; index < 4; index++) h.tracked.sync();
    expect(h.observers).toHaveLength(1);
    expect(h.live()).toHaveLength(1);
  });

  it('re-binds when a re-render replaces the element and follows the new node only', () => {
    const h = harness(),
      first = element('drawer-1'),
      second = element('drawer-2');
    h.render(first);
    h.tracked.sync();
    const stale = h.live()[0];
    // A view switch unmounts the drawer: the old node is detached and reports 0x0.
    first.isConnected = false;
    h.render(undefined);
    h.report(first, 0, 0, stale);
    h.tracked.sync();
    expect(h.tracked.target()).toBeUndefined();
    expect(stale.disconnected).toBe(true);
    // Returning renders a new drawer node, which is bound and measured.
    h.render(second);
    h.tracked.sync();
    expect(h.tracked.target()).toBe(second);
    h.report(second, 816, 262);
    expect(h.sizes).toEqual([{ width: 816, height: 262, target: 'drawer-2' }]);
  });

  it('re-binds when the element is swapped within one render (no unmounted frame)', () => {
    const h = harness(),
      first = element('drawer-1'),
      second = element('drawer-2');
    h.render(first);
    h.tracked.sync();
    first.isConnected = false;
    h.render(second);
    h.tracked.sync();
    expect(h.observers[0].disconnected).toBe(true);
    expect(h.live()[0].observed).toEqual([second]);
  });

  it('ignores zero-size reports from a hidden or detached element instead of recording them', () => {
    const h = harness(),
      drawer = element('drawer-1');
    h.render(drawer);
    h.tracked.sync();
    h.report(drawer, 816, 262);
    h.report(drawer, 0, 0);
    h.report(drawer, 816, 0);
    h.report(drawer, 0.4, 262);
    h.report(drawer, 700, 300);
    expect(h.sizes.map(({ width, height }) => `${width}x${height}`)).toEqual(['816x262', '700x300']);
  });

  it('ignores a late entry for a node it no longer tracks', () => {
    const h = harness(),
      first = element('drawer-1'),
      second = element('drawer-2');
    h.render(first);
    h.tracked.sync();
    h.render(second);
    h.tracked.sync();
    h.report(first, 500, 200, h.live()[0]);
    expect(h.sizes).toEqual([]);
  });

  it('drops reports while paused and resumes with the next report', () => {
    let dragging = false;
    const h = harness({ paused: () => dragging }),
      drawer = element('drawer-1');
    h.render(drawer);
    h.tracked.sync();
    dragging = true;
    h.report(drawer, 600, 200);
    dragging = false;
    h.report(drawer, 640, 210);
    expect(h.sizes).toEqual([{ width: 640, height: 210, target: 'drawer-1' }]);
  });

  it('releases while disabled and binds again when re-enabled (empty-then-refill)', () => {
    let open = true;
    const h = harness({ enabled: () => open }),
      drawer = element('drawer-1');
    h.render(drawer);
    h.tracked.sync();
    open = false;
    h.tracked.sync();
    expect(h.live()).toHaveLength(0);
    h.tracked.sync();
    expect(h.observers).toHaveLength(1);
    open = true;
    h.tracked.sync();
    expect(h.live()).toHaveLength(1);
    expect(h.tracked.target()).toBe(drawer);
  });

  it('binds again after an explicit disconnect even when the element is unchanged', () => {
    const h = harness(),
      drawer = element('drawer-1');
    h.render(drawer);
    h.tracked.sync();
    h.tracked.disconnect();
    expect(h.tracked.target()).toBeUndefined();
    h.tracked.sync();
    expect(h.observers).toHaveLength(2);
    expect(h.live()[0].observed).toEqual([drawer]);
  });

  it('does not bind a detached element', () => {
    const h = harness(),
      drawer = element('drawer-1');
    drawer.isConnected = false;
    h.render(drawer);
    h.tracked.sync();
    expect(h.observers).toHaveLength(0);
    expect(h.tracked.target()).toBeUndefined();
  });

  it('walks a bind, hide, remount, resize, remount sequence and reports only live sizes', () => {
    let open = true;
    const h = harness({ enabled: () => open }),
      nodes = [element('drawer-1'), element('drawer-2'), element('drawer-3')];
    h.render(nodes[0]);
    h.tracked.sync();
    h.report(nodes[0], 816, 262);
    open = false; // drawer hidden
    h.tracked.sync();
    open = true;
    h.render(nodes[1]); // reopened: a fresh node
    h.tracked.sync();
    h.report(nodes[1], 816, 262);
    h.report(nodes[1], 616, 262); // the window narrowed
    nodes[1].isConnected = false;
    h.render(undefined); // Settings view
    h.tracked.sync();
    h.render(nodes[2]); // back to the list
    h.tracked.sync();
    h.report(nodes[2], 616, 262);
    expect(h.sizes.map((size) => `${size.target}:${size.width}x${size.height}`)).toEqual([
      'drawer-1:816x262',
      'drawer-2:816x262',
      'drawer-2:616x262',
      'drawer-3:616x262',
    ]);
    expect(h.live()).toHaveLength(1);
  });
});
