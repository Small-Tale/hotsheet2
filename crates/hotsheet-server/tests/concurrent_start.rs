//! Real `hotsheet-server` binaries started at the same instant on one store (HS2-585JCP):
//! exactly one keeps serving and owns the registration; every other starter exits instead
//! of lingering as an unregistered, orphaned listener.

use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use hotsheet_server::lifecycle::InstanceInfo;
use hotsheet_ticketing::{FsStore, StoreMetadata};

struct Group(Vec<Child>);

impl Drop for Group {
    fn drop(&mut self) {
        for child in &mut self.0 {
            // SAFETY: each child leads its own test-only process group.
            unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
            let _ = child.wait();
        }
    }
}

#[test]
fn simultaneous_starts_leave_exactly_one_registered_server() {
    const STARTERS: usize = 6;
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    fs::create_dir(&home).unwrap();
    let store = fixture.path().join("store");
    FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();
    let project = hotsheet_tls::project_id(&store);
    let instance = home.join("instances").join(format!("{project}.json"));
    let lock = home.join("instances").join(format!("{project}.lock"));

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(STARTERS));
    let mut group = Group(
        (0..STARTERS)
            .map(|i| {
                let barrier = barrier.clone();
                let home = home.clone();
                let store = store.clone();
                let log = fixture.path().join(format!("server-{i}.log"));
                std::thread::spawn(move || {
                    let output = fs::File::create(log).unwrap();
                    barrier.wait();
                    Command::new(env!("CARGO_BIN_EXE_hotsheet-server"))
                        .arg("-C")
                        .arg(&store)
                        .args(["--bind", "127.0.0.1:0", "--no-terminal-broker"])
                        .env("HOTSHEET_HOME", &home)
                        .env("PATH", "/usr/bin:/bin")
                        .stdin(Stdio::null())
                        .stdout(output.try_clone().unwrap())
                        .stderr(output)
                        .process_group(0)
                        .spawn()
                        .unwrap()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect(),
    );
    let logs = || {
        (0..STARTERS)
            .map(|i| fs::read_to_string(fixture.path().join(format!("server-{i}.log"))))
            .filter_map(Result::ok)
            .collect::<Vec<_>>()
            .join("\n---\n")
    };

    // Settle: every loser exits; the survivors must be exactly one registered server.
    let deadline = Instant::now() + Duration::from_secs(30);
    let survivor = loop {
        let alive: Vec<usize> = (0..STARTERS)
            .filter(|&i| group.0[i].try_wait().unwrap().is_none())
            .collect();
        let registered = fs::read_to_string(&instance)
            .ok()
            .and_then(|text| serde_json::from_str::<InstanceInfo>(&text).ok());
        if let [only] = alive.as_slice()
            && registered
                .as_ref()
                .is_some_and(|info| info.pid == group.0[*only].id())
        {
            break *only;
        }
        assert!(
            Instant::now() < deadline,
            "did not settle to one registered server (alive: {alive:?}, registered: {registered:?}):\n{}",
            logs()
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let winner = group.0[survivor].id();
    assert_eq!(fs::read_to_string(&lock).unwrap(), winner.to_string());

    // The losers stay gone and never displaced the winner's registration or lock.
    std::thread::sleep(Duration::from_millis(500));
    for (i, child) in group.0.iter_mut().enumerate() {
        if i != survivor {
            assert!(child.try_wait().unwrap().is_some(), "starter {i} lingers");
        }
    }
    let info: InstanceInfo = serde_json::from_str(&fs::read_to_string(&instance).unwrap()).unwrap();
    assert_eq!(info.pid, winner);
    assert_eq!(fs::read_to_string(&lock).unwrap(), winner.to_string());

    // A graceful stop of the winner releases both.
    // SAFETY: positive pid of this fixture's child.
    assert_eq!(unsafe { libc::kill(winner as i32, libc::SIGTERM) }, 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    while group.0[survivor].try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "winner did not stop:\n{}",
            logs()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(!instance.exists(), "{}", logs());
    assert!(!lock.exists(), "{}", logs());
}
