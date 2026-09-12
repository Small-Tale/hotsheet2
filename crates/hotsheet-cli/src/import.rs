//! Import a portable `hotsheet-export.json` (produced by the Node HS1 exporter,
//! `docs/07-migration.md` §7.2.1 shape B) into a git store, writing ticket files
//! through the core's own writer so the format never drifts.
//!
//! Idempotent without retaining HS1 fields: source identity deterministically mints
//! each destination ULID, so a repeat import recognizes the same ticket by its HS2 id.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use hotsheet_model::{
    CloseReason, Note, NoteKind, Priority, Status, Ticket, Timestamp, Ulid, derive_slug,
};
use hotsheet_ticketing::{
    FsStore, Scope, Settings,
    commands::{CommandDefinition, CommandKind},
};
use serde::Deserialize;
use serde_json::{Map, Value};

/// The export-file `exportVersion` this importer understands (`docs/07` §7.2.1).
pub const SUPPORTED_EXPORT_VERSION: u32 = 1;

/// A parsed export bundle (`docs/07` §7.2.1).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    #[serde(default)]
    pub export_version: u32,
    #[serde(default)]
    pub project: ProjectInfo,
    #[serde(default)]
    pub settings: Map<String, Value>,
    #[serde(default)]
    pub tickets: Vec<ExportTicket>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub name: Option<String>,
    pub ticket_prefix: Option<String>,
    /// Absolute code-project root recorded by the HS1 datadir exporter. Optional
    /// for compatibility with exportVersion 1 files made by older exporters.
    pub source_root: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Hs1Command {
    #[serde(default)]
    id: Option<String>,
    name: String,
    prompt: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Hs1CommandGroup {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    id: Option<String>,
    name: String,
    #[serde(default)]
    children: Vec<Hs1Command>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Hs1CommandItem {
    Group(Hs1CommandGroup),
    Command(Hs1Command),
}

/// One HS1 ticket as exported (snake_case, mirroring the HS1 schema).
#[derive(Debug, Deserialize)]
pub struct ExportTicket {
    pub ticket_number: Option<String>,
    pub title: String,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub up_next: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Vec<ExportNote>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub completed_at: Option<String>,
    pub verified_at: Option<String>,
    pub deleted_at: Option<String>,
    #[serde(default)]
    pub attachments: Vec<ExportAttachment>,
}

#[derive(Debug, Deserialize)]
pub struct ExportNote {
    #[serde(default)]
    pub id: Option<String>,
    pub text: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// An attachment as exported: its display filename + the staged file path (relative
/// to the export JSON's directory, written by the migrator's staging pass).
#[derive(Debug, Deserialize)]
pub struct ExportAttachment {
    #[serde(default)]
    pub id: Option<String>,
    pub original_filename: Option<String>,
    pub stored_path: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// Result of an import run.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub written: usize,
    pub skipped: usize,
    pub attachments: usize,
}

/// Import `export` into `store`. Two passes: assign every source ticket a fresh ULID
/// first, then write files with `blocked_by` remapped from old `HS-N` refs to the new
/// ULIDs. HS1 note ids (`n_…`) are replaced with fresh ULIDs so appends merge (§2.6).
///
/// Attachment `stored_path`s are resolved relative to `base_dir` (the export JSON's
/// directory, where the migrator staged the files) and copied into the store.
pub fn import(store: &FsStore, export: &ExportFile, base_dir: &Path) -> Result<ImportSummary> {
    let prefix = store.metadata()?.ticket_prefix;

    let already: HashSet<Ulid> = store.list_tickets()?.into_iter().map(|t| t.id).collect();

    // Pass 1 — one stable HS2 ULID per source ticket, keyed by its old number only
    // while this in-memory conversion runs. The HS1 number is not persisted.
    let ids: Vec<Ulid> = export
        .tickets
        .iter()
        .enumerate()
        .map(|(i, t)| import_id(&export.project, t, i))
        .collect();
    let id_by_number: HashMap<&str, Ulid> = export
        .tickets
        .iter()
        .zip(&ids)
        .filter_map(|(t, id)| t.ticket_number.as_deref().map(|n| (n, *id)))
        .collect();

    // Pass 2 — build + write.
    let mut summary = ImportSummary::default();
    for (src, id) in export.tickets.iter().zip(&ids) {
        if already.contains(id) {
            summary.skipped += 1;
            continue;
        }
        store.write_ticket(&build_ticket(src, *id, &prefix, &id_by_number))?;
        summary.written += 1;
        summary.attachments += copy_attachments(store, base_dir, id, &src.attachments)?;
    }
    import_settings(store, export)?;
    Ok(summary)
}

/// Carry forward HS1's project settings that are still meaningful as shared HS2
/// settings. Identity fields initialize the store itself and are not duplicated.
/// `custom_commands` is translated separately into HS2's typed, machine-local
/// `commands` setting; every other JSON value keeps its original key.
fn import_settings(store: &FsStore, export: &ExportFile) -> Result<()> {
    let project_root = export
        .project
        .source_root
        .as_deref()
        .map(Path::new)
        .filter(|root| root.join(".hotsheet").is_dir());
    let settings = project_root
        .map(|root| Settings::with_legacy_stores(root, [store.root()]))
        .unwrap_or_else(|| Settings::new(store.root()));
    let local_was_hs2 = settings.is_schema_marked(Scope::Local)?;
    let mut shared = settings.map(Scope::Shared)?;
    let shared_source_exists = if let Some(root) = project_root {
        let current = root.join(".hotsheet2/settings.json");
        let hs1 = root.join(".hotsheet/settings.json");
        if !current.is_file() && hs1.is_file() && !settings.is_schema_marked(Scope::Shared)? {
            shared = read_hs1_settings_map(&hs1)?;
        }
        current.is_file() || hs1.is_file()
    } else {
        store.root().join("hotsheet-settings.json").is_file()
    };
    for source_only_key in ["appName", "ticketPrefix", "custom_commands"] {
        shared.remove(source_only_key);
    }
    for (key, value) in &export.settings {
        if key != "appName" && key != "ticketPrefix" && key != "custom_commands" {
            shared.insert(key.clone(), value.clone());
        }
    }
    if shared_source_exists || !shared.is_empty() {
        settings.replace_scope(Scope::Shared, &shared)?;
    }
    import_custom_commands(store, export, &settings, local_was_hs2)?;
    Ok(())
}

/// The explicit HS1 import is the one boundary allowed to interpret unmarked files in
/// HS1's directory. Normal HS2 settings reads deliberately ignore this path.
fn read_hs1_settings_map(path: &Path) -> Result<Map<String, Value>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading HS1 settings from {}", path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .with_context(|| format!("parsing HS1 settings from {}", path.display()))?;
    let mut map = value.as_object().cloned().unwrap_or_default();
    map.remove("$hotsheetSchema");
    Ok(map)
}

/// Translate HS1's ordered command tree into the flat typed HS2 command list.
/// Existing effective HS2 commands win: equivalent definitions are deduplicated,
/// while id collisions receive a deterministic suffix and never overwrite a user
/// definition. The resulting list is local because it carries machine paths/programs.
fn import_custom_commands(
    store: &FsStore,
    export: &ExportFile,
    settings: &Settings,
    local_was_hs2: bool,
) -> Result<()> {
    let Some(value) = export.settings.get("custom_commands") else {
        return Ok(());
    };
    let items = decode_hs1_commands(value)?;
    let migrated = convert_hs1_commands(store, export, &items)?;
    let original = hotsheet_ticketing::commands::from_settings(settings)?;
    let mut merged = original.clone();

    for mut command in migrated {
        if original
            .iter()
            .any(|existing| equivalent_command(existing, &command))
        {
            continue;
        }
        let base = command.id.clone();
        let mut suffix = 1_usize;
        loop {
            if let Some(existing) = merged.iter().find(|existing| existing.id == command.id) {
                if equivalent_command(existing, &command) {
                    break;
                }
                suffix += 1;
                command.id = format!("{base}-{suffix}");
                continue;
            }
            merged.push(command);
            break;
        }
    }

    if merged != original {
        let commands =
            serde_json::to_value(&merged).context("serializing migrated HS1 custom commands")?;
        if local_was_hs2 {
            settings.set("commands", commands, Scope::Local)?;
        } else {
            let mut local = serde_json::Map::new();
            local.insert("commands".into(), commands);
            settings.replace_scope(Scope::Local, &local)?;
        }
    }
    // Repair stores produced by the old importer, which copied this unused HS1 key
    // into shared settings instead of creating runnable HS2 definitions.
    settings.unset("custom_commands", Scope::Shared)?;
    Ok(())
}

fn decode_hs1_commands(value: &Value) -> Result<Vec<Hs1CommandItem>> {
    let value = match value {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(value) => value,
            Err(_) => return Ok(Vec::new()),
        },
        value => value.clone(),
    };
    if !value.is_array() {
        return Ok(Vec::new());
    }
    serde_json::from_value(value).context("parsing HS1 custom_commands")
}

fn convert_hs1_commands(
    store: &FsStore,
    export: &ExportFile,
    items: &[Hs1CommandItem],
) -> Result<Vec<CommandDefinition>> {
    let legacy_groups = !items
        .iter()
        .any(|item| matches!(item, Hs1CommandItem::Group(_)))
        && items.iter().any(|item| {
            matches!(item, Hs1CommandItem::Command(command) if command.group.as_deref().is_some_and(|group| !group.trim().is_empty()))
        });
    let tool = migrated_ai_tool(export);
    let mut definitions = Vec::new();

    if legacy_groups {
        let mut ungrouped = Vec::new();
        let mut groups: Vec<(String, Vec<(usize, &Hs1Command)>)> = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let Hs1CommandItem::Command(command) = item else {
                continue;
            };
            let group = command.group.as_deref().unwrap_or_default().trim();
            if group.is_empty() {
                ungrouped.push((index, command));
            } else if let Some((_, commands)) = groups.iter_mut().find(|(name, _)| name == group) {
                commands.push((index, command));
            } else {
                groups.push((group.to_string(), vec![(index, command)]));
            }
        }
        for (index, command) in ungrouped {
            if let Some(definition) = hs1_command_definition(
                store,
                export,
                command,
                None,
                &format!("item:{index}"),
                &tool,
            )? {
                definitions.push(definition);
            }
        }
        for (group, commands) in groups {
            for (index, command) in commands {
                if let Some(definition) = hs1_command_definition(
                    store,
                    export,
                    command,
                    Some(group.clone()),
                    &format!("item:{index}"),
                    &tool,
                )? {
                    definitions.push(definition);
                }
            }
        }
        return Ok(definitions);
    }

    for (index, item) in items.iter().enumerate() {
        match item {
            Hs1CommandItem::Command(command) => {
                let group = command
                    .group
                    .as_deref()
                    .filter(|group| !group.trim().is_empty())
                    .map(str::to_owned);
                if let Some(definition) = hs1_command_definition(
                    store,
                    export,
                    command,
                    group,
                    &format!("item:{index}"),
                    &tool,
                )? {
                    definitions.push(definition);
                }
            }
            Hs1CommandItem::Group(group) => {
                if group.kind != "group" {
                    bail!("unsupported HS1 custom command item type '{}'", group.kind);
                }
                for (child_index, command) in group.children.iter().enumerate() {
                    if let Some(definition) = hs1_command_definition(
                        store,
                        export,
                        command,
                        Some(group.name.clone()),
                        &format!(
                            "group:{}:{index}/child:{child_index}",
                            group.id.as_deref().unwrap_or("")
                        ),
                        &tool,
                    )? {
                        definitions.push(definition);
                    }
                }
            }
        }
    }
    Ok(definitions)
}

fn hs1_command_definition(
    _store: &FsStore,
    _export: &ExportFile,
    command: &Hs1Command,
    group: Option<String>,
    source_key: &str,
    tool: &str,
) -> Result<Option<CommandDefinition>> {
    if command.name.trim().is_empty() || command.prompt.trim().is_empty() {
        eprintln!("warning: skipping an HS1 custom command with an empty name or prompt");
        return Ok(None);
    }
    let kind = if command.target.as_deref() == Some("shell") {
        CommandKind::Shell
    } else {
        CommandKind::Ai
    };
    let identity = format!(
        "{source_key}\0{}",
        command.id.as_deref().unwrap_or_default()
    );
    Ok(Some(CommandDefinition {
        id: format!("hs1-{:016x}", stable_hash(&identity)),
        title: command.name.clone(),
        kind,
        program: String::new(),
        args: Vec::new(),
        cwd: None,
        group,
        confirmation: None,
        command: (kind == CommandKind::Shell).then(|| command.prompt.clone()),
        prompt: (kind == CommandKind::Ai).then(|| command.prompt.clone()),
        tool: (kind == CommandKind::Ai).then(|| tool.to_string()),
        icon: command.icon.clone(),
        color: command.color.clone(),
    }))
}

fn migrated_ai_tool(export: &ExportFile) -> String {
    let requested = export
        .settings
        .get("ai_tool")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|tool| !tool.is_empty() && !tool.eq_ignore_ascii_case("auto"))
        .map(str::to_ascii_lowercase);
    requested
        .filter(|tool| {
            hotsheet_plugins::find(tool)
                .as_ref()
                .and_then(hotsheet_plugins::ai_tool_descriptor)
                .is_some()
        })
        .unwrap_or_else(|| "claude".to_string())
}

fn equivalent_command(left: &CommandDefinition, right: &CommandDefinition) -> bool {
    left.title == right.title
        && left.program == right.program
        && left.args == right.args
        && left.cwd == right.cwd
        && left.group == right.group
        && left.confirmation == right.confirmation
        && left.kind == right.kind
        && left.command == right.command
        && left.prompt == right.prompt
        && left.tool == right.tool
        && left.icon == right.icon
        && left.color == right.color
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Stable, time-sortable import identity: the timestamp comes from the HS1 creation
/// time and the random portion is a deterministic hash of project + source ticket.
/// This provides repeat-import safety without leaking an HS1 identifier into HS2.
fn import_id(project: &ProjectInfo, ticket: &ExportTicket, index: usize) -> Ulid {
    let source = format!(
        "hotsheet1\0{}\0{}\0{}",
        project.name.as_deref().unwrap_or(""),
        project.ticket_prefix.as_deref().unwrap_or("HS"),
        ticket
            .ticket_number
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("index:{index}"))
    );
    let hash = |seed: u64| {
        source.bytes().fold(seed, |value, byte| {
            (value ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    };
    let random = ((u128::from(hash(0xcbf2_9ce4_8422_2325)) << 64)
        | u128::from(hash(0x8422_2325_cbf2_9ce4)))
        & ((1_u128 << 80) - 1);
    let timestamp_ms = ticket
        .created_at
        .as_deref()
        .and_then(|v| Timestamp::new(v).instant())
        .and_then(|v| u64::try_from(v.unix_timestamp_nanos() / 1_000_000).ok())
        .unwrap_or(0);
    Ulid::from_parts(timestamp_ms, random)
}

/// Copy a ticket's staged attachment files into `attachments/<new-ulid>/`.
fn copy_attachments(
    store: &FsStore,
    base_dir: &Path,
    id: &Ulid,
    attachments: &[ExportAttachment],
) -> Result<usize> {
    let mut n = 0;
    for att in attachments {
        let src = base_dir.join(&att.stored_path);
        let bytes = std::fs::read(&src)
            .with_context(|| format!("reading staged attachment {}", src.display()))?;
        let filename = att
            .original_filename
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&att.stored_path);
        let attachment_id = att
            .id
            .as_deref()
            .and_then(|value| Ulid::from_string(value).ok())
            .unwrap_or_else(|| FsStore::legacy_attachment_id(id, &att.stored_path));
        let ticket = store.read_ticket(id)?;
        let created_at = att
            .created_at
            .clone()
            .map(Timestamp::new)
            .unwrap_or(ticket.created_at);
        store.write_attachment(id, attachment_id, created_at, filename, &bytes)?;
        n += 1;
    }
    Ok(n)
}

fn build_ticket(
    src: &ExportTicket,
    id: Ulid,
    prefix: &str,
    id_by_number: &HashMap<&str, Ulid>,
) -> Ticket {
    let created = src.created_at.clone().unwrap_or_default();
    let updated = src.updated_at.clone().unwrap_or_else(|| created.clone());

    let mut t = Ticket::new(
        id,
        derive_slug(&id, prefix),
        src.title.clone(),
        src.category.clone().unwrap_or_else(|| "issue".to_string()),
        created,
        updated,
    );
    t.priority = parse_priority(src.priority.as_deref());
    t.status = parse_status(src.status.as_deref());
    t.up_next = src.up_next;
    t.tags = src.tags.clone();
    t.details = src.details.clone().unwrap_or_default();
    t.completed_at = src.completed_at.clone().map(Timestamp::from);
    t.verified_at = src.verified_at.clone().map(Timestamp::from);
    // Normalize HS1 terminal states into a coherent HS2 status + close outcome.
    // Backlog is inactive but not closed; deleted/archive are no longer actionable.
    t.up_next &= t.status.is_active();
    match t.status {
        Status::Completed | Status::Verified => {
            t.closed_at = t
                .verified_at
                .clone()
                .or_else(|| t.completed_at.clone())
                .or_else(|| Some(t.updated_at.clone()));
            t.close_reason = Some(CloseReason::Completed);
        }
        Status::Deleted | Status::Archive => {
            t.closed_at = src
                .deleted_at
                .clone()
                .map(Timestamp::from)
                .or_else(|| Some(t.updated_at.clone()));
            t.close_reason = Some(CloseReason::Obsolete);
        }
        _ => {}
    }

    // Remap dependency edges; drop refs to tickets outside this export.
    t.blocked_by = src
        .blocked_by
        .iter()
        .filter_map(|n| id_by_number.get(n.as_str()).copied())
        .collect();

    t.notes = src
        .notes
        .iter()
        .map(|n| {
            let id =
                n.id.as_deref()
                    .and_then(|s| Ulid::from_string(s).ok())
                    .unwrap_or_else(Ulid::new);
            let created_at = n.created_at.clone().map(Timestamp::new).unwrap_or_else(|| {
                Timestamp::from_datetime(
                    time::OffsetDateTime::from_unix_timestamp_nanos(
                        i128::from(id.timestamp_ms()) * 1_000_000,
                    )
                    .unwrap_or(time::OffsetDateTime::UNIX_EPOCH),
                )
            });
            Note {
                id,
                kind: NoteKind::Regular,
                created_at: created_at.clone(),
                edited_at: created_at,
                summary: None,
                text: n.text.clone(),
            }
        })
        .collect();

    t
}

fn parse_priority(s: Option<&str>) -> Priority {
    match s {
        Some("highest") => Priority::Highest,
        Some("high") => Priority::High,
        Some("low") => Priority::Low,
        Some("lowest") => Priority::Lowest,
        _ => Priority::Default,
    }
}

fn parse_status(s: Option<&str>) -> Status {
    match s {
        Some("started") => Status::Started,
        Some("completed") => Status::Completed,
        Some("verified") => Status::Verified,
        Some("backlog") => Status::Backlog,
        Some("archive") => Status::Archive,
        Some("deleted") => Status::Deleted,
        Some("moved") => Status::Moved,
        _ => Status::NotStarted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_ticketing::StoreMetadata;

    fn temp_store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, store)
    }

    fn export_json() -> ExportFile {
        let json = r#"{
          "exportVersion": 1,
          "project": { "name": "Demo", "ticketPrefix": "HS" },
          "tickets": [
            {
              "ticket_number": "HS-1200", "title": "Root cause",
              "category": "bug", "priority": "high", "status": "completed",
              "up_next": false, "tags": ["ui"],
              "notes": [{ "id": "n_abc", "text": "done", "created_at": "2026-08-01T00:00:00Z" }],
              "blocked_by": [],
              "created_at": "2026-08-01T00:00:00Z", "updated_at": "2026-08-02T00:00:00Z",
              "completed_at": "2026-08-02T00:00:00Z", "verified_at": null
            },
            {
              "ticket_number": "HS-1234", "title": "Depends on 1200",
              "category": "feature", "priority": "default", "status": "started",
              "up_next": true, "tags": [],
              "notes": [],
              "blocked_by": ["HS-1200", "HS-9999"],
              "created_at": "2026-08-03T00:00:00Z", "updated_at": "2026-08-03T00:00:00Z",
              "completed_at": null, "verified_at": null
            }
          ]
        }"#;
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn imports_tickets_and_remaps_edges() {
        let (_dir, store) = temp_store();
        let summary = import(&store, &export_json(), Path::new(".")).unwrap();
        assert_eq!(
            summary,
            ImportSummary {
                written: 2,
                skipped: 0,
                attachments: 0,
            }
        );

        let tickets = store.list_tickets().unwrap();
        assert_eq!(tickets.len(), 2);

        let root = tickets
            .iter()
            .find(|t| t.title == "Root cause")
            .cloned()
            .unwrap();
        let dep = tickets
            .iter()
            .find(|t| t.title == "Depends on 1200")
            .cloned()
            .unwrap();

        // The deterministic HS2 identity is the only retained handle.
        assert!(root.slug.starts_with("HS-"));
        assert_ne!(root.slug, "HS-1200");

        // completed → close outcome mapped.
        assert_eq!(root.status, Status::Completed);
        assert_eq!(root.close_reason, Some(CloseReason::Completed));
        assert_eq!(
            root.closed_at.as_ref().map(Timestamp::as_str),
            Some("2026-08-02T00:00:00Z")
        );

        // The HS1 note id (n_abc) was replaced with a real ULID.
        assert_eq!(root.notes.len(), 1);
        assert_eq!(root.notes[0].text, "done");

        // blocked_by: HS-1200 remaps to root's ULID; the out-of-export HS-9999 is dropped.
        assert_eq!(dep.blocked_by, vec![root.id]);
        assert!(dep.up_next);
    }

    #[test]
    fn import_is_idempotent_by_deterministic_hs2_identity() {
        let (_dir, store) = temp_store();
        import(&store, &export_json(), Path::new(".")).unwrap();
        let again = import(&store, &export_json(), Path::new(".")).unwrap();
        assert_eq!(
            again,
            ImportSummary {
                written: 0,
                skipped: 2,
                attachments: 0,
            }
        );
        assert_eq!(store.list_tickets().unwrap().len(), 2, "no duplicates");
    }

    #[test]
    fn imports_non_identity_hs1_settings_as_shared_project_settings() {
        let (_dir, store) = temp_store();
        let mut export = export_json();
        export
            .settings
            .insert("appName".into(), serde_json::json!("Old name"));
        export
            .settings
            .insert("ticketPrefix".into(), serde_json::json!("OLD"));
        export
            .settings
            .insert("categories".into(), serde_json::json!(["bug", "chore"]));
        export.settings.insert(
            "customViews".into(),
            serde_json::json!({"mine": {"tag": "me"}}),
        );

        import(&store, &export, Path::new(".")).unwrap();

        let migrated = Settings::new(store.root()).map(Scope::Shared).unwrap();
        assert_eq!(
            migrated.get("categories"),
            Some(&serde_json::json!(["bug", "chore"]))
        );
        assert_eq!(
            migrated.get("customViews"),
            Some(&serde_json::json!({"mine": {"tag": "me"}}))
        );
        assert!(!migrated.contains_key("appName"));
        assert!(!migrated.contains_key("ticketPrefix"));
    }

    #[test]
    fn live_hs1_project_import_reuses_project_settings_paths_and_marks_them_hs2() {
        let (_store_dir, store) = temp_store();
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(".hotsheet")).unwrap();
        std::fs::write(
            project.path().join(".hotsheet/settings.json"),
            r#"{"appName":"Old","ticketPrefix":"OLD","user_key":7}"#,
        )
        .unwrap();
        std::fs::write(
            project.path().join(".hotsheet/settings.local.json"),
            r#"{"custom_commands":[],"hs1_only_machine_value":true}"#,
        )
        .unwrap();
        let mut export = export_json();
        export.project.source_root = Some(project.path().display().to_string());
        export
            .settings
            .insert("categories".into(), serde_json::json!(["bug", "task"]));
        export.settings.insert(
            "custom_commands".into(),
            serde_json::json!([{"id":"check","name":"Check","prompt":"true","target":"shell"}]),
        );

        import(&store, &export, Path::new(".")).unwrap();

        let shared: serde_json::Value = serde_json::from_slice(
            &std::fs::read(project.path().join(".hotsheet2/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(shared["$hotsheetSchema"], 1);
        assert_eq!(shared["user_key"], 7);
        assert_eq!(shared["categories"], serde_json::json!(["bug", "task"]));
        assert!(shared.get("appName").is_none());
        assert!(shared.get("ticketPrefix").is_none());
        let local: serde_json::Value = serde_json::from_slice(
            &std::fs::read(project.path().join(".hotsheet2/settings.local.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(local["$hotsheetSchema"], 1);
        assert_eq!(local["commands"][0]["title"], "Check");
        assert!(local.get("custom_commands").is_none());
        assert!(local.get("hs1_only_machine_value").is_none());
        assert!(!store.root().join(".hotsheet2/settings.json").exists());
        assert!(!store.root().join("hotsheet-settings.json").exists());
    }

    #[test]
    fn migrates_hs1_ai_shell_and_group_commands_to_typed_local_definitions() {
        let (dir, store) = temp_store();
        let project = dir.path().join("code project");
        std::fs::create_dir(&project).unwrap();
        let json = serde_json::json!({
            "exportVersion": 1,
            "project": {
                "ticketPrefix": "HS",
                "sourceRoot": project.display().to_string()
            },
            "settings": {
                "ai_tool": "codex",
                "custom_commands": [
                    {"id":"review","name":"Review","prompt":"Review the diff","icon":"send","color":"#3b82f6"},
                    {"type":"group","id":"checks","name":"Checks","children":[
                        {"id":"test","name":"Test","prompt":"npm test && npm run lint","target":"shell"}
                    ]}
                ]
            },
            "tickets": []
        });
        let export: ExportFile = serde_json::from_value(json).unwrap();

        import(&store, &export, Path::new(".")).unwrap();

        let settings = Settings::new(store.root());
        let commands = hotsheet_ticketing::commands::from_settings(&settings).unwrap();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].title, "Review");
        assert_eq!(commands[0].kind, CommandKind::Ai);
        assert!(commands[0].program.is_empty() && commands[0].args.is_empty());
        assert_eq!(commands[0].prompt.as_deref(), Some("Review the diff"));
        assert_eq!(commands[0].tool.as_deref(), Some("codex"));
        assert_eq!(commands[0].icon.as_deref(), Some("send"));
        assert_eq!(commands[0].color.as_deref(), Some("#3b82f6"));
        assert_eq!(commands[1].kind, CommandKind::Shell);
        assert!(commands[1].program.is_empty() && commands[1].args.is_empty());
        assert_eq!(
            commands[1].command.as_deref(),
            Some("npm test && npm run lint")
        );
        assert_eq!(commands[1].group.as_deref(), Some("Checks"));
        assert!(commands.iter().all(|command| command.cwd.is_none()));
        assert!(settings.get("commands", Scope::Local).unwrap().is_some());
        assert_eq!(
            settings.get("custom_commands", Scope::Shared).unwrap(),
            None,
            "the unused HS1 key is not committed into HS2"
        );
    }

    #[test]
    fn legacy_flat_command_groups_follow_hs1_ordering_and_unknown_tools_fall_back_to_claude() {
        let (_dir, store) = temp_store();
        let json = serde_json::json!({
            "exportVersion": 1,
            "project": {"ticketPrefix":"HS"},
            "settings": {
                "ai_tool": "an-old-editor-without-an-hs2-drive",
                "custom_commands": [
                    {"name":"Build","prompt":"build","target":"shell","group":"Checks"},
                    {"name":"Review","prompt":"review"},
                    {"name":"Lint","prompt":"lint","target":"shell","group":"Checks"},
                    {"name":"Deploy","prompt":"deploy","target":"shell","group":"Release"}
                ]
            },
            "tickets": []
        });
        let export: ExportFile = serde_json::from_value(json).unwrap();

        import(&store, &export, Path::new(".")).unwrap();

        let commands =
            hotsheet_ticketing::commands::from_settings(&Settings::new(store.root())).unwrap();
        assert_eq!(
            commands
                .iter()
                .map(|command| (command.title.as_str(), command.group.as_deref()))
                .collect::<Vec<_>>(),
            [
                ("Review", None),
                ("Build", Some("Checks")),
                ("Lint", Some("Checks")),
                ("Deploy", Some("Release")),
            ]
        );
        assert_eq!(commands[0].tool.as_deref(), Some("claude"));
    }

    #[test]
    fn command_import_is_byte_idempotent_and_preserves_id_conflicts() {
        let (_dir, store) = temp_store();
        let json = serde_json::json!({
            "exportVersion": 1,
            "project": {"ticketPrefix":"HS"},
            "settings": {
                "custom_commands": [{"id":"review","name":"Review","prompt":"Review it"}]
            },
            "tickets": []
        });
        let export: ExportFile = serde_json::from_value(json).unwrap();
        import(&store, &export, Path::new(".")).unwrap();
        let settings = Settings::new(store.root());
        let local_path = store.root().join("hotsheet-settings.local.json");
        let first_bytes = std::fs::read(&local_path).unwrap();

        import(&store, &export, Path::new(".")).unwrap();
        assert_eq!(std::fs::read(&local_path).unwrap(), first_bytes);

        let mut existing = hotsheet_ticketing::commands::from_settings(&settings).unwrap();
        existing[0].title = "My edited command".into();
        settings
            .set(
                "commands",
                serde_json::to_value(&existing).unwrap(),
                Scope::Local,
            )
            .unwrap();

        import(&store, &export, Path::new(".")).unwrap();
        let after_conflict = hotsheet_ticketing::commands::from_settings(&settings).unwrap();
        assert_eq!(after_conflict.len(), 2);
        assert_eq!(after_conflict[0].title, "My edited command");
        assert_eq!(after_conflict[1].title, "Review");
        assert_eq!(after_conflict[1].id, format!("{}-2", after_conflict[0].id));

        import(&store, &export, Path::new(".")).unwrap();
        assert_eq!(
            hotsheet_ticketing::commands::from_settings(&settings).unwrap(),
            after_conflict,
            "the conflict copy is also retry-safe"
        );
    }

    #[test]
    fn imports_staged_attachments_into_the_store() {
        let (_dir, store) = temp_store();
        // A staging dir (stands in for the export JSON's directory) with one file.
        let staging = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(staging.path().join("attachments/0")).unwrap();
        std::fs::write(staging.path().join("attachments/0/shot.png"), b"PNGDATA").unwrap();

        let json = r#"{
          "exportVersion": 1,
          "project": { "ticketPrefix": "HS" },
          "tickets": [
            { "ticket_number": "HS-1", "title": "with attachment",
              "notes": [], "blocked_by": [],
              "attachments": [
                { "original_filename": "shot.png", "stored_path": "attachments/0/shot.png" }
              ] }
          ]
        }"#;
        let export: ExportFile = serde_json::from_str(json).unwrap();
        let summary = import(&store, &export, staging.path()).unwrap();
        assert_eq!(summary.written, 1);
        assert_eq!(summary.attachments, 1);

        let ticket = &store.list_tickets().unwrap()[0];
        assert_eq!(ticket.attachments.len(), 1);
        assert_eq!(ticket.attachments[0].created_at, ticket.created_at);
        assert_eq!(ticket.attachments[0].batch_id, None);
        assert_eq!(ticket.attachments[0].actor, None);
        assert_eq!(ticket.attachments[0].purpose, None);
        let file = store
            .attachment_dir(&ticket.id)
            .join(ticket.attachments[0].id.to_string())
            .join("shot.png");
        assert_eq!(std::fs::read(file).unwrap(), b"PNGDATA");
    }
}
