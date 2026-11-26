# 01. Architecture Overview

This page is the entry point to the architecture. It describes the system in
a few paragraphs and links to the detailed, source-cited material. The
[Structurizr baseline](architecture/README.md) is the current map: every
claim there cites code at one audited revision and names the test that shows
it. Where this page and the baseline disagree, the baseline and the code win.

## 1.1 The shape

Hot Sheet 2 is a local-first ticketing and AI work coordination system. A code
checkout links one or more ticket sources. Each source's provider keeps its
own authoritative records: Markdown files in a Git ticket store, or native
issues in GitHub, GitLab or Jira. A per-store SQLite index is a rebuildable
projection, never a source of truth. A checkout id is a readable path
identity; a ticket is addressed by its connection id and native id.

The runtime is a set of cooperating local processes, not one service:

| Process or store                    | What it owns                                                                                                                      | Chapter                                                                                                |
| ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| Browser workspace (`web`)           | Rendering, drafts and browser storage; talks only to the Node bridge                                                              | [Web client](architecture/narrative/60-web-client.md)                                                  |
| Node web host and bridge (`bridge`) | Every `/__hotsheet` route, the request proxy, WebSocket bridges, lazy server supervision, remote projects and migration jobs      | [Web host](architecture/narrative/50-web-host.md)                                                      |
| Tauri desktop shell (`desktop`)     | The native window; starts and stops the bundled Node bridge                                                                       | [Web host](architecture/narrative/50-web-host.md)                                                      |
| Rust server (`server`)              | One detached process per machine: checkout API, authorization, watch, index updates, change replay, AI drive, orchestration loops | [Server](architecture/narrative/40-server.md)                                                          |
| Terminal broker and launch relay    | Unix PTYs that outlive the server; the optional macOS launch relay                                                                | [Server](architecture/narrative/40-server.md)                                                          |
| CLI, MCP shim and migrator          | Headless commands against the store or a host, the detached Job monitor, stdio tools for AI sessions, and Hot Sheet 1 import      | [CLI and MCP](architecture/narrative/30-cli.md)                                                        |
| Linked Rust libraries               | The ticket model, Git store, operations, providers, sync, merge and index; orchestration, claims, resource leases and Jobs        | [Core](architecture/narrative/10-core.md), [Orchestration](architecture/narrative/20-orchestration.md) |
| Data stores                         | Ticket files; the SQLite projection; checked-in settings and plans; machine-local registries, credentials and journals            | [Core](architecture/narrative/10-core.md)                                                              |

How the processes cooperate end to end, including failure and recovery, is
in [End-to-end flows](architecture/narrative/70-flows.md).

### Superseded diagrams

The [earlier overview diagram](diagrams/architecture.svg) and its
[HTML source](diagrams/src/architecture.html) record the initial design and
are kept for history only. They are superseded as follows:

| Diagram element                                             | Superseded by                                                             | What changed                                                                                                 |
| ----------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| The single `hotsheet-core` crate                            | `HS2-Core-TicketEngine`, `HS2-Core-Providers`, `HS2-Orch-Placement`       | The domain is several crates, modeled as the `coreLib` and `orchLib` library containers                      |
| `hotsheet-server` exposing HTTP, WebSocket and MCP          | `HS2-WorkspaceRuntime`, `HS2-Cli-McpComponents`                           | MCP is a separate stdio process (`hotsheet-mcp`); the server exposes HTTP and WebSocket only                 |
| Tauri desktop "auto-starts and supervises the local server" | `HS2-WebHost-DesktopColdStart`, `HS2-Flow-DesktopColdStart`               | The Node bridge starts the server lazily on first use; nothing supervises the bridge itself                  |
| Clients and AI tools around the server                      | `HS2-Context`, `HS2-WorkspaceRuntime`                                     | The browser reaches the server only through the Node bridge; AI tools use MCP, hooks and the CLI             |
| Local versus remote server                                  | `HS2-DesktopDeployment`, `HS2-HeadlessDeployment`, `HS2-RemoteDeployment` | Remote access is a paired browser session or a paired peer server, both under the target's own authorization |
| SwiftUI and Android clients                                 | none                                                                      | Design-only; [Clients](06-clients.md) tracks them                                                            |

No other architecture diagram exists under `docs/`; the files in
`docs/design/exports/` are interface wireframes.

## 1.2 Components

### Shared Rust libraries

The CLI, server and serverless MCP paths share domain code, so a direct CLI
write and a server write follow one path. `hotsheet-model` owns the Git ticket
format. `hotsheet-ticketing` owns Git ticket operations, provider contracts,
checkout links, settings, ticket coordination and the orchestration domain.
Separate crates implement the local search index, native provider adapters,
AI-tool setup and drive, terminals, Jobs, resource leases, notifications and
shared infrastructure. The `hotsheet-cli` crate is also a library: the server
and the MCP shim link its coordination pool, orchestration runtime and remote
server transport. The
[crate ownership table](architecture/coverage/overview.md#cargo-workspace-ownership)
maps the complete workspace. Detail: [core](architecture/narrative/10-core.md),
[orchestration](architecture/narrative/20-orchestration.md),
[04-core-server-cli.md](04-core-server-cli.md).

### hotsheet-server (binary)

The long-lived Axum/Tokio server owns the checkout-scoped HTTP API, live
WebSockets, long polling, file watching, index updates, Git sync, terminal
services, the AI permission bridge, notifications, scheduled AI commands and
the orchestration runtime. One process serves every hosted store on the
machine. It does not supervise itself; a crash leaves stale instance files
that the next start reclaims. On Unix, the detached terminal broker owns the
PTYs and survives a server restart. On Windows, the server hosts PTYs in
process. Detail: [server](architecture/narrative/40-server.md),
[04-core-server-cli.md](04-core-server-cli.md).

### hotsheet-cli (binary) and MCP shim

The CLI performs direct Git-store operations with or without a server, and
asks a running host to sync after a write. It also exposes bootstrap,
provider, host, coordination and orchestration operations, and runs the
detached Job monitor that observes CI. The MCP shim presents stdio tools to
an AI process. It routes to a project-scoped or explicit server, attaches to
the server that launched its session, or opens a local Git store directly in
serverless mode. The separate legacy migrator runs the Node PGLite exporter
for Hot Sheet 1 import. Detail: [CLI and MCP](architecture/narrative/30-cli.md),
[04-core-server-cli.md](04-core-server-cli.md) · [07-migration.md](07-migration.md).

### Clients

The Kerf web workspace runs in a browser, in a paired remote browser, or in
the Tauri WebView. It holds no server secret: the Node web host and project
bridge serves the page, owns every `/__hotsheet` route, injects credentials
and discovers or starts the standalone Rust server when a request first needs
it. The packaged Tauri shell starts that bridge and stops it on exit; closing
the shell leaves the server and its terminals running. The macOS screen saver
reads a credential-free snapshot file. SwiftUI ticket clients and Android are
tracked as client designs. Detail: [web client](architecture/narrative/60-web-client.md),
[web host](architecture/narrative/50-web-host.md),
[06-clients.md](06-clients.md) · [tauri-desktop.md](tauri-desktop.md).

## 1.3 Data flow

Each flow below is narrated hop by hop, with failure and recovery, in
[End-to-end flows](architecture/narrative/70-flows.md).

- **Write a ticket.** The browser keeps typing in local drafts and saves once
  on blur or page hide. The bridge forwards the save with its credential; the
  server authorizes it, selects the provider by checkout and connection, and
  for Git writes the Markdown file, updates the index and publishes a scoped
  change in the same request. For GitHub Issues, GitLab Issues or Jira the
  native issue remains authoritative. Views: `HS2-Flow-TicketEdit`,
  `HS2-Flow-NativeProviderWrite`.
- **Pick up a store edit.** The server's watcher re-indexes ticket files
  changed outside it (a `git pull`, a sync pass, or a direct CLI write) and
  publishes scoped events. View: `HS2-Flow-OutOfBandEdit`.
- **Read a ticket list.** The server serves local indexed pages and queries
  external providers according to their capabilities. It merges checkout
  pages and reports per-source coverage and failures.
- **Keep a workspace current.** WebSockets push live changes. Long polling
  replays from a cursor. A dropped replay window forces a full re-read of the
  affected client projection. View: `HS2-Flow-ReplayOverflow`.

See the [baseline views](architecture/README.md#where-to-start) for ordered
examples, and the [transport contracts](contracts/README.md) for wire detail.

## 1.4 Process & trust model

- The Rust server is a separate host process. A managed local launch discovers
  or starts it through instance lifecycle operations.
- The local bearer belongs to instance data. Loopback trust uses the socket
  peer. An off-loopback listener defaults to mTLS, with explicit direct HTTP
  and server TLS options using paired sessions where configured.
- Paired browser/device grants govern remote browser access. Host approval is
  required for pairing. The web bridge forwards the remote session authority
  and keeps the local bearer scoped to its own instance route. Device grants
  apply to read/write REST and live stream access.
- Orchestration peers are user-managed hosts. The plan owner coordinates work
  and the target host admits operations under its own authority and capacity.
- Ticket claims, machine resource leases, and execution records represent
  different kinds of ownership. A scheduled auto-start claims the exact
  persisted occurrence ticket before handing work to the AI tool.

Views: `HS2-Flow-RemoteDevice`, `HS2-Flow-RemoteProject`,
`HS2-Flow-ClaimLifecycle`, `HS2-OrchestrationPlacement`.
[Distributed and remote](08-distributed-and-remote.md) ·
[Orchestration](26-orchestration-projects.md) ·
[Resource claims](36-resource-claims.md)

## 1.5 What is deliberately NOT a component

The Git ticket format is versioned Markdown and attachment files. SQLite is a
rebuildable query projection. One writer per store working copy owns index
maintenance. Git arbitrates concurrent content changes across working copies.
The Node host serves a client-side UI, while
the Rust API serves data and streams. Tool manifests and adapters express AI
provider capabilities behind the shared launch and drive interfaces.

## 1.6 Cross-references

- [Architecture baseline: README, depth standard and where to start](architecture/README.md)
- Area chapters: [boundaries](architecture/narrative/01-boundaries.md),
  [core](architecture/narrative/10-core.md),
  [orchestration](architecture/narrative/20-orchestration.md),
  [CLI and MCP](architecture/narrative/30-cli.md),
  [server](architecture/narrative/40-server.md),
  [web host](architecture/narrative/50-web-host.md),
  [web client](architecture/narrative/60-web-client.md),
  [end-to-end flows](architecture/narrative/70-flows.md)
- Coverage files: [overview](architecture/coverage/overview.md),
  [core](architecture/coverage/core.md),
  [orchestration](architecture/coverage/orchestration.md),
  [CLI](architecture/coverage/cli.md),
  [server](architecture/coverage/server.md),
  [web host](architecture/coverage/web-host.md),
  [web client](architecture/coverage/web-client.md)
- [Unresolved claims register](architecture/unresolved.md) and the
  [flows chapter's unresolved list](architecture/narrative/70-flows.md#unresolved)
- Key views: `HS2-Context`, `HS2-WorkspaceRuntime`, `HS2-ProviderBoundaries`,
  `HS2-DesktopDeployment`, `HS2-HeadlessDeployment`, `HS2-RemoteDeployment`,
  and the `HS2-Flow-*` dynamic views
- [Codebase map](CODEBASE-MAP.md)
- [Technology decisions](09-technology-decisions.md)
- [Ticket storage](02-ticket-storage.md)
- [Indexing and queries](03-indexing-and-query.md)

## 1.7 Deployment boundary (HS2-51HMCC)

Hot Sheet 2 currently has no Small Tale-hosted service, beta or production
environment, infrastructure definitions, state backend, or deployed URL. The
server runs as a detached process on a user's machine (§1.4); remote clients
attach through the web bridge to a user-managed host (§1.2). The
[baseline](architecture/README.md) records desktop, headless, and remote peer
deployment modes. The web client is bundled with the Tauri desktop app, and
`clients/web` has build and local-server commands.
Those are build and local runtime paths, not an infrastructure deployment.

The shared deployment-scripts rule's environment argument, SHA-tagged deployable
image, credential and state preflight, phased infrastructure apply, secret
versions, and deployed-URL smoke test have no target in this repository today.
Creating a `deploy.sh` for a nonexistent environment would imply a deployment
that cannot run. The project-level deviation is recorded in
`.rulesync/rules/project-local.md`. HS2-VXQNVM decides whether a hosted service
will exist and, if so, its environments and infrastructure contract. Artifact
distribution for the server, CLI, and desktop clients is a separate decision in
HS2-BYXA4V. Once a hosted target is selected, its infrastructure directory must
contain the single checked-in deployment entry point and a repeatable smoke test.
