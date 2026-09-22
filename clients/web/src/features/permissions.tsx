import type { Signal } from 'kerfjs';
import { signal } from 'kerfjs';

import { Api } from '../api';
import { updatePermissionCountdownText } from '../components/permission-request-card';
import { PermissionPopupSurface } from '../components/reader-overlay-surfaces';
import { beginInteractionTiming } from '../interaction-performance';
import type { Project } from '../interactions/types';
import {
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
  let permissionPolling = false,
    permissionTimerInterval: number | undefined,
    permissionCountdown: { key: string; remainingMs: number } | undefined;
  const pendingPermissions = () => permissionInbox.pending();
  const permissionHistory = () => permissionInbox.history();
  const projectPendingPermissions = (projectId = selectedProjectId.value) =>
    pendingPermissions().filter((item) => item.projectId === projectId);
  const projectPermissionHistory = (projectId = selectedProjectId.value) =>
    permissionHistory().filter((item) => item.projectId === projectId);
  const visiblePermission = () => permissionInbox.visible();
  const permissionCount = (projectId?: string) =>
    pendingPermissions().filter((item) => !projectId || item.projectId === projectId).length;
  const permissionAutomation = (projectId: string) =>
    permissionAutomationByProject.value[projectId] ?? DEFAULT_PERMISSION_AUTOMATION;
  const persistPermissionHistory = () => {
    localStorage.setItem('hotsheet.permission-history', JSON.stringify(permissionInbox.history()));
  };

  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function refreshPermissions(){if(permissionPolling)return;permissionPolling=true;try{let changed=false;await Promise.all(projects.value.map(async current=>{const client=new Api(current.apiPath);const [requests,connections]=await Promise.all([client.permissions(),client.activeToolConnections().catch(()=>[])]),owned=requests.filter(request=>permissionBelongsToProject(request,connections,projects.value,current.id));changed=permissionInbox.reconcile(current,owned,connections)||changed}));if(changed){updatePermissionTimer();permissionRevision.value+=1;persistPermissionHistory()}}catch{/* individual server disconnects remain represented by their last known requests */}finally{permissionPolling=false}}

  function startPermissionUpdates() {
    if (permissionTimerInterval === undefined)
      permissionTimerInterval = window.setInterval(updatePermissionTimer, 1_000);
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
    persistPermissionHistory();
    permissionTimer.remove(item.key);
    updatePermissionTimer();
    permissionRevision.value += 1;
    finishTiming();
    try {
      await new Api(owning.apiPath).resolvePermission(item.id, decision, scope);
    } catch (reason) {
      permissionInbox.restore(item);
      persistPermissionHistory();
      permissionResolutionErrors.value = {
        ...permissionResolutionErrors.value,
        [item.key]:
          reason instanceof Error && reason.message.includes('404')
            ? 'This request changed before Hot Sheet could answer it. Review it and try again.'
            : `Could not send the permission decision. ${reason instanceof Error ? reason.message : String(reason)}`,
      };
      updatePermissionTimer();
      permissionRevision.value += 1;
      if (reason instanceof Error && reason.message.includes('404')) await refreshPermissions();
    }
  }

  function updatePermissionTimer() {
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

  function permissionPopupSurface() {
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
    updatePermissionTimer,
    permissionPopupSurface,
    get permissionCountdown() {
      return permissionCountdown;
    },
    set permissionCountdown(value: typeof permissionCountdown) {
      permissionCountdown = value;
    },
  };
}
