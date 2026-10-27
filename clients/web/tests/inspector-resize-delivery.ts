import { expect, type Page } from '@playwright/test';

/** HS2-ZV9M6R: control delivery of one real resize, never the rendered rail state.
 * The first actual readiness read returns its real value, then releases the app's
 * registered callbacks. Both the old one-shot read and the corrected presentation
 * wait therefore encounter the same pending breakpoint transition. */
export async function controlInspectorResizeDelivery(page: Page) {
  await page.addInitScript(() => {
    const originalAdd = Object.getOwnPropertyDescriptor(window, 'addEventListener');
    const add = window.addEventListener.bind(window),
      remove = window.removeEventListener.bind(window),
      listeners: Array<{
        listener: EventListenerOrEventListenerObject;
        wrapped: EventListener;
        options?: boolean | AddEventListenerOptions;
      }> = [];
    let held = false;
    const pending: Array<() => void> = [];
    const restore = () => {
      if (originalAdd) Object.defineProperty(window, 'addEventListener', originalAdd);
      else Reflect.deleteProperty(window, 'addEventListener');
      for (const { listener, wrapped, options } of listeners) {
        remove('resize', wrapped, options);
        add('resize', listener, options);
      }
      listeners.length = 0;
    };
    Object.defineProperty(window, 'addEventListener', {
      configurable: true,
      writable: true,
      value(
        type: string,
        listener: EventListenerOrEventListenerObject | null,
        options?: boolean | AddEventListenerOptions,
      ) {
        if (listener === null) return;
        if (type !== 'resize') {
          add(type, listener, options);
          return;
        }
        const wrapped: EventListener = (event) => {
          const invoke = () => {
            if (typeof listener === 'function') listener.call(window, event);
            else listener.handleEvent(event);
          };
          if (held) pending.push(invoke);
          else invoke();
        };
        listeners.push({ listener, wrapped, options });
        add(type, wrapped, options);
      },
    });
    add('hs2-zv9m6r-arm-resize', () => {
      held = true;
    });
    add('hs2-zv9m6r-release-resize', () => {
      held = false;
      if (pending.length) document.documentElement.dataset.zvDeliveredResize = String(pending.length);
      pending.splice(0).forEach((invoke) => {
        invoke();
      });
      restore();
    });
    add('hs2-zv9m6r-probe-resize', () => {
      document.documentElement.dataset.zvPendingResize = String(pending.length);
    });
    add('hs2-zv9m6r-restore-resize', restore);
  });
}

/** Release after a real locator read, explicitly across Playwright's isolated realms. */
export async function releaseInspectorResizeDelivery(page: Page) {
  const delivered = await page.evaluate(() => {
    window.dispatchEvent(new Event('hs2-zv9m6r-release-resize'));
    return Number(document.documentElement.dataset.zvDeliveredResize ?? 0);
  });
  expect(delivered).toBeGreaterThan(0);
}

export async function expectPendingInspectorResize(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(() => {
        window.dispatchEvent(new Event('hs2-zv9m6r-probe-resize'));
        return Number(document.documentElement.dataset.zvPendingResize ?? 0);
      }),
    )
    .toBeGreaterThan(0);
}
