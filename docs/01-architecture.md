# 01. Architecture Overview

## 1.1 The shape

Hot Sheet 2 is a local-first ticketing and AI work coordination system.
The [Structurizr baseline](architecture/README.md) maps the audited runtime,
storage and trust boundaries. Its [coverage matrix](architecture/coverage/overview.md)
links each boundary to implementation and test evidence.

The [earlier overview diagram](diagrams/architecture.svg) and its
[HTML source](diagrams/src/architecture.html) document the initial design.
Refer to the baseline for current process and storage boundaries.

## 1.2 Components

### Shared Rust libraries

The CLI, server and serverless MCP paths share domain code. `hotsheet-model`
owns the Git ticket format. `hotsheet-ticketing` owns Git ticket operations,
provider contracts, checkout links, settings, ticket coordination and the
orchestration domain. Separate crates implement the local search index,
native provider adapters, AI-tool setup and drive, terminals, jobs, resource
leases, notifications and shared infrastructure. The
[crate ownership table](architecture/coverage/overview.md#cargo-workspace-ownership)
maps the complete workspace. [04-core-server-cli.md](04-core-server-cli.md)

### hotsheet-server (binary)

The long-lived Axum/Tokio server owns the checkout-scoped HTTP API, live
WebSockets, long polling, file watching, index reconciliation, terminal
services, notifications, scheduled AI commands, and orchestration runtime.
On Unix, the detached terminal broker usually owns the PTYs. On Windows,
the server hosts PTYs in process. [04-core-server-cli.md](04-core-server-cli.md)

### hotsheet-cli (binary) and MCP shim

The CLI supports direct Git-store operations while the server is stopped. It
also exposes bootstrap, provider and host operations. The MCP shim presents
stdio tools to AI clients. It can use project-scoped or explicit server HTTP
routes, or open a local Git store directly in serverless mode. The separate
legacy migrator invokes the Node PGLite exporter for HS1 import.
[04-core-server-cli.md](04-core-server-cli.md) · [07-migration.md](07-migration.md)

### Clients

The Kerf web workspace runs in a browser or the Tauri WebView. The packaged
Tauri shell starts the Node web host and project bridge, which can discover
or start the standalone Rust server. The workspace uses HTTP and WebSocket
routes through that bridge. Closing the desktop shell leaves the independently
launched server running. The macOS screen saver consumes a credential-free
snapshot file. SwiftUI ticket clients and Android are tracked as client designs.
[06-clients.md](06-clients.md) · [tauri-desktop.md](tauri-desktop.md)

## 1.3 Data flow

- **Write a ticket.** The checkout and source connection select a provider. For
  Git the engine persists Markdown and attachments in the linked store and
  the server updates its local projection. For GitHub Issues, GitLab Issues,
  or Jira the native issue remains authoritative.
- **Reconcile a store edit.** The local watcher invalidates affected Git-store
  paths, reconciles indexed rows, and publishes scoped events. CLI writes can
  take this path because direct operations do not require the server.
- **Read a ticket list.** The server serves local indexed pages and queries
  external providers according to their capabilities. It merges checkout
  pages and reports per-source coverage and failures.
- **Keep a workspace current.** WebSockets deliver live changes. Long polling
  can replay from a cursor. A dropped replay window or replaced host can require
  rebuilding the affected client projection.

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

- [Architecture model and coverage](architecture/README.md)
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
