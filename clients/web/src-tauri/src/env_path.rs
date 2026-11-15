//! Blank-as-unset path overrides for the desktop shell (HS2-15ZN8E).
//!
//! This crate is its own Cargo workspace and depends on no Hot Sheet crate, so it keeps a
//! local copy of `hotsheet_sync::env_path::non_blank`: an empty or whitespace-only value
//! counts as unset rather than becoming a relative path in the working directory. A
//! non-empty relative value is kept as given. The HS2-1D4DQF/HS2-15ZN8E policy test in
//! `crates/hotsheet-ticketing/tests/machine_home_policy.rs` pins that the shell reads its
//! override only through here.

use std::ffi::OsString;
use std::path::PathBuf;

/// Overrides the desktop browser-preferences storage directory.
pub const STORAGE_DIRECTORY_ENV: &str = "HOTSHEET_DESKTOP_STORAGE_DIRECTORY";

/// `value` as a path, or `None` when it is absent, empty, or whitespace-only.
pub fn non_blank(value: Option<OsString>) -> Option<PathBuf> {
    value
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
}

/// The storage directory: a non-blank override, else `default()`.
pub fn resolve_storage_directory<E>(
    value: Option<OsString>,
    default: impl FnOnce() -> Result<PathBuf, E>,
) -> Result<PathBuf, E> {
    non_blank(value).map_or_else(default, Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(value: &str) -> Option<OsString> {
        Some(OsString::from(value))
    }

    fn fallback() -> Result<PathBuf, ()> {
        Ok(PathBuf::from("/app-data/browser-preferences-v1"))
    }

    #[test]
    fn hs2_15zn8e_storage_directory_treats_blank_as_unset() {
        assert_eq!(
            resolve_storage_directory(None, fallback),
            Ok(PathBuf::from("/app-data/browser-preferences-v1"))
        );
        for blank in ["", " ", "\t", " \n "] {
            assert_eq!(
                resolve_storage_directory(os(blank), fallback),
                Ok(PathBuf::from("/app-data/browser-preferences-v1")),
                "{blank:?}"
            );
        }
    }

    #[test]
    fn hs2_15zn8e_storage_directory_keeps_real_overrides() {
        assert_eq!(
            resolve_storage_directory(os("/tmp/storage"), fallback),
            Ok(PathBuf::from("/tmp/storage"))
        );
        assert_eq!(
            resolve_storage_directory(os("rel/storage"), fallback),
            Ok(PathBuf::from("rel/storage"))
        );
        // A fallback error surfaces only when no override applies.
        assert_eq!(
            resolve_storage_directory(os(" "), || Err::<PathBuf, _>("no app data")),
            Err("no app data")
        );
        assert_eq!(
            resolve_storage_directory(os("/tmp/s"), || Err::<PathBuf, _>("no app data")),
            Ok(PathBuf::from("/tmp/s"))
        );
    }
}
