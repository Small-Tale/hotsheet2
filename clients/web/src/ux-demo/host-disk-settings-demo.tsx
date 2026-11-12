import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { AddHostDiskDialog } from '../components/add-host-disk-dialog';
import { HostDiskSettings } from '../components/host-disk-settings';
import { createHostDiskChoices } from '../host-disk-choices';
import { SHELL_AND_GLOBAL_ACTIONS, SHELL_AND_GLOBAL_TARGETS } from '../interaction-attrs/shell-and-global';
import type { DemoModule } from './demo-module';
import { hostHealthDisksFixture } from './host-health-fixtures';

/** A deterministic stand-in for the server: `/nowhere` holds no volume; any other path is added. */
const choices = createHostDiskChoices(
  () => ({
    setHostHealthDisk: () => Promise.resolve(),
    forgetHostHealthDisk: () => Promise.resolve(),
    addHostHealthDisk: (path: string) =>
      path === '/nowhere' ? Promise.reject(new Error(`no volume holds ${path}`)) : Promise.resolve(),
  }),
  () => Promise.resolve(),
);

/** App Settings → General → Host health disks in its states (HS2-KBVZV1). Synthetic volumes only. */
export function HostDiskSettingsDemo() {
  const dialog = choices.dialog.value;
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="Detected, Ignored, and Absent Disks"
        note="The store's disk is always monitored; a disconnected or added disk can be forgotten. Add Disk… opens the dialog; /nowhere shows a failure."
      >
        <HostDiskSettings
          disks={hostHealthDisksFixture()}
          hostName="studio"
          {...(dialog === undefined ? {} : { dialog })}
        />
      </CatalogExample>
      <CatalogExample label="Store Only" note="A machine with no other disks lists only the store's.">
        <HostDiskSettings disks={hostHealthDisksFixture().slice(0, 1)} hostName="studio" />
      </CatalogExample>
      <CatalogExample label="Loading">
        <HostDiskSettings />
      </CatalogExample>
      <CatalogExample label="Forget Failed">
        <HostDiskSettings
          disks={hostHealthDisksFixture().slice(0, 2)}
          hostName="studio"
          message="settings.json is read-only"
        />
      </CatalogExample>
    </CatalogExampleStack>
  );
}

/** The Add Disk dialog (HS2-KBVZV1) opens from Add Disk… in the settings demo. */
export function AddHostDiskDialogDemo() {
  return (
    <CatalogExample label="Closed" note="Open it with Add Disk… in the Host Health Disks demo.">
      <AddHostDiskDialog />
    </CatalogExample>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'host-disk-settings': { catalogExamples: true, Demo: HostDiskSettingsDemo },
    'add-host-disk-dialog': { catalogExamples: true, Demo: AddHostDiskDialogDemo },
  },
  wire(root) {
    const scope = createScope();
    scope.add(
      delegate(root, 'click', SHELL_AND_GLOBAL_ACTIONS.openAddHostDisk.selector, () => {
        choices.add.open();
      }),
    );
    scope.add(
      delegate(root, 'input', SHELL_AND_GLOBAL_TARGETS.hostDiskPath.selector, (_event, field) => {
        choices.add.input((field as HTMLInputElement).value);
      }),
    );
    scope.add(
      delegate(root, 'submit', SHELL_AND_GLOBAL_ACTIONS.submitAddHostDisk.selector, (event) => {
        event.preventDefault();
        void choices.add.submit();
      }),
    );
    scope.add(
      delegate(root, 'click', SHELL_AND_GLOBAL_ACTIONS.cancelAddHostDisk.selector, () => {
        choices.add.cancel();
      }),
    );
    scope.add(
      delegate(root, 'kui-request-close', '[data-add-host-disk-dialog]', () => {
        choices.add.cancel();
      }),
    );
    return () => {
      choices.add.cancel();
      scope.dispose();
    };
  },
};
