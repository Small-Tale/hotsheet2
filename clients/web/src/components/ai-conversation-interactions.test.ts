import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { activityGroupExpanded, expandedActivityGroups } from '../conversation-activity-disclosure';
import { wireAiConversation } from './ai-conversation-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: Event, target: Element) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => {
  const register = (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event, selector, handle });
    return () => undefined;
  };
  return { ...(await original<typeof import('kerfjs')>()), delegate: register, delegateCapture: register };
});
const wireComposers = vi.hoisted(() => vi.fn(() => () => undefined));
vi.mock('@kerfjs/ui/wire-composers', () => ({ wireComposers }));
const handler = (event: string, selector: string) =>
  registrations.find((item) => item.event === event && item.selector === selector)!.handle;
const bindings = () => ({
  onHidden: vi.fn(),
  save: vi.fn(),
  pickMessage: vi.fn(),
  setDraft: vi.fn(),
  send: vi.fn(),
  stop: vi.fn(),
  selectProvider: vi.fn(),
  selectModel: vi.fn(),
  selectEffort: vi.fn(),
  openManualModel: vi.fn(),
});

beforeEach(() => {
  registrations.length = 0;
  vi.stubGlobal('window', { getSelection: () => ({ toString: () => '' }) });
});
afterEach(() => vi.unstubAllGlobals());

describe('wireAiConversation (HS2-RWY78R)', () => {
  it('delegates Enter-to-send to Kerf wireComposers instead of an app keydown handler (HS2-94T958)', () => {
    const root = {} as HTMLElement;
    wireAiConversation(root, bindings());
    expect(wireComposers).toHaveBeenCalledWith(root);
    // The only keydown handler is the message-selection chord handler, never a composer one.
    expect(registrations.filter((item) => item.event === 'keydown').map((item) => item.selector)).toEqual([
      '[data-action="pick-conversation-message"]',
    ]);
  });

  const message = {
      dataset: { messageId: 'm1' },
      closest: () => ({ dataset: { selectionId: 'c1' } }),
    } as unknown as Element,
    mouse = (modifiers: Partial<MouseEvent>, control: object | null = null) =>
      ({
        metaKey: false,
        ctrlKey: false,
        shiftKey: false,
        ...modifiers,
        target: { closest: () => control },
        preventDefault: vi.fn(),
      }) as unknown as MouseEvent;
  const plain = { toggle: false, extend: false };

  it('picks a message with normalized modifiers unless the click landed on a nested control (HS2-WV88W3)', () => {
    vi.stubGlobal('navigator', { userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 15_0)' });
    const state = bindings();
    wireAiConversation({} as HTMLElement, state);
    const pick = handler('click', '[data-action="pick-conversation-message"]');
    pick(mouse({}, {}), message);
    pick(mouse({}), message);
    pick(mouse({ metaKey: true }), message);
    // Ctrl-click is the context menu on macOS, never the toggle.
    pick(mouse({ ctrlKey: true }), message);
    pick(mouse({ shiftKey: true }), message);
    pick(mouse({ shiftKey: true, metaKey: true }), message);
    expect(state.pickMessage.mock.calls).toEqual([
      ['c1', 'm1', plain],
      ['c1', 'm1', { toggle: true, extend: false }],
      ['c1', 'm1', plain],
      ['c1', 'm1', { toggle: false, extend: true }],
      ['c1', 'm1', { toggle: true, extend: true }],
    ]);
  });

  it('uses Ctrl as the toggle off Apple platforms and lets modified clicks pick over a text selection', () => {
    vi.stubGlobal('navigator', { userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' });
    vi.stubGlobal('window', { getSelection: () => ({ toString: () => 'dragged text' }) });
    const state = bindings();
    wireAiConversation({} as HTMLElement, state);
    const pick = handler('click', '[data-action="pick-conversation-message"]');
    // A plain click that ends a text drag is reading, not picking.
    pick(mouse({}), message);
    pick(mouse({ metaKey: true }), message);
    pick(mouse({ ctrlKey: true }), message);
    expect(state.pickMessage.mock.calls).toEqual([['c1', 'm1', { toggle: true, extend: false }]]);
    // Modified presses do not start or extend a text selection; plain presses and nested controls do.
    const press = handler('mousedown', '[data-action="pick-conversation-message"]'),
      events = [mouse({ shiftKey: true }), mouse({ ctrlKey: true }), mouse({}), mouse({ shiftKey: true }, {})];
    for (const event of events) press(event, message);
    expect(events.map((event) => (event.preventDefault as ReturnType<typeof vi.fn>).mock.calls.length)).toEqual([
      1, 1, 0, 0,
    ]);
  });

  it('handles Shift and Cmd/Ctrl with Enter or Space on the focused message before Kerf (KF-45RNDS)', () => {
    vi.stubGlobal('navigator', { userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 15_0)' });
    const state = bindings();
    wireAiConversation({} as HTMLElement, state);
    const keydown = handler('keydown', '[data-action="pick-conversation-message"]'),
      key = (init: Partial<KeyboardEvent>, target: unknown = message) => {
        const event = {
          key: 'Enter',
          metaKey: false,
          ctrlKey: false,
          shiftKey: false,
          altKey: false,
          repeat: false,
          target,
          preventDefault: vi.fn(),
          ...init,
        } as unknown as KeyboardEvent;
        keydown(event, message);
        return (event.preventDefault as ReturnType<typeof vi.fn>).mock.calls.length;
      };
    // Plain Enter/Space stay with Kerf's own activation (a modifier-free click).
    expect(key({})).toBe(0);
    expect(key({ key: ' ' })).toBe(0);
    expect(key({ key: 'Enter', shiftKey: true })).toBe(1);
    expect(key({ key: ' ', metaKey: true })).toBe(1);
    // Auto-repeat is swallowed without re-picking; other keys, Alt chords, and nested targets pass.
    expect(key({ key: ' ', metaKey: true, repeat: true })).toBe(1);
    expect(key({ key: 'a', shiftKey: true })).toBe(0);
    expect(key({ key: 'Enter', shiftKey: true, altKey: true })).toBe(0);
    expect(key({ key: 'Enter', shiftKey: true }, {})).toBe(0);
    expect(state.pickMessage.mock.calls).toEqual([
      ['c1', 'm1', { toggle: false, extend: true }],
      ['c1', 'm1', { toggle: true, extend: false }],
    ]);
  });

  it('toggles an activity group from its body or header button but not from nested controls (HS2-Y3SGH3)', () => {
    expandedActivityGroups.value = new Set();
    wireAiConversation({} as HTMLElement, bindings());
    const toggle = handler('click', '[data-action="toggle-conversation-activity"]'),
      group = {
        dataset: { activityGroup: 'a1' },
        closest: () => ({ dataset: { selectionId: 'c1' } }),
      } as unknown as Element,
      clickOn = (control: { matches: (selector: string) => boolean } | null) =>
        toggle({ target: { closest: () => control } } as unknown as Event, group);
    // Plain body click expands; the header toggle button collapses again.
    clickOn(null);
    expect(activityGroupExpanded('c1', 'a1')).toBe(true);
    clickOn({ matches: (selector) => selector === '.ai-conversation__activity-toggle' });
    expect(activityGroupExpanded('c1', 'a1')).toBe(false);
    // A feedback button or link inside the group does not toggle it.
    clickOn({ matches: () => false });
    expect(activityGroupExpanded('c1', 'a1')).toBe(false);
    // Neither does finishing a text selection inside the group.
    vi.stubGlobal('window', { getSelection: () => ({ toString: () => 'WebFetch' }) });
    clickOn(null);
    expect(activityGroupExpanded('c1', 'a1')).toBe(false);
    expandedActivityGroups.value = new Set();
  });

  it('routes the provider, model, and effort menus with their values', () => {
    const state = bindings();
    wireAiConversation({} as HTMLElement, state);
    const item = (value: string) => ({ dataset: { value } }) as unknown as Element;
    handler('click', '[data-action="select-conversation-provider"]')(new Event('click'), item('codex'));
    handler('click', '[data-action="select-conversation-model"]')(new Event('click'), item('gpt-5.6'));
    handler('click', '[data-action="select-conversation-effort"]')(new Event('click'), item('high'));
    expect(state.selectProvider).toHaveBeenCalledWith('codex');
    expect(state.selectModel).toHaveBeenCalledWith('gpt-5.6');
    expect(state.selectEffort).toHaveBeenCalledWith('high');
  });
});
