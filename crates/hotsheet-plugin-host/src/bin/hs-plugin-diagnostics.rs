//! A plugin that answers the `diagnostics` capability over stdin/stdout (docs/38 §38.11).
//! Hot Sheet's supervisor tests run it as a real child process; plugin authors can use it
//! to see the protocol in action.
//!
//! Flags (for fault injection in tests):
//! * `--transports json,protobuf` — the transports it speaks (default: both)
//! * `--capability <name>` — report this capability instead of `diagnostics` (repeatable)
//! * `--exit-before-hello` — exit at once, before reading the handshake
//! * `--hang-at-hello` — read the handshake and never answer
//! * `--stderr <text>` — write a line to stderr first

use std::sync::Arc;

use hotsheet_plugin_protocol::diagnostics::{CAPABILITY, DiagnosticsHandler};
use hotsheet_plugin_protocol::serve::{ServeOptions, serve};
use hotsheet_plugin_protocol::{Transport, v1};

fn main() {
    let mut transports = vec![Transport::Protobuf, Transport::Json];
    let mut capabilities = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--transports" => {
                transports = args
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .filter_map(Transport::parse)
                    .collect();
            }
            "--capability" => capabilities.push(args.next().unwrap_or_default()),
            "--exit-before-hello" => std::process::exit(3),
            "--hang-at-hello" => {
                let mut line = String::new();
                // Read the hello so the host is not blocked writing it, then hang.
                let _ = std::io::stdin().read_line(&mut line);
                loop {
                    std::thread::park();
                }
            }
            "--stderr" => eprintln!("{}", args.next().unwrap_or_default()),
            other => {
                eprintln!("unknown flag {other}");
                std::process::exit(2);
            }
        }
    }
    if capabilities.is_empty() {
        capabilities.push(CAPABILITY.to_string());
    }
    let options = ServeOptions {
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: capabilities
            .into_iter()
            .map(|name| v1::Capability {
                name,
                features: Vec::new(),
            })
            .collect(),
        transports,
    };
    if let Err(error) = serve(
        std::io::stdin(),
        std::io::stdout(),
        &options,
        Arc::new(DiagnosticsHandler),
    ) {
        eprintln!("diagnostics plugin: {error}");
        std::process::exit(1);
    }
}
