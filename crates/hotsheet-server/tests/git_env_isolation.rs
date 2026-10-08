//! HS2-RRD417: the real `hotsheet-server` binary started under an inherited git
//! repository environment (as from a git hook, `git rebase --exec`, or `git bisect run`)
//! must commit ticket writes only to its own store. `GIT_DIR`, `GIT_WORK_TREE`,
//! `GIT_INDEX_FILE`, and friends point at a throwaway *sentinel* repository, which must
//! stay byte-for-byte unchanged.

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use hotsheet_server::lifecycle::InstanceInfo;
use hotsheet_ticketing::{FsStore, StoreMetadata};

const IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "Hot Sheet test"),
    ("GIT_AUTHOR_EMAIL", "hotsheet@example.invalid"),
    ("GIT_COMMITTER_NAME", "Hot Sheet test"),
    ("GIT_COMMITTER_EMAIL", "hotsheet@example.invalid"),
];

fn git(dir: &Path, args: &[&str]) -> String {
    let out = hotsheet_ticketing::git::command_in(dir)
        .args(args)
        .envs(IDENTITY)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    snapshot_with_observer(dir, |_| {})
}

fn snapshot_with_observer(
    dir: &Path,
    mut observed: impl FnMut(&Path),
) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(
        base: &Path,
        dir: &Path,
        out: &mut BTreeMap<PathBuf, Vec<u8>>,
        observed: &mut impl FnMut(&Path),
    ) -> Result<(), (PathBuf, std::io::Error)> {
        let entries = std::fs::read_dir(dir).map_err(|error| (dir.to_owned(), error))?;
        for entry in entries {
            let path = entry.map_err(|error| (dir.to_owned(), error))?.path();
            observed(&path);
            let metadata = std::fs::metadata(&path).map_err(|error| (path.clone(), error))?;
            if metadata.is_dir() {
                walk(base, &path, out, observed)?;
            } else {
                let bytes = std::fs::read(&path).map_err(|error| (path.clone(), error))?;
                out.insert(path.strip_prefix(base).unwrap().to_owned(), bytes);
            }
        }
        Ok(())
    }

    // Git can remove a temporary repository entry after read_dir returns it. Restart
    // the whole walk so no partial snapshot can weaken the byte-for-byte comparison.
    const ATTEMPTS: usize = 4;
    for attempt in 1..=ATTEMPTS {
        let mut out = BTreeMap::new();
        match walk(dir, dir, &mut out, &mut observed) {
            Ok(()) => return out,
            Err((_, error))
                if error.kind() == std::io::ErrorKind::NotFound && attempt < ATTEMPTS => {}
            Err((path, error)) => {
                panic!(
                    "could not snapshot {} after {attempt} attempt(s): {error}",
                    path.display()
                );
            }
        }
    }
    unreachable!()
}

fn assert_sentinel_unchanged(before: &BTreeMap<PathBuf, Vec<u8>>, sentinel: &Path, stage: &str) {
    let after = snapshot(sentinel);
    let changed: Vec<_> = before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|path| before.get(*path) != after.get(*path))
        .map(|path| {
            format!(
                "{} ({} -> {} bytes)",
                path.display(),
                before.get(path).map(Vec::len).unwrap_or(0),
                after.get(path).map(Vec::len).unwrap_or(0)
            )
        })
        .collect();
    assert!(
        changed.is_empty(),
        "the inherited GIT_DIR repository changed during {stage}; changed paths: {changed:?}"
    );
}

#[test]
fn snapshot_restarts_after_a_vanishing_entry_without_hiding_sentinel_changes() {
    let root = tempfile::tempdir().unwrap();
    let git_dir = root.path().join(".git");
    std::fs::create_dir(&git_dir).unwrap();
    std::fs::write(root.path().join("README"), b"sentinel\n").unwrap();
    let before = snapshot(root.path());

    let transient = git_dir.join("index.lock");
    std::fs::write(&transient, b"temporary").unwrap();
    let mut removed = false;
    let after = snapshot_with_observer(root.path(), |path| {
        if path == transient && !removed {
            std::fs::remove_file(path).unwrap();
            removed = true;
        }
    });
    assert!(removed, "the disappearing-entry boundary was exercised");
    assert_eq!(before, after);

    std::fs::write(root.path().join("README"), b"changed\n").unwrap();
    assert_ne!(before, snapshot(root.path()));
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        // SAFETY: the negative id is this fixture's own process group.
        unsafe { libc::kill(-(self.0.id() as i32), libc::SIGKILL) };
        let _ = self.0.wait();
    }
}

/// A committed throwaway repository the hostile environment points at.
fn sentinel_repo(root: &Path) -> PathBuf {
    let sentinel = root.join("sentinel");
    std::fs::create_dir(&sentinel).unwrap();
    git(&sentinel, &["init", "-q", "-b", "main"]);
    // The fixture's own commit must not start asynchronous Git maintenance.
    git(&sentinel, &["config", "maintenance.auto", "false"]);
    std::fs::write(sentinel.join("README"), "sentinel\n").unwrap();
    git(&sentinel, &["add", "README"]);
    git(&sentinel, &["commit", "-q", "-m", "sentinel"]);
    // Git may still be finishing automatic maintenance after commit exits. A
    // baseline captured while this lock exists would differ once Git removes it,
    // even when the server never touches the sentinel repository.
    let maintenance_lock = sentinel.join(".git/objects/maintenance.lock");
    let deadline = Instant::now() + Duration::from_secs(5);
    while maintenance_lock.exists() {
        assert!(
            Instant::now() < deadline,
            "Git maintenance did not release {}",
            maintenance_lock.display()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    sentinel
}

/// The real server on a fresh store, started with `GIT_DIR` and friends aimed at
/// `sentinel`, as from a git hook. Returns the server, its instance info, and its log.
fn hostile_server(root: &Path, sentinel: &Path) -> (Server, InstanceInfo, PathBuf, PathBuf) {
    let store = root.join("store");
    std::fs::create_dir(&store).unwrap();
    git(&store, &["init", "-q", "-b", "main"]);
    FsStore::init(&store, &StoreMetadata::new("ISO")).unwrap();
    let home = root.join("home");
    std::fs::create_dir(&home).unwrap();
    let log = root.join("server.log");
    let file = std::fs::File::create(&log).unwrap();

    let git_dir = sentinel.join(".git");
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_hotsheet-server"))
            .arg("-C")
            .arg(&store)
            .args(["--bind", "127.0.0.1:0", "--secret", "iso-test"])
            .args(["--no-terminal-broker", "--exit-on-stdin-eof"])
            .env("HOTSHEET_HOME", &home)
            // Keep locally installed AI tools out of startup discovery.
            .env("PATH", "/usr/bin:/bin")
            .envs(IDENTITY)
            .env("GIT_DIR", &git_dir)
            .env("GIT_WORK_TREE", sentinel)
            .env("GIT_INDEX_FILE", git_dir.join("index"))
            .env("GIT_OBJECT_DIRECTORY", git_dir.join("objects"))
            .env("GIT_COMMON_DIR", &git_dir)
            .stdin(Stdio::piped())
            .stdout(file.try_clone().unwrap())
            .stderr(file)
            .process_group(0)
            .spawn()
            .unwrap(),
    );
    let project = hotsheet_tls::project_id(&store);
    let instance = home.join("instances").join(format!("{project}.json"));
    let deadline = Instant::now() + Duration::from_secs(15);
    let info: InstanceInfo = loop {
        let server_log = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "server exited: {server_log}"
        );
        if let Ok(text) = std::fs::read_to_string(&instance)
            && let Ok(info) = serde_json::from_str::<InstanceInfo>(&text)
            && info.pid == server.0.id()
        {
            break info;
        }
        assert!(
            Instant::now() < deadline,
            "server did not start: {server_log}"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    (server, info, store, log)
}

#[test]
fn server_ticket_writes_ignore_an_inherited_git_repository_environment() {
    let root = tempfile::tempdir().unwrap();
    let sentinel = sentinel_repo(root.path());
    let before = snapshot(&sentinel);
    let (server, info, store, log) = hostile_server(root.path(), &sentinel);
    assert_sentinel_unchanged(&before, &sentinel, "server startup");
    let server_log = || std::fs::read_to_string(&log).unwrap_or_default();

    let response = ureq::post(&format!("{}/tickets", info.url))
        .set("x-hotsheet-secret", &info.secret)
        .set("content-type", "application/json")
        .send_string(r#"{"title":"isolated server ticket"}"#)
        .unwrap();
    assert!(response.status() < 300);
    assert_sentinel_unchanged(&before, &sentinel, "ticket creation");

    // The committing write lands in the store's own history.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let log = git(&store, &["log", "--all", "--format=%s", "--name-only"]);
        if log.contains("tickets/") || log.contains(".md") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "ticket write never committed to the store: {log}\n{}",
            server_log()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(git(&store, &["rev-parse", "--is-bare-repository"]), "false");
    drop(server);

    assert_sentinel_unchanged(&before, &sentinel, "server shutdown");
    assert_eq!(git(&sentinel, &["config", "--get", "core.bare"]), "false");
    assert_eq!(git(&sentinel, &["rev-list", "--count", "HEAD"]), "1");
}

/// Polls `GET {url}{path}` until `done` accepts the JSON body.
fn poll_json(
    info: &InstanceInfo,
    path: &str,
    log: &Path,
    done: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let body: serde_json::Value = ureq::get(&format!("{}{path}", info.url))
            .set("x-hotsheet-secret", &info.secret)
            .call()
            .unwrap()
            .into_string()
            .map(|text| serde_json::from_str(&text).unwrap())
            .unwrap();
        if done(&body) {
            return body;
        }
        assert!(
            Instant::now() < deadline,
            "{path} never settled: {body}\n{}",
            std::fs::read_to_string(log).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// HS2-J79CZF: processes the server launches for users and agents (configured commands
/// and terminal shells) start without the inherited repository variables, so `git` in a
/// code checkout reports that checkout rather than the hook's repository.
#[test]
fn launched_commands_and_terminals_drop_an_inherited_git_repository_environment() {
    let root = tempfile::tempdir().unwrap();
    let sentinel = sentinel_repo(root.path());
    let before = snapshot(&sentinel);
    let checkout = root.path().join("checkout");
    std::fs::create_dir(&checkout).unwrap();
    git(&checkout, &["init", "-q", "-b", "main"]);
    let checkout = std::fs::canonicalize(&checkout).unwrap();
    let (server, info, _store, log) = hostile_server(root.path(), &sentinel);
    assert_sentinel_unchanged(&before, &sentinel, "server startup");

    // What a launched process sees: the probed variables, then git's own answer.
    let probe = "printf 'env=%s|%s|%s|%s\\n' \"${GIT_DIR-unset}\" \"${GIT_WORK_TREE-unset}\" \
                 \"${GIT_INDEX_FILE-unset}\" \"${GIT_AUTHOR_NAME-unset}\"; \
                 printf 'top=%s\\n' \"$(git rev-parse --show-toplevel)\"";
    let expected_env = "env=unset|unset|unset|Hot Sheet test";
    let expected_top = format!("top={}", checkout.display());

    let definitions = serde_json::json!([{
        "id": "probe",
        "title": "Probe",
        "program": "/bin/sh",
        "args": ["-c", probe],
        "cwd": checkout,
    }]);
    let saved = ureq::put(&format!("{}/commands", info.url))
        .set("x-hotsheet-secret", &info.secret)
        .set("content-type", "application/json")
        .send_string(&definitions.to_string())
        .unwrap();
    assert!(saved.status() < 300);
    assert_sentinel_unchanged(&before, &sentinel, "command setup");
    let run: serde_json::Value = ureq::post(&format!("{}/commands/probe/run", info.url))
        .set("x-hotsheet-secret", &info.secret)
        .call()
        .unwrap()
        .into_string()
        .map(|text| serde_json::from_str(&text).unwrap())
        .unwrap();
    let run_id = run["id"].as_str().unwrap().to_owned();
    let finished = poll_json(&info, &format!("/command-runs/{run_id}"), &log, |body| {
        body["exit_code"].is_number()
    });
    let output: String = finished["output"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|line| line["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(finished["exit_code"], 0, "command output: {output}");
    assert!(output.contains(expected_env), "command output: {output}");
    assert!(output.contains(&expected_top), "command output: {output}");
    assert_sentinel_unchanged(&before, &sentinel, "command run");

    let terminal: serde_json::Value = ureq::post(&format!("{}/terminals", info.url))
        .set("x-hotsheet-secret", &info.secret)
        .set("content-type", "application/json")
        .send_string(
            &serde_json::json!({
                "command": "/bin/sh",
                "args": ["-c", format!("{probe}; sleep 30")],
                "cwd": checkout,
            })
            .to_string(),
        )
        .unwrap()
        .into_string()
        .map(|text| serde_json::from_str::<serde_json::Value>(&text).unwrap())
        .unwrap();
    let terminal_id = terminal["id"].as_str().unwrap().to_owned();
    let read = poll_json(&info, &format!("/terminals/{terminal_id}"), &log, |body| {
        body["scrollback"]
            .as_str()
            .is_some_and(|text| text.contains("top="))
    });
    let scrollback = read["scrollback"].as_str().unwrap();
    assert!(scrollback.contains(expected_env), "terminal: {scrollback}");
    assert!(scrollback.contains(&expected_top), "terminal: {scrollback}");
    assert_sentinel_unchanged(&before, &sentinel, "terminal run");
    drop(server);

    assert_sentinel_unchanged(&before, &sentinel, "server shutdown");
}
