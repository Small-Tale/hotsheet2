import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

import { expect, type Page, test } from '@playwright/test';

import { proxyFetch, realTicketServer, routeProjectChangeSocket } from './real-ticket-server';

const execute = promisify(execFile);

/** How long the idle window measures, after the page has settled. */
const idleObservationMs = 8_000;

/**
 * HS2-GFZYCZ: an idle Hot Sheet web app with live claims held WebKit's WebContent process at 60-98%
 * CPU (the GPU process and WindowServer high too). The project tab's activity ring and the live-claim
 * LoadingSpinner each rotated a shape inside an SVG, which WebKit cannot composite, so every frame was
 * repainted for as long as a claim was live. This drives the shipped app against real servers with
 * live claims in two projects, lets it settle, and proves the idle window costs nothing per frame:
 * no running animation targets an SVG shape, no requestAnimationFrame callback runs, the DOM stays
 * still apart from the minute-granular ETA countdown, and no request reaches the servers.
 *
 * HS2-Y913AE: Kerf beta.139 turns LoadingSpinner's whole svg box with `transform` (KF-M4WWRW), so live
 * claims show the yellow spinner again. In Chromium the idle window also bounds style recalculation:
 * Kerf's old `rotate` animation ran on the main thread at about 60 recalcs a second.
 */
async function seedClaimedTickets(fixture: { store: string; root: string; env: NodeJS.ProcessEnv; cli: string }) {
  const cli = (args: string[]) => execute(fixture.cli, args, { cwd: fixture.root, env: fixture.env });
  for (let index = 1; index <= 6; index += 1) {
    const { stdout } = await cli(['new', '-C', fixture.store, `Idle ticket ${index}`, '--up-next']);
    const slug = /\b(HS2-[A-Z0-9]+)\b/.exec(stdout)?.[1];
    if (!slug) throw new Error(`No slug in: ${stdout}`);
    // Two live claims: one with an on-track ETA (determinate ring), one without (indeterminate).
    if (index <= 2)
      await cli([
        'claim',
        '-C',
        fixture.store,
        slug,
        '--worker',
        `idle-worker-${index}`,
        '--lease-minutes',
        '120',
        ...(index === 1 ? ['--eta', '1h'] : []),
      ]);
  }
}

type Server = Awaited<ReturnType<typeof realTicketServer>>;

async function openRealProjects(page: Page, projects: Array<{ id: string; name: string; server: Server }>) {
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, async (route) => {
    const root = (route.request().postDataJSON() as { root: string }).root;
    const project = projects.find((item) => item.server.root === root);
    if (!project) return route.fallback();
    return route.fulfill({
      status: 201,
      json: {
        id: project.id,
        root: project.server.root,
        name: project.name,
        stores: [project.server.store],
        apiPath: `/__hotsheet/project-api/${project.id}`,
      },
    });
  });
  for (const { id, server } of projects) {
    await page.route(`**/__hotsheet/project-api/${id}/**`, async (route) => {
      const url = new URL(route.request().url());
      const path = url.pathname
        .replace(`/__hotsheet/project-api/${id}`, '')
        .replace(`/checkouts/${id}`, `/checkouts/${server.checkoutId}`);
      const response = await proxyFetch(route, {
        url: `${server.url}${path}${url.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
      });
      await route.fulfill({ response });
    });
    await routeProjectChangeSocket(page, server, id);
  }
  await page.addInitScript(
    (roots) => {
      localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
    },
    projects.map((project) => project.server.root),
  );
}

/** Count frames, DOM mutations and running animations from before the app's first script. */
async function instrumentIdleCost(page: Page) {
  await page.addInitScript(() => {
    const state = { frames: 0, mutations: [] as string[], lastMutationAt: 0 };
    (window as unknown as { hs2IdleCost: typeof state }).hs2IdleCost = state;
    const request = window.requestAnimationFrame.bind(window);
    window.requestAnimationFrame = (callback) =>
      request((time) => {
        state.frames += 1;
        callback(time);
      });
    const describe = (node: Node) => {
      const element = node instanceof Element ? node : node.parentElement;
      const owner = element?.closest('[data-component]')?.getAttribute('data-component') ?? '';
      return `${owner}>${element?.tagName.toLowerCase() ?? '?'}`;
    };
    new MutationObserver((records) => {
      state.lastMutationAt = performance.now();
      for (const record of records)
        state.mutations.push(`${record.type}:${record.attributeName ?? ''}:${describe(record.target)}`);
    }).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
  });
}

/** Every running infinite animation, with whether its target is a shape inside an SVG. */
function runningAnimations(page: Page) {
  return page.evaluate(() => {
    const roots: Array<Document | ShadowRoot> = [document];
    for (const element of document.querySelectorAll('*')) if (element.shadowRoot) roots.push(element.shadowRoot);
    const seen = new Set<Animation>();
    return roots.flatMap((root) =>
      root
        .getAnimations()
        .filter((animation) => {
          if (seen.has(animation) || animation.playState !== 'running') return false;
          seen.add(animation);
          return animation.effect?.getTiming().iterations === Infinity;
        })
        .map((animation) => {
          const target = (animation.effect as KeyframeEffect | null)?.target;
          return {
            name: (animation as CSSAnimation).animationName,
            target: target?.tagName.toLowerCase() ?? '?',
            component: target?.closest('[data-component]')?.getAttribute('data-component') ?? '',
            svgShape: target instanceof SVGElement && !(target instanceof SVGSVGElement),
          };
        }),
    );
  });
}

test('keeps an idle app with live claims free of per-frame work and requests (HS2-GFZYCZ)', async ({
  page,
  browserName,
}) => {
  test.setTimeout(120_000);
  const alpha = await realTicketServer({ seed: seedClaimedTickets });
  const bravo = await realTicketServer({ seed: seedClaimedTickets });
  try {
    await instrumentIdleCost(page);
    await openRealProjects(page, [
      { id: 'idle-alpha', name: 'Idle alpha', server: alpha },
      { id: 'idle-bravo', name: 'Idle bravo', server: bravo },
    ]);
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    const rows = page.locator('[data-component="summary-row"][data-ticket-slug]');
    await expect(rows).toHaveCount(6, { timeout: 30_000 });
    // Both live-work symbols and the selected project's activity ring are on screen.
    const claimed = rows.filter({ has: page.locator('.ticket-list-row__claim') });
    await expect(claimed).toHaveCount(2);
    await expect(claimed.locator('[aria-valuenow]')).toHaveCount(1);
    await expect(page.locator('[data-tab-kind="project"] .project-tab__activity-ring').first()).toBeVisible();
    // Open the inspector on the claimed ticket without an ETA, as a user leaves it.
    await claimed.filter({ hasNot: page.locator('[aria-valuenow]') }).click();
    await expect(page.locator('#app-right-rail [data-component="live-claim-notice"]')).toBeVisible();
    // Settled: no DOM mutation for a second and a half.
    await expect
      .poll(
        () =>
          page.evaluate(() => {
            const state = (window as unknown as { hs2IdleCost: { lastMutationAt: number } }).hs2IdleCost;
            return performance.now() - state.lastMutationAt;
          }),
        { timeout: 30_000 },
      )
      .toBeGreaterThan(1_500);

    const requests: string[] = [];
    page.on('request', (request) => {
      const { pathname } = new URL(request.url());
      if (pathname.startsWith('/__hotsheet/')) requests.push(pathname);
    });
    await page.evaluate(() => {
      const state = (window as unknown as { hs2IdleCost: { frames: number; mutations: string[] } }).hs2IdleCost;
      state.frames = 0;
      state.mutations = [];
    });
    // Chromium reports style recalculations through CDP; WebKit's cost is bounded by the checks below.
    const cdp = browserName === 'chromium' ? await page.context().newCDPSession(page) : undefined;
    const recalcStyleCount = async () => {
      if (!cdp) return 0;
      const { metrics } = await cdp.send('Performance.getMetrics');
      return metrics.find((metric) => metric.name === 'RecalcStyleCount')?.value ?? 0;
    };
    await cdp?.send('Performance.enable');
    const recalcBefore = await recalcStyleCount();
    await page.waitForTimeout(idleObservationMs);
    const recalcs = (await recalcStyleCount()) - recalcBefore;
    const idle = await page.evaluate(
      () => (window as unknown as { hs2IdleCost: { frames: number; mutations: string[] } }).hs2IdleCost,
    );
    const animations = await runningAnimations(page);

    // WebKit composites a spinning <svg> box; it repaints a spinning SVG shape every frame.
    expect(animations.filter((animation) => animation.svgShape)).toEqual([]);
    // The activity ring and the yellow live-claim spinner still turn, so the check above is not vacuous.
    expect(animations.map((animation) => animation.name)).toEqual(
      expect.arrayContaining(['project-tab-activity-rotate', 'kui-loading-spinner-rotate']),
    );
    expect(
      animations.filter((animation) => animation.name === 'kui-loading-spinner-rotate').map(({ target }) => target),
    ).toEqual(['svg', 'svg']);
    // Composited turns need no style work: Kerf's old main-thread `rotate` cost about 480 recalcs here
    // (HS2-Y913AE, KF-M4WWRW). The countdown and caret leave a handful.
    expect(recalcs).toBeLessThanOrEqual(idleObservationMs / 1_000);
    expect(idle.frames).toBe(0);
    // Only the minute-granular ETA countdown may touch the DOM, and at most once in the window.
    expect(idle.mutations.filter((mutation) => !/progress-ring|text>|data-date-prefix/.test(mutation))).toEqual([]);
    expect(idle.mutations.length).toBeLessThanOrEqual(8);
    // The change stream is a socket: an idle window issues no HTTP request at all.
    expect(requests).toEqual([]);
    await page.screenshot({ path: 'target/visual-captures/hs2-gfzycz-idle-live-claims.png' });
  } finally {
    await alpha.stop(page);
    await bravo.stop(page);
  }
});
