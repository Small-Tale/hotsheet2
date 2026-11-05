//! Host health **thresholds** (HS2-DTAVQA): when the server's host sampler calls a machine
//! condition a warning or critical callout on the glance surfaces (`docs/host-health.md`).
//!
//! Each threshold is a machine-wide setting (`${HOTSHEET_HOME}/settings.json`), set with
//! `PUT /host-health/thresholds` or `hotsheet-cli settings set host_health.<name> <n>` (which
//! defaults to the global scope and range-checks like the REST route). An unset threshold takes
//! its default. These are pure definitions kept in `ticketing` so the CLI can validate them
//! without linking the server.

use serde::{Deserialize, Serialize};

/// Prefix shared by every host-health threshold setting key.
pub const SETTING_PREFIX: &str = "host_health.";

/// One threshold: its name (the setting key without [`SETTING_PREFIX`]), default and accepted range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThresholdSpec {
    pub name: &'static str,
    pub default: u64,
    pub min: u64,
    pub max: u64,
}

/// Every threshold, in wire order. A `0` disables the projection and growth thresholds.
pub const THRESHOLDS: [ThresholdSpec; 9] = [
    // Server-local sampling cadence; it never causes client requests.
    spec("sample_seconds", 60, 10, 3_600),
    spec("disk_warning_free_gib", 20, 0, 100_000),
    spec("disk_critical_free_gib", 5, 0, 100_000),
    spec("disk_warning_hours_to_full", 24, 0, 8_760),
    spec("disk_critical_hours_to_full", 2, 0, 8_760),
    spec("swap_warning_growth_mib_per_hour", 1_024, 0, 1_048_576),
    spec("cpu_warning_percent", 90, 1, 100),
    spec("cpu_sustain_minutes", 10, 1, 1_440),
    spec("battery_critical_percent", 20, 0, 100),
];

const fn spec(name: &'static str, default: u64, min: u64, max: u64) -> ThresholdSpec {
    ThresholdSpec {
        name,
        default,
        min,
        max,
    }
}

/// Effective threshold values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostHealthThresholds {
    pub sample_seconds: u64,
    pub disk_warning_free_gib: u64,
    pub disk_critical_free_gib: u64,
    pub disk_warning_hours_to_full: u64,
    pub disk_critical_hours_to_full: u64,
    pub swap_warning_growth_mib_per_hour: u64,
    pub cpu_warning_percent: u64,
    pub cpu_sustain_minutes: u64,
    pub battery_critical_percent: u64,
}

/// Explicit machine-wide settings; `None` means "use the default".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredHostHealthThresholds {
    #[serde(default)]
    pub sample_seconds: Option<u64>,
    #[serde(default)]
    pub disk_warning_free_gib: Option<u64>,
    #[serde(default)]
    pub disk_critical_free_gib: Option<u64>,
    #[serde(default)]
    pub disk_warning_hours_to_full: Option<u64>,
    #[serde(default)]
    pub disk_critical_hours_to_full: Option<u64>,
    #[serde(default)]
    pub swap_warning_growth_mib_per_hour: Option<u64>,
    #[serde(default)]
    pub cpu_warning_percent: Option<u64>,
    #[serde(default)]
    pub cpu_sustain_minutes: Option<u64>,
    #[serde(default)]
    pub battery_critical_percent: Option<u64>,
}

impl ConfiguredHostHealthThresholds {
    /// The configured value of threshold `name`.
    pub fn get(&self, name: &str) -> Option<u64> {
        *self.slot(name)?
    }

    /// Set threshold `name`; an unknown name is ignored.
    pub fn set(&mut self, name: &str, value: Option<u64>) {
        if let Some(slot) = self.slot_mut(name) {
            *slot = value;
        }
    }

    fn slot(&self, name: &str) -> Option<&Option<u64>> {
        Some(match name {
            "sample_seconds" => &self.sample_seconds,
            "disk_warning_free_gib" => &self.disk_warning_free_gib,
            "disk_critical_free_gib" => &self.disk_critical_free_gib,
            "disk_warning_hours_to_full" => &self.disk_warning_hours_to_full,
            "disk_critical_hours_to_full" => &self.disk_critical_hours_to_full,
            "swap_warning_growth_mib_per_hour" => &self.swap_warning_growth_mib_per_hour,
            "cpu_warning_percent" => &self.cpu_warning_percent,
            "cpu_sustain_minutes" => &self.cpu_sustain_minutes,
            "battery_critical_percent" => &self.battery_critical_percent,
            _ => return None,
        })
    }

    fn slot_mut(&mut self, name: &str) -> Option<&mut Option<u64>> {
        Some(match name {
            "sample_seconds" => &mut self.sample_seconds,
            "disk_warning_free_gib" => &mut self.disk_warning_free_gib,
            "disk_critical_free_gib" => &mut self.disk_critical_free_gib,
            "disk_warning_hours_to_full" => &mut self.disk_warning_hours_to_full,
            "disk_critical_hours_to_full" => &mut self.disk_critical_hours_to_full,
            "swap_warning_growth_mib_per_hour" => &mut self.swap_warning_growth_mib_per_hour,
            "cpu_warning_percent" => &mut self.cpu_warning_percent,
            "cpu_sustain_minutes" => &mut self.cpu_sustain_minutes,
            "battery_critical_percent" => &mut self.battery_critical_percent,
            _ => return None,
        })
    }
}

/// The defaults every unset threshold takes.
pub fn default_thresholds() -> HostHealthThresholds {
    effective_thresholds(ConfiguredHostHealthThresholds::default())
}

/// The effective thresholds: each explicit setting wins over its default.
pub fn effective_thresholds(configured: ConfiguredHostHealthThresholds) -> HostHealthThresholds {
    let value = |name: &str| {
        configured.get(name).unwrap_or_else(|| {
            THRESHOLDS
                .iter()
                .find(|spec| spec.name == name)
                .map_or(0, |spec| spec.default)
        })
    };
    HostHealthThresholds {
        sample_seconds: value("sample_seconds"),
        disk_warning_free_gib: value("disk_warning_free_gib"),
        disk_critical_free_gib: value("disk_critical_free_gib"),
        disk_warning_hours_to_full: value("disk_warning_hours_to_full"),
        disk_critical_hours_to_full: value("disk_critical_hours_to_full"),
        swap_warning_growth_mib_per_hour: value("swap_warning_growth_mib_per_hour"),
        cpu_warning_percent: value("cpu_warning_percent"),
        cpu_sustain_minutes: value("cpu_sustain_minutes"),
        battery_critical_percent: value("battery_critical_percent"),
    }
}

/// Why a threshold value was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{key} must be a whole number from {min} to {max}")]
pub struct ThresholdRangeError {
    pub key: String,
    pub min: u64,
    pub max: u64,
}

/// The full setting key of threshold `name`.
pub fn setting_key(name: &str) -> String {
    format!("{SETTING_PREFIX}{name}")
}

fn spec_for_key(key: &str) -> Option<&'static ThresholdSpec> {
    let name = key.strip_prefix(SETTING_PREFIX)?;
    THRESHOLDS.iter().find(|spec| spec.name == name)
}

fn range_error(spec: &ThresholdSpec) -> ThresholdRangeError {
    ThresholdRangeError {
        key: setting_key(spec.name),
        min: spec.min,
        max: spec.max,
    }
}

/// Whether `key` is one of the machine-wide host-health thresholds.
pub fn is_host_health_key(key: &str) -> bool {
    spec_for_key(key).is_some()
}

/// Validate explicit thresholds against their accepted ranges.
pub fn validate_thresholds(
    configured: &ConfiguredHostHealthThresholds,
) -> Result<(), ThresholdRangeError> {
    for spec in &THRESHOLDS {
        if let Some(value) = configured.get(spec.name)
            && !(spec.min..=spec.max).contains(&value)
        {
            return Err(range_error(spec));
        }
    }
    Ok(())
}

/// Parse a threshold typed on the command line: a whole number within the key's range.
pub fn parse_threshold(key: &str, raw: &str) -> Result<u64, ThresholdRangeError> {
    let spec = spec_for_key(key).ok_or(ThresholdRangeError {
        key: "host health threshold".into(),
        min: 0,
        max: 0,
    })?;
    raw.trim()
        .parse::<u64>()
        .ok()
        .filter(|value| (spec.min..=spec.max).contains(value))
        .ok_or_else(|| range_error(spec))
}

/// Read one stored threshold. A missing, non-numeric or out-of-range value (a hand-edited
/// settings file) is ignored so the default applies instead of a broken threshold.
pub fn stored_threshold(key: &str, value: Option<&serde_json::Value>) -> Option<u64> {
    let number = value?.as_u64()?;
    let spec = spec_for_key(key)?;
    (spec.min..=spec.max).contains(&number).then_some(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_threshold_has_a_slot_and_a_default_inside_its_range() {
        for spec in &THRESHOLDS {
            assert!((spec.min..=spec.max).contains(&spec.default), "{spec:?}");
            let mut configured = ConfiguredHostHealthThresholds::default();
            configured.set(spec.name, Some(spec.max));
            assert_eq!(configured.get(spec.name), Some(spec.max), "{}", spec.name);
            assert!(is_host_health_key(&setting_key(spec.name)));
        }
        let defaults = default_thresholds();
        assert_eq!(defaults.sample_seconds, 60);
        assert_eq!(defaults.disk_critical_free_gib, 5);
        assert_eq!(defaults.cpu_warning_percent, 90);
        // An unknown name has no slot and is ignored.
        let mut configured = ConfiguredHostHealthThresholds::default();
        configured.set("nope", Some(1));
        assert_eq!(configured, ConfiguredHostHealthThresholds::default());
        assert_eq!(configured.get("nope"), None);
    }

    #[test]
    fn an_explicit_threshold_wins_over_its_default() {
        let effective = effective_thresholds(ConfiguredHostHealthThresholds {
            disk_warning_free_gib: Some(50),
            cpu_sustain_minutes: Some(1),
            ..Default::default()
        });
        assert_eq!(effective.disk_warning_free_gib, 50);
        assert_eq!(effective.cpu_sustain_minutes, 1);
        assert_eq!(
            effective.sample_seconds,
            default_thresholds().sample_seconds
        );
    }

    #[test]
    fn validation_and_cli_parsing_enforce_ranges() {
        assert!(validate_thresholds(&ConfiguredHostHealthThresholds::default()).is_ok());
        let error = validate_thresholds(&ConfiguredHostHealthThresholds {
            sample_seconds: Some(9),
            ..Default::default()
        })
        .unwrap_err();
        assert_eq!(error.key, "host_health.sample_seconds");
        assert!(
            validate_thresholds(&ConfiguredHostHealthThresholds {
                cpu_warning_percent: Some(101),
                ..Default::default()
            })
            .is_err()
        );
        assert_eq!(
            parse_threshold("host_health.cpu_warning_percent", " 80 "),
            Ok(80)
        );
        let error = parse_threshold("host_health.cpu_warning_percent", "0").unwrap_err();
        assert!(error.to_string().contains("from 1 to 100"), "{error}");
        assert!(parse_threshold("host_health.disk_warning_free_gib", "1.5").is_err());
        assert!(parse_threshold("host_health.nope", "1").is_err());
        assert!(!is_host_health_key("host_health."));
        assert!(!is_host_health_key("terminal.history_lines"));
    }

    #[test]
    fn stored_thresholds_ignore_broken_values() {
        let value = |json: &str| serde_json::from_str::<serde_json::Value>(json).unwrap();
        let key = "host_health.battery_critical_percent";
        assert_eq!(stored_threshold(key, Some(&value("15"))), Some(15));
        assert_eq!(stored_threshold(key, Some(&value("150"))), None);
        assert_eq!(stored_threshold(key, Some(&value("\"15\""))), None);
        assert_eq!(stored_threshold(key, Some(&value("-1"))), None);
        assert_eq!(stored_threshold(key, None), None);
        assert_eq!(
            stored_threshold("host_health.other", Some(&value("1"))),
            None
        );
    }

    #[test]
    fn rest_bodies_reject_unknown_threshold_names() {
        assert!(
            serde_json::from_str::<ConfiguredHostHealthThresholds>(r#"{"disk_free":1}"#).is_err()
        );
        let parsed: ConfiguredHostHealthThresholds =
            serde_json::from_str(r#"{"cpu_warning_percent":null,"sample_seconds":30}"#).unwrap();
        assert_eq!(parsed.sample_seconds, Some(30));
        assert_eq!(parsed.cpu_warning_percent, None);
    }
}
