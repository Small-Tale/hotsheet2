//! HS2-W27N1D: the project type and adding or removing projects, through the router
//! every adapter uses.
use hotsheet_ticketing::orchestration::{AddTarget, ProjectType};
use serde_json::{Value, json};

use super::*;

fn call(f: &Fixture, tx: &mut Transaction<'_>, method: &str, path: &str, body: Value) -> Value {
    f.runtime
        .request(method, path, &body, tx)
        .unwrap_or_else(|error| panic!("{method} {path}: {error}"))
}

fn refused(
    f: &Fixture,
    tx: &mut Transaction<'_>,
    method: &str,
    path: &str,
    body: Value,
) -> OrchestrationError {
    f.runtime
        .request(method, path, &body, tx)
        .expect_err("refused")
}

fn person() -> Value {
    json!({"role":"human","id":"synthetic-person"})
}

fn audit_actions(f: &Fixture) -> Vec<String> {
    f.runtime
        .audit(&f.owner.id, 0, 100)
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| entry.record.action)
        .collect()
}

fn local(f: &Fixture) -> Value {
    json!({"kind":"local","checkout_id":f.target.id})
}

#[test]
fn hs2_w27n1d_project_type_switches_create_the_declaration_once_and_keep_projects() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let path = format!("/checkouts/{}/orchestration/project-type", f.owner.id);
    let manifest = Path::new(&f.owner.root).join(".hotsheet2/orchestration.json");
    // Default: Software, nothing declared, and a read writes nothing.
    let shown = call(&f, &mut tx, "GET", &path, Value::Null);
    assert_eq!(shown["project_type"], "software");
    assert_eq!(shown["declared"], false);
    assert!(!manifest.exists());
    // First use as Orchestration creates the empty declaration and the setting.
    let set = call(
        &f,
        &mut tx,
        "PUT",
        &path,
        json!({"project_type":"orchestration","actor":person()}),
    );
    assert_eq!(
        (
            &set["project_type"],
            &set["declared"],
            &set["active_projects"],
            &set["changed"]
        ),
        (
            &json!("orchestration"),
            &json!(true),
            &json!(0),
            &json!(true)
        )
    );
    assert!(manifest.exists());
    let settings: Value = serde_json::from_slice(
        &std::fs::read(Path::new(&f.owner.root).join(".hotsheet2/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["project_type"], "orchestration");
    let manifest_bytes = std::fs::read(&manifest).unwrap();
    // Repeated: no change, no new audit entry, the same declaration bytes.
    let again = call(
        &f,
        &mut tx,
        "PUT",
        &path,
        json!({"project_type":"orchestration"}),
    );
    assert_eq!(again["changed"], false);
    assert_eq!(std::fs::read(&manifest).unwrap(), manifest_bytes);
    assert_eq!(audit_actions(&f), vec!["project_type_set".to_owned()]);
    // A project is added; going back needs the confirmation and drops nothing.
    call(
        &f,
        &mut tx,
        "POST",
        &format!("/checkouts/{}/orchestration/projects", f.owner.id),
        json!({"destination":local(&f),"label":"Synthetic target","actor":person()}),
    );
    let error = refused(
        &f,
        &mut tx,
        "PUT",
        &path,
        json!({"project_type":"software"}),
    );
    assert!(matches!(
        error,
        OrchestrationError::Invalid(hotsheet_ticketing::orchestration::PROJECTS_RETAINED)
    ));
    assert_eq!(
        call(&f, &mut tx, "GET", &path, Value::Null)["project_type"],
        "orchestration"
    );
    let back = call(
        &f,
        &mut tx,
        "PUT",
        &path,
        json!({"project_type":"software","keep_projects":true}),
    );
    assert_eq!(
        (&back["project_type"], &back["active_projects"]),
        (&json!("software"), &json!(1))
    );
    let bindings = f.runtime.bindings(&f.owner.id).unwrap();
    assert!(bindings[0].destination.is_some(), "the binding is kept");
    // Out of order: adding while Software is refused, and the project is still there
    // when the type comes back.
    assert!(matches!(
        refused(
            &f,
            &mut tx,
            "POST",
            &format!("/checkouts/{}/orchestration/projects", f.owner.id),
            json!({"destination":local(&f),"actor":person()}),
        ),
        OrchestrationError::Invalid(_)
    ));
    let forward = call(
        &f,
        &mut tx,
        "PUT",
        &path,
        json!({"project_type":"orchestration"}),
    );
    assert_eq!(forward["active_projects"], 1);
    // An unknown stored value is refused, never read as Software.
    f.runtime
        .owner(&f.owner.id)
        .map(|owner| {
            hotsheet_ticketing::Settings::for_project(&owner.root)
                .set(
                    hotsheet_ticketing::orchestration::PROJECT_TYPE_SETTING,
                    json!("library"),
                    hotsheet_ticketing::Scope::Shared,
                )
                .unwrap()
        })
        .unwrap();
    assert!(matches!(
        refused(&f, &mut tx, "GET", &path, Value::Null),
        OrchestrationError::Invalid("unknown project type")
    ));
}

fn orchestration(f: &Fixture, tx: &mut Transaction<'_>) {
    f.runtime
        .set_project_type(
            &f.owner.id,
            hotsheet_ticketing::orchestration::SetProjectType {
                project_type: ProjectType::Orchestration,
                keep_projects: false,
            },
            None,
            tx,
        )
        .unwrap();
}

#[test]
fn hs2_w27n1d_adding_a_project_declares_and_binds_in_one_idempotent_step() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    orchestration(&f, &mut tx);
    let path = format!("/checkouts/{}/orchestration/projects", f.owner.id);
    // Adding is a person's consent: an AI actor is refused before anything is written.
    assert!(matches!(
        refused(
            &f,
            &mut tx,
            "POST",
            &path,
            json!({"destination":local(&f),"actor":{"role":"ai","id":"synthetic-ai"}}),
        ),
        OrchestrationError::Forbidden(_)
    ));
    assert!(matches!(
        refused(
            &f,
            &mut tx,
            "POST",
            &path,
            json!({"destination":{"kind":"local","checkout_id":f.owner.id}}),
        ),
        OrchestrationError::Invalid("a project cannot orchestrate itself")
    ));
    assert!(
        f.runtime
            .declarations(&f.owner.id)
            .unwrap()
            .targets
            .is_empty()
    );
    let added = call(
        &f,
        &mut tx,
        "POST",
        &path,
        json!({"destination":local(&f),"label":"Synthetic target","actor":person()}),
    );
    assert_eq!(added["added"], true);
    assert_eq!(added["label"], "Synthetic target");
    assert_eq!(added["resolution"]["usable"], true);
    let target_id = added["target_id"].as_str().unwrap().to_owned();
    // Repeated: the same project, nothing declared twice.
    let repeated = call(
        &f,
        &mut tx,
        "POST",
        &path,
        json!({"destination":local(&f),"label":"Other label","actor":person()}),
    );
    assert_eq!(
        (&repeated["added"], repeated["target_id"].as_str()),
        (&json!(false), Some(target_id.as_str()))
    );
    assert_eq!(
        f.runtime.declarations(&f.owner.id).unwrap().targets.len(),
        1
    );
    // A remote project on a reviewed registration is added the same way.
    register(&f);
    let remote = call(
        &f,
        &mut tx,
        "POST",
        &path,
        json!({"destination":{"kind":"remote","registration_id":id(20),"checkout_id":f.peer.projection.checkout_id},"actor":person()}),
    );
    assert_eq!(remote["added"], true);
    assert_eq!(remote["label"], f.peer.projection.checkout_id.as_str());
    assert_eq!(
        remote["resolution"]["binding"]["destination"]["kind"],
        "remote"
    );
    assert_eq!(
        audit_actions(&f),
        vec!["project_type_set", "project_add", "project_add"]
    );
}

#[test]
fn hs2_w27n1d_a_retained_target_id_binds_the_declared_target_instead_of_another() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    orchestration(&f, &mut tx);
    // Empty, then refilled: the target was declared by an earlier attempt whose bind
    // never happened (as after a failed bind); the retry with its id binds it.
    let revision = f.runtime.declarations(&f.owner.id).unwrap().revision;
    f.runtime
        .add(
            &f.owner.id,
            AddTarget {
                expected_revision: revision,
                target_id: Some(id(7)),
                label: "Earlier attempt".into(),
                expected_repository_identity: None,
            },
            &mut tx,
        )
        .unwrap();
    let added = call(
        &f,
        &mut tx,
        "POST",
        &format!("/checkouts/{}/orchestration/projects", f.owner.id),
        json!({"destination":local(&f),"target_id":id(7),"actor":person()}),
    );
    assert_eq!(added["target_id"], id(7));
    assert_eq!(added["label"], "Earlier attempt");
    assert_eq!(added["resolution"]["usable"], true);
    assert_eq!(
        f.runtime.declarations(&f.owner.id).unwrap().targets.len(),
        1
    );
}

#[test]
fn hs2_w27n1d_removing_a_project_unbinds_and_tombstones_once() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    orchestration(&f, &mut tx);
    let added = call(
        &f,
        &mut tx,
        "POST",
        &format!("/checkouts/{}/orchestration/projects", f.owner.id),
        json!({"destination":local(&f),"actor":person()}),
    );
    let target_id = added["target_id"].as_str().unwrap().to_owned();
    let path = format!(
        "/checkouts/{}/orchestration/projects/{target_id}",
        f.owner.id
    );
    let removed = call(&f, &mut tx, "DELETE", &path, json!({"actor":person()}));
    assert_eq!(
        (&removed["unbound"], &removed["removed"]),
        (&json!(true), &json!(true))
    );
    let declarations = f.runtime.declarations(&f.owner.id).unwrap();
    assert_eq!(declarations.targets[0].state, DeclarationState::Removed);
    assert!(
        f.runtime.bindings(&f.owner.id).unwrap()[0]
            .destination
            .is_none()
    );
    // Repeated: nothing more to do and nothing more audited.
    let again = call(&f, &mut tx, "DELETE", &path, Value::Null);
    assert_eq!(
        (&again["unbound"], &again["removed"]),
        (&json!(false), &json!(false))
    );
    assert_eq!(
        audit_actions(&f),
        vec!["project_type_set", "project_add", "project_remove"]
    );
    // A removed target's id is never reused, even by a retry naming it.
    assert!(matches!(
        refused(
            &f,
            &mut tx,
            "POST",
            &format!("/checkouts/{}/orchestration/projects", f.owner.id),
            json!({"destination":local(&f),"target_id":target_id,"actor":person()}),
        ),
        OrchestrationError::Removed
    ));
    assert!(matches!(
        refused(
            &f,
            &mut tx,
            "DELETE",
            &format!(
                "/checkouts/{}/orchestration/projects/{}",
                f.owner.id,
                id(55)
            ),
            Value::Null,
        ),
        OrchestrationError::NotFound
    ));
    // Adding the same checkout again declares a fresh target.
    let fresh = call(
        &f,
        &mut tx,
        "POST",
        &format!("/checkouts/{}/orchestration/projects", f.owner.id),
        json!({"destination":local(&f),"actor":person()}),
    );
    assert_ne!(fresh["target_id"], target_id.as_str());
}

#[test]
fn hs2_w27n1d_project_paths_route_and_only_add_and_remove_are_machine_local() {
    for path in [
        "/checkouts/owner/orchestration/project-type",
        "/checkouts/owner/orchestration/projects",
        "/checkouts/owner/orchestration/projects/01ARZ3NDEKTSV4RRFFQ69G5FAV",
    ] {
        assert!(is_path(path), "{path}");
    }
    assert!(!is_machine_path(
        "/checkouts/owner/orchestration/project-type"
    ));
    assert!(is_machine_path("/checkouts/owner/orchestration/projects"));
    assert!(is_machine_path(
        "/checkouts/owner/orchestration/projects/01ARZ3NDEKTSV4RRFFQ69G5FAV"
    ));
}

/// HS2-SHHGC6: the settings fingerprint changes exactly when what Settings →
/// Orchestration shows changes: the type, the declaration, the policy and this
/// machine's bindings. Reads, repeats and the resolve every detail runs leave it.
#[test]
fn hs2_shhgc6_the_settings_fingerprint_follows_what_the_settings_show() {
    let f = fixture();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let print = || f.runtime.orchestration_fingerprint(&f.owner.id).unwrap();
    let base = format!("/checkouts/{}/orchestration", f.owner.id);
    let fresh = print();
    // Reads change nothing; repeating a read neither.
    call(
        &f,
        &mut tx,
        "GET",
        &format!("{base}/project-type"),
        Value::Null,
    );
    call(&f, &mut tx, "GET", &format!("{base}/policy"), Value::Null);
    assert_eq!(print(), fresh);
    // The type.
    orchestration(&f, &mut tx);
    let typed = print();
    assert_ne!(typed, fresh);
    orchestration(&f, &mut tx);
    assert_eq!(print(), typed, "a repeated switch changes nothing");
    // A project: declared and bound in one step.
    let added = call(
        &f,
        &mut tx,
        "POST",
        &format!("{base}/projects"),
        json!({"destination":local(&f),"label":"Synthetic target","actor":person()}),
    );
    let target = added["target_id"].as_str().unwrap().to_owned();
    let with_project = print();
    assert_ne!(with_project, typed);
    // The fresh check a detail runs renews times only.
    for _ in 0..2 {
        call(
            &f,
            &mut tx,
            "GET",
            &format!("{base}/targets/{target}/resolve"),
            Value::Null,
        );
        assert_eq!(print(), with_project);
    }
    // The policy.
    call(
        &f,
        &mut tx,
        "PUT",
        &format!("{base}/policy"),
        json!({"expected_revision":null,"policy":{"machine":{"max_sessions":3}},"actor":person()}),
    );
    let with_policy = print();
    assert_ne!(with_policy, with_project);
    // Removing the project unbinds and tombstones it.
    call(
        &f,
        &mut tx,
        "DELETE",
        &format!("{base}/projects/{target}"),
        json!({"actor":person()}),
    );
    assert_ne!(print(), with_policy);
    // The paths a host watches: the owner's .hotsheet2 and this machine's home.
    let paths = f.runtime.orchestration_watch_paths(&f.owner.id).unwrap();
    assert_eq!(paths.len(), 2);
    assert!(paths[0].ends_with(".hotsheet2"));
    assert!(
        f.runtime
            .orchestration_fingerprint("unknown-owner")
            .is_err()
    );
}
