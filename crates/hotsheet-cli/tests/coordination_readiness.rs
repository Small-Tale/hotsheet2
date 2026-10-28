//! Real CLI preflight, publication, and replay over synthetic tickets (HS2-NNSWDS).

#[path = "support/coordination.rs"]
mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use hotsheet_cli::coordination::Phase;
use hotsheet_model::{ClaimEventKind, NoteKind, Ticket, Timestamp, Ulid};
use hotsheet_ticketing::{git_connection_id, ops};
use serde_json::{Value, json};
use support::{Fixture, git};

fn setup() -> (Fixture, Ticket, String) {
    let fixture = Fixture::new();
    let ticket = fixture.ticket();
    fixture.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "root",
    ]);
    fixture.run(&[
        "register",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    let base = git(&fixture.project, &["rev-parse", "HEAD"]);
    fixture.assign("readiness-op", &ticket, &base, "root");
    fixture.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
    fixture.run(&[
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "readiness-op",
        "--phase",
        "accepted",
    ]);
    fixture.cli(&["edit", &ticket.slug, "--started-phase", "initial_testing"]);
    (fixture, ticket, base)
}

fn evidence_base(fixture: &Fixture, ticket: &Ticket) -> Value {
    let ticket = fixture.store.read_ticket(&ticket.id).unwrap();
    let claim_attempt = ticket
        .claim_history
        .iter()
        .rev()
        .find(|event| event.kind == ClaimEventKind::Claim)
        .unwrap();
    json!({
        "ticket": {"connection_id": git_connection_id(&fixture.store), "native_id": ticket.id.to_string()},
        "claim_attempt": claim_attempt.id.to_string(),
        "ticket_updated_at": ticket.updated_at.as_str(),
        "latest_note": ticket.notes.last().map(|note| note.id.to_string()),
        "scope": [],
        "instructions_reviewed": true,
        "docs_reviewed": true,
        "coverage_reviewed": true,
        "unresolved_findings": [],
        "ui_affecting": false,
    })
}

fn write_evidence(fixture: &Fixture, value: &Value) -> PathBuf {
    let path = fixture.root.path().join("readiness.json");
    fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

fn call(fixture: &Fixture, path: &Path, publish: bool) -> Output {
    let mut args = fixture.coord(&[
        "preflight",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        "readiness-op",
        "--evidence-file",
        path.to_str().unwrap(),
    ]);
    if publish {
        args.push("--publish".into());
    }
    fixture
        .command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
}

fn success(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn failure(output: &Output, reason: &str) {
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(reason),
        "expected {reason}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn code_evidence(fixture: &Fixture, ticket: &Ticket, base: &str, path: &str) -> (Value, PathBuf) {
    let checkout = fixture.root.path().join("worker-code");
    git(
        &fixture.project,
        &[
            "worktree",
            "add",
            "-qb",
            "worker-code",
            checkout.to_str().unwrap(),
            base,
        ],
    );
    let changed = checkout.join(path);
    fs::create_dir_all(changed.parent().unwrap()).unwrap();
    fs::write(&changed, "synthetic reviewed change\n").unwrap();
    git(&checkout, &["add", path]);
    git(
        &checkout,
        &["commit", "-qm", "HS-SYNTHETIC Review synthetic change"],
    );
    let commit = git(&checkout, &["rev-parse", "HEAD"]);
    let tree = git(&checkout, &["rev-parse", "HEAD^{tree}"]);
    let marker = git(
        &checkout,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            &format!("hotsheet-root-gate/{tree}"),
        ],
    );
    fs::create_dir_all(Path::new(&marker).parent().unwrap()).unwrap();
    fs::write(marker, format!("{tree}\n")).unwrap();
    let mut evidence = evidence_base(fixture, ticket);
    evidence["kind"] = json!("code");
    evidence["worktree"] = json!(checkout);
    evidence["branch"] = json!("worker-code");
    evidence["commit"] = json!(commit);
    evidence["tree"] = json!(tree);
    evidence["risk_tier"] = json!("t1");
    (evidence, checkout)
}

#[test]
fn hs2_nnswds_findings_preflight_publishes_once_without_commit_or_ci() {
    let (fixture, ticket, _) = setup();
    let mut evidence = evidence_base(&fixture, &ticket);
    evidence["kind"] = json!("findings");
    evidence["note"] = json!("## Findings handoff\nSynthetic verified observation.");
    let path = write_evidence(&fixture, &evidence);

    let preflight = success(&call(&fixture, &path, false));
    assert_eq!(preflight["published"], false);
    assert_eq!(preflight["outcome"], "findings");
    let published = success(&call(&fixture, &path, true));
    assert_eq!(published["published"], true);
    let after = fixture.store.read_ticket(&ticket.id).unwrap();
    assert!(after.claimed_by.is_none());
    assert!(
        after
            .notes
            .last()
            .unwrap()
            .text
            .starts_with("## Findings handoff\n")
    );
    assert_eq!(
        fixture.run(&["status"]).state.assignments["readiness-op"].phase,
        Phase::Handoff
    );
    let note_count = after.notes.len();
    let claim_count = after.claim_history.len();
    assert_eq!(success(&call(&fixture, &path, true))["published"], true);
    let replayed = fixture.store.read_ticket(&ticket.id).unwrap();
    assert_eq!(replayed.notes.len(), note_count);
    assert_eq!(replayed.claim_history.len(), claim_count);

    let mut waiting = fixture.waiting(&[
        "next",
        "--role",
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
    ]);
    let next = fixture.ticket();
    let base = git(&fixture.project, &["rev-parse", "HEAD"]);
    fixture.assign("readiness-next", &next, &base, "root");
    assert_eq!(
        waiting
            .result()
            .state
            .active_assignment("worker")
            .unwrap()
            .id,
        "readiness-next"
    );
}

#[test]
fn hs2_nnswds_code_preflight_checks_tree_and_rejects_stale_notes_then_repairs() {
    let (fixture, ticket, base) = setup();
    let (mut evidence, checkout) = code_evidence(&fixture, &ticket, &base, "synthetic.txt");
    let path = write_evidence(&fixture, &evidence);
    assert_eq!(success(&call(&fixture, &path, false))["outcome"], "code");

    evidence["scope"] = json!(["unassigned/path"]);
    write_evidence(&fixture, &evidence);
    failure(
        &call(&fixture, &path, false),
        "exact requested path or section scope",
    );
    evidence["scope"] = json!([]);
    write_evidence(&fixture, &evidence);

    fixture.cli(&["edit", &ticket.slug, "--note", "Synthetic new instruction."]);
    failure(&call(&fixture, &path, false), "stale");
    let fresh = evidence_base(&fixture, &ticket);
    evidence["ticket_updated_at"] = fresh["ticket_updated_at"].clone();
    evidence["latest_note"] = fresh["latest_note"].clone();
    write_evidence(&fixture, &evidence);
    assert_eq!(success(&call(&fixture, &path, false))["outcome"], "code");

    fs::write(checkout.join("synthetic.txt"), "changed after gate\n").unwrap();
    failure(
        &call(&fixture, &path, false),
        "commit or preserve every local edit",
    );
    git(&checkout, &["add", "synthetic.txt"]);
    git(
        &checkout,
        &["commit", "-qm", "HS-SYNTHETIC Change after gate"],
    );
    failure(
        &call(&fixture, &path, false),
        "commit, tree, or branch changed",
    );
}

#[test]
fn hs2_nnswds_ui_evidence_is_required_for_rendered_code() {
    let (fixture, ticket, base) = setup();
    let (mut evidence, _) =
        code_evidence(&fixture, &ticket, &base, "clients/web/src/synthetic.tsx");
    let path = write_evidence(&fixture, &evidence);
    failure(&call(&fixture, &path, false), "inspect the real parent UI");
    evidence["ui"] = json!({
        "real_parent": "synthetic project screen",
        "states": ["open"],
        "viewports": ["wide", "medium", "narrow", "resize"],
        "inspected_rendered_page": true,
        "after_attachment": null
    });
    write_evidence(&fixture, &evidence);
    assert_eq!(success(&call(&fixture, &path, false))["outcome"], "code");
    assert_eq!(success(&call(&fixture, &path, true))["published"], true);
    let published = fixture.store.read_ticket(&ticket.id).unwrap();
    assert!(published.claimed_by.is_none());
    assert!(
        published
            .notes
            .last()
            .unwrap()
            .text
            .contains("UI review (self-attested): real parent synthetic project screen")
    );
    assert_eq!(
        fixture.run(&["status"]).state.assignments["readiness-op"].phase,
        Phase::Handoff
    );
    assert_eq!(success(&call(&fixture, &path, true))["published"], true);
}

#[test]
fn hs2_nnswds_missing_exact_tree_gate_repairs_without_broad_rerun() {
    let (fixture, ticket, base) = setup();
    let (evidence, checkout) = code_evidence(&fixture, &ticket, &base, "synthetic.txt");
    let tree = evidence["tree"].as_str().unwrap();
    let marker = git(
        &checkout,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            &format!("hotsheet-root-gate/{tree}"),
        ],
    );
    fs::remove_file(&marker).unwrap();
    let path = write_evidence(&fixture, &evidence);
    failure(&call(&fixture, &path, false), "no gate receipt matches");
    fs::write(marker, format!("{tree}\n")).unwrap();
    assert_eq!(success(&call(&fixture, &path, false))["outcome"], "code");
}

#[test]
fn hs2_nnswds_rejects_lost_claim_authority_before_publication() {
    let (fixture, ticket, _) = setup();
    let mut evidence = evidence_base(&fixture, &ticket);
    evidence["kind"] = json!("findings");
    evidence["note"] = json!("## Findings handoff\nSynthetic observation.");
    let path = write_evidence(&fixture, &evidence);
    fixture.cli(&["release", &ticket.slug, "--worker", "worker"]);
    failure(
        &call(&fixture, &path, true),
        "renew this exact ticket claim",
    );
    assert!(
        fixture
            .store
            .read_ticket(&ticket.id)
            .unwrap()
            .notes
            .iter()
            .all(|note| !note.text.contains("Readiness evidence SHA-256"))
    );
}

#[test]
fn hs2_nnswds_ui_findings_require_real_parent_review_without_code_gate() {
    let (fixture, ticket, _) = setup();
    let mut evidence = evidence_base(&fixture, &ticket);
    evidence["kind"] = json!("findings");
    evidence["note"] = json!("## Findings handoff\nSynthetic visual finding.");
    evidence["ui_affecting"] = json!(true);
    let path = write_evidence(&fixture, &evidence);
    failure(&call(&fixture, &path, false), "inspect the real parent UI");
    evidence["ui"] = json!({
        "real_parent": "synthetic ticket reader",
        "states": ["open"],
        "viewports": ["wide", "medium", "narrow", "resize"],
        "inspected_rendered_page": true,
        "after_attachment": null
    });
    write_evidence(&fixture, &evidence);
    assert_eq!(
        success(&call(&fixture, &path, false))["outcome"],
        "findings"
    );
}

#[test]
fn hs2_nnswds_published_findings_remain_returnable_for_correction() {
    let (fixture, ticket, _) = setup();
    let mut evidence = evidence_base(&fixture, &ticket);
    evidence["kind"] = json!("findings");
    evidence["note"] = json!("## Findings handoff\nSynthetic observation to review.");
    let path = write_evidence(&fixture, &evidence);
    success(&call(&fixture, &path, true));
    let handoff = fixture
        .store
        .read_ticket(&ticket.id)
        .unwrap()
        .notes
        .last()
        .unwrap()
        .id;
    let review = Ulid::new();
    ops::add_note(
        &fixture.store,
        &ticket.id,
        review,
        Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
        NoteKind::Regular,
        format!("## Review return\nreadiness-op {handoff}\nCorrect the synthetic finding."),
    )
    .unwrap();
    let returned = fixture.run(&[
        "review-return",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "readiness-op",
        "--evidence-note",
        &review.to_string(),
        "--inspected",
    ]);
    assert_eq!(
        returned.state.assignments["readiness-op"].phase,
        Phase::Blocked
    );
    failure(&call(&fixture, &path, true), "stale");
}

#[test]
fn hs2_nnswds_published_code_remains_returnable_for_correction() {
    let (fixture, ticket, base) = setup();
    let (evidence, checkout) = code_evidence(&fixture, &ticket, &base, "synthetic.txt");
    let commit = evidence["commit"].as_str().unwrap().to_owned();
    let path = write_evidence(&fixture, &evidence);
    success(&call(&fixture, &path, true));
    let review = Ulid::new();
    ops::add_note(
        &fixture.store,
        &ticket.id,
        review,
        Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
        NoteKind::Regular,
        format!("## Review return\nreadiness-op {commit}\nPreserve the original tree and correct synthetic behavior."),
    )
    .unwrap();
    let returned = fixture.run(&[
        "review-return",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        "readiness-op",
        "--evidence-note",
        &review.to_string(),
        "--inspected",
    ]);
    assert_eq!(
        returned.state.assignments["readiness-op"].phase,
        Phase::Blocked
    );
    assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), commit);
}
