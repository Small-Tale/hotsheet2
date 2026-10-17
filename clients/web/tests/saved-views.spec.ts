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
            display_name: 'Hot Sheet Git',
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
  await page.routeWebSocket(/\/__hotsheet\/project-api\/[^/]+\/ws\/sync(?:\?.*)?$/, () => undefined);
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
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add view' }).click();
  const dialog = page.locator('[data-dialog-component="saved-view-dialog"]');
  await expect(dialog.getByRole('textbox', { name: 'View Name' })).toBeVisible();
  await dialog.getByRole('textbox', { name: 'View Name' }).fill('Needs docs');
  const query = dialog.getByRole('searchbox', { name: 'Search query' });
  // The query is Kerf's form-field presentation, not a toolbar group (HS2-E40KC0).
  await expect(dialog.locator('[data-component="search-field"][data-presentation="form-field"]')).toHaveCount(1);
  await expect(dialog.locator('.kui-toolbar')).toHaveCount(0);
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await expect
      .poll(async () => query.evaluate((element) => element.getBoundingClientRect().width))
      .toBeGreaterThan(120);
    // The query label and field line up with the View name input's label and field.
    await expect
      .poll(async () =>
        dialog.evaluate((node) => {
          const input = node.querySelector<HTMLElement>('wa-input[name="saved-view-name"]')!,
            inputLabel = input.shadowRoot!.querySelector<HTMLElement>('[part~="form-control-label"]')!,
            inputBox = input.shadowRoot!.querySelector<HTMLElement>('[part~="base"]')!.getBoundingClientRect(),
            queryLabel = node.querySelector<HTMLElement>('[data-search-field-form-label="saved-view-query"]')!,
            queryBox = node
              .querySelector<HTMLElement>('[data-component="search-field"][data-search-field-id="saved-view-query"]')!
              .getBoundingClientRect(),
            style = (element: HTMLElement) => {
              const computed = getComputedStyle(element);
              return [computed.fontSize, computed.fontWeight, computed.textTransform].join(' ');
            },
            labelLeft = (element: HTMLElement) => {
              const range = document.createRange();
              range.selectNodeContents(element);
              return range.getBoundingClientRect().left;
            };
          return {
            typography: style(inputLabel) === style(queryLabel),
            labelInset: Math.abs(labelLeft(inputLabel) - labelLeft(queryLabel)) < 1,
            edges: Math.abs(inputBox.left - queryBox.left) < 1 && Math.abs(inputBox.right - queryBox.right) < 1,
          };
        }),
      )
      .toEqual({ typography: true, labelInset: true, edges: true });
    await query.fill('tag:docs ');
    await expect(dialog.locator('[data-component="search-field-token"]')).toContainText('tag:docs');
    await page.screenshot({ path: `target/visual-captures/hs2-y5hmrt-query-${width}.png` });
    await dialog.getByRole('button', { name: 'Clear search query' }).click();
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  await query.fill('tag:docs ');
  await expect(dialog.locator('[data-component="search-field-token"]')).toContainText('tag:docs');
  await page.waitForTimeout(250);
  await page.screenshot({
    path: 'target/visual-captures/hs2-r7gpjm-create-view-wide.png',
    clip: { x: 430, y: 175, width: 580, height: 550 },
  });
  await dialog.getByRole('button', { name: 'Create View' }).click();
  await expect(page.getByRole('button', { name: 'Needs docs', exact: true })).toHaveAttribute('aria-current', 'page');
  await expect(page.locator('.kui-toolbar-text', { hasText: 'Needs docs' })).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-DOCS"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-CODE"]')).toHaveCount(0);
  expect(views()).toEqual([{ id: 'needs-docs', name: 'Needs docs', query: 'tag:docs' }]);
  await expect(page.getByRole('button', { name: 'More Actions for Needs docs' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Rename Needs docs' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Delete Needs docs' })).toHaveCount(0);
  await page.getByRole('button', { name: 'More Actions for Needs docs' }).click();
  const menu = page.locator('[data-context-menu="saved-view"]');
  await expect(menu.getByRole('menu', { name: /View Actions$/ })).toBeVisible();
  await page.waitForTimeout(250);
  await page.screenshot({
    path: 'target/visual-captures/hs2-r7gpjm-view-menu-wide.png',
    clip: { x: 0, y: 260, width: 520, height: 400 },
  });
  await menu.getByText('Edit View…').click();
  await expect(dialog).toHaveAttribute('data-mode', 'rename');
  await expect(query).toBeVisible();
  await expect(dialog.locator('[data-component="search-field-token"]')).toContainText('tag:docs');
  await dialog.getByRole('textbox', { name: 'View Name' }).fill('Documentation');
  await query.fill('tag:client ');
  await expect(dialog.locator('[data-component="search-field-token"]')).toContainText('tag:client');
  await page.waitForTimeout(250);
  await page.screenshot({
    path: 'target/visual-captures/hs2-r7gpjm-edit-view-wide.png',
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
  await expect(dialog.getByRole('textbox', { name: 'View Name' })).toHaveValue('');
  await dialog.getByRole('textbox', { name: 'View Name' }).fill('Fresh create');
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
  const savedRow = page.locator('[data-saved-view-id="custom:needs-docs"]');
  await page.setViewportSize({ width: 720, height: 760 });
  await page.getByRole('button', { name: 'Show Project Sidebar' }).click();
  await expect(savedRow).toBeInViewport();
  await savedRow.click({ button: 'right' });
  const narrowMenu = page.locator('[data-context-menu="saved-view"]');
  await expect(narrowMenu.getByRole('menu', { name: /View Actions$/ })).toBeVisible();
  await page.waitForTimeout(250);
  await page.screenshot({
    path: 'target/visual-captures/hs2-r7gpjm-view-menu-narrow.png',
    clip: { x: 0, y: 230, width: 420, height: 430 },
  });
  // The phone sidebar overlay stays open through the menu and the dialog it launches: presses in
  // those portals are not outside presses (HS2-5APX20, Kerf `keepOpenOn`).
  const sidebarRail = page.locator('#app-left-rail');
  await narrowMenu.getByText('Delete View…').click();
  await expect(sidebarRail).toHaveAttribute('data-collapsed', 'false');
  const confirmation = page.locator('[data-dialog-component="saved-view-delete-dialog"]');
  await expect(confirmation).toContainText('Tickets are not affected.');
  await page.screenshot({
    path: 'target/visual-captures/hs2-5apx20-sidebar-delete-dialog-narrow.png',
    animations: 'disabled',
  });
  await confirmation.getByRole('button', { name: 'Delete View' }).click();
  await expect(confirmation).toHaveJSProperty('open', false);
  await expect(sidebarRail).toHaveAttribute('data-collapsed', 'false');
  await expect(page.getByRole('button', { name: 'Hide Project Sidebar' })).toBeVisible();
  await page.screenshot({
    path: 'target/visual-captures/hs2-5apx20-sidebar-after-delete-narrow.png',
    animations: 'disabled',
  });
  await expect(page.getByRole('button', { name: /Queue/ })).toHaveAttribute('aria-current', 'page');
  await expect(page.getByRole('button', { name: /Documentation/ })).toHaveCount(0);
  await expect(page.locator('[data-ticket-slug="HS2-CODE"]')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Search tickets' })).toBeVisible();
  expect(views()).toEqual([]);
  // An ordinary outside press, on the backdrop, still closes the overlay.
  await page.locator('.app-shell__scrim').click({ position: { x: 700, y: 400 } });
  await expect(sidebarRail).toHaveAttribute('data-collapsed', 'true');
});

test('scopes the search bar to a selected shared view without populating it (HS2-50R1YQ)', async ({ page }) => {
  const views = await mockSavedViews(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add view' }).click();
  const dialog = page.locator('[data-dialog-component="saved-view-dialog"]');
  await dialog.getByRole('textbox', { name: 'View Name' }).fill('Needs docs');
  await dialog.getByRole('searchbox', { name: 'Search query' }).fill('tag:docs ');
  await dialog.getByRole('button', { name: 'Create View' }).click();
  expect(views()).toEqual([{ id: 'needs-docs', name: 'Needs docs', query: 'tag:docs' }]);

  // Selecting the view shows its tickets like any other view; the search bar stays closed and empty.
  await expect(page.getByRole('button', { name: 'Needs docs', exact: true })).toHaveAttribute('aria-current', 'page');
  await expect(page.locator('[data-ticket-slug="HS2-DOCS"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-CODE"]')).toHaveCount(0);
  await expect(page.getByRole('searchbox', { name: 'Search tickets' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Search tickets' }).click();
  const search = page.getByRole('searchbox', { name: 'Search tickets' });
  await expect(search).toHaveText('');
  await expect(
    page.locator('[data-search-field-id="workspace-search"] [data-component="search-field-token"]'),
  ).toHaveCount(0);

  // A search-bar query narrows the view: (tag:docs) AND (bar query).
  await search.pressSequentially('parser');
  await expect(page.locator('[data-ticket-slug="HS2-CODE"]')).toHaveCount(0);
  await expect(page.locator('[data-ticket-slug="HS2-DOCS"]')).toHaveCount(0);
  await search.press('ControlOrMeta+a');
  await search.pressSequentially('saved');
  await expect(page.locator('[data-ticket-slug="HS2-DOCS"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-CODE"]')).toHaveCount(0);
  // A new view starts from what is visible: the selected view scoped by the search-bar query.
  await page.getByRole('button', { name: 'Add view', exact: true }).click();
  const seeded = dialog.getByRole('searchbox', { name: 'Search query' });
  await expect(seeded).toContainText(') AND (saved)');
  await expect(seeded.locator('[data-component="search-field-token"]')).toContainText('tag:docs');
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
  await search.click();

  // Leaving the view keeps the bar's own query and drops the view scope.
  await search.press('ControlOrMeta+a');
  await search.press('Backspace');
  await page.getByRole('button', { name: /Queue/ }).click();
  await expect(page.locator('[data-ticket-slug="HS2-CODE"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-DOCS"]')).toBeVisible();
  await page.getByRole('button', { name: 'Needs docs', exact: true }).click();
  await expect(page.locator('[data-ticket-slug="HS2-DOCS"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-CODE"]')).toHaveCount(0);
  await expect(search).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Search tickets' })).toBeVisible();
});

test('keeps immediate saved-view query replacement focused when opening frames resume (HS2-N7XTP4)', async ({
  page,
}) => {
  // Kerf places the caret back into the editor one frame after a chip commits; since beta.64 that
  // deferred placement yields when focus has moved to the name field (KF-DNVMQE, HS2-TNE7V7).
  await mockSavedViews(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project', exact: true }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  const frames = await holdAnimationFrames(page);
  try {
    await page.getByRole('button', { name: 'Add view', exact: true }).click();
    const dialog = page.locator('[data-dialog-component="saved-view-dialog"]'),
      name = dialog.getByRole('textbox', { name: 'View Name' }),
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
    await expect(query.locator('[data-component="search-field-token"]')).toHaveAttribute(
      'data-token-value',
      'is:active',
    );
    await expect(query.locator('[data-search-field-text]')).toHaveText(['before ', ' after ']);
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
      await page.screenshot({
        path: `target/visual-captures/hs2-n7xtp4-immediate-query-${width}.png`,
        animations: 'disabled',
      });
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
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project', exact: true }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  const frames = await holdAnimationFrames(page);
  try {
    const add = page.getByRole('button', { name: 'Add view', exact: true }),
      dialog = page.locator('[data-dialog-component="saved-view-dialog"]'),
      name = dialog.getByRole('textbox', { name: 'View Name' }),
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
    await expect(query.locator('[data-component="search-field-token"]')).toHaveAttribute(
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
    await expect(query.locator('[data-component="search-field-token"]')).toHaveAttribute(
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
    await expect(query.locator('[data-component="search-field-token"]')).toHaveAttribute(
      'data-token-value',
      'is:active',
    );
    await expect(name).toHaveValue('Refilled name');
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await expect(name).toBeInViewport();
      await page.screenshot({
        path: `target/visual-captures/hs2-zqnw62-name-reset-${width}.png`,
        animations: 'disabled',
      });
    }
  } finally {
    await frames.evaluate((clock) => {
      clock.restore();
    });
    await frames.dispose();
  }
});

test('offers tag completion, the date helper, and syntax help inside the saved-view dialog (HS2-N5G6JS)', async ({
  page,
}) => {
  const views = await mockSavedViews(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add view' }).click();
  const dialog = page.locator('[data-dialog-component="saved-view-dialog"]');
  const query = dialog.getByRole('searchbox', { name: 'Search query' });
  await dialog.getByRole('textbox', { name: 'View Name' }).fill('Recent docs');
  // The same in-place tag completion as the workspace search, fed by the project's tags.
  await query.fill('tag:');
  // Kerf's model renders the in-place tag completion inside the field (HS2-5JXBQY).
  const suggestions = dialog.locator('.kui-search-field__suggestions');
  await expect(suggestions.getByRole('button')).toHaveText(['tag:client', 'tag:docs']);
  await suggestions.getByRole('button', { name: 'tag:docs' }).click();
  const chips = dialog.locator('[data-component="search-field-token"]');
  await expect(chips).toHaveCount(1);
  await expect(chips.first()).toHaveAttribute('data-token-value', 'tag:docs');
  await expect(suggestions).toHaveCount(0);
  await expect(query).toBeFocused();
  await expect(dialog.locator('input[name="saved-view-query"]')).toHaveValue('tag:docs');
  // Syntax help opens over the dialog and closes again from the same button.
  await dialog.getByRole('button', { name: 'Search Syntax Help' }).click();
  const help = dialog.getByRole('dialog', { name: 'Search syntax' });
  await expect(help.locator('[data-syntax-group]')).toHaveText(['Tags', 'Content', 'Workflow', 'Dates']);
  // The help is a Kerf anchored surface hosted inside the Kerf dialog (KF-F52T5D, HS2-8TE69P,
  // HS2-BH7NZ4): it floats outside the field's flow, stays in the viewport, and keeps focus in the field.
  await expect(help).toBeVisible();
  expect(
    await help.evaluate((node) => {
      const box = node.getBoundingClientRect();
      return {
        position: getComputedStyle(node).position,
        hosted: node.closest('dialog.kui-dialog-surface') !== null,
        inFieldFlow: node.closest('.ticket-search-form-field') !== null,
        onScreen: box.top >= 0 && box.bottom <= innerHeight + 1 && box.left >= 0 && box.right <= innerWidth + 1,
        tall: box.height > 200,
      };
    }),
  ).toEqual({ position: 'fixed', hosted: true, inFieldFlow: false, onScreen: true, tall: true });
  // Pressing inside the help keeps it open and the field in the dialog.
  await help.locator('[data-syntax-group="Tags"]').click();
  await expect(help).toBeVisible();
  await page.screenshot({
    path: 'target/visual-captures/hs2-n5g6js-saved-view-help-wide.png',
    clip: { x: 380, y: 60, width: 680, height: 840 },
  });
  // On a phone the help still fits beside the dialog's field without horizontal overflow.
  const wide = page.viewportSize()!;
  await page.setViewportSize({ width: 390, height: 844 });
  await expect
    .poll(() =>
      help.evaluate((node) => {
        const box = node.getBoundingClientRect();
        return box.left >= 0 && box.right <= innerWidth && box.top >= 0 && box.bottom <= innerHeight + 1;
      }),
    )
    .toBe(true);
  await page.screenshot({ path: 'target/visual-captures/hs2-8te69p-saved-view-help-phone.png' });
  await page.setViewportSize(wide);
  await dialog.getByRole('button', { name: 'Search Syntax Help' }).click();
  await expect(help).toHaveCount(0);
  // Enter inside the date helper applies the date instead of submitting the form.
  await query.click();
  await page.keyboard.press('End');
  await query.pressSequentially(' updated-after:');
  const helper = dialog.getByRole('group', { name: 'Date and time helper' });
  await helper.getByLabel('Date').fill('2026-09-01');
  await helper.getByLabel('Date').press('Enter');
  await expect(dialog).toHaveAttribute('data-controlled-open', 'true');
  await expect(chips).toHaveCount(2);
  await expect(chips.nth(1)).toHaveAttribute('data-token-value', 'updated-after:2026-09-01');
  await expect(helper).toHaveCount(0);
  expect(views()).toEqual([]);
  await page.screenshot({
    path: 'target/visual-captures/hs2-n5g6js-saved-view-chips-wide.png',
    clip: { x: 430, y: 175, width: 580, height: 420 },
  });
  await dialog.getByRole('button', { name: 'Create View' }).click();
  await expect(page.getByRole('button', { name: 'Recent docs', exact: true })).toHaveAttribute('aria-current', 'page');
  expect(views()).toEqual([{ id: 'recent-docs', name: 'Recent docs', query: 'tag:docs updated-after:2026-09-01' }]);
});
