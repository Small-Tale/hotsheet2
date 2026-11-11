import { expect, type Page, test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';

/** The document's resolved color scheme and the Kerf scope classes pinning it. */
async function scheme(page: Page) {
  return page.evaluate(() => ({
    colorScheme: getComputedStyle(document.documentElement).colorScheme,
    classes: [...document.documentElement.classList].filter((name) => name.startsWith('kui-')).sort(),
    stored: localStorage.getItem('hotsheet.theme'),
  }));
}

async function openGeneral(page: Page) {
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'General', exact: true }).click();
  const picker = page.locator('[data-component="theme-picker"]');
  await expect(picker.getByRole('radiogroup', { name: 'Theme' })).toBeVisible();
  return picker;
}

/** Let layout transitions finish so a capture shows the settled state. */
async function settled(page: Page) {
  await page.evaluate(async () => {
    const finite = document
      .getAnimations()
      .filter((animation) => animation.effect?.getComputedTiming().iterations !== Infinity);
    await Promise.all(finite.map((animation) => animation.finished.catch(() => undefined)));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
}

// HS2-EFTHS6: App Settings → General → Theme chooses Auto (default, follows the system), Light, or Dark
// for this browser; the choice pins the whole app's scheme, survives a reload, and Auto releases it.
test.describe('App Settings theme (HS2-EFTHS6)', () => {
  test.use({ colorScheme: 'light' });

  test('chooses auto, light, or dark and keeps the choice across a reload', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 860 });
    await openDemoProject(page);
    const picker = await openGeneral(page);
    const radio = (name: string) => picker.getByRole('radio', { name, exact: true });

    // Auto is the default and follows the (light) system.
    await expect(radio('Auto')).toBeChecked();
    expect(await scheme(page)).toMatchObject({ colorScheme: 'light dark', classes: [], stored: null });

    // Dark pins the scheme on a light system; a keyboard arrow moves within the group.
    await radio('Dark').check();
    await expect.poll(async () => (await scheme(page)).classes).toEqual(['kui-dark']);
    expect(await scheme(page)).toMatchObject({ colorScheme: 'dark', stored: 'dark' });
    await expect(radio('Dark')).toBeFocused();
    for (const [label, width] of [
      ['wide', 1280],
      ['medium', 900],
      ['narrow', 420],
    ] as const) {
      await page.setViewportSize({ width, height: 860 });
      await picker.scrollIntoViewIfNeeded();
      await settled(page);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      expect(overflow, `${label}: no horizontal page scroll`).toBeLessThanOrEqual(0);
      await page.screenshot({ path: `target/visual-captures/hs2-efths6-theme-dark-${label}.png` });
    }
    await page.setViewportSize({ width: 1280, height: 860 });

    // The choice is applied before the first render after a reload.
    await page.reload();
    const reopened = await openGeneral(page);
    expect(await scheme(page)).toMatchObject({ colorScheme: 'dark', classes: ['kui-dark'], stored: 'dark' });
    await expect(reopened.getByRole('radio', { name: 'Dark', exact: true })).toBeChecked();

    // Light pins light even when the system turns dark.
    await page.emulateMedia({ colorScheme: 'dark' });
    await reopened.getByRole('radio', { name: 'Light', exact: true }).check();
    await expect.poll(async () => (await scheme(page)).classes).toEqual(['kui-light']);
    expect(await scheme(page)).toMatchObject({ colorScheme: 'light', stored: 'light' });
    await settled(page);
    await page.screenshot({ path: 'target/visual-captures/hs2-efths6-theme-light-wide.png' });

    // Auto releases the pin and follows the (now dark) system again; nothing stays stored.
    await reopened.getByRole('radio', { name: 'Auto', exact: true }).check();
    await expect.poll(async () => (await scheme(page)).classes).toEqual([]);
    expect(await scheme(page)).toMatchObject({ stored: null });
    expect(await page.evaluate(() => matchMedia('(prefers-color-scheme: dark)').matches)).toBe(true);
    await settled(page);
    await page.screenshot({ path: 'target/visual-captures/hs2-efths6-theme-auto-system-dark-wide.png' });
  });
});
