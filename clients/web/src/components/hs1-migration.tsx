import './hs1-migration.css';
import '@awesome.me/webawesome/dist/components/progress-bar/progress-bar.js';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { ArchiveRestore, Database, Trash2 } from 'lucide';

import { migrationCounter, type MigrationJob, migrationPercent, migrationPhaseLabel } from '../migration-progress';

export interface Hs1MigrationDialogProps {
  projectName: string;
  projectRoot: string;
  sourcePath: string;
  databasePath: string;
  postgresVersion?: string;
  defaultStore: string;
  open: boolean;
  busy: boolean;
  error?: string;
}

export function Hs1MigrationDialog({
  projectName,
  projectRoot,
  sourcePath,
  databasePath,
  postgresVersion,
  defaultStore,
  open,
  busy,
  error,
}: Hs1MigrationDialogProps) {
  return (
    <wa-dialog data-component="hs1-migration-dialog" label="Import Hot Sheet 1 project" open={open}>
      <form class="hs1-migration-dialog" data-action="import-hs1-project">
        <div class="hs1-migration-dialog__intro">
          <LucideIcon icon={ArchiveRestore} name="archive-restore" />
          <div>
            <strong>Hot Sheet 1 data found in {projectName}</strong>
            <p>
              Import its tickets, notes, attachments, settings, and AI-tool setup into Hot Sheet 2. Your old data stays
              in place until the new ticket repository is backed up.
            </p>
          </div>
        </div>
        <ValueTable label="Detected Hot Sheet 1 source">
          <ValueTableRow label="Project" value={<code>{projectRoot}</code>} />
          <ValueTableRow label="Data folder" value={<code>{sourcePath}</code>} />
          <ValueTableRow label="Database" value={<code>{databasePath}</code>} />
          {postgresVersion ? <ValueTableRow label="PostgreSQL" value={postgresVersion} /> : <></>}
        </ValueTable>
        <div class="hs1-migration-dialog__destination">
          <wa-input
            name="hs1-ticket-store"
            label="Ticket repository folder"
            value={defaultStore}
            required
            disabled={busy}
          ></wa-input>
          <wa-button appearance="outlined" type="button" data-action="browse-hs1-ticket-store" disabled={busy}>
            Choose…
          </wa-button>
        </div>
        {busy && (
          <div class="hs1-migration-dialog__progress" role="status">
            <wa-progress-bar indeterminate label="Importing Hot Sheet 1 project"></wa-progress-bar>
            <p>Exporting and importing tickets, copying attachments, and configuring detected AI tools…</p>
          </div>
        )}
        {error && (
          <p class="hs1-migration-dialog__error" role="alert">
            {error}
          </p>
        )}
        <footer>
          <wa-button appearance="plain" type="button" data-action="dismiss-hs1-migration" disabled={busy}>
            Not now
          </wa-button>
          <wa-button appearance="accent" type="submit" disabled={busy}>
            {busy ? 'Importing…' : 'Import project'}
          </wa-button>
        </footer>
      </form>
    </wa-dialog>
  );
}

export function Hs1MigrationBanner({ databasePath }: { databasePath: string }) {
  return (
    <StateBanner
      title="Hot Sheet 1 data is available to import"
      detail={databasePath}
      tone="info"
      urgency="status"
      className="hs1-migration-banner"
      icon={<LucideIcon icon={Database} name="database" />}
      action={
        <button type="button" data-action="open-hs1-migration">
          Import…
        </button>
      }
    />
  );
}
export function Hs1CleanupBanner() {
  return (
    <StateBanner
      title="Hot Sheet 1 import is safely backed up"
      detail="The old local Hot Sheet 1 files can now be removed."
      tone="success"
      urgency="status"
      className="hs1-cleanup-banner"
      icon={<LucideIcon icon={Trash2} name="trash-2" />}
      action={
        <div class="hs1-cleanup-banner__actions">
          <button type="button" data-action="dismiss-hs1-cleanup">
            Dismiss
          </button>
          <button type="button" data-action="remove-hs1-data">
            Delete old files…
          </button>
        </div>
      }
    />
  );
}

export function Hs1JobBanner({
  job,
  details = false,
  connectionError,
  backupVerified = true,
}: {
  job: MigrationJob;
  details?: boolean;
  connectionError?: string;
  backupVerified?: boolean;
}) {
  const running = job.status === 'running',
    backupUnverified = job.kind === 'backup' && job.status === 'succeeded' && !backupVerified,
    failed = job.status === 'failed' || job.status === 'interrupted' || backupUnverified,
    percent = migrationPercent(job.progress),
    phase = migrationPhaseLabel(job.progress),
    title = failed
      ? `${job.kind === 'import' ? 'Import' : 'Backup'} needs attention`
      : running
        ? phase
        : job.kind === 'backup'
          ? 'Hot Sheet 1 backup complete'
          : 'Hot Sheet 1 import complete';
  return (
    <div class="hs1-job" data-component="hs1-job-banner" data-status={job.status} data-attempt={job.attempt}>
      <StateBanner
        title={title}
        detail={
          failed
            ? backupUnverified
              ? 'The saved backup no longer matches this repository. Retry verification or choose its current remote.'
              : job.error
            : running
              ? migrationCounter(job.progress) || 'Working in the background. Other projects remain available.'
              : job.kind === 'backup'
                ? 'Backup finished. Refresh project status before removing the old files.'
                : `${job.result?.tickets ?? 0} tickets imported. Connect a backup before removing the old files.`
        }
        tone={failed ? 'warning' : running ? 'info' : 'success'}
        urgency="status"
        icon={<LucideIcon icon={ArchiveRestore} name="archive-restore" />}
        action={
          <div class="hs1-cleanup-banner__actions">
            <button type="button" data-action="migration-job-details" aria-expanded={String(details)}>
              Details
            </button>
            {(failed || (!running && job.warnings.length > 0)) && (
              <button type="button" data-action="retry-migration-job">
                Retry
              </button>
            )}
            {!running && ((!failed && job.kind === 'import') || (failed && job.kind === 'backup')) && (
              <button type="button" data-action="backup-migration-job">
                {job.kind === 'backup' ? 'Change backup…' : 'Connect backup…'}
              </button>
            )}
          </div>
        }
      />
      {running && (
        <wa-progress-bar
          class="hs1-job__progress"
          label={`${phase} progress`}
          value={String(percent ?? 0)}
          indeterminate={percent === undefined && job.progress.total !== 0}
        ></wa-progress-bar>
      )}
      {connectionError && (
        <p class="hs1-job__connection">
          {connectionError}{' '}
          <button type="button" data-action="reconnect-migration-job">
            Reconnect
          </button>
        </p>
      )}
      {details && (
        <div class="hs1-job__details">
          <p>
            {job.kind === 'import' ? 'Import destination' : 'Backup repository'}: <code>{job.store}</code>
          </p>
          <p>Progress describes the current phase. Completion includes verification and project setup.</p>
          {job.error && <p>{job.error}</p>}
          {job.warnings.map((warning) => (
            <p>{warning}</p>
          ))}
        </div>
      )}
    </div>
  );
}
