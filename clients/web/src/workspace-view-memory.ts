import type { WorkspaceViewMode } from './components/workspace-header';
import type { TicketWorkspaceViewMode } from './workspace-preferences';

/**
 * HS2-BRRSYM: the workspace view is remembered per project type. Orchestration projects remember theirs
 * (Orchestration by default) apart from the ticket view (List, Columns or Workers) software projects
 * remember, so switching between the two kinds shows each kind's own last view.
 */
export type OrchestrationProjectViewMode = 'orchestration' | TicketWorkspaceViewMode;

export const DEFAULT_ORCHESTRATION_PROJECT_VIEW: OrchestrationProjectViewMode = 'orchestration';

/** A project's own views: what a project switch may swap. Notifications and Settings are not. */
export function isOrchestrationProjectViewMode(mode: unknown): mode is OrchestrationProjectViewMode {
  return mode === 'orchestration' || mode === 'list' || mode === 'board' || mode === 'workers';
}

/**
 * The view to show on arriving at a project whose type remembers `remembered`, or `undefined` to keep
 * the current one. Only a project view is swapped: arriving from Notifications or Settings keeps them.
 */
export function projectArrivalView(
  current: WorkspaceViewMode,
  remembered: OrchestrationProjectViewMode,
): OrchestrationProjectViewMode | undefined {
  return isOrchestrationProjectViewMode(current) && current !== remembered ? remembered : undefined;
}

/** What orchestration projects remember after the person chose `mode` in one of them. */
export function rememberOrchestrationProjectView(
  mode: WorkspaceViewMode,
  remembered: OrchestrationProjectViewMode,
): OrchestrationProjectViewMode {
  return isOrchestrationProjectViewMode(mode) ? mode : remembered;
}
