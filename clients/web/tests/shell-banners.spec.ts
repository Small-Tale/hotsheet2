import { expect, type Page, test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';

/** Let layout transitions finish so geometry and captures show the settled state. */
async function settled(page: Page) {
  await page.evaluate(async () => {
    const finite = document
      .getAnimations()
      .filter((animation) => animation.effect?.getComputedTiming().iterations !== Infinity);
    await Promise.all(finite.map((animation) => animation.finished.catch(() => undefined)));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
}

/** Each visible banner's box against the banner slot, plus its corner radius. */
async function bannerGeometry(page: Page) {
  return page.evaluate(() => {
    const slot = document.querySelector('.app-shell__banner')!.getBoundingClientRect();
    const slotTop = slot.top;
    return [...document.querySelectorAll('.app-shell__banner [data-component="state-banner"]')].map((banner) => {
      const box = banner.getBoundingClientRect();
      return {
        component:
          banner
            .closest('[data-component$="-banner"]:not([data-component="state-banner"])')
            ?.getAttribute('data-component') ?? 'connection-state-banner',
        left: Math.round(box.left - slot.left),
        right: Math.round(slot.right - box.right),
        top: Math.round(box.top),
        offset: Math.round(box.top - slotTop),
        bottom: Math.round(box.bottom),
        radius: parseFloat(getComputedStyle(banner).borderTopLeftRadius),
      };
    });
  });
}

const sourceStale = {
  kind: 'compatible',
  revisionMismatch: false,
  sourceStale: true,
  canRestartServer: false,
  server: { generation: 'hs2', build_revision: 'fixture-build' },
};

// HS2-YCB15Z: every application banner uses one inset treatment — rounded, inset by the same amount
// from the shell edges, one gap apart when stacked — at wide, medium, and narrow widths and back.
test('insets every application banner consistently (HS2-YCB15Z)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openDemoProject(page, false, 1, undefined, undefined, {
    openExtras: {
      setupWarning: 'Synthetic setup warning for the banner layout test.',
      codexHooksChanged: '.codex/hooks.json',
      compatibility: sourceStale,
    },
  });
  for (const component of ['project-setup-warning-banner', 'codex-hooks-notice-banner', 'source-stale-notice-banner'])
    await expect(page.locator(`[data-component="${component}"]`)).toBeVisible();
  for (const [label, width] of [
    ['wide', 1280],
    ['medium', 900],
    ['narrow', 420],
    ['wide-again', 1280],
  ] as const) {
    await page.setViewportSize({ width, height: 900 });
    await settled(page);
    const banners = await bannerGeometry(page);
    expect(
      banners.map((banner) => banner.component),
      label,
    ).toEqual(['project-setup-warning-banner', 'codex-hooks-notice-banner', 'source-stale-notice-banner']);
    for (const banner of banners) {
      expect(banner.left, `${label} ${banner.component} left inset`).toBeGreaterThan(0);
      expect(banner.left, `${label} ${banner.component} insets match`).toBe(banners[0].left);
      expect(banner.right, `${label} ${banner.component} insets match`).toBe(banners[0].left);
      expect(banner.radius, `${label} ${banner.component} is rounded`).toBeGreaterThan(0);
    }
    const gaps = banners.slice(1).map((banner, index) => banner.top - banners[index].bottom);
    expect(new Set(gaps).size, `${label} one gap between stacked banners`).toBe(1);
    expect(gaps[0], `${label} stacked banners sit one top inset apart`).toBe(banners[0].offset);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
    expect(overflow, `${label}: no horizontal page scroll`).toBeLessThanOrEqual(0);
    await page.screenshot({ path: `target/visual-captures/hs2-ycb15z-banners-${label}.png` });
  }
});

// HS2-YCB15Z: the alert-toned connection banner shares the same inset as the shell banners beside it.
test('insets the connection banner like the other application banners (HS2-YCB15Z)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await openDemoProject(page, false, 1, undefined, undefined, {
    openExtras: {
      setupWarning: 'Synthetic setup warning for the banner layout test.',
      compatibility: { ...sourceStale, sourceStale: false, revisionMismatch: true, clientRevision: 'other-build' },
    },
  });
  await expect(page.getByText('Different server build is running')).toBeVisible();
  await settled(page);
  const banners = await bannerGeometry(page);
  expect(banners).toHaveLength(2);
  expect(banners[1].left).toBe(banners[0].left);
  expect(banners[1].right).toBe(banners[0].right);
  expect(banners[0].left).toBeGreaterThan(0);
  expect(banners[1].top - banners[0].bottom).toBe(banners[0].offset);
  await page.screenshot({ path: 'target/visual-captures/hs2-ycb15z-connection-banner-wide.png' });
});
