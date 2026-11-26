// Node web host and Tauri shell views.
// Owned by HS2-T6SD67. New views use keys prefixed HS2-WebHost-.

component bridge "HS2-WebHost-BridgeComponents" "Node web host and project bridge: entry points, route table, session registry, supervisor, proxy and stream bridges." {
    title "3 Web host and desktop · Bridge components"
    include *
    autoLayout lr 220 140
}

component desktop "HS2-WebHost-DesktopComponents" "Tauri desktop shell: bridge launch, WebView, controlled close and the terminal relay install." {
    title "3 Web host and desktop · Desktop components"
    include *
    autoLayout lr 220 140
}

dynamic bridge "HS2-WebHost-DesktopColdStart" "Release desktop launch through the first project open. Failure: if Node exits or writes no ready file within 30 s, the shell kills it and setup fails, so no window opens." {
    title "3 Web host and desktop · Flow: desktop cold start"
    desktop -> hostProductionHost "Spawns bundled Node on local-host.mjs with binary paths, the storage directory and a ready-file path"
    hostProductionHost -> hostStreams "Installs the project WebSocket upgrade handler"
    hostProductionHost -> machineData "Publishes client.json, then writes the ready file with its ephemeral URL"
    desktop -> hostProductionHost "Reads the ready file and loads the application in the main WebView"
    web -> hostRoutes "Lists checkouts, then opens a project"
    hostRoutes -> hostSessions "Resolves the root and linked store, refreshes setup"
    hostSessions -> cli "Runs compatibility and setup --refresh (a refusal only adds a warning)"
    hostSessions -> hostSupervisor "Ensures the bootstrap server"
    hostSupervisor -> machineData "Finds no live registration in instances/"
    hostSupervisor -> server "Launches hotsheet-server detached, then waits for registration and a healthy /health (90 s budget)"
    hostSessions -> server "Reads /compatibility and posts /projects/open"
    autoLayout lr 220 140
}

dynamic bridge "HS2-WebHost-ProxiedCheckoutRequest" "One local checkout-scoped API request. Failure: a transport error re-supervises the server; GET or HEAD is retried once and any other method answers 503." {
    title "3 Web host and desktop · Flow: proxied checkout request"
    web -> hostRoutes "Sends /__hotsheet/project-api/{project}/..."
    hostRoutes -> hostProxy "Strips the prefix and keeps the query"
    hostProxy -> hostSessions "Looks up the session target; an unknown project answers 404"
    hostProxy -> server "Forwards to the checkout-scoped path with x-hotsheet-secret (local mode) or the session cookie and CSRF (remote mode)"
    hostProxy -> hostSupervisor "On a transport failure, forgets the verified instance and re-supervises every session on that store"
    hostProxy -> server "Retries a GET or HEAD once; a write returns 503 with completion unknown"
    autoLayout lr 220 140
}

dynamic bridge "HS2-WebHost-TerminalWebSocket" "Terminal attach through the bridge. Failure: an unresolvable target is refused with 404 at once; either side closing closes the other, and the browser reconnects." {
    title "3 Web host and desktop · Flow: terminal WebSocket"
    web -> hostStreams "Upgrades /__hotsheet/project-api/{project}/terminals/{id}/attach with replay parameters"
    hostStreams -> hostBrowserAuth "Remote mode only: checks same-origin, the session, and the local grant for remote tabs"
    hostStreams -> hostSessions "Resolves the authenticated upstream URL"
    hostSessions -> hostSupervisor "Re-supervises the server so the URL and secret are current"
    hostStreams -> server "Opens /terminals/{id}/attach with the secret in the query (local) or forwarded session headers"
    server -> hostStreams "Streams PTY bytes and replay frames, relayed unchanged"
    autoLayout lr 220 140
}

dynamic bridge "HS2-WebHost-RemoteProjectAccess" "Opening and using a project on a paired remote server. Failure: a revoked or changed binding answers 503 and closes that tab's sockets." {
    title "3 Web host and desktop · Flow: remote project access"
    web -> hostRoutes "Posts /__hotsheet/remotes/{id}/projects/{checkout}/open"
    hostRoutes -> hostSessions "Opens the reviewed remote project"
    hostSessions -> hostRemoteProjects "Discovers checkouts and reads the private connection"
    hostRemoteProjects -> cli "Runs hotsheet-cli remote checkouts and bridge-connection"
    hostSessions -> hostProxy "Reads the peer's /compatibility through the new session"
    hostProxy -> hostRemoteProjects "Re-checks remotes.json and sends with the private session cookie, Origin, CSRF and TLS"
    hostRemoteProjects -> server "Calls the peer's checkout-scoped API with the native checkout id"
    hostRoutes -> hostRemoteProjects "Records the selected project with hotsheet-cli remote select-project"
    autoLayout lr 220 140
}

dynamic bridge "HS2-WebHost-ServerCrashRecovery" "The machine server dies while a project is open; the bridge relaunches it on the next request or socket." {
    title "3 Web host and desktop · Flow: server crash recovery"
    web -> hostRoutes "Sends a project request after the server died"
    hostRoutes -> hostProxy "Forwards the request"
    hostProxy -> server "Forward fails with a transport error"
    hostProxy -> hostSupervisor "Forgets the verified instance and re-supervises"
    hostSupervisor -> machineData "Finds the registration stale (process gone)"
    hostSupervisor -> server "Launches a detached replacement and waits until healthy"
    hostProxy -> server "Retries GET or HEAD on the new URL and secret; writes return 503"
    autoLayout lr 220 140
}

dynamic bridge "HS2-WebHost-BridgeExit" "Desktop quit and the Node bridge's own exit. Failure: if Node crashes, nothing restarts it; the window keeps the dead origin until the app is relaunched." {
    title "3 Web host and desktop · Flow: bridge exit"
    desktop -> hostProductionHost "On app exit, closes the bridge's stdin and waits up to 5 s, then kills it"
    hostProductionHost -> hostStreams "Closes the HTTP server and all connections, with a 3 s deadline"
    hostProductionHost -> machineData "Removes its client.json record; the detached server keeps running"
    autoLayout lr 220 140
}
