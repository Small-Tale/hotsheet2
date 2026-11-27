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
  title?: string;
  blocked_reason?: string | null;
  note_id?: string;
  note?: string;
  expected_token?: string;
}
interface Note {
  id: string;
  kind: 'regular';
  created_at: string;
  edited_at: string;
  text: string;
}
const note = (id: string, text: string): Note => ({
  id,
  kind: 'regular',
  created_at: '2026-09-10T00:00:00Z',
  edited_at: '2026-09-10T00:00:00Z',
  text,
});

/** A two-ticket mocked project whose ticket writes and reads the test can hold, fail, or change. */
async function mockProject(page: Page) {
  const server = {
    details: { '01': 'Intro', '02': 'Neighbour' } as Record<string, string>,
    titles: { '01': 'Edited ticket', '02': 'Neighbour ticket' } as Record<string, string>,
    blocked: { '01': null, '02': null } as Record<string, string | null>,
    notes: { '01': [], '02': [] } as Record<string, Note[]>,
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
    title: server.titles[id],
    details: server.details[id],
    blocked_reason: server.blocked[id],
    notes: server.notes[id],
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
      if (body.title !== undefined) server.titles[ticket] = body.title;
      if (body.blocked_reason !== undefined) server.blocked[ticket] = body.blocked_reason;
      if (body.note_id !== undefined && body.note !== undefined)
        server.notes[ticket] = server.notes[ticket].map((item) =>
          item.id === body.note_id ? { ...item, text: body.note ?? item.text } : item,
        );
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

test('a failed blur save is shown, keeps the text, and the next blur retries it (HS2-7D3MPP, HS2-SJ5TCW)', async ({
  page,
}) => {
  const server = await mockProject(page);
  let failing = true;
  server.onPatch = () => (failing ? 503 : undefined);
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  const editor = await editDetails(page);
  await editor.fill('Intro\n- must not be lost');
  await editor.blur();

  // The failure is visible, the editor keeps the text, and the local copy stays.
  const appError = page.locator('[data-component="app-error"]');
  await expect(appError).toContainText('The ticket store is unavailable.');
  await expect(editor).toBeVisible();
  await expect(editor).toHaveValue('Intro\n- must not be lost');
  expect(server.patches).toHaveLength(1);
  expect(await storedDrafts(page)).toEqual([{ base: 'Intro', draft: 'Intro\n- must not be lost' }]);

  // Focus in and out again with no new typing: the save is retried, and while it still fails the
  // editor stays open on the text instead of settling to the server's older value (HS2-SJ5TCW).
  await editor.focus();
  await editor.blur();
  await expect.poll(() => server.patches.length).toBe(2);
  expect(server.patches[1]?.details).toBe('Intro\n- must not be lost');
  await expect(appError).toContainText('The ticket store is unavailable.');
  await expect(editor).toBeVisible();
  await expect(editor).toHaveValue('Intro\n- must not be lost');
  expect(await storedDrafts(page)).toEqual([{ base: 'Intro', draft: 'Intro\n- must not be lost' }]);

  // Once the store answers, the next focus loss saves the text and closes the editor.
  failing = false;
  await editor.focus();
  await editor.blur();
  await expect.poll(() => server.details['01']).toBe('Intro\n- must not be lost');
  await expect(editor).toBeHidden();
  await expect(page.locator('#app-right-rail')).toContainText('must not be lost');
  expect(server.patches).toHaveLength(3);
  expect(server.conflicts).toBe(0);
  await expect.poll(() => storedDrafts(page)).toEqual([]);

  // Nothing is left to retry: opening and leaving the editor again writes nothing.
  const reopened = await editDetails(page);
  await reopened.blur();
  await expect(reopened).toBeHidden();
  expect(server.patches).toHaveLength(3);
});

test('a failed edit is never retried into another ticket, and reopening restores it (HS2-7D3MPP, HS2-SJ5TCW)', async ({
  page,
}) => {
  const server = await mockProject(page);
  let failing = true;
  server.onPatch = () => (failing ? 503 : undefined);
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  let editor = await editDetails(page);
  await editor.fill('Intro\n- must not be lost');
  await editor.blur();
  await expect(page.locator('[data-component="app-error"]')).toContainText('The ticket store is unavailable.');

  // Opening and leaving another ticket's editor writes nothing to it.
  await ticketRow(page, 'HS2-NEXT').click();
  await expect(page.locator('#app-right-rail')).toContainText('Neighbour');
  const neighbour = await editDetails(page);
  await neighbour.blur();
  await expect(neighbour).toBeHidden();
  expect(server.patches.filter((patch) => patch.id === '02')).toEqual([]);

  // Back on the ticket, reopening the editor restores the edit from its local copy, and blur saves it.
  failing = false;
  await ticketRow(page, 'HS2-EDIT').click();
  editor = await editDetails(page);
  await expect(editor).toHaveValue('Intro\n- must not be lost');
  await expect(page.locator('.app-toast')).toContainText('Restored an unsaved edit.');
  await editor.blur();
  await expect.poll(() => server.details['01']).toBe('Intro\n- must not be lost');
  expect(server.conflicts).toBe(0);
  await expect.poll(() => storedDrafts(page)).toEqual([]);
});

test('a retried save that meets an overlapping edit shows the card, and Use Theirs is not resent (HS2-SJ5TCW)', async ({
  page,
}) => {
  const server = await mockProject(page);
  let failing = true;
  server.onPatch = () => (failing ? 503 : undefined);
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  const editor = await editDetails(page);
  await editor.fill('Intro, mine');
  await editor.blur();
  await expect(page.locator('[data-component="app-error"]')).toContainText('The ticket store is unavailable.');

  // Someone changes the same text meanwhile; the retry meets it and the card takes over the edit.
  failing = false;
  server.details['01'] = 'Intro, theirs';
  server.tokens['01'] = 'T-remote';
  await editor.focus();
  await editor.blur();
  const card = conflictCard(page);
  await expect(card).toBeVisible();
  await expect(card).toContainText('Intro, mine');
  await expect(card).toContainText('Intro, theirs');
  const sent = server.patches.length;

  // Leaving the editor while the card is open does not raise it again with another write; the card
  // keeps the edit.
  await editor.focus();
  await editor.blur();
  await expect(card).toBeVisible();
  await expect(card).toContainText('Intro, mine');
  expect(server.patches).toHaveLength(sent);

  // Use Theirs resolves it; opening and leaving the editor afterwards has nothing to retry.
  await card.getByRole('button', { name: 'Use Theirs' }).click();
  await expect(card).toHaveCount(0);
  const reopened = await editDetails(page);
  await expect(reopened).toHaveValue('Intro, theirs');
  await reopened.blur();
  await expect(reopened).toBeHidden();
  expect(server.patches).toHaveLength(sent);
  expect(server.details['01']).toBe('Intro, theirs');
});

// HS2-Y0VRXR: every inspector text editor, not just Details, retries a failed save on the next focus
// loss, through the same destination-keyed autosave retry.
const otherEditors = [
  {
    name: 'note',
    typed: 'A note, edited',
    prepare: (server: Awaited<ReturnType<typeof mockProject>>) => {
      server.notes['01'] = [note('N1', 'A note')];
    },
    open: async (inspector: Locator) => {
      await inspector
        .locator('[data-note-id="N1"] .note-card__body p')
        .first()
        .click({ position: { x: 4, y: 4 } });
      return inspector.getByRole('textbox', { name: 'Note body' });
    },
    saved: (server: Awaited<ReturnType<typeof mockProject>>) => server.notes['01'][0]?.text,
    sent: (patch: Patch) => patch.note,
  },
  {
    name: 'blocked reason',
    typed: 'Waits on the vendor and the auditor',
    prepare: (server: Awaited<ReturnType<typeof mockProject>>) => {
      server.blocked['01'] = 'Waits on the vendor';
    },
    open: async (inspector: Locator) => {
      await inspector.getByRole('button', { name: 'Edit blocked reason' }).dblclick();
      return inspector.getByRole('textbox', { name: 'Blocked Reason' });
    },
    saved: (server: Awaited<ReturnType<typeof mockProject>>) => server.blocked['01'],
    sent: (patch: Patch) => patch.blocked_reason,
  },
  {
    name: 'title',
    typed: 'Edited ticket, renamed',
    prepare: () => {},
    open: async (inspector: Locator) => {
      await inspector.locator('[data-action="edit-ticket-title"]').dblclick();
      return inspector.getByRole('textbox', { name: 'Ticket Title' });
    },
    saved: (server: Awaited<ReturnType<typeof mockProject>>) => server.titles['01'],
    sent: (patch: Patch) => patch.title,
  },
] as const;

for (const editorCase of otherEditors)
  test(`a failed ${editorCase.name} save is retried by the next focus loss and closes once it commits (HS2-Y0VRXR)`, async ({
    page,
  }) => {
    const server = await mockProject(page);
    editorCase.prepare(server);
    let failing = true;
    server.onPatch = () => (failing ? 503 : undefined);
    await openProject(page);
    await ticketRow(page, 'HS2-EDIT').click();
    const inspector = page.locator('#app-right-rail');
    const editor = await editorCase.open(inspector);
    await expect(editor).toBeFocused();
    await editor.fill(editorCase.typed);
    await editor.blur();

    const appError = page.locator('[data-component="app-error"]');
    await expect(appError).toContainText('The ticket store is unavailable.');
    await expect(editor).toBeVisible();
    await expect(editor).toHaveValue(editorCase.typed);
    expect(server.patches).toHaveLength(1);

    // No new typing: the focus loss resends the edit, and the editor stays open while it fails.
    await editor.focus();
    await editor.blur();
    await expect.poll(() => server.patches.length).toBe(2);
    expect(editorCase.sent(server.patches[1])).toBe(editorCase.typed);
    await expect(appError).toContainText('The ticket store is unavailable.');
    await expect(editor).toBeVisible();
    await expect(editor).toHaveValue(editorCase.typed);

    // Once the store answers, the next focus loss saves it and closes the editor.
    failing = false;
    await editor.focus();
    await editor.blur();
    await expect.poll(() => editorCase.saved(server)).toBe(editorCase.typed);
    await expect(editor).toBeHidden();
    expect(server.patches).toHaveLength(3);
    await expect.poll(() => storedDrafts(page)).toEqual([]);

    // Nothing is left to retry: opening and leaving the editor again writes nothing.
    const reopened = await editorCase.open(inspector);
    await expect(reopened).toHaveValue(editorCase.typed);
    await reopened.blur();
    await expect(reopened).toBeHidden();
    expect(server.patches).toHaveLength(3);
  });

test('a failed page-hide save keeps the local copy and is retried by the next focus loss (HS2-Y0VRXR)', async ({
  page,
}) => {
  const server = await mockProject(page);
  let failing = true;
  server.onPatch = () => (failing ? 503 : undefined);
  await openProject(page);
  await ticketRow(page, 'HS2-EDIT').click();
  const editor = await editDetails(page);
  await editor.fill('Intro\n- typed before hiding');

  await setPageVisibility(page, 'hidden');
  await expect.poll(() => server.patches.length).toBe(1);
  await expect(page.locator('[data-component="app-error"]')).toContainText('The ticket store is unavailable.');
  expect(server.details['01']).toBe('Intro');
  expect(await storedDrafts(page)).toEqual([{ base: 'Intro', draft: 'Intro\n- typed before hiding' }]);

  // Back on the page the editor still holds the text; leaving it with no new typing retries the save.
  failing = false;
  await setPageVisibility(page, 'visible');
  await expect(editor).toBeVisible();
  await expect(editor).toHaveValue('Intro\n- typed before hiding');
  await editor.focus();
  await editor.blur();
  await expect.poll(() => server.details['01']).toBe('Intro\n- typed before hiding');
  await expect(editor).toBeHidden();
  expect(server.patches).toHaveLength(2);
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

// HS2-VMZST8: with a mouse and no stored height, the Details editor opened 48 px tall for long
// text, with the rest scrolling inside it. It now opens at its text's height on every open path.
test('the Details editor opens at its text height on open, after reload, and after a merged restore (HS2-VMZST8)', async ({
  page,
}) => {
  const server = await mockProject(page);
  const lines = (count: number, suffix = '') =>
    Array.from({ length: count }, (_, index) => `Line ${index + 1} of the synthetic details${suffix}.`);
  server.details['01'] = lines(18).join('\n');
  await openProject(page);
  expect(await page.evaluate(() => matchMedia('(pointer: coarse)').matches)).toBe(false);
  await ticketRow(page, 'HS2-EDIT').click();
  const sizing = (editor: Locator) =>
    editor.evaluate((node: HTMLTextAreaElement) => {
      const style = getComputedStyle(node);
      return {
        client: node.clientHeight,
        scroll: node.scrollHeight,
        resize: style.resize,
        inline: node.style.height,
      };
    });
  const expectFitted = async (editor: Locator) => {
    const size = await sizing(editor);
    expect(size.client).toBeGreaterThan(200);
    // No text scrolls inside the editor; the Inspector stays the one scroll owner.
    expect(size.scroll).toBeLessThanOrEqual(size.client + 1);
    // The fine-pointer grip stays, and fitting writes no inline height a pointerup would persist.
    expect(size.resize).toBe('vertical');
    expect(size.inline).toBe('');
  };

  let editor = await editDetails(page);
  await expectFitted(editor);
  // Typing more lines grows the editor rather than scrolling inside it.
  const before = (await sizing(editor)).client;
  await caretToEnd(editor);
  await editor.pressSequentially('\nOne more line\nAnd another');
  await expect.poll(async () => (await sizing(editor)).client).toBeGreaterThan(before);
  await editor.blur();
  await expect(editor).toBeHidden();

  // After a reload.
  await page.reload();
  await ticketRow(page, 'HS2-EDIT').click();
  editor = await editDetails(page);
  await expectFitted(editor);

  // After a reload whose recovery copy merges with a newer server value.
  await caretToEnd(editor);
  await editor.pressSequentially('\nTyped before the reload');
  await expect.poll(() => storedDrafts(page)).toHaveLength(1);
  server.details['01'] = `Changed elsewhere\n${server.details['01'].split('\n').slice(1).join('\n')}`;
  server.tokens['01'] = 'T-remote';
  await page.reload();
  await ticketRow(page, 'HS2-EDIT').click();
  editor = await editDetails(page);
  await expect(page.locator('.app-toast')).toContainText('Restored an unsaved edit and merged it');
  await expect(editor).toHaveValue(/^Changed elsewhere\n[\s\S]*Typed before the reload$/);
  await expectFitted(editor);
  await editor.blur();
  await expect(editor).toBeHidden();

  // Short text keeps the one-line minimum.
  await ticketRow(page, 'HS2-NEXT').click();
  const short = await editDetails(page);
  expect((await sizing(short)).client).toBe(48);
  await short.blur();
  await expect(short).toBeHidden();

  // A height the user dragged still wins over the text height.
  await page.evaluate(() => {
    localStorage.setItem('hotsheet.ticket-editor-height.details.sidebar', '120');
  });
  await page.reload();
  await ticketRow(page, 'HS2-EDIT').click();
  editor = await editDetails(page);
  expect(await editor.evaluate((node) => Math.round(node.getBoundingClientRect().height))).toBe(120);
});
