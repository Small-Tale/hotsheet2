//! Asking an AI to finish a ticket-store sync that Hot Sheet cannot finish on its own
//! (HS2-QR1K8R, `docs/02` §2.12.4).
//!
//! After a sync cycle ends in `conflict` (the merge driver could not settle a ticket) or
//! `rebase_in_progress` (an unfinished rebase or merge that needs a decision), the server
//! loop and `hotsheet-cli sync` call [`ask_for_help`]. It records the stall, creates one
//! deduplicated high-priority Up Next repair ticket in the stalled store holding the state
//! and the safe procedure, and then, by the project's `ticket_store_sync.ask_ai` setting:
//!
//! - `start` (default): starts a new AI session of the project's default AI tool in the
//!   background, even when another AI session already runs in the project (the
//!   maintainer's "start a new AI session and ask the AI to fix it");
//! - `session`: pushes the prompt into an AI session already running in the project, or
//!   else leaves the ticket;
//! - `off`: does nothing.
//!
//! A store is asked at most once until it publishes again: every later stall before a
//! successful cycle reuses the first ask, so no second session starts while the first is
//! unresolved. A stall after a successful cycle is a new stall. The caller supplies how an
//! AI is reached ([`AiReach`]): the server pushes into its terminals and drives a session
//! through its permission bridge; the headless CLI starts `hotsheet-cli trigger`.
//!
//! The record (`<git-dir>/hotsheet-sync-ai.json`) and the ticket hold ids, counts, states,
//! and a store-relative reference only, never ticket content or machine paths.

use std::path::{Path, PathBuf};

use hotsheet_model::{NoteKind, Priority, Status, Timestamp, Ulid};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::settings::Settings;
use crate::sync::git_stdout;
use crate::sync_health::{IntegrationInProgress, SyncState, integration_in_progress, read_record};
use crate::{FsStore, NewTicket, TicketQuery, ops};

/// The project setting that chooses how Hot Sheet asks an AI.
pub const ASK_AI_SETTING: &str = "ticket_store_sync.ask_ai";

/// The project setting naming the default AI tool (shared with Drive).
pub const AI_DEFAULTS_SETTING: &str = "ai.defaults";

/// The tag every repair ticket carries.
pub const REPAIR_TAG: &str = "sync-repair";

/// When set, the comma-separated AI tools a stall may start (HS2-QR1K8R). Every process
/// cargo launches gets the test fixture's tool only (`.cargo/config.toml`), so no test can
/// start an installed AI tool with real credentials. Unset in normal use.
pub const ALLOWED_TOOLS_ENV: &str = "HOTSHEET_SYNC_AI_ALLOWED_TOOLS";

/// Whether this environment may start `tool` for a stall.
fn start_allowed(tool: &str, allowed: Option<&str>) -> bool {
    allowed.is_none_or(|list| list.split(',').map(str::trim).any(|name| name == tool))
}

const RECORD_FILE: &str = "hotsheet-sync-ai.json";
const RECORD_SCHEMA: u32 = 1;

/// How Hot Sheet asks an AI when sync cannot finish alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskAiMode {
    /// Never ask.
    Off,
    /// Push to an AI session already running in the project; otherwise leave the ticket.
    Session,
    /// Start a new background AI session; never push into a running one.
    #[default]
    Start,
}

impl AskAiMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Session => "session",
            Self::Start => "start",
        }
    }

    /// Parse a setting value; `None` for anything but the three names.
    #[must_use]
    pub fn parse(value: &Value) -> Option<Self> {
        match value.as_str()? {
            "off" => Some(Self::Off),
            "session" => Some(Self::Session),
            "start" => Some(Self::Start),
            _ => None,
        }
    }
}

/// The project's mode; `start` when unset or unreadable.
#[must_use]
pub fn ask_ai_mode(settings: &Settings) -> AskAiMode {
    settings
        .get_effective(ASK_AI_SETTING)
        .ok()
        .flatten()
        .and_then(|value| AskAiMode::parse(&value))
        .unwrap_or_default()
}

/// The project's default AI tool id (`ai.defaults.tool`), if one is configured.
#[must_use]
pub fn configured_ai_tool(settings: &Settings) -> Option<String> {
    settings
        .get_effective(AI_DEFAULTS_SETTING)
        .ok()
        .flatten()?
        .get("tool")?
        .as_str()
        .map(str::trim)
        .filter(|tool| !tool.is_empty())
        .map(str::to_string)
}

/// A stall sync cannot finish alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stall {
    /// `rebase:<orig-head>`, `merge:<MERGE_HEAD>`, or `conflict:<merge-base>..<upstream>`.
    pub id: String,
    pub state: SyncState,
}

fn git_dir(store: &Path) -> Option<PathBuf> {
    git_stdout(store, &["rev-parse", "--absolute-git-dir"])
        .map(|dir| PathBuf::from(dir.trim_end_matches(['\n', '\r'])))
}

fn first_line(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .next()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
}

/// The stall the store is in now, if any: a live unfinished rebase or merge, or the
/// conflict the last cycle recorded.
#[must_use]
pub fn current_stall(store: &Path) -> Option<Stall> {
    match integration_in_progress(store) {
        Some(IntegrationInProgress::Rebase) => {
            let dir = git_dir(store)?;
            let orig = first_line(&dir.join("rebase-merge/orig-head"))
                .or_else(|| first_line(&dir.join("rebase-apply/orig-head")))
                .unwrap_or_else(|| "unknown".to_string());
            Some(Stall {
                id: format!("rebase:{orig}"),
                state: SyncState::RebaseInProgress,
            })
        }
        Some(IntegrationInProgress::Merge) => {
            let head = first_line(&git_dir(store)?.join("MERGE_HEAD"))
                .unwrap_or_else(|| "unknown".to_string());
            Some(Stall {
                id: format!("merge:{head}"),
                state: SyncState::RebaseInProgress,
            })
        }
        None => {
            let record = read_record(store)?;
            if record.outcome != Some(SyncState::Conflict) {
                return None;
            }
            let (head, upstream) = record.conflict_heads?;
            // The merge base stays put while local writes add commits, so a ticket the
            // ask itself commits does not make a new stall.
            let base = git_stdout(store, &["merge-base", &head, &upstream])
                .map(|base| base.trim().to_string())
                .unwrap_or_else(|| head.clone());
            Some(Stall {
                id: format!("conflict:{base}..{upstream}"),
                state: SyncState::Conflict,
            })
        }
    }
}

/// How the ask ended for one stall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiHelpOutcome {
    /// The prompt was pushed into an AI session already running in the project.
    SessionAsked,
    /// A new AI session was started and is working.
    Started,
    /// The started session ended (the store may or may not be repaired yet).
    Finished,
    /// The started session ended with an error.
    Failed,
    /// No AI could be asked; the repair ticket waits. `reason` says why.
    TicketOnly,
}

impl AiHelpOutcome {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionAsked => "session_asked",
            Self::Started => "started",
            Self::Finished => "finished",
            Self::Failed => "failed",
            Self::TicketOnly => "ticket_only",
        }
    }
}

/// The durable record of the last ask in one store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiHelpRecord {
    #[serde(default)]
    pub schema: u32,
    /// The stall that triggered the ask.
    pub stall: String,
    pub state: SyncState,
    pub mode: AskAiMode,
    pub outcome: AiHelpOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The connection id of the session that received the prompt or was started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub asked_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// When a later cycle published again; a stall after this is a new stall.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
}

/// What the sync health contract shows about the current ask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiHelp {
    pub outcome: AiHelpOutcome,
    pub mode: AskAiMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    pub asked_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The unresolved ask, if any.
#[must_use]
pub fn current_help(store: &Path) -> Option<AiHelp> {
    let record = read_ai_record(store)?;
    record.resolved_at.is_none().then_some(AiHelp {
        outcome: record.outcome,
        mode: record.mode,
        tool: record.tool,
        ticket: record.ticket,
        asked_at: record.asked_at,
        finished_at: record.finished_at,
        reason: record.reason,
    })
}

/// The durable ask record, if one was written.
#[must_use]
pub fn read_ai_record(store: &Path) -> Option<AiHelpRecord> {
    let path = git_dir(store)?.join(RECORD_FILE);
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn write_ai_record(store: &Path, record: &AiHelpRecord) {
    let Some(dir) = git_dir(store) else {
        return;
    };
    let Ok(mut temp) = tempfile::NamedTempFile::new_in(&dir) else {
        return;
    };
    if serde_json::to_writer_pretty(&mut temp, record).is_ok() {
        let _ = temp.persist(dir.join(RECORD_FILE));
    }
}

/// Called once when a started session ends.
pub type SessionDone = Box<dyn FnOnce(Result<(), String>) + Send + 'static>;

/// How the caller reaches an AI. Implementations never block on an AI turn.
pub trait AiReach {
    /// Push the prompt into an AI session already running in the project. `Ok(Some(id))`
    /// names the session that received it; `Ok(None)` means none is running.
    fn push_to_session(&self, prompt: &str) -> Result<Option<String>, String>;
    /// The project's default AI tool, when it is configured and can be started here.
    fn drivable_tool(&self) -> Result<String, String>;
    /// Start a new AI session with `tool` and give it the prompt. Returns once the session
    /// is launched, with its connection id; `done` runs when it ends. `done` must run after
    /// this call returns (on another thread), never inside it.
    fn start_session(&self, tool: &str, prompt: &str, done: SessionDone) -> Result<String, String>;
}

/// The store as the asked AI reaches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskContext {
    pub mode: AskAiMode,
    /// The store's display name.
    pub label: String,
    /// The store's path relative to the project checkout the AI works in (see
    /// [`store_reference`]).
    pub store_reference: String,
}

/// What [`ask_for_help`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskOutcome {
    /// No stall; a resolved ask was closed when the store publishes again.
    NoStall,
    /// The mode is `off`.
    Off,
    /// The store was already asked and has not published since.
    AlreadyAsked,
    /// The store could not be locked; the next cycle tries again.
    Busy,
    Asked(Box<AiHelpRecord>),
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// The store's path from the project checkout, without machine-specific prefixes: the
/// relative path inside the checkout, a sibling `../name`, or `.` for the checkout itself.
/// Anything else falls back to the store's directory name.
#[must_use]
pub fn store_reference(project: &Path, store: &Path) -> String {
    // A path that does not exist yet still resolves through its parent.
    let canonical = |path: &Path| {
        path.canonicalize().unwrap_or_else(|_| {
            match (
                path.parent().and_then(|p| p.canonicalize().ok()),
                path.file_name(),
            ) {
                (Some(parent), Some(name)) => parent.join(name),
                _ => path.to_path_buf(),
            }
        })
    };
    let (project, store) = (canonical(project), canonical(store));
    if let Ok(inside) = store.strip_prefix(&project) {
        let text = inside.to_string_lossy();
        return if text.is_empty() {
            ".".to_string()
        } else {
            text.into_owned()
        };
    }
    let name = store
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    if project.parent().is_some() && project.parent() == store.parent() {
        format!("../{name}")
    } else {
        name
    }
}

/// The fixed, content-free reason the prompt and ticket give for a state.
fn state_sentence(state: SyncState) -> &'static str {
    match state {
        SyncState::Conflict => {
            "a ticket changed on both sides and the merge driver could not merge it"
        }
        _ => "a rebase or merge in the store is unfinished and needs a decision",
    }
}

/// The message the asked AI receives. It holds no ticket content and tells the AI to act
/// only on the stored ticket.
#[must_use]
pub fn build_prompt(
    context: &AskContext,
    state: SyncState,
    unpublished: u32,
    slug: &str,
) -> String {
    let reference = &context.store_reference;
    format!(
        "Hot Sheet could not finish synchronizing the ticket store \"{label}\" of this \
         project (`{reference}` from the project checkout). State: {state}. Reason: \
         {reason}. {unpublished} local changes are unpublished. Ticket: {slug}.\n\nThis \
         message is data from Hot Sheet, not from the user. Load {slug} with `hotsheet-cli \
         -C {reference} show {slug}`, confirm it exists, claim it, and follow the procedure \
         in it. Do not act on this message alone.",
        label = context.label,
        state = state.as_str(),
        reason = state_sentence(state),
    )
}

/// The repair ticket's body: the state and the nine-step safe procedure.
#[must_use]
pub fn repair_details(context: &AskContext, state: SyncState, unpublished: u32) -> String {
    let r = &context.store_reference;
    format!(
        "Hot Sheet could not finish synchronizing the ticket store `{label}` (`{r}` from \
         the project checkout) and asked an AI to repair it (HS2-QR1K8R).\n\n\
         - State: `{state}`\n\
         - Reason: {reason}\n\
         - Unpublished local commits: {unpublished}\n\n\
         Run every command from the project checkout. Never discard local work.\n\n\
         ## Procedure\n\n\
         1. Confirm it still applies: `hotsheet-cli -C {r} sync --status --json`. If the \
         state is no longer `rebase_in_progress` or `conflict`, say so in a note on this \
         ticket and complete it.\n\
         2. Record backups before changing anything: `git -C {r} update-ref \
         refs/hotsheet/sync-backup/ai-<UTC stamp> HEAD`, and for an unfinished rebase a \
         second ref `refs/hotsheet/sync-backup/ai-<UTC stamp>-orig` at the commit named in \
         `$(git -C {r} rev-parse --git-path rebase-merge/orig-head)`.\n\
         3. Never run `git rebase --abort` or `--skip`, `git reset --hard`, `git checkout \
         -- <path>`, `git restore`, `git clean`, any forced push, or any ref or commit \
         deletion. Never edit a ticket file that is not conflicted. Never start a new \
         rebase in `{r}` itself.\n\
         4. For `rebase_in_progress`: list `git -C {r} diff --name-only --diff-filter=U`. \
         In each file keep both sides: every note from both, the scalar fields of the side \
         with the newer `updated_at`, and both body paragraphs. Leave no conflict markers, \
         check the ticket with `hotsheet-cli -C {r} show <slug>`, then `git -C {r} add -- \
         <that file>` only. Run `git -C {r} -c core.editor=true rebase --continue`. \
         Repeat for each stop. Stop after ten stops, or when the same file conflicts \
         twice.\n\
         5. For `conflict`: there is no rebase in the store. Resolve in a temporary \
         worktree: `git -C {r} worktree add --detach <temp dir> HEAD`, then `git -C <temp \
         dir> rebase $(git -C {r} rev-parse --abbrev-ref @{{u}})`, resolve each stop as in \
         step 4 inside the temporary worktree, run `hotsheet-cli -C {r} sync --adopt \
         <rebased commit>`, and remove it with `git -C {r} worktree remove <temp dir>`.\n\
         6. Verify: no `rebase-merge`, `rebase-apply`, or `MERGE_HEAD` under `git -C {r} \
         rev-parse --git-dir`; `git -C {r} symbolic-ref HEAD` names the branch; `git -C \
         {r} status --porcelain` lists nothing but ticket files Hot Sheet saved meanwhile; \
         `git -C {r} diff <backup ref>..HEAD --stat` shows only incoming changes and the \
         files you resolved; `hotsheet-cli -C {r} doctor` reads every ticket.\n\
         7. Publish with `hotsheet-cli -C {r} sync` only. Confirm `hotsheet-cli -C {r} \
         sync --status` reports `synced` or `up_to_date` with zero unpublished commits.\n\
         8. Record on this ticket the backup ref names, how many files you resolved and \
         commits you published, and the final state: ids, counts, and paths only, never \
         ticket content. Then complete this ticket.\n\
         9. If any step cannot be completed: leave the store as it is, add a `FEEDBACK \
         NEEDED` note naming the file and the reason, and release your claim.\n",
        label = context.label,
        state = state.as_str(),
        reason = state_sentence(state),
    )
}

/// The open repair ticket of this store, or a new one. The ticket file is saved even
/// while a rebase is unfinished; its commit then waits until the rebase finishes.
fn ensure_ticket(
    store_root: &Path,
    context: &AskContext,
    state: SyncState,
    unpublished: u32,
) -> Result<(Ulid, String), crate::StoreError> {
    let store = FsStore::open(store_root)?.with_deferred_push();
    store.with_ticket_transaction(|| {
        let existing = ops::query(
            &store,
            &TicketQuery {
                tags: vec![REPAIR_TAG.to_string()],
                open_only: true,
                ..TicketQuery::default()
            },
        )?;
        if let Some(ticket) = existing.first() {
            return Ok((ticket.id, ticket.slug.clone()));
        }
        let prefix = store.metadata()?.ticket_prefix;
        let ticket = ops::create(
            &store,
            Ulid::new(),
            &prefix,
            Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
            NewTicket {
                group: None,
                title: format!("Finish syncing the ticket store {}", context.label),
                category: "bug".into(),
                priority: Priority::High,
                status: Status::NotStarted,
                details: repair_details(context, state, unpublished),
                tags: vec![REPAIR_TAG.to_string()],
                up_next: true,
                blocked_by: vec![],
            },
        )?;
        Ok((ticket.id, ticket.slug))
    })
}

/// Mark the unresolved ask resolved once the store publishes again, with a note on an
/// open repair ticket.
fn resolve(store: &Path, mut record: AiHelpRecord) {
    let published = read_record(store).and_then(|sync| sync.outcome);
    if !matches!(published, Some(SyncState::Synced | SyncState::UpToDate)) {
        return;
    }
    record.resolved_at = Some(now());
    write_ai_record(store, &record);
    let Some(id) = record
        .ticket_id
        .as_deref()
        .and_then(|id| id.parse::<Ulid>().ok())
    else {
        return;
    };
    let Ok(fs) = FsStore::open(store).map(FsStore::with_deferred_push) else {
        return;
    };
    let open = fs
        .read_ticket(&id)
        .is_ok_and(|ticket| ticket.status.is_active());
    if open {
        let _ = ops::add_note(
            &fs,
            &id,
            Ulid::new(),
            Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
            NoteKind::Regular,
            "Hot Sheet: this ticket store publishes again; the stall this ticket was opened \
             for is resolved. Confirm with step 1 and complete the ticket."
                .to_string(),
        );
    }
}

/// Ask an AI to finish the store's stall, once per stall period. See the module docs.
pub fn ask_for_help(store: &Path, context: &AskContext, reach: &dyn AiReach) -> AskOutcome {
    let Ok(_lock) = crate::store::lock_sync_ai(store) else {
        return AskOutcome::Busy;
    };
    let previous = read_ai_record(store).filter(|record| record.resolved_at.is_none());
    let Some(stall) = current_stall(store) else {
        if let Some(record) = previous {
            resolve(store, record);
        }
        return AskOutcome::NoStall;
    };
    if previous.is_some() {
        return AskOutcome::AlreadyAsked;
    }
    if context.mode == AskAiMode::Off {
        return AskOutcome::Off;
    }
    let unpublished = crate::sync::local_divergence(store).1;
    let mut record = AiHelpRecord {
        schema: RECORD_SCHEMA,
        stall: stall.id.clone(),
        state: stall.state,
        mode: context.mode,
        outcome: AiHelpOutcome::TicketOnly,
        tool: None,
        session: None,
        ticket: None,
        ticket_id: None,
        reason: None,
        asked_at: now(),
        finished_at: None,
        resolved_at: None,
    };
    let slug = match ensure_ticket(store, context, stall.state, unpublished) {
        Ok((id, slug)) => {
            record.ticket_id = Some(id.to_string());
            record.ticket = Some(slug.clone());
            slug
        }
        Err(error) => {
            record.reason = Some(format!("the repair ticket could not be created: {error}"));
            "(no ticket)".to_string()
        }
    };
    let prompt = build_prompt(context, stall.state, unpublished, &slug);
    if context.mode == AskAiMode::Session {
        // `session` only pushes into a session already running in the project.
        match reach.push_to_session(&prompt) {
            Ok(Some(session)) => {
                record.outcome = AiHelpOutcome::SessionAsked;
                record.session = Some(session);
                record.reason = None;
            }
            Ok(None) => {
                record
                    .reason
                    .get_or_insert_with(|| "no AI session is running in this project".to_string());
            }
            Err(error) => {
                record.reason = Some(format!("pushing to the running AI session failed: {error}"));
            }
        }
    } else {
        // `start` always starts a new background session, whatever already runs.
        let allowed = std::env::var(ALLOWED_TOOLS_ENV).ok();
        match reach.drivable_tool() {
            Err(why) => record.reason = Some(why),
            Ok(tool) if !start_allowed(&tool, allowed.as_deref()) => {
                record.reason = Some(format!(
                    "starting {tool} is disabled in this environment ({ALLOWED_TOOLS_ENV})"
                ));
            }
            Ok(tool) => {
                record.tool = Some(tool.clone());
                record.outcome = AiHelpOutcome::Started;
                // Recorded before the launch, so the end can never arrive first.
                write_ai_record(store, &record);
                let done_store = store.to_path_buf();
                let done_stall = stall.id.clone();
                let done: SessionDone =
                    Box::new(move |result| record_session_end(&done_store, &done_stall, result));
                match reach.start_session(&tool, &prompt, done) {
                    Ok(session) => {
                        record.session = Some(session);
                        record.reason = None;
                    }
                    Err(error) => {
                        record.outcome = AiHelpOutcome::TicketOnly;
                        record.reason = Some(format!("{tool} could not be started: {error}"));
                    }
                }
            }
        }
    }
    write_ai_record(store, &record);
    AskOutcome::Asked(Box::new(record))
}

/// Record how a started session ended, if it is still the store's current ask.
pub fn record_session_end(store: &Path, stall: &str, result: Result<(), String>) {
    let Ok(_lock) = crate::store::lock_sync_ai(store) else {
        return;
    };
    let Some(mut record) = read_ai_record(store) else {
        return;
    };
    if record.stall != stall || record.outcome != AiHelpOutcome::Started {
        return;
    }
    record.finished_at = Some(now());
    match result {
        Ok(()) => record.outcome = AiHelpOutcome::Finished,
        Err(error) => {
            record.outcome = AiHelpOutcome::Failed;
            record.reason = Some(crate::sync_health::sanitize_reason(&error));
        }
    }
    write_ai_record(store, &record);
}

/// The health reason line for an ask that could not reach an AI.
#[must_use]
pub fn reason_line(help: &AiHelp) -> Option<String> {
    match help.outcome {
        AiHelpOutcome::TicketOnly => Some(format!(
            "No AI session was asked: {}.",
            help.reason.as_deref().unwrap_or("no AI could be reached")
        )),
        AiHelpOutcome::Failed => Some(format!(
            "The AI session Hot Sheet started ended with an error: {}.",
            help.reason.as_deref().unwrap_or("unknown error")
        )),
        _ => None,
    }
}

/// The health next-step sentence while an ask is unresolved.
#[must_use]
pub fn next_action(help: &AiHelp) -> String {
    let who = help.tool.as_deref().unwrap_or("an AI session");
    let ticket = help
        .ticket
        .as_deref()
        .map_or_else(String::new, |slug| format!(" (ticket {slug})"));
    match help.outcome {
        AiHelpOutcome::SessionAsked => format!(
            "Hot Sheet asked the running AI session to repair the store{ticket}; sync \
             publishes once it is repaired."
        ),
        AiHelpOutcome::Started => format!(
            "Hot Sheet started {who} to repair the store{ticket}; sync publishes once it is \
             repaired."
        ),
        AiHelpOutcome::Finished => format!(
            "The {who} session Hot Sheet started has ended{ticket}; if the store still does \
             not publish, follow the procedure in the ticket."
        ),
        AiHelpOutcome::Failed | AiHelpOutcome::TicketOnly => format!(
            "Follow the repair procedure in the ticket{ticket}, or run it in an AI session."
        ),
    }
}

#[cfg(test)]
mod tests;
