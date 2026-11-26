// System context, container and deployment views. Shared; HS2-M81CCP owns changes.

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
