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
