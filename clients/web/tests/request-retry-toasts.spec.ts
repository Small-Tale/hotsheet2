import { expect, type Page, type Route, test } from '@playwright/test';

import { openMockedProject, projectTab } from './open-mocked-project';
import {
  capture,
  openOrchestration,
  orchestrateSelf,
  planTwoSteps,
  scheduler,
  send,
  TARGET,
} from './orchestration-real-fixtures';
import { proxyFetch, realTicketServer } from './real-ticket-server';

type Server = Awaited<ReturnType<typeof realTicketServer>>;

/**
 * HS2-CX563Y (follow-up of HS2-JGFPG4): the real app against real servers, with only the request
 * under test intercepted. A transient failure is retried on the bounded backoff and recovers with no
 * toast; a failure that persists through five tries shows exactly one toast; showing the project or
 * the view again asks again. Synthetic data only.
 */

/**
 * Records every app toast the page shows, by its generation, and keeps each on screen (its lifetime
 * timer, scheduled from toast-lifetime.ts, is stretched) so the visual pass can resize around it.
 */
async function recordToasts(page: Page) {
  await page.addInitScript(() => {
    const original = window.setTimeout.bind(window);
    window.setTimeout = ((handler: TimerHandler, delay?: number, ...rest: unknown[]) =>
      original(
        handler,
        new Error().stack?.includes('toast-lifetime') ? 600_000 : delay,
        ...rest,
      )) as typeof window.setTimeout;
    const seen = new Map<string, string>();
    (window as unknown as { hs2Toasts: () => string[] }).hs2Toasts = () => [...seen.values()];
    const read = () => {
      for (const item of document.querySelectorAll<HTMLElement>('[data-toast-generation]')) {
        const generation = item.dataset.toastGeneration!,
          text = item.querySelector('.app-toast')?.textContent ?? '';
        if (text && !seen.has(generation)) seen.set(generation, text);
      }
    };
    new MutationObserver(read).observe(document, {
      subtree: true,
      childList: true,
      characterData: true,
      attributes: true,
      attributeFilter: ['data-toast-generation'],
    });
  });
  return (pattern: RegExp) =>
    page.evaluate(
      (source) =>
        (window as unknown as { hs2Toasts: () => string[] })
          .hs2Toasts()
          .filter((text) => new RegExp(source, 'u').test(text)),
      pattern.source,
    );
}

/** The shown toast at 1440, 1100, 390 and 1440 again, in the light then the dark scheme. */
async function toastPass(page: Page, ticket: string, state: string, text: RegExp) {
  const toast = page.locator('wa-toast-item').filter({ has: page.locator('.app-toast', { hasText: text }) });
  const check = async (width: number) => {
    await expect(toast).toBeVisible();
    const box = (await toast.boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(width + 0.5);
    // The message wraps inside the toast instead of running past it.
    expect(
      await toast
        .locator('.app-toast')
        .evaluate((node) => node.scrollWidth <= node.clientWidth + 1 && node.getBoundingClientRect().width > 0),
    ).toBe(true);
  };
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    for (const [width, height, name] of [
      [1440, 900, '1440'],
      [1100, 820, '1100'],
      [390, 844, '390'],
      [1440, 900, '1440-after-narrow'],
    ] as const) {
      await page.setViewportSize({ width, height });
      await check(width);
      await capture(page, ticket, `${state}-${name}-${scheme}`);
    }
  }
  await page.emulateMedia({ colorScheme: 'light' });
  await page.setViewportSize({ width: 1440, height: 900 });
}

const rejected = (server: Server) => ({
  checkout_id: server.checkoutId,
  stores: [
    {
      connection_id: 'git-local',
      label: 'synthetic.hs2',
      locator: '/work/synthetic.hs2',
      health: {
        state: 'rejected',
        syncing: false,
        unpublished_commits: 1,
        incoming_commits: 0,
        consecutive_failures: 1,
        reason: 'The remote refused the push.',
        next_action: 'Fix the remote, then run `hotsheet-cli sync`.',
      },
    },
  ],
});

/** Proxy a project's API to its real server. */
async function routeProject(page: Page, server: Server) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
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
  return apiPath;
}

test('HS2-CX563Y a sync-health read retries a transient failure and shows one toast for a lasting one', async ({
  page,
}) => {
  test.setTimeout(150_000);
  const alpha = await realTicketServer(),
    beta = await realTicketServer();
  try {
    const toasts = await recordToasts(page);
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    const alphaApi = await routeProject(page, alpha),
      betaApi = await routeProject(page, beta);
    // Each project's sync-health reads, answered by `plan` in order; registered last, so it wins.
    const reads = { alpha: 0, beta: 0 };
    let betaFails = true;
    await page.route('**/repository/sync', async (route: Route) => {
      const path = new URL(route.request().url()).pathname;
      const which = path.startsWith(alphaApi) ? 'alpha' : path.startsWith(betaApi) ? 'beta' : undefined;
      if (!which) return route.fallback();
      reads[which] += 1;
      // Alpha: two dropped requests, then a refused-push report. Beta: dropped until told otherwise.
      const fail = which === 'alpha' ? reads.alpha <= 2 : betaFails;
      if (fail) return route.abort('failed');
      return route.fulfill({ json: rejected(which === 'alpha' ? alpha : beta) });
    });
    let opened = 0;
    await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) => {
      const server = opened++ === 0 ? alpha : beta;
      return route.fulfill({
        status: 201,
        json: {
          id: server.checkoutId,
          root: server.root,
          name: server === alpha ? 'alpha' : 'beta',
          stores: [server.store],
          apiPath: server === alpha ? alphaApi : betaApi,
        },
      });
    });
    await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: beta.root } }));
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await openMockedProject(page);

    // Alpha: the third read answers and the banner shows; the dropped reads raised no toast.
    const banner = page.locator('[data-component="ticket-store-sync-banner"]');
    await expect(banner).toBeVisible({ timeout: 30_000 });
    expect(reads.alpha).toBe(3);
    expect(await toasts(/ticket sync health could not be read/u)).toEqual([]);

    // Beta: every read fails; after five tries one toast names the project, and no banner shows.
    const strip = page.locator('.project-tab-bar:not(.project-tab-bar--mobile)');
    await strip.getByRole('button', { name: 'Add Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await expect(projectTab(strip, 'beta')).toHaveAttribute('aria-selected', 'true');
    // HS2-1HGTD7: the same plain sentence in every browser, never the browser's own network text.
    const lasting = /^beta: ticket sync health could not be read\. The Hot Sheet server could not be reached\.$/u;
    await expect.poll(() => toasts(lasting), { timeout: 60_000 }).toHaveLength(1);
    expect(reads.beta).toBe(5);
    await expect(banner).toHaveCount(0);
    await toastPass(page, 'hs2-cx563y', 'sync-health-toast', lasting);
    expect(reads.beta).toBe(5);

    // Showing alpha does not read it again; showing beta again asks again and fills its banner.
    betaFails = false;
    await projectTab(strip, 'alpha').click();
    await expect(banner).toBeVisible();
    await projectTab(strip, 'beta').click();
    await expect(banner).toBeVisible({ timeout: 30_000 });
    expect(reads).toEqual({ alpha: 3, beta: 6 });
    expect(await toasts(/ticket sync health could not be read/u)).toHaveLength(1);
  } finally {
    await alpha.stop(page);
    await beta.stop(page);
  }
});

test('HS2-CX563Y following a validation retries a dropped request and shows one toast when it keeps failing', async ({
  page,
}) => {
  test.setTimeout(150_000);
  const server = await realTicketServer();
  try {
    const toasts = await recordToasts(page);
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    await send(server, 'POST', `/checkouts/${server.checkoutId}/orchestration/validations/define`, {
      ticket: { native_id: plan.goal.id },
      step: { step_id: 'smoke', target_id: TARGET, check: { kind: 'command', program: 'true', args: [] } },
      actor: scheduler,
    });
    await plan.dispatch();
    // The run request answers `running` when asked to start, and each follow (an 8-second wait) as
    // `follow` says: drop the request, refuse with 503, or answer `passed`.
    const follows: Array<'drop' | '503' | 'passed'> = [];
    let asked = 0,
      followed = 0;
    const answer = (state: string) => ({
      result: { state, operation_id: 'synthetic' },
      attempts_left: 0,
      evidence: {},
    });
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    await page.route('**/orchestration/validations/run', (route) => {
      const body = route.request().postDataJSON() as { wait_ms?: number };
      if (!body.wait_ms) {
        asked += 1;
        return route.fulfill({ json: answer('running') });
      }
      followed += 1;
      const next = follows.shift() ?? 'drop';
      if (next === 'drop') return route.abort('failed');
      if (next === '503') return route.fulfill({ status: 503, json: { error: 'Synthetic busy server' } });
      return route.fulfill({ json: answer('passed') });
    });
    const view = page.locator('[data-orchestration-view="true"]');
    const section = page.locator('[data-orchestration-inspector] [data-orchestration-section="Validation"]');
    const run = async () => {
      await view.locator(`[data-ticket-slug="${plan.first.slug}"]`).click();
      await expect(section).toContainText('smoke (command)');
      await section.getByRole('button', { name: 'Run Validation' }).click();
    };
    const lasting = /^Validation step smoke is no longer followed: The Hot Sheet server could not be reached\.$/u;

    // A dropped request and a 503, then the answer: followed to the end with no toast.
    follows.push('drop', '503', 'passed');
    await run();
    await expect.poll(() => followed, { timeout: 30_000 }).toBe(3);
    expect(asked).toBe(1);
    expect(await toasts(lasting)).toEqual([]);

    // Every follow dropped: five tries, then exactly one toast naming the step.
    await run();
    await expect.poll(() => toasts(lasting), { timeout: 60_000 }).toHaveLength(1);
    expect(followed).toBe(3 + 5);
    expect(asked).toBe(2);
    await toastPass(page, 'hs2-cx563y', 'validation-follow-toast', lasting);
    expect(followed).toBe(8);

    // Leaving the view and coming back, Run Validation asks and follows again, to its answer.
    await page.locator('[data-segment-value="list"]').first().click();
    await expect(view).toHaveCount(0);
    await page.locator('[data-segment-value="orchestration"]').first().click();
    await expect(view).toBeVisible();
    follows.push('passed');
    await run();
    await expect.poll(() => followed, { timeout: 30_000 }).toBe(9);
    expect(asked).toBe(3);
    expect(await toasts(lasting)).toHaveLength(1);
  } finally {
    await server.stop(page);
  }
});
