//! HS2-5P112Z: a worker on another site of a project joins the home site's one
//! coordination pool through the hosted `target-pool` route, takes the exact
//! assignment the home's authority offers it, and hands off over a pushed branch that
//! the home fetches and verifies before its integrator sees the handoff. A real git
//! checkout with a bare `origin` and a second clone stand in for the two sites;
//! synthetic tickets and identities only.

use hotsheet_model::{NoteKind, Ticket, Timestamp};
use hotsheet_ticketing::{NewTicket, ops};

use super::tickets::{NOW, machine, unsupported};
use super::*;
use crate::coordination::{Assignment, Coordinator, Phase, ReturnRoute, Role};
use crate::orchestration::site_pool::{
    HostedPoolMember, SiteEvidence, handoff_note, handoff_ref, site_evidence, valid_branch,
};

const WORKER: &str = "site-worker";
/// One evidence case: its name, the recorded phase, the ticket, the source's other
/// tickets, and what the home records.
type EvidenceCase<'a> = (&'a str, Phase, Ticket, Vec<Ticket>, Option<SiteEvidence>);
const GENERATION: &str = "session-on-site-2";

fn git(dir: &Path, args: &[&str]) -> String {
    let out = hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn commit(dir: &Path, file: &str, text: &str) -> String {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", file]);
    git(dir, &["rev-parse", "HEAD"])
}

struct Sites {
    m: super::tickets::Machine,
    /// The home's checkout, the target checkout of the machine, with its pool.
    home: PathBuf,
    /// The other site's clone of the same repository.
    site: PathBuf,
    base: String,
    coordinator: Coordinator,
}

impl Sites {
    fn new() -> Self {
        let m = machine(10, false, unsupported);
        let home = PathBuf::from(&m.target.root);
        let origin = home.parent().unwrap().join("origin.git");
        let out = hotsheet_ticketing::git::launch("git")
            .args(["init", "-q", "--bare", "-b", "main"])
            .arg(&origin)
            .output()
            .unwrap();
        assert!(out.status.success());
        git(&home, &["init", "-q", "-b", "main"]);
        let base = commit(&home, "README", "home\n");
        git(
            &home,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        git(&home, &["push", "-q", "origin", "main"]);
        let site = home.parent().unwrap().join("site-clone");
        let out = hotsheet_ticketing::git::launch("git")
            .args(["clone", "-q"])
            .arg(&origin)
            .arg(&site)
            .output()
            .unwrap();
        assert!(out.status.success());
        // The home's dispatcher set the pool up; its legacy integrator assigns here.
        let coordinator =
            Coordinator::open_with_registry_home(&home, m.target_store.root(), m.home.clone())
                .unwrap();
        coordinator
            .register(
                Role::Integrator,
                "integrator",
                "ig1",
                0,
                3600,
                ReturnRoute::HumanRelay,
            )
            .unwrap();
        Self {
            m,
            home,
            site,
            base,
            coordinator,
        }
    }

    fn member(&self) -> HostedPoolMember {
        HostedPoolMember {
            worker_id: WORKER.into(),
            generation: GENERATION.into(),
            lease_seconds: 600,
            site_target_id: id(2),
            connection_id: None,
            native_id: None,
            actor: None,
        }
    }

    fn ticket(&self) -> Ticket {
        ops::create(
            &self.m.target_store,
            ulid::Ulid::new(),
            "TGT",
            NOW.into(),
            NewTicket {
                title: "Synthetic work".into(),
                up_next: true,
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn assign(&self, ticket: &Ticket) {
        self.coordinator
            .assign(
                "integrator",
                "ig1",
                0,
                Assignment {
                    id: "assignment-1".into(),
                    worker: WORKER.into(),
                    generation: GENERATION.into(),
                    ticket: hotsheet_ticketing::TicketRef {
                        connection_id: self.m.target_connection(),
                        native_id: ticket.id.to_string(),
                    },
                    slug: ticket.slug.clone(),
                    base: self.base.clone(),
                    phase: Phase::Offered,
                    commit: None,
                    handoff: None,
                    return_route: ReturnRoute::HumanRelay,
                    recovery_note: None,
                    suspension: None,
                    suspension_history: Vec::new(),
                    scope: Vec::new(),
                    transitions: Vec::new(),
                    retirement_note: None,
                    worktree: None,
                },
            )
            .unwrap();
    }

    fn join(&self, tx: &mut Transaction<'_>) -> crate::orchestration::site_pool::HostedPoolReply {
        self.m
            .runtime
            .hosted_pool_join(&self.m.target.id, self.member(), tx)
            .unwrap()
    }
}

fn at(text: &str) -> Timestamp {
    Timestamp::new(text)
}

#[test]
fn hs2_5p112z_a_site_worker_joins_the_home_pool_and_hands_off_over_a_pushed_branch() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let s = Sites::new();
    let owner = s.m.target.id.clone();

    // Joining registers the site's worker in the home pool, once however often.
    for _ in 0..2 {
        let reply = s.join(&mut tx);
        assert!(reply.pool && reply.joined, "{reply:?}");
        assert_eq!(reply.roles.integrator_live, Some(true));
        assert_eq!(reply.roles.dispatcher_live, Some(false));
        assert!(reply.assignment.is_none());
    }
    let state = s.coordinator.state().unwrap();
    let registered = &state.workers[WORKER];
    assert_eq!(registered.generation, GENERATION);
    assert_eq!(registered.return_route, ReturnRoute::HumanRelay);

    // The home's authority offers it the exact ticket; a read sees it and records nothing.
    let ticket = s.ticket();
    s.assign(&ticket);
    let read =
        s.m.runtime
            .hosted_pool_read(&owner, Some(WORKER), Some(GENERATION), &mut tx)
            .unwrap();
    let offered = read.assignment.unwrap();
    assert_eq!(offered.phase, Phase::Offered);
    assert_eq!(offered.native_id, ticket.id.to_string());
    assert_eq!(offered.base, s.base);
    // Not claimed yet: nothing recorded.
    let waiting = s.join(&mut tx).assignment.unwrap();
    assert_eq!(waiting.phase, Phase::Offered);
    assert_eq!(waiting.waiting.as_deref(), Some("claim_not_taken"));

    // The worker claims the ticket in the shared store: accepted, once.
    ops::claim_with_eta(
        &s.m.target_store,
        &ticket.id,
        &at(NOW),
        at("2026-10-15T02:00:00Z"),
        WORKER,
        None,
        None,
    )
    .unwrap();
    let accepted = s.join(&mut tx).assignment.unwrap();
    assert_eq!(
        (accepted.phase, accepted.recorded.as_deref()),
        (Phase::Accepted, Some("accepted"))
    );
    let held = s.join(&mut tx).assignment.unwrap();
    assert_eq!(
        (
            held.phase,
            held.recorded.as_deref(),
            held.waiting.as_deref()
        ),
        (Phase::Accepted, None, Some("claim_held"))
    );

    // Work on the site, committed on a branch but not pushed yet; claim released and
    // the handoff note written. The home cannot fetch it: still accepted.
    git(&s.site, &["checkout", "-q", "-b", "site/synthetic-1"]);
    let work = commit(&s.site, "work.txt", "done\n");
    ops::release(&s.m.target_store, &ticket.id, at(NOW), WORKER, false).unwrap();
    ops::add_note(
        &s.m.target_store,
        &ticket.id,
        ulid::Ulid::new(),
        at(NOW),
        NoteKind::Regular,
        format!(
            "## Integration handoff\nBranch: site/synthetic-1\nCommit: {work}\nSite: {}",
            id(2)
        ),
    )
    .unwrap();
    let unpushed = s.join(&mut tx).assignment.unwrap();
    assert_eq!(
        (unpushed.phase, unpushed.waiting.as_deref()),
        (Phase::Accepted, Some("handoff_fetch_failed"))
    );
    assert!(!git_ok(
        &s.home,
        &["cat-file", "-e", &format!("{work}^{{commit}}")]
    ));

    // Pushed: the home fetches it, verifies it and records the handoff with the commit.
    git(&s.site, &["push", "-q", "origin", "site/synthetic-1"]);
    let handed = s.join(&mut tx).assignment.unwrap();
    assert_eq!(
        (
            handed.phase,
            handed.recorded.as_deref(),
            handed.commit.as_deref()
        ),
        (Phase::Handoff, Some("handoff"), Some(work.as_str()))
    );
    // The integrator finds the commit in the home checkout, under the assignment's ref.
    assert_eq!(
        git(&s.home, &["rev-parse", &handoff_ref("assignment-1")]),
        work
    );
    let pool = s.coordinator.state().unwrap();
    assert_eq!(pool.assignments["assignment-1"].phase, Phase::Handoff);
    assert_eq!(
        pool.assignments["assignment-1"].commit.as_deref(),
        Some(work.as_str())
    );
    // The pool's facts the scheduler reads now show the handoff pending.
    let facts = super::super::sessions::hosted_pool(&s.home, 0).unwrap();
    assert_eq!(facts.handoffs_pending, 1);

    // Repeated: nothing more is recorded.
    let again = s.join(&mut tx).assignment.unwrap();
    assert_eq!((again.phase, again.recorded), (Phase::Handoff, None));
}

#[test]
fn hs2_5p112z_a_worker_that_claimed_and_released_between_relays_is_accepted_then_handed_off() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let s = Sites::new();
    s.join(&mut tx);
    let ticket = s.ticket();
    s.assign(&ticket);
    ops::claim_with_eta(
        &s.m.target_store,
        &ticket.id,
        &at(NOW),
        at("2026-10-15T02:00:00Z"),
        WORKER,
        None,
        None,
    )
    .unwrap();
    git(&s.site, &["checkout", "-q", "-b", "site/quick"]);
    let work = commit(&s.site, "quick.txt", "quick\n");
    git(&s.site, &["push", "-q", "origin", "site/quick"]);
    ops::release(&s.m.target_store, &ticket.id, at(NOW), WORKER, false).unwrap();
    ops::add_note(
        &s.m.target_store,
        &ticket.id,
        ulid::Ulid::new(),
        at(NOW),
        NoteKind::Regular,
        format!("## Integration handoff\nBranch: `site/quick`\nCommit: `{work}`"),
    )
    .unwrap();
    let handed = s.join(&mut tx).assignment.unwrap();
    assert_eq!(
        (handed.phase, handed.commit.as_deref()),
        (Phase::Handoff, Some(work.as_str()))
    );
    let kinds: Vec<_> = s.coordinator.state().unwrap().assignments["assignment-1"]
        .transitions
        .iter()
        .map(|t| t.kind)
        .collect();
    assert_eq!(kinds.len(), 3, "offered, accepted, handoff: {kinds:?}");
}

#[test]
fn hs2_5p112z_a_checkout_without_a_pool_joins_nothing() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let m = machine(10, false, unsupported);
    git(Path::new(&m.target.root), &["init", "-q", "-b", "main"]);
    let member = HostedPoolMember {
        worker_id: WORKER.into(),
        generation: GENERATION.into(),
        lease_seconds: 600,
        site_target_id: id(2),
        connection_id: None,
        native_id: None,
        actor: None,
    };
    let reply = m
        .runtime
        .hosted_pool_join(&m.target.id, member.clone(), &mut tx)
        .unwrap();
    assert!(!reply.pool && !reply.joined);
    assert!(
        !Path::new(&m.target.root)
            .join(".git/hotsheet-coordination/state.json")
            .exists()
    );
    // A lease out of bounds is refused before anything is read.
    for lease_seconds in [9, 3601] {
        let refused = m.runtime.hosted_pool_join(
            &m.target.id,
            HostedPoolMember {
                lease_seconds,
                ..member.clone()
            },
            &mut tx,
        );
        assert!(matches!(refused, Err(OrchestrationError::Invalid(_))));
    }
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
        .status
        .success()
}

fn note_ticket(text: &str, kind: NoteKind) -> Ticket {
    let mut ticket = Ticket::default();
    ticket.notes.push(hotsheet_model::Note {
        id: ulid::Ulid::new(),
        kind,
        kind_explicit: true,
        created_at: Timestamp::new(NOW),
        edited_at: Timestamp::new(NOW),
        summary: None,
        confidence: None,
        feedback_for: None,
        feedback_request: false,
        response_to: None,
        ai_feedback: None,
        human_edited: false,
        actor: None,
        text: text.into(),
    });
    ticket
}

#[test]
fn hs2_5p112z_the_handoff_note_and_branch_are_read_strictly() {
    let sha = "a".repeat(40);
    let good = format!("## Integration handoff\nBranch: site/a\nCommit: {sha}");
    assert_eq!(
        handoff_note(&note_ticket(&good, NoteKind::Regular)),
        Some(("site/a".into(), sha.clone()))
    );
    for (case, text, kind) in [
        ("activity note", good.clone(), NoteKind::Activity),
        (
            "other heading",
            format!("## Notes\nBranch: a\nCommit: {sha}"),
            NoteKind::Regular,
        ),
        (
            "short commit",
            "## Integration handoff\nBranch: a\nCommit: abc".into(),
            NoteKind::Regular,
        ),
        (
            "no branch",
            format!("## Integration handoff\nCommit: {sha}"),
            NoteKind::Regular,
        ),
    ] {
        assert_eq!(handoff_note(&note_ticket(&text, kind)), None, "{case}");
    }
    for branch in ["site/a", "hs2-5p112z", "feature/x.y"] {
        assert!(valid_branch(branch), "{branch}");
    }
    for branch in [
        "", "-x", "a..b", "a b", "a:b", "a~1", "a^", "a?", "a*", "a[", "a\\b", "a/", "a.lock",
        "/a", ".a", "a@{1}", "a//b",
    ] {
        assert!(!valid_branch(branch), "{branch:?}");
    }
    // A plain assignment id names its own ref; anything else a digest of it.
    assert_eq!(
        handoff_ref("assignment-1"),
        "refs/hotsheet/site-handoffs/assignment-1"
    );
    assert!(handoff_ref("a/b c").starts_with("refs/hotsheet/site-handoffs/"));
    assert!(!handoff_ref("a/b c").contains(' '));
}

#[test]
fn hs2_5p112z_site_evidence_walks_offered_accepted_and_handoff() {
    let now = at("2026-10-15T01:00:00Z");
    let sha = "b".repeat(40);
    let live = |worker: &str| Ticket {
        claimed_by: Some(worker.into()),
        claim_lease_expires_at: Some(at("2026-10-15T02:00:00Z")),
        ..Ticket::default()
    };
    let note = format!("## Integration handoff\nBranch: site/b\nCommit: {sha}");
    let released = |worker: &str, with_note: bool| {
        let mut ticket = if with_note {
            note_ticket(&note, NoteKind::Regular)
        } else {
            Ticket::default()
        };
        ticket.claim_history.push(hotsheet_model::ClaimEvent {
            id: ulid::Ulid::new(),
            kind: hotsheet_model::ClaimEventKind::Release,
            worker: worker.into(),
            at: now.clone(),
            lease_expires_at: None,
            eta_at: None,
            worker_label: None,
            host: None,
        });
        ticket
    };
    let handoff = |accept_first| SiteEvidence::Handoff {
        branch: "site/b".into(),
        commit: sha.clone(),
        accept_first,
    };
    let other = Ticket {
        id: ulid::Ulid::new(),
        ..live(WORKER)
    };
    let cases: Vec<EvidenceCase<'_>> = vec![
        (
            "offered, unclaimed",
            Phase::Offered,
            Ticket::default(),
            vec![],
            Some(SiteEvidence::Wait("claim_not_taken")),
        ),
        (
            "offered, claimed by another",
            Phase::Offered,
            live("someone"),
            vec![],
            Some(SiteEvidence::Wait("claim_not_taken")),
        ),
        (
            "offered, claimed",
            Phase::Offered,
            live(WORKER),
            vec![],
            Some(SiteEvidence::Accept),
        ),
        (
            "offered, holds another",
            Phase::Offered,
            live(WORKER),
            vec![other],
            Some(SiteEvidence::Wait("worker_holds_another_claim")),
        ),
        (
            "offered, released with note",
            Phase::Offered,
            released(WORKER, true),
            vec![],
            Some(handoff(true)),
        ),
        (
            "offered, released by another",
            Phase::Offered,
            released("someone", true),
            vec![],
            Some(SiteEvidence::Wait("claim_not_taken")),
        ),
        (
            "offered, released without note",
            Phase::Offered,
            released(WORKER, false),
            vec![],
            Some(SiteEvidence::Wait("handoff_note_missing")),
        ),
        (
            "accepted, held",
            Phase::Accepted,
            live(WORKER),
            vec![],
            Some(SiteEvidence::Wait("claim_held")),
        ),
        (
            "accepted, released, no note",
            Phase::Accepted,
            released(WORKER, false),
            vec![],
            Some(SiteEvidence::Wait("handoff_note_missing")),
        ),
        (
            "accepted, released with note",
            Phase::Accepted,
            released(WORKER, true),
            vec![],
            Some(handoff(false)),
        ),
        (
            "handoff already",
            Phase::Handoff,
            released(WORKER, true),
            vec![],
            None,
        ),
        ("done", Phase::Done, Ticket::default(), vec![], None),
    ];
    for (case, phase, ticket, others, expected) in cases {
        assert_eq!(
            site_evidence(phase, &ticket, &others, WORKER, &now),
            expected,
            "{case}"
        );
    }
    let bad = note_ticket(
        &format!("## Integration handoff\nBranch: -x\nCommit: {sha}"),
        NoteKind::Regular,
    );
    assert_eq!(
        site_evidence(Phase::Accepted, &bad, &[], WORKER, &now),
        Some(SiteEvidence::Wait("handoff_branch_invalid"))
    );
}

/// HS2-RCR4A6: after a site worker's release, the home is asked again one second after
/// it, then at doubling intervals up to a minute apart, always strictly in the future.
#[test]
fn hs2_rcr4a6_a_released_handoff_is_asked_about_again_with_a_doubling_interval() {
    use crate::orchestration::assignment::{SITE_EVIDENCE_LAG, site_handoff_retry_at};
    let released = 1_000_000;
    let expected = [
        (released, released + 1_000),
        (released + 999, released + 1_000),
        (released + 1_000, released + 3_000),
        (released + 3_000, released + 7_000),
        (released + 7_000, released + 15_000),
        (released + 63_000, released + 123_000),
        (released + 123_000, released + 183_000),
    ];
    for (now, at) in expected {
        assert_eq!(site_handoff_retry_at(released, now), Some(at), "now {now}");
        assert!(site_handoff_retry_at(released, now).is_some_and(|at| at > now));
    }
    // Only the codes that mean the home has not yet seen what the site published.
    assert!(SITE_EVIDENCE_LAG.contains(&"claim_held"));
    assert!(!SITE_EVIDENCE_LAG.contains(&"handoff_out_of_scope"));
}

/// HS2-RQDH0Q: the timed asks end. Every ask is in the future and no later than the
/// cap, the last one is the cap itself, and from the cap on there is none. A newer
/// release starts the schedule over; the same release never does.
#[test]
fn hs2_rqdh0q_timed_asks_about_a_site_handoff_stop_at_the_cap() {
    use crate::orchestration::assignment::{
        SITE_HANDOFF_GIVE_UP_MS, site_handoff_attention, site_handoff_retry_at,
    };
    use hotsheet_ticketing::orchestration::{Disposition, ExternalSource, Verification};
    let released = 5_000_000;
    let cap = released + SITE_HANDOFF_GIVE_UP_MS;
    assert_eq!(SITE_HANDOFF_GIVE_UP_MS, 30 * 60 * 1000);
    // Walk every ask as a woken pass would: each strictly later, never past the cap.
    let mut now = released;
    let mut asks = 0;
    while let Some(at) = site_handoff_retry_at(released, now) {
        assert!(at > now && at <= cap, "{now} -> {at}");
        now = at;
        asks += 1;
        assert!(asks < 100, "bounded");
    }
    assert_eq!(now, cap, "the last ask is the cap");
    // One second doubling to a minute: 1, 3, 7, 15, 31, 63, then a minute apart.
    assert_eq!(
        asks,
        6 + (SITE_HANDOFF_GIVE_UP_MS - 63_000).div_ceil(60_000)
    );
    for after in [cap, cap + 1, cap + 60_000, u64::MAX] {
        assert_eq!(site_handoff_retry_at(released, after), None);
    }
    // A pass just before the cap is asked again at the cap, not past it.
    assert_eq!(site_handoff_retry_at(released, cap - 1), Some(cap));
    // A newer release starts over; an older one stays given up.
    assert_eq!(site_handoff_retry_at(cap + 10, cap + 10), Some(cap + 1_010));
    assert_eq!(site_handoff_retry_at(released, cap + 10), None);
    // The stored outcome: one finding with the waiting code, at the cap, by source.
    let claim = site_handoff_attention(released, "claim_held");
    assert_eq!(claim.at_ms, cap);
    assert_eq!(claim.disposition, Disposition::NeedsAttention);
    assert_eq!(claim.findings.len(), 1);
    assert_eq!(claim.findings[0].source, ExternalSource::TargetClaim);
    assert_eq!(claim.findings[0].verification, Verification::Mismatched);
    assert_eq!(claim.findings[0].reason.as_deref(), Some("claim_held"));
    let note = site_handoff_attention(released, "handoff_note_missing");
    assert_eq!(note.findings[0].source, ExternalSource::TargetTicket);
    assert!(note.same_facts(&site_handoff_attention(
        released + 1,
        "handoff_note_missing"
    )));
    assert!(!note.same_facts(&claim));
}
