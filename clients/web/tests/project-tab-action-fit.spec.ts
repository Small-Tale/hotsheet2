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
    await page.getByRole('button', { name: 'Local Project' }).click();
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

// HS2-B0RJBY: the Global Dashboards buttons fold into one menu before the launcher gives up its label,
// Systems stays reachable from that menu, and the buttons return once the strip has room for them.
test('HS2-B0RJBY folds the dashboards before the launcher compacts and keeps Systems reachable', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await openDemoProject(page, false, 1, DEFAULT_TERMINALS);
  const strip = page.locator('.project-tab-bar:not(.project-tab-bar--mobile)'),
    tabs = strip.locator('[data-tab-kind="project"]'),
    launcher = strip.locator('[data-component="quick-ticket-composer-launcher"]'),
    systemsButton = strip.getByRole('button', { name: 'Systems', exact: true }),
    menu = strip.locator('[data-dashboard-menu]');
  // A second project gives the strip a fold width well above the phone breakpoint.
  await page.route('**/__hotsheet/folders/choose', (route) =>
    route.fulfill({ json: { path: '/work/synthetic-alpha' } }),
  );
  await page.route('**/__hotsheet/projects/open', (route) =>
    route.fulfill({
      status: 201,
      json: {
        ...project('demo-checkout', '/work/synthetic-alpha'),
        id: 'synthetic-alpha',
        apiPath: '/__hotsheet/project-api/demo-checkout',
      },
    }),
  );
  await strip.getByRole('button', { name: 'Add Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await expect(tabs).toHaveCount(2);
  // Kerf reports the fit on the next frame after a resize, and the strip re-renders on the frame after.
  const settle = () =>
    page.evaluate(
      () =>
        new Promise<void>((resolve) => {
          let frames = 4;
          const tick = () => {
            frames -= 1;
            if (frames === 0) resolve();
            else requestAnimationFrame(tick);
          };
          requestAnimationFrame(tick);
        }),
    );
  await expect(systemsButton).toBeVisible();
  await expect(menu).toHaveCount(0);
  await expect(launcher).toHaveText('New ticket…');
  for (const pass of ['first', 'second']) {
    // Narrow until the dashboards fold while the launcher still has its full label.
    let width = 1600;
    // 10px steps stay well under the room folding frees, so the launcher keeps its label at the fold.
    while ((await menu.count()) === 0 && width > 1000) {
      width -= 10;
      await page.setViewportSize({ width, height: 900 });
      await settle();
    }
    await expect(menu, pass).toBeVisible();
    await expect(systemsButton).toHaveCount(0);
    await expect(launcher).toHaveText('New ticket…');
    if (pass === 'first') {
      await strip.screenshot({ path: 'target/visual-captures/hs2-b0rjby-strip-folded.png' });
      // Systems is one menu choice away, and choosing it switches the shell like the button.
      await menu.getByRole('button', { name: 'Global Dashboards' }).click();
      await page.locator('[data-action="set-shell-mode"][data-shell-mode="systems"]').click();
      await expect(page.locator('.project-tab-bar').first()).toHaveAttribute('data-mode', 'systems');
      await tabs.first().click();
      await expect(page.locator('.project-tab-bar').first()).toHaveAttribute('data-mode', 'project');
    }
    await page.setViewportSize({ width: 1600, height: 900 });
    await expect(systemsButton).toBeVisible();
    await expect(menu).toHaveCount(0);
    await expect(launcher).toHaveText('New ticket…');
  }
});
