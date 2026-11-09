import { mkdir } from 'node:fs/promises';

import { expect, test } from '@playwright/test';

test('HS2-7V2EEV closes a missing remembered project on a phone and keeps it forgotten after reload', async ({
  page,
}) => {
  const root = '/work/synthetic-missing',
    captureDir = process.env.HS2_7V2EEV_CAPTURE_DIR;
  let opens = 0;
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.addInitScript((remembered) => {
    if (localStorage.getItem('hs2-7v2eev-seeded')) return;
    localStorage.setItem('hs2-7v2eev-seeded', 'true');
    localStorage.setItem('hotsheet.open-projects', JSON.stringify([remembered]));
    localStorage.setItem('hotsheet.workspace.active-project-root.v1', remembered);
  }, root);
  await page.route('**/__hotsheet/projects/open', async (route) => {
    opens += 1;
    await route.fulfill({ status: 404, json: { error: 'Synthetic project folder is missing' } });
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  const failure = page.locator('.project-restore-error'),
    mobileBar = page.locator('.project-tab-bar--mobile'),
    close = mobileBar.getByRole('button', { name: 'Close synthetic-missing Project' });
  await expect(failure).toContainText('synthetic-missing could not be reopened');
  await expect(close).toBeVisible();

  for (const [name, width] of [
    ['phone', 390],
    ['medium', 940],
    ['wide', 1280],
    ['returned-wide', 1280],
  ] as const) {
    if (name === 'returned-wide') await page.setViewportSize({ width: 390, height: 844 });
    await page.setViewportSize({ width, height: 844 });
    await expect(failure).toBeVisible();
    if (width < 1024) await expect(close).toBeVisible();
    else await expect(mobileBar).toBeHidden();
    if (captureDir) {
      // The shell changes layout at this breakpoint; capture after its finite transition settles.
      await page.waitForTimeout(250);
      await mkdir(captureDir, { recursive: true });
      await page.screenshot({ path: `${captureDir}/hs2-7v2eev-${name}.png` });
    }
  }

  await page.setViewportSize({ width: 390, height: 844 });
  await close.click();
  const dialog = page.getByRole('dialog', { name: 'Close synthetic-missing?' });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(failure).toBeVisible();
  await close.click();
  await dialog.getByRole('button', { name: 'Close Project' }).click();
  await expect(failure).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => localStorage.getItem('hotsheet.open-projects'))).toBe('[]');
  const attempts = opens;
  await page.reload();
  await expect(page.getByRole('button', { name: 'Open Project' })).toBeVisible();
  await expect(failure).toHaveCount(0);
  expect(opens).toBe(attempts);
});
