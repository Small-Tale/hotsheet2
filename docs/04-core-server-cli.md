# 04. Core, Server & CLI

> **Status: Proposed.** The shared-core-in-Rust recommendation is the pivotal
> language decision — see [09-technology-decisions.md](09-technology-decisions.md)
> §9.2 and the maintainer-confirmation note there.

## 4.1 The shared core

`hotsheet-core` is a single library holding **all** domain logic, written once and
used by the **two Rust binaries** — the server and the CLI. Clients do **not**
link it; they are API consumers (§4.1 "Why a library"). This still avoids HS1's
problem of logic re-implemented per surface: there is exactly one implementation
of every operation, and the server (the only thing GUIs talk to) is the single
authority over it. The CLI shares it directly for its direct-to-disk path.

The core is **I/O-capable but policy-free**: it performs file and process I/O
through injected adapters, so hosts inject real implementations and tests inject
fakes. Nothing in the core decides _where_ to bind a port or _whether_ to open a
browser — those are host policy.

Core modules (mirrors [01-architecture.md](01-architecture.md) §1.2):
`model`, `providers` (normalized contract, routing/capabilities, default git-backed
implementation), `index` (SQLite/FTS5), `watch`, `query`, `plugins` (AI-tool
**plugin loader + registry**, setup/instruction/skill/
MCP writers, terminal manager, permission bridge — [05](05-ai-tool-plugins.md) §5.1a,
§5.11), `settings` (shared/local project settings — §4.9), `coord` (claim/lease).

Because `plugins` and `settings` are in the core, **both binaries** drive them: the
CLI runs the one-shot half headless (`hotsheet setup`, `hotsheet plugin …`,
`hotsheet settings …`) with no server, and the server runs the same code for
client-driven flows and hosts the persistent half (terminals, drive, busy). This is
the §4.5 no-duplication rule applied to setup + settings, not just ticket ops — and
it's the reversal of HS1, where the _app layer_ owned tool setup and project config.

### Why a library, not just a server

So the **CLI can operate directly on disk with no server running** (a chartered
requirement — "a cli for direct reading/manipulation of tickets on disk"), running
the exact same tested engine the server runs. The server and CLI are two thin Rust
binaries over one core; a `hotsheet new …` from the terminal and a create from the
GUI go through identical logic, and a running server's watcher reconciles whatever
the CLI wrote. The library boundary is what guarantees those two paths can't
drift.

> **Clients do not embed the core** (maintainer decision, 2026-08-19). Earlier
> drafts had Tauri/SwiftUI run the core in-process for local projects; that is
> **retired**. The server is always a separate process, even locally (§4.3), so
> the client/service split is absolute and the server can outlive the client.
> `uniffi` Swift/JNI bindings are therefore **not needed** — clients speak HTTP/WS
> like any API consumer. See [06-clients.md](06-clients.md).

## 4.2 Concepts carried over from HS1 (worth keeping)

From the current codebase, these are proven and carry forward — reimplemented on
the git+index foundation:

- **Zod-style wire SSOT** → a single schema module defining wire types + validation,
  shared across surfaces. (Rust: `serde` + a schema crate; generated TS/Swift types
  for clients.)
- **Data-driven AI-tool plugin registry with a purity boundary** — the cleanest
  abstraction in HS1; carried wholesale. [05-ai-tool-plugins.md](05-ai-tool-plugins.md).
- **Claim/lease ticket primitive** orthogonal to status/up_next, with a
  conflict-guarded write chokepoint, lazy reclaim, poison quarantine, and append-only
  `claim_history` telemetry on full ticket responses.
- **PTY broker as a detached process** so terminals survive a server restart.
- **Byte-stream busy inference** (spinner detection) rather than tool-specific
  busy APIs.
- **Worklist-as-file** contract + MCP tools proxying the same core.
- **WS sync + long-poll fallback** for live updates.
- **Tiered auth**: loopback secret → trusted-origin → mTLS for exposed binds.

Dropped: the PGLite cluster and its whole crash-recovery apparatus (snapshots,
backups, repair, cluster eviction, lock recycling-PID guards) — obsoleted by
git + a rebuildable index ([02-ticket-storage.md](02-ticket-storage.md) §2.9).

## 4.3 The server (`hotsheet-server`)

### Checkout identity and discovery

A checkout is Hot Sheet's machine-local identity for one canonical working-directory
path; it is not a git object, ticket-store identity, or credential. Its readable id is
`<folder>-<12 path-hash hex>`, so nearby projects can discover it without receiving a
secret. `${HOTSHEET_HOME}/checkouts.json` records optional repository identity and zero or
more ticket stores, permitting many-to-many mappings. CLI `checkout
register|list|resolve`, authenticated server `/checkouts` routes, MCP list/resolve tools,
and `setup` registration share this registry. Only server instance records contain a
bearer token; those files are user-readable only on Unix.

Registry mutations take an operating-system file lock across CLI and server processes,
then replace the JSON through a uniquely named, flushed temporary file. This preserves
every checkout when project opens overlap and prevents one writer from continuing through
another writer's renamed temporary file. On read, Hot Sheet can recognize the older
writer's narrow corruption signature—a complete registry followed by an exact duplicated
suffix—back up the original bytes beside the registry as
`checkouts.json.corrupt-<ULID>`, and restore the complete document automatically. Other
malformed trailing data remains an explicit error rather than being discarded.

Checkout-qualified `/checkouts/{id}/tickets` routes aggregate reads across every linked
hosted store. Get/update/close require the ticket to resolve uniquely; create selects the
only linked store or requires an explicit store id when several are linked. MCP ticket
tools accept the same optional `checkout` target (and `store` for ambiguous creates) in
both HTTP and serverless modes. Every checkout-qualified list, full-ticket read, and
mutation response resolves standing auto-context from that checkout's project settings;
one ticket store shared by several checkouts never supplies an implicit project identity.
Detail reads retain the ticket fetched during source resolution, including external-provider
payloads and historical connection aliases; they do not fetch it again after finding its
owner. Synchronous resolution runs on the blocking pool. Duplicate backlinks use each Git
store's maintained reverse index for the exact qualified target plus historical bare-ULID
references. A remembered but unhosted source is indexed and watched once on first use, with
source-specific initialization coordination; no global registry lock spans its disk scan.
Only indexed matches are read to validate current source content, so a retained healthy
cache row does not expose a now-corrupt, removed, or retargeted backlink.
Writes update the reverse index immediately, while CLI/Git changes use the existing watcher
and restart reconciliation. Missing checkout directories and absent store metadata are
skipped, and inaccessible sources remain reported. External providers retain a live-query
fallback until the provider-native reverse lookup tracked by HS2-A0PDBP exists (HS2-Y7W3Z4).
Terminal-open preparation also runs on the blocking pool, including plugin setup and
model/effort discovery; a slow catalog lock or discovery subprocess cannot occupy an
async worker needed by another client's detail read.
`GET /providers` is built from each hosted store's id, root, and metadata prefix only; it
never opens a ticket file, so its cost does not grow with ticket count. The ticket counts
in `GET /stores` (and the single-store count returned by `POST /stores`) parse ticket files
on the blocking pool, so a large store cannot stall `/health` or other requests. `POST
/stores` also opens and indexes a newly hosted store on the blocking pool, so registering a
large store never occupies an async request thread (HS2-GM4FR2). `POST /projects/open`
(store discovery plus hosting each linked store) and `PUT
/checkouts/{reference}/sources/{connection_id}` (hosting a newly linked store) do the same
work on the blocking pool with unchanged status codes and responses (HS2-2VBN8Y). `POST
/setup/{tool}` (instruction, MCP, and permission file writes plus any tool probe) also runs
on the blocking pool with unchanged status codes and report shape, then invalidates the
AI-tool discovery memo (HS2-9TV33W). Background
sync, Trash-purge, and distributed-work loops enumerate hosted roots without parsing
tickets (HS2-4XXRJP).
Unqualified and `/stores/{id}` routes remain explicitly store-only compatibility APIs.
Browser collection reads opt into a bounded envelope with `page_size=1..500` and resume
with the opaque `next_cursor`; the response contains `items`, `next_cursor`, and SQL-backed
project `counts`. When the client supplies eight local-day boundaries, those counts also
include the exact seven-day completion trend across every linked source, independent of the
selected collection or retained page. The cursor walks linked stores without concatenating
their complete row sets. GitHub, GitLab, and Jira adapters return their own opaque native
page cursors and aggregate summaries page by page, so the host never materializes a complete
external-provider result merely to slice it or calculate navigation counts. The aggregate
counts keep Queue/Backlog/Archive and project-tab summaries exact.
Paged clients may pass `collection=queue|archive|trash|all` for the built-in multi-status
collections. `all` is every collection except Trash: Queue, Backlog, and Archive, with moved
tombstones included as in Archive. The web client's All Tickets search uses it (HS2-4J6764).
The lifecycle predicate is applied by each source index before the page cap, so a mixed-status
first page cannot make the browser show fewer rows than the selected collection contains.
Omitting `page_size` preserves the legacy array response for existing integrations.

Checkout corrupt diagnostics also expose a safe repair-ticket action. It revalidates the
reported path, routes the generated work item to the affected store, and returns the
existing open repair item on repeated requests. The API schedules recoverable work for an
AI worker; it does not synchronously rewrite or delete an unreadable source file.
`GET /checkouts/{reference}/corrupt-tickets` runs on the blocking pool and never stalls
other requests (HS2-QV8B7R). Each hosted store keeps a stat-validated corrupt-ticket cache
(`hotsheet_ticketing::CorruptTicketCache`, HS2-KYSBT2), prewarmed in the background when
the store is hosted. A request lists and `stat`s the ticket tree and re-reads only files
whose fingerprint changed: size, modification time, inode, change time, and the ticket's
attachment directory. A file modified within 2 s of being observed is re-read on the next
request, and read failures other than a parse error are never cached. The answer therefore
always matches the disk, including edits made outside the server, with the same wire
shape, so the web client keeps awaiting it before painting filtered rows. It does not
reuse the `/health` scan, which covers only the primary store and may answer from a stale
listing.

`POST /projects/open` is the client onboarding transaction: it validates a code checkout,
accepts explicit git-store paths or discovers a valid `.hotsheet2/store` checkout link,
falling back to an exact sibling `<checkout>.hs2` store,
hosts those stores, and registers the checkout links. Discovery only accepts a directory
containing `hotsheet-store.json`; it never creates a store or assumes a checkout has only
one source. An empty discovered source set remains valid at the core layer so a richer
client can present provider setup. Checkout ticket enumeration is also valid for that
empty source set and returns an empty array; it must not turn a successful project-open
transaction into a later conflict while the client presents source setup.
Reopening a registered checkout preserves its linked external ticket sources, selected
default, alias, and repository metadata. A newly selected sole git store replaces the
prior sole git link without removing external sources; conservative discovery only adds
sources. An explicitly supplied complete `sources` list remains authoritative. Source
removal and default changes use their dedicated checkout operations.
Clearing a checkout's default records that choice in the machine-local registry, so a
reload or source update does not select its sole remaining source again. Older registry
entries without an explicit-clear marker still infer a default from their legacy store
link or sole source (HS2-QC87XT).
The CLI's `setup --refresh`, repeated `bootstrap`, `link`, and repeat `checkout register`
use durable open/relink behavior, so the client setup refresh cannot erase an external
source before the server receives `/projects/open`.
The response path does not regenerate the checkout's local worklist. That projection runs
shortly afterward as best-effort blocking work over the already indexed active Up Next
rows, giving the client's initial ticket-index requests priority without rescanning every
ticket file. Server-owned writes queue the same bounded projection on one coalescing
background refresher instead of regenerating it inside the request (HS2-Z99TJN, about 19 ms
per write on the HS2 store in a debug build). After a 50 ms debounce the refresher
regenerates each affected store's checkouts once; a burst of writes shares one pass. A
write that arrives while a pass runs gets one more pass afterwards, so the worklist ends
on the last write's state. Every regeneration (writes, watcher batches, and the
project-open refresh) runs on that one worker, so two never overlap and an older
regeneration cannot overwrite a newer one. A graceful shutdown waits up to 2 s for the
queue to drain. Exact short-lived write hashes suppress the writes' watcher echoes.
Unmarked external filesystem changes are parsed into the index from their exact changed
paths before regenerating the same bounded projection.

**Hosting follows open projects (HS2-ARJ9J1).** Stores hosted because a project opened them
stay hosted only while they are in use. The primary store, stores listed in `stores.json`,
and stores added with `POST /stores` stay hosted for the life of the server. A client tags
each change stream, `GET /ws/poll` or `/ws/sync`, with `checkout=<id>` and `client=<tab id>`.
The web bridge adds `checkout`, since the browser only knows its project.
`/ws/sync` also accepts repeated `checkout=` values, so one socket can hold presence for
several checkouts in the same tab. Each value has its own lease and explicit close; a
single-checkout URL remains valid for older bridges. A socket with no nonempty checkout
still pins all hosted stores for version skew.

- **Leases:** a lease stays live while a socket is open or a poll is in flight, and for 60 s
  after, which covers the gap between long-polls.
- **Explicit close:** a client closing a project calls `POST /checkouts/{reference}/close?client=<id>`.
  That drops its lease immediately and triggers an unhost sweep; an unknown checkout returns 404.
- **What the sweep keeps:** it unhosts a project store only when no live lease names a checkout
  that references it, and nothing else needs it. Live needs are a drive on the store, a live
  terminal whose working directory lies inside such a checkout, or an unexpired claim in the
  store. A terminal's working directory is the one its shell last reported with OSC 7, or its
  launch directory until then, so a program that never reports one still counts (HS2-R5KV1Q).
  A terminal opened without a `cwd`, in-process or in the broker, launches in the primary
  store's root. Both paths are compared in canonical form.
- **Grace period:** without an explicit close, the sweep runs 10 minutes after the leases lapse.
- **Unhosting** drops the store's index, watcher, discovery file and writer lock, and stops the
  repository monitor of any checkout nobody has open.
- **Pinning:** a change stream that names no checkout, from an older client, pins every store.
- **Re-hosting:** a lease on a checkout, a checkout-scoped request, or a new `POST /projects/open`
  hosts the store again, so a tab that slept past the grace period keeps working.

### Headless platform APIs

The authenticated server exposes checkout-scoped repository snapshots at
`/checkouts/{id}/repository/status`, with bounded cursor pages for working-tree files
and commits at the sibling `/repository/files` and `/repository/commits` endpoints,
current ticket-flow aggregates at
`/analytics/tickets`, and settled-plus-live usage totals at `/analytics/usage`.

**Repository size (HS2-QF44T7).** `GET /checkouts/{id}/repository/size` measures the
checkout's code repository and every linked ticket source. The CLI twin,
`hotsheet checkout repo-size [reference] [--json]`, calls the same function in
`hotsheet-ticketing::repository_size`, prints the same JSON shape, and defaults the
reference to the current directory. The read-only MCP tool `hotsheet_repository_size`
(required `checkout` id, alias, or path) returns that same JSON, serverless or through the
server endpoint (HS2-R0131R).

- **Git objects:** loose plus packed bytes from `git count-objects -v`. This is what a push
  to GitHub carries.
- **Attachments:** for each git ticket store, the on-disk `attachments/` payload. The walk
  stops after 200,000 directory entries and reports `truncated: true`, so `bytes` becomes a
  lower bound.
- **Levels:** `ok`, `warning` at 5 GB or more (GitHub's soft limit), and `critical` at
  10 GB or more (its hard limit). The thresholds are named constants (`WARNING_BYTES`,
  `CRITICAL_BYTES`) echoed in the response. Each entry carries its own level, and the
  report carries the highest one. Clients display levels; they never recompute them.

**Attachment compaction (HS2-T2Q7KM).** Project statistics offers a read-only preview for
each dedicated Git ticket store. `GET /checkouts/{id}/repository/compaction?source=<connection-id>`
returns the same bounded plan as the headless CLI:

```sh
hotsheet checkout compact <connection-id> <project> --min-age-days 90 --oversized-bytes 10000000 --json
```

The plan lists duplicate and oversized payloads only on completed, verified, or archived
tickets older than the chosen age. It shows each filename, ticket, size, SHA-256, Git path,
and reason. Its fingerprint binds those choices to the current branch head. The default
threshold is 10 MB. The UI shows a short preview; use CLI JSON to review the full list.

Execution is deliberately an explicit headless operation. Install `git-filter-repo`, choose
a new backup path outside the store, and copy the exact confirmation printed by the CLI:

```sh
hotsheet checkout compact <connection-id> <project> --min-age-days 90 --oversized-bytes 10000000 --execute --backup <new-backup.git> --confirm 'rewrite <branch> <fingerprint>'
```

The engine rejects shared code/ticket repositories, dirty stores, stale previews, multiple
branches or tags, an unpushed or changed `origin` head, an existing backup destination, and
missing `git-filter-repo`. It makes a bare mirror backup, writes an audit note with each
removed attachment's filename, size, digest, and reason in a disposable clone, filters the
selected payload paths from history, then pushes with an exact remote-head lease. The audit
note also retains each attachment's original purpose, provenance, and annotation metadata.
The original checkout is deliberately left untouched and stale. Preserve its local work, then
re-clone or reset it to the rewritten branch; every collaborator must do the same before
another ticket write or push. Keep the backup until recovery is confirmed. A working-tree
attachment deletion alone cannot reclaim old Git objects.

For future images and other large media, install Git LFS in every collaborator's ticket
store environment, run `git lfs install`, then `git lfs track '*.png' '*.jpg' '*.webp' '*.mp4'`
inside the dedicated ticket repository and commit `.gitattributes` before new attachments.
Coordinate LFS availability with the hosting provider and every automated writer; tracking
new files does not migrate old objects, so use the compaction plan for existing history.

- **Explicit non-measurement:** a store kept inside the code repository is flagged
  `shared_with_code`, since its git size is the code repository's. A non-repository directory
  reports `not_repository`, a missing path `unavailable`, and a non-git provider source
  (GitHub Issues, Jira, ...) `unsupported`; each carries a `detail` sentence.
- **Cost:** the server measures on the blocking pool. Nothing polls it. The web client
  requests it once when a project is first shown (for the shell warning) and once each time
  the statistics view opens.
- **Proactive warnings (HS2-P1GYWH):**
  - `hotsheet doctor` prints a `Repository size:` line for the registered checkout of
    `--project` (default `.`), or for the store alone when no checkout is registered. Each
    repository over a limit gets its own line. Over the 10 GB hard limit is an issue (`  !`,
    nonzero exit) because GitHub rejects the push. Over the 5 GB soft limit is a
    `  warning:` that does not fail doctor.
  - `hotsheet sync` measures the store's git objects (one `git count-objects`, no attachment
    walk) before and after a sync that pulled or pushed. It prints a `warning:` to stderr
    only when the level rose across a threshold, so a repository already over a limit is not
    re-announced on every sync.
  - The server's background sync loop does the same for every hosted store (HS2-KKFEY5).
    It records each store's level when it first sees the store, measures again after
    every pass that reached the remote (`Synced`, or `UpToDate` after the store's own
    best-effort autocommit push already published the growth), and keeps the last level per store in memory. When the level
    rises, it logs a warning and publishes one replayable `repository_size` change event
    per checkout linking the store (`id` = checkout id, `slug` = the new level,
    `message` = the warning). A drop lowers the remembered level, so a later rise is
    announced again.
  - The web project shell shows a persistent warning strip while any repository is over a
    limit (see [06](06-clients.md) "Repository size").

Historical cumulative-flow data is explicitly reported unavailable because current ticket
files do not preserve status transitions.
The CLI exposes the same live feed through the selected `-C` store's running server:

```sh
hotsheet ticket-flow
hotsheet activity --ticket HS-7f3k9q --min-importance high --limit 20
hotsheet notifications list --checkout web --recipient reviewer@example.com
hotsheet notifications ack <notification-id>
```

All four print JSON and use the server's loopback secret. Activity accepts `--session`;
notifications can filter by `--checkout`, `--store`, `--ticket`, and `--recipient`.
An absent server fails with a start-server instruction. The ticket-flow summary covers
the server's selected store; activity uses its persisted rolling window, and
notifications use its bounded server-process feed. Acknowledgement uses the same id and
authorization as the web client.

Checkout-scoped ticket usage is exposed at
`GET /checkouts/{checkout}/tickets/{ticket}/usage` (HS2-ZA7W6E, HS2-J14225): the ticket's
attributed AI usage events in time order plus the settled rollup's per-ticket cost and
rolled-up-through day from the owning git store (`docs/14` §14.5). A ticket on another
provider yields an empty history; an unknown ticket is `404`.
Native Claude terminal hooks report prompt and turn-end boundaries to the authenticated
`POST /terminals/{id}/claude-turn` route (HS2-W2GNTT). The route accepts only the
currently connected Claude session, reads appended numeric transcript usage on turn
end, and records the unique live ticket claim captured at prompt time (`docs/14` §14.5).
Transient read or metrics-write errors can be retried by the same turn-end hook
without counting the turn twice (HS2-F8578J).

Checkout-scoped code review is exposed at
`GET /checkouts/{checkout}/tickets/{ticket}/code-review`. Discovery walks at most the
newest 2,000 reachable commits, returns each commit's subject and full message body, and
associates a commit when the ticket slug is a bounded token in its subject or an explicit
`Refs:`/`Refs …` line containing one or more ticket slugs. Ordinary body mentions remain
cross-references, not ownership.
Ticket discovery also makes a second bounded scan of commits on local branches and other
worktrees' detached `HEAD`s that the checkout's `HEAD` does not contain (worktrees share
one ref namespace), so ticket work done in another worktree is visible before it is merged
(HS2-0WZRK5). Each such commit carries `unmerged: { branch?, worktree? }` naming the local
branch and the worktree directory that has it checked out; unmerged commits are listed
first. Their ranges join only parent-linked commits from the same source, and the
changed-files summary stays on the merged commits when any exist (otherwise the newest
unmerged source's run). Launch validation accepts them exactly like merged commits. The
ticket-list commit indicator counts unmerged work too; the repository browser stays on
`HEAD` history.
Adjacent matching commits form reviewable ranges without spanning unrelated work. The
response also reports the checkout's Git `diff.tool`. The matching `POST` accepts only a
single commit or adjacent range returned by a fresh discovery, rejects arbitrary refs,
and starts `git difftool --no-prompt` with an argument array in the registered checkout.
The repository-review endpoint additionally accepts two distinct, exact commits from a
fresh repository discovery for an A/B comparison. Repository commits selected from any
page are revalidated as full reachable commit IDs before launch; the ahead-range target
must still exactly match current repository metadata. The client never receives a
program path or executes a review command itself.

Project command settings use exact `program` + `args` arrays. `/commands` only starts a
configured id; requests cannot supply arbitrary shell text. Bounded run history includes
cursor-addressable stdout/stderr lines and cancellation. `/notifications` persists a
bounded server-process routing feed with checkout/store/ticket targets, deduplication, and
acknowledgement while also publishing live events. `/tts/synthesize` accepts text,
provider id, and voice only; provider adapters and their credential resolution remain in
the server process.

#### Route reference: runs, views, lifecycle, bridge probe, announce

All routes below sit behind the loopback secret (`X-Hotsheet-Secret`) like the rest of
the authenticated API. Unscoped forms act on the server's primary store; the
`/checkouts/{reference}/…` forms act on a registered checkout and return 404/400 for an
unknown reference. Provider and GitHub sign-in routes are listed in
[16-external-sync-interface.md](16-external-sync-interface.md) §16.8.

| Method | Path                                                                           | Request                                                                 | Response                                                                                                                                                                                             |
| ------ | ------------------------------------------------------------------------------ | ----------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET`  | `/command-runs`, `/checkouts/{reference}/command-runs`                         | —                                                                       | `200` array of command runs (bounded history; checkout form lists only that checkout's runs)                                                                                                         |
| `GET`  | `/command-runs/{id}`, `/checkouts/{reference}/command-runs/{id}`               | query `after=<line cursor>` (default `0`)                               | `200` the run with stdout/stderr lines after the cursor; `404` `unknown command run`                                                                                                                 |
| `POST` | `/command-runs/{id}/cancel`, `/checkouts/{reference}/command-runs/{id}/cancel` | —                                                                       | `200` the cancelled run; `409` when it is not cancellable (already finished/unknown)                                                                                                                 |
| `GET`  | `/views`, `/checkouts/{reference}/views`                                       | —                                                                       | `200` array of custom views from settings                                                                                                                                                            |
| `PUT`  | `/views`, `/checkouts/{reference}/views`                                       | JSON array of custom views (replaces the whole list)                    | `200` the saved array; `400` on validation failure. Emits a `views_updated` event over `/ws/sync` and long-poll                                                                                      |
| `GET`  | `/lifecycle/quiescence`                                                        | —                                                                       | `200` `{quiescing, report}`; `report.quiescent` says whether all server-owned work is idle                                                                                                           |
| `POST` | `/lifecycle/restart`                                                           | —                                                                       | `202` `{restarting: true}` and the server shuts down for its supervisor to restart; `409` `{error, quiescence}` when work is active (nothing is interrupted) or a restart is already being evaluated |
| `GET`  | `/permissions/bridge-probe`                                                    | —                                                                       | `200` `{bridge_reachable: true}`; `503` while stopping. Read-only: it never announces a session or changes a terminal's AI connection state                                                          |
| `POST` | `/announce`                                                                    | `{message, store?}` (`store` = store URL id, default the primary store) | `204`; `400` for an empty message. Broadcast to live `/ws/sync` subscribers only — not persisted or replayed                                                                                         |

Command runs are started with `POST /commands/{id}/run` (or the checkout form) as
described above; restart semantics are in §4.3.1.

> **Status: v1 built (HS2-7).** `crates/hotsheet-server` — axum HTTP REST
> (`/health`, `/tickets` list/create, `/tickets/{id}` get/patch,
> `/tickets/{id}/close`) + `/ws/sync` live push, over the shared engine
> `ops`, with **Tier 0** auth (`X-Hotsheet-Secret`) on loopback and **Tier 1 mTLS**
> (per-project CA + per-device client certs, HS2-VT3JMF) on off-loopback binds — see
> §4.6. It serves reads from the **SQLite/FTS index** (HS2-5)
> and owns the **filesystem watcher** (HS2-6) that reindexes changed files + emits
> change events, so a CLI/git edit shows up live. Terminal hosting, detached broker
> integration, lifecycle/discovery, and the long-poll fallback are also built. MCP is a
> separate shim, not served here (§5.8).

`GET /health` is intentionally unauthenticated and carries a non-secret protocol identity:
`generation: "hs2"`, `api_version`, ticket prefix, and store schema. An HS2 MCP HTTP
backend that receives 401/403 probes this marker so it can distinguish a bad HS2 secret
from an HS1/wrong-service endpoint instead of giving generic credential advice
(HS2-8H8BQM).

`/health` also reports the primary store's healthy `tickets` count and its unparseable
`corrupt` files (HS2-PRVPCQ). That listing parses every ticket file, so it never runs on
an async request thread and never makes the probe wait on a large or blocked store
(HS2-9PPDR1): it runs single-flight on the blocking pool (concurrent probes share the scan
in flight) and is awaited for at most 250 ms. The additive `listing` field says where the
counts came from: `"fresh"` (a scan finished within the budget), `"cached"` (the scan is
still running; these are the last completed scan's results), or `"index"` (no scan has
completed yet; `tickets` is the index's row count and `corrupt` is empty). Every other
field keeps its wire shape.

A thin binary that wraps the core and is the **always-on service** every GUI talks
to — **local use included**. It runs completely independently of any client (a
chartered goal, made absolute by the maintainer 2026-08-19: there is no
embedded-in-client mode). One instance per machine serves all local projects.

Responsibilities:

- **HTTP REST** for CRUD + query (JSON, not HTML — the client/service split).
- **WebSocket** (`/ws/sync`) for live push: index changes, claim/lease events,
  terminal streams, permission prompts. `GET /ws/poll` is the cursor/replay
  fallback for environments without WS; it accepts the loopback secret through
  either the native query form or `X-Hotsheet-Secret`, allowing the web bridge to
  authenticate without exposing credentials to browser JavaScript. The bridge uses
  both forms on its loopback-only hop so a newer client can still long-poll a running
  pre-header-auth server during an unsynchronized rollout. Watcher events enter the
  same replay ring as API mutations. Replayable events carry their monotonic cursor on
  both transports. A caught-up long poll blocks; after wake it snapshots the whole
  available span and its exact cursor atomically, so a burst cannot be skipped and an
  idle client cannot form a tight request loop.
  The browser's development bridge proxies a credential-free project-scoped
  `/ws/sync` URL to this authenticated loopback endpoint, keeping the server secret
  outside browser JavaScript. The web client handshakes and closes the WebSocket
  subscription race through the replay endpoint, consumes cursor-bearing push frames
  while connected, and replays from its last cursor before reconnecting with bounded
  backoff. Unsupported upgrades or older non-replayable event formats therefore fall
  back to long polling without losing external changes.
- **MCP** endpoint(s): the `hotsheet_*` tool surface for AI tools
  ([05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.8).
- **Owns the filesystem watcher** (→ incremental reindex) and the **terminal/PTY
  manager** (both need a persistent host).
- **Hosts client-owned AI connections.** `POST /drive/connections` prepares or attaches
  a project/tool connection through the shared `SafeTrigger`. Checkout drives bind to
  the checkout's explicit default ticket source (or a requested `source`); an ambiguous
  checkout must choose one, and providers without local activity persistence fail with an
  explicit capability error. Native activity, usage, and turn events are then recorded and
  identified against that linked source rather than the server's unrelated default store. `POST
/drive/connections/{id}/turns` sends a free-form turn and resumes the retained tool
  session; and `/interrupt` exists as an advertised connection action only when the
  concrete drive implements interruption. `GET /connections` reports state, session,
  errors, and semantic actions; `GET /drive/sessions` lists machine-local resumable
  sessions for the project. Busy/idle transitions emit replayable `drive_updated`
  events over WS/long-poll, so clients never poll connection state once per second
  (HS2-5DGFG2).
- **Tiered auth** (§4.6).

Stack (Rust recommendation): `axum` + `tokio`, `tower` middleware for auth/rate
limiting, `rusqlite` for the index, `notify` for watching, `portable-pty` for
terminals. Runs as a plain HTTP server on loopback by default; `--bind` off-box
flips it into the mTLS tier or refuses to start.

### 4.3.1 Lifecycle — independent, auto-started, survives the client

The server's life is **decoupled from any client's** (maintainer requirement,
2026-08-19):

- **Started explicitly** by `hotsheet serve`, **or auto-started by a client.** When
  a client (Tauri, SwiftUI macOS) launches and finds no server, it **spawns one
  detached** — not as a child tied to the app's lifecycle, but as a background
  process (double-fork / `setsid`, or a launchd/systemd user service) — then
  connects over HTTP/WS.
- **Survives the client closing.** Because it's detached, quitting the app leaves
  the server running (in-flight AI work, terminals, and the watcher keep going).
  This is the whole point of the separation and the direct reversal of HS1's
  Tauri-sidecar model, where the server died with the app.
- **Join-don't-collide.** A per-project lock (index writer) + a global instance
  file (`${HOTSHEET_HOME:-~/.hotsheet2}/instances/<project-id>.json`, holding the port) lets a second launch — a CLI,
  another client, a second app window — **discover and join** the running server
  instead of starting a duplicate. Carried over from HS1's instance model, minus
  the DB-lock recycling complexity (there's no DB cluster to protect, only the
  disposable index).
- **Supervised, not owned.** A client may _supervise_ the local server it started
  (restart it if it crashed, surface its health) but never _owns_ it — any client
  can attach to a server another client started, and none closing it takes it down.
- **Shutdown is explicit** — `hotsheet serve --stop` / a tray/menu "Quit server"
  action / OS service stop — never an implicit side effect of a client exiting. Server
  shutdown preserves broker-hosted terminals by default; the project-scoped destructive
  variant is `hotsheet serve --stop --kill-all-terminals`. A server launched by the pinned
  production host (`launcher` = `local-host-snapshot`) is refused by an ordinary stop and
  needs `--force` (HS2-9AWEX5).
- **On iOS** there is no local server to auto-start (background-execution limits
  make an independent daemon impractical); iOS connects to a _remote_ server on a
  Mac. See [06-clients.md](06-clients.md) §6.4.

**Built (HS2-59, server-side; HS2-5A01DC/HS2-4072GM, CLI and client supervision):**
`hotsheet-server::lifecycle` implements the
machine-local **instance registry** (`${HOTSHEET_HOME:-~/.hotsheet2}/instances/
<project-id>.json` — not `~/.hotsheet`, which HS1 owns, HS2-104), **discovery**
(`find_instance`, validating the recorded pid is alive so a crash leaves no false
positive), the **per-store index-writer lock** (a second server is refused / told to
attach; a stale lock from a dead server is reclaimed). Taking, reclaiming, and releasing
that lock is atomic across processes (HS2-585JCP): each step runs under an OS advisory
lock on a persistent `<project-id>.lock.mutex` file. Lock contents are published by
temp-file rename, so a reader never sees an empty file. A fresh empty or garbage lock
file counts as another server starting, and goes stale after 5 s. A server drops only
a lock that still names its own pid. So simultaneous starts leave exactly one registered
server, and every loser exits. Then **stop** (`hotsheet-server
--stop` → SIGTERM). `serve` takes the lock, writes the instance file (removed by a
guard on **graceful shutdown**: SIGTERM/Ctrl-C), and if a live server already serves
the store it **prints how to attach and exits** instead of duplicating. E2E-verified.

The web host has a separate discovery record, `${HOTSHEET_HOME:-~/.hotsheet2}/client.json`,
containing its local `url`, `pid`, and start identity. It points to the browser client, while
the per-store instance records above point to the machine API server. External local tools
can combine that client URL with the ticket deep-link route in [06-clients.md](06-clients.md)
(HS2-RVSPQ9).

**Shutdown is bounded (HS2-W1KJR4).** The first SIGTERM/Ctrl-C (or an accepted quiescent
restart) enters a _stopping_ state. The listener stops accepting on both the plaintext
and mTLS tiers, and long waits end at once:

- a parked `POST /permissions/ask` answers `503`, so the permission hook falls back to the
  tool's native prompt, and its pending prompt is removed;
- an idle `GET /ws/poll` returns its normal empty reply.

Open connections then get a drain of 5 s (the hidden `--shutdown-drain-ms` flag changes it
for tests). The process force-exits when that deadline passes or a **second** stop signal
arrives. On every path it first releases each hosted store's instance file and
index-writer lock explicitly, so a stuck connection can no longer leave a dead URL
registered. After a clean drain, a watchdog thread also bounds any leftover blocking
background work. AI-tool discovery (the startup catalog warmup and `GET /ai-tools`) runs its
tool probes on a detached thread rather than Tokio's blocking pool, because a dropping runtime
waits for every blocking task: a stop that lands while an installed tool's `--version` or
model probe hangs exits promptly instead of waiting out the drain (HS2-NPBZJ9). Each such
probe is itself bounded (HS2-BH3M53). It runs in its own process group with a 5 s deadline
(the hidden `--ai-probe-timeout-ms` flag changes it for tests). A probe that misses the
deadline is killed with everything it spawned, and that tool falls back to its manifest catalog,
so a hung tool can no longer hold the shared catalog lock that `/ai-tools`, `/ai-settings`,
and terminal-launch validation wait on. A stop kills every in-flight probe and refuses new
ones, so no probe child outlives the server. The short-lived `codex app-server` that lists
Codex's models is a probe too (HS2-BJ7A59): it leads its own process group, is killed and
reaped when its listing returns or times out, at the probe deadline, or when the server stops,
while long-lived drive sessions keep their own lifecycle.

**Owned servers stop with their owner (HS2-VQ8ZWT).** A process that owns a server,
such as a test harness or the scale-stress script, passes the hidden
`--exit-on-stdin-eof` flag and keeps a **pipe** on the server's stdin. When the owner
dies for any reason, even SIGKILL, the kernel closes that pipe. A blocking reader
thread sees EOF and begins the same bounded stop described above. No polling is
involved.

The flag is safe even when the dead owner also held the server's stdout and stderr:
shutdown diagnostics never panic on a broken pipe, and the reader begins stopping
before it logs anything.

**Diagnostics (HS2-PD8NJ6).** Server stdout carries only the startup contract lines
(URL, `secret:`, `index:`, hosting summaries). Every other diagnostic is a `tracing`
event written to stderr by the binary's `fmt` subscriber, at `info` level by default;
set `RUST_LOG` (for example `RUST_LOG=debug` or `RUST_LOG=hotsheet_server=warn`) to
change the filter. Server events use a constant message plus structured fields
(`store`, `checkout`, `checkout_root`, `worker`, `terminal`, `ticket`, `error`) rather
than interpolated text, so logs can be filtered and parsed (HS2-79TVFH). Failures that are deliberately ignored carry a reason comment or
are logged rather than discarded with a bare `let _ =`.

Detached client launches never pass this flag, because they must outlive their
client. Stdin on `/dev/null` would read EOF immediately, so the flag requires a held
pipe.
`hotsheet-cli serve` resolves the sibling `hotsheet-server` first, falls back to PATH,
requires its version to match the CLI, and forwards foreground/stop arguments with
clear missing-binary and mismatch diagnostics. Detached broker terminal hosting is the
normal server mode, so a stop or restart does not end PTYs; `--kill-all-terminals` is
accepted only with `--stop` and clears the selected project's retained terminals through
the existing broker protocol. New detached sockets use a short private per-user `/tmp`
namespace keyed by configured home and project, independent of `HOTSHEET_HOME` length;
non-socket state remains in that home. Live owned legacy sockets are still discovered.
Socket ownership, stale-path handling, and cleanup are described in [05](05-ai-tool-plugins.md)
§5.4. `hotsheet-cli serve --list` works without
selecting a store: it deduplicates registrations by server identity, health-checks live
processes, labels stale/unhealthy/invalid records, lists every hosted store, and never
prints bearer secrets. Every registration a server writes also carries its `launcher`
label, `build_revision`, and `executable` path (HS2-GWFEAA): the binary takes
`--launcher <label>` (or `HOTSHEET_SERVER_LAUNCHER`), the CLI passes `cli`, and the web
bridge passes `local-host-snapshot`, `stable-dev`, or `dev-bridge` for the host kind that
started it; `GET /health` and `GET /compatibility` report the same `launcher` and
`executable`, so a maintainer can tell at a glance whether the pinned LAN host's server is
the one running. `serve --list` prints those fields per server and prunes registration
files whose recorded pid is dead (invalid records are kept for inspection), and
`hotsheet-server --stop` prints the launcher and build of the process it signals.
**Ownership-aware stop (HS2-9AWEX5).** `lifecycle::stop_decision` makes an explicit stop
refuse a server whose registration names the pinned launcher `local-host-snapshot` (the
production local host): `hotsheet-server --stop` and `hotsheet-cli serve --stop` exit non-zero
without signalling, print the owner's pid and build plus the deliberate alternatives, and skip
`--kill-all-terminals` cleanup. `--force` (which requires `--stop`) stops it anyway, for
deliberate relaunches such as `npm run prod:rebuild-lan`. Servers launched by `cli`,
`dev-bridge`, `stable-dev`, an unspecified launcher, or a legacy registration without a launcher
stop as before. The decision to isolate the production host this way rather than through a
second per-profile machine server, and the bridge-side half (dev hosts never restart a pinned
server; a pinned host takes ownership of one another host launched), are recorded in
[06](06-clients.md) under the local production host. The Vite platform bridge supplies the client-side half: detached
launch, health-checked discovery, and event-driven supervision after project-open, API,
or terminal-WebSocket failures. It never starts a duplicate beside a registered live
process; the server is already separate, so it outlives the client by construction.

Clients negotiate the protected `GET /compatibility` contract before relying on the
application API. It reports the HS2 generation, semantic application version, optional
build revision, inclusive API-protocol and store-schema ranges, server start time when
available, and lifecycle capabilities. Hard compatibility is based on protocol-range
intersection, not exact version or revision equality, so rolling compatible builds can
coexist. Local development builds hash the workspace Rust crate sources, manifests,
lockfile, and bundled plugin guidance at compile time and cheaply monitor them at runtime.
The binary's `--revision-status` command reports the same build/source status without
opening a store. The CLI and standalone migrator expose the same no-store probe so the
local host can verify each companion before using it for setup or migration. The
handshake reports the built and current source revisions plus
`source_stale`; the client can therefore detect a detached server built before a
dependency change without treating unrelated Git commits as staleness. Explicitly
revisioned release builds omit local source probing, and an
unavailable source tree is not reported as stale. The `--revision-status` probe also
reports `source_unavailable_reason` whenever `source_revision` is null (an explicit
build revision, or the source root's I/O error). A Hot Sheet terminal never inherits
the host's `HOT_SHEET_*` build pins, so a `cargo build` inside one hashes its own
checkout or worktree (HS2-6C7NQ7); neither does any non-PTY process launched through
`hotsheet_ticketing::git::launch` (AI tools, configured commands, the terminal broker,
external apps, `hotsheet-cli launch`; HS2-CGVHTM). Missing or invalid metadata is an
explicit unknown state. The server advertises authenticated restart and quiescence only
with its admission gate active. A restart first refuses new mutations/background passes,
then proceeds only when mutations, sync/drive passes, commands, setup refreshes, AI turns,
permission requests, authentication flows, and server-owned terminals are quiescent;
broker-hosted terminals may survive the process replacement. The client uses this path for
protocol or store-schema upgrades and local-host build replacement only when both
capabilities are explicit, waits for the
old registration to be relinquished, and then supervises the replacement. Ambiguous writes
are never replayed across recovery. Remote restart remains a separately authorized concern.
Headless parity (HS2-10T048): `hotsheet lifecycle status` prints the selected store's
running server quiescence report (`GET /lifecycle/quiescence`), and `hotsheet lifecycle
restart` sends the same authenticated `POST /lifecycle/restart`. A busy server refuses with
its blocker message and keeps its work, so the command exits non-zero and is safe to repeat;
an absent server fails explicitly. The CLI never stops the process itself. Saved command-run
cancellation is already headless through `hotsheet commands cancel <checkout> <run>`.
If a registered local process remains alive but cannot answer its health probe, automatic
supervision preserves it instead of launching a duplicate. The local client may offer an
explicit recovery action that rechecks the complete registered process identity, requests
graceful termination, and escalates to a forced stop only while that exact process remains
registered; it then supervises a healthy replacement. This action is never exposed by the
remote server API.

The headless CLI's `compatibility --json` response also includes
`setup_assets_fingerprint`, a SHA-256 digest of every embedded first-party plugin setup
asset. The source-backed web development bridge calculates the same digest from its live
`plugins/` tree before project-open refresh, bootstrap, or post-import setup. A missing or
mismatched digest stops before any setup mutation and tells the developer to run
`cargo build -p hotsheet-cli`; project open awaits this gate so the diagnostic is visible
instead of being discarded by a background refresh (HS2-EK0B1Z). The refusal never blocks
the project itself (HS2-0TXM8S): opening or restoring continues with the setup refresh
skipped, and the project carries `setupWarning`, shown as a dismissible **Project setup was
skipped** warning banner with the reason and fix. Bootstrap and post-import setup still stop,
because they would otherwise leave a project half set up.

The stable local hosts (`npm run {prod,dev}:rebuild-lan`) are deliberately pinned until the
user restarts them: the production host runs startup copies of its client bundle and
binaries (`HOTSHEET_LOCAL_HOST_SNAPSHOT=1`) and the stable dev server sets
`HOTSHEET_WEB_STABLE_DEV=1`. Template edits made in the checkout after such a host started are
expected drift, so a mismatched (but reported) digest silently skips the open-time refresh —
the pinned CLI would write older guidance — and no banner appears on any refresh or device;
the next deliberate rebuild/relaunch refreshes setup normally (HS2-A3C894, HS2-6VQ5RV).
Bootstrap and post-import setup still stop with a "restart the host" message. A missing
digest keeps the rebuild message.

Protocol ranges assume unsynchronized rollout. A non-intersecting range stops project API
use and identifies which side requires an update; exact build differences remain
informational. Store attachment is also negotiated before mutation: a server validates
metadata before registering a store, while `hotsheet-cli compatibility --json` reports
the schema the CLI creates and (with `-C`) the selected store's schema. A graphical
bootstrap compares those values with the active project's server range before creating
anything, so a newer CLI cannot leave an older detached server hosting a store it can
only fail to write later. Persisted-format compatibility is independently governed by
[19](19-format-compatibility.md): released readers permanently accept older released
ticket/store/project/settings fixtures, while newer incompatible markers produce a
specific upgrade-required result.

## 4.4 The CLI (`hotsheet-cli`)

A thin binary that wraps the same core for **direct-to-disk** operations, usable
with **or without** a running server. AI tools can use the CLI _or_ the MCP; humans
use it in a terminal.

The [CLI/service parity audit](24-cli-service-parity-audit.md) maps current
high-level workflows across the three interfaces and tracks identified gaps.

> **Binary name (dev):** the compiled binary is currently **`hotsheet-cli`**, not
> `hotsheet`, to avoid colliding on `PATH` with a separately installed Hot Sheet 1
> `hotsheet` launcher on developer machines. The examples below use the conceptual
> `hotsheet` name; substitute `hotsheet-cli` when running against this repo's build.
> (A final shipped name is a release-time decision.)

Two families of commands:

**Ticket ops (direct to disk; server not required):**

```
hotsheet new "Fix dashboard flicker" --category bug --priority high --up-next
hotsheet ls --up-next --status started
hotsheet ls --max-confidence 69 --sort confidence   # low-confidence completions first (HS2-RD4M29)
hotsheet confidence-report [--json]        # reported confidence vs. later reopen/verified outcomes per band
                                           # (also GET /confidence-report, /checkouts/{ref}/confidence-report, MCP hotsheet_confidence_report)
hotsheet phase-report [--since D] [--until D] [--json]  # time per started_phase: wall vs. live-claim time (HS2-JS7E61)
                                           # (also GET /phase-report, /checkouts/{ref}/phase-report, MCP hotsheet_phase_report; HS2-8N7QZ2)
hotsheet checkout repo-size [ref] [--json] # git object + attachment sizes vs. GitHub's 5 GB / 10 GB limits (HS2-QF44T7)
hotsheet ls --text "flicker"              # text search via the local index
hotsheet show HS-7f3k9q
hotsheet attach HS-7f3k9q ./proof.png       # stable id + RFC3339 created_at
hotsheet attach HS-7f3k9q ./proof.png --json # [{"id":"...","filename":"proof.png"}]
hotsheet annotate HS-7f3k9q proof.png --file annotations.json # replace media annotations
hotsheet annotate HS-7f3k9q <attachment-id> --clear        # remove annotations
hotsheet edit HS-7f3k9q --status completed --note "fixed the pre-theme paint"
printf '%s' "$MARKDOWN" | hotsheet edit HS-7f3k9q --note-file - --note-kind feedback_needed
hotsheet edit HS-7f3k9q --blocked-by HS-abc123 --blocked-by HS-def456   # set blockers (slug|ULID)
hotsheet edit HS-7f3k9q --clear-blocked-by                              # remove all blockers
hotsheet batch HS-7f3k9q HS-abc123 --priority high --note-file update.md # one update, per-ticket results
hotsheet delete-note HS-7f3k9q <note-ulid>                                # remove a git ticket note
hotsheet claim-next --worker worker-1                       # self-select + claim + start
hotsheet claim HS-7f3k9q --worker orchestrator-1 --label Codex # exact claim + start
hotsheet renew HS-7f3k9q --worker orchestrator-1             # extend active lease
hotsheet release HS-7f3k9q --worker orchestrator-1           # stop advertising work
hotsheet key set openai                    # hidden terminal prompt
printf '%s' "$PROVIDER_KEY" | hotsheet key set openai  # automation via stdin; never argv/settings
hotsheet key list                         # provider names only, never values
hotsheet providers --json                 # git + configured external connections/capabilities
hotsheet github-sign-in                    # print GitHub device URL/code, then save a keychain reference
hotsheet github-connect acme/repo --credential github-app-<id> --checkout <checkout-id> --default
hotsheet github-connect acme/repo --credential github-app-<id> --attachment-repo acme/assets  # enable attachments
hotsheet provider-connect gitlab acme/app --credential gitlab-key --checkout <checkout-id> --default
hotsheet provider-connect jira APP --credential jira-key --site https://acme.atlassian.net --email dev@example.com --checkout <checkout-id>
hotsheet provider-connect jira OPS --account jira-key --checkout <checkout-id>  # reuse a sign-in: credential, site, email
hotsheet provider-attach github-main 42 screenshot.png log.txt   # upload through the provider (one batch)
hotsheet provider-ls github-main
hotsheet provider-get github-main 42
hotsheet provider-new github-main "Bug title"
hotsheet provider-edit github-main 42 --expected-token <opaque> --status started
hotsheet provider-delete-note github-main 42 <note-id> # requires note_delete capability
hotsheet provider-assign github-main 42 --to octocat # replace assignees where supported
hotsheet provider-restore <git-connection> <ticket-id> # git-backed Trash only
hotsheet provider-report-not-working <git-connection> <ticket-id> --note-file report.md --evidence proof.png --expected-token <token>
hotsheet provider-close github-main 42 --reason completed
hotsheet provider-disable github-main           # temporary: no reads/writes, tickets hidden
hotsheet provider-enable github-main
hotsheet provider-remove github-main [--json]  # permanent, every project; the account stays signed in
hotsheet checkout remove-source web github-main [--json]  # this project only; deletes the connection if no other project uses it
hotsheet checkout source-color web github-main              # JSON: checkout_id, connection_id, effective color
hotsheet checkout set-source-color web github-main '#3b82f6' # set this project's source accent
hotsheet checkout set-source-color web github-main '#6b7280' # restore the default Gray mark
hotsheet checkout review-source web <git-source-id> [--store <path>] [--yes] # review/record a Git store identity; writes only with --yes
hotsheet checkout repo-size web [--json]  # code repo + each store: git objects, attachments, GitHub-limit level (HS2-QF44T7)
hotsheet account list [--json]            # machine-wide sign-ins, their sources, and the projects using each
hotsheet account backfill-logins [--limit 5] # resolve missing managed GitHub usernames in a bounded batch
hotsheet account sign-out github-app-<id> # refused while a ticket source still uses it
```

Ticket sources belong to projects and sign-ins are machine-wide accounts (HS2-SM9PM8, see
[16](16-external-sync-interface.md#project-owned-sources-machine-wide-accounts-hs2-sm9pm8)).
Source colors are also project-local: two checkouts linked to one connection may choose
different accents. Both color commands emit the same JSON object with `checkout_id`,
`connection_id`, and `color`; an unset color reads as Gray (`#6b7280`). The write command
accepts the same palette as the checkout color API and rejects an unlinked source or
unsupported value.
`checkout remove-source`, `account list`, and `account sign-out` run the same workflows as
Project Settings → Ticket sources and App Settings → Accounts; like the other provider
commands they read the `-C` store's `providers.json`.
Source removal holds the checkout registry lock while it scans remaining links and updates
`providers.json`. Explicit `checkout add-source` checks for its provider record under that same
lock, so a concurrent orphan removal cannot leave a new dangling project link (HS2-PNCKJR).
Project registration and reopen validate requested external links under the same lock;
reopening without a newly requested external source retains existing links (HS2-2SFDY4).
`account backfill-logins` is an explicit keychain and GitHub network action; plain listing
does not look up usernames. It tries up to five missing managed GitHub logins by default
(`--limit` accepts 1–10); `--after <credential-ref>` reaches later accounts even if earlier
ones repeatedly fail. It saves each successful `/user` login in `keys.json` and reports
expired or unreachable credentials without discarding other successes. Each refresh and
login HTTP request has a 30-second transport timeout; a failed batch exits nonzero.
Each checkout stores its own source icon colors in `checkouts.json`, keyed by connection id;
linked projects can give a shared connection different colors. The checkout provider
descriptors include the effective color, and `PATCH /checkouts/{reference}/sources/{connection_id}/color`
accepts the eight ticket-source palette colors (Blue, Green, Orange, Red, Purple, Pink, Teal,
and Gray). Gray removes the stored override; Transparent is not a ticket-source option
(HS2-H1FZNV). Removing or renaming a source removes or moves its project color with it.

`PATCH /checkouts/{reference}/sources/{connection_id}/relink` accepts
`{"path":"<new-ticket-store-folder>"}` for a moved Git ticket repository. The server
opens the destination as a Hot Sheet store before changing links; an invalid path returns
a save error. One locked registry write updates every checkout sharing the old Git source
id, its default and color mapping, and historical source-id aliases. The response names
the new `connection_id` and affected `checkouts`; new store hosting and index routing are
ready before the links switch (HS2-8BG4W9). Each Git checkout link records
the store's durable `instanceId` from `hotsheet-store.json` in
`store_instance_ids`; relink rejects a different valid store before changing
any shared checkout. New stores receive the id at initialization. A legacy
checkout link with no recorded identity cannot prove continuity from the path
alone, even while that path remains reachable; ordinary reads and relink return
verification guidance instead of adopting a replacement (HS2-EFBAPC, HS2-RQXJQV).
A source that a sourceless checkout only reads back through its root's
`.hotsheet2/store` link was never a persisted registry link, so registering it (for
example `bootstrap` into a project the app already opened before its store existed)
is a new selection and records the store identity (HS2-K2YHDE).
Checkout metadata remains readable when a link cannot be verified, and provider
descriptors remain readable for identity-less legacy links in the recovery UI;
unreviewed Git descriptors carry `identity_review_required`, and project opening
does not host or project tickets from those links.
The CLI's `checkout resolve`, `checkout register`, and `checkout source-color`
inspect configuration without opening ticket data, so they remain usable for
recovery. Interactive launch discovery rejects an unverified registered source
instead of falling back to a same-path replacement.
For this case, `PATCH /checkouts/{reference}/sources/{connection_id}/relink`
accepts `review_unverified_recovery: true` only after the caller explicitly reviews the
destination, including when the path is unchanged. It never overrides a known identity
mismatch. The updated checkout records
the old locator in `unverified_store_sources` under the new connection id; the checkout
provider descriptor exposes `unverified_recovery: true`, and the marker survives later
verified moves and restart (HS2-RAQSX7).
The same recovery is available headless through
`hotsheet-cli checkout review-source <checkout> <source-id> [--store <path>] [--yes]`.
Without `--yes` it only prints the checkout, linked and reviewed locations, and the
recorded and on-disk identities. With `--yes` it calls the same registry workflow:
reviewed unverified recovery when no identity was recorded, or a verified relink when
`--store` names the moved location of a recorded store. A repeated `--yes` is a
no-op (HS2-47AFBK).
Checkout ticket and data reads also compare the recorded identity with the current
store at a linked path. A mismatch returns a conflict with restore/replacement guidance,
including when that path was already hosted before replacement. Other projects
continue to route normally (HS2-BGTDQ1).
Store-scoped and provider-scoped routes also stop using a hosted entry when its
recorded instance identity no longer matches the path; checkout routes provide
the restore/replacement guidance.
The primary store is pinned when the server starts; a legacy primary receives an
identity then. Unscoped primary-store reads and writes return an actionable
conflict if another store replaces its path. `/health` stays a successful
machine-liveness probe for unrelated projects, but reports the primary conflict
without reading replacement ticket data. Restore the original
store to recover the running server, or restart deliberately to select the new
store. Checkout and hosted-store routes for unrelated projects remain available
(HS2-34XE8B).

`github-sign-in` works without a running server. It prints GitHub's verification URL and
one-time code, waits for approval, and then prints a `github-app-*` credential reference;
it never prints the access or refresh token. Use `--web-base https://github.example.com`
for Enterprise after configuring that origin's public Client ID in
`HOTSHEET_GITHUB_ENTERPRISE_APP_CLIENT_IDS`. `github-connect` stores the non-secret
connection in `providers.json` and optionally links a registered checkout. Repeating it
for the same repository keeps the existing connection id and checkout source; `--id`
selects a specific existing id for a name or credential update. It accepts a keychain
reference from `github-sign-in` or `key set`, and GitHub App bundles refresh when a
provider command reads them (HS2-DJA052). `--attachment-repo OWNER/REPO` (with optional
`--attachment-folder`, default `hotsheet-attachments`, and `--attachment-branch`, default
`main`) enables GitHub attachments through an assets repository; a later reconnect without
those flags keeps the repository, and `--no-attachments` removes it (HS2-HSA64D, see
[16](16-external-sync-interface.md)). `provider-attach <connection> <id> <files…>` uploads
files to a provider-native ticket and fails explicitly when the connection reports no
attachment support.

`batch <ids…>` accepts the same field and note options as `edit`, reads a `--note-file`
once, and applies the update to each id independently. It prints JSON with `updated`
slugs and `errors` containing the requested id and message, like `POST /batch`; an
invalid id does not prevent valid tickets from updating. `delete-note` uses the same
git-store deletion as the service, including removal of feedback tied to the deleted
note. `provider-delete-note` checks the connection's `note_delete` capability and
fails explicitly when unsupported. Neither deletion command changes other notes.
`provider-assign` accepts repeated `--to` values, `--clear`, and repeated
`--review email:kind` requests. It checks assignment and review capabilities before a
provider write. `provider-restore` restores only a git-backed provider's Trash ticket;
external trackers do not have Hot Sheet Trash. `provider-report-not-working` requires a
note (`--note` or `--note-file`) or one or more `--evidence` files, checks the atomic
report capability, reads all evidence before mutation, and accepts an optional opaque
`--expected-token` to reject a stale report. These commands print the resulting native
ticket as JSON.

`--note` accepts one argument exactly as supplied by the caller. For multiline Markdown,
use `--note-file <path>` or `--note-file -` (stdin) so real line breaks are preserved
without shell-escape interpretation. `--note-file` works for both appended notes and
`--edit-note` replacements. To prevent escaped line breaks from silently becoming visible
text, direct `--note` input containing a literal `\n` sequence outside Markdown inline
code or fenced backtick code is rejected with guidance to use `--note-file`. Intentional
literal text outside code may be passed with `--allow-literal-backslash-n`; the file/stdin
form also accepts intentional literal sequences without needing the override.
`edit` and `provider-edit` accept `--response-to <request-note-id>` when appending a
regular reply or `No response needed` acknowledgement. The request must be a
`feedback_needed` note on that ticket. The server PATCH routes and MCP
`hotsheet_update` accept the same `response_to` JSON field; it is invalid on an
edit or without note text. New feedback requests are independently tracked, while
older notes without the linkage retain sequential review state (HS2-W1ENZD).
`--note-confidence <0-100>` (HS2-DWTJ43) records the author's completion confidence on the
**appended** note, where it requires `--note`/`--note-file`. Anything but a plain integer
from 0 to 100 is rejected before any write. With `--edit-note <note-ulid>` (HS2-CY4CWC) it
instead corrects that note's score, and `--clear-note-confidence` removes it. The note text
is then optional: a confidence-only edit keeps the text, and a text-only edit keeps the
score. `provider-edit` accepts the same flags. The server's `PATCH /tickets/{id}`, checkout and
provider ticket routes, and MCP `hotsheet_update` take the equivalent `note_confidence`
JSON integer with identical validation (400 for a non-integer, out-of-range, or orphan
append value; 409 when the provider's `note_confidence` capability is off). With `note_id`,
an absent `note_confidence` leaves the score unchanged, an integer replaces it, and JSON
`null` clears it; on an append, `null` simply means "no score". A human text edit of an
AI-authored `feedback_needed` note through `note_id` is refused with 409 unless the body also
sets `edit_feedback_prompt: true`, the client's signal that the user explicitly opened the
prompt for editing rather than replying to it (HS2-MB2Q10, HS2-ZXGEFE). Correcting a score does not
rewrite the note's `## Confidence` prose, so an author fixing a mistyped score should edit
both in the same call.
Every command accepts the global `--actor-role human|ai|system` and `--actor-id <id>`
(falling back to `HOTSHEET_ACTOR_ROLE` / `HOTSHEET_ACTOR_ID`). Server mutating request
bodies accept `"actor": {"role": "...", "id": "..."}`; an invalid role is a 400. An
`ai` actor completing a ticket without a score in the current cycle is refused before any
write, with 422 and `{"code": "confidence_required"}` (`docs/05` §5, HS2-RD4M29). Full ticket
responses carry each note's `confidence` and the derived `latest_confidence` (`docs/02`
§2.6); both are omitted when absent, so older clients see an unchanged shape. `show`
prints the stored `confidence: NN` marker token.
Appending or editing a note remains successful when an `attachment:` reference does not
resolve yet, which supports note-before-upload workflows. The CLI prints an actionable warning
to stderr, and mutation API responses include a non-persisted `warnings` array. Resolution uses
the longest actual attachment filename after the marker, so sentence punctuation such as the
last period in `attachment:proof.png.` is retained as prose rather than treated as part of the
filename. `hotsheet attach` prints the canonical filename reference before the opaque durable id
so its result can be copied directly into a note. As a final write-boundary safeguard, Git-backed
note append/edit operations translate unambiguous bare attachment ULIDs in prose to same-ticket
`attachment:filename` or cross-ticket `attachment:[TICKET-SLUG]filename` references. Inline and
fenced code, URL/path segments, ids duplicated across multiple other tickets, and filenames the
current reference grammar cannot represent remain literal; the repair never guesses when
identity or author intent is ambiguous (the backtick-filename grammar gap is HS2-H2PTVZ).

The server equivalent is `POST /tickets/{id}/attachments` with raw file bytes and
an `x-hotsheet-filename` header. Browser clients percent-encode Unicode filenames and
declare `x-hotsheet-filename-encoding: percent`; the server decodes them before normal
filename sanitization. Optional `x-hotsheet-attachment-batch`, batch-label, actor, and
purpose headers add durable provenance; percent-encoded metadata declares
`x-hotsheet-metadata-encoding: percent`. The returned `ApiTicket.attachments` carries the
stable id, sanitized filename, creation timestamp, and optional provenance. Checkout-scoped clients use
`POST /checkouts/{reference}/tickets/{id}/attachments`, which resolves the ticket's
linked store and returns the same ticket plus its store identity. `PATCH` on that
collection atomically applies batch/actor/purpose metadata to a selected id set (reuse an
id to merge; generate one to split), while `PATCH` on an individual attachment renames it.
Every checkout-scoped single-ticket route (reads, updates, notes, assignment, restore,
batch updates, attachments, and code review) accepts a ticket slug, native id, or qualified
`connection_id:native_id`; a qualified id selects its linked source directly (HS2-QS9EQD).
Attachment `GET` accepts one byte range and responds with native media MIME, `Accept-Ranges`,
and exact `Content-Range`/`Content-Length` headers. The checkout server asynchronously seeks
to the requested offset and streams only that bounded span; it does not materialize the whole
attachment for a small media probe or seek.
For a video attachment, `GET
/checkouts/{reference}/tickets/{id}/attachments/{attachment_id}/thumbnail` returns its
content-addressed cached JPEG or `404` when none exists. A capable client publishes that
same path with `PUT`, `Content-Type: image/jpeg`, and a body of at most 5 MiB. Writes are
idempotent last-write-wins, so concurrent browser backfills are harmless; the cache key is
the SHA-256 digest of the source video rather than a platform path. A server may fill a
miss with an available `ffmpeg`, but missing or unsuccessful `ffmpeg` is still `404`, not
a dependency/setup error.
CLI uploads accept `--actor-role human|ai|system|unknown` plus optional `--actor-id` and
`--actor-name`. Automated evidence must declare its actual role instead of relying on the
legacy-safe `unknown` default. `hotsheet attachment-actor <ticket> <attachment-id>…`
repairs provenance on existing attachments without changing their batch, label, or purpose.
For git-backed tickets, `hotsheet attachment-rename <ticket> <attachment-ulid> <filename>`
renames a file while retaining its stable id, and `hotsheet attachment-delete <ticket>
<attachment-ulid>` removes the record and payload. `hotsheet attachment-metadata <ticket>
--attachment <attachment-ulid> [--attachment <attachment-ulid>…] --file <json-path|->`
replaces batch, actor, and purpose metadata for the selected ids as one operation; the JSON
uses `batch_id`, `batch_label`, `actor`, and `purpose` fields. A label requires an id.
Renaming to the current sanitized filename or applying the current metadata is a no-op,
including the git commit and `updated_at`. A missing id fails explicitly. The CLI commands
operate on the local git store; external providers expose attachment changes according to
their capabilities through the provider API.
`hotsheet annotate` accepts a unique attachment filename or ULID and a JSON file (or `--file -`
for stdin) containing either `{"annotations":[…]}` or the bare array. It prints the resulting
attachment with `--json`. The shared store validator rejects duplicate ids, rectangles outside
the normalized 0–10,000 range, and incomplete or reversed time ranges before writing. An
unchanged replacement makes no commit or activity note. The server supports the same `PUT`
body at `/tickets/{id}/attachments/{attachment_id}` and the checkout-scoped path; an optional
`actor` attributes the activity note. MCP `hotsheet_annotate_attachment` accepts the same array,
resolves an attachment by ULID or unique filename, and accepts `actor_role`/`actor_id`.
`--blocked-by` (repeatable, on `new` and `edit`) takes a slug **or** ULID and is
resolved to a ULID, rejecting unknown tickets and self-references; on `edit` a present
`--blocked-by` **replaces** the set and `--clear-blocked-by` empties it. The same edge
is settable over the API and MCP: `blocked_by` (an array of slug/ULID strings) on
`hotsheet_create` / `hotsheet_update` and the server's `POST` / `PATCH /tickets` — on
update a present `blocked_by` replaces the set (`[]` clears), absent leaves it. All
surfaces share one resolver (`ops::resolve_blockers`), mirroring how `duplicate_of` is
resolved on close.
`blocked_reason` is an independently presence-aware nullable update field and the single
source of truth for whether a ticket is blocked: omission
leaves the current reason unchanged, a string sets its trimmed value, and JSON `null`
(or an empty/whitespace string) clears it. Checkout, local git-provider, and serverless
MCP paths preserve that distinction instead of treating null as an omitted patch;
external-provider capability handling is tracked separately. `blocked_by` remains structured
dependency context, but an edge without a non-empty reason never creates an invisible block.
Create requests may also carry an initial `status` of `not_started`, `started`, or
`backlog`; omitting it remains backward-compatible and defaults to `not_started`.
Backlog creation clears `up_next`, and terminal/archive statuses must be reached through
an update so lifecycle timestamps and close behavior remain coherent. The server,
serverless MCP backend, and git-backed provider boundary preserve the same initial-state
contract; capability-aware GitLab/Jira behavior is tracked separately.
These write ticket files directly and **auto-commit** each mutation to the store's git
repo, then publish without holding the user-facing mutation open for network latency
(HS2-VJD1W4, HS2-0RDWSW). Headless CLI/MCP writes launch a reaped best-effort push in the
background; server-owned stores defer publication to the server's kicked, coalescing sync
loop. Thus a headless `work` run remains clean and shareable while browser/server writes
return after local durability rather than waiting several seconds for a remote. The
headless nonblocking contract is regression-tested under deliberate CPU and filesystem-I/O
saturation: deterministic child-spawn and remote-hook handshakes establish that the remote
is blocked before the bounded local-return assertion, without relying on a startup-speed
deadline. The shared
`ops` layer routes every mutation through `FsStore::write_ticket_committing`, so CLI + MCP

- server all commit. After the one-time legacy Finder-metadata cleanup, single-ticket and
  attachment mutations stage and commit only their exact touched paths, preserving unrelated staged/user work;
  the subsequent checkout worklist refresh queries only indexed active Up Next rows instead
  of scanning the store. It's a no-op when the store isn't a git repo, and
  `HOTSHEET_NO_AUTOCOMMIT` disables it for batch work.
  A checkout-scoped multi-ticket update validates every concurrency token before writing,
  then writes the affected ticket files and commits their paths once per Git store. It
  refreshes the live index and queues one background regeneration of each affected checkout
  worklist after the commit (its `worklist` Server-Timing split is only the queueing cost), while publishing an update event for each ticket. A later update error still
  commits and publishes the earlier successful items, matching the existing partial-write
  behavior. Other staged or untracked files are not included in the batch commit.
  Server-owned stores also commit in the background (HS2-0DH8ZS): an API mutation returns
  once its ticket file is written and indexed, and a process-wide committer thread lands
  the path-scoped commit moments later, coalescing queued jobs into one commit per store.
  The sync loop flushes queued commits before each fetch/rebase/push pass and graceful
  shutdown flushes before releasing the store; an uncommitted file left by a forced exit
  is picked up by the next sync commit. Note saves dropped from ~1 s (two synchronous
  commits) to ~20–35 ms on the 1.5 GB development store.
  Every routed request is timed (`request_timing` middleware): a request over its budget
  logs a `warn` with method, matched route template (never concrete ids), status,
  `elapsed_ms`, and `budget_ms`. Ordinary API requests have a 100 ms budget; attachment,
  provider-attachment, and speech routes 2 s; WebSocket, long-poll, and terminal-attach
  routes are not timed. Every timed response also carries `Server-Timing: handler;dur=<ms>`
  (appended after any route-specific entries) so clients can separate server time from
  queueing (HS2-ZGBQ5Q).
  A machine server started by the web bridge (`dev`, `stable-dev`, and `local-host-snapshot`
  launchers) appends its stderr, and so these warnings, to
  `${HOTSHEET_HOME:-~/.hotsheet2}/logs/server.log`. Each launch rotates a log over 8 MiB to
  `server.log.old`. A home that cannot hold the log discards stderr rather than failing the
  launch (HS2-DSYPTN).
  A store list (`GET /tickets` and the store-scoped list) also reports its phases ahead of
  `handler`: `index-wait` (waiting for the index mutex), `index-query`, `index-phases`,
  `settings`, and `encode` (HS2-P867K1).
  The store watcher's full resync, run at hosting time and after every lost-event rescan,
  reads and hashes every ticket file _before_ taking the index mutex. It then compares
  them against one `content_hashes` query, so a resync no longer blocks lists, details,
  and note saves for the length of a whole-store read (HS2-P867K1).
  Request handlers never run blocking store work on the async workers (HS2-9Z0Q8N): the
  router-wide `blocking_offload` layer runs every non-streaming request (auth plus handler)
  on tokio's blocking pool, so a handler stuck on a ticket-file `flock`, an index mutex, a
  slow disk, or SQLite blocks only its own pool thread and never starves unrelated requests.
  WebSocket, long-poll, and terminal-attach routes stay on the async workers. At most 256
  requests are offloaded at once (half the default pool cap), so nested `spawn_blocking`
  calls inside handlers always find a free thread; a panicking handler answers 500. A client
  disconnect still cancels its handler at the next await point (a dropped permission hook
  withdraws its pending prompt exactly as before).
  Long-wait routes, whose response waits on a person or an external event rather than local
  work, are never offloaded and never timed (HS2-V3S89Q): `POST /permissions/ask` (parks for
  up to the 24 h permission timeout) and `GET /github-auth/device/{session_id}` (parks until
  device authorization finishes). Any number of pending prompts therefore leaves all 256 API
  slots free. The pending permission ask also waits asynchronously on the bridge, so it parks
  no blocking-pool thread either (HS2-43YT58): more open hooks than the 512-thread pool still
  leave offloaded requests prompt. A hook that disconnects drops the wait, which withdraws its
  prompt as a one-time deny; tool threads keep the synchronous `request_blocking` API. When the pool is saturated anyway, a request that waited for a slot carries a
  `Server-Timing: queue;dur=<ms>` entry and logs a saturation `warn` with its route
  template, `waited_ms`, `in_use`, and `capacity`. `GET /health`
  reports the live pool as `api_offload: {in_use, capacity, waiting, long_waits}`.
  The checkout batch response includes `Server-Timing` measurements for admission,
  ticket writes, Git commit enqueue, index refresh, event publication, worklist refresh, and total
  handling time. The path-scoped Git commit runs on the process-wide background committer and is
  flushed at shutdown; `git;dur` measures enqueue time, not commit completion. These timings describe local server work; clients can measure their
  subsequent page query separately.
  Aggressive fetch/rebase/merge-on-conflict is the sync engine (`docs/03`; HS2-19); the
  semantic merge driver (§2.7) resolves concurrent edits. Bounded `ls`/full-text reads and
  slug resolution for `show` use the same file-backed SQLite index as the server. Opening
  that index reconciles committed and uncommitted Git ticket deltas first, so direct external
  file edits remain visible while warm read cost follows changed files and result size rather
  than total store size. Exact-ULID `show` remains a direct one-file read. A missing, stale-
  schema, or corrupt index is disposable and rebuilt from the source files; `hotsheet
reindex` performs the same explicit full rebuild. If a server is running, its watcher also
  observes CLI file changes and reindexes + broadcasts them live.

**Ops / lifecycle:**

```
hotsheet init          # create/register a project + default store (+ install the merge driver)
hotsheet init --standalone [--at <path>] [--remote <url>]  # create/link a standalone store
hotsheet bootstrap [--project <path>] [--store <path>] [--remote <url>] [--tool <id> ...]
                       # idempotently prepare store, link, tools/MCP, and remote
hotsheet serve         # run the server
hotsheet serve --stop  # stop the server; preserve detached terminals
hotsheet serve --stop --kill-all-terminals  # stop it and explicitly clear project terminals
hotsheet serve --stop --force  # also stop a server owned by the pinned production host
hotsheet reindex       # drop + rebuild the index from disk; prune idle older-schema index files
hotsheet doctor --project .  # store health + read-only tool/HS1 onboarding guidance
hotsheet merge-driver  # git-invoked semantic 3-way merge for ticket files (02-ticket-storage.md §2.7)
```

**AI-tool setup + plugins (core-owned, headless — [05](05-ai-tool-plugins.md) §5.1a, §5.11):**

```
hotsheet setup claude          # write CLAUDE.md/skills/MCP config for a tool   [built: HS2-98]
hotsheet setup --detect        # set up every AI tool detected on this machine   [built: HS2-98]
hotsheet setup --refresh --json --project <code-repo> # structured paths/notices [HS2-4AR09Z]
hotsheet setup --diagnose --tool codex --project <code-repo> --json # read-only plan and connection findings [HS2-9N8KVR]
hotsheet setup --guidance-owner rulesync --project <code-repo> # delegate instruction files; preserve their content [HS2-QS38E1]
hotsheet setup --guidance-owner builtin --project <code-repo> # restore the default writer after reviewing local guidance
hotsheet guidance --project <code-repo> register --repository <git-url-or-path> --ref <tag-or-sha> # pin and validate a source [HS2-2RD1M1]
hotsheet guidance --project <code-repo> list [--query <text>] [--owner <owner>] [--tag <tag>] # browse available/installed versions
hotsheet guidance --project <code-repo> select --source <id> --rule <name> [--skill <name>] # replace this source's selection
hotsheet guidance --project <code-repo> owner builtin|rulesync # select instruction writer
hotsheet guidance --project <code-repo> apply # reconcile selected rules and skills offline
hotsheet plugin list           # installed + detected AI-tool plugins            [HS2-92]
hotsheet plugin install <path|url>   # add an external plugin (trust-gated)      [HS2-93]
hotsheet plugin verify <id>    # run the conformance suite against a plugin      [HS2-93]
hotsheet plugin remove <id>                                                    # [HS2-92]
```

These run **with no server and no client** — the loader + setup writers live in the
core (§4.1), so a purely terminal workflow prepares a project for its AI tools on
its own. When a client is in play it asks the _server_ to run the same code.
`setup <tool>` is **built (HS2-98)**: it writes a merge-safe managed block into the
tool's instruction file (e.g. `CLAUDE.md`), the worklist skill, and an `.mcp.json`
entry registering the serverless `hotsheet-mcp --path <store>` (an **absolute**
`hotsheet-mcp` path when one sits next to the CLI, so it works without PATH munging —
HS2-117); re-running refreshes the managed pieces in place. The permission-bridge
install + the `hotsheet plugin` management commands are still to come.

`hotsheet setup --refresh --project <code-repo>` is the explicit headless freshness
check (HS2-40HZMB). It migrates existing readable settings, then refreshes enabled
tools that are detected or already have Hot Sheet-managed setup. The server schedules
that same core refresh in the background whenever a project opens or reopens (including
remembered projects restored at client startup), so opening is not blocked by filesystem
work. Clean bytes are never rewritten; instruction content outside managed blocks and
unrelated MCP configuration remain user-owned. The local development bridge awaits
the current CLI refresh report before opening, preserving project access if refresh fails.
This repairs projects attached to a still-compatible detached server built before the
server hook existed. Bundled managed skills
carry the same workflow version as the repository adapters, preventing a refresh from
downgrading current instructions. Executable detection includes Windows `PATHEXT`
wrappers such as `.cmd` as well as native binaries.

`setup --json` (explicit, detected, or refresh setup) returns `{set_up, removed}` without
human progress text. Successful setup reports contain `tool`, `wrote`, and optional `notices`.
When Codex hooks bytes change, `codex_hooks_changed` carries their project-relative `path`;
the human CLI prints a checkout-specific `/hooks` review instruction. Byte-identical setup
omits the notice. See [05](05-ai-tool-plugins.md) §5.7 for the trust-review behavior.
Setup writes Claude's machine-specific permission hook to `settings.local.json`. When it
creates a tool's project MCP config (or recognizes that the complete existing config is
Hot Sheet-owned), it records only that exact path in the checkout-local `.git/info/exclude`;
mixed user-owned configs remain visible, and the repository's shared `.gitignore` is not changed.

`hotsheet ai-tools --json` reports detected drivable plugins and their effective
model/effort catalogs. A drive may expose the optional runtime-catalog capability; when
it does, the core queries the installed provider, merges live ids and effort levels with
manifest labels/default fallbacks, and degrades to the manifest when the runtime is
offline. The server caches results by plugin id plus runtime version; authenticated
`GET /ai-tools?refresh=true` bypasses the same-version cache so clients can refresh after
provider changes. The server also memoizes a complete discovery for 10 s inside the same
catalog lock, so `/ai-tools`, `/ai-settings`, and terminal-launch validation share one scan
and concurrent callers coalesce onto it; `refresh=true` bypasses and repopulates the memo,
and `POST /setup/{tool}` (or replacing the plugin roots) invalidates it. Tools installed
outside the server appear once the memo expires or on the next refresh (HS2-QV8B7R). `hotsheet ai-settings get --json` and `ai-settings set --tool <id>
[--model <id>] [--effort <level>]` provide headless parity for the machine-local defaults
exposed by `/ai-tools` and `/ai-settings`; `ai-settings set-provider --tool <id> [--model <id>]
[--effort <level>]` saves one provider's own default model and effort without changing the
default provider (HS2-EK24KF).

**Headless bootstrap (HS2-J90FXF):** `bootstrap` is the idempotent composition for a
new or existing code project. It initializes or reuses a standalone HS2 store, links
and registers the checkout, installs or refreshes every detected AI-tool integration
(or each repeated `--tool`), and optionally configures an existing Git remote without
replacing a different `origin`. Bootstrap creates a committed store baseline before
remote setup, so the first `git push -u origin HEAD` is valid even before any ticket
has been created. With no remote it prints exact provider-neutral rerun
and first-push commands. It preserves user-authored instruction and MCP configuration,
and the graphical project's Git-source flow invokes this same CLI workflow.

**Headless first run (HS2-MNHGT3):** `init` prints the same read-only onboarding
report available from `doctor --project <code-repo>`. It detects installed plugin
tools and recommends explicit, idempotent `setup` commands. It recognizes HS1 only
when `<project>/.hotsheet/db/PG_VERSION` exists, warns the user to close HS1, and
prints the exact one-project `hotsheet-migrate` command. Neither command silently
changes tool configuration or starts migration; interactive prompting and optional
continuous config sync remain client work (HS2-8B0YZX).

**Drive a tool (the headless "play") + the work loop:**

```
hotsheet trigger <tool> [--prompt …] [--project DIR] [--mcp-config F] [--env K=V]
hotsheet work <tool> [--max 50] [--max-stall 3] [--project DIR] [--worker]
```

`work` is the **headless loop (HS2-118)**, the north-star bootstrap step: it drives the
tool one turn at a time — each turn takes the single highest-priority Up Next ticket —
until Up Next is drained, a turn cap (`--max`) is hit, or the queue stops changing for
`--max-stall` turns (a **thrash guard**, so a stuck tool doesn't spin forever). It reuses
`trigger`'s HS2-103 launch safety and exits cleanly (no setup required) when nothing is
Up Next. Each turn currently spawns a fresh process; cross-turn session resume is
**HS2-3C1XK3**.

`trigger` launches/injects one turn into an AI tool and streams it (HS2-109). It is
**safe by default (HS2-117 launch safety):** it prepends a `hotsheet` → `hotsheet-cli`
shim (plus the CLI's own dir) to the tool's PATH so a bare `hotsheet` can't hit an HS1
launcher and kill the dev instance (§4.4); refuses to run when the project holds a live
HS1 store unless the selected HS2 store contains a schema-valid completed-import receipt
whose canonical `sourceProject` is that exact checkout; refuses when the tool isn't set up;
and defaults `--mcp-config` to the tool's project config so the tool can reach **only**
the Hot Sheet MCP (Claude via `--strict-mcp-config`). Missing, malformed, relative-path,
or unrelated migration receipts fail closed. Codex reads its MCP servers from
`$CODEX_HOME`, so `trigger`
**auto-builds a throwaway MCP-free `CODEX_HOME`** for it (HS2-YRDQNX): a copy of the
user's `auth.json` plus a `config.toml` whose only server is the Hot Sheet shim — so a
bare `trigger codex` can't load the user's global MCP servers (pass `--env CODEX_HOME=…`
to override). The safety primitives live in `crates/hotsheet-aitools/src/launch_safety.rs`.

**Project settings (core-owned; shared + local scopes — §4.9):**

```
hotsheet settings get <key> [--scope shared|local]
hotsheet settings set <key> <value> [--scope shared|local]
hotsheet settings list [--scope shared|local]
```

Terminal history isolation is the default. The explicit opt-out is local-only:
`hotsheet-cli settings set terminal.inherit_global_shell_history true --scope local`.
The browser's Project Settings → Terminals control writes the same key.

The **HS1→HS2 migrator is a separate, disposable bundled tool** (may be Node),
**not** part of this long-lived CLI — it runs once per old project and is retired
once data is moved. Invoke it directly (`hotsheet-migrate <old-project>`) or via the
UI prompt; see [07-migration.md](07-migration.md). (If we ever want a `hotsheet
migrate` alias, it just shells out to that bundled tool.)

`hotsheet merge-driver` is not run by hand — it is the binary git calls for the
`merge=hotsheet-ticket` driver registered in each store's `.gitattributes`. It
lives in the CLI so the format-aware merge logic is the **same core code**, tested
once, that powers automatic conflict resolution. `hotsheet init` writes the
`.gitattributes` line and configures the local git repo to use it.

`clap` (Rust) for arg parsing. The CLI and server share the core, so an operation
behaves identically whichever path invokes it — a key maintainability win over
HS1, where the CLI and server were the same Node process but the client re-derived
logic separately.

### 4.4.1 Subcommand index

Every top-level `hotsheet-cli` subcommand (as printed by `hotsheet-cli --help`), grouped
by area with the doc that owns its behavior. `hotsheet-cli <command> --help` is the
authoritative flag reference. Full-text search is `ls --text`; there is no separate
`search` subcommand.

| Area                                  | Subcommands                                                                                                                                                                                                                                                              | Owning docs                                                                                |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| Store setup                           | `init`, `bootstrap`, `link`                                                                                                                                                                                                                                              | [02](02-ticket-storage.md) §2.8                                                            |
| Format compatibility                  | `activate-format`, `compatibility`                                                                                                                                                                                                                                       | [19](19-format-compatibility.md), [02](02-ticket-storage.md)                               |
| Ticket CRUD and query                 | `new`, `ls`, `show`, `edit`, `close`, `batch`, `delete-note`                                                                                                                                                                                                             | [02](02-ticket-storage.md), [03](03-indexing-and-query.md), [17](17-ticket-file-format.md) |
| Trash                                 | `restore`, `purge-trash`                                                                                                                                                                                                                                                 | [02](02-ticket-storage.md)                                                                 |
| Attachments                           | `attach`, `attachment-rename`, `attachment-delete`, `attachment-metadata`, `attachment-actor`, `annotate`                                                                                                                                                                | [02](02-ticket-storage.md), [17](17-ticket-file-format.md)                                 |
| Ticket providers                      | `providers`, `provider-ls`, `provider-get`, `provider-new`, `provider-edit`, `provider-close`, `provider-assign`, `provider-restore`, `provider-report-not-working`, `provider-delete-note`, `provider-attach`, `provider-disable`, `provider-enable`, `provider-remove` | [16](16-external-sync-interface.md)                                                        |
| GitHub sign-in, source setup          | `github-sign-in`, `github-connect`, `provider-connect`                                                                                                                                                                                                                   | [16](16-external-sync-interface.md) §16.8                                                  |
| Cross-store / cross-provider transfer | `copy`, `move`, `provider-copy`, `provider-move`                                                                                                                                                                                                                         | [02](02-ticket-storage.md), [16](16-external-sync-interface.md)                            |
| Collaboration                         | `assign`, `people`, `read`                                                                                                                                                                                                                                               | [10](10-assignment-and-collaboration.md)                                                   |
| Claims                                | `claim`, `claim-next`, `renew`, `release`                                                                                                                                                                                                                                | [02](02-ticket-storage.md), [10](10-assignment-and-collaboration.md)                       |
| Git sync and health                   | `sync`, `doctor`, `reindex`, `worklist`                                                                                                                                                                                                                                  | [02](02-ticket-storage.md) §2.12, [03](03-indexing-and-query.md)                           |
| AI tool setup and driving             | `setup`, `plugin`, `ai-tools`, `ai-settings`, `launch`, `trigger`, `work`                                                                                                                                                                                                | [05](05-ai-tool-plugins.md), [13](13-drive-transport-interface.md)                         |
| Permission hook route                 | `permission-hook`, `hook-diagnose`                                                                                                                                                                                                                                       | [05](05-ai-tool-plugins.md) §5.7                                                           |
| AI feedback                           | `feedback-synthesis`                                                                                                                                                                                                                                                     | [23](23-ai-feedback-synthesis.md)                                                          |
| Metrics and confidence                | `metrics`, `confidence-report`, `phase-report`                                                                                                                                                                                                                           | [14](14-metrics-interface.md), §4.4 above                                                  |
| Server-backed feeds                   | `commands`, `ticket-flow`, `activity`, `notifications`                                                                                                                                                                                                                   | §4.3 Headless platform APIs, [15](15-activity-narration-interface.md)                      |
| Settings and keys                     | `settings`, `key`                                                                                                                                                                                                                                                        | §4.7, §4.7.1                                                                               |
| Checkouts and accounts                | `checkout`, `account`                                                                                                                                                                                                                                                    | §4.3 Checkout identity, [16](16-external-sync-interface.md) (HS2-SM9PM8)                   |
| Migration                             | `import`                                                                                                                                                                                                                                                                 | [07](07-migration.md)                                                                      |
| Server and transport security         | `serve`, `cert`                                                                                                                                                                                                                                                          | §4.3.1, §4.6, [08](08-distributed-and-remote.md)                                           |

## 4.5 How the three stay consistent

There is exactly one implementation of every operation (in the core). The server
and CLI are its only two adapters, and both call the same functions; clients call
those operations _through the server's API_, never a re-implementation. This is
enforceable the way HS1 enforces its structural rules (an ESLint-style lint / a
Rust crate boundary): **domain logic may not live outside `hotsheet-core`.** A
"client mirror" of core logic is a bug, not a pattern.

## 4.6 Auth & trust tiers (carried from HS1, simplified)

- **Tier 0 — loopback:** plain HTTP + per-project shared secret. The default; the
  single-user local case. Trusts localhost.
- **Tier 1 — exposed:** binding off-loopback requires **mTLS + per-device client
  certs**, or the server refuses to start (never serves plaintext).
  See [08-distributed-and-remote.md](08-distributed-and-remote.md).

  > **Built (HS2-VT3JMF):** a **per-project CA** (`hotsheet-tls`, rcgen) signs the
  > server leaf + every device's client cert; `hotsheet-cli cert init|issue|renew|role|revoke`
  > manages it (material under `${HOTSHEET_HOME}/tls/<project-id>/`, machine-local —
  > the CA key is a secret, never committed). An off-loopback `serve` bind loads a
  > rustls `ServerConfig` and serves over a manual `tokio-rustls` acceptor (axum 0.8
  > has no built-in TLS), **requiring** a client cert that chains to the CA and isn't
  > on the `revoked` fingerprint list; loopback stays Tier-0 plaintext. Chain
  > validation is delegated to rustls's `WebPkiClientVerifier` (not hand-rolled), with
  > a revocation gate layered on. The verifier **re-reads the revocation list per
  > handshake** (HS2-MPC0QF), so `cert revoke` applies **live** — no server restart. A
  > CA is valid for ten years, server leaves for 397 days, and device leaves for 90 days;
  > `cert renew` revokes the previous device leaf, issues a fresh one, and carries its ACL
  > role forward. ACLs are opt-in: without `acl.json`, CA membership retains read-write
  > access; once a role is assigned, unknown fingerprints are denied and each device may be
  > `read-only`, `read-write`, or `deny`, reloaded per request. Full live mTLS E2E proves
  > valid-in / no-cert-out / revoked-out-live plus read-only/write/unknown authorization.
  > **Deferred:** `.p12` bundling + QR enrollment (client work) and intermediate-CA UX.

## 4.7 Project settings (shared / local / client) — core-owned

> **Built (HS2-94, HS2-34, HS2-REF96F, HS2-48XDJR):** `hotsheet_ticketing::settings::Settings` — a flat
> `key -> JSON` map per scope: **global** `${HOTSHEET_HOME}/settings.json`
> (machine-wide and project-independent), **shared**
> `<project-root>/.hotsheet2/settings.json` (committed with the code project), and
> **local** `<project-root>/.hotsheet2/settings.local.json` (auto-added to the code
> project's `.gitignore`). The effective value resolves in precedence **global < shared < local**
> (most specific wins). Driven headless by `hotsheet-cli settings get|set|list
[--scope global|shared|local]`. Client/device-only settings still never enter core.
>
> **Decided (maintainer, 2026-08-20):** project settings are **core-owned and
> CLI-manageable**, not app-only. The client owns _only_ device-specific settings.
> Build: **HS2-94**.

The `.hotsheet2` project directory is intentionally distinct from HS1's `.hotsheet`
runtime directory. This lets both applications operate in one checkout without either
rewriting the other's settings, store link, or generated worklist. On first setup or
settings migration, HS2 reads schema-marked files from the former `.hotsheet` location
and writes their values to `.hotsheet2`; it never treats an unmarked HS1 settings file as
HS2 input and does not delete the compatibility source. The machine-wide default was
already `${HOTSHEET_HOME:-~/.hotsheet2}` and is unchanged.

Settings split by **scope**, which maps directly onto the already-decided
shared-vs-local on-disk model ([README](README.md); [02-ticket-storage.md](02-ticket-storage.md)
§2.11). Each scope has a clear owner and a clear on-disk home:

| Scope                    | Examples                                                                                                                                                         | On disk                                                                                                                   | Managed by                                              |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| **Global**               | cross-project personal defaults (default AI tool, editor) set once per machine                                                                                   | **`${HOTSHEET_HOME}/settings.json`** (machine-wide, not tied to a store)                                                  | core → **CLI + server**                                 |
| **Shared**               | auto-context guidance (HS2-25), categories, per-category instructions, custom views, `trash_cleanup_days` retention policy, enabled-plugin set for the _project_ | **`<project-root>/.hotsheet2/settings.json`**, committed with the code project and independent of its ticket sources      | core → **CLI + server + client**                        |
| **Local**                | which tools are enabled _on this machine_, index location, machine paths                                                                                         | **`<project-root>/.hotsheet2/settings.local.json`**, gitignored in the code project (machine-local, not device-app-local) | core → **CLI + server**; client via checkout-scoped API |
| **Client / device-only** | window geometry, theme, per-viewer PTY size prefs (§6.7)                                                                                                         | the client's own app storage                                                                                              | **client only — never enters core**                     |

The dividing test: _does a headless CLI or the server ever need this value?_ If yes,
it's shared or local and lives in core-owned settings. If it only means something to
a running GUI on one device, it's client-only and the core never sees it. This is
why `hotsheet settings` (§4.4) can manage the first two scopes with no client at all,
while a window position stays out of the core entirely.

Checkout-owned settings remain available when a project has zero ticket sources and have
one identity when it has several. Older `hotsheet-settings.json` and
`hotsheet-settings.local.json` files beside linked git stores are deterministic read-through
inputs (default source first) until the project file is written or setup migration copies
them forward; legacy store-only/serverless APIs retain their old paths rather than creating
`<ticket-store>/.hotsheet`. Global settings and ticket-source discovery are unchanged.
Settings rewrites replace each scope file atomically so concurrent CLI and server readers see
complete JSON before or after a write. `set`, `unset`, and migration hold a cross-process
lock for the entire read-modify-write transaction of each scope, preventing concurrent
writers from silently dropping different keys. Lock files live under
`${HOTSHEET_HOME}/settings-locks`, outside the committed project settings directory. Each
lock file records the settings path it guards. On Unix, at most once an hour
(`.last-prune` marker), a writer removes unheld locks whose settings file no longer
exists, plus legacy empty locks, so a lock is not left behind for every temp project ever
seen (HS2-MZ1TMH). A writer re-checks after locking that its open file is still the one at
the lock path, and retries if a prune replaced it. Windows does not prune.

`trash_cleanup_days` is a positive whole number stored in Shared scope. Its documented and
runtime default is 30. The server validates checkout-scoped writes, the generic CLI settings
writer rejects invalid values, and `hotsheet-cli purge-trash` reads the effective project
value unless `--older-than-days` explicitly overrides the current run.

The `commands` key is a machine-local array of typed definitions: `id`, `title`, exact
`program` + `args`, optional `group`/`confirmation`, and optional `cwd`. Without `cwd`
the checkout-scoped command runner uses the code-project root; HS1 migration sets it to the original
code-project root so imported shell and AI command buttons retain their working-directory
semantics without embedding a shell-formatted `cd` string. Definitions, run history,
lookup, and cancellation are isolated by checkout on a shared server.
The command editor sends `X-Hotsheet-Command-Write-Session` and an increasing
`X-Hotsheet-Command-Write-Sequence` with each `PUT /commands`. Both headers must
be present together. The server returns `409` when a request's sequence is no
newer than the last committed one for that checkout and editor session, so a
delayed autosave cannot replace a later page-hide save. The write and live
definition update share the ordering guard. The browser keeps its sequence in
session storage across reloads. `GET /commands` and `GET /command-groups` return
strong content revisions in `ETag`; both root and checkout-scoped `PUT` routes
require `If-Match` with the revision the writer read. Missing preconditions return
`428`, and a stale revision returns `409`. `If-Match: *` explicitly requests an
unconditional administrative replacement. Revisions derive from persisted content,
so a stale editor remains stale after a server restart or a CLI settings edit. A
newer sequence from the same editor can follow its own in-flight save's revision,
but a write by a different editor still conflicts (HS2-X2A3C1).
The server repeats the revision check while holding the global, shared, and
local settings file locks, so a separate `hotsheet-cli settings set` or
`unset` cannot write between the final comparison and replacement
(HS2-KYP8ZT).
The CLI exposes that same live runner without opening the web client. Start a server for
the selected `-C` ticket store, then use a registered checkout id, alias, or path:

```sh
hotsheet commands list web
hotsheet commands groups web
hotsheet commands run web build
hotsheet commands history web
hotsheet commands get web <run-id> --after 0
hotsheet commands cancel web <run-id>
```

Each command prints JSON. `run` returns a run id immediately; `get` can poll its state and
output with a sequence cursor, while `history` lists recent runs. The CLI discovers the
local server for `-C`, sends its machine-local secret only in the request header, and uses
the checkout-scoped routes. An absent server, unknown checkout or command, and a run from
another checkout fail explicitly. Run history is the server's bounded in-memory history
and ends with that server process.

The sibling machine-local `command_groups` key is an ordered list of group names the
command settings editor keeps even while no command is in them (HS2-EZ5KMC), so an empty
"Add group" group, or a group emptied by deleting or moving its last command, survives a
reload until the user deletes it. `GET`/`PUT /command-groups` (and the checkout-scoped
`/checkouts/{reference}/command-groups` the client uses) read and replace it; writes are
trimmed, blank names dropped, and duplicates collapsed. The generic settings writer
rejects anything but a list of strings, so `hotsheet-cli settings set command_groups`
is the equivalent headless path. Group writes use their own `ETag`/`If-Match`
revision, independent of command definitions. The browser retains a conflicting
draft and asks the editor to copy it before reloading the current remote settings.

Rich-activity distillation is a stricter case: consent is read from the **local scope
only**, even though normal effective settings permit shared/global fallback. A committed
project must not opt another collaborator into summarization. Enable the built-in,
no-network deterministic adapter with:

```sh
hotsheet settings set activity_distillation \
  '{"enabled":true,"adapter":"deterministic","deterministic_fallback":true}' \
  --scope local
```

The failure fallback defaults to `false`; the example opts into it explicitly. This is
separate from selecting the deterministic adapter itself.

Set `enabled` to `false` (or unset the local key) to stop it immediately; pending
in-memory windows for that checkout are discarded. Project-attributed events resolve the
explicit checkout id/root, verify that the checkout links the target store, and keep each
checkout's candidate window separate even when the store is shared. Events without a
project identity retain the legacy store-only policy; an invalid explicit identity is not
silently guessed. Native clients can select a client-owned adapter such as
`apple_foundation_models`. The server records and broadcasts the normalized local stream
but never loads Apple Foundation Models or any other client model.

Shared settings are versioned by the code project's git, independently of whether its
tickets use zero, one, or several stores; local settings are disposable machine state. The
`plugins` module reads the enabled-plugin set from settings to decide what `hotsheet
setup` writes ([05](05-ai-tool-plugins.md) §5.1a).

### 4.7.1 Secure provider keys

> **Built (HS2-M1XMSX):** `hotsheet_ticketing::secrets` provides an injected
> `SecretStore` port, native macOS Security.framework Keychain, Linux Secret Service
> (`secret-tool`), and Windows Credential Manager adapters, plus a global provider registry. `hotsheet key
set|get|list|delete` is the headless CLI surface; `set` uses a hidden prompt when
> stdin is a terminal and accepts piped stdin for automation. `${HOTSHEET_HOME}/keys.json` contains provider names and fallback environment
> variable names and non-secret account labels, is mode `0600` on Unix, and never contains
> secret values. A Hot Sheet GitHub sign-in records its `site` (the web origin signed in to)
> and, when available, its authenticated login as `identity`; both survive token refreshes
> (HS2-16MYXN, HS2-1D350D).

Settings refer to a secret as `{ "secret": "provider-id" }`; consumers call the
shared resolver. Since HS2-5A6JNS, the OS credential store holds one machine master.
Provider values and permission-vault master values live in authenticated, owner-only
envelopes under `${HOTSHEET_HOME}/machine-secrets/`, with a distinct derived key per
account. A successful master read is cached once per process; a machine-local
revision marker invalidates that cache after rotation or restore. The first access to an
old per-account Keychain item copies and verifies its envelope; the old item remains
for recovery, but later reads use the envelope. Corrupt envelopes and a missing or
denied master fail closed rather than falling back to old credentials. The first
migration launch may still request access to those old items.

`hotsheet key master-export --output <backup>` writes a new owner-only root backup;
`master-restore --input <backup>` verifies it against existing envelopes before
restoring a missing root. `master-rotate` re-encrypts envelopes under a new root;
`master-resume` completes a rotation interrupted after the native root changed.
`master-cleanup-legacy --backup <backup>` verifies the current backup and every
envelope before removing migrated per-account OS items.
Keep the backup outside the repository and treat it as access to every local secret.

If no credential exists, the only fallback is the explicit read-only environment variable
`HOTSHEET_API_KEY_<NORMALIZED_PROVIDER>`. There is deliberately no plaintext file
fallback: an unavailable keychain makes writes fail closed. Secret values must not be
placed in project/global settings, ticket files, logs, diagnostics, or command arguments.

## 4.8 Open items

- **Language confirmation** (Rust vs Go) — [09-technology-decisions.md](09-technology-decisions.md) §9.2.
- **MCP transport for the new server** — HS1 spawns a separate `channel.ts` MCP
  process per project. In HS2 the plugin host may let the server itself expose MCP,
  or keep a small per-project MCP shim; decided in [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.8.

## 4.9 Cross-references

- Storage: [02-ticket-storage.md](02-ticket-storage.md) · Index: [03-indexing-and-query.md](03-indexing-and-query.md)
- AI-tool plugins (loader, setup ownership, external plugins): [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.1a, §5.11
- Clients (API consumers) + server auto-start lifecycle: [06-clients.md](06-clients.md) §6.2
- Language/runtime decisions + server-separation ADR: [09-technology-decisions.md](09-technology-decisions.md) §9.1e, §9.2
