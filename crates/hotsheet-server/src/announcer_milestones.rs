//! Ticket milestones into the announcer's activity stream (HS2-G3HJZH, `docs/15` §15.6.1).
//!
//! One subscriber watches the change bus for ticket writes, from this server or from the CLI and
//! MCP through the store watcher, compares each changed ticket with what it saw before
//! ([`hotsheet_ticketing::announcer_milestones`]), and gathers the milestones of a store for a
//! short quiet window so one write is said once. Each batch is then decided on one worker thread,
//! so at most one AI call is ever in flight: an optional AI importance score and AI wording
//! ([`hotsheet_ticketing::announcer_ai`]), else the rule score and the template line. Decided
//! milestones are recorded with [`AppState::record_activity`], which persists them in the rolling
//! activity window and pushes them to clients over the live bus.
//!
//! The live model is the person's signed-in AI CLI, the first installed tool plugin declaring a
//! structured call (`drive.structured_call_args`; Claude's names its smallest model, no tools,
//! no MCP servers, no hooks, and no saved session), in a private empty directory, killed with
//! its process group at [`AI_TIMEOUT`]. It never runs in a test process
//! (`HOTSHEET_AI_TOOL_PROBES=off`), when the Announcer feature is off, or when both machine-wide
//! switches (`announcer.ai_importance`, `announcer.ai_rewrite`) are off.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use hotsheet_model::{AttachmentActorRole, ProjectTicketRef, Ulid};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::announcer_ai::{AiSettings, AiTask, Decider, MilestoneAi};
use hotsheet_ticketing::announcer_milestones::{
    Baseline, FollowUpTarget, Milestone, MilestoneSnapshot, milestone_event, ticket_milestones,
};
use hotsheet_ticketing::ops::TicketQuery;
use hotsheet_ticketing::{Scope, Settings};
use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::broadcast::error::RecvError;

use crate::AppState;
use crate::api_error::ApiError;

/// A store's milestones are gathered until it has been quiet this long.
pub const COALESCE: Duration = Duration::from_secs(4);
/// A busy store's batch is decided after this long however busy it stays.
pub const MAX_WAIT: Duration = Duration::from_secs(15);
/// The longest one AI call may run before its process group is killed.
pub const AI_TIMEOUT: Duration = Duration::from_secs(20);
/// Milestone ids already recorded, remembered so a re-observed write is never said twice.
const EMITTED_LIMIT: usize = 4096;
/// A store first seen after startup counts notes written this recently as new.
const LATE_SEED_LOOKBACK: time::Duration = time::Duration::seconds(30);

/// Machine-wide switch for AI importance scoring.
pub const AI_IMPORTANCE_KEY: &str = "announcer.ai_importance";
/// Machine-wide switch for AI wording.
pub const AI_REWRITE_KEY: &str = "announcer.ai_rewrite";

/// Change kinds that mean a ticket file was written.
const TICKET_KINDS: [&str; 6] = [
    "created", "updated", "changed", "closed", "claimed", "released",
];

/// The model reachable from the worker, shared so tests inject a fake.
pub type SharedAi = Arc<dyn MilestoneAi>;

/// How long batches wait; tests shorten it.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub coalesce: Duration,
    pub max_wait: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            coalesce: COALESCE,
            max_wait: MAX_WAIT,
        }
    }
}

/// Start the subscriber with the person's signed-in AI CLI. The handle keeps the bus task.
pub fn spawn_announcer_milestones(state: AppState) -> tokio::task::JoinHandle<()> {
    let ai = PluginCliAi {
        plugin_dirs: state.plugin_dirs.to_vec(),
    };
    spawn_announcer_milestones_with(state, Some(Arc::new(ai)), Timing::default())
}

/// Start the subscriber with a given model (or none) and batch timing.
pub fn spawn_announcer_milestones_with(
    state: AppState,
    ai: Option<SharedAi>,
    timing: Timing,
) -> tokio::task::JoinHandle<()> {
    let started_at = OffsetDateTime::now_utc();
    let mut events = state.subscribe();
    let startup_stores = state
        .host
        .locations()
        .into_iter()
        .map(|(store, _)| store)
        .collect();
    let (queued, pending) = mpsc::channel::<(String, String)>();
    let worker = state.clone();
    std::thread::Builder::new()
        .name("hotsheet-announcer-milestones".into())
        .spawn(move || Worker::new(worker, ai, timing).run(&pending, startup_stores, started_at))
        .expect("spawn the announcer milestone worker");
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if TICKET_KINDS.contains(&event.kind.as_str())
                        && !event.id.is_empty()
                        && queued.send((event.store, event.id)).is_err()
                    {
                        break;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "announcer milestones fell behind the change bus");
                }
                Err(RecvError::Closed) => break,
            }
        }
    })
}

struct Batch {
    first: Instant,
    due: Instant,
    milestones: Vec<Milestone>,
}

#[derive(Clone, Copy)]
struct StoreSeed {
    at: OffsetDateTime,
    at_startup: bool,
}

struct Worker {
    state: AppState,
    ai: Option<SharedAi>,
    timing: Timing,
    decider: Decider,
    /// When each store was seeded: notes and follow-ups of a ticket first seen count as new
    /// only after it.
    seeded: HashMap<String, StoreSeed>,
    snapshots: HashMap<(String, String), MilestoneSnapshot>,
    emitted: HashSet<String>,
    emitted_order: VecDeque<String>,
    batches: BTreeMap<String, Batch>,
}

impl Worker {
    fn new(state: AppState, ai: Option<SharedAi>, timing: Timing) -> Self {
        Self {
            state,
            ai,
            timing,
            decider: Decider::default(),
            seeded: HashMap::new(),
            snapshots: HashMap::new(),
            emitted: HashSet::new(),
            emitted_order: VecDeque::new(),
            batches: BTreeMap::new(),
        }
    }

    /// Seed every hosted store, then observe changes and decide due batches. Blocks on the
    /// channel while nothing is pending, so an idle server does no work.
    fn run(
        mut self,
        pending: &mpsc::Receiver<(String, String)>,
        startup_stores: Vec<String>,
        started_at: OffsetDateTime,
    ) {
        for store in startup_stores {
            self.seed(&store, started_at);
        }
        loop {
            let next = self.batches.values().map(|batch| batch.due).min();
            let received = match next {
                None => pending.recv().map_err(|_| RecvTimeoutError::Disconnected),
                Some(due) => pending.recv_timeout(due.saturating_duration_since(Instant::now())),
            };
            match received {
                Ok((store, id)) => self.observe(&store, &id),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            let now = Instant::now();
            let due: Vec<String> = self
                .batches
                .iter()
                .filter(|(_, batch)| batch.due <= now)
                .map(|(store, _)| store.clone())
                .collect();
            for store in due {
                if let Some(batch) = self.batches.remove(&store) {
                    self.decide(&store, batch.milestones);
                }
            }
        }
    }

    /// Record each ticket's transition baseline from the store's index.
    fn seed(&mut self, store: &str, at: OffsetDateTime) {
        self.seeded.insert(
            store.to_owned(),
            StoreSeed {
                at,
                at_startup: true,
            },
        );
        let Some(entry) = self.state.host.get(store) else {
            return;
        };
        let rows = entry
            .index
            .with_lock(|index| index.query(&TicketQuery::default()));
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(store, %error, "announcer milestones could not seed a store");
                return;
            }
        };
        for row in rows {
            let Some(status) = wire_enum(row.status.as_deref()) else {
                continue;
            };
            let baseline = Baseline {
                status,
                phase: wire_enum(row.started_phase.as_deref()),
                blocked: row
                    .blocked_reason
                    .as_deref()
                    .is_some_and(|reason| !reason.trim().is_empty())
                    || !row.blocked_by.is_empty(),
            };
            self.snapshots.insert(
                (store.to_owned(), row.id.clone()),
                MilestoneSnapshot::from_baseline(baseline),
            );
        }
    }

    fn observe(&mut self, store: &str, id: &str) {
        let Ok(ulid) = id.parse::<Ulid>() else {
            return;
        };
        let Some(entry) = self.state.host.get(store) else {
            return;
        };
        // A store hosted after startup has no baseline and only considers recent notes.
        let seed = *self
            .seeded
            .entry(store.to_owned())
            .or_insert_with(|| StoreSeed {
                at: OffsetDateTime::now_utc() - LATE_SEED_LOOKBACK,
                at_startup: false,
            });
        let Ok(ticket) = entry.store.read_ticket(&ulid) else {
            return;
        };
        let key = (store.to_owned(), id.to_owned());
        let resolve = |reference: &ProjectTicketRef| -> Option<FollowUpTarget> {
            let target = entry
                .store
                .read_ticket(&reference.native_id.parse::<Ulid>().ok()?)
                .ok()?;
            Some(FollowUpTarget {
                slug: target.slug,
                title: target.title,
                created_at: target.created_at.instant(),
                created_by_ai: target
                    .created_by
                    .map(|actor| actor.role == AttachmentActorRole::Ai),
            })
        };
        let previous = self.snapshots.get(&key);
        let recovered = if seed.at_startup {
            MilestoneSnapshot::recover_initial_status(previous, &ticket, seed.at)
        } else {
            None
        };
        let found = ticket_milestones(
            store,
            recovered.as_ref().or(previous),
            &ticket,
            seed.at,
            resolve,
        );
        self.snapshots.insert(key, MilestoneSnapshot::of(&ticket));
        let fresh: Vec<Milestone> = found
            .into_iter()
            .filter(|m| !self.emitted.contains(&m.id))
            .collect();
        if fresh.is_empty() {
            return;
        }
        let now = Instant::now();
        let batch = self
            .batches
            .entry(store.to_owned())
            .or_insert_with(|| Batch {
                first: now,
                due: now,
                milestones: Vec::new(),
            });
        batch.due = (now + self.timing.coalesce).min(batch.first + self.timing.max_wait);
        for m in fresh {
            if !batch.milestones.iter().any(|known| known.id == m.id) {
                batch.milestones.push(m);
            }
        }
    }

    fn remember(&mut self, id: &str) {
        if self.emitted.insert(id.to_owned()) {
            self.emitted_order.push_back(id.to_owned());
        }
        while self.emitted_order.len() > EMITTED_LIMIT {
            if let Some(old) = self.emitted_order.pop_front() {
                self.emitted.remove(&old);
            }
        }
    }

    fn decide(&mut self, store: &str, milestones: Vec<Milestone>) {
        let Some(entry) = self.state.host.get(store) else {
            return;
        };
        for m in &milestones {
            self.remember(&m.id);
        }
        let settings = read_ai_settings(&self.state);
        let wants_ai = settings.ai_importance || settings.ai_rewrite;
        let ai = self
            .ai
            .clone()
            .filter(|_| wants_ai && announcer_feature_on(&self.state));
        let decided = self
            .decider
            .decide(milestones, settings, ai.as_deref(), Instant::now());
        for (m, decision) in decided {
            let ts = OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_default();
            let event = milestone_event(&m, &decision, Ulid::new().to_string(), ts);
            if let Err(error) = self.state.record_activity(&entry.store, event) {
                tracing::warn!(%error, "announcer milestone could not be recorded");
            }
        }
    }
}

/// An index row's wire value as its model enum.
fn wire_enum<T: serde::de::DeserializeOwned>(value: Option<&str>) -> Option<T> {
    value.and_then(|text| serde_json::from_value(serde_json::json!(text)).ok())
}

fn machine_settings(state: &AppState) -> Settings {
    Settings::with_global_home(state.store.root(), state.machine_home())
}

fn switch(settings: &Settings, key: &str) -> bool {
    settings
        .get(key, Scope::Global)
        .ok()
        .flatten()
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

/// The machine-wide AI switches; an unreadable or mistyped value is on (its default).
pub fn read_ai_settings(state: &AppState) -> AiSettings {
    let settings = machine_settings(state);
    AiSettings {
        ai_importance: switch(&settings, AI_IMPORTANCE_KEY),
        ai_rewrite: switch(&settings, AI_REWRITE_KEY),
    }
}

fn announcer_feature_on(state: &AppState) -> bool {
    use hotsheet_ticketing::features::{is_on, setting_key};
    let stored = machine_settings(state)
        .get(&setting_key("announcer"), Scope::Global)
        .ok()
        .flatten();
    is_on("announcer", stored.as_ref())
}

/// `GET /announcer/settings`: the machine-wide AI switches.
pub(crate) async fn get_announcer_settings(
    State(state): State<AppState>,
) -> Result<Json<AiSettings>, ApiError> {
    tokio::task::spawn_blocking(move || read_ai_settings(&state))
        .await
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

/// A partial update: switches the body leaves out keep their value.
#[derive(Debug, Deserialize)]
pub(crate) struct AnnouncerSettingsPatch {
    ai_importance: Option<bool>,
    ai_rewrite: Option<bool>,
}

/// `PUT /announcer/settings`: set either switch; returns both.
pub(crate) async fn put_announcer_settings(
    State(state): State<AppState>,
    Json(patch): Json<AnnouncerSettingsPatch>,
) -> Result<Json<AiSettings>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let settings = machine_settings(&state);
        for (key, value) in [
            (AI_IMPORTANCE_KEY, patch.ai_importance),
            (AI_REWRITE_KEY, patch.ai_rewrite),
        ] {
            if let Some(value) = value {
                settings
                    .set(key, serde_json::Value::Bool(value), Scope::Global)
                    .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
            }
        }
        Ok(Json(read_ai_settings(&state)))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

/// The arguments of one AI call (HS2-17NWC1): the tool plugin's declared
/// `drive.structured_call_args` template with this task's prompt, schema and system prompt.
/// The plugin owns the model and the flags that turn off tools, MCP servers, hooks, slash
/// commands and saved sessions.
pub fn cli_args(template: &[String], task: AiTask, prompt: &str) -> Vec<String> {
    hotsheet_plugins::structured_call_args(template, prompt, task.schema(), task.system_prompt())
}

/// The person's signed-in AI CLI (no API key): the first installed tool plugin declaring a
/// structured call (`drive.structured_call_args`, docs/05 §5.1).
struct PluginCliAi {
    plugin_dirs: Vec<PathBuf>,
}

impl MilestoneAi for PluginCliAi {
    fn ask(&self, task: AiTask, prompt: &str) -> Result<String, String> {
        // HS2-7W3D7V: test processes never run an installed tool.
        if !hotsheet_aitools::model_catalog::ai_tool_probes_enabled() {
            return Err("AI tool calls are off in this process".into());
        }
        let (program, template) = hotsheet_plugins::detected_structured_call(&self.plugin_dirs)
            .ok_or("no installed AI tool serves a structured call")?;
        // HS2-BNDRYP: a private empty directory, so nothing the tool writes lands in a store.
        let workspace = hotsheet_aitools::probe::probe_workspace()
            .map_err(|error| format!("no private directory: {error}"))?;
        let mut command = hotsheet_ticketing::git::launch_resolved(
            &program,
            std::env::var_os("PATH").as_deref(),
            Some(workspace.path()),
        );
        command
            .args(cli_args(&template, task, prompt))
            .current_dir(workspace.path());
        // Nothing in the call may report back into Hot Sheet as a worker.
        for name in [
            "HOTSHEET_SERVER",
            "HOTSHEET_SECRET",
            "HOTSHEET_PROJECT",
            "HOTSHEET_WORKER_ID",
        ] {
            command.env_remove(name);
        }
        let output = hotsheet_aitools::probe::run_probe_within(&mut command, AI_TIMEOUT)?;
        if !output.status.success() {
            return Err(format!("{program} exited with {}", output.status));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| format!("{program} replied with invalid UTF-8"))
    }
}

#[cfg(test)]
#[path = "announcer_milestones_tests.rs"]
mod tests;
