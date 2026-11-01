import type { ClientNotification } from './api';
import type { Project } from './interactions/types';

export function shouldUseSystemNotification(visible: boolean, focused: boolean): boolean {
  return !visible || !focused;
}

let permissionRequested = false;

/** Present a background notice through Tauri, or a browser origin that already has permission. */
export async function sendSystemNotification(project: Project, item: ClientNotification): Promise<void> {
  const title = `Hot Sheet · ${project.name}`;
  const body = item.message;
  if ('__TAURI_INTERNALS__' in window) {
    const native = await import('@tauri-apps/plugin-notification');
    let granted = await native.isPermissionGranted();
    if (!granted && !permissionRequested) {
      permissionRequested = true;
      granted = (await native.requestPermission()) === 'granted';
    }
    if (granted) native.sendNotification({ title, body });
    return;
  }
  if (typeof Notification !== 'undefined' && Notification.permission === 'granted')
    new Notification(title, { body, tag: item.id });
}
