import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

import { expect, type Page, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { realTicketServer } from './real-ticket-server';

const execute = promisify(execFile);
type Server = Awaited<ReturnType<typeof realTicketServer>>;
interface CreatedTicket {
  id: string;
  slug: string;
  connection_id: string;
  native_id: string;
}
interface StoredTicket {
  follow_ups?: { project_id: string; connection_id: string; native_id: string }[];
}

/** A real server whose checkout root is a Git repository, so Code Review can read its history. */
async function followUpServer() {
  return realTicketServer({
    seed: async ({ root, env }) => {
      const git = (...args: string[]) => execute('git', ['-C', root, ...args], { env: withoutGitRepositoryEnv(env) });
      await git('init', '--quiet', '--initial-branch=main');
      await git('commit', '--quiet', '--allow-empty', '-m', 'Synthetic initial commit');
    },
  });
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
      json: { id: server.checkoutId, root: server.root, name: 'Follow-ups', stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url()),
      path = incoming.pathname.slice(apiPath.length);
    try {
      const response = await route.fetch({
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
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
}

async function openCodeReview(page: Page, slug: string) {
  await page.locator(`[data-ticket-slug="${slug}"]`).click();
  const inspector = page.locator('#app-right-rail');
  await inspector.getByRole('tab', { name: 'Code Review' }).click();
  const section = inspector.getByRole('region', { name: 'Follow-Up Tickets' });
  await expect(section).toBeVisible({ timeout: 20_000 });
  return { inspector, section, link: section.getByRole('button', { name: 'Link Existing Ticket as Follow-Up…' }) };
}

/** Search the link dialog for `query` and choose the row for `slug`. */
async function chooseTarget(page: Page, query: string, slug: string) {
  const dialog = page.locator('[data-dialog-component="ticket-follow-up-link-dialog"]');
  await expect(dialog).toBeVisible();
  const field = dialog.locator('[name="follow-up-link-target-search"]');
  await expect(field).toBeFocused();
  await field.pressSequentially(query);
  const row = dialog
    .locator('[aria-label="Matching tickets"] [data-action="select-follow-up-link-target"]')
    .filter({ hasText: slug });
  await expect(row).toHaveCount(1, { timeout: 20_000 });
  await row.click();
  return dialog;
}

test('links an existing ticket as a follow-up from the Inspector, idempotently (HS2-MCGCPQ)', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await followUpServer();
  try {
    const source = await server.request<CreatedTicket>('/tickets', 'POST', {
        title: 'Synthetic follow-up source',
        category: 'feature',
      }),
      target = await server.request<CreatedTicket>('/tickets', 'POST', {
        title: 'Synthetic existing follow-up target',
        category: 'task',
      });
    await page.setViewportSize({ width: 1440, height: 900 });
    await openProject(page, server);
    const { section, link } = await openCodeReview(page, source.slug);
    // No follow-ups yet: the header and its enabled action are still there.
    await expect(section.locator('[data-action="open-follow-up-target"]')).toHaveCount(0);
    await expect(link).toBeEnabled();
    await page.screenshot({ path: 'target/visual-captures/hs2-mcgcpq-code-review-empty-1440.png' });

    await link.click();
    // The source itself never appears as a candidate.
    const dialog = page.locator('[data-dialog-component="ticket-follow-up-link-dialog"]');
    await expect(dialog).toContainText(`Link Follow-Up to ${source.slug}`);
    await dialog.locator('[name="follow-up-link-target-search"]').pressSequentially('Synthetic');
    await expect(dialog.locator('[data-action="select-follow-up-link-target"]')).toHaveCount(1, { timeout: 20_000 });
    await expect(dialog.locator('[data-action="select-follow-up-link-target"]')).toContainText(target.slug);
    await page.screenshot({ path: 'target/visual-captures/hs2-mcgcpq-link-dialog-results-1440.png' });
    await dialog.locator('[data-action="select-follow-up-link-target"]').click();
    await expect(dialog.locator('[data-action="clear-follow-up-link-target"]')).toBeVisible();
    await page.screenshot({ path: 'target/visual-captures/hs2-mcgcpq-link-dialog-selected-1440.png' });
    await dialog.getByRole('button', { name: 'Link Follow-Up' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(
      page.locator('.app-toast').filter({ hasText: `${target.slug} is now a follow-up of ${source.slug}.` }),
    ).toBeVisible();
    const rows = section.locator('[data-action="open-follow-up-target"]');
    await expect(rows).toHaveCount(1, { timeout: 20_000 });
    await expect(rows).toContainText(target.slug);
    await expect(rows).toContainText('Synthetic existing follow-up target');
    await page.screenshot({ path: 'target/visual-captures/hs2-mcgcpq-code-review-linked-1440.png' });

    // Linking the same ticket again says so and records nothing new.
    await link.click();
    const again = await chooseTarget(page, target.slug, target.slug);
    await expect(again).toContainText(`${target.slug} is already a follow-up of ${source.slug}.`);
    await again.getByRole('button', { name: 'Link Follow-Up' }).click();
    await expect(
      page.locator('.app-toast').filter({ hasText: `${target.slug} was already a follow-up of ${source.slug}.` }),
    ).toBeVisible();
    await expect(rows).toHaveCount(1);
    const stored = await server.request<StoredTicket>(`/tickets/${source.id}`);
    expect(stored.follow_ups).toEqual([
      { project_id: server.checkoutId, connection_id: target.connection_id, native_id: target.native_id },
    ]);

    // Cancel and Escape dismiss without writing.
    await link.click();
    await page
      .locator('[data-dialog-component="ticket-follow-up-link-dialog"]')
      .getByRole('button', { name: 'Cancel' })
      .click();
    await expect(page.locator('[data-dialog-component="ticket-follow-up-link-dialog"]')).toHaveCount(0);
    await expect(link).toBeEnabled();
  } finally {
    await server.stop();
  }
});

test('disables the follow-up link with its reason for a provider without relations (HS2-MCGCPQ)', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await followUpServer();
  try {
    const source = await server.request<CreatedTicket>('/tickets', 'POST', { title: 'Synthetic provider source' });
    await page.setViewportSize({ width: 1440, height: 900 });
    // Present the same source as a provider without follow-up relations; every other route is real.
    await openProject(page, server, (body) =>
      (body as Record<string, unknown>[]).map((item) => ({
        ...item,
        provider: 'github',
        display_name: 'GitHub Issues',
      })),
    );
    const { link } = await openCodeReview(page, source.slug);
    await expect(link).toBeDisabled();
    await expect(link).toHaveAttribute(
      'title',
      'GitHub Issues tickets cannot record follow-up links. Only Hot Sheet Git tickets can.',
    );
    await page.screenshot({ path: 'target/visual-captures/hs2-mcgcpq-code-review-unsupported-1440.png' });
  } finally {
    await server.stop();
  }
});
