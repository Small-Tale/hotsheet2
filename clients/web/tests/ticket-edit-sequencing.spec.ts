import { expect, test } from '@playwright/test';

// HS2-K9SG2R: two rapid edits to the same field of the same ticket used to self-conflict — the second
// edit read the same pre-first concurrency token and got a false "ticket was modified". Per-ticket
// mutation sequencing now bases each edit off the previous edit's committed token (last-write-wins), so
// the user's own sequential edits never conflict, while a genuine external write still does.

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
const listRow = {
  connection_id: 'git-local',
  native_id: '01',
  qualified_id: 'git-local:01',
  id: '01',
  slug: 'HS2-EDIT',
  title: 'Rapid edit ticket',
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
};

test('rapid same-field edits are sequenced without a false conflict (HS2-K9SG2R)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  let token = 'T0',
    tokenSeq = 0,
    priority = 'default',
    firstPatchDelayed = false,
    conflicts = 0;
  const full = () => ({
    store: 'git-local',
    ...listRow,
    priority,
    details: '',
    blocked_reason: null,
    notes: [],
    attachments: [],
    concurrency_token: token,
  });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname,
      method = request.method();
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
    if (/\/tickets\/01$/.test(path) && method === 'PATCH') {
      const body = request.postDataJSON() as { priority?: string; expected_token?: string };
      // Token is checked/advanced on arrival (like the real server commit) so a stale concurrent write conflicts.
      if (body.expected_token && body.expected_token !== token) {
        conflicts += 1;
        return route.fulfill({ status: 409, json: { error: 'ticket changed since it was read' } });
      }
      if (body.priority !== undefined) priority = body.priority;
      tokenSeq += 1;
      token = `T${tokenSeq}`;
      const snapshot = full();
      if (!firstPatchDelayed) {
        firstPatchDelayed = true;
        await new Promise((resolve) => setTimeout(resolve, 400));
      }
      return route.fulfill({ json: snapshot });
    }
    if (/\/tickets\/01$/.test(path) && method === 'GET') return route.fulfill({ json: full() });
    if (path.endsWith('/tickets') && method === 'GET')
      return route.fulfill({
        json: {
          items: [{ ...listRow, priority }],
          counts: {
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
          },
        },
      });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll')) {
      if (url.searchParams.get('since') === null)
        return route.fulfill({ json: { cursor: 1, events: [], overflow: false } });
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
  await page.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-EDIT"]').click();
  const priority_ = page.locator('[name="inspector-priority"]');
  await expect(priority_).toBeVisible();

  // Two rapid same-field edits: the first PATCH response is delayed, so without sequencing the second
  // would fire with the stale token and get a false conflict.
  await priority_.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'high';
    node.dispatchEvent(new Event('change', { bubbles: true }));
    node.value = 'low';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });

  // Both writes are sequenced against advancing tokens (no conflict retry): first high, then low.
  await expect.poll(() => tokenSeq).toBe(2);
  // The second edit based off the first's committed token, so the server never saw a stale write —
  // no 409/conflict round-trip happened at all (before the fix this raised a spurious "ticket was modified").
  expect(conflicts).toBe(0);
  // No false conflict, and the last edit wins.
  await expect(page.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
  await expect(priority_).toHaveJSProperty('value', 'low');
  await expect(page.locator('[data-component="ticket-inspector"]')).not.toContainText('ticket was modified');
});

test('a genuine external same-field write still surfaces a conflict (HS2-K9SG2R)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  // The client loaded the ticket at T0/default; an external writer has since moved it to T1/urgent, but
  // no poll has surfaced that yet. The user's own priority edit must detect the real divergence.
  let presented = false;
  const remote = () => ({
    store: 'git-local',
    ...listRow,
    priority: 'urgent',
    details: '',
    blocked_reason: null,
    notes: [],
    attachments: [],
    concurrency_token: 'T1',
  });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname,
      method = request.method();
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
    if (/\/tickets\/01$/.test(path) && method === 'PATCH') {
      const body = request.postDataJSON() as { expected_token?: string };
      if (body.expected_token !== 'T1')
        return route.fulfill({ status: 409, json: { error: 'ticket changed since it was read' } });
      return route.fulfill({ json: remote() });
    }
    // First detail GET (selection) sees T0/default; later GETs (the conflict-path remote fetch) see T1/urgent.
    if (/\/tickets\/01$/.test(path) && method === 'GET') {
      if (!presented) {
        presented = true;
        return route.fulfill({
          json: {
            store: 'git-local',
            ...listRow,
            priority: 'default',
            details: '',
            blocked_reason: null,
            notes: [],
            attachments: [],
            concurrency_token: 'T0',
          },
        });
      }
      return route.fulfill({ json: remote() });
    }
    if (path.endsWith('/tickets') && method === 'GET')
      return route.fulfill({
        json: {
          items: [listRow],
          counts: {
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
          },
        },
      });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll')) {
      if (url.searchParams.get('since') === null)
        return route.fulfill({ json: { cursor: 1, events: [], overflow: false } });
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
  await page.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-EDIT"]').click();
  const priority_ = page.locator('[name="inspector-priority"]');
  await expect(priority_).toBeVisible();
  await priority_.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'high';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  // The real external divergence (default → urgent, vs the user's high) must raise the field conflict.
  await expect(page.locator('[data-component="ticket-field-conflict"]')).toBeVisible();
});
