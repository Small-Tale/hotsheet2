//! HS2-HWC0FS: Windows owner tests use real ACLs/handles/locks, injected secrets.
use super::super::super::{Metadata, Owner, Step};
use super::super::{
    FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, OPEN_EXISTING, READ_CONTROL, directory, identity, open,
};
use super::*;
use crate::file_lock::FileLock;
use crate::secrets::metadata::{Generation, StoredKeyMetadata, decode};
use crate::secrets::{KeyMetadata, KeyRegistry, SecretStore};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

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
struct Home {
    _parent: tempfile::TempDir,
    root: PathBuf,
}
impl Home {
    fn path(&self) -> &Path {
        &self.root
    }
}
fn home() -> Home {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("private-key-home");
    Owner::prepare(&root).unwrap();
    Home {
        _parent: parent,
        root,
    }
}
fn bytes(root: &Path) -> Vec<u8> {
    fs::read(root.join("keys.json")).unwrap()
}
fn map(root: &Path) -> Metadata {
    decode(&bytes(root)).unwrap()
}
fn metadata(account: &str) -> StoredKeyMetadata {
    StoredKeyMetadata {
        metadata: KeyMetadata {
            provider: account.into(),
            env: crate::secrets::env_name(account),
            site: None,
            identity: None,
        },
        credential_generation: Generation::default(),
    }
}
fn metadata_file(root: &Path) -> File {
    open(
        &root.join("keys.json"),
        &Principal::current().unwrap(),
        FILE_GENERIC_READ,
        OPEN_EXISTING,
        false,
        true,
    )
    .unwrap()
}

#[test]
fn hs2_hwc0fs_windows_private_creation_acl_and_stage_are_checked_before_first_byte() {
    let home = home();
    let owner = Owner::prepare(home.path()).unwrap();
    let principal = Principal::current().unwrap();
    for path in [&owner.root, &owner.accounts] {
        let held = directory(path, &principal, true, false).unwrap();
        principal.check(&held, true, false).unwrap();
    }
    let account = owner.account("synthetic").unwrap();
    let lock = owner.metadata_lock().unwrap();
    account.check().unwrap();
    lock.check().unwrap();
    let staged = owner.proof.stage(home.path()).unwrap();
    assert_eq!(staged.as_file().metadata().unwrap().len(), 0);
    principal.check(staged.as_file(), true, false).unwrap();
    identity(staged.as_file(), false).unwrap();
    // Another real open must resolve the same live object; IDs are not timestamps.
    let reopened = open(
        staged.path(),
        &principal,
        FILE_GENERIC_READ,
        OPEN_EXISTING,
        false,
        true,
    )
    .unwrap();
    assert!(identity(&reopened, false).unwrap() == identity(staged.as_file(), false).unwrap());
}

#[test]
fn hs2_hwc0fs_windows_null_and_world_write_acl_refuse_before_injected_native() {
    use windows_sys::Win32::Security::Authorization::SetSecurityInfo;
    use windows_sys::Win32::Security::{PROTECTED_DACL_SECURITY_INFORMATION, WinWorldSid};
    for null in [true, false] {
        let home = home();
        let principal = Principal::current().unwrap();
        // Modify ONLY this owned synthetic directory. No ACL repair is exercised.
        let file = open(
            home.path(),
            &principal,
            READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES,
            OPEN_EXISTING,
            true,
            false,
        )
        .unwrap();
        let world = Sid::known(WinWorldSid).unwrap();
        let length = mem::size_of::<ACL>()
            + 2 * (mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart) + SECURITY_MAX_SID_SIZE as usize);
        let mut acl = vec![0u32; length.div_ceil(mem::size_of::<u32>())];
        // SAFETY: aligned owned ACL buffers/SIDs stay live; this touches only the
        // explicit fixture handle and preserves current-user access for cleanup.
        unsafe {
            okay(InitializeAcl(
                acl.as_mut_ptr().cast(),
                length as u32,
                ACL_REVISION,
            ))
            .unwrap();
            okay(AddAccessAllowedAceEx(
                acl.as_mut_ptr().cast(),
                ACL_REVISION,
                OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
                FILE_ALL_ACCESS,
                principal.user.raw(),
            ))
            .unwrap();
            okay(AddAccessAllowedAceEx(
                acl.as_mut_ptr().cast(),
                ACL_REVISION,
                0,
                FILE_WRITE_DATA,
                world.raw(),
            ))
            .unwrap();
            let result = SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                if null {
                    ptr::null()
                } else {
                    acl.as_ptr().cast()
                },
                ptr::null(),
            );
            assert_eq!(result, 0);
        }
        assert!(matches!(
            principal.check(&file, false, false),
            Err(SecretError::Unavailable(_))
        ));
        let native = Memory::default();
        let keys = KeyRegistry::new(home.path(), native.clone());
        assert!(matches!(
            keys.set("synthetic", "value"),
            Err(SecretError::Unavailable(_))
        ));
        assert!(native.calls.lock().unwrap().is_empty());
        assert!(!home.path().join("keys.json").exists());
    }
}

#[test]
fn hs2_hwc0fs_windows_hardlink_directory_and_pinned_lock_replacement_refuse() {
    let home = home();
    let native = Memory::default();
    let keys = KeyRegistry::new(home.path(), native.clone());
    keys.set("kept", "synthetic").unwrap();
    let before = bytes(home.path());
    let alias = home.path().join("synthetic-alias");
    fs::hard_link(home.path().join("keys.json"), &alias)
        .expect("Windows HWC CI requires hard-link-capable fixture filesystem");
    assert!(matches!(
        keys.set("rejected", "value"),
        Err(SecretError::Unavailable(_))
    ));
    assert_eq!(native.calls.lock().unwrap().as_slice(), ["set"]);
    fs::remove_file(&alias).unwrap();
    let owner = Owner::prepare(home.path()).unwrap();
    let lock = owner.metadata_lock().unwrap();
    let error = fs::rename(
        home.path().join("keys.json.lock"),
        home.path().join("old-lock"),
    )
    .unwrap_err();
    assert_eq!(
        error.raw_os_error(),
        Some(32),
        "pinned lock must deny delete sharing"
    );
    lock.check().unwrap();
    drop(lock);
    drop(owner);
    fs::remove_file(home.path().join("keys.json")).unwrap();
    fs::create_dir(home.path().join("keys.json")).unwrap();
    assert!(keys.delete("kept").is_err());
    assert_eq!(native.calls.lock().unwrap().as_slice(), ["set"]);
    assert_eq!(
        fs::read(&alias).err().unwrap().kind(),
        io::ErrorKind::NotFound
    );
    fs::remove_dir(home.path().join("keys.json")).unwrap();
    // Restore only this explicit fixture's prior synthetic metadata, not a product recovery promise.
    fs::write(home.path().join("keys.json"), before).unwrap();
}

#[test]
fn hs2_hwc0fs_windows_noops_keep_handle_identity_and_set_delete_replace() {
    let home = home();
    let keys = KeyRegistry::new(home.path(), Memory::default());
    keys.set("synthetic", "first").unwrap();
    keys.record_site("synthetic", "https://example.test")
        .unwrap();
    keys.record_identity("synthetic", "synthetic-login")
        .unwrap();
    let held = metadata_file(home.path());
    let before = bytes(home.path());
    let first = identity(&held, false).unwrap();
    keys.record_site("synthetic", " https://example.test/ ")
        .unwrap();
    keys.record_identity("synthetic", " synthetic-login ")
        .unwrap();
    assert_eq!(bytes(home.path()), before);
    assert!(identity(&metadata_file(home.path()), false).unwrap() == first);
    // The no-op must preserve the held object. Release this observer before the
    // replacement: Windows may refuse a rename over an open destination.
    drop(held);
    keys.set("synthetic", "second").unwrap();
    assert!(identity(&metadata_file(home.path()), false).unwrap() != first);
    assert_eq!(keys.get("synthetic").unwrap(), "second");
    assert!(keys.delete("synthetic").unwrap());
    let empty = metadata_file(home.path());
    let previous = identity(&empty, false).unwrap();
    drop(empty);
    assert!(!keys.delete("synthetic").unwrap());
    assert!(identity(&metadata_file(home.path()), false).unwrap() != previous);
    assert!(map(home.path()).is_empty());
}

#[test]
fn hs2_hwc0fs_windows_publication_faults_leave_old_or_complete_new_and_retry() {
    for failed in [
        Step::BeforeStage,
        Step::Staged,
        Step::Written,
        Step::Synced,
        Step::BeforeRename,
        Step::Renamed,
        Step::Durable,
    ] {
        let home = home();
        let keys = KeyRegistry::new(home.path(), Memory::default());
        keys.set("existing", "synthetic").unwrap();
        let old = bytes(home.path());
        let owner = Owner::prepare(home.path()).unwrap();
        let account = owner.account("added").unwrap();
        let lock = owner.metadata_lock().unwrap();
        let (mut next, before) = owner.load().unwrap();
        next.insert("added".into(), metadata("added"));
        assert!(
            owner
                .publish_with(&account, &lock, before, &next, |step| {
                    if step == failed {
                        Err(io::Error::other("synthetic publication refusal"))
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        if matches!(failed, Step::Renamed | Step::Durable) {
            assert_eq!(map(home.path()).len(), 2);
        } else {
            assert_eq!(bytes(home.path()), old);
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
        drop(owner);
        keys.set("added", "recovery").unwrap();
        assert_eq!(map(home.path()).len(), 2);
    }
}
#[test]
fn hs2_hwc0fs_windows_native_partial_failure_never_blind_deletes_and_retry_recovers() {
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
    let home = home();
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
fn hs2_hwc0fs_windows_independent_account_native_work_does_not_hold_metadata_owner() {
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
    let home = home();
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
fn hs2_hwc0fs_windows_same_account_set_delete_cannot_invert_native_and_metadata_order() {
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
    let home = home();
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
            .args(["--ignored", "--exact", "secrets::metadata_owner::windows::security::tests::hs2_hwc0fs_windows_owned_metadata_child", "--nocapture"])
            // No inherited Git locators, native opt-ins, account tokens or home.
            .env_clear().env("SystemRoot", std::env::var_os("SystemRoot").expect("Windows system root required"))
            .env("USERPROFILE", root).env("TEMP", root).env("TMP", root)
            .env("HOME", root).env("CODEX_HOME", root)
            .env("HOTSHEET_HOME", root).env("XDG_CONFIG_HOME", root)
            .env("XDG_CACHE_HOME", root).env("TMPDIR", root)
            .env("HS2_HWC_CHILD_ROOT", root).env("HS2_HWC_CHILD_MODE", mode)
            .env("HS2_HWC_CHILD_ACCOUNT", account)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit())
            .spawn().unwrap();
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
fn hs2_hwc0fs_windows_owned_metadata_child() {
    let root = PathBuf::from(
        std::env::var_os("HS2_HWC_CHILD_ROOT").expect("explicit private root required"),
    );
    assert_eq!(
        PathBuf::from(std::env::var_os("HOTSHEET_HOME").unwrap()),
        root
    );
    let mode = std::env::var("HS2_HWC_CHILD_MODE").unwrap();
    let account = std::env::var("HS2_HWC_CHILD_ACCOUNT").unwrap();
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
fn hs2_hwc0fs_windows_separate_process_writers_merge_without_global_native_lock() {
    let home = home();
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
fn hs2_hwc0fs_windows_process_exit_releases_real_account_and_metadata_locks() {
    let home = home();
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
fn hs2_hwc0fs_windows_process_interruption_retains_old_or_new_map_and_retry_is_safe() {
    for (mode, expected) in [("crash-before", 1), ("crash-after", 2)] {
        let home = home();
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
