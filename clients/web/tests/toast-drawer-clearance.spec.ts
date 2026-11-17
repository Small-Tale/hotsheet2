import { expect, type Locator, type Page, test } from '@playwright/test';

import { desktopApp } from './desktop-draft-files-fixture';
import { openMockedProject } from './open-mocked-project';

interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

const overlaps = (a: Box, b: Box) =>
  a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

async function geometry(page: Page, toast: Locator, restore: Locator) {
  const [toastBox, restoreBox] = await Promise.all([toast.boundingBox(), restore.boundingBox()]);
  const viewport = page.viewportSize()!;
  return {
    clear: Boolean(toastBox && restoreBox && !overlaps(toastBox, restoreBox)),
    inside: Boolean(
      toastBox &&
      toastBox.x >= 0 &&
      toastBox.y >= 0 &&
      toastBox.x + toastBox.width <= viewport.width &&
      toastBox.y + toastBox.height <= viewport.height,
    ),
  };
}

// HS2-T90WCZ: a bottom-end app toast rises above the collapsed terminal drawer's round restore button
// instead of sliding under it, at wide, medium, and phone widths and across a wide-narrow-wide resize.
test('keeps the app toast clear of the terminal drawer restore button (HS2-T90WCZ)', async ({
  context,
  baseURL,
  browserName,
}) => {
  test.setTimeout(120_000);
  // Hold each toast past its 2.5 s lifetime so the resize sequence runs while it is on screen.
  await context.addInitScript(() => {
    const original = window.setTimeout.bind(window);
    window.setTimeout = ((handler: TimerHandler, delay?: number, ...rest: unknown[]) =>
      original(handler, delay === 2_500 ? 600_000 : delay, ...rest)) as typeof window.setTimeout;
  });
  const app = await desktopApp(context, baseURL!);
  try {
    const ticket = await app.server.request<{ slug: string }>(`/checkouts/${app.server.checkoutId}/tickets`, 'POST', {
      title: 'Synthetic toast clearance ticket',
      category: 'task',
    });
    const page = await app.launch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openMockedProject(page);
    const restore = page.getByRole('button', { name: 'Show Terminal Drawer' });
    await expect(restore).toBeVisible();
    await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${ticket.slug}"]`).click();
    await page
      .locator('#app-right-rail')
      .getByRole('button', { name: `Copy ticket number ${ticket.slug}` })
      .click();
    const toast = page.locator('wa-toast-item:has(.app-toast)');
    await expect(toast).toContainText(`${ticket.slug} copied to clipboard.`);

    for (const [label, width] of [
      ['wide', 1440],
      ['medium', 760],
      ['narrow', 390],
      ['wide-return', 1440],
    ] as const) {
      await page.setViewportSize({ width, height: label === 'narrow' ? 844 : 900 });
      await expect(restore).toBeVisible();
      await expect(toast).toBeVisible();
      await expect.poll(() => geometry(page, toast, restore)).toEqual({ clear: true, inside: true });
      // The restore button stays the topmost element at its centre, so it remains clickable.
      const box = (await restore.boundingBox())!;
      expect(
        await page.evaluate(
          ([x, y]) => document.elementFromPoint(x, y)?.closest('button')?.getAttribute('aria-label') ?? null,
          [box.x + box.width / 2, box.y + box.height / 2],
        ),
      ).toBe('Show Terminal Drawer');
      // Let the Web Awesome show animation (inside the item's shadow root) settle for the capture.
      await page.waitForTimeout(400);
      await page.screenshot({ path: `target/visual-captures/hs2-t90wcz-toast-${label}-${width}-${browserName}.png` });
    }
  } finally {
    await app.stop();
  }
});
