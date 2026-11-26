// Real-client screenshots (HS2-DHCY18), captured from synthetic data by
// clients/web/tests/architecture-screenshots.spec.ts. Each image view hangs off
// the element it illustrates, so double-clicking that element offers it.

image web "HS2-Screen-WorkspaceTicketList" {
    title "9 Screens · Workspace with an open project"
    description "The Acme Notes project open in the workspace: project tab, views sidebar, and the queue of tickets read from the project's checkout through the bridge and server."
    image ../narrative/screenshots/01-workspace-ticket-list.png
}
image webTickets "HS2-Screen-TicketEditorNotes" {
    title "9 Screens · Ticket editor with notes"
    description "One ticket selected: the inspector edits its fields and details and lists its notes, including an attributed AI planning note."
    image ../narrative/screenshots/02-ticket-editor-notes.png
}
image webTerminal "HS2-Screen-AiSessionTerminal" {
    title "9 Screens · AI session terminal"
    description "The project terminal drawer attached to a running AI session's PTY over the terminal WebSocket, with replay and live output."
    image ../narrative/screenshots/03-ai-session-terminal.png
}
image webAi "HS2-Screen-AiQuestionPrompt" {
    title "9 Screens · AI question prompt"
    description "A question the AI session asked arrives in the app as a prompt the user can answer here or in the terminal."
    image ../narrative/screenshots/04-ai-question-prompt.png
}
image webOrchestration "HS2-Screen-OrchestrationPlanRun" {
    title "9 Screens · Orchestration plan and run"
    description "The Orchestration view: a goal's planned steps by column, the running step dispatched to a worker, and the inspector's Where It Runs, Plan, and Run sections."
    image ../narrative/screenshots/05-orchestration-plan-run.png
}
image webRemote "HS2-Screen-RemoteDevicePairing" {
    title "9 Screens · Remote device pairing request"
    description "A browser asks to pair with this host: a notice announces it, Settings carries the count, and Devices lists the waiting request to approve or reject."
    image ../narrative/screenshots/06-remote-device-pairing.png
}
image serverSources "HS2-Screen-SettingsTicketSources" {
    title "9 Screens · Project ticket sources"
    description "Project settings list the ticket sources this checkout uses; GitHub, GitLab, and Jira sign-ins are shared app-wide under Accounts."
    image ../narrative/screenshots/07-settings-ticket-sources.png
}
image coreProviderContract "HS2-Screen-SettingsAddSource" {
    title "9 Screens · Adding a ticket source"
    description "Adding a source offers a Hot Sheet Git ticket repository or a native GitHub Issues, GitLab Issues, or Jira Cloud connection, each keeping its own authoritative records."
    image ../narrative/screenshots/07b-settings-add-source.png
}
image saver "HS2-Screen-StandbyDisplay" {
    title "9 Screens · Standby display"
    description "Standby shows every open project's live activity at a glance: token rate, tickets updated, and a stalled-work alert."
    image ../narrative/screenshots/08-standby-display.png
}
