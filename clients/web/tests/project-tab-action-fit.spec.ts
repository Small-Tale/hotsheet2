import { expect, test } from '@playwright/test';

import { DEFAULT_TERMINALS, openDemoProject, project } from './mobile-project-fixture';

// HS2-RNR35H, HS2-A2CSNB: Kerf's public TabBar fit report compacts the strip's New ticket launcher while
// the project tabs overflow, and the full label returns only once it fits beside the tabs again.
test('HS2-A2CSNB compacts the strip launcher from the public TabBar fit and restores it when room returns', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await openDemoProject(page, false, 1, DEFAULT_TERMINALS);
  const strip = page.locator('.project-tab-bar:not(.project-tab-bar--mobile)'),
    tabs = strip.locator('[data-tab-kind="project"]'),
    launcher = strip.locator('[data-component="quick-ticket-composer-launcher"]');
  await expect(tabs).toHaveCount(1);
  for (const name of ['synthetic-alpha', 'synthetic-bravo', 'synthetic-charlie']) {
    // Each synthetic folder opens as its own project; the fixture's other project routes still answer.
    await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: `/work/${name}` } }));
    await page.route('**/__hotsheet/projects/open', (route) =>
      route.fulfill({
        status: 201,
        json: {
          ...project('demo-checkout', `/work/${name}`),
          id: name,
          apiPath: '/__hotsheet/project-api/demo-checkout',
        },
      }),
    );
    const count = await tabs.count();
    await strip.getByRole('button', { name: 'Add Project' }).click();
    await expect(tabs).toHaveCount(count + 1);
  }
  await expect(launcher).toHaveText('New ticket…');
  await page.setViewportSize({ width: 1024, height: 800 });
  await expect(launcher).toHaveText('Ticket…');
  await strip.screenshot({ path: 'target/visual-captures/hs2-a2csnb-strip-compact-1024.png' });
  // Growing by less than the full label needs keeps the compact label: no oscillation.
  await page.setViewportSize({ width: 1040, height: 800 });
  await expect(launcher).toHaveText('Ticket…');
  await page.setViewportSize({ width: 1600, height: 900 });
  await expect(launcher).toHaveText('New ticket…');
  await strip.screenshot({ path: 'target/visual-captures/hs2-a2csnb-strip-full-1600.png' });
  await page.setViewportSize({ width: 1024, height: 800 });
  await expect(launcher).toHaveText('Ticket…');
  // Closing tabs frees strip room without the window growing.
  for (let index = 0; index < 3; index += 1) {
    await tabs.last().hover();
    await tabs.last().locator('.kui-tab-bar-item__close, [data-action="close-project-tab"]').first().click();
    const dialog = page.getByRole('dialog', { name: /^Close / });
    if (await dialog.count()) await dialog.getByRole('button', { name: 'Close Project' }).click();
  }
  await expect(tabs).toHaveCount(1);
  await expect(launcher).toHaveText('New ticket…');
});
