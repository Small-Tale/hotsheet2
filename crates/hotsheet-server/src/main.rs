//! `hotsheet-server` — the always-on HTTP + WebSocket service over the core
//! (`docs/04` §4.3). v1: Tier 0 (loopback + shared secret). Detached lifecycle /
//! auto-start (HS2-59), mTLS (Tier 1), the watcher, and terminals are separate.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use hotsheet_index::Index;
use hotsheet_model::Timestamp;
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::FsStore;
use time::OffsetDateTime;
use tokio::net::TcpListener;
use ulid::Ulid;

#[derive(Parser)]
#[command(
    name = "hotsheet-server",
    version,
    about = "Hot Sheet 2 server (HTTP + WS)"
)]
struct Cli {
    /// The store directory to serve.
    #[arg(short = 'C', long = "path", default_value = ".")]
    path: PathBuf,
    /// Address to bind (loopback only until mTLS lands). Use port 0 for an ephemeral port.
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: String,
    /// Shared secret required on `X-Hotsheet-Secret` (generated + printed if omitted).
    #[arg(long)]
    secret: Option<String>,
    /// Index database file (default: ${HOTSHEET_HOME:-~/.hotsheet2}/index/<project-id>.v<schema>.sqlite).
    #[arg(long)]
    index: Option<PathBuf>,
    /// Stop the running server for this store (explicit shutdown), then exit.
    #[arg(long)]
    stop: bool,

    /// With --stop, also kill every terminal retained by this project's detached broker.
    #[arg(long, requires = "stop")]
    kill_all_terminals: bool,

    /// Deprecated compatibility flag; detached terminal hosting is now the default.
    #[arg(long, hide = true)]
    terminal_broker: bool,

    /// Host terminals in this server process instead of the default detached broker.
    #[arg(long, conflicts_with = "terminal_broker")]
    no_terminal_broker: bool,

    /// Internal detached-broker socket used when no sibling broker binary is installed.
    #[arg(
        long,
        hide = true,
        value_name = "SOCKET",
        requires = "terminal_broker_project"
    )]
    terminal_broker_process: Option<PathBuf>,
    /// Internal detached-broker project identity.
    #[arg(
        long,
        hide = true,
        value_name = "PROJECT",
        requires = "terminal_broker_process"
    )]
    terminal_broker_project: Option<String>,

    /// **Opt-in** distributed driving loop (HS2-1TY7GC): spawn this AI tool (plugin id,
    /// e.g. `codex`) on each self-claimed ticket across hosted stores that have a git
    /// remote. Omitted → the loop is off (the default; the server never drives a tool
    /// unless asked, like the live-tool test tier).
    #[arg(long, value_name = "TOOL")]
    drive_tool: Option<String>,
    /// Max tickets the driving loop runs concurrently across hosted stores (default 1).
    #[arg(long, default_value_t = 1)]
    drive_workers: usize,
    /// Claim lease length for driven tickets, in minutes (default 30).
    #[arg(long, default_value_t = 30)]
    drive_lease_min: i64,
    /// Worker id recorded on claims (default: the store's git email, else `server`).
    #[arg(long)]
    drive_worker: Option<String>,

    /// How long a graceful stop waits for open connections before forcing exit, in
    /// milliseconds (HS2-W1KJR4). Hidden: tests shorten or lengthen it.
    #[arg(long, hide = true, default_value_t = DEFAULT_SHUTDOWN_DRAIN_MS)]
    shutdown_drain_ms: u64,

    /// Deadline for one AI-tool discovery probe (`--version` or a model catalog), in
    /// milliseconds; a probe that misses it is killed (HS2-BH3M53). Hidden: tests shorten it.
    #[arg(long, hide = true)]
    ai_probe_timeout_ms: Option<u64>,

    /// Stop gracefully when stdin reaches EOF (HS2-VQ8ZWT). For test harnesses and other
    /// supervisors that own this process: they keep a **pipe** on stdin, so when they die
    /// for any reason (even SIGKILL) the kernel closes it and the server cannot outlive
    /// them. Never set for detached launches, which must outlive their client; and never
    /// with stdin on `/dev/null`, which reads EOF at once.
    #[arg(long, hide = true)]
    exit_on_stdin_eof: bool,
}

/// Default bounded drain for a graceful stop (HS2-W1KJR4).
const DEFAULT_SHUTDOWN_DRAIN_MS: u64 = 5_000;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(socket) = cli.terminal_broker_process.as_ref() {
        let project = cli.terminal_broker_project.clone().unwrap_or_default();
        return Ok(hotsheet_terminals::run_broker_process(socket, project).await?);
    }

    // Explicit shutdown (HS2-59): signal the running server for this store and exit.
    if cli.stop {
        let stopped = hotsheet_server::lifecycle::stop_instance(&cli.path);
        if stopped {
            println!("stopped the running server for {}", cli.path.display());
        } else {
            println!("no running server found for {}", cli.path.display());
        }
        if cli.kill_all_terminals {
            if stopped {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while hotsheet_server::lifecycle::find_instance(&cli.path).is_some()
                    && std::time::Instant::now() < deadline
                {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                if hotsheet_server::lifecycle::find_instance(&cli.path).is_some() {
                    anyhow::bail!(
                        "server did not stop before terminal cleanup; retained terminals were preserved"
                    );
                }
            }
            let count = match hotsheet_server::terminal_broker::TerminalBroker::discover(&cli.path)
            {
                Some(broker) => broker.kill_all().await?,
                None => 0,
            };
            println!(
                "killed {count} retained terminal(s) for {}",
                cli.path.display()
            );
        }
        return Ok(());
    }

    // Join-don't-collide (HS2-59): if a server is already serving this store, print how to
    // attach and exit instead of starting a duplicate.
    if let Some(existing) = hotsheet_server::lifecycle::find_instance(&cli.path) {
        println!(
            "a server is already serving {} at {} (pid {})",
            cli.path.display(),
            existing.url,
            existing.pid
        );
        println!("secret: {}", existing.secret);
        return Ok(());
    }

    let store = FsStore::open(&cli.path)?;

    let addr: SocketAddr = cli
        .bind
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid --bind '{}' (want IP:port)", cli.bind))?;
    // Tier 0 = loopback + shared secret (plaintext). Off-loopback = Tier 1 mTLS: every client
    // must present a project-CA cert (HS2-VT3JMF). Build the TLS config before binding so a
    // missing `cert init` fails fast rather than after announcing a listener.
    let tls_config = if addr.ip().is_loopback() {
        None
    } else {
        let paths = hotsheet_tls::Paths::for_store(&cli.path);
        Some((
            hotsheet_server::tls::build_server_config(&paths).map_err(|e| {
                anyhow::anyhow!(
                    "off-loopback bind {addr} needs mTLS: {e}\n\
                 run `hotsheet-cli cert init` (optionally --host <ip/dns>) first"
                )
            })?,
            paths.acl(),
        ))
    };
    let scheme = if tls_config.is_some() {
        "https"
    } else {
        "http"
    };

    let secret = cli.secret.unwrap_or_else(|| Ulid::new().to_string());

    // File-backed index, restored from disk + reconciled with the current files.
    let index_path = match cli.index {
        Some(path) => path,
        None => default_index_path(&store)?,
    };
    let index = Index::open_reconciled(&index_path, &store)?;
    println!("index: {}", index_path.display());
    prune_stale_indexes();
    // A real run persists the indexes of any POST /stores-registered store too.
    let permission_rules_path = hotsheet_server::multistore::permission_rules_path_for(&store)?;
    let store_id = hotsheet_server::multistore::store_url_id(&store);
    let drive_root = hotsheet_plugins::hotsheet_home()
        .join("drive")
        .join(&store_id);
    let mut state = AppState::with_index(store, secret.clone(), index)
        .with_persistent_registered_indexes()
        .with_permission_rules(permission_rules_path)
        .with_client_drive_persistence(
            drive_root.join("sessions.json"),
            drive_root.join("homes"),
        )?;

    // Detached terminal hosting is the default: normal server stops/restarts disconnect from
    // the broker without killing its PTYs. `--no-terminal-broker` is an explicit diagnostic
    // escape hatch for environments where the sibling broker binary cannot run.
    if terminal_broker_enabled(cli.no_terminal_broker) {
        state = state.with_terminal_broker()?;
        println!("terminals: detached broker (survives restart)");
    }

    // Auto-host any stores configured in ${HOTSHEET_HOME}/stores.json (HS2-87 discovery).
    let extra = state.host_configured_stores();
    if extra > 0 {
        println!("hosting {extra} configured store(s) from stores.json");
    }
    // Terminals that survived a restart in the broker get their session monitors back, so an
    // ended session still releases its claims (HS2-RXWXQ8).
    hotsheet_server::resume_broker_terminal_sessions(&state).await;
    // Names of terminals that did not survive (no broker, or a crashed broker) must not carry
    // over to a later terminal reusing the id (HS2-8A0FYR).
    let pruned = hotsheet_server::prune_orphaned_terminal_names(&state).await;
    if !pruned.is_empty() {
        println!("forgot {} saved name(s) of ended terminals", pruned.len());
    }
    // Chat drives do not survive a restart; release what their ended sessions still hold.
    hotsheet_server::release_orphaned_drive_sessions(&state).await;

    // Keep the index fresh + broadcast external edits. Held for the run.
    let _watch = hotsheet_server::spawn_watcher(state.clone())?;

    // Aggressively sync each hosted store with its git remote in the background (HS2-19
    // follow-up): interval + kick-on-write + backoff. Held for the run.
    let _sync = hotsheet_server::sync_loop::spawn_sync_loop(
        state.clone(),
        hotsheet_server::sync_loop::DEFAULT_INTERVAL,
    );

    // Take the exclusive index-writer lock before binding — a second server on this store
    // would otherwise double-write the index (join-don't-collide, HS2-59).
    let writer_lock = hotsheet_server::lifecycle::acquire_writer_lock(&cli.path)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let listener = TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    let url = format!("{scheme}://{local}");
    state.set_terminal_server_url(url.clone());
    println!(
        "hotsheet-server listening on {url} (store: {}){}",
        cli.path.display(),
        if tls_config.is_some() {
            " [mTLS — client cert required]"
        } else {
            ""
        }
    );
    println!("secret: {secret}");

    // Opt-in distributed driving loop (HS2-1TY7GC): spawn a real AI tool per self-claimed
    // ticket. Off unless `--drive-tool` is given. Spawned after bind so the driven tool's
    // permission hook gets the real server URL injected (HS2-XCTAHM). Held for the run.
    let _drive = if let Some(tool) = cli.drive_tool.clone() {
        use hotsheet_server::dist_work_loop::{
            DEFAULT_INTERVAL, DistWorkConfig, LiveDriveCtx, live_drive, spawn_dist_work_loop,
        };
        let worker = cli
            .drive_worker
            .clone()
            .or_else(|| git_email(&cli.path))
            .unwrap_or_else(|| "server".to_string());
        println!(
            "driving loop: tool={tool} worker={worker} max_in_flight={} lease={}min",
            cli.drive_workers, cli.drive_lease_min
        );
        let cfg = DistWorkConfig {
            enabled: true,
            worker,
            lease_minutes: cli.drive_lease_min,
            max_in_flight: cli.drive_workers,
            tool: tool.clone(),
            ..Default::default()
        };
        // Driven approvals block on the server's permission bridge, so a client answering
        // over POST /permissions steers the tool (HS2-Q1F6HV / HS2-YMR9HE); the server URL +
        // secret let a Claude permission hook reach that route-back (HS2-XCTAHM).
        let ctx = LiveDriveCtx {
            permission_bridge: Some(state.permission_bridge()),
            drive_registry: Some(state.drive_registry()),
            server_env: Some((url.clone(), secret.clone())),
            activity_sink: Some({
                let state = state.clone();
                std::sync::Arc::new(move |store, event| {
                    if let Err(error) = state.record_activity(store, event) {
                        eprintln!("activity record failed: {error}");
                    }
                })
            }),
            turn_sink: Some({
                let state = state.clone();
                std::sync::Arc::new(move |store, connection, tool, ticket, event| {
                    state.emit_turn_event(store, connection, ticket, tool, event);
                })
            }),
            worker_sessions: Some(Default::default()),
            persistent_home_root: Some(drive_root.join("workers")),
        };
        let drive = live_drive(cfg.tool.clone(), cfg.prompt.clone(), ctx);
        Some(spawn_dist_work_loop(
            state.clone(),
            cfg,
            DEFAULT_INTERVAL,
            drive,
        ))
    } else {
        None
    };

    // Install stop-signal handling before any instance file exists, so a stop that lands
    // during startup still runs the cleanup below instead of the default kill (HS2-W1KJR4).
    let force = spawn_signal_listener(state.clone());
    if cli.exit_on_stdin_eof {
        stop_on_stdin_eof(state.clone());
    }

    // Register a discovery instance file for EVERY hosted store (the primary + any from
    // stores.json), all pointing at this one machine server — the topology-A reconciliation
    // (HS2-87): one server per machine, discoverable per project. Guards live in the state
    // and remove the files on graceful shutdown; a crash leaves stale files `find_instance`
    // ignores. Runtime `POST /stores` additions register themselves the same way.
    if let Some(ms) = cli.ai_probe_timeout_ms {
        hotsheet_aitools::probe::set_probe_timeout(std::time::Duration::from_millis(ms));
    }
    let started_at = Timestamp::from_datetime(OffsetDateTime::now_utc());
    state.publish_instances(url, started_at.as_str().to_string());
    // Warm the AI model catalog now, in the background, so the first client's startup path
    // finds it already discovered instead of paying the cold subprocess discovery inline
    // (HS2-MYDN7C / HS2-10R4VV). Never blocks serving.
    state.prewarm_ai_catalog();

    // Explicit shutdown only (HS2-59), bounded (HS2-W1KJR4): the first SIGTERM / Ctrl-C (or
    // a quiescent restart) stops accepting and tells long waits to finish; open connections
    // then get `--shutdown-drain-ms` to complete. A deadline or a second signal forces the
    // stop. Either way the instance files and writer locks are released explicitly before
    // exit, because connection tasks and parked blocking work can outlive the listener.
    let stopping = state.clone();
    let stop_accepting = async move { stopping.stopping().await };
    let serve: std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> =
        match tls_config {
            // Tier 1: mutual TLS (manual acceptor that drains its tracked connections).
            Some((config, acl_file)) => Box::pin(hotsheet_server::tls::serve_tls_with_acl(
                listener,
                app(state.clone()),
                config,
                Some(acl_file),
                stop_accepting,
            )),
            // Tier 0: plaintext loopback, with axum's per-connection graceful shutdown.
            None => {
                let serve = axum::serve(listener, app(state.clone()))
                    .with_graceful_shutdown(stop_accepting);
                Box::pin(async move { Ok(serve.await?) })
            }
        };
    let drain = std::time::Duration::from_millis(cli.shutdown_drain_ms);
    let outcome = drain_on_shutdown(serve, &state, drain, force).await;
    // A probe started during the drain must not outlive the process either (HS2-BH3M53).
    hotsheet_aitools::probe::stop_probes();
    state.release_instances();
    drop(writer_lock);
    match outcome {
        DrainOutcome::Drained(result) => {
            // Background blocking work (catalog discovery, a parked driven-tool approval)
            // would otherwise keep the runtime from dropping; bound that too.
            exit_after(drain);
            result
        }
        DrainOutcome::Forced(reason) => {
            shutdown_log(&format!(
                "shutdown: {reason}; forcing exit with connections still open"
            ));
            std::process::exit(0);
        }
    }
}

fn terminal_broker_enabled(no_terminal_broker: bool) -> bool {
    !no_terminal_broker
}

/// How a shutdown drain ended.
#[derive(Debug)]
enum DrainOutcome {
    /// Serving ended on its own and every connection finished.
    Drained(Result<()>),
    /// The drain deadline passed or a second stop signal arrived.
    Forced(&'static str),
}

/// Run `serve` until the server starts stopping, then give it `deadline` to finish its open
/// connections, cut short by a second stop signal on `force` (HS2-W1KJR4).
async fn drain_on_shutdown(
    serve: impl std::future::Future<Output = Result<()>>,
    state: &AppState,
    deadline: std::time::Duration,
    mut force: tokio::sync::mpsc::Receiver<()>,
) -> DrainOutcome {
    let mut serve = std::pin::pin!(serve);
    tokio::select! {
        result = &mut serve => return DrainOutcome::Drained(result),
        () = state.stopping() => {}
    }
    // Stop every AI-tool probe at once (HS2-BH3M53): a request waiting on discovery then
    // answers from the manifest fallback instead of holding the drain open, and no probe
    // child is orphaned by the exit.
    hotsheet_aitools::probe::stop_probes();
    tokio::select! {
        result = &mut serve => DrainOutcome::Drained(result),
        () = tokio::time::sleep(deadline) => DrainOutcome::Forced("drain deadline passed"),
        Some(()) = force.recv() => DrainOutcome::Forced("second stop signal"),
    }
}

/// Exit the process from a watchdog thread if `main` has not returned within `after`.
fn exit_after(after: std::time::Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(after);
        shutdown_log("shutdown: background work outlived the drain; exiting");
        std::process::exit(0);
    });
}

/// Turn stop signals into the shutdown sequence: the first SIGTERM / Ctrl-C (or a quiescent
/// restart request) begins stopping; any later signal is sent on the returned channel to
/// force the stop. Handlers are installed once, up front, so a repeat is never lost.
fn spawn_signal_listener(state: AppState) -> tokio::sync::mpsc::Receiver<()> {
    let (force_tx, force_rx) = tokio::sync::mpsc::channel(1);
    let mut signals = StopSignals::install();
    tokio::spawn(async move {
        tokio::select! {
            () = signals.next() => {},
            () = state.shutdown_requested() => {},
        }
        state.begin_stopping();
        signals.next().await;
        let _ = force_tx.send(()).await;
    });
    force_rx
}

/// Begin the bounded shutdown once stdin closes: the owning process exited (HS2-VQ8ZWT).
/// Input is discarded; only EOF (or a read error) matters. A blocking thread, not a timer.
fn stop_on_stdin_eof(state: AppState) {
    std::thread::spawn(move || {
        use std::io::Read;
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0_u8; 1024];
        loop {
            match stdin.read(&mut buf) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        // Stop first: the owner that died usually held our stdout/stderr pipes too.
        state.begin_stopping();
        shutdown_log("shutdown: stdin closed (owner exited); stopping");
    });
}

/// Best-effort shutdown diagnostic. Unlike `eprintln!`, never panics when stderr is a pipe
/// whose reader is gone, which is exactly the situation an owner-death stop runs in.
fn shutdown_log(message: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{message}");
}

/// Repeatable SIGTERM / SIGINT stream.
struct StopSignals {
    #[cfg(unix)]
    term: Option<tokio::signal::unix::Signal>,
    #[cfg(unix)]
    int: Option<tokio::signal::unix::Signal>,
}

impl StopSignals {
    fn install() -> Self {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Self {
                term: signal(SignalKind::terminate()).ok(),
                int: signal(SignalKind::interrupt()).ok(),
            }
        }
        #[cfg(not(unix))]
        {
            Self {}
        }
    }

    /// Resolve on the next stop signal.
    async fn next(&mut self) {
        #[cfg(unix)]
        {
            async fn recv(signal: Option<&mut tokio::signal::unix::Signal>) {
                match signal {
                    Some(signal) => {
                        signal.recv().await;
                    }
                    None => std::future::pending().await,
                }
            }
            tokio::select! {
                () = recv(self.term.as_mut()) => {},
                () = recv(self.int.as_mut()) => {},
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

/// The store's git `user.email`, if configured — the default worker id for the driving
/// loop (HS2-1TY7GC), matching how assignment identifies people (docs/10).
fn git_email(path: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["config", "user.email"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let email = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!email.is_empty()).then_some(email)
}

/// `${HOTSHEET_HOME:-~/.hotsheet2}/index/<project-id>.v<schema>.sqlite`, keyed by a hash of the store's path
/// (machine-local, gitignored, disposable — `docs/03` §3.2).
fn default_index_path(store: &FsStore) -> Result<PathBuf> {
    let root = store
        .root()
        .canonicalize()
        .unwrap_or_else(|_| store.root().to_path_buf());
    let id = &hotsheet_index::hash_bytes(root.to_string_lossy().as_bytes())[..16];
    // HS2's own machine home (${HOTSHEET_HOME:-~/.hotsheet2}) — NOT ~/.hotsheet, which
    // a separately installed Hot Sheet 1 owns (HS2-104).
    let dir = hotsheet_plugins::hotsheet_home().join("index");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(hotsheet_index::index_file_name(id)))
}

/// Remove index files that older schema generations left in the machine index dir and that
/// no running process still holds open (HS2-Y0PAEM). Best-effort: a failure is logged only.
fn prune_stale_indexes() {
    let dir = hotsheet_plugins::hotsheet_home().join("index");
    match hotsheet_index::prune_stale_index_files(&dir) {
        Ok(report) => {
            if let Some(summary) = report.summary() {
                println!("index: {summary}");
            }
        }
        Err(error) => eprintln!("index: could not prune stale index files: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detached_terminal_broker_is_default_with_an_explicit_diagnostic_opt_out() {
        let default = Cli::try_parse_from(["hotsheet-server", "-C", "store"]).unwrap();
        assert!(terminal_broker_enabled(default.no_terminal_broker));
        let disabled =
            Cli::try_parse_from(["hotsheet-server", "-C", "store", "--no-terminal-broker"])
                .unwrap();
        assert!(!terminal_broker_enabled(disabled.no_terminal_broker));
    }

    #[test]
    fn owner_watchdog_and_drain_are_hidden_opt_ins_with_safe_defaults() {
        // HS2-VQ8ZWT / HS2-W1KJR4: a detached launch never watches stdin; the drain is bounded.
        let default = Cli::try_parse_from(["hotsheet-server", "-C", "store"]).unwrap();
        assert!(!default.exit_on_stdin_eof);
        assert_eq!(default.shutdown_drain_ms, DEFAULT_SHUTDOWN_DRAIN_MS);
        let owned = Cli::try_parse_from([
            "hotsheet-server",
            "--exit-on-stdin-eof",
            "--shutdown-drain-ms",
            "250",
        ])
        .unwrap();
        assert!(owned.exit_on_stdin_eof);
        assert_eq!(owned.shutdown_drain_ms, 250);
    }

    #[test]
    fn kill_all_terminals_requires_an_explicit_server_stop() {
        assert!(Cli::try_parse_from(["hotsheet-server", "--kill-all-terminals"]).is_err());
        let stop =
            Cli::try_parse_from(["hotsheet-server", "--stop", "--kill-all-terminals"]).unwrap();
        assert!(stop.stop && stop.kill_all_terminals);
    }
}
