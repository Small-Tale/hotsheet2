## Node web host and Tauri desktop shell

![](embed:HS2-WebHost-BridgeComponents)

This is the starting diagram for this area. The **Start here** chapter lists the others.

Status: written by HS2-T6SD67 at audited source
`8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`. Per-component fields are in
[coverage/web-host.md](../coverage/web-host.md), which also holds the
definitive Node/browser classification of every `clients/web/src` module.
Views: `HS2-WebHost-BridgeComponents`, `HS2-WebHost-DesktopComponents`, and
six dynamic views listed at the end.

### Overview

The "web server" is a Node process, not the Rust server. It serves the
browser client and owns every `/__hotsheet` route; the browser never holds a
machine-server URL or secret. The Rust `hotsheet-server` is a separate,
detached process the bridge discovers or starts and then talks to over
loopback HTTP and WebSocket. The Tauri shell is a thin native window around
the same Node host.

Of 779 non-test modules under `clients/web/src`, 32 run only in Node, 15 run in
both runtimes and 732 run only in the browser (classification method in the
coverage file). The Node side is concentrated in a handful of files:
`local-host.ts` (production entry), `dev-server.ts` (the route table),
`project-bridge.ts` (sessions, supervision and proxy, 2,617 lines),
`terminal-ws-bridge.ts` (WebSockets) and the `remote-projects/` Node modules.

#### Three ways to run the host

| Mode                                      | Started by                                                                             | Serves the client from                                                             | Node host entry                                                                                                                                                       | Differences                                                                                                                                                                                                                                                                                                                                |
| ----------------------------------------- | -------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Development (`npm run dev`, `dev:hot`)    | `clients/web/scripts/stable-dev.mjs` or Vite directly (`clients/web/package.json:6-7`) | Source modules through Vite on `127.0.0.1:4175` (`clients/web/vite.config.ts:283`) | `src/dev-server.ts` loaded by `@hono/vite-dev-server`, only for allow-listed paths (`clients/web/vite.config.ts:271-275`, `clients/web/src/dev-server-routes.ts:2-3`) | No peer gateway, no desktop storage, `/ux-demo` is the source catalog shell (`clients/web/src/dev-server.ts:575-590`), launcher label `stable-dev` or `dev-bridge` (`clients/web/src/project-bridge.ts:1378-1383`). Binaries default to `<repo>/target/debug` (`clients/web/src/project-bridge.ts:288-324`).                               |
| Plain-browser production (`npm run prod`) | `node dist-host/local-host.js` (`clients/web/package.json:10`)                         | A private snapshot of `dist/` (`clients/web/src/local-host.ts:225-302`)            | `src/local-host.ts` built with `vite build --ssr` (`clients/web/package.json:23`)                                                                                     | Port 4175 by default; picks current release binaries, else current debug ones, and refuses stale ones (`clients/web/src/local-host.ts:130-177`); snapshots binaries so working builds cannot change a running host; launcher label `local-host-snapshot`, which dev hosts may not restart (`clients/web/src/project-bridge.ts:1385-1395`). |
| Desktop (release Tauri)                   | `clients/web/src-tauri/src/lib.rs:129-171`                                             | The bundled `resources/dist`                                                       | `resources/dist-host/local-host.mjs`, an esbuild bundle (`clients/web/scripts/prepare-desktop.mjs:43-54`)                                                             | Port 0 (ephemeral), all binaries supplied by the shell, `HOTSHEET_REPO_ROOT` is the resource directory, desktop storage mounted, exits on stdin EOF. A debug Tauri build instead loads the dev server at `http://127.0.0.1:4175/` and starts no Node (`clients/web/src-tauri/src/lib.rs:225-226`).                                         |

Both production modes and development mount the same route table:
`createLocalHostApp` defaults its bridge to `createDevApp(true)`
(`clients/web/src/local-host.ts:353-359`), and Vite's default export is the same
call (`clients/web/src/dev-server.ts:596`). The `dev` parameter is therefore
`true` in every real host; only tests pass `false`. Test:
`clients/web/src/local-host.test.ts` (routes the bridge, immutable assets,
other static files, and client routes to the document).

A host bound to a non-loopback address refuses to start unless
`HOTSHEET_BROWSER_AUTH_MODE=remote` (`clients/web/src/local-host.ts:409-410`,
`clients/web/vite.config.ts:261-268`). Remote mode is how another device
reaches this host, typically through Tailscale Serve; it changes
authorization, not routing. Test: untested (no test starts a host on a
non-loopback address).

### Components

Bridge container (`bridge`): `hostProductionHost`, `hostViteDev`,
`hostRoutes`, `hostBrowserAuth`, `hostSessions`, `hostSupervisor`,
`hostProxy`, `hostStreams`, `hostRemoteProjects`, `hostPeerGateway`,
`hostDesktopStorage`, `hostMigration`, `hostConversationExport`,
`hostContracts`. Desktop container (`desktop`): `tauriShell`,
`tauriWebview`, `tauriClose`, `tauriRelay`. Packaging is build-time and has a
coverage section but no component. The coverage file records each
component's responsibility, state, entry points, concurrency, errors,
configuration and invariants with citations and tests.

#### Every route the bridge serves

All routes are in `createDevApp` unless noted. "Auth" is the remote-mode
grant from `remoteRouteKind` (`clients/web/src/dev-server.ts:68-82`); in local
mode there is no authorization step. Test for the classification:
`clients/web/src/remote-auth-bridge.test.ts`.

| Route                                                                                            | Auth                                                        | Handled by                                                                                 | Forwards to                                                                                                                                                |
| ------------------------------------------------------------------------------------------------ | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET /__hotsheet/remote-auth/mode`                                                               | open                                                        | bridge (`clients/web/src/dev-server.ts:205`)                                               | nothing                                                                                                                                                    |
| `ALL /__hotsheet/remote-auth/{pair,redeem,renewal,me,rotate,logout,owner/...}`                   | open for the first three, else session; `owner*` management | `browserAuthRequest` (`clients/web/src/dev-server.ts:206-217`)                             | machine server `/remote-auth/...` with the browser's headers; local mode adds the secret only for `owner*` (`clients/web/src/project-bridge.ts:2031-2052`) |
| `POST`, `GET /__hotsheet/dev/latency`                                                            | host                                                        | dev latency log (`clients/web/src/dev-server.ts:220-231`)                                  | `logs/dev-latency.jsonl`                                                                                                                                   |
| `GET /__hotsheet/checkouts`                                                                      | session                                                     | `listServerCheckouts` (`clients/web/src/dev-server.ts:236-245`)                            | bootstrap server `GET /checkouts`, filtered to existing folders (`clients/web/src/project-bridge.ts:1938-1962`)                                            |
| `GET`, `DELETE /__hotsheet/registered-projects[/:id]`                                            | host                                                        | `clients/web/src/dev-server.ts:248-265`                                                    | server `GET /checkouts`, `DELETE /checkouts/{id}`                                                                                                          |
| `POST /__hotsheet/remote/projects/open`                                                          | session                                                     | `openRegisteredProject` (`clients/web/src/dev-server.ts:268-282`)                          | server `/compatibility`, `/projects/open`                                                                                                                  |
| `GET /__hotsheet/remotes`, `/remotes/selected-projects`                                          | host                                                        | `hotsheet-cli remote list                                                                  | selected-projects` (`clients/web/src/dev-server.ts:286-299`)                                                                                               | CLI child process |
| `POST /__hotsheet/remotes/pair`, `/remotes/pair/:handle/redeem`                                  | host                                                        | `clients/web/src/dev-server.ts:300-324`                                                    | peer `/remote-auth/pair` and `/redeem`, then `hotsheet-cli remote register                                                                                 | review`           |
| `GET /__hotsheet/remotes/:id/checkouts`, `/session`; `POST /renew`                               | host                                                        | `clients/web/src/dev-server.ts:325-348`                                                    | `hotsheet-cli remote checkouts                                                                                                                             | session           | renew` |
| `POST /__hotsheet/remotes/:id/projects/:checkout/open`, `DELETE` same                            | host                                                        | `clients/web/src/dev-server.ts:349-366`                                                    | peer `/compatibility`; `hotsheet-cli remote select-project                                                                                                 | unselect-project` |
| `DELETE /__hotsheet/remotes/:id?revision=`                                                       | host                                                        | `clients/web/src/dev-server.ts:367-377`                                                    | `hotsheet-cli remote revoke`; closes that binding's sessions and sockets                                                                                   |
| `POST /__hotsheet/projects/open`                                                                 | host                                                        | `openLocalProject` (`clients/web/src/dev-server.ts:378-392`)                               | CLI setup refresh; server `/compatibility`, `/projects/open`                                                                                               |
| `POST /__hotsheet/server/recover-unhealthy`                                                      | host                                                        | `recoverUnhealthyServer` (`clients/web/src/dev-server.ts:393-404`)                         | signals the exact registered pid                                                                                                                           |
| `POST /__hotsheet/folders/choose`                                                                | host                                                        | native chooser (`clients/web/src/dev-server.ts:405-415`)                                   | `osascript`, PowerShell or `zenity`                                                                                                                        |
| `POST /__hotsheet/conversation-exports/{destination,write,open}`                                 | host                                                        | `clients/web/src/dev-server.ts:416-449`                                                    | filesystem                                                                                                                                                 |
| `POST /__hotsheet/projects/setup-git`, `/setup-git-remote`                                       | host                                                        | `clients/web/src/dev-server.ts:450-477`                                                    | `hotsheet-cli init                                                                                                                                         | bootstrap`, `git` |
| `POST /__hotsheet/projects/migrate-hs1`                                                          | host                                                        | `clients/web/src/dev-server.ts:478-489`                                                    | `hotsheet-migrate`, migrator                                                                                                                               |
| `POST`, `GET /__hotsheet/projects/migration-jobs`                                                | host                                                        | `clients/web/src/dev-server.ts:490-515`                                                    | job registry; `GET` is a long poll                                                                                                                         |
| `GET /__hotsheet/remote/projects/migration-jobs`                                                 | session                                                     | same, registered roots only (`clients/web/src/dev-server.ts:516`)                          | job registry                                                                                                                                               |
| `DELETE /__hotsheet/projects/:project/hs1-data`                                                  | host                                                        | `clients/web/src/dev-server.ts:517-527`                                                    | filesystem                                                                                                                                                 |
| `POST /__hotsheet/change-mux/poll`                                                               | session                                                     | `multiplexChangePolls` (`clients/web/src/dev-server.ts:530-547`)                           | each project's `/ws/poll`                                                                                                                                  |
| `ALL /__hotsheet/project-api/:project/*`                                                         | session; host for `remote:` ids                             | `proxyProjectRequest` (`clients/web/src/dev-server.ts:548-561`)                            | the session's server, checkout-scoped path                                                                                                                 |
| `POST /__hotsheet/projects/:project/corrupt-tickets/reveal`                                      | host                                                        | `clients/web/src/dev-server.ts:562-574`                                                    | server `/checkouts/{id}/corrupt-tickets`, then `open -R` or equivalent                                                                                     |
| `GET /ux-demo`                                                                                   | none                                                        | dev catalog shell (`clients/web/src/dev-server.ts:575-590`)                                | never reached in production: the production host answers `/ux-demo` first (`clients/web/src/local-host.ts:361-376`)                                        |
| `GET`, `PUT /__hotsheet/desktop-storage`; `GET`, `PUT`, `DELETE /__hotsheet/desktop-draft-files` | loopback same-origin only                                   | production host only (`clients/web/src/local-host.ts:377-379`)                             | files under the desktop storage directory                                                                                                                  |
| Server top-level routes (`/tickets`, `/ws/...`, and the rest)                                    | the caller's own session                                    | peer gateway, remote-mode production host only (`clients/web/src/peer-gateway.ts:110-137`) | this host's machine server                                                                                                                                 |
| `/assets/*`, `/`, client routes                                                                  | none                                                        | static `dist/` (`clients/web/src/local-host.ts:391-399`)                                   | nothing                                                                                                                                                    |

WebSocket upgrades: `/__hotsheet/project-api/:p/terminals/:t/attach`,
`/__hotsheet/project-api/:p/ws/sync` and `/__hotsheet/change-mux/ws`
(`clients/web/src/terminal-ws-bridge.ts:18-19`, `clients/web/src/change-ws-mux.ts:2`),
plus gateway paths in remote mode (`clients/web/src/peer-gateway.ts:150-174`).
Test: `clients/web/src/terminal-ws-bridge.test.ts`,
`clients/web/src/peer-gateway.test.ts`.

#### Request authority

- **Local mode, local project.** The bridge adds `x-hotsheet-secret` from the
  instance registration to every forwarded request
  (`clients/web/src/project-bridge.ts:2358-2360`), and for `/ws/poll`, terminal
  attach and `/ws/sync` it also puts the secret in the upstream query
  (`clients/web/src/project-bridge.ts:2467-2475`,
  `clients/web/src/project-bridge.ts:2513-2514`,
  `clients/web/src/project-bridge.ts:2555-2560`). The browser never sees it.
  Test: `clients/web/src/project-bridge.test.ts` (adds the secret only to the
  loopback upstream and escapes terminal identity).
- **Remote mode (another device's browser).** The bridge removes any
  presented secret and forwards the browser's own session cookie, copying the
  `hs2_remote_csrf` cookie into `x-hotsheet-csrf` for writes; it refuses a
  session target on any server other than the bootstrap server
  (`clients/web/src/project-bridge.ts:2349-2357`). WebSocket upgrades forward
  the cookie and Origin after the same-origin and `/remote-auth/me` checks
  (`clients/web/src/terminal-ws-bridge.ts:446-476`). The machine server
  re-checks every grant. Test: `clients/web/src/remote-auth-server-integration.test.ts`
  (requires host approval, forwards only the session, and revokes both host
  paths).
- **Remote project (a paired peer server).** The private session from the
  CLI's store is sent as `hs2_remote_session`, with the paired Origin and CSRF
  for writes and the stored CA and client certificate for TLS
  (`clients/web/src/project-bridge.ts:2267-2281`,
  `clients/web/src/project-bridge.ts:2522-2548`). The binding is re-read from
  `remotes.json` before each forward and each socket. On a remote-mode host,
  remote tabs need the local grant, because they ride the host owner's peer
  credentials (`clients/web/src/dev-server.ts:70-73`). Test:
  `clients/web/src/terminal-ws-bridge-remote-grant.test.ts`.

### Durable state and file formats

| Record                         | Location                                                                          | Writer                                                                                                                                       | Crash behavior                                                                                            |
| ------------------------------ | --------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Client discovery `client.json` | `<HOTSHEET_HOME>/client.json`                                                     | each host, atomic rename, removed only by matching id (`clients/web/src/client-discovery.ts:25-45`)                                          | a crashed host leaves a stale record naming a dead pid; the next host overwrites it                       |
| Desktop ready file             | OS temp, named by the shell                                                       | production host, 0600 (`clients/web/src/local-host.ts:489-493`); deleted by the shell after reading (`clients/web/src-tauri/src/lib.rs:189`) | the shell deletes it on startup failure too                                                               |
| Host snapshot                  | `<tmp>/hotsheet-local-host-*`                                                     | production host (`clients/web/src/local-host.ts:225-302`)                                                                                    | a killed host leaves the directory behind; nothing prunes it (Test: untested)                             |
| Stable hook CLI                | `<HOTSHEET_HOME>/bin/hotsheet-cli`, `.source-sha256`                              | production host (`clients/web/src/local-host.ts:270-293`)                                                                                    | staged in a temporary directory and renamed                                                               |
| Bootstrap store                | `<HOTSHEET_HOME>/server-bootstrap.hs2`                                            | `hotsheet-cli init` on first use (`clients/web/src/project-bridge.ts:939-943`)                                                               | owned by the CLI                                                                                          |
| Server stderr log              | `<HOTSHEET_HOME>/logs/server.log` (+ `.old`)                                      | the launched server through an inherited descriptor (`clients/web/src/project-bridge.ts:1420-1446`)                                          | rotated only at the next launch; falls back to discarding stderr                                          |
| Dev latency log                | `HOTSHEET_DEV_LATENCY_LOG` or `<HOTSHEET_HOME>/logs/dev-latency.jsonl`            | route table (`clients/web/src/dev-latency-log.ts:21-31`)                                                                                     | append only                                                                                               |
| Migration checkpoints          | `<HOTSHEET_MIGRATION_JOBS or HOTSHEET_HOME/migration-jobs>/<sha256>.json`         | job registry (`clients/web/src/migration-jobs.ts:94-124`)                                                                                    | a `running` job with a dead owner reads back as `interrupted` (`clients/web/src/migration-jobs.ts:79-84`) |
| Desktop preferences and drafts | `<app data>/browser-preferences-v1/` (`clients/web/src-tauri/src/lib.rs:233-240`) | desktop storage bridge (`clients/web/src/desktop-storage-bridge.ts:76-197`)                                                                  | atomic rename; a reader sees the whole old or new record                                                  |
| Conversation export bundles    | user-chosen folder, `*.hotsheet-chat`                                             | conversation export bridge                                                                                                                   | atomic text writes                                                                                        |

The bridge reads, but never writes, `<HOTSHEET_HOME>/instances/<id>.json` and
`.lock` (`clients/web/src/project-bridge.ts:1174-1202`) and
`<HOTSHEET_HOME>/remotes.json` (`clients/web/src/remote-projects/host.ts:193-213`).
All project sessions, pending pairings and health verifications are in
memory and are lost when the Node process exits.

### Concurrency, tasks and locks

- **One event loop.** There are no worker threads in the host. Shared state is
  kept on `process` (`__hotsheetProjectSessions`,
  `__hotsheetRemoteSessionListeners`, `__hotsheetPendingHookNotices`,
  `__hotsheetMigrationJobs`) so Vite's two module graphs share it
  (`clients/web/src/project-bridge.ts:138-155`,
  `clients/web/src/migration-job-bridge.ts:9-16`).
- **Detached children.** `hotsheet-server` is spawned detached and unref'd
  (`clients/web/src/project-bridge.ts:1464-1483`); the bridge never stops it.
  Short-lived children: `hotsheet-cli` (compatibility, setup, init, bootstrap,
  link, remote), `git`, the folder chooser, `hotsheet-migrate
--hold-job-lock` (lives as long as the job lock,
  `clients/web/src/migration-lock.ts:34-71`) and migration commands behind a
  Node gate process (`clients/web/src/migration-stream.ts:80-99`). Every spawn
  goes through `child-process.ts`, which removes repository-locating `GIT_*`
  variables (`clients/web/src/child-process.ts:1-56`). Test:
  `clients/web/src/child-process.test.ts`.
- **Timers.** Supervisor backoff (100 ms to 1 s), a 3 s first and 750 ms retry
  health-probe timeout, a 10 s reuse window
  (`clients/web/src/project-bridge.ts:1344-1376`); change-mux 50 ms grace
  (`clients/web/src/change-mux-bridge.ts:20`); migration watch 25 s idle and
  the 250 ms local re-read fallback; 3 s shutdown deadline
  (`clients/web/src/local-host.ts:509`). No timer polls the network on a fixed
  interval.
- **Locks.** The bridge takes no lock of its own around the machine server;
  the server's writer lock arbitrates concurrent launches from several hosts
  (`clients/web/src/project-bridge.ts:1256-1258`). Desktop storage serializes
  writes per key with promise queues; migration uses the native lock above.
- **Tauri threads.** The setup closure blocks while it waits up to 10 s for
  the relay install and 30 s for the ready file; each close request starts one
  sleeper thread for its 30 s deadline (`clients/web/src-tauri/src/lib.rs:100-118`);
  `stop_bridge` polls the child for 5 s on exit
  (`clients/web/src-tauri/src/lib.rs:200-214`).

### Failure and recovery

- **The Rust server dies.** The next proxied request fails at transport level;
  the bridge forgets the verified instance, re-supervises (launching a
  replacement if no registration remains), updates the URL and secret of every
  session on that store, retries a GET or HEAD once, and answers any other
  method with 503 "completion is unknown"
  (`clients/web/src/project-bridge.ts:2386-2404`,
  `clients/web/src/project-bridge.ts:2438-2446`). WebSocket upstream close
  closes the browser socket; the browser reconnects, and resolving a terminal
  or sync URL re-supervises first (`clients/web/src/project-bridge.ts:2478-2519`).
  Test: the supervision half by `clients/web/src/project-bridge.test.ts`
  (restarts after a crash but refuses to duplicate a live unhealthy process);
  the retry and 503 are untested.
- **The server is registered but unhealthy.** After 20 s the open fails with a
  `recovery` payload; only the explicit recovery route signals it, after
  matching pid, URL and start time (`clients/web/src/project-bridge.ts:1513-1555`).
  Test: `clients/web/src/project-bridge.test.ts` (recovers only the expected
  unhealthy instance and escalates when it ignores graceful termination).
- **The Node bridge exits normally.** SIGINT, SIGTERM or, in the desktop,
  stdin EOF close the listener and all connections, remove `client.json` and
  the snapshot, and exit within 3 s; the server keeps running
  (`clients/web/src/local-host.ts:496-520`). Test:
  `clients/web/scripts/desktop-host-smoke.mjs`.
- **The Node bridge dies unexpectedly.** The Tauri shell never checks the child
  after startup, so it is not restarted and the WebView keeps the dead
  ephemeral origin (`clients/web/src-tauri/src/lib.rs:249-250`,
  `clients/web/src-tauri/src/lib.rs:340-344`). The snapshot directory is left
  behind, and `client.json` names a dead pid until another host replaces it.
  Running servers, terminals and AI work are unaffected. Relaunching the app
  starts a new bridge on a new port; desktop storage restores preferences.
  Test: untested. What the page shows in this state is unresolved (WH-U2).
- **The bridge fails to start.** An early exit or no ready file within 30 s
  kills the child and fails Tauri setup, so no window opens
  (`clients/web/src-tauri/src/lib.rs:173-197`). Test: untested.
- **Remote approval changes.** Revoking or reviewing a binding deletes its
  sessions and closes its sockets (`clients/web/src/project-bridge.ts:161-166`,
  `clients/web/src/dev-server.ts:316-324`); a binding changed elsewhere is
  caught by the `remotes.json` check and answers 503. Test:
  `clients/web/src/project-bridge.remote-connections.test.ts`.
- **Desktop close.** A failed or stalled page flush keeps the app open
  (`clients/web/src-tauri/src/close_lifecycle.rs:37-43`,
  `clients/web/src-tauri/src/lib.rs:100-118`). Test:
  `clients/web/src-tauri/src/close_lifecycle.rs` (unit tests).

### Configuration and environment

Defaults in parentheses. "Blank" means an empty or whitespace-only value counts
as unset (`clients/web/src/machine-home.ts:14-17`).

| Variable                                                                                                          | Read by                                                                                                                          | Meaning                                                                                                         |
| ----------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `HOTSHEET_HOME` (`~/.hotsheet2`)                                                                                  | `clients/web/src/machine-home.ts:25-27`                                                                                          | machine home for instances, logs, bootstrap store, `client.json`, `remotes.json`, migration jobs                |
| `HOTSHEET_LEGACY_HOME` (`~/.hotsheet`)                                                                            | `clients/web/src/machine-home.ts:30-32`                                                                                          | Hot Sheet 1 home                                                                                                |
| `HOTSHEET_REPO_ROOT` (`<cwd>/../..`)                                                                              | `clients/web/src/repository-root.ts:14-20`                                                                                       | checkout used for debug binaries, the migrator and setup templates; the shell sets it to the resource directory |
| `HOTSHEET_SERVER_BIN`, `HOTSHEET_CLI_BIN`, `HOTSHEET_MIGRATE_BIN` (`<repo>/target/debug/...`)                     | `clients/web/src/project-bridge.ts:297-324`, `clients/web/src/local-host.ts:114-118`                                             | binary overrides; all three set skips release selection                                                         |
| `HOTSHEET_MIGRATOR` (`<repo>/migrator/src/export.mjs`)                                                            | `clients/web/src/project-bridge.ts:311-316`                                                                                      | HS1 exporter script                                                                                             |
| `HOTSHEET_MCP_BIN`, `HOTSHEET_HOOK_CLI_BIN`, `HOT_SHEET_BUILD_REVISION`, `HOTSHEET_LOCAL_HOST_SNAPSHOT`           | set by `clients/web/src/local-host.ts:169-171`, `clients/web/src/local-host.ts:186-201`, `clients/web/src/local-host.ts:289-296` | passed to the launched server through the inherited environment                                                 |
| `HOTSHEET_BROWSER_AUTH_MODE` (local)                                                                              | `clients/web/src/project-bridge.ts:2008-2010`                                                                                    | `remote` enables session authorization and the peer gateway and disables desktop storage                        |
| `HOTSHEET_DESKTOP_STORAGE_DIRECTORY` (none; shell default `<app data>/browser-preferences-v1`)                    | `clients/web/src/local-host.ts:338-340`, `clients/web/src-tauri/src/env_path.rs:13-29`                                           | desktop storage root                                                                                            |
| `HOTSHEET_CLIENT_READY_FILE` (none)                                                                               | `clients/web/src/local-host.ts:343-345`                                                                                          | file the host writes `{url, pid}` to                                                                            |
| `HOTSHEET_HOST_EXIT_ON_STDIN_EOF` (off)                                                                           | `clients/web/src/local-host.ts:517-520`                                                                                          | `1` exits when stdin closes                                                                                     |
| `HOTSHEET_FEATURES` (none)                                                                                        | `clients/web/src/host-features.ts:45-67`                                                                                         | comma list of host features; only `ux-demo` exists                                                              |
| `HOTSHEET_MIGRATION_JOBS`                                                                                         | `clients/web/src/migration-job-bridge.ts:10-13`                                                                                  | checkpoint directory                                                                                            |
| `HOTSHEET_DEV_LATENCY_LOG`                                                                                        | `clients/web/src/dev-latency-log.ts:21-26`                                                                                       | latency log file                                                                                                |
| `HOTSHEET_WEB_STABLE_DEV`, `HOTSHEET_VITE_CACHE_DIR`, `HOTSHEET_VITE_SOURCE_WATCH`, `HOTSHEET_WEB_TEST_SERVER_ID` | `clients/web/vite.config.ts`                                                                                                     | development host only                                                                                           |
| `HOTSHEET_TEST_SERVER_OWNER_PID`                                                                                  | `clients/web/src/project-bridge.ts:1471-1473`                                                                                    | test fixtures: the value `host` is replaced by the bridge's pid                                                 |
| `HOTSHEET_DESKTOP_NODE` (the running Node)                                                                        | `clients/web/scripts/desktop-node-runtime.mjs:11-13`                                                                             | packaging only                                                                                                  |

Ports: 4175 for development and plain production, 0 (ephemeral) for the
desktop. Logging: the host writes only start-up lines and errors to its own
stdout and stderr (`clients/web/src/local-host.ts:464-495`); the desktop shell
discards Node's stdout and inherits stderr (`clients/web/src-tauri/src/lib.rs:168-170`).
Server stderr goes to `logs/server.log`. Remote CLI stderr is discarded so
peer details do not leak (`clients/web/src/remote-projects/host.ts:140-141`).

### Dynamic views

#### `HS2-WebHost-DesktopColdStart`

1. The shell spawns Node with the binary paths, storage directory and ready
   file (`clients/web/src-tauri/src/lib.rs:141-171`). On macOS it first runs
   the relay install (`clients/web/src-tauri/src/lib.rs:244-248`).
2. The host selects no binaries (all are supplied), snapshots, then installs
   the WebSocket bridge and gateway (`clients/web/src/local-host.ts:411-417`).
3. It publishes `client.json`, then writes the ready file
   (`clients/web/src/local-host.ts:426-431`, `clients/web/src/local-host.ts:489-493`).
4. The shell reads the URL, adds the origin-scoped capability and opens the
   WebView (`clients/web/src-tauri/src/lib.rs:249-310`).
5. The page lists checkouts and opens a project; the bridge resolves the
   root and store and runs `setup --refresh` (`clients/web/src/project-bridge.ts:1754-1766`).
6. It ensures the bootstrap server; with no registration it launches one
   detached and waits for registration and `/health`
   (`clients/web/src/project-bridge.ts:1264-1315`).
7. It checks `/compatibility` and posts `/projects/open`
   (`clients/web/src/project-bridge.ts:1822-1869`).

Failure: Node exits early or no ready file in 30 s, so setup fails. A server
that exits before registering fails the open with its exit status and log path.
Test: `clients/web/scripts/desktop-host-smoke.mjs` (steps 1 to 4 without
Tauri), `clients/web/src/project-bridge.test.ts` (machine server supervision).

#### `HS2-WebHost-ProxiedCheckoutRequest`

The route strips `/__hotsheet/project-api/<id>` and keeps the query
(`clients/web/src/dev-server.ts:548-561`); the proxy looks up the session,
sets authority, rewrites settings paths under `/checkouts/<id>`, forwards with
`redirect: manual`, and refreshes store membership after a source mutation
(`clients/web/src/project-bridge.ts:2349-2385`). Failure path as above. Test:
`clients/web/src/project-bridge.test.ts` (projectScopedServerPath); the retry
is untested.

#### `HS2-WebHost-TerminalWebSocket`

Upgrade matching, optional remote-mode checks, target resolution with
re-supervision, replay parameter pass-through and frame piping are at
`clients/web/src/terminal-ws-bridge.ts:421-544`. Test:
`clients/web/src/terminal-ws-bridge.test.ts` (awaits asynchronous supervision
before resolving a reconnect target; refuses an unresolvable project socket
at once).

#### `HS2-WebHost-RemoteProjectAccess`

Open discovers the checkout, reads the private connection, rejects a binding
whose revision or server id changed mid-open, records a `remote:` session and
reads the peer's `/compatibility` through the proxy
(`clients/web/src/project-bridge.ts:1769-1815`); the route then records the
selection (`clients/web/src/dev-server.ts:349-359`). Later requests take the
remote branch of the proxy (`clients/web/src/project-bridge.ts:2260-2347`).
Test: `clients/web/src/remote-project-parity.test.ts`,
`clients/web/src/project-bridge.remote-connections.test.ts`.

#### `HS2-WebHost-ServerCrashRecovery`

Narrated under "The Rust server dies" above.

#### `HS2-WebHost-BridgeExit`

On `RunEvent::Exit` the shell drops stdin, the host's stdin `end` handler runs
`stop`, and the shell kills the child if it is still running after 5 s
(`clients/web/src-tauri/src/lib.rs:200-214`, `clients/web/src/local-host.ts:496-520`).
An unexpected bridge exit has no step: nothing observes it.

### Discrepancies

Each discrepancy names the follow-up ticket filed from HS2-T6SD67. Untested retry paths and unbounded pre-open socket queues are tracked in HS2-FNBDTJ.

1. **Dev allow-list omits three bridge routes.** `devServerRouteExclude` lets
   only listed prefixes reach Hono (`clients/web/src/dev-server-routes.ts:2-3`);
   `/__hotsheet/change-mux/poll` and `/__hotsheet/remote/projects/{open,migration-jobs}`
   (`clients/web/src/dev-server.ts:268`, `clients/web/src/dev-server.ts:516`,
   `clients/web/src/dev-server.ts:530`) are not listed, so in the Vite host
   `@hono/vite-dev-server` passes them to Vite. The production host serves
   them. Test: `clients/web/src/dev-server-routes.test.ts` does not cover them.
   Runtime effect unresolved (WH-U1). Follow-up: HS2-HPFVMH.
2. **"Development-only" timing is in production.** `traceDevBridgeRequest`
   says the production host never calls it (`clients/web/src/dev-request-timing.ts:3`)
   and `proxyProjectRequest` says its timing hook is absent in production
   (`clients/web/src/project-bridge.ts:2255`), and `docs/06-clients.md:885`
   and `:896` describe the timing headers and `/__hotsheet/dev/latency` as
   development behavior. The production host mounts `createDevApp(true)`
   (`clients/web/src/local-host.ts:355`), so both are served there. Follow-up: HS2-EA1RJC.
3. **Stale Tauri statements in `docs/06-clients.md`.** Lines 725-726 call the
   Tauri host "future" and say it never connects to Vite; it exists, and a
   debug build loads the dev server (`clients/web/src-tauri/src/lib.rs:225-226`,
   `clients/web/src-tauri/tauri.conf.json:8-9`). Lines 1701 and 1725 call the
   supervisor the "Vite-only bridge" and say Tauri will replace it with a
   native lifecycle layer; the packaged desktop runs the same Node bridge. Follow-up: HS2-2FNBGH.
4. **Server start is lazy, not at desktop launch.** `docs/06-clients.md:181-184`
   says a desktop client resolves or spawns the local server on launch. The
   bridge does so on the first request that needs it (checkout listing,
   project open, remote-auth), not when the app starts
   (`clients/web/src/project-bridge.ts:1500-1507`). Follow-up: HS2-2FNBGH.
5. **No supervision of the desktop's Node bridge.** `docs/tauri-desktop.md:5-8`
   describes startup and shutdown but not bridge death; the shell neither
   detects nor restarts it (failure section above). Follow-up: HS2-37Y34R.
6. **The Node/browser split is unenforced.** Both type sets are enabled for
   every module (`clients/web/tsconfig.json:13`) and no policy test pins the
   classification in the coverage file. Follow-up: HS2-Y9QYFC.

Unresolved claims for this area are in the [register](../unresolved.md) (WH-U1, WH-U2, WH-U3).
