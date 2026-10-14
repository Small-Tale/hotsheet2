import { expect, type Page, test } from '@playwright/test';

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

async function mock(page: Page, where: 'note' | 'details') {
  const writes: Array<Record<string, unknown>> = [];
  const ticket = {
    store: 'git',
    ...row,
    details: where === 'details' ? `FEEDBACK NEEDED: ${PROMPT}` : 'Ticket context.',
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
              text: PROMPT,
            },
          ]
        : [],
    attachments: [],
  };
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (!path.startsWith('/__hotsheet/')) return route.continue();
    if (path === '/__hotsheet/projects/open')
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
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
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
