//! Worker self-review before publishing a durable handoff (HS2-NNSWDS).
//! The input stays machine-local. Only identifiers and evidence classifications reach output.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use hotsheet_cli::coordination::{Coordinator, HandoffEvidence, Phase};
use hotsheet_model::{AttachmentPurpose, ClaimEventKind, StartedPhase, Timestamp, Ulid};
use hotsheet_ticketing::{FsStore, TicketRef, git_connection_id, ops};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{git_output, review_commit_scope};

#[derive(Debug, Deserialize)]
struct Evidence {
    ticket: TicketRef,
    /// The claim event minted when this worker acquired the implementation ticket.
    claim_attempt: String,
    /// Ticket snapshot reviewed after the latest note or instruction.
    ticket_updated_at: String,
    /// The last note id at the reviewed snapshot, or null when there were no notes.
    latest_note: Option<String>,
    /// The exact repository path or section scope in the assignment.
    scope: Vec<String>,
    instructions_reviewed: bool,
    /// Worker judgment; it is reported as self-attested, never verified by this command.
    docs_reviewed: bool,
    coverage_reviewed: bool,
    unresolved_findings: Vec<UnresolvedFinding>,
    /// Worker classification for UI work outside rendered web source paths.
    ui_affecting: bool,
    #[serde(default)]
    ui: Option<UiReview>,
    #[serde(flatten)]
    outcome: Outcome,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnresolvedFinding {
    id: String,
    follow_up: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiReview {
    real_parent: String,
    states: Vec<String>,
    viewports: Vec<String>,
    inspected_rendered_page: bool,
    /// Required when this ticket carries a problem or design image.
    after_attachment: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Outcome {
    Code {
        worktree: PathBuf,
        branch: String,
        commit: String,
        tree: String,
        risk_tier: RiskTier,
    },
    Findings {
        /// Proposed note body, including its heading; this command never publishes it.
        note: String,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RiskTier {
    T0,
    T1,
    T2,
}

#[derive(Debug, Serialize)]
struct Check {
    item: &'static str,
    status: &'static str,
    detail: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct Report {
    assignment: String,
    ticket: TicketRef,
    outcome: &'static str,
    checks: Vec<Check>,
    published: bool,
}

fn check(condition: bool, classification: &str, repair: &str) -> Result<()> {
    ensure!(condition, "{classification}: {repair}");
    Ok(())
}

fn git_text(project: &Path, args: &[&str], repair: &str) -> Result<String> {
    let output = git_output(project, args)?;
    check(output.status.success(), "missing", repair)?;
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn full_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn same_ticket_snapshot(ticket: &hotsheet_model::Ticket, evidence: &Evidence) -> bool {
    ticket.updated_at.as_str() == evidence.ticket_updated_at
        && ticket.notes.last().map(|note| note.id.to_string()) == evidence.latest_note
}

pub(super) fn preflight(
    coordinator: &Coordinator,
    source: &Path,
    project: &Path,
    worker: &str,
    generation: &str,
    assignment: &str,
    evidence_file: &Path,
) -> Result<Report> {
    let bytes =
        fs::read(evidence_file).context("missing: provide --evidence-file with review facts")?;
    let evidence: Evidence = serde_json::from_slice(&bytes)
        .context("unsupported: readiness evidence must match the code or findings JSON contract")?;
    let state = coordinator.state()?;
    let assigned = state
        .assignments
        .get(assignment)
        .context("missing: inspect the exact assignment id in coordination status")?;
    check(
        assigned.worker == worker && assigned.generation == generation,
        "stale",
        "use the assignment's exact worker id and session generation",
    )?;
    check(
        assigned.phase == Phase::Accepted && assigned.handoff.is_none(),
        "stale",
        "preflight requires the accepted assignment before its durable handoff note",
    )?;
    check(
        assigned.ticket == evidence.ticket,
        "stale",
        "review the exact qualified connection and native ticket id from the assignment",
    )?;
    check(
        assigned.scope == evidence.scope,
        "stale",
        "reread the assignment's exact requested path or section scope",
    )?;
    let store = FsStore::open_without_maintenance(source)
        .context("unsupported: selected ticket source has no local claim and note authority")?;
    check(
        evidence.ticket.connection_id == git_connection_id(&store),
        "unsupported",
        "this provider cannot verify local claim and note evidence for preflight",
    )?;
    let native_id = Ulid::from_string(&evidence.ticket.native_id)?;
    let ticket = store.read_ticket(&native_id)?;
    check(
        ticket.slug == assigned.slug,
        "stale",
        "reread the assigned ticket; its slug no longer matches the mailbox",
    )?;
    let now = Timestamp::from_datetime(time::OffsetDateTime::now_utc());
    check(
        ticket.claimed_by.as_deref() == Some(worker) && !ops::claim_available(&ticket, &now),
        "stale",
        "renew this exact ticket claim before preflight; availability is not claim authority",
    )?;
    let claim_attempt = ticket
        .claim_history
        .iter()
        .rev()
        .find(|event| event.kind == ClaimEventKind::Claim);
    check(
        claim_attempt.is_some_and(|event| {
            event.id.to_string() == evidence.claim_attempt && event.worker == worker
        }),
        "stale",
        "use the current exact claim-attempt id; an intervening claim requires a fresh review",
    )?;
    check(
        same_ticket_snapshot(&ticket, &evidence),
        "stale",
        "reread the full ticket and latest notes, then update ticket_updated_at and latest_note",
    )?;
    check(
        ticket.started_phase == Some(StartedPhase::InitialTesting),
        "missing",
        "record the actual initial_testing phase before handoff; report historical gaps without backfilling",
    )?;
    check(
        evidence.instructions_reviewed && evidence.docs_reviewed && evidence.coverage_reviewed,
        "missing",
        "review latest instructions, requirements/docs and feature coverage, then attest each in the evidence file",
    )?;

    let mut checks = vec![
        Check {
            item: "qualified_ticket_and_assignment",
            status: "verified",
            detail: "exact local source",
        },
        Check {
            item: "claim_attempt_and_deadline",
            status: "verified",
            detail: "live exact ticket claim",
        },
        Check {
            item: "latest_ticket_instructions",
            status: "self_attested",
            detail: "ticket snapshot verified; worker reading attested",
        },
        Check {
            item: "requested_scope",
            status: "verified",
            detail: "exact assignment scope",
        },
        Check {
            item: "docs_and_coverage_review",
            status: "self_attested",
            detail: "worker review",
        },
    ];
    for finding in &evidence.unresolved_findings {
        check(
            !finding.id.trim().is_empty() && !finding.follow_up.trim().is_empty(),
            "missing",
            "name each unresolved finding and its linked follow-up ticket",
        )?;
        check(
            store
                .list_tickets()?
                .iter()
                .any(|candidate| candidate.slug == finding.follow_up),
            "missing",
            "create the linked follow-up ticket before handoff",
        )?;
    }
    checks.push(Check {
        item: "unresolved_findings",
        status: "self_attested",
        detail: "listed follow-ups exist",
    });

    let outcome = match &evidence.outcome {
        Outcome::Code {
            worktree,
            branch,
            commit,
            tree,
            risk_tier,
        } => {
            check(
                full_sha(commit) && full_sha(tree),
                "missing",
                "provide the immutable full commit and tree SHA",
            )?;
            check(
                worktree.is_absolute() && !branch.trim().is_empty(),
                "missing",
                "report the absolute worker worktree path and its branch",
            )?;
            let actual_commit = git_text(
                worktree,
                &["rev-parse", "HEAD"],
                "open the reported worker worktree",
            )?;
            let actual_tree = git_text(
                worktree,
                &["rev-parse", "HEAD^{tree}"],
                "read the reported commit tree",
            )?;
            let actual_branch = git_text(
                worktree,
                &["branch", "--show-current"],
                "read the reported branch",
            )?;
            check(
                actual_commit == *commit && actual_tree == *tree && actual_branch == *branch,
                "stale",
                "commit, tree, or branch changed after review; rerun gates and preflight for the new tree",
            )?;
            let common = |checkout: &Path| {
                git_text(
                    checkout,
                    &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                    "read the Git common directory",
                )
            };
            check(
                common(worktree)? == common(project)?,
                "stale",
                "use a worktree of the assigned shipping project",
            )?;
            check(
                git_text(
                    worktree,
                    &["status", "--porcelain"],
                    "inspect worker source status",
                )?
                .is_empty(),
                "missing",
                "commit or preserve every local edit before handoff",
            )?;
            check(
                git_output(
                    project,
                    &["merge-base", "--is-ancestor", &assigned.base, commit],
                )?
                .status
                .success(),
                "stale",
                "handoff commit must descend from the assignment's exact source base",
            )?;
            if !assigned.scope.is_empty() {
                review_commit_scope(project, &assigned.base, commit, &assigned.scope)?;
            }
            let marker = git_text(
                worktree,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    &format!("hotsheet-root-gate/{tree}"),
                ],
                "read the exact-tree gate marker",
            )?;
            check(
                fs::read_to_string(marker).is_ok_and(|recorded| recorded.trim() == tree),
                "missing",
                "run scripts/check.sh on this clean committed tree; no gate receipt matches it",
            )?;
            let changed = git_text(
                project,
                &["diff", "--name-only", &assigned.base, commit],
                "inspect changed source paths",
            )?;
            if evidence.ui_affecting
                || changed.lines().any(|path| {
                    path.starts_with("clients/web/src/")
                        && (path.ends_with(".tsx") || path.ends_with(".css"))
                })
            {
                inspect_ui(&ticket, evidence.ui.as_ref())?;
                checks.push(Check {
                    item: "rendered_ui_inspection",
                    status: "self_attested",
                    detail: "real parent and viewports; attachment verified separately",
                });
                if !ticket.attachments.is_empty() {
                    checks.push(Check {
                        item: "after_attachment",
                        status: "verified",
                        detail: "ticket attachment id and correctness purpose",
                    });
                }
            }
            checks.push(Check {
                item: "commit_tree_branch_worktree_scope",
                status: "verified",
                detail: "immutable clean source",
            });
            checks.push(Check {
                item: "risk_tier_gate",
                status: "verified",
                detail: "exact committed tree gate marker",
            });
            checks.push(Check {
                item: "risk_tier_selection",
                status: "self_attested",
                detail: match risk_tier {
                    RiskTier::T0 => "t0",
                    RiskTier::T1 => "t1",
                    RiskTier::T2 => "t2",
                },
            });
            "code"
        }
        Outcome::Findings { note } => {
            check(
                note.lines().next() == Some("## Findings handoff")
                    && note.lines().skip(1).any(|line| !line.trim().is_empty()),
                "missing",
                "provide a nonempty proposed Findings handoff note",
            )?;
            if evidence.ui_affecting || evidence.ui.is_some() || !ticket.attachments.is_empty() {
                inspect_ui(&ticket, evidence.ui.as_ref())?;
                checks.push(Check {
                    item: "rendered_ui_inspection",
                    status: "self_attested",
                    detail: "real parent and viewports",
                });
                if !ticket.attachments.is_empty() {
                    checks.push(Check {
                        item: "after_attachment",
                        status: "verified",
                        detail: "ticket attachment id and correctness purpose",
                    });
                }
            }
            checks.push(Check {
                item: "findings_note",
                status: "self_attested",
                detail: "proposed findings; no code or CI required",
            });
            "findings"
        }
    };
    let latest = store.read_ticket(&native_id)?;
    check(
        same_ticket_snapshot(&latest, &evidence) && latest.claim_history == ticket.claim_history,
        "stale",
        "ticket notes or claim changed during preflight; reread and rerun preflight",
    )?;
    Ok(Report {
        assignment: assignment.to_owned(),
        ticket: evidence.ticket,
        outcome,
        checks,
        published: false,
    })
}

/// Repeat the cheap preflight at the publication boundary. The ticket mutation is
/// compare-and-release under one lock; a retry after a crash resumes only the exact
/// note written from this evidence file (HS2-NNSWDS).
pub(super) fn publish(
    coordinator: &Coordinator,
    source: &Path,
    project: &Path,
    worker: &str,
    generation: &str,
    assignment: &str,
    evidence_file: &Path,
) -> Result<Report> {
    let bytes = fs::read(evidence_file).context("missing: provide the reviewed evidence file")?;
    let evidence: Evidence = serde_json::from_slice(&bytes)
        .context("unsupported: readiness evidence must match the code or findings JSON contract")?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let marker = format!("Readiness evidence SHA-256: {digest}");
    let state = coordinator.state()?;
    let assigned = state
        .assignments
        .get(assignment)
        .context("missing: inspect the exact assignment id in coordination status")?;
    check(
        assigned.worker == worker && assigned.generation == generation,
        "stale",
        "use the assignment's exact worker id and session generation",
    )?;
    check(
        assigned.ticket == evidence.ticket,
        "stale",
        "use the exact qualified ticket from the assignment",
    )?;
    check(
        assigned.scope == evidence.scope,
        "stale",
        "use the exact scope from the assignment for replay",
    )?;
    let store = FsStore::open_without_maintenance(source)
        .context("unsupported: local claim and note authority is unavailable")?;
    check(
        evidence.ticket.connection_id == git_connection_id(&store),
        "unsupported",
        "this ticket provider has no verified local publication path",
    )?;
    let native_id = Ulid::from_string(&evidence.ticket.native_id)?;
    let ticket = store.read_ticket(&native_id)?;
    if let Some(existing) = ticket.notes.iter().find(|note| note.text.contains(&marker)) {
        check(
            ticket
                .notes
                .last()
                .is_some_and(|note| note.id == existing.id),
            "stale",
            "a note followed the published handoff; inspect it before replay",
        )?;
        check(
            ticket.claimed_by.is_none(),
            "stale",
            "the published handoff still has a live claim; inspect recovery before replay",
        )?;
        let release = ticket.claim_history.last();
        let last_deadline = ticket.claim_history.iter().rev().skip(1).find_map(|event| {
            (event.worker == worker)
                .then_some(event.lease_expires_at.as_ref())
                .flatten()
        });
        check(
            release.is_some_and(|event| {
                event.kind == ClaimEventKind::Release && event.worker == worker
            }) && release
                .and_then(|event| event.at.instant())
                .zip(last_deadline.and_then(Timestamp::instant))
                .is_some_and(|(at, deadline)| at < deadline),
            "stale",
            "the exact worker claim was not released before its deadline; inspect claim history",
        )?;
        check(
            ticket
                .claim_history
                .iter()
                .rev()
                .find(|event| event.kind == ClaimEventKind::Claim)
                .is_some_and(|event| {
                    event.id.to_string() == evidence.claim_attempt && event.worker == worker
                }),
            "stale",
            "the published note belongs to a different claim attempt; inspect claim history",
        )?;
        match (&evidence.outcome, &assigned.handoff) {
            (Outcome::Code { commit, .. }, Some(HandoffEvidence::Code { commit: recorded }))
                if assigned.phase == Phase::Handoff && commit == recorded => {}
            (Outcome::Findings { .. }, Some(HandoffEvidence::Findings { note_id }))
                if assigned.phase == Phase::Handoff && *note_id == existing.id.to_string() => {}
            (_, None) if assigned.phase == Phase::Accepted => {
                record_mailbox(
                    coordinator,
                    worker,
                    generation,
                    assignment,
                    &evidence.outcome,
                    existing.id,
                )?;
            }
            _ => anyhow::bail!(
                "stale: mailbox handoff differs from the published note; inspect the assignment"
            ),
        }
        return Ok(Report {
            assignment: assignment.to_owned(),
            ticket: evidence.ticket,
            outcome: match evidence.outcome {
                Outcome::Code { .. } => "code",
                Outcome::Findings { .. } => "findings",
            },
            checks: vec![Check {
                item: "handoff_replay",
                status: "verified",
                detail: "exact published note and released claim",
            }],
            published: true,
        });
    }

    let mut report = preflight(
        coordinator,
        source,
        project,
        worker,
        generation,
        assignment,
        evidence_file,
    )?;
    let note = handoff_note(assignment, &evidence, &marker);
    let note_id = Ulid::new();
    ops::publish_handoff_note(
        &store,
        &native_id,
        ops::HandoffPublication {
            note_id,
            now: Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
            worker,
            claim_attempt: &Ulid::from_string(&evidence.claim_attempt)?,
            reviewed_updated_at: &Timestamp::new(&evidence.ticket_updated_at),
            text: note,
        },
    )?;
    record_mailbox(
        coordinator,
        worker,
        generation,
        assignment,
        &evidence.outcome,
        note_id,
    )?;
    report.published = true;
    Ok(report)
}

fn record_mailbox(
    coordinator: &Coordinator,
    worker: &str,
    generation: &str,
    assignment: &str,
    outcome: &Outcome,
    note_id: Ulid,
) -> Result<()> {
    match outcome {
        Outcome::Code { commit, .. } => coordinator.worker_transition(
            worker,
            generation,
            assignment,
            Phase::Handoff,
            Some(commit.clone()),
        ),
        Outcome::Findings { .. } => coordinator.worker_findings_transition(
            worker,
            generation,
            assignment,
            note_id.to_string(),
        ),
    }
}

fn handoff_note(assignment: &str, evidence: &Evidence, marker: &str) -> String {
    let follow_ups = evidence
        .unresolved_findings
        .iter()
        .map(|finding| format!("{} -> {}", finding.id, finding.follow_up))
        .collect::<Vec<_>>()
        .join(", ");
    let ui = evidence
        .ui
        .as_ref()
        .map(|review| {
            format!(
                "\nUI review (self-attested): real parent {}; states {}; viewports {}; after attachment {}.",
                review.real_parent,
                review.states.join(", "),
                review.viewports.join(", "),
                review.after_attachment.as_deref().unwrap_or("not required")
            )
        })
        .unwrap_or_default();
    let suffix = format!(
        "\n\nAssignment: {assignment}\nRequested scope: {}\n{marker}\nLatest instructions, docs and coverage reviewed: self-attested.{ui}\nUnresolved findings and follow-ups: {}\nTicket remains Started pending integrator review.",
        if evidence.scope.is_empty() {
            "legacy unscoped".to_owned()
        } else {
            evidence.scope.join(", ")
        },
        if follow_ups.is_empty() {
            "none"
        } else {
            &follow_ups
        }
    );
    match &evidence.outcome {
        Outcome::Code {
            worktree,
            branch,
            commit,
            tree,
            risk_tier,
        } => format!(
            "## Integration handoff\nBranch: {branch}\nCommit: {commit}\nTree: {tree}\nWorktree (machine-local): {}\nRisk tier: {:?}; exact-tree local gate verified.{suffix}",
            worktree.display(),
            risk_tier
        ),
        Outcome::Findings { note } => format!("{}{suffix}", note.trim_end()),
    }
}

fn inspect_ui(ticket: &hotsheet_model::Ticket, ui: Option<&UiReview>) -> Result<()> {
    let ui = ui
        .context("missing: inspect the real parent UI at wide, medium, narrow and resize states")?;
    check(
        ui.inspected_rendered_page && !ui.real_parent.trim().is_empty() && !ui.states.is_empty(),
        "missing",
        "attest the real rendered parent and affected states; a DOM assertion is insufficient",
    )?;
    check(
        ["wide", "medium", "narrow", "resize"]
            .iter()
            .all(|size| ui.viewports.iter().any(|item| item == size)),
        "missing",
        "inspect wide, medium, narrow, and resize transitions",
    )?;
    if !ticket.attachments.is_empty() {
        check(
            ui.after_attachment.as_deref().is_some_and(|id| {
                ticket.attachments.iter().any(|attachment| {
                    attachment.id.to_string() == id
                        && attachment.purpose == Some(AttachmentPurpose::CorrectnessEvidence)
                })
            }),
            "missing",
            "attach the matching after image as correctness evidence and reference its attachment id",
        )?;
    }
    Ok(())
}
