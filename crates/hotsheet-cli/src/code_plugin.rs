//! `hotsheet-cli plugin` for code-bearing plugins (docs/38, HS2-XY0VTN): the headless
//! path to install, trust, verify, list and remove plugins that run as separate
//! executables. They share `<HOTSHEET_HOME>/plugins/<id>/` with AI-tool data plugins; a
//! manifest with an `[executable]` table is code-bearing.
//!
//! Installing is the consent step: the disclosure shows the executable, its digest, its
//! capabilities and the secrets it asks for, and confirming records the approval. Any
//! later change to the plugin's files needs `plugin trust <id>` again.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hotsheet_plugin_host::manifest::{MANIFEST_FILE, SettingKind};
use hotsheet_plugin_host::{CodePluginManifest, HostConfig, PluginHost, TrustError, TrustStore};
use hotsheet_plugin_protocol::diagnostics::{self, method};
use hotsheet_plugin_protocol::{Transport, code, v1};

/// A code-bearing plugin directory under the machine plugin root.
pub struct Installed {
    pub dir: PathBuf,
    pub manifest: Result<CodePluginManifest, String>,
}

/// Every code-bearing plugin under `root`, sorted by directory.
pub fn installed(root: &Path) -> Vec<Installed> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<Installed> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| hotsheet_plugins::is_code_bearing_dir(dir))
        .map(|dir| Installed {
            manifest: CodePluginManifest::load(&dir),
            dir,
        })
        .collect();
    found.sort_by(|a, b| a.dir.cmp(&b.dir));
    found
}

/// The installed code-bearing plugin `id`, if any.
pub fn find(root: &Path, id: &str) -> Option<(PathBuf, CodePluginManifest)> {
    if !hotsheet_plugin_host::manifest::is_valid_plugin_id(id) {
        return None;
    }
    let dir = root.join(id);
    if !hotsheet_plugins::is_code_bearing_dir(&dir) {
        return None;
    }
    CodePluginManifest::load(&dir)
        .ok()
        .map(|manifest| (dir, manifest))
}

/// `trusted`, `not trusted`, `changed since trusted`, or why trust could not be read.
pub fn trust_state(store: &TrustStore, id: &str, dir: &Path) -> String {
    match store.check(id, dir) {
        Ok(()) => "trusted".to_string(),
        Err(TrustError::NotApproved { .. }) => "not trusted".to_string(),
        Err(TrustError::Changed { .. }) => "changed since trusted".to_string(),
        Err(TrustError::Unreadable(why)) => format!("unknown ({why})"),
    }
}

/// The trust disclosure shown by `info`, `install` and `trust`.
pub fn describe(manifest: &CodePluginManifest, dir: &Path, trust: Option<&str>) -> String {
    use std::fmt::Write;
    let program = manifest
        .resolve_program(dir)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|e| format!("! {e}"));
    let digest = TrustStore::digest(dir).unwrap_or_else(|e| format!("! {e}"));
    let settings: Vec<String> = manifest
        .settings
        .iter()
        .map(|s| {
            let kind = match s.kind {
                SettingKind::String => "text",
                SettingKind::Secret => "secret",
                SettingKind::Choice => "choice",
            };
            let required = if s.required { ", required" } else { "" };
            format!("{} ({kind}{required})", s.key)
        })
        .collect();
    let mut s = String::new();
    let _ = writeln!(s, "  id:           {}", manifest.id);
    let _ = writeln!(
        s,
        "  name:         {} {}",
        manifest.display_name, manifest.version
    );
    let _ = writeln!(
        s,
        "  kind:         code-bearing plugin (runs as a separate program)"
    );
    let _ = writeln!(s, "  provenance:   third-party (unsigned)");
    let _ = writeln!(s, "  capabilities: {}", manifest.capabilities.join(", "));
    let _ = writeln!(
        s,
        "  runs:         {program} {}",
        manifest.executable.args.join(" ")
    );
    let _ = writeln!(
        s,
        "  settings:     {}",
        if settings.is_empty() {
            "(none)".to_string()
        } else {
            settings.join(", ")
        }
    );
    let _ = writeln!(s, "  digest:       {digest}");
    if let Some(trust) = trust {
        let _ = writeln!(s, "  trust:        {trust}");
    }
    let _ = write!(
        s,
        "  note:         it runs with your user's privileges and is not sandboxed (HS2-SA5KGZ)"
    );
    s
}

/// Reads and validates the manifest of a source directory whose name may differ from
/// the plugin id.
pub fn read_source(src: &Path) -> Result<CodePluginManifest> {
    let text = std::fs::read_to_string(src.join(MANIFEST_FILE))
        .with_context(|| format!("reading {}", src.join(MANIFEST_FILE).display()))?;
    let id = text
        .parse::<toml::Table>()
        .ok()
        .and_then(|t| t.get("id").and_then(|v| v.as_str()).map(str::to_string))
        .context("the manifest has no id")?;
    CodePluginManifest::parse(&text, &id).map_err(anyhow::Error::msg)
}

/// Copies `src` into `dest_root/<id>` (replacing an earlier install of the same plugin)
/// and records its approval. The caller has shown the disclosure and has consent.
pub fn install(src: &Path, dest_root: &Path, store: &TrustStore) -> Result<String> {
    let manifest = read_source(src)?;
    let id = manifest.id.clone();
    if hotsheet_plugins::builtin_plugins()
        .iter()
        .any(|p| p.id() == id)
    {
        bail!("'{id}' is the id of a built-in plugin");
    }
    let dest = dest_root.join(&id);
    if dest.exists() && !hotsheet_plugins::is_code_bearing_dir(&dest) {
        bail!(
            "an AI-tool plugin '{id}' is already installed at {}; remove it first",
            dest.display()
        );
    }
    manifest
        .resolve_program(src)
        .map_err(anyhow::Error::msg)
        .context("the plugin cannot run here")?;
    // Bounded like approval itself: too many files or bytes is refused before copying.
    TrustStore::digest(src).map_err(|e| anyhow::anyhow!("{e}"))?;
    reject_links(src)?;

    std::fs::create_dir_all(dest_root)
        .with_context(|| format!("creating {}", dest_root.display()))?;
    let staging = dest_root.join(format!(".install-{id}"));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .with_context(|| format!("clearing {}", staging.display()))?;
    }
    copy_tree(src, &staging)?;
    // The copy must load and resolve exactly as the source did.
    CodePluginManifest::parse(&std::fs::read_to_string(staging.join(MANIFEST_FILE))?, &id)
        .map_err(anyhow::Error::msg)?
        .resolve_program(&staging)
        .map_err(anyhow::Error::msg)
        .context("the installed copy cannot run")?;
    if dest.exists() {
        std::fs::remove_dir_all(&dest).with_context(|| format!("replacing {}", dest.display()))?;
    }
    std::fs::rename(&staging, &dest)
        .with_context(|| format!("installing into {}", dest.display()))?;
    store
        .approve(&id, &dest)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(id)
}

fn reject_links(dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            bail!(
                "{} is a symbolic link; a plugin directory must hold real files",
                entry.path().display()
            );
        }
        if kind.is_dir() {
            reject_links(&entry.path())?;
        }
    }
    Ok(())
}

/// Copies a directory tree. `std::fs::copy` keeps each file's permissions, so an
/// executable stays executable.
fn copy_tree(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
    for entry in std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("copying {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// The outcome of a live verification.
#[derive(Default)]
pub struct Verification {
    /// Problems that fail verification.
    pub issues: Vec<String>,
    /// Facts worth showing (transports spoken, optional ones missing).
    pub notes: Vec<String>,
}

/// Starts the trusted plugin once per transport and checks it answers. JSON is required;
/// protobuf is optional. A plugin that implements `diagnostics` also gets an echo and a
/// stream round trip.
pub fn verify_live(home: &Path, dir: &Path, manifest: &CodePluginManifest) -> Verification {
    let mut out = Verification::default();
    if let Err(e) = manifest.resolve_program(dir) {
        out.issues.push(e);
        return out;
    }
    let has_diagnostics = manifest
        .capabilities
        .iter()
        .any(|c| c == diagnostics::CAPABILITY);
    for transport in [Transport::Json, Transport::Protobuf] {
        let mut config = HostConfig::new(
            home.to_path_buf(),
            "hotsheet-cli",
            env!("CARGO_PKG_VERSION"),
        );
        config.transports = vec![transport];
        config.default_deadline = Duration::from_secs(10);
        let host = PluginHost::new(config);
        if let Err(e) = host.register(dir) {
            out.issues.push(e);
            return out;
        }
        match check_transport(&host, &manifest.id, has_diagnostics) {
            Ok(()) => out.notes.push(format!("speaks {}", transport.name())),
            Err(e) if transport == Transport::Json => {
                out.issues.push(format!("over json (required): {e}"));
            }
            Err(e) => out
                .notes
                .push(format!("does not speak protobuf (optional): {e}")),
        }
        host.shutdown_all();
    }
    out
}

fn check_transport(host: &Arc<PluginHost>, id: &str, has_diagnostics: bool) -> Result<(), String> {
    let echo = host.call::<_, v1::EchoResponse>(
        id,
        method::ECHO,
        &v1::EchoRequest {
            text: "hotsheet verify".into(),
            data: (0..=255).collect(),
        },
        None,
    );
    if !has_diagnostics {
        // Any well-formed answer, including `unsupported`, proves the transport works.
        return match echo {
            Ok(_) => Ok(()),
            Err(e) if e.code == code::UNSUPPORTED => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    }
    let echo = echo.map_err(|e| e.to_string())?;
    if echo.text != "hotsheet verify" || echo.data != (0..=255).collect::<Vec<u8>>() {
        return Err("diagnostics.Echo changed the payload".to_string());
    }
    let stream = host
        .stream::<_, v1::CountItem, v1::CountResponse>(
            id,
            method::COUNT,
            &v1::CountRequest { count: 100 },
            Some(Duration::from_secs(10)),
        )
        .map_err(|e| e.to_string())?;
    let mut stream = stream;
    let seqs: Result<Vec<u64>, _> = stream.by_ref().map(|item| item.map(|i| i.seq)).collect();
    let seqs = seqs.map_err(|e| e.to_string())?;
    let done = stream.finish().map_err(|e| e.to_string())?;
    if seqs != (0..100).collect::<Vec<_>>() || done.sent != 100 {
        return Err("diagnostics.Count streamed the wrong items".to_string());
    }
    Ok(())
}
