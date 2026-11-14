//! HS2-W27N1D: the project type and its switch decision table.
use serde_json::json;

use super::*;

fn state(current: ProjectType, declared: bool, active_projects: usize) -> TypeState {
    TypeState {
        current,
        declared,
        active_projects,
    }
}

#[test]
fn hs2_w27n1d_setting_reads_absent_as_software_and_refuses_unknown_values() {
    let cases: [(Option<serde_json::Value>, Result<ProjectType, &str>); 7] = [
        (None, Ok(ProjectType::Software)),
        (Some(json!(null)), Ok(ProjectType::Software)),
        (Some(json!("software")), Ok(ProjectType::Software)),
        (Some(json!("orchestration")), Ok(ProjectType::Orchestration)),
        (Some(json!("Orchestration")), Err("unknown project type")),
        (Some(json!("library")), Err("unknown project type")),
        (Some(json!(1)), Err("project type must be a string")),
    ];
    for (value, expected) in cases {
        let parsed = ProjectType::from_setting(value.as_ref());
        match (parsed, expected) {
            (Ok(found), Ok(wanted)) => assert_eq!(found, wanted, "{value:?}"),
            (Err(OrchestrationError::Invalid(found)), Err(wanted)) => {
                assert_eq!(found, wanted, "{value:?}")
            }
            (other, wanted) => panic!("{value:?}: {other:?} != {wanted:?}"),
        }
    }
    for kind in [ProjectType::Software, ProjectType::Orchestration] {
        assert_eq!(
            ProjectType::from_setting(Some(&kind.setting_value())).unwrap(),
            kind
        );
        assert_eq!(ProjectType::parse(kind.code()), Some(kind));
        assert_eq!(serde_json::to_value(kind).unwrap(), kind.setting_value());
    }
    assert_eq!(ProjectType::default(), ProjectType::Software);
}

/// Every (current, declared, projects) state crossed with every request and both
/// confirmations: the full transition matrix.
#[test]
fn hs2_w27n1d_type_switch_transition_matrix() {
    use ProjectType::{Orchestration as O, Software as S};
    // (current, declared, active, requested, keep) -> (write, create, retained) | refused
    type Row = (
        ProjectType,
        bool,
        usize,
        ProjectType,
        bool,
        Option<(bool, bool, usize)>,
    );
    let rows: [Row; 24] = [
        // Software, never orchestrated.
        (S, false, 0, S, false, Some((false, false, 0))),
        (S, false, 0, S, true, Some((false, false, 0))),
        (S, false, 0, O, false, Some((true, true, 0))),
        (S, false, 0, O, true, Some((true, true, 0))),
        // Software with an empty declaration left from before.
        (S, true, 0, S, false, Some((false, false, 0))),
        (S, true, 0, O, false, Some((true, false, 0))),
        // Software keeping projects from an earlier Orchestration period.
        (S, true, 3, S, false, Some((false, false, 3))),
        (S, true, 3, S, true, Some((false, false, 3))),
        (S, true, 3, O, false, Some((true, false, 0))),
        (S, true, 3, O, true, Some((true, false, 0))),
        // Orchestration whose declaration went missing: re-created, never refused.
        (O, false, 0, O, false, Some((false, true, 0))),
        (O, false, 0, S, false, Some((true, false, 0))),
        (O, false, 0, S, true, Some((true, false, 0))),
        (O, false, 0, O, true, Some((false, true, 0))),
        // Orchestration with no projects added yet.
        (O, true, 0, O, false, Some((false, false, 0))),
        (O, true, 0, S, false, Some((true, false, 0))),
        (O, true, 0, S, true, Some((true, false, 0))),
        (O, true, 0, O, true, Some((false, false, 0))),
        // Orchestration with projects added: going back needs the confirmation.
        (O, true, 1, S, false, None),
        (O, true, 1, S, true, Some((true, false, 1))),
        (O, true, 2, S, false, None),
        (O, true, 2, S, true, Some((true, false, 2))),
        (O, true, 2, O, false, Some((false, false, 0))),
        (O, true, 2, O, true, Some((false, false, 0))),
    ];
    for (current, declared, active, requested, keep, expected) in rows {
        let result = plan_type_change(state(current, declared, active), requested, keep);
        match expected {
            Some((write_setting, create_declaration, retained_projects)) => assert_eq!(
                result.unwrap(),
                TypeChange {
                    write_setting,
                    create_declaration,
                    retained_projects
                },
                "{current:?} declared={declared} active={active} -> {requested:?} keep={keep}"
            ),
            None => assert!(
                matches!(result, Err(OrchestrationError::Invalid(PROJECTS_RETAINED))),
                "{current:?} active={active} -> {requested:?} must be refused"
            ),
        }
    }
}

/// Apply a plan to a modelled project, as the runtime does, so sequences can be walked.
fn apply(
    project: &mut TypeState,
    requested: ProjectType,
    keep: bool,
) -> Result<TypeChange, OrchestrationError> {
    let change = plan_type_change(*project, requested, keep)?;
    if change.create_declaration {
        project.declared = true;
    }
    if change.write_setting {
        project.current = requested;
    }
    Ok(change)
}

#[test]
fn hs2_w27n1d_repeated_and_out_of_order_switches_converge_without_dropping_projects() {
    let mut project = state(ProjectType::Software, false, 0);
    // Repeated: the second identical request changes nothing.
    assert!(
        apply(&mut project, ProjectType::Orchestration, false)
            .unwrap()
            .write_setting
    );
    let again = apply(&mut project, ProjectType::Orchestration, false).unwrap();
    assert!(!again.write_setting && !again.create_declaration);
    // Projects are added while orchestrating.
    project.active_projects = 2;
    // Out of order: a back-switch without the confirmation is refused and changes nothing.
    assert!(apply(&mut project, ProjectType::Software, false).is_err());
    assert_eq!(project.current, ProjectType::Orchestration);
    // Confirmed: the type changes, the projects stay.
    let back = apply(&mut project, ProjectType::Software, true).unwrap();
    assert_eq!(back.retained_projects, 2);
    assert_eq!(project.active_projects, 2);
    // Repeating the confirmed back-switch is a no-op that still reports them.
    let repeat = apply(&mut project, ProjectType::Software, false).unwrap();
    assert_eq!((repeat.write_setting, repeat.retained_projects), (false, 2));
    // Switching to Orchestration again finds the same declaration; nothing is created.
    let forward = apply(&mut project, ProjectType::Orchestration, false).unwrap();
    assert!(forward.write_setting && !forward.create_declaration);
}

#[test]
fn hs2_w27n1d_interleaved_and_empty_then_refill_sequences() {
    let mut project = state(ProjectType::Software, false, 0);
    // Interleaved with project changes made elsewhere (another client, the CLI).
    apply(&mut project, ProjectType::Orchestration, false).unwrap();
    project.active_projects = 1;
    project.active_projects = 0; // removed again before the switch
    // Empty: no confirmation needed.
    assert_eq!(
        apply(&mut project, ProjectType::Software, false)
            .unwrap()
            .retained_projects,
        0
    );
    // Refill: projects added from a CLI while the type is Software are kept and
    // reported; switching to Orchestration then needs nothing more.
    project.active_projects = 3;
    let reported = apply(&mut project, ProjectType::Software, false).unwrap();
    assert_eq!(reported.retained_projects, 3);
    apply(&mut project, ProjectType::Orchestration, false).unwrap();
    // The declaration file was deleted by hand: the next request re-creates it.
    project.declared = false;
    project.active_projects = 0;
    assert!(
        apply(&mut project, ProjectType::Orchestration, false)
            .unwrap()
            .create_declaration
    );
    assert!(project.declared);
}

#[test]
fn hs2_w27n1d_set_request_is_strict() {
    let parsed: SetProjectType =
        serde_json::from_value(json!({"project_type":"orchestration"})).unwrap();
    assert_eq!(parsed.project_type, ProjectType::Orchestration);
    assert!(!parsed.keep_projects);
    for body in [
        json!({"project_type":"Software"}),
        json!({"project_type":"software","extra":true}),
        json!({}),
    ] {
        assert!(serde_json::from_value::<SetProjectType>(body).is_err());
    }
}

/// HS2-J3ZF27: the type in effect for every stored value with and without a
/// declaration. A stored value wins; absence reads as Orchestration only for a project
/// that declared targets before Project Type existed.
#[test]
fn hs2_j3zf27_effective_type_table() {
    use ProjectType::{Orchestration as O, Software as S};
    let cases: [(Option<serde_json::Value>, bool, Result<ProjectType, &str>); 10] = [
        (None, false, Ok(S)),
        (None, true, Ok(O)),
        (Some(json!(null)), false, Ok(S)),
        (Some(json!(null)), true, Ok(O)),
        (Some(json!("software")), false, Ok(S)),
        (Some(json!("software")), true, Ok(S)),
        (Some(json!("orchestration")), false, Ok(O)),
        (Some(json!("orchestration")), true, Ok(O)),
        (Some(json!("library")), true, Err("unknown project type")),
        (Some(json!(2)), true, Err("project type must be a string")),
    ];
    for (value, declared, expected) in cases {
        match (ProjectType::effective(value.as_ref(), declared), expected) {
            (Ok(found), Ok(wanted)) => {
                assert_eq!(found, wanted, "{value:?} declared={declared}");
                assert_eq!(found.orchestrates(), wanted == O);
            }
            (Err(OrchestrationError::Invalid(found)), Err(wanted)) => assert_eq!(found, wanted),
            (other, wanted) => panic!("{value:?} declared={declared}: {other:?} != {wanted:?}"),
        }
    }
}

/// HS2-J3ZF27: walking switches through the effective type: a legacy declared project
/// orchestrates, an explicit switch back to Software stops it, and switching forward
/// again resumes it, with nothing dropped on the way.
#[test]
fn hs2_j3zf27_switch_sequences_through_the_effective_type() {
    let read = |stored: Option<ProjectType>, declared: bool| {
        ProjectType::effective(stored.map(ProjectType::setting_value).as_ref(), declared).unwrap()
    };
    // Legacy: declared, nothing stored.
    let mut stored = None;
    let mut project = state(read(stored, true), true, 2);
    assert!(project.current.orchestrates());
    // Back to Software (confirmed): written explicitly, so it no longer orchestrates.
    let change = apply(&mut project, ProjectType::Software, true).unwrap();
    assert!(change.write_setting);
    stored = Some(project.current);
    assert!(!read(stored, true).orchestrates());
    // Repeated: still Software, nothing written.
    assert!(
        !apply(&mut project, ProjectType::Software, true)
            .unwrap()
            .write_setting
    );
    // Forward again: orchestrates with the same projects.
    apply(&mut project, ProjectType::Orchestration, false).unwrap();
    stored = Some(project.current);
    assert!(read(stored, true).orchestrates());
    assert_eq!(project.active_projects, 2);
    // A fresh project with nothing declared and nothing stored is Software.
    assert!(!read(None, false).orchestrates());
}
