//! `hotsheet-mcp` — the MCP shim binary. Reads newline-delimited JSON-RPC on stdin,
//! proxies the `hotsheet_*` tools to a running server, and writes replies on stdout.

use std::io::{BufRead, Write};

use anyhow::Result;
use clap::Parser;
use hotsheet_mcp::{HttpBackend, handle_message};

#[derive(Parser)]
#[command(
    name = "hotsheet-mcp",
    version,
    about = "MCP shim exposing hotsheet_* tools over a running hotsheet-server"
)]
struct Cli {
    /// Base URL of the running server.
    #[arg(long, default_value = "http://127.0.0.1:8787")]
    server: String,
    /// The server's shared secret.
    #[arg(long)]
    secret: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let backend = HttpBackend::new(cli.server, cli.secret);

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
        if let Some(response) = handle_message(&msg, &backend) {
            writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
            stdout.flush()?;
        }
    }
    Ok(())
}
