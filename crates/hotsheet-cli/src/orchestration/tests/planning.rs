//! HS2-HCW7MD: Propose Plan through the shared runtime with a scripted terminal host
//! and real git ticket stores. Synthetic tickets, identities and tools only.
use hotsheet_model::{Status, Ticket};
use hotsheet_ticketing::orchestration::{
    AdmissionStore, AuditOutcome, MachineLimits, PlanUpdateState, PlanningState, PolicyBody,
    ReadinessError, SessionError, SessionFootprint, SessionRole, SetPolicy, TargetAction,
};
use hotsheet_ticketing::{NewTicket, TicketPatch, ops};
use serde_json::json;

use super::sessions::{Probe, ScriptedHost, roomy};
use super::tickets::{Machine, NOW, bind_and_allow, human, machine, unsupported, worker};
use super::*;

struct Fixture {
    m: Machine,
    host: Arc<ScriptedHost>,
    probe: Arc<Probe>,
}

impl Fixture {
    /// One machine, a roomy probe and a scripted terminal host. `targets` declared
    /// projects are bound, and `worker-1` may read their tickets.
    fn new(targets: u128, tx: &mut Transaction<'_>) -> Self {
        let mut m = machine(10, false, unsupported);
        let host = Arc::new(ScriptedHost::default());
        let probe = Arc::new(Probe::default());
        probe.set(|observation| *observation = roomy());
        m.runtime = m
            .runtime
            .clone()
            .with_capacity_probe(probe.clone())
            .with_session_host(host.clone());
        for n in 1..=targets {
            bind_and_allow(&m, n, &[TargetAction::TicketsRead], tx);
        }
        Self { m, host, probe }
    }

    fn goal(&self, title: &str) -> Ticket {
        self.m.orchestration_ticket(title)
    }

    fn close(&self, ticket: &Ticket) {
        ops::update(
            &self.m.owner_store,
            &ticket.id,
            NOW.into(),
            TicketPatch {
                status: Some(Status::Completed),
                ..Default::default()
            },
        )
        .unwrap();
    }

    fn propose(
        &self,
        body: serde_json::Value,
        tx: &mut Transaction<'_>,
    ) -> Result<PlanProposal, OrchestrationError> {
        let mut request = json!({"tool": "claude"});
        request
            .as_object_mut()
            .unwrap()
            .extend(body.as_object().unwrap().clone());
        self.m.runtime.plan_propose(
            &self.m.owner.id,
            serde_json::from_value(request).unwrap(),
            Some(&worker()),
            tx,
        )
    }

    fn status(&self, tx: &mut Transaction<'_>) -> PlanningView {
        self.m.runtime.plan_status(&self.m.owner.id, tx).unwrap()
    }
}

fn code(result: Result<PlanProposal, OrchestrationError>) -> &'static str {
    result.map(|_| ()).unwrap_err().code()
}

/// One press starts one planner in the orchestration project itself, through the
/// session host the drawer uses, with the planner's role prompt; pressing again while
/// it works answers with it, and a press after it ended starts the next one.
#[test]
fn hs2_hcw7md_a_press_starts_one_planner_in_the_orchestration_project() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(1, &mut tx);
    let owner = f.m.owner.id.clone();
    // Nothing planned yet.
    let idle = f.status(&mut tx);
    assert_eq!(idle.state, PlanningState::Idle);
    assert!(idle.session.is_none() && idle.proposal_id.is_none());
    assert!(!idle.placement_automatic);

    let open = f.goal("Ship the release");
    let done = f.goal("Already shipped");
    f.close(&done);
    let first = f.propose(json!({}), &mut tx).unwrap();
    assert_eq!(first.outcome, ProposeOutcome::Started);
    assert_eq!(first.targets, 1);
    assert_eq!(first.goal_count, 1);
    assert_eq!(first.goals[0].native_id, open.id.to_string());
    assert_eq!(first.goals[0].slug, open.slug);
    assert_eq!(first.goals[0].steps, 0);
    assert_eq!(first.tool_setup.map(|setup| setup.wrote), Some(2));
    assert_eq!(first.session.session.role, SessionRole::Planner);
    assert_eq!(first.session.session.checkout_id, owner);
    assert_eq!(first.session.joined, None, "a planner joins no pool");
    assert_eq!(first.role_prompt, Some(RolePrompt::Delivered));
    // Set up for its tool and launched in the orchestration project, labelled as its
    // planner, with the planner's skill, the project, the proposal and its worker id.
    assert_eq!(
        f.host.prepared(),
        [(f.m.owner.root.clone(), "claude".into())]
    );
    let launches = f.host.launches();
    assert_eq!(launches.len(), 1);
    let launch = &launches[0];
    assert_eq!(launch.role, SessionRole::Planner);
    assert_eq!(launch.tool, "claude");
    assert!(launch.label.ends_with(": planner"), "{}", launch.label);
    let worker_id = hotsheet_aitools::session_worker_id("claude", &launch.session_id);
    assert_eq!(
        launch.role_prompt,
        planner_prompt_text(&owner, &worker_id, &first.proposal_id, None, None, false)
    );
    for part in [
        "`hotsheet-planner`",
        owner.as_str(),
        first.proposal_id.as_str(),
        "every open goal ticket",
        "Do not dispatch runs, start sessions or change the policy.",
        // HS2-N5BDE4: it asks instead of stopping.
        "ask the person with a FEEDBACK NEEDED note on the goal ticket instead of stopping",
    ] {
        assert!(launch.role_prompt.contains(part), "prompt lacks {part}");
    }
    assert_eq!(
        f.host.pushes(),
        [(launch.session_id.clone(), launch.role_prompt.clone())]
    );
    assert_eq!(
        f.m.last_audit(),
        (
            "plan_propose".into(),
            AuditOutcome::Succeeded,
            Some("plan_started".into())
        )
    );

    // HS2-FDS95D: it works on its turn now.
    f.host.set_busy(&launch.session_id, true);
    // Pressed again, with no id, a new id, or the same id: the same planner answers
    // and nothing launches twice.
    for body in [
        json!({}),
        json!({"operation_id": "another-press"}),
        json!({"operation_id": first.proposal_id}),
        json!({"tool": "codex"}),
    ] {
        let again = f.propose(body, &mut tx).unwrap();
        assert_eq!(again.outcome, ProposeOutcome::Existing);
        assert_eq!(again.proposal_id, first.proposal_id);
        assert_eq!(again.session.session.tool, "claude");
    }
    assert_eq!(f.host.launches().len(), 1);
    assert_eq!(
        f.m.last_audit(),
        (
            "plan_propose".into(),
            AuditOutcome::Succeeded,
            Some("plan_existing".into())
        )
    );
    let planning = f.status(&mut tx);
    assert_eq!(planning.state, PlanningState::Planning);
    assert_eq!(
        planning.proposal_id.as_deref(),
        Some(first.proposal_id.as_str())
    );
    assert_eq!(planning.goal_count, 1);
    assert_eq!(planning.steps, 0);

    // It finished: its terminal is gone. Planning has ended, and the plan stands.
    f.host.crash(&launch.session_id);
    let ended = f.status(&mut tx);
    assert_eq!(ended.state, PlanningState::Ended);
    assert_eq!(
        ended.proposal_id.as_deref(),
        Some(first.proposal_id.as_str())
    );
    assert_eq!(
        ended.session.as_ref().and_then(|view| view.alive),
        Some(false)
    );

    // The old id still names the old planner; a new press starts the next one.
    let old = f
        .propose(json!({"operation_id": first.proposal_id}), &mut tx)
        .unwrap();
    assert_eq!(old.outcome, ProposeOutcome::Existing);
    assert!(old.session.session.ended_at_ms.is_some());
    let second = f.propose(json!({"tool": "codex"}), &mut tx).unwrap();
    assert_eq!(second.outcome, ProposeOutcome::Started);
    assert_ne!(second.proposal_id, first.proposal_id);
    assert_eq!(second.session.session.tool, "codex");
    assert_eq!(f.host.launches().len(), 2);
    assert_eq!(f.status(&mut tx).proposal_id, Some(second.proposal_id));
}

/// Nothing launches when there is nothing to plan, without an identified actor, or
/// for a goal that is not open; a host that cannot launch leaves nothing reserved.
#[test]
fn hs2_hcw7md_a_press_is_refused_before_anything_launches() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(0, &mut tx);
    f.goal("A goal");
    assert_eq!(code(f.propose(json!({}), &mut tx)), "plan_no_targets");
    assert_eq!(
        f.m.last_audit(),
        (
            "plan_propose".into(),
            AuditOutcome::Failed,
            Some("plan_no_targets".into())
        )
    );
    // No identified actor: refused and not even audited.
    assert!(matches!(
        f.m.runtime.plan_propose(
            &f.m.owner.id,
            serde_json::from_value(json!({"tool": "claude"})).unwrap(),
            None,
            &mut tx,
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    assert_eq!(
        code(f.propose(json!({"tool": "a b"}), &mut tx)),
        "invalid_request"
    );

    let f = Fixture::new(1, &mut tx);
    // A named goal that is closed is not planned. (Every goal closed plans the
    // default goal instead, HS2-YSRT6Y.)
    let closed = f.goal("Done already");
    f.close(&closed);
    assert_eq!(
        code(f.propose(json!({"goal": {"native_id": closed.slug}}), &mut tx)),
        "plan_goal_not_open"
    );
    assert_eq!(ReadinessError::GoalNotOpen.code(), "plan_goal_not_open");
    assert!(f.host.launches().is_empty());

    // The host cannot launch: the press fails and leaves nothing behind, so the same
    // press works once the host is back.
    let goal = f.goal("Ship it");
    f.host.down.store(true, Ordering::SeqCst);
    assert_eq!(
        code(f.propose(json!({"operation_id": "press-1"}), &mut tx)),
        SessionError::HostUnavailable.code()
    );
    f.host.down.store(false, Ordering::SeqCst);
    f.host.setup_failing.store(true, Ordering::SeqCst);
    assert_eq!(
        code(f.propose(json!({"operation_id": "press-1"}), &mut tx)),
        SessionError::StartFailed.code()
    );
    f.host.setup_failing.store(false, Ordering::SeqCst);
    assert!(f.host.launches().is_empty());
    assert_eq!(f.status(&mut tx).state, PlanningState::Idle);
    // One named goal: the planner's prompt names it by its slug.
    let started = f
        .propose(
            json!({"operation_id": "press-1", "goal": {"native_id": goal.slug}}),
            &mut tx,
        )
        .unwrap();
    assert_eq!(started.outcome, ProposeOutcome::Started);
    assert_eq!(started.proposal_id, "press-1");
    assert_eq!(started.goal_count, 1);
    assert_eq!(
        started.session.session.origin.ticket.as_deref(),
        Some(goal.id.to_string().as_str())
    );
    let prompt = &f.host.launches()[0].role_prompt;
    assert!(
        prompt.contains(&format!("the goal ticket `{}`", goal.slug)),
        "{prompt}"
    );
}

/// The planner is never a target session, and a hosted planner runs only in the
/// orchestration project it plans for.
#[test]
fn hs2_hcw7md_a_planner_never_starts_in_a_target() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(1, &mut tx);
    let refused = f.m.runtime.session_start(
        &f.m.owner.id,
        serde_json::from_value(json!({"target_id": id(1), "operation_id": "op-p",
            "tool": "claude", "role": "planner"}))
        .unwrap(),
        Some(&human()),
        &mut tx,
    );
    assert!(
        matches!(refused, Err(OrchestrationError::Invalid(_))),
        "{refused:?}"
    );
    let hosted: HostedSessionStart = serde_json::from_value(json!({"session": {
        "operation_id": "op-h", "tool": "claude", "role": "planner",
        "origin": {"owner_checkout_id": f.m.owner.id, "project": "Launch"}}}))
    .unwrap();
    assert!(matches!(
        f.m.runtime
            .hosted_session_start(&f.m.target.id, hosted, &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));
    assert!(f.host.launches().is_empty());
}

/// A step records the planner's preferred tool; the same definition again changes
/// nothing, and another tool is the next revision. The goal's steps are counted.
#[test]
fn hs2_hcw7md_a_step_keeps_the_preferred_tool_and_goals_count_their_steps() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(1, &mut tx);
    let goal = f.goal("Ship it");
    let target = ops::create(
        &f.m.target_store,
        ulid::Ulid::new(),
        "TGT",
        NOW.into(),
        NewTicket {
            title: "Work".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let define = |tool: Option<&str>, tx: &mut Transaction<'_>| {
        f.m.runtime
            .readiness_define(
                &f.m.owner.id,
                serde_json::from_value(json!({
                    "ticket": {"native_id": goal.slug},
                    "target_id": id(1), "connection_id": f.m.target_connection(),
                    "native_id": target.slug, "tool": tool,
                }))
                .unwrap(),
                Some(&worker()),
                tx,
            )
            .map(|view| view.readiness.step)
    };
    let first = define(Some("claude"), &mut tx).unwrap();
    assert_eq!((first.tool.as_deref(), first.revision), (Some("claude"), 1));
    let same = define(Some("claude"), &mut tx).unwrap();
    assert_eq!(same.revision, 1);
    let changed = define(Some("codex"), &mut tx).unwrap();
    assert_eq!(
        (changed.tool.as_deref(), changed.revision),
        (Some("codex"), 2)
    );
    let cleared = define(None, &mut tx).unwrap();
    assert_eq!((cleared.tool, cleared.revision), (None, 3));
    assert!(matches!(
        define(Some("not a tool"), &mut tx),
        Err(OrchestrationError::Invalid(_))
    ));
    let status = f.status(&mut tx);
    assert_eq!(status.steps, 1);
    assert_eq!(status.goals.len(), 1);
    assert_eq!(status.goals[0].steps, 1);
    // A closed goal is no longer listed, and its step stays counted in the plan.
    f.close(&goal);
    let status = f.status(&mut tx);
    assert!(status.goals.is_empty());
    assert_eq!(status.steps, 1);
}

/// HS2-5RYXNN: whether a hosted session's tool is ready, for every host answer.
#[test]
fn hs2_5ryxnn_tool_ready_is_known_only_for_a_running_session_whose_tool_reports() {
    let state = |reported| HostSessionState {
        session_id: "t-1".into(),
        busy: false,
        idle_since_ms: None,
        reported,
    };
    for alive in [None, Some(false), Some(true)] {
        for reported in [None, Some(false), Some(true)] {
            let expected = if alive == Some(true) { reported } else { None };
            assert_eq!(
                tool_ready(alive, Some(&state(reported))),
                expected,
                "{alive:?} {reported:?}"
            );
        }
        assert_eq!(tool_ready(alive, None), None, "{alive:?}");
    }
    // Claude and Codex report their session start through their hooks; a plugin that
    // is unknown reports nothing.
    for tool in ["claude", "codex"] {
        assert!(plugin_reports_start(tool, &[]), "{tool}");
    }
    assert!(!plugin_reports_start("no-such-tool", &[]));
}

/// HS2-5RYXNN: a planner whose tool has not got past its first-run questions says so in
/// `plan show` and in the press's answer, through every transition: launched and
/// waiting, answered, waiting again for the next planner, gone, and an unreachable
/// host that cannot say.
#[test]
fn hs2_5ryxnn_a_planner_waiting_on_its_tool_says_so_until_the_tool_reports() {
    let mut rng = Rng(300);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(1, &mut tx);
    f.goal("Ship the release");
    let ready = |tx: &mut Transaction<'_>| f.status(tx).session.and_then(|view| view.tool_ready);
    assert_eq!(ready(&mut tx), None, "no planner yet");
    let first = f.propose(json!({}), &mut tx).unwrap();
    let terminal = first.session.session.session_id.clone();
    // A host that does not say (the scripted default) says nothing.
    assert_eq!(first.session.tool_ready, None);
    // Launched, and its tool has not reported in: it may be on a trust question.
    f.host.report(&terminal, Some(false));
    assert_eq!(ready(&mut tx), Some(false));
    let again = f.propose(json!({}), &mut tx).unwrap();
    assert_eq!(again.outcome, ProposeOutcome::Existing);
    assert_eq!(again.session.tool_ready, Some(false));
    // Still waiting on the next read: nothing changes by itself.
    assert_eq!(ready(&mut tx), Some(false));
    // A person answered in its terminal and the tool reported its start.
    f.host.report(&terminal, Some(true));
    assert_eq!(ready(&mut tx), Some(true));
    // Its terminal is gone: nothing is waiting any more.
    f.host.crash(&terminal);
    let ended = f.status(&mut tx);
    assert_eq!(ended.state, PlanningState::Ended);
    assert_eq!(ended.session.and_then(|view| view.tool_ready), None);
    // The next planner starts out waiting again.
    let second = f.propose(json!({}), &mut tx).unwrap();
    assert_eq!(second.outcome, ProposeOutcome::Started);
    f.host
        .report(&second.session.session.session_id, Some(false));
    assert_eq!(ready(&mut tx), Some(false));
    // A host that cannot be asked cannot say either.
    f.host.down.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(ready(&mut tx), None);
}

/// HS2-FDS95D: a press gives a planner that finished its turn a new proposal through
/// its session push, never a second planner; a planner still working, waiting on its
/// tool, or not yet prompted answers as before; a retained proposal id never pushes
/// twice; an undelivered push records nothing.
#[test]
fn hs2_fds95d_a_press_replans_a_planner_that_finished_its_turn() {
    let mut rng = Rng(400);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(1, &mut tx);
    let owner = f.m.owner.id.clone();
    f.goal("Ship the release");
    let first = f
        .propose(json!({"operation_id": "press-1"}), &mut tx)
        .unwrap();
    assert_eq!(first.outcome, ProposeOutcome::Started);
    let terminal = first.session.session.session_id.clone();
    let worker = first.session.worker_id.clone();
    assert_eq!(f.host.pushes().len(), 1, "the role prompt");

    // Working on its turn: a press answers with it and pushes nothing.
    f.host.set_busy(&terminal, true);
    assert!(!f.status(&mut tx).replannable);
    let working = f
        .propose(json!({"operation_id": "press-2"}), &mut tx)
        .unwrap();
    assert_eq!(working.outcome, ProposeOutcome::Existing);
    assert_eq!(working.proposal_id, "press-1");
    assert_eq!(f.host.pushes().len(), 1);

    // It finished its turn: the press pushes the new proposal into it.
    f.host.set_busy(&terminal, false);
    let idle = f.status(&mut tx);
    assert!(idle.replannable);
    assert_eq!(idle.state, PlanningState::Planning);
    let replanned = f
        .propose(json!({"operation_id": "press-2"}), &mut tx)
        .unwrap();
    assert_eq!(replanned.outcome, ProposeOutcome::Replanned);
    assert_eq!(replanned.proposal_id, "press-2");
    assert_eq!(replanned.role_prompt, Some(RolePrompt::Delivered));
    assert_eq!(replanned.goal_count, 1);
    assert_eq!(replanned.targets, 1);
    assert_eq!(replanned.session.session.proposals, ["press-2"]);
    assert_eq!(replanned.session.session.operation_id, "press-1");
    assert_eq!(f.host.launches().len(), 1, "no second planner");
    assert_eq!(
        f.host.pushes().last().cloned(),
        Some((
            terminal.clone(),
            planner_prompt_text(&owner, &worker, "press-2", None, None, false)
        ))
    );
    assert_eq!(
        f.m.last_audit(),
        (
            "plan_propose".into(),
            AuditOutcome::Succeeded,
            Some("plan_replanned".into())
        )
    );
    assert_eq!(f.status(&mut tx).proposal_id.as_deref(), Some("press-2"));

    // The same press repeated pushes nothing, and each id still names its proposal.
    for (id, answered) in [("press-2", "press-2"), ("press-1", "press-1")] {
        let again = f.propose(json!({"operation_id": id}), &mut tx).unwrap();
        assert_eq!(again.outcome, ProposeOutcome::Existing, "{id}");
        assert_eq!(again.proposal_id, answered, "{id}");
    }
    assert_eq!(f.host.pushes().len(), 2);

    // Its tool waits on a question in its terminal: nothing is pushed.
    f.host.report(&terminal, Some(false));
    assert!(!f.status(&mut tx).replannable);
    let waiting = f
        .propose(json!({"operation_id": "press-3"}), &mut tx)
        .unwrap();
    assert_eq!(waiting.outcome, ProposeOutcome::Existing);
    assert_eq!(waiting.proposal_id, "press-2");
    assert_eq!(f.host.pushes().len(), 2);
    f.host.report(&terminal, Some(true));

    // A push that is not delivered records nothing; the same press later goes through.
    for answer in [
        PushAttempt::NotReady,
        PushAttempt::Unsupported,
        PushAttempt::Rejected,
    ] {
        f.host
            .push_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(answer);
        let refused = f
            .propose(json!({"operation_id": "press-3"}), &mut tx)
            .unwrap();
        assert_eq!(refused.outcome, ProposeOutcome::Existing, "{answer:?}");
        assert_eq!(refused.proposal_id, "press-2", "{answer:?}");
        assert_eq!(refused.session.session.proposals, ["press-2"], "{answer:?}");
    }
    let third = f.propose(json!({}), &mut tx).unwrap();
    assert_eq!(third.outcome, ProposeOutcome::Replanned);
    assert_ne!(third.proposal_id, "press-3");
    assert_eq!(
        third.session.session.proposals,
        ["press-2".to_owned(), third.proposal_id.clone()]
    );

    // It ended: the next press starts a new planner, whose prompt has not reached it
    // yet, so a press answers with it rather than pushing a proposal.
    f.host.crash(&terminal);
    f.host
        .push_answers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push_back(PushAttempt::NotReady);
    let next = f.propose(json!({}), &mut tx).unwrap();
    assert_eq!(next.outcome, ProposeOutcome::Started);
    assert_eq!(next.role_prompt, Some(RolePrompt::Pending));
    assert!(!f.status(&mut tx).replannable);
    let pending = f.propose(json!({}), &mut tx).unwrap();
    assert_eq!(pending.outcome, ProposeOutcome::Existing);
    assert_eq!(pending.proposal_id, next.proposal_id);
    assert_eq!(f.host.launches().len(), 2);
}

/// HS2-30VKV9: the planner is admitted under its project's checked-in machine limits.
/// With five AI sessions already running on the machine (a person's and other
/// agents', none orchestrated), the default cap of 2 refuses the press; the project's
/// `max_sessions: 10` admits it. A stricter machine admission file still refuses.
#[test]
fn hs2_30vkv9_propose_plan_is_admitted_under_the_projects_policy_limits() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(1, &mut tx);
    f.goal("Ship the release");
    f.probe.set(|observation| {
        observation.sessions = Some(
            (0..5)
                .map(|n| SessionFootprint {
                    session_id: format!("person-{n}"),
                    tool: Some("claude".into()),
                    busy: false,
                    on_checkout: Some(false),
                    resident_mb: None,
                })
                .collect(),
        );
    });
    // The policy defaults: refused, and nothing launched or left reserved.
    // HS2-8Z9VHH: the refusal says how many sessions run and what the cap is.
    let refused = f
        .propose(json!({"operation_id": "press-1"}), &mut tx)
        .unwrap_err();
    assert_eq!(refused.code(), SessionError::AdmissionRefused.code());
    let admission = refused.admission_refusal().expect("admission detail");
    assert_eq!(admission.reason, "session_cap_reached");
    assert_eq!(admission.setting.as_deref(), Some("max_sessions"));
    assert_eq!(admission.limit, Some(2));
    assert_eq!(admission.observed, Some(5));
    assert!(f.host.launches().is_empty());

    // A stricter machine admission file refuses even under the project's 10.
    let expected_revision = f.m.runtime.policy(&f.m.owner.id).unwrap().revision;
    f.m.runtime
        .set_policy(
            &f.m.owner.id,
            SetPolicy {
                expected_revision,
                policy: PolicyBody {
                    machine: MachineLimits {
                        max_sessions: 10,
                        ..MachineLimits::default()
                    },
                    ..PolicyBody::default()
                },
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let local = AdmissionStore::new(&f.m.home);
    local
        .update(|config| config.max_sessions = Some(5))
        .unwrap();
    assert_eq!(
        code(f.propose(json!({"operation_id": "press-1"}), &mut tx)),
        SessionError::AdmissionRefused.code()
    );
    assert!(f.host.launches().is_empty());

    // Without that file the project's own limits admit the same press.
    local.update(|config| config.max_sessions = None).unwrap();
    let started = f
        .propose(json!({"operation_id": "press-1"}), &mut tx)
        .unwrap();
    assert_eq!(started.outcome, ProposeOutcome::Started);
    assert_eq!(f.host.launches().len(), 1);
}

impl Fixture {
    fn update(&self, tx: &mut Transaction<'_>) -> Result<PlanUpdateView, OrchestrationError> {
        self.m.runtime.plan_update(
            &self.m.owner.id,
            serde_json::from_value(json!({"tool": "claude"})).unwrap(),
            Some(&worker()),
            tx,
        )
    }
    fn update_id(
        &self,
        tx: &mut Transaction<'_>,
    ) -> Option<(String, PlanUpdateState, Option<String>)> {
        self.status(tx)
            .update
            .map(|update| (update.update_id, update.state, update.reason))
    }
}

fn readiness_code<T: std::fmt::Debug>(result: Result<T, OrchestrationError>) -> &'static str {
    match result {
        Err(error) => error.code(),
        Ok(value) => panic!("expected a refusal, got {value:?}"),
    }
}

/// HS2-190RDX: one plan update at a time. Update Plan starts the planner with a
/// prompt that names the draft; a second press answers with the drafting update and
/// launches nothing; the planner's tab ending first fails the update without
/// touching the plan; cancel forgets the draft and its later writes are refused; a
/// planner still at work on another proposal cannot take an update.
#[test]
fn hs2_190rdx_one_update_at_a_time_cancel_and_a_planner_that_ends_first() {
    let mut rng = Rng(700);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(1, &mut tx);
    let owner = f.m.owner.id.clone();
    assert!(f.status(&mut tx).update.is_none());
    f.goal("Ship the release");

    let first = f.update(&mut tx).unwrap();
    assert_eq!(
        (first.state, first.outcome),
        (PlanUpdateState::Drafting, Some(UpdateOutcome::Started))
    );
    let session = first.session.clone().unwrap();
    assert_eq!(first.planner.as_deref(), Some(first.update_id.as_str()));
    assert_eq!(f.host.launches().len(), 1);
    // The launch prompt names the draft to write and how to finish.
    let worker_id = session.worker_id.clone();
    let prompt = planner_prompt_text(&owner, &worker_id, &first.update_id, None, None, true);
    assert!(prompt.contains(&format!("--plan-update {}", first.update_id)));
    assert!(prompt.contains("update-finish"));
    assert_eq!(
        f.m.last_audit(),
        (
            "plan_update".into(),
            AuditOutcome::Succeeded,
            Some("plan_update_started".into())
        )
    );

    // A second press, under another id, answers with the drafting one.
    let second =
        f.m.runtime
            .plan_update(
                &owner,
                serde_json::from_value(json!({"tool": "codex", "operation_id": "other"})).unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(second.outcome, Some(UpdateOutcome::Existing));
    assert_eq!(second.update_id, first.update_id);
    assert_eq!(f.host.launches().len(), 1, "no second planner");

    // Cancel: the draft is forgotten, the plan is unchanged, later writes refused.
    let cancelled =
        f.m.runtime
            .plan_update_cancel(&owner, &PlanUpdateRef::default(), Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(cancelled.state, PlanUpdateState::Cancelled);
    assert_eq!(cancelled.reason.as_deref(), Some("plan_update_cancelled"));
    assert_eq!(
        readiness_code(f.m.runtime.readiness_plan_of(
            &owner,
            &PlanQuery {
                plan_update: Some(first.update_id.clone()),
            },
            &mut tx,
        )),
        "plan_update_not_open"
    );
    assert_eq!(
        readiness_code(f.m.runtime.plan_update_cancel(
            &owner,
            &PlanUpdateRef::default(),
            Some(&worker()),
            &mut tx,
        )),
        "plan_update_not_open"
    );
    assert_eq!(
        readiness_code(f.m.runtime.readiness_plan_of(
            &owner,
            &PlanQuery {
                plan_update: Some("unknown".into()),
            },
            &mut tx,
        )),
        "plan_update_not_found"
    );

    // The open planner finished its turn: a new update replans it, not a new one.
    let terminal = session.session.session_id.clone();
    f.host.set_busy(&terminal, false);
    let third = f.update(&mut tx).unwrap();
    assert_eq!(third.outcome, Some(UpdateOutcome::Started));
    assert_ne!(third.update_id, first.update_id);
    assert_eq!(f.host.launches().len(), 1);
    assert_eq!(
        f.host.pushes().last().cloned(),
        Some((
            terminal.clone(),
            planner_prompt_text(&owner, &worker_id, &third.update_id, None, None, true)
        ))
    );

    // Its tab is closed before it finished: the update fails, the plan stands.
    f.host.crash(&terminal);
    let failed = f.update_id(&mut tx).unwrap();
    assert_eq!(
        failed,
        (
            third.update_id.clone(),
            PlanUpdateState::Failed,
            Some("plan_update_planner_ended".into())
        )
    );
    assert_eq!(
        readiness_code(f.m.runtime.plan_update_finish(
            &owner,
            &PlanUpdateRef::default(),
            Some(&worker()),
            &mut tx,
        )),
        "plan_update_not_open"
    );

    // A planner still at work on a proposal cannot take an update: it fails at once.
    let busy = f.propose(json!({}), &mut tx).unwrap();
    f.host.set_busy(&busy.session.session.session_id, true);
    assert_eq!(
        readiness_code(f.update(&mut tx)),
        "plan_update_planner_busy"
    );
    let refused = f.update_id(&mut tx).unwrap();
    assert_eq!(refused.1, PlanUpdateState::Failed);
    assert_eq!(refused.2.as_deref(), Some("plan_update_planner_busy"));
    // An update needs an identified actor.
    assert!(matches!(
        f.m.runtime.plan_update(
            &owner,
            serde_json::from_value(json!({"tool": "claude"})).unwrap(),
            None,
            &mut tx,
        ),
        Err(OrchestrationError::Invalid(_))
    ));
}

/// HS2-N5BDE4: a planner that cannot settle a choice asks the person instead of doing
/// nothing. Its `FEEDBACK NEEDED` note on a goal ticket shows in `plan show` as a
/// question naming the goal and the note, while the planner is still listed; the
/// person's answer settles it, and the planner's turn can continue.
#[test]
fn hs2_n5bde4_a_planner_question_is_visible_until_it_is_answered() {
    use hotsheet_model::{AttachmentActorRole, NoteActor, NoteKind};
    let mut rng = Rng(800);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(1, &mut tx);
    let goal = f.goal("Ship the release");
    let other = f.goal("Write the notes");
    let proposal = f.propose(json!({}), &mut tx).unwrap();
    let planner = proposal.session.worker_id.clone();
    assert!(f.status(&mut tx).questions.is_empty(), "nothing asked yet");

    // The planner asks on one goal, as its skill says, under its own worker id.
    let question = ulid::Ulid::new();
    ops::add_note_with_metadata(
        &f.m.owner_store,
        &goal.id,
        question,
        NOW.into(),
        NoteKind::FeedbackNeeded,
        ops::NoteMetadataInput {
            kind_explicit: true,
            actor: Some(NoteActor {
                role: AttachmentActorRole::Ai,
                id: Some(planner.clone()),
            }),
            ..Default::default()
        },
        "FEEDBACK NEEDED: Which project owns the sign-on change?\n\nCHOICE: Owner\n- **API.** It plans the endpoint first.\n- **Web.** It plans the page first.".into(),
    )
    .unwrap();
    let asked = f.status(&mut tx);
    assert_eq!(asked.state, PlanningState::Planning);
    assert_eq!(asked.questions.len(), 1, "{:?}", asked.questions);
    assert_eq!(asked.questions[0].slug, goal.slug);
    assert_eq!(asked.questions[0].native_id, goal.id.to_string());
    assert_eq!(asked.questions[0].question, Some(question.to_string()));
    let listed = asked
        .goals
        .iter()
        .find(|listed| listed.native_id == other.id.to_string())
        .unwrap();
    assert_eq!(listed.question, None, "the other goal asked nothing");

    // The person answers it: the question is settled.
    ops::add_note_with_metadata(
        &f.m.owner_store,
        &goal.id,
        ulid::Ulid::new(),
        NOW.into(),
        NoteKind::Regular,
        ops::NoteMetadataInput {
            actor: Some(NoteActor {
                role: AttachmentActorRole::Human,
                id: Some("maintainer".into()),
            }),
            response_to: Some(question.to_string()),
            ..Default::default()
        },
        "API.".into(),
    )
    .unwrap();
    assert!(f.status(&mut tx).questions.is_empty());
}

/// HS2-YSRT6Y: with no explicit goal open, Propose Plan opens the standing default
/// goal once and asks the planner to work through every open target ticket by
/// dependencies, priority, Up Next and claims. An explicit goal takes precedence; when
/// it closes, the default goal applies again without a second one being opened.
#[test]
fn hs2_ysrt6y_no_open_goal_plans_the_default_goal() {
    let mut rng = Rng(900);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(1, &mut tx);
    let owner = f.m.owner.id.clone();
    // Before any press: the implicit default goal shows, nothing is opened yet.
    let idle = f.status(&mut tx);
    assert!(idle.default_goal && idle.goals.is_empty());
    assert!(f.m.owner_store.list_tickets().unwrap().is_empty());

    let first = f
        .propose(json!({"operation_id": "press-1"}), &mut tx)
        .unwrap();
    assert_eq!(first.outcome, ProposeOutcome::Started);
    assert_eq!(first.goal_count, 1);
    let default = first.goals[0].clone();
    assert!(default.default_goal);
    let ticket =
        f.m.owner_store
            .read_ticket(&default.native_id.parse().unwrap())
            .unwrap();
    assert_eq!(ticket.title, DEFAULT_GOAL_TITLE);
    assert_eq!(ticket.tags, [DEFAULT_GOAL_TAG]);
    assert!(ticket.details.contains("blocked_by"));
    // The planner is asked for the default goal, by its slug and its ordering rules.
    let launch = f.host.launches()[0].clone();
    let worker = hotsheet_aitools::session_worker_id("claude", &launch.session_id);
    assert_eq!(
        launch.role_prompt,
        planner_prompt_text(&owner, &worker, "press-1", None, Some(&default.slug), false)
    );
    for part in [
        format!("the default goal `{}`", default.slug),
        "working through all open tickets".into(),
        "explicit `blocked_by` dependencies, implicit dependencies, priority, Up Next and live claims".into(),
    ] {
        assert!(launch.role_prompt.contains(&part), "prompt lacks {part}");
    }
    let shown = f.status(&mut tx);
    assert!(shown.default_goal);
    assert_eq!(shown.goals, std::slice::from_ref(&default));

    // A second press opens no second default goal.
    f.host.set_busy(&launch.session_id, false);
    let again = f
        .propose(json!({"operation_id": "press-2"}), &mut tx)
        .unwrap();
    assert_eq!(again.outcome, ProposeOutcome::Replanned);
    assert_eq!(f.m.owner_store.list_tickets().unwrap().len(), 1);
    assert_eq!(
        f.host.pushes().last().map(|(_, text)| text.clone()),
        Some(planner_prompt_text(
            &owner,
            &worker,
            "press-2",
            None,
            Some(&default.slug),
            false
        ))
    );

    // An explicit goal takes precedence: the default goal is no longer offered.
    let explicit = f.goal("Ship the release");
    let shown = f.status(&mut tx);
    assert!(!shown.default_goal);
    assert_eq!(shown.goals.len(), 1);
    assert_eq!(shown.goals[0].slug, explicit.slug);
    assert!(!shown.goals[0].default_goal);
    let press = f
        .propose(json!({"operation_id": "press-3"}), &mut tx)
        .unwrap();
    assert_eq!(press.goals.len(), 1);
    assert_eq!(press.goals[0].slug, explicit.slug);
    assert_eq!(
        f.host.pushes().last().map(|(_, text)| text.clone()),
        Some(planner_prompt_text(
            &owner, &worker, "press-3", None, None, false
        ))
    );

    // It closes: the standing default goal applies again; nothing new is opened.
    f.close(&explicit);
    let shown = f.status(&mut tx);
    assert!(shown.default_goal);
    assert_eq!(shown.goals, std::slice::from_ref(&default));
    f.propose(json!({"operation_id": "press-4"}), &mut tx)
        .unwrap();
    assert_eq!(f.m.owner_store.list_tickets().unwrap().len(), 2);
}

/// HS2-YSRT6Y: Update Plan with no open goal plans the default goal too, opening it
/// once; closing it and updating again opens a fresh one.
#[test]
fn hs2_ysrt6y_update_plan_without_a_goal_plans_the_default_goal() {
    let mut rng = Rng(910);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(1, &mut tx);
    let first = f.update(&mut tx).unwrap();
    assert_eq!(first.state, PlanUpdateState::Drafting);
    let goals = f.status(&mut tx).goals;
    assert_eq!(goals.len(), 1);
    assert!(goals[0].default_goal);
    let launch = f.host.launches()[0].clone();
    assert!(
        launch
            .role_prompt
            .contains(&format!("the default goal `{}`", goals[0].slug)),
        "{}",
        launch.role_prompt
    );
    assert!(launch.role_prompt.contains("--plan-update"));
    assert_eq!(f.m.owner_store.list_tickets().unwrap().len(), 1);
}
