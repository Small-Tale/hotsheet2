import { delegate, delegateCapture, type Signal } from 'kerfjs';

import { type NotificationView } from '../components/notification-navigation';
import {
  parsePermissionAutomation,
  type PermissionAutomation,
  type PermissionDecision,
  type PermissionInbox,
  type PermissionItem,
  type PermissionScope,
  type VisiblePermissionTimer,
} from '../permission-notifications';
import { type TicketLinkMatch, ticketLinkMatchKey, type TicketLinkReference } from '../ticket-link-resolution';
import { data } from './dom';
import { type Control, type Project } from './types';

/** Live application bindings used by this handler group. */
export interface NotificationAndLinkInteractionsDependencies {
  readonly notificationView: Signal<NotificationView>;
  readonly project: () => Project | undefined;
  readonly permissionTimer: VisiblePermissionTimer;
  permissionCountdown: { key: string; remainingMs: number } | undefined;
  readonly permissionAutomationByProject: Signal<Record<string, PermissionAutomation>>;
  readonly updatePermissionTimer: () => void;
  readonly permissionRevision: Signal<number>;
  readonly permissionInbox: PermissionInbox;
  readonly pendingPermissions: () => PermissionItem[];
  readonly resolvePermission: (
    item: PermissionItem,
    decision: PermissionDecision,
    scope: PermissionScope,
    automatic?: boolean,
  ) => Promise<void>;
  readonly error: Signal<string>;
  readonly selectedProjectId: Signal<string>;
  readonly hideVerifiedByProject: Signal<Record<string, boolean>>;
  ticketLinkReturnFocus: HTMLElement | undefined;
  readonly selectLinkedTicket: (slug: string, projectId?: string, preferredProjectId?: string) => Promise<void>;
  readonly ticketLinkChoice: Signal<
    { kind: 'choose'; reference: TicketLinkReference; matches: TicketLinkMatch[] } | undefined
  >;
  readonly openTicketLinkMatch: (match: TicketLinkMatch) => Promise<void>;
  readonly cancelTicketLinkChoice: () => void;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireNotificationAndLinkInteractions(dependencies: NotificationAndLinkInteractionsDependencies) {
  const {
    notificationView,
    project,
    permissionTimer,
    permissionAutomationByProject,
    updatePermissionTimer,
    permissionRevision,
    permissionInbox,
    pendingPermissions,
    resolvePermission,
    error,
    selectedProjectId,
    hideVerifiedByProject,
    selectLinkedTicket,
    ticketLinkChoice,
    openTicketLinkMatch,
    cancelTicketLinkChoice,
  } = dependencies;
  delegate(document.body, 'click', '[data-action="select-notification-view"]', (_event, target) => {
    notificationView.value = (data(target).itemId ?? 'pending') as NotificationView;
  });
  delegate(
    document.body,
    'change',
    '[name="permission-automation-action"], [name="permission-automation-delay"]',
    () => {
      const current = project();
      if (!current) return;
      const action = (document.querySelector<Control>('[name="permission-automation-action"]')?.value ??
          'off') as PermissionAutomation['action'],
        delayMs = Number(document.querySelector<Control>('[name="permission-automation-delay"]')?.value ?? 60_000),
        next = parsePermissionAutomation({ action, delayMs });
      permissionTimer.hide();
      dependencies.permissionCountdown = undefined;
      permissionAutomationByProject.value = { ...permissionAutomationByProject.value, [current.id]: next };
      localStorage.setItem(`hotsheet.project.${current.id}.permission-automation`, JSON.stringify(next));
      updatePermissionTimer();
      permissionRevision.value += 1;
    },
  );
  delegate(document.body, 'click', '[data-action="ignore-permission"]', (_event, target) => {
    const key = data(target).requestKey;
    if (!key) return;
    permissionTimer.hide();
    permissionInbox.ignore(key);
    updatePermissionTimer();
    permissionRevision.value += 1;
  });
  delegate(document.body, 'click', '[data-action="cancel-permission-automation"]', (event, target) => {
    event.stopImmediatePropagation();
    const key = data(target).requestKey;
    if (!key) return;
    permissionTimer.cancel(key);
    dependencies.permissionCountdown = undefined;
    permissionRevision.value += 1;
  });
  delegate(document.body, 'click', '[data-action="resolve-permission"]', (_event, target) => {
    const key = data(target).requestKey,
      item = pendingPermissions().find((value) => value.key === key);
    if (item)
      void resolvePermission(item, data(target).decision as PermissionDecision, data(target).scope as PermissionScope);
  });
  delegate(document.body, 'click', '[data-action="dismiss-app-error"]', () => {
    error.value = '';
  });
  delegate(document.body, 'change', '[data-action="toggle-verified-column"]', (_event, target) => {
    const id = selectedProjectId.value;
    if (!id) return;
    const checked = (target as HTMLInputElement).checked;
    hideVerifiedByProject.value = { ...hideVerifiedByProject.value, [id]: checked };
    localStorage.setItem(`hotsheet.project.${id}.hide-verified-column`, String(checked));
  });
  delegate(document.body, 'click', '[data-action="open-linked-ticket"]', (event, target) => {
    event.preventDefault();
    const slug = data(target).ticketSlug;
    if (slug) {
      dependencies.ticketLinkReturnFocus = target as HTMLElement;
      void selectLinkedTicket(slug, data(target).ticketProjectId);
    }
  });
  delegate(document.body, 'click', '[data-action="select-ticket-link-match"]', (_event, target) => {
    const choice = ticketLinkChoice.value,
      key = data(target).matchKey,
      match = choice?.matches.find((item) => ticketLinkMatchKey(item) === key);
    if (match) void openTicketLinkMatch(match);
  });
  delegate(document.body, 'click', '[data-action="cancel-ticket-link-choice"]', () => {
    cancelTicketLinkChoice();
  });
  delegateCapture(document.body, 'wa-hide', '[data-component="ticket-link-choice-dialog"]', () => {
    if (ticketLinkChoice.value) cancelTicketLinkChoice();
  });
}
