//! Interactive AI-tool launches from an ordinary terminal, with the running Hot Sheet
//! server injected for permission-hook route-back (HS2-C46G58).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use std::{io::Read, io::Seek};

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

impl ServerInstance {
    /// Whether this host is anchored on `store`, so its store-default routes (ticket flow,
    /// activity) answer for that store rather than for another one.
    #[must_use]
    pub fn anchors(&self, store: &Path) -> bool {
        let canonical = store.canonicalize().unwrap_or_else(|_| store.to_path_buf());
        let recorded = PathBuf::from(&self.store_path);
        recorded.canonicalize().unwrap_or(recorded) == canonical
    }
}

/// The machine host's instance anchor, `<home>/server-bootstrap.hs2`.
#[must_use]
pub fn machine_host_store(home: &Path) -> PathBuf {
    home.join(hotsheet_ticketing::store_git::MACHINE_ANCHOR_DIR)
}

/// Which host a CLI command addresses (HS2-VG3E8N).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostScope {
    /// Host-wide routes (browser sessions, lifecycle): the machine host first, then a host a
    /// developer started on the selected store with `hotsheet-cli serve`.
    Machine,
    /// Project routes: a host anchored on the selected store first, then the machine host,
    /// which serves every registered project.
    Project,
}

/// Find the running local host for a CLI command (HS2-VG3E8N). Under the single machine
/// host model the desktop app and headless launches anchor one host on
/// `<home>/server-bootstrap.hs2`, so its instance record is keyed by that anchor, never by
/// the project store. A per-store record remains for a host started explicitly on a store.
/// Every CLI command that talks to the local server resolves its host here.
pub fn discover_local_host(store: &Path, home: &Path, scope: HostScope) -> Result<ServerInstance> {
    let machine = machine_host_store(home);
    let same = store.canonicalize().ok() == machine.canonicalize().ok() || store == machine;
    let candidates: Vec<&Path> = match (same, scope) {
        (true, _) => vec![machine.as_path()],
        (false, HostScope::Machine) => vec![machine.as_path(), store],
        (false, HostScope::Project) => vec![store, machine.as_path()],
    };
    let mut failures = Vec::new();
    for candidate in candidates {
        match discover_running_server(candidate, home) {
            Ok(server) => return Ok(server),
            Err(error) => failures.push(format!("{error:#}")),
        }
    }
    bail!(
        "no running Hot Sheet host found: open the Hot Sheet app, which runs the machine host, \
         or start one with `hotsheet-cli serve` ({})",
        failures.join("; ")
    )
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
    // HS2-HA63BE: isolated launch fixtures need the failed server's own diagnostic.
    // Keep it in an anonymous temporary file so a detached child cannot block on a pipe,
    // and never capture ordinary server stderr from a user's project.
    let mut fixture_stderr = if std::env::var_os("HOTSHEET_TEST_SERVER_OWNER_PID").is_some() {
        Some(tempfile::tempfile().context("creating isolated server startup diagnostic")?)
    } else {
        None
    };
    let stderr = match fixture_stderr.as_ref() {
        Some(file) => Stdio::from(
            file.try_clone()
                .context("cloning isolated server diagnostic")?,
        ),
        None => Stdio::null(),
    };
    command
        .arg("-C")
        .arg(store)
        .args(["serve", "--bind", "127.0.0.1:0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);
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
            if let Some(file) = fixture_stderr.as_mut() {
                file.rewind()
                    .context("reading isolated server startup diagnostic")?;
                let mut bytes = Vec::new();
                file.take(4096).read_to_end(&mut bytes)?;
                let diagnostic = String::from_utf8_lossy(&bytes);
                bail!(
                    "Hot Sheet project server exited during startup with {status}; isolated server stderr (first 4096 bytes): {diagnostic}"
                );
            }
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

/// Journal provenance for the interactive tool child (HS2-PZRQC9). `project_id` is the
/// project id of the store hosting the server the tool routes back to, the key of that
/// server's instance registration, like every other journal caller; it is never the
/// checkout id. The session id is the launch's worker id.
pub fn tool_launch_context(
    home: &Path,
    server_store: &Path,
    worker: &str,
) -> hotsheet_process_journal::LaunchContext {
    hotsheet_process_journal::LaunchContext {
        journal: hotsheet_process_journal::ProcessJournal::at(home),
        supervisor: "hotsheet-cli",
        purpose: hotsheet_process_journal::Purpose::CliExternal,
        project_id: Some(hotsheet_tls::project_id(server_store)),
        session_id: Some(worker.to_owned()),
        binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
    }
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

    /// HS2-PZRQC9: the external tool record carries the hosting store's 16-hex project id,
    /// which names that server's instance registration, not the checkout id.
    #[test]
    fn hs2_pzrqc9_tool_launch_records_the_store_project_id() {
        let root = tempfile::tempdir().unwrap();
        let (home, store) = write_instance(root.path());
        let context = tool_launch_context(&home, &store, "claude-01TEST");
        let project_id = context.project_id.as_deref().unwrap();
        assert_eq!(project_id.len(), 16);
        assert!(project_id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(
            instance_path(&home, &store).file_stem().unwrap(),
            project_id,
            "the record correlates with the server's instance registration"
        );
        assert_eq!(
            context.purpose,
            hotsheet_process_journal::Purpose::CliExternal
        );
        assert_eq!(context.session_id.as_deref(), Some("claude-01TEST"));

        #[cfg(unix)]
        {
            let mut command = std::process::Command::new("true");
            context.status(&mut command).unwrap();
            let records = hotsheet_process_journal::ProcessJournal::at(&home)
                .list()
                .unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].project_id.as_deref(), Some(project_id));
        }
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
        // HS2-4J7WCV: the headless launch shares the manifest's `--no-daemon`, ahead of the
        // caller's arguments, exactly like a Hot Sheet terminal launch.
        assert_eq!(launch.args, ["--no-daemon", "--model", "gpt-test"]);
        let resumed = prepare(
            &store,
            "codex",
            vec!["resume".into(), "--last".into()],
            &home,
        )
        .unwrap();
        assert_eq!(resumed.args, ["--no-daemon", "resume", "--last"]);
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

    fn write_record(home: &Path, anchor: &Path, pid: u32, secret: &str) {
        std::fs::create_dir_all(anchor).unwrap();
        let path = instance_path(home, anchor);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            serde_json::json!({
                "pid": pid,
                "url": "http://127.0.0.1:8787",
                "secret": secret,
                "store_path": anchor.canonicalize().unwrap(),
            })
            .to_string(),
        )
        .unwrap();
    }

    /// A pid no live process holds, so the record reads as stale.
    #[cfg(unix)]
    const DEAD_PID: u32 = i32::MAX as u32;

    #[test]
    fn hs2_vg3e8n_per_store_host_wins_for_project_and_machine_host_for_host_wide() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let store = root.path().join("project.hs2");
        write_record(&home, &store, std::process::id(), "per-store");

        // Only a per-store host: both scopes reach it.
        for scope in [HostScope::Project, HostScope::Machine] {
            let server = discover_local_host(&store, &home, scope).unwrap();
            assert_eq!(server.secret, "per-store");
            assert!(server.anchors(&store));
        }

        // With the machine host also running, host-wide routes prefer it.
        write_record(
            &home,
            &machine_host_store(&home),
            std::process::id(),
            "machine",
        );
        assert_eq!(
            discover_local_host(&store, &home, HostScope::Project)
                .unwrap()
                .secret,
            "per-store"
        );
        assert_eq!(
            discover_local_host(&store, &home, HostScope::Machine)
                .unwrap()
                .secret,
            "machine"
        );
    }

    #[test]
    fn hs2_vg3e8n_bootstrap_only_host_serves_a_project_store() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let store = root.path().join("project.hs2");
        std::fs::create_dir_all(&store).unwrap();
        let machine = machine_host_store(&home);
        write_record(&home, &machine, std::process::id(), "machine");

        for scope in [HostScope::Project, HostScope::Machine] {
            let server = discover_local_host(&store, &home, scope).unwrap();
            assert_eq!(server.secret, "machine");
            assert!(server.anchors(&machine));
            assert!(!server.anchors(&store));
        }
        // Selecting the anchor itself resolves to the same host.
        assert_eq!(
            discover_local_host(&machine, &home, HostScope::Project)
                .unwrap()
                .secret,
            "machine"
        );
    }

    #[cfg(unix)]
    #[test]
    fn hs2_vg3e8n_stale_bootstrap_record_is_not_a_host() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let store = root.path().join("project.hs2");
        std::fs::create_dir_all(&store).unwrap();
        write_record(&home, &machine_host_store(&home), DEAD_PID, "stale");

        for scope in [HostScope::Project, HostScope::Machine] {
            let message = format!(
                "{:#}",
                discover_local_host(&store, &home, scope).unwrap_err()
            );
            assert!(message.contains("machine host"), "{message}");
            assert!(message.contains("stale"), "{message}");
        }

        // A live per-store host still answers past the stale machine record.
        write_record(&home, &store, std::process::id(), "per-store");
        assert_eq!(
            discover_local_host(&store, &home, HostScope::Machine)
                .unwrap()
                .secret,
            "per-store"
        );
    }

    #[test]
    fn hs2_vg3e8n_no_host_names_the_machine_host_and_both_records() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let store = root.path().join("project.hs2");
        std::fs::create_dir_all(&store).unwrap();

        let message = format!(
            "{:#}",
            discover_local_host(&store, &home, HostScope::Project).unwrap_err()
        );
        assert!(
            message.contains("no running Hot Sheet host found"),
            "{message}"
        );
        assert!(message.contains("machine host"), "{message}");
        assert!(message.contains("hotsheet-cli serve"), "{message}");
        let store_record = instance_path(&home, &store);
        let machine_record = instance_path(&home, &machine_host_store(&home));
        assert!(
            message.contains(&store_record.display().to_string()),
            "{message}"
        );
        assert!(
            message.contains(&machine_record.display().to_string()),
            "{message}"
        );
    }
}
