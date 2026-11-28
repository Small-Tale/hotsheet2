//! HS2-WT82VP: opt-in live check of linked feedback replies (HS2-W1ENZD) against the
//! approved disposable GitHub, GitLab, and Jira test projects. Every test is `#[ignore]`d
//! and refuses to write without its provider's opt-in, so the default suite never touches
//! a tracker. Run one provider, for example:
//!
//! ```text
//! op run --env-file=.env -- env HOTSHEET_GITLAB_LIVE_WRITE=1 \
//!   cargo nextest run -p hotsheet-extsync --test feedback_replies_live \
//!   --run-ignored only --no-capture
//! ```
//!
//! The `*_ai_author_marker_round_trips` tests (HS2-533PP8) check the AI author marker
//! (HS2-Z3SBRQ) the same way on a Not Started issue.
//!
//! Each reply-order test creates one throwaway issue, posts two feedback requests, answers the newest
//! first, reads the issue back after each step, copies it into a temporary Git store while
//! one request is open and again after both are answered, then closes the issue. Native
//! issue and comment ids are printed so a run can be recorded; tokens never are.

use std::sync::Arc;
use std::time::Duration;

use hotsheet_extsync::{
    GitHubConfig, GitHubProvider, GitHubTransport, GitLabConfig, GitLabProvider, JiraConfig,
    JiraProvider, UreqGitHubTransport,
};
use hotsheet_model::{
    AttachmentActorRole, CloseReason, NoteActor, NoteKind, Priority, Status, Timestamp, Ulid,
};
use hotsheet_ticketing::ops::NoteMetadataInput;
use hotsheet_ticketing::{
    ApiTicket, FsStore, GitProvider, MutationContext, ProviderConnection, ProviderDraft,
    ProviderPatch, ProviderRegistry, StoreMetadata, TicketProvider, TicketRef, copy_between,
};
use serde_json::json;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for this live test"))
}

fn require_opt_in(name: &str) {
    assert_eq!(
        std::env::var(name).as_deref(),
        Ok("1"),
        "write access to the live test project must be opted into with {name}=1"
    );
}

fn now() -> Timestamp {
    Timestamp::new(
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
    )
}

fn ctx() -> MutationContext {
    MutationContext {
        now: now(),
        generated_id: Ulid::new(),
    }
}

fn metadata_by(actor: Option<NoteActor>, response_to: Option<String>) -> NoteMetadataInput {
    NoteMetadataInput {
        kind_explicit: true,
        human_edited: false,
        summary: None,
        confidence: None,
        actor,
        ai_feedback: None,
        response_to,
    }
}

/// Post one note and return the tracker's native id for it, found by the Hot Sheet note
/// id the adapter embeds, and the issue as the tracker now reports it.
fn post(
    provider: &dyn TicketProvider,
    native_id: &str,
    kind: NoteKind,
    response_to: Option<String>,
    text: &str,
) -> String {
    post_by(provider, native_id, kind, None, response_to, text)
}

/// [`post`] with the note's author, as the checkout write route passes it.
fn post_by(
    provider: &dyn TicketProvider,
    native_id: &str,
    kind: NoteKind,
    actor: Option<NoteActor>,
    response_to: Option<String>,
    text: &str,
) -> String {
    // Trackers order comments by second-granularity timestamps.
    std::thread::sleep(Duration::from_millis(1100));
    let context = ctx();
    let generated = context.generated_id.to_string();
    let ticket = provider
        .add_note_with_metadata(
            native_id,
            context,
            kind,
            metadata_by(actor, response_to),
            text.into(),
        )
        .unwrap();
    ticket
        .notes
        .iter()
        .find(|note| note.hotsheet_note_id.as_deref() == Some(generated.as_str()))
        .unwrap_or_else(|| panic!("posted note {generated} is not on the issue read back"))
        .id
        .clone()
}

fn open_requests(ticket: &ApiTicket) -> Vec<String> {
    ticket
        .notes
        .iter()
        .filter(|request| {
            request.kind == NoteKind::FeedbackNeeded
                && request.feedback_request
                && !ticket
                    .notes
                    .iter()
                    .any(|reply| reply.response_to.as_deref() == Some(request.id.as_str()))
        })
        .map(|request| request.id.clone())
        .collect()
}

/// Copy the issue into a fresh Git store and return the copy as that store reports it.
fn copy_to_git(provider: Arc<dyn TicketProvider>, native_id: &str, label: &str) -> ApiTicket {
    let directory = tempfile::tempdir().unwrap();
    let store = FsStore::init(directory.path(), &StoreMetadata::new("CPY")).unwrap();
    let registry = ProviderRegistry::default();
    let source_connection = provider.descriptor().connection_id;
    registry.register(provider).unwrap();
    registry
        .register(Arc::new(GitProvider::new("live-copy", store)))
        .unwrap();
    let outcome = copy_between(
        &registry,
        TicketRef {
            connection_id: source_connection,
            native_id: native_id.into(),
        },
        "live-copy",
        &format!("hs2-wt82vp-{label}-{}", Ulid::new()),
        now(),
    )
    .unwrap();
    registry
        .get("live-copy")
        .unwrap()
        .get(&outcome.destination.native_id)
        .unwrap()
}

/// The HS2-WT82VP sequence: two requests, newest answered first, older still open,
/// then both answered, each checked on a fresh read and through provider copy.
/// `category` is the tracker's issue type for the throwaway issue.
fn reply_order_round_trip(provider: Arc<dyn TicketProvider>, name: &str, category: &str) {
    let created = provider
        .create(
            ctx(),
            ProviderDraft {
group: None,
                title: format!("Hot Sheet feedback reply live test {}", Ulid::new()),
                category: category.into(),
                priority: Priority::Default,
                status: Status::Started,
                details: "Created by an opt-in Hot Sheet live validation (HS2-WT82VP); closed automatically.".into(),
                tags: vec![],
                up_next: false,
                blocked_by: vec![],
                transfer: None,
                created_by: None,
                due_on: None,
                milestone: None,
            },
        )
        .unwrap();
    let id = created.native_id.clone();
    eprintln!("{name}: issue {id}");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let older = post(
            provider.as_ref(),
            &id,
            NoteKind::FeedbackNeeded,
            None,
            "FEEDBACK NEEDED: first synthetic question.",
        );
        let newer = post(
            provider.as_ref(),
            &id,
            NoteKind::FeedbackNeeded,
            None,
            "FEEDBACK NEEDED: second synthetic question.",
        );
        let both_open = provider.get(&id).unwrap();
        let mut expected = vec![older.clone(), newer.clone()];
        expected.sort();
        let mut open = open_requests(&both_open);
        open.sort();
        assert_eq!(open, expected, "{name}: both requests open after posting");
        assert!(
            both_open.feedback_needed,
            "{name}: two open requests need feedback"
        );

        let newer_answer = post(
            provider.as_ref(),
            &id,
            NoteKind::Regular,
            Some(newer.clone()),
            "Synthetic answer to the second question.",
        );
        let one_open = provider.get(&id).unwrap();
        assert_eq!(
            open_requests(&one_open),
            vec![older.clone()],
            "{name}: answering the newest leaves the older request open"
        );
        assert!(
            one_open.feedback_needed,
            "{name}: the older request still needs feedback"
        );
        let copy_open = copy_to_git(provider.clone(), &id, "open");
        assert!(
            copy_open.feedback_needed,
            "{name}: a copy keeps the open request"
        );
        assert_eq!(
            open_requests(&copy_open).len(),
            1,
            "{name}: the copy has one open request"
        );

        let older_answer = post(
            provider.as_ref(),
            &id,
            NoteKind::Regular,
            Some(older.clone()),
            "Synthetic answer to the first question.",
        );
        let answered = provider.get(&id).unwrap();
        assert!(
            open_requests(&answered).is_empty(),
            "{name}: both requests answered"
        );
        assert!(
            !answered.feedback_needed,
            "{name}: no feedback needed once both are answered"
        );
        let copy_closed = copy_to_git(provider.clone(), &id, "answered");
        assert!(
            !copy_closed.feedback_needed,
            "{name}: the copy is answered too"
        );
        assert!(
            open_requests(&copy_closed).is_empty(),
            "{name}: the copy links both replies"
        );
        assert_eq!(
            copy_closed
                .notes
                .iter()
                .filter(|note| note.response_to.is_some())
                .count(),
            2,
            "{name}: both replies keep their request link in the copy"
        );
        eprintln!(
            "{name}: requests older={older} newer={newer}; replies newer={newer_answer} older={older_answer}; feedback_needed two-open={} one-open={} answered={}",
            both_open.feedback_needed, one_open.feedback_needed, answered.feedback_needed
        );
    }));
    close_best_effort(provider.as_ref(), name, &id);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

/// Cleanup is best effort: a tracker without a mapped close workflow (Jira) reports the
/// operation as unsupported, which is not a feedback failure.
fn close_best_effort(provider: &dyn TicketProvider, name: &str, id: &str) {
    if let Err(close) = provider.close(id, now(), CloseReason::Completed, None) {
        let patch = ProviderPatch {
            status: Some(Status::Completed),
            ..Default::default()
        };
        if let Err(update) = provider.update(id, now(), patch) {
            eprintln!("{name}: left issue {id} open; close: {close:?}; complete: {update:?}");
        }
    }
}

fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

/// Posts a comment straight to the tracker, without Hot Sheet's note marker, as a person
/// or another tool would: `(native issue id, comment body)`.
type RawComment<'a> = &'a dyn Fn(&str, &str);

/// HS2-533PP8: the AI author marker (HS2-Z3SBRQ) on a live tracker. On a Not Started
/// issue an AI's FEEDBACK NEEDED note reads back with its AI author; a person's answer,
/// a person's note whose text imitates the marker, and (where `raw` can post one) a
/// comment carrying the marker line without a valid note marker never read as AI; the
/// answer links to the request whose author is the AI the server routes it to. Every
/// step is checked on a fresh read, and the issue is closed afterwards.
fn ai_author_round_trip(
    provider: Arc<dyn TicketProvider>,
    name: &str,
    category: &str,
    raw: Option<RawComment<'_>>,
) {
    let created = provider
        .create(
            ctx(),
            ProviderDraft {
                group: None,
                title: format!("Hot Sheet AI author live test {}", Ulid::new()),
                category: category.into(),
                priority: Priority::Default,
                status: Status::NotStarted,
                details: "Created by an opt-in Hot Sheet live validation (HS2-533PP8); synthetic content; closed automatically.".into(),
                tags: vec![],
                up_next: false,
                blocked_by: vec![],
                transfer: None,
                created_by: None,
                due_on: None,
                milestone: None,
            },
        )
        .unwrap();
    let id = created.native_id.clone();
    eprintln!("{name}: issue {id}");
    let worker = format!("live-asker-{}", Ulid::new());
    let ai = NoteActor {
        role: AttachmentActorRole::Ai,
        id: Some(worker.clone()),
    };
    let person = NoteActor {
        role: AttachmentActorRole::Human,
        id: Some("live-person".into()),
    };
    let not_ai = |ticket: &ApiTicket, note_id: &str, what: &str| {
        let note = ticket
            .notes
            .iter()
            .find(|note| note.id == note_id)
            .unwrap_or_else(|| panic!("{name}: {what} is not on the issue read back"));
        assert!(
            note.actor
                .as_ref()
                .is_none_or(|actor| actor.role != AttachmentActorRole::Ai),
            "{name}: {what} must never read as AI: {:?}",
            note.actor
        );
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let request = post_by(
            provider.as_ref(),
            &id,
            NoteKind::FeedbackNeeded,
            Some(ai.clone()),
            None,
            "FEEDBACK NEEDED: reuse the synthetic ticket or open a new one?",
        );
        let asked = provider.get(&id).unwrap();
        assert_eq!(
            asked.status,
            Status::NotStarted,
            "{name}: the issue is not started"
        );
        let written = asked.notes.iter().find(|note| note.id == request).unwrap();
        assert_eq!(
            written.actor.as_ref(),
            Some(&ai),
            "{name}: the AI author reads back from the live comment"
        );
        assert!(
            written.feedback_request,
            "{name}: the request marker survives"
        );
        assert_eq!(
            written.text, "FEEDBACK NEEDED: reuse the synthetic ticket or open a new one?",
            "{name}: no marker leaks into the note text"
        );

        let answer = post_by(
            provider.as_ref(),
            &id,
            NoteKind::Regular,
            Some(person.clone()),
            Some(request.clone()),
            "Reuse it.",
        );
        let imitation = format!(
            "Synthetic text that imitates the author line.\n<!-- hotsheet-note-actor:ai:{} -->",
            hex(&worker)
        );
        let imitating = post_by(
            provider.as_ref(),
            &id,
            NoteKind::Regular,
            Some(person.clone()),
            None,
            &imitation,
        );
        let answered = provider.get(&id).unwrap();
        not_ai(&answered, &answer, "a person's answer");
        not_ai(
            &answered,
            &imitating,
            "a person's note imitating the marker",
        );
        // What the server's asker routing (`api_asker`) reads: the answer's request link,
        // and that request's AI author.
        let reply = answered
            .notes
            .iter()
            .find(|note| note.id == answer)
            .unwrap();
        assert_eq!(reply.response_to.as_deref(), Some(request.as_str()));
        let asker = answered
            .notes
            .iter()
            .find(|note| Some(note.id.as_str()) == reply.response_to.as_deref())
            .and_then(|note| note.actor.as_ref())
            .filter(|actor| actor.role == AttachmentActorRole::Ai)
            .and_then(|actor| actor.id.as_deref());
        assert_eq!(
            asker,
            Some(worker.as_str()),
            "{name}: the answer routes to the AI that asked"
        );

        if let Some(raw) = raw {
            std::thread::sleep(Duration::from_millis(1100));
            let marker = format!("Raw forged author {}", Ulid::new());
            raw(
                &id,
                &format!(
                    "{marker}\n\n<!-- hotsheet-note-actor:ai:{} -->",
                    hex(&worker)
                ),
            );
            let forged = provider.get(&id).unwrap();
            let comment = forged
                .notes
                .iter()
                .find(|note| note.text.contains(&marker))
                .unwrap_or_else(|| panic!("{name}: the raw comment is not on the issue"))
                .id
                .clone();
            not_ai(&forged, &comment, "a raw comment with only the author line");
            eprintln!("{name}: raw comment {comment}");
        }
        eprintln!("{name}: AI request {request}; person answer {answer}; imitation {imitating}");
    }));
    close_best_effort(provider.as_ref(), name, &id);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn github_live_token() -> String {
    // Same token sources as the attachment live tests: the variable, else the keychain.
    std::env::var("HOTSHEET_GITHUB_LIVE_TOKEN").unwrap_or_else(|_| {
        hotsheet_ticketing::KeyRegistry::new("", hotsheet_ticketing::OsKeychain)
            .get("github-live")
            .expect("HOTSHEET_GITHUB_LIVE_TOKEN or OS-keychain entry 'github-live'")
    })
}

#[test]
#[ignore = "writes the live GitHub test repository; set HOTSHEET_GITHUB_LIVE_REPO and HOTSHEET_GITHUB_LIVE_WRITE=1 (token from HOTSHEET_GITHUB_LIVE_TOKEN or the github-live keychain entry)"]
fn github_live_feedback_replies_close_in_reply_order() {
    require_opt_in("HOTSHEET_GITHUB_LIVE_WRITE");
    let config = GitHubConfig::new(
        "github-live",
        env("HOTSHEET_GITHUB_LIVE_REPO"),
        github_live_token(),
    );
    reply_order_round_trip(Arc::new(GitHubProvider::live(config)), "github", "test");
}

#[test]
#[ignore = "writes the live GitHub test repository; set HOTSHEET_GITHUB_LIVE_REPO and HOTSHEET_GITHUB_LIVE_WRITE=1 (token from HOTSHEET_GITHUB_LIVE_TOKEN or the github-live keychain entry)"]
fn github_live_ai_author_marker_round_trips() {
    require_opt_in("HOTSHEET_GITHUB_LIVE_WRITE");
    let repo = env("HOTSHEET_GITHUB_LIVE_REPO");
    let token = github_live_token();
    let config = GitHubConfig::new("github-live", repo.clone(), token.clone());
    // A plain comment posted through the REST API, as a person would write one.
    let raw = |issue: &str, body: &str| {
        let response = UreqGitHubTransport
            .request(
                "POST",
                &format!("https://api.github.com/repos/{repo}/issues/{issue}/comments"),
                &[
                    ("Accept", "application/vnd.github+json".into()),
                    ("Authorization", format!("Bearer {token}")),
                    ("User-Agent", "hotsheet2-live-test".into()),
                ],
                Some(&json!({ "body": body })),
            )
            .unwrap();
        assert_eq!(response.status, 201, "raw comment was not created");
    };
    ai_author_round_trip(
        Arc::new(GitHubProvider::live(config)),
        "github",
        "test",
        Some(&raw),
    );
}

#[test]
#[ignore = "writes the live GitLab test project; set HOTSHEET_GITLAB_LIVE_PROJECT, HOTSHEET_GITLAB_LIVE_TOKEN and HOTSHEET_GITLAB_LIVE_WRITE=1"]
fn gitlab_live_feedback_replies_close_in_reply_order() {
    require_opt_in("HOTSHEET_GITLAB_LIVE_WRITE");
    reply_order_round_trip(gitlab_live_provider(), "gitlab", "test");
}

fn gitlab_live_provider() -> Arc<dyn TicketProvider> {
    let connection = ProviderConnection {
        id: "gitlab-live".into(),
        provider: "gitlab".into(),
        locator: env("HOTSHEET_GITLAB_LIVE_PROJECT"),
        name: None,
        default: false,
        settings: json!({"api_base": std::env::var("HOTSHEET_GITLAB_LIVE_API_BASE")
            .unwrap_or_else(|_| "https://gitlab.com/api/v4".into())}),
        disabled: false,
    };
    let config =
        GitLabConfig::from_connection(&connection, env("HOTSHEET_GITLAB_LIVE_TOKEN")).unwrap();
    Arc::new(GitLabProvider::live(config))
}

#[test]
#[ignore = "writes the live GitLab test project; set HOTSHEET_GITLAB_LIVE_PROJECT, HOTSHEET_GITLAB_LIVE_TOKEN and HOTSHEET_GITLAB_LIVE_WRITE=1"]
fn gitlab_live_ai_author_marker_round_trips() {
    require_opt_in("HOTSHEET_GITLAB_LIVE_WRITE");
    ai_author_round_trip(gitlab_live_provider(), "gitlab", "test", None);
}

#[test]
#[ignore = "writes the live Jira test project; set HOTSHEET_JIRA_LIVE_BASE_URL/PROJECT/EMAIL/TOKEN and HOTSHEET_JIRA_LIVE_WRITE=1"]
fn jira_live_feedback_replies_close_in_reply_order() {
    require_opt_in("HOTSHEET_JIRA_LIVE_WRITE");
    let connection = ProviderConnection {
        id: "jira-live".into(),
        provider: "jira".into(),
        locator: env("HOTSHEET_JIRA_LIVE_PROJECT"),
        name: None,
        default: false,
        settings: json!({
            "base_url": env("HOTSHEET_JIRA_LIVE_BASE_URL"),
            "email": env("HOTSHEET_JIRA_LIVE_EMAIL"),
        }),
        disabled: false,
    };
    let config = JiraConfig::from_connection(&connection, env("HOTSHEET_JIRA_LIVE_TOKEN")).unwrap();
    // Jira takes the category as its issue type name; the project defines the valid ones.
    let issue_type =
        std::env::var("HOTSHEET_JIRA_LIVE_ISSUE_TYPE").unwrap_or_else(|_| "Task".into());
    reply_order_round_trip(Arc::new(JiraProvider::live(config)), "jira", &issue_type);
}

#[test]
#[ignore = "writes the live Jira test project; set HOTSHEET_JIRA_LIVE_BASE_URL/PROJECT/EMAIL/TOKEN and HOTSHEET_JIRA_LIVE_WRITE=1"]
fn jira_live_ai_author_marker_round_trips() {
    require_opt_in("HOTSHEET_JIRA_LIVE_WRITE");
    let connection = ProviderConnection {
        id: "jira-live".into(),
        provider: "jira".into(),
        locator: env("HOTSHEET_JIRA_LIVE_PROJECT"),
        name: None,
        default: false,
        settings: json!({
            "base_url": env("HOTSHEET_JIRA_LIVE_BASE_URL"),
            "email": env("HOTSHEET_JIRA_LIVE_EMAIL"),
        }),
        disabled: false,
    };
    let config = JiraConfig::from_connection(&connection, env("HOTSHEET_JIRA_LIVE_TOKEN")).unwrap();
    let issue_type =
        std::env::var("HOTSHEET_JIRA_LIVE_ISSUE_TYPE").unwrap_or_else(|_| "Task".into());
    // The ADF round trip is the point here: the author line must survive Jira's rich text.
    ai_author_round_trip(
        Arc::new(JiraProvider::live(config)),
        "jira",
        &issue_type,
        None,
    );
}
