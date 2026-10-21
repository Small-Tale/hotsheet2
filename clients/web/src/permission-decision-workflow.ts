import type { Signal } from 'kerfjs';
import { signal } from 'kerfjs';

import { Api, ApiHttpError } from './api';
import type { Project } from './interactions/types';
import {
  permissionBelongsToProject,
  type PermissionDecision,
  type PermissionInbox,
  type PermissionItem,
  type PermissionPattern,
  type PermissionScope,
  type VisiblePermissionTimer,
} from './permission-notifications';

type PermissionRequests = Awaited<ReturnType<Api['permissions']>>;
type ToolConnections = Awaited<ReturnType<Api['activeToolConnections']>>;

export interface PermissionDecisionDependencies {
  projects: Signal<Project[]>;
  inbox: PermissionInbox;
  timer: VisiblePermissionTimer;
  persistHistory: () => void;
  updateTimer: () => void;
  clearDescription: (key: string) => void;
  beginTiming?: (item: PermissionItem) => () => void;
  load?: (project: Project) => Promise<{ requests: PermissionRequests; connections: ToolConnections }>;
  send?: (
    project: Project,
    item: PermissionItem,
    decision: PermissionDecision,
    scope: PermissionScope,
    description?: string,
    pattern?: PermissionPattern,
  ) => Promise<void>;
}

/** API-consumer transitions; storage, DOM countdown, and popup rendering are injected effects. */
export function createPermissionDecisionWorkflow(dependencies: PermissionDecisionDependencies) {
  const { projects, inbox, timer, persistHistory, updateTimer, clearDescription, beginTiming } = dependencies;
  const load =
    dependencies.load ??
    (async (current: Project) => {
      const client = new Api(current.apiPath, '', { trackBusy: false });
      const [requests, connections] = await Promise.all([
        client.permissions(),
        client.activeToolConnections().catch(() => []),
      ]);
      return { requests, connections };
    });
  const send =
    dependencies.send ??
    (async (
      owning: Project,
      item: PermissionItem,
      decision: PermissionDecision,
      scope: PermissionScope,
      description?: string,
      pattern?: PermissionPattern,
    ) => {
      await new Api(owning.apiPath).resolvePermission(item.id, decision, scope, description, pattern);
    });
  const permissionRevision = signal(0),
    permissionResolutionErrors = signal<Record<string, string>>({}),
    freshPermissionRules = signal<ReadonlySet<string>>(new Set());
  let polling = false,
    refreshRequested = false,
    resolutionEpoch = 0;
  const projectsToRefresh = new Set<string>();

  function markPermissionRulesStale(projectId?: string) {
    const fresh = freshPermissionRules.peek();
    if (projectId === undefined ? fresh.size > 0 : fresh.has(projectId))
      freshPermissionRules.value = new Set(projectId === undefined ? [] : [...fresh].filter((id) => id !== projectId));
  }

  function queueRefresh(projectId?: string) {
    if (projectId === undefined) {
      refreshRequested = true;
      projectsToRefresh.clear();
    } else if (!refreshRequested) projectsToRefresh.add(projectId);
  }

  async function refreshPermissions(projectId?: string) {
    if (polling) {
      queueRefresh(projectId);
      return;
    }
    polling = true;
    try {
      const epoch = resolutionEpoch;
      const targets = projectId === undefined ? projects.value : projects.value.filter((item) => item.id === projectId);
      const results = await Promise.all(
        targets.map(async (current) => {
          try {
            const { requests, connections } = await load(current);
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
      if (epoch !== resolutionEpoch) {
        queueRefresh(projectId);
        return;
      }
      const previousKeys = inbox.pending().map((item) => item.key);
      let changed = false;
      for (const result of results) {
        if (result.requests && projects.value.some((item) => item.id === result.current.id)) {
          changed = inbox.reconcile(result.current, result.requests, result.connections, Date.now()) || changed;
          continue;
        }
        const stale = inbox.pending().filter((item) => item.projectId === result.current.id);
        for (const item of stale) {
          timer.remove(item.key);
          clearDescription(item.key);
        }
        if (stale.some((item) => item.key in permissionResolutionErrors.value))
          permissionResolutionErrors.value = Object.fromEntries(
            Object.entries(permissionResolutionErrors.value).filter(([key]) => !stale.some((item) => item.key === key)),
          );
        changed = inbox.discardProject(result.current.id) || changed;
      }
      if (projectId === undefined) {
        const openIds = new Set(projects.value.map((item) => item.id));
        const detached = inbox.pending().filter((pending) => !openIds.has(pending.projectId));
        for (const item of detached) {
          timer.remove(item.key);
          clearDescription(item.key);
          changed = inbox.discardProject(item.projectId) || changed;
        }
        if (detached.length)
          permissionResolutionErrors.value = Object.fromEntries(
            Object.entries(permissionResolutionErrors.value).filter(
              ([key]) => !detached.some((item) => item.key === key),
            ),
          );
      }
      const currentKeys = new Set(inbox.pending().map((item) => item.key));
      for (const key of previousKeys) if (!currentKeys.has(key)) clearDescription(key);
      if (changed) {
        updateTimer();
        permissionRevision.value += 1;
        persistHistory();
      }
    } finally {
      polling = false;
      if (refreshRequested) {
        refreshRequested = false;
        void refreshPermissions();
      } else {
        const nextProject = projectsToRefresh.values().next().value;
        if (nextProject !== undefined) {
          projectsToRefresh.delete(nextProject);
          void refreshPermissions(nextProject);
        }
      }
    }
  }

  async function resolvePermission(
    item: PermissionItem,
    decision: PermissionDecision,
    scope: PermissionScope,
    automatic = false,
    grouped = !automatic,
    description?: string,
    pattern?: PermissionPattern,
  ) {
    const owning = projects.value.find((value) => value.id === item.projectId);
    if (!owning) return;
    const group = grouped ? inbox.identical(item.key) : [item];
    if (group.length > 1) {
      const [first, ...rest] = group;
      if (scope !== 'once')
        for (const other of rest) if (inbox.resolve(other.key, decision, scope)) timer.remove(other.key);
      await Promise.all([
        resolvePermission(first, decision, scope, automatic, false, description, pattern),
        ...(scope === 'once' ? rest.map((other) => resolvePermission(other, decision, scope, false, false)) : []),
      ]);
      return;
    }
    const finishTiming = beginTiming?.(item);
    permissionResolutionErrors.value = Object.fromEntries(
      Object.entries(permissionResolutionErrors.value).filter(([key]) => key !== item.key),
    );
    if (!inbox.resolve(item.key, decision, scope, automatic)) return;
    if (scope !== 'once') markPermissionRulesStale();
    resolutionEpoch += 1;
    persistHistory();
    timer.remove(item.key);
    updateTimer();
    permissionRevision.value += 1;
    finishTiming?.();
    try {
      await send(owning, item, decision, scope, scope === 'once' ? undefined : description, pattern);
      clearDescription(item.key);
    } catch (reason) {
      const noLongerPending = reason instanceof ApiHttpError && reason.status === 404;
      inbox.restore(item);
      if (noLongerPending) {
        inbox.removeExternal(item.key);
        clearDescription(item.key);
      }
      persistHistory();
      if (automatic && !noLongerPending) timer.cancel(item.key);
      if (!noLongerPending)
        permissionResolutionErrors.value = {
          ...permissionResolutionErrors.value,
          [item.key]: `Could not send the permission decision. ${reason instanceof Error ? reason.message : String(reason)}`,
        };
      if (!noLongerPending) {
        updateTimer();
        permissionRevision.value += 1;
      }
      if (noLongerPending) await refreshPermissions(item.projectId);
    }
  }

  function serverResolvedPermission(
    key: string,
    resolution?: { decision: PermissionDecision; scope: PermissionScope },
  ) {
    resolutionEpoch += 1;
    if (resolution?.scope !== 'once') markPermissionRulesStale();
    const removed = resolution ? inbox.resolve(key, resolution.decision, resolution.scope) : inbox.removeExternal(key);
    if (!removed) return;
    clearDescription(key);
    timer.remove(key);
    updateTimer();
    permissionRevision.value += 1;
    persistHistory();
  }

  return {
    permissionRevision,
    permissionResolutionErrors,
    freshPermissionRules,
    markPermissionRulesStale,
    refreshPermissions,
    resolvePermission,
    serverResolvedPermission,
  };
}
