import { expect, type Page, test } from '@playwright/test';

import { realTicketServer } from './real-ticket-server';

/**
 * HS2-J89JC7: the Orchestration view through the real shell against a real server. The project is
 * its own orchestration target ("Self"), so the plan, the run and the inspector's run controls are
 * real records. Synthetic tickets only.
 */
type Server = Awaited<ReturnType<typeof realTicketServer>>;
const TARGET = '01ARZ3NDEKTSV4RRFFQ69G5FJ8';
const scheduler = { role: 'ai', id: 'scheduler-1' };

async function send(server: Server, method: string, path: string, body?: unknown) {
  const response = await fetch(`${server.url}${path}`, {
    method,
    headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  if (!response.ok) throw new Error(`${method} ${path}: ${response.status} ${text}`);
  return JSON.parse(text) as Record<string, unknown>;
}

/** HS2-W27N1D: the Orchestration view is offered for a project whose Project Type is Orchestration. */
async function asOrchestrationProject(server: Server) {
  await send(server, 'PUT', `/checkouts/${server.checkoutId}/orchestration/project-type`, {
    project_type: 'orchestration',
    actor: { role: 'human' },
  });
}

/** Declare this project as its own target, bind it, and remember the scheduler's dispatch rule (HS2-XHQ1EH). */
async function declareSelf(server: Server) {
  const base = `/checkouts/${server.checkoutId}/orchestration`;
  const added = await send(server, 'POST', `${base}/targets`, {
    target_id: TARGET,
    label: 'Self',
    expected_revision: null,
  });
  const candidate = await send(server, 'POST', `${base}/inspect`, {
    destination: { kind: 'local', checkout_id: server.checkoutId },
  });
  await send(server, 'PUT', `${base}/targets/${TARGET}/binding`, {
    target: { owner_checkout_id: server.checkoutId, target_id: TARGET },
    expected_manifest_revision: added.revision,
    expected_binding_revision: 0,
    previous_destination: null,
    destination: candidate.destination,
  });
  await send(server, 'POST', `${base}/permission-rules`, {
    target_id: TARGET,
    action: 'work_start',
    actor: { role: 'human', id: 'maintainer' },
  });
}

async function planAndDispatch(server: Server) {
  const base = `/checkouts/${server.checkoutId}/orchestration`;
  const goal = await server.request<{ id: string; slug: string }>('/tickets', 'POST', {
    title: 'Ship the synthetic sign-on',
  });
  const first = await server.request<{ id: string; slug: string; connection_id: string }>('/tickets', 'POST', {
    title: 'Add the synthetic token endpoint',
  });
  const second = await server.request<{ id: string; slug: string }>('/tickets', 'POST', {
    title: 'Add the synthetic login page',
  });
  const define = (native: string, prerequisites: object[]) =>
    send(server, 'POST', `${base}/readiness/define`, {
      ticket: { native_id: goal.id },
      target_id: TARGET,
      connection_id: first.connection_id,
      native_id: native,
      prerequisites,
      actor: scheduler,
    });
  await define(first.id, []);
  await define(second.id, [
    { kind: 'ticket', target_id: TARGET, connection_id: first.connection_id, native_id: first.id, gates: 'none' },
  ]);
  return {
    goal,
    first,
    second,
    dispatch: () =>
      send(server, 'POST', `${base}/runs`, {
        ticket: { native_id: goal.id },
        target_id: TARGET,
        connection_id: first.connection_id,
        native_id: first.id,
        operation_id: 'op-view',
        worker_id: 'browser-worker',
        actor: scheduler,
      }),
  };
}

async function openProject(page: Page, server: Server, name: string) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name, stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url());
    try {
      const response = await route.fetch({
        url: `${server.url}${incoming.pathname.slice(apiPath.length)}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      await route.fulfill({ response });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
}

const columnTitles = (page: Page) =>
  page
    .locator('[data-orchestration-view="true"] [data-component="ticket-board-column"]')
    .evaluateAll((columns) => columns.map((column) => column.getAttribute('aria-label') ?? ''));

/** How much of a column shows inside the board's horizontal scroller, from 0 to 1. */
const visibleShare = (page: Page, label: string) =>
  page.locator(`[data-orchestration-view="true"] [aria-label="${label}"]`).evaluate((column) => {
    let track = column.parentElement;
    while (track && !/auto|scroll/.test(getComputedStyle(track).overflowX)) track = track.parentElement;
    const box = column.getBoundingClientRect(),
      view = track!.getBoundingClientRect();
    return Math.max(0, Math.min(box.right, view.right) - Math.max(box.left, view.left)) / box.width;
  });

const capture = async (page: Page, name: string) => {
  // Rails and sheets slide; capture once every finite animation has finished.
  await page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity),
  );
  await page.screenshot({ path: test.info().outputPath(`hs2-j89jc7-${name}.png`) });
};

test('HS2-J89JC7 a project that is not an orchestration project has no Orchestration view', async ({ page }) => {
  const server = await realTicketServer();
  try {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openProject(page, server, 'Plain Project');
    await expect(page.locator('[data-segment-value="workers"]').first()).toBeVisible();
    await expect(page.locator('[data-segment-value="orchestration"]')).toHaveCount(0);
  } finally {
    await server.stop();
  }
});

test('HS2-J89JC7 an orchestration project with no projects added shows the first-run empty state', async ({ page }) => {
  const server = await realTicketServer();
  try {
    await send(server, 'PUT', `/checkouts/${server.checkoutId}/orchestration/targets`, {
      expected_revision: null,
      targets: [],
    });
    await asOrchestrationProject(server);
    await page.setViewportSize({ width: 1440, height: 900 });
    await openProject(page, server, 'Launch');
    const tab = page.locator('[data-segment-value="orchestration"]').first();
    await expect(tab).toBeVisible();
    // After Workers, before Notifications.
    const order = await page
      .locator('[data-component="segmented-control"]')
      .first()
      .locator('[data-segment-value]')
      .evaluateAll((items) => items.map((item) => item.getAttribute('data-segment-value')));
    expect(order.indexOf('orchestration')).toBe(order.indexOf('workers') + 1);
    await tab.click();
    const empty = page.locator('[data-orchestration-empty="no-projects"]');
    await expect(empty).toContainText('No Projects to Orchestrate Yet');
    await expect(page.locator('[data-orchestration-toolbar]')).toHaveCount(0);
    await capture(page, 'empty-1440');
    await empty.getByRole('button', { name: 'Choose Projects…' }).click();
    await expect(page.locator('[data-segment-value="settings"][aria-pressed="true"]').first()).toBeVisible();
  } finally {
    await server.stop();
  }
});

test('HS2-J89JC7 the timeline reads newest to oldest, opens on current work, and drives the inspector', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await declareSelf(server);
    await asOrchestrationProject(server);
    const plan = await planAndDispatch(server);
    await page.setViewportSize({ width: 1440, height: 900 });
    await openProject(page, server, 'Launch');
    await page.locator('[data-segment-value="orchestration"]').first().click();

    // Before kick-off: the proposed plan, First on the right, labeled as a dry run.
    const view = page.locator('[data-orchestration-view="true"]');
    await expect(view).toHaveAttribute('data-phase', 'proposed');
    await expect(page.locator('[data-orchestration-toolbar="proposed"]')).toContainText('This Plan Is a Dry Run');
    expect(await columnTitles(page)).toEqual(['Second · About +1 h Column', 'First · On Start Column']);
    await expect(view.locator('[data-orchestration-part="state"]').first()).toBeVisible();
    const start = page.getByRole('button', { name: 'Start Orchestration' });
    await expect(start).toBeDisabled();
    await expect(start).toHaveAttribute('title', /Orchestration settings first/);
    await capture(page, 'plan-1440');

    // A dispatch reaches the open view through the long poll: Working Now and Next.
    await plan.dispatch();
    await expect(view).toHaveAttribute('data-phase', 'paused', { timeout: 30_000 });
    await expect
      .poll(() => columnTitles(page))
      .toEqual([
        expect.stringMatching(/^Later/),
        expect.stringMatching(/^Then/),
        expect.stringMatching(/^Next/),
        'Working Now Column',
        'Done Column',
      ]);
    // It opens scrolled to its right end, with Working Now in view.
    await page.locator('[data-segment-value="list"]').first().click();
    await page.locator('[data-segment-value="orchestration"]').first().click();
    await expect.poll(() => visibleShare(page, 'Working Now Column')).toBeGreaterThan(0.95);
    await expect.poll(() => visibleShare(page, 'Done Column')).toBeGreaterThan(0.95);

    // Cards: grouped by project, with the orchestration line.
    const working = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    await expect(working).toContainText('Add the synthetic token endpoint');
    await expect(working.locator('[data-orchestration-part="placement"]')).toHaveText('This machine');
    await expect(working.locator('[data-orchestration-part="goal"]')).toHaveText(plan.goal.slug);
    const waiting = view.locator(`[data-ticket-slug="${plan.second.slug}"]`);
    await expect(waiting.locator('[data-orchestration-part="state"]')).toHaveText(`Waits on ${plan.first.slug}`);
    await expect(view.locator('[data-ticket-group]').first()).toContainText('Self');

    // The inspector: where it runs, the run and the person's controls.
    await working.click();
    const inspector = page.locator(`[data-orchestration-inspector]`);
    await expect(inspector).toContainText('Where It Runs');
    await expect(inspector).toContainText('Add the synthetic token endpoint');
    await expect(inspector.locator('[data-orchestration-section="Run"]')).toContainText('Dispatched');
    await expect(inspector.locator('[data-orchestration-section="Events"]')).toBeVisible();
    await capture(page, 'running-1440');
    await inspector.locator('[data-control="pause"]').click();
    await expect(inspector.locator('[data-control="resume"]')).toBeVisible({ timeout: 30_000 });
    await expect(inspector.locator('[data-orchestration-section="Run"]')).toContainText('paused');
    await inspector.locator('[data-control="resume"]').click();
    await expect(inspector.locator('[data-control="pause"]')).toBeVisible({ timeout: 30_000 });

    // Selecting the waiting card shows what it waits on.
    await waiting.click();
    await expect(inspector.locator('[data-orchestration-section="Prerequisites"]')).toContainText(plan.first.slug);

    // Medium, then a phone: paged, opening on Working Now; then wide again.
    await page.setViewportSize({ width: 1100, height: 820 });
    await expect.poll(() => visibleShare(page, 'Working Now Column')).toBeGreaterThan(0.95);
    await capture(page, 'running-1100');
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(view).toHaveAttribute('data-layout', 'paged');
    await expect.poll(() => visibleShare(page, 'Working Now Column')).toBeGreaterThan(0.85);
    await capture(page, 'running-390');
    await view.locator(`[data-ticket-slug="${plan.first.slug}"]`).click();
    await expect(inspector.locator('[data-orchestration-section="Run"]')).toBeVisible();
    await capture(page, 'running-inspector-390');
    await page.setViewportSize({ width: 1440, height: 900 });
    await expect(view).toHaveAttribute('data-layout', 'grid');
    await expect.poll(() => visibleShare(page, 'Working Now Column')).toBeGreaterThan(0.95);
    await expect
      .poll(() => page.locator('[data-orchestration-inspector]').evaluate((node) => node.getBoundingClientRect().left))
      .toBeLessThan(1440);
    await capture(page, 'running-1440-after-narrow');
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture(page, 'running-1440-dark');
    await page.emulateMedia({ colorScheme: 'light' });

    // A project group collapses in this view's own scope.
    const group = view.locator('[data-ticket-group]').first();
    await group.locator('[data-action="toggle-ticket-group"]').first().click();
    await expect(view.locator(`[data-ticket-slug="${plan.first.slug}"]`)).toHaveCount(0);
  } finally {
    await server.stop();
  }
});
