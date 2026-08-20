//! `hotsheet-mcp` — the MCP shim binary. Reads newline-delimited JSON-RPC on stdin,
//! serves the `hotsheet_*` tools, and writes replies on stdout. Two modes:
//!   - `--path <store>`  → serverless, direct to disk (no server needed).
//!   - `--server <url> --secret <s>` → proxy a running hotsheet-server over HTTP.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;
use hotsheet_mcp::{Backend, CoreBackend, HttpBackend, handle_message};

#[derive(Parser)]
#[command(
    name = "hotsheet-mcp",
    version,
    about = "MCP shim exposing the hotsheet_* tools — serverless (--path) or over a server (--server)"
)]
struct Cli {
    /// Serve a store directly from disk (serverless). The headless default.
    #[arg(short = 'C', long = "path")]
    path: Option<PathBuf>,
    /// Base URL of a running server (proxy mode). Requires --secret.
    #[arg(long)]
    server: Option<String>,
    /// The server's shared secret (proxy mode only).
    #[arg(long)]
    secret: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let backend: Box<dyn Backend> = match (cli.path, cli.server) {
        (Some(path), None) => Box::new(CoreBackend::open(&path)?),
        (None, Some(server)) => {
            let secret = cli
                .secret
                .ok_or_else(|| anyhow::anyhow!("--server requires --secret"))?;
            Box::new(HttpBackend::new(server, secret))
        }
        (Some(_), Some(_)) => bail!("pass either --path (serverless) or --server, not both"),
        (None, None) => bail!("pass --path <store> (serverless) or --server <url> --secret <s>"),
    };

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue; // ignore un-parseable input rather than crashing the shim
        };
        if let Some(response) = handle_message(&msg, backend.as_ref()) {
            writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
            stdout.flush()?;
        }
    }
    Ok(())
}
