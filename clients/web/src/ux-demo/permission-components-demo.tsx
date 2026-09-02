import { signal } from 'kerfjs';

import { NotificationCenter } from '../components/notification-center';
import { PermissionRequestCard, type PermissionRequestCardState,PermissionRequestPopup, updatePermissionCountdownText } from '../components/permission-request-card';
import { formatPermissionCountdown, type PermissionHistoryItem, type PermissionItem } from '../permission-notifications';
import { syncSettingsControls } from './settings-controls';

const pending: PermissionItem = {
  id: 42, connection: 'claude-main', tool: 'Edit', action: `~/Documents/hotsheet2/clients/web/src/components/notification-center.tsx

Add a shared pending and history presentation for permission requests.`, always_allow_supported: true,
  key: 'hotsheet2:42', projectId: 'hotsheet2', projectName: 'Hot Sheet 2', agent: 'Claude', role: 'main worker', receivedAt: Date.now() - 68_000, ignored: false,
};

const history: PermissionHistoryItem[] = [
  { ...pending, key: 'hotsheet2:41', id: 41, tool: 'Bash', action: 'npm run test:unit', decision: 'allow', scope: 'once', resolvedAt: Date.now() - 12 * 60_000 },
  { ...pending, key: 'glassbox:17', id: 17, projectId: 'glassbox', projectName: 'Glassbox', agent: 'Codex', tool: 'applyPatchApproval', action: 'Update the provider connection contract.', decision: 'external', resolvedAt: Date.now() - 31 * 60_000 },
];

export type PermissionDemoVariant = PermissionRequestCardState | 'allowed' | 'denied' | 'external';
export type PermissionDemoRequest = 'command' | 'edit' | 'read' | 'tool-without-details';

export const permissionRequestSettings = {
  presentation: signal<'popup' | 'list'>('popup'),
  variant: signal<PermissionDemoVariant>('pending'),
  request: signal<PermissionDemoRequest>('edit'),
  automation: signal<'none' | 'allow' | 'deny'>('allow'),
  alwaysSupported: signal(true),
  explanation: signal(true),
};
let permissionDemoRemainingMs = 13_000;

export const permissionDemoCountdownRemainingMs = () => permissionDemoRemainingMs;

const isResolvedVariant = (variant: PermissionDemoVariant): variant is 'allowed' | 'denied' | 'external' =>
  variant === 'allowed' || variant === 'denied' || variant === 'external';

/** Advance the catalog's real countdown without coupling component rendering to a clock. */
export function advancePermissionRequestDemoCountdown(stepMs = 1_000): number {
  if (permissionRequestSettings.automation.value === 'none' || isResolvedVariant(permissionRequestSettings.variant.value)) return permissionDemoRemainingMs;
  permissionDemoRemainingMs = permissionDemoRemainingMs === 0
    ? 13_000
    : Math.max(0, permissionDemoRemainingMs - stepMs);
  return permissionDemoRemainingMs;
}

/** Start the demo-only clock. It updates locally and never performs network polling. */
export function startPermissionRequestDemoCountdown(root: ParentNode, isVisible: () => boolean): number {
  return window.setInterval(() => {
    if (!isVisible()) return;
    const remaining = advancePermissionRequestDemoCountdown();
    updatePermissionCountdownText(root, pending.key, formatPermissionCountdown(remaining));
  }, 1_000);
}

export function resetPermissionRequestDemoCountdown(): void {
  permissionDemoRemainingMs = 13_000;
}

export function stopPermissionRequestDemoAutomation(root?: ParentNode): void {
  resetPermissionRequestDemoCountdown();
  permissionRequestSettings.automation.value = 'none';
  if (root) syncSettingsControls(root, 'permission-request', { values: { automation: 'none' } });
}

function demoItem(): PermissionItem {
  const request = permissionRequestSettings.request.value;
  const operation = request === 'command'
    ? { tool: 'Bash', action: 'npm run test:unit\nnpm run lint' }
    : request === 'edit'
      ? { tool: 'Edit', action: '~/Documents/hotsheet2/clients/web/src/components/notification-center.tsx\n\nAdd a shared pending and history presentation for permission requests.' }
      : request === 'read'
        ? { tool: 'Read', action: '~/Documents/hotsheet2/CLAUDE.md' }
        : { tool: 'ToolSearch', action: '' };
  return { ...pending, ...operation, always_allow_supported: permissionRequestSettings.alwaysSupported.value };
}

function demoHistory(item: PermissionItem, variant: 'allowed' | 'denied' | 'external'): PermissionHistoryItem {
  return {
    ...item,
    decision: variant === 'allowed' ? 'allow' : variant === 'denied' ? 'deny' : 'external',
    scope: variant === 'allowed' && item.always_allow_supported ? 'always' : 'once',
    resolvedAt: Date.now() - 12 * 60_000,
  };
}

export function resetPermissionRequestDemo(root?: ParentNode): void {
  resetPermissionRequestDemoCountdown();
  permissionRequestSettings.presentation.value = 'popup';
  permissionRequestSettings.variant.value = 'pending';
  permissionRequestSettings.request.value = 'edit';
  permissionRequestSettings.automation.value = 'allow';
  permissionRequestSettings.alwaysSupported.value = true;
  permissionRequestSettings.explanation.value = true;
  if (root) syncSettingsControls(root, 'permission-request', {
    values: {
      presentation: permissionRequestSettings.presentation.value,
      variant: permissionRequestSettings.variant.value,
      request: permissionRequestSettings.request.value,
      automation: permissionRequestSettings.automation.value,
    },
    checked: {
      'always-supported': permissionRequestSettings.alwaysSupported.value,
      explanation: permissionRequestSettings.explanation.value,
    },
  });
}

export function PermissionRequestDemo() {
  const item = demoItem();
  const variant = permissionRequestSettings.variant.value;
  const resolved = isResolvedVariant(variant);
  const displayItem = resolved ? demoHistory(item, variant) : item;
  const automation = permissionRequestSettings.automation.value;
  const props = {
    item: displayItem,
    state: resolved ? undefined : variant,
    countdown: !resolved && automation !== 'none' ? formatPermissionCountdown(permissionDemoRemainingMs) : undefined,
    countdownAction: automation === 'deny' ? 'deny' as const : 'allow' as const,
    explanation: permissionRequestSettings.explanation.value ? 'The agent is updating the notification surface requested for this project.' : undefined,
    error: variant === 'failed' ? 'The permission response could not be delivered.' : variant === 'disconnected' ? 'The agent disconnected before this request was answered.' : undefined,
  };
  return <section class="permission-request-demo" aria-label="PermissionRequestCard demo">
    {permissionRequestSettings.presentation.value === 'popup'
      ? <PermissionRequestPopup {...props} />
      : <PermissionRequestCard {...props} presentation="list" />}
  </section>;
}

export function PermissionRequestSettings() {
  return <form class="settings-form" data-settings="permission-request">
    <wa-select name="presentation" label="Presentation" value={permissionRequestSettings.presentation.value}>
      <wa-option value="popup">Popup</wa-option><wa-option value="list">Notification list</wa-option>
    </wa-select>
    <wa-select name="variant" label="State or outcome" value={permissionRequestSettings.variant.value}>
      <wa-option value="pending">Pending</wa-option><wa-option value="resolving">Resolving</wa-option>
      <wa-option value="failed">Failed</wa-option><wa-option value="disconnected">Disconnected</wa-option>
      <wa-option value="allowed">Allowed history</wa-option><wa-option value="denied">Denied history</wa-option>
      <wa-option value="external">External decision</wa-option>
    </wa-select>
    <wa-select name="request" label="Request type" value={permissionRequestSettings.request.value}>
      <wa-option value="command">Run command</wa-option><wa-option value="edit">Edit file</wa-option>
      <wa-option value="read">Read file</wa-option><wa-option value="tool-without-details">Tool without details</wa-option>
    </wa-select>
    <wa-select name="automation" label="Automatic decision" value={permissionRequestSettings.automation.value}>
      <wa-option value="none">None</wa-option><wa-option value="allow">Allow countdown</wa-option><wa-option value="deny">Deny countdown</wa-option>
    </wa-select>
    <wa-checkbox name="always-supported" checked={permissionRequestSettings.alwaysSupported.value}>Supports Always Allow</wa-checkbox>
    <wa-checkbox name="explanation" checked={permissionRequestSettings.explanation.value}>Show explanation</wa-checkbox>
    <wa-button type="button" data-action="reset-settings">Reset</wa-button>
  </form>;
}

export function NotificationCenterDemo() {
  return <section class="notification-center-demo" aria-label="NotificationCenter demo"><NotificationCenter pending={[pending]} history={history} countdowns={{ [pending.key]: '0:13' }} /></section>;
}
