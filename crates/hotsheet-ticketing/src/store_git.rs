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

/// The machine bootstrap anchor's directory name under `${HOTSHEET_HOME}`. The anchor is a
/// metadata-only store that hosts the machine server and machine-wide state; it holds no
/// tickets by contract (HS2-726EXJ, docs/04 "Ticket-store admission").
pub const MACHINE_ANCHOR_DIR: &str = "server-bootstrap.hs2";

/// What a store is being opened for. Every source-selection entry point names one, so the
/// one machine store allowed to skip the Git check cannot quietly become a ticket source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreRole {
    /// A ticket source: linked to a checkout, hosted for clients, served over MCP, or
    /// wrapped in a `GitProvider`. Requires an owned or designated Git store.
    TicketSource,
    /// The machine bootstrap anchor (`${HOTSHEET_HOME}/server-bootstrap.hs2`). Skips the Git
    /// check but is never a checkout source or `GitProvider`.
    MachineAnchor,
}

impl StoreRole {
    /// The role a store at `root` is opened under: [`StoreRole::MachineAnchor`] exactly for
    /// the machine bootstrap anchor, [`StoreRole::TicketSource`] otherwise.
    #[must_use]
    pub fn of(root: &Path) -> Self {
        if is_machine_anchor(root) {
            Self::MachineAnchor
        } else {
            Self::TicketSource
        }
    }
}

/// The machine bootstrap anchor for the current `${HOTSHEET_HOME}`.
#[must_use]
pub fn machine_anchor() -> PathBuf {
    crate::settings::hotsheet_home().join(MACHINE_ANCHOR_DIR)
}

/// Whether `root` names the machine bootstrap anchor, comparing canonical paths when both
/// exist so a relative path or symlinked home still matches.
#[must_use]
pub fn is_machine_anchor(root: &Path) -> bool {
    is_anchor_of(root, &machine_anchor())
}

fn is_anchor_of(root: &Path, anchor: &Path) -> bool {
    match (root.canonicalize(), anchor.canonicalize()) {
        (Ok(root), Ok(anchor)) => root == anchor,
        _ => root == anchor,
    }
}

/// Why a store was refused for a role (HS2-726EXJ).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdmissionError {
    /// The store's Git layout is not a supported ticket store; the message is the
    /// [`StoreGitOwner::diagnostic`] with its exact recovery step.
    #[error("{diagnostic}")]
    UnsupportedOwner {
        root: PathBuf,
        owner: Box<StoreGitOwner>,
        diagnostic: String,
    },
    /// The machine bootstrap anchor was offered as a ticket source.
    #[error(
        "{} is Hot Sheet's machine bootstrap store, which holds no tickets; link a ticket store \
         (`hotsheet-cli init --standalone` or `hotsheet-cli bootstrap`) instead",
        root.display()
    )]
    MachineAnchorAsSource { root: PathBuf },
    /// A store other than the machine bootstrap anchor claimed the anchor role.
    #[error("{} is not Hot Sheet's machine bootstrap store", root.display())]
    NotTheMachineAnchor { root: PathBuf },
}

/// Admit the store at `root` for `role` (HS2-726EXJ). The one check every source-selection
/// entry point shares: checkout links and source registration, server hosting and its
/// primary store, the MCP backend, provider construction, and worklist generation.
///
/// - [`StoreRole::TicketSource`] requires [`StoreGitOwner::is_supported`] and refuses the
///   machine bootstrap anchor.
/// - [`StoreRole::MachineAnchor`] accepts only the anchor itself and skips the Git check.
///
/// Costs one or two `git` subprocesses for a ticket source, so callers check once when
/// they select or host a store, not on every request.
pub fn admit_store(root: &Path, role: StoreRole) -> Result<(), AdmissionError> {
    admit_store_with_anchor(root, role, &machine_anchor())
}

fn admit_store_with_anchor(
    root: &Path,
    role: StoreRole,
    anchor_path: &Path,
) -> Result<(), AdmissionError> {
    let anchor = is_anchor_of(root, anchor_path);
    match role {
        StoreRole::MachineAnchor if anchor => Ok(()),
        StoreRole::MachineAnchor => Err(AdmissionError::NotTheMachineAnchor {
            root: root.to_path_buf(),
        }),
        StoreRole::TicketSource if anchor => Err(AdmissionError::MachineAnchorAsSource {
            root: root.to_path_buf(),
        }),
        StoreRole::TicketSource => {
            let owner = FsStore::git_owner_at(root);
            match owner.diagnostic(root) {
                None => Ok(()),
                Some(diagnostic) => Err(AdmissionError::UnsupportedOwner {
                    root: root.to_path_buf(),
                    owner: Box::new(owner),
                    diagnostic,
                }),
            }
        }
    }
}

/// Make sure a repository owns the existing store directory at `root`, by its
/// [`StoreGitOwner`] rather than by the presence of `<root>/.git` (HS2-V6JEBR):
///
/// - **Owned:** nothing to do.
/// - **Designated:** the enclosing repository already tracks the store; never nest a
///   repository inside it.
/// - **NoRepository / Unowned:** `git init` makes the store its own repository.
/// - **Unavailable:** refused with the ownership diagnostic.
///
/// Idempotent: a second call on any result is a no-op.
pub fn ensure_store_repository(root: &Path) -> Result<StoreGitOwner, StoreError> {
    let owner = FsStore::git_owner_at(root);
    match owner {
        StoreGitOwner::Owned { .. } | StoreGitOwner::Designated { .. } => return Ok(owner),
        StoreGitOwner::Unavailable { .. } => return Err(unsupported(root, owner)),
        StoreGitOwner::NoRepository | StoreGitOwner::Unowned { .. } => {}
    }
    let output = crate::git::command()
        .arg("-C")
        .arg(root)
        .args(["init", "--quiet"])
        .output()?;
    if !output.status.success() {
        return Err(StoreError::Git(format!(
            "init {} failed: {}",
            root.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let owner = FsStore::git_owner_at(root);
    if owner.is_supported() {
        Ok(owner)
    } else {
        Err(unsupported(root, owner))
    }
}

fn unsupported(root: &Path, owner: StoreGitOwner) -> StoreError {
    let diagnostic = owner.diagnostic(root).unwrap_or_default();
    AdmissionError::UnsupportedOwner {
        root: root.to_path_buf(),
        owner: Box::new(owner),
        diagnostic,
    }
    .into()
}

impl FsStore {
    /// Initialize (or re-initialize) the store at `root` and settle its Git repository
    /// with [`ensure_store_repository`] (HS2-V6JEBR). Classification happens before any
    /// metadata is written, so a location Git cannot describe is refused untouched.
    /// Re-running it changes nothing further.
    pub fn init_with_repository(
        root: impl Into<PathBuf>,
        metadata: &crate::StoreMetadata,
    ) -> Result<(Self, StoreGitOwner), StoreError> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let owner = Self::git_owner_at(&root);
        if let StoreGitOwner::Unavailable { .. } = owner {
            return Err(unsupported(&root, owner));
        }
        let store = Self::init(&root, metadata)?;
        let owner = ensure_store_repository(&root)?;
        Ok((store, owner))
    }

    /// [`FsStore::init_with_repository`] for callers that only need the store: the
    /// result is always an admitted ticket source (HS2-726EXJ).
    pub fn init_git(
        root: impl Into<PathBuf>,
        metadata: &crate::StoreMetadata,
    ) -> Result<Self, StoreError> {
        Self::init_with_repository(root, metadata).map(|(store, _)| store)
    }

    /// Open the store at `root` and admit it for `role` ([`admit_store`]).
    pub fn open_admitted(root: impl Into<PathBuf>, role: StoreRole) -> Result<Self, StoreError> {
        let store = Self::open(root)?;
        admit_store(store.root(), role)?;
        Ok(store)
    }

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

    fn admit(root: &Path, role: StoreRole, anchor: &Path) -> Result<(), AdmissionError> {
        admit_store_with_anchor(root, role, anchor)
    }

    /// HS2-726EXJ: each Git owner variant under the ticket-source role, including the
    /// transitions a store can make between them, re-checked at the next selection.
    #[test]
    fn hs2_726exj_ticket_sources_require_an_owned_or_designated_store() {
        let dir = tempfile::tempdir().unwrap();
        let anchor = dir.path().join("home").join(MACHINE_ANCHOR_DIR);
        let source = StoreRole::TicketSource;

        // NoRepository → refused with the recovery step; `git init` → admitted (Owned).
        let loose = dir.path().join("loose");
        FsStore::init(&loose, &StoreMetadata::new("HS")).unwrap();
        let refused = admit(&loose, source, &anchor).unwrap_err();
        assert!(
            matches!(
                &refused,
                AdmissionError::UnsupportedOwner { owner, .. }
                    if **owner == StoreGitOwner::NoRepository
            ),
            "{refused:?}"
        );
        assert!(refused.to_string().contains("git -C"), "{refused}");
        git(&loose, &["init", "-q", "-b", "main"]);
        admit(&loose, source, &anchor).unwrap();
        // Owned → `.git` removed → refused again at the next selection.
        std::fs::remove_dir_all(loose.join(".git")).unwrap();
        assert!(admit(&loose, source, &anchor).is_err());

        // Unowned under an enclosing repository → tracked (Designated) → admitted; then
        // untracked again → refused.
        let parent = dir.path().join("parent");
        std::fs::create_dir_all(&parent).unwrap();
        git(&parent, &["init", "-q", "-b", "main"]);
        let nested = parent.join("tickets");
        FsStore::init(&nested, &StoreMetadata::new("HS")).unwrap();
        let refused = admit(&nested, source, &anchor).unwrap_err();
        assert!(
            refused.to_string().contains("does not track it"),
            "{refused}"
        );
        git(&parent, &["add", "--", "tickets/hotsheet-store.json"]);
        admit(&nested, source, &anchor).unwrap();
        git(
            &parent,
            &["rm", "-q", "--cached", "--", "tickets/hotsheet-store.json"],
        );
        assert!(admit(&nested, source, &anchor).is_err());

        // Unavailable (inside a bare repository) → refused.
        let bare = dir.path().join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        git(&bare, &["init", "-q", "--bare"]);
        FsStore::init(&bare, &StoreMetadata::new("HS")).unwrap();
        assert!(matches!(
            admit(&bare, source, &anchor),
            Err(AdmissionError::UnsupportedOwner { owner, .. })
                if matches!(*owner, StoreGitOwner::Unavailable { .. })
        ));
    }

    /// HS2-726EXJ: the machine bootstrap anchor skips the Git check under its own role but
    /// is never a ticket source, even when a repository happens to own it; no other store
    /// can claim the anchor role.
    #[test]
    fn hs2_726exj_the_machine_anchor_is_admitted_only_in_its_own_role() {
        let dir = tempfile::tempdir().unwrap();
        let anchor = dir.path().join("home").join(MACHINE_ANCHOR_DIR);
        FsStore::init(&anchor, &StoreMetadata::new("HS")).unwrap();
        admit(&anchor, StoreRole::MachineAnchor, &anchor).unwrap();
        // Another spelling of the same directory is still the anchor.
        let spelled = dir.path().join("home/./").join(MACHINE_ANCHOR_DIR);
        admit(&spelled, StoreRole::MachineAnchor, &anchor).unwrap();
        assert!(matches!(
            admit(&anchor, StoreRole::TicketSource, &anchor),
            Err(AdmissionError::MachineAnchorAsSource { .. })
        ));
        git(&anchor, &["init", "-q", "-b", "main"]);
        assert!(matches!(
            admit(&spelled, StoreRole::TicketSource, &anchor),
            Err(AdmissionError::MachineAnchorAsSource { .. })
        ));

        let other = dir.path().join("other");
        FsStore::init(&other, &StoreMetadata::new("HS")).unwrap();
        git(&other, &["init", "-q", "-b", "main"]);
        assert!(matches!(
            admit(&other, StoreRole::MachineAnchor, &anchor),
            Err(AdmissionError::NotTheMachineAnchor { .. })
        ));
        admit(&other, StoreRole::TicketSource, &anchor).unwrap();
    }
}
