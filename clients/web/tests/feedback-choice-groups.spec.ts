import { expect, type Page, test } from '@playwright/test';

import { appRequests } from './app-requests';
import { openMockedProject } from './open-mocked-project';

// HS2-83YCDP: a FEEDBACK NEEDED prompt with several CHOICE groups renders every group as its own
// selection list (including the blank-line header form) and answers each group under its question.
const PROMPT = [
  'A few decisions before I continue.',
  '',
  '**1. Which store owns the notes?**',
  '',
  'CHOICE:',
  '- The ticket store',
  '- The document store',
  '',
  '**2. Does the existing Documents "Notes" field stay?**',
  '',
  'CHOICE',
  '',
  '- Keep it read-only',
  '- Migrate and remove it',
  '',
  '**3. When should the migration run?**',
  '',
  'CHOICE:',
  '',
  '- On first launch',
  '- From an explicit command',
  '- Never automatically',
  '',
  'Anything else I should know?',
].join('\n');

const EXPECTED =
  '**1. Which store owns the notes?**\n- The document store\n\n**3. When should the migration run?**\n- On first launch\n- From an explicit command';

const row = {
  connection_id: 'git',
  native_id: '1',
  qualified_id: 'git:1',
  id: '1',
  slug: 'HS2-GROUPS',
  title: 'Several decisions',
  category: 'task',
  priority: 'default',
  status: 'started',
  up_next: true,
  feedback_needed: true,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-10-14T00:00:00Z',
  updated_at: '2026-10-14T00:00:00Z',
};

async function mock(page: Page, where: 'note' | 'details', prompt = PROMPT) {
  const writes: Array<Record<string, unknown>> = [];
  const ticket = {
    store: 'git',
    ...row,
    details: where === 'details' ? `FEEDBACK NEEDED: ${prompt}` : 'Ticket context.',
    blocked_reason: null,
    concurrency_token: 'token',
    notes:
      where === 'note'
        ? [
            {
              id: 'N4',
              kind: 'feedback_needed',
              created_at: '2026-10-14T00:01:00Z',
              edited_at: '2026-10-14T00:01:00Z',
              text: prompt,
            },
          ]
        : [],
    attachments: [],
  };
  await page.route(appRequests, async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (!path.startsWith('/__hotsheet/')) return route.continue();
    if (/^\/__hotsheet\/(?:remote\/)?projects\/open$/.test(path))
      return route.fulfill({
        status: 201,
        json: {
          id: 'demo',
          root: '/work/demo',
          name: 'Choice demo',
          stores: ['/work/demo.hs2'],
          apiPath: '/__hotsheet/project-api/demo',
          needsTicketSetup: false,
          needsHs1Migration: false,
          hs1ImportCompleted: false,
          hs1CleanupEligible: false,
        },
      });
    if (path.endsWith('/batch') && request.method() === 'POST') {
      for (const update of request.postDataJSON().updates as Array<Record<string, unknown>>) writes.push(update);
      return route.fulfill({ json: [{ ...ticket, feedback_needed: false }] });
    }
    if (/\/tickets\/[^/]+$/.test(path) && request.method() === 'PATCH') {
      writes.push(request.postDataJSON() as Record<string, unknown>);
      return route.fulfill({ json: { ...ticket, feedback_needed: false } });
    }
    if (path.endsWith('/tickets') && request.method() === 'GET')
      return route.fulfill({
        json: url.searchParams.has('page_size')
          ? { items: [row], counts: { all: 1, queue: 1, backlog: 0, archive: 0 } }
          : [row],
      });
    if (/\/tickets\/(git%3A1|HS2-GROUPS|1)$/.test(path) && request.method() === 'GET')
      return route.fulfill({ json: ticket });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git',
            provider: 'git',
            display_name: 'Hot Sheet Git',
            locator: '/work/demo.hs2',
            default: true,
            capabilities: { create: true, update: true, notes: true, attachments: true, watch: true, query_fields: [] },
          },
        ],
      });
    if (
      path.endsWith('/connections') ||
      path.endsWith('/permissions') ||
      path.endsWith('/commands') ||
      path.endsWith('/terminals') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (path.endsWith('/ws/poll')) return route.fulfill({ json: { cursor: 0, events: [], overflow: false } });
    return route.fulfill({ status: 404, json: { error: 'not mocked' } });
  });
  await page.goto('/');
  await openMockedProject(page);
  await page.locator('[data-ticket-slug="HS2-GROUPS"]').click();
  return writes;
}

const additive = process.platform === 'darwin' ? 'Meta' : 'Control';

async function answerGroups(page: Page, scope: ReturnType<Page['locator']>, capture: string) {
  const grids = scope.getByRole('grid', { name: /^Feedback Choices \d/ });
  await expect(grids).toHaveCount(3);
  await expect(grids.nth(1)).toHaveAccessibleName(
    'Feedback Choices 2: 2. Does the existing Documents "Notes" field stay?',
  );
  const group = (index: number) => grids.nth(index).locator('[data-component="content-item"]');
  await expect(group(0)).toHaveCount(2);
  await expect(group(1)).toHaveCount(2);
  await expect(group(2)).toHaveCount(3);
  // No group falls back to a literal CHOICE paragraph.
  await expect(scope.getByText(/^CHOICE:?$/)).toHaveCount(0);
  await group(0).nth(1).click();
  await group(2).nth(0).click();
  await group(2)
    .nth(1)
    .click({ modifiers: [additive] });
  // A plain click in group 2 then a second plain click clears it without touching groups 1 and 3.
  await group(1).nth(0).click();
  await group(1).nth(0).click();
  await expect(group(0).nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(group(1).nth(0)).toHaveAttribute('aria-selected', 'false');
  await expect(group(2).nth(0)).toHaveAttribute('aria-selected', 'true');
  await expect(group(2).nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(group(2).nth(2)).toHaveAttribute('aria-selected', 'false');
  await scope.screenshot({ path: `target/visual-captures/hs2-83ycdp-${capture}-1280.png` });
  await page.setViewportSize({ width: 390, height: 844 });
  await scope.screenshot({ path: `target/visual-captures/hs2-83ycdp-${capture}-390.png` });
  await page.setViewportSize({ width: 1280, height: 900 });
}

test('answers several choice groups in a feedback-needed note through the reader', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const writes = await mock(page, 'note');
  await page.getByRole('button', { name: 'Open Ticket Reader' }).click();
  const note = page.getByRole('dialog').locator('article[data-note-id="N4"]');
  await answerGroups(page, note, 'note-reader');
  await note.getByRole('button', { name: 'Respond' }).click();
  await expect.poll(() => writes.find((write) => write.note_kind === 'regular')?.note).toBe(EXPECTED);
});

test('answers several choice groups embedded in the ticket description from the inspector', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const writes = await mock(page, 'details');
  await page.locator('#app-right-rail').getByRole('button', { name: 'Respond to Feedback' }).click();
  const details = page.getByRole('dialog').locator('[data-details-feedback="true"]');
  await answerGroups(page, details, 'details-reader');
  await details.getByRole('button', { name: 'Respond' }).click();
  await expect.poll(() => writes.find((write) => write.note_kind === 'regular')?.note).toBe(EXPECTED);
});

// HS2-3MHSBS: an AI wrote the question on the CHOICE: line itself, after a Markdown table and with
// bold-led options. The prompt must still render as a selectable group with its question visible.
const INLINE_PROMPT = [
  'FEEDBACK NEEDED: the eviction is real, but it happens only when Oilpan runs a GC.',
  '',
  '| Arm | U+4E00 paints |',
  '|---|---|',
  '| removed, no GC | HiraginoSans-W3 |',
  '| removed, forced GC | **PingFangSC-Regular** |',
  '',
  'CHOICE: How should Domotion handle GC eviction of macOS ideograph fallback entries?',
  '- **Leave it unmodeled (recommended).** Keep the strong model and document it.',
  '- **Force a known GC state at capture boundaries and model it.** Deterministic, but perturbs the page.',
  "- **Record Chrome's answer at capture time.** Exact without perturbing GC.",
].join('\n');

test('renders a CHOICE: header that carries its own question as a selectable group (HS2-3MHSBS)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const writes = await mock(page, 'note', INLINE_PROMPT);
  await page.getByRole('button', { name: 'Open Ticket Reader' }).click();
  const note = page.getByRole('dialog').locator('article[data-note-id="N4"]');
  const grid = note.getByRole('grid', { name: 'Feedback Choices' });
  await expect(grid).toBeVisible();
  const options = grid.locator('[data-component="content-item"]');
  await expect(options).toHaveCount(3);
  await expect(
    note.getByText('How should Domotion handle GC eviction of macOS ideograph fallback entries?', { exact: true }),
  ).toBeVisible();
  // Neither the header keyword nor the raw list survives as prose.
  await expect(note.getByText(/CHOICE/)).toHaveCount(0);
  await expect(note.locator('table')).toBeVisible();
  await options.nth(0).click();
  await expect(options.nth(0)).toHaveAttribute('aria-selected', 'true');
  await note.screenshot({ path: 'target/visual-captures/hs2-3mhsbs-inline-choice-1280.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(options.nth(0)).toBeVisible();
  await note.screenshot({ path: 'target/visual-captures/hs2-3mhsbs-inline-choice-390.png' });
  await page.setViewportSize({ width: 1280, height: 900 });
  await note.getByRole('button', { name: 'Respond' }).click();
  await expect
    .poll(() => writes.find((write) => write.note_kind === 'regular')?.note)
    .toBe('Selected choice:\n- **Leave it unmodeled (recommended).** Keep the strong model and document it.');
});

// HS2-EWF0FT: options written as multi-line list items or as lettered paragraphs (the shape agents
// naturally write) render as selectable groups with each option's details under its label.
const DETAILED_PROMPT = [
  'CHOICE: Which rollout should the importer use?',
  '- **Ship now.** Lowest effort for this release.',
  '  Costs a schema migration in the next release, and the migration needs a backfill window.',
  '',
  '  The backfill can run in the background.',
  '- **Wait a release.**',
  '',
  'The widget cache can go stale after an upgrade. Choose one:',
  '',
  '**CHOICE A: Rebuild on every launch.** Simple and always correct. Adds about two seconds to startup.',
  '',
  '**CHOICE B: Rebuild on change.** Needs a file watcher.',
  'The watcher costs some memory while the app runs.',
  '',
  '**CHOICE C: Both.** Launch rebuild plus the watcher.',
  '',
  'Recommendation: C.',
].join('\n');

test('renders multi-line and lettered choices with details under their labels (HS2-EWF0FT)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const writes = await mock(page, 'note', DETAILED_PROMPT);
  await page.getByRole('button', { name: 'Open Ticket Reader' }).click();
  const note = page.getByRole('dialog').locator('article[data-note-id="N4"]');
  const grids = note.getByRole('grid', { name: /^Feedback Choices \d/ });
  await expect(grids).toHaveCount(2);
  await expect(grids.nth(1)).toHaveAccessibleName(
    'Feedback Choices 2: The widget cache can go stale after an upgrade. Choose one:',
  );
  const options = (index: number) => grids.nth(index).locator('[data-component="content-item"]');
  await expect(options(0)).toHaveCount(2);
  await expect(options(1)).toHaveCount(3);
  await expect(options(0).nth(0)).toContainText('The backfill can run in the background.');
  await expect(options(1).nth(1)).toContainText('The watcher costs some memory while the app runs.');
  // Trailing prose after the last lettered block stays outside every option.
  await expect(note.getByText('Recommendation: C.', { exact: true })).toBeVisible();
  await expect(grids.nth(1).getByText('Recommendation: C.')).toHaveCount(0);
  await options(0).nth(0).click();
  await options(1).nth(1).click();
  await expect(options(1).nth(1)).toHaveAttribute('aria-selected', 'true');
  await note.screenshot({ path: 'target/visual-captures/hs2-ewf0ft-detailed-choices-1280.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(options(1).nth(2)).toBeVisible();
  // The whole note is taller than a phone viewport; capture each group where it scrolls into view.
  for (const index of [0, 1]) {
    await grids.nth(index).scrollIntoViewIfNeeded();
    await grids
      .nth(index)
      .screenshot({ path: `target/visual-captures/hs2-ewf0ft-detailed-choices-${index + 1}-390.png` });
  }
  await page.setViewportSize({ width: 1280, height: 900 });
  await note.getByRole('button', { name: 'Respond' }).click();
  await expect
    .poll(() => writes.find((write) => write.note_kind === 'regular')?.note)
    .toBe(
      '**Which rollout should the importer use?**\n- **Ship now.** Lowest effort for this release.\n\n' +
        '**The widget cache can go stale after an upgrade. Choose one:**\n- CHOICE B: Rebuild on change.',
    );
});

// HS2-WX8BE6: the HS2-8T71YW note shape. Numbered `CHOICE N: <question>` headers, each followed by
// a lettered bullet list, are six separate groups, not one group whose options are the questions.
const NUMBERED_QUESTIONS = [
  'How does a Git store hold a milestone?',
  'What is a milestone in Jira?',
  'Nested milestones?',
  'When does a milestone orchestration prerequisite hold?',
  'Where do milestones live in the client?',
  'What instant is a date-only due date for scheduling?',
];
const NUMBERED_OPTIONS = [
  ['A (recommended): a ticket of a built-in milestone category', 'B: a registry file per milestone'],
  ['A (recommended): a fixVersion', 'B: the parent epic', 'C: a per-connection setting'],
  ['A (recommended): not in the first version', 'B: allow nesting, cycle-checked'],
  ['A (recommended): every linked ticket confirmed finished', 'B: when the milestone is closed', 'C: either'],
  ['A (recommended): a Milestones sidebar view', 'B: only group by milestone', 'C: a global dashboard'],
  ['A (recommended): end of that day in UTC', 'B: end of that day in a project time zone'],
];
const NUMBERED_PROMPT = [
  'FEEDBACK NEEDED: milestones design. Please answer each CHOICE; my recommendation is first and marked.',
  ...NUMBERED_QUESTIONS.flatMap((question, index) => [
    '',
    `CHOICE ${index + 1}: ${question}`,
    ...NUMBERED_OPTIONS[index].map((option) => `- ${option}`),
  ]),
].join('\n');

test('answers numbered CHOICE N: question headers as separate groups (HS2-WX8BE6)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const writes = await mock(page, 'note', NUMBERED_PROMPT);
  await page.getByRole('button', { name: 'Open Ticket Reader' }).click();
  const note = page.getByRole('dialog').locator('article[data-note-id="N4"]');
  const grids = note.getByRole('grid', { name: /^Feedback Choices \d/ });
  await expect(grids).toHaveCount(6);
  const options = (index: number) => grids.nth(index).locator('[data-component="content-item"]');
  for (const [index, question] of NUMBERED_QUESTIONS.entries()) {
    await expect(grids.nth(index)).toHaveAccessibleName(`Feedback Choices ${index + 1}: ${question}`);
    await expect(options(index)).toHaveCount(NUMBERED_OPTIONS[index].length);
    await expect(options(index).nth(0)).toContainText('A (recommended):');
  }
  // No numbered header survives as an option or as a literal CHOICE paragraph.
  await expect(note.getByText(/CHOICE \d/)).toHaveCount(0);
  // One option per group: the recommendation in odd groups, the last option in even groups.
  const picks = NUMBERED_OPTIONS.map((group, index) => (index % 2 ? group.length - 1 : 0));
  for (const [index, pick] of picks.entries()) await options(index).nth(pick).click();
  for (const [index, pick] of picks.entries())
    for (let option = 0; option < NUMBERED_OPTIONS[index].length; option += 1)
      await expect(options(index).nth(option)).toHaveAttribute('aria-selected', String(option === pick));
  await note.screenshot({ path: 'target/visual-captures/hs2-wx8be6-numbered-choices-1280.png' });
  await note.getByRole('button', { name: 'Respond' }).click();
  await expect
    .poll(() => writes.find((write) => write.note_kind === 'regular')?.note)
    .toBe(
      NUMBERED_QUESTIONS.map((question, index) => `**${question}**\n- ${NUMBERED_OPTIONS[index][picks[index]]}`).join(
        '\n\n',
      ),
    );
});

// HS2-ZXGEFE: a FEEDBACK NEEDED prompt can be edited, but only on purpose. A double-click on the
// prompt body (sidebar) or its Edit Note action (sidebar and reader) opens the ordinary note editor;
// the save says it is an explicit prompt edit. A single click still never opens it (HS2-MB2Q10).
const EDIT_PROMPT = 'Which token should I use for the Jira site?';

test('edits a feedback prompt in place from a sidebar double-click (HS2-ZXGEFE)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const writes = await mock(page, 'note', EDIT_PROMPT);
  const note = page.locator('#app-right-rail article[data-note-id="N4"]');
  const body = note.locator('[data-feedback-prompt-body="true"]');
  await expect(body).toContainText(EDIT_PROMPT);
  await body.click();
  await expect(note.locator('[name="note-body"]')).toHaveCount(0);
  await body.dblclick();
  const editor = note.locator('[name="note-body"]');
  await expect(editor).toBeFocused();
  await expect(editor).toHaveValue(EDIT_PROMPT);
  await expect(editor).not.toHaveAttribute('data-note-response', 'true');
  await editor.fill('Which PAT should I use for the HS2T Jira site?');
  await note.screenshot({ path: 'target/visual-captures/hs2-zxgefe-sidebar-editing-1280.png' });
  await editor.evaluate((field) => {
    (field as HTMLTextAreaElement).blur();
  });
  await expect
    .poll(() => writes.find((write) => write.note_id === 'N4'))
    .toMatchObject({
      note_id: 'N4',
      note: 'Which PAT should I use for the HS2T Jira site?',
      edit_feedback_prompt: true,
    });
  expect(writes.some((write) => write.note_kind === 'regular')).toBe(false);
});

test('edits a feedback prompt from the reader Edit Note action without posting a reply (HS2-ZXGEFE)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const writes = await mock(page, 'note', EDIT_PROMPT);
  await page.getByRole('button', { name: 'Open Ticket Reader' }).click();
  const note = page.getByRole('dialog').locator('article[data-note-id="N4"]');
  await expect(note.locator('textarea[data-note-response="true"]')).toBeVisible();
  await note.hover();
  const edit = note.getByRole('button', { name: 'Edit Note' });
  await expect(edit).toBeVisible();
  await note.screenshot({ path: 'target/visual-captures/hs2-zxgefe-reader-edit-action-1280.png' });
  await edit.click();
  const editor = note.locator('[name="note-body"]');
  await expect(editor).toBeFocused();
  await expect(editor).toHaveValue(EDIT_PROMPT);
  // The response editor and its actions give way to the plain editor while the prompt is edited.
  await expect(note.locator('textarea[data-note-response="true"]')).toHaveCount(0);
  await expect(note.getByRole('button', { name: 'Respond' })).toHaveCount(0);
  await editor.fill('Which PAT should I use?');
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(editor).toBeVisible();
  await note.screenshot({ path: 'target/visual-captures/hs2-zxgefe-reader-editing-390.png' });
  await page.setViewportSize({ width: 1280, height: 900 });
  await editor.evaluate((field) => {
    (field as HTMLTextAreaElement).blur();
  });
  await expect
    .poll(() => writes.find((write) => write.note_id === 'N4'))
    .toMatchObject({ note_id: 'N4', note: 'Which PAT should I use?', edit_feedback_prompt: true });
  expect(writes.some((write) => write.note_kind === 'regular')).toBe(false);
});

// HS2-08EAH1: the sidebar inspector shows a FEEDBACK NEEDED note's CHOICE groups as the same
// selectable lists as the reader. Its Respond to Feedback opens the reader with the selection carried
// over, and the reader's Respond posts exactly the response the reader-only path posts.
const SIDEBAR_PROMPT = [
  'Two decisions before I build the milestone roll-up.',
  '',
  'CHOICE: How does a Git store hold a milestone?',
  '- **A ticket of category milestone (recommended).** Reuses the existing ticket file format and sync.',
  '- **A separate milestones file per store.** Smaller tickets, but a new file kind to merge.',
  '',
  'CHOICE: Which due-date roll-up should the board show?',
  '- Latest due date of the open tickets',
  '- The milestone date only',
  '- Both, the milestone date with a warning when a ticket is later',
  '',
  'Anything else I should know?',
].join('\n');

const SIDEBAR_EXPECTED =
  '**How does a Git store hold a milestone?**\n- **A ticket of category milestone (recommended).** Reuses the existing ticket file format and sync.\n\n' +
  '**Which due-date roll-up should the board show?**\n- The milestone date only\n- Both, the milestone date with a warning when a ticket is later';

/** Captures show the resting card: hover or focus inside it reveals the header's note actions. */
async function restingCard(page: Page) {
  await page.mouse.move(1, 1);
  await page.evaluate(() => {
    (document.activeElement as HTMLElement | null)?.blur();
  });
}

async function chooseSidebarAnswers(scope: ReturnType<Page['locator']>) {
  const grids = scope.getByRole('grid', { name: /^Feedback Choices \d/ });
  await expect(grids).toHaveCount(2);
  await expect(grids.nth(0)).toHaveAccessibleName('Feedback Choices 1: How does a Git store hold a milestone?');
  const options = (index: number) => grids.nth(index).locator('[data-component="content-item"]');
  await expect(options(0)).toHaveCount(2);
  await expect(options(1)).toHaveCount(3);
  await expect(scope.getByText(/CHOICE/)).toHaveCount(0);
  await options(0).nth(0).click();
  await options(1).nth(1).click();
  await options(1)
    .nth(2)
    .click({ modifiers: [additive] });
  await expect(options(0).nth(0)).toHaveAttribute('aria-selected', 'true');
  await expect(options(0).nth(1)).toHaveAttribute('aria-selected', 'false');
  await expect(options(1).nth(0)).toHaveAttribute('aria-selected', 'false');
  await expect(options(1).nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(options(1).nth(2)).toHaveAttribute('aria-selected', 'true');
  return options;
}

test('answers choice groups from the sidebar inspector note through Respond to Feedback (HS2-08EAH1)', async ({
  page,
  browserName,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const writes = await mock(page, 'note', SIDEBAR_PROMPT);
  const note = page.locator('#app-right-rail article[data-note-id="N4"]');
  const options = await chooseSidebarAnswers(note);
  // Choosing in the sidebar never opens a response editor or inline reply there.
  await expect(note.locator('textarea')).toHaveCount(0);
  // A double-click on an option toggles it twice; it never opens Edit Note on the prompt.
  await options(0).nth(1).dblclick();
  await expect(options(0).nth(1)).toHaveAttribute('aria-selected', 'false');
  await expect(note.locator('[name="note-body"]')).toHaveCount(0);
  // Restore the first group's answer after the double toggle.
  await options(0).nth(0).click();
  await expect(options(0).nth(0)).toHaveAttribute('aria-selected', 'true');
  // Keyboard: Space on a focused option acts as a click.
  await options(1).nth(0).focus();
  await page.keyboard.press('Space');
  await expect(options(1).nth(0)).toHaveAttribute('aria-selected', 'true');
  await options(1).nth(1).click();
  await options(1)
    .nth(2)
    .click({ modifiers: [additive] });
  await expect(options(1).nth(0)).toHaveAttribute('aria-selected', 'false');
  // The choice groups wrap inside the narrow rail: nothing in the prompt overflows the note.
  const prompt = note.locator('.note-card__feedback-prompt');
  const promptFits = () =>
    prompt.evaluate((element) => {
      const card = element.closest('article')?.getBoundingClientRect();
      if (!card) return false;
      return (
        element.scrollWidth <= element.clientWidth &&
        [...element.querySelectorAll('*')].every((child) => child.getBoundingClientRect().right <= card.right + 0.5)
      );
    });
  expect(await promptFits()).toBe(true);
  const capture = (name: string) => `target/visual-captures/hs2-08eah1-${name}-${browserName}.png`;
  await restingCard(page);
  await note.screenshot({ path: capture('sidebar-1440-light') });
  await page.setViewportSize({ width: 1024, height: 900 });
  await expect(options(1).nth(2)).toBeVisible();
  expect(await promptFits()).toBe(true);
  await note.screenshot({ path: capture('sidebar-1024-light') });
  await page.emulateMedia({ colorScheme: 'dark' });
  await note.screenshot({ path: capture('sidebar-1024-dark') });
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(options(1).nth(1)).toHaveAttribute('aria-selected', 'true');
  await note.screenshot({ path: capture('sidebar-1440-dark') });
  await page.emulateMedia({ colorScheme: 'light' });
  await note.getByRole('button', { name: 'Respond to Feedback' }).click();
  const reader = page.getByRole('dialog').locator('article[data-note-id="N4"]');
  const readerOptions = (index: number) =>
    reader
      .getByRole('grid', { name: /^Feedback Choices \d/ })
      .nth(index)
      .locator('[data-component="content-item"]');
  // The reader shows the sidebar's selection: the same state, not a second copy.
  await expect(readerOptions(0).nth(0)).toHaveAttribute('aria-selected', 'true');
  await expect(readerOptions(1).nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(readerOptions(1).nth(2)).toHaveAttribute('aria-selected', 'true');
  await reader.getByRole('button', { name: 'Respond' }).click();
  await expect.poll(() => writes.find((write) => write.note_kind === 'regular')?.note).toBe(SIDEBAR_EXPECTED);
});

test('the reader-only path posts the same response for the sidebar prompt (HS2-08EAH1)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const writes = await mock(page, 'note', SIDEBAR_PROMPT);
  await page.getByRole('button', { name: 'Open Ticket Reader' }).click();
  const reader = page.getByRole('dialog').locator('article[data-note-id="N4"]');
  await chooseSidebarAnswers(reader);
  await reader.getByRole('button', { name: 'Respond' }).click();
  await expect.poll(() => writes.find((write) => write.note_kind === 'regular')?.note).toBe(SIDEBAR_EXPECTED);
});

test('keeps sidebar choice groups inside the inspector from wide to phone and back (HS2-08EAH1)', async ({
  page,
  browserName,
}) => {
  const inspector = page.locator('#app-right-rail'),
    note = inspector.locator('article[data-note-id="N4"]'),
    grids = note.getByRole('grid', { name: /^Feedback Choices \d/ });
  // Every option sits inside the rail's content edge at every width, with no page-level scroll.
  const fits = async () => {
    await expect(grids).toHaveCount(2);
    expect(
      await inspector.evaluate((rail) => {
        const edge = rail.getBoundingClientRect().right;
        return [...rail.querySelectorAll('[data-choice-id]')].every(
          (option) => option.getBoundingClientRect().right <= Math.min(edge, innerWidth) + 0.5,
        );
      }),
    ).toBe(true);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  };
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.setViewportSize({ width: 1440, height: 900 });
    if (scheme === 'light') await mock(page, 'note', SIDEBAR_PROMPT);
    await grids.nth(1).locator('[data-component="content-item"]').nth(1).click();
    await restingCard(page);
    await fits();
    await page.screenshot({ path: `target/visual-captures/hs2-08eah1-page-1440-${scheme}-${browserName}.png` });
    await page.setViewportSize({ width: 1024, height: 900 });
    await fits();
    await page.screenshot({ path: `target/visual-captures/hs2-08eah1-page-1024-${scheme}-${browserName}.png` });
    // The phone layout closes the inspector (HS2-ZK51WP); reopen it as the sheet.
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(inspector).toHaveAttribute('data-collapsed', 'true');
    await page.getByRole('button', { name: 'Show Ticket Inspector' }).click();
    await expect(inspector).toHaveAttribute('data-collapsed', 'false');
    await grids.nth(0).scrollIntoViewIfNeeded();
    await expect(grids.nth(1).locator('[data-component="content-item"]').nth(1)).toHaveAttribute(
      'aria-selected',
      'true',
    );
    await fits();
    await page.screenshot({ path: `target/visual-captures/hs2-08eah1-page-390-${scheme}-${browserName}.png` });
    await page.setViewportSize({ width: 1440, height: 900 });
    await expect(inspector).toHaveAttribute('data-collapsed', 'false');
    await fits();
    await page.screenshot({ path: `target/visual-captures/hs2-08eah1-page-1440-back-${scheme}-${browserName}.png` });
    // Clear the selection so the next scheme's pass starts from the same state.
    await grids.nth(1).locator('[data-component="content-item"]').nth(1).click();
    await expect(grids.nth(1).locator('[aria-selected="true"]')).toHaveCount(0);
  }
});

// HS2-0MET3B: a ticket description marked FEEDBACK NEEDED shows its CHOICE groups in the sidebar as
// the same selectable lists. The prose keeps its click-to-edit entry (HS2-RE1PS6), an option never
// opens the editor, and Respond to Feedback carries the selection into the reader's Details card.
test('answers description choice groups from the sidebar inspector through Respond to Feedback (HS2-0MET3B)', async ({
  page,
  browserName,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const writes = await mock(page, 'details', SIDEBAR_PROMPT);
  const details = page.locator('#app-right-rail .ticket-info-panel__details-surface');
  const surface = details.locator('[data-action="edit-markdown"]');
  const source = details.locator('[name="markdown-source"]');
  await expect(surface).toHaveAttribute('role', 'group');
  await expect(surface).toHaveAccessibleName('Edit Ticket details');
  const options = await chooseSidebarAnswers(details);
  // Choosing never opens the details editor, and a double-click on an option toggles it twice.
  await expect(source).toHaveCount(0);
  await options(0).nth(1).dblclick();
  await expect(options(0).nth(1)).toHaveAttribute('aria-selected', 'false');
  await expect(source).toHaveCount(0);
  await options(0).nth(0).click();
  await expect(options(0).nth(0)).toHaveAttribute('aria-selected', 'true');
  // A click on the prose still opens the editor in place; leaving it restores the choices as chosen.
  await details.getByText('Anything else I should know?').click();
  await expect(source).toBeFocused();
  await expect(source).toHaveValue(`FEEDBACK NEEDED: ${SIDEBAR_PROMPT}`);
  await page.locator('#app-right-rail').getByRole('button', { name: 'Respond to Feedback' }).focus();
  await expect(source).toHaveCount(0);
  await expect(options(0).nth(0)).toHaveAttribute('aria-selected', 'true');
  await expect(options(1).nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(options(1).nth(2)).toHaveAttribute('aria-selected', 'true');
  // A habitual double-click on the prose opens the editor and keeps it open.
  await details.getByText('Two decisions before I build the milestone roll-up.').dblclick();
  await expect(source).toBeFocused();
  await page.locator('#app-right-rail').getByRole('button', { name: 'Respond to Feedback' }).focus();
  await expect(source).toHaveCount(0);
  // Keyboard: Enter on the surface edits; Space on a focused option toggles it instead.
  await surface.focus();
  await page.keyboard.press('Enter');
  await expect(source).toBeFocused();
  await page.locator('#app-right-rail').getByRole('button', { name: 'Respond to Feedback' }).focus();
  await expect(source).toHaveCount(0);
  await options(1).nth(0).focus();
  await page.keyboard.press('Space');
  await expect(options(1).nth(0)).toHaveAttribute('aria-selected', 'true');
  await expect(source).toHaveCount(0);
  await options(1).nth(1).click();
  await options(1)
    .nth(2)
    .click({ modifiers: [additive] });
  await expect(options(1).nth(0)).toHaveAttribute('aria-selected', 'false');
  // Nothing was edited, so opening and leaving the editor wrote nothing.
  expect(writes).toHaveLength(0);
  const capture = (name: string) => `target/visual-captures/hs2-0met3b-${name}-${browserName}.png`;
  await restingCard(page);
  await details.screenshot({ path: capture('details-1440-light') });
  await page.emulateMedia({ colorScheme: 'dark' });
  await details.screenshot({ path: capture('details-1440-dark') });
  await page.emulateMedia({ colorScheme: 'light' });
  await details.getByRole('button', { name: 'Respond to Feedback' }).click();
  const reader = page.getByRole('dialog').locator('[data-details-feedback="true"]');
  const readerOptions = (index: number) =>
    reader
      .getByRole('grid', { name: /^Feedback Choices \d/ })
      .nth(index)
      .locator('[data-component="content-item"]');
  // The reader's Details card shows the sidebar's selection: the same state, not a second copy.
  await expect(readerOptions(0).nth(0)).toHaveAttribute('aria-selected', 'true');
  await expect(readerOptions(1).nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(readerOptions(1).nth(2)).toHaveAttribute('aria-selected', 'true');
  await reader.getByRole('button', { name: 'Respond' }).click();
  await expect.poll(() => writes.find((write) => write.note_kind === 'regular')?.note).toBe(SIDEBAR_EXPECTED);
});

test('keeps sidebar description choice groups inside the inspector from wide to phone and back (HS2-0MET3B)', async ({
  page,
  browserName,
}) => {
  const inspector = page.locator('#app-right-rail'),
    details = inspector.locator('.ticket-info-panel__details-surface'),
    grids = details.getByRole('grid', { name: /^Feedback Choices \d/ });
  const fits = async () => {
    await expect(grids).toHaveCount(2);
    expect(
      await details.evaluate((card) => {
        const edge = Math.min(card.getBoundingClientRect().right, innerWidth);
        return [...card.querySelectorAll('[data-choice-id]')].every(
          (option) => option.getBoundingClientRect().right <= edge + 0.5,
        );
      }),
    ).toBe(true);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  };
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.setViewportSize({ width: 1440, height: 900 });
    if (scheme === 'light') await mock(page, 'details', SIDEBAR_PROMPT);
    await grids.nth(1).locator('[data-component="content-item"]').nth(1).click();
    await restingCard(page);
    await fits();
    await page.screenshot({ path: `target/visual-captures/hs2-0met3b-page-1440-${scheme}-${browserName}.png` });
    await page.setViewportSize({ width: 1024, height: 900 });
    await fits();
    await page.screenshot({ path: `target/visual-captures/hs2-0met3b-page-1024-${scheme}-${browserName}.png` });
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(inspector).toHaveAttribute('data-collapsed', 'true');
    await page.getByRole('button', { name: 'Show Ticket Inspector' }).click();
    await expect(inspector).toHaveAttribute('data-collapsed', 'false');
    await grids.nth(0).scrollIntoViewIfNeeded();
    await expect(grids.nth(1).locator('[data-component="content-item"]').nth(1)).toHaveAttribute(
      'aria-selected',
      'true',
    );
    await fits();
    await page.screenshot({ path: `target/visual-captures/hs2-0met3b-page-390-${scheme}-${browserName}.png` });
    await page.setViewportSize({ width: 1440, height: 900 });
    await expect(inspector).toHaveAttribute('data-collapsed', 'false');
    await fits();
    await page.screenshot({ path: `target/visual-captures/hs2-0met3b-page-1440-back-${scheme}-${browserName}.png` });
    await grids.nth(1).locator('[data-component="content-item"]').nth(1).click();
    await expect(grids.nth(1).locator('[aria-selected="true"]')).toHaveCount(0);
  }
});
