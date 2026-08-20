//! `hotsheet-server` — the always-on HTTP + WebSocket service over the core
//! (`docs/04` §4.3). v1: Tier 0 (loopback + shared secret). Detached lifecycle /
//! auto-start (HS2-59), mTLS (Tier 1), the watcher, and terminals are separate.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;
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
    let state = AppState::new(store, secret.clone());

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
