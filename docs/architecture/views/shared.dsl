// System context, container and deployment views. Shared; HS2-M81CCP owns changes.

systemContext hs2 "HS2-Context" "People, tool integrations, authoritative providers, and host services." {
    title "1 System · Context: people, AI tools and providers"
    include *?
    autoLayout lr 220 140
}
container hs2 "HS2-WorkspaceRuntime" "Main desktop and web runtime, headless entry points, Git storage, and terminal ownership." {
    title "1 System · Runtime containers"
    include developer web desktop bridge server cli mcp broker launchRelay tickets searchIndex checkoutData machineData aiTool
    exclude serverRunsCli bridgeReadsCheckout cliUsesMachine serverDeliversBridgeEvents bridgeDeliversWebEvents
    autoLayout tb 220 140
}
container hs2 "HS2-ProviderBoundaries" "Authoritative Git or external provider records, local indexed reads, and Git synchronization." {
    title "1 System · Ticket sources and providers"
    include server cli tickets searchIndex gitRemote github gitlab jira
    exclude cliCallsServer serverRunsCli
    autoLayout lr 220 140
}
container hs2 "HS2-MigrationAndStandby" "Optional local processes for legacy import and the macOS standby display." {
    title "1 System · Migration and standby display"
    include bridge migrator legacy tickets server standby saver
    exclude bridgeCallsServer serverDeliversBridgeEvents serverCallsGitStore
    autoLayout lr 220 140
}
deployment hs2 "Desktop on Unix" "HS2-DesktopDeployment" "User-managed Tauri/Node/Rust deployment with a detached terminal broker." {
    title "1 System · Deployment: desktop on Unix"
    include *
    autoLayout tb 220 140
}
deployment hs2 "Headless and serverless" "HS2-HeadlessDeployment" "Direct CLI and serverless MCP paths." {
    title "1 System · Deployment: headless and serverless"
    include *
    autoLayout lr 220 140
}
deployment hs2 "Remote browser and peer server" "HS2-RemoteDeployment" "A paired browser, orchestrator, and a user-managed Hot Sheet target server." {
    title "1 System · Deployment: remote browser and peer server"
    include *
    autoLayout tb 220 140
}
