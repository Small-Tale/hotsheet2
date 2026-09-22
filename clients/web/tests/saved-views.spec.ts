import { expect, type Page, test } from '@playwright/test';

const project = {
  id: 'saved-views',
  root: '/work/saved-views',
  name: 'Saved views',
  stores: ['/work/saved-views.hs2'],
  apiPath: '/__hotsheet/project-api/saved-views',
};
const base = {
  connection_id: 'git',
  native_id: '1',
  qualified_id: 'git:1',
  id: '1',
  slug: 'HS2-DOCS',
  title: 'Document the saved view',
  category: 'task',
  priority: 'default',
  status: 'not_started',
  up_next: true,
  feedback_needed: false,
  tags: ['docs'],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-09-10T00:00:00Z',
  updated_at: '2026-09-10T00:00:00Z',
};
const rows = [
  base,
  {
    ...base,
    native_id: '2',
    qualified_id: 'git:2',
    id: '2',
    slug: 'HS2-CODE',
    title: 'Implement the parser',
    tags: ['client'],
  },
];

async function mockSavedViews(page: Page) {
  let views: Array<{ id: string; name: string; query: string }> = [];
  await page.route('**/*', (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (path === '/__hotsheet/projects/open') return route.fulfill({ status: 201, json: project });
    if (path.endsWith('/views') && request.method() === 'PUT') {
      views = request.postDataJSON();
      return route.fulfill({ json: views });
    }
    if (path.endsWith('/views')) return route.fulfill({ json: views });
    if (path.endsWith('/tickets') && request.method() === 'GET') {
      const tags = url.searchParams.get('tags')?.split(' ').filter(Boolean) ?? [],
        items = tags.length ? rows.filter((ticket) => tags.every((tag) => ticket.tags.includes(tag))) : rows;
      return route.fulfill({
        json: url.searchParams.has('page_size')
          ? {
              items,
              counts: {
                total: rows.length,
                queued: rows.length,
                backlog: 0,
                archive: 0,
                open: rows.length,
                up_next: rows.length,
                active: 0,
                started: 0,
                completed_today: 0,
              },
            }
          : items,
      });
    }
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git-local',
            provider: 'git',
            display_name: 'Hot Sheet git',
            locator: project.stores[0],
            default: true,
            capabilities: { create: true, update: true, notes: true, attachments: true, watch: true, query_fields: [] },
          },
        ],
      });
    if (
      path.endsWith('/connections') ||
      path.endsWith('/permissions') ||
      path.endsWith('/commands') ||
      path.endsWith('/command-runs') ||
      path.endsWith('/terminals') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(url.searchParams.get('since') ?? 0), events: [], overflow: false },
      });
    return route.continue();
  });
  await page.routeWebSocket('**/__hotsheet/project-api/*/ws/sync', () => undefined);
  return () => views;
}

async function holdAnimationFrames(page: Page) {
  return page.evaluateHandle(() => {
    const original = window.requestAnimationFrame.bind(window),
      originalCancel = window.cancelAnimationFrame.bind(window),
      pending = new Map<number, FrameRequestCallback>();
    let next = -1;
    window.requestAnimationFrame = (callback) => {
      const id = next--;
      pending.set(id, callback);
      return id;
    };
    window.cancelAnimationFrame = (id) => {
      if (id < 0) pending.delete(id);
      else originalCancel(id);
    };
    return {
      advance() {
        const callbacks = [...pending.values()];
        pending.clear();
        for (const callback of callbacks) callback(performance.now());
      },
      restore() {
        window.requestAnimationFrame = original;
        window.cancelAnimationFrame = originalCancel;
        for (const callback of pending.values()) original(callback);
        pending.clear();
      },
    };
  });
}

test('creates, renames, deletes, and shares a custom ticket view', async ({ page }) => {
  const views = await mockSavedViews(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add view' }).click();
  const dialog = page.locator('[data-component="saved-view-dialog"]');
  await expect(dialog.getByRole('textbox', { name: 'View name' })).toBeVisible();
  await dialog.getByRole('textbox', { name: 'View name' }).fill('Needs docs');
  const query = dialog.getByRole('searchbox', { name: 'Search query' });
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await expect
      .poll(async () => query.evaluate((element) => element.getBoundingClientRect().width))
      .toBeGreaterThan(120);
    await expect
      .poll(async () =>
        dialog.locator('.saved-view-dialog__query-field').evaluate((element) => {
          const parent = element.parentElement!;
          return Math.abs(element.getBoundingClientRect().width - parent.getBoundingClientRect().width);
        }),
      )
      .toBeLessThan(2);
    await query.fill('tag:docs ');
    await expect(dialog.locator('[data-component="token-search-token"]')).toContainText('tag:docs');
    await page.screenshot({ path: `/private/tmp/hs2-y5hmrt-query-${width}.png` });
    await dialog.getByRole('button', { name: 'Clear search query' }).click();
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  await query.fill('tag:docs ');
  await expect(dialog.locator('[data-component="token-search-token"]')).toContainText('tag:docs');
  await page.waitForTimeout(250);
  await page.screenshot({
    path: '/private/tmp/hs2-r7gpjm-create-view-wide.png',
    clip: { x: 430, y: 175, width: 580, height: 550 },
  });
  await dialog.getByRole('button', { name: 'Create View' }).click();
  await expect(page.getByRole('button', { name: 'Needs docs', exact: true })).toHaveAttribute('aria-current', 'page');
  await expect(page.locator('.kui-toolbar-text', { hasText: 'Needs docs' })).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-DOCS"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-CODE"]')).toHaveCount(0);
  expect(views()).toEqual([{ id: 'needs-docs', name: 'Needs docs', query: 'tag:docs' }]);
  await expect(page.getByRole('button', { name: 'More actions for Needs docs' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Rename Needs docs' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Delete Needs docs' })).toHaveCount(0);
  await page.getByRole('button', { name: 'More actions for Needs docs' }).click();
  const menu = page.getByRole('menu', { name: 'Needs docs view actions' });
  await expect(menu).toBeVisible();
  await page.waitForTimeout(250);
  await page.screenshot({
    path: '/private/tmp/hs2-r7gpjm-view-menu-wide.png',
    clip: { x: 0, y: 260, width: 520, height: 400 },
  });
  await menu.getByText('Edit view…').click();
  await expect(dialog).toHaveAttribute('data-mode', 'rename');
  await expect(query).toBeVisible();
  await expect(dialog.locator('[data-component="token-search-token"]')).toContainText('tag:docs');
  await dialog.getByRole('textbox', { name: 'View name' }).fill('Documentation');
  await query.fill('tag:client ');
  await expect(dialog.locator('[data-component="token-search-token"]')).toContainText('tag:client');
  await page.waitForTimeout(250);
  await page.screenshot({
    path: '/private/tmp/hs2-r7gpjm-edit-view-wide.png',
    clip: { x: 430, y: 175, width: 580, height: 550 },
  });
  await dialog.getByRole('button', { name: 'Edit View' }).click();
  await expect(page.getByRole('button', { name: 'Documentation', exact: true })).toHaveAttribute(
    'aria-current',
    'page',
  );
  await expect(page.locator('.kui-toolbar-text', { hasText: 'Documentation' })).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-CODE"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-DOCS"]')).toHaveCount(0);
  expect(views()).toEqual([{ id: 'needs-docs', name: 'Documentation', query: 'tag:client' }]);
  await page.getByRole('button', { name: 'Add view', exact: true }).click();
  await expect(dialog).toHaveAttribute('data-mode', 'create');
  await expect(dialog.getByRole('textbox', { name: 'View name' })).toHaveValue('');
  await dialog.getByRole('textbox', { name: 'View name' }).fill('Fresh create');
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
  const savedRow = page.locator('[data-saved-view-id="custom:needs-docs"]');
  await page.setViewportSize({ width: 720, height: 760 });
  await page.getByRole('button', { name: 'Show project sidebar' }).click();
  await expect(savedRow).toBeInViewport();
  await savedRow.click({ button: 'right' });
  const narrowMenu = page.getByRole('menu', { name: 'Documentation view actions' });
  await expect(narrowMenu).toBeVisible();
  await page.waitForTimeout(250);
  await page.screenshot({
    path: '/private/tmp/hs2-r7gpjm-view-menu-narrow.png',
    clip: { x: 0, y: 230, width: 420, height: 430 },
  });
  await narrowMenu.getByText('Delete view…').click();
  const confirmation = page.locator('[data-component="saved-view-delete-dialog"]');
  await expect(confirmation).toContainText('Tickets are not affected.');
  await confirmation.getByRole('button', { name: 'Delete View' }).click();
  await expect(page.getByRole('button', { name: /Queue/ })).toHaveAttribute('aria-current', 'page');
  await expect(page.getByRole('button', { name: /Documentation/ })).toHaveCount(0);
  await expect(page.locator('[data-ticket-slug="HS2-CODE"]')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Search tickets' })).toBeVisible();
  expect(views()).toEqual([]);
});

test('keeps immediate saved-view query replacement focused when opening frames resume (HS2-N7XTP4)', async ({
  page,
}) => {
  await mockSavedViews(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project', exact: true }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const frames = await holdAnimationFrames(page);
  try {
    await page.getByRole('button', { name: 'Add view', exact: true }).click();
    const dialog = page.locator('[data-component="saved-view-dialog"]'),
      name = dialog.getByRole('textbox', { name: 'View name' }),
      query = dialog.getByRole('searchbox', { name: 'Search query' });
    await expect(query).toBeVisible();
    // Allow the native dialog's initial autofocus. Later application opening work
    // must not reclaim focus between selecting the query and inserting replacement text.
    await frames.evaluate((clock) => {
      clock.advance();
    });
    await expect(name).toBeFocused();
    await query.selectText();
    await frames.evaluate((clock) => {
      clock.advance();
    });
    await frames.evaluate((clock) => {
      clock.advance();
    });
    await expect(query).toBeFocused();
    await page.keyboard.insertText('before is:active after ');
    await expect(query.locator('[data-component="token-search-token"]')).toHaveAttribute(
      'data-token-value',
      'is:active',
    );
    await expect(query.locator('[data-token-search-text]')).toHaveText(['before ', ' after ']);
    await expect(name).toHaveValue('');
    await name.fill('Immediate query');
    await frames.evaluate((clock) => {
      clock.advance();
    });
    await expect(name).toBeFocused();
    await expect(name).toHaveValue('Immediate query');
    await frames.evaluate((clock) => {
      clock.restore();
    });
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await expect(query).toBeInViewport();
      await page.screenshot({ path: `/private/tmp/hs2-n7xtp4-immediate-query-${width}.png`, animations: 'disabled' });
    }
  } finally {
    await frames.evaluate((clock) => {
      clock.restore();
    });
    await frames.dispose();
  }
});

test('keeps saved-view cancel and reopen authoritative across delayed frames and keyboard input (HS2-N7XTP4)', async ({
  page,
}) => {
  await mockSavedViews(page);
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project', exact: true }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const frames = await holdAnimationFrames(page);
  try {
    const add = page.getByRole('button', { name: 'Add view', exact: true }),
      dialog = page.locator('[data-component="saved-view-dialog"]'),
      name = dialog.getByRole('textbox', { name: 'View name' }),
      query = dialog.getByRole('searchbox', { name: 'Search query' });
    await add.click();
    await expect(query).toBeVisible();
    await frames.evaluate((clock) => {
      clock.advance();
    });
    await expect(name).toBeFocused();
    await page.keyboard.insertText('Discarded name');
    await expect(name).toHaveValue('Discarded name');
    await page.keyboard.press('Tab');
    await expect(query).toBeFocused();
    await page.keyboard.insertText('discarded is:active query ');
    await expect(query.locator('[data-component="token-search-token"]')).toHaveAttribute(
      'data-token-value',
      'is:active',
    );
    await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(dialog).not.toHaveAttribute('open', '');
    await frames.evaluate((clock) => {
      clock.advance();
    });
    await frames.evaluate((clock) => {
      clock.advance();
    });
    await expect(dialog).not.toHaveAttribute('open', '');
    await expect(query).toBeHidden();
    await add.focus();
    await page.keyboard.press('Enter');
    await expect(query).toBeVisible();
    await frames.evaluate((clock) => {
      clock.advance();
    });
    await expect(name).toBeFocused();
    await expect(name).toHaveValue('');
    await expect(query).toHaveText('');
    await page.keyboard.press('Tab');
    await expect(query).toBeFocused();
    await frames.evaluate((clock) => {
      clock.advance();
    });
    await frames.evaluate((clock) => {
      clock.advance();
    });
    await page.keyboard.insertText('tag:docs ');
    await expect(query.locator('[data-component="token-search-token"]')).toHaveAttribute(
      'data-token-value',
      'tag:docs',
    );
    await expect(name).toHaveValue('');
    await page.keyboard.press('Escape');
    await expect(dialog).not.toHaveAttribute('open', '');
    await frames.evaluate((clock) => {
      clock.advance();
    });
    await expect(query).toBeHidden();
    await frames.evaluate((clock) => {
      clock.restore();
    });
    await add.click();
    await expect(name).toBeFocused();
    await expect(name).toHaveValue('');
    await expect(query).toHaveText('');
    await page.keyboard.insertText('Refilled name');
    await expect(name).toHaveValue('Refilled name');
    await page.keyboard.press('Tab');
    await expect(query).toBeFocused();
    await query.fill('refilled is:active after ');
    await expect(query.locator('[data-component="token-search-token"]')).toHaveAttribute(
      'data-token-value',
      'is:active',
    );
    await expect(name).toHaveValue('Refilled name');
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await expect(name).toBeInViewport();
      await page.screenshot({ path: `/private/tmp/hs2-zqnw62-name-reset-${width}.png`, animations: 'disabled' });
    }
  } finally {
    await frames.evaluate((clock) => {
      clock.restore();
    });
    await frames.dispose();
  }
});
