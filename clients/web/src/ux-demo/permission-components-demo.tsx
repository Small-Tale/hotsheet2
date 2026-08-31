import { NotificationCenter } from '../components/notification-center';
import { PermissionRequestPopup } from '../components/permission-request-card';
import type { PermissionHistoryItem, PermissionItem } from '../permission-notifications';

const pending: PermissionItem = {
  id: 42, connection: 'claude-main', tool: 'Edit', action: `~/Documents/hotsheet2/clients/web/src/components/notification-center.tsx

Add a shared pending and history presentation for permission requests.`, always_allow_supported: true,
  key: 'hotsheet2:42', projectId: 'hotsheet2', projectName: 'Hot Sheet 2', agent: 'Claude', role: 'main worker', receivedAt: Date.now() - 68_000, ignored: false,
};

const history: PermissionHistoryItem[] = [
  { ...pending, key: 'hotsheet2:41', id: 41, tool: 'Bash', action: 'npm run test:unit', decision: 'allow', scope: 'once', resolvedAt: Date.now() - 12 * 60_000 },
  { ...pending, key: 'glassbox:17', id: 17, projectId: 'glassbox', projectName: 'Glassbox', agent: 'Codex', tool: 'applyPatchApproval', action: 'Update the provider connection contract.', decision: 'external', resolvedAt: Date.now() - 31 * 60_000 },
];

export function PermissionRequestDemo() {
  return <section class="permission-request-demo" aria-label="PermissionRequestCard demo"><PermissionRequestPopup item={pending} countdown="0:13" explanation="The agent is updating the notification surface requested for this project." /></section>;
}

export function NotificationCenterDemo() {
  return <section class="notification-center-demo" aria-label="NotificationCenter demo"><NotificationCenter pending={[pending]} history={history} countdowns={{ [pending.key]: '0:13' }} /></section>;
}
