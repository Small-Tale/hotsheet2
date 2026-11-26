//! Cached Git repository discovery (HS2-CBZV9W).
//!
//! The machine server asks Git the same location questions about the same store and
//! checkout over and over: one `local-host` e2e run spent 264 of its 774 `git`
//! processes on two `rev-parse` probes (docs/git-discovery-cache-2026-11-25.md).
//! A spawn costs about 14 ms on macOS and 90-135 ms on Windows.
//!
//! [`DiscoveryCache`] keeps a probe's answer per canonical directory
//! ([`hotsheet_sync::paths::canonicalize`]) and revalidates it on every lookup with a
//! [`DiscoveryStamp`]: a handful of `stat` calls and two tiny reads instead of a
//! process. There is no expiry, timer or polling; an entry is reused only while every
//! file the answer depends on is unchanged:
//!
//! - **Location** ([`LocationStamp`]): the directory's own identity (a deleted and
//!   recreated path, or a relocated checkout, gets a new one) and the `.git` entry of the
//!   directory and of every ancestor. A `.git` directory contributes its identity and
//!   its `config` (`core.worktree`, `core.bare`); a gitfile (linked worktree,
//!   submodule) its identity, size and mtime plus the identity of the directory it
//!   points to. A repository initialized or removed anywhere on the path, a worktree
//!   added, pruned or re-added, and a symlink retargeted (a different canonical key) all
//!   miss.
//! - **HEAD** ([`HeadStamp`], owner probes only): the nearest `.git`'s `HEAD` content
//!   and the metadata of the ref it names, `packed-refs` and `reftable/tables.list` in
//!   the common directory. Git replaces each by renaming a fresh lock file, so a commit,
//!   branch switch, reset or `pack-refs` always changes one of them.
//!
//! The stamp is taken *before* the probe runs, so a change that lands while Git runs is
//! seen by the next lookup. A probe that reports a Git directory other than the one the
//! stamp inspected (an invalid `.git` Git skipped, an unusual layout) is returned but
//! never cached, and a probe can decline caching (for example when Git could not run).
//! Concurrent lookups may each run the probe; the last answer wins, and every answer is
//! correct for the state its stamp describes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// Entries kept before the cache starts over. A machine server touches a few stores and
/// checkouts; tests create thousands of temporary directories in one process.
const MAX_ENTRIES: usize = 512;

/// One file system entry's identity and, for files, its content signature.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EntryStamp {
    identity: (u64, u64),
    is_dir: bool,
    len: u64,
    modified: Option<SystemTime>,
}

#[cfg(unix)]
fn identity(metadata: &std::fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
fn identity(metadata: &std::fs::Metadata) -> (u64, u64) {
    // No stable file index on stable Rust off Unix: the creation time tells a recreated
    // entry apart from the one it replaced.
    let created = metadata
        .created()
        .ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .unwrap_or_default();
    (created.as_secs(), u64::from(created.subsec_nanos()))
}

/// `path`'s stamp, following symlinks as Git does. A directory's size and mtime change
/// whenever an entry inside it does (a lock file, a ticket write), so only its identity
/// counts; anything inside that matters is stamped on its own.
fn entry_stamp(path: &Path) -> Option<EntryStamp> {
    let metadata = std::fs::metadata(path).ok()?;
    let is_dir = metadata.is_dir();
    Some(EntryStamp {
        identity: identity(&metadata),
        is_dir,
        len: if is_dir { 0 } else { metadata.len() },
        modified: if is_dir {
            None
        } else {
            metadata.modified().ok()
        },
    })
}

/// One `.git` entry found on the way up from the probed directory.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DotGitStamp {
    entry: EntryStamp,
    /// `config` of a `.git` directory, or the directory a gitfile points to.
    detail: Option<EntryStamp>,
}

/// Everything that decides where Git finds the repository for a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocationStamp {
    directory: Option<EntryStamp>,
    dot_gits: Vec<(PathBuf, DotGitStamp)>,
    /// The Git directory the nearest `.git` entry names, as Git would read it.
    nearest_git_dir: Option<PathBuf>,
}

/// The target of a `gitdir: <path>` file, resolved against the file's directory.
fn gitfile_target(dot_git: &Path) -> Option<PathBuf> {
    let contents = std::fs::read_to_string(dot_git).ok()?;
    let target = contents.lines().next()?.strip_prefix("gitdir:")?.trim();
    let target = hotsheet_sync::paths::from_git(target);
    Some(if target.is_absolute() {
        target
    } else {
        dot_git.parent()?.join(target)
    })
}

impl LocationStamp {
    /// Stamp the canonical directory `canonical` and every `.git` entry on its ancestry.
    pub(crate) fn of(canonical: &Path) -> Self {
        let mut dot_gits = Vec::new();
        let mut nearest_git_dir = None;
        for ancestor in canonical.ancestors() {
            let dot_git = ancestor.join(".git");
            let Some(entry) = entry_stamp(&dot_git) else {
                continue;
            };
            let (detail, git_dir) = if entry.is_dir {
                (entry_stamp(&dot_git.join("config")), Some(dot_git.clone()))
            } else {
                let target = gitfile_target(&dot_git);
                (target.as_deref().and_then(entry_stamp), target)
            };
            if nearest_git_dir.is_none() {
                nearest_git_dir =
                    Some(git_dir.and_then(|dir| hotsheet_sync::paths::canonicalize(dir).ok()));
            }
            dot_gits.push((ancestor.to_path_buf(), DotGitStamp { entry, detail }));
        }
        Self {
            directory: entry_stamp(canonical),
            dot_gits,
            nearest_git_dir: nearest_git_dir.flatten(),
        }
    }
}

/// What `HEAD` of a Git directory resolves through, without resolving it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeadStamp {
    head: Option<Vec<u8>>,
    refs: Vec<Option<EntryStamp>>,
}

impl HeadStamp {
    pub(crate) fn of(git_dir: &Path) -> Self {
        let head = std::fs::read(git_dir.join("HEAD")).ok();
        let common = std::fs::read_to_string(git_dir.join("commondir"))
            .ok()
            .map_or_else(
                || git_dir.to_path_buf(),
                |value| git_dir.join(hotsheet_sync::paths::from_git(value.trim())),
            );
        let mut refs = vec![
            entry_stamp(&common.join("packed-refs")),
            entry_stamp(&common.join("reftable").join("tables.list")),
            entry_stamp(&git_dir.join("reftable").join("tables.list")),
        ];
        if let Some(name) = head
            .as_deref()
            .and_then(|head| std::str::from_utf8(head).ok())
            .and_then(|head| head.trim().strip_prefix("ref:"))
            .map(str::trim)
        {
            refs.push(entry_stamp(&common.join(name)));
            refs.push(entry_stamp(&git_dir.join(name)));
        }
        Self { head, refs }
    }
}

/// The validation key a cached answer was recorded under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveryStamp {
    location: LocationStamp,
    head: Option<HeadStamp>,
}

/// Whether a probe's answer may be reused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reuse {
    /// Never cache this answer (Git could not run, or the answer is transient).
    Never,
    /// Cache while the stamp holds.
    WhileUnchanged,
    /// Cache only if Git reported this Git directory, the one the stamp inspected.
    WithGitDir(PathBuf),
}

/// What [`DiscoveryCache::get_or_probe`] stamps beyond the location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Watch {
    Location,
    LocationAndHead,
}

/// Probe answers per canonical directory, revalidated by [`DiscoveryStamp`].
#[derive(Debug, Default)]
pub(crate) struct DiscoveryCache<V> {
    entries: Mutex<HashMap<PathBuf, (DiscoveryStamp, V)>>,
}

impl<V: Clone> DiscoveryCache<V> {
    pub(crate) fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// The cached answer for `path` while its stamp holds; otherwise `probe(path)`,
    /// recorded when it allows reuse. `probe` always receives the caller's spelling.
    pub(crate) fn get_or_probe(
        &self,
        path: &Path,
        watch: Watch,
        probe: impl FnOnce(&Path) -> (V, Reuse),
    ) -> V {
        let Ok(key) = hotsheet_sync::paths::canonicalize(path) else {
            return probe(path).0;
        };
        let stamp = Self::stamp(&key, watch);
        if let Some(value) = self.lookup(&key, &stamp) {
            return value;
        }
        let (value, reuse) = probe(path);
        let reusable = match reuse {
            Reuse::Never => false,
            Reuse::WhileUnchanged => true,
            Reuse::WithGitDir(reported) => {
                stamp.location.nearest_git_dir.is_some()
                    && hotsheet_sync::paths::canonicalize(reported).ok()
                        == stamp.location.nearest_git_dir
            }
        };
        if reusable {
            self.record(key, stamp, value.clone());
        }
        value
    }

    fn stamp(key: &Path, watch: Watch) -> DiscoveryStamp {
        let location = LocationStamp::of(key);
        let head = match watch {
            Watch::Location => None,
            Watch::LocationAndHead => location.nearest_git_dir.as_deref().map(HeadStamp::of),
        };
        DiscoveryStamp { location, head }
    }

    fn lookup(&self, key: &Path, stamp: &DiscoveryStamp) -> Option<V> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        entries
            .get(key)
            .filter(|(recorded, _)| recorded == stamp)
            .map(|(_, value)| value.clone())
    }

    fn record(&self, key: PathBuf, stamp: DiscoveryStamp, value: V) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if entries.len() >= MAX_ENTRIES && !entries.contains_key(&key) {
            entries.clear();
        }
        entries.insert(key, (stamp, value));
    }
}

#[cfg(test)]
#[path = "git_discovery_tests.rs"]
mod tests;
