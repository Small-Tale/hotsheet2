// Rust server, terminal broker and launch relay, AI drive, notifications, and the macOS standby saver.
// Owned by HS2-P27EJM. Included inside the hs2 software system block by system.dsl.
// Keep element identifiers stable (README "Model conventions").

    server = container "Hot Sheet server" "Long-lived API, watch and replay host. Selects authoritative ticket sources and owns local session, notification, scheduling, and remote orchestration runtime." "Rust / Axum / Tokio" {
        url "${SOURCE}/crates/hotsheet-server/src/main.rs"

        // Process, transport and admission (HS2-P27EJM)
        serverLifecycle = component "Process lifecycle and instance registry" "Parses flags, takes the per-store writer lock, publishes 0600 instance files, binds the local and remote listeners, and runs the bounded shutdown drain. Does not supervise or restart itself." "Rust / Tokio" {
            url "${SOURCE}/crates/hotsheet-server/src/lifecycle.rs"
        }
        serverTls = component "TLS listeners" "Builds HTTPS and mutual-TLS server configurations from the store's certificate directory, checks revocation per handshake, and applies the client-certificate ACL role." "Rust / rustls" {
            url "${SOURCE}/crates/hotsheet-server/src/tls.rs"
        }
        serverTrust = component "Transport and authorization" "Applies local instance, paired-device, certificate, and remote-operation authority at the request boundary." "Rust / rustls" {
            url "${SOURCE}/crates/hotsheet-server/src/routes/auth.rs"
        }
        serverPairing = component "Remote device catalog" "Pairing requests, owner approval, redeemed browser sessions, admin and host grants, renewal, rotation and revocation in one machine-local catalog." "Rust / JSON file" {
            url "${SOURCE}/crates/hotsheet-server/src/remote_auth.rs"
        }
        serverMiddleware = component "Request middleware" "Runs non-streaming handlers on the blocking pool, warns on requests over their latency budget, stamps the change cursor, and maps errors to the API error shape." "Rust / Axum middleware" {
            url "${SOURCE}/crates/hotsheet-server/src/blocking_offload.rs"
        }
        serverHosting = component "Store hosting and presence" "Registers hosted stores with their indexes, writer locks and instance files, tracks change-stream leases per checkout, and unhosts project-opened stores no open checkout needs." "Rust" {
            url "${SOURCE}/crates/hotsheet-server/src/hosting.rs"
        }

        // Tickets, sources and change propagation
        serverTickets = component "Checkout ticket API" "Resolves checkouts, merges source pages, routes writes and leases by qualified identity, and maps provider capabilities into the API." "Rust / Axum" {
            url "${SOURCE}/crates/hotsheet-server/src/routes/checkouts"
        }
        serverSources = component "Provider adapters and Git operations" "Runs Git ticket operations or delegates to GitHub, GitLab, and Jira adapters. Hosts Jira write-behind where supported." "hotsheet-ticketing / hotsheet-extsync" {
            url "${SOURCE}/crates/hotsheet-server/src/routes/providers"
        }
        serverWriteBehind = component "Jira write-behind outbox" "Opt-in durable admission of Jira field edits into a per-store SQLite outbox, dispatched every two seconds with readback, conflict capture, backoff and attention states." "Rust / SQLite" {
            url "${SOURCE}/crates/hotsheet-server/src/provider_write_behind.rs"
        }
        serverAdvisory = component "Provider advisory claims" "Serves advisory claims for native issue providers and keeps provisional claims in a machine-local ledger that is replayed." "Rust / SQLite" {
            url "${SOURCE}/crates/hotsheet-server/src/provider_advisory_claims.rs"
        }
        serverCredentials = component "Managed credential cache" "Caches one OS credential read per managed sign-in and metadata revision for provider requests." "Rust / OS credential store" {
            url "${SOURCE}/crates/hotsheet-server/src/credential_cache.rs"
        }
        serverWebhooks = component "GitHub webhook receiver" "Verifies per-hook HMAC deliveries, drops duplicates and stale revisions, and turns issue events into invalidations." "Rust / HMAC-SHA256" {
            url "${SOURCE}/crates/hotsheet-server/src/github_webhooks.rs"
        }
        serverIndex = component "File reconciliation and search" "Restores and reconciles per-store SQLite projections, watches for file changes, and coordinates Git sync." "hotsheet-index / filesystem watcher" {
            url "${SOURCE}/crates/hotsheet-server/src/watcher.rs"
        }
        serverSync = component "Git sync loop" "Syncs every hosted store with its remote on a 30 second base cadence, wakes early on write kicks, backs off to five minutes, and purges trash every six hours." "Rust / Git" {
            url "${SOURCE}/crates/hotsheet-server/src/sync_loop.rs"
        }
        serverEvents = component "Change stream" "Publishes live WebSocket invalidations and supports bounded replay through cursor-based long polling." "WebSocket / HTTP long poll" {
            url "${SOURCE}/crates/hotsheet-server/src/events.rs"
        }
        serverLive = component "Live state and project activity" "Derives per-store live snapshots, rates, activity timelines and stuck causes, and pushes changed snapshots as events." "Rust" {
            url "${SOURCE}/crates/hotsheet-server/src/live_state.rs"
        }
        serverRepository = component "Repository browser and code review" "Answers repository status, files, commits and size for open code checkouts, watches code refs, and launches host review tools behind host access." "Rust / Git" {
            url "${SOURCE}/crates/hotsheet-server/src/code_review.rs"
        }

        // Execution: terminals, AI drive and permissions
        serverDrive = component "AI session and permission runtime" "Discovers tools, launches sessions, relays permission decisions, runs supported provider turns, and tracks delivery." "hotsheet-plugins / hotsheet-aitools" {
            url "${SOURCE}/crates/hotsheet-server/src/client_drive.rs"
        }
        serverPermissions = component "Permission bridge" "Queues tool permission asks FIFO, auto-resolves them from encrypted remembered rules, publishes ask and resolve events, and answers parked hooks." "hotsheet-aitools" {
            url "${SOURCE}/crates/hotsheet-aitools/src/permission.rs"
        }
        serverAiTools = component "AI tool adapters" "Per-tool drive shapes (spawn, app-server, Claude channel, ACP), discovery probes, launch safety, transcript and rollout usage, and session push." "hotsheet-aitools" {
            url "${SOURCE}/crates/hotsheet-aitools/src/lib.rs"
        }
        serverPlugins = component "Plugin manifests and setup" "Loads bundled and machine plugin manifests and performs idempotent project setup: instructions, skills, MCP entries, hooks and pinned guidance." "hotsheet-plugins" {
            url "${SOURCE}/crates/hotsheet-plugins/src/lib.rs"
        }
        serverDistWork = component "Distributed driving loop" "Opt-in loop, enabled only by a drive-tool flag, that claims tickets across hosted stores with a Git remote and drives one AI tool per claimed ticket." "Rust" {
            url "${SOURCE}/crates/hotsheet-server/src/dist_work_loop.rs"
        }
        serverTerminal = component "Terminal API and broker client" "Manages terminal launch, attach, replay, input, and sizing. Owns PTYs directly on Windows or when the Unix broker is disabled." "hotsheet-terminals" {
            url "${SOURCE}/crates/hotsheet-server/src/routes/terminals"
        }
        serverSchedule = component "Command schedule runner" "Evaluates local AI command occurrences, guards duplicate work, persists outcomes, and prepares exact-ticket handoff." "hotsheet-ticketing / Jiff" {
            url "${SOURCE}/crates/hotsheet-server/src/routes/command_schedules.rs"
        }

        // Coordination and host support
        serverOrch = component "Orchestration runtime" "Reconciles work against reviewed plans, readiness, permissions, capacity, and machine resources. Dispatches or hosts target work and observes durable results." "Rust / checkout and peer APIs" {
            url "${SOURCE}/crates/hotsheet-server/src/routes/orchestration.rs"
        }
        serverJobs = component "Job observation and validation" "Tracks supported CI, validation, and deployment job outcomes and reports them to orchestration." "hotsheet-jobs / process journal" {
            url "${SOURCE}/crates/hotsheet-jobs/src/lib.rs"
        }
        serverResources = component "Resource claim service" "Resolves configured backends and grants renewable leases for non-ticket resources." "hotsheet-resource-claims" {
            url "${SOURCE}/crates/hotsheet-resource-claims/src/service.rs"
        }
        serverHostHealth = component "Host health and capacity" "Samples this host's CPU, memory and tool capacity on a server-local cadence and answers capacity readings for orchestration." "Rust" {
            url "${SOURCE}/crates/hotsheet-server/src/host_health/mod.rs"
        }
        serverNotify = component "Notifications and standby publication" "Routes host events to the workspace and configured channels and publishes the macOS standby snapshot." "hotsheet-notify / JSON snapshots" {
            url "${SOURCE}/crates/hotsheet-server/src/notification_router.rs"
        }
        serverNotifiers = component "Notifier plugins" "Rule matching, cooldowns, bounded retry, the delivery journal, built-in notifiers, and external notifier subprocesses." "hotsheet-notify" {
            url "${SOURCE}/crates/hotsheet-notify/src/dispatch.rs"
        }
    }

    broker = container "Detached terminal broker" "Owns Unix PTYs and bounded replay independently of the serving process. Keeps eligible sessions alive through server restart." "Rust / local IPC / PTY" {
        url "${SOURCE}/crates/hotsheet-terminals/src/bin/broker.rs"

        brokerService = component "Broker request loop" "Serves newline-delimited JSON requests and attach streams on the private socket, retires itself after five idle minutes, and answers build identity for hand-off." "Rust / Tokio / Unix socket" {
            url "${SOURCE}/crates/hotsheet-terminals/src/broker.rs"
        }
        brokerSocket = component "Socket ownership" "Chooses a short private socket path and holds exclusive filesystem ownership so one broker serves a project." "Rust / Unix socket" {
            url "${SOURCE}/crates/hotsheet-terminals/src/broker_socket.rs"
        }
        brokerManager = component "Terminal manager" "Keeps the per-project map of lazily spawned PTY terminals and reaps ended ones." "Rust" {
            url "${SOURCE}/crates/hotsheet-terminals/src/manager.rs"
        }
        brokerTerminals = component "PTY terminals and replay" "Spawns PTY children with a scrubbed environment, keeps a bounded replay ring, queues input, infers busy state, and parses OSC sequences." "portable-pty" {
            url "${SOURCE}/crates/hotsheet-terminals/src/terminal.rs"
        }
        brokerSizing = component "Viewer sizing arbiter" "Chooses one PTY size from the viewers' claims and names the viewer that answers terminal queries." "Rust" {
            url "${SOURCE}/crates/hotsheet-terminals/src/sizing.rs"
        }
    }
    launchRelay = container "macOS terminal launch relay" "Optional launch-agent path that starts terminal owners outside the initiating desktop process lineage." "Rust / launchd / local IPC" {
        url "${SOURCE}/crates/hotsheet-terminals/src/launch_relay.rs"

        relayService = component "Relay listener" "Answers hello and launch requests over line JSON (protocol 1, 1 MiB line cap) from same-uid peers only, inside the Aqua session." "Rust / Unix socket" {
            url "${SOURCE}/crates/hotsheet-terminals/src/launch_relay.rs"
        }
        relayLauncher = component "Broker launcher" "Spawns the relay's own broker executable in a new session with the requester's environment minus launchd job context." "Rust / process journal" {
            url "${SOURCE}/crates/hotsheet-server/src/terminal_launch_relay.rs"
        }
        relayBootstrap = component "Bootstrap namespace probe" "Detects a dead Mach bootstrap namespace so the server knows a direct spawn would inherit it." "Rust / Mach" {
            url "${SOURCE}/crates/hotsheet-terminals/src/bootstrap.rs"
        }
        relayAgent = component "LaunchAgent installer" "Writes, loads, inspects and removes the per-user LaunchAgent idempotently for the desktop app and the headless CLI." "Rust / launchctl" {
            url "${SOURCE}/crates/hotsheet-terminals/src/launch_agent.rs"
        }
    }
    saver = container "macOS standby screen saver" "Watches the published credential-free snapshot and displays it in the OS screen-saver host." "Swift / WebKit" {
        url "${SOURCE}/clients/macos-screensaver/Sources/HotSheetSaverView.swift"

        saverWatcher = component "Snapshot watcher" "Watches the snapshot's directory with a file-system dispatch source and rereads the file on change." "Swift / GCD" {
            url "${SOURCE}/clients/macos-screensaver/Sources/HotSheetSaverView.swift"
        }
        saverView = component "Saver view" "Hosts the bundled standby web page in a web view and delivers each snapshot to it." "Swift / ScreenSaver / WebKit" {
            url "${SOURCE}/clients/macos-screensaver/Sources/HotSheetSaverView.swift"
        }
    }
    standby = container "Standby snapshot" "Credential-free projection written to the screen-saver container." "JSON" "Data Store,Derived" {
        url "${SOURCE}/crates/hotsheet-server/src/standby_saver.rs"
    }
