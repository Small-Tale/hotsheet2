//! Cross-tool **activity** events (`docs/15`, HS2-KP31ZE) — the tool-agnostic stream the
//! Announcer + a timeline consume, so narration isn't Claude-only. Each AI plugin's
//! `activity` capability maps its native signals into a common [`ActivityEvent`]; the host
//! appends them to a **bounded rolling store** (`activity/recent/<YYYY-MM-DD>.jsonl`, like
//! metrics raw — per-device, gitignored) so a **digest** can look back, and old days age out
//! ([`prune_before`]). The durable record of "what happened" is the ticket's notes + git
//! history; this stream is derived/ephemeral (`docs/15` §15.4).
//!
//! Two consumer modes over one store: **live** (subscribe as events arrive, over the WS bus)
//! and **digest** ([`timeline`] — read the stored window for a ticket/session). Same data,
//! different read.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::store::FsStore;

/// Raw activity is diagnostic input, not durable history. Keep the event day and the
/// preceding 13 calendar days on each machine; durable notes and git history outlive it.
pub const MAX_RECENT_ACTIVITY_AGE_DAYS: i64 = 14;

/// The **closed** activity-kind vocabulary (`docs/15` §15.2) — not free-form, so consumers
/// style/emphasize consistently. Serialized as its snake-ish lowercase string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    TurnStart,
    Plan,
    Edit,
    Command,
    ToolCall,
    Decision,
    Blocked,
    Permission,
    Note,
    TicketStatus,
    TurnEnd,
}

impl ActivityKind {
    /// The default narration emphasis for a kind (`docs/15` §15.7 — overridable per event).
    /// High = always speak; Normal = full timeline; Low = trace-ish.
    pub fn default_importance(self) -> Importance {
        use ActivityKind::*;
        match self {
            TurnStart | TurnEnd | Blocked | Permission | Decision => Importance::High,
            Plan | Edit | Command | ToolCall | TicketStatus => Importance::Normal,
            Note => Importance::Low,
        }
    }

    /// A generic one-line summary composed from the kind (+ a `detail` hint when present) —
    /// the host default a tool may override for quality (`docs/15` §15.7).
    pub fn default_summary(self, tool: &str, detail: &Value) -> String {
        use ActivityKind::*;
        let hint = detail
            .get("path")
            .or_else(|| detail.get("command"))
            .or_else(|| detail.get("name"))
            .or_else(|| detail.get("text"))
            .and_then(Value::as_str);
        match (self, hint) {
            (Edit, Some(p)) => format!("{tool} edited {p}"),
            (Edit, None) => format!("{tool} edited a file"),
            (Command, Some(c)) => format!("{tool} ran `{c}`"),
            (Command, None) => format!("{tool} ran a command"),
            (ToolCall, Some(n)) => format!("{tool} called {n}"),
            (ToolCall, None) => format!("{tool} used a tool"),
            (TurnStart, _) => format!("{tool} started a turn"),
            (TurnEnd, _) => format!("{tool} finished a turn"),
            (Plan, Some(t)) => format!("{tool} planned: {t}"),
            (Plan, None) => format!("{tool} made a plan"),
            (Decision, Some(t)) => format!("{tool} decided: {t}"),
            (Decision, None) => format!("{tool} made a decision"),
            (Blocked, Some(t)) => format!("{tool} is blocked: {t}"),
            (Blocked, None) => format!("{tool} is blocked"),
            (Permission, _) => format!("{tool} is asking permission"),
            (Note, Some(t)) => t.to_string(),
            (Note, None) => format!("{tool} left a note"),
            (TicketStatus, Some(t)) => format!("ticket status: {t}"),
            (TicketStatus, None) => "ticket status changed".to_string(),
        }
    }
}

/// Narration emphasis (`docs/15` §15.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Importance {
    Low,
    Normal,
    High,
}

/// One activity event — the common shape every tool's signals map into (`docs/15` §15.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityEvent {
    /// ULID string — ordering + de-dup.
    pub id: String,
    /// RFC3339 timestamp (its date picks the rolling file).
    pub ts: String,
    /// Plugin id (`claude`, `codex`, …).
    pub tool: String,
    /// The active project (ULID string), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The active ticket (ULID string) when attributable, else `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    /// The connection/session that produced it, so a multi-worker setup narrates per source
    /// (`docs/15` §15.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub kind: ActivityKind,
    /// The one-line, narration-ready string (`docs/15` §15.2).
    pub summary: String,
    /// Kind-specific structured extras a timeline can expand.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub detail: Value,
    pub importance: Importance,
}

impl ActivityEvent {
    /// Build an event, composing a default `summary` + `importance` from the kind (the host
    /// default; a caller may then override either). `id`/`ts` are supplied so the caller
    /// controls ordering + the injected clock/ULID (no globals here, for testability).
    pub fn new(
        id: impl Into<String>,
        ts: impl Into<String>,
        tool: impl Into<String>,
        kind: ActivityKind,
        detail: Value,
    ) -> Self {
        let tool = tool.into();
        ActivityEvent {
            id: id.into(),
            ts: ts.into(),
            summary: kind.default_summary(&tool, &detail),
            importance: kind.default_importance(),
            tool,
            project: None,
            ticket: None,
            session: None,
            kind,
            detail,
        }
    }

    /// The `YYYY-MM-DD` day the event belongs to (from `ts`), or `"unknown"`.
    fn day(&self) -> &str {
        self.ts.get(..10).unwrap_or("unknown")
    }
}

/// Append an activity event to the store's rolling JSONL for its day
/// (`activity/recent/<day>.jsonl`), creating the dir and gitignoring `activity/recent/`
/// (per-device, ephemeral).
pub fn record(store: &FsStore, event: &ActivityEvent) -> io::Result<()> {
    let dir = store.root().join("activity").join("recent");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.jsonl", event.day()));
    let line = serde_json::to_string(event).map_err(io::Error::other)?;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{line}")?;
    drop(f);
    ensure_gitignored(store.root(), "activity/recent/")?;
    prune_by_age(&dir, event.day(), MAX_RECENT_ACTIVITY_AGE_DAYS)?;
    Ok(())
}

fn prune_by_age(dir: &Path, reference_day: &str, retain_days: i64) -> io::Result<()> {
    let Some(mut reference) = hotsheet_model::Timestamp::new(format!("{reference_day}T00:00:00Z"))
        .instant()
        .map(|instant| instant.date())
    else {
        return Ok(());
    };
    let dated_paths: Vec<_> = fs::read_dir(dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter_map(|path| {
            let day = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|day| {
                    hotsheet_model::Timestamp::new(format!("{day}T00:00:00Z"))
                        .instant()
                        .map(|instant| instant.date())
                })?;
            (path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")).then_some((path, day))
        })
        .collect();
    if let Some(latest) = dated_paths.iter().map(|(_, day)| *day).max() {
        reference = reference.max(latest);
    }
    let cutoff = reference - time::Duration::days(retain_days.saturating_sub(1));
    for (path, day) in dated_paths {
        if day < cutoff {
            let _ = remove_if_present(&path)?;
        }
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> io::Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Read every stored activity event in **recording order** — day-files ascending, each file's
/// lines in append order — skipping unparseable lines. Append order *is* chronological for a
/// live stream, and (unlike sorting by `id`) it's deterministic for events recorded in the
/// same millisecond, since `Ulid::new()` isn't monotonic within a ms. The bounded recent
/// window a digest reads.
pub fn read_recent(store: &FsStore) -> io::Result<Vec<ActivityEvent>> {
    let dir = store.root().join("activity").join("recent");
    let mut events = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(events); // no activity yet
    };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .collect();
    files.sort(); // `YYYY-MM-DD.jsonl` names sort chronologically by day
    for path in files {
        let text = fs::read_to_string(&path)?;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if let Ok(ev) = serde_json::from_str::<ActivityEvent>(line) {
                events.push(ev);
            }
        }
    }
    Ok(events)
}

/// A timeline query: the "what happened" window for a ticket and/or session (`docs/15` §15.6).
#[derive(Debug, Clone, Default)]
pub struct TimelineFilter {
    /// Only events attributed to this ticket ULID.
    pub ticket: Option<String>,
    /// Only events from this connection/session.
    pub session: Option<String>,
    /// Only events at or above this importance (e.g. `High` for a terse digest).
    pub min_importance: Option<Importance>,
    /// Cap to the most recent N (after filtering + ordering).
    pub limit: Option<usize>,
}

/// The per-ticket / per-session **timeline** — read the stored window, filter, order, cap
/// (`docs/15` §15.6). The cheap first consumer of the stream.
pub fn timeline(store: &FsStore, filter: &TimelineFilter) -> io::Result<Vec<ActivityEvent>> {
    let rank = |i: Importance| match i {
        Importance::Low => 0,
        Importance::Normal => 1,
        Importance::High => 2,
    };
    let mut events: Vec<ActivityEvent> = read_recent(store)?
        .into_iter()
        .filter(|e| {
            filter
                .ticket
                .as_deref()
                .is_none_or(|t| e.ticket.as_deref() == Some(t))
        })
        .filter(|e| {
            filter
                .session
                .as_deref()
                .is_none_or(|s| e.session.as_deref() == Some(s))
        })
        .filter(|e| {
            filter
                .min_importance
                .is_none_or(|m| rank(e.importance) >= rank(m))
        })
        .collect();
    // Already in recording order from read_recent; cap keeps the most RECENT N (the tail).
    if let Some(n) = filter.limit {
        if events.len() > n {
            events.drain(..events.len() - n);
        }
    }
    Ok(events)
}

/// Drop rolling files strictly older than `keep_from` (`YYYY-MM-DD`) — old events age out
/// (`docs/15` §15.4). Returns how many day-files were removed.
pub fn prune_before(store: &FsStore, keep_from: &str) -> io::Result<usize> {
    let dir = store.root().join("activity").join("recent");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(0);
    };
    let mut removed = 0;
    for path in entries.flatten().map(|e| e.path()) {
        let is_old = path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|day| day < keep_from);
        if is_old
            && path.extension().and_then(|e| e.to_str()) == Some("jsonl")
            && remove_if_present(&path)?
        {
            removed += 1;
        }
    }
    Ok(removed)
}

// ---- per-tool mappers (native signal → common event) -----------------------------
//
// Pure `&Value → ActivityEvent` (no I/O, no globals) so they sample-test directly. The host
// supplies `id`/`ts` (its injected ULID/clock) and tags project/ticket/session after. `docs/15`
// §15.3. The native field names for codex should be confirmed against a live tool (like the
// usage mappers) — a mismatch yields `None`, never a crash.

/// Map a **Claude** hook event (the same JSON the PreToolUse permission hook receives —
/// `hook_event_name`, `tool_name`, `tool_input`, `session_id`) into an activity event
/// (`docs/15` §15.3). Returns `None` for an event with no narratable mapping.
pub fn claude_activity(hook: &Value, id: &str, ts: &str) -> Option<ActivityEvent> {
    let event = hook.get("hook_event_name").and_then(Value::as_str);
    let session = hook
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let (kind, detail) = match event {
        Some("UserPromptSubmit") => (ActivityKind::TurnStart, Value::Null),
        Some("Stop") | Some("SubagentStop") => (ActivityKind::TurnEnd, Value::Null),
        Some("PreToolUse") => {
            let tool_name = hook.get("tool_name").and_then(Value::as_str).unwrap_or("");
            let input = hook.get("tool_input");
            let command = input.and_then(|t| t.get("command")).and_then(Value::as_str);
            let path = input
                .and_then(|t| t.get("file_path").or_else(|| t.get("path")))
                .and_then(Value::as_str);
            match tool_name {
                "Bash" => (
                    ActivityKind::Command,
                    serde_json::json!({ "command": command.unwrap_or("") }),
                ),
                "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => (
                    ActivityKind::Edit,
                    serde_json::json!({ "path": path.unwrap_or("") }),
                ),
                other => (ActivityKind::ToolCall, serde_json::json!({ "name": other })),
            }
        }
        _ => return None,
    };
    let mut ev = ActivityEvent::new(id, ts, "claude", kind, detail);
    ev.session = session;
    Some(ev)
}

/// Map a **codex** app-server transcript item (`type` + kind-specific fields) into an activity
/// event (`docs/15` §15.3). The camelCase variants are pinned to Codex 0.152.1's generated
/// protocol schema; legacy aliases remain lenient for older app servers. Unknown items are
/// skipped rather than failing the turn.
pub fn codex_activity(item: &Value, id: &str, ts: &str) -> Option<ActivityEvent> {
    let ty = item.get("type").or_else(|| item.get("item_type"));
    let ty = ty.and_then(Value::as_str)?;
    let (kind, detail) = match ty {
        "command" | "exec" | "shell" | "commandExecution" => (
            ActivityKind::Command,
            serde_json::json!({ "command": item.get("command").and_then(Value::as_str).unwrap_or("") }),
        ),
        "edit" | "patch" | "file_change" | "fileChange" => (
            ActivityKind::Edit,
            serde_json::json!({ "path": item.get("path").and_then(Value::as_str).or_else(|| {
                item.get("changes").and_then(Value::as_array)?.first()?.get("path")?.as_str()
            }).unwrap_or("") }),
        ),
        "reasoning" | "decision" | "plan" => {
            let text = item
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    item.get("summary").and_then(Value::as_array).map(|parts| {
                        parts
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                })
                .unwrap_or_default();
            (
                if ty == "plan" {
                    ActivityKind::Plan
                } else {
                    ActivityKind::Decision
                },
                serde_json::json!({ "text": text }),
            )
        }
        "turn_end" | "completed" => (ActivityKind::TurnEnd, Value::Null),
        "mcpToolCall" | "dynamicToolCall" | "collabAgentToolCall" => {
            let tool = item.get("tool").and_then(Value::as_str).unwrap_or("");
            let server = item.get("server").and_then(Value::as_str);
            let name = server.map_or_else(|| tool.to_string(), |server| format!("{server}.{tool}"));
            (ActivityKind::ToolCall, serde_json::json!({ "name": name }))
        }
        "webSearch" => (
            ActivityKind::ToolCall,
            serde_json::json!({ "name": "web search", "query": item.get("query").and_then(Value::as_str).unwrap_or("") }),
        ),
        "imageView" => (
            ActivityKind::ToolCall,
            serde_json::json!({ "name": "image view", "path": item.get("path").and_then(Value::as_str).unwrap_or("") }),
        ),
        _ => return None,
    };
    Some(ActivityEvent::new(id, ts, "codex", kind, detail))
}

/// Append a line to the store's `.gitignore` if not already present (idempotent).
fn ensure_gitignored(root: &Path, name: &str) -> io::Result<()> {
    let gi = root.join(".gitignore");
    let existing = fs::read_to_string(&gi).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == name) {
        return Ok(());
    }
    let mut f = fs::OpenOptions::new().create(true).append(true).open(&gi)?;
    writeln!(f, "{name}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreMetadata;
    use serde_json::json;

    fn store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, store)
    }

    fn ev(id: &str, day: &str, kind: ActivityKind, detail: Value) -> ActivityEvent {
        ActivityEvent::new(id, format!("{day}T00:00:00Z"), "codex", kind, detail)
    }

    #[test]
    fn new_composes_default_summary_and_importance_from_kind() {
        let e = ev(
            "01A",
            "2026-08-19",
            ActivityKind::Edit,
            json!({"path": "src/x.rs"}),
        );
        assert_eq!(e.summary, "codex edited src/x.rs");
        assert_eq!(e.importance, Importance::Normal);

        let e = ev("01B", "2026-08-19", ActivityKind::TurnEnd, Value::Null);
        assert_eq!(e.summary, "codex finished a turn");
        assert_eq!(e.importance, Importance::High);
    }

    #[test]
    fn record_read_round_trips_in_append_order_and_gitignores() {
        let (_d, store) = store();
        // Append same-day events whose ids DESCEND (01Z then 01A) plus a later day. Read must
        // come back in APPEND order (01Z, 01A, then the next day) — NOT id-sorted — since
        // append order is the true chronological order for a live stream.
        record(
            &store,
            &ev("01Z", "2026-08-19", ActivityKind::TurnStart, Value::Null),
        )
        .unwrap();
        record(
            &store,
            &ev(
                "01A",
                "2026-08-19",
                ActivityKind::Edit,
                json!({"path": "a"}),
            ),
        )
        .unwrap();
        record(
            &store,
            &ev(
                "01C",
                "2026-08-20",
                ActivityKind::Command,
                json!({"command": "ls"}),
            ),
        )
        .unwrap();

        let all = read_recent(&store).unwrap();
        assert_eq!(
            all.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["01Z", "01A", "01C"],
            "events come back in append (recording) order, not sorted by id"
        );
        // Both day files exist and the dir is gitignored.
        assert!(
            store
                .root()
                .join("activity/recent/2026-08-19.jsonl")
                .is_file()
        );
        assert!(
            store
                .root()
                .join("activity/recent/2026-08-20.jsonl")
                .is_file()
        );
        let gi = std::fs::read_to_string(store.root().join(".gitignore")).unwrap();
        assert!(gi.lines().any(|l| l.trim() == "activity/recent/"));
    }

    #[test]
    fn timeline_filters_by_ticket_session_importance_and_caps_to_recent() {
        let (_d, store) = store();
        let mut a = ev(
            "01A",
            "2026-08-19",
            ActivityKind::Edit,
            json!({"path": "a"}),
        );
        a.ticket = Some("T1".into());
        a.session = Some("s1".into());
        let mut b = ev("01B", "2026-08-19", ActivityKind::TurnEnd, Value::Null); // High
        b.ticket = Some("T1".into());
        b.session = Some("s1".into());
        let mut c = ev(
            "01C",
            "2026-08-19",
            ActivityKind::Edit,
            json!({"path": "c"}),
        );
        c.ticket = Some("T2".into()); // different ticket
        for e in [&a, &b, &c] {
            record(&store, e).unwrap();
        }

        // Per-ticket T1 → a + b, ordered.
        let t1 = timeline(
            &store,
            &TimelineFilter {
                ticket: Some("T1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            t1.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["01A", "01B"]
        );

        // High-only digest of T1 → just the turn_end.
        let hi = timeline(
            &store,
            &TimelineFilter {
                ticket: Some("T1".into()),
                min_importance: Some(Importance::High),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            hi.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["01B"]
        );

        // limit keeps the most RECENT.
        let last = timeline(
            &store,
            &TimelineFilter {
                session: Some("s1".into()),
                limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            last.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["01B"]
        );
    }

    #[test]
    fn claude_activity_maps_hook_events() {
        // PreToolUse Bash → command.
        let e = claude_activity(
            &json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Bash",
                "tool_input": { "command": "cargo test" },
                "session_id": "s1"
            }),
            "01A",
            "2026-08-19T00:00:00Z",
        )
        .unwrap();
        assert_eq!(e.kind, ActivityKind::Command);
        assert_eq!(e.summary, "claude ran `cargo test`");
        assert_eq!(e.session.as_deref(), Some("s1"));

        // PreToolUse Edit → edit with the path.
        let e = claude_activity(
            &json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "Edit",
                "tool_input": { "file_path": "src/main.rs" }
            }),
            "01B",
            "t",
        )
        .unwrap();
        assert_eq!(e.kind, ActivityKind::Edit);
        assert_eq!(e.summary, "claude edited src/main.rs");

        // Prompt submit / stop map to turn boundaries.
        assert_eq!(
            claude_activity(
                &json!({ "hook_event_name": "UserPromptSubmit" }),
                "01C",
                "t"
            )
            .unwrap()
            .kind,
            ActivityKind::TurnStart
        );
        assert_eq!(
            claude_activity(&json!({ "hook_event_name": "Stop" }), "01D", "t")
                .unwrap()
                .kind,
            ActivityKind::TurnEnd
        );
        // An unmapped event is skipped, not a crash.
        assert!(
            claude_activity(&json!({ "hook_event_name": "Notification" }), "01E", "t").is_none()
        );
    }

    #[test]
    fn codex_activity_maps_transcript_items_leniently() {
        let e =
            codex_activity(&json!({ "type": "exec", "command": "ls -la" }), "01A", "t").unwrap();
        assert_eq!(e.kind, ActivityKind::Command);
        assert_eq!(e.summary, "codex ran `ls -la`");

        let e = codex_activity(&json!({ "type": "patch", "path": "a.rs" }), "01B", "t").unwrap();
        assert_eq!(e.kind, ActivityKind::Edit);

        let e = codex_activity(
            &json!({ "type": "plan", "text": "do X then Y" }),
            "01C",
            "t",
        )
        .unwrap();
        assert_eq!(e.kind, ActivityKind::Plan);

        // Unknown / missing type → None.
        assert!(codex_activity(&json!({ "type": "mystery" }), "01D", "t").is_none());
        assert!(codex_activity(&json!({ "foo": 1 }), "01E", "t").is_none());
    }

    #[test]
    fn prune_before_ages_out_old_day_files_only() {
        let (_d, store) = store();
        record(
            &store,
            &ev("01A", "2026-08-18", ActivityKind::Note, Value::Null),
        )
        .unwrap();
        record(
            &store,
            &ev("01B", "2026-08-20", ActivityKind::Note, Value::Null),
        )
        .unwrap();
        let removed = prune_before(&store, "2026-08-19").unwrap();
        assert_eq!(removed, 1);
        assert!(
            !store
                .root()
                .join("activity/recent/2026-08-18.jsonl")
                .exists()
        );
        assert!(
            store
                .root()
                .join("activity/recent/2026-08-20.jsonl")
                .exists()
        );
        // The surviving event is still readable.
        assert_eq!(read_recent(&store).unwrap().len(), 1);
    }

    #[test]
    fn recording_automatically_prunes_sparse_files_by_calendar_age() {
        let (_d, store) = store();
        for day in ["2026-01-01", "2026-07-31", "2026-08-01", "2026-08-14"] {
            record(&store, &ev(day, day, ActivityKind::Edit, Value::Null)).unwrap();
        }
        assert!(
            !store
                .root()
                .join("activity/recent/2026-01-01.jsonl")
                .exists()
        );
        assert!(
            store
                .root()
                .join("activity/recent/2026-08-01.jsonl")
                .exists()
        );
        assert!(
            store
                .root()
                .join("activity/recent/2026-08-14.jsonl")
                .exists()
        );
    }

    #[test]
    fn concurrent_prune_removal_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.jsonl");
        fs::write(&path, "event\n").unwrap();
        assert!(remove_if_present(&path).unwrap());
        assert!(!remove_if_present(&path).unwrap());
    }

    #[test]
    fn late_old_event_is_removed_against_the_newest_recorded_day_without_error() {
        let (_d, store) = store();
        record(
            &store,
            &ev("new", "2026-08-14", ActivityKind::Edit, Value::Null),
        )
        .unwrap();
        record(
            &store,
            &ev("late", "2026-01-01", ActivityKind::Edit, Value::Null),
        )
        .unwrap();
        assert!(
            !store
                .root()
                .join("activity/recent/2026-01-01.jsonl")
                .exists()
        );
        assert_eq!(read_recent(&store).unwrap().len(), 1);
    }
}
