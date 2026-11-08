//! Machine-local automatic permission decisions (HS2-QAGSJK).
//!
//! A browser may forget site data. This choice belongs to the code project on this
//! machine and must never be read from or written to committed shared settings.

use serde::{Deserialize, Serialize};

use crate::settings::{Scope, Settings, SettingsError};

pub const SETTING: &str = "permissions.automation";

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Off,
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct PermissionAutomation {
    pub action: Action,
    #[serde(rename = "delayMs")]
    pub delay_ms: u32,
}

impl Default for PermissionAutomation {
    fn default() -> Self {
        Self {
            action: Action::Off,
            delay_ms: 60_000,
        }
    }
}

impl PermissionAutomation {
    pub fn valid(self) -> bool {
        matches!(
            self.delay_ms,
            0 | 5_000 | 15_000 | 60_000 | 120_000 | 300_000 | 900_000 | 3_600_000
        ) && !(self.action == Action::Deny && self.delay_ms == 0)
    }
}

pub fn parse(value: serde_json::Value) -> Result<PermissionAutomation, SettingsError> {
    let setting: PermissionAutomation =
        serde_json::from_value(value).map_err(|source| SettingsError::Invalid {
            key: SETTING.into(),
            source,
        })?;
    if !setting.valid() {
        return Err(SettingsError::Invalid {
            key: SETTING.into(),
            source: <serde_json::Error as serde::de::Error>::custom(
                "invalid automatic decision or delay",
            ),
        });
    }
    Ok(setting)
}

pub fn configured(settings: &Settings) -> Result<Option<PermissionAutomation>, SettingsError> {
    settings.get(SETTING, Scope::Local)?.map(parse).transpose()
}

pub fn save(settings: &Settings, value: PermissionAutomation) -> Result<(), SettingsError> {
    let value = parse(serde_json::to_value(value).expect("serializable permission automation"))?;
    settings.set(
        SETTING,
        serde_json::to_value(value).expect("serializable permission automation"),
        Scope::Local,
    )
}

/// A legacy browser choice may fill an absent setting, never overwrite a newer
/// server or CLI choice made in another window during migration.
pub fn migrate_if_absent(
    settings: &Settings,
    value: PermissionAutomation,
) -> Result<bool, SettingsError> {
    let value = parse(serde_json::to_value(value).expect("serializable permission automation"))?;
    settings.set_if(
        SETTING,
        serde_json::to_value(value).expect("serializable permission automation"),
        Scope::Local,
        |current| current.is_none(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hs2_qagsjk_only_accepts_known_decisions_and_delays() {
        assert_eq!(
            parse(json!({"action":"allow","delayMs":5000}))
                .unwrap()
                .delay_ms,
            5_000
        );
        assert!(parse(json!({"action":"deny","delayMs":0})).is_err());
        assert!(parse(json!({"action":"allow","delayMs":1})).is_err());
        assert!(parse(json!({"action":"maybe","delayMs":5000})).is_err());
    }

    #[test]
    fn hs2_qagsjk_saves_only_to_gitignored_local_scope() {
        let root = tempfile::tempdir().unwrap();
        let other_root = tempfile::tempdir().unwrap();
        let settings = Settings::for_project(root.path());
        let choice = PermissionAutomation {
            action: Action::Allow,
            delay_ms: 5_000,
        };
        save(&settings, choice).unwrap();
        assert_eq!(configured(&settings).unwrap(), Some(choice));
        assert_eq!(
            configured(&Settings::for_project(other_root.path())).unwrap(),
            None
        );
        assert!(root.path().join(".hotsheet2/settings.local.json").exists());
        assert!(!root.path().join(".hotsheet2/settings.json").exists());
    }

    #[test]
    fn hs2_qagsjk_migration_does_not_replace_newer_choice() {
        let root = tempfile::tempdir().unwrap();
        let settings = Settings::for_project(root.path());
        let old = PermissionAutomation {
            action: Action::Allow,
            delay_ms: 5_000,
        };
        let newer = PermissionAutomation {
            action: Action::Deny,
            delay_ms: 15_000,
        };
        assert!(migrate_if_absent(&settings, old).unwrap());
        save(&settings, newer).unwrap();
        assert!(!migrate_if_absent(&settings, old).unwrap());
        assert_eq!(configured(&settings).unwrap(), Some(newer));
    }
}
