//! Deterministic local-build revision hashing and cheap runtime staleness checks.

use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const REVISION_PREFIX: &str = "source-sha256:";

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceFingerprint(Vec<(PathBuf, u64, SystemTime)>);

#[derive(Debug, Default)]
struct RevisionCache {
    fingerprint: Option<SourceFingerprint>,
    revision: Option<String>,
}

/// The server build revision and its relationship to the source tree, when available.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRevisionStatus {
    pub build_revision: Option<String>,
    pub source_revision: Option<String>,
    pub source_stale: bool,
}

/// Recomputes a local source revision only when the source file set, sizes, or mtimes change.
#[derive(Clone, Debug)]
pub struct SourceRevisionMonitor {
    build_revision: Option<String>,
    source_root: Option<PathBuf>,
    cache: Arc<Mutex<RevisionCache>>,
}

impl SourceRevisionMonitor {
    /// Monitor the source tree captured by a normal local build. Explicitly revisioned
    /// release builds intentionally omit the source root and therefore never report stale.
    pub fn current_build() -> Self {
        Self {
            build_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
            source_root: option_env!("HOT_SHEET_LOCAL_SOURCE_ROOT").map(PathBuf::from),
            cache: Arc::new(Mutex::new(RevisionCache::default())),
        }
    }

    /// Construct a monitor for an explicit source root (embedders and integration tests).
    pub fn for_source_root(build_revision: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            build_revision: Some(build_revision.into()),
            source_root: Some(root.into()),
            cache: Arc::new(Mutex::new(RevisionCache::default())),
        }
    }

    pub fn status(&self) -> SourceRevisionStatus {
        let source_revision = self.source_root.as_deref().and_then(|root| {
            let (files, fingerprint) = source_fingerprint(root).ok()?;
            if let Ok(cache) = self.cache.lock()
                && cache.fingerprint.as_ref() == Some(&fingerprint)
            {
                return cache.revision.clone();
            }

            let revision = hash_source_files(root, &files).ok();
            if let Ok(mut cache) = self.cache.lock() {
                cache.fingerprint = Some(fingerprint);
                cache.revision = revision.clone();
            }
            revision
        });
        let source_stale = matches!(
            (&self.build_revision, &source_revision),
            (Some(build), Some(source)) if build != source
        );
        SourceRevisionStatus {
            build_revision: self.build_revision.clone(),
            source_revision,
            source_stale,
        }
    }
}

/// Hash the build-relevant server crate source independently of Git metadata. This keeps
/// local builds deterministic and avoids false staleness from unrelated repository commits.
pub fn revision_for_source_root(root: &Path) -> io::Result<String> {
    let (files, _) = source_fingerprint(root)?;
    hash_source_files(root, &files)
}

fn source_fingerprint(root: &Path) -> io::Result<(Vec<PathBuf>, SourceFingerprint)> {
    if !root.join("Cargo.toml").is_file() || !root.join("src").is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "server source root is unavailable",
        ));
    }
    let mut files = vec![PathBuf::from("Cargo.toml")];
    if root.join("build.rs").is_file() {
        files.push(PathBuf::from("build.rs"));
    }
    collect_source_files(root, &root.join("src"), &mut files)?;
    files.sort();

    let mut entries = Vec::with_capacity(files.len());
    for relative in &files {
        let metadata = fs::metadata(root.join(relative))?;
        entries.push((relative.clone(), metadata.len(), metadata.modified()?));
    }
    Ok((files, SourceFingerprint(entries)))
}

fn collect_source_files(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_source_files(root, &entry.path(), files)?;
        } else if file_type.is_file() {
            files.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .expect("source entry stays under root")
                    .to_path_buf(),
            );
        }
    }
    Ok(())
}

fn hash_source_files(root: &Path, files: &[PathBuf]) -> io::Result<String> {
    let mut hash = Sha256::new();
    for relative in files {
        let path = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let contents = fs::read(root.join(relative))?;
        hash.update((path.len() as u64).to_le_bytes());
        hash.update(path.as_bytes());
        hash.update((contents.len() as u64).to_le_bytes());
        hash.update(contents);
    }
    Ok(format!("{REVISION_PREFIX}{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_tree() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("Cargo.toml"), "[package]\nname='server'\n").unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 1 }\n",
        )
        .unwrap();
        root
    }

    #[test]
    fn revision_is_deterministic_and_changes_with_build_source() {
        let first = source_tree();
        let second = source_tree();
        let first_revision = revision_for_source_root(first.path()).unwrap();
        assert_eq!(
            first_revision,
            revision_for_source_root(second.path()).unwrap()
        );

        fs::write(
            first.path().join("src/lib.rs"),
            "pub fn value() -> u16 { 22 }\n",
        )
        .unwrap();
        assert_ne!(
            first_revision,
            revision_for_source_root(first.path()).unwrap()
        );
    }

    #[test]
    fn monitor_reports_only_known_source_differences_as_stale() {
        let root = source_tree();
        let built = revision_for_source_root(root.path()).unwrap();
        let monitor = SourceRevisionMonitor::for_source_root(&built, root.path());
        assert_eq!(
            monitor.status(),
            SourceRevisionStatus {
                build_revision: Some(built.clone()),
                source_revision: Some(built),
                source_stale: false,
            }
        );

        fs::write(
            root.path().join("src/new.rs"),
            "pub const NEW: bool = true;\n",
        )
        .unwrap();
        let changed = monitor.status();
        assert!(changed.source_stale);
        assert_ne!(changed.build_revision, changed.source_revision);

        let unavailable =
            SourceRevisionMonitor::for_source_root("release-1", root.path().join("missing"));
        assert_eq!(unavailable.status().source_revision, None);
        assert!(!unavailable.status().source_stale);
    }
}
