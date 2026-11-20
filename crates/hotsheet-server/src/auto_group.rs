//! Group new tickets automatically (HS2-FN018Q, `docs/35-ticket-groups.md` §35.6).
//!
//! When a project's shared `auto_group_new_tickets` setting is on (the default), every new
//! ungrouped ticket in one of its Git stores is offered to the project's default AI tool in a
//! private one-shot turn, however it was created: in this server, by the CLI or MCP, in another
//! window, or by a clone whose write the watcher reports. New tickets are coalesced per store for
//! [`COALESCE`] and described together (at most [`AUTO_GROUP_BATCH_LIMIT`] per turn), so an agent
//! fan-out spends one turn rather than one per ticket. The reply follows the app's Add to Group ›
//! Automatic rules (`hotsheet_ticketing::groups::auto`); each choice is written only if the ticket
//! is still ungrouped, as an ordinary group change. Failures (no drivable tool, a failed or
//! timed-out turn, an unusable reply) write nothing. A ticket is considered at most once per run.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use hotsheet_model::{Status, Ulid};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::groups::GroupRegistry;
use hotsheet_ticketing::groups::auto::{
    AUTO_GROUP_BATCH_LIMIT, AutoGroupChoice, AutoGroupTicket, auto_group_prompt,
    parse_auto_group_reply,
};
use hotsheet_ticketing::{Settings, ops};
use tokio::sync::broadcast::error::RecvError;

use crate::AppState;

/// How long a store's new tickets are gathered before one turn describes them all.
pub const COALESCE: Duration = Duration::from_secs(3);
/// A ticket counts as new only when it was created within this window; an older ticket
/// that is merely edited, or one a server restart finds again, is never grouped.
pub const NEW_TICKET_WINDOW: Duration = Duration::from_secs(10 * 60);
/// The longest an automatic-grouping turn may run before it is interrupted.
pub const TURN_LIMIT: Duration = Duration::from_secs(3 * 60);
/// The system actor recorded on automatic group changes.
pub const AUTO_GROUP_ACTOR: &str = "hotsheet-auto-group";

/// One turn's request: the project, its store, its settings, and the prompt.
pub struct AskRequest<'a> {
    pub project_root: &'a Path,
    pub store_root: &'a Path,
    pub settings: &'a Settings,
    pub prompt: &'a str,
}

/// Runs one private turn and returns the assistant's text; injectable for tests.
pub type Ask = Arc<dyn Fn(&AskRequest<'_>) -> Result<String, String> + Send + Sync>;

/// Start the subscriber with the project's default AI tool. The handle keeps the bus task.
pub fn spawn_auto_grouper(state: AppState) -> tokio::task::JoinHandle<()> {
    let asking = state.clone();
    spawn_auto_grouper_with(
        state,
        Arc::new(move |request: &AskRequest<'_>| ask_default_tool(&asking, request)),
    )
}

/// Start the subscriber with a given `ask` (tests pass a scripted reply).
pub fn spawn_auto_grouper_with(state: AppState, ask: Ask) -> tokio::task::JoinHandle<()> {
    let mut events = state.subscribe();
    let (queued, pending) = mpsc::channel::<(String, String)>();
    let worker = state.clone();
    std::thread::Builder::new()
        .name("hotsheet-auto-group".into())
        .spawn(move || work(&worker, &pending, &ask))
        .expect("spawn the automatic grouping worker");
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if (event.kind == "created" || event.kind == "changed")
                        && !event.id.is_empty()
                        && queued.send((event.store, event.id)).is_err()
                    {
                        break;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "automatic grouping fell behind the change bus");
                }
                Err(RecvError::Closed) => break,
            }
        }
    })
}

/// Gather new tickets per store and group each store's batch once it has been quiet for
/// [`COALESCE`]. Blocks on the channel while nothing is pending, so an idle server does no work.
fn work(state: &AppState, pending: &mpsc::Receiver<(String, String)>, ask: &Ask) {
    let mut considered: HashSet<(String, Ulid)> = HashSet::new();
    let mut batches: BTreeMap<String, (Instant, Vec<Ulid>)> = BTreeMap::new();
    loop {
        let next = batches.values().map(|(due, _)| *due).min();
        let received = match next {
            None => pending.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(due) => pending.recv_timeout(due.saturating_duration_since(Instant::now())),
        };
        match received {
            Ok((store, id)) => {
                let Ok(id) = id.parse::<Ulid>() else { continue };
                if considered.insert((store.clone(), id)) {
                    let batch = batches
                        .entry(store)
                        .or_insert_with(|| (Instant::now(), Vec::new()));
                    batch.0 = Instant::now() + COALESCE;
                    batch.1.push(id);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let now = Instant::now();
        let due: Vec<String> = batches
            .iter()
            .filter(|(_, (at, _))| *at <= now)
            .map(|(store, _)| store.clone())
            .collect();
        for store in due {
            if let Some((_, ids)) = batches.remove(&store) {
                group_batch(state, &store, &ids, ask);
            }
        }
    }
}

/// The project that owns a store: its code checkout root and settings.
fn project_for(state: &AppState, store_root: &Path) -> Option<(PathBuf, Settings)> {
    let checkouts = state.checkout_registry.list().ok()?;
    checkouts
        .into_iter()
        .find(|checkout| {
            checkout
                .stores
                .iter()
                .any(|root| crate::watcher::same_path(Path::new(root), store_root))
        })
        .map(|checkout| (PathBuf::from(&checkout.root), checkout.settings()))
}

/// Whether a ticket is a new, ungrouped, live ticket.
fn eligible(ticket: &hotsheet_model::Ticket, now: time::OffsetDateTime) -> bool {
    if ticket.group.is_some() || matches!(ticket.status, Status::Deleted | Status::Moved) {
        return false;
    }
    let Some(created) = ticket.created_at.instant() else {
        return false;
    };
    let age = now - created;
    // A minute of clock skew between clones is still "new".
    age >= -time::Duration::minutes(1)
        && age <= time::Duration::try_from(NEW_TICKET_WINDOW).unwrap_or(time::Duration::ZERO)
}

/// Whether the machine-wide Automatic ticket grouping feature is on (HS2-3GJZGG). A settings file
/// that cannot be read leaves it at its default, off.
fn auto_grouping_feature_on(state: &AppState, store_root: &std::path::Path) -> bool {
    use hotsheet_ticketing::features::{AUTO_GROUPING, is_on, setting_key};
    let settings = hotsheet_ticketing::Settings::with_global_home(store_root, state.machine_home());
    let stored = settings
        .get(
            &setting_key(AUTO_GROUPING),
            hotsheet_ticketing::Scope::Global,
        )
        .ok()
        .flatten();
    is_on(AUTO_GROUPING, stored.as_ref())
}

/// Ask once for a store's batch and apply every usable choice.
fn group_batch(state: &AppState, store_id: &str, ids: &[Ulid], ask: &Ask) {
    let Some(entry) = state.host.get(store_id) else {
        return;
    };
    let store_root = entry.store.root().to_path_buf();
    // The machine-wide Automatic ticket grouping feature (HS2-3GJZGG) gates the AI turn; the
    // project setting keeps its value while the feature is off.
    if !auto_grouping_feature_on(state, &store_root) {
        return;
    }
    let Some((project_root, settings)) = project_for(state, &store_root) else {
        return;
    };
    match settings.auto_group_new_tickets() {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tracing::warn!(%error, "automatic grouping setting is invalid; skipping");
            return;
        }
    }
    let now = time::OffsetDateTime::now_utc();
    let tickets: Vec<hotsheet_model::Ticket> = ids
        .iter()
        .filter_map(|id| entry.store.read_ticket(id).ok())
        .filter(|ticket| eligible(ticket, now))
        .collect();
    if tickets.is_empty() {
        return;
    }
    let registry = GroupRegistry::for_checkout(&project_root);
    for chunk in tickets.chunks(AUTO_GROUP_BATCH_LIMIT) {
        let groups = match registry.names() {
            Ok(groups) => groups,
            Err(error) => {
                tracing::warn!(%error, "automatic grouping could not read the group registry");
                return;
            }
        };
        let batch: Vec<AutoGroupTicket> = chunk
            .iter()
            .map(|ticket| AutoGroupTicket {
                slug: ticket.slug.clone(),
                title: ticket.title.clone(),
                category: ticket.category.clone(),
            })
            .collect();
        let prompt = auto_group_prompt(&batch, &groups);
        let reply = match ask(&AskRequest {
            project_root: &project_root,
            store_root: &store_root,
            settings: &settings,
            prompt: &prompt,
        }) {
            Ok(reply) => reply,
            Err(error) => {
                tracing::info!(%error, tickets = batch.len(), "automatic grouping skipped");
                return;
            }
        };
        let choices = match parse_auto_group_reply(&reply, &batch, &groups) {
            Ok(choices) => choices,
            Err(error) => {
                tracing::info!(%error, "automatic grouping reply was unusable");
                continue;
            }
        };
        for choice in choices {
            apply(state, &entry, &registry, chunk, &choice);
        }
    }
}

/// Write one choice if its ticket is still ungrouped.
fn apply(
    state: &AppState,
    entry: &crate::multistore::StoreEntry,
    registry: &GroupRegistry,
    tickets: &[hotsheet_model::Ticket],
    choice: &AutoGroupChoice,
) {
    let Some(ticket) = tickets.iter().find(|ticket| ticket.slug == choice.slug()) else {
        return;
    };
    // A person (or another writer) may have grouped it while the AI was thinking.
    match entry.store.read_ticket(&ticket.id) {
        Ok(current) if current.group.is_none() => {}
        _ => return,
    }
    let id = match choice {
        AutoGroupChoice::Existing { id, .. } => id.clone(),
        AutoGroupChoice::New { name, .. } => match registry.find_or_create(name) {
            Ok((id, _, _)) => id,
            Err(error) => {
                tracing::warn!(%error, "automatic grouping could not create a group");
                return;
            }
        },
    };
    let patch = ops::TicketPatch {
        group: Some(Some(id)),
        actor: Some(hotsheet_model::NoteActor {
            role: hotsheet_model::AttachmentActorRole::System,
            id: Some(AUTO_GROUP_ACTOR.into()),
        }),
        ..ops::TicketPatch::default()
    };
    match ops::update(&entry.store, &ticket.id, crate::now(), patch) {
        Ok(updated) => state.changed_in(entry, "updated", &updated),
        Err(error) => tracing::warn!(%error, "automatic grouping could not write a group"),
    }
}

/// Run one private turn of the project's default AI tool and return its text.
fn ask_default_tool(state: &AppState, request: &AskRequest<'_>) -> Result<String, String> {
    let tool = hotsheet_ticketing::sync_ai::configured_ai_tool(request.settings)
        .ok_or_else(|| "no default AI tool is configured for this project".to_string())?;
    let drivable = hotsheet_plugins::detected_drivable_plugins(&state.plugin_dirs)
        .iter()
        .any(|candidate| candidate.id() == tool);
    if !drivable {
        return Err(format!("the default AI tool {tool} cannot be driven here"));
    }
    let mut envs = vec![format!(
        "HOTSHEET_PROJECT={}",
        request.project_root.display()
    )];
    if let Some(url) = state.terminal_server_url.lock_or_recover().clone() {
        envs.push(format!("HOTSHEET_SERVER={url}"));
        envs.push(format!("HOTSHEET_SECRET={}", state.secret));
    }
    let trigger = hotsheet_aitools::prepare_trigger_with_home(
        request.store_root,
        &tool,
        hotsheet_aitools::TriggerOptions {
            project: Some(request.project_root.to_path_buf()),
            envs,
            journal_home: Some(state.machine_home().to_path_buf()),
            plugin_dirs: Some(state.plugin_dirs.to_vec()),
            ..hotsheet_aitools::TriggerOptions::default()
        },
    )
    .map_err(|error| error.to_string())?
    // The prompt asks for no tools; any approval still waits for a person, never auto-approves.
    .with_permission_bridge(state.permission_bridge());
    let control = hotsheet_aitools::TurnControl::default();
    let (finished, timer_done) = mpsc::channel::<()>();
    let timeout = control.clone();
    std::thread::spawn(move || {
        if timer_done.recv_timeout(TURN_LIMIT) == Err(RecvTimeoutError::Timeout) {
            timeout.request_interrupt();
        }
    });
    let mut output = String::new();
    let mut turns = hotsheet_aitools::ConnectionRegistry::new(30_000);
    let result = trigger.run_turn_controlled_with_options(
        hotsheet_aitools::TurnRequest {
            prompt: request.prompt,
            resume: None,
            session_cost_baseline_usd: None,
            model: None,
            effort: None,
            permission_mode: None,
            worker: true,
            conn_id: format!("hotsheet-auto-group-{}", Ulid::new()),
        },
        &mut turns,
        &control,
        &mut |event| {
            if let hotsheet_aitools::TurnEvent::Output(text) = event {
                output.push_str(text);
            }
        },
    );
    let _ = finished.send(());
    if control.interrupt_requested() {
        return Err("the AI did not answer in time".into());
    }
    result
        .map_err(|error| error.to_string())?
        .reason
        .session_result()?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket(minutes_old: i64, group: Option<&str>, status: Status) -> hotsheet_model::Ticket {
        hotsheet_model::Ticket {
            created_at: hotsheet_model::Timestamp::from_datetime(
                time::OffsetDateTime::now_utc() - time::Duration::minutes(minutes_old),
            ),
            group: group.map(str::to_owned),
            status,
            ..hotsheet_model::Ticket::default()
        }
    }

    use std::sync::Mutex;

    use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata};

    struct Harness {
        _dir: tempfile::TempDir,
        state: AppState,
        entry: crate::multistore::StoreEntry,
        code: PathBuf,
        prompts: Arc<Mutex<Vec<String>>>,
    }

    /// A served Git store linked to a registered checkout, with the grouper running on a
    /// scripted `ask` that answers `reply(prompt)` and records every prompt.
    fn harness(reply: impl Fn(&str) -> String + Send + Sync + 'static) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let code = dir.path().join("code");
        std::fs::create_dir_all(&code).unwrap();
        let store =
            FsStore::init_git(dir.path().join("tickets"), &StoreMetadata::new("HS")).unwrap();
        let registry = dir.path().join("home/checkouts.json");
        hotsheet_ticketing::checkouts::CheckoutRegistry::new(&registry)
            .register(&code, None, None, vec![store.root().to_path_buf()])
            .unwrap();
        let state = AppState::new(store, "secret".into())
            .unwrap()
            .with_machine_home(dir.path().join("home"))
            .with_checkout_registry(&registry);
        // The Automatic ticket grouping feature is experimental (off by default, HS2-3GJZGG).
        set_feature(&state, true);
        state.host.register(state.default_entry());
        let entry = state.default_entry();
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let seen = prompts.clone();
        drop(spawn_auto_grouper_with(
            state.clone(),
            Arc::new(move |request: &AskRequest<'_>| {
                seen.lock().unwrap().push(request.prompt.to_owned());
                Ok(reply(request.prompt))
            }),
        ));
        Harness {
            _dir: dir,
            state,
            entry,
            code,
            prompts,
        }
    }

    fn set_feature(state: &AppState, on: bool) {
        Settings::with_global_home(state.store.root(), state.machine_home())
            .set(
                "features.auto-grouping",
                serde_json::json!(on),
                hotsheet_ticketing::Scope::Global,
            )
            .unwrap();
    }

    fn create(harness: &Harness, title: &str, group: Option<&str>) -> hotsheet_model::Ticket {
        ops::create(
            &harness.entry.store,
            Ulid::new(),
            "HS",
            crate::now(),
            NewTicket {
                title: title.into(),
                group: group.map(str::to_owned),
                ..NewTicket::default()
            },
        )
        .unwrap()
    }

    async fn group_of(harness: &Harness, ticket: &hotsheet_model::Ticket) -> Option<String> {
        for _ in 0..200 {
            let current = harness.entry.store.read_ticket(&ticket.id).unwrap();
            if current.group.is_some() {
                return current.group;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        None
    }

    /// HS2-FN018Q: a server-created ticket and an externally written one (reported as
    /// `changed`) are grouped in one coalesced turn; a new name creates the group, an existing
    /// group is reused, and an already grouped ticket is never offered.
    #[tokio::test(flavor = "multi_thread")]
    async fn hs2_fn018q_new_tickets_are_grouped_in_one_coalesced_turn() {
        let harness = harness(|prompt| {
            let context: serde_json::Value =
                serde_json::from_str(prompt.lines().last().unwrap()).unwrap();
            let slugs: Vec<String> = context["tickets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|ticket| ticket["slug"].as_str().unwrap().to_owned())
                .collect();
            let existing = context["groups"][0]["id"].as_str().unwrap().to_owned();
            serde_json::json!({"assignments": [
                {"slug": slugs[0], "new_group": "Release notes"},
                {"slug": slugs[1], "group_id": existing},
            ]})
            .to_string()
        });
        let (billing, _, _) = GroupRegistry::for_checkout(&harness.code)
            .find_or_create("Billing")
            .unwrap();
        let grouped = create(&harness, "Already grouped", Some(&billing));
        let server = create(&harness, "Written through the server", None);
        let external = create(&harness, "Written by the CLI", None);
        harness
            .state
            .changed_in(&harness.entry, "created", &grouped);
        harness.state.changed_in(&harness.entry, "created", &server);
        harness
            .state
            .changed_in(&harness.entry, "changed", &external);

        let first = group_of(&harness, &server)
            .await
            .expect("server ticket grouped");
        let second = group_of(&harness, &external)
            .await
            .expect("external ticket grouped");
        let names = GroupRegistry::for_checkout(&harness.code).names().unwrap();
        assert_eq!(names[&first], "Release notes");
        assert_eq!(second, billing);
        let prompts = harness.prompts.lock().unwrap().clone();
        assert_eq!(prompts.len(), 1, "one coalesced turn: {prompts:?}");
        assert!(!prompts[0].contains("Already grouped"));
        // Edits to the grouped tickets do not ask again.
        harness.state.changed_in(&harness.entry, "updated", &server);
        harness
            .state
            .changed_in(&harness.entry, "changed", &external);
        tokio::time::sleep(COALESCE + Duration::from_millis(500)).await;
        assert_eq!(harness.prompts.lock().unwrap().len(), 1);
    }

    /// HS2-FN018Q: with the project's setting off no turn runs; an unusable reply writes nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn hs2_fn018q_setting_off_or_unusable_reply_writes_nothing() {
        let harness = harness(|_| "I would rather not.".into());
        Settings::for_project(&harness.code)
            .set(
                hotsheet_ticketing::AUTO_GROUP_NEW_TICKETS_SETTING,
                serde_json::json!(false),
                hotsheet_ticketing::Scope::Shared,
            )
            .unwrap();
        let off = create(&harness, "Setting off", None);
        harness.state.changed_in(&harness.entry, "created", &off);
        tokio::time::sleep(COALESCE + Duration::from_millis(500)).await;
        assert!(harness.prompts.lock().unwrap().is_empty());

        Settings::for_project(&harness.code)
            .set(
                hotsheet_ticketing::AUTO_GROUP_NEW_TICKETS_SETTING,
                serde_json::json!(true),
                hotsheet_ticketing::Scope::Shared,
            )
            .unwrap();
        let on = create(&harness, "Setting on", None);
        harness.state.changed_in(&harness.entry, "created", &on);
        for _ in 0..200 {
            if !harness.prompts.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(harness.prompts.lock().unwrap().len(), 1);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            harness
                .entry
                .store
                .read_ticket(&on.id)
                .unwrap()
                .group
                .is_none()
        );
    }

    /// HS2-3GJZGG: with the machine-wide Automatic ticket grouping feature off no turn runs,
    /// even though the project setting stays on; turning the feature back on resumes grouping.
    #[tokio::test(flavor = "multi_thread")]
    async fn hs2_3gjzgg_feature_off_spends_no_turn_and_keeps_the_project_setting() {
        let harness = harness(|_| "I would rather not.".into());
        set_feature(&harness.state, false);
        let off = create(&harness, "Feature off", None);
        harness.state.changed_in(&harness.entry, "created", &off);
        tokio::time::sleep(COALESCE + Duration::from_millis(500)).await;
        assert!(harness.prompts.lock().unwrap().is_empty());
        assert!(
            Settings::for_project(&harness.code)
                .auto_group_new_tickets()
                .unwrap(),
            "the project setting keeps its value"
        );

        set_feature(&harness.state, true);
        let on = create(&harness, "Feature on", None);
        harness.state.changed_in(&harness.entry, "created", &on);
        for _ in 0..200 {
            if !harness.prompts.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(harness.prompts.lock().unwrap().len(), 1);
    }

    #[test]
    fn hs2_fn018q_only_new_ungrouped_live_tickets_are_eligible() {
        let now = time::OffsetDateTime::now_utc();
        assert!(eligible(&ticket(0, None, Status::NotStarted), now));
        assert!(eligible(&ticket(9, None, Status::Backlog), now));
        assert!(!eligible(&ticket(11, None, Status::NotStarted), now));
        assert!(!eligible(&ticket(0, Some("g1"), Status::NotStarted), now));
        assert!(!eligible(&ticket(0, None, Status::Deleted), now));
        assert!(!eligible(&ticket(0, None, Status::Moved), now));
    }
}
