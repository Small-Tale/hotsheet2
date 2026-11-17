import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { expect, type Page, test } from '@playwright/test';

import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer } from './real-ticket-server';

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
async function declareSelf(server: Server, rememberDispatch = true) {
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
  if (rememberDispatch)
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
      const response = await proxyFetch(route, {
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
  await openMockedProject(page);
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

const capture = async (page: Page, name: string, ticket = 'hs2-j89jc7') => {
  // Rails and sheets slide; capture once every finite animation has finished.
  await page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity),
  );
  await page.screenshot({ path: test.info().outputPath(`${ticket}-${name}.png`) });
};

test('HS2-J89JC7 a project that is not an orchestration project has no Orchestration view', async ({ page }) => {
  const server = await realTicketServer();
  try {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openProject(page, server, 'Plain Project');
    await expect(page.locator('[data-segment-value="workers"]').first()).toBeVisible();
    await expect(page.locator('[data-segment-value="orchestration"]')).toHaveCount(0);
  } finally {
    await server.stop(page);
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
    await server.stop(page);
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
    await server.stop(page);
  }
});

/**
 * HS2-DBQSWB: the scheduler's dispatch of a step waits on a person. The run route refuses the AI
 * actor with a real orchestrator permission request, and the pass's decision names it, exactly as
 * the scheduler logs it (`permission_required` with the request id), so the timeline links the card.
 */
async function waitOnPermission(server: Server, plan: Awaited<ReturnType<typeof planAndDispatch>>) {
  const base = `/checkouts/${server.checkoutId}/orchestration`;
  const timeline = await send(server, 'GET', `${base}/timeline`);
  const items = (timeline.waves as Array<{ items: Array<{ step_id: string; target: { native_id: string } }> }>)
    .flatMap((wave) => wave.items)
    .filter((item) => item.target.native_id === plan.first.id);
  const step = items[0].step_id,
    operation = `sched-run-${step}`;
  const response = await fetch(`${server.url}${base}/runs`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
    body: JSON.stringify({
      ticket: { native_id: plan.goal.id },
      target_id: TARGET,
      connection_id: plan.first.connection_id,
      native_id: plan.first.id,
      operation_id: operation,
      worker_id: 'browser-worker',
      actor: scheduler,
    }),
  });
  const refused = (await response.json()) as { code?: string; request_id?: string };
  expect(response.status).toBe(403);
  expect(refused.code).toBe('permission_required');
  const decisions = resolve(server.home, 'orchestration-scheduler-decisions');
  await mkdir(decisions, { recursive: true });
  const record = {
    seq: 1,
    at_ms: Date.now(),
    tick_id: '01ARZ3NDEKTSV4RRFFQ69G5FB0',
    mode: 'applied',
    trigger: 'event',
    kind: 'start',
    actor_id: scheduler.id,
    target_id: TARGET,
    step_id: step,
    outcome: 'permission_required',
    outcome_detail: refused.request_id,
    operation_id: operation,
  };
  await writeFile(resolve(decisions, `${server.checkoutId}.jsonl`), `${JSON.stringify(record)}\n`);
  return refused.request_id!;
}

test('HS2-DBQSWB Show Request opens that exact orchestrator request in Pending and focuses it', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await declareSelf(server, false);
    await asOrchestrationProject(server);
    const plan = await planAndDispatch(server);
    // Another pending request of the same project, so the link has to pick the right card.
    const other = await fetch(`${server.url}/checkouts/${server.checkoutId}/orchestration/runs`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
      body: JSON.stringify({
        ticket: { native_id: plan.goal.id },
        target_id: TARGET,
        connection_id: plan.first.connection_id,
        native_id: plan.second.id,
        operation_id: 'op-other',
        worker_id: 'browser-worker',
        actor: scheduler,
      }),
    });
    expect(other.status).toBe(403);
    const requestId = await waitOnPermission(server, plan);
    for (const width of [1440, 1100, 390]) {
      await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
      if (width === 1440) await openProject(page, server, 'Launch');
      // The popup asks first, as for any request; Ignore leaves both in Pending.
      const popup = page.locator('[data-component="permission-request-popup"]');
      if (width === 1440) await expect(popup).toContainText('Orchestrator: Start work on a ticket in Self');
      while (await popup.isVisible()) await popup.getByRole('button', { name: 'Ignore' }).click();
      await page.locator('[data-segment-value="orchestration"]').first().click();
      const view = page.locator('[data-orchestration-view="true"]');
      const card = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
      await expect(card.locator('[data-orchestration-part="state"]')).toHaveText('Waits for permission', {
        timeout: 30_000,
      });
      await card.click();
      const inspector = page.locator('[data-orchestration-inspector]');
      const show = inspector.getByRole('button', { name: 'Show Request' });
      await expect(show).toHaveAttribute('data-request-id', requestId);
      await capture(page, `inspector-${width}`, 'hs2-dbqswb');
      await show.click();
      await expect(page.locator('[data-segment-value="notifications"][aria-pressed="true"]').first()).toBeVisible();
      const cards = page.locator('[data-component="permission-request-card"][data-presentation="list"]');
      await expect(cards).toHaveCount(2);
      const targeted = page.locator('[data-component="permission-request-card"][data-targeted="true"]');
      await expect(targeted).toHaveCount(1);
      await expect(targeted).toBeFocused();
      // The whole card and its focus ring show, not cut by the list's scroll edge.
      await expect(targeted).toBeInViewport({ ratio: 1 });
      // Nothing covers it: on a phone the inspector sheet that held Show Request closes.
      const box = (await targeted.boundingBox())!;
      const hit = await page.evaluate(
        ([x, y]) => document.elementFromPoint(x, y)?.closest('[data-targeted="true"]') !== null,
        [box.x + box.width / 2, box.y + Math.min(box.height / 2, 40)],
      );
      expect(hit).toBe(true);
      // The focused card is the one this step waits on, not the other request.
      const pending = (await send(server, 'GET', '/permissions')) as unknown as Array<{
        id: number;
        orchestrator?: { request_id: string };
      }>;
      const bridged = pending.find((item) => item.orchestrator?.request_id === requestId);
      await expect(targeted).toHaveAttribute('data-request-key', `${server.checkoutId}:${bridged!.id}`);
      await expect(targeted).toContainText('Orchestrator:');
      const outline = await targeted.evaluate((node) => getComputedStyle(node).outlineStyle);
      expect(outline).not.toBe('none');
      // It is never a Tab stop: Tab moves on into the card's own controls.
      await page.keyboard.press('Tab');
      await expect(targeted).not.toBeFocused();
      await targeted.focus();
      await capture(page, `pending-${width}`, 'hs2-dbqswb');
    }
    // Narrow back to wide: the focused card stays the same one, in view.
    await page.setViewportSize({ width: 1440, height: 900 });
    await expect(page.locator('[data-component="permission-request-card"][data-targeted="true"]')).toBeInViewport();
    await capture(page, 'pending-1440-after-narrow', 'hs2-dbqswb');
  } finally {
    await server.stop(page);
  }
});

test('HS2-T4V1TX arrow keys move between cards within and across columns and select their steps', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await declareSelf(server);
    await asOrchestrationProject(server);
    const plan = await planAndDispatch(server);
    // A third step with nothing before it shares First with the first one.
    const third = await server.request<{ id: string; slug: string }>('/tickets', 'POST', {
      title: 'Add the synthetic audit log',
    });
    await send(server, 'POST', `/checkouts/${server.checkoutId}/orchestration/readiness/define`, {
      ticket: { native_id: plan.goal.id },
      target_id: TARGET,
      connection_id: plan.first.connection_id,
      native_id: third.id,
      prerequisites: [],
      actor: scheduler,
    });
    const titles: Record<string, string> = {
      [plan.first.slug]: 'Add the synthetic token endpoint',
      [plan.second.slug]: 'Add the synthetic login page',
      [third.slug]: 'Add the synthetic audit log',
    };
    await page.setViewportSize({ width: 1440, height: 900 });
    await openProject(page, server, 'Launch');
    await page.locator('[data-segment-value="orchestration"]').first().click();
    const view = page.locator('[data-orchestration-view="true"]');
    await expect(view).toHaveAttribute('data-phase', 'proposed');
    const inspector = page.locator('[data-orchestration-inspector]');
    const column = (label: string) => view.locator(`[aria-label="${label}"] [data-ticket-slug]`);
    const firstColumn = await column('First · On Start Column').evaluateAll((cards) =>
      cards.map((card) => card.getAttribute('data-ticket-slug') ?? ''),
    );
    expect(firstColumn).toHaveLength(2);
    const [top, bottom] = firstColumn as [string, string];
    const card = (slug: string) => view.locator(`[data-ticket-slug="${slug}"]`);
    const expectAt = async (slug: string) => {
      await expect(card(slug)).toBeFocused();
      await expect(card(slug)).toHaveAttribute('aria-selected', 'true');
      await expect(inspector).toContainText(titles[slug]);
    };

    for (const width of [1440, 1100]) {
      await page.setViewportSize({ width, height: 900 });
      await card(top).click();
      await expectAt(top);
      await page.keyboard.press('ArrowDown');
      await expectAt(bottom);
      await page.keyboard.press('ArrowDown');
      await expectAt(bottom);
      await page.keyboard.press('ArrowUp');
      await expectAt(top);
      // Left reaches Second (newer work is on the left); its only card.
      await page.keyboard.press('ArrowLeft');
      await expectAt(plan.second.slug);
      await expect(card(plan.second.slug)).toBeInViewport();
      await page.keyboard.press('ArrowLeft');
      await expectAt(plan.second.slug);
      await page.keyboard.press('ArrowRight');
      await expectAt(top);
      await page.keyboard.press('ArrowDown');
      await page.keyboard.press('ArrowLeft');
      await expectAt(plan.second.slug);
      // HS2-3JH0QV: the board fills the view below the page header, so no column scrolls its few
      // cards inside a short box (WebKit sized the auto row short of its content).
      const fit = await view.evaluate((root) => {
        const body = root.querySelector<HTMLElement>('.orchestration-timeline-view__body')!;
        return {
          gap: root.getBoundingClientRect().bottom - body.getBoundingClientRect().bottom,
          overflowing: [...root.querySelectorAll<HTMLElement>('.ticket-board-column__tickets')].filter(
            (tickets) => tickets.scrollHeight > tickets.clientHeight + 1,
          ).length,
        };
      });
      expect(fit).toEqual({ gap: 0, overflowing: 0 });
      await capture(page, `arrows-${width}`, 'hs2-t4v1tx');
    }

    // A phone pages one column at a time: Left and Right bring the card's column into view.
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(view).toHaveAttribute('data-layout', 'paged');
    await card(top).focus({ timeout: 10_000 });
    await page.keyboard.press('ArrowLeft');
    await expect(card(plan.second.slug)).toBeFocused();
    await expect.poll(() => visibleShare(page, 'Second · About +1 h Column')).toBeGreaterThan(0.85);
    await capture(page, 'arrows-390', 'hs2-t4v1tx');
    await page.keyboard.press('ArrowRight');
    await expect(card(top)).toBeFocused();
    await expect.poll(() => visibleShare(page, 'First · On Start Column')).toBeGreaterThan(0.85);

    // Wide again: the focused card stays focused and in view.
    await page.setViewportSize({ width: 1440, height: 900 });
    await expect(view).toHaveAttribute('data-layout', 'grid');
    await card(top).focus({ timeout: 10_000 });
    await page.keyboard.press('ArrowLeft');
    await expectAt(plan.second.slug);
    await expect(card(plan.second.slug)).toBeInViewport();
    await capture(page, 'arrows-1440-after-narrow', 'hs2-t4v1tx');

    // A collapsed project group draws no cards in any column; expanding it again restores the walk.
    const group = view.locator('[data-ticket-group]').first();
    await group.locator('[data-action="toggle-ticket-group"]').first().click({ timeout: 10_000 });
    await expect(view.locator('[data-ticket-slug]')).toHaveCount(0);
    await view.locator('[data-ticket-group]').first().locator('[data-action="toggle-ticket-group"]').first().click({
      timeout: 10_000,
    });
    await card(top).click({ timeout: 10_000 });
    await expectAt(top);
    await page.keyboard.press('ArrowLeft');
    await expectAt(plan.second.slug);
  } finally {
    await server.stop(page);
  }
});

test('HS2-S3FKJ7 a target project’s List and Columns show the orchestration line of planned tickets', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    // The project is its own target ("Self"), so its List shows the tickets the plan works.
    await declareSelf(server);
    await asOrchestrationProject(server);
    const plan = await planAndDispatch(server);
    const unplanned = await server.request<{ slug: string }>('/tickets', 'POST', { title: 'Unplanned synthetic work' });
    await page.setViewportSize({ width: 1440, height: 900 });
    await openProject(page, server, 'Launch');
    await page.locator('[data-segment-value="list"]').first().click();
    const row = (slug: string) => page.locator(`[data-ticket-slug="${slug}"]`).first();
    const line = (slug: string) => row(slug).locator('[data-row-orchestration="true"]');
    // A proposed plan: the first step is ready, the second comes after it, both from the same goal.
    await expect(line(plan.first.slug).locator('[data-orchestration-part="state"]')).toHaveText('Ready', {
      timeout: 30_000,
    });
    await expect(line(plan.first.slug).locator('[data-orchestration-part="placement"]')).toHaveText('This machine');
    await expect(line(plan.first.slug).locator('[data-orchestration-part="goal"]')).toHaveText(plan.goal.slug);
    await expect(line(plan.first.slug).locator('[data-orchestration-part="goal"]')).toHaveAttribute(
      'title',
      // The orchestration project's name as this machine registered it (the fixture's folder).
      `Goal ${plan.goal.slug}: Ship the synthetic sign-on, in project`,
    );
    await expect(line(plan.second.slug).locator('[data-orchestration-part="state"]')).toHaveText(
      `After ${plan.first.slug}`,
    );
    // Tickets no plan works, the goal ticket included, have no line.
    await expect(line(unplanned.slug)).toHaveCount(0);
    await expect(line(plan.goal.slug)).toHaveCount(0);
    await capture(page, 'list-1440', 'hs2-s3fkj7');

    // A dispatch reaches the open List through the long poll: the line now waits on nothing and the
    // second step waits on the running one.
    await plan.dispatch();
    await expect(line(plan.second.slug).locator('[data-orchestration-part="state"]')).toHaveText(
      `Waits on ${plan.first.slug}`,
      { timeout: 30_000 },
    );
    await expect(line(plan.first.slug).locator('[data-orchestration-part="state"]')).toHaveCount(0);
    await expect(line(plan.first.slug).locator('[data-orchestration-part="placement"]')).toHaveText('This machine');

    for (const width of [1100, 390]) {
      await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
      await expect(line(plan.second.slug)).toBeVisible();
      await capture(page, `list-${width}`, 'hs2-s3fkj7');
    }
    // Columns, narrow then wide again.
    await page.locator('[data-segment-value="board"]').first().click();
    await expect(line(plan.second.slug).locator('[data-orchestration-part="state"]')).toHaveText(
      `Waits on ${plan.first.slug}`,
    );
    await capture(page, 'columns-390', 'hs2-s3fkj7');
    for (const width of [1100, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      await expect(line(plan.second.slug)).toBeVisible();
      await capture(page, `columns-${width}`, 'hs2-s3fkj7');
    }
    await page.locator('[data-segment-value="list"]').first().click();
    await expect(line(plan.second.slug)).toBeVisible();
    await capture(page, 'list-1440-after-narrow', 'hs2-s3fkj7');
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture(page, 'list-1440-dark', 'hs2-s3fkj7');
  } finally {
    await server.stop(page);
  }
});

test('HS2-HDHT5E remote target tickets show their titles, read once through the gate while the view is open', async ({
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
    // The real timeline, with both target tickets placed on a paired server: the read on this machine has
    // only their slugs, never their titles (HS2-J89JC7).
    const remote = new Set([plan.first.id, plan.second.id]);
    await page.route(/\/orchestration\/timeline(\?|$)/, async (route) => {
      const apiPath = `/__hotsheet/project-api/${server.checkoutId}`,
        incoming = new URL(route.request().url());
      try {
        const response = await proxyFetch(route, {
          url: `${server.url}${incoming.pathname.slice(apiPath.length)}${incoming.search}`,
          headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
          timeout: 60_000,
        });
        const body = (await response.json()) as {
          waves: Array<{ items: Array<Record<string, unknown> & { target: { native_id: string } }> }>;
        };
        for (const wave of body.waves)
          for (const item of wave.items)
            if (remote.has(item.target.native_id)) {
              item.location = 'remote';
              item.machine_label = 'Build host';
              delete item.title;
            }
        await route.fulfill({ response, json: body });
      } catch {
        // A long poll still open when the page or the server goes away.
        await route.abort().catch(() => undefined);
      }
    });
    // The label read goes to the real gated route; the second ticket's target refuses it.
    const reads: string[] = [];
    await page.route(/\/orchestration\/tickets\/read$/, async (route) => {
      const native = (route.request().postDataJSON() as { native_id: string }).native_id;
      reads.push(native);
      if (native === plan.second.id)
        return route.fulfill({
          status: 404,
          json: { error: 'target ticket not found', code: 'target_ticket_not_found' },
        });
      return route.fallback();
    });
    await page.locator('[data-segment-value="orchestration"]').first().click();
    const view = page.locator('[data-orchestration-view="true"]');
    const first = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    await expect(first).toContainText('Add the synthetic token endpoint', { timeout: 30_000 });
    await expect(first.locator('[data-orchestration-part="placement"]')).toHaveText('Build host');
    // A refused read keeps the slug and the fallback title.
    const second = view.locator(`[data-ticket-slug="${plan.second.slug}"]`);
    await expect(second).toContainText(`${plan.second.slug} in Self`);
    await capture(page, 'remote-1440', 'hs2-hdht5e');
    // The inspector shows the read title too.
    await first.click();
    await expect(page.locator('[data-orchestration-inspector]')).toContainText('Add the synthetic token endpoint');
    // A new timeline version reads nothing again: each ticket was asked once.
    await plan.dispatch();
    await expect(view).toHaveAttribute('data-phase', 'paused', { timeout: 30_000 });
    await expect(first).toContainText('Add the synthetic token endpoint');
    expect(reads.sort()).toEqual([plan.first.id, plan.second.id].sort());
    for (const width of [1100, 390]) {
      await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
      await expect(view).toHaveAttribute('data-layout', width === 390 ? 'paged' : 'grid');
      await capture(page, `remote-${width}`, 'hs2-hdht5e');
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await expect(first).toContainText('Add the synthetic token endpoint');
    await capture(page, 'remote-1440-after-narrow', 'hs2-hdht5e');
    // Nothing the read found was written to this machine's records.
    const timeline = await send(server, 'GET', `/checkouts/${server.checkoutId}/orchestration/timeline`);
    expect(JSON.stringify(timeline)).not.toContain('"location":"remote"');
  } finally {
    await server.stop(page);
  }
});

test('HS2-AMCSKS an orchestration project shows its Orchestration entries on the first paint', async ({
  browser,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await asOrchestrationProject(server);
    // Record each view-mode group and settings category list as first painted, and every later
    // insertion of an Orchestration entry, so a late insertion under the pointer fails the test.
    const firstPaint = () => {
      const state = window as unknown as {
        __firstViewModes?: string[];
        __firstCategories?: string[];
        __late: string[];
      };
      state.__late = [];
      const labels = (root: Element, selector: string) =>
        [...root.querySelectorAll(selector)].map((node) => node.getAttribute('aria-label') ?? node.textContent.trim());
      const containers = '[role="group"][aria-label="View mode"], nav[aria-label="Project Settings"]';
      const painted = new WeakSet<Element>();
      new MutationObserver((records) => {
        // An Orchestration entry added inside a container painted in an earlier batch arrived late.
        for (const record of records)
          for (const node of record.addedNodes) {
            if (!(node instanceof Element)) continue;
            const container = node.closest(containers);
            if (container && painted.has(container) && /Orchestration/.test(node.textContent))
              state.__late.push(node.outerHTML.slice(0, 80));
          }
        const group = document.querySelector('[role="group"][aria-label="View mode"]');
        if (group && !state.__firstViewModes) state.__firstViewModes = labels(group, 'button');
        const nav = document.querySelector('nav[aria-label="Project Settings"]');
        if (nav && !state.__firstCategories) state.__firstCategories = labels(nav, 'button');
        for (const container of document.querySelectorAll(containers)) painted.add(container);
      }).observe(document, { childList: true, subtree: true });
    };
    for (const width of [1440, 1100, 390]) {
      // A fresh browser each time, so the first paint is the project's first paint.
      const context = await browser.newContext({ baseURL, viewport: { width, height: width === 390 ? 844 : 900 } });
      await context.addInitScript(firstPaint);
      const page = await context.newPage();
      await openProject(page, server, 'Launch');
      const group = page.getByRole('group', { name: 'View mode' });
      await expect(group.getByRole('button', { name: 'Orchestration View' })).toBeVisible();
      const first = await page.evaluate(() => (window as unknown as { __firstViewModes?: string[] }).__firstViewModes);
      expect(first).toContain('Orchestration View');
      await capture(page, `toolbar-${width}`, 'hs2-amcsks');
      await group.getByRole('button', { name: 'Settings View' }).click();
      // A phone keeps the settings sidebar closed until asked.
      const showSidebar = page.getByRole('button', { name: 'Show Settings Sidebar' });
      if (await showSidebar.isVisible()) await showSidebar.click();
      await expect(page.getByRole('navigation', { name: 'Project Settings' })).toBeVisible();
      const categories = await page.evaluate(
        () => (window as unknown as { __firstCategories?: string[] }).__firstCategories,
      );
      expect(categories?.some((label) => /Orchestration/.test(label))).toBe(true);
      await capture(page, `settings-${width}`, 'hs2-amcsks');
      expect(await page.evaluate(() => (window as unknown as { __late: string[] }).__late)).toEqual([]);
      if (width === 1440) {
        // Wide, narrow and wide again: nothing is inserted on a resize either.
        await page.setViewportSize({ width: 390, height: 844 });
        await page.setViewportSize({ width: 1440, height: 900 });
        await capture(page, 'settings-1440-after-narrow', 'hs2-amcsks');
        expect(await page.evaluate(() => (window as unknown as { __late: string[] }).__late)).toEqual([]);
      }
      await context.close();
    }
  } finally {
    await server.stop();
  }
});
