//! User-chosen terminal tab names (HS2-89FPV1). A rename is server state, not a browser
//! preference: it is stored in the project's machine-local settings (terminals are
//! machine-local PTYs) so it survives a page reload, a project restore, and a server restart,
//! and every client of that server sees the same name.

use std::collections::BTreeMap;
use std::sync::Mutex;

use hotsheet_ticketing::{Scope, Settings, SettingsError};

/// The local-scope settings key holding the `{terminal id: name}` map.
pub const SETTINGS_KEY: &str = "terminal.names";

/// The longest accepted name, in characters.
pub const MAX_NAME_CHARS: usize = 120;

/// Serializes read-modify-write updates of the map so two concurrent renames cannot drop
/// each other's write.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Every saved name. A missing, malformed, or non-string entry reads as "no name" rather than
/// failing the terminal list.
pub fn all(settings: &Settings) -> Result<BTreeMap<String, String>, SettingsError> {
    let Some(serde_json::Value::Object(map)) = settings.get(SETTINGS_KEY, Scope::Local)? else {
        return Ok(BTreeMap::new());
    };
    Ok(map
        .into_iter()
        .filter_map(|(id, value)| {
            let name = value.as_str()?.trim();
            (!name.is_empty()).then(|| (id, name.to_owned()))
        })
        .collect())
}

/// Normalize a requested name: trimmed, `None` for an empty value (which clears the rename).
pub fn normalize(name: Option<&str>) -> Result<Option<String>, String> {
    let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(None);
    };
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "terminal names can be at most {MAX_NAME_CHARS} characters"
        ));
    }
    if name.chars().any(char::is_control) {
        return Err("terminal names cannot contain control characters".to_owned());
    }
    Ok(Some(name.to_owned()))
}

/// Save (`Some`) or clear (`None`) one terminal's name. Returns whether the stored value changed.
pub fn set(settings: &Settings, id: &str, name: Option<&str>) -> Result<bool, SettingsError> {
    let _guard = WRITE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut names = all(settings)?;
    let changed = match name {
        Some(name) => names.insert(id.to_owned(), name.to_owned()).as_deref() != Some(name),
        None => names.remove(id).is_some(),
    };
    if !changed {
        return Ok(false);
    }
    if names.is_empty() {
        settings.unset(SETTINGS_KEY, Scope::Local)?;
    } else {
        settings.set(SETTINGS_KEY, serde_json::json!(names), Scope::Local)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> (tempfile::TempDir, Settings) {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::new(dir.path());
        (dir, settings)
    }

    #[test]
    fn rename_then_rename_again_then_clear_walks_every_transition() {
        let (_dir, settings) = settings();
        assert!(all(&settings).unwrap().is_empty());
        assert!(set(&settings, "a", Some("Build")).unwrap());
        assert!(set(&settings, "b", Some("Tests")).unwrap());
        assert!(
            !set(&settings, "a", Some("Build")).unwrap(),
            "same name is a no-op"
        );
        assert!(set(&settings, "a", Some("Deploy")).unwrap());
        assert_eq!(
            all(&settings).unwrap(),
            BTreeMap::from([("a".into(), "Deploy".into()), ("b".into(), "Tests".into())])
        );
        assert!(set(&settings, "a", None).unwrap());
        assert!(
            !set(&settings, "a", None).unwrap(),
            "clearing twice is a no-op"
        );
        assert!(set(&settings, "b", None).unwrap());
        assert_eq!(settings.get(SETTINGS_KEY, Scope::Local).unwrap(), None);
        // Empty-then-refill.
        assert!(set(&settings, "b", Some("Again")).unwrap());
        assert_eq!(all(&settings).unwrap().get("b").unwrap(), "Again");
    }

    #[test]
    fn malformed_stored_values_read_as_unnamed() {
        let (_dir, settings) = settings();
        settings
            .set(
                SETTINGS_KEY,
                serde_json::json!({"a": 3, "b": "  ", "c": " Named "}),
                Scope::Local,
            )
            .unwrap();
        assert_eq!(
            all(&settings).unwrap(),
            BTreeMap::from([("c".into(), "Named".into())])
        );
        settings
            .set(SETTINGS_KEY, serde_json::json!(["x"]), Scope::Local)
            .unwrap();
        assert!(all(&settings).unwrap().is_empty());
    }

    #[test]
    fn normalize_trims_clears_and_rejects_bad_names() {
        assert_eq!(normalize(None).unwrap(), None);
        assert_eq!(normalize(Some("   ")).unwrap(), None);
        assert_eq!(normalize(Some(" Logs ")).unwrap().as_deref(), Some("Logs"));
        assert!(normalize(Some(&"x".repeat(MAX_NAME_CHARS))).is_ok());
        assert!(normalize(Some(&"x".repeat(MAX_NAME_CHARS + 1))).is_err());
        assert!(normalize(Some("a\nb")).is_err());
    }

    #[test]
    fn concurrent_renames_of_different_terminals_all_persist() {
        let (_dir, settings) = settings();
        std::thread::scope(|scope| {
            for index in 0..8 {
                let settings = &settings;
                scope.spawn(move || {
                    set(
                        settings,
                        &format!("t{index}"),
                        Some(&format!("Name {index}")),
                    )
                    .unwrap();
                });
            }
        });
        assert_eq!(all(&settings).unwrap().len(), 8);
    }
}
