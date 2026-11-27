import { expect, type Locator, type Page, type Route, test } from '@playwright/test';

import { appRequests } from './app-requests';
import { openMockedProject } from './open-mocked-project';

// HS2-7D3MPP: the ticket editor never writes while the user types and saves once when focus leaves
// (HS2-RE1PS6). These walk the transitions around that rule through the shipped inspector: a reload
// whose recovery copy conflicts with a newer server value, page hide mid-edit, edits on either side
// of a held save, a selection change inside the typing pause, a failed save, and a stale cached
// detail refreshed after the edit began.

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
const row = (id: string, slug: string, title: string) => ({
  connection_id: 'git-local',
  native_id: id,
  qualified_id: `git-local:${id}`,
  id,
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
const rows = [row('01', 'HS2-EDIT', 'Edited ticket'), row('02', 'HS2-NEXT', 'Neighbour ticket')];

interface Patch {
  id: string;
  details?: string;
  expected_token?: string;
}

/** A two-ticket mocked project whose ticket writes and reads the test can hold, fail, or change. */
async function mockProject(page: Page) {
  const server = {
    details: { '01': 'Intro', '02': 'Neighbour' } as Record<string, string>,
    tokens: { '01': 'T0', '02': 'U0' } as Record<string, string>,
    patches: [] as Patch[],
    conflicts: 0,
    /** Answer a PATCH: return a status to fail it, or hold it until a promise settles. */
    onPatch: undefined as undefined | ((patch: Patch) => Promise<number | undefined> | number | undefined),
    /** Hold a detail read until the promise settles; the answer is the server state then. */
    onGet: undefined as undefined | ((id: string) => Promise<void> | undefined),
    /** Long-poll requests waiting for a change event (HS2-JJ3R1G). */
    polls: [] as Route[],
  };
  let tokenSeq = 0;
  const full = (id: string) => ({
    store: 'git-local',
    ...rows.find((item) => item.id === id),
    details: server.details[id],
    blocked_reason: null,
    notes: [],
    attachments: [],
    concurrency_token: server.tokens[id],
  });
  await page.route(appRequests, async (route: Route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname.replace(/\/tickets\/git-local%3A(?=[^/]+)/i, '/tickets/'),
      method = request.method(),
      ticket = /\/tickets\/(0[12])$/.exec(path)?.[1];
    if (/^\/__hotsheet\/(?:remote\/)?projects\/open$/.test(path)) return route.fulfill({ status: 201, json: project });
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: '/work/demo' } });
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
    if (ticket && method === 'PATCH') {
      const body = { id: ticket, ...(request.postDataJSON() as Omit<Patch, 'id'>) };
      server.patches.push(body);
      const status = await server.onPatch?.(body);
      if (status !== undefined)
        return route.fulfill({ status, json: { error: 'The ticket store is unavailable.', code: 'unavailable' } });
      if (body.expected_token && body.expected_token !== server.tokens[ticket]) {
        server.conflicts += 1;
        return route.fulfill({
          status: 409,
          json: { error: `ticket '${ticket}' changed since it was read`, code: 'ticket_changed' },
        });
      }
      if (body.details !== undefined) server.details[ticket] = body.details;
      tokenSeq += 1;
      server.tokens[ticket] = `${ticket === '01' ? 'T' : 'U'}${tokenSeq}`;
      return route.fulfill({ json: full(ticket) });
    }
    if (ticket && method === 'GET') {
      await server.onGet?.(ticket);
      return route.fulfill({ json: full(ticket) });
    }
    if (path.endsWith('/tickets') && method === 'GET')
      return route.fulfill({
        json: {
          items: rows,
          counts: {
            total: 2,
            queued: 2,
            backlog: 0,
            archive: 0,
            open: 2,
            up_next: 0,
            active: 2,
            started: 2,
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
      server.polls.push(route);
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
  return server;
}

/** A promise the test settles by hand, for holding one mocked request open. */
function gate() {
  let open!: () => void;
  const opened = new Promise<void>((resolve) => {
    open = resolve;
  });
  return { opened, open };
}

const storedDrafts = (page: Page) =>
  page.evaluate(() =>
    Object.entries(localStorage)
      .filter(([key]) => key.startsWith('hotsheet.ticket-draft:'))
      .map(([, value]) => {
        const { base, draft } = JSON.parse(value) as { base: string; draft: string };
        return { base, draft };
      }),
  );

async function openProject(page: Page) {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  await openMockedProject(page);
}
function ticketRow(page: Page, slug: string) {
  return page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${slug}"]`);
}
async function editDetails(page: Page) {
  const inspector = page.locator('#app-right-rail');
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).click();
  const editor = inspector.getByRole('textbox', { name: 'Ticket details' });
  await expect(editor).toBeFocused();
  return editor;
}
/** Put the caret after the last character, as a click at the end of the text would. */
async function caretToEnd(editor: Locator) {
  await editor.evaluate((node: HTMLTextAreaElement) => {
    node.setSelectionRange(node.value.length, node.value.length);
  });
}
async function setPageVisibility(page: Page, state: 'hidden' | 'visible') {
  await page.evaluate((next) => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => next });
    document.dispatchEvent(new Event('visibilitychange'));
  }, state);
}

test('a reload whose recovery copy conflicts with a newer server value keeps the text in a conflict card (HS2-7D3MPP)', async ({
  page,
}) => {
  const server = await mockProject(page);
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  let editor = await editDetails(page);
  await editor.fill('Intro rewritten here');
  await expect.poll(() => storedDrafts(page)).toEqual([{ base: 'Intro', draft: 'Intro rewritten here' }]);
  expect(server.patches).toEqual([]);

  // The page goes away mid-edit; meanwhile someone else rewrites the same text on the server.
  server.details['01'] = 'Intro replaced elsewhere';
  server.tokens['01'] = 'T-remote';
  await page.reload();
  await ticketRow(page, 'HS2-EDIT').click();
  editor = await editDetails(page);

  // The edit is not discarded: the conflict card shows both versions, the editor opens on the
  // server's text, and the recovery copy survives until the user resolves the card.
  const card = page.locator('[data-component="ticket-field-conflict"]');
  await expect(card).toBeVisible();
  await expect(card).toContainText('Intro replaced elsewhere');
  await expect(card).toContainText('Intro rewritten here');
  await expect(page.getByText('was discarded')).toHaveCount(0);
  await expect(editor).toHaveValue('Intro replaced elsewhere');
  expect(await storedDrafts(page)).toEqual([{ base: 'Intro', draft: 'Intro rewritten here' }]);

  // Another reload before resolving still offers the same choice.
  await page.reload();
  await ticketRow(page, 'HS2-EDIT').click();
  editor = await editDetails(page);
  await expect(card).toContainText('Intro rewritten here');

  // Applying the merged text saves it once against the server's token and clears the copy.
  const merged = card.getByRole('textbox', { name: 'Merged details' });
  await merged.fill('Intro replaced elsewhere, then rewritten here');
  await card.getByRole('button', { name: 'Apply' }).click();
  await expect(card).toHaveCount(0);
  await expect.poll(() => server.details['01']).toBe('Intro replaced elsewhere, then rewritten here');
  expect(server.patches.at(-1)?.expected_token).toBe('T-remote');
  expect(server.conflicts).toBe(0);
  await expect.poll(() => storedDrafts(page)).toEqual([]);

  // Reopening does not raise the old conflict again.
  if (await editor.isVisible()) await editor.blur();
  await page.reload();
  await ticketRow(page, 'HS2-EDIT').click();
  editor = await editDetails(page);
  await expect(editor).toHaveValue('Intro replaced elsewhere, then rewritten here');
  await expect(card).toHaveCount(0);
});

test('Use Theirs on a reload conflict clears the recovery copy so reopening is clean (HS2-7D3MPP)', async ({
  page,
}) => {
  const server = await mockProject(page);
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  let editor = await editDetails(page);
  await editor.fill('Intro mine');
  await expect.poll(() => storedDrafts(page)).toHaveLength(1);
  server.details['01'] = 'Intro theirs';
  server.tokens['01'] = 'T-remote';
  await page.reload();
  await ticketRow(page, 'HS2-EDIT').click();
  editor = await editDetails(page);
  const card = page.locator('[data-component="ticket-field-conflict"]');
  await card.getByRole('button', { name: 'Use Theirs' }).click();
  await expect(card).toHaveCount(0);
  await expect(editor).toHaveValue('Intro theirs');
  await expect.poll(() => storedDrafts(page)).toEqual([]);
  await editor.blur();
  expect(server.patches).toEqual([]);
  await page.reload();
  await ticketRow(page, 'HS2-EDIT').click();
  await editDetails(page);
  await expect(card).toHaveCount(0);
});

// HS2-3TSEAX: the reload-conflict card appears on the first click into Details. It sits beneath the
// field, so the clicked control does not move and a habitual double-click lands in the editor instead
// of on a control the card pushed down (the section tabs).
test('a double-click into Details on a reload conflict lands in the editor, which the card does not move (HS2-3TSEAX)', async ({
  page,
}) => {
  const server = await mockProject(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await openMockedProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  let editor = await editDetails(page);
  await editor.fill('Intro rewritten here');
  await expect.poll(() => storedDrafts(page)).toHaveLength(1);
  server.details['01'] = 'Intro replaced elsewhere';
  server.tokens['01'] = 'T-remote';
  await page.reload();
  await ticketRow(page, 'HS2-EDIT').click();

  const inspector = page.locator('#app-right-rail');
  const control = inspector.getByRole('button', { name: 'Edit Ticket details' });
  await expect(control).toBeVisible();
  const before = await control.boundingBox();
  if (!before) throw new Error('Edit Ticket details has no box');
  const x = before.x + before.width / 2,
    y = before.y + Math.min(before.height / 2, 12);
  await page.mouse.dblclick(x, y);

  const card = inspector.locator('[data-component="ticket-field-conflict"]');
  await expect(card).toBeVisible();
  editor = inspector.getByRole('textbox', { name: 'Ticket details' });
  await expect(editor).toBeFocused();
  await expect(editor).toHaveValue('Intro replaced elsewhere');
  await expect(inspector.locator('[data-inspector-tab="info"]')).toHaveAttribute('data-selected', 'true');
  // The editor opened where the clicked control was, and the card follows it.
  const after = await editor.boundingBox(),
    cardBox = await card.boundingBox();
  if (!after || !cardBox) throw new Error('The editor or the card has no box');
  expect(Math.abs(after.y - before.y)).toBeLessThan(24);
  expect(y).toBeGreaterThanOrEqual(after.y);
  expect(y).toBeLessThanOrEqual(after.y + after.height);
  expect(cardBox.y).toBeGreaterThanOrEqual(after.y + after.height);
  // The card is in the Info body, not the inspector header above the section tabs.
  await expect(inspector.locator('.ticket-info-panel [data-component="ticket-field-conflict"]')).toHaveCount(1);

  // Moving to another section keeps the card visible in the header until it is resolved.
  await inspector.locator('[data-inspector-tab="timeline"]').click();
  await expect(card).toBeVisible();
  await expect(inspector.locator('.ticket-info-panel [data-component="ticket-field-conflict"]')).toHaveCount(0);
  await card.getByRole('button', { name: 'Use Theirs' }).click();
  await expect(card).toHaveCount(0);
  expect(server.patches).toEqual([]);
});

test('page hide saves an open details edit; typing on after returning saves once more without a conflict (HS2-7D3MPP)', async ({
  page,
}) => {
  const server = await mockProject(page);
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  const editor = await editDetails(page);
  await editor.fill('Intro\n- before hiding');
  expect(server.patches).toEqual([]);

  await setPageVisibility(page, 'hidden');
  await expect.poll(() => server.details['01']).toBe('Intro\n- before hiding');
  expect(server.patches).toHaveLength(1);
  expect(server.patches[0].expected_token).toBe('T0');
  await expect.poll(() => storedDrafts(page)).toEqual([]);

  // The user comes back to the still-open editor and keeps typing; nothing is sent until blur.
  await setPageVisibility(page, 'visible');
  await expect(editor).toBeVisible();
  await expect(editor).toHaveValue('Intro\n- before hiding');
  await editor.focus();
  await editor.fill('Intro\n- before hiding\n- after returning');
  await page.waitForTimeout(300);
  expect(server.patches).toHaveLength(1);
  await editor.blur();
  await expect.poll(() => server.details['01']).toBe('Intro\n- before hiding\n- after returning');
  expect(server.patches).toHaveLength(2);
  expect(server.patches[1].expected_token).not.toBe('T0');
  expect(server.conflicts).toBe(0);
  await expect(page.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
});

for (const order of ['before', 'after'] as const)
  test(`an edit made ${order} a held blur save answers is saved by the next blur without a prompt (HS2-7D3MPP)`, async ({
    page,
  }) => {
    const server = await mockProject(page);
    const held = gate();
    let arrived = false;
    server.onPatch = async () => {
      if (server.patches.length !== 1) return undefined;
      arrived = true;
      await held.opened;
      return undefined;
    };
    await openProject(page);
    await ticketRow(page, 'HS2-EDIT').click();
    let editor = await editDetails(page);
    await editor.fill('Intro\n- first thought');
    await editor.blur();
    await expect.poll(() => arrived).toBe(true);

    if (order === 'before') {
      // The first save is still open: the editor stays in write mode, and the user goes back in.
      await expect(editor).toBeVisible();
      await editor.focus();
      await editor.fill('Intro\n- first thought\n- second thought');
      await page.waitForTimeout(300);
      expect(server.patches).toHaveLength(1);
      held.open();
      await expect.poll(() => server.details['01']).toBe('Intro\n- first thought');
      // The answer must not close the editor under the user's caret or drop the newer text.
      await page.waitForTimeout(300);
      await expect(editor).toBeVisible();
      await expect(editor).toHaveValue('Intro\n- first thought\n- second thought');
      expect(server.patches).toHaveLength(1);
    } else {
      held.open();
      await expect.poll(() => server.details['01']).toBe('Intro\n- first thought');
      await expect(editor).toBeHidden();
      editor = await editDetails(page);
      await expect(editor).toHaveValue('Intro\n- first thought');
      await editor.fill('Intro\n- first thought\n- second thought');
      await page.waitForTimeout(300);
      expect(server.patches).toHaveLength(1);
    }
    await editor.blur();
    await expect.poll(() => server.details['01']).toBe('Intro\n- first thought\n- second thought');
    expect(server.patches).toHaveLength(2);
    expect(server.conflicts).toBe(0);
    await expect(page.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
    await expect.poll(() => storedDrafts(page)).toEqual([]);
  });

for (const caret of ['start', 'end'] as const)
  test(`selecting another ticket inside the typing pause saves an edit at the ${caret} to its own ticket (HS2-7D3MPP)`, async ({
    page,
  }) => {
    const typed = caret === 'end' ? 'Intro and more' : ' and moreIntro';
    const server = await mockProject(page);
    await openProject(page);
    await ticketRow(page, 'HS2-EDIT').click();
    const editor = await editDetails(page);
    await expect(editor).toHaveValue('Intro');
    // Type and select the neighbour at once: the 150 ms recovery pause has not elapsed, so no local
    // copy exists yet when the selection changes.
    if (caret === 'end') await caretToEnd(editor);
    await editor.pressSequentially(' and more', { delay: 0 });
    await expect(editor).toHaveValue(typed);
    await ticketRow(page, 'HS2-NEXT').click();

    await expect.poll(() => server.details['01']).toBe(typed);
    expect(server.patches.map(({ id, details }) => ({ id, details }))).toEqual([{ id: '01', details: typed }]);
    expect(server.details['02']).toBe('Neighbour');
    await expect(page.locator('#app-right-rail')).toContainText('Neighbour');
    await expect.poll(() => storedDrafts(page)).toEqual([]);

    // Back on the edited ticket, the saved text is shown and no stale draft is restored.
    await ticketRow(page, 'HS2-EDIT').click();
    const reopened = await editDetails(page);
    await expect(reopened).toHaveValue(typed);
    await expect(page.getByText('Restored an unsaved edit.')).toHaveCount(0);
  });

test('a failed blur save is shown, keeps the text, and the edit can be retried (HS2-7D3MPP)', async ({ page }) => {
  const server = await mockProject(page);
  let failing = true;
  server.onPatch = () => (failing ? 503 : undefined);
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  let editor = await editDetails(page);
  await editor.fill('Intro\n- must not be lost');
  await editor.blur();

  // The failure is visible, the editor keeps the text, and the local copy stays.
  await expect(page.locator('[data-component="app-error"]')).toContainText('The ticket store is unavailable.');
  await expect(editor).toBeVisible();
  await expect(editor).toHaveValue('Intro\n- must not be lost');
  expect(server.patches).toHaveLength(1);
  expect(await storedDrafts(page)).toEqual([{ base: 'Intro', draft: 'Intro\n- must not be lost' }]);

  // Focus in and out again with no new typing: nothing is resent, the editor settles to the
  // server's text, and the local copy remains (HS2-7D3MPP follow-up tracks keeping it open).
  await editor.focus();
  await editor.blur();
  await expect(editor).toBeHidden();
  expect(server.patches).toHaveLength(1);
  expect(await storedDrafts(page)).toEqual([{ base: 'Intro', draft: 'Intro\n- must not be lost' }]);

  // Retry: reopening the editor restores the edit from its local copy, and blur saves it.
  failing = false;
  editor = await editDetails(page);
  await expect(editor).toHaveValue('Intro\n- must not be lost');
  await expect(page.locator('.app-toast')).toContainText('Restored an unsaved edit.');
  await editor.blur();
  await expect.poll(() => server.details['01']).toBe('Intro\n- must not be lost');
  expect(server.conflicts).toBe(0);
  await expect.poll(() => storedDrafts(page)).toEqual([]);
});

test('an edit begun on a stale cached detail merges with the refresh that lands after it (HS2-7D3MPP)', async ({
  page,
}) => {
  const server = await mockProject(page);
  await openProject(page);
  // Load the ticket once so its detail is cached, then move away.
  await ticketRow(page, 'HS2-EDIT').click();
  await expect(page.locator('#app-right-rail')).toContainText('Intro');
  await ticketRow(page, 'HS2-NEXT').click();
  await expect(page.locator('#app-right-rail')).toContainText('Neighbour');

  // Someone appends to the ticket; the cached copy is now stale and its refresh is held.
  server.details['01'] = 'Intro\n\nAppended elsewhere';
  server.tokens['01'] = 'T-remote';
  const held = gate();
  let reading = false;
  server.onGet = (id) => {
    if (id !== '01') return undefined;
    reading = true;
    return held.opened;
  };
  await ticketRow(page, 'HS2-EDIT').click();
  await expect.poll(() => reading).toBe(true);
  const editor = await editDetails(page);
  await expect(editor).toHaveValue('Intro');
  await editor.fill('Intro, edited');

  // The refresh lands mid-edit: the edit stays as typed, and nothing is written yet.
  held.open();
  await page.waitForTimeout(300);
  await expect(editor).toHaveValue('Intro, edited');
  expect(server.patches).toEqual([]);

  // Blur saves a three-way merge against the refreshed token: both edits survive, no prompt.
  await editor.blur();
  await expect.poll(() => server.details['01']).toBe('Intro, edited\n\nAppended elsewhere');
  // One write on the refreshed token, not a stale cached token answered by a 409 retry.
  expect(server.patches.map((patch) => patch.expected_token)).toEqual(['T-remote']);
  expect(server.conflicts).toBe(0);
  await expect(page.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
  // The merge is announced once, without blocking (HS2-JJ3R1G).
  await expect(page.locator('.app-toast')).toContainText('Merged your edit with a newer change to Details.');
});

test('an editor opened on a stale cached detail adopts the refresh when nothing was typed yet (HS2-7D3MPP)', async ({
  page,
}) => {
  const server = await mockProject(page);
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  await expect(page.locator('#app-right-rail')).toContainText('Intro');
  await ticketRow(page, 'HS2-NEXT').click();
  await expect(page.locator('#app-right-rail')).toContainText('Neighbour');
  server.details['01'] = 'Intro\n\nAppended elsewhere';
  server.tokens['01'] = 'T-remote';
  const held = gate();
  let reading = false;
  server.onGet = (id) => {
    if (id !== '01') return undefined;
    reading = true;
    return held.opened;
  };
  await ticketRow(page, 'HS2-EDIT').click();
  await expect.poll(() => reading).toBe(true);
  const editor = await editDetails(page);
  await expect(editor).toHaveValue('Intro');
  held.open();
  await expect(editor).toHaveValue('Intro\n\nAppended elsewhere');
  await caretToEnd(editor);
  await editor.pressSequentially(' and mine');
  await editor.blur();
  await expect.poll(() => server.details['01']).toBe('Intro\n\nAppended elsewhere and mine');
  expect(server.patches.map((patch) => patch.expected_token)).toEqual(['T-remote']);
  expect(server.conflicts).toBe(0);
});

// HS2-JJ3R1G: a conflict is resolved automatically whenever both edits can be kept; the field-conflict
// card appears only for edits that overlap.
const conflictCard = (page: Page) => page.locator('[data-component="ticket-field-conflict"]');
const draftKey = 'hotsheet.ticket-draft:demo:git-local:01:details';

test('a reload whose recovery copy touches other lines than the newer server value merges without a card (HS2-JJ3R1G)', async ({
  page,
}) => {
  const server = await mockProject(page);
  server.details['01'] = 'Intro\n\nMiddle\n\nEnd';
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  let editor = await editDetails(page);
  await editor.fill('Intro, mine\n\nMiddle\n\nEnd');
  await expect.poll(() => storedDrafts(page)).toHaveLength(1);

  // The page goes away mid-edit; meanwhile someone else edits a different paragraph.
  server.details['01'] = 'Intro\n\nMiddle\n\nEnd, theirs';
  server.tokens['01'] = 'T-remote';
  await page.reload();
  await ticketRow(page, 'HS2-EDIT').click();
  editor = await editDetails(page);

  // No card: the editor reopens on both edits and says so in a toast; nothing is written yet.
  await expect(editor).toHaveValue('Intro, mine\n\nMiddle\n\nEnd, theirs');
  await expect(page.locator('.app-toast')).toContainText(
    'Restored an unsaved edit and merged it with a newer change to Details.',
  );
  await expect(conflictCard(page)).toHaveCount(0);
  expect(server.patches).toEqual([]);

  // Typing on after the merge keeps both edits; one focus-loss save writes against the current token.
  await caretToEnd(editor);
  await editor.pressSequentially('\n\nAfter the merge');
  await page.waitForTimeout(300);
  expect(server.patches).toEqual([]);
  await editor.blur();
  await expect.poll(() => server.details['01']).toBe('Intro, mine\n\nMiddle\n\nEnd, theirs\n\nAfter the merge');
  expect(server.patches.map((patch) => patch.expected_token)).toEqual(['T-remote']);
  expect(server.conflicts).toBe(0);
  await expect.poll(() => storedDrafts(page)).toEqual([]);
});

test('a reload after the merged save committed restores nothing and clears the copy (HS2-JJ3R1G)', async ({ page }) => {
  const server = await mockProject(page);
  await openProject(page);
  // The page reloaded while a merged save was in flight; that save committed, but its copy remained.
  server.details['01'] = 'Intro, mine\n\nEnd, theirs';
  await page.evaluate(
    ([key, value]) => {
      localStorage.setItem(key, value);
    },
    [draftKey, JSON.stringify({ base: 'Intro\n\nEnd', draft: 'Intro, mine\n\nEnd', at: 1 })],
  );
  await page.reload();
  await ticketRow(page, 'HS2-EDIT').click();
  const editor = await editDetails(page);
  await expect(editor).toHaveValue('Intro, mine\n\nEnd, theirs');
  await expect(conflictCard(page)).toHaveCount(0);
  await expect(page.getByText('Restored an unsaved edit')).toHaveCount(0);
  await expect.poll(() => storedDrafts(page)).toEqual([]);
  await editor.blur();
  expect(server.patches).toEqual([]);
});

test('a recovery copy without a merge base falls back to the conflict card (HS2-JJ3R1G)', async ({ page }) => {
  const server = await mockProject(page);
  await openProject(page);
  await page.evaluate(
    ([key, value]) => {
      localStorage.setItem(key, value);
    },
    [draftKey, JSON.stringify({ draft: 'Intro from an older copy', at: 1 })],
  );
  await page.reload();
  await ticketRow(page, 'HS2-EDIT').click();
  const editor = await editDetails(page);
  const card = conflictCard(page);
  await expect(card).toBeVisible();
  await expect(card).toContainText('Intro from an older copy');
  await expect(editor).toHaveValue('Intro');
  await card.getByRole('button', { name: 'Use Theirs' }).click();
  await expect(card).toHaveCount(0);
  await expect.poll(() => page.evaluate((key) => localStorage.getItem(key), draftKey)).toBeNull();
  expect(server.patches).toEqual([]);
});

test('a save answered by a 409 merges a disjoint concurrent edit and retries once (HS2-JJ3R1G)', async ({ page }) => {
  const server = await mockProject(page);
  server.details['01'] = 'Intro\n\nEnd';
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  const editor = await editDetails(page);
  await editor.fill('Intro, mine\n\nEnd');
  // Someone edits another paragraph before focus leaves; the client still holds the old token.
  server.details['01'] = 'Intro\n\nEnd, theirs';
  server.tokens['01'] = 'T-remote';
  await editor.blur();
  await expect.poll(() => server.details['01']).toBe('Intro, mine\n\nEnd, theirs');
  expect(server.conflicts).toBe(1);
  expect(server.patches.map((patch) => patch.expected_token)).toEqual(['T0', 'T-remote']);
  await expect(conflictCard(page)).toHaveCount(0);
  await expect(page.locator('.app-toast')).toContainText('Merged your edit with a newer change to Details.');
  await expect(page.locator('#app-right-rail')).toContainText('End, theirs');
  await expect.poll(() => storedDrafts(page)).toEqual([]);
});

test('a save answered by a 409 whose concurrent edit overlaps still shows the card (HS2-JJ3R1G)', async ({ page }) => {
  const server = await mockProject(page);
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  const editor = await editDetails(page);
  await editor.fill('Intro, mine');
  server.details['01'] = 'Intro, theirs';
  server.tokens['01'] = 'T-remote';
  await editor.blur();
  const card = conflictCard(page);
  await expect(card).toBeVisible();
  await expect(card).toContainText('Intro, mine');
  await expect(card).toContainText('Intro, theirs');
  expect(server.details['01']).toBe('Intro, theirs');
  await expect(page.getByText('Merged your edit')).toHaveCount(0);
});

test('a live change merged into an open unsaved draft is announced and saved on the next blur (HS2-JJ3R1G)', async ({
  page,
}) => {
  const server = await mockProject(page);
  server.details['01'] = 'Intro\n\nEnd';
  let failing = true;
  server.onPatch = () => (failing ? 503 : undefined);
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  const editor = await editDetails(page);
  await editor.fill('Intro, mine\n\nEnd');
  // The save fails, so the editor stays open on unsaved text with nothing waiting to send.
  await editor.blur();
  await expect(page.locator('[data-component="app-error"]')).toContainText('The ticket store is unavailable.');
  await expect(editor).toHaveValue('Intro, mine\n\nEnd');

  // Another writer edits a different paragraph and the change stream reports it.
  failing = false;
  server.details['01'] = 'Intro\n\nEnd, theirs';
  server.tokens['01'] = 'T-remote';
  await expect.poll(() => server.polls.length).toBeGreaterThan(0);
  await server.polls.shift()!.fulfill({
    json: { cursor: 2, events: [{ store: 'git-local', kind: 'changed', id: '01', slug: 'HS2-EDIT' }], overflow: false },
  });

  // The open draft takes both edits without a card, the merge is announced, and nothing is written yet.
  await expect(editor).toHaveValue('Intro, mine\n\nEnd, theirs');
  await expect(page.locator('.app-toast')).toContainText('Merged your edit with a newer change to Details.');
  await expect(conflictCard(page)).toHaveCount(0);
  expect(server.patches).toHaveLength(1);

  // The next focus loss saves the merged text once on the current token.
  await editor.focus();
  await editor.blur();
  await expect.poll(() => server.details['01']).toBe('Intro, mine\n\nEnd, theirs');
  expect(server.patches.at(-1)?.expected_token).toBe('T-remote');
  expect(server.conflicts).toBe(0);
});
