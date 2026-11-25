systemContext hs2 "HS2-Context" "People, tool integrations, authoritative providers, and host services." {
    include *?
    autoLayout lr
    default
}
container hs2 "HS2-WorkspaceRuntime" "Main desktop and web runtime, headless entry points, Git storage, and terminal ownership." {
    include developer web desktop bridge server cli mcp broker launchRelay tickets searchIndex checkoutData machineData aiTool
    exclude serverRunsCli bridgeReadsCheckout cliUsesMachine serverDeliversBridgeEvents bridgeDeliversWebEvents
    autoLayout tb
}
container hs2 "HS2-ProviderBoundaries" "Authoritative Git or external provider records, local indexed reads, and Git synchronization." {
    include server cli tickets searchIndex gitRemote github gitlab jira
    exclude cliCallsServer serverRunsCli
    autoLayout lr
}
container hs2 "HS2-MigrationAndStandby" "Optional local processes for legacy import and the macOS standby display." {
    include bridge migrator legacy tickets server standby saver
    exclude bridgeCallsServer serverDeliversBridgeEvents serverCallsGitStore
    autoLayout lr
}
component web "HS2-WebComponents" "Workspace state, checkout API access, event workers, and terminal frontend." {
    include webWorkspace webApi webChanges webTerminal bridge
    autoLayout lr
}
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
dynamic server "HS2-WebGitTicketWrite" "One checkout-scoped Git write through the browser bridge and the server." {
    bridgeUsesTrust "Forwards the checkout-qualified request with its corresponding authority"
    trustAdmitsTickets "Admits the request to the checkout ticket API"
    ticketApiUsesSource "Selects the configured local Git source"
    sourceWritesGit "Persists the authoritative change in the store"
    ticketApiUsesIndex "Refreshes the affected local projection"
    ticketApiPublishes "Publishes a scoped invalidation"
    eventsDeliverBridge "Delivers live change or replay to the bridge"
    autoLayout lr
}
dynamic server "HS2-GitReconciliation" "Out-of-band store edit or sync converges into the local search projection and live stream." {
    reconcileReadsGit "Consumes a file event and reconciles the affected ticket"
    reconcileWritesIndex "Applies the updated row or removes a deleted row"
    reconcilePublishes "Publishes the scoped post-reconcile change"
    eventsDeliverBridge "Replays or delivers an invalidation to the bridge"
    autoLayout lr
}
dynamic server "HS2-NativeIssueWrite" "An authorized write to a GitHub Issues source. GitLab and Jira have their own provider capabilities." {
    bridgeUsesTrust "Forwards an authenticated write for a qualified ticket"
    trustAdmitsTickets "Admits the checkout-scoped operation"
    ticketApiUsesSource "Resolves the GitHub connection and native issue identity"
    sourceUsesGithub "Applies the provider-native mutation"
    ticketApiPublishes "Publishes an invalidation on a successful result"
    autoLayout lr
}
dynamic hs2 "HS2-HeadlessTicketWrite" "The CLI direct store path, with no server in the write path." {
    contributorUsesCli "Runs a direct ticket command in the project"
    cliCallsGitStore "Persists the ticket through the shared Git ticket engine"
    autoLayout lr
}
dynamic server "HS2-UnixTerminal" "Typical Unix terminal launch and interaction using the detached broker." {
    bridgeUsesTrust "Forwards the authenticated terminal request"
    trustAdmitsTerminal "Authorizes the checkout terminal operation"
    terminalUsesBroker "Starts or attaches to the PTY owner"
    autoLayout lr
}
dynamic server "HS2-ScheduledCommand" "An eligible AI command with a successful auto-start handoff on a hosted Git source." {
    scheduleReadsSettings "Resolves a due occurrence in the checkout schedule"
    scheduleUsesTickets "Checks for unfinished work from the command and persists one Up Next ticket"
    scheduleClaimsTicket "Claims the exact ticket under the chosen session worker"
    scheduleReadsSettings "Persists that dispatch is starting"
    scheduleHandsOff "Hands off the exact occurrence ticket"
    driveUsesTool "Starts the provider turn"
    autoLayout lr
}
dynamic server "HS2-OrchestrationPlacement" "One eligible placement through a reviewed plan. Peer-host traffic is shown in the remote deployment view." {
    orchUsesPlan "Reads the checked-in plan and target policy"
    orchUsesState "Loads run, resource, and capacity observations"
    orchUsesTickets "Checks the target work and prerequisite readiness"
    orchUsesTrust "Checks the required operation authority"
    orchUsesDrive "Places the authorized session on its admitted target"
    orchUsesJobs "Observes required validation and CI results"
    autoLayout lr
}
dynamic hs2 "HS2-LegacyImport" "Local import of a copied legacy database into an HS2 Git source." {
    bridgeRunsMigration "Starts an explicit local migration"
    migrationReadsLegacy "Exports from a copy of the HS1 database"
    migrationWritesStore "Imports records and attachments into the Git store"
    autoLayout lr
}
deployment hs2 "Desktop on Unix" "HS2-DesktopDeployment" "User-managed Tauri/Node/Rust deployment with a detached terminal broker." {
    include *
    autoLayout tb
}
deployment hs2 "Headless and serverless" "HS2-HeadlessDeployment" "Direct CLI and serverless MCP paths." {
    include *
    autoLayout lr
}
deployment hs2 "Remote browser and peer server" "HS2-RemoteDeployment" "A paired browser, orchestrator, and a user-managed Hot Sheet target server." {
    include *
    autoLayout tb
}
