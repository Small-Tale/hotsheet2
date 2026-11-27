//! HS2-A4RVYA: plan templates through the shared runtime, with a scripted terminal
//! host standing in for the planner (as the planning tests do) and real git ticket
//! stores. Applying opens one goal ticket with the brief and starts the planner once;
//! a repeat is a no-op; the applied work is a snapshot of the template; an AI apply
//! asks through the permission card first. Synthetic templates and tickets only.
use std::collections::BTreeMap;

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    ApplyGoal, ApplyState, AuditOutcome, PermissionAnswer, PlanUpdateState, SHARED_PLAN_PATH,
    StepTicket, TEMPLATE_GOAL_TAG, TEMPLATES_PATH, TargetAction, Template, TemplateApplyStore,
    TemplateState,
};
use hotsheet_ticketing::{NewTicket, ops};
use serde_json::{Value, json};

use super::sessions::{Probe, ScriptedHost, roomy};
use super::tickets::{Machine, NOW, bind_and_allow, human, machine, unsupported, worker};
use super::*;

struct Fixture {
    m: Machine,
    host: Arc<ScriptedHost>,
    _probe: Arc<Probe>,
}

impl Fixture {
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
        Self {
            m,
            host,
            _probe: probe,
        }
    }

    fn owner(&self) -> String {
        self.m.owner.id.clone()
    }

    /// The design's template: an API step, then a Web step on its green CI.
    fn define(&self, name: &str, tx: &mut Transaction<'_>) -> Template {
        let document = self
            .m
            .runtime
            .template_set(
                &self.owner(),
                serde_json::from_value(json!({"templates": [{
                    "name": name,
                    "inputs": [
                        {"key": "feature", "label": "Feature", "kind": "text", "required": true},
                        {"key": "api", "label": "API Project", "kind": "target", "default": id(1)},
                        {"key": "web", "label": "Web Project", "kind": "target", "default": id(2)}
                    ],
                    "goal": {"title": "{{feature}} in API and Web"},
                    "steps": [
                        {"key": "api", "target": "{{api}}", "title": "Add {{feature}} headers"},
                        {"key": "web", "target": "{{web}}", "title": "Show {{feature}} errors",
                         "after": [{"step": "api", "gates": "ci"}]}
                    ]
                }]}))
                .unwrap(),
                Some(&human()),
                tx,
            )
            .unwrap();
        document
            .templates
            .into_iter()
            .find(|template| template.name == name)
            .unwrap()
    }

    fn apply(
        &self,
        template: &Template,
        inputs: Value,
        operation_id: &str,
        actor: &MutationActor,
        tx: &mut Transaction<'_>,
    ) -> Result<TemplateApplyView, OrchestrationError> {
        self.m.runtime.template_apply(
            &self.owner(),
            serde_json::from_value(json!({
                "template_id": template.template_id,
                "inputs": inputs,
                "tool": "claude",
                "operation_id": operation_id,
            }))
            .unwrap(),
            Some(actor),
            tx,
        )
    }

    fn goals(&self) -> Vec<hotsheet_model::Ticket> {
        ops::query(&self.m.owner_store, &ops::TicketQuery::default())
            .unwrap()
            .into_iter()
            .filter(|ticket| ticket.tags.iter().any(|tag| tag == TEMPLATE_GOAL_TAG))
            .collect()
    }
}

fn code<T: std::fmt::Debug>(result: Result<T, OrchestrationError>) -> &'static str {
    result.unwrap_err().code()
}

/// A person's apply opens one goal ticket carrying the brief and starts one planner
/// for it; repeating the apply changes nothing, and the apply is listed and shown.
#[test]
fn hs2_a4rvya_apply_opens_a_goal_and_starts_the_planner_once() {
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    let template = f.define("Feature across API and Web", &mut tx);
    assert_eq!(template.version, 1);
    assert_eq!(template.state, TemplateState::Active);

    let applied = f
        .apply(
            &template,
            json!({"feature": "Rate limits"}),
            "op-1",
            &human(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(applied.outcome.as_deref(), Some("applied"));
    assert_eq!(applied.apply.state, ApplyState::Planning);
    assert_eq!(applied.apply.planning.as_deref(), Some("propose"));
    assert_eq!(applied.apply.proposal_id.as_deref(), Some("op-1"));
    let goal = applied.apply.goal.clone().unwrap();
    assert!(goal.created);
    // One goal ticket, with the brief as its details.
    let goals = f.goals();
    assert_eq!(goals.len(), 1);
    assert_eq!(goals[0].id.to_string(), goal.native_id);
    assert_eq!(Some(goals[0].slug.clone()), goal.slug);
    assert_eq!(goals[0].title, "Rate limits in API and Web");
    for part in [
        "orchestration template **Feature across API and Web**",
        "operation `op-1`",
        "find or create \"Add Rate limits headers\"",
        "after `api` (gates: ci)",
        "## Template brief",
    ] {
        assert!(goals[0].details.contains(part), "{part}");
    }
    // One planner, for that goal, under the apply's operation id.
    let launches = f.host.launches();
    assert_eq!(launches.len(), 1);
    assert!(
        launches[0]
            .role_prompt
            .contains(&format!("the goal ticket `{}`", goals[0].slug)),
        "{}",
        launches[0].role_prompt
    );
    assert!(launches[0].role_prompt.contains("op-1"));
    assert_eq!(
        f.m.last_audit(),
        (
            "template_apply".into(),
            AuditOutcome::Succeeded,
            Some("template_planning".into())
        )
    );

    // Repeated (a retry, a double click): the same apply answers; nothing new.
    f.host.set_busy(&launches[0].session_id, true);
    for _ in 0..3 {
        let again = f
            .apply(
                &template,
                json!({"feature": "Rate limits"}),
                "op-1",
                &human(),
                &mut tx,
            )
            .unwrap();
        assert_eq!(again.outcome.as_deref(), Some("existing"));
        assert_eq!(again.apply.goal, applied.apply.goal);
    }
    assert_eq!(f.goals().len(), 1);
    assert_eq!(f.host.launches().len(), 1);

    // Shown and listed.
    let shown =
        f.m.runtime
            .template_apply_show(
                &f.owner(),
                &ApplyRef {
                    operation_id: "op-1".into(),
                },
            )
            .unwrap();
    assert_eq!(shown.apply.state, ApplyState::Planning);
    let list = f.m.runtime.template_list(&f.owner()).unwrap();
    assert_eq!(list.templates.len(), 1);
    assert_eq!(list.applies.len(), 1);
    assert_eq!(list.applies[0].template_name, "Feature across API and Web");
    assert_eq!(
        code(f.m.runtime.template_apply_show(
            &f.owner(),
            &ApplyRef {
                operation_id: "nope".into()
            }
        )),
        "template_apply_not_found"
    );
    // The planning view lists the new goal like any goal.
    let status = f.m.runtime.plan_status(&f.owner(), &mut tx).unwrap();
    assert!(
        status
            .goals
            .iter()
            .any(|known| known.native_id == goal.native_id)
    );
}

/// Applied work is a snapshot: editing the template later changes neither the goal
/// nor a repeat of the same apply; a new apply uses the new version.
#[test]
fn hs2_a4rvya_applied_work_is_a_snapshot_of_the_template() {
    let mut rng = Rng(200);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    let template = f.define("Feature", &mut tx);
    let first = f
        .apply(
            &template,
            json!({"feature": "Search"}),
            "op-a",
            &human(),
            &mut tx,
        )
        .unwrap();
    let before = f.goals()[0].details.clone();
    assert!(before.contains("version 1"));

    // Edit the template: a new version.
    let mut edited = template.clone();
    edited.steps[0].title = "Index {{feature}} first".into();
    let document =
        f.m.runtime
            .template_set(
                &f.owner(),
                SetTemplates {
                    templates: vec![edited.clone()],
                    expected_revision: None,
                },
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(document.templates[0].version, 2);
    // The same definition again changes nothing.
    let same =
        f.m.runtime
            .template_set(
                &f.owner(),
                SetTemplates {
                    templates: vec![edited],
                    expected_revision: Some(document.revision.clone()),
                },
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(same.revision, document.revision);
    assert_eq!(same.templates[0].version, 2);

    let repeat = f
        .apply(
            &template,
            json!({"feature": "Search"}),
            "op-a",
            &human(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(repeat.outcome.as_deref(), Some("existing"));
    assert_eq!(repeat.apply.snapshot.version, 1);
    assert_eq!(repeat.apply.goal, first.apply.goal);
    assert_eq!(f.goals().len(), 1);
    assert_eq!(f.goals()[0].details, before, "the goal is not rewritten");

    // A new apply uses the template as it is now.
    f.host.crash(&f.host.launches()[0].session_id);
    let second = f
        .apply(
            &template,
            json!({"feature": "Search"}),
            "op-b",
            &human(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(second.apply.snapshot.version, 2);
    let goals = f.goals();
    assert_eq!(goals.len(), 2, "a new goal ticket per apply");
    assert!(
        goals
            .iter()
            .any(|goal| goal.details.contains("\"Index Search first\"")
                && goal.details.contains("version 2"))
    );
}

/// The same operation id with other inputs is refused; bad inputs, an unknown or
/// retired template and an undeclared project are refused before anything opens.
#[test]
fn hs2_a4rvya_apply_refusals_open_nothing() {
    let mut rng = Rng(300);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    let template = f.define("Feature", &mut tx);
    assert_eq!(
        code(f.apply(&template, json!({}), "op-x", &human(), &mut tx)),
        "template_inputs_invalid"
    );
    assert_eq!(
        code(f.apply(
            &template,
            json!({"feature": "X", "api": id(9)}),
            "op-x",
            &human(),
            &mut tx
        )),
        "template_target_unknown"
    );
    let unknown = Template {
        template_id: id(77),
        ..template.clone()
    };
    assert_eq!(
        code(f.apply(&unknown, json!({"feature": "X"}), "op-y", &human(), &mut tx)),
        "template_not_found"
    );
    assert!(f.goals().is_empty());
    assert!(f.host.launches().is_empty());

    f.apply(
        &template,
        json!({"feature": "X"}),
        "op-1",
        &human(),
        &mut tx,
    )
    .unwrap();
    assert_eq!(
        code(f.apply(
            &template,
            json!({"feature": "Y"}),
            "op-1",
            &human(),
            &mut tx
        )),
        "template_apply_conflict"
    );
    assert_eq!(
        f.m.last_audit(),
        (
            "template_apply".into(),
            AuditOutcome::Failed,
            Some("template_apply_conflict".into())
        )
    );
    // Retired: no new apply, while the finished one still answers.
    let retired =
        f.m.runtime
            .template_retire(
                &f.owner(),
                &TemplateRef {
                    template_id: template.template_id.clone(),
                },
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(retired.state, TemplateState::Retired);
    assert_eq!(
        code(f.apply(
            &template,
            json!({"feature": "X"}),
            "op-2",
            &human(),
            &mut tx
        )),
        "template_retired"
    );
    assert_eq!(
        f.apply(
            &template,
            json!({"feature": "X"}),
            "op-1",
            &human(),
            &mut tx
        )
        .unwrap()
        .outcome
        .as_deref(),
        Some("existing")
    );
    assert_eq!(f.goals().len(), 1);
    // No identified actor, or an over-long operation id.
    assert!(matches!(
        f.m.runtime.template_apply(
            &f.owner(),
            serde_json::from_value(
                json!({"template_id": template.template_id, "tool": "claude", "operation_id": "o"})
            )
            .unwrap(),
            None,
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    assert_eq!(
        code(f.apply(
            &template,
            json!({"feature": "X"}),
            &"o".repeat(141),
            &human(),
            &mut tx
        )),
        "invalid_request"
    );
}

/// An AI session's apply asks through the permission card for ticket writes in each
/// project, opens nothing until a person answers, and then goes on under the same
/// operation id. A denial is final for that operation.
#[test]
fn hs2_a4rvya_an_ai_apply_asks_first_and_continues_after_the_answer() {
    let mut rng = Rng(400);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    let template = f.define("Feature", &mut tx);
    let preview =
        f.m.runtime
            .template_preview(
                &f.owner(),
                &serde_json::from_value(json!({"template_id": template.template_id,
                    "inputs": {"feature": "Quotas"}}))
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap();
    assert!(preview.problems.is_empty(), "{:?}", preview.problems);
    assert_eq!(preview.targets.len(), 2);
    assert!(
        preview
            .targets
            .iter()
            .all(|target| target.apply == AccessNeed::Asks
                && target.ticket_writes == AccessNeed::Asks)
    );

    let OrchestrationError::PermissionRequired { request_id } = f
        .apply(
            &template,
            json!({"feature": "Quotas"}),
            "op-ai",
            &worker(),
            &mut tx,
        )
        .unwrap_err()
    else {
        panic!("expected a permission request");
    };
    let shown =
        f.m.runtime
            .template_apply_show(
                &f.owner(),
                &ApplyRef {
                    operation_id: "op-ai".into(),
                },
            )
            .unwrap();
    assert_eq!(shown.apply.state, ApplyState::WaitingPermission);
    assert_eq!(shown.apply.permission_requests.len(), 2);
    assert_eq!(shown.apply.permission_requests[0], request_id);
    assert!(f.goals().is_empty() && f.host.launches().is_empty());
    // Asking again files nothing new.
    assert!(matches!(
        f.apply(
            &template,
            json!({"feature": "Quotas"}),
            "op-ai",
            &worker(),
            &mut tx
        ),
        Err(OrchestrationError::PermissionRequired { .. })
    ));
    let requests =
        f.m.runtime
            .permission_list(&f.owner(), &mut tx)
            .unwrap()
            .requests;
    assert_eq!(requests.len(), 2);

    // A person allows both once; the same call goes on.
    for request in &shown.apply.permission_requests {
        f.m.runtime
            .permission_answer(
                &f.owner(),
                request,
                PermissionAnswer::AllowOnce,
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    }
    let applied = f
        .apply(
            &template,
            json!({"feature": "Quotas"}),
            "op-ai",
            &worker(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(applied.apply.state, ApplyState::Planning);
    assert!(applied.apply.permission_requests.is_empty());
    assert_eq!(f.goals().len(), 1);
    assert_eq!(f.host.launches().len(), 1);

    // Another AI apply, denied: final for its operation, nothing opened.
    f.host.crash(&f.host.launches()[0].session_id);
    let OrchestrationError::PermissionRequired { request_id } = f
        .apply(
            &template,
            json!({"feature": "Other"}),
            "op-deny",
            &worker(),
            &mut tx,
        )
        .unwrap_err()
    else {
        panic!("expected a permission request");
    };
    f.m.runtime
        .permission_answer(
            &f.owner(),
            &request_id,
            PermissionAnswer::Deny,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let denied = f
        .apply(
            &template,
            json!({"feature": "Other"}),
            "op-deny",
            &worker(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(denied.apply.state, ApplyState::Denied);
    assert_eq!(denied.apply.reason.as_deref(), Some("permission_denied"));
    let again = f
        .apply(
            &template,
            json!({"feature": "Other"}),
            "op-deny",
            &worker(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(again.outcome.as_deref(), Some("existing"));
    assert_eq!(f.goals().len(), 1);
    assert_eq!(f.host.launches().len(), 1);
}

/// A planner at work on another proposal leaves the apply waiting with its goal
/// open; a repeat once the planner is free asks it again under a new attempt id. With
/// a plan that already has steps, the apply drafts through Update Plan.
#[test]
fn hs2_a4rvya_a_busy_planner_leaves_the_apply_waiting_and_a_plan_uses_update() {
    let mut rng = Rng(500);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    let template = f.define("Feature", &mut tx);
    // A planner is at work on a person's own goal.
    f.m.orchestration_ticket("Ship the launch");
    let busy = f
        .m
        .runtime
        .plan_propose(
            &f.owner(),
            serde_json::from_value(json!({"tool": "claude", "operation_id": "press-1"})).unwrap(),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let terminal = busy.session.session.session_id.clone();
    f.host.set_busy(&terminal, true);

    let waiting = f
        .apply(
            &template,
            json!({"feature": "Billing"}),
            "op-w",
            &human(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(waiting.apply.state, ApplyState::WaitingPlanner);
    assert_eq!(
        waiting.apply.reason.as_deref(),
        Some("plan_update_planner_busy")
    );
    assert!(waiting.apply.goal.as_ref().is_some_and(|goal| goal.created));
    assert_eq!(f.goals().len(), 1, "the goal is open while it waits");
    assert_eq!(f.host.launches().len(), 1);

    // The planner finished its turn: the repeat gives it the goal as a new proposal.
    f.host.set_busy(&terminal, false);
    let taken = f
        .apply(
            &template,
            json!({"feature": "Billing"}),
            "op-w",
            &human(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(taken.apply.state, ApplyState::Planning);
    assert_eq!(taken.apply.proposal_id.as_deref(), Some("op-w.p1"));
    assert_eq!(taken.apply.attempts, 2);
    assert_eq!(f.goals().len(), 1, "no second goal");
    assert_eq!(f.host.launches().len(), 1, "no second planner");
    let goal_slug = f.goals()[0].slug.clone();
    assert!(
        f.host
            .pushes()
            .last()
            .is_some_and(|(_, prompt)| prompt.contains(&format!("the goal ticket `{goal_slug}`")))
    );

    // Once the live plan has a step, an apply drafts beside it with Update Plan.
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
    f.m.runtime
        .readiness_define(
            &f.owner(),
            serde_json::from_value(json!({
                "ticket": {"native_id": goal_slug},
                "target_id": id(1), "connection_id": f.m.target_connection(),
                "native_id": target.slug,
            }))
            .unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    f.host.crash(&terminal);
    let updating = f
        .apply(
            &template,
            json!({"feature": "Audit"}),
            "op-u",
            &human(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(updating.apply.state, ApplyState::Planning);
    assert_eq!(updating.apply.planning.as_deref(), Some("update"));
    let status = f.m.runtime.plan_status(&f.owner(), &mut tx).unwrap();
    let update = status.update.unwrap();
    assert_eq!(update.update_id, "op-u");
    assert_eq!(update.state, PlanUpdateState::Drafting);
    let launch = f.host.launches().last().cloned().unwrap();
    assert!(launch.role_prompt.contains("--plan-update op-u"));
    let audit_goal = f
        .goals()
        .into_iter()
        .find(|goal| goal.title.starts_with("Audit"))
        .unwrap();
    assert!(
        launch
            .role_prompt
            .contains(&format!("the goal ticket `{}`", audit_goal.slug))
    );
}

/// The pre-run summary: problems keep Apply disabled, the person's own act needs no
/// request, a remembered rule shows as such, and the planning flow is named.
#[test]
fn hs2_a4rvya_preview_names_problems_projects_and_what_asks() {
    let mut rng = Rng(600);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    let template = f.define("Feature", &mut tx);
    let preview = |inputs: Value, choices: Value, tx: &mut Transaction<'_>| {
        f.m.runtime
            .template_preview(
                &f.owner(),
                &serde_json::from_value(json!({"template_id": template.template_id,
                    "inputs": inputs, "choices": choices}))
                .unwrap(),
                Some(&human()),
                tx,
            )
            .unwrap()
    };
    let missing = preview(json!({}), json!({}), &mut tx);
    assert_eq!(
        missing
            .problems
            .iter()
            .map(|problem| (problem.code.as_str(), problem.at.as_str()))
            .collect::<Vec<_>>(),
        [("input_missing", "inputs.feature")]
    );
    let unknown = preview(
        json!({"feature": "X", "web": id(5)}),
        json!({"zzz": "create"}),
        &mut tx,
    );
    assert_eq!(
        unknown
            .problems
            .iter()
            .map(|problem| problem.code.as_str())
            .collect::<Vec<_>>(),
        ["choice_unknown", "target_unknown"]
    );
    f.m.runtime
        .permission_remember(
            &f.owner(),
            serde_json::from_value(json!({"target_id": id(1), "action": "tickets_write"})).unwrap(),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let ok = preview(json!({"feature": "X"}), json!({"web": "create"}), &mut tx);
    // No step names a test profile, so no project's profiles are read.
    assert!(ok.targets.iter().all(|target| target.profiles.is_none()));
    assert!(ok.problems.is_empty());
    assert_eq!(ok.goal_title, "X in API and Web");
    assert_eq!(ok.steps[1].ticket, StepTicket::Create);
    assert_eq!(ok.planning, "propose");
    assert!(!ok.placement_automatic);
    let needs: BTreeMap<String, (AccessNeed, AccessNeed)> = ok
        .targets
        .iter()
        .map(|target| {
            (
                target.target_id.clone(),
                (target.apply, target.ticket_writes),
            )
        })
        .collect();
    assert_eq!(needs[&id(1)], (AccessNeed::Person, AccessNeed::Remembered));
    assert_eq!(needs[&id(2)], (AccessNeed::Person, AccessNeed::Asks));
    // Previewing writes nothing.
    assert!(f.goals().is_empty() && f.host.launches().is_empty());
}

/// Save a Goal as Template… turns a goal's steps into a template whose steps search
/// first, keep their order and prerequisites, and can make each project an input.
/// The file is checked in beside the plan; set, validate and conflicts follow the
/// plan file's rules.
#[test]
fn hs2_a4rvya_save_a_goal_as_a_template_and_edit_templates_headlessly() {
    let mut rng = Rng(700);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(1, &mut tx);
    let goal = f.m.orchestration_ticket("Rate limits");
    let create = |title: &str| {
        ops::create(
            &f.m.target_store,
            ulid::Ulid::new(),
            "TGT",
            NOW.into(),
            NewTicket {
                title: title.into(),
                ..Default::default()
            },
        )
        .unwrap()
    };
    let first = create("Add rate-limit headers");
    let second = create("Show rate-limit errors");
    let define = |native: &str, prerequisites: Value, tx: &mut Transaction<'_>| {
        f.m.runtime
            .readiness_define(
                &f.owner(),
                serde_json::from_value(json!({
                    "ticket": {"native_id": goal.slug},
                    "target_id": id(1), "connection_id": f.m.target_connection(),
                    "native_id": native, "prerequisites": prerequisites, "tool": "codex",
                }))
                .unwrap(),
                Some(&worker()),
                tx,
            )
            .unwrap()
    };
    define(&first.slug, json!([]), &mut tx);
    define(
        &second.slug,
        json!([{"kind": "ticket", "target_id": id(1), "connection_id": f.m.target_connection(),
            "native_id": first.id.to_string(), "gates": "review"}]),
        &mut tx,
    );
    let empty = f.m.orchestration_ticket("Nothing planned");
    assert_eq!(
        code(
            f.m.runtime.template_save_goal(
                &f.owner(),
                serde_json::from_value(json!({"goal": {"native_id": empty.slug}, "name": "Empty"}))
                    .unwrap(),
                Some(&human()),
                &mut tx
            )
        ),
        "template_goal_empty"
    );
    let saved =
        f.m.runtime
            .template_save_goal(
                &f.owner(),
                serde_json::from_value(json!({"goal": {"native_id": goal.slug},
                    "name": "Rate limits again", "targets_as_inputs": true}))
                .unwrap(),
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(saved.version, 1);
    assert_eq!(saved.goal.title, "Rate limits");
    assert_eq!(saved.inputs.len(), 1);
    assert_eq!(saved.inputs[0].default, Some(json!(id(1))));
    assert_eq!(
        saved
            .steps
            .iter()
            .map(|step| (step.key.as_str(), step.title.as_str(), step.target.as_str()))
            .collect::<Vec<_>>(),
        [
            ("step-1", "Add rate-limit headers", "{{project-1}}"),
            ("step-2", "Show rate-limit errors", "{{project-1}}"),
        ]
    );
    assert_eq!(saved.steps[1].after[0].step, "step-1");
    assert_eq!(saved.steps[1].after[0].gates.as_deref(), Some("review"));
    assert_eq!(saved.steps[0].tool.as_deref(), Some("codex"));
    assert!(
        saved
            .steps
            .iter()
            .all(|step| step.ticket == StepTicket::Search)
    );
    // Checked in beside the plan.
    assert!(
        std::path::Path::new(&f.m.owner.root)
            .join(TEMPLATES_PATH)
            .is_file()
    );

    // Validate reports problems without writing; set refuses an invalid definition
    // and a stale revision.
    let mut broken = saved.clone();
    broken.steps[0].target = id(9);
    let validation =
        f.m.runtime
            .template_validate(
                &f.owner(),
                &ValidateTemplate {
                    template: broken.clone(),
                },
            )
            .unwrap();
    assert!(!validation.valid);
    assert_eq!(validation.problems[0].code, "target_unknown");
    broken.steps.clear();
    assert_eq!(
        code(f.m.runtime.template_set(
            &f.owner(),
            SetTemplates {
                templates: vec![broken],
                expected_revision: None
            },
            Some(&human()),
            &mut tx
        )),
        "template_invalid"
    );
    assert_eq!(
        code(f.m.runtime.template_set(
            &f.owner(),
            SetTemplates {
                templates: vec![saved.clone()],
                expected_revision: Some(id(1))
            },
            Some(&human()),
            &mut tx
        )),
        "template_conflict"
    );
    let mut twice = saved.clone();
    twice.name = "Different".into();
    assert_eq!(
        code(f.m.runtime.template_set(
            &f.owner(),
            SetTemplates {
                templates: vec![saved.clone(), twice],
                expected_revision: None
            },
            Some(&human()),
            &mut tx
        )),
        "template_conflict"
    );
    assert_eq!(
        f.m.last_audit().0,
        "template_set",
        "every change of the templates is audited"
    );
}

/// A crash between recording an id and acting on it is resumed under the same ids:
/// the goal is created with the id recorded for it, and the planner is asked under
/// the proposal id recorded before asking, so nothing is created or started twice.
#[test]
fn hs2_a4rvya_an_interrupted_apply_resumes_under_the_recorded_ids() {
    let mut rng = Rng(800);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    let template = f.define("Feature", &mut tx);
    f.m.orchestration_ticket("Ship the launch");
    let busy = f
        .m
        .runtime
        .plan_propose(
            &f.owner(),
            serde_json::from_value(json!({"tool": "claude", "operation_id": "press-1"})).unwrap(),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let terminal = busy.session.session.session_id.clone();
    f.host.set_busy(&terminal, true);
    let waiting = f
        .apply(
            &template,
            json!({"feature": "Crash"}),
            "op-c",
            &human(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(waiting.apply.state, ApplyState::WaitingPlanner);

    // As if the process died after recording a goal id and a proposal id, before
    // creating the ticket or hearing the planner's answer.
    let store = TemplateApplyStore::new(&f.m.home, &f.owner()).unwrap();
    let mut record = store.find("op-c").unwrap().unwrap();
    let recorded_goal = ulid::Ulid::from(4242).to_string();
    record.goal = Some(ApplyGoal {
        connection_id: record.goal.clone().unwrap().connection_id,
        native_id: recorded_goal.clone(),
        slug: None,
        created: false,
    });
    record.proposal_id = Some("op-c.p7".into());
    record.reason = None;
    store.write(&record).unwrap();

    f.host.set_busy(&terminal, false);
    let resumed = f
        .apply(
            &template,
            json!({"feature": "Crash"}),
            "op-c",
            &human(),
            &mut tx,
        )
        .unwrap();
    let goal = resumed.apply.goal.unwrap();
    assert_eq!(
        goal.native_id, recorded_goal,
        "created under the recorded id"
    );
    assert!(goal.created);
    assert!(
        f.goals()
            .iter()
            .any(|ticket| ticket.id.to_string() == recorded_goal)
    );
    // The planner was asked under the proposal id recorded before the crash; a
    // second resume finds it Planning and changes nothing.
    assert_eq!(resumed.apply.state, ApplyState::Planning);
    assert_eq!(resumed.apply.proposal_id.as_deref(), Some("op-c.p7"));
    let again = f
        .apply(
            &template,
            json!({"feature": "Crash"}),
            "op-c",
            &human(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(again.outcome.as_deref(), Some("existing"));
    assert_eq!(f.host.launches().len(), 1);
}

/// HS2-K3ESZR: the preview reads the test profiles each project offers through the
/// gate as an observation (never asking) and names every profile a step uses that its
/// project does not offer; a damaged profile file is a problem of each step that
/// names profiles there; a read that cannot happen leaves the project unverified
/// rather than blocking Apply; a step a false `when` leaves out is not checked.
#[test]
fn hs2_k3eszr_preview_checks_validation_profiles_against_projects() {
    let mut rng = Rng(700);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    let document =
        f.m.runtime
            .template_set(
                &f.owner(),
                serde_json::from_value(json!({"templates": [{
                    "name": "Checked",
                    "inputs": [
                        {"key": "feature", "label": "Feature", "kind": "text", "required": true},
                        {"key": "docs", "label": "Docs", "kind": "boolean", "default": false}
                    ],
                    "goal": {"title": "{{feature}}"},
                    "steps": [
                        {"key": "api", "target": id(1), "title": "API {{feature}}",
                         "validation": ["smoke", "lint"]},
                        {"key": "web", "target": id(2), "title": "Web {{feature}}",
                         "validation": ["smoke"]},
                        {"key": "docs", "target": id(1), "title": "Docs {{feature}}",
                         "validation": ["missing-only-here"], "when": "{{docs}}"}
                    ]
                }]}))
                .unwrap(),
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    let template = document.templates[0].clone();
    let preview = |actor: Option<&MutationActor>, tx: &mut Transaction<'_>| {
        f.m.runtime
            .template_preview(
                &f.owner(),
                &serde_json::from_value(json!({"template_id": template.template_id,
                    "inputs": {"feature": "Quotas"}}))
                .unwrap(),
                actor,
                tx,
            )
            .unwrap()
    };
    let found = |preview: &TemplatePreview| {
        preview
            .problems
            .iter()
            .map(|problem| (problem.code.clone(), problem.at.clone()))
            .collect::<Vec<_>>()
    };
    let unknown = |at: &str| ("validation_profile_unknown".to_owned(), at.to_owned());
    let checked = |preview: &TemplatePreview| {
        preview
            .targets
            .iter()
            .map(|target| target.profiles)
            .collect::<Vec<_>>()
    };
    let directory = std::path::Path::new(&f.m.target.root).join(".hotsheet2");
    let write = |profiles: Value| {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("validation-profiles.json"),
            json!({"schema_version": 1, "profiles": profiles}).to_string(),
        )
        .unwrap();
    };
    // No profile file: the projects offer none, so every named profile is unknown.
    // The `docs` step is left out by its `when`, so its profile is not checked.
    let none = preview(Some(&human()), &mut tx);
    assert_eq!(
        found(&none),
        [
            unknown("steps[0].validation[0]"),
            unknown("steps[0].validation[1]"),
            unknown("steps[1].validation[0]"),
        ]
    );
    assert_eq!(none.skipped, ["docs"]);
    assert_eq!(
        checked(&none),
        [Some(ProfilesCheck::Checked), Some(ProfilesCheck::Checked)]
    );
    // Read through the gate as an observation: audited, and nobody is asked.
    assert_eq!(
        f.m.last_audit(),
        ("gates_observe".into(), AuditOutcome::Succeeded, None)
    );
    assert!(
        f.m.runtime
            .permission_list(&f.owner(), &mut tx)
            .unwrap()
            .requests
            .is_empty()
    );
    // Refilled with one profile: only `lint` is still unknown (both projects share
    // the fixture checkout).
    write(json!([{"name": "smoke", "program": "true"}]));
    assert_eq!(
        found(&preview(Some(&human()), &mut tx)),
        [unknown("steps[0].validation[1]")]
    );
    // A damaged file: no profile there runs by name.
    write(json!([{"name": "smoke", "program": "a"}, {"name": "smoke", "program": "b"}]));
    assert_eq!(
        found(&preview(Some(&human()), &mut tx)),
        [
            (
                "validation_profiles_invalid".to_owned(),
                "steps[0].validation".to_owned()
            ),
            (
                "validation_profiles_invalid".to_owned(),
                "steps[1].validation".to_owned()
            ),
        ]
    );
    // Every profile offered: no problems, and previewing again answers the same.
    write(json!([{"name": "smoke", "program": "true"}, {"name": "lint", "program": "true"}]));
    let ok = preview(Some(&worker()), &mut tx);
    assert!(ok.problems.is_empty(), "{:?}", ok.problems);
    assert_eq!(found(&preview(Some(&worker()), &mut tx)), found(&ok));
    // Without an identified actor the gate cannot be passed: the projects are
    // unverified and nothing blocks Apply on their account.
    std::fs::remove_file(directory.join("validation-profiles.json")).unwrap();
    let anonymous = preview(None, &mut tx);
    assert!(anonymous.problems.is_empty(), "{:?}", anonymous.problems);
    assert_eq!(
        checked(&anonymous),
        [
            Some(ProfilesCheck::Unverified),
            Some(ProfilesCheck::Unverified)
        ]
    );
    // Previewing writes nothing.
    assert!(f.goals().is_empty() && f.host.launches().is_empty());
}

/// HS2-AMJNER: an AI session's apply that waits for a permission continues on its own
/// once a person answers: the resume pass repeats it under its operation id and as
/// its actor, exactly once, and leaves an unanswered or denied apply alone.
#[test]
fn hs2_amjner_a_waiting_apply_resumes_once_its_permission_is_answered() {
    let mut rng = Rng(1100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    let template = f.define("Feature", &mut tx);
    let mut memory = ResumeMemory::default();
    let resume = |memory: &mut ResumeMemory, tx: &mut Transaction<'_>| {
        f.m.runtime
            .template_resume_waiting(&f.owner(), memory, tx)
            .unwrap()
            .resumed
    };
    // Nothing waits: nothing is resumed and nothing is audited.
    assert!(resume(&mut memory, &mut tx).is_empty());
    assert!(matches!(
        f.apply(
            &template,
            json!({"feature": "Quotas"}),
            "op-ai",
            &worker(),
            &mut tx
        ),
        Err(OrchestrationError::PermissionRequired { .. })
    ));
    let waiting =
        f.m.runtime
            .template_apply_show(
                &f.owner(),
                &ApplyRef {
                    operation_id: "op-ai".into(),
                },
            )
            .unwrap();
    // Unanswered (none, then one of two): it keeps waiting and files nothing new.
    assert!(resume(&mut memory, &mut tx).is_empty());
    f.m.runtime
        .permission_answer(
            &f.owner(),
            &waiting.apply.permission_requests[0],
            PermissionAnswer::AllowOnce,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert!(resume(&mut memory, &mut tx).is_empty());
    assert_eq!(
        f.m.runtime
            .permission_list(&f.owner(), &mut tx)
            .unwrap()
            .requests
            .len(),
        2
    );
    f.m.runtime
        .permission_answer(
            &f.owner(),
            &waiting.apply.permission_requests[1],
            PermissionAnswer::AllowOnce,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let resumed = resume(&mut memory, &mut tx);
    assert_eq!(
        resumed,
        [ResumedApply {
            operation_id: "op-ai".into(),
            state: ApplyState::Planning,
            reason: None,
        }]
    );
    assert_eq!(f.goals().len(), 1);
    assert_eq!(f.host.launches().len(), 1);
    // Recorded as the AI session's own apply, with the ordinary audit.
    let shown =
        f.m.runtime
            .template_apply_show(
                &f.owner(),
                &ApplyRef {
                    operation_id: "op-ai".into(),
                },
            )
            .unwrap();
    assert_eq!(shown.apply.actor_id.as_deref(), worker().id.as_deref());
    assert_eq!(
        f.m.last_audit(),
        (
            "template_apply".into(),
            AuditOutcome::Succeeded,
            Some("template_planning".into())
        )
    );
    // Planning now: a second pass (and a third) does nothing more.
    assert!(resume(&mut memory, &mut tx).is_empty());
    assert!(resume(&mut ResumeMemory::default(), &mut tx).is_empty());
    assert_eq!(f.goals().len(), 1);
    assert_eq!(f.host.launches().len(), 1);

    // Denied: the pass records it denied, once, and opens nothing.
    f.host.crash(&f.host.launches()[0].session_id);
    let OrchestrationError::PermissionRequired { request_id } = f
        .apply(
            &template,
            json!({"feature": "Other"}),
            "op-deny",
            &worker(),
            &mut tx,
        )
        .unwrap_err()
    else {
        panic!("expected a permission request");
    };
    let other =
        f.m.runtime
            .template_apply_show(
                &f.owner(),
                &ApplyRef {
                    operation_id: "op-deny".into(),
                },
            )
            .unwrap();
    for request in &other.apply.permission_requests {
        f.m.runtime
            .permission_answer(
                &f.owner(),
                request,
                if *request == request_id {
                    PermissionAnswer::Deny
                } else {
                    PermissionAnswer::AllowOnce
                },
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    }
    let resumed = resume(&mut memory, &mut tx);
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0].state, ApplyState::Denied);
    assert!(resume(&mut memory, &mut tx).is_empty());
    assert_eq!(f.goals().len(), 1);
}

/// HS2-AMJNER: an apply waiting on a busy planner is not asked again while the planner
/// works; once it finishes its turn the pass gives it the goal, under the next attempt
/// id, and nothing more after that.
#[test]
fn hs2_amjner_a_waiting_apply_resumes_once_the_planner_is_free() {
    let mut rng = Rng(1200);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    let template = f.define("Feature", &mut tx);
    f.m.orchestration_ticket("Ship the launch");
    let busy = f
        .m
        .runtime
        .plan_propose(
            &f.owner(),
            serde_json::from_value(json!({"tool": "claude", "operation_id": "press-1"})).unwrap(),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let terminal = busy.session.session.session_id.clone();
    f.host.set_busy(&terminal, true);
    let waiting = f
        .apply(
            &template,
            json!({"feature": "Billing"}),
            "op-w",
            &human(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(waiting.apply.state, ApplyState::WaitingPlanner);
    let pushes = f.host.pushes().len();
    let mut memory = ResumeMemory::default();
    // Busy: the pass leaves it waiting and asks nothing of the planner.
    for _ in 0..3 {
        assert!(
            f.m.runtime
                .template_resume_waiting(&f.owner(), &mut memory, &mut tx)
                .unwrap()
                .resumed
                .is_empty()
        );
    }
    assert_eq!(f.host.pushes().len(), pushes);
    // Free: the goal is given to the planner once.
    f.host.set_busy(&terminal, false);
    let resumed =
        f.m.runtime
            .template_resume_waiting(&f.owner(), &mut memory, &mut tx)
            .unwrap()
            .resumed;
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0].state, ApplyState::Planning);
    let shown =
        f.m.runtime
            .template_apply_show(
                &f.owner(),
                &ApplyRef {
                    operation_id: "op-w".into(),
                },
            )
            .unwrap();
    assert_eq!(shown.apply.proposal_id.as_deref(), Some("op-w.p1"));
    assert_eq!(f.host.pushes().len(), pushes + 1);
    assert!(
        f.m.runtime
            .template_resume_waiting(&f.owner(), &mut memory, &mut tx)
            .unwrap()
            .resumed
            .is_empty()
    );
    assert_eq!(f.goals().len(), 1);
    assert_eq!(f.host.launches().len(), 1);
}

/// HS2-36XDDX: the planner's step for a goal, on a new ticket of target 1.
fn plan_step(f: &Fixture, goal_slug: &str, title: &str, tx: &mut Transaction<'_>) -> String {
    let target = ops::create(
        &f.m.target_store,
        ulid::Ulid::new(),
        "TGT",
        NOW.into(),
        NewTicket {
            title: title.into(),
            ..Default::default()
        },
    )
    .unwrap();
    f.m.runtime
        .readiness_define(
            &f.owner(),
            serde_json::from_value(json!({
                "ticket": {"native_id": goal_slug},
                "target_id": id(1), "connection_id": f.m.target_connection(),
                "native_id": target.slug,
            }))
            .unwrap(),
            Some(&worker()),
            tx,
        )
        .unwrap()
        .readiness
        .step
        .step_id
}

/// HS2-36XDDX: why the scheduler leaves a step unplaced, and the state its card shows.
fn standing(f: &Fixture, step_id: &str, tx: &mut Transaction<'_>) -> (String, String) {
    let explained =
        f.m.runtime
            .scheduler_explain(
                &f.owner(),
                &serde_json::from_value(json!({"step_id": step_id})).unwrap(),
                Some(&human()),
                tx,
            )
            .unwrap();
    let timeline =
        f.m.runtime
            .orchestration_timeline(&f.owner(), None, 0, tx)
            .unwrap();
    let item = timeline
        .waves
        .iter()
        .flat_map(|wave| &wave.items)
        .find(|item| item.step_id == step_id)
        .unwrap();
    (explained.reason, item.state.code().to_owned())
}

fn accept(
    f: &Fixture,
    operation_id: &str,
    actor: &MutationActor,
    tx: &mut Transaction<'_>,
) -> Result<TemplateAcceptView, OrchestrationError> {
    f.m.runtime.template_accept(
        &f.owner(),
        &serde_json::from_value(json!({"operation_id": operation_id})).unwrap(),
        Some(actor),
        tx,
    )
}

fn set_automatic(f: &Fixture, automatic: bool, tx: &mut Transaction<'_>) {
    f.m.runtime
        .set_automatic(
            &f.owner(),
            serde_json::from_value(json!({"automatic": automatic})).unwrap(),
            Some(&human()),
            tx,
        )
        .unwrap();
}

/// HS2-36XDDX: with Wait for Accept on (the default) and placement Auto, the steps the
/// planner defines for the template's goal are held until a person accepts the plan;
/// a step added later is held again; Pause shows the plain dry run, and Start
/// Orchestration accepts what is held. Only a person accepts.
#[test]
fn hs2_36xddx_a_templates_steps_wait_for_a_persons_accept_under_auto_placement() {
    let mut rng = Rng(3600);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    f.m.runtime
        .set_policy(
            &f.owner(),
            hotsheet_ticketing::orchestration::SetPolicy {
                expected_revision: None,
                policy: super::sessions::policy(),
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let template = f.define("Feature across API and Web", &mut tx);
    assert!(template.wait_for_accept, "on by default");
    let preview = f
        .m
        .runtime
        .template_preview(
            &f.owner(),
            &serde_json::from_value(
                json!({"template_id": template.template_id, "inputs": {"feature": "Rate limits"}}),
            )
            .unwrap(),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert!(preview.wait_for_accept);
    let applied = f
        .apply(
            &template,
            json!({"feature": "Rate limits"}),
            "op-1",
            &human(),
            &mut tx,
        )
        .unwrap();
    assert_eq!(applied.apply.state, ApplyState::Planning);
    let goal = applied.apply.goal.clone().unwrap().slug.unwrap();
    // Nothing planned yet: nothing to accept.
    assert_eq!(
        code(accept(&f, "op-1", &human(), &mut tx)),
        "template_accept_nothing_planned"
    );
    assert_eq!(
        f.m.last_audit(),
        (
            "template_accept".into(),
            AuditOutcome::Failed,
            Some("template_accept_nothing_planned".into())
        )
    );

    // The planner proposes a step: placement is Auto now, and the step is held.
    let first = plan_step(&f, &goal, "Add rate limit headers", &mut tx);
    assert_eq!(
        standing(&f, &first, &mut tx),
        (
            "template_plan_not_accepted".into(),
            "waits_for_accept".into()
        )
    );
    let timeline =
        f.m.runtime
            .orchestration_timeline(&f.owner(), None, 0, &mut tx)
            .unwrap();
    assert!(
        timeline.automatic,
        "Auto placement is on once the plan has a step"
    );
    let item = timeline
        .waves
        .iter()
        .flat_map(|wave| &wave.items)
        .next()
        .unwrap();
    let wait = item.accept.clone().unwrap();
    assert_eq!(wait.operation_id, "op-1");
    assert_eq!(wait.template_name, "Feature across API and Web");

    // An AI actor never accepts; an unknown apply is not found.
    assert_eq!(code(accept(&f, "op-1", &worker(), &mut tx)), "forbidden");
    assert_eq!(
        code(accept(&f, "op-missing", &human(), &mut tx)),
        "template_apply_not_found"
    );
    assert_eq!(
        standing(&f, &first, &mut tx).1,
        "waits_for_accept",
        "a refused accept releases nothing"
    );

    // Pause: a Manual plan is the plain dry run it was; the work waits for Start.
    set_automatic(&f, false, &mut tx);
    assert_ne!(standing(&f, &first, &mut tx).1, "waits_for_accept");
    // A trigger's Start is nobody's review: the step stays held.
    f.m.runtime
        .set_automatic_unreviewed(
            &f.owner(),
            serde_json::from_value(json!({"automatic": true})).unwrap(),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        standing(&f, &first, &mut tx),
        (
            "template_plan_not_accepted".into(),
            "waits_for_accept".into()
        )
    );
    set_automatic(&f, false, &mut tx);
    // Start Orchestration is the person's review of the plan: it accepts what is held.
    set_automatic(&f, true, &mut tx);
    let (reason, state) = standing(&f, &first, &mut tx);
    assert_ne!(reason, "template_plan_not_accepted");
    assert_ne!(state, "waits_for_accept");
    let record =
        f.m.runtime
            .template_apply_show(
                &f.owner(),
                &serde_json::from_value(json!({"operation_id": "op-1"})).unwrap(),
            )
            .unwrap();
    assert_eq!(record.apply.accepted_steps, std::slice::from_ref(&first));
    assert_eq!(record.apply.accepted_by.as_deref(), human().id.as_deref());
    let journal = f.m.runtime.audit(&f.owner(), 0, 500).unwrap().entries;
    assert!(
        journal
            .iter()
            .any(|entry| entry.record.action == "template_accept"
                && entry.record.reason.as_deref() == Some("start_orchestration"))
    );

    // The planner re-plans the goal and adds a step: only the new one is held.
    let second = plan_step(&f, &goal, "Show rate limit errors", &mut tx);
    assert_eq!(
        standing(&f, &second, &mut tx).0,
        "template_plan_not_accepted"
    );
    assert_ne!(standing(&f, &first, &mut tx).1, "waits_for_accept");
    // A person accepts through the plan review: the new step is released, once.
    let accepted = accept(&f, "op-1", &human(), &mut tx).unwrap();
    assert_eq!(accepted.outcome, "accepted");
    assert_eq!(accepted.released, std::slice::from_ref(&second));
    assert_eq!(
        f.m.last_audit(),
        (
            "template_accept".into(),
            AuditOutcome::Succeeded,
            Some("template_accepted".into())
        )
    );
    assert_ne!(
        standing(&f, &second, &mut tx).0,
        "template_plan_not_accepted"
    );
    let again = accept(&f, "op-1", &human(), &mut tx).unwrap();
    assert_eq!(again.outcome, "existing");
    assert!(again.released.is_empty());
    assert_eq!(
        again.apply.unwrap().accepted_at_ms,
        accepted.apply.unwrap().accepted_at_ms
    );
    // The list names the setting and the accept.
    let listed = f.m.runtime.template_list(&f.owner()).unwrap();
    assert!(listed.applies[0].wait_for_accept);
    assert!(listed.applies[0].accepted_at_ms.is_some());
}

/// HS2-36XDDX: a template with Wait for Accept off keeps today's behaviour: its steps
/// are placed within the policy as soon as they are ready.
#[test]
fn hs2_36xddx_a_template_that_does_not_wait_places_its_steps_at_once() {
    let mut rng = Rng(3700);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    f.m.runtime
        .set_policy(
            &f.owner(),
            hotsheet_ticketing::orchestration::SetPolicy {
                expected_revision: None,
                policy: super::sessions::policy(),
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let mut template = f.define("Repair failing CI", &mut tx);
    template.wait_for_accept = false;
    let saved =
        f.m.runtime
            .template_set(
                &f.owner(),
                SetTemplates {
                    templates: vec![template.clone()],
                    expected_revision: None,
                },
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    // A changed definition is the next version; the file is version 2 while it is off.
    assert_eq!(saved.templates[0].version, 2);
    assert!(!saved.templates[0].wait_for_accept);
    let on_disk: Value = serde_json::from_slice(
        &std::fs::read(std::path::Path::new(&f.m.owner.root).join(TEMPLATES_PATH)).unwrap(),
    )
    .unwrap();
    assert_eq!(on_disk["schema_version"], 2);
    let template = saved.templates[0].clone();
    let applied = f
        .apply(
            &template,
            json!({"feature": "Flaky login"}),
            "op-off",
            &human(),
            &mut tx,
        )
        .unwrap();
    let goal = applied.apply.goal.clone().unwrap().slug.unwrap();
    let step = plan_step(&f, &goal, "Repair the login test", &mut tx);
    let (reason, state) = standing(&f, &step, &mut tx);
    assert_ne!(reason, "template_plan_not_accepted");
    assert_ne!(state, "waits_for_accept");
    // Nothing waits, so an accept has nothing new to release.
    let accepted = accept(&f, "op-off", &human(), &mut tx).unwrap();
    assert_eq!(accepted.released, [step]);
    let listed = f.m.runtime.template_list(&f.owner()).unwrap();
    assert!(!listed.applies[0].wait_for_accept);
}

/// HS2-38MFXT: another machine that runs the project's scheduler has the checked-in
/// plan and no apply record. The hold travels with the plan: with this machine's apply
/// record gone (as on that machine) the step is still held, a person's accept there
/// finds the apply through the checked-in hold, and the accept is checked in. An apply
/// recorded before the hold travelled (no checked-in hold) is still held locally.
#[test]
fn hs2_38mfxt_the_hold_and_the_accept_travel_with_the_checked_in_plan() {
    let mut rng = Rng(3800);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    f.m.runtime
        .set_policy(
            &f.owner(),
            hotsheet_ticketing::orchestration::SetPolicy {
                expected_revision: None,
                policy: super::sessions::policy(),
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let template = f.define("Feature across API and Web", &mut tx);
    let plan_path = std::path::Path::new(&f.m.owner.root).join(SHARED_PLAN_PATH);
    let applied = f
        .apply(
            &template,
            json!({"feature": "Rate limits"}),
            "op-shared",
            &human(),
            &mut tx,
        )
        .unwrap();
    let goal = applied.apply.goal.clone().unwrap();
    // The hold is checked in with the goal, before any step: version 2.
    let on_disk: Value = serde_json::from_slice(&std::fs::read(&plan_path).unwrap()).unwrap();
    assert_eq!(on_disk["schema_version"], 2);
    assert_eq!(on_disk["holds"][0]["operation_id"], "op-shared");
    assert_eq!(on_disk["holds"][0]["goal"]["native_id"], goal.native_id);
    assert_eq!(
        on_disk["holds"][0]["goal_slug"],
        goal.slug.clone().unwrap().as_str()
    );
    assert!(on_disk["steps"].as_array().unwrap().is_empty());
    // A repeated apply checks nothing in again.
    let revision = on_disk["revision"].clone();
    f.apply(
        &template,
        json!({"feature": "Rate limits"}),
        "op-shared",
        &human(),
        &mut tx,
    )
    .unwrap();
    let on_disk: Value = serde_json::from_slice(&std::fs::read(&plan_path).unwrap()).unwrap();
    assert_eq!(on_disk["revision"], revision);

    let slug = goal.slug.clone().unwrap();
    let first = plan_step(&f, &slug, "Add rate limit headers", &mut tx);
    assert_eq!(
        standing(&f, &first, &mut tx),
        (
            "template_plan_not_accepted".into(),
            "waits_for_accept".into()
        )
    );
    // Another machine: the plan, and no apply record. Still held, and still explained.
    let records =
        f.m.home
            .join(hotsheet_ticketing::orchestration::TEMPLATE_APPLY_DIR);
    let kept = f.m.home.join("applies-kept");
    std::fs::rename(&records, &kept).unwrap();
    assert_eq!(
        standing(&f, &first, &mut tx),
        (
            "template_plan_not_accepted".into(),
            "waits_for_accept".into()
        )
    );
    // An AI actor still cannot accept there; a person can.
    assert_eq!(
        code(accept(&f, "op-shared", &worker(), &mut tx)),
        "forbidden"
    );
    let accepted = accept(&f, "op-shared", &human(), &mut tx).unwrap();
    assert_eq!(accepted.outcome, "accepted");
    assert_eq!(accepted.released, std::slice::from_ref(&first));
    assert!(accepted.apply.is_none(), "no apply record on this machine");
    let hold = accepted.hold.clone().unwrap();
    assert_eq!(hold.accepted_steps, std::slice::from_ref(&first));
    assert_eq!(hold.accepted_by.as_deref(), human().id.as_deref());
    let answer = serde_json::to_value(&accepted).unwrap();
    assert_eq!(answer["hold"]["operation_id"], "op-shared");
    assert!(answer.get("snapshot").is_none());
    assert_ne!(standing(&f, &first, &mut tx).1, "waits_for_accept");
    // Back on the machine that applied: its record never saw the accept, but the
    // checked-in hold is authoritative, so the step is released there too.
    std::fs::remove_dir_all(&records).ok();
    std::fs::rename(&kept, &records).unwrap();
    assert_ne!(standing(&f, &first, &mut tx).1, "waits_for_accept");
    assert_eq!(
        accept(&f, "op-shared", &human(), &mut tx).unwrap().outcome,
        "existing"
    );
    let listed = f.m.runtime.template_list(&f.owner()).unwrap();
    assert_eq!(listed.applies[0].accepted_at_ms, hold.accepted_at_ms);
    // A later step is held again; Start Orchestration on the other machine accepts it.
    let second = plan_step(&f, &slug, "Show rate limit errors", &mut tx);
    std::fs::rename(&records, &kept).unwrap();
    assert_eq!(standing(&f, &second, &mut tx).1, "waits_for_accept");
    set_automatic(&f, false, &mut tx);
    set_automatic(&f, true, &mut tx);
    assert_ne!(standing(&f, &second, &mut tx).1, "waits_for_accept");
    std::fs::remove_dir_all(&records).ok();
    std::fs::rename(&kept, &records).unwrap();
    assert_ne!(standing(&f, &second, &mut tx).1, "waits_for_accept");

    // An apply recorded before the hold travelled: no checked-in hold, so this
    // machine's record holds its steps, and its accept works as before.
    let legacy = f
        .apply(
            &template,
            json!({"feature": "Audit log"}),
            "op-legacy",
            &human(),
            &mut tx,
        )
        .unwrap();
    let mut on_disk: Value = serde_json::from_slice(&std::fs::read(&plan_path).unwrap()).unwrap();
    let holds = on_disk["holds"].as_array_mut().unwrap();
    holds.retain(|hold| hold["operation_id"] != "op-legacy");
    std::fs::write(&plan_path, serde_json::to_vec_pretty(&on_disk).unwrap()).unwrap();
    let legacy_step = plan_step(
        &f,
        &legacy.apply.goal.unwrap().slug.unwrap(),
        "Write the audit log",
        &mut tx,
    );
    assert_eq!(standing(&f, &legacy_step, &mut tx).1, "waits_for_accept");
    let accepted = accept(&f, "op-legacy", &human(), &mut tx).unwrap();
    assert_eq!(accepted.released, std::slice::from_ref(&legacy_step));
    assert!(accepted.hold.is_none());
    assert_eq!(
        accepted.apply.unwrap().accepted_steps,
        std::slice::from_ref(&legacy_step)
    );
    assert_ne!(standing(&f, &legacy_step, &mut tx).1, "waits_for_accept");
    // Unknown on this machine and in the plan: not found.
    std::fs::rename(&records, &kept).unwrap();
    assert_eq!(
        code(accept(&f, "op-legacy", &human(), &mut tx)),
        "template_apply_not_found"
    );
}

/// HS2-0YHA2S: the Orchestration header's Accept Plan accepts every template plan held
/// now, each as its own accept (audited per apply); only a person accepts, nothing held
/// answers `existing`, and a step planned later is held again.
#[test]
fn hs2_0yha2s_one_accept_releases_every_held_template_plan() {
    let mut rng = Rng(3900);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    f.m.runtime
        .set_policy(
            &f.owner(),
            hotsheet_ticketing::orchestration::SetPolicy {
                expected_revision: None,
                policy: super::sessions::policy(),
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let all = |actor: &MutationActor, tx: &mut Transaction<'_>| {
        f.m.runtime
            .template_accept_held(&f.owner(), Some(actor), tx)
    };
    // Nothing held yet: nothing to accept, and nothing written.
    let empty = all(&human(), &mut tx).unwrap();
    assert_eq!(empty.outcome, "existing");
    assert!(empty.accepted.is_empty() && empty.released.is_empty());
    assert_eq!(code(all(&worker(), &mut tx)), "forbidden");

    let first = f.define("Feature across API and Web", &mut tx);
    let second = f.define("Repair failing CI", &mut tx);
    let mut goals = Vec::new();
    for (template, operation, feature) in [
        (&first, "op-a", "Rate limits"),
        (&second, "op-b", "Flaky login"),
    ] {
        let applied = f
            .apply(
                template,
                json!({"feature": feature}),
                operation,
                &human(),
                &mut tx,
            )
            .unwrap();
        goals.push(applied.apply.goal.unwrap().slug.unwrap());
    }
    let a1 = plan_step(&f, &goals[0], "Add rate limit headers", &mut tx);
    let a2 = plan_step(&f, &goals[0], "Show rate limit errors", &mut tx);
    let b1 = plan_step(&f, &goals[1], "Repair the login test", &mut tx);
    for step in [&a1, &a2, &b1] {
        assert_eq!(standing(&f, step, &mut tx).1, "waits_for_accept");
    }
    // An AI actor never accepts, not even all at once; nothing is released.
    assert_eq!(code(all(&worker(), &mut tx)), "forbidden");
    assert_eq!(standing(&f, &a1, &mut tx).1, "waits_for_accept");
    // A request naming nothing is refused.
    assert_eq!(
        code(f.m.runtime.template_accept(
            &f.owner(),
            &AcceptTemplate::default(),
            Some(&human()),
            &mut tx
        )),
        "invalid_request"
    );

    // One accept releases both templates' plans.
    let accepted = all(&human(), &mut tx).unwrap();
    assert_eq!(accepted.outcome, "accepted");
    let mut expected = vec![a1.clone(), a2.clone(), b1.clone()];
    expected.sort();
    assert_eq!(accepted.released, expected);
    let operations: Vec<&str> = accepted
        .accepted
        .iter()
        .map(|view| view.hold.as_ref().unwrap().operation_id.as_str())
        .collect();
    assert_eq!(operations, ["op-a", "op-b"]);
    for step in [&a1, &a2, &b1] {
        assert_ne!(standing(&f, step, &mut tx).1, "waits_for_accept");
    }
    let journal = f.m.runtime.audit(&f.owner(), 0, 500).unwrap().entries;
    let accepts: Vec<_> = journal
        .iter()
        .filter(|entry| {
            entry.record.action == "template_accept"
                && entry.record.reason.as_deref() == Some("template_accepted")
        })
        .collect();
    assert_eq!(accepts.len(), 2, "one accept per apply");
    // Again: nothing held, nothing new.
    let again = all(&human(), &mut tx).unwrap();
    assert_eq!(again.outcome, "existing");
    assert!(again.accepted.is_empty());
    // The planner adds a step to one goal: only that one is held, and released again.
    let a3 = plan_step(&f, &goals[0], "Document rate limits", &mut tx);
    assert_eq!(standing(&f, &a3, &mut tx).1, "waits_for_accept");
    let later = all(&human(), &mut tx).unwrap();
    assert_eq!(later.released, std::slice::from_ref(&a3));
    assert_eq!(later.accepted.len(), 1);
}

/// HS2-MGG1GM: the checked-in hold of each template apply, by operation id, and the
/// plan's version.
fn checked_in_holds(f: &Fixture) -> (Vec<String>, u64, Value) {
    let path = std::path::Path::new(&f.m.owner.root).join(SHARED_PLAN_PATH);
    let on_disk: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let holds = on_disk
        .get("holds")
        .and_then(Value::as_array)
        .map(|holds| {
            holds
                .iter()
                .map(|hold| hold["operation_id"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default();
    (
        holds,
        on_disk["schema_version"].as_u64().unwrap(),
        on_disk["revision"].clone(),
    )
}

fn pass(f: &Fixture, dry_run: bool, tx: &mut Transaction<'_>) {
    f.m.runtime
        .scheduler_tick(&f.owner(), &TickRequest { dry_run }, Some(&human()), tx)
        .unwrap();
}

fn set_goal_status(f: &Fixture, goal: &ApplyGoal, status: hotsheet_model::Status) {
    ops::update(
        &f.m.owner_store,
        &goal.native_id.parse().unwrap(),
        NOW.into(),
        hotsheet_ticketing::TicketPatch {
            status: Some(status),
            ..Default::default()
        },
    )
    .unwrap();
}

/// HS2-MGG1GM: a template's checked-in hold is retired at the next scheduling pass
/// once its goal closes (completed, deleted, or purged where it was opened) and it
/// holds no live step, so the plan is version 1 again once no template waits. A closed
/// goal whose step nobody accepted keeps its hold; a dry run retires nothing; a goal
/// another machine opened and this one cannot find (not synced yet) keeps it; a
/// closed goal's apply record no longer holds either.
#[test]
fn hs2_mgg1gm_a_closed_goals_hold_is_retired_and_the_plan_is_version_1_again() {
    let mut rng = Rng(3900);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let f = Fixture::new(2, &mut tx);
    f.m.runtime
        .set_policy(
            &f.owner(),
            hotsheet_ticketing::orchestration::SetPolicy {
                expected_revision: None,
                policy: super::sessions::policy(),
            },
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let template = f.define("Feature across API and Web", &mut tx);
    let apply = |operation: &str, feature: &str, tx: &mut Transaction<'_>| {
        f.apply(
            &template,
            json!({ "feature": feature }),
            operation,
            &human(),
            tx,
        )
        .unwrap()
        .apply
        .goal
        .unwrap()
    };
    let closing = apply("op-close", "Rate limits", &mut tx);
    let open = apply("op-open", "Audit log", &mut tx);
    let first = plan_step(&f, closing.slug.as_deref().unwrap(), "Add headers", &mut tx);
    // Open goals: a pass retires nothing.
    pass(&f, false, &mut tx);
    assert_eq!(checked_in_holds(&f).0, ["op-close", "op-open"]);

    // The goal closes with its step unaccepted: the hold stays, the step waits.
    set_goal_status(&f, &closing, hotsheet_model::Status::Completed);
    pass(&f, false, &mut tx);
    assert_eq!(checked_in_holds(&f).0, ["op-close", "op-open"]);
    assert_eq!(standing(&f, &first, &mut tx).1, "waits_for_accept");
    // Accepted: it holds nothing now. A dry run retires nothing; the next pass does.
    accept(&f, "op-close", &human(), &mut tx).unwrap();
    pass(&f, true, &mut tx);
    assert_eq!(checked_in_holds(&f).0, ["op-close", "op-open"]);
    pass(&f, false, &mut tx);
    assert_eq!(checked_in_holds(&f).0, ["op-open"]);
    assert_eq!(
        f.m.last_audit(),
        (
            "template_hold_retire".into(),
            AuditOutcome::Succeeded,
            Some("template_goal_closed".into())
        )
    );
    // This machine's apply record does not hold it again (its goal is closed).
    assert_ne!(standing(&f, &first, &mut tx).1, "waits_for_accept");

    // A deleted goal with nothing planned: retired, and the plan is version 1 again.
    set_goal_status(&f, &open, hotsheet_model::Status::Deleted);
    pass(&f, false, &mut tx);
    let (holds, version, revision) = checked_in_holds(&f);
    assert!(holds.is_empty());
    assert_eq!(version, 1);
    let text =
        std::fs::read_to_string(std::path::Path::new(&f.m.owner.root).join(SHARED_PLAN_PATH))
            .unwrap();
    assert!(!text.contains("holds"), "{text}");
    // A repeated pass writes nothing.
    pass(&f, false, &mut tx);
    assert_eq!(checked_in_holds(&f).2, revision);

    // A goal this machine cannot find: on another machine (no apply record) it may not
    // have synced yet, so the hold stays; where it was opened, it was purged.
    let purged = apply("op-purge", "Exports", &mut tx);
    std::fs::remove_file(
        f.m.owner_store
            .ticket_path(&purged.native_id.parse().unwrap()),
    )
    .unwrap();
    let records =
        f.m.home
            .join(hotsheet_ticketing::orchestration::TEMPLATE_APPLY_DIR);
    let kept = f.m.home.join("applies-kept");
    std::fs::rename(&records, &kept).unwrap();
    pass(&f, false, &mut tx);
    assert_eq!(checked_in_holds(&f).0, ["op-purge"]);
    std::fs::rename(&kept, &records).unwrap();
    pass(&f, false, &mut tx);
    assert_eq!(checked_in_holds(&f).1, 1);

    // An apply recorded before holds were checked in (no checked-in hold): its record
    // holds the step until its goal closes.
    let legacy = apply("op-legacy", "Webhooks", &mut tx);
    let plan_path = std::path::Path::new(&f.m.owner.root).join(SHARED_PLAN_PATH);
    let mut on_disk: Value = serde_json::from_slice(&std::fs::read(&plan_path).unwrap()).unwrap();
    on_disk.as_object_mut().unwrap().remove("holds");
    on_disk["schema_version"] = json!(1);
    std::fs::write(&plan_path, serde_json::to_vec_pretty(&on_disk).unwrap()).unwrap();
    let legacy_step = plan_step(&f, legacy.slug.as_deref().unwrap(), "Send hooks", &mut tx);
    assert_eq!(standing(&f, &legacy_step, &mut tx).1, "waits_for_accept");
    set_goal_status(&f, &legacy, hotsheet_model::Status::Verified);
    assert_ne!(standing(&f, &legacy_step, &mut tx).1, "waits_for_accept");
}
