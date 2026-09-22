import '../components/heading.css';
import '../components/native-popover-dialog.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { ValueTable } from '@kerfjs/ui/value-table';
import { signal } from 'kerfjs';
import { AppWindow, ServerCog } from 'lucide';

import { Hs1CleanupBanner, Hs1JobBanner, Hs1MigrationBanner, Hs1MigrationDialog } from '../components/hs1-migration';

export function DialogHeaderDemo() {
  return (
    <section class="dialog-layout-demo dialog-surface">
      <div class="app-heading" data-component="heading" data-has-icon="true">
        <Toolbar
          dividerSides=""
          leading={
            <>
              <ToolbarControlGroup single className="app-heading__icon">
                <LucideIcon className="app-heading__symbol" icon={ServerCog} name="server-cog" />
              </ToolbarControlGroup>
              <ToolbarText text="Dialog title" id="dialog-header-demo-title" size="xlarge" />
            </>
          }
        />
        <p class="app-heading__summary">A concise explanation of the current dialog state.</p>
      </div>
    </section>
  );
}
export function ValueTableDemo() {
  return (
    <section class="dialog-layout-demo dialog-surface">
      <div class="app-heading" data-component="heading" data-has-icon="true">
        <Toolbar
          dividerSides=""
          leading={
            <>
              <ToolbarControlGroup single className="app-heading__icon">
                <LucideIcon className="app-heading__symbol" icon={AppWindow} name="app-window" />
              </ToolbarControlGroup>
              <ToolbarText text="Value table" id="value-table-demo-title" size="xlarge" />
            </>
          }
        />
        <p class="app-heading__summary">Static metadata uses inset separators and aligned values.</p>
      </div>
      <div class="dialog-layout-demo__body">
        <ValueTable label="Example metadata">
          <div>
            <dt>Version</dt>
            <dd>0.1.0</dd>
          </div>
          <div>
            <dt>Build</dt>
            <dd>
              <code>source-sha256:example</code>
            </dd>
          </div>
          <div>
            <dt>Protocol</dt>
            <dd>1–1</dd>
          </div>
        </ValueTable>
      </div>
    </section>
  );
}
export const hs1MigrationDialogDemoOpen = signal(true);
export function openHs1MigrationDialogDemo() {
  hs1MigrationDialogDemoOpen.value = true;
}
export function closeHs1MigrationDialogDemo() {
  hs1MigrationDialogDemoOpen.value = false;
}
export function Hs1MigrationDialogDemo() {
  return (
    <section class="metadata-control-demo" aria-label="Hs1MigrationDialog demo">
      {!hs1MigrationDialogDemoOpen.value && (
        <wa-button appearance="accent" data-action="open-hs1-migration-demo">
          Open import dialog
        </wa-button>
      )}
      <Hs1MigrationDialog
        projectName="Demo"
        projectRoot="/work/demo"
        sourcePath="/work/demo/.hotsheet"
        databasePath="/work/demo/.hotsheet/db"
        postgresVersion="17"
        defaultStore="/work/demo.hs2"
        open={hs1MigrationDialogDemoOpen.value}
        busy={false}
      />
    </section>
  );
}
export function Hs1MigrationBannerDemo() {
  return (
    <section class="dialog-layout-demo dialog-surface">
      <Hs1MigrationBanner databasePath="/work/demo/.hotsheet/db" />
      {(['measured', 'unknown', 'failed', 'succeeded', 'backup-unverified'] as const).map((state) => (
        <Hs1JobBanner
          details
          backupVerified={state !== 'backup-unverified'}
          job={{
            id: 'demo-migration',
            attempt: state,
            revision: 1,
            projectId: 'demo',
            root: '/work/demo',
            sourceIdentity: '/work/demo/.hotsheet/db',
            store: '/work/demo.hs2',
            kind: state === 'backup-unverified' ? 'backup' : 'import',
            ownerPid: 0,
            updatedAt: '',
            warnings: [],
            status:
              state === 'failed'
                ? 'failed'
                : state === 'succeeded' || state === 'backup-unverified'
                  ? 'succeeded'
                  : 'running',
            progress:
              state === 'measured'
                ? { version: 1, phase: 'copy_database', completed: 5242880, total: 10485760, unit: 'bytes' }
                : { version: 1, phase: 'open_database' },
            error: state === 'failed' ? 'The repository could not be written. Free disk space, then retry.' : undefined,
            result:
              state === 'succeeded'
                ? {
                    ticketStore: '/work/demo.hs2',
                    connectionId: 'git-demo',
                    tickets: 12,
                    attachments: 4,
                    toolsConfigured: true,
                  }
                : undefined,
          }}
        />
      ))}
      <Hs1CleanupBanner />
    </section>
  );
}
