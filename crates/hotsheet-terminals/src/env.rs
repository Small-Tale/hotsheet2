//! **Environment scrubbing** (`docs/05` §5.4, HS1 §22.13.1). A PTY child shell must not
//! inherit Hot Sheet's own tool-marker variables (`TSX_*`, `npm_*`, `NODE_*`, and our own
//! `HOTSHEET_*` launch markers), which otherwise leak into the AI tool and its subprocesses
//! and confuse tool detection. [`scrub_env`] drops those from a base environment.
//!
//! It also drops the repository-locating git variables (`GIT_DIR`, `GIT_WORK_TREE`,
//! `GIT_INDEX_FILE`, ...; HS2-J79CZF). A server started from a git hook or `git bisect
//! run` inherits them, and a shell or AI tool started in a code checkout would otherwise
//! run `git` against the hook's repository instead of that checkout.

/// Prefixes of variables that shouldn't reach a spawned tool shell.
/// `HOT_SHEET_` covers the local host's selected-build pins (`HOT_SHEET_BUILD_REVISION`):
/// inherited by a terminal, a `cargo build` there would bake that pin in as an explicit
/// release revision and disable source-staleness detection (HS2-6C7NQ7).
const SCRUB_PREFIXES: &[&str] = &["TSX_", "npm_", "NODE_", "HOTSHEET_", "HOT_SHEET_"];

/// Exact names of the repository-locating git variables a terminal child must not inherit.
/// This leaf crate keeps its own copy of `hotsheet_ticketing::git::REPOSITORY_ENV_VARS`; a
/// compile-time parity test reads that list from source.
pub const REPOSITORY_ENV_VARS: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_QUARANTINE_PATH",
];

/// Filter a base environment down to what a child terminal should inherit — dropping any
/// variable whose name starts with a scrubbed prefix or is a repository-locating git
/// variable.
pub fn scrub_env<I, K, V>(base: I) -> Vec<(String, String)>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    base.into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .filter(|(k, _)| {
            !SCRUB_PREFIXES.iter().any(|p| k.starts_with(p))
                && !REPOSITORY_ENV_VARS.contains(&k.as_str())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_tool_markers_keeps_the_rest() {
        let base = [
            ("PATH", "/usr/bin"),
            ("TSX_something", "x"),
            ("npm_config_foo", "y"),
            ("NODE_OPTIONS", "z"),
            ("HOTSHEET_HOME", "~/.hotsheet2"),
            ("HOT_SHEET_BUILD_REVISION", "source-sha256:pinned"),
            ("HOME", "/home/me"),
        ];
        let out: Vec<(String, String)> = scrub_env(base);
        let names: Vec<&str> = out.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains(&"PATH") && names.contains(&"HOME"));
        assert!(!names.iter().any(|k| k.starts_with("TSX_")
            || k.starts_with("npm_")
            || k.starts_with("NODE_")
            || k.starts_with("HOTSHEET_")
            || k.starts_with("HOT_SHEET_")));
    }

    #[test]
    fn drops_repository_locating_git_variables_and_keeps_identity() {
        let base = [
            ("GIT_DIR", "/hook/.git"),
            ("GIT_WORK_TREE", "/hook"),
            ("GIT_INDEX_FILE", "/hook/.git/index"),
            ("GIT_AUTHOR_NAME", "kept"),
            ("GIT_SSH_COMMAND", "ssh"),
            ("PATH", "/usr/bin"),
        ];
        let out = scrub_env(base);
        let names: Vec<&str> = out.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["GIT_AUTHOR_NAME", "GIT_SSH_COMMAND", "PATH"]);
    }

    #[test]
    fn repository_list_matches_the_ticketing_git_constructor() {
        let source = include_str!("../../hotsheet-ticketing/src/git.rs");
        let start = source
            .find("pub const REPOSITORY_ENV_VARS: &[&str] = &[")
            .expect("the ticketing list exists");
        let block = &source[start..start + source[start..].find("];").unwrap()];
        let names: Vec<&str> = block
            .split('"')
            .skip(1)
            .step_by(2)
            .filter(|name| name.starts_with("GIT_"))
            .collect();
        assert_eq!(names, REPOSITORY_ENV_VARS);
    }
}
