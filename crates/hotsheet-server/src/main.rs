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
    let state = AppState::with_index(store, secret.clone(), index);

    // Keep the index fresh + broadcast external edits. Held for the run.
    let _watch = hotsheet_server::spawn_watcher(state.clone())?;

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

    // Register this instance so clients/CLIs can discover it; the guard removes the file on
    // graceful shutdown (a crash leaves a stale file that `find_instance` ignores).
    let started_at = Timestamp::from_datetime(OffsetDateTime::now_utc());
    let info = hotsheet_server::lifecycle::InstanceInfo {
        pid: std::process::id(),
        url,
        secret,
        store_path: cli.path.display().to_string(),
        index_path: index_path.display().to_string(),
        started_at: started_at.as_str().to_string(),
    };
    let _instance = hotsheet_server::lifecycle::register_instance(&info, &cli.path)?;

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
