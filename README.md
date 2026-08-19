# Hot Sheet 2

A from-scratch rewrite of [Hot Sheet](https://github.com/brianwestphal/hotsheet) —
a fast, local, ticket-driven worklist that drives AI coding tools.

> **Status: design phase.** There is no implementation code yet. This repository
> currently holds the **design** for Hot Sheet 2.

## The core bets

- **Tickets live in git repos as plain files** — one Markdown+YAML file per ticket,
  diffable, selectively shareable, versioned; merges are almost entirely automatic
  via a semantic merge driver.
- **A shared Rust core** used by two thin binaries — the **server** and the **CLI**.
- **The server is always a separate, always-on process** (local included); every
  client is a pure API consumer that auto-starts the server and outlives it.
- **SQLite + FTS5 as a rebuildable index** — never the source of truth.
- **AI-tool integration is entirely plugin-based** — no first-class tool.
- **Clients:** Tauri + web first, then native SwiftUI (macOS → iOS), then Android.

## Where to start

Read [`docs/README.md`](docs/README.md) — the design index, the core bets, the
requirements summary, and the technology-decision log
([`docs/09-technology-decisions.md`](docs/09-technology-decisions.md)).

The implementation roadmap and open decisions are tracked as tickets in the
project's own Hot Sheet.
