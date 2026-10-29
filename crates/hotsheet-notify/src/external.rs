//! External notifier plugins (`docs/32-notifier-plugins.md` §32.6): a directory under
//! `${HOTSHEET_HOME}/notifiers/<id>/` with a `manifest.toml` that declares capabilities,
//! settings, and a command. Hot Sheet runs the command once per delivery, writes one
//! JSON envelope to its stdin, and reads one JSON result from its stdout. No shell is
//! involved; resolved secrets travel only on stdin.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::capability::{Capabilities, NotifierDescriptor, NotifierSource, SettingField};
use crate::notifier::{CommandRunner, DeliveryError, LiveRunner, Notifier, tail};
use crate::render::Rendered;
use crate::settings::Resolved;

/// The envelope protocol version Hot Sheet writes.
pub const PROTOCOL_VERSION: u32 = 1;
const MANIFEST: &str = "manifest.toml";

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    id: String,
    display_name: String,
    #[serde(default)]
    description: String,
    command: Vec<String>,
    #[serde(default = "default_timeout")]
    timeout_seconds: u64,
    capabilities: Capabilities,
    #[serde(default)]
    settings: Vec<SettingField>,
}

fn default_timeout() -> u64 {
    30
}

/// What a plugin prints on stdout.
#[derive(Debug, Deserialize, Serialize)]
pub struct PluginResult {
    pub ok: bool,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub retryable: bool,
}

pub struct ExternalNotifier {
    descriptor: NotifierDescriptor,
    dir: PathBuf,
    program: String,
    args: Vec<String>,
    timeout: Duration,
    runner: Arc<dyn CommandRunner>,
}

/// Notifier ids are short lowercase slugs so they are safe in paths and key names.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

impl ExternalNotifier {
    pub fn load(dir: &Path) -> Result<ExternalNotifier, String> {
        ExternalNotifier::load_with_runner(dir, Arc::new(LiveRunner))
    }

    pub fn load_with_runner(
        dir: &Path,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<ExternalNotifier, String> {
        let path = dir.join(MANIFEST);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let manifest: Manifest =
            toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if !valid_id(&manifest.id) {
            return Err(format!(
                "{}: id '{}' must be lowercase letters, digits, '-' or '_'",
                path.display(),
                manifest.id
            ));
        }
        let (program, args) = manifest
            .command
            .split_first()
            .ok_or_else(|| format!("{}: command must name a program", path.display()))?;
        // A relative program that exists in the plugin directory runs from there; a bare
        // name otherwise resolves on PATH (for example `node`).
        let local = dir.join(program);
        let program = if Path::new(program).is_relative() && local.is_file() {
            local.display().to_string()
        } else {
            program.clone()
        };
        let args = args
            .iter()
            .map(|arg| {
                let candidate = dir.join(arg);
                if Path::new(arg).is_relative() && arg.contains('/') && candidate.is_file() {
                    candidate.display().to_string()
                } else {
                    arg.clone()
                }
            })
            .collect();
        Ok(ExternalNotifier {
            dir: dir.to_path_buf(),
            descriptor: NotifierDescriptor {
                id: manifest.id,
                display_name: manifest.display_name,
                description: manifest.description,
                capabilities: manifest.capabilities,
                settings: manifest.settings,
                source: NotifierSource::External {
                    dir: dir.display().to_string(),
                },
            },
            program,
            args,
            timeout: Duration::from_secs(manifest.timeout_seconds.clamp(1, 300)),
            runner,
        })
    }
}

/// Plugin directories larger than this are refused rather than hashed.
const DIGEST_MAX_FILES: usize = 2000;
const DIGEST_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// SHA-256 over every file in a plugin directory: sorted relative paths, each with its
/// length and bytes; a symlink contributes its target text. Any added, removed, renamed,
/// or edited file changes it, so a channel's approval covers exactly the code it saw
/// (HS2-208RB1).
pub fn plugin_digest(dir: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut entries = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let read =
            std::fs::read_dir(&current).map_err(|e| format!("{}: {e}", current.display()))?;
        for entry in read {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                pending.push(path);
            } else {
                entries.push((path, kind.is_symlink()));
            }
            if entries.len() > DIGEST_MAX_FILES {
                return Err(format!(
                    "{} has more than {DIGEST_MAX_FILES} files",
                    dir.display()
                ));
            }
        }
    }
    entries.sort();
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    for (path, symlink) in entries {
        let relative = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = if symlink {
            std::fs::read_link(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .to_string_lossy()
                .into_owned()
                .into_bytes()
        } else {
            std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?
        };
        total += bytes.len() as u64;
        if total > DIGEST_MAX_BYTES {
            return Err(format!(
                "{} is larger than {DIGEST_MAX_BYTES} bytes",
                dir.display()
            ));
        }
        hasher.update(relative.as_bytes());
        hasher.update([0, u8::from(symlink)]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Every plugin directory under `root`, plus a message per directory that failed to load.
pub fn load_all(
    root: &Path,
    runner: Arc<dyn CommandRunner>,
) -> (Vec<ExternalNotifier>, Vec<String>) {
    let mut loaded = Vec::new();
    let mut errors = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return (loaded, errors);
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join(MANIFEST).is_file())
        .collect();
    dirs.sort();
    for dir in dirs {
        match ExternalNotifier::load_with_runner(&dir, runner.clone()) {
            Ok(notifier) => loaded.push(notifier),
            Err(error) => errors.push(error),
        }
    }
    (loaded, errors)
}

/// The JSON written to a plugin's stdin.
pub fn envelope(
    notifier: &str,
    channel_id: &str,
    rendered: &Rendered,
    settings: &Resolved,
) -> Value {
    json!({
        "protocol": PROTOCOL_VERSION,
        "notifier": notifier,
        "channel": { "id": channel_id },
        "settings": settings.0,
        "message": rendered,
    })
}

impl Notifier for ExternalNotifier {
    fn descriptor(&self) -> &NotifierDescriptor {
        &self.descriptor
    }

    fn content_digest(&self) -> Option<Result<String, String>> {
        Some(plugin_digest(&self.dir))
    }

    fn deliver(
        &self,
        channel: &str,
        rendered: &Rendered,
        settings: &Resolved,
    ) -> Result<Option<String>, DeliveryError> {
        let input = envelope(&self.descriptor.id, channel, rendered, settings).to_string();
        let output = self.runner.run(
            &self.program,
            &self.args,
            Some(input.as_bytes()),
            self.timeout,
        )?;
        let parsed: Option<PluginResult> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .rev()
            .find_map(|line| serde_json::from_str(line.trim()).ok());
        match parsed {
            Some(result) if result.ok && output.success => Ok(result.id),
            Some(result) => {
                let message = result
                    .error
                    .unwrap_or_else(|| "the plugin reported a failure".into());
                let message = format!("{}: {message}", self.descriptor.id);
                Err(if result.retryable {
                    DeliveryError::retryable(message)
                } else {
                    DeliveryError::permanent(message)
                })
            }
            // No result line: an unexpected crash, which may be transient.
            None => Err(DeliveryError::retryable(format!(
                "{} exited with {} without a result: {}",
                self.descriptor.id,
                output.code.map_or("a signal".into(), |c| c.to_string()),
                tail(&output.stderr, 300)
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{EventKind, Message};
    use crate::notifier::ProcessOutput;
    use crate::render::render;
    use crate::testing::RecordingRunner;

    const MANIFEST_TEXT: &str = r#"
id = "webhook"
display_name = "Webhook"
command = ["send.sh", "--mode", "conf/x.json"]
timeout_seconds = 5

[capabilities]
formats = ["markdown", "plain"]
max_title_bytes = 100
max_body_bytes = 2000

[capabilities.attachments]
max_count = 1
max_bytes = 1000

[[settings]]
key = "url"
label = "URL"
type = "secret"
required = true

[[settings]]
key = "style"
label = "Style"
type = "choice"
choices = ["compact", "full"]
"#;

    fn plugin_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MANIFEST), MANIFEST_TEXT).unwrap();
        std::fs::write(dir.path().join("send.sh"), "#!/bin/sh\n").unwrap();
        std::fs::create_dir(dir.path().join("conf")).unwrap();
        std::fs::write(dir.path().join("conf/x.json"), "{}").unwrap();
        dir
    }

    fn output(stdout: &str, success: bool) -> ProcessOutput {
        ProcessOutput {
            success,
            code: Some(if success { 0 } else { 2 }),
            stdout: stdout.as_bytes().to_vec(),
            stderr: b"boom".to_vec(),
        }
    }

    fn rendered(n: &ExternalNotifier) -> Rendered {
        render(
            &Message::new(EventKind::Test, "Test"),
            &n.descriptor().capabilities,
        )
    }

    #[test]
    fn loads_manifest_and_resolves_local_paths() {
        let dir = plugin_dir();
        let runner = Arc::new(RecordingRunner::with_output(output(
            r#"{"ok":true,"id":"m1"}"#,
            true,
        )));
        let n = ExternalNotifier::load_with_runner(dir.path(), runner.clone()).unwrap();
        assert_eq!(n.descriptor().id, "webhook");
        assert_eq!(n.descriptor().settings.len(), 2);
        assert_eq!(n.descriptor().capabilities.attachments.max_count, 1);
        let mut settings = Resolved::default();
        settings
            .0
            .insert("url".into(), json!("https://hooks.test/x"));
        assert_eq!(
            n.deliver("team", &rendered(&n), &settings)
                .unwrap()
                .as_deref(),
            Some("m1")
        );
        let call = runner.calls().remove(0);
        assert_eq!(
            call.program,
            dir.path().join("send.sh").display().to_string()
        );
        assert_eq!(call.args[0], "--mode");
        assert_eq!(
            call.args[1],
            dir.path().join("conf/x.json").display().to_string()
        );
        let envelope: Value = serde_json::from_slice(&call.stdin.unwrap()).unwrap();
        assert_eq!(envelope["protocol"], 1);
        assert_eq!(envelope["channel"]["id"], "team");
        assert_eq!(envelope["settings"]["url"], "https://hooks.test/x");
        assert_eq!(envelope["message"]["title"], "Test");
    }

    #[test]
    fn maps_plugin_results_to_delivery_errors() {
        let dir = plugin_dir();
        let load = |out| {
            ExternalNotifier::load_with_runner(
                dir.path(),
                Arc::new(RecordingRunner::with_output(out)),
            )
            .unwrap()
        };
        let n = load(output(
            r#"{"ok":false,"error":"bad url","retryable":false}"#,
            false,
        ));
        let err = n
            .deliver("c", &rendered(&n), &Resolved::default())
            .unwrap_err();
        assert!(!err.retryable && err.message == "webhook: bad url");
        let n = load(output(
            "log line\n{\"ok\":false,\"error\":\"rate limited\",\"retryable\":true}\n",
            true,
        ));
        assert!(
            n.deliver("c", &rendered(&n), &Resolved::default())
                .unwrap_err()
                .retryable
        );
        let n = load(output("", false));
        let err = n
            .deliver("c", &rendered(&n), &Resolved::default())
            .unwrap_err();
        assert!(err.retryable && err.message.contains("boom"), "{err}");
    }

    #[test]
    fn hs2_208rb1_digest_tracks_every_file_and_is_stable() {
        let dir = plugin_dir();
        let first = plugin_digest(dir.path()).unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(plugin_digest(dir.path()).unwrap(), first, "stable");
        let n =
            ExternalNotifier::load_with_runner(dir.path(), Arc::new(RecordingRunner::succeeding()))
                .unwrap();
        assert_eq!(n.content_digest().unwrap().unwrap(), first);
        std::fs::write(dir.path().join("send.sh"), "#!/bin/sh\necho changed\n").unwrap();
        let edited = plugin_digest(dir.path()).unwrap();
        assert_ne!(edited, first, "an edited file changes the digest");
        std::fs::write(dir.path().join("conf/extra.js"), "x").unwrap();
        let added = plugin_digest(dir.path()).unwrap();
        assert_ne!(added, edited, "a new nested file changes the digest");
        std::fs::rename(
            dir.path().join("conf/extra.js"),
            dir.path().join("conf/other.js"),
        )
        .unwrap();
        assert_ne!(
            plugin_digest(dir.path()).unwrap(),
            added,
            "a rename changes the digest"
        );
        assert!(plugin_digest(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn load_all_reports_invalid_plugins_without_hiding_valid_ones() {
        let root = tempfile::tempdir().unwrap();
        let good = root.path().join("good");
        std::fs::create_dir(&good).unwrap();
        std::fs::write(good.join(MANIFEST), MANIFEST_TEXT).unwrap();
        let bad = root.path().join("bad");
        std::fs::create_dir(&bad).unwrap();
        std::fs::write(
            bad.join(MANIFEST),
            MANIFEST_TEXT.replace("\"webhook\"", "\"Bad Id\""),
        )
        .unwrap();
        let (loaded, errors) = load_all(root.path(), Arc::new(RecordingRunner::succeeding()));
        assert_eq!(loaded.len(), 1);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Bad Id"));
        let (none, errors) = load_all(
            &root.path().join("missing"),
            Arc::new(RecordingRunner::succeeding()),
        );
        assert!(none.is_empty() && errors.is_empty());
    }
}
