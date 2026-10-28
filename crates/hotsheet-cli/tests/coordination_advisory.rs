//! Real-process advisory reviews of synthetic frozen handoffs (HS2-E20S6K).
#[allow(dead_code)] // HS2-E20S6K: shared process fixture includes unrelated waiting helpers.
#[path = "support/coordination.rs"]
mod support;

use std::fs;
use std::path::PathBuf;
use std::process::Output;

use hotsheet_cli::coordination::Phase;
use hotsheet_model::Ticket;
use serde_json::{Value, json};
use support::{Fixture, git};

fn setup(code: bool) -> (Fixture, Ticket, Option<String>) {
    let fixture = Fixture::new();
    let ticket = fixture.ticket();
    for (role, id, session) in [
        ("integrator", "integrator", "root"),
        ("worker", "worker", "implementation"),
    ] {
        fixture.run(&["register", "--role", role, "--id", id, "--session", session]);
    }
    let base = git(&fixture.project, &["rev-parse", "HEAD"]);
    fixture.run(&[
        "assign",
        "--id",
        "integrator",
        "--session",
        "root",
        "--worker",
        "worker",
        "--worker-session",
        "implementation",
        "--operation",
        "review-op",
        "--connection",
        &hotsheet_ticketing::git_connection_id(&fixture.store),
        "--native-id",
        &ticket.id.to_string(),
        "--slug",
        &ticket.slug,
        "--base",
        &base,
    ]);
    fixture.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
    fixture.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "implementation",
        "--assignment",
        "review-op",
        "--phase",
        "accepted",
    ]);
    let commit = if code {
        let checkout = fixture.root.path().join("review-source");
        git(
            &fixture.project,
            &[
                "worktree",
                "add",
                "-qb",
                "review-source",
                checkout.to_str().unwrap(),
                &base,
            ],
        );
        fs::write(checkout.join("synthetic.txt"), "synthetic reviewed code\n").unwrap();
        git(&checkout, &["add", "synthetic.txt"]);
        git(
            &checkout,
            &["commit", "-qm", "HS-SYNTHETIC Prepare review source"],
        );
        let commit = git(&checkout, &["rev-parse", "HEAD"]);
        fixture.cli(&[
            "edit",
            &ticket.slug,
            "--note",
            &format!("## Integration handoff\nCommit: {commit}"),
        ]);
        fixture.cli(&["release", &ticket.slug, "--worker", "worker"]);
        fixture.run(&[
            "worker",
            "--id",
            "worker",
            "--session",
            "implementation",
            "--assignment",
            "review-op",
            "--phase",
            "handoff",
            "--commit",
            &commit,
        ]);
        Some(commit)
    } else {
        fixture.cli(&[
            "edit",
            &ticket.slug,
            "--note",
            "## Findings handoff\nSynthetic scoped finding.",
        ]);
        let note = fixture
            .store
            .read_ticket(&ticket.id)
            .unwrap()
            .notes
            .last()
            .unwrap()
            .id
            .to_string();
        fixture.cli(&["release", &ticket.slug, "--worker", "worker"]);
        fixture.run(&[
            "worker",
            "--id",
            "worker",
            "--session",
            "implementation",
            "--assignment",
            "review-op",
            "--phase",
            "handoff",
            "--findings-note",
            &note,
        ]);
        None
    };
    for (reviewer, session) in [
        ("reviewer-one", "review-one"),
        ("reviewer-two", "review-two"),
    ] {
        request(&fixture, reviewer, session);
    }
    (fixture, ticket, commit)
}

fn request(fixture: &Fixture, reviewer: &str, session: &str) -> Value {
    let args = fixture.coord(&[
        "advisory-request",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "review-op",
        "--reviewer",
        reviewer,
        "--reviewer-session",
        session,
        "--return-route",
        "human",
    ]);
    ok(fixture
        .command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap())
}

fn cancel(fixture: &Fixture, note: &str) -> Value {
    let args = fixture.coord(&[
        "advisory-cancel",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "review-op",
        "--request-note",
        note,
    ]);
    ok(fixture
        .command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap())
}

fn call(
    fixture: &Fixture,
    reviewer: &str,
    session: &str,
    path: Option<&PathBuf>,
    publish: bool,
) -> Output {
    let mut args = fixture.coord(&[
        "advisory-review",
        "--id",
        reviewer,
        "--session",
        session,
        "--assignment",
        "review-op",
    ]);
    if let Some(path) = path {
        args.push("--evidence-file".into());
        args.push(path.display().to_string());
    }
    if publish {
        args.push("--publish".into());
    }
    fixture
        .command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
}

fn ok(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn fails(output: Output, message: &str) {
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn inspect(fixture: &Fixture, note: &str) -> Output {
    let args = fixture.coord(&[
        "advisory-inspect",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "review-op",
        "--note",
        note,
    ]);
    fixture
        .command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
}

fn evidence(fixture: &Fixture, snapshot: &Value, verdict: &str) -> (Value, PathBuf) {
    let value = json!({
        "ticket": snapshot["ticket"],
        "base": fixture.run(&["status"]).state.assignments["review-op"].base,
        "handoff": snapshot["handoff"],
        "handoff_note": snapshot["handoff_note"],
        "request_note": snapshot["request_note"],
        "ticket_digest": snapshot["ticket_digest"],
        "review_main": snapshot["review_main"],
        "tree": snapshot["tree"],
        "verdict": verdict,
        "summary": "Synthetic scoped advisory conclusion.",
        "findings": if verdict == "ready" { json!([]) } else { json!([{
            "severity": "blocking", "evidence_ref": "synthetic.txt:1",
            "action": "Correct the synthetic behavior"
        }]) },
    });
    let path = fixture.root.path().join(format!(
        "{verdict}-{}-review.json",
        snapshot["request_note"].as_str().unwrap()
    ));
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    (value, path)
}

#[test]
fn hs2_e20s6k_two_reviewers_publish_without_claiming_or_entering_main_fence() {
    let (fixture, ticket, commit) = setup(true);
    let first = ok(call(&fixture, "reviewer-one", "review-one", None, false));
    assert_eq!(first["handoff"]["commit"], commit.unwrap());
    let (mut first_evidence, first_path) = evidence(&fixture, &first, "ready");
    let mut missing_tree = first_evidence.clone();
    missing_tree["tree"] = Value::Null;
    let missing_path = fixture.root.path().join("missing-tree-review.json");
    fs::write(&missing_path, serde_json::to_vec(&missing_tree).unwrap()).unwrap();
    fails(
        call(
            &fixture,
            "reviewer-one",
            "review-one",
            Some(&missing_path),
            true,
        ),
        "immutable code tree changed",
    );
    let mut wrong_note = first_evidence.clone();
    wrong_note["handoff_note"] = json!("01M00000000000000000000000");
    let wrong_path = fixture.root.path().join("wrong-handoff-note.json");
    fs::write(&wrong_path, serde_json::to_vec(&wrong_note).unwrap()).unwrap();
    fails(
        call(
            &fixture,
            "reviewer-one",
            "review-one",
            Some(&wrong_path),
            true,
        ),
        "qualified handoff or ticket instructions changed",
    );
    assert_eq!(
        ok(call(
            &fixture,
            "reviewer-one",
            "review-one",
            Some(&first_path),
            false
        ))["published_note"],
        Value::Null
    );
    let first_note = ok(call(
        &fixture,
        "reviewer-one",
        "review-one",
        Some(&first_path),
        true,
    ))["published_note"]
        .as_str()
        .unwrap()
        .to_owned();
    let note_count = fixture.store.read_ticket(&ticket.id).unwrap().notes.len();
    assert_eq!(
        ok(call(
            &fixture,
            "reviewer-one",
            "review-one",
            Some(&first_path),
            true
        ))["published_note"],
        first_note
    );
    assert_eq!(
        fixture.store.read_ticket(&ticket.id).unwrap().notes.len(),
        note_count
    );

    let second = ok(call(&fixture, "reviewer-two", "review-two", None, false));
    assert_eq!(second["ticket_digest"], first["ticket_digest"]);
    let (_, second_path) = evidence(&fixture, &second, "changes_requested");
    let second_note = ok(call(
        &fixture,
        "reviewer-two",
        "review-two",
        Some(&second_path),
        true,
    ))["published_note"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(first_note, second_note);
    assert_eq!(ok(inspect(&fixture, &first_note))["current"], true);
    assert_eq!(
        ok(inspect(&fixture, &second_note))["verdict"],
        "changes_requested"
    );
    let after = fixture.store.read_ticket(&ticket.id).unwrap();
    assert_eq!(after.notes.len(), note_count + 1);
    assert!(after.claimed_by.is_none());
    assert_eq!(
        fixture.run(&["status"]).state.assignments["review-op"].phase,
        Phase::Handoff
    );
    assert_eq!(
        git(&fixture.project, &["rev-parse", "main"]),
        first["review_main"]
    );

    first_evidence["summary"] = json!("Different replay should create a separate result.");
    fs::write(&first_path, serde_json::to_vec(&first_evidence).unwrap()).unwrap();
    fails(
        call(
            &fixture,
            "reviewer-one",
            "review-one",
            Some(&first_path),
            true,
        ),
        "already published a different result",
    );
    fixture.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "review-op",
        "--phase",
        "integrating",
    ]);
    fails(inspect(&fixture, &first_note), "before serial integration");
}

#[test]
fn hs2_e20s6k_findings_review_needs_no_code_tree_and_rejects_new_instruction() {
    let (fixture, ticket, _) = setup(false);
    let snapshot = ok(call(&fixture, "reviewer-one", "review-one", None, false));
    assert_eq!(snapshot["tree"], Value::Null);
    let (_, path) = evidence(&fixture, &snapshot, "ready");
    fixture.cli(&[
        "edit",
        &ticket.slug,
        "--note",
        "Synthetic later instruction.",
    ]);
    fails(
        call(&fixture, "reviewer-one", "review-one", Some(&path), true),
        "ticket instructions changed",
    );
    let fresh = ok(call(&fixture, "reviewer-one", "review-one", None, false));
    let (_, path) = evidence(&fixture, &fresh, "ready");
    let note = ok(call(
        &fixture,
        "reviewer-one",
        "review-one",
        Some(&path),
        true,
    ))["published_note"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(ok(inspect(&fixture, &note))["current"], true);
    fixture.cli(&["edit", &ticket.slug, "--note", "Another later instruction."]);
    fails(
        inspect(&fixture, &note),
        "ticket instructions or evidence changed",
    );
}

#[test]
fn hs2_e20s6k_concurrent_publication_and_reviewer_restart_replay() {
    let (fixture, ticket, _) = setup(false);
    let first = ok(call(&fixture, "reviewer-one", "review-one", None, false));
    let second = ok(call(&fixture, "reviewer-two", "review-two", None, false));
    let (_, first_path) = evidence(&fixture, &first, "ready");
    let (_, second_path) = evidence(&fixture, &second, "changes_requested");
    let (one, two) = std::thread::scope(|threads| {
        let one = threads.spawn(|| {
            call(
                &fixture,
                "reviewer-one",
                "review-one",
                Some(&first_path),
                true,
            )
        });
        let two = threads.spawn(|| {
            call(
                &fixture,
                "reviewer-two",
                "review-two",
                Some(&second_path),
                true,
            )
        });
        (ok(one.join().unwrap()), ok(two.join().unwrap()))
    });
    assert_ne!(one["published_note"], two["published_note"]);
    assert_eq!(
        ok(inspect(&fixture, one["published_note"].as_str().unwrap()))["current"],
        true
    );
    assert_eq!(
        ok(inspect(&fixture, two["published_note"].as_str().unwrap()))["current"],
        true
    );
    let count = fixture.store.read_ticket(&ticket.id).unwrap().notes.len();
    assert_eq!(
        ok(call(
            &fixture,
            "reviewer-one",
            "review-one",
            Some(&first_path),
            true
        ))["published_note"],
        one["published_note"]
    );
    fails(
        call(
            &fixture,
            "reviewer-one",
            "review-one-restarted",
            None,
            false,
        ),
        "exact integrator-addressed advisory request",
    );
    request(&fixture, "reviewer-one", "review-one-restarted");
    let restarted = ok(call(
        &fixture,
        "reviewer-one",
        "review-one-restarted",
        None,
        false,
    ));
    assert_ne!(restarted["request_note"], first["request_note"]);
    let (_, restarted_path) = evidence(&fixture, &restarted, "ready");
    fails(
        call(
            &fixture,
            "reviewer-one",
            "review-one-restarted",
            Some(&first_path),
            true,
        ),
        "qualified handoff or ticket instructions changed",
    );
    fails(
        call(
            &fixture,
            "reviewer-one",
            "review-one-restarted",
            Some(&restarted_path),
            true,
        ),
        "already published a different result",
    );
    assert_eq!(
        fixture.store.read_ticket(&ticket.id).unwrap().notes.len(),
        count + 1
    );
}

#[test]
fn hs2_e20s6k_main_advancement_and_review_return_invalidate_results() {
    let (fixture, ticket, _) = setup(true);
    let snapshot = ok(call(&fixture, "reviewer-one", "review-one", None, false));
    let (_, path) = evidence(&fixture, &snapshot, "ready");
    fs::write(fixture.project.join("synthetic.txt"), "new main context\n").unwrap();
    git(&fixture.project, &["add", "synthetic.txt"]);
    git(
        &fixture.project,
        &["commit", "-qm", "HS-SYNTHETIC Advance main"],
    );
    fails(
        call(&fixture, "reviewer-one", "review-one", Some(&path), true),
        "unscoped review needs a refresh",
    );
    let fresh = ok(call(&fixture, "reviewer-one", "review-one", None, false));
    let (_, path) = evidence(&fixture, &fresh, "ready");
    let note = ok(call(
        &fixture,
        "reviewer-one",
        "review-one",
        Some(&path),
        true,
    ))["published_note"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(ok(inspect(&fixture, &note))["current"], true);
    let commit = snapshot["handoff"]["commit"].as_str().unwrap();
    fixture.cli(&[
        "edit",
        &ticket.slug,
        "--note",
        &format!("## Review return\nreview-op {commit}\nCorrect synthetic code."),
    ]);
    let return_id = fixture
        .store
        .read_ticket(&ticket.id)
        .unwrap()
        .notes
        .last()
        .unwrap()
        .id
        .to_string();
    fixture.run(&[
        "review-return",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "review-op",
        "--evidence-note",
        &return_id,
        "--inspected",
    ]);
    fails(inspect(&fixture, &note), "before serial integration");
}

#[test]
fn hs2_e20s6k_bounded_requests_cancel_lost_cue_and_reject_stale_result() {
    let (fixture, _, _) = setup(false);
    fixture.run(&[
        "register",
        "--role",
        "worker",
        "--id",
        "reviewer-in-pool",
        "--session",
        "implementation-eligible",
    ]);
    let pooled = fixture.coord(&[
        "advisory-request",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "review-op",
        "--reviewer",
        "reviewer-in-pool",
        "--reviewer-session",
        "implementation-eligible",
        "--return-route",
        "human",
    ]);
    fails(
        fixture
            .command(&pooled.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap(),
        "identity must be separate from pool roles",
    );
    let first = ok(call(&fixture, "reviewer-one", "review-one", None, false));
    let (_, path) = evidence(&fixture, &first, "ready");
    let args = fixture.coord(&[
        "advisory-request",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "review-op",
        "--reviewer",
        "reviewer-three",
        "--reviewer-session",
        "review-three",
        "--return-route",
        "human",
    ]);
    fails(
        fixture
            .command(&args.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap(),
        "two advisory reviews are already outstanding",
    );
    let cancelled = cancel(&fixture, first["request_note"].as_str().unwrap());
    assert_eq!(
        cancelled["cancelled_note"],
        cancel(&fixture, first["request_note"].as_str().unwrap())["cancelled_note"]
    );
    fails(
        call(&fixture, "reviewer-one", "review-one", Some(&path), true),
        "exact integrator-addressed advisory request",
    );
    let third = request(&fixture, "reviewer-three", "review-three");
    assert_eq!(third["delivery"], "explicit_send_required");
    let snapshot = ok(call(
        &fixture,
        "reviewer-three",
        "review-three",
        None,
        false,
    ));
    assert_eq!(snapshot["request_note"], third["request_note"]);
}
