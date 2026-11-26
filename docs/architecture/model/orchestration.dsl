// Orchestration, coordination, resource claims and jobs domain libraries.
// Owned by HS2-RJB111. Included inside the hs2 software system block by system.dsl.
// Keep element identifiers stable (README "Model conventions").

    orchLib = container "Orchestration and coordination libraries" "Linked Rust code for orchestration declarations, placement and runs, ticket and advisory claims, speculation, worker plans, resource claims and job observation. Linked into the server and CLI; not a process of its own." "Rust crates" "Library" {
        url "${SOURCE}/crates/hotsheet-ticketing/src/orchestration"
    }
