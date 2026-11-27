//! HS2-A4RVYA: plan templates. The validator's codes, typed input resolution, the
//! brief (placeholders substituted as text, `when` dropping a step, the person's
//! create choice), the checked-in file's rules and wire format, merging by template
//! id, the apply decision that makes a repeat a no-op, and the bounded apply records.
//! Synthetic templates, ids and homes only.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::*;

struct Clock;
impl crate::Clock for Clock {
    fn now_ms(&self) -> u64 {
        1_800_000_000_000
    }
}
struct Rng(u128);
impl crate::Rng for Rng {
    fn next_u128(&mut self) -> u128 {
        self.0 += 1;
        self.0
    }
}

fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}

/// The template the design mocks show: an API change, then the Web change on top of
/// its green CI, then an optional contract check.
fn feature() -> Template {
    serde_json::from_value(json!({
        "template_id": id(7),
        "name": "Feature across API and Web",
        "description": "API change, then the Web change on top of its green CI.",
        "version": 1,
        "inputs": [
            {"key": "feature", "label": "Feature", "kind": "text", "required": true},
            {"key": "priority", "label": "Priority", "kind": "priority", "default": "high"},
            {"key": "api", "label": "API Project", "kind": "target", "required": true},
            {"key": "web", "label": "Web Project", "kind": "target", "required": true},
            {"key": "contract", "label": "Run the contract check", "kind": "boolean", "default": true}
        ],
        "goal": {"title": "{{feature}}: API and Web", "tags": ["feature"]},
        "steps": [
            {"key": "api", "target": "{{api}}", "title": "Add {{feature}} headers", "priority": "{{priority}}", "tool": "claude"},
            {"key": "web", "target": "{{web}}", "title": "Show {{feature}} errors", "after": [{"step": "api", "gates": "ci"}]},
            {"key": "contract", "target": "{{api}}", "title": "Check the {{feature}} contract", "when": "{{contract}}", "after": [{"step": "api"}], "validation": ["contract-check"]},
            {"key": "docs", "target": "{{web}}", "title": "Document {{feature}}", "ticket": "create", "after": [{"step": "web"}, {"step": "contract", "gates": "none"}]}
        ],
        "instructions": "Keep {{feature}} behind a flag."
    }))
    .unwrap()
}

fn inputs(value: Value) -> BTreeMap<String, Value> {
    serde_json::from_value(value).unwrap()
}

fn codes(problems: &[TemplateProblem]) -> Vec<(String, String)> {
    problems
        .iter()
        .map(|problem| (problem.code.clone(), problem.at.clone()))
        .collect()
}

#[test]
fn hs2_a4rvya_a_valid_template_has_no_problems() {
    assert_eq!(validate_template(&feature()), []);
    assert_eq!(placeholders("{{ a }} and {{b}} {{"), ["a", "b"]);
    assert_eq!(reference(" {{api}} "), Some("api"));
    assert_eq!(reference("x {{api}}"), None);
    assert_eq!(reference("{{a}}{{b}}"), None);
}

type Change = Box<dyn Fn(&mut Template)>;

#[test]
fn hs2_a4rvya_the_validator_names_every_problem_and_where() {
    let cases: Vec<(&str, Change, &str, &str)> = vec![
        (
            "empty name",
            Box::new(|t| t.name = " ".into()),
            "name_invalid",
            "name",
        ),
        (
            "duplicate input",
            Box::new(|t| t.inputs[1].key = "feature".into()),
            "input_duplicate",
            "inputs[1].key",
        ),
        (
            "bad input key",
            Box::new(|t| t.inputs[0].key = "a b".into()),
            "input_key_invalid",
            "inputs[0].key",
        ),
        (
            "choice without choices",
            Box::new(|t| t.inputs[0].kind = InputKind::Choice),
            "input_choices_missing",
            "inputs[0].choices",
        ),
        (
            "default of the wrong type",
            Box::new(|t| t.inputs[1].default = Some(json!("urgent"))),
            "input_default_invalid",
            "inputs[1].default",
        ),
        (
            "unknown placeholder",
            Box::new(|t| t.goal.title = "{{nope}}".into()),
            "placeholder_unknown",
            "goal.title",
        ),
        (
            "no steps",
            Box::new(|t| t.steps.clear()),
            "steps_missing",
            "steps",
        ),
        (
            "duplicate step",
            Box::new(|t| t.steps[1].key = "api".into()),
            "step_duplicate",
            "steps[1].key",
        ),
        (
            "target naming a text input",
            Box::new(|t| t.steps[0].target = "{{feature}}".into()),
            "step_target_invalid",
            "steps[0].target",
        ),
        (
            "unknown after",
            Box::new(|t| t.steps[1].after[0].step = "gone".into()),
            "step_after_unknown",
            "steps[1].after[0]",
        ),
        (
            "after itself",
            Box::new(|t| t.steps[1].after[0].step = "web".into()),
            "step_after_self",
            "steps[1].after[0]",
        ),
        (
            "bad gates",
            Box::new(|t| t.steps[1].after[0].gates = Some("green".into())),
            "step_gates_invalid",
            "steps[1].after[0].gates",
        ),
        (
            "when on a text input",
            Box::new(|t| t.steps[2].when = Some("{{feature}}".into())),
            "step_when_invalid",
            "steps[2].when",
        ),
        (
            "bad priority",
            Box::new(|t| t.steps[1].priority = Some("urgent".into())),
            "step_priority_invalid",
            "steps[1].priority",
        ),
        (
            "multi-line title",
            Box::new(|t| t.steps[0].title = "a\nb".into()),
            "text_invalid",
            "steps[0].title",
        ),
    ];
    for (name, change, code, at) in cases {
        let mut template = feature();
        change(&mut template);
        let found = codes(&validate_template(&template));
        assert!(
            found.contains(&(code.into(), at.into())),
            "{name}: {found:?}"
        );
    }
    // A cycle is found whatever order the steps are in.
    let mut cyclic = feature();
    cyclic.steps[0].after = vec![StepAfter {
        step: "docs".into(),
        gates: None,
    }];
    assert!(
        validate_template(&cyclic)
            .iter()
            .any(|problem| problem.code == "step_cycle"),
        "{:?}",
        validate_template(&cyclic)
    );
    let mut many = feature();
    many.steps = (0..=MAX_TEMPLATE_STEPS)
        .map(|n| TemplateStep {
            key: format!("s{n}"),
            ..feature().steps[0].clone()
        })
        .collect();
    assert!(codes(&validate_template(&many)).contains(&("too_many_steps".into(), "steps".into())));
}

#[test]
fn hs2_a4rvya_inputs_resolve_by_type_with_defaults() {
    let template = feature();
    // Missing required inputs, an unknown key and a value of the wrong type.
    let (_, problems) = resolve_inputs(
        &template,
        &inputs(json!({"priority": "urgent", "colour": "red"})),
    );
    assert_eq!(
        codes(&problems),
        [
            ("input_invalid".into(), "inputs.priority".into()),
            ("input_missing".into(), "inputs.api".into()),
            ("input_missing".into(), "inputs.feature".into()),
            ("input_missing".into(), "inputs.web".into()),
            ("input_unknown".into(), "inputs.colour".into()),
        ]
    );
    // Defaults fill what is not given; text is trimmed.
    let (values, problems) = resolve_inputs(
        &template,
        &inputs(json!({"feature": "  Rate limits ", "api": "api-1", "web": "web-1"})),
    );
    assert_eq!(problems, []);
    assert_eq!(
        values,
        inputs(
            json!({"feature": "Rate limits", "priority": "high", "api": "api-1", "web": "web-1", "contract": true})
        )
    );
    // An empty string counts as not given; a placeholder in a value is refused, so a
    // value never adds a placeholder.
    let (_, problems) = resolve_inputs(
        &template,
        &inputs(json!({"feature": "", "api": "api-1", "web": "web-1"})),
    );
    assert_eq!(
        codes(&problems),
        [("input_missing".into(), "inputs.feature".into())]
    );
    let (_, problems) = resolve_inputs(
        &template,
        &inputs(json!({"feature": "{{api}}", "api": "api-1", "web": "web-1"})),
    );
    assert_eq!(
        codes(&problems),
        [("input_invalid".into(), "inputs.feature".into())]
    );
    // Each kind: one value that fits and one that does not.
    for (kind, good, bad) in [
        (InputKind::Number, json!(3.5), json!("3")),
        (InputKind::Boolean, json!(false), json!("no")),
        (InputKind::Priority, json!("lowest"), json!("p1")),
        (InputKind::Target, json!("web-1"), json!("a/b")),
        (InputKind::Tool, json!("codex"), json!(7)),
        (InputKind::Choice, json!("b"), json!("c")),
        (InputKind::Text, json!("ok"), json!(["x"])),
    ] {
        let input = TemplateInput {
            key: "k".into(),
            label: "K".into(),
            kind,
            required: true,
            default: None,
            choices: if kind == InputKind::Choice {
                vec!["a".into(), "b".into()]
            } else {
                Vec::new()
            },
            max_len: None,
            hint: None,
        };
        assert!(input_value(&input, &good).is_ok(), "{kind:?} {good}");
        assert!(input_value(&input, &bad).is_err(), "{kind:?} {bad}");
    }
    // An unset optional boolean reads as false.
    let mut optional = feature();
    optional.inputs[4].default = None;
    let (values, _) = resolve_inputs(
        &optional,
        &inputs(json!({"feature": "X", "api": "a", "web": "w"})),
    );
    assert_eq!(values["contract"], json!(false));
}

#[test]
fn hs2_a4rvya_the_brief_substitutes_inputs_as_text_and_follows_when_and_choices() {
    let template = feature();
    let (values, _) = resolve_inputs(
        &template,
        &inputs(json!({"feature": "Rate limits", "api": "api-1", "web": "web-1"})),
    );
    let names: BTreeMap<String, String> = [("api-1".to_owned(), "API".to_owned())].into();
    let brief = render_brief(&template, &values, &BTreeMap::new(), "op-1", &names);
    assert_eq!(brief.goal_title, "Rate limits: API and Web");
    assert_eq!(
        brief
            .steps
            .iter()
            .map(|step| (
                step.key.as_str(),
                step.target_id.as_str(),
                step.title.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            ("api", "api-1", "Add Rate limits headers"),
            ("web", "web-1", "Show Rate limits errors"),
            ("contract", "api-1", "Check the Rate limits contract"),
            ("docs", "web-1", "Document Rate limits"),
        ]
    );
    assert_eq!(brief.steps[0].priority.as_deref(), Some("high"));
    assert_eq!(brief.steps[0].tool.as_deref(), Some("claude"));
    assert_eq!(brief.steps[3].ticket, StepTicket::Create);
    assert_eq!(brief.targets(), ["api-1", "web-1"]);
    for part in [
        "**Feature across API and Web**",
        "operation `op-1`",
        "snapshot",
        "- API Project: API (`api-1`)",
        "- Web Project: `web-1`",
        "1. `api` in API (`api-1`): find or create \"Add Rate limits headers\" · priority high · preferred tool claude",
        "2. `web` in `web-1`: find or create \"Show Rate limits errors\" · after `api` (gates: ci)",
        "4. `docs` in `web-1`: create a new ticket \"Document Rate limits\"",
        "validate with the profile `contract-check`",
        "Keep Rate limits behind a flag.",
        "`tpl-op-1-<step key>`",
        "FEEDBACK NEEDED",
    ] {
        assert!(
            brief.goal_details.contains(part),
            "{part}\n{}",
            brief.goal_details
        );
    }
    // The same inputs give the same brief.
    assert_eq!(
        brief,
        render_brief(&template, &values, &BTreeMap::new(), "op-1", &names)
    );

    // A false boolean drops its step and every `after` that named it; the person's
    // choice switches a step to create.
    let mut values = values;
    values.insert("contract".into(), json!(false));
    let choices: BTreeMap<String, StepTicket> = [("web".to_owned(), StepTicket::Create)].into();
    let brief = render_brief(&template, &values, &choices, "op-2", &BTreeMap::new());
    assert_eq!(brief.skipped, ["contract"]);
    assert_eq!(
        brief
            .steps
            .iter()
            .map(|step| step.key.as_str())
            .collect::<Vec<_>>(),
        ["api", "web", "docs"]
    );
    assert_eq!(brief.steps[1].ticket, StepTicket::Create);
    assert_eq!(
        brief.steps[2]
            .after
            .iter()
            .map(|after| after.step.as_str())
            .collect::<Vec<_>>(),
        ["web"]
    );
    assert!(
        brief
            .goal_details
            .contains("Left out by the inputs: `contract`.")
    );
    // A value is text, never evaluated or re-read as a placeholder.
    assert_eq!(
        substitute("{{a}} {{b}} {{c", &inputs(json!({"a": "{{b}}", "b": 2}))),
        "{{b}} 2 {{c"
    );
}

#[test]
fn hs2_a4rvya_templates_merge_by_id_and_conflict_on_differences() {
    let a = feature();
    let mut b = feature();
    b.template_id = id(8);
    assert_eq!(
        merge_templates(std::slice::from_ref(&a), &[b.clone(), a.clone()]).unwrap(),
        [a.clone(), b.clone()]
    );
    let mut changed = a.clone();
    changed.name = "Changed".into();
    assert_eq!(
        merge_templates(&[a.clone(), b], &[changed]).unwrap_err(),
        [id(7)]
    );
    // Bookkeeping alone is not a different definition.
    let mut later = a.clone();
    later.version = 9;
    later.updated_by = Some("someone".into());
    assert!(a.same_definition(&later));
}

#[test]
fn hs2_a4rvya_the_checked_in_file_follows_the_plan_file_rules() {
    let dir = tempfile::tempdir().unwrap();
    let store = TemplateStore::new(dir.path());
    let path = dir.path().join(TEMPLATES_PATH);
    // A read never creates the file.
    assert_eq!(store.read().unwrap(), None);
    assert!(!path.exists());
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let mut second = feature();
    second.template_id = id(3);
    let lock = store.lock().unwrap();
    let written = store
        .write_locked(None, vec![feature(), second.clone()], &mut tx)
        .unwrap();
    drop(lock);
    // Ordered by template id, at schema version 1, with a revision.
    assert_eq!(written.templates[0].template_id, id(3));
    assert_eq!(store.read().unwrap().unwrap(), written);
    // The same templates again keep the revision.
    let again = store
        .write_locked(Some(&written), vec![second, feature()], &mut tx)
        .unwrap();
    assert_eq!(again.revision, written.revision);

    // Format compatibility: the wire shape is pinned.
    let on_disk: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(on_disk["schema_version"], 1);
    assert_eq!(
        on_disk["templates"][1]["inputs"][0],
        json!({"key":"feature","label":"Feature","kind":"text","required":true})
    );
    assert_eq!(on_disk["templates"][1]["steps"][3]["ticket"], "create");
    assert!(
        on_disk["templates"][1]["steps"][0].get("ticket").is_none(),
        "search is the default and is omitted"
    );
    assert_eq!(on_disk["templates"][1]["state"], "active");

    // A newer schema is refused, never overwritten.
    let mut newer = on_disk.clone();
    newer["schema_version"] = json!(2);
    std::fs::write(&path, serde_json::to_vec(&newer).unwrap()).unwrap();
    assert!(matches!(
        store.read(),
        Err(OrchestrationError::UnsupportedVersion { found: 2 })
    ));
    // Damaged content: merge markers, an unknown field, an invalid template, unsorted ids.
    let damaged: Vec<Vec<u8>> = vec![
        b"<<<<<<< ours\n{}\n=======\n{}\n>>>>>>> theirs\n".to_vec(),
        {
            let mut value = on_disk.clone();
            value["templates"][0]["extra"] = json!(1);
            serde_json::to_vec(&value).unwrap()
        },
        {
            let mut value = on_disk.clone();
            value["templates"][0]["steps"] = json!([]);
            serde_json::to_vec(&value).unwrap()
        },
        {
            let mut value = on_disk.clone();
            let templates = value["templates"].as_array_mut().unwrap();
            templates.reverse();
            serde_json::to_vec(&value).unwrap()
        },
    ];
    for bytes in damaged {
        std::fs::write(&path, bytes).unwrap();
        assert!(
            matches!(store.read(), Err(OrchestrationError::Corrupt)),
            "{:?}",
            store.read()
        );
    }
    // A file written by hand in the documented shape reads back.
    std::fs::write(
        &path,
        format!(
            r#"{{"schema_version":1,"revision":"{}","templates":[{{"template_id":"{}","name":"Repair failing CI","version":1,"goal":{{"title":"Repair CI"}},"steps":[{{"key":"fix","target":"api-1","title":"Repair the failing CI"}}]}}]}}"#,
            id(50),
            id(51)
        ),
    )
    .unwrap();
    let read = store.read().unwrap().unwrap();
    assert_eq!(read.templates[0].steps[0].ticket, StepTicket::Search);
    assert_eq!(read.templates[0].state, TemplateState::Active);
}

fn apply(state: ApplyState) -> TemplateApply {
    TemplateApply {
        operation_id: "op-1".into(),
        template_id: id(7),
        snapshot: feature(),
        inputs: inputs(
            json!({"feature": "Rate limits", "api": "api-1", "web": "web-1", "priority": "high", "contract": true}),
        ),
        choices: BTreeMap::new(),
        tool: "claude".into(),
        model: None,
        effort: None,
        actor_role: Some("human".into()),
        actor_id: Some("maintainer".into()),
        state,
        goal: None,
        proposal_id: None,
        planning: None,
        attempts: 0,
        permission_requests: Vec::new(),
        reason: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

#[test]
fn hs2_a4rvya_a_repeated_apply_is_answered_resumed_or_refused() {
    let supplied = inputs(json!({"feature": "Rate limits", "api": "api-1", "web": "web-1"}));
    let none = BTreeMap::new();
    assert_eq!(
        decide_apply(None, &id(7), &supplied, &none),
        ApplyDecision::New
    );
    for (state, expected) in [
        (ApplyState::Planning, ApplyDecision::Answer),
        (ApplyState::Denied, ApplyDecision::Answer),
        (ApplyState::WaitingPermission, ApplyDecision::Resume),
        (ApplyState::WaitingPlanner, ApplyDecision::Resume),
    ] {
        let known = apply(state);
        assert_eq!(
            decide_apply(Some(&known), &id(7), &supplied, &none),
            expected,
            "{state:?}"
        );
        // Defaults left out of a repeat are still the same apply.
        assert_eq!(
            decide_apply(
                Some(&known),
                &id(7),
                &inputs(
                    json!({"feature": "Rate limits", "api": "api-1", "web": "web-1", "priority": "", "contract": null})
                ),
                &none
            ),
            expected
        );
        // Another template, other inputs or other choices under the same id conflict.
        assert_eq!(
            decide_apply(Some(&known), &id(8), &supplied, &none),
            ApplyDecision::Conflict
        );
        assert_eq!(
            decide_apply(
                Some(&known),
                &id(7),
                &inputs(json!({"feature": "Other", "api": "api-1", "web": "web-1"})),
                &none
            ),
            ApplyDecision::Conflict
        );
        assert_eq!(
            decide_apply(
                Some(&known),
                &id(7),
                &supplied,
                &[("web".to_owned(), StepTicket::Create)].into()
            ),
            ApplyDecision::Conflict
        );
    }
}

#[test]
fn hs2_a4rvya_apply_records_are_private_bounded_and_keep_unfinished_ones() {
    let home = tempfile::tempdir().unwrap();
    let store = TemplateApplyStore::new(home.path(), "launch-0123456789ab").unwrap();
    assert!(TemplateApplyStore::new(home.path(), "../x").is_err());
    assert_eq!(store.find("op-1").unwrap(), None);
    let mut first = apply(ApplyState::WaitingPermission);
    store.write(&first).unwrap();
    first.state = ApplyState::Planning;
    store.write(&first).unwrap();
    assert_eq!(store.applies().unwrap(), [first.clone()]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file = home
            .path()
            .join(TEMPLATE_APPLY_DIR)
            .join("launch-0123456789ab.json");
        assert_eq!(
            std::fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // Past the bound the oldest finished apply goes first.
    for n in 0..MAX_TEMPLATE_APPLIES {
        let mut next = apply(ApplyState::Planning);
        next.operation_id = format!("op-fill-{n}");
        store.write(&next).unwrap();
    }
    let kept = store.applies().unwrap();
    assert_eq!(kept.len(), MAX_TEMPLATE_APPLIES);
    assert!(kept.iter().all(|known| known.operation_id != "op-1"));
    // A list full of unfinished applies refuses a new one rather than drop one.
    let full = TemplateApplyStore::new(home.path(), "other-0123456789ab").unwrap();
    for n in 0..MAX_TEMPLATE_APPLIES {
        let mut waiting = apply(ApplyState::WaitingPlanner);
        waiting.operation_id = format!("op-wait-{n}");
        full.write(&waiting).unwrap();
    }
    let mut one_more = apply(ApplyState::WaitingPlanner);
    one_more.operation_id = "op-more".into();
    assert!(matches!(
        full.write(&one_more),
        Err(OrchestrationError::Capacity)
    ));
}

#[test]
fn hs2_a4rvya_every_template_error_has_a_distinct_code() {
    let mut codes: Vec<&str> = TemplateError::ALL
        .iter()
        .map(|error| error.code())
        .collect();
    let all = codes.len();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), all);
    assert!(codes.iter().all(|code| code.starts_with("template_")));
}
