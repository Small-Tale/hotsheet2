//! Standby **settings** (HS2-EQVZTZ, `docs/34-glance-surfaces.md` §34.5): whether a client enters
//! its night display after no direct human interaction, and after how many idle minutes.
//!
//! Both are machine-wide settings (`${HOTSHEET_HOME}/settings.json`), read and written with
//! `GET`/`PUT /standby-settings` or `hotsheet-cli settings set standby.<name> <value>` (which
//! defaults to the global scope and validates like the REST route). An unset value takes its
//! default. These are pure definitions kept in `ticketing` so the CLI can validate them without
//! linking the server.

use serde::{Deserialize, Serialize};

/// Whether idle entry is on (`true`/`false`).
pub const ENABLED_SETTING: &str = "standby.enabled";
/// Minutes without direct interaction before a client enters standby.
pub const IDLE_MINUTES_SETTING: &str = "standby.idle_minutes";

pub const DEFAULT_ENABLED: bool = true;
pub const DEFAULT_IDLE_MINUTES: u64 = 15;
pub const MIN_IDLE_MINUTES: u64 = 1;
/// One day: a longer threshold is indistinguishable from "off".
pub const MAX_IDLE_MINUTES: u64 = 1_440;

/// Effective standby settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandbySettings {
    pub enabled: bool,
    pub idle_minutes: u64,
}

/// Explicit machine-wide settings; `None` means "use the default".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredStandbySettings {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub idle_minutes: Option<u64>,
}

/// The defaults every unset value takes.
pub fn default_settings() -> StandbySettings {
    effective_settings(ConfiguredStandbySettings::default())
}

/// The effective settings: each explicit value wins over its default.
pub fn effective_settings(configured: ConfiguredStandbySettings) -> StandbySettings {
    StandbySettings {
        enabled: configured.enabled.unwrap_or(DEFAULT_ENABLED),
        idle_minutes: configured.idle_minutes.unwrap_or(DEFAULT_IDLE_MINUTES),
    }
}

/// Why a standby setting was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StandbySettingError {
    #[error("{ENABLED_SETTING} must be true or false")]
    Enabled,
    #[error(
        "{IDLE_MINUTES_SETTING} must be a whole number from {MIN_IDLE_MINUTES} to {MAX_IDLE_MINUTES}"
    )]
    IdleMinutes,
    #[error("unknown standby setting '{0}' ({ENABLED_SETTING}|{IDLE_MINUTES_SETTING})")]
    Unknown(String),
}

fn idle_in_range(minutes: u64) -> bool {
    (MIN_IDLE_MINUTES..=MAX_IDLE_MINUTES).contains(&minutes)
}

/// Whether `key` is a standby setting (so it is machine-wide and validated).
pub fn is_standby_key(key: &str) -> bool {
    key.starts_with("standby.")
}

/// Validate explicit settings against their accepted values.
pub fn validate_settings(
    configured: &ConfiguredStandbySettings,
) -> Result<(), StandbySettingError> {
    match configured.idle_minutes {
        Some(minutes) if !idle_in_range(minutes) => Err(StandbySettingError::IdleMinutes),
        _ => Ok(()),
    }
}

/// Parse a value typed on the command line into the JSON the settings file stores.
pub fn parse_setting(key: &str, raw: &str) -> Result<serde_json::Value, StandbySettingError> {
    let raw = raw.trim();
    match key {
        ENABLED_SETTING => match raw {
            "true" => Ok(true.into()),
            "false" => Ok(false.into()),
            _ => Err(StandbySettingError::Enabled),
        },
        IDLE_MINUTES_SETTING => raw
            .parse::<u64>()
            .ok()
            .filter(|minutes| idle_in_range(*minutes))
            .map(serde_json::Value::from)
            .ok_or(StandbySettingError::IdleMinutes),
        other => Err(StandbySettingError::Unknown(other.to_owned())),
    }
}

/// Read the stored settings. A missing, mistyped or out-of-range value (a hand-edited settings
/// file) is ignored so its default applies.
pub fn stored_settings(
    enabled: Option<&serde_json::Value>,
    idle_minutes: Option<&serde_json::Value>,
) -> ConfiguredStandbySettings {
    ConfiguredStandbySettings {
        enabled: enabled.and_then(serde_json::Value::as_bool),
        idle_minutes: idle_minutes
            .and_then(serde_json::Value::as_u64)
            .filter(|minutes| idle_in_range(*minutes)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_apply_until_a_value_is_set() {
        assert_eq!(
            default_settings(),
            StandbySettings {
                enabled: true,
                idle_minutes: 15
            }
        );
        let effective = effective_settings(ConfiguredStandbySettings {
            enabled: Some(false),
            idle_minutes: None,
        });
        assert!(!effective.enabled);
        assert_eq!(effective.idle_minutes, DEFAULT_IDLE_MINUTES);
    }

    #[test]
    fn cli_parsing_and_validation_enforce_values() {
        assert_eq!(parse_setting(ENABLED_SETTING, " false "), Ok(json!(false)));
        assert_eq!(parse_setting(ENABLED_SETTING, "true"), Ok(json!(true)));
        assert_eq!(
            parse_setting(ENABLED_SETTING, "yes"),
            Err(StandbySettingError::Enabled)
        );
        assert_eq!(parse_setting(IDLE_MINUTES_SETTING, "30"), Ok(json!(30)));
        for bad in ["0", "1441", "1.5", "soon"] {
            let error = parse_setting(IDLE_MINUTES_SETTING, bad).unwrap_err();
            assert!(
                error.to_string().contains("from 1 to 1440"),
                "{bad}: {error}"
            );
        }
        assert!(matches!(
            parse_setting("standby.nope", "1"),
            Err(StandbySettingError::Unknown(_))
        ));
        assert!(is_standby_key(IDLE_MINUTES_SETTING));
        assert!(!is_standby_key("host_health.sample_seconds"));
        assert!(validate_settings(&ConfiguredStandbySettings::default()).is_ok());
        assert_eq!(
            validate_settings(&ConfiguredStandbySettings {
                idle_minutes: Some(0),
                ..Default::default()
            }),
            Err(StandbySettingError::IdleMinutes)
        );
    }

    #[test]
    fn stored_values_ignore_broken_entries() {
        assert_eq!(
            stored_settings(Some(&json!(false)), Some(&json!(5))),
            ConfiguredStandbySettings {
                enabled: Some(false),
                idle_minutes: Some(5)
            }
        );
        assert_eq!(
            stored_settings(Some(&json!("false")), Some(&json!(0))),
            ConfiguredStandbySettings::default()
        );
        assert_eq!(
            stored_settings(None, Some(&json!(-3))),
            ConfiguredStandbySettings::default()
        );
    }

    #[test]
    fn rest_bodies_reject_unknown_names() {
        assert!(serde_json::from_str::<ConfiguredStandbySettings>(r#"{"idle":5}"#).is_err());
        let parsed: ConfiguredStandbySettings =
            serde_json::from_str(r#"{"enabled":null,"idle_minutes":30}"#).unwrap();
        assert_eq!(parsed.idle_minutes, Some(30));
        assert_eq!(parsed.enabled, None);
    }
}
