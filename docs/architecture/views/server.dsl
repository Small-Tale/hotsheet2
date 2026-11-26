// Server component and dynamic views.
// Owned by HS2-P27EJM. New views use keys prefixed HS2-Server-.

component server "HS2-TicketComponents" "Checkout routing, provider authority, and local index reconciliation." {
    include bridge serverTrust serverTickets serverSources serverIndex serverEvents tickets searchIndex github gitlab jira
    exclude eventsDeliverBridge
    autoLayout lr
}
component server "HS2-ExecutionComponents" "Command admission, sessions, permission boundary, and terminal ownership." {
    include bridge checkoutData serverTrust serverSchedule serverTickets serverDrive serverTerminal broker aiTool
    autoLayout lr
}
component server "HS2-HostSupport" "Job observation, resource backends, notifications, and machine-local state." {
    include serverJobs serverNotify serverResources machineData github gitlab ciServices notifierServices sharedClaims gitRemote standby
    autoLayout lr
}
component server "HS2-OrchestrationComponents" "Plan inputs, readiness, authorization, run state, session placement, and job observations." {
    include bridge checkoutData machineData serverTrust serverOrch serverTickets serverDrive serverJobs serverResources github gitlab
    exclude jobsUseMachine
    autoLayout lr
}

// Deep baseline (HS2-P27EJM): subsystem component views.
component server "HS2-Server-Admission" "Process lifecycle, listeners, request middleware, the four caller kinds, the device catalog and store hosting." {
    include bridge serverLifecycle serverTls serverMiddleware serverTrust serverPairing serverHosting serverEvents serverIndex machineData
    autoLayout lr
}
component server "HS2-Server-Sources" "Provider routing, Jira write-behind, advisory claims, credentials, webhooks, sync and change publication." {
    include serverTickets serverSources serverWriteBehind serverAdvisory serverCredentials serverWebhooks serverSync serverEvents serverLive serverRepository jira github gitRemote osServices
    autoLayout lr
}
component server "HS2-Server-Execution" "Terminals, the AI drive, tool adapters, plugins and the permission bridge." {
    include cli serverTerminal serverSchedule serverDrive serverDistWork serverPermissions serverAiTools serverPlugins serverEvents broker launchRelay aiTool checkoutData
    autoLayout lr
}
component server "HS2-Server-Notifications" "Bus subscription, notifier dispatch, live snapshots and standby publication." {
    include serverEvents serverNotify serverNotifiers serverLive standby notifierServices machineData
    autoLayout lr
}
component broker "HS2-Server-Broker" "Detached broker internals: socket, terminal map, PTY sessions and viewer sizing." {
    include serverTerminal brokerService brokerSocket brokerManager brokerTerminals brokerSizing aiTool
    autoLayout lr
}
component launchRelay "HS2-Server-LaunchRelay" "macOS launch relay: LaunchAgent, listener, namespace probe and broker launcher." {
    include serverTerminal relayService relayLauncher relayBootstrap relayAgent broker osServices
    autoLayout lr
}
component saver "HS2-Server-Saver" "macOS screen saver: directory watch and the bundled standby page." {
    include serverNotify standby saverWatcher saverView
    autoLayout lr
}

// Deep baseline (HS2-P27EJM): required dynamic views, each including its failure path.
dynamic server "HS2-Server-ChangeReplay" "Change-stream delivery, a lagged WebSocket, cursor replay over long poll, and ring overflow." {
    serverTickets -> serverEvents "Records the change in the 512-event ring (cursor n) and broadcasts it on the 256-slot bus"
    serverEvents -> bridge "Pushes the event as JSON over /ws/sync"
    bridge -> serverEvents "Lagged receiver: the socket loop ends and the stream closes; the client long-polls /ws/poll?since=last cursor"
    serverEvents -> serverHosting "Begins a presence lease for the checkout and requests an unhost sweep"
    serverEvents -> bridge "Returns every event after the cursor with the new cursor, or overflow=true when the cursor fell off the ring"
    bridge -> serverTrust "On overflow, re-lists tickets; the response carries the cursor it was served at"
    serverTrust -> serverTickets "Admits the full re-read"
    autoLayout lr
}
dynamic server "HS2-Server-BrokerReattach" "Server restart with broker reattach, and the crashed-broker path." {
    serverLifecycle -> serverTerminal "Ensures the broker: ping, build identity, adopt or replace an idle broker of another build"
    serverTerminal -> broker "Lists surviving terminals"
    broker -> serverTerminal "Returns live terminals with kind and worker"
    serverTerminal -> serverAiTools "Resumes Claude and Codex usage follows and AI connection entries"
    bridge -> serverTerminal "Viewer reconnects to /terminals/{id}/attach"
    serverTerminal -> broker "Opens a stream; first frame is the replay or only the missed bytes for a matching stream offset"
    serverTerminal -> broker "Crashed broker: the next call finds no socket and spawns a fresh empty broker; pollers get not_found and release session claims"
    autoLayout lr
}
dynamic server "HS2-Server-DevicePairing" "Remote browser pairing with owner approval, and the rejected, expired and rate-limited paths." {
    web -> bridge "Posts a label to /__hotsheet/remote-auth/pair"
    bridge -> serverPairing "Forwards; ten requests a minute per catalog, 15 minute expiry"
    serverPairing -> machineData "Persists the pending request with a hashed challenge"
    serverPairing -> serverEvents "Publishes an identity-free pairing-request change"
    serverEvents -> bridge "Delivers it only to device managers"
    bridge -> serverPairing "Owner approves with role and grants (machine secret from loopback, or admin session with Origin and CSRF)"
    bridge -> serverPairing "Browser redeems: 409 until approved, 410 when expired, 429 after five bad attempts, 401 if rejected"
    serverPairing -> machineData "Stores the session and CSRF digests and returns HttpOnly SameSite=Strict cookies"
    autoLayout lr
}
dynamic server "HS2-Server-PermissionRoundTrip" "An AI tool permission ask parked on the bridge, answered by a client, with timeout, disconnect and shutdown paths." {
    aiTool -> cli "Runs the permission hook before a tool call"
    cli -> serverPermissions "Posts /permissions/ask and waits"
    serverPermissions -> serverEvents "No remembered rule matched: enqueues FIFO and publishes permission_asked"
    serverEvents -> bridge "Delivers the nudge; the client reads GET /permissions"
    bridge -> serverTrust "Posts /permissions/{id} with decision and scope"
    serverTrust -> serverPermissions "Resolves by id, persists an Always rule, and wakes the parked ask"
    serverPermissions -> serverEvents "Publishes permission_resolved"
    cli -> serverPermissions "Failure paths: hook disconnect withdraws as deny; 24 hour timeout denies; a stopping server answers 503 and the hook falls back to the tool prompt"
    autoLayout lr
}
dynamic server "HS2-Server-JiraWriteBehind" "Queued Jira field edit, dispatch with readback, and the failure states." {
    bridge -> serverTrust "Posts /providers/{connection}/tickets/queued with operation ids"
    serverTrust -> serverWriteBehind "Admits the batch into the outbox (capacity 1000) and returns the provisional ticket"
    serverWriteBehind -> machineData "Persists the operations"
    serverWriteBehind -> serverEvents "Every two seconds claims up to four ready operations and publishes updated"
    serverWriteBehind -> jira "Reads the issue, records a conflict, then updates with the concurrency token"
    serverWriteBehind -> machineData "Failure: rate limit defers the connection; auth, not-found or unsupported needs attention; other errors back off, attention after eight attempts"
    serverWriteBehind -> serverEvents "Publishes updated for the settled operation"
    autoLayout lr
}
