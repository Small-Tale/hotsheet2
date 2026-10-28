//! Short, private detached-broker socket paths and exclusive filesystem ownership.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tokio::net::UnixListener;

/// Covers connection establishment itself; stream read/write deadlines are separate.
pub const PROBE_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

/// Connect without letting a full Unix accept backlog block broker startup or cleanup.
/// The caller must treat timeout and other uncertain errors as evidence that the socket
/// may still be owned; refusal or disappearance is stale only with no held owner lock
/// (HS2-YDD60V).
pub fn connect_probe(path: &Path) -> io::Result<UnixStream> {
    std::os::unix::net::SocketAddr::from_pathname(path)?;
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: socket returns a fresh descriptor or -1, which is checked below.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd is a new owned socket descriptor and is closed when stream drops.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    // SAFETY: stream owns fd; close-on-exec prevents a concurrent child launch from
    // inheriting this temporary probe socket.
    if unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    stream.set_nonblocking(true)?;

    // SAFETY: a zeroed sockaddr_un is valid; validate the path before copying it.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() >= address.sun_path.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "broker socket path exceeds the Unix address limit",
        ));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (target, byte) in address.sun_path.iter_mut().zip(bytes.iter().copied()) {
        *target = byte as libc::c_char;
    }
    let length =
        (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1) as libc::socklen_t;
    #[cfg(target_os = "macos")]
    {
        address.sun_len = length as u8;
    }
    // SAFETY: address and length describe the initialized sockaddr_un above.
    let connected = unsafe {
        libc::connect(
            stream.as_raw_fd(),
            (&raw const address).cast::<libc::sockaddr>(),
            length,
        )
    };
    if connected < 0 {
        let error = io::Error::last_os_error();
        if !matches!(error.raw_os_error(), Some(libc::EINPROGRESS | libc::EAGAIN)) {
            return Err(error);
        }
        let deadline = Instant::now() + PROBE_CONNECT_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "broker socket connect timed out",
                ));
            }
            let mut event = libc::pollfd {
                fd: stream.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            let wait_ms = remaining.as_millis().max(1).min(i32::MAX as u128) as i32;
            // SAFETY: event points to one initialized pollfd for this owned descriptor.
            match unsafe { libc::poll(&raw mut event, 1, wait_ms) } {
                0 => continue,
                -1 if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => continue,
                -1 => return Err(io::Error::last_os_error()),
                _ => {
                    let mut socket_error: libc::c_int = 0;
                    let mut error_length = std::mem::size_of_val(&socket_error) as libc::socklen_t;
                    // SAFETY: socket_error and error_length are valid output locations.
                    if unsafe {
                        libc::getsockopt(
                            stream.as_raw_fd(),
                            libc::SOL_SOCKET,
                            libc::SO_ERROR,
                            (&raw mut socket_error).cast(),
                            &raw mut error_length,
                        )
                    } != 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                    if socket_error != 0 {
                        return Err(io::Error::from_raw_os_error(socket_error));
                    }
                    break;
                }
            }
        }
    }
    // Linux may report POLLOUT and SO_ERROR=0 after EAGAIN on a full Unix backlog
    // even though no connection exists. Confirm the peer before returning a stream
    // that callers could mistake for a live broker (HS2-YDD60V).
    stream.peer_addr()?;
    stream.set_nonblocking(false)?;
    Ok(stream)
}

/// A broker holds this exclusive lock for its entire listener lifetime. A full
/// macOS Unix backlog can report ConnectionRefused, so refusal alone does not
/// prove a socket with a live owner is stale (HS2-YDD60V).
pub fn owner_lock_held(socket: &Path) -> io::Result<bool> {
    let lock_path = socket.with_extension("lock");
    match fs::symlink_metadata(&lock_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    let lock = open_lock(socket)?;
    // SAFETY: lock owns a valid descriptor until this function returns.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        Ok(false)
    } else {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::WouldBlock {
            Ok(true)
        } else {
            Err(error)
        }
    }
}

fn user_id() -> u32 {
    // SAFETY: geteuid takes no pointers and has no preconditions.
    unsafe { libc::geteuid() }
}

/// A deterministic socket namespace isolated by both effective user and configured home.
/// `/tmp` is intentional: macOS's normal per-user temporary directory can itself exceed
/// the Unix socket limit. Only socket/lock files live here; durable state stays in `home`.
pub fn socket_path(home: &Path, project: &str) -> io::Result<PathBuf> {
    let directory = PathBuf::from(format!("/tmp/hotsheet2-brokers-{}", user_id()));
    ensure_private_directory(&directory)?;
    let home = home
        .canonicalize()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(home));
    let mut digest = Sha256::new();
    digest.update(home.as_os_str().as_bytes());
    digest.update([0]);
    digest.update(project.as_bytes());
    let key = format!("{:x}", digest.finalize());
    Ok(directory.join(format!("{}.sock", &key[..32])))
}

/// Serialize adoption across server processes before inspecting the legacy socket.
/// Its broker may exit during inspection; the persistent marker must be published
/// before another server is allowed to decide that the legacy namespace is absent.
pub fn select_socket(home: &Path, project: &str) -> io::Result<PathBuf> {
    let canonical = socket_path(home, project)?;
    let selection = open_lock(&canonical.with_extension("namespace.sock"))?;
    // SAFETY: selection owns a valid descriptor, retained until selection completes.
    if unsafe { libc::flock(selection.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error());
    }
    match fs::symlink_metadata(canonical.with_extension("lock")) {
        Ok(_) => {
            let _marker = open_lock(&canonical)?;
            return Ok(canonical);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let legacy = home.join("broker").join(format!("{project}.sock"));
    if adopt_legacy_namespace(&legacy)? {
        Ok(legacy)
    } else {
        // Pin the new namespace before returning coordinates too, so a later legacy
        // file cannot redirect fresh handles away from a retained canonical handle.
        let _marker = open_lock(&canonical)?;
        Ok(canonical)
    }
}

fn ensure_private_directory(path: &Path) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.uid() != user_id() || metadata.mode() & 0o777 != 0o700 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "broker socket directory {} must be a real directory owned by the current user with mode 0700; remove or repair it before retrying",
                path.display()
            ),
        ));
    }
    Ok(())
}

/// Old home-based sockets may still own live terminals after an upgrade. Connect only
/// to an owned socket in an owned directory that other users cannot modify.
fn owned_legacy_directory(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(directory) = fs::symlink_metadata(parent) else {
        return false;
    };
    directory.is_dir() && directory.uid() == user_id() && directory.mode() & 0o022 == 0
}

/// Pin an existing legacy namespace with a persistent ownership-lock inode. The marker
/// outlives socket cleanup, so fresh and retained server handles choose the same path
/// after idle exit. Protocol responsiveness must never decide namespace ownership.
fn adopt_legacy_namespace(socket: &Path) -> io::Result<bool> {
    if !owned_legacy_directory(socket) {
        return Ok(false);
    }
    let owned_lock = fs::symlink_metadata(socket.with_extension("lock"))
        .is_ok_and(|metadata| valid_lock(&metadata));
    if socket_identity(socket).is_none() && !owned_lock {
        return Ok(false);
    }
    std::os::unix::net::SocketAddr::from_pathname(socket)?;
    fs::set_permissions(socket.parent().unwrap(), fs::Permissions::from_mode(0o700))?;
    // Opening does not take or disturb the serving process's exclusive lock.
    let _marker = open_lock(socket)?;
    Ok(true)
}

fn socket_identity(path: &Path) -> Option<(u64, u64)> {
    let metadata = fs::symlink_metadata(path).ok()?;
    (metadata.file_type().is_socket() && metadata.uid() == user_id())
        .then_some((metadata.dev(), metadata.ino()))
}

/// Delete only the owned socket inode captured at bind time. A late old process must
/// never erase a replacement broker's socket or an unrelated file/symlink.
pub struct SocketCleanup {
    path: PathBuf,
    identity: Option<(u64, u64)>,
}

impl SocketCleanup {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let identity = socket_identity(&path);
        Self { path, identity }
    }
}

impl Drop for SocketCleanup {
    fn drop(&mut self) {
        if self.identity.is_some() && socket_identity(&self.path) == self.identity {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(crate) struct SocketOwnership {
    // Drop cleanup before unlocking, so the next owner cannot race old cleanup.
    _cleanup: SocketCleanup,
    _lock: File,
}

/// Validate the platform address length and prepare its private parent before spawning.
pub fn prepare_parent(socket: &Path) -> io::Result<()> {
    std::os::unix::net::SocketAddr::from_pathname(socket).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "invalid broker socket path {}: {error}; use a shorter socket path",
                socket.display()
            ),
        )
    })?;
    let parent = socket.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "broker socket needs a parent directory",
        )
    })?;
    ensure_private_directory(parent)
}

pub(crate) fn bind(socket: &Path) -> io::Result<(UnixListener, SocketOwnership)> {
    prepare_parent(socket)?;
    let lock = open_lock(socket)?;
    // SAFETY: lock owns a valid descriptor for the lifetime of the returned guard.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::last_os_error());
    }
    match fs::symlink_metadata(socket) {
        Ok(_) => {
            if socket_identity(socket).is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "refusing to remove an unowned or non-socket broker path",
                ));
            }
            match probe_existing_socket(|| connect_probe(socket).map(|_| ())) {
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "a broker already accepts on this socket",
                    ));
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) =>
                {
                    fs::remove_file(socket)?
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(socket)?;
    let cleanup = SocketCleanup::new(socket);
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
    Ok((
        listener,
        SocketOwnership {
            _cleanup: cleanup,
            _lock: lock,
        },
    ))
}

/// An overloaded Unix socket can briefly report WouldBlock after its listener
/// closes. Keep the inode intact while the probe is uncertain; only a later
/// connection refusal or disappearance proves that stale-socket cleanup is safe.
fn probe_existing_socket(mut probe: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    const MAX_ATTEMPTS: usize = 10;
    for attempt in 0..MAX_ATTEMPTS {
        match probe() {
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock && attempt + 1 < MAX_ATTEMPTS =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            result => return result,
        }
    }
    unreachable!("the final probe attempt returns its result")
}

fn valid_lock(metadata: &fs::Metadata) -> bool {
    metadata.is_file()
        && metadata.uid() == user_id()
        && metadata.mode() & 0o777 == 0o600
        && metadata.nlink() == 1
}

fn open_lock(socket: &Path) -> io::Result<File> {
    // A permanent lock inode avoids unlink/recreate lock races between processes.
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(socket.with_extension("lock"))?;
    let metadata = lock.metadata()?;
    if !valid_lock(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "broker lock must be an owned regular file with mode 0600",
        ));
    }
    Ok(lock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::symlink;

    #[test]
    fn socket_probe_retries_only_transient_would_block() {
        let mut attempts = 0;
        let result = probe_existing_socket(|| {
            attempts += 1;
            if attempts < 3 {
                Err(io::Error::from(io::ErrorKind::WouldBlock))
            } else {
                Err(io::Error::from(io::ErrorKind::ConnectionRefused))
            }
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::ConnectionRefused);
        assert_eq!(attempts, 3);

        let mut attempts = 0;
        let result = probe_existing_socket(|| {
            attempts += 1;
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::WouldBlock);
        assert_eq!(attempts, 10);

        let mut attempts = 0;
        let result = probe_existing_socket(|| {
            attempts += 1;
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(attempts, 1);
    }

    #[tokio::test]
    async fn hs2_ydd60v_full_backlog_probe_is_bounded_and_stale_bind_recovers() {
        let root = fixture();
        let socket = root.path().join("backlog.sock");
        let (listener, owner) = bind(&socket).unwrap();
        // SAFETY: listener owns a listening descriptor; reducing its accept backlog is valid.
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 1) }, 0);
        let mut queued = Vec::new();
        let started = Instant::now();
        let full_error = loop {
            match connect_probe(&socket) {
                Ok(stream) if queued.len() < 16 => queued.push(stream),
                Ok(_) => panic!("the reduced Unix accept backlog did not fill"),
                Err(error) => break error,
            }
        };
        assert!(
            matches!(
                full_error.kind(),
                io::ErrorKind::TimedOut
                    | io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::NotConnected
            ),
            "{full_error}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        let identity = socket_identity(&socket);
        assert!(owner_lock_held(&socket).unwrap());
        let started = Instant::now();
        assert_eq!(
            bind(&socket).err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            socket_identity(&socket),
            identity,
            "uncertain live socket removed"
        );

        drop(queued);
        drop(listener);
        drop(owner);
        assert!(!socket.exists());
        // Simulate an exited broker that left its socket inode behind.
        drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
        assert!(socket.exists());
        let (replacement, owner) = bind(&socket).unwrap();
        // Inode numbers can be reused after unlink on Linux. A connection accepted by the
        // new listener proves that stale recovery installed a usable replacement.
        let connected = connect_probe(&socket).unwrap();
        let (accepted, _) = tokio::time::timeout(Duration::from_secs(1), replacement.accept())
            .await
            .expect("replacement listener did not accept")
            .unwrap();
        assert!(owner_lock_held(&socket).unwrap());
        drop(connected);
        drop(accepted);
        drop(replacement);
        drop(owner);
    }

    fn fixture() -> tempfile::TempDir {
        let directory = tempfile::Builder::new()
            .prefix("hs-sock-")
            .tempdir_in("/tmp")
            .unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        directory
    }

    #[test]
    fn long_homes_have_short_stable_and_isolated_socket_names() {
        let home = fixture();
        let long = home.path().join("long-home-directory".repeat(12));
        fs::create_dir(&long).unwrap();
        let path = socket_path(&long, "project").unwrap();
        assert!(path.as_os_str().as_bytes().len() < 104, "{path:?}");
        assert_eq!(path, socket_path(&long, "project").unwrap());
        assert_ne!(path, socket_path(home.path(), "project").unwrap());
        assert_ne!(path, socket_path(&long, "other-project").unwrap());
        let alias = home.path().join("alias");
        symlink(&long, &alias).unwrap();
        assert_eq!(path, socket_path(&alias, "project").unwrap());
        assert!(!long.join("broker").exists());
        assert_eq!(
            fs::metadata(path.parent().unwrap()).unwrap().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn namespace_selection_waits_for_adoption_and_retains_its_marker_after_socket_exit() {
        let home = fixture();
        let directory = home.path().join("broker");
        fs::create_dir(&directory).unwrap();
        let legacy = directory.join("project.sock");
        let canonical = socket_path(home.path(), "project").unwrap();
        let selecting = open_lock(&canonical.with_extension("namespace.sock")).unwrap();
        // SAFETY: selecting owns the descriptor until the first adoption completes.
        assert_eq!(
            unsafe { libc::flock(selecting.as_raw_fd(), libc::LOCK_EX) },
            0
        );
        let (sent, received) = std::sync::mpsc::channel();
        let other_home = home.path().to_path_buf();
        let other = std::thread::spawn(move || {
            sent.send(select_socket(&other_home, "project")).unwrap();
        });
        assert!(
            received
                .recv_timeout(std::time::Duration::from_millis(50))
                .is_err()
        );
        let listener = std::os::unix::net::UnixListener::bind(&legacy).unwrap();
        assert!(adopt_legacy_namespace(&legacy).unwrap());
        drop(listener);
        fs::remove_file(&legacy).unwrap();
        drop(selecting);
        assert_eq!(
            received
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
                .unwrap(),
            legacy
        );
        other.join().unwrap();
        fs::remove_file(canonical.with_extension("namespace.lock")).unwrap();
    }

    #[test]
    fn a_selected_short_namespace_is_not_redirected_by_a_later_legacy_socket() {
        let home = fixture();
        let canonical = select_socket(home.path(), "project").unwrap();
        let directory = home.path().join("broker");
        fs::create_dir(&directory).unwrap();
        let legacy = directory.join("project.sock");
        let _listener = std::os::unix::net::UnixListener::bind(legacy).unwrap();
        assert_eq!(select_socket(home.path(), "project").unwrap(), canonical);
        fs::remove_file(canonical.with_extension("namespace.lock")).unwrap();
        fs::remove_file(canonical.with_extension("lock")).unwrap();
    }

    #[test]
    fn private_directory_rejects_symlinks_and_shared_permissions_without_mutating_them() {
        let root = fixture();
        let shared = root.path().join("shared");
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            ensure_private_directory(&shared).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(fs::metadata(&shared).unwrap().mode() & 0o777, 0o755);
        let linked = root.path().join("linked");
        symlink(root.path(), &linked).unwrap();
        assert_eq!(
            ensure_private_directory(&linked).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        let new = root.path().join("private");
        ensure_private_directory(&new).unwrap();
        ensure_private_directory(&new).unwrap();
        assert_eq!(fs::metadata(new).unwrap().mode() & 0o777, 0o700);
    }

    #[tokio::test]
    async fn binding_is_exclusive_then_reclaims_stale_socket_after_release() {
        let root = fixture();
        let socket = root.path().join("broker.sock");
        let (listener, owner) = bind(&socket).unwrap();
        assert_eq!(fs::metadata(&socket).unwrap().mode() & 0o777, 0o600);
        let identity = socket_identity(&socket);
        assert!(bind(&socket).is_err());
        assert_eq!(socket_identity(&socket), identity);
        drop(listener);
        assert!(
            bind(&socket).is_err(),
            "lock remains held until cleanup finishes"
        );
        drop(owner);
        assert!(!socket.exists());
        assert!(
            socket.with_extension("lock").exists(),
            "the lock inode remains stable"
        );
        // A crashed old broker leaves a socket but no held lock.
        drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
        let (listener, owner) = bind(&socket).unwrap();
        assert!(socket.exists());
        drop(listener);
        drop(owner);
        assert!(!socket.exists());
    }

    #[tokio::test]
    async fn binding_preserves_live_legacy_sockets_regular_files_and_symlinks() {
        let root = fixture();
        let socket = root.path().join("broker.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert_eq!(
            bind(&socket).err().unwrap().kind(),
            io::ErrorKind::AddrInUse
        );
        assert!(socket.exists());
        drop(listener);
        fs::remove_file(&socket).unwrap();
        fs::write(&socket, "retain this file").unwrap();
        assert_eq!(
            bind(&socket).err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(fs::read_to_string(&socket).unwrap(), "retain this file");
        fs::remove_file(&socket).unwrap();
        let other = root.path().join("other");
        fs::write(&other, "target").unwrap();
        symlink(&other, &socket).unwrap();
        assert!(bind(&socket).is_err());
        assert_eq!(fs::read_to_string(&other).unwrap(), "target");
        assert!(
            fs::symlink_metadata(&socket)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[tokio::test]
    async fn binding_rejects_lock_symlinks_and_hardlinks_without_changing_targets() {
        let root = fixture();
        let socket = root.path().join("broker.sock");
        let lock = socket.with_extension("lock");
        let target = root.path().join("target");
        fs::write(&target, "keep").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&target, &lock).unwrap();
        assert!(bind(&socket).is_err());
        fs::remove_file(&lock).unwrap();
        fs::hard_link(&target, &lock).unwrap();
        assert_eq!(
            bind(&socket).err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(fs::read_to_string(target).unwrap(), "keep");
        assert!(!socket.exists());
    }

    #[test]
    fn cleanup_preserves_replacement_sockets_files_and_symlinks() {
        let root = fixture();
        let socket = root.path().join("broker.sock");
        let original = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let cleanup = SocketCleanup::new(&socket);
        fs::remove_file(&socket).unwrap();
        let replacement = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        drop(cleanup);
        assert!(socket.exists());
        let cleanup = SocketCleanup::new(&socket);
        fs::remove_file(&socket).unwrap();
        fs::write(&socket, "keep").unwrap();
        drop(cleanup);
        assert_eq!(fs::read_to_string(&socket).unwrap(), "keep");
        drop(SocketCleanup::new(&socket));
        assert!(socket.exists());
        fs::remove_file(&socket).unwrap();
        symlink(root.path(), &socket).unwrap();
        drop(SocketCleanup::new(&socket));
        assert!(
            fs::symlink_metadata(&socket)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        drop((original, replacement));
    }

    #[test]
    fn explicit_long_socket_path_reports_the_platform_limit_before_spawning() {
        let root = fixture();
        let socket = root.path().join("x".repeat(150));
        let error = prepare_parent(&socket).unwrap_err();
        assert!(error.to_string().contains("use a shorter socket path"));
        assert!(!socket.exists());
    }
}
