//! The one blank-as-unset rule for path-valued environment overrides (HS2-1D4DQF,
//! HS2-15ZN8E).
//!
//! An unset, empty, or whitespace-only variable means "not set". `PathBuf::from("")` is a
//! relative path that resolves to the working directory, so a failed
//! `VAR=$(mktemp -d)` substitution would otherwise send state (an index, a LaunchAgents
//! plist, a process journal) into whatever checkout the process ran from. A non-empty
//! relative value is kept as given: it is an explicit choice. This lives in the
//! dependency-free leaf crate so `hotsheet-ticketing` and `hotsheet-terminals` share it.

use std::ffi::OsString;
use std::path::PathBuf;

/// `value` as a path, or `None` when it is absent, empty, or whitespace-only.
pub fn non_blank(value: Option<OsString>) -> Option<PathBuf> {
    value
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
}

/// A path-valued variable, or `None` when it is unset, empty, or whitespace-only.
pub fn env_path(name: &str) -> Option<PathBuf> {
    non_blank(std::env::var_os(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(value: &str) -> Option<OsString> {
        Some(OsString::from(value))
    }

    #[test]
    fn hs2_15zn8e_non_blank_treats_blank_as_unset_and_keeps_real_paths() {
        assert_eq!(non_blank(None), None);
        for blank in ["", " ", "\t", " \n "] {
            assert_eq!(non_blank(os(blank)), None, "{blank:?}");
        }
        assert_eq!(non_blank(os("rel/dir")), Some(PathBuf::from("rel/dir")));
        assert_eq!(non_blank(os("/abs/dir")), Some(PathBuf::from("/abs/dir")));
        // Surrounding spaces are part of a real path; only all-blank values are unset.
        assert_eq!(non_blank(os(" /spaced ")), Some(PathBuf::from(" /spaced ")));
    }

    #[test]
    fn hs2_15zn8e_env_path_returns_none_for_an_unset_variable() {
        assert_eq!(
            env_path("HOTSHEET_SYNC_TEST_VARIABLE_THAT_IS_NEVER_SET_15ZN8E"),
            None
        );
    }
}
