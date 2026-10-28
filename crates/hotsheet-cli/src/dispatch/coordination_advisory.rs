//! Frozen, read-only advisory review evidence (HS2-E20S6K).
//! Reviewers publish ticket notes; they never acquire the implementation claim or main fence.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use hotsheet_cli::coordination::{Coordinator, HandoffEvidence, Phase, ReturnRoute};
use hotsheet_model::{AttachmentActorRole, Note, NoteActor, NoteKind, Timestamp, Ulid};
use hotsheet_ticketing::{FsStore, TicketRef, git_connection_id};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Identity, git_output, review_commit_scope};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    ticket: TicketRef,
    base: String,
    handoff: HandoffEvidence,
    handoff_note: String,
    request_note: String,
    ticket_digest: String,
    review_main: String,
    /// Required for code; absent for findings.
    tree: Option<String>,
    verdict: Verdict,
    summary: String,
    findings: Vec<Finding>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Verdict {
    Ready,
    ChangesRequested,
}

#[derive(Debug, Deserialize, Serialize)]
struct ReviewRecord {
    version: u32,
    assignment: String,
    ticket: TicketRef,
    base: String,
    handoff: HandoffEvidence,
    handoff_note: String,
    request_note: String,
    ticket_digest: String,
    review_main: String,
    tree: Option<String>,
    reviewer: String,
    generation: String,
    verdict: Verdict,
    finding_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Finding {
    severity: Severity,
    /// Repository path and line, or an exact ticket note/attachment identifier.
    evidence_ref: String,
    action: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Severity {
    Blocking,
    Major,
    Minor,
}

#[derive(Debug, Serialize)]
pub(super) struct Report {
    assignment: String,
    ticket: TicketRef,
    handoff: HandoffEvidence,
    handoff_note: String,
    request_note: String,
    ticket_digest: String,
    review_main: String,
    tree: Option<String>,
    checks: Vec<Check>,
    published_note: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct InspectReport {
    assignment: String,
    note: String,
    reviewer: String,
    verdict: Verdict,
    finding_count: usize,
    current: bool,
    checks: Vec<Check>,
}

#[derive(Debug, Serialize)]
struct Check {
    item: &'static str,
    status: &'static str,
    detail: &'static str,
}

#[derive(Debug, Serialize, Deserialize)]
struct RequestRecord {
    version: u32,
    assignment: String,
    ticket: TicketRef,
    base: String,
    handoff: HandoffEvidence,
    handoff_note: String,
    reviewer: String,
    reviewer_session: String,
    integrator: String,
    return_route: ReturnRoute,
}

#[derive(Debug, Serialize)]
pub(super) struct RequestReport {
    assignment: String,
    request_note: String,
    return_route: ReturnRoute,
    delivery: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct CancelReport {
    assignment: String,
    request_note: String,
    cancelled_note: String,
}

fn full_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn git_text(project: &Path, args: &[&str]) -> Result<String> {
    let output = git_output(project, args)?;
    ensure!(
        output.status.success(),
        "stale: Git evidence is unavailable: {args:?}"
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn ticket_digest(ticket: &hotsheet_model::Ticket) -> Result<String> {
    let meaningful_notes = ticket
        .notes
        .iter()
        .filter(|note| {
            note.kind != NoteKind::Activity
                && (note.human_edited
                    || note
                        .actor
                        .as_ref()
                        .is_none_or(|actor| actor.role != AttachmentActorRole::Ai)
                    || !(note.text.starts_with("## Advisory review\n")
                        || note.text.starts_with("## Advisory review request\n")
                        || note.text.starts_with("## Advisory review cancellation\n")))
        })
        .map(|note| {
            serde_json::json!({
                "id": note.id,
                "kind": format!("{:?}", note.kind),
                "edited_at": note.edited_at,
                "text": note.text,
            })
        })
        .collect::<Vec<_>>();
    let snapshot = serde_json::json!({
        "id": ticket.id,
        "title": ticket.title,
        "category": ticket.category,
        "status": ticket.status,
        "priority": ticket.priority,
        "details": ticket.details,
        "tags": ticket.tags,
        "blocked_by": ticket.blocked_by,
        "follow_ups": ticket.follow_ups,
        "attachments": ticket.attachments,
        "notes": meaningful_notes,
    });
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&snapshot)?)
    ))
}

fn current_main(project: &Path) -> Result<String> {
    git_text(project, &["rev-parse", "main"])
}

fn handoff_note(ticket: &hotsheet_model::Ticket, handoff: &HandoffEvidence) -> Result<String> {
    let note = ticket
        .notes
        .iter()
        .rev()
        .find(|note| {
            note.kind == NoteKind::Regular
                && match handoff {
                    HandoffEvidence::Code { commit } => {
                        note.text.lines().next() == Some("## Integration handoff")
                            && note.text.split_whitespace().any(|part| part == commit)
                    }
                    HandoffEvidence::Findings { note_id } => {
                        note.id.to_string() == *note_id
                            && note.text.lines().next() == Some("## Findings handoff")
                    }
                }
        })
        .context("missing: inspect the exact durable code or findings handoff note")?;
    Ok(note.id.to_string())
}

fn reviewer_assignment(
    coordinator: &Coordinator,
    reviewer: &str,
    assignment: &str,
) -> Result<hotsheet_cli::coordination::Assignment> {
    let state = coordinator.state()?;
    ensure!(
        !state.workers.contains_key(reviewer)
            && state.dispatcher.as_ref().is_none_or(|s| s.id != reviewer)
            && state.integrator.as_ref().is_none_or(|s| s.id != reviewer),
        "unsupported: advisory reviewer identity must be separate from pool roles"
    );
    let assigned = state
        .assignments
        .get(assignment)
        .context("missing: inspect the durable assignment id before advisory review")?;
    ensure!(
        assigned.worker != reviewer && assigned.phase == Phase::Handoff,
        "stale: advisory review requires another worker's frozen Handoff"
    );
    Ok(assigned.clone())
}

fn request_record(note: &Note) -> Option<RequestRecord> {
    note.text
        .lines()
        .find_map(|line| line.strip_prefix("Request record: "))
        .and_then(|line| serde_json::from_str(line).ok())
}

fn cancelled(ticket: &hotsheet_model::Ticket, request_note: &str) -> Option<String> {
    ticket
        .notes
        .iter()
        .find(|note| {
            note.text.starts_with("## Advisory review cancellation\n")
                && !note.human_edited
                && note
                    .actor
                    .as_ref()
                    .is_some_and(|actor| actor.role == AttachmentActorRole::Ai)
                && note
                    .text
                    .lines()
                    .any(|line| line == format!("Request note: {request_note}"))
        })
        .map(|note| note.id.to_string())
}

fn requested_review(
    ticket: &hotsheet_model::Ticket,
    assigned: &hotsheet_cli::coordination::Assignment,
    assignment: &str,
    reviewer: &str,
    generation: &str,
) -> Result<String> {
    ticket
        .notes
        .iter()
        .rev()
        .find(|note| {
            note.kind == NoteKind::Regular
                && note.text.starts_with("## Advisory review request\n")
                && !note.human_edited
                && request_record(note).is_some_and(|record| {
                    record.version == 1
                        && record.assignment == assignment
                        && record.ticket == assigned.ticket
                        && record.base == assigned.base
                        && assigned.handoff.as_ref() == Some(&record.handoff)
                        && record.reviewer == reviewer
                        && record.reviewer_session == generation
                        && handoff_note(ticket, &record.handoff).ok().as_deref()
                            == Some(record.handoff_note.as_str())
                        && note.actor.as_ref().is_some_and(|actor| {
                            actor.id.as_deref() == Some(record.integrator.as_str())
                        })
                })
                && cancelled(ticket, &note.id.to_string()).is_none()
                && note
                    .actor
                    .as_ref()
                    .is_some_and(|actor| actor.role == AttachmentActorRole::Ai)
        })
        .map(|note| note.id.to_string())
        .context("missing: exact integrator-addressed advisory request; delivery must be explicit")
}

pub(super) fn request(
    coordinator: &Coordinator,
    source: &Path,
    identity: &Identity,
    assignment: &str,
    reviewer: &str,
    reviewer_session: &str,
    return_route: &ReturnRoute,
) -> Result<RequestReport> {
    ensure!(
        !reviewer.trim().is_empty() && !reviewer_session.trim().is_empty(),
        "missing: reviewer identity and session are required"
    );
    let state = coordinator.state()?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    ensure!(
        state
            .integrator
            .as_ref()
            .is_some_and(|s| s.id == identity.id
                && s.generation == identity.session
                && s.live(now)),
        "stale: only the exact live integrator can request advisory review"
    );
    let assigned = reviewer_assignment(coordinator, reviewer, assignment)?;
    let store = FsStore::open_without_maintenance(source)?;
    ensure!(
        assigned.ticket.connection_id == git_connection_id(&store),
        "unsupported: provider cannot publish a verified advisory request"
    );
    let native_id = Ulid::from_string(&assigned.ticket.native_id)?;
    let note_id = store.with_ticket_transaction(|| {
        let mut ticket = store.read_ticket(&native_id)?;
        let handoff = assigned.handoff.clone().context("missing: frozen handoff")?;
        let handoff_note_id = handoff_note(&ticket, &handoff)?;
        if let Ok(existing) = requested_review(&ticket, &assigned, assignment, reviewer, reviewer_session) {
            let note = ticket.notes.iter().find(|note| note.id.to_string() == existing).unwrap();
            let record = request_record(note).unwrap();
            ensure!(record.return_route == *return_route,
                "stale: same reviewer request has a different return route");
            return Ok(note.id);
        }
        let outstanding = ticket.notes.iter().filter(|note| {
            request_record(note).is_some_and(|r| r.assignment == assignment
                && r.handoff == handoff
                && cancelled(&ticket, &note.id.to_string()).is_none()
                && !ticket.notes.iter().any(|result| result.text.starts_with("## Advisory review\n")
                    && result.text.contains(&format!("Request note: {}", note.id))))
        }).count();
        ensure!(outstanding < 2, "capacity: two advisory reviews are already outstanding for this handoff");
        let current = coordinator.state()?;
        ensure!(current.integrator.as_ref().is_some_and(|s| s.id == identity.id
            && s.generation == identity.session && s.live(now))
            && current.assignments.get(assignment).is_some_and(|a| a == &assigned),
            "stale: integrator or handoff changed before request publication");
        let record = RequestRecord { version: 1, assignment: assignment.to_owned(),
            ticket: assigned.ticket.clone(), base: assigned.base.clone(), handoff,
            handoff_note: handoff_note_id, reviewer: reviewer.to_owned(),
            reviewer_session: reviewer_session.to_owned(), integrator: identity.id.clone(),
            return_route: return_route.clone() };
        let id = Ulid::new();
        let timestamp = Timestamp::from_datetime(time::OffsetDateTime::now_utc());
        ticket.notes.push(Note { id, kind: NoteKind::Regular, kind_explicit: true,
            created_at: timestamp.clone(), edited_at: timestamp.clone(), summary: None,
            confidence: None, feedback_for: None, feedback_request: false, response_to: None,
            ai_feedback: None, human_edited: false,
            actor: Some(NoteActor { role: AttachmentActorRole::Ai, id: Some(identity.id.clone()) }),
            text: format!("## Advisory review request\nRequest record: {}\n\nAddress this exact frozen handoff through the stated route. Delivery requires an explicit send or human relay; this note does not wake the reviewer.", serde_json::to_string(&record)?) });
        ticket.updated_at = timestamp;
        store.write_ticket_committing(&ticket)?;
        Ok(id)
    })?;
    Ok(RequestReport {
        assignment: assignment.to_owned(),
        request_note: note_id.to_string(),
        return_route: return_route.clone(),
        delivery: "explicit_send_required",
    })
}

pub(super) fn cancel(
    coordinator: &Coordinator,
    source: &Path,
    identity: &Identity,
    assignment: &str,
    request_note: &str,
) -> Result<CancelReport> {
    let state = coordinator.state()?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    ensure!(
        state
            .integrator
            .as_ref()
            .is_some_and(|s| s.id == identity.id
                && s.generation == identity.session
                && s.live(now)),
        "stale: only the exact live integrator can cancel advisory review"
    );
    let assigned = state
        .assignments
        .get(assignment)
        .context("missing: assignment")?;
    let store = FsStore::open_without_maintenance(source)?;
    ensure!(
        assigned.ticket.connection_id == git_connection_id(&store),
        "unsupported: provider cannot cancel advisory request"
    );
    let native_id = Ulid::from_string(&assigned.ticket.native_id)?;
    let cancelled_note = store.with_ticket_transaction(|| {
        let mut ticket = store.read_ticket(&native_id)?;
        let request = ticket.notes.iter().find(|note| note.id.to_string() == request_note
            && note.text.starts_with("## Advisory review request\n"))
            .context("missing: exact advisory request note")?;
        let record = request_record(request).context("missing: structured request record")?;
        ensure!(record.assignment == assignment && record.ticket == assigned.ticket,
            "stale: advisory request belongs to another assignment");
        if let Some(existing) = cancelled(&ticket, request_note) {
            return Ulid::from_string(&existing).map_err(Into::into);
        }
        let id = Ulid::new();
        let timestamp = Timestamp::from_datetime(time::OffsetDateTime::now_utc());
        ticket.notes.push(Note { id, kind: NoteKind::Regular, kind_explicit: true,
            created_at: timestamp.clone(), edited_at: timestamp.clone(), summary: None,
            confidence: None, feedback_for: None, feedback_request: false, response_to: None,
            ai_feedback: None, human_edited: false,
            actor: Some(NoteActor { role: AttachmentActorRole::Ai, id: Some(identity.id.clone()) }),
            text: format!("## Advisory review cancellation\nAssignment: {assignment}\nRequest note: {request_note}\n\nThe integrator cancelled this advisory request; any later result needs a new addressed request.") });
        ticket.updated_at = timestamp;
        store.write_ticket_committing(&ticket)?;
        Ok(id)
    })?;
    Ok(CancelReport {
        assignment: assignment.to_owned(),
        request_note: request_note.to_owned(),
        cancelled_note: cancelled_note.to_string(),
    })
}

fn snapshot(
    coordinator: &Coordinator,
    source: &Path,
    project: &Path,
    reviewer: &str,
    generation: &str,
    assignment: &str,
) -> Result<Report> {
    let assigned = reviewer_assignment(coordinator, reviewer, assignment)?;
    let store = FsStore::open_without_maintenance(source)
        .context("unsupported: advisory review requires a local Markdown/Git ticket source")?;
    ensure!(
        assigned.ticket.connection_id == git_connection_id(&store),
        "unsupported: this provider has no verified advisory note publication"
    );
    let ticket = store.read_ticket(&Ulid::from_string(&assigned.ticket.native_id)?)?;
    ensure!(
        ticket.slug == assigned.slug,
        "stale: assigned ticket identity changed"
    );
    let handoff = assigned
        .handoff
        .clone()
        .context("missing: assignment has no handoff")?;
    let note_id = handoff_note(&ticket, &handoff)?;
    let request_note_id = requested_review(&ticket, &assigned, assignment, reviewer, generation)?;
    let tree = match &handoff {
        HandoffEvidence::Code { commit } => {
            ensure!(
                full_sha(commit) && full_sha(&assigned.base),
                "missing: full code commit and base are required"
            );
            let tree = git_text(project, &["rev-parse", &format!("{commit}^{{tree}}")])?;
            ensure!(full_sha(&tree), "stale: code tree is unavailable");
            ensure!(
                git_output(
                    project,
                    &["merge-base", "--is-ancestor", &assigned.base, commit]
                )?
                .status
                .success(),
                "stale: handoff commit does not descend from its exact assignment base"
            );
            if !assigned.scope.is_empty() {
                review_commit_scope(project, &assigned.base, commit, &assigned.scope)?;
            }
            Some(tree)
        }
        HandoffEvidence::Findings { .. } => None,
    };
    Ok(Report {
        assignment: assignment.to_owned(),
        ticket: assigned.ticket,
        handoff,
        handoff_note: note_id,
        request_note: request_note_id,
        ticket_digest: ticket_digest(&ticket)?,
        review_main: current_main(project)?,
        tree,
        checks: vec![
            Check {
                item: "review_request",
                status: "verified",
                detail: "integrator addressed a separate reviewer session",
            },
            Check {
                item: "qualified_handoff",
                status: "verified",
                detail: "durable exact source evidence",
            },
            Check {
                item: "ticket_instructions",
                status: "verified",
                detail: "digest excludes other advisory notes",
            },
        ],
        published_note: None,
    })
}

fn check_main_context(
    project: &Path,
    reviewed: &str,
    current: &str,
    scope: &[String],
) -> Result<()> {
    ensure!(
        full_sha(reviewed),
        "missing: use the full reviewed main SHA"
    );
    if reviewed == current {
        return Ok(());
    }
    ensure!(
        git_output(project, &["merge-base", "--is-ancestor", reviewed, current])?
            .status
            .success(),
        "stale: reviewed main is not an ancestor of current main"
    );
    ensure!(
        !scope.is_empty(),
        "stale: unscoped review needs a refresh after main advanced"
    );
    let changed = git_text(project, &["diff", "--name-only", reviewed, current])?;
    ensure!(
        !changed.lines().any(|path| scope.iter().any(|declared| {
            let root = declared.split('#').next().unwrap_or(declared);
            path == root
                || path.starts_with(&format!("{root}/"))
                || root.starts_with(&format!("{path}/"))
        })),
        "stale: main advanced inside the reviewed scope; refresh the affected review"
    );
    Ok(())
}

pub(super) fn run(
    coordinator: &Coordinator,
    source: &Path,
    project: &Path,
    identity: &Identity,
    assignment: &str,
    evidence_file: Option<&Path>,
    publish: bool,
) -> Result<Report> {
    let reviewer = identity.id.as_str();
    let generation = identity.session.as_str();
    let Some(path) = evidence_file else {
        return snapshot(
            coordinator,
            source,
            project,
            reviewer,
            generation,
            assignment,
        );
    };
    let bytes = fs::read(path).context("missing: supply the reviewed evidence file")?;
    let evidence: Evidence = serde_json::from_slice(&bytes)
        .context("unsupported: advisory evidence must match the review JSON contract")?;
    let mut report = snapshot(
        coordinator,
        source,
        project,
        reviewer,
        generation,
        assignment,
    )?;
    ensure!(
        report.ticket == evidence.ticket
            && report.handoff == evidence.handoff
            && report.handoff_note == evidence.handoff_note
            && report.request_note == evidence.request_note
            && report.ticket_digest == evidence.ticket_digest,
        "stale: qualified handoff or ticket instructions changed; take a fresh advisory snapshot"
    );
    let assigned = reviewer_assignment(coordinator, reviewer, assignment)?;
    ensure!(
        assigned.base == evidence.base,
        "stale: assignment source base changed"
    );
    ensure!(
        report.tree == evidence.tree,
        "stale: immutable code tree changed"
    );
    check_main_context(
        project,
        &evidence.review_main,
        &report.review_main,
        &assigned.scope,
    )?;
    ensure!(
        !evidence.summary.trim().is_empty(),
        "missing: give an actionable review summary"
    );
    ensure!(
        evidence
            .findings
            .iter()
            .all(|finding| !finding.evidence_ref.trim().is_empty()
                && !finding.action.trim().is_empty()),
        "missing: every finding needs severity, exact evidence reference and action"
    );
    ensure!(
        !matches!(evidence.verdict, Verdict::Ready)
            || evidence
                .findings
                .iter()
                .all(|f| !matches!(f.severity, Severity::Blocking)),
        "missing: blocking findings require changes_requested verdict"
    );
    report.checks.push(Check {
        item: "main_context",
        status: "verified",
        detail: "same main or disjoint advancement",
    });
    report.checks.push(Check {
        item: "review_judgment",
        status: "self_attested",
        detail: "integrator retains final decision",
    });
    if !publish {
        return Ok(report);
    }
    let mut hash = Sha256::new();
    hash.update(reviewer.as_bytes());
    hash.update(&bytes);
    let digest = format!("{:x}", hash.finalize());
    let marker = format!("Advisory evidence SHA-256: {digest}");
    let store = FsStore::open_without_maintenance(source)?;
    let native_id = Ulid::from_string(&evidence.ticket.native_id)?;
    let note_id = store.with_ticket_transaction(|| {
        let ticket = store.read_ticket(&native_id)?;
        if let Some(existing) = ticket.notes.iter().find(|note| note.text.contains(&marker)) {
            return Ok(existing.id);
        }
        ensure!(
            !ticket.notes.iter().any(|note| {
                note.text.starts_with("## Advisory review\n")
                    && note.text.lines().any(|line| line == format!("Assignment: {assignment}"))
                    && note.text.lines().any(|line| line == format!("Reviewer: {reviewer}"))
            }),
            "stale: this reviewer already published a different result for the frozen handoff"
        );
        ensure!(ticket_digest(&ticket)? == evidence.ticket_digest,
            "stale: ticket instructions changed before advisory publication");
        let current = reviewer_assignment(coordinator, reviewer, assignment)?;
        ensure!(current.ticket == evidence.ticket && current.base == evidence.base
            && current.handoff.as_ref() == Some(&evidence.handoff),
            "stale: assignment changed before advisory publication");
        ensure!(requested_review(&ticket, &current, assignment, reviewer, generation)? == evidence.request_note,
            "stale: addressed review request changed before publication");
        let now = Timestamp::from_datetime(time::OffsetDateTime::now_utc());
        let note_id = Ulid::new();
        let findings = evidence.findings.iter().map(|finding| {
            format!("- {:?}: {} -> {}", finding.severity, finding.evidence_ref, finding.action)
        }).collect::<Vec<_>>().join("\n");
        let record = ReviewRecord {
            version: 1,
            assignment: assignment.to_owned(),
            ticket: evidence.ticket.clone(),
            base: evidence.base.clone(),
            handoff: evidence.handoff.clone(),
            handoff_note: evidence.handoff_note.clone(),
            request_note: evidence.request_note.clone(),
            ticket_digest: evidence.ticket_digest.clone(),
            review_main: evidence.review_main.clone(),
            tree: evidence.tree.clone(),
            reviewer: reviewer.to_owned(),
            generation: generation.to_owned(),
            verdict: evidence.verdict,
            finding_count: evidence.findings.len(),
        };
        let text = format!(
            "## Advisory review\nAssignment: {assignment}\nHandoff: {}\nHandoff note: {}\nRequest note: {}\nTree: {}\nSource base: {}\nReviewed main: {}\nTicket digest: {}\nReviewer: {reviewer}\nVerdict: {:?}\nReview record: {}\n{marker}\n\n{}\n\n{}\n\nAdvisory only; integrator verifies currentness and makes the final decision.",
            match &evidence.handoff { HandoffEvidence::Code { commit } => commit, HandoffEvidence::Findings { note_id } => note_id },
            evidence.handoff_note,
            evidence.request_note,
            evidence.tree.as_deref().unwrap_or("findings only"),
            evidence.base,
            evidence.review_main,
            evidence.ticket_digest,
            evidence.verdict,
            serde_json::to_string(&record)?,
            evidence.summary,
            if findings.is_empty() { "No findings." } else { &findings },
        );
        let mut ticket = ticket;
        ticket.notes.push(Note {
            id: note_id, kind: NoteKind::Regular, kind_explicit: true,
            created_at: now.clone(), edited_at: now.clone(), summary: None,
            confidence: None, feedback_for: None, feedback_request: false,
            response_to: None, ai_feedback: None, human_edited: false,
            actor: Some(NoteActor { role: AttachmentActorRole::Ai, id: Some(reviewer.to_owned()) }),
            text,
        });
        ticket.updated_at = now;
        store.write_ticket_committing(&ticket)?;
        Ok(note_id)
    })?;
    report.published_note = Some(note_id.to_string());
    Ok(report)
}

pub(super) fn inspect(
    coordinator: &Coordinator,
    source: &Path,
    project: &Path,
    integrator: &str,
    generation: &str,
    assignment: &str,
    note_id: &str,
) -> Result<InspectReport> {
    let state = coordinator.state()?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    ensure!(
        state.integrator.as_ref().is_some_and(|session| {
            session.id == integrator && session.generation == generation && session.live(now)
        }),
        "stale: only the exact live integrator can inspect an advisory result"
    );
    let assigned = state
        .assignments
        .get(assignment)
        .context("missing: assignment is unavailable")?;
    ensure!(
        assigned.phase == Phase::Handoff,
        "stale: advisory result must be inspected before serial integration or verification"
    );
    let store = FsStore::open_without_maintenance(source)
        .context("unsupported: local ticket evidence is unavailable")?;
    ensure!(
        assigned.ticket.connection_id == git_connection_id(&store),
        "unsupported: this provider has no verified advisory result inspection"
    );
    let ticket = store.read_ticket(&Ulid::from_string(&assigned.ticket.native_id)?)?;
    let note_id = Ulid::from_string(note_id)?;
    let note = ticket
        .notes
        .iter()
        .find(|note| {
            note.id == note_id
                && note.kind == NoteKind::Regular
                && note.text.lines().next() == Some("## Advisory review")
                && !note.human_edited
        })
        .context("missing: regular Advisory review note was not found on the exact ticket")?;
    let record_line = note
        .text
        .lines()
        .find_map(|line| line.strip_prefix("Review record: "))
        .context("unsupported: advisory note lacks a structured result record")?;
    let record: ReviewRecord = serde_json::from_str(record_line)
        .context("unsupported: advisory result record is malformed")?;
    ensure!(
        record.version == 1
            && record.assignment == assignment
            && record.ticket == assigned.ticket
            && record.base == assigned.base
            && assigned.handoff.as_ref() == Some(&record.handoff),
        "stale: advisory result does not match the current qualified handoff"
    );
    ensure!(
        record.handoff_note == handoff_note(&ticket, &record.handoff)?,
        "stale: exact handoff note changed"
    );
    ensure!(
        requested_review(
            &ticket,
            assigned,
            assignment,
            &record.reviewer,
            &record.generation
        )? == record.request_note,
        "stale: advisory request is missing or changed"
    );
    ensure!(
        ticket_digest(&ticket)? == record.ticket_digest,
        "stale: ticket instructions or evidence changed after the review"
    );
    ensure!(
        note.actor
            .as_ref()
            .is_some_and(|actor| actor.role == AttachmentActorRole::Ai
                && actor.id.as_deref() == Some(record.reviewer.as_str())),
        "unsupported: advisory reviewer provenance is missing"
    );
    let tree = match &record.handoff {
        HandoffEvidence::Code { commit } => Some(git_text(
            project,
            &["rev-parse", &format!("{commit}^{{tree}}")],
        )?),
        HandoffEvidence::Findings { .. } => None,
    };
    ensure!(tree == record.tree, "stale: reviewed code tree changed");
    check_main_context(
        project,
        &record.review_main,
        &current_main(project)?,
        &assigned.scope,
    )?;
    Ok(InspectReport {
        assignment: assignment.to_owned(),
        note: note_id.to_string(),
        reviewer: record.reviewer,
        verdict: record.verdict,
        finding_count: record.finding_count,
        current: true,
        checks: vec![
            Check {
                item: "reviewer_provenance",
                status: "verified",
                detail: "integrator-addressed separate reviewer session",
            },
            Check {
                item: "handoff_and_instructions",
                status: "verified",
                detail: "exact note, source and digest",
            },
            Check {
                item: "main_context",
                status: "verified",
                detail: "same main or disjoint advancement",
            },
            Check {
                item: "review_judgment",
                status: "self_attested",
                detail: "integrator makes final decision",
            },
        ],
    })
}
