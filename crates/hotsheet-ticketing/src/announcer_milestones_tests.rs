//! HS2-G3HJZH: ticket writes into announcer milestones, coalescing, rule scores, and lines.

use hotsheet_model::{
    AttachmentActorRole, Note, NoteActor, NoteKind, ProjectTicketRef, StartedPhase, Status, Ticket,
    Timestamp, Ulid,
};
use time::format_description::well_known::Rfc3339;

use super::*;

fn at(text: &str) -> OffsetDateTime {
    OffsetDateTime::parse(text, &Rfc3339).unwrap()
}

fn since() -> OffsetDateTime {
    at("2026-11-28T10:00:00Z")
}

fn actor(role: AttachmentActorRole, id: &str) -> Option<NoteActor> {
    Some(NoteActor {
        role,
        id: Some(id.into()),
    })
}

fn note(kind: NoteKind, at: &str, text: &str, summary: Option<&str>) -> Note {
    Note {
        id: Ulid::new(),
        kind,
        created_at: Timestamp::new(at),
        edited_at: Timestamp::new(at),
        summary: summary.map(str::to_owned),
        confidence: None,
        feedback_for: None,
        kind_explicit: true,
        feedback_request: kind == NoteKind::FeedbackNeeded,
        response_to: None,
        ai_feedback: None,
        human_edited: false,
        actor: actor(AttachmentActorRole::Ai, "claude-4b58913f"),
        text: text.to_owned(),
    }
}

fn ticket(status: Status) -> Ticket {
    Ticket {
        id: Ulid::from_string("01M8BD1NT8EB65ZK83EE1075MX").unwrap(),
        slug: "HS2-QXVHPB".into(),
        title: "Code organization and documentation".into(),
        status,
        updated_at: Timestamp::new("2026-11-28T10:05:00Z"),
        ..Ticket::default()
    }
}

fn no_targets(_: &ProjectTicketRef) -> Option<FollowUpTarget> {
    None
}

fn kinds(milestones: &[Milestone]) -> Vec<MilestoneKind> {
    milestones.iter().map(|m| m.kind).collect()
}

#[test]
fn hs2_g3hjzh_ai_notes_become_milestones_and_human_notes_do_not() {
    let mut t = ticket(Status::Started);
    let before = MilestoneSnapshot::of(&t);
    t.notes.push(note(
        NoteKind::Regular,
        "2026-11-28T10:01:00Z",
        "## Preliminary thoughts\n\nLots of detail that must never be read.",
        Some("Several files are over our `1000`-line limit."),
    ));
    let mut human = note(
        NoteKind::Regular,
        "2026-11-28T10:02:00Z",
        "hi",
        Some("A person note"),
    );
    human.actor = actor(AttachmentActorRole::Human, "brian");
    t.notes.push(human);
    t.notes.push(note(
        NoteKind::FeedbackNeeded,
        "2026-11-28T10:03:00Z",
        "FEEDBACK NEEDED\n\nWhich accent color?",
        Some("Pick the accent color"),
    ));
    t.notes.push(note(
        NoteKind::Activity,
        "2026-11-28T10:03:00Z",
        "Started phase changed",
        None,
    ));
    let found = ticket_milestones("s1", Some(&before), &t, since(), no_targets);
    assert_eq!(
        kinds(&found),
        [MilestoneKind::InitialThoughts, MilestoneKind::Question]
    );
    let thoughts = &found[0];
    assert_eq!(thoughts.tool, "claude");
    assert_eq!(
        thoughts.summary.as_deref(),
        Some("Several files are over our 1000-line limit")
    );
    assert!(
        thoughts
            .id
            .starts_with("s1:01M8BD1NT8EB65ZK83EE1075MX:initial_thoughts:")
    );
    // The same write observed again yields the same ids, so it is never said twice.
    let again = ticket_milestones("s1", Some(&before), &t, since(), no_targets);
    assert_eq!(again, found);
    // Once seen, nothing is new.
    let seen = MilestoneSnapshot::of(&t);
    assert!(ticket_milestones("s1", Some(&seen), &t, since(), no_targets).is_empty());
}

#[test]
fn hs2_g3hjzh_a_note_without_a_summary_uses_its_first_line_of_prose() {
    let mut t = ticket(Status::Started);
    let before = MilestoneSnapshot::of(&t);
    t.notes.push(note(
        NoteKind::Regular,
        "2026-11-28T10:01:00Z",
        "## Stopping\n\nFirst slice **built**; see [the PR](https://example.invalid/1).",
        None,
    ));
    t.notes.push(note(
        NoteKind::Regular,
        "2026-11-28T10:01:00Z",
        "  \n",
        None,
    ));
    let found = ticket_milestones("s1", Some(&before), &t, since(), no_targets);
    assert_eq!(kinds(&found), [MilestoneKind::Progress]);
    assert_eq!(found[0].summary.as_deref(), Some("Stopping"));
    assert_eq!(
        plain_fact("First slice **built**; see [the PR](https://example.invalid/1)."),
        "First slice built; see the PR"
    );
}

#[test]
fn hs2_g3hjzh_a_first_sighting_counts_only_notes_after_the_seed_time() {
    let mut t = ticket(Status::Started);
    t.notes.push(note(
        NoteKind::Regular,
        "2026-11-28T09:59:59Z",
        "old",
        Some("Old"),
    ));
    t.notes.push(note(
        NoteKind::Regular,
        "2026-11-28T10:00:01.5Z",
        "new",
        Some("New"),
    ));
    let found = ticket_milestones("s1", None, &t, since(), no_targets);
    assert_eq!(kinds(&found), [MilestoneKind::Progress]);
    assert_eq!(found[0].summary.as_deref(), Some("New"));
    // No baseline: a completed ticket first seen is not announced as completed.
    let mut done = ticket(Status::Completed);
    done.claimed_by = Some("codex-1".into());
    assert!(ticket_milestones("s1", None, &done, since(), no_targets).is_empty());
    let unknown = MilestoneSnapshot::default();
    assert!(ticket_milestones("s1", Some(&unknown), &done, since(), no_targets).is_empty());
}

#[test]
fn hs2_g3hjzh_transitions_need_ai_attribution() {
    let open = ticket(Status::NotStarted);
    let before = MilestoneSnapshot::of(&open);
    // Claimed by a worker, with no authored note: the claim attributes it.
    let mut started = ticket(Status::Started);
    started.claimed_by = Some("codex-77".into());
    let found = ticket_milestones("s1", Some(&before), &started, since(), no_targets);
    assert_eq!(kinds(&found), [MilestoneKind::Started]);
    assert_eq!(found[0].tool, "codex");
    // A person's status change writes a human-authored activity note: not narrated.
    let mut by_person = ticket(Status::Completed);
    by_person.claimed_by = Some("claude-1".into());
    let mut activity = note(
        NoteKind::Activity,
        "2026-11-28T10:04:00Z",
        "Status changed",
        None,
    );
    activity.actor = actor(AttachmentActorRole::Human, "brian");
    by_person.notes.push(activity);
    assert!(ticket_milestones("s1", Some(&before), &by_person, since(), no_targets).is_empty());
    // No claim, no note: unattributed.
    let bare = ticket(Status::Started);
    assert!(ticket_milestones("s1", Some(&before), &bare, since(), no_targets).is_empty());
}

#[test]
fn hs2_g3hjzh_status_phase_and_block_transitions() {
    let mut started = ticket(Status::Started);
    started.started_phase = Some(StartedPhase::Working);
    let before = MilestoneSnapshot::of(&started);
    let ai_activity = || {
        note(
            NoteKind::Activity,
            "2026-11-28T10:04:00Z",
            "Phase changed",
            None,
        )
    };
    for (phase, kind) in [
        (StartedPhase::InitialTesting, Some(MilestoneKind::Testing)),
        (StartedPhase::FinalTesting, Some(MilestoneKind::AwaitingCi)),
        (StartedPhase::Integrating, None),
    ] {
        let mut t = started.clone();
        t.started_phase = Some(phase);
        t.notes.push(ai_activity());
        let found = ticket_milestones("s1", Some(&before), &t, since(), no_targets);
        assert_eq!(
            kinds(&found),
            kind.into_iter().collect::<Vec<_>>(),
            "{phase:?}"
        );
    }
    let mut blocked = started.clone();
    blocked.blocked_reason = Some("waiting on `review`".into());
    blocked.notes.push(ai_activity());
    let found = ticket_milestones("s1", Some(&before), &blocked, since(), no_targets);
    assert_eq!(kinds(&found), [MilestoneKind::Blocked]);
    assert_eq!(found[0].summary.as_deref(), Some("waiting on review"));
    // Staying blocked, or leaving a state, is not a milestone.
    let still = MilestoneSnapshot::of(&blocked);
    let mut unblocked = blocked.clone();
    unblocked.notes.push(ai_activity());
    assert!(ticket_milestones("s1", Some(&still), &unblocked, since(), no_targets).is_empty());
    let mut done = started.clone();
    done.status = Status::Completed;
    done.notes.push(ai_activity());
    let found = ticket_milestones("s1", Some(&before), &done, since(), no_targets);
    assert_eq!(kinds(&found), [MilestoneKind::Completed]);
    // Completed to verified is not a second completion.
    let completed = MilestoneSnapshot::of(&done);
    let mut verified = done.clone();
    verified.status = Status::Verified;
    verified.notes.push(ai_activity());
    assert!(ticket_milestones("s1", Some(&completed), &verified, since(), no_targets).is_empty());
}

#[test]
fn hs2_g3hjzh_follow_ups_resolve_and_respect_their_creator() {
    let parent = ticket(Status::Started);
    let before = MilestoneSnapshot::of(&parent);
    let reference = |native: &str| ProjectTicketRef {
        project_id: "p".into(),
        connection_id: "git".into(),
        native_id: native.into(),
    };
    let mut t = parent.clone();
    t.claimed_by = Some("claude-1".into());
    t.follow_ups = vec![reference("a"), reference("b"), reference("c")];
    let resolve = |r: &ProjectTicketRef| match r.native_id.as_str() {
        "a" => Some(FollowUpTarget {
            slug: "HS2-123456".into(),
            title: "Pilot `docs` project".into(),
            created_at: Some(at("2026-11-28T10:02:00Z")),
            created_by_ai: Some(true),
        }),
        "b" => Some(FollowUpTarget {
            slug: "HS2-PERSON".into(),
            title: "Filed by a person".into(),
            created_at: Some(at("2026-11-28T10:02:00Z")),
            created_by_ai: Some(false),
        }),
        _ => None,
    };
    let found = ticket_milestones("s1", Some(&before), &t, since(), resolve);
    assert_eq!(kinds(&found), [MilestoneKind::FollowUp]);
    assert_eq!(
        found[0].follow_ups,
        [FollowUpFacts {
            slug: "HS2-123456".into(),
            title: "Pilot docs project".into()
        }]
    );
    // A first sighting counts a follow-up only when its target is new.
    let older = |_: &ProjectTicketRef| {
        Some(FollowUpTarget {
            slug: "HS2-OLD".into(),
            title: "Old".into(),
            created_at: Some(at("2026-11-27T10:00:00Z")),
            created_by_ai: Some(true),
        })
    };
    assert!(ticket_milestones("s1", None, &t, since(), older).is_empty());
}

fn milestone(kind: MilestoneKind, ticket: &str, key: &str) -> Milestone {
    Milestone {
        id: format!("s1:{ticket}:{}:{key}", kind.as_str()),
        kind,
        store: "s1".into(),
        ticket_id: ticket.into(),
        slug: "HS2-QXVHPB".into(),
        title: "Code organization and documentation".into(),
        tool: "claude".into(),
        summary: None,
        follow_ups: Vec::new(),
    }
}

fn with_summary(mut m: Milestone, summary: &str) -> Milestone {
    m.summary = Some(summary.into());
    m
}

fn with_follow_up(mut m: Milestone, slug: &str, title: &str) -> Milestone {
    m.follow_ups.push(FollowUpFacts {
        slug: slug.into(),
        title: title.into(),
    });
    m
}

#[test]
fn hs2_g3hjzh_coalescing_says_one_write_once() {
    use MilestoneKind::*;
    // Started and initial thoughts: only the thoughts. Another ticket keeps its own order.
    let batch = vec![
        milestone(Started, "t1", "1"),
        milestone(Started, "t2", "1"),
        with_summary(milestone(InitialThoughts, "t1", "2"), "Thoughts"),
    ];
    assert_eq!(kinds(&coalesce(batch)), [Started, InitialThoughts]);
    // A decision note takes the follow-ups created with it.
    let batch = vec![
        with_summary(milestone(Progress, "t1", "n"), "Pilot first"),
        with_follow_up(milestone(FollowUp, "t1", "a"), "HS2-123456", "Pilot"),
        with_follow_up(
            milestone(FollowUp, "t1", "b"),
            "HS2-654321",
            "Full analysis",
        ),
    ];
    let out = coalesce(batch);
    assert_eq!(kinds(&out), [Progress]);
    assert_eq!(out[0].follow_ups.len(), 2);
    // Follow-ups alone join into one line.
    let batch = vec![
        with_follow_up(milestone(FollowUp, "t1", "a"), "HS2-123456", "Pilot"),
        with_follow_up(
            milestone(FollowUp, "t1", "b"),
            "HS2-654321",
            "Full analysis",
        ),
    ];
    let out = coalesce(batch);
    assert_eq!(kinds(&out), [FollowUp]);
    assert_eq!(out[0].follow_ups.len(), 2);
    // A completion takes its note's summary and replaces the test phases.
    let batch = vec![
        milestone(Testing, "t1", "1"),
        with_summary(milestone(Progress, "t1", "n"), "All gates green"),
        milestone(Completed, "t1", "2"),
    ];
    let out = coalesce(batch);
    assert_eq!(kinds(&out), [Completed]);
    assert_eq!(out[0].summary.as_deref(), Some("All gates green"));
    assert!(coalesce(Vec::new()).is_empty());
}

#[test]
fn hs2_g3hjzh_rule_scores_say_what_matters() {
    use MilestoneKind::*;
    let said = |m: &Milestone| rule_score(m) >= SPEAK_THRESHOLD;
    for kind in [
        Question,
        Blocked,
        Completed,
        InitialThoughts,
        FollowUp,
        Progress,
    ] {
        assert!(said(&milestone(kind, "t", "k")), "{kind:?}");
    }
    for kind in [Started, Testing, AwaitingCi] {
        assert!(!said(&milestone(kind, "t", "k")), "{kind:?}");
    }
    let failing = with_summary(
        milestone(Progress, "t", "k"),
        "The browser tests failed on WebKit",
    );
    let routine = with_summary(milestone(Progress, "t", "k"), "Read the code");
    assert!(rule_score(&failing) > rule_score(&routine));
    assert_eq!(
        rule_score(&with_summary(milestone(Question, "t", "k"), "error")),
        100
    );
    let decided = with_follow_up(milestone(Progress, "t", "k"), "HS2-1", "x");
    assert_eq!(
        rule_score(&decided),
        rule_score(&milestone(Progress, "t", "k")) + 10
    );
}

#[test]
fn hs2_g3hjzh_template_lines_follow_the_maintainers_examples() {
    use MilestoneKind::*;
    let thoughts = with_summary(
        milestone(InitialThoughts, "t", "k"),
        "there are a number of files larger than our 1000 max lines limit",
    );
    assert_eq!(
        spoken_line(&thoughts),
        "I started investigating a ticket focusing on Code organization and documentation \
         (QXVHPB). My initial thoughts: there are a number of files larger than our 1000 max \
         lines limit."
    );
    assert_eq!(
        shown_line(&thoughts),
        "Started **HS2-QXVHPB** Code organization and documentation. Initial thoughts: there are \
         a number of files larger than our 1000 max lines limit"
    );
    let decision = with_follow_up(
        with_summary(
            milestone(Progress, "t", "k"),
            "Pilot first, then the full repo",
        ),
        "HS2-123456",
        "Pilot documentation project",
    );
    assert_eq!(
        spoken_line(&decision),
        "An update on Code organization and documentation (QXVHPB): Pilot first, then the full \
         repo. I've created a new ticket (123456) for Pilot documentation project."
    );
    assert_eq!(
        shown_line(&decision),
        "**HS2-QXVHPB** Code organization and documentation: Pilot first, then the full repo. \
         Follow-up: **HS2-123456** Pilot documentation project"
    );
    let question = with_summary(milestone(Question, "t", "k"), "Which color preferences?");
    assert_eq!(
        spoken_line(&question),
        "I have a question about Code organization and documentation (QXVHPB), when you have a \
         moment: Which color preferences?"
    );
    let many = (1..=5).fold(milestone(FollowUp, "t", "k"), |m, n| {
        with_follow_up(m, &format!("HS2-00000{n}"), &format!("Part {n}"))
    });
    assert_eq!(
        spoken_line(&many),
        "While working on Code organization and documentation (QXVHPB), I've created 5 new \
         tickets: 000001 for Part 1, 000002 for Part 2, 000003 for Part 3, and 2 more."
    );
    assert_eq!(
        shown_line(&with_follow_up(
            milestone(FollowUp, "t", "k"),
            "HS2-1",
            "A*b"
        )),
        "**HS2-QXVHPB** Code organization and documentation: created follow-up **HS2-1** A\\*b"
    );
    let lines = [
        (
            Started,
            None,
            "I started working on Code organization and documentation (QXVHPB).",
        ),
        (
            Testing,
            None,
            "I'm testing Code organization and documentation (QXVHPB).",
        ),
        (
            AwaitingCi,
            None,
            "I've pushed Code organization and documentation (QXVHPB) and I'm waiting on CI.",
        ),
        (
            Completed,
            None,
            "I finished Code organization and documentation (QXVHPB).",
        ),
        (
            Completed,
            Some("all gates green"),
            "I finished Code organization and documentation (QXVHPB). All gates green.",
        ),
        (
            Blocked,
            None,
            "I'm blocked on Code organization and documentation (QXVHPB).",
        ),
        (
            Blocked,
            Some("waiting on review"),
            "I'm blocked on Code organization and documentation (QXVHPB): waiting on review.",
        ),
        (
            Progress,
            None,
            "I made progress on Code organization and documentation (QXVHPB).",
        ),
    ];
    for (kind, summary, expected) in lines {
        let mut m = milestone(kind, "t", "k");
        m.summary = summary.map(str::to_owned);
        assert_eq!(spoken_line(&m), expected, "{kind:?}");
        assert!(shown_line(&m).contains("**HS2-QXVHPB**"), "{kind:?}");
    }
}

#[test]
fn hs2_g3hjzh_identity_helpers() {
    assert_eq!(short_slug("HS2-QXVHPB"), "QXVHPB");
    assert_eq!(short_slug("PLAIN"), "PLAIN");
    assert_eq!(short_slug("HS2-"), "HS2-");
    assert_eq!(tool_of("claude-4b58913f-4ec4"), "claude");
    assert_eq!(tool_of("Codex:worker"), "codex");
    assert_eq!(tool_of("brian"), "agent");
    assert_eq!(tool_of(""), "agent");
    let long = "word ".repeat(80);
    let bounded = plain_fact(&long);
    assert!(bounded.chars().count() <= MAX_FACT_CHARS + 1);
    assert!(bounded.ends_with('…'));
}

#[test]
fn hs2_g3hjzh_events_carry_the_structured_and_spoken_lines() {
    let m = with_summary(
        milestone(MilestoneKind::Question, "01TICKET", "k"),
        "Pick a color",
    );
    let decision = Decision {
        score: 95,
        scored_by: "rules",
        spoken: spoken_line(&m),
        worded_by: "template",
    };
    let event = milestone_event(&m, &decision, "01EVENT", "2026-11-28T10:00:00Z");
    assert_eq!(event.kind, ActivityKind::Blocked);
    assert_eq!(event.tool, "claude");
    assert_eq!(event.ticket.as_deref(), Some("01TICKET"));
    assert_eq!(event.importance, Importance::High);
    assert_eq!(event.summary, shown_line(&m));
    assert_eq!(event.detail["milestone"], "question");
    assert_eq!(event.detail["speak"], true);
    assert_eq!(event.detail["spoken"], decision.spoken);
    assert_eq!(event.detail["scored_by"], "rules");
    let quiet = Decision {
        score: 20,
        ..decision.clone()
    };
    let event = milestone_event(&m, &quiet, "01EVENT", "2026-11-28T10:00:00Z");
    assert_eq!(event.importance, Importance::Low);
    assert_eq!(event.detail["speak"], false);
    let middling = Decision {
        score: 40,
        ..decision
    };
    let event = milestone_event(&m, &middling, "01EVENT", "2026-11-28T10:00:00Z");
    assert_eq!(event.importance, Importance::Normal);
}
