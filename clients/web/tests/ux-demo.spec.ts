import { expect, type Locator, test } from '@playwright/test';

import { expectResponsiveFeedbackRectangle, measureFeedbackRectangle } from './dev-review-performance';
import { editLongTitleThroughWrappingEditor } from './title-editor-geometry';

test('clears the halted terminal example from its menu at wide and phone widths (HS2-6J79SA)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await page.goto('/ux-demo?component=terminal-dashboard&dev-review=false');
    const tile = page.locator('[data-component="terminal-tile"][data-terminal-key="demo:halted"]');
    await expect(tile).toHaveAttribute('data-halted', 'true');
    await tile.getByRole('button', { name: 'More actions' }).click();
    await page.getByRole('menuitem', { name: 'Clear stopped state' }).click();
    await expect(tile).not.toHaveAttribute('data-halted', 'true');
    await expect(tile.locator('.terminal-tile__halt')).toHaveCount(0);
  }
});

test('previews project loading and ticket update progress in the app indicator (HS2-TPF3EB)', async ({ page }) => {
  await page.goto('/ux-demo?component=app-loading-indicator');
  const indicator = page.locator('[data-component="app-loading-indicator"]');
  await expect(indicator).toHaveAttribute('data-loading-kind', 'project');
  await expect(indicator).toContainText('Loading…');
  for (const [action, text] of [
    ['start', 'Updating tickets… 0 of 2'],
    ['progress', 'Updating tickets… 1 of 2'],
    ['complete', 'Updating tickets… 2 of 2'],
  ]) {
    await page.locator(`[data-app-loading-demo="${action}"]`).click();
    await expect(indicator).toHaveAttribute('data-loading-kind', 'tickets');
    await expect(indicator).toContainText(text);
    if (action === 'progress') {
      await page.screenshot({ path: test.info().outputPath('hs2-tpf3eb-progress-1280.png') });
      await page.setViewportSize({ width: 390, height: 844 });
      await page.screenshot({ path: test.info().outputPath('hs2-tpf3eb-progress-390.png') });
      await page.setViewportSize({ width: 1280, height: 844 });
    }
  }
  await page.locator('[data-app-loading-demo="project"]').click();
  await expect(indicator).toHaveAttribute('data-loading-kind', 'project');
  await expect(indicator).toContainText('Loading…');
  await page.locator('[data-app-loading-demo="start"]').click();
  await expect(indicator).toContainText('Updating tickets… 0 of 2');
});

test('previews every ticket-source dialog state at wide and narrow widths (HS2-7FYYN9)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await page.goto('/ux-demo?component=ticket-source-setup-dialog&dev-review=false');
    const dialog = page.locator('[data-ticket-source-setup-dialog]'),
      scenario = dialog.locator('[data-demo-ticket-source-scenario] wa-select');
    for (const [value, expected] of [
      ['root', 'Set up ticket support'],
      ['signed-out', 'Sign in with GitHub'],
      ['accounts', 'Use a GitHub account already signed in on this computer'],
      ['jira-account', 'This project still enters its own project key.'],
      ['waiting', 'ABCD-EFGH'],
      ['authorized', 'Signed in to GitHub.'],
      ['authorized-existing-default', 'Signed in to GitHub.'],
      ['editing', 'Save changes'],
      ['editing-shared', 'Also used by marketing-site.'],
      ['removing', 'Remove Product issues from Demo project?'],
      ['busy', 'Saving…'],
      ['remote', 'Back up this ticket repository'],
    ] as const) {
      await scenario.evaluate((node: HTMLElement & { value: string }, selected) => {
        node.value = selected;
        node.dispatchEvent(new Event('change', { bubbles: true }));
      }, value);
      await expect(dialog).toHaveAttribute('data-preview-scenario', value);
      await expect(dialog).toContainText(expected);
      if (value === 'editing') {
        await expect(dialog.locator('input[name="attachment-repo"]')).toHaveJSProperty(
          'value',
          'small-tale/hotsheet-assets',
        );
        await expect(dialog.getByRole('button', { name: 'Disable' })).toBeVisible();
      }
      if (value === 'authorized')
        await expect(dialog.locator('input[name="attachment-repo"]')).toHaveJSProperty('value', '');
      if (value === 'authorized' || value === 'authorized-existing-default')
        await expect(dialog.locator('wa-checkbox[name="make-default"]')).toHaveJSProperty(
          'checked',
          value === 'authorized',
        );
      if (value === 'authorized-existing-default') {
        await scenario.evaluate((node: HTMLElement & { value: string }) => {
          node.value = 'authorized';
          node.dispatchEvent(new Event('change', { bubbles: true }));
        });
        await expect(dialog.locator('wa-checkbox[name="make-default"]')).toHaveJSProperty('checked', true);
        await scenario.evaluate((node: HTMLElement & { value: string }) => {
          node.value = 'authorized-existing-default';
          node.dispatchEvent(new Event('change', { bubbles: true }));
        });
        await expect(dialog.locator('wa-checkbox[name="make-default"]')).toHaveJSProperty('checked', false);
      }
      await dialog.evaluate(async (node) => {
        await Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished));
      });
      await page.screenshot({ path: `target/visual-captures/hs2-7fyyn9-${value}-${width}.png`, fullPage: true });
    }
  }
});

test('preserves navigation geometry through Kerf List layouts (HS2-ZMN977)', async ({ page }) => {
  test.setTimeout(90_000);
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: width === 390 ? 1200 : 844 });
    for (const [component, label, count] of [
      ['settings-navigation', 'Project Settings', 7],
      ['notification-navigation', 'Notification views', 3],
    ] as const) {
      await page.goto(`/ux-demo?component=${component}&dev-review=false`);
      // The navigator's Pane content is the navigation landmark; its group list sits in an app wrapper.
      // The notification demo also shows a paused variant (HS2-QYA9SC); measure the first navigator.
      const navigation = page.getByRole('navigation', { name: label, exact: true }).first(),
        list = navigation.locator('[data-component="list"]:has(> [data-component="list-item"])').first(),
        rows = list.getByRole('button');
      await expect(rows).toHaveCount(count);
      await expect(list).toHaveCSS('display', 'flex');
      await expect(list).toHaveCSS('flex-direction', 'column');
      await expect(list).toHaveCSS('gap', '2px');
      await expect(list).toHaveCSS('overflow-y', 'visible');
      const pane = navigation.locator('xpath=ancestor::aside');
      await expect(pane.locator('.kui-pane__content')).toHaveCSS('overflow-y', 'auto');
      await expect(pane.locator('.kui-toolbar')).not.toHaveAttribute('divider-sides');
      await rows.first().focus();
      await page.keyboard.press('Tab');
      await expect(rows.nth(1)).toBeFocused();
      const geometry = await rows.evaluateAll((buttons) =>
        buttons.map((button) => {
          const row = button.getBoundingClientRect(),
            icon = button.querySelector('.kui-list-item__icon svg')!.getBoundingClientRect();
          return {
            left: row.left,
            right: row.right,
            iconWidth: icon.width,
            iconOffset: Math.abs(icon.y + icon.height / 2 - (row.y + row.height / 2)),
          };
        }),
      );
      for (const row of geometry) {
        expect(row.left).toBeGreaterThanOrEqual(0);
        expect(row.right).toBeLessThanOrEqual(width);
        expect(row.iconWidth).toBe(18);
        expect(row.iconOffset).toBeLessThanOrEqual(1);
      }
      await pane.screenshot({ path: `target/visual-captures/hs2-zmn977-${component}-${width}.png` });
    }
  }
});

test('demonstrates native List gap and bounded scroll ownership (HS2-ZMN977)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await page.goto('/ux-demo?component=list&dev-review=false');
    const examples = page.getByRole('region', { name: 'List layout variants', exact: true }),
      lists = examples.locator('[data-component="list"]'),
      scroller = lists.nth(2);
    await expect(lists).toHaveCount(3);
    await expect(lists.nth(0)).toHaveCSS('gap', '0px');
    await expect(lists.nth(1)).toHaveCSS('gap', '8px');
    await expect(scroller).toHaveCSS('gap', '16px');
    await expect(scroller).toHaveAttribute('divider-sides', 'trbl');
    await expect(scroller).toHaveCSS('overflow-y', 'auto');
    const geometry = await scroller.evaluate((node) => {
      node.scrollTop = node.scrollHeight;
      return { height: node.clientHeight, scrollHeight: node.scrollHeight, scrollTop: node.scrollTop };
    });
    expect(geometry.height).toBe(192);
    expect(geometry.scrollHeight).toBeGreaterThan(geometry.height);
    expect(geometry.scrollTop).toBeGreaterThan(0);
    await examples.screenshot({ path: `target/visual-captures/hs2-zmn977-list-demo-${width}.png` });
  }
});

test('preserves a selected catalog component through a real reload (HS2-9TZ9AF)', async ({ page }) => {
  await page.goto('/ux-demo?dev-review=false');
  const catalog = page.getByRole('navigation', { name: 'UX components components' });
  await expect(catalog).toBeVisible();
  await catalog.getByRole('button', { name: /TicketRow/ }).click();
  await expect(page).toHaveURL('/ux-demo?dev-review=false&component=ticket-row');
  await expect(page.getByRole('region', { name: 'TicketRow demo' })).toBeVisible();
  await page.reload();
  await expect(page).toHaveURL('/ux-demo?dev-review=false&component=ticket-row');
  await expect(page.getByRole('heading', { name: 'TicketRow', exact: true })).toBeVisible();
  await expect(page.getByRole('region', { name: 'TicketRow demo' })).toBeVisible();
});

test('keeps the catalog filter and placeholder across rerenders (HS2-458BS5, HS2-WATFM5)', async ({ page }) => {
  await page.goto('/ux-demo?dev-review=false');
  const sidebar = page.locator('[data-catalog-sidebar]');
  const filter = page.locator('[data-catalog-filter]');
  const filterField = filter.locator('xpath=..');
  await expect(sidebar).toBeVisible();
  await expect(filter).toBeVisible();
  await expect(filterField).toHaveAttribute('data-placeholder-visible', 'true');
  await sidebar.locator('[data-item-id="value-table"]').scrollIntoViewIfNeeded();
  await expect(filter).toBeInViewport();

  await filter.fill('TiCkEtRoW');
  await expect(filterField).toHaveAttribute('data-placeholder-visible', 'false');
  await expect(sidebar.locator('[data-item-id="ticket-row"]')).toBeVisible();
  await expect(sidebar.locator('[data-item-id="app-shell"]')).toBeHidden();
  await sidebar.locator('[data-item-id="ticket-row"]').click();
  await expect(page).toHaveURL('/ux-demo?dev-review=false&component=ticket-row');
  await expect(filter).toHaveText('TiCkEtRoW');
  await expect(filterField).toHaveAttribute('data-placeholder-visible', 'false');

  await page.getByRole('button', { name: 'Clear filter' }).click();
  await expect(filter).toHaveText('');
  await expect(filterField).toHaveAttribute('data-placeholder-visible', 'true');
  await expect(filter).toBeFocused();
  await expect(sidebar.locator('[data-item-id="app-shell"]')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Clear filter' })).toHaveCount(0);
  await page.screenshot({ path: 'target/visual-captures/hs2-458bs5-catalog-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: 'Show UX components catalog' }).click();
  await expect(filter).toBeVisible();
  await expect
    .poll(async () => {
      const box = await filter.boundingBox();
      return Boolean(box && box.x >= 0 && box.x + box.width <= 390);
    })
    .toBe(true);
  await page.screenshot({ path: 'target/visual-captures/hs2-458bs5-catalog-narrow.png', fullPage: true });
});

test('presents catalog navigation, controls, and responsive stage (HS2-9TZ9AF, HS2-ARMAX3)', async ({ page }) => {
  await page.goto('/ux-demo');
  await expect(page.getByRole('heading', { name: 'UX components' })).toBeVisible();
  const catalogShell = page.locator('[data-component="catalog"]'),
    reviewToggle = page.locator('[data-action="toggle-dev-review"]');
  await expect(catalogShell).toBeVisible();
  // Dev Review is opt-in (HS2-TCACFR): off by default, the toggle opts in and back out through the URL.
  await expect(reviewToggle).toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('.hs-dev-review')).toHaveCount(0);
  await reviewToggle.click();
  await expect(page).toHaveURL('/ux-demo?dev-review=1');
  await expect(reviewToggle).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('.hs-dev-review')).toBeVisible();
  await reviewToggle.click();
  await expect(page).toHaveURL('/ux-demo');
  await expect(reviewToggle).toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('.hs-dev-review')).toHaveCount(0);
  const catalog = page.getByRole('navigation', { name: 'UX components components' });
  await expect(catalog.locator('[data-item-id="app-shell"]')).not.toHaveCSS('color', 'rgb(174, 174, 178)');
  await expect(catalog.locator('[data-item-id="ticket-row"]')).not.toHaveCSS('color', 'rgb(174, 174, 178)');
  await expect(catalog.locator('[data-component="list-header"]')).not.toHaveCount(0);
  await expect(catalog.locator('[data-component="list-item"]')).not.toHaveCount(0);
  await page.screenshot({ path: 'target/visual-captures/hs2-ecdq5k-kerf-catalog-wide.png', fullPage: true });
  const firstCatalogList = catalog.locator('[data-component="list"] > [data-component="list"]').first();
  const firstCatalogItem = firstCatalogList.locator('[data-component="list-item"]').first();
  const [listBox, itemBox] = await Promise.all([firstCatalogList.boundingBox(), firstCatalogItem.boundingBox()]);
  expect(itemBox!.x - listBox!.x).toBeCloseTo(8, 0);
  await expect(catalog.getByText('Ticket workspace · List', { exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'TagChip', exact: true })).toBeVisible();
  await catalog.getByRole('button', { name: /TicketRow/ }).click();
  await expect(page).toHaveURL('/ux-demo?component=ticket-row');
  await expect(page.getByRole('heading', { name: 'TicketRow', exact: true })).toBeVisible();
  await expect(page.getByRole('region', { name: 'TicketRow demo' })).toBeVisible();
  const catalogTop = await page
    .getByRole('complementary', { name: 'UX components catalog' })
    .evaluate((node) => node.getBoundingClientRect().top);
  await page.evaluate(() => {
    window.scrollTo(0, 500);
  });
  await expect
    .poll(() =>
      page
        .getByRole('complementary', { name: 'UX components catalog' })
        .evaluate((node) => node.getBoundingClientRect().top),
    )
    .toBeCloseTo(catalogTop, 0);
  // Kerf 5.0.0-beta.51 renders catalog relationships as a Components dropdown (HS2-KMDJRH).
  const relationships = page.locator('[data-catalog-related]');
  await relationships.getByRole('button', { name: /Component/ }).click();
  await expect(relationships.getByText('Used by', { exact: true })).toBeVisible();
  await expect(relationships.getByText('Uses', { exact: true })).toBeVisible();
  await relationships.getByText('TagChip', { exact: true }).click();
  await expect(page).toHaveURL('/ux-demo?component=tag-chip');
  await expect(page.getByRole('heading', { name: 'TagChip', exact: true })).toBeVisible();
  const collapse = page.getByRole('button', { name: 'Hide UX components catalog' });
  await collapse.click();
  await expect(catalogShell).toHaveAttribute('data-sidebar-collapsed', 'true');
  await page.getByRole('button', { name: 'Show UX components catalog' }).click();
  await expect(catalogShell).toHaveAttribute('data-sidebar-collapsed', 'false');
  await catalog.getByRole('button', { name: /ProjectTabBar/ }).click();
  await expect(page).toHaveURL('/ux-demo?component=project-tabs');
  await expect(page.getByRole('heading', { name: 'ProjectTabBar', exact: true })).toBeVisible();
  await expect(catalogShell.locator('[data-catalog-stage]')).toHaveAttribute('data-background-style', 'checkerboard');
  await page.screenshot({ path: 'target/visual-captures/hs2-armax3-catalog-wide.png', fullPage: true });
  await catalog.locator('[data-item-id="list"]').click();
  await expect(page.getByRole('region', { name: 'List layout variants', exact: true })).toBeVisible();
  await catalog.getByRole('button', { name: /ValueTable/ }).click();
  await expect(page.getByRole('heading', { name: 'ValueTable', exact: true })).toBeVisible();
  await catalog.getByRole('button', { name: /AppShell/ }).click();
  await catalog.getByRole('button', { name: /ProjectTabBar/ }).click();
  await expect(page.getByRole('heading', { name: 'ProjectTabBar', exact: true })).toBeVisible();
  const theme = page.getByRole('button', { name: 'Use dark theme' });
  await theme.click();
  await expect(page.locator('html')).toHaveClass(/wa-dark/);
  await expect(page.getByRole('button', { name: 'Use light theme' })).toBeVisible();
  await page.setViewportSize({ width: 760, height: 800 });
  await page.getByRole('button', { name: 'Hide UX components catalog' }).click();
  await expect(catalogShell).toHaveAttribute('data-sidebar-collapsed', 'true');
  await page.screenshot({ path: 'target/visual-captures/hs2-armax3-catalog-narrow.png', fullPage: true });
});

test('reveals deep-linked and newly selected catalog entries without moving focus', async ({ page }) => {
  await page.addInitScript(() => {
    (window as Window & { catalogReveals?: string[] }).catalogReveals = [];
    Element.prototype.scrollIntoView = function () {
      (window as Window & { catalogReveals?: string[] }).catalogReveals?.push(
        (this as HTMLElement).dataset.itemId ?? '',
      );
    };
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=hs1-migration-banner');
  await expect
    .poll(() => page.evaluate(() => (window as Window & { catalogReveals?: string[] }).catalogReveals ?? []))
    .toContain('hs1-migration-banner');
  const focus = page.locator('[data-action="toggle-dev-review"]');
  await focus.focus();
  await page
    .locator('[data-item-id="tag-chip"]')
    .first()
    .evaluate((node) => {
      (node as HTMLElement).click();
    });
  await expect
    .poll(() => page.evaluate(() => (window as Window & { catalogReveals?: string[] }).catalogReveals ?? []))
    .toContain('tag-chip');
  await expect(focus).toBeFocused();
});

test('renders the canonical ListItem and ListHeader demo routes (HS2-YGWNY7)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=list-item&dev-review=false');
  const listItemDemo = page.getByRole('region', { name: 'ListItem demo' });
  await expect(page.getByRole('heading', { name: 'ListItem', exact: true })).toBeVisible();
  await expect(listItemDemo).toBeVisible();
  await expect(listItemDemo.getByRole('heading', { name: 'Standard' })).toBeVisible();
  await expect(listItemDemo.locator('[data-component="list-item"]')).toHaveCount(7);
  await expect(listItemDemo.locator('.list-item-demo__copy small')).toHaveCSS('display', 'block');
  await expect(page.getByRole('region', { name: 'ListItem planned demo' })).toHaveCount(0);
  await page.screenshot({ path: 'target/visual-captures/hs2-ygwny7-list-item-wide.png', fullPage: true });

  await page.goto('/ux-demo?component=list-header&dev-review=false');
  const listHeaderDemo = page.getByRole('region', { name: 'ListHeader demo' });
  await expect(page.getByRole('heading', { name: 'ListHeader', exact: true })).toBeVisible();
  await expect(listHeaderDemo).toBeVisible();
  await expect(listHeaderDemo.locator('[data-component="list-header"]')).toHaveCount(2);
  await expect(listHeaderDemo.getByRole('button', { name: 'Add view' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'ListHeader planned demo' })).toHaveCount(0);
  await page.screenshot({ path: 'target/visual-captures/hs2-ygwny7-list-header-wide.png', fullPage: true });

  await page.setViewportSize({ width: 390, height: 844 });
  await expect(listHeaderDemo).toBeVisible();
  // Kerf 5.0.0-beta.56 collapses the catalog sidebar into a transient overlay on a small screen.
  await expect(page.locator('[data-component="catalog"]')).toHaveAttribute('data-sidebar-collapsed', 'true');
  await page.screenshot({ path: 'target/visual-captures/hs2-ygwny7-list-header-narrow.png', fullPage: true });
  await page.goto('/ux-demo?component=list-item&dev-review=false');
  await expect(page.getByRole('region', { name: 'ListItem demo' })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-ygwny7-list-item-narrow.png', fullPage: true });
});

test('styles demo captions through demo-owned classes, not the demoed components (HS2-TV78E1)', async ({ page }) => {
  const captions: [string, string, number][] = [
    ['ticket-list', '.collection-demo__caption', 3],
    ['ticket-board', '.collection-demo__caption', 2],
    ['toolbar-text', '.toolbar-text-demo__caption', 3],
    ['ticket-search-field', '.ticket-search-field-demo__caption', 7],
    ['toolbar-control-group', '.toolbar-control-group-demo__caption', 8],
    ['ticket-code-review', '.code-review-demo__caption', 4],
    ['list-item', '.list-item-demo__caption', 5],
    ['terminal-key-bar', '.terminal-key-bar-demo__caption', 2],
    ['fixed-aspect-terminal-card', '.fixed-aspect-terminal-card-demo__caption', 5],
  ];
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    for (const [component, selector, count] of captions) {
      await page.goto(`/ux-demo?component=${component}&dev-review=false`);
      const caption = page.locator(selector);
      await expect(caption).toHaveCount(count);
      await expect(caption.first()).toBeVisible();
      await expect(caption.first()).toHaveCSS('margin-top', '0px');
    }
    await page.goto('/ux-demo?component=list-item&dev-review=false');
    await expect(page.locator('.list-item-demo__caption').first()).toHaveCSS('text-transform', 'uppercase');
    await expect(page.locator('.list-item-demo__detail')).toHaveCount(3);
    await expect(page.locator('.list-item-demo__detail').first()).toHaveCSS('font-variant-numeric', 'tabular-nums');
    await page.goto('/ux-demo?component=ticket-list&dev-review=false');
    await expect(page.locator('.collection-demo__empty-state')).toHaveCount(3);
    await expect(page.locator('.collection-demo__empty-state').first()).toHaveCSS('border-top-style', 'solid');
    await page.goto('/ux-demo?component=ticket-code-review&dev-review=false');
    await expect(page.locator('.code-review-demo__case')).toHaveCount(4);
    await page.goto('/ux-demo?component=floating-toolbar&dev-review=false');
    await expect(page.locator('.floating-toolbar-demo__copy')).toHaveCSS('margin-top', '0px');
  }
});

test('represents the application states extracted from main.tsx in the UX catalog', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=project-dialog');
  await expect(page.locator('[data-project-dialog]')).toHaveJSProperty('open', false);
  await expect(page.locator('[data-remote-project-dialog]')).toContainText('/work/demo');
  await page.screenshot({ path: 'target/visual-captures/hs2-vbrc6a-project-dialogs.png', fullPage: true });

  await page.goto('/ux-demo?component=terminal-rename-dialog');
  const rename = page.locator('[data-terminal-rename-dialog]');
  const renameSurface = rename.locator('[part~="dialog"]');
  await expect(rename).toHaveJSProperty('open', true);
  await expect(rename.getByRole('textbox', { name: 'Terminal name' })).toHaveJSProperty('value', 'Development');
  await expect(renameSurface).toBeVisible();
  await renameSurface.screenshot({ path: 'target/visual-captures/hs2-737h3x-terminal-rename-wide.png' });
  // HS2-2Q7KTX: the renamed variant names its default and offers Reset to default; the
  // default-named variant does not.
  const renameField = rename.locator('wa-input[name="terminal-name"]'),
    resetRename = rename.getByRole('button', { name: 'Reset to default' });
  await expect(renameField).toHaveJSProperty('hint', 'Default name: Terminal 1');
  await resetRename.click();
  await expect(rename).toHaveJSProperty('open', false);
  await expect(page.locator('[data-rename-demo-output]')).toHaveText(
    'Reset to default → the tab shows Terminal 1 again',
  );
  await page.getByRole('button', { name: 'Rename a default-named terminal' }).click();
  await expect(rename).toHaveJSProperty('open', true);
  await expect(renameField).toHaveJSProperty('value', 'Terminal 1');
  await expect(renameField).toHaveJSProperty('hint', '');
  await expect(resetRename).toHaveCount(0);
  await rename.getByRole('button', { name: 'Cancel' }).click();
  await expect(rename).toHaveJSProperty('open', false);
  await page.getByRole('button', { name: 'Rename a renamed terminal' }).click();
  await expect(renameField).toHaveJSProperty('value', 'Development');
  await expect(resetRename).toBeVisible();
  await rename.getByRole('textbox', { name: 'Terminal name' }).fill('Release');
  await rename.getByRole('button', { name: 'Rename' }).click();
  await expect(page.locator('[data-rename-demo-output]')).toHaveText('Rename → the tab shows Release');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/ux-demo?component=terminal-rename-dialog');
  const narrowRename = page.locator('[data-terminal-rename-dialog]');
  await expect(narrowRename).toHaveJSProperty('open', true);
  const narrowRenameSurface = narrowRename.locator('[part~="dialog"]');
  await expect(narrowRenameSurface).toBeVisible();
  await narrowRenameSurface.screenshot({ path: 'target/visual-captures/hs2-737h3x-terminal-rename-narrow.png' });

  await page.goto('/ux-demo?component=app-empty-state');
  await expect(page.getByText('Open a Hot Sheet project', { exact: true })).toBeVisible();
  await expect(page.locator('[data-project-restore-state="true"]')).toContainText(
    'Restoring projects, tickets, and terminals',
  );
  await page.screenshot({ path: 'target/visual-captures/hs2-vbrc6a-app-empty-states.png', fullPage: true });
});

test('uses canonical spacing in local and remote project dialogs (HS2-4Y6SM9)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=project-dialog');
  const remote = page.locator('[data-remote-project-dialog]'),
    surface = remote.locator('.remote-project-dialog');
  await expect(surface).toBeVisible();
  await expect(surface.locator('> [data-component="list"]')).toHaveCount(1);
  await expect(remote.locator('[data-component="row"]')).toHaveCount(1);
  const spacing = await page.evaluate(() => {
    const style = (selector: string) => getComputedStyle(document.querySelector(selector)!);
    return {
      dialogGap: style('.project-dialog > [data-component="list"]').gap,
      pathGap: style('.project-dialog__path').gap,
      footerGap: style('.project-dialog footer [data-component="row"]').gap,
      listGap: style('.remote-project-dialog__list').gap,
      itemGap: style('.remote-project-dialog__copy').gap,
      itemPadding: style('.remote-project-dialog__list .kui-list-item').padding,
    };
  });
  expect(spacing).toEqual({
    dialogGap: '16px',
    pathGap: '8px',
    footerGap: '8px',
    listGap: '4px',
    itemGap: '4px',
    itemPadding: '8px',
  });
  await page.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-project-dialog-remote-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(surface).toBeVisible();
  const narrowBox = await surface.boundingBox();
  expect(narrowBox).not.toBeNull();
  expect(narrowBox!.x).toBeGreaterThanOrEqual(0);
  expect(narrowBox!.x + narrowBox!.width).toBeLessThanOrEqual(390);
  await page.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-project-dialog-remote-narrow.png' });
});

test('uses Kerf layout primitives across migrated settings and dialog surfaces (HS2-S3BXC0)', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 900 });

  await page.goto('/ux-demo?component=manual-model-dialog&dev-review=false');
  const manual = page.locator('[data-component="manual-model-dialog"]');
  await expect(manual).toHaveJSProperty('open', true);
  await expect(manual.locator('[data-component="list"]')).toHaveCount(1);
  await expect(manual.locator('[data-component="row"]')).toHaveCount(1);
  await expect(manual.locator('[data-component="spacer"]')).toHaveAttribute('data-flex', 'true');
  const manualSurface = manual.locator('[part~="dialog"]');
  await expect(manualSurface).toBeVisible();
  await manualSurface.screenshot({
    path: 'target/visual-captures/hs2-s3bxc0-manual-model-wide.png',
    animations: 'disabled',
  });

  await page.goto('/ux-demo?component=trash-settings&dev-review=false');
  const trash = page.locator('[data-component="trash-settings"]');
  await expect(trash.locator('[data-component="list"]')).toHaveCount(2);
  await expect(trash.locator('[data-component="row"]')).toHaveCount(1);

  await page.goto('/ux-demo?component=keyboard-settings&dev-review=false');
  const keyboard = page.locator('[data-component="keyboard-settings"]');
  // HS2-57MAAH moved the layout class onto an app-owned wrapper around the Kerf List root.
  await expect(keyboard.locator(':scope > .keyboard-settings > [data-component="list"]')).toHaveCount(1);
  await expect(keyboard.locator('li[data-shortcut-id] [data-component="row"]').first()).toBeVisible();

  await page.goto('/ux-demo?component=provider-setup-form&dev-review=false');
  const provider = page.locator('[data-component="provider-setup-form"]');
  const grid = provider.locator('[data-component="grid"]');
  await expect(grid).toHaveAttribute('data-min-column-width', 'true');
  const wideColumns = await grid.evaluate((element) => getComputedStyle(element).gridTemplateColumns.split(' ').length);
  expect(wideColumns).toBe(2);
  await provider.screenshot({ path: 'target/visual-captures/hs2-s3bxc0-provider-wide.png', animations: 'disabled' });

  await page.setViewportSize({ width: 390, height: 844 });
  await expect
    .poll(() => grid.evaluate((element) => getComputedStyle(element).gridTemplateColumns.split(' ').length))
    .toBe(1);
  const narrowBox = await provider.boundingBox();
  expect(narrowBox).not.toBeNull();
  expect(narrowBox!.x).toBeGreaterThanOrEqual(0);
  expect(narrowBox!.x + narrowBox!.width).toBeLessThanOrEqual(390);
  await provider.screenshot({ path: 'target/visual-captures/hs2-s3bxc0-provider-narrow.png', animations: 'disabled' });
});

test('renders keyboard shortcut rows edge-to-edge without a transparent left gutter (HS2-186WJT)', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 800 });
  await page.goto('/ux-demo?component=keyboard-settings');
  const list = page.locator('.keyboard-settings__list').first();
  const chord = page.locator('.keyboard-settings__chord').first();
  await expect(chord).toBeVisible();
  // Browser/Web Awesome list defaults add inline-start margin to list items. The app-owned row reset
  // must keep the white row surface flush with the list's inner border instead of exposing the page.
  const readGeometry = () =>
    list.evaluate((node) => {
      const listBox = node.getBoundingClientRect(),
        row = node.querySelector('.keyboard-settings__row');
      if (!row) throw new Error('missing keyboard shortcut row');
      const rowBox = row.getBoundingClientRect(),
        listStyle = getComputedStyle(node),
        rowStyle = getComputedStyle(row);
      return {
        inset: rowBox.left - listBox.left,
        marginLeft: rowStyle.marginLeft,
        widthGap: listBox.right - rowBox.right,
        listBackground: listStyle.backgroundColor,
        rowBackground: rowStyle.backgroundColor,
      };
    });
  const geometry = await readGeometry();
  expect(geometry.marginLeft).toBe('0px');
  expect(geometry.inset).toBe(1);
  expect(geometry.widthGap).toBe(1);
  expect(geometry.listBackground).toBe(geometry.rowBackground);
  // The chord itself also remains free of the native keycap shadow and uneven bottom border.
  const edges = await chord.evaluate((node) => {
    const style = getComputedStyle(node);
    return {
      shadow: style.boxShadow,
      widths: [style.borderTopWidth, style.borderRightWidth, style.borderBottomWidth, style.borderLeftWidth],
    };
  });
  expect(edges.shadow).toBe('none');
  expect(new Set(edges.widths).size).toBe(1);
  await list.screenshot({ path: 'target/visual-captures/hs2-186wjt-keyboard-list.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  const narrowGeometry = await readGeometry();
  expect(narrowGeometry).toEqual(geometry);
  await list.screenshot({ path: 'target/visual-captures/hs2-186wjt-keyboard-list-narrow.png' });
});

test('reopens dialog demos and keeps Feedback above the modal top layer', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=hs1-migration-dialog&dev-review=1');
  const migration = page.locator('[data-component="hs1-migration-dialog"]');
  await expect(migration).toHaveJSProperty('open', true);
  const feedback = page.locator('.hs-dev-review__feedback');
  await expect(feedback).toBeVisible();
  expect(
    await feedback.evaluate((node) => {
      const box = node.getBoundingClientRect(),
        top = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2);
      return top === node || Boolean(top?.closest('.hs-dev-review__feedback'));
    }),
  ).toBe(true);
  await migration.getByRole('button', { name: 'Not now' }).click();
  await expect(migration).toHaveJSProperty('open', false);
  await page.getByRole('button', { name: 'Open import dialog' }).click();
  await expect(migration).toHaveJSProperty('open', true);
  await page.mouse.move(1000, 700);
  await page.waitForTimeout(200);
  await page.screenshot({ path: 'target/visual-captures/hs2-9a6ssk-dialog-reopened-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(feedback).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-9a6ssk-dialog-feedback-narrow.png', fullPage: true });
});

test('uses StateBanner for the responsive HS1 migration and cleanup notices (HS2-750WSY)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=hs1-migration-banner&dev-review=false');
  const migration = page.locator('.hs1-migration-banner > [data-component="state-banner"]'),
    cleanup = page.locator('.hs1-cleanup-banner > [data-component="state-banner"]');
  for (const [banner, tone] of [
    [migration, 'info'],
    [cleanup, 'success'],
  ] as const) {
    await expect(banner).toBeVisible();
    await expect(banner).toHaveAttribute('data-component', 'state-banner');
    await expect(banner).toHaveAttribute('data-tone', tone);
    await expect(banner).toHaveAttribute('role', 'status');
    await expect(banner).toHaveAttribute('aria-live', 'polite');
    expect(
      await banner.evaluate((node) => {
        const style = getComputedStyle(node),
          copy = getComputedStyle(node.querySelector('.kui-state-banner__copy')!),
          button = getComputedStyle(node.querySelector('button')!);
        return {
          padding: style.padding,
          gap: style.gap,
          copyGap: copy.gap,
          buttonPadding: [button.paddingLeft, button.paddingRight],
        };
      }),
      // The action buttons keep Kerf's own StateBanner padding; the app no longer restyles them (HS2-9ME409).
    ).toEqual({ padding: '8px 16px', gap: '16px', copyGap: '4px', buttonPadding: ['11.2px', '11.2px'] });
  }
  await expect(cleanup.locator('.kui-state-banner__action > .hs1-cleanup-banner__actions')).toHaveCount(1);
  await page
    .locator('.dialog-layout-demo')
    .screenshot({ path: 'target/visual-captures/hs2-750wsy-hs1-state-banners-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page
    .locator('.dialog-layout-demo')
    .screenshot({ path: 'target/visual-captures/hs2-750wsy-hs1-state-banners-narrow.png' });
});

test('uses canonical spacing in the HS1 migration dialog (HS2-4Y6SM9)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=hs1-migration-dialog&dev-review=false');
  const dialog = page.locator('[data-component="hs1-migration-dialog"]'),
    form = dialog.locator('.hs1-migration-dialog');
  await expect(dialog).toHaveJSProperty('open', true);
  expect(
    await form.evaluate((node) => {
      const style = (selector: string) => getComputedStyle(node.querySelector(selector)!);
      return {
        dialogGap: getComputedStyle(node).gap,
        introGap: style('.hs1-migration-dialog__intro').gap,
        introCopyTop: style('.hs1-migration-dialog__intro p').marginTop,
        destinationGap: style('.hs1-migration-dialog__destination').gap,
        footerGap: style('footer').gap,
      };
    }),
  ).toEqual({ dialogGap: '24px', introGap: '16px', introCopyTop: '4px', destinationGap: '8px', footerGap: '8px' });
  await page.screenshot({ path: 'target/visual-captures/hs2-1yabz8-hs1-dialog-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(dialog.locator('.kui-value-table__row')).toHaveCount(4);
  await page.screenshot({ path: 'target/visual-captures/hs2-1yabz8-hs1-dialog-narrow.png' });
});

test('represents the shared repository-status composition in the UX catalog', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto('/ux-demo?component=repository-status-popover&dev-review=false');
  const dialog = page.locator('[data-component="repository-status-popover"]');
  await expect(dialog).toBeVisible();
  const compare = dialog.getByRole('button', { name: 'Compare two commits' }),
    refresh = dialog.getByRole('button', { name: 'Refresh repository status' }),
    headingActions = dialog.locator('.app-heading .kui-toolbar__trailing');
  expect(await refresh.evaluate((node) => getComputedStyle(node).color)).toBe(
    await compare.evaluate((node) => getComputedStyle(node).color),
  );
  await expect(dialog.locator('.kui-value-table__row')).toHaveCount(4);
  await expect(headingActions.locator(':scope > [data-component="toolbar-control-group"]')).toHaveCount(2);
  await expect(
    headingActions.locator(
      ':scope > [data-component="toolbar-control-group"] > [data-component="toolbar-control-group"]',
    ),
  ).toHaveCount(0);
  expect(
    await headingActions.evaluate((node) => {
      const groups = [...node.children].map((child) => child.getBoundingClientRect());
      return groups[1].left - groups[0].right;
    }),
  ).toBe(8);
  await expect(dialog).toHaveAttribute('data-embedded', 'true');
  // A list-detail SplitView (HS2-3B8345): each Pane's content owns its scrolling, and the detail
  // column carries the heading.
  await expect(dialog.locator('[data-component="split-view"]')).toHaveCount(1);
  await expect(dialog.locator('.repository-status-popover__split .kui-pane__content').first()).toHaveCSS(
    'overflow-y',
    'auto',
  );
  expect(
    await dialog.evaluate((node) => {
      const heading = node.querySelector('.app-heading')!.getBoundingClientRect(),
        navigation = node.querySelector('.repository-status-popover__navigation')!.getBoundingClientRect();
      return heading.left >= navigation.right;
    }),
  ).toBe(true);
  await expect(dialog.locator('[data-component="list-header"]')).toContainText('Views');
  await expect(dialog.locator('[data-component="list-item"]')).not.toHaveCount(0);
  const paneSpacing = await dialog.evaluate((node) => {
    const aside = node.querySelector<HTMLElement>('.repository-status-popover__navigation')!,
      detail = node.querySelector<HTMLElement>('.repository-status-popover__detail')!,
      values = [...node.querySelectorAll<HTMLElement>('.repository-status-popover__values')],
      nav = node.querySelector<HTMLElement>('nav')!,
      header = nav.querySelector<HTMLElement>('[data-component="list-header"]')!.getBoundingClientRect(),
      items = [
        ...nav.querySelectorAll<HTMLElement>(
          ':scope > .repository-status-popover__views > [data-component="list-item"]',
        ),
      ].map((item) => item.getBoundingClientRect());
    return {
      asidePadding: getComputedStyle(aside).padding,
      detailPadding: getComputedStyle(detail).padding,
      valueGap: values[1].getBoundingClientRect().top - values[0].getBoundingClientRect().bottom,
      headerGap: items[0].top - header.bottom,
      rowGaps: items.slice(1).map((item, index) => item.top - items[index].bottom),
    };
  });
  expect(paneSpacing).toEqual({
    asidePadding: '8px 16px 16px',
    detailPadding: '8px 16px 16px',
    valueGap: 16,
    headerGap: 4,
    rowGaps: [0, 0, 0, 0],
  });
  await expect(dialog.locator('.repository-status-popover__count')).toHaveCount(5);
  await page.locator('[data-action="toggle-settings"]').click();
  const inspector = page.getByRole('complementary', { name: 'RepositoryStatusPopover settings' }),
    scenario = inspector.locator('wa-select[name="scenario"]');
  for (const [value, state, copy, icon] of [
    ['clean', 'clean', 'Working tree is clean', 'circle-check'],
    ['dirty', 'dirty', 'Local changes have not been committed', 'git-branch'],
    ['ahead', 'ahead', 'Local commits have not been pushed', 'git-branch'],
    ['behind', 'behind', 'Remote commits have not been integrated', 'git-branch'],
    ['diverged', 'diverged', 'Local and remote histories have diverged', 'git-branch'],
    ['conflicted', 'conflicted', 'Repository has unresolved conflicts', 'triangle-alert'],
    ['error', 'error', 'Repository status is unavailable', 'triangle-alert'],
  ] as const) {
    await scenario.evaluate((node: HTMLElement & { value: string }, next) => {
      node.value = next;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);
    await expect(dialog).toHaveAttribute('data-state', state);
    await expect(dialog.locator('.app-heading__summary')).toHaveText(copy);
    const iconTile = dialog.locator('.app-heading [data-component="toolbar-control-group"]').first();
    await expect(iconTile.locator(`[data-lucide="${icon}"]`)).toHaveCount(1);
    await expect(iconTile).toHaveAttribute(
      'data-tile-tone',
      state === 'clean' ? 'success' : state === 'conflicted' || state === 'error' ? 'danger' : 'brand',
    );
  }
  await inspector.getByRole('button', { name: 'Reset' }).click();
  await expect(scenario).toHaveJSProperty('value', 'conflicted');
  await expect(dialog).toHaveAttribute('data-state', 'conflicted');
  await expect(dialog.locator('.app-heading [data-tile-tone="danger"] [data-lucide="triangle-alert"]')).toHaveCount(1);
  await page.screenshot({ path: 'target/visual-captures/hs2-s6f817-repository-scenario-settings.png', fullPage: true });
  await page.locator('[data-action="toggle-settings"]').click();
  await dialog.getByRole('button', { name: /Unstaged 2/ }).click();
  await dialog.screenshot({ path: 'target/visual-captures/hs2-72z7cb-repository-status-wide.png' });
  await dialog.getByRole('button', { name: /Staged 2/ }).click();
  await expect(dialog).toHaveAttribute('data-view', 'staged');
  const file = dialog.locator('[data-action="select-repository-file"]').first(),
    fileMenu = file.locator('[data-action="open-repository-file-menu-trigger"]');
  await fileMenu.click();
  await expect(dialog.getByRole('menuitem', { name: 'Show Diff' })).toBeVisible();
  await dialog.getByRole('menuitem', { name: 'Show Diff' }).click();
  await expect(page.locator('.component-stage__event')).toContainText('Would review');
  await file.dblclick();
  await expect(page.locator('.component-stage__event')).toContainText('Would open');
  await dialog.screenshot({ path: 'target/visual-captures/hs2-s6f817-repository-scenarios-wide.png' });
  await dialog.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-repository-status-wide.png' });
  await dialog.getByRole('button', { name: /Commits 24/ }).click();
  await dialog.getByRole('button', { name: 'Compare two commits' }).click();
  const rows = dialog.locator('.ticket-code-review__commit-summary');
  await rows.nth(1).click();
  await rows.nth(0).click();
  await expect(dialog.getByRole('button', { name: 'Open comparison in Glassbox' })).toBeEnabled();
  await dialog.screenshot({ path: 'target/visual-captures/hs2-qe4j38-repository-comparison.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  // Kerf 5.0.0-beta.56 keeps the catalog sidebar inline at 760px; hide it so the composition gets the
  // constrained stage width this check is about rather than the width left beside the sidebar.
  await page.getByRole('button', { name: 'Hide UX components catalog' }).click();
  await expect(dialog.locator('.repository-status-popover__navigation')).toHaveCSS('padding', '8px 16px 16px');
  await expect(dialog.locator('.repository-status-popover__detail')).toHaveCSS('padding', '8px 16px 16px');
  await expect(dialog.locator('[aria-label="Repository identity"] dd').first()).toHaveCSS('white-space', 'normal');
  expect(
    await dialog
      .locator('[aria-label="Repository identity"] dd')
      .first()
      .evaluate((node) => {
        const range = document.createRange();
        range.selectNodeContents(node);
        return range.getClientRects().length;
      }),
  ).toBe(1);
  await dialog.screenshot({ path: 'target/visual-captures/hs2-z0tsx4-repository-status-demo-narrow.png' });
  await dialog.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-repository-status-narrow.png' });
  await dialog.getByRole('button', { name: /Unstaged 2/ }).click();
  await dialog.screenshot({ path: 'target/visual-captures/hs2-72z7cb-repository-status-narrow.png' });
});

test('contains the embedded repository status dialog and header actions (HS2-MCHTAW)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=repository-status-popover&dev-review=false');
  const dialog = page.locator('[data-component="repository-status-popover"]');
  const compare = dialog.getByRole('button', { name: 'Compare two commits' });
  const refresh = dialog.getByRole('button', { name: 'Refresh repository status' });
  await expect(dialog).toBeVisible();
  await expect(compare).toBeVisible();
  await expect(refresh).toBeVisible();
  const measureContainment = () =>
    dialog.evaluate((node) => {
      const bounds = (element: Element) => {
        const rect = element.getBoundingClientRect();
        return { left: rect.left, right: rect.right };
      };
      return {
        viewport: innerWidth,
        dialog: bounds(node),
        compare: bounds(node.querySelector('[data-action="toggle-repository-comparison"]')!),
        refresh: bounds(node.querySelector('[data-action="refresh-repository-status"]')!),
      };
    });
  const assertContained = (containment: Awaited<ReturnType<typeof measureContainment>>) => {
    expect(containment.dialog.left).toBeGreaterThanOrEqual(0);
    expect(containment.dialog.right).toBeLessThanOrEqual(containment.viewport);
    for (const action of [containment.compare, containment.refresh]) {
      expect(action.left).toBeGreaterThanOrEqual(containment.dialog.left);
      expect(action.right).toBeLessThanOrEqual(containment.dialog.right);
    }
  };
  assertContained(await measureContainment());
  await dialog.screenshot({ path: 'target/visual-captures/hs2-mchtaw-repository-status-wide.png' });
  await page.addStyleTag({
    content:
      'body{min-width:0}.demo-shell{display:block}.kui-workbench__rail--left,.kui-workbench__main > .kui-pane > .kui-pane__header,.kui-workbench__main > .kui-pane > .kui-pane__footer,.settings-toggle{display:none}.kui-workbench__main{min-height:0;padding:12px}',
  });
  await page.setViewportSize({ width: 760, height: 640 });
  assertContained(await measureContainment());
  await dialog.screenshot({ path: 'target/visual-captures/hs2-mchtaw-repository-status-constrained.png' });
});

test('represents the ticket change-evidence master-detail dialog in the UX catalog', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=change-evidence-dialog');
  const dialog = page.locator('[data-component="change-evidence-dialog"]');
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveAttribute('data-embedded', 'true');
  await expect(dialog).toHaveAttribute('data-view', 'tests');
  await expect(dialog.getByRole('button', { name: /Tests 1/ })).toHaveAttribute('aria-current', 'page');
  const file = dialog.locator('[data-action="open-repository-file-menu-trigger"]');
  await expect(file).toHaveAttribute('data-item-id', 'clients/web/src/change-evidence.test.ts');
  await expect(file).toHaveAttribute('data-file-menu-source', 'ticket');
  await file.click();
  await expect(dialog.getByRole('menuitem', { name: 'Show Diff' })).toBeVisible();
  await dialog.getByRole('menuitem', { name: 'Show Diff' }).click();
  await expect(page.locator('.component-stage__event')).toContainText('Would review');
  await dialog.getByRole('button', { name: /Source 1/ }).click();
  await expect(dialog).toHaveAttribute('data-view', 'source');
  await dialog.screenshot({ path: 'target/visual-captures/hs2-s7x4sb-change-evidence-demo-wide.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await dialog.screenshot({ path: 'target/visual-captures/hs2-s7x4sb-change-evidence-demo-narrow.png' });
});

test('keeps repository file action menus visible at wide and narrow sizes', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=repository-status-popover');
  const repository = page.locator('[data-component="repository-status-popover"]');
  await repository.getByRole('button', { name: /Staged 2/ }).click();
  await repository.locator('[data-action="open-repository-file-menu-trigger"]').first().click();
  const repositoryMenu = repository.getByRole('menu');
  await expect(repositoryMenu.getByRole('menuitem').allTextContents()).resolves.toEqual([
    'Show Diff',
    'Open',
    'Show in Finder',
    'Copy Relative Path',
    'Copy Absolute Path',
  ]);
  expect(
    await repositoryMenu.evaluate((node) => {
      const menu = getComputedStyle(node),
        item = getComputedStyle(node.querySelector('button')!);
      return { padding: menu.padding, itemPadding: item.padding, gap: item.gap };
    }),
  ).toEqual({ padding: '4px', itemPadding: '0px 16px', gap: '8px' });
  await repository.screenshot({ path: 'target/visual-captures/hs2-3yzn0s-repository-file-menu-wide.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await repository.locator('[data-action="open-repository-file-menu-trigger"]').last().click();
  const narrowBounds = (await repositoryMenu.boundingBox())!;
  expect(narrowBounds.x + narrowBounds.width).toBeLessThanOrEqual(752);
  expect(narrowBounds.y + narrowBounds.height).toBeLessThanOrEqual(632);
  await repository.screenshot({ path: 'target/visual-captures/hs2-3yzn0s-repository-file-menu-narrow.png' });

  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=change-evidence-dialog');
  const evidence = page.locator('[data-component="change-evidence-dialog"]');
  await evidence.locator('[data-action="open-repository-file-menu-trigger"]').click();
  await expect(evidence.getByRole('menuitem', { name: 'Show Diff' })).toBeEnabled();
  await evidence.screenshot({ path: 'target/visual-captures/hs2-3yzn0s-change-evidence-file-menu-wide.png' });
});

test('represents every server-build details state with shared dialog geometry', async ({ page }) => {
  await page.setViewportSize({ width: 1728, height: 971 });
  await page.goto('/ux-demo?component=connection-details-dialog');
  const dialog = page.locator('[data-component="connection-details-dialog"]'),
    header = dialog.locator('[data-component="heading"]');
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveAttribute('data-embedded', 'true');
  await expect(header).toBeVisible();
  await expect(header.locator('[data-component="toolbar-control-group"]')).toHaveAttribute('data-tile-tone', 'brand');
  await expect(header).toHaveCSS('border-bottom-width', '0px');
  await expect(dialog.locator('[data-component="value-table"]')).toBeVisible();
  const rows = dialog.locator('.kui-value-table__row');
  await expect(rows).toHaveCount(6);
  expect(
    await rows.nth(1).evaluate((node) => {
      const before = getComputedStyle(node, '::before');
      return { border: before.borderTopWidth, left: before.left, right: before.right };
    }),
  ).toEqual({ border: '1px', left: '8px', right: '8px' });
  await expect(dialog.getByRole('button', { name: 'Close' })).toHaveCount(0);
  await page.locator('[data-action="toggle-settings"]').click();
  const scenario = page
    .getByRole('complementary', { name: 'ConnectionDetailsDialog settings' })
    .locator('wa-select[name="scenario"]');
  for (const [value, kind, copy] of [
    ['source-stale', 'compatible', 'cargo build -p hotsheet-server'],
    ['revision-mismatch', 'compatible', 'detached build is intentional'],
    ['server-too-old-safe', 'server_too_old', 'quiescence-safe restart'],
    ['server-too-old-manual', 'server_too_old', 'Automatic restart is unavailable'],
    ['client-too-old', 'client_too_old', 'Update this Hot Sheet client'],
    ['unknown', 'unknown', 'inspect the server log'],
  ] as const) {
    await scenario.evaluate((node: HTMLElement & { value: string }, next) => {
      node.value = next;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);
    await expect(dialog).toHaveAttribute('data-kind', kind);
    await expect(dialog).toContainText(copy);
  }
  await page
    .getByRole('complementary', { name: 'ConnectionDetailsDialog settings' })
    .getByRole('button', { name: 'Reset' })
    .click();
  await expect(scenario).toHaveJSProperty('value', 'source-stale');
  const settingsTrigger = page.locator('[data-action="toggle-settings"]');
  await settingsTrigger.click();
  await settingsTrigger.evaluate((node) => {
    (node as HTMLElement).hidden = true;
  });
  await dialog.screenshot({ path: 'target/visual-captures/hs2-1yabz8-connection-details-wide.png' });
  await header.screenshot({ path: 'target/visual-captures/hs2-rxxxph-connection-heading-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(header).toHaveCSS('border-bottom-width', '0px');
  await dialog.screenshot({ path: 'target/visual-captures/hs2-1yabz8-connection-details-narrow.png' });
  await header.screenshot({ path: 'target/visual-captures/hs2-rxxxph-connection-heading-phone.png' });
});

test('represents the production terminal dashboard and its shared context menu in the UX catalog', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=terminal-dashboard');
  const dashboard = page.getByRole('region', { name: 'Workspace grid' });
  await expect(dashboard).toBeVisible();
  await expect(dashboard).toHaveAttribute('data-basis', 'high');
  await expect(dashboard).toHaveAttribute('data-fit', '3');
  await expect(dashboard.locator('[data-fixed-aspect-terminal-card="preview"]')).toHaveCount(3);
  await expect(dashboard.locator('[data-ai-connection="connected"] [data-lucide="plug"]')).toBeVisible();
  await expect(dashboard.locator('[data-ai-connection="connected"]')).toHaveAttribute(
    'title',
    /Last trusted hook report: SessionStart at 2026-10-05T08:00:00Z.*MCP connectivity is separate/,
  );
  await expect(dashboard.locator('[data-ai-connection="missing"] [data-lucide="unplug"]')).toBeVisible();
  await expect(dashboard.locator('[data-terminal-key="demo:halted"] .terminal-tile__halt')).toBeVisible();
  await expect(dashboard.locator('[data-terminal-key="demo:halted"] [data-ai-connection]')).toHaveCount(0);
  const chat = dashboard.locator('[data-component="workspace-chat-tile"]');
  await expect(chat).toHaveCount(1);
  await expect(chat).toContainText('Codex AI chat');
  await expect(chat).toContainText('Reviewing the latest workspace changes');
  const menu = dashboard.getByRole('menu', { name: 'Terminal actions' });
  await expect(menu).toHaveCount(0);
  const first = dashboard.locator('[data-component="terminal-tile"]').first();
  await first.getByRole('button', { name: /More actions/ }).click();
  await expect(menu.getByRole('menuitem')).toHaveCount(2);
  await expect(menu.getByText('Open')).toBeVisible();
  await expect(menu.getByText('Hide Terminal')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
  await first.click({ button: 'right' });
  await expect(menu).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-hpy5r0-workspace-grid-ai-chat-wide.png', fullPage: true });
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 700, height: 700 });
  await expect(chat).toBeVisible();
  await expect.poll(() => dashboard.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
  await page.screenshot({
    path: 'target/visual-captures/hs2-hpy5r0-workspace-grid-ai-chat-narrow.png',
    fullPage: true,
  });
});

test('represents aggregate and per-project terminal operations in the UX catalog', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=terminal-operations-sidebar');
  const sidebar = page.getByRole('complementary', { name: 'Terminal operations sidebar' });
  await expect(sidebar).toBeVisible();
  await expect(sidebar.locator('[data-component="list-header"]')).toHaveCount(4);
  const summaries = sidebar.locator('[data-component="project-summary"]');
  await expect(summaries).toHaveCount(4);
  await expect(sidebar.getByText('All projects', { exact: true })).toBeVisible();
  await expect(sidebar.getByText('HotSheet2', { exact: true })).toBeVisible();
  await expect(sidebar.getByText('Best-in-Manila', { exact: true })).toBeVisible();
  await expect(sidebar.getByText('Kerf', { exact: true })).toBeVisible();
  const aggregate = sidebar.locator('[data-project-id="all"] [data-component="project-summary"]'),
    hotsheet = sidebar.locator('[data-project-id="hotsheet2"] [data-component="project-summary"]'),
    kerf = sidebar.locator('[data-project-id="kerf"] [data-component="project-summary"]');
  await expect(aggregate).toContainText('2 completed today');
  await expect(aggregate).toHaveAttribute('data-chart-tone', 'success');
  await expect(aggregate).toHaveAttribute('data-chart-background', 'false');
  await expect(hotsheet).toHaveAttribute('data-chart-tone', 'brand');
  await expect(hotsheet).toHaveAttribute('data-chart-background', 'true');
  await expect(kerf).toHaveAttribute('data-chart-background', 'true');
  for (const summary of [aggregate, hotsheet, kerf]) await expect(summary).toHaveAttribute('data-chart-maximum', '9');
  await expect(aggregate.locator('[data-bar="5"]')).toHaveAttribute('style', '--bar-height:100%');
  await expect(hotsheet.locator('[data-background-bar="5"]')).toHaveAttribute('style', '--bar-height:100%');
  await expect(hotsheet.locator('[data-bar="4"]')).toHaveAttribute('style', '--bar-height:56%');
  await expect(kerf.locator('[data-bar="5"]')).toHaveAttribute('style', '--bar-height:56%');
  for (const group of await sidebar.locator('.terminal-operations-sidebar__group').all()) {
    const heading = await group.getByRole('heading').evaluate((node) => {
      const range = document.createRange();
      range.selectNodeContents(node);
      return { x: range.getBoundingClientRect().x };
    });
    const chart = await group.locator('.project-summary__chart').boundingBox();
    expect(Math.abs(heading.x - chart!.x)).toBeLessThanOrEqual(1);
  }
  await sidebar.screenshot({ path: 'target/visual-captures/hs2-737h3x-terminal-operations-wide.png' });
  await page.setViewportSize({ width: 390, height: 1200 });
  await expect(sidebar).toBeVisible();
  await sidebar.screenshot({ path: 'target/visual-captures/hs2-737h3x-terminal-operations-narrow.png' });
});

test('represents the compact terminal ticket rail in the UX catalog', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=terminal-ticket-rail&dev-review=false');
  // The rail is a navigation panel (HS2-FY06N4): its root NavStack view's toolbar holds the project
  // selector before the standard toggle, above the pinned controls and the scrolling ticket list.
  const panel = page.locator('.terminal-ticket-rail-demo'),
    rail = page.locator('[data-component="terminal-ticket-rail"]'),
    project = panel.locator('wa-select[name="terminal-rail-project"]'),
    view = rail.locator('wa-select[name="terminal-rail-view"]'),
    launcher = rail.locator('[data-component="quick-ticket-composer-launcher"]');
  await expect(rail).toBeVisible();
  await expect(project).toHaveAttribute('value', 'demo');
  await expect(view).toHaveAttribute('value', 'all');
  await expect(launcher).toHaveClass(/quick-ticket-composer__launcher/);
  await expect(launcher).toContainText('Ticket…');
  await expect(launcher).not.toContainText('New ticket');
  await expect(panel.getByRole('button', { name: 'Hide ticket rail' })).toBeVisible();
  await expect(panel.getByRole('button', { name: 'Hide ticket rail' })).toHaveAttribute(
    'data-action',
    'toggle-ticket-inspector',
  );
  await expect(rail.locator('[data-component="ticket-list-row"]')).toHaveCount(7);
  await expect(rail.locator('[data-component="nav-stack"]')).toHaveCount(1);
  const geometry = await rail.evaluate((node) => {
    const modeElement = node.querySelector<HTMLElement>('.view-mode-switcher')!,
      mode = modeElement.getBoundingClientRect(),
      sort = node.querySelector('.workspace-header__sort-group')!.getBoundingClientRect(),
      search = node.querySelector('.ticket-search-field')!.getBoundingClientRect(),
      utility = node.querySelector('.workspace-header__utility-group')!.getBoundingClientRect(),
      project = document.querySelector('wa-select[name="terminal-rail-project"]')!.getBoundingClientRect(),
      projectChrome = getComputedStyle(
        document.querySelector<HTMLElement>('.terminal-ticket-rail-demo [data-nav-stack-chrome] .kui-toolbar')!,
      ),
      controls = getComputedStyle(node.querySelector<HTMLElement>('.terminal-ticket-rail__controls')!),
      heading = getComputedStyle(node.querySelector<HTMLElement>('.terminal-ticket-rail__heading .kui-toolbar')!),
      headingWrap = getComputedStyle(node.querySelector<HTMLElement>('.terminal-ticket-rail__heading')!),
      content = getComputedStyle(node.querySelector<HTMLElement>('.kui-nav-stack__view[data-nav-active="true"]')!),
      // The launcher is a brand Web Awesome pill (HS2-PNCDAE): its fill and radius live on the base part.
      launcherStyle = getComputedStyle(
        node
          .querySelector<HTMLElement>('.quick-ticket-composer__launcher')!
          .shadowRoot!.querySelector('[part~="base"]')!,
      );
    return {
      mode: {
        top: mode.top,
        bottom: mode.bottom,
        width: mode.width,
        radius: getComputedStyle(modeElement).borderRadius,
      },
      sortTop: sort.top,
      searchTop: search.top,
      searchLeft: search.left,
      utilityTop: utility.top,
      utilityRight: utility.right,
      projectWidth: project.width,
      railWidth: node.getBoundingClientRect().width,
      projectPadding: projectChrome.padding,
      controlsPadding: controls.padding,
      headingPadding: heading.padding,
      headingBorderBottom: headingWrap.borderBottomWidth,
      contentPadding: content.padding,
      launcherBackground: launcherStyle.backgroundColor,
      launcherRadius: launcherStyle.borderRadius,
    };
  });
  expect(geometry.mode.bottom).toBeLessThanOrEqual(geometry.sortTop);
  expect(geometry.searchTop).toBeCloseTo(geometry.sortTop, 0);
  expect(geometry.utilityTop).toBeCloseTo(geometry.sortTop, 0);
  expect(geometry.searchLeft).toBeGreaterThanOrEqual(geometry.utilityRight);
  expect(geometry.mode.radius).not.toBe('9999px');
  expect(geometry.mode.width).toBeGreaterThan(geometry.railWidth * 0.8);
  expect(geometry.projectWidth).toBeLessThan(geometry.railWidth * 0.8);
  expect({
    project: geometry.projectPadding,
    controls: geometry.controlsPadding,
    heading: geometry.headingPadding,
    content: geometry.contentPadding,
  }).toEqual({ project: '8px', controls: '8px', heading: '8px', content: geometry.contentPadding });
  // The list sits on the active view's Pane-owned sunken scroll surface.
  await expect(rail.locator('.kui-nav-stack__view[data-nav-active="true"] .kui-pane')).toHaveAttribute(
    'data-appearance',
    'sunken',
  );
  expect(geometry.headingBorderBottom).toBe('1px');
  expect(geometry.launcherBackground).not.toBe('rgba(0, 0, 0, 0)');
  expect(Number.parseFloat(geometry.launcherRadius)).toBeGreaterThan(geometry.railWidth / 4);
  const scroller = rail.locator('.kui-nav-stack__view[data-nav-active="true"] .kui-pane__content'),
    rows = rail.locator('[data-component="ticket-list-row"]'),
    lastRow = rows.last();
  await expect.poll(() => scroller.evaluate((node) => node.scrollHeight > node.clientHeight)).toBe(true);
  await scroller.hover();
  await page.mouse.wheel(0, 1_000);
  await expect.poll(() => scroller.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
  await expect(lastRow).toBeInViewport();
  await page.screenshot({ path: 'target/visual-captures/hs2-8j0378-terminal-ticket-rail-scrolled.png' });
  await scroller.evaluate((node) => {
    node.scrollTop = 0;
  });
  await rail.screenshot({ path: 'target/visual-captures/hs2-r292m4-terminal-ticket-rail-wide.png' });
  await project.click();
  const longProject = project.locator('wa-option[value="docs"]');
  await expect(longProject).toBeVisible();
  const menuGeometry = await project.evaluate((node) => {
    const listbox = node.shadowRoot!.querySelector<HTMLElement>('[part="listbox"]')!,
      listboxBox = listbox.getBoundingClientRect(),
      option = node.querySelector<HTMLElement>('wa-option[value="docs"]')!,
      label = option.shadowRoot!.querySelector<HTMLElement>('[part="label"]')!;
    return {
      triggerWidth: node.getBoundingClientRect().width,
      listboxWidth: listboxBox.width,
      listboxLeft: listboxBox.left,
      listboxRight: listboxBox.right,
      viewportWidth: innerWidth,
      labelClientWidth: label.clientWidth,
      labelScrollWidth: label.scrollWidth,
      labelClientHeight: label.clientHeight,
      labelScrollHeight: label.scrollHeight,
    };
  });
  expect(menuGeometry.listboxWidth).toBeGreaterThan(menuGeometry.triggerWidth);
  expect(menuGeometry.listboxLeft).toBeGreaterThanOrEqual(0);
  expect(menuGeometry.listboxRight).toBeLessThanOrEqual(menuGeometry.viewportWidth);
  expect(menuGeometry.labelScrollWidth - menuGeometry.labelClientWidth).toBeLessThanOrEqual(1);
  expect(menuGeometry.labelScrollHeight - menuGeometry.labelClientHeight).toBeLessThanOrEqual(1);
  await page.screenshot({
    path: 'target/visual-captures/hs2-fpftyy-terminal-ticket-rail-project-menu-wide.png',
    fullPage: true,
  });
  // HS2-7N6F67: the compact view selector must not clip its longer option labels either.
  await page.keyboard.press('Escape');
  await view.click();
  const longView = view.locator('wa-option[value="backlog"]');
  await expect(longView).toBeVisible();
  const viewMenuGeometry = await view.evaluate((node) => {
    const listbox = node.shadowRoot!.querySelector<HTMLElement>('[part="listbox"]')!,
      listboxBox = listbox.getBoundingClientRect(),
      option = node.querySelector<HTMLElement>('wa-option[value="backlog"]')!,
      label = option.shadowRoot!.querySelector<HTMLElement>('[part="label"]')!;
    return {
      triggerWidth: node.getBoundingClientRect().width,
      listboxWidth: listboxBox.width,
      listboxLeft: listboxBox.left,
      listboxRight: listboxBox.right,
      viewportWidth: innerWidth,
      labelClientWidth: label.clientWidth,
      labelScrollWidth: label.scrollWidth,
    };
  });
  expect(viewMenuGeometry.listboxWidth).toBeGreaterThanOrEqual(viewMenuGeometry.triggerWidth);
  expect(viewMenuGeometry.listboxLeft).toBeGreaterThanOrEqual(0);
  expect(viewMenuGeometry.listboxRight).toBeLessThanOrEqual(viewMenuGeometry.viewportWidth);
  expect(viewMenuGeometry.labelScrollWidth - viewMenuGeometry.labelClientWidth).toBeLessThanOrEqual(1);
  await page.screenshot({
    path: 'target/visual-captures/hs2-7n6f67-terminal-ticket-rail-view-menu-wide.png',
    fullPage: true,
  });
  await page.keyboard.press('Escape');
  // HS2-HEYASQ: the rail content clips overflow on several ancestors, so the view select's focus ring
  // must be inset (negative outline-offset) to stay fully visible rather than cropped at a container
  // edge. The project select sits in the panel toolbar's padded control band instead (HS2-QQW6CT).
  const focusRingOffsets = await rail.evaluate((node) =>
    ['terminal-rail-view'].map((name) => {
      const combobox = node
        .querySelector<HTMLElement>(`wa-select[name="${name}"]`)!
        .shadowRoot!.querySelector<HTMLElement>('[part="combobox"]')!;
      return getComputedStyle(combobox).outlineOffset;
    }),
  );
  for (const offset of focusRingOffsets) {
    expect(Number.parseFloat(offset)).toBeLessThan(0);
  }
  await page.setViewportSize({ width: 760, height: 640 });
  await expect.poll(() => rail.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
  await expect(launcher).toBeVisible();
  const narrowLauncher = await rail.evaluate((node) => {
    const rail = node.getBoundingClientRect(),
      button = node.querySelector<HTMLElement>('.quick-ticket-composer__launcher')!.getBoundingClientRect();
    return { railRight: rail.right, buttonRight: button.right };
  });
  expect(narrowLauncher.buttonRight).toBeLessThanOrEqual(narrowLauncher.railRight);
  await rail.screenshot({ path: 'target/visual-captures/hs2-r292m4-terminal-ticket-rail-narrow.png' });
});

for (const component of ['workspace-header', 'terminal-ticket-rail']) {
  test(`projects selected Up Next state through the ${component} demo (HS2-WP15AF)`, async ({ page }) => {
    await page.setViewportSize({ width: 1728, height: 971 });
    await page.goto(`/ux-demo?component=${component}&dev-review=false`);
    const demo =
      component === 'workspace-header'
        ? page.getByRole('region', { name: 'WorkspaceHeader demo' })
        : page.locator('[data-component="terminal-ticket-rail"]');
    const star = demo.getByRole('button', { name: 'Toggle Up Next for selected tickets' });
    // A plain click in the rail also pushes the ticket's detail (HS2-FY06N4); return to the list.
    const plainClick = async (slug: string) => {
      await demo.locator(`[data-component="ticket-list-row"][data-ticket-slug="${slug}"]`).click();
      if (component === 'terminal-ticket-rail') {
        await demo.getByRole('button', { name: 'Back to ticket list' }).click();
        await expect(demo.locator('.kui-nav-stack__view[data-nav-key="root"]')).toHaveAttribute(
          'data-nav-active',
          'true',
        );
      }
    };
    await expect(star).toBeDisabled();
    await plainClick('HS2-R76MMW');
    await expect(star).toHaveAttribute('aria-pressed', 'true');
    await demo
      .locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-JN3X4W"]')
      .click({ modifiers: ['Meta'] });
    await expect(star).toHaveAttribute('aria-pressed', 'mixed');
    await star.click();
    await expect(star).toHaveAttribute('aria-pressed', 'true');
    if (component === 'workspace-header')
      await demo.screenshot({
        path: 'target/visual-captures/hs2-06gdw3-workspace-selected.png',
        animations: 'disabled',
      });
    await star.click();
    await expect(star).toHaveAttribute('aria-pressed', 'false');
    await demo.getByRole('button', { name: 'More actions for selected tickets' }).click();
    await expect(page.getByRole('menu', { name: 'Ticket actions' })).toBeVisible();
    await page.keyboard.press('Escape');
    await plainClick('HS2-K00QPZ');
    await expect(star).toBeDisabled();
    await plainClick('HS2-R76MMW');
    await star.click();
    await expect(star).toHaveAttribute('aria-pressed', 'true');
    await demo
      .locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-R76MMW"]')
      .click({ modifiers: ['Meta'] });
    await expect(star).toBeDisabled();
    await expect(star).toHaveAttribute('aria-pressed', 'false');
  });
}

test('keeps the ticket rail search bordered across focus, blur, collapse, and refill (HS2-TNSD4K)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=terminal-ticket-rail&dev-review=false');
  const rail = page.locator('[data-component="terminal-ticket-rail"]');
  const group = rail.locator('.ticket-search-field');
  const search = rail.getByRole('searchbox', { name: 'Search tickets' });
  const rows = rail.locator('[data-component="ticket-list-row"]');
  const blurTarget = rail.getByRole('button', { name: 'Hide ticket rail' });
  await expect(rows).toHaveCount(7);
  // Exercise the rail's Columns projection before the search and notification transitions
  // so the control is verified as a working mode, not merely present (HS2-YJJ1MJ).
  const columns = rail.getByRole('button', { name: 'Columns view', exact: true });
  await columns.click();
  await expect(columns).toHaveAttribute('aria-pressed', 'true');
  await expect(rail.locator('[data-component="ticket-board"]')).toBeVisible();
  await rail.getByRole('button', { name: 'List view', exact: true }).click();
  await expect(rows).toHaveCount(7);
  await rail.getByRole('button', { name: 'Notifications view', exact: true }).click();
  await expect(rail.getByRole('button', { name: 'Notifications view', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(rail.getByRole('region', { name: 'Notifications', exact: true })).toBeVisible();
  await expect(rail.getByRole('button', { name: 'Search tickets' })).toBeDisabled();
  await expect(rows).toHaveCount(0);
  await rail.getByRole('button', { name: 'List view', exact: true }).click();
  await expect(rail.getByRole('button', { name: 'List view', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await expect(rail.getByRole('region', { name: 'Notifications', exact: true })).toHaveCount(0);
  await expect(rows).toHaveCount(7);
  await expect(group).toHaveCSS('border-width', '1px');
  await rail.getByRole('button', { name: 'Search tickets' }).click();
  await expect(search).toBeFocused();
  await expect(group).toHaveCSS('border-width', '1px');
  await expect(group).not.toHaveCSS('box-shadow', 'none');
  await expect(group.locator('.kui-token-search')).toHaveCSS('border-width', '0px');
  await search.fill('long-tag-example');
  await expect(rows).toHaveCount(1);
  await blurTarget.focus();
  await expect(search).toBeVisible();
  await expect(group).toHaveCSS('border-width', '1px');
  await expect(group).toHaveCSS('box-shadow', 'none');
  await search.focus();
  await expect(group).not.toHaveCSS('box-shadow', 'none');
  await rail.getByRole('button', { name: 'Clear search' }).click();
  await expect(search).toHaveText('');
  await expect(rows).toHaveCount(7);
  await blurTarget.focus();
  await expect(search).toHaveCount(0);
  await expect(group).toHaveCSS('border-width', '1px');
  await rail.getByRole('button', { name: 'Search tickets' }).click();
  await expect(search).toBeFocused();
  await expect(group).toHaveCSS('border-width', '1px');
  await expect(group).not.toHaveCSS('box-shadow', 'none');
  await rail.screenshot({
    path: 'target/visual-captures/hs2-tnsd4k-rail-search-demo-wide.png',
    animations: 'disabled',
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await search.fill('A long query that wraps onto a second line inside the narrow ticket rail');
  await expect.poll(() => rail.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
  await expect(group).toHaveCSS('border-width', '1px');
  await expect(group).not.toHaveCSS('box-shadow', 'none');
  await rail.screenshot({
    path: 'target/visual-captures/hs2-tnsd4k-rail-search-demo-narrow.png',
    animations: 'disabled',
  });
});

test('keeps rail view tabs above search and parks the closed trigger at the right (HS2-JJ6ZE1)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=terminal-ticket-rail&dev-review=false');
  const stage = page.locator('.terminal-ticket-rail-demo');
  const toolbar = stage.locator('.terminal-ticket-rail__controls');
  const closedGroupMargins = () =>
    toolbar.evaluate((node) => {
      const toolbarBox = node.getBoundingClientRect();
      const viewBox = node.querySelector('.view-mode-switcher')!.getBoundingClientRect();
      const sortBox = node.querySelector('.workspace-header__sort-group')!.getBoundingClientRect();
      return [viewBox.left - toolbarBox.left, sortBox.left - toolbarBox.left, toolbarBox.right - viewBox.right];
    });
  const closedSearchRightInset = () =>
    toolbar.evaluate(
      (node) =>
        node.getBoundingClientRect().right -
        node.querySelector<HTMLButtonElement>('[aria-label="Search tickets"]')!.getBoundingClientRect().right,
    );
  const searchBelowView = () =>
    toolbar.evaluate(
      (node) =>
        node.querySelector('.view-mode-switcher')!.getBoundingClientRect().bottom <
        node.querySelector('.workspace-header__search-actions')!.getBoundingClientRect().top,
    );
  await stage.screenshot({ path: 'target/visual-captures/hs2-jj6ze1-after-closed-narrow.png', animations: 'disabled' });
  await expect.poll(async () => Math.max(...(await closedGroupMargins()))).toBeLessThan(40);
  await expect.poll(closedSearchRightInset).toBeLessThan(40);
  const wideRailStyle = await page.addStyleTag({ content: '.terminal-ticket-rail-demo { width: 700px !important; }' });
  await expect.poll(() => toolbar.evaluate((node) => node.getBoundingClientRect().width)).toBeGreaterThan(650);
  await expect.poll(async () => Math.max(...(await closedGroupMargins()))).toBeLessThan(40);
  await expect.poll(closedSearchRightInset).toBeLessThan(40);
  await stage.screenshot({ path: 'target/visual-captures/hs2-jj6ze1-after-closed-wide.png', animations: 'disabled' });
  await toolbar.getByRole('button', { name: 'Search tickets' }).click();
  await expect(toolbar.locator('.view-mode-switcher')).toBeVisible();
  await expect(toolbar.locator('.workspace-header__sort-group')).toBeVisible();
  await expect(toolbar.locator('.workspace-header__utility-group')).toBeVisible();
  await expect(toolbar.getByRole('button', { name: 'More workspace controls' })).toBeHidden();
  await expect
    .poll(() => toolbar.locator('.ticket-search-field').evaluate((node) => node.getBoundingClientRect().width))
    .toBeGreaterThan(240);
  await expect.poll(searchBelowView).toBe(true);
  await expect.poll(() => toolbar.evaluate((node) => node.getBoundingClientRect().height)).toBeGreaterThan(80);
  await stage.screenshot({ path: 'target/visual-captures/hs2-jj6ze1-after-open-wide.png', animations: 'disabled' });
  await wideRailStyle.evaluate((node) => {
    (node as HTMLElement).remove();
  });
  await expect(toolbar.locator('.view-mode-switcher')).toBeVisible();
  await expect(toolbar.locator('.workspace-header__utility-group')).toBeHidden();
  await expect(toolbar.getByRole('button', { name: 'More workspace controls' })).toBeVisible();
  await expect.poll(searchBelowView).toBe(true);
  await expect.poll(() => toolbar.evaluate((node) => node.getBoundingClientRect().height)).toBeGreaterThan(80);
  await stage.screenshot({ path: 'target/visual-captures/hs2-jj6ze1-after-open-narrow.png', animations: 'disabled' });
  await expect
    .poll(() =>
      toolbar.evaluate((node) => {
        const more = node.querySelector<HTMLElement>('.workspace-header__overflow-group')!;
        const box = more.getBoundingClientRect();
        const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
        return hit === more || Boolean(hit?.closest('.workspace-header__overflow-group'));
      }),
    )
    .toBe(true);
  await toolbar.getByRole('button', { name: 'More workspace controls' }).click();
  await expect(stage.locator('[data-workspace-overflow-action="toggle-selected-up-next"]')).toBeVisible();
  await expect(stage.locator('[data-workspace-overflow-action="set-workspace-sort"]')).toHaveCount(4);
  await expect(stage.locator('[data-workspace-overflow-action="set-view-mode"]')).toHaveCount(3);
});

test('catalogs every FixedAspectTerminalCard variant and its dashboard relationship', async ({ page }) => {
  await page.setViewportSize({ width: 1728, height: 971 });
  await page.goto('/ux-demo?component=fixed-aspect-terminal-card');
  const stage = page.getByRole('region', { name: 'Fixed aspect terminal card variants' }),
    preview = stage.locator('[data-fixed-aspect-terminal-card="preview"]'),
    magnified = stage.locator('[data-fixed-aspect-terminal-card="magnified"]:not([data-mobile-chrome])').first(),
    phone = stage.locator('[data-mobile-chrome="true"]'),
    previewViewport = preview.locator('[data-display-mode="scaled-preview"]'),
    magnifiedViewport = magnified.locator('[data-display-mode="interactive"]');
  await expect(preview).toBeVisible();
  await expect(magnified).toBeVisible();
  for (const viewport of [previewViewport, magnifiedViewport]) {
    await expect(viewport).toHaveAttribute('data-connection', 'connected');
    await expect(viewport).toHaveAttribute('data-renderer', 'dom');
  }
  await expect(previewViewport).toHaveAttribute('data-grid-size', '80x24');
  await expect(magnifiedViewport).not.toHaveAttribute('data-grid-size', '80x24');
  const rows = previewViewport.locator('.xterm-rows > div');
  await expect(rows).toHaveCount(24);
  await expect(rows.first()).toContainText('GNU nano 8.4');
  await expect(rows.nth(1)).toContainText('File: src/main.tsx');
  await expect(rows.nth(20)).toContainText('export { app };');
  await expect(rows.last()).toContainText('^X Exit');
  for (const bar of [rows.first(), rows.nth(1), rows.nth(21), rows.nth(22), rows.last()])
    expect(await bar.evaluate((node) => node.textContent.length)).toBe(80);
  await expect(magnifiedViewport.locator('.xterm-rows')).toContainText('GNU nano 8.4');
  await expect(preview).toHaveCSS('border-width', '0px');
  await expect(magnified).toHaveCSS('border-width', '0px');
  // The stage styles only its own grid shell; every card sizes itself through `fit="aspect"` with no
  // TerminalDashboard ancestor supplying its tokens (HS2-0X36TX).
  await expect(stage).not.toHaveClass(/terminal-dashboard/);
  await expect(stage.locator('[data-fixed-aspect-terminal-card]')).toHaveCount(5);
  for (const card of await stage.locator('[data-fixed-aspect-terminal-card]').all())
    await expect(card).toHaveAttribute('data-fit', 'aspect');
  const frameAspect = await preview.evaluate((card) => {
    const frame = card.querySelector('[data-display-mode="scaled-preview"]')!.parentElement!.getBoundingClientRect();
    return frame.width / frame.height;
  });
  expect(frameAspect).toBeCloseTo(5 / 3, 2);
  const sizing = await stage.evaluate((element) => {
    const preview = element
        .querySelector<HTMLElement>('[data-fixed-aspect-terminal-card="preview"]')!
        .getBoundingClientRect(),
      magnified = element
        .querySelector<HTMLElement>('[data-fixed-aspect-terminal-card="magnified"]:not([data-mobile-chrome])')!
        .getBoundingClientRect(),
      row = element.querySelector<HTMLElement>('[data-fixed-aspect-terminal-card="preview"] .xterm-rows > div')!;
    return { previewWidth: preview.width, magnifiedWidth: magnified.width, font: getComputedStyle(row).fontFamily };
  });
  expect(sizing.previewWidth).toBeLessThanOrEqual(352);
  expect(sizing.magnifiedWidth).toBeGreaterThan(sizing.previewWidth);
  expect(sizing.font).toContain('ui-monospace');
  // HS2-WMN626 phone variant: toolbar with Close and text size on top; hidden while the keyboard is up.
  await expect(phone).toHaveCount(2);
  const phoneToolbar = phone.first().locator('.terminal-tile__footer');
  await expect(phoneToolbar.getByRole('button', { name: 'Close Development' })).toBeVisible();
  await expect(phoneToolbar.getByRole('button', { name: 'Text size: 60 columns. Change text size' })).toBeVisible();
  await expect(phoneToolbar.locator('svg[data-lucide="a-large-small"]')).toBeVisible();
  await expect(phoneToolbar.locator('svg[data-lucide="x"]')).toBeVisible();
  expect(
    await phone
      .first()
      .evaluate(
        (card) =>
          card.querySelector('.terminal-tile__footer')!.getBoundingClientRect().bottom <=
          card.querySelector('.terminal-tile__preview')!.getBoundingClientRect().top + 1,
      ),
  ).toBe(true);
  await expect(phone.first()).toHaveCSS('border-top-left-radius', '0px');
  await expect(phone.nth(1)).toHaveAttribute('data-keyboard-visible', 'true');
  await expect(phone.nth(1).locator('.terminal-tile__footer')).toBeHidden();
  await stage.screenshot({ path: test.info().outputPath('hs2-wmn626-demo-variants.png') });
  // Kerf 5.0.0-beta.51 renders catalog relationships as a Components dropdown (HS2-KMDJRH).
  const relationships = page.locator('[data-catalog-related]');
  await relationships.getByRole('button', { name: /Component/ }).click();
  await expect(relationships.getByText('Used by', { exact: true })).toBeVisible();
  await expect(relationships.getByText('TerminalDashboard', { exact: true })).toBeVisible();
  await expect(relationships).toHaveJSProperty('open', true);
  await page.waitForTimeout(1_200);
  await expect(relationships).toHaveJSProperty('open', true);
  await page.screenshot({ path: 'target/visual-captures/hs2-s59crp-related-popup-open.png', fullPage: true });
  await page.keyboard.press('Escape');
  await page.screenshot({ path: 'target/visual-captures/hs2-g4g95r-nano-fills-terminal.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(preview).toBeVisible();
  await expect(magnified).toBeVisible();
  await expect.poll(() => stage.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
  await page.screenshot({ path: 'target/visual-captures/hs2-g4g95r-nano-fills-terminal-narrow.png', fullPage: true });
});

test('represents interactive terminal visibility groups in the UX catalog', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=terminal-visibility-dialog');
  const show = page.getByRole('button', { name: 'Manage Workspace Visibility' }),
    dialog = page.locator('[data-terminal-visibility-dialog]');
  await expect(show).toBeVisible();
  await expect(dialog).toHaveJSProperty('open', false);
  await show.click();
  await expect(dialog).toHaveJSProperty('open', true);
  // One Kerf ListItem per fixture item (Codex chat, AI, Development server); the dialog no longer
  // composes a ValueTable whose header row made the old count 4.
  await expect(dialog.locator('.terminal-visibility-dialog__rows .kui-list-item')).toHaveCount(3);
  await expect(dialog.locator('.terminal-visibility-dialog__toolbar')).toHaveCSS(
    'background-color',
    'rgba(0, 0, 0, 0)',
  );
  await expect(dialog.getByRole('tab', { name: 'Focus' })).toHaveAttribute('aria-selected', 'true');
  // The Add icon is sized by its LucideIcon `size` prop, not by CSS reaching into the AppTabs (HS2-YNW0B3).
  const addIcon = dialog.getByRole('button', { name: 'Add visibility group' }).locator('svg');
  await expect(addIcon).toHaveCSS('width', '16px');
  await expect(addIcon).toHaveCSS('height', '16px');
  const types = dialog.locator('wa-select[name="terminal-visibility-types"]');
  await expect(types).toHaveJSProperty('value', ['shell', 'ai', 'chat']);
  await types.click();
  await types.locator('button[data-select-action="clear"]').click();
  await expect(types).toHaveJSProperty('open', true);
  await expect(types).toHaveJSProperty('value', []);
  await types.locator('wa-option[value="chat"]').click();
  await expect(dialog.locator('[data-action="toggle-terminal-visibility"]')).toHaveCount(1);
  await page.keyboard.press('Escape');
  await expect(types).toHaveJSProperty('open', false);
  await expect(dialog).toHaveJSProperty('open', true);
  await dialog.getByRole('button', { name: 'Add visibility group' }).click();
  const nameDialog = page.locator('[data-terminal-visibility-name-dialog]'),
    name = nameDialog.getByRole('textbox', { name: 'Group name' });
  await expect(nameDialog).toHaveJSProperty('open', true);
  await expect(name).toBeFocused();
  await name.fill('Review');
  await nameDialog.getByRole('button', { name: 'Add', exact: true }).click();
  await expect(nameDialog).toHaveJSProperty('open', false);
  const review = dialog.getByRole('tab', { name: 'Review' });
  await expect(review).toHaveAttribute('aria-selected', 'true');
  await expect(types).toHaveJSProperty('value', ['chat']);
  await expect(dialog.locator('[data-action="toggle-terminal-visibility"]')).toHaveCount(1);
  await types.locator('[part~="expand-icon"]').click();
  await expect(types).toHaveJSProperty('open', true);
  await types.locator('button[data-select-action="all"]').click();
  await expect
    .poll(() => types.evaluate((element: HTMLElement & { value?: string[] }) => [...(element.value ?? [])].sort()))
    .toEqual(['ai', 'chat', 'shell']);
  await page.keyboard.press('Escape');
  await dialog.getByRole('button', { name: /Hide Development/ }).click();
  await expect(dialog.getByRole('button', { name: /Show Development/ })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-z0m2vv-visibility-demo-open-wide.png', fullPage: true });
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveJSProperty('open', false);
  await expect(show).toBeVisible();
  await show.click();
  await expect(dialog).toHaveJSProperty('open', true);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(dialog.locator('.terminal-visibility-dialog__toolbar')).toHaveCSS(
    'background-color',
    'rgba(0, 0, 0, 0)',
  );
  await page.screenshot({ path: 'target/visual-captures/hs2-z0m2vv-visibility-demo-open-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 900 });
  await review.click({ button: 'right' });
  const menu = dialog.getByRole('menu', { name: 'Visibility group actions' });
  await expect(menu.getByText('Rename…')).toBeVisible();
  await expect(menu.getByText('Delete')).toBeVisible();
});

test('captures, reviews, cancels, and submits dev-review feedback', async ({ page }) => {
  let submitted: Record<string, unknown> | undefined;
  await page.route('**/__hotsheet/dev-review/tickets', async (route) => {
    submitted = route.request().postDataJSON();
    await route.fulfill({ status: 201, contentType: 'application/json', body: JSON.stringify({ slug: 'HS2-REVIEW' }) });
  });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-row&dev-review=1');
  const captureTarget = page.locator('.kui-workbench__rail--left [data-item-id="ticket-row"]');
  await captureTarget.scrollIntoViewIfNeeded();
  await captureTarget.evaluate((node) => {
    (node as HTMLElement).style.color = 'oklab(55% 0.1 0.1)';
  });
  await page.locator('.kui-workbench__main > .kui-pane > .kui-pane__header').evaluate((node) => {
    (node as HTMLElement).style.boxShadow = '0 0 2px oklab(55% 0.1 0.1)';
  });
  expect(await captureTarget.evaluate((node) => getComputedStyle(node).color)).toMatch(/^(?:oklab|rgb)/);
  const markerBox = (await captureTarget.boundingBox())!;
  const tool = page.locator('.hs-dev-review');
  await expect(tool.getByRole('button', { name: 'Feedback' })).toBeVisible();
  await tool.getByRole('button', { name: 'Feedback' }).click();
  await expect(tool.getByRole('button', { name: 'New Ticket' })).toBeVisible();
  const hint = tool.locator('.hs-dev-review__hint');
  await expect(hint).toBeVisible();
  await page.waitForTimeout(3200);
  await expect(hint).toHaveClass(/hs-dev-review__hint--hidden/);
  await page.keyboard.down('Alt');
  await expect(page.locator('html')).toHaveClass(/hs-dev-review--crosshair/);
  await expect(page.locator('body')).toHaveCSS('cursor', 'crosshair');
  await page.mouse.move(markerBox.x, markerBox.y);
  await page.mouse.down();
  await page.mouse.move(markerBox.x + markerBox.width, markerBox.y + markerBox.height);
  await page.mouse.up();
  await page.keyboard.up('Alt');
  await expect(page.locator('html')).not.toHaveClass(/hs-dev-review--crosshair/);
  const selection = tool.locator('.hs-dev-review__rect');
  await expect(selection).toHaveCount(1);
  await expect(selection.locator('.hs-dev-review__handle')).toHaveCount(8);
  const beforeScroll = await selection.boundingBox();
  // Kerf 5.0.0-beta.55's catalog sidebar is a Workbench rail Pane whose content region owns scrolling.
  const scroller = page.locator('.kui-workbench__rail--left .kui-pane__content');
  const initialScroll = await scroller.evaluate((node) => node.scrollTop);
  await scroller.evaluate((node) => {
    node.scrollBy(0, -80);
  });
  await expect.poll(() => scroller.evaluate((node) => node.scrollTop)).toBeLessThan(initialScroll);
  const scrollDelta = await scroller.evaluate((node, start) => node.scrollTop - start, initialScroll);
  await expect.poll(async () => (await selection.boundingBox())!.y).toBeCloseTo(beforeScroll!.y - scrollDelta, 0);
  const stableSelectionNode = await selection.elementHandle();
  const before = await selection.boundingBox();
  const resize = selection.getByRole('button', { name: /Resize capture 1 from se/ });
  const handle = await resize.boundingBox();
  await page.mouse.move(handle!.x + 4, handle!.y + 4);
  await page.mouse.down();
  await page.mouse.move(handle!.x + 54, handle!.y + 34);
  await page.mouse.up();
  const after = await selection.boundingBox();
  expect(after!.width).toBeGreaterThan(before!.width);
  expect(after!.height).toBeGreaterThan(before!.height);
  expect(await stableSelectionNode.evaluate((node) => node.isConnected)).toBe(true);
  const corner = await resize.boundingBox();
  expect(Math.abs(corner!.width - corner!.height)).toBeLessThanOrEqual(1);
  const eastResize = selection.getByRole('button', { name: /Resize capture 1 from e$/ });
  await expect(eastResize).toHaveCSS('cursor', 'ew-resize');
  const eastHandle = await eastResize.boundingBox();
  const beforeEast = await selection.boundingBox();
  await page.mouse.move(eastHandle!.x + 4, eastHandle!.y + 10);
  await page.mouse.down();
  await page.mouse.move(eastHandle!.x + 34, eastHandle!.y + 10);
  await page.mouse.up();
  const afterEast = await selection.boundingBox();
  expect(afterEast!.width).toBeGreaterThan(beforeEast!.width);
  expect(Math.abs(afterEast!.height - beforeEast!.height)).toBeLessThanOrEqual(1);
  await page.keyboard.down('Alt');
  await page.mouse.move(720, 300);
  await page.mouse.down();
  await page.mouse.move(920, 500);
  await page.mouse.up();
  await page.keyboard.up('Alt');
  await expect(tool.locator('.hs-dev-review__rect')).toHaveCount(2);
  const removable = tool.locator('.hs-dev-review__rect').last();
  const removableBox = await removable.boundingBox();
  await page.keyboard.down('Alt');
  await page.keyboard.down('Shift');
  await page.mouse.move(removableBox!.x + 30, removableBox!.y + 30);
  await expect(removable).toHaveCSS('cursor', 'not-allowed');
  await page.mouse.click(removableBox!.x + 30, removableBox!.y + 30);
  await page.keyboard.up('Shift');
  await page.keyboard.up('Alt');
  await expect(tool.locator('.hs-dev-review__rect')).toHaveCount(1);
  await page.keyboard.down('Alt');
  await page.mouse.move(720, 300);
  await page.mouse.down();
  await page.mouse.move(920, 500);
  await page.mouse.up();
  await page.keyboard.up('Alt');
  await expect(tool.locator('.hs-dev-review__rect')).toHaveCount(2);
  await page.waitForTimeout(400);
  await tool.getByRole('button', { name: 'New Ticket' }).click();
  const dialog = page.getByRole('dialog', { name: 'New Hot Sheet ticket' });
  await expect(dialog).toBeVisible();
  expect(
    await dialog.evaluate((node) => {
      const style = (selector: string) => getComputedStyle(node.querySelector(selector)!);
      return {
        body: [style('.hs-dev-review__dialog-body').padding, style('.hs-dev-review__dialog-body').gap],
        footer: [style('footer').padding, style('footer').gap],
        evidence: style('.hs-dev-review__evidence').gap,
        dropzone: style('.hs-dev-review__dropzone').padding,
        label: style('.hs-dev-review__notes').gap,
        textarea: style('textarea').padding,
        button: style('footer button').padding,
      };
    }),
  ).toEqual({
    body: ['24px', '24px'],
    footer: ['12px 24px', '8px'],
    evidence: '16px',
    dropzone: '16px',
    label: '4px',
    textarea: '16px',
    button: '0px 16px',
  });
  await expect(dialog.locator('.app-heading')).toHaveCSS('border-bottom-width', '0px');
  await expect(dialog.locator('footer')).toHaveCSS('border-top-width', '0px');
  expect(
    await dialog.evaluate((node) => {
      const probe = document.createElement('span');
      probe.style.background = 'var(--wa-color-surface-border)';
      document.body.append(probe);
      const result = {
        border: getComputedStyle(node).borderColor,
        dividerMatches: getComputedStyle(node).borderColor === getComputedStyle(probe).backgroundColor,
        surface: getComputedStyle(node).backgroundColor,
      };
      probe.remove();
      return result;
    }),
  ).toEqual({ border: 'rgba(0, 0, 28, 0.18)', dividerMatches: true, surface: 'rgb(255, 255, 255)' });
  await expect(dialog.getByRole('button', { name: 'Review captured region 1' })).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Review captured region 2' })).toBeVisible();
  await expect(dialog.getByRole('img', { name: 'Captured region 1 preview' })).toHaveAttribute(
    'src',
    /^data:image\/png;base64,/,
  );
  await page.screenshot({ path: 'target/visual-captures/hs2-66m88k-dev-review-theme-wide.png', fullPage: true });
  await page.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-dev-review-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 900 });
  await expect(dialog).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-66m88k-dev-review-theme-narrow.png', fullPage: true });
  await page.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-dev-review-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 900 });
  const capturedPixels = await dialog
    .getByRole('img', { name: 'Captured region 1 preview' })
    .evaluate(async (image) => {
      if (!(image as HTMLImageElement).complete)
        await new Promise((resolve) => {
          image.addEventListener('load', resolve, { once: true });
        });
      const canvas = document.createElement('canvas');
      canvas.width = (image as HTMLImageElement).naturalWidth;
      canvas.height = (image as HTMLImageElement).naturalHeight;
      const context = canvas.getContext('2d')!;
      context.drawImage(image as HTMLImageElement, 0, 0);
      const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
      let green = 0;
      for (let index = 0; index < pixels.length; index += 4)
        if (pixels[index] === 12 && pixels[index + 1] === 200 && pixels[index + 2] === 34) green += 1;
      return {
        center: [...context.getImageData(Math.floor(canvas.width / 2), Math.floor(canvas.height / 2), 1, 1).data],
        green,
        width: canvas.width,
        height: canvas.height,
      };
    });
  expect(capturedPixels.center.slice(0, 3), JSON.stringify(capturedPixels)).toEqual([214, 233, 249]);
  await dialog.getByRole('button', { name: 'Review captured region 2' }).click();
  await expect(dialog.getByRole('img', { name: 'Captured region 2 preview' })).toBeVisible();
  const attachmentInput = dialog.getByLabel('Add attachments');
  await attachmentInput.setInputFiles({
    name: 'notes.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('review notes'),
  });
  await expect(dialog.getByText('notes.txt')).toBeVisible();
  await dialog.getByRole('button', { name: 'Remove attachment notes.txt' }).click();
  await expect(dialog.getByText('notes.txt')).toHaveCount(0);
  const dropzone = dialog.locator('.hs-dev-review__dropzone');
  await dropzone.evaluate((node) => {
    const transfer = new DataTransfer();
    transfer.items.add(new File(['log'], 'debug.log', { type: 'text/plain' }));
    node.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
  });
  await expect(dialog.getByText('debug.log')).toBeVisible();
  await dialog.getByRole('button', { name: 'Remove captured region 2' }).click();
  await expect(dialog.getByRole('button', { name: 'Review captured region 2' })).toHaveCount(0);
  await expect(tool.locator('.hs-dev-review__rect')).toHaveCount(1);
  await expect(dialog.getByRole('button', { name: 'Cancel' })).toHaveCount(1);
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(dialog).toHaveCount(0);
  await expect(tool.getByRole('button', { name: 'New Ticket' })).toBeVisible();
  page.once('dialog', (dialog) => void dialog.dismiss());
  await tool.getByRole('button', { name: 'Feedback' }).click();
  await expect(tool.locator('.hs-dev-review__rect')).toHaveCount(1);
  page.once('dialog', (dialog) => void dialog.accept());
  await tool.getByRole('button', { name: 'Feedback' }).click();
  await expect(tool.getByRole('button', { name: 'New Ticket' })).toHaveCount(0);
  await expect(tool.locator('.hs-dev-review__rect')).toHaveCount(0);
  await tool.getByRole('button', { name: 'Feedback' }).click();
  await page.keyboard.down('Alt');
  await page.mouse.move(500, 240);
  await page.mouse.down();
  await page.mouse.move(900, 480);
  await page.mouse.up();
  await page.keyboard.up('Alt');
  await tool.getByRole('button', { name: 'New Ticket' }).click();
  const reopened = page.getByRole('dialog', { name: 'New Hot Sheet ticket' });
  await reopened
    .getByLabel('Add attachments')
    .setInputFiles({ name: 'context.txt', mimeType: 'text/plain', buffer: Buffer.from('context') });
  await reopened.getByRole('textbox', { name: 'Feedback notes' }).fill('The selected row spacing is inconsistent.');
  await reopened.getByRole('button', { name: 'Create Ticket' }).click();
  await expect(reopened.getByRole('status')).toContainText('HS2-REVIEW created');
  await expect.poll(() => submitted).toBeTruthy();
  expect(submitted).toMatchObject({
    notes: 'The selected row spacing is inconsistent.',
    captures: [{ filename: expect.stringMatching(/^ux-feedback-\d+\.png$/) }],
    attachments: [{ filename: 'context.txt', mimeType: 'text/plain' }],
  });
  expect((submitted!.captures as Array<{ dataUrl: string }>)[0].dataUrl).toMatch(/^data:image\/png;base64,/);
  await expect(tool.getByRole('button', { name: 'New Ticket' })).toHaveCount(0);
});

test('inspects catalog component bounds and margins while skipping compositions', async ({ page }) => {
  await page.goto('/ux-demo?component=tag-chip&dev-review=1');
  const review = page.locator('.hs-dev-review'),
    toolbar = review.locator('.hs-dev-review__toolbar');
  await toolbar.getByRole('button', { name: 'Additional review utilities' }).click();
  await toolbar.getByRole('menuitem', { name: 'Inspect geometry' }).click();
  const overlay = review.locator('.hs-dev-review__geometry');
  await expect(overlay.locator('[data-kind="bounds"]')).not.toHaveCount(0);
  await toolbar.getByRole('button', { name: 'Additional review utilities' }).click();
  await expect(toolbar.getByRole('menuitem', { name: 'Inspect geometry' })).toHaveCount(0);
  await toolbar.getByRole('button', { name: 'Additional review utilities' }).click();
  await page
    .locator('[data-catalog-example-stack] [data-component]')
    .first()
    .evaluate((element) => {
      (element as HTMLElement).style.margin = '12px';
    });
  await expect(overlay.locator('[data-kind="margin"]')).not.toHaveCount(0);
  await page.screenshot({ path: 'target/visual-captures/hs2-wwrmfy-geometry-wide.png', fullPage: true });
  await page
    .getByRole('navigation', { name: 'UX components components' })
    .locator('[data-item-id="app-shell"]')
    .click();
  await expect(page.locator('[data-catalog-example-stack]')).toHaveAttribute('data-catalog-geometry-skip', '');
  await expect(overlay.locator('[data-kind="bounds"]')).toHaveCount(0);
  await page.getByRole('navigation', { name: 'UX components components' }).locator('[data-item-id="tag-chip"]').click();
  await expect(overlay.locator('[data-kind="bounds"]')).not.toHaveCount(0);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(overlay).toHaveAttribute('viewBox', '0 0 390 844');
  await page.screenshot({ path: 'target/visual-captures/hs2-wwrmfy-geometry-narrow.png', fullPage: true });
  await toolbar.getByRole('button', { name: 'Geometry' }).click();
  await expect(overlay).toHaveCount(0);
  await toolbar.getByRole('button', { name: 'Additional review utilities' }).click();
  await toolbar.getByRole('menuitem', { name: 'Inspect geometry' }).click();
  await toolbar.getByRole('button', { name: 'Feedback' }).click();
  await expect(overlay).toHaveCount(0);
  await page.locator('[data-action="toggle-dev-review"]').click();
  await expect(review).toHaveCount(0);
});

test('captures before and after CSSOM snapshots through CSS Live Edit', async ({ page }) => {
  let submitted: Record<string, unknown> | undefined;
  await page.route('**/__hotsheet/dev-review/tickets', async (route) => {
    submitted = route.request().postDataJSON();
    await route.fulfill({ status: 201, contentType: 'application/json', body: JSON.stringify({ slug: 'HS2-CSS' }) });
  });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-row&dev-review=1');
  const tool = page.locator('.hs-dev-review');
  const utilities = tool.getByRole('button', { name: 'Additional review utilities' });
  await utilities.click();
  await expect(utilities).toHaveAttribute('aria-expanded', 'true');
  await expect(tool.getByRole('menuitem', { name: 'CSS Live Edit' })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-x36s5n-css-live-edit-menu-wide.png', fullPage: true });
  await tool.getByRole('menuitem', { name: 'CSS Live Edit' }).click();
  await expect(tool.getByRole('button', { name: 'Feedback' })).toHaveCount(0);
  await expect(tool.getByRole('button', { name: 'CSS Live Edit' })).toHaveAttribute('title', 'Cancel CSS Live Edit');
  await expect(tool.locator('.hs-dev-review__hint')).toContainText('Live-edit CSS in browser developer tools');
  await tool.getByRole('button', { name: 'CSS Live Edit' }).click();
  await expect(tool.getByRole('button', { name: 'Feedback' })).toBeVisible();
  expect(submitted).toBeUndefined();
  await utilities.click();
  await tool.getByRole('menuitem', { name: 'CSS Live Edit' }).click();
  // Establish the evidence viewport before editing. Resizing can legitimately rerender the demo
  // composition, which would replace a DevTools-authored inline declaration before capture.
  await page.setViewportSize({ width: 760, height: 900 });
  // Wait for that rerender to finish (no DOM mutations for several frames) instead of assuming it is
  // already over; under full-suite load it could land after the edit and drop it (HS2-MHPHZB).
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        let quietFrames = 0;
        const observer = new MutationObserver(() => {
          quietFrames = 0;
        });
        observer.observe(document.body, { subtree: true, childList: true, attributes: true });
        const tick = () => {
          quietFrames += 1;
          if (quietFrames >= 10) {
            observer.disconnect();
            resolve();
          } else requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);
      }),
  );
  await page.evaluate(() => {
    const style = document.createElement('style');
    style.id = 'playwright-css-live-edit';
    style.textContent = '.kui-workbench__main { outline: 6px solid rgb(12, 200, 34); }';
    document.head.append(style);
    document.querySelector<HTMLElement>('.kui-workbench__main > .kui-pane > .kui-pane__header')!.style.paddingTop =
      '31px';
  });
  await expect(page.locator('.kui-workbench__main')).toHaveCSS('outline-width', '6px');
  await page.screenshot({ path: 'target/visual-captures/hs2-x36s5n-css-live-edit-active-narrow.png', fullPage: true });
  // Reassert the DevTools-authored inline declaration after screenshot/layout work so the
  // captured "after" CSSOM cannot race a responsive catalog rerender under full-suite load.
  await page.locator('.kui-workbench__main > .kui-pane > .kui-pane__header').evaluate((node) => {
    (node as HTMLElement).style.paddingTop = '31px';
  });
  await expect(page.locator('.kui-workbench__main > .kui-pane > .kui-pane__header')).toHaveCSS('padding-top', '31px');
  await tool.getByRole('button', { name: 'New Ticket' }).click();
  await expect.poll(() => submitted).toBeTruthy();
  expect(submitted).toMatchObject({
    notes: expect.stringContaining(
      'Compare the attached css-live-edit-before.css and css-live-edit-after.css snapshots',
    ),
    captures: [],
    attachments: [
      { filename: 'css-live-edit-before.css', mimeType: 'text/css' },
      { filename: 'css-live-edit-after.css', mimeType: 'text/css' },
    ],
  });
  const attachments = submitted!.attachments as Array<{ dataUrl: string }>;
  const decode = (value: string) => Buffer.from(value.slice(value.indexOf(',') + 1), 'base64').toString('utf8');
  const before = decode(attachments[0].dataUrl),
    after = decode(attachments[1].dataUrl);
  expect(before).not.toContain('playwright-css-live-edit');
  expect(after).toContain('stylesheet');
  expect(after).toContain('.kui-workbench__main { outline: rgb(12, 200, 34) solid 6px; }');
  expect(after).toContain('padding-top: 31px');
  expect(after).not.toBe(before);
  await expect(tool.getByText('HS2-CSS created.')).toBeVisible();
  await expect(tool.getByRole('button', { name: 'Feedback' })).toBeVisible();
});

test('keeps feedback rectangle input within its frame budget in the UX demo', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-row&dev-review=1');
  const measurement = await measureFeedbackRectangle(page, { x: 460, y: 260 }, { x: 780, y: 460 });
  await testInfo.attach('feedback-performance.json', {
    body: JSON.stringify(measurement, null, 2),
    contentType: 'application/json',
  });
  expectResponsiveFeedbackRectangle(measurement);
  await page.screenshot({ path: 'target/visual-captures/hs2-6ppvjc-ux-demo-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: 'target/visual-captures/hs2-6ppvjc-ux-demo-narrow.png', fullPage: true });
});

test('round-trips ConfidenceCalibration states through reset and a post-reset edit (HS2-Q1WCCY)', async ({ page }) => {
  await page.goto('/ux-demo?component=confidence-calibration');
  const panel = page.locator('.component-stage [data-component="confidence-calibration"]');
  await expect(panel).toHaveAttribute('data-state', 'ready');
  await expect(panel).toContainText('7 completions, 5 scored');
  await expect(panel.locator('tbody tr')).toHaveCount(5);
  await expect(panel.locator('tbody tr[data-band="assumed"] td').nth(4)).toHaveText('50%');
  await expect(panel.locator('tbody tr[data-band="partial"] wa-progress-bar')).toHaveCount(0);
  await expect(panel.locator('.confidence-calibration__recent li')).toHaveCount(7);
  await page.locator('[data-action="toggle-settings"]').click();
  const inspector = page.getByRole('complementary', { name: 'ConfidenceCalibration settings' });
  const state = inspector.locator('wa-select[name="state"]');
  const set = (value: string) =>
    state.evaluate((node: HTMLElement & { value: string }, next) => {
      node.value = next;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);
  await set('empty');
  await expect(panel).toHaveAttribute('data-state', 'empty');
  await expect(panel).toContainText('No completions yet');
  await set('loading');
  await expect(panel).toHaveAttribute('data-state', 'loading');
  await set('error');
  await expect(panel).toHaveAttribute('data-state', 'error');
  await expect(panel).toContainText('Confidence calibration is unavailable');
  await inspector.getByRole('button', { name: 'Reset' }).click();
  await expect(state).toHaveJSProperty('value', 'ready');
  await expect(panel).toHaveAttribute('data-state', 'ready');
  await set('empty');
  await expect(panel).toHaveAttribute('data-state', 'empty');
});

test('round-trips ConfidenceBadge appearance and band controls through reset and a post-reset edit (HS2-A0Q6G6)', async ({
  page,
}) => {
  await page.goto('/ux-demo?component=confidence-badge');
  const badge = page.locator('.component-stage [data-component="confidence-badge"]');
  await expect(badge).toHaveText('82%');
  await expect(badge).toHaveAttribute('data-band', 'assumed');
  await expect(badge).toHaveAttribute('data-appearance', 'compact');
  await page.locator('[data-action="toggle-settings"]').click();
  const inspector = page.getByRole('complementary', { name: 'ConfidenceBadge settings' });
  const value = inspector.locator('wa-select[name="value"]');
  const appearance = inspector.locator('wa-select[name="appearance"]');
  const set = (control: typeof value, next: string) =>
    control.evaluate((node: HTMLElement & { value: string }, to) => {
      node.value = to;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, next);
  for (const [score, band] of [
    ['96', 'verified'],
    ['55', 'partial'],
    ['20', 'unverified'],
  ] as const) {
    await set(value, score);
    await expect(badge).toHaveText(`${score}%`);
    await expect(badge).toHaveAttribute('data-band', band);
    await expect(badge).toHaveAccessibleName(`Confidence ${score} percent`);
  }
  await set(appearance, 'labeled');
  await expect(badge).toHaveText('Confidence 20%');
  await expect(badge).toHaveAttribute('data-appearance', 'labeled');
  await expect(badge.locator('[data-lucide="gauge"]')).toHaveCount(1);
  await inspector.getByRole('button', { name: 'Reset' }).click();
  await expect(value).toHaveJSProperty('value', '82');
  await expect(appearance).toHaveJSProperty('value', 'compact');
  await expect(badge).toHaveText('82%');
  await expect(badge).toHaveAttribute('data-band', 'assumed');
  await expect(badge).toHaveAttribute('data-appearance', 'compact');
  await set(value, '55');
  await expect(badge).toHaveText('55%');
  await expect(badge).toHaveAttribute('data-band', 'partial');
});

test('resets the TicketInspector, AIConversation, QuickTicketComposer, and ContentTransition settings (HS2-X1SM48)', async ({
  page,
}) => {
  // Eight demo navigations and reset cycles share one test deadline under the full worker load.
  test.setTimeout(120_000);
  const setValue = (control: Locator, value: string) =>
    control.evaluate((node: HTMLElement & { value: string }, next) => {
      node.value = next;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);
  const openSettings = async (component: string, name: string) => {
    await page.goto(`/ux-demo?component=${component}&dev-review=false`);
    const inspector = page.getByRole('complementary', { name: `${name} settings` });
    if (component === 'ai-conversation') {
      // The demo opens the conversation as a modal dialog; close it so the catalog chrome is reachable.
      await expect(page.locator('[data-component="ai-conversation"]').getByRole('dialog')).toBeVisible();
      await page.keyboard.press('Escape');
    }
    if (!(await inspector.isVisible()))
      await page.locator('[data-action="toggle-settings"][aria-expanded="false"]').click();
    await expect(inspector).toBeVisible();
    return inspector;
  };
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });

    // TicketInspector: live claim → reset to none → edit again.
    let settings = await openSettings('ticket-inspector', 'TicketInspector');
    const notice = page.locator('[data-component="ticket-inspector"] [data-component="live-claim-notice"]');
    const liveClaim = settings.locator('wa-select[name="inspector-live-claim"]');
    await setValue(liveClaim, 'overrun');
    await expect(notice).toHaveCount(1);
    await settings.getByRole('button', { name: 'Reset' }).click();
    await expect(liveClaim).toHaveJSProperty('value', 'none');
    await expect(notice).toHaveCount(0);
    await setValue(liveClaim, 'no-eta');
    await expect(notice).toContainText('Claude is working on this');
    await page.screenshot({ path: `target/visual-captures/hs2-x1sm48-ticket-inspector-settings-${width}.png` });

    // AIConversation: embedded + failed → reset to the dialog streaming state → edit again.
    settings = await openSettings('ai-conversation', 'AIConversation');
    const host = page.locator('[data-component="ai-conversation"]');
    const presentation = settings.locator('wa-select[name="presentation"]'),
      scenario = settings.locator('wa-select[name="scenario"]');
    await setValue(scenario, 'failed');
    await expect(host.first()).toContainText('Conversation unavailable');
    await setValue(presentation, 'embedded');
    await expect(page.locator('[data-component="ai-conversation"][data-presentation="embedded"]')).toHaveCount(1);
    await settings.getByRole('button', { name: 'Reset' }).click();
    await expect(presentation).toHaveJSProperty('value', 'dialog');
    await expect(scenario).toHaveJSProperty('value', 'streaming');
    await expect(host.getByRole('dialog')).toBeVisible();
    await expect(host.first()).toContainText('Running the focused browser test');
    await setValue(scenario, 'empty');
    await expect(host.first()).toContainText('Start a conversation');
    await page.keyboard.press('Escape');

    // QuickTicketComposer: one source → reset to several (source Select) → edit again.
    settings = await openSettings('quick-ticket-composer', 'QuickTicketComposer');
    const sourceCount = settings.locator('wa-select[name="composer-source-count"]'),
      source = page.locator('[data-component="quick-ticket-composer"] wa-select[name="new-ticket-source"]');
    const expectSourceSelect = async (count: number) => {
      await page.getByRole('button', { name: /New ticket/ }).click();
      await expect(source).toHaveCount(count);
      await page.getByRole('button', { name: /Cancel/ }).click();
    };
    await setValue(sourceCount, 'one');
    await expectSourceSelect(0);
    await settings.getByRole('button', { name: 'Reset' }).click();
    await expect(sourceCount).toHaveJSProperty('value', 'several');
    await expectSourceSelect(1);
    await setValue(sourceCount, 'one');
    await expect(sourceCount).toHaveJSProperty('value', 'one');
    await expectSourceSelect(0);

    // ContentTransition: crossfade on side B → reset to push on side A → edit again.
    settings = await openSettings('content-transition', 'ContentTransition');
    const transition = page.locator('[data-component="content-transition"][data-transition-region="content"]');
    const style = settings.locator('wa-select[name="transition-style"]'),
      side = settings.locator('wa-select[name="transition-side"]');
    await setValue(style, 'crossfade');
    await setValue(side, 'b');
    await expect(transition).toHaveAttribute('data-transition-style', 'crossfade');
    await expect(transition).toHaveAttribute('data-active-side', 'b');
    await settings.getByRole('button', { name: 'Reset' }).click();
    await expect(style).toHaveJSProperty('value', 'push');
    await expect(side).toHaveJSProperty('value', 'a');
    await expect(transition).toHaveAttribute('data-transition-style', 'push');
    await expect(transition).toHaveAttribute('data-active-side', 'a');
    await expect(transition).toHaveAttribute('data-transition-direction', 'forward');
    await setValue(style, 'none');
    await expect(transition).toHaveAttribute('data-transition-style', 'none');
    await page.screenshot({ path: `target/visual-captures/hs2-x1sm48-content-transition-settings-${width}.png` });
  }
});

test('paints the AppShell work surface once through its sunken Pane (HS2-7G8PZ3)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=app-shell');
    const shell = page.locator('[data-component="app-shell"]'),
      pane = shell.locator('.app-shell__main').locator('xpath=ancestor::*[@data-component="pane"][1]'),
      content = pane.locator('.kui-pane__content').first();
    await expect(pane).toHaveAttribute('data-appearance', 'sunken');
    await expect(pane).toHaveAttribute('data-chrome-dividers', 'none');
    await expect(shell.locator('.app-shell__work-area')).toHaveCSS('border-top-width', '1px');
    await expect(content).not.toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await expect(shell.locator('.app-shell__work-area')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await expect(shell.locator('.app-shell__workspace')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await page.screenshot({ path: `target/visual-captures/hs2-7g8pz3-app-shell-${width}.png`, animations: 'disabled' });
  }
});

test('round-trips AppShell presentation and work-area focus-ring settings (HS2-8ZJMCE)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=app-shell');
    const shell = page.locator('[data-component="app-shell"]');
    const workArea = shell.locator('.app-shell__work-area');
    await expect(shell).toHaveAttribute('data-presentation', 'framed');
    await expect(workArea).toHaveAttribute('data-focus-ring', 'true');
    const framedRadius = await shell.evaluate((node) => getComputedStyle(node).borderTopLeftRadius);
    expect(framedRadius).not.toBe('0px');
    await page.locator('[data-action="toggle-settings"]').first().click();
    const inspector = page.getByRole('complementary', { name: 'AppShell settings' });
    const presentation = inspector.locator('wa-select[name="presentation"]');
    const overlay = inspector.locator('wa-checkbox[name="overlay-open"]');
    await expect(presentation).toHaveJSProperty('value', 'framed');
    await expect(overlay).toHaveJSProperty('checked', false);
    // Control → render: viewport drops the frame; an open overlay turns the work-area ring off.
    await presentation.evaluate((node: HTMLElement & { value: string }) => {
      node.value = 'viewport';
      node.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await expect(shell).toHaveAttribute('data-presentation', 'viewport');
    await expect(shell).toHaveCSS('border-top-left-radius', '0px');
    await overlay.click();
    await expect(overlay).toHaveJSProperty('checked', true);
    await expect(workArea).toHaveAttribute('data-focus-ring', 'false');
    await workArea.focus();
    await expect(workArea).toHaveCSS('outline-color', 'rgba(0, 0, 0, 0)');
    await page.screenshot({ path: `target/visual-captures/hs2-8zjmce-app-shell-viewport-overlay-${width}.png` });
    // Reset → every live control and the render return to the defaults.
    await inspector.getByRole('button', { name: 'Reset' }).click();
    await expect(presentation).toHaveJSProperty('value', 'framed');
    await expect(overlay).toHaveJSProperty('checked', false);
    await expect(shell).toHaveAttribute('data-presentation', 'framed');
    await expect(workArea).toHaveAttribute('data-focus-ring', 'true');
    await workArea.focus();
    await expect(workArea).not.toHaveCSS('outline-color', 'rgba(0, 0, 0, 0)');
    // Edit again after the reset.
    await overlay.click();
    await expect(workArea).toHaveAttribute('data-focus-ring', 'false');
    await expect(shell).toHaveAttribute('data-presentation', 'framed');
    await page.screenshot({ path: `target/visual-captures/hs2-8zjmce-app-shell-framed-overlay-${width}.png` });
  }
});

test('round-trips the AppShell phone view header and its Empty Trash action (HS2-T35VN7)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=app-shell');
  const shell = page.locator('[data-component="app-shell"]'),
    demo = page.getByRole('region', { name: 'AppShell demo' }),
    header = shell.locator('.app-shell__mobile-view-header'),
    viewSelect = header.locator('wa-select[name="mobile-view"]'),
    emptyTrash = header.getByRole('button', { name: 'Empty Trash' });
  await expect(header).toHaveCount(0);
  await page.locator('[data-action="toggle-settings"]').first().click();
  const inspector = page.getByRole('complementary', { name: 'AppShell settings' }),
    toggle = inspector.locator('wa-checkbox[name="mobile-view-header"]');
  await expect(toggle).toHaveJSProperty('checked', false);
  // Control → render: the header replaces the page header with the view Select and launcher.
  await toggle.click();
  await expect(toggle).toHaveJSProperty('checked', true);
  await expect(viewSelect).toHaveJSProperty('value', 'all');
  await expect(header.locator('[data-component="quick-ticket-composer-launcher"]')).toHaveCount(1);
  // Choosing Trash swaps the trailing action, and the action is wired.
  await viewSelect.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'trash';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(viewSelect).toHaveJSProperty('value', 'trash');
  await expect(emptyTrash).toBeVisible();
  await expect(header.locator('[data-component="quick-ticket-composer-launcher"]')).toHaveCount(0);
  await emptyTrash.click();
  await expect(demo.locator('.component-stage__event')).toHaveText('Empty Trash confirmation requested.');
  await header.screenshot({ path: 'target/visual-captures/hs2-t35vn7-app-shell-phone-header-trash.png' });
  // Reset → the toggle, the selected view, and the render return to the defaults.
  await inspector.getByRole('button', { name: 'Reset' }).click();
  await expect(toggle).toHaveJSProperty('checked', false);
  await expect(header).toHaveCount(0);
  // Edit again after the reset: the view selection was reset to Queue too.
  await toggle.click();
  await expect(viewSelect).toHaveJSProperty('value', 'all');
  await expect(emptyTrash).toHaveCount(0);
});

test('round-trips StatusBadge controls through reset and a post-reset edit', async ({ page }) => {
  await page.goto('/ux-demo?component=status-badge');
  const badge = page.locator('[data-component="status-badge"]');
  await expect(badge).toContainText('Started');
  await expect(badge.locator('[data-lucide="clock"]')).toHaveAttribute('aria-hidden', 'true');
  await page.locator('[data-action="toggle-settings"]').click();
  const inspector = page.getByRole('complementary', { name: 'StatusBadge settings' });
  const status = inspector.locator('wa-select[name="status"]');
  const appearance = inspector.locator('wa-select[name="appearance"]');
  const icon = inspector.locator('wa-checkbox[name="show-icon"]');
  const compact = inspector.locator('wa-checkbox[name="compact"]');
  const weight = inspector.locator('wa-select[name="weight"]');
  await expect(badge).toHaveCSS('font-weight', '700');
  await weight.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'semibold';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(badge).toHaveClass(/status-badge--semibold/);
  await expect(badge).toHaveCSS('font-weight', '600');
  await status.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'verified';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(badge).toContainText('Verified');
  await expect(badge.locator('[data-lucide="badge-check"]')).toHaveCount(1);
  await appearance.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'plain';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(badge).toHaveAttribute('data-appearance', 'plain');
  await expect(badge).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await compact.click();
  await expect(badge).toHaveClass(/status-badge--compact/);
  await icon.click();
  await expect(badge.locator('.status-badge__icon')).toHaveCount(0);
  await inspector.getByRole('button', { name: 'Reset' }).click();
  await expect(status).toHaveJSProperty('value', 'started');
  await expect(appearance).toHaveJSProperty('value', 'filled');
  await expect(icon).toHaveJSProperty('checked', true);
  await expect(compact).toHaveJSProperty('checked', false);
  await expect(weight).toHaveJSProperty('value', 'bold');
  await expect(badge).not.toHaveClass(/status-badge--semibold/);
  await expect(badge).toHaveCSS('font-weight', '700');
  await expect(badge).toContainText('Started');
  await expect(badge).toHaveAttribute('data-appearance', 'filled');
  await expect(badge).not.toHaveClass(/status-badge--compact/);
  await expect(badge.locator('[data-lucide="clock"]')).toHaveCount(1);
  await expect(badge.locator('.status-badge__icon')).toHaveCount(1);
  await status.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'completed';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(badge).toContainText('Completed');
  await expect(badge.locator('[data-lucide="circle-check"]')).toHaveCount(1);
});

test('shows remote final testing as a Started phase at wide and phone widths', async ({ page }, testInfo) => {
  await page.goto('/ux-demo?component=status-badge');
  await page.locator('[data-action="toggle-settings"]').click();
  const phase = page.locator('wa-select[name="started-phase"]');
  await phase.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'final_testing';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  const badge = page.locator('[data-component="status-badge"]');
  await expect(badge).toContainText('Final testing');
  await expect(badge).toHaveAttribute('data-status', 'started');
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.screenshot({ path: testInfo.outputPath('final-testing-wide.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: 'Close settings' }).click();
  await expect(badge).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('final-testing-phone.png') });
});

test('shows a released final-testing ticket in the list row at wide and phone widths', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=ticket-row');
  await page.locator('[data-action="toggle-settings"]').click();
  const settings = page.locator('[data-settings="ticket-list-row"]');
  await settings.locator('wa-select[name="started-phase"]').evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'final_testing';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  const busy = settings.locator('wa-checkbox[name="busy"]');
  await busy.click();
  const row = page.locator('[data-component="ticket-list-row"]');
  await expect(row.locator('[data-component="status-badge"]')).toContainText('Final testing');
  await expect(row.locator('[data-component="status-badge"]')).toHaveAttribute('data-status', 'started');
  await page.screenshot({ path: testInfo.outputPath('final-testing-row-wide.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: 'Close settings' }).click();
  await expect(row).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('final-testing-row-phone.png') });
});

test('shows a GitHub issue number without its repository prefix in the ticket row (HS2-04C25A)', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 560, height: 760 });
  await page.goto('/ux-demo?component=ticket-row');
  await page.locator('[data-action="toggle-settings"]').click();
  await page.getByRole('textbox', { name: 'Title' }).fill('i deleted a github issue-based ticket that was verified');
  await page.getByRole('button', { name: 'Close settings' }).click();
  const row = page.locator('[data-component="ticket-list-row"]');
  await expect(row).toHaveAttribute('data-ticket-slug', 'Small-Tale/hotsheet2#5');
  await expect(row.locator('.ticket-list-row__slug')).toHaveText('#5');
  await expect(row.locator('[data-component="ticket-source-icon"]')).toHaveAttribute('data-provider', 'github');
  await row.screenshot({ path: testInfo.outputPath('github-issue-number-560.png'), animations: 'disabled' });

  await page.setViewportSize({ width: 390, height: 844 });
  await expect(row.locator('.ticket-list-row__slug')).toHaveText('#5');
  await row.screenshot({ path: testInfo.outputPath('github-issue-number-390.png'), animations: 'disabled' });
});

test('demonstrates the production Not Working dialog and pending evidence picker', async ({ page }) => {
  await page.goto('/ux-demo?component=not-working-dialog');
  await page.getByRole('button', { name: 'Open Not Working dialog' }).click();
  const dialog = page.getByRole('dialog', { name: 'Not Working — HS2-DEMO' });
  // wa-dialog projects its content into a shadow-DOM native <dialog> in the top layer, so the
  // host element itself has zero height — asserting toBeVisible() on the host is wrong (the real
  // app's tests assert content/focus instead). Assert the dialog opened, then that its content shows.
  await expect(dialog).toHaveAttribute('open', '');
  await expect(dialog.getByText('diagnostic screenshot with a deliberately long filename.png')).toBeVisible();
  await dialog.getByRole('textbox', { name: 'What’s wrong?' }).fill('The verification failed.');
  await dialog
    .getByLabel('Browse evidence attachments')
    .setInputFiles({ name: 'new-proof.txt', mimeType: 'text/plain', buffer: Buffer.from('proof') });
  await expect(dialog.getByText('new-proof.txt')).toBeVisible();
  await dialog.getByRole('button', { name: 'Remove new-proof.txt' }).click();
  await expect(dialog.getByText('new-proof.txt')).toHaveCount(0);
  await dialog.getByRole('button', { name: 'Report Not Working' }).click();
  await expect(page.getByText('Ticket returned to Not Started and added to Up Next.')).toBeVisible();
});

test('previews and dismisses the application danger banner at wide and phone widths (HS2-KEHG7H)', async ({ page }) => {
  await page.goto('/ux-demo?component=app-error');
  const error = page.locator('[data-component="app-error"]');
  for (const width of [1100, 390]) {
    await page.setViewportSize({ width, height: 760 });
    await expect(error).toHaveAttribute('data-component', 'app-error');
    await expect(error.getByRole('alert')).toContainText('The project could not be opened.');
    const box = await error.boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(width);
    const copyBox = await error.locator('.kui-state-banner__copy').boundingBox();
    const actionBox = await error.locator('.kui-state-banner__action').boundingBox();
    expect(actionBox!.y).toBeGreaterThanOrEqual(copyBox!.y + copyBox!.height);
    await page.screenshot({ path: test.info().outputPath(`app-error-${width}.png`) });
    await error.getByRole('button', { name: 'Dismiss error' }).click();
    await expect(error).toHaveCount(0);
    await page.getByRole('button', { name: 'Show error' }).click();
  }
});

test('switches the TicketInspector live-claim header through every claim state (HS2-QKNQXC)', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 900 });
  await page.goto('/ux-demo?component=ticket-inspector');
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await expect(inspector).toBeVisible();
  const notice = inspector.locator('[data-component="live-claim-notice"]'),
    eta = notice.locator('[data-claim-eta]');
  await expect(notice).toHaveCount(0);
  await page.locator('[data-action="toggle-settings"]').click();
  const control = page
    .getByRole('complementary', { name: 'TicketInspector settings' })
    .locator('wa-select[name="inspector-live-claim"]');
  await expect(control).toHaveJSProperty('value', 'none');
  const setLiveClaim = async (value: string) => {
    await control.evaluate((node: HTMLElement & { value: string }, next) => {
      node.value = next;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);
    await expect(control).toHaveJSProperty('value', value);
  };
  await setLiveClaim('estimate');
  await expect(notice).toContainText('Claude is working on this');
  await expect(notice.locator('[data-component="loading-spinner"]')).toHaveCount(0);
  await expect(eta).toHaveAttribute('data-claim-eta', 'estimate');
  const claimRing = notice.locator('.claim-eta__ring');
  await expect(claimRing).toHaveJSProperty('value', 25);
  const ringBox = await claimRing.boundingBox();
  expect(ringBox!.width).toBeCloseTo(16.8, 1);
  expect(ringBox!.height).toBeCloseTo(16.8, 1);
  await expect(eta).toHaveText('~45m left');
  await expect(eta.locator('wa-progress-ring')).toHaveCount(0);
  // The notice leads the header notices and sits inside the inspector's width.
  const [noticeBox, inspectorBox] = await Promise.all([notice.boundingBox(), inspector.boundingBox()]);
  expect(noticeBox!.x).toBeGreaterThanOrEqual(inspectorBox!.x);
  expect(noticeBox!.x + noticeBox!.width).toBeLessThanOrEqual(inspectorBox!.x + inspectorBox!.width + 0.5);
  await notice.screenshot({ path: test.info().outputPath('live-claim-notice-estimate.png') });
  await setLiveClaim('overrun');
  await expect(eta).toHaveAttribute('data-claim-eta', 'overrun');
  await expect(eta).toHaveText('Soon');
  await expect(eta.locator('wa-progress-ring')).toHaveCount(0);
  await setLiveClaim('no-eta');
  await expect(notice).toContainText('Claude is working on this');
  await expect(eta).toHaveCount(0);
  await expect(notice.locator('[data-component="loading-spinner"]')).toBeVisible();
  await setLiveClaim('none');
  await expect(notice).toHaveCount(0);
  await setLiveClaim('estimate');
  await expect(eta).toHaveText('~45m left');
  await page.locator('[data-action="toggle-settings"]').click();
  await page.screenshot({ path: test.info().outputPath('live-claim-wide.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(notice).toBeVisible();
  const narrowRing = await notice.locator('.claim-eta__ring').boundingBox();
  expect(narrowRing!.width).toBeCloseTo(16.8, 1);
  expect(narrowRing!.height).toBeCloseTo(16.8, 1);
  await page.screenshot({ path: test.info().outputPath('live-claim-narrow.png') });
  await page.locator('[data-action="toggle-settings"]').click();
  await setLiveClaim('generated-id');
  await page.locator('[data-action="toggle-settings"]').click();
  await expect(notice).toContainText('Codex is working on this');
  await expect(notice).toHaveAttribute('title', 'codex-01M44ZE2BPEZKWTW49DCTATTPW is actively working on this ticket');
  await expect(eta).toHaveText('~45m left');
  for (const width of [390, 1100]) {
    await page.setViewportSize({ width, height: 844 });
    expect(await notice.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
    await notice.screenshot({ path: test.info().outputPath(`live-claim-generated-id-${width}.png`) });
  }
});

test('projects phone project attention through selection and reset (HS2-34VG07)', async ({ page }) => {
  await page.goto('/ux-demo?component=project-tabs');
  const frame = page.locator('[aria-label="Phone project picker"]'),
    picker = frame.locator('wa-select[name="mobile-project"]'),
    selected = picker.locator('.project-tab-bar__selected-project'),
    attention = selected.locator('[data-lucide="circle-alert"]');
  await expect(picker).toHaveJSProperty('value', 'hotsheet');
  await expect(attention).toHaveCount(0);
  await picker.click();
  const halted = picker.getByRole('option', { name: 'Internal API — Needs attention' });
  await expect(halted.locator('[data-lucide="circle-alert"]')).toBeVisible();
  await halted.click();
  await expect(picker).toHaveJSProperty('value', 'api');
  await expect(selected).toContainText('Internal API');
  await expect(attention).toBeVisible();
  await expect(attention).toHaveAttribute('aria-label', 'An AI session stopped on an error');
  for (const width of [1100, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await frame.screenshot({ path: test.info().outputPath(`phone-project-attention-${width}.png`) });
  }
  // Programmatic selection through another composition updates the picker and every facet.
  await page.setViewportSize({ width: 1100, height: 844 });
  await page.locator('[data-tab-kind="project"][data-project-id="hotsheet"]').first().click();
  await expect(picker).toHaveJSProperty('value', 'hotsheet');
  await expect(selected).toHaveText('Hot Sheet 2');
  await expect(attention).toHaveCount(0);
  await picker.click();
  await halted.click();
  await expect(picker).toHaveJSProperty('value', 'api');
  await expect(attention).toBeVisible();
});

test('uses the row claim slot for one same-size ring or spinner at wide and phone widths (HS2-SNC0S3/1FYDF0)', async ({
  page,
}) => {
  await page.goto('/ux-demo?component=ticket-row');
  const row = page.locator('[data-component="ticket-list-row"]');
  await page.locator('[data-action="toggle-settings"]').click();
  const claimEta = page
    .getByRole('complementary', { name: 'TicketRow settings' })
    .locator('wa-select[name="claim-eta"]');
  const setClaimEta = (value: string) =>
    claimEta.evaluate((node: HTMLElement & { value: string }, next) => {
      node.value = next;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);
  for (const width of [1100, 390]) {
    await page.setViewportSize({ width, height: 844 });
    const ring = row.locator('.ticket-list-row__claim .claim-eta__ring');
    await expect(ring).toBeVisible();
    await expect(row.locator('[data-component="loading-spinner"]')).toHaveCount(0);
    const ringBox = await ring.boundingBox();
    expect(ringBox!.width).toBeCloseTo(16.8, 1);
    expect(ringBox!.height).toBeCloseTo(16.8, 1);
    await expect(row.locator('[data-claim-eta]')).toHaveText('~45m left');
    await setClaimEta('none');
    const spinner = row.locator('[data-component="loading-spinner"]');
    await expect(spinner).toBeVisible();
    await expect(spinner).toHaveAttribute('style', /color:var\(--hs-ticket-state-up-next\)/);
    const box = await spinner.boundingBox();
    expect(box!.width).toBeCloseTo(16.8, 1);
    expect(box!.height).toBeCloseTo(16.8, 1);
    await setClaimEta('estimate');
    await page.getByRole('button', { name: 'Close settings' }).click();
    await page.screenshot({ path: test.info().outputPath(`live-claim-row-${width}.png`) });
    await page.locator('[data-action="toggle-settings"]').click();
  }
});

test('clips a long worker ID inside the TicketRow while retaining its full accessible name (HS2-3H4Y31)', async ({
  page,
}) => {
  await page.goto('/ux-demo?component=ticket-row');
  await page.locator('[data-action="toggle-settings"]').click();
  const worker = 'codex-01M44ZE2BPEZKWTW49DCTATTPW';
  const agent = page
    .getByRole('complementary', { name: 'TicketRow settings' })
    .getByRole('textbox', { name: 'Active agent' });
  const row = page.locator('[data-component="ticket-list-row"]');
  const owner = row.locator('.ticket-list-row__owner');
  await agent.fill(worker);
  await page.locator('[data-action="toggle-settings"]').click();
  for (const width of [1100, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await expect(owner).toHaveAttribute('title', worker);
    await expect(owner).toHaveAttribute('aria-label', worker);
    await expect(owner).toHaveCSS('text-overflow', 'ellipsis');
    const dimensions = await owner.evaluate((node) => ({ width: node.clientWidth, content: node.scrollWidth }));
    expect(dimensions.content).toBeGreaterThan(dimensions.width);
    const [ownerBox, rowBox] = await Promise.all([owner.boundingBox(), row.boundingBox()]);
    expect(ownerBox!.x + ownerBox!.width).toBeLessThanOrEqual(rowBox!.x + rowBox!.width);
    await row.screenshot({ path: test.info().outputPath(`long-worker-${width}.png`) });
  }
  await page.locator('[data-action="toggle-settings"]').click();
  await agent.fill('Codex');
  await page.locator('[data-action="toggle-settings"]').click();
  await expect(owner).toHaveText('Codex');
  const shortDimensions = await owner.evaluate((node) => ({ width: node.clientWidth, content: node.scrollWidth }));
  expect(shortDimensions.content).toBeLessThanOrEqual(shortDimensions.width);
});

test('keeps a long GitHub issue title scannable in a 228px TicketRow (HS2-KWPCFP)', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/ux-demo?component=ticket-row');
  await page.locator('[data-action="toggle-settings"]').click();
  const title = 'Improve search results when several filters change together';
  await page
    .getByRole('complementary', { name: 'TicketRow settings' })
    .getByRole('textbox', { name: 'Title' })
    .fill(title);
  await page.getByRole('button', { name: 'Close settings' }).click();

  const row = page.locator('[data-component="ticket-list-row"]');
  const identity = row.locator('.ticket-list-row__identity');
  const metrics = await identity.evaluate((node) => {
    const strong = node.querySelector('strong')!;
    const text = strong.firstChild!;
    const clip = node.getBoundingClientRect();
    const range = document.createRange();
    let visibleTitleCharacters = 0;
    for (let index = 0; index < text.textContent!.length; index += 1) {
      range.setStart(text, index);
      range.setEnd(text, index + 1);
      if ([...range.getClientRects()].some((rect) => rect.top >= clip.top && rect.bottom <= clip.bottom + 1)) {
        visibleTitleCharacters += 1;
      }
    }
    const style = getComputedStyle(node);
    return {
      rowWidth: node.closest('.ticket-list-row')!.getBoundingClientRect().width,
      visibleTitleCharacters,
      lines: Number.parseFloat(style.maxHeight) / Number.parseFloat(style.lineHeight),
    };
  });
  expect(metrics.rowWidth).toBeGreaterThanOrEqual(220);
  expect(metrics.rowWidth).toBeLessThanOrEqual(235);
  expect(metrics.lines).toBeCloseTo(3, 1);
  expect(metrics.visibleTitleCharacters).toBeGreaterThanOrEqual(12);
  await expect(row).toHaveAttribute('aria-label', `Small-Tale/hotsheet2#5: ${title}`);
  await row.screenshot({ path: test.info().outputPath('github-issue-title-390.png') });
});

test('round-trips every TicketRow setting and selection action', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-row');
  const row = page.locator('[data-component="ticket-list-row"]');
  await expect(row).toContainText('Build the first client ticket list');
  await page.locator('[data-action="toggle-settings"]').click();
  const inspector = page.getByRole('complementary', { name: 'TicketRow settings' });
  const title = inspector.getByRole('textbox', { name: 'Title' });
  const category = inspector.getByRole('textbox', { name: 'Category' });
  const tags = inspector.getByRole('textbox', { name: 'Tags (comma separated)' });
  const agent = inspector.getByRole('textbox', { name: 'Active agent' });
  const updated = inspector.getByRole('textbox', { name: 'Updated label' });
  const status = inspector.locator('wa-select[name="status"]');
  const priority = inspector.locator('wa-select[name="priority"]');
  const categoryIcon = inspector.locator('wa-select[name="category-icon"]');
  const categoryColor = inspector.locator('wa-select[name="category-color"]');
  const upNext = inspector.locator('wa-checkbox[name="up-next"]');
  const blocked = inspector.locator('wa-checkbox[name="blocked"]');
  const needsReview = inspector.locator('wa-checkbox[name="needs-review"]');
  const selected = inspector.locator('wa-checkbox[name="selected"]');
  const busy = inspector.locator('wa-checkbox[name="busy"]');
  const claimEta = inspector.locator('wa-select[name="claim-eta"]');
  const eta = row.locator('[data-claim-eta]');
  const setClaimEta = (value: string) =>
    claimEta.evaluate((node: HTMLElement & { value: string }, next) => {
      node.value = next;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);
  // A live claim with an estimate shows a progress ring and the time left (HS2-XQMDQB).
  await expect(eta).toHaveAttribute('data-claim-eta', 'estimate');
  await expect(eta).toHaveText('~45m left');
  await expect(row.locator('.ticket-list-row__claim wa-progress-ring')).toHaveJSProperty('value', 25);
  await expect(eta.locator('wa-progress-ring')).toHaveCount(0);
  await title.fill('Fix selection synchronization');
  await category.fill('bug');
  await tags.fill('client, regression');
  await agent.fill('Codex');
  await updated.fill('Now');
  await status.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'verified';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await priority.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'urgent';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await categoryIcon.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'bug';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await categoryColor.evaluate((node: HTMLElement & { value: string }) => {
    node.value = '#ef4444';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await upNext.click();
  await blocked.click();
  await needsReview.click();
  await selected.click();
  await expect(row).toContainText('Codex');
  await busy.click();
  // Without live work no estimate shows, whatever the ETA setting.
  await expect(eta).toHaveCount(0);
  await expect(row.locator('.ticket-list-row__owner')).toHaveCount(0);
  await setClaimEta('overrun');
  await expect(eta).toHaveCount(0);
  // HS2-A0Q6G6: a verified row shows the compact confidence pill; changing the score
  // updates its percentage and band, and unscored removes it.
  const confidence = inspector.locator('wa-select[name="confidence"]');
  const confidencePill = row.locator('[data-component="confidence-badge"]');
  const setConfidence = (value: string) =>
    confidence.evaluate((node: HTMLElement & { value: string }, next) => {
      node.value = next;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);
  await expect(confidencePill).toHaveCount(0);
  await setConfidence('82');
  await expect(confidencePill).toHaveText('82%');
  await expect(confidencePill).toHaveAttribute('data-band', 'assumed');
  await expect(confidencePill).toHaveAccessibleName('Confidence 82 percent');
  await expect(confidencePill.locator('[data-lucide="gauge"]')).toHaveCount(1);
  await setConfidence('25');
  await expect(confidencePill).toHaveText('25%');
  await expect(confidencePill).toHaveAttribute('data-band', 'unverified');
  const feedbackNeeded = inspector.locator('wa-checkbox[name="feedback-needed"]');
  await expect(row.locator('.ticket-list-row__feedback')).toContainText('Needs review');
  await feedbackNeeded.click();
  await expect(row.locator('.ticket-list-row__feedback')).toContainText('Needs review');
  await expect(row.locator('.ticket-list-row__feedback [data-lucide="circle-alert"]')).toHaveCount(1);
  await needsReview.click();
  await expect(row.locator('.ticket-list-row__feedback')).toContainText('Needs review');
  await feedbackNeeded.click();
  await expect(row.locator('.ticket-list-row__feedback')).toHaveCount(0);
  await feedbackNeeded.click();
  await expect(row).toContainText('Fix selection synchronization');
  await expect(row).toContainText('Verified');
  await expect(row.locator('[data-component="blocked-badge"]')).toHaveText('Blocked');
  // Rows use the colored (filled) status variant like the inspector (HS2-Y3H2Z5).
  await expect(row.locator('[data-component="status-badge"]')).toHaveAttribute('data-appearance', 'filled');
  await expect(row.locator('[data-component="status-badge"]')).toHaveCSS(
    'background-color',
    'color(srgb 0.84 0.913412 0.977412)',
  );
  await expect(row.locator('[data-lucide="bug"]')).toHaveCount(1);
  await expect(row.locator('.ticket-list-row__category')).toHaveCSS('color', 'rgb(239, 68, 68)');
  await categoryColor.evaluate((node: HTMLElement & { value: string }) => {
    node.value = '#e5e7eb';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(row.locator('.ticket-list-row__category')).toHaveCSS('color', 'rgb(156, 163, 175)');
  await categoryIcon.evaluate((node: HTMLElement & { value: string }) => {
    node.value = '';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(row.locator('.ticket-list-row__category--label')).toHaveText('BUG');
  await expect(row.locator('.ticket-list-row__category--label')).toHaveCSS('color', 'rgb(156, 163, 175)');
  await expect(row.locator('.ticket-list-row__category--label')).toHaveCSS('width', '32px');
  await expect(row).toContainText('regression');
  await expect(row.locator('[data-lucide="star"]')).toHaveCount(0);
  await expect(row.locator('[data-action="toggle-row-up-next"]')).toHaveCount(0);
  await expect(row).toContainText('Now');
  await expect(row).toHaveAttribute('data-selected', 'true');
  await expect(row.locator('[data-lucide="chevrons-up"]')).toHaveCount(1);
  await expect(row.locator('.ticket-list-row__priority')).toHaveCSS('color', 'rgb(235, 0, 5)');
  await expect(row.locator('.ticket-list-row__indicator')).toHaveClass(/needs-review/);
  await row.click();
  await expect(selected).toHaveJSProperty('checked', false);
  await expect(row).toHaveAttribute('data-selected', 'false');
  await expect(page.getByText('Ticket deselected')).toBeVisible();
  await inspector.getByRole('button', { name: 'Reset' }).click();
  await expect(title).toHaveJSProperty('value', 'Build the first client ticket list');
  await expect(status).toHaveJSProperty('value', 'started');
  await expect(priority).toHaveJSProperty('value', 'high');
  await expect(category).toHaveJSProperty('value', 'feature');
  await expect(tags).toHaveJSProperty('value', 'client, ux');
  await expect(agent).toHaveJSProperty('value', 'Claude');
  await expect(updated).toHaveJSProperty('value', '1h ago');
  await expect(upNext).toHaveJSProperty('checked', true);
  await expect(blocked).toHaveJSProperty('checked', false);
  await expect(needsReview).toHaveJSProperty('checked', false);
  await expect(categoryIcon).toHaveJSProperty('value', 'sparkles');
  await expect(categoryColor).toHaveJSProperty('value', '#3b82f6');
  await expect(selected).toHaveJSProperty('checked', false);
  await expect(busy).toHaveJSProperty('checked', true);
  await expect(claimEta).toHaveJSProperty('value', 'estimate');
  await expect(eta).toHaveAttribute('data-claim-eta', 'estimate');
  await expect(confidence).toHaveJSProperty('value', 'none');
  await expect(confidencePill).toHaveCount(0);
  await expect(row).toContainText('Build the first client ticket list');
  await expect(row).toContainText('Started');
  await expect(row.locator('[data-action="toggle-row-up-next"]')).toHaveClass(/active/);
  await expect(row.locator('.ticket-list-row__category')).toHaveCSS('width', '32px');
  await expect(row.locator('.ticket-list-row__indicator')).toHaveClass(/up-next/);
  const upNextRailColor = await row
    .locator('.ticket-list-row__indicator')
    .evaluate((element) => getComputedStyle(element).backgroundColor);
  await expect(row.locator('[data-action="toggle-row-up-next"]')).toHaveCSS('color', upNextRailColor);
  // The active star is filled, not only outlined (HS2-KGHRHS).
  await expect(row.locator('[data-action="toggle-row-up-next"] svg')).toHaveCSS('fill', upNextRailColor);
  await expect(row).toContainText('Claude');
  await expect(row).toContainText('1h ago');
  await expect(page.getByText('No actions yet')).toBeVisible();
  await title.fill('Post-reset edit works');
  await expect(row).toContainText('Post-reset edit works');
  await setClaimEta('overrun');
  await expect(eta).toHaveAttribute('data-claim-eta', 'overrun');
  await expect(eta).toHaveText('Soon');
  await expect(eta.locator('wa-progress-ring')).toHaveCount(0);
  await setClaimEta('none');
  await expect(eta).toHaveCount(0);
  await setClaimEta('estimate');
  await expect(eta).toHaveText('~45m left');
  // A started row never shows a score, even when one is set.
  await setConfidence('94');
  await expect(confidencePill).toHaveCount(0);
  await row.focus();
  await page.keyboard.press('Enter');
  await expect(row).toHaveAttribute('data-selected', 'true');
  await expect(selected).toHaveJSProperty('checked', true);

  const star = row.locator('[data-action="toggle-row-up-next"]');
  const starBox = await star.boundingBox();
  expect(starBox).not.toBeNull();
  await star.click();
  await expect(upNext).toHaveJSProperty('checked', false);
  await expect(row).toHaveAttribute('data-selected', 'true');
  await expect(star).not.toHaveClass(/active/);
  await star.focus();
  await page.keyboard.press('Enter');
  await expect(upNext).toHaveJSProperty('checked', true);
  await expect(star).toHaveClass(/active/);

  await row.click({ button: 'right' });
  const menu = page.getByRole('menu', { name: 'Ticket actions' });
  await expect(menu).toBeVisible();
  const menuItems = menu.locator('wa-dropdown-item');
  await expect(menu.locator(':scope > wa-dropdown > wa-dropdown-item')).toHaveCount(11);
  await expect(menuItems).toHaveCount(27);
  expect(
    await menuItems.evaluateAll((items) => items.every((item) => item.querySelector('[data-lucide]') !== null)),
  ).toBe(true);
  await expect(row).toHaveAttribute('data-selected', 'true');
  await menu.getByText('Toggle Up Next', { exact: true }).click();
  await expect(star).not.toHaveClass(/active/);
  await expect(upNext).toHaveJSProperty('checked', false);
  await expect(page.getByText('Toggle Up Next selected')).toBeVisible();

  await title.fill(
    'A deliberately long ticket title that must wrap across no more than two lines while the AI working indicator remains entirely visible',
  );
  await priority.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'default';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(row.locator('[data-lucide="minus"]')).toHaveCount(1);
  await tags.fill('client, regression, server, ux');
  await row.evaluate((node: HTMLElement) => {
    node.style.width = '320px';
  });
  const [rowBox, timeBox, slugBox, identityBox, titleLineBoxes] = await Promise.all([
    row.boundingBox(),
    row.locator('.ticket-list-row__updated').boundingBox(),
    row.locator('.ticket-list-row__slug').boundingBox(),
    row.locator('.ticket-list-row__identity').boundingBox(),
    row
      .locator('.ticket-list-row__identity strong')
      .evaluate((node) =>
        [...node.getClientRects()].map((rect) => ({ x: rect.x, y: rect.y, width: rect.width, height: rect.height })),
      ),
  ]);
  expect(rowBox).not.toBeNull();
  expect(timeBox).not.toBeNull();
  expect(slugBox).not.toBeNull();
  expect(identityBox).not.toBeNull();
  expect(timeBox!.x + timeBox!.width).toBeLessThanOrEqual(rowBox!.x + rowBox!.width);
  expect(Math.abs(timeBox!.y + timeBox!.height - (slugBox!.y + slugBox!.height))).toBeLessThanOrEqual(3);
  expect(identityBox!.height).toBeLessThanOrEqual(43);
  await expect(row.locator('.ticket-list-row__identity')).toHaveCSS('display', 'block');
  await expect(row.locator('.ticket-list-row__updated')).toHaveCSS('float', 'right');
  await expect(row.locator('.ticket-list-row__slug')).toHaveCSS('display', 'inline-block');
  await expect(row.locator('.ticket-list-row__priority')).toHaveCSS('display', 'inline');
  expect(
    await row
      .locator('.ticket-list-row__identity > *')
      .evaluateAll((elements) => elements.map((element) => element.className || element.tagName.toLowerCase())),
  ).toEqual(['ticket-list-row__updated', 'ticket-list-row__slug', 'ticket-list-row__priority', 'strong']);
  expect(titleLineBoxes.length).toBeGreaterThan(1);
  for (const line of titleLineBoxes) {
    expect(line.x + line.width).toBeLessThanOrEqual(rowBox!.x + rowBox!.width);
    if (line.y < timeBox!.y + timeBox!.height && line.y + line.height > timeBox!.y) {
      expect(line.x + line.width).toBeLessThanOrEqual(timeBox!.x);
    }
  }
  await expect(row.locator('[data-component="tag-chip"]')).toHaveCount(4);
  for (const chip of await row.locator('[data-component="tag-chip"]').all()) await expect(chip).toBeVisible();
});

test('presents note kinds and round-trips reader and Markdown editor compositions', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=note-card');
  const notes = page.locator('[data-component="note-card"]');
  // Eight comfortable cards plus the compact-density pair that TicketNotes uses (HS2-7RY5GK).
  await expect(notes).toHaveCount(10);
  for (const id of ['compact-regular', 'compact-activity']) {
    const compact = page.locator(`[data-component="note-card"][data-note-id="${id}"]`);
    await expect(compact).toHaveAttribute('data-density', 'compact');
    await expect(compact).toHaveCSS('padding', '11.2px 8px');
    await expect(compact).toHaveCSS('border-radius', '9.6px');
  }
  const noteById = (id: string) => page.locator(`[data-component="note-card"][data-note-id="${id}"]`);
  for (const [id, kind, icon] of [
    ['regular', 'regular', 'message-square-text'],
    ['status', 'status', 'refresh-cw'],
    ['feedback', 'feedback_needed', 'circle-alert'],
    ['draft', 'feedback_draft', 'file-pen-line'],
    ['activity', 'activity', 'activity'],
  ] as const) {
    const note = noteById(id);
    await expect(note).toHaveAttribute('data-kind', kind);
    await expect(note.locator(`[data-lucide="${icon}"]`)).toBeVisible();
  }
  await expect(noteById('status')).toHaveAttribute('data-ai-authored', 'true');
  await expect(noteById('status')).not.toHaveAttribute('aria-label', /AI-generated/);
  await expect(noteById('status').locator('[data-component="ai-content-label"]')).toHaveCount(0);
  await expect(noteById('activity')).toHaveAccessibleName('AI-generated activity by Codex; may contain errors');
  await expect(noteById('mixed-activity')).toHaveAccessibleName('AI-generated activity by Codex; may contain errors');
  // The demo exposes every confidence band (HS2-DWTJ43); unscored kinds carry no badge.
  for (const [id, value, band] of [
    ['regular', 82, 'assumed'],
    ['activity', 96, 'verified'],
    ['scored-partial', 55, 'partial'],
    ['scored-low', 32, 'unverified'],
  ] as const) {
    const badge = noteById(id).locator('.note-card__header-end [data-component="confidence-badge"]');
    await expect(badge).toHaveAttribute('data-band', band);
    await expect(badge).toHaveAccessibleName(`Confidence ${value} percent`);
    await expect(badge.locator('[data-lucide="gauge"]')).toBeVisible();
  }
  for (const id of ['status', 'feedback', 'draft'])
    await expect(noteById(id).locator('[data-component="confidence-badge"]')).toHaveCount(0);
  const standaloneNote = noteById('regular');
  const feedbackNote = noteById('feedback'),
    activityNote = noteById('activity');
  expect(
    await standaloneNote.evaluate((node) => {
      const card = getComputedStyle(node),
        header = getComputedStyle(node.querySelector('.note-card__header')!),
        kind = getComputedStyle(node.querySelector('.note-card__kind')!),
        headerEnd = getComputedStyle(node.querySelector('.note-card__header-end')!);
      return {
        padding: card.padding,
        gap: card.rowGap,
        headerGap: header.gap,
        kindGap: kind.gap,
        headerEndGap: headerEnd.gap,
      };
    }),
  ).toEqual({ padding: '16px', gap: '8px', headerGap: '16px', kindGap: '4px', headerEndGap: '4px' });
  expect(
    await activityNote.evaluate((node) => {
      const style = getComputedStyle(node);
      return { padding: style.padding, gap: style.rowGap };
    }),
  ).toEqual({ padding: '8px 16px', gap: '4px' });
  await feedbackNote.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-note-card-feedback-wide.png' });
  await activityNote.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-note-card-activity-wide.png' });
  await page.setViewportSize({ width: 430, height: 760 });
  expect(await notes.evaluateAll((items) => items.every((item) => item.scrollWidth <= item.clientWidth + 1))).toBe(
    true,
  );
  await feedbackNote.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-note-card-feedback-narrow.png' });
  await activityNote.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-note-card-activity-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await standaloneNote.dblclick();
  const standaloneEditor = standaloneNote.getByRole('textbox', { name: 'Note body' });
  await standaloneEditor.fill('Persisted standalone note');
  await standaloneEditor.blur();
  await expect(standaloneNote).toContainText('Persisted standalone note');
  await standaloneNote.dblclick();
  await standaloneNote.getByRole('textbox', { name: 'Note body' }).fill('Autosaved replacement');
  await standaloneNote.getByRole('textbox', { name: 'Note body' }).blur();
  await expect(standaloneNote).toContainText('Autosaved replacement');

  await page.goto('/ux-demo?component=ticket-reader');
  const reader = page.locator('[data-component="ticket-reader"]');
  await expect(reader.getByRole('heading', { name: 'Build TicketReader component and UX demo' })).toBeVisible();
  await expect(reader.locator('.ticket-info-panel__details-surface [data-component="markdown-preview"]')).toContainText(
    'Implementation notes',
  );
  const readerGuide = reader.getByRole('link', { name: 'Open the component guide' });
  await expect(readerGuide).toHaveAttribute('target', '_blank');
  await expect(readerGuide).toHaveAttribute('rel', 'noopener noreferrer');
  const popupPromise = page.waitForEvent('popup');
  await readerGuide.click();
  const popup = await popupPromise;
  await expect(popup).toHaveURL(/component=tag-chip/);
  await popup.close();
  await expect(reader.locator('[data-component="note-card"]')).toHaveCount(5);
  const noteHistory = reader.getByRole('link', { name: 'note history' });
  await expect(noteHistory).toHaveAttribute('target', '_blank');
  await expect(noteHistory).toHaveAttribute('rel', 'noopener noreferrer');
  await reader.screenshot({ path: 'target/visual-captures/hs2-hnh0m6-markdown-links-wide.png' });
  await page.setViewportSize({ width: 940, height: 844 });
  await noteHistory.scrollIntoViewIfNeeded();
  await reader.screenshot({ path: 'target/visual-captures/hs2-hnh0m6-markdown-links-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 720 });
  await expect(reader.getByRole('heading', { name: 'Notes, 5 notes' })).toBeVisible();
  await expect(reader.locator('.ticket-inspector-panel')).toHaveCSS('overflow-y', 'auto');
  const readerWidth = await reader.boundingBox();
  const readerContentWidth = await reader.locator('.ticket-inspector-panel').boundingBox();
  expect(readerContentWidth!.width).toBeGreaterThan(readerWidth!.width * 0.9);
  const editableNote = reader.locator('[data-component="note-card"][data-note-id="reader-note"]');
  await expect(editableNote.locator('.note-card__body')).toHaveAttribute('aria-label', 'Edit note');
  await reader.getByRole('button', { name: 'Edit Ticket details' }).getByRole('heading').first().click();
  const readerDetails = reader.getByRole('textbox', { name: 'Ticket details' });
  await expect(readerDetails).toBeFocused();
  await expect(readerDetails).toHaveCSS('resize', 'vertical');
  await editableNote.locator('.note-card__body').click({ position: { x: 4, y: 4 } });
  await expect(editableNote.getByRole('textbox', { name: 'Note body' })).toBeFocused();
  await expect(editableNote.getByRole('textbox', { name: 'Note body' })).toHaveCSS('resize', 'vertical');
  await editableNote.getByRole('textbox', { name: 'Note body' }).fill('Edited note body');
  await editableNote.getByRole('textbox', { name: 'Note body' }).blur();
  await expect(editableNote).toContainText('Edited note body');
  await editableNote.dblclick();
  await editableNote.getByRole('textbox', { name: 'Note body' }).fill('Autosaved note body');
  await editableNote.getByRole('textbox', { name: 'Note body' }).blur();
  await expect(editableNote.getByRole('textbox', { name: 'Note body' })).toHaveCount(0);
  await expect(editableNote).toContainText('Autosaved note body');
  await reader.getByRole('tab', { name: /Attachments/ }).click();
  await expect(reader.locator('[data-component="ticket-attachments"]')).toContainText('reader-wireframe.png');
  await reader
    .getByLabel('Browse and add attachments')
    .setInputFiles({ name: 'browser-added.txt', mimeType: 'text/plain', buffer: Buffer.from('added') });
  await expect(reader.locator('[data-component="ticket-attachments"]')).toContainText('browser-added.txt');
  await reader.locator('[data-component="ticket-inspector"]').evaluate((node) => {
    const transfer = new DataTransfer();
    transfer.items.add(new File(['drop'], 'dropped.txt', { type: 'text/plain' }));
    node.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
  });
  await expect(reader.locator('[data-component="ticket-attachments"]')).toContainText('dropped.txt');
  await reader.getByRole('tab', { name: 'Info' }).click();
  await expect(reader.locator('.ticket-info-panel__details-section')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(reader.locator('.ticket-info-panel__details-surface')).toHaveCSS(
    'background-color',
    'rgb(255, 255, 255)',
  );
  await expect(reader.locator('.ticket-info-panel__details-surface .markdown-editor--embedded')).toHaveCSS(
    'background-color',
    'rgba(0, 0, 0, 0)',
  );
  await expect(reader.getByRole('textbox', { name: 'Feedback response' })).toBeVisible();
  await expect(reader.getByRole('textbox', { name: 'Note body' })).toHaveValue(/keep the response/i);
  await reader.getByRole('button', { name: 'Edit Ticket details' }).getByRole('heading').first().click();
  const readerSource = reader.getByRole('textbox', { name: 'Ticket details' });
  await readerSource.fill('## Reader draft\nPreserved across the shared inspector surface.');
  await readerSource.blur();
  await expect(reader.locator('.ticket-info-panel__details-surface [data-component="markdown-preview"]')).toContainText(
    'Reader draft',
  );

  await page.goto('/ux-demo?component=markdown-editor');

  const editor = page.locator('[data-component="markdown-editor"]');
  await expect(editor.locator('[data-component="markdown-preview"]')).toContainText('Implementation notes');
  await expect(editor.getByRole('link', { name: 'Open the component guide' })).toHaveAttribute('target', '_blank');
  // A link inside the rendered Markdown follows the link and never starts editing (HS2-H1K9YY).
  const guide = page.waitForEvent('popup');
  await editor.getByRole('link', { name: 'Open the component guide' }).click();
  await (await guide).close();
  await expect(editor).toHaveAttribute('data-mode', 'preview');
  // A single click on the rendered content enters the editor.
  await expect(editor.getByRole('button', { name: 'Edit Markdown content' })).toHaveAttribute('title', 'Click to edit');
  await editor.getByRole('heading', { name: 'Implementation notes' }).click();
  const source = editor.getByRole('textbox', { name: 'Markdown content' });
  await expect(source).toHaveValue(/Implementation notes/);
  await source.fill('## Revised goal\nA preserved draft.');
  await expect(editor.locator('footer')).toHaveCount(0);
  await editor.getByRole('button', { name: 'Expand editor' }).click();
  await expect(editor).toHaveAttribute('data-expanded', 'true');
  await expect(editor).toHaveCSS('position', 'fixed');
  await editor.getByRole('textbox', { name: 'Markdown content' }).focus();
  await editor.getByRole('textbox', { name: 'Markdown content' }).blur();
  await expect(editor).toHaveAttribute('data-mode', 'preview');
  await expect(editor.locator('[data-component="markdown-preview"]')).toContainText('Revised goal');
  await editor.getByRole('button', { name: 'Edit Markdown content' }).dblclick();
  await editor.getByRole('textbox', { name: 'Markdown content' }).fill('Autosaved edit');
  await editor.getByRole('textbox', { name: 'Markdown content' }).blur();
  await expect(editor).toHaveAttribute('data-mode', 'preview');
  await expect(editor.locator('[data-component="markdown-preview"]')).toContainText('Autosaved edit');
  await editor.getByRole('button', { name: 'Use inline editor' }).click();
  await expect(editor).toHaveAttribute('data-expanded', 'false');
});

test('switches the MarkdownEditor demo appearance and inset, then resets them (HS2-QBR5HC)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=markdown-editor&dev-review=false');
    const editor = page.locator('[data-component="markdown-editor"]'),
      settings = page.locator('[data-settings="markdown-editor"]'),
      appearance = settings.locator('[name="markdown-appearance"]'),
      inset = settings.locator('[name="markdown-inset"]'),
      preview = editor.locator('.markdown-editor__preview'),
      choose = (control: typeof appearance, value: string) =>
        control.evaluate((node: HTMLElement & { value: string }, next) => {
          node.value = next;
          node.dispatchEvent(new Event('change', { bubbles: true }));
        }, value);
    await expect(editor).toHaveAttribute('data-appearance', 'standalone');
    await expect(editor).toHaveAttribute('data-inset', 'padded');
    await expect(editor.locator('.markdown-editor__toolbar')).toBeVisible();

    // The Settings button is reachable, and the live controls start at the defaults.
    await page.locator('[data-action="toggle-settings"]').click();
    await expect(appearance).toHaveJSProperty('value', 'standalone');
    await expect(inset).toHaveJSProperty('value', 'padded');

    // Control -> render: embedded drops the frame and the toolbar label; flush drops the preview inset.
    await choose(appearance, 'embedded');
    await expect(editor).toHaveAttribute('data-appearance', 'embedded');
    await expect(editor).toHaveClass(/markdown-editor--embedded/);
    await expect(editor).toHaveCSS('border-top-width', '0px');
    await expect(editor.locator('.markdown-editor__surface')).toHaveCSS('padding-top', '0px');
    await expect(preview).toHaveCSS('padding-top', '12px');
    await choose(inset, 'flush');
    await expect(editor).toHaveAttribute('data-inset', 'flush');
    await expect(editor).toHaveClass(/markdown-editor--flush/);
    await expect(preview).toHaveCSS('padding-top', '0px');
    await expect(preview).toHaveCSS('padding-left', '0px');
    await page.screenshot({ path: `target/visual-captures/claude/hs2-qbr5hc-embedded-flush-${width}.png` });
    // Closing and reopening the settings keeps the live controls on the chosen variants.
    await page.locator('.settings-inspector [data-action="toggle-settings"]').click();
    await editor.screenshot({ path: `target/visual-captures/claude/hs2-qbr5hc-embedded-flush-editor-${width}.png` });
    await page.locator('[data-action="toggle-settings"]').click();
    await expect(appearance).toHaveJSProperty('value', 'embedded');
    await expect(inset).toHaveJSProperty('value', 'flush');

    // Reset -> state, render, and the live controls all return to the defaults.
    await settings.locator('[data-action="reset-settings"]').click();
    await expect(editor).toHaveAttribute('data-appearance', 'standalone');
    await expect(editor).toHaveAttribute('data-inset', 'padded');
    await expect(editor).not.toHaveClass(/markdown-editor--(embedded|flush)/);
    await expect(editor.locator('.markdown-editor__surface')).toHaveCSS('padding-top', '16px');
    await expect(editor).not.toHaveCSS('border-top-width', '0px');
    await expect(appearance).toHaveJSProperty('value', 'standalone');
    await expect(inset).toHaveJSProperty('value', 'padded');
    await page.screenshot({ path: `target/visual-captures/claude/hs2-qbr5hc-standalone-padded-${width}.png` });

    // Another edit after reset still drives the render, with the reset padded inset kept.
    await choose(appearance, 'embedded');
    await expect(editor).toHaveAttribute('data-appearance', 'embedded');
    await expect(editor).toHaveAttribute('data-inset', 'padded');
    await expect(preview).toHaveCSS('padding-top', '12px');
  }
});

test('catalogs ProviderIcon kinds at the m and l size variants (HS2-PK8THJ)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=provider-icon&dev-review=false');
    const demo = page.locator('.provider-icon-demo');
    for (const size of ['m', 'l'] as const) {
      const variant = demo.locator(`[data-size-variant="${size}"]`);
      await expect(variant.locator('figcaption')).toContainText(`size="${size}"`);
      for (const [kind, label] of [
        ['github', 'GitHub'],
        ['gitlab', 'GitLab'],
        ['jira', 'Jira'],
      ] as const) {
        const icon = variant.getByRole('img', { name: label });
        await expect(icon).toHaveAttribute('data-provider-icon', kind);
        const { box, fontSize } = await icon.evaluate((node) => ({
          box: node.getBoundingClientRect().toJSON() as DOMRect,
          fontSize: parseFloat(getComputedStyle(node.parentElement!).fontSize),
        }));
        // m follows the surrounding font size (1em); l is the fixed 24px identity mark.
        const expected = size === 'm' ? fontSize : 24;
        expect(box.width).toBeCloseTo(expected, 1);
        expect(box.height).toBeCloseTo(expected, 1);
        expect(box.x).toBeGreaterThanOrEqual(0);
        expect(box.x + box.width).toBeLessThanOrEqual(width);
        if (size === 'm') await expect(icon).not.toHaveAttribute('data-size');
        else await expect(icon).toHaveAttribute('data-size', 'l');
      }
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    await demo.screenshot({ path: `target/visual-captures/claude/hs2-pk8thj-provider-icon-${width}.png` });
  }
});

test('previews every BulkTicketDialog presentation through demo settings, then resets (HS2-PS9BQV)', async ({
  page,
}) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=bulk-ticket-dialog&dev-review=false');
    const tagDialog = page.locator('[data-component="bulk-tag-dialog"]'),
      deleteDialog = page.locator('[data-component="bulk-delete-dialog"]'),
      trashDialog = page.locator('[data-component="empty-trash-dialog"]'),
      event = page.locator('.component-stage__event'),
      scenario = page.locator('[data-settings="bulk-ticket-dialog"] [name="bulk-scenario"]'),
      choose = (value: string) =>
        scenario.evaluate((node: HTMLElement & { value: string }, next) => {
          node.value = next;
          node.dispatchEvent(new Event('change', { bubbles: true }));
        }, value),
      shot = (name: string) =>
        page.screenshot({ path: `target/visual-captures/claude/hs2-ps9bqv-bulk-${name}-${width}.png` });

    // Default add-tag mode, then Cancel closes it so the demo settings are reachable.
    await expect(tagDialog).toHaveJSProperty('open', true);
    await expect(tagDialog.locator('form')).toHaveAttribute('data-tag-mode', 'add');
    await expect(tagDialog.locator('[data-action="choose-bulk-tag"]')).toHaveCount(0);
    await tagDialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(tagDialog).toHaveCount(0);
    await expect(event).toHaveText('Cancelled; the selection is unchanged.');
    await page.locator('[data-action="toggle-settings"]').click();
    await expect(scenario).toHaveJSProperty('value', 'add-tag');

    // Remove mode offers the selection's tags as choices; a choice fills the field and submit reports it.
    await choose('remove-tag');
    await expect(tagDialog).toHaveJSProperty('open', true);
    await expect(tagDialog).toHaveAttribute('label', 'Remove tag — 5 selected');
    await expect(tagDialog.locator('form')).toHaveAttribute('data-tag-mode', 'remove');
    await expect(tagDialog.locator('[data-action="choose-bulk-tag"]')).toHaveText(['bug', 'ui', 'backend', 'docs']);
    await shot('remove-tag');
    await tagDialog.getByRole('button', { name: 'backend' }).click();
    await expect(tagDialog.locator('wa-input[name="bulk-ticket-tag"]')).toHaveJSProperty('value', 'backend');
    await tagDialog.getByRole('button', { name: 'Remove tag' }).click();
    await expect(tagDialog).toHaveCount(0);
    await expect(event).toHaveText('Remove tag “backend” from 5 tickets requested.');

    await choose('delete');
    await expect(deleteDialog).toHaveJSProperty('open', true);
    await expect(deleteDialog).toHaveAttribute('label', 'Delete 5 tickets?');
    await shot('delete');
    await deleteDialog.getByRole('button', { name: 'Delete 5 tickets' }).click();
    await expect(deleteDialog).toHaveCount(0);
    await expect(event).toHaveText('Delete 5 tickets requested.');

    await choose('empty-trash');
    await expect(trashDialog).toHaveJSProperty('open', true);
    await expect(trashDialog.getByRole('alert')).toHaveCount(0);
    await shot('empty-trash');
    await trashDialog.getByRole('button', { name: 'Empty Trash' }).click();
    await expect(trashDialog).toHaveCount(0);
    await expect(event).toHaveText('Empty Trash requested for 5 tickets.');

    await choose('empty-trash-busy');
    await expect(trashDialog.getByRole('button', { name: 'Emptying…' })).toHaveJSProperty('disabled', true);
    await expect(trashDialog.getByRole('button', { name: 'Cancel' })).toHaveJSProperty('disabled', true);
    await shot('empty-trash-busy');

    await choose('empty-trash-error');
    await expect(trashDialog.getByRole('alert')).toContainText('Could not empty Trash');
    await expect(trashDialog.getByRole('button', { name: 'Empty Trash' })).toHaveJSProperty('disabled', false);
    await shot('empty-trash-error');
    await page.keyboard.press('Escape');
    await expect(trashDialog).toHaveCount(0);
    await expect(event).toHaveText('Dismissed; the selection is unchanged.');
    await expect(scenario).toHaveJSProperty('value', 'empty-trash-error');

    // Reset restores add-tag in the dialog and the live control, then another edit still applies.
    await page.locator('[data-settings="bulk-ticket-dialog"] [data-action="reset-settings"]').click();
    await expect(tagDialog).toHaveJSProperty('open', true);
    await expect(tagDialog.locator('form')).toHaveAttribute('data-tag-mode', 'add');
    await expect(scenario).toHaveJSProperty('value', 'add-tag');
    await choose('delete');
    await expect(deleteDialog).toHaveJSProperty('open', true);
    await expect(tagDialog).toHaveCount(0);
  }
});

test('switches the SettingsWorkspace demo across every category, then resets (HS2-PS9BQV)', async ({ page }) => {
  const categories = [
    ['sources', 'Ticket sources'],
    ['ai', 'AI tools'],
    ['commands', 'Commands'],
    ['lifecycle', 'Lifecycle'],
    ['terminals', 'Terminals'],
    ['permissions', 'Permissions'],
    ['columns', 'Column view'],
    ['general', 'General'],
    ['accounts', 'Accounts'],
    ['keyboard', 'Keyboard shortcuts'],
  ] as const;
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=settings-workspace&dev-review=false');
    const workspace = page.locator('[data-component="settings-workspace"]'),
      settings = page.locator('[data-settings="settings-workspace"]'),
      category = settings.locator('[name="workspace-category"]'),
      action = settings.locator('[name="workspace-permission-action"]'),
      delay = workspace.locator('[name="permission-automation-delay"]'),
      choose = (control: typeof category, value: string) =>
        control.evaluate((node: HTMLElement & { value: string }, next) => {
          node.value = next;
          node.dispatchEvent(new Event('change', { bubbles: true }));
        }, value),
      shot = async (name: string, ...visible: Locator[]) => {
        // The phone-width settings inspector takes the whole viewport, so check and capture the workspace
        // with the inspector closed, then reopen it.
        await page.locator('.settings-inspector [data-action="toggle-settings"]').click();
        for (const locator of visible) await expect(locator).toBeVisible();
        await workspace.screenshot({ path: `target/visual-captures/claude/hs2-ps9bqv-workspace-${name}-${width}.png` });
        await page.locator('[data-action="toggle-settings"]').click();
      };
    await expect(workspace).toHaveAttribute('data-settings-category', 'sources');
    await page.locator('[data-action="toggle-settings"]').click();
    await expect(category).toHaveJSProperty('value', 'sources');
    await expect(action).toHaveJSProperty('value', 'off');

    for (const [id, title] of categories) {
      await choose(category, id);
      await expect(workspace).toHaveAttribute('data-settings-category', id);
      await expect(workspace).toHaveAttribute('aria-label', `${title} settings`);
    }

    await choose(category, 'terminals');
    await shot('terminals', workspace.getByRole('checkbox', { name: /global shell history/ }));
    await expect(category).toHaveJSProperty('value', 'terminals');

    await choose(category, 'permissions');
    await expect(workspace.locator('.project-settings__permission-note')).toContainText('floating permission popup');
    await expect(workspace.locator('[name="permission-automation-action"]')).toHaveJSProperty('value', 'off');
    await expect(delay).toHaveJSProperty('disabled', true);
    await shot('permissions-off', workspace.locator('.project-settings__permission-grid'));
    await choose(action, 'allow');
    await expect(workspace.locator('[name="permission-automation-action"]')).toHaveJSProperty('value', 'allow');
    await expect(delay).toHaveJSProperty('disabled', false);
    await shot('permissions-allow');

    await choose(category, 'columns');
    await shot('columns', workspace.getByRole('checkbox', { name: /Hide Verified column/ }));
    await choose(category, 'general');
    await shot('general', workspace.getByRole('checkbox', { name: /Show loading activity/ }));

    // Reset restores the sources category and Off automation in the render and both live controls.
    await settings.locator('[data-action="reset-settings"]').click();
    await expect(workspace).toHaveAttribute('data-settings-category', 'sources');
    await expect(category).toHaveJSProperty('value', 'sources');
    await expect(action).toHaveJSProperty('value', 'off');
    await choose(category, 'permissions');
    await expect(workspace.locator('[name="permission-automation-action"]')).toHaveJSProperty('value', 'off');
    await expect(delay).toHaveJSProperty('disabled', true);
  }
});

test('lets the TicketReader native dialog complete dismissal before leaving the demo', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-reader');
  const reader = page.getByRole('dialog', { name: 'Read and edit HS2-H892P1' });
  await expect(reader).toBeVisible();
  await expect(reader.locator('dialog:modal')).toHaveCount(1);
  await reader.getByRole('button', { name: 'Close ticket reader' }).click();
  await expect(page).toHaveURL('/ux-demo?component=ticket-info-panel');
  await expect(reader).toHaveCount(0);
});

test('exposes every MarkdownPreview and AIContentLabel presentation variant', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=markdown-preview&dev-review=false');
    const variant = (label: string) =>
      page
        .locator('.markdown-preview-demo__variant')
        .filter({ has: page.locator('figcaption', { hasText: label }) })
        .locator('[data-component="markdown-preview"]');
    await expect(page.locator('.markdown-preview-demo [data-component="markdown-preview"]')).toHaveCount(10);
    const style = (label: string, selector: string, property: string) =>
      variant(label)
        .locator(selector)
        .first()
        .evaluate((node, name) => getComputedStyle(node).getPropertyValue(name), property);
    const surfaceColor = (label: string) =>
      variant(label).evaluate((node) => getComputedStyle(node.parentElement!).color);
    // Tone: inherit takes the container color but keeps links; inverse carries the container color into links.
    expect(await style('tone="inherit"', ':scope', 'color')).toBe(await surfaceColor('tone="inherit"'));
    expect(await style('tone="inherit"', 'a', 'color')).not.toBe(await surfaceColor('tone="inherit"'));
    expect(await style('tone="inverse"', 'a', 'color')).toBe(await surfaceColor('tone="inverse"'));
    expect(await style('tone="inverse"', 'a', 'text-decoration-color')).toBe(await surfaceColor('tone="inverse"'));
    // Size: small is the 12px secondary scale; inherit takes the container font.
    expect(await style('size="small"', ':scope', 'font-size')).toBe('12px');
    expect(await style('size="inherit"', ':scope', 'font-size')).toBe(
      await variant('size="inherit"').evaluate((node) => getComputedStyle(node.parentElement!).fontSize),
    );
    // Density: block rhythm is 16px by default, 4px compact, 0 flush.
    expect(await style('Default', 'ul', 'margin-top')).toBe('16px');
    expect(await style('density="compact"', 'ul', 'margin-top')).toBe('4px');
    expect(await style('density="flush"', 'ul', 'margin-top')).toBe('0px');
    // Media: the thumbnail crops attachment images into a bounded box.
    const thumbnail = await variant('media="thumbnail"').locator('.markdown-preview__attachment-image').boundingBox(),
      full = await variant('media="full"').locator('.markdown-preview__attachment-image').boundingBox();
    expect(thumbnail!.width).toBeLessThanOrEqual(192);
    expect(thumbnail!.height).toBeLessThanOrEqual(112);
    expect(full!.width).toBeGreaterThan(thumbnail!.width);
    await expect(
      page.locator('.markdown-preview-demo [data-component="markdown-preview"].markdown-preview--empty'),
    ).toHaveText('Nothing to preview.');
  }
  await page.goto('/ux-demo?component=ai-content-label&dev-review=false');
  const labels = page.locator('[data-component="ai-content-label"]');
  await expect(labels).toHaveCount(4);
  const inherit = page.locator('[data-component="ai-content-label"][data-tone="inherit"]');
  expect(await inherit.evaluate((node) => getComputedStyle(node).color)).toBe(
    await inherit.evaluate((node) => getComputedStyle(node.parentElement!).color),
  );
  expect(await labels.first().evaluate((node) => getComputedStyle(node).color)).not.toBe(
    await inherit.evaluate((node) => getComputedStyle(node).color),
  );
  await expect(labels.nth(1)).toHaveAccessibleName('AI-generated; may contain errors');
  await expect(labels.nth(2).getByRole('button', { name: /Helpful/ })).toBeVisible();
});

/** WCAG contrast of a code chip's text against its fill composited over the opaque container behind it. */
async function codeChipContrast(code: Locator) {
  return code.evaluate((node) => {
    const parse = (value: string) => {
      const numbers = (value.match(/[\d.]+/g) ?? []).map(Number);
      // `color(srgb r g b / a)` uses 0-1 channels; `rgb()`/`rgba()` use 0-255.
      const scale = value.startsWith('color(') ? 1 : 255;
      return {
        rgb: numbers.slice(0, 3).map((channel) => channel / scale),
        alpha: numbers.length > 3 ? numbers[3] : 1,
      };
    };
    let container = node.parentElement!;
    while (parse(getComputedStyle(container).backgroundColor).alpha < 1) container = container.parentElement!;
    const base = parse(getComputedStyle(container).backgroundColor).rgb,
      chip = parse(getComputedStyle(node).backgroundColor),
      text = parse(getComputedStyle(node).color).rgb,
      fill = base.map((channel, index) => chip.rgb[index] * chip.alpha + channel * (1 - chip.alpha)),
      luminance = (rgb: number[]) => {
        const [r, g, b] = rgb.map((channel) =>
          channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4,
        );
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
      },
      [light, dark] = [luminance(text), luminance(fill)].sort((a, b) => b - a);
    return (light + 0.05) / (dark + 0.05);
  });
}

test('keeps inverse inline code legible on the brand fill in the demo and the AI user bubble (HS2-WS438X)', async ({
  page,
}) => {
  for (const colorScheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme });
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto('/ux-demo?component=markdown-preview&dev-review=false');
      const variant = (label: string) =>
        page
          .locator('.markdown-preview-demo__variant')
          .filter({ has: page.locator('figcaption', { hasText: label }) })
          .locator('[data-component="markdown-preview"]');
      const inverseCode = variant('tone="inverse"').locator('code').first();
      await expect(inverseCode).toHaveText('code');
      // The chip takes the container's text color and reads at AA contrast on its own fill.
      expect(await inverseCode.evaluate((node) => getComputedStyle(node).color)).toBe(
        await variant('tone="inverse"').evaluate((node) => getComputedStyle(node.parentElement!).color),
      );
      expect(await codeChipContrast(inverseCode)).toBeGreaterThanOrEqual(4.5);
      // The default tone keeps its neutral chip.
      expect(await codeChipContrast(variant('Default').locator('code').first())).toBeGreaterThanOrEqual(4.5);

      await page.goto('/ux-demo?component=ai-conversation&dev-review=false');
      const userCode = page.locator('[data-component="markdown-preview"][data-tone="inverse"] code').first();
      await expect(userCode).toHaveText('api.ts');
      expect(await codeChipContrast(userCode)).toBeGreaterThanOrEqual(4.5);
    }
  }
});

test('keeps feedback Markdown list spacing compact', async ({ page }) => {
  await page.goto('/ux-demo?component=note-card');
  const feedbackNote = page.locator('[data-component="note-card"][data-kind="feedback_needed"]');
  const feedbackItems = feedbackNote.locator('li');
  await expect(feedbackItems).toHaveCount(3);
  const itemGaps = await feedbackItems.evaluateAll((items) =>
    items.slice(1).map((item, index) => item.getBoundingClientRect().top - items[index].getBoundingClientRect().bottom),
  );
  expect(Math.max(...itemGaps)).toBeLessThanOrEqual(8);
  await feedbackNote.screenshot({ path: 'target/visual-captures/hs2-8dd2dg-feedback-list-spacing.png' });
});

test('spaces paragraphs and de-emphasizes email-style quoted Markdown at wide and narrow sizes', async ({ page }) => {
  await page.goto('/ux-demo?component=markdown-editor');
  const preview = page.locator('[data-component="markdown-preview"]'),
    quote = preview.locator('blockquote'),
    body = preview.locator(':scope > p').first();
  await expect(quote).toBeVisible();
  const typography = await Promise.all(
    [quote, body].map((locator) =>
      locator.evaluate((node) => ({
        fontSize: parseFloat(getComputedStyle(node).fontSize),
        lineHeight: parseFloat(getComputedStyle(node).lineHeight),
        marginLeft: getComputedStyle(node).marginLeft,
      })),
    ),
  );
  expect(typography[0].fontSize).toBeLessThan(typography[1].fontSize);
  expect(typography[0].lineHeight).toBeLessThan(typography[1].lineHeight);
  expect(typography[0].marginLeft).toBe('0px');
  await expect(body).toHaveCSS('margin-top', '16px');
  await expect(body).toHaveCSS('margin-bottom', '16px');
  await expect(quote).toHaveCSS('padding', '4px 0px 4px 8px');
  await preview.screenshot({ path: 'target/visual-captures/hs2-9acety-quoted-content-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await quote.scrollIntoViewIfNeeded();
  await preview.screenshot({ path: 'target/visual-captures/hs2-9acety-quoted-content-narrow.png' });
});

test('uses the identical responsive TicketRow in list and board compositions', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto('/ux-demo?component=ticket-list');
  const list = page.getByRole('listbox', { name: 'Example ticket list' });
  const listRows = list.locator('[data-component="ticket-list-row"]');
  await expect(listRows).toHaveCount(20);
  const emptyExamples = page.getByLabel('TicketList empty states');
  await expect(emptyExamples.getByText('No tickets yet')).toBeVisible();
  await expect(emptyExamples.getByText('No tickets in Backlog')).toBeVisible();
  await expect(emptyExamples.getByText('No tickets match “parser”')).toBeVisible();
  await expect(list.locator('..')).toHaveCSS('border-radius', '10.4px');
  const listRow = listRows.first();
  await expect(listRow).toHaveAttribute('data-presentation', 'list');
  const listIdentity = listRow.locator('.ticket-list-row__identity');
  const listTitleMetrics = await listIdentity.evaluate((node) => {
    const style = getComputedStyle(node);
    return { lineHeight: Number.parseFloat(style.lineHeight), maxHeight: Number.parseFloat(style.maxHeight) };
  });
  expect(listTitleMetrics.maxHeight / listTitleMetrics.lineHeight).toBeCloseTo(2, 1);
  const listWidth = await listRow.evaluate((node) => node.getBoundingClientRect().width);
  expect(listWidth).toBeGreaterThan(600);
  const [listBox, listHostBox] = await Promise.all([list.boundingBox(), list.locator('xpath=..').boundingBox()]);
  expect(listBox).not.toBeNull();
  expect(listHostBox).not.toBeNull();
  expect(listBox!.width).toBeCloseTo(listHostBox!.width, 0);
  await expect(listRows.first()).toHaveCSS('border-radius', '10.4px 10.4px 0px 0px');
  await expect(listRows.nth(1)).toHaveCSS('border-radius', '0px');
  await expect(listRows.last()).toHaveCSS('border-radius', '0px 0px 10.4px 10.4px');
  await list.evaluate((node) => {
    (node.parentElement as HTMLElement).style.width = '320px';
  });
  await expect(listRows.first()).toHaveCSS('border-radius', '10.4px 10.4px 0px 0px');
  await expect(listRows.nth(1)).toHaveCSS('border-radius', '0px');
  await expect(listRows.last()).toHaveCSS('border-radius', '0px 0px 10.4px 10.4px');
  await list.screenshot({ path: 'target/visual-captures/hs2-y4de25-narrow-list-edge-rounding.png' });
  await list.evaluate((node) => {
    (node.parentElement as HTMLElement).style.width = '';
  });
  await expect(listRow).toHaveCSS('box-shadow', 'none');
  await listRow.click();
  await expect(listRow).toHaveAttribute('data-selected', 'true');
  await expect(page.getByText('1 ticket selected')).toBeVisible();
  await listRows.nth(1).click({ modifiers: ['Meta'] });
  await expect(listRows.nth(0)).toHaveAttribute('data-selected', 'true');
  await expect(listRows.nth(1)).toHaveAttribute('data-selected', 'true');
  await expect(page.getByText('2 tickets selected')).toBeVisible();
  await listRows.nth(3).click({ modifiers: ['Shift'] });
  await expect(listRows.nth(1)).toHaveAttribute('data-selected', 'true');
  await expect(listRows.nth(2)).toHaveAttribute('data-selected', 'true');
  await expect(listRows.nth(3)).toHaveAttribute('data-selected', 'true');
  await expect(listRows.nth(0)).toHaveAttribute('data-selected', 'false');
  const listStar = listRow.locator('[data-action="toggle-row-up-next"]');
  await listStar.click();
  await expect(listStar).not.toHaveClass(/active/);
  await expect(page.getByText('HS2-R76MMW removed from Up Next')).toBeVisible();

  await page.locator('.kui-workbench__rail--left [data-item-id="ticket-board"]').click();
  await expect(page).toHaveURL('/ux-demo?component=ticket-board');
  const board = page.getByRole('listbox', { name: 'Example status board' });
  await expect(board.locator('.ticket-board-column')).toHaveCount(3);
  const emptyBoard = page.getByRole('listbox', { name: 'Empty search board' });
  await expect(emptyBoard.locator('[data-component="empty-state"]')).toHaveCount(1);
  await expect(emptyBoard.getByText('No tickets match “parser”')).toBeVisible();
  expect(
    await board
      .locator('.ticket-board-column__header')
      .evaluateAll((headers) => headers.map((header) => header.getBoundingClientRect().height)),
  ).toEqual([32, 32, 32]);
  await expect(board.locator('.ticket-board-column').first()).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(board.locator('.ticket-board-column').first()).toHaveCSS('padding', '0px');
  await expect(board.locator('.ticket-board-column__tickets').first()).toHaveCSS('padding', '0px 8px 16px');
  await expect(board).toHaveCSS('padding', '0px 8px');
  await expect(board.locator('.ticket-board__columns')).toHaveCSS('gap', '0px');
  await expect(board).toHaveCSS('border-top-width', '0px');
  await expect(board).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(board.getByLabel('6 tickets')).toHaveCount(1);
  await expect(board.getByLabel('7 tickets')).toHaveCount(2);
  const boardRows = board.locator('[data-component="ticket-list-row"]');
  await expect(boardRows).toHaveCount(20);
  const scrollRegions = board.locator('.ticket-board-column__tickets');
  await expect(scrollRegions).toHaveCount(3);
  const initialScroll = await scrollRegions.evaluateAll((regions) =>
    regions.map((region) => ({
      clientHeight: region.clientHeight,
      scrollHeight: region.scrollHeight,
      scrollTop: region.scrollTop,
    })),
  );
  expect(initialScroll.every((region) => region.scrollHeight > region.clientHeight)).toBe(true);
  const firstHeaderTop = await board
    .locator('.ticket-board-column__header')
    .first()
    .evaluate((node) => node.getBoundingClientRect().top);
  await scrollRegions.first().evaluate((node) => {
    node.scrollTop = 180;
    node.dispatchEvent(new Event('scroll'));
  });
  await expect.poll(() => scrollRegions.first().evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
  expect(await scrollRegions.nth(1).evaluate((node) => node.scrollTop)).toBe(0);
  expect(
    await board
      .locator('.ticket-board-column__header')
      .first()
      .evaluate((node) => node.getBoundingClientRect().top),
  ).toBeCloseTo(firstHeaderTop, 0);
  const narrowRow = boardRows.first();
  await expect(narrowRow).toHaveAttribute('data-presentation', 'column');
  const inlineCategory = narrowRow.locator('.ticket-list-row__category');
  const inlineCategorySize = await inlineCategory.evaluate((node) => ({
    width: node.getBoundingClientRect().width,
    height: node.getBoundingClientRect().height,
  }));
  expect(inlineCategorySize.width).toBeCloseTo(17.6, 1);
  expect(inlineCategorySize.height).toBeCloseTo(17.6, 1);
  const inlineOrder = await narrowRow
    .locator('.ticket-list-row__identity')
    .evaluate((node) => [...node.children].map((child) => child.className));
  expect(inlineOrder[1]).toContain('ticket-list-row__identifier');
  const identifierOrder = await narrowRow
    .locator('.ticket-list-row__identifier')
    .evaluate((node) => [...node.children].map((child) => child.className));
  expect(identifierOrder[0]).toContain('ticket-list-row__category');
  expect(identifierOrder.at(-1)).toContain('ticket-list-row__slug');
  const sourcePosition = identifierOrder.findIndex((name) => name.includes('ticket-list-row__source'));
  if (sourcePosition >= 0) expect(sourcePosition).toBe(1);
  const [categoryBox, slugBox] = await Promise.all([
    inlineCategory.boundingBox(),
    narrowRow.locator('.ticket-list-row__slug').boundingBox(),
  ]);
  expect(categoryBox).not.toBeNull();
  expect(slugBox).not.toBeNull();
  expect(Math.abs(categoryBox!.y + categoryBox!.height / 2 - (slugBox!.y + slugBox!.height / 2))).toBeLessThanOrEqual(
    1.5,
  );
  const columnIdentity = narrowRow.locator('.ticket-list-row__identity');
  const columnTitleMetrics = await columnIdentity.evaluate((node) => {
    const style = getComputedStyle(node);
    return { lineHeight: Number.parseFloat(style.lineHeight), maxHeight: Number.parseFloat(style.maxHeight) };
  });
  expect(columnTitleMetrics.maxHeight / columnTitleMetrics.lineHeight).toBeCloseTo(4, 1);
  const longColumnRow = board.locator('[data-ticket-slug="HS2-SG1BKJ"]');
  const longColumnTitle = longColumnRow.locator('.ticket-list-row__identity strong');
  await longColumnRow.scrollIntoViewIfNeeded();
  expect(await longColumnTitle.evaluate((node) => node.getClientRects().length)).toBe(4);
  await longColumnRow.screenshot({ path: 'target/visual-captures/hs2-0tfhhs-four-line-column-title-wide.png' });
  const boardWidth = await narrowRow.evaluate((node) => node.getBoundingClientRect().width);
  expect(boardWidth).toBeLessThan(384);
  await expect(narrowRow).toHaveCSS('border-radius', '10.4px');
  await expect(narrowRow).toHaveCSS('box-shadow', 'none');
  await expect(page.locator('[data-component="ticket-card"]')).toHaveCount(0);
  await board.getByRole('button', { name: 'Select all Backlog tickets' }).click();
  await expect(board.locator('[data-column-id="backlog"] [data-selected="true"]')).toHaveCount(6);
  await page.screenshot({ path: 'target/visual-captures/hs2-x91ssp-column-select-wide.png', fullPage: true });
  await page.screenshot({ path: 'target/visual-captures/hs2-xrnsv0-column-header-wide.png', fullPage: true });
  await page.screenshot({ path: 'target/visual-captures/hs2-4gk04w-column-row-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 900 });
  expect(
    await board
      .locator('.ticket-board-column__header')
      .evaluateAll((headers) => headers.map((header) => header.getBoundingClientRect().height)),
  ).toEqual([32, 32, 32]);
  await page.screenshot({ path: 'target/visual-captures/hs2-xrnsv0-column-header-narrow.png', fullPage: true });
  await page.screenshot({ path: 'target/visual-captures/hs2-4gk04w-column-row-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1600, height: 900 });
  await narrowRow.focus();
  await page.keyboard.press('Meta+A');
  await expect(board.locator('[data-selected="true"]')).toHaveCount(20);
  await page.keyboard.press('Enter');
  await expect(board.locator('[data-selected="true"]')).toHaveCount(1);
  await expect(narrowRow).toHaveAttribute('data-selected', 'true');
  // Right-click an Up-Next-eligible (started) row: the context menu only offers
  // "Toggle Up Next" for not_started/started tickets, so a backlog row (boardRows.first())
  // correctly hides it — this must exercise an eligible row (HS2-AFB17W).
  const eligibleRow = board.locator('[data-ticket-slug="HS2-R76MMW"]');
  await expect(eligibleRow).toHaveAttribute('data-status', 'started');
  await eligibleRow.click({ button: 'right' });
  const menu = page.getByRole('menu', { name: 'Ticket actions' });
  await expect(menu).toBeVisible();
  await menu.getByText('Toggle Up Next', { exact: true }).click();
  await expect(page.getByText(/Toggle Up Next selected for HS2-R76MMW/)).toBeVisible();
  await page.goto('/ux-demo?component=ticket-board-column');
  const columnStage = page.locator('.collection-demo--column');
  const columnDemo = page.locator('[data-component="ticket-board-column"]');
  await expect(columnStage).toHaveCSS('min-width', '250px');
  expect((await columnDemo.first().boundingBox())!.width).toBeGreaterThanOrEqual(250);
  await expect(columnDemo).toHaveCount(1);
  await expect(columnDemo.first().getByLabel('7 tickets')).toBeVisible();
  await expect(columnDemo.first().locator('.ticket-board-column__tickets')).toHaveCSS('overflow-y', 'auto');
});

test('omits status sorting from column view and restores it in list view', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=workspace-header');
  const header = page.locator('.workspace-header');
  const sortSelect = header.locator('wa-select[name="workspace-sort"]');
  await expect(sortSelect.locator('wa-option[value="status"]')).toHaveCount(1);
  await header.getByRole('button', { name: 'Columns view' }).click();
  await expect(sortSelect.locator('wa-option[value="status"]')).toHaveCount(0);
  await sortSelect.click();
  await expect(sortSelect).toHaveJSProperty('open', true);
  await page.screenshot({ path: 'target/visual-captures/hs2-nydfqf-column-sort-options.png', fullPage: true });
});

test('keeps the opened inline search on the wide header row (HS2-NZK4KA)', async ({ page }) => {
  for (const width of [1440, 1100]) {
    await page.setViewportSize({ width, height: 800 });
    await page.goto('/ux-demo?component=workspace-header&dev-review=false');
    const header = page.locator('.workspace-header'),
      search = header.locator('.ticket-search-field');
    await expect(search).toHaveAttribute('data-sizing', 'content');
    await header.getByRole('button', { name: 'Search tickets' }).click();
    await expect(search).toHaveAttribute('data-expanded', 'true');
    await expect(header.getByRole('searchbox', { name: 'Search tickets' })).toBeVisible();
    expect(Math.round(await header.evaluate((node) => node.getBoundingClientRect().height))).toBe(60);
  }
});

test('keeps expanded workspace search inline with More at narrow widths (HS2-NZK4KA)', async ({ page }) => {
  for (const width of [1280, 760, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await page.goto('/ux-demo?component=workspace-header&dev-review=false');
    const header = page.locator('.workspace-header');
    await header.getByRole('button', { name: 'Search tickets' }).click();
    const search = header.getByRole('searchbox', { name: 'Search tickets' });
    await expect(search).toBeVisible();
    const geometry = await header.evaluate((node) => {
      const toolbar = node.getBoundingClientRect(),
        field = node.querySelector('.ticket-search-field')!.getBoundingClientRect();
      return {
        height: toolbar.height,
        fieldTop: field.top,
        toolbarTop: toolbar.top,
        fieldRight: field.right,
        toolbarRight: toolbar.right,
      };
    });
    expect(geometry.height, `viewport ${width}px`).toBeLessThan(80);
    expect(geometry.fieldTop - geometry.toolbarTop).toBeLessThan(20);
    expect(geometry.fieldRight).toBeLessThanOrEqual(geometry.toolbarRight + 1);
    await expect
      .poll(() =>
        header.evaluate((node) => {
          const group = node.querySelector('.ticket-search-field')!.getBoundingClientRect(),
            field = node.querySelector('.ticket-search-field .kui-token-search')!.getBoundingClientRect();
          return Math.abs(group.width - field.width);
        }),
      )
      .toBeLessThanOrEqual(4);
    await page.screenshot({ path: `target/visual-captures/hs2-nzk4ka-search-${width}.png`, fullPage: true });
    if (width === 390) {
      await expect(header.locator('.workspace-header__identity')).toBeHidden();
      await expect(header.locator('.view-mode-switcher')).toBeHidden();
      await expect(header.locator('.workspace-header__sort-group')).toBeHidden();
      await expect(header.locator('.workspace-header__utility-group')).toBeHidden();
      await header.getByRole('button', { name: 'More workspace controls' }).click();
      await expect(header.locator('[data-workspace-overflow]')).toHaveJSProperty('open', true);
      await expect(page.locator('[data-workspace-overflow-action="toggle-selected-up-next"]')).toBeVisible();
      await page.screenshot({ path: 'target/visual-captures/hs2-nzk4ka-search-more-390.png', fullPage: true });
    }
  }
});

test('fits open search around the rendered project name and fills the remaining row (HS2-NZK4KA)', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 800 });
  await page.goto('/ux-demo?component=workspace-header&dev-review=false');
  const header = page.locator('.workspace-header');
  await header.getByRole('button', { name: 'Search tickets' }).click();
  await expect(header.locator('.view-mode-switcher')).toBeVisible();
  await expect(header.locator('.workspace-header__sort-group')).toBeVisible();
  await expect(header.locator('.workspace-header__utility-group')).toBeVisible();
  await expect(header.getByRole('button', { name: 'More workspace controls' })).toBeHidden();
  await expect
    .poll(() => header.locator('.ticket-search-field').evaluate((node) => node.getBoundingClientRect().width))
    .toBeGreaterThan(300);
  await expect
    .poll(() =>
      header.evaluate((node) => {
        const search = node.querySelector('.ticket-search-field')!.getBoundingClientRect();
        return node.getBoundingClientRect().right - search.right;
      }),
    )
    .toBeLessThan(20);
  await header.screenshot({ animations: 'disabled' });
  const emptySearchWidth = await header
    .locator('.ticket-search-field')
    .evaluate((node) => node.getBoundingClientRect().width);
  await header.getByRole('searchbox', { name: 'Search tickets' }).fill('sample query');
  await header.screenshot({ animations: 'disabled' });
  await expect
    .poll(() => header.locator('.ticket-search-field').evaluate((node) => node.getBoundingClientRect().width))
    .toBeGreaterThan(emptySearchWidth - 2);
  await header.getByRole('searchbox', { name: 'Search tickets' }).fill('');
  await header.screenshot({ animations: 'disabled' });
  await expect
    .poll(() => header.locator('.ticket-search-field').evaluate((node) => node.getBoundingClientRect().width))
    .toBeGreaterThan(emptySearchWidth - 2);
  await header.locator('.workspace-header__identity .kui-toolbar-text__text').evaluate((node) => {
    node.textContent = 'A substantially longer project name for this workspace';
  });
  await expect(header.locator('.workspace-header__identity')).toBeVisible();
  await expect(header.locator('.workspace-header__utility-group')).toBeHidden();
  await expect(header.getByRole('button', { name: 'More workspace controls' })).toBeVisible();
  await expect(header.getByRole('searchbox', { name: 'Search tickets' })).toBeVisible();
  await header.locator('.workspace-header__identity .kui-toolbar-text__text').evaluate((node) => {
    node.textContent = 'Hot Sheet 2';
  });
  await expect(header.locator('.workspace-header__utility-group')).toBeVisible();
  await expect(header.getByRole('button', { name: 'More workspace controls' })).toBeHidden();
});

test('draws the workspace sort focus ring as a true pill (HS2-M1DF1D)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=workspace-header&dev-review=false');
  const header = page.locator('.workspace-header');
  const sort = header.locator('wa-select[name="workspace-sort"]');
  const group = header.locator('.workspace-header__sort-group');
  await header.getByRole('button', { name: 'Settings view' }).focus();
  await page.keyboard.press('Tab');
  await expect.poll(() => sort.evaluate((node) => node.matches(':focus-within'))).toBe(true);
  const geometry = await group.evaluate((node) => {
    const style = getComputedStyle(node),
      box = node.getBoundingClientRect(),
      combobox = node
        .querySelector<HTMLElement>('wa-select')!
        .shadowRoot!.querySelector<HTMLElement>('[part~="combobox"]')!,
      comboboxStyle = getComputedStyle(combobox);
    return {
      width: box.width,
      height: box.height,
      radius: style.borderRadius,
      outlineStyle: style.outlineStyle,
      outlineWidth: style.outlineWidth,
      outlineOffset: style.outlineOffset,
      comboboxOutlineStyle: comboboxStyle.outlineStyle,
    };
  });
  // The icon-only sort group (HS2-06GDW3) is as wide as it is tall, so a true pill is a circle:
  // the ring must follow the fully rounded group (Kerf's pill radius is half the 44px height,
  // HS2-4ZA33S) rather than a rectangle.
  expect(geometry.width).toBeGreaterThanOrEqual(geometry.height);
  expect(Number.parseFloat(geometry.radius)).toBeGreaterThanOrEqual(geometry.height / 2);
  expect(geometry).toMatchObject({
    outlineStyle: 'solid',
    outlineWidth: '3px',
    outlineOffset: '1px',
    comboboxOutlineStyle: 'none',
  });
  // Kerf's group-owned ring steps aside while the listbox is open: the popup shows focus instead.
  await page.keyboard.press('Enter');
  await expect(sort).toHaveJSProperty('open', true);
  expect(await group.evaluate((node) => getComputedStyle(node).outlineStyle)).toBe('none');
  await page.keyboard.press('Escape');
  await expect(sort).toHaveJSProperty('open', false);
  const box = (await group.boundingBox())!,
    x = Math.max(0, box.x - 12),
    y = Math.max(0, box.y - 12);
  await page.screenshot({
    path: test.info().outputPath('hs2-m1df1d-pill-focus-ring-wide.png'),
    clip: { x, y, width: Math.min(360, 1280 - x), height: Math.min(360, 800 - y) },
  });
});

test('switches workspace header groups at Kerf toolbar width thresholds (HS2-BBG8ZC)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=workspace-header&dev-review=false');
  const toolbar = page.locator('.workspace-header.kui-toolbar'),
    identity = toolbar.locator('.workspace-header__identity'),
    view = toolbar.locator('.view-mode-switcher'),
    sort = toolbar.locator('.workspace-header__sort-group'),
    utility = toolbar.locator('.workspace-header__utility-group'),
    overflow = toolbar.locator('.workspace-header__overflow-group');
  // The connected demo can rerender its toolbar while live data refreshes. Keep the test width
  // in the document stylesheet so a replacement toolbar receives the same measured width.
  const widthStyle = await page.addStyleTag({ content: '.workspace-header.kui-toolbar { width: 1280px !important; }' });
  const setWidth = async (width: number) => {
    await widthStyle.evaluate((node, next) => {
      node.textContent = `.workspace-header.kui-toolbar { width: ${next}px !important; }`;
    }, width);
    await expect(toolbar).toHaveCSS('width', `${width}px`);
  };

  await expect(utility).toBeVisible();
  await expect(overflow).toBeHidden();
  await setWidth(479);
  await expect(utility).toBeHidden();
  await expect(overflow).toBeVisible();
  await expect(sort).toBeVisible();
  await setWidth(415);
  await expect(sort).toBeHidden();
  await expect(identity).toBeVisible();
  await setWidth(223);
  await expect(identity).toBeHidden();
  await expect(view).toBeVisible();
  await setWidth(175);
  await expect(view).toBeHidden();
  await expect(overflow).toBeVisible();
  await widthStyle.evaluate((node) => node.parentNode?.removeChild(node));
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(sort).toBeHidden();
  await expect(utility).toBeHidden();
  await expect(overflow).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-bbg8zc-workspace-phone.png', animations: 'disabled' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(identity).toBeVisible();
  await expect(view).toBeVisible();
  await expect(sort).toBeVisible();
  await expect(utility).toBeVisible();
  await expect(overflow).toBeHidden();
  await page.screenshot({ path: 'target/visual-captures/hs2-bbg8zc-workspace-wide.png', animations: 'disabled' });
});

test('switches and searches the connected workspace through WorkspaceHeader', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto('/ux-demo?component=workspace-header');
  const header = page.locator('.workspace-header');
  await expect(header).toContainText('Hot Sheet 2');
  await expect(header.locator('.workspace-header__utility-group')).toHaveAttribute('data-selected-chrome', 'outline');
  await expect(header.locator('.workspace-header__utility-group')).toHaveAttribute('data-selected-tone', 'pop');
  const sortIcon = header.locator('.workspace-header__sort-group .kui-select__custom-selected'),
    idleViewIcon = header.getByRole('button', { name: 'Columns view' }),
    quietIconColor = await idleViewIcon.evaluate((node) => getComputedStyle(node).color);
  expect(await sortIcon.evaluate((node) => getComputedStyle(node).color)).toBe(quietIconColor);
  const notificationBadge = header.locator('.view-mode-switcher__badge');
  await expect(notificationBadge).toHaveText('2');
  await expect(notificationBadge).toHaveCSS('font-size', '10px');
  await expect(notificationBadge).toHaveCSS('padding', '1px 5px');
  await expect(notificationBadge).toHaveCSS('background-color', 'rgb(255, 204, 0)');
  const badgeBounds = await notificationBadge.boundingBox();
  const bellBounds = await header.locator('.view-mode-switcher [data-lucide="bell"]').boundingBox();
  expect(badgeBounds!.y + badgeBounds!.height).toBeLessThanOrEqual(bellBounds!.y + 2);
  await expect(header.getByRole('group', { name: 'View mode', exact: true })).toHaveAttribute(
    'data-component',
    'segmented-control',
  );
  await header.screenshot({ path: 'target/visual-captures/hs2-f29qat-demo-badge-wide.png', animations: 'disabled' });
  await notificationBadge.screenshot({ path: 'target/visual-captures/hs2-x9embf-notification-badge.png' });
  await page.screenshot({ path: 'target/visual-captures/hs2-rza0h3-semantic-tokens-wide.png', fullPage: true });
  await expect(header.getByRole('button', { name: 'List view' })).toHaveAttribute('aria-pressed', 'true');
  await expect(
    page.getByRole('listbox', { name: 'Workspace tickets' }).locator('[data-component="ticket-list-row"]'),
  ).toHaveCount(20);
  await header.getByRole('button', { name: 'Columns view' }).click();
  await expect(header.getByRole('button', { name: 'Columns view' })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('listbox', { name: 'Workspace board' })).toBeVisible();
  const closedHeaderHeight = await header.evaluate((node) => node.getBoundingClientRect().height);
  const searchGroup = header.locator('.ticket-search-field');
  const collapsedWidth = await searchGroup.evaluate((node) => node.getBoundingClientRect().width);
  const collapsedHeight = await searchGroup.evaluate((node) => node.getBoundingClientRect().height);
  expect(collapsedWidth).toBeCloseTo(collapsedHeight, 0);
  const findButton = header.getByRole('button', { name: 'Search tickets' });
  await findButton.click();
  await expect(findButton).toHaveCount(0);
  const searchControl = header.locator('.kui-token-search');
  const search = header.getByRole('searchbox', { name: 'Search tickets' });
  await expect(search).toBeFocused();
  await expect(searchControl.locator('[data-lucide="search"]')).toBeVisible();
  await expect(searchGroup).toHaveCSS('border-width', '1px');
  await expect(searchGroup).toHaveCSS('border-style', 'solid');
  await expect(searchControl).toHaveCSS('border-width', '0px');
  await expect(searchGroup).not.toHaveCSS('box-shadow', 'none');
  await expect
    .poll(() => searchGroup.evaluate((node) => node.getBoundingClientRect().width))
    .toBeGreaterThan(collapsedWidth * 3);
  const openHeaderHeight = await header.evaluate((node) => node.getBoundingClientRect().height);
  expect(Math.abs(openHeaderHeight - closedHeaderHeight)).toBeLessThanOrEqual(3);
  await search.fill('long-tag-example');
  await expect(
    page.getByRole('listbox', { name: 'Workspace board' }).locator('[data-component="ticket-list-row"]'),
  ).toHaveCount(1);
  // The X clear button empties the search and restores every row (HS2-Z7KP1Q).
  const clearSearch = header.locator('[data-action="clear-ticket-search"]');
  await expect(clearSearch).toBeVisible();
  await clearSearch.click();
  await expect(search).toHaveText('');
  await expect(
    page.getByRole('listbox', { name: 'Workspace board' }).locator('[data-component="ticket-list-row"]'),
  ).toHaveCount(20);
  await expect(clearSearch).toHaveCount(0);
  await search.fill('long-tag-example');
  await header.getByRole('button', { name: 'Columns view' }).focus();
  await expect(search).toBeVisible();
  await expect(searchGroup).toHaveCSS('border-width', '1px');
  await expect(searchGroup).toHaveCSS('box-shadow', 'none');
  await search.fill('');
  await header.getByRole('button', { name: 'Columns view' }).focus();
  await expect(header.getByRole('searchbox', { name: 'Search tickets' })).toHaveCount(0);
  await expect(header.getByRole('button', { name: 'Search tickets' })).toBeVisible();
  await expect(searchGroup).toHaveCSS('border-width', '1px');
  await expect(
    page.getByRole('listbox', { name: 'Workspace board' }).locator('[data-component="ticket-list-row"]'),
  ).toHaveCount(20);
  await header.getByRole('button', { name: 'List view' }).click();
  const sortSelect = header.locator('wa-select[name="workspace-sort"]');
  await expect(sortSelect).toHaveAttribute('aria-label', 'Sort tickets: Recently updated, descending');
  await expect(sortSelect.locator('.kui-select__custom-selected [data-lucide="clock-arrow-down"]')).toBeVisible();
  await expect(sortSelect.locator('.kui-select__custom-selected')).toHaveCSS('color', quietIconColor);
  const triggerGeometry = await sortSelect.evaluate((node) => {
    const root = node.shadowRoot!,
      combobox = root.querySelector<HTMLElement>('[part~="combobox"]')!,
      expand = root.querySelector<HTMLElement>('[part~="expand-icon"]')!,
      selected = node.querySelector<HTMLElement>('.kui-select__custom-selected')!,
      outer = combobox.getBoundingClientRect(),
      icon = selected.getBoundingClientRect(),
      arrow = expand.getBoundingClientRect();
    return {
      width: outer.width,
      height: outer.height,
      caretHidden: getComputedStyle(expand).display === 'none',
      iconCentered: Math.abs(icon.left + icon.width / 2 - (outer.left + outer.width / 2)),
      arrowOverflow: arrow.right - outer.right,
    };
  });
  // Kerf's caret-free icon-only trigger (beta.62 `caret={false}`, HS2-4ZA33S) is a round action:
  // as wide as it is tall, with the icon centered and no caret box.
  expect(triggerGeometry.caretHidden).toBe(true);
  expect(Math.abs(triggerGeometry.width - triggerGeometry.height)).toBeLessThanOrEqual(2);
  // Kerf's caret-free trigger still sits its icon about 4px off center (KF-5TX9Z5); tighten to 1.5px when it ships.
  expect(triggerGeometry.iconCentered).toBeLessThanOrEqual(4.5);
  expect(triggerGeometry.arrowOverflow).toBeLessThanOrEqual(0);
  await sortSelect.click();
  await expect(sortSelect.locator('wa-option[value="updated"] [data-lucide="clock-arrow-down"]')).toBeVisible();
  await expect(sortSelect.locator('wa-option[value="priority"] .kui-select__icon')).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-0dcczk-sort-select-wide.png', fullPage: true });
  const prioritySort = sortSelect.locator('wa-option[value="priority"]');
  await prioritySort.click();
  await expect(page.getByText('Sorted by priority, ascending')).toBeVisible();
  await expect(sortSelect).toHaveJSProperty('value', 'priority');
  await expect(sortSelect.locator('.kui-select__custom-selected [data-lucide="arrow-up-narrow-wide"]')).toBeVisible();
  const ascendingPriorities = await page
    .getByRole('listbox', { name: 'Workspace tickets' })
    .locator('.ticket-list-row__priority')
    .evaluateAll((nodes) => nodes.map((node) => node.getAttribute('aria-label')?.split(' ')[0]));
  expect(ascendingPriorities.map((value) => ['low', 'default', 'high', 'urgent'].indexOf(value ?? ''))).toEqual(
    [...ascendingPriorities]
      .map((value) => ['low', 'default', 'high', 'urgent'].indexOf(value ?? ''))
      .sort((a, b) => a - b),
  );
  await page.screenshot({ path: 'target/visual-captures/hs2-5avfng-priority-ascending.png', fullPage: true });
  await sortSelect.click();
  await prioritySort.click();
  await expect(page.getByText('Sorted by priority, descending')).toBeVisible();
  await expect(sortSelect.locator('.kui-select__custom-selected [data-lucide="arrow-down-wide-narrow"]')).toBeVisible();
  const descendingPriorities = await page
    .getByRole('listbox', { name: 'Workspace tickets' })
    .locator('.ticket-list-row__priority')
    .evaluateAll((nodes) => nodes.map((node) => node.getAttribute('aria-label')?.split(' ')[0]));
  expect(descendingPriorities.map((value) => ['low', 'default', 'high', 'urgent'].indexOf(value ?? ''))).toEqual(
    [...descendingPriorities]
      .map((value) => ['low', 'default', 'high', 'urgent'].indexOf(value ?? ''))
      .sort((a, b) => b - a),
  );
  const statusOption = sortSelect.locator('wa-option[value="status"]');
  await expect(prioritySort).not.toBeVisible();
  await sortSelect.click();
  await expect(statusOption).toBeVisible();
  await statusOption.click();
  const ascendingStatuses = await page
    .getByRole('listbox', { name: 'Workspace tickets' })
    .locator('[data-component="ticket-list-row"]')
    .evaluateAll((nodes) => nodes.map((node) => (node as HTMLElement).dataset.status));
  expect(
    ascendingStatuses.map((value) =>
      ['backlog', 'not_started', 'started', 'completed', 'verified', 'archive'].indexOf(value ?? ''),
    ),
  ).toEqual(
    [...ascendingStatuses]
      .map((value) => ['backlog', 'not_started', 'started', 'completed', 'verified', 'archive'].indexOf(value ?? ''))
      .sort((a, b) => a - b),
  );
  await page.screenshot({ path: 'target/visual-captures/hs2-4penqq-status-ascending.png', fullPage: true });
  for (const [value, firstIcon, secondIcon] of [
    ['title', 'arrow-down-a-z', 'arrow-up-a-z'],
    ['updated', 'clock-arrow-down', 'clock-arrow-up'],
  ] as const) {
    const option = sortSelect.locator(`wa-option[value="${value}"]`);
    await sortSelect.click();
    await expect(option).toBeVisible();
    await option.click();
    await expect(option).not.toBeVisible();
    await expect(sortSelect.locator(`.kui-select__custom-selected [data-lucide="${firstIcon}"]`)).toBeVisible();
    await sortSelect.click();
    await expect(option).toBeVisible();
    await option.click();
    await expect(option).not.toBeVisible();
    await expect(sortSelect.locator(`.kui-select__custom-selected [data-lucide="${secondIcon}"]`)).toBeVisible();
  }
  await page.setViewportSize({ width: 1024, height: 600 });
  await sortSelect.click();
  await page.screenshot({ path: 'target/visual-captures/hs2-0dcczk-sort-select-floor.png', fullPage: true });
  await page.keyboard.press('Escape');
  await header.getByRole('button', { name: 'Settings view' }).click();
  await expect(header.getByRole('button', { name: 'Settings view' })).toHaveAttribute('aria-pressed', 'true');
  await expect(sortSelect).toHaveAttribute('disabled', '');
  for (const name of ['Toggle Up Next for selected tickets', 'More actions for selected tickets', 'Search tickets'])
    await expect(header.getByRole('button', { name })).toHaveAttribute('disabled', '');
  // Kerf's ToolbarControlGroup owns the disabled utility buttons' presentation with no app restyle
  // (KF-FTADQT, HS2-0MH5V1): a not-allowed cursor, half opacity, and no hover chrome.
  for (const name of ['Toggle Up Next for selected tickets', 'More actions for selected tickets']) {
    const utility = header.getByRole('button', { name });
    await expect(utility).toHaveCSS('cursor', 'not-allowed');
    await expect(utility).toHaveCSS('opacity', '0.5');
    await utility.hover({ force: true });
    await expect(utility).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  }
  await expect(page.getByRole('region', { name: 'Project settings' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Workspace board' })).toHaveCount(0);
  await header.getByRole('button', { name: 'List view' }).click();
  await expect(page.getByRole('listbox', { name: 'Workspace tickets' })).toBeVisible();
  await page.setViewportSize({ width: 760, height: 900 });
  await expect(notificationBadge).toHaveCSS('font-size', '10px');
  await page.screenshot({ path: 'target/visual-captures/hs2-rza0h3-semantic-tokens-narrow.png', fullPage: true });
});

test('connects WorkspaceHeader notifications, actions, reset and badge count across modes (HS2-Y70MJY)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 1200 });
  await page.goto('/ux-demo?component=workspace-header&dev-review=false');
  const demo = page.getByRole('region', { name: 'WorkspaceHeader demo', exact: true }),
    notifications = demo.getByRole('button', { name: /^Notifications view/ }),
    center = demo.locator('[data-component="notification-center"]'),
    pending = center.locator('[data-component="permission-request-card"][data-state="pending"]'),
    read = center.locator('[data-request-key="workspace-demo:1"][data-component="permission-request-card"]'),
    command = center.locator('[data-request-key="workspace-demo:2"][data-component="permission-request-card"]');
  for (const mode of ['Notifications', 'Columns', 'Settings', 'List', 'Notifications']) {
    await demo.getByRole('button', { name: new RegExp(`^${mode} view`) }).click();
    await expect(notifications).toHaveAccessibleName('Notifications view, 2 pending');
    await expect(notifications.locator('.view-mode-switcher__badge')).toHaveText('2');
    await expect(center).toHaveCount(mode === 'Notifications' ? 1 : 0);
    await expect(demo.locator('[data-component="ticket-board"]')).toHaveCount(mode === 'Columns' ? 1 : 0);
  }
  await expect(notifications).toHaveAttribute('aria-pressed', 'true');
  await expect(demo.getByRole('heading', { name: 'Notifications', exact: true })).toBeVisible();
  await expect(pending).toHaveCount(2);
  await expect(read.getByRole('button', { name: 'Always Allow' })).toHaveCount(0);
  await expect(command.getByRole('button', { name: 'Always Allow' })).toBeEnabled();
  await demo.screenshot({ path: 'target/visual-captures/hs2-y70mjy-notifications-wide.png', animations: 'disabled' });
  await read.getByRole('button', { name: 'Ignore', exact: true }).click();
  await expect(
    demo.getByText('Demo prompt ignored; the request remains pending in Notifications until answered.'),
  ).toBeVisible();
  await expect(pending).toHaveCount(2);
  await read.getByRole('button', { name: 'Allow', exact: true }).click();
  await expect(read).toHaveAttribute('data-state', 'allow');
  await expect(read.locator('[data-lucide="check"]')).toBeVisible();
  await expect(read.getByRole('button')).toHaveCount(0);
  await expect(notifications).toHaveAccessibleName('Notifications view, 1 pending');
  await command.getByRole('button', { name: 'Deny', exact: true }).click();
  await expect(command).toHaveAttribute('data-state', 'deny');
  await expect(command.locator('[data-lucide="x"]')).toBeVisible();
  await expect(pending).toHaveCount(0);
  await expect(notifications).toHaveAccessibleName('Notifications view');
  await expect(notifications.locator('.view-mode-switcher__badge')).toHaveCount(0);
  await demo.getByRole('button', { name: 'List view', exact: true }).click();
  await expect(demo.getByRole('listbox', { name: 'Workspace tickets' })).toBeVisible();
  await notifications.click();
  await expect(pending).toHaveCount(0);
  await demo.getByRole('button', { name: 'Reset notifications', exact: true }).click();
  await expect(pending).toHaveCount(2);
  await expect(center.locator('[data-resolved="true"]')).toHaveCount(1);
  await expect(notifications).toHaveAccessibleName('Notifications view, 2 pending');
  await page.setViewportSize({ width: 560, height: 844 });
  await demo.screenshot({ path: 'target/visual-captures/hs2-y70mjy-notifications-narrow.png', animations: 'disabled' });
  await command.getByRole('button', { name: 'Always Allow', exact: true }).click();
  await expect(command).toHaveAttribute('data-state', 'allow');
  await expect(command.getByText(/allowed this kind of request$/)).toBeVisible();
  await expect(notifications).toHaveAccessibleName('Notifications view, 1 pending');
  await read.getByRole('button', { name: 'Deny', exact: true }).click();
  await expect(pending).toHaveCount(0);
  await expect(center.locator('[data-resolved="true"]')).toHaveCount(3);
  await expect(notifications.locator('.view-mode-switcher__badge')).toHaveCount(0);
  await demo.screenshot({ path: 'target/visual-captures/hs2-y70mjy-history-narrow.png', animations: 'disabled' });
  await demo.getByRole('button', { name: 'Reset notifications', exact: true }).click();
  await expect(pending).toHaveCount(2);
  await command.getByRole('button', { name: 'Allow Once', exact: true }).focus();
  await page.keyboard.press('Enter');
  await expect(command).toHaveAttribute('data-state', 'allow');
  await expect(command.getByText(/allowed permission$/)).toBeVisible();
  await expect(command.getByText(/allowed this kind of request$/)).toHaveCount(0);
  await expect(notifications).toHaveAccessibleName('Notifications view, 1 pending');
});

test('organizes search syntax help in the WorkspaceHeader demo', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/ux-demo?component=workspace-header');
  const header = page.locator('.workspace-header');
  await header.getByRole('button', { name: 'Search tickets' }).click();
  await header.getByRole('searchbox', { name: 'Search tickets' }).fill('client');
  await header.getByRole('button', { name: 'Search syntax help' }).click();
  const help = header.getByRole('dialog', { name: 'Search syntax' });
  await expect(help.locator('dt')).toHaveText(['Tags', 'Content', 'Workflow', 'Dates']);
  await expect(help).toContainText('Combine filters');
  await page.screenshot({ path: 'target/visual-captures/hs2-7efj3e-search-help-demo-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 640 });
  await help.scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/hs2-7efj3e-search-help-demo-narrow.png', fullPage: true });
  await expect
    .poll(() =>
      help.evaluate((node) => {
        const box = node.getBoundingClientRect();
        return box.left >= 0 && box.top >= 0 && box.right <= innerWidth && box.bottom <= innerHeight;
      }),
    )
    .toBe(true);
  await page.setViewportSize({ width: 390, height: 844 });
  await help.scrollIntoViewIfNeeded();
  await expect
    .poll(() =>
      help.evaluate((node) => {
        const box = node.getBoundingClientRect();
        return box.left >= 0 && box.right <= innerWidth;
      }),
    )
    .toBe(true);
  expect(await help.evaluate((node) => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
  await page.screenshot({ path: 'target/visual-captures/hs2-pv2ag1-search-phone.png', fullPage: true });
  await help.evaluate((node) => {
    node.scrollTop = node.scrollHeight;
  });
  await expect.poll(() => help.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
  expect(
    await help.evaluate((node) => {
      const panel = node.getBoundingClientRect(),
        finalNote = node.lastElementChild!.getBoundingClientRect();
      return panel.bottom <= innerHeight - 120 && finalNote.bottom <= panel.bottom - 4;
    }),
  ).toBe(true);
  await page.screenshot({ path: 'target/visual-captures/hs2-pv2ag1-search-phone-bottom.png', fullPage: true });
  await header.getByRole('button', { name: 'Search syntax help' }).click();
  await expect(help).toHaveCount(0);
  await header.getByRole('searchbox', { name: 'Search tickets' }).fill('updated-after:');
  const date = header.getByRole('group', { name: 'Date and time helper' });
  await expect(date).toBeVisible();
  expect(await date.evaluate((node) => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
  await page.screenshot({ path: 'target/visual-captures/hs2-pv2ag1-date-phone.png', fullPage: true });
});

test('centers search controls on the first line while the query wraps', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 600 });
  await page.goto('/ux-demo?component=workspace-header');
  const header = page.locator('.workspace-header');
  await header.getByRole('button', { name: 'Search tickets' }).click();
  const search = header.getByRole('searchbox', { name: 'Search tickets' }),
    group = header.locator('.ticket-search-field');
  const geometry = async () =>
    group.evaluate((node) => {
      const box = (selector: string) => {
        const element = node.querySelector<HTMLElement>(selector)!,
          rect = element.getBoundingClientRect(),
          style = getComputedStyle(element);
        return {
          top: rect.top,
          bottom: rect.bottom,
          height: rect.height,
          center: (rect.top + rect.bottom) / 2,
          paddingTop: Number.parseFloat(style.paddingTop),
          lineHeight: Number.parseFloat(style.lineHeight),
        };
      };
      const group = node.getBoundingClientRect();
      return {
        group: { top: group.top, bottom: group.bottom, height: group.height, center: (group.top + group.bottom) / 2 },
        search: box('.kui-token-search__editor'),
        icon: box('.kui-token-search__leading'),
        clear: node.querySelector('.kui-token-search__clear') ? box('.kui-token-search__clear') : undefined,
        help: box('.ticket-search-field__help-button'),
      };
    });
  const centered = async (expectedGroupCenter: boolean) => {
    const measured = await geometry(),
      firstLineCenter = measured.search.top + measured.search.paddingTop + measured.search.lineHeight / 2;
    expect(measured.icon.center).toBeCloseTo(firstLineCenter, 1);
    expect(measured.help.center).toBeCloseTo(firstLineCenter, 1);
    if (measured.clear) expect(measured.clear.center).toBeCloseTo(firstLineCenter, 1);
    if (expectedGroupCenter) expect(firstLineCenter).toBeCloseTo(measured.group.center, 1);
    return measured;
  };
  await search.fill('client');
  await header.getByRole('button', { name: 'Clear search' }).click();
  await expect(search).toHaveText('');
  await expect(header.getByRole('button', { name: 'Clear search' })).toHaveCount(0);
  const blank = await centered(true),
    blankBox = await group.boundingBox();
  await page.screenshot({
    path: 'target/visual-captures/hs2-dyzbf4-search-single-line-after.png',
    clip: {
      x: Math.max(0, blankBox!.x - 8),
      y: Math.max(0, blankBox!.y - 8),
      width: Math.min(1000, blankBox!.width + 16),
      height: blankBox!.height + 16,
    },
  });
  await search.fill('client');
  await expect(header.getByRole('button', { name: 'Clear search' })).toBeVisible();
  await centered(true);
  await search.fill(
    'This intentionally long ordinary search query wraps across multiple lines while its peer controls stay aligned with the first line of editable text, even inside the narrower catalog detail pane used by the shared UX demo shell',
  );
  const wrapped = await centered(false),
    wrappedBox = await group.boundingBox();
  expect(wrapped.group.height).toBeGreaterThan(blank.group.height + wrapped.search.lineHeight);
  await page.screenshot({
    path: 'target/visual-captures/hs2-dyzbf4-search-wrapped-after.png',
    clip: {
      x: Math.max(0, wrappedBox!.x - 8),
      y: Math.max(0, wrappedBox!.y - 8),
      width: Math.min(1000, wrappedBox!.width + 16),
      height: wrappedBox!.height + 16,
    },
  });
});

test('shows the ToolbarControlGroup variants with shared geometry', async ({ page }) => {
  await page.goto('/ux-demo?component=toolbar-control-group');
  const demo = page.getByRole('region', { name: 'ToolbarControlGroup demo' });
  const groups = demo.locator('.kui-toolbar-control-group');
  await expect(groups).toHaveCount(8);
  expect(await groups.evaluateAll((nodes) => nodes.every((node) => node.closest('[data-component="toolbar"]')))).toBe(
    true,
  );
  const clippedGroups = await groups.evaluateAll((nodes) =>
    nodes.flatMap((node, index) => {
      const group = node.getBoundingClientRect();
      const toolbar = node.closest('[data-component="toolbar"]')!.getBoundingClientRect();
      return group.left >= toolbar.left - 1 && group.right <= toolbar.right + 1
        ? []
        : [
            {
              index,
              groupWidth: group.width,
              toolbarWidth: toolbar.width,
              groupRight: group.right,
              toolbarRight: toolbar.right,
            },
          ];
    }),
  );
  expect(clippedGroups).toEqual([]);
  const segments = demo.getByRole('group', { name: 'View mode', exact: true });
  await expect(segments).toHaveAttribute('data-component', 'segmented-control');
  const list = segments.getByRole('button', { name: 'List view' });
  await expect(list).toHaveAttribute('aria-pressed', 'true');
  await list.focus();
  await page.keyboard.press('Tab');
  const columns = segments.getByRole('button', { name: 'Columns view' });
  await expect(columns).toBeFocused();
  await page.keyboard.press('Space');
  await expect(columns).toHaveAttribute('aria-pressed', 'true');
  await expect(list).toHaveAttribute('aria-pressed', 'false');
  await segments.getByRole('button', { name: 'Settings view' }).click();
  await expect(segments).toHaveAttribute('data-value', 'settings');
  await list.click();
  await expect(list).toHaveAttribute('aria-pressed', 'true');
  await columns.click();
  await expect(columns).toHaveAttribute('aria-pressed', 'true');
  for (const icon of ['arrow-down-a-z', 'star', 'ellipsis', 'pin', 'panel-left-open'])
    await expect(demo.locator(`[data-lucide="${icon}"]`)).toBeVisible();
  const heights = await groups.evaluateAll((nodes) => nodes.map((node) => node.getBoundingClientRect().height));
  expect(new Set(heights).size).toBe(1);
  const popup = demo.locator('wa-button[with-caret]');
  const caretSpacing = await popup.evaluate((node) => {
    const button = node.shadowRoot?.querySelector<HTMLElement>('[part~="button"]');
    const caret = node.shadowRoot?.querySelector<HTMLElement>('[part~="caret"]');
    return button && caret
      ? {
          gap: getComputedStyle(button).gap,
          margin: getComputedStyle(caret).marginInlineStart,
          width: button.getBoundingClientRect().width,
          height: button.getBoundingClientRect().height,
        }
      : null;
  });
  // Kerf 5.0.0-beta.51 fits the popup trigger to its icon-plus-caret pill (KF-Y3YZBE). The demo stage no
  // longer restyles the trigger's parts, so this is Kerf's own geometry, as production renders it (HS2-4APEJP).
  expect(caretSpacing!.height).toBe(40);
  expect(caretSpacing!.width).toBeGreaterThan(caretSpacing!.height);
  const popupGroupWidth = await groups.nth(1).evaluate((node) => node.getBoundingClientRect().width);
  expect(popupGroupWidth - caretSpacing!.width).toBeCloseTo(4, 0);
  await popup.hover();
  await expect(groups.nth(1)).toHaveCSS('background-color', 'rgb(255, 255, 255)');
  const popupBackground = await popup.evaluate((node) => {
    const button = node.shadowRoot?.querySelector<HTMLElement>('[part~="base"]');
    return button ? getComputedStyle(button).backgroundColor : null;
  });
  expect(popupBackground).toBe('rgba(0, 0, 0, 0)');
  const groupedButton = demo.locator('button[aria-label="Favorite view"]').first();
  await groupedButton.hover();
  await expect(groupedButton).toHaveCSS('background-color', 'rgb(255, 255, 255)');
  const groupedGeometry = await groupedButton.evaluate((node) => {
    return { height: node.getBoundingClientRect().height, background: getComputedStyle(node).backgroundColor };
  });
  expect(groupedGeometry).toEqual({ height: 40, background: 'rgb(255, 255, 255)' });
  const iconAlignment = await groupedButton.evaluate((node) => {
    const icon = node.querySelector<HTMLElement>('[data-lucide]');
    if (!icon) return null;
    const buttonBox = node.getBoundingClientRect();
    const iconBox = icon.getBoundingClientRect();
    return Math.abs(buttonBox.top + buttonBox.height / 2 - (iconBox.top + iconBox.height / 2));
  });
  expect(iconAlignment).toBeLessThan(1);
  const borderless = groups.nth(4);
  await expect(borderless).toHaveAttribute('data-appearance', 'borderless');
  await expect(borderless).toHaveCSS('border-color', 'rgba(0, 0, 0, 0)');
  await expect(borderless).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await borderless.getByRole('button').hover();
  await expect(borderless).toHaveCSS('background-color', 'rgba(0, 0, 49, 0.1)');
  const restingPush = demo.getByRole('button', { name: 'Resting comparison' }).locator('..');
  const pressedPush = demo.getByRole('button', { name: 'Pressed comparison' }).locator('..');
  await expect(restingPush).toHaveAttribute('data-button-appearance', 'push');
  await expect(pressedPush).toHaveCSS('background-color', 'rgb(72, 72, 74)');
  await expect(pressedPush).toHaveCSS('border-color', 'rgba(0, 0, 13, 0.318)');
  await expect(pressedPush.getByRole('button')).toHaveCSS('color', 'rgb(255, 255, 255)');
  const dark = demo.getByRole('group', { name: 'Dark navigation' });
  await expect(dark).toHaveAttribute('data-tone', 'dark');
  await expect(dark).toHaveCSS('color', 'rgb(255, 255, 255)');
  await expect(dark).toHaveCSS('border-color', 'rgb(53, 53, 54)');
  await page.screenshot({ path: 'target/visual-captures/hs2-t3m818-dark-toolbar-border-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 900 });
  await expect(dark).toHaveCSS('border-color', 'rgb(53, 53, 54)');
  await page.screenshot({ path: 'target/visual-captures/hs2-t3m818-dark-toolbar-border-narrow.png', fullPage: true });
  await expect(demo.getByRole('heading', { name: 'Single button' })).toBeVisible();
});

test('catalogs forced-dark FloatingToolbar children across page themes (HS2-HW02QG)', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=floating-toolbar&dev-review=false');
  const toolbar = page.getByRole('toolbar', { name: 'Preview zoom controls' }),
    stage = page.locator('.floating-toolbar-demo');
  await expect(toolbar).toHaveAttribute('data-component', 'floating-toolbar');
  await expect(toolbar).toHaveAttribute('data-position', 'bottom-end');
  const group = toolbar.locator('[data-component="toolbar-control-group"]');
  await expect(group).toHaveAttribute('data-tone', 'default');
  await expect(toolbar.getByRole('button', { name: 'Zoom out' })).toBeEnabled();
  await expect(toolbar.getByRole('button', { name: 'Zoom in' })).toBeEnabled();
  const insets = await toolbar.evaluate((node) => {
    const toolbarBounds = node.getBoundingClientRect(),
      stageBounds = node.parentElement!.getBoundingClientRect();
    return { right: stageBounds.right - toolbarBounds.right, bottom: stageBounds.bottom - toolbarBounds.bottom };
  });
  expect(insets.right).toBeCloseTo(16, 0);
  expect(insets.bottom).toBeCloseTo(16, 0);
  await expect(stage).toBeVisible();
  for (const theme of ['light', 'dark', 'light'] as const) {
    if (theme === 'dark') await page.getByRole('button', { name: 'Use dark theme', exact: true }).click();
    else if ((await page.locator('html').getAttribute('data-theme')) === 'dark')
      await page.getByRole('button', { name: 'Use light theme', exact: true }).click();
    await expect(group).toHaveCSS('color-scheme', 'dark');
    await expect(group).toHaveCSS('background-color', 'rgb(58, 58, 60)');
    await expect(group).toHaveCSS('color', 'rgb(194, 194, 198)');
    const zoom = toolbar.getByRole('button', { name: 'Zoom in' });
    await zoom.hover();
    await expect(zoom).toHaveCSS('background-color', 'rgb(28, 28, 30)');
    await toolbar.getByRole('button', { name: 'Zoom out' }).focus();
    await page.keyboard.press('Tab');
    await expect(zoom).toBeFocused();
    await expect(zoom).not.toHaveCSS('outline-style', 'none');
    await page.mouse.move(0, 0);
    await stage.screenshot({ path: testInfo.outputPath(`floating-toolbar-${theme}-wide.png`) });
  }
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(group).toHaveCSS('background-color', 'rgb(58, 58, 60)');
  await toolbar.scrollIntoViewIfNeeded();
  await expect(toolbar).toBeInViewport();
  await stage.screenshot({ path: testInfo.outputPath('floating-toolbar-light-narrow.png') });
});

test('shows the reader text push state at exactly one and a half times normal size', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-reader');
  const reader = page.locator('[data-component="ticket-reader"]');
  const paragraph = reader.locator('.ticket-info-panel__details-surface .markdown-preview p').first();
  const ordinarySize = parseFloat(await paragraph.evaluate((node) => getComputedStyle(node).fontSize));
  const toggle = reader.locator('[data-action="toggle-reader-text-size"]');
  await expect(toggle).toHaveAccessibleName('Use large reader text size');
  const pushGroup = toggle.locator('..');
  await expect(pushGroup).toHaveAttribute('data-button-appearance', 'push');
  await toggle.click();
  await expect(reader).toHaveAttribute('data-large-text', 'true');
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await expect(toggle).toHaveAccessibleName('Use standard reader text size');
  await expect
    .poll(async () => parseFloat(await paragraph.evaluate((node) => getComputedStyle(node).fontSize)))
    .toBeCloseTo(ordinarySize * 1.5, 1);
  await expect(pushGroup).toHaveCSS('background-color', 'rgb(72, 72, 74)');
  await expect(pushGroup).toHaveCSS('border-color', 'rgba(0, 0, 13, 0.318)');
  await expect(toggle).toHaveCSS('color', 'rgb(255, 255, 255)');
  await page.screenshot({ path: 'target/visual-captures/hs2-28frr0-reader-pressed-wide.png', fullPage: true });
  await reader
    .locator('.ticket-inspector__header')
    .screenshot({ path: 'target/visual-captures/hs2-28frr0-reader-pressed-header.png' });
  await page.setViewportSize({ width: 1024, height: 700 });
  await reader.locator('[data-component="ticket-notes"]').scrollIntoViewIfNeeded();
  await page.screenshot({ path: 'target/visual-captures/hs2-28frr0-reader-pressed-narrow.png', fullPage: true });
  await toggle.click();
  await expect(reader).toHaveAttribute('data-large-text', 'false');
});

test('shows repository comparison as a shared pressed toolbar control', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=repository-status-popover');
  const dialog = page.locator('[data-component="repository-status-popover"]');
  await dialog.getByRole('button', { name: /Commits/ }).click();
  const compare = dialog.getByRole('button', { name: 'Compare two commits' });
  const group = compare.locator('..');
  await expect(group).toHaveAttribute('data-button-appearance', 'push');
  await compare.click();
  await expect(compare).toHaveAttribute('aria-pressed', 'true');
  await expect(group).toHaveCSS('background-color', 'rgb(72, 72, 74)');
  await expect(group).toHaveCSS('border-color', 'rgba(0, 0, 13, 0.318)');
  await expect(compare).toHaveCSS('color', 'rgb(255, 255, 255)');
  await dialog.screenshot({ path: 'target/visual-captures/hs2-7cnf5b-compare-pressed-wide.png' });
  await dialog
    .locator('[data-component="heading"]')
    .screenshot({ path: 'target/visual-captures/hs2-7cnf5b-compare-pressed-header.png' });
  await page.addStyleTag({
    content:
      'body{min-width:0}.demo-shell{display:block}.kui-workbench__rail--left,.kui-workbench__main > .kui-pane > .kui-pane__header,.kui-workbench__main > .kui-pane > .kui-pane__footer,.settings-toggle{display:none}.kui-workbench__main{min-height:0;padding:12px}',
  });
  await page.setViewportSize({ width: 760, height: 640 });
  await page.mouse.move(740, 620);
  await page.waitForTimeout(150);
  await dialog.screenshot({ path: 'target/visual-captures/hs2-7cnf5b-compare-pressed-narrow.png' });
  await compare.click();
  await expect(compare).toHaveAttribute('aria-pressed', 'false');
});

test('expands, validates, creates, and cancels through QuickTicketComposer', async ({ page }) => {
  await page.goto('/ux-demo?component=quick-ticket-composer');
  await page.getByRole('button', { name: /New ticket/ }).click();
  const form = page.locator('[data-action="create-ticket-form"]');
  const title = form.getByRole('textbox', { name: 'Ticket title' });
  await expect(title).toBeFocused();
  await form.getByRole('button', { name: 'Create ticket' }).click();
  expect(await title.evaluate((node: HTMLInputElement) => node.checkValidity())).toBe(false);
  await expect(form).toBeVisible();
  await title.fill('Created from the UX demo');
  const details = form.getByRole('textbox', { name: 'Details' });
  await details.fill('One-line details that can grow.');
  expect(await details.getAttribute('rows')).toBe('1');
  expect(await details.evaluate((node) => getComputedStyle(node).resize)).toBe('vertical');
  const category = form.locator('wa-select[name="new-ticket-category"]');
  await expect(category.locator('.kui-select__icon--selected [data-lucide="list-checks"]')).toBeVisible();
  await category.click();
  const selectedOption = category.locator('wa-option[value="task"]');
  await expect(selectedOption).toHaveCSS('background-color', 'color(srgb 0.84 0.913412 0.977412)');
  // Brand-on-quiet label uses kerf's AA-compliant #1a5dcf on the quiet fill (kerf 5.0.0-beta.15, HS2-228M1N).
  await expect(selectedOption).toHaveCSS('color', 'rgb(26, 93, 207)');
  await expect(selectedOption.locator('.kui-select__icon')).toHaveCSS('color', 'rgb(20, 184, 166)');
  await expect(category.locator('wa-option[value="bug"] [data-lucide="bug"]')).toBeVisible();
  await page.keyboard.press('Escape');
  await category.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'bug';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(form.locator('.kui-select__icon--selected [data-lucide="bug"]')).toBeVisible();
  await form.getByRole('button', { name: 'Add new ticket to Up Next' }).click();
  await expect(form.getByRole('button', { name: 'Remove new ticket from Up Next' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  expect(
    await form.evaluate((node) => {
      const style = getComputedStyle(node),
        metadata = getComputedStyle(node.querySelector('.quick-ticket-composer__metadata')!),
        details = getComputedStyle(node.querySelector('.quick-ticket-composer__details')!),
        textarea = getComputedStyle(node.querySelector('textarea')!),
        attachments = getComputedStyle(node.querySelector('.quick-ticket-composer__attachments')!),
        headerLabel = getComputedStyle(node.querySelector('.quick-ticket-composer__attachments header label')!),
        drop = getComputedStyle(node.querySelector('.quick-ticket-composer__drop')!),
        footer = getComputedStyle(node.querySelector('.quick-ticket-composer__footer')!),
        actions = getComputedStyle(node.querySelector('.quick-ticket-composer__actions')!),
        source = getComputedStyle(node.querySelector('.quick-ticket-composer__source')!);
      return {
        padding: style.paddingTop,
        regionGap: style.gap,
        metadataGap: metadata.gap,
        detailsGap: details.gap,
        textareaPadding: [textarea.paddingTop, textarea.paddingLeft],
        attachmentsGap: attachments.gap,
        headerLabelGap: headerLabel.gap,
        dropPadding: [drop.paddingTop, drop.paddingLeft],
        dropGap: drop.gap,
        footerGap: footer.gap,
        actionGap: actions.gap,
        sourceGap: source.gap,
      };
    }),
  ).toEqual({
    padding: '16px',
    regionGap: '16px',
    metadataGap: '8px',
    detailsGap: '4px',
    textareaPadding: ['8px', '16px'],
    attachmentsGap: '8px',
    headerLabelGap: '4px',
    dropPadding: ['8px', '16px'],
    dropGap: '8px',
    footerGap: '16px',
    actionGap: '8px',
    sourceGap: '8px',
  });
  await form.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-quick-ticket-composer-wide.png' });
  await page.setViewportSize({ width: 560, height: 760 });
  await expect(form).toBeVisible();
  await form.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-quick-ticket-composer-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await form.getByRole('button', { name: 'Create ticket' }).click();
  const createdList = page.getByRole('listbox', { name: 'Recently updated tickets' });
  await expect(createdList).toContainText('Created from the UX demo');
  await expect(
    createdList.locator('[data-component="ticket-list-row"]').first().locator('[data-lucide="bug"]'),
  ).toBeVisible();
  await expect(
    createdList
      .locator('[data-component="ticket-list-row"]')
      .first()
      .getByRole('button', { name: 'Remove from Up Next' }),
  ).toBeVisible();
  await expect(page.getByText(/HS2-DEMO\d created in Hot Sheet git/)).toBeVisible();
  // HS2-NZMJBJ: several writable sources show a source Select; the last-used one is preselected.
  await page.getByRole('button', { name: /New ticket/ }).click();
  const source = form.locator('wa-select[name="new-ticket-source"]');
  const sourceWarning = form.locator('.quick-ticket-composer__source-warning [data-component="state-banner"]');
  await expect(source).toHaveJSProperty('value', 'git-local');
  await expect(sourceWarning).toHaveCount(0);
  await source.click();
  await source.locator('wa-option[value="github-issues"]').click();
  await expect(source).toHaveJSProperty('value', 'github-issues');
  await expect(sourceWarning).toContainText('Creating in GitHub issues');
  await expect(sourceWarning).toContainText('not the default ticket source (Hot Sheet git)');
  await form.getByRole('textbox', { name: 'Ticket title' }).fill('Routed to GitHub');
  await form.getByRole('button', { name: 'Create ticket' }).click();
  await expect(page.getByText(/HS2-DEMO\d created in GitHub issues/)).toBeVisible();
  await page.getByRole('button', { name: /New ticket/ }).click();
  await expect(source).toHaveJSProperty('value', 'github-issues');
  await expect(sourceWarning).toBeVisible();
  await page.getByRole('button', { name: /Cancel/ }).click();
  await expect(source).toHaveCount(0);
  // The demo settings switch to the single-source variant, which keeps the plain label.
  await page.locator('[data-action="toggle-settings"][aria-expanded="false"]').click();
  const sourceCount = page.locator('[data-settings="quick-ticket-composer"] wa-select[name="composer-source-count"]');
  await expect(sourceCount).toHaveJSProperty('value', 'several');
  await sourceCount.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'one';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(sourceCount).toHaveJSProperty('value', 'one');
  await page.getByRole('button', { name: /New ticket/ }).click();
  await expect(form.locator('.quick-ticket-composer__footer')).toContainText('Creating in Hot Sheet git');
  await expect(source).toHaveCount(0);
  await expect(sourceWarning).toHaveCount(0);
  await page.getByRole('button', { name: /Cancel/ }).click();
  await sourceCount.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'several';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await page.getByRole('button', { name: /New ticket/ }).click();
  await expect(source).toHaveJSProperty('value', 'github-issues');
  await expect(sourceWarning).toBeVisible();
  await page.getByRole('button', { name: /Cancel/ }).click();
  // HS2-8HHHK3: files staged for git stay listed but block Create on attachment-less GitHub issues.
  await page.getByRole('button', { name: /New ticket/ }).click();
  await expect(source).toHaveJSProperty('value', 'github-issues');
  await source.click();
  await source.locator('wa-option[value="git-local"]').click();
  await expect(source).toHaveJSProperty('value', 'git-local');
  await expect(sourceWarning).toHaveCount(0);
  await form
    .getByLabel('Browse attachments for new ticket', { exact: true })
    .setInputFiles({ name: 'demo-proof.png', mimeType: 'image/png', buffer: Buffer.from('demo') });
  await expect(form.getByText('demo-proof.png')).toBeVisible();
  const demoCreate = form.locator('wa-button[type="submit"]'),
    demoStranded = form.locator('[data-new-ticket-attachments-stranded="true"]'),
    demoRemoveAll = form.getByRole('button', { name: 'Remove all staged attachments' });
  await expect(demoCreate).toHaveJSProperty('disabled', false);
  await source.click();
  await source.locator('wa-option[value="github-issues"]').click();
  await expect(source).toHaveJSProperty('value', 'github-issues');
  await expect(demoStranded).toContainText('GitHub issues does not support attachments');
  await expect(form.getByText('demo-proof.png')).toBeVisible();
  await expect(demoCreate).toHaveJSProperty('disabled', true);
  await form.screenshot({ path: 'target/visual-captures/hs2-8hhhk3-ux-demo-stranded.png' });
  await source.click();
  await source.locator('wa-option[value="git-local"]').click();
  await expect(demoStranded).toHaveCount(0);
  await expect(form.getByText('demo-proof.png')).toBeVisible();
  await expect(demoCreate).toHaveJSProperty('disabled', false);
  await source.click();
  await source.locator('wa-option[value="github-issues"]').click();
  await demoRemoveAll.click();
  await expect(page.getByText('Staged attachments removed')).toBeVisible();
  await expect(form.getByText('demo-proof.png')).toHaveCount(0);
  await expect(demoCreate).toHaveJSProperty('disabled', false);
  await page.getByRole('button', { name: /Cancel/ }).click();
  await page.getByRole('button', { name: /New ticket/ }).click();
  await page.getByRole('textbox', { name: 'Ticket title' }).fill('Discard this');
  const cancel = page.getByRole('button', { name: /Cancel/ });
  await expect(cancel.locator('[data-lucide="x"]')).toHaveCount(0);
  await cancel.click();
  await expect(page.getByText('Ticket creation cancelled')).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Ticket title' })).toHaveCount(0);
  const launcher = page.getByRole('button', { name: /New ticket/ });
  await expect(launcher).toHaveCSS('cursor', 'pointer');
  await launcher.click();
  await expect(page.getByRole('textbox', { name: 'Ticket title' })).toBeFocused();
  await expect(page.getByRole('textbox', { name: 'Details' })).toHaveValue('');
  await expect(page.getByRole('button', { name: 'Add new ticket to Up Next' })).toHaveAttribute(
    'aria-pressed',
    'false',
  );
});

test('keeps QuickTicketComposer modal focus and dismissal in Web Awesome lifecycle order', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=quick-ticket-composer');
  const launcher = page.getByRole('button', { name: /New ticket/ });
  const dialog = page.getByRole('dialog', { name: 'Create ticket' });
  await launcher.click();
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog.getByRole('textbox', { name: 'Ticket title' })).toBeFocused();
  expect(await dialog.evaluate((node) => node.shadowRoot?.querySelector('dialog')?.matches(':modal'))).toBe(true);

  await page.evaluate(() => {
    const target = document.querySelector<HTMLElement>('.kui-workbench__rail--left')!;
    target.tabIndex = -1;
    target.focus();
  });
  expect(await dialog.evaluate((host) => host.contains(document.activeElement))).toBe(true);
  await page.mouse.click(8, 8);
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(launcher).not.toBeFocused();
  const title = dialog.getByRole('textbox', { name: 'Ticket title' });
  const close = dialog.getByRole('button', { name: 'Close' });
  await title.focus();
  await page.keyboard.press('Shift+Tab');
  await expect(close).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(title).toBeFocused();

  const category = dialog.locator('wa-select[name="new-ticket-category"]');
  await category.click();
  await expect(category).toHaveJSProperty('open', true);
  await page.keyboard.press('Escape');
  await expect(category).toHaveJSProperty('open', false);
  await page.waitForTimeout(200);
  await expect(dialog).toHaveJSProperty('open', true);
  await page.screenshot({ path: 'target/visual-captures/hs2-sq71gk-composer-modal-wide.png', fullPage: true });
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await expect(launcher).toBeFocused();

  await launcher.click();
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(dialog).toBeHidden();
  await expect(launcher).toBeFocused();

  await page.addStyleTag({
    content:
      'body{min-width:0}.demo-shell{display:block}.kui-workbench__rail--left,.kui-workbench__main > .kui-pane > .kui-pane__header,.kui-workbench__main > .kui-pane > .kui-pane__footer,.settings-toggle{display:none}.kui-workbench__main{min-height:0;padding:12px}',
  });
  await launcher.click();
  await expect
    .poll(() =>
      dialog.evaluate((node) => ({
        nativeOpen: node.shadowRoot?.querySelector('dialog')?.open,
        runningAnimations: node.shadowRoot
          ?.querySelector('dialog')
          ?.getAnimations()
          .filter((animation) => animation.playState === 'running').length,
      })),
    )
    .toEqual({ nativeOpen: true, runningAnimations: 0 });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(dialog.getByRole('button', { name: 'Create ticket' })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-sq71gk-composer-modal-390x844.png' });
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Emulation.setDeviceMetricsOverride', {
    width: 195,
    height: 422,
    deviceScaleFactor: 2,
    mobile: false,
  });
  await expect(dialog.getByRole('button', { name: 'Create ticket' })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-sq71gk-composer-modal-200-percent.png' });

  await page.goto('/ux-demo?component=ticket-row');
  await expect
    .poll(() => page.evaluate(() => document.documentElement.classList.contains('wa-scroll-lock')))
    .toBe(false);
});

test('enters, autosaves, and re-enters reader title editing in the TicketReader demo (HS2-0VFPD5)', async ({
  page,
}) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=ticket-reader');
    const reader = page.locator('[data-component="ticket-reader"]');
    const heading = reader.getByRole('heading', { name: 'Build TicketReader component and UX demo' });
    await expect(heading).toBeVisible();
    // The editor keeps the static reader title's typography and reading width (HS2-R8M8HB), in both the
    // standard and the large reader text sizes.
    const typography = (node: Element) => {
      const style = getComputedStyle(node);
      return {
        fontSize: style.fontSize,
        fontWeight: style.fontWeight,
        lineHeight: style.lineHeight,
        maxWidth: style.maxWidth,
      };
    };
    const textSizeToggle = reader.locator('[data-action="toggle-reader-text-size"]');
    for (const largeText of [true, false]) {
      await textSizeToggle.click();
      await expect(reader).toHaveAttribute('data-large-text', String(largeText));
      const staticTitle = await heading.evaluate(typography);
      expect(parseFloat(staticTitle.fontSize)).toBeGreaterThan(16);
      await heading.dblclick();
      const editor = reader.locator('.ticket-inspector__title-input');
      await expect(editor).toBeFocused();
      expect(await editor.evaluate(typography)).toEqual(staticTitle);
      await editor.blur();
      await expect(heading).toBeVisible();
    }
    await heading.dblclick();
    const titleEditor = reader.locator('.ticket-inspector__title-input');
    await expect(titleEditor).toBeVisible();
    await expect(titleEditor).toBeFocused();
    await expect(titleEditor).toHaveJSProperty('value', 'Build TicketReader component and UX demo');
    const box = (await titleEditor.boundingBox())!;
    expect(box.x + box.width).toBeLessThanOrEqual(width);
    await reader.screenshot({ path: `target/visual-captures/hs2-0vfpd5-reader-title-editing-${width}.png` });
    await titleEditor.fill(`Reader title at ${width}`);
    await titleEditor.blur();
    await expect(reader.getByRole('heading', { name: `Reader title at ${width}` })).toBeVisible();
    await expect(reader.locator('.ticket-inspector__title-input')).toHaveCount(0);
    // Keyboard re-entry starts from the saved title, and the sidebar inspector's title is untouched.
    await reader.getByRole('heading', { name: `Reader title at ${width}` }).press('Enter');
    await expect(reader.locator('.ticket-inspector__title-input')).toHaveJSProperty(
      'value',
      `Reader title at ${width}`,
    );
    await reader.locator('.ticket-inspector__title-input').blur();
    await page.goto('/ux-demo?component=ticket-inspector');
    await expect(
      page.locator('[data-component="ticket-inspector"]').getByRole('heading', { name: /Build TicketList/ }),
    ).toBeVisible();
  }
});

test('wraps a long title in the reader and sidebar title editors at 1280 and 390 (HS2-98ZVPE)', async ({ page }) => {
  const longTitle = 'Ticket title editor clips long titles on one line at narrow widths in the reader and the sidebar';
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    for (const component of ['ticket-reader', 'ticket-inspector'] as const) {
      await page.goto(`/ux-demo?component=${component}`);
      const surface = page.locator(`[data-component="${component}"]`).first();
      await editLongTitleThroughWrappingEditor(
        page,
        surface,
        () => surface.locator('[data-action="edit-ticket-title"]').dblclick(),
        longTitle,
      );
      // Keyboard re-entry still opens the editor on the saved single-line title.
      await surface.locator('[data-action="edit-ticket-title"]').press('Enter');
      const editor = surface.getByRole('textbox', { name: 'Ticket title' });
      await expect(editor).toBeFocused();
      expect(await editor.evaluate((node: HTMLTextAreaElement) => node.value.includes('\n'))).toBe(false);
      await surface
        .locator('[data-component="ticket-inspector-header"]')
        .screenshot({ path: test.info().outputPath(`hs2-98zvpe-${component}-editing-${width}.png`) });
      await editor.blur();
    }
  }
});

test('navigates, toggles, closes, and reopens TicketInspector', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-inspector');
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await expect(inspector).toContainText('Build TicketList and TicketBoard');
  const detailsParagraph = inspector.locator('.ticket-info-panel__details-surface .markdown-preview > p').first();
  await expect(detailsParagraph).toHaveCSS('margin-top', '16px');
  await expect(detailsParagraph).toHaveCSS('margin-bottom', '16px');
  await inspector.getByRole('heading', { name: /Build TicketList/ }).dblclick();
  const titleEditor = inspector.getByRole('textbox', { name: 'Ticket title' });
  await titleEditor.fill('Autosaved inspector title');
  await titleEditor.blur();
  await expect(inspector.getByRole('heading', { name: 'Autosaved inspector title' })).toBeVisible();
  await inspector.getByRole('button', { name: 'Add tag' }).click();
  const tagDialog = page.getByRole('dialog', { name: 'Add tag' }),
    tagEditor = tagDialog.getByRole('combobox', { name: 'Tag name' });
  await expect(tagEditor).toBeFocused();
  expect(
    await tagDialog.evaluate((node) => {
      const editor = getComputedStyle(node.parentElement!),
        popover = getComputedStyle(node),
        heading = getComputedStyle(node.querySelector('strong')!),
        label = getComputedStyle(node.querySelector('label')!),
        input = getComputedStyle(node.querySelector('input')!),
        help = getComputedStyle(node.querySelector('small')!);
      return {
        editorGap: editor.gap,
        popoverMargin: popover.marginTop,
        popoverPadding: popover.paddingTop,
        headingMargin: heading.marginBottom,
        labelGap: label.gap,
        inputPadding: input.paddingLeft,
        helpMargin: help.marginTop,
      };
    }),
  ).toEqual({
    editorGap: '8px',
    popoverMargin: '4px',
    popoverPadding: '16px',
    headingMargin: '16px',
    labelGap: '4px',
    inputPadding: '16px',
    helpMargin: '8px',
  });
  await tagDialog.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-ticket-tag-editor-wide.png' });
  const addTagButton = inspector.getByRole('button', { name: 'Add tag' });
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 390, height: 844 });
  await addTagButton.scrollIntoViewIfNeeded();
  await addTagButton.click();
  await expect(tagEditor).toBeFocused();
  await tagDialog.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-ticket-tag-editor-narrow.png' });
  await page.keyboard.press('Escape');
  await page.setViewportSize({ width: 1280, height: 720 });
  await addTagButton.scrollIntoViewIfNeeded();
  await addTagButton.click();
  await expect(tagEditor).toBeFocused();
  await tagEditor.fill('regression');
  await tagEditor.press('Enter');
  await expect(inspector.locator('[data-component="tag-chip"][data-tag-id="regression"]')).toBeVisible();
  await inspector.locator('[data-component="tag-chip"][data-tag-id="client"] [data-action="remove-tag-chip"]').click();
  await expect(inspector.locator('[data-component="tag-chip"][data-tag-id="client"]')).toHaveCount(0);
  await expect(inspector.getByRole('tab', { name: 'Info' })).toHaveAttribute('aria-selected', 'true');
  expect(
    await inspector.evaluate((node) => {
      const header = getComputedStyle(node.querySelector('.ticket-inspector__header')!),
        title = getComputedStyle(node.querySelector('.ticket-inspector__title')!),
        tabs = getComputedStyle(node.querySelector('.ticket-inspector__tabs')!),
        tabsFrame = getComputedStyle(node.querySelector('.ticket-inspector__tabs-frame')!),
        tabRail = getComputedStyle(node.querySelector('.ticket-inspector__tabs .kui-tab-bar__tabs')!),
        tabSelect = getComputedStyle(node.querySelector('.ticket-inspector__tabs .kui-app-tab__select')!);
      return {
        headerBottom: header.paddingBottom,
        titleMargin: [title.marginTop, title.marginLeft, title.marginBottom],
        tabsMargin: [tabs.marginRight, tabs.marginBottom],
        tabsInset: [tabsFrame.paddingLeft, tabsFrame.paddingRight, tabsFrame.paddingBottom],
        tabsPadding: tabs.paddingTop,
        tabRailPadding: tabRail.paddingTop,
        tabGap: tabSelect.gap,
      };
    }),
  ).toEqual({
    // The header holds the title, notices, and tabs under the panel toolbar (HS2-QQW6CT), so the
    // title owns the space below it.
    headerBottom: '0px',
    titleMargin: ['4px', '16px', '16px'],
    tabsMargin: ['0px', '0px'],
    tabsInset: ['8px', '8px', '8px'],
    tabsPadding: '0px',
    tabRailPadding: '1px',
    tabGap: '4px',
  });
  await page.keyboard.press('Escape');
  const captureInspectorShell = async (path: string) => {
    await inspector.evaluate((node) => {
      node.style.height = '560px';
      node.style.minHeight = '0';
      node.style.flex = 'none';
    });
    await inspector.screenshot({ path });
    await inspector.evaluate((node) => {
      node.style.height = '';
      node.style.minHeight = '';
      node.style.flex = '';
    });
  };
  await captureInspectorShell('target/visual-captures/hs2-4y6sm9-ticket-inspector-wide.png');
  await page.setViewportSize({ width: 390, height: 844 });
  await captureInspectorShell('target/visual-captures/hs2-4y6sm9-ticket-inspector-narrow.png');
  await page.setViewportSize({ width: 1280, height: 720 });
  await expect(inspector.locator('[data-component="status-badge"]')).toBeVisible();
  await expect(inspector.locator('wa-select[name="inspector-category"] [data-lucide="sparkles"]')).toHaveCount(2);
  await expect(inspector.locator('wa-select[name="inspector-priority"] [data-lucide="chevron-up"]')).toHaveCount(2);
  await expect(
    inspector.locator('.ticket-category-select .kui-select__icon--selected [data-lucide="sparkles"]'),
  ).toBeVisible();
  await expect(
    inspector.locator('.ticket-priority-select .kui-select__icon--selected [data-lucide="chevron-up"]'),
  ).toBeVisible();
  const selectedSpacing = await inspector.locator('wa-select[name="inspector-category"]').evaluate((node) => {
    const icon = node.querySelector<HTMLElement>('.kui-select__icon--selected')!.getBoundingClientRect();
    const input = node.shadowRoot!.querySelector<HTMLElement>('[part~="display-input"]')!.getBoundingClientRect();
    return {
      actual: input.left - icon.right,
      expected: Number.parseFloat(getComputedStyle(document.documentElement).fontSize) * 0.5,
    };
  });
  expect(selectedSpacing.actual).toBeCloseTo(selectedSpacing.expected, 1);
  const category = inspector.locator('wa-select[name="inspector-category"]');
  const fieldLabelGeometry = await category.evaluate((node) => {
    const label = node.shadowRoot!.querySelector<HTMLElement>('[part~="form-control-label"]')!;
    const combobox = node.shadowRoot!.querySelector<HTMLElement>('[part~="combobox"]')!;
    const labelStyle = getComputedStyle(label);
    const comboboxStyle = getComputedStyle(combobox);
    return {
      labelInset: Number.parseFloat(labelStyle.paddingInlineStart),
      valueInset:
        Number.parseFloat(comboboxStyle.borderInlineStartWidth) + Number.parseFloat(comboboxStyle.paddingInlineStart),
      textTransform: labelStyle.textTransform,
      fontWeight: labelStyle.fontWeight,
    };
  });
  expect(fieldLabelGeometry.labelInset).toBeCloseTo(fieldLabelGeometry.valueInset, 1);
  expect(fieldLabelGeometry.labelInset).toBeCloseTo(9, 1);
  expect(fieldLabelGeometry.textTransform).toBe('uppercase');
  expect(fieldLabelGeometry.fontWeight).toBe('650');
  const selectCaret = await category.evaluate((node) => {
    const caret = node.shadowRoot?.querySelector<HTMLElement>('[part~="expand-icon"]');
    return caret ? { transform: getComputedStyle(caret).transform, width: caret.getBoundingClientRect().width } : null;
  });
  expect(selectCaret?.transform).toContain('0.5');
  await category.click();
  const longOption = category.locator('wa-option[value="requirement_change"]');
  const optionLayout = await longOption.evaluate((node) => {
    const label = node.shadowRoot?.querySelector<HTMLElement>('[part~="label"]');
    return label
      ? { whiteSpace: getComputedStyle(label).whiteSpace, optionHeight: node.getBoundingClientRect().height }
      : null;
  });
  expect(optionLayout?.whiteSpace).toBe('normal');
  expect(optionLayout!.optionHeight).toBeGreaterThan(40);
  for (const value of ['task', 'requirement_change']) {
    const centerDelta = await category.locator(`wa-option[value="${value}"]`).evaluate((node) => {
      const label = node.shadowRoot!.querySelector<HTMLElement>('[part~="label"]')!.getBoundingClientRect();
      const start = node.shadowRoot!.querySelector<HTMLElement>('[part~="start"]')!.getBoundingClientRect();
      return Math.abs(label.top + label.height / 2 - (start.top + start.height / 2));
    });
    expect(centerDelta).toBeLessThan(1);
  }
  await page.keyboard.press('Escape');
  await category.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'bug';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(
    inspector.locator('.ticket-category-select .kui-select__icon--selected [data-lucide="bug"]'),
  ).toBeVisible();
  await expect(
    inspector.locator('.ticket-category-select .kui-select__icon--selected [data-lucide="sparkles"]'),
  ).toHaveCount(0);
  const priority = inspector.locator('wa-select[name="inspector-priority"]');
  await priority.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'low';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(
    inspector.locator('.ticket-priority-select .kui-select__icon--selected [data-lucide="chevron-down"]'),
  ).toBeVisible();
  await expect(
    inspector.locator('.ticket-priority-select .kui-select__icon--selected [data-lucide="chevron-up"]'),
  ).toHaveCount(0);
  await inspector
    .locator('.ticket-info-panel__metadata')
    .screenshot({ path: 'target/visual-captures/hs2-trqdh2-inspector-fields-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(inspector.locator('wa-select[name="inspector-category"]')).toBeVisible();
  await expect(inspector.locator('wa-select[name="inspector-priority"]')).toBeVisible();
  await inspector
    .locator('.ticket-info-panel__metadata')
    .screenshot({ path: 'target/visual-captures/hs2-trqdh2-inspector-fields-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 720 });
  const star = inspector.getByRole('button', { name: 'Remove from Up Next' });
  await star.click();
  await expect(inspector.getByRole('button', { name: 'Add to Up Next' })).toBeVisible();
  const statusTrigger = inspector.locator('[data-action="open-inspector-status-menu"]');
  const statusMenu = inspector.locator('[data-inspector-status-menu]');
  await expect(statusTrigger).toHaveAttribute('aria-label', 'Change status, Planning');
  await expect(statusTrigger).toHaveAttribute('data-status', 'started');
  await expect(statusTrigger).toHaveCSS('font-weight', '600');
  // The native badge trigger keeps a visible keyboard focus ring.
  await statusTrigger.evaluate((node: HTMLElement) => {
    node.focus({ focusVisible: true } as FocusOptions);
  });
  await expect
    .poll(() =>
      statusTrigger.evaluate((node) => {
        const style = getComputedStyle(node);
        return style.outlineStyle !== 'none' && parseFloat(style.outlineWidth) > 0;
      }),
    )
    .toBe(true);
  await statusTrigger.evaluate((node: HTMLElement) => {
    node.blur();
  });
  await statusTrigger.click();
  await expect(statusMenu.locator('[data-ticket-status] [data-lucide]')).toHaveCount(6);
  await expect(statusMenu.locator('wa-divider')).toHaveCount(1);
  expect(
    await statusMenu
      .locator('[data-ticket-status]')
      .evaluateAll((options) => options.map((option) => option.getAttribute('data-ticket-status'))),
  ).toEqual(['not_started', 'started', 'completed', 'verified', 'backlog', 'archive']);
  await statusMenu.locator('[data-ticket-status="completed"]').click();
  await expect(inspector.locator('[data-component="status-badge"]')).toHaveAttribute('data-status', 'completed');
  await expect(statusTrigger).toHaveAttribute('aria-label', 'Change status, Completed');
  await expect(statusTrigger).toHaveAttribute('data-status', 'completed');
  await expect(inspector.locator('[data-component="ticket-info-panel"]')).toBeVisible();
  const sectionRhythm = await inspector.locator('[data-component="ticket-info-panel"]').evaluate((node) =>
    [...node.querySelectorAll<HTMLElement>('.ticket-info-panel__section')].map((section) => ({
      gap: getComputedStyle(section).rowGap,
      headerHeight: section.querySelector('header')?.getBoundingClientRect().height,
    })),
  );
  expect(sectionRhythm).toHaveLength(3);
  expect(sectionRhythm.map((section) => section.gap)).toEqual(['8px', '6px', '8px']);
  expect(sectionRhythm[0].headerHeight).toBeUndefined();
  const categoryLabelHeight = await inspector
    .locator('wa-select[name="inspector-category"]')
    .evaluate((node) => node.shadowRoot!.querySelector('[part~="form-control-label"]')!.getBoundingClientRect().height);
  expect(sectionRhythm[1].headerHeight).toBeCloseTo(categoryLabelHeight + 16, 1);
  expect(sectionRhythm[2].headerHeight).toBeCloseTo(44, 1);
  await expect(inspector.getByRole('button', { name: 'Block ticket' })).toBeVisible();
  await inspector.getByRole('button', { name: 'Block ticket' }).click();
  const blockedReason = inspector.getByRole('textbox', { name: 'Blocked reason' });
  await expect(blockedReason).toBeFocused();
  await blockedReason.fill('Waiting for API review.');
  await blockedReason.blur();
  await expect(inspector.locator('[data-component="blocked-badge"]')).toHaveText('Blocked');
  await expect(inspector.getByText('Waiting for API review.')).toBeVisible();
  const blockedSection = inspector.locator('.ticket-info-panel__blocked-section');
  const detailsSection = inspector.locator('.ticket-info-panel__details-section');
  expect((await blockedSection.boundingBox())!.y).toBeLessThan((await detailsSection.boundingBox())!.y);
  await expect(inspector.locator('[data-component="ticket-notes"] [data-component="note-card"]')).toHaveCount(5);
  const firstNote = inspector.locator('[data-component="ticket-notes"] [data-component="note-card"]').first();
  const firstNoteBody = firstNote.locator('.note-card__body');
  await expect(firstNoteBody).toHaveAttribute('aria-label', 'Edit note');
  await expect(firstNote.locator('[data-action="open-ticket-reader"]')).toHaveCount(0);
  await firstNoteBody.press('Enter');
  await expect(firstNote.getByRole('textbox', { name: 'Note body' })).toBeFocused();
  await firstNote.getByRole('textbox', { name: 'Note body' }).fill('Autosaved inspector note');
  await firstNote.getByRole('textbox', { name: 'Note body' }).blur();
  await expect(firstNote).toContainText('Autosaved inspector note');
  await inspector.getByRole('button', { name: 'Add note', exact: true }).last().click();
  await expect(page.getByText('Note composer requested')).toBeVisible();
  await inspector.getByRole('tab', { name: 'Timeline' }).click();
  await expect(inspector.locator('[data-component="ticket-timeline"]')).toBeVisible();
  await expect(inspector.getByRole('heading', { name: 'Timeline' })).toBeVisible();
  await expect(inspector.locator('.ticket-timeline__list > li')).toHaveCount(4);
  await expect(inspector.getByText('4 events total')).toBeVisible();
  await expect(inspector.locator('.ticket-timeline__list > li').first()).toContainText('Claude started work');
  await inspector.getByRole('tab', { name: 'Code Review' }).click();
  await expect(inspector.locator('[data-component="ticket-code-review"] .ticket-code-review__commit')).toHaveCount(2);
  await inspector.getByRole('button', { name: 'Open 2 commit bundle 92ed71a through c4a38be in Glassbox' }).click();
  await expect(page.getByText('Commit range opened in Glassbox')).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-pg1hkj-code-review-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(inspector.locator('[data-component="ticket-code-review"]')).toBeVisible();
  await expect(inspector.locator('[data-commit-sha]').first().locator('strong')).toHaveCSS('overflow-wrap', 'anywhere');
  await page.screenshot({ path: 'target/visual-captures/hs2-pg1hkj-code-review-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 720 });
  await inspector.getByRole('tab', { name: 'Attachments' }).click();
  await expect(inspector.locator('[data-component="ticket-attachments"]')).toBeVisible();
  await expect(inspector.getByRole('heading', { name: 'Attachments' })).toBeVisible();
  await expect(inspector.locator('[data-attachment-id]')).toHaveCount(2);
  await expect(inspector.getByLabel('2 attachments')).toBeVisible();
  await expect(inspector.locator('[data-action="toggle-inspector-up-next"]')).toHaveCount(0);
  await inspector.getByRole('button', { name: 'Hide ticket inspector' }).click();
  await expect(inspector).toHaveCount(0);
  await page.getByRole('button', { name: 'Open ticket inspector' }).click();
  const reopened = page.locator('[data-component="ticket-inspector"]');
  await expect(reopened).toBeVisible();
  await reopened.getByRole('tab', { name: 'Info' }).click();
  await expect(
    reopened.locator('.ticket-category-select .kui-select__icon--selected [data-lucide="bug"]'),
  ).toBeVisible();
  await expect(
    reopened.locator('.ticket-priority-select .kui-select__icon--selected [data-lucide="chevron-down"]'),
  ).toBeVisible();
  await expect(reopened.locator('[data-component="status-badge"]')).toHaveAttribute('data-status', 'completed');
  await expect(reopened.getByRole('button', { name: 'Open ticket reader', exact: true })).toBeVisible();
  await reopened.getByRole('button', { name: 'Open ticket reader', exact: true }).click();
  await expect(page).toHaveURL('/ux-demo?component=ticket-reader');
  await expect(page.locator('[data-component="ticket-reader"]')).toBeVisible();
});

test('uses canonical spacing in the standalone TicketCodeReview demo (HS2-4Y6SM9, HS2-737H3X)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-code-review&dev-review=false');
  const review = page.locator('[data-component="ticket-code-review"]').first();
  await expect(review).toBeVisible();
  const spacing = await review.evaluate((node) => {
    const heading = getComputedStyle(
        node.querySelector<HTMLElement>('.ticket-code-review__header .kui-toolbar__leading')!,
      ),
      evidence = getComputedStyle(node.querySelector<HTMLElement>('.ticket-code-review__evidence')!),
      evidenceItem = getComputedStyle(node.querySelector<HTMLElement>('.ticket-code-review__evidence-grid span')!),
      range = getComputedStyle(node.querySelector<HTMLElement>('.ticket-code-review__range')!),
      rangeItem = getComputedStyle(node.querySelector<HTMLElement>('.ticket-code-review__range-item')!),
      commit = getComputedStyle(node.querySelector<HTMLElement>('.ticket-code-review__commit')!),
      summary = getComputedStyle(node.querySelector<HTMLElement>('.ticket-code-review__commit-summary')!);
    return {
      headingGap: heading.gap,
      evidenceMargin: evidence.marginBottom,
      evidencePadding: evidence.padding,
      evidenceItemGap: evidenceItem.gap,
      rangePadding: range.padding,
      rangeGap: range.gap,
      rangeItemMargin: rangeItem.margin,
      commitPadding: commit.padding,
      commitGap: commit.gap,
      summaryGap: summary.gap,
    };
  });
  expect(spacing).toEqual({
    // The heading Toolbar drops its gap token; each ToolbarText keeps its own inline padding.
    headingGap: '0px',
    evidenceMargin: '16px',
    evidencePadding: '8px',
    evidenceItemGap: '4px',
    rangePadding: '8px',
    rangeGap: '8px',
    rangeItemMargin: '8px 0px 4px',
    commitPadding: '8px 0px',
    commitGap: '8px',
    summaryGap: '4px',
  });
  await review.screenshot({ path: 'target/visual-captures/hs2-737h3x-ticket-code-review-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect
    .poll(() =>
      review.evaluate((node) => {
        const box = node.getBoundingClientRect();
        return box.left >= 0 && box.right <= innerWidth;
      }),
    )
    .toBe(true);
  await review.screenshot({ path: 'target/visual-captures/hs2-737h3x-ticket-code-review-narrow.png' });
});

test('renders standalone ticket metadata and inspector-section demos', async ({ page }) => {
  for (const [id, component] of [
    ['ticket-category-select', 'ticket-category-select'],
    ['ticket-priority-select', 'ticket-priority-select'],
    ['ticket-status-menu', 'ticket-status-menu'],
    ['ticket-info-panel', 'ticket-info-panel'],
    ['ticket-timeline', 'ticket-timeline'],
    ['ticket-code-review', 'ticket-code-review'],
    ['attachment-gallery', 'attachment-gallery'],
    ['ticket-attachments', 'ticket-attachments'],
  ] as const) {
    await page.goto(`/ux-demo?component=${id}`);
    await expect(
      page
        .locator(`[data-component="${component}"]`)
        .or(page.locator(`.${component}`))
        .first(),
    ).toBeVisible();
  }
  await expect(page.getByRole('button', { name: 'Edit batch label Brian · Round 1 · Problem evidence' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Edit batch label Corrected implementation' })).toBeVisible();
  await expect(
    page.getByRole('button', { name: 'Edit batch label System · Batch 1 · Problem evidence' }),
  ).toBeVisible();
  await expect(page.getByRole('button', { name: 'Edit batch label Legacy / Uncategorized' })).toBeVisible();
  const annotatedCard = page.getByRole('button', { name: 'Open wide-layout.svg in media gallery, 2 annotations' });
  await expect(annotatedCard.locator('.ticket-attachments__annotation-marker [data-lucide="pencil"]')).toBeVisible();
  await page
    .locator('.kui-workbench__main')
    .screenshot({ path: 'target/visual-captures/hs2-6fp1kt-attachment-batches-final-wide.png' });
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() =>
          requestAnimationFrame(() => {
            resolve();
          }),
        ),
      ),
  );
  // The demo also renders the append-only variant (HS2-HSA64D); drive the editable one.
  const editableDemo = page.getByRole('region', { name: 'TicketAttachments demo', exact: true }),
    surface = editableDemo.locator('[data-component="ticket-attachments"]').first(),
    dragged = surface.locator('[data-component="ticket-attachment-item"][data-drag-attachment-id="wide"]');
  await dragged.evaluate((node) =>
    node.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: new DataTransfer() })),
  );
  const newGroup = editableDemo.locator('[data-attachment-new-group-drop-target]');
  await expect(newGroup).toBeVisible();
  await expect(page.locator('input[type="checkbox"]')).toHaveCount(0);
  await surface.screenshot({ path: 'target/visual-captures/hs2-c0r4mx-drag-new-group.png' });
  await newGroup.evaluate((node) =>
    node.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: new DataTransfer() })),
  );
  await expect(editableDemo.locator('[data-attachment-group-drop-target]')).toHaveCount(5);
  await expect(page.getByRole('button', { name: 'Edit batch label New group' })).toBeVisible();
  await surface.screenshot({ path: 'target/visual-captures/hs2-c0r4mx-regrouped-final.png' });
  const item = page.locator('[data-attachment-id="demo-video"]'),
    trigger = item.getByRole('button', { name: 'More actions for choppy.mov' });
  await expect(item.getByRole('button')).toHaveCount(1);
  await expect(trigger).toHaveAttribute('title', 'More actions for choppy.mov');
  await expect(trigger.locator('[data-lucide="more-horizontal"]')).toBeVisible();
  await trigger.click();
  let menu = page.getByRole('menu', { name: 'Attachment actions' });
  await expect(menu.getByRole('menuitem')).toHaveCount(6);
  await expect(menu.getByRole('menuitem').allTextContents()).resolves.toEqual([
    'Open',
    'Download',
    'Copy reference',
    'Rename',
    'Show in file manager',
    'Remove',
  ]);
  const gridVideo = page.locator('.ticket-attachments__image-grid video').first();
  await expect(gridVideo).toBeVisible();
  await expect(gridVideo).toHaveAttribute('poster', '/ux-gallery-preview.svg?variant=video');
  expect(await gridVideo.evaluate((node) => (node as HTMLVideoElement).paused)).toBe(true);
  expect(await gridVideo.evaluate((node) => (node as HTMLVideoElement).autoplay)).toBe(false);
  await menu.getByRole('menuitem', { name: 'Copy reference' }).click();
  await expect(menu).toHaveCount(0);
  await surface.screenshot({ path: 'target/visual-captures/hs2-j978e9-annotation-grid-marker.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  // Kerf 5.0.0-beta.56 collapses the catalog sidebar into a transient overlay on a small screen.
  await expect(page.locator('[data-component="catalog"]')).toHaveAttribute('data-sidebar-collapsed', 'true');
  const batchHeader = page.locator('.ticket-attachments__batch > header').first(),
    batchTitle = batchHeader.locator('.ticket-attachments__batch-title'),
    batchPurpose = batchHeader.locator('select');
  await expect
    .poll(async () => ((await batchPurpose.boundingBox())?.y ?? 0) - ((await batchTitle.boundingBox())?.y ?? 0))
    .toBeGreaterThan(0);
  await item.scrollIntoViewIfNeeded();
  await item.click({ button: 'right' });
  menu = page.getByRole('menu', { name: 'Attachment actions' });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitem')).toHaveCount(6);
  await page.screenshot({
    path: 'target/visual-captures/hs2-6fp1kt-attachment-batches-final-narrow.png',
    fullPage: true,
  });
});

test('styles and edits attachment group labels while preserving drag regrouping', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-attachments');
  const surface = page
      .getByRole('region', { name: 'TicketAttachments demo', exact: true })
      .locator('[data-component="ticket-attachments"]'),
    groups = surface.locator('[data-attachment-group-drop-target]'),
    first = groups.first();
  await expect(first).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(surface.locator('[data-lucide="grip-vertical"]')).toHaveCount(0);
  const title = first.getByRole('button', { name: /Edit batch label/ }),
    purpose = first.locator('select');
  await expect(title).toHaveCSS('font-size', '16px');
  expect((await purpose.boundingBox())!.width).toBeLessThan((await first.locator('header').boundingBox())!.width);
  await title.dblclick();
  const editor = first.getByRole('textbox', { name: /Batch label/ });
  await expect(editor).toBeFocused();
  await editor.fill('Discarded label');
  await editor.press('Escape');
  await expect(first.getByRole('button', { name: /Brian · Round 1 · Problem evidence/ })).toBeVisible();
  await expect(first.getByRole('button', { name: /Brian · Round 1 · Problem evidence/ })).toBeFocused();
  await first.getByRole('button', { name: /Edit batch label/ }).dblclick();
  await editor.fill('Human review evidence');
  await editor.press('Enter');
  await expect(first.getByRole('button', { name: 'Edit batch label Human review evidence' })).toBeVisible();
  await expect(first.getByRole('button', { name: 'Edit batch label Human review evidence' })).toBeFocused();
  await page.locator('h2').first().click();
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() =>
          requestAnimationFrame(() => {
            resolve();
          }),
        ),
      ),
  );
  const dragged = surface.locator('[data-component="ticket-attachment-item"][data-drag-attachment-id]').first();
  await dragged.evaluate((node) =>
    node.dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: new DataTransfer() })),
  );
  const activeTarget = groups.nth(1);
  await activeTarget.evaluate((node) =>
    node.dispatchEvent(
      new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: new DataTransfer() }),
    ),
  );
  await expect(activeTarget).toHaveAttribute('data-drag-over', 'true');
  await expect(activeTarget).not.toHaveCSS('outline-style', 'none');
  await expect(surface.locator('[data-attachment-new-group-drop-target]')).toBeVisible();
  await surface.screenshot({ path: 'target/visual-captures/hs2-c0r4mx-feedback-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.mouse.move(2, 2);
  await expect(first.getByRole('button', { name: 'Edit batch label Human review evidence' })).toBeVisible();
  expect((await purpose.boundingBox())!.width).toBeLessThan((await first.locator('header').boundingBox())!.width);
  await surface.screenshot({ path: 'target/visual-captures/hs2-c0r4mx-feedback-narrow.png' });
});

test('keeps a demo attachment label edit open across a late catalog rerender (HS2-SG0AZY)', async ({ page }) => {
  // The demo's startup catalog metadata response rerenders the whole demo. Hold it until the editor is
  // open and typed into, so the rerender deterministically lands mid-edit.
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route('**/__hotsheet/demo-modified', async (route) => {
    await held;
    await route.fulfill({ json: { 'ticket-attachments': '2026-10-03T00:00:00Z' } });
  });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-attachments');
  const surface = page
      .getByRole('region', { name: 'TicketAttachments demo', exact: true })
      .locator('[data-component="ticket-attachments"]'),
    first = surface.locator('[data-attachment-group-drop-target]').first();
  await first.getByRole('button', { name: /Edit batch label/ }).dblclick();
  const editor = first.getByRole('textbox', { name: /Batch label/ });
  await expect(editor).toBeFocused();
  await editor.fill('Typed before the rerender');
  const responded = page.waitForResponse('**/__hotsheet/demo-modified');
  release();
  await responded;
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() =>
          requestAnimationFrame(() => {
            resolve();
          }),
        ),
      ),
  );
  await expect(editor).toBeVisible();
  await expect(editor).toBeFocused();
  await expect(editor).toHaveValue('Typed before the rerender');
  await expect(first).toHaveAttribute('data-editing-label', 'true');
  await editor.press('Escape');
  await expect(first.getByRole('button', { name: /Brian · Round 1 · Problem evidence/ })).toBeFocused();
  await expect(first).not.toHaveAttribute('data-editing-label', /.*/);
});

test('shows append-only and unsupported attachment variants in the TicketAttachments demo (HS2-HSA64D)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-attachments');
  const appendOnly = page.getByRole('region', { name: 'Append-only TicketAttachments demo' }),
    unsupported = page.getByRole('region', { name: 'Unsupported TicketAttachments demo' });
  await expect(appendOnly.getByLabel('Browse and add attachments')).toHaveCount(1);
  await expect(appendOnly.getByLabel('Drop or browse attachments')).toHaveCount(1);
  await expect(appendOnly.getByRole('button', { name: /More actions for/ })).toHaveCount(0);
  await expect(appendOnly.locator('[draggable="true"]')).toHaveCount(0);
  // No regroup targets render where attachments cannot be edited (HS2-0RTH3J).
  await expect(appendOnly.locator('[data-attachment-group-drop-target]')).toHaveCount(0);
  await expect(appendOnly.locator('[data-attachment-new-group-drop-target]')).toHaveCount(0);
  await expect(appendOnly.getByText('New group', { exact: true })).toHaveCount(0);
  await expect(appendOnly.locator('select[name="attachment-batch-purpose"]')).toHaveJSProperty('disabled', true);
  const link = appendOnly.getByRole('link', { name: 'trace.log' });
  await expect(link).toHaveAttribute('target', '_blank');
  await expect(link).toHaveCSS('cursor', 'pointer');
  await appendOnly.locator('[data-component="ticket-attachment-item"]').first().click({ button: 'right' });
  await expect(page.getByRole('menu', { name: 'Attachment actions' })).toHaveCount(0);
  // Media stays viewable in the gallery.
  await expect(appendOnly.locator('[data-action="open-attachment-gallery"]')).toHaveCount(1);
  await expect(unsupported.getByText('This provider does not support attachment actions.')).toBeVisible();
  await expect(unsupported.getByLabel('Browse and add attachments')).toHaveCount(0);
  await appendOnly.scrollIntoViewIfNeeded();
  await appendOnly.screenshot({ path: 'target/visual-captures/hs2-hsa64d-demo-append-only-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await appendOnly.scrollIntoViewIfNeeded();
  await expect(link).toBeVisible();
  await appendOnly.screenshot({ path: 'target/visual-captures/hs2-hsa64d-demo-append-only-narrow.png' });
});

for (const theme of ['light', 'dark'] as const) {
  test(`keeps the gallery filename legible with Kerf dark tone in ${theme} (HS2-V5VS0A)`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.addInitScript((value) => {
      localStorage.setItem('hotsheet.ux-demo.theme', value);
    }, theme);
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto('/ux-demo?component=attachment-gallery&dev-review=false');
    const gallery = page.locator('[data-component="attachment-gallery"]'),
      filename = gallery.locator('.attachment-gallery__filename'),
      filenameText = filename.locator('.kui-toolbar-text__text');
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    await expect(filename).toHaveAttribute('data-tone', 'dark');
    await expect(filename).toHaveCSS('color', 'rgb(255, 255, 255)');
    await gallery.screenshot({ path: `target/visual-captures/hs2-v5vs0a-gallery-${theme}-wide.png` });
    await page.setViewportSize({ width: 390, height: 844 });
    await filenameText.evaluate((node) => {
      node.textContent = 'a-very-long-attachment-filename-that-shows-its-ending-in-the-gallery.png';
    });
    await expect(filename).toHaveCSS('color', 'rgb(255, 255, 255)');
    await expect(filenameText).toHaveCSS('text-overflow', 'ellipsis');
    await expect.poll(() => filenameText.evaluate((node) => node.scrollWidth > node.clientWidth)).toBe(true);
    await gallery.screenshot({ path: `target/visual-captures/hs2-v5vs0a-gallery-${theme}-phone.png` });
  });
}

test('navigates and zooms the standalone attachment gallery demo', async ({ page }) => {
  await page.route('**/ux-gallery-preview.svg', async (route) => {
    if (new URL(route.request().url()).search) {
      await route.fallback();
      return;
    }
    await route.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="700"><rect width="1200" height="700" fill="#ffe0a8"/><path d="M0 180V0h180M1020 0h180v180M1200 520v180h-180M180 700H0V520" fill="#c64b2c"/><text x="600" y="375" text-anchor="middle" font-family="sans-serif" font-size="58" fill="#33200f">Source corners preserved</text></svg>',
    });
  });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=attachment-gallery&dev-review=false');
  let gallery = page.locator('[data-component="attachment-gallery"]');
  await expect(gallery).toHaveAttribute('aria-label', /Image 1 of 3/);
  await expect(
    gallery
      .getByRole('button', { name: 'Annotate media, 1 annotation' })
      .locator('.attachment-gallery__annotation-count'),
  ).toHaveText('1');
  const mediaWrap = gallery.locator('.attachment-gallery__media-wrap'),
    image = gallery.locator('[data-gallery-image="true"]');
  await expect(mediaWrap).toHaveCSS('border-radius', '0px');
  await expect(image).toHaveCSS('border-radius', '0px');
  await gallery.screenshot({ path: 'target/visual-captures/hs2-2fm7ed-square-media-wide.png' });
  await expect(gallery.locator('[data-component="toolbar"]')).toBeVisible();
  const darkGroups = gallery.locator('[data-component="toolbar-control-group"]');
  await expect(darkGroups).toHaveCount(4);
  // The header groups are dark-toned; the footer's floating groups take the FloatingToolbar's forced
  // dark scheme instead (HS2-VABS08). Every group reads white on dark.
  for (const group of await darkGroups.all()) {
    const light = await group
      .locator('button:not([disabled])')
      .first()
      .evaluate((node) =>
        getComputedStyle(node)
          .color.match(/\d+/g)!
          .slice(0, 3)
          .every((channel) => Number(channel) > 150),
      );
    expect(light).toBe(true);
    await expect(group).not.toHaveCSS('border-color', 'rgb(209, 209, 214)');
  }
  await expect(gallery.locator('[data-component="toolbar-control-group"][data-tone="dark"]')).toHaveCount(3);
  await expect(gallery.locator('.attachment-gallery__filename')).toHaveCSS('color', 'rgb(255, 255, 255)');
  // At fit the gallery cannot zoom out. Kerf's ToolbarControlGroup owns that disabled button's
  // presentation with no app restyle (KF-FTADQT, HS2-0MH5V1): not-allowed, half opacity, no hover.
  const zoomOut = gallery.getByRole('button', { name: 'Zoom out' });
  await expect(zoomOut).toBeDisabled();
  await expect(zoomOut).toHaveCSS('cursor', 'not-allowed');
  await expect(zoomOut).toHaveCSS('opacity', '0.5');
  await zoomOut.hover({ force: true });
  await expect(zoomOut).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  const fitWidth = await image.evaluate((node) => (node as HTMLElement).getBoundingClientRect().width);
  await gallery.getByRole('button', { name: 'Zoom in' }).click();
  expect(await image.evaluate((node) => (node as HTMLElement).getBoundingClientRect().width)).toBeGreaterThan(fitWidth);
  await page.setViewportSize({ width: 760, height: 640 });
  await expect(mediaWrap).toHaveCSS('border-radius', '0px');
  await expect(image).toHaveCSS('border-radius', '0px');
  await gallery.screenshot({ path: 'target/visual-captures/hs2-2fm7ed-square-media-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 900 });
  await gallery.getByRole('button', { name: 'Next image' }).click();
  await expect(gallery).toHaveAttribute('aria-label', /Image 2 of 3/);
  await gallery.screenshot({ path: 'target/visual-captures/hs2-ddpkts-gallery-dark-wide.png' });
  await gallery
    .locator('[data-component="toolbar"]')
    .screenshot({ path: 'target/visual-captures/hs2-ddpkts-gallery-dark-toolbar.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await gallery.getByRole('button', { name: 'Next image' }).click();
  await expect(gallery).toHaveAttribute('aria-label', /Video 3 of 3: walkthrough.mp4/);
  const video = gallery.locator('video');
  await expect(video).not.toHaveAttribute('controls', /.+/);
  await expect(gallery.getByRole('button', { name: 'Play' })).toBeVisible();
  await expect(gallery.getByRole('button', { name: 'Volume controls' })).toBeVisible();
  await expect(video).toHaveCSS('border-radius', '0px');
  expect(await video.evaluate((node) => (node as HTMLVideoElement).paused)).toBe(true);
  await video.evaluate(async (node) => {
    if (!(node instanceof HTMLVideoElement)) throw new Error('Expected gallery video');
    const canvas = document.createElement('canvas');
    canvas.width = 640;
    canvas.height = 360;
    const context = canvas.getContext('2d')!;
    context.fillStyle = '#7857a4';
    context.fillRect(0, 0, 640, 360);
    context.fillStyle = 'white';
    context.font = '32px sans-serif';
    context.fillText('Hot Sheet video evidence', 132, 190);
    const recorder = new MediaRecorder(canvas.captureStream(5), { mimeType: 'video/webm' }),
      chunks: Blob[] = [];
    recorder.ondataavailable = (event) => chunks.push(event.data);
    recorder.start();
    await new Promise((resolve) => setTimeout(resolve, 250));
    const stopped = new Promise((resolve) => {
      recorder.onstop = resolve;
    });
    recorder.stop();
    await stopped;
    node.src = URL.createObjectURL(new Blob(chunks, { type: 'video/webm' }));
    await new Promise((resolve, reject) => {
      node.onloadedmetadata = resolve;
      node.onerror = reject;
    });
  });
  await expect(video).toHaveJSProperty('videoWidth', 640);
  const stageBox = (await gallery.locator('.attachment-gallery__stage').boundingBox())!,
    footerBox = (await gallery.locator('.attachment-gallery__footer').boundingBox())!;
  expect(stageBox.y + stageBox.height).toBeLessThanOrEqual(footerBox.y + 0.5);
  await video.hover();
  await gallery.screenshot({ path: 'target/visual-captures/hs2-hz0trg-video-gallery-narrow.png' });
  await gallery.getByRole('button', { name: 'Close video gallery' }).click();
  await expect(gallery).toHaveCount(0);
  await page.getByRole('button', { name: 'Open gallery' }).click();
  gallery = page.locator('[data-component="attachment-gallery"]');
  await expect(gallery).toBeVisible();
});

test('uses canonical spacing throughout the attachment gallery chrome (HS2-4Y6SM9)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=attachment-gallery&dev-review=false');
  const gallery = page.locator('[data-component="attachment-gallery"]');
  await expect(gallery).toBeVisible();
  const imageSpacing = await gallery.evaluate((node) => {
    const toolbar = getComputedStyle(node.querySelector<HTMLElement>('.attachment-gallery__toolbar .kui-toolbar')!),
      trailing = getComputedStyle(node.querySelector<HTMLElement>('.kui-toolbar__trailing')!),
      canvas = getComputedStyle(node.querySelector<HTMLElement>('.attachment-gallery__canvas')!),
      footer = getComputedStyle(node.querySelector<HTMLElement>('.attachment-gallery__footer')!);
    return {
      toolbarPadding: toolbar.padding,
      trailingGap: trailing.gap,
      canvasPadding: canvas.padding,
      footerPadding: footer.padding,
      footerGap: footer.gap,
    };
  });
  expect(imageSpacing).toEqual({
    toolbarPadding: '8px',
    trailingGap: '16px',
    canvasPadding: '24px',
    footerPadding: '8px 16px 16px',
    footerGap: '8px',
  });
  await gallery.getByRole('button', { name: 'Next image' }).click();
  await gallery.getByRole('button', { name: 'Next image' }).click();
  const volumeButton = gallery.getByRole('button', { name: 'Volume controls' });
  await volumeButton.click();
  await expect(volumeButton).toHaveAttribute('aria-expanded', 'true');
  const videoSpacing = await gallery.evaluate((node) => {
    const timeline = getComputedStyle(node.querySelector<HTMLElement>('.attachment-gallery__timeline')!),
      popup = getComputedStyle(node.querySelector<HTMLElement>('.attachment-gallery__volume-popup')!),
      action = getComputedStyle(node.querySelector<HTMLElement>('.attachment-gallery__volume-popup > button')!);
    return {
      timelineGap: timeline.gap,
      popupPadding: popup.padding,
      popupGap: popup.gap,
      actionPadding: action.padding,
      actionGap: action.gap,
    };
  });
  expect(videoSpacing).toEqual({
    timelineGap: '8px',
    popupPadding: '16px',
    popupGap: '8px',
    actionPadding: '8px',
    actionGap: '8px',
  });
  await gallery.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-attachment-gallery-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect
    .poll(() =>
      gallery.evaluate((node) => {
        const bounds = [
          node.getBoundingClientRect(),
          ...Array.from(node.querySelectorAll<HTMLElement>('[data-component="toolbar-control-group"]'))
            .filter((element) => getComputedStyle(element).display !== 'none')
            .map((element) => element.getBoundingClientRect()),
        ];
        return bounds.every(
          (box) => box.left >= 0 && box.top >= 0 && box.right <= innerWidth && box.bottom <= innerHeight,
        );
      }),
    )
    .toBe(true);
  await expect(gallery.locator('.attachment-gallery__footer')).toHaveCSS('padding-left', '8px');
  await gallery.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-attachment-gallery-narrow.png' });
});

test('deselects an image annotation when the media canvas is clicked away', async ({ page }) => {
  await page.goto('/ux-demo?component=attachment-gallery');
  const gallery = page.locator('[data-component="attachment-gallery"]');
  await gallery.getByRole('button', { name: 'Annotate media, 1 annotation' }).click();
  const surface = gallery.locator('[data-gallery-annotation-surface="true"]'),
    annotation = surface.getByRole('button', { name: /Review this alignment/ });
  await expect(annotation).toHaveAttribute('data-selected', 'true');
  await expect(annotation.locator('[data-annotation-handle]')).toHaveCount(8);
  const box = (await surface.boundingBox())!;
  await surface.click({ position: { x: box.width * 0.9, y: box.height * 0.9 } });
  await expect(annotation).toHaveAttribute('data-selected', 'false');
  await expect(annotation.locator('[data-annotation-handle]')).toHaveCount(0);
});

test('renders every annotation shape and intent on image and timed video at fixed screen stroke widths', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=attachment-gallery&annotation-shapes');
  const gallery = page.locator('[data-component="attachment-gallery"]'),
    shapes = gallery.locator('.attachment-gallery__annotation'),
    badges = gallery.locator('.attachment-gallery__annotation-badge'),
    media = gallery.locator('.attachment-gallery__media-wrap');
  await expect(shapes).toHaveCount(7);
  await expect(badges).toHaveCount(7);
  for (const [index, shape, color] of [
    [0, 'rect', 'blue'],
    [1, 'rect', 'red'],
    [2, 'freehand', 'orange'],
    [3, 'insertion', 'green'],
    [4, 'strike', 'purple'],
    [5, 'arrow', 'teal'],
    [6, 'rect', 'yellow'],
  ] as const) {
    await expect(shapes.nth(index)).toHaveAttribute('data-shape', shape);
    await expect(shapes.nth(index)).toHaveAttribute('data-intent-color', color);
    await expect(badges.nth(index)).toHaveText(String(index + 1));
    const mediaBox = (await media.boundingBox())!,
      badgeBox = (await badges.nth(index).boundingBox())!;
    expect(badgeBox.x).toBeGreaterThanOrEqual(mediaBox.x - 1);
    expect(badgeBox.y).toBeGreaterThanOrEqual(mediaBox.y - 1);
    expect(badgeBox.x + badgeBox.width).toBeLessThanOrEqual(mediaBox.x + mediaBox.width + 1);
    expect(badgeBox.y + badgeBox.height).toBeLessThanOrEqual(mediaBox.y + mediaBox.height + 1);
  }
  const stroke = shapes.nth(5).locator('.attachment-gallery__annotation-ink');
  await expect(stroke).toHaveCSS('vector-effect', 'non-scaling-stroke');
  const strokeWidth = await stroke.evaluate((node) => getComputedStyle(node).strokeWidth);
  await gallery.getByRole('button', { name: 'Annotate media, 7 annotations' }).click();
  await expect(gallery.getByRole('region', { name: 'Annotation 4 note' })).toContainText('Add detail');
  await gallery.screenshot({ path: 'target/visual-captures/hs2-n1eh4w-all-shapes-light.png', animations: 'disabled' });
  await page.addStyleTag({ content: '.attachment-gallery__media-wrap > img { filter: brightness(.15); }' });
  await gallery.screenshot({ path: 'target/visual-captures/hs2-n1eh4w-all-shapes-dark.png', animations: 'disabled' });
  await gallery.getByRole('button', { name: 'Zoom in' }).click();
  await expect(stroke).toHaveCSS('stroke-width', strokeWidth);
  await gallery.screenshot({ path: 'target/visual-captures/hs2-n1eh4w-shapes-zoomed.png', animations: 'disabled' });
  await gallery.getByRole('button', { name: 'Zoom out' }).click();
  await gallery.getByRole('button', { name: 'Next image' }).click();
  await gallery.getByRole('button', { name: 'Next image' }).click();
  await expect(gallery).toHaveAccessibleName(/Video 3 of 3/);
  await expect(shapes).toHaveCount(7);
  await expect(shapes.nth(5)).toHaveAttribute('data-shape', 'arrow');
  await page.addStyleTag({ content: '.attachment-gallery__video { background: #171b24; }' });
  await gallery.screenshot({ path: 'target/visual-captures/hs2-n1eh4w-all-shapes-video.png', animations: 'disabled' });
  await gallery.getByRole('slider', { name: 'Video position' }).fill('4500');
  await expect(shapes.first()).toBeHidden();
  await expect(badges.first()).toBeHidden();
  await page.setViewportSize({ width: 390, height: 844 });
  await gallery.locator('[data-action="previous-gallery-image"]').click();
  await gallery.locator('[data-action="previous-gallery-image"]').click();
  await expect(gallery).toHaveAccessibleName(/Image 1 of 3/);
  await expect(badges).toHaveCount(7);
  await gallery.screenshot({ path: 'target/visual-captures/hs2-n1eh4w-all-shapes-phone.png', animations: 'disabled' });
});

test('previews and manipulates custom video and annotation timeline controls', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=attachment-gallery&dev-review=false');
  const gallery = page.locator('[data-component="attachment-gallery"]');
  await gallery.getByRole('button', { name: 'Next image' }).click();
  await gallery.getByRole('button', { name: 'Next image' }).click();
  await expect(gallery).toHaveAccessibleName(/Video 3 of 3/);
  await expect(gallery.locator('video')).not.toHaveAttribute('controls', /.+/);
  const volumeButton = gallery.getByRole('button', { name: 'Volume controls' });
  await volumeButton.click();
  await expect(volumeButton).toHaveAttribute('aria-expanded', 'true');
  await gallery.getByRole('button', { name: 'Mute video' }).click();
  await expect(gallery.getByRole('button', { name: 'Unmute video' })).toBeVisible();
  const volume = gallery.getByRole('slider', { name: 'Video volume' });
  await volume.fill('0.4');
  await expect(volume).toHaveValue('0.4');
  await gallery.screenshot({ path: 'target/visual-captures/hs2-35n9rs-volume-popup-wide.png' });
  await gallery.locator('.attachment-gallery__filename').click();
  await expect(volumeButton).toHaveAttribute('aria-expanded', 'false');
  await volumeButton.click();
  await expect(volume).toBeVisible();
  await gallery.getByRole('button', { name: 'Annotate media' }).click();
  await expect(gallery.getByRole('slider', { name: 'Video position' })).toBeVisible();
  const start = gallery.getByRole('button', { name: /Annotation range start/ }),
    end = gallery.getByRole('button', { name: /Annotation range end/ });
  await expect(start).toBeVisible();
  await expect(end).toBeVisible();
  await start.focus();
  await page.keyboard.press('ArrowRight');
  await expect(gallery.getByRole('button', { name: 'Annotation range start at 0:01' })).toBeVisible();
  const track = (await gallery.locator('.attachment-gallery__timeline-track').boundingBox())!;
  await end.hover();
  await page.mouse.down();
  await page.mouse.move(track.x + track.width * 0.75, track.y + track.height / 2);
  await page.mouse.up();
  await expect(gallery.getByRole('button', { name: 'Annotation range end at 0:04' })).toBeVisible();
  await expect(
    gallery.getByRole('region', { name: 'Annotation 1 note' }).getByText('Transition is abrupt'),
  ).toBeVisible();
  await gallery.screenshot({ path: 'target/visual-captures/hs2-hz0trg-video-annotations-wide.png' });
  await page.setViewportSize({ width: 760, height: 640 });
  await volumeButton.click();
  await gallery.screenshot({ path: 'target/visual-captures/hs2-35n9rs-volume-popup-narrow.png' });
  await gallery.getByRole('button', { name: 'Close video gallery' }).click();
  await expect(gallery).toHaveCount(0);
  await page.getByRole('button', { name: 'Open gallery' }).click();
  await expect(page.locator('[data-component="attachment-gallery"]')).toBeVisible();
});

test('represents animated SVG evidence with the shared timed annotation controls', async ({ page }) => {
  await page.route('**/ux-gallery-preview.svg?variant=narrow', (route) =>
    route.fulfill({
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"><rect width="640" height="360"><animate attributeName="fill" values="#333;#777;#333" dur="4s" repeatCount="indefinite"/></rect></svg>',
    }),
  );
  await page.goto('/ux-demo?component=attachment-gallery');
  const gallery = page.locator('[data-component="attachment-gallery"]');
  await gallery.getByRole('button', { name: 'Next image' }).click();
  await expect(gallery).toHaveAccessibleName(/Image 2 of 3/);
  await expect(gallery.getByRole('slider', { name: 'Video position' })).toBeVisible();
  await gallery.getByRole('button', { name: 'Annotate media' }).click();
  await expect(gallery.getByRole('button', { name: /Annotation range start/ })).toBeVisible();
  await gallery.screenshot({ path: 'target/visual-captures/hs2-jw17a4-animated-svg-annotations.png' });
});

test('opens the shared TicketReader intent when a composed row is double-clicked', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-list');
  await page.locator('[data-component="ticket-list-row"]').first().dblclick();
  await expect(page.getByRole('heading', { name: 'TicketReader', exact: true })).toBeVisible();
});

test('adjusts and removes TagChip through its settings inspector', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto('/ux-demo?component=tag-chip');
  const chip = page.locator('[data-component="tag-chip"]'),
    kerfChip = chip.locator('[data-component="chip"]');
  await expect(chip).toContainText('needs-design');
  await expect(kerfChip).toHaveAttribute('data-size', 'compact');
  await expect(kerfChip).toHaveAttribute('data-appearance', 'quiet');
  await expect(kerfChip).toHaveAttribute('data-shape', 'rounded');
  await expect(kerfChip).toHaveAttribute('data-item-id', 'demo-tag');
  const chipPadding = await chip.evaluate((node) => getComputedStyle(node).paddingLeft);
  expect(chipPadding).toBe('0px');
  const toggle = page.locator('[data-action="toggle-settings"]');
  await expect(toggle).toHaveCount(1);
  await expect(toggle).toContainText('Settings');
  await expect(toggle).toBeVisible();
  await toggle.click();
  await expect(toggle).toHaveCount(1);
  await expect(toggle).toContainText('Close settings');
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(toggle).toBeVisible();
  const inspector = page.getByRole('complementary', { name: 'TagChip settings' });
  await expect(inspector).toBeVisible();
  const [detailBox, inspectorBox] = await Promise.all([
    page.locator('.kui-workbench__main').boundingBox(),
    inspector.boundingBox(),
  ]);
  expect(detailBox).not.toBeNull();
  expect(inspectorBox).not.toBeNull();
  expect(detailBox!.x + detailBox!.width).toBeLessThanOrEqual(inspectorBox!.x + 1);
  const label = page.getByRole('textbox', { name: 'Label' });
  await label.fill('server');
  await expect(chip).toContainText('server');
  await expect(inspector).toBeVisible();
  await page.locator('wa-select[name="variant"]').evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'success';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(kerfChip).toHaveAttribute('data-tone', 'success');
  await expect(inspector).toBeVisible();
  await inspector.locator('wa-checkbox[name="pill"]').click();
  await expect(kerfChip).toHaveAttribute('data-shape', 'pill');
  await expect(inspector).toBeVisible();
  await chip.getByRole('button', { name: 'Remove server' }).click();
  await expect(page.getByText('Remove requested for demo-tag')).toBeVisible();
  await expect(inspector).toBeVisible();
  await inspector.getByRole('button', { name: 'Reset' }).click();
  await expect(label).toHaveJSProperty('value', 'needs-design');
  await expect(page.locator('wa-select[name="variant"]')).toHaveJSProperty('value', 'neutral');
  await expect(page.locator('wa-select[name="appearance"]')).toHaveJSProperty('value', 'filled');
  await expect(page.locator('wa-select[name="size"]')).toHaveJSProperty('value', 'small');
  await expect(inspector.locator('wa-checkbox[name="removable"]')).toHaveJSProperty('checked', true);
  await expect(inspector.locator('wa-checkbox[name="pill"]')).toHaveJSProperty('checked', false);
  await expect(inspector.locator('wa-checkbox[name="disabled"]')).toHaveJSProperty('checked', false);
  await expect(chip).toContainText('needs-design');
  await expect(kerfChip).toHaveAttribute('data-tone', 'neutral');
  await expect(kerfChip).toHaveAttribute('data-shape', 'rounded');
  await expect(page.getByText('No actions yet')).toBeVisible();
  await expect(inspector).toBeVisible();
  await toggle.click();
  await expect(inspector).toBeHidden();
  await expect(toggle).toContainText('Settings');
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
});

test('maps the brand tag variant to Kerf info tone at wide and phone widths', async ({ page }, testInfo) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await page.goto('/ux-demo?component=tag-chip');
    await page.locator('[data-action="toggle-settings"]').click();
    await page.locator('wa-select[name="variant"]').evaluate((node: HTMLElement & { value: string }) => {
      node.value = 'brand';
      node.dispatchEvent(new Event('change', { bubbles: true }));
    });
    const chip = page.locator('[data-component="tag-chip"]');
    await expect(chip.locator('[data-component="chip"]')).toHaveAttribute('data-tone', 'info');
    await page.locator('[data-action="toggle-settings"]').click();
    await expect(chip).toBeVisible();
    await chip.screenshot({ path: testInfo.outputPath(`tag-chip-brand-${width}.png`) });
  }
});

test('uses semantic cursors across native and Web Awesome interactions', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-row');
  await expect(page.locator('[data-component="ticket-list-row"]')).toHaveCSS('cursor', 'pointer');
  await expect(page.locator('[data-action="toggle-row-up-next"]')).toHaveCSS('cursor', 'pointer');
  await page.locator('[data-action="toggle-settings"]').click();
  const inspector = page.getByRole('complementary', { name: 'TicketRow settings' });
  await expect(inspector.getByRole('textbox', { name: 'Title' })).toHaveCSS('cursor', 'text');
  const selectCursor = await inspector
    .locator('wa-select[name="status"]')
    .evaluate((node) => getComputedStyle(node.shadowRoot!.querySelector('[part~="combobox"]')!).cursor);
  expect(selectCursor).toBe('pointer');
  const checkboxCursor = await inspector
    .locator('wa-checkbox[name="up-next"]')
    .evaluate((node) => getComputedStyle(node.shadowRoot!.querySelector('[part~="checkbox"]')!).cursor);
  expect(checkboxCursor).toBe('pointer');
  const buttonCursor = await inspector
    .locator('wa-button[data-action="reset-settings"]')
    .evaluate((node) => getComputedStyle(node.shadowRoot!.querySelector('[part~="base"]')!).cursor);
  expect(buttonCursor).toBe('pointer');
  await page.locator('[data-action="toggle-settings"]').click();
  await page.locator('[data-component="ticket-list-row"]').click({ button: 'right' });
  await expect(page.getByRole('menu', { name: 'Ticket actions' }).locator('wa-dropdown-item').first()).toHaveCSS(
    'cursor',
    'pointer',
  );
  await page.goto('/ux-demo?component=quick-ticket-composer');
  await page.getByRole('button', { name: /New ticket/ }).click();
  const create = page.locator('wa-button[type="submit"]');
  await create.evaluate((node) => {
    node.setAttribute('disabled', '');
  });
  await expect
    .poll(() => create.evaluate((node) => getComputedStyle(node.shadowRoot!.querySelector('[part~="base"]')!).cursor))
    .toBe('not-allowed');
});

test('exercises the five ProjectSidebar component demos and their controlled transitions', async ({ page }) => {
  await page.goto('/ux-demo?component=project-summary');
  const summaries = page.locator('[data-component="project-summary"]');
  await expect(summaries).toHaveCount(3);
  // The compact size is the terminal operations sidebar's per-project summary (HS2-4APEJP).
  const compactSummary = page.locator('[data-component="project-summary"][data-size="compact"]');
  await expect(compactSummary).toHaveCSS('padding', '8px');
  await expect(compactSummary).toHaveCSS('min-height', '68px');
  await expect(compactSummary.locator('.project-summary__chart')).toHaveCSS('height', '44px');
  await expect(compactSummary.locator('[data-background-bar]')).toHaveCount(7);
  const summary = page.locator('[data-component="project-summary"][data-chart-tone="brand"][data-size="default"]');
  const aggregateSummary = page.locator('[data-component="project-summary"][data-chart-tone="success"]');
  await expect(aggregateSummary.locator('[data-bar="0"]')).toHaveCSS('background-color', 'rgb(52, 199, 89)');
  await expect(summary).toContainText('6 completed today');
  await expect(summary).toContainText('3 in progress');
  await expect(summary).not.toContainText('%');
  await expect(summary.locator('[data-bar]')).toHaveCount(7);
  await expect(summary.locator('[data-zero="true"]')).toHaveCount(1);
  await expect(summary.locator('[data-zero="true"]')).toHaveCSS('height', '1px');
  await expect(summary.locator('[data-zero="true"]')).toHaveCSS('background-color', 'rgba(0, 0, 13, 0.318)');
  await expect(summary).toHaveCSS('cursor', 'pointer');
  const summaryGeometry = await summary.evaluate((node) => {
    const button = node.getBoundingClientRect(),
      chart = node.querySelector('.project-summary__chart')!.getBoundingClientRect(),
      counts = node.querySelector('.project-summary__counts')!.getBoundingClientRect();
    return {
      button: { top: button.top, bottom: button.bottom },
      contentTop: Math.min(chart.top, counts.top),
      contentBottom: Math.max(chart.bottom, counts.bottom),
    };
  });
  expect(summaryGeometry.button.top).toBeLessThanOrEqual(summaryGeometry.contentTop);
  expect(summaryGeometry.button.bottom).toBeGreaterThanOrEqual(summaryGeometry.contentBottom);
  await summary.hover();
  await page.screenshot({ path: 'target/visual-captures/hs2-j5c5xg-project-summary-hit-area.png', fullPage: true });
  await summary.click();
  await expect(page.getByText('Hot Sheet 2 project statistics requested.')).toBeVisible();

  await page.goto('/ux-demo?component=repository-summary');
  const repository = page.getByRole('button', { name: 'Repository status for feature/client-sidebar' });
  await expect(repository.locator('[data-lucide="git-branch"]')).toHaveCount(1);
  await expect(repository.locator('.repository-summary__branch-name')).toHaveCSS('direction', 'rtl');
  await expect(repository.locator('.repository-summary__branch-name')).toHaveCSS('text-overflow', 'ellipsis');
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await repository.screenshot({ path: `target/visual-captures/hs2-8k7qk7-summary-${width}.png` });
  }
  await page.setViewportSize({ width: 1280, height: 844 });
  await repository.click();
  await expect(page.getByText('Repository status requested.')).toBeVisible();

  await page.goto('/ux-demo?component=view-navigation');
  const views = page.locator('[data-component="view-navigation"]');
  await expect(views.locator(':scope > .kui-list')).toHaveCSS('gap', '4px');
  await expect(views.locator('.view-navigation__list')).toHaveCSS('gap', '0px');
  const navigationGeometry = await views.evaluate((node) => {
    const header = node.querySelector('header')!.getBoundingClientRect();
    const button = node.querySelector('.view-navigation__list button')!.getBoundingClientRect();
    const icon = node.querySelector('.view-navigation__list button svg')!.getBoundingClientRect();
    return {
      headerLeft: header.left,
      buttonLeft: button.left,
      iconLeft: icon.left,
      headerRight: header.right,
      buttonRight: button.right,
    };
  });
  expect(navigationGeometry.headerLeft).toBeCloseTo(navigationGeometry.buttonLeft, 0);
  expect(navigationGeometry.iconLeft - navigationGeometry.buttonLeft).toBeGreaterThan(8);
  expect(navigationGeometry.iconLeft - navigationGeometry.buttonLeft).toBeLessThan(11);
  await expect(views.getByRole('button', { name: /Queue/ })).toHaveAttribute('aria-current', 'page');
  await expect(views.locator('.view-navigation__count').first().locator('[data-component="badge"]')).toHaveAttribute(
    'data-tone',
    'danger',
  );
  await views.getByRole('button', { name: /Needs Review/ }).click();
  await expect(views.getByRole('button', { name: /Needs Review/ })).toHaveAttribute('aria-current', 'page');
  await expect(views.getByRole('button', { name: /Queue/ })).not.toHaveAttribute('aria-current', 'page');
  const addView = views.getByRole('button', { name: 'Add view' });
  await expect(addView).toBeEnabled();
  await expect(addView).toHaveAttribute('title', 'Add view');
  await page.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-view-navigation.png', fullPage: true });
  await addView.click();
  await expect(page.locator('.component-stage__event')).toContainText('New view editor requested.');

  await page.goto('/ux-demo?component=command-navigation');
  const commands = page.locator('[data-component="command-navigation"]');
  const heading = commands.getByRole('button', { name: /Project commands/ });
  await expect(heading).toHaveAttribute('aria-expanded', 'true');
  const qualityGroup = commands.getByRole('button', { name: 'Quality' }),
    releaseGroup = commands.getByRole('button', { name: 'Release' });
  await expect(qualityGroup).toHaveAttribute('aria-expanded', 'true');
  await expect(releaseGroup).toHaveAttribute('aria-expanded', 'true');
  // The Kerf row paints its own fill through the public ListItem tone tokens; the app wrapper is
  // transparent (HS2-Z5YQWT).
  await expect(commands.getByRole('button', { name: 'Verify project' })).toHaveCSS(
    'background-color',
    'rgb(20, 184, 166)',
  );
  await expect(commands.locator('.command-navigation__command').filter({ hasText: 'Verify project' })).toHaveCSS(
    'background-color',
    'rgba(0, 0, 0, 0)',
  );
  await expect(commands.getByRole('button', { name: 'Verify project' })).toHaveCSS('border-color', 'rgba(0, 0, 0, 0)');
  await commands.getByRole('button', { name: 'Verify project' }).hover();
  await expect(commands.getByRole('button', { name: 'Verify project' })).toHaveCSS('border-color', 'rgba(0, 0, 0, 0)');
  await expect(commands.getByRole('button', { name: 'Build clients' })).toHaveCSS(
    'background-color',
    'rgb(249, 115, 22)',
  );
  // The transparent palette slot has no fill but spans the same extent as the filled rows (HS2-F9JKMJ).
  const transparentCommand = commands.getByRole('button', { name: 'Run everything' });
  await expect(transparentCommand).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  const [filledCommandBox, transparentCommandBox] = await Promise.all([
    commands.getByRole('button', { name: 'Build clients' }).boundingBox(),
    transparentCommand.boundingBox(),
  ]);
  expect(Math.round(transparentCommandBox!.x)).toBe(Math.round(filledCommandBox!.x));
  expect(Math.round(transparentCommandBox!.width)).toBe(Math.round(filledCommandBox!.width));
  await expect(commands.getByRole('button', { name: 'Publish preview' })).toHaveCSS(
    'background-color',
    'rgb(139, 92, 246)',
  );
  await expect(commands.getByRole('button', { name: 'Build clients' }).getByLabel('Shell command')).toHaveCSS(
    'opacity',
    '0.5',
  );
  await expect(
    commands.getByRole('button', { name: 'Build clients' }).locator('[data-lucide="square-terminal"]'),
  ).toBeVisible();
  await expect(commands.getByRole('button', { name: 'Publish preview' }).getByLabel('AI command')).toHaveCSS(
    'opacity',
    '0.5',
  );
  await expect(commands.getByRole('button', { name: 'Publish preview' }).locator('[data-lucide="bot"]')).toBeVisible();
  await commands.screenshot({ path: 'target/visual-captures/hs2-a66p03-command-type-icons.png' });
  await commands.getByRole('button', { name: 'Verify project' }).click();
  await expect(commands.getByRole('button', { name: /Running Verify project/ })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(commands.getByRole('button', { name: /Running Verify project/ })).toHaveAttribute('aria-busy', 'true');
  await expect(
    commands.getByRole('button', { name: /Running Verify project/ }).locator('[data-component="loading-spinner"]'),
  ).toBeVisible();
  await commands.getByRole('button', { name: /Running Verify project/ }).click();
  await expect(commands.getByRole('button', { name: 'Verify project' })).toHaveAttribute('aria-pressed', 'false');
  await qualityGroup.click();
  await expect(qualityGroup).toHaveAttribute('aria-expanded', 'false');
  await expect(commands.getByRole('button', { name: 'Verify project' })).toHaveCount(0);
  await expect(commands.getByRole('button', { name: 'Publish preview' })).toBeVisible();
  await qualityGroup.click();
  await expect(commands.getByRole('button', { name: 'Verify project' })).toBeVisible();
  await heading.click();
  await expect(commands.getByRole('button', { name: 'Verify project' })).toHaveCount(0);
  await expect(heading).toHaveAttribute('aria-expanded', 'false');

  await page.goto('/ux-demo?component=drive-control');
  const drive = page.locator('[data-component="drive-control"]'),
    driveAction = drive.locator('[data-action="toggle-drive"]');
  await expect(driveAction).toHaveAccessibleName('Drive with Codex');
  await drive.click();
  await expect(driveAction).toHaveAccessibleName('Codex workflow is running');
  await drive.click();
  await expect(driveAction).toHaveAccessibleName('Drive with Codex');
});

test('holds the AppShell at its 1024 by 600 supported floor', async ({ page }) => {
  await page.setViewportSize({ width: 800, height: 500 });
  await page.goto('/ux-demo?component=app-shell');
  const shell = page.locator('[data-component="app-shell"]');
  const bounds = await shell.boundingBox();
  expect(bounds?.width).toBeGreaterThanOrEqual(1024);
  expect(bounds?.height).toBeGreaterThanOrEqual(600);
  await expect(shell.locator('#app-left-rail')).toBeVisible();
  await expect(shell.locator('#app-right-rail')).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-501eph-shell-floor.png', fullPage: true });
});

test('keeps the AppShell terminal restore action inside the main column (HS2-3ZGWMN)', async ({ page }) => {
  await page.setViewportSize({ width: 1728, height: 971 });
  await page.goto('/ux-demo?component=app-shell');
  const shell = page.locator('[data-component="app-shell"]'),
    main = shell.locator('.app-shell__main'),
    restore = shell.getByRole('toolbar', { name: 'Terminal drawer controls' });
  await expect(restore).toBeVisible();
  await expect
    .poll(async () => {
      const [mainBox, restoreBox] = await Promise.all([main.boundingBox(), restore.boundingBox()]);
      if (!mainBox || !restoreBox) return undefined;
      return {
        rightInset: Math.round(mainBox.x + mainBox.width - restoreBox.x - restoreBox.width),
        bottomInset: Math.round(mainBox.y + mainBox.height - restoreBox.y - restoreBox.height),
      };
    })
    .toEqual({ rightInset: 16, bottomInset: 16 });
  await restore.getByRole('button', { name: 'Show terminal drawer' }).click();
  await expect(shell.getByRole('region', { name: 'Example terminal drawer' })).toBeVisible();
  await shell.getByRole('button', { name: 'Hide terminal drawer' }).click();
  await expect(restore).toBeVisible();
});

test('keeps workspace spacing and the new-ticket action in the project tab bar', async ({ page }) => {
  await page.setViewportSize({ width: 1728, height: 971 });
  await page.goto('/ux-demo?component=app-shell');
  // HS2-9R1F91: desktop project views omit the separate page-header row; the view title lives in
  // the compact main toolbar and the view action sits at the trailing edge of ProjectTabBar.
  const shell = page.locator('[data-component="app-shell"]'),
    workArea = shell.locator('.app-shell__work-area'),
    workspace = shell.locator('.app-shell__workspace'),
    header = shell.locator('.project-tab-bar');
  await expect(workArea).toHaveAttribute('data-has-composer', 'false');
  await expect(shell.locator('[data-component="heading"]')).toHaveCount(0);
  await expect(shell.locator('#app-shell-demo-page-title')).toHaveText('Queue');
  expect(
    await shell.evaluate((node) => {
      const workspace = node.querySelector<HTMLElement>('.app-shell__workspace')!,
        tabBar = node.querySelector<HTMLElement>('.project-tab-bar')!,
        launcher = tabBar.querySelector<HTMLElement>('[data-component="quick-ticket-composer-launcher"]')!,
        tabs = node.querySelector<HTMLElement>('.ticket-inspector__tabs-frame')!,
        content = node.querySelector<HTMLElement>('.ticket-inspector-panel')!,
        bar = tabBar.getBoundingClientRect(),
        action = launcher.getBoundingClientRect();
      return {
        workspacePaddingTop: getComputedStyle(workspace).paddingTop,
        launcherInsideTabBar: action.top >= bar.top - 1 && action.bottom <= bar.bottom + 1,
        launcherCenterOffset: Math.abs(action.top + action.height / 2 - (bar.top + bar.height / 2)),
        launcherTrailing: bar.right - action.right < 64,
        tabsMarginBottom: getComputedStyle(tabs).paddingBottom,
        contentPaddingTop: getComputedStyle(content).paddingTop,
        tabsToContent:
          content.getBoundingClientRect().top -
          node.querySelector<HTMLElement>('.ticket-inspector__tabs')!.getBoundingClientRect().bottom,
      };
    }),
  ).toEqual({
    workspacePaddingTop: '16px',
    launcherInsideTabBar: true,
    launcherCenterOffset: expect.any(Number),
    launcherTrailing: true,
    tabsMarginBottom: '8px',
    contentPaddingTop: '0px',
    tabsToContent: 8,
  });
  const centerOffset = await header.evaluate((node) => {
    const bar = node.getBoundingClientRect(),
      action = node.querySelector('[data-component="quick-ticket-composer-launcher"]')!.getBoundingClientRect();
    return Math.abs(action.top + action.height / 2 - (bar.top + bar.height / 2));
  });
  expect(centerOffset).toBeLessThan(1);
  await shell.getByRole('button', { name: 'Columns view' }).click();
  await expect(workspace).toHaveAttribute('data-presentation', 'edge-to-edge');
  expect(
    await workspace.evaluate((node) => ({
      paddingTop: getComputedStyle(node).paddingTop,
      boardTop: node.querySelector('.ticket-board')!.getBoundingClientRect().top - node.getBoundingClientRect().top,
    })),
  ).toEqual({ paddingTop: '16px', boardTop: 16 });
  await page.screenshot({ path: test.info().outputPath('hs2-f943hj-owned-spacing-wide.png'), fullPage: true });
  await shell.getByRole('button', { name: 'Settings view' }).click();
  await expect(header.getByRole('button', { name: /New ticket/ })).toHaveCount(0);
  await expect(workspace).toHaveCSS('padding-top', '16px');
  await shell.getByRole('button', { name: 'List view' }).click();
  await expect(header.getByRole('button', { name: /New ticket/ })).toHaveCount(1);
  await page.setViewportSize({ width: 1024, height: 600 });
  await page
    .locator(
      '.kui-workbench__rail--left,.kui-workbench__main > .kui-pane > .kui-pane__header,.kui-workbench__main > .kui-pane > .kui-pane__footer',
    )
    .evaluateAll((nodes) => {
      nodes.forEach((node) => {
        (node as HTMLElement).style.display = 'none';
      });
    });
  await page.locator('.demo-shell').evaluate((node) => {
    (node as HTMLElement).style.gridTemplateColumns = '1fr';
  });
  await page.locator('.kui-workbench__main,.component-stage').evaluateAll((nodes) => {
    nodes.forEach((node) => {
      (node as HTMLElement).style.padding = '0';
      (node as HTMLElement).style.border = '0';
    });
  });
  await expect(workspace).toHaveCSS('padding-top', '16px');
  await page.screenshot({ path: test.info().outputPath('hs2-f943hj-owned-spacing-narrow.png'), fullPage: true });
});

test('composes and operates the complete ProjectSidebar demo', async ({ page }) => {
  await page.goto('/ux-demo?component=project-sidebar');
  const sidebar = page.locator('.project-sidebar');
  await expect(sidebar).toBeVisible();
  await expect(sidebar.locator('[data-component="project-work-summary"]')).toHaveText('17 open, 4 up next, 2 active');
  for (const component of [
    'project-summary',
    'repository-summary',
    'view-navigation',
    'command-navigation',
    'drive-control',
  ])
    await expect(sidebar.locator(`[data-component="${component}"]`)).toHaveCount(1);
  const menuHeaderLefts = await sidebar
    .locator('[data-component="list-header"]')
    .evaluateAll((headers) => headers.map((header) => header.getBoundingClientRect().left));
  expect(menuHeaderLefts).toHaveLength(4);
  for (const left of menuHeaderLefts.slice(1)) expect(Math.abs(left - menuHeaderLefts[0])).toBeLessThan(1);
  // Kerf 5.0.0-beta.51 centers a ListHeader action on the shared trailing toolbar-action axis
  // (KF-NRB76K): the Add view action sits under the sidebar toolbar control, not at the row edge.
  const viewActionAlignment = await sidebar.evaluate((node) => {
    const action = node
      .querySelector<HTMLElement>('.view-navigation [data-component="list-header"] button')!
      .getBoundingClientRect();
    // The trailing toolbar-action axis: a 44px toolbar control inset by the shared 8px rail.
    return { actionCenter: action.left + action.width / 2, axis: node.getBoundingClientRect().right - 8 - 22 };
  });
  // The demo pane's 1px edge separator shifts the axis by at most one pixel.
  expect(Math.abs(viewActionAlignment.actionCenter - viewActionAlignment.axis)).toBeLessThanOrEqual(1.5);
  const alignedRows = await sidebar.evaluate((node) =>
    ['.repository-summary .kui-list-item', '.view-navigation .kui-list-item', '.command-navigation .kui-list-item']
      .map((selector) => node.querySelector(selector)!)
      .map((item) => {
        const bounds = item.getBoundingClientRect();
        const icon = item.querySelector('.kui-list-item__icon')!.getBoundingClientRect();
        const label = item.querySelector('.kui-list-item__label')!.getBoundingClientRect();
        return { left: bounds.left, right: bounds.right, icon: icon.left, label: label.left };
      }),
  );
  expect(alignedRows).toHaveLength(3);
  for (const row of alignedRows.slice(1)) {
    expect(row.left).toBeCloseTo(alignedRows[0].left, 0);
    expect(row.icon).toBeCloseTo(alignedRows[0].icon, 0);
    expect(row.label).toBeCloseTo(alignedRows[0].label, 0);
  }
  // A colored command keeps its own fill and text color on hover: Kerf beta.64 ListItems inherit
  // the tone tokens the app-owned wrapper declares (KF-XD6YH1, HS2-C3SPM6).
  const command = sidebar.getByRole('button', { name: 'Verify project' });
  const commandColors = await command.evaluate((node) => {
    const probe = document.createElement('span');
    probe.style.color = 'var(--command-color)';
    node.closest('.command-navigation__command')!.append(probe);
    const fill = getComputedStyle(probe).color;
    probe.remove();
    return { color: getComputedStyle(node).color, background: getComputedStyle(node).backgroundColor, fill };
  });
  expect(commandColors.background).toBe(commandColors.fill);
  await command.hover();
  await expect(command).toHaveCSS('color', commandColors.color);
  await expect(command).toHaveCSS('background-color', commandColors.fill);
  await page.screenshot({ path: test.info().outputPath('project-sidebar-command-hover.png') });
  await sidebar.getByRole('button', { name: /Backlog/ }).click();
  await expect(sidebar.getByRole('button', { name: /Backlog/ })).toHaveAttribute('aria-current', 'page');
  await command.click();
  await expect(sidebar.getByRole('button', { name: /Running Verify project/ })).toHaveAttribute('aria-pressed', 'true');
  await sidebar.getByRole('button', { name: 'Drive with Codex' }).click();
  await expect(sidebar.getByRole('button', { name: 'Codex workflow is running' })).toBeVisible();
  const handle = page.getByRole('separator', { name: 'Resize project sidebar' });
  await expect(handle).toHaveAttribute('aria-valuenow', '640');
  const assertDrivePinned = async () => {
    const geometry = await sidebar.evaluate((node) => ({
      sidebarBottom: node.getBoundingClientRect().bottom,
      driveBottom: node.querySelector('.drive-control')!.getBoundingClientRect().bottom,
    }));
    expect(geometry.sidebarBottom - geometry.driveBottom).toBeLessThan(16);
  };
  await assertDrivePinned();
  const handleBox = await handle.boundingBox();
  expect(handleBox).not.toBeNull();
  await page.mouse.move(handleBox!.x + handleBox!.width / 2, handleBox!.y + handleBox!.height / 2);
  await page.mouse.down();
  await page.mouse.move(handleBox!.x + handleBox!.width / 2, handleBox!.y - 230, { steps: 5 });
  await page.mouse.up();
  await expect(handle).toHaveAttribute('aria-valuenow', '400');
  const scrollState = await sidebar.evaluate((node) => {
    const content = node.querySelector('.kui-pane__content')!;
    const drive = node.querySelector('.drive-control')!;
    return {
      contentClientHeight: content.clientHeight,
      contentScrollHeight: content.scrollHeight,
      sidebarBottom: node.getBoundingClientRect().bottom,
      driveBottom: drive.getBoundingClientRect().bottom,
    };
  });
  expect(scrollState.contentScrollHeight).toBeGreaterThan(scrollState.contentClientHeight);
  expect(scrollState.sidebarBottom - scrollState.driveBottom).toBeLessThan(16);
  await handle.focus();
  await page.keyboard.press('ArrowDown');
  await expect(handle).toHaveAttribute('aria-valuenow', '424');
  await assertDrivePinned();
  for (let index = 0; index < 20; index += 1) await page.keyboard.press('ArrowDown');
  await expect(handle).toHaveAttribute('aria-valuenow', '768');
  await assertDrivePinned();
});

test('catalogs project-tab states, progress, activity, and responsive geometry', async ({ page }) => {
  test.setTimeout(45_000);
  await page.goto('/ux-demo?component=project-tab');
  const tabStates = page.locator('[data-tab-kind="project"]');
  await expect(tabStates).toHaveCount(16);
  // A still-opening remembered project is a dormant placeholder with a named opening spinner (HS2-2BEJXD).
  const pendingTab = page.locator('[data-tab-kind="project"][data-project-pending="true"]');
  await expect(pendingTab).toHaveCount(1);
  await expect(pendingTab).toHaveAttribute('aria-busy', 'true');
  await expect(pendingTab.getByRole('tab')).toBeDisabled();
  await expect(pendingTab.getByRole('img', { name: 'Opening Still opening' })).toBeVisible();
  const measured = tabStates.filter({ hasText: 'Copying database' });
  await expect(measured.locator('.project-tab__operation')).toHaveAttribute(
    'aria-label',
    'Copying database, 50 percent',
  );
  await expect(measured.locator('.project-tab__operation')).toContainText('50%');
  await expect(measured.locator('.project-tab__work-count')).toHaveText('3');
  await expect(tabStates.filter({ hasText: 'Opening database' }).locator('.project-tab__operation')).not.toContainText(
    '%',
  );
  await expect(tabStates.filter({ hasText: 'Import failed' }).locator('.project-tab__operation')).toHaveAttribute(
    'data-state',
    'failed',
  );
  await expect(tabStates.filter({ hasText: 'Imported' }).locator('.project-tab__operation')).toHaveAttribute(
    'data-state',
    'succeeded',
  );
  const selectedLocal = tabStates.filter({ hasText: 'Selected local' });
  await expect(selectedLocal).toHaveAttribute('data-selected', 'true');
  const standaloneLabelCenterOffset = async () =>
    selectedLocal.evaluate((node) => {
      const tab = node.getBoundingClientRect(),
        label = node.querySelector('.kui-app-tab__name')!.getBoundingClientRect();
      return Math.abs(tab.x + tab.width / 2 - label.x - label.width / 2);
    });
  await expect.poll(standaloneLabelCenterOffset).toBeLessThanOrEqual(4);
  await expect(selectedLocal.locator('[data-lucide="folder-git-2"]')).toHaveCount(0);
  await expect(tabStates.filter({ hasText: 'Remote project' }).locator('[data-lucide="cloud"]')).toHaveCount(1);
  await expect(
    tabStates.filter({ hasText: 'Busy project' }).locator('.project-tab__busy .kui-loading-spinner'),
  ).toHaveCount(1);
  // Kerf LoadingSpinner `size` gives the busy spinner its 12.8px box, centered in the 16px slot (HS2-JVPPVV).
  const busySpinner = await tabStates
    .filter({ hasText: 'Busy project' })
    .locator('.project-tab__busy .kui-loading-spinner')
    .evaluate((node) => {
      const box = node.getBoundingClientRect(),
        slot = node.parentElement!.getBoundingClientRect();
      return [box.width, box.height, box.left - slot.left, box.top - slot.top].map((n) => Math.round(n * 10) / 10);
    });
  expect(busySpinner).toEqual([12.8, 12.8, 1.6, 1.6]);
  const activeQueue = tabStates.filter({ hasText: 'Active queue' });
  await expect(activeQueue.locator('.project-tab__work')).toHaveAttribute(
    'aria-label',
    '3 Up Next tickets, 2 active tickets',
  );
  await expect(activeQueue.locator('.project-tab__activity-ring')).toBeVisible();
  await expect(activeQueue.locator('.project-tab__work-count')).toHaveText('3');
  const activeQueueCenters = await activeQueue.locator('.project-tab__work').evaluate((node) => {
    const outer = node.getBoundingClientRect(),
      count = node.querySelector('.project-tab__work-count')!.getBoundingClientRect(),
      ring = node.querySelector('svg')!.getBoundingClientRect();
    return {
      countX: Math.abs(outer.x + outer.width / 2 - count.x - count.width / 2),
      countY: Math.abs(outer.y + outer.height / 2 - count.y - count.height / 2),
      ringX: Math.abs(outer.x + outer.width / 2 - ring.x - ring.width / 2),
      ringY: Math.abs(outer.y + outer.height / 2 - ring.y - ring.height / 2),
    };
  });
  expect(Math.max(...Object.values(activeQueueCenters))).toBeLessThan(1);
  const activeWork = tabStates.filter({ hasText: 'Active work' });
  await expect(activeWork.locator('.project-tab__work')).toHaveAttribute('aria-label', '1 active ticket');
  await expect(activeWork.locator('.project-tab__work-count')).toHaveText('0');
  for (const [label, segments, dash] of [
    ['Active work', '1', '42.4115 14.1372'],
    ['Active queue', '2', '21.2058 7.0686'],
    ['Three active', '3', '14.1372 4.7124'],
    ['Four active', '4', '10.6029 3.5343'],
    ['Capped active', '8', '5.3014 1.7671'],
  ] as const) {
    const ring = tabStates.filter({ hasText: label }).locator('.project-tab__activity-ring');
    await expect(ring).toHaveAttribute('data-segments', segments);
    await expect(ring.locator('.project-tab__activity-segments')).toHaveAttribute('stroke-dasharray', dash);
  }
  const cappedActive = tabStates.filter({ hasText: 'Capped active' });
  await expect(cappedActive.locator('.project-tab__work')).toHaveAttribute(
    'aria-label',
    '7 Up Next tickets, 12 active tickets',
  );
  await expect(cappedActive.locator('.project-tab__work-count')).toHaveText('7');
  await expect(activeWork.locator('.project-tab__activity-segments')).toHaveCSS(
    'animation-name',
    'project-tab-activity-rotate',
  );
  await expect(activeWork.locator('.project-tab__activity-segments')).toHaveCSS('animation-duration', '1.7s');
  await expect(tabStates.filter({ hasText: 'Needs attention' }).locator('[data-lucide="circle-alert"]')).toHaveCount(1);
  await expect(tabStates.filter({ hasText: 'Disconnected' }).locator('[data-lucide="wifi-off"]')).toHaveCount(1);
  await expect(tabStates.filter({ hasText: 'Not closable' }).getByRole('button', { name: /Close/ })).toHaveCount(0);
  await expect(tabStates.filter({ hasText: 'Not closable' }).locator('.project-tab__work-count')).toHaveText('99+');
  for (const [label, selector] of [
    ['Busy project', '.project-tab__busy'],
    ['Needs attention', '.project-tab__state--attention'],
    ['Disconnected', '.project-tab__state'],
  ] as const) {
    const tab = tabStates.filter({ hasText: label });
    const geometry = await tab.evaluate((node, stateSelector) => {
      const tab = node.getBoundingClientRect();
      const name = node.querySelector<HTMLElement>('.kui-app-tab__name')!.getBoundingClientRect();
      const state = node.querySelector<HTMLElement>(stateSelector)!.getBoundingClientRect();
      return { rightInset: tab.right - state.right, labelGap: state.left - name.right };
    }, selector);
    expect(geometry.rightInset).toBeGreaterThan(7);
    expect(geometry.labelGap).toBeGreaterThan(0);
  }
  await page.setViewportSize({ width: 1280, height: 720 });
  await page
    .locator('.project-tab-demo__surface')
    .screenshot({ path: 'target/visual-captures/hs2-9b7z7j-project-tab-segments-wide.png' });
  await page.setViewportSize({ width: 560, height: 844 });
  await expect
    .poll(() => page.locator('.project-tab-demo__surface').evaluate((node) => node.scrollWidth <= node.clientWidth))
    .toBe(true);
  await page
    .locator('.project-tab-demo__surface')
    .screenshot({ path: 'target/visual-captures/hs2-9b7z7j-project-tab-segments-narrow.png' });
});

test('catalogs the drawer phone focus-mode text-size control with and without the keyboard (HS2-01D4JP)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 1100 });
  await page.goto('/ux-demo?component=terminal-drawer');
  const variants = page.locator('.terminal-drawer-focus-demo__variant');
  await expect(variants).toHaveCount(2);
  const shown = variants.nth(0),
    underKeyboard = variants.nth(1),
    control = shown.getByRole('button', { name: /^Text size: \d+ columns/ });
  await expect(shown.locator('[data-component="terminal-drawer"]')).toHaveAttribute('data-focus-mode', 'true');
  await expect(control).toBeVisible();
  await expect(control).toHaveAttribute('data-columns', '60');
  await expect(control).toHaveCSS('cursor', 'pointer');
  await expect(shown.getByRole('button', { name: 'Exit terminal focus' })).toBeVisible();
  await expect(underKeyboard.getByRole('button', { name: /^Text size/ })).toBeHidden();
  await expect(underKeyboard.getByRole('button', { name: 'Exit terminal focus' })).toBeVisible();
  // Each fixed focus surface (and its blackout backdrop) stays inside its stage instead of covering the page.
  for (const variant of [shown, underKeyboard]) {
    const contained = await variant.evaluate((node) => {
      const stage = node.querySelector('.terminal-drawer-focus-demo__stage')!.getBoundingClientRect(),
        drawer = node.querySelector('[data-component="terminal-drawer"]')!.getBoundingClientRect();
      return (
        drawer.left >= stage.left - 1 &&
        drawer.top >= stage.top - 1 &&
        drawer.right <= stage.right + 1 &&
        drawer.bottom <= stage.bottom + 1
      );
    });
    expect(contained).toBe(true);
  }
  const drawerDemo = page.locator('.terminal-drawer-demo [data-component="terminal-drawer"]');
  await expect(drawerDemo).toBeVisible();
  // The drawer configures its dedicated TerminalSession through `focus`: inset in the drawer, edge to
  // edge in focus mode, with no drawer CSS reaching into the session (HS2-YNW0B3).
  const dedicated = (scope: typeof drawerDemo) =>
    scope.locator('[data-component="terminal-session"]:not([hidden]) [data-component="terminal-viewport"]');
  await expect(drawerDemo.locator('[data-component="terminal-session"]:not([hidden])')).toHaveAttribute(
    'data-focus',
    'false',
  );
  await expect(dedicated(drawerDemo)).toHaveCSS('padding', '8px');
  await expect(shown.locator('[data-component="terminal-session"]:not([hidden])')).toHaveAttribute(
    'data-focus',
    'true',
  );
  await expect(dedicated(shown)).toHaveCSS('padding', '0px');
  await expect(dedicated(shown)).toHaveCSS('overflow', 'hidden');
  // The control cycles the column fixture; both variants share it.
  await control.click();
  await expect(control).toHaveAttribute('data-columns', '50');
  await expect(control).toHaveAccessibleName('Text size: 50 columns. Change text size');
  await page
    .locator('.terminal-drawer-focus-demo')
    .screenshot({ path: test.info().outputPath('drawer-focus-demo.png') });
});

test('catalogs shared application tabs and terminal-drawer tabs', async ({ page }) => {
  test.setTimeout(45_000);
  await page.goto('/ux-demo?component=app-tab');
  await expect(
    page.locator('.app-tab-demo [data-component="tab-bar"]').getByRole('tablist', {
      name: 'Shared application tab demo',
    }),
  ).toBeVisible();
  const sharedTabs = page.locator('[data-component$="-tab"]');
  await expect(sharedTabs).toHaveCount(2);
  await expect(page.getByRole('tab', { name: /Project tab/ })).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByRole('button', { name: 'Close Terminal tab' })).toBeAttached();
  await page.locator('.app-tab-demo').screenshot({ path: 'target/visual-captures/hs2-gx51f7-app-tabs-wide.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.locator('.app-tab-demo').screenshot({ path: 'target/visual-captures/hs2-gx51f7-app-tabs-narrow.png' });
  const demoTabStrip = page.locator('.app-tab-demo [data-kui-tab-list]');
  await expect.poll(() => demoTabStrip.evaluate((node) => node.scrollWidth - node.clientWidth)).toBeGreaterThan(0);
  await demoTabStrip.evaluate((node) => (node.scrollLeft = node.scrollWidth));
  await expect(page.getByRole('tab', { name: /Terminal tab/ })).toBeInViewport();
  await page.goto('/ux-demo?component=terminal-drawer');
  const terminalDrawer = page.locator('.terminal-drawer-demo [data-component="terminal-drawer"]');
  await expect(terminalDrawer).toBeVisible();
  await expect(terminalDrawer.locator('[data-tab-kind="terminal"]')).toHaveCount(1);
  await expect(terminalDrawer.locator('.kui-tab-bar__trailing [data-component="toolbar-control-group"]')).toHaveCount(
    1,
  );
  await expect(terminalDrawer.locator('.kui-tab-bar__end [data-component="toolbar-control-group"]')).toHaveCount(1);
  await expect(terminalDrawer.getByRole('button', { name: 'Close Development' })).toBeAttached();
  const gridTab = terminalDrawer.getByRole('tab', { name: 'Project grid' }),
    gridRoot = terminalDrawer.locator('[data-component="app-tab"][data-pinned="true"]'),
    gridWidth = (await gridTab.boundingBox())!.width;
  await expect(gridTab.locator('[data-lucide="layout-grid"]')).toBeVisible();
  await expect(gridTab.locator('[data-lucide="grid-3x3"]')).toHaveCount(0);
  await expect(gridRoot).toHaveCSS('flex-shrink', '0');
  await terminalDrawer.evaluate((node) => {
    node.style.width = '520px';
    const tabs = node.querySelector('.kui-tab-bar__tabs')!,
      source = tabs.querySelector('[data-tab-kind="terminal"]')!;
    for (let index = 0; index < 8; index += 1) tabs.append(source.cloneNode(true));
  });
  await expect(terminalDrawer.locator('[data-tab-kind="terminal"]')).toHaveCount(9);
  expect((await gridTab.boundingBox())!.width).toBeCloseTo(gridWidth, 0);
  // The strip overflows once its tabs have laid out; poll rather than sample one frame (HS2-MHPHZB).
  await expect
    .poll(() => terminalDrawer.locator('.kui-tab-bar__tabs').evaluate((node) => node.scrollWidth - node.clientWidth))
    .toBeGreaterThan(0);
  await terminalDrawer
    .locator('.terminal-drawer__rail')
    .screenshot({ path: 'target/visual-captures/hs2-e3j0vv-fixed-layout-grid-tab.png' });
});

test('operates the project tab bar across pointer, keyboard, and responsive states', async ({ page }) => {
  test.setTimeout(45_000);
  await page.goto('/ux-demo?component=project-tabs');
  await page.setViewportSize({ width: 1600, height: 900 });
  const tabBar = page.locator('.project-tab-bar').first();
  // Workspace-action variant: + stays beside the last tab while Kerf's TabBar `end` zone pins the
  // workspace action at the far edge of the bar (HS2-NE8JBS, KF-A59SC4, HS2-T44PFW).
  const withAction = page.getByRole('navigation', { name: 'Open projects with workspace action' });
  const actionGeometry = await withAction.evaluate((bar) => {
    const tabs = bar.querySelector('.kui-tab-bar__tabs')!.getBoundingClientRect(),
      add = bar.querySelector('[data-action="choose-project"]')!.getBoundingClientRect(),
      action = bar.querySelector('.kui-tab-bar__end > wa-button')!.getBoundingClientRect(),
      edge = bar.getBoundingClientRect();
    return { addGap: add.left - tabs.right, actionGap: action.left - add.right, edgeGap: edge.right - action.right };
  });
  expect(actionGeometry.addGap).toBeLessThan(24);
  expect(actionGeometry.actionGap).toBeGreaterThan(48);
  expect(actionGeometry.edgeGap).toBeLessThan(24);
  // The app strip adds the outer 4px inline inset; Kerf's TabBar pads and gaps by its 4px gap token.
  await expect(tabBar).toHaveCSS('padding', '0px 4px');
  await expect(tabBar.locator('[data-component="tab-bar"]')).toHaveCSS('padding', '4px');
  await expect(tabBar.locator('[data-component="tab-bar"]')).toHaveCSS('gap', '4px');
  await expect(tabBar.getByRole('tab')).toHaveCount(4);
  // The tabs strip does not stretch, so the trailing Add-project (+) button follows the tabs instead of
  // being pushed to the far right (HS2-HV52WR).
  expect(await tabBar.locator('.kui-tab-bar__tabs').evaluate((n) => getComputedStyle(n).flexGrow)).toBe('0');
  expect(
    await page.evaluate(() => {
      const t = document.querySelector('.project-tab-bar .kui-tab-bar__tabs'),
        p = document.querySelector('.project-tab-bar__actions [data-action="choose-project"]');
      return t && p ? p.getBoundingClientRect().left - t.getBoundingClientRect().right : -1;
    }),
  ).toBeLessThan(24);
  await expect(tabBar.getByRole('tab', { name: /Hot Sheet 2/ }).locator('.project-tab__work')).toHaveCount(0);
  await expect(tabBar.getByRole('tab', { name: /Small Tale Website/ }).locator('.project-tab__work')).toHaveAttribute(
    'aria-label',
    '3 Up Next tickets, 1 active ticket',
  );
  await expect(tabBar.getByRole('tab', { name: /Internal API/ }).locator('.project-tab__work-count')).toHaveText('99+');
  await expect(tabBar.locator('[data-tab-kind="project"]').first()).toHaveCSS('border-radius', '22px');
  const firstClose = tabBar.getByRole('button', { name: 'Close Hot Sheet 2' });
  await expect(firstClose).toHaveCSS('opacity', '0');
  await tabBar.getByRole('tab', { name: /Hot Sheet 2/ }).hover();
  await expect(firstClose).toHaveCSS('opacity', '1');
  const closeGeometry = await tabBar
    .locator('[data-tab-kind="project"]')
    .first()
    .evaluate((node) => {
      const close = node.querySelector<HTMLElement>('.kui-app-tab__close')!.getBoundingClientRect();
      const select = node.querySelector<HTMLElement>('.kui-app-tab__select')!.getBoundingClientRect();
      const name = node.querySelector<HTMLElement>('.kui-app-tab__name')!.getBoundingClientRect();
      return {
        closeWidth: close.width,
        closeLeft: close.left,
        selectLeft: select.left,
        nameLeft: name.left,
        closeBackground: getComputedStyle(node.querySelector('.kui-app-tab__close')!).backgroundColor,
        selectPaddingRight: getComputedStyle(node.querySelector('.kui-app-tab__select')!).paddingRight,
      };
    });
  expect(closeGeometry.closeWidth).toBeCloseTo(20.4, 0);
  expect(closeGeometry.closeLeft).toBeLessThan(closeGeometry.selectLeft);
  expect(closeGeometry.nameLeft).toBeGreaterThan(closeGeometry.selectLeft);
  expect(closeGeometry.closeBackground).toBe('rgba(0, 0, 0, 0)');
  expect(closeGeometry.selectPaddingRight).toBe('8px');
  const firstTab = tabBar.locator('[data-tab-kind="project"]').first(),
    firstSelect = firstTab.getByRole('tab');
  const naturalLabelCenterOffset = async () =>
    firstTab.evaluate((node) => {
      const tab = node.getBoundingClientRect(),
        label = node.querySelector('.kui-app-tab__name')!.getBoundingClientRect();
      return Math.abs(tab.x + tab.width / 2 - label.x - label.width / 2);
    });
  await expect.poll(naturalLabelCenterOffset).toBeLessThanOrEqual(4);
  await firstSelect.focus();
  await expect(firstSelect).toBeFocused();
  const focusPresentation = await firstTab.evaluate((node) => {
    const root = getComputedStyle(node),
      select = getComputedStyle(node.querySelector('.kui-app-tab__select')!);
    return {
      rootOutlineStyle: root.outlineStyle,
      rootOutlineWidth: root.outlineWidth,
      rootOutlineOffset: root.outlineOffset,
      selectOutlineStyle: select.outlineStyle,
    };
  });
  expect(focusPresentation).toMatchObject({
    rootOutlineStyle: 'solid',
    rootOutlineWidth: '3px',
    rootOutlineOffset: '-2px',
    selectOutlineStyle: 'none',
  });
  await firstTab.screenshot({ path: 'target/visual-captures/hs2-3n470h-project-tab-centered-wide.png' });
  await tabBar.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-project-tab-bar.png' });
  await page.screenshot({ path: 'target/visual-captures/hs2-mrz10b-project-tab-focus-wide.png', fullPage: true });
  const tabActionCenters = await tabBar.evaluate((node) => {
    const tab = node.querySelector('[data-tab-kind="project"]')!.getBoundingClientRect();
    const add = node.querySelector('[data-action="choose-project"] svg')!.getBoundingClientRect();
    return { tab: tab.y + tab.height / 2, add: add.y + add.height / 2 };
  });
  expect(tabActionCenters.add).toBeCloseTo(tabActionCenters.tab, 0);
  const order = await tabBar
    .locator('[data-component="tab-bar"] > *')
    .evaluateAll((nodes) => nodes.map((node) => node.className));
  expect(order).toEqual(['kui-tab-bar__leading', 'kui-tab-bar__tabs', 'kui-tab-bar__trailing']);
  await expect(tabBar.getByRole('button', { name: 'More projects' })).toHaveCount(0);
  const busySpinner = tabBar.getByRole('tab', { name: /Small Tale Website/ }).locator('.project-tab__work');
  const spinnerAlignment = await busySpinner.evaluate((node) => {
    const outer = node.getBoundingClientRect();
    const icon = node.querySelector('svg')!.getBoundingClientRect();
    const tab = node.closest('[data-tab-kind="project"]')!.getBoundingClientRect();
    return {
      x: Math.abs(outer.left + outer.width / 2 - (icon.left + icon.width / 2)),
      y: Math.abs(outer.top + outer.height / 2 - (icon.top + icon.height / 2)),
      tabY: Math.abs(outer.top + outer.height / 2 - (tab.top + tab.height / 2)),
    };
  });
  expect(spinnerAlignment.x).toBeLessThan(1);
  expect(spinnerAlignment.y).toBeLessThan(1);
  expect(spinnerAlignment.tabY).toBeLessThan(1);
  const animatedCenters = await busySpinner.evaluate(async (node) => {
    const centers: Array<[number, number]> = [];
    for (let frame = 0; frame < 12; frame++)
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => {
          const box = node.querySelector('svg')!.getBoundingClientRect();
          centers.push([box.x + box.width / 2, box.y + box.height / 2]);
          resolve();
        }),
      );
    return centers;
  });
  expect(Math.max(...animatedCenters.map(([x]) => x)) - Math.min(...animatedCenters.map(([x]) => x))).toBeLessThan(0.1);
  expect(Math.max(...animatedCenters.map(([, y]) => y)) - Math.min(...animatedCenters.map(([, y]) => y))).toBeLessThan(
    0.1,
  );
  // Kerf's max-content tabs never truncate a project name; the strip scrolls instead.
  for (const name of await tabBar.locator('.kui-app-tab__name').all()) {
    expect(await name.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
  }
  await page.setViewportSize({ width: 760, height: 900 });
  const overflowState = await tabBar.evaluate((node) => {
    const strip = node.querySelector<HTMLElement>('.kui-tab-bar__tabs')!;
    const selected = node
      .querySelector<HTMLElement>('[data-tab-kind="project"][data-selected="true"]')!
      .getBoundingClientRect();
    const viewport = strip.getBoundingClientRect();
    strip.scrollLeft = 100;
    return {
      clientWidth: strip.clientWidth,
      scrollWidth: strip.scrollWidth,
      scrollLeft: strip.scrollLeft,
      overflowX: getComputedStyle(strip).overflowX,
      shadowClearance: viewport.bottom - selected.bottom,
    };
  });
  expect(overflowState.scrollWidth).toBeGreaterThan(overflowState.clientWidth);
  expect(overflowState.scrollLeft).toBeGreaterThan(0);
  expect(overflowState.overflowX).toBe('auto');
  expect(overflowState.shadowClearance).toBeGreaterThanOrEqual(3);
  await tabBar.locator('.kui-tab-bar__tabs').evaluate((node) => {
    node.scrollLeft = 0;
  });
  await firstSelect.focus();
  await expect(firstSelect).toBeFocused();
  await expect.poll(naturalLabelCenterOffset).toBeLessThanOrEqual(4);
  await page.screenshot({ path: 'target/visual-captures/hs2-mrz10b-project-tab-focus-narrow.png', fullPage: true });
  await firstSelect.blur();
  await page.mouse.move(750, 899);
  await expect(firstClose).toHaveCSS('opacity', '0');
  await expect.poll(naturalLabelCenterOffset).toBeLessThanOrEqual(4);
  await firstTab.screenshot({ path: 'target/visual-captures/hs2-3n470h-project-tab-centered-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 900 });
  await tabBar.getByRole('tab', { name: /Hot Sheet 2/ }).click({ button: 'right' });
  const tabMenu = page.getByRole('menu', { name: 'Project tab actions' });
  await expect(tabMenu.locator('wa-dropdown-item')).toHaveCount(4);
  await expect(tabMenu.locator('[data-lucide]')).toHaveCount(4);
  await page.keyboard.press('Escape');
  await expect(tabMenu).toHaveCount(0);
  await tabBar.getByRole('tab', { name: /Hot Sheet 2/ }).click({ button: 'right' });
  await tabMenu.locator('wa-dropdown-item', { hasText: 'Close Tabs to the Right' }).click();
  await expect(tabBar.getByRole('tab')).toHaveCount(1);
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.reload();
  await expect(tabBar.getByRole('tab')).toHaveCount(4);
  await tabBar.getByRole('tab', { name: /Internal API/ }).click();
  await expect(tabBar.getByRole('tab', { name: /Internal API/ })).toHaveAttribute('aria-selected', 'true');
  // Project tabs use the application's manual activation through Kerf's wireTabBars (HS2-KB5PJQ):
  // arrows move roving focus only, and Enter selects the focused project.
  await page.keyboard.press('ArrowLeft');
  await expect(tabBar.getByRole('tab', { name: /Small Tale Website/ })).toBeFocused();
  await expect(tabBar.getByRole('tab', { name: /Internal API/ })).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('Enter');
  await expect(tabBar.getByRole('tab', { name: /Small Tale Website/ })).toHaveAttribute('aria-selected', 'true');
  await expect(tabBar.getByRole('tab', { name: /Small Tale Website/ })).toBeFocused();
  await tabBar.getByRole('tab', { name: /Internal API/ }).click();
  await tabBar.getByRole('button', { name: 'Close Internal API' }).click();
  await expect(tabBar.getByRole('tab', { name: /Internal API/ })).toHaveCount(0);
  await tabBar.getByRole('button', { name: 'Add project' }).click();
  await expect(tabBar.getByRole('tab', { name: /New Project 1/ })).toHaveAttribute('aria-selected', 'true');
  await tabBar.getByRole('button', { name: 'Workspace grid' }).click();
  await expect(tabBar.getByRole('button', { name: 'Workspace grid' })).toHaveAttribute('aria-pressed', 'true');
  await expect(
    tabBar.getByRole('button', { name: 'Workspace grid' }).locator('[data-lucide="grid-3x3"]'),
  ).toBeVisible();
  await expect(tabBar.getByRole('tab', { selected: true })).toHaveCount(0);
  await tabBar.screenshot({ path: 'target/visual-captures/hs2-hpy5r0-workspace-grid-launcher.png' });
  await tabBar.getByRole('tab', { name: /Hot Sheet 2/ }).click();
  await expect(tabBar.getByRole('button', { name: 'Workspace grid' })).toHaveAttribute('aria-pressed', 'false');
});

test('operates resizable-region keyboard and collapse transitions', async ({ page }) => {
  test.setTimeout(45_000);
  await page.goto('/ux-demo?component=resizable-region');
  const horizontalRegion = page.locator('[data-region-id="resize-demo-horizontal"]');
  const horizontalHandle = horizontalRegion.locator('.kui-resizable-region__handle');
  const horizontal = page.getByRole('separator', { name: 'Resize Example sidebar' });
  const vertical = page.getByRole('separator', { name: 'Resize Example drawer' });
  await expect(horizontal).toHaveAttribute('aria-orientation', 'vertical');
  await expect(vertical).toHaveAttribute('aria-orientation', 'horizontal');
  await horizontal.focus();
  await page.keyboard.press('ArrowRight');
  await expect(horizontal).toHaveAttribute('aria-valuenow', '276');
  for (let index = 0; index < 20; index += 1) await page.keyboard.press('ArrowLeft');
  await expect(horizontal).toHaveAttribute('aria-valuenow', '250');
  await vertical.focus();
  await page.keyboard.press('ArrowDown');
  await expect(vertical).toHaveAttribute('aria-valuenow', '196');
  await expect(horizontal).toHaveCSS('cursor', 'col-resize');
  await expect(vertical).toHaveCSS('cursor', 'row-resize');
  // HS2-NADSB8: the Lucide icon sits in the wa-button start slot, on the label's line.
  const collapseHost = page.locator('wa-button[data-action="toggle-resizable-collapse"]');
  const iconPlacement = () =>
    collapseHost.evaluate((button) => {
      const icon = button.querySelector(':scope > svg[slot="start"]');
      const label = button.shadowRoot?.querySelector('[part~="label"]');
      if (!icon || !label) return null;
      const iconBox = icon.getBoundingClientRect();
      const labelBox = label.getBoundingClientRect();
      return {
        icon: icon.getAttribute('data-lucide'),
        label: button.textContent.trim(),
        sameLine: iconBox.right <= labelBox.left && iconBox.top < labelBox.bottom && iconBox.bottom > labelBox.top,
      };
    });
  await expect
    .poll(iconPlacement)
    .toEqual({ icon: 'panel-left-close', label: 'Collapse horizontal region', sameLine: true });
  await page.getByRole('button', { name: 'Collapse horizontal region' }).click();
  await expect(horizontal).toHaveCount(0);
  await expect
    .poll(iconPlacement)
    .toEqual({ icon: 'panel-left-open', label: 'Restore horizontal region', sameLine: true });
  await expect(horizontalHandle).toHaveAttribute('aria-valuenow', '0');
  await expect(horizontalHandle).toHaveAttribute('aria-valuemin', '0');
  await expect(horizontalHandle).toHaveAttribute('aria-hidden', 'true');
  await page.getByRole('button', { name: 'Restore horizontal region' }).click();
  await expect(page.getByRole('separator', { name: 'Resize Example sidebar' })).toHaveAttribute('aria-valuenow', '250');
});

test('resizes ResizableRegion demos through Kerf wireResizableRegions pointer and keyboard paths (HS2-KB5PJQ)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=resizable-region');
  const demo = page.getByRole('region', { name: 'ResizableRegion demo' });
  const horizontal = page.getByRole('separator', { name: 'Resize Example sidebar' });
  const vertical = page.getByRole('separator', { name: 'Resize Example drawer' });
  await expect(horizontal).toHaveAttribute('aria-valuenow', '260');
  // Hover the live separator so layout shifts under concurrent load cannot leave the pointer
  // at a stale bounding-box coordinate before Kerf receives pointerdown (HS2-PYRH81).
  await horizontal.hover();
  const box = (await horizontal.boundingBox())!;
  await page.mouse.down();
  await expect(
    page.locator('[data-component="resizable-region"][data-region-id="resize-demo-horizontal"]'),
  ).toHaveAttribute('data-resizing', 'true');
  await page.mouse.move(box.x + box.width / 2 + 60, box.y + box.height / 2, { steps: 4 });
  await expect(horizontal).toHaveAttribute('aria-valuenow', '320');
  await page.mouse.move(box.x + box.width / 2 + 600, box.y + box.height / 2, { steps: 4 });
  await expect(horizontal).toHaveAttribute('aria-valuenow', '420');
  await page.mouse.up();
  await expect(demo.getByText('420px')).toBeVisible();
  await horizontal.focus();
  await page.keyboard.press('Home');
  await expect(horizontal).toHaveAttribute('aria-valuenow', '250');
  await expect(demo.getByText('250px')).toBeVisible();
  await page.keyboard.press('Shift+ArrowRight');
  await expect(horizontal).toHaveAttribute('aria-valuenow', '314');
  await page.keyboard.press('End');
  await expect(horizontal).toHaveAttribute('aria-valuenow', '420');
  await vertical.hover();
  const verticalBox = (await vertical.boundingBox())!;
  await page.mouse.down();
  await expect(
    page.locator('[data-component="resizable-region"][data-region-id="resize-demo-vertical"]'),
  ).toHaveAttribute('data-resizing', 'true');
  await page.mouse.move(verticalBox.x + verticalBox.width / 2, verticalBox.y + verticalBox.height / 2 - 40, {
    steps: 4,
  });
  await page.mouse.up();
  await expect(vertical).toHaveAttribute('aria-valuenow', '140');
  await expect(demo.getByText('140px')).toBeVisible();
  // Reset through collapse/restore and drag again: the controlled size and the wiring stay in sync.
  await page.getByRole('button', { name: 'Collapse horizontal region' }).click();
  await page.getByRole('button', { name: 'Restore horizontal region' }).click();
  const restored = page.getByRole('separator', { name: 'Resize Example sidebar' });
  await expect(restored).toHaveAttribute('aria-valuenow', '420');
  const restoredBox = (await restored.boundingBox())!;
  await page.mouse.move(restoredBox.x + restoredBox.width / 2, restoredBox.y + restoredBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(restoredBox.x + restoredBox.width / 2 - 100, restoredBox.y + restoredBox.height / 2, {
    steps: 4,
  });
  await page.mouse.up();
  await expect(restored).toHaveAttribute('aria-valuenow', '320');
  await expect(demo.getByText('320px')).toBeVisible();
});

test('reorders demo project tabs through Kerf wireTabBars by keyboard and drag (HS2-KB5PJQ)', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto('/ux-demo?component=project-tabs');
  const bar = page.getByRole('navigation', { name: 'Open projects', exact: true });
  const names = () =>
    bar
      .locator('[data-tab-kind="project"]')
      .evaluateAll((tabs) => tabs.map((tab) => (tab as HTMLElement).dataset.tabId));
  await expect.poll(names).toEqual(['hotsheet', 'website', 'api', 'archive']);
  await bar.getByRole('tab', { name: /Hot Sheet 2/ }).focus();
  await page.keyboard.press('Alt+Shift+ArrowRight');
  await expect.poll(names).toEqual(['website', 'hotsheet', 'api', 'archive']);
  await expect(bar.getByRole('tab', { name: /Hot Sheet 2/ })).toBeFocused();
  // Both ProjectTabBar demos render the same controlled order.
  const second = page.getByRole('navigation', { name: 'Open projects with workspace action' });
  await expect
    .poll(() =>
      second
        .locator('[data-tab-kind="project"]')
        .evaluateAll((tabs) => tabs.map((tab) => (tab as HTMLElement).dataset.tabId)),
    )
    .toEqual(['website', 'hotsheet', 'api', 'archive']);
  // The shell-column variant shares the column surface and draws no bottom rule (HS2-DR549A).
  const shellColumn = page.locator('.project-tab-bar-demo .project-tab-bar').filter({
    has: page.getByRole('navigation', { name: 'Open projects in the shell column' }),
  });
  await expect(shellColumn).toHaveAttribute('data-surface', 'default');
  await expect(shellColumn).toHaveAttribute('data-divider', 'false');
  await expect(shellColumn).toHaveCSS('border-bottom-width', '0px');
  await expect(page.locator('.project-tab-bar-demo .project-tab-bar').first()).toHaveCSS('border-bottom-width', '1px');
  await expect
    .poll(() =>
      shellColumn
        .locator('[data-tab-kind="project"]')
        .evaluateAll((tabs) => tabs.map((tab) => (tab as HTMLElement).dataset.tabId)),
    )
    .toEqual(['website', 'hotsheet', 'api', 'archive']);
  await expect(page.locator('.project-tab-bar-demo .component-stage__event')).toHaveText(
    'Moved hotsheet after website by keyboard.',
  );
  await bar
    .locator('[data-component="app-tab"][data-tab-id="archive"]')
    .dragTo(bar.locator('[data-component="app-tab"][data-tab-id="website"]'), { targetPosition: { x: 4, y: 10 } });
  await expect.poll(names).toEqual(['archive', 'website', 'hotsheet', 'api']);
  await expect(page.locator('.project-tab-bar-demo .component-stage__event')).toHaveText(
    'Moved archive before website by pointer.',
  );
  // Delete closes the focused tab through the same wiring.
  await bar.getByRole('tab', { name: /Internal API/ }).focus();
  await page.keyboard.press('Delete');
  await expect.poll(names).toEqual(['archive', 'website', 'hotsheet']);
});

test('renders and reconnects the connection-state banner variants', async ({ page }) => {
  test.setTimeout(45_000);
  await page.goto('/ux-demo?component=connection-state-banner');
  const banners = page.locator('[data-component="state-banner"]');
  await expect(banners).toHaveCount(5);
  await expect(page.locator('.connection-state-banner--connecting')).toHaveAttribute('role', 'status');
  await expect(page.locator('.connection-state-banner--offline')).toContainText('Working from offline data');
  await expect(page.locator('.connection-state-banner--incompatible')).toContainText('Server update required');
  await expect(page.locator('.connection-state-banner--authentication')).toContainText('Authentication required');
  await page.screenshot({ path: 'target/visual-captures/hs2-hygwcm-shared-state-banners-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: 'target/visual-captures/hs2-hygwcm-shared-state-banners-narrow.png', fullPage: true });
  await page.getByRole('button', { name: 'Reconnect' }).click();
  await expect(page.getByText('Connection retry requested.')).toBeVisible();
});

test('exercises the application-shell responsive composition', async ({ page }) => {
  test.setTimeout(45_000);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/ux-demo?component=app-shell');
  const shell = page.locator('[data-component="app-shell"]');
  await expect(shell).toBeVisible();
  for (const component of ['state-banner', 'quick-ticket-composer-launcher', 'ticket-list', 'ticket-inspector-header'])
    await expect(shell.locator(`[data-component="${component}"]`)).toHaveCount(1);
  // The header identity and controls are Toolbar zone children, not wrapper components (HS2-EZ1N7Z).
  await expect(shell.locator('.kui-toolbar__leading > .workspace-header__identity')).toHaveCount(1);
  await expect(shell.locator('.kui-toolbar__trailing > .view-mode-switcher')).toHaveCount(1);
  await expect(shell.locator('#app-left-rail [data-component="pane"]')).toHaveCount(1);
  await expect(shell.locator('[data-component="tab-bar"]')).toHaveCount(2);
  const shellHierarchy = await shell.evaluate((node) => {
    const shellRect = node.getBoundingClientRect();
    const toolbarNode = node.querySelector('[data-component="toolbar"][aria-label="Workspace toolbar"]')!;
    const toolbar = toolbarNode.getBoundingClientRect();
    const leading = toolbarNode.querySelector('.kui-toolbar__leading')!.getBoundingClientRect();
    const trailing = toolbarNode.querySelector('.kui-toolbar__trailing')!.getBoundingClientRect();
    const identity = toolbarNode.querySelector('.workspace-header__identity')!.getBoundingClientRect();
    const controls = [...toolbarNode.querySelector('.kui-toolbar__trailing')!.children]
      .filter((child) => getComputedStyle(child).display !== 'none')
      .at(-1)!
      .getBoundingClientRect();
    const tabs = node.querySelector('.project-tab-bar')!.getBoundingClientRect();
    const workArea = node.querySelector('.app-shell__work-area')!.getBoundingClientRect();
    const inspector = node.querySelector('#app-right-rail')!.getBoundingClientRect();
    return {
      shellTop: shellRect.top,
      toolbarTop: toolbar.top,
      toolbarBottom: toolbar.bottom,
      toolbarRight: toolbar.right,
      toolbarGap: getComputedStyle(toolbarNode).columnGap,
      leadingLeft: leading.left,
      identityLeft: identity.left,
      trailingRight: trailing.right,
      controlsRight: controls.right,
      tabsTop: tabs.top,
      tabsBottom: tabs.bottom,
      workAreaTop: workArea.top,
      inspectorTop: inspector.top,
    };
  });
  expect(shellHierarchy.toolbarTop - shellHierarchy.shellTop).toBeLessThanOrEqual(1);
  expect(shellHierarchy.identityLeft).toBeGreaterThanOrEqual(shellHierarchy.leadingLeft);
  expect(shellHierarchy.controlsRight).toBeCloseTo(shellHierarchy.trailingRight, 0);
  expect(shellHierarchy.toolbarRight - shellHierarchy.trailingRight).toBeCloseTo(8, 0);
  expect(shellHierarchy.toolbarGap).toBe('8px');
  await expect(shell.locator('[data-component="toolbar"][aria-label="Workspace toolbar"]')).toHaveAttribute(
    'data-has-center',
    'false',
  );
  await expect(shell.locator('[data-component="toolbar"][aria-label="Workspace toolbar"]')).toHaveCSS(
    'box-shadow',
    /^(rgba\(0, 0, 0, 0\) [^,]+)(, rgba\(0, 0, 0, 0\) [^,]+){3}$/,
  );
  expect(shellHierarchy.tabsTop).toBeCloseTo(shellHierarchy.toolbarBottom, 0);
  expect(shellHierarchy.workAreaTop).toBeGreaterThanOrEqual(shellHierarchy.tabsBottom);
  expect(shellHierarchy.inspectorTop - shellHierarchy.shellTop).toBeLessThanOrEqual(1);
  await expect(shell.locator('.app-shell__main > .app-heading')).toHaveCount(0);
  await expect(shell.locator('#app-shell-demo-page-title')).toHaveText('Queue');
  await expect(shell.locator('#app-shell-demo-page-title')).toHaveAttribute('data-size', 'large');
  await expect(shell.locator('.project-tab-bar [data-component="quick-ticket-composer-launcher"]')).toHaveCount(1);
  await page.screenshot({ path: 'target/visual-captures/hs2-501eph-toolbar-wide.png', fullPage: true });
  await expect(
    shell.getByRole('button', { name: 'Hide ticket inspector' }).locator('[data-lucide="panel-right-close"]'),
  ).toHaveCount(1);
  await expect(shell.locator('.project-tab-bar')).toHaveCSS('background-color', 'rgb(255, 255, 255)');
  await expect(shell.locator('.project-tab-bar')).toHaveCSS('border-bottom-width', '0px');
  await expect(shell.locator('#app-left-rail')).toHaveCSS('background-color', 'rgb(255, 255, 255)');
  await shell.getByRole('button', { name: /New ticket/ }).click();
  const shellComposer = page.getByRole('dialog', { name: 'Create ticket' });
  await expect(shellComposer.getByRole('textbox', { name: 'Ticket title' })).toBeFocused();
  const composerControlHeights = await shellComposer.evaluate((node) => {
    const input = node
      .querySelector('wa-input')!
      .shadowRoot!.querySelector<HTMLElement>('[part~="base"]')!
      .getBoundingClientRect();
    const select = node
      .querySelector('wa-select')!
      .shadowRoot!.querySelector<HTMLElement>('[part~="combobox"]')!
      .getBoundingClientRect();
    return { input: input.height, select: select.height };
  });
  expect(composerControlHeights.input).toBeCloseTo(composerControlHeights.select, 0);
  await shellComposer.getByRole('button', { name: 'Cancel' }).click();
  await expect(shellComposer).toBeHidden();
  await shell.getByRole('button', { name: /New ticket/ }).click();
  await expect(shellComposer.getByRole('textbox', { name: 'Ticket title' })).toBeFocused();
  await shellComposer.getByRole('button', { name: 'Cancel' }).click();
  await expect(shellComposer).toBeHidden();
  const inspectorToolbarAlignment = await shell
    .locator('#app-right-rail [data-component="toolbar"][aria-label="Ticket inspector toolbar"]')
    .evaluate((node) => {
      const slug = node.querySelector('[data-component="toolbar-text"]')!.getBoundingClientRect();
      const controls = node.querySelector('[data-component="toolbar-control-group"]')!.getBoundingClientRect();
      return Math.abs(slug.top + slug.height / 2 - (controls.top + controls.height / 2));
    });
  expect(inspectorToolbarAlignment).toBeLessThan(1);
  const compactInspectorTab = shell.getByRole('tab', { name: 'Timeline' });
  await expect(compactInspectorTab.locator('svg')).toHaveCSS('flex-shrink', '0');
  expect(
    await compactInspectorTab.locator('.kui-app-tab__name').evaluate((label) => {
      const style = getComputedStyle(label);
      return { position: style.position, width: style.width, clipPath: style.clipPath };
    }),
  ).toEqual({ position: 'absolute', width: '1px', clipPath: 'inset(50%)' });
  const sidebarHandle = shell.getByRole('separator', { name: 'Resize Sidebar rail' });
  await expect(sidebarHandle).toHaveAttribute('aria-valuemin', '250');
  await expect(sidebarHandle).toHaveAttribute('aria-valuenow', '272');
  await sidebarHandle.focus();
  await expect(sidebarHandle).toBeFocused();
  await page.keyboard.press('ArrowRight');
  await expect(sidebarHandle).toHaveAttribute('aria-valuenow', '288');
  const inspectorHandle = shell.getByRole('separator', { name: 'Resize Inspector rail' });
  await inspectorHandle.focus();
  await expect(inspectorHandle).toBeFocused();
  await page.keyboard.press('ArrowLeft');
  await expect(inspectorHandle).toHaveAttribute('aria-valuenow', '368');
  await shell.screenshot({ path: 'target/visual-captures/hs2-ptm2rt-app-shell-keyboard-resize.png' });
  await page.keyboard.press('ArrowRight');
  await expect(inspectorHandle).toHaveAttribute('aria-valuenow', '352');
  const inspectorHandleBox = await inspectorHandle.boundingBox();
  expect(inspectorHandleBox).not.toBeNull();
  await shell
    .locator('[data-component="ticket-list-row"]')
    .first()
    .evaluate((node) => {
      (node as HTMLElement).dataset.resizeStability = 'same-node';
    });
  await page.mouse.move(inspectorHandleBox!.x + inspectorHandleBox!.width / 2, inspectorHandleBox!.y + 80);
  const inspectorRegion = shell.locator('#app-right-rail');
  // HS2-4KZBTT: the region width must never animate — animating it reflows the whole ticket list
  // per frame. The content still slides via a compositor-only transform.
  await expect(inspectorRegion).toHaveCSS('transition-duration', '0s');
  await page.mouse.down();
  // While dragging, Kerf's wiring marks the rail as resizing (the Workbench stylesheet does not yet
  // suppress the content transform transition on that mark the way ResizableRegion does: KF request
  // filed on HS2-P289N2).
  await expect(inspectorRegion).toHaveAttribute('data-resizing', 'true');
  await page.mouse.move(inspectorHandleBox!.x - 32, inspectorHandleBox!.y + 80);
  await expect(shell.locator('[data-resize-stability="same-node"]')).toHaveCount(1);
  await page.mouse.up();
  // After the drag ends the resizing mark clears, and the region width stays un-animated.
  await expect(inspectorRegion).not.toHaveAttribute('data-resizing');
  await expect(inspectorRegion.locator('.kui-workbench__panel-content')).not.toHaveCSS('transition-duration', '0s');
  await expect(inspectorRegion).toHaveCSS('transition-duration', '0s');
  await expect.poll(async () => Number(await inspectorHandle.getAttribute('aria-valuenow'))).toBeGreaterThan(352);
  await shell.getByRole('tab', { name: 'Timeline' }).click();
  await expect(shell.getByRole('tab', { name: 'Timeline' })).toHaveAttribute('aria-selected', 'true');
  await expect(shell.locator('[data-component="ticket-timeline"]')).toBeVisible();
  await shell.getByRole('tab', { name: 'Attachments' }).click();
  await expect(shell.locator('[data-component="ticket-attachments"]')).toBeVisible();
  await shell.getByRole('tab', { name: 'Info' }).click();
  await expect(shell.locator('[data-component="ticket-info-panel"]')).toBeVisible();
  const inspectorExpandedWidth = Number(await inspectorHandle.getAttribute('aria-valuenow'));
  await shell.getByRole('button', { name: 'Hide ticket inspector' }).click();
  const showInspector = shell.getByRole('button', { name: 'Show ticket inspector' });
  await expect(showInspector).toBeVisible();
  const collapsedInspector = shell.locator('#app-right-rail');
  await expect(collapsedInspector).toHaveAttribute('data-collapsed', 'true');
  await expect(collapsedInspector).toHaveCSS('width', '0px');
  await expect(collapsedInspector.locator('.kui-workbench__panel-content')).toHaveCSS(
    'width',
    `${inspectorExpandedWidth}px`,
  );
  await expect(collapsedInspector.locator('.kui-workbench__panel-content')).not.toHaveCSS('transform', 'none');
  await expect(showInspector.locator('[data-lucide="panel-right-open"]')).toHaveCount(1);
  await expect(showInspector.locator('xpath=ancestor::*[@data-component="tab-bar"]')).toHaveCount(0);
  await expect(showInspector.locator('xpath=ancestor::*[@data-component="toolbar"]')).toHaveCount(1);
  await showInspector.click();
  await expect(shell.locator('[data-component="ticket-inspector-header"]')).toBeVisible();
  const hideSidebar = shell.getByRole('button', { name: 'Hide project sidebar' });
  const crampedToolbar = await shell
    .locator('[data-component="toolbar"][aria-label="Workspace toolbar"]')
    .evaluate((toolbar) => {
      const toolbarRect = toolbar.getBoundingClientRect();
      const actionsRect = toolbar.querySelector('.kui-toolbar__trailing')!.getBoundingClientRect();
      return {
        toolbarLeft: toolbarRect.left,
        toolbarRight: toolbarRect.right,
        actionsLeft: actionsRect.left,
        actionsRight: actionsRect.right,
      };
    });
  expect(crampedToolbar.actionsLeft).toBeGreaterThanOrEqual(crampedToolbar.toolbarLeft);
  expect(crampedToolbar.actionsRight).toBeLessThanOrEqual(crampedToolbar.toolbarRight);
  const sidebarCollapseHit = await hideSidebar.evaluate((button) => {
    const rect = button.getBoundingClientRect();
    const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
    return {
      ownsHit: hit?.closest('[aria-label="Hide project sidebar"]') === button,
      hitLabel: hit?.closest('[aria-label]')?.getAttribute('aria-label'),
    };
  });
  expect(sidebarCollapseHit).toEqual({ ownsHit: true, hitLabel: 'Hide project sidebar' });
  await page.screenshot({ path: 'target/visual-captures/hs2-501eph-toolbar-narrow.png', fullPage: true });
  await hideSidebar.click();
  await expect(shell.locator('#app-left-rail')).toHaveAttribute('data-collapsed', 'true');
  await expect(shell.locator('#app-left-rail')).toHaveCSS('width', '0px');
  const collapsedSidebarContent = shell.locator('#app-left-rail .kui-workbench__panel-content');
  await expect(collapsedSidebarContent).toHaveCSS('width', '288px');
  await expect(collapsedSidebarContent).not.toHaveCSS('transform', 'none');
  const showSidebar = shell.getByRole('button', { name: 'Show project sidebar' });
  await expect(showSidebar.locator('xpath=ancestor::*[@data-component="tab-bar"]')).toHaveCount(0);
  await expect(showSidebar.locator('xpath=ancestor::*[@data-component="toolbar"]')).toHaveCount(1);
  await showSidebar.click();
  await expect(shell.locator('#app-left-rail')).toBeVisible();
  await expect(shell.locator('#app-left-rail')).toHaveAttribute('data-collapsed', 'false');
  await expect(shell.locator('#app-left-rail')).toHaveAttribute('data-separator', 'auto');
  // The Workbench rail draws its separator as its own inline-end border.
  const sidebarSeparator = await shell.locator('#app-left-rail').evaluate((node) => ({
    width: getComputedStyle(node).borderInlineEndWidth,
    background: getComputedStyle(node).borderInlineEndColor,
  }));
  expect(sidebarSeparator).toEqual({ width: '1px', background: 'rgba(0, 0, 28, 0.18)' });
  await shell.getByRole('button', { name: 'Columns view' }).click();
  const shellWorkspace = shell.locator('.app-shell__workspace');
  await expect(shellWorkspace).toHaveAttribute('data-presentation', 'edge-to-edge');
  const boardGeometry = await shellWorkspace.evaluate((node) => {
    const board = node.querySelector('.ticket-board')!;
    const workspaceRect = node.getBoundingClientRect();
    const boardRect = board.getBoundingClientRect();
    return {
      workspaceLeft: workspaceRect.left,
      workspaceRight: workspaceRect.right,
      workspaceBottom: workspaceRect.bottom,
      boardLeft: boardRect.left,
      boardRight: boardRect.right,
      boardBottom: boardRect.bottom,
      boardClientWidth: board.clientWidth,
      boardScrollWidth: board.scrollWidth,
    };
  });
  expect(boardGeometry.boardLeft - boardGeometry.workspaceLeft).toBeCloseTo(0, 0);
  expect(boardGeometry.workspaceRight - boardGeometry.boardRight).toBeCloseTo(0, 0);
  expect(boardGeometry.workspaceBottom - boardGeometry.boardBottom).toBeCloseTo(0, 0);
  expect(boardGeometry.boardScrollWidth).toBeGreaterThanOrEqual(boardGeometry.boardClientWidth);
  await shell.getByRole('button', { name: 'List view' }).click();
  await shell.getByRole('tab', { name: /Small Tale Website/ }).click();
  await expect(shell.locator('[data-tab-kind="project"][data-project-id="website"] [role="tab"]')).toHaveAttribute(
    'aria-selected',
    'true',
  );
  await shell.getByRole('button', { name: 'Settings view' }).click();
  await expect(shell.getByRole('region', { name: 'Project settings' })).toBeVisible();
  await expect(shell.locator('[data-component="ticket-list"]')).toHaveCount(0);
  await expect(shell.locator('[data-component="quick-ticket-composer"]')).toHaveCount(0);
  await expect(shell.locator('#app-right-rail [data-ticket-inspector-placeholder="true"]')).toBeVisible();
  for (const name of ['Sort tickets', 'Favorite view', 'More workspace actions', 'Search tickets']) {
    const control = shell.getByRole('button', { name });
    if (await control.count()) await expect(control).toHaveAttribute('disabled', '');
  }
  await shell.getByRole('button', { name: 'List view' }).click();
  await expect(shell.locator('[data-component="ticket-list"]')).toBeVisible();
  await expect(shell.locator('[data-component="quick-ticket-composer-launcher"]')).toBeVisible();
  await expect(shell.locator('[data-component="ticket-inspector-header"]')).toBeVisible();
  await shell.getByRole('button', { name: 'Search tickets' }).click();
  const shellSearch = shell.getByRole('searchbox', { name: 'Search tickets' });
  await expect(shellSearch).toBeFocused();
  await shellSearch.fill('long-tag-example');
  await expect(shell.locator('[data-component="ticket-list-row"]')).toHaveCount(1);
  await shellSearch.fill('');
  await expect(shell.locator('[data-component="ticket-list-row"]')).not.toHaveCount(1);
  // Escape collapses an empty collapsible field (Kerf's `collapseOnEscape`).
  await shellSearch.press('Escape');
  await expect(shell.getByRole('searchbox', { name: 'Search tickets' })).toHaveCount(0);
  await expect(shell.getByRole('button', { name: 'Search tickets' })).toBeVisible();
  await shell.getByRole('button', { name: 'Workspace grid' }).click();
  await expect(shell).toHaveAttribute('data-mode', 'terminals');
  await expect(shell.locator('.project-sidebar__content')).toHaveCount(0);
  await expect(shell.getByRole('region', { name: 'Terminal operations sidebar' })).toBeVisible();
  await expect(shell.getByRole('button', { name: 'Hide operations sidebar' })).toBeVisible();
  await expect(shell.locator('#app-right-rail')).toHaveAttribute('aria-label', 'Tickets rail');
  await expect(shell.locator('#app-right-rail')).toBeVisible();
  await expect(shell.locator('[data-component="ticket-inspector-header"]')).toBeVisible();
  await expect(shell.locator('[data-component="quick-ticket-composer"]')).toHaveCount(0);
  await expect(shell.locator('.workspace-header__identity').getByText('Workspace grid', { exact: true })).toBeVisible();
  await expect(shell.getByRole('region', { name: 'Workspace grid workspace' })).toBeVisible();
  await expect(shell.locator('.view-mode-switcher')).toHaveCount(0);
  await shell.getByRole('button', { name: 'Cross-project stats' }).click();
  await expect(shell).toHaveAttribute('data-mode', 'stats');
  await expect(shell.getByText('Stats', { exact: true })).toBeVisible();
  await expect(shell.getByRole('region', { name: 'Cross-project stats workspace' })).toBeVisible();
  await shell.getByRole('tab', { name: /Hot Sheet 2/ }).click();
  await expect(shell).toHaveAttribute('data-mode', 'project');
  await expect(shell.locator('#app-left-rail')).toBeVisible();
  await expect(shell.locator('[data-component="ticket-inspector-header"]')).toBeVisible();
  await expect(shell.locator('.view-mode-switcher')).toBeVisible();
  await shell.locator('[data-component="project-summary"]').click();
  await expect(shell).toHaveAttribute('data-mode', 'stats');
  await expect(shell.getByRole('region', { name: 'Hot Sheet 2 project statistics' })).toBeVisible();
  await shell.getByRole('tab', { name: /Hot Sheet 2/ }).click();
  await expect(shell).toHaveAttribute('data-mode', 'project');
  await page.setViewportSize({ width: 760, height: 900 });
  await expect(shell.locator('#app-left-rail')).toBeVisible();
  await expect(shell.locator('#app-right-rail')).toBeVisible();
  // Keep all three user-controlled regions mounted at the supported floor. The
  // center can be clipped until the user explicitly collapses a side region.
  await expect(shell.locator('[data-component="ticket-list"]')).toHaveCount(1);
});

test('projects the feedback-needed indicator through list and board compositions', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-list');
  const listFeedback = page
    .getByRole('listbox', { name: 'Example ticket list' })
    .locator('[data-ticket-slug="HS2-R76MMW"] .ticket-list-row__feedback');
  await expect(listFeedback).toContainText('Needs review');
  await expect(listFeedback.locator('[data-lucide="circle-alert"]')).toHaveCount(1);
  const listRow = page.getByRole('listbox', { name: 'Example ticket list' }).locator('[data-ticket-slug="HS2-R76MMW"]');
  await expect(listRow.locator('.ticket-list-row__indicator--needs-review')).toHaveCSS(
    'background-color',
    'rgb(203, 48, 224)',
  );
  // A ticket without a feedback_needed note shows no indicator.
  await expect(
    page
      .getByRole('listbox', { name: 'Example ticket list' })
      .locator('[data-ticket-slug="HS2-RPVFA4"] .ticket-list-row__feedback'),
  ).toHaveCount(0);

  await page.goto('/ux-demo?component=ticket-board');
  const columnRow = page
    .getByRole('listbox', { name: 'Example status board' })
    .locator('[data-ticket-slug="HS2-R76MMW"]');
  await expect(columnRow.locator('.ticket-list-row__feedback')).toContainText('Needs review');
  await expect(columnRow.locator('.ticket-list-row__indicator--needs-review')).toHaveCSS(
    'background-color',
    'rgb(203, 48, 224)',
  );

  await page.goto('/ux-demo?component=ticket-inspector');
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await expect(inspector).toHaveAttribute('data-needs-review', 'true');
  await expect(inspector.locator('.ticket-inspector__feedback')).toContainText('Needs review');
  expect(
    await inspector.evaluate((node) => {
      const rail = getComputedStyle(node, '::before');
      return { background: rail.backgroundColor, width: rail.width };
    }),
  ).toEqual({ background: 'rgba(0, 0, 0, 0)', width: 'auto' });
});

test('dims finished tickets in the list and uses the shared Tags menu header', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-list');
  const list = page.getByRole('listbox', { name: 'Example ticket list' });
  // Completed/verified rows are dimmed in list mode; active rows are not (HS2-AMBE59).
  await expect(list.locator('[data-ticket-slug="HS2-K00QPZ"]')).toHaveCSS('opacity', '0.55');
  await expect(list.locator('[data-ticket-slug="HS2-RPVFA4"]')).toHaveCSS('opacity', '0.55');
  await expect(list.locator('[data-ticket-slug="HS2-R76MMW"]')).toHaveCSS('opacity', '1');

  // Tags uses the same section-header primitive and trailing action as Views.
  await page.goto('/ux-demo?component=ticket-info-panel');
  // The demo also shows the value-free placeholder variant (HS2-XBHADT), which deliberately keeps the
  // Tags header as loading chrome; the interactive header belongs to the loaded panel (HS2-C7FY1B).
  const tagsHeader = page
    .locator('[data-component="ticket-info-panel"]:not([data-placeholder]) [data-component="list-header"]')
    .filter({ hasText: 'Tags' });
  await expect(tagsHeader).toHaveCount(1);
  await expect(tagsHeader.getByRole('button', { name: 'Add tag' }).locator('[data-lucide="plus"]')).toBeVisible();
  await expect(tagsHeader.getByRole('button', { name: 'Add tag' })).toBeEnabled();
  const placeholderTags = page
    .locator('[data-component="ticket-info-panel"][data-placeholder="true"] [data-component="list-header"]')
    .filter({ hasText: 'Tags' });
  await expect(placeholderTags).toHaveCount(1);
  await expect(placeholderTags.locator('button[aria-label="Add tag"]')).toBeDisabled();
  await expect(page.locator('.ticket-tag-editor__add')).toHaveCount(0);
});

test('resolves the shared Web Awesome and Hot Sheet semantic theme', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-list');
  const row = page.locator('[data-ticket-slug="HS2-R76MMW"]');
  const feedback = row.locator('.ticket-list-row__feedback');
  await expect(row).toBeVisible();
  await expect(feedback).toBeVisible();

  expect(
    await row.evaluate((node) => {
      const root = getComputedStyle(document.documentElement);
      const probe = document.createElement('span');
      probe.style.backgroundColor = 'var(--wa-color-surface-default)';
      document.body.append(probe);
      const surface = getComputedStyle(probe).backgroundColor;
      probe.style.backgroundColor = 'var(--wa-color-warning-fill-quiet)';
      const warning = getComputedStyle(probe).backgroundColor;
      probe.style.backgroundColor = 'var(--hs-ticket-state-needs-review)';
      const review = getComputedStyle(probe).backgroundColor;
      probe.remove();
      const feedbackNode = node.querySelector('.ticket-list-row__feedback');
      const railNode = node.querySelector('.ticket-list-row__indicator--needs-review');
      return {
        aliases: [
          root.getPropertyValue('--hs-ticket-state-needs-review').trim(),
          root.getPropertyValue('--hs-ticket-state-up-next').trim(),
        ],
        reviewMatches: railNode !== null && getComputedStyle(railNode).backgroundColor === review,
        surfaceMatches: getComputedStyle(node).backgroundColor === surface,
        warningMatches: feedbackNode !== null && getComputedStyle(feedbackNode).backgroundColor === warning,
      };
    }),
  ).toEqual({
    aliases: ['#cb30e0', '#ffcc00'],
    reviewMatches: true,
    surfaceMatches: true,
    warningMatches: true,
  });

  await page.goto('/ux-demo?component=app-shell');
  const workArea = page.locator('.app-shell__work-area');
  await expect(workArea).toBeVisible();
  // The wrapper is transparent; Kerf's sunken Pane paints the work surface.
  await expect(workArea).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  const workPane = page.locator('.app-shell__main').locator('xpath=ancestor::*[@data-component="pane"][1]');
  await expect(workPane).toHaveAttribute('data-appearance', 'sunken');
  await expect(workPane.locator('.kui-pane__content').first()).toHaveCSS(
    'background-color',
    await page.evaluate(() => {
      const probe = document.createElement('span');
      probe.style.backgroundColor = 'var(--wa-color-surface-lowered)';
      document.body.append(probe);
      const color = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return color;
    }),
  );
  const sidebarRegion = page.locator('#app-left-rail');
  expect(
    await sidebarRegion.evaluate((node) => {
      // The Workbench rail draws its separator as its own border in the semantic border token.
      const probe = document.createElement('span');
      probe.style.background = 'var(--kui-color-neutral-border-normal)';
      node.append(probe);
      const result = {
        divider: getComputedStyle(node).borderInlineEndColor,
        dividerMatches: getComputedStyle(node).borderInlineEndColor === getComputedStyle(probe).backgroundColor,
      };
      probe.remove();
      return result;
    }),
  ).toEqual({ divider: 'rgba(0, 0, 28, 0.18)', dividerMatches: true });
  await page.screenshot({ path: 'target/visual-captures/hs2-66m88k-semantic-theme-wide.png', fullPage: true });
  await page.setViewportSize({ width: 940, height: 844 });
  await expect(sidebarRegion).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-66m88k-semantic-theme-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 900 });

  await page.goto('/ux-demo?component=permission-request');
  const details = page.locator('.permission-request-card__details');
  await expect(details).toBeVisible();
  await expect(details).toHaveCSS(
    'background-color',
    await page.evaluate(() => {
      const probe = document.createElement('span');
      probe.style.backgroundColor = 'var(--wa-color-surface-lowered)';
      document.body.append(probe);
      const color = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return color;
    }),
  );
  await page.screenshot({ path: 'target/visual-captures/hotsheet-semantic-theme-wide.png', fullPage: true });

  await page.setViewportSize({ width: 760, height: 900 });
  await expect(details).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hotsheet-semantic-theme-narrow.png', fullPage: true });
});

test('previews and resets important PermissionRequestCard variants', async ({ page }) => {
  let releaseDemoModified!: () => void;
  const demoModifiedReady = new Promise<void>((resolve) => {
    releaseDemoModified = resolve;
  });
  await page.route('**/__hotsheet/demo-modified', async (route) => {
    await demoModifiedReady;
    await route.fulfill({ json: { 'permission-request': new Date().toISOString() } });
  });
  await page.setViewportSize({ width: 1728, height: 971 });
  await page.goto('/ux-demo?component=permission-request');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const settings = page.getByRole('complementary', { name: 'PermissionRequestCard settings' });
  const card = page.locator('[data-component="permission-request-card"]');
  const presentation = settings.locator('[name="presentation"]');
  const variant = settings.locator('[name="variant"]');
  const request = settings.locator('[name="request"]');
  const automation = settings.locator('[name="automation"]');
  const alwaysSupported = settings.locator('wa-checkbox[name="always-supported"]');
  const explanation = settings.locator('wa-checkbox[name="explanation"]');
  const choose = async (control: typeof variant, value: string) =>
    control.evaluate((node: HTMLElement & { value: string }, selected) => {
      node.value = selected;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);

  await expect(page.locator('[data-component="permission-request-popup"]')).toBeVisible();
  await expect(card).toHaveAttribute('data-state', 'pending');
  await expect(card).toContainText('Wants permission to edit');
  await expect(card).toContainText('Auto-allow in');
  await presentation.evaluate(async (node) => {
    await customElements.whenDefined('wa-select');
    await (node as HTMLElement & { updateComplete?: Promise<unknown> }).updateComplete;
  });
  await presentation.evaluate((node) => {
    (window as typeof window & { __permissionPresentation?: Element }).__permissionPresentation = node;
  });
  await presentation.click();
  await expect(presentation).toHaveJSProperty('open', true);
  releaseDemoModified();
  const initialCountdown = await card.locator('.permission-request-card__timer').textContent();
  await expect.poll(() => card.locator('.permission-request-card__timer').textContent()).not.toBe(initialCountdown);
  await expect(presentation).toHaveJSProperty('open', true);
  expect(
    await presentation.evaluate(
      (node) => (window as typeof window & { __permissionPresentation?: Element }).__permissionPresentation === node,
    ),
  ).toBe(true);
  await page.keyboard.press('Escape');
  await expect(presentation).toHaveJSProperty('open', false);
  // Kerf 5.0.0-beta.51's catalog shows phase and recency as one status line (HS2-KMDJRH).
  await expect(
    page.locator('[data-action="catalog-select"][data-item-id="permission-request"] .kui-list-item__status'),
  ).toHaveText(/^Feature floor · \S+/);
  await choose(presentation, 'list');
  await expect(page.locator('[data-component="permission-request-popup"]')).toHaveCount(0);
  // The shell popup variant is a manual popover lifted into the top layer, anchored to the stage that
  // stands in for the main column (HS2-ZESCM2).
  await choose(presentation, 'top-layer');
  const topLayerPopup = page.locator('[data-component="permission-request-popup"][data-layer="top"]');
  await expect(topLayerPopup).toBeVisible();
  expect(await topLayerPopup.evaluate((element) => element.matches(':popover-open'))).toBe(true);
  const [stageBox, topLayerBox] = await Promise.all([
    page.locator('.permission-request-demo').boundingBox(),
    topLayerPopup.boundingBox(),
  ]);
  expect(Math.round(topLayerBox!.y - stageBox!.y)).toBe(56);
  expect(Math.round(stageBox!.x + stageBox!.width - (topLayerBox!.x + topLayerBox!.width))).toBe(16);
  await page.screenshot({ path: 'target/visual-captures/hs2-zescm2-permission-demo-top-layer.png', fullPage: true });
  // The in-flow variant is laid out by its host (the AI conversation's foreground), not anchored to a
  // corner, and stays interactive inside a pointer-transparent host (HS2-M2W2DP).
  await choose(presentation, 'flow');
  const flowPopup = page.locator('[data-component="permission-request-popup"][data-layer="flow"]');
  await expect(flowPopup).toBeVisible();
  await expect(flowPopup).toHaveCSS('position', 'relative');
  await expect(flowPopup).toHaveCSS('pointer-events', 'auto');
  const [flowColumn, flowBox] = await Promise.all([
    page.locator('.permission-request-demo__list').boundingBox(),
    flowPopup.boundingBox(),
  ]);
  expect(Math.round(flowBox!.x)).toBe(Math.round(flowColumn!.x));
  expect(Math.round(flowBox!.width)).toBe(Math.round(flowColumn!.width));
  await choose(presentation, 'popup');
  await expect(page.locator('[data-component="permission-request-popup"]')).toHaveAttribute('data-layer', 'inline');
  await expect(page.locator('[data-component="permission-request-popup"]')).toBeVisible();
  const stopAutomation = card.getByRole('button', { name: 'Stop auto-allow countdown' });
  await expect(stopAutomation).toHaveAttribute('title', 'Stop auto-allow countdown for this request');
  await expect(stopAutomation.locator('[data-lucide="pause"]')).toBeVisible();
  expect((await stopAutomation.textContent())?.trim()).toBe('');
  await expect(card.locator('.permission-request-card__countdown')).toHaveCSS('border-top-style', 'none');
  const [timerBox, denyBox] = await Promise.all([
    card.locator('.permission-request-card__timer').boundingBox(),
    card.getByRole('button', { name: 'Deny' }).boundingBox(),
  ]);
  expect(Math.abs(timerBox!.y + timerBox!.height / 2 - (denyBox!.y + denyBox!.height / 2))).toBeLessThanOrEqual(1);
  await page.screenshot({ path: 'target/visual-captures/hs2-xrva64-permission-countdown-wide.png', fullPage: true });
  await stopAutomation.click();
  await expect(card.locator('.permission-request-card__countdown')).toHaveCount(0);
  await expect(automation).toHaveJSProperty('value', 'none');
  await choose(automation, 'allow');
  await expect(card.getByRole('button', { name: 'Always Allow' })).toBeVisible();
  await expect(card.locator('.permission-request-card__explanation')).toBeVisible();

  await choose(presentation, 'list');
  await expect(page.locator('[data-component="permission-request-popup"]')).toHaveCount(0);
  await expect(card).toHaveClass(/permission-request-card--list/);
  await choose(variant, 'resolving');
  await expect(card).toHaveAttribute('data-state', 'resolving');
  await expect(card.getByRole('button', { name: 'Deny' })).toBeDisabled();
  await choose(variant, 'failed');
  await expect(card).toContainText('could not be delivered');
  await choose(variant, 'disconnected');
  await expect(card).toContainText('disconnected before this request was answered');
  await choose(variant, 'allowed');
  await expect(card).toHaveAttribute('data-state', 'allow');
  await expect(card).toContainText('allowed this kind of request');
  await expect(card.getByRole('button', { name: 'Deny' })).toHaveCount(0);
  await choose(request, 'tool-without-details');
  await expect(card.locator('.permission-request-card__details')).toHaveCount(0);
  await expect(card.locator('.permission-request-card__footer')).toHaveCount(0);
  await expect(card).toHaveCSS('padding-bottom', '16px');
  await choose(variant, 'denied');
  await expect(card).toHaveAttribute('data-state', 'deny');
  await choose(variant, 'external');
  await expect(card).toContainText('Decision made outside Hot Sheet');

  await choose(variant, 'pending');
  await choose(request, 'command');
  await expect(card).toContainText('Wants permission to run a command');
  await expect(card.locator('.permission-request-card__details')).toContainText('npm run test:unit');
  await choose(request, 'read');
  await expect(card).toContainText('Wants permission to read');
  await choose(request, 'tool-without-details');
  await expect(card).toContainText('Wants permission to use ToolSearch');
  await expect(card.locator('.permission-request-card__details')).toHaveCount(0);
  await choose(automation, 'deny');
  await expect(card).toContainText('Auto-deny in');
  await choose(automation, 'none');
  await expect(card.locator('.permission-request-card__countdown')).toHaveCount(0);
  await alwaysSupported.click();
  await expect(card.getByRole('button', { name: 'Always Allow' })).toHaveCount(0);
  await expect(card.getByRole('button', { name: 'Allow', exact: true })).toBeVisible();
  await explanation.click();
  await expect(card.locator('.permission-request-card__explanation')).toHaveCount(0);
  await page.screenshot({ path: 'target/visual-captures/hotsheet-permission-settings-wide.png', fullPage: true });

  await settings.getByRole('button', { name: 'Reset' }).click();
  await expect(presentation).toHaveJSProperty('value', 'popup');
  await expect(variant).toHaveJSProperty('value', 'pending');
  await expect(request).toHaveJSProperty('value', 'edit');
  await expect(automation).toHaveJSProperty('value', 'allow');
  await expect(alwaysSupported).toHaveJSProperty('checked', true);
  await expect(explanation).toHaveJSProperty('checked', true);
  await expect(page.locator('[data-component="permission-request-popup"]')).toBeVisible();
  await expect(card).toContainText('Auto-allow in');
  await page.setViewportSize({ width: 760, height: 900 });
  await page.screenshot({ path: 'target/visual-captures/hs2-xrva64-permission-countdown-narrow.png', fullPage: true });
  await choose(variant, 'external');
  await expect(card).toContainText('Decision made outside Hot Sheet');
  await expect(settings).toBeVisible();
  await expect(card).toBeVisible();
  await expect(page.getByRole('button', { name: 'Close settings' })).toBeVisible();
  const [cardBox, settingsBox] = await Promise.all([card.boundingBox(), settings.boundingBox()]);
  expect(cardBox!.x + cardBox!.width).toBeLessThanOrEqual(settingsBox!.x);
  await page.screenshot({ path: 'target/visual-captures/hotsheet-permission-settings-narrow.png', fullPage: true });
});

test('previews AIConversation public states at wide and narrow sizes', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=ai-conversation');
  const conversationHost = page.locator('[data-component="ai-conversation"]'),
    dialog = conversationHost.getByRole('dialog'),
    transcript = conversationHost.locator('.ai-conversation__transcript');
  await expect(dialog).toBeVisible();
  await expect(conversationHost).toContainText('Running the focused browser test');
  await expect(conversationHost.getByRole('button', { name: 'Stop Codex' })).toBeVisible();
  await expect(conversationHost.locator('.ai-conversation__message--user .markdown-preview').first()).toHaveCSS(
    'color',
    'rgb(255, 255, 255)',
  );
  const assistantMessage = transcript.locator('.ai-conversation__message--assistant').first();
  await expect(assistantMessage).toHaveAccessibleName('AI-generated response by Codex');
  await expect(assistantMessage.locator(':scope > strong')).toHaveCount(0);
  await expect(assistantMessage.locator('[data-component="ai-content-label"]')).toHaveCount(0);
  await expect(assistantMessage.getByRole('group', { name: 'Feedback on AI-generated by Codex' })).toBeVisible();
  await dialog.screenshot({ path: 'target/visual-captures/hs2-s3j29e-ai-conversation-demo-wide.png' });
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await page.locator('[data-action="toggle-settings"]').click();
  const scenario = page.locator('[data-settings="ai-conversation"] [name="scenario"]');
  for (const value of ['empty', 'permission', 'completed', 'usage-unpriced', 'failed', 'interrupted'] as const) {
    await scenario.evaluate((node: HTMLElement & { value: string }, next) => {
      node.value = next;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);
    await expect(dialog).toBeVisible();
    if (value === 'empty') await expect(conversationHost).toContainText('Start a conversation');
    if (value === 'permission')
      await expect(conversationHost.locator('[data-component="permission-request-card"]')).toBeVisible();
    if (value === 'completed') {
      await expect(conversationHost).toContainText('≈$0.04');
      await expect(conversationHost).toContainText('AI-generated · may contain errors');
    }
    if (value === 'usage-unpriced') await expect(conversationHost).toContainText('Cost unavailable');
    if (value === 'failed') {
      await expect(conversationHost).toContainText('Conversation unavailable');
      await expect(conversationHost).toContainText('The tool turn failed.');
      await transcript.evaluate((node) => {
        node.scrollTop = node.scrollHeight;
      });
      await expect(conversationHost.getByRole('alert')).toBeInViewport();
      await dialog.screenshot({ path: 'target/visual-captures/hs2-1kqjbk-ai-conversation-failed-wide.png' });
    }
    if (value === 'interrupted') await expect(conversationHost).toContainText('Stopped before the suite completed.');
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
  }
  await scenario.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'completed';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(dialog).toBeVisible();
  let activityCard = conversationHost.locator('.ai-conversation__activity');
  const label = activityCard.locator('[data-component="ai-content-label"]'),
    feedback = activityCard.locator('.ai-content-label__feedback').first();
  await expect(label).toHaveCount(1);
  await expect(feedback).toBeVisible();
  expect(
    await label.evaluate((node) => {
      const style = getComputedStyle(node),
        attribution = getComputedStyle(node.querySelector('.ai-content-label__attribution')!),
        glyph = getComputedStyle(node.querySelector('svg')!);
      return { gap: style.gap, attributionGap: attribution.gap, glyph: [glyph.width, glyph.height] };
    }),
  ).toEqual({ gap: '8px', attributionGap: '4px', glyph: ['12px', '12px'] });
  expect(
    await feedback.evaluate((node) => {
      const style = getComputedStyle(node),
        button = getComputedStyle(node.querySelector('button')!),
        glyph = getComputedStyle(node.querySelector('button svg')!);
      return { gap: style.gap, button: [button.width, button.height], glyph: [glyph.width, glyph.height] };
    }),
  ).toEqual({ gap: '4px', button: ['32px', '32px'], glyph: ['12px', '12px'] });
  await activityCard.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-ai-content-label-wide.png' });
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await page.setViewportSize({ width: 430, height: 760 });
  await scenario.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'completed';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(dialog).toBeVisible();
  activityCard = conversationHost.locator('.ai-conversation__activity');
  await expect(activityCard.locator('[data-component="ai-content-label"]')).toHaveCount(1);
  await expect(activityCard).toContainText('AI-generated · may contain errors');
  const command = activityCard.locator('code');
  await expect(command).toContainText('npm run test');
  expect((await command.evaluate((node) => getComputedStyle(node).fontFamily)).toLowerCase()).toContain('mono');
  const activitySize = await activityCard.evaluate((node) => ({
    clientWidth: node.clientWidth,
    scrollWidth: node.scrollWidth,
  }));
  expect(activitySize.scrollWidth).toBeLessThanOrEqual(activitySize.clientWidth + 1);
  await activityCard.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-ai-content-label-narrow.png' });
  await dialog.screenshot({ path: 'target/visual-captures/hs2-ai-activity-contained-narrow.png' });
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await page.setViewportSize({ width: 760, height: 640 });
  await scenario.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'failed';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(dialog).toBeVisible();
  await transcript.evaluate((node) => {
    node.scrollTop = node.scrollHeight;
  });
  await expect(conversationHost.getByRole('alert')).toBeInViewport();
  await dialog.screenshot({ path: 'target/visual-captures/hs2-1kqjbk-ai-conversation-failed-narrow.png' });
});

test('renders the real inspector chrome as a value-free loading placeholder', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 800 });
  await page.goto('/ux-demo?component=ticket-inspector-skeleton');
  const skeleton = page.locator('[data-component="ticket-inspector-skeleton"]');
  await expect(skeleton).toBeVisible();
  await expect(skeleton).toHaveAttribute('aria-busy', 'true');
  // It IS the inspector: same aside chrome, working collapse control, real icon-only tab bar.
  await expect(skeleton).toHaveClass(/\bticket-inspector--placeholder\b/);
  await expect(skeleton.getByRole('button', { name: 'Hide ticket inspector' })).toBeVisible();
  await expect(skeleton.locator('.ticket-inspector__tabs [data-component="app-tab"]')).toHaveCount(4);
  // Real controls/section headers are drawn; only the per-ticket values are placeholders.
  for (const label of ['Category', 'Priority', 'Status', 'Block ticket', 'Details', 'Tags', 'Notes', 'Activity']) {
    await expect(skeleton.getByText(label, { exact: true }).first()).toBeVisible();
  }
  // Category and Priority use native Select placeholders; Status uses a value-free Skeleton.
  await expect(skeleton.locator('.kui-select--placeholder')).toHaveCount(2);
  await expect(skeleton.locator('.kui-skeleton')).not.toHaveCount(0);
  const [category, priority] = await skeleton
    .locator('.ticket-info-panel__metadata > .kui-select')
    .evaluateAll((nodes) => nodes.map((node) => Math.round(node.getBoundingClientRect().y)));
  expect(category).toBe(priority); // Category and Priority render side by side, not stacked (HS2-KWWSWY).
  // No stale prior-ticket values; the known slug of the loading ticket is shown as chrome.
  await expect(skeleton.locator('.ticket-info-panel__details-surface')).not.toContainText(/\w/);
  await expect(skeleton).toContainText('HS2-4J50K3');
  await skeleton.screenshot({ path: 'target/visual-captures/hs2-reg3a2-ticket-inspector-skeleton.png' });
  // The chrome is non-interactive through `inert` on its own wrappers, not CSS reaching into Kerf (HS2-MGVE50).
  await expect(skeleton.locator('.ticket-inspector__header')).toHaveJSProperty('inert', true);
  const infoPlaceholder = skeleton.locator('[data-component="ticket-info-panel"][data-placeholder="true"]');
  await expect(infoPlaceholder).toHaveJSProperty('inert', true);
  // The body composes the TicketInfoPanel and TicketNotes placeholder variants (HS2-XBHADT).
  await expect(infoPlaceholder.locator('[data-component="ticket-notes"][data-placeholder="true"]')).toBeVisible();
  // The loading tabs are the loaded sidebar inspector's icon-only strip, so all four fit at every
  // width instead of clipping wide name skeletons at phone width (HS2-MYS1MR).
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    const tabs = skeleton.locator('.ticket-inspector__tabs [data-component="app-tab"]');
    await expect(tabs).toHaveCount(4);
    await expect(tabs.first()).toHaveAttribute('data-presentation', 'icon-only');
    const fit = await skeleton.evaluate((root) => {
      const bar = root.querySelector('.ticket-inspector__tabs')!.getBoundingClientRect();
      return [...root.querySelectorAll('.ticket-inspector__tabs [data-component="app-tab"]')].every((tab) => {
        const box = tab.getBoundingClientRect();
        return box.width > 0 && box.left >= bar.left - 0.5 && box.right <= bar.right + 0.5;
      });
    });
    expect(fit).toBe(true);
    await skeleton.screenshot({ path: `target/visual-captures/hs2-mys1mr-ticket-inspector-skeleton-${width}.png` });
  }
});

test('loads the shared touch textarea tokens so catalog textareas auto-grow under a coarse pointer (HS2-YBBJEN)', async ({
  browser,
}) => {
  for (const coarse of [true, false]) {
    const context = await browser.newContext({
        viewport: { width: coarse ? 390 : 1280, height: 844 },
        hasTouch: coarse,
        isMobile: coarse,
      }),
      page = await context.newPage();
    await page.goto('/ux-demo?component=note-composer');
    expect(await page.evaluate(() => matchMedia('(pointer: coarse)').matches)).toBe(coarse);
    const textarea = page.locator('[aria-label="NoteComposer demo"] textarea').first();
    await expect(textarea).toBeVisible();
    // Coarse pointers size to content with no resize grip, exactly as production does; fine pointers keep
    // the fixed height and vertical grip.
    await expect(textarea).toHaveCSS('resize', coarse ? 'none' : 'vertical');
    const sizing = await textarea.evaluate((node) => getComputedStyle(node).getPropertyValue('field-sizing'));
    expect(sizing).toBe(coarse ? 'content' : 'fixed');
    if (coarse) {
      const before = (await textarea.boundingBox())!.height;
      await textarea.fill(Array.from({ length: 8 }, (_, index) => `Line ${index + 1}`).join('\n'));
      await expect.poll(async () => (await textarea.boundingBox())!.height).toBeGreaterThan(before);
      await textarea.screenshot({ path: 'target/visual-captures/hs2-ybbjen-note-composer-coarse-390.png' });
    } else {
      await textarea.screenshot({ path: 'target/visual-captures/hs2-ybbjen-note-composer-fine-1280.png' });
    }
    await context.close();
  }
});

test('exposes the TicketInfoPanel placeholder variant beside the loaded panel (HS2-XBHADT)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=ticket-info-panel');
    const loaded = page.getByRole('region', { name: 'TicketInfoPanel demo', exact: true });
    const placeholder = page.getByRole('region', { name: 'TicketInfoPanel placeholder demo' });
    await expect(loaded.locator('[data-component="ticket-info-panel"]')).not.toHaveAttribute('data-placeholder');
    await expect(loaded).toContainText('Hot Sheet git');
    const panel = placeholder.locator('[data-component="ticket-info-panel"][data-placeholder="true"]');
    await expect(panel).toHaveJSProperty('inert', true);
    await expect(panel.locator('.kui-select--placeholder')).toHaveCount(2);
    await expect(panel.locator('[data-component="ticket-notes"][data-placeholder="true"]')).toHaveCount(1);
    await expect(panel).not.toContainText('Hot Sheet git');
    const box = await panel.boundingBox();
    expect(box!.width).toBeLessThanOrEqual(width);
  }
});

test('lets inspector Markdown keep its own typography in sidebar and reader (HS2-MGVE50)', async ({ page }) => {
  for (const [component, root] of [
    ['ticket-inspector', '[data-component="ticket-inspector"]'],
    ['ticket-reader', '[data-component="ticket-reader"]'],
  ] as const) {
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(`/ux-demo?component=${component}&dev-review=false`);
      const details = page.locator(`${root} .ticket-info-panel__details-surface .markdown-preview`).first();
      // A Markdown heading is a Markdown heading, not an uppercase inspector section label.
      await expect(details.locator('h2').first()).toHaveCSS('text-transform', 'none');
      const [paragraph, quote] = await Promise.all([
        details
          .locator('p')
          .first()
          .evaluate((node) => getComputedStyle(node).lineHeight),
        details
          .locator('blockquote p')
          .first()
          .evaluate((node) => getComputedStyle(node).fontSize),
      ]);
      expect(paragraph).toBe('22.4px');
      expect(quote).toBe('12px');
      // The flush editor puts Details text on the surface's own 8px inset.
      await expect(
        page.locator(`${root} .ticket-info-panel__details-surface .markdown-editor__preview`).first(),
      ).toHaveCSS('padding-left', '0px');
    }
  }
});

test('sizes inspector editors and titles through child tokens and own classes (HS2-DYAR0S)', async ({ browser }) => {
  const heights = {
    '--hs-details-sidebar-height': '131px',
    '--hs-details-reader-height': '221px',
    '--hs-blocked-reason-sidebar-height': '77px',
    '--hs-blocked-reason-reader-height': '143px',
    '--hs-note-sidebar-height': '99px',
    '--hs-note-reader-height': '187px',
  };
  const editorHeights = async (root: import('@playwright/test').Locator) => {
    await root.getByRole('button', { name: 'Edit Ticket details' }).getByRole('heading').first().click();
    const details = root.getByRole('textbox', { name: 'Ticket details' });
    const detailsHeight = await details.evaluate((node) => getComputedStyle(node).height);
    await details.blur();
    const note = root.locator('[data-component="note-card"] .note-card__body[aria-label="Edit note"]').first();
    await note.click({ position: { x: 4, y: 4 } });
    const noteHeight = await root
      .getByRole('textbox', { name: 'Note body' })
      .first()
      .evaluate((node) => getComputedStyle(node).height);
    await root.getByRole('button', { name: 'Block ticket' }).click();
    const blockedHeight = await root
      .getByRole('textbox', { name: 'Blocked reason' })
      .evaluate((node) => getComputedStyle(node).height);
    return { details: detailsHeight, note: noteHeight, blocked: blockedHeight };
  };
  for (const coarse of [false, true]) {
    for (const [component, presentation] of [
      ['ticket-inspector', 'sidebar'],
      ['ticket-reader', 'reader'],
    ] as const) {
      // Touch is emulated the way the mobile layout specs do: a phone context reports a coarse pointer.
      const context = await browser.newContext({
          viewport: coarse ? { width: 390, height: 844 } : { width: 1280, height: 900 },
          hasTouch: coarse,
          isMobile: coarse,
        }),
        page = await context.newPage();
      await page.goto(`/ux-demo?component=${component}&dev-review=false`);
      expect(await page.evaluate(() => matchMedia('(pointer: coarse)').matches)).toBe(coarse);
      const root = page.locator(`[data-component="${component}"]`).first();
      await expect(root).toBeVisible();
      await page.evaluate((values) => {
        for (const [name, value] of Object.entries(values)) document.documentElement.style.setProperty(name, value);
      }, heights);
      // The title is the inspector's own element: its class carries the presentation variant.
      const title = root.locator('.ticket-inspector__title');
      await expect(title).toHaveCSS('font-size', presentation === 'reader' ? '20px' : '16px');
      await expect(title).toHaveCSS('cursor', 'text');
      // Stored desktop heights reach each child through its public height token; touch screens drop them.
      // On touch the demo now loads the shared textarea tokens (HS2-YBBJEN), so the multi-paragraph
      // details editor grows to its content instead of a stored height, while the short note and empty
      // blocked-reason editors sit at their component minimums.
      const measured = await editorHeights(root);
      if (coarse) {
        expect({ note: measured.note, blocked: measured.blocked }).toEqual({ note: '80px', blocked: '64px' });
        expect(parseFloat(measured.details)).toBeGreaterThan(
          parseFloat(heights[`--hs-details-${presentation}-height`]),
        );
      } else
        expect(measured).toEqual(
          presentation === 'reader'
            ? { details: '221px', note: '187px', blocked: '143px' }
            : { details: '131px', note: '99px', blocked: '77px' },
        );
      await context.close();
    }
  }
});

test('shows the chat model/effort as a label with a popup to change them', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 800 });
  await page.goto('/ux-demo?component=ai-conversation');
  const control = page.locator('[data-component="conversation-model-control"]').first();
  await expect(control).toBeVisible();
  await expect(control.locator('.ai-conversation__model-name')).toContainText('GPT-5.6');
  await expect(control.locator('.ai-conversation__model-effort')).toContainText('high');
  await control.locator('[data-conversation-model-menu] [slot="trigger"]').click();
  await expect(page.getByRole('menuitem', { name: /Provider/ })).toBeVisible();
  await expect(page.getByRole('menuitem', { name: /Model/ })).toBeVisible();
  await expect(page.getByRole('menuitem', { name: /Effort/ })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-r5f7a5-chat-model-popup.png' });
  // Selecting a different provider from the submenu switches the chat (HS2-PRBGRB). The nested
  // Web Awesome submenu reveal is the library's; our contract is the data-action item + handler, so
  // fire a real bubbling click on it rather than depending on hover-to-expand timing.
  await page.locator('[data-action="select-conversation-provider"][data-value="claude"]').dispatchEvent('click');
  await expect(page.locator('[data-component="ai-conversation"]')).toContainText('Claude conversation');
});

test('exposes a General app-settings entry in the settings navigator', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 800 });
  await page.goto('/ux-demo?component=settings-navigation');
  const general = page.locator('[data-item-id="general"]');
  await expect(general).toBeVisible();
  await expect(general).toContainText('General');
  await expect(page.getByText('App Settings', { exact: true })).toBeVisible();
});

test('edits custom command color and icon in the command settings editor', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 900 });
  await page.goto('/ux-demo?component=command-settings-editor');
  const editor = page.locator('[data-component="command-settings-editor"]');
  await editor.scrollIntoViewIfNeeded();
  await expect(editor).toBeVisible();
  // The WYSIWYG list groups draggable rows; each row exposes an overflow menu (HS2-D9JBXT).
  await expect(editor.locator('.command-settings-editor__row')).toHaveCount(3);
  await expect(editor.locator('.command-settings-editor__row[draggable="true"]')).toHaveCount(3);
  await expect(editor.locator('.command-settings-editor__group-label')).toHaveText(['Quality', 'Release']);
  await page.screenshot({ path: 'target/visual-captures/hs2-656xj2-command-list.png' });
  // Add command is the brand-filled primary action; Add group stays a plain secondary button (HS2-JSSMFY).
  const buttonFill = (name: string) =>
    editor.getByRole('button', { name, exact: true }).evaluate((node) => {
      const probe = document.createElement('span');
      probe.style.background = 'var(--wa-color-brand-fill-loud)';
      node.append(probe);
      const brand = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return { background: getComputedStyle(node).backgroundColor, brand };
    });
  const addCommandFill = await buttonFill('Add command'),
    addGroupFill = await buttonFill('Add group');
  expect(addCommandFill.background).toBe(addCommandFill.brand);
  expect(addGroupFill.background).not.toBe(addGroupFill.brand);
  // Details, including the color and icon pickers, live in the Edit command dialog opened from the row menu.
  const verifyRow = editor.locator('.command-settings-editor__row', { hasText: 'Verify project' });
  await verifyRow.locator('.command-settings-editor__row-menu [slot="trigger"]').click();
  await verifyRow.locator('[data-action="edit-command-setting"]').dispatchEvent('click');
  const dialog = page.locator('#command-editor-dialog');
  await expect(dialog).toBeVisible();
  const commandHeadingGroup = dialog.locator('.app-heading [data-component="toolbar-control-group"]').first();
  await expect(commandHeadingGroup).not.toHaveClass(/app-heading__icon/);
  await expect(commandHeadingGroup).toHaveAttribute('data-appearance', 'borderless');
  await expect(commandHeadingGroup.locator('.command-settings-editor__dialog-icon > svg')).toBeVisible();
  const iconInset = () =>
    commandHeadingGroup.evaluate((group) => {
      const outer = group.getBoundingClientRect(),
        inner = group.querySelector('.command-settings-editor__dialog-icon')!.getBoundingClientRect();
      return { left: inner.left - outer.left, right: outer.right - inner.right };
    });
  const wideIconInset = await iconInset();
  expect(Math.abs(wideIconInset.left - wideIconInset.right)).toBeLessThan(1);
  await expect(dialog.locator('.command-settings-editor__swatch')).toHaveCount(9);
  // The icon field is the reusable searchable Lucide picker (HS2-5VSNV3), not a fixed radio grid.
  await expect(dialog.locator('[data-component="lucide-icon-picker"]')).toBeVisible();
  // The editor styles only its own buttons, so the nested picker keeps its 18px grid icons (HS2-JSSMFY).
  const pickerIcon = dialog.locator('[data-action="select-command-icon"]').first();
  await expect(pickerIcon).toHaveCSS('display', 'grid');
  await expect(pickerIcon.locator('svg')).toHaveCSS('width', '18px');
  await dialog.getByTitle('Blue', { exact: true }).click();
  await dialog.locator('[data-action="select-command-icon"][data-icon-name="wand"]').click();
  await expect(dialog.locator('.command-settings-editor__swatch input:checked')).toHaveValue('#3b82f6');
  await expect(dialog.locator('[data-action="select-command-icon"][data-icon-name="wand"]')).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  const done = page.getByRole('button', { name: 'Done', exact: true });
  await expect(done).toHaveCSS('border-top-color', 'rgba(0, 0, 0, 0)');
  await expect(done).toHaveCSS('height', '40px');
  await page.screenshot({ path: 'target/visual-captures/hs2-656xj2-command-editor-color-icon.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(commandHeadingGroup.locator('.command-settings-editor__dialog-icon > svg')).toBeVisible();
  const phoneIconInset = await iconInset();
  expect(Math.abs(phoneIconInset.left - phoneIconInset.right)).toBeLessThan(1);
  await page.screenshot({ path: 'target/visual-captures/hs2-rxxxph-command-editor-phone.png' });
  await dialog.getByRole('button', { name: 'Done' }).click();
  await expect.poll(() => dialog.evaluate((node) => node.matches(':popover-open'))).toBe(false);
  await expect(dialog).toBeEmpty();
});

test('keeps the Markdown preview focus ring inset so an overflow-hidden editor cannot clip it (HS2-0WD3YK)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1000, height: 800 });
  await page.goto('/ux-demo?component=markdown-editor&dev-review=false');
  const preview = page.locator('.markdown-editor__preview');
  await expect(preview).toBeVisible();
  // Keyboard focus so :focus-visible applies, then verify the ring is inset and within the editor's clip box.
  await preview.evaluate((node) => {
    node.focus();
  });
  const geometry = await preview.evaluate((node) => {
    const cs = getComputedStyle(node),
      editor = node.closest('.markdown-editor')!,
      er = editor.getBoundingClientRect(),
      r = node.getBoundingClientRect(),
      width = parseFloat(cs.outlineWidth) || 0,
      offset = parseFloat(cs.outlineOffset);
    return {
      focusVisible: node.matches(':focus-visible'),
      offset,
      width,
      within:
        r.left - (offset + width) >= er.left - 0.5 &&
        r.top - (offset + width) >= er.top - 0.5 &&
        r.right + (offset + width) <= er.right + 0.5 &&
        r.bottom + (offset + width) <= er.bottom + 0.5,
    };
  });
  expect(geometry.focusVisible).toBe(true);
  expect(geometry.offset).toBeLessThan(0);
  expect(geometry.within).toBe(true);
  await page.locator('.markdown-editor').screenshot({
    path: 'target/visual-captures/claude-501/-Users-westphal-Documents-hotsheet2/88cd2d15-f2a9-4f29-8bb4-c672b5069c22/scratchpad/0WD3YK-markdown-preview-focus-inset.png',
  });
});

test('previews a running terminal through the shared TerminalPreview in the ProjectCloseDialog demo (HS2-148B5C)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=project-close-dialog&dev-review=false');
  const dialog = page.locator('[data-component="project-close-dialog"]');
  await expect(dialog).toHaveJSProperty('open', true);
  await dialog.getByRole('button', { name: /Tests/ }).click();
  await expect(dialog.getByRole('button', { name: /Tests/ })).toHaveAttribute('aria-current', 'page');
  const region = dialog.getByRole('region', { name: 'Tests terminal preview' }),
    preview = region.locator('[data-component="terminal-preview"]'),
    viewport = preview.locator('[data-component="terminal-viewport"]'),
    fallback = preview.getByText('Connecting to the live terminal…');
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  await expect(fallback).toBeHidden();
  const geometry = () =>
    region.evaluate((section) => {
      const box = section.getBoundingClientRect(),
        frame = section.querySelector<HTMLElement>('[data-component="terminal-viewport"]')!.parentElement!,
        frameBox = frame.getBoundingClientRect();
      return {
        inset: Math.round(frameBox.left - box.left),
        bottom: Math.round(box.bottom - frameBox.bottom),
        radius: getComputedStyle(frame).borderTopLeftRadius,
        overflow: getComputedStyle(frame).overflow,
        canvas: getComputedStyle(frame.firstElementChild!).width,
      };
    });
  // The dialog tunes the preview's public inset token; the preview owns the frame and 1280px canvas.
  expect(await geometry()).toEqual({ inset: 24, bottom: 24, radius: '6px', overflow: 'hidden', canvas: '1280px' });
  // Until the live terminal attaches, the preview shows its own connecting fallback.
  await viewport.evaluate((node) => {
    node.removeAttribute('data-connection');
  });
  await expect(fallback).toBeVisible();
  await viewport.evaluate((node) => {
    node.setAttribute('data-connection', 'reconnecting');
  });
  await expect(fallback).toBeHidden();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect.poll(async () => (await geometry()).inset).toBe(16);
  // Selecting the chat again swaps the preview back to the embedded conversation.
  await dialog.getByRole('button', { name: /Codex/ }).click();
  await expect(dialog.locator('[data-component="ai-conversation"]')).toBeVisible();
  await expect(preview).toHaveCount(0);
});

test('scales the ProjectCloseDialog TerminalPreview canvas to fit its frame at wide and phone widths (HS2-S7E53Q)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=project-close-dialog&dev-review=false');
  const dialog = page.locator('[data-component="project-close-dialog"]');
  await expect(dialog).toHaveJSProperty('open', true);
  await dialog.getByRole('button', { name: /Tests/ }).click();
  const region = dialog.getByRole('region', { name: 'Tests terminal preview' }),
    viewport = region.locator('[data-component="terminal-viewport"]');
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  const fit = () =>
    viewport.evaluate((node) => {
      const frame = node.parentElement!.getBoundingClientRect(),
        rows = [...node.querySelectorAll<HTMLElement>('.xterm-rows > div')].filter((row) => row.textContent.trim()),
        outside = rows
          .map((row) => row.getBoundingClientRect())
          .filter(
            (box) =>
              box.left < frame.left - 0.5 ||
              box.top < frame.top - 0.5 ||
              box.right > frame.right + 0.5 ||
              box.bottom > frame.bottom + 0.5,
          ).length,
        screen = node.querySelector('.xterm-screen')!.getBoundingClientRect();
      return {
        scale: Number(node.dataset.previewScale),
        transform: getComputedStyle(node).transform,
        rows: rows.length,
        outside,
        screenInside: screen.right <= frame.right + 0.5 && screen.bottom <= frame.bottom + 0.5,
      };
    });
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
    await expect
      .poll(async () => {
        const result = await fit();
        return result.rows > 0 && result.outside === 0 && result.screenInside;
      }, `every preview line fits inside the frame at ${width}px`)
      .toBe(true);
    const result = await fit();
    // The canvas takes the dashboard tile's preview scale: shrunk to the frame, never enlarged.
    expect(result.scale).toBeGreaterThan(0);
    expect(result.scale).toBeLessThanOrEqual(1);
    expect(result.transform).not.toBe('none');
    await region.screenshot({
      path: test.info().outputPath(`s7e53q-close-preview-${width}.png`),
    });
  }
});

test('renders the ProjectCloseDialog TerminalPreview at a legible glyph size at wide and phone widths (HS2-XHBDRV)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=project-close-dialog&dev-review=false');
  const dialog = page.locator('[data-component="project-close-dialog"]');
  await expect(dialog).toHaveJSProperty('open', true);
  await dialog.getByRole('button', { name: /Tests/ }).click();
  const region = dialog.getByRole('region', { name: 'Tests terminal preview' }),
    viewport = region.locator('[data-component="terminal-viewport"]');
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  // Wait on deterministic render signals before measuring (HS2-5114VH): under full-suite load xterm can
  // connect before it writes its first text row or before the scaled-preview transform is applied.
  await expect(viewport).toHaveAttribute('data-preview-scale', /^\d/);
  await expect(viewport.locator('.xterm-rows > div').filter({ hasText: /\S/ }).first()).toBeVisible();
  const glyphs = () =>
    viewport.evaluate((node) => {
      const frame = node.parentElement!.getBoundingClientRect(),
        screenNode = node.querySelector('.xterm-screen'),
        row = [...node.querySelectorAll<HTMLElement>('.xterm-rows > div')].find((item) => item.textContent.trim());
      // Not rendered yet (for example while a resize re-renders the grid): report a not-ready result the
      // poll below retries instead of throwing out of the evaluate.
      if (!screenNode || !row) return { gridSize: node.dataset.gridSize, rowHeight: 0, fill: 0, inside: false };
      const screen = screenNode.getBoundingClientRect();
      return {
        gridSize: node.dataset.gridSize,
        // The rendered (post-transform) height of one terminal row: the on-screen glyph size.
        rowHeight: row.getBoundingClientRect().height,
        // How much of the frame the scaled grid fills along its bound axis.
        fill: Math.max(screen.width / frame.width, screen.height / frame.height),
        inside: screen.right <= frame.right + 0.5 && screen.bottom <= frame.bottom + 0.5,
      };
    });
  // A phone-width frame is about a quarter of the 1280px canvas; the 80x24 grid at the dashboard font
  // must still render rows of about 7px instead of the 177-column fit's 3px glyphs.
  for (const [width, minimumRow] of [
    [1280, 12],
    [390, 6],
  ] as const) {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
    await expect
      .poll(async () => {
        const result = await glyphs();
        return result.inside && result.rowHeight >= minimumRow;
      }, `the preview renders legible rows inside the frame at ${width}px`)
      .toBe(true);
    const result = await glyphs();
    expect(result.gridSize).toBe('80x24');
    // The grid fills the frame along its bound axis instead of a corner of the canvas.
    expect(result.fill).toBeGreaterThan(0.95);
    if (width === 390) {
      // The phone frame takes a canvas-like aspect, so the grid is width-bound and its rows grow from
      // about 6.7px to about 8px; the consequences and actions still fit without scrolling (HS2-28EVHV).
      const phone = await viewport.evaluate((node) => {
        const frame = node.parentElement!.getBoundingClientRect(),
          screen = node.querySelector('.xterm-screen')!.getBoundingClientRect(),
          host = node.closest('[data-component="project-close-dialog"]')!,
          body = host.shadowRoot!.querySelector<HTMLElement>('[part~="body"]')!;
        return {
          aspect: frame.width / frame.height,
          gridAspect: Number(node.closest<HTMLElement>('[data-component="terminal-preview"]')!.dataset.gridAspect),
          widthFill: screen.width / frame.width,
          heightFill: screen.height / frame.height,
          bodyOverflow: body.scrollHeight - body.clientHeight,
          panelBottom: host.shadowRoot!.querySelector('[part~="dialog"]')!.getBoundingClientRect().bottom,
        };
      });
      // The frame then takes the mirrored grid's own published aspect, so the grid fills it on both
      // axes instead of staying slightly height-bound inside 5:3 (HS2-RBS46R).
      expect(phone.gridAspect).toBeGreaterThan(1.5);
      expect(phone.gridAspect).toBeLessThan(5 / 3);
      expect(phone.aspect).toBeCloseTo(phone.gridAspect, 2);
      expect(phone.widthFill).toBeGreaterThan(0.98);
      expect(phone.heightFill).toBeGreaterThan(0.98);
      expect(result.rowHeight).toBeGreaterThan(7.5);
      expect(phone.bodyOverflow).toBeLessThanOrEqual(1);
      expect(phone.panelBottom).toBeLessThanOrEqual(844);
      await expect(dialog.locator('[data-project-close-consequences]')).toBeInViewport({ ratio: 1 });
      await expect(dialog.getByRole('button', { name: 'Stop & Close' })).toBeInViewport({ ratio: 1 });
    }
    await region.screenshot({ path: test.info().outputPath(`xhbdrv-close-preview-${width}.png`) });
  }
});

test('renders the ProjectCloseDialog and ConversationExportDialog demos (HS2-QKKS05)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=project-close-dialog&dev-review=false');
  const projectClose = page.locator('[data-component="project-close-dialog"]');
  await expect(projectClose).toHaveJSProperty('open', true);
  await expect(projectClose).toHaveAttribute('data-has-resources', 'true');
  await expect(projectClose.locator('.project-close-dialog__resources > [data-component="list"]')).toHaveCSS(
    'overflow',
    'auto',
  );
  // The selected AI-chat resource renders its embedded conversation preview.
  await expect(projectClose.locator('[data-component="ai-conversation"]')).toBeVisible();
  // The embedded presentation fills the chat preview by itself; the dialog does not size it (HS2-29Q3XG).
  const chatFill = await projectClose.locator('.project-close-dialog__chat').evaluate((node) => ({
    section: node.getBoundingClientRect().height,
    chat: node.querySelector('[data-component="ai-conversation"]')!.getBoundingClientRect().height,
  }));
  expect(chatFill.section).toBeGreaterThan(0);
  expect(chatFill.chat).toBeCloseTo(chatFill.section, 0);
  await expect(projectClose).toContainText('Kerf');
  await projectClose.evaluate((node) =>
    Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished)),
  );
  await expect(projectClose.locator('[data-component="state-banner"]')).toHaveAttribute('data-tone', 'warning');
  await expect(projectClose.locator('[slot="footer"][data-component="row"]')).toHaveAttribute('data-wrap', 'true');
  await page.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-project-close-wide.png', fullPage: true });
  await page.screenshot({ path: 'target/visual-captures/hs2-qkks05-project-close-dialog.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect
    .poll(() =>
      projectClose.evaluate((node) => {
        const surface = node.shadowRoot?.querySelector('dialog')?.getBoundingClientRect();
        return (
          surface != null &&
          surface.left >= 0 &&
          surface.top >= 0 &&
          surface.right <= innerWidth &&
          surface.bottom <= innerHeight
        );
      }),
    )
    .toBe(true);
  await page.screenshot({ path: 'target/visual-captures/hs2-4y6sm9-project-close-narrow.png', fullPage: true });
  for (const [width, gutter] of [
    [672, 8],
    [673, 16],
  ]) {
    await page.setViewportSize({ width, height: 844 });
    await expect
      .poll(() =>
        projectClose.evaluate((node) => node.shadowRoot!.querySelector('dialog')!.getBoundingClientRect().left),
      )
      .toBeCloseTo(gutter, 0);
  }
  await page.setViewportSize({ width: 1280, height: 900 });

  await page.goto('/ux-demo?component=conversation-export-dialog');
  const exportDialog = page.locator('[data-component="conversation-export-dialog"]');
  await expect(exportDialog).toHaveJSProperty('open', true);
  // Fixture opens on step 2 (destination + format + bundle options).
  await expect(exportDialog).toHaveAttribute('data-step', '2');
  await expect(exportDialog).toContainText('Bundle contents');
  await exportDialog.evaluate((node) =>
    Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished)),
  );
  await page.screenshot({ path: 'target/visual-captures/hs2-qkks05-conversation-export-dialog.png', fullPage: true });
});

test('renders the CommandRunDialog demo as an opened native modal (HS2-Z0CTHN)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=command-run-dialog');
  const dialog = page.locator('[data-component="command-run-dialog"]');
  // Native <dialog> opened via showModal wiring (not an inline wa-dialog).
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog).toContainText('Run checks');
  await expect(dialog).toContainText('completed');
  await expect(dialog).toContainText('Exit 0');
  await expect(dialog.locator('[aria-label="Command output"]')).toContainText('All checks passed in 4.2s');
  // stderr lines carry the 'error: ' prefix in the output presentation.
  await expect(dialog.locator('[aria-label="Command output"]')).toContainText('error: note: 2 files skipped');
  await expect(dialog.getByRole('button', { name: 'Close' })).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/claude/hs2-z0cthn-command-run-dialog.png', fullPage: true });
});

test('swaps the CommandRunDialog demo between run output and stop confirmation (HS2-CWWX7S)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=command-run-dialog&dev-review=false');
    const output = page.locator('[data-component="command-run-dialog"]'),
      stop = page.locator('[data-component="command-cancellation-dialog"]'),
      event = page.locator('.component-stage__event'),
      presentation = page.locator('[data-settings="command-run-dialog"] [name="presentation"]');
    await expect(output).toHaveJSProperty('open', true);
    await expect(stop).toHaveCount(0);

    // Close is wired to the demo's dismiss stand-in; the modal must close before the settings are reachable.
    await output.getByRole('button', { name: 'Close' }).click();
    await expect(output).toHaveJSProperty('open', false);
    await expect(event).toHaveText('Closed Run checks output');
    await page.locator('[data-action="toggle-settings"]').click();
    await expect(presentation).toHaveJSProperty('value', 'output');

    // Swapping the presentation re-opens the native dialog modally with the stop confirmation.
    await presentation.evaluate((control: HTMLElement & { value: string }) => {
      control.value = 'stop';
      control.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await expect(stop).toHaveJSProperty('open', true);
    await expect(stop.evaluate((dialog) => dialog.matches(':modal'))).resolves.toBe(true);
    await expect(output).toHaveCount(0);
    await expect(stop.getByRole('heading', { name: 'Stop Run checks?' })).toBeVisible();
    await expect(stop.getByRole('button', { name: 'Keep running' })).toBeVisible();
    await expect(stop.getByRole('button', { name: 'Stop command' })).toHaveAttribute('data-run-id', 'run-43');
    const box = await stop.boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(width);
    await page.screenshot({ path: `target/visual-captures/claude/hs2-cwwx7s-stop-confirmation-${width}.png` });

    await stop.getByRole('button', { name: 'Keep running' }).click();
    await expect(stop).toHaveJSProperty('open', false);
    await expect(event).toHaveText('Kept Run checks running');
    await page.locator('[data-action="open-command-run-dialog-demo"]').click();
    await expect(stop).toHaveJSProperty('open', true);
    await stop.getByRole('button', { name: 'Stop command' }).click();
    await expect(stop).toHaveJSProperty('open', false);
    await expect(event).toHaveText('Stop requested for run-43');
    await expect(presentation).toHaveJSProperty('value', 'stop');

    // Reset restores the run-output presentation in both the dialog and the live control, then edits again.
    await page.locator('[data-settings="command-run-dialog"] [data-action="reset-settings"]').click();
    await expect(output).toHaveJSProperty('open', true);
    await expect(output).toContainText('All checks passed in 4.2s');
    await expect(presentation).toHaveJSProperty('value', 'output');
    await page.keyboard.press('Escape');
    await expect(output).toHaveJSProperty('open', false);
    await presentation.evaluate((control: HTMLElement & { value: string }) => {
      control.value = 'stop';
      control.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await expect(stop).toHaveJSProperty('open', true);
  }
});

for (const theme of ['light', 'dark'] as const) {
  test(`uses native AI toolbar actions in dialog and embedded views in ${theme} (HS2-WXVAF3)`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.addInitScript((value) => {
      localStorage.setItem('hotsheet.ux-demo.theme', value);
    }, theme);
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto('/ux-demo?component=ai-conversation&dev-review=false');
    const host = page.locator('[data-component="ai-conversation"]'),
      save = host.getByRole('button', { name: 'Save conversation' }),
      stop = host.getByRole('button', { name: 'Stop Codex' });
    await expect(host.getByRole('dialog')).toBeVisible();
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    await expect(save).toHaveJSProperty('tagName', 'BUTTON');
    await expect(save).toBeDisabled();
    await expect(stop).toHaveJSProperty('tagName', 'BUTTON');
    await stop.focus();
    await expect(stop).toBeFocused();
    await expect(stop).toHaveCSS('outline-style', 'solid');
    await host.getByRole('dialog').screenshot({ path: `target/visual-captures/hs2-wxvaf3-dialog-${theme}-wide.png` });
    await page.setViewportSize({ width: 390, height: 850 });
    await expect(host.locator('.ai-conversation__header-usage')).toBeHidden();
    await host.getByRole('dialog').screenshot({ path: `target/visual-captures/hs2-wxvaf3-dialog-${theme}-narrow.png` });
    await page.setViewportSize({ width: 1280, height: 900 });
    await stop.press('Space');
    await expect(stop).toHaveCount(0);
    await expect(host).toContainText('Stopped before the suite completed.');
    await expect(save).toBeEnabled();
    await save.focus();
    await save.press('Enter');
    await page.keyboard.press('Escape');
    await expect(page.locator('.ai-conversation-demo > output')).toContainText('Prepared 1 conversation export.');
    await page.locator('[data-action="toggle-settings"]').click();
    const settings = page.locator('[data-settings="ai-conversation"]');
    await settings.locator('[name="presentation"]').evaluate((node: HTMLElement & { value: string }) => {
      node.value = 'embedded';
      node.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await expect(host).toHaveAttribute('data-presentation', 'embedded');
    await settings.locator('[name="scenario"]').evaluate((node: HTMLElement & { value: string }) => {
      node.value = 'streaming';
      node.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await page.getByRole('button', { name: 'Close settings' }).click();
    await host.screenshot({ path: `target/visual-captures/hs2-wxvaf3-embedded-${theme}-wide.png` });
    await page.setViewportSize({ width: 390, height: 850 });
    await expect(save).toBeDisabled();
    await expect(stop).toBeVisible();
    await expect(stop).toHaveJSProperty('tagName', 'BUTTON');
    await expect
      .poll(async () => {
        const box = await stop.boundingBox();
        return box ? box.x >= 0 && box.x + box.width <= 390 : false;
      })
      .toBe(true);
    await host.screenshot({ path: `target/visual-captures/hs2-wxvaf3-embedded-${theme}-narrow.png` });
    await stop.click();
    await expect(save).toBeEnabled();
    await save.click();
    await expect(page.locator('.ai-conversation-demo > output')).toContainText('Prepared 2 conversation exports.');
  });
}

test('keeps equal notification card gaps across pending and history groups (HS2-D38KZF)', async ({ page }) => {
  await page.goto('/ux-demo?component=notification-center&dev-review=false');
  const center = page.locator('[data-component="notification-center"]'),
    cards = center.locator('[data-component="permission-request-card"]');
  for (const width of [1280, 560]) {
    await page.setViewportSize({ width, height: 1200 });
    await expect(cards).toHaveCount(3);
    const geometry = await cards.evaluateAll((nodes) =>
      nodes.map((node) => {
        const rect = node.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom };
      }),
    );
    expect(geometry[1].top - geometry[0].bottom).toBeCloseTo(12, 1);
    expect(geometry[2].top - geometry[1].bottom).toBeCloseTo(12, 1);
    await expect(center.locator(':scope > [data-component="list"]')).toHaveCSS('gap', '12px');
    await center.screenshot({
      path: `target/visual-captures/hs2-d38kzf-notification-spacing-${width}.png`,
      animations: 'disabled',
    });
  }
});

test('catalogs the TerminalKeyBar rows, sticky modifiers, and sent bytes (HS2-CKS78M)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=terminal-key-bar');
  const demo = page.getByRole('region', { name: 'Terminal key bar variants' }),
    bars = demo.locator('[data-component="terminal-key-bar"]'),
    keys = bars.nth(0),
    output = demo.locator('[data-key-bar-demo-output]');
  await expect(bars).toHaveCount(2);
  await expect(keys).toHaveAttribute('data-function-row', 'false');
  await expect(bars.nth(1)).toHaveAttribute('data-function-row', 'true');
  await expect(bars.nth(1).getByRole('button', { name: 'Alt (locked)' })).toHaveAttribute('aria-pressed', 'true');
  for (const name of ['Escape', 'Tab', 'Ctrl', 'Alt', 'Up arrow', 'Function and navigation keys'])
    await expect(keys.getByRole('button', { name, exact: true })).toBeVisible();
  await expect(keys.getByRole('button', { name: 'Shift' })).toHaveCount(0);
  const ctrl = keys.getByRole('button', { name: /^Ctrl/ });
  await ctrl.click();
  await expect(ctrl).toHaveAttribute('data-state', 'once');
  await keys.getByRole('button', { name: 'Right arrow' }).click();
  await expect(output).toHaveText('ArrowRight → \\u001b[1;5C');
  await expect(ctrl).toHaveAttribute('data-state', 'off');
  await keys.getByRole('button', { name: 'Function and navigation keys' }).click();
  await expect(keys).toHaveAttribute('data-function-row', 'true');
  await keys.getByRole('button', { name: 'F12', exact: true }).click();
  await expect(output).toHaveText('F12 → \\u001b[24~');
  await expect(keys.getByRole('button', { name: 'Escape' })).toHaveCSS('cursor', 'pointer');
  await demo.screenshot({ path: test.info().outputPath('hs2-cks78m-demo.png') });
  // HS2-FRB545: the Fn row's clipboard group reports what production does.
  await keys.getByRole('button', { name: 'Paste' }).click();
  await expect(output).toHaveText('Paste → sends the clipboard to the terminal');
  await keys.getByRole('button', { name: 'Copy terminal text' }).click();
  await expect(output).toHaveText('Copy → opens the terminal Copy sheet');
});

test('catalogs the touch-first desktop Copy and Paste variants (HS2-5DHHPV)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=terminal-drawer');
  const touchDrawer = page.getByRole('region', { name: 'Touch-first desktop' }),
    rail = touchDrawer.locator('.terminal-drawer__rail');
  await expect(rail.getByRole('button', { name: 'Copy terminal text' })).toBeVisible();
  await expect(rail.getByRole('button', { name: 'Paste', exact: true })).toBeVisible();
  await expect(rail.getByRole('button', { name: 'Hide terminal drawer' })).toBeVisible();
  // The ordinary drawer variant keeps its single Hide action.
  await expect(
    page.locator('.terminal-drawer-demo .terminal-drawer__rail').getByRole('button', { name: 'Copy terminal text' }),
  ).toHaveCount(0);
  await touchDrawer.screenshot({ path: test.info().outputPath('hs2-5dhhpv-drawer-demo.png') });
  await page.goto('/ux-demo?component=fixed-aspect-terminal-card');
  const cards = page.getByRole('region', { name: 'Fixed aspect terminal card variants' }),
    tablet = cards
      .locator('div', { has: page.getByRole('heading', { name: 'Magnified, touch-first desktop' }) })
      .last();
  await expect(tablet.getByRole('button', { name: 'Copy terminal text' })).toBeVisible();
  await expect(tablet.getByRole('button', { name: 'Paste', exact: true })).toBeVisible();
  await expect(tablet.getByRole('button', { name: 'Open Development in project terminal drawer' })).toBeVisible();
});

test('catalogs the long-press terminal edit menu and its actions (HS2-KKP8YJ)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=terminal-edit-menu');
    const demo = page.getByRole('region', { name: 'Terminal edit menu' }),
      output = demo.locator('[data-edit-menu-demo-output]'),
      menu = page.locator('[data-context-menu="terminal-edit"]');
    await expect(output).toHaveText('Open the menu, then choose an action.');
    await demo.getByRole('button', { name: 'Long-press here' }).click();
    const copy = menu.getByRole('menuitem', { name: 'Copy Text…' });
    await expect(copy).toBeVisible();
    await expect(copy.locator('[data-lucide="text-select"]')).toHaveCount(1);
    await expect(menu.getByRole('menuitem', { name: 'Paste' }).locator('[data-lucide="clipboard-paste"]')).toHaveCount(
      1,
    );
    await expect(menu.getByRole('menuitem')).toHaveText(['Copy Text…', 'Paste']);
    await expect(copy).toHaveCSS('cursor', 'pointer');
    await page.waitForTimeout(300);
    await page.screenshot({ path: test.info().outputPath(`hs2-kkp8yj-demo-${width}.png`) });
    await copy.click();
    await expect(output).toHaveText('Copy Text… → opens the terminal Copy sheet');
    await expect(menu).toHaveCount(0);
    // Reopen after an action, then choose the other item.
    await demo.getByRole('button', { name: 'Long-press here' }).click();
    await menu.getByRole('menuitem', { name: 'Paste' }).click();
    await expect(output).toHaveText('Paste → sends the clipboard to the terminal');
    await expect(menu).toHaveCount(0);
    // Escape dismisses without acting.
    await demo.getByRole('button', { name: 'Long-press here' }).click();
    await expect(copy).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
    await expect(output).toHaveText('Paste → sends the clipboard to the terminal');
    // After a long-press selected a word, the menu leads with Copy (HS2-EYR96N).
    await demo.getByRole('button', { name: 'Long-press a word' }).click();
    const copySelection = menu.getByRole('menuitem', { name: 'Copy', exact: true });
    await expect(menu.getByRole('menuitem')).toHaveText(['Copy', 'Copy Text…', 'Paste']);
    await expect(copySelection.locator('[data-lucide="copy"]')).toHaveCount(1);
    await page.waitForTimeout(300);
    await page.screenshot({ path: test.info().outputPath(`hs2-eyr96n-demo-${width}.png`) });
    await copySelection.click();
    await expect(output).toHaveText('Copy → copies the selected terminal text');
    await expect(menu).toHaveCount(0);
    // Reset back to the plain menu: Copy is gone again.
    await demo.getByRole('button', { name: 'Long-press here' }).click();
    await expect(menu.getByRole('menuitem')).toHaveText(['Copy Text…', 'Paste']);
    await page.keyboard.press('Escape');
  }
});

test('catalogs the terminal Copy and Paste sheets with every variant (HS2-FRB545)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=terminal-copy-dialog');
    const copyDemo = page.getByRole('region', { name: 'Terminal copy sheet' }),
      copySheet = copyDemo.locator('[data-component="terminal-copy-dialog"]'),
      output = copyDemo.locator('[data-clipboard-demo-output]'),
      field = copySheet.getByRole('textbox', { name: 'Terminal text' });
    await copyDemo.getByRole('button', { name: 'Open copy sheet' }).click();
    await expect(copySheet).toHaveJSProperty('open', true);
    await expect(field).toHaveJSProperty('readOnly', true);
    await expect(field).toHaveValue(/All 1504 tests passed\./);
    await page.waitForTimeout(400);
    await page.screenshot({ path: test.info().outputPath(`hs2-frb545-copy-demo-${width}.png`) });
    await field.evaluate((node: HTMLTextAreaElement) => {
      node.setSelectionRange(0, 'line 1'.length);
    });
    await copySheet.getByRole('button', { name: 'Copy', exact: true }).click();
    await expect(copySheet).toHaveJSProperty('open', false);
    await expect(output).toHaveText('Copied selection (1 line)');
    await copyDemo.getByRole('button', { name: 'Open copy sheet' }).click();
    await copySheet.getByRole('button', { name: 'Copy', exact: true }).click();
    await expect(output).toHaveText('Copied terminal text (43 lines)');

    await page.goto('/ux-demo?component=terminal-paste-dialog');
    const pasteDemo = page.getByRole('region', { name: 'Terminal paste sheet' }),
      pasteSheet = pasteDemo.locator('[data-component="terminal-paste-dialog"]'),
      pasteOutput = pasteDemo.locator('[data-clipboard-demo-output]'),
      pasteField = pasteSheet.getByRole('textbox', { name: 'Text to paste' });
    await pasteDemo.getByRole('button', { name: 'Clipboard denied' }).click();
    await expect(pasteSheet).toContainText('Clipboard access was not allowed.');
    await pasteField.fill('ls\npwd');
    await page.waitForTimeout(400);
    await page.screenshot({ path: test.info().outputPath(`hs2-frb545-paste-demo-${width}.png`) });
    await pasteSheet.getByRole('button', { name: 'Paste', exact: true }).click();
    await expect(pasteSheet).toHaveJSProperty('open', false);
    await expect(pasteOutput).toHaveText('Pasted → ls\\rpwd');
    await pasteDemo.getByRole('button', { name: 'Clipboard unavailable' }).click();
    await expect(pasteSheet).toContainText('This browser does not let Hot Sheet read the clipboard.');
    await expect(pasteField).toHaveValue('');
    await pasteSheet.getByRole('button', { name: 'Cancel' }).click();
    await expect(pasteSheet).toHaveJSProperty('open', false);
  }
});

test('renders project-owned ticket sources and machine-wide accounts at wide and phone widths (HS2-SM9PM8)', async ({
  page,
}) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=ticket-sources-settings');
    const sources = page.locator('[data-component="ticket-sources-settings"]');
    await expect(sources.locator('wa-select[name="project-default-source"]')).toHaveJSProperty('value', 'github-main');
    // Colors are edited in each source's dialog; the settings list shows the chosen marks.
    await expect(sources.locator('input[name="project-source-color"]')).toHaveCount(0);
    await expect(sources.locator('[data-component="ticket-source-icon"]')).toHaveCount(3);
    const gitSourceCopy = sources.locator(
      '.ticket-provider-settings__source-row[data-source-provider="git"] .ticket-provider-settings__connection-copy',
    );
    expect((await gitSourceCopy.boundingBox())!.width).toBeGreaterThan(100);
    await expect(sources).toContainText('Some ticket sources look alike');
    await expect(sources.locator('[data-component="ticket-source-icon"][data-provider="github"]').first()).toHaveCSS(
      'color',
      'rgb(59, 130, 246)',
    );
    await expect(sources.getByRole('button', { name: 'Remove Product issues from this project' })).toBeVisible();
    await expect(sources).toContainText('Also used by marketing-site');
    // Never another project's sources to attach.
    await expect(sources.locator('[data-action="attach-project-source"]')).toHaveCount(0);
    await expect(sources.locator('[data-state="disabled"]')).toHaveText('Disabled');
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    await page.screenshot({ path: test.info().outputPath(`project-sources-${width}.png`), fullPage: true });
    await page.goto('/ux-demo?component=accounts-settings');
    const accounts = page.locator('[data-component="accounts-settings"]').first();
    await expect(accounts.locator('[data-account-id="github-app-01demo"]')).toContainText(
      'small-tale/hotsheet-docs · Used by hotsheet2, marketing-site',
    );
    await expect(accounts.locator('[data-account-id="jira-token"]')).toContainText('dev@acme.test');
    // Only the unused sign-in can be signed out, and its control fits the card.
    const signOut = accounts.locator('[data-account-id="github-app-01unused"] [data-action="sign-out-account"]');
    await expect(signOut).toBeVisible();
    await expect(accounts.locator('[data-action="sign-out-account"]')).toHaveCount(1);
    const card = (await accounts.locator('[data-account-id="github-app-01unused"]').boundingBox())!,
      button = (await signOut.boundingBox())!;
    expect(button.x + button.width).toBeLessThanOrEqual(card.x + card.width);
    expect(
      await signOut.evaluate(
        (node) => getComputedStyle(node.shadowRoot!.querySelector<HTMLElement>('[part~="base"]')!).cursor,
      ),
    ).toBe('pointer');
    await expect(page.locator('[data-component="accounts-settings"]').nth(1)).toContainText('No accounts yet.');
    // The load-error alert keeps the danger color; only footnotes are quiet (HS2-JCPM76).
    const errored = page.locator('[data-component="accounts-settings"]').nth(1),
      alertColor = await errored.getByRole('alert').evaluate((node) => getComputedStyle(node).color),
      footnoteColor = await errored
        .locator('.ticket-provider-settings__footnote')
        .evaluate((node) => getComputedStyle(node).color),
      dangerColor = await errored.evaluate((node) => {
        const probe = document.createElement('span');
        probe.style.color = 'var(--wa-color-danger-on-quiet)';
        node.append(probe);
        const color = getComputedStyle(probe).color;
        probe.remove();
        return color;
      });
    expect(alertColor).toBe(dangerColor);
    expect(alertColor).not.toBe(footnoteColor);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    await page.screenshot({ path: test.info().outputPath(`accounts-${width}.png`), fullPage: true });
  }
});

test('shows the source mark after the type on cards and beside the inspector number (HS2-068Q55)', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-row');
  const row = page.locator('[data-component="ticket-list-row"]');
  await expect(row.locator('[data-component="ticket-source-icon"]')).toHaveAttribute('data-provider', 'github');
  await expect(row.locator('[data-component="ticket-source-icon"]')).toHaveCSS('color', 'rgb(59, 130, 246)');
  await page.goto('/ux-demo?component=ticket-inspector');
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await expect(
    inspector.locator('.ticket-inspector__source-identity [data-component="ticket-source-icon"]'),
  ).toHaveAttribute('data-provider', 'git');
});

test('ticket source connection rows hover flush with their card edge (HS2-KZP94T)', async ({ page }) => {
  for (const width of [1100, 390]) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto('/ux-demo?component=ticket-sources-settings');
    const card = page.locator('.ticket-provider-settings__connections'),
      rows = card.locator('.kui-list-action-row__primary');
    await expect(rows).toHaveCount(2);
    await rows.first().hover();
    const cardBox = (await card.boundingBox())!,
      rowBox = (await rows.first().boundingBox())!;
    // Only the card's 1px border separates the row from the card edge: no inline inset.
    expect(rowBox.x - cardBox.x).toBeCloseTo(1, 0);
    const hovered = await rows.first().evaluate((node) => {
      const row = node.closest<HTMLElement>('.kui-list-action-row')!,
        fills = [getComputedStyle(node).backgroundColor, getComputedStyle(row).backgroundColor];
      return {
        left: getComputedStyle(row).borderLeftWidth,
        background: fills.find((fill) => fill !== 'rgba(0, 0, 0, 0)') ?? 'rgba(0, 0, 0, 0)',
      };
    });
    // No visible per-row hover outline doubling the card border; the hover fill still shows.
    expect(hovered.left).toBe('0px');
    expect(hovered.background).not.toBe('rgba(0, 0, 0, 0)');
  }
});

test('aligns the TicketSearchFormField with a Web Awesome input and keeps its search actions (HS2-E40KC0)', async ({
  page,
}) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto('/ux-demo?component=ticket-search-field');
    const section = page.locator('.ticket-search-field-demo__form');
    await section.scrollIntoViewIfNeeded();
    const query = section.getByRole('searchbox', { name: 'View query' });
    await expect(query).toHaveAttribute('aria-required', 'true');
    await expect(
      section.getByText('Use the same words, fields, operators, and filter chips as ticket search.'),
    ).toBeVisible();
    await expect
      .poll(async () =>
        section.evaluate((node) => {
          const input = node.querySelector<HTMLElement>('wa-input')!,
            inputLabel = input.shadowRoot!.querySelector<HTMLElement>('[part~="form-control-label"]')!,
            inputBox = input.shadowRoot!.querySelector<HTMLElement>('[part~="base"]')!.getBoundingClientRect(),
            queryLabel = node.querySelector<HTMLElement>('[data-token-search-form-label]')!,
            queryBox = node
              .querySelector<HTMLElement>('[data-component="token-search-field"]')!
              .getBoundingClientRect(),
            style = (element: HTMLElement) => {
              const computed = getComputedStyle(element);
              return [computed.fontSize, computed.fontWeight, computed.textTransform].join(' ');
            };
          return {
            typography: style(inputLabel) === style(queryLabel),
            edges: Math.abs(inputBox.left - queryBox.left) < 1 && Math.abs(inputBox.right - queryBox.right) < 1,
          };
        }),
      )
      .toEqual({ typography: true, edges: true });
    // The shared delegated wiring reaches the form field: a chip, syntax help, and clear.
    await query.click();
    await page.keyboard.type('tag:docs ');
    await expect(section.locator('[data-component="token-search-token"]')).toContainText('tag:docs');
    // Its chips share the grouped field's quiet tint through the same className hook, in light and dark
    // themes (KF-5G8WJ0, HS2-RXHZVR).
    const grouped = page.getByRole('searchbox', { name: 'Search query', exact: true }).first();
    await grouped.click();
    await page.keyboard.type('tag:docs ');
    const chipTints = () =>
      page.evaluate(() => {
        const background = (selector: string) =>
            getComputedStyle(document.querySelector<HTMLElement>(`${selector} [data-component="token-search-token"]`)!)
              .backgroundColor,
          probe = document.body.appendChild(document.createElement('div'));
        probe.style.background = 'var(--wa-color-brand-fill-quiet)';
        const quiet = getComputedStyle(probe).backgroundColor;
        probe.remove();
        return {
          form: background('.ticket-search-field-demo__form'),
          grouped: background('[data-token-search-id="ticket-search-demo"]'),
          quiet,
        };
      });
    for (const theme of ['dark', 'light'] as const) {
      await page.getByRole('button', { name: `Use ${theme} theme`, exact: true }).evaluate((button: HTMLElement) => {
        button.click();
      });
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
      const tints = await chipTints();
      expect(tints.form).toBe(tints.quiet);
      expect(tints.grouped).toBe(tints.quiet);
    }
    await page.locator('[data-token-search-id="ticket-search-demo"] [data-action="clear-ticket-search"]').click();
    await expect(
      page.locator('[data-token-search-id="ticket-search-demo"] [data-component="token-search-token"]'),
    ).toHaveCount(0);
    await section.getByRole('button', { name: 'Search syntax help' }).click();
    await expect(section.getByRole('dialog', { name: 'Search syntax' })).toBeVisible();
    await section.screenshot({ path: test.info().outputPath(`ticket-search-form-field-${width}.png`) });
    await section.getByRole('button', { name: 'Search syntax help' }).click();
    await expect(section.getByRole('dialog', { name: 'Search syntax' })).toHaveCount(0);
    await section.getByRole('button', { name: 'Clear form search query' }).click();
    await expect(section.locator('[data-component="token-search-token"]')).toHaveCount(0);
  }
});

test('completes tags, applies dates, and explains syntax in the TicketSearchField demo (HS2-N5G6JS)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 1100 });
  await page.goto('/ux-demo?component=ticket-search-field');
  const demo = page.getByRole('region', { name: 'TicketSearchField demo' });
  const fields = demo.locator('.ticket-search-field');
  await expect(fields).toHaveCount(6);
  expect(
    await fields.evaluateAll((nodes) =>
      nodes.every((node) => node.getAttribute('data-expanded-overflow') === 'visible'),
    ),
  ).toBe(true);
  expect(await fields.evaluateAll((nodes) => nodes.every((node) => node.closest('[data-component="toolbar"]')))).toBe(
    true,
  );
  const query = demo.getByRole('searchbox', { name: 'Search query', exact: true }).first();
  await query.fill('parser tag:');
  // Kerf's model renders the in-place tag completion inside the field (HS2-5JXBQY).
  const suggestions = fields.first().locator('.kui-token-search__suggestions');
  await expect(suggestions.getByRole('button')).toHaveText([
    'tag:client',
    'tag:docs',
    'tag:"needs design"',
    'tag:parser',
    'tag:server',
    'tag:ui',
  ]);
  await query.fill('parser tag:NE');
  await expect(suggestions.getByRole('button')).toHaveText(['tag:"needs design"']);
  await suggestions.getByRole('button', { name: 'tag:"needs design"' }).click();
  const chips = demo.locator('[data-component="token-search-token"]');
  await expect(chips).toHaveCount(1);
  await expect(chips.first()).toHaveAttribute('data-token-value', 'tag:"needs design"');
  await expect(chips.first()).toContainText('tag:needs design');
  await expect(suggestions).toHaveCount(0);
  await expect(demo.locator('.component-stage__event')).toHaveText('Added tag:"needs design"');
  // Chip edit returns the raw filter to text; remove drops it.
  await chips.first().getByRole('button', { name: 'Edit tag:needs design' }).click();
  await expect(chips).toHaveCount(0);
  await expect(query).toContainText('tag:"needs design"');
  await demo.getByRole('button', { name: 'Clear search query' }).first().click();
  await expect(query).toHaveText('');
  // A trailing lifecycle filter opens the date helper; Apply commits the chip.
  await query.fill('updated-after:');
  const helper = demo.getByRole('group', { name: 'Date and time helper' });
  await helper.getByLabel('Date').fill('2026-09-01');
  await helper.getByLabel('Time (optional)').fill('11:05');
  await helper.getByRole('button', { name: 'Apply' }).click();
  await expect(chips).toHaveCount(1);
  await expect(chips.first()).toHaveAttribute('data-token-value', 'updated-after:2026-09-01T11:05');
  await expect(helper).toHaveCount(0);
  await chips
    .first()
    .getByRole('button', { name: /^Remove/ })
    .click();
  await expect(chips).toHaveCount(0);
  // Syntax help toggles from the field's trailing button.
  const helpButton = demo.getByRole('button', { name: 'Search syntax help' }).first();
  await helpButton.click();
  const help = fields.first().getByRole('dialog', { name: 'Search syntax' });
  await expect(help.locator('dt')).toHaveText(['Tags', 'Content', 'Workflow', 'Dates']);
  await expect(helpButton).toHaveAttribute('aria-expanded', 'true');
  // The floating popover escapes Kerf's clipped search group and overlays the sections below.
  const overlays = await help.evaluate((node) => {
    const box = node.getBoundingClientRect(),
      next = node.closest('.ticket-search-field-demo > div')!.nextElementSibling!.getBoundingClientRect();
    return box.height > 200 && box.bottom > next.top && getComputedStyle(node).position === 'absolute';
  });
  expect(overlays).toBe(true);
  await expect(fields.first()).toHaveCSS('overflow', 'visible');
  await page.screenshot({ path: 'target/visual-captures/hs2-pv2ag1-search-wide.png', fullPage: true });
  await page.screenshot({
    path: 'target/visual-captures/hs2-n5g6js-ticket-search-field-demo-wide.png',
    fullPage: true,
  });
  await page.setViewportSize({ width: 600, height: 760 });
  await help.scrollIntoViewIfNeeded();
  await expect
    .poll(() =>
      help.evaluate((node) => {
        const box = node.getBoundingClientRect();
        return box.left >= 0 && box.right <= innerWidth;
      }),
    )
    .toBe(true);
  await page.screenshot({
    path: 'target/visual-captures/hs2-n5g6js-ticket-search-field-demo-narrow.png',
    fullPage: true,
  });
  await helpButton.click();
  await expect(help).toHaveCount(0);
  await page.setViewportSize({ width: 1280, height: 1100 });
  // The external variant renders its surfaces in flow below the toolbar, inside the consumer's layout.
  const externalSection = demo.locator('.ticket-search-field-demo__external'),
    externalQuery = externalSection.getByRole('searchbox', { name: 'Dialog search query' }),
    externalSurfaces = externalSection.locator('.ticket-search-surfaces');
  await expect(fields.nth(1).locator('.ticket-search-surfaces')).toHaveCount(0);
  await externalQuery.fill('tag:d');
  // Kerf's tag completion stays inside the field itself, even with external helper surfaces; since
  // beta.63 (KF-EZRBXH, KF-94A4J3) it is an anchored popover below the editor rather than in flow.
  const externalSuggestions = fields.nth(1).locator('.kui-token-search__suggestions');
  await expect(externalSuggestions.getByRole('button')).toHaveText(['tag:docs']);
  expect(
    await externalSuggestions.evaluate((node) => {
      const field = node.closest<HTMLElement>('.ticket-search-field')!;
      return {
        position: getComputedStyle(node).position,
        insideField: field.contains(node),
        belowEditor:
          node.getBoundingClientRect().top >=
          field.querySelector<HTMLElement>('[data-token-search-editor]')!.getBoundingClientRect().bottom - 1,
      };
    }),
  ).toEqual({ position: 'absolute', insideField: true, belowEditor: true });
  await expect(externalSurfaces.locator('.kui-token-search__suggestions')).toHaveCount(0);
  await externalSuggestions.getByRole('button', { name: 'tag:docs' }).click();
  await expect(fields.nth(1).locator('[data-component="token-search-token"]')).toHaveAttribute(
    'data-token-value',
    'tag:docs',
  );
  await expect(externalSuggestions.getByRole('button')).toHaveCount(0);
  await externalSection.getByRole('button', { name: 'Search syntax help' }).click();
  const externalHelp = externalSurfaces.getByRole('dialog', { name: 'Search syntax' });
  await expect(externalHelp).toBeVisible();
  expect(await externalHelp.evaluate((node) => getComputedStyle(node).position)).toBe('static');
  await page.screenshot({
    path: 'target/visual-captures/hs2-n5g6js-ticket-search-field-demo-external.png',
    fullPage: true,
  });
  await externalSection.getByRole('button', { name: 'Search syntax help' }).click();
  await expect(externalHelp).toHaveCount(0);
  await externalSection.getByRole('button', { name: 'Clear dialog search query' }).click();
  await expect(fields.nth(1).locator('[data-component="token-search-token"]')).toHaveCount(0);
  // The collapsible variant opens from its magnifier and collapses again on Escape.
  const collapsible = fields.nth(2);
  await expect(collapsible).toHaveAttribute('data-expanded', 'false');
  await expect(collapsible).toHaveCSS('overflow', 'hidden');
  await demo.getByRole('button', { name: 'Search tickets' }).click();
  await expect(collapsible).toHaveAttribute('data-expanded', 'true');
  await expect(collapsible).toHaveCSS('overflow', 'visible');
  const collapsibleQuery = demo.getByRole('searchbox', { name: 'Search tickets' });
  await collapsibleQuery.fill('tag:s');
  await expect(collapsible.locator('.kui-token-search__suggestions').getByRole('button')).toHaveText(['tag:server']);
  await collapsible.getByRole('button', { name: 'Clear search' }).click();
  await expect(collapsibleQuery).toHaveText('');
  await collapsibleQuery.press('Escape');
  await expect(collapsible).toHaveAttribute('data-expanded', 'false');
  await expect(collapsible).toHaveCSS('overflow', 'hidden');
  // Each `layout` sizes the open field through TicketSearchField's own policy (Kerf group props,
  // HS2-DAMHD1), open -> closed -> open again, with no consumer stylesheet involved (HS2-8FS5BJ).
  const grow = fields.nth(3),
    row = fields.nth(4);
  for (let pass = 0; pass < 2; pass += 1) {
    await demo.getByRole('button', { name: 'Search grow layout' }).click();
    await expect(grow).toHaveAttribute('data-expanded', 'true');
    await expect(grow).toHaveAttribute('data-sizing', 'grow');
    await expect(grow).toHaveAttribute('data-visibility', 'hide-collapsed-tiny');
    // The 19rem floor holds beside the leading title on a wide toolbar.
    await expect.poll(async () => Math.round((await grow.boundingBox())?.width ?? 0)).toBeGreaterThanOrEqual(304);
    await demo.getByRole('searchbox', { name: 'Search grow layout' }).press('Escape');
    await expect(grow).toHaveAttribute('data-expanded', 'false');
    await expect(grow).toHaveAttribute('data-sizing', 'grow');
    await expect.poll(async () => Math.round((await grow.boundingBox())?.width ?? 0)).toBe(44);
  }
  const rowZone = row.locator('xpath=..');
  // Collapsed, the row layout's icon sits at its zone's trailing edge.
  const rowZoneBox = (await rowZone.boundingBox())!,
    rowBox = (await row.boundingBox())!;
  expect(Math.abs(rowZoneBox.x + rowZoneBox.width - (rowBox.x + rowBox.width))).toBeLessThanOrEqual(1);
  await expect(row).toHaveAttribute('data-placement', 'end');
  await demo.getByRole('button', { name: 'Search row layout' }).click();
  await expect(row).toHaveAttribute('data-expanded', 'true');
  await expect(row).toHaveAttribute('data-sizing', 'fill');
  await expect(row.locator('[data-component="token-search-field"]')).toHaveAttribute('data-fill', 'true');
  // It enters its new row with Kerf's stacked fill-search motion, not an app keyframe (KF-XFPJSY).
  expect(await row.evaluate((node) => getComputedStyle(node).animationName)).toBe('kui-toolbar-fill-search-enter');
  // Open, it fills a row of its own (Kerf `fill`).
  await expect
    .poll(async () => Math.round((await row.boundingBox())?.width ?? 0))
    .toBe(Math.round((await rowZone.boundingBox())!.width));
  await demo.getByRole('searchbox', { name: 'Search row layout' }).press('Escape');
  await expect(row).toHaveAttribute('data-expanded', 'false');
  // The disabled variant keeps its chrome but refuses input.
  await expect(demo.getByRole('searchbox', { name: 'Search query', exact: true }).nth(1)).toHaveAttribute(
    'aria-disabled',
    'true',
  );
  await expect(fields.nth(5)).toHaveAttribute('data-expanded', 'true');
});

test('pushes and pops ticket detail on the TerminalTicketRail NavStack with one toolbar row (HS2-FY06N4)', async ({
  page,
}) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=terminal-ticket-rail&dev-review=false');
    const rail = page.locator('[data-component="terminal-ticket-rail"]'),
      chromeToolbars = rail.locator(
        '[data-nav-stack-chrome] [data-component="toolbar"]:not([data-nav-stack-header] *)',
      ),
      root = rail.locator('.kui-nav-stack__view[data-nav-key="root"]');
    await expect(root).toHaveAttribute('data-nav-active', 'true');
    await expect(chromeToolbars).toHaveCount(1);
    await expect(rail.getByRole('button', { name: 'Back to ticket list' })).toHaveCount(0);
    await rail.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-R76MMW"]').click();
    const pushed = rail.locator('.kui-nav-stack__view[data-nav-key="ticket:HS2-R76MMW"]');
    await expect(pushed).toHaveAttribute('data-nav-active', 'true');
    await expect(root).toHaveAttribute('data-nav-active', 'false');
    // Wait for the NavStack push to settle (its chrome cross-fade copies are gone).
    await expect(rail.locator('[data-component="nav-stack"]')).not.toHaveAttribute(
      'data-nav-chrome-transition',
      'true',
    );
    await expect(rail.locator('[data-nav-chrome-copy]')).toHaveCount(0);
    // One toolbar row: Kerf's back control, the centered ticket number, the actions, and the toggle.
    await expect(chromeToolbars).toHaveCount(1);
    const back = chromeToolbars.getByRole('button', { name: 'Back to ticket list' });
    await expect(back).toBeVisible();
    await expect(chromeToolbars.getByRole('button', { name: /Copy ticket number HS2-R76MMW/ })).toBeVisible();
    await expect(chromeToolbars.getByRole('button', { name: 'Open ticket reader' })).toBeVisible();
    await expect(chromeToolbars.getByRole('button', { name: 'Hide ticket rail' })).toBeVisible();
    const row = await chromeToolbars.evaluate((node) => {
      const center = (selector: string) => {
        const box = node.querySelector<HTMLElement>(selector)!.getBoundingClientRect();
        return Math.round(box.top + box.height / 2);
      };
      return [center('[data-nav-back]'), center('.ticket-inspector__slug'), center('[aria-label="Hide ticket rail"]')];
    });
    expect(Math.max(...row) - Math.min(...row)).toBeLessThanOrEqual(1);
    // The inspector header stays pinned while the detail body scrolls.
    await expect(rail.locator('[data-nav-stack-header] [data-component="ticket-inspector-header"]')).toBeVisible();
    await expect(pushed.locator('[data-component="ticket-inspector-body"]')).toBeVisible();
    // Focus moved into the pushed view.
    expect(await pushed.evaluate((node) => node.contains(document.activeElement))).toBe(true);
    await page.screenshot({ path: `target/visual-captures/hs2-fy06n4-rail-detail-${width}.png` });
    await back.click();
    await expect(root).toHaveAttribute('data-nav-active', 'true');
    await expect(pushed).toHaveCount(0);
    await expect(rail.locator('wa-select[name="terminal-rail-project"]')).toBeVisible();
    await page.screenshot({ path: `target/visual-captures/hs2-fy06n4-rail-root-${width}.png` });
  }
});

test('composes startup and inspector feedback without app CSS (HS2-JMYRT3)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=app-empty-state&dev-review=false');
    const variants = page.getByRole('region', { name: 'Application empty state variants' });
    await expect(variants.locator('[data-component="empty-state"]')).toHaveCount(3);
    await expect(variants.locator('[data-busy="true"]')).toContainText('Opening Hot Sheet');
    await expect(variants.locator('[data-busy="true"] [data-component="loading-spinner"]')).toBeVisible();
    await expect(variants.getByRole('button', { name: 'Open project' })).toBeVisible();
    await expect(variants).toContainText('Project unavailable');
    const bounds = await variants.boundingBox();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(width);
    if (width === 390) {
      for (const [index, state] of (await variants.locator('[data-component="empty-state"]').all()).entries()) {
        await state.scrollIntoViewIfNeeded();
        await expect(state).toBeVisible();
        await state.screenshot({
          path: test.info().outputPath(`hs2-jmyrt3-empty-${width}-${index}.png`),
          animations: 'disabled',
        });
      }
    } else {
      await variants.screenshot({
        path: test.info().outputPath(`hs2-jmyrt3-empty-${width}.png`),
        animations: 'disabled',
      });
    }
  }
});
