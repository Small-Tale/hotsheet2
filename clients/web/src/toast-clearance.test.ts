import { describe, expect, it } from 'vitest';

import {
  createBrowserToastClearance,
  createToastClearance,
  findToastObstacle,
  GALLERY_FOOTER_SELECTOR,
  TERMINAL_DRAWER_RESTORE_SELECTOR,
  TOAST_CLEARANCE_PROPERTY,
  type ToastClearanceEnvironment,
  type ToastClearanceHost,
} from './toast-clearance';

function fakeHost() {
  const values = new Map<string, string>();
  const host: ToastClearanceHost = {
    style: {
      setProperty: (name: string, value: string | null) => {
        values.set(name, value ?? '');
      },
      removeProperty: (name: string) => {
        const previous = values.get(name) ?? '';
        values.delete(name);
        return previous;
      },
    },
  };
  return { host, value: () => values.get(TOAST_CLEARANCE_PROPERTY) ?? '' };
}

function strip(top: number, height: number) {
  let rect = { top, height };
  const node = { isConnected: true, getBoundingClientRect: () => rect };
  return {
    node,
    resize(nextTop: number, nextHeight: number) {
      rect = { top: nextTop, height: nextHeight };
    },
    remove() {
      node.isConnected = false;
      rect = { top: 0, height: 0 };
    },
  };
}
type Strip = ReturnType<typeof strip>['node'];

function harness(viewport = 900) {
  const { host, value } = fakeHost();
  let obstacle: Strip | null = null;
  const observers: { targets: Strip[]; callback: () => void; disconnected: boolean }[] = [];
  const environment: ToastClearanceEnvironment<Strip> = {
    findObstacle: () => obstacle,
    viewportHeight: () => viewport,
    observeResize: (callback) => {
      const record = { targets: [] as Strip[], callback, disconnected: false };
      observers.push(record);
      return {
        observe: (target) => record.targets.push(target),
        disconnect: () => {
          record.disconnected = true;
        },
      };
    },
  };
  return {
    clearance: createToastClearance(host, environment),
    setObstacle: (next: Strip | null) => {
      obstacle = next;
    },
    observers,
    live: () => observers.filter((observer) => !observer.disconnected),
    value,
    fireResize: () => {
      for (const observer of observers) if (!observer.disconnected) observer.callback();
    },
  };
}

describe('toast clearance (HS2-DSW9TY)', () => {
  it('leaves the stack at the viewport edge when nothing bottom-anchored is open', () => {
    const h = harness();
    h.clearance.track();
    expect(h.value()).toBe('');
    expect(h.observers).toHaveLength(0);
  });

  it('lifts the stack above the open gallery footer and follows its height changes', () => {
    const h = harness();
    const footer = strip(780, 120);
    h.setObstacle(footer.node);
    h.clearance.track();
    expect(h.value()).toBe('120px');
    // Finishing markup shrinks the footer while the toast still shows.
    footer.resize(836, 64);
    h.fireResize();
    expect(h.value()).toBe('64px');
    // A repeated show re-measures and keeps one observer on the same footer.
    footer.resize(820, 80);
    h.clearance.track();
    expect(h.value()).toBe('80px');
    expect(h.live()).toHaveLength(1);
  });

  it('returns to the viewport edge when the footer leaves or the toast hides, then re-tracks', () => {
    const h = harness();
    const footer = strip(820, 80);
    h.setObstacle(footer.node);
    h.clearance.track();
    expect(h.value()).toBe('80px');
    // The gallery closes under a showing toast: the removed footer reports no clearance.
    footer.remove();
    h.fireResize();
    expect(h.value()).toBe('');
    h.clearance.stop();
    expect(h.live()).toHaveLength(0);
    // Stopping twice is harmless.
    h.clearance.stop();
    // A later toast with a newly opened gallery measures the new footer.
    const next = strip(800, 100);
    h.setObstacle(next.node);
    h.clearance.track();
    expect(h.value()).toBe('100px');
    expect(h.observers.at(-1)?.targets).toEqual([next.node]);
    // A toast shown after the gallery closed disconnects the old observer and clears.
    h.setObstacle(null);
    h.clearance.track();
    expect(h.value()).toBe('');
    expect(h.live()).toHaveLength(0);
  });

  it('switches observers when a different strip replaces the first', () => {
    const h = harness();
    const first = strip(840, 60),
      second = strip(760, 140);
    h.setObstacle(first.node);
    h.clearance.track();
    h.setObstacle(second.node);
    h.clearance.track();
    expect(h.value()).toBe('140px');
    expect(h.live()).toHaveLength(1);
    expect(h.live()[0]?.targets).toEqual([second.node]);
  });

  it('ignores a collapsed strip and never reserves negative space', () => {
    const h = harness(600);
    const footer = strip(700, 0);
    h.setObstacle(footer.node);
    h.clearance.track();
    expect(h.value()).toBe('');
    footer.resize(650, 40);
    h.fireResize();
    expect(h.value()).toBe('');
  });

  it('measures against the window and follows the strip with a ResizeObserver in the browser form', () => {
    const observed: unknown[] = [];
    const globals = globalThis as { window?: unknown; ResizeObserver?: unknown };
    const originalWindow = globals.window,
      originalObserver = globals.ResizeObserver;
    const listeners = new Set<() => void>();
    const fakeWindow = {
      innerHeight: 844,
      addEventListener: (type: string, listener: () => void) => {
        if (type === 'resize') listeners.add(listener);
      },
      removeEventListener: (type: string, listener: () => void) => {
        if (type === 'resize') listeners.delete(listener);
      },
    };
    globals.window = fakeWindow;
    globals.ResizeObserver = class {
      observe(target: unknown) {
        observed.push(target);
      }
      disconnect() {}
    };
    try {
      const { host, value } = fakeHost();
      const footer = strip(794, 50);
      const clearance = createBrowserToastClearance(host, () => footer.node as unknown as Element);
      clearance.track();
      expect(observed).toEqual([footer.node]);
      expect(value()).toBe('50px');
      // A window resize moves a bottom-anchored obstacle without resizing it (HS2-T90WCZ).
      fakeWindow.innerHeight = 900;
      footer.resize(850, 50);
      for (const listener of listeners) listener();
      expect(value()).toBe('50px');
      footer.resize(700, 50);
      for (const listener of listeners) listener();
      expect(value()).toBe('200px');
      clearance.stop();
      expect(listeners.size).toBe(0);
      expect(value()).toBe('');
    } finally {
      globals.window = originalWindow;
      globals.ResizeObserver = originalObserver;
    }
  });
});

describe('toast obstacle (HS2-T90WCZ)', () => {
  const root = (present: Record<string, string>) => ({
    querySelector: (selector: string) => present[selector] ?? null,
  });

  it('clears the collapsed terminal drawer restore button when no gallery is open', () => {
    expect(findToastObstacle(root({ [TERMINAL_DRAWER_RESTORE_SELECTOR]: 'restore' }))).toBe('restore');
  });

  it('prefers the open gallery footer, which covers the restore button beneath it', () => {
    expect(
      findToastObstacle(root({ [GALLERY_FOOTER_SELECTOR]: 'footer', [TERMINAL_DRAWER_RESTORE_SELECTOR]: 'restore' })),
    ).toBe('footer');
  });

  it('finds nothing while the drawer is open and no gallery shows', () => {
    expect(findToastObstacle(root({}))).toBeNull();
  });

  it('then lifts the stack above the restore button and drops it once the drawer opens', () => {
    const { host, value } = fakeHost();
    const button = strip(844, 40);
    let collapsed = true;
    const clearance = createToastClearance(host, {
      findObstacle: () =>
        findToastObstacle({
          querySelector: (selector) =>
            collapsed && selector === TERMINAL_DRAWER_RESTORE_SELECTOR ? button.node : null,
        }),
      viewportHeight: () => 900,
      observeResize: () => ({ observe() {}, disconnect() {} }),
    });
    clearance.track();
    expect(value()).toBe('56px');
    collapsed = false;
    clearance.track();
    expect(value()).toBe('');
  });
});
