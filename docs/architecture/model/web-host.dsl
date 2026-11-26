// Node web host and project bridge, and the Tauri desktop shell.
// Owned by HS2-T6SD67. Included inside the hs2 software system block by system.dsl.
// Keep element identifiers stable (README "Model conventions").

    desktop = container "Tauri desktop shell" "Owns the native window and desktop lifecycle. Starts the bundled Node host and presents the workspace in a WebView." "Tauri / Rust" {
        url "${SOURCE}/clients/web/src-tauri/src/lib.rs"
    }
    bridge = container "Web host and project bridge" "Serves the web UI, opens local projects, manages host routes, forwards checkout-scoped HTTP and streams, and brokers local and remote session access." "Node.js / Hono" {
        url "${SOURCE}/clients/web/src/local-host.ts"
    }
