//! Which Git repository owns a ticket store, and the index baseline it may supply
//! (HS2-ABV2J3, docs/02 §2.1/§2.2, docs/03 §3.4).
//!
//! Every Git ticket store is a Git repository or a designated root within one. Git's
//! ordinary upward discovery cannot tell those apart from a metadata-only store that
//! merely sits beneath an unrelated checkout (for example inside its ignored `target/`
//! directory). There the parent's `HEAD` says nothing about the store, and the parent's
//! status omits the store's ignored ticket files, so trusting it as an index baseline
//! hides every later write.
//!
//! [`FsStore::git_owner`] classifies the store with one scrubbed `rev-parse` probe.
//! Only an [`StoreGitOwner::Owned`] or [`StoreGitOwner::Designated`] store with a born
//! `HEAD` yields a [`StoreGitBaseline`]; everything else has no baseline, and the index
//! falls back to its always-correct full reconcile. The owner key travels with the
//! baseline so a cached index notices when the owning repository changes.

use std::path::{Path, PathBuf};

use ulid::Ulid;

use crate::store::{FsStore, STORE_METADATA_FILE, StoreError};

/// The repository that owns a ticket store, as classified by [`FsStore::git_owner`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreGitOwner {
    /// The store root is the top level of its own working tree: a `.git` directory or
    /// a linked-worktree `.git` file. `head` is `None` while the branch is unborn.
    Owned { key: String, head: Option<String> },
    /// The store is a designated root inside an enclosing repository that tracks the
    /// store's metadata file. `prefix` is the store's path within that working tree.
    Designated {
        key: String,
        head: Option<String>,
        toplevel: PathBuf,
        prefix: String,
    },
    /// An enclosing repository contains the store but does not track it, so its
    /// history and status say nothing about these tickets.
    Unowned { toplevel: PathBuf },
    /// No Git repository contains the store.
    NoRepository,
    /// Git could not describe a working tree here (a bare repository, a broken
    /// `.git`, or a failing `git`).
    Unavailable { reason: String },
}

impl StoreGitOwner {
    /// The commit an index may use as a baseline: present only for an owned or
    /// designated store whose branch has a commit.
    #[must_use]
    pub fn head(&self) -> Option<&str> {
        match self {
            Self::Owned { head, .. } | Self::Designated { head, .. } => head.as_deref(),
            _ => None,
        }
    }

    /// Whether this layout is a supported Git ticket store (docs/02 §2.1/§2.2).
    #[must_use]
    pub fn is_supported(&self) -> bool {
        matches!(self, Self::Owned { .. } | Self::Designated { .. })
    }

    /// An actionable explanation for an unsupported layout, or `None` when the store
    /// is owned or designated. The text names the exact recovery step; Hot Sheet has no
    /// plain-files mode to fall back to.
    #[must_use]
    pub fn diagnostic(&self, store: &Path) -> Option<String> {
        let store = store.display();
        match self {
            Self::Owned { .. } | Self::Designated { .. } => None,
            Self::Unowned { toplevel } => Some(format!(
                "ticket store {store} sits inside the Git repository {} but that repository \
                 does not track it, so Git history cannot describe these tickets. Either make \
                 the store its own repository (`git -C {store} init`, then run \
                 `hotsheet-cli init` there), or track it as a designated root (remove any \
                 ignore rule that excludes it, run `git -C {store} add -- {STORE_METADATA_FILE} \
                 tickets`, and commit).",
                toplevel.display(),
            )),
            Self::NoRepository => Some(format!(
                "ticket store {store} is not a Git repository; every ticket store must be one. \
                 Run `git -C {store} init`, then `hotsheet-cli init` there."
            )),
            Self::Unavailable { reason } => Some(format!(
                "Git cannot describe a working tree for ticket store {store}: {reason}"
            )),
        }
    }
}

/// A trusted Git baseline for incremental index reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreGitBaseline {
    /// Identifies the owning working tree, Git directory and store prefix. An index
    /// recorded under a different key must not reuse its baseline.
    pub owner_key: String,
    /// The owning repository's `HEAD` commit.
    pub head: String,
    /// Ticket ids that differ from `HEAD` in the index or working tree, including
    /// untracked and ignored ticket files.
    pub dirty: Vec<Ulid>,
}

impl FsStore {
    /// Classify which Git repository, if any, owns this store. One scrubbed
    /// `rev-parse` call, plus one `ls-files` call only when the store is a
    /// subdirectory of an enclosing working tree.
    #[must_use]
    pub fn git_owner(&self) -> StoreGitOwner {
        Self::git_owner_at(self.root())
    }

    /// Classify the directory `root` exactly as [`FsStore::git_owner`] does, before it
    /// holds store metadata (HS2-V6JEBR: `init` classifies before it writes anything). An
    /// untracked directory inside an enclosing work tree is [`StoreGitOwner::Unowned`].
    #[must_use]
    pub fn git_owner_at(root: &Path) -> StoreGitOwner {
        let output = crate::git::command()
            .arg("-C")
            .arg(root)
            .args([
                "rev-parse",
                "--show-toplevel",
                "--absolute-git-dir",
                "--show-prefix",
                "--verify",
                "-q",
                "HEAD",
            ])
            .output();
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                return StoreGitOwner::Unavailable {
                    reason: format!("could not run git: {error}"),
                };
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        // Exit 0 prints all four values; exit 1 is `--verify -q` rejecting an unborn
        // HEAD after the three location values were printed. Anything else means Git
        // found no usable working tree.
        let head = match (output.status.code(), lines.as_slice()) {
            (Some(0), [_, _, _, head, ..]) => Some((*head).to_owned()),
            (Some(1), [_, _, _]) => None,
            _ => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return if stderr.contains("not a git repository") {
                    StoreGitOwner::NoRepository
                } else {
                    StoreGitOwner::Unavailable {
                        reason: stderr.trim().to_owned(),
                    }
                };
            }
        };
        let (toplevel, git_dir, prefix) = (lines[0], lines[1], lines[2]);
        let key = format!("{toplevel}\n{git_dir}\n{prefix}");
        if prefix.is_empty() {
            return StoreGitOwner::Owned { key, head };
        }
        if Self::metadata_is_tracked(root) {
            StoreGitOwner::Designated {
                key,
                head,
                toplevel: PathBuf::from(toplevel),
                prefix: prefix.to_owned(),
            }
        } else {
            StoreGitOwner::Unowned {
                toplevel: PathBuf::from(toplevel),
            }
        }
    }

    /// Whether the enclosing repository's index contains this store's metadata file.
    fn metadata_is_tracked(root: &Path) -> bool {
        crate::git::command()
            .arg("-C")
            .arg(root)
            .args(["ls-files", "--error-unmatch", "--", STORE_METADATA_FILE])
            .output()
            .is_ok_and(|output| output.status.success())
    }

    /// The baseline an index may trust for incremental reconciliation, or `None` when
    /// the store has no owning repository or no commit yet. `None` is not an error: the
    /// index then walks every ticket file.
    pub fn git_baseline(&self) -> Result<Option<StoreGitBaseline>, StoreError> {
        let owner = self.git_owner();
        let (owner_key, head) = match owner {
            StoreGitOwner::Owned {
                key,
                head: Some(head),
            }
            | StoreGitOwner::Designated {
                key,
                head: Some(head),
                ..
            } => (key, head),
            _ => return Ok(None),
        };
        Ok(Some(StoreGitBaseline {
            owner_key,
            head,
            dirty: self.changed_ticket_ids_in_worktree()?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoreMetadata;

    fn git(dir: &Path, args: &[&str]) {
        let status = crate::git::command_in(dir).args(args).status().unwrap();
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn hs2_abv2j3_store_outside_any_repository_has_no_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        let owner = store.git_owner();
        assert_eq!(owner, StoreGitOwner::NoRepository);
        assert!(!owner.is_supported());
        assert_eq!(owner.head(), None);
        let diagnostic = owner.diagnostic(dir.path()).unwrap();
        assert!(
            diagnostic.contains("is not a Git repository"),
            "{diagnostic}"
        );
        assert!(diagnostic.contains("hotsheet-cli init"), "{diagnostic}");
        assert_eq!(store.git_baseline().unwrap(), None);
    }

    #[test]
    fn hs2_abv2j3_store_inside_a_bare_repository_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "--bare"]);
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        let owner = store.git_owner();
        assert!(
            matches!(owner, StoreGitOwner::Unavailable { .. }),
            "{owner:?}"
        );
        assert!(owner.diagnostic(dir.path()).is_some());
        assert_eq!(store.git_baseline().unwrap(), None);
        assert_eq!(store.head_commit(), None);
    }

    #[test]
    fn hs2_abv2j3_owned_unborn_repository_is_supported_without_a_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        let owner = store.git_owner();
        assert!(
            matches!(owner, StoreGitOwner::Owned { head: None, .. }),
            "{owner:?}"
        );
        assert!(owner.is_supported());
        assert_eq!(owner.diagnostic(dir.path()), None);
        assert_eq!(store.git_baseline().unwrap(), None);
    }
}
