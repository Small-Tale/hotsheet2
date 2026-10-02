import { expect, type Locator, test } from '@playwright/test';

const pausedMotion = '.content-transition__side { animation-play-state: paused !important; }';

test('demonstrates adjacent full-width A-B push/pop with chrome crossfades and reduced motion', async ({ page }) => {
  await page.goto('/ux-demo?component=content-transition');
  const transition = page.locator('[data-component="content-transition"][data-transition-region="content"]'),
    label = page.locator('[data-transition-region="label"]'),
    footer = page.locator('[data-transition-region="footer"]');
  await expect(transition).toHaveAttribute('data-active-side', 'a');

  // Freeze before activation so a loaded parallel runner cannot let the 280 ms CSS motion finish
  // before the midpoint geometry assertion samples it.
  const forwardPause = await page.addStyleTag({ content: pausedMotion });
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await expect(transition).toHaveAttribute('data-active-side', 'b');
  await expect
    .poll(() => transition.locator('[data-side="a"]').evaluate((node) => getComputedStyle(node).animationName))
    .toBe('content-transition-push-out-start');
  await expect
    .poll(() => transition.locator('[data-side="b"]').evaluate((node) => getComputedStyle(node).animationName))
    .toBe('content-transition-push-in-end');
  await expect
    .poll(() => label.locator('[data-side="b"]').evaluate((node) => getComputedStyle(node).animationName))
    .toBe('content-transition-fade-in');
  await expect
    .poll(() => footer.locator('[data-side="b"]').evaluate((node) => getComputedStyle(node).animationName))
    .toBe('content-transition-fade-in');
  const geometry = await transition.evaluate((node) => {
    for (const animation of node.getAnimations({ subtree: true })) animation.currentTime = 140;
    const [a, b] = [...node.querySelectorAll<HTMLElement>(':scope > [data-side]')].map((side) =>
      side.getBoundingClientRect(),
    );
    const frame = node.getBoundingClientRect();
    return { frameWidth: frame.width, aWidth: a.width, bWidth: b.width, seam: b.left - a.right };
  });
  expect(geometry.aWidth).toBeCloseTo(geometry.frameWidth, 0);
  expect(geometry.bWidth).toBeCloseTo(geometry.frameWidth, 0);
  expect(Math.abs(geometry.seam)).toBeLessThanOrEqual(1);
  await forwardPause.evaluate((node) => {
    node.parentNode?.removeChild(node);
  });
  await expect(transition.locator('[data-side="a"]')).toHaveCSS('opacity', '0');
  await page.screenshot({ path: '/private/tmp/hs2-0epnb7-content-transition-push-after.png', fullPage: true });

  await page.setViewportSize({ width: 720, height: 720 });
  await transition.scrollIntoViewIfNeeded();
  await expect(transition).toBeInViewport();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-0epnb7-content-transition-push-after-narrow.png', fullPage: true });

  const backwardPause = await page.addStyleTag({ content: pausedMotion });
  await page.getByRole('button', { name: 'Back', exact: true }).click();
  await expect
    .poll(() => transition.locator('[data-side="a"]').evaluate((node) => getComputedStyle(node).animationName))
    .toBe('content-transition-push-in-start');
  await backwardPause.evaluate((node) => {
    node.parentNode?.removeChild(node);
  });

  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const crossfadePause = await page.addStyleTag({ content: pausedMotion });
  for (const [name, value] of [
    ['transition-style', 'crossfade'],
    ['transition-side', 'b'],
  ] as const)
    await page.locator(`wa-select[name="${name}"]`).evaluate((node: HTMLElement & { value: string }, next) => {
      node.value = next;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);
  await expect(transition).toHaveAttribute('data-transition-style', 'crossfade');
  await expect
    .poll(() => transition.locator('[data-side="b"]').evaluate((node) => getComputedStyle(node).animationName))
    .toBe('content-transition-fade-in');
  await page.emulateMedia({ reducedMotion: 'reduce' });
  expect(
    parseFloat(
      await transition.locator('[data-side="b"]').evaluate((node) => getComputedStyle(node).animationDuration),
    ),
  ).toBeLessThanOrEqual(0.001);
  await crossfadePause.evaluate((node) => {
    node.parentNode?.removeChild(node);
  });
});

test('lays out dialog footers through the ContentTransition action-row variant (HS2-29Q3XG)', async ({ page }) => {
  const right = (locator: Locator) => locator.evaluate((node) => node.getBoundingClientRect().right),
    actionRow = async (footer: Locator) => {
      const side = footer.locator(':scope > [data-active="true"]');
      await expect(footer).toHaveAttribute('data-side-layout', 'actions');
      await expect(side).toHaveCSS('display', 'flex');
      await expect(side).toHaveCSS('flex-wrap', 'wrap');
      await expect(side).toHaveCSS('justify-content', 'flex-end');
      return side;
    },
    selectScenario = (dialog: Locator, value: string) =>
      dialog
        .locator('[data-demo-ticket-source-scenario] wa-select')
        .evaluate((node: HTMLElement & { value: string }, next) => {
          node.value = next;
          node.dispatchEvent(new Event('change', { bubbles: true }));
        }, value);
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await page.goto('/ux-demo?component=content-transition&dev-review=false');
    await expect(page.locator('[data-transition-region="content"]')).toHaveAttribute('data-side-layout', 'block');
    await page.getByRole('button', { name: 'Continue', exact: true }).click();
    const demoSide = await actionRow(page.locator('[data-transition-region="footer"]'));
    expect(await right(demoSide.getByRole('button', { name: 'Connect' }))).toBeCloseTo(await right(demoSide), 0);

    await page.goto('/ux-demo?component=ticket-source-setup-dialog&dev-review=false');
    const setup = page.locator('[data-ticket-source-setup-dialog]'),
      setupFooter = setup.locator('[data-transition-region="footer"]'),
      label = setup.locator('[data-transition-region="label"]'),
      rootSide = await actionRow(setupFooter);
    // The slotted label and footer regions span their dialog slots instead of shrinking to content.
    const dialogWidth = await setup.evaluate(
      (node) => node.shadowRoot!.querySelector('[part~="dialog"]')!.getBoundingClientRect().width,
    );
    expect(await setupFooter.evaluate((node) => node.getBoundingClientRect().width)).toBeGreaterThan(dialogWidth / 2);
    expect(await label.evaluate((node) => node.getBoundingClientRect().width)).toBeGreaterThan(dialogWidth / 2);
    // The root step's lone Cancel keeps its start alignment through the dialog's own button class.
    expect(
      await rootSide.getByRole('button', { name: 'Cancel' }).evaluate((node) => node.getBoundingClientRect().left),
    ).toBeCloseTo(await rootSide.evaluate((node) => node.getBoundingClientRect().left), 0);
    await selectScenario(setup, 'editing');
    await expect(setup).toHaveAttribute('data-preview-scenario', 'editing');
    const editSide = await actionRow(setupFooter);
    await expect(editSide.getByRole('button', { name: 'Save changes' })).toBeVisible();
    expect(await right(editSide.getByRole('button', { name: 'Save changes' }))).toBeCloseTo(await right(editSide), 0);

    await page.goto('/ux-demo?component=conversation-export-dialog&dev-review=false');
    const exportSide = await actionRow(
      page.locator('[data-component="conversation-export-dialog"] [data-transition-region="footer"]'),
    );
    const submit = exportSide.getByRole('button', { name: 'Save conversation' });
    expect(await right(submit)).toBeCloseTo(await right(exportSide), 0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  }
});
