# 03. Indexing & Query

> **Status: v1 built (HS2-5 + HS2-6).** `crates/hotsheet-index` — a disposable
> SQLite + FTS5 cache (`Index`: schema/upsert/delete/`rebuild_from_store`/query +
> `hash_bytes`). The server builds an in-memory index on start, serves `GET /tickets`
> from it (structured filter + FTS prefix search), and runs a **`notify` filesystem
> watcher** (`spawn_watcher`, debounced) that reindexes changed files by content-hash
> and broadcasts change events — so a CLI/git edit shows up live. The index is
> **file-backed** at `~/.hotsheet/index/<hash>.sqlite` and **restored + reconciled on
> launch** (`Index::open_reconciled`: keep valid rows, re-read only the changed delta,
> rebuild if corrupt). Built: the `hotsheet-cli reindex` command, the git-diff
> reconcile fast path (§3.4), the `blocked_by`/`assignees`/`reviews` facet tables, and
> **keyset paging** (`page_after` cursor) + `me` identity resolution (§3.5, HS2-TCDTCH).
> **Not yet:** no-server incremental index maintenance and the watcher's git-diff
> fast path. SQLite + FTS5 is the recommendation; §3.7.

## 3.1 Why an index at all

The ticket calls it out directly: we must *"search the text of tickets
efficiently"* and *"draw tickets and their information efficiently, so we can't
necessarily be loading from disk in real time for every UI operation."*

So there is a **derived index** between the git-backed files (source of truth) and
every read the UI/CLI/API performs. The UI never walks the store directory to draw
a list; it queries the index.

**The invariant (repeated because it's load-bearing): the index is a disposable
cache.** It can be deleted and rebuilt from the ticket files at any time. If the
index and the files disagree, the files win and the index is corrected on the next
reconcile. Nothing is ever *only* in the index.

## 3.2 The choice: SQLite + FTS5

**Recommendation: SQLite with the FTS5 extension**, one index database per
machine per project (not committed — it's a cache).

Why SQLite:
- **Embedded, zero-config, transactional** — no server, single file, mature.
- **Both query shapes in one store.** Structured filtering/sorting (status,
  priority, category, tags, up_next, claim state, blocked_by) *and* full-text
  search (FTS5 over title + body + notes) in the same database, joined in one
  query. No second system to keep in sync.
- **Excellent bindings in both candidate core languages** (`rusqlite` for Rust,
  `mattn/go-sqlite3`/`modernc.org/sqlite` for Go).
- **Fast cold start.** On first run (or after `reindex`), we walk the store once
  and bulk-insert; thereafter only changed files are re-read (§3.4).

Location: `~/.hotsheet/index/<project-id>.sqlite` (machine-local, gitignored,
disposable). Keyed so a project spanning multiple stores has one index.

## 3.3 Index schema (sketch)

```sql
CREATE TABLE tickets (
  store_id      TEXT NOT NULL,        -- which store physically holds the file (derived from location)
  id            TEXT NOT NULL,        -- ULID
  slug          TEXT NOT NULL,        -- store-prefixed display slug (e.g. HS-7F3K9Q / SEC-2M8XQ1)
  title         TEXT NOT NULL,
  details       TEXT,
  category      TEXT,
  priority      TEXT,                 -- highest|high|default|low|lowest
  status        TEXT,                 -- not_started|started|completed|verified|backlog|archive|deleted|moved
  moved_to_store TEXT,                -- set only on a 'moved' tombstone (§2.13) → redirect target
  close_reason  TEXT,                 -- completed|not_planned|duplicate|obsolete (§2.6a); NULL = open
  duplicate_of  TEXT,                 -- ticket ULID when close_reason='duplicate' (resolved globally)
  closed_at     TEXT,
  up_next       INTEGER NOT NULL DEFAULT 0,
  tags_json     TEXT NOT NULL DEFAULT '[]',
  created_at    TEXT, updated_at TEXT, completed_at TEXT, verified_at TEXT,
  claimed_by    TEXT, claim_lease_expires_at TEXT, worker_label TEXT, claim_count INTEGER DEFAULT 0,
  -- provenance for incremental reindex:
  file_path     TEXT NOT NULL,
  git_blob_oid  TEXT,                 -- content hash for change detection (§3.4)
  content_hash  TEXT NOT NULL,        -- fallback hash when not in git yet
  PRIMARY KEY (store_id, id)          -- full identity is store + ULID (§2.2.1); lets a moved
);                                    --   tombstone in store A coexist with the live copy in store B

CREATE TABLE tags       (store_id TEXT, ticket_id TEXT, tag TEXT);           -- denormalized for fast tag facets
CREATE TABLE blocked_by (store_id TEXT, ticket_id TEXT, blocks_on_id TEXT);  -- flat dependency edges (blocks_on_id is a ULID, resolved to its live instance)
CREATE TABLE assignees  (store_id TEXT, ticket_id TEXT, person TEXT);        -- denormalized assignees (git email) for "assigned to me" + query-builder facets
CREATE TABLE reviews    (store_id TEXT, ticket_id TEXT, person TEXT, kind TEXT); -- review_requests (kind: work|feedback|review|fyi) — §10.2

CREATE VIRTUAL TABLE tickets_fts USING fts5(
  title, details, notes,
  content='',                          -- external-content or contentless; tuned at build
  tokenize='unicode61 remove_diacritics 2'
);

CREATE TABLE index_meta (key TEXT PRIMARY KEY, value TEXT);   -- schema version, last full reindex, watermark
```

Notes are indexed into the FTS `notes` column (concatenated) so text search hits
note bodies; the authoritative note entries live in the ticket file.

## 3.4 Incremental reindex

The server owns a **filesystem watcher** (`notify` in Rust / `fsnotify` in Go)
over every store path. On change:

1. **Debounce** rapid bursts (an editor save, a git checkout touching many files).
2. **Detect what actually changed.** For each candidate file, compare its current
   content hash (or git blob OID when committed) against `content_hash` in the
   index. Unchanged → skip (this is what makes a `git pull` that rewrites mtimes
   cheap).
3. **Re-parse only changed ticket files**, upsert their rows + FTS entries.
4. **Deletions:** a file that disappeared → delete its index rows (a soft-deleted
   ticket is a `status: deleted` file, not a missing file; a *missing* file means
   the ticket was hard-removed or moved).
5. **Emit a change event** on the WebSocket bus so every attached client redraws.

**Git-aware fast path.** When a store is a git repo and HEAD moved (a commit,
pull, checkout, or worktree switch), we diff `old-HEAD..new-HEAD` to get the exact
changed paths instead of stat-walking the tree — O(changes), not O(tickets).

**Full reindex** (`hotsheet reindex`, or automatic when `index_meta.schemaVersion`
is stale, or when the index file is missing/corrupt): drop and rebuild from a full
store walk. Because the index is disposable this is always safe.

## 3.5 The query surface

One query API, consumed by the server (REST/WS) and the CLI:

```
query(filter, sort, text?, paging) -> TicketRow[]
```
- **filter:** status set, priority set, category, tags (any/all), up_next,
  claimed/unclaimed, blocked/unblocked, **store (by `store_id`)**,
  **`close_reason`** (§2.6a — e.g. "closed as not_planned"), **assignee /
  review-requested (by person, incl. "me")** (§10.2), date ranges. These are the
  same dimensions the **custom-view query builder** exposes (HS2-29) — so views can
  filter on the new fields (store / close_reason / assignment).
- **sort:** priority-then-recency (the worklist order), created, updated, title;
  ULID gives a free chronological default.
- **text:** an FTS5 `MATCH` over title/details/notes, joined with the structured
  filter in one SQL statement.
- **projection:** a query returns compact `TicketRow`s. The Markdown `details` body
  is the one large field, so a list is **compact by default** — the body is omitted
  (fetch it per-ticket via get, or pass `compact=false` to include it). This keeps an
  unfiltered list small enough to browse (e.g. through the MCP tool's token budget).
- **limit:** an optional `limit` caps the number of rows returned (applied after
  sort, as a SQL `LIMIT` in the index and a truncate in the serverless scan). It is a
  hard cap, never a silent default — a caller asks for it explicitly.
- **paging:** keyset pagination for large stores — the general form of `limit`
  (HS2-TCDTCH, **built**). A `page_after=<ULID>` cursor returns only the rows that
  sort **strictly after** that ticket in the current `(sort, id)` total order, so a
  client pages a big store without SQL `OFFSET`. The index does it as a type-exact
  row-value comparison — `(order, t.id) > (SELECT order, id FROM the cursor row)` —
  and the serverless file-scan slices the sorted list after the cursor position; both
  paths return an **empty page** for a stale cursor (a ULID no longer in the store),
  so the client restarts from the top. To page, pass the last row's ULID as the next
  `page_after`. The CLI (`ls --page-after <slug|ULID>`) accepts a slug for convenience.
- **"me":** the `assignee` / `review_requested` person filters accept the sentinel
  `me`, resolved to the store's **git `user.email`** (the same identity assignment
  writes, §10.2) by the query builders in the CLI, server, and MCP shim (HS2-TCDTCH).
  An unresolvable `me` (no git identity) is an **error**, never a silent
  match-everyone.

**Store + move handling (§2.2.1, §2.13):**
- Every row carries `store_id`, so the UI can filter or group by store and show the
  store-prefixed slug. "Which store is this ticket in?" is a column, not a lookup.
- **`status = 'moved'` tombstones are excluded** from ticket lists by default. When
  something references a ULID (a `blocked_by` edge, a `HS-…` mention, a direct
  open), resolution returns the **single live instance** — the non-`moved` row for
  that ULID — following a `moved_to_store` redirect if it lands on a tombstone. So a
  move never breaks a reference.

This directly serves both "draw the list fast" (indexed structured query, no disk)
and "search efficiently" (FTS5).

## 3.6 Derived outputs stay derived

The `worklist.md` / `open-tickets.md` files HS1 generates for AI tools are also
**derived from the index**, regenerated (debounced) on change — not a second
source of truth. They remain the file-based contract any AI tool can read without
the API. See [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.9.

## 3.7 Alternatives considered

| Option | Verdict |
|---|---|
| **SQLite + FTS5** (recommended) | One store for structured + text queries; embedded; great bindings; transactional; disposable. **Chosen.** |
| Tantivy (Rust) / Bleve (Go) full-text engine | Excellent search, but structured queries + facets still need a second store to join against. Adds a system. Keep as a *later* option if FTS5 relevance proves weak — layer it for search only, SQLite still holds structured data. |
| In-memory index only | Fast, but slow cold start on large stores and nothing to page against; lost on restart. Rejected as the primary index (we may keep hot subsets in memory as a cache above SQLite). |
| DuckDB | Analytics-oriented; overkill for OLTP-ish ticket queries. |
| Re-embed Postgres (PGLite/real PG) | Reintroduces exactly the opaque-blob fragility the rewrite is removing. Rejected. |

## 3.8 Concurrency & safety

- **One index writer per machine per project**, guarded by a lightweight lock
  (the server, or the CLI when no server runs). Readers use SQLite WAL mode for
  concurrent reads.
- **Corruption is a non-event.** A corrupt index is deleted and rebuilt from git.
  No repair subsystem, no backups of the index (git is the backup of the data).
- **Schema evolution:** bump `index_meta.schemaVersion`; a mismatch triggers a
  full rebuild. Because it's a cache, index "migrations" are always "drop and
  rebuild," never data migrations.

## 3.9 Cross-references
- What's indexed (the file format): [02-ticket-storage.md](02-ticket-storage.md).
- Who runs the watcher and serves queries: [04-core-server-cli.md](04-core-server-cli.md).
