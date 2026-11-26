## CLI and MCP shim

![](embed:HS2-Cli-CommandTree)

This is the starting diagram for this area. The **Start here** chapter lists the others.

Status: written by HS2-TW2DKM. Audited source
`8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`. Per-component fields (state,
types, concurrency, errors, configuration, invariants, tests) are in
[coverage/cli.md](../coverage/cli.md); this chapter explains how the pieces fit
and narrates the dynamic views.

### Overview

Three runtime containers live in this area:

- **`hotsheet-cli`** (`crates/hotsheet-cli/src/main.rs`), one process per
  command. It is also a **library** (`crates/hotsheet-cli/src/lib.rs:14-43`)
  that the server (`crates/hotsheet-server/Cargo.toml:18`) and the MCP shim
  (`crates/hotsheet-mcp/Cargo.toml:30`) link. The coordination pool, the
  `OrchestrationRuntime`, the remote-server transport, host discovery, the
  sync kick, the notification watch, provisional claim notices and the
  permission-hook mapping therefore run inside the server and MCP processes
  as well. A reader who expects `hotsheet-cli/src` to be CLI-only will be
  surprised; the model tags those components `Library`.
- **`hotsheet-mcp`** (`crates/hotsheet-mcp/src/main.rs`), a long-lived stdio
  JSON-RPC process per AI session.
- **`hotsheet-migrate`** (`crates/hotsheet-cli/src/bin/hotsheet-migrate.rs`),
  a separate binary in the same crate, plus the Node exporter
  (`migrator/src/export.mjs`).

Views: `HS2-Cli-CommandTree`, `HS2-Cli-TicketingDependencies`,
`HS2-Cli-OperationsDependencies`, `HS2-Cli-McpComponents`,
`HS2-Cli-MigratorComponents`, and the dynamic views listed below.

### Components

| Component (model id)                                                         | Main modules                                                               | Runs in                |
| ---------------------------------------------------------------------------- | -------------------------------------------------------------------------- | ---------------------- |
| Command tree and dispatch (`cliEntry`)                                       | `main.rs`, `dispatch/mod.rs`, `lib.rs`                                     | CLI                    |
| Post-command sync kick and watch (`cliPostCommand`)                          | `sync_kick.rs`, `notify_watch.rs`                                          | CLI, MCP               |
| Local host discovery and server commands (`cliHostClient`)                   | `external_launch.rs`, `dispatch/server.rs`                                 | CLI, MCP               |
| Store commands (`cliStoreCmds`)                                              | `dispatch/store.rs`, `dispatch/checkout_remove.rs`, `sync_ai.rs`           | CLI                    |
| Ticket commands (`cliTicketCmds`)                                            | `dispatch/tickets.rs`, `dispatch/groups.rs`, `dispatch/usage_reroute.rs`   | CLI                    |
| Claim commands (`cliClaimCmds`)                                              | `dispatch/claims.rs`, `dispatch/claim_process.rs`, `provisional_claims.rs` | CLI                    |
| Provider commands (`cliProviderCmds`)                                        | `dispatch/providers.rs`, `feedback_synthesis.rs`                           | CLI                    |
| Setup, launch and drive (`cliSetupCmds`)                                     | `dispatch/setup.rs`, `setup.rs`, `plugin.rs`, `workloop.rs`                | CLI                    |
| Permission hook adapter (`cliPermissionHook`)                                | `permission_hook.rs`, hook functions in `dispatch/setup.rs`                | CLI per hook           |
| Coordination pool (`cliCoordination`)                                        | `coordination/`, `dispatch/coordination*.rs`, `merge_pending.rs`           | CLI, server, MCP       |
| Orchestration runtime (`cliOrchestration`)                                   | `orchestration.rs`, `orchestration/`, `dispatch/orchestration*.rs`         | CLI, server, MCP       |
| Remote servers (`cliRemotes`)                                                | `remote_servers.rs`, `dispatch/remotes.rs`                                 | CLI, server, MCP       |
| Jobs (`cliJobs`)                                                             | `dispatch/jobs.rs`, `dispatch/job_*.rs`                                    | CLI, detached monitor  |
| Resource claims (`cliResources`)                                             | `dispatch/resources.rs`, `resource_backends.rs`, `resource_remote.rs`      | CLI                    |
| Notify and remote change (`cliNotify`)                                       | `dispatch/notify.rs`, `dispatch/remote_change.rs`                          | CLI                    |
| MCP loop and backend selection (`mcpEntry`)                                  | `hotsheet-mcp/src/main.rs`                                                 | MCP                    |
| MCP tool catalog (`mcpTools`)                                                | `hotsheet-mcp/src/lib.rs`                                                  | MCP                    |
| MCP domain tools (`mcpDomainTools`)                                          | `orchestration_tools.rs`, `remote_tools.rs`, `resource_claim_tools.rs`     | MCP                    |
| MCP serverless backend (`mcpCoreBackend`)                                    | `CoreBackend` in `hotsheet-mcp/src/lib.rs`                                 | MCP                    |
| MCP HTTP backend (`mcpHttpBackend`)                                          | `HttpBackend` in `hotsheet-mcp/src/lib.rs`                                 | MCP                    |
| MCP presence (`mcpPresence`)                                                 | `presence.rs`                                                              | MCP thread             |
| Migrator entry, exporter, importer (`migBinary`, `migExporter`, `migImport`) | `bin/hotsheet-migrate.rs`, `migrator/src`, `import*.rs`                    | migrator, CLI `import` |

Coordination and orchestration domain rules are documented in
[20-orchestration](20-orchestration.md); this chapter covers their CLI host
side. Note that the coordination contract types live in this crate
(`crates/hotsheet-cli/src/coordination/contract.rs:10`), not in
`hotsheet-ticketing`.

#### Command-tree map

`Cmd` has 106 top-level variants (`crates/hotsheet-cli/src/main.rs:83-1016`)
in 12 dispatch groups (`crates/hotsheet-cli/src/dispatch/mod.rs:58-71`).
"Store" means the command opens the Git store or machine files directly;
"Server" means it needs a running host found through the instance registry.
Test: `crates/hotsheet-cli/src/main.rs:1738` (`every_subcommand_has_non_empty_help`).

| Group (dispatch module)                                                                 | Commands                                                                                                                                                                                                                                                          | Store / Server                                                                                                                                                                                                      | Domain crates                                                             | Side effects                                                                                              |
| --------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Store (`dispatch/store.rs`, `checkout_remove.rs`)                                       | `init`, `bootstrap`, `link`, `activate-format`, `prefix`, `compatibility`, `import`, `checkout`, `sync`, `doctor`, `reindex`, `worklist`, `purge-trash`, `merge-driver` (`crates/hotsheet-cli/src/dispatch/store.rs:18-110`)                                      | Store. `checkout remove`/`prune` first ask every live server (`crates/hotsheet-cli/src/dispatch/checkout_remove.rs:1-9`); `sync` defers an AI repair to a hosting server (`crates/hotsheet-cli/src/sync_ai.rs:5-9`) | hotsheet-ticketing, hotsheet-index, hotsheet-plugins                      | Git init/commit/fetch/push, link file, `checkouts.json`, worklists                                        |
| Tickets (`dispatch/tickets.rs`, `groups.rs`, `usage_reroute.rs`)                        | `new`, `link-follow-up`, `ls`, `show`, `note`, `batch`, `delete-note`, `attach`, `attachment-*`, `annotate`, `edit`, `close`, `restore`, `copy`, `move`, `assign`, `people`, `read`, `groups`, `group`, `confidence-report`, `phase-report`, `metrics`, `reprice` | Store only                                                                                                                                                                                                          | hotsheet-ticketing, hotsheet-index                                        | Ticket commits; `people seed-github` calls GitHub (`crates/hotsheet-cli/src/dispatch/tickets.rs:473-495`) |
| Claims (`dispatch/claims.rs`, `claim_process.rs`)                                       | `claim`, `claim-next`, `renew`, `release`, `claim-run`, hidden `claim-run-child`                                                                                                                                                                                  | Store only                                                                                                                                                                                                          | hotsheet-ticketing (ops)                                                  | Claim commits; `claim-run` spawns and supervises a process tree                                           |
| Providers (`dispatch/providers.rs`)                                                     | `providers`, `github-*`, `provider-*`, `feedback-synthesis`, `key`, `account`                                                                                                                                                                                     | Store and provider APIs; no server                                                                                                                                                                                  | hotsheet-ticketing, hotsheet-extsync                                      | HTTPS to GitHub, GitLab, Jira; OS keychain                                                                |
| Server (`dispatch/server.rs`)                                                           | `serve`, `cert`, `commands`, `terminal`, `lifecycle`, `ticket-flow`, `live-state`, `ci`, `live-history`, `activity`, `notifications`, `processes` (`crates/hotsheet-cli/src/dispatch/server.rs:14-75`)                                                            | Server for all but `serve` (starts one), `cert` (files), `live-history` (files) and `processes` (journal)                                                                                                           | hotsheet-tls, hotsheet-process-journal                                    | Child `hotsheet-server`; HTTP to the host                                                                 |
| Setup (`dispatch/setup.rs`, `setup_diagnose.rs`, `terminal_relay.rs`, `screensaver.rs`) | `setup`, `guidance`, `plugin`, `ai-tools`, `ai-settings`, `settings`, `host-disks`, `terminal-relay`, `screensaver`, `permission-hook`, `permission-discard`, `hook-diagnose`, `launch`, `trigger`, `work` (`crates/hotsheet-cli/src/dispatch/setup.rs:472-531`)  | Store for setup; `launch` starts or finds the machine host; `permission-hook` and `hook-diagnose` need a route-back server; `trigger`/`work` need none                                                              | hotsheet-plugins, hotsheet-aitools, hotsheet-terminals                    | Project files, tool processes, LaunchAgent and screen-saver installs                                      |
| Coordination (`dispatch/coordination*.rs`)                                              | `coordination` (55 subcommands, `crates/hotsheet-cli/src/dispatch/coordination.rs:182`)                                                                                                                                                                           | Store and local pool; no server (`:1`)                                                                                                                                                                              | hotsheet-cli coordination, hotsheet-ticketing provisional, hotsheet-jobs  | Pool files under the Git common directory; Git fetch and branch config                                    |
| Orchestration (`dispatch/orchestration*.rs`)                                            | `orchestration` (23 families, `crates/hotsheet-cli/src/dispatch/orchestration.rs:30`)                                                                                                                                                                             | In-process runtime; no server needed, but some requests call a local or remote server                                                                                                                               | hotsheet-ticketing orchestration, hotsheet-jobs, hotsheet-resource-claims | Machine journals, detached job monitor and step runner                                                    |
| Remote (`dispatch/remotes.rs`)                                                          | `remote` (13 subcommands, `crates/hotsheet-cli/src/dispatch/remotes.rs:20`)                                                                                                                                                                                       | Machine files; `remote sessions` needs the host                                                                                                                                                                     | hotsheet-cli remote_servers                                               | HTTPS/mTLS to paired servers; keychain                                                                    |
| Jobs (`dispatch/jobs.rs`, `job_*.rs`)                                                   | `job list                                                                                                                                                                                                                                                         | show                                                                                                                                                                                                                | watch                                                                     | monitor`                                                                                                  | Machine files only (`crates/hotsheet-cli/src/dispatch/jobs.rs:1-4`) | hotsheet-jobs | HTTPS to CI providers; `gh`/`glab` for tokens |
| Resources (`dispatch/resources.rs`, `resource_*.rs`, `remote_change.rs`)                | `resource …`, `remote-change …`                                                                                                                                                                                                                                   | Machine files; `resource renew                                                                                                                                                                                      | release --server` calls a server                                          | hotsheet-resource-claims, hotsheet-notify                                                                 | Claim records, OS keychain                                          |
| Notify (`dispatch/notify.rs`)                                                           | `notify …`                                                                                                                                                                                                                                                        | Machine files only                                                                                                                                                                                                  | hotsheet-notify                                                           | Notifier sends                                                                                            |

#### Server discovery and bootstrap

A server writes `${HOTSHEET_HOME}/instances/<first 16 hex of sha(canonical
anchor)>.json` with `pid`, `url`, `secret`, `store_path`
(`crates/hotsheet-cli/src/external_launch.rs:15-36`). The desktop app and
headless launches run one **machine host** anchored on
`${HOTSHEET_HOME}/server-bootstrap.hs2` (`:104-110`); `hotsheet-cli serve -C
<store>` can still run a per-store host. `discover_local_host` tries the
store's record then the machine host for project routes, and the reverse for
host-wide routes (`:128-152`). A record is used only when its pid is alive and
its `store_path` matches (`:42-68`). The credential is the instance
`secret`, sent as `X-Hotsheet-Secret`
(`crates/hotsheet-cli/src/dispatch/server.rs:1083`); paired remote servers use
bearer and cookie session tokens instead
(`crates/hotsheet-cli/src/remote_servers.rs:647-675`).

`serve` resolves `hotsheet-server` beside the CLI or on `PATH`, refuses a
different version, and runs it as a foreground child
(`crates/hotsheet-cli/src/dispatch/server.rs:78-114`, `:793-840`). Only
`launch` and the MCP project mode start a server implicitly, through
`ensure_running_server` (`crates/hotsheet-cli/src/external_launch.rs:154-231`,
`crates/hotsheet-mcp/src/main.rs:72-77`). Test:
`crates/hotsheet-cli/src/external_launch.rs:476` (`hs2_vg3e8n_bootstrap_only_host_serves_a_project_store`).

#### Exit codes and errors

`main` returns `anyhow::Result<()>` (`crates/hotsheet-cli/src/main.rs:1141`),
so every failure prints `Error: <chain>` on stderr and exits 1. There are no
other numbered exit codes except: `merge-driver` 0/1
(`crates/hotsheet-cli/src/dispatch/store.rs:1083`), `launch` passing through
the tool's code or 128 plus signal (`crates/hotsheet-cli/src/dispatch/setup.rs:1073`),
and a closed output pipe exiting 0 (`crates/hotsheet-cli/src/broken_pipe.rs:19`).
Commands that print JSON report domain refusals inside the JSON and still exit
0 unless the handler bails, for example orchestration prints
`<code>: <message>` errors (`crates/hotsheet-cli/src/dispatch/orchestration.rs:1737`).
Test: `crates/hotsheet-cli/tests/broken_pipe.rs`; the generic exit-1 path is
standard Rust behavior and untested here.

#### Headless parity

The project rule is that every graphical bootstrap or recovery path has a
headless CLI path using the same workflow (`AGENTS.md`). Verified pairs at
this revision: project bootstrap and setup (`bootstrap`, `setup`), launch
(`launch`), checkout removal (`checkout remove`, same
`remove_checkout` function, `crates/hotsheet-cli/src/dispatch/checkout_remove.rs:4-9`),
notifier setup (`notify`), resource backends, terminal relay and screen saver
installs (`terminal-relay`, `screensaver`, the desktop app runs the same
command, `crates/hotsheet-cli/src/dispatch/terminal_relay.rs:1-3`), HS1
import and attachment recovery (`import`, `hotsheet-migrate`), remote
session approval (`remote sessions`), and orchestration routes (in-process
runtime over the server's routes). A complete GUI-to-CLI audit was not done
here; see Unresolved U-CLI-3.

#### MCP versus CLI

All 43 MCP tools go through the `Backend` trait with HTTP-shaped paths
(`crates/hotsheet-mcp/src/lib.rs:24-37`); the CLI calls domain functions
directly. Mapping (tool: CLI command):

- Read: `hotsheet_query`: `ls`; `hotsheet_get`: `show`;
  `hotsheet_note_doctor`: `note doctor`; `hotsheet_groups`: `groups`;
  `hotsheet_providers`: `providers`; `hotsheet_confidence_report`,
  `hotsheet_phase_report`: same names.
- Write: `hotsheet_create`: `new`; `hotsheet_update`: `edit`;
  `hotsheet_close`, `hotsheet_restore`, `hotsheet_assign`, `hotsheet_batch`,
  `hotsheet_link_follow_up`, `hotsheet_copy`, `hotsheet_move`,
  `hotsheet_provider_copy`, `hotsheet_provider_move`: same names;
  `hotsheet_delete_note`: `delete-note` or `provider-delete-note`;
  `hotsheet_report_not_working`: `provider-report-not-working`;
  attachment tools: `attachment-rename|delete|metadata`, `annotate`.
- Claims: `hotsheet_claim`, `hotsheet_claim_next`, `hotsheet_release`,
  `hotsheet_renew`: same; `hotsheet_advisory_claim_status|event`:
  `provider-advisory-claim`.
- Machine and checkouts: `hotsheet_checkouts`: `checkout list`;
  `hotsheet_resolve_checkout`: `checkout resolve`;
  `hotsheet_repository_size`: `checkout repo-size`; `hotsheet_sync_health`:
  `sync --status`; `hotsheet_announce`: no CLI command.
- Coordination: `hotsheet_workers`: `coordination workers`;
  `hotsheet_speculation`: `coordination speculation-*`;
  `hotsheet_worker_plan_replace|dispatch|events`: `coordination
plan-replace|plan-dispatch|plan-events`.
- Other: `hotsheet_targets`: `orchestration target …`; `hotsheet_remotes`:
  `remote …`; `hotsheet_resource_claim`: `resource …`.

Behaviors that differ: MCP has no setup, launch, serve, import, sync,
coordination registration or orchestration families beyond targets; the
worker plan tools carry no actor (`crates/hotsheet-mcp/src/lib.rs:665-693`);
`hotsheet_query` defaults to compact output for a connection (`:921-925`);
`hotsheet_claim_next` refuses a provider connection (`:1246-1254`);
serverless advisory claims are refused with 409 (`:2558-2562`); and tool
failures are `isError` results rather than an exit status (`:630-634`). Both
paths kick host sync after serverless writes and use the same notification
watch. Tests: `crates/hotsheet-mcp/src/lib.rs:5147`,
`crates/hotsheet-mcp/tests/serverless_sync_kick.rs`.

#### MCP routing modes

Decided at `crates/hotsheet-mcp/src/main.rs:146-173`:

1. **Direct store (serverless)**: `--path` builds `CoreBackend` with the
   host sync kick (`:149-151`); `--path .` follows `HOTSHEET_STORE` or a
   store link (`:435-453`).
2. **Explicit server**: `--server` plus required `--secret` builds
   `HttpBackend` (`:152-157`).
3. **Project-routed**: `--project` resolves the checkout in
   `checkouts.json`, ensures the machine host unless launched, posts
   `/projects/open` and injects the checkout id into every tool
   (`:68-80`, `:158-166`, `crates/hotsheet-mcp/src/lib.rs:620-628`).
4. **Launched-session attach**: with `HOTSHEET_SERVER`/`HOTSHEET_SECRET`
   inherited (and no shared host such as the Codex app-server), project mode
   attaches to that live server and never starts one (`:229-270`).

Presence runs only after `initialize`, every 15 s, and is released on EOF
(`crates/hotsheet-mcp/src/presence.rs:22-23`, `:227-276`). Test:
`crates/hotsheet-mcp/tests/registry_presence.rs`.

### Durable state and file formats

| Record                                                                    | Writer                                                                                                    | Crash behavior                                                                                 |
| ------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `.hotsheet2/store` link (absolute path, gitignored)                       | `link_store` (`crates/hotsheet-cli/src/lib.rs:259`)                                                       | Plain write; re-run `link` or `bootstrap`                                                      |
| `.gitattributes`, `info/attributes` merge-driver lines                    | `register_merge_driver` (`crates/hotsheet-cli/src/lib.rs:513`)                                            | Best effort, idempotent                                                                        |
| `<git-common-dir>/hotsheet-coordination/{state,batches,speculation}.json` | coordination (`crates/hotsheet-cli/src/coordination/mod.rs:172-187`)                                      | Tempfile, fsync, rename under `state.lock`                                                     |
| `${HOTSHEET_HOME}/orchestration-validation-*`                             | orchestration runtime (`crates/hotsheet-cli/src/orchestration/validation_output.rs:24`)                   | Private files; spec claimed by atomic rename                                                   |
| `hotsheet-hs1-import-pending/<ulid>.json`                                 | importer (`crates/hotsheet-cli/src/import.rs:193-204`)                                                    | Atomic persist; resumes on re-run                                                              |
| `hotsheet-hs1-import.json`, `-import-completed.json`, `-backup.json`      | migrator (`crates/hotsheet-cli/src/lib.rs:410-422`, `crates/hotsheet-cli/src/import_completion.rs:10-11`) | Receipt is a plain write, then committed; completion proof invalidated at the next run's start |
| `<git-dir>/hotsheet-sync-ai.log`                                          | detached sync repair (`crates/hotsheet-cli/src/sync_ai.rs:11`)                                            | Append log                                                                                     |
| `${HOTSHEET_HOME}/tls/<project-id>/`                                      | `cert` (`crates/hotsheet-cli/src/dispatch/server.rs:118-120`)                                             | Private files created new (`:254`)                                                             |

The CLI also reads `${HOTSHEET_HOME}/checkouts.json`, `instances/*.json`,
`remotes.json`, `providers.json`, the notification configuration and the
process journal; their owners are the core and server areas.

### Concurrency, tasks and locks

- CLI: one main thread plus the 8 MiB parser thread
  (`crates/hotsheet-cli/src/main.rs:1223-1236`). `claim-run` adds a renewal
  thread and a 250 ms supervision loop (`crates/hotsheet-cli/src/dispatch/claims.rs:117-170`).
  `launch` waits on the tool with SIGINT, SIGQUIT and SIGHUP ignored
  (`crates/hotsheet-cli/src/dispatch/setup.rs:1041-1049`). Store writes are
  serialized by the core's store lock; the coordination pool by `state.lock`.
- Waits: `ensure_running_server` checks a local file every 100 ms for 10 s
  (`crates/hotsheet-cli/src/external_launch.rs:211-230`); the permission-hook
  question wait polls the server every 350 ms for up to a day
  (`crates/hotsheet-cli/src/dispatch/setup.rs:2163-2189`); orchestration and
  coordination waits are server-side long polls or filesystem watches.
- Detached children: `hotsheet-cli job monitor`, `hotsheet-cli orchestration
step-runner`, the sync repair session and the machine host started by
  `launch` outlive the command, recorded in the process journal.
- MCP: one stdin loop thread and one presence thread; shutdown is EOF,
  which drops presence, which joins the thread (`crates/hotsheet-mcp/src/main.rs:202-203`).

### Failure and recovery

- No host: server-backed commands fail naming `hotsheet-cli serve`
  (`crates/hotsheet-cli/src/external_launch.rs:145-151`); writes still commit
  and print a local-only notice (`crates/hotsheet-cli/src/sync_kick.rs:112-120`).
- Hook route failure: the tool's native prompt, or deny under headless Claude
  auto (`crates/hotsheet-cli/src/dispatch/setup.rs:779-813`).
- MCP route loss: project mode reattaches and retries once
  (`crates/hotsheet-mcp/src/main.rs:376-406`); presence relocates
  (`crates/hotsheet-mcp/src/presence.rs:215-238`).
- Import: per-ticket checkpoints make a re-run resume
  (`crates/hotsheet-cli/src/import.rs:149-190`); explicit attachment recovery
  is `import --diagnose-attachments`, `--restore-attachment`,
  `--confirm-omission` (`crates/hotsheet-cli/src/dispatch/store.rs:46-75`).
- Sync: `sync --recover-stale-locks` and `--restore-saved-writes`
  (`crates/hotsheet-cli/src/dispatch/store.rs:592`, `:617`).

### Configuration and environment

Every variable read in this area, default "unset" unless stated:

| Variable                                                                                                                                 | Read at                                                                                                                                                                                                             | Use                                                      |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| `HOTSHEET_HOME` (default `~/.hotsheet2`)                                                                                                 | `crates/hotsheet-ticketing/src/machine_home.rs:33-35`                                                                                                                                                               | Machine home for every component                         |
| `HOTSHEET_STORE`                                                                                                                         | `crates/hotsheet-cli/src/lib.rs:238`, `crates/hotsheet-mcp/src/main.rs:48`                                                                                                                                          | Store when `-C`/`--path` is `.`                          |
| `HOTSHEET_ACTOR_ROLE`, `HOTSHEET_ACTOR_ID`                                                                                               | `crates/hotsheet-cli/src/main.rs:66-67`, `crates/hotsheet-mcp/src/lib.rs:711-712`                                                                                                                                   | Mutation actor                                           |
| `HOTSHEET_WORKER_ID`                                                                                                                     | `crates/hotsheet-cli/src/dispatch/tickets.rs:558`                                                                                                                                                                   | Default worker id                                        |
| `HOTSHEET_SERVER`, `HOTSHEET_SECRET`, `HOTSHEET_PROJECT`, `HOTSHEET_PERMISSION_PROJECT`, `HOTSHEET_TERMINAL_ID`, `HOTSHEET_AGENT`        | `crates/hotsheet-cli/src/launched_env.rs:6-16`, `crates/hotsheet-mcp/src/presence.rs:51-66`                                                                                                                         | Launched-session route-back and identity; blank is unset |
| `HOTSHEET_USAGE_STORES`                                                                                                                  | set by `launch` (`crates/hotsheet-cli/src/dispatch/setup.rs:1028`)                                                                                                                                                  | Usage attribution for the launched tool                  |
| `HOTSHEET_CLAUDE_PRETOOLUSE`, `HOTSHEET_CLAUDE_HEADLESS_AUTO` (`1` enables)                                                              | `crates/hotsheet-cli/src/dispatch/setup.rs:763-764`                                                                                                                                                                 | Hook bridging mode                                       |
| `CODEX_HOME`, `CLAUDE_CODE_MESSAGING_SOCKET`, `CLAUDE_CODE_MESSAGING_TOKEN`, `AI_AGENT`, `CLAUDE_CODE_EXECPATH`                          | `crates/hotsheet-cli/src/dispatch/setup.rs:2122-2136`                                                                                                                                                               | Session connection report                                |
| `CLAUDE_CONFIG_DIR`, `HOME`                                                                                                              | `crates/hotsheet-cli/src/dispatch/usage_reroute.rs:52-54`                                                                                                                                                           | Claude project directory                                 |
| `HOTSHEET_MIGRATOR`                                                                                                                      | `crates/hotsheet-cli/src/lib.rs:436`                                                                                                                                                                                | Exporter path                                            |
| `HOTSHEET_RESOURCE_CLAIM_TOKEN`                                                                                                          | `crates/hotsheet-cli/src/dispatch/resources.rs:19`                                                                                                                                                                  | Renewal token                                            |
| `GH_TOKEN`, `GITHUB_TOKEN`, `GITLAB_TOKEN`, CI tokens and `HOTSHEET_*_API_URL`                                                           | `crates/hotsheet-cli/src/dispatch/job_github.rs:48`, `crates/hotsheet-cli/src/dispatch/job_gitlab.rs:19`, `crates/hotsheet-cli/src/dispatch/job_ci_rest.rs:72-131`                                                  | CI watch (full list in coverage)                         |
| `HOTSHEET_JOB_MONITOR_AUTOSTART` (default on)                                                                                            | `crates/hotsheet-cli/src/orchestration/job_monitor.rs:19`                                                                                                                                                           | Gate monitor autostart                                   |
| `HOTSHEET_VALIDATION_EXECUTION_ID`                                                                                                       | `crates/hotsheet-cli/src/orchestration/validation.rs:329`                                                                                                                                                           | Validation step identity                                 |
| `PATH`                                                                                                                                   | `crates/hotsheet-cli/src/dispatch/server.rs:91`, `crates/hotsheet-cli/src/dispatch/terminal_relay.rs:66`                                                                                                            | Locate sibling binaries                                  |
| Test only: `HOTSHEET_TEST_SERVER_OWNER_PID`, `HOTSHEET_TEST_CLAIM_RUN_*`, `HOTSHEET_CLAIM_RUN_START_GATE`, `HOTSHEET_JOB_WATCH_FIXTURES` | `crates/hotsheet-cli/src/external_launch.rs:181`, `crates/hotsheet-cli/src/dispatch/claims.rs:113`, `:183`, `crates/hotsheet-cli/src/dispatch/claim_process.rs:140`, `crates/hotsheet-cli/src/dispatch/jobs.rs:579` | Fixtures                                                 |

Test runs force the launched-session variables to empty
(`crates/hotsheet-cli/src/launched_env.rs:30-52`). Config files: the
machine-home files above, the project's settings through
`hotsheet_ticketing::Settings` (`crates/hotsheet-cli/src/main.rs:1647`), and
the `.hotsheet2/store` link.

### Dynamic views

#### `HS2-Cli-McpServerlessWrite`

1. The tool writes one JSON-RPC line; the loop parses it
   (`crates/hotsheet-mcp/src/main.rs:175-201`).
2. `handle_message_for_checkout` routes `tools/call`; the actor comes from
   arguments or the environment (`crates/hotsheet-mcp/src/lib.rs:75-96`, `:711-730`).
3. `hotsheet_update` becomes `PATCH /tickets/{id}` on `CoreBackend` (`:973`).
4. The core runs the update; an AI completion without confidence fails as
   `error 422: confidence_required`, an `isError` result
   (schema note `:137`, asserted at `:7610`). Test: `crates/hotsheet-mcp/src/lib.rs:7591`.
5. The ticket commits; the notification watch runs (`:2976`).
6. The host sync kick posts `/sync/kick`; with no accepting host the result
   gains a local-only notice (`:2528-2537`, `crates/hotsheet-cli/src/sync_kick.rs:38-58`).
   Test: `crates/hotsheet-mcp/tests/serverless_sync_kick.rs`.

#### `HS2-Cli-McpServerWrite`

1. The tool calls `hotsheet_update`.
2. `--project` resolves the checkout and reads the launched or machine host
   record (`crates/hotsheet-mcp/src/main.rs:68-80`, `:409-431`).
3. `/projects/open` returns the checkout id; redirects are refused (`:207-224`).
4. Dispatch injects the checkout id (`crates/hotsheet-mcp/src/lib.rs:620-628`).
5. `HttpBackend` sends the PATCH with `X-Hotsheet-Secret` (`:4641`).
6. A 3xx fails; 401/403 probe `/health` (`:4696-4779`).
7. Failure path: a transport error with no status re-reads the registry
   (`crates/hotsheet-mcp/src/main.rs:376-406`),
8. re-opens the project on the reattached host and retries once. Test:
   `crates/hotsheet-mcp/tests/launched_session.rs`.

#### `HS2-Cli-Bootstrap`

1. `bootstrap` is dispatched to the Store group without store resolution
   (`crates/hotsheet-cli/src/dispatch/mod.rs:186-211`).
2. `init_store` or `ensure_store_repository` settles the Git owner
   (`crates/hotsheet-cli/src/dispatch/store.rs:213-245`).
3. Merge driver registered and initial commit made (`:249`, `:252-285`).
4. Link written (`:250`).
5. Checkout and Git source registered in `checkouts.json`; worklist regenerated (`:290-294`).
6. Setup runs for detected tools; none detected is reported (`:340-375`).
7. Setup writes instruction files, hooks and MCP config (`crates/hotsheet-cli/src/setup.rs:30`).
8. `--remote` adds origin; without it the exact commands are printed (`crates/hotsheet-cli/src/dispatch/store.rs:296-335`, `:415-423`).
   Tests: `crates/hotsheet-cli/tests/cli.rs:7023`, `crates/hotsheet-cli/tests/cli.rs:7180`.

#### `HS2-Cli-PermissionHook`

1. The tool runs `hotsheet-cli permission-hook --agent <tool>` with hook
   JSON on stdin (`crates/hotsheet-cli/src/dispatch/setup.rs:708-715`).
2. `hook_server_route` prefers the live instance for `HOTSHEET_PROJECT`, else
   inherited `HOTSHEET_SERVER`/`HOTSHEET_SECRET` (`:1986-2012`).
3. The instance record's pid must be alive (`crates/hotsheet-cli/src/external_launch.rs:54-60`).
4. `POST /permissions/ask` blocks with no client timeout (`crates/hotsheet-cli/src/dispatch/setup.rs:2194-2206`).
5. The decision JSON is printed (`:813-821`).
6. Failure path: route error prints nothing, or deny under headless Claude
   auto (`:779-811`). Tests: `crates/hotsheet-cli/tests/cli.rs:2749`,
   `crates/hotsheet-cli/tests/cli.rs:2789`.

#### `HS2-Cli-Migrate`

1. `hotsheet-migrate` parses its arguments (`crates/hotsheet-cli/src/bin/hotsheet-migrate.rs:15-48`).
2. Earlier completion proof is invalidated (`crates/hotsheet-cli/src/lib.rs:351`).
3. `node export.mjs` runs into a private temp directory (`:355-388`).
4. The exporter opens a copy of the datadir (`migrator/src/export.mjs:213`).
5. Import runs with progress (`crates/hotsheet-cli/src/lib.rs:403`).
6. Per ticket: checkpoint, ticket, attachments, checkpoint removal (`crates/hotsheet-cli/src/import.rs:238-330`).
7. Tickets go through the core writer with deterministic ids (`crates/hotsheet-cli/src/import.rs:1-6`).
8. Failure path: the checkpoint stays and a re-run resumes or refuses a changed export (`:149-190`).
9. Verify, write the receipt, commit, record completion (`crates/hotsheet-cli/src/lib.rs:404-422`).
10. `--backup-remote` saves origin, pushes and verifies (`crates/hotsheet-cli/src/import_completion.rs:151`).
    Tests: `crates/hotsheet-cli/tests/migrate.rs`, `crates/hotsheet-cli/tests/import_recovery.rs`.

### Discrepancies

Each discrepancy names the follow-up ticket filed from HS2-TW2DKM. Item 7 is a model correction made in this baseline (HS2-RJB111).

1. `docs/04-core-server-cli.md` §4.4.1 omits 16 top-level commands:
   `claim-run`, `github-recover`, `github-webhook-setup`, `guidance`,
   `lifecycle`, `link-follow-up`, `merge-driver`, `note`, `notify`,
   `permission-discard`, `processes`, `provider-advisory-claim`, `remote`,
   `remote-change`, `reprice`, `terminal` (compare `crates/hotsheet-cli/src/main.rs:83-1016`). Follow-up: HS2-2N7MPG.
2. Seven MCP tools are not named in any requirements document:
   `hotsheet_advisory_claim_event`, `hotsheet_advisory_claim_status`,
   `hotsheet_announce`, `hotsheet_checkouts`, `hotsheet_provider_copy`,
   `hotsheet_provider_move`, `hotsheet_resolve_checkout`
   (`crates/hotsheet-mcp/src/lib.rs:148-604`). Follow-up: HS2-0P67G4.
3. The permission-hook question wait is fixed-interval network polling
   (350 ms, `crates/hotsheet-cli/src/dispatch/setup.rs:2189`), which the
   project rule forbids ("never fixed-interval network polling", `AGENTS.md`);
   `docs/05-ai-tool-plugins.md` describes it as polling. Follow-up: HS2-6D7454.
4. `launch` releases leftover claims and then calls `std::process::exit`
   (`crates/hotsheet-cli/src/dispatch/setup.rs:1049-1073`), so `main`'s
   post-command sync kick (`crates/hotsheet-cli/src/main.rs:1195`) never runs
   for those release commits. Follow-up: HS2-FVRH24.
5. The `serve` doc comment says it execs `hotsheet-server`, and sits on
   `ServeStop` rather than `cmd_serve`; the code spawns a child and waits
   (`crates/hotsheet-cli/src/dispatch/server.rs:67-69`, `:94-114`). Follow-up: HS2-T6NY9Z.
6. The crate description is stale: "init/new/ls/show/import"
   (`crates/hotsheet-cli/Cargo.toml:3`). Follow-up: HS2-T6NY9Z.
7. The shared model describes `orchLib` as holding coordination, but the
   coordination pool, its contract types and `OrchestrationRuntime` are in
   the `hotsheet-cli` crate (`crates/hotsheet-cli/src/coordination/contract.rs:10`,
   `crates/hotsheet-cli/src/orchestration.rs:186`).

Unresolved claims for this area are in the [register](../unresolved.md) (U-CLI-1, U-CLI-2, U-CLI-3).
