//! Interactive AI-tool launches from an ordinary terminal, with the running Hot Sheet
//! server injected for permission-hook route-back (HS2-C46G58).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hotsheet_ticketing::{FsStore, StoreMetadata};
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

/// Read the live server route for a ticket store. Retained terminal sessions may outlive
/// the server whose URL and secret they inherited, so permission hooks call this for each
/// request instead of trusting their launch-time environment indefinitely.
pub fn discover_running_server(store: &Path, home: &Path) -> Result<ServerInstance> {
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
    Ok(server)
}

/// Headless project launch uses the same machine bootstrap host as the graphical client.
/// The server owns all registered sources; this store is only its local instance anchor.
pub fn ensure_running_server(store: &Path, home: &Path) -> Result<ServerInstance> {
    if !store.join("hotsheet-store.json").exists() {
        FsStore::init(store, &StoreMetadata::new("HS"))?;
    } else {
        FsStore::open(store)?;
    }
    if let Ok(server) = discover_running_server(store, home) {
        return Ok(server);
    }
    let current = std::env::current_exe().context("locating the Hot Sheet binary")?;
    let cli = if current
        .file_stem()
        .is_some_and(|stem| stem == "hotsheet-cli")
    {
        current
    } else {
        let sibling = current.with_file_name("hotsheet-cli");
        if sibling.exists() {
            sibling
        } else {
            PathBuf::from("hotsheet-cli")
        }
    };
    let mut command = hotsheet_ticketing::git::launch(cli);
    command
        .arg("-C")
        .arg(store)
        .args(["serve", "--bind", "127.0.0.1:0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let context = hotsheet_process_journal::LaunchContext {
        journal: hotsheet_process_journal::ProcessJournal::at(home),
        supervisor: "hotsheet-cli",
        purpose: hotsheet_process_journal::Purpose::CliSetup,
        project_id: Some(hotsheet_tls::project_id(store)),
        session_id: None,
        binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
    };
    let mut child = context
        .spawn(&mut command)
        .context("starting the machine-local Hot Sheet project server")?;
    for _ in 0..100 {
        if let Ok(server) = discover_running_server(store, home) {
            return Ok(server);
        }
        if let Some(status) = child.try_wait()? {
            bail!("Hot Sheet project server exited during startup with {status}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("Hot Sheet project server did not become ready within 10 seconds")
}

/// Resolve a capability-aware launch. Only tools declaring a native permission hook may use
/// this external-terminal path: injecting environment variables into an unrelated prompt
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

    let server = discover_running_server(store, home)?;

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

    fn write_instance(root: &Path) -> (PathBuf, PathBuf) {
        let home = root.join("home");
        let store = root.join("tickets");
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
        (home, store)
    }

    #[test]
    fn resolves_the_server_for_a_hook_capable_interactive_tool() {
        let root = tempfile::tempdir().unwrap();
        let (home, store) = write_instance(root.path());

        let launch = prepare(&store, "claude", vec!["--resume".into()], &home).unwrap();
        assert_eq!(launch.args, vec!["--resume"]);
        assert_eq!(launch.server.secret, "test-secret");
    }

    #[test]
    fn resolves_native_codex_launch_now_that_it_declares_a_permission_hook() {
        let root = tempfile::tempdir().unwrap();
        let (home, store) = write_instance(root.path());
        let launch = prepare(
            &store,
            "codex",
            vec!["--model".into(), "gpt-test".into()],
            &home,
        )
        .unwrap();
        assert_eq!(launch.program, "codex");
        assert_eq!(launch.args, ["--model", "gpt-test"]);
    }

    #[test]
    fn retained_hook_discovers_replaced_server_route() {
        let root = tempfile::tempdir().unwrap();
        let (home, store) = write_instance(root.path());
        let first = discover_running_server(&store, &home).unwrap();
        assert_eq!(first.secret, "test-secret");

        let path = instance_path(&home, &store);
        std::fs::write(
            &path,
            serde_json::json!({
                "pid": std::process::id(),
                "url": "http://127.0.0.1:9797",
                "secret": "replacement-secret",
                "store_path": store.canonicalize().unwrap(),
            })
            .to_string(),
        )
        .unwrap();
        let replacement = discover_running_server(&store, &home).unwrap();
        assert_eq!(replacement.url, "http://127.0.0.1:9797");
        assert_eq!(replacement.secret, "replacement-secret");

        std::fs::write(
            &path,
            serde_json::json!({
                "pid": std::process::id(),
                "url": "http://127.0.0.1:9797",
                "secret": "wrong-project",
                "store_path": root.path(),
            })
            .to_string(),
        )
        .unwrap();
        assert!(discover_running_server(&store, &home).is_err());
    }
}
