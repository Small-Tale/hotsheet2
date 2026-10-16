import type { Signal } from 'kerfjs';

import { applyConversationActivity, applyConversationEvent } from '../ai-conversation';
import { Api, CHANGE_STREAM_CLIENT_ID, type PollResponse } from '../api';
import { type ChangeStreamWorkerLike, createChangeStreamWorkerHost } from '../change-stream-worker-host';
import type { ProjectTabBarMode } from '../components/project-tab-bar';
import type { Project } from '../interactions/types';
import type { LocalTicketChangeAcknowledgements } from '../local-ticket-changes';
import { parsePermissionResolution } from '../permission-notifications';
import {
  containsRepositoryChange,
  containsTicketChange,
  type ProjectChangeStreamOptions,
  startProjectChangeStream,
} from '../project-change-poll';
import type { createRefreshBarrier } from '../refresh-barrier';
import type { createDriveConversationsController } from './drive-conversations';
import type { createPermissionsController } from './permissions';
import type { createTerminalNamesController } from './terminal-names';

type DriveConversations = ReturnType<typeof createDriveConversationsController>;

/** Live application bindings the per-project change streams drive (HS2-3JGWTV). */
export interface ProjectChangeStreamsDependencies {
  projects: Signal<Project[]>;
  project: () => Project | undefined;
  shellMode: Signal<ProjectTabBarMode>;
  statsProjectId: Signal<string | undefined>;
  localTicketMutationBarrier: ReturnType<typeof createRefreshBarrier>;
  localTicketChangeAcknowledgements: LocalTicketChangeAcknowledgements;
  projectTabRefresh: { request: (current: Project) => Promise<unknown> };
  turnStreamEvents: (response: PollResponse) => Iterable<unknown>;
  conversationPersistence: DriveConversations['conversationPersistence'];
  updateConversation: DriveConversations['updateConversation'];
  conversationForActivity: DriveConversations['conversationForActivity'];
  refreshDriveConnections: DriveConversations['refreshDriveConnections'];
  refreshPermissions: ReturnType<typeof createPermissionsController>['refreshPermissions'];
  serverResolvedPermission: ReturnType<typeof createPermissionsController>['serverResolvedPermission'];
  permissionRulesChanged: ReturnType<typeof createPermissionsController>['permissionRulesChanged'];
  applyTerminalRenamed: ReturnType<typeof createTerminalNamesController>['applyTerminalRenamed'];
  refreshTerminalDashboard: (quiet?: boolean, targetProject?: Project) => Promise<unknown>;
  refreshProviderOutbox: (current: Project) => Promise<unknown>;
  refreshCommands: (current?: Project) => Promise<unknown>;
  refreshCustomViews: (current?: Project) => Promise<unknown>;
  refreshRepositoryStatus: () => Promise<unknown>;
  loadConfidenceReport: (target: Project) => Promise<unknown>;
  /**
   * Run every project's transport in the shared change-stream worker (HS2-SH9179). Off by default;
   * defaults to the `hotsheet.change-stream-worker` local setting. The main-thread stream remains
   * the fallback whenever the worker is disabled, unavailable, version-skewed, or crash-looping.
   */
  useChangeStreamWorker?: boolean;
  /** Injected for tests; defaults to the bundled module worker. */
  createChangeStreamWorker?: () => ChangeStreamWorkerLike | undefined;
}

/** Local setting that opts this browser into the shared change-stream worker (HS2-SH9179). */
export const CHANGE_STREAM_WORKER_SETTING = 'hotsheet.change-stream-worker';

export function changeStreamWorkerEnabled(): boolean {
  try {
    return localStorage.getItem(CHANGE_STREAM_WORKER_SETTING) === 'true';
  } catch {
    return false;
  }
}

const createBundledChangeStreamWorker = (): ChangeStreamWorkerLike | undefined =>
  typeof Worker === 'undefined'
    ? undefined
    : new Worker(new URL('../change-stream.worker.ts', import.meta.url), {
        type: 'module',
        name: 'hotsheet-change-stream',
      });

/**
 * Owns one live change stream per open project (WebSocket or long poll, never a polling timer)
 * plus the debounced repository refresh each stream schedules. `syncProjectChangeStreams`
 * starts streams for newly open projects and stops those of closed projects. Extracted from
 * the application runtime without behavior changes (HS2-3JGWTV).
 */
export function createProjectChangeStreamsController(dependencies: ProjectChangeStreamsDependencies) {
  const {
    projects,
    project,
    shellMode,
    statsProjectId,
    localTicketMutationBarrier,
    localTicketChangeAcknowledgements,
    projectTabRefresh,
    turnStreamEvents,
    conversationPersistence,
    updateConversation,
    conversationForActivity,
    refreshDriveConnections,
    refreshPermissions,
    serverResolvedPermission,
    permissionRulesChanged,
    applyTerminalRenamed,
    refreshTerminalDashboard,
    refreshProviderOutbox,
    refreshCommands,
    refreshCustomViews,
    refreshRepositoryStatus,
    loadConfidenceReport,
  } = dependencies;
  const projectChangeStreams = new Map<string, () => void>();
  const useWorker = dependencies.useChangeStreamWorker ?? changeStreamWorkerEnabled();
  let workerHost: ReturnType<typeof createChangeStreamWorkerHost> | undefined;
  const startStream = (current: Project, streamOptions: ProjectChangeStreamOptions): (() => void) => {
    if (!useWorker) return startProjectChangeStream(streamOptions);
    workerHost ??= createChangeStreamWorkerHost({
      createWorker: dependencies.createChangeStreamWorker ?? createBundledChangeStreamWorker,
      clientId: CHANGE_STREAM_CLIENT_ID,
    });
    const api = new Api(current.apiPath);
    return workerHost.start(current.id, streamOptions, {
      apiUrl: new URL(current.apiPath, location.href).href.replace(/\/$/u, ''),
      webSocketUrl: api.changeWebSocketUrl(),
    });
  };
  const repositoryRefreshTimers = new Map<string, number>();
  let disposed = false;
  function scheduleRepositoryRefresh(current: Project) {
    if (disposed) return;
    const existing = repositoryRefreshTimers.get(current.id);
    if (existing !== undefined) window.clearTimeout(existing);
    repositoryRefreshTimers.set(
      current.id,
      window.setTimeout(() => {
        repositoryRefreshTimers.delete(current.id);
        if (project()?.id === current.id) void refreshRepositoryStatus();
      }, 180),
    );
  }
  function syncProjectChangeStreams() {
    // A disposed runtime never reopens streams, even when a late startup restore calls in.
    if (disposed) return;
    const live = new Set(projects.value.map((item) => item.id));
    for (const [id, stop] of projectChangeStreams)
      if (!live.has(id)) {
        stop();
        projectChangeStreams.delete(id);
      }
    for (const [id, timer] of repositoryRefreshTimers)
      if (!live.has(id)) {
        window.clearTimeout(timer);
        repositoryRefreshTimers.delete(id);
      }
    for (const current of projects.value)
      if (!projectChangeStreams.has(current.id)) {
        const stop = startStream(current, {
          client: new Api(current.apiPath),
          beforeRefresh: () => localTicketMutationBarrier.wait(),
          shouldRefresh: (response) =>
            containsTicketChange({
              ...response,
              events: localTicketChangeAcknowledgements.unacknowledged(current.id, response.events),
            }),
          // Event-driven state reconciles when the stream (re)establishes continuity rather than
          // on a polling timer (HS2-NKCXW4): permissions on every resync, drive connections only
          // after an outage or overflow (opening the project already loaded them).
          onResync: async (reason) => {
            await Promise.all([
              refreshPermissions(),
              refreshTerminalDashboard(true, current),
              refreshProviderOutbox(current),
              ...(reason === 'initial' ? [] : [refreshDriveConnections(current, false, true)]),
            ]);
          },
          refresh: async () => {
            await projectTabRefresh.request(current);
            await refreshProviderOutbox(current);
            // Ticket changes can complete, reopen, or verify: keep an open calibration current.
            if (shellMode.peek() === 'stats' && statsProjectId.peek() === current.id)
              await loadConfidenceReport(current);
          },
          onEvents: async (response) => {
            const acceptedTurns = new Set(turnStreamEvents(response));
            for (const event of response.events) {
              if (event.kind === 'permission_resolved') {
                const resolution = parsePermissionResolution(event.message),
                  key = `${current.id}:${event.id}`;
                serverResolvedPermission(key, resolution);
              }
              if (event.kind === 'turn_event' && event.turn && acceptedTurns.has(event.turn)) {
                updateConversation(
                  event.turn.connection_id,
                  (state) => applyConversationEvent(state, event.turn!.event, event.turn!.turn_id, event.turn),
                  event.turn.event.type !== 'done',
                );
                if (event.turn.event.type === 'done') conversationPersistence.flush();
              }
              if (event.kind === 'activity' && event.activity) {
                const activity = event.activity,
                  connection = conversationForActivity(current, activity.tool, activity.session);
                if (connection)
                  updateConversation(connection.id, (state) => applyConversationActivity(state, activity), true);
              }
            }
            if (
              response.events.some((event) => event.kind === 'permission_asked' || event.kind === 'permission_resolved')
            )
              await refreshPermissions();
            // A remembered answer was added or revoked, possibly by another client (HS2-KYK1S7).
            if (response.events.some((event) => event.kind === 'permission_rules_changed'))
              permissionRulesChanged(current.id);
            if (response.events.some((event) => event.kind === 'drive_updated')) await refreshDriveConnections(current);
            if (response.events.some((event) => event.kind === 'command_updated') && project()?.id === current.id)
              await refreshCommands(current);
            if (response.events.some((event) => event.kind === 'views_updated')) await refreshCustomViews(current);
            for (const event of response.events)
              if (event.kind === 'terminal_renamed') applyTerminalRenamed(current, event.id, event.message);
            // A terminal's AI session halted or resumed (HS2-HJ4D1H), or connected to or left Hot Sheet
            // (HS2-EV1XK3), or a terminal's hotsheet-mcp
            // presence changed (HS2-670ADP): refetch so its tab marks it. A visible permission ask independently
            // proves a live hook and must refresh the terminal even if its connection event was
            // missed or arrived on another change-stream page (HS2-XYSXVT).
            if (
              response.events.some(
                (event) =>
                  event.kind === 'terminal_halted' ||
                  event.kind === 'terminal_question' ||
                  event.kind === 'terminal_ai_connection' ||
                  event.kind === 'terminal_hook_report' ||
                  event.kind === 'mcp_presence' ||
                  event.kind === 'permission_asked',
              )
            )
              void refreshTerminalDashboard(true, current);
            if (containsRepositoryChange(response, current.id)) scheduleRepositoryRefresh(current);
          },
        });
        projectChangeStreams.set(current.id, stop);
      }
  }
  /**
   * Stop every live stream and pending repository refresh when the runtime is disposed
   * (HS2-A9E7QB). Idempotent; later syncs are ignored, and a new runtime builds a fresh owner.
   */
  function dispose() {
    disposed = true;
    for (const stop of projectChangeStreams.values()) stop();
    projectChangeStreams.clear();
    workerHost?.dispose();
    workerHost = undefined;
    for (const timer of repositoryRefreshTimers.values()) window.clearTimeout(timer);
    repositoryRefreshTimers.clear();
  }
  return { syncProjectChangeStreams, dispose };
}
