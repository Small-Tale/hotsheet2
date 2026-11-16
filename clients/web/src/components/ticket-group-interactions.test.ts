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
    selectAll: vi.fn(),
    rename: vi.fn(),
    ungroup: vi.fn(),
    merge: vi.fn(),
    editName: vi.fn(),
    submitName: vi.fn(),
    dismissDialog: vi.fn(),
  };
  vi.stubGlobal('window', { innerWidth: 1200, innerHeight: 800 });
  const listeners: ((event: Event) => void)[] = [],
    buttons: Element[] = [],
    root = {
      addEventListener: (_type: string, listener: (event: Event) => void) => listeners.push(listener),
      removeEventListener: vi.fn(),
      querySelectorAll: () => buttons,
    };
  const dispose = wireTicketGroups(root as unknown as HTMLElement, bindings);
  const groupElement = { dataset: { ticketGroup: 'Board', ticketGroupScope: 'started' } };
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
    expect(bindings.openMenu).toHaveBeenLastCalledWith('Board', 420, 96, 'started');
    contextmenu(fake({ clientX: 0, clientY: 0 }), header);
    expect(bindings.openMenu).toHaveBeenLastCalledWith('Board', 316, 140, 'started');
    const outside = fake({ clientX: 1, clientY: 1 });
    contextmenu(outside, { dataset: {}, closest: () => null } as unknown as Element);
    expect(prevented(outside)).not.toHaveBeenCalled();
    expect(bindings.openMenu).toHaveBeenCalledTimes(2);
  });

  it('opens the group menu under the header overflow button without toggling (HS2-HKXSB1)', () => {
    const overflow = handler('click', '[data-action="open-ticket-group-menu"]'),
      before = bindings.openMenu.mock.calls.length,
      toggles = bindings.toggle.mock.calls.length,
      click = fake(),
      button = (rect: Record<string, number>) =>
        ({
          dataset: { ticketGroup: 'Board' },
          closest: () => groupElement,
          getBoundingClientRect: () => rect,
        }) as unknown as Element;
    overflow(click, button({ right: 900, bottom: 120 }));
    expect((click as unknown as { stopPropagation: ReturnType<typeof vi.fn> }).stopPropagation).toHaveBeenCalled();
    expect(bindings.openMenu).toHaveBeenLastCalledWith('Board', 668, 120, 'started');
    // A button near the leading edge or the bottom keeps the menu on screen.
    overflow(fake(), button({ right: 100, bottom: 790 }));
    expect(bindings.openMenu).toHaveBeenLastCalledWith('Board', 8, 608, 'started');
    overflow(fake(), { dataset: {}, closest: () => null } as unknown as Element);
    expect(bindings.openMenu.mock.calls.length - before).toBe(2);
    expect(bindings.toggle.mock.calls.length).toBe(toggles);
  });

  it('returns focus to the overflow button that opened the menu when it closes (HS2-HKXSB1)', () => {
    const body = { id: 'body' },
      doc = { body, activeElement: body as unknown },
      focus = vi.fn(),
      button = {
        dataset: { ticketGroup: 'Board' },
        closest: () => groupElement,
        getBoundingClientRect: () => ({ right: 900, bottom: 120 }),
        focus,
      } as unknown as Element,
      other = { dataset: { ticketGroup: 'Search' }, closest: () => null, focus: vi.fn() } as unknown as Element;
    vi.stubGlobal('document', doc);
    vi.stubGlobal('requestAnimationFrame', (run: () => void) => {
      run();
    });
    buttons.splice(0, buttons.length, other, button);
    const menu = {
        matches: (selector: string) => selector === '[data-ticket-group-menu="true"]',
        contains: () => false,
      },
      panel = {
        matches: () => true,
        parentElement: { ...menu, matches: (s: string) => s === '.kui-popup-menu' || menu.matches(s) },
      },
      closed = (target: unknown = panel) => ({ type: 'toggle', newState: 'closed', target }) as unknown as Event,
      overflow = handler('click', '[data-action="open-ticket-group-menu"]'),
      close = (event: Event) => {
        for (const listener of listeners) listener(event);
      };
    // Settle the opening an earlier test left; then a group menu closing with no overflow opening,
    // or another popup closing, does nothing.
    close(closed());
    focus.mockClear();
    close(closed());
    close(closed({ matches: () => false, parentElement: null }));
    expect(focus).not.toHaveBeenCalled();
    overflow(fake(), button);
    close(closed());
    expect(focus).toHaveBeenCalledTimes(1);
    expect((other as unknown as { focus: ReturnType<typeof vi.fn> }).focus).not.toHaveBeenCalled();
    // Only once per opening.
    close(closed());
    expect(focus).toHaveBeenCalledTimes(1);
    // A command that moved focus elsewhere, such as the rename dialog, keeps it.
    overflow(fake(), button);
    doc.activeElement = { id: 'dialog-field' };
    close(closed());
    expect(focus).toHaveBeenCalledTimes(1);
    // A menu opened from the context menu leaves focus where the browser puts it.
    doc.activeElement = body;
    overflow(fake(), button);
    handler('contextmenu', '[data-component="list-header"]')(
      fake({ clientX: 5, clientY: 5 }),
      inside({ getBoundingClientRect: () => ({ left: 0, bottom: 0 }) }),
    );
    close(closed());
    expect(focus).toHaveBeenCalledTimes(1);
    vi.unstubAllGlobals();
    vi.stubGlobal('window', { innerWidth: 1200, innerHeight: 800 });
  });

  it('removes its focus listener when disposed', () => {
    dispose();
    expect(root.removeEventListener).toHaveBeenCalledWith('toggle', expect.any(Function), true);
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

  it('routes Select All in Group with its group and scope (HS2-EYEGDQ)', () => {
    const item = (dataset: Record<string, string>) => ({ dataset }) as unknown as Element,
      select = handler('click', '[data-action="select-ticket-group"]');
    select(fake(), item({ ticketGroup: 'Board', ticketGroupScope: 'started' }));
    select(fake(), item({ ticketGroup: 'Search' }));
    select(fake(), item({ ticketGroupScope: 'list' }));
    expect(bindings.selectAll.mock.calls).toEqual([
      ['Board', 'started'],
      ['Search', 'list'],
    ]);
  });

  it('opens the menu for the list when the header sits outside a scoped container', () => {
    const contextmenu = handler('contextmenu', '[data-component="list-header"]'),
      header = {
        dataset: { ticketGroup: 'Loose' },
        closest: () => null,
        getBoundingClientRect: () => ({ left: 0, bottom: 0 }),
      } as unknown as Element;
    contextmenu(fake({ clientX: 10, clientY: 20 }), header);
    expect(bindings.openMenu).toHaveBeenLastCalledWith('Loose', 10, 20, 'list');
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
