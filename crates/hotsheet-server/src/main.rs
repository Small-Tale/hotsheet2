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
    /// Index database file (default: ${HOTSHEET_HOME:-~/.hotsheet2}/index/<project-id>.sqlite).
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
}

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
    let _lock = hotsheet_server::lifecycle::acquire_writer_lock(&cli.path)
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

    // Register a discovery instance file for EVERY hosted store (the primary + any from
    // stores.json), all pointing at this one machine server — the topology-A reconciliation
    // (HS2-87): one server per machine, discoverable per project. Guards live in the state
    // and remove the files on graceful shutdown; a crash leaves stale files `find_instance`
    // ignores. Runtime `POST /stores` additions register themselves the same way.
    let started_at = Timestamp::from_datetime(OffsetDateTime::now_utc());
    state.publish_instances(url, started_at.as_str().to_string());
    // Warm the AI model catalog now, in the background, so the first client's startup path
    // finds it already discovered instead of paying the cold subprocess discovery inline
    // (HS2-MYDN7C / HS2-10R4VV). Never blocks serving.
    state.prewarm_ai_catalog();
    let lifecycle_state = state.clone();

    // Explicit shutdown only (HS2-59): serve until SIGTERM / Ctrl-C, then the guards drop
    // (instance file + writer lock removed), and any in-flight work has already run in the
    // separate process a client can't kill by closing.
    match tls_config {
        // Tier 1: serve over mutual TLS (manual acceptor; graceful stop-accepting on signal).
        Some((config, acl_file)) => {
            hotsheet_server::tls::serve_tls_with_acl(
                listener,
                app(state),
                config,
                Some(acl_file),
                shutdown_signal(lifecycle_state),
            )
            .await?;
        }
        // Tier 0: plaintext loopback, with axum's per-connection graceful shutdown.
        None => {
            axum::serve(listener, app(state))
                .with_graceful_shutdown(shutdown_signal(lifecycle_state))
                .await?;
        }
    }
    Ok(())
}

fn terminal_broker_enabled(no_terminal_broker: bool) -> bool {
    !no_terminal_broker
}

/// Resolve when the process is asked to stop: SIGTERM (the `--stop` path) or Ctrl-C.
async fn shutdown_signal(state: AppState) {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut s) = signal(SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
        _ = state.shutdown_requested() => {},
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

/// `${HOTSHEET_HOME:-~/.hotsheet2}/index/<project-id>.sqlite`, keyed by a hash of the store's path
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
    Ok(dir.join(format!("{id}.sqlite")))
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
    fn kill_all_terminals_requires_an_explicit_server_stop() {
        assert!(Cli::try_parse_from(["hotsheet-server", "--kill-all-terminals"]).is_err());
        let stop =
            Cli::try_parse_from(["hotsheet-server", "--stop", "--kill-all-terminals"]).unwrap();
        assert!(stop.stop && stop.kill_all_terminals);
    }
}
