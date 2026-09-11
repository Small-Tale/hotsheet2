use std::collections::HashSet;

use hotsheet_ticketing::{Scope, Settings, SettingsError};
use serde::{Deserialize, Serialize};

const SETTINGS_KEY: &str = "views";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CustomView {
    pub id: String,
    pub name: String,
    pub query: String,
}

pub fn from_settings(settings: &Settings) -> Result<Vec<CustomView>, SettingsError> {
    settings
        .get(SETTINGS_KEY, Scope::Shared)?
        .map(|value| {
            serde_json::from_value(value).map_err(|source| SettingsError::Invalid {
                key: SETTINGS_KEY.to_owned(),
                source,
            })
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

pub fn replace(settings: &Settings, views: &[CustomView]) -> Result<(), SettingsError> {
    settings.set(SETTINGS_KEY, serde_json::json!(views), Scope::Shared)
}

pub fn validate(views: &[CustomView]) -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for view in views {
        let id = view.id.trim();
        let name = view.name.trim();
        let query = view.query.trim();
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("view ids must be 1–64 letters, numbers, or hyphens".to_owned());
        }
        if name.is_empty() || name.chars().count() > 80 {
            return Err("view names must be 1–80 characters".to_owned());
        }
        if query.is_empty() || query.chars().count() > 2_000 {
            return Err("view queries must be 1–2000 characters".to_owned());
        }
        if !ids.insert(id.to_ascii_lowercase()) {
            return Err("view ids must be unique".to_owned());
        }
        if !names.insert(name.to_lowercase()) {
            return Err("view names must be unique".to_owned());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_views_through_shared_project_settings() {
        let root = tempfile::tempdir().unwrap();
        let settings =
            Settings::for_project_with_global_home(root.path(), root.path().join("home"));
        let views = vec![CustomView {
            id: "needs-docs".to_owned(),
            name: "Needs docs".to_owned(),
            query: "tag:docs AND NOT status:completed".to_owned(),
        }];
        validate(&views).unwrap();
        replace(&settings, &views).unwrap();
        assert_eq!(from_settings(&settings).unwrap(), views);
        assert!(root.path().join(".hotsheet/settings.json").exists());
    }

    #[test]
    fn rejects_empty_queries_and_case_insensitive_duplicate_names() {
        let empty = vec![CustomView {
            id: "empty".into(),
            name: "Empty".into(),
            query: " ".into(),
        }];
        assert!(validate(&empty).unwrap_err().contains("queries"));
        let duplicates = vec![
            CustomView {
                id: "one".into(),
                name: "Review".into(),
                query: "one".into(),
            },
            CustomView {
                id: "two".into(),
                name: "review".into(),
                query: "two".into(),
            },
        ];
        assert!(validate(&duplicates).unwrap_err().contains("names"));
    }
}
