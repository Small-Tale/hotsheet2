developer = person "Project contributor" "Uses tickets, workspaces, terminals, and headless commands."
operator = person "Host owner or operator" "Configures ticket sources, authorizes remote access, and sets orchestration and execution policy."

aiTool = softwareSystem "AI coding tools" "Installed Claude, Codex, Antigravity, and OpenCode surfaces. Actual launch, drive, permission, and telemetry capabilities vary by tool and adapter." "External"
gitRemote = softwareSystem "Git repository remotes" "User-selected remotes for ticket-store and source-code repositories." "External"
github = softwareSystem "GitHub" "Authoritative GitHub Issues, optional webhook delivery, and supported Actions job observations." "External"
gitlab = softwareSystem "GitLab" "Authoritative GitLab Issues and supported CI job observations." "External"
jira = softwareSystem "Jira" "Authoritative Jira issues and provider-native writes." "External"
ciServices = softwareSystem "Buildkite and CircleCI" "Optional additional CI job sources observed by the Jobs monitor." "External"
notifierServices = softwareSystem "Notification destinations" "Configured local or external notifier adapters, including webhook, messaging, push, email, and SMS." "External"
sharedClaims = softwareSystem "Shared resource claim backends" "Optional Upstash Redis or an approved external backend for resource leases. Git-ref leases use the selected repository remote." "External"
osServices = softwareSystem "Host OS services" "Native filesystem notifications, process and terminal facilities, and OS credential storage." "External"
legacy = softwareSystem "Hot Sheet 1 project" "Legacy PGLite project data used as an import source." "External"

hs2 = softwareSystem "Hot Sheet 2" "Local-first, provider-neutral ticketing, terminal sessions, and coordination across registered checkouts and machines." {
    !docs narrative
    !adrs decisions

    web = container "Hot Sheet workspace" "Renders tickets, settings, workspaces, orchestration, and terminals. Projects share the browser or desktop web UI." "TypeScript, Kerf, xterm.js" {
        url "${SOURCE}/clients/web/src/main.tsx"

        webWorkspace = component "Workspace and feature state" "Owns active projects, ticket and orchestration views, drafts, filters, and optimistic projections." "Kerf" {
            url "${SOURCE}/clients/web/src/app/runtime.tsx"
        }
        webApi = component "API client" "Makes checkout-scoped requests and decodes results and source warnings through the bridge." "TypeScript" {
            url "${SOURCE}/clients/web/src/api.ts"
        }
        webChanges = component "Change-stream workers" "Share or dedicate live subscriptions and report reconnect, replay, and invalidation state to the workspace." "Web Worker / SharedWorker" {
            url "${SOURCE}/clients/web/src/features/project-change-streams.ts"
        }
        webTerminal = component "Terminal frontend" "Renders xterm output and controls terminal attach, replay, resize, and input." "xterm.js" {
            url "${SOURCE}/clients/web/src/terminal-replay-stream.ts"
        }
    }

    desktop = container "Tauri desktop shell" "Owns the native window and desktop lifecycle. Starts the bundled Node host and presents the workspace in a WebView." "Tauri / Rust" {
        url "${SOURCE}/clients/web/src-tauri/src/lib.rs"
    }
    bridge = container "Web host and project bridge" "Serves the web UI, opens local projects, manages host routes, forwards checkout-scoped HTTP and streams, and brokers local and remote session access." "Node.js / Hono" {
        url "${SOURCE}/clients/web/src/local-host.ts"
    }
    server = container "Hot Sheet server" "Long-lived API, watch and replay host. Selects authoritative ticket sources and owns local session, notification, scheduling, and remote orchestration runtime." "Rust / Axum / Tokio" {
        url "${SOURCE}/crates/hotsheet-server/src/main.rs"

        serverTrust = component "Transport and authorization" "Applies local instance, paired-device, certificate, and remote-operation authority at the request boundary." "Rust / rustls" {
            url "${SOURCE}/crates/hotsheet-server/src/routes/auth.rs"
        }
        serverTickets = component "Checkout ticket API" "Resolves checkouts, merges source pages, routes writes and leases by qualified identity, and maps provider capabilities into the API." "Rust / Axum" {
            url "${SOURCE}/crates/hotsheet-server/src/routes/checkouts"
        }
        serverSources = component "Provider adapters and Git operations" "Runs Git ticket operations or delegates to GitHub, GitLab, and Jira adapters. Hosts Jira write-behind where supported." "hotsheet-ticketing / hotsheet-extsync" {
            url "${SOURCE}/crates/hotsheet-server/src/routes/providers"
        }
        serverIndex = component "File reconciliation and search" "Restores and reconciles per-store SQLite projections, watches for file changes, and coordinates Git sync." "hotsheet-index / filesystem watcher" {
            url "${SOURCE}/crates/hotsheet-server/src/watcher.rs"
        }
        serverEvents = component "Change stream" "Publishes live WebSocket invalidations and supports bounded replay through cursor-based long polling." "WebSocket / HTTP long poll" {
            url "${SOURCE}/crates/hotsheet-server/src/events.rs"
        }
        serverDrive = component "AI session and permission runtime" "Discovers tools, launches sessions, relays permission decisions, runs supported provider turns, and tracks delivery." "hotsheet-plugins / hotsheet-aitools" {
            url "${SOURCE}/crates/hotsheet-server/src/client_drive.rs"
        }
        serverTerminal = component "Terminal API and broker client" "Manages terminal launch, attach, replay, input, and sizing. Owns PTYs directly on Windows or when the Unix broker is disabled." "hotsheet-terminals" {
            url "${SOURCE}/crates/hotsheet-server/src/routes/terminals"
        }
        serverSchedule = component "Command schedule runner" "Evaluates local AI command occurrences, guards duplicate work, persists outcomes, and prepares exact-ticket handoff." "hotsheet-ticketing / Jiff" {
            url "${SOURCE}/crates/hotsheet-server/src/routes/command_schedules.rs"
        }
        serverOrch = component "Orchestration runtime" "Reconciles work against reviewed plans, readiness, permissions, capacity, and machine resources. Dispatches or hosts target work and observes durable results." "Rust / checkout and peer APIs" {
            url "${SOURCE}/crates/hotsheet-server/src/routes/orchestration.rs"
        }
        serverJobs = component "Job observation and validation" "Tracks supported CI, validation, and deployment job outcomes and reports them to orchestration." "hotsheet-jobs / process journal" {
            url "${SOURCE}/crates/hotsheet-jobs/src/lib.rs"
        }
        serverResources = component "Resource claim service" "Resolves configured backends and grants renewable leases for non-ticket resources." "hotsheet-resource-claims" {
            url "${SOURCE}/crates/hotsheet-resource-claims/src/service.rs"
        }
        serverNotify = component "Notifications and standby publication" "Routes host events to the workspace and configured channels and publishes the macOS standby snapshot." "hotsheet-notify / JSON snapshots" {
            url "${SOURCE}/crates/hotsheet-server/src/notification_router.rs"
        }
    }

    cli = container "Hot Sheet CLI" "Runs headless ticket operations and operational commands. Can bootstrap or control a server and delegate remote operations where applicable." "Rust / Clap" {
        url "${SOURCE}/crates/hotsheet-cli/src/main.rs"
    }
    mcp = container "Hot Sheet MCP shim" "Presents stdio JSON-RPC tools to an AI process. Routes to the matching server or opens a Git store directly in serverless mode." "Rust / MCP" {
        url "${SOURCE}/crates/hotsheet-mcp/src/main.rs"
    }
    broker = container "Detached terminal broker" "Owns Unix PTYs and bounded replay independently of the serving process. Keeps eligible sessions alive through server restart." "Rust / local IPC / PTY" {
        url "${SOURCE}/crates/hotsheet-terminals/src/bin/broker.rs"
    }
    launchRelay = container "macOS terminal launch relay" "Optional launch-agent path that starts terminal owners outside the initiating desktop process lineage." "Rust / launchd / local IPC" {
        url "${SOURCE}/crates/hotsheet-terminals/src/launch_relay.rs"
    }
    saver = container "macOS standby screen saver" "Watches the published credential-free snapshot and displays it in the OS screen-saver host." "Swift / WebKit" {
        url "${SOURCE}/clients/macos-screensaver/Sources/HotSheetSaverView.swift"
    }
    migrator = container "Hot Sheet 1 migrator" "Runs the legacy export in a separate Node process and imports it into an HS2 Git store." "Rust / Node.js / PGLite" {
        url "${SOURCE}/crates/hotsheet-cli/src/bin/hotsheet-migrate.rs"
    }
    tickets = container "Git ticket store" "Authoritative Markdown ticket files, attachment payloads and store metadata under local Git history. A checkout can link multiple stores." "Filesystem / Git" "Data Store" {
        url "${SOURCE}/crates/hotsheet-ticketing/src/store.rs"
    }
    searchIndex = container "Local ticket index" "Rebuildable per-store query and full-text search projection." "SQLite / FTS5" "Data Store,Derived" {
        url "${SOURCE}/crates/hotsheet-index/src/lib.rs"
    }
    checkoutData = container "Code checkout and project settings" "Source tree, checkout-scoped configuration and shared orchestration definitions. Machine-local project settings hold local command schedules." "Filesystem / Git / JSON" "Data Store" {
        url "${SOURCE}/crates/hotsheet-ticketing/src/settings.rs"
    }
    machineData = container "Machine state" "Instance and connection registries, device sessions, local orchestration records, resource claims, journals, and private credential envelopes." "Machine-local files" "Data Store" {
        url "${SOURCE}/crates/hotsheet-server/src/lib.rs"
    }
    standby = container "Standby snapshot" "Credential-free projection written to the screen-saver container." "JSON" "Data Store,Derived" {
        url "${SOURCE}/crates/hotsheet-server/src/standby_saver.rs"
    }
}
