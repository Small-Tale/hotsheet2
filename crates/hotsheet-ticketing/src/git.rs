//! The single constructor for every `git` subprocess Hot Sheet spawns (HS2-RRD417).
//!
//! Git locates its repository from inherited environment variables before it looks at
//! the working directory or `-C`: `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`,
//! `GIT_OBJECT_DIRECTORY`, `GIT_COMMON_DIR`, and friends. Git hooks, `git rebase
//! --exec`, `git bisect run`, merge drivers, and many wrapper tools export them. A Hot
//! Sheet process started under one of them would otherwise commit tickets into the
//! *calling* repository, or turn it bare with `git init --bare <path>`.
//!
//! [`command`] returns a [`Command`] for `git` with every repository-locating variable
//! removed, so the store path Hot Sheet passes (`-C <store>` or
//! [`Command::current_dir`]) is the only thing that selects the repository.
//! [`command_in`] additionally pins both. A source-scan test forbids a bare
//! `Command::new("git")` anywhere else under `crates/`.
//!
//! Identity and configuration variables that do not pick a repository
//! (`GIT_AUTHOR_*`, `GIT_COMMITTER_*`, `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_NOSYSTEM`,
//! `GIT_SSH_COMMAND`, `GIT_TERMINAL_PROMPT`, ...) are deliberately kept.

use std::path::Path;
use std::process::Command;

/// Environment variables removed from every git subprocess.
///
/// The first block is exactly what `git rev-parse --local-env-vars` prints: the set
/// git itself clears before it runs a command in a *different* repository (for
/// example a submodule). The second block adds variables that also change which
/// repository or refs a command sees: namespace selection, discovery limits, and the
/// receive-pack quarantine directory.
pub const REPOSITORY_ENV_VARS: &[&str] = &[
    // `git rev-parse --local-env-vars`
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
    // Additional repository/ref selection.
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_QUARANTINE_PATH",
];

/// A `git` [`Command`] with every [`REPOSITORY_ENV_VARS`] entry removed.
///
/// The caller must still name the repository explicitly, with `-C <dir>`,
/// [`Command::current_dir`], or an absolute path argument (`git init <path>`).
/// Prefer [`command_in`] when one directory is the whole context.
#[must_use]
pub fn command() -> Command {
    // This is the one sanctioned bare spawn; every other site goes through here.
    let mut cmd = Command::new("git");
    for var in REPOSITORY_ENV_VARS {
        cmd.env_remove(var);
    }
    cmd
}

/// [`command`] run in `dir`: sets both the working directory and `-C <dir>`.
///
/// A relative `dir` is made absolute first, so `-C` cannot resolve it a second time
/// against the new working directory.
#[must_use]
pub fn command_in(dir: impl AsRef<Path>) -> Command {
    let dir = std::path::absolute(dir.as_ref()).unwrap_or_else(|_| dir.as_ref().to_owned());
    let dir = dir.as_path();
    let mut cmd = command();
    cmd.current_dir(dir).arg("-C").arg(dir);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    #[test]
    fn covers_every_variable_git_clears_for_other_repositories() {
        let out = command()
            .args(["rev-parse", "--local-env-vars"])
            .output()
            .expect("git runs");
        assert!(out.status.success());
        let ours: BTreeSet<&str> = REPOSITORY_ENV_VARS.iter().copied().collect();
        let missing: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|v| !v.is_empty() && !ours.contains(v))
            .map(str::to_owned)
            .collect();
        assert!(
            missing.is_empty(),
            "this git clears {missing:?} for other repositories; add them to REPOSITORY_ENV_VARS"
        );
    }

    #[test]
    fn command_removes_repository_variables_and_keeps_identity() {
        let mut cmd = command();
        cmd.env("GIT_AUTHOR_NAME", "kept");
        let envs: Vec<(String, Option<String>)> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        for var in REPOSITORY_ENV_VARS {
            assert!(
                envs.iter().any(|(k, v)| k == var && v.is_none()),
                "{var} must be removed"
            );
        }
        assert!(
            envs.iter()
                .any(|(k, v)| k == "GIT_AUTHOR_NAME" && v.as_deref() == Some("kept"))
        );
    }

    #[test]
    fn command_in_pins_working_directory_and_dash_c() {
        let dir = PathBuf::from("/some/store");
        let cmd = command_in(&dir);
        assert_eq!(cmd.get_current_dir(), Some(dir.as_path()));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, vec![std::ffi::OsStr::new("-C"), dir.as_os_str()]);
    }

    #[test]
    fn command_in_makes_a_relative_directory_absolute() {
        let cmd = command_in("relative/store");
        let expected = std::env::current_dir().unwrap().join("relative/store");
        assert_eq!(cmd.get_current_dir(), Some(expected.as_path()));
        assert_eq!(cmd.get_args().nth(1), Some(expected.as_os_str()));
    }
}
