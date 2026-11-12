import { describe, expect, it, vi } from 'vitest';

import { wireTicketGroups } from './ticket-group-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: Event, target: Element) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event, selector, handle });
    return () => undefined;
  },
}));
const handler = (event: string, selector: string) =>
  registrations.find((item) => item.event === event && item.selector.includes(selector))!.handle;
const fake = (init: Record<string, unknown> = {}) =>
  ({ preventDefault: vi.fn(), stopPropagation: vi.fn(), ...init }) as unknown as Event;
/** The fake event's `preventDefault` spy. */
const prevented = (event: Event) => (event as unknown as { preventDefault: ReturnType<typeof vi.fn> }).preventDefault;

describe('wireTicketGroups (HS2-WZ3JGZ, HS2-TX17CM, HS2-8CDFKR)', () => {
  const bindings = {
    toggle: vi.fn(),
    openMenu: vi.fn(),
    rename: vi.fn(),
    ungroup: vi.fn(),
    merge: vi.fn(),
    editName: vi.fn(),
    submitName: vi.fn(),
    dismissDialog: vi.fn(),
  };
  vi.stubGlobal('window', { innerWidth: 1200, innerHeight: 800 });
  wireTicketGroups({} as HTMLElement, bindings);
  const groupElement = { dataset: { ticketGroup: 'Board' } };
  const inside = (extra: Record<string, unknown> = {}) =>
    ({ dataset: {}, closest: () => groupElement, ...extra }) as unknown as Element;

  it('toggles the group a header belongs to, from the trigger or its container', () => {
    const toggle = handler('click', '[data-action="toggle-ticket-group"]');
    toggle(fake(), { dataset: { ticketGroup: 'Search' } } as unknown as Element);
    toggle(fake(), inside());
    toggle(fake(), { dataset: {}, closest: () => null } as unknown as Element);
    expect(bindings.toggle.mock.calls).toEqual([['Search'], ['Board']]);
  });

  it('opens the group menu at the pointer, or under the header for a keyboard request', () => {
    const contextmenu = handler('contextmenu', '[data-component="list-header"]'),
      rect = { left: 300, bottom: 140 },
      header = inside({ getBoundingClientRect: () => rect }),
      pointer = fake({ clientX: 420, clientY: 96 });
    contextmenu(pointer, header);
    expect(prevented(pointer)).toHaveBeenCalled();
    expect(bindings.openMenu).toHaveBeenLastCalledWith('Board', 420, 96);
    contextmenu(fake({ clientX: 0, clientY: 0 }), header);
    expect(bindings.openMenu).toHaveBeenLastCalledWith('Board', 316, 140);
    const outside = fake({ clientX: 1, clientY: 1 });
    contextmenu(outside, { dataset: {}, closest: () => null } as unknown as Element);
    expect(prevented(outside)).not.toHaveBeenCalled();
    expect(bindings.openMenu).toHaveBeenCalledTimes(2);
  });

  it('routes the menu’s rename and ungroup items with their group', () => {
    const item = (ticketGroup?: string) => ({ dataset: { ticketGroup } }) as unknown as Element;
    handler('click', '[data-action="rename-ticket-group"]')(fake(), item('Board'));
    handler('click', '[data-action="ungroup-ticket-group"]')(fake(), item('Search'));
    handler('click', '[data-action="rename-ticket-group"]')(fake(), item());
    handler('click', '[data-action="ungroup-ticket-group"]')(fake(), item());
    expect(bindings.rename.mock.calls).toEqual([['Board']]);
    expect(bindings.ungroup.mock.calls).toEqual([['Search']]);
  });

  it('routes Merge Duplicate Groups with the group id (HS2-9KZSD4)', () => {
    const item = (ticketGroup?: string) => ({ dataset: { ticketGroup } }) as unknown as Element;
    handler('click', '[data-action="merge-ticket-groups"]')(fake(), item('7K3M9P2QXA'));
    handler('click', '[data-action="merge-ticket-groups"]')(fake(), item());
    expect(bindings.merge.mock.calls).toEqual([['7K3M9P2QXA']]);
  });

  it('reports typed names, submits the field’s live value, and dismisses', () => {
    handler('input', '[name="ticket-group-name"]')(fake(), { value: 'Boa' } as unknown as Element);
    expect(bindings.editName).toHaveBeenCalledWith('Boa');
    const submit = fake();
    handler('submit', '[data-action="submit-ticket-group-name"]')(submit, {
      querySelector: () => ({ value: 'Board polish' }),
    } as unknown as Element);
    expect(prevented(submit)).toHaveBeenCalled();
    handler('submit', '[data-action="submit-ticket-group-name"]')(fake(), {
      querySelector: () => null,
    } as unknown as Element);
    expect(bindings.submitName.mock.calls).toEqual([['Board polish'], ['']]);
    handler('click', '[data-action="cancel-ticket-group-name"]')(fake(), {} as Element);
    const dialog = {} as Element;
    handler('kui-request-close', '[data-dialog-component="ticket-group-dialog"]')(fake({ target: dialog }), dialog);
    // A nested element's close request is not the dialog's own.
    handler('kui-request-close', '[data-dialog-component="ticket-group-dialog"]')(fake({ target: {} }), dialog);
    expect(bindings.dismissDialog).toHaveBeenCalledTimes(2);
  });
});
