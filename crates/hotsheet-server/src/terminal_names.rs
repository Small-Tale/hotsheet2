//! User-chosen terminal tab names (HS2-89FPV1). A rename is server state, not a browser
//! preference: it is stored in the project's machine-local settings (terminals are
//! machine-local PTYs) so it survives a page reload, a project restore, and a server restart,
//! and every client of that server sees the same name.

use hotsheet_sync::LockExt;
use std::collections::BTreeMap;
use std::sync::Mutex;

use hotsheet_ticketing::{Scope, Settings, SettingsError};

/// The local-scope settings key holding the `{terminal id: name}` map.
pub const SETTINGS_KEY: &str = "terminal.names";

/// The longest accepted name, in characters.
pub const MAX_NAME_CHARS: usize = 120;

/// Serializes read-modify-write updates of the map so two concurrent renames cannot drop
/// each other's write.
///
/// Why process-global (HS2-YEYF6Y): the map lives in a settings file on disk, and every
/// `Settings` handle (any `AppState`, embedder, or test) rewriting it in this process must
/// share one lock; a per-state lock would let two states race on the same file. Renames are
/// rare, so one coarse lock costs nothing.
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
    let _guard = WRITE_LOCK.lock_or_recover();
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

/// Forget every saved name whose terminal id is not in `live` (HS2-8A0FYR). A terminal that
/// disappears without `DELETE /terminals/{id}` (a server restart without the broker, or a broker
/// crash) would otherwise leave its name behind for a later terminal that reuses the id. Returns
/// the pruned ids, sorted.
pub fn retain_live<S: AsRef<str>>(
    settings: &Settings,
    live: &[S],
) -> Result<Vec<String>, SettingsError> {
    let _guard = WRITE_LOCK.lock_or_recover();
    let mut names = all(settings)?;
    let pruned: Vec<String> = names
        .keys()
        .filter(|id| !live.iter().any(|live| live.as_ref() == id.as_str()))
        .cloned()
        .collect();
    if pruned.is_empty() {
        return Ok(pruned);
    }
    names.retain(|id, _| !pruned.contains(id));
    if names.is_empty() {
        settings.unset(SETTINGS_KEY, Scope::Local)?;
    } else {
        settings.set(SETTINGS_KEY, serde_json::json!(names), Scope::Local)?;
    }
    Ok(pruned)
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

    #[test]
    fn retain_live_prunes_only_missing_terminals_across_transitions() {
        let (_dir, settings) = settings();
        // Nothing saved: nothing to prune, and the setting stays absent.
        assert!(retain_live::<&str>(&settings, &[]).unwrap().is_empty());
        assert_eq!(settings.get(SETTINGS_KEY, Scope::Local).unwrap(), None);
        set(&settings, "a", Some("Build")).unwrap();
        set(&settings, "b", Some("Tests")).unwrap();
        set(&settings, "c", Some("Logs")).unwrap();
        // Every named terminal is live: a no-op.
        assert!(
            retain_live(&settings, &["a", "b", "c", "d"])
                .unwrap()
                .is_empty()
        );
        assert_eq!(all(&settings).unwrap().len(), 3);
        // Two disappeared.
        assert_eq!(
            retain_live(&settings, &["b"]).unwrap(),
            vec!["a".to_owned(), "c".to_owned()]
        );
        assert_eq!(
            all(&settings).unwrap(),
            BTreeMap::from([("b".into(), "Tests".into())])
        );
        // Repeating the prune is idempotent.
        assert!(retain_live(&settings, &["b"]).unwrap().is_empty());
        // A reused id starts unnamed; renaming it again works (empty-then-refill).
        assert_eq!(
            retain_live::<&str>(&settings, &[]).unwrap(),
            vec!["b".to_owned()]
        );
        assert_eq!(settings.get(SETTINGS_KEY, Scope::Local).unwrap(), None);
        assert!(set(&settings, "a", Some("Again")).unwrap());
        assert_eq!(all(&settings).unwrap().get("a").unwrap(), "Again");
    }

    #[test]
    fn retain_live_drops_malformed_entries_with_the_rewrite() {
        let (_dir, settings) = settings();
        settings
            .set(
                SETTINGS_KEY,
                serde_json::json!({"gone": "Old", "live": "Kept", "junk": 3}),
                Scope::Local,
            )
            .unwrap();
        assert_eq!(
            retain_live(&settings, &["live"]).unwrap(),
            vec!["gone".to_owned()]
        );
        assert_eq!(
            settings.get(SETTINGS_KEY, Scope::Local).unwrap(),
            Some(serde_json::json!({"live": "Kept"}))
        );
    }
}
