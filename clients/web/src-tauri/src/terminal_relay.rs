//! HS2-HYF56Y: keep the macOS terminal launch relay installed while the desktop app runs in a
//! healthy desktop session. The app runs the same headless workflow a terminal user runs,
//! `hotsheet-cli terminal-relay install`, naming its bundled server, so both clients share one
//! idempotent install. A failure never blocks startup: servers then spawn terminal brokers
//! directly, as before, and say so in their log.

use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Install's own relay readiness wait is five seconds; leave room for launchctl itself.
const INSTALL_DEADLINE: Duration = Duration::from_secs(10);

/// Run the bundled CLI's relay install and wait a bounded time for it.
pub fn ensure_installed(cli: &Path, server: &Path) {
    let child = Command::new(cli)
        .args(["terminal-relay", "install", "--server"])
        .arg(server)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            eprintln!("Hot Sheet could not run the terminal launch relay install: {error}");
            return;
        }
    };
    let deadline = Instant::now() + INSTALL_DEADLINE;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    eprintln!(
                        "Hot Sheet terminal launch relay install exited with {status}; \
                         terminal brokers are spawned directly"
                    );
                }
                return;
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                eprintln!(
                    "Hot Sheet terminal launch relay install did not finish; terminal brokers \
                     are spawned directly"
                );
                return;
            }
        }
    }
}
