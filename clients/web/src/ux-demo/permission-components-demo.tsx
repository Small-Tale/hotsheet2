import { CatalogExample } from '@kerfjs/ui/catalog';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { NotificationCenter } from '../components/notification-center';
import {
  PermissionRequestCard,
  type PermissionRequestCardState,
  PermissionRequestPopup,
  updatePermissionCountdownText,
} from '../components/permission-request-card';
import {
  formatPermissionCountdown,
  type PermissionHistoryItem,
  type PermissionItem,
} from '../permission-notifications';
import { selectedId } from './components/demo-state';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { syncSettingsControls } from './settings-controls';

const pending: PermissionItem = {
  id: 42,
  connection: 'claude-main',
  tool: 'Edit',
  action: `~/Documents/hotsheet2/clients/web/src/components/notification-center.tsx

Add a shared pending and history presentation for permission requests.`,
  always_allow_supported: true,
  key: 'hotsheet2:42',
  projectId: 'hotsheet2',
  projectName: 'Hot Sheet 2',
  agent: 'Claude',
  role: 'main worker',
  receivedAt: Date.now() - 68_000,
  ignored: false,
};

const history: PermissionHistoryItem[] = [
  {
    ...pending,
    key: 'hotsheet2:41',
    id: 41,
    tool: 'Bash',
    action: 'npm run test:unit',
    decision: 'allow',
    scope: 'once',
    resolvedAt: Date.now() - 12 * 60_000,
  },
  {
    ...pending,
    key: 'glassbox:17',
    id: 17,
    projectId: 'glassbox',
    projectName: 'Glassbox',
    agent: 'Codex',
    tool: 'applyPatchApproval',
    action: 'Update the provider connection contract.',
    decision: 'external',
    resolvedAt: Date.now() - 31 * 60_000,
  },
];

export type PermissionDemoVariant = PermissionRequestCardState | 'allowed' | 'denied' | 'external';
export type PermissionDemoRequest = 'command' | 'edit' | 'read' | 'tool-without-details';

export const permissionRequestSettings = {
  presentation: signal<'popup' | 'flow' | 'top-layer' | 'list'>('popup'),
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
  if (
    permissionRequestSettings.automation.value === 'none' ||
    isResolvedVariant(permissionRequestSettings.variant.value)
  )
    return permissionDemoRemainingMs;
  permissionDemoRemainingMs =
    permissionDemoRemainingMs === 0 ? 13_000 : Math.max(0, permissionDemoRemainingMs - stepMs);
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
  const operation = (() => {
    switch (request) {
      case 'command':
        return { tool: 'Bash', action: 'npm run test:unit\nnpm run lint' };
      case 'edit':
        return {
          tool: 'Edit',
          action:
            '~/Documents/hotsheet2/clients/web/src/components/notification-center.tsx\n\nAdd a shared pending and history presentation for permission requests.',
        };
      case 'read':
        return { tool: 'Read', action: '~/Documents/hotsheet2/CLAUDE.md' };
      case 'tool-without-details':
        return { tool: 'ToolSearch', action: '' };
      default:
        return request satisfies never;
    }
  })();
  return { ...pending, ...operation, always_allow_supported: permissionRequestSettings.alwaysSupported.value };
}

function demoHistory(item: PermissionItem, variant: 'allowed' | 'denied' | 'external'): PermissionHistoryItem {
  const decision = (() => {
    switch (variant) {
      case 'allowed':
        return 'allow';
      case 'denied':
        return 'deny';
      case 'external':
        return 'external';
      default:
        return variant satisfies never;
    }
  })();
  return {
    ...item,
    decision,
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
  if (root)
    syncSettingsControls(root, 'permission-request', {
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
    countdownAction: automation === 'deny' ? ('deny' as const) : ('allow' as const),
    explanation: permissionRequestSettings.explanation.value
      ? 'The agent is updating the notification surface requested for this project.'
      : undefined,
    error:
      variant === 'failed'
        ? 'The permission response could not be delivered.'
        : variant === 'disconnected'
          ? 'The agent disconnected before this request was answered.'
          : undefined,
  };
  const columned = ['flow', 'list'].includes(permissionRequestSettings.presentation.value);
  return (
    // The frame stands in for the shell's main column: it carries the `--app-shell-main` anchor the
    // top-layer popup positions against, and its centered 640px content column stands in for the AI
    // conversation's foreground that lays out the in-flow popup (KF-D532S2, HS2-ZESCM2, HS2-M2W2DP).
    <CatalogExample
      label="Interactive"
      viewport={{
        width: 'full',
        minHeight: 'app',
        anchorName: '--app-shell-main',
        // Only the in-flow and list presentations sit in the conversation column; the corner popups
        // keep the full main column.
        ...(columned ? { contentWidth: 'medium' as const, contentAlign: 'center' as const } : {}),
      }}
      rootAttributes={{ 'data-permission-request-demo': '' }}
    >
      {permissionRequestSettings.presentation.value === 'flow' ? (
        <PermissionRequestPopup {...props} layer="flow" />
      ) : permissionRequestSettings.presentation.value !== 'list' ? (
        <PermissionRequestPopup
          {...props}
          layer={permissionRequestSettings.presentation.value === 'top-layer' ? 'top' : 'inline'}
        />
      ) : (
        <PermissionRequestCard {...props} presentation="list" />
      )}
    </CatalogExample>
  );
}

export function PermissionRequestSettings() {
  return (
    <form class="settings-form" data-settings="permission-request">
      <Select
        name="presentation"
        label="Presentation"
        value={permissionRequestSettings.presentation.value}
        choices={[
          { value: 'popup', label: 'Popup' },
          { value: 'flow', label: 'In-Flow Popup (Conversation Foreground)' },
          { value: 'top-layer', label: 'Shell Popup (Top Layer)' },
          { value: 'list', label: 'Notification List' },
        ]}
      />
      <Select
        name="variant"
        label="State or outcome"
        value={permissionRequestSettings.variant.value}
        choices={[
          { value: 'pending', label: 'Pending' },
          { value: 'resolving', label: 'Resolving' },
          { value: 'failed', label: 'Failed' },
          { value: 'disconnected', label: 'Disconnected' },
          { value: 'allowed', label: 'Allowed History' },
          { value: 'denied', label: 'Denied History' },
          { value: 'external', label: 'External Decision' },
        ]}
      />
      <Select
        name="request"
        label="Request type"
        value={permissionRequestSettings.request.value}
        choices={[
          { value: 'command', label: 'Run Command' },
          { value: 'edit', label: 'Edit File' },
          { value: 'read', label: 'Read File' },
          { value: 'tool-without-details', label: 'Tool Without Details' },
        ]}
      />
      <Select
        name="automation"
        label="Automatic decision"
        value={permissionRequestSettings.automation.value}
        choices={[
          { value: 'none', label: 'None' },
          { value: 'allow', label: 'Allow Countdown' },
          { value: 'deny', label: 'Deny Countdown' },
        ]}
      />
      <wa-checkbox name="always-supported" checked={permissionRequestSettings.alwaysSupported.value}>
        Supports Always Allow
      </wa-checkbox>
      <wa-checkbox name="explanation" checked={permissionRequestSettings.explanation.value}>
        Show explanation
      </wa-checkbox>
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
  );
}

export function NotificationCenterDemo() {
  return (
    <CatalogExample label="Pending and History" viewport={{ width: 'full', height: 'app', layout: 'flex-column' }}>
      <NotificationCenter pending={[pending]} history={history} countdowns={{ [pending.key]: '0:13' }} />
    </CatalogExample>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wirePermissionComponentsDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  startPermissionRequestDemoCountdown(root, () => selectedId.value === 'permission-request');
  demoListeners.add(
    delegate(root, 'change', '[data-settings="permission-request"] [name]', (_event, target) => {
      const control = target as FormControl;
      switch (control.getAttribute('name')) {
        case 'presentation':
          permissionRequestSettings.presentation.value =
            control.value as typeof permissionRequestSettings.presentation.value;
          break;
        case 'variant':
          permissionRequestSettings.variant.value = control.value as typeof permissionRequestSettings.variant.value;
          break;
        case 'request':
          permissionRequestSettings.request.value = control.value as typeof permissionRequestSettings.request.value;
          break;
        case 'automation':
          resetPermissionRequestDemoCountdown();
          permissionRequestSettings.automation.value =
            control.value as typeof permissionRequestSettings.automation.value;
          break;
        case 'always-supported':
          permissionRequestSettings.alwaysSupported.value = control.checked;
          break;
        case 'explanation':
          permissionRequestSettings.explanation.value = control.checked;
          break;
        case null:
          break;
      }
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.cancelPermissionAutomation.selector, (event) => {
      event.stopImmediatePropagation();
      stopPermissionRequestDemoAutomation(root);
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'permission-request': {
      catalogExamples: true,
      Demo: PermissionRequestDemo,
      Settings: PermissionRequestSettings,
      reset: resetPermissionRequestDemo,
    },
    'notification-center': { catalogExamples: true, Demo: NotificationCenterDemo },
  },
  wire: wirePermissionComponentsDemo,
};
