import { execFile } from 'node:child_process';
import { mkdir, realpath } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { promisify } from 'node:util';

import { expect, type Locator, type Page, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { fakeGithubActions } from './fake-github-actions';
import { capture, handOffInPool, SYNTHETIC_TOOL, syntheticTool } from './orchestration-real-fixtures';
import { proxyFetch, realTicketServer, routeProjectChangeSocket } from './real-ticket-server';

const execute = promisify(execFile);

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

async function openTwoProjects(
  page: Page,
  options: Parameters<typeof realTicketServer>[0] & { gitWeb?: boolean } = {},
) {
  const { gitWeb, ...fixture } = options;
  // The run starts sessions under the real default policy (2 GB free memory, 90% CPU, 5 GB free disk), which
  // a busy CI runner may not meet; the test build's roomy capacity keeps admission independent of the
  // machine running the test (HS2-D9KP86, as HS2-YWN8JT does for the Rust real-server tests).
  // Sessions need an AI tool. A developer machine may have one installed, a CI runner has none, so the
  // fixture home always carries the synthetic agent (HS2-D9KP86).
  const server = await realTicketServer({
    ...fixture,
    env: { HOTSHEET_TEST_ROOMY_CAPACITY: '1', ...fixture.env },
    seed: async (context) => {
      await syntheticTool(context.env.HOTSHEET_HOME!);
      await fixture.seed?.(context);
    },
  });
  const projects = [{ id: server.checkoutId, root: server.root, name: 'Launch' }];
  await mkdir(join(dirname(server.root), 'web'), { recursive: true });
  // HS2-2751D6: Web as a git checkout has its own coordination pool, read at its resolved path.
  const root = await realpath(join(dirname(server.root), 'web'));
  if (gitWeb) {
    const env = withoutGitRepositoryEnv(server.env);
    await execute('git', ['init', '-q', '-b', 'main'], { cwd: root, env });
    await execute('git', ['commit', '-q', '--allow-empty', '-m', 'Synthetic base'], { cwd: root, env });
  }
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
  return Object.assign(server, { webId: web.id, webRoot: root });
}

/**
 * What the planning AI session does with the default goal: a goal and three steps, each after the one before.
 * HS2-2751D6: `gates` is what each step needs of the one before (`ci`: its CI gate passed).
 */
async function proposePlan(server: Server, base: string, target: string, gates: 'none' | 'ci' = 'none') {
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
                gates,
              },
            ],
      actor: scheduler,
    });
  return { goal, steps };
}

/**
 * HS2-QCKVVY: the board is placed at its right end in the frame after a width change, so a capture waits two
 * animation frames; a screenshot taken at once can show the frame before (seen in WebKit).
 */
async function settle(page: Page, ticket: string, name: string) {
  await page.evaluate(
    () =>
      new Promise((resolve) => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            resolve(undefined);
          });
        });
      }),
  );
  await capture(page, ticket, name);
}

/** How much of a column shows inside the board's horizontal scroller, from 0 to 1. */
const visibleShare = (page: Page, label: string) =>
  page.locator(`[data-orchestration-view="true"] [aria-label="${label}"]`).evaluate((column) => {
    let track = column.parentElement;
    while (track && !/auto|scroll/.test(getComputedStyle(track).overflowX)) track = track.parentElement;
    const box = column.getBoundingClientRect(),
      view = track!.getBoundingClientRect();
    return Math.max(0, Math.min(box.right, view.right) - Math.max(box.left, view.left)) / box.width;
  });

/**
 * Wide, medium, narrow, wide again, then dark at wide and narrow (Visual QA). HS2-2751D6: `show` is an inspector
 * section to scroll into view where the inspector shows beside the board (not at phone width, where it is a sheet).
 */
async function widths(page: Page, name: string, ticket = 'hs2-1ddxns', show?: Locator) {
  const reveal = async (width: number) => {
    if (show && width > 390) await show.scrollIntoViewIfNeeded();
  };
  for (const [width, height] of [
    [1440, 900],
    [1100, 820],
    [390, 844],
    [1440, 900],
  ] as const) {
    await page.setViewportSize({ width, height });
    await reveal(width);
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
    // HS2-QCKVVY: after a width change the board returns to its right end, the selected First in view,
    // in WebKit as in Chromium (it lands within two frames of the resize).
    for (const width of [1100, 1440, 1100]) {
      await page.setViewportSize({ width, height: 820 });
      await expect.poll(() => visibleShare(page, 'First · On Start Column')).toBeGreaterThan(0.95);
    }
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
    try {
      await expect(ignore).toBeVisible({ timeout: 30_000 });
    } catch (error) {
      // Say why no session was asked for: the scheduler's own explanation of the ready step.
      const explained = await server
        .request(`${base}/scheduler/explain`, 'POST', {
          target_id: web,
          connection_id: second.connection_id,
          native_id: second.id,
          actor: { role: 'human', id: 'maintainer' },
        })
        .catch((reason: unknown) => String(reason));
      throw new Error(`${String(error)}\nscheduler explain: ${JSON.stringify(explained)}`, { cause: error });
    }
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

/** HS2-2751D6: a second orchestrated project, Web, bound to Launch as a local target. */
const WEB_TARGET = '01ARZ3NDEKTSV4RRFFQ69G5FJ9';
const REPO = 'synthetic-org/web';

async function orchestrateWeb(server: Awaited<ReturnType<typeof openTwoProjects>>, base: string) {
  const person = { role: 'human', id: 'maintainer' };
  const added = await server.request<{ revision: string }>(`${base}/targets`, 'POST', {
    target_id: WEB_TARGET,
    label: 'Web',
    expected_revision: null,
  });
  const candidate = await server.request<{ destination: unknown }>(`${base}/inspect`, 'POST', {
    destination: { kind: 'local', checkout_id: server.webId },
  });
  await server.request(`${base}/targets/${WEB_TARGET}/binding`, 'PUT', {
    target: { owner_checkout_id: server.checkoutId, target_id: WEB_TARGET },
    expected_manifest_revision: added.revision,
    expected_binding_revision: 0,
    previous_destination: null,
    destination: candidate.destination,
  });
  for (const rule of [{ action: 'work_start' }, { action: 'session_start', tool: SYNTHETIC_TOOL }])
    await server.request(`${base}/permission-rules`, 'POST', { target_id: WEB_TARGET, ...rule, actor: person });
  await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
  // Web's Coordination Sessions include the integrator; placement stays off, so the scheduler's passes are dry
  // runs and no AI session starts. The machine limits never refuse on a busy test host.
  const current = await server.request<{ revision?: string | null }>(`${base}/policy`);
  await server.request(`${base}/policy`, 'PUT', {
    expected_revision: current.revision ?? null,
    policy: {
      machine: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
      tools: [{ tool: SYNTHETIC_TOOL, max_sessions: 2 }],
      targets: [{ target_id: WEB_TARGET, tools: [{ tool: SYNTHETIC_TOOL }], max_sessions: 2 }],
      placement: {
        automatic: false,
        actor: 'scheduler-1',
        cooldown_minutes: 0,
        role_sessions: [{ target_id: WEB_TARGET, roles: ['integrator'] }],
      },
    },
    actor: person,
  });
}

/**
 * HS2-2751D6: the golden path past a finished run, through the shipped shell against a real server. A worker
 * hands its work off in Web's real coordination pool (HS2-6ES76F), so its card says no worker holds it and the
 * Integration section says a handoff waits and the integrator would start; the integrator pushes it and the
 * commit's CI gate goes red through the real gate follower and Job monitor against a loopback GitHub Actions
 * fake (HS2-8KV14Y), so the step that needs it says CI failed and the run's Gates say what to do; the fix's gate
 * passes and the ticket is completed, so the work is Done and the next step Ready. Only the planner's proposal,
 * the worker's and the integrator's actions and the CI provider are played by the test. Synthetic data only.
 */
test('HS2-2751D6 a handoff waits on the integrator, its CI goes red and the fix goes green', async ({ page }) => {
  test.setTimeout(300_000);
  const provider = await fakeGithubActions();
  const server = await openTwoProjects(page, {
    env: { ...provider.env(), HOTSHEET_JOB_MONITOR_AUTOSTART: 'on' },
    gitWeb: true,
  });
  const base = `/checkouts/${server.checkoutId}/orchestration`;
  const maintainer = { role: 'human', id: 'maintainer' };
  try {
    await orchestrateWeb(server, base);
    const plan = await proposePlan(server, base, WEB_TARGET, 'ci');
    const [first, second] = plan.steps;
    const run = await server.request<{ run_id: string }>(`${base}/runs`, 'POST', {
      ticket: { native_id: plan.goal.id },
      target_id: WEB_TARGET,
      connection_id: first.connection_id,
      native_id: first.id,
      operation_id: 'op-golden-ci',
      worker_id: 'synthetic-worker',
      actor: scheduler,
    });
    const sync = () => server.request(`${base}/runs/${run.run_id}/sync`, 'POST', { actor: maintainer });

    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.getByRole('tab', { name: /Launch/ }).click();
    await page.getByRole('group', { name: 'View mode' }).getByRole('button', { name: 'Orchestration View' }).click();
    const view = page.locator('[data-orchestration-view="true"]');
    const inspector = page.locator('[data-orchestration-inspector]');
    const firstCard = view.locator(`[data-ticket-slug="${first.slug}"]`);
    const firstLine = firstCard.locator('[data-orchestration-part="state"]');
    const secondLine = view.locator(`[data-ticket-slug="${second.slug}"] [data-orchestration-part="state"]`);
    const runSection = inspector.locator('[data-orchestration-section="Run"]');
    const gates = inspector.locator('[data-orchestration-section="Gates"]');
    const integration = inspector.locator('[data-orchestration-section="Integration"]');
    const row = (section: typeof gates, label: string) =>
      section
        .locator('dl > div')
        .filter({ has: page.locator('dt', { hasText: label }) })
        .locator('dd');
    await expect(view.locator('[aria-label="Working Now Column"]')).toContainText(first.slug, { timeout: 30_000 });
    await firstCard.click();
    await expect(runSection).toContainText('Dispatched to a worker');
    await expect(gates).toContainText('No gate is linked to this run yet.');

    // 1. The worker hands its work off in Web's pool and lets the ticket go. Its card says nobody holds it and
    // that a handoff may be in review; the Integration section says a handoff waits and the integrator would
    // start on the next pass, since placement is off.
    // The pool assigns only a worker with no live claim, so the dispatch's claim is given back to be taken again
    // under the pool's assignment.
    await server.request(`/tickets/${first.id}/release`, 'POST', { worker: 'synthetic-worker' });
    const handoff = await handOffInPool(server, first, server.webRoot);
    await sync();
    await expect(firstLine).toHaveText('No worker holds it', { timeout: 30_000 });
    await expect(runSection).toContainText('Its handoff may be in review or back for corrections');
    await expect(row(integration, 'Integrator')).toHaveText('Would start on the next pass (dry run)', {
      timeout: 30_000,
    });
    await expect(row(integration, 'Why')).toHaveText('A handoff waits to be integrated');
    await expect(integration).not.toContainText('_');
    await widths(page, 'handoff-waits', 'hs2-2751d6', integration);

    // 2. The integrator pushes it and links its commit's CI gate, which runs: the next step waits on it.
    await handoff.integrate();
    provider.run(REPO, { id: 21, sha: handoff.commit, status: 'in_progress' });
    await server.request(`${base}/runs/${run.run_id}/gates`, 'POST', {
      commit: handoff.commit,
      repository: REPO,
      actor: scheduler,
    });
    await expect(row(gates, `CI · ${handoff.commit.slice(0, 7)}`)).toHaveText('Pending', { timeout: 30_000 });
    // No integrator is live once it pushed, so a pass would still start one for the project's pool.
    await expect(row(integration, 'Integrator')).toHaveText('Would start on the next pass (dry run)');
    await expect(secondLine).toHaveText(`Waits on ${first.slug}`);
    await expect(row(gates, 'What to do')).toHaveCount(0);

    // 3. CI fails. The next step says its prerequisite's CI failed, and the run's Gates say what to do.
    provider.run(REPO, { id: 21, sha: handoff.commit, status: 'completed', conclusion: 'failure' });
    await expect(row(gates, `CI · ${handoff.commit.slice(0, 7)}`)).toHaveText('Failed', { timeout: 60_000 });
    await expect(row(gates, 'What to do')).toHaveText(
      'Fix the work and push the fix; link the new commit’s CI gate, which replaces this one. Work that needs this ticket’s CI waits until it passes.',
    );
    await expect(secondLine).toHaveText(`Waits on ${first.slug} · CI failed`, { timeout: 30_000 });
    await expect(secondLine).toHaveAttribute('data-tone', 'warning');
    await widths(page, 'ci-red', 'hs2-2751d6', gates);

    // 4. The fix is pushed and its gate linked: it replaces the failed one, passes, and the ticket is completed.
    const env = withoutGitRepositoryEnv(server.env);
    await execute('git', ['commit', '-q', '--allow-empty', '-m', 'Synthetic fix'], { cwd: server.webRoot, env });
    const fix = (await execute('git', ['rev-parse', 'HEAD'], { cwd: server.webRoot, env })).stdout.trim();
    provider.run(REPO, { id: 22, sha: fix, status: 'in_progress' });
    await server.request(`${base}/runs/${run.run_id}/gates`, 'POST', {
      commit: fix,
      repository: REPO,
      actor: scheduler,
    });
    await expect(row(gates, `CI · ${fix.slice(0, 7)}`)).toHaveText('Pending', { timeout: 30_000 });
    await expect(row(gates, `CI · ${handoff.commit.slice(0, 7)}`)).toHaveCount(0);
    await expect(secondLine).toHaveText(`Waits on ${first.slug}`, { timeout: 30_000 });
    provider.run(REPO, { id: 22, sha: fix, status: 'completed', conclusion: 'success' });
    await expect(row(gates, `CI · ${fix.slice(0, 7)}`)).toHaveText('Passed', { timeout: 60_000 });
    await expect(row(gates, 'What to do')).toHaveCount(0);
    await server.request(`/tickets/${first.id}`, 'PATCH', { status: 'completed' });
    await sync();
    await expect(view.locator('[aria-label="Done Column"]')).toContainText(first.slug, { timeout: 30_000 });
    await expect(secondLine).toHaveText('Ready', { timeout: 30_000 });
    await widths(page, 'ci-green-done', 'hs2-2751d6', gates);
    // The test only read the provider.
    expect(provider.requests.every((request) => request.startsWith('GET '))).toBe(true);
  } finally {
    await execute(server.cli, ['job', 'watch', 'stop'], { cwd: server.root, env: server.env }).catch(() => undefined);
    await server.stop(page);
    await provider.close();
  }
});
