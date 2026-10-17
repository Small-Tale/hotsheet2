import { describe, expect, it, vi } from 'vitest';

import { TICKET_TAG_COMBOBOX_COMMIT } from './app-combobox-wiring';
import {
  wireBlockedReasonEditor,
  wireFeedbackChoices,
  wireTicketTagEditor,
  wireTicketTitleEditor,
} from './ticket-field-editor-interactions';

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
// Kerf decides whether a click is the card's own activation or a nested control's (HS2-3XV34B).
vi.mock('@kerfjs/ui/wire-content-items', () => ({
  isContentItemActivation: (event: { target: { nested?: boolean } }) => !event.target.nested,
}));
vi.mock('../ticket-title-editing', () => ({
  normalizeTicketTitleField: (field: { value: string }) => field.value.replace(/\s*\n\s*/g, ' '),
  ticketTitleKeyFinishesEdit: (event: KeyboardEvent) => event.key === 'Enter' && !event.isComposing,
}));
const handler = (event: string, selector: string) =>
  [...registrations].reverse().find((item) => item.event === event && item.selector === selector)!.handle as (
    event: unknown,
    target: unknown,
  ) => unknown;
const keyEvent = (key: string, init: Record<string, unknown> = {}) =>
  ({ key, preventDefault: vi.fn(), metaKey: false, ctrlKey: false, shiftKey: false, ...init }) as unknown as Event &
    KeyboardEvent & { preventDefault: ReturnType<typeof vi.fn> };
const root = {} as HTMLElement;

describe('ticket field editor wiring (HS2-KAWNER)', () => {
  it('begins, edits, finishes, and disposes the title editor', () => {
    const bindings = { begin: vi.fn(), input: vi.fn(), finish: vi.fn() };
    const dispose = wireTicketTitleEditor(root, bindings);
    const blur = vi.fn(),
      heading = {} as Element,
      field = { value: 'one\ntwo', blur } as unknown as HTMLTextAreaElement;
    handler('dblclick', '[data-action="edit-ticket-title"]')(new Event('dblclick'), heading);
    const enterPrevented = vi.fn(),
      otherPrevented = vi.fn(),
      enter = keyEvent('Enter', { preventDefault: enterPrevented }),
      other = keyEvent('a', { preventDefault: otherPrevented });
    handler('keydown', '[data-action="edit-ticket-title"]')(enter, heading);
    handler('keydown', '[data-action="edit-ticket-title"]')(other, heading);
    expect(enterPrevented).toHaveBeenCalled();
    expect(otherPrevented).not.toHaveBeenCalled();
    expect(bindings.begin).toHaveBeenCalledTimes(2);
    handler('input', '[name="ticket-title"]')(new Event('input'), field);
    expect(bindings.input).toHaveBeenCalledWith('one two', field);
    handler('keydown', '[name="ticket-title"]')(keyEvent('Enter'), field);
    handler('keydown', '[name="ticket-title"]')(keyEvent('b'), field);
    expect(blur).toHaveBeenCalledTimes(1);
    handler('focusout', '[name="ticket-title"]')(new Event('focusout'), field);
    expect(bindings.finish).toHaveBeenCalledWith(field);
    const mine = registrations.slice(-5);
    dispose();
    expect(mine.every((entry) => entry.disposed)).toBe(true);
  });

  it('adds combobox commits and a non-empty blur, always flushes on blur, and removes a chip', () => {
    const bindings = { add: vi.fn(), flush: vi.fn(), remove: vi.fn() };
    wireTicketTagEditor(root, bindings);
    const input = { value: '' } as HTMLInputElement;
    handler(TICKET_TAG_COMBOBOX_COMMIT, '[name="ticket-tag-input"]')({ detail: 'a' }, input);
    handler(TICKET_TAG_COMBOBOX_COMMIT, '[name="ticket-tag-input"]')({ detail: 'b' }, input);
    input.value = 'x';
    handler('focusout', '[name="ticket-tag-input"]')(new Event('focusout'), input);
    input.value = '  ';
    handler('focusout', '[name="ticket-tag-input"]')(new Event('focusout'), input);
    expect(bindings.add.mock.calls).toEqual([['a'], ['b'], ['x']]);
    expect(input.value).toBe('  ');
    expect(bindings.flush).toHaveBeenCalledTimes(2);
    const remove = handler('click', '[data-action="remove-tag-chip"]');
    remove(new Event('click'), { closest: () => ({ dataset: { itemId: 'ux' } }) });
    remove(new Event('click'), { closest: () => null });
    expect(bindings.remove.mock.calls).toEqual([['ux']]);
  });

  it('reports which trigger began a blocked-reason edit and routes input and finish', () => {
    const bindings = { begin: vi.fn(), input: vi.fn(), finish: vi.fn() };
    wireBlockedReasonEditor(root, bindings);
    const text = {} as Element;
    handler('click', '[data-action="edit-blocked-reason"]')(new Event('click'), text);
    handler('dblclick', '[data-edit-blocked-reason="true"]')(new Event('dblclick'), text);
    handler('keydown', '[data-edit-blocked-reason="true"]')(keyEvent(' '), text);
    handler('keydown', '[data-edit-blocked-reason="true"]')(keyEvent('x'), text);
    expect(bindings.begin.mock.calls.map((call) => call[1])).toEqual(['action', 'dblclick', 'key']);
    const field = { value: 'waiting' } as HTMLTextAreaElement;
    handler('input', '[name="blocked-reason"]')(new Event('input'), field);
    handler('focusout', '[name="blocked-reason"]')(new Event('focusout'), field);
    expect(bindings.input).toHaveBeenCalledWith('waiting', field);
    expect(bindings.finish).toHaveBeenCalledWith(field);
  });

  it('toggles feedback choices with modifiers but leaves nested controls alone', () => {
    const bindings = { toggle: vi.fn() };
    wireFeedbackChoices(root, bindings);
    const click = handler('click', '[data-action="toggle-feedback-choice"]'),
      choice = { dataset: { noteId: 'n', choiceId: 'c' } },
      nested = { nested: true };
    click({ target: choice, metaKey: true, ctrlKey: false, shiftKey: false }, choice);
    click({ target: choice, metaKey: false, ctrlKey: false, shiftKey: true }, choice);
    click({ target: nested, metaKey: false, ctrlKey: false, shiftKey: false }, choice);
    click({ target: choice, metaKey: false, ctrlKey: false, shiftKey: false }, { dataset: {} });
    expect(bindings.toggle.mock.calls).toEqual([
      ['n', 'c', { additive: true, range: false }, choice],
      ['n', 'c', { additive: false, range: true }, choice],
    ]);
  });
});
