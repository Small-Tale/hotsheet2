desktopUnix = deploymentEnvironment "Desktop on Unix" {
    desktopMachine = deploymentNode "Contributor machine" "User-run desktop installation. The server and broker have independent lifecycles." "macOS / Unix" {
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
    }
}

headlessLocal = deploymentEnvironment "Headless and serverless" {
    terminalMachine = deploymentNode "Contributor machine" "Direct Git-store paths. Explicit CLI or MCP modes select the local store." "Local operating system" {
        headlessCli = containerInstance cli
        headlessMcp = containerInstance mcp
        headlessStore = containerInstance tickets
        headlessSettings = containerInstance checkoutData
        headlessIndex = containerInstance searchIndex
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
        targetIngress = infrastructureNode "Target peer API endpoint" "Network endpoint served by the target Hot Sheet server." "Authenticated HTTPS"
        workerServer = containerInstance server targetGroup
        workerState = containerInstance machineData targetGroup
        workerCheckout = containerInstance checkoutData targetGroup
        workerStore = containerInstance tickets targetGroup
        workerIndex = containerInstance searchIndex targetGroup
    }

    ownerRequestsTarget = ownerServer -> targetIngress "Calls scoped operations and receives target observations" "Authenticated HTTPS"
    ingressRoutesTarget = targetIngress -> workerServer "Serves peer requests under target-side admission" "Rust HTTP listener"
}
