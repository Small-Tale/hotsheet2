# Core ticketing crates: coverage

Status: written by HS2-AH9HAK. Audited source `8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`.
`node scripts/architecture-coverage.mjs inventory --area core` lists the
required entries; every one appears below in backticks. The narrative is
[10-core](../narrative/10-core.md). `HOTSHEET_HOME` means the machine home
(`crates/hotsheet-ticketing/src/machine_home.rs:20-35`).

## Ticket model and file format (`coreModel`)

- Modules: `crates/hotsheet-model/src/format.rs`, `crates/hotsheet-model/src/ticket.rs`,
  `crates/hotsheet-model/src/feedback_choices.rs`; small: `lib.rs` (schema constants),
  `ids.rs`, `enums.rs`, `timestamp.rs`, `group.rs`, `note_doctor.rs`,
  `orchestration_link.rs`, `ticket_ref.rs`.
- Responsibility: ticket types, slug derivation, group-name normalization and the
  file codec. Does not read or write files, mint ids from a clock, or merge tickets.
- State and persistence: none in memory; defines the on-disk ticket format
  (`crates/hotsheet-model/src/format.rs:118-123`) and the guarded `schema` markers
  (`crates/hotsheet-model/src/format.rs:20-24`, `crates/hotsheet-model/src/lib.rs:41-49`).
- Types and entry points: `Ticket` (`crates/hotsheet-model/src/ticket.rs:778`),
  `Note`, `Attachment`, `MediaAnnotation`, `parse_file`, `to_file_string`,
  `ParseError` (`crates/hotsheet-model/src/format.rs:74`), `derive_slug`
  (`crates/hotsheet-model/src/ids.rs:17`), `normalize_group_name`
  (`crates/hotsheet-model/src/group.rs:101`), feedback-choice parsing
  (`crates/hotsheet-model/src/feedback_choices.rs:598`).
- Concurrency: none; pure functions.
- Errors and recovery: malformed input returns `ParseError`, never panics; a single
  malformed note is skipped (`crates/hotsheet-model/src/format.rs:1-11`). An unknown
  guard string is `upgrade_required`, not corruption
  (`crates/hotsheet-model/src/format.rs:100-106`). An unknown enum value fails the
  whole parse (`crates/hotsheet-model/src/format.rs:188`).
- Configuration and environment: none.
- Invariants: round trip is byte-idempotent for a canonical file; unknown keys survive;
  `KNOWN_KEYS` matches the serialized fields
  (`crates/hotsheet-model/src/format.rs:29-71`); feedback drafts never reach the file
  (`crates/hotsheet-model/src/format.rs:147-153`).
- Tests: `crates/hotsheet-model/tests/proptest_format.rs`,
  `crates/hotsheet-model/tests/compatibility_fixtures.rs`,
  `crates/hotsheet-model/tests/feedback_choices.rs`, format tests
  (`known_keys_cover_serialized_fields`, `unknown_guarded_schema_is_rejected_instead_of_downgraded`),
  `crates/hotsheet-model/fuzz`.

## Git file store (`coreStore`)

- Modules: `crates/hotsheet-ticketing/src/store.rs`,
  `crates/hotsheet-ticketing/src/store_git.rs`, `crates/hotsheet-ticketing/src/git.rs`;
  small: `git_discovery.rs` (cached `rev-parse` answers), `file_lock.rs`,
  `corrupt_cache.rs` (stat-validated corrupt-file cache), `slash_path.rs`,
  `identity.rs` (current user is Git `user.email`), `overlay.rs` (gitignored
  `local/reads.json`), `roster.rs` (committed `people.json`), `registry.rs`
  (multi-store ULID resolution following `moved` tombstones), `ports.rs`.
- Responsibility: on-disk store layout, atomic ticket and attachment writes, the
  ticket transaction, local path-scoped commits and headless push, store admission
  and Git ownership. Does not fetch, rebase or index.
- State and persistence: see the narrative's durable-state table. Ticket path by
  shard (`crates/hotsheet-ticketing/src/store.rs:705-735`); metadata
  (`crates/hotsheet-ticketing/src/store.rs:157-171`); managed ignores
  (`crates/hotsheet-ticketing/src/store.rs:66-77`); in-memory headless-published
  store list (`crates/hotsheet-ticketing/src/store.rs:2360-2385`).
- Types and entry points: `FsStore` (`crates/hotsheet-ticketing/src/store.rs:417`),
  `init`, `open`, `write_ticket_committing`, `with_ticket_transaction`
  (`crates/hotsheet-ticketing/src/store.rs:570`), `find_ticket_by_slug`
  (`crates/hotsheet-ticketing/src/store.rs:1190`), `list_tickets_resilient`,
  `write_ticket_with_attachments_atomic` (`crates/hotsheet-ticketing/src/store.rs:1439`),
  `StoreGitOwner`, `admit_store` (`crates/hotsheet-ticketing/src/store_git.rs:193`),
  `git::command` (`crates/hotsheet-ticketing/src/git.rs:77`).
- Concurrency: note-mutation file lock per store; Git transaction lock around every
  stage and commit; deferred commits run after the note lock drops; process-wide
  `hotsheet-autocommit` thread; headless push child plus reaper thread
  (`crates/hotsheet-ticketing/src/store.rs:2269-2330`, `crates/hotsheet-ticketing/src/store.rs:1112-1150`).
- Errors and recovery: `StoreError` (`crates/hotsheet-ticketing/src/store.rs:246`);
  commit failures warn only; lock contention retried 20 times
  (`crates/hotsheet-ticketing/src/store.rs:2442-2461`); commits deferred during an
  unfinished rebase (`crates/hotsheet-ticketing/src/store.rs:1014-1020`); compaction
  recovery blocks writes (`crates/hotsheet-ticketing/src/store.rs:805-811`); legacy
  tracked lock files are untracked by a one-time repository-wide commit
  (`crates/hotsheet-ticketing/src/store.rs:1031-1083`).
- Configuration and environment: `HOTSHEET_NO_AUTOCOMMIT`
  (`crates/hotsheet-ticketing/src/store.rs:967`); `PATH` for the Git binary
  (`crates/hotsheet-ticketing/src/git.rs:173`); builder flags `with_deferred_push`,
  `with_deferred_autocommit`, `with_background_autocommit`.
- Invariants: writes are temp-and-rename (`crates/hotsheet-ticketing/src/store.rs:2141-2160`);
  inactive statuses never keep `up_next` (`crates/hotsheet-ticketing/src/store.rs:759-761`);
  the schema guard is raised to the lowest version that represents the ticket
  (`crates/hotsheet-ticketing/src/store.rs:98-140`); a designated root never pushes or
  commits outside itself; a bare `Command::new("git")` is forbidden outside `git.rs`.
- Tests: `crates/hotsheet-ticketing/src/store.rs` (`concurrent_readers_never_observe_a_partial_ticket_rewrite`,
  `note_transaction_serializes_ratings_across_processes`, `autocommit_replays_lock_collisions_until_success_or_another_writers_noop`),
  `crates/hotsheet-ticketing/tests/store_admission_policy.rs`,
  `crates/hotsheet-ticketing/tests/no_bare_git_spawn.rs`,
  `crates/hotsheet-ticketing/tests/file_lock_process.rs`,
  `crates/hotsheet-ticketing/src/git_discovery_tests.rs`.

## Ticket operations (`coreOps`)

- Modules: `crates/hotsheet-ticketing/src/ops.rs`, `crates/hotsheet-ticketing/src/worklist.rs`;
  small: `ops/` (claim tests), `actor.rs` (actor roles and AI completion rules),
  `completion_remote.rs` (AI completions must have remote commits),
  `auto_context.rs`.
- Responsibility: every ticket mutation and the file-scan query path; worklist
  projection. Does not choose a provider, publish events, or own claim backends
  (`claim_gate` belongs to orchestration).
- State and persistence: none of its own; writes tickets through `coreStore`;
  `worklist.md` at `<checkout>/.hotsheet2/worklist.md`
  (`crates/hotsheet-ticketing/src/worklist.rs:1-9`).
- Types and entry points: `query` (`crates/hotsheet-ticketing/src/ops.rs:273`),
  `resolve` (`crates/hotsheet-ticketing/src/ops.rs:468`), `create_by`
  (`crates/hotsheet-ticketing/src/ops.rs:743`), `update`
  (`crates/hotsheet-ticketing/src/ops.rs:810`), `add_note_with_metadata`
  (`crates/hotsheet-ticketing/src/ops.rs:1469`), `close`
  (`crates/hotsheet-ticketing/src/ops.rs:2424`), `copy_ticket`, `move_ticket`
  (`crates/hotsheet-ticketing/src/ops.rs:2480-2600`), `restore`, `purge_trash`
  (`crates/hotsheet-ticketing/src/ops.rs:1311`, `crates/hotsheet-ticketing/src/ops.rs:1360`),
  `claim_with_eta` (`crates/hotsheet-ticketing/src/ops.rs:3026`), `release`
  (`crates/hotsheet-ticketing/src/ops.rs:3185`), `regenerate_checkout`
  (`crates/hotsheet-ticketing/src/worklist.rs:238`).
- Concurrency: one note-mutation transaction per mutation; copy and move take two
  stores in canonical order; `claim_next_across_with_eta` retries up to 16 candidates
  (`crates/hotsheet-ticketing/src/ops.rs:2940-2951`).
- Errors and recovery: `OpError` (`crates/hotsheet-ticketing/src/ops.rs:23-60`);
  validation before the transaction so invalid input writes nothing; claim ETAs are
  capped at 14 days (`crates/hotsheet-ticketing/src/ops.rs:2701`).
- Configuration and environment: none read directly; time and ids injected.
- Invariants: one implementation for all surfaces (`crates/hotsheet-ticketing/src/ops.rs:1-6`);
  a move keeps the ULID and leaves a `moved` tombstone; a copy gets a new ULID and
  `copied_from`.
- Tests: `crates/hotsheet-ticketing/src/ops.rs` tests module,
  `crates/hotsheet-ticketing/src/ops/claim_next_across_tests.rs`,
  `crates/hotsheet-ticketing/src/ops/claim_blocked_tests.rs`,
  `crates/hotsheet-ticketing/tests/completion_remote_policy.rs`,
  `crates/hotsheet-ticketing/src/worklist.rs` tests.

## Wire DTOs and checkout ordering (`coreWire`)

- Modules: `crates/hotsheet-ticketing/src/wire.rs`,
  `crates/hotsheet-ticketing/src/checkout_page.rs`,
  `crates/hotsheet-ticketing/src/saved_views.rs`; small: `checkout_order.rs`.
- Responsibility: the JSON shape of tickets and rows, one global multi-source sort
  order, the v2 page cursor and filter fingerprint, and the saved-view query language
  for a single ticket. Does not fetch rows itself; callers supply fetch closures.
- State and persistence: none; saved views live in shared settings key `views`
  (`crates/hotsheet-ticketing/src/saved_views.rs:1-6`).
- Types and entry points: `ApiTicket`, `TicketRow`, `ApiNote`
  (`crates/hotsheet-ticketing/src/wire.rs:1-12`); `compare`
  (`crates/hotsheet-ticketing/src/checkout_order.rs:151`); `encode_cursor`,
  `decode_cursor`, `merge_page` (`crates/hotsheet-ticketing/src/checkout_page.rs:131-272`).
- Concurrency: none.
- Errors and recovery: a cursor bound to another filter set is rejected by its
  fingerprint (`crates/hotsheet-ticketing/src/checkout_page.rs:1-12`).
- Configuration and environment: none; read caps from `CHECKOUT_READ_MAX_ROWS = 500`
  (`crates/hotsheet-ticketing/src/checkout_order.rs:16`).
- Invariants: a Git ticket's `concurrency_token` is its `updated_at`
  (`crates/hotsheet-ticketing/src/wire.rs:283`).
- Tests: `crates/hotsheet-ticketing/src/wire.rs`, `crates/hotsheet-ticketing/src/checkout_page.rs`,
  `crates/hotsheet-ticketing/src/saved_views.rs` tests modules.

## Store sync (`coreSync`)

- Modules: `crates/hotsheet-ticketing/src/sync.rs`, `crates/hotsheet-ticketing/src/sync_replay.rs`,
  `crates/hotsheet-ticketing/src/sync_health.rs`, `crates/hotsheet-ticketing/src/sync_stale_locks.rs`,
  `crates/hotsheet-ticketing/src/sync_ai.rs`; small: `sync_locks.rs`,
  `sync_saved_writes.rs`, `sync/` (tests only).
- Responsibility: one fetch, integrate and push cycle; health reporting; stall
  recovery and AI help. Does not choose cadence: the server loop or the CLI calls it.
- State and persistence: `<git-dir>/hotsheet-sync-health.json`
  (`crates/hotsheet-ticketing/src/sync_health.rs:40`), `<git-dir>/hotsheet-sync-ai.json`
  (`crates/hotsheet-ticketing/src/sync_ai.rs:67`), backup refs
  (`crates/hotsheet-ticketing/src/sync_saved_writes.rs:148`), moved locks
  (`crates/hotsheet-ticketing/src/sync_stale_locks.rs:40`).
- Types and entry points: `sync_once` (`crates/hotsheet-ticketing/src/sync.rs:183`),
  `SyncReport` (`crates/hotsheet-ticketing/src/sync.rs:66`), `health`
  (`crates/hotsheet-ticketing/src/sync_health.rs:279`), `abort_stalled_integration`
  (`crates/hotsheet-ticketing/src/sync.rs:901`), `adopt_resolved_history`
  (`crates/hotsheet-ticketing/src/sync.rs:1045`), `recover_stale_locks`
  (`crates/hotsheet-ticketing/src/sync.rs:1262`), `ask_for_help`
  (`crates/hotsheet-ticketing/src/sync_ai.rs:651`).
- Concurrency: history-sync lock for the whole cycle; Git transaction lock for local
  steps only; fetch and push unlocked (`crates/hotsheet-ticketing/src/sync.rs:261-315`);
  `hotsheet-sync-ai.lock` per stall; one stdin-writer thread in the oversized-object
  scan (`crates/hotsheet-ticketing/src/sync.rs:812`).
- Errors and recovery: push and fetch races retried 4 times
  (`crates/hotsheet-ticketing/src/sync.rs:119`); held locks `Blocked`, stale ones older
  than 10 minutes moved aside when no process holds the Git directory
  (`crates/hotsheet-ticketing/src/sync_stale_locks.rs:37`); refused pushes `Rejected`
  with oversized objects named; conflicts recorded and not replayed; unfinished
  rebases continued up to 3 times (`crates/hotsheet-ticketing/src/sync_replay.rs:31`).
- Configuration and environment: setting `ticket_store_sync.ask_ai` (`start`,
  `session`, `off`) (`crates/hotsheet-ticketing/src/sync_ai.rs:40-80`).
- Invariants: never commits onto a detached rebase `HEAD`; never deletes a Git lock;
  a designated root is never synced (`crates/hotsheet-ticketing/src/sync.rs:187-195`).
- Tests: `crates/hotsheet-ticketing/src/sync/stall_tests.rs`,
  `crates/hotsheet-ticketing/src/sync/push_race_tests.rs`,
  `crates/hotsheet-ticketing/src/sync/stale_lock_tests.rs`,
  `crates/hotsheet-ticketing/src/sync/saved_writes_tests.rs`,
  `crates/hotsheet-ticketing/src/sync/lock_tests.rs`,
  `crates/hotsheet-ticketing/src/sync_stale_locks/tests.rs`,
  `crates/hotsheet-ticketing/src/sync_ai/tests.rs`, sync.rs tests
  (`concurrent_autocommits_and_sync_cycles_never_lose_or_fail_a_write`).

## Semantic ticket merge (`coreMerge`)

- Modules: `crates/hotsheet-ticketing/src/merge.rs`; small: `merge_driver.rs`,
  `line_endings.rs`.
- Responsibility: the pure 3-way merge and the local Git registration that makes Git
  call it. The effectful driver is `hotsheet-cli merge-driver`
  (`crates/hotsheet-cli/src/dispatch/store.rs:1087-1103`).
- State and persistence: `<git-dir>/info/attributes` line
  `tickets/**/*.md merge=hotsheet-ticket` and the local driver command
  (`crates/hotsheet-ticketing/src/merge_driver.rs:20-23`); `-text` for every store path
  (`crates/hotsheet-ticketing/src/line_endings.rs:35`).
- Types and entry points: `merge_tickets`, `MergeOutcome`, `BodyMerge`
  (`crates/hotsheet-ticketing/src/merge.rs:27-57`); `ensure_registered`
  (`crates/hotsheet-ticketing/src/merge_driver.rs:173`); `ensure_pinned`
  (`crates/hotsheet-ticketing/src/line_endings.rs:92`); `set_program` records the CLI
  path once per process (`crates/hotsheet-ticketing/src/merge_driver.rs:29`).
- Concurrency: none in the merge; registration is idempotent.
- Errors and recovery: a body changed on both sides is returned as `BodyMerge::Conflict`
  for a text merge; a tie in `updated_at` keeps ours (`crates/hotsheet-ticketing/src/merge.rs:59`).
- Configuration and environment: none.
- Invariants: notes union by ULID; claim history unions by event id; never a conflict
  on a scalar (`crates/hotsheet-ticketing/src/merge.rs:1-19`).
- Tests: `crates/hotsheet-ticketing/tests/merge_proptest.rs`, merge.rs tests
  (`both_changed_a_scalar_newer_updated_at_wins`, `notes_union_by_id_sorted`),
  `crates/hotsheet-ticketing/src/sync/merge_driver_tests.rs`,
  `crates/hotsheet-ticketing/src/line_endings_tests.rs`.

## Search index (`coreIndex`)

- Modules: `crates/hotsheet-index/src/lib.rs`; small: `prune.rs` (removes unheld older
  generations), `resolve.rs` (index-first slug resolution with `IndexFreshness`).
- Responsibility: the disposable projection, delta reconcile and bounded queries. Does
  not watch files; the server watcher calls it.
- State and persistence: `HOTSHEET_HOME/index/<hash16>.v26.sqlite`, WAL, plus
  `-open.lock` (`crates/hotsheet-index/src/lib.rs:26`, `crates/hotsheet-index/src/lib.rs:53`);
  schema (`crates/hotsheet-index/src/lib.rs:60-112`); `index_meta` keys `schema_version`,
  `head`, `git_owner`, `dirty_ticket_ids`.
- Types and entry points: `Index` (`crates/hotsheet-index/src/lib.rs:170`),
  `open_reconciled` (`crates/hotsheet-index/src/lib.rs:286`), `rebuild_from_store`
  (`crates/hotsheet-index/src/lib.rs:684`), `reconcile_with_invalidations`, `query`
  (`crates/hotsheet-index/src/lib.rs:1010`), `prune_stale_index_files`
  (`crates/hotsheet-index/src/prune.rs:141`), `resolve_ticket_with_freshness`
  (`crates/hotsheet-index/src/resolve.rs:99`).
- Concurrency: `-open.lock` around open and recreate; IMMEDIATE transactions for init and
  reconcile; 30 s busy timeout (`crates/hotsheet-index/src/lib.rs:32`). A full rebuild
  uses a deferred transaction (`crates/hotsheet-index/src/lib.rs:689-694`).
- Errors and recovery: only `SQLITE_CORRUPT` and `SQLITE_NOTADB` delete the file
  (`crates/hotsheet-index/src/lib.rs:35-45`); unparseable tickets keep their prior row.
- Configuration and environment: path from the host (`crates/hotsheet-cli/src/lib.rs:82-89`).
- Invariants: files win; a miss falls back to a store walk unless the watcher reports
  the index in sync (`crates/hotsheet-index/src/resolve.rs:1-12`).
- Tests: `crates/hotsheet-index/src/tests.rs`, `crates/hotsheet-index/src/git_owner_tests.rs`.

## Provider contract and transfer (`coreProviderContract`)

- Modules: `crates/hotsheet-ticketing/src/provider.rs` (trait, registry, transfer);
  small: `provider_trash.rs`, `provider/source_observation.rs`.
- Responsibility: provider-neutral contract, capability discovery, routing, paging
  helpers, Trash purge and cross-provider copy and move. Does not hold credentials.
- State and persistence: in-memory `ProviderRegistry` maps and transfer locks
  (`crates/hotsheet-ticketing/src/provider.rs:2357-2408`).
- Types and entry points: `TicketProvider` (`crates/hotsheet-ticketing/src/provider.rs:1135`),
  `ProviderCapabilities` (`crates/hotsheet-ticketing/src/provider.rs:450`),
  `ProviderError` (`crates/hotsheet-ticketing/src/provider.rs:1076`), `TicketRef`,
  `copy_between`, `move_between` (`crates/hotsheet-ticketing/src/provider.rs:2452`,
  `crates/hotsheet-ticketing/src/provider.rs:2693`), `keyset_page_from_rows`
  (`crates/hotsheet-ticketing/src/provider.rs:765`), `purge_provider_trash`.
- Concurrency: poison-recovering mutexes; per-operation-id transfer mutex in process.
- Errors and recovery: `TransferError` (`crates/hotsheet-ticketing/src/provider.rs:2424-2441`);
  unsupported fields fail before creation; derived ids make retries idempotent; Trash
  purge never reports an unconfirmed removal (`crates/hotsheet-ticketing/src/provider_trash.rs:1-10`).
- Configuration and environment: none.
- Invariants: route by `(connection_id, native_id)`; clients read capabilities, never
  the provider id; transfers require a non-empty operation id.
- Tests: provider.rs tests (`concurrent_transfer_retries_create_one_destination_and_move_closes_source`,
  `transfer_rejects_source_fields_the_destination_cannot_represent`,
  `only_expected_token_conflicts_are_changed_since_read`).

## Git provider adapter (`coreGitProvider`)

- Modules: `crates/hotsheet-ticketing/src/provider.rs` (`GitProvider`, lines 1537-2355).
- Responsibility: expose a Git store through `TicketProvider`. Does not sync.
- State and persistence: the wrapped `FsStore`; an injected resolver
  (`crates/hotsheet-ticketing/src/provider.rs:1537-1585`).
- Types and entry points: `GitProvider::new`, `with_resolver`, `git_connection_id`
  (`crates/hotsheet-ticketing/src/provider.rs:51`), `ProviderCapabilities::git`
  (`crates/hotsheet-ticketing/src/provider.rs:530`).
- Concurrency: an in-process transfer mutex serializes transfer creates.
- Errors and recovery: stale token is `Conflict` (`crates/hotsheet-ticketing/src/provider.rs:1857-1867`);
  `find_transfer` walks every ticket (`crates/hotsheet-ticketing/src/provider.rs:1777-1785`).
- Configuration and environment: none.
- Invariants: native id is the ULID text; connection id is a path hash and changes if
  the store moves.
- Tests: provider.rs tests (`git_provider_conforms_to_create_get_query_update_note_close`,
  `git_provider_reports_not_working_with_note_evidence_and_reopen_in_one_write`).

## Provider connection configuration (`coreProviderConfig`)

- Modules: `crates/hotsheet-ticketing/src/provider`,
  `crates/hotsheet-ticketing/src/provider/machine_config.rs`,
  `crates/hotsheet-ticketing/src/github_endpoint`,
  `crates/hotsheet-ticketing/src/github_endpoint/registry.rs`,
  `crates/hotsheet-ticketing/src/github_endpoint/state.rs`; small: `github_endpoint/mod.rs`.
- Responsibility: one machine-wide connection registry and its migration; fenced GitHub
  endpoint decisions stored under the reserved `github_endpoint` settings key
  (`crates/hotsheet-ticketing/src/github_endpoint/state.rs:10`). No secret values.
- State and persistence: `HOTSHEET_HOME/providers.json`, migration ledger
  `providers.migration.json`, legacy `stores/<instance id>/`
  (`crates/hotsheet-ticketing/src/provider/machine_config.rs:35-41`,
  `crates/hotsheet-ticketing/src/provider/machine_config.rs:159`).
- Types and entry points: `ProviderConfigRegistry` (`crates/hotsheet-ticketing/src/provider.rs:114`),
  `update`, `update_connection`, `migrate_machine_provider_config`, `EndpointRegistry`
  (`crates/hotsheet-ticketing/src/github_endpoint/registry.rs:106`).
- Concurrency: `providers.json.lock` sibling lock for every read-modify-write
  (`crates/hotsheet-ticketing/src/provider.rs:273-305`).
- Errors and recovery: invalid JSON is a `Conflict`, never silently rewritten; atomic
  publish with fsync preserves the file mode (`crates/hotsheet-ticketing/src/provider.rs:163-197`).
- Configuration and environment: `HOTSHEET_HOME`.
- Invariants: at most one default connection; settings carry only credential references.
- Tests: `crates/hotsheet-ticketing/src/provider/machine_config/tests.rs`,
  `crates/hotsheet-ticketing/src/github_endpoint/tests.rs`,
  `crates/hotsheet-ticketing/src/github_endpoint/writer_tests.rs`.

## Provider outbox (`coreOutbox`)

- Modules: `crates/hotsheet-ticketing/src/provider_outbox.rs`.
- Responsibility: durable admission and dispatch state for provisional external edits.
  Does not call a provider; the server dispatcher does
  (`crates/hotsheet-server/src/provider_write_behind.rs:262-297`).
- State and persistence: tables `provider_outbox`, `provider_outbox_sequence`,
  `provider_outbox_retention`; WAL, `synchronous=FULL`
  (`crates/hotsheet-ticketing/src/provider_outbox.rs:300-405`).
- Types and entry points: `ProviderOutbox::open`, `admit_batch`, `claim_ready`,
  `confirm`, `settle_applied`, `defer`, `defer_connection`, `needs_attention`, `retry`,
  `discard`, `project_pending_ticket` (`crates/hotsheet-ticketing/src/provider_outbox.rs:193`).
- Concurrency: IMMEDIATE transactions for admission and claiming; the server wraps the
  connection in a mutex.
- Errors and recovery: `OutboxError` (`crates/hotsheet-ticketing/src/provider_outbox.rs:20-60`);
  rows left `sending` return to `queued` on open; payloads compacted after 30 days
  (`crates/hotsheet-ticketing/src/provider_outbox.rs:269-271`).
- Configuration and environment: capacity from the caller (1,000 in the server).
- Invariants: per-ticket order; only the first queued intent per ticket dispatches;
  no credentials stored (`crates/hotsheet-ticketing/src/provider_outbox.rs:1-6`).
- Tests: provider_outbox.rs tests, `crates/hotsheet-server/tests/jira_outbox_http.rs`.

## GitHub Issues adapter (`coreGithub`)

- Modules: `crates/hotsheet-extsync/src/github.rs`, `crates/hotsheet-extsync/src/github_device.rs`;
  small: `github_app_config.rs`, `github_attachments.rs`, `github_credential.rs`,
  `github_pagination.rs` (validates `Link` targets before reusing auth headers),
  `github_trash.rs`, `github_webhook_setup.rs`.
- Responsibility: GitHub Issues as a provider, device-flow sign-in, token refresh,
  webhook configuration. No mirroring into Git.
- State and persistence: in-memory issue-list cache and rate-limit cooldown per live
  adapter (`crates/hotsheet-extsync/src/github.rs:407-446`); attachments as files in a
  configured assets repository plus a marked comment
  (`crates/hotsheet-extsync/src/github_attachments.rs:1-10`); token bundle in the OS
  credential store (`crates/hotsheet-extsync/src/github_credential.rs:38`).
- Types and entry points: `GitHubConfig` (`crates/hotsheet-extsync/src/github.rs:47`),
  `GitHubProvider`, `GitHubTransport` (`crates/hotsheet-extsync/src/github.rs:153`),
  `GitHubDeviceClient`, `configure_webhook`
  (`crates/hotsheet-extsync/src/github_webhook_setup.rs:23`).
- Concurrency: blocking `ureq` calls on the caller's thread; mutex-guarded cache.
- Errors and recovery: status mapping and cooldown (`crates/hotsheet-extsync/src/github.rs:466-569`).
- Configuration and environment: `HOTSHEET_GITHUB_APP_SLUG`,
  `HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS`, `HOTSHEET_GITHUB_APP_CLIENT_ID`,
  `HOTSHEET_GITHUB_ENTERPRISE_APP_CLIENT_IDS` (`crates/hotsheet-extsync/src/github_app_config.rs:8-12`,
  `crates/hotsheet-extsync/src/github_app_config.rs:70-74`).
- Invariants: native ids are issue numbers (`crates/hotsheet-extsync/src/github.rs:2276-2284`);
  `watch` stays false although webhooks exist.
- Tests: github.rs tests, `crates/hotsheet-extsync/src/github_trash_tests.rs`,
  `crates/hotsheet-extsync/src/github_pagination_tests.rs`,
  `crates/hotsheet-extsync/src/github_device_origin_tests.rs`,
  `crates/hotsheet-extsync/src/github_credential/read_tests.rs`;
  `crates/hotsheet-extsync/src/github_live_tests.rs` (opt-in live).

## GitLab Issues adapter (`coreGitlab`)

- Modules: `crates/hotsheet-extsync/src/gitlab.rs`, `crates/hotsheet-extsync/src/gitlab`,
  `crates/hotsheet-extsync/src/gitlab/attachments.rs`; small: `gitlab_trash.rs`.
- Responsibility: GitLab issues, notes, labels and uploads as a provider.
- State and persistence: none local; uploads stored in the GitLab project
  (`crates/hotsheet-extsync/src/gitlab/attachments.rs:1-10`).
- Types and entry points: `GitLabConfig` (`crates/hotsheet-extsync/src/gitlab.rs:30`),
  `GitLabProvider` (`crates/hotsheet-extsync/src/gitlab.rs:83`).
- Concurrency: blocking calls on the caller's thread.
- Errors and recovery: `checked` status mapping (`crates/hotsheet-extsync/src/gitlab.rs:136-170`).
- Configuration and environment: connection settings; `PRIVATE-TOKEN` header
  (`crates/hotsheet-extsync/src/gitlab.rs:106-111`).
- Invariants: native ids are issue iids.
- Tests: gitlab.rs tests, `crates/hotsheet-extsync/src/gitlab_trash_tests.rs`,
  gitlab/attachments.rs tests.

## Jira adapter (`coreJira`)

- Modules: `crates/hotsheet-extsync/src/jira.rs`, `crates/hotsheet-extsync/src/jira`,
  `crates/hotsheet-extsync/src/jira/attachments.rs`; small: `jira_trash.rs`.
- Responsibility: Jira issues, comments, labels, native attachments and advisory claim
  markers. No workflow transitions, so `close` is unsupported
  (`crates/hotsheet-extsync/src/jira.rs:1104-1112`).
- State and persistence: none local; Hot Sheet attachment metadata in Jira properties
  (`crates/hotsheet-extsync/src/jira/attachments.rs:1-12`).
- Types and entry points: `JiraConfig` (`crates/hotsheet-extsync/src/jira.rs:47`),
  `JiraProvider` (`crates/hotsheet-extsync/src/jira.rs:105`).
- Concurrency: blocking calls on the caller's thread.
- Errors and recovery: `checked` (`crates/hotsheet-extsync/src/jira.rs:159-181`).
- Configuration and environment: Basic auth from email and token
  (`crates/hotsheet-extsync/src/jira.rs:146`).
- Invariants: Trash is the `hotsheet-trash` label only; workflow status untouched.
- Tests: jira.rs tests, `crates/hotsheet-extsync/src/jira_trash_tests.rs`,
  jira/attachments.rs tests, `crates/hotsheet-extsync/tests/feedback_replies_live.rs` (opt-in live).

## Source connection workflow (`coreExtShared`)

- Modules: `crates/hotsheet-extsync/src/source_connect.rs`,
  `crates/hotsheet-extsync/src/note_trailer.rs`; small: `lib.rs`, `source_resolution.rs`,
  `native_id.rs`, `attachment_crop.rs`.
- Responsibility: one validated connect-and-link workflow for the HTTP route and the
  CLI; adapter construction; comment trailers.
- State and persistence: writes through `coreProviderConfig` and `coreCheckouts`.
- Types and entry points: `connect_source` (`crates/hotsheet-extsync/src/source_connect.rs:253`),
  `connect_github_source`, `recover_github_link`, `live_provider`, `descriptor`,
  `updated_connections` (`crates/hotsheet-extsync/src/lib.rs:60-97`),
  `compose_comment`, `parse_comment` (`crates/hotsheet-extsync/src/note_trailer.rs:45`,
  `crates/hotsheet-extsync/src/note_trailer.rs:146`).
- Concurrency: none of its own.
- Errors and recovery: a missing `settings.credential.secret` is `Authentication`
  (`crates/hotsheet-extsync/src/lib.rs:48-58`); an unknown provider is `Conflict`.
- Configuration and environment: none.
- Invariants: `git` connections cannot be created here
  (`crates/hotsheet-extsync/src/lib.rs:101-106`); source resolution never returns tokens.
- Tests: source_connect.rs and note_trailer.rs tests.

## Checkout and source registry (`coreCheckouts`)

- Modules: `crates/hotsheet-ticketing/src/checkouts.rs`, `crates/hotsheet-ticketing/src/checkouts`,
  `crates/hotsheet-ticketing/src/checkouts/source_observation.rs`,
  `crates/hotsheet-ticketing/src/connection_removal.rs`; small: `checkouts/relocation.rs`,
  `checkouts/respelling.rs`, `checkout_removal.rs`, `project_gitignore.rs`,
  `machine_home.rs`.
- Responsibility: which checkouts exist on this machine and which sources each links;
  idempotent removal of checkouts and connections; the managed project `.gitignore`
  block. Never deletes a checkout directory, store or credential
  (`crates/hotsheet-ticketing/src/checkout_removal.rs:1-13`).
- State and persistence: `HOTSHEET_HOME/checkouts.json`, schema 4
  (`crates/hotsheet-ticketing/src/checkouts.rs:254`); project `.gitignore` block
  (`crates/hotsheet-ticketing/src/project_gitignore.rs:51`).
- Types and entry points: `Checkout`, `TicketSource`, `CheckoutRegistry`
  (`crates/hotsheet-ticketing/src/checkouts.rs:46-422`), `checkout_id`
  (`crates/hotsheet-ticketing/src/checkouts.rs:390`), `remove_checkout`
  (`crates/hotsheet-ticketing/src/checkout_removal.rs:52`), `detach_source`,
  `remove_provider_connection` (`crates/hotsheet-ticketing/src/connection_removal.rs:80`,
  `crates/hotsheet-ticketing/src/connection_removal.rs:156`).
- Concurrency: `checkouts.json.lock`, then `providers.json.lock`.
- Errors and recovery: malformed registry JSON is never recovered or written
  (`crates/hotsheet-ticketing/src/checkouts.rs:1203-1212`); removal steps are idempotent.
- Configuration and environment: `HOTSHEET_HOME`, `HOME`.
- Invariants: checkout ids are readable path identities, not Git or store ids; links
  are many-to-many.
- Tests: checkouts.rs tests, `crates/hotsheet-ticketing/src/checkouts/link_relink_tests.rs`,
  `crates/hotsheet-ticketing/src/checkout_removal/tests.rs`,
  `crates/hotsheet-ticketing/src/connection_removal_endpoint_tests.rs`,
  `crates/hotsheet-ticketing/tests/machine_home_policy.rs`,
  `crates/hotsheet-ticketing/src/project_gitignore_tests.rs`.

## Settings (`coreSettings`)

- Modules: `crates/hotsheet-ticketing/src/settings.rs`,
  `crates/hotsheet-ticketing/src/command_schedules.rs`; small: `settings_locks.rs`,
  `features.rs`, `permission_automation.rs`, `terminal_limits.rs`, `standby.rs`,
  `commands.rs`.
- Responsibility: scoped key-value settings and typed definitions the CLI can validate
  without linking the server.
- State and persistence: global, shared and local JSON files
  (`crates/hotsheet-ticketing/src/settings.rs:1-13`); local command schedules in the
  local settings file (`crates/hotsheet-ticketing/src/command_schedules.rs:1-4`).
- Types and entry points: `Settings` (`crates/hotsheet-ticketing/src/settings.rs:142`),
  `Scope` (`crates/hotsheet-ticketing/src/settings.rs:99`), `get_effective`, `set_if`,
  `CommandSchedule` (`crates/hotsheet-ticketing/src/command_schedules.rs:104`).
- Concurrency: per-file lock in `HOTSHEET_HOME/settings-locks`, pruned at most hourly
  (`crates/hotsheet-ticketing/src/settings_locks.rs:26-33`).
- Errors and recovery: `SettingsError` (`crates/hotsheet-ticketing/src/settings.rs:74`);
  a pruned lock is detected by identity check and retried.
- Configuration and environment: keys listed in the narrative's configuration table.
- Invariants: precedence global < shared < local; secrets are references only.
- Tests: settings.rs and command_schedules.rs tests.

## Ticket groups (`coreGroups`)

- Modules: `crates/hotsheet-ticketing/src/groups.rs`, `crates/hotsheet-ticketing/src/groups`,
  `crates/hotsheet-ticketing/src/groups/git_sync.rs`; small: `groups/registry.rs`,
  `groups/auto.rs`.
- Responsibility: group listing, id/name resolution, multi-ticket rename and migration,
  and committing registry files in the code checkout without disturbing other work
  (`crates/hotsheet-ticketing/src/groups/git_sync.rs:1-12`).
- State and persistence: `<checkout>/.hotsheet2/groups/<id>.json`; sync status record.
- Types and entry points: `GroupDirectory` (`crates/hotsheet-ticketing/src/groups.rs:87`),
  `GroupRegistry` (`crates/hotsheet-ticketing/src/groups/registry.rs:50`),
  `set_group_of_members`, `rename_group`, `publish`, `sync_checkout`
  (`crates/hotsheet-ticketing/src/groups/git_sync.rs:235-256`).
- Concurrency: `groups.lock`; `hotsheet-group-registry-sync` thread.
- Errors and recovery: Git publication skipped and retried later, never failing the
  registry write; member rewrites restore earlier tickets if a later write fails
  (`crates/hotsheet-ticketing/src/store.rs:868-890`).
- Configuration and environment: `HOTSHEET_NO_AUTOCOMMIT`, setting `auto_group_new_tickets`.
- Invariants: entries are never deleted automatically; a group with no members is not listed.
- Tests: `crates/hotsheet-ticketing/src/groups/tests.rs`,
  `crates/hotsheet-ticketing/src/groups/git_sync/tests.rs`.

## Secrets and accounts (`coreSecrets`)

- Modules: `crates/hotsheet-ticketing/src/secrets.rs`, `crates/hotsheet-ticketing/src/secrets`,
  `crates/hotsheet-ticketing/src/secrets/machine_secrets.rs`,
  `crates/hotsheet-ticketing/src/secrets/metadata_owner`,
  `crates/hotsheet-ticketing/src/accounts.rs`; small: `secrets/metadata.rs`,
  `secrets/metadata_owner.rs`, `secrets/metadata_owner/unix.rs`,
  `secrets/metadata_owner/windows.rs`, `secrets/metadata_owner/windows/security.rs`,
  `secrets/test_credentials.rs` (debug builds with the `test-credentials` feature only),
  `github_origin.rs`.
- Responsibility: where credentials live and who can read them. Values: AES-256-GCM
  envelopes, HKDF-derived per account from a 32-byte master
  (`crates/hotsheet-ticketing/src/secrets/machine_secrets.rs:326-340`); master in
  macOS Keychain, Linux `secret-tool`, or Windows Credential Manager under service
  `com.smalltale.hotsheet2` (`crates/hotsheet-ticketing/src/secrets.rs:45`). Readers are
  processes of the same OS user that can open the master item.
- State and persistence: `HOTSHEET_HOME/keys.json` metadata
  (`crates/hotsheet-ticketing/src/secrets.rs:570`), `HOTSHEET_HOME/machine-secrets/`
  (`crates/hotsheet-ticketing/src/secrets/machine_secrets.rs:84`). Accounts are derived,
  never stored (`crates/hotsheet-ticketing/src/accounts.rs:1-14`).
- Types and entry points: `SecretStore` (`crates/hotsheet-ticketing/src/secrets.rs:75`),
  `OsKeychain`, `ScopedOsKeychain`, `KeyRegistry` (`crates/hotsheet-ticketing/src/secrets.rs:439`),
  `resolve_setting_secret`, `list_accounts` (`crates/hotsheet-ticketing/src/accounts.rs:187`).
- Concurrency: account lock then metadata lock; envelope directory `.lock`
  (`crates/hotsheet-ticketing/src/secrets/machine_secrets.rs:733`); in-process master
  cache keyed by home.
- Errors and recovery: `SecretError` (`crates/hotsheet-ticketing/src/secrets.rs:48`);
  `MetadataCommitIncomplete` asks for a repeat; a missing or corrupt master never falls
  back to legacy items once an envelope exists.
- Configuration and environment: `HOTSHEET_API_KEY_<PROVIDER>` overrides reads;
  `HOTSHEET_NATIVE_SECRETS=deny` refuses the native store in tests.
- Invariants: no plaintext secret on disk; settings hold `{ "secret": "<id>" }` only.
- Tests: `crates/hotsheet-ticketing/src/secrets/metadata/tests.rs`,
  `crates/hotsheet-ticketing/src/secrets/metadata_owner/unix/tests.rs`,
  `crates/hotsheet-ticketing/src/secrets/metadata_owner/windows/security/tests.rs`,
  machine_secrets.rs and accounts.rs tests.

## Remote server bindings (`coreRemoteServers`)

- Modules: `crates/hotsheet-ticketing/src/remote_servers`; small: `mod.rs`, `contract.rs`,
  `registry.rs`, `workflow.rs`, `checkout_observation.rs`.
- Responsibility: reviewed bindings to peer servers and bounded remote checkout
  observation. Transport and secrets are injected.
- State and persistence: `remotes.json` (version 1) and `server-identity.json` in the
  machine home (`crates/hotsheet-ticketing/src/remote_servers/registry.rs:17-40`,
  `crates/hotsheet-ticketing/src/remote_servers/registry.rs:386`).
- Types and entry points: `RemoteRegistry`, `server_identity`, `PairingBundle`,
  `RemoteTransport`.
- Concurrency: `remotes.json.lock` sibling lock for every change
  (`crates/hotsheet-ticketing/src/remote_servers/registry.rs:167-170`).
- Errors and recovery: `RemoteError` `Conflict` requires re-review; `Corrupt` and
  `UnsupportedVersion` refuse.
- Configuration and environment: `HOTSHEET_HOME`.
- Invariants: responses bounded by `MAX_REMOTE_RESPONSE_BYTES`; unknown fields rejected.
- Tests: `crates/hotsheet-ticketing/src/remote_servers/tests.rs`,
  `crates/hotsheet-ticketing/src/remote_servers/transition_tests.rs`,
  `crates/hotsheet-ticketing/src/remote_servers/checkout_observation_tests.rs`.

## Activity, metrics and reports (`coreActivityMetrics`)

- Modules: `crates/hotsheet-ticketing/src/activity.rs`,
  `crates/hotsheet-ticketing/src/activity_distillation.rs`, `crates/hotsheet-ticketing/src/metrics.rs`,
  `crates/hotsheet-ticketing/src/metrics`, `crates/hotsheet-ticketing/src/metrics/codex_work.rs`,
  `crates/hotsheet-ticketing/src/metrics/reprice.rs`, `crates/hotsheet-ticketing/src/phase_report.rs`;
  small: `metrics/reroute.rs`, `pricing.rs`, `analytics.rs`, `phase_timing.rs`,
  `calibration.rs`, `live_history.rs`, `annotation_activity.rs`.
- Responsibility: derived, mostly per-device records of activity and usage, and reports
  computed from ticket history. Does not change ticket state except
  `write_distilled_note` (`crates/hotsheet-ticketing/src/activity_distillation.rs:265`).
- State and persistence: `activity/recent/<date>.jsonl`
  (`crates/hotsheet-ticketing/src/activity.rs:322`), `metrics/raw/<date>.jsonl`
  (`crates/hotsheet-ticketing/src/metrics.rs:155`), committed
  `metrics/rollups/<email>/rollup.json` (`crates/hotsheet-ticketing/src/metrics.rs:367`),
  `metrics/local/corrections.json` (`crates/hotsheet-ticketing/src/metrics/reprice.rs:66`),
  `metrics/local/live-history.jsonl` (`crates/hotsheet-ticketing/src/live_history.rs:64`).
- Types and entry points: `ActivityEvent` (`crates/hotsheet-ticketing/src/activity.rs:260`),
  `timeline`, `prune_before`, `UsageEvent` (`crates/hotsheet-ticketing/src/metrics.rs:59`),
  `record` (`crates/hotsheet-ticketing/src/metrics.rs:154`), `DistillationPolicy`
  (`crates/hotsheet-ticketing/src/activity_distillation.rs:49`).
- Concurrency: `metrics/raw/.usage.lock` for appends and repricing.
- Errors and recovery: rollups rebuild from raw; readers skip torn lines; live history
  compacts after 4,096 lines keeping 7 days (`crates/hotsheet-ticketing/src/live_history.rs:26-28`).
- Configuration and environment: `CODEX_SESSION_ID`, `HOTSHEET_WORKER_ID`,
  `HOTSHEET_PROJECT`, `HOTSHEET_USAGE_STORES`, `CODEX_HOME`, `HOME`
  (`crates/hotsheet-ticketing/src/metrics/codex_work.rs:172-200`); distillation is off
  unless the machine-local policy enables it.
- Invariants: raw evidence is never rewritten by repricing; prompts and tool content are
  discarded from Codex rollouts (`crates/hotsheet-ticketing/src/metrics/codex_work.rs:1-2`).
- Tests: activity.rs, activity_distillation.rs, metrics.rs, metrics/codex_work.rs,
  metrics/reprice.rs and phase_report.rs tests.

## Repository maintenance (`coreRepoMaintenance`)

- Modules: `crates/hotsheet-ticketing/src/repository_compaction.rs`,
  `crates/hotsheet-ticketing/src/repository_size.rs`,
  `crates/hotsheet-ticketing/src/host_health_disks.rs`; small: `repository_status.rs`,
  `host_health.rs`.
- Responsibility: plan and execute attachment-history rewrites with `git-filter-repo`
  and a backup; size and status reports; host-health definitions.
- State and persistence: `<git-dir>/hotsheet-compaction-recovery.json`
  (`crates/hotsheet-ticketing/src/repository_compaction.rs:93-107`); the plan
  fingerprint binds to HEAD.
- Types and entry points: `plan_store`, `execute_store`, `recover_store`
  (`crates/hotsheet-ticketing/src/repository_compaction.rs:121`,
  `crates/hotsheet-ticketing/src/repository_compaction.rs:474`,
  `crates/hotsheet-ticketing/src/repository_compaction.rs:685`); size thresholds 5 GB
  warning (`crates/hotsheet-ticketing/src/repository_size.rs:23`).
- Concurrency: history-sync, note-mutation and Git transaction locks during execution.
- Errors and recovery: missing `git-filter-repo` refuses
  (`crates/hotsheet-ticketing/src/repository_compaction.rs:720-727`); remote replaced with
  `--force-with-lease` (`crates/hotsheet-ticketing/src/repository_compaction.rs:910-912`);
  other checkouts must recover before writing or syncing.
- Configuration and environment: `host_health.*` settings.
- Invariants: a stale plan cannot execute.
- Tests: repository_compaction.rs, repository_size.rs and host_health_disks.rs tests.

## Media and glance helpers (`coreMedia`)

- Modules: `crates/hotsheet-ticketing/src/image_crop.rs`,
  `crates/hotsheet-ticketing/src/screensaver.rs`; small: `image_crop/avif_animation.rs`,
  `plugin_digest.rs`.
- Responsibility: crop renditions that never change the original attachment bytes
  (`crates/hotsheet-ticketing/src/image_crop.rs:1`); screen-saver snapshot location and
  install workflow (`crates/hotsheet-ticketing/src/screensaver.rs:1-10`); plugin digests.
- State and persistence: screen-saver snapshot JSON in the saver container, written only
  if its directory exists.
- Types and entry points: `normalize_crop`, `validate_rendition`, `original_dimensions`
  (`crates/hotsheet-ticketing/src/image_crop.rs:441-668`), `locations_from_env`
  (`crates/hotsheet-ticketing/src/screensaver.rs:54`).
- Concurrency: none.
- Errors and recovery: `ImageCropError`; pixel and frame caps
  (`crates/hotsheet-ticketing/src/image_crop.rs:23-27`).
- Configuration and environment: `HOME` and the saver home variable
  (`crates/hotsheet-ticketing/src/screensaver.rs:55`).
- Invariants: originals are immutable.
- Tests: image_crop.rs and screensaver.rs tests.

## Shared leaf utilities (`coreUtil`)

- Modules: `crates/hotsheet-sync/src` (`lib.rs`, `env_path.rs`, `files.rs`, `paths.rs`).
- Responsibility: poison-recovering lock extension traits, blank-as-unset path
  variables, canonical path spelling, atomic replace and persist.
- State and persistence: none.
- Types and entry points: `LockExt::lock_or_recover` (`crates/hotsheet-sync/src/lib.rs:20-36`),
  `env_path` (`crates/hotsheet-sync/src/env_path.rs:22`), `files::replace`,
  `files::persist` (`crates/hotsheet-sync/src/files.rs:38-53`), `paths::canonicalize`
  (`crates/hotsheet-sync/src/paths.rs:73`).
- Concurrency: recovers poisoned mutexes and keeps serving.
- Errors and recovery: Windows rename retried briefly on sharing violations
  (`crates/hotsheet-sync/src/files.rs:13-16`).
- Configuration and environment: none.
- Invariants: one canonical spelling for every stored path.
- Tests: `crates/hotsheet-sync/tests/one_atomic_replace.rs`,
  `crates/hotsheet-sync/tests/one_path_spelling.rs`.

## mTLS material (`coreTls`)

- Modules: `crates/hotsheet-tls/src/lib.rs`.
- Responsibility: issue the per-project CA, server leaf and device certificates; record
  revocations and device roles. The server builds the rustls verifier.
- State and persistence: `HOTSHEET_HOME/tls/<project id>/` with `ca.crt`, `ca.key`,
  `server.crt`, `server.key`, `devices/<name>.crt`, `revoked`
  (`crates/hotsheet-tls/src/lib.rs:14-16`).
- Types and entry points: `init_ca`, `issue_device`, `renew_device`, `revoke_device`,
  `set_device_role` (`crates/hotsheet-tls/src/lib.rs:108-263`).
- Concurrency: none.
- Errors and recovery: `TlsError` (`crates/hotsheet-tls/src/lib.rs:30`).
- Configuration and environment: `HOTSHEET_HOME` from the caller.
- Invariants: private keys are machine-local and never committed.
- Tests: lib.rs tests module.

## Process journal (`coreProcessJournal`)

- Modules: `crates/hotsheet-process-journal/src/lib.rs`.
- Responsibility: record child-process launches with unique launch ids and spawn,
  detach and environment-replacement helpers.
- State and persistence: `HOTSHEET_HOME/processes/` JSON records, at most 16 KiB each,
  1,024 ended and 1,024 unresolved, retained 30 days
  (`crates/hotsheet-process-journal/src/lib.rs:11-15`, `crates/hotsheet-process-journal/src/lib.rs:806`).
- Types and entry points: `ProcessJournal`, `LaunchContext`, `TrackedChild`,
  `DetachedChild`, `close_inherited_descriptors` (`crates/hotsheet-process-journal/src/lib.rs:656`).
- Concurrency: none of its own; records written by temp file and fsync.
- Errors and recovery: stale temp files older than an hour are pruned.
- Configuration and environment: machine home from the caller.
- Invariants: records contain identifiers and lifecycle facts only
  (`crates/hotsheet-process-journal/src/lib.rs:1-2`).
- Tests: lib.rs tests module.
