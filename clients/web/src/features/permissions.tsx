import type { Signal } from 'kerfjs';
import { signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { Api, ApiHttpError } from '../api';
import {
  rememberedPermissionPreview,
  setRememberedPermissionPreview,
  updatePermissionCountdownText,
} from '../components/permission-request-card';
import {
  PERMISSION_RULE_BATCH_SIZE,
  type PermissionRuleEditorState,
  type PermissionRuleFilters,
  type PermissionRuleMatchKind,
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
  type PermissionItem,
  type PermissionPattern,
  type PermissionRule,
  permissionRuleKey,
  projectPermissionRules,
  VisiblePermissionTimer,
} from '../permission-notifications';
import type { LegacyPermissionPreview } from '../server-response-parsers';

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
    lockedPermissionRules = signal<ReadonlySet<string>>(new Set()),
    revokingPermissionRules = signal<readonly string[]>([]),
    permissionRuleEditor = signal<(PermissionRuleEditorState & { projectId: string }) | undefined>(undefined),
    permissionRulesDialog = signal<
      | {
          projectId: string;
          query: string;
          visibleCount: number;
          lifetime: PermissionRuleFilters['lifetime'];
          decision: PermissionRuleFilters['decision'];
          selected?: readonly string[];
          expandedActions?: readonly string[];
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
          expandedActions?: readonly string[];
          selectionAnchor?: string;
          busy?: boolean;
          confirmingDelete?: boolean;
        }
      | undefined
    >(undefined);
  const loadingPermissionRules = new Set<string>();
  const legacyDiscard = signal<
    | {
        projectId: string;
        sourceId: string;
        preview?: LegacyPermissionPreview;
        busy: boolean;
        error?: string;
      }
    | undefined
  >(undefined);

  function legacyDiscardFor(projectId: string) {
    return legacyDiscard.value?.projectId === projectId ? legacyDiscard.value : undefined;
  }
  async function openLegacyPermissionDiscard(projectId: string, sourceId: string) {
    const project = projects.value.find((item) => item.id === projectId);
    if (!project) return;
    legacyDiscard.value = { projectId, sourceId, busy: true };
    try {
      const preview = await new Api(project.apiPath).checkoutLegacyPermissionPreview(projectId, sourceId);
      if (legacyDiscardFor(projectId)?.sourceId === sourceId)
        legacyDiscard.value = { projectId, sourceId, preview, busy: false };
    } catch (reason) {
      if (legacyDiscardFor(projectId)?.sourceId === sourceId)
        legacyDiscard.value = {
          projectId,
          sourceId,
          busy: false,
          error: `Could not inspect legacy answers. ${reason instanceof Error ? reason.message : String(reason)}`,
        };
    }
  }
  function closeLegacyPermissionDiscard() {
    if (!legacyDiscard.value?.busy) legacyDiscard.value = undefined;
  }
  async function applyLegacyPermissionDiscard() {
    const review = legacyDiscard.value;
    const project = projects.value.find((item) => item.id === review?.projectId);
    if (!review?.preview || !project || review.busy) return;
    legacyDiscard.value = { ...review, busy: true, error: undefined };
    try {
      const outcome = await new Api(project.apiPath).discardCheckoutLegacyPermissions(project.id, review.sourceId);
      if (legacyDiscardFor(project.id)?.sourceId !== review.sourceId) return;
      legacyDiscard.value = undefined;
      permissionRulesMessage.value = `${outcome.discarded} legacy answer${outcome.discarded === 1 ? '' : 's'} discarded. Future requests need fresh approval.`;
      markPermissionRulesStale(project.id);
    } catch (reason) {
      if (legacyDiscardFor(project.id)?.sourceId === review.sourceId)
        legacyDiscard.value = {
          ...review,
          busy: false,
          error: `Could not discard legacy answers. ${reason instanceof Error ? reason.message : String(reason)}`,
        };
    }
  }
  // App-wide pause (HS2-QYA9SC): no popup in any project while set, so its countdown also freezes.
  const notificationsPaused = signal(loadNotificationsPaused());
  const reviewAfterAuto = signal<
    { item: PermissionItem; settling: boolean; allowed: boolean; error?: string } | undefined
  >(undefined);
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
    clearDescription: (key) => {
      if (reviewAfterAuto.peek()?.item.key !== key) setRememberedPermissionPreview(key);
    },
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
    if (reviewAfterAuto.value) {
      permissionTimer.hide();
      permissionCountdown = undefined;
      return;
    }
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
    if (remaining === 0) {
      if (setting.action === 'allow' && rememberedPermissionPreview(item.key)?.scope === 'always') {
        reviewAfterAuto.value = { item, settling: true, allowed: false };
        void resolvePermission(item, 'allow', 'once', true, false, undefined, undefined, true).then(() => {
          if (reviewAfterAuto.peek()?.item.key !== item.key) return;
          reviewAfterAuto.value = pendingPermissions().some((pending) => pending.key === item.key)
            ? undefined
            : { item, settling: false, allowed: true };
          permissionRevision.value += 1;
        });
      } else void resolvePermission(item, setting.action, 'once', true);
    }
  }

  async function rememberPermissionAfterAuto(item: PermissionItem, pattern?: PermissionPattern) {
    const review = reviewAfterAuto.peek();
    if (review?.item.key !== item.key || review.settling) return;
    const owning = projects.value.find((project) => project.id === item.projectId);
    if (!owning) return;
    reviewAfterAuto.value = { item, settling: true, allowed: true };
    try {
      await new Api(owning.apiPath).rememberPermissionAfterAllow(item.id, undefined, pattern);
      setRememberedPermissionPreview(item.key);
      reviewAfterAuto.value = undefined;
      markPermissionRulesStale(item.projectId);
      updatePermissionTimer();
    } catch (reason) {
      reviewAfterAuto.value = {
        item,
        settling: false,
        allowed: true,
        error: `Could not remember the rule. ${reason instanceof Error ? reason.message : String(reason)}`,
      };
    }
    permissionRevision.value += 1;
  }

  function dismissReviewAfterAuto(key: string) {
    if (reviewAfterAuto.peek()?.item.key !== key) return;
    reviewAfterAuto.value = undefined;
    updatePermissionTimer();
    permissionRevision.value += 1;
  }

  /**
   * The visible request's popup. The AI conversation's foreground lays out the default `flow` copy
   * (HS2-M2W2DP); the shell passes `top` so its popup paints in the top layer (HS2-ZESCM2).
   */
  function permissionPopupSurface(layer: 'flow' | 'top' = 'flow') {
    const held = notificationsPaused.value ? undefined : reviewAfterAuto.value;
    const permission = held?.item ?? visiblePermission();
    if (!permission) return undefined;
    const automation = permissionAutomation(permission.projectId),
      countdown =
        !held && permissionCountdown?.key === permission.key
          ? formatPermissionCountdown(permissionCountdown.remainingMs)
          : undefined,
      error = held?.error ?? permissionResolutionErrors.value[permission.key];
    return (
      <PermissionPopupSurface
        popup={{
          layer,
          item: permission,
          state: held?.settling ? 'resolving' : error ? 'failed' : 'pending',
          error,
          reviewAlreadyAllowed: held?.allowed,
          countdown,
          countdownAction: held || automation.action === 'off' ? undefined : automation.action,
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

  function permissionRuleEditorFor(projectId: string, visible: boolean) {
    const current = permissionRuleEditor.value;
    if (
      current &&
      (!visible ||
        current.projectId !== projectId ||
        !(permissionRulesByProject.value[projectId] ?? []).some((rule) => permissionRuleKey(rule) === current.ruleKey))
    ) {
      permissionRuleEditor.value = undefined;
      return undefined;
    }
    return visible && current?.projectId === projectId ? current : undefined;
  }

  function openPermissionRuleEditor(projectId: string, key: string) {
    if (permissionRuleEditor.value?.busy || revokingPermissionRules.value.includes(key)) return;
    const rule = (permissionRulesByProject.value[projectId] ?? []).find((rule) => permissionRuleKey(rule) === key);
    if (!rule?.exact_action) return;
    permissionRuleEditor.value = {
      projectId,
      ruleKey: key,
      kind: rule.pattern?.kind ?? 'exact',
      expression: rule.pattern?.expression ?? rule.exact_action,
      busy: false,
    };
  }

  function closePermissionRuleEditor(projectId: string) {
    const current = permissionRuleEditor.value;
    if (current && (current.projectId !== projectId || current.busy)) return false;
    permissionRuleEditor.value = undefined;
    return true;
  }

  function changePermissionRuleEditorKind(projectId: string, kind: PermissionRuleMatchKind) {
    const current = permissionRuleEditor.value;
    if (current?.projectId !== projectId || current.busy || !['exact', 'template', 'regex'].includes(kind)) return;
    permissionRuleEditor.value = { ...current, kind, error: undefined };
  }

  function changePermissionRuleEditorExpression(projectId: string, expression: string) {
    const current = permissionRuleEditor.value;
    if (current?.projectId !== projectId || current.busy) return;
    permissionRuleEditor.value = { ...current, expression, error: undefined };
  }

  async function savePermissionRuleEditor(projectId: string) {
    const current = permissionRuleEditor.value;
    if (current?.projectId !== projectId || current.busy) return;
    if (current.kind !== 'exact' && !current.expression.trim()) {
      permissionRuleEditor.value = { ...current, error: 'Enter a reviewed pattern before saving.' };
      return;
    }
    await changePermissionRulePattern(
      projectId,
      current.ruleKey,
      current.kind === 'exact' ? 'exact' : { version: 1, kind: current.kind, expression: current.expression },
    );
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
      ...(preview?.expandedActions ? { expandedActions: preview.expandedActions } : {}),
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

  function togglePermissionRuleAction(projectId: string, key: string, surface: 'preview' | 'dialog' = 'dialog') {
    const state = surface === 'preview' ? permissionRulesPreview.value : permissionRulesDialog.value;
    if ((state && state.projectId !== projectId) || state?.busy) return;
    if (
      !(permissionRulesByProject.value[projectId] ?? []).some(
        (rule) => permissionRuleKey(rule) === key && rule.exact_action,
      )
    )
      return;
    const expandedActions = new Set(state?.expandedActions ?? []);
    if (expandedActions.has(key)) expandedActions.delete(key);
    else expandedActions.add(key);
    if (surface === 'preview')
      permissionRulesPreview.value = { projectId, selected: [], ...state, expandedActions: [...expandedActions] };
    else if (permissionRulesDialog.value?.projectId === projectId)
      permissionRulesDialog.value = { ...permissionRulesDialog.value, expandedActions: [...expandedActions] };
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

  function permissionRuleActionsToCopy(projectId: string, surface: 'preview' | 'dialog', key?: string) {
    const state = surface === 'preview' ? permissionRulesPreview.value : permissionRulesDialog.value;
    if (state?.projectId !== projectId && !key) return '';
    const selected = new Set(key ? [key] : (state?.selected ?? []));
    return permissionRulesList(permissionRulesByProject.value[projectId] ?? [], '', PERMISSION_RULE_BATCH_SIZE)
      .matches.filter((rule) => selected.has(permissionRuleKey(rule)))
      .map((rule) => rule.exact_action)
      .filter((action): action is string => Boolean(action))
      .join('\n');
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
      permissionRulesByProject.value = { ...permissionRulesByProject.value, [project.id]: [] };
      if (reason instanceof ApiHttpError && reason.status === 503) {
        lockedPermissionRules.value = new Set([...lockedPermissionRules.value, project.id]);
        if (selectedProjectId.value === project.id)
          permissionRulesMessage.value =
            'Remembered answers are locked. Restore the host credential store and encrypted rule file, then reload this page to retry.';
      } else {
        if (selectedProjectId.value === project.id)
          permissionRulesMessage.value = `Could not load remembered answers. ${reason instanceof Error ? reason.message : String(reason)}`;
      }
      return;
    }
    loadingPermissionRules.delete(project.id);
    lockedPermissionRules.value = new Set([...lockedPermissionRules.value].filter((id) => id !== project.id));
    if (selectedProjectId.value === project.id) permissionRulesMessage.value = '';
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
        permissionRulesMessage.value = `Answer set to ${scope === 'always' ? 'Persistent' : 'Until Hot Sheet restarts'}.`;
    } catch (reason) {
      if (selectedProjectId.value === projectId)
        permissionRulesMessage.value = `Could not change how long the answer is remembered. ${reason instanceof Error ? reason.message : String(reason)}`;
    } finally {
      revokingPermissionRules.value = revokingPermissionRules.value.filter((value) => value !== key);
    }
  }

  async function changePermissionRulePattern(projectId: string, key: string, pattern: PermissionPattern | 'exact') {
    const project = projects.value.find((value) => value.id === projectId),
      rule = permissionRules(projectId).find((value) => permissionRuleKey(value) === key);
    if (!project || !rule || revokingPermissionRules.value.includes(key)) return;
    revokingPermissionRules.value = [...revokingPermissionRules.value, key];
    permissionRulesMessage.value = '';
    const currentEditor = permissionRuleEditor.value;
    if (currentEditor?.projectId === projectId && currentEditor.ruleKey === key)
      permissionRuleEditor.value = { ...currentEditor, busy: true, error: undefined };
    try {
      await new Api(project.apiPath).updatePermissionRulePattern(rule, pattern);
      markPermissionRulesStale(projectId);
      await loadPermissionRules(project);
      if (selectedProjectId.value === projectId) permissionRulesMessage.value = 'Reviewed match updated.';
      const editor = permissionRuleEditor.value;
      if (editor?.projectId === projectId && editor.ruleKey === key) permissionRuleEditor.value = undefined;
    } catch (reason) {
      const message = `Could not update the match. ${reason instanceof Error ? reason.message : String(reason)}`;
      if (selectedProjectId.value === projectId) permissionRulesMessage.value = message;
      const editor = permissionRuleEditor.value;
      if (editor?.projectId === projectId && editor.ruleKey === key)
        permissionRuleEditor.value = { ...editor, busy: false, error: message };
    } finally {
      revokingPermissionRules.value = revokingPermissionRules.value.filter((value) => value !== key);
      const editor = permissionRuleEditor.value;
      if (editor?.projectId === projectId && editor.ruleKey === key && editor.busy)
        permissionRuleEditor.value = { ...editor, busy: false };
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
    permissionRuleEditor.value = undefined;
    legacyDiscard.value = undefined;
    lifetime.dispose();
    lifetime = createScope();
  }
  return {
    dispose,
    permissionRules,
    permissionRulesChanged,
    permissionRulesMessage,
    legacyDiscardFor,
    openLegacyPermissionDiscard,
    closeLegacyPermissionDiscard,
    applyLegacyPermissionDiscard,
    lockedPermissionRules,
    revokingPermissionRules,
    revokePermissionRule,
    changePermissionRuleDecision,
    changePermissionRuleScope,
    changePermissionRulePattern,
    permissionRulesDialogFor,
    permissionRuleEditorFor,
    openPermissionRuleEditor,
    closePermissionRuleEditor,
    changePermissionRuleEditorKind,
    changePermissionRuleEditorExpression,
    savePermissionRuleEditor,
    permissionRulesPreviewFor,
    openPermissionRules,
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
    reviewAfterAuto,
    rememberPermissionAfterAuto,
    dismissReviewAfterAuto,
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
