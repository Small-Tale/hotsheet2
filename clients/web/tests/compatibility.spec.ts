import { expect, test } from '@playwright/test';

import type { CompatibilityAssessment } from '../src/compatibility';

async function openWithCompatibility(page: import('@playwright/test').Page, compatibility: CompatibilityAssessment) {
  await page.route('**/*', route => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/__hotsheet/projects/open') return route.fulfill({ status: 201, json: { id: 'skew', root: '/work/skew', name: 'skew', stores: ['/work/skew.hs2'], apiPath: '/__hotsheet/project-api/skew', compatibility } });
    if (path.endsWith('/providers')) return route.fulfill({ json: [] });
    if (path.endsWith('/tickets')) return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status')) return route.fulfill({ json: { ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0 } });
    return route.continue();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
}

test('does not offer unsafe restart for an old server', async ({ page }) => {
  await openWithCompatibility(page, { kind: 'server_too_old', detail: 'Server protocol 0–0 is older.', revisionMismatch: false, sourceStale: false, canRestartServer: false });
  const banner = page.locator('[data-component="connection-state-banner"]');
  await expect(banner).toContainText('Server update required');
  await expect(banner).toContainText('Safe restart is unavailable');
  await expect(banner.getByRole('button', { name: /Restart/ })).toHaveCount(0);

});

test('offers reload when the client is too old', async ({ page }) => {
  await openWithCompatibility(page, { kind: 'client_too_old', detail: 'Client protocol 1–1 is older.', revisionMismatch: false, sourceStale: false, canRestartServer: false });
  await expect(page.locator('[data-component="connection-state-banner"]')).toContainText('Client update required');
  await expect(page.getByRole('button', { name: 'Reload client' })).toBeVisible();
});

test('surfaces unavailable compatibility metadata without blocking project data', async ({ page }) => {
  await openWithCompatibility(page, { kind: 'unknown', detail: 'The server did not provide compatibility metadata.', revisionMismatch: false, sourceStale: false, canRestartServer: false });
  await expect(page.locator('[data-component="connection-state-banner"]')).toContainText('Server compatibility unknown');
  await expect(page.getByRole('heading', { name: 'Queue' })).toBeVisible();
});

test('surfaces a compatible detached server from another development revision', async ({ page }) => {
  await openWithCompatibility(page, { kind: 'compatible', revisionMismatch: true, sourceStale: false, canRestartServer: false });
  const banner = page.locator('[data-component="connection-state-banner"]');
  await expect(banner).toContainText('Different server build is running');
  await expect(banner).toContainText('protocol is compatible');
});

test('tells development users to restart a server built from older local source', async ({ page }) => {
  await openWithCompatibility(page, { kind: 'compatible', detail: 'The running server build differs from this checkout.', revisionMismatch: true, sourceStale: true, canRestartServer: false, clientProtocol: { min: 1, max: 1 }, clientRevision: 'source-sha256:client', server: { generation: 'hs2', application_version: '0.1.0', build_revision: 'source-sha256:old', source_revision: 'source-sha256:current', source_stale: true, protocol: { min: 1, max: 1 }, started_at: '2026-09-02T08:00:00Z' } });
  const banner = page.locator('[data-component="connection-state-banner"]');
  await expect(banner).toContainText('Different server build is running');
  await expect(banner).toContainText('Rebuild if needed, then restart it to pick up your latest build');
  await banner.getByRole('button', { name: 'View details' }).click();
  const dialog = page.getByRole('dialog', { name: 'Server build details' });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('term')).toHaveText(['Running server version', 'Running server build', 'Current server source', 'Client build', 'Protocol ranges', 'Server started']);
  await expect(dialog).toContainText('0.1.0');
  await expect(dialog).toContainText('source-sha256:old');
  await expect(dialog).toContainText('source-sha256:current');
  await expect(dialog).toContainText('source-sha256:client');
  await expect(dialog).toContainText('Client 1–1 · Server 1–1');
  await expect(dialog).toContainText('cargo build -p hotsheet-server');
  await page.screenshot({ path: '/private/tmp/hs2-f6e6py-server-details-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: '/private/tmp/hs2-f6e6py-server-details-narrow.png', fullPage: true });
  await dialog.getByRole('button', { name: 'Close' }).click();
  await expect(dialog).toHaveCount(0);
});
