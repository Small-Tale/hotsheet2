import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { LiveCiContextMenu, type LiveCiMenuState } from './live-ci-context-menu';
import { wireLiveCiMenu } from './live-ci-menu-interactions';

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
const contextmenu = () => handler('contextmenu', '[data-ci-clear-checkout]');
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
const section = (dataset: Record<string, string>, rect = { left: 24, bottom: 300 }) =>
  ({ dataset, getBoundingClientRect: () => rect, contains: () => true }) as unknown as HTMLElement;
const event = (clientX: number, clientY: number) => ({
  clientX,
  clientY,
  defaultPrevented: false,
  propagationStopped: false,
  preventDefault() {
    this.defaultPrevented = true;
  },
  stopPropagation() {
    this.propagationStopped = true;
  },
});
const STALE = { ciClearCheckout: 'hotsheet2-56cb', ciClearReportedAtMs: '1795693115028' };

beforeEach(() => {
  registrations.length = 0;
  vi.stubGlobal('window', { innerWidth: 1280, innerHeight: 800 });
  vi.stubGlobal('document', { activeElement: null, querySelector: () => null });
  vi.stubGlobal('requestAnimationFrame', () => 0);
});
afterEach(() => vi.unstubAllGlobals());

describe('Live CI context menu (HS2-7PSBHQ)', () => {
  it('renders one Clear CI Status item naming the run, on the live-ci context surface', () => {
    const markup = String(LiveCiContextMenu({ checkout: 'hotsheet2-56cb', reportedAtMs: 17, x: 20, y: 30 }));
    expect(markup).toContain('data-context-menu="live-ci"');
    expect(markup).toContain('Clear CI Status');
    expect(markup).toContain('data-action="clear-live-ci"');
    expect(markup).toContain('data-ci-checkout="hotsheet2-56cb"');
    expect(markup).toContain('data-ci-reported-at-ms="17"');
    expect(markup).toContain('data-lucide="eraser"');
  });

  it('opens at the pointer on a right-click of a section naming an unknown run, then clears it once', async () => {
    const menu = signal<LiveCiMenuState | undefined>(undefined),
      clear = vi.fn(),
      dispose = wireLiveCiMenu(fakeRoot(), { menu, clear });
    const right = event(120, 340);
    contextmenu()(right, section(STALE));
    expect(right.defaultPrevented).toBe(true);
    expect(menu.value).toEqual({ checkout: 'hotsheet2-56cb', reportedAtMs: 1_795_693_115_028, x: 120, y: 340 });

    const item = section({ ciCheckout: 'hotsheet2-56cb', ciReportedAtMs: '1795693115028' });
    const click = event(0, 0);
    clearClick()(click, item);
    expect(click.propagationStopped).toBe(true);
    expect(menu.value).toBeUndefined();
    expect(clear).toHaveBeenCalledTimes(1);
    expect(clear).toHaveBeenCalledWith({ checkout: 'hotsheet2-56cb', reportedAtMs: 1_795_693_115_028 });

    // A malformed item closes the menu without clearing anything.
    menu.value = { checkout: 'x', reportedAtMs: 1, x: 0, y: 0 };
    clearClick()(event(0, 0), section({ ciCheckout: 'x' }));
    expect(menu.value).toBeUndefined();
    expect(clear).toHaveBeenCalledTimes(1);
    dispose();
    expect(registrations.every((entry) => !entry.attached)).toBe(true);
  });

  it('keeps the native menu on a section without a clear target, and anchors the keyboard menu below the toggle', () => {
    const menu = signal<LiveCiMenuState | undefined>(undefined);
    wireLiveCiMenu(fakeRoot(), { menu, clear: vi.fn() });
    const plain = event(10, 10);
    contextmenu()(plain, section({ ciClearCheckout: 'c' }));
    expect(plain.defaultPrevented).toBe(false);
    expect(menu.value).toBeUndefined();

    // The context-menu key reports no pointer position.
    const key = event(0, 0);
    contextmenu()(key, section(STALE, { left: 24, bottom: 300 }));
    expect(key.defaultPrevented).toBe(true);
    expect(menu.value).toMatchObject({ x: 24, y: 300 });
  });

  it('keeps the menu inside the viewport near the right and bottom edges', () => {
    const menu = signal<LiveCiMenuState | undefined>(undefined);
    wireLiveCiMenu(fakeRoot(), { menu, clear: vi.fn() });
    contextmenu()(event(1275, 795), section(STALE));
    expect(menu.value!.x + 192).toBeLessThanOrEqual(1280);
    expect(menu.value!.y + 56).toBeLessThanOrEqual(800);
  });

  it('closes on an outside press and on Escape, and opens again afterwards', () => {
    const root = fakeRoot(),
      menu = signal<LiveCiMenuState | undefined>(undefined);
    wireLiveCiMenu(root, { menu, clear: vi.fn() });
    const open = () => contextmenu()(event(50, 60), section(STALE));
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
