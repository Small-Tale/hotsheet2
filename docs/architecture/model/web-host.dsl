// Node web host and project bridge, and the Tauri desktop shell.
// Owned by HS2-T6SD67. Included inside the hs2 software system block by system.dsl.
// Keep element identifiers stable (README "Model conventions").

    desktop = container "Tauri desktop shell" "Owns the native window and desktop lifecycle. Starts the bundled Node host and presents the workspace in a WebView." "Tauri / Rust" {
        url "${SOURCE}/clients/web/src-tauri/src/lib.rs"

        tauriShell = component "Shell bootstrap and bridge launcher" "Release builds spawn the bundled Node runtime on dist-host/local-host.mjs with pinned binary paths and a ready file, wait up to 30 s for its URL, and stop it by closing stdin on exit. Debug builds attach to the dev server on 127.0.0.1:4175 instead. Never restarts a bridge that dies." "Rust / std::process" {
            url "${SOURCE}/clients/web/src-tauri/src/lib.rs"
        }
        tauriWebview = component "Main WebView and link routing" "Builds the single main window on the bridge origin, injects the desktop-storage flag in release, grants a per-origin dynamic capability, and sends external links and window.open targets to the system opener." "Tauri WebviewWindow" {
            url "${SOURCE}/clients/web/src-tauri/src/external_links.rs"
        }
        tauriClose = component "Controlled close lifecycle" "Intercepts window close and app quit in release builds, asks the page to flush desktop preferences, accepts one generation-fenced acknowledgement from the main webview at the bridge origin, and cancels after 30 s." "Rust / Tauri commands" {
            url "${SOURCE}/clients/web/src-tauri/src/close_lifecycle.rs"
        }
        tauriRelay = component "Terminal relay installer" "macOS release builds only: runs the bundled hotsheet-cli terminal-relay install before the bridge starts, bounded to 10 s; a failure never blocks startup." "Rust / child process" {
            url "${SOURCE}/clients/web/src-tauri/src/terminal_relay.rs"
        }
    }
    bridge = container "Web host and project bridge" "Serves the web UI, opens local projects, manages host routes, forwards checkout-scoped HTTP and streams, and brokers local and remote session access." "Node.js / Hono" {
        url "${SOURCE}/clients/web/src/local-host.ts"

        hostProductionHost = component "Production local host" "local-host.ts entry: selects current release or debug binaries, snapshots dist and binaries into a private temporary directory, mounts desktop storage, the bridge route table and the peer gateway in front of static dist, installs WebSocket handlers, publishes client.json and the ready file, and exits on SIGINT, SIGTERM or stdin EOF." "Node.js / Hono / node:http" {
            url "${SOURCE}/clients/web/src/local-host.ts"
        }
        hostViteDev = component "Vite development host" "vite.config.ts plugins: serve the client from source on 127.0.0.1:4175, load dev-server.ts through @hono/vite-dev-server for an allow-list of bridge paths, install the project WebSocket bridge, and publish client.json. No peer gateway and no desktop storage." "Vite / @hono/vite-dev-server" {
            url "${SOURCE}/clients/web/vite.config.ts"
        }
        hostRoutes = component "Bridge route table" "createDevApp: every /__hotsheet HTTP route both hosts serve, including project open, folder chooser, Git setup, migration jobs, remote servers, change multiplexing, the project API proxy entry and dev latency." "Hono" {
            url "${SOURCE}/clients/web/src/dev-server.ts"
        }
        hostBrowserAuth = component "Remote browser authorization" "With HOTSHEET_BROWSER_AUTH_MODE=remote, checks each /__hotsheet request's session with the machine server, enforces session, admin and local grants plus Origin and CSRF, and forwards /remote-auth routes without the machine secret." "Hono middleware / fetch" {
            url "${SOURCE}/clients/web/src/dev-server.ts"
        }
        hostSessions = component "Project sessions and opener" "Process-global session registry keyed by browser project id, holding each target's server URL and secret or private remote connection; opens local, registered and reviewed remote projects, refreshes setup through the CLI, and checks compatibility." "TypeScript module state" {
            url "${SOURCE}/clients/web/src/project-bridge.ts"
        }
        hostSupervisor = component "Machine server supervisor" "Discovers the bootstrap server from instance files, probes /health, launches hotsheet-server detached with a rotated stderr log, waits on registration with backoff, reconciles the pinned build, and performs explicit unhealthy-server recovery." "TypeScript / child process" {
            url "${SOURCE}/clients/web/src/project-bridge.ts"
        }
        hostProxy = component "Checkout request proxy" "Forwards /__hotsheet/project-api requests and multiplexed change polls to the checkout-scoped server path with the loopback secret or the browser session, rewrites remote answers for tab ownership, and re-supervises after a transport failure." "fetch / node:https" {
            url "${SOURCE}/clients/web/src/project-bridge.ts"
        }
        hostStreams = component "Project WebSocket bridge" "Upgrade handler for terminal attach, per-project change sync and the change multiplexer; resolves authenticated upstream URLs, pipes frames, groups change subscriptions per upstream and routes events by store id." "ws" {
            url "${SOURCE}/clients/web/src/terminal-ws-bridge.ts"
        }
        hostRemoteProjects = component "Remote project bridge" "Pairs with another Hot Sheet server, keeps private session credentials and TLS material in Node memory or the CLI's private store, re-checks remotes.json on every forward, and maps composite tab ids to native checkout ids." "TypeScript / hotsheet-cli remote" {
            url "${SOURCE}/clients/web/src/remote-projects/host.ts"
        }
        hostPeerGateway = component "Peer gateway" "Remote-mode production host only: relays the machine server's native top-level routes and WebSockets to this host's own server, stripping any presented machine secret." "Hono middleware / ws" {
            url "${SOURCE}/clients/web/src/peer-gateway.ts"
        }
        hostDesktopStorage = component "Desktop storage bridge" "Loopback, same-origin routes that persist hotsheet.* preferences and staged draft attachment bytes as private atomic per-key files, so desktop state survives the ephemeral bridge port." "Hono / node:fs" {
            url "${SOURCE}/clients/web/src/desktop-storage-bridge.ts"
        }
        hostMigration = component "Migration and Git setup jobs" "Durable HS1 import and backup jobs with file checkpoints, a native lock held by hotsheet-migrate, long-poll status watches, and the Git ticket-store create and remote-connect workflows." "TypeScript / child process" {
            url "${SOURCE}/clients/web/src/migration-jobs.ts"
        }
        hostConversationExport = component "Conversation export bridge" "Chooses a destination folder and writes or reopens .hotsheet-chat bundles with bounded assets and atomic text writes." "node:fs" {
            url "${SOURCE}/clients/web/src/conversation-export-bridge.ts"
        }
        hostContracts = component "Shared bridge contracts" "Modules loaded by both the Node host and the browser: the browser's bridge client and route constants, server-response parsers, compatibility assessment, change multiplexer wire formats, host features, client build ids and remote project identity." "TypeScript (both runtimes)" {
            url "${SOURCE}/clients/web/src/local-bridge-client.ts"
        }
    }
