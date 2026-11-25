# 1. Separate the workspace, host runtime, and ticket storage

Date: 2026-11-25

## Status

Accepted, recorded retrospectively from the architecture and core/server/CLI
requirements and the audited source revision.

## Context

Hot Sheet 2 needs interactive local and remote clients, a persistent terminal
and watch runtime, and headless workflows that can operate directly on a local
Git ticket store.

## Decision

Keep the web workspace on a client transport boundary. Run the Rust server as
a separate process and keep the direct Git operations in shared library code
used by the server, CLI, and serverless MCP paths. Keep authoritative ticket
records in their selected sources and treat the search index as derived.

## Consequences

The same ticket-store format and operations are available with the server
stopped. Real-time reconciliation, client subscriptions, hosted terminal
sessions, and orchestration need the appropriate running host. A native
ticket provider remains responsible for its own record and capabilities.
