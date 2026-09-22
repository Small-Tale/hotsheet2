//! Versioned, phase-local progress shared by headless migration and the local bridge.

use serde::{Deserialize, Serialize};

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
            version: 1,
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
