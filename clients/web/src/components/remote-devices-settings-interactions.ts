import { openPopupMenu } from '@kerfjs/ui/popup-menu';
import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { RemoteDevices } from '../features/remote-devices';
import { NOTIFICATIONS_AND_LINKS_ACTIONS as ACTIONS } from '../interaction-attrs/notifications-and-links';
import type { RemoteDeviceRole } from '../remote-devices-model';

const ROW = '[data-remote-device],[data-remote-pending]';
const ROW_MENU = '[data-component="popup-menu"]';

const roleOf = (target: Element): RemoteDeviceRole | undefined => {
  const role = target.getAttribute('data-role');
  return role === 'read_only' || role === 'read_write' ? role : undefined;
};

/**
 * Delegated handlers for App Settings → Devices (HS2-DXM0YN). A row's primary button and a
 * right-click open the row's "…" menu; each menu command runs one change through the controller,
 * whose long poll then shows the host's new list. The application and the catalog demo install the
 * same handlers over the same controller.
 */
export function wireRemoteDevicesSettings(root: HTMLElement, devices: RemoteDevices): () => void {
  const lifetime = createScope();
  const deviceOf = (target: Element) => target.getAttribute('data-device-id') ?? undefined;
  const device = (id: string) => devices.list.value?.devices.find((entry) => entry.id === id);
  const openRowMenu = (row: Element | null) => {
    const menu = row?.querySelector<HTMLElement>(ROW_MENU);
    if (!menu) return false;
    openPopupMenu(menu);
    return true;
  };
  lifetime.add(
    delegate(root, 'click', ACTIONS.openRemoteDeviceActions.selector, (_event, target) => {
      openRowMenu(target.closest(ROW));
    }),
  );
  lifetime.add(
    delegate(root, 'contextmenu', ROW, (event, target) => {
      if (openRowMenu(target)) event.preventDefault();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.approveRemoteDevice.selector, (_event, target) => {
      const id = deviceOf(target),
        role = roleOf(target);
      if (id && role) void devices.approve(id, role);
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.rejectRemoteDevice.selector, (_event, target) => {
      const id = deviceOf(target);
      if (id) void devices.reject(id);
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.setRemoteDeviceRole.selector, (_event, target) => {
      const id = deviceOf(target),
        role = roleOf(target);
      if (id && role) void devices.update(id, { role });
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.toggleRemoteDeviceAdmin.selector, (_event, target) => {
      const id = deviceOf(target),
        current = id ? device(id) : undefined;
      if (id && current) void devices.update(id, { admin: !current.admin });
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.toggleRemoteDeviceHostAccess.selector, (_event, target) => {
      const id = deviceOf(target),
        current = id ? device(id) : undefined;
      if (id && current) void devices.update(id, { local: !current.local });
    }),
  );
  lifetime.add(
    delegate(root, 'click', ACTIONS.revokeRemoteDevice.selector, (_event, target) => {
      const id = deviceOf(target);
      if (id) void devices.revoke(id);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
