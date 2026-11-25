import { mkdir } from 'node:fs/promises';
import { dirname, join } from 'node:path';

import { expect, type Page, test } from '@playwright/test';

import { capture } from './orchestration-real-fixtures';
import { proxyFetch, realTicketServer, routeProjectChangeSocket } from './real-ticket-server';

/**
 * HS2-1DDXNS: the orchestration workflow as a new person meets it, in one flow through the shipped
 * shell against a real isolated server. Launch starts as a Software project beside an open Web
 * project; the person makes Launch an orchestration project, adds Web, sees the dry-run plan, follows
 * a dispatched run through a worker letting go and finishing, saves a policy and starts
 * orchestration. Only the planner's proposal and the worker's own claim actions are made through the
 * server's routes, as the planning AI session and the worker would make them. Synthetic tickets only.
 */
type Server = Awaited<ReturnType<typeof realTicketServer>>;
const scheduler = { role: 'ai', id: 'scheduler-1' };
const WORKER = 'golden-worker';

async function openTwoProjects(page: Page) {
  const server = await realTicketServer();
  const projects = [{ id: server.checkoutId, root: server.root, name: 'Launch' }];
  const root = join(dirname(server.root), 'web');
  await mkdir(root, { recursive: true });
  const web = await server.request<{ id: string }>('/checkouts', 'POST', { root, stores: [server.store] });
  projects.push({ id: web.id, root, name: 'Web' });
  await page.addInitScript(
    (roots) => {
      localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
    },
    projects.map((project) => project.root),
  );
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, async (route) => {
    const { root: opened } = route.request().postDataJSON() as { root: string };
    const project = projects.find((item) => item.root === opened);
    if (!project) return route.fulfill({ status: 404, json: { error: 'unknown project' } });
    return route.fulfill({
      status: 201,
      json: { ...project, stores: [server.store], apiPath: `/__hotsheet/project-api/${project.id}` },
    });
  });
  await page.route('**/__hotsheet/remotes', (route) => route.fulfill({ json: [] }));
  await page.route('**/__hotsheet/project-api/**', async (route) => {
    const incoming = new URL(route.request().url()),
      path = incoming.pathname.replace(/^\/__hotsheet\/project-api\/[^/]+/, '');
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
  await routeProjectChangeSocket(page, server, server.checkoutId);
  return server;
}

/** What the planning AI session does with the default goal: a goal and three steps, each after the one before. */
async function proposePlan(server: Server, base: string, target: string) {
  const goal = await server.request<{ id: string; slug: string }>('/tickets', 'POST', {
    title: 'Ship the synthetic sign-on',
  });
  const steps = [];
  for (const title of ['Add the synthetic token endpoint', 'Add the synthetic login page', 'Document the sign-on'])
    steps.push(
      await server.request<{ id: string; slug: string; connection_id: string }>('/tickets', 'POST', { title }),
    );
  const connection = steps[0].connection_id;
  for (const [index, step] of steps.entries())
    await server.request(`${base}/readiness/define`, 'POST', {
      ticket: { native_id: goal.id },
      target_id: target,
      connection_id: connection,
      native_id: step.id,
      prerequisites:
        index === 0
          ? []
          : [
              {
                kind: 'ticket',
                target_id: target,
                connection_id: connection,
                native_id: steps[index - 1].id,
                gates: 'none',
              },
            ],
      actor: scheduler,
    });
  return { goal, steps };
}

const settle = (page: Page, ticket: string, name: string) => capture(page, ticket, name);

/** Wide, medium, narrow, wide again, then dark at wide and narrow (Visual QA). */
async function widths(page: Page, name: string) {
  for (const [width, height] of [
    [1440, 900],
    [1100, 820],
    [390, 844],
    [1440, 900],
  ] as const) {
    await page.setViewportSize({ width, height });
    await settle(page, 'hs2-1ddxns', `${name}-${width}`);
  }
  await page.emulateMedia({ colorScheme: 'dark' });
  await settle(page, 'hs2-1ddxns', `${name}-1440-dark`);
  await page.setViewportSize({ width: 390, height: 844 });
  await settle(page, 'hs2-1ddxns', `${name}-390-dark`);
  await page.emulateMedia({ colorScheme: 'light' });
  await page.setViewportSize({ width: 1440, height: 900 });
}

test('HS2-1DDXNS a new orchestration project goes from Software to a started plan through the shipped shell', async ({
  page,
}) => {
  test.setTimeout(300_000);
  const server = await openTwoProjects(page);
  const base = `/checkouts/${server.checkoutId}/orchestration`;
  let stopped = false;
  try {
    const viewModes = page.getByRole('group', { name: 'View mode' });
    const settingsNav = page.getByRole('navigation', { name: 'Project Settings' });
    const view = page.locator('[data-orchestration-view="true"]');
    const toolbar = page.locator('[data-orchestration-toolbar]');
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.getByRole('tab', { name: /Launch/ }).click();

    // 1. Discover: a Software project offers no Orchestration view until its Project Type says so.
    await expect(viewModes.getByRole('button', { name: 'Orchestration View' })).toHaveCount(0);
    await page.getByLabel('Settings View').click();
    await settingsNav.getByRole('button', { name: 'General', exact: true }).click();
    const general = page.locator('[data-component="project-general-settings"]');
    await general.getByRole('combobox', { name: 'Project Type' }).click();
    await general.getByRole('option', { name: 'Orchestration', exact: true }).click();
    await expect(general).toContainText('Choose them in Orchestration settings.');

    // 2. The first-run empty state leads to the projects to orchestrate.
    await viewModes.getByRole('button', { name: 'Orchestration View' }).click();
    const empty = page.locator('[data-orchestration-empty="no-projects"]');
    await expect(empty).toContainText('No Projects to Orchestrate Yet');
    await empty.getByRole('button', { name: 'Choose Projects…' }).click();
    const settings = page.locator('[data-component="orchestration-settings"]');
    await expect(settings.getByText('No projects yet.')).toBeVisible();
    await settings.getByRole('button', { name: 'Add Project' }).click();
    const menu = page.getByRole('menu');
    await menu
      .getByRole('menuitemcheckbox', { name: /Web/ })
      .or(menu.getByRole('menuitem', { name: /Web/ }))
      .click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Web added.' })).toBeVisible();
    await expect(settings.locator('[data-orchestration-project]').filter({ hasText: 'Web' })).toContainText(
      'Local on This Computer',
    );
    const targets = await server.request<{ targets: { target_id: string; label: string }[] }>(`${base}/targets`);
    const web = targets.targets.find((target) => target.label === 'Web')!.target_id;

    // 3. No plan yet: with no goal ticket open, the plan works through all open tickets (HS2-GSRFG0).
    await viewModes.getByRole('button', { name: 'Orchestration View' }).click();
    const noPlan = page.locator('[data-orchestration-empty="no-plan"]');
    await expect(noPlan).toContainText('Propose a plan to work through all open tickets of the chosen projects');
    await expect(toolbar).toContainText('No Plan Yet · Working through all open tickets');
    await settle(page, 'hs2-gsrfg0', 'no-plan-1440');

    // 4. The planner proposes; the plan is a dry run that cannot start without a policy (HS2-1DC8X1).
    const plan = await proposePlan(server, base, web);
    const [first, second] = plan.steps;
    await expect(view).toHaveAttribute('data-phase', 'proposed', { timeout: 30_000 });
    const dryRun = toolbar.filter({ hasText: 'This Plan Is a Dry Run' });
    await expect(dryRun).toContainText('No policy is saved yet. Apply one in Orchestration settings first.');
    await expect(dryRun.getByRole('button', { name: 'Open Orchestration Settings' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Start Orchestration' })).toBeDisabled();
    const firstCard = view.locator(`[data-ticket-slug="${first.slug}"]`);
    await expect(firstCard.locator('[data-orchestration-part="state"]')).toHaveText('Ready');
    await expect(view.locator(`[data-ticket-slug="${second.slug}"] [data-orchestration-part="state"]`)).toHaveText(
      `After ${first.slug}`,
    );

    // Why It Is Not Running says it in words (HS2-2Y535C).
    await firstCard.click();
    const inspector = page.locator('[data-orchestration-inspector]');
    const why = inspector.locator('[data-orchestration-section="Why It Is Not Running"]');
    await expect(why).toContainText('Waits: no policy is saved yet; apply one in Settings › Orchestration');
    await expect(why).not.toContainText('policy absent');
    await widths(page, 'proposed');

    // 5. A worker takes the first step: it works now, and the plan reads paused, unable to start.
    await server.request(`${base}/permission-rules`, 'POST', {
      target_id: web,
      action: 'work_start',
      actor: { role: 'human', id: 'maintainer' },
    });
    const run = await server.request<{ run_id: string }>(`${base}/runs`, 'POST', {
      ticket: { native_id: plan.goal.id },
      target_id: web,
      connection_id: first.connection_id,
      native_id: first.id,
      operation_id: 'op-golden',
      worker_id: WORKER,
      actor: scheduler,
    });
    await expect(view).toHaveAttribute('data-phase', 'paused', { timeout: 30_000 });
    const blocked = page.locator('[data-orchestration-start-blocked="policy_absent"]');
    await expect(blocked).toContainText('Start Orchestration Is Unavailable');
    await expect(blocked).toContainText('Work already running continues.');
    await expect(view.locator('[aria-label="Working Now Column"]')).toContainText(first.slug);
    await firstCard.click();
    const runSection = inspector.locator('[data-orchestration-section="Run"]');
    await expect(runSection).toContainText('Dispatched to a worker');

    // 6. The worker goes away: its card says no worker holds it, and what a person can do (HS2-FXY5QM).
    await server.request(`/tickets/${first.id}/release`, 'POST', { worker: WORKER });
    await server.request(`${base}/runs/${run.run_id}/sync`, 'POST', { actor: { role: 'human', id: 'maintainer' } });
    const line = firstCard.locator('[data-orchestration-part="state"]');
    await expect(line).toHaveText('No worker holds it', { timeout: 30_000 });
    await expect(runSection).toContainText('Hand it off, record what happened with Correct…, or cancel the run.');
    await widths(page, 'worker-gone');

    // It comes back and finishes: Done, and the next step is ready.
    await server.request(`/tickets/${first.id}/claim`, 'POST', { worker: WORKER });
    await server.request(`${base}/runs/${run.run_id}/sync`, 'POST', { actor: { role: 'human', id: 'maintainer' } });
    await expect(line).toHaveCount(0, { timeout: 30_000 });
    await server.request(`/tickets/${first.id}`, 'PATCH', { status: 'completed' });
    await server.request(`${base}/runs/${run.run_id}/sync`, 'POST', { actor: { role: 'human', id: 'maintainer' } });
    await expect(view.locator('[aria-label="Done Column"]')).toContainText(first.slug, { timeout: 30_000 });
    await expect(view.locator(`[data-ticket-slug="${second.slug}"] [data-orchestration-part="state"]`)).toHaveText(
      'Ready',
    );

    // 7. The blocked banner leads to the policy; saving it with placement off enables Start.
    await blocked.getByRole('button', { name: 'Open Orchestration Settings' }).click();
    const policy = page.locator('[data-component="orchestration-policy-settings"]');
    await expect(policy).toBeVisible();
    const automatic = policy.getByRole('switch', { name: 'Place sessions automatically' });
    await automatic.click();
    await expect(automatic).not.toBeChecked();
    await policy.getByRole('button', { name: 'Apply Policy' }).click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Policy saved.' })).toBeVisible();
    await viewModes.getByRole('button', { name: 'Orchestration View' }).click();
    await expect(blocked).toHaveCount(0);
    const start = page.getByRole('button', { name: 'Start Orchestration' });
    await expect(start).toBeEnabled({ timeout: 30_000 });

    // 8. Start: running, with Pause in its place.
    await start.click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Orchestration started.' })).toBeVisible();
    await expect(view).toHaveAttribute('data-phase', 'running', { timeout: 30_000 });
    await expect(page.getByRole('button', { name: 'Pause', exact: true })).toBeEnabled();
    expect(
      (await server.request<{ policy: { placement: { automatic: boolean } } }>(`${base}/policy`)).policy.placement
        .automatic,
    ).toBe(true);
    await server.stop(page);
    stopped = true;
  } finally {
    if (!stopped) await server.stop();
  }
});
