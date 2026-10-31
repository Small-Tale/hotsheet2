import type { Page } from '@playwright/test';

type ReaderFocusControl = {
  captures: number;
  closeEvents: number;
  zeroDelayCallbacks: Array<{ row: boolean; workspace: boolean }>;
  pending: boolean;
  released: boolean;
  restored: boolean;
  release: () => void;
  restore: () => void;
  dispose: () => void;
};
type ControlledWindow = Window & { __hs2Frfpa5ReaderFocus?: ReaderFocusControl };

/** HS2-FRFPA5: delay only the real reader's close-event return-focus delivery. */
export async function holdReaderReturnFocus(page: Page) {
  await page.addInitScript(() => {
    // Keep this browser script on DOM timer signatures when the runner also loads Node types.
    const browserWindow: Window = window;
    const nativeTimeout = browserWindow.setTimeout.bind(browserWindow);
    let pending: (() => void) | undefined;
    const control: ReaderFocusControl = {
      captures: 0,
      closeEvents: 0,
      zeroDelayCallbacks: [],
      pending: false,
      released: false,
      restored: true,
      release: () => {
        if (!pending || control.released || control.captures !== 1)
          throw new Error('Reader focus delivery unavailable');
        control.released = true;
        control.pending = false;
        const callback = pending;
        pending = undefined;
        callback();
      },
      restore: () => {
        browserWindow.setTimeout = nativeTimeout;
        control.restored = true;
      },
      dispose: () => {
        document.removeEventListener('close', onClose, true);
        control.restore();
        if (control.pending && !control.released) control.release();
      },
    };
    Object.defineProperty(window, '__hs2Frfpa5ReaderFocus', { value: control, configurable: true });
    const onClose = (event: Event) => {
      if (!(event.target instanceof HTMLDialogElement) || !event.target.matches('dialog[data-reader-dialog]')) return;
      control.closeEvents++;
      control.restored = false;
      // Capture the actual application callback by its two fallback selectors,
      // not an arbitrary timer. Unrelated close-event timers retain native delivery.
      browserWindow.setTimeout = (handler: TimerHandler, delay?: number, ...args: unknown[]) => {
        if (typeof handler === 'function' && delay === 0) {
          const source = Function.prototype.toString.call(handler);
          const signature = {
            row: source.includes('select-ticket-row'),
            workspace: source.includes('data-work-area-focus-owner'),
          };
          if (control.zeroDelayCallbacks.length < 8) control.zeroDelayCallbacks.push(signature);
          if (signature.row && signature.workspace) {
            control.captures++;
            return nativeTimeout(() => {
              if (pending || control.captures !== 1) throw new Error('Ambiguous reader focus delivery');
              pending = () => {
                Reflect.apply(handler, window, args);
              };
              control.pending = true;
            }, delay);
          }
        }
        return nativeTimeout(handler, delay, ...args);
      };
      // Native event listener microtasks are not an after-ancestor-capture boundary.
      // Restore at the target, after the app's body capture handler has run.
      event.target.addEventListener('close', control.restore, { once: true });
    };
    document.addEventListener('close', onClose, true);
  });
  return {
    state: () =>
      page.evaluate(() => {
        const control = (window as ControlledWindow).__hs2Frfpa5ReaderFocus;
        if (!control) throw new Error('Reader focus control unavailable');
        const { captures, closeEvents, zeroDelayCallbacks, pending, released, restored } = control;
        return { captures, closeEvents, zeroDelayCallbacks, pending, released, restored };
      }),
    release: () =>
      page.evaluate(() => {
        const control = (window as ControlledWindow).__hs2Frfpa5ReaderFocus;
        if (!control) throw new Error('Reader focus control unavailable');
        control.release();
      }),
    restore: async () => {
      if (page.isClosed()) return;
      try {
        await page.evaluate(() => {
          const control = (window as ControlledWindow).__hs2Frfpa5ReaderFocus;
          if (!control) return;
          control.dispose();
        });
      } catch (error) {
        if (!page.isClosed()) throw error;
      }
    },
  };
}
