import { expect, type Page, type Route, test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';

/** One pending request in the server's `RemoteDeviceList.pending` shape, made now by the page's (installed)
 * clock with the server's ten-minute pairing window; synthetic content only. */
async function request(page: Page, id: string, label: string) {
  const now = Math.floor((await page.evaluate(() => Date.now())) / 1000);
  return {
    id,
    label,
    origin: 'https://synthetic-tablet.example',
    created_at: now - 5,
    expires_at: now + 600,
    approved: false,
  };
}

interface Host {
  pending: Awaited<ReturnType<typeof request>>[];
  revision: number;
  /** `GET /remote-auth/owner` reads (the badge's reads and the Devices list's first read). */
  reads: number;
  rejects: string[];
  /** The change channel's events, in cursor order. */
  events: { cursor: number; store: string; kind: string; id: string; slug: string }[];
  /** Release the Devices list's held long poll with the current list. */
  wake: () => void;
}

/**
 * The host's device-management bridge routes with real shapes: `GET /remote-auth/owner` answers the
 * `RemoteDeviceList`, `/changes?after=` is a genuine long poll held until the list changes, and
 * `POST /reject` drops the request and answers 204.
 */
async function openHost(page: Page): Promise<Host> {
  const held: (() => void)[] = [];
  const host: Host = {
    pending: [],
    revision: 1,
    reads: 0,
    rejects: [],
    events: [],
    wake: () => {
      for (const release of held.splice(0)) release();
    },
  };
  const list = () => ({
    server_id: 'synthetic-server',
    revision: host.revision,
    pair_ttl_seconds: 600,
    manager: { local: true },
    devices: [],
    pending: host.pending,
  });
  await openDemoProject(page, false, 1, undefined, undefined, {
    remoteAuthOwner: async (route: Route) => {
      const url = new URL(route.request().url());
      if (url.pathname.endsWith('/changes')) {
        if (Number(url.searchParams.get('after')) === host.revision)
          await new Promise<void>((resolve) => held.push(resolve));
        return route.fulfill({ json: list() }).catch(() => undefined);
      }
      if (url.pathname.endsWith('/reject')) {
        const id = (route.request().postDataJSON() as { id: string }).id;
        host.rejects.push(id);
        host.pending = host.pending.filter((item) => item.id !== id);
        host.revision += 1;
        return route.fulfill({ status: 204, body: '' });
      }
      host.reads += 1;
      return route.fulfill({ json: list() });
    },
  });
  // The project's cursored change channel: every event after `since`, as `/ws/poll` replays them.
  await page.route('**/ws/poll**', (route) => {
    const since = new URL(route.request().url()).searchParams.get('since'),
      after = since === null ? undefined : Number(since),
      latest = host.events.at(-1)?.cursor ?? 0;
    return route.fulfill({
      json: {
        cursor: after === undefined ? latest : Math.max(after, latest),
        events: after === undefined ? [] : host.events.filter((event) => event.cursor > after),
        overflow: false,
      },
    });
  });
  await expect(page.getByRole('tab', { name: /demo/ })).toBeVisible();
  return host;
}

/** What the server pushes, to device managers only, when the waiting requests change. */
function push(host: Host, cursor: number) {
  host.events.push({ cursor, store: '', kind: 'pairing_requests', id: '', slug: '' });
}

const settingsChoice = (page: Page) => page.locator('[data-segment-value="settings"]');
const devicesRow = (page: Page) => page.locator('[data-item-id="devices"]');
const capture = (page: Page, name: string) =>
  page.screenshot({ path: `target/visual-captures/hs2-bzhmev-${name}.png` });

async function settled(page: Page) {
  await page.evaluate(async () => {
    const finite = document
      .getAnimations()
      .filter((animation) => animation.effect?.getComputedTiming().iterations !== Infinity);
    await Promise.all(finite.map((animation) => animation.finished.catch(() => undefined)));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
}

// HS2-BZHMEV: a host page learns about a new pairing request without opening Settings. The pushed
// event re-reads the list once; Settings and Devices carry the count, a notice names the device, and
// rejecting it from Devices clears every badge. Idle time reads nothing.
test('a new pairing request badges Settings and Devices and announces itself, without polling', async ({ page }) => {
  await page.clock.install();
  await page.setViewportSize({ width: 1280, height: 860 });
  const host = await openHost(page);
  // The change stream's initial resync reads the (empty) list.
  await expect.poll(() => host.reads).toBeGreaterThanOrEqual(1);
  await expect(settingsChoice(page)).toHaveAttribute('aria-label', 'Settings View');
  await expect(settingsChoice(page).locator('.view-mode-switcher__badge')).toHaveCount(0);

  // An idle hour issues no device-list read.
  const idle = host.reads;
  await page.clock.fastForward(60 * 60_000);
  expect(host.reads).toBe(idle);

  // A remote browser asks to pair; the server pushes the anonymous event.
  host.pending = [await request(page, '01SYNTHETICTABLET', 'Synthetic tablet')];
  host.revision += 1;
  push(host, 11);
  await expect.poll(() => host.reads).toBe(idle + 1);
  await expect(settingsChoice(page)).toHaveAttribute('aria-label', 'Settings View, 1 pairing request waiting');
  await expect(settingsChoice(page).locator('.view-mode-switcher__badge')).toHaveText('1');
  const notice = page.locator('.app-toast', { hasText: '“Synthetic tablet” asks to pair with this host.' });
  await expect(notice).toBeVisible();
  await expect(notice).toContainText('Open Settings → Devices to approve or reject it.');
  await settled(page);
  await capture(page, 'settings-badge-wide');

  // The same event seen again (another stream, a duplicate) reads nothing more and announces nothing.
  push(host, 11);
  await page.clock.fastForward(1_000);
  expect(host.reads).toBe(idle + 1);

  // A second request raises the count.
  host.pending = [...host.pending, await request(page, '01SYNTHETICPHONE', 'Synthetic phone with a long device name')];
  host.revision += 1;
  push(host, 12);
  await expect(settingsChoice(page).locator('.view-mode-switcher__badge')).toHaveText('2');

  // Settings shows the count on the Devices category, which leads to the requests.
  await settingsChoice(page).click();
  await expect(devicesRow(page)).toHaveAttribute('aria-label', 'Devices, 2 pairing requests waiting');
  await expect(devicesRow(page).locator('[data-settings-pending="devices"]')).toHaveText('2');
  await settled(page);
  await capture(page, 'devices-badge-wide');
  for (const width of [820, 390]) {
    await page.setViewportSize({ width, height: 860 });
    await settled(page);
    await capture(page, `devices-badge-${String(width)}`);
  }
  // The narrow layout collapses the settings sidebar; opened, its Devices row keeps the count inside the row.
  // Let the transient notice time out first; the badge stays as its record.
  await page.clock.fastForward(30_000);
  await expect(page.locator('.app-toast')).toHaveCount(0);
  await expect(settingsChoice(page).locator('.view-mode-switcher__badge')).toHaveText('2');
  await page.getByRole('button', { name: 'Show Settings Sidebar' }).first().click();
  await devicesRow(page).scrollIntoViewIfNeeded();
  await settled(page);
  await expect(devicesRow(page).locator('[data-settings-pending="devices"]')).toBeVisible();
  await capture(page, 'devices-badge-390-sidebar');
  await page.getByRole('button', { name: 'Hide Settings Sidebar' }).first().click();
  await page.setViewportSize({ width: 1280, height: 860 });
  await settled(page);
  await expect(devicesRow(page).locator('[data-settings-pending="devices"]')).toBeVisible();
  const badge = await devicesRow(page).locator('[data-settings-pending="devices"]').boundingBox(),
    row = await devicesRow(page).boundingBox();
  expect(badge && row && badge.x + badge.width <= row.x + row.width && badge.x > row.x).toBe(true);

  await devicesRow(page).click();
  const settings = page.locator('[data-component="remote-devices-settings"]');
  await expect(settings.locator('[data-remote-pending]')).toHaveCount(2);

  // Rejecting one from Devices: the held list poll answers, and the server's event updates the badges.
  await page.getByRole('button', { name: 'Actions for Synthetic tablet' }).click();
  await page.getByRole('menuitem', { name: 'Reject' }).click();
  await expect.poll(() => host.rejects).toEqual(['01SYNTHETICTABLET']);
  host.wake();
  push(host, 13);
  await expect(settings.locator('[data-remote-pending]')).toHaveCount(1);
  await expect(devicesRow(page).locator('[data-settings-pending="devices"]')).toHaveText('1');
  await expect(settingsChoice(page).locator('.view-mode-switcher__badge')).toHaveText('1');

  // The last one goes too: every badge clears.
  host.pending = [];
  host.revision += 1;
  host.wake();
  push(host, 14);
  await expect(devicesRow(page).locator('[data-settings-pending="devices"]')).toHaveCount(0);
  await expect(settingsChoice(page).locator('.view-mode-switcher__badge')).toHaveCount(0);
  await expect(settingsChoice(page)).toHaveAttribute('aria-label', 'Settings View');
  host.wake();
});
