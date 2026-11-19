//! Approval of a claim-backend plugin by directory digest (HS2-322ZSZ, modelled on
//! HS2-208RB1). Approval is a machine decision, so it is kept under
//! `<HOTSHEET_HOME>/claim-backends/approvals.json` and never in a checked-in
//! configuration: a teammate's clone must not inherit trust in code it has not seen.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use hotsheet_ticketing::claim_backends::Change;
use hotsheet_ticketing::plugin_digest::plugin_digest;

use crate::clock::{Clock, SystemClock};
use crate::error::ClaimError;

pub const PLUGIN_ROOT_DIR: &str = "claim-backends";
const APPROVALS_FILE: &str = "approvals.json";
const SCHEMA: u32 = 1;

/// The directory plugins live in.
pub fn plugin_root(home: &Path) -> PathBuf {
    home.join(PLUGIN_ROOT_DIR)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub digest: String,
    pub approved_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct File {
    #[serde(rename = "$hotsheetSchema")]
    schema: u32,
    #[serde(default)]
    plugins: BTreeMap<String, Approval>,
}

/// Where a plugin stands against its approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum ApprovalState {
    /// The plugin directory is exactly what was approved.
    Approved { digest: String },
    /// Never approved on this machine.
    Unapproved { digest: String },
    /// Approved once, but a file was added, removed, renamed or edited since.
    Changed { approved: String, digest: String },
}

impl ApprovalState {
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved { .. })
    }
}

fn read(home: &Path) -> Result<File, ClaimError> {
    let path = plugin_root(home).join(APPROVALS_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let file: File = serde_json::from_str(&text).map_err(|_| ClaimError::Corrupt)?;
            if file.schema > SCHEMA {
                return Err(ClaimError::Corrupt);
            }
            Ok(file)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(File {
            schema: SCHEMA,
            plugins: BTreeMap::new(),
        }),
        Err(e) => Err(e.into()),
    }
}

fn write(home: &Path, file: &File) -> Result<(), ClaimError> {
    let root = plugin_root(home);
    fs::create_dir_all(&root)?;
    let text =
        serde_json::to_string_pretty(file).map_err(|e| ClaimError::Invalid(e.to_string()))?;
    let mut staged = tempfile::Builder::new()
        .prefix(".approvals-")
        .tempfile_in(&root)?;
    std::io::Write::write_all(&mut staged, text.as_bytes())?;
    staged.as_file().sync_all()?;
    staged
        .persist(root.join(APPROVALS_FILE))
        .map_err(|e| ClaimError::Storage(e.error))?;
    Ok(())
}

fn digest_of(dir: &Path) -> Result<String, ClaimError> {
    plugin_digest(dir).map_err(|e| ClaimError::Unsupported(format!("plugin digest: {e}")))
}

/// Compare a plugin directory with its recorded approval.
pub fn state(home: &Path, id: &str, dir: &Path) -> Result<ApprovalState, ClaimError> {
    let digest = digest_of(dir)?;
    Ok(match read(home)?.plugins.get(id) {
        None => ApprovalState::Unapproved { digest },
        Some(approval) if approval.digest == digest => ApprovalState::Approved { digest },
        Some(approval) => ApprovalState::Changed {
            approved: approval.digest.clone(),
            digest,
        },
    })
}

/// The refusal for a plugin that is not approved as it stands, naming the next step.
pub fn require(home: &Path, id: &str, dir: &Path) -> Result<(), ClaimError> {
    match state(home, id, dir)? {
        ApprovalState::Approved { .. } => Ok(()),
        ApprovalState::Unapproved { .. } => Err(ClaimError::Unsupported(format!(
            "claim-backend plugin '{id}' is not approved on this machine; review it, then run `hotsheet-cli resource plugin trust {id}`"
        ))),
        ApprovalState::Changed { .. } => Err(ClaimError::Unsupported(format!(
            "claim-backend plugin '{id}' changed since it was approved; review it, then run `hotsheet-cli resource plugin trust {id}`"
        ))),
    }
}

/// Approve the plugin as it is now. The same content again is `Unchanged`.
pub fn approve(home: &Path, id: &str, dir: &Path) -> Result<Change, ClaimError> {
    let digest = digest_of(dir)?;
    let mut file = read(home)?;
    let previous = file.plugins.get(id).map(|a| a.digest.clone());
    if previous.as_deref() == Some(digest.as_str()) {
        return Ok(Change::Unchanged);
    }
    file.plugins.insert(
        id.to_owned(),
        Approval {
            digest,
            approved_at: crate::model::rfc3339(SystemClock.now_ms()),
        },
    );
    write(home, &file)?;
    Ok(if previous.is_some() {
        Change::Updated
    } else {
        Change::Created
    })
}

/// Approve only when the plugin has no approval yet (adding a configuration approves the
/// plugin as it is then; a repeated add never re-trusts changed code).
pub fn approve_if_new(home: &Path, id: &str, dir: &Path) -> Result<bool, ClaimError> {
    if read(home)?.plugins.contains_key(id) {
        return Ok(false);
    }
    approve(home, id, dir).map(|_| true)
}
