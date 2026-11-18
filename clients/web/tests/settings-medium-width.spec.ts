import { expect, type Page, test } from '@playwright/test';

import { desktopApp } from './desktop-draft-files-fixture';
import { openMockedProject } from './open-mocked-project';

/** Wait out the Workbench rails' responsive collapse or restore motion before measuring a settled layout. */
async function railMotionSettled(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          document
            .getAnimations()
            .filter(
              (animation) =>
                animation.playState === 'running' &&
                animation instanceof CSSTransition &&
                animation.transitionProperty === 'transform',
            ).length,
      ),
    )
    .toBe(0);
}

/** How far any painted part of App Settings reaches past its workspace column or the viewport. */
function settingsOverflow(page: Page) {
  return page.evaluate(() => {
    const workspace = document.querySelector('[data-component="settings-workspace"]')!,
      column = workspace.closest('.app-shell__workspace')!.getBoundingClientRect(),
      left = Math.max(0, column.left),
      right = Math.min(window.innerWidth, column.right);
    let worst = 0;
    for (const element of [workspace, ...workspace.querySelectorAll('*')]) {
      const box = element.getBoundingClientRect();
      if (box.width === 0 || box.height === 0) continue;
      worst = Math.max(worst, left - box.left, box.right - right);
    }
    return Math.round(worst);
  });
}

// HS2-21E07Y: App Settings fits between the rails at a medium-narrow window, across wide → 600 → wide.
// The original captures caught the rails mid-way through their 200 ms responsive collapse; this pins the
// settled layout, where no settings content may reach past its column or the viewport.
test('keeps App Settings content inside its column at 600 px and back (HS2-21E07Y)', async ({
  context,
  baseURL,
  browserName,
}) => {
  test.setTimeout(120_000);
  const app = await desktopApp(context, baseURL!);
  try {
    const page = await app.launch();
    await page.setViewportSize({ width: 1280, height: 900 });
    await openMockedProject(page);
    await page.getByLabel('Settings View').click();
    await page
      // HS2-FN018Q: App Settings and Project Settings each have a General category.
      .getByRole('navigation', { name: 'App Settings' })
      .getByRole('button', { name: 'General', exact: true })
      .click();
    const workspace = page.locator('[data-component="settings-workspace"]');
    await expect(workspace).toHaveAttribute('data-settings-category', 'general');
    for (const [label, width] of [
      ['wide', 1280],
      ['medium', 1024],
      ['narrow', 600],
      ['wide-again', 1280],
    ] as const) {
      await page.setViewportSize({ width, height: 900 });
      await railMotionSettled(page);
      await expect(workspace).toBeVisible();
      await expect.poll(() => settingsOverflow(page)).toBeLessThanOrEqual(0);
      await page.screenshot({
        path: `target/visual-captures/hs2-21e07y-settings-${label}-${width}-${browserName}.png`,
      });
    }
  } finally {
    await app.stop();
  }
});
