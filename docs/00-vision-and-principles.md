# 00. Vision & Principles

## 0.1 What Hot Sheet is

Hot Sheet is a **fast, local, ticket-driven worklist** for software (and other)
projects. You capture work as tickets in a bullet-list interface, categorize and
prioritize them, mark the important ones **Up Next**, and then hand that
worklist to AI coding tools that pick the work up and drive it to completion —
with the human watching progress, answering permission prompts, and reviewing.

Hot Sheet 2 keeps that product intact. It rebuilds the foundation so the app is
**simpler, more maintainable, more portable, and multi-tool + multi-client by
construction** instead of by accretion.

## 0.2 Goals of the rewrite

1. **Storage you can read, diff, and version.** Tickets are plain files in git
   repos. No opaque database as the source of truth.
2. **A clean client/service split.** The service (indexing, storage, AI-tool
   orchestration) runs independently. Clients are interchangeable consumers of a
   stable API.
3. **One core, many surfaces.** A single engine powers the server, the CLI, and
   every client (Tauri, SwiftUI macOS/iOS, later Android) — no logic re-implemented
   per surface.
4. **AI-tool-agnostic from day one.** No first-class tool. Every integration is a
   plugin against one interface.
5. **Distribution-ready IDs and coordination.** Random IDs and a claim/lease
   coordination primitive that work offline and across machines.
6. **Efficient at scale.** Text search and list rendering never require reading
   every file from disk in real time — a fast index sits between disk and UI.

## 0.3 Non-goals (first pass)

- **Not a cloud SaaS.** Local-first. Remote access is peer-to-peer between a
  user's own devices/servers, secured by mTLS (carried over from HS1's shipped
  design). No central Hot Sheet cloud.
- **Not a feature-for-feature port on day one.** HS1 has ~188 requirement docs.
  We port the *architecture-defining* capabilities first (storage, index, server,
  CLI, one client, one AI tool via the plugin path, migration) and layer the long
  tail (telemetry, announcer, dashboards, custom views, etc.) afterward, each as
  its own ticket.
- **Not a hierarchical task tree.** HS1 tried parent/child sub-tasks and reverted
  them. Dependencies stay a **flat `blocked_by`** gate.
- **Not multi-user-with-accounts.** Single user per device; "sharing" is git
  sharing (a store pushed to GitHub with GitHub's own ACLs), not an in-app auth
  system.

## 0.4 Principles

- **Git is the source of truth. The index is a cache.** Anything in the index can
  be thrown away and rebuilt from the ticket files. If they disagree, the files
  win. This is the single most important invariant in the system.
- **Conflict resolution is almost entirely automatic.** A git-backed store that
  spills `<<<<<<<` markers onto users is a failed design. File-per-ticket, a
  single-writer claim/lease rule, and a format-aware semantic merge driver
  (frontmatter merged field-by-field, notes unioned by timestamp-ordered UUID)
  keep merges automatic; only two humans rewriting the same prose paragraph of the
  same ticket can still surface a conflict. See
  [02-ticket-storage.md](02-ticket-storage.md) §2.7.
- **The service is authoritative; clients are views — always.** The server is a
  **separate, always-on process even for local use**; no client embeds the engine.
  A client that finds no server auto-starts one (detached) and connects, and the
  server **outlives the client** — close the app and in-flight AI work, terminals,
  and the watcher keep running. A client never holds state the service can't
  reconstruct; two clients on the same project see the same thing.
- **Plugins declare; the host does the work.** An AI-tool plugin says *what* is
  specific to its tool; shared machinery (PTYs, MCP config writers, permission
  bridge, hooks files) lives in the host so two plugins never re-implement the
  same logic. (This is the hard-won lesson of HS1 docs/132 — start there instead
  of arriving there.)
- **No tool is privileged.** Claude, Codex, Gemini, OpenCode, Antigravity,
  Goose, and editor tools are all just plugins. The interface must fit the tool it
  was *not* designed around, or it isn't an interface.
- **Fail closed on trust boundaries.** Local (loopback) is trusted. Anything
  exposed off-box requires mTLS + per-device certs and refuses to serve
  plaintext.
- **Prefer boring, inspectable formats.** Markdown + YAML frontmatter for
  tickets. SQLite for the index. JSON for config. A user with a text editor and
  `git log` can understand and repair the whole system.
- **Everything reconstructs.** Index corrupt? Rebuild from git. Server down?
  Edit tickets with the CLI or a text editor and commit. Client offline? It shows
  last-known state and reconnects.

## 0.5 The end-to-end story (what "done" looks like for v1)

1. `hotsheet init` in a project creates a default **local** ticket store (a git
   repo, or a `.hotsheet/` store inside the project's existing repo) and registers
   the project.
2. The user opens the desktop app; it finds no server running and **auto-starts one
   in the background**, then connects to it. (The server will keep running even
   after the app is closed.)
3. The user (or an AI tool) creates tickets — files land in the store, the index
   updates, the UI redraws instantly.
4. The user marks a few **Up Next**, opens an embedded terminal, and launches an
   AI tool (via its plugin). The tool receives the worklist over its channel,
   claims tickets, does the work, and reports back — all watched live in the UI.
5. Permission prompts from the tool surface in the UI; the user approves/denies.
6. The user closes the app; **the server keeps running**, so the AI tool's work and
   terminals continue. Reopening the app (or another client) re-attaches to it.
7. Finished tickets are committed to the store's git repo like any other change;
   a store backed by a GitHub remote can be pushed and shared with the right ACLs.
8. The same project is reachable from a second device (Mac, iPhone) either by
   pointing that device's client at the running server over mTLS, or by cloning
   the store repo and running a local server there.

## 0.6 Cross-references

- Architecture: [01-architecture.md](01-architecture.md)
- The storage bet: [02-ticket-storage.md](02-ticket-storage.md)
- Technology decisions and their rationale: [09-technology-decisions.md](09-technology-decisions.md)
