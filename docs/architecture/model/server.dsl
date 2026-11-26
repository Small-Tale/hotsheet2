// Rust server, terminal broker and launch relay, AI drive, notifications, and the macOS standby saver.
// Owned by HS2-P27EJM. Included inside the hs2 software system block by system.dsl.
// Keep element identifiers stable (README "Model conventions").

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

    broker = container "Detached terminal broker" "Owns Unix PTYs and bounded replay independently of the serving process. Keeps eligible sessions alive through server restart." "Rust / local IPC / PTY" {
        url "${SOURCE}/crates/hotsheet-terminals/src/bin/broker.rs"
    }
    launchRelay = container "macOS terminal launch relay" "Optional launch-agent path that starts terminal owners outside the initiating desktop process lineage." "Rust / launchd / local IPC" {
        url "${SOURCE}/crates/hotsheet-terminals/src/launch_relay.rs"
    }
    saver = container "macOS standby screen saver" "Watches the published credential-free snapshot and displays it in the OS screen-saver host." "Swift / WebKit" {
        url "${SOURCE}/clients/macos-screensaver/Sources/HotSheetSaverView.swift"
    }
    standby = container "Standby snapshot" "Credential-free projection written to the screen-saver container." "JSON" "Data Store,Derived" {
        url "${SOURCE}/crates/hotsheet-server/src/standby_saver.rs"
    }
