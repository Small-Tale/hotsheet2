import { execFile } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { promisify } from 'node:util';

import { expect, type Locator, type Page, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { proxyFetch, type RealTicketFixtureContext, realTicketServer } from './real-ticket-server';

const execute = promisify(execFile);

/**
 * HS2-2XJSJK build tickets: a real server whose project is its own orchestration target ("Self"), so
 * the plan, runs, validation steps and the scheduler's records the card inspector and the overview
 * read are real records. Synthetic tickets only. The same set-up as `orchestration-view.spec.ts`.
 */
export type Server = Awaited<ReturnType<typeof realTicketServer>>;
export const TARGET = '01ARZ3NDEKTSV4RRFFQ69G5FJ8';
export const scheduler = { role: 'ai', id: 'scheduler-1' };

export async function send(server: Server, method: string, path: string, body?: unknown) {
  const response = await fetch(`${server.url}${path}`, {
    method,
    headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  if (!response.ok) throw new Error(`${method} ${path}: ${response.status} ${text}`);
  return JSON.parse(text) as Record<string, unknown>;
}

/** An orchestration project (HS2-W27N1D) that is its own bound target, with the dispatch rule remembered. */
export async function orchestrateSelf(server: Server) {
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
  await send(server, 'PUT', `${base}/project-type`, { project_type: 'orchestration', actor: { role: 'human' } });
}

export interface Ticket {
  id: string;
  slug: string;
  connection_id: string;
}

/** A goal and two steps, the second waiting on the first; `dispatch` starts the first's run. */
export async function planTwoSteps(server: Server) {
  const base = `/checkouts/${server.checkoutId}/orchestration`;
  const goal = await server.request<Ticket>('/tickets', 'POST', { title: 'Ship the synthetic sign-on' });
  const first = await server.request<Ticket>('/tickets', 'POST', { title: 'Add the synthetic token endpoint' });
  const second = await server.request<Ticket>('/tickets', 'POST', { title: 'Add the synthetic login page' });
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
        operation_id: 'op-details',
        worker_id: 'browser-worker',
        actor: scheduler,
      }),
  };
}

/** Open the server's project in the real app through the dev bridge, then the Orchestration view. */
export async function openOrchestration(page: Page, server: Server, name = 'Launch') {
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
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.locator('[data-segment-value="orchestration"]').first().click();
  await expect(page.locator('[data-orchestration-view="true"]')).toBeVisible();
}

/** Capture once every finite animation (rails and sheets slide) has finished. */
export async function capture(page: Page, ticket: string, name: string) {
  await page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity),
  );
  await page.screenshot({ path: test.info().outputPath(`${ticket}-${name}.png`) });
}

/**
 * The real-app visual pass of the Visual QA rule: 1440, 1100 and 390 (with `atPhone` to reopen what a
 * phone shows in its inspector sheet), then wide again after narrow, and dark at 1440.
 */
export async function visualPass(
  page: Page,
  ticket: string,
  state: string,
  options: { check: () => Promise<void>; atPhone?: () => Promise<void>; show?: Locator },
) {
  for (const [width, height] of [
    [1440, 900],
    [1100, 820],
    [390, 844],
  ] as const) {
    await page.setViewportSize({ width, height });
    if (width === 390) await options.atPhone?.();
    await options.check();
    await options.show?.scrollIntoViewIfNeeded();
    await capture(page, ticket, `${state}-${width}`);
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(page.locator('[data-orchestration-view="true"]')).toHaveAttribute('data-layout', 'grid');
  await options.check();
  await options.show?.scrollIntoViewIfNeeded();
  await capture(page, ticket, `${state}-1440-after-narrow`);
  await page.emulateMedia({ colorScheme: 'dark' });
  await capture(page, ticket, `${state}-1440-dark`);
  await page.setViewportSize({ width: 390, height: 844 });
  await options.atPhone?.();
  await options.show?.scrollIntoViewIfNeeded();
  await capture(page, ticket, `${state}-390-dark`);
  await page.emulateMedia({ colorScheme: 'light' });
  await page.setViewportSize({ width: 1440, height: 900 });
}

/** An AI tool no machine really has: detected through Node, which every test host runs (HS2-KTHJN2). */
export const SYNTHETIC_TOOL = 'hs-synthetic-agent';

/**
 * A synthetic AI tool in the fixture server's own plugin directory: it launches the Node binary
 * that runs this spec and holds, so no real AI tool is started or probed.
 */
/**
 * The synthetic tool. HS2-G2X7RT: `reportsStart` declares a `SessionStart` hook, so a session of it is
 * `tool_ready: false` until that hook reports in, as a real tool on a first-run question is.
 */
export async function syntheticTool(home: string, { reportsStart = false }: { reportsStart?: boolean } = {}) {
  const plugin = join(home, 'plugins', SYNTHETIC_TOOL);
  await mkdir(plugin, { recursive: true });
  await writeFile(join(plugin, 'instructions.md'), 'Synthetic agent\n');
  await writeFile(
    join(plugin, 'manifest.toml'),
    [
      `id = "${SYNTHETIC_TOOL}"`,
      `display_name = "${SYNTHETIC_TOOL}"`,
      'product_name = "Synthetic agent"',
      'tier = "cli-agent"',
      // Detected through the Node binary already on PATH; driven by spawning it.
      '[detection]',
      'binaries = ["node"]',
      '[drive]',
      'transport = "spawn"',
      `program = ${JSON.stringify(process.execPath)}`,
      'args = []',
      'content = "arg"',
      '[instructions]',
      'target = "AGENTS.md"',
      'section = "instructions.md"',
      '[mcp]',
      'target = ".mcp.json"',
      'format = "claude-json"',
      'server_name = "hotsheet"',
      'command = "hotsheet-mcp"',
      'args = ["--path", "{store}"]',
      '[launch]',
      `program = ${JSON.stringify(process.execPath)}`,
      `args = ["-e", "process.stdout.write('ready\\\\n'); setInterval(() => {}, 1000000)"]`,
      ...(reportsStart
        ? [
            '[hooks]',
            'target = ".synthetic/hooks.json"',
            'machine_local = true',
            'event = "PermissionRequest"',
            'additional_events = ["SessionStart"]',
            'command = "hotsheet-cli permission-hook --agent synthetic"',
          ]
        : []),
      '',
    ].join('\n'),
  );
}

/**
 * HS2-6ES76F: make the fixture project a git checkout, so it has a coordination pool (in its git common
 * directory) that the scheduler reads for the project's integrator. Use as (part of) a `realTicketServer` seed.
 */
export async function gitCheckout({ root, env }: RealTicketFixtureContext) {
  const clean = withoutGitRepositoryEnv(env);
  await execute('git', ['init', '-q', '-b', 'main'], { cwd: root, env: clean });
  await execute('git', ['commit', '-q', '--allow-empty', '-m', 'Synthetic base'], { cwd: root, env: clean });
}

/**
 * HS2-6ES76F: hand a ticket's work off in the project's real coordination pool through the headless
 * coordination CLI, as the integrator and worker role skills do: the integrator assigns the ticket and
 * leaves (so no integrator is live), the worker claims it, commits, writes its Integration handoff note,
 * releases and records the handoff. The pool then shows one handoff pending and no integrator.
 * `integrate()` plays the integrator taking it up and pushing it, so its commit waits on its CI gate.
 * The project must be a git checkout (`gitCheckout`); the ticket is synthetic.
 */
export async function handOffInPool(server: Server, ticket: Ticket) {
  const env = withoutGitRepositoryEnv({ ...server.env, HOTSHEET_ACTOR_ROLE: 'system' });
  delete env.HOTSHEET_ACTOR_ID;
  delete env.HOTSHEET_WORKER_ID;
  const cli = (...args: string[]) => execute(server.cli, ['-C', server.store, ...args], { cwd: server.root, env });
  const pool = (...args: string[]) => cli('coordination', '--project', server.root, ...args);
  const git = async (...args: string[]) => (await execute('git', args, { cwd: server.root, env })).stdout.trim();
  const assignment = 'synthetic-handoff';
  const worker = ['--id', 'synthetic-worker', '--session', 'synthetic-worker-1', '--assignment', assignment];
  const base = await git('rev-parse', 'HEAD');
  await cli('edit', ticket.slug, '--up-next');
  await pool('setup');
  await pool('register', '--role', 'integrator', '--id', 'synthetic-integrator', '--session', 'integrator-1');
  await pool('register', '--role', 'worker', '--id', 'synthetic-worker', '--session', 'synthetic-worker-1');
  await pool(
    'assign',
    ...['--id', 'synthetic-integrator', '--session', 'integrator-1'],
    ...['--worker', 'synthetic-worker', '--worker-session', 'synthetic-worker-1', '--operation', assignment],
    ...['--connection', ticket.connection_id, '--native-id', ticket.id, '--slug', ticket.slug, '--base', base],
  );
  await pool('leave', '--role', 'integrator', '--id', 'synthetic-integrator', '--session', 'integrator-1');
  await cli('claim', ticket.slug, '--worker', 'synthetic-worker', '--eta', '30m');
  await pool('worker', ...worker, '--phase', 'accepted');
  await git('commit', '-q', '--allow-empty', '-m', 'Synthetic change');
  const commit = await git('rev-parse', 'HEAD');
  await git('reset', '-q', '--hard', base);
  await cli('edit', ticket.slug, '--note', `## Integration handoff\nSynthetic commit ${commit}`);
  await cli('release', ticket.slug, '--worker', 'synthetic-worker');
  await pool('worker', ...worker, '--phase', 'handoff', '--commit', commit);
  return {
    assignment,
    commit,
    /** The integrator takes the handoff up and pushes it: the commit now waits on its CI gate. */
    integrate: async () => {
      await pool('register', '--role', 'integrator', '--id', 'synthetic-integrator', '--session', 'integrator-2');
      for (const phase of ['integrating', 'ci'])
        await pool(
          'mark',
          '--id',
          'synthetic-integrator',
          '--session',
          'integrator-2',
          '--assignment',
          assignment,
          '--phase',
          phase,
        );
      await pool('leave', '--role', 'integrator', '--id', 'synthetic-integrator', '--session', 'integrator-2');
    },
  };
}

export { realTicketServer };
