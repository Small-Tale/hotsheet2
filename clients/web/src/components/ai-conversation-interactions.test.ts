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
    expect(registrations.some((item) => item.event === 'keydown')).toBe(false);
  });

  it('picks a message unless the click landed on a nested control', () => {
    const state = bindings();
    wireAiConversation({} as HTMLElement, state);
    const pick = handler('click', '[data-action="pick-conversation-message"]'),
      message = {
        dataset: { messageId: 'm1' },
        closest: () => ({ dataset: { selectionId: 'c1' } }),
      } as unknown as Element;
    pick({ target: { closest: () => ({}) } } as unknown as Event, message);
    pick({ target: { closest: () => null } } as unknown as Event, message);
    expect(state.pickMessage.mock.calls).toEqual([['c1', 'm1']]);
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
