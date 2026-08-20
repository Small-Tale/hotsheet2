//! Project settings — **core-owned**, split into two on-disk scopes (`docs/04` §4.7):
//!
//! - **Shared** (`hotsheet-settings.json`): committed in the store repo, travels with
//!   the project (categories, auto-context guidance, custom views, enabled plugins…).
//! - **Local** (`hotsheet-settings.local.json`): machine-local, **gitignored** (tools
//!   enabled on this machine, machine paths…). `set --scope local` also adds the file
//!   to the store's `.gitignore` so it never gets committed.
//!
//! Each scope is a flat `key -> JSON value` map. The **effective** value of a key is
//! the local one if present, else the shared one. Device/app-only settings (window
//! geometry, theme) are the client's concern and never live here.

use std::path::PathBuf;

use serde_json::{Map, Value};

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
}

/// Which settings file a read/write targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Committed, travels with the project.
    Shared,
    /// Machine-local, gitignored.
    Local,
}

impl Scope {
    /// The file name for this scope, relative to the store root.
    pub fn file_name(self) -> &'static str {
        match self {
            Scope::Shared => "hotsheet-settings.json",
            Scope::Local => "hotsheet-settings.local.json",
        }
    }
}

/// Read/write the project settings beside a store root.
pub struct Settings {
    root: PathBuf,
}

impl Settings {
    /// Settings stored beside `root` (the store directory).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path(&self, scope: Scope) -> PathBuf {
        self.root.join(scope.file_name())
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
                Ok(value.as_object().cloned().unwrap_or_default())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// The effective map: shared overlaid by local.
    pub fn effective(&self) -> Result<Map<String, Value>, SettingsError> {
        let mut merged = self.map(Scope::Shared)?;
        for (k, v) in self.map(Scope::Local)? {
            merged.insert(k, v);
        }
        Ok(merged)
    }

    /// One key from a specific scope.
    pub fn get(&self, key: &str, scope: Scope) -> Result<Option<Value>, SettingsError> {
        Ok(self.map(scope)?.get(key).cloned())
    }

    /// One key's effective value (local wins over shared).
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
        let text = serde_json::to_string_pretty(&Value::Object(map.clone()))
            .unwrap_or_else(|_| "{}".to_string());
        std::fs::write(self.path(scope), text + "\n")?;
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
}
