//! HS2-AEBJYN: actual claim/release and findings correction loops, with prior code CI retained.
//! HS2-BAK4BC: explicit claims take blocked tickets and print the reason to re-evaluate,
//! claim-next still skips them, and a findings handoff on a blocked ticket is verified.
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

// ---- HS2-BAK4BC: blocked tickets are explicitly claimable --------------------------

const REASON: &str = "Waiting on a synthetic dependency";

fn blocked_coord_output(f: &Fixture, args: &[&str]) -> std::process::Output {
    let args = f.coord(args);
    f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
}

fn mark_output(f: &Fixture, assignment: &str, phase: &str) -> std::process::Output {
    blocked_coord_output(
        f,
        &[
            "mark",
            "--id",
            "integrator",
            "--session",
            "root",
            "--assignment",
            assignment,
            "--phase",
            phase,
        ],
    )
}

fn reason(f: &Fixture, ticket: &Ticket) -> Option<String> {
    f.store.read_ticket(&ticket.id).unwrap().blocked_reason
}

#[test]
fn hs2_bak4bc_cli_claim_and_renew_print_the_blocked_reason_and_claim_next_skips_it() {
    let f = Fixture::new();
    let ticket = f.ticket();
    f.cli(&["edit", &ticket.slug, "--blocked-reason", REASON]);

    assert_eq!(
        f.cli(&["claim-next", "--worker", "auto"]).trim(),
        "No claimable tickets.",
        "automatic selection skips the blocked ticket"
    );
    let claimed = f.cli(&["claim", &ticket.slug, "--worker", "person"]);
    assert!(claimed.contains(&format!("Claimed {} for person", ticket.slug)));
    assert!(
        claimed.contains(&format!("Blocked: {REASON}.")),
        "the claim names the reason to re-evaluate: {claimed}"
    );
    assert!(claimed.contains(&format!("edit {} --clear-blocked-reason", ticket.slug)));
    let renewed = f.cli(&["renew", &ticket.slug, "--worker", "person"]);
    assert!(
        renewed.contains(&format!("Blocked: {REASON}.")),
        "{renewed}"
    );
    assert_eq!(reason(&f, &ticket).as_deref(), Some(REASON));

    // The holder re-evaluates and clears the reason; the claim and its renewals continue.
    f.cli(&["edit", &ticket.slug, "--clear-blocked-reason"]);
    let renewed = f.cli(&["renew", &ticket.slug, "--worker", "person"]);
    assert!(!renewed.contains("Blocked:"), "{renewed}");
    f.cli(&["release", &ticket.slug, "--worker", "person"]);
    let next = f.cli(&["claim-next", "--worker", "auto"]);
    assert!(
        next.contains(&format!("Claimed {} for auto", ticket.slug)),
        "{next}"
    );
}

/// HS2-KBK83M: findings that conclude the ticket is blocked no longer strand the
/// handoff. The integrator claims the blocked ticket and records Verifying; a block set
/// during Verifying keeps the claim renewable and review-return still works.
#[test]
fn hs2_bak4bc_findings_handoff_on_a_blocked_ticket_is_verified_and_finished() {
    for block_during_verifying in [false, true] {
        let f = Fixture::new();
        let ticket = f.ticket();
        for (role, id, session) in [
            ("integrator", "integrator", "root"),
            ("worker", "worker", "worker-session"),
        ] {
            f.run(&["register", "--role", role, "--id", id, "--session", session]);
        }
        let base = git(&f.project, &["rev-parse", "HEAD"]);
        f.assign("findings", &ticket, &base, "root");
        f.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
        worker(&f, "findings", "accepted", &[]);
        let findings = note(
            &f,
            &ticket,
            "## Findings handoff\nSynthetic findings: this waits on a synthetic dependency.".into(),
        );
        if !block_during_verifying {
            f.cli(&["edit", &ticket.slug, "--blocked-reason", REASON]);
        }
        f.cli(&["release", &ticket.slug, "--worker", "worker"]);
        worker(&f, "findings", "handoff", &["--findings-note", &findings]);

        let claimed = f.cli(&[
            "claim",
            &ticket.slug,
            "--worker",
            "integrator",
            "--eta",
            "10m",
        ]);
        assert_eq!(
            claimed.contains("Blocked:"),
            !block_during_verifying,
            "{claimed}"
        );
        let verifying = mark_output(&f, "findings", "verifying");
        assert!(
            verifying.status.success(),
            "verifying a blocked findings ticket: {}",
            String::from_utf8_lossy(&verifying.stderr)
        );
        if block_during_verifying {
            f.cli(&["edit", &ticket.slug, "--blocked-reason", REASON]);
            let renewed = f.cli(&["renew", &ticket.slug, "--worker", "integrator"]);
            assert!(
                renewed.contains(&format!("Blocked: {REASON}.")),
                "{renewed}"
            );
            // The integrator returns the findings for more work instead of finishing.
            f.cli(&["release", &ticket.slug, "--worker", "integrator"]);
            let review = note(
                &f,
                &ticket,
                format!("## Review return\nfindings {findings}\nSynthetic follow-up request."),
            );
            let returned = f.run(&[
                "review-return",
                "--id",
                "integrator",
                "--session",
                "root",
                "--assignment",
                "findings",
                "--evidence-note",
                &review,
                "--inspected",
            ]);
            assert!(returned.state.assignments["findings"].returned_for_review());
            assert_eq!(reason(&f, &ticket).as_deref(), Some(REASON));
            continue;
        }
        // Done still requires a closed ticket (the remaining HS2-KBK83M decision).
        let early = mark_output(&f, "findings", "done");
        assert!(!early.status.success());
        assert!(
            String::from_utf8_lossy(&early.stderr)
                .contains("complete or close the findings ticket")
        );
        f.cli(&[
            "edit",
            &ticket.slug,
            "--status",
            "completed",
            "--note",
            "Verified synthetic findings.",
        ]);
        let done = mark_output(&f, "findings", "done");
        assert!(
            done.status.success(),
            "{}",
            String::from_utf8_lossy(&done.stderr)
        );
        assert_eq!(reason(&f, &ticket).as_deref(), Some(REASON));
    }
}

/// HS2-KBK83M (maintainer decision 2026-11-04: "Require review-return"): coordination
/// `done` stays closed-only for findings, and accepted findings that leave the ticket
/// open or blocked go through review-return, from Handoff or from Verifying.
#[test]
fn hs2_kbk83m_open_blocked_findings_route_through_review_return() {
    for from_verifying in [false, true] {
        let f = Fixture::new();
        let ticket = f.ticket();
        for (role, id, session) in [
            ("integrator", "integrator", "root"),
            ("worker", "worker", "worker-session"),
        ] {
            f.run(&["register", "--role", role, "--id", id, "--session", session]);
        }
        let base = git(&f.project, &["rev-parse", "HEAD"]);
        f.assign("findings", &ticket, &base, "root");
        f.cli(&["claim", &ticket.slug, "--worker", "worker", "--eta", "10m"]);
        worker(&f, "findings", "accepted", &[]);
        let findings = note(
            &f,
            &ticket,
            "## Findings handoff\nSynthetic findings: still waits on a synthetic dependency."
                .into(),
        );
        f.cli(&["edit", &ticket.slug, "--blocked-reason", REASON]);
        f.cli(&["release", &ticket.slug, "--worker", "worker"]);
        worker(&f, "findings", "handoff", &["--findings-note", &findings]);

        if from_verifying {
            f.cli(&[
                "claim",
                &ticket.slug,
                "--worker",
                "integrator",
                "--eta",
                "10m",
            ]);
            let verifying = mark_output(&f, "findings", "verifying");
            assert!(
                verifying.status.success(),
                "{}",
                String::from_utf8_lossy(&verifying.stderr)
            );
            // Done on the open, blocked ticket is refused and names the supported route.
            let done = mark_output(&f, "findings", "done");
            assert!(!done.status.success());
            let stderr = String::from_utf8_lossy(&done.stderr);
            assert!(
                stderr.contains("complete or close the findings ticket"),
                "{stderr}"
            );
            assert!(stderr.contains("review-return"), "{stderr}");
            // Returning while the integrator still holds the claim is refused.
            let early_review = note(
                &f,
                &ticket,
                format!("## Review return\nfindings {findings}\nAccepted; ticket stays blocked."),
            );
            let held = blocked_coord_output(&f, &return_args("findings", &early_review));
            assert!(!held.status.success(), "a held claim blocks the return");
            f.cli(&["release", &ticket.slug, "--worker", "integrator"]);
        }

        let review = note(
            &f,
            &ticket,
            format!("## Review return\nfindings {findings}\nAccepted; ticket stays blocked."),
        );
        let returned = f.run(&return_args("findings", &review));
        assert!(returned.state.assignments["findings"].returned_for_review());
        // A replay with the same evidence is idempotent; different evidence is refused.
        let replay = f.run(&return_args("findings", &review));
        assert!(replay.state.assignments["findings"].returned_for_review());
        let other = note(
            &f,
            &ticket,
            format!("## Review return\nfindings {findings}\nA different return."),
        );
        let conflicting = blocked_coord_output(&f, &return_args("findings", &other));
        assert!(!conflicting.status.success());
        // The ticket keeps its status and its blocked reason.
        let stored = f.store.read_ticket(&ticket.id).unwrap();
        assert!(ops::is_open(&stored));
        assert_eq!(stored.blocked_reason.as_deref(), Some(REASON));
    }
}
