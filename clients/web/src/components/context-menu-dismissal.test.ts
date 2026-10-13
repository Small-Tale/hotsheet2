import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { type DismissibleContextMenu, wireContextMenuDismissal } from './context-menu-dismissal';

interface Registration {
  event: string;
  selector: string;
  handle: (event: Event, target: Element) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => {
  const register = (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    const registration = { event, selector, handle };
    registrations.push(registration);
    return () => {
      registrations.splice(registrations.indexOf(registration), 1);
    };
  };
  return { ...(await original<typeof import('kerfjs')>()), delegate: register, delegateCapture: register };
});

type Listener = (event: Event) => void;
function fakeRoot() {
  const listeners = new Map<string, Set<Listener>>();
  return {
    listeners,
    addEventListener: (type: string, listener: Listener) => {
      if (!listeners.has(type)) listeners.set(type, new Set());
      listeners.get(type)!.add(listener);
    },
    removeEventListener: (type: string, listener: Listener) => {
      listeners.get(type)?.delete(listener);
    },
    dispatch(type: string, event: object) {
      for (const listener of listeners.get(type) ?? []) listener(event as Event);
    },
  };
}
/** A press whose composed path holds an element matching only `matching`. */
const press = (...matching: string[]) => ({
  composedPath: () => [{ matches: (selector: string) => selector.split(', ').some((part) => matching.includes(part)) }],
});
const element = (surface: string) => ({ dataset: { contextMenu: surface } }) as unknown as Element;

function menu(surface: string, options: Partial<DismissibleContextMenu> = {}) {
  const state = { open: true };
  return {
    state,
    menu: {
      surface,
      isOpen: () => state.open,
      close: vi.fn(() => {
        state.open = false;
      }),
      ...options,
    } satisfies DismissibleContextMenu,
  };
}

describe('wireContextMenuDismissal (HS2-AFCWNG)', () => {
  beforeEach(() => {
    registrations.length = 0;
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('closes only menus the press lands outside of, honoring ignored triggers', () => {
    const root = fakeRoot(),
      ticket = menu('ticket'),
      attachment = menu('attachment', { ignore: '[data-action="open-attachment-menu"]' });
    wireContextMenuDismissal(root as unknown as HTMLElement, [ticket.menu, attachment.menu]);
    root.dispatch('pointerdown', press('[data-context-menu="ticket"]'));
    expect(ticket.menu.close).not.toHaveBeenCalled();
    expect(attachment.menu.close).toHaveBeenCalledTimes(1);
    attachment.state.open = true;
    root.dispatch('pointerdown', press('[data-action="open-attachment-menu"]'));
    expect(attachment.menu.close).toHaveBeenCalledTimes(1);
    expect(ticket.menu.close).toHaveBeenCalledTimes(1);
    // A closed menu is not closed again by a later press.
    root.dispatch('pointerdown', press());
    expect(ticket.menu.close).toHaveBeenCalledTimes(1);
  });

  it('closes open menus on Escape unless a menu opts out, and ignores other keys', () => {
    const root = fakeRoot(),
      terminal = menu('terminal'),
      visibility = menu('terminal-visibility-group', { escape: false });
    wireContextMenuDismissal(root as unknown as HTMLElement, [terminal.menu, visibility.menu]);
    root.dispatch('keydown', { key: 'Enter' });
    expect(terminal.menu.close).not.toHaveBeenCalled();
    root.dispatch('keydown', { key: 'Escape' });
    expect(terminal.menu.close).toHaveBeenCalledTimes(1);
    expect(visibility.menu.close).not.toHaveBeenCalled();
  });

  it('defers Web Awesome hide dismissal for opted-in menus only, and skips bubbled hides', () => {
    const root = fakeRoot(),
      hiding = menu('attachment', { hide: true }),
      plain = menu('ticket');
    wireContextMenuDismissal(root as unknown as HTMLElement, [hiding.menu, plain.menu]);
    const hide = registrations.find((item) => item.event === 'wa-hide')!.handle;
    const attachmentMenu = element('attachment');
    hide({ target: element('attachment') } as unknown as Event, attachmentMenu);
    hide({ target: attachmentMenu } as unknown as Event, element('ticket'));
    vi.runAllTimers();
    expect(hiding.menu.close).not.toHaveBeenCalled();
    expect(plain.menu.close).not.toHaveBeenCalled();
    hide({ target: attachmentMenu } as unknown as Event, attachmentMenu);
    // An item's click handler still reads the open state before the deferred close.
    expect(hiding.menu.close).not.toHaveBeenCalled();
    vi.runAllTimers();
    expect(hiding.menu.close).toHaveBeenCalledTimes(1);
    // A hide after something else already closed it is a no-op.
    hide({ target: attachmentMenu } as unknown as Event, attachmentMenu);
    vi.runAllTimers();
    expect(hiding.menu.close).toHaveBeenCalledTimes(1);
  });

  it('registers no hide delegate without opted-in menus and removes every listener on dispose', () => {
    const root = fakeRoot(),
      ticket = menu('ticket');
    const dispose = wireContextMenuDismissal(root as unknown as HTMLElement, [ticket.menu]);
    expect(registrations.some((item) => item.event === 'wa-hide')).toBe(false);
    dispose();
    root.dispatch('pointerdown', press());
    root.dispatch('keydown', { key: 'Escape' });
    expect(ticket.menu.close).not.toHaveBeenCalled();
    expect([...root.listeners.values()].every((set) => set.size === 0)).toBe(true);
  });
});
