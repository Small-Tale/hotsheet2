import './hs1-migration.css';
import '@awesome.me/webawesome/dist/components/progress-bar/progress-bar.js';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { ValueTable } from '@kerfjs/ui/value-table';
import { ArchiveRestore, Database, Trash2 } from 'lucide';

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
          <div>
            <dt>Project</dt>
            <dd>
              <code>{projectRoot}</code>
            </dd>
          </div>
          <div>
            <dt>Data folder</dt>
            <dd>
              <code>{sourcePath}</code>
            </dd>
          </div>
          <div>
            <dt>Database</dt>
            <dd>
              <code>{databasePath}</code>
            </dd>
          </div>
          {postgresVersion ? (
            <div>
              <dt>PostgreSQL</dt>
              <dd>{postgresVersion}</dd>
            </div>
          ) : (
            <></>
          )}
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
