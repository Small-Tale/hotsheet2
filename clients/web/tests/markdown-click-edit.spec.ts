import { expect, type Page, test } from '@playwright/test';

// HS2-H1K9YY: rendered Markdown fields (ticket Details and notes) enter their editor on a single
// click. Links, ticket references, and other controls inside the rendered Markdown keep their own
// action and never start editing. Keyboard entry and the blur-flush autosave contract are unchanged.

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
const baseRow = {
  connection_id: 'git-local',
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
const rows = [
  { ...baseRow, native_id: '01', qualified_id: 'git-local:01', id: '01', slug: 'HS2-EDIT01', title: 'Click to edit' },
  { ...baseRow, native_id: '02', qualified_id: 'git-local:02', id: '02', slug: 'HS2-OTHER1', title: 'Linked target' },
];
const DETAILS =
  'Details mention HS2-OTHER1 and the [guide](https://example.invalid/guide).\n\n- [ ] Unchecked task item\n- [x] Checked task item';
const NOTE = 'Note body text with a link to HS2-OTHER1 for context.';

type Patch = Record<string, unknown>;

async function mockTickets(page: Page, fixture: { note?: string; noteKind?: string } = {}) {
  const patches: Patch[] = [];
  let details = DETAILS,
    note = fixture.note ?? NOTE,
    token = 0;
  const full = (id: string) => ({
    store: 'git-local',
    ...rows.find((row) => row.id === id)!,
    details: id === '01' ? details : 'The linked ticket.',
    blocked_reason: null,
    notes:
      id === '01'
        ? [
            {
              id: 'N1',
              kind: fixture.noteKind ?? 'regular',
              created_at: '2026-09-01T00:00:00Z',
              edited_at: '2026-09-01T00:00:00Z',
              text: note,
            },
          ]
        : [],
    attachments: [],
    concurrency_token: `T${token}`,
  });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname.replace(/\/tickets\/git-local%3A(?=[^/]+)/i, '/tickets/'),
      method = request.method();
    if (url.hostname === 'example.invalid') return route.fulfill({ body: 'external guide' });
    if (path === '/__hotsheet/projects/open') return route.fulfill({ status: 201, json: project });
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
    const one = /\/tickets\/(0\d)$/.exec(path);
    if (one && method === 'PATCH') {
      const body = request.postDataJSON() as Patch;
      patches.push(body);
      if (typeof body.details === 'string') details = body.details;
      if (body.note_id === 'N1' && typeof body.note === 'string') note = body.note;
      token += 1;
      return route.fulfill({ json: full(one[1]) });
    }
    if (one && method === 'GET') return route.fulfill({ json: full(one[1]) });
    // Link resolution searches with `text` and receives the unpaged row array.
    if (path.endsWith('/tickets') && method === 'GET' && url.searchParams.has('text'))
      return route.fulfill({ json: rows.filter((row) => row.slug === url.searchParams.get('text')) });
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
      return;
    }
    if (
      ['/terminals', '/connections', '/commands', '/command-runs', '/views', '/corrupt-tickets'].some((suffix) =>
        path.endsWith(suffix),
      )
    )
      return route.fulfill({ json: [] });
    return route.continue();
  });
  return patches;
}

async function openTicket(page: Page, tap = false) {
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const row = page.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-EDIT01"]');
  if (tap) await row.tap();
  else await row.click();
  const inspector = page.locator('#app-right-rail');
  await expect(inspector.getByRole('button', { name: 'Edit Ticket details' })).toContainText('Details mention');
  return inspector;
}

test('a single click edits details and notes, and the blur-flush autosave contract holds', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const patches = await mockTickets(page);
  const inspector = await openTicket(page);

  const preview = inspector.getByRole('button', { name: 'Edit Ticket details' });
  await expect(preview).toHaveAttribute('title', 'Click to edit');
  await preview.getByText('Details mention').click({ position: { x: 4, y: 6 } });
  const details = inspector.getByRole('textbox', { name: 'Ticket details' });
  await expect(details).toBeFocused();
  await expect(details).toHaveValue(DETAILS);
  await details.fill('Edited with one click');
  // Typing never writes; the single write happens on blur (HS2-RE1PS6).
  await page.waitForTimeout(400);
  expect(patches.filter((patch) => 'details' in patch)).toHaveLength(0);
  await details.blur();
  await expect.poll(() => patches.filter((patch) => patch.details === 'Edited with one click').length).toBe(1);
  await expect(details).toHaveCount(0);
  await expect(preview).toContainText('Edited with one click');
  // Editing again after a save is one more click, and an unchanged blur writes nothing.
  await preview.click();
  await expect(inspector.getByRole('textbox', { name: 'Ticket details' })).toBeFocused();
  await inspector.getByRole('textbox', { name: 'Ticket details' }).blur();
  await page.waitForTimeout(300);
  expect(patches.filter((patch) => 'details' in patch)).toHaveLength(1);

  const note = inspector.locator('article[data-note-id="N1"]');
  const noteSurface = note.locator('.note-card__body');
  await expect(noteSurface).toHaveAttribute('data-edit-on-click', 'true');
  await expect(noteSurface).toHaveAttribute('title', 'Click to edit');
  await noteSurface.getByText('Note body text').click();
  const noteEditor = note.getByRole('textbox', { name: 'Note body' });
  await expect(noteEditor).toBeFocused();
  await noteEditor.fill('Note edited with one click');
  await page.waitForTimeout(400);
  expect(patches.filter((patch) => patch.note_id === 'N1')).toHaveLength(0);
  await noteEditor.blur();
  await expect
    .poll(() => patches.filter((patch) => patch.note_id === 'N1' && patch.note === 'Note edited with one click').length)
    .toBe(1);
  await expect(noteEditor).toHaveCount(0);
  // Card controls keep their own action: Delete is a button, not an edit entry.
  await expect(note.getByRole('button', { name: 'Delete note' })).toBeVisible();

  // A habitual double-click still leaves the editor open and focused instead of closing it again.
  await note.locator('.note-card__body').getByText('Note edited with one click').dblclick();
  await expect(noteEditor).toBeFocused();
  await noteEditor.blur();
  await expect(noteEditor).toHaveCount(0);
  await preview.dblclick({ position: { x: 4, y: 4 } });
  await expect(inspector.getByRole('textbox', { name: 'Ticket details' })).toBeFocused();
  await inspector.getByRole('textbox', { name: 'Ticket details' }).blur();
});

test('links and ticket references inside rendered Markdown never start editing', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const patches = await mockTickets(page);
  const inspector = await openTicket(page);
  const preview = inspector.getByRole('button', { name: 'Edit Ticket details' });

  // A ticket reference opens the linked ticket's reader and leaves Details in preview.
  await preview.getByRole('link', { name: 'HS2-OTHER1' }).click();
  const linked = page.getByRole('dialog', { name: 'Read and edit HS2-OTHER1 in Demo' });
  await expect(linked).toBeVisible();
  await expect(inspector.getByRole('textbox', { name: 'Ticket details' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(linked).toHaveCount(0);

  // An external link opens its page without entering the editor.
  const popup = page.waitForEvent('popup');
  await preview.getByRole('link', { name: 'guide' }).click();
  await (await popup).close();
  await expect(inspector.getByRole('textbox', { name: 'Ticket details' })).toHaveCount(0);

  // The same holds for a ticket reference in a note.
  const note = inspector.locator('article[data-note-id="N1"]');
  await note.getByRole('link', { name: 'HS2-OTHER1' }).click();
  await expect(linked).toBeVisible();
  await expect(note.getByRole('textbox', { name: 'Note body' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(linked).toHaveCount(0);

  // Rendered task checkboxes are read-only: clicking one changes nothing, and the field stays editable.
  const task = preview.getByRole('checkbox').first();
  await expect(task).toBeDisabled();
  await task.click({ force: true });
  await expect(task).not.toBeChecked();
  await expect(preview.getByRole('checkbox').nth(1)).toBeChecked();
  await preview.getByText('Unchecked task item').click({ position: { x: 60, y: 6 } });
  await expect(inspector.getByRole('textbox', { name: 'Ticket details' })).toBeFocused();
  await inspector.getByRole('textbox', { name: 'Ticket details' }).blur();

  // A drag-selection to copy text is not a click to edit.
  await expect(preview).toBeVisible();
  const text = preview.getByText('Details mention');
  const box = (await text.boundingBox())!;
  await page.mouse.move(box.x + 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width - 2, box.y + box.height / 2, { steps: 6 });
  await page.mouse.up();
  expect(await page.evaluate(() => getSelection()?.toString().length ?? 0)).toBeGreaterThan(0);
  await expect(inspector.getByRole('textbox', { name: 'Ticket details' })).toHaveCount(0);
  expect(patches).toHaveLength(0);
});

test('keyboard entry edits the focused field, while Enter on a nested link follows the link', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await mockTickets(page);
  const inspector = await openTicket(page);
  const preview = inspector.getByRole('button', { name: 'Edit Ticket details' });

  await preview.focus();
  await page.keyboard.press('Enter');
  const details = inspector.getByRole('textbox', { name: 'Ticket details' });
  await expect(details).toBeFocused();
  await details.blur();
  await expect(details).toHaveCount(0);

  const noteSurface = inspector.locator('article[data-note-id="N1"] .note-card__body');
  await noteSurface.focus();
  await page.keyboard.press(' ');
  const noteEditor = inspector.getByRole('textbox', { name: 'Note body' });
  await expect(noteEditor).toBeFocused();
  await noteEditor.blur();
  await expect(noteEditor).toHaveCount(0);

  await inspector.locator('article[data-note-id="N1"]').getByRole('link', { name: 'HS2-OTHER1' }).focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('dialog', { name: 'Read and edit HS2-OTHER1 in Demo' })).toBeVisible();
  await expect(noteEditor).toHaveCount(0);
});

test('a single click edits details in the ticket reader too', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const patches = await mockTickets(page);
  const inspector = await openTicket(page);
  await inspector.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog', { name: /Read and edit HS2-EDIT01/ });
  await reader
    .getByRole('button', { name: 'Edit Ticket details' })
    .getByText('Details mention')
    .click({ position: { x: 4, y: 6 } });
  const details = reader.getByRole('textbox', { name: 'Ticket details' });
  await expect(details).toBeFocused();
  await details.fill('Reader edit with one click');
  await details.blur();
  await expect.poll(() => patches.some((patch) => patch.details === 'Reader edit with one click')).toBe(true);
  const note = reader.locator('article[data-note-id="N1"]');
  await note.getByRole('link', { name: 'HS2-OTHER1' }).click();
  await expect(page.getByRole('dialog', { name: 'Read and edit HS2-OTHER1 in Demo' })).toBeVisible();
  await expect(note.getByRole('textbox', { name: 'Note body' })).toHaveCount(0);
});

test('a single tap edits details and notes on a phone', async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true }),
    page = await context.newPage();
  try {
    const patches = await mockTickets(page);
    const inspector = await openTicket(page, true);
    await inspector
      .getByRole('button', { name: 'Edit Ticket details' })
      .getByText('Details mention')
      .tap({ position: { x: 4, y: 6 } });
    const details = inspector.getByRole('textbox', { name: 'Ticket details' });
    await expect(details).toBeFocused();
    await details.fill('Phone edit');
    await details.blur();
    await expect.poll(() => patches.some((patch) => patch.details === 'Phone edit')).toBe(true);
    const note = inspector.locator('article[data-note-id="N1"]');
    await note.getByRole('link', { name: 'HS2-OTHER1' }).tap();
    await expect(page.getByRole('dialog', { name: 'Read and edit HS2-OTHER1 in Demo' })).toBeVisible();
    await expect(note.getByRole('textbox', { name: 'Note body' })).toHaveCount(0);
  } finally {
    await context.close();
  }
});

// HS2-5T33YV: FEEDBACK NEEDED notes format slugs as inline code, which used to render as an
// inert chip. A code span that is exactly one ticket reference now links to that ticket.
test('a code-formatted ticket slug in a feedback-needed note opens the ticket from inspector and reader', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await mockTickets(page, {
    noteKind: 'feedback_needed',
    note: 'FEEDBACK NEEDED: this is intended from `HS2-OTHER1`; keep `hotsheet-cli show HS2-OTHER1` as code.',
  });
  const inspector = await openTicket(page);
  const note = inspector.locator('article[data-note-id="N1"]');
  const reference = note.getByRole('link', { name: 'HS2-OTHER1', exact: true });
  await expect(reference).toHaveAttribute('data-ticket-slug', 'HS2-OTHER1');
  await expect(reference.locator('code')).toHaveText('HS2-OTHER1');
  // Code that holds other text stays a plain chip.
  await expect(note.locator('code', { hasText: 'hotsheet-cli show' })).toHaveCount(1);
  await expect(note.locator('a code', { hasText: 'hotsheet-cli show' })).toHaveCount(0);
  await reference.click();
  const linked = page.getByRole('dialog', { name: 'Read and edit HS2-OTHER1 in Demo' });
  await expect(linked).toBeVisible();
  await expect(note.getByRole('textbox', { name: 'Note body' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(linked).toHaveCount(0);

  // The reader renders the same note as a feedback prompt; its code slug is a link there too, and
  // following it opens the ticket instead of adding an inline reply.
  await inspector.getByRole('button', { name: 'Open ticket reader' }).click();
  const reader = page.getByRole('dialog', { name: /Read and edit HS2-EDIT01/ });
  const prompt = reader.locator('article[data-note-id="N1"] .note-card__feedback-prompt');
  await prompt.getByRole('link', { name: 'HS2-OTHER1', exact: true }).click();
  await expect(linked).toBeVisible();
  await expect(reader.locator('[name="inline-feedback-response"]')).toHaveCount(0);
});
