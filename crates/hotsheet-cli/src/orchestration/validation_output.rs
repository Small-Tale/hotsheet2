//! HS2-ZCR3Y7: a bounded, private copy of validation step output on the machine that
//! hosts the target.
//!
//! A result records only the SHA-256 and size of each stream; the bytes used to be
//! discarded, so a failed step could not be inspected. The hosting machine now keeps a
//! stream whole when it is small enough, under the digest the result already records
//! (so the copy is content-addressed and verifiable), in
//! `${HOTSHEET_HOME}/orchestration-validation-output/<checkout-id>/<sha256>`. Nothing
//! leaves the machine: results, tickets and journals still carry digests only, and the
//! read path is machine-local. Pruning happens when a copy is written, never on a timer.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use base64::Engine;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::*;

/// The directory under a machine home.
pub const VALIDATION_OUTPUT_DIRECTORY: &str = "orchestration-validation-output";
/// A stream is kept only when it is at most this long; a longer one keeps its digest.
pub const MAX_RETAINED_STREAM_BYTES: u64 = 4 * 1024 * 1024;
/// All copies on one machine together.
pub const MAX_RETAINED_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
/// Copies on one machine.
pub const MAX_RETAINED_COPIES: usize = 512;
/// How long a copy is kept.
pub const RETAINED_OUTPUT_TTL: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// One kept stream, as the machine-local read path answers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RetainedOutput {
    pub sha256: String,
    pub bytes: u64,
    /// The stream's bytes, base64 encoded; checked against `sha256` before answering.
    pub content_base64: String,
}

fn store_root(home: &Path) -> PathBuf {
    home.join(VALIDATION_OUTPUT_DIRECTORY)
}

/// The directory a checkout's copies live in.
pub(super) fn checkout_dir(home: &Path, checkout_id: &str) -> PathBuf {
    store_root(home).join(checkout_id)
}

#[cfg(unix)]
fn private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Copies one stream while it is digested. Bytes past [`MAX_RETAINED_STREAM_BYTES`]
/// are not written, and such a stream keeps no copy.
pub(super) struct Retaining {
    directory: PathBuf,
    temporary: PathBuf,
    file: Option<fs::File>,
    written: u64,
}

impl Retaining {
    /// Start copying into a checkout's directory. `None` when it cannot be created;
    /// the step then runs as before, keeping digests only.
    pub(super) fn start(directory: &Path) -> Option<Self> {
        private_dir(directory).ok()?;
        let temporary = directory.join(format!(".partial-{}", ulid::Ulid::new()));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&temporary).ok()?;
        Some(Self {
            directory: directory.to_path_buf(),
            temporary,
            file: Some(file),
            written: 0,
        })
    }

    pub(super) fn write(&mut self, bytes: &[u8]) {
        let Some(file) = &mut self.file else { return };
        if self.written + bytes.len() as u64 > MAX_RETAINED_STREAM_BYTES
            || file.write_all(bytes).is_err()
        {
            self.file = None;
            let _ = fs::remove_file(&self.temporary);
            return;
        }
        self.written += bytes.len() as u64;
    }

    /// The stream ended with this digest: keep the copy under it, or drop it when the
    /// stream was too long or did not end cleanly. Then prune the machine's copies.
    pub(super) fn finish(mut self, digest: Option<&str>, now: SystemTime) {
        let kept = match (self.file.take(), digest) {
            (Some(file), Some(digest)) if is_digest(digest) => {
                drop(file);
                let target = self.directory.join(digest);
                if target.exists() {
                    false
                } else {
                    fs::rename(&self.temporary, &target).is_ok()
                }
            }
            _ => false,
        };
        if !kept {
            let _ = fs::remove_file(&self.temporary);
        }
        if let Some(root) = self.directory.parent() {
            prune(root, now);
        }
    }
}

/// Every kept copy under the store, with its size and time.
fn copies(root: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let mut found = Vec::new();
    let Ok(checkouts) = fs::read_dir(root) else {
        return found;
    };
    for checkout in checkouts.flatten() {
        let Ok(entries) = fs::read_dir(checkout.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !is_digest(name) {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                found.push((
                    entry.path(),
                    meta.len(),
                    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                ));
            }
        }
    }
    found
}

/// Drop expired copies, then the oldest until the machine is within its bounds.
pub fn prune(root: &Path, now: SystemTime) {
    let mut all = copies(root);
    all.retain(|(path, _, at)| {
        let expired = now
            .duration_since(*at)
            .is_ok_and(|age| age > RETAINED_OUTPUT_TTL);
        if expired {
            let _ = fs::remove_file(path);
        }
        !expired
    });
    all.sort_by_key(|(_, _, at)| *at);
    let mut total: u64 = all.iter().map(|(_, bytes, _)| bytes).sum();
    let mut count = all.len();
    for (path, bytes, _) in all {
        if total <= MAX_RETAINED_TOTAL_BYTES && count <= MAX_RETAINED_COPIES {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total -= bytes;
            count -= 1;
        }
    }
}

/// Read one kept copy of a checkout's step output by its digest, checking it still
/// matches. `NotFound` when nothing is kept under that digest for that checkout.
pub fn read_retained(
    home: &Path,
    checkout_id: &str,
    sha256: &str,
) -> Result<RetainedOutput, OrchestrationError> {
    if !is_digest(sha256) {
        return Err(OrchestrationError::Invalid(
            "a lowercase hex SHA-256 digest is required",
        ));
    }
    let path = checkout_dir(home, checkout_id).join(sha256);
    let mut file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(OrchestrationError::NotFound);
        }
        Err(error) => return Err(OrchestrationError::Storage(error)),
    };
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_RETAINED_STREAM_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(OrchestrationError::Storage)?;
    let actual: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if actual != sha256 {
        return Err(OrchestrationError::Corrupt);
    }
    Ok(RetainedOutput {
        sha256: sha256.into(),
        bytes: bytes.len() as u64,
        content_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    })
}

impl OrchestrationRuntime {
    /// HS2-ZCR3Y7: a kept copy of step output from a checkout this machine hosts, by
    /// digest. Machine-local: the route is loopback-only and no MCP action reads it.
    pub fn validation_output(
        &self,
        checkout_id: &str,
        sha256: &str,
    ) -> Result<RetainedOutput, OrchestrationError> {
        let checkout = self.owner(checkout_id)?;
        read_retained(&self.home, &checkout.id, sha256)
    }

    /// Where this machine keeps a checkout's step output.
    pub(super) fn validation_output_dir(&self, checkout_id: &str) -> PathBuf {
        checkout_dir(&self.home, checkout_id)
    }
}
