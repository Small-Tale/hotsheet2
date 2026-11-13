import { expect, type Page, test } from '@playwright/test';

import type { RegisteredProject } from '../src/registered-projects-model';
import { installTerminalFixture } from './terminal-feedback-fixture';

const OPEN = 'web-4c1d2e3f4a5b';

/**
 * The host-only bridge routes App Settings → Projects uses, as a stateful fixture with the real
 * shapes: GET lists every registered project with `available`, DELETE answers `{ removed: true }`,
 * and removing the project open in a window is refused with 409 and the server's reason.
 */
async function routeRegisteredProjects(page: Page) {
  let list: RegisteredProject[] = [
    { id: 'api-1a2b3c4d5e6f', alias: 'api', root: '/Users/synthetic/Projects/api', available: true },
    { id: OPEN, alias: 'web', root: '/Users/synthetic/Projects/web', available: true },
    {
      id: 'archive-9f8e7d6c5b4a',
      alias: 'archive',
      root: '/Volumes/Synthetic Backup/old clones/a very long folder name that wraps on a phone/archive',
      available: false,
    },
  ];
  const requests: string[] = [];
  await page.route('**/__hotsheet/registered-projects**', async (route) => {
    const request = route.request(),
      url = new URL(request.url());
    requests.push(`${request.method()} ${url.pathname}`);
    if (request.method() === 'GET') return route.fulfill({ json: list });
    const id = decodeURIComponent(url.pathname.split('/').pop() ?? '');
    if (id === OPEN)
      return route.fulfill({ status: 409, json: { error: 'web is open in a window; close it before removing it.' } });
    list = list.filter((project) => project.id !== id);
    return route.fulfill({ json: { removed: true } });
  });
  return requests;
}

async function settled(page: Page) {
  await page.evaluate(async () => {
    const finite = document
      .getAnimations()
      .filter((animation) => animation.effect?.getComputedTiming().iterations !== Infinity);
    await Promise.all(finite.map((animation) => animation.finished.catch(() => undefined)));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
}

async function remove(page: Page, name: string) {
  await page.getByRole('button', { name: `Actions for ${name}` }).click();
  const menu = page.getByRole('menu', { name: `Actions for ${name}` });
  await menu.getByRole('menuitem', { name: 'Remove from Hot Sheet' }).click();
  await expect(menu).toBeHidden();
}

// HS2-R806FZ: App Settings → Projects lists every registered project, marks a missing folder, refuses
// to remove a project open in a window, and removes another, through the shipped settings parent.
test('reviews and removes registered projects in App Settings → Projects', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 860 });
  await installTerminalFixture(page);
  const requests = await routeRegisteredProjects(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Projects', exact: true }).click();

  const settings = page.locator('[data-component="registered-projects-settings"]');
  await expect(page.locator('[data-component="settings-workspace"]')).toHaveAttribute(
    'data-settings-category',
    'projects',
  );
  await expect(settings.getByRole('heading', { name: 'Projects' })).toBeVisible();
  const rows = settings.locator('[data-registered-project]');
  await expect(rows).toHaveCount(3);
  // Sorted by name; only the missing folder carries the badge and the folder-x icon.
  await expect(rows.nth(0)).toContainText('api');
  await expect(rows.nth(1)).toContainText('archive');
  await expect(rows.nth(1)).toContainText('Folder Missing');
  await expect(rows.nth(1)).toHaveAttribute('data-project-available', 'false');
  await expect(
    rows.nth(1).locator('svg.lucide-folder-x, [data-lucide="folder-x"], [data-icon="folder-x"]'),
  ).toHaveCount(1);
  await expect(rows.nth(0)).not.toContainText('Folder Missing');
  await expect(rows.nth(2)).toContainText('/Users/synthetic/Projects/web');
  expect(requests).toEqual(['GET /__hotsheet/registered-projects']);
  await settled(page);
  await page.screenshot({ path: test.info().outputPath('hs2-r806fz-projects-wide.png') });

  // Medium and narrow widths keep every row, including the missing folder, readable and inside the
  // workspace; then back to wide.
  for (const width of [820, 390]) {
    await page.setViewportSize({ width, height: 860 });
    await settled(page);
    await expect(rows).toHaveCount(3);
    const fits = await settings.evaluate((node) => node.scrollWidth <= node.clientWidth + 1);
    expect(fits, `projects settings fits at ${String(width)}px`).toBe(true);
    await settings.screenshot({ path: test.info().outputPath(`hs2-r806fz-projects-${String(width)}.png`) });
  }
  await page.setViewportSize({ width: 1280, height: 860 });
  await settled(page);

  // A project open in a window is refused with the server's reason, and stays listed.
  await remove(page, 'web');
  await expect(settings.getByRole('alert')).toHaveText('web is open in a window; close it before removing it.');
  await expect(rows).toHaveCount(3);

  // Removing the missing archive clears the message, reloads, and drops the row.
  await remove(page, 'archive');
  await expect(rows).toHaveCount(2);
  await expect(settings.getByRole('alert')).toHaveCount(0);
  await expect(settings.getByText('Folder Missing')).toHaveCount(0);
  expect(requests).toEqual([
    'GET /__hotsheet/registered-projects',
    `DELETE /__hotsheet/registered-projects/${OPEN}`,
    'DELETE /__hotsheet/registered-projects/archive-9f8e7d6c5b4a',
    'GET /__hotsheet/registered-projects',
  ]);

  // Right-clicking a row opens the same menu.
  await rows.nth(0).click({ button: 'right' });
  await expect(page.getByRole('menuitem', { name: 'Remove from Hot Sheet' })).toBeVisible();
  await page.keyboard.press('Escape');

  await expect(rows).toHaveCount(2);
});
