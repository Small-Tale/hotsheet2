import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { LiveCiContextMenu, type LiveCiMenuState } from './live-ci-context-menu';
import { liveCiMenuSize, liveCiMenuTargets, wireLiveCiMenu } from './live-ci-menu-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: unknown, target: unknown) => unknown;
  attached: boolean;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    const entry = { event, selector, handle, attached: true };
    registrations.push(entry);
    return () => {
      entry.attached = false;
    };
  },
}));

const handler = (event: string, selector: string) => {
  const found = registrations.find((item) => item.event === event && item.selector === selector);
  expect(found, `${event} ${selector}`).toBeDefined();
  return found!.handle;
};
const contextmenu = () => handler('contextmenu', '[data-component="project-live-section"]');
const clearClick = () => handler('click', COMMANDS_AND_AI_ACTIONS.clearLiveCi.selector);

/** A fake root: records its own listeners so dismissal can be driven. */
function fakeRoot() {
  const listeners = new Map<string, (event: unknown) => void>();
  return {
    listeners,
    addEventListener: (type: string, listener: (event: unknown) => void) => listeners.set(type, listener),
    removeEventListener: (type: string) => listeners.delete(type),
  } as unknown as HTMLElement & { listeners: Map<string, (event: unknown) => void> };
}

/** One element naming a shown run, as a chip or row carries it. */
const runElement = (dataset: Record<string, string>) => ({ dataset, closest: () => null }) as unknown as HTMLElement;
const CI_RUN = {
  ciClearCheckout: 'hotsheet2-56cb',
  ciClearReportedAtMs: '1795693115028',
  ciClearBranch: 'main',
  ciClearWorkflow: 'CI',
  ciClearLabel: 'main CI',
};
const PLATFORMS_RUN = { ...CI_RUN, ciClearWorkflow: 'Platforms', ciClearLabel: 'main Platforms' };
const CI_TARGET = {
  checkout: 'hotsheet2-56cb',
  reportedAtMs: 1_795_693_115_028,
  branch: 'main',
  workflow: 'CI',
  label: 'main CI',
};
const PLATFORMS_TARGET = { ...CI_TARGET, workflow: 'Platforms', label: 'main Platforms' };

/** A fake section holding `runs`; `contains` answers for its own run elements. */
function section(runs: HTMLElement[], rect = { left: 24, bottom: 300 }) {
  return {
    dataset: {},
    getBoundingClientRect: () => rect,
    contains: (node: unknown) => runs.includes(node as HTMLElement) || node === 'focused',
    querySelectorAll: () => runs,
  } as unknown as HTMLElement;
}
/** A press on `on`: a run element's own chip, or a bare spot of the section. */
const event = (clientX: number, clientY: number, on?: HTMLElement) => ({
  clientX,
  clientY,
  target: { closest: () => on ?? null },
  defaultPrevented: false,
  propagationStopped: false,
  preventDefault() {
    this.defaultPrevented = true;
  },
  stopPropagation() {
    this.propagationStopped = true;
  },
});

beforeEach(() => {
  registrations.length = 0;
  vi.stubGlobal('window', { innerWidth: 1280, innerHeight: 800 });
  vi.stubGlobal('document', { activeElement: null, querySelector: () => null });
  vi.stubGlobal('requestAnimationFrame', () => 0);
});
afterEach(() => vi.unstubAllGlobals());

describe('Live CI context menu (HS2-7PSBHQ, HS2-D3JJ49)', () => {
  it('renders one Clear CI Status item for one run, on the live-ci context surface', () => {
    const markup = String(LiveCiContextMenu({ targets: [CI_TARGET], x: 20, y: 30 }));
    expect(markup).toContain('data-context-menu="live-ci"');
    expect(markup).toContain('Clear CI Status');
    expect(markup).toContain('data-action="clear-live-ci"');
    expect(markup).toContain('data-ci-clear-checkout="hotsheet2-56cb"');
    expect(markup).toContain('data-ci-clear-reported-at-ms="1795693115028"');
    expect(markup).toContain('data-ci-clear-workflow="CI"');
    expect(markup).toContain('data-lucide="eraser"');
  });

  it('names each run when the menu lists several', () => {
    const markup = String(LiveCiContextMenu({ targets: [CI_TARGET, PLATFORMS_TARGET], x: 20, y: 30 }));
    expect(markup).toContain('Clear main CI Status');
    expect(markup).toContain('Clear main Platforms Status');
    expect(markup.match(/data-action="clear-live-ci"/gu)).toHaveLength(2);
  });

  it('offers the run under the pointer, or every shown run once', () => {
    const ci = runElement(CI_RUN),
      platforms = runElement(PLATFORMS_RUN),
      // The compact chip and the expanded row of one run name it twice.
      again = runElement(CI_RUN),
      root = section([ci, platforms, again]);
    expect(liveCiMenuTargets(root, event(0, 0, platforms).target as unknown as EventTarget)).toEqual([
      PLATFORMS_TARGET,
    ]);
    expect(liveCiMenuTargets(root, event(0, 0).target as unknown as EventTarget)).toEqual([
      CI_TARGET,
      PLATFORMS_TARGET,
    ]);
    expect(liveCiMenuTargets(root, null)).toEqual([CI_TARGET, PLATFORMS_TARGET]);
    // A malformed element names nothing; a section without runs offers nothing.
    expect(liveCiMenuTargets(section([runElement({ ciClearCheckout: 'c' })]), null)).toEqual([]);
    expect(liveCiMenuTargets(section([]), null)).toEqual([]);
  });

  it('opens at the pointer on a right-click of a run, then clears that run once', async () => {
    const menu = signal<LiveCiMenuState | undefined>(undefined),
      clear = vi.fn(),
      dispose = wireLiveCiMenu(fakeRoot(), { menu, clear });
    const ci = runElement(CI_RUN),
      platforms = runElement(PLATFORMS_RUN);
    const right = event(120, 340, platforms);
    contextmenu()(right, section([ci, platforms]));
    expect(right.defaultPrevented).toBe(true);
    expect(menu.value).toEqual({ targets: [PLATFORMS_TARGET], x: 120, y: 340 });

    const click = event(0, 0);
    clearClick()(click, runElement(PLATFORMS_RUN));
    expect(click.propagationStopped).toBe(true);
    expect(menu.value).toBeUndefined();
    expect(clear).toHaveBeenCalledTimes(1);
    expect(clear).toHaveBeenCalledWith(PLATFORMS_TARGET);

    // A malformed item closes the menu without clearing anything.
    menu.value = { targets: [CI_TARGET], x: 0, y: 0 };
    clearClick()(event(0, 0), runElement({ ciClearCheckout: 'x' }));
    expect(menu.value).toBeUndefined();
    expect(clear).toHaveBeenCalledTimes(1);
    dispose();
    expect(registrations.every((entry) => !entry.attached)).toBe(true);
  });

  it('keeps the native menu on a section without a run, and anchors the keyboard menu below the toggle', () => {
    const menu = signal<LiveCiMenuState | undefined>(undefined);
    wireLiveCiMenu(fakeRoot(), { menu, clear: vi.fn() });
    const plain = event(10, 10);
    contextmenu()(plain, section([]));
    expect(plain.defaultPrevented).toBe(false);
    expect(menu.value).toBeUndefined();

    // The context-menu key reports no pointer position: it lists every shown run.
    const key = event(0, 0);
    contextmenu()(key, section([runElement(CI_RUN), runElement(PLATFORMS_RUN)], { left: 24, bottom: 300 }));
    expect(key.defaultPrevented).toBe(true);
    expect(menu.value).toMatchObject({ x: 24, y: 300 });
    expect(menu.value!.targets).toHaveLength(2);
  });

  it('keeps the menu inside the viewport near the right and bottom edges, for every item', () => {
    const menu = signal<LiveCiMenuState | undefined>(undefined);
    wireLiveCiMenu(fakeRoot(), { menu, clear: vi.fn() });
    contextmenu()(event(1275, 795), section([runElement(CI_RUN), runElement(PLATFORMS_RUN)]));
    const size = liveCiMenuSize(2);
    expect(size.height).toBeGreaterThan(liveCiMenuSize(1).height);
    expect(menu.value!.x + size.width).toBeLessThanOrEqual(1280);
    expect(menu.value!.y + size.height).toBeLessThanOrEqual(800);
  });

  it('closes on an outside press and on Escape, and opens again afterwards', () => {
    const root = fakeRoot(),
      menu = signal<LiveCiMenuState | undefined>(undefined);
    wireLiveCiMenu(root, { menu, clear: vi.fn() });
    const open = () => contextmenu()(event(50, 60), section([runElement(CI_RUN)]));
    open();
    root.listeners.get('pointerdown')!({ target: { closest: () => null }, composedPath: () => [] });
    expect(menu.value).toBeUndefined();
    open();
    expect(menu.value).toBeDefined();
    root.listeners.get('keydown')!({ key: 'Escape' });
    expect(menu.value).toBeUndefined();
    open();
    expect(menu.value).toMatchObject({ x: 50, y: 60 });
  });
});
