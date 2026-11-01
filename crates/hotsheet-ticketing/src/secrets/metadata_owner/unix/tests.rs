use super::*;
use crate::secrets::metadata::{Generation, StoredKeyMetadata};
use crate::secrets::{KeyMetadata, KeyRegistry, SecretStore};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
struct Memory {
    values: Arc<Mutex<BTreeMap<String, String>>>,
    calls: Arc<Mutex<Vec<&'static str>>>,
}
impl SecretStore for Memory {
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.calls.lock().unwrap().push("set");
        self.values
            .lock()
            .unwrap()
            .insert(account.into(), value.into());
        Ok(())
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        Ok(self.values.lock().unwrap().get(account).cloned())
    }
    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        self.calls.lock().unwrap().push("delete");
        Ok(self.values.lock().unwrap().remove(account).is_some())
    }
}

fn bytes(root: &Path) -> Vec<u8> {
    fs::read(root.join("keys.json")).unwrap()
}
fn image(root: &Path) -> Image {
    image_at(&root.join("keys.json")).unwrap().unwrap()
}
fn map(root: &Path) -> Metadata {
    decode(&bytes(root)).unwrap()
}
fn metadata(account: &str) -> StoredKeyMetadata {
    StoredKeyMetadata {
        metadata: KeyMetadata {
            provider: account.into(),
            env: super::super::super::env_name(account),
            site: None,
            identity: None,
        },
        credential_generation: Generation::default(),
    }
}
fn injected() -> io::Error {
    io::Error::other("synthetic publication refusal")
}

#[test]
fn hs2_hwc0fs_partial_metadata_error_keeps_removal_conversion_and_no_source_details() {
    let error = crate::connection_removal::ConnectionRemovalError::from(
        SecretError::MetadataCommitIncomplete,
    );
    assert!(matches!(
        error,
        crate::connection_removal::ConnectionRemovalError::Secret(
            SecretError::MetadataCommitIncomplete
        )
    ));
    assert_eq!(
        error.to_string(),
        SecretError::MetadataCommitIncomplete.to_string()
    );
    assert!(std::error::Error::source(&SecretError::MetadataCommitIncomplete).is_none());
}

#[test]
fn hs2_hwc0fs_native_success_always_publishes_but_true_backfill_noops_do_not() {
    let home = tempfile::tempdir().unwrap();
    let store = Memory::default();
    let keys = KeyRegistry::new(home.path(), store.clone());
    keys.set("synthetic-one", "first").unwrap();
    keys.record_site("synthetic-one", " https://example.test/ ")
        .unwrap();
    keys.record_identity("synthetic-one", " synthetic-login ")
        .unwrap();
    let before = bytes(home.path());
    let public_before = serde_json::to_value(keys.list().unwrap()).unwrap();
    // Pin the old inode, rather than assuming inode numbers cannot be reused.
    let old = File::open(home.path().join("keys.json")).unwrap();
    let old_identity = Identity::regular(&old.metadata().unwrap()).unwrap();
    keys.record_site("synthetic-one", "https://example.test")
        .unwrap();
    keys.record_identity("synthetic-one", "synthetic-login")
        .unwrap();
    keys.record_identity("synthetic-one", " ").unwrap();
    assert!(image(home.path()).identity == old_identity);
    assert_eq!(bytes(home.path()), before);
    keys.set("synthetic-one", "second").unwrap();
    assert!(image(home.path()).identity != old_identity);
    assert_ne!(
        bytes(home.path()),
        before,
        "successful replacement rotates the private generation"
    );
    assert_eq!(
        serde_json::to_value(keys.list().unwrap()).unwrap(),
        public_before
    );
    assert_eq!(keys.get("synthetic-one").unwrap(), "second");
    assert!(keys.delete("synthetic-one").unwrap());
    let empty = File::open(home.path().join("keys.json")).unwrap();
    let empty_identity = Identity::regular(&empty.metadata().unwrap()).unwrap();
    assert!(!keys.delete("synthetic-one").unwrap());
    assert!(image(home.path()).identity != empty_identity);
    assert!(map(home.path()).is_empty());
    assert_eq!(
        fs::metadata(home.path().join("keys.json")).unwrap().mode() & 0o777,
        0o600
    );
    assert!(
        !String::from_utf8(bytes(home.path()))
            .unwrap()
            .contains("second")
    );
}

#[test]
fn hs2_hwc0fs_malformed_metadata_refuses_all_mutators_before_native_work() {
    let home = tempfile::tempdir().unwrap();
    let store = Memory::default();
    let keys = KeyRegistry::new(home.path(), store.clone());
    let corrupt = b"{\"synthetic\":";
    fs::write(home.path().join("keys.json"), corrupt).unwrap();
    assert!(matches!(
        keys.set("synthetic", "value"),
        Err(SecretError::Parse { .. })
    ));
    assert!(matches!(
        keys.delete("synthetic"),
        Err(SecretError::Parse { .. })
    ));
    assert!(matches!(
        keys.record_site("synthetic", "https://example.test"),
        Err(SecretError::Parse { .. })
    ));
    assert!(matches!(
        keys.record_identity("synthetic", "login"),
        Err(SecretError::Parse { .. })
    ));
    assert!(store.calls.lock().unwrap().is_empty());
    assert_eq!(bytes(home.path()), corrupt);
}

#[test]
fn hs2_hwc0fs_symlink_directory_hardlink_and_replaced_lock_are_refused() {
    use std::os::unix::fs::symlink;
    let home = tempfile::tempdir().unwrap();
    let foreign = tempfile::tempdir().unwrap();
    fs::write(foreign.path().join("sentinel"), b"{}").unwrap();
    let store = Memory::default();
    let keys = KeyRegistry::new(home.path(), store.clone());
    symlink(
        foreign.path().join("sentinel"),
        home.path().join("keys.json"),
    )
    .unwrap();
    assert!(keys.set("synthetic", "value").is_err());
    fs::remove_file(home.path().join("keys.json")).unwrap();
    fs::remove_file(home.path().join("keys.json.lock")).unwrap();
    symlink(
        foreign.path().join("sentinel"),
        home.path().join("keys.json.lock"),
    )
    .unwrap();
    assert!(keys.set("synthetic", "value").is_err());
    fs::remove_file(home.path().join("keys.json.lock")).unwrap();
    fs::create_dir(home.path().join("keys.json")).unwrap();
    assert!(keys.delete("synthetic").is_err());
    fs::remove_dir(home.path().join("keys.json")).unwrap();
    fs::hard_link(
        foreign.path().join("sentinel"),
        home.path().join("keys.json"),
    )
    .unwrap();
    assert!(keys.set("synthetic", "value").is_err());
    fs::remove_file(home.path().join("keys.json")).unwrap();
    let owner = Owner::prepare(home.path()).unwrap();
    let held = owner.metadata_lock().unwrap();
    fs::rename(
        home.path().join("keys.json.lock"),
        home.path().join("old-lock"),
    )
    .unwrap();
    let _replacement = CheckedLock::acquire(home.path().join("keys.json.lock")).unwrap();
    assert!(held.check().is_err());
    drop(held);
    drop(_replacement);
    fs::set_permissions(
        home.path().join("key-metadata-locks"),
        fs::Permissions::from_mode(0o777),
    )
    .unwrap();
    assert!(keys.set("synthetic", "value").is_err());
    fs::set_permissions(
        home.path().join("key-metadata-locks"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    fs::set_permissions(
        home.path().join("keys.json.lock"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(keys.set("synthetic", "value").is_err());
    fs::set_permissions(
        home.path().join("keys.json.lock"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert!(store.calls.lock().unwrap().is_empty());
    assert_eq!(fs::read(foreign.path().join("sentinel")).unwrap(), b"{}");
}

#[test]
fn hs2_hwc0fs_home_alias_shares_account_owner_and_replacement_invalidates_guard() {
    use std::os::unix::fs::symlink;
    let outer = tempfile::tempdir().unwrap();
    let root = outer.path().join("home");
    let owner = Owner::prepare(&root).unwrap();
    let held = owner.account("synthetic").unwrap();
    symlink(&root, outer.path().join("alias")).unwrap();
    let alias = Owner::prepare(&outer.path().join("alias")).unwrap();
    let path = alias
        .accounts
        .join(format!("{:x}.lock", Sha256::digest(b"synthetic")));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    assert!(FileLock::try_acquire(file).unwrap().is_none());
    fs::rename(&root, outer.path().join("old-home")).unwrap();
    fs::create_dir(&root).unwrap();
    assert!(held.check().is_err());
}

#[test]
fn hs2_hwc0fs_publisher_faults_preserve_old_or_complete_new_document() {
    for failed in [
        Step::BeforeStage,
        Step::Staged,
        Step::Written,
        Step::Synced,
        Step::BeforeRename,
        Step::Renamed,
        Step::Durable,
    ] {
        let home = tempfile::tempdir().unwrap();
        let keys = KeyRegistry::new(home.path(), Memory::default());
        keys.set("existing", "synthetic").unwrap();
        let before_bytes = bytes(home.path());
        let owner = Owner::prepare(home.path()).unwrap();
        let account = owner.account("added").unwrap();
        let lock = owner.metadata_lock().unwrap();
        let (mut next, before) = owner.load().unwrap();
        next.insert("added".into(), metadata("added"));
        assert!(
            owner
                .publish_with(&account, &lock, before, &next, |step| {
                    if step == failed {
                        Err(injected())
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        if matches!(failed, Step::Renamed | Step::Durable) {
            assert_eq!(map(home.path()).len(), 2);
        } else {
            assert_eq!(bytes(home.path()), before_bytes);
        }
        assert!(fs::read_dir(home.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".keys-")
        }));
        drop(lock);
        drop(account);
        keys.set("added", "recovery").unwrap();
        assert_eq!(keys.list().unwrap().len(), 2);
    }
}

#[test]
fn hs2_hwc0fs_native_partial_failure_never_blind_deletes_and_retry_recovers() {
    struct Corrupting {
        memory: Memory,
        root: PathBuf,
        corrupt: std::cell::Cell<bool>,
    }
    impl SecretStore for Corrupting {
        fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
            self.memory.set(account, value)?;
            if self.corrupt.replace(false) {
                fs::write(self.root.join("keys.json"), b"{")?;
            }
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
            self.memory.get(account)
        }
        fn delete(&self, account: &str) -> Result<bool, SecretError> {
            let result = self.memory.delete(account)?;
            if self.corrupt.replace(false) {
                fs::write(self.root.join("keys.json"), b"{")?;
            }
            Ok(result)
        }
    }
    let home = tempfile::tempdir().unwrap();
    let store = Corrupting {
        memory: Memory::default(),
        root: home.path().into(),
        corrupt: false.into(),
    };
    let keys = KeyRegistry::new(home.path(), store);
    keys.set("unrelated", "kept").unwrap();
    let before = bytes(home.path());
    keys.store.corrupt.set(true);
    let error = keys.set("synthetic", "replacement").unwrap_err();
    assert!(matches!(error, SecretError::MetadataCommitIncomplete));
    assert_eq!(
        keys.store.memory.calls.lock().unwrap().as_slice(),
        ["set", "set"]
    );
    assert_eq!(keys.get("synthetic").unwrap(), "replacement");
    // Deliberate raw corruption belongs to this synthetic out-of-band fault only.
    fs::write(home.path().join("keys.json"), &before).unwrap();
    keys.set("synthetic", "replacement").unwrap();
    let before_delete = bytes(home.path());
    keys.store.corrupt.set(true);
    assert!(matches!(
        keys.delete("synthetic"),
        Err(SecretError::MetadataCommitIncomplete)
    ));
    assert_eq!(keys.store.memory.get("synthetic").unwrap(), None);
    fs::write(home.path().join("keys.json"), before_delete).unwrap();
    assert!(keys.delete("synthetic").unwrap());
    assert_eq!(
        keys.list()
            .unwrap()
            .into_iter()
            .map(|row| row.provider)
            .collect::<Vec<_>>(),
        ["unrelated"]
    );
    assert_eq!(
        error.to_string(),
        "Credential operation may have completed, but metadata could not be confirmed; repeat the same operation"
    );
    assert_eq!(format!("{error:?}"), "MetadataCommitIncomplete");
}

struct Release(Option<mpsc::Sender<()>>);
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}

#[test]
fn hs2_hwc0fs_independent_account_native_work_does_not_hold_metadata_owner() {
    struct Paused {
        memory: Memory,
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl SecretStore for Paused {
        fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
            self.memory.set(account, value)?;
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
            self.memory.get(account)
        }
        fn delete(&self, account: &str) -> Result<bool, SecretError> {
            self.memory.delete(account)
        }
    }
    let home = tempfile::tempdir().unwrap();
    let memory = Memory::default();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let paused = Paused {
        memory: memory.clone(),
        entered: entered_tx,
        release: Mutex::new(release_rx),
    };
    std::thread::scope(|scope| {
        let first = scope.spawn(|| KeyRegistry::new(home.path(), paused).set("first", "one"));
        let release = Release(Some(release_tx));
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let root = home.path();
        let second_memory = memory.clone();
        let second = scope.spawn(move || {
            let keys = KeyRegistry::new(root, second_memory);
            keys.set("second", "two").unwrap();
            keys.record_site("second", "https://example.test").unwrap();
            done_tx.send(()).unwrap();
        });
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("unrelated account cannot wait for first native operation");
        drop(release);
        first.join().unwrap().unwrap();
        second.join().unwrap();
    });
    let keys = KeyRegistry::new(home.path(), memory);
    assert_eq!(keys.list().unwrap().len(), 2);
    assert_eq!(
        map(home.path())["second"].metadata.site.as_deref(),
        Some("https://example.test")
    );
}

#[test]
fn hs2_hwc0fs_same_account_set_delete_cannot_invert_native_and_metadata_order() {
    struct Paused {
        memory: Memory,
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl SecretStore for Paused {
        fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
            self.memory.set(account, value)?;
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
            self.memory.get(account)
        }
        fn delete(&self, account: &str) -> Result<bool, SecretError> {
            self.memory.delete(account)
        }
    }
    let home = tempfile::tempdir().unwrap();
    let memory = Memory::default();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let paused = Paused {
        memory: memory.clone(),
        entered: entered_tx,
        release: Mutex::new(release_rx),
    };
    std::thread::scope(|scope| {
        let first = scope.spawn(|| KeyRegistry::new(home.path(), paused).set("synthetic", "one"));
        let release = Release(Some(release_tx));
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let owner = Owner::prepare(home.path()).unwrap();
        let account_path = owner
            .accounts
            .join(format!("{:x}.lock", Sha256::digest(b"synthetic")));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(account_path)
            .unwrap();
        assert!(
            FileLock::try_acquire(file).unwrap().is_none(),
            "native set must retain its account owner"
        );
        let second =
            scope.spawn(|| KeyRegistry::new(home.path(), memory.clone()).delete("synthetic"));
        drop(release);
        first.join().unwrap().unwrap();
        assert!(second.join().unwrap().unwrap());
    });
    assert_eq!(memory.calls.lock().unwrap().as_slice(), ["set", "delete"]);
    assert_eq!(memory.get("synthetic").unwrap(), None);
    assert!(map(home.path()).is_empty());
}

#[test]
fn hs2_hwc0fs_concurrent_site_identity_merge_and_postdelete_backfill_refuses() {
    let home = tempfile::tempdir().unwrap();
    let memory = Memory::default();
    KeyRegistry::new(home.path(), memory.clone())
        .set("synthetic", "value")
        .unwrap();
    std::thread::scope(|scope| {
        let site = scope.spawn(|| {
            KeyRegistry::new(home.path(), memory.clone())
                .record_site("synthetic", "https://example.test")
        });
        let identity = scope.spawn(|| {
            KeyRegistry::new(home.path(), memory.clone())
                .record_identity("synthetic", "synthetic-login")
        });
        site.join().unwrap().unwrap();
        identity.join().unwrap().unwrap();
    });
    let entry = map(home.path()).remove("synthetic").unwrap();
    assert_eq!(entry.metadata.site.as_deref(), Some("https://example.test"));
    assert_eq!(entry.metadata.identity.as_deref(), Some("synthetic-login"));
    let keys = KeyRegistry::new(home.path(), memory);
    assert!(keys.delete("synthetic").unwrap());
    assert!(matches!(
        keys.record_site("synthetic", "https://example.test"),
        Err(SecretError::NotFound(_))
    ));
    assert!(matches!(
        keys.record_identity("synthetic", "login"),
        Err(SecretError::NotFound(_))
    ));
    assert!(map(home.path()).is_empty());
}

#[test]
fn hs2_hwc0fs_native_error_does_not_publish_or_compensate() {
    struct Refusing(Memory);
    impl SecretStore for Refusing {
        fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
            Err(SecretError::Backend("synthetic refusal".into()))
        }
        fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
            self.0.get(account)
        }
        fn delete(&self, _: &str) -> Result<bool, SecretError> {
            Err(SecretError::Backend("synthetic refusal".into()))
        }
    }
    let home = tempfile::tempdir().unwrap();
    let memory = Memory::default();
    KeyRegistry::new(home.path(), memory.clone())
        .set("synthetic", "kept")
        .unwrap();
    let before = bytes(home.path());
    let pinned = File::open(home.path().join("keys.json")).unwrap();
    let before_identity = Identity::regular(&pinned.metadata().unwrap()).unwrap();
    let keys = KeyRegistry::new(home.path(), Refusing(memory.clone()));
    assert!(matches!(
        keys.set("synthetic", "new"),
        Err(SecretError::Backend(_))
    ));
    assert!(matches!(
        keys.delete("synthetic"),
        Err(SecretError::Backend(_))
    ));
    assert_eq!(bytes(home.path()), before);
    assert!(image(home.path()).identity == before_identity);
    assert_eq!(memory.calls.lock().unwrap().as_slice(), ["set"]);
}

// A synthetic disk port only: no OsKeychain, native vault, account or network API.
struct DiskStore {
    root: PathBuf,
    pause: bool,
}
impl DiskStore {
    fn path(&self, account: &str) -> PathBuf {
        self.root.join(format!(
            "synthetic-{:x}",
            Sha256::digest(account.as_bytes())
        ))
    }
}
impl SecretStore for DiskStore {
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        fs::write(self.path(account), value)?;
        if self.pause {
            println!("HWC_NATIVE_READY");
            io::stdout().flush()?;
            let mut release = [0];
            io::stdin().read_exact(&mut release)?;
        }
        Ok(())
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        match fs::read_to_string(self.path(account)) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        match fs::remove_file(self.path(account)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
}

struct ChildOwner {
    child: std::process::Child,
    events: mpsc::Receiver<String>,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl ChildOwner {
    fn start(root: &Path, mode: &str, account: &str) -> Self {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "secrets::metadata_owner::unix::tests::hs2_hwc0fs_owned_metadata_child",
                "--nocapture",
            ])
            // No inherited Git locators, native opt-ins, account tokens or home.
            .env_clear()
            .env("HOME", root)
            .env("CODEX_HOME", root)
            .env("HOTSHEET_HOME", root)
            .env("XDG_CONFIG_HOME", root)
            .env("XDG_CACHE_HOME", root)
            .env("TMPDIR", root)
            .env("HS2_HWC_CHILD_ROOT", root)
            .env("HS2_HWC_CHILD_MODE", mode)
            .env("HS2_HWC_CHILD_ACCOUNT", account)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, events) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in io::BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        let _ = tx.send(line);
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            child,
            events,
            reader: Some(reader),
        }
    }
    fn ready(&self, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = self
                .events
                .recv_timeout(remaining)
                .expect("owned metadata child did not reach its boundary");
            if line.ends_with(expected) {
                return;
            }
        }
    }
    fn release(&mut self) {
        self.child.stdin.take().unwrap().write_all(b"1").unwrap();
    }
    fn finish(&mut self, code: i32) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.reader.take().unwrap().join().unwrap();
                assert_eq!(status.code(), Some(code));
                return;
            }
            assert!(
                Instant::now() < deadline,
                "owned metadata child exceeded its completion budget"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        // Only the exact child we spawned. Native spawn/kill/wait I/O is not an
        // end-to-end real-time guarantee; the cooperative protocol is bounded.
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[test]
#[ignore = "HS2-HWC0FS: launched only by the owned synthetic metadata parent"]
fn hs2_hwc0fs_owned_metadata_child() {
    let root = PathBuf::from(
        std::env::var_os("HS2_HWC_CHILD_ROOT").expect("explicit private root required"),
    );
    assert_eq!(
        PathBuf::from(std::env::var_os("HOTSHEET_HOME").unwrap()),
        root
    );
    let mode = std::env::var("HS2_HWC_CHILD_MODE").unwrap();
    let account = std::env::var("HS2_HWC_CHILD_ACCOUNT").unwrap();
    if mode == "fifo" || mode == "fifo-lock" {
        use std::os::unix::ffi::OsStrExt;
        let name = if mode == "fifo" {
            "keys.json"
        } else {
            "keys.json.lock"
        };
        let path = std::ffi::CString::new(root.join(name).as_os_str().as_bytes()).unwrap();
        // SAFETY: the NUL-terminated path is owned by this child fixture.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let memory = Memory::default();
        let keys = KeyRegistry::new(&root, memory.clone());
        assert!(keys.set(&account, "synthetic").is_err());
        assert!(memory.calls.lock().unwrap().is_empty());
        return;
    }
    if mode == "hold" {
        let owner = Owner::prepare(&root).unwrap();
        let _account = owner.account(&account).unwrap();
        let _metadata = owner.metadata_lock().unwrap();
        println!("HWC_OWNER_READY");
        io::stdout().flush().unwrap();
        let mut release = [0];
        io::stdin().read_exact(&mut release).unwrap();
        // Deliberately bypass destructors: the OS must release both real locks.
        std::process::exit(83);
    }
    if mode == "crash-before" || mode == "crash-after" {
        let owner = Owner::prepare(&root).unwrap();
        let account = owner.account(&account).unwrap();
        let lock = owner.metadata_lock().unwrap();
        let (mut next, before) = owner.load().unwrap();
        next.insert("crash-added".into(), metadata("crash-added"));
        let at = if mode == "crash-before" {
            Step::BeforeRename
        } else {
            Step::Renamed
        };
        owner
            .publish_with(&account, &lock, before, &next, |step| {
                if step == at {
                    std::process::exit(83);
                }
                Ok(())
            })
            .unwrap();
        panic!("owned publication crash boundary was not reached");
    }
    assert!(mode == "pause" || mode == "fill");
    let keys = KeyRegistry::new(
        &root,
        DiskStore {
            root: root.clone(),
            pause: mode == "pause",
        },
    );
    keys.set(&account, "synthetic-payload").unwrap();
    keys.record_site(&account, "https://example.test").unwrap();
    keys.record_identity(&account, "synthetic-login").unwrap();
}

#[test]
fn hs2_hwc0fs_separate_process_writers_merge_without_global_native_lock() {
    let home = tempfile::tempdir().unwrap();
    let mut first = ChildOwner::start(home.path(), "pause", "first");
    first.ready("HWC_NATIVE_READY");
    let mut second = ChildOwner::start(home.path(), "fill", "second");
    second.finish(0);
    first.release();
    first.finish(0);
    let entries = map(home.path());
    assert_eq!(entries.len(), 2);
    for id in ["first", "second"] {
        assert_eq!(
            entries[id].metadata.site.as_deref(),
            Some("https://example.test")
        );
        assert_eq!(
            entries[id].metadata.identity.as_deref(),
            Some("synthetic-login")
        );
    }
}

#[test]
fn hs2_hwc0fs_process_exit_releases_real_account_and_metadata_locks() {
    let home = tempfile::tempdir().unwrap();
    let mut child = ChildOwner::start(home.path(), "hold", "synthetic");
    child.ready("HWC_OWNER_READY");
    let owner = Owner::prepare(home.path()).unwrap();
    for path in [
        owner.root.join("keys.json.lock"),
        owner
            .accounts
            .join(format!("{:x}.lock", Sha256::digest(b"synthetic"))),
    ] {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        assert!(FileLock::try_acquire(file).unwrap().is_none());
    }
    child.release();
    child.finish(83);
    KeyRegistry::new(home.path(), Memory::default())
        .set("synthetic", "recovered")
        .unwrap();
    assert_eq!(map(home.path()).len(), 1);
}

#[test]
fn hs2_hwc0fs_process_interruption_retains_old_or_new_map_and_retry_is_safe() {
    for (mode, expected) in [("crash-before", 1), ("crash-after", 2)] {
        let home = tempfile::tempdir().unwrap();
        let keys = KeyRegistry::new(home.path(), Memory::default());
        keys.set("existing", "synthetic").unwrap();
        let before = bytes(home.path());
        let mut child = ChildOwner::start(home.path(), mode, "crash-added");
        child.finish(83);
        assert_eq!(map(home.path()).len(), expected);
        if expected == 1 {
            assert_eq!(bytes(home.path()), before);
        }
        // Orphaned private staging files are not authority and are never reused.
        keys.set("crash-added", "recovery").unwrap();
        assert_eq!(map(home.path()).len(), 2);
        assert_eq!(map(home.path())["existing"].metadata.provider, "existing");
    }
}

#[test]
fn hs2_hwc0fs_fifo_replacement_refuses_promptly_in_owned_child() {
    for mode in ["fifo", "fifo-lock"] {
        let home = tempfile::tempdir().unwrap();
        let mut child = ChildOwner::start(home.path(), mode, "synthetic");
        child.finish(0);
    }
}
