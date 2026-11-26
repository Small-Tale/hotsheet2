# Node web host and Tauri desktop shell: coverage

Status: written by HS2-T6SD67 at audited source `8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`. Narrative in [50-web-host](../narrative/50-web-host.md).
`node scripts/architecture-coverage.mjs inventory --area web` lists the
required entries; every `node` and `both` one is mapped below in backticks.
Components follow the README depth standard; each row names its modules,
the seven fields, and tests.

## Production local host (`hostProductionHost`)

- Modules: `clients/web/src/local-host.ts`, `clients/web/src/client-build-host.ts`, `clients/web/src/client-discovery.ts`, `clients/web/src/dev-signing.ts`, `clients/web/src/host-features.ts` (both), `clients/web/src/client-build.ts` (both).
- Responsibility: the Node entry for `npm run prod` and the packaged desktop. It builds one Hono app: the `/ux-demo` gate, then desktop storage, then the bridge route table, then the peer gateway, then HTML auth-mode stamping, then static `dist/` with an `index.html` fallback (`clients/web/src/local-host.ts:353-401`). It never loads Vite. It does not own projects or the Rust server; those belong to the route table and supervisor. Test: `clients/web/src/local-host.test.ts` (routes the bridge, immutable assets, other static files, and client routes to the document).
- State and persistence: a private temporary snapshot `hotsheet-local-host-*` holding a copy of `dist/` and the server, CLI and migrator binaries (`clients/web/src/local-host.ts:225-302`); a stable hook CLI copy at `<HOTSHEET_HOME>/bin/hotsheet-cli` plus `hotsheet-cli.source-sha256`, rewritten only when the source digest changes (`clients/web/src/local-host.ts:270-293`); the discovery record `<HOTSHEET_HOME>/client.json`, written atomically and removed only by its own id (`clients/web/src/client-discovery.ts:25-45`); the ready file named by `HOTSHEET_CLIENT_READY_FILE`, mode 0600 (`clients/web/src/local-host.ts:489-493`). On dispose the snapshot is removed, except that when a stable hook CLI exists the pinned CLI is replaced by a symlink alias so open Codex sessions keep a working hook path (`clients/web/src/local-host.ts:234-256`). Test: `clients/web/src/local-host.test.ts` (keeps an old Codex hook command usable through a host restart; keeps an unchanged stable hook CLI file across host starts).
- Types and entry points: `createLocalHostApp`, `startLocalHost`, `releaseBinaryEnvironment`, `mcpBinaryEnvironment`, `snapshotLocalHostArtifacts`, `stampBrowserAuthMode`, and the `import.meta.url === argv[1]` main block (`clients/web/src/local-host.ts:446-521`). Arguments: `--host` (default `127.0.0.1`), `--port` (default 4175, `clients/web/src/local-host.ts:53`), repeatable `--feature`.
- Concurrency: single Node event loop. `startLocalHost` installs two `upgrade` listeners on one `node:http` server, the project WebSocket bridge and the peer gateway (`clients/web/src/local-host.ts:416-417`).
- Errors and recovery: a non-loopback `--host` without remote auth mode throws before listening (`clients/web/src/local-host.ts:409-410`); no current binary throws a rebuild instruction (`clients/web/src/local-host.ts:164-167`); a snapshot failure disposes the partial snapshot (`clients/web/src/local-host.ts:298-301`); a discovery publish failure closes the server (`clients/web/src/local-host.ts:432-435`). Shutdown on SIGINT, SIGTERM or stdin EOF closes the listener and all connections, with a 3 s deadline before `process.exit(0)` (`clients/web/src/local-host.ts:496-520`). Test: `clients/web/scripts/desktop-host-smoke.mjs` (stdin EOF exits 0 and removes the ready file and `client.json`); revision probes: `clients/web/src/local-host.test.ts` (fails fast once the child exits).
- Configuration and environment: `HOTSHEET_SERVER_BIN`, `HOTSHEET_CLI_BIN`, `HOTSHEET_MIGRATE_BIN` (all three set skips release selection, `clients/web/src/local-host.ts:454-456`), `HOTSHEET_MCP_BIN`, `HOTSHEET_HOOK_CLI_BIN` (written by the snapshot), `HOT_SHEET_BUILD_REVISION` (written when a server is selected, `clients/web/src/local-host.ts:169-171`), `HOTSHEET_LOCAL_HOST_SNAPSHOT=1` (written, `clients/web/src/local-host.ts:296`), `HOTSHEET_DESKTOP_STORAGE_DIRECTORY`, `HOTSHEET_CLIENT_READY_FILE`, `HOTSHEET_HOST_EXIT_ON_STDIN_EOF`, `HOTSHEET_FEATURES`, `HOTSHEET_BROWSER_AUTH_MODE`, `HOTSHEET_HOME`. A machine-home `dev-signing-identity` file makes the snapshot re-sign ad-hoc-signed server and CLI copies with `codesign` (`clients/web/src/dev-signing.ts:9-47`). Test: `clients/web/src/local-host.test.ts` (HS2-FYXRRM signs the copied server and both CLI copies).
- Invariants: every HTML 200 response is `Cache-Control: no-cache`; `/assets/*` is immutable for a year (`clients/web/src/local-host.ts:391-399`); the served `hotsheet-browser-auth` meta always reflects this host's mode, not the build (`clients/web/src/local-host.ts:325-330`); the client build id is read once from the pinned `index.html` (`clients/web/src/client-build-host.ts:11-17`). Test: `clients/web/src/local-host.test.ts` (stamps the running host mode into the same built document; announces the pinned dist build).
- Tests: `clients/web/src/local-host.test.ts`, `clients/web/src/client-discovery.test.ts`, `clients/web/src/host-features.test.ts`, `clients/web/src/client-build.test.ts`, `clients/web/src/dev-signing.test.ts`, `clients/web/scripts/local-host.e2e.test.mjs`, `clients/web/scripts/ux-demo-feature.e2e.test.mjs`, `clients/web/scripts/desktop-host-smoke.mjs`. `clients/web/src/client-build-host.ts` has no sibling test; it is exercised through `clients/web/src/local-host.test.ts` (announces the pinned dist build).

## Vite development host (`hostViteDev`)

- Modules: `clients/web/vite.config.ts` (outside `src`), `clients/web/src/dev-server-routes.ts`, `clients/web/src/browser-dependency-scan.ts`, `clients/web/src/vite-module-warmup.ts`. The launcher `clients/web/scripts/stable-dev.mjs` (`npm run dev`) copies the web package to a per-user cache snapshot and runs Vite there with `HOTSHEET_WEB_STABLE_DEV=1` and `HOTSHEET_REPO_ROOT`; `npm run dev:hot` runs Vite in place (`clients/web/package.json:6-7`).
- Responsibility: serves the browser client from source on `127.0.0.1:4175` with `strictPort` (`clients/web/vite.config.ts:283`) and mounts the same route table as production through `@hono/vite-dev-server` with entry `src/dev-server.ts` (`clients/web/vite.config.ts:271-275`). Only paths outside `devServerRouteExclude` reach Hono (`clients/web/src/dev-server-routes.ts:2-3`); everything else is Vite's. It installs the project WebSocket bridge on Vite's HTTP server (`clients/web/vite.config.ts:259-270`) but neither the peer gateway nor desktop storage. Test: `clients/web/src/dev-server-routes.test.ts`.
- State and persistence: `client.json` through the same discovery publisher (`clients/web/vite.config.ts:166-187`); Vite's dependency cache (`HOTSHEET_VITE_CACHE_DIR`).
- Types and entry points: plugins `testServerIdentityPlugin`, `sourceWatchPlugin`, `clientDiscoveryPlugin`, `hotsheet-project-websocket-bridge`, `stableDevClientStripPlugin`, `browserAuthModePlugin`, `clientBuildIdPlugin` (`clients/web/vite.config.ts:249-281`).
- Concurrency: Vite evaluates the config plugins and the SSR dev entry in separate module graphs; project sessions are kept on the shared `process` object so both resolve the same registry (`clients/web/src/project-bridge.ts:144-151`). Test: `clients/web/src/project-bridge.test.ts` (projectSessionRegistry).
- Errors and recovery: a non-loopback Vite host without remote auth mode throws at server configuration (`clients/web/vite.config.ts:261-268`). In stable dev, HMR and the Vite client script are removed so a dropped HMR socket cannot reload pages (`clients/web/vite.config.ts:41-43`, `clients/web/vite.config.ts:74-76`).
- Configuration and environment: `HOTSHEET_WEB_STABLE_DEV`, `HOTSHEET_VITE_CACHE_DIR`, `HOTSHEET_VITE_SOURCE_WATCH=off`, `HOTSHEET_WEB_TEST_SERVER_ID`, `HOTSHEET_BROWSER_AUTH_MODE`.
- Invariants: the dev host's client build id is one id per server process (`clients/web/vite.config.ts:199-202`).
- Tests: `clients/web/src/dev-server-routes.test.ts`, `clients/web/src/browser-dependency-scan.test.ts`, `clients/web/src/vite-module-warmup.test.ts`, `clients/web/scripts/stable-dev.e2e.test.mjs`, `clients/web/scripts/playwright-server-identity.e2e.test.mjs`.

## Bridge route table (`hostRoutes`)

- Modules: `clients/web/src/dev-server.ts`, `clients/web/src/dev-latency-log.ts`, `clients/web/src/dev-request-timing.ts`.
- Responsibility: `createDevApp` registers every `/__hotsheet` HTTP route (`clients/web/src/dev-server.ts:124-592`). Both hosts call it with `dev = true` (`clients/web/src/dev-server.ts:596`, `clients/web/src/local-host.ts:355`), so in practice no route is dev-gated; `dev = false` only appears in tests, where it turns the project, folder, setup, migration, export, change-mux and project-api routes into 404. The route list and what each does is in the chapter's route table.
- State and persistence: the dev latency log, an append-only JSON-lines file at `HOTSHEET_DEV_LATENCY_LOG` or `<HOTSHEET_HOME>/logs/dev-latency.jsonl`, with the last 200 reports in memory and at most 50 per POST (`clients/web/src/dev-latency-log.ts:21-31`). The conversation export bridge keeps destination selection tokens in memory.
- Types and entry points: `createDevApp`, `remoteRouteKind`, `mayUseRemoteProjects`, `remoteAuthOpenPath`, `changePollSessionHeaders`; default export `createDevApp(true)` for Vite.
- Concurrency: none beyond the event loop; long-running routes (change-mux poll, migration status watch) hold the request until their upstream or watcher finishes and abort on the request signal (`clients/web/src/dev-server.ts:509`, `clients/web/src/dev-server.ts:546`).
- Errors and recovery: each route maps exceptions to a JSON `{error}` with a fixed status (400, 403, 409, 502 or 503); a project open failure also returns `recovery` when the server was registered but unhealthy (`clients/web/src/dev-server.ts:383-391`). `traceDevBridgeRequest` adds `Server-Timing` bridge stages and an `x-hotsheet-trace-id` to every project API answer (`clients/web/src/dev-request-timing.ts:4-32`), in both hosts.
- Configuration and environment: `HOTSHEET_DEV_LATENCY_LOG`, `HOTSHEET_HOME`; remote mode from `HOTSHEET_BROWSER_AUTH_MODE`.
- Invariants: a remote browser can open only a checkout the host server already lists (`clients/web/src/dev-server.ts:266-282`); remote migration status is limited to registered roots (`clients/web/src/dev-server.ts:505-507`).
- Tests: `clients/web/src/dev-latency-log.test.ts`, `clients/web/src/dev-request-timing.test.ts`, `clients/web/src/remote-auth-bridge.test.ts`, `clients/web/src/checkouts-server-integration.test.ts`, `clients/web/src/migration-server-integration.test.ts`. `clients/web/src/dev-server.ts` has no sibling test file; its routes are exercised through those.

## Remote browser authorization (`hostBrowserAuth`)

- Modules: `clients/web/src/dev-server.ts` (middleware), `clients/web/src/project-bridge.ts` (`remoteBrowserAuthRequired`, `remoteCsrfCookie`, `browserAuthRequest`), `clients/web/src/client-origin.ts` (both; the browser's copy of its grants), `clients/web/src/host-only-actions.ts` (both; host-only action messages).
- Responsibility: only when `HOTSHEET_BROWSER_AUTH_MODE=remote` (`clients/web/src/project-bridge.ts:2008-2010`). Each `/__hotsheet/*` request except the four open remote-auth paths is classified as session, management or host (`clients/web/src/dev-server.ts:68-82`, `clients/web/src/dev-server.ts:95-102`), checked against `/remote-auth/me` on the machine server, and refused unless the session holds the needed grant; writes also need an Origin equal to the device's paired origin and an `hs2_remote_csrf` cookie (`clients/web/src/dev-server.ts:157-204`). Host routes answer 404, not 401, to a session without the local grant. Trust never comes from the Host header or peer address (`clients/web/src/dev-server.ts:161-163`). Test: `clients/web/src/remote-auth-bridge.test.ts` (requires a remote session and keeps the local open route off the remote allowlist; refuses a write whose origin, CSRF cookie, or paired origin is missing or different).
- State and persistence: none in the bridge; a `WeakSet` marks requests whose session carried the local grant (`clients/web/src/dev-server.ts:85`).
- Types and entry points: middleware on `/__hotsheet/*`; `browserAuthRequest(path, request, owner)` (`clients/web/src/project-bridge.ts:2031-2052`).
- Concurrency: none.
- Errors and recovery: an unreachable machine server is a 503 `host_unavailable`, so a restarting host is not mistaken for a lost approval; a 401 or 403 keeps the server's `session_expired` or `session_revoked` code (`clients/web/src/dev-server.ts:169-188`). Test: `clients/web/src/remote-auth-bridge.test.ts`.
- Configuration and environment: `HOTSHEET_BROWSER_AUTH_MODE`.
- Invariants: the loopback machine secret is never forwarded in remote mode; in local mode it is added only to `/remote-auth/owner*` routes (`clients/web/src/project-bridge.ts:2034-2042`).
- Tests: `clients/web/src/remote-auth-bridge.test.ts`, `clients/web/src/remote-auth-server-integration.test.ts`, `clients/web/src/client-origin.test.ts`, `clients/web/src/host-only-actions.test.ts`.

## Project sessions and opener (`hostSessions`)

- Modules: `clients/web/src/project-bridge.ts` (open, setup refresh, registry), `clients/web/src/project-relocation.ts`, `clients/web/src/codex-hook-compat.ts`, `clients/web/src/compatibility.ts` (both), `clients/web/src/registered-projects-model.ts` (both).
- Responsibility: owns the browser-id to target map. `openLocalProject` follows a relocated root, resolves the linked or sibling `.hs2` store, refreshes AI-tool setup through `hotsheet-cli setup --refresh` (a refusal becomes a warning, not a failure), ensures the bootstrap server, negotiates compatibility and restarts or reconciles the server build when needed, then posts `/projects/open` and records the session (`clients/web/src/project-bridge.ts:1754-1904`). `openRegisteredProject` (remote browsers) opens only listed checkouts and skips setup refresh (`clients/web/src/project-bridge.ts:1916-1936`). `openReviewedRemoteProject` creates a remote session without touching local files or the local server (`clients/web/src/project-bridge.ts:1769-1815`). It does not hold the server lifecycle; that is the supervisor.
- State and persistence: `process.__hotsheetProjectSessions` (`SessionTarget` per project: URL, secret, root, server store, ticket store, store ids, or a remote binding with private credentials) and `process.__hotsheetPendingHookNotices` (`clients/web/src/project-bridge.ts:138-151`, `clients/web/src/project-bridge.ts:544-563`). Memory only; a restarted bridge has no sessions until the browser reopens its projects. Store ids are SHA-256 prefixes of canonical store paths, refreshed after a source mutation (`clients/web/src/project-bridge.ts:2054-2087`).
- Types and entry points: `ProjectSession`, `SessionTarget`, `openLocalProject`, `openRegisteredProject`, `openReviewedRemoteProject`, `listServerCheckouts`, `listRegisteredProjects`, `removeRegisteredProject`, `refreshLocalProjectSetup`, `requireCompatibleServer`, `requireStoreSchemaCompatibility`, `closeRemoteServerSessions`, `revealCorruptTicket`, `chooseLocalFolder`.
- Concurrency: concurrent opens of the same root race only on the map entry; the server's writer lock is the final arbiter (`clients/web/src/project-bridge.ts:1256-1258`).
- Errors and recovery: a stale setup CLI is reported as a warning and the project still opens (`clients/web/src/project-bridge.ts:565-577`); a pinned host whose templates changed skips the refresh silently (`clients/web/src/project-bridge.ts:587-592`); hard protocol skew throws (`clients/web/src/project-bridge.ts:231-245`). Folder chooser and reveal run `osascript`, PowerShell or `zenity`, and `open -R`, `explorer.exe` or `xdg-open` (`clients/web/src/project-bridge.ts:169-206`).
- Configuration and environment: `HOTSHEET_CLI_BIN`, `HOTSHEET_HOOK_CLI_BIN`, `HOTSHEET_REPO_ROOT`, `HOT_SHEET_BUILD_REVISION`, `HOTSHEET_LOCAL_HOST_SNAPSHOT`, `HOTSHEET_WEB_STABLE_DEV`. Setup templates are fingerprinted from `<repo root>/plugins` (`clients/web/src/project-bridge.ts:425-446`); the desktop package copies `plugins/` into its resources for this reason (`clients/web/scripts/prepare-desktop.mjs:37-39`).
- Invariants: credentials never leave this map toward the browser; a project's API path is always `/__hotsheet/project-api/<id>` (`clients/web/src/project-bridge.ts:1887`).
- Tests: `clients/web/src/project-bridge.test.ts` (projectSessionRegistry; project setup compatibility; opens a project even when its setup refresh is refused), `clients/web/src/project-relocation.test.ts`, `clients/web/src/codex-hook-compat.test.ts`, `clients/web/src/compatibility.test.ts`, `clients/web/src/registered-projects-model.test.ts`, `clients/web/src/remote-auth-bridge.test.ts` (opens a registered checkout by root or id).

## Machine server supervisor (`hostSupervisor`)

- Modules: `clients/web/src/project-bridge.ts` (supervision), `clients/web/src/child-process.ts`, `clients/web/src/repository-root.ts`, `clients/web/src/machine-home.ts`.
- Responsibility: one bootstrap machine server per machine home. The bootstrap store is `<HOTSHEET_HOME>/server-bootstrap.hs2`, created with `hotsheet-cli init` on first use (`clients/web/src/project-bridge.ts:939-943`). Discovery reads `instances/<sha256(store)[0..16]>.json` and checks the pid is alive (`clients/web/src/project-bridge.ts:1174-1189`); a live `.lock` holder counts as starting (`clients/web/src/project-bridge.ts:1191-1202`). `superviseServer` probes, launches only when no registration exists, and waits with 100 ms to 1 s backoff (`clients/web/src/project-bridge.ts:1264-1315`, `clients/web/src/project-bridge.ts:1496-1498`). Launch is `hotsheet-server -C <store> --bind 127.0.0.1:0 --launcher <label>`, detached and unref'd (`clients/web/src/project-bridge.ts:1464-1483`). It never stops the server on bridge exit.
- State and persistence: `verifiedServerInstances` (health verification reused for 10 s) and `serverBuildRestartAttempts` (restart cooldown) in memory (`clients/web/src/project-bridge.ts:1330`, `clients/web/src/project-bridge.ts:1620`); the server's stderr goes to `<HOTSHEET_HOME>/logs/server.log`, rotated once to `server.log.old` above 8 MiB at the next launch (`clients/web/src/project-bridge.ts:1420-1446`).
- Types and entry points: `ensureServer`, `superviseServer`, `createServerHealthProbe`, `recoverUnhealthyServer`, `safelyRestartServer`, `reconcileSelectedServerBuild`, `serverLauncherLabel`, `mayReplaceServer`, `serverBinary`, `toolBinary`, `migrateBinary`, `migratorScript`.
- Concurrency: async only; a timer per wait. Concurrent hosts may both launch; the server's writer lock decides (`clients/web/src/project-bridge.ts:1256-1258`).
- Errors and recovery: a launched server that exits before registering ends the wait with its exit status and log path; otherwise 90 s for a launched or starting server and 20 s for a registered one that stays unhealthy, which raises `UnhealthyManagedServerError` and requires the explicit recovery route (`clients/web/src/project-bridge.ts:1222-1245`, `clients/web/src/project-bridge.ts:1302-1314`). Recovery re-checks pid, URL and start time before every signal (`clients/web/src/project-bridge.ts:1513-1556`). A dev host never restarts a server whose launcher is `local-host-snapshot` (`clients/web/src/project-bridge.ts:1393-1395`). Test: `clients/web/src/project-bridge.test.ts` (machine server supervision; pinned server ownership (HS2-9AWEX5)).
- Configuration and environment: `HOTSHEET_SERVER_BIN`, `HOTSHEET_HOME`, `HOTSHEET_REPO_ROOT` (default `<cwd>/../..`, `clients/web/src/repository-root.ts:14-20`), `HOTSHEET_TEST_SERVER_OWNER_PID`. Launcher labels: `local-host-snapshot`, `stable-dev`, `dev-bridge` (`clients/web/src/project-bridge.ts:1378-1383`). Every child process drops repository-locating `GIT_*` variables (`clients/web/src/child-process.ts:1-56`). Test: `clients/web/src/child-process.test.ts`.
- Invariants: `HOTSHEET_HOME` and `HOTSHEET_LEGACY_HOME` are read only through `machine-home.ts`, and blank counts as unset (`clients/web/src/machine-home.ts:14-32`). Test: `clients/web/src/machine-home.test.ts`.
- Tests: `clients/web/src/project-bridge.test.ts` (dev bridge server health probe (HS2-TANE0V); machine server supervision; openServerLog (HS2-DSYPTN); reconcileSelectedServerBuild (HS2-46BKH7)), `clients/web/src/child-process.test.ts`, `clients/web/src/machine-home.test.ts`, `clients/web/scripts/owned-server.e2e.test.mjs`. `clients/web/src/repository-root.ts` has no sibling test; it is covered by `clients/web/src/project-bridge.test.ts` (developmentRepositoryRoot).

## Checkout request proxy (`hostProxy`)

- Modules: `clients/web/src/project-bridge.ts` (`proxyProjectRequest`, `projectScopedServerPath`, `authenticatedServerUrl`), `clients/web/src/change-mux-bridge.ts` (both).
- Responsibility: forwards a project API request to its session's server. Local sessions get `x-hotsheet-secret` in local mode, or the browser's own cookie plus a CSRF header copied from its cookie in remote mode, where a session on any server other than the bootstrap server is refused (`clients/web/src/project-bridge.ts:2342-2360`). Project-owned settings paths are rewritten under `/checkouts/<id>`, and `/ws/poll` gets `checkout=<id>` (`clients/web/src/project-bridge.ts:2409-2436`). Remote sessions are covered in the remote project bridge row. The change multiplexer forwards up to 64 polls, answers with the first completion plus any inside 50 ms, and aborts the rest (`clients/web/src/change-mux-bridge.ts:15-24`, `clients/web/src/change-mux-bridge.ts:107-179`).
- State and persistence: none.
- Types and entry points: `proxyProjectRequest`, `multiplexChangePolls`, `parseChangeMuxRequest`, `CHANGE_MUX_PATH`.
- Concurrency: each multiplexed poll is an independent fetch sharing one `AbortController` tied to the browser request.
- Errors and recovery: an unknown project is 404. A transport failure forgets the verified instance and re-supervises every session on that store; GET and HEAD are retried once, any other method returns 503 with "completion is unknown" (`clients/web/src/project-bridge.ts:2386-2404`). A cancelled multiplexed poll rethrows instead of re-supervising (`clients/web/src/project-bridge.ts:2390`). Test: untested (no test exercises the retry or the 503).
- Configuration and environment: `HOTSHEET_BROWSER_AUTH_MODE`.
- Invariants: the loopback secret appears only on the server-side hop; `/ws/poll` also carries it as a query parameter for older servers (`clients/web/src/project-bridge.ts:2467-2475`). Test: `clients/web/src/project-bridge.test.ts` (uses legacy query authentication for loopback polling without changing browser URLs; adds the secret only to the loopback upstream).
- Tests: `clients/web/src/project-bridge.test.ts` (projectScopedServerPath), `clients/web/src/change-mux-bridge.test.ts`, `clients/web/src/remote-auth-bridge.test.ts` (refuses an unauthenticated, revoked, or cross-origin multiplexed poll).

## Project WebSocket bridge (`hostStreams`)

- Modules: `clients/web/src/terminal-ws-bridge.ts`, `clients/web/src/change-ws-mux.ts` (both).
- Responsibility: handles `upgrade` for three paths only, `/__hotsheet/change-mux/ws`, `/__hotsheet/project-api/<p>/ws/sync` and `/__hotsheet/project-api/<p>/terminals/<t>/attach`; every other upgrade (Vite HMR, the peer gateway) is left to its own handler (`clients/web/src/terminal-ws-bridge.ts:18-19`, `clients/web/src/terminal-ws-bridge.ts:93-97`, `clients/web/src/terminal-ws-bridge.ts:433-435`). Terminal and sync sockets are piped frame for frame to an upstream URL resolved by the session registry, with only the five replay parameters passed through (`clients/web/src/terminal-ws-bridge.ts:58-72`, `clients/web/src/terminal-ws-bridge.ts:498-534`). The change multiplexer accepts versioned subscribe and unsubscribe commands, groups members per upstream server and client, opens one `/ws/sync` with several `checkout=` parameters, routes each event by its `store` field to matching projects, and falls back to one socket per project when an older server rejects duplicate parameters (`clients/web/src/terminal-ws-bridge.ts:99-415`). Every subscription answer carries the host's `client_build` (`clients/web/src/terminal-ws-bridge.ts:128-134`).
- State and persistence: per browser socket, subscription, pending and group maps in memory.
- Types and entry points: `installProjectWebSocketBridge`, `browserProjectWebSocketTarget`, `withTerminalReplayParams`, `hideRemoteProjects`; `ChangeWsCommand` and `ChangeWsMessage` (`clients/web/src/change-ws-mux.ts:2-45`).
- Concurrency: browser frames sent before the upstream opens are queued without a bound, then flushed (`clients/web/src/terminal-ws-bridge.ts:503-519`). Subscription resolution is guarded by a per-key token so a later command wins.
- Errors and recovery: an unresolvable target is refused with 404 immediately (`clients/web/src/terminal-ws-bridge.ts:486-491`); socket errors on the raw upgrade socket are owned so they cannot crash the host (`clients/web/src/terminal-ws-bridge.ts:436-442`). Either side closing closes the other; the bridge never reconnects an upstream, the browser does. Remote mode checks same-origin and `/remote-auth/me`, answers 401 otherwise, and hides remote projects from a session without the local grant (`clients/web/src/terminal-ws-bridge.ts:446-476`). Revoking a remote binding closes that tab's sockets (`clients/web/src/terminal-ws-bridge.ts:504-512`).
- Configuration and environment: `HOTSHEET_BROWSER_AUTH_MODE`.
- Invariants: the upstream secret never appears in a browser URL; resolving a URL re-supervises the server first (`clients/web/src/project-bridge.ts:2478-2519`).
- Tests: `clients/web/src/terminal-ws-bridge.test.ts` (refuses an unresolvable project socket at once; uses one upstream for two checkouts and routes each store to its project), `clients/web/src/terminal-ws-bridge-remote-grant.test.ts`, `clients/web/src/change-ws-mux.test.ts`.

## Remote project bridge (`hostRemoteProjects`)

- Modules: `clients/web/src/remote-projects/host.ts`, `clients/web/src/remote-projects/transport.ts`, `clients/web/src/remote-projects/connections.ts`, `clients/web/src/remote-projects/permissions.ts`, `clients/web/src/remote-projects/terminals.ts`, `clients/web/src/remote-projects/identity.ts` (both), `clients/web/src/terminal-project-scope.ts` (both); the `clients/web/src/remote-projects` directory's three browser modules belong to HS2-AYKHVV.
- Responsibility: pairing calls the peer's `/remote-auth/pair` and `/remote-auth/redeem` directly, then registers or reviews the binding through `hotsheet-cli remote register|review --credentials-file -`, passing the session cookie, Origin and CSRF on stdin (`clients/web/src/remote-projects/host.ts:233-349`). Every other registry operation is a bounded `hotsheet-cli remote` call (`clients/web/src/remote-projects/host.ts:130-189`). Forwarding re-reads `<HOTSHEET_HOME>/remotes.json` and refuses a revoked or changed binding before every request or socket (`clients/web/src/remote-projects/host.ts:193-213`). In the proxy, a remote session strips browser credentials, sets `hs2_remote_session`, adds the paired Origin and CSRF for writes, maps the composite id to the native checkout id, substitutes the remote root as a terminal launch cwd, and filters terminal, AI connection and permission lists to tabs on the same binding (`clients/web/src/project-bridge.ts:2260-2341`). Composite ids are `remote:<binding ULID>:<encoded checkout>` (`clients/web/src/remote-projects/identity.ts:3-7`).
- State and persistence: `pendingPairs` in memory, never expired except on redeem (`clients/web/src/remote-projects/host.ts:75`); private credentials in the session target only; the registry and credential store are the CLI's.
- Types and entry points: `beginRemotePairing`, `finishRemotePairing`, `privateRemoteConnection`, `requireCurrentRemoteBinding`, `remoteRequest`, `remoteProjectId`.
- Concurrency: one child process per registry call; output capped at 2 MiB (`clients/web/src/remote-projects/host.ts:76`).
- Errors and recovery: CLI stderr is discarded and errors stay generic so peer details and paths do not reach the browser (`clients/web/src/remote-projects/host.ts:140-153`); a remote forward failure is 503 "unavailable or its approval changed" (`clients/web/src/project-bridge.ts:2345-2347`); redirects are surfaced, not followed (`clients/web/src/remote-projects/transport.ts:11-13`).
- Configuration and environment: `HOTSHEET_CLI_BIN`, `HOTSHEET_HOME`; optional CA, client certificate and key PEM files, each at most 128 KiB (`clients/web/src/remote-projects/host.ts:110-126`).
- Invariants: remote tabs never touch this machine's filesystem or local server on open (`clients/web/src/project-bridge.ts:1767-1768`); a remote-mode host admits remote tabs only for a local-grant session (`clients/web/src/dev-server.ts:70-73`).
- Tests: `clients/web/src/remote-projects/host.test.ts`, `clients/web/src/remote-projects/transport.test.ts`, `clients/web/src/remote-projects/connections.test.ts`, `clients/web/src/remote-projects/permissions.test.ts`, `clients/web/src/remote-projects/terminals.test.ts`, `clients/web/src/remote-projects/identity.test.ts`, `clients/web/src/project-bridge.remote-connections.test.ts`, `clients/web/src/remote-project-parity.test.ts`, `clients/web/src/terminal-project-scope.test.ts`.

## Peer gateway (`hostPeerGateway`)

- Modules: `clients/web/src/peer-gateway.ts`.
- Responsibility: in remote mode on the production host only, a request whose first path segment is one of the server's top-level routes is relayed to this host's own machine server, so a peer that reaches this host through an HTTPS proxy can use the native protocol (`clients/web/src/peer-gateway.ts:22-82`, `clients/web/src/peer-gateway.ts:110-137`). It adds no authority: `host`, `x-hotsheet-secret`, `authorization` and hop headers are removed and a `secret` query parameter is dropped (`clients/web/src/peer-gateway.ts:85-105`). WebSockets carry only cookie, Origin, CSRF and user agent (`clients/web/src/peer-gateway.ts:140-147`). It is mounted after `/__hotsheet` and before static files (`clients/web/src/local-host.ts:381-382`).
- State and persistence: none.
- Types and entry points: `peerGateway`, `installPeerWebSocketGateway`, `PEER_GATEWAY_PREFIXES`.
- Concurrency: per socket pipe; frames before upstream open are queued (`clients/web/src/peer-gateway.ts:176-199`).
- Errors and recovery: an unreachable server is 503 (`clients/web/src/peer-gateway.ts:119-135`).
- Configuration and environment: `HOTSHEET_BROWSER_AUTH_MODE=remote`.
- Invariants: the prefix list is pinned to the server router by a policy test (`clients/web/src/peer-gateway.ts:20-21`).
- Tests: `clients/web/src/peer-gateway.test.ts`, `clients/web/src/remote-auth-server-integration.test.ts` (HS2-H4REBS pairs a peer host through the remote-mode gateway).

## Desktop storage bridge (`hostDesktopStorage`)

- Modules: `clients/web/src/desktop-storage-bridge.ts`, `clients/web/src/desktop-storage-contract.ts` (both).
- Responsibility: because the desktop bridge listens on an ephemeral port, the WebView origin changes every launch; these routes keep `hotsheet.*` preferences and staged draft attachments across launches. Mounted only when a storage directory is set and the host is not in remote mode (`clients/web/src/local-host.ts:377-379`).
- State and persistence: preferences are `<dir>/<sha256(key)>.json` holding `{key, value}`, where `null` is a tombstone; drafts are `<dir>/draft-files-v1/<sha256(key)>.draft` holding a 4-byte big-endian metadata length, metadata JSON, then raw bytes. Both are written to a `wx` temporary at 0600 and renamed; directories are 0700 (`clients/web/src/desktop-storage-bridge.ts:76-97`, `clients/web/src/desktop-storage-bridge.ts:111-190`). Limits: key 512 characters, value 5 MiB, draft 100 MiB (`clients/web/src/desktop-storage-contract.ts:3-7`, `clients/web/src/desktop-storage-contract.ts:45-49`).
- Types and entry points: `GET|PUT /__hotsheet/desktop-storage`, `GET|PUT|DELETE /__hotsheet/desktop-draft-files?key=`.
- Concurrency: one promise queue per key; a preference write is skipped when its `(clientId, key)` revision is not newer than the last one this process applied (`clients/web/src/desktop-storage-bridge.ts:76-103`). The revision map is in memory, so it starts empty after a bridge restart.
- Errors and recovery: a non-loopback Host, a foreign Origin or `Sec-Fetch-Site: cross-site` is 403; a draft PUT without a numeric `Content-Length` is 411; a truncated draft record is refused, never partly served; parse errors are never logged (`clients/web/src/desktop-storage-bridge.ts:31-58`, `clients/web/src/desktop-storage-bridge.ts:201-210`).
- Configuration and environment: `HOTSHEET_DESKTOP_STORAGE_DIRECTORY`, set by the shell to `<app data>/browser-preferences-v1` unless overridden (`clients/web/src-tauri/src/lib.rs:233-240`).
- Invariants: credentials are never stored here; only keys starting `hotsheet.` and draft keys starting `composer:` or `not-working:` are accepted (`clients/web/src/desktop-storage-contract.ts:9-11`, `clients/web/src/desktop-storage-contract.ts:50-54`).
- Tests: `clients/web/src/desktop-storage-bridge.test.ts`, `clients/web/src/desktop-draft-files-bridge.test.ts`, `clients/web/src/remote-auth-bridge.test.ts` (keeps production desktop storage out of remote mode), `clients/web/scripts/desktop-storage-smoke.mjs`. `clients/web/src/desktop-storage-contract.ts` has no sibling test; its parsers are exercised by the two bridge tests.

## Migration and Git setup jobs (`hostMigration`)

- Modules: `clients/web/src/migration-job-bridge.ts`, `clients/web/src/migration-jobs.ts`, `clients/web/src/migration-lock.ts`, `clients/web/src/migration-stream.ts`, `clients/web/src/hs1-backup.ts`, and in `clients/web/src/project-bridge.ts` `migrateHs1Project`, `prepareLocalGitTicketStore`, `connectGitTicketStoreRemote`, `runHs1MigrationJob` (`clients/web/src/project-bridge.ts:601`, `clients/web/src/project-bridge.ts:977`, `clients/web/src/project-bridge.ts:1053`, `clients/web/src/project-bridge.ts:2563-2617`).
- Responsibility: one durable job per canonical project root. A start requires the owning project session and the detected HS1 database identity (`clients/web/src/migration-job-bridge.ts:37-62`); the job then runs the HS1 import or backup push to completion even if the browser disconnects, registers the new source on the server, and links the store with `hotsheet-cli link`.
- State and persistence: checkpoints `<HOTSHEET_MIGRATION_JOBS or HOTSHEET_HOME/migration-jobs>/<sha256(root)>.json`, written to a temporary and renamed, retried up to eight times on Windows sharing errors (`clients/web/src/migration-job-bridge.ts:10-16`, `clients/web/src/migration-jobs.ts:94-124`). On load, a `running` job whose owner pid is dead becomes `interrupted` (`clients/web/src/migration-jobs.ts:79-84`).
- Types and entry points: `MigrationJobs`, `migrationJobApi`, `acquireMigrationLock`, `runMigrationProcess`.
- Concurrency: the registry lives on `process.__hotsheetMigrationJobs`; starts and saves are serialized promise chains. The cross-process lock is a `hotsheet-migrate --hold-job-lock` child that prints `locked` and holds native advisory locks until its stdin closes (`clients/web/src/migration-lock.ts:33-71`). Migration commands run behind a Node gate process that waits for the parent to checkpoint its pid (`clients/web/src/migration-stream.ts:80-99`). Status watches use a native directory watcher, falling back to a 250 ms local re-read only when the OS refuses one, and end after 25 s idle (`clients/web/src/migration-jobs.ts:22-26`, `clients/web/src/migration-jobs.ts:356-398`).
- Errors and recovery: retries resume verified import work; a retry is refused while an interrupted owner still has a live child. Test: `clients/web/src/migration-jobs.test.ts` (retains failures, rejects stale retries, and reports a stopped owner as interrupted after restart; refuses retry while an interrupted owner still has a live child).
- Configuration and environment: `HOTSHEET_MIGRATION_JOBS`, `HOTSHEET_MIGRATE_BIN`, `HOTSHEET_MIGRATOR`, `HOTSHEET_LEGACY_HOME`, `HOTSHEET_REPO_ROOT`.
- Invariants: command output is capped at 16 MiB (`clients/web/src/project-bridge.ts:2575`).
- Tests: `clients/web/src/migration-jobs.test.ts`, `clients/web/src/migration-lock.test.ts`, `clients/web/src/migration-stream.test.ts`, `clients/web/src/hs1-backup.test.ts`, `clients/web/src/migration-server-integration.test.ts`, `clients/web/src/project-bridge.test.ts` (Hot Sheet 1 project import bridge; Git ticket-store remote setup). `clients/web/src/migration-job-bridge.ts` has no sibling test; it is exercised by `clients/web/src/migration-server-integration.test.ts`.

## Conversation export bridge (`hostConversationExport`)

- Modules: `clients/web/src/conversation-export-bridge.ts`, `clients/web/src/conversation-export.ts` (both).
- Responsibility: a folder chosen through the native chooser receives a `.hotsheet-chat` bundle with a manifest, Markdown transcript and summary, and assets; the bridge can also reopen one. It does not read conversations from the server; the browser supplies the payload.
- State and persistence: selection tokens in memory (`clients/web/src/conversation-export-bridge.ts:140-160`); bundle files written atomically; assets at most 50 MiB each and 200 MiB in total (`clients/web/src/conversation-export-bridge.ts:19-22`).
- Types and entry points: `createConversationExportBridge`; routes `/__hotsheet/conversation-exports/{destination,write,open}`.
- Concurrency: none.
- Errors and recovery: invalid payloads and limits are 400.
- Configuration and environment: none.
- Invariants: writes stay inside the selected destination.
- Tests: `clients/web/src/conversation-export-bridge.test.ts`, `clients/web/src/conversation-export.test.ts`.

## Shared bridge contracts (`hostContracts`)

- Modules: `clients/web/src/local-bridge-client.ts` (both), `clients/web/src/server-response-parsers.ts` (both, 1995 lines), `clients/web/src/json-value.ts` (both), and the `both` modules listed in the rows above.
- Responsibility: code both runtimes load. `local-bridge-client.ts` is the browser's typed client for bridge routes and owns `REMOTE_PROJECT_OPEN_PATH` and `REMOTE_MIGRATION_JOBS_PATH` (`clients/web/src/local-bridge-client.ts:237-239`); the host imports only those constants. `server-response-parsers.ts` validates server JSON for both the browser API layer and the bridge's open and checkout calls. These modules must not import Node built-ins or `ws`.
- State and persistence: none.
- Types and entry points: `localBridge`, `localBridgeRequest`, the `parse*` functions.
- Concurrency: none.
- Errors and recovery: a wrongly shaped body throws instead of being cast. Test: `clients/web/src/local-bridge-client.test.ts` (rejects non-JSON and wrongly shaped bodies instead of casting them).
- Configuration and environment: none.
- Invariants: runtime-neutral imports (see the classification method above).
- Tests: `clients/web/src/local-bridge-client.test.ts`, `clients/web/src/server-response-parsers.test.ts`, `clients/web/src/json-value.test.ts`.

## Shell bootstrap and bridge launcher (`tauriShell`)

- Modules: `clients/web/src-tauri/src/lib.rs`, `clients/web/src-tauri/src/main.rs`, `clients/web/src-tauri/src/env_path.rs`, `clients/web/src-tauri/src/host_features.rs`.
- Responsibility: debug builds load `http://127.0.0.1:4175/` and start nothing (`clients/web/src-tauri/src/lib.rs:225-226`); `tauri dev` starts that server through `beforeDevCommand: npm run dev` (`clients/web/src-tauri/tauri.conf.json:8-9`). Release builds install the terminal relay, then spawn `<exe dir>/node <resources>/dist-host/local-host.mjs --host 127.0.0.1 --port 0` with forwarded `--feature` flags, the bundled binary paths, `HOTSHEET_REPO_ROOT=<resources>`, the storage directory, a ready-file path in the OS temp directory, `HOTSHEET_HOST_EXIT_ON_STDIN_EOF=1`, piped stdin, null stdout and inherited stderr (`clients/web/src-tauri/src/lib.rs:129-171`). It polls the ready file every 50 ms for up to 30 s (`clients/web/src-tauri/src/lib.rs:173-197`).
- State and persistence: the `BridgeChild` mutex holding the child; the ready file is deleted after reading.
- Types and entry points: `run`, `launch_bridge`, `stop_bridge`, `forwarded_feature_arguments`, `resolve_storage_directory`.
- Concurrency: the setup closure blocks the main thread while waiting for the ready file.
- Errors and recovery: a missing resource, an early exit or the 30 s timeout kills the child and fails setup (`clients/web/src-tauri/src/lib.rs:121-127`, `clients/web/src-tauri/src/lib.rs:181-196`). After startup the child is never polled: a bridge that dies leaves the window on a dead origin and is not restarted; `stop_bridge` runs only on `RunEvent::Exit`, closes stdin, waits 5 s and then kills (`clients/web/src-tauri/src/lib.rs:200-214`, `clients/web/src-tauri/src/lib.rs:340-344`). Test: untested (no Rust test of `launch_bridge` or `stop_bridge`; `clients/web/scripts/desktop-host-smoke.mjs` exercises the host side of the same contract).
- Configuration and environment: `HOTSHEET_DESKTOP_STORAGE_DIRECTORY` (blank is unset, `clients/web/src-tauri/src/env_path.rs:13-29`); the host also inherits the shell's whole environment, so `HOTSHEET_HOME` and `HOTSHEET_FEATURES` pass through.
- Invariants: the Rust server is never a child of the shell or the bridge lifetime.
- Tests: `clients/web/src-tauri/src/env_path.rs` (unit tests), `clients/web/src-tauri/src/host_features.rs` (unit tests), `clients/web/scripts/desktop-host-smoke.mjs`.

## Main WebView and link routing (`tauriWebview`)

- Modules: `clients/web/src-tauri/src/external_links.rs`, window construction in `clients/web/src-tauri/src/lib.rs:278-310`.
- Responsibility: one window labelled `main`, 1280 by 840, minimum 360 by 560. `window.open` and `target=_blank` for http, https and mailto go to the system opener and never open a second webview; navigations off the bridge origin to those schemes also go to the system (`clients/web/src-tauri/src/external_links.rs:27-45`). Release builds inject `window.__HOTSHEET_DESKTOP_STORAGE__ = true` (`clients/web/src-tauri/src/lib.rs:302-305`) and add a dynamic capability for the exact bridge origin granting the two custom commands, `core:window:allow-set-theme` and notification permissions (`clients/web/src-tauri/src/lib.rs:252-271`). The static capability grants only `core:default` (`clients/web/src-tauri/capabilities/default.json:6`); there is no CSP (`clients/web/src-tauri/tauri.conf.json:15`).
- State and persistence: WebKit's origin-scoped storage, which a new port does not see; the desktop storage bridge compensates.
- Types and entry points: `LinkRoute`, `new_window_route`, `navigation_route`.
- Concurrency: callbacks on Tauri's event threads.
- Errors and recovery: opener failures are ignored.
- Configuration and environment: none.
- Invariants: the WebView only ever shows the bridge origin.
- Tests: `clients/web/src-tauri/src/external_links.rs` (unit tests).

## Controlled close lifecycle (`tauriClose`)

- Modules: `clients/web/src-tauri/src/close_lifecycle.rs`, commands and handlers in `clients/web/src-tauri/src/lib.rs:30-119`, `clients/web/src-tauri/src/lib.rs:315-339`.
- Responsibility: in release builds, `CloseRequested` on `main` and `ExitRequested` are prevented until the page acknowledges a flush. The shell evaluates `window.__HOTSHEET_DESKTOP_CLOSE__(token)`; the page answers with `desktop_flush_complete(request, success)`, which is accepted only from window and webview `main` at the bridge origin and only for the pending generation (`clients/web/src-tauri/src/close_lifecycle.rs:10-44`, `clients/web/src-tauri/src/lib.rs:30-63`). Success exits the app. A 30 s thread cancels the request and calls the page's cancel hook or an alert (`clients/web/src-tauri/src/lib.rs:100-118`). `desktop_hide_cursor_until_moves` is a second command for standby (`clients/web/src-tauri/src/lib.rs:69-79`).
- State and persistence: `DesktopClose { origin, lifecycle }` in a mutex.
- Types and entry points: `CloseLifecycle::{begin, cancel, acknowledge, allowed}`.
- Concurrency: one sleeper thread per close request.
- Errors and recovery: a failed flush keeps the app open; a new close starts a new generation.
- Configuration and environment: none. Debug builds manage no `DesktopClose`, so close is not intercepted.
- Invariants: at most one pending request; stale acknowledgements are errors.
- Tests: `clients/web/src-tauri/src/close_lifecycle.rs` (unit tests).

## Terminal relay installer (`tauriRelay`)

- Modules: `clients/web/src-tauri/src/terminal_relay.rs`.
- Responsibility: on macOS release launches, runs `hotsheet-cli terminal-relay install --server <bundled hotsheet-server>` and waits at most 10 s (`clients/web/src-tauri/src/terminal_relay.rs:12-55`, `clients/web/src-tauri/src/lib.rs:244-248`).
- State and persistence: none in the shell; the CLI owns the launch agent.
- Types and entry points: `ensure_installed`.
- Concurrency: blocks setup up to 10 s before the bridge starts.
- Errors and recovery: any failure is printed to stderr and startup continues; servers then spawn terminal brokers directly.
- Configuration and environment: none.
- Invariants: never blocks startup.
- Tests: untested.

## Desktop packaging (build time, no runtime component)

- Modules: `clients/web/scripts/prepare-desktop.mjs`, `clients/web/scripts/desktop-node-runtime.mjs`, `clients/web/src-tauri/tauri.conf.json`, `clients/web/src-tauri/Cargo.toml`, `clients/web/src-tauri/build.rs`, `clients/web/src-tauri/Entitlements.plist`.
- Responsibility: `npm run desktop:build` builds the client and host, builds release Rust binaries, runs `prepare-desktop.mjs`, then `cargo build` and `tauri bundle --bundles app` (`clients/web/package.json:11-12`). The script stages `binaries/<name>-<target triple>` for Node and the four Rust binaries, and `resources/` with `dist`, `plugins`, the migrator source and production dependencies, and an esbuild ESM bundle of `local-host.ts` at `dist-host/local-host.mjs` with a `createRequire` banner for `ws` (`clients/web/scripts/prepare-desktop.mjs:30-54`). Tauri places `externalBin` entries beside the executable without the triple, which is where `lib.rs` looks for `node` and the binaries (`clients/web/src-tauri/tauri.conf.json:21-28`).
- Errors and recovery: a Node linked to non-system dylibs on macOS, or not executable on Linux and Windows, fails the build (`clients/web/scripts/desktop-node-runtime.mjs:42-69`); a missing migrator `node_modules` fails it.
- Configuration and environment: `HOTSHEET_DESKTOP_NODE` (blank is unset, `clients/web/scripts/desktop-node-runtime.mjs:11-13`); Apple signing variables per `docs/tauri-desktop.md`. The hardened runtime grants Node `com.apple.security.cs.allow-jit` (`clients/web/src-tauri/Entitlements.plist:5`).
- Tests: `clients/web/scripts/prepare-desktop.test.mjs`, `clients/web/scripts/desktop-node-runtime.test.mjs`, `clients/web/scripts/stage-migrator-dependencies.test.mjs`.

## Module runtime classification

Every non-test module under `clients/web/src` runs in exactly one of three
runtimes. HS2-AYKHVV (the browser web client) maps every `browser` module;
this file maps every `node` and `both` module.

**Method.** The classification is the transitive closure of runtime (non
`import type`) imports, static and dynamic, plus `new URL(..., import.meta.url)`
worker entries, from each runtime's entry points:

- Node entries: the production host `clients/web/src/local-host.ts` (built by
  `npm run build:host`, `vite build --ssr src/local-host.ts --outDir dist-host`,
  `clients/web/package.json:23`); the dev host `clients/web/src/dev-server.ts`
  (the `@hono/vite-dev-server` entry, `clients/web/vite.config.ts:271-275`);
  and the `src` modules that `clients/web/vite.config.ts:8-15` imports into
  the Vite config process.
- Browser entries: `clients/web/index.html:21` (`/src/main.tsx`), the
  screen-saver library entry `src/standby-saver/main.tsx`
  (`clients/web/vite.saver.config.ts:17`), the stable-dev client stub
  `src/stable-vite-client.ts` (`clients/web/vite.config.ts:102`), and the two
  change-stream workers created at
  `clients/web/src/features/project-change-streams.ts:224` and `:272`.

A module reached from both sides is `both`; it must not import Node built-ins,
`ws` or `@hono/node-server`, and at the audited revision none does (checked
by scanning every `browser` and `both` module's imports). Three modules are
reached from no entry: `src/repository-state.ts` holds only types used by
browser code (classified `browser`), and `source-format-matchers.ts` and
`test-quarantine.ts` are Vitest-only helpers (classified `node`).
`tsconfig.json` gives every module both `node` and `vite/client` types
(`clients/web/tsconfig.json:13`), so the compiler does not enforce this
split; the import graph does. Test: untested (no policy test pins the split;
see Discrepancies in the chapter).

Counts: 32 `node` (30 reachable plus 2 Vitest helpers), 15 `both`, 732
`browser`.

### Node and both modules (mapped in this file)

| Module | Runtime | Lines |
| --- | --- | ---: |
| `clients/web/src/browser-dependency-scan.ts` | node | 61 |
| `clients/web/src/child-process.ts` | node | 93 |
| `clients/web/src/client-build-host.ts` | node | 18 |
| `clients/web/src/client-discovery.ts` | node | 47 |
| `clients/web/src/codex-hook-compat.ts` | node | 69 |
| `clients/web/src/conversation-export-bridge.ts` | node | 288 |
| `clients/web/src/desktop-storage-bridge.ts` | node | 211 |
| `clients/web/src/dev-latency-log.ts` | node | 100 |
| `clients/web/src/dev-request-timing.ts` | node | 33 |
| `clients/web/src/dev-server-routes.ts` | node | 4 |
| `clients/web/src/dev-server.ts` | node | 597 |
| `clients/web/src/dev-signing.ts` | node | 48 |
| `clients/web/src/hs1-backup.ts` | node | 198 |
| `clients/web/src/local-host.ts` | node | 522 |
| `clients/web/src/machine-home.ts` | node | 33 |
| `clients/web/src/migration-job-bridge.ts` | node | 77 |
| `clients/web/src/migration-jobs.ts` | node | 399 |
| `clients/web/src/migration-lock.ts` | node | 72 |
| `clients/web/src/migration-stream.ts` | node | 171 |
| `clients/web/src/peer-gateway.ts` | node | 200 |
| `clients/web/src/project-bridge.ts` | node | 2618 |
| `clients/web/src/project-relocation.ts` | node | 36 |
| `clients/web/src/remote-projects/connections.ts` | node | 49 |
| `clients/web/src/remote-projects/host.ts` | node | 350 |
| `clients/web/src/remote-projects/permissions.ts` | node | 34 |
| `clients/web/src/remote-projects/terminals.ts` | node | 73 |
| `clients/web/src/remote-projects/transport.ts` | node | 51 |
| `clients/web/src/repository-root.ts` | node | 21 |
| `clients/web/src/terminal-ws-bridge.ts` | node | 553 |
| `clients/web/src/vite-module-warmup.ts` | node | 31 |
| `clients/web/src/change-mux-bridge.ts` | both | 180 |
| `clients/web/src/change-ws-mux.ts` | both | 278 |
| `clients/web/src/client-build.ts` | both | 34 |
| `clients/web/src/client-origin.ts` | both | 132 |
| `clients/web/src/compatibility.ts` | both | 86 |
| `clients/web/src/conversation-export.ts` | both | 371 |
| `clients/web/src/desktop-storage-contract.ts` | both | 80 |
| `clients/web/src/host-features.ts` | both | 97 |
| `clients/web/src/host-only-actions.ts` | both | 63 |
| `clients/web/src/json-value.ts` | both | 53 |
| `clients/web/src/local-bridge-client.ts` | both | 342 |
| `clients/web/src/registered-projects-model.ts` | both | 46 |
| `clients/web/src/remote-projects/identity.ts` | both | 46 |
| `clients/web/src/server-response-parsers.ts` | both | 1995 |
| `clients/web/src/terminal-project-scope.ts` | both | 37 |
| `clients/web/src/source-format-matchers.ts` | node (Vitest only; imported by `clients/web/vitest.config.ts` and tests) | 54 |
| `clients/web/src/test-quarantine.ts` | node (Vitest only; imported only by its own test) | 31 |

### Browser modules (mapped by HS2-AYKHVV in coverage/web-client.md)

Every non-test module under these directories is `browser`: `src/app`,
`src/components`, `src/features`, `src/interaction-attrs`,
`src/interactions`, `src/standby-saver`, `src/ux-demo` (deliberately written
without the `clients/web/` prefix so this file does not map them for the
checker). The remaining `browser` modules are the top-level and
`remote-projects` files below. `remote-projects/` is split:
`identity.ts` is `both`, `connections.ts`, `host.ts`, `permissions.ts`,
`terminals.ts` and `transport.ts` are `node`, and the three files below are
`browser`.

```text
clients/web/src/active-ticket-work.ts
clients/web/src/advanced-search.ts
clients/web/src/advisory-claim-resource.ts
clients/web/src/ai-conversation.ts
clients/web/src/ai-launch-dialog.ts
clients/web/src/ai-provider-defaults.ts
clients/web/src/annotation-intents.ts
clients/web/src/announcer-editorial.ts
clients/web/src/announcer-layout.ts
clients/web/src/announcer-reel.ts
clients/web/src/announcer-settings.ts
clients/web/src/announcer-speech.ts
clients/web/src/api.ts
clients/web/src/app-region-resize.ts
clients/web/src/attachment-filename-editing.ts
clients/web/src/attachment-filename-interactions.ts
clients/web/src/attachment-files.ts
clients/web/src/attachment-grouping.ts
clients/web/src/attachment-label-editing.ts
clients/web/src/attachment-owner.ts
clients/web/src/attachment-references.ts
clients/web/src/automatic-ticket-group.ts
clients/web/src/away-digest.ts
clients/web/src/board-pagination.ts
clients/web/src/browser-draft-file-contract.ts
clients/web/src/browser-draft-mutation-queue.ts
clients/web/src/browser-id.ts
clients/web/src/bulk-update-progress.ts
clients/web/src/change-poll-mux.ts
clients/web/src/change-poll-slots.ts
clients/web/src/change-read-gap.ts
clients/web/src/change-stream-shared-hub.ts
clients/web/src/change-stream-worker-host.ts
clients/web/src/change-stream-worker-protocol.ts
clients/web/src/change-stream-worker-runtime.ts
clients/web/src/change-stream.shared-worker.ts
clients/web/src/change-stream.worker.ts
clients/web/src/ci-presentation.ts
clients/web/src/claim-holder.ts
clients/web/src/client-notifications.ts
clients/web/src/client-update-notice.ts
clients/web/src/clipboard-when-ready.ts
clients/web/src/command-group-dialog.ts
clients/web/src/command-hold.ts
clients/web/src/command-order.ts
clients/web/src/command-schedule-form.ts
clients/web/src/command-schedules.ts
clients/web/src/command-settings-workflow.ts
clients/web/src/composer-restore-intent.ts
clients/web/src/confirmation-interactions.ts
clients/web/src/confirmation.ts
clients/web/src/context-menu-position.ts
clients/web/src/controlled-open.ts
clients/web/src/conversation-activity-disclosure.ts
clients/web/src/conversation-composers.ts
clients/web/src/conversation-context-menu.ts
clients/web/src/conversation-history.ts
clients/web/src/conversation-persistence.ts
clients/web/src/conversation-render-scheduler.ts
clients/web/src/conversation-scroll.ts
clients/web/src/conversation-selection.ts
clients/web/src/conversation-unread.ts
clients/web/src/copy-text.ts
clients/web/src/custom-ai-command.ts
clients/web/src/debounced-autosave.ts
clients/web/src/desktop-close-contract.ts
clients/web/src/desktop-close.ts
clients/web/src/desktop-cursor.ts
clients/web/src/desktop-draft-files.ts
clients/web/src/desktop-recovery-flush.ts
clients/web/src/desktop-storage.ts
clients/web/src/desktop-window-theme.ts
clients/web/src/dev-latency-entry.ts
clients/web/src/dev-latency.ts
clients/web/src/dev-reload-diagnostics-entry.ts
clients/web/src/dev-reload-diagnostics.ts
clients/web/src/development-strategy-tags.ts
clients/web/src/device-name.ts
clients/web/src/document-scroll-restore.ts
clients/web/src/draft-file-recovery.ts
clients/web/src/draft-input-coalescing.ts
clients/web/src/draft-submission-state.ts
clients/web/src/drag-leave.ts
clients/web/src/drawer-tab-order.ts
clients/web/src/drive-interrupt-reply.ts
clients/web/src/feature-areas.ts
clients/web/src/feedback-choice-lines.ts
clients/web/src/feedback-choices.ts
clients/web/src/feedback-needed.ts
clients/web/src/feedback-prompt-edits.ts
clients/web/src/feedback-replies.ts
clients/web/src/feedback-response-drafts.ts
clients/web/src/feedback-session-labels.ts
clients/web/src/file-picker-routes.ts
clients/web/src/focused-draft-sync.ts
clients/web/src/gallery-annotation-editor.ts
clients/web/src/gallery-annotation-gesture.ts
clients/web/src/gallery-annotation-workflow.ts
clients/web/src/gallery-crop.ts
clients/web/src/gallery-drag-modifiers.ts
clients/web/src/github-account-endpoint.ts
clients/web/src/github-attachment-settings.ts
clients/web/src/github-sign-in-origin.ts
clients/web/src/grid-sidebar-live.ts
clients/web/src/guidance-search-model.ts
clients/web/src/guidance-update-cache.ts
clients/web/src/halted-sessions.ts
clients/web/src/host-cpu-feed.ts
clients/web/src/host-disk-choices.ts
clients/web/src/host-health-resource.ts
clients/web/src/host-health.ts
clients/web/src/inline-editor-escape.ts
clients/web/src/inline-search-caret.ts
clients/web/src/inline-search.ts
clients/web/src/input-dialog-interactions.ts
clients/web/src/interaction-performance.ts
clients/web/src/keyboard-shortcuts.ts
clients/web/src/live-state-icons.ts
clients/web/src/live-state.ts
clients/web/src/local-ticket-changes.ts
clients/web/src/lucide-catalog.ts
clients/web/src/machine-settings-source.ts
clients/web/src/main.tsx
clients/web/src/markdown-click-edit.ts
clients/web/src/migration-job-client.ts
clients/web/src/migration-progress.ts
clients/web/src/minimized-composer.ts
clients/web/src/minimized-dialogs.ts
clients/web/src/minimized-readers.ts
clients/web/src/mobile-layout.ts
clients/web/src/mobile-terminal-columns.ts
clients/web/src/mobile-terminal-focus.ts
clients/web/src/mobile-view-choices.ts
clients/web/src/native-notifications.ts
clients/web/src/new-ticket-attachments.ts
clients/web/src/new-ticket-source.ts
clients/web/src/not-working-workflow.ts
clients/web/src/note-authorship.ts
clients/web/src/note-repair-proposal.ts
clients/web/src/note-response-link.ts
clients/web/src/notification-pause.ts
clients/web/src/notification-settings-model.ts
clients/web/src/open-external.ts
clients/web/src/orchestration-built-ahead.ts
clients/web/src/orchestration-capabilities.ts
clients/web/src/orchestration-hand-back.ts
clients/web/src/orchestration-plan-health.ts
clients/web/src/orchestration-plan.ts
clients/web/src/orchestration-progress.ts
clients/web/src/orchestration-refusals.ts
clients/web/src/orchestration-remote-labels.ts
clients/web/src/orchestration-resources.ts
clients/web/src/orchestration-run-checks.ts
clients/web/src/orchestration-scheduler.ts
clients/web/src/orchestration-settings-model.ts
clients/web/src/orchestration-timeline-board.ts
clients/web/src/orchestration-timeline.ts
clients/web/src/orchestration-validation-steps.ts
clients/web/src/overnight-timeline.ts
clients/web/src/pending-created-tickets.ts
clients/web/src/permission-decision-workflow.ts
clients/web/src/permission-notifications.ts
clients/web/src/permission-rule-editor-focus.ts
clients/web/src/popup-menu-state.ts
clients/web/src/priority-wire.ts
clients/web/src/project-activation.ts
clients/web/src/project-change-poll.ts
clients/web/src/project-change-refresh.ts
clients/web/src/project-drive.ts
clients/web/src/project-live-summary.ts
clients/web/src/project-minimization.ts
clients/web/src/project-settings-state.ts
clients/web/src/project-startup.ts
clients/web/src/project-tab-action-fit.ts
clients/web/src/project-tab-file-drop.ts
clients/web/src/project-tab-order.ts
clients/web/src/project-tab-refresh.ts
clients/web/src/project-ticket-refresh.ts
clients/web/src/project-unavailable-cause.ts
clients/web/src/project-unavailable-shell.ts
clients/web/src/project-warm-cache.ts
clients/web/src/reader-minimize-motion.ts
clients/web/src/refresh-barrier.ts
clients/web/src/remote-devices-model.ts
clients/web/src/remote-host-health.ts
clients/web/src/remote-live-states.ts
clients/web/src/remote-projects/client.ts
clients/web/src/remote-projects/host-names.ts
clients/web/src/remote-projects/reconnect.ts
clients/web/src/remote-session-lifetime.ts
clients/web/src/render-metrics.ts
clients/web/src/repository-file-selection.ts
clients/web/src/repository-state.ts
clients/web/src/request-observer.ts
clients/web/src/resizable-region-commits.ts
clients/web/src/saved-views.ts
clients/web/src/search-collapse.ts
clients/web/src/search-projection-coalescing.ts
clients/web/src/server-busy.ts
clients/web/src/server-clock.ts
clients/web/src/session-restore-guard.ts
clients/web/src/source-stale-notice.ts
clients/web/src/stable-vite-client.ts
clients/web/src/standby.ts
clients/web/src/stats-right-now.ts
clients/web/src/stats-summary.ts
clients/web/src/stats-this-mac.ts
clients/web/src/tab-cycle.ts
clients/web/src/tab-order.ts
clients/web/src/terminal-ai-connection.ts
clients/web/src/terminal-clear.ts
clients/web/src/terminal-clipboard.ts
clients/web/src/terminal-feedback-primary.ts
clients/web/src/terminal-grid-layout.ts
clients/web/src/terminal-keys.ts
clients/web/src/terminal-limits.ts
clients/web/src/terminal-long-press.ts
clients/web/src/terminal-names.ts
clients/web/src/terminal-progressive-work.ts
clients/web/src/terminal-query-replies.ts
clients/web/src/terminal-question-answer.ts
clients/web/src/terminal-replay-stream.ts
clients/web/src/terminal-search-count-fit.ts
clients/web/src/terminal-search.ts
clients/web/src/terminal-snapshot-refresh.ts
clients/web/src/terminal-theme.ts
clients/web/src/terminal-ticket-links.ts
clients/web/src/terminal-touch-scroll.ts
clients/web/src/terminal-touch-selection.ts
clients/web/src/terminal-viewport-runtime.ts
clients/web/src/terminal-viewport.ts
clients/web/src/terminal-visibility.ts
clients/web/src/text-merge.ts
clients/web/src/theme-preference.ts
clients/web/src/theme-tile-art.tsx
clients/web/src/ticket-blockers.ts
clients/web/src/ticket-blocking.ts
clients/web/src/ticket-board-layout.ts
clients/web/src/ticket-bulk-operations.ts
clients/web/src/ticket-category-preference.ts
clients/web/src/ticket-clipboard-shortcuts.ts
clients/web/src/ticket-close.ts
clients/web/src/ticket-completion-trend.ts
clients/web/src/ticket-creator.ts
clients/web/src/ticket-deep-link.ts
clients/web/src/ticket-detail-cache.ts
clients/web/src/ticket-display-number.ts
clients/web/src/ticket-draft-store.ts
clients/web/src/ticket-drop-target.ts
clients/web/src/ticket-editor-size.ts
clients/web/src/ticket-field-reconciliation.ts
clients/web/src/ticket-follow-up-link.ts
clients/web/src/ticket-group-state.ts
clients/web/src/ticket-groups.ts
clients/web/src/ticket-link-resolution.ts
clients/web/src/ticket-motion.ts
clients/web/src/ticket-mutation.ts
clients/web/src/ticket-operations.ts
clients/web/src/ticket-prefix.ts
clients/web/src/ticket-reader-focus.ts
clients/web/src/ticket-reader-stack.ts
clients/web/src/ticket-reference-search.ts
clients/web/src/ticket-row-containment.ts
clients/web/src/ticket-scroll-state.ts
clients/web/src/ticket-search-model.ts
clients/web/src/ticket-timeline-data.ts
clients/web/src/ticket-title-disclosure.ts
clients/web/src/ticket-title-editing.ts
clients/web/src/ticket-transfer.ts
clients/web/src/ticket-usage-timeline.ts
clients/web/src/ticket-views.ts
clients/web/src/timeline-stage-estimate.ts
clients/web/src/toast-clearance.ts
clients/web/src/toast-lifetime.ts
clients/web/src/toast-modal-host.ts
clients/web/src/toast-presentation.ts
clients/web/src/top-layer-overlay.ts
clients/web/src/tracked-size-observer.ts
clients/web/src/trailing-task-internal.ts
clients/web/src/trailing-task.ts
clients/web/src/trash-empty-summary.ts
clients/web/src/turn-clock.ts
clients/web/src/ux-demo-entry.ts
clients/web/src/versioned-long-poll.ts
clients/web/src/video-posters.ts
clients/web/src/wa-dialog-semantics.ts
clients/web/src/worker-provisional.ts
clients/web/src/worker-ticket-refresh.ts
clients/web/src/worker-view-model.ts
clients/web/src/worker-view-stream.ts
clients/web/src/workspace-preferences.ts
clients/web/src/workspace-session.ts
clients/web/src/workspace-ticket-sort.ts
clients/web/src/workspace-toolbar-visibility.ts
clients/web/src/workspace-view-memory.ts
```
