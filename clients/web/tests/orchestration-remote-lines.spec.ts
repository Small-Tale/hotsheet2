import { expect, type Page, type Route, test } from '@playwright/test';

import { proxyFetch, realTicketServer } from './real-ticket-server';

/**
 * HS2-SHBSQF: a remote project's tab on a machine that orchestrates it shows this machine's orchestration lines
 * and, for the tickets this machine does not plan, the lines another owner machine pushed to the remote server
 * (HS2-KB2Y51). The fixture's real server plays both machines' ticket stores; the remote tab is the same project
 * opened under a remote composite id, and both halves of the target-lines read are answered here, the long polls
 * held open on a gate until the test releases a change. Synthetic tickets and ids only.
 */
type Server = Awaited<ReturnType<typeof realTicketServer>>;
const BINDING = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const OTHER_OWNER = '01ARZ3NDEKTSV4RRFFQ69G5FB0';

interface Ticket {
  id: string;
  slug: string;
  connection_id: string;
}

const line = (ticket: Ticket, owner: string, goal: string, state: string, observedFrom?: string) => ({
  owner_checkout_id: owner,
  owner_label: owner,
  proposed: false,
  ...(observedFrom ? { observed_from: observedFrom } : {}),
  item: {
    step_id: `step-${owner}-${ticket.slug}`,
    goal: { connection_id: 'goals', native_id: `goal-${goal}` },
    goal_slug: goal,
    target: { target_id: '01ARZ3NDEKTSV4RRFFQ69G5FJ8', connection_id: ticket.connection_id, native_id: ticket.id },
    slug: ticket.slug,
    project_label: 'API',
    location: observedFrom ? 'local' : 'remote',
    ...(observedFrom ? {} : { machine_label: 'Build host' }),
    state,
    level: 'confirmed',
  },
});

/** One half of the read: answers in order; a wait on the current version is held until `release`. */
function half(name: string) {
  const answers: Record<string, unknown>[] = [];
  const asked: string[] = [];
  const held: Array<() => void> = [];
  return {
    asked,
    push(answer: Record<string, unknown>) {
      answers.push(answer);
      for (const release of held.splice(0)) release();
    },
    async serve(route: Route) {
      const after = new URL(route.request().url()).searchParams.get('after');
      asked.push(`${name} ${after ?? '-'}`);
      const current = answers.at(-1)!;
      if (after === null || after !== current.version) return route.fulfill({ json: current });
      // A genuine long poll: open until the half changes or the page goes away.
      await new Promise<void>((release) => held.push(release));
      await route.fulfill({ json: answers.at(-1)! }).catch(() => undefined);
    },
  };
}

async function openBothTabs(page: Page, server: Server) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  const remoteId = `remote:${BINDING}:${server.checkoutId}`;
  const remotePath = `/__hotsheet/project-api/${encodeURIComponent(remoteId)}`;
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  // Adding a second project picks its folder first.
  await page.route(/\/__hotsheet\/folders\/choose$/, (route) =>
    route.fulfill({ json: { path: `${server.root}-remote` } }),
  );
  let opened = 0;
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) => {
    opened += 1;
    return route.fulfill({
      status: 201,
      json:
        opened === 1
          ? { id: server.checkoutId, root: server.root, name: 'Launch', stores: [server.store], apiPath }
          : { id: remoteId, root: remoteId, name: 'Build host · API', stores: [], apiPath: remotePath },
    });
  });
  // Both tabs reach the fixture server; the remote tab's checkout-scoped paths name the native checkout,
  // as the bridge rewrites them for a real remote tab.
  for (const [prefix, encoded] of [
    [apiPath, undefined],
    [remotePath, encodeURIComponent(remoteId)],
  ] as const)
    await page.route(`**${prefix}/**`, async (route) => {
      const incoming = new URL(route.request().url());
      let path = incoming.pathname.slice(prefix.length);
      if (encoded) path = path.replace(`/checkouts/${encoded}`, `/checkouts/${server.checkoutId}`);
      try {
        const response = await proxyFetch(route, {
          url: `${server.url}${path}${incoming.search}`,
          headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
          timeout: 60_000,
        });
        await route.fulfill({ response });
      } catch {
        await route.abort().catch(() => undefined);
      }
    });
  return { apiPath, remoteId, remotePath };
}

/** Opens the local project, then the remote tab the same way a second project is added. */
/** Rails and sheets slide; capture once every finite animation has finished. */
async function capture(page: Page, name: string) {
  await page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity),
  );
  await page.screenshot({ path: `target/visual-captures/hs2-shbsqf-${name}.png` });
}

async function openProjects(page: Page) {
  await page.getByRole('button', { name: 'Open Project', exact: true }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await expect(page.getByRole('tab', { name: 'Launch' })).toBeVisible();
  await page.getByRole('button', { name: 'Add Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
}

test('HS2-SHBSQF a remote tab shows this machine’s lines and another owner’s pushed lines', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const mine = await server.request<Ticket>('/tickets', 'POST', { title: 'Synthetic work this machine plans' });
    const theirs = await server.request<Ticket>('/tickets', 'POST', { title: 'Synthetic work another owner plans' });
    const unplanned = await server.request<Ticket>('/tickets', 'POST', { title: 'Synthetic unplanned work' });
    const here = half('here'),
      there = half('there');
    here.push({ version: 'h1', orchestrated: true, lines: [line(mine, 'launch', 'LCH-1', 'ready')] });
    // The remote server also holds this machine's own pushed copy of the first ticket; this machine's wins.
    there.push({
      version: 't1',
      orchestrated: true,
      lines: [
        line(mine, 'launch', 'LCH-1', 'waits_for_decision', '01ARZ3NDEKTSV4RRFFQ69G5FA0'),
        line(theirs, 'atlas', 'ATL-7', 'ready', OTHER_OWNER),
      ],
    });
    const { apiPath, remotePath } = await openBothTabs(page, server);
    await page.route(
      new RegExp(`${apiPath}/remotes/${BINDING}/checkouts/${server.checkoutId}/orchestration/target-lines`),
      (route) => here.serve(route),
    );
    await page.route(
      new RegExp(`${remotePath.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}/checkouts/[^/]+/orchestration/target-lines`),
      (route) => there.serve(route),
    );
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await openProjects(page);
    const tab = page.locator('[data-component="project-tab-bar"] [data-tab-kind="project"][data-location="remote"]');
    await expect(tab).toHaveCount(1);
    await page.locator('[data-segment-value="list"]').first().click();
    const row = (slug: string) => page.locator(`[data-ticket-slug="${slug}"]`).first();
    const part = (slug: string, name: string) =>
      row(slug).locator(`[data-row-orchestration="true"] [data-orchestration-part="${name}"]`);
    await expect(part(mine.slug, 'state')).toHaveText('Ready', { timeout: 30_000 });
    await expect(part(mine.slug, 'goal')).toHaveText('LCH-1');
    await expect(part(mine.slug, 'placement')).toHaveText('Build host');
    await expect(part(theirs.slug, 'goal')).toHaveText('ATL-7');
    await expect(part(theirs.slug, 'goal')).toHaveAttribute('title', 'Goal ATL-7: In atlas');
    await expect(part(theirs.slug, 'placement')).toHaveText('This machine');
    await expect(row(unplanned.slug).locator('[data-row-orchestration="true"]')).toHaveCount(0);
    // One combined long poll: each half waits on its own cursor.
    await expect
      .poll(() => [...here.asked, ...there.asked].filter((ask) => !ask.endsWith(' -')).sort())
      .toEqual(['here h1', 'there t1']);
    await capture(page, 'remote-list-1440');

    // The other owner's plan moves on the remote server: the open List follows through the long poll, and only
    // the remote half is asked again.
    there.push({
      version: 't2',
      orchestrated: true,
      lines: [line(theirs, 'atlas', 'ATL-7', 'waits_for_permission', OTHER_OWNER)],
    });
    await expect(part(theirs.slug, 'state')).toHaveText('Waits for permission', { timeout: 30_000 });
    await expect(part(mine.slug, 'state')).toHaveText('Ready');
    await expect.poll(() => there.asked.at(-1)).toBe('there t2');
    await expect.poll(() => here.asked.at(-1)).toBe('here h1');
    // This machine's half changes next; the remote half keeps its cursor.
    here.push({ version: 'h2', orchestrated: true, lines: [line(mine, 'launch', 'LCH-1', 'running')] });
    await expect(part(mine.slug, 'state')).toHaveCount(0, { timeout: 30_000 });
    await expect.poll(() => here.asked.at(-1)).toBe('here h2');
    expect(there.asked.at(-1)).toBe('there t2');
    // No request beyond one open wait per half and the reads that answered a change.
    expect(here.asked.length).toBeLessThanOrEqual(5);
    expect(there.asked.length).toBeLessThanOrEqual(5);

    for (const width of [1100, 390, 1440]) {
      await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
      await expect(part(theirs.slug, 'goal')).toBeVisible();
      await capture(page, `remote-list-${width}-after`);
    }
  } finally {
    await server.stop(page);
  }
});
