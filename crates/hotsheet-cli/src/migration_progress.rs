//! Versioned, phase-local progress shared by headless migration and the local bridge.

use serde::{Deserialize, Serialize};

/// The `--progress-json` event schema version this CLI accepts from the migrator and
/// emits itself. It must equal `PROGRESS_VERSION` in `migrator/src/export.mjs`; a test
/// pins the two together (HS2-Q77A53).
pub const MIGRATOR_PROGRESS_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrationProgress {
    pub version: u8,
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

impl MigrationProgress {
    pub fn phase(phase: &str) -> Self {
        Self {
            version: MIGRATOR_PROGRESS_VERSION,
            phase: phase.into(),
            completed: None,
            total: None,
            unit: None,
            warning: None,
        }
    }

    pub fn measured(phase: &str, completed: u64, total: u64, unit: &str) -> Self {
        Self {
            completed: Some(completed),
            total: Some(total),
            unit: Some(unit.into()),
            ..Self::phase(phase)
        }
    }
}

pub type ProgressObserver<'a> = &'a mut dyn FnMut(MigrationProgress);

/// One exporter stdout line as a typed progress event, or `None` for a diagnostic line
/// or an event of another schema version, which the caller relays to stderr instead.
pub fn parse_migrator_progress(line: &str) -> Option<MigrationProgress> {
    serde_json::from_str::<MigrationProgress>(line)
        .ok()
        .filter(|event| event.version == MIGRATOR_PROGRESS_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_q77a53_accepts_only_the_current_progress_version() {
        let current = format!(
            r#"{{"version":{MIGRATOR_PROGRESS_VERSION},"phase":"copy_database","completed":1,"total":2,"unit":"bytes"}}"#
        );
        assert_eq!(
            parse_migrator_progress(&current),
            Some(MigrationProgress::measured("copy_database", 1, 2, "bytes"))
        );
        for rejected in [
            r#"{"version":0,"phase":"copy_database"}"#,
            r#"{"version":2,"phase":"copy_database"}"#,
            r#"{"phase":"copy_database"}"#,
            r#"{"version":1}"#,
            "(reading from the 'template1' database)",
            "",
        ] {
            assert_eq!(parse_migrator_progress(rejected), None, "{rejected}");
        }
        assert_eq!(
            MigrationProgress::phase("export_start").version,
            MIGRATOR_PROGRESS_VERSION
        );
    }

    #[test]
    fn hs2_q77a53_progress_version_matches_the_migrator() {
        let exporter = include_str!("../../../migrator/src/export.mjs");
        let declaration = format!("export const PROGRESS_VERSION = {MIGRATOR_PROGRESS_VERSION};");
        assert_eq!(
            exporter.matches("export const PROGRESS_VERSION =").count(),
            1,
            "the migrator declares PROGRESS_VERSION exactly once"
        );
        assert!(
            exporter.contains(&declaration),
            "migrator/src/export.mjs must declare `{declaration}`"
        );
    }
}
