# Rust server, terminals, AI drive and notifications: coverage

Status: filled by HS2-P27EJM. Audited source `8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`.
`node scripts/architecture-coverage.mjs inventory --area server` lists the
required entries; every one appears below in backticks. The narrative is
[40-server](../narrative/40-server.md); the route inventory table is there.

Abbreviations: "home" is `${HOTSHEET_HOME:-~/.hotsheet2}`
(`crates/hotsheet-plugins/src/lib.rs:1352`). "Offload" is the global
`blocking_offload::offload` layer that runs non-streaming handlers on the
blocking pool (`crates/hotsheet-server/src/lib.rs:2963`).

## Process lifecycle and instance registry (`serverLifecycle`)

- Modules: `crates/hotsheet-server/src/main.rs`, `crates/hotsheet-server/src/lifecycle.rs`, `source_revision.rs` (build and source revision status for `--revision-status` and `/health`).
- Responsibility: parse flags, choose the process mode, take the per-store writer lock, build `AppState`, start background services, bind the listeners, publish instance files and run the bounded drain. It does not restart itself after a crash. Restart and supervision belong to the launching client.
- State and persistence: `<home>/instances/<project id>.json` for each hosted store. It holds pid, URL, **secret**, store and index paths, start time, build revision, executable, launcher, anchor and remote URL, and is written with mode 0600 (`crates/hotsheet-server/src/lifecycle.rs:29-57`, `crates/hotsheet-server/src/lifecycle.rs:186-205`). Locking uses three files:
  - `<project id>.lock`, which holds the pid and is replaced through a temp file and rename;
  - `<project id>.lock.mutex`, a persistent `flock` file that serializes starters (`crates/hotsheet-server/src/lifecycle.rs:247-283`);
  - the project id itself, which is the first 16 hex digits of a hash of the canonical store path (`crates/hotsheet-server/src/lifecycle.rs:123-128`).
- Types and entry points: `InstanceInfo`, `InstanceRegistry`, `InstanceGuard` (its drop removes the file only if the file still holds this registration; `crates/hotsheet-server/src/lifecycle.rs:413-427`), `WriterLock`, `StopDecision`, `request_stop`, and `PINNED_LAUNCHER = "local-host-snapshot"`. A pinned server is refused without `--force` (`crates/hotsheet-server/src/lifecycle.rs:76-120`).
- Concurrency: the signal task (`crates/hotsheet-server/src/main.rs:778`); the stdin-EOF thread (`crates/hotsheet-server/src/main.rs:795`); the test-owner watchdog thread, which checks every 250 ms (`crates/hotsheet-server/src/main.rs:884`); the exit watchdog thread (`crates/hotsheet-server/src/main.rs:767`). The lifecycle flags are atomics plus `Notify` (`crates/hotsheet-server/src/lib.rs:748-779`).
- Errors and recovery:
  - Stale lock and instance files whose pid is dead are reclaimed.
  - An empty or unreadable lock file younger than 5 s reads as "starting" (`crates/hotsheet-server/src/lifecycle.rs:333-365`).
  - On Windows a stop terminates the process (`crates/hotsheet-server/src/lifecycle.rs:466-471`).
- Configuration and environment: the flags `-C/--path` (default `.`), `--bind` (`127.0.0.1:8787`), `--remote-transport`, `--local-bind` (`127.0.0.1:0`), `--secret`, `--index`, `--stop`, `--force`, `--kill-all-terminals`, `--revision-status`, `--build-revision`, `--launcher`, `--no-terminal-broker`, `--drive-tool`, `--drive-workers` (1), `--drive-lease-min` (30), `--drive-worker`, and the hidden `--shutdown-drain-ms` (5000), `--ai-probe-timeout-ms` and `--exit-on-stdin-eof` (`crates/hotsheet-server/src/main.rs:39-165`). The environment variables:
  - `HOTSHEET_HOME`;
  - `HOTSHEET_SERVER_LAUNCHER`, the launcher fallback;
  - `HOTSHEET_JIRA_WRITE_BEHIND=1`;
  - `HOTSHEET_TEST_SERVER_OWNER_PID` (Unix fixtures);
  - `RUST_LOG`, default `info`;
  - `ROOMY_CAPACITY_ENV`, honored only with the test vault (`crates/hotsheet-server/src/main.rs:394-410`).
- Invariants: the writer lock is taken before any credential or store is opened (`crates/hotsheet-server/src/main.rs:312-315`); instance files are removed explicitly before exit, not left to `Drop` (`crates/hotsheet-server/src/lib.rs:784-793`).
- Tests: `crates/hotsheet-server/src/lifecycle.rs` (`writer_lock_transition_matrix`, `concurrent_starters_never_both_take_the_writer_lock`, `old_instance_guard_preserves_a_replacement_registration`), `crates/hotsheet-server/tests/graceful_shutdown.rs`, `crates/hotsheet-server/tests/pinned_stop.rs` (`a_pinned_server_survives_an_ordinary_stop_and_stops_when_forced`), `crates/hotsheet-server/tests/concurrent_start.rs`, `crates/hotsheet-server/tests/stdin_eof_git_children.rs`.

## TLS listeners (`serverTls`)

- Modules: `crates/hotsheet-server/src/tls.rs`.
- Responsibility: serve HTTPS (server certificate only) or mTLS from the store's certificate paths (`hotsheet_tls::Paths`), check revocation on every handshake, and apply the ACL role: read-write, read-only (GET, HEAD and OPTIONS only, never terminal attach) or deny. Off-box certificate callers may not use `/remotes` or orchestration machine paths (`crates/hotsheet-server/src/tls.rs:178-254`).
- State and persistence: none of its own. It reads the store's certificate and ACL files on every connection, so changes apply live.
- Types and entry points: `build_server_config`, `build_https_server_config`, `serve_tls_with_acl`, and the `CertificateAuthenticated` and `CertificateFingerprint` extensions (`crates/hotsheet-server/src/tls.rs:24-101`).
- Concurrency: one Tokio task per accepted connection. Tracked connections are drained on shutdown.
- Errors and recovery: a missing certificate fails startup with a `hotsheet-cli cert init` hint (`crates/hotsheet-server/src/main.rs:348-350`). A read-only refusal drains up to 64 KiB of the body first, so the client sees the 403 instead of a reset.
- Configuration and environment: `--remote-transport https|mtls`.
- Invariants: an unreadable ACL means deny.
- Tests: `crates/hotsheet-server/tests/mtls.rs` (`a_ca_issued_client_cert_gets_in_and_no_cert_or_revoked_is_rejected`, `shutdown_stops_accepting_then_drains_open_tls_connections`), `crates/hotsheet-server/tests/mtls_site.rs`, and `crates/hotsheet-server/src/tls.rs` (`hs2_1y45mz_off_box_registry_administration_is_denied_even_for_a_writer`).

## Transport and authorization (`serverTrust`) and request middleware (`serverMiddleware`)

- Modules: `crates/hotsheet-server/src/routes` (34 route modules; every one is named in a section of this file), `crates/hotsheet-server/src/routes/auth.rs`, `crates/hotsheet-server/src/routes/mod.rs` (it only declares the 34 route modules; the router is in `crates/hotsheet-server/src/lib.rs`), `crates/hotsheet-server/src/lib.rs` (`app`, `AppState` and builders), `blocking_offload.rs`, `request_timing.rs`, `api_error.rs` (the `{error, code}` shape), `token_throttle.rs` (the per-peer failure budget for the renewal-token route: 30 failures in 10 s, at most 1024 peers), and `dto.rs`.
- Responsibility: admit each protected request as `LocalBearer`, `Certificate` or `BrowserSession`, record the `Requester`, guard primary-store identity, gate mutations during quiescence, and stamp the change cursor (`crates/hotsheet-server/src/routes/auth.rs:130-191`). It does not authorize per operation; orchestration and per-route extractors do that.
- State and persistence: none.
- Types and entry points: `require_secret`, `Caller`, `Requester`, `HostAccess`, `HostTrust` and `primary_store_route` (`crates/hotsheet-server/src/routes/auth.rs:16-128`).
- Concurrency: the middleware runs once per request.
- Errors and recovery: 401 from the session check, 503 while quiescing, and 403 `host_access_required`.
- Configuration and environment: the machine secret; remote-listener mode, which restricts the bearer to loopback peers (`crates/hotsheet-server/src/lib.rs:949-951`).
- Invariants:
  - The secret comparison is a plain `==` (`crates/hotsheet-server/src/routes/auth.rs:157`).
  - Five host-launching handlers take `HostAccess`, and a policy test pins that count (`crates/hotsheet-server/src/routes/auth.rs:259-300`).
- Tests: `crates/hotsheet-server/src/routes/auth.rs` (`guards_primary_data_without_capturing_scoped_or_machine_routes`, `hs2_s8r3pd_only_host_trusted_callers_pass`), `crates/hotsheet-server/tests/remote_host_access.rs`, `crates/hotsheet-server/tests/remote_listener_modes.rs`, and `crates/hotsheet-server/src/request_performance_tests.rs`.

## Remote device catalog (`serverPairing`)

- Modules: `crates/hotsheet-server/src/remote_auth.rs`, `crates/hotsheet-server/src/remote_auth` (`crates/hotsheet-server/src/remote_auth/migration.rs` merges legacy per-store catalogs into the machine catalog at open, and startup reports the merge; `crates/hotsheet-server/src/main.rs:381-389`), and `crates/hotsheet-server/src/routes/remote_auth.rs` (14 public routes).
- Responsibility: browser pairing, owner approval and rejection, redeem, session authorization (cookie with Origin and CSRF, or a paired bearer), rotation, renewal requests, logout, role and grant changes, revoke and revoke-all, and filtering manager-only events.
- State and persistence: `<home>/remote-auth/machine.json`. Each write takes a file lock, re-reads the file, applies the change and replaces the file atomically with mode 0600 (`crates/hotsheet-server/src/remote_auth.rs:311-316`, `crates/hotsheet-server/src/remote_auth.rs:600-618`, `crates/hotsheet-server/src/remote_auth.rs:1078-1101`). Only SHA-256 digests of tokens and CSRF values are stored, and revoked sessions stay as tombstones.
- Types and entry points: `RemoteAuth::{request, approve, reject, redeem, authorize, authorize_bearer, rotate, request_renewal, renew, revoke, revoke_all, change}` (`crates/hotsheet-server/src/remote_auth.rs:620-1002`). Routes: `pair`, `redeem`, `me`, `rotate`, `request_renewal`, `logout`, `owner_list`, `owner_changes` (25 s long poll), `approve`, `reject`, `change`, `revoke`, `renew` and `revoke_all` (`crates/hotsheet-server/src/routes/remote_auth.rs:369-652`).
- Concurrency: a `Mutex` over the cached catalog plus a file lock across processes; a `watch` channel counts writes for the long poll.
- Errors and recovery: `AuthFailure` maps to stable codes: 429 `rate_limited`, 410 `pairing_expired`, 409 `unapproved`, 401 `session_expired`, `session_revoked` and `session_invalid`, and 403 `read_only` and `csrf_required` (`crates/hotsheet-server/src/routes/remote_auth.rs:186-226`). Without a catalog the server answers 503.
- Configuration and environment: TTLs `PAIR_TTL` 15 min, `SESSION_TTL` 90 days and `IDLE_TTL` 30 days; at most 10 requests a minute and 5 attempts (`crates/hotsheet-server/src/remote_auth.rs:47-66`); cookies `hs2_remote_session` (HttpOnly, SameSite=Strict) and `hs2_remote_csrf`, both with Max-Age 400 days (`crates/hotsheet-server/src/routes/remote_auth.rs:20-25`).
- Invariants:
  - `revoke-all` is host-only.
  - Any manager may grant `local`. That matches the module doc (`crates/hotsheet-server/src/routes/remote_auth.rs:1-5`) but contradicts the comment on `RemoteAuth::approve` (`crates/hotsheet-server/src/remote_auth.rs:679-680`).
- Events: `pairing_requests` and `browser_grants`, both without identities.
- Tests: `crates/hotsheet-server/tests/remote_auth.rs` (`hs2_7dbjms_pair_approve_redeem_role_replay_revoke`, `hs2_bzhmev_pairing_request_events_reach_only_device_managers_over_poll`, `hs2_aw2akf_bearer_writes_without_browser_proof_while_cookie_writes_need_it`) and `crates/hotsheet-server/tests/remote_session_lifetime.rs`.

## Jira write-behind outbox (`serverWriteBehind`)

- Modules: `crates/hotsheet-server/src/provider_write_behind.rs` (4 routes and the dispatch loop) and `provider_overlay.rs` (projects pending edits over provider reads).
- Responsibility: when enabled, durably admit Jira field edits (title, details, category, priority, tags), answer with the provisional ticket, and dispatch in the background with readback. Status, phase, Up Next, blockers and group stay synchronous (`crates/hotsheet-server/src/provider_write_behind.rs:446-470`).
- State and persistence: SQLite `<home>/outbox/<store id>.sqlite`, capacity 1000, with a 5 s busy timeout (`crates/hotsheet-server/src/main.rs:394-400`, `crates/hotsheet-ticketing/src/provider_outbox.rs:300-308`).
- Types and entry points: `queue_jira_updates`, `list_jira_operations`, `retry_jira_operation`, `discard_jira_operation`, `dispatch_jira_at` and `spawn_jira_dispatch_loop` (`crates/hotsheet-server/src/provider_write_behind.rs:55-298`).
- Concurrency: one Tokio task loops `dispatch_jira_once` and then sleeps 2 s, forever. It never observes `stopping()`. Each pass claims at most 4 ready operations and runs each on `spawn_blocking`. The outbox `Mutex` uses `lock_or_recover`.
- Errors and recovery (`crates/hotsheet-server/src/provider_write_behind.rs:390-440`):
  - A rate limit defers the operation and the connection by `retry_after`, 30 s by default.
  - Authentication, disabled, not-found and unsupported errors mark the operation as needing attention.
  - Every 8th attempt also marks it as needing attention.
  - Any other error backs off by `2^min(attempts, 6)` seconds plus jitter.
  - An acknowledgement that lacks the intended fields needs attention. A conflict with the base is recorded.
  - Operations survive a restart.
- Configuration and environment: `HOTSHEET_JIRA_WRITE_BEHIND=1`; the provider capability `write_behind`.
- Invariants: an operation id is one remote write whatever its retries.
- Events: `updated`, with `store` set to the connection, on admit, claim, settle, retry and discard.
- Tests: `crates/hotsheet-server/tests/jira_outbox_http.rs` (`failed_dispatch_requeues_with_backoff_and_later_edits_stay_behind_it`, `same_operation_id_retried_across_every_dispatch_state_is_one_remote_write`). The loop itself is untested.

## Provider advisory claims (`serverAdvisory`) and managed credentials (`serverCredentials`)

- Modules: `crates/hotsheet-server/src/provider_advisory_claims.rs` (2 routes), `credential_cache.rs` (with tests under `crates/hotsheet-server/src/credential_cache`).
- Responsibility:
  - Advisory claims on Jira issues, for a checkout that links the connection. `GET` replays at most 16 provisional ledger entries. Entries settle as confirmed, needs-attention or expired; an entry expires after 7 days (`crates/hotsheet-server/src/provider_advisory_claims.rs:180-237`).
  - The managed secret cache does one OS credential read per sign-in and metadata revision.
- State and persistence: `<home>/advisory_claims/provisional.sqlite` (`crates/hotsheet-server/src/main.rs:401-403`); the credential cache is in memory.
- Types and entry points: `get_advisory_claim`, `post_advisory_claim` and `replay_pending_once` (`crates/hotsheet-server/src/provider_advisory_claims.rs:180-346`); `ManagedSecretCache`.
- Concurrency: the ledger `Mutex`. Provider I/O runs inside the offloaded handler.
- Errors and recovery: authentication, rate-limit and other errors skip an entry until the next read. A new claim is refused with 409 while provisional entries are unresolved.
- Configuration and environment: none.
- Invariants: a `GET` can write to Jira; OpenAPI documents this.
- Events: `advisory_claim_changed`.
- Tests: `crates/hotsheet-server/tests/jira_advisory_claim_http.rs` (`hs2_bpkm76_provisional_claim_replays_after_restart_and_reconciles`); `crates/hotsheet-server/src/credential_cache` tests.

## GitHub webhook receiver (`serverWebhooks`)

- Modules: `crates/hotsheet-server/src/github_webhooks.rs`, `crates/hotsheet-server/src/github_webhooks` (`admission_tests.rs`), `github_app_config.rs`.
- Responsibility: a public receiver plus authenticated setup routes. The receiver requires:
  - a known, enabled GitHub connection that is linked to a registered checkout and has a key-registry secret reference;
  - a valid `x-hub-signature-256` HMAC, compared in constant time;
  - an `issues` or `issue_comment` event for the same repository;
  - a delivery id within the dedupe ring of 1024 entries and 24 h;
  - a signed `updated_at` no more than 5 minutes in the future.

  It then re-reads the issue from the provider and publishes `changed` or `deleted` (`crates/hotsheet-server/src/github_webhooks.rs:22-24`, `crates/hotsheet-server/src/github_webhooks.rs:397-602`).
- State and persistence: the delivery and revision maps are in memory, so a restart forgets them. The secret stays in the key registry; the `PUT` never returns it.
- Types and entry points: `receive`, `setup_instructions`, `configure`, `DeliveryState`.
- Concurrency: one `spawn_blocking` per delivery.
- Errors and recovery: the receiver answers 401, 403, 404, 400, 502 (retryable), 503 or 200 with `accepted: false` for an ignored event.
- Configuration and environment: the body limit is 256 KiB.
- Invariants: per-issue revisions only move forward.
- Tests: `crates/hotsheet-server/tests/http.rs` (`hs2_sgq183_webhook_setup_and_delivery_are_authenticated_and_source_bound`), `crates/hotsheet-server/src/github_webhooks/admission_tests.rs`. The 413 path is untested.

## Host health and capacity (`serverHostHealth`)

- Modules: `crates/hotsheet-server/src/host_health` (`crates/hotsheet-server/src/host_health/mod.rs` monitor and sampler, `crates/hotsheet-server/src/host_health/source.rs` platform readers for `ps`, `vm_stat`, `pmset` and `/proc`, `crates/hotsheet-server/src/host_health/capacity.rs` capacity readings for orchestration, `crates/hotsheet-server/src/host_health/capabilities.rs` toolchain, GPU and OS detection, `live_cpu.rs` the on-demand CPU long poll, `routes.rs` 7 routes), and `tool_capacity_signals.rs` (records tool usage and limit signals for capacity).
- Responsibility: sample this host on a server-local cadence, evaluate thresholds, publish `host_health` crossings, serve live CPU while someone watches, and answer capacity for placement.
- State and persistence: the monitor in memory; disk choices and thresholds in global settings.
- Types and entry points: `AppState::spawn_host_health_sampler`, `run_sampler`, `Capacity::observe`, `get_live_cpu`.
- Concurrency:
  - One Tokio sampler task, guarded by a `started` flag. It sleeps `sample_seconds` and stops on `stopping()`.
  - The live-CPU task samples every 2 s and stops after 15 s without a request.
- Errors and recovery: a panic in the sample's `spawn_blocking` ends the sampler for good, silently (`crates/hotsheet-server/src/host_health/mod.rs:1362-1364`).
- Configuration and environment: `HOTSHEET_HOST_HEALTH_SAMPLER=off`; `ROOMY_CAPACITY_ENV` (tests).
- Invariants: none beyond the above.
- Tests: `crates/hotsheet-server/src/host_health/tests.rs` (`thresholds_round_trip_validate_and_feed_the_sampler`, `crossings_are_cursored_change_events_a_long_poll_replays`), `capacity_tests.rs`, `capabilities_tests.rs`.

## Live state and project activity (`serverLive`)

- Modules: `crates/hotsheet-server/src/live_state.rs` (3 routes), `live_history.rs` (1 route), `crates/hotsheet-server/src/ci_status.rs` (1 route), `crates/hotsheet-server/src/project_activity.rs`, `crates/hotsheet-server/src/stuck_causes.rs`, `live_record.rs`, `live_worker_plan.rs`, `activity_capture.rs`, `health_scan.rs`.
- Responsibility:
  - Build each hosted store's live snapshot: claims, workers, token rates, feedback, CI and stuck causes.
  - Push a snapshot as `live_state` only when it changes materially (`crates/hotsheet-server/src/live_state.rs:403-454`).
  - Record transitions into a history file.
  - Keep the newest reported CI run per checkout.
  - Derive project activity and idle windows, which notifications also use.
- State and persistence:
  - `<home>/ci-status.json`, replaced atomically under its mutex (`crates/hotsheet-server/src/ci_status.rs:26`, `crates/hotsheet-server/src/ci_status.rs:285-304`).
  - `metrics/local/live-peaks.json` in the store, plus a `.gitignore` entry.
  - Live-history records per store.
  - The live-state record, through `live_record`.
- Types and entry points: `live_state::{touch, refresh, refresh_all, current_all, snapshot_all}`, `live_history::observe`, `CiReports::record`.
- Concurrency: `touch` coalesces for 250 ms and then rebuilds on `spawn_blocking`; a `building` mutex serializes rebuilds.
- Errors and recovery: file writes are best effort, and failures are logged and retried on the next publish. A corrupt `ci-status.json` is ignored.
- Configuration and environment: none.
- Invariants: the live-state `GET` routes rebuild, and can write and emit. `current_all` is the read-only variant.
- Tests: `crates/hotsheet-server/tests/live_state.rs` (`hs2_jttqmy_live_state_follows_claims_release_and_feedback_without_rules`, `hs2_m8hhea_reported_ci_runs_are_pushed_in_the_live_state`), `crates/hotsheet-server/tests/live_history.rs`, and `crates/hotsheet-server/src/ci_status.rs` (`hs2_m8hhea_reports_persist_and_bad_files_are_ignored`).

## Provider adapters and Git operations (`serverSources`)

- Modules: `crates/hotsheet-server/src/routes/providers` (28 routes): `crates/hotsheet-server/src/routes/providers/mod.rs` (connections, transfers, `provider_for`, the live client cache) and `crates/hotsheet-server/src/routes/providers/tickets.rs` (provider ticket CRUD, close, restore, assign, not-working, AI feedback).
  - Test-only children: `crates/hotsheet-server/src/routes/providers/admission_tests` with `crates/hotsheet-server/src/routes/providers/admission_tests/read_coverage.rs` (HS2-6H8ZY6 credential-admission composition in an isolated child process), and `crates/hotsheet-server/src/routes/providers/disabled_transfer_tests` with `crates/hotsheet-server/src/routes/providers/disabled_transfer_tests/scenarios.rs` (HS2-DJWTB0 disabled-connection transfer admission).
  - Small helpers: `crates/hotsheet-server/src/provider_overlay.rs` (Jira pending overlay) and `crates/hotsheet-server/src/github_app_config.rs`.
- Responsibility: route by `(connection_id, native_id)` to Git, GitHub, GitLab or Jira. Refuse disabled connections before building a client. Copy or move between providers with `operation_id` and `confirm`. Keep live clients keyed by a hash of the connection and token.
- State and persistence: connection records go through the provider configuration registry (`hotsheet_extsync`); `state.live_providers` is a cache.
- Types and entry points: `provider_for` (`crates/hotsheet-server/src/routes/providers/mod.rs:1020`), `provider_copy_route` and `provider_move_route` (`crates/hotsheet-server/src/routes/providers/tickets.rs:831-861`).
- Concurrency: the `live_providers` `Mutex`; provider network I/O runs in offloaded handlers.
- Errors and recovery: a stale token gives 409 with `ticket_changed`; other provider errors map to 404, 409, 401 or 400.
- Configuration and environment: provider connection settings; the key registry.
- Invariants: cross-provider moves need `confirm=true` and an operation id.
- Events: `created` and `updated` only for hosted Git connections. Assign, delete-note and not-working emit nothing from the route.
- Tests: `crates/hotsheet-server/tests/http.rs` (`provider_transfer_is_idempotent_and_move_closes_source_after_copy`, `a_disabled_source_is_never_contacted_and_its_tickets_return_when_enabled`), and `crates/hotsheet-server/tests/github_trash_http.rs`, `gitlab_trash_http.rs` and `jira_trash_http.rs`. Provider assign and assign-current are untested.

## Orchestration host (`serverOrch`, `serverResources`)

- Modules: `crates/hotsheet-server/src/routes/orchestration.rs` (94 routes through two handlers), `crates/hotsheet-server/src/routes/orchestration_settings_watch.rs`, `crates/hotsheet-server/src/routes/orchestration_sessions.rs` (`ServerSessions`, the orchestration `SessionHost` that opens, lists and kills terminals in-process and pushes role prompts; emits `orchestration_session`), `crates/hotsheet-server/src/routes/resource_claims.rs` (7 routes), `crates/hotsheet-server/src/routes/remotes.rs` (7 routes), `crates/hotsheet-server/src/dist_work_loop.rs` (see `serverDistWork`). The orchestration domain is in [orchestration coverage](orchestration.md).
- Responsibility:
  - `read` (GET) and `write` (POST, PUT, PATCH, DELETE) pass the path to `OrchestrationRuntime::request_now` on `spawn_blocking` (`crates/hotsheet-server/src/routes/orchestration.rs:943-991`, `crates/hotsheet-cli/src/orchestration/request.rs:489`). Writes pass the `Requester`, and reads keep the query string.
  - Per-owner background tasks follow runs, gates and the scheduler.
  - The settings watcher announces CLI-made changes.
  - Resource-claim routes wrap `ClaimService` with a 55 s maximum wait and at most 64 parked requests.
  - `remotes` manages this machine's outbound peer registry through `RemoteRuntime`.
- State and persistence: all records live in the orchestration and claims libraries under the home and the owner's `.hotsheet2`. This module only creates the scheduler wake directory.
- Types and entry points: `read`, `write`, `execute_blocking`, `ensure_watchers`, `ensure_gate_follower`, `ensure_scheduler`, `spawn_run_recovery`, `spawn_hosted_session_follower`, `spawn_target_line_push`, `spawn_scheduler_wake_follower` (`crates/hotsheet-server/src/routes/orchestration.rs:265-903`), and `ensure_settings_watcher` (`crates/hotsheet-server/src/routes/orchestration_settings_watch.rs:85`).
- Concurrency: see the task inventory in the chapter. The dedupe registries are process-global `OnceLock<Mutex<HashSet>>` sets keyed by home, lifecycle and owner.
- Errors and recovery:
  - Watchers back off from 1 s to 60 s on failure and end when inactive or stopping.
  - Background tasks end silently if the runtime cannot be built.
  - A failed join of a blocking task gives 500.
- Configuration and environment: none of its own.
- Invariants:
  - Off-box mTLS callers cannot reach machine paths (`crates/hotsheet-server/src/tls.rs:204-218`).
  - A write drops the query string (`crates/hotsheet-server/src/routes/orchestration.rs:988-990`).
- Events: `orchestration_changed` and `terminal_orchestrator`.
- Tests: `crates/hotsheet-server/tests/remote_servers.rs` and its modules under `crates/hotsheet-server/tests/remote_servers` (`orchestration.rs`, `runs.rs`, `recovery.rs`, `scheduler.rs`, `sessions.rs`, `permissions.rs`), `crates/hotsheet-server/tests/resource_claims.rs` and `crates/hotsheet-server/tests/resource_token_throttle.rs`. `target-pool`, `/remote-identity` and `/remote-checkouts` have no server HTTP test.

## Checkout ticket API (`serverTickets`)

- Modules:
  - Checkout listing, sources and repository: `crates/hotsheet-server/src/routes/checkouts` (`crates/hotsheet-server/src/routes/checkouts/mod.rs`, `crates/hotsheet-server/src/routes/checkouts/listing.rs`, and `removal.rs`, which refuses with 409 while a presence lease holds the checkout).
  - Checkout ticket writes: `crates/hotsheet-server/src/routes/checkout_tickets` (`crates/hotsheet-server/src/routes/checkout_tickets/mod.rs` and `crates/hotsheet-server/src/routes/checkout_tickets/attachments.rs`).
  - Primary and store routes: `crates/hotsheet-server/src/routes/ticket_writes.rs`, `crates/hotsheet-server/src/routes/tickets.rs`, `crates/hotsheet-server/src/routes/claims.rs`, `crates/hotsheet-server/src/routes/transfer.rs`, `crates/hotsheet-server/src/routes/store_scoped.rs`.
  - Groups, activity and accounts: `crates/hotsheet-server/src/routes/groups.rs`, `crates/hotsheet-server/src/routes/activity.rs`, `crates/hotsheet-server/src/routes/accounts.rs`.
  - Workers and settings: `crates/hotsheet-server/src/routes/checkout_workers.rs`, `crates/hotsheet-server/src/routes/ticket_creation_settings.rs`, `crates/hotsheet-server/src/routes/features.rs`, `crates/hotsheet-server/src/routes/standby.rs`, `crates/hotsheet-server/src/routes/sync_kick.rs`.
  - `crates/hotsheet-server/src/auto_group.rs`.
  - Small helpers: `media.rs` (byte ranges and video posters through `HOTSHEET_FFMPEG`), `image_crop_cache.rs` (at most 128 entries and 256 MiB), `custom_views.rs`, `worklist_refresh.rs` (one coalescing worker, 50 ms debounce; its job panics are caught), `health_scan.rs` (single-flight `/health` listing with a 250 ms budget).
- Responsibility:
  - Resolve checkouts and their linked sources.
  - Merge paged reads, capped at 500 unpaged rows, with `x-hotsheet-truncated` and `x-hotsheet-partial` headers.
  - Route writes by qualified identity to Git (`ops`) or to a provider.
  - Claims, with the worker id taken from the body and a 30-minute default lease.
  - Groups, attachments, trash, transfers, activity, accounts and the GitHub device flow.
  - Worker views, plans and strategies through `hotsheet_cli::coordination`.
  - Feature, standby and ticket-creation settings.
  - Auto-grouping new tickets with the project's AI tool.

  It does not enforce claim-holder checks itself; `ops` does.
- State and persistence:
  - Ticket files go through `FsStore`, the index through `StoreEntry`.
  - Settings are stored at local, shared and global scope.
  - The `github_auth_sessions` map is in memory. It is never pruned and counts as a quiescence blocker.
  - The `x-hotsheet-change-cursor` header is stamped on every response.
- Types and entry points: `do_create`, `do_update_with_ticket`, `do_assign` and `do_close` (`crates/hotsheet-server/src/routes/ticket_writes.rs:79-436`); `checkout_ticket_owner` (`crates/hotsheet-server/src/routes/checkout_tickets/mod.rs:179`); `open_project` (`crates/hotsheet-server/src/routes/checkouts/mod.rs:210`); `spawn_auto_grouper` (`crates/hotsheet-server/src/auto_group.rs:55`).
- Concurrency:
  - Handlers run offloaded.
  - `open_project` schedules a worklist regeneration and a single-flight setup refresh.
  - The auto-grouper has one OS thread plus one Tokio task. It coalesces for 3 s per store and gives a turn at most 3 minutes (`crates/hotsheet-server/src/auto_group.rs:32-37`).
  - `GET /checkouts/{ref}/workers` is a long poll, up to 30 s.
- Errors and recovery:
  - A stale expected token gives 409 `ticket_changed`.
  - The checkout batch validates every token before its first write. A write failure partway still commits and publishes the earlier writes (`crates/hotsheet-server/src/routes/checkout_tickets/mod.rs:909-1018`).
  - The primary `/batch` is best effort per id.
  - A create with a follow-up link that fails returns 201 with `follow_up_link_error`.
- Configuration and environment: the `auto_group_new_tickets` shared setting; feature-area settings; the `push_policy` seed on project open.
- Invariants:
  - Routing is by `(connection_id, native_id)`.
  - Provider sources are refused for store-only operations, with 409 naming the provider.
  - A cross-store `move` needs `confirm: true`.
- Events: see the route inventory in the chapter.
- Tests: `crates/hotsheet-server/tests/http.rs` (`checkout_scoped_ticket_routes_aggregate_and_resolve_linked_stores`, `claim_next_release_renew_over_http`, `checkout_batch_queues_one_path_scoped_commit_for_its_ticket_writes`), `crates/hotsheet-server/tests/groups_http.rs`, `crates/hotsheet-server/tests/claim_blocked.rs`, `crates/hotsheet-server/tests/project_relocation.rs`, and `crates/hotsheet-server/src/auto_group.rs` (`hs2_fn018q_new_tickets_are_grouped_in_one_coalesced_turn`). The leak of device-flow sessions is untested.

## Store hosting and presence (`serverHosting`)

- Modules: `crates/hotsheet-server/src/multistore.rs` (`StoreHost`, `stores.json`, index and permission-rule paths), `hosting.rs` (host, unhost, project hosting, repository monitors, unhost sweep requests, instance publication), `presence.rs` (stream leases with a 60 s reconnect gap and a 600 s unhost grace), `mcp_presence.rs` and `crates/hotsheet-server/src/routes/mcp_presence.rs` (MCP leases with a 15 s heartbeat and a 45 s TTL).
- Responsibility: one entry per hosted store, holding its index, corrupt cache and freshness; per-store writer locks and instance files; re-hosting on demand; unhosting stores opened by a project once no live lease, terminal, client drive or live claim needs them.
- State and persistence: `<home>/stores.json` (`{ "stores": [...] }`; missing or malformed reads as empty); `<home>/index/<id>.v<schema>.sqlite`; `<home>/permissions/<store id>.json`; the instance and lock files described under `serverLifecycle`.
- Types and entry points: `StoreHost::{register, get, locations}` (`crates/hotsheet-server/src/multistore.rs:217-296`), `host_configured_stores_deferred` (`crates/hotsheet-server/src/hosting.rs:438`), `publish_instances` (`crates/hotsheet-server/src/hosting.rs:330`), `request_unhost_sweep` (`crates/hotsheet-server/src/hosting.rs:302`), `sweep_unhosted` (`crates/hotsheet-server/src/routes/store_scoped.rs:294`).
- Concurrency:
  - The `stores` mutex is never held during disk I/O (`crates/hotsheet-server/src/multistore.rs:280-296`).
  - A cold-init lock exists per store id; its entries are never removed.
  - The unhost sweeper is one Tokio task that waits on a `Notify` and never observes `stopping()`.
  - `watch_checkout_repository` holds `repository_watchers` across a Git status snapshot (`crates/hotsheet-server/src/hosting.rs:223-229`).
- Errors and recovery: `get` refuses an entry whose on-disk instance id has changed. A failed relink rolls back the new host.
- Configuration and environment: `HOTSHEET_HOME`.
- Invariants:
  - Only project-opened stores are unhosted. The primary and configured stores stay hosted.
  - `GET .../duplicate-backlinks` hosts stores through `host_store`, so the sweep never unhosts them.
- Tests: `crates/hotsheet-server/tests/http.rs` (`closing_the_last_open_project_unhosts_its_store_until_it_is_needed_again`, `an_untagged_change_stream_keeps_every_project_store_hosted`), `crates/hotsheet-server/src/presence.rs` (`a_poll_lease_outlives_its_request_by_the_reconnect_gap`), `crates/hotsheet-server/src/mcp_presence.rs` (`transition_matrix_over_two_leases_sharing_a_terminal`).

## Change stream (`serverEvents`)

- Modules: `crates/hotsheet-server/src/events.rs` (`ChangeEvent`, `EventLog`, `emit_change`, index-write repair, worklist scheduling, sync and repository announcements, turn events), `turn_stream.rs` (bounded turn projection: 128 events and 64 KiB), `notifications.rs` (in-memory inbox, unbounded, not persisted). Stream routes live in `crates/hotsheet-server/src/routes/store_scoped.rs`.
- Responsibility: give each event its cursor, keep the 512-event ring, broadcast on the 256-slot bus, serve `/ws/sync` and `/ws/poll`, and stamp the change-cursor header. Failed index writes are repaired with a backoff from 1 s up to 60 s (`crates/hotsheet-server/src/events.rs:160-170`).
- State and persistence: memory only. A restart resets the cursor to 0.
- Types and entry points: `emit` (`crates/hotsheet-server/src/events.rs:203`), `changed_in` (`crates/hotsheet-server/src/events.rs:235`), `removed_in` (`crates/hotsheet-server/src/events.rs:296`), `announce` (`crates/hotsheet-server/src/events.rs:486`, bus only), `EventLog::since` (`crates/hotsheet-server/src/events.rs:664`), `ws_sync` and `poll_events` (`crates/hotsheet-server/src/routes/store_scoped.rs:160`, `crates/hotsheet-server/src/routes/store_scoped.rs:471`).
- Concurrency: the sequence number is assigned under the `event_log` lock, and the broadcast happens after the lock is released. Two concurrent emitters can therefore broadcast out of cursor order (`crates/hotsheet-server/src/events.rs:639-646`). The poll re-reads the ring, so a polling client always sees cursor order.
- Errors and recovery:
  - A WebSocket closes on lag.
  - A poll returns `overflow` when its cursor fell off the ring or the broadcast lagged.
  - Subscribers that lag, such as the router and the auto-grouper, lose events with a warning.
- Configuration and environment: `POLL_DEFAULT_MS` is 25 000 and `POLL_MAX_MS` is 55 000 (`crates/hotsheet-server/src/routes/store_scoped.rs:441-442`).
- Invariants: an event is recorded in the ring before it is broadcast. A long poll subscribes before it reads the ring.
- Tests: `crates/hotsheet-server/src/events.rs` (`falling_behind_the_ring_signals_overflow`, `the_background_retry_backoff_is_bounded_and_resets_when_drained`), `crates/hotsheet-server/tests/sync_ws.rs`, and `crates/hotsheet-server/tests/http.rs` (`long_poll_hands_back_a_cursor_then_replays_events_since_it`). The WebSocket lag path is untested.

## File reconciliation and search (`serverIndex`) and Git sync loop (`serverSync`)

- Modules: `crates/hotsheet-server/src/watcher.rs`, `store_watch_scope.rs` (watches the store root on Windows and `tickets/` elsewhere), `crates/hotsheet-server/src/target_store_watch.rs` (watches linked stores that are not hosted, with a 1 s recheck and a 10-minute idle), `code_ref_watch.rs` (watches remote-tracking refs), `crates/hotsheet-server/src/sync_loop.rs`, `claim_conflict_push.rs` (lost-claim notices), `sync_ai.rs` (asks an AI to finish a stuck sync, limited by `HOTSHEET_SYNC_AI_ALLOWED_TOOLS`).
- Responsibility:
  - Keep each hosted store's index fresh from native file events. Until the native stream is live, a 250 ms content-comparing poller bridges the gap.
  - Debounce for 150 ms.
  - Re-arm after a store is recreated.
  - Run the checkout repository monitors.
  - Sync every hosted store, plus registered Git stores that are not hosted, with their remotes; purge trash; sync group registries.
- State and persistence: the index files. Freshness atomics tell callers whether an index miss can be trusted.
- Types and entry points: `spawn_watcher` (`crates/hotsheet-server/src/watcher.rs:166`), `spawn_sync_loop` (`crates/hotsheet-server/src/sync_loop.rs:356`), `next_delay` (`crates/hotsheet-server/src/sync_loop.rs:306`), `request_store_sync` (`crates/hotsheet-server/src/lib.rs:1448`).
- Concurrency:
  - The watchers are OS threads.
  - The sync loop is one OS thread that enters the runtime handle. Its base cadence is 30 s. Write kicks arrive over `std::sync::mpsc`. Failures back off up to 300 s, and a lost race gets up to 3 re-passes 2 s apart (`crates/hotsheet-server/src/sync_loop.rs:32-40`).
  - Passes are skipped while quiescing.
- Errors and recovery:
  - The sync loop never stops before the process exits. The kick sender lives in `AppState`, so `SyncHandle` has no effect (`crates/hotsheet-server/src/sync_loop.rs:94-98`).
  - Kicks during a backoff are ignored until the deadline.
- Configuration and environment: `DEFAULT_INTERVAL`, `TRASH_PURGE_INTERVAL` (6 h) and `HOTSHEET_SYNC_AI_ALLOWED_TOOLS`.
- Invariants: a watcher's thread exits when its `WatchHandle` is dropped, because its channel disconnects.
- Tests: `crates/hotsheet-server/src/watcher.rs` (`the_watch_loop_walks_every_freshness_transition`, `hs2_r6fz54_native_watcher_rearms_after_store_recreation`), `crates/hotsheet-server/src/sync_loop.rs` (`kick_during_backoff_neither_resets_nor_shortens_the_backoff`), `crates/hotsheet-server/tests/sync_kick.rs`, `crates/hotsheet-server/tests/sync_ai.rs`, and `crates/hotsheet-server/src/target_store_watch.rs` (`hs2_r49d43_a_write_before_the_stream_is_live_is_recorded_by_the_recheck`).

## Repository browser and code review (`serverRepository`)

- Modules: `crates/hotsheet-server/src/code_review.rs` (finds commits by slug in the subject or a `Refs:` trailer, reading at most 2000 log entries, and launches the difftool detached), `crates/hotsheet-server/src/repository_browser.rs` (status, init, file and commit pages, host file actions through OS openers).
- Responsibility: read-only repository views for open checkouts, and the host launches behind `HostAccess`.
- State and persistence: none. The repository monitors are owned by `serverHosting`.
- Types and entry points: `code_review::{discover_for_ticket, launch, launch_repository}`; `repository_browser::{discover, initialize, configure_origin, files_page, commits_page, act_on_file}`.
- Concurrency: Git subprocesses run in offloaded handlers; launches are detached processes.
- Errors and recovery: a missing repository gives 404; a Git failure is mapped to a status.
- Configuration and environment: the user's difftool configuration.
- Invariants: every launcher takes `HostAccess`, and a policy test pins this.
- Events: `repository_changed`.
- Tests: `crates/hotsheet-server/tests/http.rs` (`code_review_discovers_ticket_commits_and_only_launches_returned_targets`, `repository_status_endpoint_reports_real_git_state`), and the in-file tests in both modules.

## Command schedule runner (`serverSchedule`) and project tools

- Modules: `crates/hotsheet-server/src/routes/command_schedules.rs`, `crates/hotsheet-server/src/routes/project_tools.rs` (analytics, reports, commands, views, notifications, TTS), `crates/hotsheet-server/src/commands.rs` (configured command runs), `tts.rs`.
- Responsibility:
  - Store local command schedules.
  - Preview and deliver due AI occurrences: create one Up Next ticket, claim it for the session worker, and hand off a turn.
  - Reconcile interrupted deliveries on the first pass.
  - Run configured shell commands with an output history.
- State and persistence: local settings, written with compare-and-set (`Settings::set_if`; 409 on a race). Command settings need `If-Match`, whose ETag is a SHA-256 of the definitions.
- Types and entry points: `spawn_command_schedule_runner` (`crates/hotsheet-server/src/routes/command_schedules.rs:1515`), `deliver_one` (`crates/hotsheet-server/src/routes/command_schedules.rs:1055`), `CommandManager`.
- Concurrency:
  - The runner is a Tokio task that runs a blocking pass, then sleeps until the next due time (at most 60 s), a `command_schedule_kick`, or `stopping()`.
  - Each command run has three OS threads, and the exit watcher polls `try_wait` every 20 ms (`crates/hotsheet-server/src/commands.rs:332-361`).
- Errors and recovery: a failed manual turn notes the ticket and releases the claim. An uncertain exact turn is not replayed after a restart.
- Configuration and environment: `SHELL` or `COMSPEC`; command output is capped at 10 000 lines, with a 2 s drain grace.
- Invariants: these routes are not host-gated, by design (`docs/04-core-server-cli.md`).
- Events: `command_schedules_updated`, `command_updated`, `views_updated`, `notification_acknowledged`, `notification`.
- Tests: `crates/hotsheet-server/src/routes/command_schedules.rs` (`hs2_mb3bqe_host_startup_delivers_existing_pending_one_time_ticket`, `hs2_g5j2e7_dst_gap_runs_at_next_valid_time_and_fold_once`), `crates/hotsheet-server/tests/http.rs` (`configured_commands_stream_output_keep_history_and_cancel`), `crates/hotsheet-server/src/commands.rs` (`hs2_wdnj0x_a_held_pipe_still_completes_after_the_drain_bound`).

## AI session runtime (`serverDrive`) and distributed driving loop (`serverDistWork`)

- Modules: `crates/hotsheet-server/src/client_drive.rs`, `crates/hotsheet-server/src/routes/permissions.rs` (the drive, AI settings and tools, and permission routes), `crates/hotsheet-server/src/routes/session_messages.rs` (session push routes and adapters), `crates/hotsheet-server/src/routes/feedback_session.rs` and `crates/hotsheet-server/src/routes/feedback_journal.rs` (routing feedback notes to sessions, with a durable per-note journal), `crates/hotsheet-server/src/dist_work_loop.rs`, `ai_tool_discovery.rs` (a 10 s memo), `crates/hotsheet-server/src/tool_capacity_signals.rs`, `crates/hotsheet-server/src/stuck_causes.rs`.
- Responsibility:
  - Chat drives: one connection per chat, sequential turns, interrupt where supported, and a permission mode per chat.
  - Session push into interactive terminals.
  - Feedback routing, in this order: the live claim worker, the asking AI, the chosen or oldest primary terminal, then a client drive.
  - The opt-in driving loop.
- State and persistence: `<home>/drive/<store>/sessions.json`, holding sessions and live drives and written by temp file and replace (`crates/hotsheet-server/src/client_drive.rs:839-866`); `homes/` per connection; `<home>/feedback-deliveries/<ULID>.json` (`crates/hotsheet-server/src/routes/feedback_journal.rs:80-109`).
- Types and entry points: `ClientDriveManager::{create_or_attach, begin_turn, finish_turn, interrupt, close, take_orphaned_drives}` (`crates/hotsheet-server/src/client_drive.rs:362-833`); `spawn_dist_work_loop` (`crates/hotsheet-server/src/dist_work_loop.rs:628`).
- Concurrency:
  - Each turn runs on its own OS thread.
  - The driving loop is one OS thread with a 15 s interval. It drives stores one after another; `max_in_flight` caps the claims per pass (`crates/hotsheet-server/src/dist_work_loop.rs:113-160`).
  - A queued feedback turn waits up to 1 h on `drive_updated` events.
- Errors and recovery:
  - Corrupt `sessions.json` fails server startup.
  - Drives left open by the last run are released once.
  - A feedback journal write failure gives 503 and sends nothing; a failed send removes its entry.
- Configuration and environment: `--drive-tool`, `--drive-workers`, `--drive-lease-min`, `--drive-worker`. A drive's environment carries `HOTSHEET_SERVER` and `HOTSHEET_SECRET` (`crates/hotsheet-server/src/routes/permissions.rs:1144-1147`).
- Invariants: a chat runs at most one turn at a time, and a second gets 409.
- Events: `drive_updated`, `turn_event`.
- Tests: `crates/hotsheet-server/src/client_drive.rs` (`drives_left_open_by_a_previous_run_are_reported_once`), `crates/hotsheet-server/src/dist_work_loop.rs` (`max_in_flight_caps_driven_tickets_across_stores`), `crates/hotsheet-server/tests/session_messages.rs`, and `crates/hotsheet-server/src/tool_capacity_signals.rs` and `crates/hotsheet-server/src/stuck_causes.rs` (in-file). The live drive in `dist_work_loop` is untested.

## Permission bridge (`serverPermissions`)

- Modules: `crates/hotsheet-aitools/src/permission.rs`, `crates/hotsheet-aitools/src/permission_vault.rs`, `permission_pattern.rs` (reviewed matchers), `permission_suggestions.rs` (secret span hints that hold offsets only), `crates/hotsheet-server/src/routes/orchestration_permissions.rs` (orchestrator asks on the same bridge), `permission_startup_tests.rs`.
- Responsibility:
  - Remembered rules auto-resolve a request.
  - Otherwise the request joins a FIFO queue. Concurrent asks are preserved and can be answered in any order.
  - Durable rules are encrypted.
  - Orchestrator asks share the bridge, but their answers go to the orchestration runtime.
- State and persistence:
  - `<home>/permissions/<store id>.json` and `checkout-<id>.json`. Envelope v4, with a 256-bit AEAD key in the OS credential store, a sibling `flock` file, and caps of 8 MiB per file and 5 MiB of plaintext (`crates/hotsheet-aitools/src/permission_vault.rs:19-24`).
  - Keychain reads happen before the lock is taken.
- Types and entry points: `PermissionBridge`, `SharedPermissionBridge`, `request_timeout_with_pending_async` (`crates/hotsheet-aitools/src/permission.rs:1285`), `DEFAULT_PERMISSION_TIMEOUT` = 24 h (`crates/hotsheet-aitools/src/permission.rs:224`).
- Concurrency: the bridge is a `Mutex` with a `Condvar` and `Notify`. An async ask parks no thread.
- Errors and recovery:
  - Dropping the ask future withdraws it as a one-time deny.
  - The timeout denies.
  - A stopping server answers 503.
  - A failed vault load leaves durable rules locked; the bridge stays fail-closed.
- Configuration and environment: `HOTSHEET_PERMISSION_PROJECT` (in `live.rs`).
- Invariants: no rule file contains rule text in the clear; action fingerprints are HMAC-SHA256 with the vault key.
- Events: `permission_asked`, `permission_resolved`, `permission_rules_changed`.
- Tests: `crates/hotsheet-aitools/src/permission.rs` (`concurrent_requests_are_preserved_and_answered_out_of_order`, `dropping_an_async_ask_withdraws_it_once_and_leaves_no_result`), `crates/hotsheet-aitools/src/permission_vault.rs` (`encrypted_round_trip_rejects_tampering_and_keeps_key_out_of_files`), `crates/hotsheet-server/src/permission_startup_tests.rs` (`stalled_vault_loader_leaves_http_available_and_rules_locked`), and `crates/hotsheet-server/tests/http.rs` (`permission_round_trip_lists_answers_and_unblocks_the_tool`).

## AI tool adapters (`serverAiTools`)

- Modules:
  - Drive shapes: `crates/hotsheet-aitools/src/acp.rs` (ACP drive for OpenCode), `crates/hotsheet-aitools/src/claude.rs` (Claude channel drive), `crates/hotsheet-aitools/src/codex.rs` (Codex app-server JSON-RPC client; request timeout 30 s, turn timeout 600 s), `appserver.rs`, `spawn.rs` (Antigravity), `drive.rs`, `host.rs`.
  - Usage and halts: `crates/hotsheet-aitools/src/claude_transcript_usage.rs` (Claude transcript usage), `crates/hotsheet-aitools/src/codex_rollout_watch.rs` (Codex rollout halts and usage), `vnode_watch.rs`.
  - Launch and run: `crates/hotsheet-aitools/src/launch_safety.rs` (shim directory, isolated `CODEX_HOME`, pinned MCP home), `crates/hotsheet-aitools/src/live.rs` (real-tool glue), `crates/hotsheet-aitools/src/safe_trigger.rs` (the shared safe trigger; sets `HOTSHEET_ACTOR_ROLE=ai`).
  - Discovery and plumbing: `crates/hotsheet-aitools/src/model_catalog.rs` (`HOTSHEET_AI_TOOL_PROBES=off`), `crates/hotsheet-aitools/src/probe.rs` (bounded probes in their own process group; 5 s default; `stop_probes`), `crates/hotsheet-aitools/src/procio.rs` (piped NDJSON child stdio).
  - `crates/hotsheet-aitools/src/session_push` (`mod.rs`, `claude.rs` inbox socket, `codex.rs` `codex queue`, `opencode.rs` HTTP Basic, `opencode_v2.rs`, `http.rs`).
  - Small: `registry.rs` (connection registry and busy view), `ports.rs`, `system.rs`, `structured_failure.rs`, `tool_progress.rs`, `usage_resets.rs`, `lib.rs`.
- Responsibility: one `Drive` interface whose capabilities are probed. The transport is chosen from the plugin manifest, never by tool id (`crates/hotsheet-aitools/src/host.rs:20-66`). Permission return paths differ by tool:
  - Claude: through its PreToolUse hook to `/permissions/ask`.
  - Codex: in-protocol approval requests answered through the bridge. With no policy set, every approval is allowed (`crates/hotsheet-aitools/src/codex.rs:244`).
  - OpenCode ACP: `session/request_permission` is answered in-process by choosing a reject option, and never reaches the bridge (`crates/hotsheet-aitools/src/acp.rs:74-90`).
  - Antigravity: no permission channel.
- State and persistence: the probe process journal under the home; tool homes under the drive root; isolated temp shim directories.
- Types and entry points: `Drive`, `TurnEvent`, `TurnControl`, `SafeTrigger`, `prepare_trigger_with_home`, `SessionPush`.
- Concurrency: reader threads per stream child; a discovery thread; vnode watch threads.
- Errors and recovery: `stop_probes` kills the probes in flight at shutdown. Structured failures map rate and usage limits to capacity signals.
- Configuration and environment: `HOTSHEET_WORKER_ID`, `HOTSHEET_ACTOR_ROLE`, `HOTSHEET_ACTOR_ID`, `CODEX_HOME`, `HOTSHEET_CLAUDE_PRETOOLUSE`, `HOTSHEET_CLAUDE_HEADLESS_AUTO`, `HOTSHEET_ANTIGRAVITY_CLI_DIR`.
- Invariants: callers never branch on the tool id.
- Tests: `crates/hotsheet-aitools/src/tests.rs` (`trigger_codex_uses_the_app_server_and_registers_a_connection`), `crates/hotsheet-aitools/src/codex.rs` (`no_policy_auto_approves_and_maps_reply_tokens`), `crates/hotsheet-aitools/src/claude.rs` (`headless_stream_marks_pre_tool_permission_bridging_except_in_auto_mode`), `crates/hotsheet-aitools/src/session_push/tests.rs`, and `crates/hotsheet-aitools/src/probe.rs` (`hs2_9820jg_the_deadline_never_signals_a_reaped_leader`). The ACP reject path is untested.

## Plugin manifests and setup (`serverPlugins`)

- Modules: `crates/hotsheet-plugins/src/lib.rs` (manifest and loader), `crates/hotsheet-plugins/src/setup.rs` (project setup), `crates/hotsheet-plugins/src/guidance_sources.rs` (pinned sources in `.hotsheet2/guidance-sources.json`; only registration uses the network), `crates/hotsheet-plugins/src/guidance_updates.rs`, `crates/hotsheet-plugins/src/guidance_recommendations.rs`, `crates/hotsheet-plugins/src/guidance_interview.rs`, `rulesync_updates.rs`, `rulesync_consolidation.rs`, `shared_host.rs`, `mcp_local_layer.rs`. Server routes: `crates/hotsheet-server/src/routes/guidance.rs`, `POST /setup/{tool}`.
- Responsibility:
  - Load the bundled plugins (Claude, Codex, Antigravity, OpenCode) and `<home>/plugins/<id>/`. First-party ids win a collision.
  - Write the managed instruction sections, skills, MCP entries, hooks and shared settings in a project, plus the Claude local MCP layer for the machine.
  - Pinned guidance review.

  The server and the CLI share one implementation; `hotsheet-cli setup` is the headless path.
- State and persistence: project files (`CLAUDE.md`, `AGENTS.md`, `.claude/skills`, `.agents/skills`, `.mcp.json`, `.codex/config.toml`, `opencode.json`, `.claude/settings.local.json`, `.codex/hooks.json`, `.hotsheet2/settings.json`), `~/.claude.json` and `<home>/rulesync-reviews`.
- Types and entry points: `run_setup_in`, `refresh_setup_in` (plan, then apply), `Manifest` (`crates/hotsheet-plugins/src/lib.rs:81`).
- Concurrency: none of its own. The server runs setup on the blocking pool, single-flight per checkout.
- Errors and recovery: a bad plugin directory is skipped silently.
- Configuration and environment: `HOTSHEET_HOME`, `HOTSHEET_CLAUDE_USER_CONFIG`, `CLAUDE_CONFIG_DIR`.
- Invariants: `write_file` skips identical bytes and replaces through a temp file, so setup is byte-idempotent.
- Tests: `crates/hotsheet-plugins/src/tests.rs` (`every_builtin_carries_the_full_default_guidance`), `crates/hotsheet-plugins/src/setup.rs` (`refresh_migrates_sharing_tools_and_is_byte_idempotent`), and `crates/hotsheet-server/tests/http.rs` (`setup_endpoint_prepares_the_project_like_the_cli`).

## Notifications (`serverNotify`, `serverNotifiers`)

- Modules: `crates/hotsheet-server/src/notification_router.rs`, `crates/hotsheet-server/src/project_activity.rs` (claim tracker: idle grace 120 s, idle window 30 min, updated window 60 min), `crates/hotsheet-server/src/routes/notifiers.rs` (12 admin routes), and `crates/hotsheet-notify/src`:
  - `crates/hotsheet-notify/src/dispatch.rs` (rules, cooldowns, retries)
  - `crates/hotsheet-notify/src/config.rs` (`notifications.json`)
  - `crates/hotsheet-notify/src/admin.rs` (idempotent administration shared with the CLI)
  - `delivery_log.rs` (journal, compacting at 1000 entries down to 500)
  - `crates/hotsheet-notify/src/builtin` (email, ntfy, Slack, speak, Teams, Twilio, webhook)
  - `external.rs` (subprocess notifiers, with a timeout clamped to 1–300 s)
  - `registry.rs`, `render.rs`, `message.rs`, `settings.rs`, `transitions.rs`, `views.rs`, `links.rs`, `http.rs`, `capability.rs`, `notifier.rs`
  - `crates/hotsheet-notify/src/remote_change` (`crates/hotsheet-notify/src/remote_change/ably.rs` and `crates/hotsheet-notify/src/remote_change/hub.rs`). These are inbound "remote advanced" hints. The CLI uses them; the server does not start the hub.
- Responsibility: turn bus events into provider-neutral messages, match rules, hold messages for quiet hours and hourly limits, deliver with retry, journal each delivery, and publish the standby snapshot.
- State and persistence: `<home>/notifications.json` (secret-free; it holds key-registry references only); `<home>/notification-deliveries.jsonl` with lock files; `<home>/notifiers/<id>/manifest.toml`; `<home>/remote-change/connections.json`.
- Types and entry points: `spawn_notification_router` (`crates/hotsheet-server/src/notification_router.rs:131`), `route_change_event`, `flush_held`, `Dispatcher::dispatch` (`crates/hotsheet-notify/src/dispatch.rs:143`).
- Concurrency:
  - A Tokio forwarder feeds one blocking worker over an unbounded channel, with a 60 s tick.
  - Per-store activity timers.
  - The journal locks never enclose transport work, and the flush lock is try-acquired.
- Errors and recovery:
  - A lagging router loses events.
  - Retryable failures are retried after 2 s and 8 s; then the delivery is given up.
  - Cooldowns survive a restart through the journal.
  - An interrupted journal entry is recovered as unknown and not resent.
- Configuration and environment: `link_base` from `<home>/client.json`; per-channel settings.
- Invariants: credentials are resolved only at delivery time.
- Tests: `crates/hotsheet-server/tests/notification_delivery.rs` (`ticket_transitions_permission_requests_and_published_notifications_reach_channels`), `crates/hotsheet-notify/src/dispatch.rs` (`retries_retryable_failures_with_backoff_then_gives_up`), `crates/hotsheet-notify/src/delivery_log_tests.rs` (`hs2_mfd44d_two_processes_and_restarts_send_one_summary`), `crates/hotsheet-notify/src/remote_change/tests.rs`, and `crates/hotsheet-server/src/project_activity.rs` (`release_then_grace_reports_finished_once_with_completed_tickets`).

## Terminal API and broker client (`serverTerminal`)

- Modules: `crates/hotsheet-server/src/routes/terminals` (all files; 34 endpoints). The large files are `crates/hotsheet-server/src/routes/terminals/mod.rs` (settings, limits, helpers), `crates/hotsheet-server/src/routes/terminals/signals.rs` (list, read, input, clear, kill, hook signals, permission answers and rules), `crates/hotsheet-server/src/routes/terminals/launch.rs` (`POST /terminals`, exit watchers, restart resume), `crates/hotsheet-server/src/routes/terminals/attach.rs` (WebSocket), `crates/hotsheet-server/src/routes/terminals/replay.rs` (replay framing), `crates/hotsheet-server/src/routes/terminals/claude_usage.rs` and `crates/hotsheet-server/src/routes/terminals/codex_watch.rs` (usage follows). The small files are `hook_turns.rs` (busy state from hook turn boundaries) and `usage_store.rs` (which store records a terminal's usage). The server-side broker client is `crates/hotsheet-server/src/terminal_broker.rs`; `terminal_broker_startup.rs` holds its startup wait (50 ms probes, 2 s grace, 30 s ceiling). The Windows stub is `terminal_broker_windows.rs`. The rest: `terminal_names.rs` (tab names in the `terminal.names` local setting, 120-character cap, a process-wide write lock), `terminal_launch_relay.rs` (the relay's broker launcher), `opencode_v2_terminal.rs` (the `--opencode-v2-terminal` PTY foreground helper).
- Responsibility: launch shells, commands and AI tools in PTYs; list, read, write, clear, rename and kill them; serve the attach WebSocket; record hook signals (halt, question, AI connection, Claude turn and activity); answer permission requests and manage remembered rules. It does not gate launches on `HostAccess` (see Discrepancies in the chapter).
- State and persistence:
  - In memory: `terminal_questions`, `terminal_ai_connections`, `permission_rule_continuations` (at most 64, 1 h TTL), `feedback_primary_choices` and `terminal_hook_turns`, each behind `lock_or_recover` (`crates/hotsheet-server/src/routes/terminals/signals.rs:1554-1569`).
  - Files: tab names in local settings (`crates/hotsheet-server/src/terminal_names.rs:13`); `claude-usage/<terminal>.json` and `codex-usage/<terminal>.json` under the home, which hold ids and paths only (`crates/hotsheet-server/src/routes/terminals/claude_usage.rs:40`, `crates/hotsheet-server/src/routes/terminals/codex_watch.rs:40`); terminal limits in global settings.
- Types and entry points: `open_terminal` (`crates/hotsheet-server/src/routes/terminals/launch.rs:23`), `attach_terminal` (`crates/hotsheet-server/src/routes/terminals/attach.rs:32`), `resolve_permission` (`crates/hotsheet-server/src/routes/terminals/signals.rs:1407`), `TerminalBroker::{ensure, discover, call, kill_all}` (`crates/hotsheet-server/src/terminal_broker.rs:58`, `crates/hotsheet-server/src/terminal_broker.rs:107`, `crates/hotsheet-server/src/terminal_broker.rs:151`, `crates/hotsheet-server/src/terminal_broker.rs:121`), and the restart hooks `resume_broker_terminal_sessions` and `prune_orphaned_terminal_names` (`crates/hotsheet-server/src/routes/terminals/launch.rs:1103`, `crates/hotsheet-server/src/routes/terminals/launch.rs:1164`).
- Concurrency:
  - Every launch runs in `spawn_blocking`.
  - 500 ms `tokio::spawn` liveness loops per AI or shell terminal poll the process or the broker `info` (`crates/hotsheet-server/src/routes/terminals/launch.rs:794-797`, `crates/hotsheet-server/src/routes/terminals/launch.rs:1045-1048`).
  - An attach has three tasks: output pump, OSC scanner and writer. The writer is biased toward pongs on an 8-slot control queue.
  - One restart mutex serializes broker respawn (`crates/hotsheet-server/src/terminal_broker.rs:195`).
- Errors and recovery:
  - A broker I/O error returns 500. An old broker's `bad request` falls back or reports unsupported (`crates/hotsheet-server/src/terminal_broker.rs:171-174`).
  - `call` after a broker crash spawns a fresh empty broker (`crates/hotsheet-server/src/terminal_broker.rs:188-231`). The pollers then see `not_found` and release session claims.
  - A broker start failure at startup aborts the server (`crates/hotsheet-server/src/main.rs:416`).
  - A store still loading permission rules answers 503 (`crates/hotsheet-server/src/routes/terminals/signals.rs:1496-1504`).
- Configuration and environment: `--no-terminal-broker`, the hidden `--terminal-broker-process` and `--terminal-launch-relay` flags; `HOTSHEET_WORKER_ID` in a terminal spec; terminal limits (`/terminal-limits`, memory-derived defaults).
- Invariants:
  - Every route checks for the broker and otherwise uses the in-process manager.
  - The attach replay is the first frame, so every output byte lands once, in either the replay or the live stream (`crates/hotsheet-terminals/src/terminal.rs:384`).
- Events: `terminal_ai_connection`, `terminal_hook_report`, `terminal_halted`, `terminal_question`, `terminal_renamed`, `terminal_feedback_primary`, `terminal_exited` (AI connections only), `permission_resolved` and `permission_rules_changed` (`crates/hotsheet-server/src/routes/terminals/signals.rs:1011-1024`, `crates/hotsheet-server/src/routes/terminals/signals.rs:1711-1728`, `crates/hotsheet-server/src/routes/terminals/launch.rs:938-956`).
- Tests: `crates/hotsheet-server/tests/terminal_ws.rs`, `crates/hotsheet-server/tests/terminal_replay.rs`, `crates/hotsheet-server/tests/terminal_broker.rs`, `crates/hotsheet-server/tests/broker_startup.rs` (`long_home_starts_real_server_and_detached_broker_and_preserves_terminals_on_restart`), `crates/hotsheet-server/tests/remote_terminals.rs`, `crates/hotsheet-server/tests/terminal_clear.rs`, `crates/hotsheet-server/tests/windows_terminal_fallback.rs` (`hs2_2td7m7_windows_server_hosts_terminal_in_process`), and `crates/hotsheet-server/tests/http.rs` (`broker_mode_routes_terminals_and_survives_a_server_restart`, `a_restart_prunes_names_of_terminals_that_did_not_survive`). Broker death during a live attach is untested.

## Detached broker (`brokerService`, `brokerSocket`, `brokerManager`, `brokerTerminals`, `brokerSizing`)

- Modules: `crates/hotsheet-terminals/src/broker.rs` (request loop and client), `crates/hotsheet-terminals/src/broker_socket.rs` (paths and ownership), `crates/hotsheet-terminals/src/terminal.rs` (one PTY), `crates/hotsheet-terminals/src/sizing.rs` (size arbiter), `crates/hotsheet-terminals/src/bin/broker.rs` (binary entry). Small modules:
  - `manager.rs`: the `(project, id)` map with lazy spawn and reap. It keeps failed AI launches up to 120 s, at most 16.
  - `input_queue.rs`: a 1 MiB input budget with one writer thread.
  - `pty_io.rs`: non-blocking PTY master I/O (Unix).
  - `viewer_output.rs`: a 32-frame viewer queue with coalescing.
  - `busy.rs`: OSC 133 busy inference.
  - `osc.rs`: OSC 7, 8 and 9, with a 2048-byte payload cap.
  - `env.rs`: drops `TSX_`, `npm_`, `NODE_`, `HOTSHEET_` and Git variables.
  - `wire_bytes.rs`: base64 or legacy byte arrays.
  - `limits.rs`: physical memory.
  - `bootstrap.rs`: the macOS Mach namespace probe.
  - `bin/fake_agent.rs`: `hs-fake-agent`, a test PTY emitter.
  - `lib.rs`: re-exports; the socket, relay and PTY I/O modules are Unix-only.
- Responsibility: own PTYs and their replay rings outside the server process, so terminals survive a server restart. It does not authenticate peers beyond file permissions.
- State and persistence: memory only — one `TerminalManager`, the per-terminal replay ring, and push state with port, password and session (`crates/hotsheet-terminals/src/broker.rs:297-327`). On disk:
  - the socket at `/tmp/hotsheet2-brokers-<euid>/<32 hex>.sock` (`crates/hotsheet-terminals/src/broker_socket.rs:154-166`);
  - a 0700 directory, a 0600 socket and a `<socket>.lock` owner lock (`crates/hotsheet-terminals/src/broker_socket.rs:220-238`);
  - a namespace-selection lock that adopts a legacy `<home>/broker/<project>.sock` (`crates/hotsheet-terminals/src/broker_socket.rs:194-254`).
- Types and entry points: `run_broker_process` (`crates/hotsheet-terminals/src/broker.rs:394`), `serve_broker_with_idle` (`crates/hotsheet-terminals/src/broker.rs:465`), `BrokerRequest`, `BrokerResponse` and the stream frames (`crates/hotsheet-terminals/src/broker.rs:70-265`), `Terminal::spawn` (`crates/hotsheet-terminals/src/terminal.rs:674`), `SizeArbiter` (`crates/hotsheet-terminals/src/sizing.rs:77`).
- Concurrency:
  - One Tokio task per connection, plus a housekeeping tick of `min(grace, 250 ms)`.
  - Per terminal, OS threads: the output drain, the input writer, the slave keeper (Unix), the journal exit watcher and trailing resizes (`crates/hotsheet-terminals/src/terminal.rs:598-983`). Terminal state is behind `std::sync::Mutex`.
- Errors and recovery:
  - The broker exits after 5 idle minutes with no terminals and no clients (`crates/hotsheet-terminals/src/broker.rs:44`, `crates/hotsheet-terminals/src/broker.rs:515-529`).
  - `retire_if_idle` lets a server of another build replace an idle broker. A broker hosting terminals is always adopted.
  - A stale socket with a free lock is reclaimed on bind (`crates/hotsheet-terminals/src/broker_socket.rs:325-357`).
  - A dead bootstrap namespace refuses new spawns (`crates/hotsheet-terminals/src/broker.rs:788-793`).
  - Client requests have no timeout (`crates/hotsheet-terminals/src/broker.rs:944-957`).
  - Whether a crashed broker's PTY children are orphaned is unresolved.
- Configuration and environment: `HOTSHEET_PROCESS_JOURNAL_HOME` (blank means unset; `crates/hotsheet-terminals/src/broker.rs:47-52`); `HOTSHEET_BROKER_CAPTURE_STARTUP_STDERR`; `HOTSHEET_TEST_SERVER_OWNER_PID` (tests only); the replay size per terminal: default 256 KiB, clamped to 64 KiB through 64 MiB (`crates/hotsheet-terminals/src/terminal.rs:27-33`).
- Invariants:
  - Snapshot-and-subscribe and publish take the same ring lock (`crates/hotsheet-terminals/src/terminal.rs:384-426`).
  - Size claims belong to a connection and are released when it closes (`crates/hotsheet-terminals/src/sizing.rs:305`).
- Tests: `crates/hotsheet-terminals/src/broker.rs` (`broker_hosts_a_pty_reachable_over_the_socket`, `idle_gc_waits_for_clients_then_exits_when_empty`, `hs2_h0wfjg_retire_only_when_no_terminal_and_no_other_client`), `crates/hotsheet-terminals/src/broker_socket.rs` (`binding_is_exclusive_then_reclaims_stale_socket_after_release`), `crates/hotsheet-terminals/src/terminal.rs` (`snapshot_and_live_subscription_partition_concurrent_output_exactly_once`), `crates/hotsheet-terminals/src/sizing.rs` (`a_disconnecting_driver_self_heals_immediately`), and `crates/hotsheet-terminals/tests/bootstrap_broker.rs`.

## macOS launch relay (`relayService`, `relayLauncher`, `relayBootstrap`, `relayAgent`)

- Modules: `crates/hotsheet-terminals/src/launch_relay.rs` (listener and client), `crates/hotsheet-terminals/src/launch_agent.rs` (LaunchAgent install), `crates/hotsheet-server/src/terminal_launch_relay.rs` (broker launcher run inside the relay), `crates/hotsheet-terminals/src/bootstrap.rs`.
- Responsibility: start brokers from the GUI (Aqua) bootstrap namespace, so a server whose namespace died does not hand it on to new terminals.
- State and persistence: the plist in `~/Library/LaunchAgents`, labelled `com.smalltale.hotsheet2.terminal-relay`, with a suffix for a non-default home (`crates/hotsheet-terminals/src/launch_agent.rs:25`, `crates/hotsheet-terminals/src/launch_agent.rs:138-209`); the socket `/tmp/hotsheet2-brokers-<euid>/relay-<16 hex>.sock`; the log at `<home>/logs/terminal-relay.log`.
- Types and entry points: `install`, `uninstall` and `status` (`crates/hotsheet-terminals/src/launch_agent.rs:383`, `crates/hotsheet-terminals/src/launch_agent.rs:464`, `crates/hotsheet-terminals/src/launch_agent.rs:513`); `RELAY_PROTOCOL` 1 with `hello` and `launch` ops (`crates/hotsheet-terminals/src/launch_relay.rs:30-113`); `run_launch_relay` behind the `--terminal-launch-relay` flag (`crates/hotsheet-server/src/main.rs:235-242`).
- Concurrency: one Tokio listener; the launcher spawns the broker detached with `setsid` and a reaper thread (`crates/hotsheet-server/src/terminal_launch_relay.rs:91-161`).
- Errors and recovery: every `RelayUnavailable` variant (absent, untrusted, unresponsive, mismatch, refused) falls back to a direct spawn and logs a warning (`crates/hotsheet-server/src/terminal_launch_relay.rs:76-87`). Install changes nothing outside an Aqua session and prints next steps.
- Configuration and environment: `HOTSHEET_LAUNCHCTL` (default `/bin/launchctl`), `HOTSHEET_LAUNCH_AGENTS_DIR`, `HOTSHEET_HOME`. The headless path is `hotsheet-cli terminal-relay install`.
- Invariants:
  - Requests are capped at 1 MiB per line. Both sides check the peer uid (`crates/hotsheet-terminals/src/launch_relay.rs:147-179`).
  - The server uses only a relay running the same executable (`crates/hotsheet-terminals/src/launch_relay.rs:376-396`).
  - `XPC_*` job context is never forwarded (`crates/hotsheet-server/src/terminal_launch_relay.rs:18-30`).
- Tests: `crates/hotsheet-terminals/src/launch_relay_tests.rs`, `crates/hotsheet-terminals/src/launch_agent_tests.rs` (`hs2_hyf56y_install_is_idempotent_and_reloads_only_a_changed_definition`), `crates/hotsheet-server/tests/terminal_launch_relay.rs` (`hs2_hyf56y_a_stopped_relay_leaves_its_brokers_running`).

## Standby snapshot and screen saver (`standby`, `saverWatcher`, `saverView`)

- Modules: `crates/hotsheet-server/src/standby_saver.rs` (writer; its format lives in `crates/hotsheet-ticketing/src/screensaver.rs`), `clients/macos-screensaver/Sources/HotSheetSaverView.swift`.
- Responsibility: mirror the live state that clients read into the screen-saver sandbox, and show it in the OS screen-saver host. Neither side has network access, and the saver does not poll.
- State and persistence:
  - The writer writes `standby-snapshot.json` only into screen-saver container directories that already exist. The snapshot holds `{version: 1, written_at_ms, valid_until_ms = now + 3 min, live}`.
  - Each write goes to a temp file, then fsync and rename; the file is readable by the user only (`crates/hotsheet-ticketing/src/screensaver.rs:88-149`).
  - A per-directory failure is logged (`crates/hotsheet-server/src/standby_saver.rs:39-42`).
- Types and entry points: `standby_saver::write` and `write_to` (`crates/hotsheet-server/src/standby_saver.rs:19`, `crates/hotsheet-server/src/standby_saver.rs:27`); `SnapshotWatcher` and `HotSheetSaverView` (`clients/macos-screensaver/Sources/HotSheetSaverView.swift:37`, `clients/macos-screensaver/Sources/HotSheetSaverView.swift:84`).
- Concurrency:
  - The writer has no timer of its own. It runs on the notification router's blocking worker for each `live_state` event and on the 60 s tick (`crates/hotsheet-server/src/notification_router.rs:160-173`).
  - The saver uses a GCD `DispatchSource` on the directory, because the writer renames the file.
- Errors and recovery: a crashed server leaves the file behind. Its `valid_until_ms` lapses after 3 minutes. How the bundled page shows that is unresolved.
- Configuration and environment: `HOTSHEET_SCREENSAVER_HOME`. The opt-in is `hotsheet-cli screensaver install`, which creates the directory.
- Invariants: the snapshot carries no credential field, but it does carry store names and absolute root paths (`crates/hotsheet-server/src/live_state.rs:106-126`).
- Tests: `crates/hotsheet-server/tests/standby_saver.rs` (`hs2_mzm988_the_snapshot_mirrors_the_live_state_clients_read_and_pushes_nothing`) and `crates/hotsheet-ticketing/src/screensaver.rs` (`the_snapshot_is_readable_by_the_user_only`). The Swift side has a harness only (`clients/macos-screensaver/Harness/main.swift`).
