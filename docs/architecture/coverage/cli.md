# CLI and MCP shim: coverage

Status: written by HS2-TW2DKM against audited source `8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`.
`node scripts/architecture-coverage.mjs inventory --area cli` lists the
required entries; every one appears below in backticks. The chapter is
[30-cli](../narrative/30-cli.md).

Library note: the `hotsheet-cli` crate is a binary and a library. The server
links it (`crates/hotsheet-server/Cargo.toml:18`) for coordination,
orchestration, remote servers, process liveness and setup; the MCP shim links
it (`crates/hotsheet-mcp/Cargo.toml:30`) for the same plus host discovery,
sync kick and the notification watch. Components marked "Library" below run in
all three processes.

## Command tree and dispatch (`cliEntry`)

- Modules: `crates/hotsheet-cli/src/main.rs`, `crates/hotsheet-cli/src/dispatch` (router `crates/hotsheet-cli/src/dispatch/mod.rs`; every other entry is named in the component that owns its group),
  `crates/hotsheet-cli/src/lib.rs` (store resolution, launch helpers, Git helpers);
  small: `broken_pipe.rs`, `launched_env.rs`, `process_liveness.rs`, and
  `source_revision`, which is the server's file included by path
  (`crates/hotsheet-cli/src/lib.rs:38-40`).
- Responsibility: parse the Clap tree (`Cmd`, 106 top-level variants,
  `crates/hotsheet-cli/src/main.rs:83`), resolve the store, read the actor, run
  one group handler, then the post-command hooks. It does not hold domain
  logic. Every variant maps to exactly one `Group` through an exhaustive match,
  so a new command does not compile until routed
  (`crates/hotsheet-cli/src/dispatch/mod.rs:74-183`, `:264-283`).
- State and persistence: none of its own. Store resolution order is explicit
  `-C` other than `.`, then `HOTSHEET_STORE`, then the nearest
  `.hotsheet2/store` or legacy `.hotsheet/store` link walking up from cwd,
  then `.` (`crates/hotsheet-cli/src/lib.rs:234-254`). Commands that act on a
  literal path skip it (`crates/hotsheet-cli/src/dispatch/mod.rs:186-211`).
  Test: `crates/hotsheet-cli/src/lib.rs:624` (`explicit_c_wins_and_a_missing_link_is_the_requested_path`).
- Types and entry points: `main` (`crates/hotsheet-cli/src/main.rs:1141`),
  `Ctx` (`crates/hotsheet-cli/src/dispatch/mod.rs:48`), `Cmd::group`
  (`:74`), `run` (`:264`).
- Concurrency: parsing runs on a named thread with an 8 MiB stack because the
  Clap builder exceeds the Windows entry stack (`crates/hotsheet-cli/src/main.rs:1223-1236`).
  Everything else is synchronous on the main thread.
- Errors and recovery: `main` returns `anyhow::Result`, so any error prints
  `Error: …` and exits 1 (Rust's `Termination` for `Result`). Exceptions:
  `merge-driver` exits 0 or 1 for clean or conflicted merges
  (`crates/hotsheet-cli/src/dispatch/store.rs:1083`, `:1103`); `launch` exits
  with the tool's code or 128 plus signal
  (`crates/hotsheet-cli/src/dispatch/setup.rs:1073`, `crates/hotsheet-cli/src/lib.rs:130`);
  a closed stdout pipe exits 0 quietly (`crates/hotsheet-cli/src/broken_pipe.rs:19`,
  `:58`). `--test-fixture-capabilities`, `--build-revision` and revision status
  return before parsing (`crates/hotsheet-cli/src/main.rs:1149-1163`).
  Tests: `crates/hotsheet-cli/tests/broken_pipe.rs` (`listing_into_a_closed_pipe_ends_quietly_without_a_panic`),
  `crates/hotsheet-cli/src/lib.rs:804` (`a_wrapper_reports_its_childs_exit_code_or_signal`),
  `crates/hotsheet-cli/tests/revision_status.rs`.
- Configuration and environment: global `-C/--path` (default `.`),
  `--actor-role`/`--actor-id` falling back to `HOTSHEET_ACTOR_ROLE` and
  `HOTSHEET_ACTOR_ID` (`crates/hotsheet-cli/src/main.rs:44-80`); an id without
  a role is refused (`:69-71`). `HOTSHEET_STORE` (`crates/hotsheet-cli/src/lib.rs:238`).
  `HOTSHEET_HOME` (default `~/.hotsheet2`) through `hotsheet_plugins::hotsheet_home`
  (`crates/hotsheet-ticketing/src/machine_home.rs:33-35`). Hidden
  `--test-fixture-credentials` enables synthetic credentials in debug fixtures
  (`crates/hotsheet-cli/src/main.rs:40-42`). Launched-session variables are
  read through `launched_env::value`, which treats blank as unset
  (`crates/hotsheet-cli/src/launched_env.rs:6-22`).
- Invariants: the process journal is configured before dispatch except for
  the Windows guarded child (`crates/hotsheet-cli/src/main.rs:1168-1174`); the
  current executable is registered as the merge-driver program
  (`:1146-1148`). Test: `crates/hotsheet-cli/src/main.rs:1738`
  (`every_subcommand_has_non_empty_help`).

## Post-command sync kick and notification watch (`cliPostCommand`, Library)

- Modules: `crates/hotsheet-cli/src/sync_kick.rs`, `crates/hotsheet-cli/src/notify_watch.rs`.
- Responsibility: after every CLI command (and every serverless MCP write),
  ask a live host to sync each store the process committed to; and when no
  live server hosts the store, compute ticket transitions and deliver
  notifications locally. It never fails the command.
- State and persistence: reads `${HOTSHEET_HOME}/instances/*.json` and the
  notification configuration; writes the delivery log through
  `hotsheet_notify`. The committed-store set comes from
  `hotsheet_ticketing::take_headless_published_stores`
  (`crates/hotsheet-cli/src/sync_kick.rs:112-120`).
- Types and entry points: `request_host_sync` (`:38`), `kick` posting
  `/sync/kick` with `X-Hotsheet-Secret` and a 5 s timeout (`:60-72`, `:19`),
  `Watch::before`/`after` (`crates/hotsheet-cli/src/notify_watch.rs:94`,
  `:119`). Called from `crates/hotsheet-cli/src/main.rs:1189-1198`.
- Concurrency: synchronous; one HTTP request per candidate host.
- Errors and recovery: with no accepting host the CLI prints that commits may
  be local only and names `hotsheet-cli sync`; the kick runs even when the
  command failed after a commit (`crates/hotsheet-cli/src/main.rs:1193-1195`).
  A notification failure is a stderr warning (`crates/hotsheet-cli/src/notify_watch.rs:119-123`).
  Tests: `crates/hotsheet-cli/src/sync_kick.rs:129` (`hs2_d9zgae_no_host_reports_local_only_commits_and_names_sync`),
  `crates/hotsheet-mcp/tests/serverless_sync_kick.rs`, `crates/hotsheet-mcp/tests/serverless_notifications.rs`.
- Configuration and environment: `HOTSHEET_HOME`.
- Invariants: a host is tried in order: anchored on the store, the machine
  host, then any other live host (`crates/hotsheet-cli/src/sync_kick.rs:38-58`).
  The watch only acts when no instance record whose `store_path` is the store
  has a live pid (`crates/hotsheet-cli/src/notify_watch.rs:51-72`); see
  Unresolved U-CLI-1 in the chapter.

## Local host discovery and server commands (`cliHostClient`, Library)

- Modules: `crates/hotsheet-cli/src/external_launch.rs`, `crates/hotsheet-cli/src/dispatch/server.rs`.
- Responsibility: find, start and call the local Hot Sheet host. Commands:
  `serve`, `cert`, `commands`, `terminal`, `lifecycle`, `ticket-flow`,
  `live-state`, `ci report`, `live-history`, `activity`, `notifications`,
  `processes` (`crates/hotsheet-cli/src/dispatch/server.rs:14-75`).
- State and persistence: reads instance records
  `${HOTSHEET_HOME}/instances/<sha16(canonical store)>.json` with `pid`, `url`,
  `secret`, `store_path` (`crates/hotsheet-cli/src/external_launch.rs:15-36`);
  `serve --list` prunes stale records (`crates/hotsheet-cli/src/dispatch/server.rs:588`, `:777`).
  `cert` writes mTLS material under `${HOTSHEET_HOME}/tls/<project-id>/`
  (`:118-120`). `live-history` reads local files, not the server (`:975-998`).
- Types and entry points: `discover_running_server` (`crates/hotsheet-cli/src/external_launch.rs:42`),
  `discover_local_host` with `HostScope::Machine|Project` (`:114-152`),
  `ensure_running_server` (`:154`), `server_json_with_body` (`crates/hotsheet-cli/src/dispatch/server.rs:1075`),
  `cmd_serve` (`:78`).
- Concurrency: `ensure_running_server` spawns `hotsheet-cli -C <anchor> serve
  --bind 127.0.0.1:0` detached and checks the instance record every 100 ms
  for up to 10 s (`crates/hotsheet-cli/src/external_launch.rs:177-230`).
  `serve` runs `hotsheet-server` as a foreground child and waits
  (`crates/hotsheet-cli/src/dispatch/server.rs:94-114`).
- Errors and recovery: a missing or stale record, or one naming another store,
  is an error that names `hotsheet-cli serve` (`crates/hotsheet-cli/src/external_launch.rs:44-67`).
  `serve` refuses a `hotsheet-server` whose `--version` differs
  (`crates/hotsheet-cli/src/dispatch/server.rs:817-840`). A non-2xx host reply
  becomes `Hot Sheet server request failed: <error>` (`:1093-1101`); host calls
  time out after 10 s (`:1084`).
- Configuration and environment: `PATH` to find `hotsheet-server` when it is
  not beside the CLI (`crates/hotsheet-cli/src/dispatch/server.rs:91`, `:793-815`);
  `HOTSHEET_TEST_SERVER_OWNER_PID` captures a fixture server's stderr
  (`crates/hotsheet-cli/src/external_launch.rs:181`).
- Invariants: the machine host anchors on `${HOTSHEET_HOME}/server-bootstrap.hs2`
  (`crates/hotsheet-cli/src/external_launch.rs:108-110`); project routes try
  the store's host first, host-wide routes the machine host first (`:128-139`).
  Authentication is the `X-Hotsheet-Secret` header, not a bearer header
  (`crates/hotsheet-cli/src/dispatch/server.rs:1083`).
- Tests: `crates/hotsheet-cli/src/external_launch.rs:441` (`hs2_vg3e8n_per_store_host_wins_for_project_and_machine_host_for_host_wide`),
  `crates/hotsheet-cli/tests/cli.rs:2399` (`hs2_jttqmy_live_state_reads_the_local_server_snapshot`),
  `crates/hotsheet-cli/tests/cli.rs:2488` (`hs2_m8hhea_ci_report_posts_the_run_to_the_local_server`),
  `crates/hotsheet-cli/tests/cli.rs:5101` (`hs2_9an4dn_live_history_prints_segments_and_completions_without_a_server`).
  `serve` itself: untested in this crate.

## Store bootstrap, checkout and maintenance commands (`cliStoreCmds`)

- Modules: `crates/hotsheet-cli/src/dispatch/store.rs`, `crates/hotsheet-cli/src/dispatch/checkout_remove.rs`,
  `crates/hotsheet-cli/src/sync_ai.rs`, store helpers in `crates/hotsheet-cli/src/lib.rs`
  (`init_store`, `link_store`, `register_merge_driver`, `git_commit_all`).
- Responsibility: commands `init`, `bootstrap`, `link`, `activate-format`,
  `prefix`, `compatibility`, `import`, `checkout …`, `sync`, `doctor`,
  `reindex`, `worklist`, `purge-trash`, `merge-driver`
  (`crates/hotsheet-cli/src/dispatch/store.rs:18-110`). Opens the store
  directly; only `checkout remove`/`prune` consult live servers, and `sync`
  may ask a hosting server to start an AI repair.
- State and persistence: store files and Git; `.hotsheet2/store` link
  (`crates/hotsheet-cli/src/lib.rs:104`, `:259`); `${HOTSHEET_HOME}/checkouts.json`
  (`crates/hotsheet-cli/src/dispatch/store.rs:290-294`); `.gitattributes` and
  `info/attributes` merge-driver lines (`crates/hotsheet-cli/src/lib.rs:513`);
  `<git-dir>/hotsheet-sync-ai.log` for a detached repair (`crates/hotsheet-cli/src/sync_ai.rs:1-15`).
- Types and entry points: `cmd_bootstrap` (`crates/hotsheet-cli/src/dispatch/store.rs:191`),
  `cmd_init` (`:428`), `cmd_sync` (`:639`), `cmd_doctor` (`:813`),
  `cmd_checkout` (`:1137`), `cmd_checkout_remove` (`crates/hotsheet-cli/src/dispatch/checkout_remove.rs:26`),
  `ask_after_cli_sync` (`crates/hotsheet-cli/src/sync_ai.rs:71`).
- Concurrency: synchronous; store writes use the core's store lock. `sync`
  without a hosting server can start a detached `hotsheet-cli sync --run-repair`
  that outlives the command (`crates/hotsheet-cli/src/sync_ai.rs:8-12`).
- Errors and recovery: bootstrap is idempotent (`crates/hotsheet-cli/src/dispatch/store.rs:213-216`);
  without `--remote` and origin it prints the exact `git remote add` and
  `git push -u origin HEAD` steps (`:415-423`). `git_commit_all` warns and keeps
  written files when a commit fails (`crates/hotsheet-cli/src/lib.rs:559-567`).
  `sync --recover-stale-locks` and `--restore-saved-writes` are explicit
  recovery modes (`crates/hotsheet-cli/src/dispatch/store.rs:592`, `:617`).
- Configuration and environment: `HOTSHEET_HOME`; `-C` is resolved for most
  commands but not `init`, `bootstrap`, `link` or `checkout`
  (`crates/hotsheet-cli/src/dispatch/mod.rs:186-211`).
- Invariants: bootstrap's Git owner decides `git init`, never the presence of
  `<store>/.git` (`crates/hotsheet-cli/src/dispatch/store.rs:213-216`).
- Tests: `crates/hotsheet-cli/tests/cli.rs:7023` (`bootstrap_prepares_a_clean_project_headlessly_and_is_idempotent`),
  `crates/hotsheet-cli/tests/cli.rs:7180` (`bootstrap_prints_exact_remote_instructions_when_origin_is_absent`),
  `crates/hotsheet-cli/tests/cli.rs:6946` (`init_standalone_creates_git_store_links_project_and_sets_remote`),
  `crates/hotsheet-cli/tests/cli.rs:7754` (`doctor_reports_ok_on_a_healthy_store`),
  `crates/hotsheet-cli/tests/checkout_remove.rs` (`hs2_x4rz73_remove_is_idempotent_and_keeps_shared_sources_and_files`),
  `crates/hotsheet-cli/tests/store_admission.rs`, `crates/hotsheet-cli/tests/checkout_relocation.rs`.

## Ticket, note, attachment and group commands (`cliTicketCmds`)

- Modules: `crates/hotsheet-cli/src/dispatch/tickets.rs`, `crates/hotsheet-cli/src/dispatch/groups.rs`,
  `crates/hotsheet-cli/src/dispatch/usage_reroute.rs`.
- Responsibility: `new`, `link-follow-up`, `ls`, `show`, `note`, `batch`,
  `delete-note`, `attach`, `attachment-rename|delete|metadata|actor`,
  `annotate`, `edit`, `close`, `restore`, `copy`, `move`, `assign`, `people`,
  `read`, `groups`, `group`, `confidence-report`, `phase-report`, `metrics`,
  `reprice` (`crates/hotsheet-cli/src/dispatch/tickets.rs:25-116`). All open
  the Git store directly; none needs a server. `people seed-github` calls the
  GitHub API with the keychain token (`:473-495`).
- State and persistence: ticket files, attachments, `people.json`, the group
  registry under the code checkout (`crates/hotsheet-cli/src/dispatch/groups.rs:68`,
  `:97`), and the shared index `${HOTSHEET_HOME}/index/<project-id>.v<schema>.sqlite`
  (`crates/hotsheet-cli/src/lib.rs:80-100`). `metrics reroute-bootstrap-usage`
  moves usage rows between stores (`crates/hotsheet-cli/src/dispatch/usage_reroute.rs:105`).
- Types and entry points: `run` (`crates/hotsheet-cli/src/dispatch/tickets.rs:25`), `NewArgs` (`:1377`),
  `TicketEditArgs` (`:1457`), `GroupCmd` (`crates/hotsheet-cli/src/dispatch/groups.rs:21`).
- Concurrency: synchronous; write serialization is the core store lock.
- Errors and recovery: an AI completion without `--note-confidence` fails
  (see `docs/04-core-server-cli.md` §4.4); literal `\n` in a note is refused
  unless allowed (`crates/hotsheet-cli/src/main.rs:1380`).
- Configuration and environment: `HOTSHEET_ACTOR_ROLE`, `HOTSHEET_ACTOR_ID`
  (`crates/hotsheet-cli/src/dispatch/tickets.rs:709`, `:725`), `HOTSHEET_WORKER_ID`
  (`:558`); `CLAUDE_CONFIG_DIR` and `HOME` for usage reroute
  (`crates/hotsheet-cli/src/dispatch/usage_reroute.rs:52-54`).
- Invariants: read commands are excluded from the notification watch
  (`crates/hotsheet-cli/src/main.rs:1206-1221`).
- Tests: `crates/hotsheet-cli/tests/cli.rs:6865` (`init_new_ls_show_edit_close_flow`),
  `crates/hotsheet-cli/tests/cli.rs:8481` (`batch_reports_partial_results_and_preserves_actor_note_metadata`),
  `crates/hotsheet-cli/tests/groups.rs` (`hs2_9kzsd4_a_bare_store_takes_ids_and_refuses_names`),
  `crates/hotsheet-cli/tests/cli.rs:3776` (`trash_restore_and_purge_have_headless_cli_parity`).

## Ticket claim commands and guarded runs (`cliClaimCmds`)

- Modules: `crates/hotsheet-cli/src/dispatch/claims.rs`, `crates/hotsheet-cli/src/dispatch/claim_process.rs`,
  `crates/hotsheet-cli/src/provisional_claims.rs` (Library; the server and MCP
  also call it).
- Responsibility: `claim`, `claim-next`, `renew`, `release`, `claim-run`, and
  the hidden Windows `claim-run-child` (`crates/hotsheet-cli/src/dispatch/claims.rs:20-78`).
  Claim answers name provisional prerequisites but never change what is
  claimed (`crates/hotsheet-cli/src/provisional_claims.rs:1-6`).
- State and persistence: claim fields and history in the ticket file.
- Types and entry points: `cmd_claim_run` (`crates/hotsheet-cli/src/dispatch/claims.rs:81`),
  `GuardedChild` (`crates/hotsheet-cli/src/dispatch/claim_process.rs:12`).
- Concurrency: `claim-run` requires the caller's exact live claim attempt,
  spawns the command's process tree, renews on a background renewal at a
  third of the lease (1 to 60 s early), and wakes at most every 250 ms
  (`crates/hotsheet-cli/src/dispatch/claims.rs:88-170`). On Windows the child
  joins a Job Object through a start gate (`crates/hotsheet-cli/src/dispatch/claim_process.rs:140`).
- Errors and recovery: a failed renewal stops the process tree and fails; a
  non-zero child fails without inferring authority; a final renewal confirms
  authority after a clean exit (`crates/hotsheet-cli/src/dispatch/claims.rs:123-150`).
- Configuration and environment: debug-only `HOTSHEET_TEST_CLAIM_RUN_PARENT_READY_PATH`
  and `HOTSHEET_TEST_CLAIM_RUN_RENEWAL_MARGIN_SECONDS` (`:113`, `:183`).
- Invariants: the claim attempt id must match through every renewal.
- Tests: `crates/hotsheet-cli/tests/claim_run.rs:338` (`hs2_mfyyj9_delayed_child_renews_the_same_claim_attempt_and_reconnects`),
  `crates/hotsheet-cli/tests/cli.rs:7840` (`claim_next_release_and_renew`),
  `crates/hotsheet-cli/tests/claim_provisional.rs` (`hs2_xyf4yt_cli_claim_and_renew_name_a_prerequisite_awaiting_ci`),
  `crates/hotsheet-cli/tests/ticket_claim_backend.rs`.

## Provider, key and account commands (`cliProviderCmds`)

- Modules: `crates/hotsheet-cli/src/dispatch/providers.rs`, `crates/hotsheet-cli/src/feedback_synthesis.rs`.
- Responsibility: `providers`, `github-sign-in`, `github-connect`,
  `github-recover`, `github-webhook-setup`, `provider-connect|attach|ls|get|
  disable|enable|remove|new|edit|close|assign|advisory-claim|restore|
  report-not-working|delete-note|copy|move`, `feedback-synthesis`, `key`,
  `account` (`crates/hotsheet-cli/src/dispatch/providers.rs:29-230`). Calls
  provider adapters directly; no server.
- State and persistence: provider configuration (`providers.json`), keychain
  references, private feedback-synthesis files (`crates/hotsheet-cli/src/feedback_synthesis.rs:1-2`).
- Types and entry points: `run` (`crates/hotsheet-cli/src/dispatch/providers.rs:29`),
  `ProviderConnectArgs` (`:1643`), `KeyCmd` (`:1843`), `AccountCmd` (`:1821`).
- Concurrency: synchronous HTTPS calls through the adapters.
- Errors and recovery: provider errors are returned as command errors.
- Configuration and environment: keys from the OS credential store; no
  environment variables in this module.
- Invariants: cross-provider copy and move require an explicit command and
  carry provenance (project rule in `AGENTS.md`).
- Tests: `crates/hotsheet-cli/tests/cli.rs:5192` (`providers_reports_default_git_connection_and_capabilities`),
  `crates/hotsheet-cli/tests/cli.rs:5372` (`provider_copy_retries_use_one_destination_ticket`),
  `crates/hotsheet-cli/tests/key_metadata.rs`, `crates/hotsheet-cli/tests/feedback_synthesis.rs`.

## AI tool setup, launch and drive (`cliSetupCmds`)

- Modules: `crates/hotsheet-cli/src/dispatch/setup.rs` (except the hook
  functions), `crates/hotsheet-cli/src/dispatch/setup_diagnose.rs`,
  `crates/hotsheet-cli/src/setup.rs`, `crates/hotsheet-cli/src/plugin.rs`,
  `crates/hotsheet-cli/src/workloop.rs`, `crates/hotsheet-cli/src/dispatch/terminal_relay.rs`,
  `crates/hotsheet-cli/src/dispatch/screensaver.rs`.
- Responsibility: `setup` (and `--refresh`, `--diagnose`), `guidance`,
  `plugin`, `ai-tools`, `ai-settings`, `settings`, `host-disks`,
  `terminal-relay`, `screensaver`, `permission-discard`, `launch`, `trigger`,
  `work` (`crates/hotsheet-cli/src/dispatch/setup.rs:472-531`). Setup writers
  are `hotsheet_plugins::run_setup`, shared with the server
  (`crates/hotsheet-cli/src/setup.rs:1-6`).
- State and persistence: instruction files, skills, hooks and MCP config in
  the project; settings; `${HOTSHEET_HOME}/server-bootstrap.hs2`; plugin
  directory copies (`crates/hotsheet-cli/src/plugin.rs:20`).
- Types and entry points: `cmd_setup` (`crates/hotsheet-cli/src/dispatch/setup.rs:1163`),
  `cmd_launch` (`:954`), `cmd_trigger` (`:533`), `cmd_work` (`:593`),
  `notify_running_servers` (`:1626`).
- Concurrency: `launch` initializes the bootstrap store, runs setup, ensures
  the machine host, posts `/projects/open`, spawns the tool with
  `HOTSHEET_SERVER`, `HOTSHEET_SECRET`, `HOTSHEET_PROJECT`,
  `HOTSHEET_USAGE_STORES`, `HOTSHEET_PERMISSION_PROJECT`, `HOTSHEET_WORKER_ID`
  and AI actor variables, ignores SIGINT, SIGQUIT and SIGHUP while waiting,
  then releases the worker's leftover claims (`:987-1072`). `work` drives up to
  `--max` turns and stops after `--max-stall` turns without queue change
  (`:632-701`, `crates/hotsheet-cli/src/workloop.rs:1-6`).
- Errors and recovery: `launch` ends with `std::process::exit`, so `main`'s
  post-command sync kick does not run after its claim release
  (`crates/hotsheet-cli/src/dispatch/setup.rs:1073`). Setting changes that
  running servers miss are a warning (`:1536-1540`).
- Configuration and environment: `HOTSHEET_HOME`, `CODEX_HOME`,
  `CLAUDE_CODE_MESSAGING_SOCKET`, `CLAUDE_CODE_MESSAGING_TOKEN`, `AI_AGENT`,
  `CLAUDE_CODE_EXECPATH` read for connection reports (`:2122-2136`).
- Invariants: launch records the bootstrap store's server, never the checkout
  (`:1036`).
- Tests: `crates/hotsheet-cli/tests/cli.rs:6188` (`launch_opens_the_project_and_injects_the_running_server`),
  `crates/hotsheet-cli/tests/cli.rs:6435` (`launch_releases_the_claims_its_session_left_behind`),
  `crates/hotsheet-cli/tests/cli.rs:4078` (`setup_refresh_is_headless_and_idempotently_repairs_managed_artifacts`),
  `crates/hotsheet-cli/tests/cli.rs:1686` (`setup_diagnose_is_read_only_and_predicts_refresh_paths`),
  `crates/hotsheet-cli/tests/cli.rs:7426` (`work_drains_empty_queue_and_requires_setup_when_there_is_work`),
  `crates/hotsheet-cli/tests/terminal_relay.rs`, `crates/hotsheet-cli/tests/screensaver.rs`,
  `crates/hotsheet-cli/tests/plugin_conformance.rs`.

## Permission hook adapter (`cliPermissionHook`)

- Modules: `crates/hotsheet-cli/src/permission_hook.rs` (pure mapping, also
  used by the server), `cmd_permission_hook` and helpers in
  `crates/hotsheet-cli/src/dispatch/setup.rs:708-951`, `:1986-2206`,
  `cmd_hook_diagnose` (`:1078`).
- Responsibility: one process per hook event. Session lifecycle events report
  halt, resume, start and end to `/terminals/{id}/halt` and
  `/terminals/{id}/ai-connection`; a question waits for a browser answer;
  a permission event posts `/permissions/ask` and prints the decision
  (`crates/hotsheet-cli/src/dispatch/setup.rs:724-826`).
- State and persistence: none.
- Types and entry points: `HookDecision` (`crates/hotsheet-cli/src/permission_hook.rs:12`),
  `permission_hook_event` (`:35`), `should_bridge_permission` (`:123`),
  `hook_server_route` (`crates/hotsheet-cli/src/dispatch/setup.rs:1986`),
  `ask_server` (`:2194`).
- Concurrency: `ask_server` sets no timeout and blocks until the server
  answers (`:2194-2206`). The question wait polls `/terminals/{id}/question`
  every 350 ms for up to 86,400 s (`:2163-2192`).
- Errors and recovery: on route failure an interactive hook prints nothing and
  the tool shows its native prompt; under `HOTSHEET_CLAUDE_HEADLESS_AUTO=1`
  with the Claude agent a PermissionRequest is denied (`:762-786`). A failed
  question poll deletes the notice and returns to the native prompt
  (`:2179-2186`).
- Configuration and environment: `HOTSHEET_PROJECT`, `HOTSHEET_PERMISSION_PROJECT`,
  `HOTSHEET_SERVER`, `HOTSHEET_SECRET`, `HOTSHEET_TERMINAL_ID`, `HOTSHEET_AGENT`,
  `HOTSHEET_CLAUDE_PRETOOLUSE`, `HOTSHEET_CLAUDE_HEADLESS_AUTO`
  (`:727`, `:763-764`, `:774-775`, `:1987`, `:2008-2009`).
- Invariants: the live instance record wins over inherited variables so a
  retained terminal follows a restarted server (`:1986-2012`); PreToolUse is
  only bridged when `HOTSHEET_CLAUDE_PRETOOLUSE=1` (`:762-770`).
- Tests: `crates/hotsheet-cli/src/permission_hook.rs:326` (unit module),
  `crates/hotsheet-cli/tests/cli.rs:2749` (`headless_claude_auto_denies_an_unanswered_native_permission_request`),
  `crates/hotsheet-cli/tests/cli.rs:2789` (`retained_permission_hook_uses_restarted_server_route`),
  `crates/hotsheet-cli/tests/cli.rs:2999` (`permission_hook_returns_all_browser_answers_to_claude`),
  `crates/hotsheet-cli/tests/hook_diagnose.rs` (`headless_probe_distinguishes_bridge_from_terminal_hook_and_mcp`).

## Local coordination pool (`cliCoordination`, Library)

- Modules: `crates/hotsheet-cli/src/coordination` (`crates/hotsheet-cli/src/coordination/mod.rs`,
  `crates/hotsheet-cli/src/coordination/contract.rs`, `crates/hotsheet-cli/src/coordination/batch.rs`,
  `crates/hotsheet-cli/src/coordination/batch_staged.rs`, `crates/hotsheet-cli/src/coordination/preferences.rs`,
  `crates/hotsheet-cli/src/coordination/snapshot.rs`, `crates/hotsheet-cli/src/coordination/speculation.rs`,
  `crates/hotsheet-cli/src/coordination/speculation_runtime.rs`, `crates/hotsheet-cli/src/coordination/strategy.rs`,
  `crates/hotsheet-cli/src/coordination/worker_view.rs`; small: `authority.rs`,
  `development_strategy.rs`, `identity.rs`, `next_plan.rs`, `retire.rs`,
  `review_return.rs`, `scope.rs`, `source_support.rs`, `speculation_api.rs`,
  `wait.rs`); dispatch `crates/hotsheet-cli/src/dispatch/coordination.rs`,
  `crates/hotsheet-cli/src/dispatch/coordination_advisory.rs`,
  `crates/hotsheet-cli/src/dispatch/coordination_batch.rs`,
  `crates/hotsheet-cli/src/dispatch/coordination_claims.rs`,
  `crates/hotsheet-cli/src/dispatch/coordination_readiness.rs`,
  `crates/hotsheet-cli/src/dispatch/coordination_speculation.rs`;
  `crates/hotsheet-cli/src/merge_pending.rs`.
- Responsibility: host side of `coordination` (55 subcommands: setup, status,
  workers, register, next, assign, publish, authority, strategies, batch,
  speculation, advisory review, preflight, finish, leave and more,
  `crates/hotsheet-cli/src/dispatch/coordination.rs:182`). Headless and local:
  no server, provider token or AI launcher (`:1`). The domain types live in
  this crate (`crates/hotsheet-cli/src/coordination/contract.rs:10`, `:22`),
  not in `hotsheet-ticketing`; rule logic is delegated to
  `hotsheet_ticketing::provisional` (`crates/hotsheet-cli/src/coordination/speculation.rs:24`).
  Orchestration chapter: [20-orchestration](../narrative/20-orchestration.md).
- State and persistence: `<git-common-dir>/hotsheet-coordination/` holding
  `state.json`, `batches.json`, `speculation.json`, `identity-migrations.log`,
  `state.lock` and `wait-<sha>.lock` (`crates/hotsheet-cli/src/coordination/mod.rs:146-187`,
  `crates/hotsheet-cli/src/coordination/batch.rs:29`, `crates/hotsheet-cli/src/coordination/speculation.rs:32`);
  speculation keys in branch `git config` (`crates/hotsheet-cli/src/coordination/speculation_runtime.rs:24-26`).
- Types and entry points: `Coordinator` (`crates/hotsheet-cli/src/coordination/mod.rs:77`),
  `open` (`:114`), dispatch `run` (`crates/hotsheet-cli/src/dispatch/coordination.rs:809`),
  `prune` for merge-pending branches (`crates/hotsheet-cli/src/merge_pending.rs:1-6`).
- Concurrency: every pool write takes the `state.lock` file lock and persists
  through tempfile, fsync and rename (`crates/hotsheet-cli/src/coordination/mod.rs:172-187`).
  `next` waits on filesystem events, not network polling (`crates/hotsheet-cli/src/coordination/wait.rs:1-2`).
- Errors and recovery: `repair-identity` reads a pool an ordinary open
  refuses; `recover`, `batch-recover` and `speculation-reopen` are explicit
  recovery commands (`crates/hotsheet-cli/src/dispatch/coordination.rs:182`).
  `capabilities` answers without opening the pool, because opening may migrate
  it (`:813-817`).
- Configuration and environment: committed project settings for strategy and
  speculation policy; no environment variables.
- Invariants: the pool holds identifiers only, never credentials
  (`crates/hotsheet-cli/src/coordination/contract.rs:1`).
- Tests: `crates/hotsheet-cli/src/coordination/tests.rs`, `crates/hotsheet-cli/src/coordination/batch_tests.rs`,
  `crates/hotsheet-cli/src/coordination/speculation_tests.rs`, `crates/hotsheet-cli/tests/coordination.rs`
  (`hs2_xcmavt_real_cli_plan_revision_and_dispatch_replay`), `crates/hotsheet-cli/tests/coordination_batch.rs`
  (`hs2_9g0m3v_batch_flow_collects_pushes_once_and_gates_red_completion_on_follow_ups`),
  `crates/hotsheet-cli/tests/coordination_speculation.rs`, `crates/hotsheet-cli/tests/coordination_advisory.rs`,
  `crates/hotsheet-cli/tests/coordination_readiness.rs`, `crates/hotsheet-cli/src/merge_pending_tests.rs`.

## Orchestration runtime composition (`cliOrchestration`, Library)

- Modules: `crates/hotsheet-cli/src/orchestration.rs`, `crates/hotsheet-cli/src/orchestration`
  (`crates/hotsheet-cli/src/orchestration/access.rs`, `crates/hotsheet-cli/src/orchestration/assignment.rs`,
  `crates/hotsheet-cli/src/orchestration/capacity.rs`, `crates/hotsheet-cli/src/orchestration/gates.rs`,
  `crates/hotsheet-cli/src/orchestration/permissions.rs`, `crates/hotsheet-cli/src/orchestration/plan_update.rs`,
  `crates/hotsheet-cli/src/orchestration/planning.rs`, `crates/hotsheet-cli/src/orchestration/projects.rs`,
  `crates/hotsheet-cli/src/orchestration/provisional.rs`, `crates/hotsheet-cli/src/orchestration/readiness.rs`,
  `crates/hotsheet-cli/src/orchestration/reconcile_sources.rs`, `crates/hotsheet-cli/src/orchestration/recovery.rs`,
  `crates/hotsheet-cli/src/orchestration/request.rs`, `crates/hotsheet-cli/src/orchestration/runs.rs`,
  `crates/hotsheet-cli/src/orchestration/scheduler.rs`, `crates/hotsheet-cli/src/orchestration/session_executor.rs`,
  `crates/hotsheet-cli/src/orchestration/sessions.rs`, `crates/hotsheet-cli/src/orchestration/site_pool.rs`,
  `crates/hotsheet-cli/src/orchestration/tickets.rs`, `crates/hotsheet-cli/src/orchestration/validation_runner.rs`,
  `crates/hotsheet-cli/src/orchestration/validation.rs`; small: `auto_replan.rs`,
  `cancel_hold.rs`, `capacity_watch.rs`, `decisions.rs`, `hosted_audit.rs`,
  `hosted_deployments.rs`, `hosted_gates.rs`, `job_monitor.rs`, `joining.rs`,
  `line_push.rs`, `observer.rs`, `policy.rs`, `readiness_timer.rs`,
  `resources.rs`, `scheduler_apply.rs`, `scheduler_dispositions.rs`,
  `session_control.rs`, `session_worktrees.rs`, `step_job.rs`,
  `step_process.rs`, `step_program.rs`, `summary.rs`, `target_lines.rs`,
  `ticket_list.rs`, `timeline.rs`, `tool_capacity.rs`, `validation_output.rs`,
  `watch.rs`); dispatch `crates/hotsheet-cli/src/dispatch/orchestration.rs`,
  `crates/hotsheet-cli/src/dispatch/orchestration_capacity.rs`,
  `crates/hotsheet-cli/src/dispatch/orchestration_planning.rs`,
  `crates/hotsheet-cli/src/dispatch/orchestration_projects.rs`,
  `crates/hotsheet-cli/src/dispatch/orchestration_readiness.rs`,
  `crates/hotsheet-cli/src/dispatch/orchestration_scheduler.rs`,
  `crates/hotsheet-cli/src/dispatch/orchestration_sessions.rs`,
  `crates/hotsheet-cli/src/dispatch/orchestration_validation.rs`.
- Responsibility: `orchestration` with 23 subcommand families
  (`crates/hotsheet-cli/src/dispatch/orchestration.rs:30`). Each family shapes
  an HTTP-like `(method, path, body)` that the in-process
  `OrchestrationRuntime::request_now` executes, the same routes the server
  serves (`:1451-1648`, `:1689-1735`). No server is required. Domain rules are
  `hotsheet_ticketing::orchestration` (`crates/hotsheet-cli/src/orchestration.rs:174`);
  module-by-module domain detail is in the orchestration chapter.
- State and persistence: `${HOTSHEET_HOME}/remotes.json` (`crates/hotsheet-cli/src/orchestration.rs:245`),
  audit and hosted journals, `orchestration-validation-steps/` and
  `orchestration-validation-output/` (private files, atomic rename claim,
  14-day retention, `crates/hotsheet-cli/src/orchestration/validation_output.rs:24`),
  wake and signal tokens (`crates/hotsheet-cli/src/orchestration/scheduler.rs:43`,
  `crates/hotsheet-cli/src/orchestration/permissions.rs:111`).
- Types and entry points: `OrchestrationRuntime` (`crates/hotsheet-cli/src/orchestration.rs:186`),
  `live` (`:433`), dispatch `run` (`crates/hotsheet-cli/src/dispatch/orchestration.rs:1441`),
  `step-runner` (`:1452-1459`).
- Concurrency: waits are long polls capped at 8 to 25 s per request (for
  example `crates/hotsheet-cli/src/orchestration/scheduler.rs:41`); the CLI
  caps `--wait-seconds` at 20 (`crates/hotsheet-cli/src/dispatch/orchestration.rs:1620`).
  Named threads in the server-hosted runtime: capacity wait, claim wait,
  readiness lapse (`crates/hotsheet-cli/src/orchestration/capacity_watch.rs:144-145`,
  `crates/hotsheet-cli/src/orchestration/resources.rs:412-413`,
  `crates/hotsheet-cli/src/orchestration/readiness_timer.rs:65-68`). Detached
  children: `hotsheet-cli job monitor` and `hotsheet-cli orchestration
  step-runner` (`crates/hotsheet-cli/src/orchestration/job_monitor.rs:87-92`,
  `crates/hotsheet-cli/src/orchestration/validation_runner.rs:329-339`).
- Errors and recovery: errors print as `<code>: <message>`
  (`crates/hotsheet-cli/src/dispatch/orchestration.rs:1737`); a failed monitor
  start leaves `monitor_error: job_monitor_unavailable` and the gate pending
  (`:1738-1747`); an invalid profile file fails the command (`:1753-1758`).
  Cleanup requests run before `-C` is even resolved (`:1656-1660`).
- Configuration and environment: `HOTSHEET_JOB_MONITOR_AUTOSTART`
  (`crates/hotsheet-cli/src/orchestration/job_monitor.rs:19`; default on,
  except test-vault builds), `HOTSHEET_VALIDATION_EXECUTION_ID`
  (`crates/hotsheet-cli/src/orchestration/validation.rs:329`), `HOTSHEET_HOME`
  passed to children (`crates/hotsheet-cli/src/orchestration/validation_runner.rs:339`).
- Invariants: a non-Git source in the owner checkout needs a configured
  provider registry, else nothing is created (`crates/hotsheet-cli/src/dispatch/orchestration.rs:1684-1690`).
- Tests: `crates/hotsheet-cli/src/orchestration/tests.rs` and
  `crates/hotsheet-cli/src/orchestration/tests/` (for example
  `crates/hotsheet-cli/src/orchestration/tests/scheduler.rs`,
  `crates/hotsheet-cli/src/orchestration/tests/validation.rs`,
  `crates/hotsheet-cli/src/orchestration/tests/sessions.rs`), `crates/hotsheet-cli/tests/job_monitor.rs`
  (`start_and_stop_are_idempotent`). The dispatch request shaping has no
  dedicated CLI process test found: untested at the process level.

## Remote server registry and transport (`cliRemotes`, Library)

- Modules: `crates/hotsheet-cli/src/remote_servers.rs`, `crates/hotsheet-cli/src/dispatch/remotes.rs`.
- Responsibility: `remote bridge-connection|sessions|identity|list|
  selected-projects|select-project|unselect-project|register|review|revoke|
  checkouts|session|renew` (`crates/hotsheet-cli/src/dispatch/remotes.rs:20`).
  `remote sessions …` call the local host's `remote-auth/owner` routes
  (`:144-195`); the rest run `RemoteRuntime` in-process (`:137`).
- State and persistence: `${HOTSHEET_HOME}/remotes.json` and keychain
  credentials (`crates/hotsheet-cli/src/remote_servers.rs:25-30`).
- Types and entry points: `RemoteRuntime::live` (`crates/hotsheet-cli/src/dispatch/remotes.rs:137`),
  `remote_auth_request` (`crates/hotsheet-cli/src/remote_servers.rs:647`).
- Concurrency: synchronous ureq calls with 5 s connect, 10 s read, 5 s
  write, 15 s total (`crates/hotsheet-cli/src/remote_servers.rs:521-524`).
- Errors and recovery: 401/403 map to expired, revoked or unauthorized by the
  peer's `code` (`:634-644`); redirects are refused (`:520`).
- Configuration and environment: none beyond `HOTSHEET_HOME`.
- Invariants: a pinned CA or public roots; mTLS when a client certificate is
  present; paired session tokens travel as bearer and cookie
  (`:525-571`, `:647-675`).
- Tests: `crates/hotsheet-cli/src/remote_servers/tests.rs` (`hs2_w4jjje_remote_session_uses_cookie_without_forwarding_machine_bearer`).

## Job and CI watch commands (`cliJobs`)

- Modules: `crates/hotsheet-cli/src/dispatch/jobs.rs`, `crates/hotsheet-cli/src/dispatch/job_github.rs`,
  `crates/hotsheet-cli/src/dispatch/job_gitlab.rs`, `crates/hotsheet-cli/src/dispatch/job_ci_rest.rs`.
- Responsibility: `job list|show|watch|monitor` (`crates/hotsheet-cli/src/dispatch/jobs.rs:30`).
  Needs no server or ticket store (`:1-4`).
- State and persistence: the machine Job store from `hotsheet_jobs`.
- Types and entry points: `run` (`:180`), `UreqGithubApi` (`crates/hotsheet-cli/src/dispatch/job_github.rs:25`),
  `RestProvider` (`crates/hotsheet-cli/src/dispatch/job_ci_rest.rs:64`).
- Concurrency: the monitor runs as a detached process when started by the
  orchestration runtime; tokens are kept in memory only (`crates/hotsheet-cli/src/dispatch/job_github.rs:1-4`).
- Errors and recovery: no token means a 60 s retry (`crates/hotsheet-cli/src/dispatch/job_github.rs:19-20`).
- Configuration and environment: `GH_TOKEN`, `GITHUB_TOKEN`, else `gh`
  (`crates/hotsheet-cli/src/dispatch/job_github.rs:48`, `:67`);
  `HOTSHEET_GITHUB_API_URL` (default `https://api.github.com`, `:14-16`);
  `GITLAB_TOKEN`, else `glab` (`crates/hotsheet-cli/src/dispatch/job_gitlab.rs:19`, `:70`);
  `HOTSHEET_GITLAB_API_URL` (`:17`, default `https://gitlab.com/api/v4`);
  `HOTSHEET_BUILDKITE_API_URL`, `HOTSHEET_CIRCLECI_API_URL`, `HOTSHEET_JENKINS_URL`
  (no default), `HOTSHEET_AZURE_DEVOPS_API_URL`, `HOTSHEET_BITBUCKET_API_URL`
  (`crates/hotsheet-cli/src/dispatch/job_ci_rest.rs:72-125`); tokens
  `BUILDKITE_API_TOKEN`, `CIRCLECI_TOKEN`, `JENKINS_USER`, `JENKINS_API_TOKEN`,
  `AZURE_DEVOPS_EXT_PAT`, `BITBUCKET_ACCESS_TOKEN`, `BITBUCKET_API_TOKEN`,
  `BITBUCKET_USERNAME` (`:100-131`, `:294-302`, `:386`); debug-only
  `HOTSHEET_JOB_WATCH_FIXTURES` (`crates/hotsheet-cli/src/dispatch/jobs.rs:579`).
- Invariants: API roots are HTTPS except loopback (`crates/hotsheet-cli/src/dispatch/job_ci_rest.rs:1-7`).
- Tests: `crates/hotsheet-cli/tests/job_ci_watch.rs` (`a_commit_is_watched_to_a_green_gate_through_the_real_http_client`),
  `crates/hotsheet-cli/tests/job_gitlab_token.rs`, `crates/hotsheet-cli/tests/job_monitor.rs`.

## Resource claim commands (`cliResources`)

- Modules: `crates/hotsheet-cli/src/dispatch/resources.rs`, `crates/hotsheet-cli/src/dispatch/resource_backends.rs`,
  `crates/hotsheet-cli/src/dispatch/resource_remote.rs`.
- Responsibility: `resource acquire|renew|release|show|list|backend|plugin|
  namespace|source` through `ClaimService` against the machine home, with no
  server (`crates/hotsheet-cli/src/dispatch/resources.rs:1-4`, `:22`);
  `renew|release --server <url>` use only the renewal token as a bearer
  (`crates/hotsheet-cli/src/dispatch/resource_remote.rs:1-8`).
- State and persistence: claim records and backend registry
  (`hotsheet_ticketing::claim_backends`); keys in the OS credential store
  (`crates/hotsheet-cli/src/dispatch/resource_backends.rs:1-4`).
- Types and entry points: `run` (`crates/hotsheet-cli/src/dispatch/resources.rs:123`),
  `validate_server` (`crates/hotsheet-cli/src/dispatch/resource_remote.rs:24`), `call` (`:103`).
- Concurrency: `--wait` blocks inside `ClaimService`.
- Errors and recovery: a held resource fails at once without `--wait`
  (`crates/hotsheet-cli/src/dispatch/resources.rs:24-26`).
- Configuration and environment: `HOTSHEET_RESOURCE_CLAIM_TOKEN` (`:19`).
- Invariants: remote URL must be HTTPS, loopback HTTP, or explicitly
  `--allow-insecure-http` (`crates/hotsheet-cli/src/dispatch/resource_remote.rs:6-8`).
- Tests: `crates/hotsheet-cli/tests/resource_claims.rs` (`hs2_fmhh9y_acquire_prints_the_grant_and_a_conflict_names_the_holder`),
  `crates/hotsheet-cli/tests/resource_backends.rs` (`hs2_jjwesd_configuration_lifecycle_is_idempotent_and_secret_free`).

## Notifier and remote-change commands (`cliNotify`)

- Modules: `crates/hotsheet-cli/src/dispatch/notify.rs`, `crates/hotsheet-cli/src/dispatch/remote_change.rs`.
- Responsibility: `notify notifiers|config|channel|rule|send|link-base|
  project-timing|deliveries` and `remote-change plugins|list|connection|bind|
  unbind`, through `hotsheet_notify` workflows shared with the server, with no
  server (`crates/hotsheet-cli/src/dispatch/notify.rs:1-4`, `crates/hotsheet-cli/src/dispatch/remote_change.rs:1-4`).
- State and persistence: machine notification configuration and delivery log.
- Types and entry points: `run` (`crates/hotsheet-cli/src/dispatch/notify.rs:255`,
  `crates/hotsheet-cli/src/dispatch/remote_change.rs:119`).
- Concurrency: synchronous.
- Errors and recovery: idempotent commands; credentials never printed
  (`crates/hotsheet-cli/src/dispatch/remote_change.rs:2-3`).
- Configuration and environment: `HOTSHEET_HOME`.
- Invariants: JSON output.
- Tests: `crates/hotsheet-cli/tests/notify.rs` (`headless_setup_send_test_and_removal_through_an_external_plugin`),
  `crates/hotsheet-cli/tests/remote_change.rs`.

## Stdio loop and backend selection (`mcpEntry`)

- Modules: `crates/hotsheet-mcp/src/main.rs` (including `ProjectBackend`).
- Responsibility: parse `-C/--path`, `--project`, `--server`, `--secret`
  (`crates/hotsheet-mcp/src/main.rs:22-35`), choose the backend
  (`:146-173`), and run the line loop (`:175-203`).
- State and persistence: none; reads `checkouts.json`, `server-bootstrap.hs2`
  and `instances/*.json` under `HOTSHEET_HOME`.
- Types and entry points: `ProjectBackend` (`:273`), `launched_route` (`:229`),
  `attach_route` (`:255`).
- Concurrency: one thread for the loop; presence adds one more.
- Errors and recovery: project mode re-opens the project when the server URL,
  secret or `checkouts.json` bytes change (`:303-343`) and reattaches and
  retries once after a transport error (`:376-406`). Invalid JSON lines are
  ignored (`:182-183`).
- Configuration and environment: `HOTSHEET_STORE` for `--path .` (`:48`);
  `HOTSHEET_SERVER`, `HOTSHEET_SECRET` (`:59`, `:236-237`), ignored under a
  shared host such as the Codex app-server (`:58-67`).
- Invariants: a launched session never starts a server (`:244-270`); without
  launch, project mode ensures the machine host (`:72-77`).
- Tests: `crates/hotsheet-mcp/src/main.rs:462` (`hs2_jfe7jj_the_launched_route_comes_from_the_terminal_unless_a_shared_host_runs_it`),
  `crates/hotsheet-mcp/tests/launched_session.rs` (`hs2_jfe7jj_a_launched_sessions_shim_uses_its_server_and_starts_none`),
  `crates/hotsheet-mcp/tests/portable_project_config.rs`, `crates/hotsheet-mcp/src/project_cleanup_tests.rs`.

## Tool catalog and dispatch (`mcpTools`)

- Modules: `crates/hotsheet-mcp/src/lib.rs` (lines 1 to about 1620:
  JSON-RPC, catalog, actor injection, dispatch, rendering).
- Responsibility: `initialize`, `tools/list`, `tools/call`, `ping`
  (`crates/hotsheet-mcp/src/lib.rs:75-115`); 43 tools (40 inline in
  `base_tools_list`, `:148-604`, plus three submodule tools); dispatch match
  (`:782-1333`).
- State and persistence: none.
- Types and entry points: `Backend` trait (`:24`), `handle_message_for_checkout` (`:75`),
  `dispatch_tool` (`:782`), `ActorBackend` (`:742`).
- Concurrency: single-threaded.
- Errors and recovery: protocol errors are JSON-RPC errors `-32601` and
  `-32602` (`:104-115`); tool failures are `isError: true` results (`:630-634`),
  formatted `error <status>: <message>` (`:1604-1611`); an unknown tool is an
  `isError` result (`:1332`).
- Configuration and environment: `HOTSHEET_ACTOR_ROLE`, `HOTSHEET_ACTOR_ID`
  (`:711-712`); arguments win; absent means no actor.
- Invariants: only `MUTATING_TOOLS` take actor arguments (`:665-693`); in
  project mode the checkout id is injected unless `checkout` or `connection`
  is given (`:620-628`).
- Tests: `crates/hotsheet-mcp/src/lib.rs:5147` (`tools_list_has_the_hotsheet_tools`),
  `crates/hotsheet-mcp/src/lib.rs:5225` (`unknown_method_is_an_rpc_error`),
  `crates/hotsheet-mcp/src/lib.rs:7591` (`ai_actor_completion_requires_a_score_through_mcp`).

## Orchestration, remote and resource tools (`mcpDomainTools`)

- Modules: `crates/hotsheet-mcp/src/orchestration_tools.rs`, `crates/hotsheet-mcp/src/remote_tools.rs`,
  `crates/hotsheet-mcp/src/resource_claim_tools.rs`.
- Responsibility: `hotsheet_targets` (`crates/hotsheet-mcp/src/orchestration_tools.rs:6`),
  `hotsheet_remotes` (`crates/hotsheet-mcp/src/remote_tools.rs:9`),
  `hotsheet_resource_claim` (`crates/hotsheet-mcp/src/resource_claim_tools.rs:23`).
- State and persistence: none.
- Types and entry points: each module's `dispatch`, called at `crates/hotsheet-mcp/src/lib.rs:784-786`.
- Concurrency: resource waits are client long polls capped at 50 s per
  request and 10 minutes per call (`crates/hotsheet-mcp/src/resource_claim_tools.rs:13-16`).
- Errors and recovery: machine-only target actions are preflighted
  (`crates/hotsheet-mcp/src/orchestration_tools.rs:212`); remotes refuse a
  non-machine-local backend (`crates/hotsheet-mcp/src/remote_tools.rs:49-51`).
- Configuration and environment: none.
- Invariants: same routes as the server's.
- Tests: `crates/hotsheet-mcp/src/orchestration_tools.rs:1043`, `crates/hotsheet-mcp/src/remote_tools.rs:171`,
  `crates/hotsheet-mcp/tests/resource_claims.rs`.

## Serverless core backend (`mcpCoreBackend`)

- Modules: `crates/hotsheet-mcp/src/lib.rs` (`CoreBackend`, about lines 1677 to 4605).
- Responsibility: interpret HTTP-shaped paths in-process; special paths go
  to speculation, orchestration, resource claims and the remote registry first
  (`crates/hotsheet-mcp/src/lib.rs:2538-2557`).
- State and persistence: the admitted store (`FsStore::open_admitted`,
  `:1698-1703`) and the checkout registry.
- Types and entry points: `CoreBackend` (`:1677`), `with_host_sync_kick` (`:1808`).
- Concurrency: single-threaded; store lock in the core.
- Errors and recovery: core errors map to 400, 404, 409, 422 or 500 (`:4461-4605`);
  advisory claims are refused with 409 (`:2558-2562`). Writes run the
  notification watch (`:1998`, `:2976`) and the host sync kick.
- Configuration and environment: `HOTSHEET_HOME`.
- Invariants: a hosted provider source is omitted with a sanitized reason.
- Tests: `crates/hotsheet-mcp/tests/serverless_provider_get.rs`, `crates/hotsheet-mcp/tests/serverless_sync_kick.rs`
  (`hs2_4bg2qn_serverless_mcp_writes_kick_the_host_or_report_local_only`),
  `crates/hotsheet-mcp/tests/groups.rs`.

## Server HTTP backend (`mcpHttpBackend`)

- Modules: `crates/hotsheet-mcp/src/lib.rs` (`HttpBackend`, about lines 4615 to 4780).
- Responsibility: send requests with `X-Hotsheet-Secret` (`:4641`).
- State and persistence: none.
- Types and entry points: `HttpBackend` (`:4615`).
- Concurrency: blocking ureq agent with redirects disabled (`:4627`).
- Errors and recovery: a 3xx is an error (`:4696-4699`); coded 403s pass
  through; other 401/403 probe `/health` to explain a secret mismatch
  (`:4711-4779`).
- Configuration and environment: `--server`, `--secret`.
- Invariants: remote-registry and machine paths require a loopback origin
  (`:4661-4676`).
- Tests: `crates/hotsheet-mcp/src/lib.rs:5009` (`http_backend_probes_health_after_an_auth_failure`),
  `crates/hotsheet-mcp/tests/http_redirects.rs`.

## Presence heartbeat (`mcpPresence`)

- Modules: `crates/hotsheet-mcp/src/presence.rs`.
- Responsibility: register the shim's presence with the server so the UI can
  show a connected MCP session.
- State and persistence: server-side lease only; nothing local.
- Types and entry points: `PresenceTarget` (`crates/hotsheet-mcp/src/presence.rs:27`),
  `HEARTBEAT` (`:23`), started on `initialize` only (`:274-276`).
- Concurrency: one heartbeat thread, 15 s period, server TTL three heartbeats;
  stop and join on drop (`:22-23`, `:227-270`).
- Errors and recovery: a failed upsert relocates to the registry server
  (`:215-238`); a crash lets the lease lapse (`:3-5`).
- Configuration and environment: `HOTSHEET_SERVER`, `HOTSHEET_SECRET`,
  `HOTSHEET_TERMINAL_ID`, `HOTSHEET_AGENT` (`:51-52`, `:64-66`).
- Invariants: `POST /mcp-presence`, `DELETE /mcp-presence/{lease}`, 5 s
  timeout, no redirects (`:157-197`).
- Tests: `crates/hotsheet-mcp/src/presence.rs:473` (`heartbeat_registers_renews_and_releases_once_on_stop`),
  `crates/hotsheet-mcp/tests/registry_presence.rs`, `crates/hotsheet-mcp/tests/startup_presence_redirects.rs`,
  `crates/hotsheet-mcp/src/presence/redirect_tests.rs`.

## hotsheet-migrate entry (`migBinary`)

- Modules: `crates/hotsheet-cli/src/bin/hotsheet-migrate.rs`, `run_migrate_with_progress`
  and `resolve_migrator` in `crates/hotsheet-cli/src/lib.rs:342-457`.
- Responsibility: one-step HS1 migration plus `--backup-remote`,
  `--verify-backup`, `--prepare-backup`, `--progress-json` and the hidden
  `--hold-job-lock` bridge guard (`crates/hotsheet-cli/src/bin/hotsheet-migrate.rs:15-48`).
- State and persistence: a private temp staging directory for the export
  (`crates/hotsheet-cli/src/lib.rs:355-359`).
- Types and entry points: `main` (`crates/hotsheet-cli/src/bin/hotsheet-migrate.rs:50`).
- Concurrency: the exporter is a child `node` process recorded in the process
  journal; its stdout NDJSON is read incrementally (`crates/hotsheet-cli/src/lib.rs:362-396`).
- Errors and recovery: a failed export bails before any import
  (`:398-400`); earlier completion proof is invalidated first (`:351`).
- Configuration and environment: `--migrator`, else `HOTSHEET_MIGRATOR`, else
  `migrator/src/export.mjs` relative to cwd or the executable
  (`crates/hotsheet-cli/src/lib.rs:429-457`).
- Invariants: only a copy of the legacy database is opened.
- Tests: `crates/hotsheet-cli/tests/migrate.rs` (`machine_progress_keeps_hook_output_on_stderr_and_returns_typed_results`,
  `reports_a_missing_migrator`).

## Node HS1 exporter (`migExporter`)

- Modules: `migrator/src/export.mjs`; small: `migrator/src/introspect.mjs`.
- Responsibility: export tickets, notes, attachments and settings from PGLite
  0.3.x or 0.4.x datadirs (`migrator/src/export.mjs:1-18`).
- State and persistence: temporary copy under the OS temp directory
  (`migrator/src/export.mjs:213`); output `hotsheet-export.json`.
- Types and entry points: `exportVersion` field (`:158`), engine load (`:440-442`).
- Concurrency: single Node process.
- Errors and recovery: newer datadirs fall back to `pglite-migrate`
  (`:13-16`).
- Configuration and environment: none.
- Invariants: read-only against the source.
- Tests: `migrator/test/export.test.mjs`, `migrator/test/progress.test.mjs`.

## HS1 importer and recovery (`migImport`, Library)

- Modules: `crates/hotsheet-cli/src/import.rs`, `crates/hotsheet-cli/src/import_recovery.rs`,
  `crates/hotsheet-cli/src/import_completion.rs`; small: `migration_progress.rs`.
- Responsibility: import a version-1 export (`crates/hotsheet-cli/src/import.rs:25`)
  through the core writer, with deterministic ULIDs (`:1-6`); diagnose and
  repair legacy attachments (`crates/hotsheet-cli/src/import_recovery.rs:171`, `:267`);
  verify, back up and record completion (`crates/hotsheet-cli/src/import_completion.rs:34`, `:151`, `:287`).
- State and persistence: `hotsheet-hs1-import-pending/<ulid>.json` checkpoints
  written atomically under the ticket transaction (`crates/hotsheet-cli/src/import.rs:136`, `:193-204`);
  `hotsheet-hs1-import.json` receipt (`crates/hotsheet-cli/src/lib.rs:106`);
  `hotsheet-hs1-import-completed.json`, `hotsheet-hs1-backup.json`
  (`crates/hotsheet-cli/src/import_completion.rs:10-11`);
  `hotsheet-hs1-attachment-omissions.json` (`crates/hotsheet-cli/src/import_recovery.rs:17`);
  `git config hotsheet.backupOrigin` (`crates/hotsheet-cli/src/import_completion.rs:99`).
- Types and entry points: `import_with_progress` (`crates/hotsheet-cli/src/import.rs:238`),
  `PendingImport` (`:142`).
- Concurrency: synchronous; checkpoint writes take the store's ticket transaction.
- Errors and recovery: an export missing a pending ticket, a changed
  attachment list, an unsupported checkpoint version or a vanished ticket
  fails and keeps the checkpoint (`crates/hotsheet-cli/src/import.rs:149-190`).
- Configuration and environment: none.
- Invariants: a completed ticket without a checkpoint is skipped on reimport.
- Tests: `crates/hotsheet-cli/tests/import_recovery.rs` (`cli_diagnoses_without_writes_and_repairs_only_selected_legacy_payloads`),
  `crates/hotsheet-cli/tests/cli.rs:5476` (`import_retry_repairs_attachments_without_rewriting_edits_and_commits_recovery`),
  `crates/hotsheet-cli/tests/migrate.rs` (`repeated_migration_rejects_failed_commits_and_invalidates_cleanup_proof`).
