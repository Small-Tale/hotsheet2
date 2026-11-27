# 03. Indexing & Query

> **Status: v1 built (HS2-5 + HS2-6).** `crates/hotsheet-index` — a disposable
> SQLite + FTS5 cache (`Index`: schema/upsert/delete/`rebuild_from_store`/query +
> `hash_bytes`). The server builds an in-memory index on start, serves `GET /tickets`
> from it (structured filter + FTS prefix search), and runs a **`notify` filesystem
> watcher** (`spawn_watcher`, debounced) that reindexes changed files by content-hash
> and broadcasts change events — so a CLI/git edit shows up live. The index is
> **file-backed** at `${HOTSHEET_HOME:-~/.hotsheet2}/index/<hash>.v<schema>.sqlite` and **restored + reconciled on
> launch** (`Index::open_reconciled`: keep valid rows, re-read only the changed delta,
> rebuild if corrupt). Built: the `hotsheet-cli reindex` command, the git-diff
> reconcile fast path (§3.4), the `blocked_by`/`assignees`/`reviews` facet tables, and
> **keyset paging** (`page_after` cursor) + `me` identity resolution (§3.5, HS2-TCDTCH).
> **Not yet:** no-server incremental index maintenance (HS2-8XXZBV). The watcher's
> git-diff fast path is built. SQLite + FTS5 is the recommendation; §3.7.

## 3.1 Why an index at all

The ticket calls it out directly: we must _"search the text of tickets
efficiently"_ and _"draw tickets and their information efficiently, so we can't
necessarily be loading from disk in real time for every UI operation."_

So there is a **derived index** between authoritative provider records and every read
the UI/CLI/API performs. Today those records are git-backed files; direct external
providers will refresh the same normalized cache through their query/watch contracts.
The UI never walks a store directory or calls a provider directly to draw a list; it
queries the host.

Checkout text search includes connected GitHub Issues, GitLab, and Jira sources.
External list responses are matched locally against the issue key, title, description,
and labels using the same all-term word-prefix rule as the git index. Provider notes
are read on detail requests and are not searched in external list results.

Search scope follows the currently selected sidebar view. Queue, Backlog, and Archive
apply their collection predicate before the bounded search page, including for exact-slug
queries. After the selected view settles, the client performs bounded background searches
for the other views so their sidebar counts describe the same query without delaying the
primary results. The ordinary toolbar search accepts inline
field tokens without opening a separate advanced-search dialog. Typing `tag:` offers
matching project tags, including quoted tags with spaces, and selecting or completing one
turns it into a removable, double-click-to-edit chip. Complete filters take effect before
the trailing space is typed; an explicit space or Enter commits them as chips, while incomplete
bare and quoted values remain editable through incidental focus movement. Real editable
boundaries keep the keyboard caret reachable before, between, and after chips. Chips do not
truncate their values and their compact height preserves the empty/single-line search pill.
When wrapping makes the search taller, the toolbar grows downward while the other controls keep
their original top alignment and the project tabs and work area move down intact. The same
surface accepts `has:attachment`,
`has:media-annotation`, `has:commit`, `attachment:<name-or-*-wildcard>`, and inclusive
lifecycle bounds such as `created-after:<local-date>` or
`completed-before:<local-date-and-time>`. Attachment and media-annotation presence are
indexed fields; commit presence is evaluated against the selected checkout's git history.
`is:ai-created` matches only a recorded AI ticket creator; missing or unknown creator
provenance does not match. A lone positive term filters local indexed rows before
bounded pagination and applies the same predicate to provider rows, while Boolean
combinations, including `NOT is:ai-created`, evaluate against the complete candidate
set (HS2-18QFJ5).
Manual input
uses the client machine's date/time order, punctuation, clock, and numerals; ISO 8601 dates
and date-times (for example `2026-09-01` and `2026-09-01T11:05`) are always accepted.
Relative minute, hour, day, and week values such as `updated-after:4h ago` are resolved at
search time.
A native date/optional-time helper appears for lifecycle prefixes, and an adjacent help control
opens the complete syntax guide without overloading the ordinary search placeholder. The guide
groups compact, wrapping examples under Tags, Content, Workflow, and Dates, with boolean and
date rules summarized separately so the full syntax remains scannable instead of becoming one
long vertical list.
Created, updated, completed, and verified use their persisted timestamps. Because the v2
ticket format has no separate started/archived timestamp, those two tokens constrain the
current status and compare its `updated_at` transition time.
Ticket-reference mentions may participate in results only when the client labels why the
ticket matched. Full-text indexing includes attachment filenames alongside slug, title,
tags, details, and notes.

**The invariant (repeated because it's load-bearing): the index is a disposable
cache.** It can be deleted and rebuilt from configured providers. For the git
provider, files win; for an external provider, its native tracker wins. Nothing
durable is ever _only_ in the index.

## 3.2 The choice: SQLite + FTS5

**Recommendation: SQLite with the FTS5 extension**, one index database per
machine per project (not committed — it's a cache).

Why SQLite:

- **Embedded, zero-config, transactional** — no server, single file, mature.
- **Both query shapes in one store.** Structured filtering/sorting (status,
  priority, category, tags, up_next, claim state, blocked reason) _and_ full-text
  search (FTS5 over slug + title + tags + body + notes) in the same database,
  joined in one query. No second system to keep in sync.
- **Excellent bindings in both candidate core languages** (`rusqlite` for Rust,
  `mattn/go-sqlite3`/`modernc.org/sqlite` for Go).
- **Fast cold start.** On first run (or after `reindex`), we walk the store once
  and bulk-insert; thereafter only changed files are re-read (§3.4).

Location: `${HOTSHEET_HOME:-~/.hotsheet2}/index/<project-id>.v<schema>.sqlite` (machine-local, gitignored,
disposable). Keyed so a project spanning multiple stores has one index. The file name carries
the index `SCHEMA_VERSION` (`hotsheet_index::index_file_name`): opening rebuilds any file whose
schema differs, so binaries from different builds (an orphaned older server, a stale installed
CLI) each keep their own file instead of downgrading the schema under a newer running process
(`no such column`, HS2-8ZM4PT). Files left behind by older schema generations are inert caches,
pruned by `hotsheet_index::prune_stale_index_files` at server start and after `hotsheet-cli
reindex` (HS2-Y0PAEM): an unversioned `<project-id>.sqlite` or a `.v<N>` below this build's
schema is removed with its `-wal`/`-shm`/`-journal` sidecars **only when no live connection
holds it**. The probe takes a non-blocking exclusive SQLite lock (`locking_mode=EXCLUSIVE`,
`BEGIN EXCLUSIVE`), which fails while any process keeps the file open, because every open
WAL connection holds a shared lock on it. The file is unlinked while that lock is still held.
A held file is kept for a later prune. Newer generations (a newer build that may be running)
are never touched.
Several processes (the server, the CLI, the app's setup refresh) may open one file at once:
each connection waits up to 30 s on a busy lock, the schema check and creation run in one
`BEGIN IMMEDIATE` transaction so a concurrent opener re-reads the version the winner committed,
and only genuine corruption (`SQLITE_CORRUPT`/`SQLITE_NOTADB`) deletes and rebuilds the file — a
lock or race error never discards an index another process is using (HS2-SY8T90).
Opening, initializing, and any corruption delete-and-recreate of one file additionally run
under an exclusive OS advisory lock (`flock`) on a never-deleted `<index>-open.lock` sidecar:
switching a fresh file to WAL can fail with an immediate `SQLITE_BUSY`, and a reader can see
another creator's half-written header as "not a database", which SQLite's own locking does not
cover. Reconcile runs in one `BEGIN IMMEDIATE` transaction, because each ticket's upsert is several
statements (row upsert, then delete and reinsert of its tags, assignees, and reviews) that
concurrent reconcilers must not interleave; the later one then finds current hashes and has
nothing to do. Pruning an older generation also removes its open-lock sidecar (HS2-0ATCY8).

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
  close_reason  TEXT,                 -- completed|not_planned|duplicate|obsolete|works_as_designed (§2.6a); NULL = open
  duplicate_of  TEXT,                 -- qualified project/source/native ref, or a legacy bare ULID
  closed_at     TEXT,
  up_next       INTEGER NOT NULL DEFAULT 0,
  tags_json     TEXT NOT NULL DEFAULT '[]',
  created_at    TEXT, updated_at TEXT, completed_at TEXT, verified_at TEXT,
  claimed_by    TEXT, claim_lease_expires_at TEXT, claim_eta_at TEXT, claim_started_at TEXT, worker_label TEXT, claim_host TEXT, claim_count INTEGER DEFAULT 0,  -- claim_host: HS2-DQJPW0; rows report claim_origin relative to the reader
  latest_confidence INTEGER,          -- derived ops::latest_confidence (0-100) of a completed/verified ticket; NULL otherwise (HS2-RD4M29)
  -- provenance for incremental reindex:
  file_path     TEXT NOT NULL,
  git_blob_oid  TEXT,                 -- content hash for change detection (§3.4)
  content_hash  TEXT NOT NULL,        -- fallback hash when not in git yet
  PRIMARY KEY (store_id, id)          -- full identity is store + ULID (§2.2.1); lets a moved
);                                    --   tombstone in store A coexist with the live copy in store B

CREATE INDEX idx_tickets_duplicate_target ON tickets(store_id, duplicate_of)
  WHERE close_reason = 'duplicate';
CREATE TABLE tags       (store_id TEXT, ticket_id TEXT, tag TEXT);           -- denormalized for fast tag facets
CREATE TABLE blocked_by (store_id TEXT, ticket_id TEXT, blocks_on_id TEXT);  -- flat dependency edges (blocks_on_id is a ULID, resolved to its live instance)
CREATE TABLE assignees  (store_id TEXT, ticket_id TEXT, person TEXT);        -- denormalized assignees (git email) for "assigned to me" + query-builder facets
CREATE TABLE reviews    (store_id TEXT, ticket_id TEXT, person TEXT, kind TEXT); -- review_requests (kind: work|feedback|review|fyi) — §10.2

CREATE VIRTUAL TABLE tickets_fts USING fts5(
  slug, title, tags, details, notes,
  content='',                          -- external-content or contentless; tuned at build
  tokenize='unicode61 remove_diacritics 2'
);

CREATE TABLE index_meta (key TEXT PRIMARY KEY, value TEXT);   -- schema version, last full reindex, watermark
```

Tags and notes are indexed into concatenated FTS columns so one query covers ticket
identity, visible metadata, Markdown details, and note bodies; the authoritative values
remain in the ticket file.

Reverse duplicate lookup returns only source id, slug, and title for matching targets. The
partial index is maintained by the existing upsert/delete operations, so retargeting,
reopening, title changes, purging, external edits, and schema-rebuild recovery do not need
a second cache or a reader-side ticket walk. Schema version 15 rebuilds older disposable
indexes to add this lookup (HS2-Y7W3Z4).

## 3.4 Incremental reindex

The server owns a **filesystem watcher** (`notify` in Rust / `fsnotify` in Go)
over every store path. Every hosted store — the primary store and stores registered at
runtime — uses its own platform-native watcher (FSEvents on macOS), started on a background thread
so opening a project never waits for the OS to start the stream (HS2-P3SSGR). A native stream
drops changes made while it is still starting, and starting it can take seconds on a busy macOS
machine, so a short-lived 250ms poller bridges that window: the store is re-checked when the bridge
starts, the bridge is dropped once the native stream has proven it delivers (below), and the store is
re-checked once more. Each re-check processes only ticket files whose bytes differ from the index. The bridge compares file
contents, not just mtimes, because notify's poller records mtimes in whole seconds and would otherwise
miss an edit made in the same second as its previous scan until the native stream was up (HS2-XAHR91). That start-up
gap, not a second stream, is what made the old macOS registered-store test fail (HS2-SG1BKJ); the
permanent 250ms polling fallback it replaces re-walked each registered store four times a second and
kept an idle server busy. Steady-state detection takes tens of milliseconds plus the 150ms debounce.

**Which directory the native stream holds (HS2-VN96J5, HS2-C3REQ1).** On macOS and Linux the stream
watches the store's `tickets/` directory. On Windows it watches the store root and admits only events under
`tickets/`, because NTFS refuses to rename a directory while any handle is open beneath it: a
`tickets/` watch kept a hosted store from being moved or renamed (os error 5), so the relink flow
for a moved store could not be reached. A handle on the root itself does not block the move.
Windows directory handles and Linux inotify watches follow the directory, and notify keeps reporting
the old path. On those platforms, events that arrive while the root is missing from its path are
dropped rather than applied as deletions; the first ticket event after the store returns asks for
a full re-check, which indexes writes made while it was away.
Git and other churn beside `tickets/` never reaches the watch loop, so an idle store stays silent.
A relinked store is watched again at its new path when it is hosted there. The target-feed watch of
an unhosted linked store (HS2-G5FYP6) uses the same scope.

If a hosted store is deleted and recreated at the same path, the server watches for the store root
to return (HS2-R6FZ54) through a root watch (`hotsheet_jobs::watch::RootWatch`) on macOS and Linux,
not a registration of the parent with the native watcher. Windows still registers the parent
without its subtree (see below). On macOS it does not register the parent
with FSEvents (HS2-AZFCB6): an FSEvents stream is always
recursive, so the parent registration routed every event beneath the parent (a whole projects
folder on a development Mac) to the server only to be dropped. Instead a root watch
(`hotsheet_jobs::watch::RootWatch`) holds kqueue vnode filters on the store root (removed, renamed,
revoked) and on its parent's entries (`NOTE_WRITE` fires for an entry added, removed or renamed,
never for anything deeper), plus the parent being moved or removed; while the parent is missing it
watches the nearest existing ancestor's entries. It reports only when the directory object at the
store path changed (device and inode, or present and absent) or the root or its parent moved, so
churn beside the store delivers nothing. Each wake registers again while the old registration stays
in place and collects events raised in between, so a move away and straight back is still reported.
FSEvents' own `kFSEventStreamCreateFlagWatchRoot` was measured first and rejected: it dropped about
one in six root-changed notices when the store or its parent moved away and back, sometimes several
in a row.

On Linux and Windows a parent registration belonged to the parent's inode or handle, so it ended
when the parent was removed and a parent recreated with a store inside was never re-armed
(HS2-CRXWR9). The root watch there follows the path instead. A platform-neutral plan
(`crates/hotsheet-jobs/src/root_plan.rs`) picks the anchor: the store's parent, or while it is
missing (or cannot be opened) the nearest existing ancestor, whose entries are watched for the one
name that leads to the store. It reports when a wake may have moved the store (the store, the
anchor or an ancestor left; notices were dropped; the store's own entry in its parent changed) or
the directory object at the store path changed, and re-registers on any wake that concerns the path
while the old registration still collects notices. Linux uses inotify directly: the anchor's
entries and its own removal or move, the store's own removal or move, and every ancestor above the
anchor for its own removal or move only (so a grandparent moved away and back is reported, without
its entry churn). Windows uses `ReadDirectoryChangesW` (subdirectory names, no subtree) on the
anchor and on the anchor's parent (for the anchor's own name, since Windows has no self-move
notice); a failed read (the anchor deleted) or an overflowed one re-resolves and reports. A
registration that went stale while it was made (a path changed between resolving and registering
it) is never kept, since the notice that would correct it may already be taken; an existing
directory the platform refuses to register counts as missing, so this ends. Names
compare without case on Windows. A root watch that cannot start falls back to the parent
registration. The Windows adapter is built and its decisions are unit-tested on every host, but it
has not run on Windows yet, so `RootWatch::supported()` is false there and Windows production keeps
the parent registration, with the removed-parent gap, until HS2-PR9X5H verifies it on Windows and
flips `supported()`. On macOS and Linux the watcher also watches the root without
recursion for a new `tickets/` directory. It removes the invalid native watch, registers the
replacement directory, and reconciles the entire store index before declaring the watch live.
The removal signal marks the index out of sync before a platform-native unwatch can block,
so queries do not trust rows from a deleted store during re-registration.
That reconciliation removes rows from the deleted store as well as indexing files in the new one.
When rows or a corrupt ticket's source hash change, it emits one store-scoped ticket
invalidation so connected clients reread the index state.
These parent and root monitors respond to filesystem events; they do not rescan idle stores.
Their events only signal re-arming: on every platform the watch loop admits only paths under
`tickets/`, so churn in a busy parent (often the shared temporary directory) never reaches it
(HS2-TFBFRP).
The parent (or the macOS root watch) is registered before its child watches; if the root watch
cannot start, the parent registration is used instead. During startup, a missing child leaves the
parent monitor running, and a directory identity sampled before and after child registration
detects replacement during that call and triggers a fresh child registration
and full reconcile before the watcher becomes live.

**A native registration is trusted only once it reports a probe (HS2-190GDZ).** On a busy macOS
machine (heavy file churn keeping `fseventsd` behind) a freshly started or re-armed FSEvents stream
can return from registration and then report nothing under `tickets/`, with no dropped-event flag,
so a write landing after the reconcile was lost. A fixed wait cannot tell such a stream from a
live one. After every registration (startup, a startup replacement, and every re-arm) the watcher
creates and deletes an empty probe file, `.hotsheet-watch-probe.<pid>.<n>.tmp`, in the watched
directory and waits for the stream to report it. Native streams deliver in order, so a reported probe
proves every later write will be reported and every re-arm signal raised before it has arrived.
Only then is the bridge retired and the store reconciled again (the index is already live through
the start-up bridge, HS2-P702Y8). An
unanswered probe is retried after 1 s, doubling to at most 30 s, and every second unanswered probe
registers the directory again, since a dead registration may never recover. Under a deep
`fseventsd` backlog no probe may arrive for minutes. The first unanswered probe therefore starts
the 250ms bridge (or keeps the startup bridge), and a full reconcile makes the bridge-backed index
live. The bridge then sees every write within one scan, like the native stream's own delivery
latency. Once a probe finally arrives, the bridge is retired and the store is reconciled again.
The bridge polls only while a registration is unproven; an idle store with a proven stream is not
polled. The bridge follows the `tickets/` path, so while the store root is missing (moved away) it
drops its events instead of reporting every ticket deleted, and marks the index out of sync. When
the root returns, it requests the full reconcile, as the native re-arm does. Probe events never reach
the watch loop. Probe names match the store's `.*.tmp` staged-write ignore pattern, and the ticket
listing skips plain files directly under `tickets/`. A store that cannot take a probe (read-only)
falls back to the former 1.5 s start-up wait. On Linux and Windows the probe is reported at once,
so startup no longer waits a fixed delay there either.

**A proven stream can still lag; it is verified on demand (HS2-P702Y8).** A probe proves a
registration, not later timeliness: under a deep `fseventsd` backlog (four loops creating and deleting
200 files each in a sibling temp directory) a proven macOS stream reported a ticket write more than
two minutes late. No stream setting helps: notify's FSEvents backend and the repository watcher's own
stream already use latency 0 with `NoDefer` and `FileEvents`, and the backlog sits in `fseventsd`,
which serves every client in order. Nothing polls to detect lag; instead the watcher proves its
stream current when something depends on it:

- **Start-up and re-arm never wait on registration.** The start-up bridge's full reconcile makes the
  index live at once, as the Poll backend's resync does, so a registration that blocks for minutes
  no longer leaves misses walking. A re-arm starts the bridge (and its reconcile) before the
  unwatch and watch calls that can block, and a root change reported while the bridge holds the
  store queues a full reconcile itself, so the index recovers even while the supervisor is still
  waiting for a probe.
- **Verification.** A native watcher registers a verifier on the store's `IndexFreshness`. A request
  writes a probe; native streams deliver in order, so once it is reported every earlier write is
  queued, and a marker sent behind it tells the watch loop to record the verification once applied
  (the batch closes at once rather than after its 150 ms quiet wait). If the probe is unanswered
  after 1 s the stream is lagging: the bridge starts, a full reconcile brings the index current, and
  the proof continues with its back-off until a probe arrives, when the bridge is retired and the
  store is reconciled again. At most one request is outstanding.
- **Who asks.** An in-sync miss is final only within 1 s (`VERIFIED_WINDOW`) of the last
  verification (any applied full reconcile counts). Otherwise the lookup requests one, waits up to
  250 ms (`MISS_VERIFY_WAIT`), and asks the index again once it lands; if it does not land, the
  lookup walks. Every server ticket write and purge requests one too unless one landed within the
  window, since the stream owes it an echo, so active use detects a lag within about a second and
  bridges external writes. An idle store is never probed. A store without a verifier (the Poll
  backend, or a stopped watcher) keeps the in-sync rule alone, and a read-only store, which cannot
  take a probe, is treated as verified.

Watcher tests that must survive a backlog drive an in-process stream (`StartupHooks::stream`) that
reports events and probes in order and can be held and released like a backlogged `fseventsd`. The
real-stream tests request a verification after an external write, so a lagging stream answers
through the bridge within a bounded time. `store_root_scope_tests::hs2_vn96j5_root_scope_is_quiet_when_idle_and_ignores_git_churn`
remains a real-FSEvents smoke test: it waits for the stream's own delivery, ordered after the churn
it must reject.

**Followed stores, checkout refs and repository monitors (HS2-BN9MMJ).** The target feed's watches
on unhosted linked stores (HS2-G5FYP6) and on a followed checkout's refs and coordination pool
(HS2-XA9DZA, HS2-ESVYQH) trust a live stream just as the store watcher did, so under a backlog a
linked-store claim, a push or a review return reached the feed minutes late. They are now caught up
by the follower's own requests, never by a timer:

- Each feed request that asks again for a live watch reads it once (local files only): the
  store's ticket files written since the watch began, at most once a second per store, or the
  checkout's remote-tracking fingerprint and pool state. A follower's long poll therefore bounds
  the lag to its own request cadence. Until a stream is live, the existing one-second pending
  re-check still covers it.
- A stream event, a pending re-check and a request's read record each change once: a linked
  store notes every file a stream event reports at its modification time, and the ref watch
  records only when the remote-tracking fingerprint or the pool state actually changed. The
  fingerprint counts only the `refs/remotes/` lines of `packed-refs`, so packing local branches or
  tags is no push. A rescan notice (the platform dropped events) reads the state.
- The native repository monitor still re-reads Git status only on events; its catch-up under a
  backlog is HS2-GMYHNT.

Consumer tests run on `hotsheet_jobs::watch::InProcessStream`, an in-process stand-in for the
platform service that routes reported events by registered path and depth, in order, and can be
held and released like a backlogged `fseventsd`. Each consumer module keeps one real-stream smoke
test, and the platform adapter's own `PathWatcher` tests stay on the real service. Real-stream waits
end at an ordered delivery (a marker written after the writes a test checks), never at a quiet
window, and fail only past `REAL_STREAM_TEST_BOUND` (five minutes).

**The checkout's repository watcher leaves ignored build trees out at the source (HS2-2HZN2C).**
Separate from the store watcher, each open checkout has a repository monitor (`docs/06-clients.md`,
HS2-4S9ENS) whose stream covers the whole working tree. FSEvents streams are recursive, and notify
filters only in its own callback, so every write under `target/`, `node_modules/` or an agent
worktree's `target/` was routed by `fseventsd` to the server and dropped there. That kept
`fseventsd` near 100% CPU and the server around a third of a core on a busy development Mac. On
macOS the repository stream is now created directly (`hotsheet_jobs::watch::PathWatcher::start_excluding`)
with `FSEventStreamSetExclusionPaths`, so the file-event service drops those events itself. A stream
takes at most eight exclusions. They are chosen from the ignored directories Git reports
(`git ls-files --others --ignored --directory`) inside the working tree and confirmed by
`git check-ignore`: Git also lists an untracked directory whose files all happen to be ignored
(`.claude/` holding only `.claude/worktrees/`), and a new file there must still count, so the event
filter and the exclusions keep only entries an ignore pattern matches. Both Git reads are skipped
while the listing is unchanged. An exclusion is never one that is or contains
a watched Git directory or the group registry, ranked by events the filter has dropped so far, then
known heavy output names (`target`, `node_modules`, `worktrees`, `dist`, `build`, ...), then the
shallowest path. An excluded directory's own creation and removal are still reported to its parent;
nothing inside it is. The ignored set is re-read after every status read. When the pick changes, a
replacement stream starts on a helper thread while the current one keeps reporting, the old one is
stopped off-thread, and the status is read once more. An exclusion Git no longer ignores (a
`.gitignore` edit or a forced add) is dropped at once; a new ignored tree (a first build) is added
at once; any other swap waits 30 s after the last replacement, so churn alternating among more than
eight trees cannot restart the stream often. The exclusions are paths, so they survive the checkout
root being removed and recreated.

**On Linux the repository watcher takes no inotify watches under ignored trees (HS2-236XY3).**
inotify has no recursive watch: notify's recursive mode adds one kernel watch per directory, so
every directory under `target/`, `node_modules/` and agent worktrees counted against the per-user
watch limit and delivered its churn. `PathWatcher::start_excluding` on Linux watches each directory
outside the excluded trees on its own (non-recursive, with notify's per-directory mask, so tracked
paths report what they did before) and accepts up to 64 exclusions, picked by the same ranking. The
planning is platform-neutral (`crates/hotsheet-jobs/src/tree_plan.rs`) and the inotify adapter is
small (`inotify_tree.rs`):

- The start walks the tree, watching a directory before listing it, pruning excluded subtrees and
  never following a symlink below a root (notify's recursive walk followed them).
- A directory created or renamed into a watched directory is walked and watched with everything
  under it. Entries already inside it (created before its watch) are reported as one create event,
  so a `mkdir -p` followed by a write is not missed. A directory inside an excluded tree is never
  watched; one renamed out of an excluded tree into a tracked directory is.
- A directory removed or renamed away is forgotten with every watched directory under it, so a
  rename within the tree reports under the new path only. Removed and recreated, it is watched again.
- A queue overflow drops every watch and walks the tree again, after the rescan event already tells
  the monitor to re-read.
- A watch refused for a directory that vanished or cannot be read skips that subtree. Exhausting
  the watch limit fails the start (the monitor then falls back to its Git-status poll, as before);
  after the start it is delivered as an error, which the monitor treats as a change and re-reads.
- notify runs its handler on its event-loop thread and `watch` waits for that thread, so the handler
  only queues what an event means and a manager thread adds and removes watches.
- Two deviations: a removed or moved tracked subdirectory also reports its own removal or
  rename-from (same path, a second kind), because notify gives every directory watched on its own
  the root's self flags; and, as before on Linux, a removed root is not followed when recreated.

Windows `ReadDirectoryChangesW` watches a whole tree through one directory handle and has no way to
leave a subtree out (nor any per-directory watch cost to save), so Windows keeps notify's recursive
watcher and the event filter drops ignored events.
The store watcher's former stream over a store's parent directory (HS2-R6FZ54) was a separate
source of the same cost; it is replaced by the root watch above (HS2-AZFCB6 on macOS, HS2-CRXWR9 on
Linux; Windows pending HS2-PR9X5H).

On change:

1. **Debounce** rapid bursts (an editor save, a git checkout touching many files). A batch
   is applied after a 150 ms pause in events, or at most 1 s after its first event, so a
   stream that never pauses cannot hold a write back indefinitely (HS2-TFBFRP).
   A new shard directory (`tickets/<XY>/`) is listed when its event is applied, and once
   more 1 s later: Linux inotify reports a new directory before it watches it, so a ticket
   file created in between reports no event. The second listing is a one-shot per
   directory event, not a repeating poll, and applies only files whose bytes differ from
   the index (HS2-ZER3MA).
2. **Detect what actually changed.** For each candidate file, compare its current
   content hash (or git blob OID when committed) against `content_hash` in the
   index. Unchanged → skip (this is what makes a `git pull` that rewrites mtimes
   cheap).
3. **Re-parse only changed ticket files**, upsert their rows + FTS entries. A parse
   failure keeps the last healthy indexed row for cache resilience but still emits a
   `changed` invalidation so clients refresh the separate corrupt-ticket diagnostics.
4. **Deletions:** a file that disappeared → delete its index rows (a soft-deleted
   ticket is a `status: deleted` file, not a missing file; a _missing_ file means
   the ticket was hard-removed or moved). A checkout can also remove a file after
   enumeration but before the read (HS2-PT0Y8A). Rebuild skips that path and full
   or Git-delta reconciliation drops only its cached row; neither blocks project
   open. The warning names the ULID and operation without copying ticket content or
   local paths. Other read errors remain failures, and restoration is indexed from
   the file's real bytes on the next reconcile.
5. **Record and emit a change event.** The server appends it to the bounded replay
   ring before broadcasting it, so both WebSocket subscribers and cursor-based
   long-poll clients observe external CLI/git writes without a race. A client that
   reconnects after its cursor fell out of the ring receives `overflow` and performs
   an authoritative project refresh.

**Failed index writes are repaired, not swallowed.** When a server write or a watcher
upsert/delete/hash record fails in SQLite, the failure is logged and the ticket is queued
for repair (HS2-JD7TK0). The store's next server write retries the queue first, and each
store watcher also retries it on its own bounded backoff (1s doubling to 60s, reset once the
queue drains), so a store with no further writes still converges without a restart
(HS2-5BAEX9). A repair re-indexes the ticket from its current file, or drops the row when
the file is gone; a still-failing repair stays queued.

**Git-aware fast path.** Once a store has an indexed HEAD baseline, reconciliation
combines the `old-HEAD..new-HEAD` diff with Git's current ticket worktree delta. It also
remembers the previous dirty-ticket set so reverting an external edit refreshes the stale
projection even after the path disappears from `git status`. Warm server and CLI reads
therefore parse only committed or uncommitted changed ticket files rather than stat-walking
the tree — O(changes), not O(tickets). A missing baseline or non-Git store uses the safe
full walk.

The baseline must come from the repository that owns the store (HS2-ABV2J3,
`hotsheet_ticketing::store_git`). One scrubbed `rev-parse` classifies the store as
**owned** (its root is a working-tree top level, including a linked worktree's `.git`
file), **designated** (a subdirectory whose `hotsheet-store.json` the enclosing repository
tracks; diff and status stay scoped to that subtree), **unowned** (an enclosing checkout
that does not track it, such as a store under an ignored `target/`), no repository, or
unavailable (bare or failing Git). Only owned and designated stores with a commit supply a
baseline; an unborn branch, an unowned layout and the other states take the full walk and
clear any stored baseline, so an unrelated parent's HEAD never hides a write. The index
stores the owner key beside HEAD and trusts a warm delta only when the key still matches;
a legacy cache without a key, a replaced repository, or a stored HEAD the repository no
longer has also takes one full walk. The worktree delta includes ignored ticket files.
`hotsheet-cli doctor` reports unowned, repository-less and unavailable layouts with the
recovery step.

**Full reindex** (`hotsheet reindex`, or automatic when `index_meta.schemaVersion`
is stale, or when the index file is missing/corrupt): drop and rebuild from a full
store walk. Because the index is disposable this is always safe.

**Resolving one ticket by slug (HS2-ZXFNE6).** A ULID names its file directly. A slug
(ASCII case-insensitive) is resolved index-first: the server asks the store index for the
candidate ULIDs carrying that slug (`Index::slug_candidates`, lowest ULID first), reads
only the first candidate's file, and returns it when the file still carries the slug
(`hotsheet_index::resolve_ticket`, used by every server route through
`StoreEntry::resolve`). The files stay authoritative, so a miss, a stale row (the ticket
was renamed, moved, or deleted since it was indexed), or an index that cannot answer falls
back to the store walk, which gives the same answer as before. The walk
(`FsStore::find_ticket_by_slug`, behind `ops::resolve` for index-free callers such as the
CLI and MCP) reads each file's frontmatter `slug:` line and fully parses only possible
matches. It parses a file whose slug line is not a plain token, keeps lowest-ULID-wins for
duplicate slugs, and lets a corrupt file fail only the lookup of its own recoverable slug.
Server `blocked_by` references on create and update (`ops::resolve_blockers_with`) and the
git provider the server builds for each hosted store (`GitProvider::with_resolver`, given
`StoreEntry::resolver`) resolve through the same index-backed lookup, so provider routes
and blocker references never walk a hosted store on a hit (HS2-HEKMKH). Distilled activity
notes (whose provider is built over the hosted entry) and the note route's cross-ticket
`attachment:[TICKET-SLUG]` reference checks (`ops::attachment_reference_warnings_with`) use
the same lookup (HS2-2MJCMV); the git provider's
own default, used without an index (CLI, MCP), stays the walk. The git provider also
refuses a self-referencing `blocked_by` on update, as the server route does.
On a generated 3,000-ticket store (debug build), a slug `GET /tickets/<SLUG>` dropped from
about 1.7 s to about 3 ms; a miss, which still walks, dropped to about 130 ms.

**Index-authoritative misses (HS2-3R0C6G).** While the store's watcher reports the index
in sync, an index miss is the answer and the walk is skipped
(`hotsheet_index::IndexFreshness`, carried by each `StoreEntry`). In sync means the watcher
is live (its stream is running and the resync covering writes made while it started has
been applied), no watcher event is queued or mid-debounce, and no failed index write is
waiting for repair. Any of those — and a store nobody watches, a watcher error or rescan
request (until the full re-check it triggers is applied), or a stopped watcher — sends a
miss back to the walk. Freshness is sampled before and after the candidate query, so an
event queued meanwhile also walks. A stale candidate still walks. Where the watcher can
prove its stream current, the miss is also final only after a recent verification
(HS2-P702Y8, above), so a lagging stream cannot make a stale index authoritative. The
remaining blind spot is a write made after the last verification, at most about a second
before the lookup, and not yet delivered; a lookup racing an external create can miss it,
as one sent a moment earlier would. A
corrupt ticket file is never indexed, so before trusting an in-sync miss `StoreEntry::resolve`
consults the entry's stat-validated corrupt-file cache (only changed files are re-read);
when a corrupt file still declares the slug, the walk runs and returns that file's parse
error, keeping the lookup pointed at recovery (HS2-SGNK11). Measured on a 3,000-ticket store (debug build, in-memory
index): a miss took about 89 ms through the walk and about 1.6 ms in sync.

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
  **Completion confidence (HS2-RD4M29):** `min_confidence` / `max_confidence` are
  inclusive 0-100 bounds on the indexed, derived `latest_confidence`. Either bound
  matches only scored (completed/verified) tickets, so `max_confidence=69` lists the
  low-confidence completions to review. CLI `ls --min-confidence/--max-confidence`,
  server `GET /tickets?min_confidence=&max_confidence=`, and MCP `hotsheet_query` share
  the contract (a bound outside 0-100 is rejected). GitHub, GitLab, and Jira reject these
  filters explicitly, because their list reads carry no comment-derived score. Every list
  row carries `latest_confidence` when present.
  Blocked/unblocked uses the normalized, non-empty `blocked_reason` as its single source
  of truth. `blocked_by` edges remain indexed dependency context, but never create a
  blocked state without visible explanatory text.
  **Groups (HS2-PDSHNN, HS2-44W3TP, HS2-9KZSD4):** `group=<id or name>` matches the
  tickets of one group and `has_group=true|false` matches grouped or ungrouped tickets.
  The index stores the ticket's raw value in `group_name` and its comparison key in
  `group_key`: the group id itself, or for an unmigrated legacy name its
  Unicode-lowercased key (computed in Rust because SQLite's `lower()` folds only ASCII).
  A legacy key is lower case and an id is upper case, so the two never collide. A partial
  index covers `(store_id, group_key)`. A filter value matches its own id and the legacy
  key of its text; a caller that has the project's group registry (the checkout routes,
  the CLI inside a linked checkout, the MCP checkout paths) also passes
  `TicketQuery::group_ids`, the ids whose registry names have that key, so a name finds
  id-keyed tickets. A value that normalizes to nothing matches no ticket.
  `Index::group_members` feeds the group directory (listing, rename, ungroup, merge, and
  migration candidates) without a store walk. Schema 23 added the columns and schema 24
  changed `group_key` to hold ids, so an older index file is rebuilt on open like any
  other schema change (§3.2). CLI `ls --group/--has-group/--no-group`, server `GET
/tickets?group=&has_group=` (and the checkout and provider lists), and MCP
  `hotsheet_query` share the contract; the file scan (`ops::query`) and the index agree.
  Saved-view `group:` terms resolve through the checkout's registry the same way.
  A provider without the `groups` capability reports every ticket ungrouped, so both
  filters are answered for it rather than rejected. In the saved-view and notification
  query language (`hotsheet_ticketing::saved_views`), `group:name` and
  `group:"name with spaces"` match by the same key with the quoting and `\"` escaping of
  `tag:`, `has:group` matches any grouped ticket, and `NOT has:group` finds ungrouped
  tickets. Every list row carries `group` when set.
- **sort:** priority-then-recency (the worklist order), created, updated, title,
  confidence (least confident first, unscored last; `checkout_order::confidence_rank`
  in memory and `coalesce(latest_confidence,255)` in SQL, so keyset pages agree);
  ULID gives a free chronological default. Queries accept ascending or descending order;
  priority/status/title retain recent-first then stable-id tie breakers in either direction.
- **text:** an FTS5 `MATCH` over slug/title/tags/details/notes, joined with the structured
  filter in one SQL statement.
- **projection:** a query returns compact `TicketRow`s. The Markdown `details` body
  is the one large field, so a list is **compact by default** — the body is omitted
  (fetch it per-ticket via get, or pass `compact=false` to include it). This keeps an
  unfiltered list small enough to browse (e.g. through the MCP tool's token budget).
  For an even leaner row, `fields=slug,status,up_next,title` keeps only the named keys
  (plus `slug`, always) — a JSON allow-list applied at the wire layer over the shared
  `TicketRow`, so server + MCP share one projection (`wire::project_fields`, HS2-GY3GWT).
  Names are trimmed and empty entries ignored, so `fields=slug, title` works. A name
  that is not a ticket row key (`wire::PROJECTABLE_FIELDS`: the serialized `TicketRow`
  and provider `ApiTicket` keys, pinned to both structs by a source test) is refused
  with 400 `unknown fields: …` on the store, provider, and checkout lists and in the
  MCP, never silently dropped (HS2-Q2JEGK).
  The MCP shim additionally drops from pretty- to **compact JSON** once a result crosses
  ~8 KB, so a big list dump doesn't spend the caller's tokens on indentation.
- **provider-qualified lists (HS2-DXD7DZ):** `GET /providers/{connection_id}/tickets`
  returns full provider tickets (body, notes, attachments, claim history) by default.
  An explicit `compact=true` returns the shared
  compact `TicketRow` instead (the same keys as a compact store row, with the provider's
  own qualified identity), and `fields` selects from whichever shape was chosen. MCP
  `hotsheet_query` with `connection` sends `compact=true` unless the caller passes
  `compact`, so the tool's compact default holds for every target. `compact=false` and
  `hotsheet_get` remain the deliberate full reads. The serverless MCP backend answers its
  own Git connection's list with the same projection and refuses any other connection
  with 409. Its filters match the server's, including the inclusive lexical
  `created_*`, `updated_*`, `completed_*` and `verified_*` bounds, which pass through
  unvalidated on both backends. On 27 synthetic tickets with 11 notes and one attachment each, the
  five-field projection through the real MCP binary dropped from 506,457 to 3,979
  characters (41 to 5 keys), and the median call from 78 ms to 15 ms.
- **limit:** an optional `limit` caps the number of rows returned (applied after
  sort, as a SQL `LIMIT` in the index and a truncate in the serverless scan). It is a
  hard cap, never a silent default — a caller asks for it explicitly.
- **bounded store lists (HS2-3JEFQT):** every unpaged store-level list — server
  `GET /tickets` and `GET /stores/{id}/tickets`, and MCP `hotsheet_query` without
  `checkout` through either backend — follows the same 500-row contract as checkout reads
  (`ops::StoreReadBound`, `STORE_READ_MAX_ROWS`):
  - without `limit`, a read that would return more than 500 rows fails with
    `400 more than 500 tickets match; pass limit (at most 500) and page with page_after`;
  - `limit` above 500 is `400`;
  - an explicit `limit` accepts truncation; the server flags omitted rows with
    `x-hotsheet-truncated: true` (MCP wraps a truncated checkout array in
    `{items, partial, truncated, warnings}`; store-level reads use the explicit
    `limit` and a full page as the signal to continue).

  The provider-qualified array `GET /providers/{connection_id}/tickets` follows the same
  bound (HS2-6YXGKH), so a large GitHub, GitLab, or Jira project is never serialized into
  one response. Provider lists have no `page_after`, so their refusals point at
  `GET /checkouts/{checkout}/tickets` with `page_size` for larger reads. The web client
  reads provider tickets through checkout pages, not this route.

  The CLI `ls` is intentionally unbounded: it reads the local index directly and streams
  rows to a terminal rather than serializing one response, and it already offers
  `--limit` and `--page-after`. The web client does not use store-level lists.

- **paging:** keyset pagination for large stores — the general form of `limit`
  (HS2-TCDTCH, **built**). A `page_after=<ULID>` cursor returns only the rows that
  sort **strictly after** that ticket in the current total order, so a
  client pages a big store without SQL `OFFSET`. The index does it as a type-exact
  direction-aware comparison (including the mixed primary/recent-first/id order used by
  workspace sorts), and the serverless file-scan slices the sorted list after the cursor position; both
  paths return an **empty page** for a stale cursor (a ULID no longer in the store),
  so the client restarts from the top. To page, pass the last row's ULID as the next
  `page_after`. The CLI (`ls --page-after <slug|ULID>`) accepts a slug for convenience.
  `page_after` is a **row-identity** keyset for one store: it re-reads the boundary row's
  _current_ sort values, so editing that row's sort key under a traversal can skip or repeat
  rows, and purging it ends the traversal (empty page). That hazard is accepted for this
  single-store convenience surface (decided HS2-74H84S); callers that need mutation-safe
  continuation use the checkout `cursor` below, whose value keysets never depend on the
  boundary row. `TicketQuery::after_key` exposes the same value keyset to the index and the
  serverless file scan.
  Checkout/browser collection reads use the higher-level `page_size` + opaque `cursor`
  contract. Each response is capped at 500 compact rows and includes constant-memory SQL
  aggregates for navigation counts; callers must explicitly request another page, so a
  100K or 1M checkout is never serialized into one response or eagerly retained by the UI.
  A versioned checkout cursor (`v2.`) records the last emitted row's sort-key values plus
  each source's exhaustion flag and optional provider resume hint. The server reads each
  source in bounded batches (about `page_size` rows: one index query or one provider keyset
  read per refill, with `has_commit` evaluated once per batch, HS2-BGZ0NY) and performs a
  k-way merge over those buffers in the requested total order, including recent-first `updated_at` and stable qualified-identity ties for priority,
  status, and title sorts. Continuations therefore resume the same global order instead of
  concatenating independently sorted source pages (HS2-2BDSRK).
  Without `page_size`, the checkout route returns a plain row array (the shape MCP
  `hotsheet_query` and client search/lookup use) in that same global order: the server runs
  the same batched merge with the shared `hotsheet_ticketing::checkout_order` comparator,
  applies `limit` to the checkout-wide result, and projects `fields` only after merging
  (every row keeps its source `store`). The serverless MCP backend merges multi-store
  checkouts the same way (HS2-M0YTB6).
  A failing external source does not suppress healthy git or other provider rows. Paged
  responses include `source_errors` when a provider could not be read; their `counts`
  then cover only successfully read sources, so the client shows a partial-data warning
  alongside the available tickets. Unpaged responses retain their array shape and set
  `x-hotsheet-partial: true` when a source was omitted. MCP `hotsheet_query` preserves
  a complete unpaged checkout result as a plain array, but wraps a partial or
  truncated result in `{items, partial, truncated, warnings}`. The warning strings
  identify omitted sources and a limit that cut rows. The HTTP backend reads both
  response headers; without a server, the MCP backend marks a checkout partial
  when it has enabled external sources that only the server can read (a disabled
  connection, or a `source=git` read, omits nothing, as on the server). The serverless
  backend detects truncation by reading one additional merged row. A fresh page after recovery
  includes the source again; callers restart pagination rather than continuing a
  partial cursor when they need the recovered source's older rows (HS2-190BAS).
  The web client carries partial status through checkout search, sidebar search counts,
  continuation pages, and bounded title/lookup reads. It keeps healthy search rows visible,
  warns when a source failed, and omits incomplete search counts. Project, view, board,
  tab, and operations-sidebar totals are lower bounds until a fresh healthy read
  clears the partial state; the visible counts stay plain numbers and the lower bound is
  carried by their tooltips and accessible names ("At least N …"), not a `≥` glyph
  (HS2-521WNH). Its unpaged client method retains the partial and truncated
  headers alongside the row array; title deduplication refuses an incomplete destination
  read because it cannot guarantee a unique title (HS2-NX6JQJ).
  The unpaged array is **bounded** by `CHECKOUT_READ_MAX_ROWS` (500), the same ceiling as a
  page (HS2-CYXS0N):
  - without `limit`, a read that would return more than 500 rows fails with
    `400 more than 500 tickets match; …` rather than serializing the whole checkout;
  - an explicit `limit` (at most 500; larger is `400`) accepts truncation, and the server
    flags omitted rows with the `x-hotsheet-truncated: true` response header;
  - `cursor` without `page_size` is rejected; whole-checkout readers page instead.

  **Skipping counts (HS2-VPEAM4).** Every page computes `counts` by default, and a
  hosted-provider source computes its summary with a full provider walk. A caller that
  ignores counts passes `counts=false`: the page then carries an explicit `"counts": null`
  (never a stale object) and no source summary is read. Any other `counts` value
  is `400`. `counts` is not a filter, so it does not change the cursor fingerprint. Whole-
  checkout walkers (`Api.checkoutTicketRowsPaged`, MCP `hotsheet_query` walks with
  `counts: false`) use it; the web list's own pages keep counts.

  Callers: the web client's copy-drag title de-duplication reads the destination through
  title-only, count-free cursor pages (`Api.checkoutTicketRowsPaged`), and its slug lookup and
  duplicate-target search pass an explicit `limit=500`. MCP `hotsheet_query` forwards
  `page_size` and `cursor` for checkout queries and returns the page envelope. Store-level
  `/tickets` reads without a checkout keep `limit` + `page_after`.

  **Shared merge, with or without a server (HS2-JVF20F).** The batched k-way merge, the
  `v2.` cursor codec, the filter fingerprint, and the `{items, next_cursor, counts}` envelope
  live in `hotsheet_ticketing::checkout_page`; each caller supplies only a per-source fetch
  (a bounded batch strictly after a `MergeKey`). The server fetches from its SQLite indexes
  and provider adapters; the serverless MCP backend (`CoreBackend`) fetches by file scan
  with `TicketQuery::after_key`, re-sorting each scan by the shared comparator before
  cutting the batch, and computes counts from each store's summary. Serverless pages
  therefore apply the same 1–500 `page_size` bound, the same staleness rules, and the same
  zero-filled seven-day `completion_trend`. Source keys are `git:{connection id}` and
  `provider:{connection id}`, so a cursor started on one side continues on the other
  whenever the checkout's source set matches. Both sides choose that set with the shared
  `checkout_page::merged_external_sources` rule (enabled non-git links, none for
  `source=git`). The serverless backend cannot read hosted providers (they need the
  trusted credential broker of `docs/16-external-sync-interface.md` §16.8.1, HS2-C1E8Q4),
  so it keeps each hosted source's `provider:{connection id}` key, reads it as exhausted,
  names it in `source_errors` as `{connection id}: <reason>` with a fixed reason that a
  running Hot Sheet server is needed, and counts only its git stores, exactly as
  the server reports a source whose read failed. Its cursors therefore continue on the
  server and the reverse, with the same partial-cursor caveat as any failed source; an
  external-only checkout returns no rows and names every omitted source rather than
  failing (HS2-DX78E7). Rows keep their `store` field: the hosted store id on the server,
  the store path serverlessly.

- **Continuation under concurrent mutation (decided HS2-ZYW6K8; built HS2-74H84S):**
  checkout cursors are _value-keyset continuations_, not snapshots. The server keeps no
  per-cursor snapshot or retained result set, so memory stays bounded and cursors stay
  stateless across restarts and hosts. Instead, a continuation resumes strictly after the
  last emitted row's **sort-key values** in the shared `checkout_order` total order, and
  it guarantees:
  - every row that exists with an unchanged sort key across the whole traversal is
    emitted exactly once, whatever else is inserted, edited, or purged;
  - a row inserted, or whose sort key changes, during traversal appears only if its
    current key sorts after the cursor, so it may appear twice or not at all; clients
    de-duplicate by `qualified_id` and rely on live WebSocket updates to reconcile edits;
  - editing or purging the boundary row never ends or rewinds a source;
  - `counts` describe the current request, not the traversal's start;
  - a cursor is rejected (`400 stale checkout ticket cursor`) when the source set, sort,
    direction, or effective filter set changed. The filter binding is a canonical
    fingerprint of the parsed filters (after `me` resolution, with set-valued filters such
    as `tags` sorted and de-duplicated, and paging/ordering parameters excluded), so an
    equivalent query in a different parameter order continues (HS2-Z1TQ7Z, **built**).
    Store writes never stale a cursor. The web client keeps every cursor paired with the
    exact query that issued it: a warm project switch restores the pair, board columns
    continue with the base query they were loaded with, and clearing a search drops the
    search cursor. If the server still rejects a cursor as stale, Load more reloads page
    one of the visible list or board instead of showing an error (HS2-38MPMC, **built**).

  How sources resume (HS2-74H84S):
  - **Local stores** use `TicketQuery::after_key`, an exact SQL predicate that mirrors
    `checkout_order::compare` term by term, including the `{connection}:{id}` qualified-id
    tiebreaker against keys from other sources. Title order folds ASCII case only, matching
    SQLite's built-in `lower()`, so SQL keysets and the in-memory merge agree byte for byte.
  - **Providers** implement `TicketProvider::query_after(query, after, resume, limit)`. The
    default reads the full filtered result and keeps rows strictly after the key. GitHub and
    GitLab walk native pages for ascending `created` order, and Jira for ascending
    `created`/`updated`; they resume from the hinted native page, drop rows at or before the
    key, and restart from the first page when the hinted page is gone or already starts
    after the key (earlier deletions shifted rows back). Their `id` order is numeric
    natively but string-ordered in the checkout comparator, so `id` and every other sort
    use the full-scan keyset.
  - A pre-value-keyset `v1.` cursor is rejected as stale; the client restarts from the top.

  The `needs_review=true|false` query filter uses the normalized review state before
  a page is cut: any unresolved independently linked `feedback_needed` request or
  legacy sequential exchange (including the case-sensitive
  `FEEDBACK NEEDED` description marker) or an explicit review request. Local indexes
  combine their feedback flag with the reviews facet; provider adapters apply the
  same predicate while walking native pages. The row's `needs_review` boolean carries
  that presentation state separately from `feedback_needed`, which describes the
  feedback exchange alone (HS2-5B9SWQ).

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

The worklist files HS1 generated for AI tools remain a derived contract, not a second
source of truth. HS2 writes one machine-local `<checkout>/.hotsheet2/worklist.md` per code
checkout, aggregated from every git ticket store configured for that checkout; it never
writes the projection into a ticket repository. CLI mutations refresh affected local
checkouts synchronously from an index-bounded Up Next projection (then read only those
result ticket files), project open/registration seeds it, and server watchers coalesce
external/git changes. A registered checkout whose root folder no longer exists is skipped
rather than recreated (HS2-HM0Y6X). Registered and ad-hoc project projections always carry the explicit
checkout/root into settings resolution, so two worklists backed by one shared store can
apply different project guidance; the legacy store-only generator remains an explicit
compatibility API. The Up Next section includes only `not_started`/`started` tickets
even when projecting stale legacy data; other open tickets are deliberately omitted so the
file remains a focused executable queue. Concise status/note/follow-up instructions are
embedded so these files remain useful to an AI tool without the skill or API. See
[05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.9.
When an Up Next ticket has media annotations, indented worklist lines identify the
attachment, annotation id, shape, and explicit intents (or the shape's default).
The full Markdown note remains in the ticket; worklist lines escape user-supplied
filenames and intent strings as inert text.

## 3.7 Alternatives considered

| Option                                       | Verdict                                                                                                                                                                                                                            |
| -------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **SQLite + FTS5** (recommended)              | One store for structured + text queries; embedded; great bindings; transactional; disposable. **Chosen.**                                                                                                                          |
| Tantivy (Rust) / Bleve (Go) full-text engine | Excellent search, but structured queries + facets still need a second store to join against. Adds a system. Keep as a _later_ option if FTS5 relevance proves weak — layer it for search only, SQLite still holds structured data. |
| In-memory index only                         | Fast, but slow cold start on large stores and nothing to page against; lost on restart. Rejected as the primary index (we may keep hot subsets in memory as a cache above SQLite).                                                 |
| DuckDB                                       | Analytics-oriented; overkill for OLTP-ish ticket queries.                                                                                                                                                                          |
| Re-embed Postgres (PGLite/real PG)           | Reintroduces exactly the opaque-blob fragility the rewrite is removing. Rejected.                                                                                                                                                  |

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
