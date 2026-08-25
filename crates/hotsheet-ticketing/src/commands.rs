//! Typed project command settings. Requests select a configured id; they never submit
//! an arbitrary shell string to the server.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandDefinition {
    pub id: String,
    pub title: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub confirmation: Option<String>,
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
    fn parses_argv_schema_without_a_shell_command_field() {
        let value =
            serde_json::json!([{"id":"test","title":"Test","program":"cargo","args":["test"]}]);
        let defs: Vec<CommandDefinition> = serde_json::from_value(value).unwrap();
        assert_eq!(defs[0].args, ["test"]);
        assert!(
            serde_json::from_value::<Vec<CommandDefinition>>(
                serde_json::json!([{"id":"x","title":"X","command":"rm -rf /"}])
            )
            .is_err()
        );
    }
}
