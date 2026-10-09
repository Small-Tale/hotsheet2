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
      path = url.pathname.replace(/\/tickets\/git-local%3A(?=[^/]+)/i, '/tickets/'),
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
  await expect(page.locator('#app-right-rail')).not.toContainText('ticket was modified');
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
      path = url.pathname.replace(/\/tickets\/git-local%3A(?=[^/]+)/i, '/tickets/'),
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

// HS2-RE1PS6: appending to the same line again while an earlier details autosave is still in flight must
// base the next save on the committed text, never treat the user's own saved words as a concurrent edit.
test('typing never writes to the server and one blur saves the whole edit without a merge prompt (HS2-RE1PS6)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  let token = 'T0',
    tokenSeq = 0,
    details = 'Intro',
    firstPatchDelayed = false,
    conflicts = 0;
  const patches: string[] = [];
  const full = () => ({
    store: 'git-local',
    ...listRow,
    details,
    blocked_reason: null,
    notes: [],
    attachments: [],
    concurrency_token: token,
  });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname.replace(/\/tickets\/git-local%3A(?=[^/]+)/i, '/tickets/'),
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
      const body = request.postDataJSON() as { details?: string; expected_token?: string };
      if (body.expected_token && body.expected_token !== token) {
        conflicts += 1;
        return route.fulfill({ status: 409, json: { error: 'ticket changed since it was read' } });
      }
      if (body.details !== undefined) {
        details = body.details;
        patches.push(body.details);
      }
      tokenSeq += 1;
      token = `T${tokenSeq}`;
      const snapshot = full();
      if (!firstPatchDelayed) {
        firstPatchDelayed = true;
        await new Promise((resolve) => setTimeout(resolve, 600));
      }
      return route.fulfill({ json: snapshot });
    }
    if (/\/tickets\/01$/.test(path) && method === 'GET') return route.fulfill({ json: full() });
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
  const inspector = page.locator('#app-right-rail');
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  const editor = inspector.getByRole('textbox', { name: 'Ticket details' });

  // The user keeps appending at the same spot with pauses longer than the old autosave debounce:
  // nothing reaches the server, but the local recovery copy follows the draft on its edit-start base.
  const storedDraft = () =>
    page.evaluate(() => {
      const entry = Object.entries(localStorage).find(([key]) => key.startsWith('hotsheet.ticket-draft:'));
      return entry ? (JSON.parse(entry[1]) as { base: string; draft: string }) : undefined;
    });
  await editor.fill('Intro\n- the sign in');
  await page.waitForTimeout(400);
  await editor.fill('Intro\n- the sign in flow is');
  await page.waitForTimeout(400);
  await editor.fill('Intro\n- the sign in flow is quite awkward');
  await expect.poll(storedDraft).toMatchObject({ base: 'Intro', draft: 'Intro\n- the sign in flow is quite awkward' });
  expect(tokenSeq).toBe(0);
  expect(patches).toEqual([]);
  await expect(page.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);

  // Focus leaves the editor: exactly one write carries the whole edit, and the recovery copy is cleared.
  await editor.blur();
  await expect.poll(() => details).toBe('Intro\n- the sign in flow is quite awkward');
  expect(patches).toEqual(['Intro\n- the sign in flow is quite awkward']);
  expect(conflicts).toBe(0);
  await expect(page.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
  await expect.poll(storedDraft).toBeUndefined();

  // Continuing after the save bases the next write off the committed token; still no prompt.
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  await editor.fill('Intro\n- the sign in flow is quite awkward\n- and the sign out too');
  await page.waitForTimeout(400);
  expect(tokenSeq).toBe(1);
  await editor.blur();
  await expect.poll(() => details).toBe('Intro\n- the sign in flow is quite awkward\n- and the sign out too');
  expect(conflicts).toBe(0);
  expect(tokenSeq).toBe(2);
  await expect(page.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
});

test('restores an unsaved edit from its local recovery copy after a reload (HS2-RE1PS6)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  let details = 'Intro',
    token = 'T0';
  const patches: Array<{ details?: string; expected_token?: string }> = [];
  const full = () => ({
    store: 'git-local',
    ...listRow,
    details,
    blocked_reason: null,
    notes: [],
    attachments: [],
    concurrency_token: token,
  });
  await page.route('**/*', (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname.replace(/\/tickets\/git-local%3A(?=[^/]+)/i, '/tickets/'),
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
      const body = request.postDataJSON() as { details?: string; expected_token?: string };
      patches.push(body);
      if (body.details !== undefined) details = body.details;
      token = 'T1';
      return route.fulfill({ json: full() });
    }
    if (/\/tickets\/01$/.test(path) && method === 'GET') return route.fulfill({ json: full() });
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
  let openedOnce = false;
  const open = async () => {
    await page.goto('/');
    const row = page.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-EDIT"]');
    if (!openedOnce) {
      await page.getByRole('button', { name: 'Open project' }).click();
      await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
      openedOnce = true;
    }
    // After reload, the remembered project must restore the row without another open.
    await expect(row).toBeVisible();
    await row.click();
    const inspector = page.locator('#app-right-rail');
    await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
    return inspector.getByRole('textbox', { name: 'Ticket details' });
  };
  let editor = await open();
  await editor.fill('Intro\n- typed before the crash');
  await expect
    .poll(() => page.evaluate(() => Object.keys(localStorage).some((key) => key.startsWith('hotsheet.ticket-draft:'))))
    .toBe(true);
  expect(patches).toEqual([]);
  // The page goes away without the editor ever losing focus (a crash, a closed tab).
  editor = await open();
  await expect(editor).toHaveValue('Intro\n- typed before the crash');
  await expect(page.locator('.app-toast')).toContainText('Restored an unsaved edit.');
  await editor.fill('Intro\n- typed before the crash\n- and after it');
  await editor.blur();
  await expect.poll(() => details).toBe('Intro\n- typed before the crash\n- and after it');
  expect(patches).toHaveLength(1);
  expect(patches[0].expected_token).toBe('T0');
  await expect(page.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
  await expect
    .poll(() => page.evaluate(() => Object.keys(localStorage).some((key) => key.startsWith('hotsheet.ticket-draft:'))))
    .toBe(false);
});

// HS2-RE1PS6: adding an attachment refreshes the project outside the local-mutation barrier. That refresh's
// ticket read could be answered with the pre-save details and land after the details autosave committed, so
// the user's own older text looked like a remote edit ("Their latest version") and raised a merge prompt —
// or, when the user had stopped typing, silently replaced the draft with the older text.
for (const keepTyping of [true, false])
  test(`an attachment refresh racing a details autosave never ${keepTyping ? 'conflicts' : 'reverts'} the draft (HS2-RE1PS6)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    let token = 'T0',
      tokenSeq = 0,
      details = 'Intro',
      attachments: Array<{ id: string; filename: string; created_at: string }> = [];
    const patches: string[] = [],
      gets: Array<() => void> = [];
    let releasePatch: (() => void) | undefined,
      holdReads = false;
    const full = () => ({
      store: 'git-local',
      ...listRow,
      details,
      blocked_reason: null,
      notes: [],
      attachments,
      concurrency_token: token,
    });
    await page.route('**/*', async (route) => {
      const request = route.request(),
        url = new URL(request.url()),
        path = url.pathname.replace(/\/tickets\/git-local%3A(?=[^/]+)/i, '/tickets/'),
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
      if (/\/tickets\/01\/attachments$/.test(path) && method === 'POST') {
        attachments = [...attachments, { id: 'A1', filename: 'proof.txt', created_at: '2026-09-14T00:00:00Z' }];
        return route.fulfill({ status: 201, json: full() });
      }
      if (/\/tickets\/01$/.test(path) && method === 'PATCH') {
        const body = request.postDataJSON() as { details?: string; expected_token?: string };
        // The first save is held open so the attachment refresh can read the ticket before it commits.
        if (patches.length === 0) await new Promise<void>((resolve) => (releasePatch = resolve));
        if (body.expected_token && body.expected_token !== token)
          return route.fulfill({ status: 409, json: { error: 'ticket changed since it was read' } });
        if (body.details !== undefined) {
          details = body.details;
          patches.push(body.details);
        }
        tokenSeq += 1;
        token = `T${tokenSeq}`;
        return route.fulfill({ json: full() });
      }
      if (/\/tickets\/01$/.test(path) && method === 'GET') {
        // During the race window, snapshot on arrival and answer only when the test releases it.
        const snapshot = full();
        if (!holdReads) return route.fulfill({ json: snapshot });
        await new Promise<void>((resolve) => gets.push(resolve));
        return route.fulfill({ json: snapshot });
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
    const inspector = page.locator('#app-right-rail');
    await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
    const editor = inspector.getByRole('textbox', { name: 'Ticket details' });

    await editor.fill('Intro\n- the sign in');
    // The save starts when focus leaves the editor (HS2-RE1PS6).
    await editor.blur();
    await expect.poll(() => Boolean(releasePatch)).toBe(true);
    // While that save is still open, drop an attachment onto the ticket; its refresh reads the ticket.
    holdReads = true;
    await inspector.evaluate((node) => {
      const transfer = new DataTransfer();
      transfer.items.add(new File(['proof'], 'proof.txt', { type: 'text/plain' }));
      node.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
    });
    // Give the refresh time to reach the ticket read (it may wait for the save before reading).
    await page.waitForTimeout(400);
    releasePatch?.();
    await expect.poll(() => patches.length).toBe(1);
    const finalText = keepTyping ? 'Intro\n- the sign in flow is quite awkward' : 'Intro\n- the sign in';
    if (keepTyping) {
      // The released save settles the editor into preview; the user reopens it and keeps typing.
      await expect(editor).toBeHidden();
      await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
      await editor.fill(finalText);
      await editor.blur();
      await expect.poll(() => details).toBe(finalText);
    }
    await page.waitForTimeout(300);
    // Now answer every held read; a stale one must not be mistaken for someone else's edit.
    holdReads = false;
    while (gets.length) gets.shift()?.();
    await page.waitForTimeout(500);
    while (gets.length) gets.shift()?.();
    await page.waitForTimeout(300);
    await expect(page.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
    expect(details).toBe(finalText);
    // Reopening shows the saved text; typing on after the stale read lands must still save cleanly on blur.
    if (!(await editor.isVisible())) await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
    await expect(editor).toHaveValue(finalText);
    const later = `${finalText}\n- one more thought`;
    await editor.fill(later);
    await page.waitForTimeout(300);
    expect(details).toBe(finalText);
    await editor.blur();
    await expect.poll(() => details).toBe(later);
    await page.waitForTimeout(300);
    await expect(page.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
    await inspector.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
    await expect(editor).toHaveValue(later);
    await page.screenshot({
      path: `target/visual-captures/hs2-re1ps6-no-merge-prompt-${keepTyping ? 'typing' : 'idle'}.png`,
    });
  });
