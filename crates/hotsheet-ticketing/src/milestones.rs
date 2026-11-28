//! The project milestone registry (HS2-9PYNCN, docs/38 §38.2): one committed file per
//! milestone under the code checkout, `<checkout>/.hotsheet2/milestones/<id>.json`.
//!
//! A milestone stores only its own record: title, optional Markdown description, optional
//! due date, and open or closed state (with why and when it closed). Everything else a
//! milestone shows is derived from its tickets when read (HS2-A820C1).
//!
//! - **Format.** Pretty-printed with two-space indentation, the known keys in a fixed order
//!   (`title`, `description`, `due_on`, `state`, `closed_reason`, `closed_at`), one key per
//!   line and a trailing newline, so a diff names exactly the field that changed. Keys this
//!   build does not know are kept verbatim after the known ones (docs/19 §19.2).
//! - **Writes** are atomic, serialized on one machine by `.hotsheet2/milestones.lock`, and
//!   committed and published by the group registry's machinery ([`crate::groups::git_sync`],
//!   docs/35 §35.7). A write that changes nothing writes and commits nothing.
//! - **Titles** are unique without regard to case or whitespace. Two clones that each create
//!   one title leave two ids; both are listed and flagged `duplicate`.
//! - **Conflicts.** A file holding Git conflict markers, or one that does not parse as a
//!   milestone, reads as `state: conflicted` and refuses every write
//!   (`milestone_registry_conflict`) until the person resolves it with Git. Hot Sheet never
//!   picks a side.
//!
//! Every surface (server, CLI, MCP) calls this module so they agree.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::file_lock::FileLock;
pub use crate::groups::git_sync::MILESTONE_REGISTRY_DIR;
use crate::groups::git_sync::{self, RegistryKind, RegistrySyncStatus};

/// Machine-local lock beside the registry; `.hotsheet2/*` stays ignored, so it is never
/// committed.
pub const MILESTONE_REGISTRY_LOCK: &str = ".hotsheet2/milestones.lock";
/// The longest title, in characters, after normalization.
pub const MILESTONE_TITLE_MAX_CHARS: usize = 120;

/// The known keys, in the order they are written.
const KNOWN_KEYS: [&str; 6] = [
    "title",
    "description",
    "due_on",
    "state",
    "closed_reason",
    "closed_at",
];

/// Whether `value` has the shape of a milestone id: ten uppercase Crockford base32
/// characters, the group id shape.
pub fn is_milestone_id(value: &str) -> bool {
    hotsheet_model::is_group_id(value)
}

/// A new random milestone id.
pub fn new_milestone_id() -> String {
    hotsheet_model::new_group_id()
}

/// The display title of an id whose file cannot be read as a milestone.
pub fn milestone_placeholder_title(id: &str) -> String {
    format!("Milestone {id}")
}

/// The registry-relative path of one milestone's file, for messages (never machine-local).
pub fn milestone_file(id: &str) -> String {
    format!("{MILESTONE_REGISTRY_DIR}/{id}.json")
}

/// Why a milestone operation was refused. Each variant has a stable [`code`](Self::code),
/// which every message also names so CLI and MCP callers can match on it.
#[derive(Debug, thiserror::Error)]
pub enum MilestoneError {
    #[error("milestone registry {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{0} (invalid_milestone_title)")]
    InvalidTitle(String),
    #[error("due date '{0}' is not a calendar date in the form YYYY-MM-DD (invalid_due_on)")]
    InvalidDueOn(String),
    #[error(
        "close reason '{0}' is not one of completed or not_planned (invalid_milestone_close_reason)"
    )]
    InvalidCloseReason(String),
    #[error("no milestone matches '{0}' (milestone_not_found)")]
    NotFound(String),
    #[error("several milestones are titled '{0}'; use a milestone id (milestone_ambiguous)")]
    Ambiguous(String),
    #[error("milestone {id} is already titled '{title}' (milestone_title_taken)")]
    TitleTaken { title: String, id: String },
    #[error(
        "milestone file {file} {detail}; resolve it with Git before changing this milestone \
         (milestone_registry_conflict)"
    )]
    Conflict { file: String, detail: String },
    #[error(
        "milestones need the project's code checkout, which holds the milestone registry: run \
         this inside a code checkout linked to the store, or name a checkout \
         (milestone_registry_required)"
    )]
    RegistryRequired,
}

impl MilestoneError {
    /// The stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "milestone_registry_io",
            Self::InvalidTitle(_) => "invalid_milestone_title",
            Self::InvalidDueOn(_) => "invalid_due_on",
            Self::InvalidCloseReason(_) => "invalid_milestone_close_reason",
            Self::NotFound(_) => "milestone_not_found",
            Self::Ambiguous(_) => "milestone_ambiguous",
            Self::TitleTaken { .. } => "milestone_title_taken",
            Self::Conflict { .. } => "milestone_registry_conflict",
            Self::RegistryRequired => "milestone_registry_required",
        }
    }
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> MilestoneError + '_ {
    move |source| MilestoneError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Normalize a requested title: trimmed, inner whitespace runs collapsed to one space, a
/// single line of 1 to 120 characters.
pub fn normalize_milestone_title(input: &str) -> Result<String, MilestoneError> {
    if input.chars().any(char::is_control) {
        return Err(MilestoneError::InvalidTitle(
            "a milestone title must be one line without control characters".into(),
        ));
    }
    let title = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        return Err(MilestoneError::InvalidTitle(
            "a milestone needs a title".into(),
        ));
    }
    if title.chars().count() > MILESTONE_TITLE_MAX_CHARS {
        return Err(MilestoneError::InvalidTitle(format!(
            "a milestone title must be at most {MILESTONE_TITLE_MAX_CHARS} characters"
        )));
    }
    Ok(title)
}

/// The comparison key of a title: titles are unique without regard to case or whitespace.
pub fn milestone_title_key(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Validate a `YYYY-MM-DD` calendar date (surrounding whitespace is ignored).
pub fn validate_due_on(input: &str) -> Result<String, MilestoneError> {
    let date = input.trim();
    let invalid = || MilestoneError::InvalidDueOn(date.to_string());
    let bytes = date.as_bytes();
    let shaped = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit());
    if !shaped {
        return Err(invalid());
    }
    let year: i32 = date[..4].parse().map_err(|_| invalid())?;
    let month: u8 = date[5..7].parse().map_err(|_| invalid())?;
    let day: u8 = date[8..].parse().map_err(|_| invalid())?;
    let month = time::Month::try_from(month).map_err(|_| invalid())?;
    time::Date::from_calendar_date(year, month, day).map_err(|_| invalid())?;
    Ok(date.to_string())
}

/// A milestone's state. `conflicted` is never stored: it is how a file that holds conflict
/// markers or does not parse reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MilestoneState {
    Open,
    Closed,
    Conflicted,
    /// Never stored: an id that tickets name but the project's registry does not (another
    /// project's store, a registry not yet pulled, a deleted milestone; HS2-A820C1).
    Unknown,
}

/// Why a milestone was closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    /// The default of Close Milestone…
    #[default]
    Completed,
    NotPlanned,
}

impl CloseReason {
    /// The stored spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::NotPlanned => "not_planned",
        }
    }

    /// Parse a requested reason (`not-planned` is accepted for the CLI's spelling).
    pub fn parse(input: &str) -> Result<Self, MilestoneError> {
        match input.trim() {
            "completed" => Ok(Self::Completed),
            "not_planned" | "not-planned" => Ok(Self::NotPlanned),
            other => Err(MilestoneError::InvalidCloseReason(other.to_string())),
        }
    }
}

/// One milestone as every surface reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Milestone {
    pub id: String,
    /// The stored title, or `Milestone <id>` while the file is conflicted.
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_on: Option<String>,
    pub state: MilestoneState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_reason: Option<CloseReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    /// Another milestone has the same title key, as after two clones each created it.
    pub duplicate: bool,
    /// While conflicted: the repository-relative file and what is wrong with it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict: Option<MilestoneConflict>,
    /// Whether the project's registry names this id. `false` for an unknown id that
    /// tickets name, listed as `Milestone <id>` with `state: unknown` (HS2-A820C1).
    pub named: bool,
}

impl Milestone {
    /// The placeholder for an id the registry does not name (docs/38 §38.2 "Unknown ids").
    pub fn unknown(id: &str) -> Self {
        Self {
            id: id.to_string(),
            title: milestone_placeholder_title(id),
            description: None,
            due_on: None,
            state: MilestoneState::Unknown,
            closed_reason: None,
            closed_at: None,
            duplicate: false,
            conflict: None,
            named: false,
        }
    }
}

/// Why a milestone file reads as conflicted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MilestoneConflict {
    /// The file, relative to the checkout root.
    pub file: String,
    /// What is wrong, for people.
    pub detail: String,
}

/// A requested change to a milestone's fields. `None` leaves a field as it is; for
/// `description` and `due_on`, `Some(None)` clears it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MilestoneEdit {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub due_on: Option<Option<String>>,
}

/// The result of a write: the milestone afterwards, and whether anything changed (and so
/// was written and committed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MilestoneWrite {
    pub milestone: Milestone,
    pub changed: bool,
}

/// A parsed milestone file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    title: String,
    description: Option<String>,
    due_on: Option<String>,
    closed: Option<(CloseReason, Option<String>)>,
    /// Keys this build does not know, kept verbatim.
    extra: Map<String, Value>,
}

/// What one file holds.
#[derive(Debug)]
enum Read {
    Missing,
    Conflicted(String),
    Entry(Entry),
}

/// Whether `text` holds Git conflict markers (a line starting `<<<<<<<`, `=======` alone,
/// or `>>>>>>>`, or diff3's `|||||||`).
fn has_conflict_markers(text: &str) -> bool {
    text.lines().any(|line| {
        line.starts_with("<<<<<<<")
            || line.starts_with(">>>>>>>")
            || line.starts_with("|||||||")
            || line.trim_end() == "======="
    })
}

fn optional_text(object: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format!("has a non-text \"{key}\"")),
    }
}

/// Parse a file's text, or say why it does not read as a milestone.
fn parse_entry(text: &str) -> Result<Entry, String> {
    if has_conflict_markers(text) {
        return Err("holds Git conflict markers".into());
    }
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("is not valid JSON ({error})"))?;
    let Value::Object(mut object) = value else {
        return Err("is not a JSON object".into());
    };
    let title = optional_text(&object, "title")?
        .ok_or_else(|| "has no \"title\"".to_string())
        .and_then(|title| {
            normalize_milestone_title(&title).map_err(|_| "has an invalid \"title\"".to_string())
        })?;
    let description = optional_text(&object, "description")?.filter(|text| !text.is_empty());
    let due_on = optional_text(&object, "due_on")?
        .map(|date| validate_due_on(&date).map_err(|_| "has an invalid \"due_on\"".to_string()))
        .transpose()?;
    let state = optional_text(&object, "state")?.ok_or_else(|| "has no \"state\"".to_string())?;
    let closed = match state.as_str() {
        "open" => None,
        "closed" => {
            let reason = optional_text(&object, "closed_reason")?
                .map(|reason| {
                    CloseReason::parse(&reason)
                        .map_err(|_| "has an invalid \"closed_reason\"".to_string())
                })
                .transpose()?
                .unwrap_or_default();
            let at = optional_text(&object, "closed_at")?;
            if let Some(at) = &at {
                time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339)
                    .map_err(|_| "has an invalid \"closed_at\"".to_string())?;
            }
            Some((reason, at))
        }
        _ => return Err("has an invalid \"state\"".into()),
    };
    for key in KNOWN_KEYS {
        object.remove(key);
    }
    Ok(Entry {
        title,
        description,
        due_on,
        closed,
        extra: object,
    })
}

/// The file text of `entry`: known keys in order, then unknown keys, one per line.
fn render_entry(entry: &Entry) -> String {
    let mut fields: Vec<(String, Value)> = vec![("title".into(), entry.title.clone().into())];
    if let Some(description) = &entry.description {
        fields.push(("description".into(), description.clone().into()));
    }
    if let Some(due_on) = &entry.due_on {
        fields.push(("due_on".into(), due_on.clone().into()));
    }
    match &entry.closed {
        None => fields.push(("state".into(), "open".into())),
        Some((reason, at)) => {
            fields.push(("state".into(), "closed".into()));
            fields.push(("closed_reason".into(), reason.as_str().into()));
            if let Some(at) = at {
                fields.push(("closed_at".into(), at.clone().into()));
            }
        }
    }
    fields.extend(
        entry
            .extra
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    let mut text = String::from("{\n");
    for (index, (key, value)) in fields.iter().enumerate() {
        let key = serde_json::to_string(key).unwrap_or_default();
        // A nested unknown value keeps serde's two-space layout, shifted under its key.
        let value = serde_json::to_string_pretty(value)
            .unwrap_or_default()
            .replace('\n', "\n  ");
        let comma = if index + 1 < fields.len() { "," } else { "" };
        text.push_str(&format!("  {key}: {value}{comma}\n"));
    }
    text.push_str("}\n");
    text
}

/// The current instant as stored in `closed_at`: RFC 3339 UTC to the second.
fn now_rfc3339() -> String {
    let now = time::OffsetDateTime::now_utc();
    now.replace_nanosecond(0)
        .unwrap_or(now)
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// The milestones of one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneRegistry {
    root: PathBuf,
}

impl MilestoneRegistry {
    /// The registry of the code checkout rooted at `checkout_root`. Nothing is created
    /// until the first milestone is written.
    pub fn for_checkout(checkout_root: &Path) -> Self {
        Self {
            root: checkout_root.to_path_buf(),
        }
    }

    /// The code checkout this registry belongs to.
    pub fn checkout_root(&self) -> &Path {
        &self.root
    }

    /// The directory holding the milestone files.
    pub fn dir(&self) -> PathBuf {
        self.root.join(MILESTONE_REGISTRY_DIR)
    }

    fn entry_path(&self, id: &str) -> PathBuf {
        self.dir().join(format!("{id}.json"))
    }

    /// How the registry's Git publication last went, when it was attempted.
    pub fn sync_status(&self) -> Option<RegistrySyncStatus> {
        git_sync::read_status_of(RegistryKind::Milestones, &self.root)
    }

    fn read(&self, id: &str) -> Result<Read, MilestoneError> {
        let path = self.entry_path(id);
        match fs::read(&path) {
            Ok(bytes) => Ok(match String::from_utf8(bytes) {
                Err(_) => Read::Conflicted("is not UTF-8 text".into()),
                Ok(text) => match parse_entry(&text) {
                    Ok(entry) => Read::Entry(entry),
                    Err(detail) => Read::Conflicted(detail),
                },
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Read::Missing),
            Err(error) => Err(io_error(&path)(error)),
        }
    }

    fn milestone(id: &str, read: Read) -> Option<Milestone> {
        Some(match read {
            Read::Missing => return None,
            Read::Conflicted(detail) => Milestone {
                id: id.to_string(),
                title: milestone_placeholder_title(id),
                description: None,
                due_on: None,
                state: MilestoneState::Conflicted,
                closed_reason: None,
                closed_at: None,
                duplicate: false,
                conflict: Some(MilestoneConflict {
                    file: milestone_file(id),
                    detail,
                }),
                named: true,
            },
            Read::Entry(entry) => Milestone {
                id: id.to_string(),
                title: entry.title,
                description: entry.description,
                due_on: entry.due_on,
                state: if entry.closed.is_some() {
                    MilestoneState::Closed
                } else {
                    MilestoneState::Open
                },
                closed_reason: entry.closed.as_ref().map(|(reason, _)| *reason),
                closed_at: entry.closed.and_then(|(_, at)| at),
                duplicate: false,
                conflict: None,
                named: true,
            },
        })
    }

    /// Every milestone, open, closed, and conflicted, with `duplicate` flags: due date
    /// first (undated last), then title, then id. A missing directory is an empty
    /// registry; files whose names are not ids are skipped.
    pub fn list(&self) -> Result<Vec<Milestone>, MilestoneError> {
        let dir = self.dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(io_error(&dir)(error)),
        };
        let mut milestones = Vec::new();
        for entry in entries {
            let path = entry.map_err(io_error(&dir))?.path();
            let Some(id) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".json"))
                .filter(|id| is_milestone_id(id))
            else {
                continue;
            };
            if let Some(milestone) = Self::milestone(id, self.read(id)?) {
                milestones.push(milestone);
            }
        }
        let keys: Vec<Option<String>> = milestones
            .iter()
            .map(|milestone| {
                (milestone.state != MilestoneState::Conflicted)
                    .then(|| milestone_title_key(&milestone.title))
            })
            .collect();
        for (index, milestone) in milestones.iter_mut().enumerate() {
            milestone.duplicate = keys[index].as_ref().is_some_and(|key| {
                keys.iter()
                    .enumerate()
                    .any(|(other, candidate)| other != index && candidate.as_ref() == Some(key))
            });
        }
        milestones.sort_by(|a, b| {
            (
                a.due_on.is_none(),
                &a.due_on,
                milestone_title_key(&a.title),
                &a.id,
            )
                .cmp(&(
                    b.due_on.is_none(),
                    &b.due_on,
                    milestone_title_key(&b.title),
                    &b.id,
                ))
        });
        Ok(milestones)
    }

    /// One milestone by id, with its `duplicate` flag.
    pub fn get(&self, id: &str) -> Result<Option<Milestone>, MilestoneError> {
        if !is_milestone_id(id) {
            return Ok(None);
        }
        Ok(self
            .list()?
            .into_iter()
            .find(|milestone| milestone.id == id))
    }

    /// The milestone `input` names: an existing id, else a title (case and spacing do not
    /// matter). A duplicated title is ambiguous and needs the id; a conflicted milestone
    /// is reached by its id only.
    pub fn resolve(&self, input: &str) -> Result<Milestone, MilestoneError> {
        let input = input.trim();
        let milestones = self.list()?;
        if let Some(found) = milestones.iter().find(|milestone| milestone.id == input) {
            return Ok(found.clone());
        }
        let key = milestone_title_key(input);
        let mut matches = milestones.into_iter().filter(|milestone| {
            milestone.state != MilestoneState::Conflicted
                && milestone_title_key(&milestone.title) == key
        });
        match (matches.next(), matches.next()) {
            (Some(found), None) => Ok(found),
            (Some(_), Some(_)) => Err(MilestoneError::Ambiguous(input.to_string())),
            (None, _) => Err(MilestoneError::NotFound(input.to_string())),
        }
    }

    /// Run `f` while holding the registry's process-safe lock.
    fn locked<T>(
        &self,
        f: impl FnOnce() -> Result<T, MilestoneError>,
    ) -> Result<T, MilestoneError> {
        let lock_path = self.root.join(MILESTONE_REGISTRY_LOCK);
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(io_error(parent))?;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(io_error(&lock_path))?;
        let _lock = FileLock::acquire(file).map_err(io_error(&lock_path))?;
        f()
    }

    /// Commit (and publish) a registry write. Never fails the write: Git problems become
    /// the registry's sync status.
    fn written(&self, message: &str) {
        git_sync::registry_written(RegistryKind::Milestones, &self.root, message);
    }

    /// Refuse `title` when a milestone other than `except` already has its key.
    fn check_title_free(&self, title: &str, except: Option<&str>) -> Result<(), MilestoneError> {
        let key = milestone_title_key(title);
        if let Some(taken) = self.list()?.into_iter().find(|milestone| {
            milestone.state != MilestoneState::Conflicted
                && Some(milestone.id.as_str()) != except
                && milestone_title_key(&milestone.title) == key
        }) {
            return Err(MilestoneError::TitleTaken {
                title: taken.title,
                id: taken.id,
            });
        }
        Ok(())
    }

    fn write_entry(&self, id: &str, entry: &Entry) -> Result<(), MilestoneError> {
        let path = self.entry_path(id);
        crate::store::write_file_atomically(&path, render_entry(entry).as_bytes())
            .map_err(io_error(&path))
    }

    /// Create an open milestone. Refused when another milestone has the title
    /// (`milestone_title_taken`).
    pub fn create(
        &self,
        title: &str,
        description: Option<&str>,
        due_on: Option<&str>,
    ) -> Result<Milestone, MilestoneError> {
        let entry = Entry {
            title: normalize_milestone_title(title)?,
            description: description
                .filter(|text| !text.trim().is_empty())
                .map(str::to_string),
            due_on: due_on.map(validate_due_on).transpose()?,
            closed: None,
            extra: Map::new(),
        };
        let id = self.locked(|| {
            self.check_title_free(&entry.title, None)?;
            let dir = self.dir();
            fs::create_dir_all(&dir).map_err(io_error(&dir))?;
            loop {
                let id = new_milestone_id();
                let path = self.entry_path(&id);
                match fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                {
                    Ok(_) => {
                        self.write_entry(&id, &entry)?;
                        return Ok(id);
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(io_error(&path)(error)),
                }
            }
        })?;
        self.written(&format!("Create milestone {}", entry.title));
        Self::milestone(&id, Read::Entry(entry)).ok_or(MilestoneError::NotFound(id))
    }

    /// Read `id` for a write, refusing a missing or conflicted file.
    fn entry_for_write(&self, id: &str) -> Result<Entry, MilestoneError> {
        if !is_milestone_id(id) {
            return Err(MilestoneError::NotFound(id.to_string()));
        }
        match self.read(id)? {
            Read::Missing => Err(MilestoneError::NotFound(id.to_string())),
            Read::Conflicted(detail) => Err(MilestoneError::Conflict {
                file: milestone_file(id),
                detail,
            }),
            Read::Entry(entry) => Ok(entry),
        }
    }

    /// Apply `change` to the milestone `id` under the lock; write and commit with the
    /// message `change` returns only when the entry changed.
    fn update(
        &self,
        id: &str,
        change: impl FnOnce(&mut Entry) -> Result<String, MilestoneError>,
    ) -> Result<MilestoneWrite, MilestoneError> {
        let (entry, message) = self.locked(|| {
            let before = self.entry_for_write(id)?;
            let mut after = before.clone();
            let message = change(&mut after)?;
            if after == before {
                return Ok((after, None));
            }
            self.write_entry(id, &after)?;
            Ok((after, Some(message)))
        })?;
        let changed = message.is_some();
        if let Some(message) = message {
            self.written(&message);
        }
        let mut milestone = Self::milestone(id, Read::Entry(entry))
            .ok_or_else(|| MilestoneError::NotFound(id.to_string()))?;
        milestone.duplicate = self.get(id)?.is_some_and(|stored| stored.duplicate);
        Ok(MilestoneWrite { milestone, changed })
    }

    /// Change the title, description, or due date of `id`. Renaming onto another
    /// milestone's title is refused; changing only the case or spacing of its own is not.
    pub fn edit(&self, id: &str, edit: &MilestoneEdit) -> Result<MilestoneWrite, MilestoneError> {
        let title = edit
            .title
            .as_deref()
            .map(normalize_milestone_title)
            .transpose()?;
        let description = edit.description.as_ref().map(|description| {
            description
                .as_deref()
                .filter(|text| !text.trim().is_empty())
                .map(str::to_string)
        });
        let due_on = edit
            .due_on
            .as_ref()
            .map(|due| due.as_deref().map(validate_due_on).transpose())
            .transpose()?;
        self.update(id, |entry| {
            let mut changes = Vec::new();
            if let Some(title) = title
                && title != entry.title
            {
                self.check_title_free(&title, Some(id))?;
                entry.title = title.clone();
                changes.push(format!("Rename milestone {id} to {title}"));
            }
            if let Some(description) = description
                && description != entry.description
            {
                entry.description = description;
                changes.push(format!("Edit the description of milestone {id}"));
            }
            if let Some(due_on) = due_on
                && due_on != entry.due_on
            {
                changes.push(match &due_on {
                    Some(date) => format!("Set due date of milestone {id} to {date}"),
                    None => format!("Clear due date of milestone {id}"),
                });
                entry.due_on = due_on;
            }
            Ok(match changes.len() {
                1 => changes.remove(0),
                _ => format!("Edit milestone {id}"),
            })
        })
    }

    /// Close `id` for `reason`, recording when. Closing a milestone already closed for
    /// the same reason changes nothing; another reason replaces it and keeps `closed_at`.
    pub fn close(&self, id: &str, reason: CloseReason) -> Result<MilestoneWrite, MilestoneError> {
        self.update(id, |entry| {
            let at = match &entry.closed {
                Some((_, at)) => at.clone(),
                None => Some(now_rfc3339()),
            };
            entry.closed = Some((reason, at));
            Ok(match reason {
                CloseReason::Completed => format!("Close milestone {id}"),
                CloseReason::NotPlanned => format!("Close milestone {id} as not planned"),
            })
        })
    }

    /// Reopen `id`, removing `closed_reason` and `closed_at`. Reopening an open milestone
    /// changes nothing.
    pub fn reopen(&self, id: &str) -> Result<MilestoneWrite, MilestoneError> {
        self.update(id, |entry| {
            entry.closed = None;
            Ok(format!("Reopen milestone {id}"))
        })
    }

    /// Delete the file of `id`, returning the milestone it held; `None` when it is already
    /// gone. A conflicted file is refused like every other write. Callers clear the
    /// milestone from its tickets first (Delete Milestone…, HS2-A820C1).
    pub fn remove(&self, id: &str) -> Result<Option<Milestone>, MilestoneError> {
        if !is_milestone_id(id) {
            return Err(MilestoneError::NotFound(id.to_string()));
        }
        let removed = self.locked(|| {
            let entry = match self.read(id)? {
                Read::Missing => return Ok(None),
                Read::Conflicted(detail) => {
                    return Err(MilestoneError::Conflict {
                        file: milestone_file(id),
                        detail,
                    });
                }
                Read::Entry(entry) => entry,
            };
            let path = self.entry_path(id);
            match fs::remove_file(&path) {
                Ok(()) => Ok(Self::milestone(id, Read::Entry(entry))),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(io_error(&path)(error)),
            }
        })?;
        if let Some(milestone) = &removed {
            self.written(&format!("Delete milestone {}", milestone.title));
        }
        Ok(removed)
    }
}

#[cfg(test)]
#[path = "milestones/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "milestones/git_tests.rs"]
mod git_tests;
