import { mkdir } from 'node:fs/promises';
import { dirname, join } from 'node:path';

import { expect, type Page, test } from '@playwright/test';

import { capture } from './orchestration-real-fixtures';
import { proxyFetch, realTicketServer, routeProjectChangeSocket } from './real-ticket-server';

/**
 * HS2-1DDXNS: the orchestration workflow as a new person meets it, in one flow through the shipped
 * shell against a real isolated server. Launch starts as a Software project beside an open Web
 * project; the person makes Launch an orchestration project, adds Web, sees the dry-run plan, follows
 * a dispatched run through a worker letting go and finishing, starts orchestration with no policy saved
 * (which saves the default one, HS2-D9KP86), pauses it and applies a policy that places sessions again. Only the planner's proposal and the worker's own claim actions are made through the
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
async function widths(page: Page, name: string, ticket = 'hs2-1ddxns') {
  for (const [width, height] of [
    [1440, 900],
    [1100, 820],
    [390, 844],
    [1440, 900],
  ] as const) {
    await page.setViewportSize({ width, height });
    await settle(page, ticket, `${name}-${width}`);
  }
  await page.emulateMedia({ colorScheme: 'dark' });
  await settle(page, ticket, `${name}-1440-dark`);
  await page.setViewportSize({ width: 390, height: 844 });
  await settle(page, ticket, `${name}-390-dark`);
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
    // A project with no work yet says so in the overview, never "not read" (HS2-70MHXJ).
    const overview = page.locator('[data-orchestration-overview]');
    await expect(overview).toContainText('0 done · 0 working · no work yet');
    await expect(overview).not.toContainText('not read');
    await widths(page, 'no-work-yet', 'hs2-70mhxj');

    // 4. The planner proposes; the plan is a dry run. No policy is saved, and Start Orchestration says it
    // saves the default one, with the way to review it (HS2-D9KP86).
    const plan = await proposePlan(server, base, web);
    const [first, second] = plan.steps;
    await expect(view).toHaveAttribute('data-phase', 'proposed', { timeout: 30_000 });
    const dryRun = toolbar.filter({ hasText: 'This Plan Is a Dry Run' });
    await expect(dryRun).toContainText(
      'Start Orchestration saves the default policy shown in Orchestration settings and starts sessions within it.',
    );
    await expect(dryRun.getByRole('button', { name: 'Open Orchestration Settings' })).toBeVisible();
    const start = page.getByRole('button', { name: 'Start Orchestration' });
    await expect(start).toBeEnabled();
    await expect(start).toHaveAttribute('title', 'Save the default policy and place sessions automatically within it');
    const firstCard = view.locator(`[data-ticket-slug="${first.slug}"]`);
    await expect(firstCard.locator('[data-orchestration-part="state"]')).toHaveText('Ready');
    await expect(view.locator(`[data-ticket-slug="${second.slug}"] [data-orchestration-part="state"]`)).toHaveText(
      `After ${first.slug}`,
    );

    // Why It Is Not Running says it in words (HS2-2Y535C).
    await firstCard.click();
    const inspector = page.locator('[data-orchestration-inspector]');
    const why = inspector.locator('[data-orchestration-section="Why It Is Not Running"]');
    await expect(why).toContainText('Waits: no policy is saved yet; Start Orchestration saves the default one');
    await expect(why).not.toContainText('policy absent');
    await widths(page, 'proposed');

    // 5. A worker takes the first step: it works now; orchestration still places nothing and can start.
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
    await expect(page.locator('[data-orchestration-start-blocked]')).toHaveCount(0);
    await expect(start).toBeEnabled();
    // Nobody started orchestration, so it is Not Started with the work counted, not Paused (HS2-38R8J2).
    await expect(toolbar).toContainText('Not Started · 1 Working');
    await expect(toolbar).not.toContainText('Paused');
    await expect(dryRun).toBeVisible();
    await widths(page, 'not-started', 'hs2-38r8j2');
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

    // 7. Start with no policy saved: one press saves the default policy with placement on (HS2-D9KP86).
    await start.click();
    await expect(
      page.locator('.app-toast').filter({ hasText: 'Saved the default policy and started orchestration.' }),
    ).toBeVisible();
    await expect(view).toHaveAttribute('data-phase', 'running', { timeout: 30_000 });
    const pause = page.getByRole('button', { name: 'Pause', exact: true });
    await expect(pause).toBeEnabled();
    const stored = await server.request<{
      policy: { placement: { automatic: boolean }; machine: Record<string, number> };
    }>(`${base}/policy`);
    expect(stored.policy.placement.automatic).toBe(true);
    expect(stored.policy.machine).toEqual({
      max_sessions: 2,
      min_free_ram_mb: 2048,
      max_cpu_percent: 90,
      min_free_disk_mb: 5120,
    });
    // Running asks a person before the first session starts; the request shows over the header.
    const ignore = page.getByRole('button', { name: 'Ignore', exact: true });
    await expect(ignore).toBeVisible({ timeout: 30_000 });
    await widths(page, 'started-default', 'hs2-d9kp86');
    await ignore.click();
    await expect(ignore).toHaveCount(0);

    // 8. Pause, then Apply Policy with placement on: the save starts placing sessions and says so.
    await pause.click();
    await expect(page.locator('.app-toast').filter({ hasText: 'Orchestration paused.' })).toBeVisible();
    await expect(view).toHaveAttribute('data-phase', 'paused', { timeout: 30_000 });
    // Started once, so a pause is Paused (HS2-38R8J2).
    await expect(toolbar).toContainText('Paused · ');
    await expect(toolbar).not.toContainText('Not Started');
    await expect(dryRun).toHaveCount(0);
    await widths(page, 'paused-after-start', 'hs2-38r8j2');
    await page.getByLabel('Settings View').click();
    await settingsNav.getByRole('button', { name: 'Orchestration', exact: true }).click();
    const policy = page.locator('[data-component="orchestration-policy-settings"]');
    const automatic = policy.getByRole('switch', { name: 'Place sessions automatically' });
    await expect(automatic).not.toBeChecked();
    await automatic.click();
    await expect(automatic).toBeChecked();
    await policy.getByRole('button', { name: 'Apply Policy' }).click();
    await expect(
      page.locator('.app-toast').filter({ hasText: 'Policy saved. Orchestration is placing sessions.' }),
    ).toBeVisible();
    await widths(page, 'policy-applied', 'hs2-d9kp86');
    await viewModes.getByRole('button', { name: 'Orchestration View' }).click();
    await expect(view).toHaveAttribute('data-phase', 'running', { timeout: 30_000 });
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
