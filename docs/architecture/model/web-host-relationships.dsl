// web-host relationships. Owned by HS2-T6SD67. Both endpoints belong to this area,
// or one endpoint is an external system or person. Edges between areas go in
// relationships-cross.dsl. Included after every element, so order does not matter.

desktopLaunchesBridge = desktop -> bridge "Starts the bundled host and loads its local origin" "Child process / HTTP"

// Tauri shell internals (HS2-T6SD67).
hostContributorLaunchesShell = developer -> tauriShell "Launches the installed app, optionally with --feature flags" "Native launch"
tauriShellInstallsRelay = tauriShell -> tauriRelay "Installs the macOS terminal launch relay before starting the bridge" "Rust"
tauriShellBuildsWebview = tauriShell -> tauriWebview "Builds the main window on the URL from the ready file" "Rust"
tauriShellRunsHost = tauriShell -> hostProductionHost "Spawns bundled Node on local-host.mjs, polls the ready file, closes stdin on exit" "Child process / file"
tauriWebviewLoadsHost = tauriWebview -> hostProductionHost "Loads the application document, assets and bridge routes" "HTTP / WebSocket"
tauriCloseAsksWebview = tauriClose -> tauriWebview "Evaluates the page's close hook and its cancel hook after 30 s" "WebView eval"
tauriShellUsesOs = tauriShell -> osServices "Reads the resource and app-data directories and the executable directory" "Tauri path API"
tauriWebviewUsesOs = tauriWebview -> osServices "Opens external http, https and mailto targets in the system handler" "tauri-plugin-opener"

// Host entry points (HS2-T6SD67).
hostDesktopUsesProductionHost = desktop -> hostProductionHost "Spawns, then loads the application from the release host" "Child process / HTTP"
hostProductionMountsRoutes = hostProductionHost -> hostRoutes "Mounts createDevApp(true) under the static client" "Hono"
hostProductionMountsStorage = hostProductionHost -> hostDesktopStorage "Mounts when a storage directory is set and the host is in local mode" "Hono"
hostProductionMountsGateway = hostProductionHost -> hostPeerGateway "Mounts the HTTP gateway and installs its upgrade handler" "Hono / node:http"
hostProductionInstallsStreams = hostProductionHost -> hostStreams "Installs the project WebSocket upgrade handler" "node:http upgrade"
hostProductionPinsBinaries = hostProductionHost -> hostSupervisor "Pins snapshot binary paths through process environment" "Environment"
hostViteLoadsRoutes = hostViteDev -> hostRoutes "Loads dev-server.ts for allow-listed bridge paths" "@hono/vite-dev-server"
hostViteInstallsStreams = hostViteDev -> hostStreams "Installs the project WebSocket upgrade handler on the Vite server" "node:http upgrade"

// Route table to services (HS2-T6SD67).
hostRoutesUseAuth = hostRoutes -> hostBrowserAuth "Admits remote-mode requests by session grants, Origin and CSRF" "Hono middleware"
hostRoutesOpenProjects = hostRoutes -> hostSessions "Opens local, registered and reviewed remote projects; lists checkouts" "TypeScript"
hostRoutesProxy = hostRoutes -> hostProxy "Forwards project-api requests and change-mux polls" "TypeScript"
hostRoutesRemote = hostRoutes -> hostRemoteProjects "Pairs, lists, discovers, renews, selects and revokes remote servers" "TypeScript"
hostRoutesMigration = hostRoutes -> hostMigration "Starts and watches migration jobs; sets up Git ticket stores" "TypeScript"
hostRoutesExports = hostRoutes -> hostConversationExport "Chooses destinations and writes or opens conversation bundles" "TypeScript"
hostRoutesRecover = hostRoutes -> hostSupervisor "Runs explicit unhealthy-server recovery" "TypeScript"
hostSessionsSupervise = hostSessions -> hostSupervisor "Ensures the bootstrap server and restarts it for an upgrade" "TypeScript"
hostSessionsRemote = hostSessions -> hostRemoteProjects "Reads discovery and the private connection for a reviewed binding" "TypeScript"
hostProxyResolves = hostProxy -> hostSessions "Looks up the session target for a project id" "Session registry"
hostProxyResupervises = hostProxy -> hostSupervisor "Forgets the verified instance and re-supervises after a failed forward" "TypeScript"
hostProxyRemote = hostProxy -> hostRemoteProjects "Re-checks the binding and sends with the private session and TLS" "TypeScript"
hostStreamsResolve = hostStreams -> hostSessions "Resolves authenticated terminal and sync upstream URLs" "TypeScript"
hostStreamsAuth = hostStreams -> hostBrowserAuth "Checks the upgrade's session and local grant in remote mode" "TypeScript"
hostAuthSupervise = hostBrowserAuth -> hostSupervisor "Ensures the bootstrap server before forwarding remote-auth" "TypeScript"
hostGatewaySupervise = hostPeerGateway -> hostSupervisor "Resolves this host's own machine server URL" "TypeScript"
hostMigrationSessions = hostMigration -> hostSessions "Requires the owning project session and its ticket store" "Session registry"
hostSessionsProxy = hostSessions -> hostProxy "Reads a remote peer's /compatibility through the proxy" "TypeScript"
hostRoutesUseContracts = hostRoutes -> hostContracts "Shares route constants and parsers with the browser" "TypeScript"

// External systems (HS2-T6SD67).
hostSessionsUseOs = hostSessions -> osServices "Runs the native folder chooser and reveals corrupt ticket files" "osascript / PowerShell / zenity / open"
hostExportsUseOs = hostConversationExport -> osServices "Writes and reads bundles in a chosen folder" "Filesystem"
hostStorageUsesOs = hostDesktopStorage -> osServices "Writes private per-key files in the desktop app-data directory" "Filesystem"
hostMigrationUsesGit = hostMigration -> gitRemote "Adds and pushes the HS1 backup remote of a new ticket store" "git"
hostMigrationReadsLegacy = hostMigration -> legacy "Detects and imports Hot Sheet 1 data" "Filesystem / child process"
