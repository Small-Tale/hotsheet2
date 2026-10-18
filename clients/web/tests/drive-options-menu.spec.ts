import { expect, type Locator, test } from '@playwright/test';

/**
 * A submenu choice's icon and label insets once its submenu has finished opening. Wait for every
 * running animation in the document (and any ancestor shadow root), then require a few identical
 * frames; a frame-starved run under parallel load can otherwise read the scaled,
 * mid-animation geometry (HS2-X99SQ9).
 */
async function settledChoiceGeometry(choice: Locator) {
  return choice.evaluate(async (node) => {
    const roots: (Document | ShadowRoot)[] = [document];
    for (let current: Node | null = node; current;) {
      const root = current.getRootNode();
      if (root instanceof ShadowRoot) {
        roots.push(root);
        current = root.host;
      } else current = current.parentNode;
      if (current instanceof Element && current.shadowRoot && !roots.includes(current.shadowRoot))
        roots.push(current.shadowRoot);
    }
    const read = () => {
      const item = node.getBoundingClientRect();
      const icon = node.querySelector('.kui-popup-menu__icon')!.getBoundingClientRect();
      const label = node.querySelector('.kui-popup-menu__item-label')!.getBoundingClientRect();
      return { iconInset: icon.left - item.left, labelInset: label.left - item.left, width: item.width };
    };
    let previous = read(),
      stable = 0;
    for (let frame = 0; frame < 240 && stable < 3; frame += 1) {
      await Promise.all(roots.flatMap((root) => root.getAnimations()).map((animation) => animation.finished));
      await new Promise((resolve) => requestAnimationFrame(resolve));
      const next = read();
      stable = JSON.stringify(next) === JSON.stringify(previous) ? stable + 1 : 0;
      previous = next;
    }
    return previous;
  });
}

/** The top-level items of a Kerf PopupMenu root, including the triggers of its submenus. */
const TOP_ITEMS =
  ':scope > .kui-popup-menu__menu > .kui-popup-menu__item, :scope > .kui-popup-menu__menu > .kui-popup-menu__submenu-host > .kui-popup-menu__item';

test('opens the Drive options popup above its anchor (KF-MXM1SH, HS2-S97ETV)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 800 });
    await page.goto('/ux-demo?component=drive-options-menu');
    const menu = page.locator('[data-context-menu="drive-options"]');
    await expect(menu.locator(':scope > .kui-popup-menu__context-anchor')).toHaveAttribute('aria-expanded', 'true');
    expect(
      await menu.evaluate((node) => {
        const anchor = node.querySelector(':scope > .kui-popup-menu__context-anchor')!.getBoundingClientRect(),
          popup = node.querySelector(':scope > .kui-popup-menu__menu')!.getBoundingClientRect();
        return anchor.top - popup.bottom;
      }),
    ).toBeGreaterThanOrEqual(0);
    await page.screenshot({ path: test.info().outputPath(`hs2-s97etv-drive-options-catalog-${width}.png`) });
  }
});

test('uses one disclosure and aligned icon-label choices at wide and narrow sizes', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=drive-options-menu');

  const menu = page.locator('[data-context-menu="drive-options"]');
  const top = menu.locator(TOP_ITEMS);
  const provider = top.filter({ hasText: 'Provider' });
  const model = top.filter({ hasText: 'Model' });
  const effort = top.filter({ hasText: 'Effort' });

  await expect(menu.getByRole('menu', { name: 'Drive Provider, Model, and Effort Options' })).toBeVisible();
  await expect(provider).toBeVisible();
  await expect(provider.locator(':scope > [data-lucide="chevron-right"]')).toHaveCount(0);
  // Kerf draws the one submenu disclosure; the app adds none.
  await expect(provider.locator(':scope > .kui-popup-menu__submenu-icon')).toHaveCount(1);
  // A context-mode PopupMenu: the stage opened it at its anchor (placement: see the test below).
  await expect(menu.locator(':scope > .kui-popup-menu__context-anchor')).toHaveAttribute('aria-expanded', 'true');

  await model.hover();
  const modelMenu = menu.getByRole('menu', { name: 'Model', exact: true });
  const modelChoices = modelMenu.locator(':scope > .kui-popup-menu__item');
  await expect(modelChoices.first()).toBeVisible();
  // The current model is a checked PopupMenu choice; submenus hold no divider (KF-7KR1BC).
  await expect(modelChoices.first()).toHaveAttribute('aria-checked', 'true');
  await expect(modelMenu.locator(':scope > .kui-popup-menu__divider')).toHaveCount(0);
  const modelGeometry = await settledChoiceGeometry(modelChoices.first());
  await page.screenshot({ path: test.info().outputPath('drive-options-menu-wide.png'), fullPage: true });

  await effort.hover();
  const effortChoices = menu
    .getByRole('menu', { name: 'Effort', exact: true })
    .locator(':scope > .kui-popup-menu__item');
  expect(await effortChoices.evaluateAll((items) => items.map((item) => item.getAttribute('value')))).toEqual([
    'low',
    'medium',
    'high',
    'xhigh',
    'max',
  ]);
  await expect(effortChoices.and(page.locator('[value="high"]'))).toHaveAttribute('aria-checked', 'true');
  const effortGeometry = await settledChoiceGeometry(effortChoices.first());
  expect(Math.abs(effortGeometry.iconInset - modelGeometry.iconInset)).toBeLessThan(1);
  expect(Math.abs(effortGeometry.labelInset - modelGeometry.labelInset)).toBeLessThan(4);

  await page.setViewportSize({ width: 760, height: 640 });
  await effort.hover();
  await expect(effortChoices.first()).toBeVisible();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: test.info().outputPath('drive-options-menu-narrow.png'), fullPage: true });
});

test('keeps icon and label columns aligned across checked and plain rows (HS2-JFSAG0)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 800 });
    await page.goto('/ux-demo?component=drive-options-menu');
    const menu = page.locator('[data-context-menu="drive-options"]');
    await expect(menu.locator(':scope > .kui-popup-menu__context-anchor')).toHaveAttribute('aria-expanded', 'true');
    const columns = (item: import('@playwright/test').Locator) =>
      item.evaluate((node) => {
        const icon = node.querySelector(':scope > .kui-popup-menu__icon')?.getBoundingClientRect().left ?? NaN,
          range = document.createRange();
        const text = node.querySelector(':scope > .kui-popup-menu__item-label');
        if (text) range.selectNodeContents(text);
        return { icon, label: text ? range.getBoundingClientRect().left : NaN };
      });
    const top = menu.locator(TOP_ITEMS);
    const fallback = await columns(top.filter({ hasText: 'Default' }).first()),
      provider = await columns(top.filter({ hasText: 'Provider' }).first());
    expect(Math.abs(fallback.icon - provider.icon)).toBeLessThan(1);
    const model = top.filter({ hasText: 'Model' }).first();
    await model.hover();
    const choices = menu.getByRole('menu', { name: 'Model', exact: true }).locator(':scope > .kui-popup-menu__item');
    await expect(choices.first()).toBeVisible();
    await page.waitForTimeout(400);
    const checked = await columns(choices.first()),
      other = await columns(choices.last());
    expect(Math.abs(checked.icon - other.icon)).toBeLessThan(1);
    expect(Math.abs(checked.label - other.label)).toBeLessThan(1);
    await page.screenshot({ path: test.info().outputPath(`hs2-jfsag0-after-${width}.png`) });
  }
});
