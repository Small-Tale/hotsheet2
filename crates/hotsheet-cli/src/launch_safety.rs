//! HS2-103 launch safety for `hotsheet-cli trigger`.
//!
//! When we drive an AI tool headlessly, the tool (and its own MCP config) can invoke a
//! bare `hotsheet` — and on a developer's login PATH that name may resolve to an **HS1**
//! launcher, which starts HS1's production instance and can kill the running dev
//! instance (`docs/04-core-server-cli.md` §4.4 — the whole reason our binary is named
//! `hotsheet-cli`, not `hotsheet`). These helpers make a bare `trigger` safe by default:
//!
//! - [`ShimDir`] + [`prepend_path`] put a `hotsheet` → `hotsheet-cli` shim (and the CLI's
//!   own dir) at the front of the launched tool's PATH, so bare `hotsheet` hits *our*
//!   safe CLI and `hotsheet-mcp` resolves.
//! - [`assert_no_hs1`] refuses to drive a tool in a project that still holds an HS1
//!   store, and [`assert_hotsheet_resolves`] confirms the shim actually wins on PATH.
//! - [`mcp_command`] lets `setup` write an **absolute** `hotsheet-mcp` path, so the MCP
//!   config works even without the PATH shim.
//!
//! The primitives here are pure/injectable so they unit-test without launching anything.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// The directory holding the running `hotsheet-cli` executable — so its siblings
/// (`hotsheet-cli`, `hotsheet-mcp`) resolve when it's placed on the child PATH.
pub fn exe_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("locating the running hotsheet-cli executable")?;
    exe.parent()
        .map(Path::to_path_buf)
        .context("the hotsheet-cli executable has no parent directory")
}

/// The `hotsheet-mcp` command string to record in a tool's MCP config: the absolute
/// sibling next to the running CLI when it exists (so the config needs no PATH munging —
/// HS2-103), otherwise the bare `fallback` (resolved via PATH at launch).
pub fn mcp_command(fallback: &str) -> String {
    mcp_command_for(exe_dir().ok().as_deref(), fallback)
}

fn mcp_command_for(exe_dir: Option<&Path>, fallback: &str) -> String {
    exe_dir
        .map(|d| d.join("hotsheet-mcp"))
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| fallback.to_string())
}

/// A transient directory holding a `hotsheet` → `hotsheet-cli` shim. Cleaned on drop, so
/// keep it alive for as long as the launched tool runs.
pub struct ShimDir {
    dir: tempfile::TempDir,
}

impl ShimDir {
    /// Create the shim dir with an executable `hotsheet` that execs `hotsheet_cli`.
    pub fn create(hotsheet_cli: &Path) -> Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("hs2-shim-")
            .tempdir()
            .context("creating the launch-safety shim dir")?;
        write_shim(dir.path(), hotsheet_cli)?;
        Ok(Self { dir })
    }

    /// The shim directory (put this at the front of the child PATH).
    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

#[cfg(unix)]
fn write_shim(dir: &Path, hotsheet_cli: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let shim = dir.join("hotsheet");
    // `{:?}` quotes the path, which is valid sh for ordinary install/temp paths.
    let script = format!("#!/bin/sh\nexec {hotsheet_cli:?} \"$@\"\n");
    std::fs::write(&shim, script).with_context(|| format!("writing shim {}", shim.display()))?;
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))
        .with_context(|| format!("chmod +x {}", shim.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_shim(_dir: &Path, _hotsheet_cli: &Path) -> Result<()> {
    bail!("`hotsheet-cli trigger` launch safety is only implemented on unix");
}

/// Build a child `PATH` with `dirs` prepended (in order) ahead of `base`, dropping
/// duplicates so the shim keeps priority.
pub fn prepend_path(dirs: &[&Path], base: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut push = |s: String, out: &mut Vec<String>| {
        if seen.insert(s.clone()) {
            out.push(s);
        }
    };
    for d in dirs {
        push(d.to_string_lossy().into_owned(), &mut out);
    }
    for part in base.split(':').filter(|s| !s.is_empty()) {
        push(part.to_string(), &mut out);
    }
    out.join(":")
}

/// Refuse to drive a tool in a project that still holds an HS1 store (`.hotsheet/`) —
/// the HS2-103 `assert_no_hs1` gate, so a headless run can't touch HS1 data.
pub fn assert_no_hs1(project: &Path) -> Result<()> {
    if project.join(".hotsheet").exists() {
        bail!(
            "refusing to drive a tool in {}: an HS1 store (.hotsheet/) is present — \
             migrate or move it first (HS2-103 launch safety)",
            project.display()
        );
    }
    Ok(())
}

/// Assert that a bare `hotsheet` on `path` resolves to our shim — i.e. the first PATH
/// directory carrying an executable `hotsheet` is `shim_dir`.
pub fn assert_hotsheet_resolves(path: &str, shim_dir: &Path) -> Result<()> {
    match first_dir_with_executable(path, "hotsheet") {
        Some(dir) if dir == shim_dir => Ok(()),
        Some(other) => bail!(
            "launch-safety check failed: bare `hotsheet` would resolve to {} instead of the \
             Hot Sheet shim in {}",
            other.join("hotsheet").display(),
            shim_dir.display()
        ),
        None => bail!(
            "launch-safety check failed: the Hot Sheet `hotsheet` shim ({}) is not on the child PATH",
            shim_dir.display()
        ),
    }
}

fn first_dir_with_executable(path: &str, name: &str) -> Option<PathBuf> {
    path.split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .find(|dir| is_executable_file(&dir.join(name)))
}

#[cfg(unix)]
fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(p: &Path) -> bool {
    p.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepend_path_puts_dirs_first_and_dedups() {
        let a = Path::new("/shim");
        let b = Path::new("/exe");
        let out = prepend_path(&[a, b], "/usr/bin:/exe:/bin");
        // shim + exe lead; the duplicate /exe from the base is dropped.
        assert_eq!(out, "/shim:/exe:/usr/bin:/bin");
    }

    #[test]
    fn prepend_path_handles_an_empty_base() {
        assert_eq!(prepend_path(&[Path::new("/shim")], ""), "/shim");
    }

    #[test]
    fn assert_no_hs1_flags_a_legacy_store() {
        let dir = tempfile::tempdir().unwrap();
        assert!(assert_no_hs1(dir.path()).is_ok());
        std::fs::create_dir(dir.path().join(".hotsheet")).unwrap();
        let err = assert_no_hs1(dir.path()).unwrap_err().to_string();
        assert!(err.contains("HS1 store"), "{err}");
    }

    #[test]
    fn shim_execs_the_cli_and_wins_on_path() {
        let cli = tempfile::tempdir().unwrap();
        let cli_bin = cli.path().join("hotsheet-cli");
        std::fs::write(&cli_bin, "#!/bin/sh\necho ran-cli\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&cli_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let shim = ShimDir::create(&cli_bin).unwrap();
        assert!(shim.path().join("hotsheet").is_file());

        // The shim must win over another `hotsheet` later on PATH.
        let other = tempfile::tempdir().unwrap();
        let other_hs = other.path().join("hotsheet");
        std::fs::write(&other_hs, "#!/bin/sh\necho hs1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&other_hs, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = prepend_path(&[shim.path(), other.path()], "");
        assert!(assert_hotsheet_resolves(&path, shim.path()).is_ok());

        // If the shim isn't first, the check fails.
        let unsafe_path = format!("{}:{}", other.path().display(), shim.path().display());
        assert!(assert_hotsheet_resolves(&unsafe_path, shim.path()).is_err());

        // Running the shim actually execs the CLI.
        #[cfg(unix)]
        {
            let out = std::process::Command::new(shim.path().join("hotsheet"))
                .output()
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ran-cli");
        }
    }

    #[test]
    fn assert_hotsheet_resolves_errors_when_absent() {
        let shim = tempfile::tempdir().unwrap();
        assert!(assert_hotsheet_resolves("/usr/bin:/bin", shim.path()).is_err());
    }

    #[test]
    fn mcp_command_prefers_the_absolute_sibling() {
        let dir = tempfile::tempdir().unwrap();
        // No sibling yet → falls back to the bare command.
        assert_eq!(
            mcp_command_for(Some(dir.path()), "hotsheet-mcp"),
            "hotsheet-mcp"
        );
        assert_eq!(mcp_command_for(None, "hotsheet-mcp"), "hotsheet-mcp");
        // With a sibling present → the absolute path.
        let sib = dir.path().join("hotsheet-mcp");
        std::fs::write(&sib, "x").unwrap();
        assert_eq!(
            mcp_command_for(Some(dir.path()), "hotsheet-mcp"),
            sib.to_string_lossy()
        );
    }
}
