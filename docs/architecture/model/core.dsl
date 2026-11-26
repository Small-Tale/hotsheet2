// Core ticketing libraries and the authoritative and derived ticket stores.
// Owned by HS2-AH9HAK. Included inside the hs2 software system block by system.dsl.
// Keep element identifiers stable (README "Model conventions").
// Descriptions write the machine home as HOTSHEET_HOME without a dollar-brace, because
// the DSL substitutes dollar-brace names as constants.

    tickets = container "Git ticket store" "Authoritative Markdown ticket files (tickets/<last 2 ULID chars>/<ULID>.md), attachment payloads (attachments/<ticket>/<attachment>/<file>), hotsheet-store.json metadata and a managed .gitignore that excludes machine-local runtime directories. The private Git directory holds sync health, coordination locks and recovery markers. A checkout can link multiple stores." "Filesystem / Git" "Data Store" {
        url "${SOURCE}/crates/hotsheet-ticketing/src/store.rs"
    }
    searchIndex = container "Local ticket index" "Rebuildable per-store SQLite projection with FTS5 at HOTSHEET_HOME/index/<store hash>.v<schema>.sqlite. Reconciled from ticket file hashes and a Git baseline; never authoritative." "SQLite / FTS5" "Data Store,Derived" {
        url "${SOURCE}/crates/hotsheet-index/src/lib.rs"
    }
    checkoutData = container "Code checkout and project settings" "Source tree, checkout-scoped configuration (.hotsheet2/settings.json shared, settings.local.json local), the committed group registry (.hotsheet2/groups/<id>.json), the derived worklist.md, and shared orchestration definitions. Machine-local project settings hold local command schedules." "Filesystem / Git / JSON" "Data Store" {
        url "${SOURCE}/crates/hotsheet-ticketing/src/settings.rs"
    }
    coreLib = container "Ticketing core libraries" "Linked Rust crates for the ticket model, Git store and operations, provider contracts and adapters, sync and merge, the index, and shared utilities. Linked into the server, CLI and MCP processes; not a process of its own." "Rust crates" "Library" {
        url "${SOURCE}/crates/hotsheet-ticketing/src/lib.rs"

        // ---- ticket model and Git store ----------------------------------------------
        coreModel = component "Ticket model and file format" "Pure types, ULID slug derivation, guarded schema markers v2 to v6, and the round-trip YAML frontmatter plus bounded Markdown body and notes parser and serializer. No I/O." "Rust crate hotsheet-model / serde_yaml" {
            url "${SOURCE}/crates/hotsheet-model/src/format.rs"
        }
        coreStore = component "Git file store" "FsStore: sharded ticket paths, atomic temp-and-rename writes, the per-store note-mutation lock, path-scoped autocommits under the Git transaction lock, a background committer, headless best-effort push, store admission and Git ownership classification." "Rust / git subprocess" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/store.rs"
        }
        coreOps = component "Ticket operations" "The single query, create, update, note, close, copy, move and claim implementation every surface calls. Synchronous; the caller injects time and ids. Writes go through the store transaction and committing writes." "Rust" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/ops.rs"
        }
        coreWire = component "Wire DTOs and checkout ordering" "ApiTicket and TicketRow (one JSON shape for server, MCP and CLI), the concurrency token, the global checkout order, the cross-source keyset page merge and cursor codec, and saved-view evaluation." "Rust / serde_json" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/wire.rs"
        }

        // ---- sync and merge --------------------------------------------------------------
        coreSync = component "Store sync" "One fetch-integrate-push cycle per remote-backed store: isolated replay or live rebase through the merge driver, bounded push and fetch race retries, lock and stale-lock handling, durable sync health, saved-write recovery and the ask-an-AI repair ticket." "Rust / git subprocess" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/sync.rs"
        }
        coreMerge = component "Semantic ticket merge" "Pure 3-way field merge (scalars last-writer-wins by updated_at, set unions, notes by ULID, body text conflict surfaced) plus local registration of the hotsheet-ticket merge driver and the line-ending pin." "Rust" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/merge.rs"
        }

        // ---- index -----------------------------------------------------------------------
        coreIndex = component "Search index" "Opens, rebuilds and reconciles the per-store SQLite and FTS5 projection, answers bounded queries and counts, resolves slugs index-first with a store-walk fallback, and prunes older schema generations." "Rust crate hotsheet-index / rusqlite" {
            url "${SOURCE}/crates/hotsheet-index/src/lib.rs"
        }

        // ---- provider contract and adapters ----------------------------------------------
        coreProviderContract = component "Provider contract and transfer" "TicketProvider trait, ProviderCapabilities, ProviderError, ProviderRegistry routing by connection id, keyset paging helpers, provider Trash purge, and copy_between and move_between with operation-id provenance." "Rust" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/provider.rs"
        }
        coreGitProvider = component "Git provider adapter" "GitProvider: maps native ULID ids to ticket operations, checks expected concurrency tokens, records transfer provenance and reports the full Git capability set." "Rust" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/provider.rs"
        }
        coreProviderConfig = component "Provider connection configuration" "Machine-wide providers.json registry with a sibling lock and atomic publication, one-time migration from per-store layouts, and the fenced GitHub endpoint state kept in connection settings." "Rust / JSON files" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/provider/machine_config.rs"
        }
        coreOutbox = component "Provider outbox" "Durable SQLite admission log for provisional external field edits: idempotent batches, backpressure, per-ticket sequence, dispatch states and retention. Used only by the server's opt-in Jira write-behind." "Rust / rusqlite (WAL, synchronous FULL)" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/provider_outbox.rs"
        }
        coreGithub = component "GitHub Issues adapter" "Issues over REST with a shared ureq transport, issue-list cache, rate-limit cooldown, label-based Trash, assets-repository attachments, device-flow sign-in and webhook setup." "Rust crate hotsheet-extsync / ureq" {
            url "${SOURCE}/crates/hotsheet-extsync/src/github.rs"
        }
        coreGitlab = component "GitLab Issues adapter" "Issues over REST v4 with PRIVATE-TOKEN auth, label-based Trash and native project uploads for attachments." "Rust crate hotsheet-extsync / ureq" {
            url "${SOURCE}/crates/hotsheet-extsync/src/gitlab.rs"
        }
        coreJira = component "Jira adapter" "Jira issues over REST v3 with Basic auth, native attachments with metadata properties, label-based Trash, advisory claim markers and the write-behind capability." "Rust crate hotsheet-extsync / ureq" {
            url "${SOURCE}/crates/hotsheet-extsync/src/jira.rs"
        }
        coreExtShared = component "Source connection workflow" "Validates and links a new provider connection, builds live adapters from a connection and token, projects metadata-only source identity, recognizes native id shapes offline, and encodes note metadata into comment trailers." "Rust crate hotsheet-extsync" {
            url "${SOURCE}/crates/hotsheet-extsync/src/source_connect.rs"
        }

        // ---- machine and project state -----------------------------------------------
        coreCheckouts = component "Checkout and source registry" "Machine checkouts.json registry: readable checkout ids, many-to-many source links, relocation and respelling, removal of checkouts and connections, the managed project .gitignore block, and the machine-home resolver." "Rust / JSON files" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/checkouts.rs"
        }
        coreSettings = component "Settings" "Global, shared and local settings scopes with bounded per-file locks, typed setting definitions (features, terminal limits, host health, standby, permission automation, project commands) and local AI command schedules." "Rust / JSON files" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/settings.rs"
        }
        coreGroups = component "Ticket groups" "Group directory derived from tickets, the committed per-checkout group registry, rename, merge and migrate workflows, Git publication of registry files, and automatic-grouping reply validation." "Rust / Git" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/groups.rs"
        }
        coreSecrets = component "Secrets and accounts" "KeyRegistry over the OS credential store: AES-256-GCM envelopes under HOTSHEET_HOME/machine-secrets keyed by one native master item, keys.json metadata, environment overrides, and machine-wide accounts derived from connections." "Rust / ring / OS keychain" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/secrets.rs"
        }
        coreRemoteServers = component "Remote server bindings" "Machine-local reviewed bindings to peer Hot Sheet servers (remotes.json), server identity, selected remote projects and bounded checkout observation. Transport and secrets are injected ports." "Rust / JSON files" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/remote_servers/mod.rs"
        }
        coreActivityMetrics = component "Activity, metrics and reports" "Per-device activity and usage JSONL, per-contributor rollups, pricing and repricing, activity distillation, phase timing and reports, confidence calibration, live-state history and flow analytics." "Rust / JSONL files" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/metrics.rs"
        }
        coreRepoMaintenance = component "Repository maintenance" "Attachment-history compaction with a recovery marker, repository size and status reports, and host-health threshold and disk definitions." "Rust / git subprocess" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/repository_compaction.rs"
        }
        coreMedia = component "Media and glance helpers" "Non-destructive image crop renditions (static and animated), the macOS screen-saver snapshot and install workflow, and plugin directory digests." "Rust / image / libavif" {
            url "${SOURCE}/crates/hotsheet-ticketing/src/image_crop.rs"
        }

        // ---- small utility crates ------------------------------------------------------
        coreUtil = component "Shared leaf utilities" "hotsheet-sync: poison-recovering locks, blank-as-unset path environment variables, one canonical path spelling, and atomic file replacement that works on Windows." "Rust crate hotsheet-sync" {
            url "${SOURCE}/crates/hotsheet-sync/src/lib.rs"
        }
        coreTls = component "mTLS material" "Per-project CA, server leaf, device certificate issue, renew and revoke, and device roles under HOTSHEET_HOME/tls/<project id>/." "Rust crate hotsheet-tls / rcgen" {
            url "${SOURCE}/crates/hotsheet-tls/src/lib.rs"
        }
        coreProcessJournal = component "Process journal" "Machine-local child-process provenance records under HOTSHEET_HOME/processes and the spawn and detach helpers that write them." "Rust crate hotsheet-process-journal" {
            url "${SOURCE}/crates/hotsheet-process-journal/src/lib.rs"
        }
    }
