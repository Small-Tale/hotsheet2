import { expect, test } from '@playwright/test';

for (const systemTheme of ['light', 'dark'] as const) {
  test(`explicit catalog themes override the ${systemTheme} system preference (HS2-0DD4XQ)`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme: systemTheme });
    await page.goto('/ux-demo?component=project-dialog&dev-review=false');
    const root = page.locator('html');
    const dialog = page.locator('[data-remote-project-dialog]');
    const stage = page.getByRole('region', { name: 'Project dialog variants' });
    const palette = () =>
      dialog.evaluate((element) => ({
        surface: getComputedStyle(element.shadowRoot!.querySelector('dialog')!).backgroundColor,
        foreground: getComputedStyle(element.querySelector('[data-component="list-item"]')!).color,
        canvas: getComputedStyle(document.documentElement).backgroundColor,
      }));
    const switchTheme = async (theme: 'light' | 'dark') => {
      await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
      await expect(dialog).toHaveJSProperty('open', false);
      await page.getByRole('button', { name: `Use ${theme} theme`, exact: true }).click();
      await expect(root).toHaveCSS('color-scheme', theme);
      await stage.getByRole('button', { name: 'Remote projects', exact: true }).click();
      await expect(dialog).toHaveJSProperty('open', true);
    };
    await expect(dialog).toHaveJSProperty('open', true);
    await expect(root).toHaveCSS('color-scheme', 'light');
    const light = await palette();
    await page.screenshot({
      path: testInfo.outputPath(`catalog-light-system-${systemTheme}-wide.png`),
      animations: 'disabled',
    });
    await switchTheme('dark');
    const dark = await palette();
    expect(dark.surface).not.toBe(light.surface);
    expect(dark.foreground).not.toBe(light.foreground);
    expect(dark.canvas).not.toBe(light.canvas);
    await page.emulateMedia({ colorScheme: systemTheme === 'light' ? 'dark' : 'light' });
    await expect.poll(palette).toEqual(dark);
    await page.emulateMedia({ colorScheme: systemTheme });
    await page.reload();
    await expect(dialog).toHaveJSProperty('open', true);
    await expect(root).toHaveCSS('color-scheme', 'dark');
    await expect.poll(palette).toEqual(dark);
    await page.screenshot({
      path: testInfo.outputPath(`catalog-dark-system-${systemTheme}-wide.png`),
      animations: 'disabled',
    });
    await switchTheme('light');
    await expect.poll(palette).toEqual(light);
    await expect.poll(() => page.evaluate(() => localStorage.getItem('hotsheet.ux-demo.theme'))).toBe('light');
    await page.setViewportSize({ width: 390, height: 844 });
    await expect.poll(palette).toEqual(light);
    await page.screenshot({
      path: testInfo.outputPath(`catalog-light-system-${systemTheme}-narrow.png`),
      animations: 'disabled',
    });
  });
}

test('demonstrates project chooser states and opens again after empty, error, and selection (HS2-XX5Y2X)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.addInitScript(() => {
    localStorage.setItem('hotsheet.ux-demo.theme', 'dark');
  });
  await page.goto('/ux-demo?component=project-dialog&dev-review=false');
  const remote = page.locator('[data-remote-project-dialog]');
  const local = page.locator('[data-project-dialog]');
  const stage = page.getByRole('region', { name: 'Project dialog variants' });
  const cancel = () => remote.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(remote).toHaveJSProperty('open', true);
  await expect(remote.locator('[data-component="list-item"]')).toHaveCount(10);
  await expect
    .poll(() =>
      remote.evaluate(
        (element) =>
          element.shadowRoot
            ?.querySelector('dialog')
            ?.getAnimations()
            .filter((animation) => animation.playState === 'running').length ?? -1,
      ),
    )
    .toBe(0);
  await remote.getByRole('button', { name: 'Demo /work/demo', exact: true }).focus();
  const firstRow = await remote.locator('[data-component="list-item"]').first().boundingBox();
  const list = await remote.getByRole('list', { name: 'Open projects' }).boundingBox();
  expect(firstRow!.y - list!.y).toBeGreaterThanOrEqual(4);
  await page.screenshot({ path: '/private/tmp/hs2-xx5y2x-remote-project-mobile-dark.png' });
  await cancel();
  for (const state of [
    { name: 'Remote loading', copy: 'Loading projects…' },
    { name: 'Remote empty', copy: 'No projects are open on the server yet.' },
    { name: 'Remote error', copy: 'Could not load the projects open on the Hot Sheet server.' },
  ]) {
    await stage.getByRole('button', { name: state.name, exact: true }).click();
    await expect(remote).toHaveJSProperty('open', true);
    await expect(remote).toContainText(state.copy);
    await expect(remote.locator('[data-component="list-item"]')).toHaveCount(0);
    if (state.name === 'Remote empty') {
      await remote.getByRole('button', { name: 'Cancel', exact: true }).focus();
      await page.screenshot({ path: '/private/tmp/hs2-xx5y2x-remote-project-empty.png', animations: 'disabled' });
    }
    await cancel();
  }
  await stage.getByRole('button', { name: 'Remote projects', exact: true }).click();
  await expect(remote.locator('[data-component="list-item"]')).toHaveCount(10);
  await remote.getByRole('button', { name: 'Demo /work/demo', exact: true }).click();
  await expect(remote).toHaveJSProperty('open', false);
  await expect(stage.getByRole('status')).toHaveText('Opened /work/demo');
  await stage.getByRole('button', { name: 'Remote projects', exact: true }).click();
  await expect(remote).toHaveJSProperty('open', true);
  await remote.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(remote).toHaveJSProperty('open', false);

  await stage.getByRole('button', { name: 'Local recovery', exact: true }).click();
  await expect(local).toHaveJSProperty('open', true);
  await expect(local).toContainText('process 4242');
  await local.getByRole('button', { name: 'Stop server and retry' }).click();
  await expect(local.locator('.project-dialog__server-recovery')).toHaveCount(0);
  await local.getByRole('textbox', { name: /^Project folder/ }).fill('/work/edited');
  await local.getByRole('button', { name: 'Open project', exact: true }).click();
  await expect(local).toHaveJSProperty('open', false);
  await expect(stage.getByRole('status')).toHaveText('Opened /work/edited');
  await stage.getByRole('button', { name: 'Local project', exact: true }).click();
  await expect(local.getByRole('textbox', { name: /^Project folder/ })).toHaveValue('/work/edited');
  await local.getByRole('button', { name: 'Cancel', exact: true }).click();
  await stage.getByRole('button', { name: 'Recovery in progress', exact: true }).click();
  await expect(local.getByRole('button', { name: 'Recovering…' })).toBeDisabled();
});
