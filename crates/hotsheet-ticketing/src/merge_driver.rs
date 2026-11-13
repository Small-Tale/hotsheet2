//! Local registration of the semantic ticket merge driver (HS2-18, HS2-9TV6A2, `docs/02`
//! §2.7).
//!
//! Git consults a merge driver only when an attribute names it and the repository's config
//! defines its command. The committed `.gitattributes` line is not enough: a rebase or a
//! `git merge-tree` replay reads attributes from the tree being merged, and an upstream
//! without that file leaves every replayed ticket to Git's text merge. So each checkout of
//! a store also carries the attribute in `<git-dir>/info/attributes`, which Git always
//! reads, and the driver command in its local config. Neither is ever committed, so every
//! machine that shares the remote registers them for itself; [`ensure_registered`] runs
//! before every sync integration and when a store is created, linked, opened by the
//! server, or checked by `doctor`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// The merge driver's name in Git config and attributes.
pub const DRIVER_NAME: &str = "hotsheet-ticket";

/// The attribute line, relative to the store root.
pub const ATTRIBUTE_LINE: &str = "tickets/**/*.md merge=hotsheet-ticket";

static PROGRAM: OnceLock<PathBuf> = OnceLock::new();

/// Record the `hotsheet-cli` executable that implements `merge-driver`. The CLI and the
/// server set it once at startup; the first value wins.
pub fn set_program(program: PathBuf) {
    let _ = PROGRAM.set(program);
}

#[cfg(test)]
thread_local! {
    /// A per-test driver program, so sync tests on one thread use a fake driver.
    pub(crate) static TEST_PROGRAM: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// The `hotsheet-cli` executable this process registers, if one was set.
#[must_use]
pub fn program() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(program) = TEST_PROGRAM.with(|program| program.borrow().clone()) {
        return Some(program);
    }
    PROGRAM.get().cloned()
}

/// The Git config value for `program`: Git appends nothing, so the placeholders name the
/// ancestor, ours (also the output), and theirs.
#[must_use]
pub fn driver_command(program: &Path) -> String {
    format!("\"{}\" merge-driver %O %A %B", program.display())
}

/// Whether the driver applies in a store, as found or after [`ensure_registered`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeDriverStatus {
    /// The attribute and a runnable driver command were already present.
    Registered,
    /// This call added the attribute or (re)wrote the driver command.
    Repaired,
    /// No runnable driver command is configured and none was available to register; Git
    /// falls back to its text merge for ticket files.
    Missing,
}

impl MergeDriverStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Repaired => "repaired",
            Self::Missing => "missing",
        }
    }

    /// Whether ticket merges go through the driver.
    #[must_use]
    pub fn applies(self) -> bool {
        self != Self::Missing
    }
}

struct Layout {
    attributes: PathBuf,
    line: String,
}

/// The store's `info/attributes` path and the attribute line relative to the repository
/// root (a designated root's tickets live under its prefix).
fn layout(store: &Path) -> Option<Layout> {
    let out = crate::git::command()
        .arg("-C")
        .arg(store)
        .args(["rev-parse", "--absolute-git-dir", "--show-prefix"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let git_dir = PathBuf::from(lines.next()?.trim_end_matches('\r'));
    let prefix = lines.next().unwrap_or("").trim_end_matches('\r');
    Some(Layout {
        attributes: git_dir.join("info").join("attributes"),
        line: format!("{prefix}{ATTRIBUTE_LINE}"),
    })
}

fn has_attribute(layout: &Layout) -> bool {
    std::fs::read_to_string(&layout.attributes)
        .is_ok_and(|text| text.lines().any(|line| line.trim() == layout.line))
}

fn configured_driver(store: &Path) -> Option<String> {
    let out = crate::git::command()
        .arg("-C")
        .arg(store)
        .args(["config", "--get", &format!("merge.{DRIVER_NAME}.driver")])
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !value.is_empty()).then_some(value)
}

/// Whether a configured driver command can still run: a quoted or absolute program must
/// exist; a bare name is left to `PATH`.
fn runnable(command: &str) -> bool {
    let program = command
        .strip_prefix('"')
        .and_then(|rest| rest.split_once('"').map(|(program, _)| program))
        .unwrap_or_else(|| command.split_whitespace().next().unwrap_or(""));
    if program.is_empty() {
        return false;
    }
    let path = Path::new(program);
    !path.is_absolute() || path.is_file()
}

/// The driver's status without changing anything.
#[must_use]
pub fn status(store: &Path) -> MergeDriverStatus {
    let Some(layout) = layout(store) else {
        return MergeDriverStatus::Missing;
    };
    let config = configured_driver(store).is_some_and(|command| runnable(&command));
    if config && has_attribute(&layout) {
        MergeDriverStatus::Registered
    } else {
        MergeDriverStatus::Missing
    }
}

/// Register the driver locally when it is missing: the attribute in `info/attributes`,
/// and the driver command when none is configured or the configured program no longer
/// exists. `program` defaults to [`program`]. Idempotent; never commits anything.
pub fn ensure_registered(store: &Path, program_override: Option<&Path>) -> MergeDriverStatus {
    let Some(layout) = layout(store) else {
        return MergeDriverStatus::Missing;
    };
    let mut repaired = false;
    let config_ok = configured_driver(store).is_some_and(|command| runnable(&command));
    let config_ok = config_ok || {
        let program = program_override.map(Path::to_path_buf).or_else(program);
        match program {
            Some(program) => {
                let name_set = git_config(store, &format!("merge.{DRIVER_NAME}.name"), {
                    "Hot Sheet semantic ticket merge"
                });
                let driver_set = git_config(
                    store,
                    &format!("merge.{DRIVER_NAME}.driver"),
                    &driver_command(&program),
                );
                repaired = name_set && driver_set;
                repaired
            }
            None => false,
        }
    };
    // The attribute is harmless without a driver command (Git then text-merges), so it is
    // written either way and the driver applies as soon as a command is configured.
    if !has_attribute(&layout) && append_attribute(&layout) {
        repaired = true;
    }
    match (config_ok && has_attribute(&layout), repaired) {
        (false, _) => MergeDriverStatus::Missing,
        (true, true) => MergeDriverStatus::Repaired,
        (true, false) => MergeDriverStatus::Registered,
    }
}

fn git_config(store: &Path, key: &str, value: &str) -> bool {
    crate::git::command()
        .arg("-C")
        .arg(store)
        .args(["config", key, value])
        .status()
        .is_ok_and(|status| status.success())
}

fn append_attribute(layout: &Layout) -> bool {
    if let Some(dir) = layout.attributes.parent()
        && std::fs::create_dir_all(dir).is_err()
    {
        return false;
    }
    let mut text = std::fs::read_to_string(&layout.attributes).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("# Hot Sheet semantic ticket merge (HS2-9TV6A2)\n");
    text.push_str(&layout.line);
    text.push('\n');
    crate::store::write_file_atomically(&layout.attributes, text.as_bytes()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) -> bool {
        crate::git::command()
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap()
            .success()
    }

    fn fake_program(dir: &Path) -> PathBuf {
        let program = dir.join("fake-hotsheet-cli");
        std::fs::write(&program, "#!/bin/sh\nexit 0\n").unwrap();
        program
    }

    #[test]
    fn hs2_9tv6a2_registers_the_attribute_and_driver_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        std::fs::create_dir(&store).unwrap();
        assert!(git(&store, &["init", "--quiet"]));
        let program = fake_program(dir.path());
        assert_eq!(status(&store), MergeDriverStatus::Missing);
        assert_eq!(
            ensure_registered(&store, Some(&program)),
            MergeDriverStatus::Repaired
        );
        assert_eq!(status(&store), MergeDriverStatus::Registered);
        assert_eq!(
            ensure_registered(&store, Some(&program)),
            MergeDriverStatus::Registered
        );
        let attributes = std::fs::read_to_string(store.join(".git/info/attributes")).unwrap();
        assert_eq!(
            attributes.matches(ATTRIBUTE_LINE).count(),
            1,
            "{attributes}"
        );
        assert_eq!(configured_driver(&store).unwrap(), driver_command(&program));
        // Nothing is committed or added to the working tree.
        assert!(!store.join(".gitattributes").exists());
    }

    #[test]
    fn hs2_9tv6a2_a_driver_whose_program_vanished_is_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        std::fs::create_dir(&store).unwrap();
        assert!(git(&store, &["init", "--quiet"]));
        let gone = dir.path().join("removed-install").join("hotsheet-cli");
        assert!(git_config(
            &store,
            "merge.hotsheet-ticket.driver",
            &driver_command(&gone)
        ));
        let program = fake_program(dir.path());
        assert_eq!(status(&store), MergeDriverStatus::Missing);
        assert_eq!(
            ensure_registered(&store, Some(&program)),
            MergeDriverStatus::Repaired
        );
        assert_eq!(configured_driver(&store).unwrap(), driver_command(&program));
        // A bare program name is left to PATH.
        assert!(runnable("hotsheet-cli merge-driver %O %A %B"));
        assert!(!runnable(""));
    }

    #[test]
    fn hs2_9tv6a2_without_a_program_the_attribute_is_kept_and_status_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        std::fs::create_dir(&store).unwrap();
        assert!(git(&store, &["init", "--quiet"]));
        assert_eq!(ensure_registered(&store, None), MergeDriverStatus::Missing);
        assert!(
            std::fs::read_to_string(store.join(".git/info/attributes"))
                .unwrap()
                .contains(ATTRIBUTE_LINE)
        );
        assert_eq!(
            ensure_registered(&store, Some(&fake_program(dir.path()))),
            MergeDriverStatus::Repaired
        );
    }

    #[test]
    fn hs2_9tv6a2_a_designated_root_registers_its_prefixed_tickets() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("code");
        let store = repo.join("tracking");
        std::fs::create_dir_all(&store).unwrap();
        assert!(git(&repo, &["init", "--quiet"]));
        let program = fake_program(dir.path());
        assert_eq!(
            ensure_registered(&store, Some(&program)),
            MergeDriverStatus::Repaired
        );
        let attributes = std::fs::read_to_string(repo.join(".git/info/attributes")).unwrap();
        assert!(
            attributes.contains("tracking/tickets/**/*.md merge=hotsheet-ticket"),
            "{attributes}"
        );
        assert_eq!(status(&store), MergeDriverStatus::Registered);
    }

    #[test]
    fn hs2_9tv6a2_outside_a_repository_nothing_is_registered() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            ensure_registered(dir.path(), Some(&fake_program(dir.path()))),
            MergeDriverStatus::Missing
        );
    }
}
