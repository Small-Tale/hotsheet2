import { expect, type Page, test } from '@playwright/test';

import { installTerminalFixture, project } from './terminal-feedback-fixture';

// HS2-AQ40G7: adding or removing a drawer tab never passes through the drawer's Project grid.
// The grid mounts every terminal as a tile (reparenting the live instance and claiming the tile's
// PTY size), so a one-frame detour redraws and resizes terminals the person never asked to see.

type TransitionWindow = typeof window & {
  __drawerModes: string[];
  __drawerGridMounts: number;
  __terminalFeedbackSockets: Array<{ url: string; sent: unknown[] }>;
};

function deferred() {
  let resolve: () => void = () => undefined;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

/** Gate terminal create and delete behind promises so the pending state is observable. */
async function installGatedTerminals(page: Page) {
  const sessions = [
      { id: 'nano', alive: true, busy: false, cwd: project.root },
      { id: 'second', alive: true, busy: false, cwd: project.root },
    ],
    gates = { create: deferred(), remove: deferred() };
  await page.route(
    (url) => /\/terminals(\/[^/]+)?$/.test(url.pathname) && url.pathname.startsWith(project.apiPath),
    async (route) => {
      const request = route.request(),
        path = new URL(request.url()).pathname,
        id = path.match(/\/terminals\/([^/]+)$/)?.[1];
      if (request.method() === 'POST' && !id) {
        await gates.create.promise;
        sessions.push({ id: 'terminal-new', alive: true, busy: false, cwd: project.root });
        return route.fulfill({ status: 201, json: sessions.at(-1) });
      }
      if (request.method() === 'DELETE' && id) {
        await gates.remove.promise;
        sessions.splice(
          sessions.findIndex((session) => session.id === id),
          1,
        );
        return route.fulfill({ status: 204, body: '' });
      }
      if (!id) return route.fulfill({ json: sessions });
      return route.fallback();
    },
  );
  return gates;
}

async function recordDrawerModes(page: Page) {
  await page.evaluate(() => {
    const target = window as TransitionWindow;
    target.__drawerModes = [];
    target.__drawerGridMounts = 0;
    const record = () => {
      const drawer = document.querySelector<HTMLElement>('[data-component="terminal-drawer"]');
      const mode = drawer?.dataset.mode ?? 'none';
      if (target.__drawerModes.at(-1) !== mode) target.__drawerModes.push(mode);
      if (drawer?.querySelector('[data-component="terminal-dashboard"]')) target.__drawerGridMounts += 1;
    };
    record();
    new MutationObserver(record).observe(document.body, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ['data-mode'],
    });
  });
}

/** Each attach socket's sent-message count, so later claims are read per socket. */
function sentCounts(page: Page) {
  return page.evaluate(() =>
    (window as TransitionWindow).__terminalFeedbackSockets.map((socket) => socket.sent.length),
  );
}

/** Visible resize claims sent after `counts`; a parked terminal only sends `visible:false`. */
function visibleClaimsSince(page: Page, counts: number[]) {
  return page.evaluate(
    (before) =>
      (window as TransitionWindow).__terminalFeedbackSockets.flatMap((socket, index) =>
        socket.sent
          .slice(before[index] ?? 0)
          .filter(
            (value): value is string =>
              typeof value === 'string' && value.includes('"resize"') && value.includes('"visible":true'),
          )
          .map((value) => ({ url: socket.url, value })),
      ),
    counts,
  );
}

for (const width of [1280, 900, 390]) {
  test(`adds and removes a drawer terminal without passing through the Project grid at ${width}px (HS2-AQ40G7)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 800 });
    await installTerminalFixture(page);
    const gates = await installGatedTerminals(page);
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    const drawer = page.locator('[data-component="terminal-drawer"]');
    await drawer.locator('[data-tab-kind="terminal"][data-terminal-id="nano"] .kui-tab-bar-item__select').click();
    await expect(drawer).toHaveAttribute('data-mode', 'dedicated');
    await expect(drawer.locator('[data-component="terminal-viewport"][data-terminal-id="nano"]')).toHaveAttribute(
      'data-connection',
      'connected',
    );
    await recordDrawerModes(page);

    // Add: a pending tab is selected at once and shows a busy placeholder until the terminal exists.
    await drawer.getByRole('button', { name: 'New Drawer Item' }).click();
    await drawer.getByRole('menuitem', { name: 'Terminal' }).click();
    const pendingTab = drawer.locator('[data-pending-tab-id]');
    await expect(pendingTab).toHaveCount(1);
    await expect(pendingTab.getByRole('tab')).toHaveAttribute('aria-selected', 'true');
    await expect(pendingTab).toHaveAttribute('aria-busy', 'true');
    await expect(pendingTab.getByRole('tab')).toHaveAccessibleName('Terminal');
    await expect(drawer).toHaveAttribute('data-mode', 'pending');
    await expect(drawer.locator('.terminal-drawer__content [aria-busy="true"]')).toBeVisible();
    // From here the previous terminal is parked; any visible claim but the new tab's would be a grid tile.
    const claimsBefore = await sentCounts(page);
    await drawer.screenshot({ path: `target/visual-captures/hs2-aq40g7-pending-tab-${width}.png` });
    if (width === 1280) {
      // Wide to narrow to wide while starting: the placeholder keeps the selection throughout.
      for (const next of [390, 1280]) {
        await page.setViewportSize({ width: next, height: 800 });
        await expect(drawer).toHaveAttribute('data-mode', 'pending');
        await expect(pendingTab.getByRole('tab')).toHaveAttribute('aria-selected', 'true');
        await drawer.screenshot({ path: `target/visual-captures/hs2-aq40g7-pending-tab-resized-${next}.png` });
      }
    }
    gates.create.resolve();
    const created = drawer.locator('[data-tab-kind="terminal"][data-terminal-id="terminal-new"]');
    await expect(created.getByRole('tab')).toHaveAttribute('aria-selected', 'true');
    await expect(pendingTab).toHaveCount(0);
    await expect(drawer).toHaveAttribute('data-mode', 'dedicated');
    await expect(
      drawer.locator('[data-component="terminal-viewport"][data-terminal-id="terminal-new"]'),
    ).toHaveAttribute('data-connection', 'connected');

    // Remove: closing the selected tab selects its neighbor directly.
    await created.hover();
    await created.locator('[data-action="close-terminal-tab"]').click();
    await expect(
      drawer.locator('[data-tab-kind="terminal"][data-terminal-id="second"]').getByRole('tab'),
    ).toHaveAttribute('aria-selected', 'true');
    gates.remove.resolve();
    await expect(created).toHaveCount(0);
    await expect(drawer).toHaveAttribute('data-mode', 'dedicated');

    const recorded = await page.evaluate(() => ({
      modes: (window as TransitionWindow).__drawerModes,
      gridMounts: (window as TransitionWindow).__drawerGridMounts,
    }));
    expect(recorded.modes).toEqual(['dedicated', 'pending', 'dedicated']);
    expect(recorded.gridMounts).toBe(0);
    // No size claim reaches a grid tile: every visible claim after the add is a dedicated terminal's.
    const claims = await visibleClaimsSince(page, claimsBefore);
    expect(claims.filter((claim) => !/terminals\/(terminal-new|second)\/attach/.test(claim.url))).toEqual([]);
  });
}
