import type { Locator, Page } from '@playwright/test';

/**
 * HS2-DXJ2RZ, HS2-GDBK53: the next answer to a ticket list read. Opening a project makes about
 * twenty mocked requests before the list read, each a round trip through the test process, so under
 * full-gate load the first rows could arrive after one `expect` timeout. Wait for the list itself,
 * within the test's budget, before asserting on its rows.
 */
export function ticketListLoaded(page: Page) {
  return page.waitForResponse(
    (response) =>
      response.request().method() === 'GET' && /\/checkouts\/[^/]+\/tickets$/.test(new URL(response.url()).pathname),
  );
}

/**
 * Open the mocked local project through the Open Project dialog and wait for its first ticket list
 * read (HS2-GDBK53). Use it instead of the raw three clicks so no spec asserts rows before they load.
 */
export async function openMockedProject(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  const listed = ticketListLoaded(page);
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await listed;
}

/**
 * The project tab named `name` (HS2-YGSYJT). A project tab's accessible name carries its work counts
 * after the project name ("drafts 1 Up Next ticket"), and they change as tickets enter Up Next or
 * become active, so an exact-name match stops finding a tab whose project gains work. Match the
 * project name, followed by the end of the label or a space.
 */
export function projectTab(scope: Page | Locator, name: string): Locator {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&');
  return scope.getByRole('tab', { name: new RegExp(`^${escaped}(?: |$)`, 'u') });
}
