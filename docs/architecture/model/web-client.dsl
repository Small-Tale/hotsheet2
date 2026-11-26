// Browser web client: Kerf workspace, features, state, change streams, drafts, terminal frontend.
// Owned by HS2-AYKHVV. Included inside the hs2 software system block by system.dsl.
// Keep element identifiers stable (README "Model conventions").
// Module-to-component assignment is in coverage/web-client.md. Node-side and shared modules of
// clients/web/src belong to the web host area (HS2-T6SD67).

    web = container "Hot Sheet workspace" "Renders tickets, settings, workspaces, orchestration, and terminals. Projects share the browser or desktop web UI." "TypeScript, Kerf, xterm.js" {
        url "${SOURCE}/clients/web/src/main.tsx"

        webShell = component "Entry, runtime composition and interaction wiring" "Boots the page or the /ux-demo catalog, composes every feature controller inside one runtime closure, mounts the Kerf shell, and registers delegated interactions, keyboard shortcuts and Kerf UI wirings. Holds no server state of its own." "Kerf" {
            url "${SOURCE}/clients/web/src/app/runtime.tsx"
        }
        webWorkspace = component "Workspace and project lifecycle" "Owns the open-project list, the selected project, project tabs and sidebar, project open, startup, minimization, close and teardown, and the unavailable-project shell." "Kerf signals" {
            url "${SOURCE}/clients/web/src/features/project-lifecycle.ts"
        }
        webApi = component "API client" "Builds checkout-scoped requests, tracks busy and in-flight requests, and decodes results, errors and source warnings. Does not cache; callers own the state." "TypeScript / fetch" {
            url "${SOURCE}/clients/web/src/api.ts"
        }
        webChanges = component "Change streams and refresh" "Follows each open project's change cursor over WebSocket or long poll, on the main thread by default or in an opt-in SharedWorker or dedicated Worker. Detects read gaps and overflow and turns events into coalesced, barrier-ordered refreshes." "WebSocket / HTTP long poll / optional SharedWorker or Web Worker" {
            url "${SOURCE}/clients/web/src/features/project-change-streams.ts"
        }
        webTickets = component "Tickets, board, search and reader" "Ticket list and board views, ticket groups, saved views, search models, the inspector and stacked readers, notes, blockers, bulk operations, and optimistic ticket mutations with rollback." "Kerf components" {
            url "${SOURCE}/clients/web/src/features/ticket-workflows.ts"
        }
        webDrafts = component "Drafts and edit coalescing" "Keeps typed text in controlled drafts and local recovery copies, writes once on blur, page hide or explicit finish (HS2-RE1PS6), merges against the edit-start value, and restores unsaved drafts after reload." "Kerf signals / localStorage / IndexedDB" {
            url "${SOURCE}/clients/web/src/debounced-autosave.ts"
        }
        webPersistence = component "Browser persistence" "Reads and writes workspace preferences, per-project sessions and the theme in localStorage, mirrors hotsheet.* keys to the desktop storage endpoint in the release shell, and keeps in-memory warm-project and ticket detail caches." "localStorage / loopback HTTP" {
            url "${SOURCE}/clients/web/src/desktop-storage.ts"
        }
        webTerminal = component "Terminal frontend" "Renders xterm output and controls terminal attach, replay, resize, input, search, clipboard, visibility, the drawer and the dashboard grid." "xterm.js" {
            url "${SOURCE}/clients/web/src/terminal-replay-stream.ts"
        }
        webAttachments = component "Attachments and gallery" "Uploads, renames, labels and groups ticket attachments, and runs the gallery with crop and annotation editing." "Kerf components" {
            url "${SOURCE}/clients/web/src/features/gallery.tsx"
        }
        webAi = component "AI conversation, drive and permissions" "Renders AI conversations and composers, launches and drives AI sessions, and presents permission requests and rules, feedback prompts and halted sessions." "Kerf components" {
            url "${SOURCE}/clients/web/src/features/drive-conversations.ts"
        }
        webOrchestration = component "Orchestration UI" "Orchestration view, plan, timeline, inspector, settings and policy editing, worker columns, claim clocks and provisional-work presentation." "Kerf components" {
            url "${SOURCE}/clients/web/src/features/orchestration-view.ts"
        }
        webCommands = component "Commands and schedules" "Custom command definitions, groups, ordering, press-and-hold runs, and command schedule editing." "Kerf components" {
            url "${SOURCE}/clients/web/src/features/commands.tsx"
        }
        webNotifications = component "Notifications, toasts and announcer" "Server-backed attention inbox, system notifications, notification rules and pause, toast presentation, the away digest, and the announcer overlay and speech." "Kerf components / Web Notifications / Tauri notification plugin" {
            url "${SOURCE}/clients/web/src/client-notifications.ts"
        }
        webRemote = component "Remote access and devices" "Remote-browser sign-in screen, browser grants, paired-device and pairing-request management, remote project placeholders and reconnect, and GitHub sign-in settings." "Kerf components / HTTP long poll" {
            url "${SOURCE}/clients/web/src/features/remote-devices.ts"
        }
        webHostOps = component "Host, repository and maintenance surfaces" "Host health, CPU and disk feeds, stats dashboards, repository status and setup, legacy migration progress, guidance settings, trash, update notices, desktop close handling, and the standby screen and saver bundle." "Kerf components" {
            url "${SOURCE}/clients/web/src/features/standby.tsx"
        }
        webUxDemo = component "UX demo catalog" "The /ux-demo catalog: production components rendered with fixtures in a Kerf Catalog. Loaded as its own chunk only when the host enables the ux-demo feature." "Kerf Catalog" {
            url "${SOURCE}/clients/web/src/ux-demo/main.tsx"
        }
    }
