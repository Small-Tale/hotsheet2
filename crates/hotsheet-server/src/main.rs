//! `hotsheet-server` — the always-on HTTP + WebSocket service over the core
//! (`docs/04` §4.3). v1: Tier 0 (loopback + shared secret). Detached lifecycle /
//! auto-start (HS2-59), mTLS (Tier 1), the watcher, and terminals are separate.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Result, bail};
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
    /// Index database file (default: ~/.hotsheet/index/<project-id>.sqlite).
    #[arg(long)]
    index: Option<PathBuf>,
    /// Stop the running server for this store (explicit shutdown), then exit.
    #[arg(long)]
    stop: bool,

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

    // Explicit shutdown (HS2-59): signal the running server for this store and exit.
    if cli.stop {
        if hotsheet_server::lifecycle::stop_instance(&cli.path) {
            println!("stopped the running server for {}", cli.path.display());
        } else {
            println!("no running server found for {}", cli.path.display());
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
    // Tier 0 only for now: off-loopback binds require mTLS (Tier 1), which isn't built.
    if !addr.ip().is_loopback() {
        bail!(
            "refusing to bind {addr}: off-loopback needs mTLS (not yet built); \
             bind a loopback address"
        );
    }

    let secret = cli.secret.unwrap_or_else(|| Ulid::new().to_string());

    // File-backed index, restored from disk + reconciled with the current files.
    let index_path = match cli.index {
        Some(path) => path,
        None => default_index_path(&store)?,
    };
    let index = Index::open_reconciled(&index_path, &store)?;
    println!("index: {}", index_path.display());
    // A real run persists the indexes of any POST /stores-registered store too.
    let state =
        AppState::with_index(store, secret.clone(), index).with_persistent_registered_indexes();

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

    // Opt-in distributed driving loop (HS2-1TY7GC): spawn a real AI tool per self-claimed
    // ticket across hosted stores with a remote. Off unless `--drive-tool` is given.
    let _drive = if let Some(tool) = cli.drive_tool.clone() {
        use hotsheet_server::dist_work_loop::{
            DEFAULT_INTERVAL, DistWorkConfig, live_drive, spawn_dist_work_loop,
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
        let drive = live_drive(cfg.tool.clone(), cfg.prompt.clone());
        Some(spawn_dist_work_loop(
            state.clone(),
            cfg,
            DEFAULT_INTERVAL,
            drive,
        ))
    } else {
        None
    };

    // Take the exclusive index-writer lock before binding — a second server on this store
    // would otherwise double-write the index (join-don't-collide, HS2-59).
    let _lock = hotsheet_server::lifecycle::acquire_writer_lock(&cli.path)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let listener = TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    let url = format!("http://{local}");
    println!(
        "hotsheet-server listening on {url} (store: {})",
        cli.path.display()
    );
    println!("secret: {secret}");

    // Register a discovery instance file for EVERY hosted store (the primary + any from
    // stores.json), all pointing at this one machine server — the topology-A reconciliation
    // (HS2-87): one server per machine, discoverable per project. Guards live in the state
    // and remove the files on graceful shutdown; a crash leaves stale files `find_instance`
    // ignores. Runtime `POST /stores` additions register themselves the same way.
    let started_at = Timestamp::from_datetime(OffsetDateTime::now_utc());
    state.publish_instances(url, started_at.as_str().to_string());

    // Explicit shutdown only (HS2-59): serve until SIGTERM / Ctrl-C, then the guards drop
    // (instance file + writer lock removed), and any in-flight work has already run in the
    // separate process a client can't kill by closing.
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Resolve when the process is asked to stop: SIGTERM (the `--stop` path) or Ctrl-C.
async fn shutdown_signal() {
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

/// `~/.hotsheet/index/<project-id>.sqlite`, keyed by a hash of the store's path
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
