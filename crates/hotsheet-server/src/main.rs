//! `hotsheet-server` — the always-on HTTP + WebSocket service over the core
//! (`docs/04` §4.3). v1: Tier 0 (loopback + shared secret). Detached lifecycle /
//! auto-start (HS2-59), mTLS (Tier 1), the watcher, and terminals are separate.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;
use hotsheet_index::Index;
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::FsStore;
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
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

    let listener = TcpListener::bind(addr).await?;
    let local = listener.local_addr()?;
    println!(
        "hotsheet-server listening on http://{local} (store: {})",
        cli.path.display()
    );
    println!("secret: {secret}");

    axum::serve(listener, app(state)).await?;
    Ok(())
}

/// `~/.hotsheet/index/<project-id>.sqlite`, keyed by a hash of the store's path
/// (machine-local, gitignored, disposable — `docs/03` §3.2).
fn default_index_path(store: &FsStore) -> Result<PathBuf> {
    let root = store
        .root()
        .canonicalize()
        .unwrap_or_else(|_| store.root().to_path_buf());
    let id = &hotsheet_index::hash_bytes(root.to_string_lossy().as_bytes())[..16];
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join(".hotsheet").join("index");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{id}.sqlite")))
}
