// Browser web client component and dynamic views.
// Owned by HS2-AYKHVV. New views use keys prefixed HS2-WebClient-.

component web "HS2-WebComponents" "Workspace state, checkout API access, event workers, and terminal frontend." {
    include webWorkspace webApi webChanges webTerminal bridge
    autoLayout lr
}
