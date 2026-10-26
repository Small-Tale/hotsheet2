import { expect, type Locator, type Page, test } from '@playwright/test';

import { desktopApp, dialog, openNewTicket, openNotWorking, responded, stage } from './desktop-draft-files-fixture';
import { expectProjectSidebarLayout } from './project-sidebar-layout';

async function inspectWidths(page: Page, surface: Locator, name: string) {
  for (const [label, width] of [
    ['wide', 1280],
    ['medium', 900],
    ['narrow', 390],
    ['wide-again', 1280],
  ] as const) {
    await page.setViewportSize({ width, height: 900 });
    await expectProjectSidebarLayout(page, width >= 1024);
    await expect(surface.getByRole('textbox').first()).toBeVisible();
    const bounds = (await surface.locator('dialog').boundingBox())!;
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(width + 1);
    await page.screenshot({ path: `target/visual-captures/hs2-p36r6n-${name}-${label}.png` });
  }
}

for (const colorScheme of ['light', 'dark'] as const)
  test.describe(colorScheme, () => {
    test.use({ colorScheme });
    test('restores the project sidebar behind both modal flows (HS2-P36R6N)', async ({
      context,
      baseURL,
      browserName,
    }) => {
      test.setTimeout(120_000);
      const app = await desktopApp(context, baseURL!);
      try {
        const page = await app.launch();
        await openNewTicket(page, true);
        await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic responsive draft');
        const saved = responded(page, 'PUT');
        await stage(page, [{ name: 'responsive-proof.txt', bytes: [1, 2, 3] }]);
        await saved;
        await inspectWidths(page, dialog(page), `${browserName}-${colorScheme}-create`);
        await expect(dialog(page).getByRole('textbox', { name: 'Ticket Title' })).toHaveValue(
          'Synthetic responsive draft',
        );
        await expect(dialog(page).getByText('responsive-proof.txt')).toBeVisible();
        await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
        await expect(dialog(page)).toBeHidden();
        await page.getByRole('button', { name: 'Hide Project Sidebar', exact: true }).click();
        await expectProjectSidebarLayout(page, false);
        await page.getByRole('button', { name: 'Show Project Sidebar', exact: true }).click();
        await expectProjectSidebarLayout(page, true);
        await openNewTicket(page);
        // Cancel deliberately discards the draft; a new surface must start clean.
        await expect(dialog(page).getByRole('textbox', { name: 'Ticket Title' })).toHaveValue('');
        await expect(dialog(page).getByText('responsive-proof.txt')).toHaveCount(0);
        await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
        const ticket = await app.completedTicket('Synthetic responsive report');
        const report = await openNotWorking(page, ticket);
        await report.getByRole('textbox').fill('Synthetic report note');
        await inspectWidths(page, report, `${browserName}-${colorScheme}-not-working`);
        await expect(report.getByRole('textbox')).toHaveValue('Synthetic report note');
        await report.getByRole('button', { name: 'Cancel', exact: true }).click();
        await expect(report.getByRole('textbox')).toBeHidden();
        await expectProjectSidebarLayout(page, true);
        // A user-hidden desktop rail stays hidden through the same responsive round trip.
        await page.getByRole('button', { name: 'Hide Project Sidebar', exact: true }).click();
        await expectProjectSidebarLayout(page, false);
        await openNewTicket(page);
        for (const width of [390, 1280]) {
          await page.setViewportSize({ width, height: 900 });
          await expectProjectSidebarLayout(page, false);
        }
        await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
        await page.getByRole('button', { name: 'Show Project Sidebar', exact: true }).click();
        await expectProjectSidebarLayout(page, true);
      } finally {
        await app.stop();
      }
    });
  });
