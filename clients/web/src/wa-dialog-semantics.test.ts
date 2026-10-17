import { afterEach, describe, expect, it, vi } from 'vitest';

import { quietWebAwesomeNativeDialog } from './wa-dialog-semantics';

function fakeDialog(initial: Record<string, string>) {
  const attributes = new Map(Object.entries(initial));
  return {
    attributes,
    getAttribute: (name: string) => attributes.get(name) ?? null,
    hasAttribute: (name: string) => attributes.has(name),
    setAttribute: (name: string, value: string) => attributes.set(name, value),
    removeAttribute: (name: string) => attributes.delete(name),
  };
}
const host = (dialog: unknown) => ({ shadowRoot: { querySelector: () => dialog } }) as unknown as Element;

describe('quietWebAwesomeNativeDialog (HS2-BH7NZ4)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('leaves the host as the only named dialog, now and after Web Awesome relabels', () => {
    const observers: (() => void)[] = [];
    vi.stubGlobal(
      'MutationObserver',
      class {
        constructor(private readonly callback: () => void) {}
        observe() {
          observers.push(this.callback);
        }
      },
    );
    const dialog = fakeDialog({ 'aria-label': 'Read and edit HS2-1', part: 'dialog' });
    expect(quietWebAwesomeNativeDialog(host(dialog))).toBe(dialog);
    expect(Object.fromEntries(dialog.attributes)).toEqual({ part: 'dialog', role: 'presentation' });
    // A header label (aria-labelledby) re-rendered later is stripped by the one installed observer.
    dialog.setAttribute('aria-labelledby', 'title');
    observers.forEach((callback) => {
      callback();
    });
    expect(dialog.hasAttribute('aria-labelledby')).toBe(false);
    quietWebAwesomeNativeDialog(host(dialog));
    expect(observers).toHaveLength(1);
  });

  it('quiets the shadow dialog once Web Awesome finishes its first render', async () => {
    const dialog = fakeDialog({ 'aria-labelledby': 'title' });
    let rendered = false;
    const pending = {
      shadowRoot: { querySelector: () => (rendered ? dialog : null) },
      updateComplete: Promise.resolve().then(() => {
        rendered = true;
      }),
    } as unknown as Element;
    expect(quietWebAwesomeNativeDialog(pending)).toBeNull();
    // A second call while the render is pending does not queue another wait.
    expect(quietWebAwesomeNativeDialog(pending)).toBeNull();
    await (pending as unknown as { updateComplete: Promise<void> }).updateComplete;
    await Promise.resolve();
    expect(Object.fromEntries(dialog.attributes)).toEqual({ role: 'presentation' });
  });

  it('returns null before Web Awesome renders its shadow dialog', () => {
    expect(quietWebAwesomeNativeDialog(host(null))).toBeNull();
    expect(quietWebAwesomeNativeDialog({ shadowRoot: null } as unknown as Element)).toBeNull();
  });
});
