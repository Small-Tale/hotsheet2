//! HS2-RRD417: the real `hotsheet-server` binary started under an inherited git
//! repository environment (as from a git hook, `git rebase --exec`, or `git bisect run`)
//! must commit ticket writes only to its own store. `GIT_DIR`, `GIT_WORK_TREE`,
//! `GIT_INDEX_FILE`, and friends point at a throwaway *sentinel* repository, which must
//! stay byte-for-byte unchanged.

use std::collections::BTreeMap;
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
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                out.insert(path.strip_prefix(base).unwrap().to_owned(), bytes);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        // SAFETY: the negative id is this fixture's own process group.
        unsafe { libc::kill(-(self.0.id() as i32), libc::SIGKILL) };
        let _ = self.0.wait();
    }
}

#[test]
fn server_ticket_writes_ignore_an_inherited_git_repository_environment() {
    let root = tempfile::tempdir().unwrap();
    let sentinel = root.path().join("sentinel");
    std::fs::create_dir(&sentinel).unwrap();
    git(&sentinel, &["init", "-q", "-b", "main"]);
    std::fs::write(sentinel.join("README"), "sentinel\n").unwrap();
    git(&sentinel, &["add", "README"]);
    git(&sentinel, &["commit", "-q", "-m", "sentinel"]);
    let before = snapshot(&sentinel);

    let store = root.path().join("store");
    std::fs::create_dir(&store).unwrap();
    git(&store, &["init", "-q", "-b", "main"]);
    FsStore::init(&store, &StoreMetadata::new("ISO")).unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let log = root.path().join("server.log");
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
            .env("GIT_WORK_TREE", &sentinel)
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
    let server_log = || std::fs::read_to_string(&log).unwrap_or_default();

    let project = hotsheet_tls::project_id(&store);
    let instance = home.join("instances").join(format!("{project}.json"));
    let deadline = Instant::now() + Duration::from_secs(15);
    let info: InstanceInfo = loop {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "server exited: {}",
            server_log()
        );
        if let Ok(text) = std::fs::read_to_string(&instance)
            && let Ok(info) = serde_json::from_str::<InstanceInfo>(&text)
            && info.pid == server.0.id()
        {
            break info;
        }
        assert!(
            Instant::now() < deadline,
            "server did not start: {}",
            server_log()
        );
        std::thread::sleep(Duration::from_millis(25));
    };

    let response = ureq::post(&format!("{}/tickets", info.url))
        .set("x-hotsheet-secret", &info.secret)
        .set("content-type", "application/json")
        .send_string(r#"{"title":"isolated server ticket"}"#)
        .unwrap();
    assert!(response.status() < 300);

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

    assert_eq!(
        before,
        snapshot(&sentinel),
        "the inherited GIT_DIR repository must be byte-for-byte untouched"
    );
    assert_eq!(git(&sentinel, &["config", "--get", "core.bare"]), "false");
    assert_eq!(git(&sentinel, &["rev-list", "--count", "HEAD"]), "1");
}
