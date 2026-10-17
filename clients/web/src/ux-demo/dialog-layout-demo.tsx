import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { Hs1CleanupBanner, Hs1JobBanner, Hs1MigrationBanner, Hs1MigrationDialog } from '../components/hs1-migration';
import { NotificationsPausedBanner } from '../components/notifications-paused-banner';
import { type DemoDevice, DeviceSetting } from './components/device-setting';
import { DEMO_ACTIONS, DEMO_COMPONENTS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

export function ValueTableDemo() {
  return (
    <CatalogExample
      label="Metadata"
      note="Static metadata uses inset separators and aligned values."
      viewport={{ width: 'medium' }}
    >
      <ValueTable label="Example metadata">
        <ValueTableRow label="Version" value="0.1.0" />
        <ValueTableRow label="Build" value={<code>source-sha256:example</code>} />
        <ValueTableRow label="Protocol" value="1–1" />
      </ValueTable>
    </CatalogExample>
  );
}
export const hs1MigrationDialogDemoOpen = signal(true);
/** Remote clients disable the host-only folder chooser (HS2-NWWF1Z). */
export const hs1MigrationDialogDemoDevice = signal<DemoDevice>('host');
export function openHs1MigrationDialogDemo() {
  hs1MigrationDialogDemoOpen.value = true;
}
export function closeHs1MigrationDialogDemo() {
  hs1MigrationDialogDemoOpen.value = false;
}
export function Hs1MigrationDialogDemo() {
  return (
    <CatalogExample align="glyph" label="Interactive">
      {!hs1MigrationDialogDemoOpen.value && (
        <Button emphasis="primary" action={'open-hs1-migration-demo'}>
          Open Import Dialog
        </Button>
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
        hostActionsAvailable={hs1MigrationDialogDemoDevice.value === 'host'}
      />
    </CatalogExample>
  );
}
export function Hs1MigrationDialogSettings() {
  return (
    <DemoSettingsForm settings="hs1-migration-dialog">
      <DeviceSetting value={hs1MigrationDialogDemoDevice.value} />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** Restore the import dialog's device setting and reopen it (HS2-NWWF1Z). */
export function resetHs1MigrationDialogDemo(root?: ParentNode): void {
  hs1MigrationDialogDemoDevice.value = 'host';
  hs1MigrationDialogDemoOpen.value = true;
  if (root) syncSettingsControls(root, 'hs1-migration-dialog', { values: { device: 'host' } });
}

/** Both detail variants of the app-wide notification pause banner (HS2-QYA9SC). */
export function NotificationsPausedBannerDemo() {
  return (
    <>
      <CatalogExample label="Nothing Waiting">
        <NotificationsPausedBanner waiting={0} />
      </CatalogExample>
      <CatalogExample label="Notifications Waiting">
        <NotificationsPausedBanner waiting={3} />
      </CatalogExample>
    </>
  );
}

const hs1JobLabels = {
  measured: 'Import running, measured progress',
  unknown: 'Import running, unknown progress',
  failed: 'Import failed',
  succeeded: 'Import succeeded',
  'backup-unverified': 'Backup unverified',
} as const;

export function Hs1MigrationBannerDemo() {
  return (
    <>
      <CatalogExample label="Migration Available">
        <Hs1MigrationBanner databasePath="/work/demo/.hotsheet/db" />
      </CatalogExample>
      {(['measured', 'unknown', 'failed', 'succeeded', 'backup-unverified'] as const).map((state) => (
        <CatalogExample label={hs1JobLabels[state]}>
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
              error:
                state === 'failed' ? 'The repository could not be written. Free disk space, then retry.' : undefined,
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
        </CatalogExample>
      ))}
      <CatalogExample label="Cleanup Offered">
        <Hs1CleanupBanner />
      </CatalogExample>
    </>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireDialogLayoutDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(delegate(root, 'click', DEMO_ACTIONS.openHs1MigrationDemo.selector, openHs1MigrationDialogDemo));
  demoListeners.add(
    delegate(root, 'change', '[data-settings="hs1-migration-dialog"] [name="device"]', (_event, target) => {
      hs1MigrationDialogDemoDevice.value = (target as FormControl).value as DemoDevice;
      openHs1MigrationDialogDemo();
    }),
  );
  demoListeners.add(delegate(root, 'click', DEMO_ACTIONS.dismissHs1Migration.selector, closeHs1MigrationDialogDemo));
  demoListeners.add(
    delegate(root, 'wa-hide', DEMO_COMPONENTS.hs1MigrationDialog.selector, closeHs1MigrationDialogDemo),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'value-table': { catalogExamples: true, Demo: ValueTableDemo },
    'hs1-migration-dialog': {
      catalogExamples: true,
      Demo: Hs1MigrationDialogDemo,
      Settings: Hs1MigrationDialogSettings,
      reset: resetHs1MigrationDialogDemo,
    },
    'hs1-migration-banner': { catalogExamples: true, Demo: Hs1MigrationBannerDemo },
    'notifications-paused-banner': { catalogExamples: true, Demo: NotificationsPausedBannerDemo },
  },
  wire: wireDialogLayoutDemo,
};
