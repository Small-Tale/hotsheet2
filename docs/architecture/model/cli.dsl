// Hot Sheet CLI, MCP shim, and Hot Sheet 1 migrator.
// Owned by HS2-TW2DKM. Included inside the hs2 software system block by system.dsl.
// Keep element identifiers stable (README "Model conventions").

    cli = container "Hot Sheet CLI" "Runs headless ticket operations and operational commands. Can bootstrap or control a server and delegate remote operations where applicable." "Rust / Clap" {
        url "${SOURCE}/crates/hotsheet-cli/src/main.rs"
    }
    mcp = container "Hot Sheet MCP shim" "Presents stdio JSON-RPC tools to an AI process. Routes to the matching server or opens a Git store directly in serverless mode." "Rust / MCP" {
        url "${SOURCE}/crates/hotsheet-mcp/src/main.rs"
    }
    migrator = container "Hot Sheet 1 migrator" "Runs the legacy export in a separate Node process and imports it into an HS2 Git store." "Rust / Node.js / PGLite" {
        url "${SOURCE}/crates/hotsheet-cli/src/bin/hotsheet-migrate.rs"
    }
