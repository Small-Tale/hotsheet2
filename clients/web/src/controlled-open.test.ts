import { describe, expect, it, vi } from 'vitest';

import { CONTROLLED_OPEN_ATTRIBUTE, syncControlledOpen, withControlledOpen } from './controlled-open';

const surface = vi.hoisted(() => ({ openDialog: vi.fn(), closeDialog: vi.fn() }));
vi.mock('@kerfjs/ui/surface-scaffold', () => surface);

type FakeElement = { open?: boolean; attributes: Map<string, string> } & Pick<
  Element,
  'getAttribute' | 'hasAttribute' | 'toggleAttribute'
>;

function element(state: string, open?: boolean, attribute = false): FakeElement {
  const attributes = new Map<string, string>([[CONTROLLED_OPEN_ATTRIBUTE, state]]);
  if (attribute) attributes.set('open', '');
  return {
    ...(open === undefined ? {} : { open }),
    attributes,
    getAttribute: (name: string) => attributes.get(name) ?? null,
    hasAttribute: (name: string) => attributes.has(name),
    toggleAttribute: (name: string, force?: boolean) => {
      if (force) attributes.set(name, '');
      else attributes.delete(name);
      return Boolean(force);
    },
  };
}

const rootOf = (...elements: FakeElement[]) => ({ querySelectorAll: () => elements }) as unknown as ParentNode;

describe('controlled custom-element open (HS2-KMDJRH)', () => {
  it('closes and opens property-backed elements to their declared state, leaving matches alone', () => {
    const stayingOpen = element('true', true),
      closing = element('false', true),
      opening = element('true', false),
      staysClosed = element('false', false);
    syncControlledOpen(rootOf(stayingOpen, closing, opening, staysClosed));
    expect([stayingOpen.open, closing.open, opening.open, staysClosed.open]).toEqual([true, false, true, false]);
  });

  it('toggles the attribute on elements without an open property', () => {
    const closing = element('false', undefined, true),
      opening = element('true', undefined, false);
    syncControlledOpen(rootOf(closing, opening));
    expect(closing.hasAttribute('open')).toBe(false);
    expect(opening.hasAttribute('open')).toBe(true);
    // Repeated syncs are idempotent, and an empty root is a no-op.
    syncControlledOpen(rootOf(closing, opening));
    expect([closing.hasAttribute('open'), opening.hasAttribute('open')]).toEqual([false, true]);
    expect(() => {
      syncControlledOpen(rootOf());
    }).not.toThrow();
  });

  it('syncs after each render of a wrapped mount function, returning the render result unchanged', async () => {
    const dialog = element('false', true),
      render = vi.fn(() => 'markup'),
      wrapped = withControlledOpen(rootOf(dialog), render);
    expect(wrapped()).toBe('markup');
    // Not yet: the sync waits for the synchronous morph that follows render.
    expect(dialog.open).toBe(true);
    await Promise.resolve();
    expect(dialog.open).toBe(false);
    // A user reopening between renders is overridden only by the next render's declared state.
    dialog.open = true;
    dialog.attributes.set(CONTROLLED_OPEN_ATTRIBUTE, 'true');
    wrapped();
    await Promise.resolve();
    expect(dialog.open).toBe(true);
    expect(render).toHaveBeenCalledTimes(2);
  });

  describe('Kerf DialogSurface (HS2-BH7NZ4)', () => {
    /** A fake native `<dialog class="kui-dialog-surface">` whose `open` the pass must never toggle. */
    function dialog(state: string, connected = true) {
      const attributes = new Map<string, string>([[CONTROLLED_OPEN_ATTRIBUTE, state]]);
      return {
        localName: 'dialog',
        isConnected: connected,
        open: false,
        classList: { contains: (name: string) => name === 'kui-dialog-surface' },
        getAttribute: (name: string) => attributes.get(name) ?? null,
        hasAttribute: (name: string) => attributes.has(name),
        toggleAttribute: vi.fn(),
        attributes,
      };
    }
    const rootWith = (...elements: object[]) => ({ querySelectorAll: () => elements }) as unknown as ParentNode;

    it('opens a connected dialog with openDialog and closes one with closeDialog, never toggling open', () => {
      surface.openDialog.mockClear();
      surface.closeDialog.mockClear();
      const opening = dialog('true'),
        closing = dialog('false');
      syncControlledOpen(rootWith(opening, closing));
      expect(surface.openDialog).toHaveBeenCalledExactlyOnceWith(opening);
      expect(surface.closeDialog).toHaveBeenCalledExactlyOnceWith(closing);
      expect(opening.open).toBe(false);
      expect(opening.toggleAttribute).not.toHaveBeenCalled();
      expect(closing.toggleAttribute).not.toHaveBeenCalled();
    });

    it('waits for a detached dialog to connect before opening it modally', () => {
      surface.openDialog.mockClear();
      const detached = dialog('true', false);
      syncControlledOpen(rootWith(detached));
      expect(surface.openDialog).not.toHaveBeenCalled();
      detached.isConnected = true;
      syncControlledOpen(rootWith(detached));
      expect(surface.openDialog).toHaveBeenCalledExactlyOnceWith(detached);
    });

    it('delegates idempotence to Kerf: every sync re-asserts the declared state, including after a flip', () => {
      surface.openDialog.mockClear();
      surface.closeDialog.mockClear();
      const element = dialog('true');
      syncControlledOpen(rootWith(element));
      syncControlledOpen(rootWith(element));
      expect(surface.openDialog).toHaveBeenCalledTimes(2);
      element.attributes.set(CONTROLLED_OPEN_ATTRIBUTE, 'false');
      syncControlledOpen(rootWith(element));
      syncControlledOpen(rootWith(element));
      expect(surface.closeDialog).toHaveBeenCalledTimes(2);
      // Reopening while Kerf may still be running the exit motion asks Kerf to reverse it.
      element.attributes.set(CONTROLLED_OPEN_ATTRIBUTE, 'true');
      syncControlledOpen(rootWith(element));
      expect(surface.openDialog).toHaveBeenCalledTimes(3);
    });

    it('treats a dialog without the Kerf surface class as an ordinary open-property element', () => {
      surface.openDialog.mockClear();
      const plain = { ...dialog('true'), classList: { contains: () => false } };
      syncControlledOpen(rootWith(plain));
      expect(surface.openDialog).not.toHaveBeenCalled();
      expect(plain.open).toBe(true);
    });
  });
});
