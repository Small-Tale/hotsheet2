// Browser web client component and dynamic views.
// Owned by HS2-AYKHVV. New views use keys prefixed HS2-WebClient-.

component web "HS2-WebComponents" "Workspace state, checkout API access, change streams, and terminal frontend." {
    include webWorkspace webApi webChanges webTerminal bridge
    autoLayout lr
}

// Component views, split so each stays readable (README "Model conventions").
component web "HS2-WebClient-Core" "Entry, workspace, API, persistence and the editing core." {
    include developer webShell webWorkspace webApi webPersistence webTickets webDrafts webChanges webUxDemo bridge
    autoLayout lr
}
component web "HS2-WebClient-LiveData" "How change events reach rendered state." {
    include webChanges webTickets webAi webNotifications webTerminal webHostOps webCommands webRemote webApi webPersistence bridge
    autoLayout lr
}
component web "HS2-WebClient-Editing" "Drafts, recovery copies, attachments and optimistic ticket writes." {
    include developer webTickets webDrafts webPersistence webAttachments webApi webChanges webNotifications bridge
    autoLayout lr
}
component web "HS2-WebClient-Features" "Feature components composed by the runtime." {
    include webShell webAi webOrchestration webCommands webNotifications webRemote webHostOps webAttachments webTerminal
    autoLayout lr
}
component web "HS2-WebClient-FeatureServices" "Feature components and their outside dependencies." {
    include operator webAi webOrchestration webCommands webNotifications webRemote webHostOps webTerminal webApi bridge desktop
    autoLayout lr
}

dynamic web "HS2-WebClient-OpenProject" "Opening a project: restore its session, load tickets, then follow its change cursor." {
    developer -> webShell "Chooses a project tab or opens a registered project"
    webShell -> webWorkspace "Asks the project lifecycle controller to activate the project"
    webWorkspace -> webPersistence "Restores the per-project session and touches the warm-project order"
    webWorkspace -> webApi "Requests the checkout's ticket pages and board columns"
    webApi -> bridge "Sends checkout-scoped reads through the project API path"
    bridge -> webApi "Answers with rows and the x-hotsheet-change-cursor stamp, or an unavailable error"
    webWorkspace -> webTickets "Publishes rows, counts and projection signals; an error renders the unavailable shell"
    webWorkspace -> webChanges "Opens one change stream for the project"
    webChanges -> bridge "Handshakes with a zero-cursor poll, then opens the WebSocket"
    bridge -> webChanges "Delivers the starting cursor; a read stamped below it replays the gap once"
    autoLayout lr
}

dynamic web "HS2-WebClient-CoalescedEdit" "Typing, a focus move, one merged save, a refused save, and reload recovery (HS2-RE1PS6)." {
    developer -> webTickets "Types into a ticket field"
    webTickets -> webDrafts "Updates the controlled draft; publishes once per typing pause"
    webDrafts -> webPersistence "After 150 ms writes a local recovery copy with the edit-start base; no server write"
    developer -> webTickets "Moves focus out of the field"
    webTickets -> webDrafts "Blur flushes the field's focus-loss autosave"
    webDrafts -> webTickets "Hands one value, merged against the edit-start base, to the mutation sequencer"
    webTickets -> webChanges "Begins the refresh barrier and records the write as this tab's own change"
    webTickets -> webApi "Sends the serialized write; a concurrency conflict rebases and retries up to 4 times"
    webApi -> bridge "Forwards the checkout-scoped write"
    webTickets -> webNotifications "On refusal rolls back the optimistic row and reports it"
    webDrafts -> webPersistence "After reload, reads the stored copy and reconciles it against the fresh ticket"
    autoLayout lr
}

dynamic web "HS2-WebClient-OtherTabChange" "A write in tab A reaches tab B through its own change stream." {
    webTickets -> webApi "Tab A sends a ticket write"
    webApi -> bridge "Tab A's write reaches the server through the bridge"
    bridge -> webChanges "Tab B's stream (main thread, or its port of the shared worker) receives the event"
    webChanges -> webTickets "Waits for tab B's barrier, skips own acknowledged writes, runs a coalesced refresh"
    webTickets -> webApi "Re-reads rows, counts and the selected ticket's detail"
    webTickets -> webDrafts "An active draft adopts a remote-only change, merges, or shows a field conflict"
    autoLayout lr
}

dynamic web "HS2-WebClient-ReplayOverflow" "Reconnect after a dropped stream, when the replay overflowed or could not be served." {
    webChanges -> bridge "After backoff, long-polls the replay from the last applied cursor"
    bridge -> webChanges "Answers with overflow set, or the replay fails and the cursor is reset for a fresh handshake"
    webChanges -> webAi "Resync re-reads permissions and drive connections"
    webChanges -> webNotifications "Resync re-reads the attention inbox"
    webChanges -> webHostOps "Resync re-reads live state and host health and marks the stream healthy"
    webChanges -> webTerminal "Resync re-reads the terminal dashboard list"
    webChanges -> webTickets "Forces a full ticket refresh: the projection is rebuilt from authoritative reads"
    webTickets -> webApi "Reads ticket pages, board columns and the selected detail"
    webChanges -> webRemote "Clears the reconnecting banner for a remote project"
    autoLayout lr
}

dynamic web "HS2-WebClient-TerminalAttach" "Attaching a terminal with replay, resize arbitration, and recovery from a corrupt replay." {
    developer -> webTerminal "Opens the terminal drawer tab"
    webTerminal -> webApi "Reads the terminal session list"
    webTerminal -> bridge "Opens the attach WebSocket with replay protocol 2, byte cap and resume position"
    bridge -> webTerminal "Sends a replay header and bytes, then live PTY output"
    webTerminal -> bridge "Sends a resize claim with viewer id, cols, rows and focus"
    bridge -> webTerminal "Broadcasts pty_size naming the driving and answering viewer"
    developer -> webTerminal "Types; input buffers up to 64 KiB while the socket reopens"
    webTerminal -> bridge "On a corrupt replay or stale socket, abandons it and reattaches with resume_from"
    autoLayout lr
}
