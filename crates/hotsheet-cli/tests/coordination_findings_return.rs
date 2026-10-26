//! HS2-AEBJYN: actual claim/release and findings correction loops, with prior code CI retained.
#[path = "support/coordination.rs"]
mod support;

use hotsheet_cli::coordination::{HandoffEvidence, Phase};
use hotsheet_model::{NoteKind, Ticket, Timestamp, Ulid};
use hotsheet_ticketing::ops;
use support::{Fixture, git};

fn note(f: &Fixture, ticket: &Ticket, text: String) -> String {
    let id = Ulid::new();
    ops::add_note(
        &f.store,
        &ticket.id,
        id,
        Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
        NoteKind::Regular,
        text,
    )
    .unwrap();
    id.to_string()
}
fn worker(f: &Fixture, assignment: &str, phase: &str, evidence: &[&str]) {
    let mut args = vec![
        "worker",
        "--id",
        "worker",
        "--session",
        "worker-session",
        "--assignment",
        assignment,
        "--phase",
        phase,
    ];
    args.extend_from_slice(evidence);
    f.run(&args);
}
fn mark(f: &Fixture, assignment: &str, phase: &str) {
    f.run(&[
        "mark",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        assignment,
        "--phase",
        phase,
    ]);
}
fn return_args<'a>(assignment: &'a str, review: &'a str) -> Vec<&'a str> {
    vec![
        "review-return",
        "--id",
        "integrator",
        "--session",
        "root",
        "--assignment",
        assignment,
        "--evidence-note",
        review,
        "--inspected",
    ]
}

#[test]
fn hs2_aebjyn_real_findings_correction_loop_preserves_original_and_pending_code_ci() {
    for from_verifying in [false, true] {
        let f = Fixture::new();
        let ticket = f.ticket();
        let code = f.ticket();
        for (role, id, session) in [
            ("integrator", "integrator", "root"),
            ("worker", "worker", "worker-session"),
        ] {
            f.run(&["register", "--role", role, "--id", id, "--session", session]);
        }
        let base = git(&f.project, &["rev-parse", "HEAD"]);
        f.assign("code-ci", &code, &base, "root");
        f.cli(&["claim", &code.slug, "--worker", "worker", "--eta", "10m"]);
        worker(&f, "code-ci", "accepted", &[]);
        note(
            &f,
            &code,
            format!("## Integration handoff\nVerified synthetic existing commit {base}"),
        );
        f.cli(&["release", &code.slug, "--worker", "worker"]);
        worker(&f, "code-ci", "handoff", &["--commit", &base]);
        mark(&f, "code-ci", "integrating");
        mark(&f, "code-ci", "ci");

        f.assign("original", &ticket, &base, "root");
        f.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
        worker(&f, "original", "accepted", &[]);
        let original_note = note(
            &f,
            &ticket,
            "## Findings handoff\nSynthetic measured findings need a clarification.".into(),
        );
        f.cli(&["release", &ticket.slug, "--worker", "worker"]);
        worker(
            &f,
            "original",
            "handoff",
            &["--findings-note", &original_note],
        );
        if from_verifying {
            f.cli(&[
                "claim",
                &ticket.slug,
                "--worker",
                "integrator",
                "--eta",
                "10m",
            ]);
            mark(&f, "original", "verifying");
        }
        let review = note(
            &f,
            &ticket,
            format!(
                "## Review return\noriginal {original_note}\nPreserve the original findings and clarify the synthetic measurement."
            ),
        );
        let args = return_args("original", &review);
        if from_verifying {
            let command = f.coord(&args);
            let refused = f
                .command(&command.iter().map(String::as_str).collect::<Vec<_>>())
                .output()
                .unwrap();
            assert!(
                !refused.status.success(),
                "a live verifying claim must prevent return"
            );
            f.cli(&["release", &ticket.slug, "--worker", "integrator"]);
        }
        let returned = f.run(&args).state.assignments["original"].clone();
        assert!(returned.returned_for_review());
        assert_eq!(returned.commit, None);
        assert_eq!(
            returned.handoff,
            Some(HandoffEvidence::Findings {
                note_id: original_note.clone()
            })
        );
        let mut waiting = f.waiting(&[
            "next",
            "--role",
            "worker",
            "--id",
            "worker",
            "--session",
            "worker-session",
        ]);
        f.assign("correction", &ticket, &base, "root");
        assert_eq!(
            waiting
                .result()
                .state
                .active_assignment("worker")
                .unwrap()
                .id,
            "correction"
        );
        f.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
        // A lost return receipt replays after the new operation has a real ticket claim.
        f.run(&args);
        assert_eq!(
            f.store
                .read_ticket(&ticket.id)
                .unwrap()
                .claimed_by
                .as_deref(),
            Some("worker")
        );
        worker(&f, "correction", "accepted", &[]);
        let corrected = note(
            &f,
            &ticket,
            "## Findings handoff\nSynthetic measured findings, clarified and verified.".into(),
        );
        f.cli(&["release", &ticket.slug, "--worker", "worker"]);
        worker(
            &f,
            "correction",
            "handoff",
            &["--findings-note", &corrected],
        );
        f.cli(&[
            "claim",
            &ticket.slug,
            "--worker",
            "integrator",
            "--eta",
            "10m",
        ]);
        mark(&f, "correction", "verifying");
        f.cli(&[
            "edit",
            &ticket.slug,
            "--status",
            "completed",
            "--note",
            "Verified synthetic corrected findings.",
        ]);
        mark(&f, "correction", "done");
        f.cli(&["release", &ticket.slug, "--worker", "integrator"]);
        let old_done = f.coord(&[
            "mark",
            "--id",
            "integrator",
            "--session",
            "root",
            "--assignment",
            "original",
            "--phase",
            "done",
        ]);
        let refused = f
            .command(&old_done.iter().map(String::as_str).collect::<Vec<_>>())
            .output()
            .unwrap();
        assert!(!refused.status.success());
        assert!(
            String::from_utf8_lossy(&refused.stderr)
                .contains("review-returned handoff is preserved")
        );
        let final_state = f.run(&args).state;
        assert_eq!(final_state.version, 3);
        let persisted: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                f.root
                    .path()
                    .join("project/.git/hotsheet-coordination/state.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(persisted["version"], 3);
        assert_eq!(final_state.assignments["original"], returned);
        assert_eq!(final_state.assignments["correction"].phase, Phase::Done);
        assert_eq!(final_state.assignments["code-ci"].phase, Phase::Ci);
        let saved = f.store.read_ticket(&ticket.id).unwrap();
        assert!(
            saved
                .notes
                .iter()
                .any(|n| n.id.to_string() == original_note)
        );
        assert!(saved.notes.iter().any(|n| n.id.to_string() == corrected));
        assert_eq!(
            git(&f.project, &["rev-parse", "HEAD"]),
            base,
            "findings do not require an empty commit"
        );
    }
}
