import { execFile } from 'node:child_process';
import { writeFile } from 'node:fs/promises';
import { isAbsolute, join } from 'node:path';
import { promisify } from 'node:util';

import { expect, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import type { FullTicket, TicketRow } from '../src/api';
import { desktopApp } from './desktop-draft-files-fixture';
import { openMockedProject } from './open-mocked-project';

// HS2-ZR8B4N: a claim this machine took while its claim backend was unreachable (provisional) and a claim
// it lost to another machine show distinctly from an exclusive claim in the queue and the inspector, read
// by the real server from the store's machine-local records in the shape `claim_gate` writes them.
// Synthetic tickets and workers only.
test('shows provisional and lost claims distinctly from an exclusive claim (HS2-ZR8B4N)', async ({
  context,
  baseURL,
}) => {
  test.setTimeout(150_000);
  const app = await desktopApp(context, baseURL!);
  try {
    const create = (title: string) => app.server.request<FullTicket>('/tickets', 'POST', { title });
    const exclusive = await create('Synthetic exclusive claim');
    const provisional = await create('Synthetic provisional claim');
    const lost = await create('Synthetic lost claim');
    for (const [ticket, worker] of [
      [exclusive, 'synthetic-worker-a'],
      [provisional, 'synthetic-worker-a'],
      [lost, 'synthetic-worker-b'],
    ] as const)
      await app.server.request(`/tickets/${ticket.id}/claim`, 'POST', { worker, lease_minutes: 60 });
    const gitPath = async (name: string) => {
      const path = (
        await promisify(execFile)('git', ['rev-parse', '--git-path', name], {
          cwd: app.server.store,
          env: withoutGitRepositoryEnv(process.env),
        })
      ).stdout.trim();
      return isAbsolute(path) ? path : join(app.server.store, path);
    };
    await writeFile(
      await gitPath('hotsheet-provisional-claims.json'),
      JSON.stringify({ [provisional.id]: { claim_id: '01JSYNTHETICATTEMPT0000000', worker: 'synthetic-worker-a' } }),
    );
    await writeFile(
      await gitPath('hotsheet-claim-conflicts.json'),
      JSON.stringify([
        {
          ticket: lost.id,
          slug: lost.slug,
          worker: 'synthetic-worker-a',
          holder: 'synthetic-worker-b',
          detected_at: '2026-11-24T12:00:00Z',
        },
      ]),
    );
    const rows = await app.server.request<TicketRow[]>('/tickets');
    expect(rows.find((row) => row.id === provisional.id)?.claim_standing).toEqual({ kind: 'provisional' });

    const page = await app.launch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openMockedProject(page);
    const row = (slug: string) => page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${slug}"]`);
    const standing = (slug: string) => row(slug).locator('[data-claim-standing]');
    await expect(standing(provisional.slug)).toHaveAttribute('data-claim-standing', 'provisional');
    await expect(standing(provisional.slug)).toHaveText('Provisional');
    await expect(standing(lost.slug)).toHaveAttribute('data-claim-standing', 'lost');
    await expect(standing(lost.slug)).toHaveText('Claim lost');
    await expect(standing(exclusive.slug)).toHaveCount(0);
    await expect(row(provisional.slug).getByLabel(/under a provisional claim/)).toHaveCount(1);
    await expect(row(exclusive.slug).getByLabel(/is actively working on this ticket/)).toHaveCount(1);

    const notice = page.locator('#app-right-rail [data-component="live-claim-notice"]');
    await row(exclusive.slug).click();
    await expect(notice).not.toHaveAttribute('data-claim-standing');
    await expect(notice).toContainText('is working on this.');
    await row(lost.slug).click();
    await expect(notice).toHaveAttribute('data-claim-standing', 'lost');
    await expect(notice).toContainText('lost this claim.');
    await expect(notice).toContainText('synthetic-worker-b');
    await row(provisional.slug).click();
    await expect(notice).toHaveAttribute('data-claim-standing', 'provisional');
    await expect(notice).toContainText('is working on this provisionally.');
    await expect(notice).not.toContainText('is working on this.');

    const settle = () =>
      page.waitForFunction(() =>
        document
          .getAnimations()
          .every(
            (animation) =>
              animation.playState !== 'running' || animation.effect?.getComputedTiming().endTime === Infinity,
          ),
      );
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const [label, width] of [
        ['1440', 1440],
        ['1100', 1100],
        ['390', 390],
        ['1440-after-390', 1440],
      ] as const) {
        await page.setViewportSize({ width, height: 900 });
        await expect(standing(provisional.slug)).toBeVisible();
        await settle();
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
        await page.screenshot({ path: `target/visual-captures/hs2-zr8b4n-claims-${label}-${scheme}.png` });
      }
    }
    await page.emulateMedia({ colorScheme: 'light' });
    await row(lost.slug).click();
    await expect(notice).toHaveAttribute('data-claim-standing', 'lost');
    await page.screenshot({ path: 'target/visual-captures/hs2-zr8b4n-lost-1440-light.png' });

    // Released, the provisional claim's marker goes away with the claim.
    await app.server.request(`/tickets/${provisional.id}/release`, 'POST', { worker: 'synthetic-worker-a' });
    await expect(standing(provisional.slug)).toHaveCount(0);
  } finally {
    await app.stop();
  }
});
