import type { Signal } from 'kerfjs';
import { signal } from 'kerfjs';

import { Api, ApiHttpError } from '../api';
import { updatePermissionCountdownText } from '../components/permission-request-card';
import { PermissionPopupSurface } from '../components/reader-overlay-surfaces';
import { beginInteractionTiming } from '../interaction-performance';
import type { Project } from '../interactions/types';
import {
  loadNotificationsPaused,
  notificationsPausedFromStorageEvent,
  saveNotificationsPaused,
} from '../notification-pause';
import {
  allowsImmediately,
  DEFAULT_PERMISSION_AUTOMATION,
  formatPermissionCountdown,
  parsePermissionAutomation,
  parsePermissionHistory,
  type PermissionAutomation,
  permissionBelongsToProject,
  type PermissionDecision,
  PermissionInbox,
  type PermissionItem,
  type PermissionScope,
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
  const permissionRevision = signal(0),
    permissionInbox = new PermissionInbox(storedPermissionHistory),
    permissionTimer = new VisiblePermissionTimer();
  const permissionAutomationByProject = signal<Record<string, PermissionAutomation>>({});
  const permissionResolutionErrors = signal<Record<string, string>>({});
  // App-wide pause (HS2-QYA9SC): no popup in any project while set, so its countdown also freezes.
  const notificationsPaused = signal(loadNotificationsPaused());
  let permissionPolling = false,
    permissionRefreshRequested = false,
    permissionResolutionEpoch = 0,
    permissionTimerInterval: number | undefined,
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

  async function refreshPermissions() {
    if (permissionPolling) {
      permissionRefreshRequested = true;
      return;
    }
    permissionPolling = true;
    try {
      const resolutionEpoch = permissionResolutionEpoch;
      const results = await Promise.all(
        projects.value.map(async (current) => {
          // Reconciliation runs on every change-stream resync and permission event, for every open
          // project: invisible background work that must not drive the server-busy indicator, which
          // otherwise flashed a generic "Loading…" over unrelated work such as a warm tab switch
          // (HS2-7G3C19).
          const client = new Api(current.apiPath, '', { trackBusy: false });
          try {
            const [requests, connections] = await Promise.all([
              client.permissions(),
              client.activeToolConnections().catch(() => []),
            ]);
            return {
              current,
              requests: requests.filter((request) =>
                permissionBelongsToProject(request, connections, projects.value, current.id),
              ),
              connections,
            };
          } catch {
            return { current };
          }
        }),
      );
      if (resolutionEpoch !== permissionResolutionEpoch) {
        permissionRefreshRequested = true;
        return;
      }
      let changed = false;
      for (const result of results) {
        if (result.requests && projects.value.some((project) => project.id === result.current.id)) {
          changed =
            permissionInbox.reconcile(result.current, result.requests, result.connections, Date.now()) || changed;
          continue;
        }
        const stale = permissionInbox.pending().filter((item) => item.projectId === result.current.id);
        for (const item of stale) permissionTimer.remove(item.key);
        if (stale.some((item) => item.key in permissionResolutionErrors.value))
          permissionResolutionErrors.value = Object.fromEntries(
            Object.entries(permissionResolutionErrors.value).filter(([key]) => !stale.some((item) => item.key === key)),
          );
        changed = permissionInbox.discardProject(result.current.id) || changed;
      }
      if (changed) {
        updatePermissionTimer();
        permissionRevision.value += 1;
        persistPermissionHistory();
      }
    } finally {
      permissionPolling = false;
      if (permissionRefreshRequested) {
        permissionRefreshRequested = false;
        void refreshPermissions();
      }
    }
  }

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
      permissionTimerInterval = window.setInterval(updatePermissionTimer, 1_000);
      // Another window paused or resumed; follow it without writing the key back.
      window.addEventListener('storage', (event) => {
        const paused = notificationsPausedFromStorageEvent(event);
        if (paused !== undefined) setNotificationsPaused(paused, false);
      });
    }
    void refreshPermissions();
  }

  async function resolvePermission(
    item: PermissionItem,
    decision: PermissionDecision,
    scope: PermissionScope,
    automatic = false,
  ) {
    const finishTiming = beginInteractionTiming('permission-decision', { project: item.projectId }),
      owning = projects.value.find((value) => value.id === item.projectId);
    if (!owning) return;
    permissionResolutionErrors.value = Object.fromEntries(
      Object.entries(permissionResolutionErrors.value).filter(([key]) => key !== item.key),
    );
    if (!permissionInbox.resolve(item.key, decision, scope, automatic)) return;
    permissionResolutionEpoch += 1;
    persistPermissionHistory();
    permissionTimer.remove(item.key);
    updatePermissionTimer();
    permissionRevision.value += 1;
    finishTiming();
    try {
      await new Api(owning.apiPath).resolvePermission(item.id, decision, scope);
    } catch (reason) {
      const noLongerPending = reason instanceof ApiHttpError && reason.status === 404;
      permissionInbox.restore(item);
      if (noLongerPending) permissionInbox.removeExternal(item.key);
      persistPermissionHistory();
      // A failed automatic decision falls back to a manual one: never retry it on the next timer tick,
      // which for an immediate (0 s) Auto-allow would become a once-per-second request loop.
      if (automatic && !noLongerPending) permissionTimer.cancel(item.key);
      if (!noLongerPending)
        permissionResolutionErrors.value = {
          ...permissionResolutionErrors.value,
          [item.key]: `Could not send the permission decision. ${reason instanceof Error ? reason.message : String(reason)}`,
        };
      if (!noLongerPending) {
        updatePermissionTimer();
        permissionRevision.value += 1;
      }
      if (noLongerPending) await refreshPermissions();
    }
  }

  function serverResolvedPermission(
    key: string,
    resolution?: { decision: PermissionDecision; scope: PermissionScope },
  ) {
    permissionResolutionEpoch += 1;
    const removed = resolution
      ? permissionInbox.resolve(key, resolution.decision, resolution.scope)
      : permissionInbox.removeExternal(key);
    if (!removed) return;
    permissionTimer.remove(key);
    updatePermissionTimer();
    permissionRevision.value += 1;
    persistPermissionHistory();
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
        }}
      />
    );
  }

  return {
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
