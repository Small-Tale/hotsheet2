//! Typed project command settings. Requests select a configured id; they never submit
//! an arbitrary shell string to the server.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    #[default]
    Program,
    Shell,
    Ai,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandDefinition {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "is_program_kind")]
    pub kind: CommandKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub program: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Optional command working directory. Migrated HS1 shell/AI buttons use the
    /// original code-project root; ordinary HS2 commands fall back to the store root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub confirmation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

fn is_program_kind(kind: &CommandKind) -> bool {
    *kind == CommandKind::Program
}

pub fn from_settings(
    settings: &crate::Settings,
) -> Result<Vec<CommandDefinition>, crate::SettingsError> {
    settings
        .get_effective("commands")?
        .map(serde_json::from_value)
        .transpose()
        .map(|v| v.unwrap_or_default())
        .map_err(|source| crate::SettingsError::Invalid {
            key: "commands".into(),
            source,
        })
}

/// Settings key holding the project's explicitly kept command groups (HS2-EZ5KMC).
pub const COMMAND_GROUPS_KEY: &str = "command_groups";

/// Normalize kept command-group names: trimmed, blank names dropped, first occurrence wins.
pub fn normalize_groups(groups: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    groups
        .into_iter()
        .map(|group| group.trim().to_owned())
        .filter(|group| !group.is_empty() && seen.insert(group.clone()))
        .collect()
}

/// The command groups the settings editor keeps even while they contain no commands. They
/// live beside `commands` in the same settings scope, so an empty group survives a reload.
pub fn groups_from_settings(
    settings: &crate::Settings,
) -> Result<Vec<String>, crate::SettingsError> {
    settings
        .get_effective(COMMAND_GROUPS_KEY)?
        .map(serde_json::from_value::<Vec<String>>)
        .transpose()
        .map(|v| normalize_groups(v.unwrap_or_default()))
        .map_err(|source| crate::SettingsError::Invalid {
            key: COMMAND_GROUPS_KEY.into(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_kept_command_groups() {
        let groups = normalize_groups(
            [" Ideas ", "", "Later", "Ideas", "  "]
                .into_iter()
                .map(String::from),
        );
        assert_eq!(groups, ["Ideas", "Later"]);
    }

    #[test]
    fn reads_kept_command_groups_from_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings = crate::Settings::new(dir.path());
        assert!(groups_from_settings(&settings).unwrap().is_empty());
        settings
            .set(
                COMMAND_GROUPS_KEY,
                serde_json::json!(["Ideas", " Ideas", "Later"]),
                crate::Scope::Local,
            )
            .unwrap();
        assert_eq!(groups_from_settings(&settings).unwrap(), ["Ideas", "Later"]);
        // The generic settings writer (and so `hotsheet-cli settings set`) rejects a non-list.
        assert!(
            settings
                .set(
                    COMMAND_GROUPS_KEY,
                    serde_json::json!("nope"),
                    crate::Scope::Local,
                )
                .is_err()
        );
        assert_eq!(groups_from_settings(&settings).unwrap(), ["Ideas", "Later"]);
    }
    #[test]
    fn parses_legacy_argv_and_native_ai_command_schemas() {
        let value = serde_json::json!([
            {"id":"test","title":"Test","program":"cargo","args":["test"],"cwd":"/code/project"},
            {"id":"review","title":"Review","kind":"ai","prompt":"Review this","tool":"codex","model":"gpt-5.6","effort":"high","icon":"send","color":"#3b82f6"}
        ]);
        let defs: Vec<CommandDefinition> = serde_json::from_value(value).unwrap();
        assert_eq!(defs[0].args, ["test"]);
        assert_eq!(defs[0].cwd.as_deref(), Some("/code/project"));
        assert_eq!(defs[1].kind, CommandKind::Ai);
        assert!(defs[1].program.is_empty());
        assert_eq!(defs[1].icon.as_deref(), Some("send"));
        assert_eq!(defs[1].prompt.as_deref(), Some("Review this"));
        assert_eq!(defs[1].model.as_deref(), Some("gpt-5.6"));
        assert_eq!(defs[1].effort.as_deref(), Some("high"));
    }
}
