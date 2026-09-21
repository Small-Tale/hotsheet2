import { expect, test } from '@playwright/test';

// HS2-6AXG6Z: with a workspace search active, selecting a matching ticket that is not on the base
// view's first page and then editing it (whose autosave change event triggers a background refresh)
// must not deselect the ticket or tear down the inspector. Before the fix, refreshProject replaced the
// visible rows/selection with the base-view page — which lacked the search-only ticket — deselecting it
// and yanking focus out of the field being edited.

const capabilities = {
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
const project = {
  id: 'demo',
  root: '/work/demo',
  name: 'Demo',
  stores: ['/work/demo.hs2'],
  apiPath: '/__hotsheet/project-api/demo',
  needsTicketSetup: false,
  needsHs1Migration: false,
  hs1ImportCompleted: false,
  hs1CleanupEligible: false,
};
const row = (slug: string, title: string) => ({
  connection_id: 'git-local',
  native_id: slug,
  qualified_id: `git-local:${slug}`,
  id: slug,
  slug,
  title,
  category: 'feature',
  priority: 'default',
  status: 'started',
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-14T00:00:00Z',
});
const base = [row('HS2-BASE1', 'Base one'), row('HS2-BASE2', 'Base two')];
const match = row('HS2-MATCH', 'Parser fix');

test('keeps a search-selected ticket selected through a background refresh (HS2-6AXG6Z)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  const polls: import('@playwright/test').Route[] = [];
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (path === '/__hotsheet/projects/open') return route.fulfill({ status: 201, json: project });
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: '/work/demo' } });
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
    // A request carrying the `text` search param is the workspace search (returns the search-only
    // match); without it, it is the base-view refresh (returns only the base rows, no match).
    if (path.endsWith('/tickets') && request.method() === 'GET') {
      const counts = {
        total: 1,
        queued: 1,
        backlog: 0,
        archive: 0,
        open: 1,
        up_next: 0,
        active: 1,
        started: 1,
        completed_today: 0,
        completion_trend: [0, 0, 0, 0, 0, 0, 0],
      };
      return url.searchParams.has('text')
        ? route.fulfill({ json: { items: [match], counts } })
        : route.fulfill({ json: { items: base, counts: { ...counts, total: 2, queued: 2, open: 2, started: 2 } } });
    }
    if (/\/tickets\/HS2-MATCH$/.test(path))
      return route.fulfill({
        json: {
          store: 'git-local',
          ...match,
          details: 'Investigate the parser edge case.',
          blocked_reason: null,
          notes: [],
          attachments: [],
          concurrency_token: 'tok-match',
        },
      });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    // First poll (no cursor) establishes the cursor; later `since` polls carry events for delivery.
    if (path.endsWith('/ws/poll')) {
      if (url.searchParams.get('since') === null)
        return route.fulfill({ json: { cursor: 1, events: [], overflow: false } });
      polls.push(route);
      return;
    }
    if (
      path.endsWith('/terminals') ||
      path.endsWith('/connections') ||
      path.endsWith('/commands') ||
      path.endsWith('/command-runs') ||
      path.endsWith('/views') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    return route.continue();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-ticket-slug="HS2-BASE1"]')).toBeVisible();

  // Run a search and select the search-only match.
  await page.getByRole('button', { name: 'Search tickets' }).click();
  const search = page.getByRole('searchbox', { name: 'Search tickets' });
  await search.fill('parser ');
  await expect(page.locator('[data-ticket-slug="HS2-MATCH"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-BASE1"]')).toHaveCount(0);
  await page.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-MATCH"]').click();
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await expect(inspector).toBeVisible();
  await expect(inspector).toHaveAttribute('data-ticket-slug', 'HS2-MATCH');

  // Start editing the details field so it holds focus.
  await inspector.locator('[data-action="edit-markdown"]').first().dblclick();
  const editor = inspector.locator('[name="markdown-source"]');
  await expect(editor).toBeFocused();

  // A background change event (e.g. from an autosave round-trip) triggers refreshProject. The base
  // refresh does NOT contain HS2-MATCH, but the search selection — and the focused editor — must survive.
  await expect.poll(() => polls.length).toBeGreaterThan(0);
  await polls.shift()!.fulfill({
    json: {
      cursor: 2,
      events: [{ store: 'git-local', kind: 'changed', id: 'HS2-BASE1', slug: 'HS2-BASE1' }],
      overflow: false,
    },
  });

  await expect(inspector).toBeVisible();
  await expect(inspector).toHaveAttribute('data-ticket-slug', 'HS2-MATCH');
  await expect(editor).toBeFocused();
  const matchRow = page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-MATCH"]');
  await expect(matchRow).toBeVisible();
  await expect(matchRow).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-BASE1"]')).toHaveCount(0);
});
