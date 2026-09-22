import { expect, type Page, test } from '@playwright/test';

import type { MigrationJob, MigrationProgress } from '../src/migration-progress';

async function mockHs1Project(page: Page, initialState: 'hs1' | 'imported' = 'hs1') {
  let job: MigrationJob | undefined;
  const listeners = new Set<() => void>();
  const publish = () => {
    if (job) job.revision++;
    for (const listener of listeners) listener();
    listeners.clear();
  };
  const state = {
    imported: initialState === 'imported',
    remote: initialState === 'imported',
    deleted: false,
    providerRequests: 0,
  };
  // Intercept the fixture API only: routing every Vite module adds thousands of browser/worker
  // round trips to each reload and can exhaust the scenario budget under parallel suite load.
  await page.route('**/__hotsheet/**', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: '/work/other' } });
    if (path === '/__hotsheet/projects/open' && request.postDataJSON().root === '/work/other')
      return route.fulfill({
        status: 201,
        json: {
          id: 'other',
          root: '/work/other',
          name: 'Other project',
          stores: ['/work/other.hs2'],
          apiPath: '/__hotsheet/project-api/other',
          needsTicketSetup: false,
        },
      });
    if (path === '/__hotsheet/projects/migration-jobs') {
      if (request.method() === 'POST') {
        const input = request.postDataJSON() as {
          kind: 'import' | 'backup';
          location: string;
          remote?: string;
          retryAttempt?: string;
        };
        expect(input.location).toBe('/work/legacy.hs2');
        if (!job || job.status !== 'running')
          job = {
            id: 'legacy-job',
            attempt: `attempt-${(job?.revision ?? 0) + 1}`,
            revision: (job?.revision ?? 0) + 1,
            projectId: 'legacy',
            root: '/work/legacy',
            store: input.location,
            kind: input.kind,
            remote: input.remote,
            sourceIdentity: 'legacy-db',
            ownerPid: 1,
            updatedAt: '',
            status: 'running',
            warnings: [],
            progress: {
              version: 1,
              phase: input.kind === 'import' ? 'copy_database' : 'push_start',
              ...(input.kind === 'import' ? { completed: 5, total: 10, unit: 'bytes' } : {}),
            },
          };
        return route.fulfill({ status: 202, json: job });
      }
      if (url.searchParams.get('root') !== '/work/legacy') return route.fulfill({ json: {} });
      if (job?.status === 'running' && Number(url.searchParams.get('after')) >= job.revision)
        await new Promise<void>((resolve) => listeners.add(resolve));
      return route.fulfill({ json: { job } }).catch(() => {});
    }
    if (path === '/__hotsheet/projects/open')
      return route.fulfill({
        status: 201,
        json: state.imported
          ? {
              id: 'legacy',
              root: '/work/legacy',
              name: 'Legacy project',
              stores: ['/work/legacy.hs2'],
              apiPath: '/__hotsheet/project-api/legacy',
              needsTicketSetup: false,
              needsHs1Migration: false,
              hs1ImportCompleted: true,
              hs1CleanupEligible: state.remote && !state.deleted,
              ...(state.deleted ? {} : { hs1DatabasePath: '/work/legacy/.hotsheet/db', hs1PostgresVersion: '17' }),
            }
          : {
              id: 'legacy',
              root: '/work/legacy',
              name: 'Legacy project',
              stores: [],
              apiPath: '/__hotsheet/project-api/legacy',
              needsTicketSetup: true,
              needsHs1Migration: true,
              hs1ImportCompleted: false,
              hs1CleanupEligible: false,
              hs1DatabasePath: '/work/legacy/.hotsheet/db',
              hs1PostgresVersion: '17',
            },
      });
    if (path.includes('/sources/') && request.method() === 'PUT')
      return route.fulfill({
        json: {
          id: 'legacy',
          root: '/work/legacy',
          alias: 'Legacy project',
          stores: ['/work/legacy.hs2'],
          sources: [{ connection_id: 'git-import', provider: 'git', locator: '/work/legacy.hs2' }],
          default_source: 'git-import',
        },
      });
    if (path === '/__hotsheet/projects/legacy/hs1-data' && request.method() === 'DELETE') {
      state.deleted = true;
      return route.fulfill({ json: { removed: ['db', 'attachments', 'settings.json'] } });
    }
    if (path.endsWith('/providers')) {
      state.providerRequests += 1;
      return route.fulfill({
        json: state.imported
          ? [
              {
                connection_id: 'git-import',
                provider: 'git',
                display_name: 'Imported Hot Sheet 1 tickets',
                locator: '/work/legacy.hs2',
                default: true,
                capabilities: {
                  create: true,
                  update: true,
                  atomic_batch: true,
                  notes: true,
                  attachments: true,
                  watch: true,
                  query_fields: [],
                },
              },
            ]
          : [],
      });
    }
    if (
      path.endsWith('/tickets') ||
      path.endsWith('/corrupt-tickets') ||
      path.endsWith('/connections') ||
      path.endsWith('/permissions') ||
      path.endsWith('/commands') ||
      path.endsWith('/command-runs') ||
      path.endsWith('/views') ||
      path.endsWith('/ai-tools') ||
      path.endsWith('/drive/sessions') ||
      path.endsWith('/terminals')
    )
      return route.fulfill({ json: [] });
    if (path.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll')) {
      // Establish the cursor (and answer zero-timeout catch-up) immediately. Idle polls stay
      // pending until navigation closes them, matching the server's blocking transport.
      if (!url.searchParams.has('since') || url.searchParams.get('timeout_ms') === '0')
        return route.fulfill({ json: { cursor: 0, events: [], overflow: false } });
      return;
    }
    return route.continue();
  });
  return {
    state,
    advance: (progress: MigrationProgress) => {
      if (job) job.progress = progress;
      publish();
    },
    failImport: () => {
      if (job) {
        job.status = 'failed';
        job.error = 'The destination is full. Free disk space and retry.';
      }
      publish();
    },
    releaseImport: () => {
      state.imported = true;
      if (job) {
        job.status = 'succeeded';
        job.progress = { version: 1, phase: 'complete' };
        job.result = {
          ticketStore: '/work/legacy.hs2',
          connectionId: 'git-import',
          tickets: 27,
          attachments: 4,
          toolsConfigured: true,
          stores: ['/work/legacy.hs2'],
        };
      }
      publish();
    },
    releaseRemote: () => {
      state.remote = true;
      if (job) {
        job.status = 'succeeded';
        job.progress = { version: 1, phase: 'complete' };
      }
      publish();
    },
  };
}

async function reloadRestoredProject(page: Page) {
  await page.reload({ waitUntil: 'domcontentloaded' });
  // A missing banner alone can pass while the page is still restoring its remembered project.
  // Wait for both the owning project and its completed ticket load before testing persistence.
  await expect(page.locator('.project-tab-bar')).toContainText('Legacy project');
  await expect(page.locator('.ticket-empty-state--project')).toBeVisible();
  await expect(page.locator('.app-error')).toHaveCount(0);
}

test('imports an HS1 project, then offers cleanup only after remote backup', async ({ page }, testInfo) => {
  test.setTimeout(60_000);
  const { state, releaseImport, releaseRemote, advance, failImport } = await mockHs1Project(page);
  await page.setViewportSize({ width: 1100, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const dialog = page.locator('[data-component="hs1-migration-dialog"]');
  await expect(dialog).toHaveJSProperty('open', true);
  await expect(dialog).toContainText('tickets, notes, attachments, settings, and AI-tool setup');
  await expect(dialog.getByRole('textbox', { name: 'Ticket repository folder' })).toHaveValue('/work/legacy.hs2');
  await dialog.evaluate(async (node) => {
    await Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished));
  });
  await page.screenshot({ path: testInfo.outputPath('hs1-import-wide.png'), fullPage: true, animations: 'disabled' });
  await page.setViewportSize({ width: 520, height: 720 });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('hs1-import-narrow.png'), fullPage: true, animations: 'disabled' });
  await dialog.getByRole('button', { name: 'Import project' }).click();
  await expect(dialog).toHaveCount(0);
  const jobBanner = page.locator('[data-component="hs1-job-banner"]');
  await expect(jobBanner).toContainText('Copying database');
  await expect(jobBanner.locator('wa-progress-bar')).toHaveJSProperty('value', 50);
  await page.screenshot({
    path: testInfo.outputPath('hs1-background-measured-narrow.png'),
    fullPage: true,
    animations: 'disabled',
  });
  advance({ version: 1, phase: 'open_database' });
  await expect(jobBanner.locator('wa-progress-bar')).toHaveAttribute('indeterminate', '');
  await page.setViewportSize({ width: 1100, height: 800 });
  await page.screenshot({
    path: testInfo.outputPath('hs1-background-unknown-wide.png'),
    fullPage: true,
    animations: 'disabled',
  });
  releaseImport();
  await expect(jobBanner).toContainText('27 tickets imported');
  await expect(page.locator('[data-ticket-source-setup-dialog]')).toHaveJSProperty('open', false);
  await jobBanner.getByRole('button', { name: 'Connect backup…' }).click();
  await expect(page.locator('[data-ticket-source-setup-dialog]')).toHaveJSProperty('open', true);
  expect(state.providerRequests).toBeGreaterThanOrEqual(2);
  const banner = page.locator('.hs1-cleanup-banner');
  await expect(banner).toHaveCount(0);
  await page.getByRole('textbox', { name: 'Remote URL' }).fill('git@example.com:team/legacy.hs2.git');
  await page.getByRole('button', { name: 'Connect & push' }).click();
  await expect(page.locator('[data-ticket-source-setup-dialog]')).toHaveJSProperty('open', false);
  await expect(jobBanner).toContainText('Connecting backup');
  await expect(jobBanner.locator('wa-progress-bar')).toHaveAttribute('indeterminate', '');
  await expect(banner).toHaveCount(0);
  await page.reload({ waitUntil: 'domcontentloaded' });
  await expect(jobBanner).toContainText('Connecting backup');
  await expect(banner).toHaveCount(0);
  failImport();
  await expect(jobBanner).toContainText('Backup needs attention');
  await jobBanner.getByRole('button', { name: 'Change backup…' }).click();
  await page.getByRole('textbox', { name: 'Remote URL' }).fill('git@example.com:team/corrected.hs2.git');
  await page.getByRole('button', { name: 'Connect & push' }).click();
  await expect(jobBanner).toContainText('Connecting backup');
  releaseRemote();
  await expect(banner).toBeVisible();
  expect(state.remote).toBe(true);
  await expect(banner).toHaveAttribute('data-component', 'state-banner');
  await expect(banner).toHaveAttribute('data-tone', 'success');
  await expect(banner).toHaveAttribute('role', 'status');
  await expect(banner).toHaveAttribute('aria-live', 'polite');
  await expect(banner).toContainText('safely backed up');
  state.remote = false;
  await page.reload({ waitUntil: 'domcontentloaded' });
  await expect(page.locator('[data-project-id="legacy"]')).toBeVisible();
  await expect(banner).toHaveCount(0);
  await expect(jobBanner).toContainText('Backup needs attention');
  await expect(jobBanner).toHaveAttribute('data-status', 'succeeded');
  await expect(jobBanner.getByRole('button', { name: 'Change backup…' })).toBeVisible();
  await jobBanner.screenshot({
    path: testInfo.outputPath('hs1-backup-reverification-wide.png'),
    animations: 'disabled',
  });
  await expect(page.locator('.project-tab__operation')).toHaveAttribute(
    'aria-label',
    'Legacy project: Migration needs attention',
  );
  await page.setViewportSize({ width: 520, height: 720 });
  await expect(page.locator('.project-tab-bar__operation')).toContainText('Attention');
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({
    path: testInfo.outputPath('hs1-backup-reverification-narrow.png'),
    fullPage: true,
    animations: 'disabled',
  });
  await jobBanner.getByRole('button', { name: 'Retry', exact: true }).click();
  await expect(jobBanner).toContainText('Connecting backup');
  await expect(banner).toHaveCount(0);
  releaseRemote();
  await expect(banner).toBeVisible();
  await expect(banner.locator('.kui-state-banner__action > .hs1-cleanup-banner__actions')).toHaveCount(1);
  await page.setViewportSize({ width: 2048, height: 900 });
  await banner.screenshot({ path: testInfo.outputPath('hs1-cleanup-banner-wide.png'), animations: 'disabled' });
  await page.setViewportSize({ width: 760, height: 720 });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await banner.screenshot({ path: testInfo.outputPath('hs1-cleanup-banner-narrow.png'), animations: 'disabled' });
  page.once('dialog', (prompt) => prompt.accept());
  await banner.getByRole('button', { name: 'Delete old files…' }).click();
  await expect(banner).toHaveCount(0);
  expect(state.deleted).toBe(true);
  await expect(jobBanner).toHaveCount(0);
  await expect(page.locator('.app-error')).toHaveCount(0);
  await expect(page.locator('.app-toast')).toContainText('Removed 3 old Hot Sheet 1 items');
  await page.setViewportSize({ width: 2048, height: 1280 });
  await page.screenshot({
    path: testInfo.outputPath('hs1-project-owned-cleanup-wide.png'),
    fullPage: true,
    animations: 'disabled',
  });
  await reloadRestoredProject(page);
  await expect(banner).toHaveCount(0);
  await expect(jobBanner).toHaveCount(0);
});

// Persistence has its own scenario so unrelated module navigations do not consume the import/
// backup/delete flow's timeout budget. Both tests still restore through the real application.
test('persists HS1 cleanup dismissal until the saved dismissal is cleared', async ({ page }) => {
  test.setTimeout(60_000);
  const { state } = await mockHs1Project(page, 'imported');
  await page.setViewportSize({ width: 760, height: 720 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  const banner = page.locator('.hs1-cleanup-banner');
  await expect(banner).toBeVisible();
  await banner.getByRole('button', { name: 'Dismiss' }).click();
  await expect(banner).toHaveCount(0);
  await reloadRestoredProject(page);
  await expect(banner).toHaveCount(0);
  await page.evaluate(() => {
    for (const key of Object.keys(localStorage))
      if (key.startsWith('hotsheet.workspace.hs1-cleanup-dismissed.')) localStorage.removeItem(key);
  });
  await reloadRestoredProject(page);
  await expect(banner).toBeVisible();
  expect(state.deleted).toBe(false);
});

test('keeps background import failure and retry owned by its project across navigation and reload', async ({
  page,
}, testInfo) => {
  test.setTimeout(60_000);
  const controls = await mockHs1Project(page);
  await page.setViewportSize({ width: 1100, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project', exact: true }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.locator('[data-component="hs1-migration-dialog"]').getByRole('button', { name: 'Import project' }).click();
  await expect(page.locator('[data-component="hs1-job-banner"]')).toContainText('Copying database');
  await page.getByRole('button', { name: 'Add project', exact: true }).click();
  await expect(page.locator('[data-project-id="other"]')).toHaveAttribute('data-selected', 'true');
  await expect(page.locator('[data-component="hs1-job-banner"]')).toHaveCount(0);
  await expect(page.locator('[data-project-id="legacy"] .project-tab__operation')).toBeVisible();
  controls.failImport();
  await expect(page.locator('[data-project-id="legacy"] .project-tab__operation')).toHaveAttribute(
    'data-state',
    'failed',
  );
  await expect(page.locator('[data-ticket-source-setup-dialog]')).toHaveJSProperty('open', false);
  await page.screenshot({
    path: testInfo.outputPath('hs1-background-other-project-wide.png'),
    fullPage: true,
    animations: 'disabled',
  });
  await page.reload({ waitUntil: 'domcontentloaded' });
  await expect(page.locator('[data-project-id="legacy"]')).toBeVisible();
  await page.locator('[data-project-id="legacy"]').click();
  const banner = page.locator('[data-component="hs1-job-banner"]');
  await expect(banner).toContainText('destination is full');
  await page.screenshot({
    path: testInfo.outputPath('hs1-background-retry-wide.png'),
    fullPage: true,
    animations: 'disabled',
  });
  await page.setViewportSize({ width: 520, height: 720 });
  await banner.getByRole('button', { name: 'Details', exact: true }).click();
  await expect(banner).toContainText('/work/legacy.hs2');
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({
    path: testInfo.outputPath('hs1-background-retry-details-narrow.png'),
    fullPage: true,
    animations: 'disabled',
  });
  await banner.getByRole('button', { name: 'Details', exact: true }).click();
  await page.setViewportSize({ width: 1100, height: 800 });
  await banner.getByRole('button', { name: 'Retry', exact: true }).click();
  await expect(banner).toHaveAttribute('data-status', 'running');
  await page.locator('[data-project-id="other"]').click();
  controls.releaseImport();
  await expect(page.locator('[data-project-id="legacy"] .project-tab__operation')).toHaveAttribute(
    'data-state',
    'succeeded',
  );
  await expect(page.locator('[data-project-id="other"]')).toHaveAttribute('data-selected', 'true');
  await expect(page.locator('[data-ticket-source-setup-dialog]')).toHaveJSProperty('open', false);
  await page.locator('[data-project-id="legacy"]').click();
  await expect(banner).toContainText('27 tickets imported');
  await expect(page.locator('.hs1-cleanup-banner')).toHaveCount(0);
  await page.screenshot({
    path: testInfo.outputPath('hs1-background-import-success-wide.png'),
    fullPage: true,
    animations: 'disabled',
  });
});
