import { describe, expect, it, vi } from 'vitest';

import {
  wireTerminalClipboardActions,
  wireTerminalClipboardSheets,
  wireTerminalDashboardMenu,
  wireTerminalKeyBar,
  wireTerminalRenameDialog,
  wireTerminalTextSizeControl,
} from './terminal-component-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: Event, target: Element) => unknown;
  disposed: boolean;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    const entry = { event, selector, handle, disposed: false };
    registrations.push(entry);
    return () => {
      entry.disposed = true;
    };
  },
}));
const handler = (event: string, selector: string) =>
  [...registrations].reverse().find((item) => item.event === event && item.selector === selector)!.handle as (
    event: unknown,
    target: unknown,
  ) => unknown;
const action = (name: string) => `[data-action="${name}"]`;

describe('terminal component wiring (HS2-XE048K)', () => {
  it('routes modifier, Fn-row, and key presses, scrolling a swapped row to its start', () => {
    const bindings = { toggleModifier: vi.fn(), toggleFunctionRow: vi.fn(), sendKey: vi.fn() };
    const dispose = wireTerminalKeyBar({} as HTMLElement, bindings);
    handler('click', action('toggle-terminal-modifier'))(new Event('click'), { dataset: { modifier: 'ctrl' } });
    handler('click', action('toggle-terminal-modifier'))(new Event('click'), { dataset: {} });
    const scrollTo = vi.fn();
    handler('click', action('toggle-terminal-function-row'))(new Event('click'), { closest: () => ({ scrollTo }) });
    handler('click', action('send-terminal-key'))(new Event('click'), { dataset: { key: 'ArrowUp' } });
    handler('click', action('send-terminal-key'))(new Event('click'), { dataset: {} });
    expect(bindings.toggleModifier.mock.calls.map((call) => call[0])).toEqual(['ctrl']);
    expect(scrollTo).toHaveBeenCalledWith({ left: 0 });
    expect(bindings.toggleFunctionRow).toHaveBeenCalledTimes(1);
    expect(bindings.sendKey.mock.calls.map((call) => call[0])).toEqual(['ArrowUp']);
    const mine = registrations.slice(-3);
    dispose();
    expect(mine.every((entry) => entry.disposed)).toBe(true);
  });

  it('routes the three clipboard actions with their targets', () => {
    const bindings = { copySelection: vi.fn(), copyText: vi.fn(), paste: vi.fn() };
    wireTerminalClipboardActions({} as HTMLElement, bindings);
    const target = {};
    handler('click', action('copy-terminal-selection'))(new Event('click'), target);
    handler('click', action('copy-terminal-text'))(new Event('click'), target);
    handler('click', action('paste-terminal-text'))(new Event('click'), target);
    expect(
      [bindings.copySelection, bindings.copyText, bindings.paste].every((spy) => spy.mock.calls[0][0] === target),
    ).toBe(true);
  });

  it('confirms a copy from the sheet field, submits paste text, and closes either sheet every way', () => {
    const field = { value: 'x' };
    let present = true;
    const root = { querySelector: () => (present ? field : null) } as unknown as HTMLElement;
    const bindings = { confirmCopy: vi.fn(), closeCopy: vi.fn(), submitPaste: vi.fn(), closePaste: vi.fn() };
    wireTerminalClipboardSheets(root, bindings);
    handler('click', action('confirm-terminal-copy'))(new Event('click'), {});
    present = false;
    handler('click', action('confirm-terminal-copy'))(new Event('click'), {});
    expect(bindings.confirmCopy.mock.calls).toEqual([[field]]);
    handler('click', action('close-terminal-copy'))(new Event('click'), {});
    handler('wa-hide', '[data-component="terminal-copy-dialog"]')(new Event('wa-hide'), {});
    expect(bindings.closeCopy).toHaveBeenCalledTimes(2);
    const preventDefault = vi.fn();
    handler('submit', action('submit-terminal-paste'))({ preventDefault }, { querySelector: () => ({ value: 'ls' }) });
    handler('submit', action('submit-terminal-paste'))({ preventDefault }, { querySelector: () => null });
    expect(preventDefault).toHaveBeenCalledTimes(2);
    expect(bindings.submitPaste.mock.calls).toEqual([['ls'], ['']]);
    handler('click', action('cancel-terminal-paste'))(new Event('click'), {});
    handler('wa-hide', '[data-component="terminal-paste-dialog"]')(new Event('wa-hide'), {});
    expect(bindings.closePaste).toHaveBeenCalledTimes(2);
  });

  it('renames only to a trimmed non-empty name, resets, and reports how the dialog was dismissed', () => {
    const bindings = { rename: vi.fn(), reset: vi.fn(), dismiss: vi.fn() };
    wireTerminalRenameDialog({} as HTMLElement, bindings);
    const submit = handler('submit', action('rename-terminal-form')),
      form = (value: string) => ({ querySelector: () => ({ value }) });
    submit({ preventDefault: vi.fn() }, form('  Build  '));
    submit({ preventDefault: vi.fn() }, form('   '));
    expect(bindings.rename.mock.calls).toEqual([['Build']]);
    handler('click', action('reset-terminal-rename'))(new Event('click'), {});
    handler('click', action('cancel-terminal-rename'))(new Event('click'), {});
    handler('wa-hide', '[data-terminal-rename-dialog]')(new Event('wa-hide'), {});
    expect(bindings.reset).toHaveBeenCalledTimes(1);
    expect(bindings.dismiss.mock.calls).toEqual([['cancel'], ['hide']]);
  });
});

describe('terminal dashboard menu and text-size wiring (HS2-05NMW0)', () => {
  it('opens the menu from an unmagnified tile or its button inside the viewport, and clears a halt', () => {
    vi.stubGlobal('window', { innerWidth: 400, innerHeight: 300 });
    const bindings = { open: vi.fn(), clearHalt: vi.fn() };
    const dispose = wireTerminalDashboardMenu({} as HTMLElement, bindings);
    const contextmenu = handler('contextmenu', '[data-component="terminal-tile"]');
    const magnified = { preventDefault: vi.fn(), clientX: 10, clientY: 10 };
    contextmenu(magnified, { dataset: { magnified: 'true', terminalKey: 'a' } });
    expect(magnified.preventDefault).not.toHaveBeenCalled();
    const pointer = { preventDefault: vi.fn(), clientX: 390, clientY: 290 };
    contextmenu(pointer, { dataset: { terminalKey: 'a' } });
    contextmenu({ preventDefault: vi.fn(), clientX: 5, clientY: 5 }, { dataset: { itemId: 'b' } });
    contextmenu({ preventDefault: vi.fn(), clientX: 5, clientY: 5 }, { dataset: {} });
    const click = { preventDefault: vi.fn() };
    const box = { right: 20, bottom: 30 };
    handler('click', action('open-terminal-context-menu'))(click, {
      dataset: { itemId: 'c' },
      getBoundingClientRect: () => box,
    });
    handler('click', action('open-terminal-context-menu'))(click, {
      dataset: { terminalKey: 'd' },
      getBoundingClientRect: () => box,
    });
    handler('click', action('clear-terminal-halt'))({}, { dataset: { itemId: 'c' } });
    expect(pointer.preventDefault).toHaveBeenCalled();
    expect(click.preventDefault).toHaveBeenCalledTimes(2);
    const opened = bindings.open.mock.calls.map(([menu]) => menu as { key: string; x: number; y: number });
    expect(opened.map((menu) => menu.key)).toEqual(['a', 'b', '', 'c', 'd']);
    // A press near the corner keeps the 224 by 104 menu inside the 400 by 300 viewport.
    expect(opened[0].x + 224).toBeLessThanOrEqual(400);
    expect(opened[0].y + 104).toBeLessThanOrEqual(300);
    expect(bindings.clearHalt).toHaveBeenCalledWith('c');
    dispose();
    expect(registrations.slice(-3).every((item) => item.disposed)).toBe(true);
    vi.unstubAllGlobals();
  });

  it('cycles the focus-mode text size with the pressed control', () => {
    const cycle = vi.fn();
    const dispose = wireTerminalTextSizeControl({} as HTMLElement, cycle);
    const control = { id: 'size' };
    handler('click', action('cycle-mobile-terminal-columns'))({}, control);
    expect(cycle).toHaveBeenCalledWith(control);
    dispose();
    expect(registrations.at(-1)!.disposed).toBe(true);
  });
});
