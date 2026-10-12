import { expect, type Page, test } from '@playwright/test';

// Terminals follow the app's light/dark appearance through token-derived xterm themes (HS2-QNSS9C).
const xtermColors = (page: Page) =>
  page
    .locator('[data-component="terminal-viewport"] .xterm')
    .first()
    .evaluate((xterm) => {
      // xterm paints the theme foreground on its rows and, for the fixture's inverse nano title bar, swaps
      // the theme pair: that cell's text is the theme background.
      const rows = xterm.querySelector('.xterm-rows')!,
        inverse = [...rows.querySelectorAll('span')].find(
          (span) => getComputedStyle(span).backgroundColor !== 'rgba(0, 0, 0, 0)',
        );
      // Space the rows leave shows the viewport, which must match the surface too.
      return {
        background: inverse && getComputedStyle(inverse).color,
        foreground: getComputedStyle(rows).color,
        viewport: getComputedStyle(xterm.querySelector('.xterm-viewport')!).backgroundColor,
      };
    });

for (const width of [1440, 390]) {
  test(`terminals switch between the light and dark xterm themes at ${width}px (HS2-QNSS9C)`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=terminal-drawer');
    const html = page.locator('html'),
      toggle = () =>
        page.evaluate(() => {
          document.querySelector<HTMLElement>('[data-action="catalog-toggle-theme"]')!.click();
        });
    if ((await html.getAttribute('data-theme')) === 'dark') await toggle();
    await expect(html).toHaveAttribute('data-theme', 'light');
    await expect(page.locator('[data-component="terminal-viewport"] .xterm-rows').first()).toBeAttached();
    await expect
      .poll(() => xtermColors(page))
      .toEqual({ background: 'rgb(255, 255, 255)', foreground: 'rgb(29, 29, 31)', viewport: 'rgb(255, 255, 255)' });
    await page.screenshot({ path: `target/visual-captures/hs2-qnss9c-terminal-light-${width}.png` });

    // Switching the app to dark re-themes the mounted terminal without a remount.
    await toggle();
    await expect(html).toHaveAttribute('data-theme', 'dark');
    await expect
      .poll(() => xtermColors(page))
      .toEqual({ background: 'rgb(0, 0, 0)', foreground: 'rgb(245, 245, 247)', viewport: 'rgb(0, 0, 0)' });
    await page.screenshot({ path: `target/visual-captures/hs2-qnss9c-terminal-dark-${width}.png` });

    // And back again.
    await toggle();
    await expect
      .poll(() => xtermColors(page))
      .toEqual({ background: 'rgb(255, 255, 255)', foreground: 'rgb(29, 29, 31)', viewport: 'rgb(255, 255, 255)' });
  });
}

test('a terminal without an explicit app theme follows the system color scheme (HS2-QNSS9C)', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto('/ux-demo?component=terminal-drawer');
  await page.evaluate(() => {
    document.documentElement.removeAttribute('data-theme');
    document.documentElement.classList.remove('wa-dark');
  });
  await expect
    .poll(() => xtermColors(page))
    .toEqual({ background: 'rgb(0, 0, 0)', foreground: 'rgb(245, 245, 247)', viewport: 'rgb(0, 0, 0)' });
  await page.emulateMedia({ colorScheme: 'light' });
  await expect
    .poll(() => xtermColors(page))
    .toEqual({ background: 'rgb(255, 255, 255)', foreground: 'rgb(29, 29, 31)', viewport: 'rgb(255, 255, 255)' });
});
