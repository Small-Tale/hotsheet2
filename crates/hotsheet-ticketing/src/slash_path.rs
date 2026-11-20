//! The display form of a store- or repository-relative path (HS2-T6YXN0).
//!
//! Hot Sheet shows a path inside a store or checkout with `/` between its parts on every
//! platform, as Git does: `.git/index.lock`, `.git/info/exclude`, `tickets/01/<id>.md`.
//! Sync and lock reasons travel between paired machines, setup plans are compared with
//! what was written, and people read all of them, so one form avoids `.git\index.lock` on
//! Windows and mixed forms such as `.git\info/exclude`. Real filesystem access keeps the
//! operating system's paths; only the text shown or exchanged uses this form. An absolute
//! path is a machine-local location, not a store-relative one, and keeps its native form.

use std::path::{Component, Path};

/// `path` with `/` between its parts when it is relative; an absolute path as displayed
/// by the platform. A part is never rewritten, so a Unix file name holding `\` survives.
#[must_use]
pub fn slash_path(path: &Path) -> String {
    if path.has_root()
        || path
            .components()
            .any(|part| matches!(part, Component::Prefix(_)))
    {
        return path.display().to_string();
    }
    let parts: Vec<_> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect();
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::slash_path;
    use std::path::{Path, PathBuf};

    #[test]
    fn hs2_t6yxn0_relative_paths_show_forward_slashes_on_every_platform() {
        let built: PathBuf = [".git", "refs", "remotes", "origin", "feature.lock"]
            .iter()
            .collect();
        assert_eq!(slash_path(&built), ".git/refs/remotes/origin/feature.lock");
        // A mixed path, as `Path::join` with a slash literal builds on Windows.
        let mixed = Path::new(".git").join("info/exclude");
        assert_eq!(slash_path(&mixed), ".git/info/exclude");
        assert_eq!(slash_path(Path::new("AGENTS.md")), "AGENTS.md");
        assert_eq!(slash_path(Path::new("")), "");
        assert_eq!(
            slash_path(Path::new("../sibling/store")),
            "../sibling/store"
        );
    }

    #[cfg(windows)]
    #[test]
    fn hs2_t6yxn0_windows_separators_become_slashes_and_absolute_paths_stay_native() {
        assert_eq!(slash_path(Path::new(r".git\index.lock")), ".git/index.lock");
        assert_eq!(slash_path(Path::new(r"tickets/01\X.md")), "tickets/01/X.md");
        assert_eq!(slash_path(Path::new(r"C:\store\x.md")), r"C:\store\x.md");
    }

    #[cfg(unix)]
    #[test]
    fn hs2_t6yxn0_a_unix_name_with_a_backslash_is_kept_and_absolute_paths_stay_native() {
        assert_eq!(slash_path(Path::new(r"dir/a\b.md")), r"dir/a\b.md");
        assert_eq!(slash_path(Path::new("/store/x.md")), "/store/x.md");
    }
}
