// Cross-layer dynamic views (end-to-end flows).
// Owned by HS2-M81CCP. New views use keys prefixed HS2-Flow-.

dynamic server "HS2-WebGitTicketWrite" "One checkout-scoped Git write through the browser bridge and the server." {
    title "8 End-to-end flows · Web Git ticket write (summary)"
    bridgeUsesTrust "Forwards the checkout-qualified request with its corresponding authority"
    trustAdmitsTickets "Admits the request to the checkout ticket API"
    ticketApiUsesSource "Selects the configured local Git source"
    sourceWritesGit "Persists the authoritative change in the store"
    ticketApiUsesIndex "Refreshes the affected local projection"
    ticketApiPublishes "Publishes a scoped invalidation"
    eventsDeliverBridge "Delivers live change or replay to the bridge"
    autoLayout lr 220 140
}
dynamic server "HS2-GitReconciliation" "Out-of-band store edit or sync converges into the local search projection and live stream." {
    title "8 End-to-end flows · Git reconciliation (summary)"
    reconcileReadsGit "Consumes a file event and reconciles the affected ticket"
    reconcileWritesIndex "Applies the updated row or removes a deleted row"
    reconcilePublishes "Publishes the scoped post-reconcile change"
    eventsDeliverBridge "Replays or delivers an invalidation to the bridge"
    autoLayout lr 220 140
}
dynamic server "HS2-NativeIssueWrite" "An authorized write to a GitHub Issues source. Other clients learn of it only through a webhook (HS2-M81CCP); see HS2-Flow-NativeProviderWrite." {
    title "8 End-to-end flows · Native issue write (summary)"
    bridgeUsesTrust "Forwards an authenticated write for a qualified ticket"
    trustAdmitsTickets "Admits the checkout-scoped operation"
    ticketApiUsesSource "Resolves the GitHub connection and native issue identity"
    sourceUsesGithub "Applies the provider-native mutation; the server publishes no change event for it"
    githubDeliversWebhook "Later delivers a signed issue event, when a webhook is configured"
    webhooksPublish "Re-reads the issue and publishes a changed invalidation"
    autoLayout lr 220 140
}
dynamic hs2 "HS2-HeadlessTicketWrite" "The CLI direct store path, with no server in the write path." {
    title "8 End-to-end flows · Headless ticket write (summary)"
    contributorUsesCli "Runs a direct ticket command in the project"
    cliCallsGitStore "Persists the ticket through the shared Git ticket engine"
    autoLayout lr 220 140
}
dynamic server "HS2-UnixTerminal" "Typical Unix terminal launch and interaction using the detached broker." {
    title "8 End-to-end flows · Unix terminal (summary)"
    bridgeUsesTrust "Forwards the authenticated terminal request"
    trustAdmitsTerminal "Authorizes the checkout terminal operation"
    terminalUsesBroker "Starts or attaches to the PTY owner"
    autoLayout lr 220 140
}
dynamic server "HS2-ScheduledCommand" "An eligible AI command with a successful auto-start handoff on a hosted Git source." {
    title "8 End-to-end flows · Scheduled command (summary)"
    scheduleReadsSettings "Resolves a due occurrence in the checkout schedule"
    scheduleUsesTickets "Checks for unfinished work from the command and persists one Up Next ticket"
    scheduleClaimsTicket "Claims the exact ticket under the chosen session worker"
    scheduleReadsSettings "Persists that dispatch is starting"
    scheduleHandsOff "Hands off the exact occurrence ticket"
    driveUsesTool "Starts the provider turn"
    autoLayout lr 220 140
}
dynamic server "HS2-OrchestrationPlacement" "One eligible placement through a reviewed plan. Peer-host traffic is shown in the remote deployment view." {
    title "8 End-to-end flows · Orchestration placement (summary)"
    orchUsesPlan "Reads the checked-in plan and target policy"
    orchUsesState "Loads run, resource, and capacity observations"
    orchUsesTickets "Checks the target work and prerequisite readiness"
    orchUsesTrust "Checks the required operation authority"
    orchUsesDrive "Places the authorized session on its admitted target"
    orchUsesJobs "Observes required validation and CI results"
    autoLayout lr 220 140
}
dynamic hs2 "HS2-LegacyImport" "Local import of a copied legacy database into an HS2 Git source." {
    title "8 End-to-end flows · Legacy import (summary)"
    bridgeRunsMigration "Starts an explicit local migration"
    migrationReadsLegacy "Exports from a copy of the HS1 database"
    migrationWritesStore "Imports records and attachments into the Git store"
    autoLayout lr 220 140
}

// HS2-M81CCP: end-to-end flows. Each required flow has a view and a failure or
// recovery view. Narrated hop by hop in narrative/70-flows.md.

// 1. Ticket edit
dynamic server "HS2-Flow-TicketEdit" "Ticket edit: one coalesced browser save through the bridge, authorization, Git write, synchronous index upsert, change event and another tab's refresh." {
    title "8 End-to-end flows · Ticket edit"
    contributorUsesWeb "Types in the detail editor; drafts coalesce and save once on blur or page hide"
    webCallsBridge "Sends PATCH /__hotsheet/project-api/<id>/checkouts/<c>/tickets/<id> with expected_token"
    bridgeUsesTrust "Forwards with the instance secret (local) or the browser session (remote)"
    trustAdmitsTickets "Admits the caller and counts an active mutation"
    ticketApiUsesHosting "Resolves the checkout and the hosted Git store"
    ticketApiUsesSource "Checks the expected token against the indexed ticket, then routes to the Git path"
    sourceWritesGit "ops::update writes the file atomically under the note-mutation lock"
    ticketApiUsesIndex "Upserts the row in the same request and remembers the hash to suppress the watcher echo"
    reconcileWritesIndex "Writes the SQLite row"
    ticketApiPublishes "Emits updated into the replay ring, then the broadcast bus"
    ticketApiKicksSync "Kicks a sync pass; the background committer commits the file"
    eventsDeliverBridge "Pushes the event to every subscribed bridge socket"
    bridgeDeliversWebEvents "Delivers it to the other tab, which re-reads the ticket after its barrier"
    autoLayout lr 220 140
}
dynamic bridge "HS2-Flow-TicketEditFailure" "Ticket edit failure: the server dies under a save. The bridge re-supervises, answers 503 for the PATCH, and the browser rolls back while the draft stays." {
    title "8 End-to-end flows · Ticket edit failure"
    hostWebCallsRoutes "Sends the coalesced PATCH"
    hostRoutesProxy "Strips the project prefix and forwards"
    hostProxyResolves "Looks up the project session"
    hostProxyCallsServer "Forwards; the request fails at transport level"
    hostProxyResupervises "Forgets the verified instance"
    hostSupervisorReadsMachine "Finds no live registration for the store"
    hostSupervisorRunsServer "Launches a replacement server and waits for /health"
    flowProxyAnswersWeb "Answers 503 completion unknown; the browser rolls back the row and keeps the draft"
    autoLayout lr 220 140
}

// 2. Out-of-band edit
dynamic server "HS2-Flow-OutOfBandEdit" "Out-of-band edit: a git pull, a sync pass or a direct CLI write changes a ticket file; the watcher re-indexes it and clients refresh." {
    title "8 End-to-end flows · Out-of-band edit"
    flowDeveloperEditsStore "Pulls or edits a ticket file outside Hot Sheet"
    syncUsesGit "Or: the sync loop fetches another machine's commits"
    flowSyncWritesStore "Integrates them into the working tree; sync itself emits no ticket event"
    reconcileReadsGit "Receives native events, debounced 150 ms, and reads each changed path"
    reconcileWritesIndex "Upserts or deletes the row unless the hash is this server's own write"
    reconcilePublishes "Emits changed or deleted"
    eventsDeliverBridge "Pushes the event to subscribed bridges"
    bridgeDeliversWebEvents "Delivers it; the browser refreshes rows and counts"
    autoLayout lr 220 140
}
dynamic server "HS2-Flow-ReplayOverflow" "Recovery: a dropped or lagged stream reconnects from its cursor; an overflow forces a full re-read. A server restart resets the cursor without a signal." {
    title "8 End-to-end flows · Replay overflow"
    webCallsBridge "Long-polls /ws/poll?since=<last cursor> after the socket drops"
    bridgeOpensStream "Forwards the poll with its authority"
    eventsUseHosting "Takes a presence lease for the checkout"
    eventsDeliverBridge "Answers the backlog, or overflow when since+1 left the 512-event ring"
    bridgeDeliversWebEvents "Delivers overflow; the browser resyncs"
    webCallsBridge "Re-reads the ticket list"
    bridgeUsesTrust "Forwards the read"
    trustAdmitsTickets "Admits it"
    ticketApiUsesIndex "Serves the indexed page"
    middlewareStampsCursor "Stamps X-Hotsheet-Change-Cursor so the next poll resumes there"
    autoLayout lr 220 140
}

// 3. Native provider write
dynamic server "HS2-Flow-NativeProviderWrite" "Native provider write: a synchronous GitHub, GitLab or Jira edit, and the opt-in Jira write-behind queue." {
    title "8 End-to-end flows · Native provider write"
    webCallsBridge "Sends the ticket PATCH for a qualified connection:native id"
    bridgeUsesTrust "Forwards with its authority"
    trustAdmitsTickets "Admits the checkout operation"
    ticketApiUsesSource "Routes a non-Git source to do_provider_update"
    sourcesUseCredentials "Reads the managed sign-in token once"
    credentialsUseOs "Reads it from the OS credential store"
    sourceUsesGithub "Updates the issue; no change event is published"
    githubDeliversWebhook "A configured webhook later delivers the issue event"
    webhooksPublish "Re-reads the issue and publishes changed"
    eventsDeliverBridge "Delivers the invalidation to other clients"
    trustAdmitsWriteBehind "Or, for Jira with write_behind: admits a queued batch and answers 202"
    writeBehindPersists "Persists the operations in the per-store outbox"
    writeBehindPublishes "Publishes updated for the provisional ticket"
    autoLayout lr 220 140
}
dynamic server "HS2-Flow-NativeProviderFailure" "Native provider failure: a rate limit reaches the browser as a plain 409 on the synchronous path; the Jira write-behind defers, retries and asks for attention." {
    title "8 End-to-end flows · Native provider failure"
    webCallsBridge "Sends the synchronous PATCH"
    bridgeUsesTrust "Forwards"
    trustAdmitsTickets "Admits"
    ticketApiUsesSource "Calls the provider adapter"
    sourceUsesGithub "Gets 429 or a rate-limit 403; GitHub also starts a local cooldown"
    eventsDeliverBridge "Nothing is published; the 409 carries the message and the browser rolls back"
    writeBehindUsesSources "Write-behind: the dispatcher resolves the Jira adapter every 2 s"
    writeBehindUsesJira "Reads back, then writes with the concurrency token; RateLimited defers the connection"
    writeBehindPersists "Records deferral, backoff or needs_attention"
    writeBehindPublishes "Publishes updated for each claimed and settled operation"
    autoLayout lr 220 140
}

// 4. Claim lifecycle
dynamic hs2 "HS2-Flow-ClaimLifecycle" "Claim lifecycle: a claim through the CLI (direct store) and MCP (server), renewal, lazy expiry and session-end release. The web UI only displays claims." {
    title "8 End-to-end flows · Claim lifecycle"
    contributorUsesCli "Runs hotsheet-cli claim <slug> --worker w1 --eta"
    cliCallsGitStore "ops::claim_with_eta writes the lease and a Claim event and commits"
    toolCallsMcp "An AI session calls hotsheet_claim"
    mcpCallsServer "POSTs /checkouts/<c>/tickets/<id>/claim with the launched bearer"
    serverCallsGitStore "Writes the same frontmatter through ops and emits claimed"
    serverDeliversBridgeEvents "Delivers claimed, renewed and released"
    bridgeDeliversWebEvents "The workspace shows the holder and lease clock"
    cliCallsGitStore "renew extends the lease; after the lease end it is refused as expired"
    serverCallsGitStore "A session that ends releases its worker's claims in every hosted store"
    serverSyncsGit "Publishes the claim commits to the store remote"
    autoLayout lr 220 140
}
dynamic orchLib "HS2-Flow-ClaimContention" "Claim contention between two machines: a git_ref-bound store decides at the remote by compare-and-swap; an unbound store is settled silently by the semantic merge." {
    title "8 End-to-end flows · Claim contention"
    cliLinksCore "Machine A: hotsheet-cli claim runs ops::claim_with_eta"
    orchLeaseRulesInCore "The claim gate runs before the frontmatter write"
    orchClaimsUseCas "Acquires refs/hotsheet/claims/<ULID>"
    orchCasUsesGit "Force-with-lease push; a lost race retries up to 4 times"
    serverLinksCore "Machine B: the server claims the same ticket"
    orchLeaseRulesInCore "Its gate runs"
    orchClaimsUseCas "The marker is held by A: HeldBy"
    orchClaimsUseStore "B rewrites its copy to name A and answers WrongWorker"
    serverLinksCore "Unbound store: after a pulling sync, newest updated_at wins in the merge"
    orchClaimsUseStore "Bound store with a provisional offline claim: reconcile records the conflict in the Git dir"
    serverDrivesTool "The sync loop pushes the conflict once to the losing session"
    autoLayout lr 220 140
}

// 5. AI session
dynamic server "HS2-Flow-AiSession" "AI session from the UI: terminal launch with setup, detached broker, MCP attach to the launching server, and completion that releases claims." {
    title "8 End-to-end flows · AI session"
    contributorUsesWeb "Chooses a tool in the terminal drawer"
    webCallsBridge "POST /terminals with connect, model and cwd"
    bridgeUsesTrust "Forwards with its authority"
    trustAdmitsTerminal "Admits the launch (not host-gated)"
    terminalUsesAiTools "Resolves the tool and writes setup"
    aiToolsUsePlugins "Reads the manifest's MCP and hook declarations"
    pluginsWriteCheckout "Writes .mcp.json, hooks and instructions idempotently"
    terminalUsesBroker "BrokerRequest::Open with HOTSHEET_SERVER, SECRET, PROJECT, TERMINAL_ID and WORKER_ID"
    brokerRunsTool "Spawns the tool in a PTY"
    toolCallsMcp "Starts hotsheet-mcp --project"
    flowMcpCallsTrust "Attaches to the launching server and calls checkout routes"
    trustAdmitsTickets "Admits ticket and claim tools"
    brokerReportsTerminal "Reports the PTY gone when the tool exits"
    flowTerminalReleasesClaims "Releases the session worker's claims"
    terminalPublishes "Emits terminal_exited"
    eventsDeliverBridge "Delivers it; the drawer refreshes the dashboard"
    autoLayout lr 220 140
}
dynamic server "HS2-Flow-AiPermission" "AI session permission: the tool's hook asks the server, the UI decides; failure falls back to the tool's own prompt or deny." {
    title "8 End-to-end flows · AI permission"
    toolRunsHook "Runs hotsheet-cli permission-hook --agent <tool>"
    hookAsksPermission "POSTs /permissions/ask and blocks with no client timeout"
    permissionsPublish "Parks the ask (24 h, deny default) and emits permission_asked"
    eventsDeliverBridge "Delivers the event"
    bridgeDeliversWebEvents "The workspace refetches GET /permissions and shows the prompt"
    webCallsBridge "POSTs the decision to /permissions/<id>"
    bridgeUsesTrust "Forwards"
    trustAdmitsPermissions "Admits the answer; an Always answer persists an encrypted rule"
    permissionsPublish "Emits permission_resolved; the parked ask returns the decision"
    autoLayout lr 220 140
}

// 6. Scheduled command to placement and CI gate
dynamic server "HS2-Flow-ScheduledCommand" "Scheduled command: due occurrence, duplicate guard, Up Next ticket, exact claim and a headless drive turn; failure releases the claim and retries the same ticket." {
    title "8 End-to-end flows · Scheduled command"
    scheduleReadsSettings "Wakes at the next due time (60 s cap) and reads the schedule cursor"
    scheduleUsesTickets "Skips when the command has an open ticket; else creates one Up Next ticket tagged hs2-command"
    scheduleClaimsTicket "Claims it for the drive's session worker with a 30 min ETA"
    scheduleReadsSettings "Records DeliveryDispatching so a restart does not resend"
    scheduleHandsOff "send_scheduled_drive_turn: always a chat drive, never a terminal"
    driveUsesTool "Starts the provider turn"
    scheduleClaimsTicket "Failure: releases the claim and notes the failure; the next pass retries the same ticket"
    autoLayout lr 220 140
}
dynamic server "HS2-Flow-PlacementToGate" "From a new ticket to orchestration placement and a CI gate observation. Schedules reach orchestration only through ticket events; remote placement is in HS2-Orch-RefusedPlacement." {
    title "8 End-to-end flows · Placement to gate"
    ticketApiPublishes "The scheduled ticket's create emits created"
    flowEventsWakeOrch "The target follower treats it as a ticket change; auto re-plan may add a step"
    orchUsesPlan "Reads the plan and policy"
    orchUsesTickets "Checks readiness of the step's ticket"
    orchUsesTrust "Asks the permission ledger for session_start"
    orchUsesHostHealth "Reads capacity"
    orchUsesDrive "Starts the session locally, or posts target-sessions to the paired peer"
    orchUsesJobs "A gate link records a CI gate for the commit"
    flowOrchStartsMonitor "Ensures the detached hotsheet-cli job monitor"
    cliUsesGithub "The monitor observes GitHub Actions or GitLab CI"
    flowCliObservesCi "or Buildkite, CircleCI, Jenkins, Azure Pipelines and Bitbucket"
    jobsUseMachine "The gate follower reads the finished watch Job"
    orchUsesState "Applies the typed reading; a red gate blocks dependents and wakes a pass"
    autoLayout lr 220 140
}

// 7. Desktop cold start and shutdown
dynamic hs2 "HS2-Flow-DesktopColdStart" "Desktop cold start: relay install, Node bridge, WebView, then the lazy server start on the first project open." {
    title "8 End-to-end flows · Desktop cold start"
    contributorUsesDesktop "Launches the app"
    flowDesktopInstallsRelay "macOS: runs hotsheet-cli terminal-relay install"
    desktopLaunchesBridge "Spawns Node; waits up to 30 s for the ready file"
    bridgeUsesMachine "Publishes client.json and the stable hook CLI"
    flowDesktopHostsWeb "Opens the WebView on the bridge origin"
    webCallsBridge "Lists checkouts and opens a project"
    bridgeRunsCli "Runs setup --refresh for the project"
    flowBridgeStartsServer "No live instance: launches hotsheet-server detached"
    serverUsesMachine "Takes the writer lock and publishes instance files"
    serverUsesBroker "Pings or spawns the detached broker and resumes surviving terminals"
    bridgeCallsServer "Checks /compatibility and posts /projects/open"
    autoLayout lr 220 140
}
dynamic hs2 "HS2-Flow-DesktopShutdown" "Desktop shutdown: page flush, bridge stop; the server, broker, terminals, claims and drafts survive. A failed flush keeps the app open." {
    title "8 End-to-end flows · Desktop shutdown"
    flowDesktopHostsWeb "Close requested: dispatches the flush event with a token"
    webCallsBridge "Flushes drafts and mirrors hotsheet.* keys to desktop storage"
    flowWebAcksDesktop "Acknowledges with desktop_flush_complete; no answer in 30 s cancels the close"
    flowDesktopStopsBridge "On exit: closes stdin, kills after 5 s"
    bridgeUsesMachine "Removes client.json and its snapshot"
    serverUsesBroker "Untouched: the server and broker keep running"
    autoLayout lr 220 140
}

// 8. Remote device and remote project
dynamic server "HS2-Flow-RemoteDevice" "Remote device: a browser on another device pairs with this host, the owner approves, the device redeems a session and opens a registered project; revocation closes its streams." {
    title "8 End-to-end flows · Remote device"
    contributorUsesWeb "Remote browser: enters a label on the sign-in screen"
    webCallsBridge "POST /__hotsheet/remote-auth/pair"
    bridgeForwardsPairing "Forwards; 10 requests a minute; 15 min TTL"
    pairingPersists "Stores the challenge digest"
    pairingPublishes "Emits an identity-free pairing_requests event to device managers"
    eventsDeliverBridge "Delivers it to the owner's workspace"
    operatorUsesWeb "Owner approves with role and grants"
    bridgeForwardsPairing "Approve, then the device redeems; HttpOnly session and CSRF cookies"
    webCallsBridge "Device opens a registered project and edits"
    bridgeUsesTrust "Forwards the session cookie and CSRF, never the secret"
    trustUsesPairing "Validates the session, Origin and CSRF"
    trustAdmitsTickets "Admits the checkout operation"
    eventsUsePairing "Revocation: streams recheck each second and close"
    autoLayout lr 220 140
}
dynamic bridge "HS2-Flow-RemoteProject" "Remote project: this host pairs with a peer server, registers the reviewed binding, then proxies a remote: project; a changed or revoked binding answers 503." {
    title "8 End-to-end flows · Remote project"
    hostWebCallsRoutes "POST /__hotsheet/remotes/pair"
    hostRoutesRemote "Starts pairing with the peer"
    hostRemoteCallsPeer "Calls the peer's /remote-auth/pair; later redeems after approval there"
    operatorUsesWeb "The peer's owner approves this host as a device"
    hostRemoteRunsCli "Runs hotsheet-cli remote register and review"
    cliUsesMachine "Writes remotes.json and keychain credentials"
    hostRoutesOpenProjects "Opens a remote checkout"
    hostSessionsRemote "Reads the private connection; rejects a binding changed mid-open"
    hostRoutesProxy "Forwards later requests"
    hostProxyRemote "Re-reads the binding before each forward"
    hostRemoteReadsMachine "Checks remotes.json is current"
    hostRemoteCallsPeer "Sends with the private session, CSRF and client certificate"
    flowProxyAnswersWeb "A revoked or changed binding answers 503 and closes the tab's sockets"
    autoLayout lr 220 140
}

// 9. Fresh install (headless parity)
dynamic hs2 "HS2-Flow-FreshInstallGui" "Fresh install, GUI path: the bridge runs the same hotsheet-cli bootstrap and setup commands, then registers through the server." {
    title "8 End-to-end flows · Fresh install GUI"
    contributorUsesDesktop "Opens the app on a new machine"
    desktopLaunchesBridge "Starts the bridge"
    webCallsBridge "Chooses a folder and 'Create a Hot Sheet 2 git ticket repository'"
    bridgeRunsCli "Runs hotsheet-cli init for server-bootstrap.hs2, then bootstrap --project --store --prefix HS2"
    cliCallsGitStore "Initializes the store, merge driver and first commit"
    cliReadsCheckout "Writes the .hotsheet2/store link and AI-tool setup"
    cliUsesMachine "Registers the checkout in checkouts.json"
    flowBridgeStartsServer "Starts the machine server"
    bridgeCallsServer "Posts /projects/open and PUTs the checkout source"
    serverCallsGitStore "Hosts and watches the store"
    autoLayout lr 220 140
}
dynamic hs2 "HS2-Flow-FreshInstallHeadless" "Fresh install, headless path: the same bootstrap with no server; remote creation prints exact next steps." {
    title "8 End-to-end flows · Fresh install headless"
    contributorUsesCli "Runs hotsheet-cli bootstrap --project <root>"
    cliCallsGitStore "Initializes or repairs the store, merge driver and first commit"
    cliReadsCheckout "Writes the link, worklist and detected AI-tool setup"
    cliUsesMachine "Registers the checkout in checkouts.json"
    cliSyncsGit "With --remote adds origin; otherwise prints the exact commands"
    cliCallsServer "Later: hotsheet-cli launch starts or finds the machine host"
    autoLayout lr 220 140
}

// 10. Migration
dynamic bridge "HS2-Flow-Migration" "Hot Sheet 1 migration from the UI: a durable job with locks and checkpoints runs hotsheet-migrate; an interrupted job is retried and resumes from per-ticket checkpoints." {
    title "8 End-to-end flows · Migration"
    hostWebCallsRoutes "POST /__hotsheet/projects/migration-jobs kind import"
    hostRoutesMigration "Starts the job"
    hostMigrationSessions "Requires the owning project session"
    hostMigrationWritesMachine "Writes the job checkpoint before launch"
    hostMigrationRunsMigrator "Holds the job lock and runs hotsheet-migrate --progress-json"
    migrationReadsLegacy "Exports from a copy of the PGLite datadir"
    migrationWritesStore "Imports with per-ticket checkpoints, verifies, writes the receipt and commits"
    flowMigrationAnswersWeb "Long-polls progress; a dead owner reads as interrupted"
    hostMigrationUsesGit "Backup: pushes and records proof"
    autoLayout lr 220 140
}
