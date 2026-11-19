import { type Browser, type BrowserType, type LaunchOptions, test } from '@playwright/test';

import { acquireBrowserSlot, DEFAULT_WAIT_MS } from '../scripts/browser-slots.mjs';

/**
 * HS2-M6TZB5: launch a browser a spec starts itself through a machine-wide `browser-engines` slot.
 * Use `slotted(webkit).launch()` instead of `webkit.launch()`; the slot is released when the
 * browser closes or disconnects. Waiting for a slot extends the current test's timeout by the time
 * it waited, so a busy pool does not fail the test.
 */
export function slotted(engine: BrowserType) {
  return {
    async launch(options?: LaunchOptions): Promise<Browser> {
      if (engine.name() !== 'webkit') return engine.launch(options);
      let info: ReturnType<typeof test.info> | undefined;
      try {
        info = test.info();
      } catch {
        info = undefined;
      }
      const base = info?.timeout ?? 0;
      if (info && base > 0) info.setTimeout(base + DEFAULT_WAIT_MS);
      const slot = await acquireBrowserSlot({ engine: 'webkit', purpose: 'playwright-spec' });
      if (info && base > 0) info.setTimeout(base + (slot.waitedMs ?? 0));
      let browser: Browser;
      try {
        browser = await engine.launch(options);
      } catch (error) {
        await slot.release();
        throw error;
      }
      browser.once('disconnected', () => void slot.release());
      return browser;
    },
  };
}
