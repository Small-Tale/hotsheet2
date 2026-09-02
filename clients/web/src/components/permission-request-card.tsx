import './permission-request-card.css';

import { Bot, Check, CircleAlert, Clock3, ExternalLink, ShieldCheck, X } from 'lucide';

import type { PermissionHistoryItem, PermissionItem } from '../permission-notifications';
import { LucideIcon } from './lucide-icon';

export type PermissionRequestCardState = 'pending' | 'resolving' | 'failed' | 'disconnected';

export interface PermissionRequestCardProps {
  item: PermissionItem | PermissionHistoryItem;
  presentation?: 'popup' | 'list';
  state?: PermissionRequestCardState;
  explanation?: string;
  countdown?: string;
  countdownAction?: 'allow' | 'deny';
  error?: string;
}

function isHistory(item: PermissionItem | PermissionHistoryItem): item is PermissionHistoryItem {
  return 'decision' in item;
}

function operationLabel(item: PermissionItem): string {
  const tool = item.tool.toLowerCase();
  const target = item.action.split('\n', 1)[0];
  if (tool === 'edit' || tool === 'write' || tool.includes('filechange')) return `Wants permission to edit ${target}`;
  if (tool === 'read') return `Wants permission to read ${target}`;
  if (tool === 'bash' || tool.includes('command')) return 'Wants permission to run a command';
  return `Wants permission to use ${item.tool}`;
}

function historyLabel(item: PermissionHistoryItem): string {
  if (item.decision === 'external') return 'Decision made outside Hot Sheet';
  const prefix = item.automatic ? 'Automatically ' : '';
  if (item.decision === 'deny') return `${prefix}denied permission`;
  if (item.scope === 'always') return `${prefix}allowed this kind of request`;
  return `${prefix}allowed permission`;
}

function relativeTime(timestamp: number): string {
  const elapsed = Math.max(0, Date.now() - timestamp);
  if (elapsed < 60_000) return 'Now';
  if (elapsed < 3_600_000) return `${Math.floor(elapsed / 60_000)}m ago`;
  if (elapsed < 86_400_000) return `${Math.floor(elapsed / 3_600_000)}h ago`;
  return `${Math.floor(elapsed / 86_400_000)}d ago`;
}

/** Shared permission presentation for floating prompts and notification history. */
export function PermissionRequestCard({ item, presentation = 'list', state = 'pending', explanation, countdown, countdownAction = 'allow', error }: PermissionRequestCardProps) {
  const history = isHistory(item);
  const alwaysSupported = !history && item.always_allow_supported === true;
  const statusLabel = history ? historyLabel(item) : operationLabel(item);
  const timestamp = history ? item.resolvedAt : item.receivedAt;
  const stateIcon = history
    ? item.decision === 'allow' ? Check : item.decision === 'deny' ? X : ExternalLink
    : state === 'failed' || state === 'disconnected' ? CircleAlert : state === 'resolving' ? Clock3 : ShieldCheck;
  const iconName = history
    ? item.decision === 'allow' ? 'check' : item.decision === 'deny' ? 'x' : 'external-link'
    : state === 'failed' || state === 'disconnected' ? 'circle-alert' : state === 'resolving' ? 'clock-3' : 'shield-check';

  return <article class={`permission-request-card permission-request-card--${presentation}`} data-component="permission-request-card" data-state={history ? item.decision : state} data-request-key={item.key}>
    <header class="permission-request-card__header">
      <span class="permission-request-card__identity"><LucideIcon icon={Bot} name="bot" /><strong>{item.agent}</strong><span aria-hidden="true">·</span><span>{item.role}</span></span>
      <span class="permission-request-card__project" title={item.projectName}>{item.projectName}</span>
      <time>{relativeTime(timestamp)}</time>
    </header>
    <div class="permission-request-card__summary">
      <LucideIcon icon={stateIcon} name={iconName} />
      <strong>{statusLabel}</strong>
    </div>
    {item.action.trim() && <pre class="permission-request-card__details"><code>{item.action}</code></pre>}
    {explanation && <p class="permission-request-card__explanation">{explanation}</p>}
    {error && <p class="permission-request-card__error" role="alert">{error}</p>}
    {!history && <footer class="permission-request-card__footer">
      <button type="button" class="permission-request-card__quiet-action" data-action="ignore-permission" data-request-key={item.key}>Ignore</button>
      <div class="permission-request-card__decision-area">
        {countdown && <span class="permission-request-card__countdown">Automatically {countdownAction === 'allow' ? 'allowed' : 'denied'} in <strong>{countdown}</strong><button type="button" data-action="cancel-permission-automation" data-request-key={item.key}>Cancel</button></span>}
        <div class="permission-request-card__buttons">
          <button type="button" data-action="resolve-permission" data-decision="deny" data-scope="once" data-request-key={item.key} disabled={state === 'resolving'}>Deny</button>
          {alwaysSupported && <button type="button" data-action="resolve-permission" data-decision="allow" data-scope="always" data-request-key={item.key} disabled={state === 'resolving'}>Always Allow</button>}
          <button type="button" class="permission-request-card__primary" data-action="resolve-permission" data-decision="allow" data-scope="once" data-request-key={item.key} disabled={state === 'resolving'}>{alwaysSupported ? 'Allow Once' : 'Allow'}</button>
        </div>
      </div>
    </footer>}
  </article>;
}

export function PermissionRequestPopup(props: Omit<PermissionRequestCardProps, 'presentation'>) {
  return <aside class="permission-request-popup" data-component="permission-request-popup" aria-label="Permission request"><PermissionRequestCard {...props} presentation="popup" /></aside>;
}
