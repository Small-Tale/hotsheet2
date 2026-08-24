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

/// The `hotsheet-mcp` command string to record in a tool's MCP config — the core resolver
/// (absolute sibling next to the running binary, else the bare fallback). Re-exported here
/// for the CLI's launch paths (`IsolatedCodexHome`); the canonical impl lives in the core
/// plugins crate so the server resolves it identically (HS2-91).
pub use hotsheet_plugins::mcp_command;

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

/// The user's real `CODEX_HOME` — the ambient `$CODEX_HOME` if set, else `~/.codex`. Used
/// as the source to copy auth from when building an isolated home.
pub fn default_codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

/// A transient, MCP-free `CODEX_HOME` for a headless codex launch (HS2-YRDQNX). Codex reads
/// its MCP servers from `$CODEX_HOME/config.toml`, so `--mcp-config` can't isolate it the
/// way it isolates Claude; instead we hand codex a throwaway home whose ONLY `mcp_servers`
/// entry is the Hot Sheet shim — never the user's global servers (which may include an HS1
/// channel that could kill the dev instance). Cleaned on drop, so keep it alive for as long
/// as the launched tool runs.
pub struct IsolatedCodexHome {
    dir: tempfile::TempDir,
    /// The tool program whose daemon runs against this home. `Some` for a daemon home —
    /// its daemon is stopped on drop so a run can't orphan it (HS2-9M6T68).
    daemon_program: Option<String>,
}

impl IsolatedCodexHome {
    /// Build the isolated home for a **direct** `app-server` (a fresh process per
    /// connection): copy `auth.json` from `source_home` (if present, so the launched codex
    /// stays signed in) and write a `config.toml` whose sole MCP server is `server_name` →
    /// `command`/`args`. No other user state is carried over.
    pub fn create(
        source_home: &Path,
        server_name: &str,
        command: &str,
        args: &[String],
    ) -> Result<Self> {
        Self::build(source_home, server_name, command, args, None)
    }

    /// Build the isolated home for the shared **daemon** (HS2-B7C66H): like [`create`], but
    /// under a **short** root (the daemon's control socket must fit `sun_path`, ~104 bytes
    /// on macOS — the default temp dir can overflow it) and with the managed standalone
    /// install symlinked in (the daemon needs `<home>/packages`). `program` is the tool
    /// whose daemon we start — recorded so it's stopped when this home drops (HS2-9M6T68).
    pub fn create_for_daemon(
        source_home: &Path,
        server_name: &str,
        command: &str,
        args: &[String],
        program: &str,
    ) -> Result<Self> {
        Self::build(source_home, server_name, command, args, Some(program))
    }

    fn build(
        source_home: &Path,
        server_name: &str,
        command: &str,
        args: &[String],
        daemon_program: Option<&str>,
    ) -> Result<Self> {
        let for_daemon = daemon_program.is_some();
        let mut builder = tempfile::Builder::new();
        builder.prefix("hs2cx-");
        // For the daemon, keep the base short so `<home>/app-server-control/…sock` fits
        // sun_path; `/tmp` is short and writable on the unix targets `trigger` supports.
        let dir = if for_daemon && Path::new("/tmp").is_dir() {
            builder.tempdir_in("/tmp")
        } else {
            builder.tempdir()
        }
        .context("creating the isolated CODEX_HOME")?;

        let auth = source_home.join("auth.json");
        if auth.is_file() {
            std::fs::copy(&auth, dir.path().join("auth.json")).with_context(|| {
                format!("copying {} into the isolated CODEX_HOME", auth.display())
            })?;
        }
        std::fs::write(
            dir.path().join("config.toml"),
            isolated_codex_config(server_name, command, args),
        )
        .context("writing the isolated CODEX_HOME config.toml")?;

        if for_daemon {
            // The daemon manages a standalone install under `<home>/packages`; symlink the
            // user's so it needn't re-download into the throwaway home.
            let src_pkgs = source_home.join("packages");
            if src_pkgs.exists() {
                symlink_dir(&src_pkgs, &dir.path().join("packages")).with_context(|| {
                    format!(
                        "symlinking {} into the isolated CODEX_HOME",
                        src_pkgs.display()
                    )
                })?;
            }
        }
        Ok(Self {
            dir,
            daemon_program: daemon_program.map(str::to_string),
        })
    }

    /// The isolated home (set `CODEX_HOME` to this for the launched tool).
    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for IsolatedCodexHome {
    fn drop(&mut self) {
        // Stop this home's daemon (if any) BEFORE the TempDir removes the dir, so a
        // shared-daemon run doesn't leave an orphaned codex process pointing at a home that
        // no longer exists (HS2-9M6T68). Best-effort: `daemon stop` is a no-op when none is
        // running, and any error is irrelevant to teardown.
        if let Some(program) = &self.daemon_program {
            let _ = crate::stop_codex_daemon_in(program, self.dir.path());
        }
    }
}

#[cfg(unix)]
fn symlink_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(not(unix))]
fn symlink_dir(_src: &Path, _dst: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "the shared codex daemon is only supported on unix",
    ))
}

/// A `config.toml` body with a single `[mcp_servers.<name>]` and nothing else, so codex
/// loads only the Hot Sheet shim.
fn isolated_codex_config(server_name: &str, command: &str, args: &[String]) -> String {
    let mut entry = toml::Table::new();
    entry.insert("command".into(), toml::Value::String(command.to_string()));
    entry.insert(
        "args".into(),
        toml::Value::Array(args.iter().cloned().map(toml::Value::String).collect()),
    );
    let mut servers = toml::Table::new();
    servers.insert(server_name.to_string(), toml::Value::Table(entry));
    let mut root = toml::Table::new();
    root.insert("mcp_servers".into(), toml::Value::Table(servers));
    toml::to_string_pretty(&root).expect("serializing the isolated codex config")
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

/// Resolve a manifest-declared program to an executable absolute path without invoking a
/// shell. Absolute paths are accepted directly; relative paths containing separators are
/// rejected so a project cannot substitute its own executable implicitly.
pub fn resolve_program(program: &str) -> Result<PathBuf> {
    let candidate = Path::new(program);
    if candidate.is_absolute() {
        if is_executable_file(candidate) {
            return Ok(candidate.to_path_buf());
        }
        bail!("launch program is not executable: {}", candidate.display());
    }
    if candidate.components().count() != 1 {
        bail!("launch program must be a binary name or absolute path: {program}");
    }
    let path = std::env::var("PATH").unwrap_or_default();
    first_dir_with_executable(&path, program)
        .map(|dir| dir.join(program))
        .with_context(|| format!("launch program '{program}' was not found on PATH"))
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
    fn resolve_program_finds_path_binary_and_rejects_relative_paths() {
        assert!(resolve_program("sh").unwrap().is_absolute());
        assert!(resolve_program("./tool").is_err());
    }

    #[test]
    fn isolated_codex_home_copies_auth_and_writes_an_mcp_only_config() {
        // A fake user CODEX_HOME with auth + a global MCP server that must NOT leak in.
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("auth.json"), r#"{"token":"secret"}"#).unwrap();
        std::fs::write(
            src.path().join("config.toml"),
            "[mcp_servers.evil]\ncommand = \"hs1-channel\"\n",
        )
        .unwrap();

        let home = IsolatedCodexHome::create(
            src.path(),
            "hotsheet",
            "/abs/hotsheet-mcp",
            &["--path".into(), "/store".into()],
        )
        .unwrap();

        // auth carried over verbatim.
        assert_eq!(
            std::fs::read_to_string(home.path().join("auth.json")).unwrap(),
            r#"{"token":"secret"}"#
        );
        // config has ONLY the hotsheet server — the user's global one is gone.
        let cfg: toml::Table =
            toml::from_str(&std::fs::read_to_string(home.path().join("config.toml")).unwrap())
                .unwrap();
        let servers = cfg["mcp_servers"].as_table().unwrap();
        assert_eq!(servers.len(), 1, "no user servers leak in");
        let hs = servers["hotsheet"].as_table().unwrap();
        assert_eq!(hs["command"].as_str().unwrap(), "/abs/hotsheet-mcp");
        let args: Vec<&str> = hs["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        assert_eq!(args, vec!["--path", "/store"]);
    }

    #[test]
    fn daemon_isolated_home_symlinks_packages_and_fits_sun_path() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("auth.json"), "{}").unwrap();
        std::fs::create_dir_all(src.path().join("packages/standalone")).unwrap();

        let home = IsolatedCodexHome::create_for_daemon(
            src.path(),
            "hotsheet",
            "hotsheet-mcp",
            &["--path".into(), "/store".into()],
            "true", // harmless "daemon stop" on drop
        )
        .unwrap();

        // The managed install is symlinked in (a symlink, resolving to the source).
        let pkg = home.path().join("packages");
        assert!(
            std::fs::symlink_metadata(&pkg)
                .unwrap()
                .file_type()
                .is_symlink(),
            "packages is a symlink"
        );
        assert!(
            pkg.join("standalone").is_dir(),
            "symlink resolves to source packages"
        );

        // Still MCP-only (isolation holds for the daemon path too).
        let cfg: toml::Table =
            toml::from_str(&std::fs::read_to_string(home.path().join("config.toml")).unwrap())
                .unwrap();
        assert_eq!(cfg["mcp_servers"].as_table().unwrap().len(), 1);

        // The daemon's control socket path must fit the platform's sun_path limit (~104 on
        // macOS, 108 on Linux) — the whole reason the daemon home uses a short root.
        let sock = home
            .path()
            .join("app-server-control")
            .join("app-server-control.sock");
        assert!(
            sock.to_string_lossy().len() < 104,
            "socket path is too long for sun_path: {} ({} bytes)",
            sock.display(),
            sock.to_string_lossy().len()
        );
    }

    #[test]
    #[cfg(unix)]
    fn daemon_isolated_home_stops_its_daemon_on_drop() {
        use std::os::unix::fs::PermissionsExt;
        // A fake "codex" that records how it was invoked (args + CODEX_HOME) to a marker
        // OUTSIDE the home (so the record survives the home's removal on drop).
        let src = tempfile::tempdir().unwrap();
        let marker = src.path().join("stop.log");
        let fake = src.path().join("fakecodex");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\necho \"home=$CODEX_HOME args=$*\" >> {marker:?}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

        let home_path;
        {
            let home = IsolatedCodexHome::create_for_daemon(
                src.path(),
                "hotsheet",
                "hotsheet-mcp",
                &["--path".into()],
                fake.to_str().unwrap(),
            )
            .unwrap();
            home_path = home.path().to_path_buf();
            assert!(home_path.exists());
        } // <- drop: must stop the daemon, then remove the dir

        let log = std::fs::read_to_string(&marker).expect("daemon stop was invoked on drop");
        assert!(
            log.contains("args=app-server daemon stop"),
            "drop runs `app-server daemon stop`: {log}"
        );
        assert!(
            log.contains(&format!("home={}", home_path.display())),
            "…with this home's CODEX_HOME: {log}"
        );
        assert!(!home_path.exists(), "the home dir is removed after drop");
    }

    #[test]
    fn isolated_codex_home_without_auth_still_writes_config() {
        // No auth.json in the source → none copied, but the isolated config is still written.
        let src = tempfile::tempdir().unwrap();
        let home =
            IsolatedCodexHome::create(src.path(), "hotsheet", "hotsheet-mcp", &["--path".into()])
                .unwrap();
        assert!(!home.path().join("auth.json").exists());
        assert!(home.path().join("config.toml").is_file());
    }
    // `mcp_command` now lives in `hotsheet-plugins` (its resolver is tested there, HS2-91).
}
