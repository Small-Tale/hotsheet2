import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { TextField } from '@kerfjs/ui/text-field';

import type { AddHostDiskDialogState } from '../host-disk-choices';
import { SHELL_AND_GLOBAL_ACTIONS, SHELL_AND_GLOBAL_TARGETS } from '../interaction-attrs/shell-and-global';

/**
 * The Add Disk dialog of App Settings → Host health disks (HS2-KBVZV1): one path on the server's machine;
 * its volume is monitored, which covers mounts detection skips, such as a network share. A path on no
 * volume, or on the store's own disk, keeps the dialog open and says why.
 */
export function AddHostDiskDialog({ state }: { state?: AddHostDiskDialogState }) {
  if (!state) return null;
  return (
    <DialogSurface
      label="Add Disk"
      preferredWidth={remify(448)}
      viewportGutter={remify(16)}
      rootAttributes={{
        'data-key': 'add-host-disk-dialog',
        'data-add-host-disk-dialog': 'true',
        'data-controlled-open': 'true',
      }}
    >
      <form {...SHELL_AND_GLOBAL_ACTIONS.submitAddHostDisk.attrs}>
        <List gap="l">
          <TextField
            {...SHELL_AND_GLOBAL_TARGETS.hostDiskPath.attrs}
            label="Path on the Disk"
            value={state.value}
            placeholder="/Volumes/Backup"
            hint="Any folder on the disk, on the server's machine."
            autofocus
          />
          {state.message ? <StateBanner tone="danger" title="Disk Not Added" detail={state.message} /> : null}
          <Row vAlign="middle" gap="xs">
            <Button emphasis="plain" type="button" action={SHELL_AND_GLOBAL_ACTIONS.cancelAddHostDisk.value}>
              Cancel
            </Button>
            <Spacer flex />
            <Button emphasis="primary" type="submit" disabled={state.busy}>
              Add Disk
            </Button>
          </Row>
        </List>
      </form>
    </DialogSurface>
  );
}
