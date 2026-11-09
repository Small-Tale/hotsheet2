import { readdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { expect, test } from '@playwright/test';

import type { FullTicket, TicketRow } from '../src/api';
import { desktopApp } from './desktop-draft-files-fixture';

// HS2-DQJPW0: a live claim written by a separate Hot Sheet host and seen through the synced
// store is marked remote in the queue and the inspector; this server's own claims keep their
// worker names. Synthetic tickets, workers and host ids only.
const OTHER_HOST = '01ARZ3NDEKTSV4RRFFQ69G5FZZ';

async function ticketFile(store: string, id: string): Promise<string> {
  for (const shard of await readdir(join(store, 'tickets'))) {
    const files = await readdir(join(store, 'tickets', shard)).catch((): string[] => []);
    if (files.includes(`${id}.md`)) return join(store, 'tickets', shard, `${id}.md`);
  }
  throw new Error(`no ticket file for ${id}`);
}

test('marks a claim from another Hot Sheet host as remote and keeps local workers (HS2-DQJPW0)', async ({
  context,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const app = await desktopApp(context, baseURL!);
  try {
    const create = (title: string) => app.server.request<FullTicket>('/tickets', 'POST', { title });
    const local = await create('Synthetic local claim');
    const remote = await create('Synthetic remote claim');
    for (const [ticket, worker] of [
      [local, 'synthetic-local-worker'],
      [remote, 'synthetic-remote-worker'],
    ] as const)
      await app.server.request(`/tickets/${ticket.id}/claim`, 'POST', { worker, lease_minutes: 60 });
    const rows = async () => app.server.request<TicketRow[]>('/tickets');
    expect((await rows()).find((row) => row.id === local.id)?.claim_origin).toBe('local');

    // The other host's claim arrives through the synced store: only the claim host differs.
    const path = await ticketFile(app.server.store, remote.id);
    const text = await readFile(path, 'utf8');
    expect(text).toMatch(/^claim_host: \S+$/m);
    await writeFile(path, text.replace(/^claim_host: \S+$/m, `claim_host: ${OTHER_HOST}`));
    await expect
      .poll(async () => (await rows()).find((row) => row.id === remote.id)?.claim_origin, { timeout: 20_000 })
      .toBe('remote');

    const page = await app.launch();
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    const row = (slug: string) => page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${slug}"]`);
    const owner = (slug: string) => row(slug).locator('[data-claim-origin="remote"]');
    await expect(owner(remote.slug)).toHaveText('Remote');
    await expect(owner(remote.slug).locator('[data-lucide="cloud"]')).toHaveCount(1);
    await expect(row(remote.slug)).not.toContainText('synthetic-remote-worker');
    await expect(row(local.slug).locator('.ticket-list-row__owner')).toHaveText('synthetic-local-worker');
    await expect(owner(local.slug)).toHaveCount(0);

    await row(remote.slug).click();
    const notice = page.locator('#app-right-rail [data-component="live-claim-notice"]');
    await expect(notice).toHaveAttribute('title', 'Remote server is actively working on this ticket');
    for (const [label, width] of [
      ['wide', 1280],
      ['medium', 900],
      ['narrow', 390],
      ['wide-return', 1280],
    ] as const) {
      await page.setViewportSize({ width, height: 844 });
      await expect(owner(remote.slug)).toBeVisible();
      // Let rail transitions settle so the capture shows the resting layout.
      await page.waitForFunction(() =>
        document
          .getAnimations()
          .every(
            (animation) =>
              animation.playState !== 'running' || animation.effect?.getComputedTiming().endTime === Infinity,
          ),
      );
      await page.screenshot({ path: test.info().outputPath(`hs2-dqjpw0-remote-claim-${label}.png`) });
    }

    // The remote claim is released on its host and synced back: the marker goes away.
    await app.server.request(`/tickets/${remote.id}/release`, 'POST', { worker: 'synthetic-remote-worker' });
    await expect(owner(remote.slug)).toHaveCount(0);
    expect((await rows()).find((row) => row.id === remote.id)?.claim_origin).toBeUndefined();
  } finally {
    await app.stop();
  }
});
