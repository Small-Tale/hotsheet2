//! HS2-HCW7MD: Propose Plan through the shared runtime with a scripted terminal host
//! and real git ticket stores. Synthetic tickets, identities and tools only.
use hotsheet_model::{Status, Ticket};
use hotsheet_ticketing::orchestration::{
    AuditOutcome, PlanningState, ReadinessError, SessionError, SessionRole, TargetAction,
};
use hotsheet_ticketing::{NewTicket, TicketPatch, ops};
use serde_json::json;

use super::sessions::{Probe, ScriptedHost, roomy};
use super::tickets::{Machine, NOW, bind_and_grant, human, machine, unsupported, worker};
use super::*;

struct Fixture {
    m: Machine,
    host: Arc<ScriptedHost>,
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
            .with_capacity_probe(probe)
            .with_session_host(host.clone());
        for n in 1..=targets {
            bind_and_grant(&m, n, &[TargetAction::TicketsRead], tx);
        }
        Self { m, host }
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
        planner_prompt_text(&owner, &worker_id, &first.proposal_id, None)
    );
    for part in [
        "`hotsheet-planner`",
        owner.as_str(),
        first.proposal_id.as_str(),
        "every open goal ticket",
        "Do not dispatch runs, start sessions or change the policy.",
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
    // Every goal is closed: nothing to plan.
    let closed = f.goal("Done already");
    f.close(&closed);
    assert_eq!(code(f.propose(json!({}), &mut tx)), "plan_no_goals");
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
