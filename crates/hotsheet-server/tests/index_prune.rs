//! HS2-Y0PAEM: a real server start prunes index files left by older schema generations
//! from the machine index dir, but never one another process still holds open.

use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use hotsheet_index::{Index, index_file_name};
use hotsheet_ticketing::{FsStore, StoreMetadata};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        // The server runs in its own test-only process group; end the whole group so any
        // detached helper it started goes too.
        // SAFETY: a negative pid targets exactly the group this fixture created.
        unsafe { libc::kill(-(self.0.id() as i32), libc::SIGKILL) };
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The schema generation this build writes, read back from the public file-name contract.
fn current_generation() -> i64 {
    index_file_name("x")
        .trim_end_matches(".sqlite")
        .rsplit_once(".v")
        .unwrap()
        .1
        .parse()
        .unwrap()
}

fn wait_for_instance(server: &mut Server, home: &Path, store: &Path, log: &Path) {
    let instance = home
        .join("instances")
        .join(format!("{}.json", hotsheet_tls::project_id(store)));
    let deadline = Instant::now() + Duration::from_secs(20);
    while !instance.is_file() {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "server exited: {}",
            fs::read_to_string(log).unwrap_or_default()
        );
        assert!(
            Instant::now() < deadline,
            "server did not start: {}",
            fs::read_to_string(log).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn server_start_prunes_idle_older_generation_indexes_and_keeps_held_ones() {
    let home = tempfile::tempdir().unwrap();
    let store_dir = tempfile::tempdir().unwrap();
    FsStore::init(store_dir.path(), &StoreMetadata::new("HS")).unwrap();
    let index_dir = home.path().join("index");
    fs::create_dir_all(&index_dir).unwrap();

    let version = current_generation();
    let idle_older = index_dir.join(format!("aaaa.v{}.sqlite", version - 1));
    let idle_legacy = index_dir.join("bbbb.sqlite");
    let held_older = index_dir.join(format!("cccc.v{}.sqlite", version - 1));
    let newer = index_dir.join(format!("dddd.v{}.sqlite", version + 1));
    for path in [&idle_older, &idle_legacy, &newer] {
        drop(Index::open(path, "fixture").unwrap());
    }
    // This test process plays an older build's still-running server.
    let held = Index::open(&held_older, "older-build").unwrap();

    let log = home.path().join("server.log");
    let output = fs::File::create(&log).unwrap();
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_hotsheet-server"))
            .arg("-C")
            .arg(store_dir.path())
            .args(["--bind", "127.0.0.1:0", "--secret", "prune-test"])
            .env("HOTSHEET_HOME", home.path())
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .process_group(0)
            .spawn()
            .unwrap(),
    );
    wait_for_instance(&mut server, home.path(), store_dir.path(), &log);

    let text = fs::read_to_string(&log).unwrap();
    assert!(
        text.contains("pruned 2 index file(s) from older schema generations; kept 1"),
        "startup logs the prune: {text}"
    );
    assert!(!idle_older.exists(), "an idle older generation is pruned");
    assert!(!idle_legacy.exists(), "an idle unversioned index is pruned");
    assert!(held_older.exists(), "a held older generation is kept");
    assert!(newer.exists(), "a newer build's index is never pruned");
    // The holder's index still works: its file was not pulled out from under it.
    assert!(held.ticket_count().is_ok());
    drop(server);
    drop(held);
}
