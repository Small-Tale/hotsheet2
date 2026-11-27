//! HS2-A4RVYA: plan templates through the shared runtime, with a scripted terminal
//! host standing in for the planner (as the planning tests do) and real git ticket
//! stores. Applying opens one goal ticket with the brief and starts the planner once;
//! a repeat is a no-op; the applied work is a snapshot of the template; an AI apply
//! asks through the permission card first. Synthetic templates and tickets only.
use std::collections::BTreeMap;

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    ApplyGoal, ApplyState, AuditOutcome, PermissionAnswer, PlanUpdateState, StepTicket,
    TEMPLATE_GOAL_TAG, TEMPLATES_PATH, TargetAction, Template, TemplateApplyStore, TemplateState,
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
    let preview = |inputs: Value, choices: Value| {
        f.m.runtime
            .template_preview(
                &f.owner(),
                &serde_json::from_value(json!({"template_id": template.template_id,
                    "inputs": inputs, "choices": choices}))
                .unwrap(),
                Some(&human()),
            )
            .unwrap()
    };
    let missing = preview(json!({}), json!({}));
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
    let ok = preview(json!({"feature": "X"}), json!({"web": "create"}));
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
