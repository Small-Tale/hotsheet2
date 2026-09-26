import { describe, expect, it, vi } from 'vitest';

import { CONTROLLED_OPEN_ATTRIBUTE, syncControlledOpen, withControlledOpen } from './controlled-open';

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
});
