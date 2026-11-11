import { expect, test } from '@playwright/test';

import type { LiveHistoryList, LiveStoreState } from '../src/api';
import { liveHistoryState } from '../src/overnight-timeline';
import { glanceScenario } from '../src/ux-demo/live-state-fixtures';
import { openDemoProject } from './mobile-project-fixture';

const HOUR = 3_600_000;

/** The demo tab's stores in the server's `GET /live-state` shape; synthetic content only. */
function stores(now: number): LiveStoreState[] {
  return glanceScenario(now)
    .slice(0, 3)
    .map((state) => ({ ...state, checkouts: ['demo-checkout'] }));
}

/** `GET /live-history` for those stores over `[since, now]`: stopped, then each store's current state. */
function history(since: number, now: number): LiveHistoryList {
  return {
    since_ms: since,
    until_ms: now,
    stores: stores(now).map((store, index) => ({
      store: store.store,
      name: store.name,
      checkouts: store.checkouts,
      segments: [
        { state: 'stopped', start_ms: since, end_ms: now - (index + 2) * HOUR },
        { state: liveHistoryState(store), start_ms: now - (index + 2) * HOUR, end_ms: now },
      ],
      completions: [
        { slug: `DEMO-${String(index + 1)}`, at_ms: now - 6 * HOUR },
        { slug: `DEMO-${String(index + 4)}`, at_ms: now - 5 * HOUR },
      ],
    })),
  };
}

test('Stats Overnight shows a lane per project with state segments and completion ticks, read only on open and window change (HS2-9AN4DN)', async ({
  page,
}) => {
  const now = Date.now();
  const reads: number[] = [];
  await openDemoProject(page, false, 1, undefined, undefined, {
    liveState: () => ({ epoch_ms: 1, stores: stores(now) }),
  });
  // Registered after the fixture's catch-all route so it takes precedence.
  await page.route('**/live-history?*', (route) => {
    const since = Number(new URL(route.request().url()).searchParams.get('since_ms'));
    reads.push(since);
    return route.fulfill({ json: history(since, now) });
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByRole('button', { name: 'Cross-Project Stats' }).click();
  const card = page.locator('[data-component="stats-overnight"]');
  await expect(card).toBeVisible();
  await expect.poll(() => reads.length).toBe(1);
  // Standby was never entered: the last 12 hours.
  expect(Math.abs(reads[0] - (now - 12 * HOUR))).toBeLessThan(60_000);
  const lanes = card.locator('[role="listitem"]');
  await expect(lanes).toHaveCount(3);
  // Lanes stay in name order.
  expect(await lanes.evaluateAll((items) => items.map((item) => item.getAttribute('data-store')))).toEqual([
    'store-hotsheet',
    'store-website',
    'store-review',
  ]);
  const website = card.locator('[data-store="store-website"]');
  const segments = website.locator('rect.stats-overnight__segment');
  await expect(segments).toHaveCount(2);
  await expect(segments.nth(0)).toHaveAttribute('data-state', 'stopped');
  await expect(segments.nth(1)).toHaveAttribute('data-state', 'may_be_stuck');
  // Hover text: state, times, and duration.
  await expect(segments.nth(1).locator('title')).toHaveText(/^May be stuck · .+ – .+ · 2h 0\dm$/u);
  await expect(website.locator('line.stats-overnight__tick')).toHaveCount(2);
  await expect(card.locator('[data-store="store-review"] rect[data-state="waiting_on_you"]')).toHaveCount(1);
  await expect(card.locator('[data-overnight-legend]')).toContainText('Waiting on CI');
  await expect(card.locator('[data-overnight-legend]')).toContainText('Completed ticket');
  // Above This Mac, below Right now.
  const order = await page.evaluate(() =>
    ['stats-right-now', 'stats-overnight'].map(
      (name) => document.querySelector(`[data-component="${name}"]`)?.getBoundingClientRect().top ?? -1,
    ),
  );
  expect(order[0]).toBeGreaterThanOrEqual(0);
  expect(order[0]).toBeLessThan(order[1]);

  // Rendering and the display clock read nothing more.
  expect(reads).toHaveLength(1);

  // Choosing the last 24 hours reads once more, from 24 hours ago.
  const select = card.locator('kui-select[name="stats-overnight-window"]');
  await select.locator('.kui-select__trigger').click();
  await select.locator('.kui-select__option[data-value="24h"]').click();
  await expect.poll(() => reads.length).toBe(2);
  expect(Math.abs(reads[1] - (now - 24 * HOUR))).toBeLessThan(60_000);
  await expect(select).toHaveJSProperty('value', '24h');
  // Captures show the resting card, not the select's focus ring.
  await page.evaluate(() => {
    (document.activeElement as HTMLElement | null)?.blur();
  });

  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    for (const [label, width] of [
      ['wide', 1440],
      ['medium', 1024],
      ['narrow', 600],
      ['wide-again', 1440],
    ] as const) {
      await page.setViewportSize({ width, height: 900 });
      await expect(card).toBeVisible();
      const box = await card.boundingBox();
      expect(box!.width).toBeGreaterThan(0);
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
      await card.screenshot({ path: `target/visual-captures/hs2-9an4dn-overnight-${scheme}-${label}.png` });
    }
  }
});
