//! Per-checkout MCP overrides in a client's own local layer (HS2-PN848F).
//!
//! A committed (shared) MCP config keeps the portable `hotsheet-mcp --project .` entry, which
//! needs the client to find `hotsheet-mcp` on its `PATH` and to start the server inside the
//! checkout. A Dock- or Finder-launched app gets launchd's minimal `PATH` and may use another
//! working directory. Where the client layers a per-checkout config over the shared one,
//! setup also writes an entry with the absolute binary and project paths there.
//!
//! Only Claude Code has such a layer today: its `local` MCP scope lives in the user's
//! `.claude.json` under `projects["<checkout realpath>"].mcpServers` and takes precedence
//! over the project's `.mcp.json`. Codex, OpenCode, and Antigravity document no local or
//! untracked MCP layer, so setup reports the limitation for them instead.
//!
//! The user file is Claude's own state, so edits are minimal: only the Hot Sheet server
//! entry of one project changes, an unparseable file or a server entry the user owns is
//! never overwritten, and the merge is replayed against the file's contents at apply time.

use std::path::{Path, PathBuf};

/// Test and host override for the Claude user config file. Tests force it through
/// `.cargo/config.toml` so no test edits the developer's real `~/.claude.json`.
pub const CLAUDE_USER_CONFIG_ENV: &str = "HOTSHEET_CLAUDE_USER_CONFIG";

/// Claude Code's user config file: the override, else `$CLAUDE_CONFIG_DIR/.claude.json`,
/// else `~/.claude.json` (the locations Claude Code itself reads).
pub fn claude_user_config_path() -> Option<PathBuf> {
    claude_user_config_path_from(
        std::env::var_os(CLAUDE_USER_CONFIG_ENV).map(PathBuf::from),
        std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn claude_user_config_path_from(
    explicit: Option<PathBuf>,
    config_dir: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let non_empty = |path: Option<PathBuf>| path.filter(|path| !path.as_os_str().is_empty());
    non_empty(explicit)
        .or_else(|| non_empty(config_dir).map(|dir| dir.join(".claude.json")))
        .or_else(|| non_empty(home).map(|home| home.join(".claude.json")))
}

/// The key Claude Code files a checkout under: its resolved real path.
pub fn project_key(project: &Path) -> String {
    project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// One planned change to a project's server entry in the Claude user config.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalMcpEdit {
    pub project_key: String,
    pub server: String,
    /// The manifest's bare MCP command, used to recognize a Hot Sheet-owned entry.
    pub bundled_command: String,
    /// `Some(entry)` registers or refreshes the server; `None` removes a Hot Sheet entry.
    pub entry: Option<serde_json::Value>,
}

/// What applying a [`LocalMcpEdit`] to the file's contents yields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalMcpOutcome {
    Unchanged,
    Changed(String),
    Blocked(LocalMcpBlock),
}

/// Why setup left the user's local layer alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalMcpBlock {
    /// The file exists but is not a JSON object; rewriting it could destroy Claude's state.
    Unreadable,
    /// A same-named server the user registered with another command.
    UserOwnedEntry,
}

impl LocalMcpBlock {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Unreadable => "the file is not a JSON object",
            Self::UserOwnedEntry => "it already holds a same-named server with another command",
        }
    }
}

/// The Claude local-scope entry for an absolute `hotsheet-mcp` launch.
pub fn claude_entry(command: &str, args: &[String], env: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "type": "stdio", "command": command, "args": args, "env": env })
}

/// Whether an installed MCP `command` launches Hot Sheet's MCP binary (`bundled` is the
/// manifest command; the installed one may be an absolute sibling path or a `.exe`).
pub fn is_hotsheet_command(installed: &str, bundled: &str) -> bool {
    let stem = |command: &str| {
        Path::new(command)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
    };
    stem(installed).is_some_and(|installed| Some(installed) == stem(bundled))
}

/// Apply `edit` to the user config `contents` (`None` when the file does not exist).
pub fn apply(contents: Option<&str>, edit: &LocalMcpEdit) -> LocalMcpOutcome {
    let mut root = match contents {
        None => serde_json::json!({}),
        Some(text) => match serde_json::from_str::<serde_json::Value>(text) {
            Ok(value) if value.is_object() => value,
            _ => return LocalMcpOutcome::Blocked(LocalMcpBlock::Unreadable),
        },
    };
    let existing = root
        .get("projects")
        .and_then(|projects| projects.get(&edit.project_key))
        .and_then(|project| project.get("mcpServers"))
        .and_then(|servers| servers.get(&edit.server))
        .cloned();
    if let Some(existing) = &existing {
        let owned = existing
            .get("command")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|command| is_hotsheet_command(command, &edit.bundled_command));
        if !owned {
            return match edit.entry {
                Some(_) => LocalMcpOutcome::Blocked(LocalMcpBlock::UserOwnedEntry),
                None => LocalMcpOutcome::Unchanged,
            };
        }
    }
    if existing.as_ref() == edit.entry.as_ref() {
        return LocalMcpOutcome::Unchanged;
    }
    let object = root.as_object_mut().expect("checked above");
    match &edit.entry {
        Some(entry) => {
            let projects = object
                .entry("projects")
                .or_insert_with(|| serde_json::json!({}));
            if !projects.is_object() {
                return LocalMcpOutcome::Blocked(LocalMcpBlock::Unreadable);
            }
            // A project Claude has never opened gets the defaults Claude itself writes for a
            // new project, so its own readers find every list they expect.
            let project = projects
                .as_object_mut()
                .expect("checked above")
                .entry(edit.project_key.clone())
                .or_insert_with(new_project_entry);
            let Some(project) = project.as_object_mut() else {
                return LocalMcpOutcome::Blocked(LocalMcpBlock::Unreadable);
            };
            let servers = project
                .entry("mcpServers")
                .or_insert_with(|| serde_json::json!({}));
            let Some(servers) = servers.as_object_mut() else {
                return LocalMcpOutcome::Blocked(LocalMcpBlock::Unreadable);
            };
            servers.insert(edit.server.clone(), entry.clone());
        }
        None => {
            object["projects"][&edit.project_key]["mcpServers"]
                .as_object_mut()
                .expect("existing entry implies the path")
                .remove(&edit.server);
        }
    }
    LocalMcpOutcome::Changed(serde_json::to_string_pretty(&root).expect("JSON serializes"))
}

fn new_project_entry() -> serde_json::Value {
    serde_json::json!({
        "allowedTools": [],
        "mcpContextUris": [],
        "mcpServers": {},
        "enabledMcpjsonServers": [],
        "disabledMcpjsonServers": [],
        "hasTrustDialogAccepted": false,
        "hasClaudeMdExternalIncludesApproved": false,
        "hasClaudeMdExternalIncludesWarningShown": false,
    })
}

/// Replay planned edits against the file as it is now (Claude may have rewritten it since
/// planning) and write it only when something changed. A file that became unreadable is
/// left alone.
pub fn apply_to_file(path: &Path, edits: &[LocalMcpEdit]) -> std::io::Result<bool> {
    let mut contents = match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let mut changed = false;
    for edit in edits {
        match apply(contents.as_deref(), edit) {
            LocalMcpOutcome::Changed(next) => {
                contents = Some(next);
                changed = true;
            }
            LocalMcpOutcome::Unchanged | LocalMcpOutcome::Blocked(_) => {}
        }
    }
    if changed {
        if let Some(contents) = contents {
            write_atomically(path, contents.as_bytes())?;
        }
    }
    Ok(changed)
}

fn write_atomically(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "claude.json".into());
    let staged = path.with_file_name(format!(".{name}.hotsheet-{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&staged)?;
        file.write_all(contents)?;
        file.sync_all()?;
        // Keep the user's file mode (Claude writes it 0600).
        if let Ok(metadata) = std::fs::metadata(path) {
            std::fs::set_permissions(&staged, metadata.permissions())?;
        }
        std::fs::rename(&staged, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(entry: Option<serde_json::Value>) -> LocalMcpEdit {
        LocalMcpEdit {
            project_key: "/work/app".into(),
            server: "hotsheet".into(),
            bundled_command: "hotsheet-mcp".into(),
            entry,
        }
    }

    fn hotsheet(command: &str) -> serde_json::Value {
        claude_entry(
            command,
            &["--project".into(), "/work/app".into()],
            serde_json::json!({ "HOTSHEET_ACTOR_ROLE": "ai" }),
        )
    }

    fn changed(outcome: LocalMcpOutcome) -> serde_json::Value {
        match outcome {
            LocalMcpOutcome::Changed(text) => serde_json::from_str(&text).unwrap(),
            other => panic!("expected a change, got {other:?}"),
        }
    }

    #[test]
    fn hs2_pn848f_path_prefers_override_then_claude_config_dir_then_home() {
        let some = |p: &str| Some(PathBuf::from(p));
        assert_eq!(
            claude_user_config_path_from(some("/x/c.json"), some("/cfg"), some("/home")),
            some("/x/c.json")
        );
        assert_eq!(
            claude_user_config_path_from(None, some("/cfg"), some("/home")),
            some("/cfg/.claude.json")
        );
        assert_eq!(
            claude_user_config_path_from(some(""), some(""), some("/home")),
            some("/home/.claude.json")
        );
        assert_eq!(claude_user_config_path_from(None, None, None), None);
    }

    #[test]
    fn hs2_pn848f_new_file_and_new_project_get_claude_defaults() {
        let root = changed(apply(None, &edit(Some(hotsheet("/bin/hotsheet-mcp")))));
        let project = &root["projects"]["/work/app"];
        assert_eq!(
            project["mcpServers"]["hotsheet"],
            hotsheet("/bin/hotsheet-mcp")
        );
        assert_eq!(project["allowedTools"], serde_json::json!([]));
        assert_eq!(project["hasTrustDialogAccepted"], false);
    }

    #[test]
    fn hs2_pn848f_existing_project_keeps_unrelated_state_and_refreshes_idempotently() {
        let original = serde_json::json!({
            "numStartups": 3,
            "projects": {
                "/work/app": {
                    "allowedTools": ["Bash"],
                    "hasTrustDialogAccepted": true,
                    "mcpServers": { "other": { "command": "other-mcp", "args": [] } }
                },
                "/work/other": { "mcpServers": { "hotsheet": hotsheet("/old/hotsheet-mcp") } }
            }
        })
        .to_string();
        let first = changed(apply(
            Some(&original),
            &edit(Some(hotsheet("/a/hotsheet-mcp"))),
        ));
        assert_eq!(first["numStartups"], 3);
        assert_eq!(
            first["projects"]["/work/app"]["allowedTools"],
            serde_json::json!(["Bash"])
        );
        assert_eq!(
            first["projects"]["/work/app"]["hasTrustDialogAccepted"],
            true
        );
        assert_eq!(
            first["projects"]["/work/app"]["mcpServers"]["other"]["command"],
            "other-mcp"
        );
        assert_eq!(
            first["projects"]["/work/other"]["mcpServers"]["hotsheet"]["command"],
            "/old/hotsheet-mcp",
            "another checkout's entry is untouched"
        );
        let text = first.to_string();
        assert_eq!(
            apply(Some(&text), &edit(Some(hotsheet("/a/hotsheet-mcp")))),
            LocalMcpOutcome::Unchanged
        );
        let moved = changed(apply(Some(&text), &edit(Some(hotsheet("/b/hotsheet-mcp")))));
        assert_eq!(
            moved["projects"]["/work/app"]["mcpServers"]["hotsheet"]["command"],
            "/b/hotsheet-mcp"
        );
    }

    #[test]
    fn hs2_pn848f_never_overwrites_unreadable_files_or_user_owned_servers() {
        assert_eq!(
            apply(Some("{not json"), &edit(Some(hotsheet("/a/hotsheet-mcp")))),
            LocalMcpOutcome::Blocked(LocalMcpBlock::Unreadable)
        );
        assert_eq!(
            apply(Some("[]"), &edit(None)),
            LocalMcpOutcome::Blocked(LocalMcpBlock::Unreadable)
        );
        let user = serde_json::json!({ "projects": { "/work/app": { "mcpServers": {
            "hotsheet": { "command": "my-own-server" } } } } })
        .to_string();
        assert_eq!(
            apply(Some(&user), &edit(Some(hotsheet("/a/hotsheet-mcp")))),
            LocalMcpOutcome::Blocked(LocalMcpBlock::UserOwnedEntry)
        );
        assert_eq!(apply(Some(&user), &edit(None)), LocalMcpOutcome::Unchanged);
    }

    #[test]
    fn hs2_pn848f_remove_drops_only_the_hotsheet_entry() {
        let text = serde_json::json!({ "projects": { "/work/app": {
            "allowedTools": [],
            "mcpServers": { "hotsheet": hotsheet("/a/hotsheet-mcp"), "other": { "command": "x" } }
        } } })
        .to_string();
        let removed = changed(apply(Some(&text), &edit(None)));
        let servers = &removed["projects"]["/work/app"]["mcpServers"];
        assert!(servers.get("hotsheet").is_none());
        assert_eq!(servers["other"]["command"], "x");
        assert_eq!(
            apply(Some(&removed.to_string()), &edit(None)),
            LocalMcpOutcome::Unchanged
        );
        assert_eq!(apply(None, &edit(None)), LocalMcpOutcome::Unchanged);
    }

    #[test]
    fn hs2_pn848f_apply_to_file_replays_against_current_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".claude.json");
        let planned = [edit(Some(hotsheet("/a/hotsheet-mcp")))];
        assert!(apply_to_file(&path, &planned).unwrap());
        // Claude rewrote the file after planning; the replay keeps its new state.
        let mut current: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        current["numStartups"] = 9.into();
        std::fs::write(&path, current.to_string()).unwrap();
        assert!(!apply_to_file(&path, &planned).unwrap());
        assert!(apply_to_file(&path, &[edit(None)]).unwrap());
        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(after["numStartups"], 9);
        std::fs::write(&path, "{broken").unwrap();
        assert!(!apply_to_file(&path, &planned).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
    }
}
