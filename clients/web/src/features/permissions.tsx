import type { Signal } from 'kerfjs';
import { signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { Api, ApiHttpError } from '../api';
import { clearSelectedBashDescription, updatePermissionCountdownText } from '../components/permission-request-card';
import {
  PERMISSION_RULE_BATCH_SIZE,
  type PermissionRuleFilters,
  permissionRulesList,
} from '../components/permission-rules-settings';
import { PermissionPopupSurface } from '../components/reader-overlay-surfaces';
import { beginInteractionTiming } from '../interaction-performance';
import type { Project } from '../interactions/types';
import {
  loadNotificationsPaused,
  notificationsPausedFromStorageEvent,
  saveNotificationsPaused,
} from '../notification-pause';
import { createPermissionDecisionWorkflow } from '../permission-decision-workflow';
import {
  allowsImmediately,
  DEFAULT_PERMISSION_AUTOMATION,
  formatPermissionCountdown,
  parsePermissionAutomation,
  parsePermissionHistory,
  type PermissionAutomation,
  PermissionInbox,
  type PermissionRule,
  permissionRuleKey,
  projectPermissionRules,
  VisiblePermissionTimer,
} from '../permission-notifications';

export interface PermissionsDependencies {
  projects: Signal<Project[]>;
  selectedProjectId: Signal<string>;
}

export function createPermissionsController(dependencies: PermissionsDependencies) {
  const { projects, selectedProjectId } = dependencies;
  function loadPermissionHistory() {
    try {
      return parsePermissionHistory(JSON.parse(localStorage.getItem('hotsheet.permission-history') || '[]'));
    } catch {
      return [];
    }
  }
  function loadPermissionAutomation(projectId: string) {
    try {
      return parsePermissionAutomation(
        JSON.parse(localStorage.getItem(`hotsheet.project.${projectId}.permission-automation`) || 'null'),
      );
    } catch {
      return DEFAULT_PERMISSION_AUTOMATION;
    }
  }
  const storedPermissionHistory = loadPermissionHistory();
  // Owns the countdown interval and cross-window storage listener (HS2-30J7W5).
  let lifetime = createScope();
  const permissionInbox = new PermissionInbox(storedPermissionHistory),
    permissionTimer = new VisiblePermissionTimer();
  const permissionAutomationByProject = signal<Record<string, PermissionAutomation>>({});
  // Remembered answers per project for Settings → Permissions (HS2-JNHAYB). Loaded when that view
  // shows a project whose list is missing or stale; any permission resolution marks every project
  // stale, and a `permission_rules_changed` event from another client marks its project stale
  // (HS2-KYK1S7). Freshness is reactive, so a shown list refetches once when it goes stale. Never polled.
  const permissionRulesByProject = signal<Record<string, PermissionRule[]>>({}),
    permissionRulesMessage = signal(''),
    revokingPermissionRules = signal<readonly string[]>([]),
    permissionRulesDialog = signal<
      | {
          projectId: string;
          query: string;
          visibleCount: number;
          lifetime: PermissionRuleFilters['lifetime'];
          decision: PermissionRuleFilters['decision'];
          selected?: readonly string[];
          selectionAnchor?: string;
          busy?: boolean;
          confirmingDelete?: boolean;
        }
      | undefined
    >(undefined),
    permissionRulesPreview = signal<
      | {
          projectId: string;
          selected: readonly string[];
          selectionAnchor?: string;
          busy?: boolean;
          confirmingDelete?: boolean;
        }
      | undefined
    >(undefined);
  const loadingPermissionRules = new Set<string>();
  // App-wide pause (HS2-QYA9SC): no popup in any project while set, so its countdown also freezes.
  const notificationsPaused = signal(loadNotificationsPaused());
  let permissionTimerInterval: number | undefined,
    permissionCountdown: { key: string; remainingMs: number } | undefined,
    allowingImmediately = false;
  const pendingPermissions = () => permissionInbox.pending();
  const permissionHistory = () => permissionInbox.history();
  const projectPendingPermissions = (projectId = selectedProjectId.value) =>
    pendingPermissions().filter((item) => item.projectId === projectId);
  const projectPermissionHistory = (projectId = selectedProjectId.value) =>
    permissionHistory().filter((item) => item.projectId === projectId);
  const visiblePermission = () => (notificationsPaused.value ? undefined : permissionInbox.visible());
  const permissionCount = (projectId?: string) =>
    pendingPermissions().filter((item) => !projectId || item.projectId === projectId).length;
  const permissionAutomation = (projectId: string) =>
    permissionAutomationByProject.value[projectId] ?? DEFAULT_PERMISSION_AUTOMATION;
  const persistPermissionHistory = () => {
    localStorage.setItem('hotsheet.permission-history', JSON.stringify(permissionInbox.history()));
  };
  const decisionWorkflow = createPermissionDecisionWorkflow({
    projects,
    inbox: permissionInbox,
    timer: permissionTimer,
    persistHistory: persistPermissionHistory,
    updateTimer: () => {
      updatePermissionTimer();
    },
    clearDescription: clearSelectedBashDescription,
    beginTiming: (item) => beginInteractionTiming('permission-decision', { project: item.projectId }),
  });
  const {
    permissionRevision,
    permissionResolutionErrors,
    freshPermissionRules,
    markPermissionRulesStale,
    refreshPermissions,
    resolvePermission,
    serverResolvedPermission,
  } = decisionWorkflow;

  /**
   * Start the local countdown and reconcile once. Later changes arrive only through the
   * project change stream (`permission_asked` / `permission_resolved`) and its resync hook
   * after a reconnect or overflow; there is no network polling timer (HS2-NKCXW4). The
   * 1 s interval below only re-renders the local countdown and never touches the network.
   */
  function setNotificationsPaused(paused: boolean, persist = true) {
    if (persist) saveNotificationsPaused(paused);
    if (notificationsPaused.value === paused) return;
    notificationsPaused.value = paused;
    // Hide (and freeze) or show the popup now instead of on the next countdown tick.
    updatePermissionTimer();
  }

  function startPermissionUpdates() {
    if (permissionTimerInterval === undefined) {
      const interval = window.setInterval(updatePermissionTimer, 1_000);
      permissionTimerInterval = interval;
      // Another window paused or resumed; follow it without writing the key back.
      const sync = (event: StorageEvent) => {
        const paused = notificationsPausedFromStorageEvent(event);
        if (paused !== undefined) setNotificationsPaused(paused, false);
      };
      window.addEventListener('storage', sync);
      lifetime.add(() => {
        window.clearInterval(interval);
        window.removeEventListener('storage', sync);
        permissionTimerInterval = undefined;
      });
    }
    void refreshPermissions();
  }

  /**
   * Resolve every pending request whose project is set to Auto-allow after 0 seconds (HS2-EBGCGW)
   * before anything renders, so its popup is never presented. Each decision still goes through
   * {@link resolvePermission}: it is sent to the server and recorded in history as automatic.
   * Ignored requests and requests whose automation was stopped (or whose automatic send failed)
   * keep waiting for a manual decision.
   */
  function allowImmediatePermissions() {
    if (allowingImmediately) return;
    allowingImmediately = true;
    try {
      for (const item of pendingPermissions())
        if (
          !item.ignored &&
          !permissionTimer.isCancelled(item.key) &&
          allowsImmediately(permissionAutomation(item.projectId))
        )
          void resolvePermission(item, 'allow', 'once', true);
    } finally {
      allowingImmediately = false;
    }
  }

  function updatePermissionTimer() {
    allowImmediatePermissions();
    const item = visiblePermission();
    if (!item) {
      permissionTimer.hide();
      permissionCountdown = undefined;
      return;
    }
    const setting = permissionAutomation(item.projectId);
    if (setting.action === 'off') {
      permissionTimer.hide();
      permissionCountdown = undefined;
      return;
    }
    const remaining = permissionTimer.tick(item.key, setting.delayMs);
    permissionCountdown = remaining === undefined ? undefined : { key: item.key, remainingMs: remaining };
    if (remaining !== undefined)
      updatePermissionCountdownText(document, item.key, formatPermissionCountdown(remaining));
    if (remaining === 0) void resolvePermission(item, setting.action, 'once', true);
  }

  /**
   * The visible request's popup. The AI conversation's foreground lays out the default `flow` copy
   * (HS2-M2W2DP); the shell passes `top` so its popup paints in the top layer (HS2-ZESCM2).
   */
  function permissionPopupSurface(layer: 'flow' | 'top' = 'flow') {
    const permission = visiblePermission();
    if (!permission) return undefined;
    const automation = permissionAutomation(permission.projectId),
      countdown =
        permissionCountdown?.key === permission.key
          ? formatPermissionCountdown(permissionCountdown.remainingMs)
          : undefined,
      error = permissionResolutionErrors.value[permission.key];
    return (
      <PermissionPopupSurface
        popup={{
          layer,
          item: permission,
          state: error ? 'failed' : 'pending',
          error,
          countdown,
          countdownAction: automation.action === 'off' ? undefined : automation.action,
          groupCount: permissionInbox.identical(permission.key).length,
        }}
      />
    );
  }

  function permissionRules(projectId = selectedProjectId.value) {
    const project = projects.value.find((value) => value.id === projectId);
    if (project && !freshPermissionRules.value.has(projectId) && !loadingPermissionRules.has(projectId))
      queueMicrotask(() => void loadPermissionRules(project));
    return permissionRulesByProject.value[projectId] ?? [];
  }

  function permissionRulesDialogFor(projectId: string, visible: boolean) {
    const current = permissionRulesDialog.value;
    if (current && (!visible || current.projectId !== projectId)) permissionRulesDialog.value = undefined;
    return visible && current?.projectId === projectId ? current : undefined;
  }

  function permissionRulesPreviewFor(projectId: string, visible: boolean) {
    const current = permissionRulesPreview.value;
    if (current && (!visible || current.projectId !== projectId)) permissionRulesPreview.value = undefined;
    return visible && current?.projectId === projectId ? current : undefined;
  }

  function openPermissionRules(projectId: string) {
    const preview = permissionRulesPreview.value?.projectId === projectId ? permissionRulesPreview.value : undefined;
    permissionRulesDialog.value = {
      projectId,
      query: '',
      visibleCount: PERMISSION_RULE_BATCH_SIZE,
      lifetime: 'permanent',
      decision: 'allow',
      selected: preview?.selected ?? [],
    };
    permissionRulesPreview.value = undefined;
  }

  function closePermissionRules() {
    permissionRulesDialog.value = undefined;
  }

  function searchPermissionRules(projectId: string, query: string) {
    if (permissionRulesDialog.value?.projectId === projectId)
      permissionRulesDialog.value = {
        ...permissionRulesDialog.value,
        query,
        visibleCount: PERMISSION_RULE_BATCH_SIZE,
        selectionAnchor: undefined,
        confirmingDelete: false,
      };
  }

  function filterPermissionRules(projectId: string, filters: Partial<PermissionRuleFilters>) {
    const current = permissionRulesDialog.value;
    if (current?.projectId === projectId && !current.busy)
      permissionRulesDialog.value = {
        ...current,
        ...filters,
        visibleCount: PERMISSION_RULE_BATCH_SIZE,
        selectionAnchor: undefined,
        confirmingDelete: false,
      };
  }

  function loadMorePermissionRules(projectId: string) {
    const current = permissionRulesDialog.value;
    if (current?.projectId !== projectId || current.busy) return;
    const rules = permissionRulesByProject.value[projectId] ?? [],
      visibleCount = permissionRulesList(
        rules,
        current.query,
        current.visibleCount + PERMISSION_RULE_BATCH_SIZE,
        current,
      ).visibleCount;
    permissionRulesDialog.value = { ...current, visibleCount };
  }

  function selectPermissionRuleRow(
    projectId: string,
    key: string,
    modifiers: { toggle: boolean; range: boolean },
    surface: 'preview' | 'dialog' = 'dialog',
  ) {
    const state = surface === 'preview' ? permissionRulesPreview.value : permissionRulesDialog.value;
    if (state?.projectId !== projectId && surface === 'dialog') return;
    if (state?.busy) return;
    const rules = permissionRulesByProject.value[projectId] ?? [],
      rows = (
        surface === 'preview'
          ? permissionRulesList(rules, '', PERMISSION_RULE_BATCH_SIZE).rows.slice(0, 3)
          : permissionRulesList(
              rules,
              permissionRulesDialog.value?.query ?? '',
              permissionRulesDialog.value?.visibleCount ?? PERMISSION_RULE_BATCH_SIZE,
              permissionRulesDialog.value,
            ).rows
      ).map(permissionRuleKey),
      index = rows.indexOf(key);
    if (index < 0) return;
    const selected = new Set(modifiers.toggle || modifiers.range ? (state?.selected ?? []) : []);
    if (modifiers.range && state?.selectionAnchor && rows.includes(state.selectionAnchor)) {
      const anchorIndex = rows.indexOf(state.selectionAnchor);
      for (const rowKey of rows.slice(Math.min(anchorIndex, index), Math.max(anchorIndex, index) + 1))
        selected.add(rowKey);
    } else if (modifiers.toggle && selected.has(key)) selected.delete(key);
    else selected.add(key);
    permissionRulesMessage.value = '';
    const selectionAnchor =
      modifiers.range && rows.includes(state?.selectionAnchor ?? '') ? state?.selectionAnchor : key;
    if (surface === 'preview')
      permissionRulesPreview.value = {
        ...state,
        projectId,
        selected: [...selected],
        selectionAnchor,
        confirmingDelete: false,
      };
    else
      permissionRulesDialog.value = {
        ...permissionRulesDialog.value!,
        selected: [...selected],
        selectionAnchor,
        confirmingDelete: false,
      };
  }

  function selectVisiblePermissionRules(projectId: string, surface: 'preview' | 'dialog' = 'dialog') {
    if (surface === 'preview') {
      const current =
        permissionRulesPreview.value?.projectId === projectId
          ? permissionRulesPreview.value
          : { projectId, selected: [] as readonly string[], busy: false };
      if (current.busy) return;
      const selected = new Set(current.selected);
      for (const rule of permissionRulesList(
        permissionRulesByProject.value[projectId] ?? [],
        '',
        PERMISSION_RULE_BATCH_SIZE,
      ).rows.slice(0, 3))
        selected.add(permissionRuleKey(rule));
      permissionRulesPreview.value = { ...current, selected: [...selected], confirmingDelete: false };
      return;
    }
    const dialog = permissionRulesDialog.value;
    if (dialog?.projectId !== projectId || dialog.busy) return;
    const rows = permissionRulesList(
      permissionRulesByProject.value[projectId] ?? [],
      dialog.query,
      dialog.visibleCount,
      dialog,
    ).rows;
    const selected = new Set(dialog.selected ?? []);
    for (const rule of rows) selected.add(permissionRuleKey(rule));
    permissionRulesMessage.value = '';
    permissionRulesDialog.value = { ...dialog, selected: [...selected], confirmingDelete: false };
  }

  function clearPermissionRuleSelection(projectId: string, surface: 'preview' | 'dialog' = 'dialog') {
    if (surface === 'preview') {
      const current = permissionRulesPreview.value;
      if (current?.projectId === projectId && !current.busy) {
        permissionRulesMessage.value = '';
        permissionRulesPreview.value = {
          ...current,
          selected: [],
          selectionAnchor: undefined,
          confirmingDelete: false,
        };
      }
      return;
    }
    const dialog = permissionRulesDialog.value;
    if (dialog?.projectId === projectId && !dialog.busy) {
      permissionRulesMessage.value = '';
      permissionRulesDialog.value = { ...dialog, selected: [], selectionAnchor: undefined, confirmingDelete: false };
    }
  }

  function requestDeleteSelectedPermissionRules(
    projectId: string,
    confirmingDelete: boolean,
    surface: 'preview' | 'dialog' = 'dialog',
  ) {
    if (surface === 'preview') {
      const current = permissionRulesPreview.value;
      if (current?.projectId === projectId && !current.busy && current.selected.length > 0) {
        permissionRulesMessage.value = '';
        permissionRulesPreview.value = { ...current, confirmingDelete };
      }
      return;
    }
    const dialog = permissionRulesDialog.value;
    if (dialog?.projectId === projectId && !dialog.busy && (dialog.selected?.length ?? 0) > 0) {
      permissionRulesMessage.value = '';
      permissionRulesDialog.value = { ...dialog, confirmingDelete };
    }
  }

  async function applySelectedPermissionRules(
    projectId: string,
    operation: 'delete' | 'allow' | 'deny' | 'session' | 'always',
    surface: 'preview' | 'dialog' = 'dialog',
  ) {
    const dialog = permissionRulesDialog.value,
      preview = permissionRulesPreview.value,
      selection = surface === 'preview' ? preview : dialog,
      project = projects.value.find((value) => value.id === projectId);
    if (!project || selection?.projectId !== projectId || selection.busy || !selection.selected?.length) return;
    if (operation === 'delete' && !selection.confirmingDelete) return;
    const rules = permissionRulesByProject.value[projectId] ?? [],
      selected = new Set(selection.selected),
      scopeChange = operation === 'session' || operation === 'always',
      targets = rules.filter(
        (rule) =>
          selected.has(permissionRuleKey(rule)) &&
          (!scopeChange || (rule.scope !== 'chat' && rule.scope !== operation)),
      ),
      api = new Api(project.apiPath, '', { trackBusy: false });
    if (!targets.length) {
      if (scopeChange) permissionRulesMessage.value = 'No selected answers need that duration change.';
      return;
    }
    if (surface === 'preview') permissionRulesPreview.value = { ...preview!, busy: true, confirmingDelete: false };
    else permissionRulesDialog.value = { ...dialog!, busy: true, confirmingDelete: false };
    permissionRulesMessage.value = '';
    const succeeded = new Set<string>(),
      failed = new Set<string>();
    for (const rule of targets) {
      const key = permissionRuleKey(rule);
      try {
        if (operation === 'delete') await api.revokePermissionRule(rule);
        else if (operation === 'session' || operation === 'always')
          await api.updatePermissionRuleScope(rule, operation);
        else await api.updatePermissionRule(rule, operation);
        succeeded.add(key);
      } catch (reason) {
        if (operation === 'delete' && reason instanceof ApiHttpError && reason.status === 404) succeeded.add(key);
        else failed.add(key);
      }
    }
    permissionRulesByProject.value = {
      ...permissionRulesByProject.value,
      [projectId]: rules.flatMap((rule) => {
        if (!succeeded.has(permissionRuleKey(rule))) return [rule];
        if (operation === 'delete') return [];
        return operation === 'session' || operation === 'always'
          ? [{ ...rule, scope: operation }]
          : [{ ...rule, decision: operation }];
      }),
    };
    let refreshFailed = false;
    try {
      const refreshed = await api.permissionRules();
      permissionRulesByProject.value = {
        ...permissionRulesByProject.value,
        [projectId]: projectPermissionRules(refreshed, project),
      };
      freshPermissionRules.value = new Set([...freshPermissionRules.value, projectId]);
    } catch {
      refreshFailed = true;
      markPermissionRulesStale(projectId);
    }
    function finishSelection() {
      const remaining = new Set((permissionRulesByProject.value[projectId] ?? []).map(permissionRuleKey));
      const selected = [...failed].filter((key) => remaining.has(key));
      if (surface === 'preview') {
        const current = permissionRulesPreview.value;
        if (current?.projectId !== projectId) return false;
        permissionRulesPreview.value = { ...current, busy: false, selected, selectionAnchor: undefined };
      } else {
        const current = permissionRulesDialog.value;
        if (current?.projectId !== projectId) return false;
        permissionRulesDialog.value = {
          ...current,
          busy: false,
          selected,
          selectionAnchor: undefined,
          visibleCount: permissionRulesList(
            permissionRulesByProject.value[projectId] ?? [],
            current.query,
            current.visibleCount,
            current,
          ).visibleCount,
        };
      }
      return true;
    }
    if (finishSelection())
      permissionRulesMessage.value = `${operation === 'delete' ? 'Deleted' : 'Updated'} ${succeeded.size} of ${targets.length} answers.${failed.size ? ` ${failed.size} failed and remain selected for retry.` : ''}${refreshFailed ? ' Could not refresh the list; retry loading to confirm the latest state.' : ''}`;
  }

  /** Another client remembered or revoked one of `projectId`'s answers (HS2-KYK1S7). */
  function permissionRulesChanged(projectId: string) {
    markPermissionRulesStale(projectId);
  }

  /** Fetch one project's remembered answers once; a failure leaves it stale for the next view. */
  async function loadPermissionRules(project: Project) {
    if (freshPermissionRules.peek().has(project.id) || loadingPermissionRules.has(project.id)) return;
    loadingPermissionRules.add(project.id);
    freshPermissionRules.value = new Set([...freshPermissionRules.peek(), project.id]);
    // Settle the loading mark before publishing, so a view re-rendered by the result can refetch at
    // once when a rules change made it stale while this request was in flight (HS2-KYK1S7).
    let rules: PermissionRule[];
    try {
      rules = await new Api(project.apiPath, '', { trackBusy: false }).permissionRules();
    } catch (reason) {
      loadingPermissionRules.delete(project.id);
      permissionRulesMessage.value = `Could not load remembered answers. ${reason instanceof Error ? reason.message : String(reason)}`;
      return;
    }
    loadingPermissionRules.delete(project.id);
    permissionRulesByProject.value = {
      ...permissionRulesByProject.value,
      [project.id]: projectPermissionRules(rules, project),
    };
    const dialog = permissionRulesDialog.value;
    if (dialog?.projectId === project.id)
      permissionRulesDialog.value = {
        ...dialog,
        visibleCount: permissionRulesList(
          permissionRulesByProject.value[project.id],
          dialog.query,
          dialog.visibleCount,
          dialog,
        ).visibleCount,
      };
  }

  /** Change one remembered answer's decision from its row menu (HS2-X4ES6Z). */
  async function changePermissionRuleDecision(projectId: string, key: string, decision: 'allow' | 'deny') {
    const project = projects.value.find((value) => value.id === projectId),
      rule = permissionRules(projectId).find((value) => permissionRuleKey(value) === key);
    if (
      !project ||
      !rule ||
      rule.decision === decision ||
      revokingPermissionRules.value.includes(key) ||
      permissionRulesDialog.value?.busy
    )
      return;
    revokingPermissionRules.value = [...revokingPermissionRules.value, key];
    permissionRulesMessage.value = '';
    try {
      await new Api(project.apiPath).updatePermissionRule(rule, decision);
      permissionRulesByProject.value = {
        ...permissionRulesByProject.value,
        [projectId]: permissionRules(projectId).map((value) =>
          permissionRuleKey(value) === key ? { ...value, decision } : value,
        ),
      };
      if (selectedProjectId.value === projectId)
        permissionRulesMessage.value = `Answer set to ${decision === 'allow' ? 'Allow' : 'Disallow'}.`;
    } catch (reason) {
      if (selectedProjectId.value === projectId)
        permissionRulesMessage.value = `Could not change the answer. ${reason instanceof Error ? reason.message : String(reason)}`;
    } finally {
      revokingPermissionRules.value = revokingPermissionRules.value.filter((value) => value !== key);
    }
  }

  /** Change a project-wide answer's duration from either remembered-answer list (HS2-V95VEB). */
  async function changePermissionRuleScope(projectId: string, key: string, scope: 'session' | 'always') {
    const project = projects.value.find((value) => value.id === projectId),
      rule = permissionRules(projectId).find((value) => permissionRuleKey(value) === key);
    if (
      !project ||
      !rule ||
      rule.scope === 'chat' ||
      rule.scope === scope ||
      revokingPermissionRules.value.includes(key) ||
      permissionRulesDialog.value?.busy
    )
      return;
    revokingPermissionRules.value = [...revokingPermissionRules.value, key];
    permissionRulesMessage.value = '';
    try {
      await new Api(project.apiPath).updatePermissionRuleScope(rule, scope);
      permissionRulesByProject.value = {
        ...permissionRulesByProject.value,
        [projectId]: permissionRules(projectId).map((value) =>
          permissionRuleKey(value) === key ? { ...value, scope } : value,
        ),
      };
      if (selectedProjectId.value === projectId)
        permissionRulesMessage.value = `Answer set to ${scope === 'always' ? 'Persistent' : 'Session (until restart)'}.`;
    } catch (reason) {
      if (selectedProjectId.value === projectId)
        permissionRulesMessage.value = `Could not change how long the answer is remembered. ${reason instanceof Error ? reason.message : String(reason)}`;
    } finally {
      revokingPermissionRules.value = revokingPermissionRules.value.filter((value) => value !== key);
    }
  }

  /** Forget one remembered answer from Settings → Permissions (HS2-JNHAYB). */
  async function revokePermissionRule(projectId: string, key: string) {
    const project = projects.value.find((value) => value.id === projectId),
      rule = permissionRules(projectId).find((value) => permissionRuleKey(value) === key);
    if (!project || !rule) return;
    revokingPermissionRules.value = [...revokingPermissionRules.value, key];
    permissionRulesMessage.value = '';
    try {
      await new Api(project.apiPath).revokePermissionRule(rule);
      permissionRulesByProject.value = {
        ...permissionRulesByProject.value,
        [projectId]: permissionRules(projectId).filter((value) => permissionRuleKey(value) !== key),
      };
    } catch (reason) {
      if (reason instanceof ApiHttpError && reason.status === 404)
        permissionRulesByProject.value = {
          ...permissionRulesByProject.value,
          [projectId]: permissionRules(projectId).filter((value) => permissionRuleKey(value) !== key),
        };
      else
        permissionRulesMessage.value = `Could not revoke the rule. ${reason instanceof Error ? reason.message : String(reason)}`;
    } finally {
      const dialog = permissionRulesDialog.value;
      if (dialog?.projectId === projectId)
        permissionRulesDialog.value = {
          ...dialog,
          visibleCount: permissionRulesList(
            permissionRulesByProject.value[projectId] ?? [],
            dialog.query,
            dialog.visibleCount,
            dialog,
          ).visibleCount,
        };
      revokingPermissionRules.value = revokingPermissionRules.value.filter((value) => value !== key);
    }
  }

  /** Stop the countdown interval and storage listener; a later start re-registers them. */
  function dispose() {
    lifetime.dispose();
    lifetime = createScope();
  }
  return {
    dispose,
    permissionRules,
    permissionRulesChanged,
    permissionRulesMessage,
    revokingPermissionRules,
    revokePermissionRule,
    changePermissionRuleDecision,
    changePermissionRuleScope,
    permissionRulesDialogFor,
    permissionRulesPreviewFor,
    openPermissionRules,
    closePermissionRules,
    searchPermissionRules,
    filterPermissionRules,
    loadMorePermissionRules,
    selectPermissionRuleRow,
    selectVisiblePermissionRules,
    clearPermissionRuleSelection,
    requestDeleteSelectedPermissionRules,
    applySelectedPermissionRules,
    loadPermissionAutomation,
    permissionRevision,
    permissionInbox,
    permissionTimer,
    permissionAutomationByProject,
    pendingPermissions,
    permissionHistory,
    projectPendingPermissions,
    projectPermissionHistory,
    permissionCount,
    permissionAutomation,
    persistPermissionHistory,
    refreshPermissions,
    startPermissionUpdates,
    resolvePermission,
    serverResolvedPermission,
    updatePermissionTimer,
    permissionPopupSurface,
    notificationsPaused,
    setNotificationsPaused,
    get permissionCountdown() {
      return permissionCountdown;
    },
    set permissionCountdown(value: typeof permissionCountdown) {
      permissionCountdown = value;
    },
  };
}
