import { Button } from '@kerfjs/ui/button';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { FramedList, FramedListRow } from '@kerfjs/ui/framed-list';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';
import { Text } from '@kerfjs/ui/text';

import type { AddHostDiskDialogState } from '../host-disk-choices';
import type { HostHealthDiskVolume } from '../host-health';
import { SHELL_AND_GLOBAL_ACTIONS } from '../interaction-attrs/shell-and-global';
import { formatBytes } from '../stats-this-mac';
import { AddHostDiskDialog } from './add-host-disk-dialog';

export interface HostDiskSettingsProps {
  /** The server host's volumes from the pushed host-health snapshot; absent while loading. */
  disks?: readonly HostHealthDiskVolume[];
  /** The server host's name, because the viewer may be on another device. */
  hostName?: string;
  /** The Add Disk dialog while it is open. */
  dialog?: AddHostDiskDialogState;
  message?: string;
}

/** The quiet line under a disk's name: where it is, its size, and why it is in the list. */
export function hostDiskDetail(disk: HostHealthDiskVolume): string {
  const parts = [
    disk.store ? 'Holds the ticket store; always monitored' : disk.mount,
    disk.free_bytes !== null && disk.total_bytes !== null
      ? `${formatBytes(disk.free_bytes)} free of ${formatBytes(disk.total_bytes)}`
      : undefined,
    disk.present ? undefined : 'not connected',
    disk.decision === 'pending' ? 'new' : undefined,
    disk.manual ? 'added by path' : undefined,
  ];
  return parts.filter((part): part is string => Boolean(part)).join(' · ');
}

/**
 * App Settings → General → Host Health Disks (HS2-KBVZV1): every volume host health knows about on the
 * server's machine, with a Monitor switch for each. The ticket store's disk is always monitored. A
 * disk that is not connected keeps its choice and can be forgotten; any path adds its volume, which
 * covers mounts detection skips, such as a network share; Add Disk… asks for it in a dialog. `hotsheet-cli host-disks` makes the same
 * choices headlessly.
 */
export function HostDiskSettings({ disks, hostName, dialog, message = '' }: HostDiskSettingsProps) {
  return (
    <div data-component="host-disk-settings">
      <List gap="xs">
        <Text variant="h2">Host health disks</Text>
        <Text tone="quiet" flush>
          {`Disks whose free space host health watches on ${hostName ?? 'the server host'}. A newly connected disk asks once on the Stats page.`}
        </Text>
        {disks === undefined ? (
          <Text tone="quiet" size="compact" flush>
            Loading disks…
          </Text>
        ) : (
          <FramedList label="Host Health Disks">
            {disks.map((disk) => (
              <FramedListRow rootAttributes={{ 'data-host-disk': disk.id }}>
                <Row gap="xs" vAlign="middle">
                  <Checkbox
                    action={SHELL_AND_GLOBAL_ACTIONS.toggleHostDisk.value}
                    name={`host-disk:${disk.id}`}
                    checked={disk.store || disk.decision === 'included'}
                    disabled={disk.store}
                    label={disk.store ? `${disk.label} (store)` : disk.label}
                    hint={hostDiskDetail(disk)}
                    rootAttributes={{ 'data-disk-id': disk.id }}
                  />
                  <Spacer flex />
                  {!disk.store && (!disk.present || disk.manual) ? (
                    <Button
                      type="button"
                      size="compact"
                      action={SHELL_AND_GLOBAL_ACTIONS.forgetHostDisk.value}
                      rootAttributes={{ 'data-disk-id': disk.id }}
                    >
                      Forget
                    </Button>
                  ) : null}
                </Row>
              </FramedListRow>
            ))}
          </FramedList>
        )}
        <Row gap="xs" vAlign="middle">
          <Button type="button" action={SHELL_AND_GLOBAL_ACTIONS.openAddHostDisk.value}>
            Add Disk…
          </Button>
        </Row>
        <Text variant="span" tone="danger" size="compact" role="status">
          {message}
        </Text>
      </List>
      <AddHostDiskDialog state={dialog} />
    </div>
  );
}
