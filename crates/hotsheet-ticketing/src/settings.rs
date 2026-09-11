//! Project settings — **core-owned**, split into three on-disk scopes (`docs/04` §4.7,
//! HS2-34):
//!
//! - **Global** (`${HOTSHEET_HOME}/settings.json`): machine-wide, **not** tied to a
//!   store — a person's cross-project defaults (default AI tool, editor…) they set once.
//! - **Shared** (`<project>/.hotsheet/settings.json`): committed in the code project,
//!   independent of how many ticket sources that project uses.
//! - **Local** (`<project>/.hotsheet/settings.local.json`): machine-local,
//!   **gitignored**. `set --scope local` adds the path to the project's `.gitignore`.
//!
//! Each scope is a flat `key -> JSON value` map. The **effective** value of a key is
//! resolved in precedence order **Global < Shared < Local** — the most specific wins.
//! Device/app-only settings (window geometry, theme) are the client's concern and never
//! live here.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

const SETTINGS_SCHEMA_KEY: &str = "$hotsheetSchema";
const SETTINGS_SCHEMA_VERSION: u64 = 1;
const SETTINGS_DIR: &str = ".hotsheet";
const LEGACY_SHARED_FILE: &str = "hotsheet-settings.json";
const LEGACY_LOCAL_FILE: &str = "hotsheet-settings.local.json";

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
    /// Machine-wide, project-independent (`${HOTSHEET_HOME}/settings.json`).
    Global,
    /// Committed, travels with the project.
    Shared,
    /// Machine-local, gitignored.
    Local,
}

impl Scope {
    /// The file name for this scope. Global lives under `${HOTSHEET_HOME}`; shared and
    /// local live under the project root's `.hotsheet` directory.
    pub fn file_name(self) -> &'static str {
        match self {
            Scope::Global => "settings.json",
            Scope::Shared => "settings.json",
            Scope::Local => "settings.local.json",
        }
    }

    fn legacy_file_name(self) -> Option<&'static str> {
        match self {
            Scope::Global => None,
            Scope::Shared => Some(LEGACY_SHARED_FILE),
            Scope::Local => Some(LEGACY_LOCAL_FILE),
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

/// Read/write settings owned by a code project.
///
/// Old HS2 releases wrote project settings beside a ticket store. Those files remain a
/// read-through compatibility source until the next write or explicit migration creates
/// the project-owned file. Call [`Settings::with_legacy_stores`] when a checkout uses
/// standalone stores so their old settings can be carried forward deterministically.
pub struct Settings {
    project_root: PathBuf,
    legacy_roots: Vec<PathBuf>,
    project_owned: bool,
    global_home: Option<PathBuf>,
}

impl Settings {
    /// Legacy store-owned settings compatibility. New project-aware code should use
    /// [`Settings::for_project`] or [`Settings::with_legacy_stores`]. Keeping this
    /// constructor on the original paths prevents store-only callers from silently
    /// inventing `<ticket-store>/.hotsheet` without a known code project.
    pub fn new(store_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: store_root.into(),
            legacy_roots: Vec::new(),
            project_owned: false,
            global_home: None,
        }
    }

    /// Settings owned by a code-project root. The same root is checked for files written
    /// by older HS2 releases, covering projects whose ticket store lived in the project.
    pub fn for_project(project_root: impl Into<PathBuf>) -> Self {
        let project_root = project_root.into();
        Self {
            legacy_roots: vec![project_root.clone()],
            project_root,
            project_owned: true,
            global_home: None,
        }
    }

    /// Settings for a project with one or more standalone legacy ticket stores. Legacy
    /// roots are consulted in caller-provided order and only fill keys not supplied by an
    /// earlier root. New writes always target the project, never a store.
    pub fn with_legacy_stores<P, I, S>(project_root: P, stores: I) -> Self
    where
        P: Into<PathBuf>,
        I: IntoIterator<Item = S>,
        S: Into<PathBuf>,
    {
        let project_root = project_root.into();
        let mut legacy_roots = vec![project_root.clone()];
        for store in stores {
            let store = store.into();
            if !legacy_roots.contains(&store) {
                legacy_roots.push(store);
            }
        }
        Self {
            project_root,
            legacy_roots,
            project_owned: true,
            global_home: None,
        }
    }

    /// Settings with an explicitly injected machine-wide home. Intended for
    /// isolated hosts and tests that must not mutate process-global environment.
    pub fn with_global_home(
        project_root: impl Into<PathBuf>,
        global_home: impl Into<PathBuf>,
    ) -> Self {
        let mut settings = Self::new(project_root);
        settings.global_home = Some(global_home.into());
        settings
    }

    /// Project-owned settings with an explicitly injected machine-wide home.
    pub fn for_project_with_global_home(
        project_root: impl Into<PathBuf>,
        global_home: impl Into<PathBuf>,
    ) -> Self {
        let mut settings = Self::for_project(project_root);
        settings.global_home = Some(global_home.into());
        settings
    }

    fn path(&self, scope: Scope) -> PathBuf {
        match scope {
            // Global lives under the machine home, independent of any project.
            Scope::Global => self
                .global_home
                .clone()
                .unwrap_or_else(hotsheet_home)
                .join(Scope::Global.file_name()),
            _ if self.project_owned => self.project_root.join(SETTINGS_DIR).join(scope.file_name()),
            _ => self
                .project_root
                .join(scope.legacy_file_name().expect("non-global scope")),
        }
    }

    fn read_map(
        &self,
        path: &Path,
        scope: Scope,
    ) -> Result<Option<Map<String, Value>>, SettingsError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let value: Value = serde_json::from_str(&text).map_err(|source| SettingsError::Parse {
            path: path.display().to_string(),
            source,
        })?;
        let mut map = value.as_object().cloned().unwrap_or_default();
        if let Some(version) = map.remove(SETTINGS_SCHEMA_KEY)
            && version
                .as_u64()
                .is_none_or(|version| version > SETTINGS_SCHEMA_VERSION)
        {
            return Err(SettingsError::UpgradeRequired {
                scope: scope.label(),
                found: version.to_string(),
                supported: SETTINGS_SCHEMA_VERSION,
            });
        }
        Ok(Some(map))
    }

    fn legacy_map(&self, scope: Scope) -> Result<Map<String, Value>, SettingsError> {
        let Some(file_name) = scope.legacy_file_name() else {
            return Ok(Map::new());
        };
        let mut merged = Map::new();
        for root in &self.legacy_roots {
            if let Some(map) = self.read_map(&root.join(file_name), scope)? {
                for (key, value) in map {
                    merged.entry(key).or_insert(value);
                }
            }
        }
        Ok(merged)
    }

    /// The raw map for one scope (empty if the file doesn't exist yet).
    pub fn map(&self, scope: Scope) -> Result<Map<String, Value>, SettingsError> {
        let path = self.path(scope);
        if let Some(map) = self.read_map(&path, scope)? {
            return Ok(map);
        }
        self.legacy_map(scope)
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

    /// Whether the active file for a scope carries an HS2 schema marker. Migration code
    /// uses this to distinguish an HS1 file that happens to occupy the new project path
    /// from settings already owned by HS2.
    pub fn is_schema_marked(&self, scope: Scope) -> Result<bool, SettingsError> {
        let current = self.path(scope);
        if current.is_file() {
            return schema_marker_at(&current);
        }
        if let Some(file_name) = scope.legacy_file_name() {
            for root in &self.legacy_roots {
                let path = root.join(file_name);
                if path.is_file() {
                    return schema_marker_at(&path);
                }
            }
        }
        Ok(false)
    }

    /// Replace one scope atomically at the Settings abstraction boundary. Intended for
    /// format migration when source-only keys must not become HS2 settings.
    pub fn replace_scope(
        &self,
        scope: Scope,
        map: &Map<String, Value>,
    ) -> Result<(), SettingsError> {
        self.write(scope, map)
    }

    /// Rewrite existing readable settings files in the current schema, preserving every
    /// user key. Missing scopes stay missing and already-current bytes are left untouched.
    pub fn migrate_existing(&self) -> Result<(), SettingsError> {
        for scope in [Scope::Global, Scope::Shared, Scope::Local] {
            let current_exists = self.path(scope).is_file();
            let map = self.map(scope)?;
            if current_exists || (!map.is_empty() && scope != Scope::Global) {
                self.write(scope, &map)?;
            }
        }
        Ok(())
    }

    fn write(&self, scope: Scope, map: &Map<String, Value>) -> Result<(), SettingsError> {
        let mut persisted = map.clone();
        persisted.insert(SETTINGS_SCHEMA_KEY.into(), SETTINGS_SCHEMA_VERSION.into());
        let text = serde_json::to_string_pretty(&Value::Object(persisted))
            .unwrap_or_else(|_| "{}".to_string());
        let path = self.path(scope);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = text + "\n";
        if !std::fs::read(&path).is_ok_and(|existing| existing == text.as_bytes()) {
            std::fs::write(&path, text)?;
        }
        if scope == Scope::Local {
            let ignored = if self.project_owned {
                format!("{SETTINGS_DIR}/{}", Scope::Local.file_name())
            } else {
                LEGACY_LOCAL_FILE.to_owned()
            };
            self.ensure_gitignored(&ignored)?;
        }
        Ok(())
    }

    /// Ensure `name` is listed in the project's `.gitignore` (create/append as needed),
    /// so a local settings file is never committed.
    fn ensure_gitignored(&self, name: &str) -> Result<(), SettingsError> {
        let gi = self.project_root.join(".gitignore");
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

fn schema_marker_at(path: &Path) -> Result<bool, SettingsError> {
    let text = std::fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text).map_err(|source| SettingsError::Parse {
        path: path.display().to_string(),
        source,
    })?;
    Ok(value
        .as_object()
        .and_then(|map| map.get(SETTINGS_SCHEMA_KEY))
        .and_then(Value::as_u64)
        .is_some())
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
        let s = Settings::for_project(d.path());

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
        let s = Settings::for_project(d.path());
        s.set("categories", json!(["bug"]), Scope::Shared).unwrap();
        s.set("index_path", json!("/tmp/idx"), Scope::Local)
            .unwrap();

        // shared file exists and is NOT in .gitignore
        assert!(d.path().join(".hotsheet/settings.json").is_file());
        // local file exists AND is gitignored
        assert!(d.path().join(".hotsheet/settings.local.json").is_file());
        let gi = std::fs::read_to_string(d.path().join(".gitignore")).unwrap();
        assert!(
            gi.lines()
                .any(|line| line == ".hotsheet/settings.local.json")
        );
        assert!(!gi.lines().any(|line| line == ".hotsheet/settings.json"));
    }

    #[test]
    fn gitignore_is_not_duplicated() {
        let d = root();
        std::fs::write(
            d.path().join(".gitignore"),
            "target/\n.hotsheet/settings.local.json\n",
        )
        .unwrap();
        let s = Settings::for_project(d.path());
        s.set("a", json!(1), Scope::Local).unwrap();
        s.set("b", json!(2), Scope::Local).unwrap();
        let gi = std::fs::read_to_string(d.path().join(".gitignore")).unwrap();
        assert_eq!(gi.matches(".hotsheet/settings.local.json").count(), 1);
        assert!(gi.contains("target/"), "existing entries preserved");
    }

    #[test]
    fn unset_removes_a_key() {
        let d = root();
        let s = Settings::for_project(d.path());
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
        let s = Settings::for_project(d.path());
        assert!(s.map(Scope::Shared).unwrap().is_empty());
        assert!(s.effective().unwrap().is_empty());
        assert_eq!(s.get_effective("x").unwrap(), None);
    }

    #[test]
    fn legacy_unversioned_settings_remain_readable_and_future_settings_require_upgrade() {
        let d = root();
        std::fs::create_dir(d.path().join(".hotsheet")).unwrap();
        std::fs::write(
            d.path().join(".hotsheet/settings.json"),
            r#"{"theme":"dark"}"#,
        )
        .unwrap();
        let s = Settings::for_project(d.path());
        assert_eq!(s.get("theme", Scope::Shared).unwrap(), Some(json!("dark")));

        std::fs::write(
            d.path().join(".hotsheet/settings.json"),
            r#"{"$hotsheetSchema":99,"theme":"dark"}"#,
        )
        .unwrap();
        let error = s.map(Scope::Shared).unwrap_err().to_string();
        assert!(error.contains("newer version of Hot Sheet 2"));
        assert!(error.contains("Update Hot Sheet 2"));
    }

    #[test]
    fn migrate_existing_versions_legacy_settings_and_is_byte_idempotent() {
        let d = root();
        let legacy_path = d.path().join("hotsheet-settings.json");
        std::fs::write(&legacy_path, r#"{"theme":"dark","user_key":7}"#).unwrap();
        let settings = Settings::for_project(d.path());
        settings.migrate_existing().unwrap();
        let path = d.path().join(".hotsheet/settings.json");
        let migrated = std::fs::read(&path).unwrap();
        let value: Value = serde_json::from_slice(&migrated).unwrap();
        assert_eq!(value[SETTINGS_SCHEMA_KEY], SETTINGS_SCHEMA_VERSION);
        assert_eq!(value["user_key"], 7);
        settings.migrate_existing().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), migrated);
        assert_eq!(
            std::fs::read_to_string(legacy_path).unwrap(),
            r#"{"theme":"dark","user_key":7}"#
        );
        assert!(!d.path().join(".hotsheet/settings.local.json").exists());
    }

    #[test]
    fn standalone_legacy_stores_merge_deterministically_then_stop_driving_reads() {
        let project = root();
        let preferred = root();
        let secondary = root();
        std::fs::write(
            preferred.path().join(LEGACY_SHARED_FILE),
            r#"{"preferred":true,"overlap":"first"}"#,
        )
        .unwrap();
        std::fs::write(
            secondary.path().join(LEGACY_SHARED_FILE),
            r#"{"secondary":true,"overlap":"second"}"#,
        )
        .unwrap();
        let settings =
            Settings::with_legacy_stores(project.path(), [preferred.path(), secondary.path()]);

        assert_eq!(
            settings.get("overlap", Scope::Shared).unwrap(),
            Some(json!("first"))
        );
        assert_eq!(
            settings.get("secondary", Scope::Shared).unwrap(),
            Some(json!(true))
        );
        settings.migrate_existing().unwrap();
        std::fs::write(
            preferred.path().join(LEGACY_SHARED_FILE),
            r#"{"preferred":false,"overlap":"changed"}"#,
        )
        .unwrap();
        assert_eq!(
            settings.get("overlap", Scope::Shared).unwrap(),
            Some(json!("first"))
        );
        assert_eq!(
            settings.get("preferred", Scope::Shared).unwrap(),
            Some(json!(true))
        );
        assert_eq!(
            settings.get("secondary", Scope::Shared).unwrap(),
            Some(json!(true))
        );
    }

    #[test]
    fn project_settings_work_without_any_ticket_store() {
        let project = root();
        let settings = Settings::with_legacy_stores(project.path(), std::iter::empty::<&Path>());
        settings
            .set("views", json!(["mine"]), Scope::Shared)
            .unwrap();
        settings.set("commands", json!([]), Scope::Local).unwrap();

        assert!(project.path().join(".hotsheet/settings.json").is_file());
        assert!(
            project
                .path()
                .join(".hotsheet/settings.local.json")
                .is_file()
        );
        assert_eq!(
            settings.get_effective("views").unwrap(),
            Some(json!(["mine"]))
        );
    }

    #[test]
    fn global_layer_is_machine_wide_and_lowest_precedence() {
        let home = root();
        let d = root();
        let s = Settings::for_project_with_global_home(d.path(), home.path());

        // A global default + a project override of the same key.
        s.set("default_tool", json!("claude"), Scope::Global)
            .unwrap();
        s.set("editor", json!("vim"), Scope::Global).unwrap();
        s.set("default_tool", json!("codex"), Scope::Shared)
            .unwrap();

        // Global lives under ${HOTSHEET_HOME}, not the project settings directory.
        assert!(home.path().join("settings.json").is_file());
        assert!(!d.path().join("settings.json").exists());
        assert!(d.path().join(".hotsheet/settings.json").is_file());

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
