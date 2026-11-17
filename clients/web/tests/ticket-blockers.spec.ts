import { expect, type Page, test } from '@playwright/test';

import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer } from './real-ticket-server';

type Server = Awaited<ReturnType<typeof realTicketServer>>;
interface CreatedTicket {
  id: string;
  slug: string;
}
interface StoredTicket {
  blocked_by: string[];
}

/** Proxy the project's API to the real server; `providers` may rewrite the provider list. */
async function openProject(page: Page, server: Server, providers?: (body: unknown) => unknown) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name: 'Blockers', stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url()),
      path = incoming.pathname.slice(apiPath.length);
    try {
      const response = await proxyFetch(route, {
        url: `${server.url}${path}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      if (providers && path === '/providers' && route.request().method() === 'GET')
        await route.fulfill({ response, json: providers(await response.json()) });
      else await route.fulfill({ response });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  await page.goto('/');
  await openMockedProject(page);
}

/** Select a ticket and return its Info tab's Blocked By section and Add Blocker action. */
async function blockedBy(page: Page, slug: string) {
  await page.locator(`[data-ticket-slug="${slug}"]`).first().click();
  const inspector = page.locator('#app-right-rail');
  await expect(inspector).toContainText(slug, { timeout: 20_000 });
  const section = inspector.getByRole('region', { name: 'Blocked By' });
  await expect(section).toBeVisible({ timeout: 20_000 });
  return { section, add: section.getByRole('button', { name: 'Add Blocker…' }) };
}

/** Open the Add Blocker dialog, search for `query`, and choose the row for `slug`. */
async function chooseBlocker(page: Page, add: ReturnType<Page['locator']>, query: string, slug: string) {
  await add.click();
  const dialog = page.locator('[data-dialog-component="ticket-blocker-dialog"]');
  await expect(dialog).toBeVisible();
  const field = dialog.locator('[name="blocker-target-search"]');
  await expect(field).toBeFocused();
  await field.pressSequentially(query);
  const row = dialog
    .locator('[aria-label="Matching tickets"] [data-action="select-blocker"]')
    .filter({ hasText: slug });
  await expect(row).toHaveCount(1, { timeout: 20_000 });
  return { dialog, row };
}

test('adds, refuses a cycle for, and removes ticket blockers in the real Inspector (HS2-CHH28E)', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer();
  try {
    const blocker = await server.request<CreatedTicket>('/tickets', 'POST', {
        title: 'Synthetic upstream dependency',
        category: 'task',
      }),
      blocked = await server.request<CreatedTicket>('/tickets', 'POST', {
        title: 'Synthetic downstream feature',
        category: 'feature',
      });
    await page.setViewportSize({ width: 1440, height: 900 });
    await openProject(page, server);

    const { section, add } = await blockedBy(page, blocked.slug);
    await expect(section.locator('[data-blocker-state]')).toHaveCount(0);
    await expect(add).toBeEnabled();
    const { dialog, row } = await chooseBlocker(page, add, 'Synthetic', blocker.slug);
    // The inspected ticket itself is never offered.
    await expect(dialog.locator('[data-action="select-blocker"]').filter({ hasText: blocked.slug })).toHaveCount(0);
    await expect(dialog).toContainText(`Add Blocker to ${blocked.slug}`);
    await page.screenshot({ path: 'target/visual-captures/hs2-chh28e-blocker-dialog-results-1440.png' });
    await row.click();
    await expect(dialog.locator('[data-action="clear-blocker"]')).toBeVisible();
    await dialog.getByRole('button', { name: 'Add Blocker' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(
      page.locator('.app-toast').filter({ hasText: `${blocker.slug} now blocks ${blocked.slug}.` }),
    ).toBeVisible();
    const rows = section.locator(`[data-blocker-state][data-blocker-id="${blocker.id}"]`);
    await expect(rows).toHaveCount(1);
    await expect(rows).toContainText('Synthetic upstream dependency');
    expect((await server.request<StoredTicket>(`/tickets/${blocked.id}`)).blocked_by).toEqual([blocker.id]);
    await page.screenshot({ path: 'target/visual-captures/hs2-chh28e-blocked-by-1440.png' });

    // The reverse dependency would close a cycle: the server's refusal stays in the dialog.
    const upstream = await blockedBy(page, blocker.slug);
    const reverse = await chooseBlocker(page, upstream.add, blocked.slug, blocked.slug);
    await reverse.row.click();
    await reverse.dialog.getByRole('button', { name: 'Add Blocker' }).click();
    await expect(reverse.dialog.getByRole('alert')).toContainText(
      `${blocked.slug} already depends on ${blocker.slug}, so it cannot also block ${blocker.slug} (dependency cycle)`,
    );
    await expect(reverse.dialog).toBeVisible();
    await page.screenshot({ path: 'target/visual-captures/hs2-chh28e-blocker-dialog-cycle-1440.png' });
    expect((await server.request<StoredTicket>(`/tickets/${blocker.id}`)).blocked_by).toEqual([]);
    await reverse.dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(reverse.dialog).toHaveCount(0);

    // Remove the blocker from the downstream ticket's row.
    const downstream = await blockedBy(page, blocked.slug);
    await downstream.section
      .getByRole('button', { name: `Remove ${blocker.slug} as a blocker of ${blocked.slug}` })
      .click();
    await expect(
      page.locator('.app-toast').filter({ hasText: `${blocker.slug} no longer blocks ${blocked.slug}.` }),
    ).toBeVisible();
    await expect(downstream.section.locator('[data-blocker-state]')).toHaveCount(0);
    expect((await server.request<StoredTicket>(`/tickets/${blocked.id}`)).blocked_by).toEqual([]);
  } finally {
    await server.stop(page);
  }
});

test('opens a blocker from its row and keeps the layout across widths (HS2-CHH28E)', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer();
  try {
    const blocker = await server.request<CreatedTicket>('/tickets', 'POST', {
      title: 'Synthetic upstream dependency with a deliberately long title that has to wrap inside a narrow inspector',
    });
    const done = await server.request<CreatedTicket>('/tickets', 'POST', {
      title: 'Synthetic finished dependency',
    });
    await server.request(`/tickets/${done.id}`, 'PATCH', { status: 'completed' });
    const blocked = await server.request<CreatedTicket>('/tickets', 'POST', {
      title: 'Synthetic downstream feature',
      blocked_by: [blocker.id, done.id],
    });
    await openProject(page, server);
    for (const width of [1440, 1024, 800, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      const { section } = await blockedBy(page, blocked.slug);
      await expect(section.locator('[data-blocker-state]')).toHaveCount(2);
      await expect(section.locator(`[data-blocker-state][data-blocker-id="${done.id}"]`)).toContainText('Done');
      await section.scrollIntoViewIfNeeded();
      await page.screenshot({ path: `target/visual-captures/hs2-chh28e-blocked-by-${width}.png` });
    }
    const { section } = await blockedBy(page, blocked.slug);
    await section
      .locator(`[data-blocker-state][data-blocker-id="${blocker.id}"] [data-action="open-duplicate-target"]`)
      .click();
    await expect(page.locator('#app-right-rail')).toContainText(blocker.slug, { timeout: 20_000 });
  } finally {
    await server.stop(page);
  }
});

test('disables Add Blocker with its reason for a source without dependencies (HS2-CHH28E)', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const ticket = await server.request<CreatedTicket>('/tickets', 'POST', { title: 'Synthetic provider ticket' });
    await page.setViewportSize({ width: 1440, height: 900 });
    // Present the same source as a provider without dependencies; every other route is real.
    await openProject(page, server, (body) =>
      (body as { capabilities: Record<string, unknown> }[]).map((item) => ({
        ...item,
        display_name: 'GitHub Issues',
        capabilities: { ...item.capabilities, dependencies: false },
      })),
    );
    const { add } = await blockedBy(page, ticket.slug);
    await expect(add).toBeDisabled();
    await expect(add).toHaveAttribute('title', 'GitHub Issues tickets cannot record blocking dependencies.');
    await page.screenshot({ path: 'target/visual-captures/hs2-chh28e-blocked-by-unsupported-1440.png' });
  } finally {
    await server.stop(page);
  }
});

const READER_WIDTHS = [
  ['wide', 1440, 900],
  ['medium', 1024, 800],
  ['narrow', 390, 844],
  ['wide-again', 1440, 900],
] as const;

test('adds, refuses a cycle for, and removes blockers in the modal reader and a linked reader (HS2-15PE59)', async ({
  page,
}) => {
  test.setTimeout(240_000);
  const server = await realTicketServer();
  try {
    const blocker = await server.request<CreatedTicket>('/tickets', 'POST', {
        title: 'Synthetic reader upstream dependency',
        category: 'task',
      }),
      blocked = await server.request<CreatedTicket>('/tickets', 'POST', {
        title: 'Synthetic reader downstream feature',
        category: 'feature',
      });
    await page.setViewportSize({ width: 1440, height: 900 });
    await openProject(page, server);
    await page.locator(`[data-ticket-slug="${blocked.slug}"]`).first().click();
    await page.locator('#app-right-rail').getByRole('button', { name: 'Open Ticket Reader' }).click();
    const reader = page.getByRole('dialog', { name: `Read and edit ${blocked.slug}` });
    await reader.getByRole('tab', { name: 'Info' }).click();
    const section = reader.getByRole('region', { name: 'Blocked By' });
    await expect(section).toBeVisible({ timeout: 20_000 });
    const add = section.getByRole('button', { name: 'Add Blocker…' });
    await expect(add).toBeEnabled();

    // The dialog opens inside the modal reader; Escape closes only the dialog.
    await add.click();
    const dialog = reader.locator('[data-dialog-component="ticket-blocker-dialog"]');
    await expect(dialog).toBeVisible();
    await expect(page.locator('[data-dialog-component="ticket-blocker-dialog"]')).toHaveCount(1);
    await expect(dialog).toContainText(`Add Blocker to ${blocked.slug}`);
    await expect(dialog.locator('[name="blocker-target-search"]')).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);
    await expect(reader).toBeVisible();

    await add.click();
    await dialog.locator('[name="blocker-target-search"]').pressSequentially('upstream');
    const row = dialog
      .locator('[aria-label="Matching tickets"] [data-action="select-blocker"]')
      .filter({ hasText: blocker.slug });
    await expect(row).toHaveCount(1, { timeout: 20_000 });
    await row.click();
    for (const [label, width, height] of READER_WIDTHS) {
      await page.setViewportSize({ width, height });
      await expect(dialog.getByRole('button', { name: 'Add Blocker' })).toBeInViewport();
      await page.screenshot({ path: `target/visual-captures/hs2-15pe59-reader-blocker-dialog-${label}.png` });
    }
    await dialog.getByRole('button', { name: 'Add Blocker' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(
      page.locator('.app-toast').filter({ hasText: `${blocker.slug} now blocks ${blocked.slug}.` }),
    ).toBeVisible();
    const rows = section.locator(`[data-blocker-state][data-blocker-id="${blocker.id}"]`);
    await expect(rows).toHaveCount(1);
    await expect(rows).toContainText('Synthetic reader upstream dependency');
    expect((await server.request<StoredTicket>(`/tickets/${blocked.id}`)).blocked_by).toEqual([blocker.id]);
    for (const [label, width, height] of READER_WIDTHS) {
      await page.setViewportSize({ width, height });
      await section.scrollIntoViewIfNeeded();
      await expect(rows).toBeInViewport();
      await page.screenshot({ path: `target/visual-captures/hs2-15pe59-reader-blocked-by-${label}.png` });
    }

    // The blocker opens in a linked reader, whose own Blocked By refuses the reverse dependency.
    await rows.locator('[data-action="open-duplicate-target"]').click();
    const linked = page.getByRole('dialog', { name: `Read and edit ${blocker.slug}` });
    await expect(linked).toBeVisible({ timeout: 20_000 });
    await linked.getByRole('tab', { name: 'Info' }).click();
    const linkedSection = linked.getByRole('region', { name: 'Blocked By' });
    await expect(linkedSection).toBeVisible({ timeout: 20_000 });
    await linkedSection.getByRole('button', { name: 'Add Blocker…' }).click();
    const linkedDialog = linked.locator('[data-dialog-component="ticket-blocker-dialog"]');
    await expect(linkedDialog).toContainText(`Add Blocker to ${blocker.slug}`);
    await linkedDialog.locator('[name="blocker-target-search"]').pressSequentially('downstream');
    const reverse = linkedDialog
      .locator('[aria-label="Matching tickets"] [data-action="select-blocker"]')
      .filter({ hasText: blocked.slug });
    await expect(reverse).toHaveCount(1, { timeout: 20_000 });
    await reverse.click();
    await linkedDialog.getByRole('button', { name: 'Add Blocker' }).click();
    await expect(linkedDialog.getByRole('alert')).toContainText(
      `${blocked.slug} already depends on ${blocker.slug}, so it cannot also block ${blocker.slug} (dependency cycle)`,
    );
    await page.screenshot({ path: 'target/visual-captures/hs2-15pe59-linked-reader-cycle-1440.png' });
    expect((await server.request<StoredTicket>(`/tickets/${blocker.id}`)).blocked_by).toEqual([]);
    await linkedDialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(linkedDialog).toHaveCount(0);
    await expect(linked).toBeVisible();
    await linked
      .getByRole('button', { name: /^Close/ })
      .first()
      .click();
    await expect(linked).toHaveCount(0);

    // Remove the blocker from the workspace reader's row.
    await section.getByRole('button', { name: `Remove ${blocker.slug} as a blocker of ${blocked.slug}` }).click();
    await expect(
      page.locator('.app-toast').filter({ hasText: `${blocker.slug} no longer blocks ${blocked.slug}.` }),
    ).toBeVisible();
    await expect(section.locator('[data-blocker-state]')).toHaveCount(0);
    expect((await server.request<StoredTicket>(`/tickets/${blocked.id}`)).blocked_by).toEqual([]);
  } finally {
    await server.stop(page);
  }
});
