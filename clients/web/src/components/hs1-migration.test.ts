import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import type { MigrationJob } from '../migration-progress';
import { Hs1CleanupBanner, Hs1JobBanner, Hs1MigrationBanner, Hs1MigrationDialog } from './hs1-migration';

describe('HS1 migration presentation', () => {
  it('uses canonical dialog spacing and StateBanner customization tokens', () => {
    const css = readFileSync(new URL('./hs1-migration.css', import.meta.url), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).toMatchSource(/\.hs1-migration-dialog \{[^}]*gap:var\(--kui-space-l\)/);
    expect(css).toMatchSource(/__intro \{[^}]*gap:var\(--kui-space-m\)/);
    expect(css).toMatchSource(/__destination \{[^}]*gap:var\(--kui-space-xs\)/);
    expect(css).toMatchSource(
      /\.hs1-cleanup-banner,\.hs1-migration-banner \{[^}]*--kui-layout-item-gap:var\(--kui-space-m\)[^}]*--kui-layout-item-padding:var\(--kui-space-xs\) var\(--kui-space-m\)/,
    );
    expect(css).toMatchSource(
      /\.hs1-cleanup-banner,\.hs1-migration-banner \{[^}]*--kui-state-banner-copy-gap:var\(--kui-space-2xs\)/,
    );
    expect(css).not.toContain('.kui-state-banner__copy {');
    expect(css).not.toMatch(/\.kui-state-banner__action \{/);
    expect(css).not.toContain('.kui-state-banner {');
    // The app strip wraps the Kerf StateBanner (KUI-L022): it owns the bottom rule and the grid
    // that stretches the banner; the banner root carries no app class.
    expect(css).toMatchSource(/\.hs1-cleanup-banner,\.hs1-migration-banner \{[^}]*display:grid/);
    expect(css).toMatchSource(
      /\.hs1-cleanup-banner,\.hs1-migration-banner \{[^}]*--kui-state-banner-border:transparent/,
    );
    expect(css).not.toMatch(/\.hs1-cleanup-banner,\.hs1-migration-banner \{[^}]*border-width/);
    // The dialog styles its own path elements, never ValueTable's anatomy (Kerf doctor KUI-L019).
    expect(css).toMatchSource(/\.hs1-migration-dialog__path \{[^}]*font-size:var\(--wa-font-size-xs\)/);
    expect(css).not.toContain('.kui-value-table__row');
    expect(css).not.toContain('.kui-state-banner__');
  });

  it('asks only where to store tickets and describes the complete import', () => {
    const markup = String(
      Hs1MigrationDialog({
        projectName: 'Demo',
        projectRoot: '/work/demo',
        sourcePath: '/work/demo/.hotsheet',
        databasePath: '/work/demo/.hotsheet/db',
        postgresVersion: '17',
        defaultStore: '/work/demo.hs2',
        open: true,
        busy: false,
      }),
    );
    expect(markup).toContain('Hot Sheet 1 data found in Demo');
    expect(markup).toContain('tickets, notes, attachments, settings, and AI-tool setup');
    expect(markup).toContain('name="hs1-ticket-store"');
    expect(markup).toContain('/work/demo/.hotsheet/db');
    expect(markup).toContain('PostgreSQL');
    expect(markup.match(/class="kui-value-table__row"/g)).toHaveLength(4);
    expect(markup).not.toContain('provider');
  });
  it('shows honest indeterminate progress for the multi-stage import', () => {
    const markup = String(
      Hs1MigrationDialog({
        projectName: 'Demo',
        projectRoot: '/work/demo',
        sourcePath: '/work/demo/.hotsheet',
        databasePath: '/work/demo/.hotsheet/db',
        defaultStore: '/work/demo.hs2',
        open: true,
        busy: true,
      }),
    );
    expect(markup).toContain('wa-progress-bar indeterminate');
    expect(markup).toContain('label="Importing Hot Sheet 1 project"');
    expect(markup).toContain('copying attachments');
    expect(markup).toContain('disabled');
    expect(markup.match(/class="kui-value-table__row"/g)).toHaveLength(3);
  });
  it('keeps a dismissed import available from a polite informational StateBanner', () => {
    const markup = String(Hs1MigrationBanner({ databasePath: '/work/demo/.hotsheet/db' }));
    expect(markup).toContain('data-component="state-banner"');
    expect(markup).toContain('<div class="hs1-migration-banner"><section class="kui-state-banner"');
    expect(markup).toContain('data-tone="info"');
    expect(markup).toContain('role="status"');
    expect(markup).toContain('aria-live="polite"');
    expect(markup).toContain('/work/demo/.hotsheet/db');
    expect(markup).toContain('data-action="open-hs1-migration"');
  });
  it('offers cleanup or dismissal from one StateBanner action slot', () => {
    const markup = String(Hs1CleanupBanner());
    expect(markup).toContain('data-component="state-banner"');
    expect(markup).toContain('<div class="hs1-cleanup-banner"><section class="kui-state-banner"');
    expect(markup).toContain('data-tone="success"');
    expect(markup).toContain('safely backed up');
    // Kerf stacks the copy and places the actions below it (beta.60 props) instead of app grid rules.
    expect(markup).toContain('data-copy-layout="stacked" data-action-placement="below"');
    expect(markup).toContain('class="kui-state-banner__action"><div class="hs1-cleanup-banner__actions"');
    expect(markup).toContain('data-action="remove-hs1-data"');
    expect(markup).toContain('data-action="dismiss-hs1-cleanup"');
  });
});

it('transitions measured, unknown, failed and warned success with explicit owner actions', () => {
  const job: MigrationJob = {
    id: 'job',
    attempt: 'attempt',
    revision: 1,
    projectId: 'project',
    root: '/project',
    store: '/store',
    sourceIdentity: 'db',
    kind: 'import',
    status: 'running',
    progress: { version: 1, phase: 'copy_database', completed: 3, total: 4, unit: 'bytes' },
    warnings: [],
    ownerPid: 1,
    updatedAt: '',
  };
  let markup = String(Hs1JobBanner({ job }));
  expect(markup).toContain('value="75"');
  expect(markup).toContain('3 B of 4 B');
  expect(markup).not.toContain('retry-migration-job');
  markup = String(Hs1JobBanner({ job: { ...job, progress: { version: 1, phase: 'open_database' } } }));
  expect(markup).toContain('indeterminate');
  markup = String(
    Hs1JobBanner({ job: { ...job, status: 'failed', error: 'Disk full' }, details: true, connectionError: 'Offline' }),
  );
  expect(markup).toContain('Disk full');
  expect(markup).toContain('retry-migration-job');
  expect(markup).toContain('reconnect-migration-job');
  expect(markup).not.toContain('wa-progress-bar');
  markup = String(
    Hs1JobBanner({ job: { ...job, status: 'succeeded', warnings: ['Tool setup needs attention'] }, details: true }),
  );
  expect(markup).toContain('Tool setup needs attention');
  expect(markup).toContain('retry-migration-job');
  expect(markup).toContain('backup-migration-job');
  expect(markup).not.toContain('remove-hs1-data');
  markup = String(Hs1JobBanner({ job: { ...job, kind: 'backup', status: 'succeeded' }, backupVerified: false }));
  expect(markup).toContain('data-status="succeeded"'); // History remains truthful.
  expect(markup).toContain('Backup needs attention');
  expect(markup).toContain('no longer matches');
  expect(markup).toContain('retry-migration-job');
  expect(markup).toContain('Change backup');
  expect(markup).not.toContain('remove-hs1-data');
});
