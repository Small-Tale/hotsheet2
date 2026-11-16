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

/** HS2-15W35F: the PWA theme-color metas as [light media, dark media] contents. */
async function themeColors(page: Page) {
  return page.evaluate(() =>
    ['light', 'dark'].map(
      (scheme) =>
        document
          .querySelector(`meta[name="theme-color"][media="(prefers-color-scheme: ${scheme})"]`)
          ?.getAttribute('content') ?? null,
    ),
  );
}

/** The theme-color the browser applies: the content of the meta whose media query matches now. */
async function activeThemeColor(page: Page) {
  return page.evaluate(
    () =>
      [...document.querySelectorAll<HTMLMetaElement>('meta[name="theme-color"]')]
        .find((meta) => matchMedia(meta.media).matches)
        ?.getAttribute('content') ?? null,
  );
}

async function openGeneral(page: Page) {
  await page.getByLabel('Settings View').click();
  await page
    .getByRole('navigation', { name: 'App Settings' })
    .getByRole('button', { name: 'General', exact: true })
    .click(); // HS2-W27N1D: Project Settings has its own General
  // HS2-VW7JVV: the picker is Kerf's RadioGroup tiles, marked by the app's data-theme-picker.
  const picker = page.locator('[data-theme-picker]');
  await expect(picker).toHaveAttribute('data-presentation', 'tiles');
  await expect(page.getByRole('radiogroup', { name: 'Theme' })).toBeVisible();
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
// HS2-15W35F: the PWA theme-color metas follow the same pin and return to their media under Auto.
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
    expect(await themeColors(page)).toEqual(['#ffffff', '#1c1c1e']);
    expect(await activeThemeColor(page)).toBe('#ffffff');

    // Dark pins the scheme on a light system; a keyboard arrow moves within the group.
    await radio('Dark').check();
    await expect.poll(async () => (await scheme(page)).classes).toEqual(['kui-dark']);
    expect(await scheme(page)).toMatchObject({ colorScheme: 'dark', stored: 'dark' });
    // HS2-15W35F: the browser and installed-app chrome color follows the pin on a light system.
    expect(await themeColors(page)).toEqual(['#1c1c1e', '#1c1c1e']);
    expect(await activeThemeColor(page)).toBe('#1c1c1e');
    // WebKit, like Safari, does not focus a radio on click, so focus it before the keyboard steps.
    await radio('Dark').focus();
    // HS2-VW7JVV: arrow keys move the selection between Kerf tiles, and each move applies its scheme.
    await page.keyboard.press('ArrowLeft');
    await expect(radio('Light')).toBeChecked();
    await expect.poll(async () => (await scheme(page)).classes).toEqual(['kui-light']);
    await page.keyboard.press('ArrowRight');
    await expect(radio('Dark')).toBeChecked();
    await expect(radio('Dark')).toBeFocused();
    await expect.poll(async () => (await scheme(page)).classes).toEqual(['kui-dark']);
    // Re-rendering General from the stored preference keeps the Dark tile selected.
    const appSettings = page.getByRole('navigation', { name: 'App Settings' });
    await appSettings.getByRole('button', { name: 'Accounts', exact: true }).click();
    await expect(picker).toHaveCount(0);
    await appSettings.getByRole('button', { name: 'General', exact: true }).click();
    await expect(radio('Dark')).toBeChecked();
    await expect(picker.locator('input:checked')).toHaveCount(1);
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
    expect(await themeColors(page)).toEqual(['#1c1c1e', '#1c1c1e']);
    await expect(reopened.getByRole('radio', { name: 'Dark', exact: true })).toBeChecked();

    // Light pins light even when the system turns dark.
    await page.emulateMedia({ colorScheme: 'dark' });
    await reopened.getByRole('radio', { name: 'Light', exact: true }).check();
    await expect.poll(async () => (await scheme(page)).classes).toEqual(['kui-light']);
    expect(await scheme(page)).toMatchObject({ colorScheme: 'light', stored: 'light' });
    expect(await themeColors(page)).toEqual(['#ffffff', '#ffffff']);
    expect(await activeThemeColor(page)).toBe('#ffffff');
    await settled(page);
    await page.screenshot({ path: 'target/visual-captures/hs2-efths6-theme-light-wide.png' });

    // Auto releases the pin and follows the (now dark) system again; nothing stays stored.
    await reopened.getByRole('radio', { name: 'Auto', exact: true }).check();
    await expect.poll(async () => (await scheme(page)).classes).toEqual([]);
    expect(await scheme(page)).toMatchObject({ stored: null });
    expect(await themeColors(page)).toEqual(['#ffffff', '#1c1c1e']);
    expect(await activeThemeColor(page)).toBe('#1c1c1e');
    expect(await page.evaluate(() => matchMedia('(prefers-color-scheme: dark)').matches)).toBe(true);
    await settled(page);
    await page.screenshot({ path: 'target/visual-captures/hs2-efths6-theme-auto-system-dark-wide.png' });
  });
});
