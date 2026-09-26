import { expect, type Page, test } from '@playwright/test';

import type { Capabilities } from '../src/api';
import type { Project } from '../src/interactions/types';

const capabilities: Capabilities = {
  create: true,
  update: true,
  close: true,
  notes: true,
  note_edit: true,
  note_delete: true,
  attachments: true,
  assignment: true,
  review_requests: true,
  dependencies: true,
  up_next: true,
  close_reasons: true,
  claims: true,
  atomic_batch: true,
  not_working_report: true,
  offline_mutation: true,
  history: true,
  watch: true,
  provider_idempotency: true,
  query_fields: [],
};
const rootFor = (name: string) => `/work/${name}`;
const projectFor = (name: string): Project => ({
  id: name,
  name,
  root: rootFor(name),
  stores: [`/work/${name}.hs2`],
  apiPath: `/__hotsheet/project-api/${name}`,
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
});
const sessionFor = (name: string, selectedView = 'all') => ({
  selectedView,
  selectedTicketSlugs: [],
  searchOpen: true,
  searchQuery: '',
  inspectorTab: 'info',
  readerTab: 'info',
  composer: {
    open: true,
    title: `${name} preserved draft`,
    details: `${name} notes`,
    category: 'feature',
    upNext: true,
    attachments: [],
  },
  composingNote: false,
  newNoteDraft: '',
  feedbackReplies: {},
  feedbackSelections: {},
  feedbackDraft: '',
});
function gate() {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}
async function startupFixture(page: Page, roots: string[], active: string | undefined) {
  const attempts = new Map<string, number>(),
    calls: string[] = [],
    pending = new Map<string, ReturnType<typeof gate>>();
  const unavailable = new Map<string, { pid?: number; attempts?: number }>();
  const projects = new Map(['alpha', 'beta', 'gamma'].map((name) => [name, projectFor(name)]));
  const ticketGates = new Map<string, ReturnType<typeof gate>>();
  await page.addInitScript(
    ({ roots, active, sessions }) => {
      if (!localStorage.getItem('startup-test-seeded')) {
        localStorage.setItem('startup-test-seeded', 'true');
        localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
        if (active) localStorage.setItem('hotsheet.workspace.active-project-root.v1', active);
        for (const [name, session] of sessions)
          localStorage.setItem(`hotsheet.workspace.project-session.v1.${name}`, JSON.stringify(session));
      }
    },
    {
      roots: roots.map(rootFor),
      active: active ? rootFor(active) : undefined,
      sessions: ['alpha', 'beta', 'gamma'].map(
        (name) => [name, sessionFor(name, name === 'beta' ? 'backlog' : 'all')] as const,
      ),
    },
  );
  await page.routeWebSocket('**/__hotsheet/project-api/*/ws/sync', () => undefined);
  await page.route('**/__hotsheet/**', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    calls.push(`${path}${url.search}`);
    if (path === '/__hotsheet/projects/open') {
      const name = (request.postDataJSON().root as string).split('/').at(-1)!;
      const attempt = (attempts.get(name) ?? 0) + 1;
      attempts.set(name, attempt);
      await pending.get(name)?.promise;
      const failure = unavailable.get(name);
      if (failure && attempt <= (failure.attempts ?? Number.POSITIVE_INFINITY))
        return route.fulfill({
          status: 409,
          json: {
            error: `${name} server unavailable`,
            ...(failure.pid
              ? {
                  recovery: {
                    store: `${rootFor(name)}.hs2`,
                    expected: {
                      pid: failure.pid,
                      url: 'http://127.0.0.1:8787',
                      started_at: '2026-09-22T01:00:00Z',
                    },
                  },
                }
              : {}),
          },
        });
      return route.fulfill({ status: 201, json: projects.get(name) ?? projectFor(name) });
    }
    if (path === '/__hotsheet/projects/migration-jobs') return route.fulfill({ json: { job: null } });
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: rootFor('alpha') } });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git-local',
            provider: 'git',
            display_name: 'Hot Sheet git',
            locator: '/tickets',
            default: true,
            capabilities,
          },
        ],
      });
    if (path.endsWith('/tickets')) {
      const name = path.split('/')[3];
      await ticketGates.get(name)?.promise;
      const row = {
        id: `${name}-ticket`,
        native_id: `${name}-ticket`,
        connection_id: 'git-local',
        qualified_id: `git-local:${name}-ticket`,
        slug: `HS2-${name.toUpperCase()}`,
        title: `${name} restored work`,
        status: name === 'beta' ? 'backlog' : 'started',
        up_next: true,
        feedback_needed: false,
        tags: [],
        blocked_by: [],
        claim_count: 0,
      };
      return route.fulfill({
        json: {
          items: [row],
          counts: {
            total: 1,
            queued: name === 'beta' ? 0 : 1,
            backlog: name === 'beta' ? 1 : 0,
            archive: 0,
            trash: 0,
            open: 1,
            up_next: 1,
            active: 0,
            started: name === 'beta' ? 0 : 1,
            completed_today: 0,
            completion_trend: [0, 0, 0, 0, 0, 0, 0],
          },
        },
      });
    }
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: {
          branch: 'main',
          ahead: 0,
          behind: 0,
          staged: 0,
          unstaged: 0,
          untracked: 0,
          conflicted: 0,
          clean: true,
        },
      });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: {
          cursor: Number(url.searchParams.get('since') ?? 0),
          events: [],
          overflow: false,
        },
      });
    if (path.endsWith('/ai-settings')) return route.fulfill({ json: { tool: '', model: '', effort: '' } });
    if (
      /\/(permissions|connections|commands|command-runs|corrupt-tickets|views|ai-tools|sessions|terminals)$/.test(path)
    )
      return route.fulfill({ json: [] });
    return route.fulfill({ status: 404, json: { error: `No fixture for ${path}` } });
  });
  return { attempts, calls, pending, unavailable, projects, ticketGates };
}
const heavyCalls = (calls: string[]) =>
  calls.filter((path) => /\/(tickets|commands|command-runs|views)(\?|$)/.test(path));
const draft = (page: Page, name: string) =>
  page.evaluate(
    (id) =>
      JSON.parse(localStorage.getItem(`hotsheet.workspace.project-session.v1.${id}`) ?? '{}').composer.title as string,
    name,
  );

test('opens remembered projects concurrently, wires original order, and restores only the middle active project', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const fixture = await startupFixture(page, ['alpha', 'beta', 'alpha', 'gamma'], 'beta');
  for (const name of ['alpha', 'beta', 'gamma']) fixture.pending.set(name, gate());
  const tickets = gate();
  fixture.ticketGates.set('beta', tickets);
  await page.goto('/?dev-review=false');
  await expect
    .poll(() => [...fixture.attempts])
    .toEqual([
      ['alpha', 1],
      ['beta', 1],
      ['gamma', 1],
    ]);
  const restoring = page.locator('[data-component="project-restore-state"]');
  await expect(restoring).toBeVisible();
  fixture.pending.get('gamma')!.release();
  fixture.pending.get('beta')!.release();
  await expect(page.locator('[data-component="app-shell"]')).toHaveCount(0);
  fixture.pending.get('alpha')!.release();
  await expect.poll(() => heavyCalls(fixture.calls).some((path) => path.includes('/beta/'))).toBe(true);
  // Cross the normal session debounce while startup is still hidden; no stored draft may be overwritten.
  await page.waitForTimeout(850);
  await expect(draft(page, 'beta')).resolves.toBe('beta preserved draft');
  await page.screenshot({ path: '/private/tmp/hs2-v9zvw0-restoring-wide.png', fullPage: true });
  tickets.release();
  await expect(restoring).toHaveCount(0);
  const tabs = page.locator('.project-tab-bar [role="tab"]');
  await expect(tabs).toHaveText(['alpha', 'beta1', 'gamma']);
  await expect(page.getByRole('tab', { name: /^beta/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByText('beta restored work', { exact: true })).toBeVisible();
  expect(heavyCalls(fixture.calls).every((path) => path.includes('/beta/'))).toBe(true);
  await expect(draft(page, 'alpha')).resolves.toBe('alpha preserved draft');
  await expect(draft(page, 'gamma')).resolves.toBe('gamma preserved draft');
  await page.screenshot({ path: '/private/tmp/hs2-v9zvw0-restored-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'dark';
  });
  await expect(page.getByRole('combobox', { name: 'Project', exact: true })).toHaveValue('beta');
  await page.locator('[data-component="app-shell"]').evaluate(async (node) => {
    await Promise.all(
      node
        .getAnimations({ subtree: true })
        .filter((animation) => animation.effect?.getTiming().iterations !== Number.POSITIVE_INFINITY)
        .map((animation) => animation.finished.catch(() => undefined)),
    );
  });
  await page.screenshot({ path: '/private/tmp/hs2-v9zvw0-restored-mobile-dark.png', fullPage: true });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByRole('tab', { name: /^alpha/ }).click();
  await expect(page.getByText('alpha restored work', { exact: true })).toBeVisible();
  await expect(page.locator('[data-component="quick-ticket-composer"]')).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Ticket title' })).toHaveValue('alpha preserved draft');
});

test('shows the active project while other remembered projects are still opening, as in-place placeholder tabs (HS2-X74D4B, HS2-2BEJXD)', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const fixture = await startupFixture(page, ['alpha', 'beta', 'gamma'], 'beta');
  for (const name of ['alpha', 'beta', 'gamma']) fixture.pending.set(name, gate());
  await page.goto('/?dev-review=false');
  await expect.poll(() => fixture.attempts.size).toBe(3);
  const restoring = page.locator('[data-component="project-restore-state"]'),
    tabs = page.locator('.project-tab-bar [role="tab"]');
  await expect(restoring).toBeVisible();
  // Only the active project opens; the slow alpha stays pending.
  fixture.pending.get('beta')!.release();
  await expect(restoring).toHaveCount(0);
  await expect(page.getByText('beta restored work', { exact: true })).toBeVisible();
  // Still-opening projects hold their remembered positions as dormant placeholder tabs (HS2-2BEJXD).
  const tabHosts = page.locator('.project-tab-bar [data-tab-kind="project"]');
  await expect(tabHosts).toHaveCount(3);
  await expect
    .poll(() => tabHosts.evaluateAll((items) => items.map((item) => item.getAttribute('data-pending'))))
    .toEqual(['true', 'false', 'true']);
  await expect(page.getByRole('img', { name: 'Opening alpha' })).toBeVisible();
  await expect(page.getByRole('img', { name: 'Opening gamma' })).toBeVisible();
  await expect(tabHosts.nth(0).getByRole('tab')).toBeDisabled();
  await expect(page.getByRole('tab', { name: /^beta/ })).toHaveAttribute('aria-selected', 'true');
  await page.screenshot({ path: testInfo.outputPath('active-first-while-others-restore.png') });
  // Each placeholder is replaced in place as its project registers, without moving the selection.
  fixture.pending.get('gamma')!.release();
  await expect
    .poll(() => tabHosts.evaluateAll((items) => items.map((item) => item.getAttribute('data-pending'))))
    .toEqual(['true', 'false', 'false']);
  await expect(tabs.nth(2)).toHaveText('gamma');
  await page.locator('.project-tab-bar').screenshot({ path: testInfo.outputPath('pending-tab-replaced-in-place.png') });
  fixture.pending.get('alpha')!.release();
  await expect(tabs).toHaveText(['alpha', 'beta1', 'gamma']);
  await expect(page.locator('.project-tab-bar [data-pending="true"]')).toHaveCount(0);
  await expect(page.getByRole('tab', { name: /^beta/ })).toHaveAttribute('aria-selected', 'true');
  expect(heavyCalls(fixture.calls).every((path) => path.includes('/beta/'))).toBe(true);
  await page.getByRole('tab', { name: /^alpha/ }).click();
  await expect(page.getByText('alpha restored work', { exact: true })).toBeVisible();
});

test('retries failed roots in parallel and retains the original order when the active root recovers', async ({
  page,
}) => {
  const fixture = await startupFixture(page, ['alpha', 'beta', 'gamma'], 'alpha');
  fixture.unavailable.set('alpha', { attempts: 1 });
  fixture.unavailable.set('gamma', { attempts: 1 });
  await page.goto('/?dev-review=false');
  await expect(page.getByRole('tab', { name: /^alpha/ })).toHaveAttribute('aria-selected', 'true');
  expect([...fixture.attempts]).toEqual([
    ['alpha', 2],
    ['beta', 1],
    ['gamma', 2],
  ]);
  await expect(page.locator('.project-tab-bar [role="tab"]')).toHaveText(['alpha1', 'beta', 'gamma']);
  expect(heavyCalls(fixture.calls).every((path) => path.includes('/alpha/'))).toBe(true);
  await expect(page.locator('.app-error')).toHaveCount(0);
});

test('keeps each failed root and recovery identity, then recovers the selected tab without loading other roots', async ({
  page,
}) => {
  const fixture = await startupFixture(page, ['alpha', 'beta'], 'beta');
  fixture.unavailable.set('alpha', { pid: 111 });
  fixture.unavailable.set('beta', {});
  await page.goto('/?dev-review=false');
  const failure = page.locator('.project-restore-error');
  await expect(failure).toContainText('beta server unavailable');
  await expect(failure).not.toContainText('111');
  expect([...fixture.attempts]).toEqual([
    ['alpha', 2],
    ['beta', 2],
  ]);
  expect(heavyCalls(fixture.calls)).toEqual([]);
  await page.getByRole('tab', { name: 'alpha', exact: true }).click();
  await expect(failure).toContainText('111');
  await page.getByRole('tab', { name: 'beta', exact: true }).click();
  fixture.unavailable.delete('beta');
  await failure.getByRole('button', { name: 'Retry project' }).click();
  await expect(page.getByText('beta restored work', { exact: true })).toBeVisible();
  expect(heavyCalls(fixture.calls).every((path) => path.includes('/beta/'))).toBe(true);
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.open-projects'))).resolves.toBe(
    JSON.stringify(['/work/alpha', '/work/beta']),
  );
  await page.keyboard.press('Escape');
  await expect(page.locator('[data-component="quick-ticket-composer"]')).toBeHidden();
  await page.getByRole('tab', { name: 'alpha', exact: true }).click();
  await expect(failure).toContainText('111');
  await page.screenshot({ path: '/private/tmp/hs2-v9zvw0-partial-recovery-wide.png', fullPage: true });
});

test('defers inactive setup and migration prompts until their tab is activated', async ({ page }) => {
  const fixture = await startupFixture(page, ['alpha', 'beta', 'gamma'], 'beta');
  fixture.projects.set('alpha', { ...projectFor('alpha'), stores: [], needsTicketSetup: true });
  fixture.projects.set('gamma', {
    ...projectFor('gamma'),
    stores: [],
    needsHs1Migration: true,
    needsTicketSetup: true,
    hs1DatabasePath: '/work/gamma/.hotsheet/db',
    hs1PostgresVersion: '17',
  });
  await page.goto('/?dev-review=false');
  await expect(page.getByRole('tab', { name: /^beta/ })).toHaveAttribute('aria-selected', 'true');
  const setup = page.locator('[data-component="ticket-source-setup-dialog"]');
  const migration = page.locator('[data-component="hs1-migration-dialog"]');
  await expect(setup).toBeHidden();
  await expect(migration).toBeHidden();
  await page.getByRole('tab', { name: /^alpha/ }).click();
  await expect(setup).toHaveJSProperty('open', true);
  await expect(setup.getByRole('button', { name: 'Connect GitHub Issues' })).toBeVisible();
  await expect(page.locator('[data-component="quick-ticket-composer"]')).not.toHaveAttribute('open', '');
  await expect(migration).toBeHidden();
  await page.keyboard.press('Escape');
  await expect(setup).toBeHidden();
  await page.keyboard.press('Escape');
  await page.getByRole('tab', { name: /^gamma/ }).click();
  await expect(migration).toHaveJSProperty('open', true);
  await expect(migration.getByRole('button', { name: 'Not now' })).toBeVisible();
  await expect(setup).toBeHidden();
});

test('survives all-failed, empty, and refilled startup transitions without retaining an obsolete active root', async ({
  page,
}) => {
  const fixture = await startupFixture(page, ['alpha', 'beta'], 'beta');
  fixture.unavailable.set('alpha', {});
  fixture.unavailable.set('beta', {});
  await page.goto('/?dev-review=false');
  await expect(page.locator('.project-restore-error')).toContainText('beta server unavailable');
  await page.evaluate(() => {
    localStorage.setItem('hotsheet.open-projects', '[]');
  });
  await page.reload();
  await expect(page.getByRole('button', { name: 'Open project', exact: true })).toBeVisible();
  await expect(page.locator('.project-restore-error')).toHaveCount(0);
  fixture.unavailable.clear();
  await page.evaluate(() => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify(['/work/gamma', '/work/alpha']));
    localStorage.setItem('hotsheet.workspace.active-project-root.v1', '/work/gamma');
  });
  await page.reload();
  await expect(page.getByRole('tab', { name: /^gamma/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByText('gamma restored work', { exact: true })).toBeVisible();
  await expect(page.locator('.project-restore-error')).toHaveCount(0);
  expect(heavyCalls(fixture.calls).every((path) => path.includes('/gamma/'))).toBe(true);
});

test('activates a passive tab after closing the active project, then opens again after closing every project', async ({
  page,
}) => {
  test.setTimeout(60_000);
  const fixture = await startupFixture(page, ['alpha', 'beta', 'gamma'], 'beta');
  await page.goto('/?dev-review=false');
  await expect(page.getByRole('tab', { name: /^beta/ })).toHaveAttribute('aria-selected', 'true');
  await page.getByRole('tab', { name: /^beta/ }).hover();
  await page.getByRole('button', { name: 'Close beta', exact: true }).click();
  await page
    .locator('[data-component="project-close-dialog"]')
    .getByRole('button', { name: 'Close Project', exact: true })
    .click();
  await expect(page.getByRole('tab', { name: /^gamma/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByText('gamma restored work', { exact: true })).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Ticket title' })).toHaveValue('gamma preserved draft');
  await page.keyboard.press('Escape');
  await expect(page.locator('[data-component="quick-ticket-composer"]')).toBeHidden();
  for (const name of ['gamma', 'alpha']) {
    await page.getByRole('tab', { name: new RegExp(`^${name}`) }).hover();
    await page.getByRole('button', { name: `Close ${name}`, exact: true }).click();
    await page
      .locator('[data-component="project-close-dialog"]')
      .getByRole('button', { name: 'Close Project', exact: true })
      .click();
    await expect(page.getByRole('tab', { name: new RegExp(`^${name}`) })).toHaveCount(0);
    if (name === 'gamma') {
      await expect(page.getByRole('textbox', { name: 'Ticket title' })).toHaveValue('alpha preserved draft');
      await page.keyboard.press('Escape');
      await expect(page.locator('[data-component="quick-ticket-composer"]')).toBeHidden();
    }
  }
  await expect(page.getByRole('button', { name: 'Open project', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Open project', exact: true }).click();
  await page.locator('[data-project-dialog]').getByRole('textbox', { name: 'Project folder' }).fill('/work/alpha');
  await page.locator('[data-project-dialog]').getByRole('button', { name: 'Open project', exact: true }).click();
  await expect(page.getByRole('tab', { name: /^alpha/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByText('alpha restored work', { exact: true })).toBeVisible();
  expect(fixture.attempts.get('alpha')).toBe(2);
});

test('restores inactive AI chats without activating their project and loads aggregate counts only on demand', async ({
  page,
}) => {
  const fixture = await startupFixture(page, ['alpha', 'beta', 'gamma'], 'beta');
  await page.addInitScript(() => {
    localStorage.setItem('hotsheet.terminals.drawer-open', 'true');
  });
  await page.route('**/__hotsheet/project-api/alpha/connections', (route) =>
    route.fulfill({
      json: [
        {
          id: 'hotsheet-drawer-chat-alpha',
          tool: 'codex',
          project: '/work/alpha',
          role: 'main',
          busy: false,
          actions: ['send_turn', 'interrupt'],
          session_id: 'alpha-session',
        },
      ],
    }),
  );
  await page.goto('/?dev-review=false');
  await expect(page.getByRole('tab', { name: /^beta/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-component="terminal-drawer"]')).toBeVisible();
  expect(heavyCalls(fixture.calls).every((path) => path.includes('/beta/'))).toBe(true);
  expect(fixture.calls.some((path) => /\/(alpha|gamma)\/ai-tools/.test(path))).toBe(false);
  await expect(draft(page, 'alpha')).resolves.toBe('alpha preserved draft');
  await page.getByRole('button', { name: 'Workspace grid', exact: true }).click();
  await expect(page.locator('[data-component="workspace-chat-tile"]')).toContainText('alpha › Codex chat');
  await expect
    .poll(() => heavyCalls(fixture.calls).filter((path) => /\/(alpha|gamma)\/.*\/tickets\?/.test(path)).length)
    .toBe(2);
  expect(fixture.calls.some((path) => /\/(alpha|gamma)\/(commands|command-runs|views)(\?|$)/.test(path))).toBe(false);
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.workspace.active-project-root.v1'))).resolves.toBe(
    '/work/beta',
  );
  await expect(draft(page, 'alpha')).resolves.toBe('alpha preserved draft');
  await expect(
    page
      .locator('.terminal-operations-sidebar__group[data-project-id="alpha"]')
      .getByRole('button', { name: /1 in progress/ }),
  ).toBeVisible();
});
