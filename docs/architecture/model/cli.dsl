// Hot Sheet CLI, MCP shim, and Hot Sheet 1 migrator.
// Owned by HS2-TW2DKM. Included inside the hs2 software system block by system.dsl.
// Keep element identifiers stable (README "Model conventions").
// The hotsheet-cli crate is both the `hotsheet-cli` binary and a library that the
// server and MCP crates link (crates/hotsheet-server/Cargo.toml line 18,
// crates/hotsheet-mcp/Cargo.toml line 30). Components tagged "Library" are linked
// into those processes too.

    cli = container "Hot Sheet CLI" "Runs headless ticket operations and operational commands. Can bootstrap or control a server and delegate remote operations where applicable." "Rust / Clap" {
        url "${SOURCE}/crates/hotsheet-cli/src/main.rs"

        cliEntry = component "Command tree and dispatch" "Parses the Clap tree on an 8 MiB parser thread, resolves the store path (-C, HOTSHEET_STORE, .hotsheet2/store link), reads the actor, and routes each command to exactly one dispatch group." "Rust / Clap" {
            url "${SOURCE}/crates/hotsheet-cli/src/dispatch/mod.rs"
        }
        cliPostCommand = component "Post-command sync kick and notification watch" "After a command, asks a live host to sync every store this process committed to, and delivers ticket-transition notifications itself when no live server hosts the store." "Rust / ureq" "Library" {
            url "${SOURCE}/crates/hotsheet-cli/src/sync_kick.rs"
        }
        cliHostClient = component "Local host discovery and server commands" "Reads instance records under HOTSHEET_HOME/instances, starts a machine host when a launch needs one, runs serve as a child hotsheet-server, and calls host routes with the instance secret." "Rust / ureq" "Library" {
            url "${SOURCE}/crates/hotsheet-cli/src/external_launch.rs"
        }
        cliStoreCmds = component "Store bootstrap, checkout and maintenance commands" "init, bootstrap, link, format, import, checkout registry, sync, doctor, reindex, worklist, purge-trash and the Git merge driver. Opens the store directly." "Rust" {
            url "${SOURCE}/crates/hotsheet-cli/src/dispatch/store.rs"
        }
        cliTicketCmds = component "Ticket, note, attachment and group commands" "Git ticket CRUD, notes, attachments, transfer, people, groups and reports, run directly against the store and its index." "Rust" {
            url "${SOURCE}/crates/hotsheet-cli/src/dispatch/tickets.rs"
        }
        cliClaimCmds = component "Ticket claim commands and guarded runs" "claim, claim-next, renew, release, and claim-run, which keeps one exact claim attempt renewed while a child process tree runs." "Rust" {
            url "${SOURCE}/crates/hotsheet-cli/src/dispatch/claims.rs"
        }
        cliProviderCmds = component "Provider, key and account commands" "Provider connections, provider-native ticket operations, cross-provider copy and move, keychain keys, accounts and feedback synthesis." "Rust / hotsheet-extsync" {
            url "${SOURCE}/crates/hotsheet-cli/src/dispatch/providers.rs"
        }
        cliSetupCmds = component "AI tool setup, launch and drive" "setup and refresh, guidance, plugins, settings, host disks, launch of an interactive tool against the machine host, and the headless trigger and work loops." "Rust / hotsheet-plugins / hotsheet-aitools" {
            url "${SOURCE}/crates/hotsheet-cli/src/dispatch/setup.rs"
        }
        cliPermissionHook = component "Permission hook adapter" "The permission-hook command a tool runs per hook event: maps hook JSON to a tool and action, asks the route-back server, prints the decision, and reports session and question lifecycle to the terminal." "Rust / ureq" {
            url "${SOURCE}/crates/hotsheet-cli/src/permission_hook.rs"
        }
        cliCoordination = component "Local coordination pool" "Durable per-repository mailbox for dispatcher, integrator and worker sessions: assignments, authority, strategies, batches, speculation and merge-pending pruning. Also linked by the server and MCP." "Rust / file locks / Git" "Library" {
            url "${SOURCE}/crates/hotsheet-cli/src/coordination/mod.rs"
        }
        cliOrchestration = component "Orchestration runtime composition" "OrchestrationRuntime wires the orchestration domain to machine files, keychain, remotes, gates, sessions and validation runners. The CLI shapes HTTP-like requests and runs them in-process. Also linked by the server and MCP." "Rust" "Library" {
            url "${SOURCE}/crates/hotsheet-cli/src/orchestration.rs"
        }
        cliRemotes = component "Remote server registry and transport" "Paired remote Hot Sheet servers: registry, keychain credentials, pinned TLS or mTLS transport, and the remote command family." "Rust / rustls / ureq" "Library" {
            url "${SOURCE}/crates/hotsheet-cli/src/remote_servers.rs"
        }
        cliJobs = component "Job and CI watch commands" "Reads the machine Job store and runs the watch monitor against GitHub, GitLab, Buildkite, CircleCI, Jenkins, Azure Pipelines and Bitbucket." "Rust / hotsheet-jobs / ureq" {
            url "${SOURCE}/crates/hotsheet-cli/src/dispatch/jobs.rs"
        }
        cliResources = component "Resource claim commands" "Headless resource claims through the shared ClaimService, backend configuration, and renewal-token calls to a server from a task without a shared home." "Rust / hotsheet-resource-claims" {
            url "${SOURCE}/crates/hotsheet-cli/src/dispatch/resources.rs"
        }
        cliNotify = component "Notifier and remote-change commands" "Headless notifier channels, rules and sends, and remote-change connections, through the same hotsheet-notify workflows as the server." "Rust / hotsheet-notify" {
            url "${SOURCE}/crates/hotsheet-cli/src/dispatch/notify.rs"
        }
    }
    mcp = container "Hot Sheet MCP shim" "Presents stdio JSON-RPC tools to an AI process. Routes to the matching server or opens a Git store directly in serverless mode." "Rust / MCP" {
        url "${SOURCE}/crates/hotsheet-mcp/src/main.rs"

        mcpEntry = component "Stdio loop and backend selection" "Reads one JSON-RPC message per line on one thread and chooses the serverless, explicit-server or project-routed backend, attaching a launched session to its live host." "Rust / Clap" {
            url "${SOURCE}/crates/hotsheet-mcp/src/main.rs"
        }
        mcpTools = component "Tool catalog and dispatch" "Lists 43 hotsheet tools, injects actor and checkout scope, maps each call onto an HTTP-shaped Backend request, and reports tool failures as isError results." "Rust / serde_json" {
            url "${SOURCE}/crates/hotsheet-mcp/src/lib.rs"
        }
        mcpDomainTools = component "Orchestration, remote and resource tools" "hotsheet_targets, hotsheet_remotes and hotsheet_resource_claim: action preflight, route shaping and error mapping for the library runtimes." "Rust" {
            url "${SOURCE}/crates/hotsheet-mcp/src/orchestration_tools.rs"
        }
        mcpCoreBackend = component "Serverless core backend" "Interprets the same HTTP-shaped paths in-process against an admitted FsStore, the checkout registry and the library runtimes, and kicks the host to sync after writes." "Rust / hotsheet-ticketing" {
            url "${SOURCE}/crates/hotsheet-mcp/src/lib.rs"
        }
        mcpHttpBackend = component "Server HTTP backend" "Sends requests to a server with X-Hotsheet-Secret, refuses redirects, and explains authentication and route mismatches." "Rust / ureq" {
            url "${SOURCE}/crates/hotsheet-mcp/src/lib.rs"
        }
        mcpPresence = component "Presence heartbeat" "After initialize, upserts an MCP presence lease every 15 s on one thread, relocates to the registry server when the route fails, and releases the lease on EOF." "Rust / thread" {
            url "${SOURCE}/crates/hotsheet-mcp/src/presence.rs"
        }
    }
    migrator = container "Hot Sheet 1 migrator" "Runs the legacy export in a separate Node process and imports it into an HS2 Git store." "Rust / Node.js / PGLite" {
        url "${SOURCE}/crates/hotsheet-cli/src/bin/hotsheet-migrate.rs"

        migBinary = component "hotsheet-migrate entry" "Standalone binary: runs the Node exporter on a copy of the legacy database, imports, verifies, writes the receipt and handles backup-remote steps." "Rust / Clap" {
            url "${SOURCE}/crates/hotsheet-cli/src/bin/hotsheet-migrate.rs"
        }
        migExporter = component "Node HS1 exporter" "Opens a temporary copy of the PGLite datadir with the bundled engine and writes hotsheet-export.json plus NDJSON progress." "Node.js / PGLite" {
            url "${SOURCE}/migrator/src/export.mjs"
        }
        migImport = component "HS1 importer and recovery" "Imports a portable export through the core ticket writer with per-ticket pending checkpoints, verifies completion and backups, and repairs legacy attachments. Also linked by the CLI import command." "Rust" "Library" {
            url "${SOURCE}/crates/hotsheet-cli/src/import.rs"
        }
    }
