# Hot Sheet 2

**A [Small Tale Inc.](https://github.com/Small-Tale) project.** A fast, local,
ticket-driven worklist that drives AI coding tools — a from-scratch rewrite of the
original [Hot Sheet](https://github.com/brianwestphal/hotsheet).

> **Status: early implementation.** The Rust core, CLI, server, git ticket provider,
> indexing, automation, and integration foundations exist; client work is beginning.

## The core bets

- **Ticketing is pluggable** — the default provider uses Markdown+YAML files in git;
  teams can instead connect existing GitHub Issues/Jira/etc. directly, and one project
  can use multiple providers without continuous mirroring.
- **A shared Rust core** used by two thin binaries — the **server** and the **CLI**.
- **The server is always a separate, always-on process** (local included); every
  client is a pure API consumer that auto-starts the server and outlives it.
- **SQLite + FTS5 as a rebuildable index** — never the source of truth; the selected
  ticket provider is authoritative.
- **AI-tool integration is entirely plugin-based** — no first-class tool.
- **Clients:** Tauri + web first, then native SwiftUI (macOS → iOS), then Android.

## Where to start

Read [`docs/README.md`](docs/README.md) — the design index, the core bets, the
requirements summary, and the technology-decision log
([`docs/09-technology-decisions.md`](docs/09-technology-decisions.md)).

The implementation roadmap and open decisions are tracked as tickets in the
project's own Hot Sheet.

## License

[MIT](LICENSE) © Small Tale Inc.

---

Hot Sheet 2 is developed by **Small Tale Inc.** Copyright © 2026 Small Tale Inc.
