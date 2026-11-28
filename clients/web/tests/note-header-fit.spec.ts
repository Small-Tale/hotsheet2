import { expect, type Locator, type Page, test } from '@playwright/test';

import { appRequests } from './app-requests';
import { openMockedProject } from './open-mocked-project';

// HS2-D6RRST: a FEEDBACK NEEDED note card in the sidebar inspector keeps its header on one line.
// The hover-revealed Edit Note and Delete Note actions take no width at rest, so the "Feedback
// needed" label neither wraps nor pushes the time past the card edge; when hover or keyboard focus
// reveals them, the label ellipsizes on its one line (Kerf Text truncation) and the time stays inside
// the card. Runs through the shipped inspector in Chromium and WebKit (`--browser webkit`).

const row = {
  connection_id: 'git',
  native_id: '1',
  qualified_id: 'git:1',
  id: '1',
  slug: 'HS2-HEADER',
  title: 'Header fit',
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

const ticket = {
  store: 'git',
  ...row,
  details: 'Ticket context.',
  blocked_reason: null,
  concurrency_token: 'token',
  notes: [
    {
      id: 'N1',
      kind: 'feedback_needed',
      created_at: '2026-10-14T00:01:00Z',
      edited_at: '2026-10-14T00:01:00Z',
      text: 'Which store should own the milestone roll-up?',
    },
  ],
  attachments: [],
};

async function mock(page: Page) {
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
          name: 'Header demo',
          stores: ['/work/demo.hs2'],
          apiPath: '/__hotsheet/project-api/demo',
          needsTicketSetup: false,
          needsHs1Migration: false,
          hs1ImportCompleted: false,
          hs1CleanupEligible: false,
        },
      });
    if (path.endsWith('/tickets') && request.method() === 'GET')
      return route.fulfill({
        json: url.searchParams.has('page_size')
          ? { items: [row], counts: { all: 1, queue: 1, backlog: 0, archive: 0 } }
          : [row],
      });
    if (/\/tickets\/(git%3A1|HS2-HEADER|1)$/.test(path) && request.method() === 'GET')
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
}

interface HeaderGeometry {
  labelLines: number;
  labelTruncated: boolean;
  timeInsideCard: boolean;
  timeClipped: boolean;
  timeInsideViewport: boolean;
}

/** Line boxes of the label text, its ellipsis state, and the time's position against the card frame. */
function headerGeometry(card: Locator): Promise<HeaderGeometry> {
  return card.evaluate((frame) => {
    const kind = frame.querySelector<HTMLElement>('.note-card__kind')!,
      text = kind.querySelector<HTMLElement>('[data-component="text"]')!,
      time = frame.querySelector<HTMLElement>('.note-card__time')!,
      walker = document.createTreeWalker(kind, NodeFilter.SHOW_TEXT),
      rects: DOMRect[] = [];
    // Line boxes of the label's own text: a wrap starts a rect below the previous line's bottom.
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const range = document.createRange();
      range.selectNodeContents(node);
      rects.push(...[...range.getClientRects()].filter((rect) => rect.width > 0));
    }
    rects.sort((a, b) => a.top - b.top);
    let lines = 0,
      lineBottom = -Infinity;
    for (const rect of rects)
      if (rect.top >= lineBottom - 1) {
        lines += 1;
        lineBottom = rect.bottom;
      } else lineBottom = Math.max(lineBottom, rect.bottom);
    // The card's content box: the header row, inside the frame's padding.
    const cardBox = frame.querySelector<HTMLElement>('.note-card__header')!.getBoundingClientRect(),
      timeBox = time.getBoundingClientRect();
    return {
      labelLines: lines,
      labelTruncated: text.scrollWidth > text.clientWidth + 0.5,
      timeInsideCard: timeBox.left >= cardBox.left - 0.5 && timeBox.right <= cardBox.right + 0.5,
      timeClipped: time.scrollWidth > time.clientWidth + 0.5,
      timeInsideViewport: timeBox.right <= document.documentElement.clientWidth + 0.5,
    };
  });
}

async function expectHeaderFits(card: Locator, label: string, { fullLabel }: { fullLabel: boolean }) {
  // Wait out the actions' opacity transition so geometry is read in its settled state.
  await expect
    .poll(
      async () => {
        const geometry = await headerGeometry(card);
        return {
          lines: geometry.labelLines,
          time: geometry.timeInsideCard && !geometry.timeClipped && geometry.timeInsideViewport,
          full: !geometry.labelTruncated,
        };
      },
      { message: label },
    )
    .toEqual({ lines: 1, time: true, full: fullLabel ? true : expect.any(Boolean) });
}

test('the sidebar FEEDBACK NEEDED note header keeps its label on one line and its time inside the card @ci-webkit', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mock(page);
  await page.goto('/');
  await openMockedProject(page);
  await page.locator('[data-ticket-slug="HS2-HEADER"]').click();
  const rail = page.locator('#app-right-rail'),
    card = rail.locator('[data-key="note-N1"]');
  await expect(card.locator('.note-card__kind')).toContainText('Feedback needed');
  await expect(card.locator('.note-card__time')).toBeVisible();

  // Wide, medium, narrow sheet, then back to wide: the transitions must not leave a stale layout.
  for (const width of [1440, 1024, 390, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    // The phone layout collapses the rail; its toolbar toggle opens the inspector as a sheet.
    if (width === 390) await page.getByRole('button', { name: 'Show Ticket Inspector', exact: true }).click();
    await expect(card).toBeInViewport();
    await page.mouse.move(0, 0);
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    // At rest the hidden actions take no width, so the whole label fits.
    await expectHeaderFits(card, `rest at ${width}`, { fullLabel: true });
    // Pointer hover reveals Edit Note and Delete Note; the label may ellipsize but never wraps.
    await card.locator('.note-card__time').hover();
    await expect(card.getByRole('button', { name: 'Delete Note' })).toBeVisible();
    await expectHeaderFits(card, `hover at ${width}`, { fullLabel: false });
    await page.mouse.move(0, 0);
    // Keyboard focus reaches the collapsed actions and reveals them the same way.
    await card.getByRole('button', { name: 'Edit Note' }).focus();
    await expect(card.getByRole('button', { name: 'Edit Note' })).toBeFocused();
    await expectHeaderFits(card, `focus at ${width}`, { fullLabel: false });
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  }
});
