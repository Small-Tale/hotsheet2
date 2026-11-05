//! Worker ticket preferences and requests across the schema-nine upgrade (HS2-6WFCZR).

use hotsheet_model::{Priority, Timestamp, Ulid};
use hotsheet_ticketing::NewTicket;

use super::preferences::{PREFERENCES_SCHEMA, priority_rank};
use super::tests::{NOW, SHA, fixture, register};
use super::worker_view::WorkerView;
use super::*;

fn filters(raw: &[&str]) -> Vec<TicketFilter> {
    raw.iter().map(|f| f.parse().unwrap()).collect()
}

fn stated(
    prefer: &[&str],
    only: &[&str],
    avoid: &[&str],
    request: Option<&str>,
) -> WorkerPreferences {
    WorkerPreferences::stated(
        filters(prefer),
        filters(only),
        filters(avoid),
        request.map(str::to_owned),
    )
    .unwrap()
}

fn register_worker(
    c: &Coordinator,
    id: &str,
    generation: &str,
    now: i64,
    preferences: PreferenceRequest,
) -> Result<()> {
    c.register_full(
        Role::Worker,
        id,
        generation,
        now,
        120,
        ReturnRoute::HumanRelay,
        StrategyRequest::default(),
        preferences,
    )
}

fn state_json(c: &Coordinator) -> serde_json::Value {
    serde_json::from_slice(&fs::read(c.directory.join("state.json")).unwrap()).unwrap()
}

struct Tickets {
    store: FsStore,
    source: String,
}

impl Tickets {
    fn new(c: &Coordinator) -> Self {
        let store = FsStore::open(&c.source).unwrap();
        let source = git_connection_id(&store);
        Self { store, source }
    }

    fn create(
        &self,
        title: &str,
        category: &str,
        priority: Priority,
        tags: &[&str],
    ) -> hotsheet_model::Ticket {
        ops::create(
            &self.store,
            Ulid::new(),
            "HS",
            Timestamp::from_datetime(time::OffsetDateTime::from_unix_timestamp(NOW).unwrap()),
            NewTicket {
                title: title.into(),
                category: category.into(),
                priority,
                tags: tags.iter().map(|t| (*t).to_owned()).collect(),
                up_next: true,
                ..NewTicket::default()
            },
        )
        .unwrap()
    }

    fn edit(
        &self,
        ticket: &hotsheet_model::Ticket,
        change: impl FnOnce(&mut hotsheet_model::Ticket),
    ) {
        let mut current = self.store.read_ticket(&ticket.id).unwrap();
        change(&mut current);
        self.store.write_ticket(&current).unwrap();
    }

    fn assignment(&self, id: &str, worker: &str, ticket: &hotsheet_model::Ticket) -> Assignment {
        let mut a = super::tests::assignment(id, worker, &ticket.id.to_string());
        a.ticket.connection_id = self.source.clone();
        a.slug = ticket.slug.clone();
        a
    }
}

fn slugs(m: &WorkerMatch) -> Vec<String> {
    m.candidates.iter().map(|c| c.slug.clone()).collect()
}

#[test]
fn hs2_6wfczr_filters_parse_display_serialize_and_match_case_insensitively() {
    for (raw, shown) in [
        ("tag:glance", "tag:glance"),
        (" Category:Bug ", "category:bug"),
        ("priority:HIGH", "priority:high"),
        ("tier:t2", "tier:T2"),
    ] {
        let filter: TicketFilter = raw.parse().unwrap();
        assert_eq!(filter.to_string(), shown);
        let encoded = serde_json::to_value(&filter).unwrap();
        assert_eq!(encoded, shown);
        assert_eq!(
            serde_json::from_value::<TicketFilter>(encoded).unwrap(),
            filter
        );
    }
    for bad in [
        "glance",
        "tag:",
        "label:x",
        "priority:urgent",
        "tier:2",
        "tier:TX",
        "tag:a\nb",
    ] {
        assert!(bad.parse::<TicketFilter>().is_err(), "{bad}");
    }
    let ticket = hotsheet_model::Ticket {
        category: "bug".into(),
        priority: Priority::High,
        tags: vec!["Glance".into(), "tier:T2".into()],
        ..hotsheet_model::Ticket::default()
    };
    for (raw, matches) in [
        ("tag:glance", true),
        ("tag:tier", false),
        ("category:BUG", true),
        ("category:feature", false),
        ("priority:high", true),
        ("priority:default", false),
        ("tier:T2", true),
        ("tier:T1", false),
    ] {
        assert_eq!(
            raw.parse::<TicketFilter>().unwrap().matches(&ticket),
            matches,
            "{raw}"
        );
    }
    assert!(priority_rank(Priority::Highest) < priority_rank(Priority::Lowest));
}

#[test]
fn hs2_6wfczr_stated_preferences_normalize_and_reject_contradictions() {
    let prefs = stated(&["tag:b", "tag:a", "tag:a"], &[], &[], Some(" HS-1 "));
    assert_eq!(prefs.prefer, filters(&["tag:a", "tag:b"]));
    assert_eq!(prefs.request.as_deref(), Some("HS-1"));
    assert!(!prefs.is_strict() && !prefs.is_empty());
    assert!(
        WorkerPreferences::stated(filters(&[]), filters(&["tag:x"]), filters(&["tag:x"]), None)
            .unwrap_err()
            .to_string()
            .contains("both --only and --avoid")
    );
    for request in ["", "two words", "a\tb"] {
        assert!(
            WorkerPreferences::stated(Vec::new(), Vec::new(), Vec::new(), Some(request.into()))
                .is_err(),
            "{request:?}"
        );
    }
    let many: Vec<TicketFilter> = (0..33)
        .map(|i| TicketFilter::Tag(format!("t{i}")))
        .collect();
    assert!(WorkerPreferences::stated(many, Vec::new(), Vec::new(), None).is_err());
    assert!(
        WorkerPreferences::stated(Vec::new(), Vec::new(), Vec::new(), None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn hs2_6wfczr_pools_without_preferences_keep_their_schema_and_bytes() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    register(&c, Role::Worker, "worker", "generation-1", NOW);
    // An explicit empty statement and a clear are both "no preferences".
    register_worker(
        &c,
        "worker",
        "generation-1",
        NOW + 1,
        PreferenceRequest::replace(WorkerPreferences::default(), false),
    )
    .unwrap();
    assert_eq!(c.state().unwrap().version, 3);
    let raw = state_json(&c);
    assert!(
        raw["workers"]["worker"].get("preferences").is_none(),
        "{raw}"
    );
    let snapshot = c.snapshot(NOW + 1).unwrap();
    assert!(snapshot.worker_matches.is_empty() && snapshot.unmatched_tickets.is_empty());
}

#[test]
fn hs2_6wfczr_first_preferences_require_an_explicit_upgrade_and_persist_schema_nine() {
    let (_root, c) = fixture();
    register(&c, Role::Worker, "worker", "generation-1", NOW);
    let before = fs::read(c.directory.join("state.json")).unwrap();
    let refused = register_worker(
        &c,
        "worker",
        "generation-1",
        NOW + 1,
        PreferenceRequest::replace(stated(&["tag:glance"], &[], &[], None), false),
    )
    .unwrap_err()
    .to_string();
    assert!(refused.contains("--all-participants-upgraded"), "{refused}");
    // A refused upgrade changes nothing: same bytes, same schema, same lease.
    assert_eq!(fs::read(c.directory.join("state.json")).unwrap(), before);
    register_worker(
        &c,
        "worker",
        "generation-1",
        NOW + 1,
        PreferenceRequest::replace(stated(&["tag:glance"], &[], &[], None), true),
    )
    .unwrap();
    assert_eq!(c.state().unwrap().version, PREFERENCES_SCHEMA);
    assert_eq!(
        state_json(&c)["workers"]["worker"]["preferences"]["prefer"][0],
        "tag:glance"
    );
    // Once on schema nine, another worker needs no upgrade flag.
    register_worker(
        &c,
        "second",
        "generation-1",
        NOW + 1,
        PreferenceRequest::replace(stated(&[], &["category:bug"], &[], None), false),
    )
    .unwrap();
    // Other roles never state preferences.
    let other = c
        .register_full(
            Role::Integrator,
            "integrator",
            "generation-1",
            NOW,
            120,
            ReturnRoute::HumanRelay,
            StrategyRequest::default(),
            PreferenceRequest::replace(stated(&["tag:x"], &[], &[], None), true),
        )
        .unwrap_err()
        .to_string();
    assert!(other.contains("only a worker"), "{other}");
}

#[test]
fn hs2_6wfczr_schema_eight_pool_upgrades_and_fence_refuses_smuggled_preferences() {
    let (_root, c) = fixture();
    // An older schema-eight pool (a non-serial strategy) with an integrator and a plain worker.
    c.register_with_strategy(
        Role::Integrator,
        "integrator",
        "generation-1",
        NOW,
        120,
        ReturnRoute::HumanRelay,
        StrategyRequest {
            requested: Some(IntegrationStrategy::Batch),
            default: IntegrationStrategy::Serial,
            all_participants_upgraded: true,
        },
    )
    .unwrap();
    register(&c, Role::Worker, "plain", "generation-1", NOW);
    assert_eq!(c.state().unwrap().version, 8);
    // Preferences written by hand below schema nine are refused, not silently dropped.
    let path = c.directory.join("state.json");
    let original = fs::read(&path).unwrap();
    let mut raw = state_json(&c);
    raw["workers"]["plain"]["preferences"] = serde_json::json!({"only": ["tag:glance"]});
    fs::write(&path, serde_json::to_vec_pretty(&raw).unwrap()).unwrap();
    let refused = c.state().unwrap_err().to_string();
    assert!(refused.contains("schema nine"), "{refused}");
    fs::write(&path, &original).unwrap();
    register_worker(
        &c,
        "picky",
        "generation-1",
        NOW,
        PreferenceRequest::replace(stated(&[], &["tag:glance"], &[], None), true),
    )
    .unwrap();
    let state = c.state().unwrap();
    assert_eq!(state.version, 9);
    // Mixed participants: the batch integrator and the plain worker keep their records.
    assert_eq!(
        Coordinator::active_strategy(&state),
        IntegrationStrategy::Batch
    );
    assert!(state.workers["plain"].preferences.is_empty());
    register(&c, Role::Worker, "plain", "generation-1", NOW + 5);
    assert_eq!(c.state().unwrap().version, 9);
    assert!(
        CAPABILITIES.schemas_read.contains(&9)
            && CAPABILITIES.schema_written_with_worker_preferences == 9
    );
}

#[test]
fn hs2_6wfczr_preferences_replay_across_renewals_and_reset_per_generation() {
    let (_root, c) = fixture();
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    let glance = stated(&["tag:glance"], &[], &["tier:T2"], None);
    register_worker(
        &c,
        "worker",
        "generation-1",
        NOW,
        PreferenceRequest::replace(glance.clone(), true),
    )
    .unwrap();
    // The plain renewal path (lease renewal inside every wait) keeps the set.
    for step in 1..=3 {
        register(&c, Role::Worker, "worker", "generation-1", NOW + step);
        register_worker(
            &c,
            "worker",
            "generation-1",
            NOW + step,
            PreferenceRequest::keep(),
        )
        .unwrap();
        assert_eq!(c.state().unwrap().workers["worker"].preferences, glance);
    }
    // A new statement replaces the whole set.
    let bugs = stated(&["category:bug"], &[], &[], None);
    register_worker(
        &c,
        "worker",
        "generation-1",
        NOW + 4,
        PreferenceRequest::replace(bugs.clone(), false),
    )
    .unwrap();
    assert_eq!(c.state().unwrap().workers["worker"].preferences, bugs);
    // An explicit clear empties it; the schema is never lowered.
    register_worker(
        &c,
        "worker",
        "generation-1",
        NOW + 5,
        PreferenceRequest::replace(WorkerPreferences::default(), false),
    )
    .unwrap();
    let state = c.state().unwrap();
    assert!(state.workers["worker"].preferences.is_empty());
    assert_eq!(state.version, 9);
    // A replacement generation starts without the old generation's preferences.
    register_worker(
        &c,
        "worker",
        "generation-1",
        NOW + 6,
        PreferenceRequest::replace(bugs.clone(), false),
    )
    .unwrap();
    register(&c, Role::Worker, "worker", "generation-2", NOW + 500);
    assert!(c.state().unwrap().workers["worker"].preferences.is_empty());
}

#[test]
fn hs2_6wfczr_strict_filters_bind_assignment_and_prefer_ranks_within_priority() {
    let (_root, c) = fixture();
    let t = Tickets::new(&c);
    let glance = t.create("Glance item", "task", Priority::Default, &["glance"]);
    let bug = t.create("Default bug", "bug", Priority::Default, &[]);
    let urgent = t.create("Urgent risky", "feature", Priority::Highest, &["tier:T2"]);
    let low_bug = t.create("Low bug", "bug", Priority::Low, &[]);
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    register_worker(
        &c,
        "soft",
        "generation-1",
        NOW,
        PreferenceRequest::replace(stated(&["category:bug"], &[], &["tier:T2"], None), true),
    )
    .unwrap();
    register_worker(
        &c,
        "strict",
        "generation-1",
        NOW,
        PreferenceRequest::replace(stated(&[], &["tag:glance"], &[], None), false),
    )
    .unwrap();
    let snapshot = c.snapshot(NOW).unwrap();
    // Priority first; the preferred bug outranks the same-priority glance ticket; T2 avoided.
    assert_eq!(
        slugs(&snapshot.worker_matches["soft"]),
        [bug.slug.clone(), glance.slug.clone(), low_bug.slug.clone()]
    );
    assert!(snapshot.worker_matches["soft"].candidates[0].preferred);
    assert_eq!(
        slugs(&snapshot.worker_matches["strict"]),
        std::slice::from_ref(&glance.slug)
    );
    assert!(snapshot.worker_matches["strict"].idle_reason.is_none());
    // The highest-priority ticket is admitted by no idle worker: the starvation signal.
    assert_eq!(
        snapshot.unmatched_tickets,
        std::slice::from_ref(&urgent.slug)
    );
    // Strict filters are enforced at the reservation boundary, whoever assigns.
    let refused = c
        .assign(
            "integrator",
            "generation-1",
            NOW,
            t.assignment("op-urgent", "soft", &urgent),
        )
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("strict preferences") && refused.contains("avoiding tier:T2"),
        "{refused}"
    );
    let refused = c
        .assign(
            "integrator",
            "generation-1",
            NOW,
            t.assignment("op-bug", "strict", &bug),
        )
        .unwrap_err()
        .to_string();
    assert!(refused.contains("only tag:glance"), "{refused}");
    c.assign(
        "integrator",
        "generation-1",
        NOW,
        t.assignment("op-glance", "strict", &glance),
    )
    .unwrap();
    // Soft preference never refuses: an unpreferred eligible ticket is still assignable.
    c.assign(
        "integrator",
        "generation-1",
        NOW,
        t.assignment("op-low", "soft", &low_bug),
    )
    .unwrap();
}

#[test]
fn hs2_6wfczr_strict_worker_with_no_match_idles_with_an_explicit_reason() {
    let (_root, c) = fixture();
    let t = Tickets::new(&c);
    let blocked = t.create("Blocked glance", "task", Priority::Default, &["glance"]);
    t.edit(&blocked, |ticket| {
        ticket.blocked_reason = Some("Waiting on HS-9".into())
    });
    t.create("Other", "task", Priority::Default, &[]);
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    register_worker(
        &c,
        "strict",
        "generation-1",
        NOW,
        PreferenceRequest::replace(stated(&[], &["tag:glance"], &[], None), true),
    )
    .unwrap();
    let snapshot = c.snapshot(NOW).unwrap();
    let matching = &snapshot.worker_matches["strict"];
    assert!(matching.candidates.is_empty());
    let reason = matching.idle_reason.as_deref().unwrap();
    assert!(
        reason.contains("only tag:glance")
            && reason.contains(&blocked.slug)
            && reason.contains("1 matching not ready"),
        "{reason}"
    );
    // No silent fallback: the other ticket is not offered to it, and is reported unmatched.
    assert_eq!(snapshot.unmatched_tickets.len(), 1);
    // Unblocking the matching ticket clears the reason and offers it.
    t.edit(&blocked, |ticket| ticket.blocked_reason = None);
    let snapshot = c.snapshot(NOW).unwrap();
    assert_eq!(
        slugs(&snapshot.worker_matches["strict"]),
        std::slice::from_ref(&blocked.slug)
    );
    assert!(snapshot.worker_matches["strict"].idle_reason.is_none());
    // With nothing matching at all, the reason still names the filter.
    t.edit(&blocked, |ticket| ticket.up_next = false);
    let reason = c.snapshot(NOW).unwrap().worker_matches["strict"]
        .idle_reason
        .clone()
        .unwrap();
    assert_eq!(reason, "no ready tickets match only tag:glance");
}

#[test]
fn hs2_6wfczr_request_checks_name_each_refusal_reason() {
    let (_root, c) = fixture();
    let t = Tickets::new(&c);
    let ready = t.create("Ready", "task", Priority::Default, &[]);
    let blocked = t.create("Blocked", "task", Priority::Default, &[]);
    t.edit(&blocked, |ticket| {
        ticket.blocked_reason = Some("waiting".into())
    });
    let claimed = t.create("Claimed", "task", Priority::Default, &[]);
    t.edit(&claimed, |ticket| {
        ticket.claimed_by = Some("someone-else".into());
        ticket.claim_lease_expires_at = Some(Timestamp::from_datetime(
            time::OffsetDateTime::from_unix_timestamp(NOW + 600).unwrap(),
        ));
    });
    let parked = t.create("Not up next", "task", Priority::Default, &[]);
    t.edit(&parked, |ticket| ticket.up_next = false);
    let taken = t.create("Taken", "task", Priority::Default, &[]);
    let risky = t.create("Risky", "task", Priority::Default, &["tier:T2"]);
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    register(&c, Role::Worker, "other", "generation-1", NOW);
    c.assign(
        "integrator",
        "generation-1",
        NOW,
        t.assignment("op-taken", "other", &taken),
    )
    .unwrap();
    for (slug, expected) in [
        (ready.slug.as_str(), vec![]),
        (blocked.slug.as_str(), vec![RefusalReason::Blocked]),
        (claimed.slug.as_str(), vec![RefusalReason::Claimed]),
        (parked.slug.as_str(), vec![RefusalReason::NotReady]),
        ("HS-NOPE", vec![RefusalReason::NotReady]),
        (taken.slug.as_str(), vec![RefusalReason::Assigned]),
        (risky.slug.as_str(), vec![RefusalReason::Filtered]),
    ] {
        register_worker(
            &c,
            "asker",
            "generation-1",
            NOW,
            PreferenceRequest::replace(stated(&[], &[], &["tier:T2"], Some(slug)), true),
        )
        .unwrap();
        let check = c.snapshot(NOW).unwrap().worker_matches["asker"]
            .request
            .clone()
            .unwrap();
        assert_eq!(check.reasons, expected, "{slug}");
        assert_eq!(check.slug, slug);
    }
}

#[test]
fn hs2_6wfczr_honored_request_is_consumed_and_a_second_requester_sees_assigned() {
    let (_root, c) = fixture();
    let t = Tickets::new(&c);
    let wanted = t.create("Wanted", "task", Priority::Default, &[]);
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    for worker in ["first", "second"] {
        register_worker(
            &c,
            worker,
            "generation-1",
            NOW,
            PreferenceRequest::replace(stated(&["tag:x"], &[], &[], Some(&wanted.slug)), true),
        )
        .unwrap();
    }
    c.assign(
        "integrator",
        "generation-1",
        NOW,
        t.assignment("op-1", "first", &wanted),
    )
    .unwrap();
    let state = c.state().unwrap();
    // The honored request is consumed; the rest of the set remains.
    assert_eq!(state.workers["first"].preferences.request, None);
    assert_eq!(
        state.workers["first"].preferences.prefer,
        filters(&["tag:x"])
    );
    let check = c.snapshot(NOW).unwrap().worker_matches["second"]
        .request
        .clone()
        .unwrap();
    assert_eq!(check.reasons, [RefusalReason::Assigned]);
    // Re-registering while assigned restates preferences without touching the assignment.
    register_worker(
        &c,
        "first",
        "generation-1",
        NOW + 1,
        PreferenceRequest::replace(stated(&[], &["category:bug"], &[], None), false),
    )
    .unwrap();
    let state = c.state().unwrap();
    assert_eq!(state.assignments["op-1"].phase, Phase::Offered);
    assert_eq!(state.active_assignment("first").unwrap().id, "op-1");
}

#[test]
fn hs2_6wfczr_refusals_are_authority_checked_idempotent_and_tied_to_the_request() {
    let (_root, c) = fixture();
    let t = Tickets::new(&c);
    let wanted = t.create("Wanted", "task", Priority::Default, &[]);
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    let request = || PreferenceRequest::replace(stated(&[], &[], &[], Some(&wanted.slug)), true);
    register_worker(&c, "worker", "generation-1", NOW, request()).unwrap();
    let refuse = |authority: &str, slug: &str, reason| {
        c.refuse_request(
            authority,
            "generation-1",
            NOW,
            "worker",
            "generation-1",
            slug,
            reason,
            Some("scope crates/a overlaps HS-2".into()),
        )
    };
    assert!(refuse("intruder", &wanted.slug, RefusalReason::Overlap).is_err());
    assert!(
        refuse("integrator", "HS-OTHER", RefusalReason::Overlap)
            .unwrap_err()
            .to_string()
            .contains("does not currently request")
    );
    let first = refuse("integrator", &wanted.slug, RefusalReason::Overlap).unwrap();
    assert_eq!(first.by.role, Role::Integrator);
    let revision = c.state().unwrap().revision;
    assert_eq!(
        refuse(
            "integrator",
            &wanted.slug.to_lowercase(),
            RefusalReason::Overlap
        )
        .unwrap(),
        first
    );
    assert_eq!(
        c.state().unwrap().revision,
        revision,
        "an identical refusal is a no-op"
    );
    let check = c.snapshot(NOW).unwrap().worker_matches["worker"]
        .request
        .clone()
        .unwrap();
    assert_eq!(
        check.refusal.as_ref().unwrap().reason,
        RefusalReason::Overlap
    );
    // Restating the same request keeps the refusal; a renewal keeps it too.
    register_worker(&c, "worker", "generation-1", NOW + 1, request()).unwrap();
    register(&c, Role::Worker, "worker", "generation-1", NOW + 2);
    assert!(
        c.state().unwrap().workers["worker"]
            .preferences
            .refusal
            .is_some()
    );
    // A different request clears it.
    register_worker(
        &c,
        "worker",
        "generation-1",
        NOW + 3,
        PreferenceRequest::replace(stated(&[], &[], &[], Some("HS-OTHER")), false),
    )
    .unwrap();
    assert!(
        c.state().unwrap().workers["worker"]
            .preferences
            .refusal
            .is_none()
    );
    // Too long a detail is refused before any write.
    assert!(
        c.refuse_request(
            "integrator",
            "generation-1",
            NOW,
            "worker",
            "generation-1",
            "HS-OTHER",
            RefusalReason::Overlap,
            Some("x".repeat(501)),
        )
        .is_err()
    );
}

#[test]
fn hs2_6wfczr_a_new_refusal_wakes_the_waiting_worker_once() {
    let (_root, c) = fixture();
    let t = Tickets::new(&c);
    let wanted = t.create("Wanted", "task", Priority::Default, &[]);
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    register_worker(
        &c,
        "worker",
        "generation-1",
        NOW,
        PreferenceRequest::replace(stated(&[], &[], &[], Some(&wanted.slug)), true),
    )
    .unwrap();
    let request = WaitRequest {
        role: Role::Worker,
        id: "worker".into(),
        generation: "generation-1".into(),
        lease_seconds: 120,
        return_route: ReturnRoute::HumanRelay,
        after: None,
        strategy: StrategyRequest::default(),
        preferences: PreferenceRequest::keep(),
    };
    let before = c.snapshot(NOW).unwrap();
    assert!(!wait::ready(&request, &before, None));
    c.refuse_request(
        "integrator",
        "generation-1",
        NOW,
        "worker",
        "generation-1",
        &wanted.slug,
        RefusalReason::Claimed,
        None,
    )
    .unwrap();
    let after = c.snapshot(NOW).unwrap();
    assert!(
        wait::ready(&request, &after, None),
        "a new refusal wakes the worker"
    );
    let seen = after.state.workers["worker"].preferences.refusal.clone();
    assert!(
        !wait::ready(&request, &after, seen.as_ref()),
        "a seen refusal does not loop"
    );
}

#[test]
fn hs2_6wfczr_worker_view_reports_preferences_matching_and_unmatched_tickets() {
    let (_root, c) = fixture();
    let t = Tickets::new(&c);
    let glance = t.create("Glance", "task", Priority::Default, &["glance"]);
    let other = t.create("Other", "task", Priority::Default, &[]);
    register(&c, Role::Integrator, "integrator", "generation-1", NOW);
    register_worker(
        &c,
        "worker",
        "generation-1",
        NOW,
        PreferenceRequest::replace(stated(&[], &["tag:glance"], &[], Some(&glance.slug)), true),
    )
    .unwrap();
    register(&c, Role::Worker, "plain", "generation-1", NOW);
    let snapshot = c.snapshot(NOW).unwrap();
    let view = WorkerView::from_snapshot(&snapshot, NOW, &Default::default(), false);
    let row = view.workers.iter().find(|row| row.id == "worker").unwrap();
    assert_eq!(
        row.preferences.as_ref().unwrap().only,
        filters(&["tag:glance"])
    );
    let matching = row.matching.as_ref().unwrap();
    assert_eq!(slugs(matching), std::slice::from_ref(&glance.slug));
    assert!(matching.request.as_ref().unwrap().reasons.is_empty());
    let plain = view.workers.iter().find(|row| row.id == "plain").unwrap();
    assert!(plain.preferences.is_none() && plain.matching.is_none());
    // A plain idle worker admits everything, so nothing is unmatched.
    assert!(view.unmatched_tickets.is_empty());
    let encoded = serde_json::to_value(&view).unwrap();
    assert_eq!(encoded["workers"].as_array().unwrap().len(), 3);
    let _ = (other, SHA);
}
