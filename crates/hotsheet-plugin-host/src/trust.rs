//! The trust gate for code-bearing plugins (docs/38 §38.7). Approving a plugin records the
//! digest of its whole directory (`plugin_digest`, shared with notifier and claim-backend
//! approvals); every spawn recomputes it, and any added, removed or edited file refuses
//! to start until the user trusts the plugin again.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use hotsheet_ticketing::plugin_digest::plugin_digest;
use serde::{Deserialize, Serialize};

/// The approvals file, relative to the machine's Hot Sheet home.
pub const APPROVALS_FILE: &str = "plugins/approvals.json";

const SCHEMA: u32 = 1;

/// Why a plugin may not start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrustError {
    /// The plugin has never been approved on this machine.
    NotApproved { id: String, digest: String },
    /// The plugin's files changed since it was approved.
    Changed {
        id: String,
        approved: String,
        current: String,
    },
    /// The directory or the approvals file could not be read.
    Unreadable(String),
}

impl std::fmt::Display for TrustError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustError::NotApproved { id, .. } => write!(
                f,
                "plugin {id} is not trusted on this machine; review it, then run `hotsheet-cli plugin trust {id}`"
            ),
            TrustError::Changed { id, .. } => write!(
                f,
                "plugin {id} changed since it was trusted; review it, then run `hotsheet-cli plugin trust {id}`"
            ),
            TrustError::Unreadable(why) => write!(f, "plugin trust could not be checked: {why}"),
        }
    }
}

impl std::error::Error for TrustError {}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Approvals {
    schema: u32,
    #[serde(default)]
    approvals: BTreeMap<String, Approval>,
}

/// One recorded approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub digest: String,
}

/// The machine's plugin approvals.
#[derive(Clone, Debug)]
pub struct TrustStore {
    path: PathBuf,
}

impl TrustStore {
    /// The store under `home` (the machine's `HOTSHEET_HOME`).
    pub fn new(home: &Path) -> Self {
        TrustStore {
            path: home.join(APPROVALS_FILE),
        }
    }

    /// The approvals file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Result<Approvals, TrustError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                let approvals: Approvals = serde_json::from_slice(&bytes)
                    .map_err(|e| TrustError::Unreadable(format!("{}: {e}", self.path.display())))?;
                if approvals.schema != SCHEMA {
                    return Err(TrustError::Unreadable(format!(
                        "{} has schema {}, expected {SCHEMA}",
                        self.path.display(),
                        approvals.schema
                    )));
                }
                Ok(approvals)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Approvals {
                schema: SCHEMA,
                ..Default::default()
            }),
            Err(e) => Err(TrustError::Unreadable(format!(
                "{}: {e}",
                self.path.display()
            ))),
        }
    }

    fn write(&self, approvals: &Approvals) -> Result<(), TrustError> {
        let unreadable =
            |e: std::io::Error| TrustError::Unreadable(format!("{}: {e}", self.path.display()));
        let parent = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(unreadable)?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(unreadable)?;
        let bytes = serde_json::to_vec_pretty(approvals)
            .map_err(|e| TrustError::Unreadable(e.to_string()))?;
        file.write_all(&bytes).map_err(unreadable)?;
        file.persist(&self.path).map_err(|e| unreadable(e.error))?;
        Ok(())
    }

    /// The current digest of `dir`.
    pub fn digest(dir: &Path) -> Result<String, TrustError> {
        plugin_digest(dir).map_err(TrustError::Unreadable)
    }

    /// The recorded approval for `id`, if any.
    pub fn approval(&self, id: &str) -> Result<Option<Approval>, TrustError> {
        Ok(self.read()?.approvals.get(id).cloned())
    }

    /// Records approval of `id` at its current contents and returns the digest.
    pub fn approve(&self, id: &str, dir: &Path) -> Result<String, TrustError> {
        let digest = Self::digest(dir)?;
        let mut approvals = self.read()?;
        approvals.approvals.insert(
            id.to_string(),
            Approval {
                digest: digest.clone(),
            },
        );
        self.write(&approvals)?;
        Ok(digest)
    }

    /// Removes `id`'s approval. Returns whether one existed.
    pub fn revoke(&self, id: &str) -> Result<bool, TrustError> {
        let mut approvals = self.read()?;
        let existed = approvals.approvals.remove(id).is_some();
        if existed {
            self.write(&approvals)?;
        }
        Ok(existed)
    }

    /// Checks that `dir` still matches `id`'s approval.
    pub fn check(&self, id: &str, dir: &Path) -> Result<(), TrustError> {
        let current = Self::digest(dir)?;
        match self.approval(id)? {
            None => Err(TrustError::NotApproved {
                id: id.to_string(),
                digest: current,
            }),
            Some(approval) if approval.digest == current => Ok(()),
            Some(approval) => Err(TrustError::Changed {
                id: id.to_string(),
                approved: approval.digest,
                current,
            }),
        }
    }
}
