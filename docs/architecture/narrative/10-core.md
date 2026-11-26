## Core ticketing crates

![](embed:HS2-Core-TicketEngine)

This is the starting diagram for this area. The **Start here** chapter lists the others.

Status: written by HS2-AH9HAK. Audited source
`8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`. Follow the depth standard and evidence format in the architecture
README. Per-module rows, including every 500-line module, are in
[coverage/core.md](../coverage/core.md).

### Overview

The core area is the set of linked Rust crates in the `coreLib` container plus the
three data stores it owns: the Git ticket store (`tickets`), the derived SQLite index
(`searchIndex`) and the code checkout's project files (`checkoutData`). No core crate
runs as a process. The server, the CLI and the MCP shim link the same code, so a
direct CLI write and a server write follow one path.

- `hotsheet-model` is pure: types, ids and the file format, with no I/O
  (`crates/hotsheet-model/src/lib.rs:1`).
- `hotsheet-ticketing` is the engine: the Git file store, operations, provider
  contract, sync and merge, settings, checkouts, secrets, groups, metrics and
  maintenance. Its public API is synchronous (`crates/hotsheet-ticketing/src/lib.rs:1`).
  Its `orchestration/`, claim and coordination modules belong to the orchestration
  area (HS2-RJB111).
- `hotsheet-index` is the disposable SQLite and FTS5 projection
  (`crates/hotsheet-index/src/lib.rs:1`).
- `hotsheet-extsync` holds the GitHub, GitLab and Jira adapters. Remote systems stay
  authoritative; nothing is mirrored into Git (`crates/hotsheet-extsync/src/lib.rs:1`).
- `hotsheet-sync`, `hotsheet-tls` and `hotsheet-process-journal` are small utilities.

Views: `HS2-Core-TicketEngine`, `HS2-Core-Providers` and `HS2-Core-MachineAndProject`
(component views), and four dynamic views described below.

### Components

Each component's seven depth-standard fields are in its coverage row. This section
gives what a reader needs before opening the code.

**Ticket model and file format (`coreModel`).** A ticket is one file: a `---` YAML
frontmatter, a body between `hotsheet:body` markers, and an optional notes container
whose notes each sit between `hotsheet:note:begin <ULID> …` and `hotsheet:note:end`
markers (`crates/hotsheet-model/src/format.rs:118-123`). `to_file_string` and
`parse_file` are the only codec (`crates/hotsheet-model/src/format.rs:126`,
`crates/hotsheet-model/src/format.rs:162`). Unknown frontmatter keys are kept in
`Ticket::extra` and re-emitted sorted after known keys
(`crates/hotsheet-model/src/format.rs:190-199`). The `schema` key is a writer guard:
the serializer writes the lowest guard string that can represent the ticket, from
`hotsheet/v2-bounded-notes` to `hotsheet/v6-annotation-arrow-heads`, and the parser
rejects any guard string it does not know with `UnsupportedSchema`, whose code is
`upgrade_required` (`crates/hotsheet-model/src/format.rs:20-24`,
`crates/hotsheet-model/src/format.rs:292-312`, `crates/hotsheet-model/src/format.rs:100-106`).
A numeric `schema` value passes through unchecked. Feedback drafts and empty-text notes
are never written (`crates/hotsheet-model/src/format.rs:147-153`). Slugs are
`<PREFIX>-<6 Crockford chars>` from an FNV-1a hash of the ULID bytes
(`crates/hotsheet-model/src/ids.rs:17`); collisions are possible and are resolved by
lookup order, not prevented. Test: `crates/hotsheet-model/tests/proptest_format.rs`,
`crates/hotsheet-model/tests/compatibility_fixtures.rs`, and `round_trips_and_is_byte_idempotent`
in `crates/hotsheet-model/src/format.rs`.

**Git file store (`coreStore`).** `FsStore` owns paths, atomic writes, locks and local
commits. Details are in the next three sections. It never fetches or rebases; that is
`coreSync`. Test: `crates/hotsheet-ticketing/src/store.rs` (tests module), `crates/hotsheet-ticketing/tests/store_admission_policy.rs`.

**Ticket operations (`coreOps`).** `ops.rs` is the one implementation of query,
create, update, note, close, copy, move, restore, purge and claim that every surface
calls (`crates/hotsheet-ticketing/src/ops.rs:1-6`). Callers pass `now` and ids; ULIDs
are minted by `mint_ulid` from injected ports (`crates/hotsheet-ticketing/src/lib.rs:143`).
Each mutation reads, revises and writes inside `with_ticket_transaction`, for example
`update` (`crates/hotsheet-ticketing/src/ops.rs:810`) and `claim_with_eta`, which
also consults the orchestration area's claim gate before writing
(`crates/hotsheet-ticketing/src/ops.rs:3035-3048`). Test: `crates/hotsheet-ticketing/src/ops.rs` tests module,
`crates/hotsheet-ticketing/src/ops/claim_next_across_tests.rs`.

**Wire DTOs and checkout ordering (`coreWire`).** `ApiTicket` and `TicketRow` are the
single JSON shape (`crates/hotsheet-ticketing/src/wire.rs:1`). A Git ticket's
`concurrency_token` is its `updated_at` text (`crates/hotsheet-ticketing/src/wire.rs:283`).
`checkout_order::compare` and `checkout_page::merge_page` give one global order and one
cursor codec across Git and hosted sources (`crates/hotsheet-ticketing/src/checkout_order.rs:151`,
`crates/hotsheet-ticketing/src/checkout_page.rs:272`). Test: `crates/hotsheet-ticketing/src/wire.rs`,
`crates/hotsheet-ticketing/src/checkout_page.rs` tests modules.

**Provider contract (`coreProviderContract`, `coreGitProvider`).** `TicketProvider` is a
synchronous `Send + Sync` trait (`crates/hotsheet-ticketing/src/provider.rs:1135`).
Required methods are `descriptor`, `query`, `find_transfer`, `get`, `create`, `update`,
`add_note`, `close`, `assign`, `claim_next`, `release` and `renew`; the rest default to
`Unsupported` or a compatible fallback (`crates/hotsheet-ticketing/src/provider.rs:1135-1534`).
Clients must read `ProviderCapabilities`, never infer from the provider id
(`crates/hotsheet-ticketing/src/provider.rs:448-512`). `ProviderError` is the only
error surface: `UnknownConnection`, `NotFound`, `Unsupported`, `Authentication`,
`Conflict`, `RateLimited { retry_after_seconds }`, `Disabled`, `Store`, `Operation`,
`InvalidNativeId` and `Endpoint` (`crates/hotsheet-ticketing/src/provider.rs:1076-1115`).
A stale expected token is the `Conflict` message `ticket changed since it was read`,
recognized by `is_changed_since_read` (`crates/hotsheet-ticketing/src/provider.rs:1118-1132`).
`MutationContext.generated_id` is the provider-neutral idempotency key
(`crates/hotsheet-ticketing/src/provider.rs:1051-1058`). `ProviderRegistry` maps
connection id to adapter behind a poison-recovering mutex
(`crates/hotsheet-ticketing/src/provider.rs:2357-2408`). Test:
`git_provider_conforms_to_create_get_query_update_note_close` and
`registry_aggregates_success_and_partial_provider_failure` in `crates/hotsheet-ticketing/src/provider.rs`.

Capability differences at the audited revision (`crates/hotsheet-ticketing/src/provider.rs:530`,
`crates/hotsheet-extsync/src/github.rs:2223`, `crates/hotsheet-extsync/src/gitlab.rs:1039`,
`crates/hotsheet-extsync/src/jira.rs:1510`):

| Capability                                                 | Git | GitHub                | GitLab | Jira     |
| ---------------------------------------------------------- | --- | --------------------- | ------ | -------- |
| close                                                      | yes | yes                   | yes    | no       |
| note edit, delete, kind edit, response link                | yes | no                    | no     | no       |
| attachments                                                | yes | only with assets repo | yes    | yes      |
| attachment edit / delete                                   | yes | no / no               | no/no  | no / yes |
| assignment                                                 | yes | yes                   | no     | yes      |
| review requests, dependencies, claims, groups, ai feedback | yes | no                    | no     | no       |
| up next                                                    | yes | yes                   | no     | no       |
| close reasons                                              | yes | yes                   | no     | no       |
| advisory claims                                            | no  | no                    | no     | yes      |
| write behind                                               | no  | no                    | no     | yes      |
| watch                                                      | yes | no                    | no     | no       |
| offline mutation, atomic batch, not-working report         | yes | no                    | no     | no       |
| note confidence                                            | yes | yes (comment trailer) | yes    | yes      |

**External adapters (`coreGithub`, `coreGitlab`, `coreJira`, `coreExtShared`).** All
three share one blocking `ureq` transport with a 30-second request timeout and a
300-second timeout for its second request form (`crates/hotsheet-extsync/src/github.rs:314-370`).
Status mapping differs:

- GitHub maps 429, or 403 with `x-ratelimit-remaining: 0`, `retry-after` or a
  "rate limit" message, to `RateLimited`, using `retry-after`, else
  `x-ratelimit-reset`, else 60 s, and stores a cooldown so later calls fail fast until
  it passes (`crates/hotsheet-extsync/src/github.rs:466-569`). Other 401/403 is
  `Authentication`; 404/410 is `NotFound`; 409/412/422 is `Conflict`.
- GitLab maps 429, and 403 with `retry-after` or `RateLimit-Remaining: 0`, to
  `RateLimited` without a cooldown (`crates/hotsheet-extsync/src/gitlab.rs:136-170`).
- Jira maps 401 and 403 to `Authentication` and 429 to `RateLimited`
  (`crates/hotsheet-extsync/src/jira.rs:159-181`).
- A transport failure (DNS, TLS, timeout) is a `Conflict` carrying the message in all
  three (`crates/hotsheet-extsync/src/github.rs:492-497`).

None of the adapters retries by itself; retry belongs to the caller (the outbox
dispatcher below). Remote Delete is a label-based Trash in all three
(`crates/hotsheet-extsync/src/github_trash.rs:1`, `crates/hotsheet-extsync/src/gitlab_trash.rs:1`,
`crates/hotsheet-extsync/src/jira_trash.rs:1`). Notes carry Hot Sheet metadata as a
plain-text trailer (`crates/hotsheet-extsync/src/note_trailer.rs:1`). `live_provider`
builds an adapter from a connection record and a resolved token
(`crates/hotsheet-extsync/src/lib.rs:60`). Test: `crates/hotsheet-extsync/src/github_trash_tests.rs`,
`crates/hotsheet-extsync/src/gitlab_trash_tests.rs`, `crates/hotsheet-extsync/src/jira_trash_tests.rs`,
`crates/hotsheet-extsync/src/github_pagination_tests.rs`, `crates/hotsheet-extsync/tests/provider_latency.rs`;
live tests run only when `HOTSHEET_*_LIVE_*` variables are set.

**Provider outbox (`coreOutbox`).** See the dynamic view `HS2-Core-OutboxRetry`. Only the
Jira write-behind uses it, and only when the server starts with
`HOTSHEET_JIRA_WRITE_BEHIND=1` (`crates/hotsheet-server/src/main.rs:394-398`).

**Search index (`coreIndex`).** See the index sections below and `HS2-Core-IndexRebuild`.

**Machine and project state.** `coreCheckouts`, `coreProviderConfig`, `coreSettings`,
`coreGroups`, `coreSecrets` and `coreRemoteServers` hold machine-local or project-local
JSON. `coreActivityMetrics`, `coreRepoMaintenance` and `coreMedia` are leaf features that
write into the store or the machine home. `coreUtil`, `coreTls` and `coreProcessJournal`
are separate crates. Their rows in the coverage file record state, locks and tests.

### Durable state and file formats

| Record                                          | Location                                                                                    | Writer                                                                    | Crash behavior                                                                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Ticket file                                     | `<store>/tickets/<last 2 ULID chars>/<ULID>.md`                                             | `coreStore`                                                               | Written to `.<name>.<pid>.<seq>.tmp` then renamed; a reader sees old or new bytes, never a partial file                   |
| Legacy ticket path                              | `<store>/tickets/<first 2 ULID chars>/<ULID>.md` (store schema 2)                           | read only until activation                                                | Activation rewrites each healthy ticket, removes the old path, and writes metadata last, so a rerun completes it          |
| Store metadata                                  | `<store>/hotsheet-store.json`                                                               | `coreStore`                                                               | `init` writes atomically; `activate_current_format` and `set_ticket_prefix` use plain writes                              |
| Attachment payload                              | `<store>/attachments/<ticket ULID>/<attachment ULID>/<filename>`                            | `coreStore`                                                               | Payload written with `fs::write`, then the ticket is rewritten atomically; a crash between leaves an unreferenced payload |
| Managed `.gitignore`                            | `<store>/.gitignore`                                                                        | `coreStore`                                                               | Appends missing managed lines; idempotent                                                                                 |
| Note-mutation lock                              | `<store>/.hotsheet-note-mutation.lock`                                                      | `coreStore`                                                               | Never deleted; OS releases the lock on exit                                                                               |
| Git coordination locks                          | `<git-dir>/hotsheet-autocommit.lock`, `hotsheet-history-sync.lock`, `hotsheet-sync-ai.lock` | `coreStore`, `coreSync`                                                   | Never deleted; OS releases on exit                                                                                        |
| Sync health                                     | `<git-dir>/hotsheet-sync-health.json`                                                       | `coreSync`                                                                | Rewritten per cycle; holds no credentials or ticket content                                                               |
| Sync AI record                                  | `<git-dir>/hotsheet-sync-ai.json`                                                           | `coreSync`                                                                | One ask per stall until the store publishes again                                                                         |
| Saved-write backup                              | `refs/hotsheet/sync-backup/<stamp>-saved-writes`                                            | `coreSync`                                                                | Written before an explicit abort, so a crash between abort and restore loses nothing                                      |
| Moved stale locks                               | `<common-git-dir>/hotsheet-stale-locks/<UTC stamp>/` + `recovery.json`                      | `coreSync`                                                                | Locks are renamed, never deleted                                                                                          |
| Compaction recovery marker                      | `<git-dir>/hotsheet-compaction-recovery.json`                                               | `coreRepoMaintenance`                                                     | Present: every write and sync refuses until recovery                                                                      |
| Merge driver registration                       | `<git-dir>/info/attributes` and local Git config                                            | `coreMerge`                                                               | Re-registered before every integration                                                                                    |
| Read overlay, activity, raw usage, live history | `<store>/local/reads.json`, `activity/recent/`, `metrics/raw/`, `metrics/local/`            | `coreActivityMetrics`, overlay                                            | Gitignored and machine-local; readers skip a torn final JSONL line                                                        |
| Usage rollups, prices, people roster            | `<store>/metrics/rollups/<email>/rollup.json`, `metrics/prices.json`, `people.json`         | `coreActivityMetrics`, roster                                             | Committed and shared; rollups are a derived cache                                                                         |
| Index                                           | `HOTSHEET_HOME/index/<hash16>.v26.sqlite` plus `-wal`, `-shm`, `-open.lock`                 | `coreIndex`                                                               | Corrupt file deleted and rebuilt; schema mismatch drops tables                                                            |
| Outbox                                          | `HOTSHEET_HOME/outbox/<store id>.sqlite`                                                    | `coreOutbox`                                                              | WAL, `synchronous=FULL`; `sending` rows reset to `queued` on open                                                         |
| Checkouts, providers, keys, remotes             | `HOTSHEET_HOME/checkouts.json`, `providers.json`, `keys.json`, `remotes.json`               | `coreCheckouts`, `coreProviderConfig`, `coreSecrets`, `coreRemoteServers` | Locked read-modify-write; `providers.json` published by temp file, fsync and rename                                       |
| Secret envelopes                                | `HOTSHEET_HOME/machine-secrets/`                                                            | `coreSecrets`                                                             | AES-256-GCM per account; the master key lives only in the native credential store                                         |
| Settings                                        | `HOTSHEET_HOME/settings.json`, `<checkout>/.hotsheet2/settings.json`, `settings.local.json` | `coreSettings`                                                            | Atomic replace under `HOTSHEET_HOME/settings-locks/<sha256>.lock`                                                         |
| Group registry                                  | `<checkout>/.hotsheet2/groups/<id>.json`                                                    | `coreGroups`                                                              | One file per group; committed with `git commit --only`                                                                    |
| TLS material, process records                   | `HOTSHEET_HOME/tls/<project id>/`, `HOTSHEET_HOME/processes/`                               | `coreTls`, `coreProcessJournal`                                           | Private keys written with owner-only permissions; journal records written by temp file and fsync                          |

Evidence: store layout and constants `crates/hotsheet-ticketing/src/store.rs:6-11`,
`crates/hotsheet-ticketing/src/store.rs:28-80`, `crates/hotsheet-ticketing/src/store.rs:705-735`;
atomic write `crates/hotsheet-ticketing/src/store.rs:2141-2160`; activation
`crates/hotsheet-ticketing/src/store.rs:814-858`; attachment write
`crates/hotsheet-ticketing/src/store.rs:1351-1356`; locks
`crates/hotsheet-ticketing/src/store.rs:2396-2440`; sync health
`crates/hotsheet-ticketing/src/sync_health.rs:40`; backup ref
`crates/hotsheet-ticketing/src/sync_saved_writes.rs:148`; stale locks
`crates/hotsheet-ticketing/src/sync_stale_locks.rs:37-40`; compaction marker
`crates/hotsheet-ticketing/src/repository_compaction.rs:93-107`; outbox
`crates/hotsheet-ticketing/src/provider_outbox.rs:300-405`; secrets
`crates/hotsheet-ticketing/src/secrets/machine_secrets.rs:84`,
`crates/hotsheet-ticketing/src/secrets/machine_secrets.rs:326-340`; settings locks
`crates/hotsheet-ticketing/src/settings.rs:257-260`; group registry
`crates/hotsheet-ticketing/src/groups/registry.rs:22`; index path
`crates/hotsheet-cli/src/lib.rs:82-89`; TLS `crates/hotsheet-tls/src/lib.rs:14-16`;
journal `crates/hotsheet-process-journal/src/lib.rs:806-830`.

**Identity.** A ticket's key is its ULID; the slug is derived and can be rewritten when
a ticket moves stores (`crates/hotsheet-ticketing/src/ops.rs:2547`). A store's durable
identity is `instanceId`, a ULID kept across reinitialization
(`crates/hotsheet-ticketing/src/store.rs:436-449`). A Git connection id is the first 16
hex digits of SHA-256 over the canonical store path, so it changes when the store moves
(`crates/hotsheet-ticketing/src/provider.rs:51-57`). A checkout id is
`<folder slug>-<12 hex of SHA-256(canonical root)>` (`crates/hotsheet-ticketing/src/checkouts.rs:397-418`).

**Index schema.** `tickets` (one row per `(store_id, id)` with denormalized fields, the
content hash and file path), `phase_spans`, `tags`, `assignees`, `reviews`, the
`tickets_fts` FTS5 table over slug, title, tags, details, notes, attachment names and
legacy number, and `index_meta` holding `schema_version`, `head`, `git_owner` and
`dirty_ticket_ids` (`crates/hotsheet-index/src/lib.rs:60-112`). The file name carries
the schema generation, so builds from different commits never share a file
(`crates/hotsheet-index/src/lib.rs:53`). Test: `index_file_name_is_scoped_by_schema_version_so_builds_never_share_a_file`
in `crates/hotsheet-index/src/tests.rs`.

### Concurrency, tasks and locks

The core crates start two named background threads and otherwise run on the caller's
thread:

- `hotsheet-autocommit`, the process-wide background committer, started on first use
  by a store built `with_background_autocommit` (the server). Jobs queued while it works
  coalesce into one commit per store root. It is never stopped; callers must call
  `flush_background_commits` before exit (`crates/hotsheet-ticketing/src/store.rs:2269-2330`,
  `crates/hotsheet-ticketing/src/store.rs:2387`). Test: `background_autocommit_commits_after_flush`,
  `queued_commits_coalesce_per_store_with_their_path_union` in `crates/hotsheet-ticketing/src/store.rs`.
- `hotsheet-group-registry-sync`, started by `groups::git_sync::enable_background`;
  `flush_background` waits for it (`crates/hotsheet-ticketing/src/groups/git_sync.rs:297-317`).
  Test: `crates/hotsheet-ticketing/src/groups/git_sync/tests.rs`.
- A headless commit spawns `git push --quiet` and an unnamed reaper thread that waits for
  it; the child outlives a short CLI process (`crates/hotsheet-ticketing/src/store.rs:1112-1150`).
  Test: `committing_write_does_not_wait_for_a_slow_remote_push`.

Locks (all `FileLock` advisory OS locks, released on process exit,
`crates/hotsheet-ticketing/src/file_lock.rs:1-3`):

| Lock                                         | Scope                                        | Held by                                                                                            |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `<store>/.hotsheet-note-mutation.lock`       | One store's read-revise-write                | `with_ticket_transaction` (`crates/hotsheet-ticketing/src/store.rs:570-607`)                       |
| `<git-dir>/hotsheet-autocommit.lock`         | Stage, commit, rebase, branch move           | autocommit, sync local steps, compaction                                                           |
| `<git-dir>/hotsheet-history-sync.lock`       | A whole sync cycle against a history rewrite | `sync_once`, `execute_store`                                                                       |
| `<git-dir>/hotsheet-sync-ai.lock`            | One AI ask per stall                         | `sync_ai`                                                                                          |
| `HOTSHEET_HOME/checkouts.json.lock`          | Checkout registry edit                       | `CheckoutRegistry` (`crates/hotsheet-ticketing/src/checkouts.rs:1186-1210`)                        |
| `providers.json.lock`                        | Provider configuration edit                  | `ProviderConfigRegistry::lock_configuration` (`crates/hotsheet-ticketing/src/provider.rs:291-305`) |
| `HOTSHEET_HOME/settings-locks/<sha256>.lock` | One settings file                            | `Settings` (`crates/hotsheet-ticketing/src/settings_locks.rs:33`)                                  |
| `<checkout>/.hotsheet2/groups.lock`          | Group find-or-create                         | `GroupRegistry::locked` (`crates/hotsheet-ticketing/src/groups/registry.rs:142-160`)               |
| `<index>-open.lock`                          | Index open, init and corruption recreate     | `OpenLock` (`crates/hotsheet-index/src/lib.rs:1475-1510`)                                          |
| `metrics/raw/.usage.lock`                    | Raw usage append and reprice                 | metrics (`crates/hotsheet-ticketing/src/metrics.rs:155-165`)                                       |

Lock order, as the code takes them:

1. history-sync, then note-mutation, then Git transaction, for compaction
   (`crates/hotsheet-ticketing/src/repository_compaction.rs:692-696`);
2. history-sync, then Git transaction, for sync; network steps run with only the
   history-sync lock held (`crates/hotsheet-ticketing/src/sync.rs:277-315`);
3. note-mutation only, for a ticket write. Commits produced inside the transaction are
   queued in a thread-local and run after the note lock is dropped, so the Git
   transaction lock is never taken while the note lock is held
   (`crates/hotsheet-ticketing/src/store.rs:597-606`);
4. two stores in canonical path order for copy and move
   (`crates/hotsheet-ticketing/src/store.rs:612-630`);
5. `checkouts.json.lock`, then `providers.json.lock` (`crates/hotsheet-ticketing/src/checkouts.rs:1788-1802`);
6. key account, then key metadata (`crates/hotsheet-ticketing/src/secrets/metadata_owner.rs:1`).

In-process mutexes use the poison-recovery policy of `hotsheet-sync`
(`crates/hotsheet-sync/src/lib.rs:1-8`). `ProviderRegistry` and `GitProvider` each
hold an in-process transfer mutex; neither spans processes
(`crates/hotsheet-ticketing/src/provider.rs:2401-2408`, `crates/hotsheet-ticketing/src/provider.rs:1545`).
SQLite connections wait up to 30 s on a busy lock (`crates/hotsheet-index/src/lib.rs:32`)
or 5 s for the outbox (`crates/hotsheet-ticketing/src/provider_outbox.rs:308`).

### Failure and recovery

- **Write path.** A ticket write must succeed; its Git commit is best effort and only
  warns (`crates/hotsheet-ticketing/src/store.rs:2183-2188`). Autocommit retries lock
  contention up to 20 times with 25 ms doubling to 250 ms backoff and never removes
  Git's lock (`crates/hotsheet-ticketing/src/store.rs:2442-2461`). It defers, leaving the
  file uncommitted, while a rebase or merge is unfinished
  (`crates/hotsheet-ticketing/src/store.rs:1014-1020`). A panic inside a transaction drops
  its queued commits; the files stay and the next commit picks them up
  (`crates/hotsheet-ticketing/src/store.rs:2196-2240`). Test:
  `transaction_failure_and_panic_do_not_strand_deferred_commits`,
  `autocommit_bounds_lock_retries_and_does_not_retry_other_errors`.
- **Writer format guard.** A store schema newer than 3 is refused; a schema-2 prefix
  store stays writable until explicit activation; any other mix returns
  `FormatActivationRequired` (`crates/hotsheet-ticketing/src/store.rs:779-803`). Test:
  `newer_store_schema_is_rejected_before_writing`.
- **Corrupt tickets.** `list_tickets_resilient` returns healthy tickets and reports
  corrupt files (`crates/hotsheet-ticketing/src/store.rs:1948`); the index skips them
  and keeps the last healthy row (`crates/hotsheet-index/src/lib.rs:938-954`). Test:
  `list_tickets_resilient_survives_multiple_and_varied_bad_files`,
  `a_corrupt_ticket_file_is_skipped_not_fatal_on_rebuild_and_reconcile`.
- **Concurrency token.** A Git provider update with a stale `expected_token` fails with
  `ticket changed since it was read` (`crates/hotsheet-ticketing/src/provider.rs:1857-1867`).
  The check runs before `ops::update` takes the ticket transaction (Discrepancies, D5).
- **Sync.** Outcomes are `NoRemote`, `UpToDate`, `Synced`, `Offline`, `PushRaced`,
  `Rejected`, `Conflict`, `Blocked`, `RebaseInProgress` and `HistoryRecoveryRequired`
  (`crates/hotsheet-ticketing/src/sync.rs:66-99`). Push and fetch races retry up to
  `PUSH_RACE_RETRIES = 4` times with jittered pauses (`crates/hotsheet-ticketing/src/sync.rs:119`).
  Replays retry up to 3 times when writes keep touching incoming paths
  (`crates/hotsheet-ticketing/src/sync_replay.rs:34`); an unfinished rebase is
  continued at most 3 times (`crates/hotsheet-ticketing/src/sync_replay.rs:31`). A
  conflict is never replayed again for the same `(HEAD, upstream)` pair
  (`crates/hotsheet-ticketing/src/sync.rs:489-491`). Kept: all local commits and the
  working tree. Lost: nothing; publication waits.
- **Outbox.** Every transition is a guarded `UPDATE … WHERE state AND dispatch_state`;
  a mismatch is `InvalidTransition` (`crates/hotsheet-ticketing/src/provider_outbox.rs:715-813`).
- **Transfer.** Unsupported fields fail before the destination exists
  (`crates/hotsheet-ticketing/src/provider.rs:2468-2546`). After creation, a later
  failure leaves a partial destination; a retry with the same operation id reuses the
  same derived ids (`crates/hotsheet-ticketing/src/provider.rs:2443-2450`). A move whose
  source close fails reports `SourceCloseFailed` naming the destination
  (`crates/hotsheet-ticketing/src/provider.rs:2709-2714`). Test:
  `move_reports_created_destination_and_retry_recovers_after_source_close_failure`.

### Configuration and environment

| Variable or key                                                                                                                                  | Default                                                  | Read at                                                                                                                                                                                             |
| ------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `HOTSHEET_HOME`                                                                                                                                  | `~/.hotsheet2`, else `<temp>/.hotsheet2`; blank is unset | `crates/hotsheet-ticketing/src/machine_home.rs:20-35`                                                                                                                                               |
| `HOTSHEET_NO_AUTOCOMMIT`                                                                                                                         | unset (commits on)                                       | `crates/hotsheet-ticketing/src/store.rs:967`, `crates/hotsheet-ticketing/src/groups/git_sync.rs:589`                                                                                                |
| `HOTSHEET_NATIVE_SECRETS=deny`                                                                                                                   | unset (keychain allowed)                                 | `crates/hotsheet-ticketing/src/secrets.rs:209-217`                                                                                                                                                  |
| `HOTSHEET_API_KEY_<PROVIDER>`                                                                                                                    | unset                                                    | `crates/hotsheet-ticketing/src/secrets.rs:533-538`, name rule `crates/hotsheet-ticketing/src/secrets.rs:592`                                                                                        |
| `HOTSHEET_GITHUB_APP_SLUG`, `HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS`, `HOTSHEET_GITHUB_APP_CLIENT_ID`, `HOTSHEET_GITHUB_ENTERPRISE_APP_CLIENT_IDS` | bundled values                                           | `crates/hotsheet-extsync/src/github_app_config.rs:8-12`, `crates/hotsheet-extsync/src/github_app_config.rs:70-74`                                                                                   |
| `CODEX_SESSION_ID`, `HOTSHEET_WORKER_ID`, `HOTSHEET_PROJECT`, `HOTSHEET_USAGE_STORES`, `CODEX_HOME`                                              | unset; `CODEX_HOME` falls back to `~/.codex`             | `crates/hotsheet-ticketing/src/metrics/codex_work.rs:172-200`                                                                                                                                       |
| `HOTSHEET_JIRA_WRITE_BEHIND=1` (server)                                                                                                          | unset (no outbox)                                        | `crates/hotsheet-server/src/main.rs:394`                                                                                                                                                            |
| `PATH`, `HOME`                                                                                                                                   | process environment                                      | `crates/hotsheet-ticketing/src/git.rs:173`, `crates/hotsheet-ticketing/src/machine_home.rs:40`                                                                                                      |
| Settings `ticket_store_sync.ask_ai`                                                                                                              | `start`                                                  | `crates/hotsheet-ticketing/src/sync_ai.rs:40`                                                                                                                                                       |
| Settings `trash_cleanup_days`, `auto_group_new_tickets`                                                                                          | 30, true                                                 | `crates/hotsheet-ticketing/src/settings.rs:33-39`                                                                                                                                                   |
| Settings `features.<id>`, terminal limits, `host_health.*`, `standby.*`                                                                          | per definition                                           | `crates/hotsheet-ticketing/src/features.rs:1`, `crates/hotsheet-ticketing/src/terminal_limits.rs:1`, `crates/hotsheet-ticketing/src/host_health.rs:1`, `crates/hotsheet-ticketing/src/standby.rs:1` |

Every `git` child is built by `git::command`, which strips repository-locating
variables such as `GIT_DIR` and `GIT_INDEX_FILE`, sets `GIT_OPTIONAL_LOCKS=0` and gives
Git an empty stdin (`crates/hotsheet-ticketing/src/git.rs:1-60`). Test:
`crates/hotsheet-ticketing/tests/no_bare_git_spawn.rs`. Variables named
`HOTSHEET_*_LIVE_*` and `HOTSHEET_*_TEST_*` are read only by tests.

### Dynamic views

#### HS2-Core-GitSyncConflict

1. The server sync loop (or `hotsheet-cli sync`) calls `sync_once`; a store without a
   remote, or a designated root, returns `NoRemote` (`crates/hotsheet-ticketing/src/sync.rs:187-203`).
2. Under the history-sync and Git transaction locks the cycle refuses a compaction
   recovery state, reports held Git locks as `Blocked` after moving provably stale ones
   aside, continues or refuses an unfinished rebase, and commits pending writes
   (`crates/hotsheet-ticketing/src/sync.rs:261-315`).
3. It registers the merge driver and the `-text` attribute in the private Git
   directory (`crates/hotsheet-ticketing/src/merge_driver.rs:173`,
   `crates/hotsheet-ticketing/src/line_endings.rs:92`).
4. `git fetch` runs with only the history-sync lock held. A failure is `Offline`, a
   lock failure is `Blocked`, a lost tracking-ref race retries
   (`crates/hotsheet-ticketing/src/sync.rs:378-405`).
5. When behind, `integrate` replays local commits with `git merge-tree` and
   `git commit-tree` outside the working tree, or falls back to `git rebase`
   (`crates/hotsheet-ticketing/src/sync.rs:478-516`, `crates/hotsheet-ticketing/src/sync_replay.rs:75`).
6. Git runs `hotsheet-cli merge-driver` per conflicting ticket. It parses the three
   versions and calls `merge_tickets`: scalars take the changed side or, when both
   changed, the newer `updated_at` (ties keep ours); tags, blockers and assignees are
   3-way unions; notes union by ULID; a body changed on both sides goes to
   `git merge-file`, and markers make the driver exit 1
   (`crates/hotsheet-ticketing/src/merge.rs:57-59`, `crates/hotsheet-ticketing/src/merge.rs:362`,
   `crates/hotsheet-ticketing/src/merge.rs:407`, `crates/hotsheet-cli/src/dispatch/store.rs:1087-1103`).
   Unparseable input is text-merged whole (`crates/hotsheet-cli/src/dispatch/store.rs:1078-1084`).
7. On conflict the isolated replay is simply not applied; a live rebase is aborted.
   The cycle reports `Conflict` with the heads, which sync health records so the same
   pair is not replayed again (`crates/hotsheet-ticketing/src/sync.rs:576-600`).
8. The caller runs `sync_ai::ask_for_help`, which creates one deduplicated Up Next
   repair ticket and, by `ticket_store_sync.ask_ai`, starts or prompts an AI session
   (`crates/hotsheet-ticketing/src/sync_ai.rs:651`).

Test: `hs2_qr1k8r_a_conflict_leaves_no_rebase_and_the_working_tree_untouched` in
`crates/hotsheet-ticketing/src/sync/stall_tests.rs`, `crates/hotsheet-ticketing/src/sync/merge_driver_tests.rs`,
`crates/hotsheet-ticketing/tests/merge_proptest.rs`, `crates/hotsheet-ticketing/src/sync_ai/tests.rs`.

#### HS2-Core-IndexRebuild

1. `open_reconciled` takes the `-open.lock` sidecar, opens the file in WAL mode, and
   deletes and recreates it only for `SQLITE_CORRUPT` or `SQLITE_NOTADB`
   (`crates/hotsheet-index/src/lib.rs:286-308`, `crates/hotsheet-index/src/lib.rs:35-45`).
2. `init` checks `schema_version` inside an IMMEDIATE transaction; any other value
   drops every table and recreates the schema (`crates/hotsheet-index/src/lib.rs:312-352`).
3. `reconcile_with_invalidations` asks the store for its Git baseline: the owning
   repository key, HEAD and dirty ticket ids (`crates/hotsheet-index/src/lib.rs:760-800`,
   `crates/hotsheet-ticketing/src/store_git.rs:440`).
4. If the stored `head` and `git_owner` match, only the union of committed changes,
   current dirty ids and previous dirty ids is re-read. A missing baseline, an owner
   change or a HEAD the repository no longer has takes the full walk
   (`crates/hotsheet-index/src/lib.rs:782-805`).
5. Each candidate file is read and SHA-256 hashed; only a changed hash is parsed
   and upserted. A parse failure records the new hash, keeps the old row, and sets
   `corrupt_source_changed` so the watcher still invalidates clients
   (`crates/hotsheet-index/src/lib.rs:819-852`, `crates/hotsheet-index/src/lib.rs:912-975`).
6. Rows whose file is gone are deleted; the new baseline is written; the transaction
   commits (`crates/hotsheet-index/src/lib.rs:871-900`, `crates/hotsheet-index/src/lib.rs:960-975`, `crates/hotsheet-index/src/lib.rs:752-756`).

Drift detection is therefore hash-based and Git-assisted, never time-based. Test:
`a_stale_schema_version_triggers_a_rebuild`, `a_corrupt_index_file_is_deleted_and_rebuilt`,
`open_reconciled_restores_then_picks_up_offline_edits`, `dirty_tree_reconciles_only_changed_ticket_ids`
in `crates/hotsheet-index/src/tests.rs`; `hs2_abv2j3_owner_change_and_legacy_cache_force_a_full_walk`
in `crates/hotsheet-index/src/git_owner_tests.rs`.

#### HS2-Core-OutboxRetry

1. `admit_batch` runs in one IMMEDIATE transaction: it counts queued rows against the
   capacity (1,000 in the server), rejects a changed payload under a known id, returns
   the existing row for an identical retry, and assigns the next per-ticket sequence
   (`crates/hotsheet-ticketing/src/provider_outbox.rs:407-530`,
   `crates/hotsheet-server/src/main.rs:394-398`).
2. A Tokio task loops every 2 s (`crates/hotsheet-server/src/provider_write_behind.rs:288-297`).
   `claim_ready` marks at most four rows `sending`, only the lowest-sequence queued row
   of each ticket (`crates/hotsheet-ticketing/src/provider_outbox.rs:679-713`).
3. Dispatch reads the issue; if the fields already match it confirms. Otherwise it
   records changed remote values as a conflict and updates with the current token
   (`crates/hotsheet-server/src/provider_write_behind.rs:303-391`).
4. `RateLimited` defers the row and the whole connection until `Retry-After`, default
   30 s (`crates/hotsheet-server/src/provider_write_behind.rs:395-410`).
5. Authentication, disabled, not found and unsupported errors, and every eighth
   attempt, move the row to `needs_attention`; a person retries or discards it. Other
   errors back off `2^min(attempts,6)` seconds plus 0–2 s (`crates/hotsheet-server/src/provider_write_behind.rs:411-438`).
6. On open, rows left `sending` by a crash return to `queued`; settled payloads older
   than 30 days are compacted, and settled rows are pruned
   (`crates/hotsheet-ticketing/src/provider_outbox.rs:383-403`).

Test: `dispatch_claims_one_per_ticket_and_survives_rate_limit_attention_and_restart`,
`batch_is_atomic_and_backpressure_counts_only_new_pending_intents` in
`crates/hotsheet-ticketing/src/provider_outbox.rs`; `crates/hotsheet-server/tests/jira_outbox_http.rs`.

#### HS2-Core-CrossProviderTransfer

1. A user-initiated `copy_between` or `move_between` needs a non-empty operation id and
   takes the registry's in-process transfer lock (`crates/hotsheet-ticketing/src/provider.rs:2452-2463`).
2. The source ticket is read; crop metadata, dependencies, and notes, attachments,
   assignees, reviews, edited notes, confidence or AI feedback the destination cannot
   represent fail with `UnsupportedField` before anything is created
   (`crates/hotsheet-ticketing/src/provider.rs:2466-2546`).
3. The destination `create` receives `TransferProvenance` and a ULID derived from
   SHA-256 of the operation id. A Git destination returns the earlier ticket for the
   same operation id and source, or refuses a different source
   (`crates/hotsheet-ticketing/src/provider.rs:1800-1814`).
4. The Git adapter writes the ticket, then records `transfer_operation_id` and
   `transferred_from` in a second committed write (`crates/hotsheet-ticketing/src/provider.rs:1832-1844`).
5. Notes are copied in request-before-reply order with derived ids; AI-feedback targets
   are remapped; attachments and assignment follow (`crates/hotsheet-ticketing/src/provider.rs:2547-2680`).
6. A move then closes the source as `obsolete` (`crates/hotsheet-ticketing/src/provider.rs:2693-2716`).

Test: `concurrent_transfer_retries_create_one_destination_and_move_closes_source`,
`transfer_rejects_attachments_before_creating_the_destination_ticket`,
`transfer_operation_id_cannot_collide_across_sources` in `crates/hotsheet-ticketing/src/provider.rs`.

Same-store-kind copy and move between two Git stores use `ops::copy_ticket` (new ULID,
`copied_from`) and `ops::move_ticket` (same ULID, a `moved` tombstone in the source,
Git history keeps the old copy) (`crates/hotsheet-ticketing/src/ops.rs:2480-2600`).

### Discrepancies

Each discrepancy names the follow-up ticket filed from HS2-AH9HAK.

- **D1.** `docs/17-ticket-file-format.md` omits the `moved_at` frontmatter field that the
  model writes on a move tombstone (`crates/hotsheet-model/src/format.rs:64`,
  `crates/hotsheet-model/src/ticket.rs:874-875`). Follow-up: HS2-7F8DGD.
- **D2.** `docs/17-ticket-file-format.md` §17.4 says an invalid enum value degrades to a
  kept value plus a note. The enums have no fallback variant, so an unknown `status` or
  `priority` fails the parse with `ParseError::Yaml` and the ticket is reported corrupt
  (`crates/hotsheet-model/src/enums.rs:20-21`, `crates/hotsheet-model/src/format.rs:188`). Follow-up: HS2-7F8DGD.
- **D3.** `docs/03-indexing-and-query.md` §3.3 shows a `blocked_by` table and
  `moved_to_store`, `git_blob_oid` columns that the schema lacks, and omits
  `phase_spans`, group, creator and attachment columns and two FTS columns
  (`crates/hotsheet-index/src/lib.rs:60-112`). Follow-up: HS2-N9SNDJ.
- **D4.** `move_between` does not check the source's `close` capability first. Jira
  reports `close: false` and its `close` returns `Unsupported`, so every move from Jira
  creates the destination and then fails with `SourceCloseFailed`
  (`crates/hotsheet-extsync/src/jira.rs:1104-1112`, `crates/hotsheet-ticketing/src/provider.rs:2700-2714`). Follow-up: HS2-DAG8P8.
- **D5.** `GitProvider::update` checks `expected_token` against a read made before
  `ops::update` takes the ticket transaction, so a write landing between the check and
  the transaction is overwritten without a conflict
  (`crates/hotsheet-ticketing/src/provider.rs:1857-1875`). Follow-up: HS2-W3JD5K.
- **D6.** `docs/17-ticket-file-format.md` says `claim_eta_at` is cleared with the claim.
  `ops::copy_ticket` clears claim fields but not `claim_eta_at`, and the move tombstone
  clears neither `claim_eta_at` nor `worker_label` (`crates/hotsheet-ticketing/src/ops.rs:2496-2513`,
  `crates/hotsheet-ticketing/src/ops.rs:2562-2569`). Follow-up: HS2-BY7MKY.
- **D7.** Module documentation drift: the crate and ports docs say the engine never
  reads the clock or a global RNG and touches no filesystem or Git directly, but
  `StoreMetadata::new` uses `Ulid::new()` and sync reads the wall clock
  (`crates/hotsheet-ticketing/src/ports.rs:1-4`, `crates/hotsheet-ticketing/src/store.rs:224`,
  `crates/hotsheet-ticketing/src/sync.rs:210`). The secrets module says values live in
  the OS keychain; values are encrypted files and only the master key is native
  (`crates/hotsheet-ticketing/src/secrets.rs:1-7`, `crates/hotsheet-ticketing/src/secrets/machine_secrets.rs:1-4`).
  The compaction module says it is read-only planning but also executes rewrites
  (`crates/hotsheet-ticketing/src/repository_compaction.rs:1-4`,
  `crates/hotsheet-ticketing/src/repository_compaction.rs:685`). Adapter comments say a
  confidence score fails explicitly while each reports `note_confidence: true`
  (`crates/hotsheet-extsync/src/jira.rs:1537-1538`). Follow-up: HS2-1EMNMJ.
- **D8.** `docs/25-write-behind-sync-plan.md` says Phase 3 still owns dispatch,
  confirmation and retry. The server implements them
  (`crates/hotsheet-server/src/provider_write_behind.rs:288`) and
  `docs/16-external-sync-interface.md` describes them. Follow-up: HS2-BNGVKE.

Unresolved claims for this area are in the [register](../unresolved.md) (U-core-1, U-core-2, U-core-3, U-core-4).
