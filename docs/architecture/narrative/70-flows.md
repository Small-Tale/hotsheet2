# End-to-end flows, failure and recovery

Status: written by HS2-M81CCP. Audited source
`8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`. Follows the depth standard and
evidence format in the architecture README.

## Overview

Each area chapter stops at its own boundary. This chapter follows ten paths
across the browser, the Node bridge, the Rust server, the linked libraries
and storage (or the CLI, the MCP shim and storage), so that no cross-layer
behavior is surprising. It reuses the area chapters' verified facts and adds
the hops between them; each hop cites the code and names a test or says
`untested`.

Every flow has a dynamic view keyed `HS2-Flow-` in `views/flows.dsl`, plus a
failure or recovery view where one view would be too dense. Views that span
browser, bridge and server are scoped to the `server` or `bridge` container,
so the other processes appear as containers; component detail on the other
side is in the linked area view. Cross-machine flows show the peer server as
the same `server` container; `HS2-RemoteDeployment` shows the two hosts.

| Flow                                                                    | Views                                                            | Area views it stitches                                                                                         |
| ----------------------------------------------------------------------- | ---------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| [Ticket edit](#1-ticket-edit)                                           | `HS2-Flow-TicketEdit`, `HS2-Flow-TicketEditFailure`              | `HS2-WebClient-CoalescedEdit`, `HS2-WebClient-OtherTabChange`, `HS2-WebHost-ProxiedCheckoutRequest`            |
| [Out-of-band edit](#2-out-of-band-edit)                                 | `HS2-Flow-OutOfBandEdit`, `HS2-Flow-ReplayOverflow`              | `HS2-Core-GitSyncConflict`, `HS2-Server-ChangeReplay`, `HS2-WebClient-ReplayOverflow`                          |
| [Native provider write](#3-native-provider-write)                       | `HS2-Flow-NativeProviderWrite`, `HS2-Flow-NativeProviderFailure` | `HS2-Server-JiraWriteBehind`, `HS2-Core-OutboxRetry`                                                           |
| [Claim lifecycle](#4-claim-lifecycle)                                   | `HS2-Flow-ClaimLifecycle`, `HS2-Flow-ClaimContention`            | `HS2-Orch-LeaseExpiry`, `HS2-Cli-McpServerWrite`                                                               |
| [AI session](#5-ai-session)                                             | `HS2-Flow-AiSession`, `HS2-Flow-AiPermission`                    | `HS2-UnixTerminal`, `HS2-Server-PermissionRoundTrip`, `HS2-Cli-PermissionHook`, `HS2-WebClient-TerminalAttach` |
| [Scheduled command](#6-scheduled-command-to-placement-and-ci-gate)      | `HS2-Flow-ScheduledCommand`, `HS2-Flow-PlacementToGate`          | `HS2-ScheduledCommand`, `HS2-OrchestrationPlacement`, `HS2-Orch-RefusedPlacement`, `HS2-Orch-FailedCiGate`     |
| [Desktop cold start and shutdown](#7-desktop-cold-start-and-shutdown)   | `HS2-Flow-DesktopColdStart`, `HS2-Flow-DesktopShutdown`          | `HS2-WebHost-DesktopColdStart`, `HS2-WebHost-BridgeExit`, `HS2-Server-BrokerReattach`                          |
| [Remote device and remote project](#8-remote-device-and-remote-project) | `HS2-Flow-RemoteDevice`, `HS2-Flow-RemoteProject`                | `HS2-Server-DevicePairing`, `HS2-WebHost-RemoteProjectAccess`                                                  |
| [Fresh install](#9-fresh-install-and-headless-parity)                   | `HS2-Flow-FreshInstallGui`, `HS2-Flow-FreshInstallHeadless`      | `HS2-Cli-Bootstrap`                                                                                            |
| [Migration](#10-hot-sheet-1-migration)                                  | `HS2-Flow-Migration`                                             | `HS2-LegacyImport`, `HS2-Cli-Migrate`                                                                          |

The eight baseline flow views (`HS2-WebGitTicketWrite`,
`HS2-GitReconciliation`, `HS2-NativeIssueWrite`, `HS2-HeadlessTicketWrite`,
`HS2-UnixTerminal`, `HS2-ScheduledCommand`, `HS2-OrchestrationPlacement`,
`HS2-LegacyImport`) keep their keys as short summaries. `HS2-NativeIssueWrite`
was corrected here: it claimed the server publishes an invalidation after a
GitHub write, which it does not (Discrepancy 2).

## Components

This chapter adds no components. It adds 16 cross-area relationships under
`// HS2-M81CCP: end-to-end flows` in `model/relationships-cross.dsl`, because
every dynamic step needs an explicit edge (`!impliedRelationships false`).
The ones that record facts the area models lacked:

- `flowSyncWritesStore`: the sync loop writes integrated commits into the
  store, and clients learn of them only through the watcher
  (`crates/hotsheet-server/src/sync_loop.rs:146`). Test: untested end to end.
- `flowEventsWakeOrch`: orchestration target followers treat `created`,
  `updated`, `claimed` and `released` bus events as ticket changes
  (`crates/hotsheet-server/src/routes/orchestration.rs:127-140`). Test:
  `crates/hotsheet-cli/src/orchestration/tests/watch.rs`
  (`hs2_bqn0m5_a_new_target_ticket_replans_the_default_goal_once`).
- `flowOrchStartsMonitor` and `flowCliObservesCi`: CI is observed by a
  detached `hotsheet-cli job monitor` process, not by the server
  (`crates/hotsheet-cli/src/orchestration/job_monitor.rs:5-6`,
  `crates/hotsheet-cli/src/dispatch/jobs.rs:551-575`). Test:
  `crates/hotsheet-cli/tests/job_monitor.rs`
  (`hs2_vvhvf0_a_link_starts_the_monitor_once_and_only_for_a_gate_that_waits`).
- `flowTerminalReleasesClaims`: a session's end releases its worker's claims
  in every hosted store
  (`crates/hotsheet-server/src/routes/terminals/launch.rs:965-974`). Test:
  `crates/hotsheet-server/tests/http.rs`
  (`a_shell_terminal_session_releases_its_claims_when_the_terminal_ends`).
- `flowBridgeStartsServer`, `flowDesktopStopsBridge`, `flowDesktopInstallsRelay`,
  `flowDesktopHostsWeb` and `flowWebAcksDesktop`: container-level forms of
  web-host component edges, for the system-scoped desktop views.

The shared `ciServices` system now names all five additional CI services
(`crates/hotsheet-jobs/src/lib.rs:50-71`).

## Durable state and file formats

No new formats. The table lists what each flow leaves on disk when it is
interrupted, which is what survives.

| Flow                  | Survives an interruption                                                             | Lost                                                                             |
| --------------------- | ------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------- |
| Ticket edit           | the browser recovery copy (localStorage or desktop storage); a written ticket file   | the last 150 ms of typing; a write whose completion the bridge could not confirm |
| Out-of-band edit      | the file; the index is rebuilt by reconcile                                          | the replay ring on a server restart                                              |
| Native provider write | the native issue; queued Jira operations in `outbox/<store id>.sqlite`               | nothing is queued on the synchronous path                                        |
| Claim                 | frontmatter lease and `claim_history`; `refs/hotsheet/claims/<ULID>` on bound stores | nothing; expiry is computed on read                                              |
| AI session            | broker PTYs, their replay rings, remembered permission rules                         | parked permission asks (deny) and chat drives on a server restart                |
| Scheduled command     | the occurrence ticket and the schedule cursor with its delivery outcome              | an in-flight turn becomes `DeliveryUncertain` and is never resent                |
| Desktop               | server, broker, terminals, claims, desktop storage drafts                            | the Node bridge's sessions and pending pairings                                  |
| Remote project        | `remotes.json` bindings and keychain credentials                                     | open remote sockets on revoke                                                    |
| Migration             | job checkpoint, per-ticket import checkpoints, receipt                               | nothing; a dead owner reads back as `interrupted`                                |

## Concurrency, tasks and locks

The flows add no task or lock. The cross-layer orderings that matter:

- **Write versus watcher.** A server write upserts the index in the request
  and records the file hash; the watcher then skips that echo
  (`crates/hotsheet-server/src/events.rs:250-267`,
  `crates/hotsheet-server/src/watcher.rs:1400-1406`). A failed upsert leaves
  the echo free to re-index. Test: `crates/hotsheet-server/src/events.rs`
  (`failed_index_writes_are_repaired_on_the_next_store_write`); echo
  suppression end to end is untested.
- **Refresh barrier.** A browser mutation holds the refresh barrier, so its
  own change event cannot overwrite the optimistic row
  (`clients/web/src/refresh-barrier.ts:6-27`). Test:
  `clients/web/src/refresh-barrier.test.ts`.
- **One store transaction.** CLI, serverless MCP and server writes all take
  `<store>/.hotsheet-note-mutation.lock` through `with_ticket_transaction`
  (`crates/hotsheet-ticketing/src/store.rs:570-607`), so the three surfaces
  serialize per store. Test: `crates/hotsheet-ticketing/src/store.rs` (tests module).

## Failure and recovery

Each flow section ends with its failure path. Summary:

| Failure                       | Recovery                                                          | Section |
| ----------------------------- | ----------------------------------------------------------------- | ------- |
| Server dies under a save      | the bridge relaunches it; a non-idempotent request answers 503    | 1       |
| Stale token                   | 409 `ticket_changed`; the browser rebases up to 4 times           | 1       |
| Stream lag or ring overflow   | long-poll replay; overflow forces a full re-read                  | 2       |
| Server restart (cursor reset) | none: events are skipped silently                                 | 2       |
| Provider rate limit           | synchronous: rollback and a message; write-behind: deferred retry | 3       |
| Claim lease lapses            | lazy expiry; late renew refused; takeover                         | 4       |
| Hook cannot reach the server  | the tool's own prompt, or deny under headless Claude auto         | 5       |
| Scheduled handoff fails       | claim released, note written, same ticket retried                 | 6       |
| Page flush fails on close     | the app stays open                                                | 7       |
| Remote binding revoked        | 503 and sockets closed                                            | 8       |
| Migration owner dies          | the job reads `interrupted`; retry resumes from checkpoints       | 10      |

## Configuration and environment

Variables that cross a process boundary in these flows:

| Variable                                                                         | Set by                                                                                                                                                                      | Read by                                    |
| -------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| `HOTSHEET_SERVER`, `HOTSHEET_SECRET`, `HOTSHEET_PROJECT`, `HOTSHEET_TERMINAL_ID` | the server's terminal launch (`crates/hotsheet-server/src/routes/terminals/launch.rs:644-658`)                                                                              | `hotsheet-mcp` attach, `permission-hook`   |
| `HOTSHEET_WORKER_ID`, `HOTSHEET_ACTOR_ROLE`, `HOTSHEET_ACTOR_ID`                 | the server's terminal launch (`crates/hotsheet-server/src/routes/terminals/launch.rs:332-338`) and `hotsheet-cli launch` (`crates/hotsheet-cli/src/dispatch/setup.rs:1032`) | claims, notes, claim-conflict push         |
| `HOTSHEET_BROWSER_AUTH_MODE`                                                     | the operator                                                                                                                                                                | the bridge (remote device flow)            |
| `HOTSHEET_JIRA_WRITE_BEHIND`                                                     | the operator, at server start                                                                                                                                               | the server (native provider flow)          |
| `HOTSHEET_JOB_MONITOR_AUTOSTART`                                                 | the operator; tests force `off`                                                                                                                                             | the orchestration runtime (scheduled flow) |

`HOTSHEET_SERVER` is set only when the server knows its own URL
(`crates/hotsheet-server/src/routes/terminals/launch.rs:657-658`). Without
it the hook and the MCP shim fall back to instance discovery. Test: untested
(no server test asserts `HOTSHEET_SERVER`).

## Dynamic views

### 1. Ticket edit

Views: `HS2-Flow-TicketEdit`, `HS2-Flow-TicketEditFailure`.

1. **Typing.** Keystrokes update a controlled draft; 150 ms after the last
   one a recovery copy is written, with no server request
   (`clients/web/src/debounced-autosave.ts:132-138`). Test:
   `clients/web/src/debounced-autosave.test.ts`.
2. **Coalesced save.** Blur, page hide or finish flushes the field;
   `applyTicketPatch` projects the row, takes the refresh barrier, records the
   write as this tab's own and sends it through the per-ticket sequencer
   (`clients/web/src/features/ticket-workflows.ts:604-724`). Test:
   `clients/web/tests/ticket-edit-sequencing.spec.ts`.
3. **Bridge.** `/__hotsheet/project-api/<id>/*` strips the prefix
   (`clients/web/src/dev-server.ts:548-561`); the proxy adds the instance
   secret in local mode or forwards the browser session in remote mode
   (`clients/web/src/project-bridge.ts:2349-2360`). Test:
   `clients/web/src/project-bridge.test.ts`.
4. **Route.** `PATCH /checkouts/{reference}/tickets/{id}`
   (`crates/hotsheet-server/src/lib.rs:2073-2074`) is admitted by
   `require_secret`, which also counts an active mutation
   (`crates/hotsheet-server/src/routes/auth.rs:180-189`). Test:
   `crates/hotsheet-server/tests/http.rs`
   (`checkout_scoped_ticket_routes_aggregate_and_resolve_linked_stores`).
5. **Source selection.** `update_checkout_ticket` resolves the checkout and
   the owning source; a non-Git source goes to the provider path and a Git
   one to its hosted store
   (`crates/hotsheet-server/src/routes/checkout_tickets/mod.rs:793-835`).
   Test: same as hop 4.
6. **Token check.** The expected token is compared with the indexed ticket's
   `updated_at`; a mismatch is 409 `ticket_changed`
   (`crates/hotsheet-server/src/routes/ticket_writes.rs:146-154`,
   `crates/hotsheet-server/src/api_error.rs:58`). Test:
   `crates/hotsheet-server/tests/http.rs`
   (`expected_token_conflicts_carry_the_ticket_changed_code_on_every_write_route`).
7. **Git write.** `ops::update` revises and writes the file inside the ticket
   transaction (`crates/hotsheet-server/src/routes/ticket_writes.rs:276`,
   `crates/hotsheet-ticketing/src/ops.rs:810-923`). Test: hop 4's test.
8. **Index and event.** `changed_in` upserts the index row in the request,
   records the hash, emits `updated` into the 512-event ring and then the
   bus, and kicks sync (`crates/hotsheet-server/src/events.rs:250-290`,
   `crates/hotsheet-server/src/events.rs:639-646`). Test:
   `crates/hotsheet-server/src/events.rs`
   (`failed_index_writes_are_repaired_on_the_next_store_write`); no test
   asserts that a checkout PATCH emits `updated`.
9. **Commit.** The background committer commits after the transaction
   (`crates/hotsheet-ticketing/src/store.rs:2175-2176`). Test:
   `crates/hotsheet-ticketing/src/store.rs`
   (`background_autocommit_commits_after_flush`).
10. **Other tab.** The bridge forwards the event; tab B waits on its barrier,
    skips its own writes, coalesces for 180 ms and re-reads the ticket; an
    active draft merges or shows a conflict
    (`clients/web/src/project-change-poll.ts:213-237`,
    `clients/web/src/ticket-field-reconciliation.ts:45-52`). Test:
    `clients/web/src/ticket-field-reconciliation.test.ts`.

**Failure and recovery.**

- A stale token rebases and retries up to 4 times
  (`clients/web/src/features/ticket-workflows.ts:487`). Test:
  `clients/web/tests/ticket-edit-sequencing.spec.ts`.
- The server dies: the proxy forgets the instance, re-supervises (launching a
  replacement), retries a GET or HEAD once and answers a PATCH with 503
  "completion is unknown" (`clients/web/src/project-bridge.ts:2386-2404`).
  The browser rolls back the row and keeps the draft
  (`clients/web/src/features/ticket-workflows.ts:794-808`). Test: supervision
  by `clients/web/src/project-bridge.test.ts`; the 503 and the rollback are
  untested.
- **What survives:** the recovery copy, restored or discarded on reload
  (`clients/web/src/features/ticket-workflows.ts:917-937`), and any file the
  server wrote before it died. The browser cannot tell which; the next
  re-read decides (FLOW-U4).

### 2. Out-of-band edit

Views: `HS2-Flow-OutOfBandEdit`, `HS2-Flow-ReplayOverflow`.

1. **Edit.** A `git pull`, an editor, a direct CLI write or the server's own
   sync pass changes ticket files. The sync loop runs `sync_once` and
   announces only sync health and repository size, never a ticket event
   (`crates/hotsheet-server/src/sync_loop.rs:146-177`). Test: untested end
   to end.
2. **Watch.** The native watcher drops access events and forwards the rest
   (`crates/hotsheet-server/src/watcher.rs:330-352`); the loop debounces
   150 ms (`crates/hotsheet-server/src/watcher.rs:1113`). Test:
   `crates/hotsheet-server/src/watcher.rs`
   (`a_running_watcher_goes_live_and_still_sees_external_writes`).
3. **Re-index.** Each changed path goes through `handle_path_change`, which
   skips this server's own echo, upserts the row and emits `changed`, or
   `deleted` for a removed file (`crates/hotsheet-server/src/watcher.rs:1164`,
   `crates/hotsheet-server/src/watcher.rs:1392-1432`). The full
   `reconcile_with_invalidations` runs only when the watcher is re-armed
   (`crates/hotsheet-server/src/watcher.rs:1131-1138`). Test:
   `crates/hotsheet-server/tests/http.rs` (`watcher_reindexes_an_external_write`),
   `crates/hotsheet-server/src/watcher.rs`
   (`hs2_r6fz54_rearm_resync_reconciles_removed_and_new_tickets`).
4. **Deliver.** As in flow 1 hop 10. The watcher's kind is `changed`, not
   `updated`; the client treats both as ticket changes
   (`clients/web/src/project-change-poll.ts:8-14`). Test:
   `clients/web/src/project-change-poll.test.ts`.

**Replay and overflow.** A lagged WebSocket closes without a close frame
(`crates/hotsheet-server/src/routes/store_scoped.rs:426`); the client
long-polls from its last cursor; an `overflow: true` answer, or a failed
replay, triggers a full authoritative re-read
(`crates/hotsheet-server/src/routes/store_scoped.rs:506-521`,
`clients/web/src/project-change-poll.ts:467-492`). Test:
`crates/hotsheet-server/src/events.rs` (`falling_behind_the_ring_signals_overflow`),
`clients/web/src/project-change-poll.test.ts`. The lag close is untested.

**Server restart.** The ring restarts at cursor 0. A poll whose `since` is
above the new `seq` is treated as caught up and returns no events with the
new, lower cursor (`crates/hotsheet-server/src/events.rs:663`). The client
adopts it without comparing (`clients/web/src/project-change-poll.ts:273`,
`clients/web/src/project-change-poll.ts:482`), and its WebSocket path
discards events at or below its old cursor
(`clients/web/src/project-change-poll.ts:401`). Events the new server emits
in that range are skipped until a request fails and a fresh handshake
resyncs (`clients/web/src/project-change-poll.ts:263-264`). This answers
S-U6; see Discrepancy 3. Test: untested.

**What survives:** the files and Git history; the index is reconciled at the
next start (view `HS2-Core-IndexRebuild`).

### 3. Native provider write

Views: `HS2-Flow-NativeProviderWrite`, `HS2-Flow-NativeProviderFailure`.

1. **Choice of path.** For a source whose capabilities include
   `write_behind`, with edits limited to title, details, category, priority
   and tags, the browser posts to `/providers/{c}/tickets/queued`
   (`clients/web/src/features/ticket-workflows.ts:650-693`,
   `clients/web/src/api.ts:2215-2219`); otherwise it PATCHes the checkout
   route as in flow 1. Test: `clients/web/src/api.test.ts`.
2. **Synchronous write.** The checkout handler sends a non-Git source to
   `do_provider_update`, which builds or reuses the adapter with the managed
   token and calls `update_timed` with the expected token
   (`crates/hotsheet-server/src/routes/checkout_tickets/mod.rs:801-824`,
   `crates/hotsheet-server/src/routes/providers/tickets.rs:286-458`,
   `crates/hotsheet-server/src/routes/providers/mod.rs:1020-1086`). Test:
   `crates/hotsheet-server/tests/provider_feedback_http.rs`
   (`hs2_h9z9kz_provider_note_routes_best_effort_without_a_persisted_record`).
3. **No event.** `reindex_hosted_provider_write` returns early for anything
   but a hosted Git connection, so the write publishes nothing
   (`crates/hotsheet-server/src/routes/providers/tickets.rs:535`,
   `crates/hotsheet-server/src/routes/providers/tickets.rs:565-586`). The
   saving tab updates from the response. Test: untested for external sources.
4. **Webhook.** Only GitHub has a receiver. A signed delivery re-reads the
   issue and emits `changed` (`crates/hotsheet-server/src/lib.rs:2941-2942`,
   `crates/hotsheet-server/src/github_webhooks.rs:591-595`). GitLab and Jira
   writes reach other clients only on their next read. Test:
   `crates/hotsheet-server/tests/http.rs`
   (`hs2_sgq183_webhook_setup_and_delivery_are_authenticated_and_source_bound`).
5. **Write-behind.** Narrated in `HS2-Server-JiraWriteBehind`: admission
   answers 202 and emits `updated`; the dispatcher reads back, writes with
   the concurrency token, and settles or asks for attention
   (`crates/hotsheet-server/src/provider_write_behind.rs:159-298`). If the
   server answers 409 `write_behind_unavailable`, the browser falls back to
   the synchronous PATCH (`clients/web/src/features/ticket-workflows.ts:702-716`).
   Test: `crates/hotsheet-server/tests/jira_outbox_http.rs`
   (`write_behind_unavailable_refusal_carries_a_typed_code_hs2_deveye`).

**Rate limit and failure.**

- GitHub maps 429 or a rate-limit 403 to `RateLimited` and starts a local
  cooldown; GitLab and Jira map 429 without one
  (`crates/hotsheet-extsync/src/github.rs:538`,
  `crates/hotsheet-extsync/src/gitlab.rs:143-154`,
  `crates/hotsheet-extsync/src/jira.rs:166`). Test:
  `crates/hotsheet-extsync/src/github.rs`
  (`live_read_state_coalesces_issue_walks_and_honors_rate_limit_cooldown`).
- On the synchronous path every provider error that is not a stale token
  becomes a plain 409 with the message, no `Retry-After` and no code
  (`crates/hotsheet-server/src/routes/providers/mod.rs:1247-1254`). The
  browser rolls back and shows the text; it never retries
  (`clients/web/src/features/ticket-workflows.ts:729`,
  `clients/web/src/features/ticket-workflows.ts:794-800`). Test: untested
  (Discrepancy 1).
- On the write-behind path, `RateLimited` defers the operation and its
  connection by `Retry-After`, or 30 s
  (`crates/hotsheet-server/src/provider_write_behind.rs:396-400`). Test:
  `crates/hotsheet-server/tests/jira_outbox_http.rs`
  (`timeout_after_success_and_rate_limit_reconcile_without_duplicate_write`).
- **What survives:** the native issue is authoritative. Queued Jira
  operations survive a restart; a `sending` row returns to `queued` on open
  (`crates/hotsheet-ticketing/src/provider_outbox.rs:383-403`). Test:
  `crates/hotsheet-ticketing/src/provider_outbox.rs`
  (`dispatch_claims_one_per_ticket_and_survives_rate_limit_attention_and_restart`).

### 4. Claim lifecycle

Views: `HS2-Flow-ClaimLifecycle`, `HS2-Flow-ClaimContention`.

1. **CLI.** `hotsheet-cli claim` opens the store directly and calls
   `ops::claim_with_eta`; no server is involved
   (`crates/hotsheet-cli/src/dispatch/claims.rs:366`). The commit's file event
   reaches clients through flow 2. Test: `crates/hotsheet-cli/tests/claim_run.rs`
   (`hs2_mfyyj9_expired_renewal_refuses_and_requires_a_new_exact_claim`).
2. **MCP.** `hotsheet_claim` posts `/checkouts/<c>/tickets/<id>/claim`
   (`crates/hotsheet-mcp/src/lib.rs:1220-1225`,
   `crates/hotsheet-mcp/src/lib.rs:1541-1555`); serverless mode calls the same
   `ops` function in process. Test: `crates/hotsheet-mcp/src/lib.rs`
   (`hs2_em6ywp_project_claim_lifecycle_targets_the_ticket_checkout`).
3. **Server.** The claim routes refuse non-Git sources with 409, write through
   `ops` and emit `claimed`, `released` or `renewed`
   (`crates/hotsheet-server/src/routes/claims.rs:181`,
   `crates/hotsheet-server/src/routes/claims.rs:248-255`). Test:
   `crates/hotsheet-server/tests/http.rs` (`claim_next_release_renew_over_http`).
4. **UI.** The web client never claims, renews or releases; it shows the
   holder and lease from `claimed`, `released` and `renewed` events
   (`clients/web/src/project-change-poll.ts:8-14`). Test:
   `clients/web/src/project-change-poll.test.ts`.
5. **Gate.** Inside the transaction `claim_with_eta` runs the git_ref claim
   gate when the store is bound (`crates/hotsheet-ticketing/src/ops.rs:3026-3046`).
   Test: `crates/hotsheet-ticketing/src/claim_gate_tests.rs`
   (`hs2_wjf0d8_concurrent_claims_from_two_clones_have_exactly_one_winner`).
6. **Renewal and expiry.** Renew extends the lease; expiry is lazy; a renew
   after the lease end is refused, and another worker can then take the
   ticket (`HS2-Orch-LeaseExpiry`, `crates/hotsheet-ticketing/src/ops.rs:2679-2685`,
   `crates/hotsheet-ticketing/src/ops.rs:3314-3319`). Test:
   `crates/hotsheet-ticketing/src/ops.rs`
   (`exact_claim_is_eligible_holder_idempotent_and_expiry_takeover_safe`).
7. **Session end.** A server-launched terminal's monitor releases its
   worker's claims when the session ends
   (`crates/hotsheet-server/src/routes/terminals/launch.rs:965-974`);
   `hotsheet-cli launch` does the same after the tool exits
   (`crates/hotsheet-cli/src/dispatch/setup.rs:1059`). Test:
   `crates/hotsheet-server/tests/http.rs`
   (`broker_terminal_sessions_release_claims_including_after_a_server_restart`),
   `crates/hotsheet-cli/tests/cli.rs` (`launch_releases_the_claims_its_session_left_behind`).

**Contention on two machines.**

- **Bound store.** Both gates race on `refs/hotsheet/claims/<ULID>`; the
  loser sees `HeldBy`, rewrites its copy to name the holder and answers
  `WrongWorker` (`crates/hotsheet-ticketing/src/claim_gate.rs:283-314`). An
  offline claim may be provisional; after a pulling sync, `reconcile`
  records a lost claim in `hotsheet-claim-conflicts.json`
  (`crates/hotsheet-ticketing/src/sync.rs:196-198`,
  `crates/hotsheet-ticketing/src/claim_gate.rs:695-732`), and the server's
  sync loop pushes it once to the losing session
  (`crates/hotsheet-server/src/sync_loop.rs:168`,
  `crates/hotsheet-server/src/claim_conflict_push.rs:43-61`). Test:
  `crates/hotsheet-ticketing/src/claim_gate_tests.rs`
  (`hs2_wjf0d8_a_sync_that_pulls_flags_a_claim_lost_to_another_machine`),
  `crates/hotsheet-server/src/claim_conflict_push.rs`
  (`hs2_1639q0_the_losing_session_alone_is_told_once_and_the_others_stay_pending`).
- **Unbound store.** Both claims commit locally. The merge takes the claim
  fields from the newer `updated_at` and unions the history
  (`crates/hotsheet-ticketing/src/merge.rs:126`,
  `crates/hotsheet-ticketing/src/merge.rs:276-301`). `reconcile` returns at
  once without a gate, so nobody is told
  (`crates/hotsheet-ticketing/src/claim_gate.rs:696-697`). The loser learns
  only when its next renew fails. Test: `crates/hotsheet-ticketing/src/merge.rs`
  (`concurrent_claim_events_union_by_id_in_time_order`); the silent loss is
  untested (Discrepancy 5).
- **What survives:** the frontmatter lease and `claim_history` in Git; on a
  bound store, the remote marker until release or lapse.

### 5. AI session

Views: `HS2-Flow-AiSession`, `HS2-Flow-AiPermission`.

1. **Launch.** The terminal drawer posts `/terminals` with the tool, model
   and `cwd` (`clients/web/src/features/terminal-drawer.ts:405-408`,
   `clients/web/src/api.ts:3514`); the server finds the checkout from `cwd`
   (`crates/hotsheet-server/src/lib.rs:2785`,
   `crates/hotsheet-server/src/routes/terminals/launch.rs:23-34`). Test:
   `clients/web/src/features/terminal-drawer.test.ts`.
2. **Setup.** Before the tool starts, the server runs the same idempotent
   setup as `hotsheet-cli setup`, which writes the MCP entry and hooks
   (`crates/hotsheet-server/src/routes/terminals/launch.rs:285-297`). Test:
   `crates/hotsheet-server/tests/codex_terminal_launch.rs`
   (`hs2_4j7wcv_every_hot_sheet_codex_terminal_launches_without_the_daemon`).
3. **Environment.** The launch adds the route-back and identity variables
   listed under Configuration
   (`crates/hotsheet-server/src/routes/terminals/launch.rs:329-338`). Test:
   `crates/hotsheet-server/tests/http.rs`
   (`a_connect_terminal_releases_its_sessions_claims_when_it_exits`).
4. **Broker.** `BrokerRequest::Open` spawns the PTY in the detached broker,
   restarting the broker if needed; without a broker the PTY is in process
   (`crates/hotsheet-server/src/routes/terminals/launch.rs:65-76`,
   `crates/hotsheet-server/src/terminal_broker.rs:151-153`). Test:
   `crates/hotsheet-terminals/src/broker.rs` (`broker_hosts_a_pty_reachable_over_the_socket`).
5. **MCP attach.** The manifest starts `hotsheet-mcp --project {project}`
   (`plugins/claude/manifest.toml:45-46`). With `HOTSHEET_SERVER` and
   `HOTSHEET_SECRET` inherited, it attaches to the launching server or a
   running machine host, and never starts its own
   (`crates/hotsheet-mcp/src/main.rs:58-75`, `crates/hotsheet-mcp/src/main.rs:255-266`).
   Test: `crates/hotsheet-mcp/src/main.rs`
   (`hs2_jfe7jj_the_launched_route_comes_from_the_terminal_unless_a_shared_host_runs_it`);
   `attach_route` itself is untested.
6. **Permission.** The hook runs `hotsheet-cli permission-hook`, which posts
   `/permissions/ask` and blocks (`crates/hotsheet-cli/src/dispatch/setup.rs:708`,
   `crates/hotsheet-cli/src/dispatch/setup.rs:2199`). The server parks it and
   emits `permission_asked` (`crates/hotsheet-server/src/routes/permissions.rs:600`,
   `crates/hotsheet-server/src/lib.rs:587-592`). The workspace refetches
   `GET /permissions`, shows the prompt and posts the decision
   (`clients/web/src/features/project-change-streams.ts:534-535`,
   `clients/web/src/permission-decision-workflow.ts:63`). `permission_resolved`
   follows and the hook prints the decision. Test:
   `crates/hotsheet-server/tests/http.rs`
   (`permissions_ask_blocks_then_returns_the_human_answer`),
   `clients/web/src/permission-decision-workflow.test.ts`.
7. **Completion.** The broker monitor sees the terminal gone, releases the
   session's claims and emits `terminal_exited`; the drawer refreshes its
   dashboard (`crates/hotsheet-server/src/routes/terminals/launch.rs:913-945`,
   `clients/web/src/features/project-change-streams.ts:577-588`). Test:
   `crates/hotsheet-server/tests/http.rs`
   (`a_connect_terminal_releases_its_sessions_claims_when_it_exits`); no test
   asserts `terminal_exited`.

**Failure and recovery.**

- The hook cannot reach the server: the tool's own prompt decides, or deny
  under headless Claude auto (`crates/hotsheet-cli/src/dispatch/setup.rs:779-813`).
  Test: `crates/hotsheet-cli/tests/cli.rs`
  (`headless_claude_auto_denies_an_unanswered_native_permission_request`).
- Unanswered asks deny after 24 h; a stopping server answers 503
  (`HS2-Server-PermissionRoundTrip`).
- The broker monitor also releases claims when the broker is merely
  unreachable (`crates/hotsheet-server/src/routes/terminals/launch.rs:913`).
  Test: untested (Discrepancy 6).
- **What survives:** a server restart keeps broker PTYs, and the next start
  resumes their monitors (`HS2-Server-BrokerReattach`). Parked asks and chat
  drives do not survive.

### 6. Scheduled command to placement and CI gate

Views: `HS2-Flow-ScheduledCommand`, `HS2-Flow-PlacementToGate`.

Scheduled commands and orchestration are separate subsystems. No
orchestration code refers to schedules, and the schedule runner never calls
orchestration. The only link is a ticket event: the scheduled ticket's
`created` event can wake an orchestration follower, whose automatic re-plan
may add a step for it.

1. **Due occurrence.** The runner wakes at the next due time, at most 60 s
   later, or on a schedule write
   (`crates/hotsheet-server/src/routes/command_schedules.rs:1515-1573`).
   Missed sub-day slots collapse into one run
   (`crates/hotsheet-server/src/routes/command_schedules.rs:763`). Test:
   `crates/hotsheet-server/src/routes/command_schedules.rs`
   (`hs2_29v4zg_missed_slots_coalesce_and_fold_does_not_replay`).
2. **Duplicate guard.** An occurrence reuses its own ticket, and is skipped
   while the command has any open ticket
   (`crates/hotsheet-server/src/routes/command_schedules.rs:700-726`). Test:
   `hs2_29v4zg_open_ticket_skips_once_then_next_slot_runs_after_completion`
   in the same file.
3. **Ticket.** One Up Next task at highest priority, tagged `hs2-command:<id>`
   and the occurrence tag
   (`crates/hotsheet-server/src/routes/command_schedules.rs:660-694`). Test:
   `hs2_g5j2e7_restart_reuses_occurrence_ticket_and_preserves_history`.
4. **Claim.** The exact ticket is claimed for the drive's session worker with
   a 30 min ETA; a refused claim waits for the next pass
   (`crates/hotsheet-server/src/routes/command_schedules.rs:1084-1105`). Test:
   `hs2_mb3bqe_new_session_claims_exact_ticket_before_handoff`.
5. **Handoff.** `DeliveryDispatching` is recorded first, then
   `send_scheduled_drive_turn` starts a headless chat drive turn; schedules
   never use a terminal
   (`crates/hotsheet-server/src/routes/command_schedules.rs:1112-1134`,
   `crates/hotsheet-server/src/routes/permissions.rs:1259`). Test: same as hop 4.
6. **Orchestration (optional).** The `created` event matches the target
   follower's ticket-change filter
   (`crates/hotsheet-server/src/routes/orchestration.rs:127-140`). Automatic
   re-planning runs only when placement is on and the standing default goal is
   the only open goal (`crates/hotsheet-cli/src/orchestration/auto_replan.rs:1`,
   `crates/hotsheet-cli/src/orchestration/planning.rs:279-283`). A planned
   step is then placed as in `HS2-OrchestrationPlacement`; a remote target is
   `HS2-Orch-RefusedPlacement`. Test:
   `crates/hotsheet-cli/src/orchestration/tests/watch.rs`
   (`hs2_bqn0m5_a_new_target_ticket_replans_the_default_goal_once`); no test
   starts from a scheduled command.
7. **CI gate.** `gate link` records a CI gate for a commit and ensures an
   idempotent watch Job and the detached monitor
   (`crates/hotsheet-cli/src/orchestration/gates.rs:677-764`,
   `crates/hotsheet-jobs/src/ci.rs:148-157`). The monitor process observes the
   provider; the server's gate follower reads the finished Job and applies
   the reading (`HS2-Orch-FailedCiGate`). Test:
   `crates/hotsheet-cli/src/orchestration/tests/gates.rs`
   (`hs2_5jbypv_a_linked_ci_gate_follows_typed_readings_and_publishes_gate_events`).

**Failure and recovery.**

- A failed handoff releases the claim, writes one note and retries the same
  ticket on the next pass
  (`crates/hotsheet-server/src/routes/command_schedules.rs:1151-1155`,
  `crates/hotsheet-server/src/routes/command_schedules.rs:1445-1446`). Test:
  `hs2_mb3bqe_failed_delivery_retries_same_ticket_after_host_restart`.
- A restart during a turn marks it `DeliveryUncertain` and never resends
  (`crates/hotsheet-server/src/routes/command_schedules.rs:1257`). Test:
  `hs2_nr0vrf_restart_preserves_uncertain_exact_turn_without_replay`.
- A red gate blocks dependents and ranks the repair first
  (`HS2-Orch-FailedCiGate`); a refused remote start backs off from 60 s
  (`HS2-Orch-RefusedPlacement`).
- **What survives:** the ticket, the schedule cursor and its outcome; run,
  gate and session records under the machine home.

### 7. Desktop cold start and shutdown

Views: `HS2-Flow-DesktopColdStart`, `HS2-Flow-DesktopShutdown`.

1. **Relay.** On macOS the shell runs `hotsheet-cli terminal-relay install`
   before starting Node (`clients/web/src-tauri/src/lib.rs:244-247`). Test:
   `crates/hotsheet-cli/tests/terminal_relay.rs`
   (`hs2_hyf56y_headless_install_status_and_uninstall_drive_a_real_relay`);
   the Tauri wrapper is untested.
2. **Bridge.** The shell spawns Node and waits up to 30 s for the ready file;
   the host snapshots binaries, writes the stable hook CLI, publishes
   `client.json` and writes the ready file
   (`clients/web/src-tauri/src/lib.rs:141-197`,
   `clients/web/src/local-host.ts:271-292`,
   `clients/web/src/local-host.ts:426-431`). Test:
   `clients/web/scripts/desktop-host-smoke.mjs`.
3. **Page.** The WebView opens on the bridge origin
   (`clients/web/src-tauri/src/lib.rs:249-310`), and the page hydrates
   desktop storage before mounting. Test: `clients/web/src/desktop-storage.test.ts`.
4. **Lazy server.** No server starts until a request needs one. Opening a
   project runs `setup --refresh`, then launches `hotsheet-server -C <store>
--bind 127.0.0.1:0` detached with stdin ignored, so the server does not
   follow the bridge's life (`clients/web/src/project-bridge.ts:1763`,
   `clients/web/src/project-bridge.ts:1464-1483`). Test:
   `clients/web/src/project-bridge.test.ts`.
5. **Server.** The server takes its writer lock, ensures the broker, resumes
   surviving terminals, then publishes instance files
   (`crates/hotsheet-server/src/main.rs:314`,
   `crates/hotsheet-server/src/main.rs:435`,
   `crates/hotsheet-server/src/main.rs:603`). Test:
   `crates/hotsheet-server/tests/broker_startup.rs`
   (`long_home_starts_real_server_and_detached_broker_and_preserves_terminals_on_restart`).

**Shutdown.**

1. A close request evaluates the page's close hook; the page flushes drafts,
   captures recovery and acknowledges with `desktop_flush_complete`
   (`clients/web/src-tauri/src/lib.rs:95-118`,
   `clients/web/src/desktop-close.ts:50-57`). Test:
   `clients/web/tests/desktop-native-close.spec.ts`.
2. No answer within 30 s cancels the close and keeps the app open. Test:
   `clients/web/src-tauri/src/close_lifecycle.rs` (unit tests).
3. On exit the shell closes Node's stdin and kills it after 5 s; the host
   removes `client.json` and its snapshot
   (`clients/web/src-tauri/src/lib.rs:200-214`,
   `clients/web/src/local-host.ts:496-520`). Test:
   `clients/web/scripts/desktop-host-smoke.mjs`.

**What survives:** the server, which was started without
`--exit-on-stdin-eof` (`crates/hotsheet-server/src/main.rs:580-582`), the
broker and its terminals, every claim and lease (they live in ticket files),
and desktop storage drafts. MCP presence leases end only when their tool
exits. Lost: the bridge's in-memory sessions and pending pairings. A bridge
that crashes is not restarted (`HS2-WebHost-BridgeExit`).

### 8. Remote device and remote project

Views: `HS2-Flow-RemoteDevice`, `HS2-Flow-RemoteProject`. Two different
pairings exist.

**A remote browser device** pairs with this host, whose bridge must run with
`HOTSHEET_BROWSER_AUTH_MODE=remote`:

1. The device posts a label; the bridge forwards it; the server rate-limits,
   stores the challenge digest for 15 minutes and emits an identity-free
   `pairing_requests` event to device managers
   (`clients/web/src/components/remote-auth-screen.tsx:214`,
   `crates/hotsheet-server/src/remote_auth.rs:620-660`,
   `crates/hotsheet-server/src/routes/remote_auth.rs:119-158`). Test:
   `crates/hotsheet-server/tests/remote_auth.rs`
   (`hs2_bzhmev_pairing_request_events_reach_only_device_managers_over_poll`).
2. The owner approves with a role and grants; the device redeems and gets
   HttpOnly session and CSRF cookies
   (`crates/hotsheet-server/src/routes/remote_auth.rs:568-589`,
   `crates/hotsheet-server/src/remote_auth.rs:736-795`). Test:
   `hs2_7dbjms_pair_approve_redeem_role_replay_revoke` in the same file.
3. The device opens a registered project
   (`clients/web/src/dev-server.ts:268-282`) and edits as in flow 1; the
   bridge forwards the cookie and CSRF and strips any secret
   (`clients/web/src/project-bridge.ts:2349-2357`). Test:
   `clients/web/src/remote-auth-server-integration.test.ts`.
4. **Failure:** revocation closes WebSocket, attach and poll streams within
   about 1 s (`crates/hotsheet-server/src/routes/store_scoped.rs:160-209`).
   Test: same as step 3.

**A remote project** is a checkout on a paired peer server:

1. This host's bridge pairs with the peer's `/remote-auth/pair`, redeems
   after the peer's owner approves, then runs `hotsheet-cli remote register`
   and `review` (`clients/web/src/dev-server.ts:300-324`). Test:
   `clients/web/src/project-bridge.remote-connections.test.ts`.
2. Opening reads the private connection, rejects a binding changed mid-open
   and reads the peer's `/compatibility`
   (`clients/web/src/project-bridge.ts:1769-1815`). Test:
   `clients/web/src/remote-project-parity.test.ts`.
3. Each forward re-reads `remotes.json` and sends the private session, CSRF
   and client certificate (`clients/web/src/project-bridge.ts:2260-2347`).
   Test: same as step 2.
4. **Failure:** a revoked or changed binding answers 503 and closes the
   tab's sockets (`clients/web/src/project-bridge.ts:161-166`). Test:
   `clients/web/src/project-bridge.remote-connections.test.ts`.

**What survives:** device sessions in `remote-auth/machine.json` and
bindings in `remotes.json`. Pending pairings on the bridge side are memory
only.

### 9. Fresh install and headless parity

Views: `HS2-Flow-FreshInstallGui`, `HS2-Flow-FreshInstallHeadless`.

The GUI's "Create a Hot Sheet 2 git ticket repository" runs `hotsheet-cli
bootstrap --project <root> --store <store> --prefix HS2`
(`clients/web/src/project-bridge.ts:911`, `clients/web/src/project-bridge.ts:972`).
Headless, the user runs the same command (`HS2-Cli-Bootstrap`). Step by step:

| #   | GUI step                                                                                                                                                         | Headless equivalent                                                                                                                                                                | Same workflow?                                          | Test                                                                                                         |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| 1   | Native folder chooser (`clients/web/src/dev-server.ts:405-415`)                                                                                                  | the user types the path                                                                                                                                                            | not applicable                                          | untested                                                                                                     |
| 2   | Machine anchor `server-bootstrap.hs2` by `hotsheet-cli init --prefix HS2` (`clients/web/src/project-bridge.ts:907`, `clients/web/src/project-bridge.ts:939-943`) | `hotsheet-cli launch` creates it with `FsStore::init` and prefix `HS` (`crates/hotsheet-cli/src/dispatch/setup.rs:988-990`)                                                        | no: different code and prefix (Discrepancy 7)           | `clients/web/src/project-bridge.test.ts`                                                                     |
| 3   | Store, merge driver, link, first commit, `checkouts.json`, worklist and AI-tool setup through `bootstrap`                                                        | `hotsheet-cli bootstrap` (`crates/hotsheet-cli/src/dispatch/store.rs:191-375`)                                                                                                     | yes: the GUI runs the CLI                               | `clients/web/src/project-bridge.test.ts`; `crates/hotsheet-cli/tests/cli.rs`                                 |
| 4   | "Link existing" repository: a server `PUT /checkouts/{id}/sources/{conn}` only (`clients/web/src/project-bridge.ts:984-989`)                                     | `hotsheet-cli link <store>` (`crates/hotsheet-cli/src/dispatch/store.rs:490`)                                                                                                      | no: the GUI skips `link` (FLOW-U2)                      | `clients/web/src/project-bridge.test.ts`                                                                     |
| 5   | Setup refresh on every open (`clients/web/src/project-bridge.ts:595`)                                                                                            | `hotsheet-cli setup --refresh --project <root>` (`crates/hotsheet-cli/src/dispatch/setup.rs:1255-1256`)                                                                            | yes                                                     | `clients/web/src/project-bridge.test.ts` ("exposes the same setup refresh as a non-graphical CLI operation") |
| 6   | Remote: `git remote add`, save `hotsheet.backupOrigin`, `git push -u origin HEAD` (`clients/web/src/project-bridge.ts:1053-1107`)                                | `bootstrap --remote <url>` adds origin only and prints the push command (`crates/hotsheet-cli/src/dispatch/store.rs:310-336`, `crates/hotsheet-cli/src/dispatch/store.rs:415-423`) | no: the GUI pushes and saves the origin (Discrepancy 8) | `crates/hotsheet-cli/tests/cli.rs` (`bootstrap_prints_exact_remote_instructions_when_origin_is_absent`)      |
| 7   | Server start: the bridge launches `hotsheet-server` (`clients/web/src/project-bridge.ts:1464`)                                                                   | `hotsheet-cli launch` or `serve` (`crates/hotsheet-cli/src/external_launch.rs:194-196`)                                                                                            | no: different supervisors; same server binary           | `crates/hotsheet-cli/src/external_launch.rs` (`hs2_vg3e8n_bootstrap_only_host_serves_a_project_store`)       |
| 8   | Terminal relay on the macOS desktop (`clients/web/src-tauri/src/lib.rs:244-247`)                                                                                 | `hotsheet-cli terminal-relay install`                                                                                                                                              | yes: the shell runs the CLI                             | `crates/hotsheet-cli/tests/terminal_relay.rs`                                                                |
| 9   | Stable hook CLI in `HOTSHEET_HOME/bin` (`clients/web/src/local-host.ts:271-292`)                                                                                 | none; headless setup uses `HOTSHEET_HOOK_CLI_BIN` or the sibling binary                                                                                                            | **no headless equivalent**                              | `clients/web/src/local-host.test.ts`                                                                         |
| 10  | `client.json` for notification deep links (`clients/web/src/client-discovery.ts:25-45`)                                                                          | `hotsheet-cli notify link-base <url>`                                                                                                                                              | partial: a different setting                            | `clients/web/src/client-discovery.test.ts`                                                                   |
| 11  | none                                                                                                                                                             | `hotsheet-cli screensaver install`                                                                                                                                                 | **no GUI equivalent**                                   | `crates/hotsheet-cli/tests/screensaver.rs`                                                                   |

Steps with no headless equivalent: the folder chooser (1, inherent), the
stable hook CLI copy (9), and the GUI's push of the first commit (6; the CLI
prints the command instead). The GUI also discards bootstrap's printed next
steps (`clients/web/src/project-bridge.ts:972`).

**Failure and recovery:** `bootstrap` is idempotent; a re-run repairs a
missing link, registration or setup (`HS2-Cli-Bootstrap`). A GUI remote
failure is explained in guidance prose, and the saved origin is kept for a
retry (`clients/web/src/project-bridge.ts:1017-1051`). Test:
`clients/web/src/project-bridge.test.ts`.

### 10. Hot Sheet 1 migration

View: `HS2-Flow-Migration`.

1. **Detect.** Opening a project checks for `.hotsheet/db/PG_VERSION`
   (`clients/web/src/project-bridge.ts:330`,
   `clients/web/src/project-bridge.ts:1874-1897`). Headless, `hotsheet-cli
doctor` and `init` report it; `bootstrap` does not
   (`crates/hotsheet-cli/src/dispatch/store.rs:1798-1809`). Test:
   `clients/web/tests/hs1-migration.spec.ts`.
2. **Job.** The browser posts `/__hotsheet/projects/migration-jobs`; the job
   requires the project session, takes project and store locks through
   `hotsheet-migrate --hold-job-lock` and writes its checkpoint before launch
   (`clients/web/src/dev-server.ts:490-515`,
   `clients/web/src/migration-jobs.ts:165-168`,
   `clients/web/src/migration-lock.ts:45-59`). Test:
   `clients/web/src/migration-jobs.test.ts`.
3. **Import.** The job runs `hotsheet-migrate <root>/.hotsheet -C <store>
--migrator <exporter> --progress-json`
   (`clients/web/src/project-bridge.ts:622-624`); the binary exports from a
   copy, imports with per-ticket checkpoints, verifies, writes the receipt and
   commits (`HS2-Cli-Migrate`). Test: `crates/hotsheet-cli/tests/migrate.rs`.
4. **After import.** Only the GUI then runs `setup --detect`, registers the
   source and runs `hotsheet-cli link`
   (`clients/web/src/project-bridge.ts:648-659`,
   `clients/web/src/project-bridge.ts:2605-2615`). Test:
   `clients/web/src/migration-server-integration.test.ts`.
5. **Backup.** The browser can connect a backup remote; the bridge prepares,
   pushes and records the proof in TypeScript, while `hotsheet-migrate
--backup-remote` does the same in Rust
   (`clients/web/src/hs1-backup.ts:121-148`,
   `crates/hotsheet-cli/src/import_completion.rs:151`). Test:
   `clients/web/src/hs1-backup.test.ts`, `crates/hotsheet-cli/tests/migrate.rs`
   (`hs2_8sewhj_headless_backup_remote_is_saved_pushed_and_verified`).
6. **Progress.** The browser long-polls the job
   (`clients/web/src/migration-jobs.ts:356`). Test:
   `clients/web/tests/hs1-migration.spec.ts`.

**Failure and recovery.** A `running` job whose owner is dead reads back as
`interrupted` (`clients/web/src/migration-jobs.ts:79-85`); a retry is refused
while the old owner or child lives, and a new attempt reruns the binary,
which resumes from `hotsheet-hs1-import-pending`
(`crates/hotsheet-cli/src/import.rs:149-190`). Test:
`clients/web/src/migration-jobs.test.ts`. A headless run takes no job lock,
so it can overlap a GUI job on the same store (Discrepancy 9).

**What survives:** the job record, per-ticket checkpoints, the receipt and
the completion proof.

## Discrepancies

Each discrepancy names the follow-up ticket filed from HS2-M81CCP.

1. **Provider rate limits reach the browser as a plain 409.** Every
   non-token provider error becomes 409 without `Retry-After` or a code
   (`crates/hotsheet-server/src/routes/providers/mod.rs:1247-1254`); the client
   cannot tell a rate limit from a conflict and never retries
   (`clients/web/src/features/ticket-workflows.ts:794-800`). Follow-up: HS2-P6VGDC.
2. **Synchronous external writes publish no change event.**
   `reindex_hosted_provider_write` is a no-op for GitHub, GitLab and Jira
   (`crates/hotsheet-server/src/routes/providers/tickets.rs:565-586`); only
   GitHub webhooks invalidate other clients. The baseline view
   `HS2-NativeIssueWrite` claimed otherwise and is corrected here. Follow-up: HS2-YBF6Y3.
3. **A server restart silently skips events.** The client adopts a lower
   cursor without a resync (`crates/hotsheet-server/src/events.rs:663`,
   `clients/web/src/project-change-poll.ts:273`). This resolves S-U6. Follow-up: HS2-BMBWF9.
4. **The server's stale-token check is not atomic with the write.** The token
   is compared with an index read before `ops::update` takes the transaction
   (`crates/hotsheet-server/src/routes/ticket_writes.rs:146-154`,
   `crates/hotsheet-server/src/routes/ticket_writes.rs:276`), the same race as
   core D5 on the server's Git path. Follow-up: extend HS2-W3JD5K. Follow-up: HS2-W3JD5K (extended).
5. **Unbound-store claim contention is silent.** No conflict is recorded or
   pushed (`crates/hotsheet-ticketing/src/claim_gate.rs:696-697`). Follow-up: HS2-JKMV7Q.
6. **Broker unreachability releases live claims.** The monitor treats an
   unreachable broker like an exited terminal
   (`crates/hotsheet-server/src/routes/terminals/launch.rs:913`). Follow-up: HS2-YZHAYR.
7. **The store prefix differs by path.** The GUI uses `HS2`; `launch` and
   `hotsheet-migrate` default to `HS` (`clients/web/src/project-bridge.ts:907`,
   `crates/hotsheet-cli/src/dispatch/setup.rs:990`,
   `crates/hotsheet-cli/src/bin/hotsheet-migrate.rs:43`). Follow-up: HS2-WCF6NY.
8. **Bootstrap headless parity gaps.** The stable hook CLI has no headless
   installer; the GUI pushes the first commit and saves the backup origin
   while the CLI only prints; the GUI drops bootstrap's next steps
   (`clients/web/src/local-host.ts:271-292`,
   `clients/web/src/project-bridge.ts:1053-1107`). Follow-up: HS2-NTH6S7.
9. **Migration headless parity gaps.** `hotsheet-migrate` does not set up,
   register or link the project and takes no job lock, and HS1 cleanup has no
   CLI (`clients/web/src/project-bridge.ts:648-659`,
   `clients/web/src/project-bridge.ts:895-901`). The backup proof is
   implemented twice (`clients/web/src/hs1-backup.ts:121-148`,
   `crates/hotsheet-cli/src/import_completion.rs:34-94`). Follow-up: HS2-NTH6S7.
10. **MCP claim tools with `connection` call a route that does not exist.**
    `checkout_route` builds `/providers/{connection}/tickets/{id}/claim`
    (`crates/hotsheet-mcp/src/lib.rs:1541-1548`); the server registers no
    claim route under `/providers` (`crates/hotsheet-server/src/lib.rs:2203-2237`). Follow-up: HS2-XA5ZCN.
11. **The model attributes CI observation to the server.** The relationships
    `jobsUseGithub`, `jobsUseGitlab` and `jobsUseCi` say `serverJobs`
    observes CI (`docs/architecture/model/server-relationships.dsl:39-44`);
    the detached CLI monitor does (`crates/hotsheet-cli/src/dispatch/jobs.rs:551-575`). Follow-up: none; corrected in this baseline (the three serverJobs CI edges were removed and the serverJobs description fixed).
12. **Dead route.** `/__hotsheet/projects/migrate-hs1` has no client caller
    (`clients/web/src/dev-server.ts:478-489`). Follow-up: HS2-P1T6BC.

Unresolved claims for this area are in the [register](../unresolved.md) (FLOW-U1, FLOW-U2, FLOW-U3, FLOW-U4).
