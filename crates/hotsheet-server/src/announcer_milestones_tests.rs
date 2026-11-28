//! HS2-G3HJZH: ticket writes through the real change bus become recorded announcer milestones,
//! decided by a scripted model or by the rules, never by a real one.

use std::sync::Mutex;

use hotsheet_model::{NoteActor, NoteKind, Status};
use hotsheet_ticketing::activity::ActivityEvent;
use hotsheet_ticketing::ops::{self, NoteMetadataInput};
use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, TicketPatch};
use serde_json::json;

use super::*;
use crate::multistore::StoreEntry;

/// The scripted answer to one prompt.
type Reply = Box<dyn Fn(AiTask, &str) -> Result<String, String> + Send + Sync>;

/// A scripted model that records every prompt.
struct Fake {
    prompts: Mutex<Vec<(AiTask, String)>>,
    reply: Reply,
}

impl MilestoneAi for Fake {
    fn ask(&self, task: AiTask, prompt: &str) -> Result<String, String> {
        self.prompts.lock().unwrap().push((task, prompt.to_owned()));
        (self.reply)(task, prompt)
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    state: AppState,
    entry: StoreEntry,
    events: tokio::sync::broadcast::Receiver<crate::events::ChangeEvent>,
}

fn harness(ai: Option<SharedAi>) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(dir.path().join("tickets"), &StoreMetadata::new("HS2")).unwrap();
    let state = AppState::new(store, "secret".into())
        .unwrap()
        .with_machine_home(dir.path().join("home"));
    state.host.register(state.default_entry());
    let entry = state.default_entry();
    let events = state.subscribe();
    drop(spawn_announcer_milestones_with(
        state.clone(),
        ai,
        Timing {
            coalesce: Duration::from_millis(150),
            max_wait: Duration::from_secs(2),
        },
    ));
    Harness {
        _dir: dir,
        state,
        entry,
        events,
    }
}

fn ai_actor() -> Option<NoteActor> {
    Some(NoteActor {
        role: AttachmentActorRole::Ai,
        id: Some("claude-4b58913f".into()),
    })
}

fn create(harness: &Harness, title: &str) -> hotsheet_model::Ticket {
    let ticket = ops::create(
        &harness.entry.store,
        Ulid::new(),
        "HS2",
        crate::now(),
        NewTicket {
            title: title.into(),
            ..NewTicket::default()
        },
    )
    .unwrap();
    harness.state.changed_in(&harness.entry, "created", &ticket);
    ticket
}

fn note(
    harness: &Harness,
    ticket: &hotsheet_model::Ticket,
    kind: NoteKind,
    text: &str,
    summary: &str,
) {
    let updated = ops::add_note_with_metadata(
        &harness.entry.store,
        &ticket.id,
        Ulid::new(),
        crate::now(),
        kind,
        NoteMetadataInput {
            kind_explicit: true,
            summary: Some(summary.into()),
            actor: ai_actor(),
            ..NoteMetadataInput::default()
        },
        text.into(),
    )
    .unwrap();
    harness
        .state
        .changed_in(&harness.entry, "updated", &updated);
}

/// Wait for the next recorded milestone on the bus (the event the client receives).
async fn next_milestone(harness: &mut Harness) -> ActivityEvent {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = harness.events.recv().await.unwrap();
            if let Some(activity) = event.activity
                && activity.detail.get("milestone").is_some()
            {
                return activity;
            }
        }
    })
    .await
    .expect("a milestone is recorded")
}

/// Nothing more arrives for a while.
async fn quiet(harness: &mut Harness) {
    let more = tokio::time::timeout(Duration::from_millis(600), async {
        loop {
            let event = harness.events.recv().await.unwrap();
            if event.activity.is_some() {
                return event;
            }
        }
    })
    .await;
    assert!(more.is_err(), "unexpected activity: {more:?}");
}

/// HS2-G3HJZH: with no model, an AI note is recorded and pushed with its rule score and template
/// line; a person's note is not; and the same write observed again is not said twice.
#[tokio::test(flavor = "multi_thread")]
async fn hs2_g3hjzh_ai_notes_are_recorded_with_rules_and_templates() {
    let mut harness = harness(None);
    let ticket = create(&harness, "Code organization and documentation");
    note(
        &harness,
        &ticket,
        NoteKind::Regular,
        "## Preliminary thoughts\n\nSecret body text that must not be spoken.",
        "Several files are over our 1000-line limit",
    );
    let event = next_milestone(&mut harness).await;
    assert_eq!(event.tool, "claude");
    assert_eq!(
        event.ticket.as_deref(),
        Some(ticket.id.to_string().as_str())
    );
    assert_eq!(event.detail["milestone"], "initial_thoughts");
    assert_eq!(event.detail["scored_by"], "rules");
    assert_eq!(event.detail["worded_by"], "template");
    assert_eq!(event.detail["speak"], true);
    let short = hotsheet_ticketing::announcer_milestones::short_slug(&ticket.slug);
    assert_eq!(
        event.detail["spoken"],
        format!(
            "I started investigating a ticket focusing on Code organization and documentation \
             ({short}). My initial thoughts: Several files are over our 1000-line limit."
        )
    );
    assert!(!event.summary.contains("Secret body"));
    // Persisted in the rolling window the digest reads.
    let stored = hotsheet_ticketing::activity::read_recent(&harness.entry.store).unwrap();
    assert!(stored.iter().any(|recorded| recorded.id == event.id));
    // The same write reported again, and a person's note, say nothing.
    let current = harness.entry.store.read_ticket(&ticket.id).unwrap();
    harness
        .state
        .changed_in(&harness.entry, "changed", &current);
    let person = ops::add_note_with_metadata(
        &harness.entry.store,
        &ticket.id,
        Ulid::new(),
        crate::now(),
        NoteKind::Regular,
        NoteMetadataInput {
            summary: Some("A person's note".into()),
            actor: Some(NoteActor {
                role: AttachmentActorRole::Human,
                id: None,
            }),
            ..NoteMetadataInput::default()
        },
        "hi".into(),
    )
    .unwrap();
    harness.state.changed_in(&harness.entry, "updated", &person);
    quiet(&mut harness).await;
}

/// HS2-G3HJZH: a model scores and words one coalesced batch with one call each; its prompts carry
/// summaries and titles but never note bodies; a completion takes the note written with it.
#[tokio::test(flavor = "multi_thread")]
async fn hs2_g3hjzh_a_model_scores_and_words_one_coalesced_batch() {
    let fake = Arc::new(Fake {
        prompts: Mutex::default(),
        reply: Box::new(|task, prompt| {
            let items: Vec<serde_json::Value> =
                serde_json::from_str(prompt.split("Milestones:\n").nth(1).unwrap()).unwrap();
            let payload = match task {
                AiTask::Classify => json!({"scores": items
                    .iter()
                    .map(|item| json!({"id": item["id"], "score": 88}))
                    .collect::<Vec<_>>()}),
                AiTask::Rewrite => json!({"lines": items
                    .iter()
                    .map(|item| json!({
                        "id": item["id"],
                        "speech": format!(
                            "I wrapped up {} and every gate is green.",
                            item["ticket"].as_str().unwrap()
                        )
                    }))
                    .collect::<Vec<_>>()}),
            };
            Ok(json!({"is_error": false, "structured_output": payload}).to_string())
        }),
    });
    let mut harness = harness(Some(fake.clone()));
    let ticket = create(&harness, "Ship the widget");
    let started = ops::update(
        &harness.entry.store,
        &ticket.id,
        crate::now(),
        TicketPatch {
            status: Some(Status::Started),
            actor: ai_actor(),
            ..TicketPatch::default()
        },
    )
    .unwrap();
    harness
        .state
        .changed_in(&harness.entry, "updated", &started);
    // The start alone scores 88 by the fake; wait for it so the next batch is separate.
    let first = next_milestone(&mut harness).await;
    assert_eq!(first.detail["milestone"], "started");
    note(
        &harness,
        &ticket,
        NoteKind::Regular,
        "## Done\n\nPrivate details of the work.",
        "All gates green",
    );
    let done = ops::update(
        &harness.entry.store,
        &ticket.id,
        crate::now(),
        TicketPatch {
            status: Some(Status::Completed),
            actor: ai_actor(),
            ..TicketPatch::default()
        },
    )
    .unwrap();
    harness.state.changed_in(&harness.entry, "updated", &done);
    let event = next_milestone(&mut harness).await;
    assert_eq!(event.detail["milestone"], "completed");
    assert_eq!(event.detail["note_summary"], "All gates green");
    assert_eq!(event.detail["scored_by"], "ai");
    assert_eq!(event.detail["score"], 88);
    assert_eq!(event.detail["worded_by"], "ai");
    let short = hotsheet_ticketing::announcer_milestones::short_slug(&ticket.slug);
    assert_eq!(
        event.detail["spoken"],
        format!("I wrapped up {short} and every gate is green.")
    );
    quiet(&mut harness).await;
    let prompts = fake.prompts.lock().unwrap().clone();
    let tasks: Vec<AiTask> = prompts.iter().map(|(task, _)| *task).collect();
    // The start: classify, rewrite. The coalesced completion: classify, rewrite.
    assert_eq!(
        tasks,
        [
            AiTask::Classify,
            AiTask::Rewrite,
            AiTask::Classify,
            AiTask::Rewrite
        ]
    );
    for (_, prompt) in &prompts {
        assert!(!prompt.contains("Private details"), "{prompt}");
        assert!(prompt.contains("Ship the widget"));
    }
}

/// HS2-G3HJZH: both switches off, the model is never asked; the routes read and write them.
#[tokio::test(flavor = "multi_thread")]
async fn hs2_g3hjzh_switches_off_never_ask_the_model() {
    let fake = Arc::new(Fake {
        prompts: Mutex::default(),
        reply: Box::new(|_, _| Err("must not be asked".into())),
    });
    let mut harness = harness(Some(fake.clone()));
    assert_eq!(read_ai_settings(&harness.state), AiSettings::default());
    let Json(updated) = put_announcer_settings(
        State(harness.state.clone()),
        Json(AnnouncerSettingsPatch {
            ai_importance: Some(false),
            ai_rewrite: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        updated,
        AiSettings {
            ai_importance: false,
            ai_rewrite: true
        }
    );
    let Json(_) = put_announcer_settings(
        State(harness.state.clone()),
        Json(AnnouncerSettingsPatch {
            ai_importance: None,
            ai_rewrite: Some(false),
        }),
    )
    .await
    .unwrap();
    let Json(read) = get_announcer_settings(State(harness.state.clone()))
        .await
        .unwrap();
    assert_eq!(
        read,
        AiSettings {
            ai_importance: false,
            ai_rewrite: false
        }
    );
    let ticket = create(&harness, "Pick a color");
    note(
        &harness,
        &ticket,
        NoteKind::FeedbackNeeded,
        "FEEDBACK NEEDED\n\nWhich accent?",
        "Which accent color",
    );
    let event = next_milestone(&mut harness).await;
    assert_eq!(event.detail["milestone"], "question");
    assert_eq!(event.detail["scored_by"], "rules");
    assert!(fake.prompts.lock().unwrap().is_empty());
}

/// HS2-G3HJZH, HS2-17NWC1: the live call's arguments come from the Claude plugin's declared
/// structured call and ask for no tools, MCP servers, hooks, or saved session, and for the small
/// model's structured JSON.
#[test]
fn hs2_g3hjzh_cli_arguments_are_bounded() {
    let template = hotsheet_plugins::builtin_plugins()
        .into_iter()
        .filter_map(|plugin| plugin.manifest.drive)
        .find(|drive| !drive.structured_call_args.is_empty())
        .expect("a built-in plugin declares a structured call")
        .structured_call_args;
    let args = cli_args(&template, AiTask::Classify, "PROMPT");
    let after = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .map(|at| args[at + 1].as_str())
    };
    assert_eq!(after("-p"), Some("PROMPT"));
    assert_eq!(after("--model"), Some("claude-haiku-5-5"));
    assert_eq!(
        after("--system-prompt"),
        Some(AiTask::Classify.system_prompt())
    );
    assert_eq!(after("--output-format"), Some("json"));
    assert_eq!(after("--json-schema"), Some(AiTask::Classify.schema()));
    assert_eq!(after("--tools"), Some(""));
    assert_eq!(after("--settings"), Some(r#"{"disableAllHooks":true}"#));
    for flag in [
        "--strict-mcp-config",
        "--no-session-persistence",
        "--disable-slash-commands",
    ] {
        assert!(args.iter().any(|arg| arg == flag), "{flag}");
    }
    // A test process never runs the installed tool.
    let live = PluginCliAi {
        plugin_dirs: Vec::new(),
    };
    assert!(live.ask(AiTask::Classify, "x").is_err());
}
