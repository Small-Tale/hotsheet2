import './hs1-migration.css';
import '@awesome.me/webawesome/dist/components/progress-bar/progress-bar.js';

import { Button } from '@kerfjs/ui/button';
import { uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';
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
        <Row gap="xs">
          <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissHs1Cleanup.attrs}>
            Dismiss
          </button>
          <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.removeHs1Data.attrs}>
            Delete Old Files…
          </button>
        </Row>
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
        <Row gap="xs">
          <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissHs1Cleanup.attrs}>
            Dismiss
          </button>
          <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.removeHs1Data.attrs}>
            Repair Old App Registration…
          </button>
        </Row>
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
    <div data-component="hs1-job-banner" data-status={job.status} data-attempt={job.attempt}>
      <List gap="2xs" controlInsets="tb">
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
            <Row gap="xs">
              <button
                type="button"
                {...PROJECT_LIFECYCLE_ACTIONS.migrationJobDetails.attrs}
                aria-expanded={String(details)}
              >
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
            </Row>
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
          {details && (
            <List gap="xs">
              <Text size="compact" wrap="anywhere" flush>
                {job.kind === 'import' ? 'Import destination' : 'Backup repository'}: <code>{job.store}</code>
              </Text>
              <Text size="compact" flush>
                Progress describes the current phase. Completion includes verification and project setup.
              </Text>
              {job.error ? (
                <Text size="compact" wrap="anywhere" flush>
                  {job.error}
                </Text>
              ) : (
                <></>
              )}
              {job.warnings.map((warning) => (
                <Text size="compact" wrap="anywhere" flush>
                  {warning}
                </Text>
              ))}
            </List>
          )}
        </List>
      </List>
    </div>
  );
}
