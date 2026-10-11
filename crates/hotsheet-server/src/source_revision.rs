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
    /// Why `source_revision` is unavailable (HS2-6C7NQ7): an explicitly revisioned build
    /// monitors no source root, or the monitored root could not be read or hashed.
    pub source_unavailable_reason: Option<String>,
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
        let (source_revision, source_unavailable_reason) = match self.source_root.as_deref() {
            None => (
                None,
                Some(
                    "explicit build revision: HOT_SHEET_BUILD_REVISION was set when this binary \
                     was built, so no source root is monitored"
                        .to_owned(),
                ),
            ),
            Some(root) => match self.source_revision(root) {
                Ok(revision) => (Some(revision), None),
                Err(error) => (
                    None,
                    Some(format!(
                        "source root {} unreadable: {error}",
                        root.display()
                    )),
                ),
            },
        };
        let source_stale = matches!(
            (&self.build_revision, &source_revision),
            (Some(build), Some(source)) if build != source
        );
        SourceRevisionStatus {
            build_revision: self.build_revision.clone(),
            source_revision,
            source_stale,
            source_unavailable_reason,
        }
    }

    fn source_revision(&self, root: &Path) -> io::Result<String> {
        let (files, fingerprint) = source_fingerprint(root)?;
        if let Ok(cache) = self.cache.lock()
            && cache.fingerprint.as_ref() == Some(&fingerprint)
            && let Some(revision) = cache.revision.clone()
        {
            return Ok(revision);
        }
        let revision = hash_source_files(root, &files);
        if let Ok(mut cache) = self.cache.lock() {
            cache.fingerprint = Some(fingerprint);
            cache.revision = revision.as_ref().ok().cloned();
        }
        revision
    }
}

/// Hash local workspace source independently of Git metadata. Covering every local crate
/// is conservative: a change in a server dependency can never leave an old binary marked
/// current, even when Cargo's workspace dependency graph changes later.
pub fn revision_for_source_root(root: &Path) -> io::Result<String> {
    let (files, _) = source_fingerprint(root)?;
    hash_source_files(root, &files)
}

fn source_fingerprint(root: &Path) -> io::Result<(Vec<PathBuf>, SourceFingerprint)> {
    if !root.join("Cargo.toml").is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "server source root is unavailable",
        ));
    }
    let mut files = vec![PathBuf::from("Cargo.toml")];
    if root.join("crates/hotsheet-server/src").is_dir() {
        if root.join("Cargo.lock").is_file() {
            files.push(PathBuf::from("Cargo.lock"));
        }
        if root
            .join("crates/hotsheet-server/github-app-client-id.txt")
            .is_file()
        {
            files.push(PathBuf::from(
                "crates/hotsheet-server/github-app-client-id.txt",
            ));
        }
        for bundled in [
            "plugins",
            ".claude/skills/hotsheet",
            ".agents/skills/hotsheet",
        ] {
            if root.join(bundled).is_dir() {
                collect_source_files(root, &root.join(bundled), &mut files)?;
            }
        }
        let mut crates = fs::read_dir(root.join("crates"))?.collect::<Result<Vec<_>, _>>()?;
        crates.sort_by_key(fs::DirEntry::file_name);
        for entry in crates {
            if !entry.file_type()?.is_dir() || !entry.path().join("Cargo.toml").is_file() {
                continue;
            }
            let relative = entry.path().strip_prefix(root).unwrap().to_path_buf();
            files.push(relative.join("Cargo.toml"));
            if entry.path().join("build.rs").is_file() {
                files.push(relative.join("build.rs"));
            }
            if entry.path().join("src").is_dir() {
                collect_source_files(root, &entry.path().join("src"), &mut files)?;
            }
        }
    } else if root.join("src").is_dir() {
        if root.join("build.rs").is_file() {
            files.push(PathBuf::from("build.rs"));
        }
        collect_source_files(root, &root.join("src"), &mut files)?;
    } else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "server source root is unavailable",
        ));
    }
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
                source_unavailable_reason: None,
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
        let reason = unavailable.status().source_unavailable_reason.unwrap();
        assert!(
            reason.contains("unreadable") && reason.contains("missing"),
            "{reason}"
        );
    }

    /// HS2-6C7NQ7: a binary built with an inherited HOT_SHEET_BUILD_REVISION monitors no
    /// source root; the status must say so instead of a silent null.
    #[test]
    fn explicit_revision_build_reports_why_source_revision_is_unavailable() {
        let monitor = SourceRevisionMonitor {
            build_revision: Some("release-1".into()),
            source_root: None,
            cache: Arc::new(Mutex::new(RevisionCache::default())),
        };
        let status = monitor.status();
        assert_eq!(status.source_revision, None);
        assert!(!status.source_stale);
        assert!(
            status
                .source_unavailable_reason
                .unwrap()
                .contains("HOT_SHEET_BUILD_REVISION")
        );
    }

    /// HS2-6C7NQ7: a linked git worktree (a `.git` file, not a directory) under a hidden
    /// `.claude/worktrees/<name>` parent hashes exactly like the main checkout.
    #[test]
    fn linked_worktree_under_hidden_parent_hashes_like_its_main_checkout() {
        let base = tempfile::tempdir().unwrap();
        let write_workspace = |root: &Path| {
            fs::create_dir_all(root.join("crates/hotsheet-server/src")).unwrap();
            fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
            fs::write(
                root.join("crates/hotsheet-server/Cargo.toml"),
                "[package]\n",
            )
            .unwrap();
            fs::write(
                root.join("crates/hotsheet-server/src/lib.rs"),
                "pub fn f() {}\n",
            )
            .unwrap();
        };
        let main = base.path().join("main");
        write_workspace(&main);
        fs::create_dir_all(main.join(".git")).unwrap();
        let worktree = main.join(".claude/worktrees/agent-1");
        write_workspace(&worktree);
        fs::write(
            worktree.join(".git"),
            "gitdir: ../../../.git/worktrees/agent-1\n",
        )
        .unwrap();
        let built = revision_for_source_root(&main).unwrap();
        assert_eq!(built, revision_for_source_root(&worktree).unwrap());
        let status = SourceRevisionMonitor::for_source_root(&built, &worktree).status();
        assert_eq!(status.source_revision.as_deref(), Some(built.as_str()));
        assert_eq!(status.source_unavailable_reason, None);
    }

    #[test]
    fn workspace_revision_changes_when_a_sibling_crate_changes() {
        let workspace = tempfile::tempdir().unwrap();
        fs::write(workspace.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(workspace.path().join("Cargo.lock"), "lock-v1").unwrap();
        for name in ["hotsheet-server", "hotsheet-ticketing"] {
            let crate_root = workspace.path().join("crates").join(name);
            fs::create_dir_all(crate_root.join("src")).unwrap();
            fs::write(
                crate_root.join("Cargo.toml"),
                format!("[package]\nname='{name}'\n"),
            )
            .unwrap();
            fs::write(crate_root.join("src/lib.rs"), "pub const VALUE: u8 = 1;\n").unwrap();
        }
        let built = revision_for_source_root(workspace.path()).unwrap();
        let monitor = SourceRevisionMonitor::for_source_root(&built, workspace.path());
        assert!(!monitor.status().source_stale);
        fs::write(
            workspace
                .path()
                .join("crates/hotsheet-ticketing/src/lib.rs"),
            "pub const VALUE: u16 = 22;\n",
        )
        .unwrap();
        assert!(monitor.status().source_stale);
        let sibling_revision = revision_for_source_root(workspace.path()).unwrap();
        fs::write(workspace.path().join("Cargo.lock"), "lock-v2").unwrap();
        let lock_revision = revision_for_source_root(workspace.path()).unwrap();
        assert_ne!(sibling_revision, lock_revision);
        fs::create_dir_all(workspace.path().join("plugins/codex")).unwrap();
        fs::write(workspace.path().join("plugins/codex/SKILL.md"), "changed").unwrap();
        assert_ne!(
            lock_revision,
            revision_for_source_root(workspace.path()).unwrap()
        );
    }
}
