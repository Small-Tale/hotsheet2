//! The one resolver for Hot Sheet 2's machine home and other path-valued environment
//! overrides (HS2-1D4DQF).
//!
//! An unset, empty, or whitespace-only variable means "not set". Before this module each
//! crate resolved `HOTSHEET_HOME` itself with `PathBuf::from(value)`, so a failed
//! `HOTSHEET_HOME=$(mktemp -d)` substitution produced `""` and the CLI wrote its SQLite
//! `index/` (and settings, checkouts, keys metadata) into the working directory. A
//! non-empty relative value is kept as given: it is an explicit choice, and silently
//! replacing it with the default home could send a test harness to the real `~/.hotsheet2`.

use std::ffi::OsString;
use std::path::PathBuf;

/// The directory name of the default machine home under the user's home directory. NOT
/// `.hotsheet` (HS1's data directory).
pub const DEFAULT_HOME_DIR: &str = ".hotsheet2";

/// The blank-as-unset rule lives in the leaf `hotsheet-sync` crate so crates that do not
/// depend on this one share it (HS2-15ZN8E).
pub use hotsheet_sync::env_path::{env_path, non_blank};

/// Pure resolution: `hotsheet_home` when set, else `<home>/.hotsheet2`, else
/// `<temp>/.hotsheet2`. Blank values count as unset at both levels.
pub fn resolve_hotsheet_home(hotsheet_home: Option<OsString>, home: Option<OsString>) -> PathBuf {
    non_blank(hotsheet_home).unwrap_or_else(|| {
        non_blank(home)
            .unwrap_or_else(std::env::temp_dir)
            .join(DEFAULT_HOME_DIR)
    })
}

/// The machine-wide Hot Sheet 2 home: `${HOTSHEET_HOME}`, else `~/.hotsheet2`.
pub fn hotsheet_home() -> PathBuf {
    resolve_hotsheet_home(std::env::var_os("HOTSHEET_HOME"), std::env::var_os("HOME"))
}

/// The default machine home for the current user (`~/.hotsheet2`), ignoring
/// `HOTSHEET_HOME`; `None` when `HOME` is unset or blank.
pub fn default_hotsheet_home() -> Option<PathBuf> {
    env_path("HOME").map(|home| home.join(DEFAULT_HOME_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(value: &str) -> Option<OsString> {
        Some(OsString::from(value))
    }

    #[test]
    fn hs2_1d4dqf_unset_home_falls_back_to_the_user_default() {
        assert_eq!(
            resolve_hotsheet_home(None, os("/users/a")),
            PathBuf::from("/users/a/.hotsheet2")
        );
    }

    #[test]
    fn hs2_1d4dqf_empty_and_whitespace_homes_count_as_unset() {
        for blank in ["", " ", "\t", " \n "] {
            assert_eq!(
                resolve_hotsheet_home(os(blank), os("/users/a")),
                PathBuf::from("/users/a/.hotsheet2"),
                "HOTSHEET_HOME={blank:?}"
            );
        }
    }

    #[test]
    fn hs2_1d4dqf_absolute_and_relative_homes_are_kept_as_given() {
        assert_eq!(
            resolve_hotsheet_home(os("/tmp/hs-home"), os("/users/a")),
            PathBuf::from("/tmp/hs-home")
        );
        assert_eq!(
            resolve_hotsheet_home(os("target/test-home"), os("/users/a")),
            PathBuf::from("target/test-home")
        );
        // Surrounding spaces are part of a real path; only all-blank values are unset.
        assert_eq!(
            resolve_hotsheet_home(os(" /tmp/spaced "), None),
            PathBuf::from(" /tmp/spaced ")
        );
    }

    #[test]
    fn hs2_1d4dqf_blank_user_home_never_yields_a_relative_default() {
        for home in [None, os(""), os("   ")] {
            let resolved = resolve_hotsheet_home(os(""), home.clone());
            assert_eq!(
                resolved,
                std::env::temp_dir().join(".hotsheet2"),
                "HOME={home:?}"
            );
            assert!(resolved.is_absolute());
        }
    }

    #[test]
    fn hs2_1d4dqf_non_blank_filters_each_transition() {
        assert_eq!(non_blank(None), None);
        assert_eq!(non_blank(os("")), None);
        assert_eq!(non_blank(os("  ")), None);
        assert_eq!(non_blank(os("rel")), Some(PathBuf::from("rel")));
        assert_eq!(non_blank(os("/abs")), Some(PathBuf::from("/abs")));
    }

    #[test]
    fn hs2_1d4dqf_live_resolver_reads_the_test_home() {
        // `.cargo/config.toml` forces a repository-local HOTSHEET_HOME for every test.
        let home = hotsheet_home();
        assert!(!home.as_os_str().is_empty());
        assert_eq!(Some(home), env_path("HOTSHEET_HOME"));
    }
}
