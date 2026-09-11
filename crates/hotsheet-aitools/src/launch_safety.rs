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
//! - [`assert_trigger_project_safe`] refuses to drive a tool in a project that still
//!   holds a live HS1 store unless the selected HS2 store has the matching completed-
//!   import receipt. [`assert_no_hs1`] remains the strict gate for non-trigger launches,
//!   and [`assert_hotsheet_resolves`] confirms the shim actually wins on PATH.
//! - [`mcp_command`] lets `setup` write an **absolute** `hotsheet-mcp` path, so the MCP
//!   config works even without the PATH shim.
//!
//! The primitives here are pure/injectable so they unit-test without launching anything.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const HS1_IMPORT_RECEIPT: &str = "hotsheet-hs1-import.json";

#[derive(serde::Deserialize)]
struct Hs1ImportReceipt {
    #[serde(rename = "$hotsheetSchema")]
    schema: u32,
    #[serde(rename = "sourceProject")]
    source_project: PathBuf,
}

/// Locate the real `hotsheet-cli` that a safety shim must execute. The caller may be the
/// CLI itself, the sibling server binary, or a Cargo test under `target/*/deps`.
pub fn hotsheet_cli() -> Result<PathBuf> {
    let current = std::env::current_exe().context("locating the running Hot Sheet executable")?;
    resolve_hotsheet_cli(&current, std::env::var_os("PATH").as_deref())
}

fn resolve_hotsheet_cli(current: &Path, path: Option<&std::ffi::OsStr>) -> Result<PathBuf> {
    let binary = format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX);
    if current
        .file_name()
        .is_some_and(|name| name == binary.as_str())
    {
        return Ok(current.to_path_buf());
    }
    let parent = current
        .parent()
        .context("the running Hot Sheet executable has no parent directory")?;
    for candidate in [
        parent.join(&binary),
        parent.parent().unwrap_or(parent).join(&binary),
    ] {
        if is_executable_file(&candidate) {
            return Ok(candidate);
        }
    }
    if let Some(path) = path {
        if let Some(candidate) = std::env::split_paths(path)
            .map(|dir| dir.join(&binary))
            .find(|candidate| is_executable_file(candidate))
        {
            return Ok(candidate);
        }
    }
    bail!(
        "hotsheet-cli is not installed beside {} or on PATH",
        current.display()
    )
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
fn write_shim(dir: &Path, hotsheet_cli: &Path) -> Result<()> {
    let shim = dir.join("hotsheet.cmd");
    let script = format!("@echo off\r\n\"{}\" %*\r\n", hotsheet_cli.display());
    std::fs::write(&shim, script).with_context(|| format!("writing shim {}", shim.display()))
}

/// The user's real `CODEX_HOME` — the ambient `$CODEX_HOME` if set, else `~/.codex`. Used
/// as the source to copy auth from when building an isolated home.
pub fn default_codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

/// An MCP-free `CODEX_HOME` for a headless codex launch (HS2-YRDQNX). Codex reads
/// its MCP servers from `$CODEX_HOME/config.toml`, so `--mcp-config` can't isolate it the
/// way it isolates Claude; instead we hand codex a throwaway home whose ONLY `mcp_servers`
/// entry is the Hot Sheet shim — never the user's global servers (which may include an HS1
/// channel that could kill the dev instance). One-shot homes are cleaned on drop; durable
/// client/session homes retain thread metadata across server restarts.
pub struct IsolatedCodexHome {
    dir: CodexHomeDir,
    /// The tool program whose daemon runs against this home. `Some` for a daemon home —
    /// its daemon is stopped on drop so a run can't orphan it (HS2-9M6T68).
    daemon_program: Option<String>,
}

enum CodexHomeDir {
    Temporary(tempfile::TempDir),
    Persistent { _storage: PathBuf, runtime: PathBuf },
}

impl CodexHomeDir {
    fn path(&self) -> &Path {
        match self {
            Self::Temporary(dir) => dir.path(),
            Self::Persistent { runtime, .. } => runtime,
        }
    }
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

    /// Build the same isolated daemon home at a durable machine-local path. Used by
    /// client-owned conversations so Codex's thread metadata survives a server restart.
    pub fn create_persistent_for_daemon(
        path: &Path,
        source_home: &Path,
        server_name: &str,
        command: &str,
        args: &[String],
        program: Option<&str>,
    ) -> Result<Self> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("creating persistent CODEX_HOME {}", path.display()))?;
        let runtime = persistent_runtime_path(path, program.is_some())?;
        Self::populate(
            CodexHomeDir::Persistent {
                _storage: path.to_path_buf(),
                runtime,
            },
            source_home,
            server_name,
            command,
            args,
            program,
        )
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

        Self::populate(
            CodexHomeDir::Temporary(dir),
            source_home,
            server_name,
            command,
            args,
            daemon_program,
        )
    }

    fn populate(
        dir: CodexHomeDir,
        source_home: &Path,
        server_name: &str,
        command: &str,
        args: &[String],
        daemon_program: Option<&str>,
    ) -> Result<Self> {
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

        if daemon_program.is_some() {
            // The daemon manages a standalone install under `<home>/packages`; symlink the
            // user's so it needn't re-download into the throwaway home.
            let src_pkgs = source_home.join("packages");
            if src_pkgs.exists() && !dir.path().join("packages").exists() {
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

#[cfg(unix)]
fn persistent_runtime_path(storage: &Path, needs_short_socket_path: bool) -> Result<PathBuf> {
    use std::hash::{Hash, Hasher};
    if !needs_short_socket_path {
        return Ok(storage.to_path_buf());
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    storage.hash(&mut hasher);
    let state_root = storage
        .ancestors()
        .find(|candidate| candidate.file_name().is_some_and(|name| name == "drive"))
        .and_then(Path::parent)
        .or_else(|| storage.parent())
        .context("persistent CODEX_HOME has no state root")?;
    let runtime_root = state_root.join("d");
    std::fs::create_dir_all(&runtime_root).with_context(|| {
        format!(
            "creating short persistent CODEX_HOME root {}",
            runtime_root.display()
        )
    })?;
    let runtime = runtime_root.join(format!("{:016x}", hasher.finish()));
    if std::fs::symlink_metadata(storage).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        if !runtime.exists() {
            std::fs::create_dir(&runtime)
                .with_context(|| format!("creating CODEX_HOME {}", runtime.display()))?;
        }
        let actual = runtime.canonicalize()?;
        let expected = storage.canonicalize()?;
        if actual != expected {
            bail!(
                "persistent CODEX_HOME alias {} points to {} instead of {}",
                runtime.display(),
                actual.display(),
                expected.display()
            );
        }
    } else if runtime.exists() {
        let storage_empty = std::fs::read_dir(storage)
            .with_context(|| format!("reading persistent CODEX_HOME {}", storage.display()))?
            .next()
            .is_none();
        if !storage_empty {
            bail!(
                "both persistent CODEX_HOME locations contain data: {} and {}",
                storage.display(),
                runtime.display()
            );
        }
        std::fs::remove_dir(storage)?;
        symlink_dir(&runtime, storage).with_context(|| {
            format!(
                "linking persistent CODEX_HOME {} to {}",
                storage.display(),
                runtime.display()
            )
        })?;
    } else {
        std::fs::rename(storage, &runtime).with_context(|| {
            format!(
                "moving persistent CODEX_HOME {} to short path {}",
                storage.display(),
                runtime.display()
            )
        })?;
        symlink_dir(&runtime, storage).with_context(|| {
            format!(
                "linking persistent CODEX_HOME {} to {}",
                storage.display(),
                runtime.display()
            )
        })?;
    }
    Ok(runtime)
}

#[cfg(not(unix))]
fn persistent_runtime_path(storage: &Path, _needs_short_socket_path: bool) -> Result<PathBuf> {
    Ok(storage.to_path_buf())
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
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut push = |path: PathBuf, out: &mut Vec<PathBuf>| {
        if seen.insert(path.clone()) {
            out.push(path);
        }
    };
    for d in dirs {
        push((*d).to_path_buf(), &mut out);
    }
    for part in std::env::split_paths(std::ffi::OsStr::new(base)) {
        if !part.as_os_str().is_empty() {
            push(part, &mut out);
        }
    }
    std::env::join_paths(out)
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Refuse to drive a tool in a project that still holds an HS1 PGLite store. The
/// `.hotsheet/` directory itself is also used by HS2 for checkout-local metadata.
pub fn assert_no_hs1(project: &Path) -> Result<()> {
    if project.join(".hotsheet/db/PG_VERSION").is_file() {
        bail!(
            "refusing to drive a tool in {}: an HS1 store (.hotsheet/db/PG_VERSION) is present — \
             migrate or move it first (HS2-103 launch safety)",
            project.display()
        );
    }
    Ok(())
}

/// Apply the HS1 gate for a driven AI turn. A retained HS1 marker is safe only after
/// migration has durably recorded a receipt in the selected HS2 store for this exact
/// checkout. Receipt reads, parsing, and path resolution all fail closed.
pub fn assert_trigger_project_safe(project: &Path, store_path: &Path) -> Result<()> {
    if !project.join(".hotsheet/db/PG_VERSION").is_file()
        || hs1_import_receipt_matches(project, store_path)
    {
        return Ok(());
    }
    assert_no_hs1(project)
}

fn hs1_import_receipt_matches(project: &Path, store_path: &Path) -> bool {
    let Ok(contents) = std::fs::read(store_path.join(HS1_IMPORT_RECEIPT)) else {
        return false;
    };
    let Ok(receipt) = serde_json::from_slice::<Hs1ImportReceipt>(&contents) else {
        return false;
    };
    if receipt.schema != 1 || !receipt.source_project.is_absolute() {
        return false;
    }
    let (Ok(source_project), Ok(project)) = (
        receipt.source_project.canonicalize(),
        project.canonicalize(),
    ) else {
        return false;
    };
    source_project == project
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
    std::env::split_paths(std::ffi::OsStr::new(path))
        .find(|dir| executable_named(dir, name).is_some())
}

fn executable_named(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if is_executable_file(&exact) {
        return Some(exact);
    }
    #[cfg(windows)]
    for suffix in [".exe", ".cmd", ".bat", ".com"] {
        let candidate = dir.join(format!("{name}{suffix}"));
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
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
        .and_then(|dir| executable_named(&dir, program))
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
    fn resolves_cli_beside_server_or_above_a_cargo_test() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        let deps = bin.join("deps");
        std::fs::create_dir_all(&deps).unwrap();
        let cli = bin.join(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&cli, "cli").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let server = bin.join(format!("hotsheet-server{}", std::env::consts::EXE_SUFFIX));
        let test = deps.join(format!("http-test{}", std::env::consts::EXE_SUFFIX));
        assert_eq!(resolve_hotsheet_cli(&server, None).unwrap(), cli);
        assert_eq!(resolve_hotsheet_cli(&test, None).unwrap(), cli);
    }

    #[test]
    fn assert_no_hs1_flags_a_legacy_store() {
        let dir = tempfile::tempdir().unwrap();
        assert!(assert_no_hs1(dir.path()).is_ok());
        std::fs::create_dir_all(dir.path().join(".hotsheet/db")).unwrap();
        std::fs::write(dir.path().join(".hotsheet/db/PG_VERSION"), "17").unwrap();
        let err = assert_no_hs1(dir.path()).unwrap_err().to_string();
        assert!(err.contains("HS1 store"), "{err}");
    }

    #[test]
    fn trigger_allows_a_retained_hs1_store_only_for_its_completed_import() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let store = root.path().join("tickets.hs2");
        std::fs::create_dir_all(project.join(".hotsheet/db")).unwrap();
        std::fs::create_dir(&store).unwrap();
        std::fs::write(project.join(".hotsheet/db/PG_VERSION"), "17").unwrap();

        assert!(assert_trigger_project_safe(&project, &store).is_err());
        std::fs::write(
            store.join(HS1_IMPORT_RECEIPT),
            serde_json::json!({
                "$hotsheetSchema": 1,
                "sourceProject": project.canonicalize().unwrap(),
            })
            .to_string(),
        )
        .unwrap();

        assert!(assert_trigger_project_safe(&project, &store).is_ok());
        assert!(assert_no_hs1(&project).is_err());
    }

    #[test]
    fn trigger_rejects_unrelated_or_invalid_import_receipts() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let other = root.path().join("other");
        let store = root.path().join("tickets.hs2");
        std::fs::create_dir_all(project.join(".hotsheet/db")).unwrap();
        std::fs::create_dir(&other).unwrap();
        std::fs::create_dir(&store).unwrap();
        std::fs::write(project.join(".hotsheet/db/PG_VERSION"), "17").unwrap();

        for receipt in [
            serde_json::json!({
                "$hotsheetSchema": 1,
                "sourceProject": other.canonicalize().unwrap(),
            })
            .to_string(),
            r#"{"$hotsheetSchema":1,"sourceProject":"relative/project"}"#.to_string(),
            serde_json::json!({
                "$hotsheetSchema": 2,
                "sourceProject": project.canonicalize().unwrap(),
            })
            .to_string(),
            serde_json::json!({
                "sourceProject": project.canonicalize().unwrap(),
            })
            .to_string(),
            "not json".to_string(),
        ] {
            std::fs::write(store.join(HS1_IMPORT_RECEIPT), receipt).unwrap();
            let err = assert_trigger_project_safe(&project, &store)
                .unwrap_err()
                .to_string();
            assert!(err.contains("HS1 store"), "{err}");
        }
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

    #[test]
    fn persistent_codex_home_keeps_thread_storage_across_owner_drop() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let storage = root.path().join("durable");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("auth.json"), "{}").unwrap();
        let home = IsolatedCodexHome::create_persistent_for_daemon(
            &storage,
            &source,
            "hotsheet",
            "hotsheet-mcp",
            &[],
            None,
        )
        .unwrap();
        std::fs::write(home.path().join("thread-state.json"), "state").unwrap();
        drop(home);
        assert_eq!(
            std::fs::read_to_string(storage.join("thread-state.json")).unwrap(),
            "state"
        );
        assert!(storage.join("config.toml").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn persistent_daemon_home_uses_a_short_alias_to_durable_storage() {
        let root = tempfile::tempdir().unwrap();
        let storage = root
            .path()
            .join("drive/store/homes/a-very-long-machine-local-drive-home");
        std::fs::create_dir_all(&storage).unwrap();
        std::fs::write(storage.join("thread-state.json"), "state").unwrap();
        let runtime = persistent_runtime_path(&storage, true).unwrap();
        assert!(runtime.to_string_lossy().len() < storage.to_string_lossy().len());
        assert_eq!(
            runtime.canonicalize().unwrap(),
            storage.canonicalize().unwrap()
        );
        assert_eq!(
            std::fs::read_to_string(runtime.join("thread-state.json")).unwrap(),
            "state"
        );
        assert!(
            std::fs::symlink_metadata(&storage)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the historical long home remains as a compatibility alias"
        );
    }
    // `mcp_command` now lives in `hotsheet-plugins` (its resolver is tested there, HS2-91).
}
