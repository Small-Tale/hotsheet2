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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_legacy_argv_and_native_ai_command_schemas() {
        let value = serde_json::json!([
            {"id":"test","title":"Test","program":"cargo","args":["test"],"cwd":"/code/project"},
            {"id":"review","title":"Review","kind":"ai","prompt":"Review this","tool":"codex","icon":"send","color":"#3b82f6"}
        ]);
        let defs: Vec<CommandDefinition> = serde_json::from_value(value).unwrap();
        assert_eq!(defs[0].args, ["test"]);
        assert_eq!(defs[0].cwd.as_deref(), Some("/code/project"));
        assert_eq!(defs[1].kind, CommandKind::Ai);
        assert!(defs[1].program.is_empty());
        assert_eq!(defs[1].icon.as_deref(), Some("send"));
        assert_eq!(defs[1].prompt.as_deref(), Some("Review this"));
    }
}
