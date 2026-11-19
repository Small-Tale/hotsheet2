//! The content digest of a plugin directory (HS2-208RB1, shared with HS2-322ZSZ): the
//! approval of an external notifier or claim-backend plugin covers exactly the code it
//! saw, so any added, removed, renamed or edited file invalidates it.

use std::path::Path;

/// Plugin directories larger than this are refused rather than hashed.
const DIGEST_MAX_FILES: usize = 2000;
const DIGEST_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// SHA-256 over every file in a plugin directory: sorted relative paths, each with its
/// length and bytes; a symlink contributes its target text. Any added, removed, renamed,
/// or edited file changes it, so a channel's approval covers exactly the code it saw
/// (HS2-208RB1).
pub fn plugin_digest(dir: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut entries = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let read =
            std::fs::read_dir(&current).map_err(|e| format!("{}: {e}", current.display()))?;
        for entry in read {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                pending.push(path);
            } else {
                entries.push((path, kind.is_symlink()));
            }
            if entries.len() > DIGEST_MAX_FILES {
                return Err(format!(
                    "{} has more than {DIGEST_MAX_FILES} files",
                    dir.display()
                ));
            }
        }
    }
    entries.sort();
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    for (path, symlink) in entries {
        let relative = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = if symlink {
            std::fs::read_link(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .to_string_lossy()
                .into_owned()
                .into_bytes()
        } else {
            std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?
        };
        total += bytes.len() as u64;
        if total > DIGEST_MAX_BYTES {
            return Err(format!(
                "{} is larger than {DIGEST_MAX_BYTES} bytes",
                dir.display()
            ));
        }
        hasher.update(relative.as_bytes());
        hasher.update([0, u8::from(symlink)]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
