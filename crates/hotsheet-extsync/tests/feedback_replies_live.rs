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
//! Each test creates one throwaway issue, posts two feedback requests, answers the newest
//! first, reads the issue back after each step, copies it into a temporary Git store while
//! one request is open and again after both are answered, then closes the issue. Native
//! issue and comment ids are printed so a run can be recorded; tokens never are.

use std::sync::Arc;
use std::time::Duration;

use hotsheet_extsync::{
    GitHubConfig, GitHubProvider, GitLabConfig, GitLabProvider, JiraConfig, JiraProvider,
};
use hotsheet_model::{CloseReason, NoteKind, Priority, Status, Timestamp, Ulid};
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

fn metadata(response_to: Option<String>) -> NoteMetadataInput {
    NoteMetadataInput {
        kind_explicit: true,
        human_edited: false,
        summary: None,
        confidence: None,
        actor: None,
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
    // Trackers order comments by second-granularity timestamps.
    std::thread::sleep(Duration::from_millis(1100));
    let context = ctx();
    let generated = context.generated_id.to_string();
    let ticket = provider
        .add_note_with_metadata(native_id, context, kind, metadata(response_to), text.into())
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
                title: format!("Hot Sheet feedback reply live test {}", Ulid::new()),
                category: category.into(),
                priority: Priority::Default,
                status: Status::Started,
                details: "Created by an opt-in Hot Sheet live validation (HS2-WT82VP); closed automatically.".into(),
                tags: vec![],
                up_next: false,
                blocked_by: vec![],
                transfer: None,
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
    // Cleanup is best effort: a tracker without a mapped close workflow (Jira) reports the
    // operation as unsupported, which is not a feedback-reply failure.
    if let Err(close) = provider.close(&id, now(), CloseReason::Completed, None) {
        let patch = ProviderPatch {
            status: Some(Status::Completed),
            ..Default::default()
        };
        if let Err(update) = provider.update(&id, now(), patch) {
            eprintln!("{name}: left issue {id} open; close: {close:?}; complete: {update:?}");
        }
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[test]
#[ignore = "writes the live GitHub test repository; set HOTSHEET_GITHUB_LIVE_REPO and HOTSHEET_GITHUB_LIVE_WRITE=1 (token from HOTSHEET_GITHUB_LIVE_TOKEN or the github-live keychain entry)"]
fn github_live_feedback_replies_close_in_reply_order() {
    require_opt_in("HOTSHEET_GITHUB_LIVE_WRITE");
    // Same token sources as the attachment live tests: the variable, else the keychain.
    let token = std::env::var("HOTSHEET_GITHUB_LIVE_TOKEN").unwrap_or_else(|_| {
        hotsheet_ticketing::KeyRegistry::new("", hotsheet_ticketing::OsKeychain)
            .get("github-live")
            .expect("HOTSHEET_GITHUB_LIVE_TOKEN or OS-keychain entry 'github-live'")
    });
    let config = GitHubConfig::new("github-live", env("HOTSHEET_GITHUB_LIVE_REPO"), token);
    reply_order_round_trip(Arc::new(GitHubProvider::live(config)), "github", "test");
}

#[test]
#[ignore = "writes the live GitLab test project; set HOTSHEET_GITLAB_LIVE_PROJECT, HOTSHEET_GITLAB_LIVE_TOKEN and HOTSHEET_GITLAB_LIVE_WRITE=1"]
fn gitlab_live_feedback_replies_close_in_reply_order() {
    require_opt_in("HOTSHEET_GITLAB_LIVE_WRITE");
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
    reply_order_round_trip(Arc::new(GitLabProvider::live(config)), "gitlab", "test");
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
