// Browser web client: Kerf workspace, features, state, change streams, drafts, terminal frontend.
// Owned by HS2-AYKHVV. Included inside the hs2 software system block by system.dsl.
// Keep element identifiers stable (README "Model conventions").

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
