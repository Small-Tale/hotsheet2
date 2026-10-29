import { openPopupMenuAt } from '@kerfjs/ui/popup-menu';
import { delegate, delegateCapture, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { type NotificationView } from '../components/notification-navigation';
import {
  rememberedPermissionPreview,
  reviewedPermissionPattern,
  setRememberedPermissionPattern,
  setRememberedPermissionPreview,
  toggleRememberedPermissionSuggestion,
} from '../components/permission-request-card';
import { type PermissionRuleFilters } from '../components/permission-rules-settings';
import {
  NOTIFICATIONS_AND_LINKS_ACTIONS,
  NOTIFICATIONS_AND_LINKS_TARGETS,
} from '../interaction-attrs/notifications-and-links';
import {
  parsePermissionAutomation,
  type PermissionAutomation,
  type PermissionDecision,
  type PermissionInbox,
  type PermissionItem,
  type PermissionPattern,
  type PermissionScope,
  type VisiblePermissionTimer,
} from '../permission-notifications';
import { createPermissionRuleEditorFocus } from '../permission-rule-editor-focus';
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
  readonly reviewAfterAuto: Signal<
    { item: PermissionItem; settling: boolean; allowed: boolean; error?: string } | undefined
  >;
  readonly rememberPermissionAfterAuto: (item: PermissionItem, pattern?: PermissionPattern) => Promise<void>;
  readonly dismissReviewAfterAuto: (key: string) => void;
  /** App-wide notification pause (HS2-QYA9SC). */
  readonly setNotificationsPaused: (paused: boolean) => void;
  readonly resolvePermission: (
    item: PermissionItem,
    decision: PermissionDecision,
    scope: PermissionScope,
    automatic?: boolean,
    grouped?: boolean,
    description?: string,
    pattern?: PermissionPattern,
  ) => Promise<void>;
  /** Forget one remembered permission answer from Settings (HS2-JNHAYB). */
  readonly revokePermissionRule: (projectId: string, key: string) => Promise<void>;
  readonly changePermissionRuleDecision: (projectId: string, key: string, decision: 'allow' | 'deny') => Promise<void>;
  readonly changePermissionRuleScope: (projectId: string, key: string, scope: 'session' | 'always') => Promise<void>;
  readonly changePermissionRulePattern: (
    projectId: string,
    key: string,
    pattern: PermissionPattern | 'exact',
  ) => Promise<void>;
  readonly openPermissionRuleEditor: (projectId: string, key: string) => void;
  readonly closePermissionRuleEditor: (projectId: string) => boolean;
  readonly changePermissionRuleEditorKind: (projectId: string, kind: 'exact' | 'template' | 'regex') => void;
  readonly changePermissionRuleEditorExpression: (projectId: string, expression: string) => void;
  readonly savePermissionRuleEditor: (projectId: string) => Promise<void>;
  readonly openPermissionRules: (projectId: string) => void;
  /** Reload a list whose refresh failed, without resending any change (HS2-9YVX6Q). */
  readonly retryPermissionRules: (projectId: string) => Promise<void>;
  readonly openLegacyPermissionDiscard: (projectId: string, sourceId: string) => Promise<void>;
  readonly closeLegacyPermissionDiscard: () => void;
  readonly applyLegacyPermissionDiscard: () => Promise<void>;
  readonly closePermissionRules: () => void;
  readonly searchPermissionRules: (projectId: string, query: string) => void;
  readonly filterPermissionRules: (projectId: string, filters: Partial<PermissionRuleFilters>) => void;
  readonly loadMorePermissionRules: (projectId: string) => void;
  readonly togglePermissionRuleAction: (projectId: string, key: string, surface?: 'preview' | 'dialog') => void;
  readonly selectPermissionRuleRow: (
    projectId: string,
    key: string,
    modifiers: { toggle: boolean; range: boolean },
    surface?: 'preview' | 'dialog',
  ) => void;
  readonly selectVisiblePermissionRules: (projectId: string, surface?: 'preview' | 'dialog') => void;
  readonly permissionRuleActionsToCopy: (projectId: string, surface: 'preview' | 'dialog', key?: string) => string;
  readonly clearPermissionRuleSelection: (projectId: string, surface?: 'preview' | 'dialog') => void;
  readonly requestDeleteSelectedPermissionRules: (
    projectId: string,
    confirming: boolean,
    surface?: 'preview' | 'dialog',
  ) => void;
  readonly applySelectedPermissionRules: (
    projectId: string,
    operation: 'delete' | 'allow' | 'deny' | 'session' | 'always',
    surface?: 'preview' | 'dialog',
  ) => Promise<void>;
  readonly error: Signal<string>;
  readonly refreshError: Signal<string>;
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
  const lifetime = createScope();
  const {
    notificationView,
    project,
    permissionTimer,
    permissionAutomationByProject,
    updatePermissionTimer,
    permissionRevision,
    permissionInbox,
    pendingPermissions,
    reviewAfterAuto,
    rememberPermissionAfterAuto,
    dismissReviewAfterAuto,
    resolvePermission,
    revokePermissionRule,
    changePermissionRuleDecision,
    changePermissionRuleScope,
    openPermissionRuleEditor,
    closePermissionRuleEditor,
    changePermissionRuleEditorKind,
    changePermissionRuleEditorExpression,
    savePermissionRuleEditor,
    openPermissionRules,
    retryPermissionRules,
    openLegacyPermissionDiscard,
    closeLegacyPermissionDiscard,
    applyLegacyPermissionDiscard,
    closePermissionRules,
    searchPermissionRules,
    filterPermissionRules,
    loadMorePermissionRules,
    selectPermissionRuleRow,
    togglePermissionRuleAction,
    selectVisiblePermissionRules,
    permissionRuleActionsToCopy,
    clearPermissionRuleSelection,
    requestDeleteSelectedPermissionRules,
    applySelectedPermissionRules,
    error,
    refreshError,
    selectedProjectId,
    hideVerifiedByProject,
    selectLinkedTicket,
    ticketLinkChoice,
    openTicketLinkMatch,
    cancelTicketLinkChoice,
  } = dependencies;
  const permissionRuleEditorFocus = createPermissionRuleEditorFocus(document, () => selectedProjectId.value);
  lifetime.add(permissionRuleEditorFocus.dispose);
  const heldReviewItem = (key: string | undefined) => {
    const item = reviewAfterAuto.value?.item;
    return item?.key === key ? item : undefined;
  };
  const permissionSelectionSurface = (target: Element): 'preview' | 'dialog' =>
    target.closest<HTMLElement>('[data-permission-selection-surface]')?.dataset.permissionSelectionSurface === 'preview'
      ? 'preview'
      : 'dialog';
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.selectNotificationView.selector,
      (_event, target) => {
        notificationView.value = (data(target).itemId ?? 'pending') as NotificationView;
      },
    ),
  );
  lifetime.add(
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
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.pauseNotifications.selector, () => {
      dependencies.setNotificationsPaused(true);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.resumeNotifications.selector, () => {
      dependencies.setNotificationsPaused(false);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.ignorePermission.selector, (_event, target) => {
      const key = data(target).requestKey;
      if (!key) return;
      permissionTimer.hide();
      permissionInbox.ignore(key);
      updatePermissionTimer();
      permissionRevision.value += 1;
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.cancelPermissionAutomation.selector,
      (event, target) => {
        event.stopImmediatePropagation();
        const key = data(target).requestKey;
        if (!key) return;
        permissionTimer.cancel(key);
        dependencies.permissionCountdown = undefined;
        permissionRevision.value += 1;
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.resolvePermission.selector, (_event, target) => {
      const key = data(target).requestKey,
        item = pendingPermissions().find((value) => value.key === key) ?? heldReviewItem(key);
      if (item) {
        const scope = data(target).scope as PermissionScope;
        if ((scope === 'chat' || scope === 'always') && target.getAttribute('data-rule-confirmed') !== 'true') {
          setRememberedPermissionPreview(item.key, scope, item.action);
          permissionRevision.value += 1;
          return;
        }
        if ((scope === 'chat' || scope === 'always') && rememberedPermissionPreview(item.key)?.scope !== scope) return;
        if (reviewAfterAuto.value?.item.key === item.key) {
          if (scope === 'always') void rememberPermissionAfterAuto(item, reviewedPermissionPattern(item.key));
          return;
        }
        void resolvePermission(
          item,
          data(target).decision as PermissionDecision,
          scope,
          false,
          true,
          undefined,
          reviewedPermissionPattern(item.key),
        );
      }
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', '[data-permission-pattern-kind]', (_event, target) => {
      const control = target as HTMLSelectElement,
        key = control.dataset.permissionPatternKind;
      if (!key) return;
      const current = rememberedPermissionPreview(key);
      if (!current) return;
      const kind = control.value as 'exact' | 'template' | 'regex';
      const action = (pendingPermissions().find((item) => item.key === key) ?? heldReviewItem(key))?.action ?? '';
      setRememberedPermissionPattern(
        key,
        kind,
        kind === 'regex' ? `^${action.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$` : action,
      );
      permissionRevision.value += 1;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', '[data-permission-suggestion-index]', (_event, target) => {
      const key = data(target).requestKey;
      const index = Number((target as HTMLElement).dataset.permissionSuggestionIndex);
      const item = pendingPermissions().find((value) => value.key === key) ?? heldReviewItem(key);
      if (!item || !Number.isInteger(index)) return;
      toggleRememberedPermissionSuggestion(item, index);
      permissionRevision.value += 1;
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', '[data-permission-pattern-expression]', (_event, target) => {
      const control = target as HTMLTextAreaElement,
        key = control.dataset.permissionPatternExpression;
      if (key) setRememberedPermissionPattern(key, rememberedPermissionPreview(key)?.kind ?? 'template', control.value);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', '[data-permission-insert-placeholder]', (_event, target) => {
      const key = (target as HTMLElement).dataset.permissionInsertPlaceholder;
      const field =
        key &&
        [...document.querySelectorAll<HTMLTextAreaElement>('[data-permission-pattern-expression]')].find(
          (item) => item.dataset.permissionPatternExpression === key,
        );
      if (!key || !field || field.selectionStart === field.selectionEnd) return;
      const start = field.selectionStart,
        end = field.selectionEnd;
      field.value = `${field.value.slice(0, start)}{{token}}${field.value.slice(end)}`;
      setRememberedPermissionPattern(key, 'template', field.value);
      field.focus();
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.openPermissionRuleEditor.selector,
      (_event, target) => {
        const key = data(target).ruleKey;
        if (key) {
          permissionRuleEditorFocus.remember(target as HTMLElement);
          openPermissionRuleEditor(selectedProjectId.value, key);
        }
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.changePermissionRuleEditorKind.selector,
      (_event, target) => {
        const kind = (target as HTMLElement).dataset.segmentValue;
        if (kind === 'exact' || kind === 'template' || kind === 'regex')
          changePermissionRuleEditorKind(selectedProjectId.value, kind);
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'input',
      NOTIFICATIONS_AND_LINKS_TARGETS.permissionRuleEditorPattern.selector,
      (_event, target) => {
        changePermissionRuleEditorExpression(selectedProjectId.value, (target as HTMLTextAreaElement).value);
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.savePermissionRuleEditor.selector, () => {
      const projectId = selectedProjectId.value,
        opener = permissionRuleEditorFocus.snapshot();
      void savePermissionRuleEditor(projectId).then(() => {
        if (selectedProjectId.value === projectId) permissionRuleEditorFocus.restore(opener);
      });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.closePermissionRuleEditor.selector, () => {
      if (closePermissionRuleEditor(selectedProjectId.value)) permissionRuleEditorFocus.restore();
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.openLegacyPermissionDiscard.selector,
      (_event, target) => {
        const sourceId = data(target).sourceId;
        if (sourceId) void openLegacyPermissionDiscard(selectedProjectId.value, sourceId);
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.closeLegacyPermissionDiscard.selector, () => {
      closeLegacyPermissionDiscard();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.applyLegacyPermissionDiscard.selector, () => {
      void applyLegacyPermissionDiscard();
    }),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'kui-request-close',
      NOTIFICATIONS_AND_LINKS_TARGETS.legacyPermissionDialog.selector,
      () => {
        closeLegacyPermissionDiscard();
      },
    ),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'kui-request-close',
      NOTIFICATIONS_AND_LINKS_TARGETS.permissionRuleEditor.selector,
      (event) => {
        if (!closePermissionRuleEditor(selectedProjectId.value)) event.preventDefault();
        else permissionRuleEditorFocus.restore();
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.cancelRememberedPermission.selector,
      (_event, target) => {
        const key = data(target).requestKey;
        if (!key) return;
        setRememberedPermissionPreview(key);
        dismissReviewAfterAuto(key);
        permissionRevision.value += 1;
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.copyPermissionRuleAction.selector,
      (_event, target) => {
        const action = permissionRuleActionsToCopy(
          selectedProjectId.value,
          permissionSelectionSurface(target),
          data(target).ruleKey,
        );
        if (action)
          void navigator.clipboard.writeText(action).catch(() => {
            error.value = 'Could not copy the action.';
          });
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.copySelectedPermissionRuleActions.selector,
      (_event, target) => {
        const actions = permissionRuleActionsToCopy(selectedProjectId.value, permissionSelectionSurface(target));
        if (actions)
          void navigator.clipboard.writeText(actions).catch(() => {
            error.value = 'Could not copy the selected actions.';
          });
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'contextmenu', '[data-permission-rule]', (event, target) => {
      if (target.getAttribute('data-rule-disabled') === 'true') return;
      const key = target.getAttribute('data-permission-rule');
      if (!key) return;
      event.preventDefault();
      const pointer = event as MouseEvent;
      if (target.getAttribute('data-rule-selected') !== 'true')
        selectPermissionRuleRow(
          selectedProjectId.value,
          key,
          { toggle: false, range: false },
          permissionSelectionSurface(target),
        );
      const { clientX, clientY } = pointer;
      queueMicrotask(() => {
        const row = [...document.querySelectorAll<HTMLElement>('[data-permission-rule]')].find(
          (item) =>
            item.getAttribute('data-permission-rule') === key &&
            permissionSelectionSurface(item) === permissionSelectionSurface(target),
        );
        const menu = row?.querySelector<HTMLElement>('[data-permission-rule-menu]');
        if (menu) openPopupMenuAt(menu, clientX, clientY);
      });
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'keydown',
      '[data-permission-rule] [data-action="select-permission-rule-row"]',
      (event, target) => {
        const keyboard = event as KeyboardEvent;
        if (keyboard.key !== 'ContextMenu' && !(keyboard.shiftKey && keyboard.key === 'F10')) return;
        const menu = target
          .closest('[data-permission-rule]')
          ?.querySelector<HTMLElement>('[data-permission-rule-menu]');
        if (!menu) return;
        event.preventDefault();
        const row = target.closest<HTMLElement>('[data-permission-rule]');
        const key = row?.getAttribute('data-permission-rule');
        if (key && row && row.getAttribute('data-rule-selected') !== 'true')
          selectPermissionRuleRow(
            selectedProjectId.value,
            key,
            { toggle: false, range: false },
            permissionSelectionSurface(row),
          );
        const rect = target.getBoundingClientRect();
        const surface = permissionSelectionSurface(row ?? target);
        queueMicrotask(() => {
          const current = [...document.querySelectorAll<HTMLElement>('[data-permission-rule]')].find(
            (item) => item.getAttribute('data-permission-rule') === key && permissionSelectionSurface(item) === surface,
          );
          const currentMenu = current?.querySelector<HTMLElement>('[data-permission-rule-menu]');
          if (currentMenu) openPopupMenuAt(currentMenu, rect.left + 20, rect.top + 20);
        });
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'dblclick',
      NOTIFICATIONS_AND_LINKS_ACTIONS.selectPermissionRuleRow.selector,
      (_event, target) => {
        const key = data(target).ruleKey;
        if (!key) return;
        permissionRuleEditorFocus.remember(target as HTMLElement);
        openPermissionRuleEditor(selectedProjectId.value, key);
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.revokePermissionRule.selector,
      (_event, target) => {
        const key = data(target).ruleKey;
        if (key) void revokePermissionRule(selectedProjectId.value, key);
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.changePermissionRuleDecision.selector,
      (_event, target) => {
        const key = data(target).ruleKey,
          decision = data(target).decision;
        if (key && (decision === 'allow' || decision === 'deny'))
          void changePermissionRuleDecision(selectedProjectId.value, key, decision);
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.changePermissionRuleScope.selector,
      (_event, target) => {
        const { ruleKey, scope } = data(target);
        if (ruleKey && (scope === 'session' || scope === 'always'))
          void changePermissionRuleScope(selectedProjectId.value, ruleKey, scope);
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.openPermissionRules.selector, () => {
      openPermissionRules(selectedProjectId.value);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.retryPermissionRules.selector, () => {
      void retryPermissionRules(selectedProjectId.value);
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'input',
      NOTIFICATIONS_AND_LINKS_TARGETS.permissionRulesSearch.selector,
      (_event, target) => {
        searchPermissionRules(selectedProjectId.value, (target as Control).value);
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.loadMorePermissionRules.selector, () => {
      loadMorePermissionRules(selectedProjectId.value);
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.filterPermissionRuleLifetime.selector,
      (_event, target) => {
        const lifetime = (target as HTMLElement).dataset.segmentValue;
        if (lifetime === 'permanent' || lifetime === 'session')
          filterPermissionRules(selectedProjectId.value, { lifetime });
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.filterPermissionRuleDecision.selector,
      (_event, target) => {
        const decision = (target as HTMLElement).dataset.segmentValue;
        if (decision === 'allow' || decision === 'deny') filterPermissionRules(selectedProjectId.value, { decision });
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.togglePermissionRuleAction.selector,
      (_event, target) => {
        const key = data(target).ruleKey;
        if (key) togglePermissionRuleAction(selectedProjectId.value, key, permissionSelectionSurface(target));
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.selectPermissionRuleRow.selector,
      (event, target) => {
        const key = data(target).ruleKey;
        if (key) {
          const pointer = event as MouseEvent;
          selectPermissionRuleRow(
            selectedProjectId.value,
            key,
            {
              toggle: pointer.metaKey || pointer.ctrlKey,
              range: pointer.shiftKey,
            },
            permissionSelectionSurface(target),
          );
        }
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'keydown', '.permission-rules-settings__all', (event) => {
      const keyboard = event as KeyboardEvent;
      if (!(keyboard.metaKey || keyboard.ctrlKey) || keyboard.altKey || keyboard.key.toLowerCase() !== 'a') return;
      keyboard.preventDefault();
      selectVisiblePermissionRules(selectedProjectId.value);
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', '.permission-rules-settings__recent', (event) => {
      const keyboard = event as KeyboardEvent;
      if (!(keyboard.metaKey || keyboard.ctrlKey) || keyboard.altKey || keyboard.key.toLowerCase() !== 'a') return;
      keyboard.preventDefault();
      selectVisiblePermissionRules(selectedProjectId.value, 'preview');
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.clearPermissionRuleSelection.selector,
      (_event, target) => {
        clearPermissionRuleSelection(selectedProjectId.value, permissionSelectionSurface(target));
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.editSelectedPermissionRules.selector,
      (_event, target) => {
        const decision = data(target).decision;
        if (decision === 'allow' || decision === 'deny')
          void applySelectedPermissionRules(selectedProjectId.value, decision, permissionSelectionSurface(target));
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.requestDeleteSelectedPermissionRules.selector,
      (_event, target) => {
        requestDeleteSelectedPermissionRules(selectedProjectId.value, true, permissionSelectionSurface(target));
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.changeSelectedPermissionRuleScope.selector,
      (_event, target) => {
        const scope = data(target).scope;
        if (scope === 'session' || scope === 'always')
          void applySelectedPermissionRules(selectedProjectId.value, scope, permissionSelectionSurface(target));
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.cancelDeleteSelectedPermissionRules.selector,
      (_event, target) => {
        requestDeleteSelectedPermissionRules(selectedProjectId.value, false, permissionSelectionSurface(target));
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.confirmDeleteSelectedPermissionRules.selector,
      (_event, target) => {
        void applySelectedPermissionRules(selectedProjectId.value, 'delete', permissionSelectionSurface(target));
      },
    ),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'kui-request-close',
      NOTIFICATIONS_AND_LINKS_TARGETS.permissionRulesDialog.selector,
      () => {
        closePermissionRules();
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.dismissAppError.selector, () => {
      error.value = '';
      refreshError.value = '';
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'change',
      NOTIFICATIONS_AND_LINKS_ACTIONS.toggleVerifiedColumn.selector,
      (_event, target) => {
        const id = selectedProjectId.value;
        if (!id) return;
        const checked = (target as HTMLInputElement).checked;
        hideVerifiedByProject.value = { ...hideVerifiedByProject.value, [id]: checked };
        localStorage.setItem(`hotsheet.project.${id}.hide-verified-column`, String(checked));
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.openLinkedTicket.selector, (event, target) => {
      event.preventDefault();
      const slug = data(target).ticketSlug;
      if (slug) {
        dependencies.ticketLinkReturnFocus = target as HTMLElement;
        void selectLinkedTicket(slug, data(target).ticketProjectId);
      }
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.selectTicketLinkMatch.selector,
      (_event, target) => {
        const choice = ticketLinkChoice.value,
          key = data(target).matchKey,
          match = choice?.matches.find((item) => ticketLinkMatchKey(item) === key);
        if (match) void openTicketLinkMatch(match);
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', NOTIFICATIONS_AND_LINKS_ACTIONS.cancelTicketLinkChoice.selector, () => {
      cancelTicketLinkChoice();
    }),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'kui-request-close',
      NOTIFICATIONS_AND_LINKS_TARGETS.ticketLinkChoiceDialog.selector,
      () => {
        if (ticketLinkChoice.value) cancelTicketLinkChoice();
      },
    ),
  );
  return () => {
    lifetime.dispose();
  };
}
