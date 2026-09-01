//! Interactive AI-tool launches from an ordinary terminal, with the running Hot Sheet
//! server injected for permission-hook route-back (HS2-C46G58).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The subset of the server's machine-local instance record needed by a launched tool.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServerInstance {
    pub pid: u32,
    pub url: String,
    pub secret: String,
    pub store_path: String,
}

/// A fully resolved interactive launch. Kept as data so discovery and capability checks
/// can be unit-tested without replacing the test process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalLaunch {
    pub program: String,
    pub args: Vec<String>,
    pub server: ServerInstance,
}

/// Locate the per-store instance record written by `hotsheet-server`.
pub fn instance_path(home: &Path, store: &Path) -> PathBuf {
    let canonical = store.canonicalize().unwrap_or_else(|_| store.to_path_buf());
    let id = &hotsheet_index::hash_bytes(canonical.to_string_lossy().as_bytes())[..16];
    home.join("instances").join(format!("{id}.json"))
}

/// Resolve a capability-aware launch. Only tools declaring a permission hook may use this
/// external-terminal path: injecting environment variables into an unrelated native prompt
/// would otherwise misleadingly claim Hot Sheet is governing it.
pub fn prepare(
    store: &Path,
    tool: &str,
    extra_args: Vec<String>,
    home: &Path,
) -> Result<ExternalLaunch> {
    let plugin =
        hotsheet_plugins::find(tool).with_context(|| format!("unknown AI tool plugin '{tool}'"))?;
    let launch = plugin.manifest.launch.as_ref().with_context(|| {
        format!("AI tool '{tool}' does not declare an interactive terminal launch")
    })?;
    if plugin.manifest.hooks.is_none() {
        bail!(
            "AI tool '{tool}' cannot yet route permissions from its native interactive CLI \
             into Hot Sheet; use `hotsheet-cli trigger {tool}` for a Hot Sheet-driven turn"
        );
    }

    let record_path = instance_path(home, store);
    let text = std::fs::read_to_string(&record_path).with_context(|| {
        format!(
            "no running Hot Sheet server found for {}; start the web app or run \
             `hotsheet-cli serve` first (expected {})",
            store.display(),
            record_path.display()
        )
    })?;
    let server: ServerInstance = serde_json::from_str(&text)
        .with_context(|| format!("reading server instance {}", record_path.display()))?;
    if !process_is_alive(server.pid) {
        bail!(
            "the Hot Sheet server instance for {} is stale; start the web app or run \
             `hotsheet-cli serve` first",
            store.display()
        );
    }
    let canonical = store.canonicalize().unwrap_or_else(|_| store.to_path_buf());
    let recorded = PathBuf::from(&server.store_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&server.store_path));
    if canonical != recorded {
        bail!("server instance record belongs to a different ticket store");
    }

    let mut args = launch.args.clone();
    args.extend(extra_args);
    Ok(ExternalLaunch {
        program: launch.program.clone(),
        args,
        server,
    })
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    pid > 0
        && pid <= i32::MAX as u32
        && std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|status| status.success())
}

#[cfg(not(unix))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_ticketing::{FsStore, StoreMetadata};

    #[test]
    fn resolves_the_server_for_a_hook_capable_interactive_tool() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let store = root.path().join("tickets");
        FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();
        let path = instance_path(&home, &store);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            serde_json::json!({
                "pid": std::process::id(),
                "url": "http://127.0.0.1:8787",
                "secret": "test-secret",
                "store_path": store.canonicalize().unwrap(),
            })
            .to_string(),
        )
        .unwrap();

        let launch = prepare(&store, "claude", vec!["--resume".into()], &home).unwrap();
        assert_eq!(launch.args, vec!["--resume"]);
        assert_eq!(launch.server.secret, "test-secret");
    }

    #[test]
    fn refuses_to_claim_native_codex_permissions_are_routed() {
        let root = tempfile::tempdir().unwrap();
        let error = prepare(root.path(), "codex", vec![], root.path()).unwrap_err();
        assert!(error.to_string().contains("cannot yet route permissions"));
    }
}
