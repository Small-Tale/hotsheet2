# 09. Technology Decisions (ADR log)

Consolidated core technology decisions for Hot Sheet 2, each with rationale,
alternatives, and status. This is the doc to read to understand *why* the stack is
what it is.

> Status legend: **Decided** · **Proposed** (recommendation, wants maintainer
> confirmation) · **Open**.

---

## 9.1 Storage: git repos, file-per-ticket — **Proposed → strong recommendation**

**Decision.** Tickets are Markdown-with-YAML-frontmatter files in git
repositories; git is the source of truth. One file per ticket; a project
references one or more stores; stores carry their own visibility + sync policy.

**Why.** Inspectable, diffable, selectively shareable with real (GitHub) ACLs,
versioned with the project, distribution-native, and it deletes an entire class of
HS1 fragility (the embedded-Postgres cluster needed a `template1` pin, tiered
snapshots, backups, a repair subsystem, and cluster eviction just to stay alive).

**Alternatives.** Keep PGLite (rejected — the fragility the rewrite exists to
remove); a single SQLite file as source of truth (rejected — not diffable/mergeable
per-ticket, not selectively shareable, reintroduces an opaque blob); a bespoke
binary format (rejected — not inspectable). Detail: [02](02-ticket-storage.md).

---

## 9.1a Automatic conflict resolution: **semantic merge driver** — **Decided** (maintainer, 2026-08-19)

**Decision.** Conflict resolution is a first-class goal: it must be *almost
entirely automatic*. A per-store git **merge driver** (`hotsheet merge-driver`,
registered via `.gitattributes` on `tickets/**/*.md`) performs a **format-aware
3-way merge** instead of git's line merge — frontmatter merged field-by-field
(last-writer-wins by `updated_at`; sets unioned), notes unioned by their
timestamp-ordered UUID, and only same-paragraph body edits able to surface a
human-visible conflict. File-per-ticket + the claim/lease single-writer rule remove
most conflicts before the driver even runs.

**Why.** A git-backed ticket store that spills `<<<<<<<` markers onto users would
be a failed design. The maintainer made "almost entirely automatic" an explicit
requirement. Timestamp-ordered UUIDs on notes (§9.4) exist specifically to make
note merges a clean union. Detail: [02](02-ticket-storage.md) §2.7.

## 9.1b Automatic repo syncing: **background sync engine** — **Decided** (maintainer, 2026-08-19)

**Decision.** For every `git-remote` store, Hot Sheet **aggressively** fetches,
auto-integrates (rebase/merge via the §9.1a driver), auto-commits local edits, and
auto-pushes — offline-tolerant, backing off on failure. Users *can* run git by hand
but should almost never need to. Detail: [02](02-ticket-storage.md) §2.12; build:
HS2-19.

**Why.** Maintainer requirement. Paired with automatic conflict resolution (§9.1a),
it makes a shared git ticket store feel like a live database without the user
managing git.

## 9.1c Shared vs. local data: **on-disk gitignored overlay, index is a cache** — **Decided** (maintainer, 2026-08-19)

**Decision.** Ticket data is tiered: **shared** fields committed in the ticket file;
**per-user/per-machine** data (read tracking, feedback drafts, UI state, device
prefs) stored **on disk in gitignored overlay files** in the store, never
committed; and **local-only stores** for wholly-private tickets. The SQLite index
caches both but is **never the sole home** of any durable data — it must always
rebuild from disk. Detail: [02](02-ticket-storage.md) §2.11; build: HS2-21.

**Why.** Answers the maintainer's question directly and upholds the "everything
reconstructs from disk" principle — local data only in a disposable DB would be lost
on reindex.

## 9.1d Human assignment: **separate from machine claim/lease** — **Proposed** (design; maintainer, 2026-08-19)

**Decision (proposed).** Human assignment (`assignees` + `review_requests` with a
`work`/`feedback`/`review`/`fyi` kind) is a **distinct, durable, shared** concept
from the ephemeral machine `claim/lease`. Identity = git email + an optional roster;
attention delivered in-app + live push + on-sync. Detail:
[10-assignment-and-collaboration.md](10-assignment-and-collaboration.md); build:
HS2-20. Several sub-questions remain open (§10.5).

**Why.** Teams need to direct specific people to do work or give feedback; that is
not what claim/lease (worker coordination) models, and conflating them is the trap.

## 9.1e Server is always a separate process; clients never embed the core — **Decided** (maintainer, 2026-08-19)

**Decision.** `hotsheet-server` runs **independently of any client in every case,
local included.** No client embeds `hotsheet-core`. A client that finds no server
**auto-starts one, detached**, and connects; the server **outlives the client**
(closing the app leaves it running). One local server instance per machine serves
all local projects. Locally it uses loopback + shared secret; **mTLS is optional
even locally** where it adds security (a shared multi-user machine). Detail:
[04](04-core-server-cli.md) §4.3.1, [06](06-clients.md) §6.2; build: **HS2-59**.

**Why.** Makes the client/service split absolute and delivers what the maintainer
wants operationally: in-flight AI work, terminals, and the watcher keep running
when the GUI is closed; any client can attach to the already-running server; the
system has exactly one authority. It reverses HS1's Tauri-owns-a-Node-sidecar model
(server died with the app). The trade — a client must be able to *spawn and
supervise* a detached process (double-fork / `setsid` / a launchd/systemd user
service) — is well-trodden and worth it. **Consequence:** the earlier
embedded-core / `uniffi` client plan is retired, which also reshapes the language
rationale (§9.2) and the client design (§9.5).

## 9.2 Implementation language: **Rust core** — **Decided** (maintainer, 2026-08-19)

**Decision (confirmed).** A **Rust core library** (`hotsheet-core`) with thin Rust
binaries for the server (`axum`/`tokio`) and CLI (`clap`). The core is used by those
**two binaries only**; clients do **not** embed it (§9.1e) — so no `uniffi`/JNI
bindings are needed.

> **Rationale update (2026-08-19).** Earlier drafts made "one core embedded in
> every native client via `uniffi`" the *decisive* Rust-over-Go argument. The
> maintainer's server-always-separate decision (§9.1e) **retires that argument** —
> clients are pure API consumers now. Rust nonetheless stands, on the reasons
> below, and the maintainer has re-confirmed it. This entry is kept honest about
> the shift rather than pretending the original reason still holds.

**Why Rust still wins for the server + CLI:**
- **The Tauri shell is Rust regardless.** It launches/supervises the local server
  and holds the mTLS proxy (HS1 already scaffolded this in Rust). One systems
  language across server, CLI, and the desktop shell.
- **Best-in-class libraries for exactly this surface:** `rusqlite` (index),
  `notify` (watching), `portable-pty` (terminals), `tokio`/`axum` (server),
  `rustls`/`reqwest`/`tokio-tungstenite` (the mTLS remote path HS1 already proved
  in Rust).
- **Correctness + performance** for a long-lived service hosting file-watch,
  indexing, process supervision, and terminals.
- **CLI ↔ server share one tested engine** — the direct-to-disk CLI and the server
  can't drift because they call the same functions.

**The cost, stated honestly.** Rust iterates slower than Go, and this app has a
*lot* of small glue features (the HS1 long tail); Go's simplicity would genuinely
help there. With the client-embedding argument gone, **Go is now a closer call than
before** — a Go server + CLI would be perfectly viable, and clients (already
API-only) wouldn't care. The maintainer nonetheless **re-confirmed Rust**
(2026-08-19), on the server-surface library ecosystem + Tauri-shell-is-Rust
reasons above. Go remains the documented fallback of record.

---

## 9.3 Index: **SQLite + FTS5** — **Proposed**

**Decision.** A machine-local, disposable SQLite database (WAL mode) with FTS5 for
full-text search; both structured queries and text search in one store; rebuilt
from git on demand.

**Why.** Embedded, transactional, one store for both query shapes, great Rust
bindings, cheap cold start with incremental (git-diff-driven) reindex.

**Alternatives.** Tantivy/Bleve (search only — keep as a later add-on if FTS5
relevance disappoints, layered over SQLite); in-memory only (slow cold start, no
paging); DuckDB (analytics overkill); re-embed Postgres (reintroduces the fragility
we're removing). Detail: [03](03-indexing-and-query.md) §3.7.

---

## 9.4 Ticket IDs: **ULID + derived all-caps slug** — **Decided** (maintainer, 2026-08-19)

**Decision.** 128-bit ULIDs as the real key (no central sequence, mintable
offline, k-sortable for a free chronological default) + a short deterministic
**ALL-CAPS** display slug (`HS-7F3K9Q`). Note ids are ULIDs too, for automatic
merge (§9.1a).

**Why.** The ticket explicitly wants "uuid/random based instead of linearly
increasing." ULID beats plain UUIDv4 by being time-sortable (good default order,
even sharding) while staying coordination-free. The slug keeps a human handle.

**Cost.** We lose HS1's friendly monotonic `HS-1234`; migration preserves it as
`legacy_number`. Detail: [02](02-ticket-storage.md) §2.4.

---

## 9.5 Clients: **Tauri + native SwiftUI, all pure API consumers** — **Decided** (maintainer, 2026-08-19)

**Decision.** Every client is a pure API consumer (HTTP/WS/MCP) — **none embeds the
core** (§9.1e). Tauri desktop (Rust shell that *launches/supervises* the local
server + holds the mTLS proxy for remote) + native SwiftUI (macOS/iOS, HTTP/WS,
remote-first on iOS). Android later, same API. No `uniffi`/JNI bindings.

**Sequencing (confirmed):**
1. **Tauri + web** to feature floor first (direct successor to HS1's UI, fastest
   path to a usable app; the web build lands with it).
2. **Native SwiftUI macOS.**
3. **Native SwiftUI iOS** (remote-first — no local server on iOS).
4. **Android (Kotlin/Compose).**

Detail: [06](06-clients.md).

**Sub-decision (client UI framework).** For the Tauri web UI, recommend a small
mainstream reactive framework (Solid/Svelte) over hand-rolling a runtime — HS1's
bespoke `kerfjs`/JSX runtime was a maintenance tax. Client-local, revisitable.

---

## 9.6 Transport & API: **JSON REST + WebSocket + MCP** — **Decided**

**Decision.** The server speaks data, not markup: JSON REST for CRUD/query, a
WebSocket bus for live push (+ long-poll fallback), MCP for AI tools. No
server-rendered HTML (the client/service split).

**Why.** Makes clients interchangeable and the service independent — the chartered
goal. Carries over HS1's WS-sync + long-poll + MCP-proxy patterns onto the new
core. Detail: [04](04-core-server-cli.md) §4.3.

---

## 9.7 AI-tool integration: **plugin-only, host-carries-machinery** — **Decided (design)**

**Decision.** One plugin interface; no first-class tool (Claude included); a
declarative half (client-safe data) + a behavioral half (host-side); shared
machinery in the host; absence-of-capability as the only feature test; a lint that
forbids tool-id branches outside a plugin.

**Why.** This is HS1's own end-state (docs/132), reached only after an eight-phase
epic unwinding scattered `if (tool === …)` branches. HS2 starts there. Detail:
[05](05-ai-tool-plugins.md).

---

## 9.8 Remote security: **tiered, mTLS off-box** — **Decided (carry over)**

**Decision.** Loopback = plaintext + shared secret; any off-box bind = mTLS +
per-device certs + ACLs or refuse to start. Per-project CA, `.p12`/QR enrollment,
revocation. Carried over unchanged from HS1's shipped design. Detail:
[08](08-distributed-and-remote.md) §8.4.

---

## 9.9 Migration: **two-step export(Node)/import(core)** — **Proposed**

**Decision.** A bundled Node exporter reads the PGLite cluster read-only → portable
JSON; `hotsheet migrate` imports JSON → git stores. Idempotent, UI-prompted,
non-destructive. Detail: [07](07-migration.md).

---

## 9.10 Testing & docs conventions — **Decided**

- **Double coverage** (unit + E2E), transition-matrix tests for stateful modules,
  merged coverage — carried over from the project's testing philosophy (see
  `CLAUDE.md`). The test-runner specifics get filled in once the language is
  confirmed and the first code lands (a follow-up ticket).
- **Requirements docs are the source of truth**, kept in sync with code in the same
  change; an AI-readable codebase-map + requirements-summary maintained (this
  `docs/` folder is that home). See [README.md](README.md).

---

## 9.11 Decision status

**Resolved by the maintainer (2026-08-19):**

| # | Decision | Resolution |
|---|---|---|
| L1 | Server/CLI/core language | **Rust core** — §9.2 |
| — | Storage / ID / index direction | Approved as designed (git files, ULID + all-caps slug, SQLite+FTS5) |
| — | Automatic conflict resolution | Required; semantic merge driver — §9.1a / [02](02-ticket-storage.md) §2.7 |
| S1 | Notes storage | **Inline**, each note a timestamp-ordered UUID — [02](02-ticket-storage.md) §2.6 |
| C1 | Client sequencing | **Tauri+web → SwiftUI macOS → SwiftUI iOS → Android** — §9.5 |
| — | Attachments | Supported (first-class) — [02](02-ticket-storage.md) §2.5 |

**Still open (to resolve as implementation reaches each area — tracked in HS2-15):**

| # | Decision | Recommendation | Doc |
|---|---|---|---|
| A1 | Human assignment: identity mapping, review-request UX, off-server notification, roster source | Proposal in doc 10; confirm §10.5 | [10](10-assignment-and-collaboration.md) §10.5 |
| F1 | Which HS1 features to port / change / drop | Per-area decision tickets from the survey | doc 11 / HS2-22 |
| D1 | Orchestration: live-mount vs clone-and-serve (both?) | Both; live-mount primary | [08](08-distributed-and-remote.md) O1 |
| D2 | Cross-server aggregate views in v1? | Defer | [08](08-distributed-and-remote.md) O2 |
| M1 | iOS local stores vs remote-only | Remote-first, keep option | [08](08-distributed-and-remote.md) O4 |
| O5 | iOS push notifications | Defer; design event bus for a later relay | [08](08-distributed-and-remote.md) O5 |
| O6 | Remote terminals over wss | Defer past remote data | [08](08-distributed-and-remote.md) O6 |

The pivotal language decision (L1) is now **settled: Rust.** Implementation can
proceed.
