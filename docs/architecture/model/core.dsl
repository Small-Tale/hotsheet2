// Core ticketing libraries and the authoritative and derived ticket stores.
// Owned by HS2-AH9HAK. Included inside the hs2 software system block by system.dsl.
// Keep element identifiers stable (README "Model conventions").

    tickets = container "Git ticket store" "Authoritative Markdown ticket files, attachment payloads and store metadata under local Git history. A checkout can link multiple stores." "Filesystem / Git" "Data Store" {
        url "${SOURCE}/crates/hotsheet-ticketing/src/store.rs"
    }
    searchIndex = container "Local ticket index" "Rebuildable per-store query and full-text search projection." "SQLite / FTS5" "Data Store,Derived" {
        url "${SOURCE}/crates/hotsheet-index/src/lib.rs"
    }
    checkoutData = container "Code checkout and project settings" "Source tree, checkout-scoped configuration and shared orchestration definitions. Machine-local project settings hold local command schedules." "Filesystem / Git / JSON" "Data Store" {
        url "${SOURCE}/crates/hotsheet-ticketing/src/settings.rs"
    }
    coreLib = container "Ticketing core libraries" "Linked Rust crates for the ticket model, Git store and operations, provider contracts and adapters, sync and merge, the index, and shared utilities. Linked into the server, CLI and MCP processes; not a process of its own." "Rust crates" "Library" {
        url "${SOURCE}/crates/hotsheet-ticketing/src/lib.rs"
    }
