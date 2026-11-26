# Hot Sheet 2 at the baseline

The workspace describes source revision
`8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`. Start with **HS2-Context** for
users and providers and **HS2-WorkspaceRuntime** for the main process and storage
boundaries.

A code checkout links one or more ticket sources. A source's provider retains
its authoritative records. A local Git source stores ticket bodies, notes and
attachment metadata in Markdown and payload files. SQLite is a per-store search
projection. Checkout IDs address project context. A connection ID and native
ID together address a ticket.

The web application sends checkout operations through a Node bridge to the
long-lived Rust server. The installed Tauri shell starts that bridge and loads
the web application. The CLI and the MCP shim also have direct Git-store paths
using the same Rust domain crates. The server owns live watcher and replay
state, schedules, notifications, and running coordination. The Unix PTY broker
can outlive the server, while the Windows server hosts its own PTYs.

Orchestration separates the reviewed plan from runtime observations. The
orchestrator chooses eligible work and a site. The target host admits and
executes it under its capabilities, authority, and capacity. The target's
provider or Git store still owns ticket state. CI and validation observations
are evidence for gates, and local machine records retain execution state.

The repository coverage matrix records the code and contracts behind each
view. Requirements retain future designs and policy rationale.
