import { expect, type Page, test } from '@playwright/test';

import type { Capabilities } from '../src/api';
import type { Project } from '../src/interactions/types';
import { remoteProjectId } from '../src/remote-projects/identity';
import { ticketsJson } from './ticket-page-fixture';

/**
 * HS2-MBFGRV: a prose ticket link whose slug matches copies of one ticket in a local tab and in a
 * remote tab (a clone of the same project on another machine) opens the best copy without asking.
 * The copies share the ticket's ULID but differ in connection and store. Everything is synthetic.
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
const remoteName = 'studio · Workspace';
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
// Each clone's Hot Sheet Git source has its own connection id; a ticket's ULID is the same in both.
const localConnection = 'git-1a2b3c',
  remoteConnection = 'git-4d5e6f';
const ulid = (suffix: string) => `01JX${'0'.repeat(22 - suffix.length)}${suffix}`;
const sharedUlid = ulid('ABC'),
  localDuplicateUlid = ulid('DP1'),
  remoteDuplicateUlid = ulid('DP2');
const EARLIER = '2026-11-01T00:00:00Z',
  LATER = '2026-11-02T00:00:00Z';

type Row = ReturnType<typeof row>;
const row = (connection: string, native: string, slug: string, title: string, updatedAt = EARLIER) => ({
  id: native,
  native_id: native,
  connection_id: connection,
  qualified_id: `${connection}:${native}`,
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
  created_at: EARLIER,
  updated_at: updatedAt,
});
const source = row(localConnection, ulid('SRC'), 'HS2-SRC001', 'Synthetic link source');
const sourceDetails = 'Copied ticket: HS2-ABC123. Reused slug: HS2-DUP999.';

/** The two tabs' tickets; `remoteUpdatedAt` dates the remote copy of the shared ticket. */
function stores(remoteUpdatedAt: string) {
  return {
    local: [
      source,
      row(localConnection, sharedUlid, 'HS2-ABC123', 'Shared ticket, local copy'),
      row(localConnection, localDuplicateUlid, 'HS2-DUP999', 'Local ticket reusing a slug'),
    ],
    remote: [
      row(remoteConnection, sharedUlid, 'HS2-ABC123', 'Shared ticket, remote copy', remoteUpdatedAt),
      row(remoteConnection, remoteDuplicateUlid, 'HS2-DUP999', 'Remote ticket reusing a slug'),
    ],
  };
}

async function bestCopyFixture(page: Page, remoteUpdatedAt: string) {
  const tickets = stores(remoteUpdatedAt),
    fullReads: string[] = [];
  await page.addInitScript(
    ({ root }) => {
      if (!localStorage.getItem('best-copy-seeded')) {
        localStorage.setItem('best-copy-seeded', 'true');
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
          name: remoteName,
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
    const remote = decodeURIComponent(path).startsWith(`/__hotsheet/project-api/${remoteId}/`),
      side = remote ? 'remote' : 'local',
      items: Row[] = tickets[side];
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: remote ? remoteConnection : localConnection,
            provider: 'git',
            display_name: 'Hot Sheet Git',
            locator: '/tickets',
            default: true,
            capabilities,
          },
        ],
      });
    const single = /\/checkouts\/[^/]+\/tickets\/([^/]+)$/.exec(path);
    if (single && request.method() === 'GET') {
      const id = decodeURIComponent(single[1]);
      fullReads.push(`${side}:${id}`);
      const ticket = items.find((item) => [item.id, item.qualified_id, item.native_id, item.slug].includes(id));
      return ticket
        ? route.fulfill({
            json: {
              ...ticket,
              details: ticket === source ? sourceDetails : `${ticket.title}.`,
              notes: [],
              attachments: [],
              store: remote ? `/stores/${checkout}.hs2` : local.stores[0],
            },
          })
        : route.fulfill({ status: 404, json: { error: 'ticket not found' } });
    }
    if (path.endsWith('/tickets') && request.method() === 'GET') {
      const text = url.searchParams.get('text')?.toLocaleLowerCase(),
        matched = text
          ? items.filter((item) => `${item.slug} ${item.title}`.toLocaleLowerCase().includes(text))
          : items;
      return route.fulfill({ json: ticketsJson(request, matched) });
    }
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(url.searchParams.get('since') ?? 0), events: [], overflow: false },
      });
    if (path.endsWith('/ai-settings')) return route.fulfill({ json: { tool: '', model: '', effort: '' } });
    if (
      /\/(permissions|connections|commands|command-runs|corrupt-tickets|views|ai-tools|sessions|terminals)$/.test(path)
    )
      return route.fulfill({ json: [] });
    return route.fulfill({ status: 404, json: { error: `No fixture for ${path}` } });
  });
  return { fullReads };
}

/** Open the remote tab beside the local one, then show the local source ticket in the inspector. */
async function openSource(page: Page) {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  const alpha = page.getByRole('tab', { name: /^alpha/ });
  await expect(alpha).toBeVisible();
  await page.getByRole('button', { name: 'Add Project' }).click();
  await page.locator('[data-project-source-choice]').getByRole('button', { name: 'Remote Project' }).click();
  const dialog = page.locator('[data-desktop-remote-dialog]');
  await dialog.getByRole('button', { name: 'Studio', exact: true }).click();
  await dialog.getByRole('button', { name: 'Workspace' }).click();
  await expect(page.getByRole('tab', { name: /studio · Workspace/ })).toBeVisible();
  await alpha.click();
  await expect(alpha).toHaveAttribute('aria-selected', 'true');
  await page.locator(`[data-component="summary-row"][data-ticket-slug="${source.slug}"]`).click();
  const inspector = page.locator('#app-right-rail');
  await expect(inspector.getByRole('link', { name: 'HS2-ABC123', exact: true })).toBeVisible();
  return inspector;
}

const choice = (page: Page) => page.locator('[data-dialog-component="ticket-link-choice-dialog"]');

test('a link to one ticket in local and remote tabs opens the local copy when equally recent (HS2-MBFGRV)', async ({
  page,
}) => {
  const { fullReads } = await bestCopyFixture(page, EARLIER);
  const inspector = await openSource(page);
  await inspector.getByRole('link', { name: 'HS2-ABC123', exact: true }).click();
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-ABC123 in alpha' });
  await expect(reader).toContainText('Shared ticket, local copy');
  await expect(choice(page).and(page.locator('[open]'))).toHaveCount(0);
  expect(fullReads).toContain(`local:${localConnection}:${sharedUlid}`);
  expect(fullReads.filter((read) => read.startsWith(`remote:`) && read.endsWith(sharedUlid))).toEqual([]);
});

test('a link to one ticket in local and remote tabs opens the newer remote copy (HS2-MBFGRV)', async ({ page }) => {
  const { fullReads } = await bestCopyFixture(page, LATER);
  const inspector = await openSource(page);
  await inspector.getByRole('link', { name: 'HS2-ABC123', exact: true }).click();
  const reader = page.getByRole('dialog', { name: `Read and edit HS2-ABC123 in ${remoteName}` });
  await expect(reader).toContainText('Shared ticket, remote copy');
  await expect(choice(page).and(page.locator('[open]'))).toHaveCount(0);
  expect(fullReads).toContain(`remote:${remoteConnection}:${sharedUlid}`);
});

test('a slug naming different tickets in local and remote tabs still asks which one (HS2-MBFGRV)', async ({ page }) => {
  await bestCopyFixture(page, EARLIER);
  const inspector = await openSource(page);
  await inspector.getByRole('link', { name: 'HS2-DUP999', exact: true }).click();
  await expect(choice(page)).toHaveAttribute('open', '');
  await expect(choice(page).getByRole('button').filter({ hasText: 'Local ticket reusing a slug' })).toHaveCount(1);
  await expect(choice(page).getByRole('button').filter({ hasText: 'Remote ticket reusing a slug' })).toHaveCount(1);
});
