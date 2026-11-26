# Rust server, terminals, AI drive and notifications

Status: written by HS2-P27EJM. Audited source
`8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`. Follow the depth standard and evidence format in the architecture
README. Module-by-module rows are in the [server coverage file](../coverage/server.md).

## Overview

`hotsheet-server` is one Axum/Tokio process per machine. It serves a primary
store given by `-C`, plus every store it hosts for configured or opened
projects. The binary also has four other modes, chosen by flags before any
store is opened:

- `--opencode-v2-terminal` runs an OpenCode helper and exits
  (`crates/hotsheet-server/src/main.rs:171`).
- `--terminal-broker-process` runs the detached terminal broker
  (`crates/hotsheet-server/src/main.rs:230`).
- `--terminal-launch-relay` runs the macOS launch relay; on other platforms it
  refuses (`crates/hotsheet-server/src/main.rs:235`).
- `--stop` signals the running server for the store and exits
  (`crates/hotsheet-server/src/main.rs:246`).

The router has 444 method and path pairs in 345 `.route` calls
(`crates/hotsheet-server/src/lib.rs:1677-2967`). Every pair has an operation in
`docs/contracts/v1/openapi.yaml`, and the contract has no path the router lacks.
`clients/web/scripts/check-openapi-routes.mjs:11` enforces this.
Test: `clients/web/scripts/check-openapi-routes.mjs` (run by the root gate).

The model splits the `server` container into 30 components in four groups. Each
group has its own view.

| View                       | Components                                                                                                                                                                                    |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `HS2-Server-Admission`     | `serverLifecycle`, `serverTls`, `serverMiddleware`, `serverTrust`, `serverPairing`, `serverHosting`                                                                                           |
| `HS2-Server-Sources`       | `serverTickets`, `serverSources`, `serverWriteBehind`, `serverAdvisory`, `serverCredentials`, `serverWebhooks`, `serverIndex`, `serverSync`, `serverEvents`, `serverLive`, `serverRepository` |
| `HS2-Server-Execution`     | `serverTerminal`, `serverDrive`, `serverDistWork`, `serverPermissions`, `serverAiTools`, `serverPlugins`, `serverSchedule`                                                                    |
| `HS2-Server-Notifications` | `serverEvents`, `serverNotify`, `serverNotifiers`, `serverLive`                                                                                                                               |

`serverOrch`, `serverJobs` and `serverResources` are the server host of the
orchestration domain. The domain is in [20-orchestration](20-orchestration.md).
This chapter covers only the routes, tasks and authority around them.

The `broker`, `launchRelay` and `saver` containers have their own component
views: `HS2-Server-Broker`, `HS2-Server-LaunchRelay` and `HS2-Server-Saver`.

## Startup and shutdown

`main` runs this sequence (`crates/hotsheet-server/src/main.rs:169-701`). Each
step names its line.

1. Register `hotsheet-cli merge-driver` as the semantic merge driver
   (`crates/hotsheet-server/src/main.rs:179`).
2. Handle the info flags `--test-fixture-capabilities` (`:182`) and
   `--build-revision` (`:192`). Refuse a fixture-owned run without the
   isolated permission vault (`crates/hotsheet-server/src/main.rs:720-726`).
   Test: `crates/hotsheet-server/src/main.rs`
   (`hs2_b14hyw_fixture_owner_requires_isolated_artifact_without_changing_normal_hosts`).
3. Install the stderr `tracing` subscriber. `RUST_LOG` overrides the `info`
   default, and write failures are dropped (`crates/hotsheet-server/src/main.rs:915`).
4. Branch to the broker, relay or stop modes (`:230`, `:235`, `:246`).
   `--stop --kill-all-terminals` waits up to 5 s for the server to go, then
   lists and kills every broker terminal (`crates/hotsheet-server/src/main.rs:268-291`).
   Test: `crates/hotsheet-server/src/main.rs` (`kill_all_terminals_requires_an_explicit_server_stop`).
5. **Join, don't collide.** If a live instance file exists for the store, print
   its URL and exit 0. The secret is printed only for a loopback URL
   (`crates/hotsheet-server/src/main.rs:297-311`).
6. **Writer lock.** Take the per-store writer lock before opening any store or
   credential (`crates/hotsheet-server/src/main.rs:314`). See
   [Durable state](#durable-state-and-file-formats).
   Test: `crates/hotsheet-server/tests/concurrent_start.rs`
   (`simultaneous_starts_leave_exactly_one_registered_server`).
7. Open the primary store with role admission (`crates/hotsheet-server/src/main.rs:320`).
8. **Listener mode.** A loopback `--bind` (default `127.0.0.1:8787`) is a local
   HTTP listener. An off-loopback bind without `--remote-transport` defaults to
   mTLS; `http` and `https` must be chosen explicitly
   (`crates/hotsheet-server/src/main.rs:28-32`). A remote listener always gets a
   second, plain-HTTP loopback listener from `--local-bind` (default
   `127.0.0.1:0`). That listener must be loopback
   (`crates/hotsheet-server/src/main.rs:700-718`).
   Test: `crates/hotsheet-server/src/main.rs`
   (`hs2_w4jjje_remote_transport_is_explicit_except_legacy_mtls`,
   `hs2_qpjq7a_a_remote_listener_gets_a_loopback_local_listener`).
9. The secret is `--secret` or a fresh ULID (`crates/hotsheet-server/src/main.rs:359`).
10. Open and reconcile the index, prune stale index files, and unregister
    abandoned temporary checkouts. Pruning failures are only logged
    (`crates/hotsheet-server/src/main.rs:366-369`).
11. Build `AppState`. The bus is a 256-slot broadcast
    (`crates/hotsheet-server/src/lib.rs:564`). The builders add persistent
    registered indexes, deferred permission rules, client-drive persistence
    under `<home>/drive/<store id>/` and the machine remote-auth catalog
    (`crates/hotsheet-server/src/main.rs:376-380`). Optional parts follow:
    - remote-listener mode (`:392`);
    - the Jira outbox, only when `HOTSHEET_JIRA_WRITE_BEHIND=1`, with capacity
      1000 (`:394-400`);
    - the advisory claim ledger (`:403`);
    - the detached broker on Unix unless `--no-terminal-broker` (`:415-416`). A
      broker start failure aborts startup; nothing falls back to in-process
      terminals.
12. Host the stores in `stores.json` (`:423`). Reconcile feedback deliveries on
    a blocking task (`:431`). Resume broker terminal monitors (`:435`). Prune
    names of terminals that did not survive (`:438`). Release orphaned chat
    drives (`:443`).
13. Spawn the background services: watcher (`:446`), notification router
    (`:449`), auto-grouper (`:452`), group-registry background commits (`:455`),
    group-registry sync watch (`:458`), sync loop (`:461`) and Jira dispatch
    loop (`:466`).
14. Bind the listeners (`:468`). Start the permission-rule loader thread. The
    bridge stays fail-closed until it finishes, because Keychain can block
    (`crates/hotsheet-server/src/main.rs:492`).
15. The opt-in distributed driving loop starts only with `--drive-tool`
    (`crates/hotsheet-server/src/main.rs:512`).
16. Install the stop signals (`:579`), the stdin-EOF watcher (`:581`) and the
    test-owner watchdog (`:584`).
17. Publish one instance file per hosted store (`crates/hotsheet-server/src/main.rs:603`).
    Then start the services that need the URL: catalog prewarm, host-health
    sampler, run recovery, the permission follower, the hosted-session
    follower, the target-line push, the scheduler-wake follower and the
    command-schedule runner (`:607-624`).
18. **Serve and drain.** A single future serves the remote listener (TLS or
    plain) and the local listener (`crates/hotsheet-server/src/main.rs:631-669`).
    The first SIGTERM, SIGINT or quiescent-restart request calls
    `begin_stopping`. The listeners stop accepting, and long waits that race
    `stopping()` finish: permission asks answer 503, and long polls return
    empty (`crates/hotsheet-server/src/lib.rs:754-779`). Open connections get
    `--shutdown-drain-ms` (default 5000) to finish. The deadline, or a second
    signal, forces exit (`crates/hotsheet-server/src/main.rs:744-764`).
19. **Cleanup.** In order: stop AI probes, flush background autocommits and
    group-registry commits, wait up to 2 s for worklist refreshes, remove the
    instance files and writer locks, then drop the primary writer lock
    (`crates/hotsheet-server/src/main.rs:673-681`). A drained run arms a
    watchdog thread that calls `exit(0)` if background work outlives the drain
    (`crates/hotsheet-server/src/main.rs:767`). A forced run exits 0 at once.

Tests: `crates/hotsheet-server/tests/graceful_shutdown.rs`
(`stop_cancels_a_held_permission_hook_with_503_and_exits_cleanly`,
`stop_cancels_an_idle_long_poll`, `drain_deadline_forces_exit_past_a_stuck_request`,
`second_stop_signal_forces_exit_during_a_long_drain`,
`a_sigkilled_owner_cannot_leave_its_server_running`).

The server does not supervise itself. A crash leaves the instance file and the
lock file behind. Both read as stale because their pid is dead, and the next
start reclaims them (`crates/hotsheet-server/src/lifecycle.rs:178-182`).
Test: `crates/hotsheet-server/src/lifecycle.rs` (`find_instance_ignores_a_stale_file`,
`writer_lock_blocks_a_second_live_holder_but_reclaims_a_stale_one`).

On Windows there is no SIGTERM. `stop` calls `TerminateProcess`, so the
graceful drain never runs and the stale instance file is replaced on the next
start (`crates/hotsheet-server/src/lifecycle.rs:466-471`, `crates/hotsheet-server/src/lifecycle.rs:493-497`). Test: untested on
the CI platforms named in this chapter.

**Quiescent restart.** A mutating request takes an `ActiveMutation` counter in
`require_secret`. While the server is quiescing, the counter is refused with
503 (`crates/hotsheet-server/src/routes/auth.rs:180-189`,
`crates/hotsheet-server/src/routes/tickets.rs:127-143`). `POST /lifecycle/restart`
and repository compaction are exempt. `shutdown_requested` lets the signal task
treat a granted restart like SIGTERM (`crates/hotsheet-server/src/lib.rs:748`,
`crates/hotsheet-server/src/main.rs:778-791`).

## Admission and authority

Requests pass four layers, outermost first.

1. **Timing and offload.** `request_timing::time_request` warns on any request
   over its latency budget. `blocking_offload::offload` runs every
   non-streaming handler on the blocking pool, so synchronous file, lock and
   SQLite work never holds an async worker
   (`crates/hotsheet-server/src/lib.rs:2963-2965`).
2. **Transport.** On an mTLS listener, `tls::serve_tls_with_acl` reads the ACL
   file on every handshake. The roles are:
   - no ACL file: read-write;
   - unknown or unreadable fingerprint: deny;
   - read-only: allows only GET, HEAD and OPTIONS, and never terminal attach
     (`crates/hotsheet-server/src/tls.rs:178-254`).
     Off-box certificate callers are refused `/remotes` and every orchestration
     machine path (`crates/hotsheet-server/src/tls.rs:204-218`).
     Test: `crates/hotsheet-server/tests/mtls.rs`
     (`explicit_acl_is_live_and_enforces_read_only_read_write_and_unknown`).
3. **Caller.** `require_secret` (`crates/hotsheet-server/src/routes/auth.rs:130`)
   admits exactly one of three callers and records which one:
   - `Certificate`, from the TLS extension, recorded as `cert-<fingerprint>`;
   - `LocalBearer`, when `X-Hotsheet-Secret` matches and the peer is allowed.
     With a remote listener, only loopback peers may use the bearer
     (`crates/hotsheet-server/src/lib.rs:949-951`);
   - `BrowserSession { local }`, from `remote_auth::session`: a paired bearer,
     or the `hs2_remote_session` cookie. A cookie write needs the paired
     Origin and the CSRF header (`crates/hotsheet-server/src/routes/remote_auth.rs:335-367`).

   Primary-store routes, such as `/tickets` and `/batch`, also check that the
   store at the path still has the startup instance id
   (`crates/hotsheet-server/src/routes/auth.rs:16-42`). The
   `stamp_change_cursor` layer then stamps `X-Hotsheet-Change-Cursor` with the
   replay cursor read before the handler ran
   (`crates/hotsheet-server/src/events.rs:186-200`).
   Tests: `crates/hotsheet-server/tests/remote_listener_modes.rs`
   (`hs2_w4jjje_remote_peer_cannot_use_local_bearer_on_any_entry_point`),
   `crates/hotsheet-server/tests/http.rs`
   (`hs2_sqkjyg_responses_carry_the_change_cursor_they_were_served_at`).

4. **Per-route.** The `HostAccess` and `HostTrust` extractors refuse a browser
   session without the `local` grant
   (`crates/hotsheet-server/src/routes/auth.rs:91-128`). Five handlers take
   them: the repository browser and code-review launchers, and a policy test
   pins that count (`crates/hotsheet-server/src/routes/auth.rs:259-300`).
   Terminals, command runs and AI drive launches also start host programs but
   are not host-gated. `docs/04-core-server-cli.md` describes that as
   deliberate.
   Test: `crates/hotsheet-server/tests/remote_host_access.rs`
   (`hs2_s8r3pd_host_routes_need_the_local_grant_or_the_local_bearer`).

Public routes authorize themselves:

- `/health`;
- the 14 `/remote-auth/*` routes;
- `/webhooks/github/{connection_id}`, which needs a per-hook HMAC and has a
  256 KiB limit;
- `/resource-claim-token/{action}`, which takes a renewal bearer, with a
  per-peer failure budget of 30 failures in 10 s;
- `/ws/sync`, `/ws/poll` and `/terminals/{id}/attach`, which take the query
  secret, a certificate or a session (`crates/hotsheet-server/src/lib.rs:2901-2962`).

**Device catalog.** `RemoteAuth` keeps one JSON catalog at
`<home>/remote-auth/machine.json`. Every write takes a file lock, re-reads the
catalog, applies the change and atomically replaces the file with mode 0600
(`crates/hotsheet-server/src/remote_auth.rs:311-316`,
`crates/hotsheet-server/src/remote_auth.rs:600-618`,
`crates/hotsheet-server/src/remote_auth.rs:1078-1101`). The catalog keeps:

- pending pairings with their 15-minute TTL;
- sessions with a 90-day absolute life and a 30-day idle life;
- the reminder schedule from day 45;
- SHA-256 digests of the token and CSRF values;
- revoked tombstones;
- a limit of 10 pairing requests per minute per catalog and 5 redeem attempts
  per pairing (`crates/hotsheet-server/src/remote_auth.rs:47-66`).

Idle activity is persisted at most once a minute
(`crates/hotsheet-server/src/remote_auth.rs:797-856`).

Device management (`/remote-auth/owner/*`) accepts the machine secret from an
allowed peer, or an admin session with Origin and CSRF
(`crates/hotsheet-server/src/routes/remote_auth.rs:278-318`). `revoke-all`
accepts only the machine secret (`crates/hotsheet-server/src/routes/remote_auth.rs:642-652`).
Test: `crates/hotsheet-server/tests/remote_auth.rs`
(`hs2_85w6be_dxm0yn_admin_and_local_grants_gate_device_management`).

## Change stream: events, cursors, replay and overflow

- **Emit.** `emit_change` pushes the event into the `EventLog` ring and gives it
  the next `seq` as its `cursor`. Only then does it broadcast the event
  (`crates/hotsheet-server/src/events.rs:639-646`). The ring keeps 512 events
  (`crates/hotsheet-server/src/events.rs:626`). The broadcast channel holds 256
  (`crates/hotsheet-server/src/lib.rs:564`). Every emitted event gets a cursor,
  even ones built with `cursor: None`. The one exception is `announce`: it is
  broadcast without entering the ring, carries no cursor and is never replayed
  (`crates/hotsheet-server/src/events.rs:486-500`). Test:
  `crates/hotsheet-server/tests/http.rs`
  (`announce_broadcasts_live_but_is_not_persisted_in_the_poll_ring`). The
  `ChangeEvent` fields and kinds are in
  `crates/hotsheet-server/src/events.rs:589-615`. The coverage file lists the
  kinds each route emits.
- **WebSocket `/ws/sync`.** The socket only pushes; it never replays. It
  subscribes before the upgrade and holds a presence lease for each `checkout`
  query value. A remote session is rechecked every second, and the socket is
  closed when the session is revoked. Pairing events are filtered out for
  callers who are not device managers. On any receive error, including
  `Lagged`, the loop ends and the socket closes
  (`crates/hotsheet-server/src/routes/store_scoped.rs:160-209`,
  `crates/hotsheet-server/src/routes/store_scoped.rs:409-438`).
  Test: `crates/hotsheet-server/tests/sync_ws.rs`
  (`websocket_push_and_long_poll_fallback_share_one_replay_cursor`). The
  lag-close path is untested.
- **Long poll `/ws/poll`.** Without `since`, the poll returns the current cursor
  and no backlog. With `since`, it subscribes first and then reads the ring, so
  no event falls in the gap. It answers at once with the backlog, or with
  `overflow: true` when `since + 1` is older than the oldest retained event.
  Otherwise it waits up to `timeout_ms`: 25 s by default, capped at 55 s. On
  wake it re-reads the ring, so the cursor never skips an undelivered event.
  If the poll falls behind the broadcast, it returns overflow. A stopping
  server ends the wait early. A remote session is rechecked after the wait
  (`crates/hotsheet-server/src/routes/store_scoped.rs:441-560`).
  Test: `crates/hotsheet-server/src/events.rs`
  (`since_returns_the_tail_and_advances_the_cursor`,
  `falling_behind_the_ring_signals_overflow`).
- **Read gap.** An authenticated read carries the cursor it was served at.
  The client replays `since` that cursor instead of reading again. On
  `overflow`, the web client re-lists
  (`clients/web/src/project-change-poll.ts:218`).
- **Lost data.** The ring and the bus are in memory only. A server restart
  starts again at cursor 0. A client cursor from the old process is then ahead
  of `seq`, so the poll returns nothing until new events pass it. Clients
  handle this by reconnecting.

  > **Unresolved:** web-side detection of a server restart (cursor reset) was
  > not traced. See [Unresolved](#unresolved).

The `HS2-Server-ChangeReplay` view shows the lagged-socket and overflow path.

## Terminals

`serverTerminal` has two backends, and every route checks which is present.

- **Detached broker.** The default on Unix. Each operation becomes a
  `BrokerRequest` on the broker socket
  (`crates/hotsheet-server/src/terminal_broker.rs:151`).
- **In-process manager.** Used on Windows (ConPTY) or with
  `--no-terminal-broker`
  (`crates/hotsheet-server/src/main.rs:729-730`, `crates/hotsheet-terminals/src/terminal.rs:577`).

**Broker protocol.** The socket is a Unix stream socket at
`/tmp/hotsheet2-brokers-<euid>/<hash>.sock`, in a 0700 directory with a 0600
socket and an owner lock file (`crates/hotsheet-terminals/src/broker_socket.rs:154-238`).
The broker relies on those file permissions; it does not check the peer uid.
The relay does (`crates/hotsheet-terminals/src/launch_relay.rs:147-150`).

Messages are newline-delimited JSON with no line-length cap
(`crates/hotsheet-terminals/src/broker.rs:547-552`). Requests are tagged
`op`, responses `result` and stream frames `frame`
(`crates/hotsheet-terminals/src/broker.rs:70-265`). `attach` turns the
connection into a stream:

- outbound: the scrollback, then the size, then output, size and replacement
  frames;
- inbound: input and resize frames.

There is no version number. An old broker answers an unknown op with
`bad request`, and the server treats that answer as "unsupported"
(`crates/hotsheet-terminals/src/broker.rs:582-590`,
`crates/hotsheet-server/src/terminal_broker.rs:171-174`).

Buffers:

- Each terminal's replay ring defaults to 256 KiB, clamped to 64 KiB through
  64 MiB.
- The live broadcast holds 256 chunks.
- Each viewer queue holds 32 frames. When it fills, output is coalesced and
  then replaced by a full scrollback
  (`crates/hotsheet-terminals/src/terminal.rs:27-37`,
  `crates/hotsheet-terminals/src/viewer_output.rs:26-31`).

A broker exits after five idle minutes with no terminals and no clients.
`BrokerClient::request` has no timeout
(`crates/hotsheet-terminals/src/broker.rs:944-957`).
Test: `crates/hotsheet-terminals/src/broker.rs`
(`attach_streams_scrollback_then_live_output_and_forwards_input`).

**Attach** (`crates/hotsheet-server/src/routes/terminals/attach.rs:32`).

- **Admission.** A certificate, or the query secret from an allowed peer, or a
  paired session with write access. A remote session is rechecked every second
  (`crates/hotsheet-server/src/routes/terminals/attach.rs:41-47`,
  `crates/hotsheet-server/src/routes/terminals/attach.rs:121-129`).
- **First frames.** The replay is the first binary frame, sent even when empty.
  Then come a size frame and a liveness hello.
- **Replay protocol 2.** Opted-in viewers get framed, capped and
  deflate-compressed replays, and can resume from a stream offset
  (`crates/hotsheet-server/src/routes/terminals/replay.rs:50-229`).
- **Sizing.** Resize claims go to the size arbiter, in process or in the
  broker. A claim is released when its connection closes.
- **Broker death.** If the broker dies during an attach, the socket closes and
  the server does not reconnect it
  (`crates/hotsheet-server/src/routes/terminals/attach.rs:525-530`).

Tests: `crates/hotsheet-server/tests/terminal_ws.rs`
(`attach_replays_scrollback_streams_output_and_forwards_input`,
`broker_mode_attach_streams_through_the_broker`). Broker death during an
attach is untested.

**Launch relay (macOS).** Before every broker spawn on macOS, the server tries
the per-user relay. The relay is a LaunchAgent labelled
`com.smalltale.hotsheet2.terminal-relay`, limited to the Aqua session
(`crates/hotsheet-terminals/src/launch_agent.rs:25`,
`crates/hotsheet-terminals/src/launch_agent.rs:167-209`).

The relay speaks line JSON protocol 1, with `hello` and `launch` ops and a
1 MiB line cap. Both sides check the peer uid. The server uses the relay only
when it runs the same executable
(`crates/hotsheet-terminals/src/launch_relay.rs:30-179`,
`crates/hotsheet-terminals/src/launch_relay.rs:376-396`). Any
`RelayUnavailable` result falls back to a direct spawn
(`crates/hotsheet-server/src/terminal_launch_relay.rs:76-87`).

`hotsheet-cli terminal-relay install` is the headless setup path.
Test: `crates/hotsheet-server/tests/terminal_launch_relay.rs`
(`hs2_hyf56y_dead_namespace_server_gets_a_live_broker_through_the_relay`,
`hs2_hyf56y_without_a_relay_the_server_spawns_directly_and_says_why`).

## AI drive, tools, plugins and permissions

- **Client drive.** `serverDrive` keeps chat connections in `ClientDriveManager`,
  persisted to `<home>/drive/<store>/sessions.json`
  (`crates/hotsheet-server/src/client_drive.rs:362-417`,
  `crates/hotsheet-server/src/main.rs:373-380`). Each turn runs on its own OS
  thread (`crates/hotsheet-server/src/routes/permissions.rs:1407`). The drive
  environment includes `HOTSHEET_SECRET`, the machine bearer
  (`crates/hotsheet-server/src/routes/permissions.rs:1144-1147`). Chat drives do
  not survive a restart; their claims are released at startup
  (`crates/hotsheet-server/src/main.rs:443`).
  Test: `crates/hotsheet-server/tests/http.rs`
  (`client_drive_starts_resumes_and_interrupts_through_real_routes`).
- **Tool adapters.** `hotsheet-aitools` has one `Drive` interface with optional
  capabilities, and callers never branch on the tool id
  (`crates/hotsheet-aitools/src/drive.rs:1-3`). The drive shapes are:
  - spawn per turn (`crates/hotsheet-aitools/src/spawn.rs:1`);
  - the Codex app-server (`crates/hotsheet-aitools/src/appserver.rs:1`,
    `crates/hotsheet-aitools/src/codex.rs:1`);
  - the Claude channel (`crates/hotsheet-aitools/src/claude.rs:1`);
  - ACP (`crates/hotsheet-aitools/src/acp.rs:1`).
    Session push reaches running interactive sessions
    (`crates/hotsheet-aitools/src/session_push/mod.rs:1`).
- **Plugins.** Manifests come from four bundled directories (Claude, Codex,
  Antigravity, OpenCode) and from `<home>/plugins/<id>/`. First-party ids win a
  collision (`crates/hotsheet-plugins/src/lib.rs:11-46`). Setup is idempotent
  and runs the same way from the CLI (`crates/hotsheet-plugins/src/setup.rs:1`).
- **Permission bridge.** One `SharedPermissionBridge` lives in `AppState`. Its
  pending and removed hooks emit `permission_asked` and `permission_resolved`
  on the bus (`crates/hotsheet-server/src/lib.rs:580-624`). Remembered rules
  load on a thread after bind, so the bridge stays fail-closed while the
  Keychain prompt is open (`crates/hotsheet-server/src/lib.rs:1242-1290`). The
  flow is in the `HS2-Server-PermissionRoundTrip` view.

## Notifications and standby

- **Router.** `spawn_notification_router` subscribes to the bus, seeds ticket
  snapshots on a blocking task, then forwards every event in order to one
  blocking worker over an unbounded channel. A 60 s tick flushes held
  summaries, refreshes every live state and rewrites the standby snapshot. A
  router that lags the 256-slot bus logs the skip count and loses those
  notifications (`crates/hotsheet-server/src/notification_router.rs:131-205`).
  Test: `crates/hotsheet-server/tests/notification_delivery.rs`.
- **Dispatch.** Dispatch is synchronous on that worker. It matches rules,
  suppresses repeats within each rule's cooldown, and retries retryable
  failures after 2 s and 8 s. Records go to `notification-deliveries.jsonl`
  (`crates/hotsheet-notify/src/dispatch.rs:20`,
  `crates/hotsheet-notify/src/dispatch.rs:69-80`). Channels and rules live in
  `<home>/notifications.json` and hold only key-registry references
  (`crates/hotsheet-notify/src/config.rs:1`).
  Test: `crates/hotsheet-notify/src/dispatch.rs`
  (`cooldown_suppresses_repeats_per_channel_and_key`).
- **Standby snapshot.** It mirrors `live_state::current_all` into
  `standby-snapshot.json` in each screen-saver container directory that
  already exists. It has a 3-minute `valid_until_ms` and is written by
  temp-file rename (`crates/hotsheet-server/src/standby_saver.rs:19-42`,
  `crates/hotsheet-ticketing/src/screensaver.rs:88-149`). The Swift saver
  watches the directory with a `DispatchSource` and passes valid JSON to the
  bundled page (`clients/macos-screensaver/Sources/HotSheetSaverView.swift:37-77`).
  The snapshot holds no credential, but it does include absolute store root
  paths (`crates/hotsheet-server/src/live_state.rs:106-126`).
  Test: `crates/hotsheet-server/tests/standby_saver.rs`
  (`hs2_mzm988_the_running_router_keeps_the_snapshot_current_from_live_state_events`).

## Route inventory

There is one row for each module under `crates/hotsheet-server/src/routes`, and
one for each non-route module the router mounts. The router line range for each
module is in `crates/hotsheet-server/src/lib.rs:1677-2967`. Unless a row says
otherwise, every route sits behind `require_secret` and the global offload
layer.

"Primary gate" means the route also checks the primary store's identity
(`crates/hotsheet-server/src/routes/auth.rs:16-42`). Path abbreviations:

- `R/` is `crates/hotsheet-server/src/routes/`.
- `T/` is `crates/hotsheet-server/tests/`.

The cells name each module and test; the full citations are in the coverage
file.

| Module                                              | Endpoints | Paths                                                                                                                                                                                                                        | Authority beyond `require_secret`                                                                                                                                            | Domain call                                                                                         | Events emitted                                                                                                                                                                     | Test                                                                                                   |
| --------------------------------------------------- | --------: | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `accounts.rs`                                       |        10 | `/stores` GET, `/accounts/*`, `/github-auth/device/*`                                                                                                                                                                        | none                                                                                                                                                                         | `hotsheet_extsync` device flow, `hotsheet_ticketing::accounts`                                      | none                                                                                                                                                                               | `T/http.rs` (`project_owned_sources_and_machine_wide_accounts`)                                        |
| `activity.rs`                                       |         6 | `/activity`, `/announce`, `/stores/{id}/activity`, `/checkouts/{ref}/activity[/current-diff]`                                                                                                                                | primary gate on `/activity`; diff confined to the checkout root                                                                                                              | `activity::record`, `activity::timeline`, `git diff` subprocess (64 KiB cap)                        | `activity`, `announce` (bus only)                                                                                                                                                  | `T/http.rs` (`checkout_current_diff_is_on_demand_confined_and_non_retained`)                           |
| `auth.rs`                                           |         0 | middleware                                                                                                                                                                                                                   | defines `require_secret`, `HostAccess`, `HostTrust`                                                                                                                          | none                                                                                                | none                                                                                                                                                                               | `R/auth.rs` (in-file)                                                                                  |
| `checkout_tickets/` (`mod.rs`, `attachments.rs`)    |        27 | `/checkouts/{ref}/tickets/{id}/*`, `/batch`, `/follow-up-links`, `/trash/*`, the attachment routes                                                                                                                           | `HostAccess` on the two attachment `action` routes                                                                                                                           | `ticket_writes::do_*`, `ops::*`, `FsStore` attachment writes, `TicketProvider`                      | `updated`, `deleted`, `attachment_added`, `attachment_removed`, `attachment_metadata_updated`, `attachment_renamed`, `attachment_markup_updated`, `attachment_annotations_updated` | `T/http.rs` (`checkout_scoped_ticket_routes_aggregate_and_resolve_linked_stores`)                      |
| `checkout_workers.rs`                               |        12 | `/checkouts/{ref}/workers`, `worker-plan*`, `*-strategies`, `development-strategy`, `speculation/*`                                                                                                                          | the coordinator checks plan authority                                                                                                                                        | `hotsheet_cli::coordination::Coordinator`, `StrategyPolicy`, `DevelopmentPolicy`, `speculation_api` | `development_strategy_changed`                                                                                                                                                     | `T/http.rs` (`hs2_dbecd0_worker_view_uses_authenticated_registered_checkout_and_replays_cursor`)       |
| `checkouts/` (`mod.rs`, `listing.rs`, `removal.rs`) |        29 | `/checkouts`, `/projects/*`, `/checkouts/{ref}`, `sources/*`, `repository/*`, `code-review`, `usage`, `tickets` list, `corrupt-tickets*`                                                                                     | `HostAccess` on open, relocation, register, relink, review and file actions; `HostTrust` for Git source add; removal refused (409) while a presence lease holds the checkout | `CheckoutRegistry`, `repository_browser`, `code_review`, `repository_compaction`, `checkout_page`   | `repository_changed`                                                                                                                                                               | `T/http.rs` (`checkout_registry_is_authenticated_and_resolvable`)                                      |
| `claims.rs`                                         |         8 | `/claim-next`, `/tickets/{id}/{claim,release,renew}` and the checkout variants                                                                                                                                               | primary gate; the worker id comes from the body; holder checks run in `ops`                                                                                                  | `ops::claim_*`, `release`, `renew_with_eta` (30 min default lease), `provisional_claims`            | `claimed`, `released`, `renewed`                                                                                                                                                   | `T/http.rs` (`claim_next_release_renew_over_http`)                                                     |
| `command_schedules.rs`                              |         5 | `/checkouts/{ref}/command-schedules*`, `commands/{id}/ai-run`                                                                                                                                                                | none (starts AI sessions on the host)                                                                                                                                        | `hotsheet_ticketing::command_schedules`, `Settings::set_if`, `ops::claim_with_eta`                  | `command_schedules_updated`, `claimed`, `released`, `note_added`                                                                                                                   | `R/command_schedules.rs` (in-file only)                                                                |
| `features.rs`                                       |         3 | `/feature-settings[/changed]`                                                                                                                                                                                                | none                                                                                                                                                                         | `hotsheet_ticketing::features`, global settings                                                     | `feature_settings`                                                                                                                                                                 | `R/features.rs` (in-file)                                                                              |
| `feedback_journal.rs`                               |         0 | helper                                                                                                                                                                                                                       | none                                                                                                                                                                         | per-note journal `<home>/feedback-deliveries/<ULID>.json`                                           | none                                                                                                                                                                               | `R/feedback_journal.rs` (in-file)                                                                      |
| `feedback_session.rs`                               |         1 | `/checkouts/{ref}/tickets/{id}/notes/{note}/notify`                                                                                                                                                                          | the note must be a human note on a Started ticket                                                                                                                            | candidate routing, `session_messages`, `send_feedback_turn`                                         | `updated`                                                                                                                                                                          | `T/session_messages.rs` (`hs2_wkrfd5_a_failed_delivery_record_never_resends_across_retry_and_restart`) |
| `groups.rs`                                         |        19 | `/groups/*` and the `stores`, `providers` and `checkouts` variants                                                                                                                                                           | primary gate on `/groups`                                                                                                                                                    | `hotsheet_ticketing::groups`, `GroupRegistry`                                                       | `updated`, `group_registry_changed`                                                                                                                                                | `T/groups_http.rs`                                                                                     |
| `guidance.rs`                                       |        12 | `/checkouts/{ref}/guidance/*`                                                                                                                                                                                                | none (writes project instruction files)                                                                                                                                      | `hotsheet_plugins` `guidance_*`, `rulesync_*`, `setup`                                              | none directly                                                                                                                                                                      | `T/http.rs` (`checkout_guidance_route_selects_and_applies_pinned_rules`)                               |
| `mcp_presence.rs`                                   |         3 | `/mcp-presence[/{lease}]`                                                                                                                                                                                                    | none                                                                                                                                                                         | `crate::mcp_presence` (15 s heartbeat, 45 s TTL)                                                    | `mcp_presence`                                                                                                                                                                     | `T/http.rs` (`mcp_presence_lease_registers_renews_silently_lists_marks_terminal_and_unregisters`)      |
| `mod.rs`                                            |         0 | declares the route modules                                                                                                                                                                                                   | none                                                                                                                                                                         | none                                                                                                | none                                                                                                                                                                               | none                                                                                                   |
| `notifiers.rs`                                      |        12 | `/notifiers`, `/notification-*`                                                                                                                                                                                              | none (including plugin `trust`)                                                                                                                                              | `hotsheet_notify::admin`                                                                            | none                                                                                                                                                                               | `T/notification_delivery.rs` (`admin_routes_validate_store_secrets_by_reference_and_test_channels`)    |
| `orchestration.rs`                                  |        94 | `/checkouts/{owner}/orchestration/*`, the hosted `/checkouts/{c}/target-*`, `/remotes/{id}/checkouts/{c}/orchestration/target-lines`, `/remote-checkouts/{c}/resolution`                                                     | the mTLS ACL refuses machine paths to off-box callers; per-operation authority is in `hotsheet_cli`                                                                          | `OrchestrationRuntime::request_now` through the two handlers `read` and `write`                     | `orchestration_changed`, `terminal_orchestrator`                                                                                                                                   | `T/remote_servers/orchestration.rs`                                                                    |
| `orchestration_permissions.rs`                      |         0 | helper                                                                                                                                                                                                                       | none                                                                                                                                                                         | `BridgePrompt`, `sync_waiting`, `resolve`, `rules`                                                  | `permission_resolved`                                                                                                                                                              | `T/remote_servers/permissions.rs`                                                                      |
| `orchestration_sessions.rs`                         |         0 | helper (`SessionHost`)                                                                                                                                                                                                       | in-process calls bypass extractors                                                                                                                                           | `open_terminal`, `kill_terminal`, `session_messages`                                                | `orchestration_session`                                                                                                                                                            | `T/remote_servers/sessions.rs`                                                                         |
| `orchestration_settings_watch.rs`                   |         0 | helper                                                                                                                                                                                                                       | none                                                                                                                                                                         | file watcher over owner settings                                                                    | `orchestration_changed`                                                                                                                                                            | `T/remote_servers/projects.rs` (`hs2_shhgc6_a_cli_change_without_the_server_is_announced`)             |
| `permissions.rs`                                    |        21 | `/permissions`, `/permissions/ask`, `/permissions/bridge-probe`, `/connections`, `/ai-tools`, `/ai-settings`, `/drive/*`, `/checkouts/{ref}/{ai-settings,permission-automation*,permissions/legacy*,drive/connections/{id}}` | none (AI launches are host programs by design)                                                                                                                               | `SharedPermissionBridge`, `ClientDriveManager`, AI tool discovery                                   | `drive_updated`, `turn_event`, `permission_rules_changed` (and the bridge's `permission_asked` and `permission_resolved`)                                                          | `T/http.rs` (`permissions_ask_blocks_then_returns_the_human_answer`)                                   |
| `project_tools.rs`                                  |        36 | `/analytics/*`, `/confidence-report`, `/phase-report`, `/commands*`, `/command-groups`, `/command-runs*`, `/views`, `/notifications*`, `/tts/*` and the checkout variants                                                    | primary gate; command writes need `If-Match`                                                                                                                                 | `CommandManager`, `analytics`, `custom_views`, `tts`                                                | `views_updated`, `notification_acknowledged`, `command_updated`                                                                                                                    | `T/http.rs` (`configured_commands_stream_output_keep_history_and_cancel`)                              |
| `providers/` (`mod.rs`, `tickets.rs`)               |        28 | `/providers*`, `/provider-connections*`, `/provider-transfers/*`, `/provider-attachments/copy`, `/checkouts/{ref}/provider-connections*`                                                                                     | a disabled connection is refused; moves need `confirm` and an operation id                                                                                                   | `provider_for`, `TicketProvider`, `copy_between`, `move_between`                                    | `created`, `updated` (hosted Git connections only)                                                                                                                                 | `T/http.rs` (`provider_transfer_is_idempotent_and_move_closes_source_after_copy`)                      |
| `remote_auth.rs`                                    |        14 | `/remote-auth/*` (public)                                                                                                                                                                                                    | self-authorizing; `owner` and `manager` checks                                                                                                                               | `RemoteAuth`                                                                                        | `pairing_requests`, `browser_grants`                                                                                                                                               | `T/remote_auth.rs`                                                                                     |
| `remotes.rs`                                        |         7 | `/remote-identity`, `/remote-checkouts`, `/remotes*`                                                                                                                                                                         | the mTLS ACL refuses `/remotes` off-box                                                                                                                                      | `hotsheet_cli::remote_servers::RemoteRuntime`                                                       | none                                                                                                                                                                               | `T/remote_servers.rs` (`hs2_1y45mz_authenticated_service_and_serverless_mcp_use_the_same_registry`)    |
| `resource_claims.rs`                                |         7 | `/resource-claims/*`, `/resource-claim-token/{action}` (public)                                                                                                                                                              | renewal token for renew and release; the public route takes a bearer and is throttled                                                                                        | `hotsheet_resource_claims::ClaimService` (55 s wait, 64 parked)                                     | none                                                                                                                                                                               | `T/resource_claims.rs`                                                                                 |
| `session_messages.rs`                               |         1 | `/connections/{id}/message`                                                                                                                                                                                                  | none                                                                                                                                                                         | `hotsheet_aitools::session_push`                                                                    | none                                                                                                                                                                               | `T/session_messages.rs`                                                                                |
| `standby.rs`                                        |         3 | `/standby-settings[/changed]`                                                                                                                                                                                                | none                                                                                                                                                                         | `hotsheet_ticketing::standby`, global settings                                                      | `standby_settings`                                                                                                                                                                 | `R/standby.rs` (in-file)                                                                               |
| `store_scoped.rs`                                   |        10 | `/stores/{id}/tickets*`, `/resolve/{id}`, `/setup/{tool}`, `/checkouts/{ref}/close`, `/ws/sync` and `/ws/poll` (public)                                                                                                      | primary gate on `/setup`; streams authorize themselves                                                                                                                       | `ticket_writes::do_*`, `hotsheet_plugins::run_setup_in`, `presence`                                 | through `do_*`                                                                                                                                                                     | `T/sync_ws.rs`                                                                                         |
| `sync_kick.rs`                                      |         1 | `/sync/kick`                                                                                                                                                                                                                 | the store must be hosted or registered                                                                                                                                       | `AppState::request_store_sync`                                                                      | `ticket_store_sync`                                                                                                                                                                | `T/sync_kick.rs`                                                                                       |
| `terminals/`                                        |        34 | `/terminals*`, `/terminal-settings`, `/terminal-limits*`, `/permissions/{id}*`, `/permissions/rules`, the checkout terminal and trash settings, `/terminals/{id}/attach` (public)                                            | none (launches are host programs by design); attach authorizes itself                                                                                                        | `TerminalBroker` or `TerminalManager`, `hotsheet_aitools`                                           | `terminal_*`, `permission_resolved`, `permission_rules_changed`                                                                                                                    | `T/terminal_ws.rs`                                                                                     |
| `ticket_creation_settings.rs`                       |         2 | `/checkouts/{ref}/ticket-creation-settings`                                                                                                                                                                                  | none                                                                                                                                                                         | shared settings `auto_group_new_tickets`                                                            | none                                                                                                                                                                               | `T/http.rs` (`source_free_checkout_settings_round_trip_under_the_project_root`)                        |
| `ticket_writes.rs`                                  |         9 | `/tickets` POST and PATCH, notes, attachments, close, assign, `/batch`, `/stores` POST, `/stores/{id}/tickets` GET                                                                                                           | primary gate; `HostAccess` on `POST /stores`                                                                                                                                 | `ops::create_by`, `update`, `close_as`, `assign`, `FsStore`                                         | `created`, `updated`, `assigned`, `assignment`, `closed`, `attachment_added`                                                                                                       | `T/http.rs` (`create_get_update_close_and_query`)                                                      |
| `tickets.rs`                                        |         7 | `/health` (public), `/tickets` GET, `/tickets/{id}`, `note-doctor`, `/compatibility`, `/lifecycle/{quiescence,restart}`                                                                                                      | primary gate; restart needs only the bearer                                                                                                                                  | index listing, `health_scan`, `quiescence_report`                                                   | none                                                                                                                                                                               | `T/http.rs` (`lifecycle_restart_preserves_active_work_then_closes_mutation_admission`)                 |
| `transfer.rs`                                       |         2 | `/tickets/{id}/copy`, `/tickets/{id}/move`                                                                                                                                                                                   | primary gate; a move needs `confirm`                                                                                                                                         | `ops::copy_ticket`, `move_ticket`                                                                   | `created`, `moved`                                                                                                                                                                 | `T/http.rs` (`cross_store_copy_and_move_between_hosted_stores`)                                        |
| `crate::host_health`                                |         7 | `/host-health*`                                                                                                                                                                                                              | none                                                                                                                                                                         | monitor, global settings                                                                            | `host_health`                                                                                                                                                                      | `crates/hotsheet-server/src/host_health/tests.rs`                                                      |
| `crate::live_state`, `crate::live_history`          |         4 | `/live-state`, `/stores/{id}/live-state`, `/stores/{id}/live-tokens`, `/live-history`                                                                                                                                        | none                                                                                                                                                                         | `live_state::refresh`                                                                               | `live_state` (a GET can emit it)                                                                                                                                                   | `T/live_state.rs`                                                                                      |
| `crate::ci_status`                                  |         1 | `/checkouts/{ref}/ci-status`                                                                                                                                                                                                 | none                                                                                                                                                                         | `CiReports::record`                                                                                 | through `live_state::touch`                                                                                                                                                        | `crates/hotsheet-server/src/ci_status.rs` (in-file)                                                    |
| `crate::provider_write_behind`                      |         4 | `/providers/{c}/tickets/queued`, `/providers/{c}/outbox*`                                                                                                                                                                    | `write_behind` capability and the outbox enabled                                                                                                                             | `provider_outbox`                                                                                   | `updated`                                                                                                                                                                          | `T/jira_outbox_http.rs`                                                                                |
| `crate::provider_advisory_claims`                   |         2 | `/checkouts/{c}/providers/{conn}/tickets/{id}/advisory-claim`                                                                                                                                                                | the checkout must link a Jira connection                                                                                                                                     | `advisory_claim_ledger`, Jira provider                                                              | `advisory_claim_changed`                                                                                                                                                           | `T/jira_advisory_claim_http.rs`                                                                        |
| `crate::github_webhooks`                            |         3 | `/providers/{c}/webhook` GET and PUT, `/webhooks/github/{c}` (public)                                                                                                                                                        | HMAC, source binding, dedupe                                                                                                                                                 | `provider.get` re-read                                                                              | `changed`, `deleted`                                                                                                                                                               | `T/http.rs` (`hs2_sgq183_webhook_setup_and_delivery_are_authenticated_and_source_bound`)               |

The endpoint counts total 444. OpenAPI path and method drift is zero in both
directions.

The contract drift that remains is in security schemes, parameters and status
codes; it is listed under [Discrepancies](#discrepancies).

## Durable state and file formats

Every path below is under the machine home `${HOTSHEET_HOME:-~/.hotsheet2}`
unless it says otherwise.

| File                                                      | Writer                                                                                                     | Format and crash behavior                                                                                                   |
| --------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `instances/<project id>.json`                             | `InstanceRegistry::register_instance` (`crates/hotsheet-server/src/lifecycle.rs:186`)                      | Pretty JSON holding the bearer, mode 0600. A crash leaves the file, which reads as stale once its pid is dead.              |
| `instances/<project id>.lock` and `.lock.mutex`           | `acquire_writer_lock` (`crates/hotsheet-server/src/lifecycle.rs:247`)                                      | The pid, published by temp file and rename. The `.lock.mutex` file is a persistent `flock` target. A dead pid is reclaimed. |
| `index/<id>.v<schema>.sqlite`                             | index writer (see [10-core](10-core.md))                                                                   | Rebuildable. Older schema generations are pruned at start.                                                                  |
| `remote-auth/machine.json`                                | `RemoteAuth::write` (`crates/hotsheet-server/src/remote_auth.rs:600`)                                      | JSON, mode 0600, under a file lock with re-read and atomic replace. Holds digests only.                                     |
| `outbox/<store id>.sqlite`                                | Jira write-behind (`crates/hotsheet-server/src/main.rs:394-400`)                                           | SQLite. Operations survive a restart; a send in flight is requeued (see Unresolved).                                        |
| `advisory_claims/provisional.sqlite`                      | `provider_advisory_claims` (`crates/hotsheet-server/src/main.rs:401-403`)                                  | SQLite. Replayed on the next read.                                                                                          |
| `drive/<store>/sessions.json`, `drive/<store>/homes/`     | `ClientDriveManager` (`crates/hotsheet-server/src/client_drive.rs:839-866`)                                | Temp file and persist. A corrupt file fails startup.                                                                        |
| `permissions/<store>.json`, `checkout-<id>.json`          | permission vault (`crates/hotsheet-aitools/src/permission_vault.rs:19-24`)                                 | Encrypted envelope v4 with the key in the OS credential store and a `flock` sibling.                                        |
| `feedback-deliveries/<ULID>.json`                         | `feedback_journal` (`crates/hotsheet-server/src/routes/feedback_journal.rs:93-109`)                        | Temp file, fsync, replace and a directory fsync. Reconciled at startup. A corrupt entry refuses rather than resends.        |
| `notifications.json`, `notification-deliveries.jsonl`     | `hotsheet-notify` (`crates/hotsheet-notify/src/config.rs:14`, `crates/hotsheet-notify/src/dispatch.rs:20`) | Secret-free config. The append journal compacts at 1000 entries and keeps 500.                                              |
| `ci-status.json`                                          | `CiReports` (`crates/hotsheet-server/src/ci_status.rs:26`)                                                 | Version 1, atomic replace. A corrupt file is ignored.                                                                       |
| `claude-usage/`, `codex-usage/<terminal>.json`            | terminal usage follows                                                                                     | Ids and paths only. Resumed after a restart.                                                                                |
| `stores.json`                                             | the CLI and users (read here)                                                                              | `{ "stores": [...] }`. A malformed file reads as empty.                                                                     |
| `/tmp/hotsheet2-brokers-<euid>/*.sock`, `*.lock`          | broker and relay (`crates/hotsheet-terminals/src/broker_socket.rs:154-238`)                                | 0700 directory, 0600 socket and lock. A stale socket with a free lock is reclaimed.                                         |
| `~/Library/Containers/.../HotSheet/standby-snapshot.json` | `standby_saver` (`crates/hotsheet-ticketing/src/screensaver.rs:88-149`)                                    | Version 1 with a 3-minute validity. Written only where the directory exists.                                                |
| store `metrics/local/*`                                   | live state, live history, live record                                                                      | Machine-local and gitignored. Best effort.                                                                                  |

## Concurrency, tasks and locks

**Runtime.** `#[tokio::main]` uses the default multi-thread runtime
(`crates/hotsheet-server/src/main.rs:169`).

Handlers run on the blocking pool through `blocking_offload`. The pool is
limited by a 256-permit semaphore; streaming routes stay async
(`crates/hotsheet-server/src/blocking_offload.rs:47-49`).

Only one place catches panics: `catch_unwind` in the worklist refresher
(`crates/hotsheet-server/src/worklist_refresh.rs:124`). A panicking background
loop ends and is not restarted. Poisoned mutexes are recovered through
`LockExt::lock_or_recover` (`crates/hotsheet-sync/src/lib.rs:21-36`).
Test: `crates/hotsheet-server/src/events.rs`
(`poisoned_server_locks_are_recovered_by_later_writes`).

**Task and thread inventory.** Kinds: T is a Tokio task, B a `spawn_blocking`
loop, S an OS thread.

| Task                                                               | Kind    | Spawn site                                                                                                                         | Owns, trigger and cadence                                                            | Shutdown                                                             | Test                                                                                                                 |
| ------------------------------------------------------------------ | ------- | ---------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | -------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Store watchers (primary and per store)                             | S       | `crates/hotsheet-server/src/watcher.rs:166`                                                                                        | native events debounced 150 ms; 250 ms bridge poller until live; repair retry 1–60 s | drop of `WatchHandle`                                                | `crates/hotsheet-server/src/watcher.rs` (`the_watch_loop_walks_every_freshness_transition`)                          |
| Repository monitors                                                | S       | `crates/hotsheet-server/src/hosting.rs:218-245`                                                                                    | Git status after events (175 ms quiet, 1 s max); 750 ms to 2 s fallback poller       | drop; unhost sweep                                                   | `crates/hotsheet-server/src/watcher.rs` (`git_repository_monitor_emits_once_for_a_stable_change`)                    |
| Notification router                                                | T + B   | `crates/hotsheet-server/src/notification_router.rs:131-205`                                                                        | bus events in order; 60 s tick (held flush, live refresh, standby write)             | never; ends with the process                                         | `crates/hotsheet-server/tests/notification_delivery.rs`                                                              |
| Auto-grouper                                                       | T + S   | `crates/hotsheet-server/src/auto_group.rs:55-72`                                                                                   | bus `created` events; coalesce 3 s; turn limit 3 min                                 | never                                                                | `crates/hotsheet-server/src/auto_group.rs` (`hs2_fn018q_new_tickets_are_grouped_in_one_coalesced_turn`)              |
| Sync loop                                                          | S       | `crates/hotsheet-server/src/sync_loop.rs:356-366`                                                                                  | 30 s base, write kicks, backoff to 300 s, trash purge every 6 h                      | never (the sender lives in `AppState`); skips passes while quiescing | `crates/hotsheet-server/src/sync_loop.rs` (`healthy_kick_runs_an_immediate_pass_and_timeouts_keep_the_base_cadence`) |
| Jira dispatch                                                      | T + B   | `crates/hotsheet-server/src/provider_write_behind.rs:288-298`                                                                      | 2 s sleep; at most 4 operations per pass                                             | never; does not observe `stopping()` or quiescence                   | untested (the pass is tested)                                                                                        |
| Permission-rule load                                               | S       | `crates/hotsheet-server/src/lib.rs:1266`                                                                                           | once, after bind                                                                     | detached                                                             | `crates/hotsheet-server/src/permission_startup_tests.rs`                                                             |
| Distributed driving loop                                           | S       | `crates/hotsheet-server/src/dist_work_loop.rs:643`                                                                                 | 15 s; only with `--drive-tool`                                                       | sender dropped when `main` returns                                   | `crates/hotsheet-server/src/dist_work_loop.rs`                                                                       |
| Signal listener, stdin EOF, owner watchdog                         | T, S, S | `crates/hotsheet-server/src/main.rs:778`, `crates/hotsheet-server/src/main.rs:795`, `crates/hotsheet-server/src/main.rs:884`       | signals; stdin EOF; owner checked every 250 ms                                       | after the second signal; at EOF; when the owner dies                 | `crates/hotsheet-server/tests/graceful_shutdown.rs`                                                                  |
| AI catalog prewarm                                                 | S       | `crates/hotsheet-server/src/routes/permissions.rs:820-834`                                                                         | once; probes stopped at shutdown                                                     | detached                                                             | `crates/hotsheet-server/tests/graceful_shutdown.rs` (`stop_is_prompt_while_startup_ai_catalog_discovery_hangs`)      |
| Host-health sampler                                                | T + B   | `crates/hotsheet-server/src/host_health/mod.rs:1344-1407`                                                                          | `sample_seconds` (default 60)                                                        | `stopping()`; a panicked sample ends it for good                     | `crates/hotsheet-server/src/host_health/tests.rs`                                                                    |
| Run recovery and followers                                         | T + B   | `crates/hotsheet-server/src/routes/orchestration.rs:265-560`                                                                       | long-request steps; backoff 1–60 s                                                   | `is_stopping()`; inactive                                            | `crates/hotsheet-server/tests/remote_servers/recovery.rs`                                                            |
| Permission, hosted-session, line-push and scheduler-wake followers | S       | `crates/hotsheet-server/src/routes/orchestration_permissions.rs:145`, `crates/hotsheet-server/src/routes/orchestration.rs:777-903` | file-signal waits with 1 s safety rechecks                                           | `is_stopping()`                                                      | `crates/hotsheet-server/tests/remote_servers/sessions.rs`                                                            |
| Command-schedule runner                                            | T + B   | `crates/hotsheet-server/src/routes/command_schedules.rs:1515-1577`                                                                 | next due time, at most 60 s; `Notify` kick                                           | `stopping()`                                                         | `crates/hotsheet-server/src/routes/command_schedules.rs`                                                             |
| Unhost sweeper                                                     | T       | `crates/hotsheet-server/src/hosting.rs:315-321`                                                                                    | earliest requested due time                                                          | never                                                                | `crates/hotsheet-server/tests/http.rs` (`closing_the_last_open_project_unhosts_its_store_until_it_is_needed_again`)  |
| Terminal session monitors                                          | T       | `crates/hotsheet-server/src/routes/terminals/launch.rs:794-1048`                                                                   | 500 ms local liveness poll                                                           | when the terminal ends                                               | `crates/hotsheet-server/tests/http.rs` (`broker_terminal_sessions_release_claims_including_after_a_server_restart`)  |
| Attach pumps                                                       | T       | `crates/hotsheet-server/src/routes/terminals/attach.rs:380-407`                                                                    | per viewer                                                                           | aborted on drop                                                      | `crates/hotsheet-server/tests/terminal_ws.rs`                                                                        |
| Client-drive turn                                                  | S       | `crates/hotsheet-server/src/routes/permissions.rs:1407`                                                                            | one per turn                                                                         | at turn end                                                          | `crates/hotsheet-server/tests/http.rs` (`client_drive_starts_resumes_and_interrupts_through_real_routes`)            |
| Command runs                                                       | S × 3   | `crates/hotsheet-server/src/commands.rs:332-453`                                                                                   | exit watcher polls `try_wait` every 20 ms, plus two output readers                   | when the process exits                                               | `crates/hotsheet-server/src/commands.rs`                                                                             |
| MCP lease expiry                                                   | T       | `crates/hotsheet-server/src/routes/mcp_presence.rs:143-163`                                                                        | 15 s heartbeat, 45 s TTL                                                             | when the lease is gone                                               | `crates/hotsheet-server/src/mcp_presence.rs` (registry only)                                                         |
| Target-store and code-ref watches                                  | S       | `crates/hotsheet-server/src/target_store_watch.rs:62-216`                                                                          | file events with a 1 s recheck; pruned after 10 idle minutes on the next follow      | `forget`                                                             | `crates/hotsheet-server/src/target_store_watch.rs`                                                                   |
| Live-state rebuild                                                 | T + B   | `crates/hotsheet-server/src/live_state.rs:377-401`                                                                                 | 250 ms coalesce                                                                      | one-shot                                                             | `crates/hotsheet-server/tests/live_state.rs`                                                                         |
| TLS connections                                                    | T       | `crates/hotsheet-server/src/tls.rs:117-168`                                                                                        | per connection; no handshake timeout                                                 | drained on stop                                                      | `crates/hotsheet-server/tests/mtls.rs`                                                                               |

The subagent inventories also list the one-shot threads: the corrupt-ticket
prewarm, feedback reconciliation, the device-flow poller, the Codex bootstrap
watch, the usage-reset query and the relay reaper. Their tests were not
located; see [Unresolved](#unresolved).

**Locks.** All `AppState` locks are `std::sync::Mutex`, wrapped in `Arc`.

- The data each lock guards is listed in the `AppState` field comments
  (`crates/hotsheet-server/src/lib.rs:260-506`).
- No global lock order is documented. The convention is one lock at a time,
  dropped before I/O.
- Three acquisitions nest:
  - `live_state` takes `building` and then `stores`
    (`crates/hotsheet-server/src/live_state.rs:409-410`).
  - `watch_checkout_repository` holds `repository_watchers` across a Git
    subprocess (`crates/hotsheet-server/src/hosting.rs:223-229`).
  - The managed-secret cache holds its mutex across the OS credential read on
    purpose, so parallel requests share one prompt
    (`crates/hotsheet-server/src/credential_cache.rs:81-82`).

**Admission.** Mutations and background passes are counted by `ActiveMutation`
and `ActiveBackgroundWork`. Only sync and driving passes take background work
(`crates/hotsheet-server/src/routes/tickets.rs:144-159`). Jira dispatch,
auto-group turns, schedule passes and orchestration followers are not counted
when a restart checks for quiescence
(`crates/hotsheet-server/src/lib.rs:794-875`).

## Failure and recovery

- **Server crash.** The broker keeps its PTYs. Instance and lock files read as
  stale. The ring and bus are lost, and clients re-list on overflow. The
  outbox, advisory ledger, feedback journal and drive catalog are replayed or
  reconciled at the next start. Chat drives are released.
- **Broker crash.** The next broker call spawns an empty broker. Monitors see
  `not_found` and release session claims. The next start prunes the names of
  the lost terminals.
- **Keychain prompt at startup.** HTTP keeps serving while permission rules
  stay locked.
- **Lagging subscribers.**
  - WebSocket: the socket closes.
  - Poll: `overflow`.
  - Router and auto-grouper: events are lost, with a warning.
- **Remote session revoked.** WebSocket, attach and poll streams close within
  about 1 s, or after the poll wait.
- **Jira failure.** See the `HS2-Server-JiraWriteBehind` view below.

## Configuration and environment

The coverage file lists the flags and defaults under `serverLifecycle`. These
environment variables are read in this area:

| Variable                                                                                                                                                                     | Default and effect                                                                               |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `HOTSHEET_HOME`                                                                                                                                                              | `~/.hotsheet2` (`crates/hotsheet-ticketing/src/machine_home.rs:33`)                              |
| `HOTSHEET_SERVER_LAUNCHER`                                                                                                                                                   | used when `--launcher` is absent; otherwise the launcher is recorded as unspecified              |
| `HOTSHEET_JIRA_WRITE_BEHIND`                                                                                                                                                 | `1` enables the outbox                                                                           |
| `HOTSHEET_TEST_SERVER_OWNER_PID`, `HOTSHEET_TEST_ROOMY_CAPACITY`                                                                                                             | test fixtures only                                                                               |
| `RUST_LOG`                                                                                                                                                                   | `info`                                                                                           |
| `HOTSHEET_HOST_HEALTH_SAMPLER`                                                                                                                                               | `off` disables the sampler                                                                       |
| `HOTSHEET_SYNC_AI_ALLOWED_TOOLS`                                                                                                                                             | unset allows every tool (`crates/hotsheet-server/src/sync_ai.rs:134`)                            |
| `HOTSHEET_FFMPEG`, `HOTSHEET_CACHE_DIR`                                                                                                                                      | `ffmpeg` on `PATH`; the platform cache directory (`crates/hotsheet-server/src/media.rs:187-293`) |
| `HOTSHEET_SCREENSAVER_HOME`                                                                                                                                                  | screen-saver container override                                                                  |
| `HOTSHEET_AI_TOOL_PROBES`                                                                                                                                                    | `off` disables model probes                                                                      |
| `HOTSHEET_LAUNCHCTL`, `HOTSHEET_LAUNCH_AGENTS_DIR`                                                                                                                           | `/bin/launchctl`; `~/Library/LaunchAgents`                                                       |
| `HOTSHEET_PROCESS_JOURNAL_HOME`, `HOTSHEET_BROKER_CAPTURE_STARTUP_STDERR`                                                                                                    | broker process options                                                                           |
| `HOTSHEET_PERMISSION_PROJECT`, `HOTSHEET_CLAUDE_PRETOOLUSE`, `HOTSHEET_CLAUDE_HEADLESS_AUTO`, `HOTSHEET_WORKER_ID`, `HOTSHEET_ACTOR_ROLE`, `HOTSHEET_ACTOR_ID`, `CODEX_HOME` | AI drive and hook wiring                                                                         |
| `SHELL`, `COMSPEC`, `ZDOTDIR`, `PATH`, `PATHEXT`                                                                                                                             | terminal and command shells, and tool resolution                                                 |
| `OPENCODE_SERVER_PASSWORD`                                                                                                                                                   | required for `--opencode-v2-terminal`                                                            |

Compile-time values: `HOT_SHEET_BUILD_REVISION`,
`HOT_SHEET_LOCAL_SOURCE_ROOT` and the bundled GitHub App id and slug
(`crates/hotsheet-server/src/source_revision.rs:45-46`,
`crates/hotsheet-server/src/github_app_config.rs:4-5`).

## Dynamic views

### `HS2-Server-ChangeReplay`: WebSocket lag, cursor replay and overflow

1. A ticket write calls `changed_in`. It records the event in the 512-event
   ring, which assigns cursor n, then broadcasts it on the 256-slot bus
   (`crates/hotsheet-server/src/events.rs:235`,
   `crates/hotsheet-server/src/events.rs:639-646`).
2. `ws_loop` sends the event as one text frame
   (`crates/hotsheet-server/src/routes/store_scoped.rs:425-434`).
3. A receiver that fell 256 events behind gets `Lagged`. The loop breaks and the
   socket drops without a close frame
   (`crates/hotsheet-server/src/routes/store_scoped.rs:426`). The client polls
   `/ws/poll?since=<last cursor>`.
4. The poll takes a presence lease, which also schedules an unhost sweep
   (`crates/hotsheet-server/src/routes/store_scoped.rs:213-235`).
5. It subscribes, reads the ring, and returns the backlog with the new cursor.
   If `since + 1` is older than the oldest retained event, it returns
   `overflow: true` (`crates/hotsheet-server/src/routes/store_scoped.rs:506-521`,
   `crates/hotsheet-server/src/events.rs:664-680`).
6. On overflow the client re-lists. The read's `X-Hotsheet-Change-Cursor`
   names the cursor to resume from (`crates/hotsheet-server/src/events.rs:186-200`,
   `clients/web/src/project-change-poll.ts:218`).

Tests: `crates/hotsheet-server/src/events.rs`
(`falling_behind_the_ring_signals_overflow`),
`crates/hotsheet-server/tests/sync_ws.rs`
(`websocket_push_and_long_poll_fallback_share_one_replay_cursor`). Step 3 is
untested.

### `HS2-Server-BrokerReattach`: server restart and the crashed-broker path

1. At startup `with_terminal_broker`, through `TerminalBroker::ensure`, pings
   the broker socket.
   - A live broker of another build is retired only when idle; one with
     terminals is adopted.
   - A missing broker is spawned, through the relay on macOS.

   (`crates/hotsheet-server/src/terminal_broker.rs:58-96`,
   `crates/hotsheet-server/src/terminal_broker.rs:440-497`.)

2. `resume_broker_terminal_sessions` lists the surviving terminals
   (`crates/hotsheet-server/src/routes/terminals/launch.rs:1103-1147`).
3. It re-registers AI connections, push state and usage follows, and arms exit
   watchers that release claims.
4. `prune_orphaned_terminal_names` keeps only live ids. It skips pruning when
   the broker cannot be reached
   (`crates/hotsheet-server/src/routes/terminals/launch.rs:1164-1187`).
5. A viewer reconnects to `/terminals/{id}/attach`.
6. The broker stream's first frame is the replay, or only the missed bytes for
   a matching `resume_from`
   (`crates/hotsheet-server/src/routes/terminals/replay.rs:157-182`).
7. **Failure:** if the broker died, the next `call` runs `connect_or_restart`,
   which spawns an empty broker. The viewer gets 404, and pollers get
   `not_found` and release claims
   (`crates/hotsheet-server/src/terminal_broker.rs:188-231`).

Tests: `crates/hotsheet-server/tests/broker_startup.rs`
(`long_home_starts_real_server_and_detached_broker_and_preserves_terminals_on_restart`),
`crates/hotsheet-server/tests/terminal_replay.rs`
(`hs2_k93qha_a_broker_reconnect_resumes_with_only_missed_output`),
`crates/hotsheet-server/src/terminal_broker.rs`
(`a_call_connection_restarts_an_idle_broker_with_a_missing_socket`). Killing the
broker mid-session end to end is untested.

### `HS2-Server-DevicePairing`

1. The remote-auth screen posts a label to `/__hotsheet/remote-auth/pair`
   (`clients/web/src/components/remote-auth-screen.tsx:214`), and the bridge
   forwards it.
2. `pair` requires `Origin` and calls `RemoteAuth::request`. That applies the
   limit of 10 requests a minute, creates a challenge, stores its digest with a
   15-minute expiry, and returns 201
   (`crates/hotsheet-server/src/routes/remote_auth.rs:369-381`,
   `crates/hotsheet-server/src/remote_auth.rs:620-660`).
3. An identity-free `pairing_requests` event reaches only device managers
   (`crates/hotsheet-server/src/routes/remote_auth.rs:119-158`).
4. The owner approves with a role and the `admin` and `local` grants. The
   caller must be the machine secret from an allowed peer, or an admin session
   with Origin and CSRF (`crates/hotsheet-server/src/routes/remote_auth.rs:568-589`).
5. The browser redeems with its challenge and Origin. The answer depends on
   the request's state:
   - not yet approved: 409 `unapproved`;
   - expired: 410;
   - after five bad attempts: 429;
   - rejected: 401;
   - approved: a session and CSRF token are issued as HttpOnly, SameSite=Strict
     cookies, and only their digests are stored
     (`crates/hotsheet-server/src/remote_auth.rs:736-795`,
     `crates/hotsheet-server/src/routes/remote_auth.rs:383-421`).

Tests: `crates/hotsheet-server/tests/remote_auth.rs`
(`hs2_7dbjms_pair_approve_redeem_role_replay_revoke`,
`hs2_bzhmev_pairing_request_events_reach_only_device_managers_over_poll`).

### `HS2-Server-PermissionRoundTrip`

1. Before a tool call, the tool's hook runs `hotsheet-cli permission-hook`,
   which posts to `/permissions/ask`
   (`crates/hotsheet-cli/src/dispatch/setup.rs:2199`).
2. `ask_permission` does three things before it parks
   (`crates/hotsheet-server/src/routes/permissions.rs:564-621`):
   - answers 503 at once if the server is stopping;
   - marks the hook's terminal as AI-connected;
   - resolves the project.

   It then parks on `request_timeout_with_pending_async`, with a 24-hour
   timeout and deny as the default. The ask holds no thread.

3. If no rule matches, the bridge enqueues the request. Its pending hook emits
   `permission_asked` (`crates/hotsheet-server/src/lib.rs:580-601`).
4. A client lists `GET /permissions` and answers `POST /permissions/{id}` with
   a decision and scope (`crates/hotsheet-server/src/routes/terminals/signals.rs:1407-1551`).
   - An Always answer persists an encrypted rule.
   - An orchestrator request is forwarded to the orchestration runtime.
5. The removed hook emits `permission_resolved`, and the parked ask answers
   `{decision}`.
6. **Failure paths:**
   - A hook that disconnects drops its future, which withdraws the prompt as a
     one-time deny.
   - The timeout denies.
   - A stopping server answers 503, and the hook falls back to the tool's own
     prompt.

   This answers U-CLI-2: the server bounds the ask.

Codex approvals use the same bridge in-protocol. OpenCode ACP permission
requests never reach it (see Discrepancies). Tests:
`crates/hotsheet-server/tests/http.rs`
(`permissions_ask_blocks_then_returns_the_human_answer`,
`an_always_answer_persists_a_rule_that_auto_resolves_next_time`),
`crates/hotsheet-server/tests/graceful_shutdown.rs`
(`stop_cancels_a_held_permission_hook_with_503_and_exits_cleanly`),
`crates/hotsheet-server/tests/terminal_ws.rs`
(`fake_agent_permission_round_trip_in_a_terminal_persists_always_rule`).

### `HS2-Server-JiraWriteBehind`

1. `POST /providers/{c}/tickets/queued` validates that the edits touch only
   title, details, category, priority and tags. It admits the batch with its
   operation ids into the outbox (capacity 1000) and answers 202 with the
   provisional ticket
   (`crates/hotsheet-server/src/provider_write_behind.rs:159-240`,
   `crates/hotsheet-server/src/provider_write_behind.rs:446-470`).
2. The loop claims up to four ready operations every 2 s and emits `updated`
   (`crates/hotsheet-server/src/provider_write_behind.rs:264-298`).
3. `dispatch_one` reads the issue first:
   - If the issue already shows the edit, it confirms.
   - If the remote fields differ from the base, it records a conflict and
     still writes the local intent.
   - It writes with the remote concurrency token. An acknowledgement that
     lacks the intent needs attention
     (`crates/hotsheet-server/src/provider_write_behind.rs:311-389`).
4. **Failure:** the outcome depends on the error
   (`crates/hotsheet-server/src/provider_write_behind.rs:390-440`):
   - A rate limit defers the operation and its whole connection by
     `Retry-After`, or 30 s.
   - Authentication, disabled, not-found or unsupported errors need attention.
   - Every 8th attempt needs attention.
   - Any other error backs off `2^min(attempts, 6)` seconds plus jitter.

   A manual retry opens a new window of eight attempts. A discard reads back
   first.

5. Each settled operation emits `updated`.

Tests: `crates/hotsheet-server/tests/jira_outbox_http.rs`
(`failed_dispatch_requeues_with_backoff_and_later_edits_stay_behind_it`,
`attention_retry_and_discard_keep_the_remote_and_projection_honest`,
`timeout_after_success_and_rate_limit_reconcile_without_duplicate_write`).

## Discrepancies

Each discrepancy names the follow-up ticket filed from HS2-P27EJM. S-U2 is under security investigation in HS2-35J75Q.

1. **OpenAPI security for streams.** `/ws/sync` and `/terminals/{id}/attach`
   declare only `HotsheetQuerySecret`, with `secret` required
   (`docs/contracts/v1/openapi.yaml:5467-5470`). The code also admits client
   certificates and paired sessions
   (`crates/hotsheet-server/src/routes/store_scoped.rs:168-177`,
   `crates/hotsheet-server/src/routes/terminals/attach.rs:41-47`). Protected
   routes also accept `RemoteSessionCookie`, but the global `security` lists
   only `HotsheetSecret` (`docs/contracts/v1/openapi.yaml:8-9`). Follow-up:
   align the OpenAPI security schemes. Follow-up: HS2-0BT74F.
2. **OpenAPI omissions.** The attach replay parameters and control frames are
   not in the contract (`crates/hotsheet-server/src/routes/terminals/replay.rs:50-77`).
   Neither are the 429 and `Retry-After` on `/resource-claim-token`
   (`crates/hotsheet-server/src/routes/resource_claims.rs:133-146`). Follow-up: HS2-0BT74F.
3. **Stale comments.**
   - `RemoteAuth::approve` says `local` is granted only for host-trusted
     callers (`crates/hotsheet-server/src/remote_auth.rs:679-680`). The route
     lets any admin grant it (`crates/hotsheet-server/src/routes/remote_auth.rs:1-5`).
   - The broker is called "opt-in" (`crates/hotsheet-server/src/lib.rs:1498`).
   - `SyncHandle` claims to stop the loop
     (`crates/hotsheet-server/src/sync_loop.rs:94-98`).
   - The plugin header lists two built-ins (`crates/hotsheet-plugins/src/lib.rs:12`)
     and the notifier header three (`crates/hotsheet-notify/src/lib.rs:3`).
   - `--drive-workers` says "concurrently" (`crates/hotsheet-server/src/main.rs:137`). Follow-up: HS2-FSWB8C.
4. **Host access without `HostAccess`.** Compaction execute writes a backup to
   a caller-named host path (`crates/hotsheet-server/src/routes/checkouts/mod.rs:818-905`).
   `routes/auth.rs` says naming a host path needs host trust
   (`crates/hotsheet-server/src/routes/auth.rs:65-67`), and the policy test
   checks only launchers. Follow-up: decide on the policy, then gate the route
   or document it as intended. Follow-up: HS2-JTBA4F.
5. **OpenCode ACP permissions are auto-rejected in-process.** They never reach
   the bridge (`crates/hotsheet-aitools/src/acp.rs:74-90`), and the path is
   untested. Codex with no policy auto-allows
   (`crates/hotsheet-aitools/src/codex.rs:244`). Follow-up: HS2-K6HH96.
6. **Device-flow sessions are never pruned.** Each one counts as a quiescence
   blocker until the process exits, so restart and compaction refuse
   (`crates/hotsheet-server/src/routes/accounts.rs:89-92`,
   `crates/hotsheet-server/src/lib.rs:855-862`). Follow-up: HS2-NNWG8H.
7. **`duplicate-backlinks` pins stores.** It hosts stores outside
   `project_hosted`, so the unhost sweep never releases them
   (`crates/hotsheet-server/src/routes/checkout_tickets/mod.rs:663-665`). Follow-up: HS2-V7NK9T.
8. **WebSocket lag is silent and untested.** `/ws/sync` drops a lagged socket
   without a close frame or an overflow signal
   (`crates/hotsheet-server/src/routes/store_scoped.rs:426`). Follow-up: HS2-8H4ZTX.
9. **Loops ignore shutdown and quiescence.** Jira dispatch, the router, the
   auto-grouper and the unhost sweeper never observe `stopping()`. Jira
   dispatch is not counted by quiescence either
   (`crates/hotsheet-server/src/provider_write_behind.rs:288-298`). Follow-up: HS2-SDDD2A.
10. **GET routes with side effects.** Live state, live history,
    `permissions/rules`, `guidance/rulesync-updates` and the advisory-claim GET
    can write or emit. Follow-up: HS2-GT2Y5G.
11. **Checkout batch partial failure.** It is documented as never partially
    applying, but a write error partway commits the earlier writes
    (`crates/hotsheet-server/src/routes/checkout_tickets/mod.rs:909-1018`). Follow-up: HS2-A8TTD7.
12. **Doc drift.** `docs/25-write-behind-sync-plan.md` still lists dispatch as
    future Phase 3 work.

Not drift: `POST /remote-auth/owner/revoke-all` deliberately accepts only the
machine secret, and the contract says so. The 444 method and path pairs match
in both directions. Follow-up: HS2-BNGVKE (filed from HS2-AH9HAK).

Unresolved claims for this area are in the [register](../unresolved.md) (S-U1, S-U2, S-U3, S-U4, S-U5, S-U6, S-U7, S-U8).
