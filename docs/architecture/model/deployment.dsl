// Deployment environments. Shared; HS2-M81CCP owns changes.

desktopUnix = deploymentEnvironment "Desktop on Unix" {
    desktopMachine = deploymentNode "Contributor machine" "User-run desktop installation. The Tauri shell owns the Node bridge; the server and broker are detached and outlive both. The bridge starts the server lazily on the first request that needs it. The launch relay exists only on macOS (HS2-M81CCP)." "macOS / Unix" {
        desktopUi = containerInstance desktop
        desktopWorkspace = containerInstance web
        desktopBridge = containerInstance bridge
        desktopServer = containerInstance server
        desktopBroker = containerInstance broker
        desktopCli = containerInstance cli
        desktopMcp = containerInstance mcp
        desktopStore = containerInstance tickets
        desktopIndex = containerInstance searchIndex
        desktopSettings = containerInstance checkoutData
        desktopState = containerInstance machineData
        desktopRelay = containerInstance launchRelay
    }
}

headlessLocal = deploymentEnvironment "Headless and serverless" {
    terminalMachine = deploymentNode "Contributor machine" "Direct Git-store paths. Explicit CLI or MCP modes select the local store. No server is required; a running machine host is only kicked to sync. The detached Job monitor is also a hotsheet-cli process (HS2-M81CCP)." "Local operating system" {
        headlessCli = containerInstance cli
        headlessMcp = containerInstance mcp
        headlessStore = containerInstance tickets
        headlessSettings = containerInstance checkoutData
        headlessIndex = containerInstance searchIndex
        headlessState = containerInstance machineData
    }
}

peerHosts = deploymentEnvironment "Remote browser and peer server" {
    operatorGroup = deploymentGroup "Operator host"
    targetGroup = deploymentGroup "Target host"
    browserDevice = deploymentNode "Remote browser device" "A paired browser using the host bridge." "Browser" {
        remoteWorkspace = containerInstance web operatorGroup
    }
    orchestratorHost = deploymentNode "User-managed orchestrator host" "Hosts the browser entry and owns the reviewed orchestration plan." "User machine" {
        remoteBridge = containerInstance bridge operatorGroup
        ownerServer = containerInstance server operatorGroup
        ownerState = containerInstance machineData operatorGroup
        ownerSettings = containerInstance checkoutData operatorGroup
    }
    workerHost = deploymentNode "User-managed target host" "Hosts the target checkout, evaluates admission, and owns local execution." "User machine" {
        targetIngress = infrastructureNode "Target peer API endpoint" "Network endpoint for paired callers: the target server's remote listener, or a remote-mode Node host's peer gateway in front of it (HS2-M81CCP)." "Authenticated HTTPS / mTLS"
        workerServer = containerInstance server targetGroup
        workerState = containerInstance machineData targetGroup
        workerCheckout = containerInstance checkoutData targetGroup
        workerStore = containerInstance tickets targetGroup
        workerIndex = containerInstance searchIndex targetGroup
    }

    ownerRequestsTarget = ownerServer -> targetIngress "Calls scoped operations and receives target observations" "Authenticated HTTPS"
    ingressRoutesTarget = targetIngress -> workerServer "Serves peer requests under target-side admission" "Rust HTTP listener"
    // HS2-M81CCP: a reviewed remote project is opened by the operator host's bridge, not its server.
    bridgeRequestsTarget = remoteBridge -> targetIngress "Opens reviewed remote projects with the private session and client certificate" "HTTPS / mTLS"
}
