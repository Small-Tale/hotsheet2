#![cfg(unix)] // Unix process-group fixture; the terminal probe runs /bin/sh.

//! A machine that serves remote peers only over mTLS, as the second machine of a
//! two-machine orchestration does (HS2-GV12FJ rehearsal), end to end with the real
//! `hotsheet-server` and `hotsheet-cli` binaries on isolated homes:
//!
//! - HS2-QPJQ7A: its own CLI, sync kick, launched sessions and their permission hook use
//!   the loopback listener the server keeps beside the mTLS one.
//! - HS2-FZXXTG: a second isolated machine pairs with it headlessly from a pairing bundle,
//!   with no hand-built credential file and no bearer.
//!
//! All stores, tickets, devices and labels are synthetic.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use hotsheet_server::lifecycle::InstanceInfo;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};

#[path = "../../test-support/first_launch.rs"]
mod first_launch;
#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

const STARTUP: Duration = Duration::from_secs(30);

fn binary(name: &str) -> PathBuf {
    first_launch::launched(workspace_bin::path(name), &["--version"])
}

fn git(cwd: &Path, args: &[&str]) {
    let status = hotsheet_ticketing::git::command()
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// One isolated machine: its own `HOTSHEET_HOME`, a ticket store published to a bare
/// remote, and optionally its running server.
struct Machine {
    home: PathBuf,
    store: PathBuf,
    server: Option<Child>,
    log: PathBuf,
}

impl Machine {
    fn new(root: &Path, name: &str) -> Self {
        let base = root.join(name);
        let home = base.join("home");
        for directory in ["user-home", "cache", "tmp"] {
            std::fs::create_dir_all(home.join(directory)).unwrap();
        }
        let store = base.join("store.hs2");
        FsStore::init_git(&store, &StoreMetadata::new("SYN")).unwrap();
        git(&store, &["config", "user.name", "Synthetic Site"]);
        git(&store, &["config", "user.email", "site@test.invalid"]);
        git(&store, &["add", "-A"]);
        git(
            &store,
            &["commit", "--quiet", "--allow-empty", "-m", "init"],
        );
        let remote = base.join("remote.git");
        git(
            &base,
            &["init", "--quiet", "--bare", remote.to_str().unwrap()],
        );
        git(
            &store,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&store, &["push", "--quiet", "-u", "origin", "HEAD"]);
        Self {
            log: base.join("server.log"),
            home,
            store,
            server: None,
        }
    }

    fn isolate(&self, command: &mut Command) {
        command
            .env("HOME", self.home.join("user-home"))
            .env("HOTSHEET_HOME", &self.home)
            .env("HOTSHEET_CACHE_DIR", self.home.join("cache"))
            .env("HOTSHEET_NATIVE_SECRETS", "deny")
            .env("HOTSHEET_AI_TOOL_PROBES", "off")
            .env("TMPDIR", self.home.join("tmp"))
            .env("PATH", "/usr/bin:/bin")
            .env_remove("HOTSHEET_STORE")
            .env_remove("HOTSHEET_SERVER")
            .env_remove("HOTSHEET_SECRET")
            .env_remove("HOTSHEET_TERMINAL_ID")
            .env_remove("HOTSHEET_PROJECT")
            .env_remove("HOTSHEET_PERMISSION_PROJECT")
            .env_remove("HOTSHEET_ACTOR_ROLE")
            .env_remove("HOTSHEET_ACTOR_ID")
            .current_dir(&self.home);
    }

    /// `hotsheet-cli` on this machine, with synthetic fixture credentials in place of the
    /// OS keychain.
    fn cli_command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(binary("hotsheet-cli"));
        self.isolate(&mut command);
        command.arg("--test-fixture-credentials").args(args);
        command
    }

    fn cli(&self, args: &[&str]) -> Output {
        self.cli_command(args).output().unwrap()
    }

    fn cli_ok(&self, args: &[&str]) -> Output {
        let output = self.cli(args);
        assert!(
            output.status.success(),
            "hotsheet-cli {args:?} failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn cli_json(&self, args: &[&str]) -> Value {
        serde_json::from_slice(&self.cli_ok(args).stdout).unwrap()
    }

    fn store_arg(&self) -> &str {
        self.store.to_str().unwrap()
    }

    /// Start this machine's server and wait for its discovery record.
    fn serve(&mut self, extra: &[&str]) -> InstanceInfo {
        let log = std::fs::File::create(&self.log).unwrap();
        let mut command = Command::new(binary("hotsheet-server"));
        self.isolate(&mut command);
        use std::os::unix::process::CommandExt;
        let child = command
            .arg("-C")
            .arg(&self.store)
            .args(["--bind", "127.0.0.1:0", "--no-terminal-broker"])
            .args(extra)
            // Piped and held: the server stops if this test dies (HS2-VQ8ZWT).
            .arg("--exit-on-stdin-eof")
            .stdin(Stdio::piped())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        self.server = Some(child);
        let record = self
            .home
            .join("instances")
            .join(format!("{}.json", hotsheet_tls::project_id(&self.store)));
        let deadline = Instant::now() + STARTUP;
        loop {
            if let Ok(text) = std::fs::read_to_string(&record)
                && let Ok(info) = serde_json::from_str::<InstanceInfo>(&text)
                && info.pid == pid
            {
                return info;
            }
            assert!(
                self.server.as_mut().unwrap().try_wait().unwrap().is_none(),
                "server exited: {}",
                self.log()
            );
            assert!(
                Instant::now() < deadline,
                "server did not start: {}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Some(mut child) = self.server.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn get(info: &InstanceInfo, path: &str) -> Value {
    ureq::get(&format!("{}{path}", info.url))
        .set("x-hotsheet-secret", info.secret.expose())
        .call()
        .unwrap_or_else(|error| panic!("GET {path}: {error}"))
        .into_string()
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap()
}

fn send(info: &InstanceInfo, method: &str, path: &str, body: &Value) -> Value {
    ureq::request(method, &format!("{}{path}", info.url))
        .set("x-hotsheet-secret", info.secret.expose())
        .set("content-type", "application/json")
        .send_string(&body.to_string())
        .unwrap_or_else(|error| panic!("{method} {path}: {error}"))
        .into_string()
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap()
}

/// Wait until `read` returns a value; the server answers each read from its own state.
fn until<T>(what: &str, machine: &Machine, mut read: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + STARTUP;
    loop {
        if let Some(value) = read() {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "{what} never happened: {}",
            machine.log()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// An mTLS site with its CA initialized and its server running.
fn mtls_site(root: &Path) -> (Machine, InstanceInfo) {
    let mut site = Machine::new(root, "site");
    site.cli_ok(&["cert", "init", "-C", site.store_arg()]);
    let info = site.serve(&["--remote-transport", "mtls"]);
    (site, info)
}

#[test]
fn hs2_qpjq7a_local_clients_reach_a_server_that_serves_remote_peers_only_over_mtls() {
    let root = tempfile::tempdir().unwrap();
    let (site, info) = mtls_site(root.path());

    // The discovery record names the loopback listener for local clients and keeps the
    // mTLS listener beside it.
    assert!(info.url.starts_with("http://127.0.0.1:"), "{info:?}");
    let remote = info
        .remote_url
        .clone()
        .expect("the remote listener is recorded");
    assert!(remote.starts_with("https://127.0.0.1:"), "{remote}");
    assert_ne!(remote.replace("https://", "http://"), info.url);
    assert!(site.log().contains(&format!("local clients: {}", info.url)));

    // A local CLI command: the inventory health-checks the loopback listener.
    let listed = String::from_utf8(site.cli_ok(&["serve", "--list"]).stdout).unwrap();
    assert!(
        listed.contains(&format!("healthy   pid={} url={}", info.pid, info.url)),
        "{listed}"
    );
    assert!(
        listed.contains(&format!("  remote listener: {remote}")),
        "{listed}"
    );
    assert!(!listed.contains(info.secret.expose()), "{listed}");
    // A CLI read the server answers.
    site.cli_ok(&["-C", site.store_arg(), "live-state"]);

    // The sync kick after a CLI write reaches the host instead of failing TLS.
    let write = site.cli_ok(&[
        "-C",
        site.store_arg(),
        "new",
        "--title",
        "Synthetic kick ticket",
        "--category",
        "task",
    ]);
    let stderr = String::from_utf8_lossy(&write.stderr);
    assert!(
        !stderr.contains("may be local only") && !stderr.contains("UnknownIssuer"),
        "{stderr}"
    );
    assert_eq!(
        hotsheet_cli::sync_kick::request_host_sync(&site.home, &site.store),
        hotsheet_cli::sync_kick::KickOutcome::Accepted
    );

    // A session the server launches is routed back to the loopback listener.
    let probe = "printf 'route=%s|%s|%s|%s\\n' \"$HOTSHEET_SERVER\" \"$HOTSHEET_SECRET\" \
                 \"$HOTSHEET_PROJECT\" \"$HOTSHEET_PERMISSION_PROJECT\"; sleep 60";
    let terminal = send(
        &info,
        "POST",
        "/terminals",
        &json!({"command": "/bin/sh", "args": ["-c", probe], "cwd": site.store, "id": "qpjq7a-session"}),
    );
    let terminal_id = terminal["id"].as_str().unwrap().to_owned();
    let route = until("the session's route-back line", &site, || {
        let read = get(&info, &format!("/terminals/{terminal_id}"));
        let scrollback = read["scrollback"].as_str()?.to_owned();
        let line = scrollback.lines().find(|line| line.starts_with("route="))?;
        Some(line.trim_end().trim_start_matches("route=").to_owned())
    });
    let route: Vec<&str> = route.split('|').collect();
    assert_eq!(route[0], info.url, "the session's HOTSHEET_SERVER");
    assert_eq!(
        route[1],
        info.secret.expose(),
        "the session's HOTSHEET_SECRET"
    );

    // That session's permission hook raises its prompt in Hot Sheet and gets the answer.
    let mut hook = site.cli_command(&["permission-hook", "--agent", "claude"]);
    hook.env("HOTSHEET_SERVER", route[0])
        .env("HOTSHEET_SECRET", route[1])
        .env("HOTSHEET_PROJECT", route[2])
        .env("HOTSHEET_PERMISSION_PROJECT", route[3])
        .env("HOTSHEET_TERMINAL_ID", &terminal_id)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut hook = hook.spawn().unwrap();
    hook.stdin
        .take()
        .unwrap()
        .write_all(
            json!({
                "hook_event_name": "PermissionRequest",
                "session_id": "qpjq7a-session",
                "tool_name": "Bash",
                "tool_input": {"command": "git status"},
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();
    let request = until("the hook's permission request", &site, || {
        get(&info, "/permissions")
            .as_array()?
            .iter()
            .find(|request| request["action"] == "git status")?["id"]
            .as_u64()
    });
    send(
        &info,
        "POST",
        &format!("/permissions/{request}"),
        &json!({"decision": "allow", "scope": "once"}),
    );
    let answered = hook.wait_with_output().unwrap();
    assert!(answered.status.success());
    let decision: Value = serde_json::from_slice(&answered.stdout).unwrap_or_else(|_| {
        panic!(
            "the hook printed no decision: {}{}",
            String::from_utf8_lossy(&answered.stdout),
            String::from_utf8_lossy(&answered.stderr)
        )
    });
    assert_eq!(
        decision["hookSpecificOutput"]["decision"]["behavior"],
        "allow"
    );
    ureq::delete(&format!("{}/terminals/{terminal_id}", info.url))
        .set("x-hotsheet-secret", info.secret.expose())
        .call()
        .unwrap();
}

#[test]
fn hs2_fzxxtg_two_isolated_machines_pair_headlessly_from_a_bundle() {
    let root = tempfile::tempdir().unwrap();
    let (site, site_info) = mtls_site(root.path());
    let remote = site_info.remote_url.clone().unwrap();
    // A code checkout on the site that the home should discover.
    let code = root.path().join("site/widgets");
    std::fs::create_dir_all(&code).unwrap();
    let checkout = site.cli_json(&[
        "checkout",
        "register",
        code.to_str().unwrap(),
        "--store",
        site.store_arg(),
        "--alias",
        "widgets",
    ]);
    let checkout_id = checkout["id"].as_str().unwrap().to_owned();

    // The site writes the bundle: no PEM copying, no bearer.
    let bundle = root.path().join("site/home-orchestrator.json");
    let bundle_arg = bundle.to_str().unwrap();
    let issued = site.cli_json(&[
        "cert",
        "issue",
        "home-orchestrator",
        "-C",
        site.store_arg(),
        "--bundle",
        bundle_arg,
        "--origin",
        &remote,
        "--role",
        "read-write",
    ]);
    let identity = site.cli_json(&["remote", "identity"]);
    assert_eq!(issued["server_id"], identity["server_id"]);
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&bundle).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the bundle holds a private key");
    }
    let written: Value = serde_json::from_str(&std::fs::read_to_string(&bundle).unwrap()).unwrap();
    assert_eq!(written["format"], "hotsheet-pairing-bundle/v1");
    assert_eq!(written["origin"], remote.as_str());
    assert_eq!(written["credentials"]["bearer_secret"], "");
    // Issuing to an existing file is refused before a second device is issued.
    let again = site.cli(&[
        "cert",
        "issue",
        "home-orchestrator-2",
        "-C",
        site.store_arg(),
        "--bundle",
        bundle_arg,
    ]);
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("already exists"));

    // The home machine runs its own isolated server and imports the bundle.
    let mut home = Machine::new(root.path(), "home");
    let home_info = home.serve(&[]);
    assert_eq!(
        home_info.remote_url, None,
        "a local-only server has one listener"
    );
    let registered = home.cli_json(&[
        "remote",
        "register",
        "--label",
        "Synthetic site",
        "--bundle",
        bundle_arg,
    ]);
    let id = registered["remote"]["id"].as_str().unwrap().to_owned();
    assert_eq!(registered["remote"]["origin"], remote.as_str());
    assert_eq!(
        registered["remote"]["identity"]["server_id"],
        identity["server_id"]
    );
    assert_eq!(registered["checkouts"][0]["id"], checkout_id.as_str());
    assert_eq!(registered["checkouts"][0]["state"], "available");

    // Importing again is idempotent: the same registration, reviewed in place.
    let repeated = home.cli_json(&[
        "remote",
        "register",
        "--label",
        "Synthetic site",
        "--credentials-file",
        bundle_arg,
    ]);
    assert_eq!(repeated["remote"]["id"], id.as_str());
    assert_eq!(
        home.cli_json(&["remote", "list"]).as_array().unwrap().len(),
        1
    );

    // A typed server id that contradicts the bundle is refused.
    let contradicted = home.cli(&[
        "remote",
        "register",
        "--label",
        "Synthetic site",
        "--bundle",
        bundle_arg,
        "--expected-server-id",
        "01BX5ZZKBKACTAV9WEVGEMMVRZ",
    ]);
    assert!(!contradicted.status.success());

    // The home's own server uses the imported binding: fresh discovery over mTLS.
    let discovered = get(&home_info, &format!("/remotes/{id}/checkouts"));
    assert_eq!(discovered["checkouts"][0]["id"], checkout_id.as_str());
    assert_eq!(discovered["checkouts"][0]["state"], "available");
}
