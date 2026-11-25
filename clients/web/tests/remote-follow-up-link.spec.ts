import { expect, type Page, test } from '@playwright/test';

import type { Capabilities, CodeReview } from '../src/api';
import type { Project } from '../src/interactions/types';
import { remoteProjectId } from '../src/remote-projects/identity';

/**
 * HS2-4SPTA4: a ticket in a remote project tab records its follow-ups with the remote host's native
 * checkout id, while the tab's id is `remote:<binding>:<checkout>`. Opening a follow-up resolves the
 * native id against the source tab's own server. Everything here is synthetic and mocked.
 */
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
const compatibility = { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false };
const binding = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const checkout = 'workspace-a1b2c3';
const remoteId = remoteProjectId(binding, checkout);
const server = {
  id: binding,
  label: 'Studio',
  origin: 'http://studio.test:8787',
  identity: { server_id: binding, ca_fingerprint: null },
  revision: 1,
  revoked: false,
  last_failure: null,
  stale: false,
};
const local: Project = {
  id: 'alpha',
  name: 'alpha',
  root: '/work/alpha',
  stores: ['/work/alpha.hs2'],
  apiPath: '/__hotsheet/project-api/alpha',
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
};
const row = (native: string, slug: string, title: string) => ({
  id: native,
  native_id: native,
  connection_id: 'git-local',
  qualified_id: `git-local:${native}`,
  slug,
  title,
  category: 'task',
  priority: 'default',
  status: 'not_started',
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-11-01T00:00:00Z',
  updated_at: '2026-11-01T00:00:00Z',
});
const source = {
    ...row('remote-source', 'HS2-RMTSRC', 'Synthetic remote follow-up source'),
    follow_ups: [
      { project_id: checkout, connection_id: 'git-local', native_id: 'remote-target' },
      { project_id: 'elsewhere-d4e5f6', connection_id: 'git-local', native_id: 'remote-other' },
    ],
  },
  target = row('remote-target', 'HS2-RMTTGT', 'Synthetic remote follow-up target');
const full = (ticket: typeof target) => ({
  ...ticket,
  details: '',
  notes: [],
  attachments: [],
  store: `/stores/${checkout}.hs2`,
});
const codeReview = (id: string): CodeReview => ({
  commits: [],
  ranges: [],
  truncated: false,
  follow_ups:
    id === source.native_id
      ? [
          {
            target: source.follow_ups[0],
            state: 'resolved',
            slug: target.slug,
            title: target.title,
          },
          {
            target: source.follow_ups[1],
            state: 'resolved',
            slug: 'HS2-RMTOTH',
            title: 'Synthetic follow-up in a project not open here',
          },
        ]
      : [],
});

async function remoteFixture(page: Page) {
  const ticketReads: string[] = [];
  await page.addInitScript(
    ({ root }) => {
      if (!localStorage.getItem('remote-follow-up-seeded')) {
        localStorage.setItem('remote-follow-up-seeded', 'true');
        localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
        localStorage.setItem('hotsheet.workspace.active-project-root.v1', root);
      }
    },
    { root: local.root },
  );
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  await page.route('**/__hotsheet/**', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (/^\/__hotsheet\/(?:remote\/)?projects\/open$/.test(path)) return route.fulfill({ status: 201, json: local });
    if (path === '/__hotsheet/projects/migration-jobs') return route.fulfill({ json: { job: null } });
    if (path === '/__hotsheet/remotes') return route.fulfill({ json: [server] });
    if (path === '/__hotsheet/remotes/selected-projects') return route.fulfill({ json: [] });
    if (path === `/__hotsheet/remotes/${binding}/checkouts`)
      return route.fulfill({
        json: {
          remote: server,
          host_name: 'studio',
          checkouts: [{ id: checkout, name: 'Workspace', state: 'available', sources: [] }],
        },
      });
    if (path === `/__hotsheet/remotes/${binding}/projects/${checkout}/open`)
      // The bridge's remote open reply (project-bridge.ts), with the remote machine's name.
      return route.fulfill({
        status: 201,
        json: {
          id: remoteId,
          root: remoteId,
          name: 'studio · Workspace',
          remoteHost: 'studio',
          stores: [],
          apiPath: `/__hotsheet/project-api/${encodeURIComponent(remoteId)}`,
          compatibility,
          needsTicketSetup: false,
          needsHs1Migration: false,
          hs1ImportCompleted: false,
          hs1CleanupEligible: false,
          hs1RegistrationRepairAvailable: false,
        },
      });
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
        ],
      });
    const remote = decodeURIComponent(path).startsWith(`/__hotsheet/project-api/${remoteId}/`);
    const review = /\/tickets\/([^/]+)\/code-review$/.exec(path);
    if (review && request.method() === 'GET') {
      const id = decodeURIComponent(review[1]);
      return route.fulfill({ json: codeReview(id.replace(/^git-local:/, '')) });
    }
    const single = /\/checkouts\/[^/]+\/tickets\/([^/]+)$/.exec(path);
    if (single && request.method() === 'GET') {
      const id = decodeURIComponent(single[1]);
      ticketReads.push(id);
      const ticket = [source, target].find((item) =>
        [item.id, item.qualified_id, item.native_id, item.slug].includes(id),
      );
      return ticket
        ? route.fulfill({ json: full(ticket) })
        : route.fulfill({ status: 404, json: { error: 'ticket not found' } });
    }
    if (path.endsWith('/tickets') && request.method() === 'GET') {
      const items = remote ? [source, target] : [];
      return route.fulfill({
        json: {
          items,
          counts: {
            total: items.length,
            queued: items.length,
            backlog: 0,
            archive: 0,
            trash: 0,
            open: items.length,
            up_next: 0,
            active: 0,
            started: 0,
            completed_today: 0,
            completion_trend: [0, 0, 0, 0, 0, 0, 0],
          },
        },
      });
    }
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(url.searchParams.get('since') ?? 0), events: [], overflow: false },
      });
    // The Code Review tab is an experimental feature area; this server has it on.
    if (path.endsWith('/feature-settings') && request.method() === 'GET')
      return route.fulfill({
        json: {
          defaults: { 'code-review': false },
          configured: { 'code-review': true },
          effective: { 'code-review': true },
        },
      });
    if (path.endsWith('/ai-settings')) return route.fulfill({ json: { tool: '', model: '', effort: '' } });
    if (
      /\/(permissions|connections|commands|command-runs|corrupt-tickets|views|ai-tools|sessions|terminals)$/.test(path)
    )
      return route.fulfill({ json: [] });
    return route.fulfill({ status: 404, json: { error: `No fixture for ${path}` } });
  });
  return { ticketReads };
}

test('a remote tab opens a follow-up recorded with the host checkout id (HS2-4SPTA4)', async ({ page }) => {
  const { ticketReads } = await remoteFixture(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await expect(page.getByRole('tab', { name: /^alpha/ })).toBeVisible();
  await page.getByRole('button', { name: 'Add Project' }).click();
  await page.locator('[data-project-source-choice]').getByRole('button', { name: 'Remote Project' }).click();
  const dialog = page.locator('[data-desktop-remote-dialog]');
  await dialog.getByRole('button', { name: 'Studio', exact: true }).click();
  await dialog.getByRole('button', { name: 'Workspace' }).click();
  await expect(page.getByRole('tab', { name: /studio · Workspace/ })).toBeVisible();

  await page.locator(`[data-ticket-slug="${source.slug}"]`).click();
  await page.locator('#app-right-rail').getByRole('button', { name: 'Open Ticket Reader' }).click();
  const reader = page.getByRole('dialog', { name: `Read and edit ${source.slug}` });
  await reader.getByRole('tab', { name: 'Code Review' }).click();
  const section = reader.getByRole('region', { name: 'Follow-Up Tickets' });
  await expect(section).toBeVisible({ timeout: 20_000 });
  const rows = section.locator('[data-action="open-follow-up-target"]');
  await expect(rows).toHaveCount(2);
  const toasts = page.locator('.app-toast');

  // The host's checkout id maps to this tab, and the target ticket opens in the reader.
  await rows.filter({ hasText: target.slug }).click();
  const linked = page.getByRole('dialog', { name: `Read and edit ${target.slug}` });
  await expect(linked).toBeVisible({ timeout: 20_000 });
  await expect(linked).toContainText(target.title);
  expect(ticketReads).toContain('git-local:remote-target');
  await expect(toasts.filter({ hasText: 'Open project' })).toHaveCount(0);

  // A checkout that is not open on this host names the host, not a raw checkout id.
  await page.keyboard.press('Escape');
  await expect(linked).toHaveCount(0);
  await expect(reader).toBeVisible();
  await rows.filter({ hasText: 'HS2-RMTOTH' }).click();
  await expect(toasts.filter({ hasText: 'Open that project on studio to view this ticket.' })).toBeVisible();
  await expect(toasts.filter({ hasText: 'Open project' })).toHaveCount(0);
});
