//! Project settings — **core-owned**, split into three on-disk scopes (`docs/04` §4.7,
//! HS2-34):
//!
//! - **Global** (`${HOTSHEET_HOME}/settings.json`): machine-wide, **not** tied to a
//!   store — a person's cross-project defaults (default AI tool, editor…) they set once.
//! - **Shared** (`hotsheet-settings.json`): committed in the store repo, travels with
//!   the project (categories, auto-context guidance, custom views, enabled plugins…).
//! - **Local** (`hotsheet-settings.local.json`): machine-local, **gitignored** (tools
//!   enabled on this machine, machine paths…). `set --scope local` also adds the file
//!   to the store's `.gitignore` so it never gets committed.
//!
//! Each scope is a flat `key -> JSON value` map. The **effective** value of a key is
//! resolved in precedence order **Global < Shared < Local** — the most specific wins.
//! Device/app-only settings (window geometry, theme) are the client's concern and never
//! live here.

use std::path::PathBuf;

use serde_json::{Map, Value};

const SETTINGS_SCHEMA_KEY: &str = "$hotsheetSchema";
const SETTINGS_SCHEMA_VERSION: u64 = 1;

/// A settings I/O or parse failure.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("parsing {path}: {source}")]
    Parse {
        path: String,
        source: serde_json::Error,
    },
    #[error("invalid setting {key}: {source}")]
    Invalid {
        key: String,
        source: serde_json::Error,
    },
    #[error(
        "This {scope} settings file was created by a newer version of Hot Sheet 2 and cannot be opened by this version. Update Hot Sheet 2 to open it (found schema {found}, supported through {supported})."
    )]
    UpgradeRequired {
        scope: &'static str,
        found: String,
        supported: u64,
    },
}

/// Which settings file a read/write targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Machine-wide, store-independent (`${HOTSHEET_HOME}/settings.json`).
    Global,
    /// Committed, travels with the project.
    Shared,
    /// Machine-local, gitignored.
    Local,
}

impl Scope {
    /// The file name for this scope. For `Global` this is just the basename; its file
    /// lives under `${HOTSHEET_HOME}`, not the store root (see [`Settings::path`]).
    pub fn file_name(self) -> &'static str {
        match self {
            Scope::Global => "settings.json",
            Scope::Shared => "hotsheet-settings.json",
            Scope::Local => "hotsheet-settings.local.json",
        }
    }
}

/// The machine-wide Hot Sheet 2 home (`${HOTSHEET_HOME}`, else `~/.hotsheet2`). Kept in
/// sync with `hotsheet_plugins::hotsheet_home` but resolved here so `ticketing` stays free
/// of a dependency on the plugin crate (`docs/12` §12.2.1). NOT `~/.hotsheet` (HS1's).
fn hotsheet_home() -> PathBuf {
    if let Some(h) = std::env::var_os("HOTSHEET_HOME") {
        return PathBuf::from(h);
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".hotsheet2")
}

/// Read/write the project settings beside a store root.
pub struct Settings {
    root: PathBuf,
    global_home: Option<PathBuf>,
}

impl Settings {
    /// Settings stored beside `root` (the store directory).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            global_home: None,
        }
    }

    /// Settings with an explicitly injected machine-wide home. Intended for
    /// isolated hosts and tests that must not mutate process-global environment.
    pub fn with_global_home(root: impl Into<PathBuf>, global_home: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            global_home: Some(global_home.into()),
        }
    }

    fn path(&self, scope: Scope) -> PathBuf {
        match scope {
            // Global lives under the machine home, independent of any store.
            Scope::Global => self
                .global_home
                .clone()
                .unwrap_or_else(hotsheet_home)
                .join(Scope::Global.file_name()),
            _ => self.root.join(scope.file_name()),
        }
    }

    /// The raw map for one scope (empty if the file doesn't exist yet).
    pub fn map(&self, scope: Scope) -> Result<Map<String, Value>, SettingsError> {
        let path = self.path(scope);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let value: Value =
                    serde_json::from_str(&text).map_err(|source| SettingsError::Parse {
                        path: path.display().to_string(),
                        source,
                    })?;
                let mut map = value.as_object().cloned().unwrap_or_default();
                if let Some(version) = map.remove(SETTINGS_SCHEMA_KEY) {
                    if version.as_u64().is_none_or(|v| v > SETTINGS_SCHEMA_VERSION) {
                        return Err(SettingsError::UpgradeRequired {
                            scope: scope.label(),
                            found: version.to_string(),
                            supported: SETTINGS_SCHEMA_VERSION,
                        });
                    }
                }
                Ok(map)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// The effective map, in precedence order **Global < Shared < Local** (most specific
    /// wins): machine-wide defaults, overlaid by the project's committed settings, overlaid
    /// by this machine's local overrides.
    pub fn effective(&self) -> Result<Map<String, Value>, SettingsError> {
        let mut merged = self.map(Scope::Global)?;
        for (k, v) in self.map(Scope::Shared)? {
            merged.insert(k, v);
        }
        for (k, v) in self.map(Scope::Local)? {
            merged.insert(k, v);
        }
        Ok(merged)
    }

    /// One key from a specific scope.
    pub fn get(&self, key: &str, scope: Scope) -> Result<Option<Value>, SettingsError> {
        Ok(self.map(scope)?.get(key).cloned())
    }

    /// One key's effective value (precedence Global < Shared < Local).
    pub fn get_effective(&self, key: &str) -> Result<Option<Value>, SettingsError> {
        Ok(self.effective()?.get(key).cloned())
    }

    /// Set a key in a scope (read-modify-write). Writing a local key also ensures the
    /// local file is gitignored.
    pub fn set(&self, key: &str, value: Value, scope: Scope) -> Result<(), SettingsError> {
        let mut map = self.map(scope)?;
        map.insert(key.to_string(), value);
        self.write(scope, &map)
    }

    /// Remove a key from a scope; returns whether it was present.
    pub fn unset(&self, key: &str, scope: Scope) -> Result<bool, SettingsError> {
        let mut map = self.map(scope)?;
        let existed = map.remove(key).is_some();
        if existed {
            self.write(scope, &map)?;
        }
        Ok(existed)
    }

    fn write(&self, scope: Scope, map: &Map<String, Value>) -> Result<(), SettingsError> {
        let mut persisted = map.clone();
        persisted.insert(SETTINGS_SCHEMA_KEY.into(), SETTINGS_SCHEMA_VERSION.into());
        let text = serde_json::to_string_pretty(&Value::Object(persisted))
            .unwrap_or_else(|_| "{}".to_string());
        let path = self.path(scope);
        // Global lives under ${HOTSHEET_HOME}, which may not exist yet.
        if scope == Scope::Global {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(&path, text + "\n")?;
        if scope == Scope::Local {
            self.ensure_gitignored(Scope::Local.file_name())?;
        }
        Ok(())
    }

    /// Ensure `name` is listed in the store's `.gitignore` (create/append as needed),
    /// so a local settings file is never committed.
    fn ensure_gitignored(&self, name: &str) -> Result<(), SettingsError> {
        let gi = self.root.join(".gitignore");
        let existing = std::fs::read_to_string(&gi).unwrap_or_default();
        if existing.lines().any(|l| l.trim() == name) {
            return Ok(());
        }
        let mut out = existing;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(name);
        out.push('\n');
        std::fs::write(&gi, out)?;
        Ok(())
    }
}

impl Scope {
    fn label(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Shared => "shared",
            Self::Local => "local",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn set_get_and_effective_override() {
        let d = root();
        let s = Settings::new(d.path());

        s.set("categories", json!(["bug", "task"]), Scope::Shared)
            .unwrap();
        s.set("theme_hint", json!("dark"), Scope::Shared).unwrap();
        // local overrides shared for the same key
        s.set("theme_hint", json!("light"), Scope::Local).unwrap();
        s.set("index_path", json!("/tmp/idx"), Scope::Local)
            .unwrap();

        assert_eq!(
            s.get("theme_hint", Scope::Shared).unwrap(),
            Some(json!("dark"))
        );
        assert_eq!(
            s.get("theme_hint", Scope::Local).unwrap(),
            Some(json!("light"))
        );
        assert_eq!(s.get_effective("theme_hint").unwrap(), Some(json!("light")));
        assert_eq!(
            s.get_effective("categories").unwrap(),
            Some(json!(["bug", "task"]))
        );
        assert_eq!(
            s.get_effective("index_path").unwrap(),
            Some(json!("/tmp/idx"))
        );
        assert_eq!(s.get_effective("nope").unwrap(), None);

        let eff = s.effective().unwrap();
        assert_eq!(eff.len(), 3); // categories, theme_hint (local), index_path
    }

    #[test]
    fn shared_is_a_committed_file_local_is_gitignored() {
        let d = root();
        let s = Settings::new(d.path());
        s.set("categories", json!(["bug"]), Scope::Shared).unwrap();
        s.set("index_path", json!("/tmp/idx"), Scope::Local)
            .unwrap();

        // shared file exists and is NOT in .gitignore
        assert!(d.path().join("hotsheet-settings.json").is_file());
        // local file exists AND is gitignored
        assert!(d.path().join("hotsheet-settings.local.json").is_file());
        let gi = std::fs::read_to_string(d.path().join(".gitignore")).unwrap();
        assert!(gi.lines().any(|l| l == "hotsheet-settings.local.json"));
        assert!(!gi.contains("hotsheet-settings.json\n") || gi.contains("local"));
    }

    #[test]
    fn gitignore_is_not_duplicated() {
        let d = root();
        std::fs::write(
            d.path().join(".gitignore"),
            "target/\nhotsheet-settings.local.json\n",
        )
        .unwrap();
        let s = Settings::new(d.path());
        s.set("a", json!(1), Scope::Local).unwrap();
        s.set("b", json!(2), Scope::Local).unwrap();
        let gi = std::fs::read_to_string(d.path().join(".gitignore")).unwrap();
        assert_eq!(gi.matches("hotsheet-settings.local.json").count(), 1);
        assert!(gi.contains("target/"), "existing entries preserved");
    }

    #[test]
    fn unset_removes_a_key() {
        let d = root();
        let s = Settings::new(d.path());
        s.set("a", json!(1), Scope::Shared).unwrap();
        assert!(s.unset("a", Scope::Shared).unwrap());
        assert_eq!(s.get("a", Scope::Shared).unwrap(), None);
        assert!(
            !s.unset("a", Scope::Shared).unwrap(),
            "second unset is a no-op"
        );
    }

    #[test]
    fn missing_files_read_as_empty() {
        let d = root();
        let s = Settings::new(d.path());
        assert!(s.map(Scope::Shared).unwrap().is_empty());
        assert!(s.effective().unwrap().is_empty());
        assert_eq!(s.get_effective("x").unwrap(), None);
    }

    #[test]
    fn legacy_unversioned_settings_remain_readable_and_future_settings_require_upgrade() {
        let d = root();
        std::fs::write(
            d.path().join("hotsheet-settings.json"),
            r#"{"theme":"dark"}"#,
        )
        .unwrap();
        let s = Settings::new(d.path());
        assert_eq!(s.get("theme", Scope::Shared).unwrap(), Some(json!("dark")));

        std::fs::write(
            d.path().join("hotsheet-settings.json"),
            r#"{"$hotsheetSchema":99,"theme":"dark"}"#,
        )
        .unwrap();
        let error = s.map(Scope::Shared).unwrap_err().to_string();
        assert!(error.contains("newer version of Hot Sheet 2"));
        assert!(error.contains("Update Hot Sheet 2"));
    }

    #[test]
    fn global_layer_is_machine_wide_and_lowest_precedence() {
        let home = root();
        let d = root();
        let s = Settings::with_global_home(d.path(), home.path());

        // A global default + a project override of the same key.
        s.set("default_tool", json!("claude"), Scope::Global)
            .unwrap();
        s.set("editor", json!("vim"), Scope::Global).unwrap();
        s.set("default_tool", json!("codex"), Scope::Shared)
            .unwrap();

        // Global lives under ${HOTSHEET_HOME}, NOT the store — the store dir has no
        // settings.json.
        assert!(home.path().join("settings.json").is_file());
        assert!(!d.path().join("settings.json").exists());

        // Precedence Global < Shared < Local: shared wins the shared key, global fills
        // the one only it sets.
        assert_eq!(
            s.get_effective("default_tool").unwrap(),
            Some(json!("codex"))
        );
        assert_eq!(s.get_effective("editor").unwrap(), Some(json!("vim")));

        // Local still beats both.
        s.set("default_tool", json!("gemini"), Scope::Local)
            .unwrap();
        assert_eq!(
            s.get_effective("default_tool").unwrap(),
            Some(json!("gemini"))
        );
    }
}
