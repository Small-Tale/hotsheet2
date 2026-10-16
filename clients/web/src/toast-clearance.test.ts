import { describe, expect, it } from 'vitest';

import {
  createBrowserToastClearance,
  createToastClearance,
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
    globals.window = { innerHeight: 844 };
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
      clearance.stop();
      expect(value()).toBe('');
    } finally {
      globals.window = originalWindow;
      globals.ResizeObserver = originalObserver;
    }
  });
});
