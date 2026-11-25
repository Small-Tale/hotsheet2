import { expect, type Page, test, type WebSocketRoute } from '@playwright/test';

import type { Capabilities } from '../src/api';
import type { Project } from '../src/interactions/types';
import { remoteProjectId } from '../src/remote-projects/identity';
import { projectTab } from './open-mocked-project';

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
async function startupFixture(page: Page, roots: string[], active: string | undefined, composerOpen = true) {
  const attempts = new Map<string, number>(),
    calls: string[] = [],
    openBodies: Record<string, unknown>[] = [],
    pending = new Map<string, ReturnType<typeof gate>>();
  const unavailable = new Map<string, { pid?: number; attempts?: number }>();
  const projects = new Map(['alpha', 'beta', 'gamma'].map((name) => [name, projectFor(name)]));
  const ticketGates = new Map<string, ReturnType<typeof gate>>();
  const fullTicketGates = new Map<string, ReturnType<typeof gate>>();
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
      sessions: ['alpha', 'beta', 'gamma'].map((name) => {
        const session = sessionFor(name, name === 'beta' ? 'backlog' : 'all');
        session.composer.open = composerOpen;
        return [name, session] as const;
      }),
    },
  );
  await page.routeWebSocket(/\/__hotsheet\/project-api\/[^/]+\/ws\/sync(?:\?.*)?$/, () => undefined);
  await page.route('**/__hotsheet/**', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    calls.push(`${path}${url.search}`);
    if (/^\/__hotsheet\/(?:remote\/)?projects\/open$/.test(path)) {
      const body = request.postDataJSON() as Record<string, unknown>;
      openBodies.push(body);
      const name = (body.root as string).split('/').at(-1)!;
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
            display_name: 'Hot Sheet Git',
            locator: '/tickets',
            default: true,
            capabilities,
          },
          ...(fullTicketGates.has(path.split('/')[3])
            ? [
                {
                  connection_id: 'github-main',
                  provider: 'github',
                  display_name: 'GitHub',
                  locator: 'acme/repo',
                  default: false,
                  capabilities,
                },
              ]
            : []),
        ],
      });
    if (path.endsWith('/tickets')) {
      const name = path.split('/')[3];
      const remoteFixture = decodeURIComponent(name).startsWith('remote:');
      await ticketGates.get(name)?.promise;
      if (url.searchParams.get('source') !== 'git') await fullTicketGates.get(name)?.promise;
      const row = {
        id: `${name}-ticket`,
        native_id: `${name}-ticket`,
        connection_id: 'git-local',
        qualified_id: `git-local:${name}-ticket`,
        slug: remoteFixture ? 'HS2-DEMO' : `HS2-${name.toUpperCase()}`,
        title: remoteFixture ? 'Workspace restored work' : `${name} restored work`,
        status: name === 'beta' ? 'backlog' : 'started',
        up_next: true,
        feedback_needed: false,
        tags: [],
        blocked_by: [],
        claim_count: 0,
      };
      return route.fulfill({
        json: {
          items: [
            row,
            ...(fullTicketGates.has(name) && url.searchParams.get('source') !== 'git'
              ? [
                  {
                    ...row,
                    id: `${name}-remote`,
                    native_id: '42',
                    connection_id: 'github-main',
                    qualified_id: 'github-main:42',
                    slug: 'GH-42',
                    title: `${name} remote work`,
                  },
                ]
              : []),
          ],
          counts: {
            total: fullTicketGates.has(name) && url.searchParams.get('source') !== 'git' ? 2 : 1,
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
  return { attempts, calls, openBodies, pending, unavailable, projects, ticketGates, fullTicketGates };
}
const heavyCalls = (calls: string[]) =>
  calls.filter((path) => /\/(tickets|commands|command-runs|views)(\?|$)/.test(path));
const draft = (page: Page, name: string) =>
  page.evaluate(
    (id) =>
      JSON.parse(localStorage.getItem(`hotsheet.workspace.project-session.v1.${id}`) ?? '{}').composer.title as string,
    name,
  );

test('desktop source choice keeps two same-id remote projects in distinct tabs (HS2-BT4KN6)', async ({ page }) => {
  const first = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
  const second = '01ARZ3NDEKTSV4RRFFQ69G5FAW';
  const remotes = [
    { id: first, label: 'Studio', origin: 'http://studio.test:8787' },
    { id: second, label: 'Laptop', origin: 'https://laptop.test:8787' },
  ].map((item) => ({
    ...item,
    identity: { server_id: item.id, ca_fingerprint: null },
    revision: 1,
    revoked: false,
    last_failure: null as string | null,
    stale: false,
  }));
  await startupFixture(page, ['alpha'], 'alpha', false);
  await page.route('**/__hotsheet/remotes**', async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/__hotsheet/remotes') return route.fulfill({ json: remotes });
    if (path === '/__hotsheet/remotes/selected-projects') return route.fulfill({ json: [] });
    const match = /^\/__hotsheet\/remotes\/([^/]+)\/(?:checkouts|projects\/([^/]+)\/open)$/.exec(path);
    if (!match) return route.fulfill({ status: 404, json: { error: 'Unknown synthetic remote route' } });
    const server = remotes.find((item) => item.id === match[1]);
    if (!server) return route.fulfill({ status: 404, json: { error: 'Unknown synthetic server' } });
    if (!match[2])
      return route.fulfill({
        json: { remote: server, checkouts: [{ id: 'same', name: 'Workspace', state: 'available', sources: [] }] },
      });
    const id = remoteProjectId(server.id, 'same');
    return route.fulfill({
      status: 201,
      json: {
        id,
        root: id,
        name: `${server.label} · Workspace`,
        stores: [],
        apiPath: `/__hotsheet/project-api/${encodeURIComponent(id)}`,
        compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
        needsTicketSetup: false,
        needsHs1Migration: false,
        hs1ImportCompleted: false,
        hs1CleanupEligible: false,
        hs1RegistrationRepairAvailable: false,
      },
    });
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await expect(page.getByRole('tab', { name: /^alpha/ })).toBeVisible();
  const add = () => page.getByRole('button', { name: 'Add Project' });
  await add().click();
  const choice = page.locator('[data-project-source-choice]');
  await expect(choice).toHaveJSProperty('open', true);
  await page.screenshot({ path: 'target/visual-captures/hs2-bt4kn6-choice-wide.png', animations: 'disabled' });
  for (const [name, width, height] of [
    ['medium', 900, 700],
    ['narrow', 390, 844],
  ] as const) {
    await page.setViewportSize({ width, height });
    await expect(choice.getByRole('button', { name: 'Local Project' })).toBeInViewport();
    await expect(choice.getByRole('button', { name: 'Remote Project' })).toBeInViewport();
    await page.screenshot({ path: `target/visual-captures/hs2-bt4kn6-choice-${name}.png`, animations: 'disabled' });
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(choice).toHaveJSProperty('open', true);
  await page.screenshot({ path: 'target/visual-captures/hs2-bt4kn6-choice-returned-wide.png', animations: 'disabled' });
  await choice.getByRole('button', { name: 'Remote Project' }).click();
  const dialog = page.locator('[data-desktop-remote-dialog]');
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog.getByText('http://studio.test:8787')).toBeVisible();
  await page.setViewportSize({ width: 900, height: 700 });
  await dialog.getByRole('button', { name: 'Add Server' }).click();
  await expect(dialog.getByRole('textbox', { name: 'Server Address' })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-bt4kn6-address-form-medium.png', animations: 'disabled' });
  await dialog.getByRole('button', { name: 'Back to Servers' }).click();
  await page.screenshot({ path: 'target/visual-captures/hs2-bt4kn6-address-medium.png', animations: 'disabled' });
  await dialog.getByRole('button', { name: 'Studio', exact: true }).click();
  await expect(dialog.getByRole('button', { name: 'Workspace' })).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: 'target/visual-captures/hs2-bt4kn6-projects-phone.png', animations: 'disabled' });
  await dialog.getByRole('button', { name: 'Workspace' }).click();
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(page.getByRole('tab', { name: /Studio · Workspace/ })).toBeVisible();
  await add().click();
  await choice.getByRole('button', { name: 'Remote Project' }).click();
  await dialog.getByRole('button', { name: 'Laptop', exact: true }).click();
  await dialog.getByRole('button', { name: 'Workspace' }).click();
  await expect(page.getByRole('tab', { name: /Studio · Workspace/ })).toBeVisible();
  await expect(page.getByRole('tab', { name: /Laptop · Workspace/ })).toBeVisible();
  await page.screenshot({
    path: 'target/visual-captures/hs2-bt4kn6-two-hosts-returned-wide.png',
    animations: 'disabled',
  });
  remotes[0].last_failure = 'Synthetic host unavailable';
  await add().click();
  await choice.getByRole('button', { name: 'Remote Project' }).click();
  await expect(dialog.getByRole('button', { name: 'Retry Studio' })).toBeVisible();
  await dialog.locator('[data-action="cancel-desktop-remote-dialog"]').click();
  await expect(dialog).toHaveJSProperty('open', false);
  await expect(choice).toHaveJSProperty('open', false);
  await expect(page.locator(`[data-project-id="${remoteProjectId(first, 'same')}"]`)).toHaveAttribute(
    'data-disconnected',
    'true',
  );
  await expect(page.locator(`[data-project-id="${remoteProjectId(second, 'same')}"]`)).toHaveAttribute(
    'data-disconnected',
    'false',
  );
  await expect(
    page.locator(`[data-project-id="${remoteProjectId(first, 'same')}"] [data-lucide="wifi-off"]`),
  ).toBeVisible();
  await expect(
    page.locator(`[data-project-id="${remoteProjectId(second, 'same')}"] [data-lucide="wifi-off"]`),
  ).toHaveCount(0);
  await page.screenshot({
    path: 'target/visual-captures/hs2-bt4kn6-one-host-disconnected-wide.png',
    animations: 'disabled',
  });
});

test('shows local tickets while a mixed-source page waits for credential access', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const fixture = await startupFixture(page, ['alpha'], 'alpha', false);
  const full = gate();
  fixture.fullTicketGates.set('alpha', full);
  await page.goto('/');
  await expect(page.getByText('alpha restored work', { exact: true })).toBeVisible();
  await expect(page.getByText('alpha remote work', { exact: true })).toHaveCount(0);
  expect(fixture.calls.some((path) => path.includes('/tickets?') && path.includes('source=git'))).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('local-while-credential-pending-wide.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByText('alpha restored work', { exact: true })).toBeVisible();
  await page.waitForTimeout(350);
  await page.screenshot({ path: testInfo.outputPath('local-while-credential-pending-phone.png') });
  full.release();
  await expect(page.getByText('alpha remote work', { exact: true })).toBeVisible();
  await expect(page.getByText('alpha restored work', { exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('both-sources-after-credential-phone.png') });
});

test('does not replay a saved view over a choice made during credential wait', async ({ page }) => {
  const fixture = await startupFixture(page, ['alpha'], 'alpha', false);
  const full = gate();
  fixture.fullTicketGates.set('alpha', full);
  await page.goto('/');
  await expect(page.getByText('alpha restored work', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: /Backlog/ }).click();
  await expect(page.locator('.kui-toolbar-text', { hasText: 'Backlog' })).toBeVisible();
  const fullPage = page.waitForResponse((response) => {
    const url = new URL(response.url());
    return url.pathname.endsWith('/tickets') && !url.searchParams.has('source');
  });
  full.release();
  await fullPage;
  await page.waitForTimeout(300);
  await expect(page.locator('.kui-toolbar-text', { hasText: 'Backlog' })).toBeVisible();
});

test('opens remembered projects concurrently, wires original order, and restores only the middle active project', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const fixture = await startupFixture(page, ['alpha', 'beta', 'alpha', 'gamma'], 'beta');
  for (const name of ['alpha', 'beta', 'gamma']) fixture.pending.set(name, gate());
  const tickets = gate();
  fixture.ticketGates.set('beta', tickets);
  await page.goto('/');
  await expect
    .poll(() => [...fixture.attempts])
    .toEqual([
      ['alpha', 1],
      ['beta', 1],
      ['gamma', 1],
    ]);
  const restoring = page.locator('[data-project-restore-state="true"]');
  await expect(restoring).toBeVisible();
  fixture.pending.get('gamma')!.release();
  fixture.pending.get('beta')!.release();
  await expect(page.locator('[data-component="app-shell"]')).toHaveCount(0);
  fixture.pending.get('alpha')!.release();
  await expect.poll(() => heavyCalls(fixture.calls).some((path) => path.includes('/beta/'))).toBe(true);
  // Cross the normal session debounce while startup is still hidden; no stored draft may be overwritten.
  await page.waitForTimeout(850);
  await expect(draft(page, 'beta')).resolves.toBe('beta preserved draft');
  await page.screenshot({ path: 'target/visual-captures/hs2-v9zvw0-restoring-wide.png', fullPage: true });
  tickets.release();
  await expect(restoring).toHaveCount(0);
  const tabs = page.locator('.project-tab-bar [role="tab"]');
  await expect(tabs).toHaveText(['alpha', 'beta1', 'gamma']);
  await expect(page.getByRole('tab', { name: /^beta/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByText('beta restored work', { exact: true })).toBeVisible();
  expect(heavyCalls(fixture.calls).every((path) => path.includes('/beta/'))).toBe(true);
  await expect(draft(page, 'alpha')).resolves.toBe('alpha preserved draft');
  await expect(draft(page, 'gamma')).resolves.toBe('gamma preserved draft');
  await page.screenshot({ path: 'target/visual-captures/hs2-v9zvw0-restored-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'dark';
  });
  await expect(page.getByRole('combobox', { name: 'Project', exact: true })).toContainText('beta');
  await page.locator('[data-component="app-shell"]').evaluate(async (node) => {
    await Promise.all(
      node
        .getAnimations({ subtree: true })
        .filter((animation) => animation.effect?.getTiming().iterations !== Number.POSITIVE_INFINITY)
        .map((animation) => animation.finished.catch(() => undefined)),
    );
  });
  await page.screenshot({ path: 'target/visual-captures/hs2-v9zvw0-restored-mobile-dark.png', fullPage: true });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.getByRole('tab', { name: /^alpha/ }).click();
  await expect(page.getByText('alpha restored work', { exact: true })).toBeVisible();
  await expect(page.locator('[data-component="quick-ticket-composer"]')).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Ticket Title' })).toHaveValue('alpha preserved draft');
});

test('shows the active project while other remembered projects are still opening, as in-place placeholder tabs (HS2-X74D4B, HS2-2BEJXD)', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const fixture = await startupFixture(page, ['alpha', 'beta', 'gamma'], 'beta');
  for (const name of ['alpha', 'beta', 'gamma']) fixture.pending.set(name, gate());
  await page.goto('/');
  await expect.poll(() => fixture.attempts.size).toBe(3);
  const restoring = page.locator('[data-project-restore-state="true"]'),
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
    .poll(() => tabHosts.evaluateAll((items) => items.map((item) => item.getAttribute('data-project-pending'))))
    .toEqual(['true', 'false', 'true']);
  // Kerf's named pending TabBarItem keeps the name as the visible and accessible name, busy, with close
  // and drag off (HS2-XGSSK1).
  const alphaPending = tabHosts.nth(0);
  await expect(alphaPending.getByRole('tab', { name: 'alpha', exact: true })).toBeEnabled();
  await expect(alphaPending.getByRole('tab', { name: 'gamma', exact: true })).toHaveCount(0);
  await expect(tabHosts.nth(2).getByRole('tab', { name: 'gamma', exact: true })).toBeVisible();
  await expect(alphaPending).toHaveAttribute('aria-busy', 'true');
  await expect(alphaPending).toHaveAttribute('draggable', 'false');
  await expect(alphaPending.getByRole('button', { name: 'Close alpha' })).toHaveCount(0);
  await expect(page.getByRole('tab', { name: /^beta/ })).toHaveAttribute('aria-selected', 'true');
  await page.screenshot({ path: testInfo.outputPath('active-first-while-others-restore.png') });
  await page.locator('.project-tab-bar').screenshot({ path: 'target/visual-captures/hs2-xgssk1-pending-tabs.png' });
  // Each placeholder is replaced in place as its project registers, without moving the selection.
  fixture.pending.get('gamma')!.release();
  await expect
    .poll(() => tabHosts.evaluateAll((items) => items.map((item) => item.getAttribute('data-project-pending'))))
    .toEqual(['true', 'false', 'false']);
  await expect(tabs.nth(2)).toHaveText('gamma');
  await page.locator('.project-tab-bar').screenshot({ path: testInfo.outputPath('pending-tab-replaced-in-place.png') });
  expect(heavyCalls(fixture.calls).every((path) => path.includes('/beta/'))).toBe(true);
  // Selecting the still-opening alpha shows its opening panel, then the project once it is ready (HS2-XGSSK1).
  await projectTab(page, 'alpha').click();
  const opening = page.locator('[data-project-opening-state="true"]');
  await expect(opening).toContainText('Opening alpha…');
  await expect(projectTab(page, 'alpha')).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('tab', { name: /^beta/ })).toHaveAttribute('aria-selected', 'false');
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.screenshot({ path: 'target/visual-captures/hs2-xgssk1-opening-panel-1280.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator('.project-tab-bar--mobile')).toBeVisible();
  await expect(opening).toBeVisible();
  await expect(page.locator('kui-select[name="mobile-project"]')).toHaveJSProperty(
    'value',
    `pending:${rootFor('alpha')}`,
  );
  await page.screenshot({ path: 'target/visual-captures/hs2-xgssk1-opening-panel-390.png' });
  await page.setViewportSize({ width: 1440, height: 900 });
  // Returning to beta and back keeps the opening panel tied to the pending tab.
  await page.getByRole('tab', { name: /^beta/ }).click();
  await expect(opening).toHaveCount(0);
  await expect(page.getByText('beta restored work', { exact: true })).toBeVisible();
  await projectTab(page, 'alpha').click();
  await expect(opening).toBeVisible();
  fixture.pending.get('alpha')!.release();
  await expect(tabs).toHaveText(['alpha', 'beta1', 'gamma']);
  await expect(page.locator('.project-tab-bar [data-project-pending="true"]')).toHaveCount(0);
  await expect(opening).toHaveCount(0);
  await expect(page.getByText('alpha restored work', { exact: true })).toBeVisible();
  await expect(page.getByRole('tab', { name: /^alpha/ })).toHaveAttribute('aria-selected', 'true');
});

test('retries failed roots in parallel and retains the original order when the active root recovers', async ({
  page,
}) => {
  const fixture = await startupFixture(page, ['alpha', 'beta', 'gamma'], 'alpha');
  fixture.unavailable.set('alpha', { attempts: 1 });
  fixture.unavailable.set('gamma', { attempts: 1 });
  await page.goto('/');
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
  await page.goto('/');
  const failure = page.locator('.project-restore-error');
  await expect(failure).toContainText('beta server unavailable');
  await expect(failure).not.toContainText('111');
  expect([...fixture.attempts]).toEqual([
    ['alpha', 2],
    ['beta', 2],
  ]);
  expect(heavyCalls(fixture.calls)).toEqual([]);
  await projectTab(page, 'alpha').click();
  await expect(failure).toContainText('111');
  await projectTab(page, 'beta').click();
  await expect(failure).toContainText('beta server unavailable');
  // The failure panel uses the phone shell like every other branch (HS2-RSX13Q).
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(page.locator('.project-tab-bar--mobile')).toHaveCount(0);
  await page.screenshot({ path: 'target/visual-captures/hs2-rsx13q-restore-failure-1280.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.locator('.project-tab-bar--mobile')).toBeVisible();
  await expect(projectTab(page, 'beta')).toBeHidden();
  await expect(failure).toBeVisible();
  await expect(page.locator('kui-select[name="mobile-project"]')).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-rsx13q-restore-failure-390.png' });
  await page.setViewportSize({ width: 1280, height: 720 });
  fixture.unavailable.delete('beta');
  await failure.getByRole('button', { name: 'Retry project' }).click();
  await expect(page.getByText('beta restored work', { exact: true })).toBeVisible();
  expect(heavyCalls(fixture.calls).every((path) => path.includes('/beta/'))).toBe(true);
  await expect(page.evaluate(() => localStorage.getItem('hotsheet.open-projects'))).resolves.toBe(
    JSON.stringify(['/work/alpha', '/work/beta']),
  );
  await page.keyboard.press('Escape');
  await expect(page.locator('[data-component="quick-ticket-composer"]')).toBeHidden();
  await projectTab(page, 'alpha').click();
  await expect(failure).toContainText('111');
  await page.screenshot({ path: 'target/visual-captures/hs2-v9zvw0-partial-recovery-wide.png', fullPage: true });
});

test('defers inactive setup and migration prompts until their tab is activated', async ({ page }) => {
  const fixture = await startupFixture(page, ['alpha', 'beta', 'gamma'], 'beta');
  const alphaOpening = gate();
  fixture.pending.set('alpha', alphaOpening);
  fixture.projects.set('alpha', { ...projectFor('alpha'), stores: [], needsTicketSetup: true });
  fixture.projects.set('gamma', {
    ...projectFor('gamma'),
    stores: [],
    needsHs1Migration: true,
    needsTicketSetup: true,
    hs1DatabasePath: '/work/gamma/.hotsheet/db',
    hs1PostgresVersion: '17',
  });
  await page.goto('/');
  await expect(page.getByRole('tab', { name: /^beta/ })).toHaveAttribute('aria-selected', 'true');
  const setup = page.locator('[data-component="ticket-source-setup-dialog"]');
  const migration = page.locator('[data-component="hs1-migration-dialog"]');
  const pendingAlpha = page.locator('[data-tab-kind="project"][data-project-id="pending:/work/alpha"]');
  const loadedTab = (id: string) => page.locator(`[data-tab-kind="project"][data-project-id="${id}"]`).getByRole('tab');
  await expect(pendingAlpha).toBeVisible();
  await expect(setup).toBeHidden();
  await expect(migration).toBeHidden();
  alphaOpening.release();
  await expect(pendingAlpha).toHaveCount(0);
  await loadedTab('alpha').click();
  await expect(setup).toHaveJSProperty('open', true);
  await expect(setup.getByRole('button', { name: 'Connect GitHub Issues' })).toBeVisible();
  await expect(page.locator('[data-component="quick-ticket-composer"]')).not.toHaveAttribute('open', '');
  await expect(migration).toBeHidden();
  await page.keyboard.press('Escape');
  await expect(setup).toBeHidden();
  await page.keyboard.press('Escape');
  await loadedTab('gamma').click();
  await expect(migration).toHaveJSProperty('open', true);
  await expect(migration.getByRole('button', { name: 'Not Now' })).toBeVisible();
  await expect(setup).toBeHidden();
});

test('survives all-failed, empty, and refilled startup transitions without retaining an obsolete active root', async ({
  page,
}) => {
  const fixture = await startupFixture(page, ['alpha', 'beta'], 'beta');
  fixture.unavailable.set('alpha', {});
  fixture.unavailable.set('beta', {});
  await page.goto('/');
  await expect(page.locator('.project-restore-error')).toContainText('beta server unavailable');
  await page.evaluate(() => {
    localStorage.setItem('hotsheet.open-projects', '[]');
  });
  await page.reload();
  await expect(page.getByRole('button', { name: 'Open Project', exact: true })).toBeVisible();
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
  await page.goto('/');
  await expect(page.getByRole('tab', { name: /^beta/ })).toHaveAttribute('aria-selected', 'true');
  await page.getByRole('tab', { name: /^beta/ }).hover();
  await page.getByRole('button', { name: 'Close beta', exact: true }).click();
  await page
    .locator('[data-dialog-component="project-close-dialog"]')
    .getByRole('button', { name: 'Close Project', exact: true })
    .click();
  await expect(page.getByRole('tab', { name: /^gamma/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByText('gamma restored work', { exact: true })).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Ticket Title' })).toHaveValue('gamma preserved draft');
  await page.keyboard.press('Escape');
  await expect(page.locator('[data-component="quick-ticket-composer"]')).toBeHidden();
  for (const name of ['gamma', 'alpha']) {
    await page.getByRole('tab', { name: new RegExp(`^${name}`) }).hover();
    await page.getByRole('button', { name: `Close ${name}`, exact: true }).click();
    await page
      .locator('[data-dialog-component="project-close-dialog"]')
      .getByRole('button', { name: 'Close Project', exact: true })
      .click();
    await expect(page.getByRole('tab', { name: new RegExp(`^${name}`) })).toHaveCount(0);
    if (name === 'gamma') {
      await expect(page.getByRole('textbox', { name: 'Ticket Title' })).toHaveValue('alpha preserved draft');
      await page.keyboard.press('Escape');
      await expect(page.locator('[data-component="quick-ticket-composer"]')).toBeHidden();
    }
  }
  // The closed Open project dialog must not inherit the just-closed confirmation dialog's open element.
  expect(await page.locator('wa-dialog[data-project-dialog][open]').count()).toBe(0);
  await expect(page.getByRole('button', { name: 'Open Project', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Open Project', exact: true }).click();
  await page.locator('[data-project-source-choice]').getByRole('button', { name: 'Local Project' }).click();
  await page.locator('[data-project-dialog]').getByRole('textbox', { name: 'Project Folder' }).fill('/work/alpha');
  await page.locator('[data-project-dialog]').getByRole('button', { name: 'Open Project', exact: true }).click();
  await expect(page.getByRole('tab', { name: /^alpha/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByText('alpha restored work', { exact: true })).toBeVisible();
  expect(fixture.attempts.get('alpha')).toBe(2);
  expect(fixture.openBodies.at(-1)).toEqual({ root: '/work/alpha' });
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
  await page.goto('/');
  await expect(page.getByRole('tab', { name: /^beta/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-component="terminal-drawer"]')).toBeVisible();
  expect(heavyCalls(fixture.calls).every((path) => path.includes('/beta/'))).toBe(true);
  expect(fixture.calls.some((path) => /\/(alpha|gamma)\/ai-tools/.test(path))).toBe(false);
  await expect(draft(page, 'alpha')).resolves.toBe('alpha preserved draft');
  await page.getByRole('button', { name: 'Workspace Grid', exact: true }).click();
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
    page.locator('.grid-sidebar-group[data-project-id="alpha"]').getByRole('button', { name: /1 in progress/ }),
  ).toBeVisible();
});

// HS2-MV0BA4: the Remote project form's focused field shows its whole focus ring. The ring (the
// field box plus Kerf's 4px focus-ring reach) must stay inside every clipping ancestor at wide and
// phone widths; an app-owned scroller around the form used to cut it off at both inline edges.
test('keeps the focused remote server form field ring unclipped (HS2-MV0BA4)', async ({ page }) => {
  await startupFixture(page, ['alpha'], 'alpha', false);
  await page.route('**/__hotsheet/remotes**', async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/__hotsheet/remotes') return route.fulfill({ json: [] });
    if (path === '/__hotsheet/remotes/selected-projects') return route.fulfill({ json: [] });
    return route.fulfill({ status: 404, json: { error: 'Unknown synthetic remote route' } });
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await expect(page.getByRole('tab', { name: /^alpha/ })).toBeVisible();
  await page.getByRole('button', { name: 'Add Project' }).click();
  await page.locator('[data-project-source-choice]').getByRole('button', { name: 'Remote Project' }).click();
  const dialog = page.locator('[data-desktop-remote-dialog]');
  await expect(dialog).toHaveJSProperty('open', true);
  const address = dialog.getByRole('textbox', { name: 'Server Address' });
  if (!(await address.isVisible())) await dialog.getByRole('button', { name: 'Add Server' }).click();
  const name = dialog.getByRole('textbox', { name: "This Device's Name" });
  await expect(name).toBeVisible();
  for (const [width, height] of [
    [1440, 900],
    [390, 844],
    [1440, 900],
  ] as const) {
    await page.setViewportSize({ width, height });
    for (const field of [address, name]) {
      await field.focus();
      const clipped = await field.evaluate((input) => {
        const reach = 4;
        // The ring is drawn around the field box: the first ancestor wider than the bare input.
        const inputWidth = input.getBoundingClientRect().width;
        let ring = input as HTMLElement;
        for (let node = input.parentElement; node && node !== document.body; node = node.parentElement) {
          ring = node;
          if (node.getBoundingClientRect().width > inputWidth + 1) break;
        }
        const box = ring.getBoundingClientRect();
        const outer = {
          left: box.left - reach,
          right: box.right + reach,
          top: box.top - reach,
          bottom: box.bottom + reach,
        };
        const cuts: string[] = [];
        for (let node = ring.parentElement; node && node !== document.documentElement; node = node.parentElement) {
          const style = getComputedStyle(node);
          if (style.overflowX === 'visible' && style.overflowY === 'visible') continue;
          const clip = node.getBoundingClientRect();
          if (outer.left < clip.left - 0.5 || outer.right > clip.right + 0.5)
            cuts.push(
              `${node.className || node.tagName}: inline ${outer.left}-${outer.right} vs ${clip.left}-${clip.right}`,
            );
        }
        return cuts;
      });
      expect(clipped, `focus ring at ${width}px`).toEqual([]);
    }
    await page.screenshot({
      path: `target/visual-captures/hs2-mv0ba4-remote-form-${width}.png`,
      animations: 'disabled',
    });
  }
});

// HS2-A5CKQS: approved servers and a server's projects are Kerf list rows, not solid buttons. A
// server row opens discovery and keeps Edit and Remove behind its overflow menu (the maintainer's
// "Row overflow menu" choice); the projects sit under a heading that names the server; the advanced
// TLS fields sit in a Kerf Disclosure rather than a hand-built details element.
test('lists remote servers and projects as rows with an overflow menu (HS2-A5CKQS)', async ({ page }) => {
  const id = '01ARZ3NDEKTSV4RRFFQ69G5FAX';
  const remotes = [
    {
      id,
      label: 'Studio',
      origin: 'http://studio.test:8787',
      identity: { server_id: id, ca_fingerprint: null },
      revision: 3,
      revoked: false,
      last_failure: null,
      stale: false,
    },
  ];
  const deletes: string[] = [];
  await startupFixture(page, ['alpha'], 'alpha', false);
  await page.route('**/__hotsheet/remotes**', async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    if (route.request().method() === 'DELETE') {
      deletes.push(`${path}${url.search}`);
      const removed = remotes.splice(0, 1)[0];
      return route.fulfill({ json: { ...removed, revoked: true } });
    }
    if (path === '/__hotsheet/remotes') return route.fulfill({ json: remotes });
    if (path === '/__hotsheet/remotes/selected-projects') return route.fulfill({ json: [] });
    if (path === `/__hotsheet/remotes/${id}/checkouts`)
      return route.fulfill({
        json: {
          remote: remotes[0],
          checkouts: [
            { id: 'open', name: 'Workspace', state: 'available', sources: [] },
            { id: 'gone', name: 'Archive', state: 'inaccessible', sources: [] },
          ],
        },
      });
    return route.fulfill({ status: 404, json: { error: 'Unknown synthetic remote route' } });
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await expect(page.getByRole('tab', { name: /^alpha/ })).toBeVisible();
  await page.getByRole('button', { name: 'Add Project' }).click();
  await page.locator('[data-project-source-choice]').getByRole('button', { name: 'Remote Project' }).click();
  const dialog = page.locator('[data-desktop-remote-dialog]');
  await expect(dialog).toHaveJSProperty('open', true);

  const servers = dialog.locator('[data-remote-servers]');
  await expect(servers.getByText('Approved Servers')).toBeVisible();
  const row = servers.locator(`[data-component="list-action-row"][data-remote-id="${id}"]`);
  await expect(row).toBeVisible();
  await expect(row.getByText('http://studio.test:8787')).toBeVisible();
  // Edit and Remove are not inline row buttons any more.
  await expect(dialog.getByRole('button', { name: 'Edit Studio' })).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: 'Remove Studio' })).toHaveCount(0);

  await row.getByRole('button', { name: 'Studio', exact: true }).click();
  const projects = dialog.locator('[data-remote-checkouts]');
  await expect(projects.getByText('Projects on Studio')).toBeVisible();
  await expect(projects.locator('[data-checkout-id="open"]')).toBeEnabled();
  await expect(projects.locator('[data-checkout-id="gone"]')).toBeDisabled();
  await expect(projects.locator('[data-checkout-id="gone"]')).toContainText('Unavailable');
  await expect(row.getByRole('button', { name: 'Studio', exact: true })).toHaveAttribute('aria-current', /.+/);

  for (const [width, height] of [
    [1440, 900],
    [900, 700],
    [390, 844],
    [1440, 900],
  ] as const) {
    await page.setViewportSize({ width, height });
    await expect(row.getByRole('button', { name: 'Actions for Studio' })).toBeInViewport();
    await page.screenshot({ path: `target/visual-captures/hs2-a5ckqs-servers-${width}.png`, animations: 'disabled' });
  }

  await row.getByRole('button', { name: 'Actions for Studio' }).click();
  const menu = page.getByRole('menu', { name: 'Actions for Studio' });
  await expect(menu.getByRole('menuitem', { name: 'Edit' })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Remove' })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-a5ckqs-server-menu-1440.png', animations: 'disabled' });
  await menu.getByRole('menuitem', { name: 'Edit' }).click();
  await expect(dialog.getByRole('textbox', { name: 'Server Address' })).toHaveValue('http://studio.test:8787');

  const tls = dialog.locator('[data-remote-tls-settings]');
  await expect(tls).toHaveJSProperty('open', false);
  await expect(dialog.getByRole('textbox', { name: 'Host CA PEM Path (Optional)' })).toBeHidden();
  await tls.getByText('Advanced TLS Settings').click();
  await expect(tls).toHaveJSProperty('open', true);
  await expect(dialog.getByRole('textbox', { name: 'Host CA PEM Path (Optional)' })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-a5ckqs-tls-open-1440.png', animations: 'disabled' });
  await dialog.getByRole('button', { name: 'Back to Servers' }).click();

  await row.getByRole('button', { name: 'Actions for Studio' }).click();
  await menu.getByRole('menuitem', { name: 'Remove' }).click();
  await expect.poll(() => deletes).toEqual([`/__hotsheet/remotes/${id}?revision=3`]);
  await expect(row).toHaveCount(0);
});

/**
 * A faithful stand-in for the host's remote routes (dev-server.ts): the registry list with
 * `last_confirmed_checkouts`, `selected-projects`, a reachability probe that answers 503 while the
 * peer is down, an open that answers 409 while down, and the forget DELETE. Opens can be gated.
 */
async function remoteRegistryFixture(page: Page, hosts: Array<{ id: string; label: string; checkouts: string[] }>) {
  const up = new Map(hosts.map((host) => [host.id, true])),
    openGates = new Map<string, ReturnType<typeof gate>>(),
    /** HS2-4B0KSD: a host's live discovery failure code, and its answer about this computer's session. */
    failures = new Map<string, string>(),
    sessions = new Map<string, Record<string, unknown>>(),
    calls: string[] = [];
  const servers = hosts.map((host) => ({
    id: host.id,
    label: host.label,
    origin: `http://${host.label.toLowerCase()}.test:8787`,
    identity: { server_id: host.id, ca_fingerprint: null },
    revision: 1,
    revoked: false,
    last_failure: null as string | null,
    last_confirmed_checkouts: host.checkouts.map((id) => ({ id, name: `${id}-name`, state: 'available', sources: [] })),
    stale: false,
  }));
  const selected = hosts.flatMap((host) => host.checkouts.map((checkout_id) => ({ binding_id: host.id, checkout_id })));
  await page.route('**/__hotsheet/remotes**', async (route) => {
    const request = route.request(),
      path = new URL(request.url()).pathname;
    calls.push(`${request.method()} ${path}`);
    if (path === '/__hotsheet/remotes') return route.fulfill({ json: servers });
    if (path === '/__hotsheet/remotes/selected-projects') return route.fulfill({ json: selected });
    const lifetime = /^\/__hotsheet\/remotes\/([^/]+)\/(session|renew)$/.exec(path);
    if (lifetime) {
      const failure = failures.get(lifetime[1]);
      if (lifetime[2] === 'renew' && failure) sessions.set(lifetime[1], { renewal_requested_at: 1 });
      return route.fulfill({
        json:
          failure && lifetime[2] === 'session'
            ? { binding_id: lifetime[1], failure }
            : { binding_id: lifetime[1], device: sessions.get(lifetime[1]) },
      });
    }
    const match = /^\/__hotsheet\/remotes\/([^/]+)\/(?:checkouts|projects\/([^/]+)(\/open)?)$/.exec(path);
    const server = match && servers.find((item) => item.id === match[1]);
    if (!match || !server) return route.fulfill({ status: 404, json: { error: 'Unknown synthetic remote route' } });
    if (!match[2] && failures.has(server.id))
      return route.fulfill({
        json: {
          remote: { ...server, last_failure: 'unauthorized' },
          checkouts: null,
          failure: failures.get(server.id),
        },
      });
    if (!match[2])
      return up.get(server.id)
        ? route.fulfill({ json: { remote: server, checkouts: server.last_confirmed_checkouts } })
        : route.fulfill({
            status: 503,
            json: { error: 'Could not reach or authenticate the approved remote server.' },
          });
    const checkout = decodeURIComponent(match[2]);
    if (request.method() === 'DELETE') return route.fulfill({ json: { selected: false } });
    await openGates.get(`${server.id}/${checkout}`)?.promise;
    if (!up.get(server.id))
      return route.fulfill({ status: 409, json: { error: 'This remote project is unavailable. Retry discovery.' } });
    const id = remoteProjectId(server.id, checkout);
    return route.fulfill({
      status: 201,
      json: {
        id,
        root: id,
        name: `${server.label} · ${checkout}-name`,
        remoteHost: server.label,
        stores: [],
        apiPath: `/__hotsheet/project-api/${encodeURIComponent(id)}`,
        compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
        needsTicketSetup: false,
        needsHs1Migration: false,
        hs1ImportCompleted: false,
        hs1CleanupEligible: false,
        hs1RegistrationRepairAvailable: false,
      },
    });
  });
  return { up, openGates, failures, sessions, calls };
}

const STUDIO = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const LAPTOP = '01ARZ3NDEKTSV4RRFFQ69G5FAW';
const projectTabIds = (page: Page) =>
  page
    .locator('[data-tab-kind="project"]')
    .evaluateAll((tabs) => tabs.map((tab) => (tab as HTMLElement).dataset.projectId ?? ''));

test('shows remembered remote tabs before any open resolves and opens each host on its own (HS2-EJ5K09)', async ({
  page,
}) => {
  await startupFixture(page, ['alpha'], 'alpha', false);
  const remote = await remoteRegistryFixture(page, [
    { id: STUDIO, label: 'Studio', checkouts: ['one'] },
    { id: LAPTOP, label: 'Laptop', checkouts: ['two'] },
  ]);
  const slow = gate(),
    fast = gate();
  remote.openGates.set(`${STUDIO}/one`, slow);
  remote.openGates.set(`${LAPTOP}/two`, fast);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  // Both tabs render from the registry alone while both opens are still held.
  const studio = page.locator(
      `[data-tab-kind="project"][data-project-id="remote-placeholder:${remoteProjectId(STUDIO, 'one')}"]`,
    ),
    laptop = page.locator(
      `[data-tab-kind="project"][data-project-id="remote-placeholder:${remoteProjectId(LAPTOP, 'two')}"]`,
    );
  await expect(studio).toHaveAttribute('data-project-pending', 'true');
  await expect(laptop).toHaveAttribute('data-project-pending', 'true');
  await expect(page.getByRole('tab', { name: /Studio · one-name/ })).toBeVisible();
  await expect(page.getByRole('tab', { name: /Laptop · two-name/ })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-ej5k09-loading-tabs-wide.png', animations: 'disabled' });
  await page.setViewportSize({ width: 900, height: 700 });
  await page.screenshot({ path: 'target/visual-captures/hs2-ej5k09-loading-tabs-medium.png', animations: 'disabled' });
  await page.setViewportSize({ width: 1440, height: 900 });
  // Both opens started in parallel.
  await expect
    .poll(() => remote.calls.filter((call) => call.startsWith('POST') && call.endsWith('/open')).length)
    .toBe(2);
  // The fast host opens while the slow one is still loading.
  fast.release();
  await expect(
    page.locator(`[data-tab-kind="project"][data-project-id="${remoteProjectId(LAPTOP, 'two')}"]`),
  ).toBeVisible();
  await expect(studio).toHaveAttribute('data-project-pending', 'true');
  expect(await projectTabIds(page)).toEqual([
    'alpha',
    `remote-placeholder:${remoteProjectId(STUDIO, 'one')}`,
    remoteProjectId(LAPTOP, 'two'),
  ]);
  slow.release();
  await expect(
    page.locator(`[data-tab-kind="project"][data-project-id="${remoteProjectId(STUDIO, 'one')}"]`),
  ).toBeVisible();
  // The saved order holds, and the local project kept the selection.
  expect(await projectTabIds(page)).toEqual(['alpha', remoteProjectId(STUDIO, 'one'), remoteProjectId(LAPTOP, 'two')]);
  await expect(page.locator('[data-tab-kind="project"][data-project-id="alpha"]')).toHaveAttribute(
    'data-selected',
    'true',
  );
});

test('keeps a dragged mix of local and remote project tabs in order across reloads (HS2-NRBWGF)', async ({ page }) => {
  await startupFixture(page, ['alpha', 'beta'], 'alpha', false);
  await remoteRegistryFixture(page, [{ id: STUDIO, label: 'Studio', checkouts: ['one'] }]);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  const studio = remoteProjectId(STUDIO, 'one'),
    tab = (id: string) => page.locator(`[data-tab-kind="project"][data-project-id="${id}"]`);
  await expect(tab(studio)).toBeVisible();
  await expect(tab('beta')).toBeVisible();
  // Before any drag, remote tabs follow the local ones.
  await expect.poll(() => projectTabIds(page)).toEqual(['alpha', 'beta', studio]);
  // Drag the remote tab between the two local ones.
  const bounds = (await tab('alpha').boundingBox())!,
    dataTransfer = await page.evaluateHandle(() => new DataTransfer()),
    event = { dataTransfer, clientX: bounds.x + bounds.width - 1, clientY: bounds.y + bounds.height / 2 };
  await tab(studio).dispatchEvent('dragstart', event);
  await tab('alpha').dispatchEvent('dragover', event);
  await tab('alpha').dispatchEvent('drop', event);
  await tab(studio).dispatchEvent('dragend', event);
  await expect.poll(() => projectTabIds(page)).toEqual(['alpha', studio, 'beta']);
  for (const round of [1, 2]) {
    await page.reload();
    await expect(tab(studio), `reload ${round}`).toBeVisible();
    await expect(tab('beta'), `reload ${round}`).toBeVisible();
    await expect.poll(() => projectTabIds(page), `reload ${round}`).toEqual(['alpha', studio, 'beta']);
  }
  // The local reopen list stays local-only.
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('hotsheet.open-projects') ?? '[]'))).toEqual([
    '/work/alpha',
    '/work/beta',
  ]);
});

test('keeps an unreachable remote tab reconnecting and opens it in place once the host answers (HS2-WNK73S)', async ({
  page,
}) => {
  await startupFixture(page, ['alpha'], 'alpha', false);
  const remote = await remoteRegistryFixture(page, [
    { id: STUDIO, label: 'Studio', checkouts: ['one'] },
    { id: LAPTOP, label: 'Laptop', checkouts: ['two'] },
  ]);
  remote.up.set(STUDIO, false);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  const placeholderId = `remote-placeholder:${remoteProjectId(STUDIO, 'one')}`,
    studio = page.locator(`[data-tab-kind="project"][data-project-id="${placeholderId}"]`);
  await expect(
    page.locator(`[data-tab-kind="project"][data-project-id="${remoteProjectId(LAPTOP, 'two')}"]`),
  ).toBeVisible();
  // The down host keeps its tab, in its saved place, with the disconnected indicator.
  await expect(studio).toHaveAttribute('data-disconnected', 'true');
  await expect(studio.locator('[data-lucide="wifi-off"]')).toBeVisible();
  expect(await projectTabIds(page)).toEqual(['alpha', placeholderId, remoteProjectId(LAPTOP, 'two')]);
  await studio.click();
  const panel = page.locator('[data-remote-project-placeholder="reconnecting"]');
  await expect(panel).toContainText('Reconnecting to Studio · one-name');
  for (const [scheme, width, height] of [
    ['light', 1440, 900],
    ['light', 900, 700],
    ['light', 390, 844],
    ['dark', 1440, 900],
    ['dark', 900, 700],
    ['dark', 390, 844],
  ] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.setViewportSize({ width, height });
    await expect(panel.getByRole('button', { name: 'Retry now' })).toBeInViewport();
    await page.screenshot({
      path: `target/visual-captures/hs2-wnk73s-reconnecting-${scheme}-${width}.png`,
      animations: 'disabled',
    });
  }
  await page.emulateMedia({ colorScheme: 'light' });
  await page.setViewportSize({ width: 1440, height: 900 });
  // The host comes back; an explicit retry probes it once and opens the project in the same tab position.
  remote.up.set(STUDIO, true);
  const probesBefore = remote.calls.filter((call) => call === `GET /__hotsheet/remotes/${STUDIO}/checkouts`).length;
  await panel.getByRole('button', { name: 'Retry now' }).click();
  const opened = page.locator(`[data-tab-kind="project"][data-project-id="${remoteProjectId(STUDIO, 'one')}"]`);
  await expect(opened).toHaveAttribute('data-selected', 'true');
  await expect(page.getByText('Workspace restored work', { exact: true })).toBeVisible();
  expect(await projectTabIds(page)).toEqual(['alpha', remoteProjectId(STUDIO, 'one'), remoteProjectId(LAPTOP, 'two')]);
  expect(remote.calls.filter((call) => call === `GET /__hotsheet/remotes/${STUDIO}/checkouts`).length).toBeGreaterThan(
    probesBefore,
  );
  // Nothing forgot the selection along the way.
  expect(remote.calls.some((call) => call.startsWith('DELETE'))).toBe(false);
});

test('closing a reconnecting remote tab is the only thing that forgets it (HS2-WNK73S)', async ({ page }) => {
  await startupFixture(page, ['alpha'], 'alpha', false);
  const remote = await remoteRegistryFixture(page, [{ id: STUDIO, label: 'Studio', checkouts: ['one'] }]);
  remote.up.set(STUDIO, false);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  const studio = page.locator(
    `[data-tab-kind="project"][data-project-id="remote-placeholder:${remoteProjectId(STUDIO, 'one')}"]`,
  );
  await expect(studio).toHaveAttribute('data-disconnected', 'true');
  expect(remote.calls.some((call) => call.startsWith('DELETE'))).toBe(false);
  await studio.hover();
  await studio.locator('[data-action="close-project-tab"]').click();
  await page.getByRole('button', { name: 'Close Project', exact: true }).click();
  await expect(studio).toHaveCount(0);
  await expect
    .poll(() => remote.calls.filter((call) => call.startsWith('DELETE')))
    .toEqual([`DELETE /__hotsheet/remotes/${STUDIO}/projects/one`]);
});

test('keeps an open remote tab with a reconnecting state while its host drops mid-session (HS2-WNK73S)', async ({
  page,
}) => {
  await startupFixture(page, ['alpha'], 'alpha', false);
  await remoteRegistryFixture(page, [{ id: LAPTOP, label: 'Laptop', checkouts: ['two'] }]);
  const id = remoteProjectId(LAPTOP, 'two'),
    host = { down: false, sockets: [] as WebSocketRoute[] };
  await page.routeWebSocket(/\/__hotsheet\/project-api\/remote[^/]+\/ws\/sync(?:\?.*)?$/, (socket) => {
    if (host.down) void socket.close();
    else host.sockets.push(socket);
  });
  await page.route(/\/__hotsheet\/project-api\/remote[^/]+\/ws\/poll/, (route) =>
    host.down ? route.fulfill({ status: 502, json: { error: 'Remote server unreachable.' } }) : route.fallback(),
  );
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  const tab = page.locator(`[data-tab-kind="project"][data-project-id="${id}"]`);
  await tab.click();
  await expect(page.getByText('Workspace restored work', { exact: true })).toBeVisible();
  const banner = page.locator('[data-component="remote-reconnecting-banner"]');
  await expect(banner).toHaveCount(0);
  host.down = true;
  for (const socket of host.sockets.splice(0)) await socket.close();
  // The tab and its loaded tickets stay; past the grace the tab and the workspace say it is reconnecting.
  await expect(banner).toContainText('Reconnecting to Laptop', { timeout: 15_000 });
  await expect(tab).toHaveAttribute('data-disconnected', 'true');
  await expect(page.getByText('Workspace restored work', { exact: true })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-wnk73s-mid-session-wide.png', animations: 'disabled' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(banner).toBeInViewport();
  await page.screenshot({ path: 'target/visual-captures/hs2-wnk73s-mid-session-narrow.png', animations: 'disabled' });
  await page.setViewportSize({ width: 1440, height: 900 });
  host.down = false;
  await expect(banner).toHaveCount(0, { timeout: 40_000 });
  await expect(tab).toHaveAttribute('data-disconnected', 'false');
});

test('a click on a reconnecting remote tab retries its change stream at once, not after its backoff (HS2-ZTAJ1R)', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await startupFixture(page, ['alpha'], 'alpha', false);
  await remoteRegistryFixture(page, [{ id: LAPTOP, label: 'Laptop', checkouts: ['two'] }]);
  const id = remoteProjectId(LAPTOP, 'two'),
    host = { down: false, failedPolls: 0, sockets: [] as WebSocketRoute[] };
  await page.routeWebSocket(/\/__hotsheet\/project-api\/remote[^/]+\/ws\/sync(?:\?.*)?$/, (socket) => {
    if (host.down) void socket.close();
    else host.sockets.push(socket);
  });
  await page.route(/\/__hotsheet\/project-api\/remote[^/]+\/ws\/poll/, (route) => {
    if (!host.down) return route.fallback();
    host.failedPolls += 1;
    return route.fulfill({ status: 502, json: { error: 'Remote server unreachable.' } });
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  const tab = page.locator(`[data-tab-kind="project"][data-project-id="${id}"]`),
    alpha = page.locator('[data-tab-kind="project"][data-project-id="alpha"]'),
    banner = page.locator('[data-component="remote-reconnecting-banner"]');
  await tab.click();
  await expect(page.getByText('Workspace restored work', { exact: true })).toBeVisible();
  host.down = true;
  for (const socket of host.sockets.splice(0)) await socket.close();
  await expect(banner).toContainText('Reconnecting to Laptop', { timeout: 15_000 });
  // Let the stream back off until its next retry is many seconds away (500 ms doubling per failure).
  await expect.poll(() => host.failedPolls, { timeout: 60_000 }).toBeGreaterThanOrEqual(7);
  const before = host.failedPolls;
  host.down = false;
  // The host is back, but the stream is asleep in its backoff; selecting the tab wakes it now.
  await alpha.click();
  const clicked = Date.now();
  await tab.click();
  await expect(banner).toHaveCount(0, { timeout: 6_000 });
  expect(Date.now() - clicked).toBeLessThan(6_000);
  await expect(tab).toHaveAttribute('data-disconnected', 'false');
  expect(host.failedPolls).toBe(before);
});

test('an expired remote approval offers Renew, never reconnecting, and opens once the host renews it (HS2-4B0KSD)', async ({
  page,
}) => {
  await startupFixture(page, ['alpha'], 'alpha', false);
  const remote = await remoteRegistryFixture(page, [{ id: STUDIO, label: 'Studio', checkouts: ['one'] }]);
  remote.up.set(STUDIO, false);
  remote.failures.set(STUDIO, 'session_expired');
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  const studio = page.locator(
    `[data-tab-kind="project"][data-project-id="remote-placeholder:${remoteProjectId(STUDIO, 'one')}"]`,
  );
  await studio.click();
  const panel = page.locator('[data-remote-project-placeholder="expired"]');
  await expect(panel).toContainText('Renew Access to Studio · one-name');
  await expect(page.locator('[data-remote-project-placeholder="reconnecting"]')).toHaveCount(0);
  for (const [scheme, width, height] of [
    ['light', 1440, 900],
    ['light', 900, 700],
    ['light', 390, 844],
    ['dark', 1440, 900],
    ['dark', 390, 844],
  ] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.setViewportSize({ width, height });
    await expect(panel.getByRole('button', { name: 'Renew' })).toBeInViewport();
    await page.screenshot({
      path: `target/visual-captures/hs2-4b0ksd-expired-${scheme}-${width}.png`,
      animations: 'disabled',
    });
  }
  await page.emulateMedia({ colorScheme: 'light' });
  await page.setViewportSize({ width: 1440, height: 900 });
  // Renew asks the remote host to renew this computer in place; the binding is never reviewed or removed.
  await panel.getByRole('button', { name: 'Renew' }).click();
  await expect(panel).toContainText('Renewal requested.');
  expect(remote.calls).toContain(`POST /__hotsheet/remotes/${STUDIO}/renew`);
  await page.screenshot({
    path: 'target/visual-captures/hs2-4b0ksd-renewal-requested-light-1440.png',
    animations: 'disabled',
  });
  // The host renewed it: Check Again opens the same tab without pairing again.
  remote.failures.delete(STUDIO);
  remote.up.set(STUDIO, true);
  await panel.getByRole('button', { name: 'Check Again' }).click();
  await expect(
    page.locator(`[data-tab-kind="project"][data-project-id="${remoteProjectId(STUDIO, 'one')}"]`),
  ).toHaveAttribute('data-selected', 'true');
  expect(remote.calls.some((call) => call.startsWith('DELETE') || call.includes('/pair'))).toBe(false);
});

test('a revoked remote approval says so and offers pairing again (HS2-4B0KSD)', async ({ page }) => {
  await startupFixture(page, ['alpha'], 'alpha', false);
  const remote = await remoteRegistryFixture(page, [{ id: STUDIO, label: 'Studio', checkouts: ['one'] }]);
  remote.up.set(STUDIO, false);
  remote.failures.set(STUDIO, 'session_revoked');
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page
    .locator(`[data-tab-kind="project"][data-project-id="remote-placeholder:${remoteProjectId(STUDIO, 'one')}"]`)
    .click();
  const panel = page.locator('[data-remote-project-placeholder="revoked"]');
  await expect(panel).toContainText('Access Revoked by Studio · one-name');
  await expect(panel.getByRole('button', { name: 'Pair Again' })).toBeVisible();
  await expect(panel.getByRole('button', { name: 'Renew' })).toHaveCount(0);
});

test('an open remote tab reminds before its approval expires, with Renew and Dismiss (HS2-4B0KSD)', async ({
  page,
}) => {
  await startupFixture(page, ['alpha'], 'alpha', false);
  const remote = await remoteRegistryFixture(page, [{ id: LAPTOP, label: 'Laptop', checkouts: ['two'] }]);
  const now = Math.floor(Date.now() / 1000),
    day = 24 * 60 * 60;
  remote.sessions.set(LAPTOP, {
    renewed_at: now - 70 * day - 60,
    expires_at: now + 20 * day - 60,
    idle_expires_at: now + 30 * day,
    reminder_day: 70,
    renewal_requested_at: null,
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.locator(`[data-tab-kind="project"][data-project-id="${remoteProjectId(LAPTOP, 'two')}"]`).click();
  const banner = page.locator('[data-component="remote-approval-reminder-banner"]');
  await expect(banner).toContainText('Renew access for Laptop');
  await expect(banner).toContainText('in 20 days');
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
    await expect(banner.getByRole('button', { name: 'Renew' })).toBeInViewport();
    await page.screenshot({
      path: `target/visual-captures/hs2-4b0ksd-reminder-light-${width}.png`,
      animations: 'disabled',
    });
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  // The reminder read the session once when the tab opened; nothing polls it.
  expect(remote.calls.filter((call) => call === `GET /__hotsheet/remotes/${LAPTOP}/session`)).toHaveLength(1);
  await banner.getByRole('button', { name: 'Dismiss' }).click();
  await expect(banner).toHaveCount(0);
  // A reload keeps it dismissed until the next reminder day.
  await page.reload();
  await page.locator(`[data-tab-kind="project"][data-project-id="${remoteProjectId(LAPTOP, 'two')}"]`).click();
  await expect(page.getByText('Workspace restored work', { exact: true })).toBeVisible();
  await expect(banner).toHaveCount(0);
});
