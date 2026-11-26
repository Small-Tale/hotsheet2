// web-client relationships. Owned by HS2-AYKHVV. Both endpoints belong to this area,
// or one endpoint is an external system or person. Edges between areas go in
// relationships-cross.dsl. Included after every element, so order does not matter.

workspaceQueriesApi = webWorkspace -> webApi "Requests scoped reads and writes" "TypeScript"
workspaceUsesChanges = webWorkspace -> webChanges "Opens and closes one change stream per open project and receives events and resyncs" "TypeScript callbacks"

// HS2-AYKHVV: composition root. runtime.tsx creates every feature controller and passes it signals.
webShellComposesWorkspace = webShell -> webWorkspace "Creates the project lifecycle controller and renders the shell" "TypeScript"
webShellComposesTickets = webShell -> webTickets "Creates ticket, search, group and saved-view controllers" "TypeScript"
webShellComposesTerminal = webShell -> webTerminal "Creates the terminal drawer, dashboard and viewport controllers" "TypeScript"
webShellComposesAi = webShell -> webAi "Creates drive, conversation, permission and halted-session controllers" "TypeScript"
webShellComposesOrchestration = webShell -> webOrchestration "Creates the orchestration view and target-line controllers" "TypeScript"
webShellComposesCommands = webShell -> webCommands "Creates command and schedule controllers" "TypeScript"
webShellComposesNotifications = webShell -> webNotifications "Creates the inbox, toast and announcer controllers" "TypeScript"
webShellComposesRemote = webShell -> webRemote "Checks the remote browser session and creates grant and device controllers" "TypeScript"
webShellComposesHostOps = webShell -> webHostOps "Creates host health, stats, repository and standby controllers" "TypeScript"
webShellComposesAttachments = webShell -> webAttachments "Creates the gallery controller" "TypeScript"
webShellLoadsPersistence = webShell -> webPersistence "Hydrates desktop storage and restores workspace preferences before mounting" "TypeScript"
webShellBootsUxDemo = webShell -> webUxDemo "Loads the catalog chunk instead of the app on /ux-demo when the host enables it" "Dynamic import"
webUxDemoRendersProduction = webUxDemo -> webTickets "Renders production components with fixtures" "Kerf"
webShellFlushesDrafts = webShell -> webDrafts "Flushes every field draft on pagehide and before an update reload" "DOM event"

// Workspace and project lifecycle.
webWorkspaceRestoresSession = webWorkspace -> webPersistence "Reads and writes per-project sessions, active project root and warm-cache order" "localStorage"
webWorkspaceRefreshesTickets = webWorkspace -> webTickets "Loads ticket pages and board columns for the active project" "Kerf signals"
webWorkspaceStartsTerminals = webWorkspace -> webTerminal "Parks and restores terminal viewports on project switch and close" "TypeScript"
webWorkspacePresentsNotifications = webWorkspace -> webNotifications "Shows project errors and outcomes as toasts" "TypeScript"

// Change streams to state.
webChangesRefreshesTickets = webChanges -> webTickets "Runs a coalesced, barrier-gated refresh of rows, counts and the selected detail" "Kerf signals"
webTicketsHoldsBarrier = webTickets -> webChanges "Holds the refresh barrier while a local mutation runs and acknowledges own writes" "TypeScript"
webChangesUpdatesAi = webChanges -> webAi "Applies turn events and refetches permissions and drive state" "Kerf signals"
webChangesUpdatesNotifications = webChanges -> webNotifications "Refreshes the attention inbox on notification events and resync" "TypeScript"
webChangesUpdatesTerminal = webChanges -> webTerminal "Refetches the terminal dashboard list on terminal events" "TypeScript"
webChangesUpdatesHostOps = webChanges -> webHostOps "Applies host health and live-state events and reports stream health" "Kerf signals"
webChangesUpdatesCommands = webChanges -> webCommands "Refetches command data on command events" "TypeScript"
webChangesReportsRemote = webChanges -> webRemote "Reports remote stream health for the reconnecting banner" "Kerf signals"
webChangesUsesApi = webChanges -> webApi "Builds stream URLs and reads the change-cursor stamp of ticket reads" "TypeScript"
webChangesUsesPersistence = webChanges -> webPersistence "Reads the worker opt-in and shared-worker keys" "localStorage"

// Ticket editing.
webTicketsUsesDrafts = webTickets -> webDrafts "Holds field edits in drafts and flushes them on blur, page hide or finish" "Kerf signals"
webDraftsSavesThroughTickets = webDrafts -> webTickets "Hands one merged write to the per-ticket mutation sequencer" "TypeScript"
webTicketsUsesApi = webTickets -> webApi "Reads tickets and applies serialized mutations" "TypeScript"
webTicketsUsesPersistence = webTickets -> webPersistence "Reads the bounded ticket detail cache and scroll memory" "TypeScript"
webDraftsStoresRecovery = webDrafts -> webPersistence "Stores and restores recovery copies and staged files" "localStorage / IndexedDB"
webTicketsOpensAttachments = webTickets -> webAttachments "Opens attachments in the gallery and stages new-ticket files" "TypeScript"
webAttachmentsUsesDrafts = webAttachments -> webDrafts "Stages unsent files and label or filename drafts" "TypeScript"
webTicketsReportsOutcome = webTickets -> webNotifications "Shows rolled-back writes and restored or discarded drafts as toasts" "TypeScript"

// Other feature edges.
webAttachmentsUsesApi = webAttachments -> webApi "Uploads, renames and annotates attachments" "TypeScript"
webAiUsesApi = webAi -> webApi "Starts turns, answers permissions and feedback" "TypeScript"
webAiUsesPersistence = webAi -> webPersistence "Persists conversation transcripts" "localStorage"
webOrchestrationUsesApi = webOrchestration -> webApi "Reads orchestration views and applies plan and policy changes" "TypeScript"
webCommandsUsesApi = webCommands -> webApi "Reads and saves commands and schedules, runs commands" "TypeScript"
webNotificationsUsesApi = webNotifications -> webApi "Reads and acknowledges notifications" "TypeScript"
webHostOpsUsesApi = webHostOps -> webApi "Reads host health, stats, repository and migration state" "TypeScript"
webTerminalUsesApi = webTerminal -> webApi "Lists, creates, renames and closes terminal sessions" "TypeScript"
webTerminalUsesPersistence = webTerminal -> webPersistence "Stores visibility groups and phone column choice" "localStorage"
webRemoteUsesApi = webRemote -> webApi "Reads grants and pairing requests" "TypeScript"
webAiOpensTerminal = webAi -> webTerminal "Opens the AI session's terminal and shows its connection state" "TypeScript"
webOrchestrationOpensTerminal = webOrchestration -> webTerminal "Opens worker session terminals from the timeline" "TypeScript"

// People.
webDeveloperUsesShell = developer -> webShell "Opens projects, edits tickets, runs terminals" "Web UI"
webDeveloperEditsTickets = developer -> webTickets "Types in ticket fields and moves focus" "Web UI"
webDeveloperUsesTerminal = developer -> webTerminal "Types into and resizes terminals" "Web UI"
webOperatorUsesRemote = operator -> webRemote "Signs in remotely and manages devices" "Web UI"
