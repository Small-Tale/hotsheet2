import { Badge } from '@kerfjs/ui/badge';
import { rem } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';
import { Ban, Check, Clock, Eye, House, MonitorSmartphone, MoreHorizontal, Pencil, ShieldCheck, X } from 'lucide';

import type { RemoteDevicesState } from '../features/remote-devices';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { deviceTime, pendingDeviceSummary, type RemoteDeviceList, remoteDeviceSummary } from '../remote-devices-model';

export interface RemoteDevicesSettingsProps {
  state?: RemoteDevicesState;
  /** The host's answer; absent while loading or when refused. */
  list?: RemoteDeviceList;
  /** The last failure, shown until the next change or successful load. */
  message?: string;
  /** Devices and requests with a change in flight. */
  busy?: readonly string[];
  /** Formats a Unix time in seconds; the catalog passes a fixed clock. */
  formatTime?: (seconds: number) => string;
}

const ACTIONS = NOTIFICATIONS_AND_LINKS_ACTIONS;
const HOST_ACCESS_REASON = 'Only the host or a device with host access can change host access.';

const menuIcon = <LucideIcon icon={MoreHorizontal} name="more-horizontal" />;

/**
 * App Settings → Devices (HS2-DXM0YN, docs/08 §8.2.2): pairing requests and paired browsers for the
 * trusted host and admin sessions. Every row action sits in its "…" menu, which the row itself and a
 * right-click also open; the delegated handlers live in `remote-devices-settings-interactions.ts`. A
 * session without the admin grant sees the category with the reason instead of the list.
 */
export function RemoteDevicesSettings({
  state = 'idle',
  list,
  message = '',
  busy = [],
  formatTime = deviceTime,
}: RemoteDevicesSettingsProps) {
  const loading = !list && state !== 'forbidden',
    hostAccess = list?.manager.local ?? false,
    pending = list?.pending ?? [],
    devices = list?.devices ?? [],
    minutes = Math.round((list?.pair_ttl_seconds ?? 900) / 60);
  return (
    <div data-component="remote-devices-settings">
      <List gap="l">
        <List gap="xs">
          <Text variant="h2">Devices</Text>
          <Text tone="quiet">{`Browsers paired with Hot Sheet on this computer. A pairing request expires after ${String(minutes)} minutes.`}</Text>
          {message ? (
            <Text tone="danger" size="compact" role="alert">
              {message}
            </Text>
          ) : null}
        </List>
        {state === 'forbidden' ? (
          <EmptyState
            icon={<LucideIcon icon={ShieldCheck} name="shield-check" />}
            title="Administrator access required."
            detail="Ask an administrator to grant this device administrator access in Settings → Devices, or grant it on the host with the hotsheet-cli remote sessions update command."
            rootAttributes={{ 'data-remote-devices-state': 'forbidden' }}
          />
        ) : (
          <>
            <List gap="xs">
              <ListHeader label="Pairing Requests" />
              {loading ? (
                <List gap={rem(0.125)}>
                  <ListActionRow
                    label="Loading"
                    action={ACTIONS.openRemoteDeviceActions.value}
                    placeholder
                    trailingMenu={{ label: 'Request Actions', icon: menuIcon, items: [] }}
                  />
                </List>
              ) : pending.length === 0 ? (
                <EmptyState
                  title="No pairing requests."
                  detail="A browser that opens Hot Sheet from another device asks to pair here."
                />
              ) : (
                <List gap={rem(0.125)}>
                  {pending.map((request) => {
                    const attributes = { 'data-device-id': request.id };
                    return (
                      <ListActionRow
                        label={request.label}
                        description={pendingDeviceSummary(request, formatTime)}
                        status={
                          <Badge tone={request.approved ? 'success' : 'warning'} appearance="quiet" size="compact">
                            {request.approved ? 'Approved' : 'Waiting'}
                          </Badge>
                        }
                        icon={<LucideIcon icon={Clock} name="clock" />}
                        iconTile="neutral"
                        action={ACTIONS.openRemoteDeviceActions.value}
                        itemId={request.id}
                        busy={busy.includes(request.id)}
                        rootAttributes={{ 'data-remote-pending': request.id }}
                        trailingMenu={{
                          label: `Actions for ${request.label}`,
                          icon: menuIcon,
                          items: [
                            {
                              label: 'Approve Read-Write',
                              action: ACTIONS.approveRemoteDevice.value,
                              icon: <LucideIcon icon={Check} name="check" />,
                              attributes: { ...attributes, 'data-role': 'read_write' },
                              disabled: request.approved,
                            },
                            {
                              label: 'Approve Read-Only',
                              action: ACTIONS.approveRemoteDevice.value,
                              icon: <LucideIcon icon={Eye} name="eye" />,
                              attributes: { ...attributes, 'data-role': 'read_only' },
                              disabled: request.approved,
                            },
                            { kind: 'divider' as const },
                            {
                              label: 'Reject',
                              action: ACTIONS.rejectRemoteDevice.value,
                              tone: 'danger' as const,
                              icon: <LucideIcon icon={X} name="x" />,
                              attributes,
                            },
                          ],
                        }}
                      />
                    );
                  })}
                </List>
              )}
            </List>
            <List gap="xs">
              <ListHeader label="Paired Devices" />
              {loading ? null : devices.length === 0 ? (
                <EmptyState title="No paired devices." detail="Approved browsers appear here until they are revoked." />
              ) : (
                <List gap={rem(0.125)}>
                  {devices.map((device) => {
                    const attributes = { 'data-device-id': device.id },
                      self = device.id === list?.manager.device_id,
                      writable = device.role === 'read_write';
                    return (
                      <ListActionRow
                        label={device.label}
                        description={remoteDeviceSummary(device, formatTime)}
                        status={
                          self ? (
                            <Badge tone="neutral" appearance="quiet" size="compact">
                              This Device
                            </Badge>
                          ) : undefined
                        }
                        icon={<LucideIcon icon={MonitorSmartphone} name="monitor-smartphone" />}
                        iconTile="neutral"
                        action={ACTIONS.openRemoteDeviceActions.value}
                        itemId={device.id}
                        busy={busy.includes(device.id)}
                        rootAttributes={{ 'data-remote-device': device.id }}
                        trailingMenu={{
                          label: `Actions for ${device.label}`,
                          icon: menuIcon,
                          items: [
                            {
                              label: writable ? 'Make Read-Only' : 'Allow Changes',
                              action: ACTIONS.setRemoteDeviceRole.value,
                              icon: writable ? (
                                <LucideIcon icon={Eye} name="eye" />
                              ) : (
                                <LucideIcon icon={Pencil} name="pencil" />
                              ),
                              attributes: { ...attributes, 'data-role': writable ? 'read_only' : 'read_write' },
                            },
                            {
                              label: 'Administrator',
                              action: ACTIONS.toggleRemoteDeviceAdmin.value,
                              icon: <LucideIcon icon={ShieldCheck} name="shield-check" />,
                              checked: device.admin,
                              attributes,
                            },
                            {
                              label: 'Host Access',
                              action: ACTIONS.toggleRemoteDeviceHostAccess.value,
                              icon: <LucideIcon icon={House} name="house" />,
                              checked: device.local,
                              disabled: !hostAccess,
                              disabledReason: hostAccess ? undefined : HOST_ACCESS_REASON,
                              attributes,
                            },
                            { kind: 'divider' as const },
                            {
                              label: self ? 'Revoke This Device' : 'Revoke',
                              action: ACTIONS.revokeRemoteDevice.value,
                              tone: 'danger' as const,
                              icon: <LucideIcon icon={Ban} name="ban" />,
                              attributes,
                            },
                          ],
                        }}
                      />
                    );
                  })}
                </List>
              )}
            </List>
          </>
        )}
      </List>
    </div>
  );
}
