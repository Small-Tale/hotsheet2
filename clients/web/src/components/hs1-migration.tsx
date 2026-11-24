import './hs1-migration.css';
import '@awesome.me/webawesome/dist/components/progress-bar/progress-bar.js';

import { Button } from '@kerfjs/ui/button';
import { uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListInsetText } from '@kerfjs/ui/list-inset-text';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { ArchiveRestore, Database, Trash2 } from 'lucide';

import { hostActionsAvailableHere, hostOnlyDisabledReason } from '../host-only-actions';
import { PROJECT_LIFECYCLE_ACTIONS, PROJECT_LIFECYCLE_TARGETS } from '../interaction-attrs/project-lifecycle';
import { migrationCounter, type MigrationJob, migrationPercent, migrationPhaseLabel } from '../migration-progress';
import { ShellBanner } from './shell-banner';

/** A detected filesystem path shown as a compact code value inside the source ValueTable. */
function SourcePath({ path }: { path: string }) {
  return (
    <Text variant="span" size="compact" wrap="anywhere">
      <code>{path}</code>
    </Text>
  );
}

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
  /** False on a remote client: host-only controls render disabled with an explanation (HS2-TYS5SS). */
  hostActionsAvailable?: boolean;
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
  hostActionsAvailable = hostActionsAvailableHere(),
}: Hs1MigrationDialogProps) {
  return (
    <wa-dialog
      data-key="hs1-migration"
      {...PROJECT_LIFECYCLE_TARGETS.hs1MigrationDialog.attrs}
      label="Import Hot Sheet 1 project"
      open={open}
      data-controlled-open={String(open)}
    >
      <form {...PROJECT_LIFECYCLE_ACTIONS.importHs1Project.attrs}>
        <List gap="l">
          <Row gap="m" vAlign="top">
            <LucideIcon icon={ArchiveRestore} name="archive-restore" size="l" color={uiColor('brand-on-quiet')} />
            <List gap="2xs" flex>
              <Text variant="span">
                <strong>Hot Sheet 1 data found in {projectName}</strong>
              </Text>
              <Text tone="quiet" flush>
                Import its tickets, notes, attachments, settings, and AI-tool setup into Hot Sheet 2. Your old data
                stays in place until the new ticket repository is backed up.
              </Text>
            </List>
          </Row>
          <ValueTable label="Detected Hot Sheet 1 source">
            <ValueTableRow label="Project" value={<SourcePath path={projectRoot} />} />
            <ValueTableRow label="Data folder" value={<SourcePath path={sourcePath} />} />
            <ValueTableRow label="Database" value={<SourcePath path={databasePath} />} />
            {postgresVersion ? <ValueTableRow label="PostgreSQL" value={postgresVersion} /> : <></>}
          </ValueTable>
          <Row gap="xs" vAlign="bottom" wrap>
            <List flex>
              <TextField
                name="hs1-ticket-store"
                label="Ticket Repository Folder"
                value={defaultStore}
                required
                disabled={busy}
              />
            </List>
            <Button
              emphasis="secondary"
              type="button"
              action={PROJECT_LIFECYCLE_ACTIONS.browseHs1TicketStore.value}
              disabled={busy || !hostActionsAvailable}
              title={hostOnlyDisabledReason('choose-folder', hostActionsAvailable)}
            >
              Choose…
            </Button>
          </Row>
          {busy && (
            <div role="status">
              <List gap="xs">
                <wa-progress-bar indeterminate label="Importing Hot Sheet 1 project" />
                <Text size="compact" color={uiColor('brand-on-quiet')} flush>
                  Exporting and importing tickets, copying attachments, and configuring detected AI tools…
                </Text>
              </List>
            </div>
          )}
          {error ? (
            <Text size="compact" tone="danger" role="alert" flush>
              {error}
            </Text>
          ) : (
            <></>
          )}
          <Row hAlign="right" gap="xs">
            <Button
              emphasis="plain"
              type="button"
              action={PROJECT_LIFECYCLE_ACTIONS.dismissHs1Migration.value}
              disabled={busy}
            >
              Not Now
            </Button>
            <Button emphasis="primary" type="submit" disabled={busy}>
              {busy ? 'Importing…' : 'Import project'}
            </Button>
          </Row>
        </List>
      </form>
    </wa-dialog>
  );
}

export function Hs1MigrationBanner({ databasePath }: { databasePath: string }) {
  return (
    <ShellBanner
      component="hs1-migration-banner"
      title="Hot Sheet 1 data is available to import"
      detail={databasePath}
      tone="info"
      icon={<LucideIcon icon={Database} name="database" />}
      action={
        <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.openHs1Migration.attrs}>
          Import…
        </button>
      }
    />
  );
}
export function Hs1CleanupBanner() {
  return (
    <ShellBanner
      component="hs1-cleanup-banner"
      title="Hot Sheet 1 import is safely backed up"
      detail="The old local Hot Sheet 1 files can now be removed."
      tone="success"
      actionPlacement="below"
      icon={<LucideIcon icon={Trash2} name="trash-2" />}
      action={
        <>
          <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissHs1Cleanup.attrs}>
            Dismiss
          </button>
          <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.removeHs1Data.attrs}>
            Delete Old Files…
          </button>
        </>
      }
    />
  );
}

export function Hs1RegistrationRepairBanner() {
  return (
    <ShellBanner
      component="hs1-cleanup-banner"
      title="Hot Sheet 1 may reopen this imported project"
      detail="An old saved-project or channel registration remains after the local database was removed."
      tone="warning"
      actionPlacement="below"
      icon={<LucideIcon icon={Database} name="database" />}
      action={
        <>
          <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissHs1Cleanup.attrs}>
            Dismiss
          </button>
          <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.removeHs1Data.attrs}>
            Repair Old App Registration…
          </button>
        </>
      }
    />
  );
}

export function Hs1JobBanner({
  job,
  connectionError,
  backupVerified = true,
}: {
  job: MigrationJob;
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
    <div data-component="hs1-job-banner" data-status={job.status} data-attempt={job.attempt}>
      {/* Inset like every application banner (ShellBanner): 8px around and between (HS2-GXJTSE). */}
      <List gap="2xs" controlInsets="t">
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
          copyLayout="stacked"
          icon={<LucideIcon icon={ArchiveRestore} name="archive-restore" />}
          action={
            <>
              <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.migrationJobDetails.attrs} aria-haspopup="dialog">
                Details
              </button>
              {(failed || (!running && job.warnings.length > 0)) && (
                <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.retryMigrationJob.attrs}>
                  Retry
                </button>
              )}
              {!running && ((!failed && job.kind === 'import') || (failed && job.kind === 'backup')) && (
                <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.backupMigrationJob.attrs}>
                  {job.kind === 'backup' ? 'Change backup…' : 'Connect backup…'}
                </button>
              )}
            </>
          }
        />
        <List gap="xs" textInsets="rl">
          {running && (
            <wa-progress-bar
              class="hs1-job__progress"
              label={`${phase} progress`}
              value={String(percent ?? 0)}
              indeterminate={percent === undefined && job.progress.total !== 0}
            />
          )}
          {connectionError ? (
            <Text size="compact" wrap="anywhere" flush>
              {connectionError}{' '}
              <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.reconnectMigrationJob.attrs}>
                Reconnect
              </button>
            </Text>
          ) : (
            <></>
          )}
        </List>
      </List>
    </div>
  );
}

/**
 * A Hot Sheet 1 import or backup job's details, shown from its banner's Details button in a modal
 * dialog rather than expanded below the banner (HS2-EEBM4W): the destination, what progress means,
 * the error, and any warnings. Closing it (its close button or Escape) clears the app state.
 */
export function Hs1JobDetailsDialog({ job }: { job?: MigrationJob }) {
  if (!job) return null;
  const subject = job.kind === 'import' ? 'Import' : 'Backup';
  return (
    <DialogSurface
      label={`${subject} Details`}
      describedBy="hs1-job-details-summary"
      rootAttributes={{
        'data-key': 'hs1-job-details-dialog',
        ...PROJECT_LIFECYCLE_TARGETS.hs1JobDetailsDialog.attrs,
        'data-controlled-open': 'true',
      }}
    >
      <List gap="m">
        <ListInsetText sides="rl">
          <Text variant="p" tone="quiet" flush id="hs1-job-details-summary">
            Progress describes the current phase. Completion includes verification and project setup.
          </Text>
        </ListInsetText>
        <ValueTable label={`${subject} details`}>
          <ValueTableRow
            label={job.kind === 'import' ? 'Import destination' : 'Backup repository'}
            value={<SourcePath path={job.store} />}
          />
          <ValueTableRow label="Phase" value={migrationPhaseLabel(job.progress)} />
        </ValueTable>
        {job.error || job.warnings.length > 0 ? (
          <ListInsetText sides="rl">
            <List gap="xs">
              {job.error ? (
                <Text variant="p" wrap="anywhere" flush>
                  {job.error}
                </Text>
              ) : (
                <></>
              )}
              {job.warnings.map((warning) => (
                <Text variant="p" wrap="anywhere" flush>
                  {warning}
                </Text>
              ))}
            </List>
          </ListInsetText>
        ) : (
          <></>
        )}
      </List>
    </DialogSurface>
  );
}
