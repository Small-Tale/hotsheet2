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
    /// An AI command's run action (HS2-17Y2EC). Absent keeps the legacy click: queue the
    /// ticket and signal an idle compatible session when one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<crate::command_schedules::ScheduleAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Keep an AI command's sidebar button while it has an active schedule (HS2-MA31ZN).
    /// Absent (false) hides a scheduled command from the sidebar; the schedule still runs.
    #[serde(default, skip_serializing_if = "is_false")]
    pub show_when_scheduled: bool,
    /// What happens to an AI command's tickets once they are completed (HS2-27C161).
    /// Absent is `none`: they stay where they are.
    #[serde(
        default,
        skip_serializing_if = "crate::command_cleanup::CommandCleanup::is_none"
    )]
    pub cleanup: crate::command_cleanup::CommandCleanup,
    /// When `cleanup` was turned on, as RFC 3339 (HS2-27C161). The settings writer owns it;
    /// only tickets completed at or after it are cleaned up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_since: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_program_kind(kind: &CommandKind) -> bool {
    *kind == CommandKind::Program
}

pub fn from_settings(
    settings: &crate::Settings,
) -> Result<Vec<CommandDefinition>, crate::SettingsError> {
    let mut value = settings.get_effective("commands")?;
    // HS2-27C161: a command with cleanup on but no `cleanup_since` counts from now, and the
    // stamp is persisted so nothing completed earlier is ever swept.
    if let Some(commands) = value.as_mut()
        && crate::command_cleanup::needs_cleanup_since(commands)
    {
        let prior = commands.clone();
        crate::command_cleanup::stamp_cleanup_since(
            Some(&prior),
            commands,
            &crate::command_cleanup::now_rfc3339(),
        );
        // The writer stamps its own instant, so read back what it stored when it can.
        if persist_cleanup_since(settings, commands)
            && let Some(stored) = settings.get_effective("commands")?
            && !crate::command_cleanup::needs_cleanup_since(&stored)
        {
            *commands = stored;
        }
    }
    value
        .map(serde_json::from_value)
        .transpose()
        .map(|v| v.unwrap_or_default())
        .map_err(|source| crate::SettingsError::Invalid {
            key: "commands".into(),
            source,
        })
}

/// Write stamped commands back to the most specific scope that holds them; whether it
/// wrote. A failed write keeps the in-memory stamp, and the next read tries again.
fn persist_cleanup_since(settings: &crate::Settings, commands: &serde_json::Value) -> bool {
    for scope in [
        crate::Scope::Local,
        crate::Scope::Shared,
        crate::Scope::Global,
    ] {
        match settings.get("commands", scope) {
            Ok(Some(_)) => return settings.set("commands", commands.clone(), scope).is_ok(),
            Ok(None) => {}
            Err(_) => return false,
        }
    }
    false
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
    fn hs2_17y2ec_command_action_is_optional_and_round_trips() {
        let legacy: CommandDefinition = serde_json::from_value(serde_json::json!(
            {"id": "review", "title": "Review", "kind": "ai", "prompt": "Review"}
        ))
        .unwrap();
        assert_eq!(legacy.action, None);
        assert!(
            serde_json::to_value(&legacy)
                .unwrap()
                .get("action")
                .is_none()
        );
        let explicit: CommandDefinition = serde_json::from_value(serde_json::json!(
            {"id": "review", "title": "Review", "kind": "ai", "prompt": "Review",
             "action": "reuse_or_new_ai_session"}
        ))
        .unwrap();
        assert_eq!(
            explicit.action,
            Some(crate::command_schedules::ScheduleAction::ReuseOrNewAiSession)
        );
        assert_eq!(
            serde_json::to_value(&explicit).unwrap()["action"],
            "reuse_or_new_ai_session"
        );
    }

    /// HS2-MA31ZN: the sidebar option is optional, omitted when off, and editable headlessly
    /// through the same validating settings writer as every other command field.
    #[test]
    fn hs2_ma31zn_show_when_scheduled_is_optional_and_round_trips() {
        let hidden: CommandDefinition = serde_json::from_value(serde_json::json!(
            {"id": "review", "title": "Review", "kind": "ai", "prompt": "Review"}
        ))
        .unwrap();
        assert!(!hidden.show_when_scheduled);
        assert!(
            serde_json::to_value(&hidden)
                .unwrap()
                .get("show_when_scheduled")
                .is_none()
        );
        let dir = tempfile::tempdir().unwrap();
        let settings = crate::Settings::new(dir.path());
        let command = |show: serde_json::Value| {
            serde_json::json!([{"id": "review", "title": "Review", "kind": "ai",
                "prompt": "Review", "show_when_scheduled": show}])
        };
        settings
            .set(
                "commands",
                command(serde_json::json!(true)),
                crate::Scope::Local,
            )
            .unwrap();
        let shown = &from_settings(&settings).unwrap()[0];
        assert!(shown.show_when_scheduled);
        assert_eq!(
            serde_json::to_value(shown).unwrap()["show_when_scheduled"],
            true
        );
        assert!(
            settings
                .set(
                    "commands",
                    command(serde_json::json!("yes")),
                    crate::Scope::Local
                )
                .is_err()
        );
        assert!(from_settings(&settings).unwrap()[0].show_when_scheduled);
    }

    /// HS2-27C161: cleanup defaults to none (stored absent), round-trips archive and delete,
    /// and the settings writer behind `hotsheet-cli settings set commands` refuses others.
    #[test]
    fn hs2_27c161_command_cleanup_round_trips_and_validates() {
        use crate::command_cleanup::CommandCleanup;
        let plain: CommandDefinition = serde_json::from_value(serde_json::json!(
            {"id": "review", "title": "Review", "kind": "ai", "prompt": "Review"}
        ))
        .unwrap();
        assert_eq!(plain.cleanup, CommandCleanup::None);
        assert!(
            serde_json::to_value(&plain)
                .unwrap()
                .get("cleanup")
                .is_none()
        );
        let dir = tempfile::tempdir().unwrap();
        let settings = crate::Settings::new(dir.path());
        let command = |cleanup: &str| {
            serde_json::json!([{"id": "review", "title": "Review", "kind": "ai",
                "prompt": "Review", "cleanup": cleanup}])
        };
        for (raw, expected) in [
            ("archive", CommandCleanup::Archive),
            ("delete", CommandCleanup::Delete),
            ("none", CommandCleanup::None),
        ] {
            settings
                .set("commands", command(raw), crate::Scope::Local)
                .unwrap();
            let stored = &from_settings(&settings).unwrap()[0];
            assert_eq!(stored.cleanup, expected);
            assert_eq!(
                serde_json::to_value(stored)
                    .unwrap()
                    .get("cleanup")
                    .is_some(),
                expected != CommandCleanup::None
            );
        }
        assert!(
            settings
                .set("commands", command("purge"), crate::Scope::Local)
                .is_err()
        );
        assert_eq!(
            from_settings(&settings).unwrap()[0].cleanup,
            CommandCleanup::None
        );
    }

    /// HS2-27C161: every write through the settings writer (the server's command saves and
    /// `hotsheet-cli settings set commands`) stamps `cleanup_since` when cleanup turns on,
    /// keeps it across Archive and Delete, and drops it at None.
    #[test]
    fn hs2_27c161_settings_writer_owns_cleanup_since() {
        let dir = tempfile::tempdir().unwrap();
        let settings = crate::Settings::new(dir.path());
        let write = |cleanup: &str, since: Option<&str>| {
            let mut command = serde_json::json!({"id": "review", "title": "Review", "kind": "ai",
                "prompt": "Review", "cleanup": cleanup});
            if let Some(since) = since {
                command["cleanup_since"] = since.into();
            }
            settings
                .set(
                    "commands",
                    serde_json::json!([command]),
                    crate::Scope::Local,
                )
                .unwrap();
            from_settings(&settings).unwrap().remove(0).cleanup_since
        };
        let first = write("archive", Some("2001-01-01T00:00:00Z")).expect("stamped");
        assert_ne!(first, "2001-01-01T00:00:00Z", "turning it on records now");
        assert_eq!(write("delete", None).as_deref(), Some(first.as_str()));
        assert_eq!(
            write("archive", Some("2030-01-01T00:00:00Z")).as_deref(),
            Some(first.as_str())
        );
        assert_eq!(write("none", Some(first.as_str())), None);
        let again = write("delete", None).expect("restamped");
        let parse = |raw: &str| crate::command_cleanup::parse_instant(raw).unwrap();
        assert!(parse(&again) >= parse(&first));
        // set_if (the server's save path) stamps the same way.
        let written = settings
            .set_if(
                "commands",
                serde_json::json!([{"id": "review", "title": "Review", "kind": "ai",
                    "prompt": "Review", "cleanup": "archive"}]),
                crate::Scope::Local,
                |_| true,
            )
            .unwrap();
        assert!(written);
        assert_eq!(
            from_settings(&settings).unwrap()[0]
                .cleanup_since
                .as_deref(),
            Some(again.as_str())
        );
    }

    /// HS2-27C161: a command stored with cleanup on but no `cleanup_since` counts from its
    /// first read, and that stamp is persisted, so later reads keep it.
    #[test]
    fn hs2_27c161_missing_cleanup_since_is_stamped_and_persisted_on_read() {
        let dir = tempfile::tempdir().unwrap();
        let settings = crate::Settings::new(dir.path());
        let mut map = serde_json::Map::new();
        map.insert(
            "commands".into(),
            serde_json::json!([{"id": "review", "title": "Review", "kind": "ai",
                "prompt": "Review", "cleanup": "archive"}]),
        );
        settings.replace_scope(crate::Scope::Local, &map).unwrap();
        let stamped = from_settings(&settings).unwrap()[0]
            .cleanup_since
            .clone()
            .expect("stamped on read");
        let stored = settings
            .get("commands", crate::Scope::Local)
            .unwrap()
            .unwrap();
        assert_eq!(stored[0]["cleanup_since"], stamped.as_str());
        assert_eq!(
            from_settings(&settings).unwrap()[0]
                .cleanup_since
                .as_deref(),
            Some(stamped.as_str())
        );
    }

    /// HS2-17Y2EC: `hotsheet-cli settings set commands` goes through this writer, so a
    /// headless edit can set a valid action and cannot store an unknown one.
    #[test]
    fn hs2_17y2ec_settings_writer_validates_command_action() {
        let dir = tempfile::tempdir().unwrap();
        let settings = crate::Settings::new(dir.path());
        let command = |action: &str| {
            serde_json::json!([{"id": "review", "title": "Review", "kind": "ai",
                "prompt": "Review", "action": action}])
        };
        settings
            .set("commands", command("new_ai_session"), crate::Scope::Local)
            .unwrap();
        assert_eq!(
            from_settings(&settings).unwrap()[0].action,
            Some(crate::command_schedules::ScheduleAction::NewAiSession)
        );
        assert!(
            settings
                .set("commands", command("run_everything"), crate::Scope::Local)
                .is_err()
        );
        assert_eq!(
            from_settings(&settings).unwrap()[0].action,
            Some(crate::command_schedules::ScheduleAction::NewAiSession)
        );
    }

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
    /// The repository's committed Quality commands (HS2-11285R) parse as typed AI commands, and
    /// every skill a prompt names exists for both Claude (`.claude/skills`) and Codex
    /// (`.agents/skills`) with identical text, so a button never points at a missing skill.
    #[test]
    fn repository_quality_commands_reference_present_skills() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let settings = crate::Settings::for_project(&repo);
        let value = settings
            .get("commands", crate::Scope::Shared)
            .unwrap()
            .expect("committed commands");
        let commands: Vec<CommandDefinition> = serde_json::from_value(value).unwrap();
        let quality: Vec<_> = commands
            .iter()
            .filter(|command| command.group.as_deref() == Some("Quality"))
            .collect();
        let titles: Vec<_> = quality
            .iter()
            .map(|command| command.title.as_str())
            .collect();
        assert_eq!(
            titles,
            [
                "Reqs ↔ Code",
                "Check Code Hygiene",
                "Analyze Code Quality",
                "Everything"
            ]
        );
        let skills = [
            "check-requirements-against-code",
            "check-code-hygiene",
            "analyze-code-quality",
        ];
        for command in &quality {
            assert_eq!(command.kind, CommandKind::Ai, "{}", command.id);
            let prompt = command
                .prompt
                .as_deref()
                .expect("AI commands carry a prompt");
            let named: Vec<_> = skills
                .iter()
                .filter(|skill| prompt.contains(*skill))
                .collect();
            assert!(!named.is_empty(), "{} names no quality skill", command.id);
            assert!(
                command.icon.is_some() && command.color.is_some(),
                "{}",
                command.id
            );
        }
        // Everything runs all three, in order.
        let everything = quality.last().unwrap().prompt.as_deref().unwrap();
        let positions: Vec<_> = skills
            .iter()
            .map(|skill| everything.find(skill).expect(skill))
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        for skill in skills {
            let claude =
                std::fs::read_to_string(repo.join(format!(".claude/skills/{skill}/SKILL.md")))
                    .unwrap();
            let codex =
                std::fs::read_to_string(repo.join(format!(".agents/skills/{skill}/SKILL.md")))
                    .unwrap();
            assert!(claude.contains(&format!("name: {skill}")), "{skill}");
            assert_eq!(claude, codex, "{skill} Claude and Codex copies drifted");
        }
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
